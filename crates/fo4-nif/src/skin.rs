//! Skinned shapes, established by measurement over every skinned NIF of one
//! installation (`spec/nif.md`, "Skinned shapes"). The blocks are read from
//! their raw bytes on request; the block reader leaves them raw as before.
use crate::{Data, Nif, Value, Vertex};
use fo4_core::{half_to_f32, limit, Budget, Error, Limits, Result};
use serde::Serialize;
use std::ops::Range;

/// One bone of a skinned shape: the node it follows and the transform
/// from the shape's skin space to that bone's space.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Bone {
    /// Block index of the bone's node in the same file.
    pub node: usize,
    /// Centre and radius of a sphere in the bone's space.
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub sphere: [f32; 4],
    /// Nine stored words as three rows: bone = rows * skin + translation.
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub rotation: [[f32; 3]; 3],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub translation: [f32; 3],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub scale: f32,
}
impl Bone {
    /// The bone-space point of a skin-space point (stored rows times the
    /// point, scaled, plus the translation).
    pub fn to_bone(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            f64::from(self.translation[i])
                + f64::from(self.scale)
                    * (0..3)
                        .map(|j| f64::from(self.rotation[i][j]) * p[j])
                        .sum::<f64>()
        })
    }
}
/// The skin of one shape: its `BSSkin::Instance` and `BSSkin::BoneData`.
#[derive(Debug, Serialize)]
pub struct Skin {
    pub instance: usize,
    pub bone_data: usize,
    /// First word of the instance: a node link (-1 for none).
    pub root: i32,
    pub bones: Vec<Bone>,
    /// The instance's trailing 12-byte entries, unread: count and range.
    pub unread_entries: u32,
    pub unread: Range<usize>,
}
/// Weights and bone indices of one vertex: four binary16 weights, then
/// four bytes indexing [`Skin::bones`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Weights {
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub weights: [f32; 4],
    pub bones: [u8; 4],
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn float(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(word(bytes, at))
}
fn block_type<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n str> {
    n.blocks
        .get(i)
        .map(|b| n.block_types[b.type_index as usize].as_str())
}
/// The skin of shape block `shape`, `None` when the shape has no Skin link.
pub fn shape_skin(
    n: &Nif<'_>,
    shape: usize,
    file: &str,
    l: &Limits,
    b: &Budget,
) -> Result<Option<Skin>> {
    let Some(Data::Mesh(mesh)) = n.blocks.get(shape).and_then(|s| s.decoded.as_ref()) else {
        return Err(Error::new(
            "NifSkinShape",
            file,
            0,
            "decoded shape block",
            shape,
        ));
    };
    let Some(link) = mesh.fields.iter().find_map(|f| match f.value {
        Value::Ref(r) if f.name == "Skin" => Some(r),
        _ => None,
    }) else {
        return Ok(None);
    };
    if link < 0 {
        return Ok(None);
    }
    let instance = link as usize;
    let at = n.blocks[instance].data.start as u64;
    if block_type(n, instance) != Some("BSSkin::Instance") {
        return Err(Error::new(
            "UnsupportedNifSkinInstance",
            file,
            at,
            "BSSkin::Instance",
            block_type(n, instance).unwrap_or("none"),
        ));
    }
    let raw = n.raw_block(instance).unwrap_or_default();
    let size_error =
        |expected: String| Error::new("NifSkinInstanceSize", file, at, expected, raw.len());
    if raw.len() < 16 {
        return Err(size_error("at least 16 bytes".into()));
    }
    let count = word(raw, 8) as u64;
    limit(count, l.max_entries, file, at + 8)?;
    let tail_at = 12 + 4 * count as usize;
    if raw.len() < tail_at + 4 {
        return Err(size_error(format!("16 + 4 * {count} + 12 * m bytes")));
    }
    let entries = word(raw, tail_at);
    if raw.len() as u64 != tail_at as u64 + 4 + 12 * u64::from(entries) {
        return Err(size_error(format!(
            "16 + 4 * {count} + 12 * {entries} bytes"
        )));
    }
    let link_at = |o: usize| -> Result<i32> {
        let v = word(raw, o) as i32;
        if v < -1 || v >= 0 && v as usize >= n.blocks.len() {
            return Err(Error::new(
                "InvalidNifReference",
                file,
                at + o as u64,
                format!("-1 or 0..{}", n.blocks.len()),
                v,
            ));
        }
        Ok(v)
    };
    let root = link_at(0)?;
    let data = link_at(4)?;
    if data < 0 || block_type(n, data as usize) != Some("BSSkin::BoneData") {
        return Err(Error::new(
            "UnsupportedNifSkinBoneData",
            file,
            at + 4,
            "BSSkin::BoneData",
            if data < 0 {
                "none"
            } else {
                block_type(n, data as usize).unwrap_or("none")
            },
        ));
    }
    let bone_data = data as usize;
    let bd = n.raw_block(bone_data).unwrap_or_default();
    let bd_at = n.blocks[bone_data].data.start as u64;
    if bd.len() < 4 || bd.len() as u64 != 4 + 68 * u64::from(word(bd, 0)) {
        return Err(Error::new(
            "NifSkinBoneDataSize",
            file,
            bd_at,
            "4 + 68 * n bytes",
            bd.len(),
        ));
    }
    if u64::from(word(bd, 0)) != count {
        return Err(Error::new(
            "NifSkinBoneCount",
            file,
            bd_at,
            count,
            word(bd, 0),
        ));
    }
    let mut bones = Vec::new();
    b.reserve(&mut bones, count as usize, file, at)?;
    for k in 0..count as usize {
        let node = link_at(12 + 4 * k)?;
        if node < 0 {
            return Err(Error::new(
                "NifSkinBoneNode",
                file,
                at + 12 + 4 * k as u64,
                "node link",
                node,
            ));
        }
        let o = 4 + 68 * k;
        bones.push(Bone {
            node: node as usize,
            sphere: std::array::from_fn(|i| float(bd, o + 4 * i)),
            rotation: std::array::from_fn(|i| {
                std::array::from_fn(|j| float(bd, o + 16 + 4 * (3 * i + j)))
            }),
            translation: std::array::from_fn(|i| float(bd, o + 52 + 4 * i)),
            scale: float(bd, o + 64),
        });
    }
    let start = n.blocks[instance].data.start + tail_at + 4;
    Ok(Some(Skin {
        instance,
        bone_data,
        root,
        bones,
        unread_entries: entries,
        unread: start..n.blocks[instance].data.end,
    }))
}
/// The skin bytes of `v` (vertex attribute 64) read from the file's bytes.
pub fn vertex_weights(bytes: &[u8], v: &Vertex) -> Option<Weights> {
    let raw = bytes.get(v.raw_skin.clone()?)?;
    if raw.len() != 12 {
        return None;
    }
    Some(Weights {
        weights: std::array::from_fn(|i| {
            half_to_f32(u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]))
        }),
        bones: [raw[8], raw[9], raw[10], raw[11]],
    })
}
