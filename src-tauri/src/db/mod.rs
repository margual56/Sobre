pub mod key;
pub mod store;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};

use key::DbKey;

const DB_FILE: &str = "mail.db";
const SCHEMA: &str = include_str!("schema.sql");

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_FILE)
}

fn apply_key(conn: &Connection, pragma: &str, key: &DbKey) -> Result<()> {
    let literal = zeroize::Zeroizing::new(format!("x'{}'", key::to_hex(&key[..])));
    conn.pragma_update(None, pragma, literal.as_str())?;
    Ok(())
}

/// Open (or create) the database with `key`. A wrong key is an error.
pub fn open(data_dir: &Path, key: &DbKey) -> Result<Connection> {
    std::fs::create_dir_all(data_dir)?;
    let conn = Connection::open(db_path(data_dir)).context("opening the database file")?;
    apply_key(&conn, "key", key)?;
    // The first real read is what proves the key.
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .is_err()
    {
        bail!("wrong key or passphrase");
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "secure_delete", "ON")?;
    // Temporary tables and sort spills must never reach the disk in the clear.
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.execute_batch(SCHEMA).context("creating the schema")?;
    conn.execute_batch(
        "UPDATE messages SET auth_verdict = NULL, auth_detail = NULL
           WHERE auth_verdict IS NOT NULL
             AND folder_id IN (SELECT id FROM folders WHERE role IN ('sent', 'drafts'));
         UPDATE messages SET auth_verdict = NULL, auth_detail = NULL
           WHERE auth_verdict = 'failed' AND auth_detail LIKE '%requires its mail to be signed%';",
    )?;
    Ok(conn)
}

pub fn rekey(conn: &Connection, new_key: &DbKey) -> Result<()> {
    apply_key(conn, "rekey", new_key)
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn get_setting(conn: &Connection, name: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [name], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, name: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [name, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroizing;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sobre-test-{tag}-{}",
            key::to_hex(&key::random_bytes::<6>())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn file_is_encrypted_and_wrong_key_fails() {
        let dir = temp_dir("enc");
        let key: DbKey = Zeroizing::new(key::random_bytes::<32>());
        let marker = "VERY-RECOGNISABLE-SUBJECT-LINE";
        {
            let conn = open(&dir, &key).unwrap();
            set_setting(&conn, "marker", marker).unwrap();
            conn.pragma_update(None, "wal_checkpoint", "TRUNCATE").ok();
        }
        for entry in std::fs::read_dir(&dir).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!bytes.starts_with(b"SQLite format 3"));
            assert!(!contains(&bytes, marker.as_bytes()));
        }
        let wrong: DbKey = Zeroizing::new(key::random_bytes::<32>());
        assert!(open(&dir, &wrong).is_err());
        let conn = open(&dir, &key).unwrap();
        assert_eq!(
            get_setting(&conn, "marker").unwrap().as_deref(),
            Some(marker)
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rekey_keeps_data() {
        let dir = temp_dir("rekey");
        let old: DbKey = Zeroizing::new(key::random_bytes::<32>());
        let new = key::derive_key("a passphrase", b"0123456789abcdef").unwrap();
        {
            let conn = open(&dir, &old).unwrap();
            set_setting(&conn, "k", "v").unwrap();
            rekey(&conn, &new).unwrap();
        }
        assert!(open(&dir, &old).is_err());
        let conn = open(&dir, &new).unwrap();
        assert_eq!(get_setting(&conn, "k").unwrap().as_deref(), Some("v"));
        std::fs::remove_dir_all(dir).ok();
    }
}
