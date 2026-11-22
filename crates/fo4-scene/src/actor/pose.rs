//! Rest pose of a skeleton and skinning of a shape in the actor's model
//! space (`spec/actor.md`, Rest pose).
//!
//! Node rotations here act on column vectors: a node maps a child point p
//! to `t + s * R * p`, R being the nine stored words as three rows. This was
//! measured on the skinned models of one installation: with it the models'
//! own bone nodes equal the world transforms composed from their skeletons
//! (94.5 % within 0.01 units; 3 % with the transposed reading), and each
//! bone's stored skin-to-bone transform is the inverse of its node's world
//! transform up to one translation per shape. `crate::transform::Transform`
//! keeps the static renderer's own reading and is not used here.
use fo4_core::{Budget, Error, Limits, Result};
use fo4_nif::{skin::Skin, Data, Nif, Value};
use std::collections::BTreeMap;

/// `x -> translation + scale * rows * x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub rows: [[f64; 3]; 3],
    pub translation: [f64; 3],
    pub scale: f64,
}
impl Affine {
    pub const IDENTITY: Self = Self {
        rows: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        translation: [0.; 3],
        scale: 1.,
    };
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            self.translation[i] + self.scale * (0..3).map(|j| self.rows[i][j] * p[j]).sum::<f64>()
        })
    }
    /// `self` after `local`: the world transform of a child whose local
    /// transform is `local`.
    pub fn compose(&self, local: &Self) -> Self {
        Self {
            rows: std::array::from_fn(|i| {
                std::array::from_fn(|j| (0..3).map(|k| self.rows[i][k] * local.rows[k][j]).sum())
            }),
            translation: self.apply(local.translation),
            scale: self.scale * local.scale,
        }
    }
    /// A node's local transform from its decoded Translation, Rotation and
    /// Scale fields; non-finite words are refused.
    pub fn from_fields(fields: &[fo4_nif::Field], file: &str) -> Result<Option<Self>> {
        let mut words = Vec::with_capacity(13);
        for name in ["Translation", "Rotation", "Scale"] {
            for f in fields.iter().filter(|f| f.name == name) {
                let Value::FloatBits(bits) = f.value else {
                    return Err(Error::new(
                        "ActorNodeTransform",
                        file,
                        f.offset,
                        "float",
                        name,
                    ));
                };
                let v = f32::from_bits(bits);
                if !v.is_finite() {
                    return Err(Error::new(
                        "ActorNodeTransform",
                        file,
                        f.offset,
                        "finite",
                        bits,
                    ));
                }
                words.push(f64::from(v));
            }
        }
        if words.is_empty() {
            return Ok(None);
        }
        if words.len() != 13 {
            return Err(Error::new("ActorNodeTransform", file, 0, 13, words.len()));
        }
        Ok(Some(Self {
            translation: [words[0], words[1], words[2]],
            rows: std::array::from_fn(|i| std::array::from_fn(|j| words[3 + 3 * i + j])),
            scale: words[12],
        }))
    }
}

/// The name of block `i`, when it has a Name string.
pub fn name<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [u8]> {
    let data = n.blocks.get(i)?.decoded.as_ref()?;
    crate::model_policy::fields(data)
        .iter()
        .find_map(|f| match f.value {
            Value::StringIndex(s) if f.name == "Name" => {
                n.strings.get(s as usize).map(Vec::as_slice)
            }
            _ => None,
        })
}

/// World transforms of the nodes of a file, composed from its roots.
pub fn node_worlds(
    n: &Nif<'_>,
    file: &str,
    l: &Limits,
    b: &Budget,
) -> Result<BTreeMap<usize, Affine>> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<(usize, Affine, u64)> = Vec::new();
    b.reserve(&mut stack, n.roots.len(), file, 0)?;
    for r in n.roots.iter().filter(|r| **r >= 0) {
        stack.push((*r as usize, Affine::IDENTITY, 0));
    }
    let mut visits = 0u64;
    while let Some((i, parent, depth)) = stack.pop() {
        fo4_core::limit(depth, l.max_depth, file, 0)?;
        visits = fo4_core::add(visits, 1, file, 0)?;
        fo4_core::limit(visits, l.max_entries, file, 0)?;
        let Some(Data::Node { fields, children }) =
            n.blocks.get(i).and_then(|x| x.decoded.as_ref())
        else {
            continue;
        };
        if out.contains_key(&i) {
            return Err(Error::new(
                "ActorNodeCycle",
                file,
                n.blocks[i].data.start as u64,
                "tree",
                i,
            ));
        }
        let world = match Affine::from_fields(fields, file)? {
            Some(local) => parent.compose(&local),
            None => parent,
        };
        b.charge(96, file, 0)?;
        out.insert(i, world);
        for c in children.iter().filter(|c| **c >= 0) {
            b.reserve(&mut stack, 1, file, 0)?;
            stack.push((*c as usize, world, depth + 1));
        }
    }
    Ok(out)
}

/// The rest pose of a skeleton file: the world transform of each named node.
#[derive(Debug, Default)]
pub struct Skeleton {
    pub bones: BTreeMap<Vec<u8>, Affine>,
    /// Names held by more than one node; the first in block order is kept.
    pub repeated_names: u64,
}
impl Skeleton {
    pub fn read(n: &Nif<'_>, file: &str, l: &Limits, b: &Budget) -> Result<Self> {
        let mut s = Self::default();
        for (i, world) in node_worlds(n, file, l, b)? {
            let Some(name) = name(n, i) else { continue };
            if s.bones.contains_key(name) {
                s.repeated_names += 1;
                continue;
            }
            b.charge(name.len() as u64 + 128, file, 0)?;
            s.bones.insert(name.to_vec(), world);
        }
        Ok(s)
    }
}

/// The composite skinning transform of each bone of `skin`: the bone's
/// node world transform in `pose` after the stored skin-to-bone transform.
pub fn bone_matrices(
    n: &Nif<'_>,
    skin: &Skin,
    pose: &Skeleton,
    file: &str,
    b: &Budget,
) -> Result<Vec<Affine>> {
    let mut out = Vec::new();
    b.reserve(&mut out, skin.bones.len(), file, 0)?;
    for bone in &skin.bones {
        let bone_name = name(n, bone.node).ok_or_else(|| {
            Error::new(
                "ActorSkinBoneName",
                file,
                n.blocks[bone.node].data.start as u64,
                "named node",
                bone.node,
            )
        })?;
        let world = pose.bones.get(bone_name).ok_or_else(|| {
            Error::new(
                "ActorSkinBoneMissing",
                file,
                n.blocks[bone.node].data.start as u64,
                "bone named in the skeleton",
                String::from_utf8_lossy(bone_name),
            )
        })?;
        let to_bone = Affine {
            rows: bone.rotation.map(|r| r.map(f64::from)),
            translation: bone.translation.map(f64::from),
            scale: f64::from(bone.scale),
        };
        if !to_bone
            .rows
            .iter()
            .flatten()
            .chain(&to_bone.translation)
            .all(|v| v.is_finite())
            || !to_bone.scale.is_finite()
        {
            return Err(Error::new(
                "ActorSkinNonFinite",
                file,
                n.blocks[skin.bone_data].data.start as u64,
                "finite",
                "bone data",
            ));
        }
        out.push(world.compose(&to_bone));
    }
    Ok(out)
}

/// Linearly blended positions of shape `shape` under `matrices`, in the
/// order of its vertices. A vertex whose four weights are all zero has no
/// position under the blend and fails the shape.
pub fn skin_positions(
    n: &Nif<'_>,
    shape: usize,
    matrices: &[Affine],
    file: &str,
    b: &Budget,
) -> Result<Vec<[f64; 3]>> {
    let Some(Data::Mesh(mesh)) = n.blocks.get(shape).and_then(|s| s.decoded.as_ref()) else {
        return Err(Error::new(
            "ActorSkinShape",
            file,
            0,
            "decoded shape",
            shape,
        ));
    };
    let at = n.blocks[shape].data.start as u64;
    let vertices = mesh
        .vertices
        .as_ref()
        .ok_or_else(|| Error::new("ActorSkinVertices", file, at, "vertices", "absent"))?;
    let mut out = Vec::new();
    b.reserve(&mut out, vertices.len(), file, at)?;
    for (k, v) in vertices.iter().enumerate() {
        let p = v
            .position
            .ok_or_else(|| Error::new("ActorSkinVertices", file, at, "position", k))?;
        if p.iter().any(|x| !x.is_finite()) {
            return Err(Error::new(
                "ActorSkinNonFinite",
                file,
                at,
                "finite position",
                k,
            ));
        }
        let w = fo4_nif::skin::vertex_weights(n.bytes, v)
            .ok_or_else(|| Error::new("ActorSkinWeights", file, at, "12 skin bytes", k))?;
        let p = p.map(f64::from);
        let mut acc = [0.; 3];
        let mut total = 0.;
        for (weight, bone) in w.weights.iter().zip(w.bones) {
            if *weight == 0. {
                continue;
            }
            if !weight.is_finite() {
                return Err(Error::new(
                    "ActorSkinNonFinite",
                    file,
                    at,
                    "finite weight",
                    k,
                ));
            }
            let m = matrices.get(bone as usize).ok_or_else(|| {
                Error::new(
                    "ActorSkinBoneIndex",
                    file,
                    at,
                    format!("< {}", matrices.len()),
                    bone,
                )
            })?;
            let x = m.apply(p);
            for a in 0..3 {
                acc[a] += f64::from(*weight) * x[a];
            }
            total += f64::from(*weight);
        }
        if total == 0. {
            return Err(Error::new(
                "ActorSkinZeroWeights",
                file,
                at,
                "a non-zero weight",
                k,
            ));
        }
        out.push(acc);
    }
    Ok(out)
}

/// How the bind pose a model carries (its own bone nodes) fits the stored
/// skin: the largest rotation error of the composite transforms, the spread
/// of their translations, and the first composite's translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BindFit {
    pub rotation_error: f64,
    pub translation_spread: f64,
    pub translation: [f64; 3],
}
pub fn bind_fit(n: &Nif<'_>, skin: &Skin, file: &str, l: &Limits, b: &Budget) -> Result<BindFit> {
    let worlds = node_worlds(n, file, l, b)?;
    let mut own = Skeleton::default();
    for bone in &skin.bones {
        if let (Some(name), Some(w)) = (name(n, bone.node), worlds.get(&bone.node)) {
            own.bones.insert(name.to_vec(), *w);
        }
    }
    let matrices = bone_matrices(n, skin, &own, file, b)?;
    let mut fit = BindFit {
        rotation_error: 0.,
        translation_spread: 0.,
        translation: matrices.first().map_or([0.; 3], |m| m.translation),
    };
    for m in &matrices {
        for i in 0..3 {
            for j in 0..3 {
                let target = if i == j { 1. } else { 0. };
                fit.rotation_error = fit
                    .rotation_error
                    .max((m.scale * m.rows[i][j] - target).abs());
            }
            fit.translation_spread = fit
                .translation_spread
                .max((m.translation[i] - fit.translation[i]).abs());
        }
    }
    Ok(fit)
}
