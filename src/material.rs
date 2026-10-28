//! `.mat` material files: UTF-16 JSON describing a shader graph instance and
//! the textures bound to it.

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Material {
    /// Path of the base-colour texture (`.dds`), if one is bound.
    pub albedo: Option<String>,
    /// Shader graph the material instantiates (`parent`).
    pub graph: String,
    /// False when the alpha channel of the colour texture means transparency
    /// (cut-out/blended graphs); true for graphs that reuse it as a mask.
    pub opaque: bool,
}

fn decode_text(b: &[u8]) -> Result<String> {
    if b.len() >= 2 && b[1] == 0 || b.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .skip((b.starts_with(&[0xff, 0xfe])) as usize)
            .collect();
        Ok(String::from_utf16(&units)?)
    } else {
        Ok(String::from_utf8(b.to_vec())?)
    }
}

pub fn parse(bytes: &[u8]) -> Result<Material> {
    let v: Value = serde_json::from_str(&decode_text(bytes)?).context("material is not valid JSON")?;
    let mut graph = String::new();
    let mut albedo = None;
    for inst in v["graphinstances"].as_array().into_iter().flatten() {
        if graph.is_empty() {
            graph = inst["parent"].as_str().unwrap_or("").to_string();
        }
        for cp in inst["custom_parameters"].as_array().into_iter().flatten() {
            let name = cp["name"].as_str().unwrap_or("").to_ascii_lowercase();
            if !(name.contains("albedo") || name.contains("colour") || name.contains("basecolor") || name.contains("diffuse")) {
                continue;
            }
            let path = cp["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|p| p["path"].as_str());
            if let (None, Some(p)) = (&albedo, path) {
                albedo = Some(p.to_string());
            }
        }
    }
    // Fall back to the first texture whose file name looks like colour.
    if albedo.is_none() {
        albedo = v["textures"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["filename"].as_str())
            .find(|f| {
                let l = f.to_ascii_lowercase();
                l.contains("colour") || l.contains("color") || l.contains("albedo") || l.contains("diffuse")
            })
            .map(str::to_string);
    }
    let opaque = graph.to_ascii_lowercase().contains("opaque");
    Ok(Material { albedo, graph, opaque })
}
