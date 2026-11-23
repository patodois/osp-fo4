//! A model's controller sequences (`NiControllerSequence`) as stored, and
//! the transform track one of them gives a named node: what the moving
//! part of an animated model (an elevator's platform) does while a
//! sequence plays (`spec/timeline.md`, The elevator second;
//! `spec/seen-census.txt`).
//!
//! Read: each sequence's name, start and stop times, cycle type and
//! frequency, and its controlled blocks (the node name and the
//! interpolator link); a block's `NiTransformInterpolator` with its
//! `NiTransformData` keys ([`crate::clock::NodeKeys`]). Which sequence
//! plays, from when, is not in the model: the run's animation graph says
//! it (`fo4_vm`'s world model), and a frame or a walk takes it from its
//! caller. Sampling the keys is [`crate::clock::Keys::sample`]'s reading.
use crate::clock::NodeKeys;
use fo4_core::{Budget, Result};
use fo4_nif::{Data, Field, Nif, Value};

/// One `NiControllerSequence` of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// The block index.
    pub block: usize,
    pub name: String,
    pub start: f32,
    pub stop: f32,
    pub cycle: u32,
    pub frequency: f32,
    /// (node name, interpolator block) of each controlled block, in order.
    pub blocks: Vec<(String, Option<usize>)>,
    /// Whether it links a text key block.
    pub text_keys: bool,
}

fn fields<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [Field]> {
    match n.blocks.get(i)?.decoded.as_ref()? {
        Data::Fields(f) => Some(f),
        Data::Node { fields, .. } => Some(fields),
        _ => None,
    }
}

fn type_of<'n>(n: &'n Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map_or("", String::as_str)
}

fn text(n: &Nif<'_>, v: &Value) -> Option<String> {
    match v {
        Value::StringIndex(s) => n
            .strings
            .get(*s as usize)
            .map(|x| String::from_utf8_lossy(x).into_owned()),
        _ => None,
    }
}

fn float(v: &Value) -> Option<f32> {
    match v {
        Value::FloatBits(b) => Some(f32::from_bits(*b)),
        _ => None,
    }
}

fn link(v: &Value) -> Option<usize> {
    match v {
        Value::Ref(r) if *r >= 0 => Some(*r as usize),
        _ => None,
    }
}

/// Every controller sequence of `n`, in block order.
pub fn sequences(n: &Nif<'_>) -> Vec<Sequence> {
    let mut out = Vec::new();
    for i in 0..n.blocks.len() {
        if type_of(n, i) != "NiControllerSequence" {
            continue;
        }
        let Some(f) = fields(n, i) else { continue };
        let mut s = Sequence {
            block: i,
            name: String::new(),
            start: 0.,
            stop: 0.,
            cycle: 0,
            frequency: 1.,
            blocks: Vec::new(),
            text_keys: false,
        };
        let mut interpolator = None;
        for x in f {
            match x.name {
                "Name" => s.name = text(n, &x.value).unwrap_or_default(),
                "Interpolator" => interpolator = Some(link(&x.value)),
                "Node Name" => {
                    let node = text(n, &x.value).unwrap_or_default();
                    s.blocks.push((node, interpolator.take().flatten()));
                }
                "Start Time" => s.start = float(&x.value).unwrap_or(0.),
                "Stop Time" => s.stop = float(&x.value).unwrap_or(0.),
                "Frequency" => s.frequency = float(&x.value).unwrap_or(1.),
                "Cycle Type" => {
                    if let Value::U32(c) = x.value {
                        s.cycle = c;
                    }
                }
                "Text Keys" => s.text_keys = link(&x.value).is_some(),
                _ => {}
            }
        }
        out.push(s);
    }
    out
}

/// The keys of the transform `s` gives `node`: its block's
/// `NiTransformInterpolator` with an `NiTransformData`; `Ok(None)` when the
/// sequence has no such block for the node or the keys are not the reader's.
pub fn node_keys(n: &Nif<'_>, s: &Sequence, node: &str, b: &Budget) -> Result<Option<NodeKeys>> {
    for (name, ip) in &s.blocks {
        if name != node {
            continue;
        }
        let Some(ip) = *ip else { continue };
        if type_of(n, ip) != "NiTransformInterpolator" {
            continue;
        }
        let Some(ipf) = fields(n, ip) else { continue };
        let Some(d) = ipf
            .iter()
            .find(|x| x.name == "Data")
            .and_then(|x| link(&x.value))
        else {
            continue;
        };
        if type_of(n, d) != "NiTransformData" {
            continue;
        }
        let Some(df) = fields(n, d) else { continue };
        return NodeKeys::read(df, b);
    }
    Ok(None)
}

/// The block index and stored translation of the node named `name`.
pub fn node(n: &Nif<'_>, name: &str) -> Option<(usize, [f32; 3])> {
    for i in 0..n.blocks.len() {
        let Some(Data::Node { fields, .. }) = n.blocks[i].decoded.as_ref() else {
            continue;
        };
        let named = fields
            .iter()
            .any(|x| x.name == "Name" && text(n, &x.value).as_deref() == Some(name));
        if !named {
            continue;
        }
        let t: Vec<f32> = fields
            .iter()
            .filter(|x| x.name == "Translation")
            .filter_map(|x| float(&x.value))
            .collect();
        if let Ok(t) = <[f32; 3]>::try_from(t) {
            return Some((i, t));
        }
    }
    None
}

/// What a sequence's transform interpolator holds its node at in the
/// channels its keys leave out (`nif_sequence_interpolator_held_pose_v1`, a
/// pending rule): the interpolator's own translation and rotation (w, x, y,
/// z) where it links no data block or its data holds no keys for that
/// channel; each `None` where the channel has keys or every component is
/// stored as a value beyond [`UNSET`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldPose {
    pub translation: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
}

/// A stored component at or beyond this magnitude is read as not set (the
/// files store -3.4e38, the most negative `f32`, there).
pub const UNSET: f32 = 1e30;

/// The pose `s` holds `node` at in the channels without keys: its
/// controlled block for the node names an `NiTransformInterpolator`.
/// `None` when the sequence does not control the node so or its data block
/// is not the reader's ([`crate::clock::NodeKeys::read`]).
pub fn held_pose(n: &Nif<'_>, s: &Sequence, node: &str, b: &Budget) -> Result<Option<HeldPose>> {
    let Some(ip) = s
        .blocks
        .iter()
        .find(|(name, ip)| {
            name == node && ip.is_some_and(|i| type_of(n, i) == "NiTransformInterpolator")
        })
        .and_then(|(_, ip)| *ip)
    else {
        return Ok(None);
    };
    let Some(f) = fields(n, ip) else {
        return Ok(None);
    };
    let (mut keyed_t, mut keyed_r) = (false, false);
    if let Some(d) = f
        .iter()
        .find(|x| x.name == "Data")
        .and_then(|x| link(&x.value))
    {
        if type_of(n, d) != "NiTransformData" {
            return Ok(None);
        }
        let Some(df) = fields(n, d) else {
            return Ok(None);
        };
        let Some(keys) = NodeKeys::read(df, b)? else {
            return Ok(None);
        };
        keyed_t = !keys.translation.keys.is_empty();
        keyed_r = keys.rotation.is_some();
    }
    let values = |name: &str| -> Vec<f32> {
        f.iter()
            .filter(|x| x.name == name)
            .filter_map(|x| float(&x.value))
            .collect()
    };
    let set = |v: &[f32]| v.iter().all(|x| x.is_finite() && x.abs() < UNSET);
    let t = values("Translation");
    let q = values("Quaternion");
    Ok(Some(HeldPose {
        translation: <[f32; 3]>::try_from(t.as_slice())
            .ok()
            .filter(|v| !keyed_t && set(v)),
        rotation: <[f32; 4]>::try_from(q.as_slice())
            .ok()
            .filter(|v| !keyed_r && set(v)),
    }))
}

/// The local transform `s` gives the chain block `block` at key time
/// `time` (`nif_sequence_chain_pose_v1`): the sequence's controlled block
/// for the block's name must name an `NiTransformInterpolator`; each
/// channel takes its keys at `time` where its data holds some (translation
/// and scale as the clock samples them, quaternion rotation keys as
/// `nif_clock_transform_rotation_quaternion_v1` reads them, XYZ rotation
/// keys composed as `nif_clock_transform_rotation_xyz_v1` composes them),
/// else the interpolator's own value where set ([`UNSET`]), else the
/// block's stored words ([`crate::clock::TransformTrack::words_at`]).
/// `Ok(None)` when the sequence does not control the block so, the block is
/// not a node with stored transform words, its data is not the reader's or
/// a key type has no reading.
pub fn block_local(
    n: &Nif<'_>,
    s: &Sequence,
    block: usize,
    time: f64,
    b: &Budget,
) -> Result<Option<crate::transform::Transform>> {
    let Some(Data::Node { fields: nf, .. }) = n.blocks.get(block).and_then(|x| x.decoded.as_ref())
    else {
        return Ok(None);
    };
    let Some(name) = nf
        .iter()
        .find(|x| x.name == "Name")
        .and_then(|x| text(n, &x.value))
    else {
        return Ok(None);
    };
    let Some(ip) = s
        .blocks
        .iter()
        .find(|(x, ip)| {
            *x == name && ip.is_some_and(|i| type_of(n, i) == "NiTransformInterpolator")
        })
        .and_then(|(_, ip)| *ip)
    else {
        return Ok(None);
    };
    let Some(f) = fields(n, ip) else {
        return Ok(None);
    };
    let words = |fs: &[Field], name: &str| -> Vec<f32> {
        fs.iter()
            .filter(|x| x.name == name)
            .filter_map(|x| float(&x.value))
            .collect()
    };
    let (Ok(st), Ok(sr), Ok([ss])) = (
        <[f32; 3]>::try_from(words(nf, "Translation").as_slice()),
        <[f32; 9]>::try_from(words(nf, "Rotation").as_slice()),
        <[f32; 1]>::try_from(words(nf, "Scale").as_slice()),
    ) else {
        return Ok(None);
    };
    let keys = match f
        .iter()
        .find(|x| x.name == "Data")
        .and_then(|x| link(&x.value))
    {
        None => None,
        Some(d) => {
            if type_of(n, d) != "NiTransformData" {
                return Ok(None);
            }
            let Some(df) = fields(n, d) else {
                return Ok(None);
            };
            let Some(k) = NodeKeys::read(df, b)? else {
                return Ok(None);
            };
            Some(k)
        }
    };
    let set = |v: &[f32]| v.iter().all(|x| x.is_finite() && x.abs() < UNSET);
    let pt = words(f, "Translation");
    let pq = words(f, "Quaternion");
    let ps = words(f, "Scale");
    let track = crate::clock::TransformTrack {
        timing: crate::clock::Timing {
            flags: 0,
            frequency: s.frequency,
            phase: 0.,
            start: s.start,
            stop: s.stop,
        },
        node: block,
        stored_translation: st,
        stored_rotation: sr,
        stored_scale: ss,
        pose_translation: <[f32; 3]>::try_from(pt.as_slice()).ok().filter(|v| set(v)),
        pose_rotation: <[f32; 4]>::try_from(pq.as_slice()).ok().filter(|v| set(v)),
        pose_scale: <[f32; 1]>::try_from(ps.as_slice())
            .ok()
            .filter(|v| set(v))
            .map(|[v]| v),
        keys,
    };
    Ok(track.words_at(time).map(|w| w.transform()))
}

/// A collision body's chain posed by a sequence: the composition of its
/// node transforms, root first, as stored and as the sequence poses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainPose {
    pub stored: crate::transform::Transform,
    pub posed: crate::transform::Transform,
    /// The chain blocks the sequence sets.
    pub controlled: usize,
    /// The deepest chain block the sequence sets: the composition stops
    /// there (the nodes below it move with it).
    pub deepest: usize,
}

/// The chain `chain` (blocks root first, Target last, as
/// `collision::model` reads them) as stored and as `s` poses it at key time
/// `time` (`nif_sequence_chain_pose_v1`): composed root first, as the
/// collision placement composes it, up to the deepest block the sequence
/// sets through a transform interpolator ([`block_local`]); a block the
/// sequence sets takes its local transform from [`block_local`], every
/// other block its stored one (a block without stored transform words
/// adds nothing, as in the placement). With `root_left_out` the root's own
/// transform is left out of both (`nif_root_transform_replaced_v1`).
/// `Ok(None)` when the sequence sets no block of the chain or a block it
/// controls is not read.
pub fn chain_pose(
    n: &Nif<'_>,
    s: &Sequence,
    chain: &[usize],
    root_left_out: bool,
    time: f64,
    b: &Budget,
) -> Result<Option<ChainPose>> {
    use crate::transform::Transform;
    let controls = |block: usize| -> bool {
        let Some(Data::Node { fields: nf, .. }) =
            n.blocks.get(block).and_then(|x| x.decoded.as_ref())
        else {
            return false;
        };
        let Some(name) = nf
            .iter()
            .find(|x| x.name == "Name")
            .and_then(|x| text(n, &x.value))
        else {
            return false;
        };
        s.blocks.iter().any(|(x, ip)| {
            *x == name && ip.is_some_and(|i| type_of(n, i) == "NiTransformInterpolator")
        })
    };
    let first = usize::from(root_left_out);
    let Some(deepest) = (first..chain.len()).rev().find(|k| controls(chain[*k])) else {
        return Ok(None);
    };
    let (mut stored, mut posed) = (Transform::IDENTITY, Transform::IDENTITY);
    let mut controlled = 0;
    for &block in &chain[first..=deepest] {
        let own = match n.blocks.get(block).and_then(|x| x.decoded.as_ref()) {
            Some(Data::Node { fields: nf, .. }) if nf.iter().any(|f| f.name == "Translation") => {
                Some(Transform::from_fields(nf, "sequence chain")?)
            }
            _ => None,
        };
        if let Some(t) = own {
            stored = stored.compose(t);
        }
        if controls(block) {
            let Some(t) = block_local(n, s, block, time, b)? else {
                return Ok(None);
            };
            posed = posed.compose(t);
            controlled += 1;
        } else if let Some(t) = own {
            posed = posed.compose(t);
        }
    }
    Ok(Some(ChainPose {
        stored,
        posed,
        controlled,
        deepest: chain[deepest],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_nif::{Block, Field};

    fn field(name: &'static str, value: Value) -> Field {
        Field {
            name,
            offset: 0,
            value,
        }
    }

    fn fl(name: &'static str, v: f32) -> Field {
        field(name, Value::FloatBits(v.to_bits()))
    }

    /// An `NiTransformData` with XYZ rotation keys (one key on each axis)
    /// and, with `translation`, one translation key.
    fn data(translation: bool) -> Vec<Field> {
        let u = |name: &'static str, v: u32| field(name, Value::U32(v));
        let mut f = vec![u("Num Rotation Keys", 1), u("Rotation Type", 4)];
        for _ in 0..3 {
            f.extend([
                u("Num Keys", 1),
                u("Interpolation", 1),
                fl("Key Time", 0.),
                fl("Key Value", 0.),
            ]);
        }
        f.push(u("Num Keys", u32::from(translation)));
        if translation {
            f.extend([
                u("Interpolation", 1),
                fl("Key Time", 0.),
                fl("Key Value", 5.),
                fl("Key Value", 5.),
                fl("Key Value", 5.),
            ]);
        }
        f.push(u("Num Keys", 0));
        f
    }

    fn nif(data_link: i32, t: [f32; 3]) -> Nif<'static> {
        let block = |ty: u16, f: Vec<Field>| Block {
            type_index: ty,
            data: 0..0,
            decoded: Some(Data::Fields(f)),
            raw_tail: 0..0,
            diagnostics: Vec::new(),
        };
        Nif {
            version: 0x1402_0007,
            user_version: 12,
            bs_version: 130,
            exports: Vec::new(),
            block_types: vec![
                "NiControllerSequence".into(),
                "NiTransformInterpolator".into(),
                "NiTransformData".into(),
            ],
            strings: vec![b"S".to_vec(), b"part".to_vec()],
            max_string_length: 4,
            groups: Vec::new(),
            roots: vec![0],
            blocks: vec![
                block(
                    0,
                    vec![
                        field("Name", Value::StringIndex(0)),
                        field("Interpolator", Value::Ref(1)),
                        field("Node Name", Value::StringIndex(1)),
                    ],
                ),
                block(
                    1,
                    vec![
                        fl("Translation", t[0]),
                        fl("Translation", t[1]),
                        fl("Translation", t[2]),
                        fl("Quaternion", f32::MIN),
                        fl("Quaternion", f32::MIN),
                        fl("Quaternion", f32::MIN),
                        fl("Quaternion", f32::MIN),
                        fl("Scale", 1.),
                        field("Data", Value::Ref(data_link)),
                    ],
                ),
                block(2, data(false)),
            ],
            bytes: &[],
        }
    }

    #[test]
    fn an_interpolator_without_keys_holds_its_own_translation() {
        let n = nif(-1, [1., 2., 3.]);
        let s = &sequences(&n)[0];
        let b = Budget::new(1 << 20);
        let p = held_pose(&n, s, "part", &b).unwrap().unwrap();
        assert_eq!(p.translation, Some([1., 2., 3.]));
        // The sentinel rotation is not set.
        assert_eq!(p.rotation, None);
        // Another node: no held pose.
        assert_eq!(held_pose(&n, s, "other", &b).unwrap(), None);
        // Data with rotation keys only: the translation is still held.
        let n = nif(2, [1., 2., 3.]);
        let p = held_pose(&n, &sequences(&n)[0], "part", &b)
            .unwrap()
            .unwrap();
        assert_eq!(p.translation, Some([1., 2., 3.]));
        // Data with translation keys: not held.
        let mut n = nif(2, [1., 2., 3.]);
        n.blocks[2].decoded = Some(Data::Fields(data(true)));
        let p = held_pose(&n, &sequences(&n)[0], "part", &b)
            .unwrap()
            .unwrap();
        assert_eq!(p.translation, None);
        // A sentinel translation is not set either.
        let n = nif(-1, [f32::MIN, f32::MIN, f32::MIN]);
        let p = held_pose(&n, &sequences(&n)[0], "part", &b)
            .unwrap()
            .unwrap();
        assert_eq!(p.translation, None);
    }

    /// A node's fields: name, stored translation, rotation words (identity)
    /// and scale 1.
    fn node_fields(name: u32, t: [f32; 3]) -> Vec<Field> {
        let mut f = vec![field("Name", Value::StringIndex(name))];
        f.extend(t.map(|v| fl("Translation", v)));
        for k in 0..9 {
            f.push(fl("Rotation", if k % 4 == 0 { 1. } else { 0. }));
        }
        f.push(fl("Scale", 1.));
        f
    }

    /// Root (10, 0, 0) -> part (5, 0, 0) -> tail (0, 3, 0), the sequence
    /// setting the part with XYZ rotation keys: a quarter turn about Z, no
    /// translation keys and an unset interpolator translation.
    fn chain_nif() -> Nif<'static> {
        let mut n = nif(2, [f32::MIN; 3]);
        let u = |name: &'static str, v: u32| field(name, Value::U32(v));
        let mut d = vec![u("Num Rotation Keys", 1), u("Rotation Type", 4)];
        for angle in [0., 0., std::f32::consts::FRAC_PI_2] {
            d.extend([
                u("Num Keys", 1),
                u("Interpolation", 1),
                fl("Key Time", 0.),
                fl("Key Value", angle),
            ]);
        }
        d.extend([u("Num Keys", 0), u("Num Keys", 0)]);
        n.blocks[2].decoded = Some(Data::Fields(d));
        n.strings.extend([b"root".to_vec(), b"tail".to_vec()]);
        let node = |f: Vec<Field>, children: Vec<i32>| Block {
            type_index: 3,
            data: 0..0,
            decoded: Some(Data::Node {
                fields: f,
                children,
            }),
            raw_tail: 0..0,
            diagnostics: Vec::new(),
        };
        n.block_types.push("NiNode".into());
        n.blocks.push(node(node_fields(2, [10., 0., 0.]), vec![4]));
        n.blocks.push(node(node_fields(1, [5., 0., 0.]), vec![5]));
        n.blocks.push(node(node_fields(3, [0., 3., 0.]), vec![]));
        n
    }

    #[test]
    fn a_sequence_poses_a_chain_with_its_rotation_keys_up_to_its_deepest_node() {
        let n = chain_nif();
        let s = &sequences(&n)[0];
        let b = Budget::new(1 << 20);
        // The part's local transform: the stored translation (the
        // interpolator's is unset), turned a quarter about Z.
        let l = block_local(&n, s, 4, 0., &b).unwrap().unwrap();
        let p = l.apply([1., 0., 0.]);
        assert!(
            (p[0] - 5.).abs() < 1e-6 && (p[1] - 1.).abs() < 1e-6,
            "{p:?}"
        );
        // Blocks the sequence does not set: none.
        assert_eq!(block_local(&n, s, 3, 0., &b).unwrap(), None);
        assert_eq!(block_local(&n, s, 1, 0., &b).unwrap(), None);
        let c = chain_pose(&n, s, &[3, 4, 5], false, 0., &b)
            .unwrap()
            .unwrap();
        assert_eq!((c.controlled, c.deepest), (1, 4));
        // Composed to the part: the tail below it is left out.
        assert_eq!(c.stored.apply([0.; 3]), [15., 0., 0.]);
        let q = c.posed.apply([1., 0., 0.]);
        assert!(
            (q[0] - 15.).abs() < 1e-6 && (q[1] - 1.).abs() < 1e-6,
            "{q:?}"
        );
        // The root left out of both.
        let r = chain_pose(&n, s, &[3, 4, 5], true, 0., &b)
            .unwrap()
            .unwrap();
        assert_eq!(r.stored.apply([0.; 3]), [5., 0., 0.]);
        // A chain the sequence does not set.
        assert_eq!(chain_pose(&n, s, &[3, 5], false, 0., &b).unwrap(), None);
        // Translation keys win over the stored translation.
        let mut n = chain_nif();
        let Some(Data::Fields(d)) = n.blocks[2].decoded.as_mut() else {
            unreachable!()
        };
        let at = d.len() - 2;
        d.splice(
            at..at + 1,
            [
                field("Num Keys", Value::U32(1)),
                field("Interpolation", Value::U32(1)),
                fl("Key Time", 0.),
                fl("Key Value", 7.),
                fl("Key Value", 0.),
                fl("Key Value", 0.),
            ],
        );
        let s = &sequences(&n)[0];
        let c = chain_pose(&n, s, &[3, 4, 5], false, 0., &b)
            .unwrap()
            .unwrap();
        let q = c.posed.apply([0.; 3]);
        assert!((q[0] - 17.).abs() < 1e-6, "{q:?}");
    }
}
