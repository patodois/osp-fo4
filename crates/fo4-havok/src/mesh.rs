//! Typed views of `hknpCompressedMeshShape` and the
//! `hknpCompressedMeshShapeData` object it links to: the array slots of both,
//! the 96-byte sections of the data object, and the vertices and 4-index
//! primitives the sections address.
//!
//! Every rule enforced here held on every stored object of both classes in
//! the measured installation; `spec/havok-mesh.md` gives the counts. Offsets
//! are from the object's offset. Field names are this project's labels for
//! measured roles ("section", "primitive", "shared vertex", "run"); what the
//! game does with them is a hypothesis. Sections whose byte 93 is 1 store
//! some primitives in a form that is not decoded, and byte 92 has values whose
//! meaning is open; [`MeshData::geometry`] leaves such sections out and lists
//! them.
use crate::view::{Array, View};
use crate::Container;
use fo4_core::{Budget, Error, Result};
use serde::Serialize;
use std::ops::Range;

pub const COMPRESSED_MESH: &str = "hknpCompressedMeshShape";
pub const COMPRESSED_MESH_DATA: &str = "hknpCompressedMeshShapeData";
/// Bytes of the shape before its first storage block.
pub const MESH_SHAPE_FIXED: usize = 160;
/// Bytes of the data object before its first storage block.
pub const MESH_DATA_FIXED: usize = 208;
/// Size of one section of the array at +80 of the data object.
pub const SECTION: usize = 96;
/// (slot offset, element size) of the shape's two arrays.
pub const MESH_SHAPE_ARRAYS: [(u32, u32); 2] = [(104, 4), (128, 4)];
/// (slot offset, element size) of the data object's arrays, in the order
/// their storage follows each other (the section trees come right after the
/// sections).
pub const MESH_DATA_ARRAYS: [(u32, u32); 8] = [
    (16, 5),
    (80, 96),
    (96, 4),
    (112, 2),
    (128, 4),
    (144, 8),
    (160, 4),
    (184, 112),
];
/// Element size of the array whose slot opens each section.
pub const SECTION_TREE_NODE: u32 = 4;
/// A primitive stored as these four bytes addresses no vertex.
pub const PLACEHOLDER_PRIMITIVE: [u8; 4] = [0xde, 0xad, 0xde, 0xad];
/// Decoded vertices lie inside their section's box within this fraction of
/// max(1, largest stored box coordinate). The largest measured excess is 6e-7.
pub const MESH_BOX_TOLERANCE: f64 = 1e-5;
/// Largest index a section's vertices can take (u8 primitive indices).
pub const SECTION_VERTICES: usize = 256;

const TREE: usize = 0;
const SECTIONS: usize = 1;
const PRIMITIVES: usize = 2;
const SHARED_INDICES: usize = 3;
const VERTICES: usize = 4;
const SHARED_VERTICES: usize = 5;
const RUNS: usize = 6;
const TAIL: usize = 7;
/// Number of elements of the array at +184 in every measured object.
const TAIL_COUNT: u32 = 2;
/// Quantization steps of a shared vertex (21, 21 and 22 bits).
const SHARED_STEPS: [u64; 3] = [(1 << 21) - 1, (1 << 21) - 1, (1 << 22) - 1];

fn round16(n: usize) -> usize {
    n.div_ceil(16).saturating_mul(16)
}

/// `hknpCompressedMeshShape`: a link at 32 to an `hkRefCountedProperties`
/// object, a link at 96 to its `hknpCompressedMeshShapeData` object, and two
/// arrays of u32 words at 104 and 128 whose lengths are set by the u32 at 120
/// and 144 (one bit per slot, rounded up to whole words).
#[derive(Debug, Clone, Serialize)]
pub struct CompressedMesh<'a> {
    pub object: usize,
    /// Third-table index of the object linked from byte 32.
    pub properties: usize,
    /// Third-table index of the `hknpCompressedMeshShapeData` linked from byte 96.
    pub data: usize,
    /// Byte 18: equals the u32 at +68 of the data object.
    pub key_bits: u8,
    /// u32 at 120: bits of the array at 104.
    pub primitive_slots: u32,
    /// u32 at 144: bits of the array at 128.
    pub key_slots: u32,
    pub arrays: [Array; 2],
    #[serde(skip)]
    bytes: &'a [u8],
}

impl<'a> CompressedMesh<'a> {
    pub fn read(c: &Container<'a>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[COMPRESSED_MESH],
            "InvalidHavokCompressedMesh",
        )?;
        let properties = v.required_link(32, "properties")?;
        let data = v.required_link(96, "mesh data")?;
        let class = c.objects.get(data).map_or("", |o| o.class);
        if class != COMPRESSED_MESH_DATA {
            return Err(v.err(96, format!("a link to {COMPRESSED_MESH_DATA}"), class));
        }
        let mut arrays = [Array {
            field: 0,
            count: 0,
            storage: 0,
            element: 0,
        }; 2];
        let mut at = MESH_SHAPE_FIXED;
        for (slot, (field, element)) in arrays.iter_mut().zip(MESH_SHAPE_ARRAYS) {
            let a = v.array(field as usize, element)?;
            let slots = v.u32(field as usize + 16)?;
            if a.count != slots.div_ceil(32) || a.count == 0 {
                return Err(v.err(
                    field as usize + 8,
                    format!(
                        "{} words for the {slots} bits of the u32 at +{}",
                        slots.div_ceil(32),
                        field + 16
                    ),
                    a.count,
                ));
            }
            if a.storage as usize != at {
                return Err(v.err(field as usize, format!("storage at +{at}"), a.storage));
            }
            at = round16(at + (a.count * element) as usize);
            *slot = a;
        }
        v.span_len(at)?;
        let primitive_slots = v.u32(120)?;
        let key_slots = v.u32(144)?;
        let key_bits = v.bytes.get(18).copied().unwrap_or(0);
        Ok(Self {
            object: index,
            properties,
            data,
            key_bits,
            primitive_slots,
            key_slots,
            arrays,
            bytes: v.bytes,
        })
    }
    /// The object's stored bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    /// Bit `k` of the array at 104 (`which` = 0) or 128 (`which` = 1), for
    /// `k` below the u32 at 120 or 144. Each set bit falls on a stored
    /// primitive or triangle slot; what a bit means is open.
    pub fn bit(&self, which: usize, k: u32) -> Option<bool> {
        let a = self.arrays.get(which)?;
        let limit = if which == 0 {
            self.primitive_slots
        } else {
            self.key_slots
        };
        if k >= limit {
            return None;
        }
        let at = a.element_at((k / 32) as usize)?;
        let w = self.bytes.get(at..at + 4)?;
        let w = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        Some(w >> (k % 32) & 1 == 1)
    }
}

/// One 96-byte section of the data object.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeshSection {
    pub index: usize,
    /// Offset of the section from the data object's offset.
    pub offset: u32,
    /// The array slot at section byte 0: 2 × primitives − 1 nodes of 4 bytes,
    /// kept opaque.
    pub tree: Array,
    /// Section bytes 16 and 32: three f32 each, the box of its vertices.
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// The fourth f32 at bytes 28 and 44, kept as stored; meaning open.
    pub lanes: [f32; 2],
    /// Bytes 48 and 60: origin and step of the section's vertices, read
    /// without a check when the section has none.
    pub origin: [f32; 3],
    pub scale: [f32; 3],
    /// Elements of the u32 vertex array (+128) owned by the section.
    pub vertices: Range<u32>,
    /// Elements of the u16 shared-index array (+112) owned by the section.
    pub shared: Range<u32>,
    /// Elements of the primitive array (+96) owned by the section.
    pub primitives: Range<u32>,
    /// Elements of the run array (+160) used by the section.
    pub runs: Range<u32>,
    /// u16 at byte 90; meaning open.
    pub word90: u16,
    /// Byte 93 is 1: some primitives are stored in a form not decoded here.
    pub flagged: bool,
    /// Byte 92: 0 to 4 in the measured sections; meaning open. Only
    /// sections with 0 are decoded.
    pub byte92: u8,
}
impl MeshSection {
    /// Vertex indices a primitive of this section can use.
    pub fn index_limit(&self) -> u32 {
        self.vertices.len() as u32 + self.shared.len() as u32
    }
}

/// One stored primitive with its indices into [`Geometry::vertices`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Primitive {
    pub section: u32,
    /// Low u16 of the run that covers the primitive; below the length of the
    /// material array linked from the shape in every measured run.
    pub run_value: u16,
    /// The four stored bytes, each resolved to a vertex. The third and fourth
    /// are equal in about one measured primitive in six; how the four form
    /// triangles is not established (`spec/havok-mesh.md`).
    pub indices: [u32; 4],
}

/// Decoded vertices and primitives of the sections whose bytes 92 and 93 are
/// zero.
#[derive(Debug, Clone, Serialize)]
pub struct Geometry {
    /// The section vertices of each decoded section in stored order, each
    /// section's own vertices followed by the shared vertices it is the first
    /// to reference.
    pub vertices: Vec<[f32; 3]>,
    pub primitives: Vec<Primitive>,
    /// Primitives stored as [`PLACEHOLDER_PRIMITIVE`], left out of `primitives`.
    pub placeholders: usize,
    /// Sections left out because byte 92 or 93 is not zero.
    pub skipped: Vec<u32>,
}
impl Geometry {
    /// Every section was decoded.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
}

/// `hknpCompressedMeshShapeData`: the eight array slots of
/// [`MESH_DATA_ARRAYS`], the box at 32 and 48, and the sections.
#[derive(Debug, Clone, Serialize)]
pub struct MeshData<'a> {
    pub object: usize,
    pub arrays: Vec<Array>,
    /// Bytes 32 and 48: the box of all sections (their exact union).
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// The fourth f32 at bytes 44 and 60, the union of the sections' lanes.
    pub lanes: [f32; 2],
    /// u32 at 64, 68 and 72, kept as stored (see `spec/havok-mesh.md`).
    pub counts: [u32; 3],
    pub sections: Vec<MeshSection>,
    #[serde(skip)]
    bytes: &'a [u8],
    #[serde(skip)]
    file: String,
    #[serde(skip)]
    at0: u64,
}

fn inside(lo: f32, hi: f32, x: f32, tolerance: f64) -> bool {
    f64::from(x) >= f64::from(lo) - tolerance && f64::from(x) <= f64::from(hi) + tolerance
}

impl<'a> MeshData<'a> {
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
            &[COMPRESSED_MESH_DATA],
            "InvalidHavokCompressedMeshData",
        )?;
        let mut arrays = Vec::new();
        b.reserve(&mut arrays, MESH_DATA_ARRAYS.len(), file, v.abs(16))?;
        for (field, element) in MESH_DATA_ARRAYS {
            arrays.push(v.array(field as usize, element)?);
        }
        let n = arrays[SECTIONS].count;
        if n == 0 {
            return Err(v.err(88, "at least one section", 0));
        }
        let nodes = 2 * u64::from(n) - 1;
        if u64::from(arrays[TREE].count) != nodes {
            return Err(v.err(
                24,
                format!("{nodes} tree nodes for {n} sections"),
                arrays[TREE].count,
            ));
        }
        if arrays[TAIL].count != TAIL_COUNT {
            return Err(v.err(192, format!("{TAIL_COUNT} elements"), arrays[TAIL].count));
        }
        let min = v.finite::<3>(32, "box minimum")?;
        let max = v.finite::<3>(48, "box maximum")?;
        if let Some(k) = (0..3).find(|k| min[*k] > max[*k]) {
            return Err(v.err(48 + 4 * k, format!("maximum >= {}", min[k]), max[k]));
        }
        let lanes = [f32::from_bits(v.u32(44)?), f32::from_bits(v.u32(60)?)];
        let counts = [v.u32(64)?, v.u32(68)?, v.u32(72)?];
        let mut sections = Vec::new();
        b.reserve(&mut sections, n as usize, file, v.abs(88))?;
        let mut union = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for j in 0..n as usize {
            let e = arrays[SECTIONS]
                .element_at(j)
                .ok_or_else(|| v.err(88, "section inside the array", j))?;
            let s = section(&v, &arrays, j, e)?;
            for k in 0..3 {
                if s.min[k] < min[k] || s.max[k] > max[k] {
                    return Err(v.err(
                        e + 16 + 4 * k,
                        format!("a section box inside [{}, {}]", min[k], max[k]),
                        format!("[{}, {}]", s.min[k], s.max[k]),
                    ));
                }
                union.0[k] = union.0[k].min(s.min[k]);
                union.1[k] = union.1[k].max(s.max[k]);
            }
            sections.push(s);
        }
        if union != (min, max) {
            return Err(v.err(32, "the union of the section boxes", format!("{union:?}")));
        }
        // The storage blocks follow each other in slot order, the section
        // trees right after the sections, each starting on a 16-byte boundary.
        let mut blocks = Vec::new();
        b.reserve(&mut blocks, arrays.len() + sections.len(), file, v.abs(0))?;
        for (k, a) in arrays.iter().enumerate() {
            blocks.push(*a);
            if k == SECTIONS {
                blocks.extend(sections.iter().map(|s| s.tree));
            }
        }
        let mut at = MESH_DATA_FIXED;
        for a in blocks.iter().filter(|a| a.count > 0) {
            if a.storage as usize != at {
                return Err(v.err(a.field as usize, format!("storage at +{at}"), a.storage));
            }
            at = round16(at + (a.count as usize) * (a.element as usize));
        }
        v.span_len(at)?;
        Ok(Self {
            object: index,
            arrays,
            min,
            max,
            lanes,
            counts,
            sections,
            bytes: v.bytes,
            file: file.to_owned(),
            at0: v.abs(0),
        })
    }
    /// The object's stored bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
    /// The checked slot at `field`.
    pub fn array(&self, field: u32) -> Option<&Array> {
        self.arrays.iter().find(|a| a.field == field)
    }
    fn err(
        &self,
        code: &str,
        field: usize,
        expected: impl ToString,
        found: impl ToString,
    ) -> Error {
        Error::new(
            code,
            &self.file,
            self.at0.saturating_add(field as u64),
            expected,
            found,
        )
    }
    fn word(&self, at: usize) -> Option<u32> {
        let w = self.bytes.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    }
    /// Section vertex `i` of the array at +128 (11, 11 and 10 bits from its
    /// low bit, times the section's step, plus its origin). `None` outside
    /// the array or the section.
    pub fn section_vertex(&self, s: &MeshSection, i: u32) -> Option<[f32; 3]> {
        if !s.vertices.contains(&i) {
            return None;
        }
        let w = self.word(self.arrays[VERTICES].element_at(i as usize)?)?;
        let q = [w & 0x7ff, (w >> 11) & 0x7ff, w >> 22];
        Some(std::array::from_fn(|k| {
            s.origin[k] + q[k] as f32 * s.scale[k]
        }))
    }
    /// Shared vertex `t` of the array at +144: 21, 21 and 22 bits from its
    /// low bit, each the fraction of its maximum along the object's box.
    pub fn shared_vertex(&self, t: u32) -> Option<[f32; 3]> {
        let at = self.arrays[SHARED_VERTICES].element_at(t as usize)?;
        let w = u64::from(self.word(at)?) | u64::from(self.word(at + 4)?) << 32;
        let q = [w & 0x1f_ffff, (w >> 21) & 0x1f_ffff, w >> 42];
        Some(std::array::from_fn(|k| {
            let (lo, hi) = (f64::from(self.min[k]), f64::from(self.max[k]));
            (lo + q[k] as f64 * (hi - lo) / SHARED_STEPS[k] as f64) as f32
        }))
    }
    /// The u16 of the shared-index array at element `j`.
    pub fn shared_index(&self, j: u32) -> Option<u16> {
        let at = self.arrays[SHARED_INDICES].element_at(j as usize)?;
        let w = self.bytes.get(at..at + 2)?;
        Some(u16::from_le_bytes([w[0], w[1]]))
    }
    /// The four stored bytes of primitive `j` of the array at +96.
    pub fn primitive(&self, j: u32) -> Option<[u8; 4]> {
        let at = self.arrays[PRIMITIVES].element_at(j as usize)?;
        let w = self.bytes.get(at..at + 4)?;
        Some([w[0], w[1], w[2], w[3]])
    }
    /// Run `j` of the array at +160: (low u16, first primitive, count).
    pub fn run(&self, j: u32) -> Option<(u16, u8, u8)> {
        let w = self.word(self.arrays[RUNS].element_at(j as usize)?)?;
        Some((w as u16, (w >> 16) as u8, (w >> 24) as u8))
    }
    /// Decodes every section whose bytes 92 and 93 are zero and lists the
    /// others in [`Geometry::skipped`]. A vertex outside its section's box, a
    /// shared index past the shared vertices or a primitive index at or past
    /// the section's vertices is `InvalidHavokCompressedMeshData`.
    pub fn geometry(&self, b: &Budget) -> Result<Geometry> {
        const CODE: &str = "InvalidHavokCompressedMeshData";
        let shared = self.arrays[SHARED_VERTICES].count as usize;
        let mut out = Geometry {
            vertices: Vec::new(),
            primitives: Vec::new(),
            placeholders: 0,
            skipped: Vec::new(),
        };
        // Position in `vertices` of each shared vertex once referenced.
        let mut placed = Vec::new();
        b.reserve(&mut placed, shared, &self.file, self.at0)?;
        placed.resize(shared, u32::MAX);
        for (si, s) in self.sections.iter().enumerate() {
            if s.flagged || s.byte92 != 0 {
                b.reserve(&mut out.skipped, 1, &self.file, self.at0)?;
                out.skipped.push(si as u32);
                continue;
            }
            let at = self.at0.saturating_add(u64::from(s.offset));
            b.reserve(&mut out.vertices, s.index_limit() as usize, &self.file, at)?;
            b.reserve(&mut out.primitives, s.primitives.len(), &self.file, at)?;
            self.section_geometry(si, s, &mut out, &mut placed)
                .map_err(|(at, expected, found)| self.err(CODE, at, expected, found))?;
        }
        Ok(out)
    }

    fn section_geometry(
        &self,
        si: usize,
        s: &MeshSection,
        out: &mut Geometry,
        placed: &mut [u32],
    ) -> std::result::Result<(), (usize, String, String)> {
        let m = s
            .min
            .iter()
            .chain(&s.max)
            .fold(1f64, |m, x| m.max(f64::from(x.abs())));
        let t = m * MESH_BOX_TOLERANCE;
        let outside = |p: [f32; 3]| (0..3).find(|k| !inside(s.min[*k], s.max[*k], p[*k], t));
        let mut map = [0u32; SECTION_VERTICES];
        for (slot, i) in map.iter_mut().zip(s.vertices.clone()) {
            let at = self.arrays[VERTICES].element_at(i as usize).unwrap_or(0);
            let p = self.section_vertex(s, i).ok_or((
                at,
                "a vertex inside the array".to_owned(),
                i.to_string(),
            ))?;
            if let Some(k) = outside(p) {
                return Err((
                    at,
                    format!(
                        "a vertex inside the section box [{}, {}]",
                        s.min[k], s.max[k]
                    ),
                    p[k].to_string(),
                ));
            }
            *slot = out.vertices.len() as u32;
            out.vertices.push(p);
        }
        let nv = s.vertices.len();
        let shared = placed.len();
        for (k, j) in s.shared.clone().enumerate() {
            let at = self.arrays[SHARED_INDICES]
                .element_at(j as usize)
                .unwrap_or(0);
            let target = self.shared_index(j).unwrap_or(u16::MAX) as usize;
            let p = self
                .shared_vertex(target as u32)
                .filter(|_| target < shared)
                .ok_or((
                    at,
                    format!("a shared vertex index < {shared}"),
                    target.to_string(),
                ))?;
            if let Some(a) = outside(p) {
                return Err((
                    at,
                    format!(
                        "a shared vertex inside the section box [{}, {}]",
                        s.min[a], s.max[a]
                    ),
                    p[a].to_string(),
                ));
            }
            let Some(at_list) = placed.get_mut(target) else {
                continue;
            };
            if *at_list == u32::MAX {
                *at_list = out.vertices.len() as u32;
                out.vertices.push(p);
            }
            if let Some(slot) = map.get_mut(nv + k) {
                *slot = *at_list;
            }
        }
        let limit = s.index_limit();
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
            if let Some(x) = raw.iter().position(|x| u32::from(*x) >= limit) {
                let at = self.arrays[PRIMITIVES].element_at(j as usize).unwrap_or(0);
                return Err((
                    at + x,
                    format!("a vertex index < {limit}"),
                    raw[x].to_string(),
                ));
            }
            out.primitives.push(Primitive {
                section: si as u32,
                run_value: value,
                indices: raw.map(|x| map[x as usize]),
            });
        }
        Ok(())
    }
}

/// Reads and checks section `j` at data-object offset `e`.
fn section(v: &View<'_, '_>, arrays: &[Array], j: usize, e: usize) -> Result<MeshSection> {
    let tree = v.array(e, SECTION_TREE_NODE)?;
    let min = v.finite::<3>(e + 16, "section box minimum")?;
    let max = v.finite::<3>(e + 32, "section box maximum")?;
    if let Some(k) = (0..3).find(|k| min[*k] > max[*k]) {
        return Err(v.err(e + 32 + 4 * k, format!("maximum >= {}", min[k]), max[k]));
    }
    let lanes = [
        f32::from_bits(v.u32(e + 28)?),
        f32::from_bits(v.u32(e + 44)?),
    ];
    let f = |at: usize| v.u32(e + at).map(f32::from_bits);
    let origin = [f(48)?, f(52)?, f(56)?];
    let scale = [f(60)?, f(64)?, f(68)?];
    let w72 = v.u32(e + 72)?;
    let w76 = v.u32(e + 76)?;
    let w80 = v.u32(e + 80)?;
    let w84 = v.u32(e + 84)?;
    let w88 = v.u32(e + 88)?;
    let w92 = v.u32(e + 92)?;
    let vc = w76 & 0xff;
    let shc = (w88 >> 8) & 0xff;
    let range = |first: u32, count: u32| first..first.saturating_add(count);
    let s = MeshSection {
        index: j,
        offset: e as u32,
        tree,
        min,
        max,
        lanes,
        origin,
        scale,
        vertices: range(w72, vc),
        shared: range(w76 >> 8, shc),
        primitives: range(w80 >> 8, w80 & 0xff),
        runs: range(w84 >> 8, w84 & 0xff),
        word90: (w88 >> 16) as u16,
        flagged: w92 & 0x100 != 0,
        byte92: w92 as u8,
    };
    if w88 & 0xff != vc {
        return Err(v.err(e + 88, format!("low byte {vc}, as byte 76"), w88 & 0xff));
    }
    if w92 & 0xffff_fe00 != 0 {
        return Err(v.err(
            e + 92,
            "byte 93 0 or 1, bytes 94 and 95 zero",
            format!("{w92:08x}"),
        ));
    }
    let pc = s.primitives.len() as u32;
    if pc == 0 {
        return Err(v.err(e + 80, "at least one primitive", 0));
    }
    if tree.count != 2 * pc - 1 {
        return Err(v.err(
            e + 8,
            format!("{} tree nodes for {pc} primitives", 2 * pc - 1),
            tree.count,
        ));
    }
    if s.index_limit() as usize > SECTION_VERTICES {
        return Err(v.err(
            e + 88,
            format!("at most {SECTION_VERTICES} vertices"),
            s.index_limit(),
        ));
    }
    for (field, r, k) in [
        (72, &s.vertices, VERTICES),
        (76, &s.shared, SHARED_INDICES),
        (80, &s.primitives, PRIMITIVES),
        (84, &s.runs, RUNS),
    ] {
        if r.end > arrays[k].count {
            return Err(v.err(
                e + field,
                format!("elements below {}", arrays[k].count),
                format!("{r:?}"),
            ));
        }
    }
    if !s.vertices.is_empty() {
        let bad = |k: &usize| !origin[*k].is_finite() || !scale[*k].is_finite() || scale[*k] < 0.0;
        if let Some(k) = (0..3).find(bad) {
            return Err(v.err(
                e + 48 + 4 * k,
                "finite origin and finite step >= 0",
                format!("{} {}", origin[k], scale[k]),
            ));
        }
    }
    // The runs cover the primitives in order, from 0 to the primitive count.
    if s.runs.is_empty() {
        return Err(v.err(e + 84, "at least one run", 0));
    }
    let mut next = 0u32;
    for r in s.runs.clone() {
        let at = arrays[RUNS].element_at(r as usize).unwrap_or(0);
        let w = v.u32(at)?;
        let (first, count) = ((w >> 16) & 0xff, w >> 24);
        if first != next {
            return Err(v.err(at + 2, format!("run starting at primitive {next}"), first));
        }
        next += count;
    }
    if next != pc {
        return Err(v.err(e + 84, format!("runs covering {pc} primitives"), next));
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
