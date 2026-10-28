//! Terrain tiles: a `.tgt` descriptor (UTF-16 text) names a grid of sub-tiles,
//! each stored as a `.tgm` geometry file.

use anyhow::{Context, Result};

use crate::mesh::{dolm, f16, Cur, Mesh, Shape};

#[derive(Debug, Clone)]
pub struct Tgt {
    pub version: u32,
    /// Sub-tile grid size (columns, rows).
    pub size: (u32, u32),
    /// Path prefix of the sub-tile files (`<root>_c<col>r<row>.tgm`).
    pub mesh_root: String,
    pub materials: Vec<String>,
    /// Per sub-tile (rows from the top, columns left to right): runs of
    /// `(material index, number of consecutive shapes)`.
    pub subtile_runs: Vec<Vec<(u32, u32)>>,
}

fn decode(b: &[u8]) -> Result<String> {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(String::from_utf16(&units)?)
}

fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

pub fn parse_tgt(bytes: &[u8]) -> Result<Tgt> {
    let text = decode(bytes)?;
    let mut version = 0;
    let mut size = (1, 1);
    let mut mesh_root = None;
    let mut materials = Vec::new();
    let mut remaining = 0usize;
    let mut in_runs = false;
    let mut subtile_runs: Vec<Vec<(u32, u32)>> = Vec::new();
    for line in text.lines().map(str::trim) {
        if in_runs {
            let nums: Vec<u32> = line.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            if !nums.is_empty() && subtile_runs.len() < (size.0 * size.1) as usize {
                let n = nums[0] as usize;
                let rest = &nums[1..];
                let stride = if n > 0 && rest.len() == n * 3 { 3 } else { 2 };
                subtile_runs.push(rest.chunks_exact(stride).map(|c| (c[0], c[stride - 1])).collect());
                continue;
            }
        }
        if remaining > 0 {
            if let Some(p) = quoted(line) {
                materials.push(p.to_string());
                remaining -= 1;
                continue;
            }
        }
        if let Some(v) = line.strip_prefix("version ") {
            version = v.trim().parse()?;
        } else if let Some(sz) = line.strip_prefix("Size ") {
            let mut it = sz.split_whitespace().map(|n| n.parse::<u32>());
            if let (Some(Ok(w)), Some(Ok(h))) = (it.next(), it.next()) {
                size = (w, h);
            }
        } else if let Some(r) = line.strip_prefix("TileMeshRoot ") {
            mesh_root = quoted(r).map(str::to_string);
        } else if let Some(r) = line.strip_prefix("TileMesh ") {
            mesh_root = quoted(r).map(str::to_string);
        } else if let Some(n) = line.strip_prefix("NormalMaterials") {
            remaining = n.trim().parse().unwrap_or(0);
        } else if line.starts_with("SubTileMaterialIndices") {
            in_runs = true;
        }
    }
    Ok(Tgt { version, size, mesh_root: mesh_root.context("no TileMeshRoot")?, materials, subtile_runs })
}

/// Geometry of one sub-tile: the main surface and the ground layer below it.
pub struct Tgm {
    pub main: Mesh,
    /// Material slot of each shape of `main` (index into the tile's materials).
    pub main_slots: Vec<u32>,
    pub ground: Mesh,
    pub ground_slots: Vec<u32>,
}

/// Versions below 9: sizes up front, then per mesh the shape table, vertices and indices.
fn parse_tgm_old(c: &mut Cur, version: u8, bbox: [f32; 6]) -> Result<Tgm> {
    let mut info = [(0usize, 0u32, 0u32); 2];
    for i in &mut info {
        *i = (c.u16()? as usize, c.u32()?, c.u32()?);
    }
    let vertex_format = c.u8()?;
    let tail_count = c.u8()? as usize;
    if version == 8 {
        c.u32()?;
    }

    let mut parts = Vec::new();
    for (is_main, &(shapes, verts, tris)) in [true, false].iter().zip(&info) {
        let mut extents = Vec::with_capacity(shapes);
        let mut slots = Vec::with_capacity(shapes);
        for _ in 0..shapes {
            slots.push(c.u16()? as u32);
            c.take(24)?;
            extents.push((c.u32()?, c.u32()?));
        }
        let (mut pos, mut nor, mut uv) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..verts {
            pos.push([c.f32()?, c.f32()?, c.f32()?]);
            let n = c.take(4)?;
            nor.push([n[0] as i8 as f32 / 127.0, n[1] as i8 as f32 / 127.0, n[2] as i8 as f32 / 127.0]);
            c.take(4)?; // tangent
            if *is_main {
                uv.push([f16(c.u16()?), f16(c.u16()?)]);
            } else {
                // Ground vertices: format 3 carries two UV sets, format 2 a single unused pair.
                let mut t = [0.0; 2];
                if version >= 4 && vertex_format == 3 {
                    t = [f16(c.u16()?), f16(c.u16()?)];
                    c.take(4)?;
                } else if version >= 4 && vertex_format == 2 {
                    c.take(4)?;
                }
                uv.push(t);
            }
        }
        let count = tris as usize * 3;
        let indices: Vec<u32> = if verts < 0x10000 {
            (0..count).map(|_| c.u16().map(u32::from)).collect::<Result<_>>()?
        } else {
            (0..count).map(|_| c.u32()).collect::<Result<_>>()?
        };
        let shapes_out = extents
            .into_iter()
            .map(|(first_index, index_count)| Shape { first_index, index_count, ..Default::default() })
            .collect();
        parts.push((Mesh { positions: pos, normals: nor, uvs: uv, indices, shapes: shapes_out, bbox, ..Default::default() }, slots));
    }
    let _ = tail_count; // trailing entries are not needed
    let (ground, ground_slots) = parts.pop().unwrap();
    let (main, main_slots) = parts.pop().unwrap();
    Ok(Tgm { main, main_slots, ground, ground_slots })
}

/// Parse a v9+ `.tgm` (the layout used by current content).
pub fn parse_tgm(data: &[u8]) -> Result<Tgm> {
    let mut c = Cur { d: data, p: 0 };
    let version = c.u8()?;
    let bbox = c.bbox()?;
    if version < 9 {
        return parse_tgm_old(&mut c, version, bbox);
    }
    c.u16()?; // shape count
    c.u16()?;
    c.u8()?;
    let _tail_count = c.u8()?;

    let mut parts = Vec::new();
    for is_main in [true, false] {
        let mut mesh = dolm(&mut c)?;
        let n = mesh.shapes.len();
        let mut slots = Vec::with_capacity(n);
        for _ in 0..n {
            // Material slot: u16 for v9 and for ground meshes, u32 for main meshes after v9.
            let slot = if version == 9 || !is_main { c.u16()? as u32 } else { c.u32()? };
            slots.push(slot);
            if version >= 12 && is_main {
                c.u16()?;
            }
            c.take(24)?; // per-shape bounding box
        }
        mesh.bbox = bbox;
        parts.push((mesh, slots));
    }
    let (ground, ground_slots) = parts.pop().unwrap();
    let (main, main_slots) = parts.pop().unwrap();
    Ok(Tgm { main, main_slots, ground, ground_slots })
}

/// Append `part` to `out`, giving each shape the material for its slot.
pub fn append(out: &mut Mesh, part: &Mesh, slots: &[u32], materials: &[String]) {
    let base = out.positions.len() as u32;
    let first = out.indices.len() as u32;
    out.positions.extend_from_slice(&part.positions);
    out.normals.extend_from_slice(&part.normals);
    out.uvs.extend_from_slice(&part.uvs);
    out.indices.extend(part.indices.iter().map(|i| i + base));
    // Shapes without a slot are left untextured.
    for (i, sh) in part.shapes.iter().enumerate() {
        out.shapes.push(Shape {
            name: sh.name.clone(),
            material: slots.get(i).and_then(|&s| materials.get(s as usize)).cloned().unwrap_or_default(),
            first_index: first + sh.first_index,
            index_count: sh.index_count,
        });
    }
}

/// Material index for each shape of a sub-tile, from its `(material, count)` runs.
pub fn expand_runs(runs: &[(u32, u32)]) -> Vec<u32> {
    runs.iter().flat_map(|&(m, n)| std::iter::repeat_n(m, n as usize)).collect()
}
