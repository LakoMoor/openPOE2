//! `skincheck <Bundles2> <skin.smd>`: report bone index range and weight sums.

use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::{index::Index, mesh};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: skincheck <Bundles2> <smd>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let m = mesh::parse(a.get(2).context("missing path")?, &idx.read_file(&dir, &a[2])?)?;
    let maxj = m.joints.iter().flatten().max().copied().unwrap_or(0);
    let (mut lo, mut hi) = (255u32, 0u32);
    for w in &m.weights {
        let s: u32 = w.iter().map(|&v| v as u32).sum();
        lo = lo.min(s);
        hi = hi.max(s);
    }
    println!("{} verts, {} with joints, max joint {maxj}, weight sum {lo}..{hi}", m.positions.len(), m.joints.len());
    for i in (0..m.joints.len()).step_by((m.joints.len() / 6).max(1)) {
        println!("  {:?} {:?} {:?}", m.positions[i], m.joints[i], m.weights[i]);
    }
    Ok(())
}
