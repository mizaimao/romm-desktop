//! Print the controller hotkeys that would be written for this machine.
//!
//!     cargo run --example padprofile_dump -- "Xbox Wireless Controller"
//!
//! The argument is optional and is the name the pad reports; without it the
//! best profile for this OS's input driver is used. Handy when a hotkey fires
//! at the wrong time: it shows which profile was chosen and the raw indices
//! derived from it, which is what RetroArch will actually act on.

use romm_desktop::padprofile;
use romm_desktop::retroarch::RetroArch;

fn main() -> anyhow::Result<()> {
    let device = std::env::args().nth(1);
    let ra = RetroArch::locate(None)?;
    eprintln!("RetroArch: {}", ra.root.display());

    match padprofile::find(&ra.root, device.as_deref()) {
        Some(p) => eprintln!("Profile:   {} ({})", p.device, p.driver),
        None => eprintln!("Profile:   none under autoconfig/ — using the built-in fallback"),
    }
    print!("{}", ra.hotkeys(device.as_deref()));
    Ok(())
}
