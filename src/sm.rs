//! `.sm` files: small UTF-16 text descriptors that accompany a skinned mesh
//! (`.smd`) and name the materials its shapes use.

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct SkinnedMeshInfo {
    /// Path of the `.smd` this descriptor belongs to.
    pub mesh: String,
    /// `(material path, number of consecutive shapes using it)`.
    pub materials: Vec<(String, usize)>,
}

fn decode(b: &[u8]) -> Result<String> {
    let b = b.strip_prefix(&[0xff, 0xfe]).unwrap_or(b);
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Ok(String::from_utf16(&units)?)
}

/// Split `"quoted path" rest` into the path and the rest of the line.
fn quoted(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some((&rest[..end], rest[end + 1..].trim()))
}

pub fn parse(bytes: &[u8]) -> Result<SkinnedMeshInfo> {
    let text = decode(bytes)?;
    let mut mesh = None;
    let mut materials = Vec::new();
    let mut remaining = 0usize;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("SkinnedMeshData") {
            mesh = quoted(rest).map(|(p, _)| p.to_string());
        } else if let Some(n) = line.strip_prefix("Materials") {
            remaining = n.trim().parse().unwrap_or(0);
        } else if remaining > 0 {
            if let Some((path, count)) = quoted(line) {
                materials.push((path.to_string(), count.parse().unwrap_or(1)));
                remaining -= 1;
            }
        }
    }
    Ok(SkinnedMeshInfo { mesh: mesh.context("no SkinnedMeshData line")?, materials })
}

/// The `.sm` that most likely describes `smd_path`: same directory, file name
/// without the trailing `_<hash>` before the extension.
pub fn descriptor_path(smd_path: &str) -> Option<String> {
    let stem = smd_path.strip_suffix(".smd")?;
    let (head, hash) = stem.rsplit_once('_')?;
    (hash.len() == 8 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("{head}.sm"))
}
