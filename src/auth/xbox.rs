//! Xbox Live and XSTS token exchange.
//!
//! Step 2 & 3 of the auth chain:
//! - Exchange MSA token for Xbox Live User Token
//! - Exchange XBL User Token for XSTS Authorization Token

use serde::{Deserialize, Serialize};
use tracing::debug;

use super::AuthError;

type Result<T> = std::result::Result<T, AuthError>;

const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

/// An Xbox token (User Token or XSTS Token) with the associated user hash.
#[derive(Debug, Clone)]
pub struct XboxToken {
    pub token: String,
    pub user_hash: String,
}

#[derive(Serialize)]
struct XblRequest {
    #[serde(rename = "Properties")]
    properties: XblProperties,
    #[serde(rename = "RelyingParty")]
    relying_party: String,
    #[serde(rename = "TokenType")]
    token_type: String,
}

#[derive(Serialize)]
struct XblProperties {
    #[serde(rename = "AuthMethod")]
    auth_method: String,
    #[serde(rename = "SiteName")]
    site_name: String,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Serialize)]
struct XstsRequest {
    #[serde(rename = "Properties")]
    properties: XstsProperties,
    #[serde(rename = "RelyingParty")]
    relying_party: String,
    #[serde(rename = "TokenType")]
    token_type: String,
}

#[derive(Serialize)]
struct XstsProperties {
    #[serde(rename = "SandboxId")]
    sandbox_id: String,
    #[serde(rename = "UserTokens")]
    user_tokens: Vec<String>,
}

#[derive(Deserialize)]
struct XTokenResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Deserialize)]
struct XuiClaim {
    uhs: String,
}

#[derive(Deserialize)]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: Option<u64>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

// Xbox error codes are fixed protocol values, not arbitrary numbers.
const XERR_NO_XBOX_PROFILE: u64 = 2_148_916_233;
const XERR_REGION_BLOCKED: u64 = 2_148_916_235;
const XERR_MINOR_FAMILY: u64 = 2_148_916_238;

/// Exchange an MSA access token for an Xbox Live User Token.
pub async fn get_user_token(msa_token: &str, http: &reqwest::Client) -> Result<XboxToken> {
    debug!("Exchanging MSA token for Xbox Live User Token...");

    let body = XblRequest {
        properties: XblProperties {
            auth_method: "RPS".to_string(),
            site_name: "user.auth.xboxlive.com".to_string(),
            rps_ticket: format!("d={msa_token}"),
        },
        relying_party: "http://auth.xboxlive.com".to_string(),
        token_type: "JWT".to_string(),
    };

    let resp = http
        .post(XBL_AUTH_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    parse_xbox_response(resp, "Xbox Live User Token").await
}

/// Exchange an Xbox Live User Token for an XSTS Authorization Token.
pub async fn get_xsts_token(user_token: &XboxToken, http: &reqwest::Client) -> Result<XboxToken> {
    debug!("Exchanging Xbox Live User Token for XSTS Token...");

    let body = XstsRequest {
        properties: XstsProperties {
            sandbox_id: "RETAIL".to_string(),
            user_tokens: vec![user_token.token.clone()],
        },
        relying_party: "rp://api.minecraftservices.com/".to_string(),
        token_type: "JWT".to_string(),
    };

    let resp = http
        .post(XSTS_AUTH_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    parse_xbox_response(resp, "XSTS").await
}

async fn parse_xbox_response(resp: reqwest::Response, label: &str) -> Result<XboxToken> {
    let status = resp.status();
    let body = resp.text().await?;

    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<XstsError>(&body) {
            let msg = match err.xerr {
                Some(XERR_NO_XBOX_PROFILE) => "This Microsoft account does not have an Xbox Live profile. \
                                     You may need to sign up at xbox.com."
                    .to_string(),
                Some(XERR_REGION_BLOCKED) => {
                    "Xbox Live is not available in your country/region.".to_string()
                }
                Some(XERR_MINOR_FAMILY) => "This account belongs to a minor and must be added to a \
                     Microsoft Family to use Xbox Live."
                    .to_string(),
                Some(code) => format!(
                    "Xbox error {code}: {}",
                    err.message.as_deref().unwrap_or("unknown")
                ),
                None => format!(
                    "Xbox error: {}",
                    err.message.as_deref().unwrap_or("unknown")
                ),
            };
            return Err(AuthError::Xbox {
                label: label.to_string(),
                message: msg,
            });
        }
        return Err(AuthError::XboxRequest {
            label: label.to_string(),
            status: status.to_string(),
            body,
        });
    }

    let resp: XTokenResponse = serde_json::from_str(&body)?;
    let user_hash = resp
        .display_claims
        .xui
        .first()
        .ok_or_else(|| AuthError::XboxMissingHash(label.to_string()))?
        .uhs
        .clone();

    Ok(XboxToken {
        token: resp.token,
        user_hash,
    })
}
