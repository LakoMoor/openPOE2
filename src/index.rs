//! Parser for `Bundles2/_.index.bin`: bundle list, file table and path table.

use std::collections::HashMap;

use std::path::Path;

use anyhow::{ensure, Context, Result};

use crate::bundle;

#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub name: String,
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct FileRecord {
    pub hash: u64,
    pub bundle: u32,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Copy)]
struct PathRep {
    hash: u64,
    offset: u32,
    size: u32,
    recursive_size: u32,
}

pub struct Index {
    pub bundles: Vec<BundleInfo>,
    pub files: Vec<FileRecord>,
    /// File path -> record. Paths are lower-case-agnostic: keys are stored as
    /// they appear in the path table.
    pub paths: HashMap<String, FileRecord>,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.d.get(self.p..self.p + n).context("index truncated")?;
        self.p += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
}

impl Index {
    /// Read one file's bytes out of its bundle, decompressing only the blocks
    /// that cover it. `dir` is the `Bundles2` directory.
    pub fn read_file(&self, dir: &Path, path: &str) -> Result<Vec<u8>> {
        let rec = self
            .paths
            .get(&path.to_ascii_lowercase())
            .or_else(|| self.paths.get(path))
            .with_context(|| format!("{path}: not in index"))?;
        let info = &self.bundles[rec.bundle as usize];
        let raw = std::fs::read(dir.join(format!("{}.bundle.bin", info.name)))
            .with_context(|| format!("reading bundle {}", info.name))?;
        let hdr = bundle::BundleHeader::parse(&raw)?;
        let gran = hdr.granularity as usize;
        let (start, len) = (rec.offset as usize, rec.size as usize);
        ensure!(len > 0, "{path}: empty file record");
        let first = start / gran;
        let last = (start + len - 1) / gran;
        let blocks = bundle::decompress_with(&raw, &hdr, first, last - first + 1)?;
        let off = start - first * gran;
        Ok(blocks
            .get(off..off + len)
            .with_context(|| format!("{path}: record exceeds bundle"))?
            .to_vec())
    }

    /// Parse the raw bytes of `_.index.bin` (itself a compressed bundle).
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let data = bundle::decompress(raw).context("decompressing index")?;
        let mut r = Reader { d: &data, p: 0 };

        let n = r.u32()? as usize;
        let mut bundles = Vec::with_capacity(n);
        for _ in 0..n {
            let len = r.u32()? as usize;
            let name = String::from_utf8(r.take(len)?.to_vec())?;
            let uncompressed_size = r.u32()?;
            bundles.push(BundleInfo { name, uncompressed_size });
        }

        let n = r.u32()? as usize;
        let mut files = Vec::with_capacity(n);
        for _ in 0..n {
            files.push(FileRecord {
                hash: r.u64()?,
                bundle: r.u32()?,
                offset: r.u32()?,
                size: r.u32()?,
            });
        }

        let n = r.u32()? as usize;
        let mut reps = Vec::with_capacity(n);
        for _ in 0..n {
            reps.push(PathRep {
                hash: r.u64()?,
                offset: r.u32()?,
                size: r.u32()?,
                recursive_size: r.u32()?,
            });
        }

        // The path table is a second compressed bundle occupying the rest.
        let path_data = bundle::decompress(&data[r.p..]).context("decompressing path table")?;
        let by_hash: HashMap<u64, FileRecord> = files.iter().map(|f| (f.hash, *f)).collect();
        let mut paths = HashMap::with_capacity(files.len());
        for rep in &reps {
            let seg = path_data
                .get(rep.offset as usize..(rep.offset + rep.size) as usize)
                .context("path rep out of range")?;
            for path in expand_path_rep(seg)? {
                let h = crate::hash::path_hash(&path);
                if let Some(rec) = by_hash.get(&h) {
                    paths.insert(path, *rec);
                }
            }
            let _ = (rep.hash, rep.recursive_size);
        }
        Ok(Self { bundles, files, paths })
    }
}

/// Expand one path-representation blob into full paths.
///
/// The blob is a sequence of u32 commands. `0` toggles between "base" and
/// "build" mode; any other value `n` refers to a NUL-terminated string that
/// follows. In build mode the string is appended to the entry `n - 1` (or to
/// nothing when `n` refers to no entry yet); in base mode it is collected.
fn expand_path_rep(seg: &[u8]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut bases: Vec<String> = Vec::new();
    let mut base_mode = false;
    let mut p = 0usize;
    while p + 4 <= seg.len() {
        let cmd = u32::from_le_bytes(seg[p..p + 4].try_into()?) as usize;
        p += 4;
        if cmd == 0 {
            base_mode = !base_mode;
            if base_mode {
                bases.clear();
            }
            continue;
        }
        let end = seg[p..].iter().position(|&b| b == 0).context("unterminated string")?;
        let s = std::str::from_utf8(&seg[p..p + end])?;
        p += end + 1;
        let full = match bases.get(cmd - 1) {
            Some(prefix) => format!("{prefix}{s}"),
            None => s.to_string(),
        };
        if base_mode {
            bases.push(full);
        } else {
            out.push(full);
        }
    }
    ensure!(p <= seg.len(), "path rep overrun");
    Ok(out)
}
