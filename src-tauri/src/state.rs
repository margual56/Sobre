use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{anyhow, bail, Result};
use mail_auth::MessageAuthenticator;
use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::{mpsc::UnboundedSender, Semaphore};
use zeroize::Zeroizing;

use crate::db::{
    self,
    key::{self, KeyConfig, KeyMode},
};

/// What the backend tells the UI (and the tray) about.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    MailChanged {
        account_id: i64,
    },
    MessageUpdated {
        id: i64,
    },
    NewMail {
        account_id: i64,
        from: String,
        subject: String,
        count: usize,
    },
    SyncStatus {
        account_id: i64,
        state: String,
        detail: String,
    },
    Sent,
    ThemeChanged,
    Locked,
}

pub enum SyncCmd {
    Poke,
    LoadOlder(i64),
    Stop,
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

pub struct AppState {
    pub data_dir: PathBuf,
    db: Mutex<Option<Connection>>,
    pub key_mode: Mutex<Option<KeyMode>>,
    pub token: String,
    pub images_allowed: Mutex<HashSet<i64>>,
    pub syncers: Mutex<HashMap<i64, UnboundedSender<SyncCmd>>>,
    pub access_tokens: tokio::sync::Mutex<HashMap<i64, (String, Instant)>>,
    pub http: reqwest::Client,
    resolver: Mutex<Option<Arc<MessageAuthenticator>>>,
    pub verify_slots: Arc<Semaphore>,
    pub temp_files: Mutex<Vec<PathBuf>>,
    pub tray_problem: Mutex<Option<String>>,
    pub compose_drafts: Mutex<HashMap<u32, serde_json::Value>>,
    sink: Mutex<Option<EventSink>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            data_dir,
            db: Mutex::new(None),
            key_mode: Mutex::new(None),
            token: key::to_hex(&key::random_bytes::<24>()),
            images_allowed: Mutex::default(),
            syncers: Mutex::default(),
            access_tokens: Default::default(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            resolver: Mutex::new(None),
            verify_slots: Arc::new(Semaphore::new(4)),
            temp_files: Mutex::default(),
            compose_drafts: Mutex::default(),
            tray_problem: Mutex::default(),
            sink: Mutex::new(None),
        })
    }

    pub fn set_sink(&self, sink: EventSink) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    pub fn emit(&self, event: Event) {
        let sink = self.sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(event);
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.db.lock().unwrap().is_some()
    }

    /// Run `f` against the open database.
    pub fn with_db<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.db.lock().unwrap();
        let conn = guard
            .as_ref()
            .ok_or_else(|| anyhow!("the mail store is locked"))?;
        f(conn)
    }

    pub fn mode(&self) -> Option<KeyMode> {
        *self.key_mode.lock().unwrap()
    }

    fn install(&self, conn: Connection, mode: KeyMode) {
        *self.db.lock().unwrap() = Some(conn);
        *self.key_mode.lock().unwrap() = Some(mode);
    }

    /// First run: create the store with a wallet-held key.
    pub fn create_with_wallet(&self) -> Result<()> {
        let key = key::wallet_key(true)?;
        let conn = db::open(&self.data_dir, &key)?;
        KeyConfig {
            mode: KeyMode::Wallet,
            salt: None,
        }
        .save(&self.data_dir)?;
        self.install(conn, KeyMode::Wallet);
        Ok(())
    }

    /// First run: create the store under a passphrase.
    pub fn create_with_passphrase(&self, passphrase: &str) -> Result<()> {
        check_passphrase(passphrase)?;
        let salt = key::random_bytes::<16>();
        let key = key::derive_key(passphrase, &salt)?;
        let conn = db::open(&self.data_dir, &key)?;
        KeyConfig {
            mode: KeyMode::Passphrase,
            salt: Some(key::to_hex(&salt)),
        }
        .save(&self.data_dir)?;
        self.install(conn, KeyMode::Passphrase);
        Ok(())
    }

    /// Open an existing store. Wallet mode needs no input.
    pub fn unlock(&self, passphrase: Option<&str>) -> Result<()> {
        let config =
            KeyConfig::load(&self.data_dir)?.ok_or_else(|| anyhow!("no mail store yet"))?;
        let key = match config.mode {
            KeyMode::Wallet => key::wallet_key(false)?,
            KeyMode::Passphrase => {
                let salt = key::from_hex(
                    config
                        .salt
                        .as_deref()
                        .ok_or_else(|| anyhow!("config.json has no salt"))?,
                )?;
                key::derive_key(
                    passphrase.ok_or_else(|| anyhow!("a passphrase is required"))?,
                    &salt,
                )?
            }
        };
        let conn = db::open(&self.data_dir, &key)?;
        self.install(conn, config.mode);
        Ok(())
    }

    /// Drop the key and every decrypted thing held in memory.
    pub fn lock(&self) {
        for (_, tx) in self.syncers.lock().unwrap().drain() {
            tx.send(SyncCmd::Stop).ok();
        }
        *self.db.lock().unwrap() = None;
        self.images_allowed.lock().unwrap().clear();
        self.compose_drafts.lock().unwrap().clear();
        if let Ok(mut tokens) = self.access_tokens.try_lock() {
            tokens.clear();
        }
        self.remove_temp_files();
        self.emit(Event::Locked);
    }

    pub fn change_key_mode(&self, target: KeyMode, passphrase: Option<&str>) -> Result<()> {
        let current = self
            .mode()
            .ok_or_else(|| anyhow!("the mail store is locked"))?;
        let (new_key, config): (Zeroizing<[u8; 32]>, KeyConfig) = match target {
            KeyMode::Wallet => {
                key::delete_wallet_key()?;
                (
                    key::wallet_key(true)?,
                    KeyConfig {
                        mode: KeyMode::Wallet,
                        salt: None,
                    },
                )
            }
            KeyMode::Passphrase => {
                let passphrase = passphrase.ok_or_else(|| anyhow!("a passphrase is required"))?;
                check_passphrase(passphrase)?;
                let salt = key::random_bytes::<16>();
                (
                    key::derive_key(passphrase, &salt)?,
                    KeyConfig {
                        mode: KeyMode::Passphrase,
                        salt: Some(key::to_hex(&salt)),
                    },
                )
            }
        };
        self.with_db(|conn| {
            if current != target {
                crate::accounts::secrets::migrate(conn, current, target)?;
            }
            db::rekey(conn, &new_key)
        })?;
        config.save(&self.data_dir)?;
        if target == KeyMode::Passphrase {
            key::delete_wallet_key().ok();
        }
        *self.key_mode.lock().unwrap() = Some(target);
        Ok(())
    }

    /// DNS resolver for DKIM keys and DMARC records, built on first use.
    pub fn resolver(&self) -> Option<Arc<MessageAuthenticator>> {
        let mut guard = self.resolver.lock().unwrap();
        if guard.is_none() {
            *guard = MessageAuthenticator::new_system_conf()
                .or_else(|_| MessageAuthenticator::new_cloudflare_tls())
                .ok()
                .map(Arc::new);
        }
        guard.clone()
    }

    pub fn poke(&self, account_id: i64) {
        if let Some(tx) = self.syncers.lock().unwrap().get(&account_id) {
            tx.send(SyncCmd::Poke).ok();
        }
    }

    pub fn remove_temp_files(&self) {
        for path in self.temp_files.lock().unwrap().drain(..) {
            std::fs::remove_file(path).ok();
        }
    }
}

fn check_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < 8 {
        bail!("use a passphrase of at least 8 characters");
    }
    Ok(())
}
