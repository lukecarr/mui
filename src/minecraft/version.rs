//! Parse individual version metadata JSON files.
//!
//! Each Minecraft version has a metadata JSON describing:
//! - Required libraries (with download URLs and platform rules)
//! - Asset index reference
//! - Client JAR download info
//! - JVM and game arguments (with token placeholders)
//! - Main class to launch

use std::collections::HashMap;

use color_eyre::Result;
use serde::Deserialize;
use tracing::debug;

/// Top-level version metadata for a specific Minecraft version.
///
/// Parsed from the version metadata JSON linked in the version manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionMeta {
    /// Version identifier (e.g., "1.21.4").
    pub id: String,
    /// Version type string (e.g., "release", "snapshot").
    #[serde(rename = "type")]
    pub version_type: String,
    /// Fully-qualified Java main class to launch.
    #[serde(rename = "mainClass")]
    pub main_class: String,

    /// Modern argument format (1.13+).
    pub arguments: Option<Arguments>,
    /// Legacy argument format (pre-1.13).
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,

    /// Required libraries with download info and platform rules.
    pub libraries: Vec<Library>,

    /// Reference to the asset index for this version.
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,

    /// Client JAR and other download artifacts.
    pub downloads: Downloads,
}

/// JVM and game arguments for modern versions (1.13+).
#[derive(Debug, Clone, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentValue>,
    #[serde(default)]
    pub jvm: Vec<ArgumentValue>,
}

/// An argument value is either a plain string or a conditional object.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Simple(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValueInner,
    },
}

/// The value inside a conditional argument can be a single string or an array.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValueInner {
    Single(String),
    Multiple(Vec<String>),
}

/// A platform rule that determines whether a library or argument applies.
#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    /// `"allow"` or `"disallow"`.
    pub action: String,
    /// OS-specific conditions.
    pub os: Option<OsRule>,
    /// Feature flag conditions (e.g., demo mode, quick play).
    pub features: Option<HashMap<String, bool>>,
}

/// OS-specific conditions within a [`Rule`].
#[derive(Debug, Clone, Deserialize)]
pub struct OsRule {
    /// Target OS name (e.g., `"windows"`, `"osx"`, `"linux"`).
    pub name: Option<String>,
    /// Target architecture (e.g., `"x86"`, `"x86_64"`).
    pub arch: Option<String>,
}

/// A library dependency required by the Minecraft version.
#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    /// Maven coordinates: `group:artifact:version[:classifier]`.
    pub name: String,
    /// Download information for the library artifact.
    pub downloads: Option<LibraryDownloads>,
    /// Platform rules that determine if this library should be included.
    pub rules: Option<Vec<Rule>>,
    /// Map of OS name to native classifier template (e.g., `"linux"` -> `"natives-linux"`).
    pub natives: Option<HashMap<String, String>>,
    /// Custom repository base URL (for libraries not hosted on libraries.minecraft.net).
    pub url: Option<String>,
}

/// Download information for a library's artifacts and classifiers.
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryDownloads {
    /// The main library artifact.
    pub artifact: Option<DownloadInfo>,
    /// Platform-specific native classifiers.
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

/// Download metadata for a single file.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfo {
    /// Relative file path within the libraries directory.
    pub path: Option<String>,
    /// Download URL.
    pub url: String,
    /// Expected SHA-1 hash for verification.
    pub sha1: Option<String>,
    /// File size in bytes.
    pub size: Option<u64>,
}

/// Reference to a version's asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    /// Asset index identifier (e.g., "17").
    pub id: String,
    /// SHA-1 hash of the asset index JSON.
    pub sha1: String,
    /// Size of the asset index JSON in bytes.
    pub size: u64,
    /// URL to download the asset index JSON.
    pub url: String,
}

/// Top-level download artifacts for the version.
#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    /// The client JAR download information.
    pub client: DownloadInfo,
}

/// The asset index JSON, mapping logical paths to hash+size.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

/// A single asset object in the asset index.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    /// SHA-1 hash of the asset (also used as the filename).
    pub hash: String,
    /// File size in bytes.
    pub size: u64,
}

/// Fetch and parse the version metadata JSON for a specific version.
pub async fn fetch_version_meta(url: &str, http: &reqwest::Client) -> Result<VersionMeta> {
    debug!("Fetching version metadata from {url}...");
    let meta: VersionMeta = http.get(url).send().await?.json().await?;
    debug!(
        "Version {}: {} libraries, main class: {}",
        meta.id,
        meta.libraries.len(),
        meta.main_class
    );
    Ok(meta)
}

/// Fetch and parse an asset index JSON.
pub async fn fetch_asset_index(url: &str, http: &reqwest::Client) -> Result<AssetIndex> {
    debug!("Fetching asset index from {url}...");
    let index: AssetIndex = http.get(url).send().await?.json().await?;
    debug!("Asset index has {} objects", index.objects.len());
    Ok(index)
}

impl Library {
    /// Convert Maven coordinates to a file path.
    /// e.g., "com.mojang:brigadier:1.0.18" -> "com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar"
    pub fn maven_path(&self) -> Option<String> {
        let parts: Vec<&str> = self.name.split(':').collect();
        if parts.len() < 3 {
            return None;
        }
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        // Handle optional classifier (4th part)
        if parts.len() >= 4 {
            let classifier = parts[3];
            Some(format!(
                "{group}/{artifact}/{version}/{artifact}-{version}-{classifier}.jar"
            ))
        } else {
            Some(format!(
                "{group}/{artifact}/{version}/{artifact}-{version}.jar"
            ))
        }
    }
}
