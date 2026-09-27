use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

use crate::db::key::{random_bytes, to_hex};

pub struct Provider {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static str,
    pub extra: &'static [(&'static str, &'static str)],
}

pub fn provider(name: &str) -> Result<Provider> {
    match name {
        "google" => Ok(Provider {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: "https://mail.google.com/",
            // Without these Google returns no refresh token on repeat sign-ins.
            extra: &[("access_type", "offline"), ("prompt", "consent")],
        }),
        "microsoft" => Ok(Provider {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scopes: "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access",
            extra: &[],
        }),
        other => bail!("unknown OAuth provider {other}"),
    }
}

#[derive(Debug, Clone)]
pub struct ClientCredentials {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

async fn token_request(
    http: &reqwest::Client,
    token_url: &str,
    pairs: &[(&str, &str)],
) -> Result<Tokens> {
    let response = http
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form(pairs))
        .send()
        .await
        .context("contacting the token endpoint")?;
    let status = response.status();
    let body = response.bytes().await?;
    if !status.is_success() {
        if let Ok(e) = serde_json::from_slice::<TokenError>(&body) {
            bail!(
                "sign-in was refused: {} {}",
                e.error,
                e.error_description.unwrap_or_default()
            );
        }
        bail!("token endpoint returned HTTP {status}");
    }
    serde_json::from_slice(&body).context("reading the token response")
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Signed in</title>\
<body style=\"font:16px system-ui;margin:15vh auto;max-width:28em;text-align:center\">\
<h2>You are signed in</h2><p>You can close this tab and return to the mail app.</p>";

/// Wait for the browser to come back to the loopback listener with `code` and `state`.
async fn receive_redirect(listener: TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request.split_whitespace().nth(1).unwrap_or("/");
        let url = Url::parse(&format!("http://127.0.0.1{path}"))?;
        let get = |k: &str| {
            url.query_pairs()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.into_owned())
        };

        // Browsers also ask for /favicon.ico and the like; ignore anything else.
        if get("code").is_none() && get("error").is_none() {
            socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .ok();
            continue;
        }
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{DONE_PAGE}",
            DONE_PAGE.len()
        );
        socket.write_all(reply.as_bytes()).await.ok();
        socket.shutdown().await.ok();

        if let Some(error) = get("error") {
            bail!("sign-in was cancelled or refused ({error})");
        }
        if get("state").as_deref() != Some(expected_state) {
            bail!("sign-in response did not match this request");
        }
        return get("code").ok_or_else(|| anyhow!("no authorization code"));
    }
}

/// Run the interactive flow. `open_browser` is handed the URL to show the user.
pub async fn authorize(
    http: &reqwest::Client,
    provider_name: &str,
    client: &ClientCredentials,
    login_hint: &str,
    open_browser: impl FnOnce(&str) -> Result<()>,
) -> Result<Tokens> {
    let p = provider(provider_name)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes::<48>());
    let state = to_hex(&random_bytes::<16>());

    let mut url = Url::parse(p.auth_url)?;
    url.query_pairs_mut()
        .append_pair("client_id", &client.client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", p.scopes)
        .append_pair("state", &state)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("login_hint", login_hint)
        .extend_pairs(p.extra);
    open_browser(url.as_str())?;

    let code = tokio::time::timeout(Duration::from_secs(300), receive_redirect(listener, &state))
        .await
        .map_err(|_| anyhow!("timed out waiting for the browser sign-in"))??;

    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", client.client_id.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    if let Some(secret) = &client.client_secret {
        pairs.push(("client_secret", secret));
    }
    let tokens = token_request(http, p.token_url, &pairs).await?;
    if tokens.refresh_token.is_none() {
        bail!("the provider did not return a refresh token");
    }
    Ok(tokens)
}

pub async fn refresh(
    http: &reqwest::Client,
    provider_name: &str,
    client: &ClientCredentials,
    refresh_token: &str,
) -> Result<Tokens> {
    let p = provider(provider_name)?;
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client.client_id.as_str()),
    ];
    if let Some(secret) = &client.client_secret {
        pairs.push(("client_secret", secret));
    }
    token_request(http, p.token_url, &pairs).await
}

/// SASL XOAUTH2 initial response, before base64.
pub fn xoauth2(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn xoauth2_format() {
        assert_eq!(
            xoauth2("a@b.c", "tok"),
            "user=a@b.c\x01auth=Bearer tok\x01\x01"
        );
    }

    #[tokio::test]
    async fn redirect_listener_checks_state() {
        async fn hit(port: u16, path: &str) {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut sink = Vec::new();
            s.read_to_end(&mut sink).await.ok();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move { receive_redirect(listener, "good").await });
        hit(port, "/favicon.ico").await;
        hit(port, "/?state=good&code=abc123").await;
        assert_eq!(task.await.unwrap().unwrap(), "abc123");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move { receive_redirect(listener, "good").await });
        hit(port, "/?state=evil&code=abc123").await;
        assert!(task.await.unwrap().is_err());
    }
}
