//! `.ast` skeletons: the bone hierarchy and rest pose. Animation data (which
//! sits in an embedded bundle after the bone list) is not read yet.

use anyhow::{ensure, Context, Result};
use glam::{Mat4, Vec3};

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Rest transform relative to the parent (column-vector convention).
    pub local: Mat4,
}

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub version: u8,
    pub bones: Vec<Bone>,
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
        Ok(Self { version, bones })
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
