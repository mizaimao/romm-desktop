//! `config.toml` — see `config.example.toml` for the documented template.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: Server,
    #[serde(default)]
    pub library: Library,
    #[serde(default)]
    pub retroarch: RetroArchCfg,
    #[serde(default)]
    pub saves: SavesCfg,
    #[serde(default)]
    pub controllers: ControllersCfg,
    #[serde(default)]
    pub theme: ThemeCfg,
    #[serde(default)]
    pub cores: CoresCfg,
    #[serde(default)]
    pub shaders: ShadersCfg,
    #[serde(default)]
    pub lightgun: LightgunCfg,
    #[serde(default)]
    pub media: MediaCfg,
    #[serde(default)]
    pub icons: IconsCfg,
    #[serde(default)]
    pub esde: EsdeCfg,
    /// `[achievements]`, with `[cheevos]` still accepted: that is RetroArch's
    /// own name for the feature and what every key it writes is called, so an
    /// existing config keeps working.
    #[serde(default, alias = "cheevos")]
    pub achievements: AchievementsCfg,
}

/// RetroAchievements — the `[achievements]` section of config.toml.
///
/// See [`crate::achievements`] for why a username is required alongside the
/// credential, and why hardcore mode is written explicitly every launch.
#[derive(Debug, Default, Deserialize)]
pub struct AchievementsCfg {
    #[serde(default)]
    pub enabled: bool,
    /// RetroAchievements account name. The token alone authenticates nothing.
    #[serde(default)]
    pub username: Option<String>,
    /// Login token — what RetroArch stores after a successful login, and what
    /// it prefers thereafter. Either this or `password`.
    #[serde(default)]
    pub token: Option<String>,
    /// Account password, exchanged for a token by RetroArch on first use.
    #[serde(default)]
    pub password: Option<String>,
    /// Disables save states, fast-forward and rewind — four of the gamepad
    /// hotkeys this app binds.
    #[serde(default)]
    pub hardcore: bool,
    #[serde(default)]
    pub test_unofficial: bool,
}

impl AchievementsCfg {
    pub fn settings(&self) -> crate::achievements::Settings {
        crate::achievements::Settings {
            enabled: self.enabled,
            username: self.username.clone(),
            token: self.token.clone(),
            password: self.password.clone(),
            hardcore: self.hardcore,
            test_unofficial: self.test_unofficial,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Server {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// A RomM client token, used in preference to username/password.
    ///
    /// Better than Basic for a device that keeps credentials on disk: it
    /// carries only the scopes this client needs, and can be revoked from the
    /// server without touching the account password.
    #[serde(default)]
    pub token: Option<String>,
}

impl Server {
    /// Build a client from whichever credential is configured.
    pub fn client(&self) -> anyhow::Result<crate::api::Client> {
        crate::api::Client::with_auth(
            &self.url,
            &self.username,
            &self.password,
            self.token.as_deref(),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct Library {
    #[serde(default = "default_local_root")]
    pub local_root: String,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            local_root: default_local_root(),
        }
    }
}

fn default_local_root() -> String {
    "./library".to_owned()
}

/// Controllers beyond the first.
#[derive(Debug, Deserialize)]
pub struct ControllersCfg {
    /// Bind players 2-4 the same way as player 1.
    ///
    /// On by default. RetroArch binds each pad from its own autoconfig profile
    /// and gives a controller it has no profile for nothing at all, which looks
    /// like a dead port rather than a missing file. Copying player one is right
    /// whenever the other pads are the same model — the usual case — and wrong
    /// for a genuinely different device, which is why it can be turned off.
    #[serde(default = "yes")]
    pub mirror_player_one: bool,
}

impl Default for ControllersCfg {
    fn default() -> Self {
        Self { mirror_player_one: true }
    }
}

#[derive(Debug, Deserialize)]
pub struct SavesCfg {
    /// Directory containing `saves/` and `states/` subdirectories. In a
    /// portable RetroArch install this is the portable root itself.
    #[serde(default = "default_saves_root")]
    pub root: String,

    /// Pull before a game starts and push after it exits, Steam-cloud style.
    ///
    /// On by default now that every overwrite is backed up first — see
    /// [`crate::savebackup`]. Without those backups this would be the one
    /// setting in the app that can destroy something unrecoverable, which is
    /// why it did not exist until they did.
    ///
    /// A conflict is still never resolved automatically: both sides are left
    /// as they are and reported.
    #[serde(default = "yes")]
    pub auto_sync: bool,

    /// Ask before deleting a save state from the shelf.
    ///
    /// Off by default, which is the unusual choice for a destructive action and
    /// is deliberate. Deleting states is housekeeping done several at a time —
    /// a shelf collects a dozen of them — and a dialog for each turns a tidy-up
    /// into a chore, which is how people end up not tidying up at all. The file
    /// is not gone either way: it goes to the same backup folder an overwritten
    /// save does, so the undo exists whether or not the question was asked.
    #[serde(default)]
    pub confirm_delete_state: bool,
}

impl Default for SavesCfg {
    fn default() -> Self {
        Self { root: default_saves_root(), auto_sync: true, confirm_delete_state: false }
    }
}

fn default_saves_root() -> String {
    "./Saves".to_owned()
}

#[derive(Debug, Default, Deserialize)]
pub struct CoresCfg {
    /// Platform slug -> libretro core stem, overriding the ES-DE default.
    ///
    /// Needed when a collection's ROMs do not match what the default core
    /// expects — arcade romsets in particular are version-locked, and a
    /// MAME 2003-Plus set will not run under current MAME.
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,

    /// `"<platform>/<fs_name>"` -> core, for the one game that needs a
    /// different core from the rest of its platform.
    ///
    /// Keyed by path rather than ROM id: ids are reassigned when the server
    /// rescans, and a rebuilt library would silently point these at the wrong
    /// games. The path is also readable, so this table can be hand-edited.
    #[serde(default)]
    pub per_game: BTreeMap<String, String>,
}

/// Config key for a single game's core override.
pub fn game_key(platform: &str, fs_name: &str) -> String {
    format!("{platform}/{fs_name}")
}

/// A local ES-DE library, used instead of a RomM server.
///
/// When `root` is set the app indexes that tree directly: nothing is
/// downloaded, because the ROMs and artwork are already on disk.
#[derive(Debug, Default, Deserialize)]
pub struct EsdeCfg {
    /// ES-DE data directory — the one holding `gamelists/` and
    /// `downloaded_media/`.
    #[serde(default)]
    pub root: Option<String>,
    /// ROMs directory, if it is not `<root>/ROMs`. ES-DE keeps this separate
    /// and configurable, so it usually is.
    #[serde(default)]
    pub roms: Option<String>,
}

impl EsdeCfg {
    /// Artwork root of the ES-DE library, if one is configured.
    pub fn media_dir(&self) -> Option<PathBuf> {
        self.layout().map(|l| l.media).filter(|p| p.is_dir())
    }

    pub fn layout(&self) -> Option<crate::esde::Layout> {
        let root = crate::util::expand_tilde(self.root.as_deref()?);
        let roms = self.roms.as_deref().map(crate::util::expand_tilde);
        Some(crate::esde::Layout::new(&root, roms.as_deref()))
    }
}

/// Which per-system artwork the platform grid shows.
#[derive(Debug, Deserialize)]
pub struct IconsCfg {
    /// One of `logo`, `consolegame`, `controller`, `systemart`,
    /// `systemart_legacy`.
    ///
    /// Defaults to hardware renders, not `logo`. ES-DE's "logo" art is the
    /// system's *name* set as a wordmark — it is a picture of text, which is
    /// exactly what the grid is supposed to avoid.
    #[serde(default = "default_icon_style")]
    pub style: String,
}

fn default_icon_style() -> String {
    "systemart".to_owned()
}

impl Default for IconsCfg {
    fn default() -> Self {
        Self { style: default_icon_style() }
    }
}

#[derive(Debug, Deserialize)]
pub struct ShadersCfg {
    /// Master switch. Off means no shader is applied and RetroArch's own
    /// setting is left alone.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Platform slug -> preset path under `shaders_slang/` (no extension), or
    /// `"none"` to force no shader for that platform.
    #[serde(default)]
    pub by_platform: BTreeMap<String, String>,
    /// Strobe/BFI pass chained on top of the platform shader for CRT
    /// consoles, e.g. `"subframe-bfi/adaptive_strobe-koko"`. Off by default:
    /// how well it works depends on the display, so it is a deliberate choice
    /// rather than something that should surprise anyone.
    #[serde(default)]
    pub motion: Option<String>,
}

impl Default for ShadersCfg {
    fn default() -> Self {
        Self { enabled: true, by_platform: BTreeMap::new(), motion: None }
    }
}

/// Which ES-DE artwork the interface shows where.
#[derive(Debug, Deserialize)]
pub struct MediaCfg {
    /// The image on each game in the list or grid. Cartridge art by default:
    /// it is the thing you recognise a game by when you owned it, and within a
    /// console every cartridge is the same shape, so the grid stays even.
    #[serde(default = "cart_art")]
    pub list_art: String,
    /// The image in the info pane. The miximage, which already combines box,
    /// screenshot and logo, so the pane says everything at once.
    #[serde(default = "mix_art")]
    pub detail_art: String,
}

fn cart_art() -> String {
    "physicalmedia".to_owned()
}

fn mix_art() -> String {
    "miximages".to_owned()
}

impl Default for MediaCfg {
    fn default() -> Self {
        Self { list_art: cart_art(), detail_art: mix_art() }
    }
}

/// Systems switched over to a light gun.
///
/// Its own section rather than a flag on each platform because on most of
/// these consoles the gun takes the port a second pad would use, so leaving it
/// on breaks two-player games. Keeping it in one visible list makes it obvious
/// what has been switched.
#[derive(Debug, Default, Deserialize)]
pub struct LightgunCfg {
    /// Platform slug -> `"on"`. Anything else, or absent, means off.
    #[serde(default)]
    pub by_platform: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ThemeCfg {
    /// Extra directory to search for ES-DE themes, checked before the
    /// standard locations. Unset means probe ~/ES-DE/themes and the ES-DE.app
    /// bundle.
    pub root: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RetroArchCfg {
    /// Single install. Kept for older configs; `installs` supersedes it.
    pub root: Option<String>,

    /// Ordered list of installs, tried top to bottom like a boot order. The
    /// first enabled entry that actually contains RetroArch wins, so a
    /// portable build can shadow a system one without deleting either.
    #[serde(default)]
    pub installs: Vec<RetroArchInstall>,

    /// Extra RetroArch settings appended at launch, on top of ours.
    ///
    /// Path to a file in RetroArch's own `key = "value"` format. Anything here
    /// wins, so button maps and video filters can be pinned without ever
    /// opening RetroArch's menu.
    pub user_config: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetroArchInstall {
    /// Directory containing `RetroArch.app` (macOS) or `retroarch.exe`.
    pub path: String,
    /// Shown in Settings; defaults to the path when absent.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl RetroArchCfg {
    /// Install paths to try, in order, honouring `enabled`.
    ///
    /// Falls back to the legacy single `root` so an existing config keeps
    /// working untouched.
    pub fn ordered_paths(&self) -> Vec<String> {
        if !self.installs.is_empty() {
            return self
                .installs
                .iter()
                .filter(|i| i.enabled)
                .map(|i| i.path.clone())
                .collect();
        }
        self.root.clone().into_iter().collect()
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new("config.toml"))
    }

    /// True when `load` fell back to defaults because no file was there.
    ///
    /// A missing config used to be indistinguishable from a configured one
    /// pointing at a dead server: both produce an unreachable client and an
    /// empty library, which reads as "the app is broken" rather than "nothing
    /// has been set up yet". Callers use this to say which it is.
    pub fn exists(path: &str) -> bool {
        Path::new(path).is_file()
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.is_file() {
            // Absent config is fine for commands that don't touch the server.
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn local_roms_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("roms")
    }

    pub fn media_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("downloaded_media")
    }

    /// The user's own RetroArch settings file, appended at launch.
    pub fn user_retroarch_config(&self) -> PathBuf {
        match &self.retroarch.user_config {
            Some(p) => crate::util::expand_tilde(p),
            None => PathBuf::from(&self.library.local_root).join("retroarch-user.cfg"),
        }
    }

    /// The BIOS folder synced from the server, used as RetroArch's system
    /// directory when it exists. Inside `library/` so one visible folder still
    /// holds everything this app downloads.
    pub fn system_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("system")
    }

    /// Where downloaded ES-DE themes go. Inside the library folder so the
    /// "delete one folder to reclaim everything" property holds.
    pub fn themes_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("themes")
    }
}


/// Set `key = "value"` inside `[table]` in a TOML file, creating the table if
/// needed.
///
/// A targeted text edit rather than parse-and-reserialise: the config carries
/// hand-written comments explaining non-obvious choices (which arcade core and
/// why, what the shader groups mean), and round-tripping through a serialiser
/// would delete all of them.
pub fn set_table_entry(path: &str, table: &str, key: &str, value: &str) -> Result<()> {
    write_entry(path, table, key, Some(value))
}

/// Set a boolean. TOML booleans are bare literals, so they cannot go through
/// the quoted-string writer — `enabled = "true"` is a string and parses as one,
/// which then fails to deserialise into a bool.
pub fn set_table_bool(path: &str, table: &str, key: &str, value: bool) -> Result<()> {
    write_raw(path, table, key, Some(if value { "true" } else { "false" }))
}

/// Remove `key` from `[table]` if present.
pub fn clear_table_entry(path: &str, table: &str, key: &str) -> Result<()> {
    write_entry(path, table, key, None)
}

/// TOML bare keys allow only letters, digits, `_` and `-`. Per-game keys are
/// file paths, so they have to be quoted and escaped.
fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return key.to_owned();
    }
    format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Does this line define `key`, in either its bare or quoted form?
fn defines_key(line: &str, key: &str) -> bool {
    let Some(lhs) = line.split('=').next() else {
        return false;
    };
    let lhs = lhs.trim();
    lhs == key || lhs == toml_key(key)
}

fn write_entry(path: &str, table: &str, key: &str, value: Option<&str>) -> Result<()> {
    let quoted = value.map(|v| {
        format!(
            "{} = \"{}\"",
            toml_key(key),
            v.replace('\\', "\\\\").replace('"', "\\\"")
        )
    });
    write_line(path, table, key, quoted)
}

/// As [`write_entry`], with the right-hand side written verbatim — for values
/// that are not TOML strings.
fn write_raw(path: &str, table: &str, key: &str, value: Option<&str>) -> Result<()> {
    write_line(path, table, key, value.map(|v| format!("{} = {v}", toml_key(key))))
}

fn write_line(path: &str, table: &str, key: &str, entry: Option<String>) -> Result<()> {
    let file = Path::new(path);
    let original = std::fs::read_to_string(file).unwrap_or_default();
    let header = format!("[{table}]");

    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();

    // Locate the table, and the key within it.
    let table_at = lines.iter().position(|l| l.trim() == header);
    let Some(start) = table_at else {
        // Nothing to remove from a table that does not exist.
        let Some(entry) = entry else { return Ok(()) };
        // No such table yet: append it.
        if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(entry);
        std::fs::write(file, lines.join("\n") + "\n")
            .with_context(|| format!("writing {}", file.display()))?;
        return Ok(());
    };

    // The table ends at the next header line.
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());

    let existing = lines[start + 1..end]
        .iter()
        .position(|l| defines_key(l, key))
        .map(|i| start + 1 + i);

    match (existing, entry) {
        (Some(i), Some(entry)) => lines[i] = entry,
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(entry)) => lines.insert(end, entry),
        (None, None) => return Ok(()),
    }
    std::fs::write(file, lines.join("\n") + "\n")
        .with_context(|| format!("writing {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boot order is the point: a portable build can shadow a system one
    /// without uninstalling either, and disabling an entry must skip it rather
    /// than reorder anything.
    #[test]
    fn the_retroarch_boot_order_honours_position_and_enabled() {
        let cfg: Config = toml::from_str(
            r#"
            [[retroarch.installs]]
            path = "/first"
            [[retroarch.installs]]
            path = "/disabled"
            enabled = false
            [[retroarch.installs]]
            path = "/second"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.retroarch.ordered_paths(), ["/first", "/second"]);
    }

    /// An entry with no `enabled` key is on. Defaulting the other way would
    /// silently disable every install in an existing config.
    #[test]
    fn an_install_is_enabled_unless_it_says_otherwise() {
        let cfg: Config =
            toml::from_str("[[retroarch.installs]]\npath = \"/ra\"\n").unwrap();
        assert_eq!(cfg.retroarch.ordered_paths(), ["/ra"]);
    }

    /// `installs` supersedes the legacy single `root`, but an older config that
    /// only sets `root` has to keep working untouched.
    #[test]
    fn the_legacy_single_root_still_works_and_is_superseded() {
        let legacy: Config =
            toml::from_str("[retroarch]\nroot = \"/old\"\n").unwrap();
        assert_eq!(legacy.retroarch.ordered_paths(), ["/old"]);

        let both: Config = toml::from_str(
            "[retroarch]\nroot = \"/old\"\n[[retroarch.installs]]\npath = \"/new\"\n",
        )
        .unwrap();
        assert_eq!(
            both.retroarch.ordered_paths(),
            ["/new"],
            "the list wins; the old key is not appended"
        );
    }

    /// Nothing configured means "probe the usual places", which is an empty
    /// list rather than a list containing an empty path.
    #[test]
    fn no_retroarch_config_asks_for_the_default_probe() {
        assert!(Config::default().retroarch.ordered_paths().is_empty());
    }

    /// A missing config is not an error — most commands work offline against
    /// the local cache and should not require one.
    #[test]
    fn a_missing_config_loads_defaults_rather_than_failing() {
        let cfg = Config::load_from(Path::new("/nonexistent/config.toml"))
            .expect("absent config is fine");
        assert_eq!(cfg.library.local_root, "./library");
        assert!(cfg.server.url.is_empty());
        assert!(cfg.shaders.enabled, "shaders default on");
        assert!(!Config::exists("/nonexistent/config.toml"));
    }

    /// Malformed TOML must name the file. It used to surface as a bare parse
    /// error with no indication of which file to go and look at.
    #[test]
    fn a_broken_config_says_which_file_it_is() {
        let dir = std::env::temp_dir().join("romm-config-broken");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[server\nurl = ").unwrap();

        let err = Config::load_from(&path).expect_err("not valid TOML").to_string();
        assert!(err.contains("config.toml"), "got: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every derived path hangs off `library.local_root`, so one setting moves
    /// the whole footprint — which is what makes "delete that folder" true.
    #[test]
    fn every_download_location_derives_from_one_setting() {
        let cfg: Config =
            toml::from_str("[library]\nlocal_root = \"/data/romm\"\n").unwrap();
        for dir in [cfg.local_roms_dir(), cfg.media_dir(), cfg.system_dir(), cfg.themes_dir()] {
            assert!(
                dir.starts_with("/data/romm"),
                "{} escaped the library root",
                dir.display()
            );
        }
    }

    /// The section was renamed, and the old name has to keep working — a config
    /// that silently stops applying does not error, it just turns achievements
    /// off, which is indistinguishable from never having set them up.
    #[test]
    fn the_old_cheevos_section_name_is_still_accepted() {
        let renamed: Config = toml::from_str(
            "[achievements]\nenabled = true\nusername = \"frank\"\ntoken = \"t\"\n",
        )
        .unwrap();
        let legacy: Config = toml::from_str(
            "[cheevos]\nenabled = true\nusername = \"frank\"\ntoken = \"t\"\n",
        )
        .unwrap();

        for (label, cfg) in [("achievements", renamed), ("cheevos", legacy)] {
            let s = cfg.achievements.settings();
            assert!(s.enabled, "[{label}] should be read");
            assert!(s.usable(), "[{label}] should authenticate");
            assert_eq!(s.username.as_deref(), Some("frank"), "[{label}]");
            assert_eq!(
                s.credential(),
                Some(("cheevos_token", "t")),
                "[{label}] credential"
            );
        }
    }

    /// Absent means off, and off means the user's own RetroArch settings are
    /// left alone rather than overwritten with a disabled login.
    #[test]
    fn no_achievements_section_means_untouched() {
        let cfg: Config = toml::from_str("[server]\nurl = \"http://x\"\n").unwrap();
        let s = cfg.achievements.settings();
        assert!(!s.enabled);
        assert!(!s.usable());
        assert!(crate::achievements::config_lines(&s).is_empty());
    }

    /// Every field of the section has to survive the trip, including the two
    /// that change what the emulator does.
    #[test]
    fn the_achievements_section_maps_every_field() {
        let cfg: Config = toml::from_str(
            "[achievements]\nenabled = true\nusername = \"frank\"\n\
             password = \"pw\"\nhardcore = true\ntest_unofficial = true\n",
        )
        .unwrap();
        let s = cfg.achievements.settings();
        assert_eq!(s.credential(), Some(("cheevos_password", "pw")));
        assert!(s.hardcore);
        assert!(s.test_unofficial);
        let out = crate::achievements::config_lines(&s);
        assert!(out.contains("cheevos_hardcore_mode_enable = \"true\""));
        assert!(out.contains("cheevos_test_unofficial = \"true\""));
    }

    /// A configured token has to actually reach the client, or the app keeps
    /// sending Basic and the token silently does nothing.
    #[test]
    fn a_configured_token_reaches_the_client_as_a_bearer() {
        let cfg: Config = toml::from_str(
            "[server]\nurl = \"http://dev.lan\"\ntoken = \"rmm_abc\"\nusername = \"frank\"\n",
        )
        .unwrap();
        assert_eq!(cfg.server.client().unwrap().auth(), "Bearer rmm_abc");
    }

    /// Without a token it falls back to Basic, so an existing config that never
    /// had one keeps working untouched.
    #[test]
    fn without_a_token_the_client_falls_back_to_basic() {
        let cfg: Config = toml::from_str(
            "[server]\nurl = \"http://dev.lan\"\nusername = \"user\"\npassword = \"pass\"\n",
        )
        .unwrap();
        assert_eq!(cfg.server.client().unwrap().auth(), "Basic dXNlcjpwYXNz");
    }

    /// A token alone is enough — the password was deliberately removed from the
    /// real config, so building a client must not require one.
    #[test]
    fn a_token_alone_is_enough_to_build_a_client() {
        let cfg: Config =
            toml::from_str("[server]\nurl = \"http://dev.lan\"\ntoken = \"rmm_abc\"\n").unwrap();
        assert!(
            cfg.server.client().is_ok(),
            "a config with no username or password must still connect"
        );
    }

    /// Per-game keys are file paths, which TOML cannot express as bare keys.
    /// Round-tripping through a real parser is the only check that matters.
    #[test]
    fn per_game_keys_round_trip() {
        let dir = std::env::temp_dir().join(format!("romm-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.toml");
        let p = path.to_str().unwrap();
        std::fs::write(&path, "[cores.overrides]\narcade = \"mame2003_plus\"\n").unwrap();

        // Names taken from the real library: spaces, dots, brackets, commas,
        // apostrophes, and a quote for good measure.
        let keys = [
            "arcade/blazstar.zip",
            "psx/Final Fantasy VII (USA) (Disc 1).chd",
            "snes/Blow'em Out! (USA) (Aftermarket) (Unl).zip",
            "nes/Say \"Hello\" [b1].nes",
            "gba/back\\slash.gba",
        ];
        for (i, k) in keys.iter().enumerate() {
            set_table_entry(p, "cores.per_game", k, &format!("core{i}")).unwrap();
        }

        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap())
            .expect("must still be valid TOML");
        let tbl = parsed["cores"]["per_game"].as_table().unwrap();
        assert_eq!(tbl.len(), keys.len(), "one entry per key, no duplicates");
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(tbl[*k].as_str(), Some(format!("core{i}").as_str()));
        }

        // Rewriting a key must replace, not append.
        set_table_entry(p, "cores.per_game", keys[0], "fbneo").unwrap();
        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let tbl = parsed["cores"]["per_game"].as_table().unwrap();
        assert_eq!(tbl.len(), keys.len(), "rewrite must not duplicate");
        assert_eq!(tbl[keys[0]].as_str(), Some("fbneo"));

        // Clearing removes it.
        clear_table_entry(p, "cores.per_game", keys[0]).unwrap();
        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["cores"]["per_game"].as_table().unwrap().len(), keys.len() - 1);

        // The hand-written table above must survive untouched.
        assert_eq!(parsed["cores"]["overrides"]["arcade"].as_str(), Some("mame2003_plus"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod bool_tests {
    use super::*;

    /// A TOML boolean is a bare literal. Written as a quoted string it parses
    /// as a string and then fails to deserialise into a bool — so a switch in
    /// Settings would appear to save and do nothing on the next launch.
    #[test]
    fn booleans_are_written_unquoted() {
        let dir = std::env::temp_dir().join("romm-cfg-bool");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.toml");
        let p = path.to_str().unwrap();
        std::fs::write(&path, "[achievements]\nusername = \"frank\"\n").unwrap();

        set_table_bool(p, "achievements", "enabled", true).unwrap();
        set_table_bool(p, "achievements", "hardcore", false).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("enabled = true"), "unquoted: {raw}");
        assert!(!raw.contains("\"true\""), "never quoted: {raw}");

        // And it has to survive the real parser into the real struct.
        let cfg: Config = toml::from_str(&raw).unwrap();
        assert!(cfg.achievements.enabled);
        assert!(!cfg.achievements.hardcore);
        assert_eq!(cfg.achievements.username.as_deref(), Some("frank"));

        // Flipping it back rewrites in place rather than appending a second key.
        set_table_bool(p, "achievements", "enabled", false).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.matches("enabled").count(), 1, "one key, not two: {raw}");
        assert!(!toml::from_str::<Config>(&raw).unwrap().achievements.enabled);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Strings and booleans share the writer, so the string path must still
    /// quote and must not have been broken by adding the raw one.
    #[test]
    fn strings_are_still_quoted() {
        let dir = std::env::temp_dir().join("romm-cfg-str");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.toml");
        let p = path.to_str().unwrap();

        set_table_entry(p, "library", "local_root", "/data/romm").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("local_root = \"/data/romm\""), "{raw}");
        let cfg: Config = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.library.local_root, "/data/romm");
        std::fs::remove_dir_all(&dir).ok();
    }
}
