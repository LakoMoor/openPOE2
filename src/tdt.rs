//! Header of `.tdt` tile definition files (versions 14 and 15).
//!
//! Layout: `u32 version, u32 n_chars`, `n_chars` UTF-16 code units holding
//! NUL-separated strings, then `u32 len(tgt), u32 0, u32 len(tgt)`, four edge
//! string offsets, `u8 w, u8 h` (size in sub-tiles), four ground string offsets
//! and a zero `u32`. Offsets count UTF-16 units into the string table, `-1` is
//! none. The packed grid data that follows the header is not decoded.

use anyhow::{ensure, Context, Result};

#[derive(Debug, Clone)]
pub struct Tdt {
    pub version: u32,
    pub tgt: String,
    pub w: usize,
    pub h: usize,
    /// Edge types in the order of the file (assumed N, W, S, E).
    pub edges: [Option<String>; 4],
    pub grounds: [Option<String>; 4],
}

pub fn parse(b: &[u8]) -> Result<Tdt> {
    let u32_at = |o: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(b.get(o..o + 4).context("truncated tdt")?.try_into()?))
    };
    let version = u32_at(0)?;
    ensure!(version == 14 || version == 15, "unsupported tdt version {version}");
    let n = u32_at(4)? as usize;
    let raw = b.get(8..8 + n * 2).context("truncated tdt string table")?;
    let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let string_at = |off: u32| -> Option<String> {
        if off == u32::MAX {
            return None;
        }
        let rest = units.get(off as usize..)?;
        let end = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        Some(String::from_utf16_lossy(&rest[..end]))
    };
    let p = 8 + n * 2;
    let mut edges: [Option<String>; 4] = Default::default();
    let mut grounds: [Option<String>; 4] = Default::default();
    for i in 0..4 {
        edges[i] = string_at(u32_at(p + 12 + i * 4)?);
        grounds[i] = string_at(u32_at(p + 30 + i * 4)?);
    }
    Ok(Tdt {
        version,
        tgt: string_at(0).unwrap_or_default(),
        w: *b.get(p + 28).context("truncated tdt")? as usize,
        h: *b.get(p + 29).context("truncated tdt")? as usize,
        edges,
        grounds,
    })
}
