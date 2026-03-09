//! Global application configuration: data directories, client ID, and paths.

use std::path::PathBuf;

use color_eyre::Result;
use directories::ProjectDirs;

/// MSA Client ID for OAuth2, set at build time via the MUI_MSA_CLIENT_ID env var.
/// Register your own at https://portal.azure.com/ and request approval at
/// https://aka.ms/mce-reviewappid
const DEFAULT_MSA_CLIENT_ID: &str = env!("MUI_MSA_CLIENT_ID");

/// Global application configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Microsoft Azure application Client ID for OAuth2
    pub msa_client_id: String,
    /// Root data directory (e.g., ~/.local/share/mui on Linux)
    pub data_dir: PathBuf,
    /// Directory where instances are stored
    pub instances_dir: PathBuf,
    /// Directory for shared assets (Minecraft assets/objects)
    pub assets_dir: PathBuf,
    /// Directory for shared libraries
    pub libraries_dir: PathBuf,
    /// Directory for version metadata JSONs
    pub versions_dir: PathBuf,
    /// Path to the auth token store file
    pub auth_store_path: PathBuf,
}

impl Config {
    /// Initialize configuration using platform-appropriate directories.
    ///
    /// Uses XDG base directories on Linux (e.g., `~/.local/share/mui`).
    /// Creates all required subdirectories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails or platform data
    /// directory cannot be determined.
    pub fn load() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "mui")
            .ok_or_else(|| color_eyre::eyre::eyre!("Could not determine data directory"))?;

        let data_dir = project_dirs.data_dir().to_path_buf();

        let instances_dir = data_dir.join("instances");
        let assets_dir = data_dir.join("assets");
        let libraries_dir = data_dir.join("libraries");
        let versions_dir = data_dir.join("versions");
        let auth_store_path = data_dir.join("auth.json");

        // Ensure directories exist
        std::fs::create_dir_all(&instances_dir)?;
        std::fs::create_dir_all(&assets_dir)?;
        std::fs::create_dir_all(&libraries_dir)?;
        std::fs::create_dir_all(&versions_dir)?;

        Ok(Config {
            msa_client_id: DEFAULT_MSA_CLIENT_ID.to_string(),
            data_dir,
            instances_dir,
            assets_dir,
            libraries_dir,
            versions_dir,
            auth_store_path,
        })
    }
}
