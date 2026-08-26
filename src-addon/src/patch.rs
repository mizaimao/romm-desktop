//! Applying, reverting, and — the one that earns its keep — reading back.
//!
//! Every patch here is a list of **steps**, and every step is one of two
//! things: a marked block inside a text config, or a file we own. That is not
//! a simplification for its own sake. It came out of placing all of these by
//! hand first, and finding that KNULLI already provides a `/userdata`
//! override for nearly everything — so the work was never the change, it was
//! knowing which file survives an upgrade.
//!
//! Because a step can be asked whether it is already satisfied, a patch can
//! report which of its options the device is actually sitting at, or that it
//! is at none of them because an update moved the ground. That third answer
//! is why this is not a list of booleans.
//!
//! Reverting is not a special path. "off" is an option like any other, and its
//! steps are "clear that block" and "put that file back", so the machinery
//! that applies is the machinery that undoes.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Where everything lives. A struct rather than constants so the tests can
/// point the whole engine at a temporary directory and watch what it writes.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Paths { root: PathBuf::from("/") }
    }
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Paths { root: root.into() }
    }

    fn at(&self, rest: &str) -> PathBuf {
        self.root.join(rest)
    }

    /// The one file KNULLI reads for almost every setting, and the one that
    /// survives an upgrade.
    pub fn knulli_conf(&self) -> PathBuf {
        self.at("userdata/system/knulli.conf")
    }

    pub fn boot_custom(&self) -> PathBuf {
        self.at("boot/boot-custom.sh")
    }

    pub fn user_startup(&self) -> PathBuf {
        self.at("userdata/system/custom.sh")
    }

    pub fn trigger_conf(&self) -> PathBuf {
        self.at("userdata/system/configs/multimedia_keys.conf")
    }

    pub fn es_input(&self) -> PathBuf {
        self.at("userdata/system/configs/emulationstation/es_input.cfg")
    }

    pub fn decoration(&self, rest: &str) -> PathBuf {
        self.at(&format!("userdata/decorations/moose/systems/{rest}"))
    }

    pub fn shader(&self, rest: &str) -> PathBuf {
        self.at(&format!("userdata/shaders/moose/{rest}"))
    }

    pub fn blank_logo(&self) -> PathBuf {
        self.at("userdata/system/moose-patch/blank-logo.png")
    }

    /// The image EmulationStation draws while it loads. On the squashfs, so
    /// replacing it only lasts until the next boot — which is what
    /// `boot-custom.sh` is for.
    pub fn es_logo(&self) -> PathBuf {
        self.at("usr/share/emulationstation/resources/logo.png")
    }

    pub fn gpu_choice(&self) -> PathBuf {
        self.at("userdata/system/gpu/selected")
    }

    /// KNULLI's own trigger file, which the one in /userdata replaces.
    pub fn stock_triggers(&self) -> PathBuf {
        self.at("etc/triggerhappy/triggers.d/multimedia_keys.conf")
    }

    pub fn profile(&self) -> PathBuf {
        self.at("userdata/system/moose-patch/profile.toml")
    }
}

/// The marker a block is wrapped in. Rewriting between a matched pair is what
/// makes a config patch idempotent, and what makes "off" possible without
/// remembering what the file looked like before.
fn open_marker(id: &str) -> String {
    format!("## moose-patch: {id}")
}
fn close_marker(id: &str) -> String {
    format!("## moose-patch: {id} end")
}

/// The body between the markers, if the block is there at all.
pub fn read_block(text: &str, id: &str) -> Option<String> {
    let open = open_marker(id);
    let close = close_marker(id);
    let mut inside = false;
    let mut body = Vec::new();
    for line in text.lines() {
        if line.trim() == close {
            return Some(body.join("\n"));
        }
        if inside {
            body.push(line);
        }
        // Checked after the close, so a one-line block cannot swallow its own
        // terminator: `close` starts with `open`, and the trim-compare above
        // would otherwise match the opener first.
        if line.trim() == open {
            inside = true;
        }
    }
    None
}

/// Put a block in, replacing any previous one. Appends when there was none.
pub fn set_block(text: &str, id: &str, body: &str) -> String {
    let cleared = clear_block(text, id);
    let mut out = cleared.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&open_marker(id));
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');
    out.push_str(&close_marker(id));
    out.push('\n');
    out
}

/// Take a block out, markers and all, leaving the rest of the file alone.
pub fn clear_block(text: &str, id: &str) -> String {
    let open = open_marker(id);
    let close = close_marker(id);
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim() == close {
            inside = false;
            continue;
        }
        if line.trim() == open {
            inside = true;
            continue;
        }
        if !inside {
            out.push(line);
        }
    }
    let mut text = out.join("\n");
    while text.ends_with("\n\n\n") {
        text.pop();
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// One thing a patch does.
#[derive(Clone, Debug)]
pub enum Step {
    /// A marked block in a text config. `None` means the block should not be
    /// there — which is how "off" is written.
    ///
    /// `seed` is the file to copy in first if the target is missing. It
    /// matters for exactly one case and that case is a trap:
    /// `multimedia_keys.conf` in `/userdata` *replaces* the one in `/etc`
    /// rather than adding to it, so creating it with only our block in it
    /// would take the volume, power and lid keys away.
    Block { file: PathBuf, id: String, body: Option<String>, seed: Option<PathBuf> },
    /// A file this app owns. `None` means it should not be there, and
    /// whatever was there before comes back.
    Place { path: PathBuf, bytes: Option<&'static [u8]> },
}

/// Write, even if the filesystem says no.
///
/// `/boot` on this device is mounted read-only, and `boot-custom.sh` lives
/// there because it is the only hook that runs before EmulationStation. So a
/// patch that has to write there gets one retry with the mount flipped, and
/// the mount is put back afterwards whatever happens.
fn write_through(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match fs::write(path, bytes) {
        Err(e) if e.kind() == std::io::ErrorKind::ReadOnlyFilesystem => {}
        other => return other,
    }
    let Some(point) = mount_point_of(path) else {
        return fs::write(path, bytes);
    };
    let _ = remount(&point, "rw");
    let result = fs::write(path, bytes);
    let _ = remount(&point, "ro");
    result
}

fn remount(point: &str, how: &str) -> std::io::Result<()> {
    std::process::Command::new("mount")
        .args(["-o", &format!("remount,{how}"), point])
        .status()
        .map(|_| ())
}

/// The longest mount point in /proc/mounts that this path sits under.
fn mount_point_of(path: &Path) -> Option<String> {
    let mounts = fs::read_to_string("/proc/mounts").ok()?;
    let target = path.to_str()?;
    mounts
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|point| *point != "/" && target.starts_with(point))
        .max_by_key(|point| point.len())
        .map(str::to_string)
}

/// Kept beside a file we replaced, so "off" can put the original back rather
/// than deleting something KNULLI shipped.
fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".moose-orig");
    PathBuf::from(name)
}

impl Step {
    /// Is the device already like this?
    pub fn satisfied(&self) -> bool {
        match self {
            Step::Block { file, id, body, .. } => {
                let text = fs::read_to_string(file).unwrap_or_default();
                match (read_block(&text, id), body) {
                    (Some(found), Some(want)) => found.trim() == want.trim(),
                    (None, None) => true,
                    _ => false,
                }
            }
            Step::Place { path, bytes } => match bytes {
                Some(want) => fs::read(path).map(|got| got == *want).unwrap_or(false),
                None => !path.exists(),
            },
        }
    }

    pub fn apply(&self) -> Result<()> {
        match self {
            Step::Block { file, id, body, seed } => {
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent).ok();
                }
                if let Some(seed) = seed
                    && !file.exists()
                {
                    fs::copy(seed, file).ok();
                }
                let text = fs::read_to_string(file).unwrap_or_default();
                let next = match body {
                    Some(body) => set_block(&text, id, body),
                    None => clear_block(&text, id),
                };
                write_through(file, next.as_bytes())
                    .with_context(|| format!("writing {}", file.display()))
            }
            Step::Place { path, bytes } => match bytes {
                Some(bytes) => {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).ok();
                    }
                    // Keep whatever was there the first time, and only the
                    // first time — a second apply must not back up our own
                    // copy over the real original.
                    let backup = backup_of(path);
                    if path.exists() && !backup.exists() {
                        fs::copy(path, &backup).ok();
                    }
                    write_through(path, bytes)
                        .with_context(|| format!("writing {}", path.display()))
                }
                None => {
                    let backup = backup_of(path);
                    if backup.exists() {
                        fs::copy(&backup, path)
                            .with_context(|| format!("restoring {}", path.display()))?;
                        fs::remove_file(&backup).ok();
                    } else if path.exists() {
                        fs::remove_file(path)
                            .with_context(|| format!("removing {}", path.display()))?;
                    }
                    Ok(())
                }
            },
        }
    }
}

/// One setting a patch can be at.
#[derive(Clone, Debug)]
pub struct Choice {
    pub name: String,
    pub steps: Vec<Step>,
}

/// Where a patch currently is.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum State {
    /// Sitting exactly at one of its options.
    At(usize),
    /// At none of them. Either somebody edited the file by hand, or KNULLI
    /// shipped an update and the ground moved. Worth saying out loud rather
    /// than quietly overwriting.
    Changed,
}

#[derive(Clone, Debug)]
pub struct Patch {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub choices: Vec<Choice>,
}

impl Patch {
    pub fn state(&self) -> State {
        self.choices
            .iter()
            .position(|c| c.steps.iter().all(Step::satisfied))
            .map(State::At)
            .unwrap_or(State::Changed)
    }

    pub fn option_names(&self) -> Vec<String> {
        self.choices.iter().map(|c| c.name.clone()).collect()
    }

    /// Run one option's steps in order. Stops at the first failure and says
    /// which step it was, because a half-applied patch that reports success
    /// is worse than one that fails loudly.
    pub fn apply(&self, index: usize) -> Result<()> {
        let choice = self
            .choices
            .get(index)
            .with_context(|| format!("{} has no option {index}", self.id))?;
        for (n, step) in choice.steps.iter().enumerate() {
            step.apply()
                .with_context(|| format!("{} step {}", self.id, n + 1))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("moose-patch-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_block_round_trips() {
        let text = "keep=1\n";
        let with = set_block(text, "hotkeys", "a=1\nb=2");
        assert_eq!(read_block(&with, "hotkeys").as_deref(), Some("a=1\nb=2"));
        assert!(with.contains("keep=1"), "the rest of the file must survive");
        let without = clear_block(&with, "hotkeys");
        assert_eq!(read_block(&without, "hotkeys"), None);
        assert!(without.contains("keep=1"));
    }

    #[test]
    fn applying_twice_does_not_stack_blocks() {
        // knulli.conf is read by a parser that takes the last value it sees.
        // Two copies of a block is how a revert silently fails.
        let once = set_block("keep=1\n", "shaders", "x=1");
        let twice = set_block(&once, "shaders", "x=1");
        assert_eq!(twice.matches("## moose-patch: shaders\n").count(), 1);
        assert_eq!(once, twice, "a second apply should change nothing");
    }

    #[test]
    fn changing_a_block_replaces_it_rather_than_appending() {
        let once = set_block("keep=1\n", "shaders", "set=a");
        let twice = set_block(&once, "shaders", "set=b");
        assert_eq!(read_block(&twice, "shaders").as_deref(), Some("set=b"));
        assert!(!twice.contains("set=a"));
    }

    #[test]
    fn clearing_a_block_that_was_never_there_is_harmless() {
        assert_eq!(clear_block("keep=1\n", "nope"), "keep=1\n");
    }

    #[test]
    fn a_block_step_reads_its_own_state() {
        let dir = scratch("block-state");
        let file = dir.join("knulli.conf");
        fs::write(&file, "keep=1\n").unwrap();

        let on = Step::Block {
            file: file.clone(),
            id: "hotkeys".into(),
            body: Some("a=1".into()),
            seed: None,
        };
        let off = Step::Block { file, id: "hotkeys".into(), body: None, seed: None };

        assert!(off.satisfied(), "no block means off is already true");
        assert!(!on.satisfied());
        on.apply().unwrap();
        assert!(on.satisfied());
        assert!(!off.satisfied());
        off.apply().unwrap();
        assert!(off.satisfied(), "off has to be able to undo on");
    }

    #[test]
    fn placing_a_file_keeps_the_original_and_gives_it_back() {
        // This is the whole reason "off" is safe on es_input.cfg and on the
        // ES logo: what KNULLI shipped has to come back, not vanish.
        let dir = scratch("place");
        let path = dir.join("logo.png");
        fs::write(&path, b"the original").unwrap();

        let on = Step::Place { path: path.clone(), bytes: Some(b"ours") };
        let off = Step::Place { path: path.clone(), bytes: None };

        on.apply().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"ours");
        assert!(on.satisfied());

        off.apply().unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"the original",
            "reverting must restore what was there, not delete it"
        );
        assert!(!backup_of(&path).exists(), "and clean up after itself");
    }

    #[test]
    fn a_second_apply_does_not_overwrite_the_backup() {
        let dir = scratch("backup-once");
        let path = dir.join("logo.png");
        fs::write(&path, b"the original").unwrap();
        let on = Step::Place { path: path.clone(), bytes: Some(b"ours") };
        on.apply().unwrap();
        on.apply().unwrap();
        Step::Place { path: path.clone(), bytes: None }.apply().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"the original");
    }

    #[test]
    fn a_file_we_added_is_removed_rather_than_restored() {
        let dir = scratch("place-new");
        let path = dir.join("added.conf");
        Step::Place { path: path.clone(), bytes: Some(b"x") }.apply().unwrap();
        assert!(path.exists());
        Step::Place { path: path.clone(), bytes: None }.apply().unwrap();
        assert!(!path.exists(), "nothing was there before, so nothing after");
    }

    #[test]
    fn a_patch_reports_which_option_it_is_at() {
        let dir = scratch("patch-state");
        let file = dir.join("knulli.conf");
        fs::write(&file, "keep=1\n").unwrap();

        let patch = Patch {
            id: "shaders",
            title: "Shaders",
            detail: "",
            choices: vec![
                Choice {
                    name: "off".into(),
                    steps: vec![Step::Block {
                        file: file.clone(),
                        id: "shaders".into(),
                        body: None,
                        seed: None,
                    }],
                },
                Choice {
                    name: "ON".into(),
                    steps: vec![Step::Block {
                        file: file.clone(),
                        id: "shaders".into(),
                        body: Some("set=a".into()),
                        seed: None,
                    }],
                },
            ],
        };

        assert_eq!(patch.state(), State::At(0));
        patch.apply(1).unwrap();
        assert_eq!(patch.state(), State::At(1));
        patch.apply(0).unwrap();
        assert_eq!(patch.state(), State::At(0), "and back again");
    }

    #[test]
    fn a_hand_edited_block_reads_as_changed() {
        // The case this whole enum exists for: KNULLI ships an update, or
        // somebody edits the file, and what is there is not any of our
        // options. Overwriting silently would be the wrong answer.
        let dir = scratch("patch-changed");
        let file = dir.join("knulli.conf");
        fs::write(&file, set_block("keep=1\n", "shaders", "set=SOMETHING ELSE")).unwrap();

        let patch = Patch {
            id: "shaders",
            title: "Shaders",
            detail: "",
            choices: vec![Choice {
                name: "ON".into(),
                steps: vec![Step::Block {
                    file,
                    id: "shaders".into(),
                    body: Some("set=a".into()),
                    seed: None,
                }],
            }],
        };
        assert_eq!(patch.state(), State::Changed);
    }
}
