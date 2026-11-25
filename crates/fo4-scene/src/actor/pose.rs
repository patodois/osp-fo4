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
//! reads the words the same way (`spec/nif.md`, Node rotation words).
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
    node_worlds_with(n, file, &BTreeMap::new(), l, b)
}

/// [`node_worlds`] with the local translation of each node block in
/// `translations` replaced by the given one (a moving part's place in a
/// sequence, `crate::timeline::ride`).
pub fn node_worlds_with(
    n: &Nif<'_>,
    file: &str,
    translations: &BTreeMap<usize, [f64; 3]>,
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
        let world = match (Affine::from_fields(fields, file)?, translations.get(&i)) {
            (Some(local), Some(t)) => parent.compose(&Affine {
                translation: *t,
                ..local
            }),
            (Some(local), None) => parent.compose(&local),
            (None, _) => parent,
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
#[derive(Debug, Default, Clone)]
pub struct Skeleton {
    pub bones: BTreeMap<Vec<u8>, Affine>,
    /// Names held by more than one node; the first in block order is kept.
    pub repeated_names: u64,
}
impl Skeleton {
    pub fn read(n: &Nif<'_>, file: &str, l: &Limits, b: &Budget) -> Result<Self> {
        Self::of_worlds(n, node_worlds(n, file, l, b)?, file, b)
    }
    /// The named nodes of `worlds` (from [`node_worlds`] or
    /// [`node_worlds_with`]); the first of a repeated name is kept.
    pub fn of_worlds(
        n: &Nif<'_>,
        worlds: BTreeMap<usize, Affine>,
        file: &str,
        b: &Budget,
    ) -> Result<Self> {
        let mut s = Self::default();
        for (i, world) in worlds {
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
    Ok(bone_matrices_bound(n, skin, pose, file, false, b)?.0)
}

/// [`bone_matrices`]; with `missing_at_bind` a bone the skeleton lacks is
/// skinned at its bind transform (the identity: its vertices keep their
/// stored positions, `actor_missing_bone_bind_pose_v1`) instead of failing
/// the shape. Returns the matrices and how many bones were missing.
pub fn bone_matrices_bound(
    n: &Nif<'_>,
    skin: &Skin,
    pose: &Skeleton,
    file: &str,
    missing_at_bind: bool,
    b: &Budget,
) -> Result<(Vec<Affine>, usize)> {
    let mode = if missing_at_bind {
        MissingBones::Identity
    } else {
        MissingBones::Fail
    };
    bone_matrices_missing(n, skin, pose, file, mode, b)
}

/// What a skinned shape's bone the skeleton lacks is skinned with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingBones {
    /// The shape fails (`ActorSkinBoneMissing`).
    #[default]
    Fail,
    /// The identity (`actor_missing_bone_bind_pose_v1`).
    Identity,
    /// The composite matrix of the bone of the same skin the skeleton holds
    /// whose bind origin (the skin-to-bone transform's inverse applied to
    /// the bone's origin) lies nearest the missing bone's: the missing bone
    /// kept at its bind place relative to that bone
    /// (`actor_missing_bone_follows_nearest_v1`); the identity when the
    /// skeleton holds none of the skin's bones.
    Nearest,
}

/// Where a bone's origin lies in skin space at bind: the stored
/// skin-to-bone transform (`bone = scale * rows * skin + translation`)
/// solved for the bone's origin, the rows read as a rotation (transposed).
fn bind_origin(to_bone: &Affine) -> [f64; 3] {
    let t = to_bone.translation;
    std::array::from_fn(|j| -(0..3).map(|i| to_bone.rows[i][j] * t[i]).sum::<f64>() / to_bone.scale)
}

/// [`bone_matrices_bound`] with the reading of a missing bone `mode` names.
pub fn bone_matrices_missing(
    n: &Nif<'_>,
    skin: &Skin,
    pose: &Skeleton,
    file: &str,
    mode: MissingBones,
    b: &Budget,
) -> Result<(Vec<Affine>, usize)> {
    let (out, missing, gaps) =
        bone_matrices_gaps(n, skin, pose, file, mode != MissingBones::Fail, b)?;
    if mode != MissingBones::Nearest || gaps.is_empty() {
        return Ok((out, missing));
    }
    let origins: Vec<[f64; 3]> = skin
        .bones
        .iter()
        .map(|bone| {
            bind_origin(&Affine {
                rows: bone.rotation.map(|r| r.map(f64::from)),
                translation: bone.translation.map(f64::from),
                scale: f64::from(bone.scale),
            })
        })
        .collect();
    let mut out = out;
    for k in &gaps {
        let near = (0..skin.bones.len())
            .filter(|j| !gaps.contains(j))
            .min_by(|x, y| {
                let d = |j: usize| -> f64 {
                    (0..3)
                        .map(|i| (origins[j][i] - origins[*k][i]).powi(2))
                        .sum::<f64>()
                };
                d(*x).total_cmp(&d(*y)).then(x.cmp(y))
            });
        if let Some(j) = near {
            out[*k] = out[j];
        }
    }
    Ok((out, missing))
}

/// The composite matrices, how many bones were missing and their indexes
/// (each given the identity), or `ActorSkinBoneMissing` without
/// `missing_at_bind`.
fn bone_matrices_gaps(
    n: &Nif<'_>,
    skin: &Skin,
    pose: &Skeleton,
    file: &str,
    missing_at_bind: bool,
    b: &Budget,
) -> Result<(Vec<Affine>, usize, Vec<usize>)> {
    let mut missing = 0;
    let mut gaps = Vec::new();
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
        let Some(world) = pose.bones.get(bone_name) else {
            if missing_at_bind {
                missing += 1;
                b.reserve(&mut gaps, 1, file, 0)?;
                gaps.push(out.len());
                out.push(Affine::IDENTITY);
                continue;
            }
            return Err(Error::new(
                "ActorSkinBoneMissing",
                file,
                n.blocks[bone.node].data.start as u64,
                "bone named in the skeleton",
                String::from_utf8_lossy(bone_name),
            ));
        };
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
    Ok((out, missing, gaps))
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

/// A skinned shape kept to be posed again: each bone's name and stored
/// skin-to-bone transform, and each vertex's stored position with its four
/// weights and bone indices.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinnedShape {
    pub bones: Vec<(Vec<u8>, Affine)>,
    pub vertices: Vec<SkinnedVertex>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinnedVertex {
    pub position: [f64; 3],
    pub weights: [f32; 4],
    pub bones: [u8; 4],
}
impl SkinnedShape {
    /// Reads shape `shape` of `n` with its skin, checked as
    /// [`bone_matrices`] and [`skin_positions`] check them.
    pub fn read(n: &Nif<'_>, shape: usize, skin: &Skin, file: &str, b: &Budget) -> Result<Self> {
        let mut out = Self {
            bones: Vec::new(),
            vertices: Vec::new(),
        };
        b.reserve(&mut out.bones, skin.bones.len(), file, 0)?;
        for bone in &skin.bones {
            let at = n.blocks.get(bone.node).map_or(0, |x| x.data.start as u64);
            let bone_name = name(n, bone.node).ok_or_else(|| {
                Error::new("ActorSkinBoneName", file, at, "named node", bone.node)
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
                    at,
                    "finite",
                    "bone data",
                ));
            }
            b.charge(bone_name.len() as u64 + 128, file, at)?;
            out.bones.push((bone_name.to_vec(), to_bone));
        }
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
        b.reserve(&mut out.vertices, vertices.len(), file, at)?;
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
                if usize::from(bone) >= out.bones.len() {
                    return Err(Error::new(
                        "ActorSkinBoneIndex",
                        file,
                        at,
                        format!("< {}", out.bones.len()),
                        bone,
                    ));
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
            out.vertices.push(SkinnedVertex {
                position: p.map(f64::from),
                weights: w.weights,
                bones: w.bones,
            });
        }
        Ok(out)
    }
    /// The composite transform of each bone in `pose`, as [`bone_matrices`].
    pub fn matrices(&self, pose: &Skeleton, file: &str, out: &mut Vec<Affine>) -> Result<()> {
        out.clear();
        for (bone_name, to_bone) in &self.bones {
            let world = pose.bones.get(bone_name).ok_or_else(|| {
                Error::new(
                    "ActorSkinBoneMissing",
                    file,
                    0,
                    "bone named in the skeleton",
                    String::from_utf8_lossy(bone_name),
                )
            })?;
            out.push(world.compose(to_bone));
        }
        Ok(())
    }
    /// Linearly blended positions under `matrices`, with the arithmetic of
    /// [`skin_positions`].
    pub fn positions(&self, matrices: &[Affine], out: &mut Vec<[f64; 3]>) {
        out.clear();
        for v in &self.vertices {
            let mut acc = [0.; 3];
            for (weight, bone) in v.weights.iter().zip(v.bones) {
                if *weight == 0. {
                    continue;
                }
                let Some(m) = matrices.get(usize::from(bone)) else {
                    continue;
                };
                let x = m.apply(v.position);
                for a in 0..3 {
                    acc[a] += f64::from(*weight) * x[a];
                }
            }
            out.push(acc);
        }
    }
}
