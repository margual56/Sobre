pub mod detect;
pub mod sanitize;

use pulldown_cmark::{html, Options, Parser};
use serde::Serialize;

pub use detect::RenderKind;
use sanitize::{escape, plain_to_html, sanitize_html, SanitizeOptions};

pub const BODY_CSP: &str =
    "default-src 'none'; img-src 'self' mailbody: data:; style-src 'unsafe-inline'; \
                            form-action 'none'; base-uri 'none'; frame-ancestors *";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Auto,
    Plain,
    Source,
}

pub struct BodyParts<'a> {
    pub html: Option<&'a str>,
    pub text: Option<&'a str>,
    pub raw: &'a [u8],
}

#[derive(Debug, Serialize)]
pub struct Rendered {
    pub kind: RenderKind,
    pub document: String,
    pub blocked_images: usize,
    pub deceptive_links: Vec<String>,
}

pub fn markdown_to_html(text: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(text, options));
    out
}

pub fn kind_of(parts: &BodyParts) -> RenderKind {
    match (parts.html, parts.text) {
        (Some(h), _) if !h.trim().is_empty() => RenderKind::Html,
        (_, Some(t)) if detect::looks_like_markdown(t) => RenderKind::Markdown,
        _ => RenderKind::Plain,
    }
}

const BASE_CSS: &str = "
html { color-scheme: light; }
body { margin: 16px; font: 15px/1.5 system-ui, sans-serif; color: #1c1c1e; background: #fff;
       overflow-wrap: anywhere; }
img { max-width: 100%; height: auto; }
table { max-width: 100%; }
a { color: #0b61d6; }
pre, code { font-family: ui-monospace, monospace; font-size: 13px; }
pre { white-space: pre-wrap; }
blockquote { margin: 0 0 0 4px; padding-left: 12px; border-left: 3px solid #c9c9cf; color: #55555c; }
body.own.dark { color: #e6e6ea; background: #1b1b1f; }
body.own.dark a { color: #7fb2ff; }
body.own.dark blockquote { border-color: #44444c; color: #a5a5ad; }
body.own table { border-collapse: collapse; }
body.own td, body.own th { border: 1px solid #8884; padding: 4px 8px; }
";

fn page(body: &str, mail_css: &str, classes: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{BODY_CSP}\">\
         <style>{BASE_CSS}</style><style>{mail_css}</style></head>\
         <body class=\"{classes}\">{body}</body></html>"
    )
}

/// Build the full document shown in the reader iframe.
pub fn render(parts: &BodyParts, view: View, opts: &SanitizeOptions, dark: bool) -> Rendered {
    // HTML mail assumes a white page; only our own renderings follow the theme.
    let own = if dark { "own dark" } else { "own" };
    let plain = |text: &str, kind| Rendered {
        kind,
        document: page(&format!("<pre>{}</pre>", plain_to_html(text)), "", own),
        blocked_images: 0,
        deceptive_links: Vec::new(),
    };

    match view {
        View::Source => {
            let source = String::from_utf8_lossy(parts.raw);
            Rendered {
                kind: RenderKind::Plain,
                document: page(&format!("<pre>{}</pre>", escape(&source)), "", own),
                blocked_images: 0,
                deceptive_links: Vec::new(),
            }
        }
        View::Plain => plain(parts.text.unwrap_or(""), RenderKind::Plain),
        View::Auto => match kind_of(parts) {
            RenderKind::Html => {
                let clean = sanitize_html(parts.html.unwrap_or(""), opts);
                Rendered {
                    kind: RenderKind::Html,
                    document: page(&clean.body, &clean.css, "mail"),
                    blocked_images: clean.blocked_images,
                    deceptive_links: clean.deceptive_links,
                }
            }
            RenderKind::Markdown => {
                // Markdown may embed raw HTML, so it takes the same path as HTML mail.
                let clean = sanitize_html(&markdown_to_html(parts.text.unwrap_or("")), opts);
                Rendered {
                    kind: RenderKind::Markdown,
                    document: page(&clean.body, "", own),
                    blocked_images: clean.blocked_images,
                    deceptive_links: clean.deceptive_links,
                }
            }
            RenderKind::Plain => plain(parts.text.unwrap_or(""), RenderKind::Plain),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTS: SanitizeOptions = SanitizeOptions {
        asset_prefix: "/t/1",
        allow_remote_images: false,
    };

    #[test]
    fn picks_the_right_renderer() {
        let html = BodyParts {
            html: Some("<p>hi</p>"),
            text: Some("hi"),
            raw: b"",
        };
        assert_eq!(
            render(&html, View::Auto, &OPTS, false).kind,
            RenderKind::Html
        );
        let md = BodyParts {
            html: None,
            text: Some("# Title\n\nSome **bold** and [a link](https://example.com)."),
            raw: b"",
        };
        let out = render(&md, View::Auto, &OPTS, false);
        assert_eq!(out.kind, RenderKind::Markdown);
        assert!(out.document.contains("<h1>Title</h1>"));
        let text = BodyParts {
            html: None,
            text: Some("just words"),
            raw: b"",
        };
        assert_eq!(
            render(&text, View::Auto, &OPTS, false).kind,
            RenderKind::Plain
        );
    }

    #[test]
    fn markdown_cannot_smuggle_script() {
        let md = BodyParts { html: None, text: Some("# T\n\n**b** [x](javascript:alert(1))\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>"), raw: b"" };
        let out = render(&md, View::Auto, &OPTS, false);
        let body = out
            .document
            .split("<body")
            .nth(1)
            .unwrap()
            .to_ascii_lowercase();
        assert!(
            !body.contains("<script") && !body.contains("javascript:") && !body.contains("onerror")
        );
    }

    #[test]
    fn source_view_is_inert() {
        let parts = BodyParts {
            html: Some("<script>x</script>"),
            text: None,
            raw: b"Subject: x\r\n\r\n<script>alert(1)</script>",
        };
        let out = render(&parts, View::Source, &OPTS, true);
        assert!(out.document.contains("&lt;script&gt;alert(1)"));
    }
}
