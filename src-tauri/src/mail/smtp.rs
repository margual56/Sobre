use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use lettre::{
    message::{header::ContentType, Attachment, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::{Credentials, Mechanism},
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde::Deserialize;

use crate::{
    accounts::Login,
    db::{key, store::Account},
    render,
};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Draft {
    pub account_id: i64,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// Also send an HTML rendering of the body, treating it as markdown.
    #[serde(default)]
    pub markdown: bool,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Option<String>,
    /// Paths of files to attach.
    #[serde(default)]
    pub attachments: Vec<String>,
    /// The message being answered, to flag it afterwards.
    #[serde(default)]
    pub reply_to_id: Option<i64>,
}

fn mailbox(text: &str) -> Result<Mailbox> {
    text.trim()
        .parse::<Mailbox>()
        .map_err(|e| anyhow!("\"{}\" is not a valid address: {e}", text.trim()))
}

pub fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("txt") | Some("md") | Some("log") => "text/plain",
        Some("csv") => "text/csv",
        Some("html") | Some("htm") => "text/html",
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("odt") => "application/vnd.oasis.opendocument.text",
        Some("ics") => "text/calendar",
        _ => "application/octet-stream",
    }
}

fn message_id_token(id: &str) -> String {
    let id = id.trim();
    if id.starts_with('<') {
        id.to_string()
    } else {
        format!("<{id}>")
    }
}

pub fn build(account: &Account, draft: &Draft) -> Result<Message> {
    if draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .all(|a| a.trim().is_empty())
    {
        bail!("add at least one recipient");
    }
    let from_addr = account
        .email
        .parse()
        .map_err(|e| anyhow!("account address is invalid: {e}"))?;
    let name =
        (!account.display_name.trim().is_empty()).then(|| account.display_name.trim().to_string());
    let domain = account.email.rsplit('@').next().unwrap_or("localhost");

    let mut builder = Message::builder()
        .from(Mailbox::new(name, from_addr))
        .subject(draft.subject.trim())
        // A random id on the account's domain; the default would leak the machine's hostname.
        .message_id(Some(format!(
            "<{}@{domain}>",
            key::to_hex(&key::random_bytes::<16>())
        )));
    for to in draft.to.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.to(mailbox(to)?);
    }
    for cc in draft.cc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.cc(mailbox(cc)?);
    }
    for bcc in draft.bcc.iter().filter(|a| !a.trim().is_empty()) {
        builder = builder.bcc(mailbox(bcc)?);
    }
    if let Some(id) = draft
        .in_reply_to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let id = message_id_token(id);
        let references = match draft.references.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(refs) if !refs.contains(&id) => format!("{} {id}", refs.trim()),
            Some(refs) => refs.trim().to_string(),
            None => id.clone(),
        };
        builder = builder.in_reply_to(id).references(references);
    }

    let text = if draft.markdown {
        let html = ammonia::clean(&render::markdown_to_html(&draft.body));
        let page = format!(
            "<!doctype html><html><body style=\"font-family:sans-serif\">{html}</body></html>"
        );
        MultiPart::alternative_plain_html(draft.body.clone(), page)
    } else {
        MultiPart::mixed().singlepart(SinglePart::plain(draft.body.clone()))
    };
    if draft.attachments.is_empty() {
        return Ok(builder.multipart(text)?);
    }
    let mut mixed = MultiPart::mixed().multipart(text);
    for path in &draft.attachments {
        let path = Path::new(path);
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let content_type = ContentType::parse(mime_for(path)).map_err(|e| anyhow!("{e}"))?;
        mixed = mixed.singlepart(Attachment::new(name).body(data, content_type));
    }
    Ok(builder.multipart(mixed)?)
}

pub async fn send(account: &Account, login: &Login, message: Message) -> Result<()> {
    let builder = if account.smtp_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&account.smtp_host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&account.smtp_host)
    }
    .map_err(|e| anyhow!("SMTP setup failed: {e}"))?
    .port(account.smtp_port);
    let transport = match login {
        Login::Password(password) => builder
            .credentials(Credentials::new(account.username.clone(), password.clone()))
            .authentication(vec![Mechanism::Plain, Mechanism::Login]),
        Login::Bearer(token) => builder
            .credentials(Credentials::new(account.username.clone(), token.clone()))
            .authentication(vec![Mechanism::Xoauth2]),
    }
    .build();
    transport
        .send(message)
        .await
        .map_err(|e| anyhow!("sending failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::parse;

    fn account() -> Account {
        Account {
            id: 1,
            email: "me@example.org".into(),
            display_name: "Marcos".into(),
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
    fn builds_a_threaded_markdown_reply() {
        let draft = Draft {
            account_id: 1,
            to: vec!["Ana <ana@example.com>".into()],
            cc: vec!["bob@example.com".into(), "  ".into()],
            subject: "Re: Hola".into(),
            body: "Sure, **yes**.\n\n<script>alert(1)</script>".into(),
            markdown: true,
            in_reply_to: Some("abc@example.com".into()),
            references: Some("<root@example.com>".into()),
            ..Default::default()
        };
        let raw = build(&account(), &draft).unwrap().formatted();
        let msg = parse::parse(&raw).unwrap();
        let env = parse::envelope(&msg);
        assert_eq!(env.from.addr, "me@example.org");
        assert_eq!(env.to[0].addr, "ana@example.com");
        assert_eq!(env.cc.len(), 1);
        assert_eq!(env.in_reply_to.as_deref(), Some("<abc@example.com>"));
        assert_eq!(
            env.references.as_deref(),
            Some("<root@example.com> <abc@example.com>")
        );
        assert!(env.message_id.unwrap().ends_with("@example.org"));
        let html = parse::html_body(&msg).unwrap();
        assert!(html.contains("<strong>yes</strong>") && !html.contains("<script"));
        assert!(parse::text_body(&msg).unwrap().contains("**yes**"));
    }

    #[test]
    fn attaches_files_and_validates_recipients() {
        let path = std::env::temp_dir().join(format!(
            "sobre-att-{}.txt",
            key::to_hex(&key::random_bytes::<4>())
        ));
        std::fs::write(&path, b"hello attachment").unwrap();
        let draft = Draft {
            to: vec!["ana@example.com".into()],
            subject: "File".into(),
            body: "see attached".into(),
            attachments: vec![path.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let raw = build(&account(), &draft).unwrap().formatted();
        let msg = parse::parse(&raw).unwrap();
        let atts = parse::attachments(&msg);
        assert_eq!(atts.len(), 1);
        assert_eq!((atts[0].mime.as_str(), atts[0].size), ("text/plain", 16));
        std::fs::remove_file(path).ok();

        assert!(build(
            &account(),
            &Draft {
                subject: "x".into(),
                ..Default::default()
            }
        )
        .is_err());
        assert!(build(
            &account(),
            &Draft {
                to: vec!["not an address".into()],
                ..Default::default()
            }
        )
        .is_err());
    }
}
