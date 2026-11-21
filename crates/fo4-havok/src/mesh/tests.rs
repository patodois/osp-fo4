use crate::tests::{build, name_offset, read, Spec};
use crate::*;
use fo4_core::Budget;

/// Offsets inside the object bytes: the root, the shape and its data object.
const SHAPE: usize = 0x30;
const SHAPE_SPAN: usize = 192;
const DATA: usize = SHAPE + SHAPE_SPAN;
/// Storage of the data object, from its offset (see `data`).
const SECTION_AT: usize = 224;
const PRIMS_AT: usize = 336;
const SHARED_INDEX_AT: usize = 352;
const VERTS_AT: usize = 368;
const SHARED_AT: usize = 384;
const RUNS_AT: usize = 400;
const TAIL_AT: usize = 416;
const DATA_SPAN: usize = 640;
const F: &str = "synthetic.nif";

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn putf(b: &mut [u8], at: usize, v: &[f32]) {
    for (k, x) in v.iter().enumerate() {
        put(b, at + 4 * k, x.to_bits());
    }
}
fn slot(b: &mut [u8], at: usize, n: u32) {
    put(b, at + 8, n);
    put(b, at + 12, ARRAY_FLAG | n);
}

/// Two bitset words of one bit each, sized by the u32 at 120 and 144.
fn shape() -> Vec<u8> {
    let mut b = vec![0u8; SHAPE_SPAN];
    b[16..20].copy_from_slice(&[4, 2, 3, 2]);
    put(&mut b, 48, u32::MAX);
    put(&mut b, 68, ARRAY_FLAG);
    put(&mut b, 84, ARRAY_FLAG);
    put(&mut b, 88, u32::MAX);
    slot(&mut b, 104, 1);
    put(&mut b, 120, 2);
    slot(&mut b, 128, 1);
    put(&mut b, 144, 4);
    put(&mut b, 160, 0b11);
    put(&mut b, 176, 0b1101);
    b
}

/// One section over the box (-1, -1, 0)..(1, 1, 2): three section vertices
/// (-1, -1, 0), (1, -1, 0), (-1, 1, 1), one shared vertex (1, 1, 2), and the
/// primitives [0, 1, 2, 2] and [0, 1, 3, 2].
fn data() -> Vec<u8> {
    let mut b = vec![0u8; DATA_SPAN];
    for (field, n) in [
        (16, 1),
        (80, 1),
        (96, 2),
        (112, 1),
        (128, 3),
        (144, 1),
        (160, 1),
        (184, 2),
    ] {
        slot(&mut b, field, n);
    }
    putf(&mut b, 32, &[-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 1.0]);
    put(&mut b, 64, 3);
    put(&mut b, 68, 3);
    put(&mut b, 72, 3);
    let s = SECTION_AT;
    slot(&mut b, s, 3);
    putf(&mut b, s + 16, &[-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 1.0]);
    putf(
        &mut b,
        s + 48,
        &[-1.0, -1.0, 0.0, 2.0 / 2047.0, 2.0 / 2047.0, 1.0 / 1023.0],
    );
    put(&mut b, s + 72, 0);
    put(&mut b, s + 76, 3);
    put(&mut b, s + 80, 2);
    put(&mut b, s + 84, 1);
    put(&mut b, s + 88, (1 << 16) | (1 << 8) | 3);
    b[PRIMS_AT..PRIMS_AT + 8].copy_from_slice(&[0, 1, 2, 2, 0, 1, 3, 2]);
    for (k, w) in [0u32, 2047, (2047 << 11) | (1023 << 22)]
        .into_iter()
        .enumerate()
    {
        put(&mut b, VERTS_AT + 4 * k, w);
    }
    let top = ((1u64 << 21) - 1) | (((1u64 << 21) - 1) << 21) | (((1u64 << 22) - 1) << 42);
    b[SHARED_AT..SHARED_AT + 8].copy_from_slice(&top.to_le_bytes());
    put(&mut b, RUNS_AT, 2 << 24);
    for row in 0..14 {
        let w = if row % 7 == 6 {
            0
        } else if row % 7 % 2 == 0 {
            0x7f7f_ffee
        } else {
            0xff7f_ffee
        };
        for k in 0..4 {
            put(&mut b, TAIL_AT + 16 * row + 4 * k, w);
        }
    }
    b
}

fn spec(shape: Vec<u8>, data: Vec<u8>) -> Spec {
    let names = vec![
        ("hkClass", 1),
        ("RootThing", 2),
        (COMPRESSED_MESH, 3),
        (COMPRESSED_MESH_DATA, 4),
    ];
    let root = name_offset(&names, 1);
    let (s, d) = (SHAPE as u32, DATA as u32);
    let mut pointers = vec![[s + 104, s + 160], [s + 128, s + 176]];
    for (field, at) in [
        (16, 208),
        (80, SECTION_AT),
        (96, PRIMS_AT),
        (112, SHARED_INDEX_AT),
        (128, VERTS_AT),
        (144, SHARED_AT),
        (160, RUNS_AT),
        (184, TAIL_AT),
        (SECTION_AT, 320),
    ] {
        pointers.push([d + field as u32, d + at as u32]);
    }
    Spec {
        object_bytes: DATA + DATA_SPAN,
        content: vec![(SHAPE, shape), (DATA, data)],
        pointers,
        links: vec![[s + 32, 2, 0], [s + 96, 2, d]],
        objects: vec![
            [0, 0, root],
            [s, 0, name_offset(&names, 2)],
            [d, 0, name_offset(&names, 3)],
        ],
        root: [2, 0, 0, root],
        names,
    }
}

/// Absolute file offset of object byte `at` (base 1000 plus the `__data__` start).
fn abs(bytes: &[u8], at: usize) -> u64 {
    let c = read(bytes).unwrap();
    1000 + u64::from(c.data().unwrap().start) + at as u64
}

fn geometry(bytes: &[u8]) -> Result<Geometry> {
    let c = read(bytes)?;
    let d = MeshData::read(&c, 2, F, 1000, &Budget::new(1 << 20))?;
    d.geometry(&Budget::new(1 << 20))
}

/// The error of the shape and data reads and the geometry, in that order.
fn first_error(bytes: &[u8]) -> Option<(String, u64)> {
    let c = match read(bytes) {
        Ok(c) => c,
        Err(e) => return Some((e.code, e.source.offset)),
    };
    let b = Budget::new(1 << 20);
    let r = CompressedMesh::read(&c, 1, F, 1000)
        .and_then(|s| MeshData::read(&c, s.data, F, 1000, &b))
        .and_then(|d| d.geometry(&b).map(|_| ()));
    r.err().map(|e| (e.code, e.source.offset))
}

#[test]
fn reads_a_synthetic_mesh() {
    let bytes = build(&spec(shape(), data()));
    let c = read(&bytes).unwrap();
    let s = CompressedMesh::read(&c, 1, F, 1000).unwrap();
    assert_eq!((s.properties, s.data, s.key_bits), (0, 2, 3));
    assert_eq!((s.primitive_slots, s.key_slots), (2, 4));
    assert_eq!(s.arrays.map(|a| (a.storage, a.count)), [(160, 1), (176, 1)]);
    assert_eq!(
        (0..4).map(|k| s.bit(1, k)).collect::<Vec<_>>(),
        [Some(true), Some(false), Some(true), Some(true)]
    );
    assert_eq!((s.bit(0, 2), s.bit(2, 0)), (None, None));
    let d = MeshData::read(&c, 2, F, 1000, &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        (d.min, d.max, d.lanes),
        ([-1.0, -1.0, 0.0], [1.0, 1.0, 2.0], [1.0; 2])
    );
    assert_eq!(d.counts, [3, 3, 3]);
    let sec = &d.sections[0];
    assert_eq!(
        (
            sec.vertices.clone(),
            sec.shared.clone(),
            sec.primitives.clone()
        ),
        (0..3, 0..1, 0..2)
    );
    assert_eq!(
        (sec.word90, sec.flagged, sec.byte92, sec.index_limit()),
        (1, false, 0, 4)
    );
    assert_eq!((sec.tree.storage, sec.tree.count), (320, 3));
    assert_eq!(d.run(0), Some((0, 0, 2)));
    let g = d.geometry(&Budget::new(1 << 20)).unwrap();
    assert!(g.is_complete());
    let expected = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, 1.0, 1.0],
        [1.0, 1.0, 2.0],
    ];
    assert_eq!(g.vertices.len(), 4);
    for (v, e) in g.vertices.iter().zip(expected) {
        assert!(v.iter().zip(e).all(|(a, b)| (a - b).abs() < 1e-6), "{v:?}");
    }
    // The shared vertex takes the box corners exactly.
    assert_eq!(g.vertices[3], [1.0, 1.0, 2.0]);
    let idx: Vec<_> = g.primitives.iter().map(|p| p.indices).collect();
    assert_eq!(idx, [[0, 1, 2, 2], [0, 1, 3, 2]]);
    assert_eq!((g.primitives[1].section, g.primitives[1].run_value), (0, 0));
    assert_eq!(g.placeholders, 0);
    let e = d.geometry(&Budget::new(16)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
    let e = MeshData::read(&c, 2, F, 1000, &Budget::new(8)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
}

#[test]
fn placeholders_and_unsupported_sections() {
    let mut d = data();
    d[PRIMS_AT + 4..PRIMS_AT + 8].copy_from_slice(&PLACEHOLDER_PRIMITIVE);
    let g = geometry(&build(&spec(shape(), d))).unwrap();
    assert_eq!((g.primitives.len(), g.placeholders), (1, 1));
    for (byte, value) in [(93, 1u8), (92, 1), (92, 4)] {
        let mut d = data();
        d[SECTION_AT + byte] = value;
        let g = geometry(&build(&spec(shape(), d))).unwrap();
        assert_eq!(g.skipped, [0]);
        assert!(!g.is_complete());
        assert!(g.vertices.is_empty() && g.primitives.is_empty());
    }
}

type Change = Box<dyn Fn(&mut Vec<u8>, &mut Vec<u8>)>;

/// Each change of the shape or data bytes gives `code` at object byte `at`.
fn check(cases: Vec<(&str, Change, usize)>) {
    for (k, (code, change, at)) in cases.into_iter().enumerate() {
        let (mut s, mut d) = (shape(), data());
        change(&mut s, &mut d);
        let bytes = build(&spec(s, d));
        let found = first_error(&bytes);
        assert_eq!(found, Some((code.to_string(), abs(&bytes, at))), "case {k}");
    }
}

#[test]
fn shape_errors() {
    const C: &str = "InvalidHavokCompressedMesh";
    check(vec![
        // One word for 33 bits.
        (C, Box::new(|s, _| put(s, 120, 33)), SHAPE + 112),
        (C, Box::new(|s, _| put(s, 144, 0)), SHAPE + 136),
        (C, Box::new(|s, _| put(s, 116, 1)), SHAPE + 116),
    ]);
    // The data link pointing at the root.
    let mut sp = spec(shape(), data());
    sp.links[1][2] = 0;
    let bytes = build(&sp);
    assert_eq!(
        first_error(&bytes),
        Some(("InvalidHavokCompressedMesh".into(), abs(&bytes, SHAPE + 96)))
    );
    // No properties link.
    let mut sp = spec(shape(), data());
    sp.links.remove(0);
    let bytes = build(&sp);
    assert_eq!(
        first_error(&bytes),
        Some(("InvalidHavokCompressedMesh".into(), abs(&bytes, SHAPE + 32)))
    );
    // Wrong class and index.
    let bytes = build(&spec(shape(), data()));
    let c = read(&bytes).unwrap();
    assert_eq!(
        CompressedMesh::read(&c, 2, F, 1000).unwrap_err().code,
        "UnexpectedHavokClass"
    );
    let b = Budget::new(1 << 20);
    assert_eq!(
        MeshData::read(&c, 1, F, 1000, &b).unwrap_err().code,
        "UnexpectedHavokClass"
    );
    assert_eq!(
        MeshData::read(&c, 3, F, 1000, &b).unwrap_err().code,
        "InvalidHavokObjectSpan"
    );
}

#[test]
fn data_errors() {
    const C: &str = "InvalidHavokCompressedMeshData";
    let s = DATA + SECTION_AT;
    check(vec![
        // Top tree of two nodes for one section (the storage still fits).
        (C, Box::new(|_, d| slot(d, 16, 2)), DATA + 24),
        (C, Box::new(|_, d| slot(d, 184, 1)), DATA + 192),
        (C, Box::new(|_, d| putf(d, 48, &[-2.0])), DATA + 48),
        (C, Box::new(|_, d| putf(d, 36, &[f32::NAN])), DATA + 36),
        // Section box wider than the object box.
        (C, Box::new(|_, d| putf(d, SECTION_AT + 36, &[1.5])), s + 20),
        // Object box wider than the union of the section boxes.
        (C, Box::new(|_, d| putf(d, 56, &[3.0])), DATA + 32),
        (C, Box::new(|_, d| put(d, SECTION_AT + 88, 2)), s + 88),
        (
            C,
            Box::new(|_, d| put(d, SECTION_AT + 92, 0x1_0000)),
            s + 92,
        ),
        (C, Box::new(|_, d| put(d, SECTION_AT + 80, 0)), s + 80),
        (C, Box::new(|_, d| put(d, SECTION_AT + 80, 1)), s + 8),
        // Primitives past the array.
        (
            C,
            Box::new(|_, d| put(d, SECTION_AT + 80, (1 << 8) | 2)),
            s + 80,
        ),
        (C, Box::new(|_, d| put(d, SECTION_AT + 72, 1)), s + 72),
        (C, Box::new(|_, d| put(d, SECTION_AT + 84, 0)), s + 84),
        (C, Box::new(|_, d| put(d, RUNS_AT, 1 << 24)), s + 84),
        (
            C,
            Box::new(|_, d| put(d, RUNS_AT, (2 << 24) | (1 << 16))),
            DATA + RUNS_AT + 2,
        ),
        (
            C,
            Box::new(|_, d| putf(d, SECTION_AT + 60, &[f32::NAN])),
            s + 48,
        ),
        (
            C,
            Box::new(|_, d| putf(d, SECTION_AT + 64, &[-1.0])),
            s + 52,
        ),
        // A section vertex outside the section box.
        (
            C,
            Box::new(|_, d| putf(d, SECTION_AT + 60, &[4.0 / 2047.0])),
            DATA + VERTS_AT + 4,
        ),
        (
            C,
            Box::new(|_, d| d[SHARED_INDEX_AT] = 1),
            DATA + SHARED_INDEX_AT,
        ),
        (C, Box::new(|_, d| d[PRIMS_AT + 6] = 4), DATA + PRIMS_AT + 6),
    ]);
    // Storage of the vertices moved by 16 bytes.
    let mut sp = spec(shape(), data());
    let k = sp
        .pointers
        .iter()
        .position(|p| p[0] == (DATA + 128) as u32)
        .unwrap();
    sp.pointers[k][1] += 16;
    let bytes = build(&sp);
    assert_eq!(first_error(&bytes).map(|e| e.0), Some(C.into()));
}

#[test]
fn every_shorter_span_is_an_error() {
    for end in (16..DATA_SPAN).step_by(16) {
        let mut sp = spec(shape(), data());
        let root = sp.objects[0][2];
        sp.objects.push([(DATA + end) as u32, 0, root]);
        let bytes = build(&sp);
        assert!(first_error(&bytes).is_some(), "data span {end}");
    }
    for end in (16..SHAPE_SPAN).step_by(16) {
        let mut sp = spec(shape(), data());
        let root = sp.objects[0][2];
        sp.objects.insert(2, [(SHAPE + end) as u32, 0, root]);
        let bytes = build(&sp);
        assert!(first_error(&bytes).is_some(), "shape span {end}");
    }
    assert_eq!(first_error(&build(&spec(shape(), data()))), None);
}

#[test]
fn single_byte_changes_never_panic() {
    let (s, d) = (shape(), data());
    for at in 0..s.len() + d.len() {
        for value in [0u8, 1, 2, 0x7f, 0x80, 0xff] {
            let (mut s2, mut d2) = (s.clone(), d.clone());
            if at < s.len() {
                s2[at] = value;
            } else {
                d2[at - s.len()] = value;
            }
            let _ = first_error(&build(&spec(s2, d2)));
        }
    }
}
