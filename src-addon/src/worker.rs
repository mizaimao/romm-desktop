//! The network, on a thread of its own.
//!
//! The menu draws at 640×480 on a device with four slow cores; a sync that
//! blocked the loop would look exactly like the app having hung, and this one
//! has looked hung enough times already. So everything here runs on a worker
//! and reports back through a channel the interface drains once a frame.
//!
//! `sync::Stage` is the vocabulary. This file only produces those states — it
//! makes no decisions, which is why the decisions are all testable without a
//! server.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use romm_desktop::cache::Cache;
use romm_desktop::config::Config;
use romm_desktop::coremap::CoreMap;

use crate::sync::{Review, Stage};

/// Where the library index lives.
///
/// Matching a save to a server rom id needs the cache, and building one means
/// pulling the whole library — 7,883 rows on this device. The archived front
/// end already did that, so its database is looked for before anything is
/// rebuilt. A wrong guess here is not fatal: an absent cache means every save
/// resolves to nothing and the plan comes back empty, which reads as "sync is
/// broken" — so the search order is written down rather than left to luck.
pub fn find_cache(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

/// The places worth looking, in order.
pub fn cache_search_path(app_dir: &Path) -> Vec<PathBuf> {
    vec![
        // Ours, once the addon builds its own.
        app_dir.join("cache.sqlite3"),
        // The archived front end's, on this device.
        app_dir.join("../romm/cache.sqlite3"),
        PathBuf::from("/userdata/system/romm/cache.sqlite3"),
    ]
}

/// What the worker sends back.
#[derive(Debug)]
pub enum Message {
    /// Something to show while waiting. Not progress in the counted sense —
    /// scanning and negotiating have no total until they finish.
    Note(String),
    Plan(Box<Review>),
    Failed(String),
}

/// A running job. Dropping it does not cancel the thread; the channel simply
/// stops being read, which is the right behaviour when the app is closing.
pub struct Job {
    rx: Receiver<Message>,
}

impl Job {
    /// Everything the worker has said since the last look.
    ///
    /// Drains rather than taking one, so a burst of notes cannot leave the
    /// interface a frame behind the truth.
    pub fn drain(&self) -> Vec<Message> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(m) => out.push(m),
                Err(TryRecvError::Empty) => break,
                // The thread finished and dropped its end. Nothing more is
                // coming, which is not an error.
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

/// Ask the server what a sync would do. Moves nothing.
///
/// This is the whole of the first stage: scan what is on the card, hand it to
/// `/api/sync/negotiate`, and hand back the plan for a person to look at.
pub fn negotiate(cfg: &Config, ra_root: &Path, app_dir: &Path) -> Job {
    let (tx, rx) = channel();
    let server = cfg.server.url.clone();
    let username = cfg.server.username.clone();
    let password = cfg.server.password.clone();
    let token = cfg.server.token.clone();
    let ra_root = ra_root.to_path_buf();
    let app_dir = app_dir.to_path_buf();
    let data_dir = app_dir.clone();

    std::thread::spawn(move || {
        let say = |m: Message| {
            // A closed channel means the app moved on. Not worth reporting to
            // a channel that is closed.
            let _ = tx.send(m);
        };

        if server.trim().is_empty() {
            say(Message::Failed("no server in config.toml".into()));
            return;
        }
        let Some(cache_path) = find_cache(&cache_search_path(&app_dir)) else {
            say(Message::Failed(
                "no library index — the save list cannot be matched to the server".into(),
            ));
            return;
        };

        say(Message::Note("reading the library index".into()));
        let cache = match Cache::open(&cache_path) {
            Ok(c) => c,
            Err(e) => return say(Message::Failed(format!("opening the index: {e:#}"))),
        };
        let map = CoreMap::load_or_embedded(&app_dir.join("data/esde-core-map.json"));

        say(Message::Note("scanning saves".into()));
        let candidates = match romm_desktop::savesync::scan(&cache, &map, &ra_root) {
            Ok(c) => c,
            Err(e) => return say(Message::Failed(format!("scanning saves: {e:#}"))),
        };
        let (states, skipped) = romm_desktop::savesync::client_states(&candidates);
        say(Message::Note(format!(
            "{} saves found, {skipped} unmatched — asking the server",
            states.len()
        )));

        // The client is async and this thread is not, so it gets a runtime of
        // its own. One current-thread runtime, not the multi-threaded one:
        // there is exactly one request in flight and four slow cores to leave
        // alone.
        let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(r) => r,
            Err(e) => return say(Message::Failed(format!("starting the network: {e}"))),
        };
        let result = runtime.block_on(async {
            let client =
                romm_desktop::api::Client::with_auth(&server, &username, &password, token.as_deref())?;
            let identity =
                romm_desktop::savesync::DeviceIdentity::ensure(&client, &data_dir).await?;
            client.negotiate(&identity.device_id, &states).await
        });

        match result {
            Ok(plan) => say(Message::Plan(Box::new(Review::from_plan(&plan)))),
            Err(e) => say(Message::Failed(format!("{e:#}"))),
        }
    });

    Job { rx }
}

/// Fold everything the worker has said into the stage the interface draws.
///
/// Separate from the thread so the whole of it is testable: the interface's
/// behaviour on a burst of notes, on a plan, and on a failure is decided here
/// and nowhere else.
pub fn apply(stage: &mut Stage, messages: Vec<Message>) {
    for message in messages {
        *stage = match message {
            Message::Note(note) => Stage::Asking { note },
            Message::Plan(review) => Stage::Ready(*review),
            Message::Failed(why) => Stage::Failed(why),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_word_wins() {
        // Notes arrive faster than frames. Showing the first of a burst would
        // leave the line a step behind what the worker is actually doing.
        let mut stage = Stage::Idle;
        apply(
            &mut stage,
            vec![
                Message::Note("reading the library index".into()),
                Message::Note("scanning saves".into()),
            ],
        );
        assert_eq!(stage.note(), "scanning saves");
    }

    #[test]
    fn a_plan_arriving_after_notes_is_what_is_shown() {
        let mut stage = Stage::Idle;
        apply(
            &mut stage,
            vec![
                Message::Note("scanning saves".into()),
                Message::Plan(Box::default()),
            ],
        );
        assert!(matches!(stage, Stage::Ready(_)));
        assert!(!stage.is_busy(), "a plan means the worker is done");
    }

    #[test]
    fn a_failure_is_not_left_looking_busy() {
        // The state that mattered: a stage stuck on "asking the server" after
        // the request already failed is indistinguishable from a hang.
        let mut stage = Stage::Asking { note: "asking the server".into() };
        apply(&mut stage, vec![Message::Failed("no route to host".into())]);
        assert_eq!(stage.note(), "failed: no route to host");
        assert!(!stage.is_busy());
    }

    #[test]
    fn nothing_said_changes_nothing() {
        let mut stage = Stage::Asking { note: "scanning saves".into() };
        apply(&mut stage, vec![]);
        assert_eq!(stage, Stage::Asking { note: "scanning saves".into() });
    }

    #[test]
    fn the_index_is_looked_for_where_it_actually_is() {
        // The archived front end's database is the one this device has, and
        // rebuilding it means pulling 7,883 rows over wifi.
        let dir = std::env::temp_dir().join("moose-cache-search");
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join("moose-patch");
        std::fs::create_dir_all(app.join("../romm")).unwrap();

        assert_eq!(find_cache(&cache_search_path(&app)), None, "nothing to find yet");

        let theirs = app.join("../romm/cache.sqlite3");
        std::fs::write(&theirs, b"x").unwrap();
        assert_eq!(find_cache(&cache_search_path(&app)), Some(theirs));

        // Ours wins once it exists.
        let ours = app.join("cache.sqlite3");
        std::fs::write(&ours, b"x").unwrap();
        assert_eq!(find_cache(&cache_search_path(&app)), Some(ours));
    }
}
