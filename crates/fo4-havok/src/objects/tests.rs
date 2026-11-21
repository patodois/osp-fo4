use crate::tests::{build, name_offset, read, Spec};
use crate::*;

const AT: usize = 0x30;
/// Absolute file offset of the polytope: base 1000, `__data__` at 320.
const ABS: u64 = 1000 + 320 + AT as u64;

/// A square pyramid: five vertices padded to eight rows, five faces padded
/// to eight plane rows, sixteen indices. Every byte is generated here.
fn pyramid() -> Vec<u8> {
    let mut b = vec![0u8; 384];
    let put16 =
        |b: &mut Vec<u8>, at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    for (at, count, rel) in [(48, 8u16, 32u16), (64, 8, 144), (68, 5, 268), (72, 16, 296)] {
        put16(&mut b, at, count);
        put16(&mut b, at + 2, rel);
    }
    let v = [
        [1.0f32, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    for j in 0..8 {
        let k = j.min(4);
        let at = 80 + 16 * j;
        for (c, x) in v[k].iter().enumerate() {
            b[at + 4 * c..at + 4 * c + 4].copy_from_slice(&x.to_le_bytes());
        }
        b[at + 12..at + 16].copy_from_slice(&(VERTEX_TAG | k as u32).to_le_bytes());
    }
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let planes = [
        [0.0f32, 0.0, -1.0, 0.0],
        [0.0, s, s, -s],
        [-s, 0.0, s, -s],
        [0.0, -s, s, -s],
        [s, 0.0, s, -s],
    ];
    for (j, p) in planes.iter().enumerate() {
        for (c, x) in p.iter().enumerate() {
            let at = 208 + 16 * j + 4 * c;
            b[at..at + 4].copy_from_slice(&x.to_le_bytes());
        }
    }
    for (j, (first, count)) in [(0u16, 4u8), (4, 3), (7, 3), (10, 3), (13, 3)]
        .iter()
        .enumerate()
    {
        put16(&mut b, 336 + 4 * j, *first);
        b[336 + 4 * j + 2] = *count;
        b[336 + 4 * j + 3] = 0x7f;
    }
    b[368..384].copy_from_slice(&[0, 3, 2, 1, 0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4]);
    b
}

fn spec_with(object: Vec<u8>, extra: Option<usize>) -> Spec {
    let names = vec![
        ("hkClass", 0x1111_1111),
        ("RootThing", 0x2222_2222),
        (CONVEX_POLYTOPE, 0x3333_3333),
    ];
    let root = name_offset(&names, 1);
    let shape = name_offset(&names, 2);
    let mut objects = vec![[0, 0, root], [AT as u32, 0, shape]];
    if let Some(at) = extra {
        objects.push([at as u32, 0, root]);
    }
    Spec {
        object_bytes: AT + object.len().div_ceil(16) * 16,
        content: vec![(AT, object)],
        pointers: vec![[0x10, 0x20]],
        links: vec![[0x18, 2, AT as u32]],
        objects,
        root: [2, 0, 0, root],
        names,
    }
}

fn polytope(bytes: &[u8]) -> Result<ConvexPolytope<'_>> {
    let c = read(bytes)?;
    // Rebuild the view on the same bytes; the container borrows `bytes`.
    let p = ConvexPolytope::read(&c, 1, "synthetic.nif", 1000)?;
    Ok(p)
}

fn err(object: Vec<u8>) -> (String, u64) {
    let bytes = build(&spec_with(object, None));
    let e = polytope(&bytes).expect_err("must fail");
    (e.code, e.source.offset)
}

/// A change applied to the generated polytope bytes.
type Change = Box<dyn Fn(&mut Vec<u8>)>;

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn reads_a_synthetic_polytope() {
    let bytes = build(&spec_with(pyramid(), None));
    let p = polytope(&bytes).unwrap();
    assert_eq!(
        (
            p.vertex_rows,
            p.vertex_count,
            p.plane_rows,
            p.face_count,
            p.index_count
        ),
        (8, 5, 8, 5, 16)
    );
    assert_eq!(p.offset, AT as u32);
    assert_eq!(p.bytes().len(), 384);
    assert_eq!(p.vertex(4), Some([0.0, 0.0, 1.0]));
    assert_eq!(p.vertex(5), None);
    assert_eq!(p.vertices().count(), 5);
    assert_eq!(p.plane(0), Some([0.0, 0.0, -1.0, 0.0]));
    assert_eq!(p.plane(5), None);
    assert_eq!(
        p.face(1),
        Some(Face {
            first: 4,
            count: 3,
            tag: 0x7f
        })
    );
    assert_eq!(p.face(5), None);
    assert_eq!(p.indices().len(), 16);
    // Every stored vertex is behind or on every face plane of this pyramid.
    for j in 0..p.face_count {
        let [a, b, c, d] = p.plane(j).unwrap();
        for [x, y, z] in p.vertices() {
            assert!(a * x + b * y + c * z + d <= 1e-6);
        }
    }
}

#[test]
fn class_and_index_errors_are_named() {
    let bytes = build(&spec_with(pyramid(), None));
    let c = read(&bytes).unwrap();
    let e = ConvexPolytope::read(&c, 0, "synthetic.nif", 1000).unwrap_err();
    assert_eq!(
        (e.code.as_str(), e.source.offset),
        ("UnexpectedHavokClass", 1320)
    );
    let e = ConvexPolytope::read(&c, 2, "synthetic.nif", 1000).unwrap_err();
    assert_eq!(e.code, "InvalidHavokObjectSpan");
}

#[test]
fn layout_errors_are_named() {
    let code = "InvalidHavokConvexPolytope".to_string();
    let cases: Vec<(Change, u64)> = vec![
        (Box::new(|b| put16(b, 50, 48)), 50),
        (Box::new(|b| put16(b, 66, 160)), 66),
        (Box::new(|b| put16(b, 70, 252)), 70),
        (Box::new(|b| put16(b, 74, 280)), 74),
        // Indices ending in an earlier 16-byte group than the span's last.
        (Box::new(|b| put16(b, 72, 0)), 72),
        // Plane rows that are not the face count rounded up to 4.
        (Box::new(|b| put16(b, 68, 3)), 64),
        // Vertex tag with other high bits.
        (Box::new(|b| b[80 + 16 * 2 + 14] = 0x3e), 80 + 32 + 12),
        // Non-finite coordinate.
        (
            Box::new(|b| b[84..88].copy_from_slice(&f32::NAN.to_le_bytes())),
            84,
        ),
        // A padding row that differs from the last tagged row.
        (Box::new(|b| b[80 + 16 * 6] ^= 1), 80 + 16 * 6),
        // First row not tagged 0: no tagged rows.
        (Box::new(|b| b[80 + 12] = 1), 92),
        // Index past the last tagged vertex.
        (Box::new(|b| b[370] = 5), 370),
    ];
    for (k, (change, field)) in cases.into_iter().enumerate() {
        let mut b = pyramid();
        change(&mut b);
        assert_eq!(err(b), (code.clone(), ABS + field), "case {k}");
    }
    // Eight rows of which only four are tagged: four is not rounded up to eight.
    let mut b = pyramid();
    let row3 = b[80 + 48..80 + 64].to_vec();
    for j in 4..8 {
        b[80 + 16 * j..96 + 16 * j].copy_from_slice(&row3);
    }
    assert_eq!(err(b), (code.clone(), ABS + 92));
    // Five vertex rows (not a multiple of 4) with the arrays still contiguous.
    let mut b = pyramid()[..160].to_vec();
    b.extend_from_slice(&pyramid()[208..384]);
    put16(&mut b, 48, 5);
    put16(&mut b, 66, 160 - 64);
    put16(&mut b, 70, 288 - 68);
    put16(&mut b, 74, 320 - 72);
    assert_eq!(err(b), (code, ABS + 48));
}

#[test]
fn every_shorter_span_is_a_named_error() {
    for end in (16..384).step_by(16) {
        let bytes = build(&spec_with(pyramid(), Some(AT + end)));
        let e = polytope(&bytes).expect_err("shorter span must fail");
        assert_eq!(e.code, "InvalidHavokConvexPolytope", "span {end}");
        assert!(e.source.offset >= ABS);
    }
    // A span longer than the stored arrays is refused too.
    let mut b = pyramid();
    b.resize(400, 0);
    assert_eq!(err(b), ("InvalidHavokConvexPolytope".into(), ABS + 72));
}

#[test]
fn single_byte_changes_never_panic() {
    let good = pyramid();
    for at in 0..good.len() {
        for value in [0u8, 1, 4, 0x3f, 0x7f, 0x80, 0xff] {
            let mut b = good.clone();
            b[at] = value;
            let bytes = build(&spec_with(b, None));
            if let Ok(p) = polytope(&bytes) {
                let _ = (p.vertices().count(), p.indices().len());
                for j in 0..p.plane_rows + 1 {
                    let _ = (p.plane(j), p.face(j));
                }
            }
        }
    }
}
