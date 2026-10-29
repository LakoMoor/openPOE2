//! `tileheight <Bundles2> <tile.tgt>`: print the bounding box of an assembled tile.
use anyhow::{Context, Result};
use poe2_bundles::{index::Index, model};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(a.get(1).context("usage: tileheight <Bundles2> <tgt>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let m = model::load_mesh(&idx, &dir, a.get(2).context("missing tgt")?)?;
    println!("bbox x {:?} y {:?} z {:?}", &m.bbox[0..2], &m.bbox[2..4], &m.bbox[4..6]);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &m.positions {
        lo = lo.min(p[2]);
        hi = hi.max(p[2]);
    }
    if let Some(out) = a.get(3) {
        let text: String = m.positions.iter().map(|p| format!("{} {} {}
", p[0], p[1], p[2])).collect();
        std::fs::write(out, text)?;
    }
    println!("z {lo}..{hi}, {} vertices", m.positions.len());
    Ok(())
}
