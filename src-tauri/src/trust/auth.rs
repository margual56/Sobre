use mail_auth::{AuthenticatedMessage, DkimResult, MessageAuthenticator};
use serde::{Deserialize, Serialize};

use crate::{
    mail::parse::{self, domain_of},
    render::sanitize::host_tail,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Verified,
    Unverified,
    Failed,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Unverified => "unverified",
            Verdict::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DkimCheck {
    pub domain: String,
    pub selector: String,
    pub result: String,
    pub detail: String,
    pub aligned: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderResults {
    pub authserv_id: String,
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthReport {
    pub verdict: Verdict,
    pub from_domain: String,
    pub dkim: Vec<DkimCheck>,
    pub dmarc_policy: Option<String>,
    pub provider: Option<ProviderResults>,
    pub warnings: Vec<String>,
    pub summary: String,
}

/// Parse one `Authentication-Results` header value (RFC 8601).
pub fn parse_authentication_results(value: &str) -> Option<ProviderResults> {
    // Comments can contain semicolons and equals signs; drop them first.
    let mut clean = String::with_capacity(value.len());
    let mut depth = 0;
    for c in value.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    let mut parts = clean.split(';');
    let authserv_id = parts
        .next()?
        .split_whitespace()
        .next()?
        .to_ascii_lowercase();
    let mut out = ProviderResults {
        authserv_id,
        ..Default::default()
    };
    for part in parts {
        let Some(first) = part.split_whitespace().next() else {
            continue;
        };
        let Some((method, result)) = first.split_once('=') else {
            continue;
        };
        let result = result.to_ascii_lowercase();
        let slot = match method.to_ascii_lowercase().as_str() {
            "spf" => &mut out.spf,
            "dkim" => &mut out.dkim,
            "dmarc" => &mut out.dmarc,
            _ => continue,
        };
        // Several DKIM results may be listed; a pass wins.
        if slot.as_deref() != Some("pass") {
            *slot = Some(result);
        }
    }
    Some(out)
}

const PROVIDER_AUTHSERV: &[(&str, &[&str])] = &[
    ("gmail.com", &["google.com"]),
    ("googlemail.com", &["google.com"]),
    ("office365.com", &["outlook.com", "microsoft.com"]),
    ("outlook.com", &["outlook.com", "microsoft.com"]),
    ("yahoo.com", &["yahoo.com", "yahoodns.net"]),
    ("aol.com", &["aol.com", "yahoo.com"]),
    ("me.com", &["icloud.com", "apple.com", "me.com"]),
    ("icloud.com", &["icloud.com", "apple.com", "me.com"]),
    ("fastmail.com", &["messagingengine.com", "fastmail.com"]),
    ("zoho.com", &["zoho.com"]),
    ("zoho.eu", &["zoho.eu"]),
    ("gmx.net", &["gmx.net"]),
    ("gmx.com", &["gmx.com", "gmx.net"]),
    ("web.de", &["web.de"]),
];

/// A header is only believed when it was written by the server we fetch from.
pub fn authserv_is_trusted(authserv_id: &str, imap_host: &str) -> bool {
    let id_tail = host_tail(authserv_id);
    let imap_tail = host_tail(imap_host);
    if id_tail == imap_tail {
        return true;
    }
    PROVIDER_AUTHSERV
        .iter()
        .any(|(imap, ids)| *imap == imap_tail && ids.contains(&id_tail.as_str()))
}

pub fn provider_results(headers: &[String], imap_host: &str) -> Option<ProviderResults> {
    let top = parse_authentication_results(headers.first()?)?;
    authserv_is_trusted(&top.authserv_id, imap_host).then_some(top)
}

fn parse_dmarc_policy(record: &str) -> Option<String> {
    if !record
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("v=dmarc1")
    {
        return None;
    }
    record.split(';').find_map(|tag| {
        let (k, v) = tag.split_once('=')?;
        (k.trim().eq_ignore_ascii_case("p")).then(|| v.trim().to_ascii_lowercase())
    })
}

async fn dmarc_policy(resolver: &MessageAuthenticator, from_domain: &str) -> Option<String> {
    let org = host_tail(from_domain);
    let mut names = vec![format!("_dmarc.{from_domain}.")];
    if org != from_domain {
        names.push(format!("_dmarc.{org}."));
    }
    for name in names {
        if let Ok(raw) = resolver.txt_raw_lookup(name).await {
            if let Some(policy) = parse_dmarc_policy(&String::from_utf8_lossy(&raw)) {
                return Some(policy);
            }
        }
    }
    None
}

/// Combine the evidence into one verdict. Pure, so it can be tested offline.
pub fn decide(
    dkim: &[DkimCheck],
    provider: Option<&ProviderResults>,
    dmarc_policy: Option<&str>,
) -> (Verdict, String) {
    let aligned_pass = dkim.iter().find(|d| d.aligned && d.result == "pass");
    let aligned_fail = dkim.iter().any(|d| d.aligned && d.result == "fail");
    let provider_dmarc = provider.and_then(|p| p.dmarc.as_deref());
    let enforcing = matches!(dmarc_policy, Some("reject") | Some("quarantine"));

    if let Some(sig) = aligned_pass {
        return (
            Verdict::Verified,
            format!("Signed by {} and the signature is valid.", sig.domain),
        );
    }
    if provider_dmarc == Some("pass") {
        return (
            Verdict::Verified,
            "Your mail provider confirmed the sender's domain (DMARC pass).".into(),
        );
    }
    if provider_dmarc == Some("fail") {
        return (
            Verdict::Failed,
            "Your mail provider reports that this message fails the sender domain's DMARC check."
                .into(),
        );
    }
    if aligned_fail {
        return (
            Verdict::Failed,
            "The sender domain's signature does not match the message: it was altered or forged."
                .into(),
        );
    }
    if enforcing && provider.is_none() {
        // DMARC can also pass on SPF alone, which a mail client cannot check,
        // so a missing signature is not proof of forgery.
        return (
            Verdict::Unverified,
            "The sender's domain asks receivers to check its mail, and this message carries no signature from it. It may still have been accepted on other grounds.".into(),
        );
    }
    if dkim.iter().any(|d| d.result == "pass") {
        let d = dkim.iter().find(|d| d.result == "pass").unwrap();
        return (
            Verdict::Unverified,
            format!(
                "Signed by {}, which is not the domain shown in the From address.",
                d.domain
            ),
        );
    }
    (
        Verdict::Unverified,
        "Nothing proves this message comes from the address it claims.".into(),
    )
}

fn describe(result: &DkimResult) -> (String, String) {
    match result {
        DkimResult::Pass => ("pass".into(), String::new()),
        DkimResult::Fail(e) => ("fail".into(), e.to_string()),
        DkimResult::Neutral(e) | DkimResult::PermError(e) | DkimResult::TempError(e) => {
            ("error".into(), e.to_string())
        }
        DkimResult::None => ("none".into(), String::new()),
    }
}

/// Verify a raw message. Needs DNS for DKIM keys and the DMARC policy.
pub async fn verify(resolver: &MessageAuthenticator, raw: &[u8], imap_host: &str) -> AuthReport {
    let parsed = parse::parse(raw);
    let (from, reply_to, provider) = match &parsed {
        Some(msg) => {
            let env = parse::envelope(msg);
            let headers = parse::raw_headers_all(msg, "Authentication-Results");
            (
                env.from,
                env.reply_to,
                provider_results(&headers, imap_host),
            )
        }
        None => Default::default(),
    };
    let from_domain = domain_of(&from.addr).to_string();
    let org = host_tail(&from_domain);

    let mut dkim = Vec::new();
    if let Some(message) = AuthenticatedMessage::parse(raw) {
        for output in resolver.verify_dkim(&message).await {
            let (result, detail) = describe(output.result());
            let (domain, selector) = output
                .signature()
                .map(|s| (s.d.to_ascii_lowercase(), s.s.clone()))
                .unwrap_or_default();
            if result == "none" && domain.is_empty() {
                continue;
            }
            let aligned = !domain.is_empty() && !org.is_empty() && host_tail(&domain) == org;
            dkim.push(DkimCheck {
                domain,
                selector,
                result,
                detail,
                aligned,
            });
        }
    }

    let policy = if from_domain.is_empty() {
        None
    } else {
        dmarc_policy(resolver, &from_domain).await
    };
    let (verdict, summary) = decide(&dkim, provider.as_ref(), policy.as_deref());
    let warnings = super::heuristics::warnings(&from.name, &from.addr, reply_to.as_deref());

    AuthReport {
        verdict,
        from_domain,
        dkim,
        dmarc_policy: policy,
        provider,
        warnings,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(domain: &str, result: &str, aligned: bool) -> DkimCheck {
        DkimCheck {
            domain: domain.into(),
            selector: "s".into(),
            result: result.into(),
            detail: String::new(),
            aligned,
        }
    }

    #[test]
    fn parses_gmail_style_header() {
        let r = parse_authentication_results(
            "mx.google.com; dkim=pass header.i=@example.com header.s=s1 header.b=abc; \
             dkim=fail (body hash; did not verify) header.i=@other.example; \
             spf=pass (google.com: domain of a@example.com designates 1.2.3.4 as permitted sender) smtp.mailfrom=a@example.com; \
             dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=example.com",
        )
        .unwrap();
        assert_eq!(r.authserv_id, "mx.google.com");
        assert_eq!(r.dkim.as_deref(), Some("pass"));
        assert_eq!(r.spf.as_deref(), Some("pass"));
        assert_eq!(r.dmarc.as_deref(), Some("pass"));
    }

    #[test]
    fn parses_failures_and_none() {
        let r = parse_authentication_results("mx.example.net; spf=softfail smtp.mailfrom=x@y.z; dkim=none; dmarc=fail header.from=bank.example").unwrap();
        assert_eq!(
            (r.spf.as_deref(), r.dkim.as_deref(), r.dmarc.as_deref()),
            (Some("softfail"), Some("none"), Some("fail"))
        );
        assert!(parse_authentication_results("").is_none());
    }

    #[test]
    fn only_the_providers_own_header_is_trusted() {
        assert!(authserv_is_trusted("mx.google.com", "imap.gmail.com"));
        assert!(authserv_is_trusted("mail.example.org", "imap.example.org"));
        assert!(!authserv_is_trusted("mx.google.com", "imap.example.org"));
        assert!(!authserv_is_trusted("attacker.example", "imap.gmail.com"));
        // A forged header below the real one is ignored; a forged header on top
        // of nothing is rejected by its authserv-id.
        let forged = vec!["attacker.example; dmarc=pass header.from=bank.example".to_string()];
        assert!(provider_results(&forged, "imap.gmail.com").is_none());
        let real_then_forged = vec![
            "mx.google.com; dmarc=fail header.from=bank.example".to_string(),
            "mx.google.com; dmarc=pass header.from=bank.example".to_string(),
        ];
        assert_eq!(
            provider_results(&real_then_forged, "imap.gmail.com")
                .unwrap()
                .dmarc
                .as_deref(),
            Some("fail")
        );
    }

    #[test]
    fn verdicts() {
        let pass = ProviderResults {
            dmarc: Some("pass".into()),
            ..Default::default()
        };
        let fail = ProviderResults {
            dmarc: Some("fail".into()),
            ..Default::default()
        };
        let silent = ProviderResults::default();

        assert_eq!(
            decide(&[check("example.com", "pass", true)], None, None).0,
            Verdict::Verified
        );
        assert_eq!(decide(&[], Some(&pass), None).0, Verdict::Verified);
        assert_eq!(decide(&[], Some(&fail), Some("reject")).0, Verdict::Failed);
        // Tampered body: the aligned signature no longer verifies.
        assert_eq!(
            decide(&[check("example.com", "fail", true)], None, None).0,
            Verdict::Failed
        );
        // Signed by a mailing service only: proves nothing about the From domain.
        assert_eq!(
            decide(
                &[check("sendgrid.net", "pass", false)],
                Some(&silent),
                Some("none")
            )
            .0,
            Verdict::Unverified
        );
        // Unsigned mail from an enforcing domain may have passed on SPF, which
        // cannot be checked here: not proof of forgery.
        assert_eq!(decide(&[], None, Some("reject")).0, Verdict::Unverified);
        assert_eq!(decide(&[], None, None).0, Verdict::Unverified);
        // A local DNS hiccup must not turn into "failed".
        assert_eq!(
            decide(
                &[check("example.com", "error", true)],
                Some(&pass),
                Some("reject")
            )
            .0,
            Verdict::Verified
        );
    }

    #[test]
    fn dmarc_record_parsing() {
        assert_eq!(
            parse_dmarc_policy("v=DMARC1; p=reject; rua=mailto:x@y.z").as_deref(),
            Some("reject")
        );
        assert_eq!(
            parse_dmarc_policy("v=DMARC1;p=None").as_deref(),
            Some("none")
        );
        assert_eq!(parse_dmarc_policy("v=spf1 -all"), None);
    }
}
