//! Mojang-provided JRE download and management.
//!
//! Downloads official Java runtimes from Mojang's launcher meta API and installs
//! them into `~/.local/share/mui/java/<component>/`. Each runtime component
//! (e.g., `java-runtime-delta` for Java 21) is stored in its own subdirectory.
//!
//! The Mojang Java runtime manifest is at:
//! `https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json`
//!
//! The manifest structure is:
//! ```text
//! { "<platform>": { "<component>": [{ manifest: { url, sha1 }, version: { name } }] } }
//! ```
//!
//! Each component's manifest lists every file with download URLs (raw + lzma),
//! SHA-1 hashes, and executable flags.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use color_eyre::Result;
use digest::Digest;
use serde::Deserialize;
use sha1::Sha1;
use tokio::{sync::mpsc, task::JoinSet};
#[cfg(not(windows))]
use tracing::{debug, info};
#[cfg(windows)]
use tracing::{debug, info, warn};

use crate::minecraft::download::DownloadProgress;

/// URL to Mojang's Java runtime index.
const RUNTIME_INDEX_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Maximum concurrent file downloads when installing a runtime.
const MAX_CONCURRENT_DOWNLOADS: usize = 20;

// ─── Mojang runtime index types ────────────────────────────────────────────

/// The top-level runtime index: maps platform name to component entries.
///
/// Example platforms: `"linux"`, `"mac-os"`, `"mac-os-arm64"`, `"windows-x64"`.
pub type RuntimeIndex = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

/// A single runtime entry for a component on a platform.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeEntry {
    /// Where to fetch the component's file manifest.
    pub manifest: ManifestRef,
    /// Version info (e.g., `"21.0.7"`).
    pub version: RuntimeVersion,
}

/// Reference to a component's file manifest.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // Fields used for deserialization; sha1/size available for future verification
pub struct ManifestRef {
    /// SHA-1 hash of the manifest JSON.
    pub sha1: String,
    /// Size of the manifest JSON in bytes.
    pub size: u64,
    /// URL to download the manifest JSON.
    pub url: String,
}

/// Version metadata for a runtime component.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeVersion {
    /// Human-readable version string (e.g., `"21.0.7"`, `"8u202"`).
    pub name: String,
}

// ─── Component file manifest types ─────────────────────────────────────────

/// A component's file manifest listing every file to install.
#[derive(Debug, Clone, Deserialize)]
pub struct ComponentManifest {
    pub files: HashMap<String, ComponentFile>,
}

/// A single entry in the component file manifest.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ComponentFile {
    /// A regular file with download info.
    #[serde(rename = "file")]
    File {
        /// Download URLs (raw and optionally lzma-compressed).
        downloads: Option<FileDownloads>,
        /// Whether the file should be marked executable.
        #[serde(default)]
        executable: bool,
    },
    /// A directory to create.
    #[serde(rename = "directory")]
    Directory {},
    /// A symbolic link.
    #[serde(rename = "link")]
    Link {
        /// Target of the symlink.
        target: String,
    },
}

/// Download URLs for a file, with raw (uncompressed) and optional lzma variants.
#[derive(Debug, Clone, Deserialize)]
pub struct FileDownloads {
    /// The uncompressed download.
    pub raw: FileDownloadInfo,
    // Note: lzma variant exists but we only use raw for simplicity.
    // Adding lzma support later would reduce download sizes.
}

/// Download metadata for a single file variant.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // size field used for deserialization; available for future progress reporting
pub struct FileDownloadInfo {
    /// SHA-1 hash for verification.
    pub sha1: String,
    /// File size in bytes.
    pub size: u64,
    /// Download URL.
    pub url: String,
}

// ─── Platform detection ────────────────────────────────────────────────────

/// Return the Mojang platform key for the current OS and architecture.
///
/// Mojang uses these platform keys:
/// - `linux`, `linux-i386`
/// - `mac-os`, `mac-os-arm64`
/// - `windows-x64`, `windows-x86`, `windows-arm64`
pub fn current_platform() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86"))]
    {
        "linux-i386"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux"
    } // Mojang doesn't provide linux-arm64; fall back to linux
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "mac-os-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "mac-os"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86"))]
    {
        "windows-x86"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "windows-arm64"
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    compile_error!(
        "Unsupported platform/architecture for MUI managed Java runtimes. \
         Supported: linux (x86_64/x86/aarch64), macOS (x86_64/aarch64), \
         Windows (x86_64/x86/aarch64)."
    );
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Fetch the Mojang Java runtime index (`all.json`).
pub async fn fetch_runtime_index(http: &reqwest::Client) -> Result<RuntimeIndex> {
    info!("Fetching Mojang Java runtime index...");
    let index: RuntimeIndex = http.get(RUNTIME_INDEX_URL).send().await?.json().await?;
    debug!("Runtime index has {} platforms", index.len());
    Ok(index)
}

/// Look up a specific component for the current platform in the runtime index.
///
/// Returns the first (most recent) entry for the component, or `None` if
/// the component is not available for this platform.
pub fn find_component<'a>(index: &'a RuntimeIndex, component: &str) -> Option<&'a RuntimeEntry> {
    let platform = current_platform();
    index
        .get(platform)
        .and_then(|components| components.get(component))
        .and_then(|entries| entries.first())
}

/// Return the path to the `java` binary for a managed runtime component.
///
/// Returns `None` if the runtime is not installed.
///
/// On macOS, Mojang's runtimes use a `jre.bundle` structure, so the binary
/// lives at `<component>/jre.bundle/Contents/Home/bin/java` rather than
/// `<component>/bin/java`.
pub fn get_java_path(java_dir: &Path, component: &str) -> Option<PathBuf> {
    let component_dir = java_dir.join(component);

    // Check platform-specific paths in order of likelihood
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &["bin/java.exe"]
    } else if cfg!(target_os = "macos") {
        &["jre.bundle/Contents/Home/bin/java", "bin/java"]
    } else {
        &["bin/java"]
    };

    for candidate in candidates {
        let path = component_dir.join(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Check whether a managed runtime component is installed.
#[allow(dead_code)] // Public utility for checking runtime availability
pub fn is_installed(java_dir: &Path, component: &str) -> bool {
    get_java_path(java_dir, component).is_some()
}

/// List all installed managed runtimes.
///
/// Returns a list of `(component_name, java_binary_path)` pairs.
pub fn list_installed(java_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut installed = Vec::new();
    if let Ok(entries) = std::fs::read_dir(java_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(java_path) = get_java_path(java_dir, &name) {
                installed.push((name, java_path));
            }
        }
    }
    installed
}

/// Download and install a Mojang Java runtime component.
///
/// Fetches the component's file manifest, downloads all files with SHA-1
/// verification, creates directories, sets executable permissions on Unix,
/// and creates symlinks. Files that already exist and pass verification
/// are skipped.
///
/// Progress updates are sent through `progress_tx` if provided, reusing the
/// existing [`DownloadProgress`] type from the download engine.
pub async fn download_runtime(
    component: &str,
    manifest_url: &str,
    java_dir: &Path,
    http: &reqwest::Client,
    progress_tx: Option<mpsc::Sender<DownloadProgress>>,
) -> Result<PathBuf> {
    info!("Downloading Java runtime '{component}'...");

    // Fetch the component's file manifest
    let manifest: ComponentManifest = http.get(manifest_url).send().await?.json().await?;
    let component_dir = java_dir.join(component);

    // First pass: create all directories
    for (path, file) in &manifest.files {
        if matches!(file, ComponentFile::Directory { .. }) {
            let dir_path = component_dir.join(path);
            tokio::fs::create_dir_all(&dir_path).await?;
        }
    }

    // Collect files to download
    let mut download_tasks: Vec<(String, FileDownloadInfo, bool)> = Vec::new();
    let mut link_tasks: Vec<(String, String)> = Vec::new();

    for (path, file) in &manifest.files {
        match file {
            ComponentFile::File {
                downloads,
                executable,
            } => {
                if let Some(dl) = downloads {
                    let file_path = component_dir.join(path);

                    // Skip if already exists and SHA-1 matches
                    if file_path.exists() && verify_sha1(&file_path, &dl.raw.sha1) {
                        debug!("Skipping (verified): {path}");
                        continue;
                    }

                    // Ensure parent directory exists
                    if let Some(parent) = file_path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }

                    download_tasks.push((path.clone(), dl.raw.clone(), *executable));
                }
            }
            ComponentFile::Link { target } => {
                link_tasks.push((path.clone(), target.clone()));
            }
            ComponentFile::Directory { .. } => {} // Already handled above
        }
    }

    let total_files = download_tasks.len();
    info!(
        "Java runtime '{component}': {total_files} files to download, {} up to date",
        manifest.files.len() - total_files - link_tasks.len()
    );

    // Download files concurrently
    let completed = Arc::new(AtomicUsize::new(0));
    let sem = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut join_set = JoinSet::new();

    for (path, dl_info, executable) in download_tasks {
        let http = http.clone();
        let component_dir = component_dir.clone();
        let sem = sem.clone();
        let completed = completed.clone();
        let progress_tx = progress_tx.clone();

        join_set.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");

            let file_path = component_dir.join(&path);
            let bytes = http.get(&dl_info.url).send().await?.bytes().await?;

            // Verify SHA-1
            let mut hasher = Sha1::new();
            hasher.update(&bytes);
            let hash = format!("{:x}", hasher.finalize());
            if hash != dl_info.sha1 {
                return Err(color_eyre::eyre::eyre!(
                    "SHA-1 mismatch for {path}: expected {}, got {hash}",
                    dl_info.sha1
                ));
            }

            tokio::fs::write(&file_path, &bytes).await?;

            // Set executable permission on Unix
            #[cfg(unix)]
            if executable {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                tokio::fs::set_permissions(&file_path, perms).await?;
            }

            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(ref tx) = progress_tx {
                let _ = tx
                    .send(DownloadProgress {
                        total_files,
                        completed_files: done,
                        current_file: path.clone(),
                    })
                    .await;
            }

            Ok::<(), color_eyre::Report>(())
        });
    }

    // Wait for all downloads to complete
    while let Some(result) = join_set.join_next().await {
        result??;
    }

    // Create symlinks
    #[cfg(unix)]
    for (path, target) in &link_tasks {
        let link_path = component_dir.join(path);
        // Remove existing symlink/file if present
        let _ = tokio::fs::remove_file(&link_path).await;
        if let Some(parent) = link_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::symlink(target, &link_path).await?;
    }

    #[cfg(windows)]
    for (path, target) in &link_tasks {
        let link_path = component_dir.join(path);
        let _ = tokio::fs::remove_file(&link_path).await;
        if let Some(parent) = link_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // On Windows, try file symlink first (requires developer mode or admin).
        // If that fails, fall back to copying the target file instead.
        if let Err(e) = tokio::fs::symlink_file(target, &link_path).await {
            warn!(
                "Symlink creation failed for {path} -> {target}: {e}. \
                 Falling back to file copy."
            );
            // Resolve the target path relative to the link's parent directory
            let target_path = link_path
                .parent()
                .map(|p| p.join(target))
                .unwrap_or_else(|| component_dir.join(target));
            if target_path.exists() {
                tokio::fs::copy(&target_path, &link_path).await?;
            } else {
                warn!(
                    "Copy fallback failed: target {target} does not exist at {}",
                    target_path.display()
                );
            }
        }
    }

    let java_path = get_java_path(java_dir, component).ok_or_else(|| {
        color_eyre::eyre::eyre!("Java binary not found after installing runtime '{component}'")
    })?;

    info!(
        "Java runtime '{component}' installed at {}",
        java_path.display()
    );
    Ok(java_path)
}

/// Verify a file's SHA-1 hash.
fn verify_sha1(path: &Path, expected: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let hash = format!("{:x}", hasher.finalize());
    hash == expected
}
