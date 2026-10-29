//! `monsterstats <Bundles2> <dat-schema dir> <.ao path or id>`
use anyhow::{Context, Result};
use poe2_bundles::{index::Index, monster};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(a.get(1).context("usage: monsterstats <Bundles2> <schema> <ao>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    println!("{:#?}", monster::find(&idx, &dir, std::path::Path::new(a.get(2).context("schema")?), a.get(3).context("ao")?)?);
    Ok(())
}
