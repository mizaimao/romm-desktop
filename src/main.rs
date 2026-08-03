//! Command-line entry point.
//!
//! The CLI and TUI here, and the Tauri GUI in `src-tauri/`, are three frontends
//! over the same `romm_desktop` library. Run with no arguments for the command
//! list.
//!
//! Commands that touch the server (`check`, `sync`, `get`, `hash-parity`) need
//! `config.toml`; the rest work offline against the local cache.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use romm_desktop::{
    api, cache, cores, download, media, parity, saves, theme, theme_remote, tui,
};
use romm_desktop::config::Config;
use romm_desktop::coremap::{self, CoreMap};
use romm_desktop::retroarch::{self, RetroArch};
use romm_desktop::retroarch_install;
use romm_desktop::launch;
use romm_desktop::probe;
use romm_desktop::shaders;
use romm_desktop::util::human;

const CORE_MAP: &str = "data/esde-core-map.json";

/// `[retroarch] root` from config.toml, if set.
/// Locate RetroArch using the configured boot order.
fn locate_retroarch(cfg: &Config) -> Result<RetroArch> {
    RetroArch::locate_in(&cfg.retroarch.ordered_paths())
}

/// Infer the RomM platform slug from a ROM path under `.../roms/<slug>/...`.
///
/// Walks up to the directory whose parent is `roms`, so it works at any depth:
/// a plain ROM, a disc inside a `MultiDisk/` folder, or a file inside a
/// per-game folder ROM (`roms/dc/Shenmue (USA)/Shenmue (USA).m3u`).
fn platform_from_path(rom: &Path) -> Option<String> {
    let mut dir = rom.parent()?;
    loop {
        let parent = dir.parent()?;
        if parent.file_name().is_some_and(|n| n == "roms") {
            return Some(dir.file_name()?.to_str()?.to_owned());
        }
        dir = parent;
    }
}

fn cmd_doctor() -> Result<()> {
    let cfg = Config::load()?;
    let ra = locate_retroarch(&cfg)?;
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
    // Resolve the same way a launch does, so this reports what would actually
    // run rather than the ES-DE map's default — the two differ wherever
    // [cores.overrides] applies, which was silently misreported before.
    for platform in map.default_core_by_romm_platform.keys() {
        let core = coremap::resolve_core(&map, &cfg.cores.overrides, platform, |c| ra.has_core(c))
            .or_else(|| map.default_core(platform).map(str::to_owned));
        let Some(core) = core else { continue };
        if ra.has_core(&core) {
            ready.push((platform.clone(), core));
        } else {
            missing.push((platform.clone(), core));
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
    let cfg = Config::load()?;
    let ra = RetroArch::locate(cfg.retroarch.root.as_deref())?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;

    let platform = platform_from_path(rom)
        .with_context(|| format!("cannot infer platform from {}", rom.display()))?;
    let user_cfg = cfg.user_retroarch_config();
    let req = launch::Request {
        rom,
        platform: &platform,
        fs_name: rom.file_name().and_then(|n| n.to_str()).unwrap_or_default(),
        library_root: Path::new(&cfg.library.local_root),
        user_cfg: &user_cfg,
        shaders_enabled: cfg.shaders.enabled,
        shader_overrides: &cfg.shaders.by_platform,
        core_overrides: &cfg.cores.overrides,
        core_per_game: &cfg.cores.per_game,
        core_override,
    };
    let plan = launch::plan(&ra, &map, &req)?;

    for note in &plan.notes {
        println!("{note}");
    }
    if let Some(label) = &plan.shader_label {
        println!("shader  {label}");
    }
    let core_label = plan
        .core_label
        .as_ref()
        .map(|l| format!(" ({l})"))
        .unwrap_or_default();
    println!("core    {}{core_label}", plan.core);
    println!("rom     {}", rom.display());
    println!("command {}", retroarch::render(&plan.command(&ra, rom, fullscreen)?));

    if !go {
        println!("\n(dry run — pass --go to actually launch)");
        return Ok(());
    }

    println!("\nlaunching…");
    let status = plan.run(&ra, rom, fullscreen)?;
    match status.code() {
        Some(0) => println!("RetroArch exited cleanly (0)"),
        Some(c) => println!("RetroArch exited with status {c}"),
        None => println!("RetroArch terminated by signal"),
    }
    Ok(())
}

/// List one launchable ROM per platform that has an installed core.
fn cmd_suggest() -> Result<()> {
    let cfg = Config::load()?;
    let ra = locate_retroarch(&cfg)?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let roms = cfg.local_roms_dir();
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
    match client.heartbeat().await {
        Ok(hb) => {
            let v = &hb.system.version;
            let note = if v == romm_desktop::VERIFIED_AGAINST {
                "verified".to_owned()
            } else {
                format!("UNVERIFIED — client was checked against {}", romm_desktop::VERIFIED_AGAINST)
            };
            println!("version   RomM {v} ({note})");
        }
        Err(e) => println!("version   unknown ({e})"),
    }
    match client.config().await {
        Ok(sc) => println!(
            "hashing   {} excluded names, {} excluded exts{}",
            sc.default_excluded_files.len(),
            sc.default_excluded_extensions.len(),
            if sc.skip_hash_calculation { ", SKIP_HASH_CALCULATION set" } else { "" }
        ),
        Err(e) => println!("hashing   could not read /api/config ({e})"),
    }
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

/// Show the shader assigned to each platform, and the alternatives.
fn cmd_shaders(platform: Option<&str>) -> Result<()> {
    let cfg = Config::load()?;
    let ra = locate_retroarch(&cfg)?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;

    if let Some(slug) = platform {
        let display = shaders::display_of(slug);
        let current = shaders::preset_for(&cfg.shaders.by_platform, slug);
        println!("{slug} — {} display\n", match display {
            shaders::Display::Crt => "CRT / television",
            shaders::Display::Handheld => "handheld LCD",
        });
        for opt in shaders::available(&ra, display) {
            let mark = if current.as_deref() == Some(opt.path) { "*" } else { " " };
            println!("  {mark} {:<28} {:<34} {}", opt.label, opt.path, opt.note);
        }
        println!("  {} {:<28} no shader", if current.is_none() { "*" } else { " " }, "None");
        println!("\nSet in config.toml:\n  [shaders.by_platform]\n  {slug} = \"crt/crt-geom\"");
        return Ok(());
    }

    println!("{:<17} {:<11} shader", "platform", "display");
    for p in store.platforms()? {
        let d = match shaders::display_of(&p.fs_slug) {
            shaders::Display::Crt => "CRT",
            shaders::Display::Handheld => "handheld",
        };
        let cur = shaders::preset_for(&cfg.shaders.by_platform, &p.fs_slug);
        let shown = match &cur {
            Some(x) if shaders::resolve(&ra, x).is_some() => shaders::label_of(x).to_owned(),
            Some(x) => format!("{x} (MISSING)"),
            None => "none".to_owned(),
        };
        println!("  {:<15} {:<11} {}", p.fs_slug, d, shown);
    }
    println!("\n`shaders <platform>` lists the alternatives.");
    Ok(())
}

/// Download and install RetroArch itself.
async fn cmd_install_retroarch(version: Option<&str>) -> Result<()> {
    let cfg = Config::load()?;
    if let Ok(existing) = locate_retroarch(&cfg) {
        println!("already installed: {}", existing.root.display());
        println!("(installing again would add a second entry; remove it first if that is what you want)");
        return Ok(());
    }

    let http = reqwest::Client::builder().user_agent("romm-desktop/0.1").build()?;
    let version = match version {
        Some(v) => v.to_owned(),
        None => retroarch_install::latest_available(&http).await?,
    };
    // Alongside the rest of our data, so one folder still holds everything.
    let dest = PathBuf::from(&cfg.library.local_root).join("RetroArch");
    println!("installing RetroArch {version} into {}", dest.display());

    let started = std::time::Instant::now();
    let mut last = std::time::Instant::now();
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let root = retroarch_install::install(&http, &version, &dest, |done, total| {
        if !interactive || last.elapsed().as_millis() < 200 {
            return;
        }
        last = std::time::Instant::now();
        let pct = if total > 0 { done as f64 / total as f64 * 100.0 } else { 0.0 };
        print!("\r  {pct:5.1}%  {} / {}   ", human(done), human(total));
        use std::io::Write as _;
        std::io::stdout().flush().ok();
    })
    .await?;

    println!("\ninstalled in {:.0}s -> {}", started.elapsed().as_secs_f64(), root.display());
    println!("\nAdd it to your boot order in config.toml:");
    println!("  [[retroarch.installs]]");
    println!("  label = \"Downloaded\"");
    println!("  path = \"{}\"", root.display());
    println!("  enabled = true");
    println!("\nThen `cores --install` to fetch the libretro cores.");
    Ok(())
}

/// Stage 0.5 — install missing cores from the buildbot.
async fn cmd_cores(install: bool) -> Result<()> {
    let cfg = Config::load()?;
    let ra = locate_retroarch(&cfg)?;
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

    // Refresh the settings that govern how we hash and verify, so a server
    // config change cannot silently corrupt later verification.
    match client.config().await {
        Ok(cfg) => {
            if cfg.skip_hash_calculation {
                println!(
                    "note: server has SKIP_HASH_CALCULATION set — it stores no hashes, \n\
                     so downloads can only be size-checked."
                );
            }
            store.save_server_config(&cfg).ok();
        }
        Err(e) => eprintln!("warning: could not read /api/config ({e}); using last known values"),
    }
    if let Ok(hb) = client.heartbeat().await {
        let v = hb.system.version;
        if !v.is_empty() {
            if store.server_version().as_deref() != Some(v.as_str())
                && v != romm_desktop::VERIFIED_AGAINST
            {
                eprintln!(
                    "warning: server is RomM {v}, but this client's server-specific behaviour\n\
                     (archive hashing, query params) was verified against {}. Re-check\n\
                     `hash-parity` and a download or two.",
                    romm_desktop::VERIFIED_AGAINST
                );
            }
            store.set_server_version(&v).ok();
        }
    }

    let before = store.rom_count().unwrap_or(0);
    let started = std::time::Instant::now();
    let (platforms, upserted, incremental) = store.sync(&client, full).await?;
    // Removals never show up in an incremental pull, so reconcile against the
    // server's full id list.
    match client.rom_identifiers().await {
        Ok(ids) => match store.prune_missing(&ids) {
            Ok(n) if n > 0 => println!("pruned {n} rom(s) the server no longer has"),
            Ok(_) => {}
            Err(e) => eprintln!("warning: prune failed ({e})"),
        },
        Err(e) => eprintln!("warning: could not list server rom ids ({e}); skipped pruning"),
    }

    // Collections come from the server wholesale — they are RomM's grouping,
    // not ours, so there is nothing to merge.
    match client.all_collections().await {
        Ok(items) => match store.replace_collections(&items) {
            Ok(n) => println!("collections: {n} synced"),
            Err(e) => eprintln!("warning: storing collections failed ({e})"),
        },
        Err(e) => eprintln!("warning: could not fetch collections ({e}); kept the previous set"),
    }

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

/// Download every ROM of one platform, several at a time.
///
/// Sequential transfers waste most of the wall clock on per-request latency
/// when the files are small — an arcade set averages 4.5 MB.
async fn cmd_get_platform(slug: &str, jobs: usize) -> Result<()> {
    use futures_util::StreamExt as _;

    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    romm_desktop::apply_cached_server_config(&store);
    let rows = store.roms_for(slug)?;
    if rows.is_empty() {
        bail!("no cached roms for platform {slug:?} — try `sync` first");
    }

    let roms_dir = cfg.local_roms_dir();
    let client = std::sync::Arc::new(api::Client::new(
        &cfg.server.url,
        &cfg.server.username,
        &cfg.server.password,
    )?);

    let total: i64 = rows.iter().map(|r| r.fs_size_bytes.max(0)).sum();
    println!("{} games, {} — {jobs} at a time", rows.len(), human(total as u64));
    let started = std::time::Instant::now();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let failed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let count = rows.len();

    futures_util::stream::iter(rows.into_iter().map(|rom| {
        let client = client.clone();
        let roms_dir = roms_dir.clone();
        let done = done.clone();
        let failed = failed.clone();
        async move {
            let members = if rom.multi_file {
                client.member_hashes(rom.id).await
            } else {
                Vec::new()
            };
            let target = download::Target {
                rom_id: rom.id,
                members: &members,
                fs_name: &rom.fs_name,
                platform_slug: &rom.platform_slug,
                expected_size: (rom.fs_size_bytes > 0).then_some(rom.fs_size_bytes as u64),
                md5: rom.md5_hash.as_deref(),
                sha1: rom.sha1_hash.as_deref(),
                multi_file: rom.multi_file,
            };
            let r = download::fetch(
                client.http(),
                client.base(),
                client.auth(),
                &target,
                &roms_dir,
                |_, _| {},
            )
            .await;
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Err(e) = r {
                failed.lock().unwrap().push(format!("{}: {e}", rom.fs_name));
            }
            if n.is_multiple_of(25) || n == count {
                let secs = started.elapsed().as_secs_f64();
                println!("  {n}/{count} in {secs:.0}s");
            }
        }
    }))
    .buffer_unordered(jobs.max(1))
    .collect::<Vec<_>>()
    .await;

    let failed = failed.lock().unwrap();
    println!(
        "\n{}/{count} downloaded in {:.0}s",
        count - failed.len(),
        started.elapsed().as_secs_f64()
    );
    for f in failed.iter().take(15) {
        println!("  failed: {f}");
    }
    if failed.len() > 15 {
        println!("  … and {} more", failed.len() - 15);
    }
    Ok(())
}

/// Find out which cores actually run something, by running them.
///
/// Downloads what it needs first: a verdict about a game we do not have is
/// worthless, and arcade ROMs are small.
async fn cmd_probe(
    term: Option<&str>,
    platform: Option<&str>,
    sample: usize,
    cores: Option<&str>,
    frames: u32,
) -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let ra = RetroArch::locate_in(&cfg.retroarch.ordered_paths())?;
    let scratch = probe::scratch_dir(&cfg.library.local_root);

    let roms = match (term, platform) {
        (Some(t), _) => {
            let found = if let Ok(id) = t.parse::<i64>() {
                store.rom_by_id(id)?.into_iter().collect()
            } else {
                store.search(t, 5)?
            };
            if found.is_empty() {
                bail!("nothing matches {t:?}");
            }
            found
        }
        (None, Some(p)) => {
            let mut all = store.roms_for(p)?;
            if all.is_empty() {
                bail!("no cached roms for platform {p:?}");
            }
            // Spread the sample across the alphabet rather than taking the
            // first N, which on this library would be all numeric titles.
            let step = (all.len() / sample.max(1)).max(1);
            all = all.into_iter().step_by(step).take(sample).collect();
            all
        }
        (None, None) => bail!("give a game, or --platform <slug>"),
    };

    let slug = roms[0].platform_slug.clone();
    let candidates: Vec<String> = match cores {
        Some(list) => list.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect(),
        None => {
            let mut v: Vec<String> = map.alternatives(&slug).into_iter().map(str::to_owned).collect();
            if let Some(d) = map.default_core(&slug)
                && !v.iter().any(|c| c == d)
            {
                v.insert(0, d.to_owned());
            }
            v
        }
    };
    if candidates.is_empty() {
        bail!("no candidate cores for platform {slug:?}; pass --cores");
    }

    println!(
        "probing {} game(s) against {}: {}\n",
        roms.len(),
        candidates.len(),
        candidates.join(", ")
    );

    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password).ok();
    let roms_dir = cfg.local_roms_dir();
    let mut tally: std::collections::BTreeMap<String, (usize, usize)> = Default::default();

    for rom in &roms {
        let local = roms_dir.join(&rom.platform_slug).join(&rom.fs_name);
        if !local.exists() {
            let Some(client) = client.as_ref() else {
                println!("{:<44} skipped (not downloaded, no server)", short(&rom.name));
                continue;
            };
            let members = if rom.multi_file { client.member_hashes(rom.id).await } else { Vec::new() };
            let target = download::Target {
                rom_id: rom.id,
                members: &members,
                fs_name: &rom.fs_name,
                platform_slug: &rom.platform_slug,
                expected_size: (rom.fs_size_bytes > 0).then_some(rom.fs_size_bytes as u64),
                md5: rom.md5_hash.as_deref(),
                sha1: rom.sha1_hash.as_deref(),
                multi_file: rom.multi_file,
            };
            if let Err(e) =
                download::fetch(client.http(), client.base(), client.auth(), &target, &roms_dir, |_, _| {})
                    .await
            {
                println!("{:<44} skipped ({e})", short(&rom.name));
                continue;
            }
        }
        // Neo Geo and similar need their BIOS in RetroArch's system dir, or
        // every core refuses the content for a reason unrelated to the core.
        if let Some(d) = local.parent() {
            let _ = ra.install_bios(d);
        }

        let results = probe::probe_cores(&ra, &local, &candidates, frames, &scratch)?;
        let marks: Vec<String> = results
            .iter()
            .map(|r| format!("{:>7}", if r.verdict.ok() { "ok" } else { "-" }))
            .collect();
        println!("{:<44}{}", short(&rom.name), marks.join(""));
        for r in &results {
            let e = tally.entry(r.core.clone()).or_default();
            e.1 += 1;
            if r.verdict.ok() {
                e.0 += 1;
            }
        }
    }

    println!("\n{:<22}{:>8}  share", "core", "ran");
    let mut ranked: Vec<_> = tally.into_iter().collect();
    ranked.sort_by_key(|(_, (ok, _))| std::cmp::Reverse(*ok));
    for (core, (ok, total)) in &ranked {
        println!(
            "{core:<22}{:>8}  {:.0}%",
            format!("{ok}/{total}"),
            if *total > 0 { *ok as f64 / *total as f64 * 100.0 } else { 0.0 }
        );
    }
    println!("\ncolumns are the cores in the order listed above");
    Ok(())
}

fn short(s: &str) -> String {
    s.chars().take(42).collect()
}

/// Show the collection groups, or what is inside one.
fn cmd_collections(group: Option<&str>) -> Result<()> {
    let store = cache::Cache::open(Path::new(CACHE_DB))?;

    let Some(group) = group else {
        let groups = store.collection_groups()?;
        if groups.is_empty() {
            bail!("no collections cached — run `sync` first");
        }
        println!("{:<14} {:>6}", "group", "count");
        for (name, n) in &groups {
            println!("{name:<14} {n:>6}");
        }
        println!("\n{} collections total", store.collection_count()?);
        return Ok(());
    };

    let items = store.collections_in(group)?;
    if items.is_empty() {
        bail!("no collections in group {group:?} — try `collections` for the list");
    }
    println!("{:<46} {:>6}", format!("{group} collections"), "games");
    for c in items.iter().take(60) {
        println!(
            "{:<46} {:>6}",
            c.name.chars().take(44).collect::<String>(),
            c.rom_count
        );
    }
    if items.len() > 60 {
        println!("… and {} more", items.len() - 60);
    }
    Ok(())
}

/// Stage 4 — download a ROM by search term.
async fn cmd_get(needle: &str) -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    romm_desktop::apply_cached_server_config(&store);
    // A bare number is an id: with several platforms carrying identically
    // named folders, a search term alone cannot always name one ROM.
    if let Ok(id) = needle.parse::<i64>()
        && let Some(rom) = store.rom_by_id(id)?
    {
        return download_one(rom, &cfg).await;
    }

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

    download_one(rom, &cfg).await
}

/// Fetch one resolved ROM, reporting progress and how it was verified.
async fn download_one(rom: cache::RomRow, cfg: &Config) -> Result<()> {
    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)?;
    let roms_dir = cfg.local_roms_dir();

    println!(
        "{} [{}]  {}",
        rom.name,
        rom.platform_slug,
        human(rom.fs_size_bytes as u64)
    );

    // Folder ROMs verify per member; the rom-level hash is not reproducible.
    let members = if rom.multi_file {
        client.member_hashes(rom.id).await
    } else {
        Vec::new()
    };

    let target = download::Target {
        rom_id: rom.id,
        members: &members,
        fs_name: &rom.fs_name,
        platform_slug: &rom.platform_slug,
        expected_size: (rom.fs_size_bytes > 0).then_some(rom.fs_size_bytes as u64),
        md5: rom.md5_hash.as_deref(),
        sha1: rom.sha1_hash.as_deref(),
        multi_file: rom.multi_file,
    };

    let started = std::time::Instant::now();
    let mut last_tick = std::time::Instant::now();
    // Bytes already on disk when we started, so the rate reflects what this
    // session actually transferred rather than crediting us with the resume.
    let mut baseline: Option<u64> = None;
    // Carriage-return progress only makes sense on a terminal; piped or
    // redirected it produces thousands of lines of noise.
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let outcome = download::fetch(
        client.http(),
        client.base(),
        client.auth(),
        &target,
        &roms_dir,
        |done, total| {
            let base = *baseline.get_or_insert(done);
            if !interactive {
                return;
            }
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
            let how = verified.describe();
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

/// Resolve artwork for a ROM — local first, else fetch from the server.
async fn cmd_art(needle: &str) -> Result<()> {
    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    let client = api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password).ok();
    let media_root = PathBuf::from(&cfg.library.local_root).join("downloaded_media");

    let matches = store.search(needle, 5)?;
    if matches.is_empty() {
        bail!("nothing matches {needle:?}");
    }
    for rom in matches {
        let stem = Path::new(&rom.fs_name)
            .file_stem().map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| rom.fs_name.clone());
        println!("{} [{}]", rom.name, rom.platform_slug);
        for (kind, _) in media::ESDE_TYPES {
            let before =
                media::find_local(&media_root, &rom.platform_slug, &stem, kind).is_some();
            // RomM's own copies for the two types it knows; the ES-DE tree for
            // everything else.
            let got = match *kind {
                media::COVERS if rom.cover_path.is_some() => {
                    media::ensure(
                        client.as_ref(), &media_root, &rom.platform_slug, &stem,
                        kind, rom.cover_path.as_deref(),
                    ).await
                }
                _ => {
                    media::ensure_esde(
                        client.as_ref(), &media_root, &rom.platform_slug, &stem, kind,
                    ).await
                }
            };
            let how = match (&got, before) {
                (Some(_), true) => "local",
                (Some(_), false) => "FETCHED",
                (None, _) => "none",
            };
            println!("  {kind:<15} {how:<8} {}",
                     got.map(|p| p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
                        .unwrap_or_default());
        }
    }
    Ok(())
}

/// Inspect ES-DE themes and install their system logos locally.
fn cmd_themes(install: bool) -> Result<()> {
    let cfg = Config::load()?;
    let map = CoreMap::load(Path::new(CORE_MAP))?;
    let store = cache::Cache::open(Path::new(CACHE_DB))?;
    let slugs: Vec<String> = store.platforms()?.into_iter().map(|p| p.fs_slug).collect();

    let themes = theme::discover_with(cfg.theme.root.as_deref(), Some(&cfg.themes_dir()));
    if themes.is_empty() {
        bail!("no ES-DE themes found — install ES-DE, or set [theme] root in config.toml");
    }
    println!("themes found:");
    for t in &themes {
        println!("  {:<16} {}", t.name, t.path.display());
    }

    let found = theme::logos(&themes, &map, &slugs);
    println!("\nlogos: {}/{} platforms", found.len(), slugs.len());
    for slug in &slugs {
        match found.get(slug) {
            Some(p) => println!("  {slug:<16} {}", p.display()),
            None => println!("  {slug:<16} — none"),
        }
    }

    if install {
        let media_root = cfg.media_dir();
        let n = theme::install(&themes, &map, &slugs, &media_root)?;
        println!("\ninstalled {n} logos into {}/_platforms", media_root.display());
    } else {
        println!("\n(pass --install to copy them into the local media tree)");
    }
    Ok(())
}

/// List themes available from the official ES-DE themes list.
async fn cmd_themes_available(filter: Option<&str>) -> Result<()> {
    let http = reqwest::Client::builder().user_agent("romm-desktop/0.1").build()?;
    let mut list = theme_remote::list(&http).await?;
    if let Some(f) = filter {
        list.retain(|t| t.matches(f));
    }
    let cfg = Config::load()?;
    let dir = cfg.themes_dir();

    println!("{} themes available\n", list.len());
    for t in &list {
        let installed = dir.join(t.dir_name()).is_dir();
        println!(
            "  {}{:<26} {:<28} {}",
            if installed { "* " } else { "  " },
            t.name.chars().take(24).collect::<String>(),
            t.dir_name(),
            t.author
        );
    }
    println!("\n* = already downloaded    (cargo run -- themes --get <name>)");
    Ok(())
}

/// Download (or update) a theme from the official list.
async fn cmd_themes_get(needle: &str, logos_only: bool) -> Result<()> {
    let cfg = Config::load()?;
    let http = reqwest::Client::builder().user_agent("romm-desktop/0.1").build()?;
    let list = theme_remote::list(&http).await?;

    let hits: Vec<_> = list.iter().filter(|t| t.matches(needle)).collect();
    let theme_entry = match hits.len() {
        0 => bail!("no theme matches {needle:?} — try `themes --available`"),
        1 => hits[0],
        _ => {
            // An exact name match disambiguates; otherwise make the user choose.
            match hits.iter().find(|t| t.name.eq_ignore_ascii_case(needle)) {
                Some(t) => t,
                None => {
                    println!("{} matches — be more specific:", hits.len());
                    for t in hits {
                        println!("  {:<26} {}", t.name, t.dir_name());
                    }
                    return Ok(());
                }
            }
        }
    };

    let dir = cfg.themes_dir();
    println!("{} — {}", theme_entry.name, theme_entry.url);
    println!("cloning into {} …", dir.join(theme_entry.dir_name()).display());
    let (path, fresh) = theme_remote::install(theme_entry, &dir)?;
    let size = theme_remote::size_of(&path);
    println!(
        "{} ({:.1} MB)",
        if fresh { "downloaded" } else { "updated" },
        size as f64 / 1_048_576.0
    );

    if logos_only {
        // Themes ship hundreds of MB of wallpapers and per-system art we never
        // render. Keep the logos, drop the rest.
        let map = CoreMap::load(Path::new(CORE_MAP))?;
        let store = cache::Cache::open(Path::new(CACHE_DB))?;
        let slugs: Vec<String> =
            store.platforms()?.into_iter().map(|p| p.fs_slug).collect();
        let one = vec![romm_desktop::theme::Theme {
            name: theme_entry.dir_name(),
            path: path.clone(),
        }];
        let n = theme::install(&one, &map, &slugs, &cfg.media_dir())?;
        theme_remote::remove(&theme_entry.dir_name(), &dir)?;
        println!(
            "kept {n} logos, deleted the {:.0} MB checkout",
            size as f64 / 1_048_576.0
        );
        return Ok(());
    }

    println!("\nRun `themes --install` to copy its logos into the platform grid.");
    println!("(or re-run with --logos-only to keep just the icons and delete the rest)");
    Ok(())
}

/// Update every downloaded theme.
async fn cmd_themes_update() -> Result<()> {
    let cfg = Config::load()?;
    let dir = cfg.themes_dir();
    let http = reqwest::Client::builder().user_agent("romm-desktop/0.1").build()?;
    let list = theme_remote::list(&http).await?;

    let Ok(entries) = std::fs::read_dir(&dir) else {
        bail!("nothing downloaded yet — see `themes --available`");
    };
    let mut n = 0;
    for entry in entries.flatten().filter(|e| e.path().is_dir()) {
        let name = entry.file_name().to_string_lossy().to_string();
        match list.iter().find(|t| t.dir_name() == name) {
            Some(t) => {
                print!("  {name:<28} ");
                use std::io::Write as _;
                std::io::stdout().flush().ok();
                match theme_remote::install(t, &dir) {
                    Ok(_) => { println!("ok"); n += 1; }
                    Err(e) => println!("FAILED — {e}"),
                }
            }
            None => println!("  {name:<28} not in the official list, skipped"),
        }
    }
    println!("\n{n} theme(s) updated");
    Ok(())
}

/// Debug: show how a file hashes, as a whole and as an archive member.
fn cmd_hashcheck(path: &Path) -> Result<()> {
    let (md5, sha1) = download::hash_file(path)?;
    println!("file        md5 {md5}");
    println!("            sha1 {sha1}");
    if let Some((m, s)) = download::hash_archive_composite(path) {
        println!("composite   md5 {m}");
        println!("            sha1 {s}");
    }
    for (name, md5, sha1) in download::hash_archive_members(path) {
        println!("member      {name}");
        println!("            md5 {md5}");
        println!("            sha1 {sha1}");
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

/// Frontends over one library: this CLI/TUI, and the Tauri GUI in `src-tauri/`.
#[derive(Parser)]
#[command(
    name = "romm-desktop",
    about = "Browse, download and launch a self-hosted RomM library",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify server auth and that the library is reachable
    Check,
    /// Pull metadata into the local cache
    Sync {
        /// Ignore the watermark and re-fetch everything
        #[arg(long)]
        full: bool,
    },
    /// Terminal library browser
    Browse,
    /// Test which cores actually run a game, or a sample of a platform.
    ///
    /// OPENS A RETROARCH WINDOW PER PROBE. On macOS `video_driver = "null"`
    /// silences rendering but does NOT stop the window appearing, so N games x
    /// M cores means N*M windows. Requires --i-know-it-opens-windows.
    Probe {
        /// ROM id, or a search term
        term: Option<String>,
        /// Probe a sample of this platform instead of one game
        #[arg(long)]
        platform: Option<String>,
        /// How many games to sample from the platform
        #[arg(long, default_value_t = 12)]
        sample: usize,
        /// Cores to try, comma separated. Defaults to every core the map
        /// knows for the platform.
        #[arg(long)]
        cores: Option<String>,
        /// Frames to run before exiting. 180 is about three seconds.
        #[arg(long, default_value_t = 180)]
        frames: u32,
        /// Required. Each probe opens a RetroArch window; there is no headless
        /// mode on macOS.
        #[arg(long)]
        i_know_it_opens_windows: bool,
    },
    /// List collections mirrored from the server
    Collections {
        /// Show the collections inside one group, e.g. `genre`
        group: Option<String>,
    },
    /// Download a ROM (resumable, hash-verified)
    Get {
        /// Name or filename to search for
        term: Option<String>,
        /// Download an entire platform instead of one game
        #[arg(long)]
        platform: Option<String>,
        /// Concurrent transfers when fetching a whole platform
        #[arg(long, default_value_t = 6)]
        jobs: usize,
    },
    /// Check our content_hash implementation against the server's
    HashParity,
    /// Inspect local saves and states
    Scan,
    /// ES-DE theme logos for the console grid
    Themes {
        /// Copy logos from installed themes into the media tree
        #[arg(long)]
        install: bool,
        /// List downloadable themes, optionally filtered
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        available: Option<String>,
        /// Download or update one theme by name
        #[arg(long, value_name = "NAME")]
        get: Option<String>,
        /// With --get: keep only the platform icons and delete the checkout
        #[arg(long)]
        logos_only: bool,
        /// Update every downloaded theme
        #[arg(long)]
        update: bool,
    },
    /// Show what is installed and which platforms can launch
    Doctor,
    /// Show or list per-platform video shaders
    Shaders {
        /// Limit to one platform and list its alternatives
        platform: Option<String>,
    },
    /// Download and install RetroArch itself
    InstallRetroarch {
        /// Pin a release instead of taking the newest known one
        #[arg(long)]
        version: Option<String>,
    },
    /// List or install missing libretro cores
    Cores {
        /// Download the missing cores from the libretro buildbot
        #[arg(long)]
        install: bool,
    },
    /// List one launchable ROM per platform
    Suggest,
    /// Resolve a ROM's core and print the launch command
    Launch {
        rom: PathBuf,
        /// Override the core rather than resolving one
        #[arg(long, value_name = "NAME")]
        core: Option<String>,
        /// Launch fullscreen instead of windowed
        #[arg(long)]
        fullscreen: bool,
        /// Actually spawn the emulator; without this it is a dry run
        #[arg(long)]
        go: bool,
    },
    /// Resolve a ROM's artwork, fetching from the server if needed
    Art {
        term: String,
    },
    /// Show how a file hashes, whole and as archive members
    Hashcheck {
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Check => cmd_check().await,
        Command::Sync { full } => cmd_sync(full).await,
        Command::Browse => cmd_browse(),
        Command::Collections { group } => cmd_collections(group.as_deref()),
        Command::Probe { term, platform, sample, cores, frames, i_know_it_opens_windows } => {
            if !i_know_it_opens_windows {
                bail!(
                    "probe opens one RetroArch window per game per core, and macOS has no\n\
                     headless mode for it. Re-run with --i-know-it-opens-windows if that is\n\
                     what you want."
                );
            }
            cmd_probe(term.as_deref(), platform.as_deref(), sample, cores.as_deref(), frames).await
        }
        Command::Get { term, platform, jobs } => match (term, platform) {
            (_, Some(slug)) => cmd_get_platform(&slug, jobs).await,
            (Some(t), None) => cmd_get(&t).await,
            (None, None) => bail!("give a search term, or --platform <slug>"),
        },
        Command::HashParity => {
            let cfg = Config::load()?;
            let client =
                api::Client::new(&cfg.server.url, &cfg.server.username, &cfg.server.password)?;
            parity::run(&client).await
        }
        Command::Scan => cmd_scan(),
        Command::Themes {
            install,
            available,
            get,
            logos_only,
            update,
        } => match (available, get, update) {
            // An empty string means --available was given with no filter.
            (Some(filter), _, _) => {
                cmd_themes_available(Some(filter.as_str()).filter(|f| !f.is_empty())).await
            }
            (_, Some(name), _) => cmd_themes_get(&name, logos_only).await,
            (_, _, true) => cmd_themes_update().await,
            _ => cmd_themes(install),
        },
        Command::Doctor => cmd_doctor(),
        Command::Shaders { platform } => cmd_shaders(platform.as_deref()),
        Command::InstallRetroarch { version } => cmd_install_retroarch(version.as_deref()).await,
        Command::Cores { install } => cmd_cores(install).await,
        Command::Suggest => cmd_suggest(),
        Command::Launch {
            rom,
            core,
            fullscreen,
            go,
        } => cmd_launch(&rom, go, core.as_deref(), fullscreen),
        Command::Art { term } => cmd_art(&term).await,
        Command::Hashcheck { file } => cmd_hashcheck(&file),
    }
}
