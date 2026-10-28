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
