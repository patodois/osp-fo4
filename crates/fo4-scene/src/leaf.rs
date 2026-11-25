//! A static's leaf: a model whose one controller is a free-running,
//! clamped transform controller that turns one node off the collision
//! chain from its stored pose and back, read as two poses
//! (`play_static_leaf_two_pose_v1`, [`hypotheses::STATIC_LEAF_TWO_POSE`];
//! `spec/play.md`, The checkpoint gate drawn).
//!
//! The clock reading (`spec/controller-clock.md`) would play the whole
//! track from the model's load; this reading does not run the clock. It
//! names two poses instead: the stored one and the pose of the rotation key
//! farthest from it ([`Leaf::key_time`]). Which of the two is the leaf shut
//! and which open is not in the model: the play window takes the pose that
//! lies nearer the collision marker boxes meeting the static's bounds as
//! the one that holds the way, while such a box stands. Only [`read`]
//! decides whether a model is such a leaf; [`aside`] names the blockers the
//! reading sets aside for its render readiness, behind
//! `Loader::set_render_static_leaves` (off by default), under which the
//! static is drawn as its model stores it.
use crate::clock::{self, Rotation, TransformTrack};
use crate::transform::Transform;
use crate::{hypotheses, readiness};
use fo4_core::{Budget, Error, Result};
use fo4_nif::{Data, Field, Nif, Value};

/// A model's leaf: its controller, the node it turns and the two poses.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    /// The controller block.
    pub controller: usize,
    /// The node the controller turns (its target).
    pub node: usize,
    pub track: TransformTrack,
    /// The key time of the rotation key farthest from the stored rotation
    /// (the earliest of equal ones): the leaf's other pose.
    pub key_time: f64,
    /// The angle between the stored rotation and the rotation at
    /// [`Leaf::key_time`], in radians.
    pub key_angle: f64,
}

fn fields<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [Field]> {
    match n.blocks.get(i)?.decoded.as_ref()? {
        Data::Fields(f) => Some(f),
        Data::Node { fields, .. } => Some(fields),
        Data::Mesh(m) => Some(&m.fields),
    }
}

fn type_of<'n>(n: &'n Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map_or("", String::as_str)
}

fn children(n: &Nif<'_>, i: usize) -> Vec<usize> {
    match n.blocks.get(i).and_then(|b| b.decoded.as_ref()) {
        Some(Data::Node { children, .. }) => children
            .iter()
            .filter_map(|c| usize::try_from(*c).ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether block `i` or a block below it links a collision object.
fn bears_collision(n: &Nif<'_>, i: usize) -> bool {
    let mut stack = vec![i];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(k) = stack.pop() {
        if !seen.insert(k) {
            continue;
        }
        let linked = fields(n, k).is_some_and(|f| {
            f.iter()
                .any(|x| x.name == "Collision" && matches!(x.value, Value::Ref(r) if r >= 0))
        });
        if linked {
            return true;
        }
        stack.extend(children(n, k));
    }
    false
}

/// The angle between two rotations given as rows acting on column vectors.
fn angle_between(a: &[f64; 9], b: &[f32; 9]) -> f64 {
    // trace(A^T B)
    let t: f64 = (0..9).map(|k| a[k] * f64::from(b[k])).sum();
    ((t - 1.) / 2.).clamp(-1., 1.).acos()
}

/// The key times of a track's rotation keys, in order, each once.
fn rotation_key_times(r: &Rotation) -> Vec<f64> {
    let mut out: Vec<f64> = match r {
        Rotation::Quaternion { keys, .. } => keys.iter().map(|k| f64::from(k.0)).collect(),
        Rotation::Xyz(axes) => axes
            .iter()
            .flat_map(|k| k.keys.iter().map(|x| f64::from(x.time)))
            .collect(),
    };
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// Whether `n` is a leaf model: exactly one block whose type names a
/// controller, and it a transform controller the clock can run
/// ([`clock::transform_track`]) with the clamped cycle type, keys the clock
/// reads, rotation the only channel that varies, the stored rotation at
/// the start time, and a target node with no collision object at or below
/// it; then the leaf with its other pose's key. Otherwise the reason.
pub fn read(n: &Nif<'_>, b: &Budget) -> Result<std::result::Result<Leaf, &'static str>> {
    let controllers: Vec<usize> = (0..n.blocks.len())
        .filter(|i| {
            let t = type_of(n, *i);
            t.ends_with("Controller") || t == "NiControllerManager"
        })
        .collect();
    let [controller] = controllers[..] else {
        return Ok(Err("not exactly one controller"));
    };
    let track = match clock::transform_track(n, controller, b)? {
        Ok(t) => t,
        Err(why) => return Ok(Err(why)),
    };
    if track.timing.cycle() != clock::CYCLE_CLAMP {
        return Ok(Err("cycle type not clamped"));
    }
    if track.unread().is_some() {
        return Ok(Err("keys the clock does not read"));
    }
    if track.varies() != [false, true, false] {
        return Ok(Err(
            "a channel other than rotation varies, or rotation does not",
        ));
    }
    let Some(rotation) = track.keys.as_ref().and_then(|k| k.rotation.as_ref()) else {
        return Ok(Err("no rotation keys"));
    };
    let tol = f64::from(crate::rest::TOLERANCE[1]);
    let start = f64::from(track.timing.start);
    let Some(first) = track.words_at(start) else {
        return Ok(Err("the start pose is not read"));
    };
    if angle_between(&first.rotation, &track.stored_rotation) > tol {
        return Ok(Err("the start pose is not the stored pose"));
    }
    let mut far = None::<(f64, f64)>;
    for t in rotation_key_times(rotation) {
        let Some(w) = track.words_at(t) else {
            return Ok(Err("a key's pose is not read"));
        };
        let a = angle_between(&w.rotation, &track.stored_rotation);
        if far.is_none_or(|(_, best)| a > best) {
            far = Some((t, a));
        }
    }
    let Some((key_time, key_angle)) = far.filter(|(_, a)| *a > tol) else {
        return Ok(Err("no key leaves the stored pose"));
    };
    if bears_collision(n, track.node) {
        return Ok(Err("the turned node bears collision"));
    }
    Ok(Ok(Leaf {
        controller,
        node: track.node,
        track,
        key_time,
        key_angle,
    }))
}

/// The motion in the model's space that the keyed pose ([`Leaf::key_time`])
/// gives a draw whose
/// node chain is `chain` (blocks from the root, as
/// `fo4_render::draw::DrawChain::blocks`): the chain composed root first up
/// to the leaf's node with that node at its keyed pose, after the chain as
/// stored undone. `None` when the chain does not pass through the leaf's
/// node (the draw does not move) or a block is not read.
pub fn key_motion(n: &Nif<'_>, leaf: &Leaf, chain: &[usize]) -> Option<Transform> {
    let at = chain.iter().position(|b| *b == leaf.node)?;
    let words = leaf.track.words_at(leaf.key_time)?;
    let key_local = {
        let mut w = words;
        // Only rotation varies ([`read`]): the other channels keep the
        // stored words exactly.
        w.translation = leaf.track.stored_translation.map(f64::from);
        w.scale = f64::from(leaf.track.stored_scale);
        w.transform()
    };
    let (mut stored, mut posed) = (Transform::IDENTITY, Transform::IDENTITY);
    for &block in &chain[..=at] {
        let own = match n.blocks.get(block).and_then(|x| x.decoded.as_ref()) {
            Some(Data::Node { fields: nf, .. }) if nf.iter().any(|f| f.name == "Translation") => {
                Some(Transform::from_fields(nf, "leaf chain").ok()?)
            }
            _ => None,
        };
        if let Some(t) = own {
            stored = stored.compose(t);
        }
        if block == leaf.node {
            posed = posed.compose(key_local);
        } else if let Some(t) = own {
            posed = posed.compose(t);
        }
    }
    Some(posed.compose(inverse(stored)))
}

/// The inverse of a transform with an orthonormal rotation and a scale
/// that is not 0 (`Transform::apply` is `t + s R^T p`).
pub fn inverse(t: Transform) -> Transform {
    let r = t.rotation;
    let s = 1.0 / t.scale;
    Transform {
        rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[j][i])),
        translation: std::array::from_fn(|i| {
            -s * (0..3).map(|j| r[i][j] * t.translation[j]).sum::<f64>()
        }),
        scale: s,
    }
}

/// The block types whose animation blockers a leaf's reading sets aside:
/// the controller, its interpolator and its data.
const LEAF_BLOCKS: [&str; 3] = [
    "NiTransformController",
    "NiTransformInterpolator",
    "NiTransformData",
];

/// Whether `e`, of origin `origin`, is one of the animation blockers of a
/// leaf model ([`read`]) that the reading sets aside for render readiness:
/// an `UnsupportedAnimationRuntime` raised in the model for its transform
/// controller, interpolator or data that no rule covers.
pub fn aside(e: &Error, origin: readiness::Origin) -> bool {
    origin == readiness::Origin::Model
        && e.code == "UnsupportedAnimationRuntime"
        && hypotheses::covering(e).is_none()
        && e.found
            .split(';')
            .next()
            .is_some_and(|t| LEAF_BLOCKS.contains(&t))
}

#[cfg(test)]
mod tests;
