//! The two box trees of `hknpCompressedMeshShapeData`: the top tree at +16
//! (5-byte nodes, one leaf per section) and each section's tree (4-byte
//! nodes, one leaf per primitive).
//!
//! Both are stored in preorder. Bytes 0, 1 and 2 of a node hold two 4-bit
//! values each, for x, y and z: the high nibble `a` moves the minimum of the
//! parent's box up by a² / 226 of its extent, the low nibble `b` moves the
//! maximum down by b² / 226. The root's parent is the object box (top tree) or
//! the section box (section tree). Decoded this way every leaf box contains
//! its section box or its primitive's vertices (`spec/havok-mesh.md`); the
//! denominator is fixed by the depth-1 nodes, whose nibbles are the largest
//! that keep the box conservative.
use super::MeshData;
use super::MeshSection;
use fo4_core::{Budget, Result};

/// Denominator of the squared nibble (see the module documentation).
pub const TREE_SHRINK_DENOMINATOR: f64 = 226.0;

/// A box as (minimum, maximum), computed in f64 from the stored f32 boxes.
pub type MeshBox = ([f64; 3], [f64; 3]);

/// The child box of `parent` for the three node bytes `nibbles`.
pub fn shrink(parent: &MeshBox, nibbles: [u8; 3]) -> MeshBox {
    let (lo, hi) = parent;
    let mut out = (*lo, *hi);
    for (k, n) in nibbles.into_iter().enumerate() {
        let e = hi[k] - lo[k];
        let (a, b) = (f64::from(n >> 4), f64::from(n & 15));
        out.0[k] = lo[k] + a * a / TREE_SHRINK_DENOMINATOR * e;
        out.1[k] = hi[k] - b * b / TREE_SHRINK_DENOMINATOR * e;
    }
    out
}

/// One stored node: the three nibble bytes, whether it is an inner node, and
/// the leaf number when it is not.
pub(super) struct Node {
    pub nibbles: [u8; 3],
    pub inner: bool,
    pub leaf: u32,
}

/// Failure of [`decode`]: (node index, expected, found).
pub(super) type TreeError = (usize, String, String);

/// Decodes `count` preorder nodes read by `node` into one box per leaf
/// number below `leaves`, each leaf number once.
pub(super) fn decode(
    count: usize,
    leaves: usize,
    root: MeshBox,
    b: &Budget,
    file: &str,
    at: u64,
    node: impl Fn(usize) -> Option<Node>,
) -> Result<std::result::Result<Vec<MeshBox>, TreeError>> {
    let mut out: Vec<Option<MeshBox>> = Vec::new();
    b.reserve(&mut out, leaves, file, at)?;
    out.resize(leaves, None);
    // Parent boxes of the subtrees still to be read, the next one last.
    let mut pending: Vec<MeshBox> = Vec::new();
    b.reserve(&mut pending, count.min(64), file, at)?;
    pending.push(root);
    for i in 0..count {
        let Some(n) = node(i) else {
            return Ok(Err((i, "a node inside the array".into(), i.to_string())));
        };
        let Some(parent) = pending.pop() else {
            return Ok(Err((
                i,
                "no node after the tree ends".into(),
                i.to_string(),
            )));
        };
        let own = shrink(&parent, n.nibbles);
        if n.inner {
            if pending.len() + 2 > count {
                return Ok(Err((i, "a leaf".into(), "an inner node".into())));
            }
            b.reserve(&mut pending, 2, file, at)?;
            pending.push(own);
            pending.push(own);
            continue;
        }
        match out.get_mut(n.leaf as usize) {
            Some(slot) if slot.is_none() => *slot = Some(own),
            _ => {
                return Ok(Err((
                    i,
                    format!("a leaf number < {leaves} used once"),
                    n.leaf.to_string(),
                )))
            }
        }
    }
    if !pending.is_empty() {
        return Ok(Err((
            count,
            "a complete tree".into(),
            "missing nodes".into(),
        )));
    }
    let mut boxes = Vec::new();
    b.reserve(&mut boxes, leaves, file, at)?;
    for (k, x) in out.into_iter().enumerate() {
        match x {
            Some(x) => boxes.push(x),
            None => return Ok(Err((count, format!("a leaf numbered {k}"), "none".into()))),
        }
    }
    Ok(Ok(boxes))
}

fn widen(lo: [f32; 3], hi: [f32; 3]) -> MeshBox {
    (lo.map(f64::from), hi.map(f64::from))
}

impl MeshData<'_> {
    /// The box of every section from the top tree at +16, in section order.
    pub fn section_boxes(&self, b: &Budget) -> Result<Vec<MeshBox>> {
        const CODE: &str = "InvalidHavokCompressedMeshData";
        let a = self.arrays[super::TREE];
        let r = decode(
            a.count as usize,
            self.sections.len(),
            widen(self.min, self.max),
            b,
            &self.file,
            self.at0,
            |i| {
                let at = a.element_at(i)?;
                let n = self.bytes.get(at..at.checked_add(5)?)?;
                Some(Node {
                    nibbles: [n[0], n[1], n[2]],
                    inner: n[3] & 0x80 != 0,
                    leaf: u32::from(n[4]) | u32::from(n[3] & 0x7f) << 8,
                })
            },
        )?;
        r.map_err(|(i, expected, found)| {
            let at = a.element_at(i).unwrap_or(a.storage as usize);
            self.err(CODE, at, expected, found)
        })
    }
    /// The box of every primitive of section `s` from its tree, in primitive
    /// order.
    pub fn primitive_boxes(&self, s: &MeshSection, b: &Budget) -> Result<Vec<MeshBox>> {
        const CODE: &str = "InvalidHavokCompressedMeshData";
        let a = s.tree;
        let r = decode(
            a.count as usize,
            s.primitives.len(),
            widen(s.min, s.max),
            b,
            &self.file,
            self.at0.saturating_add(u64::from(s.offset)),
            |i| {
                let at = a.element_at(i)?;
                let n = self.bytes.get(at..at.checked_add(4)?)?;
                Some(Node {
                    nibbles: [n[0], n[1], n[2]],
                    inner: n[3] & 1 == 1,
                    leaf: u32::from(n[3] >> 1),
                })
            },
        )?;
        r.map_err(|(i, expected, found)| {
            let at = a.element_at(i).unwrap_or(a.storage as usize);
            self.err(CODE, at, expected, found)
        })
    }
}
