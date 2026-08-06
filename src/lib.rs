//! Shared backend for the CLI/TUI (`src/main.rs`) and the Tauri GUI
//! (`src-tauri/`). Everything the GUI needs already lives here — the GUI adds
//! a window, not a second implementation.

pub mod api;
pub mod arcade;
pub mod cache;
pub mod cheevos;
pub mod config;
pub mod coremap;
pub mod cores;
pub mod download;
pub mod esde;
pub mod launch;
pub mod media;
pub mod padprofile;
pub mod parity;
pub mod probe;
pub mod retroarch;
pub mod retroarch_install;
pub mod savehash;
pub mod shaders;
pub mod slangp;
pub mod theme;
pub mod tweaks;
pub mod theme_remote;
pub mod saves;
pub mod savesync;
pub mod tui;
pub mod util;

/// RomM release this client's server-specific behaviour was verified against.
///
/// Archive hashing, param names and the `/api/config` shape were all read out
/// of this version. A different server may still work, but the mismatch is
/// worth surfacing rather than debugging from first principles again.
pub const VERIFIED_AGAINST: &str = "5.0.0";

/// Load cached server settings into the modules that need them.
///
/// Called at startup by every frontend so archive verification behaves the
/// same in the CLI, TUI and GUI — and offline, where `/api/config` is
/// unreachable but the last-known values are still correct.
pub fn apply_cached_server_config(store: &cache::Cache) {
    if let Some((files, exts)) = store.server_exclusions() {
        download::set_exclusions(files, exts);
    }
}
