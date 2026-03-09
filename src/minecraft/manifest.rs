//! Fetch and parse the Minecraft version manifest.
//!
//! The manifest at piston-meta.mojang.com lists all available Minecraft versions
//! with links to their individual metadata JSONs.

use color_eyre::Result;
use serde::Deserialize;
use tracing::debug;

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// The top-level version manifest listing all available Minecraft versions.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    /// Latest release and snapshot version IDs.
    pub latest: LatestVersions,
    /// All available versions.
    pub versions: Vec<VersionEntry>,
}

/// Latest stable and snapshot version identifiers.
#[derive(Debug, Clone, Deserialize)]
pub struct LatestVersions {
    /// Latest stable release version ID (e.g., "1.21.4").
    pub release: String,
    /// Latest snapshot version ID.
    pub snapshot: String,
}

/// A single version entry from the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    /// Version identifier (e.g., "1.21.4").
    pub id: String,
    /// Version type category.
    #[serde(rename = "type")]
    pub version_type: VersionType,
    /// URL to the version's metadata JSON.
    pub url: String,
}

/// Category of a Minecraft version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

impl std::fmt::Display for VersionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Release => write!(f, "release"),
            Self::Snapshot => write!(f, "snapshot"),
            Self::OldBeta => write!(f, "old_beta"),
            Self::OldAlpha => write!(f, "old_alpha"),
        }
    }
}

/// Fetch the version manifest from Mojang's servers.
pub async fn fetch_manifest(http: &reqwest::Client) -> Result<VersionManifest> {
    debug!("Fetching version manifest...");

    let manifest: VersionManifest = http.get(VERSION_MANIFEST_URL).send().await?.json().await?;

    debug!(
        "Got {} versions (latest release: {}, snapshot: {})",
        manifest.versions.len(),
        manifest.latest.release,
        manifest.latest.snapshot
    );

    Ok(manifest)
}
