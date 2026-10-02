use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use tauri::http::{Response, StatusCode};
use url::Url;

use super::{sanitize::SanitizeOptions, BodyParts, View, BODY_CSP};
use crate::{
    db::store,
    mail::{parse, sync},
    net,
    state::AppState,
};

const MAX_IMAGE: usize = 10 * 1024 * 1024;

pub fn asset_prefix(state: &AppState, message_id: i64) -> String {
    format!("/{}/{message_id}", state.token)
}

fn reply(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Content-Security-Policy", BODY_CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .body(body)
        .expect("static response parts are valid")
}

fn param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// Render a stored message for the reader.
pub fn document(
    state: &AppState,
    message_id: i64,
    raw: &[u8],
    view: View,
    images: bool,
    dark: bool,
) -> super::Rendered {
    let prefix = asset_prefix(state, message_id);
    let opts = SanitizeOptions {
        asset_prefix: &prefix,
        allow_remote_images: images,
    };
    let msg = parse::parse(raw);
    let html = msg.as_ref().and_then(parse::html_body);
    let text = msg.as_ref().and_then(parse::text_body);
    super::render(
        &BodyParts {
            html: html.as_deref(),
            text: text.as_deref(),
            raw,
        },
        view,
        &opts,
        dark,
    )
}

async fn remote_image(state: &AppState, url: &str) -> Result<(String, Vec<u8>)> {
    if let Some((Some(mime), Some(data), _)) =
        state.with_db(|c| store::cached_blob(c, "image_cache", "url", url))?
    {
        return Ok((mime, data));
    }
    let fetched = net::fetch_public(url, MAX_IMAGE).await?;
    // Only real images are passed on, whatever the server claims to send.
    let mime = net::sniff_image(&fetched.data).ok_or_else(|| anyhow!("not an image"))?;
    state.with_db(|c| {
        store::cache_blob(
            c,
            "image_cache",
            "url",
            url,
            Some(mime),
            Some(&fetched.data),
        )
    })?;
    Ok((mime.to_string(), fetched.data))
}

async fn route(state: &Arc<AppState>, uri: &str) -> Result<Response<Vec<u8>>> {
    let url = Url::parse(uri)?;
    let mut segments = url.path_segments().ok_or_else(|| anyhow!("bad path"))?;
    let (token, id, what) = (segments.next(), segments.next(), segments.next());
    if token != Some(state.token.as_str()) {
        bail!("forbidden");
    }
    let id: i64 = id.ok_or_else(|| anyhow!("bad path"))?.parse()?;

    match what {
        Some("doc") => {
            let raw = sync::fetch_body_now(state, id).await?;
            let images = param(&url, "images").as_deref() == Some("1");
            if images {
                state.images_allowed.lock().unwrap().insert(id);
            } else {
                state.images_allowed.lock().unwrap().remove(&id);
            }
            let view = match param(&url, "view").as_deref() {
                Some("plain") => View::Plain,
                Some("source") => View::Source,
                _ => View::Auto,
            };
            let dark = param(&url, "theme").as_deref() == Some("dark");
            let rendered = document(state, id, &raw, view, images, dark);
            Ok(reply(
                StatusCode::OK,
                "text/html; charset=utf-8",
                rendered.document.into_bytes(),
            ))
        }
        Some("cid") => {
            let cid = param(&url, "id").ok_or_else(|| anyhow!("missing id"))?;
            let raw = state
                .with_db(|c| store::body(c, id))?
                .ok_or_else(|| anyhow!("no body"))?;
            let msg = parse::parse(&raw).ok_or_else(|| anyhow!("unparseable"))?;
            let part = parse::part_by_cid(&msg, &cid).ok_or_else(|| anyhow!("no such part"))?;
            let data = part.contents().to_vec();
            let mime = net::sniff_image(&data).ok_or_else(|| anyhow!("not an image"))?;
            Ok(reply(StatusCode::OK, mime, data))
        }
        Some("img") => {
            if !state.images_allowed.lock().unwrap().contains(&id) {
                bail!("remote images are not enabled for this message");
            }
            let target = param(&url, "u").ok_or_else(|| anyhow!("missing url"))?;
            let (mime, data) = remote_image(state, &target).await?;
            Ok(reply(StatusCode::OK, &mime, data))
        }
        _ => bail!("not found"),
    }
}

pub async fn handle(state: Arc<AppState>, uri: String) -> Response<Vec<u8>> {
    match route(&state, &uri).await {
        Ok(response) => response,
        Err(e) => {
            let page = format!(
                "<!doctype html><meta charset=utf-8><body style=\"font:14px system-ui;color:#888;margin:24px\">Could not show this message: {}</body>",
                super::sanitize::escape(&format!("{e:#}"))
            );
            reply(
                StatusCode::NOT_FOUND,
                "text/html; charset=utf-8",
                page.into_bytes(),
            )
        }
    }
}
