//! `edgeorder <Bundles2> <list of .arm paths file>`: for adjacent slots, count how often
//! edge index i of one slot equals edge index j of its neighbour (horizontal and vertical).
use anyhow::{Context, Result};
use poe2_bundles::{index::Index, room};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(a.get(1).context("usage: edgeorder <Bundles2> <list>")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let list = std::fs::read_to_string(a.get(2).context("list")?)?;
    let mut horiz = [[0u32; 4]; 4];
    let mut vert = [[0u32; 4]; 4];
    let mut pairs = (0, 0);
    for p in list.lines().filter(|l| !l.is_empty()) {
        let Ok(r) = idx.read_file(&dir, p).map_err(anyhow::Error::from).and_then(|b| room::parse_arm(&b)) else { continue };
        for s in r.slots.iter().filter(|s| s.edges.iter().any(Option::is_some)) {
            for t in r.slots.iter().filter(|t| t.edges.iter().any(Option::is_some)) {
                // t directly to the right of s / directly below s (slot sizes in grid units).
                let right = t.row == s.row && t.col == s.col + s.w;
                let below = t.col == s.col && t.row == s.row + s.h;
                for i in 0..4 {
                    for j in 0..4 {
                        let eq = s.edges[i].is_some() && s.edges[i] == t.edges[j];
                        if right && eq {
                            horiz[i][j] += 1;
                        }
                        if below && eq {
                            vert[i][j] += 1;
                        }
                    }
                }
                pairs.0 += right as u32;
                pairs.1 += below as u32;
            }
        }
    }
    println!("right-neighbour pairs {}, below-neighbour pairs {}", pairs.0, pairs.1);
    println!("horizontal (row = edge of left slot, col = edge of right slot):");
    for r in horiz { println!("  {r:?}"); }
    println!("vertical (row = edge of upper slot, col = edge of lower slot):");
    for r in vert { println!("  {r:?}"); }
    Ok(())
}
