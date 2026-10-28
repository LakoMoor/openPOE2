//! `shapes <Bundles2> <model>`: list shapes with their materials.
use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::{index::Index, model};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: shapes <Bundles2> <model>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let path = a.get(2).context("missing model")?;
    let mesh = model::load_mesh(&idx, &dir, path)?;
    let tex = model::load_shape_textures(&idx, &dir, path, &mesh);
    for (s, t) in mesh.shapes.iter().zip(&tex) {
        println!("{:28} idx {:6}+{:6} {} {}", s.name, s.first_index, s.index_count, if t.is_some() { "tex " } else { "NONE" }, s.material);
    }
    Ok(())
}
