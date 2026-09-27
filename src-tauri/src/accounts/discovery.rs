use serde::{Deserialize, Serialize};

use crate::{mail::parse::domain_of, render::sanitize::host_tail};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_starttls: bool,
    pub oauth_provider: Option<String>,
    pub source: String,
}

fn config(
    imap: &str,
    smtp: &str,
    smtp_port: u16,
    oauth: Option<&str>,
    source: &str,
) -> ServerConfig {
    ServerConfig {
        imap_host: imap.into(),
        imap_port: 993,
        smtp_host: smtp.into(),
        smtp_port,
        smtp_starttls: smtp_port == 587,
        oauth_provider: oauth.map(str::to_string),
        source: source.into(),
    }
}

/// Providers keyed by mailbox domain.
pub fn builtin(domain: &str) -> Option<ServerConfig> {
    let c = |imap, smtp, port, oauth| Some(config(imap, smtp, port, oauth, "builtin"));
    match domain {
        "gmail.com" | "googlemail.com" => {
            c("imap.gmail.com", "smtp.gmail.com", 465, Some("google"))
        }
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "outlook.es" | "hotmail.es" => c(
            "outlook.office365.com",
            "smtp-mail.outlook.com",
            587,
            Some("microsoft"),
        ),
        "yahoo.com" | "yahoo.es" | "ymail.com" => {
            c("imap.mail.yahoo.com", "smtp.mail.yahoo.com", 465, None)
        }
        "aol.com" => c("imap.aol.com", "smtp.aol.com", 465, None),
        "icloud.com" | "me.com" | "mac.com" => c("imap.mail.me.com", "smtp.mail.me.com", 587, None),
        "fastmail.com" | "fastmail.fm" => c("imap.fastmail.com", "smtp.fastmail.com", 465, None),
        "zoho.com" => c("imap.zoho.com", "smtp.zoho.com", 465, None),
        "zoho.eu" => c("imap.zoho.eu", "smtp.zoho.eu", 465, None),
        "gmx.com" | "gmx.us" => c("imap.gmx.com", "mail.gmx.com", 465, None),
        "gmx.net" | "gmx.de" | "gmx.es" => c("imap.gmx.net", "mail.gmx.net", 465, None),
        "web.de" => c("imap.web.de", "smtp.web.de", 587, None),
        "mailbox.org" => c("imap.mailbox.org", "smtp.mailbox.org", 465, None),
        "posteo.de" | "posteo.net" => c("posteo.de", "posteo.de", 465, None),
        _ => None,
    }
}

/// Hosted domains reveal their provider through MX records.
pub fn from_mx(mx_host: &str) -> Option<ServerConfig> {
    let c = |imap, smtp, port, oauth| Some(config(imap, smtp, port, oauth, "mx"));
    match host_tail(mx_host).as_str() {
        "google.com" | "googlemail.com" => {
            c("imap.gmail.com", "smtp.gmail.com", 465, Some("google"))
        }
        "outlook.com" => c(
            "outlook.office365.com",
            "smtp.office365.com",
            587,
            Some("microsoft"),
        ),
        "messagingengine.com" => c("imap.fastmail.com", "smtp.fastmail.com", 465, None),
        "zoho.com" => c("imap.zoho.com", "smtp.zoho.com", 465, None),
        "zoho.eu" => c("imap.zoho.eu", "smtp.zoho.eu", 465, None),
        "yahoodns.net" => c("imap.mail.yahoo.com", "smtp.mail.yahoo.com", 465, None),
        "icloud.com" => c("imap.mail.me.com", "smtp.mail.me.com", 587, None),
        _ => None,
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    Some(&tag[start..start + tag[start..].find('"')?])
}

fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    Some(xml[start..start + xml[start..].find(&format!("</{name}>"))?].trim())
}

/// Parse a Thunderbird autoconfig document.
pub fn parse_autoconfig(xml: &str) -> Option<ServerConfig> {
    let mut imap = None;
    let mut rest = xml;
    while let Some(i) = rest.find("<incomingServer") {
        let block = &rest[i..];
        let end = block.find("</incomingServer>")?;
        let server = &block[..end];
        let tag = &server[..server.find('>')?];
        if attr(tag, "type") == Some("imap") && element(server, "socketType") == Some("SSL") {
            imap = Some((
                element(server, "hostname")?.to_string(),
                element(server, "port")?.parse::<u16>().ok()?,
            ));
            break;
        }
        rest = &block[end..];
    }
    let (imap_host, imap_port) = imap?;

    let mut smtp = None;
    let mut rest = xml;
    while let Some(i) = rest.find("<outgoingServer") {
        let block = &rest[i..];
        let end = block.find("</outgoingServer>")?;
        let server = &block[..end];
        let socket = element(server, "socketType").unwrap_or("");
        if matches!(socket, "SSL" | "STARTTLS") {
            smtp = Some((
                element(server, "hostname")?.to_string(),
                element(server, "port")?.parse::<u16>().ok()?,
                socket == "STARTTLS",
            ));
            break;
        }
        rest = &block[end..];
    }
    let (smtp_host, smtp_port, smtp_starttls) = smtp?;
    let oauth_provider = match host_tail(&imap_host).as_str() {
        "gmail.com" | "googlemail.com" => Some("google".to_string()),
        "office365.com" | "outlook.com" => Some("microsoft".to_string()),
        _ => None,
    };
    Some(ServerConfig {
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
        smtp_starttls,
        oauth_provider,
        source: "autoconfig".into(),
    })
}

async fn mx_host(domain: &str) -> Option<String> {
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .ok()?
        .build()
        .ok()?;
    let lookup = resolver.mx_lookup(format!("{domain}.")).await.ok()?;
    lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            hickory_resolver::proto::rr::RData::MX(mx) => {
                Some((mx.preference, mx.exchange.to_utf8()))
            }
            _ => None,
        })
        .min_by_key(|(pref, _)| *pref)
        .map(|(_, host)| host.trim_end_matches('.').to_ascii_lowercase())
}

pub async fn discover(email: &str) -> ServerConfig {
    let domain = domain_of(email).to_ascii_lowercase();
    if let Some(found) = builtin(&domain) {
        return found;
    }
    // Only the domain is sent to the autoconfig database, never the address.
    let url = format!("https://autoconfig.thunderbird.net/v1.1/{domain}");
    if let Ok(reply) = crate::net::fetch_public(&url, 256 * 1024).await {
        if let Some(found) = parse_autoconfig(&String::from_utf8_lossy(&reply.data)) {
            return found;
        }
    }
    if let Some(mx) = mx_host(&domain).await {
        if let Some(found) = from_mx(&mx) {
            return found;
        }
    }
    config(
        &format!("imap.{domain}"),
        &format!("smtp.{domain}"),
        465,
        None,
        "guess",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_providers() {
        let g = builtin("gmail.com").unwrap();
        assert_eq!(
            (g.imap_host.as_str(), g.oauth_provider.as_deref()),
            ("imap.gmail.com", Some("google"))
        );
        assert!(builtin("example.org").is_none());
        assert_eq!(
            from_mx("aspmx.l.google.com").unwrap().imap_host,
            "imap.gmail.com"
        );
        assert_eq!(
            from_mx("example-org.mail.protection.outlook.com")
                .unwrap()
                .oauth_provider
                .as_deref(),
            Some("microsoft")
        );
    }

    #[test]
    fn autoconfig_document() {
        let xml = r#"<clientConfig version="1.1"><emailProvider id="example.org">
            <incomingServer type="pop3"><hostname>pop.example.org</hostname><port>995</port><socketType>SSL</socketType></incomingServer>
            <incomingServer type="imap"><hostname>mail.example.org</hostname><port>993</port><socketType>SSL</socketType><username>%EMAILADDRESS%</username></incomingServer>
            <outgoingServer type="smtp"><hostname>send.example.org</hostname><port>587</port><socketType>STARTTLS</socketType></outgoingServer>
            </emailProvider></clientConfig>"#;
        let c = parse_autoconfig(xml).unwrap();
        assert_eq!(
            (c.imap_host.as_str(), c.imap_port),
            ("mail.example.org", 993)
        );
        assert_eq!(
            (c.smtp_host.as_str(), c.smtp_port, c.smtp_starttls),
            ("send.example.org", 587, true)
        );
        assert!(parse_autoconfig("<html>not found</html>").is_none());
    }
}
