use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};

use crate::{db::store, net, render::sanitize::host_tail, state::AppState};

const MAX_ICON: usize = 256 * 1024;
const RETRY_MISS_AFTER: i64 = 7 * 24 * 3600;
const REFRESH_AFTER: i64 = 30 * 24 * 3600;

fn data_url(mime: &str, data: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(data))
}

/// `l=` tag of a BIMI record.
pub fn bimi_logo_url(record: &str) -> Option<String> {
    if !record
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("v=bimi1")
    {
        return None;
    }
    record.split(';').find_map(|tag| {
        let (k, v) = tag.split_once('=')?;
        let v = v.trim();
        (k.trim().eq_ignore_ascii_case("l") && v.starts_with("https://")).then(|| v.to_string())
    })
}

async fn fetch_image(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let fetched = net::fetch_public(url, MAX_ICON).await.ok()?;
    let mime = net::sniff_image(&fetched.data)?;
    Some((mime, fetched.data))
}

async fn lookup(state: &AppState, domain: &str, verified: bool) -> Option<(&'static str, Vec<u8>)> {
    let org = host_tail(domain);
    if verified {
        if let Some(resolver) = state.resolver() {
            for name in [
                format!("default._bimi.{domain}."),
                format!("default._bimi.{org}."),
            ] {
                if let Ok(raw) = resolver.txt_raw_lookup(name).await {
                    if let Some(url) = bimi_logo_url(&String::from_utf8_lossy(&raw)) {
                        if let Some(found) = fetch_image(&url).await {
                            return Some(found);
                        }
                    }
                }
            }
        }
    }
    for url in [
        format!("https://{org}/favicon.ico"),
        format!("https://www.{org}/favicon.ico"),
    ] {
        if let Some(found) = fetch_image(&url).await {
            return Some(found);
        }
    }
    None
}

/// A data URL for the domain's icon, or `None` when it has none.
pub async fn icon_for(state: &AppState, domain: &str, verified: bool) -> Result<Option<String>> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || !domain.contains('.') {
        return Ok(None);
    }
    let now = crate::db::now();
    if let Some((mime, data, at)) =
        state.with_db(|c| store::cached_blob(c, "icon_cache", "domain", &domain))?
    {
        match (mime, data) {
            (Some(mime), Some(data)) if now - at < REFRESH_AFTER => {
                return Ok(Some(data_url(&mime, &data)))
            }
            (None, _) if now - at < RETRY_MISS_AFTER => return Ok(None),
            _ => {}
        }
    }
    let found = lookup(state, &domain, verified).await;
    state.with_db(|c| match &found {
        Some((mime, data)) => {
            store::cache_blob(c, "icon_cache", "domain", &domain, Some(mime), Some(data))
        }
        None => store::cache_blob(c, "icon_cache", "domain", &domain, None, None),
    })?;
    Ok(found.map(|(mime, data)| data_url(mime, &data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bimi_records() {
        assert_eq!(
            bimi_logo_url("v=BIMI1; l=https://example.com/logo.svg; a=https://example.com/vmc.pem")
                .as_deref(),
            Some("https://example.com/logo.svg")
        );
        assert_eq!(
            bimi_logo_url("v=BIMI1; l=http://example.com/logo.svg"),
            None
        );
        assert_eq!(bimi_logo_url("v=BIMI1; l=;"), None);
        assert_eq!(bimi_logo_url("v=spf1 -all"), None);
    }
}
