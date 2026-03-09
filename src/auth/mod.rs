pub mod minecraft;
pub mod msa;
pub mod store;
pub mod xbox;

pub use store::AuthStore;

/// Errors that can occur during the authentication flow.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// HTTP request failed during auth exchange.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parsing failed on an auth response.
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    /// IO error during auth (e.g., TCP listener, file operations).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Microsoft returned a token error response.
    #[error("MSA token error: {error} - {description}")]
    MsaToken {
        /// OAuth2 error code from Microsoft.
        error: String,
        /// Human-readable error description.
        description: String,
    },

    /// Microsoft token endpoint returned a non-success status.
    #[error("MSA token request failed ({status}): {body}")]
    MsaRequest {
        /// HTTP status code.
        status: String,
        /// Response body.
        body: String,
    },

    /// No authorization code found in the browser redirect URL.
    #[error("No authorization code found in redirect URL: {0}")]
    NoAuthCode(String),

    /// Failed to open the browser for OAuth login.
    ///
    /// Kept separate from [`Io`](AuthError::Io) so callers can distinguish
    /// browser failures from general IO errors.
    #[error("Failed to open browser: {0}")]
    Browser(#[source] std::io::Error),

    /// A spawned blocking task failed to join (panic or cancellation).
    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    /// Xbox Live / XSTS authentication failed with a known error.
    #[error("Xbox auth failed for {label}: {message}")]
    Xbox {
        /// Which step failed (e.g., "Xbox Live User Token", "XSTS").
        label: String,
        /// Human-readable error message.
        message: String,
    },

    /// Xbox endpoint returned a non-success status without a parseable error.
    #[error("Xbox request failed for {label} ({status}): {body}")]
    XboxRequest {
        /// Which step failed.
        label: String,
        /// HTTP status code.
        status: String,
        /// Response body.
        body: String,
    },

    /// Xbox response was missing the user hash in DisplayClaims.
    #[error("Xbox response missing user hash for {0}")]
    XboxMissingHash(String),

    /// Minecraft login endpoint returned a non-success status.
    #[error("Minecraft login failed ({status}): {body}")]
    MinecraftLogin {
        /// HTTP status code.
        status: String,
        /// Response body.
        body: String,
    },

    /// Entitlements check failed at the HTTP level.
    #[error("Entitlements check failed ({status}): {body}")]
    EntitlementsFailed {
        /// HTTP status code.
        status: String,
        /// Response body.
        body: String,
    },

    /// Account does not own Minecraft.
    #[error("This account does not own Minecraft")]
    NotOwned,

    /// Account has no Minecraft profile (game may not be purchased).
    #[error("This Microsoft account does not have a Minecraft profile. You may need to purchase the game.")]
    NoProfile,

    /// Minecraft profile endpoint returned a non-success status.
    #[error("Minecraft profile request failed ({status}): {body}")]
    ProfileFailed {
        /// HTTP status code.
        status: String,
        /// Response body.
        body: String,
    },
}
