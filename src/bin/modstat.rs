//! `modstat <Bundles2> <dat-schema dir> <Mods Id>...`: first stat and lowest roll of each affix.
use anyhow::{Context, Result};
use poe2_bundles::{index::Index, monster};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(a.get(1).context("usage: modstat <Bundles2> <schema> <mod id>...")?);
    let schema = std::path::PathBuf::from(a.get(2).context("schema")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    for id in &a[3..] {
        match monster::mod_stat(&idx, &dir, &schema, id) {
            Ok((name, stat, v)) => println!("{id}: \"{name}\" {stat} {v}"),
            Err(e) => println!("{id}: {e:#}"),
        }
    }
    Ok(())
}
