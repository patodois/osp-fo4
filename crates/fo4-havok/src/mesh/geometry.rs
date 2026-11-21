//! Decoded geometry of `hknpCompressedMeshShapeData`: the vertices of every
//! section, its 4-index primitives and, in sections whose byte 93 is 1, the
//! vertex records some primitives address (`spec/havok-mesh.md`).
use super::tree::MeshBox;
use super::{
    MeshData, MeshSection, MESH_BOX_TOLERANCE, PLACEHOLDER_PRIMITIVE, PRIMITIVES, SECTION_VERTICES,
    SHARED_INDICES, SHARED_VERTICES, VERTICES,
};
use fo4_core::{Budget, Result};
use serde::Serialize;
use std::ops::Range;

/// Low byte of the first u16 of every measured vertex record.
pub const RECORD_KINDS: [u8; 4] = [0x12, 0x50, 0x51, 0x52];
/// Bit of the kind byte set when the record stores a third u16, the high
/// half of an f32 radius.
const RADIUS_BIT: u8 = 0x40;
/// Steps of the 11, 11 and 10-bit fields of a record word.
const RECORD_STEPS: [f64; 3] = [2047.0, 2047.0, 1023.0];
const CODE: &str = "InvalidHavokCompressedMeshData";

/// One stored primitive with its indices into [`Geometry::vertices`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Primitive {
    pub section: u32,
    /// Low u16 of the run that covers the primitive; below the length of the
    /// material array linked from the shape in every measured run.
    pub run_value: u16,
    /// The third and fourth stored bytes differ.
    pub quad: bool,
    /// The four stored bytes, each resolved to a vertex.
    pub indices: [u32; 4],
}
impl Primitive {
    /// The triangle [a, b, c] and, when the third and fourth stored bytes
    /// differ, the second triangle [a, c, d]: the split and winding that
    /// coincide with the render mesh in the measured files
    /// (`spec/havok-mesh.md`).
    pub fn triangles(&self) -> ([u32; 3], Option<[u32; 3]>) {
        let [a, b, c, d] = self.indices;
        ([a, b, c], self.quad.then_some([a, c, d]))
    }
}

/// A primitive of a section whose byte 93 is 1 whose last three bytes are
/// equal and whose first byte, minus the section's vertex count, names a
/// position in the section's shared indices. There a u16 holds the kind (low
/// byte) and the word count (high byte), the next u16 the offset of the words
/// in 8-byte elements of the array at +144 (plus byte 92 << 16), and, when
/// bit 0x40 of the kind is set, a third u16 the high half of an f32 radius.
/// Each word holds 11, 11 and 10-bit fields spread over the primitive's box
/// in the section tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeshRecord {
    pub section: u32,
    /// Position of the primitive in its section.
    pub primitive: u32,
    pub run_value: u16,
    /// Low byte of the record's first u16, one of [`RECORD_KINDS`].
    pub kind: u8,
    /// The repeated second to fourth byte of the primitive; meaning open.
    pub repeated: u8,
    /// 0 when the kind has no radius bit.
    pub radius: f32,
    /// First 8-byte element of the array at +144 holding the words (byte 92
    /// << 16 included).
    pub offset: u32,
    /// The record's words in [`Geometry::record_vertices`].
    pub vertices: Range<u32>,
    /// The primitive's box from the section tree.
    pub frame: MeshBox,
}

/// Decoded vertices, primitives and vertex records of every section.
#[derive(Debug, Clone, Serialize)]
pub struct Geometry {
    /// The section vertices of each section in stored order, each section's
    /// own vertices followed by the shared vertices it is the first to
    /// reference.
    pub vertices: Vec<[f32; 3]>,
    pub primitives: Vec<Primitive>,
    pub records: Vec<MeshRecord>,
    pub record_vertices: Vec<[f32; 3]>,
    /// Primitives stored as [`PLACEHOLDER_PRIMITIVE`], left out of `primitives`.
    pub placeholders: usize,
}
impl Geometry {
    /// Every triangle of [`Primitive::triangles`], in primitive order.
    pub fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.primitives.iter().flat_map(|p| {
            let (t, u) = p.triangles();
            std::iter::once(t).chain(u)
        })
    }
}

fn inside(lo: f32, hi: f32, x: f32, tolerance: f64) -> bool {
    f64::from(x) >= f64::from(lo) - tolerance && f64::from(x) <= f64::from(hi) + tolerance
}

/// Whether stored primitive bytes `raw` address a record in section `s`.
pub(super) fn is_record(s: &MeshSection, raw: [u8; 4]) -> bool {
    s.flagged
        && raw[1] == raw[2]
        && raw[2] == raw[3]
        && u32::from(raw[0]) >= s.vertices.len() as u32
}

impl MeshData<'_> {
    /// Decodes every section. A vertex outside its section's box, an index
    /// past the vertices, a primitive reaching into a record, or a record
    /// with an unmeasured kind, no words, words past the array at +144, a
    /// radius that is not a finite f32 ≥ 0 or an inverted box is
    /// `InvalidHavokCompressedMeshData`.
    pub fn geometry(&self, b: &Budget) -> Result<Geometry> {
        let shared = self.arrays[SHARED_VERTICES].count as usize;
        let mut out = Geometry {
            vertices: Vec::new(),
            primitives: Vec::new(),
            records: Vec::new(),
            record_vertices: Vec::new(),
            placeholders: 0,
        };
        // Position in `vertices` of each shared vertex once referenced.
        let mut placed = Vec::new();
        b.reserve(&mut placed, shared, &self.file, self.at0)?;
        placed.resize(shared, u32::MAX);
        for (si, s) in self.sections.iter().enumerate() {
            let at = self.at0.saturating_add(u64::from(s.offset));
            b.reserve(&mut out.vertices, s.index_limit() as usize, &self.file, at)?;
            b.reserve(&mut out.primitives, s.primitives.len(), &self.file, at)?;
            self.section_geometry(si, s, &mut out, &mut placed, b)?;
        }
        Ok(out)
    }

    fn word_of(&self, element: u32, half: u32) -> Option<u32> {
        let at = self.arrays[SHARED_VERTICES].element_at(element as usize)?;
        self.word(at.checked_add(4 * half as usize)?)
    }

    fn section_geometry(
        &self,
        si: usize,
        s: &MeshSection,
        out: &mut Geometry,
        placed: &mut [u32],
        b: &Budget,
    ) -> Result<()> {
        let m = s
            .min
            .iter()
            .chain(&s.max)
            .fold(1f64, |m, x| m.max(f64::from(x.abs())));
        let t = m * MESH_BOX_TOLERANCE;
        let outside = |p: [f32; 3]| (0..3).find(|k| !inside(s.min[*k], s.max[*k], p[*k], t));
        let box_err = |at: usize, what: &str, p: [f32; 3], k: usize| {
            self.err(
                CODE,
                at,
                format!("{what} inside the section box [{}, {}]", s.min[k], s.max[k]),
                p[k],
            )
        };
        let limit = s.index_limit();
        let nv = s.vertices.len() as u32;
        let index_at = |pos: u32| {
            self.arrays[SHARED_INDICES]
                .element_at((s.shared.start + pos) as usize)
                .unwrap_or(0)
        };
        // Shared-index positions that belong to a record.
        let mut in_record = [false; SECTION_VERTICES];
        let mut records = 0usize;
        for j in s.primitives.clone() {
            let raw = self.primitive(j).unwrap_or(PLACEHOLDER_PRIMITIVE);
            // A record's repeated byte is not an index.
            let checked = if is_record(s, raw) {
                &raw[..1]
            } else {
                &raw[..]
            };
            if let Some(x) = checked.iter().position(|x| u32::from(*x) >= limit) {
                if raw != PLACEHOLDER_PRIMITIVE {
                    let at = self.arrays[PRIMITIVES].element_at(j as usize).unwrap_or(0);
                    return Err(self.err(
                        CODE,
                        at + x,
                        format!("a vertex index < {limit}"),
                        raw[x],
                    ));
                }
            }
            if !is_record(s, raw) {
                continue;
            }
            records += 1;
            let pos = u32::from(raw[0]) - nv;
            let tag = self.shared_index(s.shared.start + pos).unwrap_or(0);
            let len = if tag as u8 & RADIUS_BIT != 0 { 3 } else { 2 };
            if pos + len > s.shared.len() as u32 {
                return Err(self.err(
                    CODE,
                    index_at(pos),
                    format!("a record of {len} u16 inside the section's shared indices"),
                    pos,
                ));
            }
            for x in pos..pos + len {
                if let Some(f) = in_record.get_mut(x as usize) {
                    *f = true;
                }
            }
        }
        let mut map = [0u32; SECTION_VERTICES];
        for (slot, i) in map.iter_mut().zip(s.vertices.clone()) {
            let at = self.arrays[VERTICES].element_at(i as usize).unwrap_or(0);
            let p = self
                .section_vertex(s, i)
                .ok_or_else(|| self.err(CODE, at, "a vertex inside the array", i))?;
            if let Some(k) = outside(p) {
                return Err(box_err(at, "a vertex", p, k));
            }
            *slot = out.vertices.len() as u32;
            out.vertices.push(p);
        }
        let shared = placed.len();
        for (k, j) in s.shared.clone().enumerate() {
            if in_record.get(k).copied().unwrap_or(false) {
                continue;
            }
            let at = index_at(k as u32);
            let target =
                self.shared_index(j)
                    .map_or(u32::MAX, |x| u32::from(x) + s.shared_high()) as usize;
            let p = self
                .shared_vertex(target as u32)
                .filter(|_| target < shared)
                .ok_or_else(|| {
                    self.err(
                        CODE,
                        at,
                        format!("a shared vertex index < {shared}"),
                        target,
                    )
                })?;
            if let Some(a) = outside(p) {
                return Err(box_err(at, "a shared vertex", p, a));
            }
            let Some(at_list) = placed.get_mut(target) else {
                continue;
            };
            if *at_list == u32::MAX {
                *at_list = out.vertices.len() as u32;
                out.vertices.push(p);
            }
            if let Some(slot) = map.get_mut(nv as usize + k) {
                *slot = *at_list;
            }
        }
        let frames = if records > 0 {
            self.primitive_boxes(s, b)?
        } else {
            Vec::new()
        };
        b.reserve(&mut out.records, records, &self.file, self.at0)?;
        let mut run = s.runs.start;
        let mut run_end = 0u32;
        let mut value = 0u16;
        for (k, j) in s.primitives.clone().enumerate() {
            while k as u32 >= run_end && run < s.runs.end {
                if let Some((val, first, count)) = self.run(run) {
                    value = val;
                    run_end = u32::from(first) + u32::from(count);
                }
                run += 1;
            }
            let raw = self.primitive(j).unwrap_or(PLACEHOLDER_PRIMITIVE);
            if raw == PLACEHOLDER_PRIMITIVE {
                out.placeholders += 1;
                continue;
            }
            let at = self.arrays[PRIMITIVES].element_at(j as usize).unwrap_or(0);
            if is_record(s, raw) {
                let frame = frames.get(k).copied().ok_or_else(|| {
                    self.err(CODE, s.tree.storage as usize, "a box per primitive", k)
                })?;
                let pos = u32::from(raw[0]) - nv;
                self.record(si, s, k as u32, pos, raw[1], value, frame, out, b)?;
                continue;
            }
            let reaches = |x: &u8| {
                u32::from(*x) >= nv
                    && in_record
                        .get((u32::from(*x) - nv) as usize)
                        .copied()
                        .unwrap_or(false)
            };
            if let Some(x) = raw.iter().position(reaches) {
                return Err(self.err(CODE, at + x, "an index that is not inside a record", raw[x]));
            }
            out.primitives.push(Primitive {
                section: si as u32,
                run_value: value,
                quad: raw[2] != raw[3],
                indices: raw.map(|x| map.get(x as usize).copied().unwrap_or(0)),
            });
        }
        Ok(())
    }

    /// Decodes the record at shared-index position `pos` of section `s`.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        si: usize,
        s: &MeshSection,
        primitive: u32,
        pos: u32,
        repeated: u8,
        run_value: u16,
        frame: MeshBox,
        out: &mut Geometry,
        b: &Budget,
    ) -> Result<()> {
        let j = s.shared.start + pos;
        let at = self.arrays[SHARED_INDICES]
            .element_at(j as usize)
            .unwrap_or(0);
        let u = |k: u32| self.shared_index(j + k).unwrap_or(0);
        let (tag, offset) = (u(0), u32::from(u(1)) + s.shared_high());
        let (kind, count) = (tag as u8, u32::from(tag >> 8));
        if !RECORD_KINDS.contains(&kind) {
            return Err(self.err(
                CODE,
                at,
                format!("a record kind in {RECORD_KINDS:02x?}"),
                format!("{kind:02x}"),
            ));
        }
        if count == 0 {
            return Err(self.err(CODE, at + 1, "at least one record word", 0));
        }
        let elements = self.arrays[SHARED_VERTICES].count;
        if offset.saturating_add(count.div_ceil(2)) > elements {
            return Err(self.err(
                CODE,
                at + 2,
                format!("record words inside the {elements} elements at +144"),
                format!("{offset} + {}", count.div_ceil(2)),
            ));
        }
        let radius = if kind & RADIUS_BIT != 0 {
            f32::from_bits(u32::from(u(2)) << 16)
        } else {
            0.0
        };
        if !radius.is_finite() || radius < 0.0 {
            return Err(self.err(CODE, at + 4, "a finite radius >= 0", radius));
        }
        let (lo, hi) = frame;
        if let Some(k) = (0..3).find(|k| lo[*k] > hi[*k]) {
            return Err(self.err(
                CODE,
                s.tree.storage as usize,
                format!("a primitive box with minimum <= maximum on axis {k}"),
                format!("[{}, {}]", lo[k], hi[k]),
            ));
        }
        let start = out.record_vertices.len() as u32;
        b.reserve(
            &mut out.record_vertices,
            count as usize,
            &self.file,
            self.at0,
        )?;
        for i in 0..count {
            // With an odd count the low half of the last element is left
            // over and the last word is its high half.
            let w = if count % 2 == 1 && i == count - 1 {
                2 * offset + count
            } else {
                2 * offset + i
            };
            let word = self.word_of(w / 2, w % 2).ok_or_else(|| {
                self.err(CODE, at + 2, "record words inside the array at +144", w)
            })?;
            let q = [word & 0x7ff, (word >> 11) & 0x7ff, word >> 22];
            out.record_vertices.push(std::array::from_fn(|k| {
                (lo[k] + f64::from(q[k]) * (hi[k] - lo[k]) / RECORD_STEPS[k]) as f32
            }));
        }
        out.records.push(MeshRecord {
            section: si as u32,
            primitive,
            run_value,
            kind,
            repeated,
            radius,
            offset,
            vertices: start..start + count,
            frame,
        });
        Ok(())
    }
}
