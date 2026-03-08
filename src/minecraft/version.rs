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

/// Top-level version metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionMeta {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,

    /// Modern argument format (1.13+)
    pub arguments: Option<Arguments>,
    /// Legacy argument format (pre-1.13)
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,

    pub libraries: Vec<Library>,

    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,

    pub downloads: Downloads,
}

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

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: String, // "allow" or "disallow"
    pub os: Option<OsRule>,
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub name: String, // Maven coordinates: group:artifact:version
    pub downloads: Option<LibraryDownloads>,
    pub rules: Option<Vec<Rule>>,
    pub natives: Option<HashMap<String, String>>,
    pub url: Option<String>, // Repository base URL (for libs without downloads.artifact)
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadInfo>,
    pub classifiers: Option<HashMap<String, DownloadInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadInfo {
    pub path: Option<String>,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub client: DownloadInfo,
}

/// The asset index JSON, mapping logical paths to hash+size.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
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
