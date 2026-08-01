//! Tauri GUI shell.
//!
//! Deliberately thin: every command here delegates to `romm_desktop`, the same
//! crate the CLI and TUI use. If logic starts accumulating in this file it
//! belongs in the core crate instead.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Emitter, State};

use romm_desktop::{
    api, cache, config::Config, coremap::CoreMap, download, media, retroarch::RetroArch,
    theme,
};

const CACHE_DB: &str = "cache.sqlite3";
const CORE_MAP: &str = "data/esde-core-map.json";

/// Long-lived process state. The SQLite connection is not `Sync`, so it lives
/// behind a mutex and is only held for the duration of a query.
struct AppState {
    cache: Mutex<cache::Cache>,
    map: CoreMap,
    client: Option<Arc<api::Client>>,
    retroarch: Option<RetroArch>,
    roms_dir: PathBuf,
    media_dir: PathBuf,
    theme_root: Option<String>,
}

#[derive(Serialize)]
struct PlatformView {
    slug: String,
    name: String,
    rom_count: i64,
    /// Whether a libretro core for this platform is actually installed.
    playable: bool,
    /// ES-DE theme logo, if one has been installed locally.
    logo: Option<String>,
}

#[derive(Serialize)]
struct RomView {
    id: i64,
    name: String,
    fs_name: String,
    platform: String,
    size_bytes: i64,
    downloaded: bool,
}

#[derive(Serialize)]
struct RomDetail {
    id: i64,
    name: String,
    fs_name: String,
    platform: String,
    size_bytes: i64,
    downloaded: bool,
    core: Option<String>,
    core_label: Option<String>,
    /// Local media, as `asset:` URLs the webview can load directly.
    cover: Option<String>,
    video: Option<String>,
    screenshot: Option<String>,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
fn platforms(state: State<'_, AppState>) -> CmdResult<Vec<PlatformView>> {
    let cache = state.cache.lock().map_err(err)?;
    let rows = cache.platforms().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|p| PlatformView {
            playable: resolve_core(&state, &p.fs_slug).is_some(),
            logo: theme::installed_logo(&state.media_dir, &p.fs_slug)
                .map(|p| p.display().to_string()),
            slug: p.fs_slug,
            name: p.display_name,
            rom_count: p.rom_count,
        })
        .collect())
}

#[tauri::command]
fn roms(state: State<'_, AppState>, platform: String) -> CmdResult<Vec<RomView>> {
    let cache = state.cache.lock().map_err(err)?;
    let rows = cache.roms_for(&platform).map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|r| RomView {
            downloaded: local_path(&state, &r.platform_slug, &r.fs_name).is_some(),
            id: r.id,
            name: r.name,
            fs_name: r.fs_name,
            platform: r.platform_slug,
            size_bytes: r.fs_size_bytes,
        })
        .collect())
}

#[tauri::command]
fn search(state: State<'_, AppState>, term: String) -> CmdResult<Vec<RomView>> {
    let cache = state.cache.lock().map_err(err)?;
    let rows = cache.search(&term, 200).map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|r| RomView {
            downloaded: local_path(&state, &r.platform_slug, &r.fs_name).is_some(),
            id: r.id,
            name: r.name,
            fs_name: r.fs_name,
            platform: r.platform_slug,
            size_bytes: r.fs_size_bytes,
        })
        .collect())
}

#[tauri::command]
async fn rom_detail(state: State<'_, AppState>, id: i64) -> CmdResult<RomDetail> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let core = resolve_core(&state, &row.platform_slug);
    let core_label = core
        .as_deref()
        .and_then(|c| state.map.label_for(c))
        .map(str::to_owned);

    // ES-DE files media under <media>/<platform>/<type>/<rom basename>.<ext>.
    let stem = Path::new(&row.fs_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| row.fs_name.clone());

    // Local ES-DE media covers only ~2% of this library, so fall back to the
    // server's artwork and cache it into the same tree.
    let client = state.client.clone();
    let media_root = state.media_dir.clone();
    let as_url = |p: Option<std::path::PathBuf>| p.map(|p| p.display().to_string());

    let cover = media::ensure(
        client.as_deref(), &media_root, &row.platform_slug, &stem,
        media::COVERS, row.cover_path.as_deref(),
    ).await;
    let screenshot = media::ensure(
        client.as_deref(), &media_root, &row.platform_slug, &stem,
        media::SCREENSHOTS, row.screenshot_path.as_deref(),
    ).await;
    // No server-side video exists on this deployment; local only.
    let video = media::find_local(&media_root, &row.platform_slug, &stem, media::VIDEOS);

    Ok(RomDetail {
        cover: as_url(cover),
        video: as_url(video),
        screenshot: as_url(screenshot),
        downloaded: local_path(&state, &row.platform_slug, &row.fs_name).is_some(),
        id: row.id,
        name: row.name,
        fs_name: row.fs_name,
        platform: row.platform_slug,
        size_bytes: row.fs_size_bytes,
        core,
        core_label,
    })
}

/// Download a ROM, emitting `download-progress` events as it goes.
#[tauri::command]
async fn download_rom(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<String> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let client = state
        .client
        .clone()
        .ok_or("no server connection — check config.toml")?;
    let roms_dir = state.roms_dir.clone();

    let target = download::Target {
        rom_id: row.id,
        fs_name: &row.fs_name,
        platform_slug: &row.platform_slug,
        expected_size: (row.fs_size_bytes > 0).then_some(row.fs_size_bytes as u64),
        md5: row.md5_hash.as_deref(),
        sha1: row.sha1_hash.as_deref(),
    };

    let mut last = std::time::Instant::now();
    let outcome = download::fetch(
        client.http(),
        client.base(),
        client.auth(),
        &target,
        &roms_dir,
        |done, total| {
            // Throttle: the webview does not need 60 events a second.
            if last.elapsed().as_millis() < 100 {
                return;
            }
            last = std::time::Instant::now();
            let _ = app.emit("download-progress", (id, done, total));
        },
    )
    .await
    .map_err(err)?;

    let _ = app.emit("download-progress", (id, 1u64, 1u64));
    Ok(match outcome {
        download::Outcome::AlreadyHave(p) => format!("already had {}", p.display()),
        download::Outcome::Downloaded { path, .. } => format!("downloaded {}", path.display()),
    })
}

/// Launch a ROM in RetroArch. Blocks until the emulator exits.
#[tauri::command]
async fn launch_rom(state: State<'_, AppState>, id: i64) -> CmdResult<String> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let ra = state.retroarch.as_ref().ok_or("RetroArch not found")?;
    let path = local_path(&state, &row.platform_slug, &row.fs_name)
        .ok_or("not downloaded yet")?;
    let core = resolve_core(&state, &row.platform_slug)
        .ok_or_else(|| format!("no installed core for {}", row.platform_slug))?;

    let status = ra.launch(&core, &path, false).map_err(err)?;
    Ok(if status.success() {
        format!("{} exited cleanly", row.name)
    } else {
        format!("{} exited with {status}", row.name)
    })
}

/// Copy system logos out of an installed ES-DE theme into the media tree.
#[tauri::command]
fn install_theme_logos(state: State<'_, AppState>) -> CmdResult<String> {
    let slugs: Vec<String> = {
        let cache = state.cache.lock().map_err(err)?;
        cache.platforms().map_err(err)?.into_iter().map(|p| p.fs_slug).collect()
    };
    let themes = theme::discover(state.theme_root.as_deref());
    if themes.is_empty() {
        return Err("no ES-DE themes found — install ES-DE or set [theme] root".into());
    }
    let n = theme::install(&themes, &state.map, &slugs, &state.media_dir).map_err(err)?;
    Ok(format!("installed {n} logos from {}", themes[0].name))
}

#[derive(Serialize)]
struct Status {
    server: String,
    connected: bool,
    retroarch: Option<String>,
    cores_installed: usize,
    roms_cached: i64,
    /// Absolute paths, shown in the UI so downloaded data is never a mystery.
    roms_dir: String,
    media_dir: String,
    disk_bytes: u64,
}

#[tauri::command]
fn status(state: State<'_, AppState>) -> CmdResult<Status> {
    let cache = state.cache.lock().map_err(err)?;
    Ok(Status {
        server: state
            .client
            .as_ref()
            .map(|c| c.base().to_owned())
            .unwrap_or_default(),
        connected: state.client.is_some(),
        retroarch: state
            .retroarch
            .as_ref()
            .map(|r| r.root.display().to_string()),
        cores_installed: state
            .retroarch
            .as_ref()
            .map(|r| r.installed_cores().len())
            .unwrap_or(0),
        roms_cached: cache.rom_count().unwrap_or(0),
        roms_dir: abs(&state.roms_dir),
        media_dir: abs(&state.media_dir),
        disk_bytes: dir_size(&state.roms_dir) + dir_size(&state.media_dir),
    })
}

fn abs(p: &Path) -> String {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).display().to_string()
}

/// Recursive size of everything we have downloaded, so the UI can say how much
/// disk this app is using before you go looking for it.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

// --- helpers -------------------------------------------------------------

fn local_path(state: &State<'_, AppState>, platform: &str, fs_name: &str) -> Option<PathBuf> {
    let p = state.roms_dir.join(platform).join(fs_name);
    p.is_file().then_some(p)
}

fn resolve_core(state: &State<'_, AppState>, platform: &str) -> Option<String> {
    let ra = state.retroarch.as_ref()?;
    if let Some(default) = state.map.default_core(platform)
        && ra.has_core(default)
    {
        return Some(default.to_owned());
    }
    state
        .map
        .alternatives(platform)
        .into_iter()
        .find(|c| ra.has_core(c))
        .map(str::to_owned)
}

/// Find the project root and make it the working directory.
///
/// Config, cache, core map and library are all addressed relative to the
/// project root, but the process cwd varies by how the app was started —
/// `tauri dev` uses `src-tauri/`, a bundled `.app` uses `/`. Anchor once here
/// so everything downstream can keep using plain relative paths.
fn anchor_to_project_root() {
    const MARKER: &str = "data/esde-core-map.json";

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend(cwd.ancestors().map(Path::to_path_buf));
    }
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().map(Path::to_path_buf));
    }

    for root in roots {
        if root.join(MARKER).is_file() {
            let _ = std::env::set_current_dir(&root);
            return;
        }
    }
    eprintln!(
        "warning: could not locate {MARKER}; run from the project root or the \
         library and cache will appear empty"
    );
}

fn main() {
    anchor_to_project_root();
    let cfg = Config::load().unwrap_or_default();
    let store = cache::Cache::open(Path::new(CACHE_DB)).expect("opening metadata cache");
    let map = CoreMap::load(Path::new(CORE_MAP)).expect("loading core map");
    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)
        .ok()
        .map(Arc::new);
    let retroarch = RetroArch::locate(cfg.retroarch.root.as_deref()).ok();
    let roms_dir = cfg.local_roms_dir();
    let media_dir = PathBuf::from(&cfg.library.local_root).join("downloaded_media");

    tauri::Builder::default()
        .manage(AppState {
            cache: Mutex::new(store),
            map,
            client,
            retroarch,
            roms_dir,
            media_dir,
            theme_root: cfg.theme.root.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            platforms,
            roms,
            search,
            rom_detail,
            download_rom,
            launch_rom,
            install_theme_logos,
            status
        ])
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
