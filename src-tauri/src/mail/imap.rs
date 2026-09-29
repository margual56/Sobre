use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use async_imap::types::{Flag, NameAttribute};
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_rustls::{
    client::TlsStream,
    rustls::{pki_types::ServerName, ClientConfig, RootCertStore},
    TlsConnector,
};

use crate::accounts::{oauth, Login};

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

/// System trust store, loaded once.
pub fn tls_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            for cert in rustls_native_certs::load_native_certs().certs {
                roots.add(cert).ok();
            }
            Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

struct XOAuth2(String);

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        // On failure the server sends a JSON challenge and expects an empty reply.
        std::mem::take(&mut self.0)
    }
}

pub struct Conn {
    pub session: Session,
    pub can_move: bool,
    pub can_idle: bool,
}

pub async fn connect(host: &str, port: u16, user: &str, login: &Login) -> Result<Conn> {
    let tcp = tokio::time::timeout(Duration::from_secs(20), TcpStream::connect((host, port)))
        .await
        .map_err(|_| anyhow!("timed out connecting to {host}"))?
        .with_context(|| format!("connecting to {host}:{port}"))?;
    tcp.set_nodelay(true).ok();
    let name = ServerName::try_from(host.to_string()).context("invalid server name")?;
    let tls = TlsConnector::from(tls_config())
        .connect(name, tcp)
        .await
        .context("TLS handshake failed")?;

    let mut client = async_imap::Client::new(tls);
    client
        .read_response()
        .await
        .context("reading the server greeting")?
        .ok_or_else(|| anyhow!("server closed the connection"))?;

    let mut session = match login {
        Login::Password(password) => client.login(user, password).await.map_err(|(e, _)| e),
        Login::Bearer(token) => client
            .authenticate("XOAUTH2", XOAuth2(oauth::xoauth2(user, token)))
            .await
            .map_err(|(e, _)| e),
    }
    .map_err(|e| anyhow!("sign-in to {host} failed: {e}"))?;

    let caps = session.capabilities().await?;
    Ok(Conn {
        can_move: caps.has_str("MOVE"),
        can_idle: caps.has_str("IDLE"),
        session,
    })
}

fn role_from_name(name: &str) -> &'static str {
    let leaf = name
        .rsplit(['/', '.'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    if name.eq_ignore_ascii_case("INBOX") {
        return "inbox";
    }
    match leaf.as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" | "enviados" => "sent",
        "drafts" | "draft" | "borradores" => "drafts",
        "junk" | "spam" | "bulk mail" | "junk e-mail" | "junk email" | "correo no deseado" => {
            "junk"
        }
        "trash" | "deleted items" | "deleted messages" | "bin" | "papelera" => "trash",
        "archive" | "archives" | "all mail" => "archive",
        _ => "other",
    }
}

/// List selectable folders as (name, role).
pub async fn list_folders(session: &mut Session) -> Result<Vec<(String, String)>> {
    let names: Vec<_> = session
        .list(Some(""), Some("*"))
        .await?
        .try_collect()
        .await?;
    let mut out: Vec<(String, String)> = Vec::new();
    for name in &names {
        let mut role = None;
        let mut skip = false;
        for attr in name.attributes() {
            match attr {
                NameAttribute::NoSelect => skip = true,
                // Gmail's virtual views would duplicate every message.
                NameAttribute::All | NameAttribute::Flagged => skip = true,
                NameAttribute::Sent => role = Some("sent"),
                NameAttribute::Drafts => role = Some("drafts"),
                NameAttribute::Junk => role = Some("junk"),
                NameAttribute::Trash => role = Some("trash"),
                NameAttribute::Archive => role = Some("archive"),
                NameAttribute::Extension(ext) => {
                    let ext = ext.to_ascii_lowercase();
                    if ext == "\\important" || ext == "\\nonexistent" {
                        skip = true;
                    }
                }
                _ => {}
            }
        }
        if skip {
            continue;
        }
        let role = if name.name().eq_ignore_ascii_case("INBOX") {
            "inbox"
        } else {
            role.unwrap_or_else(|| role_from_name(name.name()))
        };
        out.push((name.name().to_string(), role.to_string()));
    }
    // Each special role belongs to one folder; later claimants are ordinary folders.
    let mut seen = std::collections::HashSet::new();
    for (_, role) in out.iter_mut() {
        if role != "other" && !seen.insert(role.clone()) {
            *role = "other".into();
        }
    }
    if !out.iter().any(|(_, r)| r == "inbox") {
        bail!("the server lists no INBOX");
    }
    Ok(out)
}

pub fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
}

pub fn read_flags<'a>(flags: impl Iterator<Item = Flag<'a>>) -> Flags {
    let mut out = Flags {
        seen: false,
        flagged: false,
        answered: false,
    };
    for flag in flags {
        match flag {
            Flag::Seen => out.seen = true,
            Flag::Flagged => out.flagged = true,
            Flag::Answered => out.answered = true,
            _ => {}
        }
    }
    out
}

/// Move messages, falling back to copy + delete on servers without MOVE.
pub async fn move_uids(conn: &mut Conn, uids: &[u32], target: &str) -> Result<()> {
    let set = uid_set(uids);
    let target = quote(target);
    if conn.can_move {
        conn.session.uid_mv(&set, &target).await?;
    } else {
        conn.session.uid_copy(&set, &target).await?;
        delete_uids(conn, uids).await?;
    }
    Ok(())
}

pub async fn delete_uids(conn: &mut Conn, uids: &[u32]) -> Result<()> {
    let set = uid_set(uids);
    conn.session
        .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    conn.session
        .uid_expunge(&set)
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

/// Quote a mailbox name for use as a command argument.
pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_from_names() {
        assert_eq!(role_from_name("INBOX"), "inbox");
        assert_eq!(role_from_name("INBOX.Sent"), "sent");
        assert_eq!(role_from_name("[Gmail]/Spam"), "junk");
        assert_eq!(role_from_name("Deleted Items"), "trash");
        assert_eq!(role_from_name("Projects/2026"), "other");
    }

    #[test]
    fn quoting_and_sets() {
        assert_eq!(quote("My \"odd\" box"), "\"My \\\"odd\\\" box\"");
        assert_eq!(uid_set(&[3, 5, 9]), "3,5,9");
    }
}
