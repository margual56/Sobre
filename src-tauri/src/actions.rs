use std::collections::BTreeMap;

use anyhow::{bail, Result};

use crate::{
    db::store::{self, MessageRow},
    mail::ops::{self, Op},
    state::{AppState, Event},
    trust::blocklist,
};

/// Group messages by (account, folder name) so each group becomes one command.
fn grouped(state: &AppState, ids: &[i64]) -> Result<BTreeMap<(i64, String), Vec<MessageRow>>> {
    let mut groups: BTreeMap<(i64, String), Vec<MessageRow>> = BTreeMap::new();
    state.with_db(|c| {
        for id in ids {
            if let Some(row) = store::message(c, *id)? {
                if let Some(folder) = store::folder(c, row.folder_id)? {
                    groups
                        .entry((row.account_id, folder.name))
                        .or_default()
                        .push(row);
                }
            }
        }
        Ok(())
    })?;
    Ok(groups)
}

fn changed(state: &AppState, accounts: impl IntoIterator<Item = i64>) {
    let mut seen = Vec::new();
    for account_id in accounts {
        if !seen.contains(&account_id) {
            seen.push(account_id);
            state.emit(Event::MailChanged { account_id });
        }
    }
}

pub fn set_flag(state: &AppState, ids: &[i64], flag: &str, on: bool) -> Result<()> {
    if !matches!(flag, "seen" | "flagged" | "answered") {
        bail!("unknown flag {flag}");
    }
    let groups = grouped(state, ids)?;
    for ((account_id, folder), rows) in &groups {
        state.with_db(|c| {
            for r in rows {
                let (seen, flagged, answered) = match flag {
                    "seen" => (on, r.flagged, r.answered),
                    "flagged" => (r.seen, on, r.answered),
                    _ => (r.seen, r.flagged, on),
                };
                store::set_flags(c, r.folder_id, r.uid, seen, flagged, answered)?;
            }
            Ok(())
        })?;
        let uids = rows.iter().map(|r| r.uid).collect();
        ops::enqueue(
            state,
            *account_id,
            &Op::Flag {
                folder: folder.clone(),
                uids,
                flag: flag.into(),
                on,
            },
        )?;
    }
    changed(state, groups.keys().map(|(a, _)| *a));
    Ok(())
}

pub fn move_messages(state: &AppState, ids: &[i64], role: &str) -> Result<()> {
    if !matches!(role, "trash" | "junk" | "archive" | "inbox") {
        bail!("cannot move to {role}");
    }
    let groups = grouped(state, ids)?;
    for ((account_id, folder), rows) in &groups {
        let has_target = state
            .with_db(|c| store::folder_by_role(c, *account_id, role))?
            .is_some();
        let source_role = rows[0].folder_role.as_str();
        let uids: Vec<u32> = rows.iter().map(|r| r.uid).collect();
        let op = if source_role == role && role == "trash" {
            Op::Expunge {
                folder: folder.clone(),
                uids,
            }
        } else if source_role == role {
            continue;
        } else if !has_target {
            bail!("this account has no {role} folder");
        } else {
            Op::Move {
                folder: folder.clone(),
                uids,
                to_role: role.into(),
            }
        };
        // The rows reappear under the target folder on the next sync.
        let local: Vec<i64> = rows.iter().map(|r| r.id).collect();
        state.with_db(|c| store::delete_messages(c, &local))?;
        ops::enqueue(state, *account_id, &op)?;
    }
    changed(state, groups.keys().map(|(a, _)| *a));
    Ok(())
}

/// Block an address or domain and sweep its mail out of every inbox.
pub fn block_sender(state: &AppState, pattern: &str) -> Result<String> {
    let Some(pattern) = blocklist::normalize(pattern) else {
        bail!("not an address or domain")
    };
    let own = state.with_db(|c| {
        Ok(store::accounts(c)?
            .into_iter()
            .map(|a| a.email)
            .collect::<Vec<_>>())
    })?;
    if own
        .iter()
        .any(|email| blocklist::is_blocked(&[pattern.as_str()], email))
    {
        bail!("that would block your own address");
    }
    let ids = state.with_db(|c| {
        store::block(c, &pattern)?;
        let mut stmt = c.prepare(
            "SELECT m.id, m.from_addr FROM messages m JOIN folders f ON f.id = m.folder_id WHERE f.role = 'inbox'",
        )?;
        let rows: Vec<(i64, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows.into_iter().filter(|(_, addr)| blocklist::is_blocked(&[pattern.as_str()], addr)).map(|(id, _)| id).collect::<Vec<_>>())
    })?;
    // Accounts without a Junk folder keep the mail; the block still applies to new arrivals where possible.
    move_messages(state, &ids, "junk").ok();
    Ok(pattern)
}
