//! `meshview <Bundles2> <model path (.smd/.fmt)> <out.png> [yaw-degrees]`
//! Software-rendered preview used to check geometry. Prints a summary too.

use std::path::PathBuf;

use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use poe2_bundles::index::Index;
use poe2_bundles::{mesh, model};

const SIZE: u32 = 900;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() >= 4, "usage: meshview <Bundles2> <model> <out.png> [yaw]");
    let dir = PathBuf::from(&a[1]);
    let yaw: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(35.0f32).to_radians();
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;
    let data = idx.read_file(&dir, &a[2])?;
    let m = mesh::parse(&a[2], &data).with_context(|| format!("parsing {}", a[2]))?;

    println!(
        "{}: {} vertices, {} triangles, {} shapes, bbox {:?}",
        a[2],
        m.positions.len(),
        m.indices.len() / 3,
        m.shapes.len(),
        m.bbox
    );
    for s in &m.shapes {
        println!("  shape {:?} material {:?} ({} indices)", s.name, s.material, s.index_count);
    }

    let textures = model::load_shape_textures(&idx, &dir, &m);
    println!("textured shapes: {}/{}", textures.iter().flatten().count(), textures.len());
    // Shape owning each triangle.
    let mut tri_shape = vec![usize::MAX; m.indices.len() / 3];
    for (si, sh) in m.shapes.iter().enumerate() {
        let a = (sh.first_index / 3) as usize;
        let b = a + (sh.index_count / 3) as usize;
        for t in a..b.min(tri_shape.len()) {
            tri_shape[t] = si;
        }
    }

    // Camera: rotate around the vertical axis, slight downward pitch.
    let (cy, sy) = (yaw.cos(), yaw.sin());
    let pitch = 0.35f32;
    let (cp, sp) = (pitch.cos(), pitch.sin());
    // The bbox is stored as [xmin, xmax, ymin, ymax, zmin, zmax]; up is -Z in model space.
    let center = [
        (m.bbox[0] + m.bbox[1]) / 2.0,
        (m.bbox[2] + m.bbox[3]) / 2.0,
        (m.bbox[4] + m.bbox[5]) / 2.0,
    ];
    // Map (x, y, z) to (right, up, depth) = (x, -z, y), then yaw about up and pitch.
    let view = |v: [f32; 3]| -> [f32; 3] {
        let (x, up, dep) = (v[0], -v[2], v[1]);
        let (x, dep) = (x * cy + dep * sy, -x * sy + dep * cy);
        let (up, dep) = (up * cp - dep * sp, up * sp + dep * cp);
        [x, up, dep]
    };
    let pts: Vec<[f32; 3]> = m
        .positions
        .iter()
        .map(|p| view([p[0] - center[0], p[1] - center[1], p[2] - center[2]]))
        .collect();
    let nrm: Vec<[f32; 3]> = m.normals.iter().map(|&n| view(n)).collect();

    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
    let scale = (SIZE as f32 * 0.86) / span;
    let ox = SIZE as f32 / 2.0 - (lo[0] + hi[0]) / 2.0 * scale;
    let oy = SIZE as f32 / 2.0 + (lo[1] + hi[1]) / 2.0 * scale; // flip Y: model Y is up
    let proj = |p: [f32; 3]| [p[0] * scale + ox, -p[1] * scale + oy, p[2]];

    let mut img = RgbaImage::from_pixel(SIZE, SIZE, Rgba([14, 10, 8, 255]));
    let mut zbuf = vec![f32::MAX; (SIZE * SIZE) as usize];
    let light = {
        let l = [0.4f32, 0.7, -0.6];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };

    for (ti, t) in m.indices.chunks_exact(3).enumerate() {
        let tex = textures.get(tri_shape[ti]).and_then(|t| t.as_ref());
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (Some(&p0), Some(&p1), Some(&p2)) = (pts.get(i0), pts.get(i1), pts.get(i2)) else { continue };
        let (s0, s1, s2) = (proj(p0), proj(p1), proj(p2));
        let area = (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s2[0] - s0[0]) * (s1[1] - s0[1]);
        if area.abs() < 1e-9 {
            continue;
        }
        let min_x = s0[0].min(s1[0]).min(s2[0]).floor().max(0.0) as u32;
        let max_x = s0[0].max(s1[0]).max(s2[0]).ceil().min(SIZE as f32 - 1.0) as u32;
        let min_y = s0[1].min(s1[1]).min(s2[1]).floor().max(0.0) as u32;
        let max_y = s0[1].max(s1[1]).max(s2[1]).ceil().min(SIZE as f32 - 1.0) as u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((s1[0] - px) * (s2[1] - py) - (s2[0] - px) * (s1[1] - py)) / area;
                let w1 = ((s2[0] - px) * (s0[1] - py) - (s0[0] - px) * (s2[1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s0[2] + w1 * s1[2] + w2 * s2[2];
                let zi = (y * SIZE + x) as usize;
                if z >= zbuf[zi] {
                    continue;
                }
                // Base colour: texture sample if available, else flat tan.
                let mut base = [0.89f32, 0.74, 0.59];
                if let Some(tx) = tex {
                    let (mut u, mut v) = (0.0f32, 0.0f32);
                    for (w, i) in [(w0, i0), (w1, i1), (w2, i2)] {
                        if let Some(uv) = m.uvs.get(i) {
                            u += w * uv[0];
                            v += w * uv[1];
                        }
                    }
                    let (tw, th) = (tx.img.width(), tx.img.height());
                    let tx_x = (u.rem_euclid(1.0) * tw as f32) as u32 % tw;
                    let tx_y = (v.rem_euclid(1.0) * th as f32) as u32 % th;
                    let px = tx.img.get_pixel(tx_x, tx_y).0;
                    if tx.alpha_test && px[3] < 128 {
                        continue;
                    }
                    base = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
                }
                zbuf[zi] = z;
                let mut n = [0.0f32; 3];
                for (w, i) in [(w0, i0), (w1, i1), (w2, i2)] {
                    if let Some(v) = nrm.get(i) {
                        for k in 0..3 {
                            n[k] += w * v[k];
                        }
                    }
                }
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                let lam = ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs();
                let v = 0.18 + 0.82 * lam;
                img.put_pixel(x, y, Rgba([(255.0 * base[0] * v) as u8, (255.0 * base[1] * v) as u8, (255.0 * base[2] * v) as u8, 255]));
            }
        }
    }
    img.save(&a[3])?;
    println!("-> {}", a[3]);
    Ok(())
}
