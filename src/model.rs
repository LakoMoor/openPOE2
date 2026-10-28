//! Helpers shared by the preview tools: resolve a mesh's materials to
//! decoded base-colour textures.

use std::io::Cursor;
use std::path::Path;

use image::RgbaImage;
use image_dds::ddsfile::Dds;

use crate::{index::Index, material, mesh::Mesh};

pub struct ShapeTex {
    pub img: RgbaImage,
    /// Treat the alpha channel as cut-out transparency.
    pub alpha_test: bool,
}

/// One entry per shape; `None` when the material or texture can't be resolved.
pub fn load_shape_textures(idx: &Index, dir: &Path, mesh: &Mesh) -> Vec<Option<ShapeTex>> {
    let load = |mat: &str| -> Option<ShapeTex> {
        if mat.is_empty() {
            return None;
        }
        let m = material::parse(&idx.read_file(dir, mat).ok()?).ok()?;
        let dds = Dds::read(&mut Cursor::new(idx.read_file(dir, &m.albedo?).ok()?)).ok()?;
        let img = image_dds::image_from_dds(&dds, 0).ok()?;
        Some(ShapeTex { img, alpha_test: !m.opaque })
    };
    mesh.shapes.iter().map(|s| load(&s.material)).collect()
}
