//! `meshcheck <Bundles2> [limit]` — parse every .smd/.fmt and report success rates by version.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use poe2_bundles::{index::Index, mesh};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(&a[1]);
    let limit: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let t = Instant::now();
    let mut seen = 0usize;
    let mut stats: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut errs: BTreeMap<String, (u32, String)> = BTreeMap::new();
    idx.for_each_file(
        &dir,
        |p| (p.ends_with(".smd") || p.ends_with(".fmt")) && { seen += 1; seen <= limit },
        |p, data| {
            let ext = if p.ends_with(".smd") { "smd" } else { "fmt" };
            let Ok(d) = data else { return };
            let key = format!("{ext} v{}", d.first().copied().unwrap_or(255));
            let e = stats.entry(key.clone()).or_default();
            match mesh::parse(p, d) {
                Ok(m) if m.positions.iter().flatten().all(|v| v.is_finite()) => e.0 += 1,
                Ok(_) => { e.1 += 1; errs.entry(key).or_insert((0, format!("non-finite vertex {p}"))).0 += 1 }
                Err(er) => { e.1 += 1; let x = errs.entry(key).or_insert((0, format!("{er:#} [{p}]"))); x.0 += 1 }
            }
        },
    );
    for (k, (ok, bad)) in &stats {
        println!("{k}: ok {ok}, failed {bad}");
    }
    for (k, (n, ex)) in &errs {
        println!("  {k}: {n} failures, e.g. {ex}");
    }
    println!("{:.1}s", t.elapsed().as_secs_f32());
    Ok(())
}
