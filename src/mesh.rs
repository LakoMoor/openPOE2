//! Mesh loading for `.smd` (v3) and `.fmt` (v9) files, both of which embed a
//! `DOLm` geometry block.
//!
//! The field layout was worked out with the help of the format notes in the
//! community project `poe_data_tools` (MIT/Apache-2.0) and checked against the
//! installed game files.

use anyhow::{bail, ensure, Context, Result};

#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub name: String,
    pub material: String,
    pub first_index: u32,
    pub index_count: u32,
}

/// Level-of-detail 0 of a model.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Bone indices and weights per vertex (empty for static meshes).
    pub joints: Vec<[u8; 4]>,
    pub weights: Vec<[u8; 4]>,
    /// Skeleton the joints refer to (characters only).
    pub rig: Option<std::sync::Arc<crate::skeleton::Skeleton>>,
    pub shapes: Vec<Shape>,
    pub bbox: [f32; 6],
}

pub(crate) struct Cur<'a> {
    pub(crate) d: &'a [u8],
    pub(crate) p: usize,
}

impl<'a> Cur<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.d.get(self.p..self.p + n).context("unexpected end of mesh file")?;
        self.p += n;
        Ok(s)
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub(crate) fn bbox(&mut self) -> Result<[f32; 6]> {
        let mut b = [0.0; 6];
        for v in &mut b {
            *v = self.f32()?;
        }
        Ok(b)
    }
}

pub(crate) fn f16(bits: u16) -> f32 {
    let sign = if bits >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = ((bits >> 10) & 0x1f) as i32;
    let man = (bits & 0x3ff) as f32;
    match exp {
        0 => sign * man * 2f32.powi(-24),
        31 => if man == 0.0 { sign * f32::INFINITY } else { f32::NAN },
        e => sign * (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Parse a `DOLm` block, returning LOD 0 and the shape index ranges.
pub(crate) fn dolm(c: &mut Cur) -> Result<Mesh> {
    ensure!(c.take(4)? == b"DOLm", "missing DOLm marker");
    let variant = c.u16()?;
    let lods = c.u8()? as usize;
    let shapes = c.u16()? as usize;
    let fmt = c.u32()?;

    let extents: Vec<(u32, u32)> =
        (0..lods).map(|_| Ok((c.u32()?, c.u32()?))).collect::<Result<_>>()?;

    let mut out = Mesh::default();
    for (lod, &(tris, verts)) in extents.iter().enumerate() {
        let ranges: Vec<(u32, u32)> =
            (0..shapes).map(|_| Ok((c.u32()?, c.u32()?))).collect::<Result<_>>()?;
        let count = tris as usize * 3;
        let indices: Vec<u32> = if verts < 0x10000 {
            (0..count).map(|_| c.u16().map(u32::from)).collect::<Result<_>>()?
        } else {
            (0..count).map(|_| c.u32()).collect::<Result<_>>()?
        };

        let (mut pos, mut nor, mut uv) = (Vec::new(), Vec::new(), Vec::new());
        let (mut jn, mut wt) = (Vec::new(), Vec::new());
        for _ in 0..verts {
            pos.push([c.f32()?, c.f32()?, c.f32()?]);
            let n = c.take(4)?;
            nor.push([n[0] as i8 as f32 / 127.0, n[1] as i8 as f32 / 127.0, n[2] as i8 as f32 / 127.0]);
            c.take(4)?; // tangent
            let mut t = [0.0; 2];
            if fmt >> 3 & 1 == 1 {
                t = [f16(c.u16()?), f16(c.u16()?)];
            }
            if fmt >> 2 & 1 == 1 {
                let b = c.take(8)?; // bone indices + weights
                jn.push([b[0], b[1], b[2], b[3]]);
                wt.push([b[4], b[5], b[6], b[7]]);
            }
            if fmt >> 1 & 1 == 1 {
                c.take(4)?;
            }
            if fmt & 1 == 1 {
                c.take(4)?; // second UV set
            }
            if fmt >> 6 & 1 == 1 {
                c.take(4)?;
            }
            uv.push(t);
        }
        if lod == 0 {
            out.positions = pos;
            out.normals = nor;
            out.uvs = uv;
            out.joints = jn;
            out.weights = wt;
            out.indices = indices;
            out.shapes = ranges
                .into_iter()
                .map(|(first_index, index_count)| Shape { first_index, index_count, ..Default::default() })
                .collect();
        }
    }

    // Trailing per-model blocks, needed to reach whatever follows the block.
    if fmt >> 6 & 1 == 1 {
        c.take(36 * shapes)?;
        if variant == 2 {
            c.take(4 * shapes)?;
        }
    }
    if variant == 4 && lods > 0 {
        c.take(4)?;
    }
    Ok(out)
}

fn utf16(raw: &[u8]) -> String {
    let units: Vec<u16> = raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    String::from_utf16_lossy(&units)
}

fn read_indices(c: &mut Cur, verts: u32, tris: u32) -> Result<Vec<u32>> {
    let count = tris as usize * 3;
    if verts < 0x10000 {
        (0..count).map(|_| c.u16().map(u32::from)).collect()
    } else {
        (0..count).map(|_| c.u32()).collect()
    }
}

fn normal(n: &[u8]) -> [f32; 3] {
    [n[0] as i8 as f32 / 127.0, n[1] as i8 as f32 / 127.0, n[2] as i8 as f32 / 127.0]
}

/// Turn "first triangle of each shape" markers into index ranges.
fn shapes_from_starts(starts: &[u32], total_tris: u32) -> Vec<Shape> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(total_tris);
            Shape { first_index: start * 3, index_count: end.saturating_sub(start) * 3, ..Default::default() }
        })
        .collect()
}

/// `.smd`, version 3 layout.
fn parse_smd_v3(c: &mut Cur) -> Result<Mesh> {
    c.u8()?; // vertex format
    let shapes = c.u16()? as usize;
    c.u32()?; // bytes in the name section
    let bbox = c.bbox()?;
    let mut mesh = dolm(c)?;
    let lens: Vec<usize> = (0..shapes).map(|_| c.u32().map(|v| v as usize)).collect::<Result<_>>()?;
    for (i, len) in lens.into_iter().enumerate() {
        let name = utf16(c.take(len)?);
        if let Some(s) = mesh.shapes.get_mut(i) {
            s.name = name;
        }
    }
    mesh.bbox = bbox;
    Ok(mesh)
}

/// `.smd`, versions 1 and 2: flat vertex/index buffers.
fn parse_smd_v1_2(c: &mut Cur, version: u8) -> Result<Mesh> {
    let tris = c.u32()?;
    let verts = c.u32()?;
    let fmt = c.u8()?;
    let shapes = c.u16()? as usize;
    c.u32()?; // bytes in the name section
    let bbox = c.bbox()?;
    if version == 2 {
        c.u32()?;
    }
    let extents: Vec<(usize, u32)> =
        (0..shapes).map(|_| Ok((c.u32()? as usize, c.u32()?))).collect::<Result<_>>()?;
    let names: Vec<String> = extents.iter().map(|&(len, _)| c.take(len).map(utf16)).collect::<Result<_>>()?;
    let indices = read_indices(c, verts, tris)?;

    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    for _ in 0..verts {
        positions.push([c.f32()?, c.f32()?, c.f32()?]);
        normals.push(normal(c.take(4)?));
        c.take(4)?; // tangent
        uvs.push([f16(c.u16()?), f16(c.u16()?)]);
        let b = c.take(8)?; // bone indices + weights
        joints.push([b[0], b[1], b[2], b[3]]);
        weights.push([b[4], b[5], b[6], b[7]]);
        if fmt >> 1 & 1 == 1 {
            c.take(4)?;
        }
        if fmt & 1 == 1 {
            c.take(4)?;
        }
    }
    let starts: Vec<u32> = extents.iter().map(|e| e.1).collect();
    let mut shapes_out = shapes_from_starts(&starts, tris);
    for (s, n) in shapes_out.iter_mut().zip(names) {
        s.name = n;
    }
    Ok(Mesh { positions, normals, uvs, indices, joints, weights, shapes: shapes_out, bbox, rig: None })
}

pub fn parse_smd(data: &[u8]) -> Result<Mesh> {
    let mut c = Cur { d: data, p: 0 };
    match c.u8()? {
        v @ (1 | 2) => parse_smd_v1_2(&mut c, v),
        v if v >= 3 => parse_smd_v3(&mut c),
        v => bail!("smd version {v} is not supported"),
    }
}

/// `.fmt`: versions up to 8 store flat buffers, 9 embeds a `DOLm` block.
pub fn parse_fmt(data: &[u8]) -> Result<Mesh> {
    let mut c = Cur { d: data, p: 0 };
    let version = c.u8()?;
    let counts = if version < 9 { Some((c.u32()?, c.u32()?)) } else { None };
    let shapes = c.u16()? as usize;
    let subs = c.u8()? as usize;
    let _d1_total = c.u16()?;
    let d3s = c.u8()? as usize;
    let bbox = c.bbox()?;

    // (name offset, material offset, first triangle for flat layouts)
    let (mut mesh, shape_refs): (Mesh, Vec<(u32, u32, u32)>);
    let mut d1_counts = Vec::new();
    if let Some((tris, verts)) = counts {
        let vertex_format = if version >= 8 { c.u32()? } else { 0 };
        let refs: Vec<(u32, u32, u32)> =
            (0..shapes).map(|_| Ok((c.u32()?, c.u32()?, c.u32()?))).collect::<Result<_>>()?;
        for _ in 0..subs {
            c.u8()?;
            d1_counts.push(c.u8()? as usize);
            c.u32()?;
        }
        let indices = read_indices(&mut c, verts, tris)?;
        let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..verts {
            positions.push([c.f32()?, c.f32()?, c.f32()?]);
            normals.push(normal(c.take(4)?));
            c.take(4)?; // tangent
            uvs.push([f16(c.u16()?), f16(c.u16()?)]);
            if vertex_format == 1 {
                c.take(4)?;
            }
        }
        let starts: Vec<u32> = refs.iter().map(|r| r.2).collect();
        mesh = Mesh { positions, normals, uvs, indices, joints: Vec::new(), weights: Vec::new(), rig: None, shapes: shapes_from_starts(&starts, tris), bbox };
        shape_refs = refs;
    } else {
        mesh = dolm(&mut c)?;
        shape_refs = (0..shapes).map(|_| Ok((c.u32()?, c.u32()?, 0))).collect::<Result<_>>()?;
        for _ in 0..subs {
            c.u8()?;
            d1_counts.push(c.u8()? as usize);
            c.u32()?;
        }
    }
    for n in d1_counts {
        c.take(12 * n)?;
    }
    let d3_width = match version {
        0..3 => 45,
        3 => 70,
        4..6 => 78,
        6 => 83,
        _ => 87,
    };
    c.take(d3_width * d3s)?;
    let len = c.u32()? as usize;
    let table: Vec<u16> = c
        .take(len * 2)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let read = |off: u32| -> String {
        let tail = table.get(off as usize..).unwrap_or(&[]);
        let end = tail.iter().position(|&u| u == 0).unwrap_or(tail.len());
        String::from_utf16_lossy(&tail[..end])
    };
    for (s, (name, material, _)) in mesh.shapes.iter_mut().zip(shape_refs) {
        s.name = read(name);
        s.material = read(material);
    }
    mesh.bbox = bbox;
    Ok(mesh)
}

/// Pick the parser from the file extension.
pub fn parse(path: &str, data: &[u8]) -> Result<Mesh> {
    if path.ends_with(".smd") {
        parse_smd(data)
    } else if path.ends_with(".fmt") {
        parse_fmt(data)
    } else {
        bail!("{path}: not a mesh file")
    }
}
