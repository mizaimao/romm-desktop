//! Tauri GUI shell.
//!
//! Deliberately thin: every command here delegates to `romm_desktop`, the same
//! crate the CLI and TUI use. If logic starts accumulating in this file it
//! belongs in the core crate instead.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use romm_desktop::{
    api, cache, config::Config, coremap::{self, CoreMap}, download, media, retroarch::RetroArch,
    savesync, shaders, theme, theme_remote, util,
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
    /// Artwork of a locally scanned ES-DE library. Keyed by ES-DE *system*
    /// name rather than RomM slug, so it needs its own lookup rather than
    /// being folded into `media_dir`.
    esde_media: Option<PathBuf>,
    theme_root: Option<String>,
    themes_dir: PathBuf,
    /// Behind mutexes so a choice made in the UI takes effect on the next
    /// launch rather than the next restart. `config.toml` stays the source of
    /// truth; these are the live copy.
    core_overrides: Mutex<std::collections::BTreeMap<String, String>>,
    core_per_game: Mutex<std::collections::BTreeMap<String, String>>,
    user_retroarch_cfg: PathBuf,
    shaders_enabled: bool,
    shader_overrides: Mutex<std::collections::BTreeMap<String, String>>,
    /// Systems switched over to a light gun, so gun games aim with the mouse.
    lightgun: Mutex<std::collections::BTreeMap<String, String>>,
    /// Which ES-DE artwork the list and the info pane draw.
    list_art: Mutex<String>,
    detail_art: String,
    /// Strobe/BFI pass chained onto CRT shaders, if the user enabled one.
    motion_shader: Mutex<Option<String>>,
    /// Index into `IconStyle::ALL`. Atomic so the UI can switch style without
    /// taking any lock the render path also needs.
    icon_style: AtomicU8,
    /// RetroAchievements, read once at startup from this project's config.toml
    /// — see `romm_desktop::achievements`.
    achievements: romm_desktop::achievements::Settings,
    /// Pull before a launch and push after it exits.
    auto_sync: bool,
    /// Conflicts awaiting the user's answer, so the resolve command can act on
    /// one by name rather than the UI having to hand the whole record back.
    pending_conflicts: Mutex<Vec<romm_desktop::savesync::SaveConflict>>,
}

#[derive(Serialize)]
struct PlatformView {
    slug: String,
    name: String,
    rom_count: i64,
    /// Whether a libretro core for this platform is actually installed.
    playable: bool,
    /// ES-DE theme art, if any has been installed locally.
    logo: Option<String>,
    /// True only for the `logo` style, whose art is a white-on-transparent
    /// wordmark and therefore needs inverting on a light page. Hardware and
    /// console art is full colour and must not be touched.
    logo_wordmark: bool,
    /// Typical cover aspect (w/h) for this platform, so the grid can shape its
    /// cards instead of cropping. Null until enough covers are cached.
    cover_aspect: Option<f32>,
}

#[derive(Serialize)]
struct RomView {
    id: i64,
    name: String,
    fs_name: String,
    platform: String,
    size_bytes: i64,
    downloaded: bool,
    /// In a starred collection. Shown with a star and sorted to the top.
    favourite: bool,
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
    /// Present only when the video is already on this machine. The pane never
    /// downloads one: see `has_video`.
    video: Option<String>,
    /// Whether a gameplay video exists at all, local or on the server.
    ///
    /// Separate from `video` because the pane shows an indicator, not a
    /// player. A video is tens of megabytes against tens of kilobytes for
    /// every other kind of media a game has, and fetching one to find out
    /// whether it existed happened for every game the cursor touched.
    has_video: bool,
    /// Every screenshot we could resolve; the UI cycles through them.
    screenshots: Vec<String>,
    /// ES-DE artwork by type — 3dboxes, miximages, marquees, fanart and the
    /// rest. Far richer than RomM's own cover + one screenshot.
    art: std::collections::BTreeMap<String, String>,

    // Descriptive metadata, straight from RomM (which on this server got it
    // from the ES-DE gamelist import).
    summary: Option<String>,
    genres: Vec<String>,
    companies: Vec<String>,
    franchises: Vec<String>,
    game_modes: Vec<String>,
    player_count: Option<String>,
    /// 0-100 as RomM stores it.
    rating: Option<f64>,
    release_year: Option<i32>,
    alt_names: Vec<String>,
    regions: Vec<String>,
    manual: Option<String>,
    youtube_id: Option<String>,
}

/// Pull the library from the server into the local cache.
///
/// The GUI had no way to do this. The Windows release ships only the GUI, so a
/// fresh install there had an empty cache, nothing on screen, and no command to
/// fill it — which reads as "it cannot connect to the server" even though the
/// server is perfectly reachable. On macOS the cache was always populated by
/// the CLI beforehand, which is why it never showed up there.
///
/// Opens its own cache connection rather than using the shared one: `sync`
/// needs `&mut Cache` and awaits, and a `MutexGuard` cannot be held across an
/// await. Two SQLite connections to one file is fine, and the shared one sees
/// the committed rows immediately afterwards.
#[tauri::command]
async fn sync_library(app: tauri::AppHandle, state: State<'_, AppState>, full: bool) -> CmdResult<String> {
    let client = state
        .client
        .clone()
        .ok_or("no server configured — set [server] in config.toml")?;
    let mut store = cache::Cache::open(Path::new(CACHE_DB)).map_err(err)?;

    let _ = app.emit("sync-progress", "checking the server…");
    // Refresh the settings that govern hashing before anything downloads.
    if let Ok(cfg) = client.config().await {
        store.save_server_config(&cfg).ok();
    }

    let _ = app.emit("sync-progress", "fetching the library…");
    let (platforms, upserted, incremental) = store.sync(&client, full).await.map_err(err)?;

    // Removals never appear in an incremental pull.
    let mut pruned = 0;
    if let Ok(ids) = client.rom_identifiers().await {
        pruned = store.prune_missing(&ids).unwrap_or(0);
    }

    let _ = app.emit("sync-progress", "collections…");
    let collections = match client.all_collections().await {
        Ok(items) => store.replace_collections(&items).unwrap_or(0),
        Err(_) => 0,
    };

    // Console pictures for the platform grid. Cheap — a few KB of vector art
    // each, only for platforms not already cached — and it means the grid has
    // real artwork without anyone downloading a theme for it.
    let _ = app.emit("sync-progress", "console pictures…");
    let icons = match client.platforms().await {
        Ok(list) => {
            let pairs: Vec<(String, String)> =
                list.iter().map(|p| (p.slug.clone(), p.fs_slug.clone())).collect();
            romm_desktop::platformicon::ensure(&client, &state.media_dir, &pairs)
                .await
                .unwrap_or(0)
        }
        Err(_) => 0,
    };

    // A sync rewrites names from the server, so the real arcade titles go back
    // afterwards rather than before.
    let names = romm_desktop::arcade::names(Path::new("data/arcade-names.json"));
    let renamed = store.apply_arcade_names(&names).unwrap_or(0);

    let total = store.rom_count().unwrap_or(0);
    let _ = app.emit("sync-progress", "done");
    Ok(format!(
        "{} sync: {total} games across {platforms} platforms ({upserted} updated{}{}{})",
        if incremental { "Incremental" } else { "Full" },
        if pruned > 0 { format!(", {pruned} removed") } else { String::new() },
        if collections > 0 { format!(", {collections} collections") } else { String::new() },
        if renamed > 0 { format!(", {renamed} arcade titles") } else { String::new() },
    ) + &if icons > 0 { format!(", {icons} console pictures") } else { String::new() })
}

/// Open the Settings window, or focus it if it is already up.
///
/// A real second window rather than an overlay: settings are a place you go and
/// come back from, and the old panel put both binding tables — the longest
/// thing in the app — on top of the library you were trying to use. This one is
/// resizable and remembers nothing about the grid behind it.
#[tauri::command]
async fn open_settings(app: tauri::AppHandle) -> CmdResult<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    // Focus rather than open a second copy. Two settings windows would each
    // hold their own binding state and the last one saved would win.
    if let Some(existing) = app.get_webview_window("settings") {
        existing.set_focus().map_err(err)?;
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Settings")
        .inner_size(1000.0, 740.0)
        // Below this the tab rail and a binding row stop fitting side by side.
        .min_inner_size(640.0, 460.0)
        .resizable(true)
        .build()
        .map_err(err)?;
    Ok(())
}

/// The config.toml values Settings can show and edit.
///
/// Deliberately a fixed list rather than "whatever is in the file". Half of
/// config.toml is not a setting — `cores.per_game` is 155 rows that belong in
/// the game detail pane, and `[scraper]` is read by nothing. A field here means
/// somebody decided it belongs on screen.
#[derive(Serialize)]
struct ConfigFields {
    library_root: String,
    server_url: String,
    server_username: String,
    /// Present or not, never the value. A settings pane has no reason to hand a
    /// credential back to the webview to display; the box shows "set" and lets
    /// you replace it.
    server_token_set: bool,
    achievements_enabled: bool,
    achievements_username: String,
    achievements_token_set: bool,
    achievements_hardcore: bool,
    shaders_enabled: bool,
    /// Present so the UI can say where it is writing, and warn when there is
    /// nothing to write to.
    config_path: String,
    config_exists: bool,
}

#[tauri::command]
fn config_fields() -> CmdResult<ConfigFields> {
    let cfg = Config::load().unwrap_or_default();
    Ok(ConfigFields {
        library_root: cfg.library.local_root.clone(),
        server_url: cfg.server.url.clone(),
        server_username: cfg.server.username.clone(),
        server_token_set: cfg.server.token.as_deref().is_some_and(|t| !t.trim().is_empty()),
        achievements_enabled: cfg.achievements.enabled,
        achievements_username: cfg.achievements.username.clone().unwrap_or_default(),
        achievements_token_set: cfg
            .achievements
            .token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty()),
        achievements_hardcore: cfg.achievements.hardcore,
        shaders_enabled: cfg.shaders.enabled,
        config_path: abs(Path::new("config.toml")),
        config_exists: Config::exists("config.toml"),
    })
}

/// Write one value into config.toml, or remove it when empty.
///
/// Goes through the same targeted TOML edit the per-system settings use, so the
/// hand-written comments explaining non-obvious choices survive — round-tripping
/// through a serialiser would delete every one of them.
///
/// Only the fields `config_fields` exposes are accepted. An allowlist rather
/// than passing a section and key straight through: this is called from a
/// webview, and "write any key into any table" is a larger door than this needs.
#[tauri::command]
fn set_config_field(field: String, value: String) -> CmdResult<String> {
    let (table, key) = match field.as_str() {
        "library_root" => ("library", "local_root"),
        "server_url" => ("server", "url"),
        "server_token" => ("server", "token"),
        "server_username" => ("server", "username"),
        "scraper_ssid" => ("scraper", "ssid"),
        "scraper_sspassword" => ("scraper", "sspassword"),
        "scraper_devid" => ("scraper", "devid"),
        "scraper_devpassword" => ("scraper", "devpassword"),
        "achievements_enabled" => ("achievements", "enabled"),
        "achievements_username" => ("achievements", "username"),
        "achievements_token" => ("achievements", "token"),
        "achievements_hardcore" => ("achievements", "hardcore"),
        "shaders_enabled" => ("shaders", "enabled"),
        other => return Err(format!("unknown setting {other}")),
    };

    // Booleans are TOML literals, not strings, so they cannot go through the
    // quoted-string writer.
    let boolean = matches!(
        field.as_str(),
        "achievements_enabled" | "achievements_hardcore" | "shaders_enabled"
    );

    if value.trim().is_empty() && !boolean {
        romm_desktop::config::clear_table_entry("config.toml", table, key).map_err(err)?;
        return Ok(format!("{key} cleared"));
    }
    if boolean {
        romm_desktop::config::set_table_bool(
            "config.toml",
            table,
            key,
            value == "true" || value == "1",
        )
        .map_err(err)?;
    } else {
        romm_desktop::config::set_table_entry("config.toml", table, key, value.trim())
            .map_err(err)?;
    }
    Ok(format!("{key} saved — restart to apply"))
}

/// Try the credentials without saving them.
///
/// The point of a Verify button is to answer "will this work" *before* it is
/// committed, so this builds a throwaway client from whatever is in the box
/// rather than from the running config.
#[tauri::command]
async fn verify_server(
    url: String,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
) -> CmdResult<String> {
    let client = api::Client::with_auth(
        url.trim(),
        username.as_deref().unwrap_or_default(),
        password.as_deref().unwrap_or_default(),
        token.as_deref(),
    )
    .map_err(err)?;

    // Heartbeat first: it needs no credentials, so a failure here is the server
    // or the network rather than the token, and saying which saves a lot of
    // guessing.
    let version = match client.heartbeat().await {
        Ok(hb) => hb.system.version,
        Err(e) => {
            return Err(format!(
                "cannot reach {} — {}",
                url.trim(),
                e.to_string().lines().next().unwrap_or("no answer")
            ));
        }
    };

    // Then something that does need them.
    match client.me().await {
        Ok(me) => {
            let count = client.rom_count().await.unwrap_or(0);
            Ok(format!(
                "Connected to RomM {version} as {} — {count} games",
                me.username
            ))
        }
        Err(e) => Err(format!(
            "reached the server (RomM {version}) but the credentials were refused — {}",
            e.to_string().lines().next().unwrap_or("")
        )),
    }
}

/// Download the BIOS set from the server.
///
/// Optional and explicit: it is 300-odd MB on this server, and a launcher that
/// pulls that on first run without asking is a launcher people uninstall.
#[tauri::command]
async fn sync_bios(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    let client = state.client.clone().ok_or("not connected to a server")?;
    let library_root = state.roms_dir.parent().unwrap_or(Path::new(".")).to_path_buf();

    let summary = romm_desktop::bios::sync(&client, &library_root, |done, total, name| {
        let _ = app.emit("bios-progress", format!("{done}/{total}  {name}"));
    })
    .await
    .map_err(err)?;

    let mut out = summary.headline();
    if !summary.notes.is_empty() {
        out.push('\n');
        out.push_str(&summary.notes.iter().take(6).cloned().collect::<Vec<_>>().join("\n"));
    }
    Ok(out)
}

/// Pull the useful bits out of RomM's merged `metadatum` blob.
fn meta_strings(meta: &Option<serde_json::Value>, key: &str) -> Vec<String> {
    meta.as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// This build's version, and the server's if we have ever spoken to it.
///
/// Both, because "which version am I on" is nearly always asked when something
/// behaves differently on two machines, and the answer is as often the server
/// as the client.
#[tauri::command]
fn versions(state: State<'_, AppState>) -> CmdResult<(String, Option<String>)> {
    let server = state.cache.lock().ok().and_then(|c| c.server_version());
    Ok((env!("CARGO_PKG_VERSION").to_owned(), server))
}

/// The games played most recently, for the row at the top of the library.
///
/// Server timestamps, so the list is the same wherever you sign in — the point
/// is picking up where you left off, and that is rarely the machine you are
/// sitting at now.
#[tauri::command]
fn recent_games(state: State<'_, AppState>, limit: Option<usize>) -> CmdResult<Vec<RomView>> {
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        cache.recently_played(limit.unwrap_or(8)).map_err(err)?
    };
    Ok(to_views(&state, rows))
}

/// What a bulk download would cost, before starting one.
#[tauri::command]
fn download_estimate(
    state: State<'_, AppState>,
    platform: Option<String>,
    collection: Option<String>,
    art: String,
    videos: bool,
    manuals: bool,
) -> CmdResult<(String, bool, String)> {
    use romm_desktop::{bulk, diskspace};

    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        match (&platform, &collection) {
            (_, Some(id)) => cache.roms_in_collection(id).map_err(err)?,
            (Some(p), _) => cache.roms_for(p).map_err(err)?,
            _ => return Err("nothing chosen".into()),
        }
    };
    let want = bulk::Want {
        roms: true,
        art: match art.as_str() {
            "none" => bulk::Art::None,
            "full" => bulk::Art::Full,
            _ => bulk::Art::Minimal,
        },
        videos,
        manuals,
    };
    let est = bulk::estimate(&rows, want, |r| row_path(&state, r).is_some());
    let (fits, note) = match diskspace::fits(&state.roms_dir, est.total()) {
        diskspace::Fit::Yes { available } => {
            (true, format!("{:.0} GB free", available as f64 / 1e9))
        }
        diskspace::Fit::No { available, short } => (
            false,
            format!(
                "only {:.0} GB free — {:.0} GB short, counting the {} GB this leaves spare",
                available as f64 / 1e9,
                short as f64 / 1e9,
                diskspace::MARGIN / 1_000_000_000,
            ),
        ),
        // Never a refusal: turning a failed syscall into "you cannot download"
        // would be a worse bug than the one the check exists to prevent.
        diskspace::Fit::Unknown => (true, "could not read free space".to_owned()),
    };
    Ok((est.describe(), fits, note))
}

/// Download a whole platform or collection, with the media that was asked for.
///
/// Refuses up front when it would not fit. The failure without that check is
/// the worst kind: an hour of transfer, a full disk, and both a half-written
/// game and no room to clear it.
#[tauri::command]
async fn download_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: Option<String>,
    collection: Option<String>,
    art: String,
    videos: bool,
    manuals: bool,
) -> CmdResult<String> {
    use romm_desktop::{bulk, diskspace};

    let client = state.client.clone().ok_or("no server configured")?;
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        match (&platform, &collection) {
            (_, Some(id)) => cache.roms_in_collection(id).map_err(err)?,
            (Some(p), _) => cache.roms_for(p).map_err(err)?,
            _ => return Err("nothing chosen".into()),
        }
    };
    let want = bulk::Want {
        roms: true,
        art: match art.as_str() {
            "none" => bulk::Art::None,
            "full" => bulk::Art::Full,
            _ => bulk::Art::Minimal,
        },
        videos,
        manuals,
    };
    let est = bulk::estimate(&rows, want, |r| row_path(&state, r).is_some());
    if let diskspace::Fit::No { short, .. } = diskspace::fits(&state.roms_dir, est.total()) {
        return Err(format!(
            "not enough room — {:.1} GB short. Free some space or take less media.",
            short as f64 / 1e9
        ));
    }

    let media_root = state.media_dir.clone();
    let list_art = state.list_art.lock().map_err(err)?.clone();
    let mut games = 0usize;
    let total = rows.len();

    for (i, row) in rows.iter().enumerate() {
        if row_path(&state, row).is_none() {
            let members = if row.multi_file {
                client.member_hashes(row.id).await
            } else {
                Vec::new()
            };
            let target = romm_desktop::download::Target {
                rom_id: row.id,
                members: &members,
                fs_name: &row.fs_name,
                platform_slug: &row.platform_slug,
                expected_size: (row.fs_size_bytes > 0).then_some(row.fs_size_bytes as u64),
                md5: row.md5_hash.as_deref(),
                sha1: row.sha1_hash.as_deref(),
                multi_file: row.multi_file,
            };
            if romm_desktop::download::fetch(
                client.http(), client.base(), client.auth(), &target, &state.roms_dir, |_, _| {},
            )
            .await
            .is_ok()
            {
                games += 1;
            }
        }

        let stem = Path::new(&row.fs_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| row.fs_name.clone());
        // Whatever media was asked for. `ensure_*` skip anything already here,
        // so re-running after an interruption costs a stat rather than a fetch.
        if want.art != bulk::Art::None {
            let _ = media::ensure_art(
                Some(&client), &media_root, &row.platform_slug, &stem, &list_art,
            ).await;
            let _ = media::ensure_art(
                Some(&client), &media_root, &row.platform_slug, &stem, media::MIXIMAGES,
            ).await;
        }
        if want.art == bulk::Art::Full {
            for (kind, _) in media::ESDE_TYPES {
                if matches!(*kind, media::VIDEOS) {
                    continue;
                }
                let _ = media::ensure_esde(
                    Some(&client), &media_root, &row.platform_slug, &stem, kind,
                ).await;
            }
        }
        if want.videos {
            let _ = media::ensure_esde(
                Some(&client), &media_root, &row.platform_slug, &stem, media::VIDEOS,
            ).await;
        }
        if want.manuals {
            let _ = media::ensure_esde(
                Some(&client), &media_root, &row.platform_slug, &stem, "manuals",
            ).await;
        }

        if (i + 1) % 5 == 0 || i + 1 == total {
            let _ = app.emit("bulk-progress", format!("{}/{} — {games} downloaded", i + 1, total));
        }
    }
    Ok(format!("{games} game(s) downloaded, {total} checked"))
}

#[tauri::command]
fn platforms(state: State<'_, AppState>) -> CmdResult<Vec<PlatformView>> {
    let cache = state.cache.lock().map_err(err)?;
    let rows = cache.platforms().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|p| PlatformView {
            playable: resolve_core(&state, &p.fs_slug).is_some(),
            // A theme, if one is installed, then the console picture from the
            // server. The theme wins because installing one is a deliberate
            // choice and this is the fallback that means nobody has to.
            logo: theme::installed_logo(&state.media_dir, &p.fs_slug, current_style(&state))
                .or_else(|| romm_desktop::platformicon::installed(&state.media_dir, &p.fs_slug))
                .map(|p| p.display().to_string()),
            logo_wordmark: theme::installed_logo(&state.media_dir, &p.fs_slug, current_style(&state))
                .is_some()
                && current_style(&state) == theme::IconStyle::Logo,
            cover_aspect: media::cover_aspect(&state.media_dir, &p.fs_slug),
            slug: p.fs_slug,
            name: p.display_name,
            rom_count: p.rom_count,
        })
        .collect())
}

/// Shape cache rows for the list/grid, marking what is already on disk.
fn to_views(state: &State<'_, AppState>, rows: Vec<cache::RomRow>) -> Vec<RomView> {
    // One query for the whole list rather than one per row.
    let favourites = state
        .cache
        .lock()
        .ok()
        .and_then(|c| c.favourite_ids().ok())
        .unwrap_or_default();
    rows.into_iter()
        .map(|r| RomView {
            favourite: favourites.contains(&r.id),
            downloaded: row_path(state, &r).is_some(),
            id: r.id,
            name: r.name,
            fs_name: r.fs_name,
            platform: r.platform_slug,
            size_bytes: r.fs_size_bytes,
        })
        .collect()
}

#[tauri::command]
fn roms(state: State<'_, AppState>, platform: String) -> CmdResult<Vec<RomView>> {
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        cache.roms_for(&platform).map_err(err)?
    };
    Ok(to_views(&state, rows))
}

/// One collection group — `genre`, `franchise`, `user`, …
#[derive(serde::Serialize)]
struct GroupView {
    group: String,
    /// Human label; the raw group name is a server-side slug.
    label: String,
    count: i64,
}

#[derive(serde::Serialize)]
struct CollectionView {
    id: String,
    name: String,
    rom_count: i64,
    is_favorite: bool,
    /// A few member ROM ids — the card fetches their covers through the same
    /// local cache the game grids use, so this works offline too.
    sample_ids: Vec<i64>,
    /// How many of its games are downloaded here.
    ///
    /// The question a collection card cannot otherwise answer: "can I play any
    /// of this on a plane". Counted rather than stored, because a file can be
    /// deleted from under us and a stale count is worse than none.
    local_count: i64,
}

/// Plural label for a group, since the server's names are singular slugs.
fn group_label(group: &str) -> String {
    match group {
        "user" => "My collections".to_owned(),
        "smart" => "Smart collections".to_owned(),
        "collection" => "Series".to_owned(),
        "genre" => "Genres".to_owned(),
        "franchise" => "Franchises".to_owned(),
        "company" => "Companies".to_owned(),
        "mode" => "Player modes".to_owned(),
        "age_rating" => "Age ratings".to_owned(),
        // Unknown kinds still appear rather than being hidden — a RomM that
        // grows a new one should show up without a client change.
        other => {
            let mut c = other.replace('_', " ");
            c[..1].make_ascii_uppercase();
            c
        }
    }
}

#[tauri::command]
fn collection_groups(state: State<'_, AppState>) -> CmdResult<Vec<GroupView>> {
    let cache = state.cache.lock().map_err(err)?;
    Ok(cache
        .collection_groups()
        .map_err(err)?
        .into_iter()
        .map(|(group, count)| GroupView {
            label: group_label(&group),
            group,
            count,
        })
        .collect())
}

#[tauri::command]
fn collections_in(state: State<'_, AppState>, group: String) -> CmdResult<Vec<CollectionView>> {
    let cache = state.cache.lock().map_err(err)?;
    Ok(cache
        .collections_in(&group)
        .map_err(err)?
        .into_iter()
        .map(|c| CollectionView {
            local_count: cache
                .roms_in_collection(&c.id)
                .map(|rows| rows.iter().filter(|r| row_path(&state, r).is_some()).count() as i64)
                .unwrap_or(0),
            sample_ids: c.sample_ids,
            id: c.id,
            name: c.name,
            rom_count: c.rom_count,
            is_favorite: c.is_favorite,
        })
        .collect())
}

#[tauri::command]
fn collection_roms(state: State<'_, AppState>, id: String) -> CmdResult<Vec<RomView>> {
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        cache.roms_in_collection(&id).map_err(err)?
    };
    Ok(to_views(&state, rows))
}

#[tauri::command]
fn search(state: State<'_, AppState>, term: String) -> CmdResult<Vec<RomView>> {
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        cache.search(&term, 200).map_err(err)?
    };
    Ok(to_views(&state, rows))
}

#[tauri::command]
async fn rom_detail(state: State<'_, AppState>, id: i64) -> CmdResult<RomDetail> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let core = resolve_core_for(&state, &row.platform_slug, Some(&row.fs_name));
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
    // A locally scanned ES-DE library keeps its artwork on the same disk as
    // the games, keyed by ES-DE system name. Nothing needs fetching there, so
    // the server client is dropped for those rows — otherwise every miss would
    // become a pointless request against a server this library did not come
    // from.
    let (scope_dir, scope_key) = media_scope(&state, &row);
    let media_root = scope_dir.to_path_buf();
    let media_key = scope_key.to_owned();
    let client = if state.esde_media.is_some() && row.esde_system.is_some() {
        None
    } else {
        client
    };
    let as_url = |p: Option<std::path::PathBuf>| p.map(|p| p.display().to_string());

    // ES-DE's own art, picked the way its Canvas theme picks it. RomM's cover
    // is no longer consulted: it is a second scrape from a different source,
    // and one game's art coming from one place and the next game's from
    // another is the inconsistency this replaces.
    let cover =
        media::ensure_art(client.as_deref(), &media_root, &media_key, &stem, &state.detail_art)
            .await;
    let screenshots = media::ensure_set(
        client.as_deref(), &media_root, &media_key, &stem,
        &row.screenshots(),
    ).await;
    // Only if it is already here. Downloading is what the play button does.
    let video = media::find_local(&media_root, &media_key, &stem, media::VIDEOS);
    let has_video = video.is_some()
        || media::video_exists(client.as_deref(), &media_root, &media_key, &stem).await;

    // Manuals are PDFs, which the webview renders natively.
    let manual = media::ensure(
        client.as_deref(), &media_root, &media_key, &stem,
        media::MANUALS, row.manual_path.as_deref(),
    ).await;

    // Everything ES-DE has for this game, fetched lazily and cached.
    let mut art = std::collections::BTreeMap::new();
    for (kind, _) in media::ESDE_TYPES {
        // Videos are fetched by the play button, never by looking at a game.
        if matches!(*kind, media::VIDEOS) {
            continue;
        }
        if let Some(p) = media::ensure_esde(
            client.as_deref(), &media_root, &media_key, &stem, kind,
        )
        .await
        {
            art.insert((*kind).to_owned(), p.display().to_string());
        }
    }

    let meta: Option<serde_json::Value> = row
        .meta_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let json_list = |s: &Option<String>| -> Vec<String> {
        s.as_deref()
            .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
            .unwrap_or_default()
    };
    // RomM stores the release date as epoch milliseconds.
    let release_year = meta
        .as_ref()
        .and_then(|m| m.get("first_release_date"))
        .and_then(|v| v.as_f64())
        .map(|ms| 1970 + (ms / 1000.0 / 31_556_952.0) as i32);

    Ok(RomDetail {
        cover: as_url(cover),
        video: as_url(video),
        has_video,
        screenshots: screenshots.into_iter().map(|p| p.display().to_string()).collect(),
        art,
        downloaded: local_path(&state, &row.platform_slug, &row.fs_name).is_some(),
        id: row.id,
        name: row.name,
        fs_name: row.fs_name,
        platform: row.platform_slug,
        size_bytes: row.fs_size_bytes,
        core,
        core_label,
        summary: row.summary.clone().filter(|s| !s.is_empty()),
        genres: meta_strings(&meta, "genres"),
        companies: meta_strings(&meta, "companies"),
        franchises: meta_strings(&meta, "franchises"),
        game_modes: meta_strings(&meta, "game_modes"),
        player_count: meta
            .as_ref()
            .and_then(|m| m.get("player_count"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        rating: meta
            .as_ref()
            .and_then(|m| m.get("average_rating"))
            .and_then(|v| v.as_f64()),
        release_year,
        alt_names: json_list(&row.alt_names_json),
        regions: json_list(&row.regions_json),
        manual: manual.map(|p| p.display().to_string()),
        youtube_id: row.youtube_id.clone().filter(|s| !s.is_empty()),
    })
}

#[derive(Serialize)]
struct CoverView {
    id: i64,
    cover: Option<String>,
}

/// Resolve covers for a batch of ROMs.
///
/// The grid asks only for what has scrolled into view, and this bounds the
/// fan-out further, so browsing a 2,400-game platform never turns into 2,400
/// simultaneous requests.
#[tauri::command]
async fn rom_covers(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<Vec<CoverView>> {
    const CONCURRENCY: usize = 8;

    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        ids.iter()
            .filter_map(|id| cache.rom_by_id(*id).ok().flatten())
            .collect::<Vec<_>>()
    };

    let list_art = state.list_art.lock().map_err(err)?.clone();
    let mut out = Vec::with_capacity(rows.len());
    for chunk in rows.chunks(CONCURRENCY) {
        let mut set = tokio::task::JoinSet::new();
        for row in chunk {
            let client = state.client.clone();
            let media_root = state.media_dir.clone();
            let (id, platform, fs_name) =
                (row.id, row.platform_slug.clone(), row.fs_name.clone());
            let art = list_art.clone();
            set.spawn(async move {
                let stem = Path::new(&fs_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or(fs_name);
                let cover =
                    media::ensure_art(client.as_deref(), &media_root, &platform, &stem, &art)
                        .await;
                CoverView { id, cover: cover.map(|p| p.display().to_string()) }
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok(v) = res {
                out.push(v);
            }
        }
    }
    // Keep what this batch learned, so scrolling back over the same cards — and
    // the next launch — costs nothing.
    for platform in rows.iter().map(|r| r.platform_slug.as_str()).collect::<std::collections::BTreeSet<_>>() {
        media::save_art_index(&state.media_dir, platform);
    }
    Ok(out)
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

    // Folder ROMs verify per member; the rom-level hash is not reproducible.
    let members = if row.multi_file {
        client.member_hashes(row.id).await
    } else {
        Vec::new()
    };

    let target = download::Target {
        rom_id: row.id,
        members: &members,
        fs_name: &row.fs_name,
        platform_slug: &row.platform_slug,
        expected_size: (row.fs_size_bytes > 0).then_some(row.fs_size_bytes as u64),
        md5: row.md5_hash.as_deref(),
        sha1: row.sha1_hash.as_deref(),
        multi_file: row.multi_file,
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
        download::Outcome::Downloaded { path, verified, .. } => {
            format!("downloaded {} ({})", path.display(), verified.describe())
        }
    })
}

/// Launch a ROM in RetroArch. Blocks until the emulator exits.
///
/// The artwork choices for a game list, and which one is in force.
#[tauri::command]
fn list_art_options(state: State<'_, AppState>) -> CmdResult<(Vec<(String, String)>, String)> {
    let current = state.list_art.lock().map_err(err)?.clone();
    let choices = media::LIST_ART_CHOICES
        .iter()
        .map(|(k, label)| ((*k).to_owned(), (*label).to_owned()))
        .collect();
    Ok((choices, current))
}

/// Change what the game lists draw.
///
/// The cached image for a game is keyed by kind, so switching does not throw
/// anything away — art already fetched for the old choice stays where it is and
/// is there again if you switch back.
#[tauri::command]
fn set_list_art(state: State<'_, AppState>, value: String) -> CmdResult<String> {
    if !media::LIST_ART_CHOICES.iter().any(|(k, _)| *k == value) {
        return Err(format!("unknown artwork type {value}"));
    }
    romm_desktop::config::set_table_entry("config.toml", "media", "list_art", &value)
        .map_err(err)?;
    *state.list_art.lock().map_err(err)? = value.clone();
    Ok(format!("game lists now show {value}"))
}

/// Fetch artwork for games that have none, through the server's ScreenScraper
/// account.
///
/// Serial and unhurried on purpose: ScreenScraper throttles by account tier and
/// answers an exceeded allowance with a rejection rather than a picture, so a
/// run that hurries finishes fast and fetches nothing. The allowance being
/// spent is the server's, shared with anything else pointed at it.
#[tauri::command]
async fn scrape_missing(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    platform: Option<String>,
) -> CmdResult<String> {
    use romm_desktop::scrape;

    let client = state
        .client
        .clone()
        .ok_or("no server configured — set [server] in config.toml")?;
    let media_root = state.media_dir.clone();

    let todo = {
        let cache = state.cache.lock().map_err(err)?;
        scrape::missing(&cache, &media_root, platform.as_deref()).map_err(err)?
    };
    if todo.is_empty() {
        return Ok("every game already has artwork".to_owned());
    }

    let _ = app.emit("scrape-progress", format!("{} to look up…", todo.len()));
    let mut report = scrape::Report::default();
    for (i, row) in todo.iter().enumerate() {
        let _ = scrape::fill_one(&client, &media_root, row, false, &mut report).await;
        if (i + 1).is_multiple_of(10) || i + 1 == todo.len() {
            let _ = app.emit(
                "scrape-progress",
                format!("{}/{} — {} found", i + 1, todo.len(), report.fetched),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    // Those games have art now, so what the grid learned about them is wrong.
    for platform in todo.iter().map(|r| r.platform_slug.as_str()).collect::<std::collections::BTreeSet<_>>() {
        media::clear_art_index(&state.media_dir, platform);
    }
    Ok(report.describe())
}

/// Fetch a game's video and hand back its path, downloading it if needed.
///
/// Deliberately a separate command from `rom_detail`, and deliberately slow:
/// this is the one moment someone has asked for a video, so it is the one
/// moment worth spending tens of megabytes on.
#[tauri::command]
async fn game_video(state: State<'_, AppState>, id: i64) -> CmdResult<String> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let stem = Path::new(&row.fs_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| row.fs_name.clone());
    let (scope_dir, scope_key) = media_scope(&state, &row);
    let (media_root, media_key) = (scope_dir.to_path_buf(), scope_key.to_owned());
    let client = state.client.clone();

    media::ensure_esde(client.as_deref(), &media_root, &media_key, &stem, media::VIDEOS)
        .await
        .map(|p| p.display().to_string())
        .ok_or_else(|| format!("no video for {}", row.name))
}

/// The display the app is on, in the units RetroArch's window sizing uses.
///
/// Not a single number, because the two platforms disagree. Windows sizes
/// windows in device pixels, so a 4K screen at 150% scaling wants 3840. macOS
/// sizes them in points, so the same physical screen wants the logical figure —
/// pass it the device pixels there and the window is asked to be twice the size
/// of the display, which the compositor answers by clamping it to a shape
/// neither centred nor the size that was asked for.
///
/// Asked of the monitor rather than of the webview: `window.screen` reports CSS
/// pixels, which is the logical figure on both, so it is wrong on exactly one
/// of them and looks right while being tested on the other.
fn work_area(app: &tauri::AppHandle) -> Option<(u32, u32)> {
    use tauri::Manager as _;

    // The monitor the library window is on, so launching from a laptop screen
    // with an external display attached sizes for the one being looked at.
    let monitor = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;

    let size = monitor.size();
    if cfg!(target_os = "macos") {
        let scale = monitor.scale_factor().max(1.0);
        Some(((size.width as f64 / scale) as u32, (size.height as f64 / scale) as u32))
    } else {
        Some((size.width, size.height))
    }
}

/// `pad` is the name the frontend's Gamepad API reports for the connected
/// controller, used to pick the RetroArch autoconfig profile the gamepad
/// hotkeys are derived from. Absent when nothing is connected.
#[tauri::command]
async fn launch_rom(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: i64,
    pad: Option<String>,
    refresh: Option<f32>,
    // Set by the retry after the user answered an offline warning.
    skip_sync: Option<bool>,
) -> CmdResult<String> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let ra = state.retroarch.as_ref().ok_or("RetroArch not found")?;
    let path = local_path(&state, &row.platform_slug, &row.fs_name)
        .ok_or("not downloaded yet")?;
    // One shared planner for GUI, CLI and TUI — see launch.rs for why.
    let overrides = state.core_overrides.lock().map_err(err)?.clone();
    let per_game = state.core_per_game.lock().map_err(err)?.clone();
    let shader_overrides = state.shader_overrides.lock().map_err(err)?.clone();
    let motion = state.motion_shader.lock().map_err(err)?.clone();
    let lightgun = state.lightgun.lock().map_err(err)?.clone();
    let lib = state.roms_dir.parent().unwrap_or(Path::new("."));
    let req = romm_desktop::launch::Request {
        rom: &path,
        platform: &row.platform_slug,
        fs_name: &row.fs_name,
        library_root: lib,
        user_cfg: &state.user_retroarch_cfg,
        shaders_enabled: state.shaders_enabled,
        shader_overrides: &shader_overrides,
        motion_shader: motion.as_deref(),
        refresh_hz: refresh,
        core_overrides: &overrides,
        core_per_game: &per_game,
        core_override: None,
        pad: pad.as_deref(),
        achievements: Some(&state.achievements),
        lightgun: &lightgun,
        screen: work_area(&app),
    };
    // Fetch what is missing before planning. `plan` only ever picks among cores
    // already on disk, so without this a fresh install — which has none — fails
    // with "no installed core" even when the buildbot has it.
    let wanted = coremap::resolve_core_for(
        &state.map,
        &overrides,
        &per_game,
        &row.platform_slug,
        Some(&row.fs_name),
        |_| true, // ignore what is installed; that is the point
    );
    let mut fetched = Vec::new();
    // Reuses the API client's HTTP stack; without a configured server there is
    // nothing to download from anyway.
    if let (Some(core), Some(api)) = (wanted.as_deref(), state.client.as_ref()) {
        let http = api.http();
        match romm_desktop::cores::ensure(http, ra, core).await {
            Ok(true) => fetched.push(format!("downloaded the {core} core")),
            Ok(false) => {}
            // Not fatal: an offline launch of an already-installed core should
            // still work, and `plan` reports the real problem if it does not.
            Err(e) => fetched.push(format!("could not fetch {core}: {e}")),
        }
        if state.shaders_enabled {
            match shaders::ensure_pack(http, ra).await {
                Ok(true) => fetched.push("downloaded the shader pack".to_owned()),
                Ok(false) => {}
                Err(e) => fetched.push(format!("could not fetch shaders: {e}")),
            }
        }
        // BIOS, for the same reason as the core: telling someone to go and
        // install one is advice delivered at the exact moment they cannot see
        // why the screen is black. Only what this platform actually needs.
        let library_root = state.roms_dir.parent().unwrap_or(Path::new("."));
        match romm_desktop::bios::ensure(api, library_root, core, &row.platform_slug).await {
            Ok(0) => {}
            Ok(n) => fetched.push(format!("fetched {n} BIOS file(s)")),
            Err(e) => fetched.push(format!("could not fetch BIOS: {e}")),
        }
    }

    let plan = romm_desktop::launch::plan(ra, &state.map, &req).map_err(err)?;

    // Steam-cloud shape: pull what the server has that is newer, play, then
    // push whatever changed. `plan.run` blocks until the emulator exits, so
    // those two moments are a real boundary rather than a guess about when
    // someone stopped playing.
    let mut notes = fetched;
    let pre = if skip_sync.unwrap_or(false) {
        // The user already said "play anyway" to an offline warning; asking
        // again on the retry would be a loop they cannot get out of.
        notes.push("saves: not synced — you chose to play anyway".to_owned());
        AutoSync::default()
    } else {
        auto_sync(&state, ra, &row, savesync::When::BeforeLaunch).await
    };
    if let Some(note) = pre.note {
        notes.push(note);
    }
    // Could not sync at all. Steam asks rather than either blocking or starting
    // silently, and it is the right call: the save may be stale, which is worth
    // knowing before you put an hour into it, but being unable to play because
    // a server is off would be worse.
    if let Some(why) = pre.failed {
        return Err(format!("SAVE_OFFLINE:{why}"));
    }
    // A conflict stops the launch, as Steam does. Playing on top of a save
    // whose ownership is unresolved is how the loser gets overwritten for good
    // on the way back out — the one moment where continuing quietly is worse
    // than refusing.
    if !pre.conflicts.is_empty() {
        *state.pending_conflicts.lock().map_err(err)? = pre.conflicts.clone();
        return Err(format!(
            "SAVE_CONFLICT:{}",
            serde_json::to_string(&pre.conflicts).unwrap_or_default()
        ));
    }

    let status = plan.run(ra, &path, false).map_err(err)?;

    let post = if skip_sync.unwrap_or(false) {
        AutoSync::default()
    } else {
        auto_sync(&state, ra, &row, savesync::When::AfterExit).await
    };
    if let Some(note) = post.note {
        notes.push(note);
    }
    // After the fact there is nothing to ask: the game has already been played.
    // Say so in the notes so progress that has not left the machine is visible.
    if let Some(why) = post.failed {
        notes.push(format!("saves: NOT uploaded — {why}"));
    }
    if !post.conflicts.is_empty() {
        *state.pending_conflicts.lock().map_err(err)? = post.conflicts;
    }

    let prefix = if notes.is_empty() {
        String::new()
    } else {
        format!("{}; ", notes.join("; "))
    };
    Ok(if status.success() {
        format!("{prefix}{} exited cleanly", row.name)
    } else {
        format!("{prefix}{} exited with {status}", row.name)
    })
}

/// What one half of the automatic sync produced.
#[derive(Default)]
struct AutoSync {
    /// A line for the launch notes, when anything happened.
    note: Option<String>,
    /// Saves changed on both sides. Non-empty means the user has to choose.
    conflicts: Vec<romm_desktop::savesync::SaveConflict>,
    /// Set when the sync could not run at all — server down, DNS gone, timeout.
    /// Distinct from a conflict: nothing is wrong with the save, we simply do
    /// not know whether it is current.
    failed: Option<String>,
}

/// One half of the automatic sync for a single game.
///
/// A sync *failure* never stops anything: an unreachable server must not lock
/// the library, so the problem goes into the launch notes and play continues
/// offline. A *conflict* is different — that is a question only the user can
/// answer, and the caller refuses to launch until they have.
async fn auto_sync(
    state: &State<'_, AppState>,
    ra: &RetroArch,
    row: &cache::RomRow,
    when: savesync::When,
) -> AutoSync {
    if !state.auto_sync {
        return AutoSync::default();
    }
    let Some(client) = state.client.clone() else {
        return AutoSync::default();
    };
    let library_root = state.roms_dir.parent().unwrap_or(Path::new(".")).to_path_buf();

    // The SQLite connection is not Sync, so the scan takes the lock and drops
    // it before any awaiting starts.
    let candidates = {
        let Ok(cache) = state.cache.lock() else {
            return AutoSync::default();
        };
        match savesync::scan_for_rom(&cache, &state.map, &ra.root, &row.fs_name) {
            Ok(c) => c,
            Err(e) => {
                return AutoSync {
                    failed: Some(format!("could not read the save folder: {e}")),
                    ..Default::default()
                };
            }
        }
    };

    match savesync::run_all(&client, &candidates, &ra.root, Path::new("."), &library_root).await {
        Ok(summary) => AutoSync {
            note: savesync::describe(when, &summary),
            conflicts: summary.conflicts,
            failed: None,
        },
        Err(e) => AutoSync {
            failed: Some(
                e.to_string().lines().next().unwrap_or("the server did not answer").to_owned(),
            ),
            ..Default::default()
        },
    }
}

/// Carry out the user's answer to a conflict, then report what happened.
///
/// Separate from `launch_rom` so the UI can present the choice and then launch
/// again: a Tauri command cannot block waiting for a webview dialog.
#[tauri::command]
async fn resolve_save_conflict(
    state: State<'_, AppState>,
    file_name: String,
    keep: romm_desktop::savesync::Keep,
) -> CmdResult<String> {
    let client = state.client.clone().ok_or("not connected to a server")?;
    let ra = state.retroarch.as_ref().ok_or("RetroArch not found")?;
    let library_root = state.roms_dir.parent().unwrap_or(Path::new(".")).to_path_buf();

    let conflict = {
        let pending = state.pending_conflicts.lock().map_err(err)?;
        pending
            .iter()
            .find(|c| c.file_name == file_name)
            .cloned()
            .ok_or_else(|| format!("no pending conflict for {file_name}"))?
    };

    let outcome = savesync::resolve(
        &client,
        &conflict,
        keep,
        &ra.root,
        &library_root,
        Path::new("."),
    )
    .await
    .map_err(err)?;

    state
        .pending_conflicts
        .lock()
        .map_err(err)?
        .retain(|c| c.file_name != file_name);
    Ok(outcome)
}

#[derive(Serialize)]
struct ThemeView {
    name: String,
    reponame: String,
    author: String,
    url: String,
    variants: Vec<String>,
    screenshot: Option<String>,
    installed: bool,
    /// Bytes on disk, 0 when not installed.
    size_bytes: u64,
}

/// The official ES-DE themes list, annotated with what is already installed.
#[tauri::command]
async fn themes_available(state: State<'_, AppState>) -> CmdResult<Vec<ThemeView>> {
    let list = theme_remote::list_default().await.map_err(err)?;
    let dir = state.themes_dir.clone();
    Ok(list
        .into_iter()
        .map(|t| {
            let path = dir.join(t.dir_name());
            let installed = path.is_dir();
            ThemeView {
                screenshot: t.screenshot_url(),
                size_bytes: if installed { theme_remote::size_of(&path) } else { 0 },
                installed,
                reponame: t.dir_name(),
                name: t.name,
                author: t.author,
                url: t.url,
                variants: t.variants,
            }
        })
        .collect())
}

/// Download a theme. With `logos_only`, keep just the platform icons and
/// delete the checkout — themes run to hundreds of MB and we render ~240 KB.
#[tauri::command]
async fn theme_download(
    state: State<'_, AppState>,
    reponame: String,
    logos_only: bool,
) -> CmdResult<String> {
    let list = theme_remote::list_default().await.map_err(err)?;
    let entry = list
        .into_iter()
        .find(|t| t.dir_name() == reponame)
        .ok_or_else(|| format!("{reponame} is not in the themes list"))?;

    let dir = state.themes_dir.clone();
    // Reuses the API client's HTTP stack when there is one; a theme is a
    // public download, so an unconfigured server does not prevent it.
    let http = state
        .client
        .as_ref()
        .map(|c| c.http().clone())
        .unwrap_or(util::http_client(None).map_err(err)?);
    let (path, fresh) = theme_remote::install(&http, &entry, &dir).await.map_err(err)?;
    let size = theme_remote::size_of(&path);

    let slugs: Vec<String> = {
        let cache = state.cache.lock().map_err(err)?;
        cache.platforms().map_err(err)?.into_iter().map(|p| p.fs_slug).collect()
    };
    let one = vec![theme::Theme { name: entry.dir_name(), path: path.clone() }];
    let n = theme::install(&one, &state.map, &slugs, &state.media_dir).map_err(err)?;

    if logos_only {
        theme_remote::remove(&entry.dir_name(), &dir).map_err(err)?;
        return Ok(format!(
            "{}: kept {n} icons, freed {:.0} MB",
            entry.name,
            size as f64 / 1_048_576.0
        ));
    }
    Ok(format!(
        "{} {} ({:.0} MB), {n} icons applied",
        entry.name,
        if fresh { "downloaded" } else { "updated" },
        size as f64 / 1_048_576.0
    ))
}

#[tauri::command]
fn theme_remove(state: State<'_, AppState>, reponame: String) -> CmdResult<String> {
    theme_remote::remove(&reponame, &state.themes_dir).map_err(err)?;
    Ok(format!("removed {reponame}"))
}

#[derive(Serialize)]
struct EmulatorOption {
    core: String,
    label: String,
    installed: bool,
    /// True for the core ES-DE would pick by default.
    is_default: bool,
}

#[derive(Serialize)]
struct ShaderOptionView {
    path: String,
    label: String,
    note: String,
}

#[derive(Serialize)]
struct SystemView {
    slug: String,
    name: String,
    rom_count: i64,
    display: String,
    /// Currently selected core and shader, whether defaulted or chosen.
    core: Option<String>,
    shader: Option<String>,
    emulators: Vec<EmulatorOption>,
    shaders: Vec<ShaderOptionView>,
    /// What this console's light gun was called, when it had one. `None` means
    /// no switch is offered for this system.
    gun: Option<String>,
    gun_on: bool,
}

/// Per-system configuration, ES-DE style: every alternative emulator the theme
/// data knows about, plus the shader presets this RetroArch can load.
#[tauri::command]
fn systems(state: State<'_, AppState>) -> CmdResult<Vec<SystemView>> {
    let rows = {
        let cache = state.cache.lock().map_err(err)?;
        cache.platforms().map_err(err)?
    };
    let ra = state.retroarch.as_ref();

    Ok(rows
        .into_iter()
        .map(|p| {
            let slug = p.fs_slug;
            let default_core = state.map.default_core(&slug);

            // Alternatives come from the ES-DE extraction, so the list matches
            // what ES-DE itself offers for the system.
            let mut emulators: Vec<EmulatorOption> = state
                .map
                .alternatives(&slug)
                .into_iter()
                .map(|core| EmulatorOption {
                    label: state.map.label_for(core).unwrap_or(core).to_owned(),
                    installed: ra.is_some_and(|r| r.has_core(core)),
                    is_default: Some(core) == default_core,
                    core: core.to_owned(),
                })
                .collect();
            // Installed first, then ES-DE's own ordering.
            emulators.sort_by_key(|e| (!e.installed, !e.is_default));

            let display = shaders::display_of(&slug);
            let shader_list = ra
                .map(|r| {
                    shaders::available(r, display)
                        .into_iter()
                        .map(|o| ShaderOptionView {
                            path: o.path.to_owned(),
                            label: o.label.to_owned(),
                            note: o.note.to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default();

            SystemView {
                gun: romm_desktop::lightgun::label(&slug).map(str::to_owned),
                gun_on: state
                    .lightgun
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&slug)
                    .is_some_and(|v| v.trim() == "on"),
                core: resolve_core(&state, &slug),
                shader: if state.shaders_enabled {
                    shaders::preset_for(
                        &state.shader_overrides.lock().unwrap_or_else(|e| e.into_inner()),
                        &slug,
                    )
                } else {
                    None
                },
                display: match display {
                    shaders::Display::Crt => "CRT",
                    shaders::Display::Handheld => "Handheld",
                }
                .to_owned(),
                name: p.display_name,
                rom_count: p.rom_count,
                emulators,
                shaders: shader_list,
                slug,
            }
        })
        .collect())
}

/// Sync saves and save states with the server.
///
/// An explicit action rather than something that happens on launch: a save is
/// the only thing here that cannot be fetched again if it goes wrong, so
/// overwriting one should be a decision, not a side effect.
#[tauri::command]
async fn sync_saves(state: State<'_, AppState>) -> CmdResult<String> {
    let client = state.client.clone().ok_or("not connected to a server")?;
    let ra = state.retroarch.as_ref().ok_or("RetroArch not found")?;
    let root = ra.root.clone();

    // The cache is not Sync, so the scan takes the lock and releases it before
    // any awaiting starts. A future holding the connection across an await
    // cannot be spawned at all.
    let candidates = {
        let cache = state.cache.lock().map_err(err)?;
        romm_desktop::savesync::scan(&cache, &state.map, &root).map_err(err)?
    };

    let summary = romm_desktop::savesync::run_all(
        &client,
        &candidates,
        &root,
        Path::new("."),
        state.roms_dir.parent().unwrap_or(Path::new(".")),
    )
        .await
        .map_err(err)?;
    Ok(format!("{}\n{}", summary.headline(), summary.notes.join("\n")))
}

/// The motion (strobe/BFI) layer: what is installed, and what is selected.
///
/// Global rather than per-system: it depends on the display, not the console.
/// It chains *onto* whichever shader a system already uses rather than
/// replacing it, so picking one here leaves every per-system choice intact.
#[derive(Serialize)]
struct MotionView {
    current: Option<String>,
    options: Vec<ShaderOptionView>,
}

#[tauri::command]
fn motion_options(state: State<'_, AppState>) -> CmdResult<MotionView> {
    let installed: Vec<ShaderOptionView> = state
        .retroarch
        .as_ref()
        .map(|ra| {
            shaders::MOTION
                .iter()
                .filter(|o| shaders::resolve(ra, o.path).is_some())
                .map(|o| ShaderOptionView {
                    path: o.path.to_owned(),
                    label: o.label.to_owned(),
                    note: o.note.to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(MotionView {
        current: state.motion_shader.lock().map_err(err)?.clone(),
        options: installed,
    })
}

#[tauri::command]
fn set_motion_shader(state: State<'_, AppState>, value: String) -> CmdResult<String> {
    romm_desktop::config::set_table_entry("config.toml", "shaders", "motion", &value)
        .map_err(err)?;
    let chosen = (value != "none" && !value.is_empty()).then_some(value);
    *state.motion_shader.lock().map_err(err)? = chosen.clone();
    Ok(match chosen {
        Some(v) => format!("motion layer: {v}"),
        None => "motion layer off".to_owned(),
    })
}

/// Persist a per-system core or shader choice to config.toml.
///
/// Written through a TOML edit rather than a full re-serialise so comments and
/// hand-written sections survive.
#[tauri::command]
fn set_system_choice(
    state: State<'_, AppState>,
    slug: String,
    field: String,
    value: String,
) -> CmdResult<String> {
    let table = match field.as_str() {
        "core" => "cores.overrides",
        "shader" => "shaders.by_platform",
        "lightgun" => "lightgun.by_platform",
        other => return Err(format!("unknown field {other}")),
    };
    romm_desktop::config::set_table_entry("config.toml", table, &slug, &value).map_err(err)?;

    // Reflect it in the live copy too, so the next launch uses it without a
    // restart. config.toml remains authoritative on startup.
    match field.as_str() {
        "core" => {
            state.core_overrides.lock().map_err(err)?.insert(slug.clone(), value.clone());
        }
        "shader" => {
            state.shader_overrides.lock().map_err(err)?.insert(slug.clone(), value.clone());
        }
        "lightgun" => {
            state.lightgun.lock().map_err(err)?.insert(slug.clone(), value.clone());
        }
        _ => {}
    }
    Ok(format!("{slug}: {field} = {value}"))
}

/// Cores this game could run under, with the one in force marked.
///
/// Arcade is the reason this is per-game rather than per-platform: a mixed
/// romset has no single correct core, so individual games must be able to
/// escape the platform default.
#[derive(Serialize)]
struct CoreChoice {
    core: String,
    label: String,
    installed: bool,
    /// True for the core this game would launch with right now.
    current: bool,
    /// True when that is because of a per-game override rather than the
    /// platform default.
    pinned: bool,
}

#[tauri::command]
fn game_cores(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<CoreChoice>> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let current = resolve_core_for(&state, &row.platform_slug, Some(&row.fs_name));
    let pinned = state
        .core_per_game
        .lock()
        .map_err(err)?
        .contains_key(&romm_desktop::config::game_key(&row.platform_slug, &row.fs_name));

    let mut cores: Vec<String> = state
        .map
        .alternatives(&row.platform_slug)
        .into_iter()
        .map(str::to_owned)
        .collect();
    // The platform default and whatever is in force are always offered, even
    // if ES-DE never listed them for this system.
    for extra in [state.map.default_core(&row.platform_slug).map(str::to_owned), current.clone()]
        .into_iter()
        .flatten()
    {
        if !cores.contains(&extra) {
            cores.push(extra);
        }
    }

    Ok(cores
        .into_iter()
        .map(|core| CoreChoice {
            label: state.map.label_for(&core).unwrap_or(&core).to_owned(),
            installed: state.retroarch.as_ref().is_some_and(|ra| ra.has_core(&core)),
            current: current.as_deref() == Some(core.as_str()),
            pinned: pinned && current.as_deref() == Some(core.as_str()),
            core,
        })
        .collect())
}

/// Pin a core to one game, or clear the pin with an empty `core`.
#[tauri::command]
fn set_game_core(state: State<'_, AppState>, id: i64, core: String) -> CmdResult<String> {
    let row = {
        let cache = state.cache.lock().map_err(err)?;
        cache.rom_by_id(id).map_err(err)?
    }
    .ok_or_else(|| format!("no rom with id {id}"))?;

    let key = romm_desktop::config::game_key(&row.platform_slug, &row.fs_name);
    if core.is_empty() {
        romm_desktop::config::clear_table_entry("config.toml", "cores.per_game", &key)
            .map_err(err)?;
    } else {
        romm_desktop::config::set_table_entry("config.toml", "cores.per_game", &key, &core)
            .map_err(err)?;
    }

    let mut live = state.core_per_game.lock().map_err(err)?;
    if core.is_empty() {
        live.remove(&key);
        return Ok(format!("{}: back to the {} default", row.name, row.platform_slug));
    }
    live.insert(key, core.clone());
    Ok(format!("{}: pinned to {core}", row.name))
}

/// Copy system logos out of an installed ES-DE theme into the media tree.
#[tauri::command]
fn install_theme_logos(state: State<'_, AppState>) -> CmdResult<String> {
    let slugs: Vec<String> = {
        let cache = state.cache.lock().map_err(err)?;
        cache.platforms().map_err(err)?.into_iter().map(|p| p.fs_slug).collect()
    };
    let themes = theme::discover_with(state.theme_root.as_deref(), Some(&state.themes_dir));
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
    /// False when there is no config.toml at all — a different problem from a
    /// server that will not answer, and worth saying so in the UI.
    configured: bool,
    retroarch: Option<String>,
    cores_installed: usize,
    roms_cached: i64,
    /// Absolute paths, shown in the UI so downloaded data is never a mystery.
    roms_dir: String,
    media_dir: String,
    disk_bytes: u64,
    /// Directory every relative path is resolved against, and therefore where
    /// `config.toml` is read from. Reported because "it cannot find my config"
    /// is otherwise unanswerable from inside the app.
    data_dir: String,
    config_path: String,
    /// True when the app has no config.toml and is sitting in a directory that
    /// already holds other things. It is about to create a library folder, a
    /// cache and a config here, and dropping all that into someone's Downloads
    /// or Desktop is rude.
    crowded_folder: bool,
    /// What else is in there, for the warning to name.
    folder_entries: usize,
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
        configured: Config::exists("config.toml"),
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
        disk_bytes: util::dir_size(&state.roms_dir) + util::dir_size(&state.media_dir),
        data_dir: abs(Path::new(".")),
        config_path: abs(Path::new("config.toml")),
        crowded_folder: !Config::exists("config.toml") && neighbours() > 2,
        folder_entries: neighbours(),
    })
}

/// Point the app at a RetroArch install by hand.
///
/// The automatic search only knows conventional locations — `/Applications`,
/// `C:/Program Files/RetroArch` and so on — which is no help when the install
/// lives somewhere like `E:/Emulators/RetroArch`. The path is verified before
/// it is written, so a typo fails here rather than at the next launch.
///
/// `state.retroarch` is resolved once at startup and is not behind a lock — it
/// is read inside an async command, where holding one across an await would be
/// a hazard — so the new path applies on the next launch of the app rather than
/// immediately. The returned string says so.
#[tauri::command]
fn set_retroarch_root(path: String) -> CmdResult<String> {
    let path = path.trim().to_owned();

    if path.is_empty() {
        // Empty means "go back to probing the usual places".
        romm_desktop::config::clear_table_entry("config.toml", "retroarch", "root").map_err(err)?;
        return Ok("Cleared. The usual locations will be searched again after a restart.".into());
    }

    // Verify before writing, so a typo fails here rather than at the next launch.
    let found = RetroArch::locate(Some(&path)).map_err(|e| e.to_string())?;
    romm_desktop::config::set_table_entry("config.toml", "retroarch", "root", &path)
        .map_err(err)?;

    Ok(format!(
        "Found {} with {} cores. Restart to use it.",
        found.binary.display(),
        found.installed_cores().len()
    ))
}

fn current_style(state: &State<'_, AppState>) -> theme::IconStyle {
    let i = state.icon_style.load(Ordering::Relaxed) as usize;
    theme::IconStyle::ALL[i.min(theme::IconStyle::ALL.len() - 1)]
}

#[derive(Serialize)]
struct IconStyleView {
    key: String,
    label: String,
    /// How many of our platforms have art in this style.
    available: usize,
    selected: bool,
}

/// The per-system art styles ES-DE themes provide, with coverage counts.
#[tauri::command]
fn icon_styles(state: State<'_, AppState>) -> CmdResult<Vec<IconStyleView>> {
    let slugs: Vec<String> = {
        let cache = state.cache.lock().map_err(err)?;
        cache.platforms().map_err(err)?.into_iter().map(|p| p.fs_slug).collect()
    };
    let cur = current_style(&state);
    Ok(theme::installed_counts(&state.media_dir, &slugs)
        .into_iter()
        .map(|(style, available)| IconStyleView {
            key: style.key().to_owned(),
            label: style.label().to_owned(),
            available,
            selected: style == cur,
        })
        .collect())
}

#[tauri::command]
fn set_icon_style(state: State<'_, AppState>, key: String) -> CmdResult<String> {
    let style = theme::IconStyle::parse(&key).ok_or_else(|| format!("unknown style {key}"))?;
    let idx = theme::IconStyle::ALL.iter().position(|s| *s == style).unwrap_or(0);
    state.icon_style.store(idx as u8, Ordering::Relaxed);
    // Persist it, or the grid silently reverts to wordmarks on next launch.
    romm_desktop::config::set_table_entry("config.toml", "icons", "style", style.key())
        .map_err(err)?;
    Ok(style.label().to_owned())
}

/// How many things sit alongside the app, ignoring what the app itself puts
/// there.
///
/// The app's own executable does not count, and neither does anything it
/// created — a second run should not report the folder as crowded because of
/// the library it made on the first.
fn neighbours() -> usize {
    const OURS: &[&str] = &[
        "library", "cache.sqlite3", "config.toml", "data", "state.json",
        "states-seen.json", "crash.log", "saves-backup",
    ];
    let Ok(entries) = std::fs::read_dir(".") else { return 0 };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || OURS.contains(&name.as_str()) {
                return false;
            }
            // The app itself, under any of the names it ships as.
            !(name.ends_with(".exe") && name.contains("romm"))
                && !name.starts_with("romm-desktop")
                && !name.starts_with("romm-gui")
                && !name.starts_with("romm-cli")
                && !name.ends_with(".app")
        })
        .count()
}

fn abs(p: &Path) -> String {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).display().to_string()
}


// --- helpers -------------------------------------------------------------

fn local_path(state: &State<'_, AppState>, platform: &str, fs_name: &str) -> Option<PathBuf> {
    let p = state.roms_dir.join(platform).join(fs_name);
    p.is_file().then_some(p)
}

/// Where a row's file actually is.
///
/// A locally scanned ES-DE game records its own absolute path — it lives on
/// whatever disk the library is on, which `<roms_dir>/<slug>/<file>` cannot
/// express.
fn row_path(state: &State<'_, AppState>, row: &cache::RomRow) -> Option<PathBuf> {
    if let Some(p) = row.local_path.as_deref().map(PathBuf::from)
        && (p.is_file() || p.is_dir())
    {
        return Some(p);
    }
    local_path(state, &row.platform_slug, &row.fs_name)
}

/// Artwork directory and key to look under.
///
/// ES-DE keys media by its own system name (`genesis`, `neogeo`), while the
/// downloaded RomM tree keys by slug — so the pair must travel together.
fn media_scope<'a>(state: &'a State<'_, AppState>, row: &'a cache::RomRow) -> (&'a Path, &'a str) {
    match (state.esde_media.as_deref(), row.esde_system.as_deref()) {
        (Some(dir), Some(system)) => (dir, system),
        _ => (state.media_dir.as_path(), row.platform_slug.as_str()),
    }
}

fn resolve_core(state: &State<'_, AppState>, platform: &str) -> Option<String> {
    resolve_core_for(state, platform, None)
}

/// As [`resolve_core`], honouring a per-game override when the file is known.
fn resolve_core_for(
    state: &State<'_, AppState>,
    platform: &str,
    fs_name: Option<&str>,
) -> Option<String> {
    let ra = state.retroarch.as_ref()?;
    let overrides = state.core_overrides.lock().ok()?;
    let per_game = state.core_per_game.lock().ok()?;
    coremap::resolve_core_for(&state.map, &overrides, &per_game, platform, fs_name, |c| {
        ra.has_core(c)
    })
}

/// Find the data root and make it the working directory.
///
/// Config, cache, core map and library are all addressed relative to one
/// directory, but where that is depends on how the app was started:
///
/// * a config.toml the user put beside the executable, or in the cwd
/// * a dev checkout — the repo itself, found by walking up for the core map
/// * otherwise the executable's own directory
///
/// Nothing is created and nothing is written to the home directory. See
/// [`choose_data_root`], which holds the ordering and is where the tests are.
fn anchor_to_data_root() {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe().ok();
    match choose_data_root(cwd.as_deref(), exe.as_deref(), &|p| p.is_file()) {
        Some(root) => {
            let _ = std::env::set_current_dir(&root);
        }
        None => eprintln!(
            "warning: could not locate the executable; leaving the working directory alone"
        ),
    }
}

const MARKER: &str = "data/esde-core-map.json";
const CONFIG: &str = "config.toml";

/// Decide the data root. Split out from [`anchor_to_data_root`] because the
/// *order* is the part that was wrong, and a function that calls
/// `set_current_dir` cannot be tested — the working directory is per-process,
/// so tests would fight each other.
///
/// `is_file` is injected for the same reason: the interesting cases are layouts
/// nobody has on disk (a Windows portable install, a `.app` in /Applications).
fn choose_data_root(
    cwd: Option<&Path>,
    exe: Option<&Path>,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let exe_dir = exe.and_then(app_dir);

    // 1. A config.toml beside the executable, or in the working directory.
    //
    // This is what a portable install looks like and what everyone expects of
    // one: unzip the exe, drop config.toml next to it, run it. Without this the
    // Windows build ignored that file completely — it anchored to %USERPROFILE%
    // \RomM and looked for the config there, so a config sitting right beside
    // the exe was never on any path it consulted.
    //
    // Checked before the marker search because it is the more specific signal:
    // a config.toml is somewhere a user deliberately put a file, whereas the
    // marker only says "a checkout is somewhere above us".
    for dir in [cwd, exe_dir.as_deref()].into_iter().flatten() {
        if is_file(&dir.join(CONFIG)) {
            return Some(dir.to_path_buf());
        }
    }

    // 2. A source checkout, if we are running from one.
    let ancestors = cwd
        .into_iter()
        .flat_map(Path::ancestors)
        .chain(exe.into_iter().flat_map(Path::ancestors));
    for root in ancestors {
        if is_file(&root.join(MARKER)) {
            return Some(root.to_path_buf());
        }
    }

    // 3. The executable's own directory, and nowhere else.
    //
    // The app lives where it was put. It does not create a folder in the home
    // directory, and nothing is written from here at all — the core map is
    // compiled into the binary (`CoreMap::load_or_embedded`), so no file has to
    // exist on disk before startup.
    exe_dir
}

/// The directory a user would say the app is "in".
///
/// On macOS the executable is buried at `RomM-Desktop.app/Contents/MacOS/`,
/// which is inside the signed bundle: writing there breaks the signature and is
/// wiped on update. The directory holding the `.app` is the equivalent of the
/// folder a loose `.exe` sits in, so that is what gets used.
fn app_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let mut cur = dir;
    while let Some(parent) = cur.parent() {
        if cur.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    Some(dir.to_path_buf())
}

/// Write panics somewhere findable before the process dies.
///
/// With no console attached, a panic in a release build produces no output at
/// all. This is the difference between "it does nothing when I double-click"
/// and a file naming the line that failed.
fn install_panic_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Beside the app, in the data directory `anchor_to_data_root` chose —
        // not in the home directory. Nothing is created: if that location is
        // not writable the panic still reaches stderr, which is no worse than
        // before and does not leave a folder behind.
        let path = PathBuf::from("crash.log");
        if std::fs::write(&path, format!("{info}\n")).is_ok() {
            let shown = path.canonicalize().unwrap_or(path);
            eprintln!("panic written to {}", shown.display());
        }
        previous(info);
    }));
}

fn main() {
    install_panic_log();
    anchor_to_data_root();
    let cfg = Config::load().unwrap_or_default();
    let store = cache::Cache::open(Path::new(CACHE_DB)).expect("opening metadata cache");
    // Archive verification depends on the server's exclusion lists; load the
    // cached copy before anything can download.
    romm_desktop::apply_cached_server_config(&store);
    // Never `expect` here. A release build has no console (see
    // `windows_subsystem` at the top of this file), so a panic at startup is
    // completely silent: the icon bounces and nothing happens, with no way to
    // tell whether the app crashed or never ran.
    let map = CoreMap::load_or_embedded(Path::new(CORE_MAP));
    let client = cfg.server.client()
        .ok()
        .map(Arc::new);
    let retroarch = RetroArch::locate_in(&cfg.retroarch.ordered_paths())
        .ok()
        .map(|ra| ra.with_system_dir(Some(cfg.system_dir())));
    let roms_dir = cfg.local_roms_dir();
    let media_dir = PathBuf::from(&cfg.library.local_root).join("downloaded_media");

    // Artwork now comes from ES-DE alone. Anything fetched from RomM before
    // that goes, once, or the art chain would keep finding it and only the
    // games nobody had browsed yet would look consistent.
    match romm_desktop::media::drop_romm_covers(&media_dir) {
        0 => {}
        n => eprintln!("cleared {n} cover(s) fetched from RomM; artwork now comes from ES-DE"),
    }

    tauri::Builder::default()
        // Native folder picker for the RetroArch location setting.
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            cache: Mutex::new(store),
            map,
            client,
            retroarch,
            roms_dir,
            media_dir,
            esde_media: cfg.esde.media_dir(),
            theme_root: cfg.theme.root.clone(),
            themes_dir: cfg.themes_dir(),
            core_overrides: Mutex::new(cfg.cores.overrides.clone()),
            core_per_game: Mutex::new(cfg.cores.per_game.clone()),
            user_retroarch_cfg: cfg.user_retroarch_config(),
            shaders_enabled: cfg.shaders.enabled,
            shader_overrides: Mutex::new(cfg.shaders.by_platform.clone()),
            lightgun: Mutex::new(cfg.lightgun.by_platform.clone()),
            list_art: Mutex::new(cfg.media.list_art.clone()),
            detail_art: cfg.media.detail_art.clone(),
            motion_shader: Mutex::new(cfg.shaders.motion.clone()),
            // From config, not hardcoded: index 0 is `logo`, which is ES-DE's
            // wordmark art — a picture of the system's name. The grid wants
            // hardware, and the user's choice has to survive a restart.
            icon_style: AtomicU8::new(
                theme::IconStyle::parse(&cfg.icons.style)
                    .and_then(|s| theme::IconStyle::ALL.iter().position(|x| *x == s))
                    .unwrap_or(3) as u8,
            ),
            achievements: cfg.achievements.settings(),
            auto_sync: cfg.saves.auto_sync,
            pending_conflicts: Mutex::new(Vec::new()),
        })
        .invoke_handler(tauri::generate_handler![
            download_set,
            recent_games,
            download_estimate,
            scrape_missing,
            list_art_options,
            set_list_art,
            game_video,
            versions,
            platforms,
            roms,
            search,
            collection_groups,
            collections_in,
            collection_roms,
            rom_detail,
            download_rom,
            rom_covers,
            launch_rom,
            install_theme_logos,
            themes_available,
            theme_download,
            theme_remove,
            icon_styles,
            set_icon_style,
            set_retroarch_root,
            systems,
            sync_saves,
            sync_library,
            sync_bios,
            resolve_save_conflict,
            motion_options,
            set_motion_shader,
            set_system_choice,
            game_cores,
            set_game_core,
            status,
            open_settings,
            config_fields,
            set_config_field,
            verify_server
        ])
        .run(tauri::generate_context!())
        .expect("running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On macOS the executable is buried inside the signed bundle. Writing
    /// there breaks the signature and is wiped on update, so the directory
    /// holding the `.app` is the app's location as far as data goes — the
    /// equivalent of the folder a loose .exe sits in.
    #[test]
    fn a_macos_bundle_resolves_to_the_directory_holding_it() {
        assert_eq!(
            app_dir(Path::new("/Applications/RomM-Desktop.app/Contents/MacOS/romm-gui")),
            Some(PathBuf::from("/Applications"))
        );
        assert_eq!(
            app_dir(Path::new(
                "/Users/frank/Projects/romm-desktop/RomM-Desktop.app/Contents/MacOS/romm-gui"
            )),
            Some(PathBuf::from("/Users/frank/Projects/romm-desktop"))
        );
    }

    /// A loose executable — the Windows and Linux shape — anchors to its own
    /// directory. Unzip it anywhere, drop a config.toml beside it, run it.
    #[test]
    fn a_loose_executable_anchors_beside_itself() {
        assert_eq!(
            app_dir(Path::new("D:/Emulators/RomM/romm-desktop.exe")),
            Some(PathBuf::from("D:/Emulators/RomM"))
        );
        assert_eq!(
            app_dir(Path::new("/opt/romm/romm-desktop")),
            Some(PathBuf::from("/opt/romm"))
        );
    }

    /// A directory merely *containing* the string "app" is not a bundle; only a
    /// `.app` extension counts, or an install under /home/apps/ would anchor a
    /// level too high.
    #[test]
    fn only_a_dot_app_extension_counts_as_a_bundle() {
        assert_eq!(
            app_dir(Path::new("/home/frank/apps/romm-desktop")),
            Some(PathBuf::from("/home/frank/apps"))
        );
        assert_eq!(
            app_dir(Path::new("/srv/appdata/romm/romm-desktop")),
            Some(PathBuf::from("/srv/appdata/romm"))
        );
    }

    /// A set of paths that "exist", for driving `choose_data_root` over layouts
    /// nobody has on this disk.
    fn exists(paths: &[&str]) -> impl Fn(&Path) -> bool + use<> {
        let owned: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
        move |p: &Path| owned.iter().any(|o| Path::new(o) == p)
    }

    /// The reported bug, as a test: a portable Windows install with its config
    /// beside the exe, launched from a working directory that has nothing to do
    /// with it. This used to land on %USERPROFILE%\RomM, where the config was
    /// never on any path the app consulted.
    ///
    /// The checkout marker above the working directory is what gives this test
    /// teeth. Without it the answer is right either way — step 3 also returns
    /// the executable's directory — so the ordering would not actually be under
    /// test. A mutation run (delete the config check, watch this still pass)
    /// is what surfaced that.
    #[test]
    fn a_config_beside_the_executable_wins() {
        let root = choose_data_root(
            Some(Path::new("C:/Users/frank/checkout/sub")),
            Some(Path::new("D:/Emulators/RomM/romm-desktop.exe")),
            &exists(&[
                "D:/Emulators/RomM/config.toml",
                // A checkout above the cwd, which wins if the config is
                // consulted second instead of first.
                "C:/Users/frank/checkout/data/esde-core-map.json",
            ]),
        );
        assert_eq!(root, Some(PathBuf::from("D:/Emulators/RomM")));
    }

    /// The working directory is checked before the executable, so running from
    /// a configured folder uses that config rather than the app's own.
    #[test]
    fn the_working_directory_is_preferred_over_the_executables() {
        let root = choose_data_root(
            Some(Path::new("/srv/romm-live")),
            Some(Path::new("/opt/romm/romm-desktop")),
            &exists(&["/srv/romm-live/config.toml", "/opt/romm/config.toml"]),
        );
        assert_eq!(root, Some(PathBuf::from("/srv/romm-live")));
    }

    /// A config.toml is a deliberate act; the core map only says a checkout is
    /// somewhere above us. The specific signal has to win, or a developer with
    /// a checkout above their portable install gets the wrong data directory.
    #[test]
    fn a_config_beats_a_checkout_found_further_up() {
        let root = choose_data_root(
            Some(Path::new("/home/frank/Projects/romm-desktop/portable")),
            Some(Path::new("/home/frank/Projects/romm-desktop/portable/romm-desktop")),
            &exists(&[
                "/home/frank/Projects/romm-desktop/portable/config.toml",
                "/home/frank/Projects/romm-desktop/data/esde-core-map.json",
            ]),
        );
        assert_eq!(
            root,
            Some(PathBuf::from("/home/frank/Projects/romm-desktop/portable"))
        );
    }

    /// With no config anywhere, a checkout above the working directory is used
    /// — this is what makes `cargo run` from a subdirectory work.
    #[test]
    fn a_checkout_is_found_by_walking_up() {
        let root = choose_data_root(
            Some(Path::new("/home/frank/Projects/romm-desktop/src-tauri")),
            Some(Path::new("/home/frank/Projects/romm-desktop/target/debug/romm-gui")),
            &exists(&["/home/frank/Projects/romm-desktop/data/esde-core-map.json"]),
        );
        assert_eq!(root, Some(PathBuf::from("/home/frank/Projects/romm-desktop")));
    }

    /// Nothing configured and no checkout: the app lives where it was put. The
    /// previous behaviour — inventing ~/RomM and writing a core map into it —
    /// is what this asserts is gone.
    #[test]
    fn with_nothing_to_go_on_it_anchors_beside_the_app_not_in_home() {
        let root = choose_data_root(
            Some(Path::new("/")),
            Some(Path::new("/Applications/RomM-Desktop.app/Contents/MacOS/romm-gui")),
            &exists(&[]),
        );
        assert_eq!(
            root,
            Some(PathBuf::from("/Applications")),
            "the bundle resolves to the folder holding it, and never to $HOME"
        );
        assert!(
            !format!("{root:?}").contains("RomM/"),
            "no invented data folder"
        );
    }

    /// Without an executable path there is nothing to anchor to, and leaving the
    /// working directory alone beats guessing.
    #[test]
    fn no_executable_and_no_markers_changes_nothing() {
        assert_eq!(choose_data_root(Some(Path::new("/tmp")), None, &exists(&[])), None);
    }
}
