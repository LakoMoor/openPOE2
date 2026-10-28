//! `tgmcheck <Bundles2> <file.tgm>`: summarise both meshes of a tile.
use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::{index::Index, terrain::parse_tgm};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: tgmcheck <Bundles2> <tgm>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let t = parse_tgm(&idx.read_file(&dir, a.get(2).context("missing path")?)?)?;
    for (name, m) in [("main", &t.main), ("ground", &t.ground)] {
        let max = m.indices.iter().max().copied().unwrap_or(0);
        let span = m.shapes.iter().map(|s| s.first_index + s.index_count).max().unwrap_or(0);
        println!("{name}: {} verts, {} indices (max index {max}), {} shapes, last shape end {span}, bbox {:.0?}", m.positions.len(), m.indices.len(), m.shapes.len(), m.bbox);
        for s in m.shapes.iter().take(4) {
            println!("   shape {}+{}", s.first_index, s.index_count);
        }
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &m.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        println!("   positions {lo:.0?} .. {hi:.0?}");
    }
    Ok(())
}
