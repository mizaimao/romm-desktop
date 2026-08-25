// What is on screen, and what the cursor is on.
//
// No drawing here and no SDL: this is the front end's state, and every rule it
// applies belongs to `romm_desktop`. Which order a list is in comes from
// `gamesort`, what a filter keeps from `gamefilter`, where the cursor goes next
// from `gridnav`, and what a key or a button means from `binds`. That was the
// point of doing task 1 first — this file is a few hundred lines because none
// of those decisions are taken twice.

use anyhow::{Context, Result};
use romm_desktop::cache::{Cache, CollectionRow};
use romm_desktop::gamelist::{self, Chosen, Row};
use romm_desktop::gridnav::{self, Moves};
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
    pub favorite: bool,
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
        if self.favorite {
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
/// The shoulder buttons cycle them — `prevSection` and `nextSection`, which is
/// why every one of them is in the list whether or not it draws anything yet.
/// A shoulder press that lands on a hole is worse than one that lands on a page
/// saying what will be there.
///
/// The order is the order of use, not the order they were built: Continue is
/// first because on a handheld the thing you want four times out of five is
/// the game you were already playing, and Settings is last because it is the
/// one you want least often.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub label: &'static str,
    /// Whether this tab draws its own content yet. One that does not says so on
    /// the page rather than showing an empty frame.
    pub ready: bool,
}

pub const SECTIONS: &[Section] = &[
    Section { id: "continue", label: "Continue", ready: true },
    Section { id: "library", label: "Library", ready: true },
    Section { id: "mine", label: "Collections", ready: true },
    Section { id: "settings", label: "Settings", ready: false },
    Section { id: "history", label: "History", ready: true },
    Section { id: "syncing", label: "Syncing", ready: true },
];

/// One game in the Continue playing strip.
pub struct Recent {
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub stem: String,
    pub downloaded: bool,
}

/// Which screen is showing, within whichever tab is open.
///
/// One enum for every tab rather than a place per tab, because only one list is
/// ever on screen: the cursor, the movement table and the scroll position are
/// all singular, and giving each tab its own copy is how they drift apart. Which
/// place a tab *starts* at is [`root_view`], and `back` walks up from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Library: the consoles.
    Platforms,
    /// Library or Collections: a list of games.
    Roms,
    /// Collections: the kinds — user, franchise, genre, company.
    Groups,
    /// Collections: the collections within one kind.
    Collections,
    /// History: everything played, most time first.
    History,
}

/// Where a tab starts when you arrive at it.
///
/// Switching tabs resets to this. Without it, leaving Library inside a console's
/// games and shouldering over to Collections arrives at a game list holding the
/// *console's* games under the Collections heading.
pub fn root_view(section: &str) -> View {
    match section {
        "mine" => View::Groups,
        "history" => View::History,
        _ => View::Platforms,
    }
}

/// RomM's group names, as something to read.
fn kind_label(group: &str) -> &'static str {
    match group {
        "collection" => "Series",
        "franchise" => "Franchises",
        "genre" => "Genres",
        "company" => "Companies",
        "mode" => "Modes",
        _ => "Other",
    }
}

/// One row on the Collections tab's first screen.
///
/// Your own collections and RomM's generated kinds share a list because they
/// are the same gesture — pick a shelf, get games. They are not the same
/// *depth*, though: yours open straight onto their games, and a kind opens onto
/// the collections inside it, because there are 1,022 company collections and
/// nobody wants those flattened into anything.
pub enum Shelf {
    /// One of yours. Opens its games.
    Mine { id: String, name: String, games: i64 },
    /// A kind of RomM collection. Opens the collections within it.
    Kind { group: String, label: &'static str, count: i64 },
}

impl Shelf {
    pub fn name(&self) -> &str {
        match self {
            Shelf::Mine { name, .. } => name,
            Shelf::Kind { label, .. } => label,
        }
    }

    /// The number on the right: games for one of yours, collections for a kind.
    pub fn count(&self) -> i64 {
        match self {
            Shelf::Mine { games, .. } => *games,
            Shelf::Kind { count, .. } => *count,
        }
    }
}

/// One game in the History list.
pub struct Played {
    pub name: String,
    pub platform: String,
    pub seconds: i64,
    pub runs: i64,
    /// When it was last started, as the server recorded it.
    pub last: String,
}

/// What the Syncing page can say without touching the network.
///
/// Read once at startup from the cache the other front ends fill. Nothing here
/// performs a sync — this is the state a sync would change, which is the honest
/// half to build first.
pub struct SyncState {
    pub server_version: Option<String>,
    pub watermark: Option<String>,
    pub games: i64,
    pub consoles: usize,
    pub collections: i64,
    /// From the play log. Named rather than a tuple because
    /// `Cache::play_totals` returns them in the order seconds, sessions,
    /// games — which read as games, sessions, seconds reports 6,065 games
    /// played for twenty seconds, and looks plausible enough to ship.
    pub seconds_played: i64,
    pub sessions: i64,
    pub games_played: i64,
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
    /// The Collections tab's first screen: yours flat, then RomM's kinds.
    pub shelves: Vec<Shelf>,
    pub shelf_at: usize,
    /// How many of `shelves` are yours — the divider goes after this many, and
    /// it is 0 when you have none.
    pub mine_count: usize,
    pub cols: Vec<CollectionRow>,
    pub col_at: usize,
    /// The open collection's name, for the header once its games are showing.
    pub col_name: String,
    pub history: Vec<Played>,
    pub history_at: usize,
    pub sync: SyncState,
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
    /// `romm_collections` is `[library] romm_collections` — whether RomM's
    /// 1,931 generated collections join yours on the Collections tab.
    pub fn open(
        path: &Path,
        media_root: PathBuf,
        roms_dir: PathBuf,
        romm_collections: bool,
    ) -> Result<Self> {
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
            shelves: Vec::new(),
            shelf_at: 0,
            mine_count: 0,
            cols: Vec::new(),
            col_at: 0,
            col_name: String::new(),
            history: Vec::new(),
            history_at: 0,
            sync: SyncState {
                server_version: None,
                watermark: None,
                games: 0,
                consoles: 0,
                collections: 0,
                seconds_played: 0,
                sessions: 0,
                games_played: 0,
            },
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

        // The kinds of collection, and everything the History and Syncing
        // pages show. All three are counts and short lists out of the same
        // cache, cheap enough to read once here rather than on arriving at a
        // tab — which would stall the shoulder button.
        lib.build_shelves(romm_collections);
        lib.history = lib
            .cache
            .play_by_game(200)
            .unwrap_or_default()
            .into_iter()
            .map(|(r, seconds, runs, last)| Played {
                name: r.name,
                platform: r.platform_slug,
                seconds,
                runs,
                last,
            })
            .collect();
        let totals = lib.cache.play_totals().unwrap_or((0, 0, 0));
        lib.sync = SyncState {
            server_version: lib.cache.server_version(),
            watermark: lib.cache.watermark(),
            games: lib.cache.rom_count().unwrap_or(0),
            consoles: lib.consoles.len(),
            collections: lib.cache.collection_count().unwrap_or(0),
            seconds_played: totals.0,
            sessions: totals.1,
            games_played: totals.2,
        };

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
    /// Move to a tab, and arrive at that tab's own root.
    ///
    /// The one way in. Setting `section` on its own leaves `view` pointing at
    /// wherever the last tab was, and the drawing switches on `view` — which is
    /// how Collections drew an empty game list instead of its list of kinds.
    pub fn go_to_section(&mut self, index: usize) {
        self.section = index.min(SECTIONS.len() - 1);
        self.view = root_view(self.section().id);
        self.scrolled = 0.0;
        self.columns = 0;
        self.relayout(1);
    }

    /// Which tab is open.
    ///
    /// By name rather than by index: the drawing switches on this, and an index
    /// would silently mean a different page the moment a tab is added in the
    /// middle.
    pub fn section(&self) -> &'static Section {
        SECTIONS.get(self.section).unwrap_or(&SECTIONS[0])
    }

    /// Whether Back would have nowhere to go — at which point it leaves.
    pub fn at_top(&self) -> bool {
        self.view == root_view(self.section().id)
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
            favorite: row.favorite,
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
        let fetched = self.cache.roms_for(&slug).with_context(|| format!("reading {slug}"))?;
        self.take_rows(fetched);
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

    /// Turn cache rows into the list this front end draws.
    ///
    /// Shared by a console and a collection: both are a list of games, and the
    /// only thing that differed was which query filled it.
    fn take_rows(&mut self, fetched: Vec<romm_desktop::cache::RomRow>) {
        let favorites = self.cache.favorite_ids().unwrap_or_default();
        let roms_dir = self.roms_dir.clone();
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
                favorite: favorites.contains(&r.id),
                // Whether it will play with the server off. The row's own path
                // if it has one, and the library folder otherwise — which is
                // where a download lands.
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
    }

    /// The Collections tab's first screen.
    ///
    /// Yours first and flat — you made them, they are few, and putting them
    /// behind a "Mine" folder costs a press every single time. Then RomM's
    /// generated kinds, which are 1,931 collections nobody browses by name and
    /// which `[library] romm_collections = false` removes outright.
    pub fn build_shelves(&mut self, romm: bool) {
        let mut mine = self.cache.collections_in("user").unwrap_or_default();
        // Alphabetical. The cache hands them back largest first, which is the
        // right order for a wall of cards you are browsing and the wrong one
        // for a list you are looking a known name up in — and yours is always
        // the second thing.
        mine.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.mine_count = mine.len();
        self.shelves = mine
            .into_iter()
            .map(|c| Shelf::Mine { id: c.id, name: c.name, games: c.rom_count })
            .collect();
        if !romm {
            return;
        }
        for (group, count) in self.cache.collection_groups().unwrap_or_default() {
            if group == "user" {
                continue;
            }
            self.shelves.push(Shelf::Kind { label: kind_label(&group), group, count });
        }
    }

    /// Open whatever is under the cursor on that first screen.
    ///
    /// One of yours goes straight to its games; a kind goes to the collections
    /// inside it. Two depths from one list, which is the point of `Shelf`.
    pub fn open_shelf(&mut self) -> Result<()> {
        let Some(shelf) = self.shelves.get(self.shelf_at) else { return Ok(()) };
        match shelf {
            Shelf::Mine { id, name, .. } => {
                let (id, name) = (id.clone(), name.clone());
                self.open_collection_id(&id, &name)
            }
            Shelf::Kind { group, .. } => {
                let group = group.clone();
                self.cols = self.cache.collections_in(&group).unwrap_or_default();
                self.view = View::Collections;
                self.col_at = 0;
                self.scrolled = 0.0;
                self.columns = 0;
                self.relayout(1);
                Ok(())
            }
        }
    }

    /// Open the collection under the cursor: its games become the list.
    ///
    /// The same `rows`/`arranged`/`at` the Library tab fills, so the sorting,
    /// the filters, the info pane and the drawing are all shared — a collection
    /// of games is a list of games.
    pub fn open_collection(&mut self) -> Result<()> {
        let Some(col) = self.cols.get(self.col_at) else { return Ok(()) };
        let (id, name) = (col.id.clone(), col.name.clone());
        self.open_collection_id(&id, &name)
    }

    /// Load one collection's games into the shared list.
    ///
    /// The same `rows`/`arranged`/`at` the Library tab fills, so the sorting,
    /// the filters, the info pane and the drawing are all shared — a collection
    /// of games is a list of games.
    fn open_collection_id(&mut self, id: &str, name: &str) -> Result<()> {
        let fetched = self
            .cache
            .roms_in_collection(id)
            .with_context(|| format!("reading collection {name}"))?;
        self.col_name = name.to_owned();
        self.take_rows(fetched);
        // A collection mixes consoles, so there is no one box shape to draw it
        // in. The default is the least wrong answer rather than a measured one.
        self.aspect = DEFAULT_ASPECT;
        self.view = View::Roms;
        self.at = 0;
        self.scrolled = 0.0;
        self.rearrange();
        Ok(())
    }

    /// Up one level, wherever that is for the tab you are in.
    ///
    /// The Library goes games -> consoles; Collections goes games ->
    /// collections -> kinds. `at_top` is what decides whether Back leaves the
    /// app instead, and it has to agree with this or Back either traps you or
    /// quits a level early.
    pub fn back(&mut self) {
        self.view = match (self.section().id, self.view) {
            ("mine", View::Roms) => View::Collections,
            ("mine", View::Collections) => View::Groups,
            (_, View::Roms) => View::Platforms,
            (id, _) => root_view(id),
        };
        self.scrolled = 0.0;
        // Rebuilt outright rather than through `relayout`, which skips the work
        // when the count looks unchanged — and a console list the same length
        // as the games list it came from is not impossible.
        self.columns = 0;
        self.relayout(1);
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
            View::Groups => self.shelves.len(),
            View::Collections => self.cols.len(),
            View::History => self.history.len(),
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
            View::Groups => &mut self.shelf_at,
            View::Collections => &mut self.col_at,
            View::History => &mut self.history_at,
            View::Roms => &mut self.at,
        };
        let count = match self.view {
            View::Platforms => self.consoles.len(),
            View::Groups => self.shelves.len(),
            View::Collections => self.cols.len(),
            View::History => self.history.len(),
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
            View::Groups => &mut self.shelf_at,
            View::Collections => &mut self.col_at,
            View::History => &mut self.history_at,
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
            "activate" => match self.view {
                View::Platforms => {
                    self.open_console()?;
                    Ok(true)
                }
                View::Groups => {
                    self.open_shelf()?;
                    Ok(true)
                }
                View::Collections => {
                    self.open_collection()?;
                    Ok(true)
                }
                // Nothing to open: History is already the games, and a game
                // list has no launcher behind it yet.
                View::Roms | View::History => Ok(false),
            },
            "back" | "back2" if !self.at_top() => {
                self.back();
                Ok(true)
            }
            // The shoulder buttons, and q/e. The cheapest navigation there is,
            // which is why they move between tabs rather than within one.
            "prevSection" | "nextSection" => {
                let delta = if action == "nextSection" { 1 } else { SECTIONS.len() - 1 };
                self.go_to_section((self.section + delta) % SECTIONS.len());
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
                favorite: *fav,
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
            shelves: Vec::new(),
            shelf_at: 0,
            mine_count: 0,
            cols: Vec::new(),
            col_at: 0,
            col_name: String::new(),
            history: Vec::new(),
            history_at: 0,
            sync: SyncState {
                server_version: None,
                watermark: None,
                games: 0,
                consoles: 0,
                collections: 0,
                seconds_played: 0,
                sessions: 0,
                games_played: 0,
            },
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

    /// Favorites on top, which is `gamesort`'s rule and not one of ours.
    #[test]
    fn favorites_lead_whatever_the_order() {
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
            true,
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
        assert_eq!(SECTIONS.len(), 6);
        assert_eq!(lib.section, 0);
        lib.act("nextSection").unwrap();
        assert_eq!(lib.section, 1);
        lib.act("prevSection").unwrap();
        assert_eq!(lib.section, 0);
        lib.act("prevSection").unwrap();
        assert_eq!(lib.section, SECTIONS.len() - 1, "going back from the first did not wrap");
    }

    /// The six tabs, in the order they were asked for.
    ///
    /// Named rather than counted: a test that only checks the length passes
    /// while the tabs say something else entirely, and the drawing switches on
    /// these ids.
    #[test]
    fn the_tabs_are_the_six_the_panel_was_designed_around() {
        let ids: Vec<_> = SECTIONS.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["continue", "library", "mine", "settings", "history", "syncing"]);
    }

    /// The shoulders reach every tab, including the ones with nothing behind
    /// them yet — a tab they skip is one nobody can see is coming.
    #[test]
    fn the_shoulders_reach_every_tab_and_wrap() {
        let mut lib = seeded(rows(&[("a", false)]));
        let mut seen = vec![lib.section];
        for _ in 1..SECTIONS.len() {
            lib.act("nextSection").unwrap();
            seen.push(lib.section);
        }
        assert_eq!(seen, (0..SECTIONS.len()).collect::<Vec<_>>(), "a tab was skipped");
        lib.act("nextSection").unwrap();
        assert_eq!(lib.section, 0, "the last tab did not wrap round to the first");
    }

    /// Every tab arrives at its own root, whatever the last one was showing.
    ///
    /// The bug this pins: `ROMM_SDL_TAB=mine` set the section without touching
    /// the view, so Collections opened at `Platforms`, fell through the
    /// drawing's match to the game-list arm, and rendered an empty list and an
    /// empty info pane under a "— 0 games" heading.
    #[test]
    fn arriving_at_a_tab_lands_on_its_own_root() {
        let mut lib = seeded(rows(&[("a", false)]));
        assert_eq!(lib.view, View::Roms, "the fixture starts inside a console");
        for (i, want) in [
            (0, View::Platforms),
            (1, View::Platforms),
            (2, View::Groups),
            (3, View::Platforms),
            (4, View::History),
            (5, View::Platforms),
        ] {
            lib.go_to_section(i);
            assert_eq!(lib.view, want, "{} landed wrong", SECTIONS[i].id);
        }
    }

    /// Back walks up the tab it is in, and says when there is nowhere left.
    ///
    /// Collections is three deep and the Library two, so a single shared rule
    /// would either trap you in Collections or quit a level early out of it.
    #[test]
    fn back_walks_up_the_tab_it_is_in() {
        let mut lib = seeded(rows(&[("a", false)]));

        lib.go_to_section(2);
        assert!(lib.at_top(), "the kinds are the top of Collections");
        lib.view = View::Collections;
        assert!(!lib.at_top());
        lib.back();
        assert_eq!(lib.view, View::Groups, "collections should go up to the kinds");
        assert!(lib.at_top());

        lib.view = View::Roms;
        lib.back();
        assert_eq!(lib.view, View::Collections, "a collection's games go up to the collections");

        lib.go_to_section(1);
        lib.view = View::Roms;
        lib.back();
        assert_eq!(lib.view, View::Platforms, "a console's games go up to the consoles");
        assert!(lib.at_top());
    }

    /// Yours first and flat, then RomM's kinds — and `mine_count` is the join
    /// between them, which is where the divider goes.
    #[test]
    fn the_shelf_puts_yours_first_and_flat() {
        let mut lib = seeded(rows(&[("a", false)]));
        lib.shelves = vec![
            Shelf::Mine { id: "1".into(), name: "Best of nes".into(), games: 75 },
            Shelf::Kind { group: "company".into(), label: "Companies", count: 1022 },
        ];
        lib.mine_count = 1;
        assert_eq!(lib.shelves[0].name(), "Best of nes");
        assert_eq!(lib.shelves[0].count(), 75, "yours count games");
        assert_eq!(lib.shelves[1].name(), "Companies");
        assert_eq!(lib.shelves[1].count(), 1022, "a kind counts collections, not games");
    }

    /// `[library] romm_collections = false` leaves only yours, and then there
    /// is no divider to draw — `mine_count` equals the whole list.
    #[test]
    fn turning_romm_collections_off_leaves_only_yours() {
        let mut lib = seeded(rows(&[("a", false)]));
        lib.build_shelves(false);
        assert_eq!(
            lib.mine_count,
            lib.shelves.len(),
            "a kind survived with RomM collections turned off"
        );
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
