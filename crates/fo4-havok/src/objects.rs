//! Object level of the `__data__` section: where each object of the third
//! table ends, and the one shape class whose stored layout was measured.
//!
//! Every rule here was measured over all the payloads of one installation;
//! `spec/havok-objects.md` tags each one with its count. The names of fields
//! are this project's descriptive labels. Nothing here panics on any input.
use crate::{Container, Object};
use fo4_core::{Error, Result};
use serde::Serialize;

/// Class name of the shape that [`ConvexPolytope`] reads.
pub const CONVEX_POLYTOPE: &str = "hknpConvexPolytopeShape";
/// Bytes of a convex polytope before its vertex rows.
pub const CONVEX_POLYTOPE_FIXED: usize = 80;
/// The (count, relative offset) u16 pairs of a convex polytope: vertex rows,
/// plane rows, faces and vertex indices.
const VERTICES_AT: usize = 48;
const PLANES_AT: usize = 64;
const FACES_AT: usize = 68;
const INDICES_AT: usize = 72;
const ROW: usize = 16;
const FACE: usize = 4;
/// High 24 bits of the fourth word of every vertex row.
pub const VERTEX_TAG: u32 = 0x3f00_0000;
/// Largest vertex-row count whose low tag byte can carry the row index.
pub const MAX_VERTEX_ROWS: usize = 256;

/// Bytes of the `__data__` object area owned by one object of the third table:
/// from its offset to the next object's offset, or to the end of the object
/// bytes for the last one. Offsets are relative to the `__data__` start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObjectSpan<'a> {
    pub index: usize,
    pub offset: u32,
    pub end: u32,
    pub class: &'a str,
}
impl ObjectSpan<'_> {
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.offset)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Index of the object whose span holds `offset`: the last object starting at
/// or before it. `None` before the first object or at or past `end`.
pub(crate) fn owner(objects: &[Object<'_>], end: u32, offset: u32) -> Option<usize> {
    if offset >= end {
        return None;
    }
    objects
        .partition_point(|o| o.offset <= offset)
        .checked_sub(1)
}

impl<'a> Container<'a> {
    /// End of the object bytes, relative to the `__data__` start.
    pub fn objects_end(&self) -> u32 {
        self.data().map_or(0, |d| d.offsets[0])
    }
    /// Span of object `index` of the third table.
    pub fn object_span(&self, index: usize) -> Option<ObjectSpan<'a>> {
        let o = self.objects.get(index)?;
        let end = match self.objects.get(index + 1) {
            Some(next) => next.offset,
            None => self.objects_end(),
        };
        Some(ObjectSpan {
            index,
            offset: o.offset,
            end,
            class: o.class,
        })
    }
    /// Spans of all objects, in stored order.
    pub fn object_spans(&self) -> impl Iterator<Item = ObjectSpan<'a>> + '_ {
        (0..self.objects.len()).filter_map(|i| self.object_span(i))
    }
    /// Index of the object whose span holds a `__data__` offset.
    pub fn object_at(&self, offset: u32) -> Option<usize> {
        owner(&self.objects, self.objects_end(), offset)
    }
    /// The stored bytes of an object's span.
    pub fn span_bytes(&self, span: &ObjectSpan<'_>) -> &'a [u8] {
        self.object_bytes()
            .get(span.offset as usize..span.end as usize)
            .unwrap_or(&[])
    }
}

/// One stored face entry: its first position in the index bytes, its number
/// of indices and a fourth byte whose meaning is open. Read as stored; see
/// `spec/havok-objects.md` for the rules that hold on all but eight objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Face {
    pub first: u16,
    pub count: u8,
    pub tag: u8,
}

/// Typed view of one `hknpConvexPolytopeShape` object. `read` checks the
/// layout that held on every measured object; planes and faces are returned
/// as stored, without a claim about their consistency.
#[derive(Debug, Clone, Serialize)]
pub struct ConvexPolytope<'a> {
    pub object: usize,
    /// Offset of the object relative to the `__data__` start.
    pub offset: u32,
    #[serde(skip)]
    bytes: &'a [u8],
    /// Stored vertex rows, a multiple of 4.
    pub vertex_rows: usize,
    /// Leading rows whose tag byte equals their index; the rows after them
    /// repeat the last one.
    pub vertex_count: usize,
    /// Stored plane rows: the face count rounded up to a multiple of 4.
    pub plane_rows: usize,
    pub face_count: usize,
    pub index_count: usize,
    planes_at: usize,
    faces_at: usize,
    indices_at: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let w = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([w[0], w[1]]))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let w = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}
fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    u32_at(b, at).map(f32::from_bits)
}
fn round4(n: usize) -> usize {
    n.div_ceil(4).saturating_mul(4)
}

impl<'a> ConvexPolytope<'a> {
    /// Reads object `index` of `c`. `file` and `base` are those passed to
    /// [`Container::read`]; error offsets are absolute file offsets.
    pub fn read(c: &Container<'a>, index: usize, file: &str, base: u64) -> Result<Self> {
        let data_start = c.data().map_or(0, |d| d.start as u64);
        let span = c.object_span(index).ok_or_else(|| {
            Error::new(
                "InvalidHavokObjectSpan",
                file,
                base.saturating_add(data_start),
                format!("object index < {}", c.objects.len()),
                index,
            )
        })?;
        let at0 = base
            .saturating_add(data_start)
            .saturating_add(span.offset as u64);
        let err = |field: usize, expected: String, found: String| {
            Error::new(
                "InvalidHavokConvexPolytope",
                file,
                at0.saturating_add(field as u64),
                expected,
                found,
            )
        };
        if span.class != CONVEX_POLYTOPE {
            return Err(Error::new(
                "UnexpectedHavokClass",
                file,
                at0,
                CONVEX_POLYTOPE,
                span.class,
            ));
        }
        let bytes = c.span_bytes(&span);
        let len = bytes.len();
        if len < CONVEX_POLYTOPE_FIXED {
            return Err(err(
                0,
                format!("object span >= {CONVEX_POLYTOPE_FIXED} bytes"),
                len.to_string(),
            ));
        }
        // Each pair: u16 count, u16 offset relative to the pair's own position.
        let pair = |at: usize| -> (usize, usize) {
            (
                u16_at(bytes, at).unwrap_or(0) as usize,
                at + u16_at(bytes, at + 2).unwrap_or(0) as usize,
            )
        };
        let (vertex_rows, vertices_at) = pair(VERTICES_AT);
        let (plane_rows, planes_at) = pair(PLANES_AT);
        let (face_count, faces_at) = pair(FACES_AT);
        let (index_count, indices_at) = pair(INDICES_AT);
        let vertices_end = vertices_at + vertex_rows * ROW;
        let planes_end = planes_at + plane_rows * ROW;
        // The face array holds one entry per plane row.
        let faces_end = faces_at + plane_rows * FACE;
        let indices_end = indices_at + index_count;
        for (field, found, expected, what) in [
            (
                VERTICES_AT + 2,
                vertices_at,
                CONVEX_POLYTOPE_FIXED,
                "vertex rows right after the fixed bytes",
            ),
            (
                PLANES_AT + 2,
                planes_at,
                vertices_end,
                "plane rows right after the vertex rows",
            ),
            (
                FACES_AT + 2,
                faces_at,
                planes_end,
                "faces right after the plane rows",
            ),
            (
                INDICES_AT + 2,
                indices_at,
                faces_end,
                "indices right after one face entry per plane row",
            ),
        ] {
            if found != expected {
                return Err(err(
                    field,
                    format!("{what} at +{expected}"),
                    found.to_string(),
                ));
            }
        }
        if indices_end.div_ceil(16) * 16 != len {
            return Err(err(
                INDICES_AT,
                format!("indices ending in the last 16 bytes of the {len}-byte span"),
                format!("end at +{indices_end}"),
            ));
        }
        if vertex_rows == 0 || vertex_rows % 4 != 0 || vertex_rows > MAX_VERTEX_ROWS {
            return Err(err(
                VERTICES_AT,
                format!("vertex rows a non-zero multiple of 4, <= {MAX_VERTEX_ROWS}"),
                vertex_rows.to_string(),
            ));
        }
        if plane_rows != round4(face_count) {
            return Err(err(
                PLANES_AT,
                format!("plane rows = face count {face_count} rounded up to 4"),
                plane_rows.to_string(),
            ));
        }
        let row = |j: usize| bytes.get(vertices_at + j * ROW..vertices_at + (j + 1) * ROW);
        let mut vertex_count = vertex_rows;
        for j in 0..vertex_rows {
            let at = vertices_at + j * ROW;
            let tag = u32_at(bytes, at + 12).unwrap_or(0);
            if tag & 0xffff_ff00 != VERTEX_TAG {
                return Err(err(
                    at + 12,
                    format!("vertex tag {VERTEX_TAG:08x} | index"),
                    format!("{tag:08x}"),
                ));
            }
            for k in 0..3 {
                let v = f32_at(bytes, at + 4 * k).unwrap_or(f32::NAN);
                if !v.is_finite() {
                    return Err(err(
                        at + 4 * k,
                        "finite vertex coordinate".into(),
                        v.to_string(),
                    ));
                }
            }
            if vertex_count == vertex_rows && (tag & 0xff) as usize != j {
                vertex_count = j;
            }
        }
        if vertex_count == 0 || round4(vertex_count) != vertex_rows {
            return Err(err(
                vertices_at + 12,
                format!("leading rows tagged 0.. whose count rounds up to {vertex_rows}"),
                vertex_count.to_string(),
            ));
        }
        let last = row(vertex_count - 1);
        for j in vertex_count..vertex_rows {
            if row(j) != last {
                return Err(err(
                    vertices_at + j * ROW,
                    format!("padding row equal to row {}", vertex_count - 1),
                    row(j).map(crate::hex).unwrap_or_default(),
                ));
            }
        }
        let indices = bytes.get(indices_at..indices_end).unwrap_or(&[]);
        if let Some(k) = indices.iter().position(|v| *v as usize >= vertex_count) {
            return Err(err(
                indices_at + k,
                format!("vertex index < {vertex_count}"),
                indices[k].to_string(),
            ));
        }
        Ok(Self {
            object: index,
            offset: span.offset,
            bytes,
            vertex_rows,
            vertex_count,
            plane_rows,
            face_count,
            index_count,
            planes_at,
            faces_at,
            indices_at,
        })
    }
    /// The object's stored bytes, from its offset to the end of its span.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    /// Coordinates of vertex `i` (`i < vertex_count`).
    pub fn vertex(&self, i: usize) -> Option<[f32; 3]> {
        if i >= self.vertex_count {
            return None;
        }
        let at = CONVEX_POLYTOPE_FIXED + i * ROW;
        Some([
            f32_at(self.bytes, at)?,
            f32_at(self.bytes, at + 4)?,
            f32_at(self.bytes, at + 8)?,
        ])
    }
    pub fn vertices(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        (0..self.vertex_count).filter_map(|i| self.vertex(i))
    }
    /// Plane row `j` as four stored f32 (`j < face_count`), unchecked.
    pub fn plane(&self, j: usize) -> Option<[f32; 4]> {
        if j >= self.face_count {
            return None;
        }
        let at = self.planes_at + j * ROW;
        Some([
            f32_at(self.bytes, at)?,
            f32_at(self.bytes, at + 4)?,
            f32_at(self.bytes, at + 8)?,
            f32_at(self.bytes, at + 12)?,
        ])
    }
    /// Face entry `j` as stored (`j < face_count`), unchecked.
    pub fn face(&self, j: usize) -> Option<Face> {
        if j >= self.face_count {
            return None;
        }
        let at = self.faces_at + j * FACE;
        let b = self.bytes.get(at..at + FACE)?;
        Some(Face {
            first: u16::from_le_bytes([b[0], b[1]]),
            count: b[2],
            tag: b[3],
        })
    }
    /// The vertex-index bytes; each is below `vertex_count`.
    pub fn indices(&self) -> &'a [u8] {
        self.bytes
            .get(self.indices_at..self.indices_at + self.index_count)
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests;
