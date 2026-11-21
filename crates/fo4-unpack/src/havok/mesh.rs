//! Census of `hknpCompressedMeshShape` and `hknpCompressedMeshShapeData`:
//! the views, the rules they do not enforce, the box trees, the vertex
//! records, small distributions, and decoded meshes against the render
//! meshes of the same file. Counts and small stored constants only.
use super::{u32_at, zero, Kind};
use fo4_core::Budget;
use fo4_havok::{
    CompressedMesh, Container, Geometry, MeshBox, MeshData, MeshSection, COMPRESSED_MESH_DATA,
    PLACEHOLDER_PRIMITIVE,
};
use std::collections::HashMap;

/// Box of the render meshes of a file whose nodes and meshes all store the
/// identity transform.
pub type Extent = ([f32; 3], [f32; 3]);

/// Render meshes of a file whose nodes and meshes all store the identity
/// transform: their box, every vertex position and every triangle, indices
/// into `positions`.
pub struct Render {
    pub extent: Extent,
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// The two 112-byte elements of the array at +184 of the first measured
/// object: three pairs of rows, f32 bits 0x7f7fffee then 0xff7fffee, and 16
/// zero bytes.
fn tail_pattern() -> Vec<u8> {
    let mut one = Vec::new();
    for _ in 0..3 {
        for w in [0x7f7f_ffeeu32, 0xff7f_ffee] {
            for _ in 0..4 {
                one.extend(w.to_le_bytes());
            }
        }
    }
    one.extend([0u8; 16]);
    [one.clone(), one].concat()
}

fn bit_width(n: u32) -> u32 {
    u32::BITS - n.leading_zeros()
}

/// Reads the shape at `i`, its data object and the geometry. Returns the
/// decoded geometry or the error code of the first view that failed.
pub(super) fn census(
    k: &mut Kind,
    c: &Container<'_>,
    i: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Geometry, String> {
    const SHAPE: &str = fo4_havok::COMPRESSED_MESH;
    let span = c.object_span(i).ok_or("InvalidHavokObjectSpan")?;
    let bytes = c.span_bytes(&span);
    for (rule, ok) in [
        ("bytes_0_to_16_zero", zero(bytes, 0, 16)),
        (
            "bytes_16_17_19_are_04_02_02",
            bytes
                .get(16..20)
                .is_some_and(|w| w[0] == 4 && w[1] == 2 && w[3] == 2),
        ),
        ("bytes_20_to_24_zero", zero(bytes, 20, 24)),
        ("bytes_28_to_32_zero", zero(bytes, 28, 32)),
        ("bytes_40_to_48_zero", zero(bytes, 40, 48)),
        ("u32_48_ffffffff", u32_at(bytes, 48) == Some(u32::MAX)),
        ("bytes_52_to_64_zero", zero(bytes, 52, 64)),
        (
            "empty_array_slots_56_and_72",
            u32_at(bytes, 68) == Some(0x8000_0000)
                && u32_at(bytes, 84) == Some(0x8000_0000)
                && zero(bytes, 72, 84),
        ),
        ("u32_88_ffffffff", u32_at(bytes, 88) == Some(u32::MAX)),
        ("bytes_92_to_96_zero", zero(bytes, 92, 96)),
        ("bytes_124_to_128_zero", zero(bytes, 124, 128)),
        ("bytes_148_to_160_zero", zero(bytes, 148, 160)),
    ] {
        k.object_check(SHAPE, rule, ok);
    }
    let shape = view(k, SHAPE, CompressedMesh::read(c, i, file, base))?;
    let props = c.objects.get(shape.properties).map_or("none", |o| o.class);
    k.count("mesh_properties_link", props);
    // hkRefCountedProperties: the link at +16 of its single element.
    let material = c
        .object_span(shape.properties)
        .and_then(|s| {
            let at = s.offset as usize + 16;
            c.object_pointers.iter().find(|l| l.source as usize == at)
        })
        .and_then(|l| {
            c.objects
                .binary_search_by_key(&l.destination, |o| o.offset)
                .ok()
        });
    let materials = material.and_then(|m| {
        k.count("mesh_material_link", c.objects[m].class);
        let at = c.objects[m].offset as usize + 24;
        u32_at(c.object_bytes(), at)
    });
    let padding = shape.arrays.iter().all(|a| {
        let end = a.storage as usize + (a.count * a.element) as usize;
        zero(shape.bytes(), end, end.div_ceil(16) * 16)
    });
    k.object_check(SHAPE, "storage_padding_zero", padding);
    let data = view(
        k,
        COMPRESSED_MESH_DATA,
        MeshData::read(c, shape.data, file, base, b),
    )?;
    data_checks(k, &data);
    k.object_check(
        SHAPE,
        "byte_18_equals_data_u32_68",
        u32::from(shape.key_bits) == data.counts[1],
    );
    k.object_check(
        SHAPE,
        "u32_144_equals_data_u32_72_plus_1",
        Some(shape.key_slots) == data.counts[2].checked_add(1),
    );
    // Slots and keys of the stored primitives: section << primitive bits | primitive.
    let n = data.sections.len() as u32;
    let widest = data
        .sections
        .iter()
        .map(|s| s.primitives.len() as u32)
        .max()
        .unwrap_or(0);
    let pb = bit_width(widest);
    k.object_check(
        COMPRESSED_MESH_DATA,
        "u32_68_is_bits(sections-1)+bits(widest section)+1",
        data.counts[1] == bit_width(n - 1) + pb + 1,
    );
    let mut slots = Vec::new();
    let mut keys = Vec::new();
    let mut triangles = 0u64;
    for (si, s) in data.sections.iter().enumerate() {
        for (p, j) in s.primitives.clone().enumerate() {
            let slot = ((si as u32) << pb) | p as u32;
            let raw = data.primitive(j).unwrap_or(PLACEHOLDER_PRIMITIVE);
            slots.push(slot);
            keys.push(slot << 1);
            if raw[2] != raw[3] {
                keys.push((slot << 1) | 1);
            }
            triangles += if raw != PLACEHOLDER_PRIMITIVE && raw[2] != raw[3] {
                2
            } else {
                1
            };
        }
    }
    slots.sort_unstable();
    keys.sort_unstable();
    k.object_check(
        SHAPE,
        "u32_120_is_last_primitive_slot_plus_1",
        slots.last().map(|x| x + 1) == Some(shape.primitive_slots),
    );
    k.object_check(
        COMPRESSED_MESH_DATA,
        "u32_72_is_last_key",
        keys.last().copied() == Some(data.counts[2]),
    );
    k.object_check(
        COMPRESSED_MESH_DATA,
        "u32_64_is_triangles_quad_2_other_1",
        u64::from(data.counts[0]) == triangles,
    );
    let subset = |which: usize, set: &[u32], limit: u32| {
        (0..limit).all(|x| shape.bit(which, x) != Some(true) || set.binary_search(&x).is_ok())
    };
    k.object_check(
        SHAPE,
        "bits_104_set_only_on_stored_primitive_slots",
        subset(0, &slots, shape.primitive_slots),
    );
    k.object_check(
        SHAPE,
        "bits_128_set_only_on_stored_keys",
        subset(1, &keys, shape.key_slots),
    );
    if let Some(m) = materials {
        let ok = data.sections.iter().all(|s| {
            s.runs
                .clone()
                .all(|r| data.run(r).is_some_and(|(v, _, _)| u32::from(v) < m))
        });
        k.object_check(COMPRESSED_MESH_DATA, "run_values_below_material_count", ok);
    }
    for s in &data.sections {
        k.count("mesh_section_flagged", &s.flagged.to_string());
    }
    if data.sections.iter().any(|s| s.byte92 != 0) {
        byte92_contrast(k, &data);
    }
    trees(k, &data, b);
    let g = view(k, "geometry", data.geometry(b))?;
    geometry_checks(k, &data, &g);
    if !g.records.is_empty() {
        records(k, &data, &g);
        record_contrasts(k, &data, &g);
    }
    Ok(g)
}

fn view<T>(k: &mut Kind, class: &str, r: fo4_core::Result<T>) -> Result<T, String> {
    let key = match &r {
        Ok(_) => "read".to_owned(),
        Err(e) => e.code.clone(),
    };
    k.count("view", &format!("{class}|{key}"));
    r.map_err(|e| e.code)
}

fn data_checks(k: &mut Kind, d: &MeshData<'_>) {
    const C: &str = COMPRESSED_MESH_DATA;
    let b = d.bytes();
    for (rule, ok) in [
        ("bytes_0_to_16_zero", zero(b, 0, 16)),
        ("u32_76_zero", u32_at(b, 76) == Some(0)),
        ("bytes_176_to_184_zero", zero(b, 176, 184)),
        ("bytes_200_to_208_zero", zero(b, 200, 208)),
        ("box_lanes_both_1", d.lanes == [1.0, 1.0]),
        (
            "box_lanes_union_of_section_lanes",
            d.sections
                .iter()
                .map(|s| s.lanes[0])
                .fold(f32::INFINITY, f32::min)
                == d.lanes[0]
                && d.sections
                    .iter()
                    .map(|s| s.lanes[1])
                    .fold(f32::NEG_INFINITY, f32::max)
                    == d.lanes[1],
        ),
        (
            "tail_is_fixed_pattern",
            d.array(184).is_some_and(|a| {
                let at = a.storage as usize;
                b.get(at..at + 224) == Some(&tail_pattern()[..])
            }),
        ),
    ] {
        k.object_check(C, rule, ok);
    }
    // Padding after every storage block, up to the next 16-byte boundary.
    let mut blocks = d.arrays.to_vec();
    blocks.extend(d.sections.iter().map(|s| s.tree));
    k.object_check(
        C,
        "storage_padding_zero",
        blocks.iter().filter(|a| a.count > 0).all(|a| {
            let end = a.storage as usize + (a.count * a.element) as usize;
            zero(b, end, end.div_ceil(16) * 16)
        }),
    );
    k.count("mesh_sections", &bucket(d.sections.len()));
    // Vertex, shared-index and primitive ranges of consecutive sections.
    let (mut v, mut sh, mut pr) = (0u32, 0u32, 0u32);
    let mut contiguous = true;
    for s in &d.sections {
        contiguous &= s.vertices.start == v && s.shared.start == sh && s.primitives.start == pr;
        v = s.vertices.end;
        sh = s.shared.end;
        pr = s.primitives.end;
    }
    let count = |f: u32| d.array(f).map_or(0, |a| a.count);
    k.object_check(
        C,
        "section_vertex_shared_primitive_ranges_follow_each_other_and_cover_arrays",
        contiguous && v == count(128) && sh == count(112) && pr == count(96),
    );
    let kind = match (
        d.sections.iter().any(|s| s.flagged),
        d.sections.iter().any(|s| s.byte92 != 0),
    ) {
        (false, false) => "none",
        (true, false) => "byte93_only",
        (false, true) => "byte92_only",
        (true, true) => "both",
    };
    k.count("mesh_objects_with_sections_byte92_or_93", kind);
    // The 5-byte nodes at +16: bit 7 of byte 3 marks an inner node; the
    // other nodes carry byte 4 | (byte 3 & 0x7f) << 8.
    let top: Vec<(bool, u32)> = d.array(16).map_or(Vec::new(), |a| {
        (0..a.count as usize)
            .filter_map(|j| a.element_at(j))
            .filter_map(|at| b.get(at..at + 5))
            .map(|n| {
                (
                    n[3] & 0x80 != 0,
                    u32::from(n[4]) | u32::from(n[3] & 0x7f) << 8,
                )
            })
            .collect()
    });
    k.object_check(
        C,
        "top_tree_preorder_inner_bit_and_leaves_number_each_section_once",
        tree_ok(&top, d.sections.len()),
    );
    for s in &d.sections {
        // The 4-byte nodes of each section: an odd byte 3 marks an inner
        // node; the other nodes carry byte 3 / 2.
        let a = s.tree;
        let nodes: Vec<(bool, u32)> = (0..a.count as usize)
            .filter_map(|j| a.element_at(j))
            .filter_map(|at| b.get(at + 3))
            .map(|x| (x & 1 == 1, u32::from(x >> 1)))
            .collect();
        k.object_check(
            C,
            "section_tree_preorder_odd_byte3_inner_and_leaves_number_each_primitive_once",
            tree_ok(&nodes, s.primitives.len()),
        );
        k.object_check(
            C,
            "section_primitives_at_most_128",
            s.primitives.len() <= 128,
        );
        let nv = s.vertices.len();
        k.object_check(C, "section_lanes_both_1", s.lanes == [1.0, 1.0]);
        k.object_check(
            C,
            "section_word90_zero_iff_no_shared_index",
            (s.word90 == 0) == s.shared.is_empty(),
        );
        k.count(
            "mesh_section_bytes_92_93",
            &format!("{}|{}", s.byte92, u8::from(s.flagged)),
        );
        if nv == 0 {
            let bits = |v: [f32; 3]| v.map(f32::to_bits);
            k.count(
                "mesh_section_without_vertices_origin_scale_bits",
                &format!("{:08x?}|{:08x?}", bits(s.origin), bits(s.scale)),
            );
        } else {
            let q: Vec<u32> = s
                .vertices
                .clone()
                .filter_map(|i| d.array(128).and_then(|a| a.element_at(i as usize)))
                .filter_map(|at| u32_at(b, at))
                .collect();
            let lo = [0, 11, 22].map(|sh| q.iter().map(|w| (w >> sh) & 0x7ff).min().unwrap_or(0));
            k.object_check(
                C,
                "section_vertex_fields_reach_0_on_each_axis",
                lo == [0; 3],
            );
            let hi = [(0, 0x7ff), (11, 0x7ff), (22, 0x3ff)]
                .map(|(sh, m)| q.iter().map(|w| (w >> sh) & m).max().unwrap_or(0));
            let full = hi
                .iter()
                .zip([0x7ff, 0x7ff, 0x3ff])
                .zip(s.scale)
                .all(|((h, m), sc)| *h == m || sc == 0.0);
            k.object_check(
                C,
                "section_vertex_fields_reach_max_on_each_axis_with_nonzero_step",
                full,
            );
        }
        let mut pattern = [0u64; 4];
        let mut record_pattern = 0u64;
        for j in s.primitives.clone() {
            let p = d.primitive(j).unwrap_or(PLACEHOLDER_PRIMITIVE);
            if p[1] == p[2] && p[2] == p[3] && u32::from(p[0]) >= nv as u32 {
                record_pattern += 1;
            }
            let kind = if p == PLACEHOLDER_PRIMITIVE {
                0
            } else if p[2] == p[3] {
                1
            } else if p[0] != p[1] && p[0] != p[2] && p[0] != p[3] && p[1] != p[2] && p[1] != p[3] {
                2
            } else {
                3
            };
            pattern[kind] += 1;
        }
        *k.totals_mut(if s.flagged {
            "mesh_primitives_flagged_last_three_equal_first_at_least_vertex_count"
        } else {
            "mesh_primitives_plain_last_three_equal_first_at_least_vertex_count"
        }) += record_pattern;
        let f = if s.flagged { "flagged" } else { "plain" };
        for (name, n) in ["placeholder", "third_eq_fourth", "four_distinct", "other"]
            .iter()
            .zip(pattern)
        {
            if n > 0 {
                k.count("mesh_primitive_kind_sections", &format!("{f}|{name}"));
            }
            *k.totals_mut(match (s.flagged, *name) {
                (false, "placeholder") => "mesh_primitives_plain_placeholder",
                (false, "third_eq_fourth") => "mesh_primitives_plain_third_eq_fourth",
                (false, "four_distinct") => "mesh_primitives_plain_four_distinct",
                (false, _) => "mesh_primitives_plain_other",
                (true, _) => "mesh_primitives_flagged",
            }) += n;
        }
    }
}

/// Largest absolute coordinate of a box, at least 1: the scale of the
/// relative tolerances.
fn scale(lo: &[f32; 3], hi: &[f32; 3]) -> f64 {
    lo.iter()
        .chain(hi)
        .fold(1f64, |m, x| m.max(f64::from(x.abs())))
}

fn geometry_checks(k: &mut Kind, d: &MeshData<'_>, g: &Geometry) {
    const C: &str = COMPRESSED_MESH_DATA;
    let mut used = vec![false; g.vertices.len()];
    for p in &g.primitives {
        for x in p.indices {
            if let Some(u) = used.get_mut(x as usize) {
                *u = true;
            }
        }
    }
    k.object_check(
        C,
        "every_vertex_used_by_a_primitive",
        used.iter().all(|u| *u),
    );
    *k.totals_mut("mesh_vertices_decoded") += g.vertices.len() as u64;
    *k.totals_mut("mesh_primitives_decoded") += g.primitives.len() as u64;
    *k.totals_mut("mesh_triangles_decoded") += g.triangles().count() as u64;
    *k.totals_mut("mesh_placeholders") += g.placeholders as u64;
    *k.totals_mut("mesh_records_decoded") += g.records.len() as u64;
    *k.totals_mut("mesh_record_vertices_decoded") += g.record_vertices.len() as u64;
    // Each section box against the box of what its primitives reach: the
    // vertices of 4-index primitives, and record vertices widened by the
    // radius. A record side may differ by half a step of its frame plus the
    // radius's lost low half (1/128 of it).
    let n = d.sections.len();
    let mut lo = vec![[(f64::INFINITY, 0f64); 3]; n];
    let mut hi = vec![[(f64::NEG_INFINITY, 0f64); 3]; n];
    let mut has_record = vec![false; n];
    for p in &g.primitives {
        let si = p.section as usize;
        let (Some(l), Some(h)) = (lo.get_mut(si), hi.get_mut(si)) else {
            continue;
        };
        for v in p.indices.iter().filter_map(|x| g.vertices.get(*x as usize)) {
            for a in 0..3 {
                let x = f64::from(v[a]);
                if x < l[a].0 {
                    l[a] = (x, 0.0);
                }
                if x > h[a].0 {
                    h[a] = (x, 0.0);
                }
            }
        }
    }
    for r in &g.records {
        let si = r.section as usize;
        let (Some(l), Some(h), Some(f)) = (lo.get_mut(si), hi.get_mut(si), has_record.get_mut(si))
        else {
            continue;
        };
        *f = true;
        let rad = f64::from(r.radius);
        let steps = [2047.0, 2047.0, 1023.0];
        for v in g
            .record_vertices
            .get(r.vertices.start as usize..r.vertices.end as usize)
            .unwrap_or(&[])
        {
            for a in 0..3 {
                let slack = (r.frame.1[a] - r.frame.0[a]) / steps[a] / 2.0 + rad / 128.0;
                let x = f64::from(v[a]);
                if x - rad < l[a].0 {
                    l[a] = (x - rad, slack);
                }
                if x + rad > h[a].0 {
                    h[a] = (x + rad, slack);
                }
            }
        }
    }
    for (j, s) in d.sections.iter().enumerate() {
        let (l, h) = (lo[j], hi[j]);
        if l[0].0 > h[0].0 {
            continue;
        }
        let m = scale(&s.min, &s.max);
        // Excess of each side over its slack, relative to the scale.
        let mut err = 0f64;
        let mut excess = 0f64;
        for a in 0..3 {
            let dl = (l[a].0 - f64::from(s.min[a])).abs();
            let dh = (h[a].0 - f64::from(s.max[a])).abs();
            err = err.max(dl.max(dh) / m);
            excess = excess.max((dl - l[a].1).max(dh - h[a].1) / m);
        }
        if has_record[j] {
            k.object_check(
                C,
                "section_box_is_box_of_primitive_vertices_and_record_vertices_plus_radius_within_half_step_rel_1e-5",
                excess <= 1e-5,
            );
            k.object_check(
                C,
                "section_box_is_box_of_primitive_vertices_and_record_vertices_plus_radius_rel_1e-3",
                err <= 1e-3,
            );
            continue;
        }
        k.object_check(
            C,
            "section_box_is_box_of_used_vertices_rel_1e-5",
            err <= 1e-5,
        );
        k.object_check(
            C,
            "section_box_is_box_of_used_vertices_rel_1e-6",
            err <= 1e-6,
        );
        let mx = k
            .object_maxima
            .entry("mesh_section_box_rel_error_x1e9")
            .or_default();
        *mx = (*mx).max((err * 1e9) as u64);
    }
}

/// One stored tree node: nibble bytes, inner flag, leaf number.
type Node = ([u8; 3], bool, u32);

/// Preorder decode with shrink a² / `den` and b² / `den`; for each leaf its
/// box, its parent's box and its nibbles. `None` when the nodes do not form
/// one tree numbering each leaf once.
#[allow(clippy::type_complexity)]
fn decode_with(
    nodes: &[Node],
    leaves: usize,
    root: MeshBox,
    den: f64,
) -> Option<Vec<(MeshBox, MeshBox, [u8; 3])>> {
    let mut out: Vec<Option<(MeshBox, MeshBox, [u8; 3])>> = vec![None; leaves];
    let mut pending = vec![root];
    for (nib, inner, leaf) in nodes {
        let parent = pending.pop()?;
        let own = shrink_with(&parent, *nib, den);
        if *inner {
            pending.push(own);
            pending.push(own);
        } else {
            let slot = out.get_mut(*leaf as usize)?;
            if slot.is_some() {
                return None;
            }
            *slot = Some((own, parent, *nib));
        }
    }
    if !pending.is_empty() {
        return None;
    }
    out.into_iter().collect()
}

fn shrink_with(parent: &MeshBox, nibbles: [u8; 3], den: f64) -> MeshBox {
    let (lo, hi) = parent;
    let mut out = (*lo, *hi);
    for (a, n) in nibbles.into_iter().enumerate() {
        let e = hi[a] - lo[a];
        let (x, y) = (f64::from(n >> 4), f64::from(n & 15));
        out.0[a] = lo[a] + x * x / den * e;
        out.1[a] = hi[a] - y * y / den * e;
    }
    out
}

/// Whether `v` lies in `bx` within `tol`.
fn within(bx: &MeshBox, v: [f32; 3], tol: f64) -> bool {
    (0..3).all(|a| f64::from(v[a]) >= bx.0[a] - tol && f64::from(v[a]) <= bx.1[a] + tol)
}

/// The box trees: the top tree against the section boxes, and each
/// section's tree against the vertices of its 4-index primitives, decoded
/// with the denominator 226 that the reader uses and with 225 for contrast.
fn trees(k: &mut Kind, d: &MeshData<'_>, b: &Budget) {
    const C: &str = COMPRESSED_MESH_DATA;
    let bytes = d.bytes();
    let m = scale(&d.min, &d.max);
    match d.section_boxes(b) {
        Ok(boxes) => {
            let ok = d
                .sections
                .iter()
                .zip(&boxes)
                .all(|(s, bx)| within(bx, s.min, 1e-6 * m) && within(bx, s.max, 1e-6 * m));
            k.object_check(C, "top_tree_leaf_boxes_contain_section_boxes_rel_1e-6", ok);
        }
        Err(e) => k.count("mesh_top_tree_error", &e.code),
    }
    for s in &d.sections {
        let reader = match d.primitive_boxes(s, b) {
            Ok(x) => x,
            Err(e) => {
                k.count("mesh_section_tree_error", &e.code);
                continue;
            }
        };
        let nodes: Vec<Node> = (0..s.tree.count as usize)
            .filter_map(|j| s.tree.element_at(j))
            .filter_map(|at| bytes.get(at..at + 4))
            .map(|n| ([n[0], n[1], n[2]], n[3] & 1 == 1, u32::from(n[3] >> 1)))
            .collect();
        let root = (s.min.map(f64::from), s.max.map(f64::from));
        let (Some(t226), Some(t225)) = (
            decode_with(&nodes, s.primitives.len(), root, 226.0),
            decode_with(&nodes, s.primitives.len(), root, 225.0),
        ) else {
            continue;
        };
        k.object_check(
            C,
            "section_tree_reader_boxes_equal_census_decode",
            reader.iter().zip(&t226).all(|(r, t)| *r == t.0),
        );
        let m = scale(&s.min, &s.max);
        let tol = 1e-6 * m;
        for (p, j) in s.primitives.clone().enumerate() {
            let Some(raw) = d.primitive(j) else { continue };
            if raw == PLACEHOLDER_PRIMITIVE || is_record_bytes(s, raw) {
                continue;
            }
            let v: Vec<[f32; 3]> = raw.iter().filter_map(|x| vertex(d, s, *x)).collect();
            if v.len() != 4 {
                continue;
            }
            for (rule, t) in [
                (
                    "section_tree_leaf_box_contains_primitive_vertices_d226_rel_1e-6",
                    &t226,
                ),
                (
                    "section_tree_leaf_box_contains_primitive_vertices_d225_rel_1e-6",
                    &t225,
                ),
            ] {
                let ok = t
                    .get(p)
                    .is_some_and(|x| v.iter().all(|q| within(&x.0, *q, tol)));
                k.object_check(C, rule, ok);
            }
            // Whether one more step of any nibble below 15 would cut a vertex.
            let Some((_, parent, nib)) = t226.get(p) else {
                continue;
            };
            let maximal = (0..3).all(|a| {
                [(4u8, true), (0u8, false)].iter().all(|(sh, low_side)| {
                    let n = (nib[a] >> sh) & 15;
                    if n == 15 {
                        return true;
                    }
                    let mut more = *nib;
                    more[a] = if *low_side {
                        (n + 1) << 4 | (nib[a] & 15)
                    } else {
                        (nib[a] & 0xf0) | (n + 1)
                    };
                    let bx = shrink_with(parent, more, 226.0);
                    !v.iter().all(|q| within(&bx, *q, 0.0))
                })
            });
            k.object_check(C, "section_tree_leaf_nibbles_maximal_d226", maximal);
        }
    }
}

/// Whether primitive bytes `raw` address a record (the reader's rule).
fn is_record_bytes(s: &MeshSection, raw: [u8; 4]) -> bool {
    s.flagged
        && raw[1] == raw[2]
        && raw[2] == raw[3]
        && u32::from(raw[0]) >= s.vertices.len() as u32
}

/// The position of primitive byte `x` of section `s`, read as the reader
/// does (shared index plus byte 92 << 16).
fn vertex(d: &MeshData<'_>, s: &MeshSection, x: u8) -> Option<[f32; 3]> {
    let nv = s.vertices.len() as u32;
    let x = u32::from(x);
    if x < nv {
        return d.section_vertex(s, s.vertices.start + x);
    }
    let t = u32::from(d.shared_index(s.shared.start + x - nv)?) + s.shared_high();
    d.shared_vertex(t)
        .filter(|_| t < d.array(144).map_or(0, |a| a.count))
}

/// Sections whose byte 92 is not zero: their shared references read without
/// the byte-92 high part, against the section box.
fn byte92_contrast(k: &mut Kind, d: &MeshData<'_>) {
    for s in d.sections.iter().filter(|s| s.byte92 != 0) {
        let nv = s.vertices.len() as u32;
        let m = scale(&s.min, &s.max);
        let bx = (s.min.map(f64::from), s.max.map(f64::from));
        for j in s.primitives.clone() {
            let Some(raw) = d.primitive(j) else { continue };
            if raw == PLACEHOLDER_PRIMITIVE || is_record_bytes(s, raw) {
                continue;
            }
            for x in raw.into_iter().filter(|x| u32::from(*x) >= nv) {
                let low = d
                    .shared_index(s.shared.start + u32::from(x) - nv)
                    .and_then(|t| d.shared_vertex(u32::from(t)));
                let key = match low {
                    Some(v) if within(&bx, v, 1e-5 * m) => "inside_section_box",
                    Some(_) => "outside_section_box",
                    None => "past_shared_vertices",
                };
                k.count("mesh_byte92_shared_reference_without_high_part", key);
            }
        }
    }
}

/// Vertex records of one decoded object: kinds and word counts, the left-over
/// word of odd counts, the repeated byte, and the use of the array at +144.
fn records(k: &mut Kind, d: &MeshData<'_>, g: &Geometry) {
    const C: &str = COMPRESSED_MESH_DATA;
    let elements = d.array(144).map_or(0, |a| a.count) as usize;
    // 1: a shared vertex of a 4-index primitive; 2: record words.
    let mut use_of = vec![0u8; elements];
    let mut overlap_without_byte92 = 0u64;
    for s in &d.sections {
        let nv = s.vertices.len() as u32;
        for j in s.primitives.clone() {
            let Some(raw) = d.primitive(j) else { continue };
            if raw == PLACEHOLDER_PRIMITIVE || is_record_bytes(s, raw) {
                continue;
            }
            for x in raw.into_iter().filter(|x| u32::from(*x) >= nv) {
                let t = d
                    .shared_index(s.shared.start + u32::from(x) - nv)
                    .map(|t| u32::from(t) + s.shared_high());
                if let Some(u) = t.and_then(|t| use_of.get_mut(t as usize)) {
                    *u |= 1;
                }
            }
        }
    }
    for r in &g.records {
        let count = r.vertices.len() as u32;
        let units = count.div_ceil(2);
        for e in r.offset..r.offset + units {
            if let Some(u) = use_of.get_mut(e as usize) {
                *u |= 2;
            }
        }
        let bucket = match count {
            0..=8 => count.to_string(),
            9..=32 => "9-32".into(),
            _ => "33+".into(),
        };
        k.count(
            "mesh_record_kind_words",
            &format!("{:02x}|{bucket}", r.kind),
        );
        let Some(s) = d.sections.get(r.section as usize) else {
            continue;
        };
        let nv = s.vertices.len() as u32;
        let rep = u32::from(r.repeated);
        k.count(
            "mesh_record_repeated_byte",
            if rep < nv {
                "below_vertex_count"
            } else if rep < s.index_limit() {
                "shared_position"
            } else {
                "past_index_limit"
            },
        );
        if count % 2 == 1 {
            let at = d
                .array(144)
                .and_then(|a| a.element_at((r.offset + units - 1) as usize));
            let w = at.and_then(|at| u32_at(d.bytes(), at));
            k.count(
                "mesh_record_odd_count_left_over_word",
                match w {
                    Some(0) => "zero",
                    Some(w) if w < 0x1_0000 => "below_65536",
                    Some(_) => "other",
                    None => "unread",
                },
            );
        }
        if s.byte92 != 0 {
            let low = r.offset - s.shared_high();
            for e in low..low + units {
                if use_of.get(e as usize).is_some_and(|u| *u & 1 == 1) {
                    overlap_without_byte92 += 1;
                }
            }
        }
    }
    k.object_check(
        C,
        "array_144_element_is_shared_vertex_or_record_word_not_both",
        use_of.iter().all(|u| *u != 3),
    );
    *k.totals_mut("mesh_array_144_elements_unused_in_objects_with_records") +=
        use_of.iter().filter(|u| **u == 0).count() as u64;
    *k.totals_mut("mesh_record_words_on_shared_vertices_without_byte92") += overlap_without_byte92;
}

/// The section-box rule of `geometry_checks` for sections with records,
/// with one reading changed at a time: no radius, the words of an odd count
/// read in stored order, and the section box as the frame.
fn record_contrasts(k: &mut Kind, d: &MeshData<'_>, g: &Geometry) {
    const C: &str = COMPRESSED_MESH_DATA;
    let steps = [2047.0, 2047.0, 1023.0];
    let n = d.sections.len();
    let mut plain = vec![([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]); n];
    for p in &g.primitives {
        let Some(bx) = plain.get_mut(p.section as usize) else {
            continue;
        };
        for v in p.indices.iter().filter_map(|x| g.vertices.get(*x as usize)) {
            for (a, x) in v.iter().enumerate() {
                bx.0[a] = bx.0[a].min(f64::from(*x));
                bx.1[a] = bx.1[a].max(f64::from(*x));
            }
        }
    }
    let word = |t: u32| {
        d.array(144)
            .and_then(|a| a.element_at((t / 2) as usize))
            .and_then(|at| u32_at(d.bytes(), at + 4 * (t % 2) as usize))
    };
    for (variant, rule) in [
        (0, "record_contrast_no_radius"),
        (1, "record_contrast_odd_count_words_in_stored_order"),
        (2, "record_contrast_section_box_as_frame"),
    ] {
        // Per section: (lo, slack), (hi, slack) per axis.
        let mut lo = vec![[(f64::INFINITY, 0f64); 3]; n];
        let mut hi = vec![[(f64::NEG_INFINITY, 0f64); 3]; n];
        let mut with = vec![false; n];
        for r in &g.records {
            let si = r.section as usize;
            let Some(s) = d.sections.get(si) else {
                continue;
            };
            let frame = if variant == 2 {
                (s.min.map(f64::from), s.max.map(f64::from))
            } else {
                r.frame
            };
            let rad = if variant == 0 {
                0.0
            } else {
                f64::from(r.radius)
            };
            let count = r.vertices.len() as u32;
            let (Some(l), Some(h), Some(w)) = (lo.get_mut(si), hi.get_mut(si), with.get_mut(si))
            else {
                continue;
            };
            *w = true;
            for i in 0..count {
                let t = if variant != 1 && count % 2 == 1 && i == count - 1 {
                    2 * r.offset + count
                } else {
                    2 * r.offset + i
                };
                let Some(x) = word(t) else { continue };
                let q = [x & 0x7ff, (x >> 11) & 0x7ff, x >> 22];
                for a in 0..3 {
                    let st = (frame.1[a] - frame.0[a]) / steps[a];
                    let v = frame.0[a] + f64::from(q[a]) * st;
                    let slack = st / 2.0 + f64::from(r.radius) / 128.0;
                    if v - rad < l[a].0 {
                        l[a] = (v - rad, slack);
                    }
                    if v + rad > h[a].0 {
                        h[a] = (v + rad, slack);
                    }
                }
            }
        }
        for (j, s) in d.sections.iter().enumerate() {
            if !with[j] {
                continue;
            }
            let m = scale(&s.min, &s.max);
            let mut excess = 0f64;
            for a in 0..3 {
                let (mut l, mut h) = (lo[j][a], hi[j][a]);
                if plain[j].0[a] < l.0 {
                    l = (plain[j].0[a], 0.0);
                }
                if plain[j].1[a] > h.0 {
                    h = (plain[j].1[a], 0.0);
                }
                let dl = (l.0 - f64::from(s.min[a])).abs() - l.1;
                let dh = (h.0 - f64::from(s.max[a])).abs() - h.1;
                excess = excess.max(dl.max(dh) / m);
            }
            k.object_check(C, rule, excess <= 1e-5);
        }
    }
}

/// Largest extent of any section box on any axis.
pub(super) fn widest_section(d: &MeshData<'_>) -> f64 {
    d.sections
        .iter()
        .flat_map(|s| (0..3).map(move |a| f64::from(s.max[a]) - f64::from(s.min[a])))
        .fold(0f64, f64::max)
}

/// Decoded triangles of a body that stores no placement against the render
/// triangles of its file: each collision vertex is matched to the nearest
/// render position scaled by the ratio of the largest extents, within 2.5
/// steps of a 10-bit axis spanning `widest`.
pub(super) fn triangles_vs_render(k: &mut Kind, g: &Geometry, widest: f64, r: &Render) {
    const S: &str = "mesh_render_triangles";
    let collision = vertex_box(g);
    let size = |e: Extent| -> f64 {
        (0..3)
            .map(|a| f64::from(e.1[a]) - f64::from(e.0[a]))
            .fold(0f64, f64::max)
    };
    if size(collision) <= 1e-3 {
        k.count(S, "body|flat");
        return;
    }
    let q = size(r.extent) / size(collision);
    if !(69.9..=70.1).contains(&q) {
        k.count(S, "body|ratio_outside_69.9_70.1");
        return;
    }
    k.count(S, "body|compared");
    // Canonical render position per stored position.
    let mut canon: HashMap<[u32; 3], u32> = HashMap::new();
    let ids: Vec<u32> = r
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| *canon.entry(p.map(f32::to_bits)).or_insert(i as u32))
        .collect();
    let mut tris: HashMap<[u32; 3], [u32; 3]> = HashMap::new();
    for t in &r.triangles {
        let Some(c) = t
            .iter()
            .map(|x| ids.get(*x as usize).copied())
            .collect::<Option<Vec<u32>>>()
        else {
            continue;
        };
        let c = [c[0], c[1], c[2]];
        if c[0] == c[1] || c[1] == c[2] || c[0] == c[2] {
            continue;
        }
        let mut key = c;
        key.sort_unstable();
        tris.entry(key).or_insert(c);
    }
    let tol = (2.5 * widest / 1023.0).max(1e-4) * q;
    let cell = |p: [f64; 3]| p.map(|x| (x / tol).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for (p, i) in canon.iter() {
        let v = p.map(|b| f64::from(f32::from_bits(b)));
        grid.entry(cell(v)).or_default().push(*i);
    }
    let nearest: Vec<Option<u32>> = g
        .vertices
        .iter()
        .map(|v| {
            let p = v.map(|x| f64::from(x) * q);
            let c = cell(p);
            let mut best: Option<(f64, u32)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else {
                            continue;
                        };
                        for i in list {
                            let Some(rp) = r.positions.get(*i as usize) else {
                                continue;
                            };
                            let d2: f64 = (0..3).map(|a| (f64::from(rp[a]) - p[a]).powi(2)).sum();
                            // Ties go to the lower id, whatever the map order.
                            if d2 <= tol * tol
                                && best.is_none_or(|b| d2 < b.0 || (d2 == b.0 && *i < b.1))
                            {
                                best = Some((d2, *i));
                            }
                        }
                    }
                }
            }
            best.map(|b| b.1)
        })
        .collect();
    let has = |t: [u32; 3]| {
        let mut key = t;
        key.sort_unstable();
        tris.get(&key).copied()
    };
    // Same cyclic order as the render triangle.
    let same =
        |t: [u32; 3], r: [u32; 3]| (0..3).any(|s| [r[s], r[(s + 1) % 3], r[(s + 2) % 3]] == t);
    for p in &g.primitives {
        let Some(m) = p
            .indices
            .iter()
            .map(|x| nearest.get(*x as usize).copied().flatten())
            .collect::<Option<Vec<u32>>>()
        else {
            k.count(S, "primitive|vertex_unmatched");
            continue;
        };
        let [a, b, c, e] = [m[0], m[1], m[2], m[3]];
        let distinct = |v: &[u32]| {
            let mut w = v.to_vec();
            w.sort_unstable();
            w.dedup();
            w.len()
        };
        let stored = distinct(&p.indices);
        if distinct(&m) != stored {
            k.count(S, "primitive|vertices_merge");
            continue;
        }
        if !p.quad {
            let key = match has([a, b, c]) {
                Some(rt) if same([a, b, c], rt) => "triangle|found_same_winding",
                Some(_) => "triangle|found_opposite_winding",
                None => "triangle|not_found",
            };
            k.count(S, key);
            continue;
        }
        let ac = (has([a, b, c]), has([a, c, e]));
        let bd = (has([a, b, e]), has([b, c, e]));
        let ac_ok = ac.0.is_some() && ac.1.is_some();
        let bd_ok = bd.0.is_some() && bd.1.is_some();
        let v: Vec<[f64; 3]> = p
            .indices
            .iter()
            .filter_map(|x| g.vertices.get(*x as usize))
            .map(|v| v.map(f64::from))
            .collect();
        let planar = v.len() == 4 && {
            let sub = |x: [f64; 3], y: [f64; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
            let (e1, e2, e3) = (sub(v[1], v[0]), sub(v[2], v[0]), sub(v[3], v[0]));
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let nn = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            let len = |x: [f64; 3]| x.iter().map(|y| y * y).sum::<f64>().sqrt();
            let diag = len(e2).max(len(sub(v[3], v[1])));
            nn == 0.0
                || diag == 0.0
                || (e3.iter().zip(n).map(|(x, y)| x * y).sum::<f64>()).abs() / nn / diag <= 1e-2
        };
        let shape = if planar {
            "quad"
        } else {
            "quad_nonplanar_over_1pct"
        };
        k.count(
            S,
            &format!(
                "{shape}|diagonal_ac={}|diagonal_bd={}",
                u8::from(ac_ok),
                u8::from(bd_ok)
            ),
        );
        if let (true, false, (Some(t1), Some(t2))) = (ac_ok, bd_ok, ac) {
            let w = (same([a, b, c], t1), same([a, c, e], t2));
            k.count(
                S,
                &format!(
                    "quad_diagonal_ac_only|winding_abc_same={}|winding_acd_same={}",
                    u8::from(w.0),
                    u8::from(w.1)
                ),
            );
        }
    }
}

/// Whether `nodes` (inner flag, leaf value) read in stored order form one
/// binary tree in preorder (each inner node followed by its two subtrees)
/// whose leaves carry 0..`leaves` once each.
fn tree_ok(nodes: &[(bool, u32)], leaves: usize) -> bool {
    let mut need = 1usize;
    let mut seen = vec![false; leaves];
    for (inner, value) in nodes {
        if need == 0 {
            return false;
        }
        need -= 1;
        if *inner {
            need += 2;
        } else {
            match seen.get_mut(*value as usize) {
                Some(s) if !*s => *s = true,
                _ => return false,
            }
        }
    }
    need == 0 && seen.iter().all(|s| *s)
}

fn bucket(n: usize) -> String {
    match n {
        0..=4 => n.to_string(),
        5..=16 => "5-16".into(),
        17..=256 => "17-256".into(),
        _ => "257+".into(),
    }
}

/// Box of the vertices of a decoded mesh.
pub(super) fn vertex_box(g: &Geometry) -> Extent {
    let mut bx = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for v in &g.vertices {
        grow(&mut bx, *v);
    }
    bx
}

/// Widens `bx` to hold `v`.
pub(super) fn grow(bx: &mut Extent, v: [f32; 3]) {
    for (a, x) in v.into_iter().enumerate() {
        bx.0[a] = bx.0[a].min(x);
        bx.1[a] = bx.1[a].max(x);
    }
}

/// Records the ratio of the render box to the box of the decoded vertices
/// for a body that stores no placement and links the mesh directly.
pub(super) fn units(k: &mut Kind, collision: Extent, render: Extent) {
    let size =
        |e: Extent| -> [f64; 3] { std::array::from_fn(|a| f64::from(e.1[a]) - f64::from(e.0[a])) };
    let (c, r) = (size(collision), size(render));
    let largest = |v: [f64; 3]| v.iter().copied().fold(0f64, f64::max);
    if largest(c) <= 1e-3 || largest(r) <= 1e-3 {
        k.count("mesh_render_ratio_largest_extent", "flat");
        return;
    }
    let q = largest(r) / largest(c);
    k.count("mesh_render_ratio_largest_extent", &ratio_bin(q));
    let fine = if (69.95..70.05).contains(&q) {
        format!("{q:.3}")
    } else {
        "outside_69.95_70.05".into()
    };
    k.count("mesh_render_ratio_largest_extent_fine", &fine);
    let per_axis: Vec<f64> = (0..3)
        .filter(|a| c[*a] > 1e-3 && r[*a] > 1e-3)
        .map(|a| r[a] / c[a])
        .collect();
    if per_axis.len() == 3 {
        let mn = per_axis.iter().copied().fold(f64::INFINITY, f64::min);
        let mx = per_axis.iter().copied().fold(0f64, f64::max);
        let key = if mx <= mn * 1.05 {
            format!("three_axes_within_5pct|{}", ratio_bin((mn + mx) / 2.0))
        } else {
            "axes_differ".into()
        };
        k.count("mesh_render_ratio_per_axis", &key);
    }
}

fn ratio_bin(q: f64) -> String {
    if !(0.01..1000.0).contains(&q) {
        return if q < 0.01 { "below_0.01" } else { "above_1000" }.into();
    }
    // Two significant digits.
    let e = q.log10().floor();
    let m = (q / 10f64.powf(e - 1.0)).round() * 10f64.powf(e - 1.0);
    format!("{m:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preorder_trees() {
        let leaf = |v| (false, v);
        let inner = (true, 0);
        assert!(tree_ok(&[leaf(0)], 1));
        assert!(tree_ok(&[inner, leaf(1), inner, leaf(0), leaf(2)], 3));
        assert!(!tree_ok(&[inner, leaf(1), leaf(1)], 2));
        assert!(!tree_ok(&[leaf(0), leaf(1)], 2));
        assert!(!tree_ok(&[inner, leaf(0)], 2));
        assert!(!tree_ok(&[inner, leaf(0), leaf(2)], 2));
    }

    #[test]
    fn ratio_bins_and_units() {
        assert_eq!(ratio_bin(69.991), "70.000");
        assert_eq!(ratio_bin(0.5), "0.500");
        assert_eq!(ratio_bin(1234.0), "above_1000");
        let mut k = Kind::default();
        units(
            &mut k,
            ([0.0; 3], [1.0, 2.0, 0.5]),
            ([0.0; 3], [70.0, 140.0, 35.0]),
        );
        units(&mut k, ([0.0; 3], [0.0; 3]), ([0.0; 3], [1.0; 3]));
        let row = |s: &str| k.counts.get(s).copied();
        assert_eq!(row("mesh_render_ratio_largest_extent\t70.000"), Some(1));
        assert_eq!(
            row("mesh_render_ratio_largest_extent_fine\t70.000"),
            Some(1)
        );
        assert_eq!(
            row("mesh_render_ratio_per_axis\tthree_axes_within_5pct|70.000"),
            Some(1)
        );
        assert_eq!(row("mesh_render_ratio_largest_extent\tflat"), Some(1));
    }

    #[test]
    fn tree_decode_with_denominators() {
        let root = ([0.0; 3], [226.0; 3]);
        let nodes = [
            ([0, 0, 0], true, 0),
            ([0x10, 0, 0], false, 1),
            ([0, 0x01, 0], false, 0),
        ];
        let t = decode_with(&nodes, 2, root, 226.0).unwrap();
        assert_eq!(t[1].0, ([1.0, 0.0, 0.0], [226.0; 3]));
        assert_eq!(t[0].0, ([0.0; 3], [226.0, 225.0, 226.0]));
        assert_eq!(t[0].1, root);
        assert!(decode_with(&nodes[..2], 2, root, 226.0).is_none());
        assert!(decode_with(&[([0; 3], false, 0), ([0; 3], false, 0)], 2, root, 226.0).is_none());
        let bx = shrink_with(&root, [0x11, 0, 0], 225.0);
        assert!((bx.0[0] - 226.0 / 225.0).abs() < 1e-12);
    }

    #[test]
    fn render_triangles_are_matched() {
        use fo4_havok::Primitive;
        let g = Geometry {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.5],
            ],
            primitives: vec![
                Primitive {
                    section: 0,
                    run_value: 0,
                    quad: true,
                    indices: [0, 1, 2, 3],
                },
                Primitive {
                    section: 0,
                    run_value: 0,
                    quad: false,
                    indices: [0, 2, 1, 1],
                },
            ],
            records: Vec::new(),
            record_vertices: Vec::new(),
            placeholders: 0,
        };
        let q = 70.0f32;
        let r = Render {
            extent: ([0.0; 3], [q, q, q / 2.0]),
            positions: g.vertices.iter().map(|v| v.map(|x| x * q)).collect(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        let mut k = Kind::default();
        triangles_vs_render(&mut k, &g, 1.0, &r);
        let row = |s: &str| {
            k.counts
                .get(&format!("mesh_render_triangles\t{s}"))
                .copied()
        };
        assert_eq!(row("body|compared"), Some(1));
        assert_eq!(
            row("quad_nonplanar_over_1pct|diagonal_ac=1|diagonal_bd=0"),
            Some(1)
        );
        assert_eq!(
            row("quad_diagonal_ac_only|winding_abc_same=1|winding_acd_same=1"),
            Some(1)
        );
        assert_eq!(row("triangle|found_opposite_winding"), Some(1));
    }

    #[test]
    fn tail_pattern_is_two_elements() {
        let t = tail_pattern();
        assert_eq!(t.len(), 224);
        assert_eq!(&t[..4], &0x7f7f_ffeeu32.to_le_bytes());
        assert_eq!(&t[112..116], &0x7f7f_ffeeu32.to_le_bytes());
        assert!(t[96..112].iter().all(|x| *x == 0));
    }
}
