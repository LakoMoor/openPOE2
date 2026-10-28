use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use poe2_bundles::index::Index;

fn main() -> Result<()> {
    let dir = PathBuf::from(
        std::env::args().nth(1).context("usage: poe2_bundles <Bundles2 dir> [path-substring | exact-path out-file]")?,
    );
    let filter = std::env::args().nth(2);
    let out_path = std::env::args().nth(3);

    let t = Instant::now();
    let raw = std::fs::read(dir.join("_.index.bin"))?;
    let idx = Index::parse(&raw)?;
    println!(
        "index: {} bundles, {} files, {} resolved paths ({:.1}s)",
        idx.bundles.len(),
        idx.files.len(),
        idx.paths.len(),
        t.elapsed().as_secs_f32()
    );

    if filter.as_deref() == Some("--extract-prefix") {
        let prefix = out_path.context("--extract-prefix <prefix> <out dir> [ext]")?;
        let out_dir = PathBuf::from(std::env::args().nth(4).context("missing out dir")?);
        let ext = std::env::args().nth(5).unwrap_or_default();
        std::fs::create_dir_all(&out_dir)?;
        let (mut ok, mut fail) = (0, 0);
        for (p, rec) in &idx.paths {
            if !p.starts_with(&prefix) || !p.ends_with(&ext) || rec.size == 0 {
                continue;
            }
            match idx.read_file(&dir, p) {
                Ok(b) => {
                    let rel = p[prefix.len()..].replace('/', "__");
                    std::fs::write(out_dir.join(rel), b)?;
                    ok += 1;
                }
                Err(e) => {
                    eprintln!("{p}: {e:#}");
                    fail += 1;
                }
            }
        }
        println!("extracted {ok}, failed {fail}");
        return Ok(());
    }

    if let (Some(path), Some(out)) = (filter.as_deref(), out_path) {
        let bytes = idx.read_file(&dir, path)?;
        std::fs::write(&out, &bytes)?;
        println!("extracted {path}: {} bytes -> {out}", bytes.len());
        return Ok(());
    }

    let mut shown = 0;
    let mut paths: Vec<_> = idx.paths.keys().collect();
    paths.sort();
    for p in paths {
        if filter.as_deref().map_or(true, |f| p.contains(f)) {
            println!("{p}");
            shown += 1;
            if shown >= 40 {
                break;
            }
        }
    }
    Ok(())
}
