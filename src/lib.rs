//! Shared backend for the CLI/TUI (`src/main.rs`) and the Tauri GUI
//! (`src-tauri/`). Everything the GUI needs already lives here — the GUI adds
//! a window, not a second implementation.

pub mod api;
pub mod cache;
pub mod config;
pub mod coremap;
pub mod cores;
pub mod download;
pub mod media;
pub mod parity;
pub mod retroarch;
pub mod savehash;
pub mod theme;
pub mod saves;
pub mod tui;
