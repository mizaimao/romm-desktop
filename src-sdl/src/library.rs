// What is on screen, and what the cursor is on.
//
// No drawing here and no SDL: this is the front end's state, and every rule it
// applies belongs to `romm_desktop`. Which order a list is in comes from
// `gamesort`, what a filter keeps from `gamefilter`, where the cursor goes next
// from `gridnav`, and what a key or a button means from `binds`. That was the
// point of doing task 1 first — this file is a few hundred lines because none
// of those decisions are taken twice.

use anyhow::{Context, Result};
use romm_desktop::cache::Cache;
use romm_desktop::gamelist::{self, Chosen, Row};
use romm_desktop::gridnav::{self, Moves};
use romm_desktop::layout::Panes;
use romm_desktop::{gamefilter, gamesort};
use std::path::{Path, PathBuf};

/// What a cover is shaped like before anything has been measured. Three by
/// four, which is most console box art and all of the placeholder cards.
pub const DEFAULT_ASPECT: f32 = 0.75;

/// Everything the preview column says about one game.
pub struct Detail {
    pub id: i64,
    pub name: String,
    pub platform: String,
    /// What its artwork is filed under.
    pub stem: String,
    pub size: String,
    pub favourite: bool,
    pub last_played: Option<String>,
    pub downloaded: bool,
    pub hardware: Option<&'static str>,
    pub maker: Option<&'static str>,
}

impl Detail {
    /// The pane's rows, as label and value. Skipping what is not known rather
    /// than printing a blank: a field with nothing in it is a question the
    /// pane cannot answer, and a dash is not an answer.
    pub fn facts(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![("Console", self.platform.clone()), ("Size", self.size.clone())];
        if self.favourite {
            out.push(("Starred", "yes".to_owned()));
        }
        if let Some(maker) = self.maker {
            out.push(("Made by", maker.to_owned()));
        }
        if let Some(hardware) = self.hardware {
            out.push(("Hardware", hardware.to_owned()));
        }
        match &self.last_played {
            // The date only. The time it was started is not something anybody
            // is looking for here.
            Some(when) => out.push(("Last played", when.chars().take(10).collect())),
            None => out.push(("Last played", "never".to_owned())),
        }
        out.push(("On this machine", if self.downloaded { "yes" } else { "no" }.to_owned()));
        out
    }
}

/// The tabs across the top.
///
/// The same five the webview has, in the same order and with the same names —
/// `ui/js/tabs.js`. Only the first two do anything yet; the rest are drawn
/// because a tab row with holes in it is worse than one with tabs that are
/// not finished, and because the shoulder buttons move between them and have
/// to have somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub label: &'static str,
    pub ready: bool,
}

pub const SECTIONS: &[Section] = &[
    Section { id: "library", label: "Library", ready: true },
    Section { id: "mine", label: "My collections", ready: false },
    Section { id: "history", label: "History", ready: false },
    Section { id: "browse", label: "RomM browse", ready: false },
];

/// One game in the Continue playing strip.
pub struct Recent {
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub stem: String,
    pub downloaded: bool,
}

/// How the window is arranged.
///
/// The choice `ui/js/shell.js` calls Sofa and Desk, and it is a preference
/// rather than a measurement: the window's width is a ceiling — see
/// `layout::Panes::at_most` — and this is what is wanted under it.
///
/// Sofa is one pane at a time with a back button, which is what a handheld
/// always gets and what a television across a room wants: big tiles, one thing
/// to look at. Desk is the columns, which wants a mouse and a monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Sofa,
    Desk,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Sofa => "Sofa",
            Mode::Desk => "Desk",
        }
    }

    /// The most panes this arrangement ever wants.
    pub fn panes(self) -> Panes {
        match self {
            Mode::Sofa => Panes::One,
            Mode::Desk => Panes::Three,
        }
    }

    pub fn other(self) -> Mode {
        match self {
            Mode::Sofa => Mode::Desk,
            Mode::Desk => Mode::Sofa,
        }
    }
}

pub const MODES: [Mode; 2] = [Mode::Sofa, Mode::Desk];

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The consoles.
    Platforms,
    /// One console's games.
    Roms,
}

pub struct Console {
    pub slug: String,
    pub name: String,
    pub games: i64,
}

pub struct Library {
    cache: Cache,
    media_root: PathBuf,
    /// Where downloads land, for deciding what will play offline.
    roms_dir: PathBuf,
    pub consoles: Vec<Console>,
    /// Which console the picker is on, always — the picker column shows it
    /// highlighted whether or not its games are open.
    pub console_at: usize,
    pub view: View,
    /// The games of the open console, unsorted and unfiltered. What `arranged`
    /// holds is which of them to draw, in what order.
    rows: Vec<Row>,
    /// The ROM filename without its extension, per row, in the same order.
    ///
    /// What the artwork on disk is named after — see `media::local_art`. Kept
    /// beside the rows rather than on them because `gamelist::Row` is the
    /// shape a *list* needs, and where a file sits is not part of that.
    stems: Vec<String>,
    arranged: Vec<usize>,
    /// Which tab is open.
    pub section: usize,
    /// Sofa or Desk. Not what the window can hold — what is wanted.
    pub mode: Mode,
    recent: Vec<Recent>,
    /// Where the cursor is, as a place in `arranged`.
    pub at: usize,
    /// The shape of this console's box art, width over height.
    ///
    /// Measured off what is actually on disk — a PSP UMD case is 0.58 and a
    /// SNES box 1.37, and one ratio for all of them crops most systems. Per
    /// console rather than per game so the grid stays regular, which is also
    /// what lets the cursor move by arithmetic.
    pub aspect: f32,
    /// How far the grid is scrolled, in points. Owned here rather than by the
    /// drawing so it survives a redraw and a resize.
    pub scrolled: f32,
    chosen: Chosen,
    /// Where each drawn row leads, worked out once per relayout.
    moves: Moves,
    columns: usize,
}

impl Library {
    /// Open the metadata cache and read the consoles.
    pub fn open(path: &Path, media_root: PathBuf, roms_dir: PathBuf) -> Result<Self> {
        let cache = Cache::open(path).with_context(|| format!("opening {}", path.display()))?;
        let consoles = cache
            .platforms()
            .context("reading the consoles")?
            .into_iter()
            .map(|p| Console { slug: p.fs_slug, name: p.display_name, games: p.rom_count })
            .collect();
        let mut lib = Library {
            cache,
            media_root,
            roms_dir,
            consoles,
            console_at: 0,
            view: View::Platforms,
            rows: Vec::new(),
            stems: Vec::new(),
            arranged: Vec::new(),
            section: 0,
            mode: Mode::Sofa,
            recent: Vec::new(),
            at: 0,
            aspect: DEFAULT_ASPECT,
            scrolled: 0.0,
            chosen: Chosen::default(),
            moves: Moves::default(),
            columns: 1,
        };
        // One more than fits in the strip, so it can say there is a "more"
        // without a second query.
        lib.recent = lib
            .cache
            .recently_played(21)
            .unwrap_or_default()
            .into_iter()
            .map(|r| Recent {
                downloaded: lib.roms_dir.join(&r.platform_slug).join(&r.fs_name).exists(),
                stem: Path::new(&r.fs_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| r.fs_name.clone()),
                id: r.id,
                name: r.name,
                platform: r.platform_slug,
            })
            .collect();

        // The console list is a list before it is anything else, and the
        // cursor has to be able to move on it. Without this the move table
        // stayed empty until a console was opened — which needs the cursor.
        lib.moves = gridnav::uniform(lib.consoles.len(), 1);
        Ok(lib)
    }

    /// The games played most recently, for the strip above the consoles.
    ///
    /// Server timestamps, so it is the same list on every machine — the point
    /// is picking up where you left off, and that is rarely the machine you
    /// are sitting at now.
    pub fn recent(&self) -> &[Recent] {
        &self.recent
    }

    /// Whether Back has anywhere left to go.
    ///
    /// On the console list it has not, and on a handheld with no window to
    /// close that is where Back means leave.
    pub fn at_top(&self) -> bool {
        self.view == View::Platforms
    }

    pub fn console(&self) -> Option<&Console> {
        self.consoles.get(self.console_at)
    }

    /// The games to draw, in the order to draw them, each with the name its
    /// artwork is filed under.
    pub fn showing(&self) -> impl Iterator<Item = (&Row, &str)> {
        self.arranged.iter().filter_map(|i| {
            Some((self.rows.get(*i)?, self.stems.get(*i).map(String::as_str).unwrap_or("")))
        })
    }

    pub fn shown(&self) -> usize {
        self.arranged.len()
    }

    /// What the third column shows about the game under the cursor.
    ///
    /// Assembled here rather than by the drawing, so the pane is a list of
    /// facts to lay out rather than a place where things are worked out. What
    /// is *in* the list comes from the row and from `platformfacts`, both
    /// already in the core.
    pub fn detail(&self) -> Option<Detail> {
        let row = self.selected()?;
        let stem = self.arranged.get(self.at).and_then(|i| self.stems.get(*i))?;
        Some(Detail {
            name: row.name.clone(),
            platform: row.platform.clone(),
            stem: stem.clone(),
            id: row.id,
            size: romm_desktop::util::human(row.size_bytes as u64),
            favourite: row.favourite,
            last_played: row.last_played.clone(),
            downloaded: row.downloaded,
            hardware: romm_desktop::platformfacts::of(&row.platform).map(|f| f.hardware),
            maker: romm_desktop::platformfacts::of(&row.platform).map(|f| f.manufacturer),
        })
    }

    pub fn selected(&self) -> Option<&Row> {
        self.arranged.get(self.at).and_then(|i| self.rows.get(*i))
    }

    /// The list the sort and the filters belong to. See `gamelist::scope` —
    /// "order this console by rating" is a statement about that console.
    fn scope(&self) -> String {
        gamelist::scope("roms", self.console().map(|c| c.slug.as_str()), None)
    }

    pub fn order_label(&self) -> &'static str {
        self.chosen.order(&self.scope()).label
    }

    pub fn filters(&self) -> Vec<String> {
        self.chosen.filters(&self.scope()).into_iter().collect()
    }

    /// Open the console the picker is on.
    pub fn open_console(&mut self) -> Result<()> {
        let Some(slug) = self.console().map(|c| c.slug.clone()) else { return Ok(()) };
        let favourites = self.cache.favourite_ids().unwrap_or_default();
        let roms_dir = self.roms_dir.clone();
        let fetched = self.cache.roms_for(&slug).with_context(|| format!("reading {slug}"))?;
        self.stems = fetched
            .iter()
            .map(|r| {
                Path::new(&r.fs_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| r.fs_name.clone())
            })
            .collect();
        self.rows = fetched
            .into_iter()
            .map(|r| Row {
                favourite: favourites.contains(&r.id),
                // Whether it will play with the server off. The row's own
                // path if it has one, and the library folder otherwise —
                // which is where a download lands.
                downloaded: r
                    .local_path
                    .as_deref()
                    .map(Path::new)
                    .filter(|p| p.exists())
                    .is_some()
                    || roms_dir.join(&r.platform_slug).join(&r.fs_name).exists(),
                id: r.id,
                name: r.name,
                platform: r.platform_slug,
                size_bytes: r.fs_size_bytes,
                last_played: r.last_played,
                // The rest lives in a metadata blob this front end does not
                // parse yet. Absent is a real answer for all three — a game
                // with no rating sorts last rather than first.
                ..Row::default()
            })
            .collect();
        self.aspect = romm_desktop::media::cover_aspect(&self.media_root, &slug)
            .filter(|a| a.is_finite() && *a > 0.1 && *a < 4.0)
            .unwrap_or(DEFAULT_ASPECT);
        self.view = View::Roms;
        self.at = 0;
        self.scrolled = 0.0;
        // rearrange relayouts, but only after the view has changed — the
        // table is built for whichever list is showing.
        self.rearrange();
        Ok(())
    }

    pub fn back(&mut self) {
        self.view = View::Platforms;
        // Rebuilt outright rather than through `relayout`, which skips the
        // work when the count looks unchanged — and a console list the same
        // length as the games grid it came from is not impossible.
        self.columns = 1;
        self.moves = gridnav::uniform(self.consoles.len(), 1);
    }

    /// Work out which rows survive the filters and what order they go in.
    fn rearrange(&mut self) {
        let scope = self.scope();
        let order = self.chosen.order(&scope).id;
        let filters = self.chosen.filters(&scope);
        let ids = gamelist::arrange(&self.rows, order, &filters);
        // `arrange` answers in ids because that is what a front end holding
        // its own rows wants; this one wants places in its own array.
        let mut by_id = std::collections::HashMap::with_capacity(self.rows.len());
        for (i, row) in self.rows.iter().enumerate() {
            by_id.insert(row.id, i);
        }
        self.arranged = ids.into_iter().filter_map(|id| by_id.get(&id).copied()).collect();
        self.at = self.at.min(self.arranged.len().saturating_sub(1));
        self.moves = gridnav::uniform(self.arranged.len(), self.columns);
    }

    /// Tell it how wide the grid came out, so the cursor knows what is beside
    /// what.
    ///
    /// A uniform grid needs no geometry — `gridnav::uniform` works it out from
    /// two numbers, which is also why it can answer for rows that were never
    /// drawn.
    pub fn relayout(&mut self, columns: usize) {
        let columns = columns.max(1);
        let count = match self.view {
            View::Platforms => self.consoles.len(),
            View::Roms => self.arranged.len(),
        };
        // Only when it would come out different. This is called from the
        // drawing, so it runs every frame, and the table is six vectors the
        // length of the list — 15,000 allocations a frame on the arcade
        // console for an answer that changes when the window is resized.
        if columns == self.columns && count == self.moves.down.len() {
            return;
        }
        self.columns = columns;
        self.moves = gridnav::uniform(count, columns);
    }

    /// Put the cursor on a particular row, if it is one.
    ///
    /// For the mouse, which does not step — it arrives.
    pub fn point_at(&mut self, index: usize) -> bool {
        let cursor = match self.view {
            View::Platforms => &mut self.console_at,
            View::Roms => &mut self.at,
        };
        let count = match self.view {
            View::Platforms => self.consoles.len(),
            View::Roms => self.arranged.len(),
        };
        if index >= count || *cursor == index {
            return false;
        }
        *cursor = index;
        true
    }

    /// Do what a key or a button asked for. Returns whether anything changed.
    pub fn act(&mut self, action: &str) -> Result<bool> {
        let cursor = match self.view {
            View::Platforms => &mut self.console_at,
            View::Roms => &mut self.at,
        };
        let moved = |table: &[Option<usize>], at: usize| table.get(at).copied().flatten();

        let next = match action {
            "left" => moved(&self.moves.left, *cursor),
            "right" => moved(&self.moves.right, *cursor),
            "up" => moved(&self.moves.up, *cursor),
            "down" => moved(&self.moves.down, *cursor),
            "pageUp" => moved(&self.moves.page_up, *cursor),
            "pageDown" => moved(&self.moves.page_down, *cursor),
            "first" => self.moves.first,
            "last" => self.moves.last,
            _ => None,
        };
        if let Some(to) = next {
            *cursor = to;
            return Ok(true);
        }

        match action {
            "activate" if self.view == View::Platforms => {
                self.open_console()?;
                Ok(true)
            }
            "back" | "back2" if self.view == View::Roms => {
                self.back();
                self.relayout(self.columns);
                Ok(true)
            }
            // Sofa or Desk. The one control that changes the shape of the
            // whole window, so it is on a binding of its own rather than
            // buried in a menu.
            "layout" => {
                self.mode = self.mode.other();
                Ok(true)
            }
            // The shoulder buttons, and q/e. The cheapest navigation there is,
            // which is why they move between tabs rather than within one.
            "prevSection" | "nextSection" => {
                let delta = if action == "nextSection" { 1 } else { SECTIONS.len() - 1 };
                self.section = (self.section + delta) % SECTIONS.len();
                Ok(true)
            }
            "sortCycle" if self.view == View::Roms => {
                let scope = self.scope();
                let next = gamesort::cycle(self.chosen.order(&scope).id, 1);
                self.chosen.set_order(&scope, next.id);
                self.rearrange();
                Ok(true)
            }
            "filterMenu" if self.view == View::Roms => {
                // No menu yet. Stepping through the filters one press at a
                // time is enough to prove they are the core's filters and not
                // a second set — the menu is a later phase.
                let scope = self.scope();
                let on = self.chosen.filters(&scope);
                let next = gamefilter::FILTERS
                    .iter()
                    .position(|f| on.contains(f.id))
                    .map(|i| (i + 1) % (gamefilter::FILTERS.len() + 1))
                    .unwrap_or(0);
                self.chosen.clear_filters(&scope);
                if let Some(f) = gamefilter::FILTERS.get(next) {
                    self.chosen.toggle_filter(&scope, f.id);
                }
                self.rearrange();
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(names: &[(&str, bool)]) -> Vec<Row> {
        names
            .iter()
            .enumerate()
            .map(|(i, (name, fav))| Row {
                id: i as i64 + 1,
                name: (*name).to_owned(),
                favourite: *fav,
                ..Row::default()
            })
            .collect()
    }

    /// A library with rows already in it, so the tests need no database.
    fn seeded(rows: Vec<Row>) -> Library {
        let mut lib = Library {
            cache: Cache::open(Path::new(":memory:")).expect("an in-memory cache"),
            media_root: PathBuf::new(),
            roms_dir: PathBuf::new(),
            consoles: vec![Console { slug: "snes".into(), name: "SNES".into(), games: 3 }],
            console_at: 0,
            view: View::Roms,
            stems: rows.iter().map(|r| r.name.clone()).collect(),
            rows,
            arranged: Vec::new(),
            section: 0,
            mode: Mode::Sofa,
            recent: Vec::new(),
            at: 0,
            aspect: DEFAULT_ASPECT,
            scrolled: 0.0,
            chosen: Chosen::default(),
            moves: Moves::default(),
            columns: 1,
        };
        lib.rearrange();
        lib
    }

    #[test]
    fn a_list_comes_out_in_the_cores_order() {
        let lib = seeded(rows(&[("Zelda", false), ("asteroids", false), ("Metroid", false)]));
        let names: Vec<_> = lib.showing().map(|(r, _)| r.name.as_str()).collect();
        assert_eq!(names, ["asteroids", "Metroid", "Zelda"]);
    }

    /// Favourites on top, which is `gamesort`'s rule and not one of ours.
    #[test]
    fn favourites_lead_whatever_the_order() {
        let lib = seeded(rows(&[("Alpha", false), ("Zebra", true)]));
        assert_eq!(lib.showing().next().map(|(r, _)| r.name.as_str()), Some("Zebra"));
    }

    #[test]
    fn the_cursor_moves_by_the_grid_it_was_told_about() {
        let mut lib = seeded(rows(&[
            ("a", false), ("b", false), ("c", false), ("d", false),
            ("e", false), ("f", false), ("g", false),
        ]));
        lib.relayout(3);
        assert!(lib.act("right").unwrap());
        assert_eq!(lib.at, 1);
        assert!(lib.act("down").unwrap());
        assert_eq!(lib.at, 4, "down did not move a whole row");
        assert!(lib.act("last").unwrap());
        assert_eq!(lib.at, 6);
        // And stops rather than wrapping.
        assert!(!lib.act("down").unwrap(), "the cursor ran off the bottom");
    }

    /// Cycling the sort is `gamesort::cycle`, and it belongs to this console.
    #[test]
    fn cycling_the_sort_changes_the_order_and_says_so() {
        let mut lib = seeded(rows(&[("b", false), ("a", false)]));
        assert_eq!(lib.order_label(), "Name");
        lib.act("sortCycle").unwrap();
        assert_eq!(lib.order_label(), "Rating");
    }

    /// The filters narrow what is shown, and the cursor cannot be left past
    /// the end of what is left.
    #[test]
    fn filtering_narrows_the_list_and_brings_the_cursor_back() {
        let mut lib = seeded(rows(&[("a", false), ("b", true), ("c", false)]));
        lib.at = 2;
        // The third filter is "Starred".
        lib.act("filterMenu").unwrap();
        lib.act("filterMenu").unwrap();
        lib.act("filterMenu").unwrap();
        assert_eq!(lib.filters(), ["fav"]);
        assert_eq!(lib.shown(), 1);
        assert!(lib.at < lib.shown(), "the cursor was left past the end");
    }

    /// End to end, against the cache the other front ends read.
    ///
    /// Skipped where there is no cache — CI has none — but on a machine that
    /// has one this is the test that would have caught the console list
    /// drawing and nothing else happening.
    #[test]
    fn a_real_console_opens_and_has_games_in_it() {
        // `cargo test` runs from the crate directory, not the workspace root.
        let db = Path::new("../cache.sqlite3");
        if !db.is_file() {
            eprintln!("no cache.sqlite3 here; skipping");
            return;
        }
        let mut lib = Library::open(
            db,
            PathBuf::from("../library/downloaded_media"),
            PathBuf::from("../library/roms"),
        )
        .expect("opening the cache");
        assert!(!lib.consoles.is_empty(), "a cache with no consoles in it");

        // The first console with anything in it, since a library can hold
        // empty ones.
        let at = lib
            .consoles
            .iter()
            .position(|c| c.games > 0)
            .expect("no console has any games");
        lib.console_at = at;
        lib.act("activate").expect("opening a console");

        assert_eq!(lib.view, View::Roms, "activate did not open the console");
        assert!(lib.shown() > 0, "the console opened with nothing in it");
        assert!(lib.selected().is_some(), "nothing is under the cursor");
    }

    /// The console list has to be navigable before anything is opened.
    ///
    /// It was not: the move table was only ever worked out for the games
    /// grid, so on the console screen every direction did nothing and the
    /// cursor sat on whatever was first.
    #[test]
    fn the_console_cursor_moves_before_a_console_is_opened() {
        let mut lib = seeded(rows(&[("a", false)]));
        lib.view = View::Platforms;
        lib.consoles = vec![
            Console { slug: "a".into(), name: "A".into(), games: 1 },
            Console { slug: "b".into(), name: "B".into(), games: 2 },
            Console { slug: "c".into(), name: "C".into(), games: 3 },
        ];
        lib.relayout(1);
        assert!(lib.act("down").unwrap(), "down did nothing on the console list");
        assert_eq!(lib.console_at, 1);
        assert!(lib.act("last").unwrap());
        assert_eq!(lib.console_at, 2);
    }

    /// The mouse arrives rather than stepping, so it must not be able to land
    /// outside the list — a pointer over the gap below the last row is a
    /// common thing and must not move the cursor there.
    #[test]
    fn pointing_outside_the_list_moves_nothing() {
        let mut lib = seeded(rows(&[("a", false), ("b", false)]));
        assert!(lib.point_at(1));
        assert_eq!(lib.at, 1);
        assert!(!lib.point_at(9), "the cursor followed the pointer off the end");
        assert_eq!(lib.at, 1);
        assert!(!lib.point_at(1), "pointing at where it already is counts as a change");
    }

    /// The tabs wrap, both ways, and there are as many as the webview has.
    #[test]
    fn the_shoulders_walk_the_tabs_and_wrap() {
        let mut lib = seeded(rows(&[("a", false)]));
        assert_eq!(SECTIONS.len(), 4);
        assert_eq!(lib.section, 0);
        lib.act("nextSection").unwrap();
        assert_eq!(lib.section, 1);
        lib.act("prevSection").unwrap();
        assert_eq!(lib.section, 0);
        lib.act("prevSection").unwrap();
        assert_eq!(lib.section, SECTIONS.len() - 1, "going back from the first did not wrap");
    }

    /// The window's width is a ceiling and the preference lives under it, so
    /// a narrow window asking for Desk still gets one pane — and the handheld
    /// never gets three however it is set.
    #[test]
    fn the_arrangement_is_wanted_but_the_window_decides() {
        let mut lib = seeded(rows(&[("a", false)]));
        assert_eq!(lib.mode, Mode::Sofa);
        lib.act("layout").unwrap();
        assert_eq!(lib.mode, Mode::Desk);

        let wide = Panes::fitting(1400.0);
        let pocket = Panes::fitting(640.0);
        assert_eq!(wide.at_most(lib.mode.panes()), Panes::Three);
        assert_eq!(pocket.at_most(lib.mode.panes()), Panes::One, "the handheld got columns");
        lib.act("layout").unwrap();
        assert_eq!(wide.at_most(lib.mode.panes()), Panes::One, "Sofa is one pane on any screen");
    }

    #[test]
    fn back_returns_to_the_consoles() {
        let mut lib = seeded(rows(&[("a", false)]));
        assert!(!lib.at_top(), "a console's games are not the top level");
        assert!(lib.act("back").unwrap());
        assert_eq!(lib.view, View::Platforms);
        assert!(lib.at_top(), "back left us somewhere that is not the top");
    }
}
