//! Enhanced Java detection with version validation.
//!
//! Replaces the basic `detect_java()` from `launch.rs` with a version-aware
//! resolution chain:
//!
//! 1. Instance override (`java_path` in instance config) — used as-is, no validation
//! 2. MUI-managed runtime matching the required component
//! 3. System Java (`JAVA_HOME`, `PATH`, common install locations) with major version validation
//! 4. Error with a helpful message

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use tracing::{debug, info, warn};

use super::runtime;

/// The result of Java resolution, indicating which source was used.
#[derive(Debug, Clone)]
pub struct ResolvedJava {
    /// Path to the `java` binary.
    pub path: String,
    /// How the Java binary was found.
    pub source: JavaSource,
    /// Detected major version (if we were able to determine it).
    pub major_version: Option<u32>,
}

/// How a Java installation was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaSource {
    /// User-specified path in instance config.
    InstanceOverride,
    /// MUI-managed Mojang runtime.
    Managed { component: String },
    /// Found on the system (JAVA_HOME, PATH, or common locations).
    System,
}

/// Resolve the Java binary to use for launching Minecraft.
///
/// Tries, in order:
/// 1. Instance override (`instance_java_path`)
/// 2. MUI-managed runtime for `required_component`
/// 3. System Java, validated against `required_major`
///
/// Returns `None` if no suitable Java could be found.
pub fn resolve_java(
    java_dir: &Path,
    required_component: Option<&str>,
    required_major: Option<u32>,
    instance_java_path: Option<&str>,
) -> Option<ResolvedJava> {
    // 1. Instance override — trust the user, skip validation
    if let Some(path) = instance_java_path {
        if Path::new(path).exists() {
            let version = get_java_major_version(path);
            info!("Using instance Java override: {path}");
            return Some(ResolvedJava {
                path: path.to_string(),
                source: JavaSource::InstanceOverride,
                major_version: version,
            });
        }
        warn!("Instance Java override path does not exist: {path}");
    }

    // 2. MUI-managed runtime
    if let Some(component) = required_component {
        if let Some(java_path) = runtime::get_java_path(java_dir, component) {
            let path_str = java_path.to_string_lossy().to_string();
            let version = get_java_major_version(&path_str);
            info!("Using managed Java runtime '{component}': {path_str}");
            return Some(ResolvedJava {
                path: path_str,
                source: JavaSource::Managed {
                    component: component.to_string(),
                },
                major_version: version,
            });
        }
        debug!("Managed runtime '{component}' not installed");
    }

    // 3. System Java with version validation
    if let Some(java) = detect_system_java(required_major) {
        return Some(java);
    }

    None
}

/// Parse the major version number from `java -version` output.
///
/// The output format varies:
/// - Java 8: `java version "1.8.0_202"` or `openjdk version "1.8.0_312"`
/// - Java 9+: `java version "9.0.1"` or `openjdk version "17.0.8"`
///
/// Returns `None` if the version cannot be determined.
pub fn get_java_major_version(java_path: &str) -> Option<u32> {
    let output = Command::new(java_path).arg("-version").output().ok()?;

    // `java -version` writes to stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_java_version(&stderr)
}

/// Parse the major version from `java -version` stderr output.
fn parse_java_version(output: &str) -> Option<u32> {
    // Look for a quoted version string like "1.8.0_202" or "21.0.7"
    let version_line = output.lines().next()?;
    let start = version_line.find('"')? + 1;
    let end = version_line[start..].find('"')? + start;
    let version_str = &version_line[start..end];

    // Split on '.' and parse
    let parts: Vec<&str> = version_str.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let first: u32 = parts[0].parse().ok()?;
    if first == 1 && parts.len() >= 2 {
        // Java 8 and earlier use 1.x format: "1.8.0_202" -> major = 8
        parts[1].parse().ok()
    } else {
        // Java 9+ uses direct versioning: "17.0.8" -> major = 17
        Some(first)
    }
}

/// Detect a system Java installation, optionally validating the major version.
///
/// Searches in order:
/// 1. `JAVA_HOME` environment variable
/// 2. `PATH` (via `which`/`where`)
/// 3. Common install locations (platform-specific)
///
/// If `required_major` is `Some`, only returns a Java whose major version matches.
/// If `required_major` is `None`, returns the first Java found.
fn detect_system_java(required_major: Option<u32>) -> Option<ResolvedJava> {
    let java_bin = if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    };

    let mut candidates: Vec<String> = Vec::new();

    // 1. JAVA_HOME
    //
    // JAVA_HOME is a user-controlled environment variable. We validate that
    // it is a real, absolute directory and that the resolved binary lives
    // within it (via canonicalization) to guard against path traversal.
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let home_path = Path::new(&java_home);
        if home_path.is_absolute() && home_path.is_dir() {
            let java_path = home_path.join("bin").join(java_bin);
            if java_path.exists() {
                // Canonicalize both paths and verify the binary is inside JAVA_HOME
                if let (Ok(canon_home), Ok(canon_java)) =
                    (home_path.canonicalize(), java_path.canonicalize())
                {
                    if canon_java.starts_with(&canon_home) {
                        candidates.push(canon_java.to_string_lossy().to_string());
                    } else {
                        warn!(
                            "JAVA_HOME binary resolved outside JAVA_HOME (symlink escape?): {}",
                            canon_java.display()
                        );
                    }
                }
            }
        } else {
            debug!("Ignoring non-absolute or non-existent JAVA_HOME: {java_home}");
        }
    }

    // 2. PATH via which/where
    let find_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    if let Ok(output) = Command::new(find_cmd).arg("java").output()
        && output.status.success()
    {
        // `where` on Windows can return multiple lines
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = line.trim().to_string();
            if !path.is_empty() && !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }

    // 3. Common install locations
    add_common_locations(&mut candidates);

    // Evaluate candidates
    // First pass: find one matching the required major version
    if let Some(required) = required_major {
        for path in &candidates {
            if let Some(major) = get_java_major_version(path) {
                if major == required {
                    info!("Found system Java {major} at {path}");
                    return Some(ResolvedJava {
                        path: path.clone(),
                        source: JavaSource::System,
                        major_version: Some(major),
                    });
                }
                debug!("System Java at {path} is version {major}, need {required}");
            }
        }
    }

    // If a specific version was required but not found, return None so the
    // caller can auto-download the correct managed runtime instead of launching
    // with an incompatible Java (which will just crash Minecraft).
    if let Some(required) = required_major {
        warn!(
            "No system Java {required} found (searched {} candidates)",
            candidates.len()
        );
        return None;
    }

    // No version requirement: return the first working Java we can find.
    for path in &candidates {
        let major = get_java_major_version(path);
        if major.is_some() || Path::new(path).exists() {
            info!("Found system Java at {path}");
            return Some(ResolvedJava {
                path: path.clone(),
                source: JavaSource::System,
                major_version: major,
            });
        }
    }

    None
}

/// Add platform-specific common Java install locations to the candidate list.
fn add_common_locations(candidates: &mut Vec<String>) {
    #[cfg(target_os = "linux")]
    {
        // Common Linux JDK/JRE locations
        let jvm_dir = Path::new("/usr/lib/jvm");
        if let Ok(entries) = std::fs::read_dir(jvm_dir) {
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path().join("bin/java"))
                .filter(|p| p.exists())
                .collect();
            // Sort to prefer newer versions (higher names sort later)
            paths.sort();
            paths.reverse();
            for p in paths {
                let s = p.to_string_lossy().to_string();
                if !candidates.contains(&s) {
                    candidates.push(s);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: /Library/Java/JavaVirtualMachines/*/Contents/Home/bin/java
        for base in &[
            "/Library/Java/JavaVirtualMachines",
            &format!(
                "{}/Library/Java/JavaVirtualMachines",
                std::env::var("HOME").unwrap_or_default()
            ),
        ] {
            if let Ok(entries) = std::fs::read_dir(base) {
                let mut paths: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path().join("Contents/Home/bin/java"))
                    .filter(|p| p.exists())
                    .collect();
                paths.sort();
                paths.reverse();
                for p in paths {
                    let s = p.to_string_lossy().to_string();
                    if !candidates.contains(&s) {
                        candidates.push(s);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates_list = [
            "C:\\Program Files\\Java\\jdk-21\\bin\\java.exe",
            "C:\\Program Files\\Java\\jdk-17\\bin\\java.exe",
            "C:\\Program Files\\Java\\jdk-8\\bin\\java.exe",
            "C:\\Program Files\\Java\\jre-1.8\\bin\\java.exe",
            "C:\\Program Files\\Eclipse Adoptium\\jdk-21\\bin\\java.exe",
            "C:\\Program Files\\Eclipse Adoptium\\jdk-17\\bin\\java.exe",
            "C:\\Program Files\\Eclipse Adoptium\\jdk-8\\bin\\java.exe",
            "C:\\Program Files\\Microsoft\\jdk-21\\bin\\java.exe",
            "C:\\Program Files\\Microsoft\\jdk-17\\bin\\java.exe",
            "C:\\Program Files\\Zulu\\zulu-21\\bin\\java.exe",
            "C:\\Program Files\\Zulu\\zulu-17\\bin\\java.exe",
        ];
        for path in &candidates_list {
            if Path::new(path).exists() && !candidates.contains(&path.to_string()) {
                candidates.push(path.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_java_8_version() {
        let output = r#"openjdk version "1.8.0_312"
OpenJDK Runtime Environment (build 1.8.0_312-8u312-b07-0ubuntu1-b07)
OpenJDK 64-Bit Server VM (build 25.312-b07, mixed mode)"#;
        assert_eq!(parse_java_version(output), Some(8));
    }

    #[test]
    fn test_parse_java_17_version() {
        let output = r#"openjdk version "17.0.8" 2023-07-18
OpenJDK Runtime Environment (build 17.0.8+7-Ubuntu-1)
OpenJDK 64-Bit Server VM (build 17.0.8+7-Ubuntu-1, mixed mode, sharing)"#;
        assert_eq!(parse_java_version(output), Some(17));
    }

    #[test]
    fn test_parse_java_21_version() {
        let output = r#"openjdk version "21.0.7" 2025-04-15
OpenJDK Runtime Environment Temurin-21.0.7+6 (build 21.0.7+6)
OpenJDK 64-Bit Server VM Temurin-21.0.7+6 (build 21.0.7+6, mixed mode, sharing)"#;
        assert_eq!(parse_java_version(output), Some(21));
    }

    #[test]
    fn test_parse_java_25_version() {
        let output = r#"openjdk version "25.0.1" 2025-10-14
OpenJDK Runtime Environment (build 25.0.1+6)
OpenJDK 64-Bit Server VM (build 25.0.1+6, mixed mode, sharing)"#;
        assert_eq!(parse_java_version(output), Some(25));
    }

    #[test]
    fn test_parse_java_version_no_quotes() {
        let output = "java version something";
        assert_eq!(parse_java_version(output), None);
    }

    #[test]
    fn test_parse_java_version_empty() {
        assert_eq!(parse_java_version(""), None);
    }
}
