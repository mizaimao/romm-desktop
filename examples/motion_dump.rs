//! Print the generated preset that chains a motion pass onto a base shader.
//!
//!     cargo run --example motion_dump -- crt/crt-guest-advanced subframe-bfi/adaptive_strobe-koko

use romm_desktop::{retroarch::RetroArch, shaders};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "crt/crt-guest-advanced".into());
    let motion = args.next().unwrap_or_else(|| "subframe-bfi/adaptive_strobe-koko".into());
    let ra = RetroArch::locate(None)?;
    let dir = std::env::temp_dir();

    match shaders::write_chained(&ra, &dir, Some(&base), &motion) {
        Some(p) => {
            eprintln!("wrote {}", p.display());
            print!("{}", std::fs::read_to_string(p)?);
        }
        None => eprintln!("could not chain {motion} onto {base} — is it installed?"),
    }
    Ok(())
}
