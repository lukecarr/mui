# MUI - Minecraft TUI Launcher

## Project Overview

MUI is a terminal user interface (TUI) launcher for Minecraft Java Edition, built in Rust.
It is similar to MultiMC/Prism Launcher but runs entirely in the terminal using ratatui.

## Architecture

```
src/
├── main.rs              # Entry point: terminal setup, tokio runtime, tracing init, app loop
├── app.rs               # Top-level App state, screen routing, event dispatch (channel-based async)
├── config.rs            # Global config: data dirs, client ID, paths
├── auth/
│   ├── mod.rs           # Re-exports
│   ├── msa.rs           # Microsoft OAuth2 authorization code flow (localhost callback)
│   ├── xbox.rs          # Xbox Live user token + XSTS token exchange
│   ├── minecraft.rs     # Minecraft token exchange, entitlements, profile fetch
│   └── store.rs         # Token persistence: save/load JSON to disk, refresh logic
├── minecraft/
│   ├── mod.rs           # Re-exports
│   ├── manifest.rs      # Fetch + parse version_manifest_v2.json
│   ├── version.rs       # Parse version metadata JSON (libraries, args, assets, rules)
│   ├── download.rs      # Download engine: assets, libraries, client JAR, with SHA-1 verify
│   ├── launch.rs        # Build JVM command line, spawn game process, capture output
│   └── rules.rs         # Evaluate OS/arch rules for library inclusion
├── instance/
│   ├── mod.rs           # Re-exports
│   ├── config.rs        # Per-instance settings (memory, Java path, window size, etc.)
│   └── manager.rs       # Create/list/delete/configure instances, directory layout
└── ui/
    ├── mod.rs           # Re-exports
    ├── theme.rs         # Color palette, styling constants
    ├── screens/
    │   ├── mod.rs       # Screen module exports
    │   ├── home.rs      # Instance list with header, log panel, and keybind footer
    │   ├── login.rs     # Auth status display, trigger login flow
    │   ├── versions.rs  # Browse MC versions, create new instance with name input
    │   ├── instance.rs  # Instance detail view: settings display
    │   └── launch.rs    # Launch progress bar, download status, game log output
    └── widgets/
        ├── mod.rs       # Widget exports
        └── log_panel.rs # Shared LogBuffer, TuiLogLayer for tracing, log panel renderer
```

**27 source files, ~3,950 lines of Rust.**

## Key Design Decisions

- **Auth flow**: Authorization Code Flow with a temporary localhost HTTP server to catch
  the redirect. Browser opens for the user to log in. No device code flow.
- **No offline mode** in MVP. Microsoft authentication is required.
- **MSA Client ID**: Uses Prism Launcher's approved client ID for development
  (`c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb`). Configurable via config file for production.
- **From scratch**: All auth, download, and launch logic is implemented directly using
  the raw Microsoft/Mojang APIs, not via third-party Minecraft crates.
- **Async**: tokio runtime with reqwest for HTTP. TUI event loop runs on the main thread
  with async operations spawned as tasks.
- **Instance management**: Each instance has its own directory with config, game files,
  and version info. Similar to Prism Launcher's instance model.
- **Channel-based async architecture**: Background tasks (auth, downloads, game process)
  communicate back to the main event loop via `mpsc::UnboundedSender<AppEvent>`. The main
  loop uses `tokio::select!` to handle both terminal input and background events without
  ever blocking the UI.

## Event-Driven Architecture (app.rs)

The app uses a non-blocking event loop. All long-running operations are spawned as tokio
tasks that send `AppEvent` messages back through an unbounded channel:

```
AppEvent variants:
  LoginSuccess(String)              # Auth completed, carries username
  LoginError(String)                # Auth failed
  ManifestLoaded(Vec<VersionEntry>) # Version list fetched
  ManifestError(String)             # Version list fetch failed
  LaunchStatus(String)              # Status message from launch pipeline
  DownloadProgress(DownloadProgress)# File download progress update
  DownloadComplete                  # All downloads finished
  DownloadError(String)             # Download failed
  GameStarted                       # Game process spawned
  GameOutput(String)                # Line from game stdout/stderr
  GameFinished(i32)                 # Game exited with code
  LaunchError(String)               # Launch pipeline failed
```

The main loop in `App::run()` uses `tokio::select!` between:
1. Terminal input events (via `tokio::task::spawn_blocking` wrapping crossterm's sync poll)
2. Background `AppEvent`s from the channel

All keyboard handlers are synchronous (`fn`, not `async fn`) and never block. They spawn
background tasks via `tokio::spawn` when async work is needed.

## TUI Logging

Tracing output is captured in real-time and displayed in the TUI via a custom system:

- **`LogBuffer`**: Thread-safe (`Arc<Mutex<Vec>>`) ring buffer of log lines, capped at 200
- **`TuiLogLayer`**: Custom `tracing_subscriber::Layer` that pushes every tracing event
  into the `LogBuffer`
- **`render_log_panel()`**: Widget that renders recent lines with level-colored tags
  (ERR=red, WRN=yellow, INF=blue, DBG=gray)
- Tracing is configured with two layers: file output (`mui.log`) + TUI display
- `AppEvent::LaunchStatus` messages are also pushed into the log buffer for visibility

The log panel appears on the home screen (below instance list) and the launch screen
(below game output).

## Home Screen Layout

```
┌─────────────────────────────────────────────────────────┐
│ MUI  |  Logged in as username                           │  <- header (3 rows)
├─────────────────────────────────────────────────────────┤
│ Instances                                               │
│ ▸ Minecraft 1.21.4                                      │
│   1.21.4  |  Last played: 2026-03-05                    │  <- instance list (Min)
├─────────────────────────────────────────────────────────┤
│  INF MUI starting up                                    │
│  INF Loaded auth for username                            │  <- log panel (12 rows)
├─────────────────────────────────────────────────────────┤
│ Enter Launch  n New  e Edit  d Delete  l Login  q Quit  │  <- footer (3 rows)
└─────────────────────────────────────────────────────────┘
```

## Launch Pipeline

The full launch pipeline runs as a single background task (`run_launch_pipeline`), sending
`AppEvent`s at each stage:

1. **Validate auth** → refresh tokens if expired (via `AuthStore::ensure_valid`)
2. **Fetch version metadata** → download the version JSON for the selected MC version
3. **Fetch asset index** → download the asset index JSON
4. **Download game files** → client JAR, libraries (with OS/arch rule filtering), assets
   - SHA-1 verification on every file
   - Progress reported via `DownloadProgress` events
   - Files already present and verified are skipped
5. **Detect Java** → check `JAVA_HOME`, `PATH`, common install locations
6. **Build launch config** → JVM args, classpath, token substitution
7. **Extract natives** → platform-specific `.so`/`.dll`/`.dylib` from library JARs
8. **Spawn game process** → stdout and stderr streamed as `GameOutput` events
9. **Wait for exit** → report `GameFinished` with exit code, clean up natives dir

## Authentication Flow

The full Microsoft -> Minecraft auth chain:

1. **Microsoft OAuth2** (Authorization Code Flow)
   - Open browser to `https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize`
   - Localhost HTTP server catches redirect with auth code
   - Exchange code for access token at `/token` endpoint
   - Scope: `XboxLive.signin offline_access`

2. **Xbox Live User Token**
   - POST `https://user.auth.xboxlive.com/user/authenticate`
   - Send MSA token with `"RpsTicket": "d=<token>"`

3. **XSTS Authorization Token**
   - POST `https://xsts.auth.xboxlive.com/xsts/authorize`
   - RelyingParty: `rp://api.minecraftservices.com/`

4. **Minecraft Access Token**
   - POST `https://api.minecraftservices.com/authentication/login_with_xbox`
   - Identity token: `XBL3.0 x=<uhs>;<xsts_token>`

5. **Entitlements Check**
   - GET `https://api.minecraftservices.com/entitlements/mcstore`

6. **Minecraft Profile**
   - GET `https://api.minecraftservices.com/minecraft/profile`

Token persistence: all tokens are saved to `~/.local/share/mui/auth.json` with expiry
timestamps. On launch, tokens are checked and refreshed if expired. The MSA refresh token
is long-lived; the MC access token expires after ~24 hours.

## API Endpoints Reference

| Purpose | URL |
|---------|-----|
| MSA Authorize | `https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize` |
| MSA Token | `https://login.microsoftonline.com/consumers/oauth2/v2.0/token` |
| Xbox Live Auth | `https://user.auth.xboxlive.com/user/authenticate` |
| XSTS Auth | `https://xsts.auth.xboxlive.com/xsts/authorize` |
| MC Login | `https://api.minecraftservices.com/authentication/login_with_xbox` |
| MC Entitlements | `https://api.minecraftservices.com/entitlements/mcstore` |
| MC Profile | `https://api.minecraftservices.com/minecraft/profile` |
| Version Manifest | `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` |
| Assets | `https://resources.download.minecraft.net/<hash[0:2]>/<hash>` |
| Libraries | `https://libraries.minecraft.net/<path>` |

## Instance Management

Each instance is stored in `~/.local/share/mui/instances/<dirname>/`:

```
<instance-dir>/
├── instance.json    # InstanceConfig: name, version, memory, java path, etc.
└── minecraft/       # The .minecraft game directory (saves, mods, etc.)
```

Shared files are stored globally to avoid duplication:
- `~/.local/share/mui/assets/` — Minecraft asset objects + indexes
- `~/.local/share/mui/libraries/` — Shared library JARs
- `~/.local/share/mui/versions/` — Version metadata + client JARs

## Dependencies

```toml
ratatui = "0.30"                          # TUI framework
crossterm = "0.29"                        # Terminal backend
tokio = { version = "1", features = ["full"] }  # Async runtime
reqwest = { version = "0.13", features = ["json", "form"] }  # HTTP client
serde = { version = "1", features = ["derive"] }  # Serialization
serde_json = "1"                          # JSON
sha1 = "0.10"                             # SHA-1 hash verification
digest = "0.10"                           # Digest trait for sha1
zip = "8"                                 # ZIP extraction for native libs
directories = "6"                         # XDG/platform directories
thiserror = "2"                           # Error derive macro
color-eyre = "0.6"                        # Error reporting
open = "5"                                # Open browser for OAuth
uuid = { version = "1", features = ["v4"] }  # UUID generation
chrono = { version = "0.4", features = ["serde"] }  # Date/time
tracing = "0.1"                           # Structured logging
tracing-subscriber = { version = "0.3", features = ["env-filter"] }  # Log output
serde_urlencoded = "0.7"                  # URL encoding for OAuth
url = "2"                                 # URL parsing
```

## Development Notes

- Reference implementation: Prism Launcher source is at `~/src/prism-launcher/`
  - Auth flow: `launcher/minecraft/auth/steps/`
  - Launch: `launcher/minecraft/launch/LauncherPartLaunch.cpp`
  - Version parsing: `launcher/minecraft/MojangVersionFormat.cpp`
- The TUI uses ratatui 0.30 with crossterm backend
- All network operations are async (tokio + reqwest)
- Errors use `thiserror` for library errors, `color-eyre` for application-level reporting
- Token data is stored as JSON in the platform data directory
- Edition 2024 is used — note that `ref` in pattern matching is handled differently
  (implicit borrowing in match arms; explicit `ref`/`ref mut` is usually not needed)
- The login flow runs in a background task and writes auth to disk; the main App reloads
  from disk on `LoginSuccess` to pick up the new tokens

## Coding Conventions

- Use `color_eyre::Result` as the default Result type in application code
- Use `thiserror` for error types in library modules (auth, minecraft, instance)
- Prefer `tracing::{info, warn, error, debug}` over `println!`/`eprintln!`
- Keep modules focused: one responsibility per file
- All public API types should derive `Debug`; data types should also derive `Clone`, `Serialize`, `Deserialize` where appropriate
- All keyboard handlers in `app.rs` must be synchronous (`fn`, not `async fn`) — spawn
  background tasks with `tokio::spawn` and send results back via `AppEvent` channel
- Never block the TUI event loop with `.await` on long operations — always spawn

## Keyboard Shortcuts

### Home Screen
| Key | Action |
|-----|--------|
| `j` / `Down` | Select next instance |
| `k` / `Up` | Select previous instance |
| `Enter` | Launch selected instance |
| `n` | New instance (opens version browser) |
| `e` | Edit/view instance details |
| `d` | Delete selected instance |
| `l` | Login with Microsoft account |
| `q` | Quit |
| `Ctrl+C` | Force quit (any screen) |

### Version Browser
| Key | Action |
|-----|--------|
| `j` / `Down` | Select next version |
| `k` / `Up` | Select previous version |
| `Enter` | Select version → enter instance name |
| `s` | Toggle snapshot versions |
| `Esc` | Back to home |

### Login Screen
| Key | Action |
|-----|--------|
| `Enter` | Start login / retry on error / go home on success |
| `Esc` | Back to home |

### Launch Screen
| Key | Action |
|-----|--------|
| `Esc` | Back to home (game continues running) |
