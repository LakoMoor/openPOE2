//! Reading of Oodle-compressed `.bundle.bin` containers.
//!
//! Layout: 12-byte header, a head payload describing the blocks, then the
//! compressed blocks back to back. Every block decompresses to `granularity`
//! bytes except the last one.

use anyhow::{bail, ensure, Context, Result};
use oozextract::Extractor;


fn u32_at(b: &[u8], off: usize) -> Result<u32> {
    let s = b.get(off..off + 4).context("truncated bundle header")?;
    Ok(u32::from_le_bytes(s.try_into()?))
}

fn u64_at(b: &[u8], off: usize) -> Result<u64> {
    let s = b.get(off..off + 8).context("truncated bundle header")?;
    Ok(u64::from_le_bytes(s.try_into()?))
}

#[derive(Debug, Clone)]
pub struct BundleHeader {
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub encoder: u32,
    pub granularity: u32,
    pub block_sizes: Vec<u32>,
    /// Offset of the first compressed block in the file.
    pub data_offset: usize,
}

impl BundleHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let head_size = u32_at(data, 8)? as usize;
        let h = 12;
        ensure!(data.len() >= h + head_size, "bundle shorter than its head payload");
        let encoder = u32_at(data, h)?;
        let uncompressed_size = u64_at(data, h + 8)?;
        let compressed_size = u64_at(data, h + 16)?;
        let block_count = u32_at(data, h + 24)? as usize;
        let granularity = u32_at(data, h + 28)?;
        ensure!(granularity > 0, "zero block granularity");
        ensure!(
            head_size >= 48 + block_count * 4,
            "head payload too small for {block_count} blocks"
        );
        let block_sizes = (0..block_count)
            .map(|i| u32_at(data, h + 48 + i * 4))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            uncompressed_size,
            compressed_size,
            encoder,
            granularity,
            block_sizes,
            data_offset: h + head_size,
        })
    }
}

/// Decompress an entire bundle held in memory.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let hdr = BundleHeader::parse(data)?;
    decompress_with(data, &hdr, 0, hdr.block_sizes.len())
}

/// Decompress blocks `first..first + count`. The result begins at the start of
/// block `first`.
pub fn decompress_with(
    data: &[u8],
    hdr: &BundleHeader,
    first: usize,
    count: usize,
) -> Result<Vec<u8>> {
    ensure!(first + count <= hdr.block_sizes.len(), "block range out of bounds");
    let gran = hdr.granularity as usize;
    let total = hdr.uncompressed_size as usize;

    let mut src = hdr.data_offset
        + hdr.block_sizes[..first].iter().map(|&s| s as usize).sum::<usize>();
    let mut out = Vec::with_capacity(count * gran);
    let mut ex = Extractor::new();
    let mut buf = vec![0u8; gran];

    for i in first..first + count {
        let csize = hdr.block_sizes[i] as usize;
        let block = data
            .get(src..src + csize)
            .with_context(|| format!("block {i} runs past end of file"))?;
        let want = gran.min(total - i * gran);
        let got = ex
            .read_from_slice(block, &mut buf[..want])
            .map_err(|e| anyhow::anyhow!("oodle block {i}: {e:?}"))?;
        if got < want {
            bail!("block {i}: decoded {got} bytes, expected {want}");
        }
        out.extend_from_slice(&buf[..want]);
        src += csize;
    }
    Ok(out)
}
