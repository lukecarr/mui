pub mod config;
pub mod manager;

/// Errors that can occur during instance management.
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    /// The instance directory already exists on disk.
    #[error("Instance directory already exists: {0}")]
    AlreadyExists(String),

    /// IO error during instance operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
