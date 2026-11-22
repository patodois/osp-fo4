//! An actor's skeleton posed by a Havok animation clip (`spec/actor.md`,
//! Clip pose).
//!
//! The clip's spline animation is decoded by `fo4_havok`; which clip an
//! actor plays is behaviour-graph logic that is not read, so the clip is
//! always named by the caller. Two pending rules cover what is drawn: the
//! Havok skeleton of an actor is the `.hkx` file beside its skeleton NIF
//! (`actor_clip_skeleton_beside_nif_v1`), and the clip's bone transforms
//! replace the skeleton's nodes of the same name in model space, other nodes
//! following their parent and bones without a track keeping the reference
//! pose (`actor_clip_pose_by_bone_name_v1`).
use super::pose::{self, Affine};
use crate::{hypotheses, State};
use fo4_core::{Budget, Error, Limits, Result};
use fo4_havok as hk;
use fo4_nif::{Data, Nif};

/// Largest spread between the three scale words of a sampled bone that is
/// still drawn with one scale (the reference poses spread by 2e-4 at most).
pub const SCALE_SPREAD: f64 = 1e-3;

/// The bones of a Havok skeleton, owned: names, parents and reference pose.
#[derive(Debug, Clone, PartialEq)]
pub struct HkSkeleton {
    pub name: Vec<u8>,
    pub bones: Vec<Vec<u8>>,
    pub parents: Vec<i16>,
    pub reference: Vec<hk::TrackPose>,
}
impl HkSkeleton {
    /// The first `hkaSkeleton` object of a `.hkx` file.
    pub fn read(bytes: &[u8], file: &str, b: &Budget) -> Result<Self> {
        let c = hk::Container::read(bytes, file, 0, b)?;
        let i = c
            .objects
            .iter()
            .position(|o| o.class == hk::SKELETON)
            .ok_or_else(|| Error::new("ActorClipSkeleton", file, 0, hk::SKELETON, "none"))?;
        let s = hk::Skeleton::read(&c, i, file, 0, b)?;
        let mut out = Self {
            name: s.name.to_vec(),
            bones: Vec::new(),
            parents: s.parents.clone(),
            reference: Vec::new(),
        };
        b.reserve(&mut out.bones, s.bones.len(), file, 0)?;
        b.reserve(&mut out.reference, s.pose.len(), file, 0)?;
        for (name, p) in s.bones.iter().zip(&s.pose) {
            b.charge(name.len() as u64, file, 0)?;
            out.bones.push(name.to_vec());
            out.reference.push(hk::TrackPose {
                translation: p.translation.map(f64::from),
                rotation: p.rotation.map(f64::from),
                scale: p.scale.map(f64::from),
            });
        }
        Ok(out)
    }
}

/// A clip: the binding's skeleton name and track bones, and its decoded
/// spline animation.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub skeleton_name: Vec<u8>,
    pub tracks: Vec<i16>,
    pub animation: hk::DecodedSpline,
}
impl Clip {
    /// The one binding of a `.hkx` file and the spline animation it links.
    pub fn read(bytes: &[u8], file: &str, b: &Budget) -> Result<Self> {
        let c = hk::Container::read(bytes, file, 0, b)?;
        let bindings: Vec<usize> = c
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.class == hk::BINDING)
            .map(|(i, _)| i)
            .collect();
        let [i] = bindings[..] else {
            return Err(Error::new(
                "ActorClipBinding",
                file,
                0,
                "one hkaAnimationBinding",
                bindings.len(),
            ));
        };
        let binding = hk::Binding::read(&c, i, file, 0, b)?;
        // Byte 80 is 2 in 98 of the 102 clips whose file name says
        // "additive", and 1 or 2 in 951 others; what it selects is not
        // established, so only clips that store 0 there are posed.
        if binding.byte80 != 0 {
            return Err(Error::new(
                "ActorClipBindingByte80",
                file,
                0,
                "0 at binding byte 80 (other values' meaning is open)",
                binding.byte80,
            ));
        }
        let class = c.objects.get(binding.animation).map_or("none", |o| o.class);
        if class != hk::SPLINE {
            return Err(Error::new(
                "ActorClipCompression",
                file,
                0,
                format!("{} (the decoded class)", hk::SPLINE),
                class,
            ));
        }
        let spline = hk::SplineAnimation::read(&c, binding.animation, file, 0, b)?;
        Ok(Self {
            skeleton_name: binding.skeleton_name.to_vec(),
            tracks: binding.transform_tracks.clone(),
            animation: spline.decode(file, b)?,
        })
    }
    pub fn duration(&self) -> f64 {
        self.animation.duration
    }
    /// The binding fits `s`: same name, every track a distinct bone of it.
    pub fn fits(&self, s: &HkSkeleton) -> bool {
        let mut seen = vec![false; s.bones.len()];
        self.skeleton_name == s.name
            && self.tracks.iter().all(|t| {
                usize::try_from(*t)
                    .ok()
                    .and_then(|i| seen.get_mut(i))
                    .is_some_and(|x| !std::mem::replace(x, true))
            })
    }
    /// Local transform of every bone of `s` at clip time `t`: the bone's
    /// track, or its reference pose when no track drives it. `s` must fit.
    pub fn locals(&self, s: &HkSkeleton, t: f64) -> Vec<hk::TrackPose> {
        let mut out = s.reference.clone();
        for (pose, bone) in self.animation.sample(t).into_iter().zip(&self.tracks) {
            if let Some(slot) = usize::try_from(*bone).ok().and_then(|i| out.get_mut(i)) {
                *slot = pose;
            }
        }
        out
    }
}

/// A bone's local transform as an [`Affine`]: rows of the quaternion
/// (x, y, z, w) acting on column vectors (`spec/havok-animation.md`), and
/// one scale, the first word, when the three agree within [`SCALE_SPREAD`].
pub fn affine(p: &hk::TrackPose, file: &str) -> Result<Affine> {
    let [x, y, z, w] = p.rotation;
    let s = p.scale;
    if (s[0] - s[1]).abs() > SCALE_SPREAD || (s[0] - s[2]).abs() > SCALE_SPREAD {
        return Err(Error::new(
            "ActorClipScale",
            file,
            0,
            format!("three scale words within {SCALE_SPREAD}"),
            format!("{s:?}"),
        ));
    }
    Ok(Affine {
        rows: [
            [
                1. - 2. * (y * y + z * z),
                2. * (x * y - w * z),
                2. * (x * z + w * y),
            ],
            [
                2. * (x * y + w * z),
                1. - 2. * (x * x + z * z),
                2. * (y * z - w * x),
            ],
            [
                2. * (x * z - w * y),
                2. * (y * z + w * x),
                1. - 2. * (x * x + y * y),
            ],
        ],
        translation: p.translation,
        scale: s[0],
    })
}

/// Model-space transform of every bone from its local transform, parents
/// first (the skeleton reader enforces that parents precede children).
pub fn worlds(s: &HkSkeleton, locals: &[hk::TrackPose], file: &str) -> Result<Vec<Affine>> {
    let mut out: Vec<Affine> = Vec::with_capacity(locals.len());
    for (i, p) in locals.iter().enumerate() {
        let local = affine(p, file)?;
        let w = match s
            .parents
            .get(i)
            .and_then(|p| usize::try_from(*p).ok())
            .and_then(|k| out.get(k))
        {
            Some(parent) => parent.compose(&local),
            None => local,
        };
        out.push(w);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
struct RigNode {
    block: usize,
    name: Option<Vec<u8>>,
    parent: Option<usize>,
    local: Affine,
    bone: Option<usize>,
}

/// The nodes of a skeleton NIF in parent-first order, with their local
/// transforms and the Havok bone of the same name, if any.
#[derive(Debug, Clone)]
pub struct Rig {
    nodes: Vec<RigNode>,
}
impl Rig {
    pub fn new(n: &Nif<'_>, file: &str, s: &HkSkeleton, l: &Limits, b: &Budget) -> Result<Self> {
        let mut nodes: Vec<RigNode> = Vec::new();
        let mut stack: Vec<(usize, Option<usize>, u64)> = Vec::new();
        b.reserve(&mut stack, n.roots.len(), file, 0)?;
        for r in n.roots.iter().filter(|r| **r >= 0) {
            stack.push((*r as usize, None, 0));
        }
        let mut seen = vec![false; n.blocks.len()];
        while let Some((i, parent, depth)) = stack.pop() {
            fo4_core::limit(depth, l.max_depth, file, 0)?;
            fo4_core::limit(nodes.len() as u64, l.max_entries, file, 0)?;
            let Some(Data::Node { fields, children }) =
                n.blocks.get(i).and_then(|x| x.decoded.as_ref())
            else {
                continue;
            };
            if std::mem::replace(&mut seen[i], true) {
                return Err(Error::new(
                    "ActorNodeCycle",
                    file,
                    n.blocks[i].data.start as u64,
                    "tree",
                    i,
                ));
            }
            let name = pose::name(n, i).map(<[u8]>::to_vec);
            let bone = name
                .as_ref()
                .and_then(|x| s.bones.iter().position(|b| b == x));
            b.reserve(&mut nodes, 1, file, 0)?;
            nodes.push(RigNode {
                block: i,
                name,
                parent,
                local: Affine::from_fields(fields, file)?.unwrap_or(Affine::IDENTITY),
                bone,
            });
            let me = nodes.len() - 1;
            for c in children.iter().filter(|c| **c >= 0) {
                b.reserve(&mut stack, 1, file, 0)?;
                stack.push((*c as usize, Some(me), depth + 1));
            }
        }
        Ok(Self { nodes })
    }
    /// Named nodes that are bones of the Havok skeleton.
    pub fn matched(&self) -> usize {
        self.nodes.iter().filter(|x| x.bone.is_some()).count()
    }
    /// The model-space pose: a node named like a Havok bone takes the bone's
    /// transform in `bones` (from [`worlds`]); any other node its parent's
    /// pose after its own local transform. Named as `pose::Skeleton::read`
    /// names a NIF's nodes (the first node of a name in block order).
    pub fn pose(&self, bones: &[Affine]) -> pose::Skeleton {
        let mut worlds: Vec<Affine> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let w = match node.bone.and_then(|k| bones.get(k)) {
                Some(w) => *w,
                None => match node.parent.and_then(|p| worlds.get(p)) {
                    Some(parent) => parent.compose(&node.local),
                    None => node.local,
                },
            };
            worlds.push(w);
        }
        let mut order: Vec<usize> = (0..self.nodes.len()).collect();
        order.sort_by_key(|k| self.nodes[*k].block);
        let mut out = pose::Skeleton::default();
        for k in order {
            let Some(name) = &self.nodes[k].name else {
                continue;
            };
            if out.bones.contains_key(name) {
                out.repeated_names += 1;
                continue;
            }
            out.bones.insert(name.clone(), worlds[k]);
        }
        out
    }
}

/// The rule state of drawing a clip pose: both clip rules, pending.
pub fn state(b: &Budget) -> Result<State> {
    let errors = [
        Error::new(
            "UnverifiedActorClipSkeleton",
            "actor clip",
            0,
            hypotheses::ACTOR_CLIP_SKELETON,
            ".hkx beside the skeleton NIF",
        ),
        Error::new(
            "UnverifiedActorClipPose",
            "actor clip",
            0,
            hypotheses::ACTOR_CLIP_POSE,
            "bone transforms replace nodes of the same name",
        ),
    ];
    hypotheses::classify(&errors, false, b)
}

/// The archive path of the Havok skeleton beside a skeleton NIF: the same
/// path with `.hkx` for `.nif` (`actor_clip_skeleton_beside_nif_v1`).
pub fn skeleton_hkx_path(nif: &[u8]) -> Option<Vec<u8>> {
    let lower = nif.to_ascii_lowercase();
    let stem = lower.strip_suffix(b".nif")?;
    let mut out = stem.to_vec();
    out.extend_from_slice(b".hkx");
    Some(out)
}

#[cfg(test)]
mod tests;
