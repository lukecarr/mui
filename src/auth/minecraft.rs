//! Minecraft authentication: token exchange, entitlements, and profile.
//!
//! Step 4-6 of the auth chain:
//! - Exchange XSTS token for Minecraft access token
//! - Check game entitlements (ownership)
//! - Fetch Minecraft profile (username, UUID, skin)

use serde::Deserialize;
use tracing::{debug, info};

use super::{AuthError, store::MinecraftProfile, xbox::XboxToken};

type Result<T> = std::result::Result<T, AuthError>;

const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Tokens received from the Minecraft login exchange.
#[derive(Debug, Clone)]
pub struct McTokens {
    pub access_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct McLoginResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct EntitlementsResponse {
    items: Vec<EntitlementItem>,
}

#[derive(Deserialize)]
struct EntitlementItem {
    name: String,
}

#[derive(Deserialize)]
struct ProfileResponse {
    id: String,
    name: String,
}

/// Exchange an XSTS token for a Minecraft access token.
pub async fn login_with_xbox(xsts: &XboxToken, http: &reqwest::Client) -> Result<McTokens> {
    debug!("Exchanging XSTS token for Minecraft access token...");

    let identity_token = format!("XBL3.0 x={};{}", xsts.user_hash, xsts.token);

    let body = serde_json::json!({
        "identityToken": identity_token,
    });

    let resp = http
        .post(MC_LOGIN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        return Err(AuthError::MinecraftLogin {
            status: status.to_string(),
            body: text,
        });
    }

    let login: McLoginResponse = serde_json::from_str(&text)?;
    info!("Obtained Minecraft access token");

    Ok(McTokens {
        access_token: login.access_token,
        expires_in: login.expires_in,
    })
}

/// Check that the account owns Minecraft.
pub async fn check_entitlements(mc_token: &str, http: &reqwest::Client) -> Result<bool> {
    debug!("Checking Minecraft entitlements...");

    let resp = http
        .get(MC_ENTITLEMENTS_URL)
        .bearer_auth(mc_token)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        return Err(AuthError::EntitlementsFailed {
            status: status.to_string(),
            body: text,
        });
    }

    let ent: EntitlementsResponse = serde_json::from_str(&text)?;
    let owns_game = ent
        .items
        .iter()
        .any(|item| item.name == "product_minecraft" || item.name == "game_minecraft");

    if !owns_game {
        tracing::warn!("Account does not appear to own Minecraft");
    }

    Ok(owns_game)
}

/// Fetch the Minecraft profile (username, UUID) for the authenticated user.
pub async fn get_profile(mc_token: &str, http: &reqwest::Client) -> Result<MinecraftProfile> {
    debug!("Fetching Minecraft profile...");

    let resp = http
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_token)
        .send()
        .await?;

    let status = resp.status();
    let text = resp.text().await?;

    if status.as_u16() == 404 {
        return Err(AuthError::NoProfile);
    }

    if !status.is_success() {
        return Err(AuthError::ProfileFailed {
            status: status.to_string(),
            body: text,
        });
    }

    let profile: ProfileResponse = serde_json::from_str(&text)?;
    info!("Logged in as {} ({})", profile.name, profile.id);

    Ok(MinecraftProfile {
        uuid: profile.id,
        username: profile.name,
    })
}
