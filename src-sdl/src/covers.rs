// Box art: found on disk, decoded once, and let go again.
//
// The letting go is the point. A cover is a few tens of kilobytes as a PNG and
// about 786 KB once decoded, and the webview held every one it had ever drawn
// — 578 MB of a 671 MB process, measured on 2026-08-20. That is merely
// wasteful on a Mac. On a 1 GB handheld it is the whole machine.
//
// So this is bounded, and the bound is a count of textures rather than a
// count of bytes: every card on screen is the same size, so a texture is the
// same size as its neighbours and counting them is counting memory.

use crate::gfx::{Gfx, Texture};
use romm_desktop::media;
use sdl2::image::LoadSurface;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many decoded covers to hold.
///
/// A screenful is a few dozen; this is enough for a screen either side of
/// where the cursor is, so scrolling back never re-decodes. At roughly 786 KB
/// apiece it is about 150 MB at the limit, which is why the handheld will want
/// this lower — and why it is one number rather than scattered.
const LIMIT: usize = 192;

/// What a cover can be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Drawn, and in `held`.
    Ready,
    /// Looked for and not there. Remembered so it is not looked for again on
    /// every frame — a filesystem miss is cheap and 2,506 of them a frame is
    /// not.
    None,
}

pub struct Covers {
    media_root: PathBuf,
    /// Which artwork to prefer, from `[media] list_art`.
    look: String,
    held: HashMap<i64, Texture>,
    known: HashMap<i64, State>,
    /// The order things were asked for, oldest first. A plain queue rather
    /// than a proper LRU: what is asked for is what is on screen, so the
    /// oldest entry is the furthest from the cursor by construction.
    order: Vec<i64>,
}

impl Covers {
    pub fn new(media_root: PathBuf, look: String) -> Self {
        Covers {
            media_root,
            look,
            held: HashMap::new(),
            known: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn holding(&self) -> usize {
        self.held.len()
    }

    /// The cover for a game, decoding it if it is on disk and not yet held.
    ///
    /// `stem` is the ROM's filename without its extension, which is what the
    /// artwork is named after — see `media::local_art`.
    pub fn get(&mut self, gfx: &Gfx, id: i64, platform: &str, stem: &str) -> Option<&Texture> {
        match self.known.get(&id) {
            Some(State::None) => return None,
            Some(State::Ready) => {
                // Asked for again: it stays.
                if let Some(at) = self.order.iter().position(|&held| held == id) {
                    let moved = self.order.remove(at);
                    self.order.push(moved);
                }
                return self.held.get(&id);
            }
            None => {}
        }

        let Some(path) = media::local_art(&self.media_root, platform, stem, &self.look) else {
            self.known.insert(id, State::None);
            return None;
        };
        match self.load(gfx, &path) {
            Some(texture) => {
                self.make_room();
                self.held.insert(id, texture);
                self.known.insert(id, State::Ready);
                self.order.push(id);
                self.held.get(&id)
            }
            None => {
                // On disk but not decodable — a truncated download, or
                // something that is not an image. Remembered as absent so it
                // is not retried every frame.
                self.known.insert(id, State::None);
                None
            }
        }
    }

    /// Read the file and hand its pixels to the GPU.
    ///
    /// SDL_image decodes — it is already a package on the handheld and knows
    /// every format a scraper produces — but the texture is ours, on our
    /// context. `into_rgba` because a decoder returns whatever the file was,
    /// and the uploader wants one layout.
    fn load(&self, gfx: &Gfx, path: &Path) -> Option<Texture> {
        let surface = sdl2::surface::Surface::from_file(path).ok()?;
        let rgba = surface.convert_format(sdl2::pixels::PixelFormatEnum::ABGR8888).ok()?;
        let (w, h) = (rgba.width(), rgba.height());
        let pitch = rgba.pitch() as usize;
        let bytes = rgba.without_lock()?;
        // Rows can be padded. Copied tightly, because the uploader is told
        // rows are not.
        let mut tight = Vec::with_capacity(w as usize * h as usize * 4);
        for row in 0..h as usize {
            let from = row * pitch;
            tight.extend_from_slice(&bytes[from..from + w as usize * 4]);
        }
        Some(gfx.upload_rgba(w, h, &tight))
    }

    /// Drop the oldest until there is room for one more.
    fn make_room(&mut self) {
        while self.held.len() >= LIMIT {
            let Some(oldest) = self.order.first().copied() else { break };
            self.order.remove(0);
            self.held.remove(&oldest);
            // Forgotten rather than marked absent: it was there a moment ago
            // and scrolling back has to find it again.
            self.known.remove(&oldest);
        }
    }
}

/// The bound is what makes this safe on a 1 GB machine, so it is checked
/// rather than trusted — at compile time, since it is a constant and a test
/// that can never fail at runtime is not a test.
///
/// Too small and scrolling back re-decodes; too large and the handheld is
/// gone. At roughly 786 KB a cover, 192 is about 150 MB.
const _: () = assert!(LIMIT >= 96 && LIMIT <= 256);
