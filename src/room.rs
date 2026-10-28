//! Rooms (`.arm`) and doodad descriptors (`.ao`).
//!
//! Only the parts needed to show a room are read: its size and the placed
//! doodads. Tile selection for `K` slots (via tile sets) is not handled here.

use anyhow::{Context, Result};

use crate::mesh::{Mesh, Shape};

/// World units per grid cell: a 23-cell sub-tile is 250 units wide.
pub const CELL: f32 = 250.0 / 23.0;
/// Grid cells along one side per unit of `side_length`.
pub const CELLS_PER_SIDE: u32 = 23;

#[derive(Debug, Clone)]
pub struct Doodad {
    pub x: f32,
    pub y: f32,
    /// Rotation about the vertical axis, radians.
    pub rotation: f32,
    pub scale: f32,
    pub ao: String,
}

/// A tile slot of the room grid: where it sits, how big it is, and the ground
/// type (a `.gt` path) and height at each corner, ordered SW, SE, NE, NW.
#[derive(Debug, Clone)]
pub struct KSlot {
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    pub grounds: [Option<String>; 4],
    pub heights: [i32; 4],
}

#[derive(Debug, Clone)]
pub struct Room {
    pub slots: Vec<KSlot>,
    pub version: u32,
    /// Side length of the room in sub-tiles, if it could be determined.
    pub side: Option<u32>,
    pub doodads: Vec<Doodad>,
}

fn decode(b: &[u8]) -> Result<String> {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(String::from_utf16(&units)?)
}

fn first_quoted(s: &str) -> Option<&str> {
    let a = s.find('"')? + 1;
    Some(&s[a..a + s[a..].find('"')?])
}

pub fn parse_arm(bytes: &[u8]) -> Result<Room> {
    let text = decode(bytes)?;
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let version: u32 = lines
        .first()
        .and_then(|l| l.strip_prefix("version "))
        .and_then(|v| v.trim().parse().ok())
        .context("missing version line")?;

    // Layout: version, string count, strings, then the dimension line.
    let side = lines.get(1).and_then(|n| n.parse::<usize>().ok()).and_then(|n| {
        lines.get(2 + n)?.split_whitespace().next()?.parse::<u32>().ok()
    });

    // Doodad lines: `x y radians ... scale "<path>.ao" "<stub>"`.
    let mut doodads = Vec::new();
    for line in &lines {
        let Some(q) = line.find('"') else { continue };
        let Some(ao) = first_quoted(line).filter(|p| p.to_ascii_lowercase().ends_with(".ao")) else { continue };
        let nums: Vec<f32> = line[..q].split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if nums.len() < 4 || line[..q].split_whitespace().count() != nums.len() {
            continue;
        }
        let scale = nums.last().copied().filter(|s| *s > 0.0 && *s < 100.0).unwrap_or(1.0);
        doodads.push(Doodad { x: nums[0], y: nums[1], rotation: nums[2], scale, ao: ao.to_string() });
    }
    let slots = parse_slots(&text, version).unwrap_or_default();
    Ok(Room { slots, version, side, doodads })
}

fn unquote(l: &str) -> String {
    l.trim().trim_matches('"').to_string()
}

/// Walk the line-based sections up to the slot grid and read its `k` slots.
fn parse_slots(text: &str, version: u32) -> Option<Vec<KSlot>> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let n: usize = lines.get(1)?.parse().ok()?;
    let strings: Vec<String> = lines.get(2..2 + n)?.iter().map(|l| unquote(l)).collect();
    let mut i = 2 + n;
    i += 1; // dimensions
    let numbers: u32 = lines.get(i)?.split_whitespace().filter_map(|t| t.parse::<u32>().ok()).sum();
    i += 3; // numbers, tag, bools
    let root: Vec<&str> = lines.get(i)?.split_whitespace().collect();
    if root.first() != Some(&"k") {
        return None;
    }
    let grid_h: usize = root.get(2)?.parse().ok()?;
    i += 1;
    i += numbers as usize * 2; // thingies
    let groups = match version {
        ..20 => 9,
        20..26 => 10,
        26..29 => 5,
        _ => 6,
    };
    for _ in 0..groups {
        if version < 32 {
            let c: usize = lines.get(i)?.parse().ok()?;
            i += 1 + c;
        } else {
            while *lines.get(i)? != "-1" {
                i += 1;
            }
            i += 1;
        }
    }
    if version >= 35 {
        i += 1;
    }

    let mut slots = Vec::new();
    for row in 0..grid_h {
        let toks: Vec<&str> = lines.get(i + row)?.split_whitespace().collect();
        let (mut t, mut col) = (0usize, 0usize);
        while t < toks.len() {
            match toks[t] {
                "k" => {
                    let f: Vec<i32> = toks.get(t + 1..t + 24)?.iter().filter_map(|x| x.parse().ok()).collect();
                    if f.len() != 23 {
                        return None;
                    }
                    t += 24;
                    // An optional origin direction follows.
                    if toks.get(t).is_some_and(|x| x.parse::<i32>().is_ok()) {
                        t += 1;
                    }
                    let ground = |k: usize| -> Option<String> {
                        let g = f[14 + k];
                        (g > 0).then(|| strings.get(g as usize - 1).cloned()).flatten()
                    };
                    slots.push(KSlot {
                        col,
                        row,
                        w: f[0].max(1) as usize,
                        h: f[1].max(1) as usize,
                        grounds: [ground(0), ground(1), ground(2), ground(3)],
                        heights: [f[18], f[19], f[20], f[21]],
                    });
                }
                "f" => t += 2,
                _ => t += 1,
            }
            col += 1;
        }
    }
    Some(slots)
}

/// Ground for the slots of a room: each slot is a patch of `w` x `h` grid units
/// (250 world units each), split into small cells that take the ground type of
/// their nearest corner. `material` maps a `.gt` path to a `.mat` path.
pub fn ground_from_slots(out: &mut Mesh, slots: &[KSlot], material: &dyn Fn(&str) -> String) {
    use std::collections::BTreeMap;
    const UNIT: f32 = CELL * CELLS_PER_SIDE as f32;
    const SPLIT: usize = 2;
    // Per material: quads as [x0, y0, x1, y1].
    let mut quads: BTreeMap<String, Vec<[f32; 4]>> = BTreeMap::new();
    for s in slots {
        if s.grounds.iter().all(Option::is_none) {
            continue;
        }
        let (nx, ny) = (s.w * SPLIT, s.h * SPLIT);
        for cy in 0..ny {
            for cx in 0..nx {
                // Pick the nearest corner of SW (0,1), SE (1,1), NE (1,0), NW (0,0).
                let (fx, fy) = ((cx as f32 + 0.5) / nx as f32, (cy as f32 + 0.5) / ny as f32);
                let corner = match (fx >= 0.5, fy >= 0.5) {
                    (false, true) => 0,
                    (true, true) => 1,
                    (true, false) => 2,
                    (false, false) => 3,
                };
                let Some(gt) = s.grounds[corner].as_deref().or_else(|| s.grounds.iter().flatten().next().map(String::as_str)) else {
                    continue;
                };
                let x0 = (s.col as f32 + cx as f32 / SPLIT as f32) * UNIT;
                let y0 = (s.row as f32 + cy as f32 / SPLIT as f32) * UNIT;
                quads.entry(material(gt)).or_default().push([x0, y0, x0 + UNIT / SPLIT as f32, y0 + UNIT / SPLIT as f32]);
            }
        }
    }
    for (mat, list) in quads {
        let first = out.indices.len() as u32;
        for [x0, y0, x1, y1] in &list {
            let base = out.positions.len() as u32;
            for (x, y) in [(*x0, *y0), (*x1, *y0), (*x1, *y1), (*x0, *y1)] {
                out.positions.push([x, y, 0.0]);
                out.normals.push([0.0, 0.0, -1.0]);
                out.uvs.push([x / UNIT, y / UNIT]);
            }
            out.indices.extend([0, 1, 2, 0, 2, 3].map(|k| k + base));
        }
        out.shapes.push(Shape { name: "ground".into(), material: mat, first_index: first, index_count: list.len() as u32 * 6 });
    }
}

/// The mesh an `.ao` descriptor points at (`fixed_mesh = "…"`), if any.
pub fn ao_fixed_mesh(bytes: &[u8]) -> Option<String> {
    let text = decode(bytes).ok()?;
    text.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("fixed_mesh"))
        .and_then(|r| first_quoted(r))
        .map(str::to_string)
}

/// Add `part` to `out`, rotated about the vertical (model Z) axis by
/// `rotation`, scaled and moved by `offset`.
pub fn append_placed(out: &mut Mesh, part: &Mesh, rotation: f32, scale: f32, offset: [f32; 3]) {
    let (s, c) = rotation.sin_cos();
    let rot = |v: [f32; 3]| [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
    let base = out.positions.len() as u32;
    let first = out.indices.len() as u32;
    for p in &part.positions {
        let r = rot(*p);
        out.positions.push([r[0] * scale + offset[0], r[1] * scale + offset[1], r[2] * scale + offset[2]]);
    }
    out.normals.extend(part.normals.iter().map(|&n| rot(n)));
    out.uvs.extend_from_slice(&part.uvs);
    out.indices.extend(part.indices.iter().map(|i| i + base));
    for sh in &part.shapes {
        out.shapes.push(Shape {
            name: sh.name.clone(),
            material: sh.material.clone(),
            first_index: first + sh.first_index,
            index_count: sh.index_count,
        });
    }
}

/// A flat square of `size` world units on the ground plane (z = 0), facing up.
pub fn ground_quad(out: &mut Mesh, size: f32) {
    let base = out.positions.len() as u32;
    let first = out.indices.len() as u32;
    for (x, y) in [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)] {
        out.positions.push([x, y, 0.0]);
        out.normals.push([0.0, 0.0, -1.0]); // up is -Z
        out.uvs.push([x / size, y / size]);
    }
    out.indices.extend([0, 1, 2, 0, 2, 3].map(|i| i + base));
    out.shapes.push(Shape { name: "ground".into(), material: String::new(), first_index: first, index_count: 6 });
}

pub fn recompute_bbox(m: &mut Mesh) {
    let mut b = [f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MAX, f32::MIN];
    for p in &m.positions {
        for k in 0..3 {
            b[k * 2] = b[k * 2].min(p[k]);
            b[k * 2 + 1] = b[k * 2 + 1].max(p[k]);
        }
    }
    m.bbox = b;
}
