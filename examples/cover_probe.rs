//! Reproduce what the grid asks for when it scrolls, outside the GUI.
//!
//! The frontend swallows a failed cover batch on purpose — a missing thumbnail
//! is not worth interrupting browsing over — which also means a batch that
//! fails every time is invisible. This runs the same call the GUI makes and
//! says what happened.
//!
//!   cargo run --example cover_probe -- megadrive 40

use std::path::Path;
use std::time::Instant;

use romm_desktop::{cache, config::Config, media};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let platform = args.next().unwrap_or_else(|| "megadrive".to_owned());
    let count: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(40);

    let cfg = Config::load()?;
    let store = cache::Cache::open(Path::new("cache.sqlite3"))?;
    let client = cfg.server.client().ok();
    let media_root = cfg.media_dir();

    let rows: Vec<_> = store.roms_for(&platform)?.into_iter().take(count).collect();
    println!("{} rom(s) from {platform}\n", rows.len());

    let started = Instant::now();
    let (mut got, mut missing, mut no_path) = (0, 0, 0);

    for row in &rows {
        let stem = Path::new(&row.fs_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| row.fs_name.clone());

        if row.cover_small_path.is_none() && row.cover_path.is_none() {
            no_path += 1;
            continue;
        }

        let each = Instant::now();
        let out = media::ensure_thumb(
            client.as_ref(),
            &media_root,
            &row.platform_slug,
            &stem,
            row.cover_small_path.as_deref(),
            row.cover_path.as_deref(),
        )
        .await;

        match out {
            Some(_) => got += 1,
            None => {
                missing += 1;
                println!("  MISS  {:<44} {:?}", stem.chars().take(42).collect::<String>(), row.cover_small_path);
            }
        }
        if each.elapsed().as_millis() > 400 {
            println!("  SLOW  {:<44} {} ms", stem.chars().take(42).collect::<String>(), each.elapsed().as_millis());
        }
    }

    println!(
        "\n{got} resolved, {missing} failed, {no_path} have no cover on the server \
         — {:.1}s total, {:.0} ms each",
        started.elapsed().as_secs_f64(),
        started.elapsed().as_millis() as f64 / rows.len().max(1) as f64
    );
    Ok(())
}
