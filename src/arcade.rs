//! Real titles for arcade romsets.
//!
//! RomM names a ROM from its metadata match, and falls back to the filename
//! when nothing matches. For arcade that fallback is the romset short name, so
//! the library shows `kof98`, `samsho4`, `tophuntr` — 63 of 152 Neo Geo games
//! and 345 of 2,413 arcade ones.
//!
//! The DATs already downloaded for core-coverage analysis carry a
//! `<description>` per romset, which is the real title. `tools/arcade_names.py`
//! flattens FBNeo's and MAME's into `data/arcade-names.json`, and this applies
//! it wherever RomM left a bare romset name behind.

use std::collections::BTreeMap;
use std::path::Path;

/// Platforms whose filenames are romset short names.
pub const ARCADE_PLATFORMS: &[&str] = &["arcade", "mame", "neogeoaes"];

/// Load the romset → title map. Absent file is not an error: the names simply
/// stay as they are.
pub fn names(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// True when RomM clearly had no metadata and fell back to the file name.
pub fn is_bare_romset(name: &str, fs_name: &str) -> bool {
    let stem = fs_name.rsplit_once('.').map_or(fs_name, |(s, _)| s);
    name.is_empty() || name.eq_ignore_ascii_case(stem)
}
