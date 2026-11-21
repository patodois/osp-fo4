//! The top-level object of a payload, `hknpPhysicsSystemData` or
//! `hknpRagdollData`: its array slots and the 96-byte elements of the array at
//! +64, each of which links to one shape object and stores a translation and
//! four f32 of unit norm.
//!
//! Every rule enforced here held on all 275,342 payloads of the measured
//! installation; `spec/havok-bodies.md` gives the counts. Field names are
//! this project's labels. That an element is a body and that Body ID i of a
//! `bhkNPCollisionObject` selects element i are hypotheses.
use crate::view::{norm, Array, View};
use crate::Container;
use fo4_core::{Budget, Result};
use serde::Serialize;

pub const PHYSICS_SYSTEM: &str = "hknpPhysicsSystemData";
pub const RAGDOLL: &str = "hknpRagdollData";
/// (slot offset, element size) of the arrays of both top-level classes.
pub const SYSTEM_ARRAYS: [(u32, u32); 6] =
    [(16, 80), (32, 64), (48, 112), (64, 96), (80, 24), (96, 8)];
/// The extra slot of `hknpRagdollData`.
pub const RAGDOLL_ARRAY: (u32, u32) = (128, 4);
/// Bytes of one element of the array at +64.
pub const BODY: usize = 96;
/// Largest stored deviation of a body's four f32 from unit norm was 1.9e-7.
pub const UNIT_TOLERANCE: f64 = 1e-5;
const BODIES: usize = 3;
const SHAPE_LIST: usize = 5;

/// One element of the array at +64.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Body {
    pub index: usize,
    /// Third-table index of the object linked from byte 0.
    pub shape: usize,
    /// u16 at byte 18: the position in the array at +96 that links to the
    /// same object. The values form a permutation of 0..n.
    pub shape_slot: u16,
    /// Three f32 at byte 48.
    pub translation: [f32; 3],
    /// Four f32 at byte 64, stored order, unit norm within [`UNIT_TOLERANCE`].
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct PhysicsSystem<'a> {
    pub object: usize,
    pub class: &'a str,
    /// The checked slots: the six of [`SYSTEM_ARRAYS`], then [`RAGDOLL_ARRAY`]
    /// for a ragdoll.
    pub arrays: Vec<Array>,
    pub bodies: Vec<Body>,
    #[serde(skip)]
    bytes: &'a [u8],
}

impl<'a> PhysicsSystem<'a> {
    /// Reads object `index` of `c`. `file` and `base` are those passed to
    /// [`Container::read`].
    pub fn read(
        c: &Container<'a>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[PHYSICS_SYSTEM, RAGDOLL],
            "InvalidHavokPhysicsSystem",
        )?;
        let mut arrays = Vec::new();
        let slots = SYSTEM_ARRAYS
            .iter()
            .chain((v.span.class == RAGDOLL).then_some(&RAGDOLL_ARRAY));
        for (field, element) in slots {
            b.reserve(&mut arrays, 1, file, v.abs(*field as usize))?;
            arrays.push(v.array(*field as usize, *element)?);
        }
        let n = arrays[BODIES].count;
        for k in [0, SHAPE_LIST] {
            if arrays[k].count != n {
                return Err(v.err(
                    arrays[k].field as usize + 8,
                    format!("{n} elements, as the array at +64"),
                    arrays[k].count,
                ));
            }
        }
        let list = arrays[SHAPE_LIST];
        let body_array = arrays[BODIES];
        let mut seen = Vec::new();
        b.reserve(&mut seen, n as usize, file, v.abs(72))?;
        seen.resize(n as usize, false);
        let mut bodies = Vec::new();
        b.reserve(&mut bodies, n as usize, file, v.abs(72))?;
        for i in 0..n as usize {
            let e = body_array
                .element_at(i)
                .ok_or_else(|| v.err(72, "element inside the array", i))?;
            let shape = v.required_link(e, "shape")?;
            let slot = v.u16(e + 18)?;
            let fresh = seen.get_mut(slot as usize).filter(|s| !**s);
            let Some(fresh) = fresh else {
                return Err(v.err(
                    e + 18,
                    format!("an unused position below {n} in the array at +96"),
                    slot,
                ));
            };
            *fresh = true;
            let listed = list.element_at(slot as usize).and_then(|l| v.link(l));
            if listed != Some(shape) {
                return Err(v.err(
                    e,
                    format!("the object linked from position {slot} of the array at +96"),
                    format!("{shape} (that position links to {listed:?})"),
                ));
            }
            let translation = v.finite::<3>(e + 48, "translation")?;
            let rotation = v.finite::<4>(e + 64, "rotation")?;
            let r = norm(&rotation);
            if (r - 1.0).abs() > UNIT_TOLERANCE {
                return Err(v.err(e + 64, format!("unit norm within {UNIT_TOLERANCE}"), r));
            }
            bodies.push(Body {
                index: i,
                shape,
                shape_slot: slot,
                translation,
                rotation,
            });
        }
        Ok(Self {
            object: index,
            class: v.span.class,
            arrays,
            bodies,
            bytes: v.bytes,
        })
    }
    /// The object's stored bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    /// The stored bytes of body `i`.
    pub fn body_bytes(&self, i: usize) -> Option<&'a [u8]> {
        let at = self.arrays.get(BODIES)?.element_at(i)?;
        self.bytes.get(at..at.checked_add(BODY)?)
    }
    /// The checked slot at `field`, if this class has one there.
    pub fn array(&self, field: u32) -> Option<&Array> {
        self.arrays.iter().find(|a| a.field == field)
    }
}

#[cfg(test)]
mod tests;
