//! Animated-object (`.ao`) descriptors as used by characters: skinned meshes,
//! per-shape materials and attachments.

use std::collections::HashMap;

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Attachment {
    /// Bone the object is attached to (`<root>` for the root).
    pub bone: String,
    pub path: String,
    pub alias: String,
}

#[derive(Debug, Clone, Default)]
pub struct AoInfo {
    /// Skinned mesh descriptor (`.sm`).
    pub skin: Option<String>,
    /// Shape name -> material path.
    pub shape_materials: HashMap<String, String>,
    /// Segment alias (e.g. `Body_Seg`) -> shape names.
    pub segments: HashMap<String, Vec<String>>,
    pub attachments: Vec<Attachment>,
    pub fixed_mesh: Option<String>,
    /// Socket name (lower-case) -> (parent bone or socket, translation).
    pub sockets: HashMap<String, (String, [f32; 3])>,
}

fn decode(b: &[u8]) -> Result<String> {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(String::from_utf16(&units)?)
}

/// `key = "value"` -> (key, value)
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    let v = v.trim().strip_prefix('"')?;
    Some((k.trim(), &v[..v.rfind('"')?]))
}

pub fn parse_ao(bytes: &[u8]) -> Result<AoInfo> {
    let text = decode(bytes)?;
    let mut info = AoInfo::default();
    let mut socket: Option<String> = None;
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = key_value(line) else { continue };
        match key {
            "skin" => info.skin = Some(value.to_string()),
            "fixed_mesh" => info.fixed_mesh = Some(value.to_string()),
            "attached_object" => {
                // "<bone> <path> alias <name>"
                let mut it = value.split_whitespace();
                if let (Some(bone), Some(path)) = (it.next(), it.next()) {
                    let alias = value.split_once(" alias ").map(|(_, a)| a.trim().to_string()).unwrap_or_default();
                    info.attachments.push(Attachment { bone: bone.into(), path: path.into(), alias });
                }
            }
            "socket" => {
                socket = Some(value.to_ascii_lowercase());
                info.sockets.insert(value.to_ascii_lowercase(), (String::new(), [0.0; 3]));
            }
            "parent" => {
                if let Some(s) = info.sockets.get_mut(socket.as_deref().unwrap_or("")) {
                    s.0 = value.to_string();
                }
            }
            "translation" => {
                let v: Vec<f32> = value.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if let (Some(s), [x, y, z]) = (info.sockets.get_mut(socket.as_deref().unwrap_or("")), v.as_slice()) {
                    s.1 = [*x, *y, *z];
                }
            }
            "alias" => {
                if let Some((name, shapes)) = value.split_once(' ') {
                    let list = shapes.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                    info.segments.entry(name.to_string()).or_default().extend(list);
                }
            }
            _ if value.to_ascii_lowercase().contains(".mat") => {
                // "path.mat:0" - the suffix selects a variant we don't use.
                let path = value.split(".mat").next().map(|p| format!("{p}.mat")).unwrap_or_default();
                info.shape_materials.insert(key.to_string(), path);
            }
            _ => {}
        }
    }
    Ok(info)
}
