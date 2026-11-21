#![forbid(unsafe_code)]
//! Container level of the Havok payload that Fallout 4 stores in the
//! `bhkPhysicsSystem` and `bhkRagdollSystem` ByteArrays.
//!
//! Every rule this reader enforces was measured over all the payloads of one
//! installation; `spec/havok-container.md` tags each one with its count. No
//! Havok SDK, header, converter or third-party reader was consulted. The reader
//! splits a payload into its three named sections, lists the class-name
//! entries and reads the three tables at the end of the data section. Each
//! object owns the bytes up to the next object (`spec/havok-objects.md`), and
//! every first-table pointer stays inside the object that holds its source.
//! Typed object views: [`ConvexPolytope`], the top-level [`PhysicsSystem`]
//! with its bodies (shape link and placement), and the simple shapes
//! [`Sphere`], [`Capsule`], [`ConvexShape`], [`ScaledConvex`] and
//! [`Compound`] (`spec/havok-bodies.md`), and the compressed mesh
//! [`CompressedMesh`] with its [`MeshData`] (`spec/havok-mesh.md`). A payload
//! outside the measured profile is a named error carrying the file offset;
//! nothing here panics.
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

mod bodies;
mod mesh;
mod objects;
mod shapes;
mod view;
pub use bodies::{
    Body, PhysicsSystem, BODY, PHYSICS_SYSTEM, RAGDOLL, RAGDOLL_ARRAY, SYSTEM_ARRAYS,
    UNIT_TOLERANCE,
};
pub use mesh::{
    CompressedMesh, Geometry, MeshData, MeshSection, Primitive, COMPRESSED_MESH,
    COMPRESSED_MESH_DATA, MESH_BOX_TOLERANCE, MESH_DATA_ARRAYS, MESH_DATA_FIXED, MESH_SHAPE_ARRAYS,
    MESH_SHAPE_FIXED, PLACEHOLDER_PRIMITIVE, SECTION, SECTION_TREE_NODE, SECTION_VERTICES,
};
pub use objects::{
    ConvexPolytope, Face, ObjectSpan, CONVEX_POLYTOPE, CONVEX_POLYTOPE_FIXED, MAX_VERTEX_ROWS,
    VERTEX_TAG,
};
pub use shapes::{
    orthonormal_error, segment_distance, Capsule, Compound, ConvexShape, Instance, ScaledConvex,
    Sphere, CAPSULE, CAPSULE_PAIRS, CAPSULE_SPAN, COMPOUND_FIXED, CONVEX, DYNAMIC_COMPOUND,
    INSTANCE, ORTHONORMAL_TOLERANCE, SCALED_CONVEX, SCALED_CONVEX_SPAN, SPHERE, SPHERE_SPAN,
    STATIC_COMPOUND,
};
pub use view::{Array, ARRAY_FLAG};

/// Bytes before the first section record.
pub const HEADER_LEN: usize = 64;
/// Size of one section record.
pub const RECORD_LEN: usize = 64;
/// The first eight bytes of every measured payload.
pub const MAGIC: [u8; 8] = [0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10];
/// Bytes 16..20 of every measured payload. Byte 16 equals the measured
/// pointer slot size ([`POINTER_SLOT`]); the others are constant, unexplained.
pub const LAYOUT: [u8; 4] = [8, 1, 0, 1];
/// The NUL-terminated text at byte 40 of every measured payload.
pub const VERSION: &str = "hk_2014.1.0-r1";
/// Section record names, in stored order.
pub const SECTION_NAMES: [&str; 3] = ["__classnames__", "__types__", "__data__"];
pub const CLASS_NAMES: usize = 0;
pub const TYPES: usize = 1;
pub const DATA: usize = 2;
/// The byte between an entry's 4-byte signature and its name.
pub const CLASS_NAME_MARKER: u8 = 9;
/// Bytes at every table source that are zero in the stored data.
pub const POINTER_SLOT: u64 = 8;
const FILL: u8 = 0xff;
const MAX_LEN: usize = u32::MAX as usize;
const VERSION_AT: usize = 40;
const VERSION_END: usize = 56;
const RECORD_NAME_LEN: usize = 20;

/// The first 64 bytes as sixteen little-endian u32 words, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Header {
    pub words: [u32; 16],
}
impl Header {
    /// Word at byte 20: the number of section records (3 in every payload).
    pub fn section_count(&self) -> u32 {
        self.words[5]
    }
    /// Word at byte 24: index of the section holding the first object.
    pub fn root_section(&self) -> u32 {
        self.words[6]
    }
    /// Word at byte 28: offset of the first object in that section.
    pub fn root_offset(&self) -> u32 {
        self.words[7]
    }
    /// Word at byte 32: index of the section holding its class name.
    pub fn root_class_section(&self) -> u32 {
        self.words[8]
    }
    /// Word at byte 36: offset of that class name in its section.
    pub fn root_class_offset(&self) -> u32 {
        self.words[9]
    }
}

/// One 64-byte section record and the bounds it declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section<'a> {
    pub index: usize,
    /// Payload offset of the record.
    pub record: u32,
    pub name: &'a str,
    /// Record word 20: payload offset of the section's first byte.
    pub start: u32,
    /// Record words 24 to 44, relative to `start`, non-decreasing. The last
    /// one is the section length.
    pub offsets: [u32; 6],
}
impl Section<'_> {
    pub fn len(&self) -> u32 {
        self.offsets[5]
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Payload offset one past the last byte. Checked when the container is read.
    pub fn end(&self) -> u32 {
        self.start.saturating_add(self.offsets[5])
    }
}

/// One entry of `__classnames__`: u32 signature, marker byte, NUL-terminated name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassName<'a> {
    /// Offset of the first character of the name, relative to the section start.
    /// The tables and the header refer to names by this offset.
    pub offset: u32,
    pub signature: u32,
    pub marker: u8,
    pub name: &'a str,
}

/// Entry of the first data table: two u32 offsets inside the object bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Pointer {
    pub source: u32,
    pub destination: u32,
}

/// Entry of the second data table: a source in the object bytes, a section
/// index (always `__data__`) and the offset of an object listed in the third table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObjectPointer {
    pub source: u32,
    pub section: u32,
    pub destination: u32,
}

/// Entry of the third data table: where an object starts and its class name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Object<'a> {
    pub offset: u32,
    /// Section of the class name (always `__classnames__`).
    pub section: u32,
    pub class_offset: u32,
    pub class: &'a str,
}

#[derive(Debug, Serialize)]
pub struct Container<'a> {
    #[serde(skip)]
    pub bytes: &'a [u8],
    pub header: Header,
    pub version: &'a str,
    pub sections: Vec<Section<'a>>,
    pub class_names: Vec<ClassName<'a>>,
    pub pointers: Vec<Pointer>,
    pub object_pointers: Vec<ObjectPointer>,
    pub objects: Vec<Object<'a>>,
    /// Bytes of 0xff after the last entry of `__classnames__` and of each data table.
    pub fill: [u32; 4],
}

struct Ctx<'a, 'b> {
    bytes: &'a [u8],
    file: &'b str,
    base: u64,
    b: &'b Budget,
}
impl<'a> Ctx<'a, '_> {
    fn err(&self, code: &str, at: usize, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(
            code,
            self.file,
            self.base.saturating_add(at as u64),
            expected,
            found,
        )
    }
    fn slice(&self, at: usize, len: usize, what: &str) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.bytes.get(at..end))
            .ok_or_else(|| {
                self.err(
                    "TruncatedHavokContainer",
                    at,
                    format!("{len} bytes of {what}"),
                    format!("payload of {} bytes", self.bytes.len()),
                )
            })
    }
    fn u32(&self, at: usize, what: &str) -> Result<u32> {
        let s = self.slice(at, 4, what)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    /// NUL-terminated printable ASCII inside `[at, end)`.
    fn text(&self, at: usize, end: usize, code: &str, what: &str) -> Result<(&'a str, usize)> {
        let window = self.bytes.get(at..end.min(self.bytes.len())).unwrap_or(&[]);
        let nul = window.iter().position(|c| *c == 0).ok_or_else(|| {
            self.err(
                code,
                at,
                format!("NUL-terminated {what} before {end}"),
                "no NUL",
            )
        })?;
        let text = &window[..nul];
        if text.is_empty() || !text.iter().all(|c| (0x20..0x7f).contains(c)) {
            return Err(self.err(code, at, format!("printable ASCII {what}"), hex(text)));
        }
        // Printable ASCII is valid UTF-8.
        let text = std::str::from_utf8(text)
            .map_err(|_| self.err(code, at, format!("ASCII {what}"), hex(text)))?;
        Ok((text, at + nul))
    }
    fn push<T>(&self, v: &mut Vec<T>, item: T, at: usize) -> Result<()> {
        self.b
            .reserve(v, 1, self.file, self.base.saturating_add(at as u64))?;
        v.push(item);
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().take(32).map(|b| format!("{b:02x}")).collect()
}

impl<'a> Container<'a> {
    /// Reads the container level of one payload.
    ///
    /// `file` names the NIF and `base` is the file offset of the payload's first
    /// byte (`fo4_nif::System::payload_offset`); error offsets are `base` plus
    /// the payload offset of the field at fault.
    pub fn read(bytes: &'a [u8], file: &str, base: u64, b: &Budget) -> Result<Self> {
        let c = Ctx {
            bytes,
            file,
            base,
            b,
        };
        if bytes.len() > MAX_LEN {
            return Err(c.err(
                "LimitExceeded",
                0,
                format!("payload <= {MAX_LEN} bytes"),
                bytes.len(),
            ));
        }
        let head = c.slice(0, HEADER_LEN, "header")?;
        if head[..8] != MAGIC {
            return Err(c.err("InvalidHavokMagic", 0, hex(&MAGIC), hex(&head[..8])));
        }
        if head[16..20] != LAYOUT {
            return Err(c.err(
                "UnsupportedHavokLayout",
                16,
                hex(&LAYOUT),
                hex(&head[16..20]),
            ));
        }
        let mut words = [0u32; 16];
        for (i, w) in words.iter_mut().enumerate() {
            *w = c.u32(i * 4, "header")?;
        }
        let header = Header { words };
        let (version, _) = c.text(
            VERSION_AT,
            VERSION_END,
            "UnsupportedHavokVersion",
            "version",
        )?;
        if version != VERSION {
            return Err(c.err("UnsupportedHavokVersion", VERSION_AT, VERSION, version));
        }
        if header.section_count() as usize != SECTION_NAMES.len() {
            return Err(c.err(
                "UnsupportedHavokSectionCount",
                20,
                SECTION_NAMES.len(),
                header.section_count(),
            ));
        }
        let mut sections: Vec<Section<'a>> = Vec::new();
        for (index, expected) in SECTION_NAMES.iter().enumerate() {
            let record = HEADER_LEN + index * RECORD_LEN;
            c.slice(record, RECORD_LEN, "section record")?;
            let (name, _) = c.text(
                record,
                record + RECORD_NAME_LEN,
                "InvalidHavokSectionRecord",
                "section name",
            )?;
            if name != *expected {
                return Err(c.err("InvalidHavokSectionRecord", record, expected, name));
            }
            let start = c.u32(record + 20, "section start")?;
            let mut offsets = [0u32; 6];
            for (j, o) in offsets.iter_mut().enumerate() {
                *o = c.u32(record + 24 + 4 * j, "section offset")?;
            }
            for j in 1..6 {
                if offsets[j] < offsets[j - 1] {
                    return Err(c.err(
                        "InvalidHavokSectionBounds",
                        record + 24 + 4 * j,
                        format!(">= {} (non-decreasing offsets)", offsets[j - 1]),
                        offsets[j],
                    ));
                }
            }
            let expected_start = match sections.last() {
                None => HEADER_LEN + SECTION_NAMES.len() * RECORD_LEN,
                Some(prev) => prev.end() as usize,
            };
            if start as usize != expected_start {
                return Err(c.err(
                    "InvalidHavokSectionBounds",
                    record + 20,
                    format!("section start {expected_start} (end of the previous one)"),
                    start,
                ));
            }
            let end = start as u64 + offsets[5] as u64;
            if end > bytes.len() as u64 {
                return Err(c.err(
                    "InvalidHavokSectionBounds",
                    record + 44,
                    format!("section end <= {}", bytes.len()),
                    end,
                ));
            }
            c.push(
                &mut sections,
                Section {
                    index,
                    record: record as u32,
                    name,
                    start,
                    offsets,
                },
                record,
            )?;
        }
        let last = sections[DATA].end() as usize;
        if last != bytes.len() {
            return Err(c.err(
                "InvalidHavokSectionBounds",
                HEADER_LEN + DATA * RECORD_LEN + 44,
                format!("last section ends at the payload length {}", bytes.len()),
                last,
            ));
        }
        let names = &sections[CLASS_NAMES];
        if names.offsets.iter().any(|o| *o != names.len()) {
            return Err(c.err(
                "UnsupportedHavokSectionLayout",
                names.record as usize + 24,
                "__classnames__ offsets all equal to its length",
                format!("{:?}", names.offsets),
            ));
        }
        let types = &sections[TYPES];
        if types.offsets.iter().any(|o| *o != 0) {
            return Err(c.err(
                "UnsupportedHavokSectionLayout",
                types.record as usize + 24,
                "empty __types__ (all offsets zero)",
                format!("{:?}", types.offsets),
            ));
        }
        let data = &sections[DATA];
        if data.offsets[3] != data.offsets[5] || data.offsets[4] != data.offsets[5] {
            return Err(c.err(
                "UnsupportedHavokSectionLayout",
                data.record as usize + 36,
                "__data__ words 36, 40 and 44 equal (no fourth or fifth table)",
                format!("{:?}", data.offsets),
            ));
        }
        let (class_names, names_fill) =
            class_names(&c, names.start as usize, names.end() as usize)?;
        let d = data.start as usize;
        let o = data.offsets.map(|x| x as usize);
        let objects_end = o[0] as u64;
        let (raw_pointers, f1) = table::<2>(&c, d + o[0], d + o[1], "pointer")?;
        let (raw_objects, f3) = table::<3>(&c, d + o[2], d + o[3], "object")?;
        let (raw_links, f2) = table::<3>(&c, d + o[1], d + o[2], "object pointer")?;
        let slot = |source: u32, at: usize| -> Result<()> {
            if source as u64 + POINTER_SLOT > objects_end {
                return Err(c.err(
                    "InvalidHavokFixup",
                    at,
                    format!("source + {POINTER_SLOT} <= {objects_end}"),
                    source,
                ));
            }
            Ok(())
        };
        let mut pointers = Vec::new();
        for (i, [source, destination]) in raw_pointers.into_iter().enumerate() {
            let at = d + o[0] + i * 8;
            slot(source, at)?;
            if destination as u64 >= objects_end {
                return Err(c.err(
                    "InvalidHavokFixup",
                    at + 4,
                    format!("destination < {objects_end}"),
                    destination,
                ));
            }
            c.push(
                &mut pointers,
                Pointer {
                    source,
                    destination,
                },
                at,
            )?;
        }
        let mut objects: Vec<Object<'a>> = Vec::new();
        for (i, [offset, section, class_offset]) in raw_objects.into_iter().enumerate() {
            let at = d + o[2] + i * 12;
            slot(offset, at)?;
            if let Some(prev) = objects.last() {
                if offset <= prev.offset {
                    return Err(c.err(
                        "InvalidHavokFixup",
                        at,
                        format!("object offset > {} (increasing)", prev.offset),
                        offset,
                    ));
                }
            }
            if section as usize != CLASS_NAMES {
                return Err(c.err("InvalidHavokFixup", at + 4, CLASS_NAMES, section));
            }
            let class = class_names
                .binary_search_by_key(&class_offset, |n| n.offset)
                .map(|k| class_names[k].name)
                .map_err(|_| {
                    c.err(
                        "InvalidHavokFixup",
                        at + 8,
                        "offset of a name in __classnames__",
                        class_offset,
                    )
                })?;
            c.push(
                &mut objects,
                Object {
                    offset,
                    section,
                    class_offset,
                    class,
                },
                at,
            )?;
        }
        let mut object_pointers = Vec::new();
        for (i, [source, section, destination]) in raw_links.into_iter().enumerate() {
            let at = d + o[1] + i * 12;
            slot(source, at)?;
            if section as usize != DATA {
                return Err(c.err("InvalidHavokFixup", at + 4, DATA, section));
            }
            if objects
                .binary_search_by_key(&destination, |x| x.offset)
                .is_err()
            {
                return Err(c.err(
                    "InvalidHavokFixup",
                    at + 8,
                    "offset of an object in the third table",
                    destination,
                ));
            }
            c.push(
                &mut object_pointers,
                ObjectPointer {
                    source,
                    section,
                    destination,
                },
                at,
            )?;
        }
        // Each object owns the bytes up to the next one: a first-table
        // pointer's destination lies in the span of the object holding its
        // source, and a second-table source lies in some object's span.
        for (i, p) in pointers.iter().enumerate() {
            let at = d + o[0] + i * 8;
            let owner = objects::owner(&objects, objects_end as u32, p.source);
            if owner.is_none() {
                return Err(c.err(
                    "InvalidHavokObjectSpan",
                    at,
                    "source inside an object's span",
                    p.source,
                ));
            }
            if objects::owner(&objects, objects_end as u32, p.destination) != owner {
                let (lo, hi) = owner
                    .map(|k| {
                        let end = objects.get(k + 1).map_or(objects_end as u32, |n| n.offset);
                        (objects[k].offset, end)
                    })
                    .unwrap_or_default();
                return Err(c.err(
                    "InvalidHavokObjectSpan",
                    at + 4,
                    format!("destination in the source object's span [{lo}, {hi})"),
                    p.destination,
                ));
            }
        }
        for (i, p) in object_pointers.iter().enumerate() {
            if objects::owner(&objects, objects_end as u32, p.source).is_none() {
                return Err(c.err(
                    "InvalidHavokObjectSpan",
                    d + o[1] + i * 12,
                    "source inside an object's span",
                    p.source,
                ));
            }
        }
        let root = objects
            .binary_search_by_key(&header.root_offset(), |x| x.offset)
            .ok()
            .map(|k| &objects[k]);
        let root_ok = header.root_section() as usize == DATA
            && header.root_class_section() as usize == CLASS_NAMES
            && root.is_some_and(|r| r.class_offset == header.root_class_offset());
        if !root_ok {
            return Err(c.err(
                "InvalidHavokRoot",
                24,
                "words 24..40 name an object of __data__ and its class",
                format!(
                    "{} {} {} {}",
                    header.root_section(),
                    header.root_offset(),
                    header.root_class_section(),
                    header.root_class_offset()
                ),
            ));
        }
        Ok(Self {
            bytes,
            header,
            version,
            sections,
            class_names,
            pointers,
            object_pointers,
            objects,
            fill: [names_fill, f1, f2, f3],
        })
    }

    /// The `__data__` section; always present after `read`.
    pub fn data(&self) -> Option<&Section<'a>> {
        self.sections.get(DATA)
    }
    /// The bytes before the first table of `__data__`; every table offset is
    /// relative to their first byte.
    pub fn object_bytes(&self) -> &'a [u8] {
        // Checked by `read`; empty if the public fields were changed since.
        let Some(d) = self.data() else {
            return &[];
        };
        let start = d.start as usize;
        self.bytes
            .get(start..start.saturating_add(d.offsets[0] as usize))
            .unwrap_or(&[])
    }
    /// The object named by header words 24 to 36.
    pub fn root(&self) -> Option<&Object<'a>> {
        self.objects
            .binary_search_by_key(&self.header.root_offset(), |x| x.offset)
            .ok()
            .map(|k| &self.objects[k])
    }
}

fn class_names<'a>(c: &Ctx<'a, '_>, start: usize, end: usize) -> Result<(Vec<ClassName<'a>>, u32)> {
    let mut out = Vec::new();
    let mut p = start;
    while p < end {
        let rest = c.slice(p, end - p, "__classnames__")?;
        if rest.iter().all(|x| *x == FILL) {
            return Ok((out, rest.len() as u32));
        }
        // Signature, marker, at least one character and the NUL.
        if rest.len() < 7 {
            return Err(c.err(
                "InvalidHavokClassName",
                p,
                "a class-name entry of at least 7 bytes or 0xff up to the section end",
                hex(rest),
            ));
        }
        let signature = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
        let marker = rest[4];
        if marker != CLASS_NAME_MARKER {
            return Err(c.err(
                "InvalidHavokClassName",
                p + 4,
                format!("marker byte {CLASS_NAME_MARKER}"),
                marker,
            ));
        }
        let (name, nul) = c.text(p + 5, end, "InvalidHavokClassName", "class name")?;
        c.push(
            &mut out,
            ClassName {
                offset: (p + 5 - start) as u32,
                signature,
                marker,
                name,
            },
            p,
        )?;
        p = nul + 1;
    }
    Ok((out, 0))
}

/// Entries of `N` u32 words from `lo`, until the bytes left are all 0xff.
fn table<const N: usize>(
    c: &Ctx<'_, '_>,
    lo: usize,
    hi: usize,
    what: &str,
) -> Result<(Vec<[u32; N]>, u32)> {
    let mut out = Vec::new();
    let mut q = lo;
    let width = 4 * N;
    while q < hi {
        let rest = c.slice(q, hi - q, what)?;
        if rest.iter().all(|x| *x == FILL) {
            return Ok((out, rest.len() as u32));
        }
        if rest.len() < width || rest[..4] == [FILL; 4] {
            return Err(c.err(
                "InvalidHavokFixupTable",
                q,
                format!("a {width}-byte {what} entry or 0xff up to {hi}"),
                hex(&rest[..rest.len().min(width)]),
            ));
        }
        let mut entry = [0u32; N];
        for (k, w) in entry.iter_mut().enumerate() {
            *w = c.u32(q + 4 * k, what)?;
        }
        c.push(&mut out, entry, q)?;
        q += width;
    }
    Ok((out, 0))
}

#[cfg(test)]
mod tests;
