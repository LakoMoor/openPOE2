//! Path hashing used by the bundle index: MurmurHash64A over the lower-cased
//! path, seed `0x1337b33f`.

const SEED: u64 = 0x1337_b33f;
const M: u64 = 0xc6a4_a793_5bd1_e995;
const R: u32 = 47;

pub fn murmur64a(data: &[u8], seed: u64) -> u64 {
    let mut h = seed ^ (data.len() as u64).wrapping_mul(M);
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        let mut k = u64::from_le_bytes(c.try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        for (i, &b) in tail.iter().enumerate() {
            h ^= (b as u64) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

pub fn path_hash(path: &str) -> u64 {
    murmur64a(path.to_ascii_lowercase().as_bytes(), SEED)
}
