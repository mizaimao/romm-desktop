//! Stage 0 launch spike — see PLAN.md §5.
//!
//! Goal: prove a game boots in RetroArch from a local ROM and that we cleanly
//! detect the exit. Everything else in the client depends on this working.
//!
//!     cargo run -- doctor                 # what's installed, what can launch
//!     cargo run -- suggest                # list launchable ROMs in library/
//!     cargo run -- launch <rom>           # resolve + print the command
//!     cargo run -- launch <rom> --go      # actually spawn it

mod api;
mod config;
mod coremap;
mod cores;
mod retroarch;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use config::Config;
use coremap::CoreMap;
use retroarch::RetroArch;

const CORE_MAP: &str = "data/esde-core-map.json";

/// `[retroarch] root` from config.toml, if set.
fn configured_root() -> Option<String> {
    Config::load().ok().and_then(|c| c.retroarch.root)
}

/// Infer the RomM platform slug from a ROM path inside `library/roms/<slug>/`.
///
/// Multi-disc members live one level deeper in `MultiDisk/` (or `MuliDisk/`,
/// misspelled in the 3do source), so step back up when we land in one.
fn platform_from_path(rom: &Path) -> Option<String> {
    let mut dir = rom.parent()?;
    let name = dir.file_name()?.to_str()?;
    if name.eq_ignore_ascii_case("MultiDisk") || name.eq_ignore_ascii_case("MuliDisk") {
        dir = dir.parent()?;
    }
    Some(dir.file_name()?.to_str()?.to_owned())
}

fn cmd_doctor() -> Result<()> {
    let ra = RetroArch::locate(configured_root().as_deref())?;
    println!("RetroArch");
    println!("  root      {}", ra.root.display());
    println!("  binary    {}", ra.binary.display());
    println!(
        "  portable  {}",
        if ra.portable {
            "yes (portable.txt present)"
        } else {
            "NO — saves/states go to ~/Documents/RetroArch"
        }
    );

    let installed = ra.installed_cores();
    println!(
        "  cores     {} installed at {}",
        installed.len(),
        ra.cores_dir().display()
    );

    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let mut ready = Vec::new();
    let mut missing = Vec::new();
    for (platform, core) in &map.default_core_by_romm_platform {
        if ra.has_core(core) {
            ready.push((platform, core));
        } else {
            missing.push((platform, core));
        }
    }

    println!(
        "\n{} of {} platforms ready to launch:",
        ready.len(),
        ready.len() + missing.len()
    );
    for (platform, core) in &ready {
        println!("  {platform:<16} {core}");
    }
    if !missing.is_empty() {
        println!("\n{} platforms need a core download:", missing.len());
        for (platform, core) in &missing {
            // Is a non-default alternative already installed?
            let alt = map
                .alternatives(platform)
                .into_iter()
                .find(|c| ra.has_core(c));
            match alt {
                Some(a) => println!("  {platform:<16} {core:<18} (alternative installed: {a})"),
                None => println!("  {platform:<16} {core}"),
            }
        }
    }
    Ok(())
}

fn cmd_launch(rom: &Path, go: bool, core_override: Option<&str>, fullscreen: bool) -> Result<()> {
    let ra = RetroArch::locate(configured_root().as_deref())?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;

    let core = match core_override {
        Some(c) => c.to_owned(),
        None => {
            let platform = platform_from_path(rom)
                .with_context(|| format!("cannot infer platform from {}", rom.display()))?;
            let default = map
                .default_core(&platform)
                .with_context(|| format!("no core mapped for platform {platform:?}"))?;
            // Fall back to any installed alternative rather than dying.
            if ra.has_core(default) {
                default.to_owned()
            } else {
                match map
                    .alternatives(&platform)
                    .into_iter()
                    .find(|c| ra.has_core(c))
                {
                    Some(alt) => {
                        eprintln!("note: default core {default:?} not installed, using {alt:?}");
                        alt.to_owned()
                    }
                    None => bail!(
                        "no installed core for platform {platform:?} (default {default:?}).\n\
                         Install it, or pass --core <name>."
                    ),
                }
            }
        }
    };

    // A multi-disc .m3u whose discs were never indexed is a stub; refuse it
    // here rather than failing deep inside the emulator. See PLAN.md §3.
    if rom.extension().is_some_and(|e| e == "m3u") {
        let size = std::fs::metadata(rom).map(|m| m.len()).unwrap_or(0);
        let dir = rom.parent().unwrap_or(Path::new("."));
        let text = std::fs::read_to_string(rom).unwrap_or_default();
        let missing: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter(|l| !dir.join(l).is_file())
            .collect();
        if !missing.is_empty() {
            bail!(
                "playlist is incomplete ({size} bytes); missing disc(s):\n{}",
                missing
                    .iter()
                    .map(|m| format!("  {m}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    let cmd = ra.launch_command(&core, rom, fullscreen)?;
    let label = map
        .label_for(&core)
        .map(|l| format!(" ({l})"))
        .unwrap_or_default();
    println!("core    {core}{label}");
    println!("rom     {}", rom.display());
    println!("command {}", retroarch::render(&cmd));

    if !go {
        println!("\n(dry run — pass --go to actually launch)");
        return Ok(());
    }

    println!("\nlaunching…");
    let status = ra.launch(&core, rom, fullscreen)?;
    match status.code() {
        Some(0) => println!("RetroArch exited cleanly (0)"),
        Some(c) => println!("RetroArch exited with status {c}"),
        None => println!("RetroArch terminated by signal"),
    }
    Ok(())
}

/// List one launchable ROM per platform that has an installed core.
fn cmd_suggest() -> Result<()> {
    let ra = RetroArch::locate(configured_root().as_deref())?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let roms = Config::load()?.local_roms_dir();
    if !roms.is_dir() {
        bail!(
            "{} not found — build it with tools/build_test_library.py",
            roms.display()
        );
    }
    println!("launchable ROMs in {}:\n", roms.display());
    let mut shown = 0;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&roms)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let platform = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        let Some(core) = map.default_core(&platform) else {
            continue;
        };
        if !ra.has_core(core) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        if let Some(first) = files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e != "m3u"))
            .min()
        {
            println!("  {platform:<16} {}", first.display());
            shown += 1;
        }
    }
    if shown == 0 {
        println!("  (none — no installed core matches any staged platform)");
    }
    Ok(())
}

/// Stage 1 — prove auth works and the library is reachable.
async fn cmd_check() -> Result<()> {
    let cfg = Config::load()?;
    let client = api::Client::new(
        &cfg.server.url,
        &cfg.server.username,
        &cfg.server.password,
    )?;

    let me = client.me().await?;
    println!("server    {}", cfg.server.url);
    println!("user      {} (id {}, role {})", me.username, me.id, me.role);

    let count = client.rom_count().await?;
    println!("roms      {count}");

    let mut platforms = client.platforms().await?;
    platforms.retain(|p| p.rom_count > 0);
    platforms.sort_by(|a, b| b.rom_count.cmp(&a.rom_count));
    println!("platforms {} populated", platforms.len());
    for p in platforms.iter().take(8) {
        println!("    {:<18} {}", p.fs_slug, p.rom_count);
    }
    if platforms.len() > 8 {
        println!("    … and {} more", platforms.len() - 8);
    }
    Ok(())
}

/// Stage 0.5 — install missing cores from the buildbot.
async fn cmd_cores(install: bool) -> Result<()> {
    let ra = RetroArch::locate(configured_root().as_deref())?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let segment = cores::platform_segment()?;

    // Unique cores we need but don't have, and which platforms want them.
    let mut wanted: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for (platform, core) in &map.default_core_by_romm_platform {
        if !ra.has_core(core) {
            wanted.entry(core).or_default().push(platform);
        }
    }

    if wanted.is_empty() {
        println!("all {} mapped platforms have their default core installed",
                 map.default_core_by_romm_platform.len());
        return Ok(());
    }

    println!("{} core(s) missing (buildbot: {segment})\n", wanted.len());
    for (core, platforms) in &wanted {
        println!("  {:<20} for {}", core, platforms.join(", "));
    }

    if !install {
        println!("\n(dry run — pass --install to download into {})", ra.cores_dir().display());
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent("romm-desktop/0.1")
        .build()?;
    let dest = ra.cores_dir();
    println!("\ninstalling into {}", dest.display());

    let mut ok = 0;
    let mut failed = Vec::new();
    for core in wanted.keys() {
        print!("  {core:<20} ");
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        match cores::install(&client, core, &dest, segment).await {
            Ok(bytes) => {
                println!("ok ({:.1} MB)", bytes as f64 / 1_048_576.0);
                ok += 1;
            }
            Err(e) => {
                println!("FAILED — {e}");
                failed.push(*core);
            }
        }
    }

    println!("\n{ok} installed, {} failed", failed.len());
    if !failed.is_empty() {
        println!("failed: {}", failed.join(", "));
        println!("Some cores are not built for every arch; try the x86_64 buildbot under Rosetta.");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let go = args.iter().any(|a| a == "--go");
    let fullscreen = args.iter().any(|a| a == "--fullscreen");
    let install = args.iter().any(|a| a == "--install");
    let core = args
        .iter()
        .position(|a| a == "--core")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str);

    match args.first().map(String::as_str) {
        Some("doctor") => cmd_doctor(),
        Some("suggest") => cmd_suggest(),
        Some("check") => cmd_check().await,
        Some("cores") => cmd_cores(install).await,
        Some("launch") => {
            let rom = args
                .get(1)
                .filter(|a| !a.starts_with("--"))
                .context("usage: launch <rom-path> [--core <name>] [--fullscreen] [--go]")?;
            cmd_launch(Path::new(rom), go, core, fullscreen)
        }
        _ => {
            eprintln!("usage:");
            eprintln!("  check                            verify server auth + library reach");
            eprintln!("  doctor                           what's installed, what can launch");
            eprintln!("  cores [--install]                list/install missing libretro cores");
            eprintln!("  suggest                          list launchable ROMs in library/");
            eprintln!("  launch <rom> [--core c]          resolve and print the command");
            eprintln!("        [--fullscreen] [--go]      --go spawns; windowed unless --fullscreen");
            Ok(())
        }
    }
}
