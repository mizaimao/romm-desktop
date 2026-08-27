//! Syncing, as a state machine the interface can draw.
//!
//! Nothing here talks to the network. The worker thread does that and hands
//! back one of these states; keeping the two apart is what lets the whole of
//! the behaviour — including "the server said conflict" — be tested without a
//! server.
//!
//! The shape is fixed by one rule: **nothing moves until the plan has been
//! shown and accepted.** `negotiate` tells us what would happen before a byte
//! is transferred, so there is no reason to act first and report afterwards,
//! and every reason not to on a device whose only copy of a save is local.

use romm_desktop::api::{SyncOperation, SyncPlan};

/// What the server decided about one save.
///
/// **Declaration order is the display order** — `derive(Ord)` takes it from
/// here, and `Review::from_plan` sorts by it. Conflicts first because they are
/// the only rows that need a decision; pulls before pushes because a pull
/// overwrites something local and deserves the closer look.
#[derive(Copy, Clone, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Action {
    /// Both sides moved since the last sync. Needs a person.
    Conflict,
    /// Theirs is newer.
    Download,
    /// Ours is newer.
    Upload,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Conflict => "conflict",
            Action::Upload => "push",
            Action::Download => "pull",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "upload" => Action::Upload,
            "download" => Action::Download,
            "conflict" => Action::Conflict,
            // `no_op` is the server saying "already agreed". Worth counting,
            // never worth a row.
            _ => return None,
        })
    }
}

/// One line of the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub action: Action,
    pub title: String,
    /// The server's own words for why, when it gave any. Shown verbatim
    /// rather than paraphrased — it knows things this device does not.
    pub reason: Option<String>,
    pub rom_id: i64,
    pub save_id: Option<i64>,
}

/// A plan, ready to be shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    pub lines: Vec<Line>,
    pub agreed: usize,
}

impl Review {
    /// Conflicts first, then pulls, then pushes.
    ///
    /// Not alphabetical: the only rows that need a decision are the conflicts,
    /// and a list that buries them under two hundred agreed pushes is a list
    /// nobody reads to the bottom of.
    pub fn from_plan(plan: &SyncPlan) -> Self {
        let mut lines: Vec<Line> = plan
            .operations
            .iter()
            .filter_map(Self::line_of)
            .collect();
        lines.sort_by(|a, b| a.action.cmp(&b.action).then(a.title.cmp(&b.title)));
        Review { lines, agreed: plan.total_no_op.max(0) as usize }
    }

    fn line_of(op: &SyncOperation) -> Option<Line> {
        let action = Action::parse(&op.action)?;
        Some(Line {
            action,
            title: op
                .file_name
                .clone()
                .unwrap_or_else(|| format!("rom {}", op.rom_id)),
            reason: op.reason.clone().filter(|r| !r.trim().is_empty()),
            rom_id: op.rom_id,
            save_id: op.save_id,
        })
    }

    pub fn count(&self, action: Action) -> usize {
        self.lines.iter().filter(|l| l.action == action).count()
    }

    /// Nothing to do. Distinct from "not asked yet", which is `Stage::Idle`.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// One line for the top of the panel.
    pub fn headline(&self) -> String {
        if self.is_empty() {
            return match self.agreed {
                0 => "nothing to sync".into(),
                n => format!("nothing to do — {n} already match"),
            };
        }
        let mut parts = Vec::new();
        for action in [Action::Conflict, Action::Download, Action::Upload] {
            let n = self.count(action);
            if n > 0 {
                parts.push(format!("{n} to {}", action.label()));
            }
        }
        parts.join(", ")
    }
}

/// Where a sync has got to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    /// Never asked.
    #[default]
    Idle,
    /// Scanning, or waiting on the server. `note` says which.
    Asking { note: String },
    /// A plan, waiting for a person.
    Ready(Review),
    /// Carrying it out.
    Running { done: usize, total: usize },
    /// Finished. `conflicts` is a count rather than the conflicts themselves:
    /// those are held beside the app so this stays cheap to clone and compare.
    Done { moved: usize, conflicts: usize, note: String },
    Failed(String),
}

impl Stage {
    /// What A does next, in words, or `None` when A does nothing.
    ///
    /// One place decides this so the help line, the prompt and the handler
    /// cannot drift apart — which is how "A is not wired to apply" happened.
    pub fn next_step(&self) -> Option<&'static str> {
        match self {
            Stage::Ready(review) if review.is_empty() => Some("see what would sync"),
            Stage::Ready(_) => Some("carry this out"),
            Stage::Idle | Stage::Done { .. } | Stage::Failed(_) => Some("see what would sync"),
            // Asking, Running: the worker has it.
            _ => None,
        }
    }

    /// Can a person start something right now?
    ///
    /// False while the network is busy, which is what stops a second press
    /// starting a second sync over the top of the first.
    pub fn is_busy(&self) -> bool {
        matches!(self, Stage::Asking { .. } | Stage::Running { .. })
    }

    /// The line under the tab, whatever state it is in.
    pub fn note(&self) -> String {
        match self {
            Stage::Idle => "not synced yet".into(),
            Stage::Asking { note } => note.clone(),
            Stage::Ready(review) => review.headline(),
            Stage::Running { done, total } => format!("{done} of {total}"),
            Stage::Done { note, .. } => note.clone(),
            Stage::Failed(why) => format!("failed: {why}"),
        }
    }
}

/// Where the favourites-and-collections sync has got to.
///
/// Its own type rather than a second use of [`Stage`]: that one's `Ready`
/// carries a save review, and the two syncs are independent — you can look at
/// what the stars would do without touching the saves, and the sync tab draws
/// both lines at once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Stars {
    #[default]
    Idle,
    Asking(String),
    /// A plan, waiting for a person. `moves` is zero when both sides agree.
    Ready { headline: String, moves: usize },
    Done(String),
    Failed(String),
}

impl Stars {
    /// What A does next, in words, or `None` while the worker has it.
    ///
    /// One place decides this, so the help line and the handler cannot drift
    /// apart — the mistake that made A do nothing on the patches tab.
    pub fn next_step(&self) -> Option<&'static str> {
        match self {
            Stars::Ready { moves, .. } if *moves > 0 => Some("carry this out"),
            Stars::Ready { .. } | Stars::Idle | Stars::Done(_) | Stars::Failed(_) => {
                Some("see what would sync")
            }
            Stars::Asking(_) => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Stars::Asking(_))
    }

    pub fn note(&self) -> String {
        match self {
            Stars::Idle => "not checked yet".into(),
            Stars::Asking(note) => note.clone(),
            Stars::Ready { headline, .. } => headline.clone(),
            Stars::Done(note) => note.clone(),
            Stars::Failed(why) => format!("failed: {why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(action: &str, name: &str, rom_id: i64) -> SyncOperation {
        SyncOperation {
            action: action.into(),
            rom_id,
            save_id: Some(rom_id),
            file_name: Some(name.into()),
            slot: Some("autosave".into()),
            emulator: Some("mgba".into()),
            reason: None,
            server_content_hash: None,
            server_updated_at: None,
        }
    }

    fn plan(ops: Vec<SyncOperation>, no_op: i64) -> SyncPlan {
        SyncPlan {
            session_id: Some(1),
            operations: ops,
            total_upload: 0,
            total_download: 0,
            total_conflict: 0,
            total_no_op: no_op,
        }
    }

    #[test]
    fn conflicts_come_first() {
        // The only rows needing a decision are the conflicts. A list that
        // buries them under two hundred agreed pushes is one nobody reads to
        // the bottom of.
        let review = Review::from_plan(&plan(
            vec![
                op("upload", "Zelda.srm", 1),
                op("download", "Metroid.srm", 2),
                op("conflict", "Crash.srm", 3),
                op("upload", "Aria.srm", 4),
            ],
            0,
        ));
        let order: Vec<Action> = review.lines.iter().map(|l| l.action).collect();
        assert_eq!(
            order,
            vec![Action::Conflict, Action::Download, Action::Upload, Action::Upload]
        );
        // and alphabetical within a group
        assert_eq!(review.lines[2].title, "Aria.srm");
    }

    #[test]
    fn no_op_is_counted_and_never_shown() {
        // "Already agreed" is worth a number so the user can tell a working
        // sync from one that found nothing — but it is not worth 380 rows.
        let review = Review::from_plan(&plan(vec![op("no_op", "Same.srm", 1)], 380));
        assert!(review.is_empty());
        assert_eq!(review.agreed, 380);
        assert_eq!(review.headline(), "nothing to do — 380 already match");
    }

    #[test]
    fn an_empty_plan_says_so_rather_than_looking_broken() {
        let review = Review::from_plan(&plan(vec![], 0));
        assert_eq!(review.headline(), "nothing to sync");
    }

    #[test]
    fn the_headline_counts_each_kind() {
        let review = Review::from_plan(&plan(
            vec![
                op("upload", "a.srm", 1),
                op("upload", "b.srm", 2),
                op("download", "c.srm", 3),
                op("conflict", "d.srm", 4),
            ],
            5,
        ));
        assert_eq!(review.headline(), "1 to conflict, 1 to pull, 2 to push");
        assert_eq!(review.count(Action::Upload), 2);
    }

    #[test]
    fn an_operation_with_no_filename_still_names_something() {
        // Never an empty row: a blank line in a confirmation is worse than an
        // ugly one, because there is nothing to decide about.
        let mut o = op("upload", "", 77);
        o.file_name = None;
        let review = Review::from_plan(&plan(vec![o], 0));
        assert_eq!(review.lines[0].title, "rom 77");
    }

    #[test]
    fn the_servers_reason_survives_but_blank_ones_do_not() {
        let mut o = op("conflict", "Crash.srm", 1);
        o.reason = Some("both changed since 2026-08-20".into());
        let mut blank = op("upload", "Zelda.srm", 2);
        blank.reason = Some("   ".into());
        let review = Review::from_plan(&plan(vec![o, blank], 0));
        assert_eq!(
            review.lines[0].reason.as_deref(),
            Some("both changed since 2026-08-20")
        );
        assert_eq!(review.lines[1].reason, None);
    }

    #[test]
    fn a_stage_with_an_empty_plan_offers_to_look_again_not_to_run_it() {
        // Carrying out nothing is not a thing to offer. Before, the help line
        // and the handler each decided this for themselves.
        let empty = Stage::Ready(Review::from_plan(&plan(vec![], 12)));
        assert_eq!(empty.next_step(), Some("see what would sync"));

        let something = Stage::Ready(Review::from_plan(&plan(vec![op("upload", "a.srm", 1)], 0)));
        assert_eq!(something.next_step(), Some("carry this out"));
    }

    #[test]
    fn nothing_is_offered_while_the_worker_is_busy() {
        assert_eq!(Stage::Asking { note: "scanning".into() }.next_step(), None);
        assert_eq!(Stage::Running { done: 1, total: 4 }.next_step(), None);
    }

    #[test]
    fn after_a_run_you_can_look_again() {
        // Including after a failure — a sync that failed on a dropped wifi
        // should be one button away from being retried, not a restart.
        let done = Stage::Done { moved: 4, conflicts: 1, note: "4 moved".into() };
        assert_eq!(done.next_step(), Some("see what would sync"));
        assert_eq!(
            Stage::Failed("no route to host".into()).next_step(),
            Some("see what would sync")
        );
    }

    #[test]
    fn a_busy_stage_refuses_to_start_another() {
        // Two syncs at once would race on the same files. The interface asks
        // this before it lets A do anything.
        assert!(!Stage::Idle.is_busy());
        assert!(Stage::Asking { note: "asking the server".into() }.is_busy());
        assert!(Stage::Running { done: 1, total: 9 }.is_busy());
        assert!(!Stage::Ready(Review::default()).is_busy());
        assert!(!Stage::Failed("no route to host".into()).is_busy());
        assert!(!Stage::Done { moved: 0, conflicts: 0, note: "done".into() }.is_busy());
    }

    #[test]
    fn every_stage_has_something_to_say() {
        // The sync tab always shows a line. "Nothing at all" reads as frozen.
        for stage in [
            Stage::Idle,
            Stage::Asking { note: "scanning saves".into() },
            Stage::Ready(Review::default()),
            Stage::Running { done: 2, total: 7 },
            Stage::Done { moved: 3, conflicts: 0, note: "3 saves pushed".into() },
            Stage::Failed("no route to host".into()),
        ] {
            assert!(!stage.note().is_empty(), "{stage:?} said nothing");
        }
        assert_eq!(Stage::Running { done: 2, total: 7 }.note(), "2 of 7");
        assert_eq!(
            Stage::Failed("no route to host".into()).note(),
            "failed: no route to host"
        );
    }

    #[test]
    fn the_stars_stage_offers_to_look_before_it_offers_to_act() {
        assert_eq!(Stars::Idle.next_step(), Some("see what would sync"));
        assert_eq!(
            Stars::Ready { headline: "2 to send".into(), moves: 2 }.next_step(),
            Some("carry this out")
        );
        // A plan with nothing in it is not a thing to carry out.
        assert_eq!(
            Stars::Ready { headline: "nothing to do".into(), moves: 0 }.next_step(),
            Some("see what would sync")
        );
        assert_eq!(Stars::Asking("looking".into()).next_step(), None);
    }

    #[test]
    fn the_stars_stage_always_has_something_to_say() {
        for stage in [
            Stars::Idle,
            Stars::Asking("reading the card".into()),
            Stars::Ready { headline: "1 to send".into(), moves: 1 },
            Stars::Done("1 sent".into()),
            Stars::Failed("no route to host".into()),
        ] {
            assert!(!stage.note().is_empty(), "{stage:?} said nothing");
        }
        assert_eq!(
            Stars::Failed("no route to host".into()).note(),
            "failed: no route to host"
        );
    }

    #[test]
    fn a_second_press_while_it_is_working_starts_nothing() {
        assert!(Stars::Asking("looking".into()).is_busy());
        assert!(!Stars::Idle.is_busy());
        assert!(!Stars::Ready { headline: "x".into(), moves: 1 }.is_busy());
    }
}
