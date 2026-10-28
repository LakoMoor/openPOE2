//! `dds2png <Bundles2 dir> <in-bundle .dds path> <out.png>`

use std::io::Cursor;
use std::path::PathBuf;

use anyhow::{Context, Result};
use image_dds::ddsfile::Dds;
use poe2_bundles::index::Index;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (dir, path, out) = (
        PathBuf::from(a.get(1).context("usage: dds2png <Bundles2> <path.dds> <out.png>")?),
        a.get(2).context("missing path")?,
        a.get(3).context("missing output")?,
    );
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let bytes = idx.read_file(&dir, path)?;
    let dds = Dds::read(&mut Cursor::new(bytes))?;
    let img = image_dds::image_from_dds(&dds, 0)?;
    img.save(out)?;
    println!("{path}: {}x{} -> {out}", img.width(), img.height());
    Ok(())
}
