//! The screen's geometry, asked of macOS directly.
//!
//! The window manager, the frontend and the emulator have to agree on one
//! coordinate space, and going through the toolkit did not get us there. Tauri
//! reports a monitor as a pixel size plus a scale factor, which is the right
//! answer only when the display is running at its native resolution. On a
//! MacBook set to a scaled mode — which is the default on these machines — the
//! panel is 3024x1964, the backing scale factor is still 2, and the desktop is
//! actually 1800x1169 points. Dividing one by the other gives 1512x982: a
//! number that is wrong by a third, in a way that is invisible until a window
//! lands somewhere strange.
//!
//! `CGDisplayBounds` reports points, in the same top-left-origin space the
//! window server uses, with no scale factor involved. That is what Cocoa lays
//! windows out in, and therefore what RetroArch's window position means.

/// A display's bounds in points, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Bounds;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    // SAFETY contract for both: they take a display id and return by value,
    // touch no memory we own, and are safe to call from any thread.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
    }

    /// The display holding the menu bar.
    ///
    /// Deliberately the main display rather than "whichever screen the library
    /// window is on". Picking the second one means agreeing with the window
    /// server about which screen that is *and* about where it starts, and
    /// getting either wrong puts the game window off the side of the desktop —
    /// which is what kept happening. The main display is the one every part of
    /// the system already agrees about.
    pub fn main_display() -> Option<Bounds> {
        // SAFETY: see the extern block. No arguments are pointers.
        let r = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        if r.size.width <= 0.0 || r.size.height <= 0.0 {
            return None;
        }
        Some(Bounds {
            x: r.origin.x,
            y: r.origin.y,
            width: r.size.width,
            height: r.size.height,
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Bounds;
    pub fn main_display() -> Option<Bounds> {
        None
    }
}

pub use imp::main_display;

#[cfg(test)]
mod tests {
    use super::*;

    /// Points, not pixels. A Retina panel reports thousands of pixels across
    /// and a desktop of about two thousand points; a four-figure width in the
    /// five thousands would mean we are reading the framebuffer instead.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_main_display_is_measured_in_points() {
        let b = main_display().expect("macOS always has a main display");
        assert!(b.width > 640.0 && b.width < 4000.0, "{b:?} looks like pixels");
        assert!(b.height > 480.0 && b.height < 3000.0, "{b:?} looks like pixels");
        // The main display is the origin of the coordinate space by definition.
        assert_eq!((b.x, b.y), (0.0, 0.0));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn there_is_nothing_to_ask_off_macos() {
        assert!(main_display().is_none());
    }
}
