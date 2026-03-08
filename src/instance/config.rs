//! Per-instance configuration (memory, Java path, window size, etc.)

use serde::{Deserialize, Serialize};

/// Configuration for a single Minecraft instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// Human-readable instance name
    pub name: String,
    /// Minecraft version ID (e.g., "1.21.4")
    pub version_id: String,
    /// URL to the version metadata JSON
    pub version_url: String,
    /// Optional custom Java path (None = auto-detect)
    pub java_path: Option<String>,
    /// Minimum heap memory in MB
    pub min_memory_mb: u32,
    /// Maximum heap memory in MB
    pub max_memory_mb: u32,
    /// Window width
    pub window_width: u32,
    /// Window height
    pub window_height: u32,
    /// Custom JVM arguments
    pub jvm_args: Vec<String>,
    /// When this instance was created
    pub created_at: String,
    /// When this instance was last launched
    pub last_played: Option<String>,
}

impl InstanceConfig {
    pub fn new(name: String, version_id: String, version_url: String) -> Self {
        Self {
            name,
            version_id,
            version_url,
            java_path: None,
            min_memory_mb: 512,
            max_memory_mb: 2048,
            window_width: 854,
            window_height: 480,
            jvm_args: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_played: None,
        }
    }
}
