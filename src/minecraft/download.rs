//! Download engine: assets, libraries, client JAR.
//!
//! Downloads files with SHA-1 verification and progress reporting via a channel.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use digest::Digest;
use sha1::Sha1;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::MinecraftError;
use super::rules;
use super::version::{AssetIndex, VersionMeta};

type Result<T> = std::result::Result<T, MinecraftError>;

/// Progress update sent during downloads.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Total number of files to download.
    pub total_files: usize,
    /// Number of files downloaded so far.
    pub completed_files: usize,
    /// Label of the file currently being downloaded.
    pub current_file: String,
}

/// Download all required files for a version: client JAR, libraries, and assets.
///
/// Files that already exist and pass SHA-1 verification are skipped.
/// Progress updates are sent through `progress_tx` if provided.
///
/// # Errors
///
/// Returns [`MinecraftError`] if any download or verification step fails.
pub async fn download_version(
    meta: &VersionMeta,
    asset_index: &AssetIndex,
    libraries_dir: &Path,
    assets_dir: &Path,
    versions_dir: &Path,
    http: &reqwest::Client,
    progress_tx: Option<mpsc::Sender<DownloadProgress>>,
) -> Result<()> {
    // Collect all download tasks
    let mut tasks: Vec<DownloadTask> = Vec::new();

    // 1. Client JAR
    let client_jar_path = versions_dir.join(&meta.id).join(format!("{}.jar", meta.id));
    tasks.push(DownloadTask {
        url: meta.downloads.client.url.clone(),
        path: client_jar_path,
        sha1: meta.downloads.client.sha1.clone(),
        size: meta.downloads.client.size.unwrap_or(0),
        label: format!("client {}", meta.id),
    });

    // 2. Libraries
    for lib in &meta.libraries {
        if let Some(ref lib_rules) = lib.rules
            && !rules::rules_match(lib_rules)
        {
            continue;
        }

        // Regular artifact
        if let Some(ref downloads) = lib.downloads {
            if let Some(ref artifact) = downloads.artifact
                && let Some(ref path) = artifact.path
            {
                let dest = libraries_dir.join(path);
                tasks.push(DownloadTask {
                    url: artifact.url.clone(),
                    path: dest,
                    sha1: artifact.sha1.clone(),
                    size: artifact.size.unwrap_or(0),
                    label: lib.name.clone(),
                });
            }

            // Natives (platform-specific classifiers)
            if let Some(ref natives_map) = lib.natives {
                let os = rules::current_os();
                if let Some(classifier_template) = natives_map.get(os) {
                    // Replace ${arch} placeholder in classifier name
                    let classifier = classifier_template.replace(
                        "${arch}",
                        if cfg!(target_arch = "x86_64") {
                            "64"
                        } else {
                            "32"
                        },
                    );
                    if let Some(ref classifiers) = downloads.classifiers
                        && let Some(native_dl) = classifiers.get(&classifier)
                        && let Some(ref path) = native_dl.path
                    {
                        let dest = libraries_dir.join(path);
                        tasks.push(DownloadTask {
                            url: native_dl.url.clone(),
                            path: dest,
                            sha1: native_dl.sha1.clone(),
                            size: native_dl.size.unwrap_or(0),
                            label: format!("{} (native)", lib.name),
                        });
                    }
                }
            }
        } else if let Some(ref base_url) = lib.url {
            // Library with a custom repository URL but no explicit download info
            if let Some(path) = lib.maven_path() {
                let url = format!("{base_url}{path}");
                let dest = libraries_dir.join(&path);
                tasks.push(DownloadTask {
                    url,
                    path: dest,
                    sha1: None,
                    size: 0,
                    label: lib.name.clone(),
                });
            }
        } else if let Some(path) = lib.maven_path() {
            // Default to libraries.minecraft.net
            let url = format!("https://libraries.minecraft.net/{path}");
            let dest = libraries_dir.join(&path);
            tasks.push(DownloadTask {
                url,
                path: dest,
                sha1: None,
                size: 0,
                label: lib.name.clone(),
            });
        }
    }

    // 3. Assets
    let objects_dir = assets_dir.join("objects");
    for obj in asset_index.objects.values() {
        let prefix = &obj.hash[..2];
        let dest = objects_dir.join(prefix).join(&obj.hash);
        let url = format!(
            "https://resources.download.minecraft.net/{prefix}/{}",
            obj.hash
        );
        tasks.push(DownloadTask {
            url,
            path: dest,
            sha1: Some(obj.hash.clone()),
            size: obj.size,
            label: obj.hash[..8].to_string(),
        });
    }

    // 4. Asset index JSON
    let index_path = assets_dir
        .join("indexes")
        .join(format!("{}.json", meta.asset_index.id));
    tasks.push(DownloadTask {
        url: meta.asset_index.url.clone(),
        path: index_path,
        sha1: Some(meta.asset_index.sha1.clone()),
        size: meta.asset_index.size,
        label: format!("asset index {}", meta.asset_index.id),
    });

    let total_files = tasks.len();
    let total_bytes: u64 = tasks.iter().map(|t| t.size).sum();
    info!(
        "Need to check/download {} files ({:.1} MB)",
        total_files,
        total_bytes as f64 / 1_048_576.0
    );

    // Filter out already-downloaded files
    let tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| !file_valid(&task.path, &task.sha1))
        .collect();

    let pending = tasks.len();
    if pending == 0 {
        info!("All files already downloaded and verified");
        return Ok(());
    }

    info!("{pending} files need downloading");

    let mut completed = total_files - pending;

    for task in &tasks {
        if let Some(ref tx) = progress_tx {
            let _ = tx
                .send(DownloadProgress {
                    total_files,
                    completed_files: completed,
                    current_file: task.label.clone(),
                })
                .await;
        }

        download_file(http, task).await?;
        completed += 1;
    }

    if let Some(ref tx) = progress_tx {
        let _ = tx
            .send(DownloadProgress {
                total_files,
                completed_files: total_files,
                current_file: "Done".to_string(),
            })
            .await;
    }

    info!("All downloads complete");
    Ok(())
}

struct DownloadTask {
    url: String,
    path: PathBuf,
    sha1: Option<String>,
    size: u64,
    label: String,
}

/// Check if a file exists and matches its expected SHA-1 hash.
fn file_valid(path: &Path, expected_sha1: &Option<String>) -> bool {
    if !path.exists() {
        return false;
    }
    if let Some(expected) = expected_sha1 {
        if let Ok(data) = std::fs::read(path) {
            let hash = hex_sha1(&data);
            return hash == *expected;
        }
        return false;
    }
    // If no hash to check, existence is enough
    true
}

/// Download a single file, verify its SHA-1, and write it to disk.
async fn download_file(http: &reqwest::Client, task: &DownloadTask) -> Result<()> {
    debug!("Downloading {} -> {:?}", task.url, task.path);

    // Ensure parent directory exists
    if let Some(parent) = task.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = http.get(&task.url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        // Consume the response body (capped at 1 KiB) so the connection can be
        // reused, and include a truncated snippet in the error for diagnostics.
        const MAX_ERROR_BODY: usize = 1024;
        let full_body = resp.text().await.unwrap_or_default();
        let body = if full_body.len() > MAX_ERROR_BODY {
            format!("{}... (truncated)", &full_body[..MAX_ERROR_BODY])
        } else {
            full_body
        };
        return Err(MinecraftError::DownloadFailed {
            label: task.label.clone(),
            status: status.to_string(),
            url: task.url.clone(),
            body,
        });
    }

    let data = resp.bytes().await?;

    // Verify SHA-1 if expected
    if let Some(ref expected) = task.sha1 {
        let actual = hex_sha1(&data);
        if actual != *expected {
            warn!(
                "SHA-1 mismatch for {}: expected {}, got {}",
                task.label, expected, actual
            );
            return Err(MinecraftError::Sha1Mismatch(task.label.clone()));
        }
    }

    tokio::fs::write(&task.path, &data).await?;
    Ok(())
}

/// Compute the hex-encoded SHA-1 hash of some data.
///
/// Uses a pre-allocated buffer to avoid per-byte string allocations.
fn hex_sha1(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(40);
    for b in result {
        // write! to a String is infallible
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// Collect paths for all libraries that should be on the classpath.
///
/// Evaluates platform rules to include only applicable libraries.
/// The client JAR is appended last.
pub fn collect_classpath(
    meta: &VersionMeta,
    libraries_dir: &Path,
    versions_dir: &Path,
) -> Vec<PathBuf> {
    let mut classpath = Vec::new();

    for lib in &meta.libraries {
        // Check rules
        if let Some(ref lib_rules) = lib.rules
            && !rules::rules_match(lib_rules)
        {
            continue;
        }

        // Skip native-only libraries (they go to the natives dir, not classpath)
        if lib.natives.is_some()
            && lib
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .is_none()
        {
            continue;
        }

        // Get the artifact path
        if let Some(ref downloads) = lib.downloads {
            if let Some(ref artifact) = downloads.artifact
                && let Some(ref path) = artifact.path
            {
                classpath.push(libraries_dir.join(path));
            }
        } else if let Some(path) = lib.maven_path() {
            classpath.push(libraries_dir.join(path));
        }
    }

    // Client JAR goes last on classpath
    let client_jar = versions_dir.join(&meta.id).join(format!("{}.jar", meta.id));
    classpath.push(client_jar);

    classpath
}

/// Collect paths for all native library JARs that need to be extracted.
///
/// Evaluates platform rules and natives classifier maps to find
/// the correct native JARs for the current OS and architecture.
pub fn collect_native_jars(meta: &VersionMeta, libraries_dir: &Path) -> Vec<PathBuf> {
    let mut natives = Vec::new();

    for lib in &meta.libraries {
        if let Some(ref lib_rules) = lib.rules
            && !rules::rules_match(lib_rules)
        {
            continue;
        }

        if let Some(ref natives_map) = lib.natives {
            let os = rules::current_os();
            if let Some(classifier_template) = natives_map.get(os) {
                let classifier = classifier_template.replace(
                    "${arch}",
                    if cfg!(target_arch = "x86_64") {
                        "64"
                    } else {
                        "32"
                    },
                );
                if let Some(ref downloads) = lib.downloads
                    && let Some(ref classifiers) = downloads.classifiers
                    && let Some(native_dl) = classifiers.get(&classifier)
                    && let Some(ref path) = native_dl.path
                {
                    natives.push(libraries_dir.join(path));
                }
            }
        }
    }

    natives
}

/// Extract native libraries (`.so`, `.dll`, `.dylib`) from their JARs into a directory.
///
/// Skips `META-INF` entries and directories. Only extracts files with
/// native library extensions.
///
/// # Errors
///
/// Returns [`MinecraftError`] if a JAR file can't be opened or extraction fails.
pub fn extract_natives(native_jars: &[PathBuf], natives_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(natives_dir)?;

    for jar_path in native_jars {
        if !jar_path.exists() {
            warn!("Native JAR not found: {jar_path:?}");
            continue;
        }

        debug!("Extracting natives from {jar_path:?}");
        let file = std::fs::File::open(jar_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();

            // Skip META-INF and directories
            if name.starts_with("META-INF") || name.ends_with('/') {
                continue;
            }

            // Only extract shared libraries
            if name.ends_with(".so")
                || name.ends_with(".dll")
                || name.ends_with(".dylib")
                || name.ends_with(".jnilib")
            {
                let dest = natives_dir.join(
                    Path::new(&name)
                        .file_name()
                        .unwrap_or(std::ffi::OsStr::new(&name)),
                );
                let mut outfile = std::fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut outfile)?;
                debug!("Extracted {name} -> {dest:?}");
            }
        }
    }

    Ok(())
}
