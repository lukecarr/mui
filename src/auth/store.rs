//! Token persistence: save/load auth state to disk as JSON.
//!
//! Stores the full token chain (MSA, Xbox, Minecraft) so the user
//! doesn't have to re-authenticate every time.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::AuthError;
use super::msa::{self, MsaTokens};
use super::xbox;

type Result<T> = std::result::Result<T, AuthError>;

/// The user's Minecraft profile (username + UUID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftProfile {
    pub uuid: String,
    pub username: String,
}

/// All persistent auth data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthData {
    /// MSA refresh token (long-lived).
    pub msa_refresh_token: String,
    /// MSA access token.
    pub msa_access_token: String,
    /// When the MSA token expires.
    pub msa_expires_at: DateTime<Utc>,
    /// Minecraft access token (used for launching).
    pub mc_access_token: String,
    /// When the MC token expires.
    pub mc_expires_at: DateTime<Utc>,
    /// Player profile.
    pub profile: MinecraftProfile,
}

impl AuthData {
    /// Whether the Minecraft access token is still valid (with 5-minute buffer).
    pub fn mc_token_valid(&self) -> bool {
        Utc::now() + Duration::minutes(5) < self.mc_expires_at
    }

    /// Whether the MSA access token is still valid (with 5-minute buffer).
    pub fn msa_token_valid(&self) -> bool {
        Utc::now() + Duration::minutes(5) < self.msa_expires_at
    }
}

/// Manages loading, saving, and refreshing auth data.
#[derive(Debug)]
pub struct AuthStore {
    pub data: Option<AuthData>,
    path: std::path::PathBuf,
}

impl AuthStore {
    /// Load auth data from disk.
    ///
    /// Returns `Ok` with `data: None` if the file doesn't exist or can't be parsed.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Io`] if the file exists but can't be read.
    pub fn load(path: &Path) -> Result<Self> {
        let path = path.to_path_buf();
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            match serde_json::from_str::<AuthData>(&contents) {
                Ok(data) => {
                    debug!("Loaded auth data for {}", data.profile.username);
                    Ok(Self {
                        data: Some(data),
                        path,
                    })
                }
                Err(e) => {
                    tracing::warn!("Failed to parse auth store, starting fresh: {e}");
                    Ok(Self { data: None, path })
                }
            }
        } else {
            Ok(Self { data: None, path })
        }
    }

    /// Save current auth data to disk.
    ///
    /// On Unix, sets file permissions to `0o600` (owner-only) since the file
    /// contains sensitive tokens.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Json`] if serialization fails, or [`AuthError::Io`]
    /// if writing to disk fails.
    pub fn save(&self) -> Result<()> {
        if let Some(ref data) = self.data {
            let json = serde_json::to_string_pretty(data)?;
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&self.path, json)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &self.path,
                    std::fs::Permissions::from_mode(0o600),
                )?;
            }
            debug!("Saved auth data to {:?}", self.path);
        }
        Ok(())
    }

    /// Perform a fresh login (opens browser).
    ///
    /// Runs the full 6-step auth chain: MSA -> Xbox Live -> XSTS -> MC login
    /// -> entitlements check -> profile fetch. Saves the result to disk.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] if any step of the auth chain fails.
    pub async fn login(
        &mut self,
        client_id: &str,
        http: &reqwest::Client,
    ) -> Result<()> {
        // Step 1: Microsoft OAuth2
        let msa = msa::login(client_id, http).await?;
        let msa_expires_at = Utc::now() + Duration::seconds(msa.expires_in as i64);

        // Steps 2-3: Xbox Live
        let xbl = xbox::get_user_token(&msa.access_token, http).await?;
        let xsts = xbox::get_xsts_token(&xbl, http).await?;

        // Step 4: Minecraft token
        let mc = super::minecraft::login_with_xbox(&xsts, http).await?;
        let mc_expires_at = Utc::now() + Duration::seconds(mc.expires_in as i64);

        // Step 5: Check entitlements
        let owns_game = super::minecraft::check_entitlements(&mc.access_token, http).await?;
        if !owns_game {
            return Err(AuthError::NotOwned);
        }

        // Step 6: Get profile
        let profile = super::minecraft::get_profile(&mc.access_token, http).await?;

        self.data = Some(AuthData {
            msa_refresh_token: msa.refresh_token,
            msa_access_token: msa.access_token,
            msa_expires_at,
            mc_access_token: mc.access_token,
            mc_expires_at,
            profile,
        });

        self.save()?;
        info!("Login complete!");
        Ok(())
    }

    /// Refresh tokens if needed.
    ///
    /// Returns `Ok(true)` if tokens are valid (either already valid or
    /// successfully refreshed), `Ok(false)` if not logged in.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError`] if the refresh flow fails.
    pub async fn ensure_valid(
        &mut self,
        client_id: &str,
        http: &reqwest::Client,
    ) -> Result<bool> {
        let data = match &self.data {
            Some(d) => d,
            None => return Ok(false), // Not logged in
        };

        // If MC token is still good, we're fine
        if data.mc_token_valid() {
            return Ok(true);
        }

        info!("Minecraft token expired, refreshing...");

        // Refresh MSA token
        let msa: MsaTokens = if data.msa_token_valid() {
            // MSA token still good, but MC expired — re-do Xbox+MC chain
            MsaTokens {
                access_token: data.msa_access_token.clone(),
                refresh_token: data.msa_refresh_token.clone(),
                expires_in: 0, // doesn't matter, we won't recalculate expiry
            }
        } else {
            // Refresh MSA token first
            msa::refresh(client_id, &data.msa_refresh_token, http).await?
        };

        // Redo the Xbox + MC chain
        let xbl = xbox::get_user_token(&msa.access_token, http).await?;
        let xsts = xbox::get_xsts_token(&xbl, http).await?;
        let mc = super::minecraft::login_with_xbox(&xsts, http).await?;
        let mc_expires_at = Utc::now() + Duration::seconds(mc.expires_in as i64);

        // Update stored data
        let profile = if let Some(ref d) = self.data {
            d.profile.clone()
        } else {
            super::minecraft::get_profile(&mc.access_token, http).await?
        };

        let msa_expires_at = if data.msa_token_valid() {
            data.msa_expires_at
        } else {
            Utc::now() + Duration::seconds(msa.expires_in as i64)
        };

        self.data = Some(AuthData {
            msa_refresh_token: if msa.refresh_token.is_empty() {
                data.msa_refresh_token.clone()
            } else {
                msa.refresh_token
            },
            msa_access_token: msa.access_token,
            msa_expires_at,
            mc_access_token: mc.access_token,
            mc_expires_at,
            profile,
        });

        self.save()?;
        Ok(true)
    }
}
