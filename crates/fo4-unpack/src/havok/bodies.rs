//! Census of the body array of the top-level object, the simple shape views
//! and the array slots. Counts and small stored constants only.
use super::{f32_at, u16_at, u32_at, zero, Kind};
use fo4_core::Budget;
use fo4_havok::{
    orthonormal_error, segment_distance, Capsule, Compound, Container, ConvexPolytope, ConvexShape,
    PhysicsSystem, ScaledConvex, Sphere, CAPSULE, CONVEX, CONVEX_POLYTOPE, DYNAMIC_COMPOUND,
    INSTANCE, RAGDOLL, SCALED_CONVEX, SPHERE, STATIC_COMPOUND,
};
use serde::Serialize;

/// Element-size bounds of one (class, slot) from the first table: for every
/// array, the bytes from its destination to the next destination of the
/// object (or the span end) divided by its count.
#[derive(Default, Serialize, Clone, Copy)]
pub struct Gap {
    pub arrays: u64,
    pub count_max: u64,
    /// Largest element size that fits every array.
    pub element_max: u64,
    /// Smallest element size that leaves fewer than 16 bytes after every array.
    pub element_min: u64,
    /// Arrays whose gap is an exact multiple of `element_max`.
    pub exact: u64,
}

/// Records the gaps of one object's first-table entries; `ptrs` are the
/// (source, destination) pairs it owns, `lead` its smallest destination
/// minus its offset.
pub(super) fn gaps(
    k: &mut Kind,
    data: &[u8],
    class: &str,
    offset: u32,
    end: u32,
    ptrs: &[(u32, u32)],
) {
    let lead = ptrs.iter().map(|p| p.1).min().map_or(0, |d| d - offset);
    let mut dests: Vec<u32> = ptrs.iter().map(|p| p.1).collect();
    dests.sort_unstable();
    for (s, d) in ptrs {
        let Some(n) = u32_at(data, *s as usize + 8).filter(|n| *n > 0) else {
            continue;
        };
        let next = dests
            .get(dests.partition_point(|x| x <= d))
            .copied()
            .unwrap_or(end);
        let gap = u64::from(next - d);
        let n = u64::from(n);
        let key = if s - offset < lead {
            format!("{class}|+{:03}", s - offset)
        } else {
            format!("{class}|inside_storage")
        };
        let g = k.array_gaps.entry(key).or_insert(Gap {
            element_max: u64::MAX,
            ..Gap::default()
        });
        g.arrays += 1;
        g.count_max = g.count_max.max(n);
        g.element_max = g.element_max.min(gap / n);
        g.element_min = g.element_min.max(gap.saturating_sub(15).div_ceil(n));
        if gap % n == 0 && gap / n == g.element_max {
            g.exact += 1;
        }
    }
}

#[derive(Clone)]
enum Geo {
    Leaf,
    Scaled(usize),
    Compound(Vec<usize>),
    Missing,
}

fn record(k: &mut Kind, class: &str, r: Result<(), String>) -> bool {
    let key = match &r {
        Ok(()) => "read".to_owned(),
        Err(code) => code.clone(),
    };
    k.count("view", &format!("{class}|{key}"));
    r.is_ok()
}

/// Reads the views of one payload and counts their rules and the bodies.
pub(super) fn census(k: &mut Kind, c: &Container<'_>, file: &str, base: u64, body_ids: &[u32]) {
    let data = c.object_bytes();
    let b = Budget::new(1 << 26);
    let link = |source: usize| -> Option<usize> {
        let l = c
            .object_pointers
            .iter()
            .find(|l| l.source as usize == source)?;
        c.objects
            .binary_search_by_key(&l.destination, |o| o.offset)
            .ok()
    };
    let class_of = |i: Option<usize>| i.map_or("none", |i| c.objects[i].class);
    let mut geo = vec![Geo::Missing; c.objects.len()];
    for span in c.object_spans() {
        let (i, o) = (span.index, span.offset as usize);
        let bytes = c.span_bytes(&span);
        let class = span.class;
        match class {
            CONVEX_POLYTOPE => {
                if ConvexPolytope::read(c, i, file, base).is_ok() {
                    geo[i] = Geo::Leaf;
                }
            }
            SPHERE => {
                let r = Sphere::read(c, i, file, base);
                if let Ok(s) = &r {
                    geo[i] = Geo::Leaf;
                    k.object_check(class, "center_zero", s.center == [0.0; 3]);
                    k.object_check(class, "radius_positive", s.radius > 0.0);
                }
                record(k, class, r.map(|_| ()).map_err(|e| e.code));
                for (rule, ok) in [
                    ("bytes_0_to_16_zero", zero(bytes, 0, 16)),
                    ("bytes_28_to_48_zero", zero(bytes, 28, 48)),
                    ("bytes_52_to_64_zero", zero(bytes, 52, 64)),
                ] {
                    k.object_check(class, rule, ok);
                }
                k.count("u16_16_18", &format!("{class}|{}", u16_pair(bytes)));
            }
            CAPSULE => {
                let r = Capsule::read(c, i, file, base);
                if let Ok(s) = &r {
                    geo[i] = Geo::Leaf;
                    let far = s
                        .vertices
                        .iter()
                        .map(|v| segment_distance(*v, s.points[0], s.points[1]))
                        .fold(0f64, f64::max);
                    k.object_check(
                        class,
                        "vertices_within_0.02_radius_of_segment",
                        far <= 0.02 * f64::from(s.radius),
                    );
                    k.object_check(
                        class,
                        "vertex_coordinates_two_values_per_axis",
                        (0..3).all(|a| {
                            let mut v: Vec<u32> =
                                s.vertices.iter().map(|p| p[a].to_bits()).collect();
                            v.sort_unstable();
                            v.dedup();
                            v.len() == 2
                        }),
                    );
                }
                record(k, class, r.map(|_| ()).map_err(|e| e.code));
                for (rule, ok) in [
                    ("bytes_0_to_16_zero", zero(bytes, 0, 16)),
                    ("bytes_28_to_48_zero", zero(bytes, 28, 48)),
                    ("bytes_52_to_64_zero", zero(bytes, 52, 64)),
                    ("bytes_76_to_80_zero", zero(bytes, 76, 80)),
                ] {
                    k.object_check(class, rule, ok);
                }
                k.count("u16_16_18", &format!("{class}|{}", u16_pair(bytes)));
            }
            CONVEX => {
                let r = ConvexShape::read(c, i, file, base);
                if r.is_ok() {
                    geo[i] = Geo::Leaf;
                }
                record(k, class, r.map(|_| ()).map_err(|e| e.code));
            }
            SCALED_CONVEX => {
                let r = ScaledConvex::read(c, i, file, base);
                if let Ok(s) = &r {
                    geo[i] = Geo::Scaled(s.child);
                    k.count("scaled_convex_child", class_of(Some(s.child)));
                    k.object_check(
                        class,
                        "scale_uniform",
                        s.scale[0] == s.scale[1] && s.scale[1] == s.scale[2],
                    );
                    let child = &c.objects[s.child];
                    if child.class == CONVEX_POLYTOPE {
                        let own = u32_at(bytes, 20);
                        let theirs = u32_at(data, child.offset as usize + 20);
                        k.object_check(class, "f32_20_equals_polytope_child_f32_20", own == theirs);
                    }
                }
                record(k, class, r.map(|_| ()).map_err(|e| e.code));
                for (rule, ok) in [
                    ("bytes_0_to_16_zero", zero(bytes, 0, 16)),
                    ("bytes_28_to_48_zero", zero(bytes, 28, 48)),
                    ("bytes_56_to_64_zero", zero(bytes, 56, 64)),
                    ("bytes_96_to_112_zero", zero(bytes, 96, 112)),
                    (
                        "u32_76_high_24_bits_3f0000",
                        u32_at(bytes, 76).is_some_and(|w| w & 0xffff_ff00 == 0x3f00_0000),
                    ),
                    (
                        "f32_92_finite",
                        f32_at(bytes, 92).is_some_and(f32::is_finite),
                    ),
                ] {
                    k.object_check(class, rule, ok);
                }
                k.count("u16_16_18", &format!("{class}|{}", u16_pair(bytes)));
            }
            STATIC_COMPOUND | DYNAMIC_COMPOUND => {
                let r = Compound::read(c, i, file, base, &b);
                if let Ok(s) = &r {
                    geo[i] = Geo::Compound(s.instances.iter().map(|x| x.child).collect());
                    let n = s.instances.len();
                    k.count("compound_instances", &format!("{class}|{n}"));
                    let lo = o + 208;
                    let hi = lo + INSTANCE * n;
                    let links = c
                        .object_pointers
                        .iter()
                        .filter(|l| (lo..hi).contains(&(l.source as usize)))
                        .count();
                    k.object_check(class, "links_in_instances_equal_count", links == n);
                    k.count(
                        "compound_link192",
                        &format!("{class}|{}", class_of(link(o + 192))),
                    );
                    for (j, x) in s.instances.iter().enumerate() {
                        let e = 208 + INSTANCE * j;
                        k.count(
                            "compound_child",
                            &format!("{class}|{}", class_of(Some(x.child))),
                        );
                        for (rule, ok) in [
                            (
                                "instance_u32_88_is_ffffffff",
                                u32_at(bytes, e + 88) == Some(u32::MAX),
                            ),
                            (
                                "instance_bytes_84_and_92_to_128_zero",
                                zero(bytes, e + 84, e + 88) && zero(bytes, e + 92, e + 128),
                            ),
                            (
                                "instance_scale_uniform",
                                x.scale[0] == x.scale[1] && x.scale[1] == x.scale[2],
                            ),
                            (
                                "instance_f32_76_is_1",
                                u32_at(bytes, e + 76) == Some(0x3f80_0000),
                            ),
                            (
                                "instance_rotation_orthonormal_within_1e-5",
                                orthonormal_error(&x.rotation) <= 1e-5,
                            ),
                        ] {
                            k.object_check(class, rule, ok);
                        }
                    }
                }
                record(k, class, r.map(|_| ()).map_err(|e| e.code));
            }
            _ => {}
        }
    }
    let Some(root) = c.root() else { return };
    let class = root.class;
    // The top-level object is the first object in every measured payload.
    let r = PhysicsSystem::read(c, 0, file, base, &b);
    record(k, class, r.as_ref().map(|_| ()).map_err(|e| e.code.clone()));
    let Ok(sys) = r else { return };
    let count = |f: u32| sys.array(f).map_or(0, |a| a.count);
    let n = sys.bodies.len();
    for (rule, ok) in [
        (
            "n32_is_0_or_body_count",
            count(32) == 0 || count(32) as usize == n,
        ),
        ("n48_equals_n32", count(48) == count(32)),
        ("bytes_0_to_16_zero", zero(sys.bytes(), 0, 16)),
        ("bytes_112_to_128_zero", zero(sys.bytes(), 112, 128)),
    ] {
        k.object_check(class, rule, ok);
    }
    if class == RAGDOLL {
        k.object_check(class, "n128_equals_body_count", count(128) as usize == n);
        k.count("ragdoll_link120", class_of(link(120)));
    }
    // Bytes between each array's storage and the next storage block.
    let span_end = sys.bytes().len();
    let mut dests: Vec<usize> = sys
        .arrays
        .iter()
        .filter(|a| a.count > 0)
        .map(|a| a.storage as usize)
        .collect();
    dests.sort_unstable();
    for a in sys.arrays.iter().filter(|a| a.count > 0) {
        let lo = a.storage as usize + (a.count * a.element) as usize;
        let hi = dests
            .iter()
            .copied()
            .find(|d| *d > a.storage as usize)
            .unwrap_or(span_end);
        k.object_check(
            class,
            &format!("array_{:03}_bytes_after_storage_zero_and_below_16", a.field),
            zero(sys.bytes(), lo, hi) && hi - lo < 16,
        );
    }
    let mut shapes: Vec<usize> = sys.bodies.iter().map(|x| x.shape).collect();
    shapes.sort_unstable();
    shapes.dedup();
    k.object_check(class, "body_shapes_distinct", shapes.len() == n);
    k.object_check(
        class,
        "body_shape_is_a_shape_class",
        sys.bodies
            .iter()
            .all(|x| c.objects[x.shape].class.ends_with("Shape")),
    );
    let n48 = count(48);
    for body in &sys.bodies {
        let Some(e) = sys.body_bytes(body.index) else {
            continue;
        };
        let w12 = u32_at(e, 12).unwrap_or(0);
        for (rule, ok) in [
            ("body_u32_8_is_7fffffff", u32_at(e, 8) == Some(0x7fff_ffff)),
            (
                "body_u32_12_is_7fffffff_or_below_n48",
                w12 == 0x7fff_ffff || w12 < n48,
            ),
            (
                "body_bytes_16_17_ff_00",
                e.get(16..18) == Some(&[0xff, 0][..]),
            ),
            (
                "body_u16_18_is_index",
                body.shape_slot as usize == body.index,
            ),
            ("body_bytes_28_to_48_zero", zero(e, 28, 48)),
            ("body_u32_60_zero", u32_at(e, 60) == Some(0)),
            ("body_bytes_80_to_96_zero", zero(e, 80, 96)),
            (
                "body_rotation_unit_within_1e-6",
                (body
                    .rotation
                    .iter()
                    .map(|x| f64::from(*x).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - 1.0)
                    .abs()
                    <= 1e-6,
            ),
        ] {
            k.object_check(class, rule, ok);
        }
        let shape_class = c.objects[body.shape].class;
        k.count("body_shape", &format!("{class}|{shape_class}"));
        let key = match resolve(&geo, c, body.shape, 0) {
            Ok(()) => "geometry".to_owned(),
            Err(missing) => format!("no_geometry_below|{missing}"),
        };
        k.count("body_geometry", &format!("{class}|{shape_class}|{key}"));
    }
    if let Some(a) = sys.array(80) {
        for j in 0..a.count as usize {
            let Some(e) = a.element_at(j) else { continue };
            let bytes = sys.bytes();
            let (x, y) = (u32_at(bytes, e + 8), u32_at(bytes, e + 12));
            k.count(
                "constraint_class",
                &format!("{class}|{}", class_of(link(e))),
            );
            for (rule, ok) in [
                (
                    "constraint_u32_8_below_body_count",
                    x.is_some_and(|v| (v as usize) < n),
                ),
                (
                    "constraint_u32_12_below_body_count_or_7fffffff",
                    y.is_some_and(|v| (v as usize) < n || v == 0x7fff_ffff),
                ),
                (
                    "constraint_bytes_16_to_24_zero",
                    zero(bytes, e + 16, e + 24),
                ),
            ] {
                k.object_check(class, rule, ok);
            }
        }
    }
    // Body IDs of the bhkNPCollisionObject on this system against the body count.
    let below = body_ids.iter().filter(|id| (**id as usize) < n).count() as u64;
    let c2 = k.body_ids.entry(format!("{class}|objects")).or_default();
    c2.holds += below;
    c2.fails += body_ids.len() as u64 - below;
    let c3 = k.body_ids.entry(format!("{class}|systems")).or_default();
    if below as usize == body_ids.len() {
        c3.holds += 1;
    } else {
        c3.fails += 1;
    }
}

fn u16_pair(bytes: &[u8]) -> String {
    format!(
        "{}|{}",
        u16_at(bytes, 16).unwrap_or(0),
        u16_at(bytes, 18).unwrap_or(0)
    )
}

/// Ok when every shape reached from `i` has a geometry view; otherwise the
/// class of the first one without.
fn resolve<'a>(geo: &[Geo], c: &Container<'a>, i: usize, depth: usize) -> Result<(), &'a str> {
    let class = c.objects.get(i).map_or("none", |o| o.class);
    if depth > 4 {
        return Err(class);
    }
    match geo.get(i) {
        Some(Geo::Leaf) => Ok(()),
        Some(Geo::Scaled(child)) => resolve(geo, c, *child, depth + 1),
        Some(Geo::Compound(children)) => children
            .iter()
            .try_for_each(|ch| resolve(geo, c, *ch, depth + 1)),
        _ => Err(class),
    }
}
