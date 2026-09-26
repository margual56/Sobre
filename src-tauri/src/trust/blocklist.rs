/// Normalise what the user typed or clicked into a stored pattern.
pub fn normalize(pattern: &str) -> Option<String> {
    let p = pattern.trim().trim_start_matches('@').to_ascii_lowercase();
    let domain = p.rsplit_once('@').map(|(_, d)| d).unwrap_or(&p);
    (!p.is_empty() && domain.contains('.') && !p.contains(char::is_whitespace)).then_some(p)
}

/// `patterns` hold full addresses or bare domains; a domain also covers its subdomains.
pub fn is_blocked<S: AsRef<str>>(patterns: &[S], addr: &str) -> bool {
    let addr = addr.trim().to_ascii_lowercase();
    let Some((_, domain)) = addr.rsplit_once('@') else {
        return false;
    };
    patterns.iter().any(|p| {
        let p = p.as_ref();
        if p.contains('@') {
            p == addr
        } else {
            domain == p || domain.ends_with(&format!(".{p}"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_addresses_and_domains() {
        let list = ["spam@example.com", "bad.example"];
        assert!(is_blocked(&list, "Spam@Example.com"));
        assert!(!is_blocked(&list, "ham@example.com"));
        assert!(is_blocked(&list, "x@bad.example"));
        assert!(is_blocked(&list, "x@mail.bad.example"));
        assert!(!is_blocked(&list, "x@notbad.example"));
        assert!(!is_blocked(&list, "not-an-address"));
    }

    #[test]
    fn normalizes_input() {
        assert_eq!(normalize(" @Example.COM ").as_deref(), Some("example.com"));
        assert_eq!(normalize("A@b.co").as_deref(), Some("a@b.co"));
        assert_eq!(normalize("localhost"), None);
        assert_eq!(normalize(""), None);
    }
}
