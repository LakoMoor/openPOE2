//! Helpers shared by the preview tools: resolve a mesh's materials to
//! decoded base-colour textures.

use std::io::Cursor;
use std::path::Path;

use image::RgbaImage;
use image_dds::ddsfile::Dds;

use crate::{index::Index, material, mesh::Mesh, sm};

#[derive(Clone)]
pub struct ShapeTex {
    pub img: RgbaImage,
    /// Treat the alpha channel as cut-out transparency.
    pub alpha_test: bool,
    /// Helper material that must not be drawn.
    pub invisible: bool,
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
        if m.invisible {
            return Some(ShapeTex { img: RgbaImage::new(1, 1), alpha_test: false, invisible: true });
        }
        let dds = Dds::read(&mut Cursor::new(idx.read_file(dir, &m.albedo?).ok()?)).ok()?;
        let img = image_dds::image_from_dds(&dds, 0).ok()?;
        Some(ShapeTex { img, alpha_test: !m.opaque, invisible: false })
    };
    // Many shapes share a material: decode each one once.
    let mut cache: std::collections::HashMap<String, Option<ShapeTex>> = std::collections::HashMap::new();
    shape_materials(idx, dir, mesh_path, mesh)
        .iter()
        .map(|m| cache.entry(m.clone()).or_insert_with(|| load(m)).clone())
        .collect()
}

/// Load any supported geometry: a model (`.fmt`, `.smd`) or a whole terrain tile (`.tgt`).
pub fn load_mesh(idx: &Index, dir: &Path, path: &str) -> anyhow::Result<Mesh> {
    use anyhow::Context;
    if path.ends_with(".ao") {
        return load_character(idx, dir, path);
    }
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
            // The ground layer has no material list in the .tgt; draw it untextured.
            crate::terrain::append(&mut out, &tgm.ground, &[], &tgt.materials);
        }
    }
    out.bbox = bbox.unwrap_or_default();
    Ok(out)
}

/// Ground materials of an area: its `master.tsi` names a `materials.mtd`, which
/// maps ground type names to materials.
struct AreaGround {
    by_name: std::collections::HashMap<String, String>,
}

impl AreaGround {
    fn load(idx: &Index, dir: &Path, room_path: &str) -> Self {
        let mut by_name = std::collections::HashMap::new();
        let mut parts: Vec<&str> = room_path.split('/').collect();
        while parts.pop().is_some() && !parts.is_empty() {
            let area = parts.join("/");
            let Ok(tsi) = idx.read_file(dir, &format!("{area}/master.tsi")) else { continue };
            let text = String::from_utf16_lossy(
                &tsi.strip_prefix(&[0xff, 0xfe]).unwrap_or(&tsi).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
            );
            let list = text.lines().find(|l| l.trim_start().starts_with("MaterialsList")).and_then(|l| l.split('"').nth(1).map(str::to_string));
            if let Some(mtd) = list.and_then(|f| idx.read_file(dir, &format!("{area}/{f}")).ok()) {
                by_name = crate::ground::parse_mtd(&mtd);
            }
            break;
        }
        Self { by_name }
    }

    /// Material for the ground type stored in `gt_path`.
    fn material(&self, idx: &Index, dir: &Path, gt_path: &str) -> String {
        let name = idx
            .read_file(dir, gt_path)
            .ok()
            .and_then(|b| crate::ground::gt_name(&b))
            .map(|n| n.to_ascii_lowercase())
            .unwrap_or_default();
        self.by_name.get(&name).or_else(|| self.by_name.get("")).cloned().unwrap_or_default()
    }
}

/// A room: a ground plane with its doodads placed on it.
fn load_room(idx: &Index, dir: &Path, path: &str) -> anyhow::Result<Mesh> {
    use crate::room;
    use std::collections::HashMap;
    let r = room::parse_arm(&idx.read_file(dir, path)?)?;
    let mut out = Mesh::default();
    let side = r.side.unwrap_or(2).max(1) as f32 * room::CELLS_PER_SIDE as f32 * room::CELL;
    if r.slots.is_empty() {
        room::ground_quad(&mut out, side);
    } else {
        let ground = AreaGround::load(idx, dir, path);
        room::ground_from_slots(&mut out, &r.slots, &|gt: &str| ground.material(idx, dir, gt));
    }

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
        // Blockers and other editor helpers are invisible in the game.
        let lower = mesh_path.to_ascii_lowercase();
        if lower.contains("blocker") || lower.contains("/tools/") {
            continue;
        }
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

/// Load the skinned mesh named by an `.sm` descriptor, with its shapes' materials
/// taken from `overrides` (by shape name) or else from the descriptor.
fn load_skin(idx: &Index, dir: &Path, sm_path: &str, overrides: &std::collections::HashMap<String, String>) -> anyhow::Result<Mesh> {
    let info = sm::parse(&idx.read_file(dir, sm_path)?)?;
    let mut mesh = crate::mesh::parse(&info.mesh, &idx.read_file(dir, &info.mesh)?)?;
    let from_sm: Vec<String> =
        info.materials.iter().flat_map(|(m, n)| std::iter::repeat_n(m.clone(), *n)).collect();
    for (i, s) in mesh.shapes.iter_mut().enumerate() {
        s.material = overrides
            .get(&s.name)
            .cloned()
            .or_else(|| from_sm.get(i).cloned())
            .unwrap_or_default();
    }
    Ok(mesh)
}

/// Names of base shapes in `list` that `piece` covers: at least half of a shape's
/// vertices lie close to some vertex of the piece. Upper arms always stay.
fn covered_shapes(base: &Mesh, piece: &Mesh, xf: &glam::Mat4, list: &[String]) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    const R: f32 = 4.0;
    let cell = |p: glam::Vec3| [(p.x / R).floor() as i32, (p.y / R).floor() as i32, (p.z / R).floor() as i32];
    let mut grid: HashMap<[i32; 3], Vec<glam::Vec3>> = HashMap::new();
    for p in &piece.positions {
        let p = xf.transform_point3(glam::Vec3::from(*p));
        grid.entry(cell(p)).or_default().push(p);
    }
    let near = |p: glam::Vec3| {
        let c = cell(p);
        (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dz| {
                    grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]).is_some_and(|v| v.iter().any(|q| q.distance(p) <= R))
                })
            })
        })
    };
    let mut out = Vec::new();
    for s in &base.shapes {
        if !list.contains(&s.name) || s.name.contains("upperArm") {
            continue;
        }
        let range = s.first_index as usize..(s.first_index + s.index_count) as usize;
        let verts: HashSet<u32> = base.indices[range].iter().copied().collect();
        let close = verts.iter().filter(|&&i| near(glam::Vec3::from(base.positions[i as usize]))).count();
        if close * 2 >= verts.len() {
            out.push(s.name.clone());
        }
    }
    out
}

/// A character in its rest pose: the base body plus outfit pieces. Outfit pieces
/// replace the matching base segments (body, gloves, boots); pieces attached to
/// a bone or socket are moved to that bone's rest position.
fn load_character(idx: &Index, dir: &Path, path: &str) -> anyhow::Result<Mesh> {
    use crate::character::{parse_ao, Attachment};
    use crate::skeleton::Skeleton;
    use glam::{Mat4, Vec3};
    use std::collections::HashSet;

    let root = parse_ao(&idx.read_file(dir, path)?)?;
    // Bones and sockets come from the shared character descriptor.
    let shared = idx.read_file(dir, "metadata/characters/character.ao").ok().and_then(|b| parse_ao(&b).ok());
    let skeleton = idx
        .read_file(dir, "art/models/charactersfour/onerig.ast")
        .ok()
        .and_then(|b| Skeleton::parse(&b).ok());
    let sockets = shared.map(|s| s.sockets).unwrap_or_default();
    // Transform of a bone or socket in rest pose, and the bone it ends up on.
    let resolve = |name: &str| -> (Mat4, usize) {
        let Some(skel) = &skeleton else { return (Mat4::IDENTITY, 0) };
        let mut name = name.to_string();
        let mut extra = Mat4::IDENTITY;
        for _ in 0..6 {
            if let Some(i) = skel.index_of(&name) {
                return (skel.world(i) * extra, i);
            }
            match sockets.get(&name.to_ascii_lowercase()) {
                Some((parent, t)) if !parent.is_empty() => {
                    extra = Mat4::from_translation(Vec3::from(*t)) * extra;
                    name = parent.clone();
                }
                _ => break,
            }
        }
        (Mat4::IDENTITY, 0)
    };

    // (mesh, is_base, transform, bone for rigid parts)
    let mut parts: Vec<(Mesh, bool, Mat4, usize)> = Vec::new();
    if let Some(skin) = &root.skin {
        parts.push((load_skin(idx, dir, skin, &root.shape_materials)?, true, Mat4::IDENTITY, 0));
    }

    let mut hidden: HashSet<String> = HashSet::new();
    let mut queue: Vec<(Attachment, usize, Mat4, usize)> = root
        .attachments
        .iter()
        .map(|a| {
            let (xf, bone) = if a.bone == "<root>" { (Mat4::IDENTITY, 0) } else { resolve(&a.bone) };
            (a.clone(), 1, xf, bone)
        })
        .collect();
    while let Some((att, depth, xf, bone)) = queue.pop() {
        let Ok(bytes) = idx.read_file(dir, &att.path) else { continue };
        let Ok(ao) = parse_ao(&bytes) else { continue };
        if let Some(skin) = &ao.skin {
            if let Ok(m) = load_skin(idx, dir, skin, &ao.shape_materials) {
                let slot = att.alias.strip_suffix("_Attach").unwrap_or("");
                if !matches!(slot, "" | "Head" | "Face") {
                    if let Some(list) = root.segments.get(&format!("{slot}_Seg")) {
                        // Keep the upper arms: outfit bodies don't cover them.
                        if let Some(base_mesh) = parts.first().map(|p| &p.0) {
                            hidden.extend(covered_shapes(base_mesh, &m, &xf, list));
                        }
                    }
                }
                // Pieces attached to a bone (hair, necklace, tassle) are skinned to their own
                // small rig; keep them rigid on the bone they hang from.
                let m = if att.bone == "<root>" { m } else { Mesh { joints: Vec::new(), weights: Vec::new(), ..m } };
                parts.push((m, false, xf, bone));
            }
        }
        if depth < 4 {
            queue.extend(ao.attachments.into_iter().map(|a| {
                if a.bone == "<root>" {
                    (a, depth + 1, xf, bone)
                } else {
                    let (local, b) = resolve(&a.bone);
                    (a, depth + 1, xf * local, b)
                }
            }));
        }
    }

    let mut out = Mesh::default();
    for (mesh, is_base, xf, bone) in &parts {
        let base = out.positions.len() as u32;
        out.positions.extend(mesh.positions.iter().map(|p| xf.transform_point3(Vec3::from(*p)).to_array()));
        out.normals.extend(mesh.normals.iter().map(|n| xf.transform_vector3(Vec3::from(*n)).to_array()));
        out.uvs.extend_from_slice(&mesh.uvs);
        if mesh.joints.len() == mesh.positions.len() {
            out.joints.extend_from_slice(&mesh.joints);
            out.weights.extend_from_slice(&mesh.weights);
        } else {
            // Rigid piece: follow the bone it is attached to.
            let rigid = (0..mesh.positions.len()).map(|_| ([*bone as u8, 0, 0, 0], [255, 0, 0, 0]));
            for (j, w) in rigid {
                out.joints.push(j);
                out.weights.push(w);
            }
        }
        let first = out.indices.len() as u32;
        out.indices.extend(mesh.indices.iter().map(|i| i + base));
        for s in &mesh.shapes {
            if *is_base && hidden.contains(&s.name) {
                continue;
            }
            out.shapes.push(crate::mesh::Shape {
                name: s.name.clone(),
                material: s.material.clone(),
                first_index: first + s.first_index,
                index_count: s.index_count,
            });
        }
    }
    out.rig = skeleton.map(std::sync::Arc::new);
    anyhow::ensure!(!out.positions.is_empty(), "{path}: no skinned mesh found");
    crate::room::recompute_bbox(&mut out);
    Ok(out)
}
