use std::{collections::HashSet, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use async_imap::extensions::idle::IdleResponse;
use futures::TryStreamExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::{
    imap::{self, Conn},
    ops, parse,
};
use crate::{
    accounts::login_for,
    db::{
        self,
        store::{self, Account, Folder, NewMessage},
    },
    state::{AppState, Event, SyncCmd},
    trust::{self, blocklist},
};

/// Messages fetched per folder on first sync; older ones load on demand.
const INITIAL_WINDOW: usize = 500;
const OLDER_PAGE: usize = 200;
/// Bodies up to this size are fetched ahead of time.
const PREFETCH_MAX: u32 = 256 * 1024;
const HEADER_CHUNK: usize = 100;
const BODY_CHUNK: usize = 10;

pub fn start(state: &Arc<AppState>, account_id: i64) {
    let (tx, rx) = unbounded_channel();
    if let Some(old) = state.syncers.lock().unwrap().insert(account_id, tx) {
        old.send(SyncCmd::Stop).ok();
    }
    let state = state.clone();
    tokio::spawn(async move { run_account(state, account_id, rx).await });
}

pub fn start_all(state: &Arc<AppState>) {
    for account in state.with_db(store::accounts).unwrap_or_default() {
        start(state, account.id);
    }
}

pub fn stop(state: &AppState, account_id: i64) {
    if let Some(tx) = state.syncers.lock().unwrap().remove(&account_id) {
        tx.send(SyncCmd::Stop).ok();
    }
}

fn status(state: &AppState, account_id: i64, s: &str, detail: impl Into<String>) {
    state.emit(Event::SyncStatus {
        account_id,
        state: s.into(),
        detail: detail.into(),
    });
}

async fn run_account(state: Arc<AppState>, account_id: i64, mut rx: UnboundedReceiver<SyncCmd>) {
    let mut backoff = 15;
    loop {
        let Ok(Some(account)) = state.with_db(|c| store::account(c, account_id)) else {
            return;
        };
        match session(&state, &account, &mut rx).await {
            Ok(()) => return,
            Err(e) => {
                log::warn!("sync for account {account_id} stopped: {e:#}");
                status(&state, account_id, "error", format!("{e:#}"));
            }
        }
        // Wait before reconnecting, but wake at once if the user asks.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff)) => backoff = (backoff * 2).min(300),
            cmd = rx.recv() => match cmd {
                Some(SyncCmd::Stop) | None => return,
                Some(_) => backoff = 15,
            },
        }
    }
}

enum Wake {
    NewData,
    Timeout,
    Cmd(SyncCmd),
}

/// Sit in IDLE (or sleep, on servers without it) until something happens.
async fn wait(conn: Conn, rx: &mut UnboundedReceiver<SyncCmd>) -> Result<(Conn, Wake)> {
    if !conn.can_idle {
        let wake = tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => Wake::Timeout,
            cmd = rx.recv() => Wake::Cmd(cmd.unwrap_or(SyncCmd::Stop)),
        };
        return Ok((conn, wake));
    }
    let Conn {
        session,
        can_move,
        can_idle,
    } = conn;
    let mut idle = session.idle();
    idle.init().await?;
    let wake = {
        let (fut, _interrupt) = idle.wait_with_timeout(Duration::from_secs(9 * 60));
        tokio::pin!(fut);
        tokio::select! {
            r = &mut fut => match r? {
                IdleResponse::NewData(_) => Wake::NewData,
                _ => Wake::Timeout,
            },
            cmd = rx.recv() => Wake::Cmd(cmd.unwrap_or(SyncCmd::Stop)),
        }
    };
    let session = idle.done().await?;
    Ok((
        Conn {
            session,
            can_move,
            can_idle,
        },
        wake,
    ))
}

async fn session(
    state: &Arc<AppState>,
    account: &Account,
    rx: &mut UnboundedReceiver<SyncCmd>,
) -> Result<()> {
    status(state, account.id, "connecting", "");
    let login = login_for(state, account).await?;
    let mut conn = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &login,
    )
    .await?;

    status(state, account.id, "syncing", "");
    let folders = imap::list_folders(&mut conn.session).await?;
    state.with_db(|c| store::sync_folders(c, account.id, &folders))?;
    ops::run_pending(state, account, &mut conn).await?;
    full_pass(state, account, &mut conn, false).await?;

    loop {
        status(state, account.id, "idle", "");
        conn.session.select("INBOX").await?;
        let (back, wake) = wait(conn, rx).await?;
        conn = back;
        status(state, account.id, "syncing", "");
        match wake {
            Wake::Cmd(SyncCmd::Stop) => {
                conn.session.logout().await.ok();
                return Ok(());
            }
            Wake::Cmd(SyncCmd::LoadOlder(folder_id)) => {
                if let Some(folder) = state.with_db(|c| store::folder(c, folder_id))? {
                    load_older(state, account, &mut conn, &folder).await?;
                    backfill_bodies(state, account, &mut conn, &folder, 50).await?;
                }
            }
            Wake::NewData => {
                ops::run_pending(state, account, &mut conn).await?;
                if let Some(inbox) =
                    state.with_db(|c| store::folder_by_role(c, account.id, "inbox"))?
                {
                    sync_folder(state, account, &mut conn, &inbox, true).await?;
                    backfill_bodies(state, account, &mut conn, &inbox, 50).await?;
                }
            }
            Wake::Timeout | Wake::Cmd(SyncCmd::Poke) => {
                ops::run_pending(state, account, &mut conn).await?;
                full_pass(state, account, &mut conn, true).await?;
            }
        }
    }
}

async fn full_pass(
    state: &Arc<AppState>,
    account: &Account,
    conn: &mut Conn,
    notify: bool,
) -> Result<()> {
    let folders = state.with_db(|c| store::folders(c, Some(account.id)))?;
    for folder in &folders {
        sync_folder(state, account, conn, folder, notify).await?;
    }
    for folder in &folders {
        let budget = if folder.role == "inbox" { 200 } else { 40 };
        backfill_bodies(state, account, conn, folder, budget).await?;
    }
    Ok(())
}

async fn fetch_headers(
    state: &Arc<AppState>,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
    uids: &[u32],
) -> Result<Vec<(String, String)>> {
    let blocked = state.with_db(store::blocked)?;
    let junk = state.with_db(|c| store::folder_by_role(c, account.id, "junk"))?;
    let mut arrived = Vec::new();
    let mut to_junk = Vec::new();

    for chunk in uids.chunks(HEADER_CHUNK) {
        let fetched: Vec<_> = conn
            .session
            .uid_fetch(
                imap::uid_set(chunk),
                "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER])",
            )
            .await?
            .try_collect()
            .await?;
        state.with_db(|c| {
            let tx = c.unchecked_transaction()?;
            for f in &fetched {
                let (Some(uid), Some(header)) = (f.uid, f.header()) else {
                    continue;
                };
                let Some(msg) = parse::parse(header) else {
                    continue;
                };
                let envelope = parse::envelope(&msg);
                if folder.role == "inbox"
                    && junk.is_some()
                    && blocklist::is_blocked(&blocked, &envelope.from.addr)
                {
                    to_junk.push(uid);
                    continue;
                }
                let flags = imap::read_flags(f.flags());
                let new = NewMessage {
                    account_id: account.id,
                    folder_id: folder.id,
                    uid,
                    size: f.size.unwrap_or(0),
                    seen: flags.seen,
                    flagged: flags.flagged,
                    answered: flags.answered,
                    envelope: &envelope,
                };
                if store::insert_message(c, &new)?.is_some() && !flags.seen {
                    let who = if envelope.from.name.is_empty() {
                        envelope.from.addr.clone()
                    } else {
                        envelope.from.name.clone()
                    };
                    arrived.push((who, envelope.subject.clone()));
                }
            }
            tx.commit()?;
            Ok(())
        })?;
    }
    if let (false, Some(junk)) = (to_junk.is_empty(), junk) {
        imap::move_uids(conn, &to_junk, &junk.name).await?;
    }
    Ok(arrived)
}

/// Bring one folder up to date: removals, new arrivals and flag changes.
async fn sync_folder(
    state: &Arc<AppState>,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
    notify: bool,
) -> Result<()> {
    let mailbox = match conn.session.select(&folder.name).await {
        Ok(m) => m,
        // A folder that cannot be opened is skipped, not fatal.
        Err(async_imap::error::Error::No(_)) | Err(async_imap::error::Error::Bad(_)) => {
            return Ok(())
        }
        Err(e) => return Err(e.into()),
    };
    let validity = mailbox.uid_validity.map(i64::from);
    if folder.uidvalidity.is_some() && folder.uidvalidity != validity {
        // UIDs were renumbered; nothing we hold can be matched to the server.
        state.with_db(|c| store::clear_folder(c, folder.id))?;
    }
    if let Some(v) = validity {
        state.with_db(|c| store::set_uidvalidity(c, folder.id, v))?;
    }

    let server: HashSet<u32> = if mailbox.exists == 0 {
        HashSet::new()
    } else {
        conn.session.uid_search("ALL").await?
    };
    let local = state.with_db(|c| store::local_uids(c, folder.id))?;
    let gone: Vec<i64> = local
        .iter()
        .filter(|(_, uid)| !server.contains(uid))
        .map(|(id, _)| *id)
        .collect();
    let mut changed = !gone.is_empty();
    state.with_db(|c| store::delete_messages(c, &gone))?;

    let kept: Vec<u32> = local
        .iter()
        .map(|(_, uid)| *uid)
        .filter(|uid| server.contains(uid))
        .collect();
    let newest_local = kept.iter().max().copied();
    let mut wanted: Vec<u32> = match newest_local {
        Some(max) => server.iter().copied().filter(|uid| *uid > max).collect(),
        None => server.iter().copied().collect(),
    };
    wanted.sort_unstable_by(|a, b| b.cmp(a));
    if newest_local.is_none() {
        wanted.truncate(INITIAL_WINDOW);
    }

    let first_sync = newest_local.is_none();
    let mut arrived = Vec::new();
    if !wanted.is_empty() {
        arrived = fetch_headers(state, account, conn, folder, &wanted).await?;
        changed = true;
    }

    if let (Some(min), Some(max)) = (kept.iter().min(), kept.iter().max()) {
        let fetched: Vec<_> = conn
            .session
            .uid_fetch(format!("{min}:{max}"), "(UID FLAGS)")
            .await?
            .try_collect()
            .await?;
        state.with_db(|c| {
            let tx = c.unchecked_transaction()?;
            for f in &fetched {
                if let Some(uid) = f.uid {
                    let flags = imap::read_flags(f.flags());
                    changed |= store::set_flags(
                        c,
                        folder.id,
                        uid,
                        flags.seen,
                        flags.flagged,
                        flags.answered,
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })?;
    }

    if changed {
        state.emit(Event::MailChanged {
            account_id: account.id,
        });
    }
    if notify && !first_sync && folder.role == "inbox" {
        if let Some((from, subject)) = arrived.first().cloned() {
            state.emit(Event::NewMail {
                account_id: account.id,
                from,
                subject,
                count: arrived.len(),
            });
        }
    }
    Ok(())
}

async fn load_older(
    state: &Arc<AppState>,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
) -> Result<()> {
    let mailbox = conn.session.select(&folder.name).await?;
    if mailbox.exists == 0 {
        return Ok(());
    }
    let server = conn.session.uid_search("ALL").await?;
    let local = state.with_db(|c| store::local_uids(c, folder.id))?;
    let oldest = local.iter().map(|(_, uid)| *uid).min().unwrap_or(u32::MAX);
    let mut older: Vec<u32> = server.into_iter().filter(|uid| *uid < oldest).collect();
    older.sort_unstable_by(|a, b| b.cmp(a));
    older.truncate(OLDER_PAGE);
    if !older.is_empty() {
        fetch_headers(state, account, conn, folder, &older).await?;
        state.emit(Event::MailChanged {
            account_id: account.id,
        });
    }
    Ok(())
}

/// Store a freshly fetched body and everything derived from it.
pub fn process_body(state: &Arc<AppState>, account: &Account, id: i64, raw: &[u8]) -> Result<()> {
    let (preview, has_attachments, text) = match parse::parse(raw) {
        Some(msg) => {
            let text = parse::any_text(&msg);
            (
                parse::preview(&text),
                !parse::attachments(&msg).is_empty(),
                text,
            )
        }
        None => Default::default(),
    };
    state.with_db(|c| store::store_body(c, id, raw, &preview, has_attachments, &text))?;
    state.emit(Event::MessageUpdated { id });
    spawn_verify(state.clone(), account.clone(), id, raw.to_vec());
    Ok(())
}

/// Check authenticity off the sync path; DNS can be slow.
pub fn spawn_verify(state: Arc<AppState>, account: Account, id: i64, raw: Vec<u8>) {
    tokio::spawn(async move {
        match state.with_db(|c| store::message(c, id)) {
            Ok(Some(row)) if !matches!(row.folder_role.as_str(), "sent" | "drafts") => {}
            _ => return,
        }
        let Ok(_permit) = state.verify_slots.clone().acquire_owned().await else {
            return;
        };
        let Some(resolver) = state.resolver() else {
            return;
        };
        let report = trust::auth::verify(&resolver, &raw, &account.imap_host).await;
        let failed = report.verdict == trust::auth::Verdict::Failed;
        let Ok(detail) = serde_json::to_string(&report) else {
            return;
        };
        if state
            .with_db(|c| store::set_auth(c, id, report.verdict.as_str(), &detail))
            .is_err()
        {
            return;
        }
        state.emit(Event::MessageUpdated { id });

        // Optional: forged mail goes straight to Junk.
        let auto = state
            .with_db(|c| db::get_setting(c, "auto_junk_failed"))
            .ok()
            .flatten()
            .as_deref()
            == Some("1");
        if failed && auto {
            if let Ok(Some(row)) = state.with_db(|c| store::message(c, id)) {
                if row.folder_role == "inbox" {
                    let _ = crate::actions::move_messages(&state, &[id], "junk");
                }
            }
        }
    });
}

async fn backfill_bodies(
    state: &Arc<AppState>,
    account: &Account,
    conn: &mut Conn,
    folder: &Folder,
    budget: u32,
) -> Result<()> {
    let missing = state.with_db(|c| store::missing_bodies(c, folder.id, PREFETCH_MAX, budget))?;
    if missing.is_empty() {
        return Ok(());
    }
    conn.session.select(&folder.name).await?;
    for chunk in missing.chunks(BODY_CHUNK) {
        let uids: Vec<u32> = chunk.iter().map(|(_, uid)| *uid).collect();
        let fetched: Vec<_> = conn
            .session
            .uid_fetch(imap::uid_set(&uids), "(UID BODY.PEEK[])")
            .await?
            .try_collect()
            .await?;
        for f in &fetched {
            let (Some(uid), Some(body)) = (f.uid, f.body()) else {
                continue;
            };
            if let Some((id, _)) = chunk.iter().find(|(_, u)| *u == uid) {
                process_body(state, account, *id, body)?;
            }
        }
    }
    state.emit(Event::MailChanged {
        account_id: account.id,
    });
    Ok(())
}

/// Fetch one body right now, on its own connection, because the user opened it.
pub async fn fetch_body_now(state: &Arc<AppState>, message_id: i64) -> Result<Vec<u8>> {
    if let Some(raw) = state.with_db(|c| store::body(c, message_id))? {
        return Ok(raw);
    }
    let row = state
        .with_db(|c| store::message(c, message_id))?
        .ok_or_else(|| anyhow!("message not found"))?;
    let account = state
        .with_db(|c| store::account(c, row.account_id))?
        .ok_or_else(|| anyhow!("account not found"))?;
    let folder = state
        .with_db(|c| store::folder(c, row.folder_id))?
        .ok_or_else(|| anyhow!("folder not found"))?;
    let login = login_for(state, &account).await?;
    let mut conn = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &login,
    )
    .await?;
    conn.session.examine(&folder.name).await?;
    let fetched: Vec<_> = conn
        .session
        .uid_fetch(row.uid.to_string(), "(UID BODY.PEEK[])")
        .await?
        .try_collect()
        .await?;
    conn.session.logout().await.ok();
    let raw = fetched
        .iter()
        .find_map(|f| f.body().map(<[u8]>::to_vec))
        .ok_or_else(|| anyhow!("the server no longer has this message"))?;
    process_body(state, &account, message_id, &raw)?;
    Ok(raw)
}
