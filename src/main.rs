//! MUI - A TUI launcher for Minecraft Java Edition.
//!
//! Entry point: initializes the terminal, loads config, and starts the app loop.

mod app;
mod auth;
mod config;
mod instance;
mod java;
mod minecraft;
mod ui;

use color_eyre::Result;
use tracing::info;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::ui::widgets::{LogBuffer, TuiLogLayer};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize error reporting
    color_eyre::install()?;

    // Create the shared log buffer (displayed in the TUI)
    let log_buffer = LogBuffer::new();

    // Initialize tracing with two layers:
    //  1. File writer (for persistent logs)
    //  2. TuiLogLayer (for in-app display)
    let log_dir = directories::ProjectDirs::from("", "", "mui")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("mui.log");
    let file_writer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .unwrap_or_else(|_| {
            let null_path = if cfg!(target_os = "windows") {
                "NUL"
            } else {
                "/dev/null"
            };
            std::fs::File::open(null_path).expect("opening /dev/null or NUL should always succeed")
        });

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mui=info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(file_writer)
        .with_ansi(false);

    let tui_layer = TuiLogLayer::new(log_buffer.clone());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(tui_layer)
        .init();

    info!("MUI starting up");

    // Load config
    let config = config::Config::load()?;
    info!("Data directory: {:?}", config.data_dir);

    // Load auth store
    let auth_store = auth::AuthStore::load(&config.auth_store_path)?;
    if let Some(ref data) = auth_store.data {
        info!("Loaded auth for {}", data.profile.username);
    }

    // Initialize terminal
    let mut terminal = ratatui::init();

    // Run app
    let mut app = app::App::new(config, auth_store, log_buffer);
    let result = app.run(&mut terminal).await;

    // Restore terminal
    ratatui::restore();

    result
}
