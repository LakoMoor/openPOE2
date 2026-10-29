//! `tdtcheck <file.tdt>...`: parse each file completely and report failures.
use poe2_bundles::tdt;

fn main() {
    let (mut ok, mut bad) = (0, 0);
    for p in std::env::args().skip(1) {
        match tdt::parse_full(&std::fs::read(&p).unwrap()) {
            Ok(t) => {
                ok += 1;
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
