use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::now;
use crate::mail::parse::{Contact, Envelope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_starttls: bool,
    pub username: String,
    pub auth_kind: String,
    pub oauth_provider: Option<String>,
}

const ACCOUNT_COLS: &str = "id, email, display_name, imap_host, imap_port, smtp_host, smtp_port, \
                            smtp_starttls, username, auth_kind, oauth_provider";

fn account_row(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        email: r.get(1)?,
        display_name: r.get(2)?,
        imap_host: r.get(3)?,
        imap_port: r.get(4)?,
        smtp_host: r.get(5)?,
        smtp_port: r.get(6)?,
        smtp_starttls: r.get(7)?,
        username: r.get(8)?,
        auth_kind: r.get(9)?,
        oauth_provider: r.get(10)?,
    })
}

pub fn accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(&format!("SELECT {ACCOUNT_COLS} FROM accounts ORDER BY id"))?;
    let rows = stmt
        .query_map([], account_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn account(conn: &Connection, id: i64) -> Result<Option<Account>> {
    Ok(conn
        .query_row(
            &format!("SELECT {ACCOUNT_COLS} FROM accounts WHERE id = ?1"),
            [id],
            account_row,
        )
        .optional()?)
}

pub fn insert_account(conn: &Connection, a: &Account) -> Result<i64> {
    conn.execute(
        "INSERT INTO accounts (email, display_name, imap_host, imap_port, smtp_host, smtp_port,
                               smtp_starttls, username, auth_kind, oauth_provider, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            a.email,
            a.display_name,
            a.imap_host,
            a.imap_port,
            a.smtp_host,
            a.smtp_port,
            a.smtp_starttls,
            a.username,
            a.auth_kind,
            a.oauth_provider,
            now()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_account(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM messages_fts WHERE rowid IN (SELECT id FROM messages WHERE account_id = ?1)",
        [id],
    )?;
    conn.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    pub role: String,
    pub uidvalidity: Option<i64>,
}

fn folder_row(r: &Row) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        account_id: r.get(1)?,
        name: r.get(2)?,
        role: r.get(3)?,
        uidvalidity: r.get(4)?,
    })
}

pub fn folders(conn: &Connection, account_id: Option<i64>) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, role, uidvalidity FROM folders
         WHERE ?1 IS NULL OR account_id = ?1
         ORDER BY account_id,
                  CASE role WHEN 'inbox' THEN 0 WHEN 'sent' THEN 1 WHEN 'drafts' THEN 2
                            WHEN 'archive' THEN 3 WHEN 'junk' THEN 4 WHEN 'trash' THEN 5 ELSE 6 END,
                  name COLLATE NOCASE",
    )?;
    let rows = stmt
        .query_map([account_id], folder_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn folder(conn: &Connection, id: i64) -> Result<Option<Folder>> {
    Ok(conn
        .query_row(
            "SELECT id, account_id, name, role, uidvalidity FROM folders WHERE id = ?1",
            [id],
            folder_row,
        )
        .optional()?)
}

pub fn folder_by_role(conn: &Connection, account_id: i64, role: &str) -> Result<Option<Folder>> {
    Ok(conn
        .query_row(
            "SELECT id, account_id, name, role, uidvalidity FROM folders
             WHERE account_id = ?1 AND role = ?2 ORDER BY id LIMIT 1",
            params![account_id, role],
            folder_row,
        )
        .optional()?)
}

/// Make the stored folder list match the server's.
pub fn sync_folders(conn: &Connection, account_id: i64, server: &[(String, String)]) -> Result<()> {
    for (name, role) in server {
        conn.execute(
            "INSERT INTO folders (account_id, name, role) VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id, name) DO UPDATE SET role = excluded.role",
            params![account_id, name, role],
        )?;
    }
    let names: Vec<&str> = server.iter().map(|(n, _)| n.as_str()).collect();
    for f in folders(conn, Some(account_id))? {
        if !names.contains(&f.name.as_str()) {
            clear_folder(conn, f.id)?;
            conn.execute("DELETE FROM folders WHERE id = ?1", [f.id])?;
        }
    }
    Ok(())
}

pub fn clear_folder(conn: &Connection, folder_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM messages_fts WHERE rowid IN (SELECT id FROM messages WHERE folder_id = ?1)",
        [folder_id],
    )?;
    conn.execute("DELETE FROM messages WHERE folder_id = ?1", [folder_id])?;
    Ok(())
}

pub fn set_uidvalidity(conn: &Connection, folder_id: i64, value: i64) -> Result<()> {
    conn.execute(
        "UPDATE folders SET uidvalidity = ?2 WHERE id = ?1",
        params![folder_id, value],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageRow {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub folder_role: String,
    pub uid: u32,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub has_attachments: bool,
    pub preview: String,
    pub auth_verdict: Option<String>,
}

const ROW_COLS: &str = "m.id, m.account_id, m.folder_id, f.role, m.uid, m.subject, m.from_name, \
                        m.from_addr, m.date, m.seen, m.flagged, m.answered, m.has_attachments, \
                        m.preview, m.auth_verdict";

fn message_row(r: &Row) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        folder_id: r.get(2)?,
        folder_role: r.get(3)?,
        uid: r.get(4)?,
        subject: r.get(5)?,
        from_name: r.get(6)?,
        from_addr: r.get(7)?,
        date: r.get(8)?,
        seen: r.get(9)?,
        flagged: r.get(10)?,
        answered: r.get(11)?,
        has_attachments: r.get(12)?,
        preview: r.get(13)?,
        auth_verdict: r.get(14)?,
    })
}

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    pub account_id: Option<i64>,
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Turn free text into an FTS5 query: every word must match, as a prefix.
pub fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '@' && c != '.' && c != '-' && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

pub fn list_messages(conn: &Connection, q: &ListQuery) -> Result<Vec<MessageRow>> {
    let role = q.role.clone().unwrap_or_else(|| "inbox".into());
    let limit = q.limit.unwrap_or(200).min(1000);
    let fts = q.search.as_deref().and_then(fts_query);
    let sql = format!(
        "SELECT {ROW_COLS} FROM messages m JOIN folders f ON f.id = m.folder_id
         WHERE (?1 IS NULL OR m.account_id = ?1)
           AND (CASE WHEN ?2 IS NOT NULL THEN m.folder_id = ?2
                     WHEN ?5 IS NOT NULL THEN f.role NOT IN ('junk', 'trash')
                     ELSE f.role = ?3 END)
           AND (?5 IS NULL OR m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?5))
         ORDER BY m.date DESC, m.id DESC LIMIT ?4 OFFSET ?6"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(
            params![q.account_id, q.folder_id, role, limit, fts, q.offset],
            message_row,
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn message(conn: &Connection, id: i64) -> Result<Option<MessageRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {ROW_COLS} FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?1"),
            [id],
            message_row,
        )
        .optional()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageExtra {
    pub to: Vec<Contact>,
    pub cc: Vec<Contact>,
    pub reply_to: Option<String>,
    pub message_id: Option<String>,
    pub references: Option<String>,
    pub auth_detail: Option<serde_json::Value>,
    pub size: i64,
}

pub fn message_extra(conn: &Connection, id: i64) -> Result<Option<MessageExtra>> {
    Ok(conn
        .query_row(
            "SELECT to_json, cc_json, reply_to, message_id, refs, auth_detail, size FROM messages WHERE id = ?1",
            [id],
            |r| {
                let to: String = r.get(0)?;
                let cc: String = r.get(1)?;
                let detail: Option<String> = r.get(5)?;
                Ok(MessageExtra {
                    to: serde_json::from_str(&to).unwrap_or_default(),
                    cc: serde_json::from_str(&cc).unwrap_or_default(),
                    reply_to: r.get(2)?,
                    message_id: r.get(3)?,
                    references: r.get(4)?,
                    auth_detail: detail.and_then(|d| serde_json::from_str(&d).ok()),
                    size: r.get(6)?,
                })
            },
        )
        .optional()?)
}

pub struct NewMessage<'a> {
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub size: u32,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub envelope: &'a Envelope,
}

pub fn insert_message(conn: &Connection, m: &NewMessage) -> Result<Option<i64>> {
    let e = m.envelope;
    let changed = conn.execute(
        "INSERT OR IGNORE INTO messages
            (account_id, folder_id, uid, message_id, in_reply_to, refs, subject, from_name, from_addr,
             to_json, cc_json, reply_to, date, size, seen, flagged, answered)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            m.account_id, m.folder_id, m.uid, e.message_id, e.in_reply_to, e.references, e.subject,
            e.from.name, e.from.addr, serde_json::to_string(&e.to)?, serde_json::to_string(&e.cc)?,
            e.reply_to, e.date, m.size, m.seen, m.flagged, m.answered
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO messages_fts (rowid, subject, sender, body) VALUES (?1, ?2, ?3, '')",
        params![id, e.subject, format!("{} {}", e.from.name, e.from.addr)],
    )?;
    Ok(Some(id))
}

pub fn delete_messages(conn: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        conn.execute("DELETE FROM messages_fts WHERE rowid = ?1", [id])?;
        conn.execute("DELETE FROM messages WHERE id = ?1", [id])?;
    }
    Ok(())
}

pub fn local_uids(conn: &Connection, folder_id: i64) -> Result<Vec<(i64, u32)>> {
    let mut stmt = conn.prepare("SELECT id, uid FROM messages WHERE folder_id = ?1")?;
    let rows = stmt
        .query_map([folder_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn set_flags(
    conn: &Connection,
    folder_id: i64,
    uid: u32,
    seen: bool,
    flagged: bool,
    answered: bool,
) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE messages SET seen = ?3, flagged = ?4, answered = ?5
         WHERE folder_id = ?1 AND uid = ?2 AND (seen != ?3 OR flagged != ?4 OR answered != ?5)",
        params![folder_id, uid, seen, flagged, answered],
    )? > 0)
}

pub fn store_body(
    conn: &Connection,
    id: i64,
    raw: &[u8],
    preview: &str,
    has_attachments: bool,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO bodies (message_id, raw) VALUES (?1, ?2)
         ON CONFLICT (message_id) DO UPDATE SET raw = excluded.raw",
        params![id, raw],
    )?;
    conn.execute(
        "UPDATE messages SET preview = ?2, has_attachments = ?3 WHERE id = ?1",
        params![id, preview, has_attachments],
    )?;
    // Cap what goes in the index; nobody searches for the tail of a 2 MB newsletter.
    let indexed: String = text.chars().take(20_000).collect();
    conn.execute(
        "UPDATE messages_fts SET body = ?2 WHERE rowid = ?1",
        params![id, indexed],
    )?;
    Ok(())
}

pub fn body(conn: &Connection, id: i64) -> Result<Option<Vec<u8>>> {
    Ok(conn
        .query_row("SELECT raw FROM bodies WHERE message_id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Messages still waiting for their body, newest first: (id, uid).
pub fn missing_bodies(
    conn: &Connection,
    folder_id: i64,
    max_size: u32,
    limit: u32,
) -> Result<Vec<(i64, u32)>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.uid FROM messages m
         WHERE m.folder_id = ?1 AND m.size <= ?2
           AND NOT EXISTS (SELECT 1 FROM bodies b WHERE b.message_id = m.id)
         ORDER BY m.date DESC LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![folder_id, max_size, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn set_auth(conn: &Connection, id: i64, verdict: &str, detail: &str) -> Result<()> {
    conn.execute(
        "UPDATE messages SET auth_verdict = ?2, auth_detail = ?3 WHERE id = ?1",
        params![id, verdict, detail],
    )?;
    Ok(())
}

pub fn unread_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM messages m JOIN folders f ON f.id = m.folder_id WHERE f.role = 'inbox' AND m.seen = 0",
        [],
        |r| r.get(0),
    )?)
}

/// Has this address written to us before `message_id`?
pub fn seen_sender_before(conn: &Connection, addr: &str, message_id: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM messages WHERE from_addr = ?1 AND id != ?2
                        AND date < (SELECT date FROM messages WHERE id = ?2))",
        params![addr, message_id],
        |r| r.get(0),
    )?)
}

pub fn blocked(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT pattern FROM blocked ORDER BY pattern")?;
    let rows = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn block(conn: &Connection, pattern: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO blocked (pattern, created_at) VALUES (?1, ?2)",
        params![pattern, now()],
    )?;
    Ok(())
}

pub fn unblock(conn: &Connection, pattern: &str) -> Result<()> {
    conn.execute("DELETE FROM blocked WHERE pattern = ?1", [pattern])?;
    Ok(())
}

pub fn images_trusted(conn: &Connection, addr: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM trusted_image_senders WHERE addr = ?1)",
        [addr],
        |r| r.get(0),
    )?)
}

pub fn trust_images(conn: &Connection, addr: &str, trusted: bool) -> Result<()> {
    if trusted {
        conn.execute(
            "INSERT OR IGNORE INTO trusted_image_senders (addr) VALUES (?1)",
            [addr],
        )?;
    } else {
        conn.execute("DELETE FROM trusted_image_senders WHERE addr = ?1", [addr])?;
    }
    Ok(())
}

pub fn push_op(conn: &Connection, account_id: i64, payload: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pending_ops (account_id, payload, created_at) VALUES (?1, ?2, ?3)",
        params![account_id, payload, now()],
    )?;
    Ok(())
}

pub fn pending_ops(conn: &Connection, account_id: i64) -> Result<Vec<(i64, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT id, payload, attempts FROM pending_ops WHERE account_id = ?1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn finish_op(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM pending_ops WHERE id = ?1", [id])?;
    Ok(())
}

pub fn fail_op(conn: &Connection, id: i64, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE pending_ops SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
        params![id, error],
    )?;
    Ok(())
}

pub fn cached_blob(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &str,
) -> Result<Option<(Option<String>, Option<Vec<u8>>, i64)>> {
    Ok(conn
        .query_row(
            &format!("SELECT mime, data, fetched_at FROM {table} WHERE {key_col} = ?1"),
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

pub fn cache_blob(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &str,
    mime: Option<&str>,
    data: Option<&[u8]>,
) -> Result<()> {
    conn.execute(
        &format!("INSERT OR REPLACE INTO {table} ({key_col}, mime, data, fetched_at) VALUES (?1, ?2, ?3, ?4)"),
        params![key, mime, data, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{key, open};
    use zeroize::Zeroizing;

    pub fn test_db() -> (Connection, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sobre-store-{}",
            key::to_hex(&key::random_bytes::<6>())
        ));
        let conn = open(&dir, &Zeroizing::new(key::random_bytes::<32>())).unwrap();
        (conn, dir)
    }

    fn sample_account() -> Account {
        Account {
            id: 0,
            email: "me@example.org".into(),
            display_name: "Me".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            smtp_starttls: false,
            username: "me@example.org".into(),
            auth_kind: "password".into(),
            oauth_provider: None,
        }
    }

    #[test]
    fn messages_round_trip_search_and_cascade() {
        let (conn, dir) = test_db();
        let acc = insert_account(&conn, &sample_account()).unwrap();
        sync_folders(
            &conn,
            acc,
            &[
                ("INBOX".into(), "inbox".into()),
                ("Spam".into(), "junk".into()),
            ],
        )
        .unwrap();
        let inbox = folder_by_role(&conn, acc, "inbox").unwrap().unwrap();
        let junk = folder_by_role(&conn, acc, "junk").unwrap().unwrap();

        let mut env = Envelope {
            subject: "Quarterly invoice".into(),
            date: 100,
            ..Default::default()
        };
        env.from = Contact {
            name: "Ana Ruiz".into(),
            addr: "ana@example.com".into(),
        };
        fn new(acc: i64, folder_id: i64, uid: u32, envelope: &Envelope) -> NewMessage<'_> {
            NewMessage {
                account_id: acc,
                folder_id,
                uid,
                size: 10,
                seen: false,
                flagged: false,
                answered: false,
                envelope,
            }
        }
        let id = insert_message(&conn, &new(acc, inbox.id, 1, &env))
            .unwrap()
            .unwrap();
        assert!(insert_message(&conn, &new(acc, inbox.id, 1, &env))
            .unwrap()
            .is_none());
        env.subject = "Cheap pills".into();
        env.date = 200;
        insert_message(&conn, &new(acc, junk.id, 1, &env))
            .unwrap()
            .unwrap();

        store_body(
            &conn,
            id,
            b"raw",
            "preview text",
            true,
            "the pineapple shipment arrives monday",
        )
        .unwrap();
        assert_eq!(body(&conn, id).unwrap().unwrap(), b"raw");
        assert_eq!(unread_count(&conn).unwrap(), 1);

        let inbox_rows = list_messages(&conn, &ListQuery::default()).unwrap();
        assert_eq!(inbox_rows.len(), 1);
        assert!(inbox_rows[0].has_attachments);

        let find = |s: &str| {
            list_messages(
                &conn,
                &ListQuery {
                    search: Some(s.into()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        assert_eq!(find("pineap").len(), 1);
        assert_eq!(find("ana invoice").len(), 1);
        // Junk is not searched unless that folder is open.
        assert_eq!(find("pills").len(), 0);
        assert_eq!(
            list_messages(
                &conn,
                &ListQuery {
                    search: Some("pills".into()),
                    folder_id: Some(junk.id),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(find("\"); DROP TABLE messages; --").len(), 0);

        assert!(set_flags(&conn, inbox.id, 1, true, false, false).unwrap());
        assert!(!set_flags(&conn, inbox.id, 1, true, false, false).unwrap());
        assert_eq!(unread_count(&conn).unwrap(), 0);

        delete_account(&conn, acc).unwrap();
        let left: i64 = conn.query_row("SELECT (SELECT count(*) FROM messages) + (SELECT count(*) FROM bodies) + (SELECT count(*) FROM messages_fts)", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        std::fs::remove_dir_all(dir).ok();
    }
}
