//! `xpcheck <Bundles2> <dat-schema dir>`: monster experience per level and the player level thresholds.
use anyhow::{Context, Result};
use poe2_bundles::{index::Index, monster};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(a.get(1).context("usage: xpcheck <Bundles2> <schema>")?);
    let schema = std::path::PathBuf::from(a.get(2).context("missing schema dir")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let xp: Vec<_> = (1..=12).map(|l| monster::level_xp(&idx, &dir, &schema, l).unwrap_or(-1)).collect();
    println!("monster xp, levels 1..12: {xp:?}");
    let lv = monster::experience_levels(&idx, &dir, &schema)?;
    println!("{} thresholds: {:?}", lv.len(), &lv[..lv.len().min(12)]);
    Ok(())
}
