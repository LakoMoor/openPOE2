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

#[derive(Debug, Clone)]
pub struct Room {
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
    Ok(Room { version, side, doodads })
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
