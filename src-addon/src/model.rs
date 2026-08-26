//! What the menu holds, apart from how it is drawn.
//!
//! The shape of this file is one decision: **nothing happens when you turn a
//! dial.** A row remembers what is true on the device and what you have
//! turned it to, and the difference between those two is the work queued up.
//! Applying is a separate, deliberate act at the end.
//!
//! That is not decoration. Half of these patches stop EmulationStation
//! starting if they are wrong, and the device is then a black screen with no
//! menu to undo them from. Queuing means one confirmation covers the whole
//! batch, and it means backing out is free right up until the moment it is not.

/// Which half of the app you are looking at.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tab {
    Sync,
    Patches,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Sync, Tab::Patches];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Sync => "SYNC",
            Tab::Patches => "PATCHES",
        }
    }
}

/// What a row does when you press A on it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A setting with named options, cycled left and right. Not a checkbox:
    /// the graphics driver has two names rather than an on and an off, and
    /// bezels will have a list of packs.
    Choice { options: Vec<String>, live: usize, picked: usize },
    /// Something to read. No options, never pending.
    Fact { value: String },
    /// An action — push saves, pull saves. Runs on its own rather than
    /// joining the queue, because there is nothing to reconcile.
    Action { note: String },
}

/// One line.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub title: String,
    /// One sentence, shown when X is pressed. Says what the patch actually
    /// changes and where, because "on" is not enough to undo something by
    /// hand at two in the morning.
    pub detail: String,
    pub kind: Kind,
}

impl Row {
    pub fn choice(
        id: &str,
        title: &str,
        detail: &str,
        options: &[&str],
        live: usize,
    ) -> Self {
        Row {
            id: id.into(),
            title: title.into(),
            detail: detail.into(),
            kind: Kind::Choice {
                options: options.iter().map(|s| (*s).to_string()).collect(),
                live,
                picked: live,
            },
        }
    }

    pub fn fact(id: &str, title: &str, value: &str) -> Self {
        Row {
            id: id.into(),
            title: title.into(),
            detail: String::new(),
            kind: Kind::Fact { value: value.into() },
        }
    }

    pub fn action(id: &str, title: &str, detail: &str, note: &str) -> Self {
        Row {
            id: id.into(),
            title: title.into(),
            detail: detail.into(),
            kind: Kind::Action { note: note.into() },
        }
    }

    /// Turned away from what the device is actually doing.
    pub fn pending(&self) -> bool {
        matches!(&self.kind, Kind::Choice { live, picked, .. } if live != picked)
    }

    /// Only rows you can put a cursor on. A fact is not one of them — landing
    /// on a line that does nothing when you press a button reads as the app
    /// having stopped responding.
    pub fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Fact { .. })
    }

    /// What the right-hand column reads.
    pub fn value(&self) -> &str {
        match &self.kind {
            Kind::Choice { options, picked, .. } => {
                options.get(*picked).map(String::as_str).unwrap_or("")
            }
            Kind::Fact { value } => value,
            Kind::Action { note } => note,
        }
    }

    /// What it read before you touched it, if that is different.
    pub fn was(&self) -> Option<&str> {
        match &self.kind {
            Kind::Choice { options, live, picked } if live != picked => {
                options.get(*live).map(String::as_str)
            }
            _ => None,
        }
    }

    /// Cycling wraps. With two options that is the only sane behaviour, and
    /// with six it saves a long walk back.
    pub fn turn(&mut self, by: i32) {
        if let Kind::Choice { options, picked, .. } = &mut self.kind {
            if options.is_empty() {
                return;
            }
            let n = options.len() as i32;
            *picked = (((*picked as i32 + by) % n + n) % n) as usize;
        }
    }

    /// After the script has run and the device really is like this.
    pub fn settle(&mut self) {
        if let Kind::Choice { live, picked, .. } = &mut self.kind {
            *live = *picked;
        }
    }

    /// Put the dial back where the device is.
    pub fn revert(&mut self) {
        if let Kind::Choice { live, picked, .. } = &mut self.kind {
            *picked = *live;
        }
    }
}

/// A tab's worth of rows, and where the cursor is in them.
#[derive(Clone, Debug)]
pub struct Page {
    pub rows: Vec<Row>,
    pub cursor: usize,
}

impl Page {
    pub fn new(rows: Vec<Row>) -> Self {
        let cursor = rows.iter().position(Row::selectable).unwrap_or(0);
        Page { rows, cursor }
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    /// Moves to the next row that can hold a cursor, skipping the facts.
    /// Stops at the ends rather than wrapping — a list that jumps from the
    /// bottom back to the top hides how long it is.
    pub fn step(&mut self, by: i32) {
        let mut at = self.cursor as i32;
        loop {
            at += by;
            if at < 0 || at as usize >= self.rows.len() {
                return;
            }
            if self.rows[at as usize].selectable() {
                self.cursor = at as usize;
                return;
            }
        }
    }

    pub fn turn_selected(&mut self, by: i32) {
        if let Some(row) = self.rows.get_mut(self.cursor) {
            row.turn(by);
        }
    }

    pub fn pending(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.pending()).collect()
    }

    pub fn revert_all(&mut self) {
        for row in &mut self.rows {
            row.revert();
        }
    }

    pub fn settle_all(&mut self) {
        for row in &mut self.rows {
            row.settle();
        }
    }
}

/// What is on screen on top of the menu, if anything.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    /// A is pressed and there is a queue. Lists it and waits.
    ConfirmApply,
    /// B is pressed with a queue outstanding. Leaving would throw it away, so
    /// it asks instead of silently discarding work.
    ConfirmDiscard,
    /// X on a row.
    Detail,
    /// The scripts are running, one after another.
    Applying { done: usize, total: usize },
}

/// The whole app, minus the drawing.
pub struct App {
    pub tab: Tab,
    pub sync: Page,
    pub patches: Page,
    pub overlay: Overlay,
    pub should_quit: bool,
}

impl App {
    pub fn page(&self) -> &Page {
        match self.tab {
            Tab::Sync => &self.sync,
            Tab::Patches => &self.patches,
        }
    }

    pub fn page_mut(&mut self) -> &mut Page {
        match self.tab {
            Tab::Sync => &mut self.sync,
            Tab::Patches => &mut self.patches,
        }
    }

    /// Everything queued, across both tabs — switching tabs does not abandon
    /// what you dialled up on the other one.
    pub fn queue(&self) -> Vec<&Row> {
        self.sync
            .rows
            .iter()
            .chain(self.patches.rows.iter())
            .filter(|r| r.pending())
            .collect()
    }

    pub fn next_tab(&mut self, by: i32) {
        let n = Tab::ALL.len() as i32;
        let at = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0) as i32;
        self.tab = Tab::ALL[(((at + by) % n + n) % n) as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page::new(vec![
            Row::fact("f", "Server", "connected"),
            Row::choice("a", "Hotkeys", "", &["ON", "OFF"], 0),
            Row::choice("b", "Driver", "", &["stock", "wayland"], 1),
        ])
    }

    #[test]
    fn the_cursor_starts_below_a_fact() {
        // A fact cannot be selected, so a page that opens with one has to put
        // the cursor on the first row that can be.
        assert_eq!(page().cursor, 1);
    }

    #[test]
    fn stepping_skips_facts_and_stops_at_the_ends() {
        let mut p = page();
        p.step(-1);
        assert_eq!(p.cursor, 1, "should not land on the fact above");
        p.step(1);
        assert_eq!(p.cursor, 2);
        p.step(1);
        assert_eq!(p.cursor, 2, "should not run off the bottom");
    }

    #[test]
    fn turning_wraps_both_ways() {
        let mut row = Row::choice("a", "t", "", &["ON", "OFF"], 0);
        row.turn(1);
        assert_eq!(row.value(), "OFF");
        row.turn(1);
        assert_eq!(row.value(), "ON", "two options should wrap round");
        row.turn(-1);
        assert_eq!(row.value(), "OFF", "and wrap backwards");
    }

    #[test]
    fn pending_is_the_difference_from_the_device() {
        let mut row = Row::choice("a", "t", "", &["ON", "OFF"], 0);
        assert!(!row.pending());
        row.turn(1);
        assert!(row.pending());
        assert_eq!(row.was(), Some("ON"), "the queue has to say what it was");
        row.settle();
        assert!(!row.pending(), "settling is what applying leaves behind");
        assert_eq!(row.was(), None);
    }

    #[test]
    fn reverting_puts_the_dial_back() {
        let mut p = page();
        p.turn_selected(1);
        assert_eq!(p.pending().len(), 1);
        p.revert_all();
        assert!(p.pending().is_empty());
    }

    #[test]
    fn a_fact_is_never_queued() {
        let mut row = Row::fact("f", "Server", "connected");
        row.turn(1);
        assert!(!row.pending());
        assert_eq!(row.value(), "connected");
    }
}
