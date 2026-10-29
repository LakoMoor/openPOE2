//! `tdtcheck <file.tdt>...`: parse each file completely and report failures.
use poe2_bundles::tdt;

fn main() {
    let (mut ok, mut bad) = (0, 0);
    for p in std::env::args().skip(1) {
        match tdt::parse_full(&std::fs::read(&p).unwrap()) {
            Ok(t) => {
                ok += 1;
                if let Some(out) = std::env::var_os("DUMPNODES") {
                    let mut text = String::new();
                    for (i, n) in t.nodes.iter().enumerate() {
                        if let Some(g) = &n.grid {
                            text.push_str(&format!("{i} {} {}
", n.flags, (0..529).map(|c| g.get(c).to_string()).collect::<Vec<_>>().join(" ")));
                        }
                    }
                    std::fs::write(out, text).unwrap();
                }
                if std::env::var_os("SUB").is_some() {
                    println!("{p}: subtile bytes {:?}", t.subtile_bytes);
                }
                if std::env::var_os("MAX").is_some() {
                    let m = t.nodes.iter().filter_map(|n| n.grid.as_ref()).flat_map(|g| (0..529).map(move |i| g.get(i))).max();
                    println!("{p}: max node height {m:?}");
                }
                if std::env::var_os("MAP").is_some() {
                    if let Some(g) = t.grids.first() {
                        let (w, h) = (t.header.w * 23, t.header.h * 23);
                        for y in 0..h {
                            println!("{}", (0..w).map(|x| char::from_digit(g.get(y * w + x) as u32 % 36, 36).unwrap()).collect::<String>());
                        }
                    }
                    for (i, n) in t.nodes.iter().enumerate().filter(|_| std::env::var_os("NODES").is_some()) {
                        if let Some(g) = &n.grid {
                            for y in 0..23 {
                                println!("  {}", (0..23).map(|x| format!("{:02x}", g.get(y * 23 + x))).collect::<Vec<_>>().join(" "));
                            }
                        }
                        println!("  blob {:02x?}", &n.blob[..n.blob.len().min(40)]);
                    }
                    for (i, n) in t.nodes.iter().enumerate() {
                        println!("node {i}: flags {:#x} kind {} grid {:?} blob {}", n.flags, n.kind, n.grid.as_ref().map(|g| match g { tdt::Grid::Constant(v) => format!("c{v}"), tdt::Grid::Cells(c) => format!("{}", c.len()) }), n.blob.len());
                    }
                }
                if std::env::var_os("V").is_some() {
                    println!("{p}: {}x{} nodes {} grids {:?}", t.header.w, t.header.h, t.nodes.len(), t.grids.iter().map(|g| match g { tdt::Grid::Constant(v) => format!("c{v}"), tdt::Grid::Cells(c) => format!("{}", c.len()) }).collect::<Vec<_>>());
                }
            }
            Err(e) => {
                bad += 1;
                if bad <= 8 {
                    println!("{p}: {e:#}");
                }
            }
        }
    }
    println!("ok {ok} bad {bad}");
}
