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

/// A compressed byte grid (`FUN_142917f30` in the client): a header byte holds
/// the mode (low 3 bits), the byte count of the size field (2 bits) and the low
/// bits of the compressed size. Mode 0 is a constant fill; modes 1 to 4 are LZ4
/// blocks holding a palette (2, 4 or 16 entries) and 1, 2 or 4 bits per cell, or
/// plain bytes (mode 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grid {
    /// Every cell has this value (0 when the grid is empty).
    Constant(u8),
    /// One value per cell.
    Cells(Vec<u8>),
}

impl Grid {
    pub fn get(&self, i: usize) -> u8 {
        match self {
            Grid::Constant(v) => *v,
            Grid::Cells(c) => c.get(i).copied().unwrap_or(0),
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.b.get(self.pos..self.pos + n).context("tdt data runs past end of file")?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
}

/// LZ4 block decompression into exactly `out_len` bytes.
fn lz4(src: &[u8], out_len: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(out_len);
    let mut i = 0;
    let byte = |i: &mut usize| -> Result<usize> {
        let v = *src.get(*i).context("lz4 input truncated")?;
        *i += 1;
        Ok(v as usize)
    };
    while i < src.len() {
        let token = byte(&mut i)?;
        let mut lit = token >> 4;
        if lit == 15 {
            loop {
                let v = byte(&mut i)?;
                lit += v;
                if v != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(src.get(i..i + lit).context("lz4 literals truncated")?);
        i += lit;
        if i >= src.len() {
            break;
        }
        let offset = byte(&mut i)? | byte(&mut i)? << 8;
        ensure!(offset != 0 && offset <= out.len(), "lz4 bad offset");
        let mut len = token & 15;
        if len == 15 {
            loop {
                let v = byte(&mut i)?;
                len += v;
                if v != 255 {
                    break;
                }
            }
        }
        for _ in 0..len + 4 {
            out.push(out[out.len() - offset]);
        }
    }
    ensure!(out.len() == out_len, "lz4 produced {} bytes, expected {out_len}", out.len());
    Ok(out)
}

/// Read a grid of `n` cells.
fn read_grid(r: &mut Reader, n: usize) -> Result<Grid> {
    let head = r.u8()?;
    let (mode, extra) = (head & 7, (head >> 3) & 3);
    let mut size = (head >> 5) as usize & 7;
    for k in 0..extra as usize {
        size |= (r.u8()? as usize) << (3 + 8 * k);
    }
    let (bits, bound) = match mode {
        0 => return Ok(Grid::Constant(size as u8)),
        1 => (1, (n + 7) / 8 + 2),
        2 => (2, (n + 3) / 4 + 4),
        3 => (4, (n + 1) / 2 + 16),
        4 => (8, n),
        m => anyhow::bail!("unknown grid compression {m}"),
    };
    let data = lz4(r.bytes(size)?, bound)?;
    if bits == 8 {
        return Ok(Grid::Cells(data));
    }
    let palette_len = 1usize << bits;
    let (palette, packed) = data.split_at(palette_len.min(data.len()));
    let cells = (0..n)
        .map(|i| {
            let bit = i * bits;
            let v = packed.get(bit / 8).copied().unwrap_or(0) >> (bit % 8) & ((1 << bits) - 1);
            palette.get(v as usize).copied().unwrap_or(0)
        })
        .collect();
    Ok(Grid::Cells(cells))
}

/// One placeable node of a tile (a sub-tile slot with its own 23 x 23 grid).
#[derive(Debug, Clone)]
pub struct Node {
    pub flags: u16,
    pub kind: u8,
    pub grid: Option<Grid>,
    pub blob: Vec<u8>,
}

/// The whole file. `grids` holds the tile-wide grids of `w * h * 529` cells.
#[derive(Debug, Clone)]
pub struct TdtFull {
    pub header: Tdt,
    pub nodes: Vec<Node>,
    pub grids: Vec<Grid>,
    /// Per sub-tile bytes (`w * h`), when present.
    pub subtile_bytes: Vec<u8>,
}

pub fn parse_full(b: &[u8]) -> Result<TdtFull> {
    let header = parse(b)?;
    let v = header.version;
    let n_chars = u32::from_le_bytes(b[4..8].try_into()?) as usize;
    let mut r = Reader { b, pos: 8 + n_chars * 2 };
    let (flags_str, _tgt, _tag) = (r.u32()?, r.u32()?, r.u32()?);
    ensure!(flags_str == 0xffff_ffff || b[8 + flags_str as usize * 2..].starts_with(&[0, 0]), "tdt with a flags byte is not supported");
    r.bytes(46)?; // edges, size, grounds, three per-corner byte arrays
    let (w, h) = (header.w, header.h);
    let f = r.u16()?;
    let hi = (f >> 8) as u8;
    r.u8()?;
    let hull = r.u8()?;
    ensure!(hull <= 4, "bad hull plane count");
    if hull >= 2 {
        r.bytes(hull as usize)?;
    }
    if hi & 2 != 0 {
        r.u8()?;
    }
    let count = r.u16()? as usize;
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let flags = r.u16()?;
        let kind = r.u8()?;
        if hi & 2 != 0 {
            r.bytes(3)?;
        }
        let grid = if flags & 4 == 0 { Some(read_grid(&mut r, 529)?) } else { None };
        let blob = if flags & 3 != 3 {
            let len = r.u16()? as usize;
            r.bytes(len)?.to_vec()
        } else {
            Vec::new()
        };
        nodes.push(Node { flags, kind, grid, blob });
    }
    let corners = (w + 1) * (h + 1);
    if f & 2 != 0 {
        r.bytes(corners * 4)?;
    }
    if f & 1 != 0 {
        r.bytes(corners)?;
    }
    r.bytes(9)?; // two string offsets and a byte
    for k in 0..8 {
        if r.u8()? & 0x80 == 0 {
            break;
        }
        ensure!(k < 7, "bad unique mesh data");
    }
    let n = w * h * 529;
    let mut grids = Vec::new();
    if f & 0x10 != 0 {
        grids.push(read_grid(&mut r, n)?);
        if v > 13 {
            for _ in 0..r.u8()? {
                let len = r.u16()? as usize;
                r.bytes(len * 2)?;
            }
        }
    }
    let subtile_bytes = if f & 0x20 != 0 { r.bytes(w * h)?.to_vec() } else { Vec::new() };
    for _ in 0..if v > 11 { 4 } else { 3 } {
        grids.push(read_grid(&mut r, n)?);
    }
    ensure!(r.pos == b.len(), "tdt parse ended at {} of {}", r.pos, b.len());
    Ok(TdtFull { header, nodes, grids, subtile_bytes })
}
