pub mod discovery;
pub mod oauth;
pub mod secrets;

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

use crate::{
    db::{self, store::Account},
    state::AppState,
};
use oauth::ClientCredentials;

/// What a connection authenticates with.
pub enum Login {
    Password(String),
    Bearer(String),
}

pub fn oauth_client(state: &AppState, provider: &str) -> Result<ClientCredentials> {
    state.with_db(|conn| {
        let client_id = db::get_setting(conn, &format!("oauth.{provider}.client_id"))?
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("set up an OAuth client ID for {provider} first"))?;
        let client_secret = db::get_setting(conn, &format!("oauth.{provider}.client_secret"))?
            .filter(|s| !s.is_empty());
        Ok(ClientCredentials {
            client_id,
            client_secret,
        })
    })
}

/// Credentials for `account`, refreshing the OAuth access token when needed.
pub async fn login_for(state: &AppState, account: &Account) -> Result<Login> {
    let mode = state
        .mode()
        .ok_or_else(|| anyhow!("the mail store is locked"))?;
    let secret = state.with_db(|conn| secrets::get(conn, mode, account.id, &account.email))?;
    if account.auth_kind != "oauth" {
        return Ok(Login::Password(secret));
    }

    let mut tokens = state.access_tokens.lock().await;
    if let Some((token, expiry)) = tokens.get(&account.id) {
        if *expiry > Instant::now() + Duration::from_secs(60) {
            return Ok(Login::Bearer(token.clone()));
        }
    }
    let provider = account
        .oauth_provider
        .as_deref()
        .ok_or_else(|| anyhow!("account has no OAuth provider"))?;
    let client = oauth_client(state, provider)?;
    let fresh = oauth::refresh(&state.http, provider, &client, &secret).await?;
    if let Some(rotated) = &fresh.refresh_token {
        state.with_db(|conn| secrets::set(conn, mode, account.id, &account.email, rotated))?;
    }
    let expiry = Instant::now() + Duration::from_secs(fresh.expires_in.unwrap_or(3600));
    tokens.insert(account.id, (fresh.access_token.clone(), expiry));
    Ok(Login::Bearer(fresh.access_token))
}
