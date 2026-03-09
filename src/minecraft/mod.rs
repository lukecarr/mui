pub mod download;
pub mod launch;
pub mod manifest;
pub mod rules;
pub mod version;

/// Errors that can occur during Minecraft file operations (download, launch, etc.).
#[derive(Debug, thiserror::Error)]
pub enum MinecraftError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parsing failed.
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    /// IO error during file operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A file download returned a non-success HTTP status.
    #[error("Download failed for {label} ({status}): {url}")]
    DownloadFailed {
        /// Human-readable label for the file being downloaded.
        label: String,
        /// HTTP status code.
        status: String,
        /// The URL that was requested.
        url: String,
    },

    /// SHA-1 hash of the downloaded file does not match the expected value.
    #[error("SHA-1 verification failed for {0}")]
    Sha1Mismatch(String),

    /// Error extracting files from a ZIP archive (native libraries).
    #[error("ZIP extraction error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// A spawned tokio task failed to join.
    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}
