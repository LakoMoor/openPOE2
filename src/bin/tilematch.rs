//! `tilematch <Bundles2> <room.arm>`: for every `k` slot of a room, list the
//! tiles of the area's tile set whose size and edge types fit.
use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::{index::Index, room, tdt};

fn text(b: &[u8]) -> String {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    String::from_utf16_lossy(&b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>())
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: tilematch <Bundles2> <room.arm>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let path = a.get(2).context("missing room")?.to_ascii_lowercase();
    let room = room::parse_arm(&idx.read_file(&dir, &path)?)?;

    let mut parts: Vec<&str> = path.split('/').collect();
    let mut tiles = Vec::new();
    while parts.pop().is_some() && !parts.is_empty() {
        let area = parts.join("/");
        let Ok(tsi) = idx.read_file(&dir, &format!("{area}/master.tsi")) else { continue };
        let tsi = text(&tsi);
        let tst = tsi.lines().find(|l| l.trim_start().starts_with("TileSet")).and_then(|l| l.split('"').nth(1).map(str::to_string)).context("no TileSet")?;
        println!("area {area}, tile set {tst}");
        for l in text(&idx.read_file(&dir, &format!("{area}/{}", tst.to_ascii_lowercase()))?).lines() {
            if let Some(p) = l.split('"').nth(1).filter(|p| p.to_ascii_lowercase().ends_with(".tdt")) {
                if let Ok(t) = idx.read_file(&dir, &p.to_ascii_lowercase()).map_err(anyhow::Error::from).and_then(|b| tdt::parse(&b)) {
                    tiles.push((p.to_string(), t));
                }
            }
        }
        break;
    }
    println!("{} tiles in the set, {} slots", tiles.len(), room.slots.len());
    for s in &room.slots {
        let fits: Vec<_> = tiles
            .iter()
            .filter(|(_, t)| (t.w, t.h) == (s.w, s.h) || (t.w, t.h) == (s.h, s.w))
            .filter(|(_, t)| (0..4).any(|r| (0..4).all(|i| t.edges[i] == s.edges[(i + r) % 4])))
            .collect();
        println!("slot ({},{}) {}x{} edges {:?}: {} fit", s.col, s.row, s.w, s.h, s.edges, fits.len());
        for (p, t) in fits.iter().take(3) {
            println!("    {p} grounds {:?}", t.grounds);
        }
    }
    Ok(())
}
