//! Drawing. Everything here reads the model and writes pixels; nothing here
//! decides anything.
//!
//! Sizes are in points against a 640×480 panel, because that is the screen
//! this runs on and pretending otherwise is how the last front end ended up
//! being redrawn three times.

use romm_desktop::layout::Rect;
use romm_sdl::gfx::Gfx;
use romm_sdl::text::{Painter, Spec};

use crate::model::{App, Kind, Overlay, Row, Tab};

pub const PANEL: (u32, u32) = (640, 480);

/// One place for the palette, so a row that is "changed" is the same colour
/// everywhere it appears — in the list, in the counter and in the confirmation.
pub mod ink {
    use romm_sdl::gfx::Rgba;
    pub const BACKDROP: Rgba = Rgba(0.055, 0.063, 0.086, 1.0);
    pub const BAR: Rgba = Rgba(0.094, 0.106, 0.141, 1.0);
    pub const LINE: Rgba = Rgba(1.0, 1.0, 1.0, 0.08);
    pub const TEXT: Rgba = Rgba(0.88, 0.90, 0.94, 1.0);
    pub const DIM: Rgba = Rgba(0.55, 0.58, 0.65, 1.0);
    pub const FAINT: Rgba = Rgba(0.38, 0.41, 0.48, 1.0);
    pub const PICKED: Rgba = Rgba(0.20, 0.24, 0.34, 1.0);
    pub const ACCENT: Rgba = Rgba(0.55, 0.78, 0.45, 1.0);
    /// Queued but not run. Deliberately not the accent colour: "chosen" and
    /// "actually true on the device" must never look the same.
    pub const QUEUED: Rgba = Rgba(0.95, 0.72, 0.32, 1.0);
    pub const SHADE: Rgba = Rgba(0.0, 0.0, 0.0, 0.72);
}

mod size {
    pub const NAME_BAR: f32 = 26.0;
    pub const TAB_BAR: f32 = 26.0;
    pub const HELP: f32 = 22.0;
    pub const ROW: f32 = 30.0;
    pub const PAD: f32 = 12.0;
    pub const TEXT: f32 = 12.0;
    pub const SMALL: f32 = 10.0;
}

pub struct Ui {
    pub scale: f32,
    /// Which row is at the top of the visible window. Kept here rather than in
    /// the model because it is a fact about the screen, not about the patches.
    pub first: usize,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { scale: 1.0, first: 0 }
    }
}

impl Ui {
    fn spec(&self, text: &str, points: f32) -> Spec {
        Spec::new(text, points, self.scale)
    }

    pub fn draw(&mut self, gfx: &Gfx, painter: &mut Painter, app: &App) {
        let (w, h) = (PANEL.0 as f32, PANEL.1 as f32);
        gfx.clear(ink::BACKDROP);

        self.name_bar(gfx, painter, w);
        self.tab_bar(gfx, painter, w, app);

        let top = size::NAME_BAR + size::TAB_BAR;
        let body = Rect::new(0.0, top, w, h - top - size::HELP);
        self.rows(gfx, painter, body, app);
        self.help(gfx, painter, Rect::new(0.0, h - size::HELP, w, size::HELP), app);

        match &app.overlay {
            Overlay::None => {}
            Overlay::Detail => self.detail(gfx, painter, app),
            Overlay::ConfirmApply => self.confirm_apply(gfx, painter, app),
            Overlay::ConfirmDiscard => self.confirm_discard(gfx, painter, app),
            Overlay::Applying { done, total } => {
                self.applying(gfx, painter, *done, *total)
            }
        }
    }

    fn name_bar(&self, gfx: &Gfx, painter: &mut Painter, w: f32) {
        gfx.fill(Rect::new(0.0, 0.0, w, size::NAME_BAR), ink::BAR);
        let at = Rect::new(size::PAD, 6.0, w - size::PAD * 2.0, size::NAME_BAR);
        painter.put(gfx, &self.spec("moose-patch", size::TEXT), at, ink::TEXT);
    }

    fn tab_bar(&self, gfx: &Gfx, painter: &mut Painter, w: f32, app: &App) {
        let y = size::NAME_BAR;
        gfx.fill(Rect::new(0.0, y, w, size::TAB_BAR), ink::BACKDROP);
        gfx.fill(Rect::new(0.0, y + size::TAB_BAR - 1.0, w, 1.0), ink::LINE);

        let width = 104.0;
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let x = size::PAD + i as f32 * width;
            let here = *tab == app.tab;
            let at = Rect::new(x, y, width, size::TAB_BAR);
            if here {
                // The underline, not a filled block: a selected tab should
                // read as part of the page below it rather than a button.
                gfx.fill(
                    Rect::new(x, y + size::TAB_BAR - 2.0, width - 16.0, 2.0),
                    ink::ACCENT,
                );
            }
            let colour = if here { ink::TEXT } else { ink::FAINT };
            painter.put(
                gfx,
                &self.spec(tab.title(), size::SMALL),
                Rect::new(at.x, at.y + 8.0, at.w, at.h),
                colour,
            );
        }
    }

    fn rows(&mut self, gfx: &Gfx, painter: &mut Painter, body: Rect, app: &App) {
        let page = app.page();
        let fits = ((body.h - size::PAD) / size::ROW).floor().max(1.0) as usize;

        // Keep the cursor on screen without ever jumping further than it has
        // to — a list that recentres on every step is hard to read.
        if page.cursor < self.first {
            self.first = page.cursor;
        } else if page.cursor >= self.first + fits {
            self.first = page.cursor + 1 - fits;
        }
        let last = (self.first + fits).min(page.rows.len());

        for (n, row) in page.rows[self.first..last].iter().enumerate() {
            let i = self.first + n;
            let y = body.y + size::PAD * 0.5 + n as f32 * size::ROW;
            let at = Rect::new(size::PAD, y, body.w - size::PAD * 2.0, size::ROW);
            self.row(gfx, painter, at, row, i == page.cursor);
        }

        if last < page.rows.len() {
            painter.put_right(
                gfx,
                &self.spec("more below", size::SMALL),
                Rect::new(0.0, body.bottom() - 14.0, body.w - size::PAD, 12.0),
                ink::FAINT,
            );
        }
    }

    fn row(&self, gfx: &Gfx, painter: &mut Painter, at: Rect, row: &Row, here: bool) {
        if here {
            gfx.rounded(4.0, || gfx.fill(at, ink::PICKED));
        }

        let is_fact = matches!(row.kind, Kind::Fact { .. });
        let title_ink = if is_fact {
            ink::DIM
        } else if here {
            ink::TEXT
        } else {
            ink::DIM
        };
        painter.put(
            gfx,
            &self.spec(&row.title, size::TEXT),
            Rect::new(at.x + 10.0, at.y + 9.0, at.w * 0.55, at.h),
            title_ink,
        );

        // The right-hand column. A choice gets arrows so it is obvious it is
        // a dial rather than a switch; a fact gets none, because there is
        // nothing to turn.
        let value_ink = if row.pending() {
            ink::QUEUED
        } else if is_fact {
            ink::FAINT
        } else {
            ink::TEXT
        };
        let shown = match &row.kind {
            Kind::Choice { .. } if here => format!("‹ {} ›", row.value()),
            Kind::Choice { .. } => format!("  {}  ", row.value()),
            _ => row.value().to_string(),
        };
        painter.put_right(
            gfx,
            &self.spec(&shown, size::TEXT),
            Rect::new(at.x, at.y + 9.0, at.w - 10.0, at.h),
            value_ink,
        );

        // What it is now, when it is about to become something else. Without
        // this the queue is a list of destinations with no origins.
        if let Some(was) = row.was() {
            painter.put_right(
                gfx,
                &self.spec(&format!("was {was}"), size::SMALL),
                Rect::new(at.x, at.y + 1.0, at.w - 10.0, 12.0),
                ink::FAINT,
            );
        }
    }

    fn help(&self, gfx: &Gfx, painter: &mut Painter, at: Rect, app: &App) {
        gfx.fill(at, ink::BAR);
        let keys = match app.overlay {
            Overlay::None => "←→ change   A apply   B back   X details   L/R tabs",
            Overlay::Detail => "B close",
            Overlay::ConfirmApply => "A confirm   B cancel",
            Overlay::ConfirmDiscard => "A discard   B stay",
            Overlay::Applying { .. } => "working…",
        };
        painter.put(
            gfx,
            &self.spec(keys, size::SMALL),
            Rect::new(at.x + size::PAD, at.y + 6.0, at.w, at.h),
            ink::FAINT,
        );

        let queued = app.queue().len();
        if queued > 0 {
            let word = if queued == 1 { "change" } else { "changes" };
            painter.put_right(
                gfx,
                &self.spec(&format!("{queued} {word} not applied"), size::SMALL),
                Rect::new(at.x, at.y + 6.0, at.w - size::PAD, at.h),
                ink::QUEUED,
            );
        }
    }

    /// The shared frame behind every overlay.
    fn panel(&self, gfx: &Gfx, title: &str, painter: &mut Painter, lines: usize) -> Rect {
        let (w, h) = (PANEL.0 as f32, PANEL.1 as f32);
        gfx.fill(Rect::new(0.0, 0.0, w, h), ink::SHADE);
        let ph = (76.0 + lines as f32 * 18.0).min(h - 40.0);
        let at = Rect::new(40.0, (h - ph) * 0.5, w - 80.0, ph);
        gfx.rounded(6.0, || gfx.fill(at, ink::BAR));
        gfx.outline(at, 1.0, ink::LINE);
        painter.put(
            gfx,
            &self.spec(title, size::TEXT),
            Rect::new(at.x + 14.0, at.y + 14.0, at.w - 28.0, 20.0),
            ink::TEXT,
        );
        at
    }

    fn detail(&self, gfx: &Gfx, painter: &mut Painter, app: &App) {
        let Some(row) = app.page().selected() else { return };
        let at = self.panel(gfx, &row.title, painter, 5);
        let body = self.spec(&row.detail, size::SMALL).wrapped(at.w - 28.0, 8);
        painter.put(
            gfx,
            &body,
            Rect::new(at.x + 14.0, at.y + 42.0, at.w - 28.0, at.h - 56.0),
            ink::DIM,
        );
    }

    fn confirm_apply(&self, gfx: &Gfx, painter: &mut Painter, app: &App) {
        let queue = app.queue();
        let at = self.panel(gfx, "Apply these changes?", painter, queue.len() + 1);
        for (i, row) in queue.iter().enumerate() {
            let y = at.y + 42.0 + i as f32 * 18.0;
            if y > at.bottom() - 20.0 {
                break;
            }
            painter.put(
                gfx,
                &self.spec(&row.title, size::SMALL),
                Rect::new(at.x + 14.0, y, at.w * 0.6, 16.0),
                ink::DIM,
            );
            let change = match row.was() {
                Some(was) => format!("{was}  →  {}", row.value()),
                None => row.value().to_string(),
            };
            painter.put_right(
                gfx,
                &self.spec(&change, size::SMALL),
                Rect::new(at.x, y, at.w - 14.0, 16.0),
                ink::QUEUED,
            );
        }
    }

    fn confirm_discard(&self, gfx: &Gfx, painter: &mut Painter, app: &App) {
        let n = app.queue().len();
        let at = self.panel(gfx, "Leave without applying?", painter, 2);
        let word = if n == 1 { "change" } else { "changes" };
        painter.put(
            gfx,
            &self
                .spec(
                    &format!("{n} {word} would be thrown away. Nothing has been written yet."),
                    size::SMALL,
                )
                .wrapped(at.w - 28.0, 3),
            Rect::new(at.x + 14.0, at.y + 44.0, at.w - 28.0, 40.0),
            ink::DIM,
        );
    }

    fn applying(&self, gfx: &Gfx, painter: &mut Painter, done: usize, total: usize) {
        let at = self.panel(gfx, "Applying", painter, 2);
        painter.put(
            gfx,
            &self.spec(&format!("{done} of {total}"), size::SMALL),
            Rect::new(at.x + 14.0, at.y + 46.0, at.w - 28.0, 16.0),
            ink::DIM,
        );
        let bar = Rect::new(at.x + 14.0, at.y + 70.0, at.w - 28.0, 6.0);
        gfx.fill(bar, ink::LINE);
        let width = if total == 0 {
            0.0
        } else {
            bar.w * (done as f32 / total as f32)
        };
        gfx.fill(Rect::new(bar.x, bar.y, width, bar.h), ink::ACCENT);
    }
}
