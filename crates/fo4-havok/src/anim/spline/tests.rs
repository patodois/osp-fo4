use super::*;
use crate::{AnimationHeader, Array, SplineAnimation, SPLINE_KIND};

const AT: u64 = 5000;

fn header(tracks: u32) -> AnimationHeader {
    AnimationHeader {
        object: 0,
        kind: SPLINE_KIND,
        duration: 1.0,
        transform_tracks: tracks,
        float_tracks: 0,
        reference_frame: None,
        annotation_tracks: Array {
            field: 40,
            count: 0,
            storage: 0,
            element: 24,
        },
    }
}

fn anim(data: &[u8], float_offset: u32) -> SplineAnimation<'_> {
    SplineAnimation {
        header: header(2),
        frames: 31,
        frames_per_block: 256,
        mask_bytes: 8,
        block_duration: 8.5,
        block_inverse_duration: 1.0 / 8.5,
        frame_duration: 1.0 / 30.0,
        block_offsets: vec![0],
        float_offsets: vec![float_offset],
        data,
        data_at: AT,
    }
}

fn q40(fields: [u64; 3], index: u64, negative: bool) -> [u8; 5] {
    let v = fields[0] | fields[1] << 12 | fields[2] << 24 | index << 36 | u64::from(negative) << 38;
    let b = v.to_le_bytes();
    [b[0], b[1], b[2], b[3], b[4]]
}

fn q48(fields: [u16; 3], index: u16, negative: bool) -> [u8; 6] {
    let w = [
        fields[0] | (index & 1) << 15,
        fields[1] | (index >> 1) << 15,
        fields[2] | u16::from(negative) << 15,
    ];
    let mut out = [0u8; 6];
    for k in 0..3 {
        out[2 * k..2 * k + 2].copy_from_slice(&w[k].to_le_bytes());
    }
    out
}

/// Two tracks: (x static, y and z linear splines; rotation a linear spline
/// of two 40-bit values; no scale) and (static translation, static 48-bit
/// rotation, static scale x). Transform data ends at 88; 96 bytes in all.
fn block() -> Vec<u8> {
    let mut b = vec![0x45, 0x61, 0xf0, 0x00, 0x49, 0x07, 0x0f, 0x01];
    // Track 0 translation: 2 control points, degree 1, knots 0 0 30 30.
    b.extend(1u16.to_le_bytes());
    b.push(1);
    b.extend([0, 0, 30, 30]);
    b.push(0); // to 16
    for v in [1.5f32, 0.0, 10.0, -2.0, 2.0] {
        b.extend(v.to_le_bytes());
    }
    for v in [0u16, 0, 65535, 65535] {
        b.extend(v.to_le_bytes());
    }
    assert_eq!(b.len(), 44);
    // Track 0 rotation: identity, then 90 degrees about x.
    b.extend(1u16.to_le_bytes());
    b.push(1);
    b.extend([0, 0, 30, 30]);
    b.extend(q40([2047, 2047, 2047], 3, false));
    b.extend(q40([4094, 2047, 2047], 3, false));
    b.extend([0, 0, 0]); // to 64
                         // Track 1: translation, then 180 degrees about z, then scale x.
    for v in [3.0f32, 4.0, 5.0] {
        b.extend(v.to_le_bytes());
    }
    b.extend(q48([16383, 16383, 16383], 2, false));
    b.extend([0, 0]); // to 84
    b.extend(2.0f32.to_le_bytes());
    assert_eq!(b.len(), 88);
    b.resize(96, 0);
    b
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn quantised_rotations() {
    let q = Quantised::from_40(q40([2047, 2047, 2047], 3, false));
    assert_eq!(q.quaternion(), [0.0, 0.0, 0.0, 1.0]);
    let q = Quantised::from_40(q40([4094, 2047, 2047], 3, true)).quaternion();
    let h = std::f32::consts::FRAC_1_SQRT_2;
    assert!((q[0] - h).abs() < 1e-6 && (q[3] + h).abs() < 1e-6, "{q:?}");
    let q = Quantised::from_40(q40([2047, 0, 2047], 0, false));
    assert_eq!(q.index, 0);
    assert!((q.fields[1] + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    let q = Quantised::from_48(q48([16383, 16383, 16383], 2, false)).quaternion();
    assert_eq!(q, [0.0, 0.0, 1.0, 0.0]);
    let q = Quantised::from_48(q48([16383, 32766, 16383], 1, true));
    assert_eq!((q.index, q.negative), (1, true));
    // Fields whose squares sum above 1: the left-out component is 0.
    let q = Quantised::from_40(q40([0, 0, 0], 2, false));
    assert!(q.square_sum() > 1.0);
    assert_eq!(q.quaternion()[2], 0.0);
}

#[test]
fn de_boor_on_clamped_curves() {
    let knots = [0u8, 0, 0, 0, 30, 30, 30, 30];
    let pts = [[0.0], [3.0], [3.0], [0.0]];
    assert_eq!(de_boor(&knots, &pts, 3, 0.0), [0.0]);
    assert_eq!(de_boor(&knots, &pts, 3, 30.0), [0.0]);
    assert!((de_boor(&knots, &pts, 3, 15.0)[0] - 2.25).abs() < 1e-12);
    // Outside the range clamps to the ends.
    assert_eq!(de_boor(&knots, &pts, 3, 99.0), [0.0]);
    // Degree 1 interpolates linearly between knots.
    let knots = [0u8, 0, 10, 30, 30];
    let pts = [[0.0], [1.0], [5.0]];
    assert!((de_boor(&knots, &pts, 1, 5.0)[0] - 0.5).abs() < 1e-12);
    assert!((de_boor(&knots, &pts, 1, 20.0)[0] - 3.0).abs() < 1e-12);
    // Inconsistent counts give the first point rather than panicking.
    assert_eq!(de_boor(&knots, &pts[..2], 1, 5.0), [0.0]);
    assert_eq!(de_boor::<1>(&knots, &[], 1, 5.0), [0.0]);
}

#[test]
fn decodes_a_synthetic_block() {
    let data = block();
    let a = anim(&data, 88);
    assert_eq!(a.block_frames(0), 31);
    let tracks = a.decode_block(0, "x.hkx", &Budget::new(1 << 20)).unwrap();
    assert_eq!(tracks.len(), 2);
    let t0 = &tracks[0];
    assert_eq!(t0.translation.components[0], Component::Static(1.5));
    assert_eq!(
        t0.translation.components[1],
        Component::Spline {
            min: 0.0,
            max: 10.0
        }
    );
    assert_eq!(t0.scale.components, [Component::Absent; 3]);
    let p = t0.sample(15.0);
    assert!(close(&p.translation, &[1.5, 5.0, 0.0], 1e-9), "{p:?}");
    let s = std::f64::consts::FRAC_PI_8;
    assert!(
        close(&p.rotation, &[s.sin(), 0.0, 0.0, s.cos()], 1e-6),
        "{p:?}"
    );
    assert_eq!(p.scale, [1.0; 3]);
    let p = t0.sample(30.0);
    assert!(close(&p.translation, &[1.5, 10.0, 2.0], 1e-9));
    let t1 = &tracks[1];
    assert_eq!(t1.rotation, RotationTrack::Static([0.0, 0.0, 1.0, 0.0]));
    let p = t1.sample(7.0);
    assert_eq!(p.translation, [3.0, 4.0, 5.0]);
    assert_eq!(p.scale, [2.0, 1.0, 1.0]);
    let d = a.decode("x.hkx", &Budget::new(1 << 20)).unwrap();
    let poses = d.sample(0.5);
    assert!(close(&poses[0].translation, &[1.5, 5.0, 0.0], 1e-6));
    assert_eq!(d.locate(-1.0), (0, 0.0));
    assert_eq!(d.locate(99.0), (0, 30.0));
}

#[test]
fn locates_frames_across_blocks() {
    let d = DecodedSpline {
        blocks: vec![Vec::new(), Vec::new()],
        frames: 300,
        frames_per_block: 256,
        frame_duration: 0.5,
        duration: 149.5,
    };
    assert_eq!(d.locate(127.0), (0, 254.0));
    assert_eq!(d.locate(127.5), (1, 0.0));
    assert_eq!(d.locate(1000.0), (1, 44.0));
    assert_eq!(d.locate(f64::NAN), (0, 0.0));
}

fn code(data: &[u8], float_offset: u32) -> (String, u64) {
    let a = anim(data, float_offset);
    let e = a
        .decode_block(0, "x.hkx", &Budget::new(1 << 20))
        .unwrap_err();
    (e.code, e.source.offset)
}

#[test]
fn block_errors_are_named() {
    let good = block();
    let c = "InvalidHavokSplineBlock".to_string();
    // Rotation code 3.
    let mut b = good.clone();
    b[4] = 0x4d;
    assert_eq!(code(&b, 88), (c.clone(), AT + 4));
    // A component both static and spline.
    let mut b = good.clone();
    b[1] = 0x63;
    assert_eq!(code(&b, 88), (c.clone(), AT + 8));
    // Degree 0.
    let mut b = good.clone();
    b[10] = 0;
    assert_eq!(code(&b, 88), (c.clone(), AT + 8));
    // Knots that do not end at frame 30.
    let mut b = good.clone();
    b[14] = 29;
    assert_eq!(code(&b, 88), (c.clone(), AT + 11));
    // A non-zero alignment byte.
    let mut b = good.clone();
    b[15] = 1;
    assert_eq!(code(&b, 88), (c.clone(), AT + 15));
    // A non-finite static value.
    let mut b = good.clone();
    b[16..20].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert_eq!(code(&b, 88), (c.clone(), AT + 16));
    // The transform data does not end at the float offset.
    assert_eq!(code(&good, 92), (c.clone(), AT + 88));
    // Bytes after it that are not zero.
    let mut b = good.clone();
    b[95] = 1;
    assert_eq!(code(&b, 88), (c.clone(), AT + 88));
    // A block index out of range.
    let a = anim(&good, 88);
    let e = a
        .decode_block(1, "x.hkx", &Budget::new(1 << 20))
        .unwrap_err();
    assert_eq!(e.code, c);
}

#[test]
fn every_truncation_and_byte_change_is_handled() {
    let good = block();
    // Shorter than the transform data; the zero padding after it may be cut.
    for n in 0..88 {
        let a = anim(&good[..n], 88);
        assert!(a.decode_block(0, "x.hkx", &Budget::new(1 << 20)).is_err());
    }
    for at in 0..good.len() {
        for v in [0u8, 1, 3, 0x0f, 0x10, 0x80, 0xff] {
            let mut b = good.clone();
            b[at] = v;
            let a = anim(&b, 88);
            if let Ok(t) = a.decode_block(0, "x.hkx", &Budget::new(1 << 20)) {
                for x in &t {
                    let p = x.sample(12.5);
                    assert!(p.rotation.iter().all(|v| v.is_finite()));
                }
            }
        }
    }
    let a = anim(&good, 88);
    assert_eq!(
        a.decode_block(0, "x.hkx", &Budget::new(16))
            .unwrap_err()
            .code,
        "LimitExceeded"
    );
}
