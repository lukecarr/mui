//! Top-level application state, screen routing, and event dispatch.
//!
//! Uses a channel-based architecture: background tasks (downloads, auth, game process)
//! send `AppEvent`s back to the main loop, which never blocks on async work.

use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;
use tracing::info;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::instance::manager::{Instance, InstanceManager};
use crate::minecraft::{download, launch, manifest, version};
use crate::ui::screens::{
    home::HomeScreen,
    instance::InstanceScreen,
    launch::{LaunchScreen, LaunchState},
    login::{LoginScreen, LoginState},
    versions::VersionsScreen,
};
use crate::ui::widgets::log_panel::{self, LogBuffer};

/// Events sent from background tasks back to the main event loop.
enum AppEvent {
    // ── Login events ──
    LoginSuccess(String),
    LoginError(String),

    // ── Version manifest ──
    ManifestLoaded(Vec<manifest::VersionEntry>),
    ManifestError(String),

    // ── Launch pipeline events ──
    LaunchStatus(String),
    DownloadProgress(download::DownloadProgress),
    DownloadComplete,
    DownloadError(String),
    GameStarted,
    GameOutput(String),
    GameFinished(i32),
    LaunchError(String),
}

/// Which screen is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Login,
    Versions,
    Instance,
    Launch,
}

/// Top-level application state.
pub struct App {
    config: Config,
    http: reqwest::Client,
    auth_store: AuthStore,
    instance_manager: InstanceManager,

    screen: Screen,
    should_quit: bool,

    // Screen states
    home: HomeScreen,
    login: LoginScreen,
    versions: VersionsScreen,
    instance_screen: InstanceScreen,
    launch_screen: Option<LaunchScreen>,

    // Channel for receiving events from background tasks
    event_tx: mpsc::UnboundedSender<AppEvent>,
    event_rx: mpsc::UnboundedReceiver<AppEvent>,

    // Track the currently-launching instance so we can update last_played etc.
    launching_instance: Option<Instance>,

    // Shared log buffer displayed in the TUI
    log_buffer: LogBuffer,
}

impl App {
    pub fn new(config: Config, auth_store: AuthStore, log_buffer: LogBuffer) -> Self {
        let instance_manager = InstanceManager::new(&config.instances_dir);

        let mut home = HomeScreen::new();
        if let Ok(instances) = instance_manager.list() {
            home.instances = instances;
            if !home.instances.is_empty() {
                home.list_state.select(Some(0));
            }
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            http: reqwest::Client::new(),
            auth_store,
            instance_manager,
            config,
            screen: Screen::Home,
            should_quit: false,
            home,
            login: LoginScreen::new(),
            versions: VersionsScreen::new(),
            instance_screen: InstanceScreen::new(),
            launch_screen: None,
            event_tx,
            event_rx,
            launching_instance: None,
            log_buffer,
        }
    }

    /// Main event loop. Alternates between checking terminal input and
    /// draining the background-event channel. Never blocks on async work.
    pub async fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        loop {
            // 1. Draw the current state
            terminal.draw(|frame| self.draw(frame))?;

            // 2. Wait for EITHER a terminal event OR a background event,
            //    with a short timeout so we keep redrawing.
            tokio::select! {
                // Terminal input (crossterm is sync, so poll in a blocking task)
                maybe_event = tokio::task::spawn_blocking(|| {
                    if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                        event::read().ok()
                    } else {
                        None
                    }
                }) => {
                    if let Ok(Some(Event::Key(key))) = maybe_event {
                        self.handle_key(key)?;
                    }
                }
                // Background task event
                Some(app_event) = self.event_rx.recv() => {
                    self.handle_app_event(app_event);
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        match self.screen {
            Screen::Home => {
                let username = self
                    .auth_store
                    .data
                    .as_ref()
                    .map(|d| d.profile.username.as_str());
                self.home.render(frame, area, username, &self.log_buffer);
            }
            Screen::Login => {
                self.login.render(frame, area);
            }
            Screen::Versions => {
                self.versions.render(frame, area);
            }
            Screen::Instance => {
                self.instance_screen.render(frame, area);
            }
            Screen::Launch => {
                if let Some(ref launch) = self.launch_screen {
                    // Split: launch UI on top, log panel on bottom
                    let chunks = ratatui::layout::Layout::default()
                        .direction(ratatui::layout::Direction::Vertical)
                        .constraints([
                            ratatui::layout::Constraint::Min(10),    // Launch content
                            ratatui::layout::Constraint::Length(12), // Log panel
                        ])
                        .split(area);

                    launch.render(frame, chunks[0]);
                    log_panel::render_log_panel(&self.log_buffer, frame, chunks[1], "Log");
                }
            }
        }
    }

    // ── Handle background events ─────────────────────────────────────

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            // Login
            AppEvent::LoginSuccess(username) => {
                self.login.state = LoginState::Success(username);
                // Reload auth store from disk so we have the new tokens
                if let Ok(store) = AuthStore::load(&self.config.auth_store_path) {
                    self.auth_store = store;
                }
            }
            AppEvent::LoginError(err) => {
                self.log_buffer.push_info(format!("Login failed: {err}"));
                self.login.state = LoginState::Error(err);
            }

            // Version manifest
            AppEvent::ManifestLoaded(versions) => {
                self.versions.versions = versions;
                self.versions.loading = false;
                if !self.versions.filtered_versions().is_empty() {
                    self.versions.list_state.select(Some(0));
                }
            }
            AppEvent::ManifestError(err) => {
                self.versions.loading = false;
                self.screen = Screen::Home;
                // Could show a popup, for now just log
                tracing::error!("Failed to fetch versions: {err}");
            }

            // Launch pipeline
            AppEvent::LaunchStatus(msg) => {
                self.log_buffer.push_info(msg);
            }
            AppEvent::DownloadProgress(progress) => {
                if let Some(ref mut ls) = self.launch_screen {
                    ls.progress = Some(progress);
                }
            }
            AppEvent::DownloadComplete => {
                self.log_buffer
                    .push_info("Downloads complete, starting game...".to_string());
                if let Some(ref mut ls) = self.launch_screen {
                    ls.progress = None;
                    ls.state = LaunchState::Starting;
                }
            }
            AppEvent::DownloadError(err) => {
                self.log_buffer.push_info(format!("Download failed: {err}"));
                if let Some(ref mut ls) = self.launch_screen {
                    ls.state = LaunchState::Error(format!("Download failed: {err}"));
                    ls.progress = None;
                }
            }
            AppEvent::GameStarted => {
                if let Some(ref mut ls) = self.launch_screen {
                    ls.state = LaunchState::Running;
                }
                // Update last_played
                if let Some(ref mut inst) = self.launching_instance {
                    inst.config.last_played = Some(chrono::Utc::now().to_rfc3339());
                    let _ = self.instance_manager.save_config(inst);
                    self.refresh_instances();
                }
            }
            AppEvent::GameOutput(line) => {
                if let Some(ref mut ls) = self.launch_screen {
                    ls.add_log_line(line);
                }
            }
            AppEvent::GameFinished(code) => {
                if let Some(ref mut ls) = self.launch_screen {
                    ls.state = LaunchState::Finished(code);
                    ls.add_log_line(format!("Game exited with code {code}"));
                }
                // Clean up natives dir
                if let Some(ref inst) = self.launching_instance {
                    let _ = std::fs::remove_dir_all(inst.natives_dir());
                }
                self.launching_instance = None;
            }
            AppEvent::LaunchError(err) => {
                self.log_buffer.push_info(format!("Launch error: {err}"));
                if let Some(ref mut ls) = self.launch_screen {
                    ls.state = LaunchState::Error(err);
                }
                self.launching_instance = None;
            }
        }
    }

    // ── Handle keyboard input (synchronous, never blocks) ────────────

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Global quit on Ctrl+C
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        match self.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::Login => self.handle_login_key(key),
            Screen::Versions => self.handle_versions_key(key),
            Screen::Instance => self.handle_instance_key(key),
            Screen::Launch => self.handle_launch_key(key),
        }
    }

    // ── Home screen ──────────────────────────────────────────────────

    fn handle_home_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.home.select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.home.select_next();
            }
            KeyCode::Char('n') => {
                // New instance -> go to version browser, fetch manifest in background
                self.versions = VersionsScreen::new();
                self.versions.loading = true;
                self.screen = Screen::Versions;

                let http = self.http.clone();
                let tx = self.event_tx.clone();
                tokio::spawn(async move {
                    match manifest::fetch_manifest(&http).await {
                        Ok(m) => {
                            let _ = tx.send(AppEvent::ManifestLoaded(m.versions));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::ManifestError(format!("{e}")));
                        }
                    }
                });
            }
            KeyCode::Char('l') => {
                self.login = LoginScreen::new();
                self.screen = Screen::Login;
            }
            KeyCode::Char('d') => {
                if let Some(inst) = self.home.selected_instance() {
                    let inst_clone = inst.clone();
                    let _ = self.instance_manager.delete(&inst_clone);
                    self.refresh_instances();
                }
            }
            KeyCode::Enter => {
                if let Some(inst) = self.home.selected_instance() {
                    let inst_clone = inst.clone();
                    self.start_launch(inst_clone);
                }
            }
            KeyCode::Char('e') => {
                if let Some(inst) = self.home.selected_instance() {
                    self.instance_screen.instance = Some(inst.clone());
                    self.screen = Screen::Instance;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Login screen ─────────────────────────────────────────────────

    fn handle_login_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Home;
            }
            KeyCode::Enter => {
                match &self.login.state {
                    LoginState::Prompt | LoginState::Error(_) => {
                        self.login.state = LoginState::WaitingForBrowser;

                        // Spawn login in background
                        let client_id = self.config.msa_client_id.clone();
                        let http = self.http.clone();
                        let auth_path = self.config.auth_store_path.clone();
                        let tx = self.event_tx.clone();
                        tokio::spawn(async move {
                            let mut store = match AuthStore::load(&auth_path) {
                                Ok(s) => s,
                                Err(e) => {
                                    let _ = tx.send(AppEvent::LoginError(format!("{e}")));
                                    return;
                                }
                            };
                            match store.login(&client_id, &http).await {
                                Ok(()) => {
                                    let username = store
                                        .data
                                        .as_ref()
                                        .map(|d| d.profile.username.clone())
                                        .unwrap_or_default();
                                    let _ = tx.send(AppEvent::LoginSuccess(username));
                                }
                                Err(e) => {
                                    let _ = tx.send(AppEvent::LoginError(format!("{e}")));
                                }
                            }
                        });
                    }
                    LoginState::Success(_) => {
                        // Reload auth store from disk so main App has the tokens
                        if let Ok(store) = AuthStore::load(&self.config.auth_store_path) {
                            self.auth_store = store;
                        }
                        self.screen = Screen::Home;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Version browser ──────────────────────────────────────────────

    fn handle_versions_key(&mut self, key: KeyEvent) -> Result<()> {
        // If we're in name input mode
        if let Some(ref name) = self.versions.input_name.clone() {
            match key.code {
                KeyCode::Esc => {
                    self.versions.input_name = None;
                }
                KeyCode::Enter => {
                    if !name.is_empty()
                        && let Some(ver) = self.versions.selected_version()
                    {
                        let ver_id = ver.id.clone();
                        let ver_url = ver.url.clone();
                        match self.instance_manager.create(name, &ver_id, &ver_url) {
                            Ok(_) => {
                                info!("Created instance '{name}' with version {ver_id}");
                                self.refresh_instances();
                                self.screen = Screen::Home;
                            }
                            Err(e) => {
                                tracing::error!("Failed to create instance: {e}");
                                self.versions.input_name = None;
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    let mut n = name.clone();
                    n.pop();
                    self.versions.input_name = Some(n);
                }
                KeyCode::Char(c) => {
                    let mut n = name.clone();
                    n.push(c);
                    self.versions.input_name = Some(n);
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Home;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.versions.select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.versions.select_next();
            }
            KeyCode::Char('s') => {
                self.versions.show_snapshots = !self.versions.show_snapshots;
                self.versions.list_state.select(Some(0));
            }
            KeyCode::Enter => {
                if self.versions.selected_version().is_some() {
                    let ver = self.versions.selected_version().unwrap();
                    self.versions.input_name = Some(format!("Minecraft {}", ver.id));
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Instance detail screen ───────────────────────────────────────

    fn handle_instance_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Home;
            }
            KeyCode::Enter => {
                if let Some(inst) = self.instance_screen.instance.clone() {
                    self.start_launch(inst);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Launch screen ────────────────────────────────────────────────

    fn handle_launch_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Esc {
            self.screen = Screen::Home;
            self.launch_screen = None;
        }
        Ok(())
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn refresh_instances(&mut self) {
        if let Ok(instances) = self.instance_manager.list() {
            let had_selection = self.home.list_state.selected();
            self.home.instances = instances;
            if self.home.instances.is_empty() {
                self.home.list_state.select(None);
            } else if let Some(prev) = had_selection {
                let new_idx = prev.min(self.home.instances.len().saturating_sub(1));
                self.home.list_state.select(Some(new_idx));
            }
        }
    }

    /// Kick off the entire launch pipeline as a background task.
    /// The pipeline: validate auth -> fetch metadata -> download -> extract natives -> launch -> stream output.
    fn start_launch(&mut self, instance: Instance) {
        // Check if we have auth data at all
        let auth_data = match &self.auth_store.data {
            Some(d) => d.clone(),
            None => {
                self.login = LoginScreen::new();
                self.screen = Screen::Login;
                return;
            }
        };

        // Set up the launch screen immediately
        let launch_screen = LaunchScreen::new(instance.config.name.clone());
        self.launch_screen = Some(launch_screen);
        self.launching_instance = Some(instance.clone());
        self.screen = Screen::Launch;

        // Clone everything we need for the background task
        let tx = self.event_tx.clone();
        let http = self.http.clone();
        let config = self.config.clone();
        let client_id = self.config.msa_client_id.clone();

        tokio::spawn(async move {
            if let Err(e) =
                run_launch_pipeline(tx.clone(), http, config, client_id, auth_data, instance).await
            {
                let _ = tx.send(AppEvent::LaunchError(format!("{e}")));
            }
        });
    }
}

/// The full launch pipeline, running on a background task.
/// Sends `AppEvent`s back to the main loop at each stage.
async fn run_launch_pipeline(
    tx: mpsc::UnboundedSender<AppEvent>,
    http: reqwest::Client,
    config: Config,
    client_id: String,
    auth_data: crate::auth::store::AuthData,
    instance: Instance,
) -> Result<()> {
    // 1. Ensure auth is valid (refresh if needed)
    let _ = tx.send(AppEvent::LaunchStatus(
        "Validating authentication...".into(),
    ));
    let mc_token;
    if auth_data.mc_token_valid() {
        mc_token = auth_data.mc_access_token.clone();
    } else {
        let _ = tx.send(AppEvent::LaunchStatus("Refreshing tokens...".into()));
        let mut store = AuthStore::load(&config.auth_store_path)?;
        store.ensure_valid(&client_id, &http).await?;
        mc_token = store
            .data
            .as_ref()
            .map(|d| d.mc_access_token.clone())
            .ok_or_else(|| color_eyre::eyre::eyre!("Auth failed after refresh"))?;
    }

    // 2. Fetch version metadata
    let _ = tx.send(AppEvent::LaunchStatus(format!(
        "Fetching metadata for {}...",
        instance.config.version_id
    )));
    let meta = version::fetch_version_meta(&instance.config.version_url, &http).await?;

    // 3. Fetch asset index
    let _ = tx.send(AppEvent::LaunchStatus("Fetching asset index...".into()));
    let asset_index = version::fetch_asset_index(&meta.asset_index.url, &http).await?;

    // 4. Download all files
    let _ = tx.send(AppEvent::LaunchStatus("Downloading game files...".into()));

    let (progress_tx, mut progress_rx) = mpsc::channel(64);

    let dl_http = http.clone();
    let dl_meta = meta.clone();
    let dl_asset_index = asset_index.clone();
    let libraries_dir = config.libraries_dir.clone();
    let assets_dir = config.assets_dir.clone();
    let versions_dir = config.versions_dir.clone();

    let download_handle = tokio::spawn(async move {
        download::download_version(
            &dl_meta,
            &dl_asset_index,
            &libraries_dir,
            &assets_dir,
            &versions_dir,
            &dl_http,
            Some(progress_tx),
        )
        .await
    });

    // Forward progress events
    let progress_tx_clone = tx.clone();
    let progress_forwarder = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            let _ = progress_tx_clone.send(AppEvent::DownloadProgress(progress));
        }
    });

    // Wait for download to finish
    let download_result = download_handle.await?;
    progress_forwarder.abort(); // stop forwarder

    if let Err(e) = download_result {
        let _ = tx.send(AppEvent::DownloadError(format!("{e}")));
        return Ok(());
    }
    let _ = tx.send(AppEvent::DownloadComplete);

    // 5. Detect Java
    let java_path = instance
        .config
        .java_path
        .clone()
        .or_else(launch::detect_java)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "Java not found. Install Java or set java_path in instance config."
            )
        })?;

    // 6. Build launch config & launch
    let launch_config = launch::LaunchConfig {
        java_path,
        game_dir: instance.game_dir(),
        assets_dir: config.assets_dir.clone(),
        libraries_dir: config.libraries_dir.clone(),
        versions_dir: config.versions_dir.clone(),
        natives_dir: instance.natives_dir(),
        min_memory: instance.config.min_memory_mb,
        max_memory: instance.config.max_memory_mb,
        window_width: instance.config.window_width,
        window_height: instance.config.window_height,
        username: auth_data.profile.username.clone(),
        uuid: auth_data.profile.uuid.clone(),
        access_token: mc_token,
    };

    let _ = tx.send(AppEvent::LaunchStatus("Starting Minecraft...".into()));

    let mut child = launch::launch(&meta, &launch_config).await?;
    let _ = tx.send(AppEvent::GameStarted);

    // 7. Stream stdout and stderr to the launch screen
    let stdout_tx = tx.clone();
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = stdout_tx.send(AppEvent::GameOutput(line));
            }
        });
    }

    let stderr_tx = tx.clone();
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = stderr_tx.send(AppEvent::GameOutput(line));
            }
        });
    }

    // 8. Wait for the game process to exit
    let status = child.wait().await?;
    let code = status.code().unwrap_or(-1);
    let _ = tx.send(AppEvent::GameFinished(code));

    Ok(())
}
