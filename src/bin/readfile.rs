//! `readfile <Bundles2> <path> [out]`: write one file from the bundles to `out`
//! (or stdout summary when omitted).
use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::index::Index;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: readfile <Bundles2> <path> [out]")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let path = a.get(2).context("missing path")?;
    let bytes = idx.read_file(&dir, path)?;
    match a.get(3) {
        Some(out) => {
            std::fs::write(out, &bytes)?;
            println!("{path}: {} bytes -> {out}", bytes.len());
        }
        None => println!("{path}: {} bytes", bytes.len()),
    }
    Ok(())
}
