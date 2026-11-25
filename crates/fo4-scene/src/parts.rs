//! A refused static's unmoved parts (`static_unmoved_parts_drawn_v1`,
//! [`hypotheses::STATIC_PARTS`]; `spec/play.md`, The first lift's room
//! drawn by its unmoved parts).
//!
//! A static whose model holds blocks this project cannot settle (a
//! behaviour graph, particle systems and their controllers, blocks the
//! reader does not decode) is refused whole by render readiness, though
//! most of its shapes are plain geometry those blocks do not reach. This
//! reading takes the model's scene graph apart by what each shape reads:
//! the nodes from the root down to it, the shape and every block its links
//! reach other than nodes and collision (its data, shader and texture set,
//! alpha property, skin, extra data, controllers). A shape is left out,
//! counted by a code, when
//!
//! - a blocker no usable rule covers ([`open`]) lies on a block it reads
//!   (the blocker's code);
//! - a controller or controller sequence of the model can drive a block it
//!   reads ([`MOVED`]): an animation-state block in what it reads, an
//!   animation-state block whose `Target` (other than a holder's own,
//!   [`crate::collision::model::HOLDER_TARGETS`]) or `Extra Target` is a
//!   block it reads, or a controller sequence whose controlled block names
//!   a node of its chain or the shape itself; or a block whose type names a
//!   controller is not decoded, so its target is unknown;
//!
//! and every particle system the scene graph reaches is left out too
//! ([`PARTICLE`]). The other shapes are drawn as any shape of a ready model
//! is. Only [`read`] decides; [`aside`] names the blockers render readiness
//! then sets aside, behind `Loader::set_render_static_parts` (off by
//! default), and only for a static at least one of whose shapes stays.
use crate::model_policy::{block_class, fields};
use crate::{hypotheses, readiness};
use fo4_core::{Budget, Error, Result};
use fo4_nif::{Data, Nif, Value};
use fo4_plugin::static_frame::Class;
use std::collections::{BTreeMap, BTreeSet};

/// The code a shape a controller can drive is left out under.
pub const MOVED: &str = "ControllerDriven";
/// The code a particle system is left out under.
pub const PARTICLE: &str = "ParticleSystem";
/// The code the draw leaves out a shape under whose blend factors come
/// from a material file (`bgsm_blend_factors_alpha_function_v1`, a
/// numbering no source gives): not drawn under this reading.
pub const MATERIAL_BLEND: &str = "UnsourcedMaterialBlend";

/// What [`read`] found in a model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parts {
    /// Blocks the draw leaves out, by block index, with the code each is
    /// counted under.
    pub left: BTreeMap<usize, String>,
    /// Shapes the scene graph reaches that stay.
    pub kept: usize,
}

impl Parts {
    /// The code block `i` is left out under, if it is.
    pub fn left_out(&self, i: usize) -> Option<&str> {
        self.left.get(&i).map(String::as_str)
    }
}

/// Whether `e` is a blocker this reading places on a block: one of the
/// codes raised on one block of a model ([`readiness::BLOCK_CODES`]) that
/// no rule covers, or whose rule is refuted.
pub fn open(e: &Error) -> bool {
    readiness::BLOCK_CODES.contains(&readiness::Code::of(&e.code))
        && hypotheses::covering(e)
            .is_none_or(|h| hypotheses::status(h) == Some(hypotheses::Status::Refuted))
}

/// Whether `e`, of origin `origin`, is set aside for render readiness by
/// this reading: an [`open`] blocker raised in the model.
pub fn aside(e: &Error, origin: readiness::Origin) -> bool {
    origin == readiness::Origin::Model && open(e)
}

fn type_of<'n>(n: &'n Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map_or("", String::as_str)
}

fn name_of<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [u8]> {
    let data = n.blocks.get(i)?.decoded.as_ref()?;
    fields(data).iter().find_map(|f| match (f.name, &f.value) {
        ("Name", Value::StringIndex(s)) => n.strings.get(*s as usize).map(Vec::as_slice),
        _ => None,
    })
}

fn is_shape(n: &Nif<'_>, i: usize) -> bool {
    match n.blocks.get(i).and_then(|b| b.decoded.as_ref()) {
        Some(Data::Mesh(_)) => true,
        Some(Data::Fields(f)) => f.iter().any(|x| x.name == "Shader Property"),
        _ => false,
    }
}

fn is_node(n: &Nif<'_>, i: usize) -> bool {
    matches!(
        n.blocks.get(i).and_then(|b| b.decoded.as_ref()),
        Some(Data::Node { .. })
    )
}

/// The blocks a shape reads besides its chain: the shape and every block
/// its links reach, nodes and collision objects excepted.
fn reached(n: &Nif<'_>, shape: usize, b: &Budget) -> Result<BTreeSet<usize>> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![shape];
    while let Some(k) = stack.pop() {
        if k >= n.blocks.len() || !seen.insert(k) {
            continue;
        }
        fo4_core::limit(seen.len() as u64, n.blocks.len() as u64, "parts", 0)?;
        let Some(data) = n.blocks[k].decoded.as_ref() else {
            continue;
        };
        for f in fields(data) {
            if let Value::Ref(r) = f.value {
                let Ok(r) = usize::try_from(r) else { continue };
                if f.name == "Collision" || is_node(n, r) {
                    continue;
                }
                b.reserve(&mut stack, 1, "parts", 0)?;
                stack.push(r);
            }
        }
    }
    Ok(seen)
}

/// What can drive a block: the blocks named by animation-state blocks'
/// targets, the node names controller sequences name, and whether an
/// undecoded controller leaves its target unknown.
struct Movers {
    targets: BTreeSet<usize>,
    names: Vec<Vec<u8>>,
    unknown: bool,
}

fn movers(n: &Nif<'_>) -> Movers {
    let mut m = Movers {
        targets: BTreeSet::new(),
        names: Vec::new(),
        unknown: false,
    };
    for k in 0..n.blocks.len() {
        let ty = type_of(n, k);
        let Some(data) = n.blocks[k].decoded.as_ref() else {
            m.unknown |= ty.ends_with("Controller") || ty.ends_with("Ctlr");
            continue;
        };
        if block_class(ty) != Class::State {
            continue;
        }
        let holder = crate::collision::model::HOLDER_TARGETS.contains(&ty);
        for f in fields(data) {
            match (f.name, &f.value) {
                ("Target", Value::Ref(r)) if !holder => {
                    if let Ok(r) = usize::try_from(*r) {
                        m.targets.insert(r);
                    }
                }
                ("Extra Target", Value::Ref(r)) => {
                    if let Ok(r) = usize::try_from(*r) {
                        m.targets.insert(r);
                    }
                }
                ("Node Name", Value::StringIndex(s)) if ty == "NiControllerSequence" => {
                    if let Some(name) = n.strings.get(*s as usize) {
                        if !name.is_empty() && !m.names.contains(name) {
                            m.names.push(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    m
}

/// The parts of model `n` given its blockers (those [`open`] count; the
/// others are left to their rules): which shapes the draw leaves out, and
/// how many it keeps.
pub fn read(n: &Nif<'_>, blockers: &[Error], b: &Budget) -> Result<Parts> {
    let mut on: BTreeMap<usize, &str> = BTreeMap::new();
    for e in blockers.iter().filter(|e| open(e)) {
        let offset = e.source.offset;
        if let Some(k) = n
            .blocks
            .iter()
            .position(|x| (x.data.start as u64..x.data.end as u64).contains(&offset))
        {
            on.entry(k).or_insert(e.code.as_str());
        }
    }
    let movers = movers(n);
    let mut parts = Parts::default();
    // Depth first from the roots: (block, chain of nodes above it).
    let mut stack: Vec<(usize, Vec<usize>)> = n
        .roots
        .iter()
        .rev()
        .filter_map(|r| usize::try_from(*r).ok())
        .map(|r| (r, Vec::new()))
        .collect();
    let mut visited = BTreeSet::new();
    while let Some((i, chain)) = stack.pop() {
        if i >= n.blocks.len() || !visited.insert(i) {
            continue;
        }
        fo4_core::limit(visited.len() as u64, n.blocks.len() as u64, "parts", 0)?;
        if type_of(n, i).ends_with("ParticleSystem") {
            parts.left.insert(i, PARTICLE.into());
            continue;
        }
        if let Some(Data::Node { children, .. }) = n.blocks[i].decoded.as_ref() {
            let mut below = chain.clone();
            below.push(i);
            for c in children.iter().rev() {
                if let Ok(c) = usize::try_from(*c) {
                    b.reserve(&mut stack, 1, "parts", 0)?;
                    stack.push((c, below.clone()));
                }
            }
            continue;
        }
        if !is_shape(n, i) {
            continue;
        }
        let reads = reached(n, i, b)?;
        let all = || chain.iter().copied().chain(reads.iter().copied());
        if let Some(code) = all().find_map(|k| on.get(&k)) {
            parts.left.insert(i, (*code).into());
            continue;
        }
        let state = |k: usize| block_class(type_of(n, k)) == Class::State;
        let named = |k: usize| {
            name_of(n, k).is_some_and(|x| !x.is_empty() && movers.names.iter().any(|y| y == x))
        };
        let moved = movers.unknown
            || reads.iter().any(|k| state(*k))
            || all().any(|k| movers.targets.contains(&k))
            || chain.iter().chain([&i]).any(|k| named(*k));
        if moved {
            parts.left.insert(i, MOVED.into());
            continue;
        }
        parts.kept += 1;
    }
    Ok(parts)
}

#[cfg(test)]
mod tests;
