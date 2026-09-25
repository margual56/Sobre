use std::{fs, path::Path};

use anyhow::{anyhow, bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const KEYRING_SERVICE: &str = "sobre";
const KEYRING_DB_USER: &str = "database-key";
const CONFIG_FILE: &str = "config.json";

pub type DbKey = Zeroizing<[u8; 32]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyMode {
    Wallet,
    Passphrase,
}

/// The only file stored outside the encrypted database. It holds nothing secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyConfig {
    pub mode: KeyMode,
    /// Hex Argon2 salt, present in passphrase mode.
    #[serde(default)]
    pub salt: Option<String>,
}

impl KeyConfig {
    pub fn load(data_dir: &Path) -> Result<Option<Self>> {
        let path = data_dir.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).context("reading config.json")?;
        Ok(Some(
            serde_json::from_str(&raw).context("parsing config.json")?,
        ))
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        fs::create_dir_all(data_dir)?;
        fs::write(data_dir.join(CONFIG_FILE), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand::fill(&mut buf[..]);
    buf
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        bail!("odd hex length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| anyhow!(e)))
        .collect()
}

fn wallet_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_DB_USER).context("opening the system wallet")
}

/// Fetch the wallet key, creating it on first use.
pub fn wallet_key(create: bool) -> Result<DbKey> {
    let entry = wallet_entry()?;
    match entry.get_password() {
        Ok(hex) => {
            let hex = Zeroizing::new(hex);
            let bytes = Zeroizing::new(from_hex(&hex)?);
            let key: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("wallet key has the wrong length"))?;
            Ok(Zeroizing::new(key))
        }
        Err(keyring::Error::NoEntry) if create => {
            let key = Zeroizing::new(random_bytes::<32>());
            let hex = Zeroizing::new(to_hex(&key[..]));
            entry
                .set_password(&hex)
                .context("storing the database key in the wallet")?;
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => bail!("the database key is missing from the system wallet"),
        Err(e) => Err(anyhow!(e).context("reading the database key from the wallet")),
    }
}

pub fn delete_wallet_key() -> Result<()> {
    match wallet_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!(e)),
    }
}

pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<DbKey> {
    // 64 MiB, 3 passes: about half a second on a desktop CPU.
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| anyhow!("{e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key[..])
        .map_err(|e| anyhow!("{e}"))?;
    Ok(key)
}

/// Account secrets live in the wallet in wallet mode.
pub fn wallet_secret_get(account_key: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, account_key)?;
    match entry.get_password() {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!(e)),
    }
}

pub fn wallet_secret_set(account_key: &str, secret: &str) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, account_key)?.set_password(secret)?;
    Ok(())
}

pub fn wallet_secret_delete(account_key: &str) -> Result<()> {
    match keyring::Entry::new(KEYRING_SERVICE, account_key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes = random_bytes::<32>();
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn derive_is_deterministic_and_salted() {
        let a = derive_key("correct horse", b"0123456789abcdef").unwrap();
        let b = derive_key("correct horse", b"0123456789abcdef").unwrap();
        let c = derive_key("correct horse", b"fedcba9876543210").unwrap();
        assert_eq!(a[..], b[..]);
        assert_ne!(a[..], c[..]);
    }
}
