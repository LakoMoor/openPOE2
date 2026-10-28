//! Helpers shared by the preview tools: resolve a mesh's materials to
//! decoded base-colour textures.

use std::io::Cursor;
use std::path::Path;

use image::RgbaImage;
use image_dds::ddsfile::Dds;

use crate::{index::Index, material, mesh::Mesh, sm};

pub struct ShapeTex {
    pub img: RgbaImage,
    /// Treat the alpha channel as cut-out transparency.
    pub alpha_test: bool,
}

/// Material path for each shape. `.fmt` files name them directly; for `.smd`
/// they come from the companion `.sm`, handed out to shapes in order.
pub fn shape_materials(idx: &Index, dir: &Path, mesh_path: &str, mesh: &Mesh) -> Vec<String> {
    let mut mats: Vec<String> = mesh.shapes.iter().map(|s| s.material.clone()).collect();
    if mats.iter().all(|m| m.is_empty()) {
        // The descriptor usually shares the mesh's name, but may be named after
        // the directory, so try the guess first and then every `.sm` beside it.
        let dir_key = mesh_path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        let guess = sm::descriptor_path(mesh_path);
        let siblings = idx.sm_by_dir.get(&dir_key).into_iter().flatten();
        let info = guess
            .iter()
            .chain(siblings)
            .filter_map(|p| idx.read_file(dir, p).ok())
            .filter_map(|b| sm::parse(&b).ok())
            .find(|i| i.mesh.eq_ignore_ascii_case(mesh_path));
        if let Some(info) = info {
            let expanded = info.materials.iter().flat_map(|(m, n)| std::iter::repeat_n(m.clone(), *n));
            for (slot, m) in mats.iter_mut().zip(expanded) {
                *slot = m;
            }
        }
    }
    mats
}

/// One entry per shape; `None` when the material or texture can't be resolved.
pub fn load_shape_textures(idx: &Index, dir: &Path, mesh_path: &str, mesh: &Mesh) -> Vec<Option<ShapeTex>> {
    let load = |mat: &str| -> Option<ShapeTex> {
        if mat.is_empty() {
            return None;
        }
        let m = material::parse(&idx.read_file(dir, mat).ok()?).ok()?;
        let dds = Dds::read(&mut Cursor::new(idx.read_file(dir, &m.albedo?).ok()?)).ok()?;
        let img = image_dds::image_from_dds(&dds, 0).ok()?;
        Some(ShapeTex { img, alpha_test: !m.opaque })
    };
    shape_materials(idx, dir, mesh_path, mesh).iter().map(|m| load(m)).collect()
}

/// Load any supported geometry: a model (`.fmt`, `.smd`) or a whole terrain tile (`.tgt`).
pub fn load_mesh(idx: &Index, dir: &Path, path: &str) -> anyhow::Result<Mesh> {
    use anyhow::Context;
    if path.ends_with(".arm") {
        return load_room(idx, dir, path);
    }
    if !path.ends_with(".tgt") {
        return crate::mesh::parse(path, &idx.read_file(dir, path)?);
    }
    let tgt = crate::terrain::parse_tgt(&idx.read_file(dir, path)?)?;
    let mut out = Mesh::default();
    let mut bbox: Option<[f32; 6]> = None;
    for col in 1..=tgt.size.0 {
        for row in 1..=tgt.size.1 {
            let file = format!("{}_c{col}r{row}.tgm", tgt.mesh_root);
            let tgm = crate::terrain::parse_tgm(&idx.read_file(dir, &file).with_context(|| file.clone())?)
                .with_context(|| file.clone())?;
            let b = tgm.main.bbox;
            bbox = Some(match bbox {
                None => b,
                Some(a) => [a[0].min(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].max(b[3]), a[4].min(b[4]), a[5].max(b[5])],
            });
            // tgt lists sub-tiles from the top row down, columns left to right.
            let line = ((tgt.size.1 - row) * tgt.size.0 + (col - 1)) as usize;
            let slots = tgt.subtile_runs.get(line).map(|r| crate::terrain::expand_runs(r)).unwrap_or_default();
            crate::terrain::append(&mut out, &tgm.main, &slots, &tgt.materials);
        }
    }
    out.bbox = bbox.unwrap_or_default();
    Ok(out)
}

/// A room: a ground plane with its doodads placed on it.
fn load_room(idx: &Index, dir: &Path, path: &str) -> anyhow::Result<Mesh> {
    use crate::room;
    use std::collections::HashMap;
    let r = room::parse_arm(&idx.read_file(dir, path)?)?;
    let mut out = Mesh::default();
    let side = r.side.unwrap_or(2).max(1) as f32 * room::CELLS_PER_SIDE as f32 * room::CELL;
    room::ground_quad(&mut out, side);

    let mut cache: HashMap<String, Option<Mesh>> = HashMap::new();
    let mut placed = 0;
    for d in &r.doodads {
        let Some(mesh_path) = idx
            .read_file(dir, &d.ao)
            .ok()
            .and_then(|b| room::ao_fixed_mesh(&b))
        else {
            continue;
        };
        let part = cache.entry(mesh_path.clone()).or_insert_with(|| load_mesh(idx, dir, &mesh_path).ok());
        if let Some(part) = part {
            room::append_placed(&mut out, part, d.rotation, d.scale, [d.x * room::CELL, d.y * room::CELL, 0.0]);
            placed += 1;
        }
    }
    eprintln!("{path}: version {}, {} doodads listed, {placed} placed", r.version, r.doodads.len());
    room::recompute_bbox(&mut out);
    Ok(out)
}
