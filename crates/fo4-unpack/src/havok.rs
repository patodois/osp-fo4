//! Havok container census: counts and small stored values only.
//!
//! Reads every bhkSystem payload of the winning NIFs with
//! `fo4_havok::Container` and counts, per system kind, how often each
//! container-level property holds. Properties the reader enforces are counted
//! by the reads that succeed; the others are counted here, holds and fails.
//! Object rules (spans, leading sizes, the convex polytope view) are counted
//! per object. Nothing here writes object contents.
use crate::Reports;
use fo4_core::{Budget, Manifest, Result};
use fo4_havok::{Container, ConvexPolytope, Object, CONVEX_POLYTOPE};
use fo4_nif::Nif;
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

mod bodies;
mod mesh;

const CAP: usize = 256;
const OTHER: &str = "<other>";
/// Distinct span lengths kept per class; more are reported as `SPAN_CAP+`.
const SPAN_CAP: usize = 1024;
/// 16-byte groups after the first 16 bytes of the root object whose third
/// u32 is compared with the number of collision objects on the system.
const ROOT_GROUPS: usize = 8;

fn bump<K: Ord + Clone>(map: &mut BTreeMap<K, u64>, key: K, other: K) {
    let key = if map.contains_key(&key) || map.len() < CAP {
        key
    } else {
        other
    };
    *map.entry(key).or_default() += 1;
}

#[derive(Default, Serialize, Clone, Copy)]
pub struct Check {
    pub holds: u64,
    pub fails: u64,
}

#[derive(Default, Serialize)]
pub struct Class {
    /// Payloads whose `__classnames__` lists the name.
    pub listed: u64,
    /// Payloads with at least one object of the class.
    pub payloads: u64,
    pub objects: u64,
    pub signatures: BTreeMap<u32, u64>,
    /// Payloads where the number of objects of the class equals the number of
    /// bhkNPCollisionObject on the system, and where it differs.
    pub objects_eq_system_objects: Check,
    /// Object span lengths (offset of the next object minus this offset).
    pub spans: BTreeSet<u32>,
    /// Leading size: smallest first-table destination owned by the object
    /// minus its offset, or "none" when it owns no destination.
    pub leads: BTreeMap<String, u64>,
}

#[derive(Default, Serialize)]
pub struct Kind {
    pub payloads: u64,
    pub read: u64,
    pub errors: BTreeMap<String, u64>,
    pub checks: BTreeMap<&'static str, Check>,
    pub classes: BTreeMap<String, Class>,
    pub markers: BTreeMap<u8, u64>,
    /// "table|bytes" -> payloads.
    pub fill: BTreeMap<String, u64>,
    pub totals: BTreeMap<&'static str, u64>,
    pub maxima: BTreeMap<&'static str, u64>,
    pub header_words: BTreeMap<String, u64>,
    pub versions: BTreeMap<String, u64>,
    /// Classes of objects whose offset is also a first-table source.
    pub pointer_source_is_object: BTreeMap<String, u64>,
    /// Classes of the objects second-table entries point to.
    pub object_pointer_targets: BTreeMap<String, u64>,
    pub root_class: BTreeMap<String, u64>,
    /// "root class|+offset" -> u32 there equal / not equal to the system's objects.
    pub root_counts: BTreeMap<String, Check>,
    /// "root class|+group offset|property" -> holds / fails.
    pub root_groups: BTreeMap<String, Check>,
    /// "class|rule" -> objects where the rule holds / fails.
    pub object_checks: BTreeMap<String, Check>,
    /// ConvexPolytope::read results: "read" or the error code -> objects.
    pub convex_polytope: BTreeMap<String, u64>,
    /// "u16 at +16|u16 at +18" of convex polytopes -> objects.
    pub convex_polytope_word16: BTreeMap<String, u64>,
    /// Class of the object the second-table source at +32 of a convex
    /// polytope points to, or "none".
    pub convex_polytope_link32: BTreeMap<String, u64>,
    /// Vertex rows after the last tagged one (copies of it) -> objects.
    pub convex_polytope_padding_rows: BTreeMap<u64, u64>,
    /// Largest value in a single object.
    pub object_maxima: BTreeMap<&'static str, u64>,
    /// "class|+slot" (or "class|inside_storage") -> element-size bounds.
    pub array_gaps: BTreeMap<String, bodies::Gap>,
    /// "section|key" -> objects, bodies or instances, as the section says.
    pub counts: BTreeMap<String, u64>,
    /// "root class|objects" or "root class|systems" -> Body ID below the
    /// body count of the system (holds / fails).
    pub body_ids: BTreeMap<String, Check>,
}
impl Kind {
    fn check(&mut self, name: &'static str, ok: bool) {
        let c = self.checks.entry(name).or_default();
        if ok {
            c.holds += 1;
        } else {
            c.fails += 1;
        }
    }
    fn object_check(&mut self, class: &str, rule: &str, ok: bool) {
        let c = self
            .object_checks
            .entry(format!("{class}|{rule}"))
            .or_default();
        if ok {
            c.holds += 1;
        } else {
            c.fails += 1;
        }
    }
    fn count(&mut self, section: &str, key: &str) {
        let key = format!("{section}\t{key}");
        let key = if self.counts.contains_key(&key) || self.counts.len() < 16 * CAP {
            key
        } else {
            format!("{section}\t{OTHER}")
        };
        *self.counts.entry(key).or_default() += 1;
    }
    fn totals_mut(&mut self, name: &'static str) -> &mut u64 {
        self.totals.entry(name).or_default()
    }
    fn total(&mut self, name: &'static str, n: u64) {
        *self.totals.entry(name).or_default() += n;
        let m = self.maxima.entry(name).or_default();
        *m = (*m).max(n);
    }
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub nif_files: u64,
    pub nif_read: u64,
    pub nif_failed: u64,
    pub collision_view_failed: u64,
    pub kinds: BTreeMap<String, Kind>,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let w = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// Measures one payload; `body_ids` are the Body IDs of the
/// bhkNPCollisionObject on its system; `render` holds the file's render
/// meshes when the file has one system and identity transforms.
fn payload(
    k: &mut Kind,
    bytes: &[u8],
    body_ids: &[u32],
    render: Option<&mesh::Render>,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<()> {
    let objects = body_ids.len() as u64;
    k.payloads += 1;
    // Byte order: record starts read big-endian against the payload length.
    let be_in_bounds = (0..3).all(|i| {
        bytes
            .get(84 + 64 * i..88 + 64 * i)
            .map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) as usize)
            .is_some_and(|v| v <= bytes.len())
    });
    k.check("record_starts_read_big_endian_in_bounds", be_in_bounds);
    let c = match Container::read(bytes, file, base, b) {
        Ok(c) => c,
        Err(e) => {
            bump(&mut k.errors, e.code.clone(), OTHER.into());
            return Err(e);
        }
    };
    k.read += 1;
    for (i, w) in c.header.words.iter().enumerate() {
        bump(
            &mut k.header_words,
            format!("{:02}|{w:08x}", i * 4),
            OTHER.into(),
        );
    }
    bump(&mut k.versions, c.version.to_owned(), OTHER.into());
    // Always present after a successful read.
    let Some(data) = c.data() else {
        return Ok(());
    };
    let d = data.start as usize;
    k.check(
        "section_starts_and_offsets_multiple_of_16",
        c.sections
            .iter()
            .all(|s| s.start.is_multiple_of(16) && s.offsets.iter().all(|o| o.is_multiple_of(16))),
    );
    k.check(
        "section_records_byte19_and_48_to_63_are_ff",
        c.sections.iter().all(|s| {
            let r = s.record as usize;
            bytes.get(r + 19) == Some(&0xff)
                && bytes
                    .get(r + 48..r + 64)
                    .is_some_and(|x| x.iter().all(|v| *v == 0xff))
        }),
    );
    k.check(
        "section_record_name_padding_zero",
        c.sections.iter().all(|s| {
            let r = s.record as usize;
            bytes
                .get(r + s.name.len()..r + 19)
                .is_some_and(|x| x.iter().all(|v| *v == 0))
        }),
    );
    k.total("class_names", c.class_names.len() as u64);
    k.total("pointers", c.pointers.len() as u64);
    k.total("object_pointers", c.object_pointers.len() as u64);
    k.total("objects", c.objects.len() as u64);
    k.total("object_bytes", data.offsets[0] as u64);
    for (table, fill) in ["classnames", "pointers", "object_pointers", "objects"]
        .iter()
        .zip(c.fill)
    {
        bump(&mut k.fill, format!("{table}|{fill:02}"), OTHER.into());
    }
    k.check("classnames_fill_below_16", c.fill[0] < 16);
    k.check("pointer_fill_below_16", c.fill[1] < 16);
    k.check("object_pointer_fill_below_16", c.fill[2] < 16);
    k.check("object_fill_below_16", c.fill[3] < 16);
    let mut listed = BTreeSet::new();
    for n in &c.class_names {
        bump(&mut k.markers, n.marker, 0);
        listed.insert(n.name);
        let class = k.classes.entry(n.name.to_owned()).or_default();
        class.listed += 1;
        bump(&mut class.signatures, n.signature, u32::MAX);
    }
    k.check("class_names_distinct", listed.len() == c.class_names.len());
    let mut per_class: BTreeMap<&str, u64> = BTreeMap::new();
    for o in &c.objects {
        *per_class.entry(o.class).or_default() += 1;
    }
    for name in &listed {
        let n = per_class.get(name).copied().unwrap_or(0);
        let class = k.classes.entry((*name).to_owned()).or_default();
        if n > 0 {
            class.payloads += 1;
            class.objects += n;
        }
        let check = &mut class.objects_eq_system_objects;
        if n == objects {
            check.holds += 1;
        } else {
            check.fails += 1;
        }
    }
    let zero = |source: u32| {
        bytes
            .get(d + source as usize..d + source as usize + 8)
            .is_some_and(|x| x.iter().all(|v| *v == 0))
    };
    k.check(
        "pointer_source_multiple_of_8",
        c.pointers.iter().all(|p| p.source.is_multiple_of(8)),
    );
    k.check(
        "pointer_destination_multiple_of_16",
        c.pointers.iter().all(|p| p.destination.is_multiple_of(16)),
    );
    k.check(
        "object_pointer_source_multiple_of_8",
        c.object_pointers.iter().all(|p| p.source.is_multiple_of(8)),
    );
    k.check(
        "object_offset_multiple_of_16",
        c.objects.iter().all(|o| o.offset.is_multiple_of(16)),
    );
    k.check(
        "pointer_sources_increasing",
        c.pointers.windows(2).all(|w| w[0].source < w[1].source),
    );
    k.check(
        "pointer_sources_increasing_iff_no_hknpCompressedMeshShape_listed",
        c.pointers.windows(2).all(|w| w[0].source < w[1].source)
            != c.class_names
                .iter()
                .any(|n| n.name == "hknpCompressedMeshShape"),
    );
    k.check(
        "object_pointer_sources_increasing",
        c.object_pointers
            .windows(2)
            .all(|w| w[0].source < w[1].source),
    );
    let mut pointer_sources: Vec<u32> = c.pointers.iter().map(|p| p.source).collect();
    pointer_sources.sort_unstable();
    let mut link_sources: Vec<u32> = c.object_pointers.iter().map(|p| p.source).collect();
    link_sources.sort_unstable();
    k.check(
        "pointer_sources_distinct",
        pointer_sources.windows(2).all(|w| w[0] != w[1]),
    );
    k.check(
        "object_pointer_sources_distinct",
        link_sources.windows(2).all(|w| w[0] != w[1]),
    );
    k.check(
        "pointer_and_object_pointer_sources_disjoint",
        link_sources
            .iter()
            .all(|s| pointer_sources.binary_search(s).is_err()),
    );
    let mut all: Vec<u32> = pointer_sources
        .iter()
        .chain(&link_sources)
        .copied()
        .collect();
    all.sort_unstable();
    k.check(
        "pointer_and_object_pointer_sources_8_apart",
        all.windows(2).all(|w| w[1] - w[0] >= 8),
    );
    k.check(
        "eight_zero_bytes_at_every_pointer_source",
        c.pointers.iter().all(|p| zero(p.source)),
    );
    k.check(
        "eight_zero_bytes_at_every_object_pointer_source",
        c.object_pointers.iter().all(|p| zero(p.source)),
    );
    k.check(
        "eight_zero_bytes_at_every_object_offset",
        c.objects.iter().all(|o| zero(o.offset)),
    );
    let find = |offset: u32| -> Option<&Object<'_>> {
        c.objects
            .binary_search_by_key(&offset, |o| o.offset)
            .ok()
            .map(|i| &c.objects[i])
    };
    k.check(
        "pointer_destination_not_an_object_offset",
        c.pointers.iter().all(|p| find(p.destination).is_none()),
    );
    for p in &c.pointers {
        if let Some(o) = find(p.source) {
            bump(
                &mut k.pointer_source_is_object,
                o.class.to_owned(),
                OTHER.into(),
            );
        }
    }
    k.check(
        "object_pointer_source_not_an_object_offset",
        c.object_pointers.iter().all(|p| find(p.source).is_none()),
    );
    for p in &c.object_pointers {
        if let Some(o) = find(p.destination) {
            bump(
                &mut k.object_pointer_targets,
                o.class.to_owned(),
                OTHER.into(),
            );
        }
    }
    k.check(
        "root_is_first_object",
        c.objects.first().map(|o| o.offset) == Some(c.header.root_offset()),
    );
    k.check(
        "root_class_is_fifth_class_name",
        c.class_names.get(4).map(|n| n.offset) == Some(c.header.root_class_offset()),
    );
    k.check(
        "first_four_class_names_never_objects",
        c.class_names
            .iter()
            .take(4)
            .all(|n| !per_class.contains_key(n.name)),
    );
    objects_census(k, &c, file, base);
    bodies::census(k, &c, file, base, body_ids, render);
    if let Some(root) = c.root() {
        bump(&mut k.root_class, root.class.to_owned(), OTHER.into());
        let at = d + root.offset as usize;
        for g in 0..ROOT_GROUPS {
            let off = 16 + 16 * g + 8;
            let key = format!("{}|+{off:03}", root.class);
            let check = k.root_counts.entry(key).or_default();
            let count = u32_at(bytes, at + off);
            if count.map(u64::from) == Some(objects) {
                check.holds += 1;
            } else {
                check.fails += 1;
            }
            // The u32 after the compared one, and whether the group's first
            // eight bytes are a first-table source.
            let next = u32_at(bytes, at + off + 4);
            let start = root.offset + 16 + 16 * g as u32;
            let pointer = c.pointers.iter().any(|p| p.source == start);
            for (name, ok) in [
                (
                    "next_u32_is_0x80000000_or_count",
                    count.zip(next).is_some_and(|(n, x)| x == 0x8000_0000 | n),
                ),
                (
                    "pointer_source_at_group_start_iff_count_nonzero",
                    count.is_some_and(|n| pointer == (n > 0)),
                ),
            ] {
                let check = k
                    .root_groups
                    .entry(format!("{}|+{:03}|{name}", root.class, off - 8))
                    .or_default();
                if ok {
                    check.holds += 1;
                } else {
                    check.fails += 1;
                }
            }
        }
    }
    Ok(())
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    u32_at(bytes, at).map(f32::from_bits)
}
fn zero(bytes: &[u8], lo: usize, hi: usize) -> bool {
    bytes.get(lo..hi).is_some_and(|x| x.iter().all(|v| *v == 0))
}

/// Object spans, leading sizes, array headers at pointer sources and the
/// convex shapes. Counted per object.
fn objects_census(k: &mut Kind, c: &Container<'_>, file: &str, base: u64) {
    let data = c.object_bytes();
    let mut owned: Vec<Vec<(u32, u32)>> = vec![Vec::new(); c.objects.len()];
    for p in &c.pointers {
        if let Some(i) = c.object_at(p.source) {
            owned[i].push((p.source, p.destination));
        }
    }
    let mut after = true;
    let mut header = true;
    let mut distinct = true;
    for span in c.object_spans() {
        let class = k.classes.entry(span.class.to_owned()).or_default();
        if class.spans.len() < SPAN_CAP || class.spans.contains(&span.len()) {
            class.spans.insert(span.len());
        }
        let mut ptrs = std::mem::take(&mut owned[span.index]);
        let lead = ptrs
            .iter()
            .map(|p| p.1)
            .min()
            .map_or("none".to_owned(), |d| (d - span.offset).to_string());
        bump(&mut class.leads, lead, OTHER.into());
        ptrs.sort_unstable();
        bodies::gaps(k, data, span.class, span.offset, span.end, &ptrs);
        let order = ptrs.windows(2).all(|w| w[0].1 < w[1].1);
        k.object_check(span.class, "destinations_follow_source_order", order);
        for (s, d) in &ptrs {
            after &= d > s;
            let n = u32_at(data, *s as usize + 8);
            let flag = u32_at(data, *s as usize + 12);
            header &= n
                .zip(flag)
                .is_some_and(|(n, f)| n > 0 && f == 0x8000_0000 | n);
        }
        let mut dests: Vec<u32> = ptrs.iter().map(|p| p.1).collect();
        dests.sort_unstable();
        distinct &= dests.windows(2).all(|w| w[0] != w[1]);
        let bytes = c.span_bytes(&span);
        if span.class == "hknpConvexShape" {
            let rows = u16_at(bytes, 48).unwrap_or(0) as usize;
            let rel = u16_at(bytes, 50).unwrap_or(0) as usize;
            k.object_check(span.class, "vertex_pair_at_48_points_to_64", rel == 16);
            k.object_check(
                span.class,
                "span_is_64_plus_16_per_vertex_row",
                bytes.len() == 64 + 16 * rows,
            );
            k.object_check(
                span.class,
                "vertex_rows_tagged_3f0000_and_index",
                (0..rows).all(|j| u32_at(bytes, 64 + 16 * j + 12) == Some(0x3f00_0000 | j as u32)),
            );
        }
        if span.class == CONVEX_POLYTOPE {
            polytope_census(k, c, span.index, file, base);
        }
    }
    k.check("pointer_destination_after_source", after);
    k.check("pointer_destinations_distinct", distinct);
    k.check(
        "pointer_source_u32_8_nonzero_and_u32_12_is_0x80000000_or_it",
        header,
    );
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let w = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([w[0], w[1]]))
}

fn polytope_census(k: &mut Kind, c: &Container<'_>, index: usize, file: &str, base: u64) {
    let p = match ConvexPolytope::read(c, index, file, base) {
        Ok(p) => p,
        Err(e) => {
            bump(&mut k.convex_polytope, e.code, OTHER.into());
            return;
        }
    };
    bump(&mut k.convex_polytope, "read".into(), OTHER.into());
    let class = CONVEX_POLYTOPE;
    let b = p.bytes();
    let w16 = format!(
        "{}|{}",
        u16_at(b, 16).unwrap_or(0),
        u16_at(b, 18).unwrap_or(0)
    );
    bump(&mut k.convex_polytope_word16, w16, OTHER.into());
    let link = c
        .object_pointers
        .iter()
        .find(|l| l.source == p.offset + 32)
        .and_then(|l| {
            c.objects
                .binary_search_by_key(&l.destination, |o| o.offset)
                .ok()
        })
        .map_or("none".to_owned(), |i| c.objects[i].class.to_owned());
    bump(&mut k.convex_polytope_link32, link, OTHER.into());
    let faces_at = 80 + 16 * p.vertex_rows + 16 * p.plane_rows;
    let indices_end = faces_at + 4 * p.plane_rows + p.index_count;
    for (rule, ok) in [
        ("bytes_0_to_16_zero", zero(b, 0, 16)),
        ("bytes_28_to_48_zero", zero(b, 28, 48)),
        ("bytes_52_to_64_zero", zero(b, 52, 64)),
        ("bytes_76_to_80_zero", zero(b, 76, 80)),
        (
            "f32_at_20_finite_nonnegative",
            f32_at(b, 20).is_some_and(|v| v.is_finite() && v >= 0.0),
        ),
        (
            "padding_face_entries_zero",
            zero(b, faces_at + 4 * p.face_count, faces_at + 4 * p.plane_rows),
        ),
        ("bytes_after_indices_zero", zero(b, indices_end, b.len())),
    ] {
        k.object_check(class, rule, ok);
    }
    let faces: Vec<_> = (0..p.face_count).filter_map(|j| p.face(j)).collect();
    let mut sum = 0usize;
    let mut running = true;
    for f in &faces {
        running &= f.first as usize == sum;
        sum += f.count as usize;
    }
    k.object_check(class, "face_first_is_running_sum_of_counts", running);
    k.object_check(
        class,
        "face_counts_sum_to_index_count",
        sum == p.index_count,
    );
    k.object_check(
        class,
        "face_count_at_least_3",
        faces.iter().all(|f| f.count >= 3),
    );
    let planes: Vec<[f64; 4]> = (0..p.face_count)
        .filter_map(|j| p.plane(j))
        .map(|q| q.map(f64::from))
        .collect();
    let verts: Vec<[f64; 3]> = p.vertices().map(|v| v.map(f64::from)).collect();
    k.object_check(
        class,
        "plane_normals_unit_within_1e-4",
        planes
            .iter()
            .all(|q| ((q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt() - 1.0).abs() <= 1e-4),
    );
    let scale = verts
        .iter()
        .flat_map(|v| v.iter().map(|x| x.abs()))
        .fold(1.0f64, f64::max);
    let dist =
        |q: &[f64; 4], v: &[f64; 3]| (q[0] * v[0] + q[1] * v[1] + q[2] * v[2] + q[3]) / scale;
    let behind = planes
        .iter()
        .flat_map(|q| verts.iter().map(move |v| (q, v)))
        .map(|(q, v)| dist(q, v))
        .fold(f64::MIN, f64::max);
    k.object_check(
        class,
        "vertices_behind_face_planes_rel_1e-3",
        behind <= 1e-3,
    );
    let idx = p.indices();
    let mut on = 0f64;
    for (q, f) in planes.iter().zip(&faces) {
        let lo = f.first as usize;
        for i in idx.get(lo..lo + f.count as usize).unwrap_or(&[]) {
            if let Some(v) = verts.get(*i as usize) {
                on = on.max(dist(q, v).abs());
            }
        }
    }
    k.object_check(class, "face_vertices_on_own_plane_rel_1e-2", on <= 1e-2);
    let mut used = vec![false; p.vertex_count];
    for i in idx {
        if let Some(u) = used.get_mut(*i as usize) {
            *u = true;
        }
    }
    k.object_check(class, "every_vertex_in_some_face", used.iter().all(|u| *u));
    *k.totals.entry("convex_polytope_vertices").or_default() += p.vertex_count as u64;
    bump(
        &mut k.convex_polytope_padding_rows,
        (p.vertex_rows - p.vertex_count) as u64,
        u64::MAX,
    );
    for (name, n) in [
        ("convex_polytope_vertex_rows", p.vertex_rows),
        ("convex_polytope_plane_rows", p.plane_rows),
        ("convex_polytope_index_count", p.index_count),
    ] {
        let m = k.object_maxima.entry(name).or_default();
        *m = (*m).max(n as u64);
    }
}

pub fn sweep(m: &Manifest, r: &mut Reports) -> Result<()> {
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets".into());
    let mut s = Summary::default();
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(c) = chain.last() else { continue };
        let Some(key) = &c.key else { continue };
        if !key.ends_with(b".nif") {
            continue;
        }
        s.nif_files += 1;
        let work = Budget::new(m.limits.max_memory);
        let bytes = match v.read_candidate(c, &work) {
            Ok(bytes) => bytes,
            Err(e) => {
                s.nif_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        let n = match Nif::read(&bytes, &c.original, m.game, &m.limits, &work) {
            Ok(n) => n,
            Err(e) => {
                s.nif_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        s.nif_read += 1;
        let col = match n.collision(&c.original, &work) {
            Ok(col) => col,
            Err(e) => {
                s.collision_view_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        let render = if col.systems.len() == 1 {
            render(&n)
        } else {
            None
        };
        for system in &col.systems {
            let body_ids: Vec<u32> = col
                .objects
                .iter()
                .filter(|o| o.data == system.block as i32)
                .map(|o| o.body_id)
                .collect();
            let kind = s
                .kinds
                .entry(system.kind.block_type().to_owned())
                .or_default();
            let scratch = Budget::new(m.limits.max_memory);
            if let Err(e) = payload(
                kind,
                system.payload,
                &body_ids,
                render.as_ref(),
                &c.original,
                system.payload_offset,
                &scratch,
            ) {
                r.error(&e)?;
            }
        }
    }
    let mut text = String::new();
    for (k, value) in [
        (
            "build",
            m.build.clone().unwrap_or_else(|| "undeclared".into()),
        ),
        ("plugins", m.plugins.len().to_string()),
        ("mounts", m.mounts.len().to_string()),
        (
            "vfs_fingerprint",
            r.summary.fingerprint.clone().unwrap_or_default(),
        ),
    ] {
        text.push_str(&format!("census\t{k}\t{value}\n"));
    }
    text.push_str(&tsv(&s));
    r.tsv = Some(("havok.tsv", text));
    r.summary.havok = s;
    Ok(())
}

/// Box, vertex positions and triangles of the render meshes, when every node
/// and mesh block stores translation 0, the identity rotation and scale 1.
fn render(n: &fo4_nif::Nif<'_>) -> Option<mesh::Render> {
    use fo4_nif::{Data, Value};
    let mut bx = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let identity = [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    for block in &n.blocks {
        let fields = match &block.decoded {
            Some(Data::Node { fields, .. }) => fields,
            Some(Data::Mesh(m)) => &m.fields,
            _ => continue,
        };
        let floats = |name: &str| -> Vec<f32> {
            fields
                .iter()
                .filter(|f| f.name == name)
                .filter_map(|f| match f.value {
                    Value::FloatBits(b) => Some(f32::from_bits(b)),
                    _ => None,
                })
                .collect()
        };
        let t = floats("Translation");
        let r = floats("Rotation");
        let s = floats("Scale");
        let placed = (t.is_empty() || t == [0.0; 3])
            && (r.is_empty() || r == identity)
            && (s.is_empty() || s == [1.0]);
        if !placed {
            return None;
        }
        if let Some(Data::Mesh(m)) = &block.decoded {
            let first = positions.len() as u32;
            for v in m.vertices.iter().flatten() {
                let p = v.position.unwrap_or([f32::NAN; 3]);
                if v.position.is_some() {
                    mesh::grow(&mut bx, p);
                }
                positions.push(p);
            }
            let count = positions.len() as u32 - first;
            for t in m.triangles.iter().flatten() {
                if t.iter().all(|x| u32::from(*x) < count) {
                    triangles.push(t.map(|x| first + u32::from(x)));
                }
            }
        }
    }
    (bx.0[0] <= bx.1[0]).then_some(mesh::Render {
        extent: bx,
        positions,
        triangles,
    })
}

/// Text report: `section<TAB>key<TAB>value`, counts and small stored values.
pub fn tsv(s: &Summary) -> String {
    let mut out = String::from("# section\tkey\tvalue\n");
    let mut row = |section: &str, key: &str, value: String| {
        out.push_str(section);
        out.push('\t');
        out.push_str(key);
        out.push('\t');
        out.push_str(&value);
        out.push('\n');
    };
    for (k, v) in [
        ("nif_files", s.nif_files),
        ("nif_read", s.nif_read),
        ("nif_failed", s.nif_failed),
        ("collision_view_failed", s.collision_view_failed),
    ] {
        row("total", k, v.to_string());
    }
    let check = |c: &Check| format!("holds={} fails={}", c.holds, c.fails);
    for (kind, k) in &s.kinds {
        let sec = |name: &str| format!("havok.{kind}.{name}");
        row(&sec("read"), "payloads", k.payloads.to_string());
        row(&sec("read"), "read", k.read.to_string());
        for (code, n) in &k.errors {
            row(&sec("error"), code, n.to_string());
        }
        for (name, c) in &k.checks {
            row(&sec("check"), name, check(c));
        }
        for (name, n) in &k.totals {
            row(&sec("total"), name, n.to_string());
        }
        for (name, n) in &k.maxima {
            row(&sec("max_per_payload"), name, n.to_string());
        }
        for (key, n) in &k.header_words {
            row(&sec("header_word"), key, n.to_string());
        }
        for (key, n) in &k.versions {
            row(&sec("version"), key, n.to_string());
        }
        for (key, n) in &k.markers {
            row(&sec("class_name_marker"), &key.to_string(), n.to_string());
        }
        for (key, n) in &k.fill {
            row(&sec("fill"), key, n.to_string());
        }
        for (name, c) in &k.classes {
            let signatures: Vec<String> = c
                .signatures
                .iter()
                .map(|(sig, n)| format!("{sig:08x}x{n}"))
                .collect();
            row(
                &sec("class"),
                name,
                format!(
                    "listed={} payloads={} objects={} signatures={} count_eq_system_objects={} span_min={} span_max={} span_distinct={}",
                    c.listed,
                    c.payloads,
                    c.objects,
                    signatures.join(","),
                    check(&c.objects_eq_system_objects),
                    c.spans.first().copied().unwrap_or(0),
                    c.spans.last().copied().unwrap_or(0),
                    if c.spans.len() >= SPAN_CAP {
                        format!("{SPAN_CAP}+")
                    } else {
                        c.spans.len().to_string()
                    }
                ),
            );
        }
        for (name, c) in &k.classes {
            for (lead, n) in &c.leads {
                row(
                    &sec("object_lead"),
                    &format!("{name}|{lead}"),
                    n.to_string(),
                );
            }
        }
        for (key, c) in &k.object_checks {
            row(&sec("object_check"), key, check(c));
        }
        for (key, n) in &k.convex_polytope {
            row(&sec("convex_polytope"), key, n.to_string());
        }
        for (key, n) in &k.convex_polytope_word16 {
            row(&sec("convex_polytope_u16_16_18"), key, n.to_string());
        }
        for (key, n) in &k.convex_polytope_link32 {
            row(&sec("convex_polytope_link32"), key, n.to_string());
        }
        for (key, n) in &k.convex_polytope_padding_rows {
            row(
                &sec("convex_polytope_padding_rows"),
                &key.to_string(),
                n.to_string(),
            );
        }
        for (name, n) in &k.object_maxima {
            row(&sec("max_per_object"), name, n.to_string());
        }
        for (key, g) in &k.array_gaps {
            row(
                &sec("array_gap"),
                key,
                format!(
                    "arrays={} count_max={} element_min={} element_max={} exact={}",
                    g.arrays, g.count_max, g.element_min, g.element_max, g.exact
                ),
            );
        }
        for (key, n) in &k.counts {
            let (section, key) = key.split_once('\t').unwrap_or(("count", key));
            row(&sec(section), key, n.to_string());
        }
        for (key, c) in &k.body_ids {
            row(&sec("body_id_below_body_count"), key, check(c));
        }
        for (key, n) in &k.pointer_source_is_object {
            row(&sec("pointer_source_is_object"), key, n.to_string());
        }
        for (key, n) in &k.object_pointer_targets {
            row(&sec("object_pointer_target"), key, n.to_string());
        }
        for (key, n) in &k.root_class {
            row(&sec("root_class"), key, n.to_string());
        }
        for (key, c) in &k.root_counts {
            row(&sec("root_u32_eq_system_objects"), key, check(c));
        }
        for (key, c) in &k.root_groups {
            row(&sec("root_group"), key, check(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic container: root object with one group whose third u32 is 2.
    fn container() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(fo4_havok::MAGIC);
        out.extend(0u32.to_le_bytes());
        out.extend(11u32.to_le_bytes());
        out.extend(fo4_havok::LAYOUT);
        for w in [3u32, 2, 0, 0, 5] {
            out.extend(w.to_le_bytes());
        }
        out.extend(b"hk_2014.1.0-r1\0\xff");
        out.extend([0u8; 8]);
        let mut names = Vec::new();
        names.extend(7u32.to_le_bytes());
        names.push(9);
        names.extend(b"Root\0");
        names.resize(16, 0xff);
        let mut data = vec![0u8; 0x40];
        data[0x18..0x1c].copy_from_slice(&2u32.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&0x8000_0002u32.to_le_bytes());
        for w in [0x10u32, 0x30] {
            data.extend(w.to_le_bytes());
        }
        data.resize(0x50, 0xff);
        data.resize(0x60, 0xff);
        for w in [0u32, 0, 5] {
            data.extend(w.to_le_bytes());
        }
        data.resize(0x70, 0xff);
        for (i, (name, start, words)) in [
            ("__classnames__", 256u32, [16u32; 6]),
            ("__types__", 272, [0; 6]),
            ("__data__", 272, [0x40, 0x50, 0x60, 0x70, 0x70, 0x70]),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(out.len(), 64 + 64 * i);
            let mut record = name.as_bytes().to_vec();
            record.resize(19, 0);
            record.push(0xff);
            record.extend(start.to_le_bytes());
            words.iter().for_each(|w| record.extend(w.to_le_bytes()));
            record.extend([0xff; 16]);
            out.extend(record);
        }
        out.extend(names);
        out.extend(data);
        out
    }

    #[test]
    fn counts_checks_classes_and_root_groups() {
        let bytes = container();
        let b = Budget::new(1 << 20);
        let mut k = Kind::default();
        payload(&mut k, &bytes, &[0, 1], None, "a.nif", 0, &b).unwrap();
        payload(&mut k, &bytes, &[0], None, "b.nif", 0, &b).unwrap();
        assert!(payload(&mut k, &bytes[..100], &[0], None, "c.nif", 0, &b).is_err());
        assert_eq!((k.payloads, k.read), (3, 2));
        assert_eq!(k.errors.get("TruncatedHavokContainer"), Some(&1));
        let c = k.checks["eight_zero_bytes_at_every_pointer_source"];
        assert_eq!((c.holds, c.fails), (2, 0));
        let c = k.checks["record_starts_read_big_endian_in_bounds"];
        assert_eq!((c.holds, c.fails), (0, 3));
        let root = k.root_counts["Root|+024"];
        assert_eq!((root.holds, root.fails), (1, 1));
        let group = k.root_groups["Root|+016|next_u32_is_0x80000000_or_count"];
        assert_eq!((group.holds, group.fails), (2, 0));
        let group = k.root_groups["Root|+032|next_u32_is_0x80000000_or_count"];
        assert_eq!((group.holds, group.fails), (0, 2));
        let group = k.root_groups["Root|+016|pointer_source_at_group_start_iff_count_nonzero"];
        assert_eq!((group.holds, group.fails), (2, 0));
        let class = &k.classes["Root"];
        assert_eq!((class.listed, class.payloads, class.objects), (2, 2, 2));
        assert_eq!(class.objects_eq_system_objects.holds, 1);
        let mut s = Summary::default();
        s.kinds.insert("bhkPhysicsSystem".into(), k);
        let text = tsv(&s);
        assert!(
            text.contains("havok.bhkPhysicsSystem.read\tread\t2\n"),
            "{text}"
        );
        assert!(text.contains(
            "havok.bhkPhysicsSystem.class\tRoot\tlisted=2 payloads=2 objects=2 signatures=00000007x2 count_eq_system_objects=holds=1 fails=1 span_min=64 span_max=64 span_distinct=1\n"
        ));
        assert!(text.contains("havok.bhkPhysicsSystem.fill\tclassnames|06\t2\n"));
        assert!(text.contains("havok.bhkPhysicsSystem.object_lead\tRoot|48\t2\n"));
        assert!(text.contains(
            "havok.bhkPhysicsSystem.check\tpointer_destination_after_source\tholds=2 fails=0\n"
        ));
        assert!(text.contains(
            "havok.bhkPhysicsSystem.check\tpointer_source_u32_8_nonzero_and_u32_12_is_0x80000000_or_it\tholds=2 fails=0\n"
        ));
        // 16 bytes from the destination to the span end over 2 elements.
        assert!(text.contains(
            "havok.bhkPhysicsSystem.array_gap\tRoot|+016\tarrays=2 count_max=2 element_min=1 element_max=8 exact=2\n"
        ));
        // The root is not a top-level class the body view reads.
        assert!(text.contains("havok.bhkPhysicsSystem.view\tRoot|UnexpectedHavokClass\t2\n"));
        assert!(!text.contains("body_id_below_body_count"));
    }
}
