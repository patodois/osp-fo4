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
