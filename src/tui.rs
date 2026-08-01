//! Stage 2 — terminal library browser.
//!
//! Drill-down: platforms → games. Reads only the local SQLite cache, so it is
//! usable with the server unreachable. Enter launches a game when the ROM is
//! present locally.

use std::io::{Stdout, stdout};
use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::cache::{Cache, PlatformRow, RomRow};
use crate::coremap::CoreMap;
use crate::retroarch::RetroArch;

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(PartialEq)]
enum View {
    Platforms,
    Roms,
}

pub struct App {
    view: View,
    platforms: Vec<PlatformRow>,
    roms: Vec<RomRow>,
    /// Indices into `roms` surviving the current filter.
    filtered: Vec<usize>,
    platform_state: ListState,
    rom_state: ListState,
    filter: String,
    searching: bool,
    status: String,
    local_roms: PathBuf,
    ra: Option<RetroArch>,
    map: CoreMap,
    quit: bool,
}

impl App {
    pub fn new(
        cache: &Cache,
        local_roms: PathBuf,
        ra: Option<RetroArch>,
        map: CoreMap,
    ) -> Result<Self> {
        let platforms = cache.platforms()?;
        let mut platform_state = ListState::default();
        if !platforms.is_empty() {
            platform_state.select(Some(0));
        }
        let status = match &ra {
            Some(r) => format!("RetroArch: {}", r.root.display()),
            None => "RetroArch not found — launching disabled".to_owned(),
        };
        Ok(Self {
            view: View::Platforms,
            platforms,
            roms: Vec::new(),
            filtered: Vec::new(),
            platform_state,
            rom_state: ListState::default(),
            filter: String::new(),
            searching: false,
            status,
            local_roms,
            ra,
            map,
            quit: false,
        })
    }

    fn selected_platform(&self) -> Option<&PlatformRow> {
        self.platforms.get(self.platform_state.selected()?)
    }

    fn selected_rom(&self) -> Option<&RomRow> {
        let i = *self.filtered.get(self.rom_state.selected()?)?;
        self.roms.get(i)
    }

    /// Local path for a ROM, if it was staged into `library/`.
    fn local_path(&self, rom: &RomRow) -> Option<PathBuf> {
        let p = self.local_roms.join(&rom.platform_slug).join(&rom.fs_name);
        p.is_file().then_some(p)
    }

    fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.filtered = self
            .roms
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                needle.is_empty()
                    || r.name.to_lowercase().contains(&needle)
                    || r.fs_name.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        self.rom_state
            .select(if self.filtered.is_empty() { None } else { Some(0) });
    }

    fn enter_platform(&mut self, cache: &Cache) -> Result<()> {
        let Some(p) = self.selected_platform().cloned() else {
            return Ok(());
        };
        self.roms = cache.roms_for(&p.fs_slug)?;
        self.filter.clear();
        self.apply_filter();
        self.view = View::Roms;
        Ok(())
    }

    fn move_selection(&mut self, delta: isize) {
        let (state, len) = match self.view {
            View::Platforms => (&mut self.platform_state, self.platforms.len()),
            View::Roms => (&mut self.rom_state, self.filtered.len()),
        };
        if len == 0 {
            return;
        }
        let cur = state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1);
        state.select(Some(next as usize));
    }

    /// Resolve the core for a ROM, preferring the mapped default but falling
    /// back to any installed alternative.
    fn resolve_core(&self, ra: &RetroArch, platform: &str) -> Option<String> {
        if let Some(default) = self.map.default_core(platform)
            && ra.has_core(default)
        {
            return Some(default.to_owned());
        }
        self.map
            .alternatives(platform)
            .into_iter()
            .find(|c| ra.has_core(c))
            .map(str::to_owned)
    }

    fn launch(&mut self, term: &mut Term) -> Result<()> {
        let Some(rom) = self.selected_rom().cloned() else {
            return Ok(());
        };
        let Some(ra) = &self.ra else {
            self.status = "RetroArch not found".into();
            return Ok(());
        };
        let Some(path) = self.local_path(&rom) else {
            self.status = format!("not downloaded: {} (Stage 4 adds fetching)", rom.fs_name);
            return Ok(());
        };
        let Some(core) = self.resolve_core(ra, &rom.platform_slug) else {
            self.status = format!("no installed core for {}", rom.platform_slug);
            return Ok(());
        };

        // Hand the terminal back before spawning, or the emulator and the TUI
        // fight over it; restore afterwards.
        restore_terminal(term)?;
        let result = ra.launch(&core, &path, false);
        setup_terminal(term)?;

        self.status = match result {
            Ok(s) if s.success() => format!("{} exited cleanly", rom.name),
            Ok(s) => format!("{} exited with {}", rom.name, s),
            Err(e) => format!("launch failed: {e}"),
        };
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers, cache: &Cache, term: &mut Term)
        -> Result<()>
    {
        // Search box swallows most keys while active.
        if self.searching {
            match code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.filter.clear();
                    self.apply_filter();
                }
                KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.apply_filter();
                }
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.apply_filter();
                }
                _ => {}
            }
            return Ok(());
        }

        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home => self.move_selection(isize::MIN / 2),
            KeyCode::End => self.move_selection(isize::MAX / 2),
            KeyCode::Char('/') if self.view == View::Roms => {
                self.searching = true;
                self.filter.clear();
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.view {
                View::Platforms => self.enter_platform(cache)?,
                View::Roms => self.launch(term)?,
            },
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if self.view == View::Roms {
                    self.view = View::Platforms;
                    self.filter.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn human(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    for (i, u) in UNITS.iter().enumerate() {
        if v < 1024.0 || i == UNITS.len() - 1 {
            return format!("{v:.1} {u}");
        }
        v /= 1024.0;
    }
    unreachable!()
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(if app.searching { 3 } else { 0 }),
            Constraint::Length(3),
        ])
        .split(f.area());

    match app.view {
        View::Platforms => draw_platforms(f, app, chunks[0]),
        View::Roms => draw_roms(f, app, chunks[0]),
    }

    if app.searching {
        let p = Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(app.filter.clone()),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ]))
        .block(Block::default().borders(Borders::ALL).title(" search "));
        f.render_widget(p, chunks[1]);
    }

    let help = match app.view {
        View::Platforms => "↑↓/jk move   ⏎ open   q quit",
        View::Roms => "↑↓/jk move   ⏎ launch   / search   esc back   q quit",
    };
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(
            app.status.clone(),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(help, Style::default().fg(Color::DarkGray))),
    ])
    .wrap(Wrap { trim: true })
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[2]);
}

fn draw_platforms(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let total: i64 = app.platforms.iter().map(|p| p.rom_count).sum();
    let items: Vec<ListItem> = app
        .platforms
        .iter()
        .map(|p| {
            let playable = app
                .ra
                .as_ref()
                .and_then(|ra| app.resolve_core(ra, &p.fs_slug))
                .is_some();
            let mark = if playable { "●" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {mark} "),
                    Style::default().fg(if playable { Color::Green } else { Color::DarkGray }),
                ),
                Span::raw(format!("{:<24}", p.display_name)),
                Span::styled(
                    format!("{:>6}", p.rom_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            " platforms — {} games   ● = core installed ",
            total
        )))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, area, &mut app.platform_state);
}

fn draw_roms(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let slug = app
        .selected_platform()
        .map(|p| p.display_name.clone())
        .unwrap_or_default();

    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .filter_map(|&i| app.roms.get(i))
        .map(|r| {
            let local = app.local_path(r).is_some();
            let mark = if local { "▣" } else { "·" };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {mark} "),
                    Style::default().fg(if local { Color::Green } else { Color::DarkGray }),
                ),
                Span::raw(format!("{:<52}", truncate(&r.name, 52))),
                Span::styled(
                    format!("{:>10}", human(r.fs_size_bytes)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let title = format!(
        " {slug} — {} of {} shown   ▣ = downloaded ",
        app.filtered.len(),
        app.roms.len()
    );
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, area, &mut app.rom_state);
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn setup_terminal(term: &mut Term) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    term.hide_cursor()?;
    term.clear()?;
    Ok(())
}

fn restore_terminal(term: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    Ok(())
}

pub fn run(cache: &Cache, local_roms: &Path, ra: Option<RetroArch>, map: CoreMap) -> Result<()> {
    let mut app = App::new(cache, local_roms.to_path_buf(), ra, map)?;
    if app.platforms.is_empty() {
        anyhow::bail!("cache is empty — run `sync` first");
    }

    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    setup_terminal(&mut term)?;

    // Keep the terminal restorable even if drawing panics.
    let result = (|| -> Result<()> {
        while !app.quit {
            term.draw(|f| draw(f, &mut app))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.on_key(key.code, key.modifiers, cache, &mut term)?;
            }
        }
        Ok(())
    })();

    restore_terminal(&mut term)?;
    result
}
