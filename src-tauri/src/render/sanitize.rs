use std::{
    borrow::Cow,
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use ammonia::{Builder, UrlRelative};

/// Scheme and host the reader iframe is served from.
pub const ORIGIN: &str = "mailbody://localhost";

pub struct SanitizeOptions<'a> {
    pub asset_prefix: &'a str,
    pub allow_remote_images: bool,
}

#[derive(Debug, Default)]
pub struct Sanitized {
    pub body: String,
    pub css: String,
    pub blocked_images: usize,
    pub deceptive_links: Vec<String>,
}

const EXTRA_TAGS: &[&str] = &[
    "font", "center", "u", "s", "strike", "big", "main", "section", "label",
];

const STYLE_PROPERTIES: &[&str] = &[
    "color",
    "background-color",
    "background",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "font-variant",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "vertical-align",
    "white-space",
    "word-break",
    "word-wrap",
    "overflow-wrap",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-color",
    "border-style",
    "border-width",
    "border-radius",
    "border-collapse",
    "border-spacing",
    "width",
    "min-width",
    "max-width",
    "height",
    "min-height",
    "max-height",
    "display",
    "float",
    "clear",
    "table-layout",
    "list-style",
    "list-style-type",
    "direction",
    "opacity",
    "box-sizing",
    "text-overflow",
    "overflow",
    "overflow-x",
    "overflow-y",
];

fn percent_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn scheme_of(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
        .collect();
    let colon = cleaned.find(':')?;
    let scheme = &cleaned[..colon];
    if !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        Some(scheme.to_ascii_lowercase())
    } else {
        None
    }
}

/// CSS values may not pull in anything external or run legacy script hooks.
fn css_value_is_safe(value: &str) -> bool {
    let v: String = value
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    !(v.contains("url(")
        || v.contains("expression(")
        || v.contains("@import")
        || v.contains("javascript:")
        || v.contains("behavior:")
        || v.contains("-moz-binding")
        || v.contains("image-set(")
        || v.contains('\\'))
}

fn filter_stylesheet(css: &str) -> String {
    let css = css.replace("<!--", "").replace("-->", "");
    if css.to_ascii_lowercase().contains("</style") || css.contains('\\') {
        return String::new();
    }
    // `@import ...;` and `@charset ...;` are statements, not blocks.
    let mut statements = String::with_capacity(css.len());
    let mut rest = css.as_str();
    while let Some(at) = rest
        .to_ascii_lowercase()
        .find("@import")
        .or_else(|| rest.to_ascii_lowercase().find("@charset"))
    {
        statements.push_str(&rest[..at]);
        rest = match rest[at..].find(';') {
            Some(end) => &rest[at + end + 1..],
            None => "",
        };
    }
    statements.push_str(rest);

    let mut out = String::with_capacity(statements.len());
    for piece in statements.split_inclusive('}') {
        let compact: String = piece
            .to_ascii_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if compact.contains("@font-face")
            || compact.contains("position:fixed")
            || !css_value_is_safe(piece)
        {
            continue;
        }
        out.push_str(piece);
    }
    // Unbalanced braces mean a rule was cut mid-way; safer to drop everything.
    if out.matches('{').count() != out.matches('}').count() {
        return String::new();
    }
    out
}

fn extract_styles(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let mut body = String::with_capacity(html.len());
    let mut css = String::new();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<style") {
        let start = pos + start;
        body.push_str(&html[pos..start]);
        let Some(open_end) = lower[start..].find('>') else {
            pos = html.len();
            break;
        };
        let content_start = start + open_end + 1;
        let Some(close) = lower[content_start..].find("</style") else {
            pos = html.len();
            break;
        };
        css.push_str(&html[content_start..content_start + close]);
        css.push('\n');
        let after = content_start + close;
        pos = lower[after..]
            .find('>')
            .map(|i| after + i + 1)
            .unwrap_or(html.len());
    }
    body.push_str(&html[pos..]);
    (body, filter_stylesheet(&css))
}

pub fn sanitize_html(html: &str, opts: &SanitizeOptions) -> Sanitized {
    let (html, css) = extract_styles(html);
    let blocked = Arc::new(AtomicUsize::new(0));
    let blocked_in_filter = blocked.clone();
    let prefix = opts.asset_prefix.to_string();
    let allow_remote = opts.allow_remote_images;

    let mut builder = Builder::default();
    builder
        .add_tags(EXTRA_TAGS)
        .add_generic_attributes(&[
            "style", "align", "valign", "bgcolor", "dir", "lang", "title",
        ])
        .add_tag_attributes(
            "table",
            &[
                "width",
                "height",
                "border",
                "cellpadding",
                "cellspacing",
                "role",
            ],
        )
        .add_tag_attributes("td", &["width", "height", "nowrap"])
        .add_tag_attributes("th", &["width", "height", "nowrap"])
        .add_tag_attributes("tr", &["height"])
        .add_tag_attributes("font", &["color", "face", "size"])
        .add_tag_attributes("img", &["width", "height", "alt", "src", "border"])
        .rm_tag_attributes("img", &["srcset"])
        .filter_style_properties(STYLE_PROPERTIES.iter().copied().collect::<HashSet<_>>())
        .url_schemes(
            ["http", "https", "mailto", "cid", "data", "mailbody"]
                .into_iter()
                .collect(),
        )
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer nofollow"))
        .strip_comments(true)
        .attribute_filter(
            move |element, attribute, value| match (element, attribute) {
                ("img", "src") => {
                    let trimmed = value.trim();
                    match scheme_of(trimmed).as_deref() {
                        Some("cid") => {
                            let cid = trimmed[4..].trim_matches(|c| c == '<' || c == '>');
                            Some(Cow::Owned(format!(
                                "{ORIGIN}{prefix}/cid?id={}",
                                percent_encode(cid)
                            )))
                        }
                        Some("data")
                            if trimmed.to_ascii_lowercase().starts_with("data:image/")
                                && !trimmed.to_ascii_lowercase().starts_with("data:image/svg") =>
                        {
                            Some(Cow::Borrowed(value))
                        }
                        Some("http") | Some("https") if allow_remote => Some(Cow::Owned(format!(
                            "{ORIGIN}{prefix}/img?u={}",
                            percent_encode(trimmed)
                        ))),
                        Some("http") | Some("https") => {
                            blocked_in_filter.fetch_add(1, Ordering::Relaxed);
                            None
                        }
                        _ => None,
                    }
                }
                (_, "href") | (_, "cite") => match scheme_of(value).as_deref() {
                    Some("http") | Some("https") | Some("mailto") => Some(Cow::Borrowed(value)),
                    _ => None,
                },
                (_, "style") => css_value_is_safe(value).then_some(Cow::Borrowed(value)),
                _ => Some(Cow::Borrowed(value)),
            },
        );

    let body = builder.clean(&html).to_string();
    let deceptive_links = find_deceptive_links(&body);
    Sanitized {
        body,
        css,
        blocked_images: blocked.load(Ordering::Relaxed),
        deceptive_links,
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape plain text and turn bare http(s) URLs into links.
pub fn plain_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    loop {
        let next = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(start) = next else {
            out.push_str(&escape(rest));
            break;
        };
        out.push_str(&escape(&rest[..start]));
        let tail = &rest[start..];
        let mut end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .unwrap_or(tail.len());
        while end > 0
            && matches!(
                tail.as_bytes()[end - 1],
                b'.' | b',' | b')' | b';' | b':' | b'!' | b'?'
            )
        {
            end -= 1;
        }
        let link = &tail[..end];
        if url::Url::parse(link).is_ok() {
            let safe = escape(link);
            out.push_str(&format!(
                "<a href=\"{safe}\" rel=\"noopener noreferrer nofollow\">{safe}</a>"
            ));
        } else {
            out.push_str(&escape(link));
        }
        rest = &tail[end..];
    }
    out
}

fn strip_tags(fragment: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in fragment.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&nbsp;", " ")
}

pub fn host_tail(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return host;
    }
    // Two-part public suffixes such as co.uk / com.au.
    let second = labels[n - 2];
    if labels[n - 1].len() == 2
        && matches!(second, "co" | "com" | "org" | "net" | "ac" | "gov" | "edu")
    {
        labels[n - 3..].join(".")
    } else {
        labels[n - 2..].join(".")
    }
}

/// If the visible text of a link looks like a URL or host, which host is it?
fn host_named_by_text(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let candidate = if text.contains("://") {
        text.to_string()
    } else {
        format!("http://{text}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    let host = url.host_str()?;
    (host.contains('.') && !host.ends_with('.')).then(|| host.to_string())
}

/// Every link in sanitised HTML as (target, visible text).
fn anchors(sanitized: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = sanitized;
    while let Some(start) = rest.find("<a ") {
        let tag = &rest[start..];
        let Some(tag_end) = tag.find('>') else { break };
        let Some(close) = tag.find("</a>") else { break };
        let attrs = &tag[..tag_end];
        if let Some(h) = attrs.find("href=\"") {
            let value = &attrs[h + 6..];
            if let Some(q) = value.find('"') {
                let href = value[..q].replace("&amp;", "&");
                found.push((href, strip_tags(&tag[tag_end + 1..close.max(tag_end + 1)])));
            }
        }
        rest = &tag[close + 4..];
    }
    found
}

/// Scan sanitised HTML for links such as `<a href="https://evil.example">paypal.com</a>`.
pub fn find_deceptive_links(sanitized: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (href, text) in anchors(sanitized) {
        if let (Some(named), Ok(real)) = (host_named_by_text(&text), url::Url::parse(&href)) {
            if let Some(real_host) = real.host_str() {
                if host_tail(&named) != host_tail(real_host) && !found.contains(&href) {
                    found.push(href);
                }
            }
        }
    }
    found
}

const UNSUBSCRIBE_WORDS: &[&str] = &[
    "unsubscribe",
    "opt out",
    "opt-out",
    "darse de baja",
    "darte de baja",
    "dar de baja",
    "cancelar la suscripci",
    "cancelar suscripci",
    "anular la suscripci",
    "desuscrib",
    "se désabonner",
    "désinscri",
    "abbestellen",
    "abmelden",
    "annulla l'iscrizione",
    "disiscriv",
    "cancelar inscri",
    "descadastr",
];

pub fn find_unsubscribe_link(sanitized: &str) -> Option<String> {
    anchors(sanitized)
        .into_iter()
        .filter(|(href, text)| {
            let text = text.to_lowercase();
            href.starts_with("http") && UNSUBSCRIBE_WORDS.iter().any(|w| text.contains(w))
        })
        .map(|(href, _)| href)
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> Sanitized {
        sanitize_html(
            html,
            &SanitizeOptions {
                asset_prefix: "/t/1",
                allow_remote_images: false,
            },
        )
    }

    #[test]
    fn hostile_markup_is_removed() {
        let cases = [
            "<script>alert(1)</script>",
            "<img src=x onerror=alert(1)>",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a href=\"JaVa\tScRiPt:alert(1)\">x</a>",
            "<a href=\"data:text/html,<script>alert(1)</script>\">x</a>",
            "<iframe src=\"https://evil.example\"></iframe>",
            "<form action=\"https://evil.example\"><input name=p><button>go</button></form>",
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            "<base href=\"https://evil.example/\">",
            "<object data=\"x.swf\"></object><embed src=\"x.swf\">",
            "<svg><script>alert(1)</script></svg>",
            "<svg onload=alert(1)>",
            "<link rel=stylesheet href=\"https://evil.example/x.css\">",
            "<div style=\"background:url(https://evil.example/t.gif)\">x</div>",
            "<div style=\"width: expression(alert(1))\">x</div>",
            "<p style=\"position:fixed;top:0;left:0\">overlay</p>",
            "<a href=\"mailbody://localhost/anything\">x</a>",
            "<a href=\"/relative\">x</a>",
            "<img src=\"data:image/svg+xml,<svg onload=alert(1)>\">",
            "<body onload=alert(1)>hi</body>",
            "<math><mtext><script>alert(1)</script></mtext></math>",
        ];
        for case in cases {
            let out = clean(case);
            let all = format!("{}{}", out.body, out.css).to_ascii_lowercase();
            for bad in [
                "<script",
                "onerror",
                "onload",
                "javascript:",
                "<iframe",
                "<form",
                "<input",
                "<meta",
                "<base",
                "<object",
                "<embed",
                "<link",
                "url(",
                "expression",
                "position",
                "data:text",
                "mailbody://localhost/anything",
                "href=\"/relative",
                "svg",
            ] {
                assert!(!all.contains(bad), "{bad:?} survived in {case:?}: {all}");
            }
        }
    }

    #[test]
    fn remote_images_are_blocked_until_allowed() {
        let html = "<img src=\"https://tracker.example/p.gif?u=1\"><img src=\"cid:logo@x\">";
        let blocked = clean(html);
        assert_eq!(blocked.blocked_images, 1);
        assert!(!blocked.body.contains("tracker.example"));
        assert!(blocked
            .body
            .contains("mailbody://localhost/t/1/cid?id=logo%40x"));

        let allowed = sanitize_html(
            html,
            &SanitizeOptions {
                asset_prefix: "/t/1",
                allow_remote_images: true,
            },
        );
        assert_eq!(allowed.blocked_images, 0);
        assert!(allowed.body.contains(
            "mailbody://localhost/t/1/img?u=https%3A%2F%2Ftracker.example%2Fp.gif%3Fu%3D1"
        ));
        // Never the raw remote URL: every fetch goes through the proxy.
        assert!(!allowed.body.contains("src=\"https://"));
    }

    #[test]
    fn style_blocks_are_filtered() {
        let out = clean(
            "<style>@import url(https://evil.example/a.css); .a{color:red} .b{background:url(https://evil.example/x)} .c{font-weight:bold}</style><p class=a>hi</p>",
        );
        assert!(out.css.contains(".a{color:red}"));
        assert!(out.css.contains(".c{font-weight:bold}"));
        assert!(!out.css.contains("evil.example"));
        assert!(!out.body.contains("color:red"));
        assert!(clean("<style>a{color:red}</style><script>x</style>")
            .css
            .contains("color:red"));
        assert_eq!(
            clean("<style>a{} </STYLE ><style>x{y:z} </style")
                .css
                .contains("</"),
            false
        );
    }

    #[test]
    fn ordinary_formatting_survives() {
        let out = clean("<table width=\"600\"><tr><td style=\"color:#333;padding:4px\"><b>Hi</b> <a href=\"https://example.com/a?b=1\">link</a></td></tr></table>");
        assert!(out.body.contains("<table width=\"600\">"));
        assert!(out.body.contains("color:#333"));
        assert!(out.body.contains("href=\"https://example.com/a?b=1\""));
        assert!(out.body.contains("noopener"));
    }

    #[test]
    fn plain_text_is_escaped_and_linkified() {
        let out = plain_to_html("see <b>https://example.com/x?a=1&b=2</b>, ok. <script>");
        assert!(out.contains("&lt;b&gt;"));
        assert!(out.contains("<a href=\"https://example.com/x?a=1&amp;b=2\""));
        assert!(out.contains("&lt;script&gt;"));
        assert!(!out.contains("<script"));
    }

    #[test]
    fn deceptive_links_are_reported() {
        let out = clean(concat!(
            "<a href=\"https://evil.example/login\">https://www.paypal.com/signin</a>",
            "<a href=\"https://www.paypal.com/x\">paypal.com</a>",
            "<a href=\"https://paypal.com.evil.example/\"><b>paypal.com</b></a>",
            "<a href=\"https://news.example.org/\">Read more</a>",
        ));
        assert_eq!(
            out.deceptive_links,
            vec![
                "https://evil.example/login",
                "https://paypal.com.evil.example/"
            ]
        );
    }

    #[test]
    fn unsubscribe_link_in_body() {
        let out = clean("<p><a href=\"https://shop.example/sale\">Big sale</a></p><p>Not interested? <a href=\"https://shop.example/u?t=1&amp;x=2\"><u>Unsubscribe</u></a> or <a href=\"https://shop.example/prefs\">manage preferences</a></p>");
        assert_eq!(
            find_unsubscribe_link(&out.body).as_deref(),
            Some("https://shop.example/u?t=1&x=2")
        );
        assert_eq!(
            find_unsubscribe_link(&clean("<a href=\"https://x.example/b\">Darse de baja</a>").body)
                .as_deref(),
            Some("https://x.example/b")
        );
        assert!(
            find_unsubscribe_link(&clean("<a href=\"https://x.example/\">Read more</a>").body)
                .is_none()
        );
        assert!(
            find_unsubscribe_link(&clean("<a href=\"mailto:a@b.c\">unsubscribe</a>").body)
                .is_none()
        );
    }

    #[test]
    fn host_tails() {
        assert_eq!(host_tail("mail.google.com"), "google.com");
        assert_eq!(host_tail("a.b.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(host_tail("example.com"), "example.com");
    }
}
