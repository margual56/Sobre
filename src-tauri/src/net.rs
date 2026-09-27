use std::{net::IpAddr, time::Duration};

use anyhow::{anyhow, bail, Result};
use url::Url;

pub struct Fetched {
    pub mime: String,
    pub data: Vec<u8>,
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // carrier-grade NAT
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80) // link local
        }
    }
}

const MAX_REDIRECTS: usize = 5;

async fn client_for(url: &Url) -> Result<reqwest::Client> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("unsupported scheme");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("no host"))?
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = tokio::net::lookup_host((host.as_str(), port))
        .await?
        .collect();
    let Some(addr) = addrs.first().copied() else {
        bail!("host does not resolve")
    };
    if addrs.iter().any(|a| !is_public(a.ip())) {
        bail!("refusing to contact a non-public address");
    }
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addr)
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (mail client)")
        .build()?)
}

/// GET `url`, following redirects by hand so every hop is checked.
pub async fn fetch_public(url: &str, max_bytes: usize) -> Result<Fetched> {
    let mut current = Url::parse(url)?;
    for _ in 0..=MAX_REDIRECTS {
        let client = client_for(&current).await?;
        let mut response = client
            .get(current.clone())
            .header("Accept", "image/*,*/*;q=0.5")
            .send()
            .await?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .ok_or_else(|| anyhow!("redirect without a location"))?;
            current = current.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            bail!("HTTP {}", response.status());
        }
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if data.len() + chunk.len() > max_bytes {
                bail!("response too large");
            }
            data.extend_from_slice(&chunk);
        }
        return Ok(Fetched { mime, data });
    }
    bail!("too many redirects")
}

/// POST a form body to a public HTTPS address. Redirects are not followed.
pub async fn post_form_public(url: &str, body: &str) -> Result<()> {
    let url = Url::parse(url)?;
    if url.scheme() != "https" {
        bail!("only https is accepted here");
    }
    let response = client_for(&url)
        .await?
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .send()
        .await?;
    if response.status().is_client_error() || response.status().is_server_error() {
        bail!("the server answered HTTP {}", response.status());
    }
    Ok(())
}

/// Identify an image by its magic bytes; the declared type is not trusted.
pub fn sniff_image(data: &[u8]) -> Option<&'static str> {
    let head = &data[..data.len().min(512)];
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.len() > 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if head.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        (text.contains("<svg")).then_some("image/svg+xml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ranges_are_rejected() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.16.0.9",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:192.168.0.1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "142.250.184.5", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn sniffs_images() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(
            sniff_image(b"<?xml version=\"1.0\"?><svg xmlns="),
            Some("image/svg+xml")
        );
        assert_eq!(sniff_image(b"<html><script>"), None);
    }
}
