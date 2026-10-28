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
    pub shapes: Vec<Shape>,
    pub bbox: [f32; 6],
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.d.get(self.p..self.p + n).context("unexpected end of mesh file")?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn bbox(&mut self) -> Result<[f32; 6]> {
        let mut b = [0.0; 6];
        for v in &mut b {
            *v = self.f32()?;
        }
        Ok(b)
    }
}

fn f16(bits: u16) -> f32 {
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
fn dolm(c: &mut Cur) -> Result<Mesh> {
    ensure!(c.take(4)? == b"DOLm", "missing DOLm marker");
    let variant = c.u16()?;
    let lods = c.u8()? as usize;
    let shapes = c.u16()? as usize;
    let fmt = c.u32()?;
    ensure!(lods > 0, "model has no LODs");

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
                c.take(8)?; // bone indices + weights
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
    if variant == 4 {
        c.take(4)?;
    }
    Ok(out)
}

/// `.smd`, version 3 layout.
pub fn parse_smd(data: &[u8]) -> Result<Mesh> {
    let mut c = Cur { d: data, p: 0 };
    let version = c.u8()?;
    if version < 3 {
        bail!("smd version {version} is not supported yet");
    }
    c.u8()?; // vertex format
    let shapes = c.u16()? as usize;
    c.u32()?; // bytes in the name section
    let bbox = c.bbox()?;
    let mut mesh = dolm(&mut c)?;
    let lens: Vec<usize> = (0..shapes).map(|_| c.u32().map(|v| v as usize)).collect::<Result<_>>()?;
    for (i, len) in lens.into_iter().enumerate() {
        let raw = c.take(len)?;
        let units: Vec<u16> = raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        if let Some(s) = mesh.shapes.get_mut(i) {
            s.name = String::from_utf16_lossy(&units);
        }
    }
    mesh.bbox = bbox;
    Ok(mesh)
}

/// `.fmt`, version 9 layout.
pub fn parse_fmt(data: &[u8]) -> Result<Mesh> {
    let mut c = Cur { d: data, p: 0 };
    let version = c.u8()?;
    if version < 9 {
        bail!("fmt version {version} is not supported yet");
    }
    let shapes = c.u16()? as usize;
    let subs = c.u8()? as usize;
    let _d1_total = c.u16()?;
    let d3s = c.u8()? as usize;
    let bbox = c.bbox()?;
    let mut mesh = dolm(&mut c)?;

    let shape_refs: Vec<(u32, u32)> =
        (0..shapes).map(|_| Ok((c.u32()?, c.u32()?))).collect::<Result<_>>()?;
    let mut d1_counts = Vec::new();
    for _ in 0..subs {
        c.u8()?;
        d1_counts.push(c.u8()? as usize);
        c.u32()?;
    }
    for n in d1_counts {
        c.take(12 * n)?;
    }
    c.take(87 * d3s)?;
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
    for (s, (name, material)) in mesh.shapes.iter_mut().zip(shape_refs) {
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
