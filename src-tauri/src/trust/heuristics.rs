use crate::{mail::parse::domain_of, render::sanitize::host_tail};

fn script_of(c: char) -> u8 {
    match c as u32 {
        0x0041..=0x024F => 1, // Latin
        0x0370..=0x03FF => 2, // Greek
        0x0400..=0x052F => 3, // Cyrillic
        _ => 0,
    }
}

/// A label mixing Latin with Greek or Cyrillic is the classic homograph trick.
pub fn is_mixed_script(domain: &str) -> bool {
    domain.split('.').any(|label| {
        let mut seen = [false; 4];
        for c in label.chars().filter(|c| c.is_alphabetic()) {
            seen[script_of(c) as usize] = true;
        }
        seen[1] && (seen[2] || seen[3])
    })
}

/// An address-looking token inside a display name.
fn address_in(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | ','))
        .find(|t| {
            t.split_once('@')
                .map(|(l, d)| !l.is_empty() && d.contains('.') && !d.ends_with('.'))
                .unwrap_or(false)
        })
        .map(|t| t.to_ascii_lowercase())
}

pub fn warnings(from_name: &str, from_addr: &str, reply_to: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let domain = domain_of(from_addr);

    if let Some(shown) = address_in(from_name) {
        if host_tail(domain_of(&shown)) != host_tail(domain) {
            out.push(format!(
                "The sender name shows {shown}, but the mail comes from {from_addr}."
            ));
        }
    }
    if let Some(reply) = reply_to {
        let reply_domain = domain_of(reply);
        if !reply_domain.is_empty() && host_tail(reply_domain) != host_tail(domain) {
            out.push(format!(
                "Replies go to a different domain ({reply_domain})."
            ));
        }
    }
    if domain.split('.').any(|l| l.starts_with("xn--")) {
        out.push(
            "The sender domain uses non-ASCII characters (punycode); check it carefully.".into(),
        );
    } else if is_mixed_script(domain) {
        out.push(
            "The sender domain mixes alphabets, a common way to imitate another domain.".into(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_spoofed_display_name() {
        let w = warnings("support@paypal.com", "x@evil.example", None);
        assert_eq!(w.len(), 1);
        assert!(warnings("Ana (ana@example.com)", "ana@mail.example.com", None).is_empty());
    }

    #[test]
    fn flags_reply_to_and_lookalikes() {
        assert_eq!(
            warnings("Bank", "info@bank.example", Some("x@gmail.com")).len(),
            1
        );
        assert!(warnings(
            "Bank",
            "info@bank.example",
            Some("help@support.bank.example")
        )
        .is_empty());
        assert_eq!(warnings("", "a@xn--pypal-4ve.com", None).len(), 1);
        assert!(is_mixed_script("p\u{0430}ypal.com"));
        assert!(!is_mixed_script("paypal.com"));
        assert!(!is_mixed_script(
            "\u{043F}\u{043E}\u{0447}\u{0442}\u{0430}.\u{0440}\u{0444}"
        ));
    }
}
