use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::Connection;

use crate::{
    db::{
        self,
        store::{self, Account, NewMessage},
    },
    mail::parse,
    trust::{
        auth::{AuthReport, DkimCheck, ProviderResults, Verdict},
        heuristics,
    },
};

pub const DEMO_EMAIL: &str = "alex@example.com";

struct Sample {
    folder: &'static str,
    from_name: &'static str,
    from_addr: &'static str,
    to: &'static str,
    subject: &'static str,
    hours_ago: f64,
    seen: bool,
    flagged: bool,
    answered: bool,
    /// None leaves the message unchecked, as sent mail is.
    verdict: Option<Verdict>,
    text: &'static str,
    html: Option<&'static str>,
    attachment: Option<(&'static str, &'static str, &'static [u8])>,
    headers: &'static str,
}

const BASE: Sample = Sample {
    folder: "INBOX",
    from_name: "",
    from_addr: "",
    to: "Alex Rivera <alex@example.com>",
    subject: "",
    hours_ago: 1.0,
    seen: true,
    flagged: false,
    answered: false,
    verdict: Some(Verdict::Verified),
    text: "",
    html: None,
    attachment: None,
    headers: "",
};

const PDF: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n";

const RELEASE_HTML: &str = r#"<div style="font-family:sans-serif;max-width:640px;margin:auto;color:#1f2328">
<p style="color:#59636e;font-size:13px">margual56 published a release in <b>margual56/Sobre</b></p>
<h2 style="margin:8px 0">Sobre 0.2.0</h2>
<h3>Features</h3>
<ul><li>self-update for the appimage from github releases</li><li>warn on every start when the tray icon cannot be shown</li></ul>
<p><a href="https://github.com/margual56/Sobre/releases/tag/v0.2.0" style="background:#1f883d;color:#fff;padding:8px 14px;border-radius:6px;text-decoration:none">View release</a></p>
<p style="color:#59636e;font-size:12px;border-top:1px solid #d1d9e0;padding-top:12px">You are receiving this because you are watching this repository.</p></div>"#;

const RECEIPT_HTML: &str = r#"<table width="100%" style="background:#f6f9fc;font-family:sans-serif"><tr><td align="center" style="padding:24px">
<table width="480" style="background:#ffffff;border-radius:8px;padding:28px;color:#30313d">
<tr><td><div style="font-size:13px;color:#687385">Receipt from Northwind Analytics</div>
<div style="font-size:34px;font-weight:bold;margin:6px 0">€14.00</div>
<div style="font-size:13px;color:#687385">Paid 6 October 2026</div></td></tr>
<tr><td style="padding-top:22px"><table width="100%" style="font-size:14px;border-collapse:collapse">
<tr><td style="padding:8px 0;border-bottom:1px solid #e3e8ee">Analytics, monthly plan</td><td align="right" style="border-bottom:1px solid #e3e8ee">€14.00</td></tr>
<tr><td style="padding:8px 0"><b>Amount paid</b></td><td align="right"><b>€14.00</b></td></tr></table></td></tr>
<tr><td style="padding-top:20px;font-size:13px;color:#687385">Receipt number 2931-5521 · Visa ending 4242<br>Questions? <a href="https://support.stripe.com/">Visit our support site</a>.</td></tr>
</table></td></tr></table>"#;

const PHISH_HTML: &str = r#"<div style="font-family:Arial,sans-serif;max-width:560px;margin:auto;color:#2c2e2f">
<h2 style="color:#003087">Your account has been limited</h2>
<p>We noticed unusual activity and have temporarily limited what you can do with your account.</p>
<p>To restore full access, confirm your identity within <b>24 hours</b>:</p>
<p><a href="https://account-verify.example/login?session=8841">https://www.paypal.com/signin</a></p>
<p>If you do not confirm, your account will be permanently closed.</p>
<p style="font-size:12px;color:#6c7378">Copyright 2026. All rights reserved.</p></div>"#;

const NEWSLETTER_HTML: &str = r#"<style>.card{border:1px solid #e5e7eb;border-radius:10px;padding:16px;margin:14px 0}.tag{font-size:11px;text-transform:uppercase;letter-spacing:.06em;color:#7c3aed}</style>
<div style="font-family:Georgia,serif;max-width:600px;margin:auto;color:#111827">
<img src="https://img.fieldweekly.example/open.gif?u=7f3a21" width="1" height="1" alt="">
<h1 style="font-size:26px;margin-bottom:4px">Field Weekly</h1>
<p style="color:#6b7280;margin-top:0">Issue 214 · small tools, done well</p>
<div class="card"><div class="tag">Deep dive</div><h3 style="margin:6px 0">How SQLite stores a page</h3>
<p>A walk through the B-tree layout, with diagrams you can actually follow. <a href="https://fieldweekly.example/214/sqlite">Read more</a></p></div>
<div class="card"><div class="tag">Release</div><h3 style="margin:6px 0">Rust 1.98 lands</h3>
<p>Faster incremental builds and a handful of long-awaited stabilisations. <a href="https://fieldweekly.example/214/rust">Read more</a></p></div>
<div class="card"><div class="tag">Show and tell</div><h3 style="margin:6px 0">A mail client in three panes</h3>
<p>One reader built a tiny desktop client that refuses to load anything a sender asks for. <a href="https://fieldweekly.example/214/mail">Read more</a></p></div>
<p style="font-size:12px;color:#6b7280">You get this because you signed up at fieldweekly.example. <a href="https://fieldweekly.example/unsubscribe?u=7f3a21">Unsubscribe</a></p></div>"#;

const NOTES_MD: &str = "# Sync meeting, 5 October\n\nThanks all. Here is what we agreed:\n\n1. **Ship the importer** by the 16th. Dana owns it.\n2. Move the weekly call to *Tuesdays at 10:00*.\n3. Freeze the schema until the importer is out.\n\n## Open questions\n\n- Do we keep the old export format? See [the thread](https://tracker.fieldnotes.example/t/481).\n- Who reviews the migration?\n\n```\nimporter --dry-run --from legacy.db\n```\n\nShout if I missed anything.\n\nDana\n";

fn samples() -> Vec<Sample> {
    vec![
        Sample { from_name: "GitHub", from_addr: "notifications@github.com", subject: "[margual56/Sobre] Release v0.2.0 - Sobre 0.2.0", hours_ago: 0.4, seen: false, text: "Sobre 0.2.0\n\nFeatures\n- self-update for the appimage from github releases\n- warn on every start when the tray icon cannot be shown\n", html: Some(RELEASE_HTML), ..BASE },
        Sample { from_name: "Clara Jiménez", from_addr: "clara@northwind.example", subject: "Lunch on Thursday?", hours_ago: 1.6, seen: false, text: "Hi Alex,\n\nAre you free on Thursday around 13:30? There's a new place by the river I've been meaning to try, and I want to hear how the move went.\n\nIf Thursday is bad, Friday works too.\n\nClara\n", ..BASE },
        Sample { from_name: "Stripe", from_addr: "receipts@stripe.com", subject: "Your receipt from Northwind Analytics #2931-5521", hours_ago: 5.2, text: "Receipt from Northwind Analytics\n\nAmount paid: EUR 14.00\nDate: 6 October 2026\nReceipt number: 2931-5521\n", html: Some(RECEIPT_HTML), attachment: Some(("Receipt-2931-5521.pdf", "application/pdf", PDF)), ..BASE },
        Sample { from_name: "service@paypal.com", from_addr: "security@account-verify.example", subject: "Action required: confirm your account", hours_ago: 7.5, seen: false, verdict: Some(Verdict::Failed), text: "Your account has been limited. Confirm your identity within 24 hours.\n", html: Some(PHISH_HTML), headers: "Reply-To: helpdesk@fastmail-recovery.example\r\n", ..BASE },
        Sample { from_name: "Field Weekly", from_addr: "hello@fieldweekly.example", subject: "#214: SQLite pages, Rust 1.98, and a three-pane mail client", hours_ago: 20.0, text: "Field Weekly, issue 214.\n", html: Some(NEWSLETTER_HTML), headers: "List-Unsubscribe: <https://fieldweekly.example/unsubscribe?u=7f3a21>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n", ..BASE },
        Sample { from_name: "Dana Okafor", from_addr: "dana@fieldnotes.example", subject: "Notes from the sync meeting", hours_ago: 26.0, flagged: true, verdict: Some(Verdict::Unverified), text: NOTES_MD, ..BASE },
        Sample { from_name: "Hetzner Online", from_addr: "billing@hetzner.com", subject: "Invoice 0842117 is available", hours_ago: 49.0, text: "Dear customer,\n\nYour invoice 0842117 for October 2026 is now available in your account.\n\nAmount due: EUR 4.51\nPayment method: direct debit\n\nNo action is needed.\n\nKind regards\nHetzner Online\n", attachment: Some(("Hetzner_2026-10-01_0842117.pdf", "application/pdf", PDF)), ..BASE },
        Sample { from_name: "Lucía Rivera", from_addr: "lucia.rivera@gmail.com", subject: "Photos from the weekend", hours_ago: 74.0, text: "Hola!\n\nHere are the photos from Saturday. The one by the lighthouse came out great.\n\nCall me on Sunday?\n\nBesos,\nLucía\n", attachment: Some(("lighthouse.pdf", "application/pdf", PDF)), ..BASE },
        Sample { from_name: "Let's Encrypt", from_addr: "expiry@letsencrypt.org", subject: "Your certificate for mail.example.com expires in 20 days", hours_ago: 98.0, text: "Hello,\n\nYour certificate (or certificates) for the names listed below will expire in 20 days. Please make sure to renew your certificate before then, or visitors to your web site will encounter errors.\n\n  mail.example.com\n\nRegards,\nThe Let's Encrypt Team\n", ..BASE },
        Sample { from_name: "Tomás Vidal", from_addr: "tomas@studio-vidal.example", subject: "Re: Icon drafts", hours_ago: 121.0, answered: true, verdict: Some(Verdict::Unverified), text: "The second one, no question. The envelope reads well even at 16 px.\n\nCan you send it as SVG? I'll try it in the tray tonight.\n\nTomás\n\n> On Thu, Alex Rivera wrote:\n> Three drafts attached. I lean towards the blue one but tell me\n> what you think.\n", ..BASE },
        Sample { folder: "Sent", from_name: "Alex Rivera", from_addr: DEMO_EMAIL, to: "Clara Jiménez <clara@northwind.example>", subject: "Re: Lunch on Thursday?", hours_ago: 1.1, verdict: None, text: "Thursday at 13:30 works. Send me the address?\n\nAlex\n", ..BASE },
        Sample { folder: "Sent", from_name: "Alex Rivera", from_addr: DEMO_EMAIL, to: "Tomás Vidal <tomas@studio-vidal.example>", subject: "Re: Icon drafts", hours_ago: 119.0, verdict: None, text: "SVG attached. Let me know how it looks next to the clock.\n\nAlex\n", attachment: Some(("app-icon.pdf", "application/pdf", PDF)), ..BASE },
        Sample { folder: "Archive", from_name: "Clara Jiménez", from_addr: "clara@northwind.example", subject: "Train tickets", hours_ago: 300.0, text: "Booked! Coach 4, seats 11 and 12. I'll forward the PDF tonight.\n\nClara\n", ..BASE },
        Sample { folder: "Spam", from_name: "Prize Department", from_addr: "winner@lucky-draw-intl.example", subject: "You have been selected: claim EUR 850,000", hours_ago: 9.0, seen: false, verdict: Some(Verdict::Failed), text: "Dear beneficiary,\n\nYour email address was selected in our international draw. To claim your prize reply with your full name, address and bank details.\n", headers: "Reply-To: claims.office@inbox-mailer.example\r\n", ..BASE },
        Sample { folder: "Spam", from_name: "Dr. Health", from_addr: "offers@best-pharma-deals.example", subject: "80% off, today only", hours_ago: 31.0, seen: false, verdict: Some(Verdict::Unverified), text: "Limited stock. Order now.\n", ..BASE },
    ]
}

fn encoded(name: &str) -> String {
    if name.is_ascii() {
        format!("\"{name}\"")
    } else {
        format!("=?utf-8?B?{}?=", STANDARD.encode(name))
    }
}

fn encode_address(addr: &str) -> String {
    match addr.rsplit_once(" <") {
        Some((name, rest)) => format!("{} <{rest}", encoded(name)),
        None => addr.to_string(),
    }
}

fn part(mime: &str, body: &str) -> String {
    format!(
        "Content-Type: {mime}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        wrap(&STANDARD.encode(body))
    )
}

fn wrap(b64: &str) -> String {
    b64.as_bytes()
        .chunks(76)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn raw_message(s: &Sample, index: usize, date: i64) -> Vec<u8> {
    let when = chrono::DateTime::from_timestamp(date, 0)
        .unwrap_or_default()
        .to_rfc2822();
    let domain = s.from_addr.rsplit('@').next().unwrap_or("example.com");
    let mut out = format!(
        "From: {} <{}>\r\nTo: {}\r\nSubject: {}\r\nDate: {when}\r\nMessage-ID: <demo-{index}@{domain}>\r\n{}MIME-Version: 1.0\r\n",
        encoded(s.from_name),
        s.from_addr,
        encode_address(s.to),
        s.subject,
        s.headers
    );
    let body = match s.html {
        Some(html) => format!(
            "Content-Type: multipart/alternative; boundary=\"alt{index}\"\r\n\r\n--alt{index}\r\n{}--alt{index}\r\n{}--alt{index}--\r\n",
            part("text/plain", s.text),
            part("text/html", html)
        ),
        None => part("text/plain", s.text),
    };
    match s.attachment {
        Some((name, mime, data)) => out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"mix{index}\"\r\n\r\n--mix{index}\r\n{body}--mix{index}\r\nContent-Type: {mime}; name=\"{name}\"\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--mix{index}--\r\n",
            wrap(&STANDARD.encode(data))
        )),
        None => out.push_str(&body),
    }
    out.into_bytes()
}

fn report(s: &Sample, verdict: Verdict) -> AuthReport {
    let domain = s
        .from_addr
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_string();
    let (dkim, provider, policy, summary) = match verdict {
        Verdict::Verified => (
            vec![DkimCheck {
                domain: domain.clone(),
                selector: "s1".into(),
                result: "pass".into(),
                detail: String::new(),
                aligned: true,
            }],
            ProviderResults {
                authserv_id: "mx.example.com".into(),
                spf: Some("pass".into()),
                dkim: Some("pass".into()),
                dmarc: Some("pass".into()),
            },
            Some("reject".to_string()),
            format!("Signed by {domain} and the signature is valid."),
        ),
        Verdict::Failed => (
            Vec::new(),
            ProviderResults {
                authserv_id: "mx.example.com".into(),
                spf: Some("fail".into()),
                dkim: Some("none".into()),
                dmarc: Some("fail".into()),
            },
            Some("reject".to_string()),
            "Your mail provider reports that this message fails the sender domain's DMARC check."
                .to_string(),
        ),
        Verdict::Unverified => (
            Vec::new(),
            ProviderResults {
                authserv_id: "mx.example.com".into(),
                spf: Some("pass".into()),
                dkim: Some("none".into()),
                dmarc: Some("none".into()),
            },
            None,
            "Nothing proves this message comes from the address it claims.".to_string(),
        ),
    };
    let reply_to = s
        .headers
        .split("\r\n")
        .find_map(|h| h.strip_prefix("Reply-To: "));
    AuthReport {
        verdict,
        from_domain: domain,
        dkim,
        dmarc_policy: policy,
        provider: Some(provider),
        warnings: heuristics::warnings(s.from_name, s.from_addr, reply_to),
        summary,
    }
}

/// Fill an empty store with one made-up account and its mail.
pub fn seed(conn: &Connection) -> Result<()> {
    let account = Account {
        id: 0,
        email: DEMO_EMAIL.into(),
        display_name: "Alex Rivera".into(),
        imap_host: "imap.example.com".into(),
        imap_port: 993,
        smtp_host: "smtp.example.com".into(),
        smtp_port: 465,
        smtp_starttls: false,
        username: DEMO_EMAIL.into(),
        auth_kind: "password".into(),
        oauth_provider: None,
    };
    let account_id = store::insert_account(conn, &account)?;
    let folders = [
        ("INBOX", "inbox"),
        ("Sent", "sent"),
        ("Drafts", "drafts"),
        ("Archive", "archive"),
        ("Spam", "junk"),
        ("Trash", "trash"),
    ];
    store::sync_folders(
        conn,
        account_id,
        &folders.map(|(n, r)| (n.to_string(), r.to_string())),
    )?;
    let all = store::folders(conn, Some(account_id))?;
    let now = db::now();

    for (index, s) in samples().iter().enumerate() {
        let folder = all
            .iter()
            .find(|f| f.name == s.folder)
            .expect("sample folder exists");
        let raw = raw_message(s, index, now - (s.hours_ago * 3600.0) as i64);
        let msg = parse::parse(&raw).expect("demo message parses");
        let envelope = parse::envelope(&msg);
        let new = NewMessage {
            account_id,
            folder_id: folder.id,
            uid: index as u32 + 1,
            size: raw.len() as u32,
            seen: s.seen,
            flagged: s.flagged,
            answered: s.answered,
            envelope: &envelope,
        };
        let Some(id) = store::insert_message(conn, &new)? else {
            continue;
        };
        let text = parse::any_text(&msg);
        store::store_body(
            conn,
            id,
            &raw,
            &parse::preview(&text),
            !parse::attachments(&msg).is_empty(),
            &text,
        )?;
        if let Some(verdict) = s.verdict {
            store::set_auth(
                conn,
                id,
                verdict.as_str(),
                &serde_json::to_string(&report(s, verdict))?,
            )?;
        }
    }
    store::block(conn, "best-pharma-deals.example")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{key, open, store::ListQuery};
    use zeroize::Zeroizing;

    #[test]
    fn seeds_a_browsable_mailbox() {
        let dir = std::env::temp_dir().join(format!(
            "sobre-demo-{}",
            key::to_hex(&key::random_bytes::<6>())
        ));
        let conn = open(&dir, &Zeroizing::new(key::random_bytes::<32>())).unwrap();
        seed(&conn).unwrap();

        let inbox = store::list_messages(&conn, &ListQuery::default()).unwrap();
        assert_eq!(inbox.len(), 10);
        assert!(inbox.windows(2).all(|w| w[0].date >= w[1].date));
        assert_eq!(inbox[1].from_name, "Clara Jiménez");
        assert!(inbox
            .iter()
            .all(|m| !m.preview.is_empty() && m.auth_verdict.is_some()));
        assert_eq!(inbox.iter().filter(|m| m.has_attachments).count(), 3);
        assert_eq!(store::unread_count(&conn).unwrap(), 3);

        let phish = inbox
            .iter()
            .find(|m| m.auth_verdict.as_deref() == Some("failed"))
            .unwrap();
        let extra = store::message_extra(&conn, phish.id).unwrap().unwrap();
        let detail: AuthReport = serde_json::from_value(extra.auth_detail.unwrap()).unwrap();
        assert_eq!(detail.warnings.len(), 2);

        let news = inbox
            .iter()
            .find(|m| m.from_addr == "hello@fieldweekly.example")
            .unwrap();
        let raw = store::body(&conn, news.id).unwrap().unwrap();
        assert!(parse::list_unsubscribe(&parse::parse(&raw).unwrap())
            .unwrap()
            .one_click
            .is_some());

        let sent = store::list_messages(
            &conn,
            &ListQuery {
                role: Some("sent".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent.iter().all(|m| m.auth_verdict.is_none()));
        std::fs::remove_dir_all(dir).ok();
    }
}
