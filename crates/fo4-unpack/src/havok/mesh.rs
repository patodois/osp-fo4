//! Census of `hknpCompressedMeshShape` and `hknpCompressedMeshShapeData`:
//! the views, the rules they do not enforce, small distributions, and the
//! extent of decoded meshes against the render meshes of the same file.
//! Counts and small stored constants only.
use super::{u32_at, zero, Kind};
use fo4_core::Budget;
use fo4_havok::{
    CompressedMesh, Container, Geometry, MeshData, COMPRESSED_MESH_DATA, PLACEHOLDER_PRIMITIVE,
};

/// Box of the render meshes of a file whose nodes and meshes all store the
/// identity transform.
pub type Extent = ([f32; 3], [f32; 3]);

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
    if data.sections.iter().any(|s| s.flagged) {
        flagged(k, &data);
    }
    let g = view(k, "geometry", data.geometry(b))?;
    geometry_checks(k, &data, &g);
    if g.is_complete() {
        k.count("view", "geometry|complete");
        Ok(g)
    } else {
        k.count("view", "geometry|sections_skipped");
        *k.totals_mut("mesh_primitives_decoded_in_objects_with_skipped_sections") +=
            g.primitives.len() as u64;
        *k.totals_mut("mesh_sections_skipped") += g.skipped.len() as u64;
        Err("UnsupportedHavokMeshSection".into())
    }
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
        for j in s.primitives.clone() {
            let p = d.primitive(j).unwrap_or(PLACEHOLDER_PRIMITIVE);
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

/// Flagged sections (byte 93 is 1). A primitive whose last three bytes are
/// equal and whose first byte is at least the section's vertex count is
/// called "special" here; its first byte minus the vertex count is a position
/// in the section's shared-index range. The other primitives are read like
/// those of an unflagged section and their shared vertices checked against
/// the section box.
fn flagged(k: &mut Kind, d: &MeshData<'_>) {
    const C: &str = COMPRESSED_MESH_DATA;
    for s in d.sections.iter().filter(|s| s.flagged) {
        let nv = s.vertices.len() as u32;
        let stream: Vec<u16> = s
            .shared
            .clone()
            .map(|j| d.shared_index(j).unwrap_or(u16::MAX))
            .collect();
        let mut records = Vec::new();
        let mut special = 0u64;
        for j in s.primitives.clone() {
            let Some(p) = d.primitive(j) else { continue };
            if p[1] == p[2] && p[2] == p[3] && p[0] != p[1] && u32::from(p[0]) >= nv {
                special += 1;
                let at = (u32::from(p[0]) - nv) as usize;
                records.push(at);
                let tag = stream.get(at).copied();
                let key = match tag {
                    Some(t) => format!("{:02x}", t & 0xff),
                    None => "outside_range".into(),
                };
                k.count("mesh_special_record_low_byte", &key);
                let units = tag.map(|t| u32::from(t >> 8).div_ceil(2));
                let off = stream.get(at + 1).map(|x| u32::from(*x));
                k.object_check(
                    C,
                    "special_record_blob_inside_array_144",
                    units
                        .zip(off)
                        .is_some_and(|(u, o)| o + u <= d.array(144).map_or(0, |a| a.count)),
                );
                k.object_check(
                    C,
                    "special_fourth_byte_below_vertex_count",
                    u32::from(p[1]) < nv,
                );
            }
        }
        *k.totals_mut("mesh_flagged_special_primitives") += special;
        records.sort_unstable();
        let covered = |x: usize| {
            records
                .iter()
                .any(|r| (*r..r.saturating_add(3)).contains(&x))
        };
        let m = s
            .min
            .iter()
            .chain(&s.max)
            .fold(1f64, |m, x| m.max(f64::from(x.abs())));
        for j in s.primitives.clone() {
            let Some(p) = d.primitive(j) else { continue };
            if p == PLACEHOLDER_PRIMITIVE
                || (p[1] == p[2] && p[2] == p[3] && p[0] != p[1] && u32::from(p[0]) >= nv)
            {
                continue;
            }
            for x in p.map(u32::from).into_iter().filter(|x| *x >= nv) {
                let at = (x - nv) as usize;
                let key = if covered(at) {
                    "inside_a_special_record"
                } else {
                    match stream.get(at).and_then(|t| d.shared_vertex(u32::from(*t))) {
                        None => "past_range_or_shared_vertices",
                        Some(v)
                            if (0..3).all(|a| {
                                f64::from(v[a]) >= f64::from(s.min[a]) - 1e-5 * m
                                    && f64::from(v[a]) <= f64::from(s.max[a]) + 1e-5 * m
                            }) =>
                        {
                            "shared_vertex_inside_section_box"
                        }
                        Some(_) => "shared_vertex_outside_section_box",
                    }
                };
                k.count("mesh_flagged_plain_shared_reference", key);
            }
        }
    }
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
    *k.totals_mut("mesh_placeholders") += g.placeholders as u64;
    // Each section box against the box of the vertices it uses.
    let mut boxes = vec![([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]); d.sections.len()];
    for p in &g.primitives {
        let Some(bx) = boxes.get_mut(p.section as usize) else {
            continue;
        };
        for x in p.indices {
            if let Some(v) = g.vertices.get(x as usize) {
                grow(bx, *v);
            }
        }
    }
    for (s, bx) in d.sections.iter().zip(&boxes) {
        if bx.0[0] > bx.1[0] {
            continue;
        }
        let m = s
            .min
            .iter()
            .chain(&s.max)
            .fold(1f64, |m, x| m.max(f64::from(x.abs())));
        let err = (0..3)
            .map(|a| {
                (f64::from(bx.0[a]) - f64::from(s.min[a]))
                    .abs()
                    .max((f64::from(bx.1[a]) - f64::from(s.max[a])).abs())
            })
            .fold(0f64, f64::max)
            / m;
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
    fn tail_pattern_is_two_elements() {
        let t = tail_pattern();
        assert_eq!(t.len(), 224);
        assert_eq!(&t[..4], &0x7f7f_ffeeu32.to_le_bytes());
        assert_eq!(&t[112..116], &0x7f7f_ffeeu32.to_le_bytes());
        assert!(t[96..112].iter().all(|x| *x == 0));
    }
}
