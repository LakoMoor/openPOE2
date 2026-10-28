//! `.ast` skeletons: bone hierarchy, rest pose and animation clips. Track data (which
//! sits in an embedded bundle after the bone list) is decoded by `Skeleton::tracks`.

use anyhow::{ensure, Context, Result};
use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Rest transform relative to the parent (column-vector convention).
    pub local: Mat4,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub parent: String,
    pub framerate: u8,
    pub num_tracks: usize,
    /// Offset and length of the track data in the decompressed animation blob.
    pub location: (u32, u32),
}

/// Per-bone keyframes. Each key holds a time followed by its value.
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub bone: usize,
    pub scales: Vec<[f32; 4]>,
    pub rotations: Vec<[f32; 5]>,
    pub positions: Vec<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub version: u8,
    pub bones: Vec<Bone>,
    pub animations: Vec<Animation>,
    /// Decompressed animation blob (version 8 and newer).
    pub blob: Vec<u8>,
}

impl Skeleton {
    pub fn parse(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 8, "ast too short");
        let version = data[0];
        let count = data[1] as usize;
        let mut p = 8usize;
        let mut raw: Vec<(Option<u8>, Option<u8>, Mat4, String)> = Vec::with_capacity(count);
        for _ in 0..count {
            let link = |b: u8| (b != 255).then_some(b);
            let sibling = link(*data.get(p).context("truncated bone")?);
            let child = link(*data.get(p + 1).context("truncated bone")?);
            let mut m = [0f32; 16];
            for (i, v) in m.iter_mut().enumerate() {
                let o = p + 2 + i * 4;
                *v = f32::from_le_bytes(data.get(o..o + 4).context("truncated bone")?.try_into()?);
            }
            p += 66;
            let name_len = *data.get(p).context("truncated bone")? as usize;
            p += 1 + (version >= 8) as usize;
            let name = String::from_utf8_lossy(data.get(p..p + name_len).context("truncated bone name")?).into_owned();
            p += name_len;
            // The file stores row-vector matrices; reading the floats as columns transposes them.
            raw.push((sibling, child, Mat4::from_cols_array(&m), name));
        }
        // Parents follow from the child/sibling links.
        let mut parent = vec![None; count];
        for (i, (_, child, _, _)) in raw.iter().enumerate() {
            let mut next = child.map(usize::from);
            while let Some(c) = next {
                ensure!(c < count, "bone link out of range");
                parent[c] = Some(i);
                next = raw[c].0.map(usize::from);
            }
        }
        let bones = raw
            .into_iter()
            .zip(parent)
            .map(|((_, _, local, name), parent)| Bone { name, parent, local })
            .collect();
        // Lights are skipped; animation records follow them.
        let light_count = data[7] as usize;
        for _ in 0..light_count {
            let name_len = *data.get(p).context("truncated light")? as usize;
            p += 1 + 51 + if version >= 7 { 4 } else { 0 } + if version >= 9 { 4 } else { 0 } + name_len;
        }
        let anim_count = u16::from_le_bytes([data[3], data[4]]) as usize;
        let mut animations = Vec::new();
        for _ in 0..anim_count {
            let u8_at = |o: usize| data.get(o).copied().context("truncated animation");
            let num_tracks = u8_at(p)? as usize;
            let framerate = u8_at(p + 2)?;
            let mut q = p + 4;
            if version >= 10 {
                q += 1;
            }
            let name_len = u8_at(q)? as usize;
            q += 1;
            let parent_len = if version >= 11 {
                q += 1;
                u8_at(q - 1)? as usize
            } else {
                0
            };
            ensure!(version >= 8, "inline animation data (ast version {version}) is not supported");
            let u32_at = |o: usize| -> Result<u32> {
                Ok(u32::from_le_bytes(data.get(o..o + 4).context("truncated animation")?.try_into()?))
            };
            let location = (u32_at(q)?, u32_at(q + 4)?);
            q += 8;
            let name = String::from_utf8_lossy(data.get(q..q + name_len).context("truncated name")?).into_owned();
            q += name_len;
            let parent = String::from_utf8_lossy(data.get(q..q + parent_len).context("truncated name")?).into_owned();
            q += parent_len;
            animations.push(Animation { name, parent, framerate, num_tracks, location });
            p = q;
        }
        let blob = if version >= 8 && p < data.len() {
            crate::bundle::decompress(&data[p..]).context("decompressing animation data")?
        } else {
            Vec::new()
        };
        Ok(Self { version, bones, animations, blob })
    }

    /// Read the tracks of animation `a`.
    pub fn tracks(&self, a: &Animation) -> Result<Vec<Track>> {
        let (off, len) = (a.location.0 as usize, a.location.1 as usize);
        let d = self.blob.get(off..off + len).context("animation data out of range")?;
        let mut p = 0usize;
        let u32_at = |p: &mut usize| -> Result<u32> {
            let v = u32::from_le_bytes(d.get(*p..*p + 4).context("truncated track")?.try_into()?);
            *p += 4;
            Ok(v)
        };
        let floats = |p: &mut usize, n: usize, w: usize| -> Result<Vec<Vec<f32>>> {
            (0..n)
                .map(|_| {
                    (0..w)
                        .map(|_| {
                            let v = f32::from_le_bytes(d.get(*p..*p + 4).context("truncated keys")?.try_into()?);
                            *p += 4;
                            Ok(v)
                        })
                        .collect()
                })
                .collect()
        };
        let mut out = Vec::new();
        for _ in 0..a.num_tracks {
            p += 1;
            let bone = u32_at(&mut p)? as usize;
            let (ns, nr, np, n2, n3, n4) =
                (u32_at(&mut p)?, u32_at(&mut p)?, u32_at(&mut p)?, u32_at(&mut p)?, u32_at(&mut p)?, u32_at(&mut p)?);
            if self.version >= 10 {
                u32_at(&mut p)?;
            }
            let scales = floats(&mut p, ns as usize, 4)?;
            let rotations = floats(&mut p, nr as usize, 5)?;
            let positions = floats(&mut p, np as usize, 4)?;
            floats(&mut p, n2 as usize, 4)?;
            floats(&mut p, n3 as usize, 5)?;
            floats(&mut p, n4 as usize, 4)?;
            out.push(Track {
                bone,
                scales: scales.iter().map(|v| [v[0], v[1], v[2], v[3]]).collect(),
                rotations: rotations.iter().map(|v| [v[0], v[1], v[2], v[3], v[4]]).collect(),
                positions: positions.iter().map(|v| [v[0], v[1], v[2], v[3]]).collect(),
            });
        }
        Ok(out)
    }

    /// Local bone matrices at `frame` (in the animation's own time units).
    /// Bones without a track keep their rest transform.
    pub fn pose(&self, tracks: &[Track], frame: f32) -> Vec<Mat4> {
        let mut locals: Vec<Mat4> = self.bones.iter().map(|b| b.local).collect();
        for t in tracks {
            let Some(rest) = self.bones.get(t.bone) else { continue };
            let pos = sample(&t.positions, frame, |k| Vec3::new(k[1], k[2], k[3]), |a, b, f| a.lerp(b, f));
            let rot = sample(
                &t.rotations,
                frame,
                |k| Quat::from_xyzw(k[1], k[2], k[3], k[4]).normalize(),
                |a, b, f| a.slerp(if a.dot(b) < 0.0 { -b } else { b }, f),
            );
            let scale = sample(&t.scales, frame, |k| Vec3::new(k[1], k[2], k[3]), |a, b, f| a.lerp(b, f));
            let (rs, rr, rt) = rest.local.to_scale_rotation_translation();
            locals[t.bone] = Mat4::from_scale_rotation_translation(
                scale.unwrap_or(rs),
                rot.unwrap_or(rr),
                pos.unwrap_or(rt),
            );
        }
        locals
    }

    /// Matrices taking a rest-pose model-space point to its posed position.
    pub fn skin_matrices(&self, locals: &[Mat4]) -> Vec<Mat4> {
        let mut posed: Vec<Mat4> = Vec::with_capacity(self.bones.len());
        let mut rest: Vec<Mat4> = Vec::with_capacity(self.bones.len());
        // Parents can come after their children in the file, so resolve lazily.
        for i in 0..self.bones.len() {
            posed.push(self.chain(i, locals));
            rest.push(self.world(i));
        }
        posed.iter().zip(&rest).map(|(p, r)| *p * r.inverse()).collect()
    }

    fn chain(&self, mut i: usize, locals: &[Mat4]) -> Mat4 {
        let mut m = locals[i];
        while let Some(p) = self.bones[i].parent {
            m = locals[p] * m;
            i = p;
        }
        m
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }

    /// Rest-pose transform of a bone in model space.
    pub fn world(&self, mut i: usize) -> Mat4 {
        let mut m = self.bones[i].local;
        while let Some(p) = self.bones[i].parent {
            m = self.bones[p].local * m;
            i = p;
        }
        m
    }

    pub fn world_position(&self, i: usize) -> Vec3 {
        self.world(i).transform_point3(Vec3::ZERO)
    }
}

/// Interpolate keys of the form `[time, value...]` at `t`.
fn sample<const N: usize, T: Copy>(
    keys: &[[f32; N]],
    t: f32,
    value: impl Fn(&[f32; N]) -> T,
    mix: impl Fn(T, T, f32) -> T,
) -> Option<T> {
    let first = keys.first()?;
    let last = keys.last()?;
    if t <= first[0] || keys.len() == 1 {
        return Some(value(first));
    }
    if t >= last[0] {
        return Some(value(last));
    }
    let hi = keys.partition_point(|k| k[0] <= t);
    let (a, b) = (&keys[hi - 1], &keys[hi]);
    let span = b[0] - a[0];
    let f = if span > 0.0 { (t - a[0]) / span } else { 0.0 };
    Some(mix(value(a), value(b), f))
}
