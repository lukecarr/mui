//! Build JVM command line and spawn the Minecraft game process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use color_eyre::Result;
use tokio::process::Command;
use tracing::{debug, info};

use super::download;
use super::rules;
use super::version::{ArgumentValue, ArgumentValueInner, VersionMeta};

/// Configuration for launching Minecraft.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// Path to the java executable
    pub java_path: String,
    /// Game directory (the instance's .minecraft dir)
    pub game_dir: PathBuf,
    /// Global assets directory
    pub assets_dir: PathBuf,
    /// Global libraries directory
    pub libraries_dir: PathBuf,
    /// Versions directory (where client JARs live)
    pub versions_dir: PathBuf,
    /// Native libraries extraction directory (temp)
    pub natives_dir: PathBuf,
    /// Minimum heap memory in MB
    pub min_memory: u32,
    /// Maximum heap memory in MB
    pub max_memory: u32,
    /// Window width
    pub window_width: u32,
    /// Window height
    pub window_height: u32,
    /// Player username
    pub username: String,
    /// Player UUID (from Minecraft profile)
    pub uuid: String,
    /// Minecraft access token
    pub access_token: String,
}

/// Build the complete JVM command and spawn the game process.
pub async fn launch(
    meta: &VersionMeta,
    config: &LaunchConfig,
) -> Result<tokio::process::Child> {
    // Extract natives
    let native_jars = download::collect_native_jars(meta, &config.libraries_dir);
    download::extract_natives(&native_jars, &config.natives_dir)?;

    // Build classpath
    let classpath_entries =
        download::collect_classpath(meta, &config.libraries_dir, &config.versions_dir);
    let classpath = classpath_entries
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator());

    // Build token replacement map
    let tokens = build_token_map(meta, config, &classpath);

    // Build JVM arguments
    let mut args: Vec<String> = Vec::new();

    // Memory settings
    args.push(format!("-Xms{}m", config.min_memory));
    args.push(format!("-Xmx{}m", config.max_memory));

    // Native library path
    args.push(format!(
        "-Djava.library.path={}",
        config.natives_dir.to_string_lossy()
    ));

    // Launcher branding
    args.push("-Dminecraft.launcher.brand=mui".to_string());
    args.push("-Dminecraft.launcher.version=0.1.0".to_string());

    // JVM args from version metadata (1.13+)
    if let Some(ref arguments) = meta.arguments {
        for arg in &arguments.jvm {
            match arg {
                ArgumentValue::Simple(s) => {
                    args.push(replace_tokens(s, &tokens));
                }
                ArgumentValue::Conditional { rules, value } => {
                    if rules::rules_match(rules) {
                        match value {
                            ArgumentValueInner::Single(s) => {
                                args.push(replace_tokens(s, &tokens));
                            }
                            ArgumentValueInner::Multiple(list) => {
                                for s in list {
                                    args.push(replace_tokens(s, &tokens));
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Pre-1.13: add classpath manually since there's no JVM args in metadata
        args.push("-cp".to_string());
        args.push(classpath.clone());
    }

    // Main class
    args.push(meta.main_class.clone());

    // Game arguments
    if let Some(ref arguments) = meta.arguments {
        for arg in &arguments.game {
            match arg {
                ArgumentValue::Simple(s) => {
                    args.push(replace_tokens(s, &tokens));
                }
                ArgumentValue::Conditional { rules, value } => {
                    if rules::rules_match(rules) {
                        match value {
                            ArgumentValueInner::Single(s) => {
                                args.push(replace_tokens(s, &tokens));
                            }
                            ArgumentValueInner::Multiple(list) => {
                                for s in list {
                                    args.push(replace_tokens(s, &tokens));
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if let Some(ref mc_args) = meta.minecraft_arguments {
        // Pre-1.13 legacy arguments format
        for arg in mc_args.split_whitespace() {
            args.push(replace_tokens(arg, &tokens));
        }
    }

    info!("Launching Minecraft {}...", meta.id);
    debug!("Java: {}", config.java_path);
    debug!("Args: {:?}", args);

    // Ensure game directory exists
    tokio::fs::create_dir_all(&config.game_dir).await?;

    let child = Command::new(&config.java_path)
        .args(&args)
        .current_dir(&config.game_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    info!("Game process started (PID: {:?})", child.id());
    Ok(child)
}

/// Build the token replacement map for arguments.
fn build_token_map(
    meta: &VersionMeta,
    config: &LaunchConfig,
    classpath: &str,
) -> HashMap<String, String> {
    let mut tokens = HashMap::new();

    // Auth tokens
    tokens.insert("auth_player_name".to_string(), config.username.clone());
    tokens.insert("auth_uuid".to_string(), config.uuid.clone());
    tokens.insert("auth_access_token".to_string(), config.access_token.clone());
    tokens.insert("auth_session".to_string(), format!("token:{}:{}", config.access_token, config.uuid));
    tokens.insert("auth_xuid".to_string(), String::new()); // Not tracking XUID
    tokens.insert("clientid".to_string(), String::new());
    tokens.insert("user_type".to_string(), "msa".to_string());
    tokens.insert("user_properties".to_string(), "{}".to_string());

    // Version info
    tokens.insert("version_name".to_string(), meta.id.clone());
    tokens.insert("version_type".to_string(), meta.version_type.clone());

    // Paths
    tokens.insert("game_directory".to_string(), config.game_dir.to_string_lossy().to_string());
    tokens.insert("assets_root".to_string(), config.assets_dir.to_string_lossy().to_string());
    tokens.insert("game_assets".to_string(), config.assets_dir.to_string_lossy().to_string());
    tokens.insert("assets_index_name".to_string(), meta.asset_index.id.clone());
    tokens.insert("natives_directory".to_string(), config.natives_dir.to_string_lossy().to_string());
    tokens.insert("library_directory".to_string(), config.libraries_dir.to_string_lossy().to_string());
    tokens.insert("classpath_separator".to_string(), classpath_separator().to_string());
    tokens.insert("classpath".to_string(), classpath.to_string());

    // Window
    tokens.insert("resolution_width".to_string(), config.window_width.to_string());
    tokens.insert("resolution_height".to_string(), config.window_height.to_string());

    // Misc
    tokens.insert("profile_name".to_string(), "MUI".to_string());
    tokens.insert("launcher_name".to_string(), "mui".to_string());
    tokens.insert("launcher_version".to_string(), "0.1.0".to_string());

    tokens
}

/// Replace `${token}` placeholders in a string.
fn replace_tokens(template: &str, tokens: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in tokens {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

fn classpath_separator() -> &'static str {
    if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    }
}

/// Detect the system Java installation.
pub fn detect_java() -> Option<String> {
    // Try JAVA_HOME first
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_path = Path::new(&java_home).join("bin").join("java");
        if java_path.exists() {
            return Some(java_path.to_string_lossy().to_string());
        }
    }

    // Try PATH
    if let Ok(output) = std::process::Command::new("which").arg("java").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }

    // Windows: try common locations
    if cfg!(target_os = "windows") {
        let candidates = [
            "C:\\Program Files\\Java\\jdk-21\\bin\\java.exe",
            "C:\\Program Files\\Java\\jdk-17\\bin\\java.exe",
            "C:\\Program Files\\Eclipse Adoptium\\jdk-21\\bin\\java.exe",
            "C:\\Program Files\\Eclipse Adoptium\\jdk-17\\bin\\java.exe",
        ];
        for path in &candidates {
            if Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
    }

    None
}
