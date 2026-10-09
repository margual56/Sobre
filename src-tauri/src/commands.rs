use std::{io::Write, os::unix::fs::OpenOptionsExt, sync::Arc};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::{
    accounts::{self, discovery::ServerConfig, oauth, secrets, Login},
    actions,
    db::{
        self,
        key::{self, KeyConfig, KeyMode},
        store::{self, Account, Folder, ListQuery, MessageRow},
    },
    icons,
    mail::{imap, ops, parse, smtp, sync},
    render::{protocol, sanitize::ORIGIN, RenderKind, View},
    state::{AppState, SyncCmd},
    trust::auth::AuthReport,
};

type Shared<'a> = State<'a, Arc<AppState>>;
type Cmd<T> = std::result::Result<T, String>;

fn fail<T>(r: Result<T>) -> Cmd<T> {
    r.map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
pub struct Status {
    stage: &'static str,
    key_mode: Option<KeyMode>,
    has_tray: bool,
    demo: bool,
    /// `missing-library`, or the error the tray gave.
    tray_problem: Option<String>,
}

#[tauri::command]
pub fn app_status(state: Shared) -> Cmd<Status> {
    let config = fail(KeyConfig::load(&state.data_dir))?;
    let stage = match (&config, state.is_unlocked()) {
        _ if state.is_demo() => "ready",
        (None, _) => "new",
        (Some(_), true) => "ready",
        (Some(_), false) => "locked",
    };
    // Read once: taking this lock twice in one expression deadlocks.
    let tray_problem = state.tray_problem.lock().unwrap().clone();
    Ok(Status {
        stage,
        key_mode: config.map(|c| c.mode),
        has_tray: tray_problem.is_none(),
        demo: state.is_demo(),
        tray_problem,
    })
}

/// First run: `passphrase` chooses passphrase mode, `None` the system wallet.
#[tauri::command]
pub async fn create_store(state: Shared<'_>, passphrase: Option<String>) -> Cmd<()> {
    let state = state.inner().clone();
    fail(
        tokio::task::spawn_blocking(move || {
            if KeyConfig::load(&state.data_dir)?.is_some() {
                bail!("the mail store already exists");
            }
            match passphrase {
                Some(p) => state.create_with_passphrase(&p),
                None => state.create_with_wallet(),
            }
        })
        .await
        .map_err(|e| anyhow!(e))
        .and_then(|r| r),
    )
}

#[tauri::command]
pub async fn unlock(state: Shared<'_>, passphrase: Option<String>) -> Cmd<()> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    let shared = state.inner().clone();
    let worker = shared.clone();
    // Argon2 is deliberately slow; keep it off the UI thread.
    fail(
        tokio::task::spawn_blocking(move || worker.unlock(passphrase.as_deref()))
            .await
            .map_err(|e| anyhow!(e))
            .and_then(|r| r),
    )?;
    sync::start_all(&shared);
    Ok(())
}

#[tauri::command]
pub fn lock(state: Shared) -> Cmd<()> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    if state.mode() != Some(KeyMode::Passphrase) {
        return Err(
            "locking needs passphrase mode; in wallet mode the key is always available".into(),
        );
    }
    state.lock();
    Ok(())
}

#[tauri::command]
pub async fn set_key_mode(state: Shared<'_>, mode: KeyMode, passphrase: Option<String>) -> Cmd<()> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    let state = state.inner().clone();
    fail(
        tokio::task::spawn_blocking(move || state.change_key_mode(mode, passphrase.as_deref()))
            .await
            .map_err(|e| anyhow!(e))
            .and_then(|r| r),
    )
}

#[tauri::command]
pub async fn discover_account(email: String) -> Cmd<ServerConfig> {
    if !email.contains('@') {
        return Err("enter a full email address".into());
    }
    Ok(accounts::discovery::discover(email.trim()).await)
}

#[derive(Deserialize)]
pub struct NewAccount {
    email: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    username: Option<String>,
    config: ServerConfig,
    /// Absent when signing in with OAuth.
    #[serde(default)]
    password: Option<String>,
}

#[tauri::command]
pub async fn add_account(app: AppHandle, state: Shared<'_>, account: NewAccount) -> Cmd<Account> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    let state = state.inner().clone();
    fail(add_account_inner(app, state, account).await)
}

async fn add_account_inner(
    app: AppHandle,
    state: Arc<AppState>,
    new: NewAccount,
) -> Result<Account> {
    let email = new.email.trim().to_ascii_lowercase();
    let mode = state
        .mode()
        .ok_or_else(|| anyhow!("the mail store is locked"))?;
    if state
        .with_db(store::accounts)?
        .iter()
        .any(|a| a.email == email)
    {
        bail!("{email} is already set up");
    }
    let oauth_provider = match &new.password {
        Some(_) => None,
        None => Some(
            new.config
                .oauth_provider
                .clone()
                .ok_or_else(|| anyhow!("enter a password for this account"))?,
        ),
    };
    let mut account = Account {
        id: 0,
        username: new
            .username
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| email.clone()),
        email: email.clone(),
        display_name: new.display_name.trim().to_string(),
        imap_host: new.config.imap_host.trim().to_string(),
        imap_port: new.config.imap_port,
        smtp_host: new.config.smtp_host.trim().to_string(),
        smtp_port: new.config.smtp_port,
        smtp_starttls: new.config.smtp_starttls,
        auth_kind: if oauth_provider.is_some() {
            "oauth".into()
        } else {
            "password".into()
        },
        oauth_provider: oauth_provider.clone(),
    };

    let (secret, login) = match (&oauth_provider, new.password) {
        (Some(provider), _) => {
            let client = accounts::oauth_client(&state, provider)?;
            let tokens = oauth::authorize(&state.http, provider, &client, &email, |url| {
                app.opener()
                    .open_url(url, None::<&str>)
                    .map_err(|e| anyhow!("could not open the browser: {e}"))
            })
            .await?;
            (
                tokens.refresh_token.unwrap_or_default(),
                Login::Bearer(tokens.access_token),
            )
        }
        (None, Some(password)) => (password.clone(), Login::Password(password)),
        (None, None) => unreachable!("checked above"),
    };

    // Prove the sign-in works before saving anything.
    let mut conn = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &login,
    )
    .await?;
    conn.session.logout().await.ok();

    account.id = state.with_db(|c| {
        let id = store::insert_account(c, &account)?;
        secrets::set(c, mode, id, &account.email, &secret)?;
        Ok(id)
    })?;
    sync::start(&state, account.id);
    Ok(account)
}

#[tauri::command]
pub fn list_accounts(state: Shared) -> Cmd<Vec<Account>> {
    fail(state.with_db(store::accounts))
}

#[tauri::command]
pub fn remove_account(state: Shared, id: i64) -> Cmd<()> {
    sync::stop(&state, id);
    fail(state.with_db(|c| {
        if let (Some(account), Some(mode)) = (store::account(c, id)?, state.mode()) {
            secrets::delete(mode, &account.email).ok();
        }
        store::delete_account(c, id)
    }))
}

#[derive(Serialize, Deserialize)]
pub struct OAuthClient {
    provider: String,
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

#[tauri::command]
pub fn get_oauth_client(state: Shared, provider: String) -> Cmd<OAuthClient> {
    fail(state.with_db(|c| {
        Ok(OAuthClient {
            client_id: db::get_setting(c, &format!("oauth.{provider}.client_id"))?
                .unwrap_or_default(),
            client_secret: db::get_setting(c, &format!("oauth.{provider}.client_secret"))?
                .unwrap_or_default(),
            provider,
        })
    }))
}

#[tauri::command]
pub fn set_oauth_client(state: Shared, client: OAuthClient) -> Cmd<()> {
    fail(oauth::provider(&client.provider).map(|_| ()))?;
    fail(state.with_db(|c| {
        db::set_setting(
            c,
            &format!("oauth.{}.client_id", client.provider),
            client.client_id.trim(),
        )?;
        db::set_setting(
            c,
            &format!("oauth.{}.client_secret", client.provider),
            client.client_secret.trim(),
        )
    }))
}

#[tauri::command]
pub fn list_folders(state: Shared, account_id: Option<i64>) -> Cmd<Vec<Folder>> {
    fail(state.with_db(|c| store::folders(c, account_id)))
}

#[tauri::command]
pub fn list_messages(state: Shared, query: ListQuery) -> Cmd<Vec<MessageRow>> {
    fail(state.with_db(|c| store::list_messages(c, &query)))
}

#[tauri::command]
pub fn unread_count(state: Shared) -> Cmd<i64> {
    fail(state.with_db(store::unread_count))
}

#[derive(Serialize)]
pub struct MessageDetail {
    row: MessageRow,
    extra: store::MessageExtra,
    attachments: Vec<parse::AttachmentMeta>,
    auth: Option<AuthReport>,
    first_time_sender: bool,
    sender_blocked: bool,
    images_trusted: bool,
    kind: RenderKind,
    blocked_images: usize,
    deceptive_links: Vec<String>,
    unsubscribe: Option<UnsubscribeOffer>,
    body_url: String,
}

#[tauri::command]
pub async fn get_message(state: Shared<'_>, id: i64) -> Cmd<MessageDetail> {
    let state = state.inner().clone();
    fail(
        async {
            // Large messages are fetched on first open.
            let raw = sync::fetch_body_now(&state, id).await?;
            let row = state
                .with_db(|c| store::message(c, id))?
                .ok_or_else(|| anyhow!("message not found"))?;
            let extra = state
                .with_db(|c| store::message_extra(c, id))?
                .ok_or_else(|| anyhow!("message not found"))?;
            let images_trusted = state.with_db(|c| store::images_trusted(c, &row.from_addr))?;
            let rendered = protocol::document(&state, id, &raw, View::Auto, false, false);
            let attachments = parse::parse(&raw)
                .map(|m| parse::attachments(&m))
                .unwrap_or_default();
            let auth: Option<AuthReport> = extra
                .auth_detail
                .clone()
                .and_then(|d| serde_json::from_value(d).ok());
            if auth.is_none() {
                if let Some(account) = state.with_db(|c| store::account(c, row.account_id))? {
                    sync::spawn_verify(state.clone(), account, id, raw.clone());
                }
            }
            // Forged mail is exactly where an "unsubscribe" link is bait.
            let failed = auth
                .as_ref()
                .map(|a| a.verdict == crate::trust::auth::Verdict::Failed)
                .unwrap_or(false);
            let outgoing = matches!(row.folder_role.as_str(), "sent" | "drafts");
            let unsubscribe = if failed || outgoing {
                None
            } else {
                unsubscribe_plan(&raw, &rendered.document).map(|p| p.offer())
            };
            let blocked = state.with_db(store::blocked)?;
            Ok(MessageDetail {
                first_time_sender: !state
                    .with_db(|c| store::seen_sender_before(c, &row.from_addr, id))?,
                sender_blocked: crate::trust::blocklist::is_blocked(&blocked, &row.from_addr),
                images_trusted,
                kind: rendered.kind,
                blocked_images: rendered.blocked_images,
                deceptive_links: rendered.deceptive_links,
                unsubscribe,
                body_url: format!("{ORIGIN}{}/doc", protocol::asset_prefix(&state, id)),
                attachments,
                auth,
                extra,
                row,
            })
        }
        .await,
    )
}

/// How to leave a mailing list, best method first.
enum UnsubscribePlan {
    OneClick(String),
    Mail {
        to: String,
        subject: String,
        body: String,
    },
    Link(String),
}

#[derive(Serialize)]
pub struct UnsubscribeOffer {
    method: &'static str,
    target: String,
}

impl UnsubscribePlan {
    fn offer(&self) -> UnsubscribeOffer {
        let host = |u: &str| {
            url::Url::parse(u)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default()
        };
        match self {
            UnsubscribePlan::OneClick(url) => UnsubscribeOffer {
                method: "one_click",
                target: host(url),
            },
            UnsubscribePlan::Mail { to, .. } => UnsubscribeOffer {
                method: "mail",
                target: to.clone(),
            },
            UnsubscribePlan::Link(url) => UnsubscribeOffer {
                method: "link",
                target: host(url),
            },
        }
    }
}

fn unsubscribe_plan(raw: &[u8], rendered_document: &str) -> Option<UnsubscribePlan> {
    let header = parse::parse(raw)
        .and_then(|m| parse::list_unsubscribe(&m))
        .unwrap_or_default();
    if let Some(url) = header.one_click {
        return Some(UnsubscribePlan::OneClick(url));
    }
    if let Some((to, subject, body)) = header.mailto.as_deref().and_then(parse::parse_mailto) {
        return Some(UnsubscribePlan::Mail { to, subject, body });
    }
    header
        .link
        .or_else(|| crate::render::sanitize::find_unsubscribe_link(rendered_document))
        .map(UnsubscribePlan::Link)
}

#[tauri::command]
pub async fn unsubscribe(state: Shared<'_>, id: i64) -> Cmd<Option<String>> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    let state = state.inner().clone();
    fail(
        async {
            let raw = state
                .with_db(|c| store::body(c, id))?
                .ok_or_else(|| anyhow!("message body is not downloaded"))?;
            let row = state
                .with_db(|c| store::message(c, id))?
                .ok_or_else(|| anyhow!("message not found"))?;
            let document = protocol::document(&state, id, &raw, View::Auto, false, false).document;
            match unsubscribe_plan(&raw, &document)
                .ok_or_else(|| anyhow!("this message offers no way to unsubscribe"))?
            {
                UnsubscribePlan::OneClick(url) => {
                    crate::net::post_form_public(&url, "List-Unsubscribe=One-Click").await?;
                    Ok(None)
                }
                UnsubscribePlan::Mail { to, subject, body } => {
                    let account = state
                        .with_db(|c| store::account(c, row.account_id))?
                        .ok_or_else(|| anyhow!("account not found"))?;
                    let draft = smtp::Draft {
                        account_id: account.id,
                        to: vec![to],
                        subject,
                        body,
                        ..Default::default()
                    };
                    let message = smtp::build(&account, &draft)?;
                    let login = accounts::login_for(&state, &account).await?;
                    smtp::send(&account, &login, message).await?;
                    Ok(None)
                }
                UnsubscribePlan::Link(url) => Ok(Some(url)),
            }
        }
        .await,
    )
}

/// Plain text of a message, for quoting in a reply.
#[tauri::command]
pub fn quote_text(state: Shared, id: i64) -> Cmd<String> {
    fail(state.with_db(|c| {
        let raw = store::body(c, id)?.ok_or_else(|| anyhow!("message body is not downloaded"))?;
        Ok(parse::parse(&raw)
            .map(|m| parse::any_text(&m))
            .unwrap_or_default())
    }))
}

#[tauri::command]
pub async fn sender_icon(state: Shared<'_>, domain: String, verified: bool) -> Cmd<Option<String>> {
    let enabled =
        fail(state.with_db(|c| db::get_setting(c, "fetch_icons")))?.as_deref() != Some("0");
    if !enabled {
        return Ok(None);
    }
    fail(icons::icon_for(&state, &domain, verified).await)
}

#[tauri::command]
pub fn set_flag(state: Shared, ids: Vec<i64>, flag: String, on: bool) -> Cmd<()> {
    fail(actions::set_flag(&state, &ids, &flag, on))
}

#[tauri::command]
pub fn move_messages(state: Shared, ids: Vec<i64>, role: String) -> Cmd<()> {
    fail(actions::move_messages(&state, &ids, &role))
}

#[tauri::command]
pub fn block_sender(state: Shared, pattern: String) -> Cmd<String> {
    fail(actions::block_sender(&state, &pattern))
}

#[tauri::command]
pub fn unblock_sender(state: Shared, pattern: String) -> Cmd<()> {
    fail(state.with_db(|c| store::unblock(c, &pattern)))
}

#[tauri::command]
pub fn list_blocked(state: Shared) -> Cmd<Vec<String>> {
    fail(state.with_db(store::blocked))
}

#[tauri::command]
pub fn trust_images(state: Shared, addr: String, trusted: bool) -> Cmd<()> {
    fail(state.with_db(|c| store::trust_images(c, &addr.to_ascii_lowercase(), trusted)))
}

#[tauri::command]
pub fn sync_now(state: Shared) -> Cmd<()> {
    let ids: Vec<i64> = state.syncers.lock().unwrap().keys().copied().collect();
    let known = fail(state.with_db(store::accounts))?;
    for account in known {
        if ids.contains(&account.id) {
            state.poke(account.id);
        } else {
            sync::start(state.inner(), account.id);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn load_older(state: Shared, folder_id: i64) -> Cmd<()> {
    let folder = fail(state.with_db(|c| store::folder(c, folder_id)))?.ok_or("folder not found")?;
    if let Some(tx) = state.syncers.lock().unwrap().get(&folder.account_id) {
        tx.send(SyncCmd::LoadOlder(folder_id)).ok();
    }
    Ok(())
}

/// Open a link from a message in the system browser. Only web and mail links.
#[tauri::command]
pub fn open_link(app: AppHandle, url: String) -> Cmd<()> {
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err("only web and mail links can be opened".into());
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

fn attachment_bytes(state: &AppState, id: i64, index: u32) -> Result<(String, Vec<u8>)> {
    let raw = state
        .with_db(|c| store::body(c, id))?
        .ok_or_else(|| anyhow!("message body is not downloaded"))?;
    let msg = parse::parse(&raw).ok_or_else(|| anyhow!("message cannot be parsed"))?;
    let part = msg
        .attachment(index)
        .ok_or_else(|| anyhow!("no such attachment"))?;
    let name = mail_parser::MimeHeaders::attachment_name(part).unwrap_or("attachment");
    // Keep only the file name: a sender must not choose where the file lands.
    let safe: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("attachment")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let safe = safe.trim_start_matches('.').to_string();
    Ok((
        if safe.is_empty() {
            "attachment".into()
        } else {
            safe
        },
        part.contents().to_vec(),
    ))
}

#[tauri::command]
pub async fn save_attachment(
    app: AppHandle,
    state: Shared<'_>,
    id: i64,
    index: u32,
) -> Cmd<Option<String>> {
    let (name, data) = fail(attachment_bytes(&state, id, index))?;
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_file_name(name).blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn open_attachment(app: AppHandle, state: Shared, id: i64, index: u32) -> Cmd<()> {
    fail((|| {
        let (name, data) = attachment_bytes(&state, id, index)?;
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("sobre-{}", key::to_hex(&key::random_bytes::<8>())));
        std::fs::DirBuilder::new().recursive(true).create(&dir)?;
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        let path = dir.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&data)?;
        state.temp_files.lock().unwrap().push(path.clone());
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .context("opening the attachment")?;
        Ok(())
    })())
}

#[tauri::command]
pub async fn pick_files(app: AppHandle) -> Cmd<Vec<String>> {
    let picked = tokio::task::spawn_blocking(move || app.dialog().file().blocking_pick_files())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
        .collect())
}

#[tauri::command]
pub async fn send_message(state: Shared<'_>, draft: smtp::Draft) -> Cmd<()> {
    if state.is_demo() {
        return Err("Not available in the demo mailbox.".into());
    }
    let state = state.inner().clone();
    fail(
        async {
            let account = state
                .with_db(|c| store::account(c, draft.account_id))?
                .ok_or_else(|| anyhow!("pick an account to send from"))?;
            let message = smtp::build(&account, &draft)?;
            let raw = message.formatted();
            let login = accounts::login_for(&state, &account).await?;
            smtp::send(&account, &login, message).await?;

            // Gmail files sent mail by itself; everyone else needs a copy uploaded.
            let is_gmail = matches!(
                crate::render::sanitize::host_tail(&account.imap_host).as_str(),
                "gmail.com" | "googlemail.com"
            );
            if !is_gmail
                && state
                    .with_db(|c| store::folder_by_role(c, account.id, "sent"))?
                    .is_some()
            {
                ops::enqueue(&state, account.id, &ops::append_op("sent", &raw))?;
            }
            if let Some(id) = draft.reply_to_id {
                actions::set_flag(&state, &[id], "answered", true).ok();
            }
            state.poke(account.id);
            state.emit(crate::state::Event::Sent);
            Ok(())
        }
        .await,
    )
}

#[tauri::command]
pub fn open_compose_window(app: AppHandle, state: Shared, draft: serde_json::Value) -> Cmd<()> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.compose_drafts.lock().unwrap().insert(id, draft);
    let token = state.token.clone();
    let nav_app = app.clone();
    tauri::WebviewWindowBuilder::new(
        &app,
        format!("compose-{id}"),
        tauri::WebviewUrl::App(format!("compose?draft={id}").into()),
    )
    .title("New message")
    .inner_size(760.0, 640.0)
    .min_inner_size(480.0, 360.0)
    .incognito(true)
    .on_navigation(move |url| crate::allow_navigation(&nav_app, &token, url))
    .build()
    .map(|_| ())
    .map_err(|e| {
        state.compose_drafts.lock().unwrap().remove(&id);
        e.to_string()
    })
}

#[tauri::command]
pub fn take_compose_draft(state: Shared, id: u32) -> Cmd<Option<serde_json::Value>> {
    Ok(state.compose_drafts.lock().unwrap().remove(&id))
}

#[tauri::command]
pub fn close_window(window: tauri::WebviewWindow) -> Cmd<()> {
    if window.label() == "main" {
        return Err("the main window is not closed this way".into());
    }
    window.close().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct PeriodStats {
    label: &'static str,
    received: i64,
    sent: i64,
    spam: i64,
}

#[derive(Serialize)]
pub struct Stats {
    version: &'static str,
    disk_bytes: u64,
    messages: i64,
    unread: i64,
    downloaded: i64,
    mail_bytes: i64,
    image_count: i64,
    image_bytes: i64,
    icon_count: i64,
    icon_bytes: i64,
    blocked: i64,
    periods: Vec<PeriodStats>,
    top_senders: Vec<(String, String, i64)>,
    verdicts: Vec<(String, i64)>,
    accounts: Vec<(String, i64)>,
}

fn disk_bytes(state: &AppState) -> u64 {
    let db = db::db_path(&state.data_dir);
    ["", "-wal", "-shm"]
        .iter()
        .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", db.display())).ok())
        .map(|m| m.len())
        .sum()
}

const RECEIVED: &str = "f.role NOT IN ('sent', 'drafts', 'junk', 'trash')";

#[tauri::command]
pub fn get_stats(state: Shared) -> Cmd<Stats> {
    fail(state.with_db(|c| {
        let one = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get::<_, Option<i64>>(0))?.unwrap_or(0)) };
        let since = |role_filter: &str, seconds: i64| -> Result<i64> {
            Ok(c.query_row(
                &format!("SELECT count(*) FROM messages m JOIN folders f ON f.id = m.folder_id WHERE {role_filter} AND m.date >= ?1"),
                [db::now() - seconds],
                |r| r.get(0),
            )?)
        };
        const DAY: i64 = 86_400;
        let mut periods = Vec::new();
        for (label, seconds) in [("Last 24 hours", DAY), ("Last 7 days", 7 * DAY), ("Last 30 days", 30 * DAY), ("Last 12 months", 365 * DAY)] {
            periods.push(PeriodStats {
                label,
                received: since(RECEIVED, seconds)?,
                sent: since("f.role = 'sent'", seconds)?,
                spam: since("f.role = 'junk'", seconds)?,
            });
        }
        let mut stmt = c.prepare(&format!(
            "SELECT m.from_addr, max(m.from_name), count(*) AS n FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE {RECEIVED} AND m.date >= ?1 AND m.from_addr != '' GROUP BY m.from_addr ORDER BY n DESC LIMIT 8"
        ))?;
        let top_senders = stmt.query_map([db::now() - 30 * DAY], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut stmt = c.prepare(&format!(
            "SELECT coalesce(m.auth_verdict, 'not checked'), count(*) FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE {RECEIVED} GROUP BY 1 ORDER BY 2 DESC"
        ))?;
        let verdicts = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut stmt = c.prepare(
            "SELECT a.email, (SELECT count(*) FROM messages m WHERE m.account_id = a.id) FROM accounts a ORDER BY a.id",
        )?;
        let accounts = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;

        Ok(Stats {
            version: env!("CARGO_PKG_VERSION"),
            disk_bytes: disk_bytes(&state),
            messages: one("SELECT count(*) FROM messages")?,
            unread: store::unread_count(c)?,
            downloaded: one("SELECT count(*) FROM bodies")?,
            mail_bytes: one("SELECT sum(length(raw)) FROM bodies")?,
            image_count: one("SELECT count(*) FROM image_cache")?,
            image_bytes: one("SELECT sum(length(data)) FROM image_cache")?,
            icon_count: one("SELECT count(*) FROM icon_cache WHERE data IS NOT NULL")?,
            icon_bytes: one("SELECT sum(length(data)) FROM icon_cache")?,
            blocked: one("SELECT count(*) FROM blocked")?,
            periods,
            top_senders,
            verdicts,
            accounts,
        })
    }))
}

#[tauri::command]
pub async fn clear_storage(state: Shared<'_>, what: String) -> Cmd<u64> {
    let state = state.inner().clone();
    let worker = state.clone();
    let freed = fail(
        tokio::task::spawn_blocking(move || {
            let before = disk_bytes(&worker);
            worker.with_db(|c| {
                match what.as_str() {
                    "images" => c.execute_batch("DELETE FROM image_cache; DELETE FROM icon_cache;")?,
                    // Queued changes stay: they still have to reach the server.
                    "mail" => c.execute_batch("DELETE FROM messages_fts; DELETE FROM messages; UPDATE folders SET uidvalidity = NULL;")?,
                    other => bail!("cannot clear {other}"),
                }
                // Deleting only marks pages free; this hands them back to the disk.
                c.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
                Ok(())
            })?;
            Ok(before.saturating_sub(disk_bytes(&worker)))
        })
        .await
        .map_err(|e| anyhow!(e))
        .and_then(|r| r),
    )?;
    for account in fail(state.with_db(store::accounts))? {
        state.emit(crate::state::Event::MailChanged {
            account_id: account.id,
        });
        state.poke(account.id);
    }
    Ok(freed)
}

#[derive(Serialize, Deserialize)]
pub struct Settings {
    notifications: bool,
    fetch_icons: bool,
    auto_junk_failed: bool,
    autostart: bool,
    #[serde(default = "yes")]
    check_updates: bool,
    /// system | light | dark
    #[serde(default = "default_theme")]
    theme: String,
}

fn yes() -> bool {
    true
}

fn default_theme() -> String {
    "system".into()
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: Shared) -> Cmd<Settings> {
    fail(state.with_db(|c| {
        let on = |key: &str, default: bool| -> Result<bool> {
            Ok(db::get_setting(c, key)?
                .map(|v| v == "1")
                .unwrap_or(default))
        };
        Ok(Settings {
            notifications: on("notifications", true)?,
            fetch_icons: on("fetch_icons", true)?,
            auto_junk_failed: on("auto_junk_failed", false)?,
            autostart: app.autolaunch().is_enabled().unwrap_or(false),
            check_updates: on("check_updates", true)?,
            theme: db::get_setting(c, "theme")?.unwrap_or_else(default_theme),
        })
    }))
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: Shared, settings: Settings) -> Cmd<()> {
    fail(state.with_db(|c| {
        let bit = |b: bool| if b { "1" } else { "0" };
        db::set_setting(c, "notifications", bit(settings.notifications))?;
        db::set_setting(c, "fetch_icons", bit(settings.fetch_icons))?;
        db::set_setting(c, "check_updates", bit(settings.check_updates))?;
        db::set_setting(c, "auto_junk_failed", bit(settings.auto_junk_failed))?;
        let theme = if matches!(settings.theme.as_str(), "light" | "dark") {
            settings.theme.as_str()
        } else {
            "system"
        };
        db::set_setting(c, "theme", theme)
    }))?;
    state.emit(crate::state::Event::ThemeChanged);
    let launcher = app.autolaunch();
    if settings.autostart {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}

/// Look for a newer release. Only AppImage copies can replace themselves, so
/// other installs report nothing. `force` ignores the startup setting.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: Shared<'_>,
    force: bool,
) -> Cmd<Option<crate::update::UpdateInfo>> {
    if crate::update::running_appimage().is_none() {
        return Ok(None);
    }
    let enabled =
        fail(state.with_db(|c| db::get_setting(c, "check_updates")))?.as_deref() != Some("0");
    if !enabled && !force {
        return Ok(None);
    }
    fail(crate::update::check(&state.http, &app.package_info().version.to_string()).await)
}

#[tauri::command]
pub async fn install_update(app: AppHandle, state: Shared<'_>) -> Cmd<String> {
    use tauri::Emitter as _;
    fail(
        async {
            let target = crate::update::running_appimage().ok_or_else(|| {
                anyhow!(
                    "this copy was not started from an AppImage file, so it cannot replace itself"
                )
            })?;
            // Ask again rather than trusting anything the window sends.
            let info = crate::update::check(&state.http, &app.package_info().version.to_string())
                .await?
                .ok_or_else(|| anyhow!("there is no newer version"))?;
            let mut last = 0;
            crate::update::install(&info, &target, |done, total| {
                let percent = done * 100 / total.max(1);
                if percent != last {
                    last = percent;
                    app.emit("update-progress", percent).ok();
                }
            })
            .await?;
            Ok(info.version)
        }
        .await,
    )
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// Show a mailbox of made-up mail in place of the real one.
#[tauri::command]
pub async fn enter_demo(state: Shared<'_>) -> Cmd<()> {
    let state = state.inner().clone();
    fail(
        tokio::task::spawn_blocking(move || state.enter_demo())
            .await
            .map_err(|e| anyhow!(e))
            .and_then(|r| r),
    )
}

/// Back to the real mailbox. Wallet mode reopens by itself; passphrase mode
/// lands on the unlock screen.
#[tauri::command]
pub async fn leave_demo(state: Shared<'_>) -> Cmd<()> {
    let shared = state.inner().clone();
    if !shared.is_demo() {
        return Ok(());
    }
    shared.leave_demo();
    if let Ok(Some(KeyConfig {
        mode: KeyMode::Wallet,
        ..
    })) = KeyConfig::load(&shared.data_dir)
    {
        let worker = shared.clone();
        fail(
            tokio::task::spawn_blocking(move || worker.unlock(None))
                .await
                .map_err(|e| anyhow!(e))
                .and_then(|r| r),
        )?;
        sync::start_all(&shared);
    }
    shared.emit(crate::state::Event::Reset);
    Ok(())
}

#[tauri::command]
pub fn install_status() -> crate::install::InstallStatus {
    crate::install::status()
}

/// Copy this AppImage to the user's programs folder and add it to the menu.
#[tauri::command]
pub async fn install_app() -> Cmd<crate::install::InstallStatus> {
    fail(
        tokio::task::spawn_blocking(|| {
            let source = crate::update::running_appimage()
                .ok_or_else(|| anyhow!("only the AppImage download can install itself; this copy was installed another way"))?;
            let places = crate::install::Places::for_user()?;
            crate::install::install(&source, &places)?;
            crate::install::refresh_menus(&places);
            Ok(crate::install::status())
        })
        .await
        .map_err(|e| anyhow!(e))
        .and_then(|r| r),
    )
}

#[tauri::command]
pub async fn uninstall_app() -> Cmd<crate::install::InstallStatus> {
    fail(
        tokio::task::spawn_blocking(|| {
            let places = crate::install::Places::for_user()?;
            crate::install::uninstall(&places)?;
            crate::install::refresh_menus(&places);
            Ok(crate::install::status())
        })
        .await
        .map_err(|e| anyhow!(e))
        .and_then(|r| r),
    )
}
