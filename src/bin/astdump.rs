//! `astdump <Bundles2> <file.ast> [animation-name-substring]`

use std::path::PathBuf;

use anyhow::{Context, Result};
use poe2_bundles::{index::Index, skeleton::Skeleton};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(a.get(1).context("usage: astdump <Bundles2> <file.ast> [filter]")?);
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let sk = Skeleton::parse(&idx.read_file(&dir, a.get(2).context("missing path")?)?)?;
    println!("version {}, {} bones, {} animations, blob {} bytes", sk.version, sk.bones.len(), sk.animations.len(), sk.blob.len());
    if a.get(3).map(String::as_str) == Some("-bones") {
        for (i, b) in sk.bones.iter().enumerate().take(40) {
            println!("{i:3} {:28} parent {:?} pos {:.1?}", b.name, b.parent, sk.world_position(i));
        }
        return Ok(());
    }
    if let Some(f) = a.get(3).and_then(|f| f.strip_prefix("-rest=")) {
        let an = sk.animations.iter().find(|x| x.name == f).context("no such animation")?;
        let base = Skeleton::parse(&idx.read_file(&dir, "art/models/charactersfour/onerig.ast")?)?;
        for t in sk.tracks(an)?.iter().take(12) {
            let b = &base.bones[t.bone];
            println!("{:3} {:24} rest t {:.2?} | key0 pos {:.2?} rot {:.3?} sc {:.2?} (n={},{},{})", t.bone, b.name,
                b.local.w_axis.truncate(), t.positions.first(), t.rotations.first(), t.scales.first(),
                t.scales.len(), t.rotations.len(), t.positions.len());
        }
        return Ok(());
    }
    let filter = a.get(3).map(|s| s.to_ascii_lowercase());
    let mut shown = 0;
    for an in &sk.animations {
        if filter.as_ref().is_some_and(|f| !an.name.to_ascii_lowercase().contains(f)) {
            continue;
        }
        let tracks = sk.tracks(an);
        match &tracks {
            Ok(t) => {
                let keys: usize = t.iter().map(|t| t.rotations.len()).sum();
                let last = t.iter().flat_map(|t| t.rotations.iter().map(|k| k[0])).fold(0.0f32, f32::max);
                println!("{:40} {}fps tracks {:3} rot keys {:5} last t {:.3}", an.name, an.framerate, an.num_tracks, keys, last);
                if shown == 0 {
                    if let Some(tr) = t.first() {
                        println!("  bone {} rot[0..3] {:?}", tr.bone, &tr.rotations[..tr.rotations.len().min(3)]);
                        println!("  pos[0..3] {:?}", &tr.positions[..tr.positions.len().min(3)]);
                        println!("  scale[0..2] {:?}", &tr.scales[..tr.scales.len().min(2)]);
                    }
                }
            }
            Err(e) => println!("{:40} tracks failed: {e:#}", an.name),
        }
        shown += 1;
        if shown >= 15 {
            break;
        }
    }
    Ok(())
}
