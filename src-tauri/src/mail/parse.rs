use mail_parser::{Message, MessageParser, MimeHeaders, PartType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub addr: String,
}

#[derive(Debug, Clone, Default)]
pub struct Envelope {
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub subject: String,
    pub from: Contact,
    pub to: Vec<Contact>,
    pub cc: Vec<Contact>,
    pub reply_to: Option<String>,
    pub date: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentMeta {
    pub index: u32,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

pub fn parse(raw: &[u8]) -> Option<Message<'_>> {
    MessageParser::default().parse(raw)
}

fn contacts(addr: Option<&mail_parser::Address>) -> Vec<Contact> {
    addr.map(|a| {
        a.iter()
            .filter_map(|x| {
                Some(Contact {
                    name: x.name().unwrap_or_default().trim().to_string(),
                    addr: x.address()?.trim().to_ascii_lowercase(),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Value of a header by name, unfolded.
pub fn raw_header(msg: &Message, name: &str) -> Option<String> {
    msg.headers_raw()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn raw_headers_all(msg: &Message, name: &str) -> Vec<String> {
    msg.headers_raw()
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

pub fn envelope(msg: &Message) -> Envelope {
    Envelope {
        message_id: msg.message_id().map(str::to_string),
        in_reply_to: raw_header(msg, "In-Reply-To"),
        references: raw_header(msg, "References"),
        subject: msg.subject().unwrap_or_default().trim().to_string(),
        from: contacts(msg.from()).into_iter().next().unwrap_or_default(),
        to: contacts(msg.to()),
        cc: contacts(msg.cc()),
        reply_to: contacts(msg.reply_to()).into_iter().next().map(|c| c.addr),
        date: msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
    }
}

/// The real HTML part, if the sender wrote one (not a conversion of the text part).
pub fn html_body(msg: &Message) -> Option<String> {
    msg.html_bodies().find_map(|p| match &p.body {
        PartType::Html(h) => Some(h.to_string()),
        _ => None,
    })
}

/// The real plain-text part, if any.
pub fn text_body(msg: &Message) -> Option<String> {
    msg.text_bodies().find_map(|p| match &p.body {
        PartType::Text(t) => Some(t.to_string()),
        _ => None,
    })
}

/// Text for previews and the search index, whatever the message is made of.
pub fn any_text(msg: &Message) -> String {
    text_body(msg)
        .or_else(|| msg.body_text(0).map(|t| t.to_string()))
        .unwrap_or_default()
}

pub fn preview(text: &str) -> String {
    let mut out = String::with_capacity(200);
    for word in text.split_whitespace() {
        // Long runs of separators or tracking URLs add nothing to a preview.
        if word.len() > 60 || word.chars().all(|c| !c.is_alphanumeric()) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.chars().count() >= 180 {
            break;
        }
    }
    out.chars().take(180).collect()
}

pub fn attachments(msg: &Message) -> Vec<AttachmentMeta> {
    msg.attachments()
        .enumerate()
        // Inline images are shown in the body, not listed.
        .filter(|(_, p)| !(p.content_id().is_some() && is_inline(p)))
        .map(|(i, p)| AttachmentMeta {
            index: i as u32,
            name: p.attachment_name().unwrap_or("attachment").to_string(),
            mime: mime_of(p),
            size: p.contents().len(),
        })
        .collect()
}

fn is_inline(part: &mail_parser::MessagePart) -> bool {
    part.content_disposition()
        .map(|d| d.is_inline())
        .unwrap_or(true)
        && mime_of(part).starts_with("image/")
}

pub fn mime_of(part: &mail_parser::MessagePart) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub).to_ascii_lowercase(),
            None => ct.ctype().to_ascii_lowercase(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
}

/// Find an inline part by Content-ID.
pub fn part_by_cid<'a>(msg: &'a Message, cid: &str) -> Option<&'a mail_parser::MessagePart<'a>> {
    let wanted = cid.trim_matches(|c| c == '<' || c == '>');
    msg.parts.iter().find(|p| {
        p.content_id()
            .map(|id| id.trim_matches(|c| c == '<' || c == '>') == wanted)
            .unwrap_or(false)
    })
}

/// What the sender offers for leaving its list (RFC 2369 and RFC 8058).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListUnsubscribe {
    pub one_click: Option<String>,
    pub link: Option<String>,
    pub mailto: Option<String>,
}

pub fn list_unsubscribe(msg: &Message) -> Option<ListUnsubscribe> {
    let header = raw_header(msg, "List-Unsubscribe")?;
    let one_click_ok = raw_header(msg, "List-Unsubscribe-Post")
        .map(|v| {
            v.replace(' ', "")
                .eq_ignore_ascii_case("List-Unsubscribe=One-Click")
        })
        .unwrap_or(false);
    let mut out = ListUnsubscribe::default();
    for piece in header.split('<').skip(1) {
        // Folding may have left spaces inside a long URL.
        let target: String = piece
            .split('>')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let lower = target.to_ascii_lowercase();
        if lower.starts_with("https://") {
            if one_click_ok && out.one_click.is_none() {
                out.one_click = Some(target.clone());
            }
            out.link.get_or_insert(target);
        } else if lower.starts_with("http://") {
            out.link.get_or_insert(target);
        } else if lower.starts_with("mailto:") && target.contains('@') {
            out.mailto.get_or_insert(target);
        }
    }
    (out != ListUnsubscribe::default()).then_some(out)
}

/// Split a `mailto:` URL into (address, subject, body).
pub fn parse_mailto(mailto: &str) -> Option<(String, String, String)> {
    let url = url::Url::parse(mailto).ok()?;
    if url.scheme() != "mailto" {
        return None;
    }
    let decode = |s: &str| {
        url::form_urlencoded::parse(s.replace('+', "%2B").as_bytes())
            .next()
            .map(|(k, _)| k.into_owned())
            .unwrap_or_default()
    };
    let addr = decode(url.path())
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if !addr.contains('@') {
        return None;
    }
    let field = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.into_owned())
    };
    Some((
        addr,
        field("subject").unwrap_or_else(|| "unsubscribe".into()),
        field("body").unwrap_or_default(),
    ))
}

pub fn domain_of(addr: &str) -> &str {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTIPART: &[u8] = b"From: \"Ana Ruiz\" <Ana@Example.com>\r\nTo: me@example.org, Bob <bob@example.org>\r\nReply-To: other@example.net\r\nSubject: Hola\r\nMessage-ID: <abc@example.com>\r\nDate: Fri, 09 Oct 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: multipart/alternative; boundary=\"c\"\r\n\r\n--c\r\nContent-Type: text/plain\r\n\r\nplain body here\r\n--c\r\nContent-Type: text/html\r\n\r\n<p>html body</p>\r\n--c--\r\n--b\r\nContent-Type: application/pdf; name=\"doc.pdf\"\r\nContent-Disposition: attachment; filename=\"doc.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERg==\r\n--b--\r\n";

    #[test]
    fn envelope_and_bodies() {
        let msg = parse(MULTIPART).unwrap();
        let env = envelope(&msg);
        assert_eq!(
            env.from,
            Contact {
                name: "Ana Ruiz".into(),
                addr: "ana@example.com".into()
            }
        );
        assert_eq!(env.to.len(), 2);
        assert_eq!(env.reply_to.as_deref(), Some("other@example.net"));
        assert_eq!(env.subject, "Hola");
        assert!(env.date > 1_700_000_000);
        assert_eq!(html_body(&msg).unwrap().trim(), "<p>html body</p>");
        assert_eq!(text_body(&msg).unwrap().trim(), "plain body here");
        let atts = attachments(&msg);
        assert_eq!(atts.len(), 1);
        assert_eq!(
            (atts[0].name.as_str(), atts[0].mime.as_str(), atts[0].size),
            ("doc.pdf", "application/pdf", 4)
        );
    }

    #[test]
    fn text_only_message_has_no_html_body() {
        let msg = parse(b"From: a@b.c\r\nSubject: x\r\n\r\njust text\r\n").unwrap();
        assert!(html_body(&msg).is_none());
        assert_eq!(text_body(&msg).unwrap().trim(), "just text");
    }

    #[test]
    fn list_unsubscribe_header() {
        let raw = b"From: news@example.com\r\nList-Unsubscribe: <mailto:leave@example.com?subject=unsub%20me>,\r\n <https://example.com/u?id=abc\r\n def>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\nhi";
        let u = list_unsubscribe(&parse(raw).unwrap()).unwrap();
        assert_eq!(
            u.one_click.as_deref(),
            Some("https://example.com/u?id=abcdef")
        );
        assert_eq!(
            u.mailto.as_deref(),
            Some("mailto:leave@example.com?subject=unsub%20me")
        );
        assert_eq!(
            parse_mailto(u.mailto.as_deref().unwrap()).unwrap(),
            ("leave@example.com".into(), "unsub me".into(), String::new())
        );

        // Without the Post header a link is only a page to visit.
        let plain = list_unsubscribe(
            &parse(b"From: a@b.c\r\nList-Unsubscribe: <https://example.com/u>\r\n\r\nhi").unwrap(),
        )
        .unwrap();
        assert_eq!(
            (plain.one_click, plain.link.as_deref()),
            (None, Some("https://example.com/u"))
        );
        // One-click is never offered over plain http.
        let http = list_unsubscribe(&parse(b"From: a@b.c\r\nList-Unsubscribe: <http://example.com/u>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\nhi").unwrap()).unwrap();
        assert!(http.one_click.is_none());
        assert!(list_unsubscribe(
            &parse(b"From: a@b.c\r\nList-Unsubscribe: <javascript:alert(1)>\r\n\r\nhi").unwrap()
        )
        .is_none());
        assert!(list_unsubscribe(&parse(b"From: a@b.c\r\n\r\nhi").unwrap()).is_none());
        assert!(parse_mailto("mailto:nobody").is_none());
    }

    #[test]
    fn preview_skips_noise() {
        let p = preview("Hello   there\n\n-----\nhttps://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa world");
        assert_eq!(p, "Hello there world");
    }
}
