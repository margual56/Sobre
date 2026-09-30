use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};

use super::imap::{self, Conn};
use crate::{
    db::store::{self, Account},
    state::AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Flag {
        folder: String,
        uids: Vec<u32>,
        flag: String,
        on: bool,
    },
    Move {
        folder: String,
        uids: Vec<u32>,
        to_role: String,
    },
    Expunge {
        folder: String,
        uids: Vec<u32>,
    },
    Append {
        role: String,
        raw: String,
    },
}

pub fn enqueue(state: &AppState, account_id: i64, op: &Op) -> Result<()> {
    state.with_db(|conn| store::push_op(conn, account_id, &serde_json::to_string(op)?))?;
    state.poke(account_id);
    Ok(())
}

pub fn append_op(role: &str, raw: &[u8]) -> Op {
    Op::Append {
        role: role.into(),
        raw: STANDARD.encode(raw),
    }
}

fn imap_flag(flag: &str) -> &'static str {
    match flag {
        "flagged" => "\\Flagged",
        "answered" => "\\Answered",
        _ => "\\Seen",
    }
}

async fn apply(
    state: &AppState,
    account: &Account,
    conn: &mut Conn,
    op: &Op,
) -> Result<(), async_imap::error::Error> {
    let role_folder = |role: &str| {
        state
            .with_db(|c| store::folder_by_role(c, account.id, role))
            .ok()
            .flatten()
            .map(|f| f.name)
            .ok_or_else(|| {
                async_imap::error::Error::No(format!("this account has no {role} folder"))
            })
    };
    match op {
        Op::Flag {
            folder,
            uids,
            flag,
            on,
        } => {
            conn.session.select(folder).await?;
            let sign = if *on { '+' } else { '-' };
            conn.session
                .uid_store(
                    imap::uid_set(uids),
                    format!("{sign}FLAGS.SILENT ({})", imap_flag(flag)),
                )
                .await?
                .try_collect::<Vec<_>>()
                .await?;
        }
        Op::Move {
            folder,
            uids,
            to_role,
        } => {
            let target = role_folder(to_role)?;
            conn.session.select(folder).await?;
            imap::move_uids(conn, uids, &target)
                .await
                .map_err(to_imap_error)?;
        }
        Op::Expunge { folder, uids } => {
            conn.session.select(folder).await?;
            imap::delete_uids(conn, uids).await.map_err(to_imap_error)?;
        }
        Op::Append { role, raw } => {
            let target = role_folder(role)?;
            let bytes = STANDARD
                .decode(raw)
                .map_err(|e| async_imap::error::Error::No(e.to_string()))?;
            conn.session
                .append(&target, Some("(\\Seen)"), None, bytes)
                .await?;
        }
    }
    Ok(())
}

fn to_imap_error(e: anyhow::Error) -> async_imap::error::Error {
    match e.downcast::<async_imap::error::Error>() {
        Ok(e) => e,
        Err(other) => async_imap::error::Error::No(other.to_string()),
    }
}

pub async fn run_pending(state: &AppState, account: &Account, conn: &mut Conn) -> Result<bool> {
    let pending = state.with_db(|c| store::pending_ops(c, account.id))?;
    let ran = !pending.is_empty();
    for (id, payload, attempts) in pending {
        let Ok(op) = serde_json::from_str::<Op>(&payload) else {
            state.with_db(|c| store::finish_op(c, id))?;
            continue;
        };
        match apply(state, account, conn, &op).await {
            Ok(()) => state.with_db(|c| store::finish_op(c, id))?,
            Err(async_imap::error::Error::No(why)) | Err(async_imap::error::Error::Bad(why)) => {
                log::warn!("server refused a queued change: {why}");
                state.with_db(|c| store::finish_op(c, id))?;
            }
            Err(e) => {
                state.with_db(|c| {
                    if attempts >= 5 {
                        store::finish_op(c, id)
                    } else {
                        store::fail_op(c, id, &e.to_string())
                    }
                })?;
                return Err(e.into());
            }
        }
    }
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_survive_serialisation() {
        let ops = [
            Op::Flag {
                folder: "INBOX".into(),
                uids: vec![1, 2],
                flag: "seen".into(),
                on: true,
            },
            Op::Move {
                folder: "INBOX".into(),
                uids: vec![7],
                to_role: "junk".into(),
            },
            Op::Expunge {
                folder: "Trash".into(),
                uids: vec![9],
            },
            append_op("sent", b"Subject: x\r\n\r\nbody"),
        ];
        for op in ops {
            let json = serde_json::to_string(&op).unwrap();
            assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
        }
    }
}
