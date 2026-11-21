use crate::tests::{build, name_offset, read, Spec};
use crate::*;
use fo4_core::Budget;

/// Absolute file offset of the root: base 1000, `__data__` at 320.
const ABS: u64 = 1000 + 320;
const CODE: &str = "InvalidHavokPhysicsSystem";
const SPHERES: [u32; 2] = [496, 624];

/// (fixed bytes, +64 storage, +96 storage, end) of the generated root.
fn layout(ragdoll: bool) -> (usize, usize, usize, usize) {
    let fixed = if ragdoll { 144 } else { 128 };
    (fixed, fixed + 160, fixed + 352, fixed + 368)
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn putf(b: &mut [u8], at: usize, v: &[f32]) {
    for (k, x) in v.iter().enumerate() {
        put(b, at + 4 * k, x.to_bits());
    }
}

fn root() -> Vec<u8> {
    root_of(false)
}

/// Physics root of 496 bytes: slots at 16 (2 x 80 bytes at 128), 64 (2 x 96
/// at 288) and 96 (2 x 8 at 480); the others empty. A ragdoll root has an
/// empty slot at 128 and everything after it 16 bytes later.
fn root_of(ragdoll: bool) -> Vec<u8> {
    let (_, bodies, _, end) = layout(ragdoll);
    let mut b = vec![0u8; end];
    put(&mut b, 140, ARRAY_FLAG);
    for field in [16usize, 32, 48, 64, 80, 96] {
        let n = if matches!(field, 16 | 64 | 96) { 2 } else { 0 };
        put(&mut b, field + 8, n);
        put(&mut b, field + 12, ARRAY_FLAG | n);
    }
    for i in 0..2 {
        let e = bodies + 96 * i;
        put(&mut b, e + 8, 0x7fff_ffff);
        put(&mut b, e + 12, 0x7fff_ffff);
        b[e + 16] = 0xff;
        b[e + 18] = i as u8;
        putf(&mut b, e + 48, &[1.0 + i as f32, -2.0, 0.25]);
        let q = if i == 0 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [0.6, 0.0, 0.0, 0.8]
        };
        putf(&mut b, e + 64, &q);
    }
    b
}

fn sphere() -> Vec<u8> {
    let mut b = vec![0u8; 128];
    putf(&mut b, 20, &[0.5]);
    put(&mut b, 48, 4 | (16 << 16));
    for j in 0..4 {
        put(&mut b, 64 + 16 * j + 12, VERTEX_TAG);
    }
    b
}

fn spec_with(root: Vec<u8>, class: &'static str) -> Spec {
    let names = vec![("hkClass", 1), (class, 2), (SPHERE, 3)];
    let r = name_offset(&names, 1);
    let s = name_offset(&names, 2);
    let (fixed, bodies, list, end) = layout(class == RAGDOLL);
    let [a, b] = [end as u32, end as u32 + 128];
    let (fixed, bodies, list) = (fixed as u32, bodies as u32, list as u32);
    Spec {
        object_bytes: end + 256,
        content: vec![(0, root), (end, sphere()), (end + 128, sphere())],
        pointers: vec![[16, fixed], [64, bodies], [96, list]],
        links: vec![
            [bodies, 2, a],
            [bodies + 96, 2, b],
            [list, 2, a],
            [list + 8, 2, b],
        ],
        objects: vec![[0, 0, r], [a, 0, s], [b, 0, s]],
        root: [2, 0, 0, r],
        names,
    }
}

fn system(s: &Spec) -> Result<(usize, Vec<Body>)> {
    let bytes = build(s);
    let c = read(&bytes)?;
    let p = PhysicsSystem::read(&c, 0, "synthetic.nif", 1000, &Budget::new(1 << 20))?;
    Ok((p.arrays.len(), p.bodies))
}

fn err(s: &Spec) -> (String, u64) {
    let e = system(s).expect_err("must fail");
    (e.code, e.source.offset)
}

#[test]
fn reads_bodies_shapes_and_placements() {
    let (arrays, bodies) = system(&spec_with(root(), PHYSICS_SYSTEM)).unwrap();
    assert_eq!(arrays, 6);
    assert_eq!(bodies.len(), 2);
    assert_eq!((bodies[0].shape, bodies[1].shape), (1, 2));
    assert_eq!(bodies[1].shape_slot, 1);
    assert_eq!(bodies[1].translation, [2.0, -2.0, 0.25]);
    assert_eq!(bodies[1].rotation, [0.6, 0.0, 0.0, 0.8]);
    let bytes = build(&spec_with(root(), PHYSICS_SYSTEM));
    let c = read(&bytes).unwrap();
    let p = PhysicsSystem::read(&c, 0, "synthetic.nif", 1000, &Budget::new(1 << 20)).unwrap();
    assert_eq!(p.body_bytes(1).map(<[u8]>::len), Some(BODY));
    assert_eq!(p.body_bytes(2), None);
    assert_eq!(p.array(64).map(|a| (a.count, a.storage)), Some((2, 288)));
    assert_eq!(p.array(128), None);
    // The spheres the bodies link to read too.
    let s = Sphere::read(&c, bodies[0].shape, "synthetic.nif", 1000).unwrap();
    assert_eq!((s.center, s.radius), ([0.0; 3], 0.5));
}

#[test]
fn ragdoll_has_a_seventh_slot() {
    let s = spec_with(root_of(true), RAGDOLL);
    let (arrays, bodies) = system(&s).unwrap();
    assert_eq!((arrays, bodies.len()), (7, 2));
    // The physics layout has no slot at 128: its flag word is required there.
    let mut s = spec_with(root_of(true), RAGDOLL);
    put(&mut s.content[0].1, 140, 0);
    assert_eq!(err(&s), (CODE.into(), ABS + 140));
    // A first-table count that differs from the body count.
    let mut s = spec_with(root_of(true), RAGDOLL);
    put(&mut s.content[0].1, 24, 1);
    put(&mut s.content[0].1, 28, ARRAY_FLAG | 1);
    assert_eq!(err(&s), (CODE.into(), ABS + 24));
}

#[test]
fn errors_are_named_with_offsets() {
    type Change = Box<dyn Fn(&mut Spec)>;
    let cases: Vec<(Change, &str, u64)> = vec![
        // Flag word that is not 0x80000000 | count.
        (Box::new(|s| put(&mut s.content[0].1, 76, 2)), CODE, 76),
        // A count without a storage pointer.
        (
            Box::new(|s| {
                put(&mut s.content[0].1, 40, 1);
                put(&mut s.content[0].1, 44, ARRAY_FLAG | 1);
            }),
            CODE,
            32,
        ),
        // A storage pointer for an empty slot.
        (Box::new(|s| s.pointers.push([32, 112])), CODE, 32),
        // Storage running into the next block: three bodies of 96 bytes.
        (
            Box::new(|s| {
                put(&mut s.content[0].1, 72, 3);
                put(&mut s.content[0].1, 76, ARRAY_FLAG | 3);
            }),
            CODE,
            72,
        ),
        // Storage past the span end: 3 shape-list entries end at 504.
        (
            Box::new(|s| {
                put(&mut s.content[0].1, 104, 3);
                put(&mut s.content[0].1, 108, ARRAY_FLAG | 3);
            }),
            CODE,
            104,
        ),
        // Body without its shape link.
        (
            Box::new(|s| {
                s.links.remove(1);
            }),
            CODE,
            384,
        ),
        // Shape-list position used twice.
        (Box::new(|s| s.content[0].1[384 + 18] = 0), CODE, 384 + 18),
        // Shape-list position out of range.
        (Box::new(|s| s.content[0].1[384 + 18] = 2), CODE, 384 + 18),
        // The shape list links the other object at that position.
        (
            Box::new(|s| {
                s.links[2][2] = SPHERES[1];
                s.links[3][2] = SPHERES[0];
            }),
            CODE,
            288,
        ),
        // Non-finite translation.
        (
            Box::new(|s| putf(&mut s.content[0].1, 384 + 52, &[f32::INFINITY])),
            CODE,
            384 + 52,
        ),
        // Rotation not of unit norm.
        (
            Box::new(|s| putf(&mut s.content[0].1, 384 + 64, &[0.6, 0.0, 0.0, 0.81])),
            CODE,
            384 + 64,
        ),
        // Wrong class.
        (
            Box::new(|s| s.objects[0][2] = s.objects[1][2]),
            "InvalidHavokRoot",
            0,
        ),
    ];
    for (k, (change, code, field)) in cases.into_iter().enumerate() {
        let mut s = spec_with(root(), PHYSICS_SYSTEM);
        change(&mut s);
        let (found, at) = err(&s);
        assert_eq!(found, code, "case {k}");
        if code == CODE {
            assert_eq!(at, ABS + field, "case {k}");
        }
    }
    let bytes = build(&spec_with(root(), PHYSICS_SYSTEM));
    let c = read(&bytes).unwrap();
    let b = Budget::new(1 << 20);
    let e = PhysicsSystem::read(&c, 1, "synthetic.nif", 1000, &b).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.source.offset),
        ("UnexpectedHavokClass", ABS + 496)
    );
    let e = PhysicsSystem::read(&c, 3, "synthetic.nif", 1000, &b).unwrap_err();
    assert_eq!(e.code, "InvalidHavokObjectSpan");
    // A budget too small for the slot list.
    let e = PhysicsSystem::read(&c, 0, "synthetic.nif", 1000, &Budget::new(8)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
}

#[test]
fn every_shorter_root_span_is_an_error() {
    for end in (16..496).step_by(16) {
        let mut s = spec_with(root(), PHYSICS_SYSTEM);
        let name = s.objects[1][2];
        s.objects.insert(1, [end, 0, name]);
        let bytes = build(&s);
        // The container refuses pointers that now cross objects; otherwise
        // the view refuses the shorter root.
        let Ok(c) = read(&bytes) else { continue };
        let e = PhysicsSystem::read(&c, 0, "synthetic.nif", 1000, &Budget::new(1 << 20));
        assert!(e.is_err(), "span {end}");
    }
}

#[test]
fn single_byte_changes_never_panic() {
    let good = root();
    for at in 0..good.len() {
        for value in [0u8, 1, 2, 3, 0x7f, 0x80, 0xff] {
            let mut r = good.clone();
            r[at] = value;
            if let Ok((_, bodies)) = system(&spec_with(r, PHYSICS_SYSTEM)) {
                assert!(bodies.len() <= 2);
            }
        }
    }
}
