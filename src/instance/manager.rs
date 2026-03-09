//! Instance management: create, list, delete, configure instances.
//!
//! Each instance has its own directory containing:
//! - `instance.json`: Instance configuration
//! - `minecraft/`: The `.minecraft` game directory

use std::path::{Path, PathBuf};

use tracing::{debug, info};

use super::config::InstanceConfig;
use super::InstanceError;

type Result<T> = std::result::Result<T, InstanceError>;

const INSTANCE_CONFIG_FILE: &str = "instance.json";

/// An instance with its config and directory path.
#[derive(Debug, Clone)]
pub struct Instance {
    /// The parsed instance configuration.
    pub config: InstanceConfig,
    /// The root directory of this instance on disk.
    pub dir: PathBuf,
}

impl Instance {
    /// The game directory (where Minecraft stores saves, mods, etc.)
    pub fn game_dir(&self) -> PathBuf {
        self.dir.join("minecraft")
    }

    /// Temporary directory for native libraries during launch.
    pub fn natives_dir(&self) -> PathBuf {
        self.dir.join("natives")
    }
}

/// Manages instances on disk.
///
/// Handles creating, listing, deleting, and saving instance configurations
/// within the instances directory.
#[derive(Debug)]
pub struct InstanceManager {
    instances_dir: PathBuf,
}

impl InstanceManager {
    /// Create a new instance manager for the given instances directory.
    pub fn new(instances_dir: &Path) -> Self {
        Self {
            instances_dir: instances_dir.to_path_buf(),
        }
    }

    /// List all instances found on disk.
    ///
    /// Instances with unparseable configs are skipped with a warning.
    /// Results are sorted by name.
    pub fn list(&self) -> Result<Vec<Instance>> {
        let mut instances = Vec::new();

        if !self.instances_dir.exists() {
            return Ok(instances);
        }

        for entry in std::fs::read_dir(&self.instances_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join(INSTANCE_CONFIG_FILE);
            if config_path.exists() {
                match std::fs::read_to_string(&config_path) {
                    Ok(contents) => match serde_json::from_str::<InstanceConfig>(&contents) {
                        Ok(config) => {
                            instances.push(Instance { config, dir: path });
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse instance config at {:?}: {}",
                                config_path,
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read instance config at {:?}: {}",
                            config_path,
                            e
                        );
                    }
                }
            }
        }

        // Sort by name
        instances.sort_by(|a, b| a.config.name.cmp(&b.config.name));
        Ok(instances)
    }

    /// Create a new instance with the given name and Minecraft version.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceError::InvalidName`] if the name is empty or would
    /// cause path traversal, or [`InstanceError::AlreadyExists`] if the
    /// sanitized directory name already exists on disk.
    pub fn create(&self, name: &str, version_id: &str, version_url: &str) -> Result<Instance> {
        // Generate a directory name from the instance name
        let dir_name = sanitize_dirname(name);

        // Reject names that are empty or resolve to special directory entries
        if dir_name.is_empty() || dir_name == "." || dir_name == ".." {
            return Err(InstanceError::InvalidName(name.to_string()));
        }

        let instance_dir = self.instances_dir.join(&dir_name);

        // Defense-in-depth: verify the resolved path stays within instances_dir
        if !instance_dir.starts_with(&self.instances_dir) {
            return Err(InstanceError::InvalidName(name.to_string()));
        }

        if instance_dir.exists() {
            return Err(InstanceError::AlreadyExists(dir_name));
        }

        info!("Creating instance '{}' at {:?}", name, instance_dir);

        // Create directories
        std::fs::create_dir_all(&instance_dir)?;
        std::fs::create_dir_all(instance_dir.join("minecraft"))?;

        // Write config
        let config = InstanceConfig::new(
            name.to_string(),
            version_id.to_string(),
            version_url.to_string(),
        );
        let config_json = serde_json::to_string_pretty(&config)?;
        std::fs::write(instance_dir.join(INSTANCE_CONFIG_FILE), config_json)?;

        debug!("Instance created successfully");

        Ok(Instance {
            config,
            dir: instance_dir,
        })
    }

    /// Save an instance's updated config to disk.
    pub fn save_config(&self, instance: &Instance) -> Result<()> {
        let config_json = serde_json::to_string_pretty(&instance.config)?;
        std::fs::write(instance.dir.join(INSTANCE_CONFIG_FILE), config_json)?;
        Ok(())
    }

    /// Delete an instance and all its files from disk.
    pub fn delete(&self, instance: &Instance) -> Result<()> {
        info!("Deleting instance '{}'", instance.config.name);
        std::fs::remove_dir_all(&instance.dir)?;
        Ok(())
    }
}

/// Convert an instance name to a safe directory name.
///
/// Replaces any character that isn't alphanumeric, `-`, `_`, or `.` with `_`,
/// then strips leading dots to prevent `.` and `..` traversal.
fn sanitize_dirname(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Strip leading dots to prevent "." and ".." directory traversal
    sanitized.trim_start_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_dirname_normal_name() {
        assert_eq!(sanitize_dirname("Minecraft 1.21.4"), "Minecraft_1.21.4");
    }

    #[test]
    fn sanitize_dirname_strips_path_separators() {
        // "../../etc/passwd" -> ".._.._ etc_passwd" (slashes/spaces become _)
        // -> leading dots stripped -> "_.._etc_passwd"
        assert_eq!(sanitize_dirname("../../etc/passwd"), "_.._etc_passwd");
    }

    #[test]
    fn sanitize_dirname_dot_dot() {
        // ".." must not survive sanitization — leading dots are stripped
        assert_eq!(sanitize_dirname(".."), "");
    }

    #[test]
    fn sanitize_dirname_single_dot() {
        assert_eq!(sanitize_dirname("."), "");
    }

    #[test]
    fn sanitize_dirname_leading_dots_stripped() {
        assert_eq!(sanitize_dirname("...hidden"), "hidden");
        assert_eq!(sanitize_dirname("..name"), "name");
    }

    #[test]
    fn sanitize_dirname_preserves_internal_dots() {
        assert_eq!(sanitize_dirname("my.instance.1.21"), "my.instance.1.21");
    }

    #[test]
    fn sanitize_dirname_empty_input() {
        assert_eq!(sanitize_dirname(""), "");
    }

    #[test]
    fn sanitize_dirname_only_special_chars() {
        // All replaced with `_`, no leading dots
        assert_eq!(sanitize_dirname("@#$ %^&"), "_______");
    }
}
