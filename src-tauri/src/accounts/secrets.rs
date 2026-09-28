use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::{
    key::{self, KeyMode},
    store,
};

fn wallet_name(email: &str) -> String {
    format!("account:{email}")
}

pub fn set(
    conn: &Connection,
    mode: KeyMode,
    account_id: i64,
    email: &str,
    secret: &str,
) -> Result<()> {
    match mode {
        KeyMode::Wallet => key::wallet_secret_set(&wallet_name(email), secret),
        KeyMode::Passphrase => {
            conn.execute(
                "UPDATE accounts SET secret = ?2 WHERE id = ?1",
                params![account_id, secret],
            )?;
            Ok(())
        }
    }
}

pub fn get(conn: &Connection, mode: KeyMode, account_id: i64, email: &str) -> Result<String> {
    let found = match mode {
        KeyMode::Wallet => key::wallet_secret_get(&wallet_name(email))?,
        KeyMode::Passphrase => conn
            .query_row(
                "SELECT secret FROM accounts WHERE id = ?1",
                [account_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten(),
    };
    found.ok_or_else(|| anyhow!("no saved sign-in for {email}; add the account again"))
}

pub fn delete(mode: KeyMode, email: &str) -> Result<()> {
    match mode {
        KeyMode::Wallet => key::wallet_secret_delete(&wallet_name(email)),
        KeyMode::Passphrase => Ok(()),
    }
}

/// Move every account secret from one home to the other.
pub fn migrate(conn: &Connection, from: KeyMode, to: KeyMode) -> Result<()> {
    for account in store::accounts(conn)? {
        let Ok(secret) = get(conn, from, account.id, &account.email) else {
            continue;
        };
        set(conn, to, account.id, &account.email, &secret)?;
        match from {
            KeyMode::Wallet => key::wallet_secret_delete(&wallet_name(&account.email))?,
            KeyMode::Passphrase => {
                conn.execute(
                    "UPDATE accounts SET secret = NULL WHERE id = ?1",
                    [account.id],
                )?;
            }
        }
    }
    Ok(())
}
