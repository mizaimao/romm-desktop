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
mod cache;
mod config;
mod coremap;
mod cores;
mod download;
mod parity;
mod retroarch;
mod savehash;
mod saves;
mod tui;

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

const CACHE_DB: &str = "cache.sqlite3";

/// Stage 2 — pull metadata into the local cache.
async fn cmd_sync(full: bool) -> Result<()> {
    let cfg = Config::load()?;
    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)?;
    let mut store = cache::Cache::open(Path::new(CACHE_DB))?;

    let before = store.rom_count().unwrap_or(0);
    let started = std::time::Instant::now();
    let (platforms, upserted, incremental) = store.sync(&client, full).await?;
    let after = store.rom_count().unwrap_or(0);

    println!(
        "{} sync: {platforms} platforms, {upserted} rom rows in {:.1}s",
        if incremental { "incremental" } else { "full" },
        started.elapsed().as_secs_f64()
    );
    println!("cache now holds {after} roms ({:+})", after - before);
    if let Some(w) = store.watermark() {
        println!("watermark {w}");
    }
    Ok(())
}

fn human(bytes: u64) -> String {
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

/// Stage 4 — download a ROM by search term.
async fn cmd_get(needle: &str) -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    let matches = store.search(needle, 25)?;

    let rom = match matches.len() {
        0 => bail!("nothing in the cache matches {needle:?} — try `sync` first"),
        1 => matches.into_iter().next().unwrap(),
        n => {
            println!("{n} matches — be more specific:");
            for m in matches.iter().take(15) {
                println!(
                    "  {:<16} {:<48} {:>10}",
                    m.platform_slug,
                    m.name.chars().take(46).collect::<String>(),
                    human(m.fs_size_bytes as u64)
                );
            }
            return Ok(());
        }
    };

    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)?;
    let roms_dir = cfg.local_roms_dir();

    println!(
        "{} [{}]  {}",
        rom.name,
        rom.platform_slug,
        human(rom.fs_size_bytes as u64)
    );

    let target = download::Target {
        rom_id: rom.id,
        fs_name: &rom.fs_name,
        platform_slug: &rom.platform_slug,
        expected_size: (rom.fs_size_bytes > 0).then_some(rom.fs_size_bytes as u64),
        md5: rom.md5_hash.as_deref(),
        sha1: rom.sha1_hash.as_deref(),
    };

    let started = std::time::Instant::now();
    let mut last_tick = std::time::Instant::now();
    // Bytes already on disk when we started, so the rate reflects what this
    // session actually transferred rather than crediting us with the resume.
    let mut baseline: Option<u64> = None;
    let outcome = download::fetch(
        client.http(),
        client.base(),
        client.auth(),
        &target,
        &roms_dir,
        |done, total| {
            let base = *baseline.get_or_insert(done);
            // Throttle so the terminal isn't the bottleneck on a fast transfer.
            if last_tick.elapsed().as_millis() < 200 {
                return;
            }
            last_tick = std::time::Instant::now();
            let pct = if total > 0 {
                format!("{:5.1}%", done as f64 / total as f64 * 100.0)
            } else {
                "  ?  ".to_owned()
            };
            let rate = done.saturating_sub(base) as f64
                / started.elapsed().as_secs_f64().max(0.001)
                / 1_048_576.0;
            print!(
                "\r  {pct}  {:>10} / {:<10} {rate:6.1} MB/s   ",
                human(done),
                human(total)
            );
            use std::io::Write as _;
            std::io::stdout().flush().ok();
        },
    )
    .await;

    println!();
    match outcome? {
        download::Outcome::AlreadyHave(p) => println!("already downloaded and verified: {}", p.display()),
        download::Outcome::Downloaded { path, bytes, resumed_from, verified } => {
            let how = match verified {
                download::Verified::Md5 => "md5 verified",
                download::Verified::Sha1 => "sha1 verified",
                download::Verified::SizeOnly => "size only — server published no hash",
            };
            if resumed_from > 0 {
                println!("resumed from {}", human(resumed_from));
            }
            println!(
                "{} in {:.1}s ({})\n{}",
                human(bytes),
                started.elapsed().as_secs_f64(),
                how,
                path.display()
            );
        }
    }
    Ok(())
}

/// Stage 5a — report what the local save scanner finds.
fn cmd_scan() -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    if store.rom_count().unwrap_or(0) == 0 {
        bail!("cache is empty — run `sync` first so saves can be resolved to rom ids");
    }
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let root = Path::new(&cfg.saves.root);
    if !root.is_dir() {
        bail!("{} not found — set [saves] root in config.toml", root.display());
    }

    let found = saves::scan(root, &store, &map)?;
    println!("scanning {}\n", root.display());

    let (mut ok, mut amb, mut unmatched, mut unknown, mut superseded) = (0, 0, 0, 0, 0);
    for c in &found {
        let kind = match c.kind { saves::Kind::Save => "save ", saves::Kind::State => "state" };
        let core = c.core.clone().unwrap_or_else(|| format!("?{}", c.core_dir));
        print!("{kind} {:<10} {:<9} {:<44}", core, c.slot, c.rom_base.chars().take(42).collect::<String>());
        match &c.resolution {
            saves::Resolution::Resolved { rom_id, platform, .. } => {
                if c.canonical {
                    ok += 1;
                    println!(" -> {platform}/{rom_id}");
                } else {
                    superseded += 1;
                    println!(
                        " -> {platform}/{rom_id}  SUPERSEDED by {}",
                        c.superseded_by.as_deref().unwrap_or("?")
                    );
                }
            }
            saves::Resolution::Ambiguous(hits) => {
                amb += 1;
                println!(" -> AMBIGUOUS: {}", hits.iter()
                    .map(|(id, p, _)| format!("{p}/{id}"))
                    .collect::<Vec<_>>().join(", "));
            }
            saves::Resolution::Unmatched => { unmatched += 1; println!(" -> no matching rom"); }
            saves::Resolution::UnknownCore => { unknown += 1; println!(" -> unknown core dir"); }
        }
    }

    println!("\n{} files: {ok} to sync, {superseded} superseded, {amb} ambiguous, \
              {unmatched} unmatched, {unknown} unknown core", found.len());
    if superseded > 0 {
        println!("\nSuperseded entries share a (rom_id, slot) with a save from the platform's\n\
                  default core. Only the default core's file is synced, or they would\n\
                  overwrite each other on the server every run.");
    }
    if amb > 0 {
        println!("\nAmbiguous entries are NOT guessed — the same filename exists on more than one\n\
                  platform (this library has arcade+mame and nes+famicom overlapping).");
    }
    Ok(())
}

/// Stage 2 — browse the cache.
fn cmd_browse() -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    if store.rom_count().unwrap_or(0) == 0 {
        bail!("cache is empty — run `cargo run -- sync` first");
    }
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    // Missing RetroArch is not fatal; browsing still works, launching doesn't.
    let ra = RetroArch::locate(cfg.retroarch.root.as_deref()).ok();
    // Likewise a missing server only disables downloading.
    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)
        .ok()
        .map(std::sync::Arc::new);
    tui::run(
        &store,
        &cfg.local_roms_dir(),
        ra,
        map,
        client,
        tokio::runtime::Handle::current(),
    )
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
        Some("sync") => cmd_sync(args.iter().any(|a| a == "--full")).await,
        Some("browse") => cmd_browse(),
        Some("scan") => cmd_scan(),
        Some("hash-parity") => {
            let cfg = Config::load()?;
            let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)?;
            parity::run(&client).await
        }
        Some("get") => {
            let needle = args.get(1).filter(|a| !a.starts_with("--"))
                .context("usage: get <search term>")?;
            cmd_get(needle).await
        }
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
            eprintln!("  sync [--full]                    pull metadata into the local cache");
            eprintln!("  browse                           TUI library browser");
            eprintln!("  get <term>                       download a ROM (resumable, verified)");
            eprintln!("  hash-parity                      verify content_hash matches the server");
            eprintln!("  scan                             inspect local saves/states");
            eprintln!("  doctor                           what's installed, what can launch");
            eprintln!("  cores [--install]                list/install missing libretro cores");
            eprintln!("  suggest                          list launchable ROMs in library/");
            eprintln!("  launch <rom> [--core c]          resolve and print the command");
            eprintln!("        [--fullscreen] [--go]      --go spawns; windowed unless --fullscreen");
            Ok(())
        }
    }
}
