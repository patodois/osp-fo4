use crate::tests::{build, name_offset, read, Spec};
use crate::*;
use fo4_core::Budget;

/// The shape under test starts at 0x30: absolute file offset 1000 + 320 + 48.
const AT: usize = 0x30;
const ABS: u64 = 1000 + 320 + AT as u64;

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn putf(b: &mut [u8], at: usize, v: &[f32]) {
    for (k, x) in v.iter().enumerate() {
        put(b, at + 4 * k, x.to_bits());
    }
}
fn pair(b: &mut [u8], at: usize, count: u16, rel: u16) {
    put(b, at, u32::from(count) | (u32::from(rel) << 16));
}

fn sphere() -> Vec<u8> {
    let mut b = vec![0u8; SPHERE_SPAN];
    putf(&mut b, 20, &[0.25]);
    pair(&mut b, 48, 4, 16);
    for j in 0..4 {
        putf(&mut b, 64 + 16 * j, &[1.0, 2.0, 3.0]);
        put(&mut b, 76 + 16 * j, VERTEX_TAG);
    }
    b
}

/// Segment from (0, 0, -1) to (0, 0, 1), radius 0.5, and the box around it
/// widened by 0.01 on every axis.
fn capsule() -> Vec<u8> {
    let mut b = vec![0u8; CAPSULE_SPAN];
    putf(&mut b, 20, &[0.5]);
    for (at, (n, rel)) in [48, 64, 68, 72].into_iter().zip(CAPSULE_PAIRS) {
        pair(&mut b, at, n, rel);
    }
    putf(&mut b, 80, &[0.0, 0.0, -1.0, 1.0]);
    putf(&mut b, 96, &[0.0, 0.0, 1.0, 1.0]);
    for j in 0..8 {
        let s = |bit: usize| if j >> bit & 1 == 1 { 1.0f32 } else { -1.0 };
        putf(
            &mut b,
            112 + 16 * j,
            &[0.01 * s(0), 0.01 * s(1), 1.01 * s(2)],
        );
        put(&mut b, 124 + 16 * j, VERTEX_TAG | j as u32);
    }
    b
}

fn convex() -> Vec<u8> {
    let mut b = vec![0u8; 64 + 16 * 4];
    pair(&mut b, 48, 4, 16);
    for j in 0..4 {
        putf(&mut b, 64 + 16 * j, &[j as f32, 0.0, 1.0]);
        put(&mut b, 76 + 16 * j, VERTEX_TAG | j as u32);
    }
    b
}

/// Links at 48 to the root object of the synthetic payload.
fn scaled() -> Vec<u8> {
    let mut b = vec![0u8; SCALED_CONVEX_SPAN];
    putf(&mut b, 64, &[1.0, 2.0, 0.5, 0.5]);
    putf(&mut b, 80, &[0.0, -3.0, 4.0]);
    b
}

/// Two instances: identity and a quarter turn about the third axis.
fn compound() -> Vec<u8> {
    let mut b = vec![0u8; COMPOUND_FIXED + 2 * INSTANCE];
    put(&mut b, 104, 2);
    put(&mut b, 108, ARRAY_FLAG | 2);
    let rows = [
        [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    ];
    for (i, r) in rows.iter().enumerate() {
        let e = COMPOUND_FIXED + INSTANCE * i;
        for (k, row) in r.iter().enumerate() {
            putf(&mut b, e + 16 * k, row);
        }
        putf(&mut b, e + 48, &[i as f32, 0.0, 0.0]);
        putf(&mut b, e + 64, &[2.0, 2.0, 2.0, 1.0]);
        put(&mut b, e + 88, u32::MAX);
    }
    b
}

/// The root at 0, the shape at `AT`, and the links it needs to the root.
fn spec_with(class: &'static str, object: Vec<u8>, extra: Option<usize>) -> Spec {
    let names = vec![("hkClass", 1), ("RootThing", 2), (class, 3)];
    let root = name_offset(&names, 1);
    let shape = name_offset(&names, 2);
    let mut objects = vec![[0, 0, root], [AT as u32, 0, shape]];
    if let Some(at) = extra {
        objects.push([at as u32, 0, root]);
    }
    let at = AT as u32;
    let (pointers, links) = match class {
        SCALED_CONVEX => (vec![], vec![[at + 48, 2, 0]]),
        STATIC_COMPOUND | DYNAMIC_COMPOUND => (
            vec![[at + 96, at + COMPOUND_FIXED as u32]],
            vec![[at + 208 + 80, 2, 0], [at + 336 + 80, 2, 0]],
        ),
        _ => (vec![], vec![]),
    };
    Spec {
        object_bytes: AT + object.len().div_ceil(16) * 16,
        content: vec![(AT, object)],
        pointers,
        links,
        objects,
        root: [2, 0, 0, root],
        names,
    }
}

/// Reads the shape of `class` from `object`; Ok(()) or (code, offset).
fn view(class: &'static str, object: Vec<u8>, extra: Option<usize>) -> Result<()> {
    let s = spec_with(class, object, extra);
    view_spec(class, &s)
}

fn view_spec(class: &str, s: &Spec) -> Result<()> {
    let bytes = build(s);
    let c = read(&bytes)?;
    let f = "synthetic.nif";
    match class {
        SPHERE => Sphere::read(&c, 1, f, 1000).map(|_| ()),
        CAPSULE => Capsule::read(&c, 1, f, 1000).map(|_| ()),
        CONVEX => ConvexShape::read(&c, 1, f, 1000).map(|_| ()),
        SCALED_CONVEX => ScaledConvex::read(&c, 1, f, 1000).map(|_| ()),
        _ => Compound::read(&c, 1, f, 1000, &Budget::new(1 << 20)).map(|_| ()),
    }
}

fn err(class: &'static str, object: Vec<u8>) -> (String, u64) {
    let e = view(class, object, None).expect_err("must fail");
    (e.code, e.source.offset)
}

type Change = Box<dyn Fn(&mut Vec<u8>)>;
type Generator = fn() -> Vec<u8>;

fn all_shapes() -> [(&'static str, Generator); 6] {
    [
        (SPHERE, sphere),
        (CAPSULE, capsule),
        (CONVEX, convex),
        (SCALED_CONVEX, scaled),
        (STATIC_COMPOUND, compound),
        (DYNAMIC_COMPOUND, compound),
    ]
}

fn check(class: &'static str, good: Generator, code: &str, cases: Vec<(Change, usize)>) {
    for (k, (change, field)) in cases.into_iter().enumerate() {
        let mut b = good();
        change(&mut b);
        assert_eq!(
            err(class, b),
            (code.to_string(), ABS + field as u64),
            "{class} case {k}"
        );
    }
}

#[test]
fn reads_each_synthetic_shape() {
    let f = "synthetic.nif";
    let bytes = build(&spec_with(SPHERE, sphere(), None));
    let s = Sphere::read(&read(&bytes).unwrap(), 1, f, 1000).unwrap();
    assert_eq!((s.center, s.radius), ([1.0, 2.0, 3.0], 0.25));
    let bytes = build(&spec_with(CAPSULE, capsule(), None));
    let s = Capsule::read(&read(&bytes).unwrap(), 1, f, 1000).unwrap();
    assert_eq!(s.points, [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]]);
    assert_eq!((s.radius, s.vertices[7]), (0.5, [0.01, 0.01, 1.01]));
    let bytes = build(&spec_with(CONVEX, convex(), None));
    let c = read(&bytes).unwrap();
    let s = ConvexShape::read(&c, 1, f, 1000).unwrap();
    assert_eq!(s.vertices().collect::<Vec<_>>()[3], [3.0, 0.0, 1.0]);
    assert_eq!(s.vertex(4), None);
    let bytes = build(&spec_with(SCALED_CONVEX, scaled(), None));
    let s = ScaledConvex::read(&read(&bytes).unwrap(), 1, f, 1000).unwrap();
    assert_eq!(
        (s.child, s.scale, s.translation),
        (0, [1.0, 2.0, 0.5], [0.0, -3.0, 4.0])
    );
    for class in [STATIC_COMPOUND, DYNAMIC_COMPOUND] {
        let bytes = build(&spec_with(class, compound(), None));
        let c = read(&bytes).unwrap();
        let s = Compound::read(&c, 1, f, 1000, &Budget::new(1 << 20)).unwrap();
        assert_eq!((s.class, s.array.count, s.instances.len()), (class, 2, 2));
        assert_eq!(s.instances[1].rotation[1], [-1.0, 0.0, 0.0]);
        assert_eq!(
            (s.instances[1].translation, s.instances[1].scale),
            ([1.0, 0.0, 0.0], [2.0; 3])
        );
        assert_eq!(s.instances[0].child, 0);
        let e = Compound::read(&c, 1, f, 1000, &Budget::new(8)).unwrap_err();
        assert_eq!(e.code, "LimitExceeded");
    }
}

#[test]
fn class_and_index_errors_are_named() {
    let bytes = build(&spec_with(SPHERE, sphere(), None));
    let c = read(&bytes).unwrap();
    let f = "synthetic.nif";
    let e = Capsule::read(&c, 1, f, 1000).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.source.offset),
        ("UnexpectedHavokClass", ABS)
    );
    let e = Sphere::read(&c, 0, f, 1000).unwrap_err();
    assert_eq!(e.code, "UnexpectedHavokClass");
    for e in [
        Sphere::read(&c, 2, f, 1000).unwrap_err(),
        ScaledConvex::read(&c, 2, f, 1000).unwrap_err(),
        ConvexShape::read(&c, 2, f, 1000).unwrap_err(),
        Compound::read(&c, 2, f, 1000, &Budget::new(1 << 20)).unwrap_err(),
    ] {
        assert_eq!(e.code, "InvalidHavokObjectSpan");
    }
}

#[test]
fn sphere_errors() {
    check(
        SPHERE,
        sphere,
        "InvalidHavokSphere",
        vec![
            (Box::new(|b| putf(b, 20, &[-0.5])), 20),
            (Box::new(|b| putf(b, 20, &[f32::NAN])), 20),
            (Box::new(|b| pair(b, 48, 3, 16)), 48),
            (Box::new(|b| put(b, 76, VERTEX_TAG | 1)), 76),
            (Box::new(|b| putf(b, 68, &[f32::INFINITY])), 68),
            (Box::new(|b| b[64 + 16 * 3] ^= 1), 112),
        ],
    );
    let mut b = sphere();
    b.resize(SPHERE_SPAN + 16, 0);
    assert_eq!(err(SPHERE, b), ("InvalidHavokSphere".into(), ABS));
}

#[test]
fn capsule_errors() {
    check(
        CAPSULE,
        capsule,
        "InvalidHavokCapsule",
        vec![
            (Box::new(|b| putf(b, 20, &[0.0])), 20),
            (Box::new(|b| pair(b, 68, 5, 300)), 68),
            (Box::new(|b| putf(b, 92, &[0.5])), 92),
            (Box::new(|b| putf(b, 104, &[f32::NAN])), 104),
            (Box::new(|b| putf(b, 96, &[0.0, 0.0, -1.0])), 96),
            (Box::new(|b| put(b, 124 + 16 * 3, VERTEX_TAG)), 124 + 48),
            (Box::new(|b| putf(b, 112 + 16 * 2 + 4, &[f32::NAN])), 148),
            // A point outside the vertex box on the third axis.
            (Box::new(|b| putf(b, 104, &[1.5])), 104),
            // A vertex farther than the radius from the segment.
            (Box::new(|b| putf(b, 112 + 16 * 5, &[0.9])), 112 + 80),
        ],
    );
}

#[test]
fn convex_shape_errors() {
    check(
        CONVEX,
        convex,
        "InvalidHavokConvexShape",
        vec![
            (Box::new(|b| pair(b, 48, 4, 32)), 50),
            (Box::new(|b| pair(b, 48, 0, 16)), 48),
            (Box::new(|b| pair(b, 48, 5, 16)), 0),
            (Box::new(|b| put(b, 76 + 16 * 2, VERTEX_TAG | 3)), 76 + 32),
            (Box::new(|b| putf(b, 64, &[f32::NAN])), 64),
        ],
    );
}

#[test]
fn scaled_convex_errors() {
    check(
        SCALED_CONVEX,
        scaled,
        "InvalidHavokScaledConvex",
        vec![
            (Box::new(|b| putf(b, 68, &[0.0])), 68),
            (Box::new(|b| putf(b, 64, &[f32::NAN])), 64),
            (Box::new(|b| putf(b, 88, &[f32::INFINITY])), 88),
        ],
    );
    let mut s = spec_with(SCALED_CONVEX, scaled(), None);
    s.links.clear();
    let e = view_spec(SCALED_CONVEX, &s).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.source.offset),
        ("InvalidHavokScaledConvex", ABS + 48)
    );
}

#[test]
fn compound_errors() {
    let code = "InvalidHavokCompound";
    let e0 = COMPOUND_FIXED;
    let e1 = COMPOUND_FIXED + INSTANCE;
    check(
        STATIC_COMPOUND,
        compound,
        code,
        vec![
            (Box::new(|b| put(b, 108, ARRAY_FLAG | 3)), 108),
            // Three instances do not fit the span.
            (
                Box::new(|b| {
                    put(b, 104, 3);
                    put(b, 108, ARRAY_FLAG | 3);
                }),
                104,
            ),
            (Box::new(move |b| putf(b, e1 + 4, &[f32::NAN])), e1 + 4),
            (Box::new(move |b| putf(b, e1, &[0.0, 1.1, 0.0])), e1),
            // A reflection: one row negated.
            (Box::new(move |b| putf(b, e1 + 32, &[0.0, 0.0, -1.0])), e1),
            (Box::new(move |b| putf(b, e0 + 56, &[f32::NAN])), e0 + 56),
            (Box::new(move |b| putf(b, e0 + 68, &[-2.0])), e0 + 68),
        ],
    );
    // One instance in a span made for two.
    let mut b = compound();
    put(&mut b, 104, 1);
    put(&mut b, 108, ARRAY_FLAG | 1);
    assert_eq!(err(STATIC_COMPOUND, b), (code.into(), ABS + 104));
    // No instances.
    let mut s = spec_with(STATIC_COMPOUND, compound(), None);
    put(&mut s.content[0].1, 104, 0);
    put(&mut s.content[0].1, 108, ARRAY_FLAG);
    s.pointers.clear();
    let e = view_spec(STATIC_COMPOUND, &s).unwrap_err();
    assert_eq!((e.code.as_str(), e.source.offset), (code, ABS + 104));
    // Instance without its shape link.
    let mut s = spec_with(STATIC_COMPOUND, compound(), None);
    s.links.pop();
    let e = view_spec(STATIC_COMPOUND, &s).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.source.offset),
        (code, ABS + e1 as u64 + 80)
    );
    // Storage that does not start at byte 208.
    let mut s = spec_with(STATIC_COMPOUND, compound(), None);
    s.pointers[0][1] += 16;
    let e = view_spec(STATIC_COMPOUND, &s).unwrap_err();
    assert_eq!(e.code, code);
}

#[test]
fn every_shorter_span_is_an_error() {
    let shapes = all_shapes();
    for (class, good) in shapes {
        let len = good().len();
        for end in (16..len).step_by(16) {
            // Either the container refuses a link or pointer that now falls
            // in the next object, or the view refuses the shorter span.
            assert!(
                view(class, good(), Some(AT + end)).is_err(),
                "{class} span {end}"
            );
        }
        assert!(view(class, good(), None).is_ok(), "{class}");
    }
}

#[test]
fn single_byte_changes_never_panic() {
    let shapes = all_shapes();
    for (class, good) in shapes {
        let g = good();
        for at in 0..g.len() {
            for value in [0u8, 1, 2, 0x3f, 0x7f, 0x80, 0xff] {
                let mut b = g.clone();
                b[at] = value;
                let _ = view(class, b, None);
            }
        }
    }
}
