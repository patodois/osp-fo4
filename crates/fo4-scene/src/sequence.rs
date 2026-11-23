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
}
