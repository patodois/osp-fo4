//! Synthetic skeleton NIF, Havok skeleton and clip, all built here.
use super::*;
use crate::tests::nif_blocks;
use fo4_core::Game;
use fo4_havok::{Curve, DecodedSpline, RotationTrack, Track, TrackPose, VectorTrack};

fn words(v: &mut Vec<u8>, w: &[u32]) {
    for x in w {
        v.extend(x.to_le_bytes());
    }
}
fn floats(v: &mut Vec<u8>, f: &[f32]) {
    for x in f {
        v.extend(x.to_le_bytes());
    }
}
fn node(name: u32, rows: [[f32; 3]; 3], t: [f32; 3], children: &[u32]) -> Vec<u8> {
    let mut v = Vec::new();
    words(&mut v, &[name, 0, u32::MAX, 0]);
    floats(&mut v, &t);
    for r in rows {
        floats(&mut v, &r);
    }
    floats(&mut v, &[1.]);
    words(&mut v, &[u32::MAX, children.len() as u32]);
    words(&mut v, children);
    v
}
const I: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
/// A quarter turn about z as rows acting on column vectors.
const RZ: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];

/// Root, BoneA at z 10 turned a quarter about z, then an unnamed-in-Havok
/// node Helper (2 along BoneA's x) whose child BoneB sits 3 further along.
fn skeleton_nif() -> Vec<u8> {
    nif_blocks(
        &[
            ("NiNode", node(0, I, [0.; 3], &[1])),
            ("NiNode", node(1, RZ, [0., 0., 10.], &[2])),
            ("NiNode", node(2, I, [2., 0., 0.], &[3])),
            ("NiNode", node(3, I, [3., 0., 0.], &[])),
        ],
        &[b"Root", b"BoneA", b"Helper", b"BoneB"],
    )
}

fn local(t: [f64; 3], q: [f64; 4]) -> TrackPose {
    TrackPose {
        translation: t,
        rotation: q,
        scale: [1.0; 3],
    }
}

/// Havok bones Root, BoneA and BoneB (parent BoneA, 5 along its x), in the
/// NIF's reference pose.
fn hk_skeleton() -> HkSkeleton {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    HkSkeleton {
        name: b"Root".to_vec(),
        bones: vec![b"Root".to_vec(), b"BoneA".to_vec(), b"BoneB".to_vec()],
        parents: vec![-1, 0, 1],
        reference: vec![
            local([0.; 3], [0., 0., 0., 1.]),
            local([0., 0., 10.], [0., 0., h, h]),
            local([5., 0., 0.], [0., 0., 0., 1.]),
        ],
    }
}

fn vector(flags: u8, v: f32) -> VectorTrack {
    use fo4_havok::Component::{Absent, Static};
    VectorTrack {
        flags,
        components: if flags == 0 {
            [Absent; 3]
        } else {
            [Static(0.0), Static(0.0), Static(v)]
        },
        curve: None,
    }
}

/// One track driving BoneA: z translation 10, rotation a linear curve from
/// a quarter to a half turn about z over 31 frames (one second).
fn clip() -> Clip {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let track = Track {
        mask: [0x45, 0x07, 0xf0, 0x00],
        translation: vector(0x07, 10.0),
        rotation: RotationTrack::Spline(Curve {
            degree: 1,
            knots: vec![0, 0, 30, 30],
            points: vec![[0., 0., s, s], [0., 0., 1., 0.]],
        }),
        scale: vector(0, 0.0),
    };
    Clip {
        skeleton_name: b"Root".to_vec(),
        tracks: vec![1],
        animation: DecodedSpline {
            blocks: vec![vec![track]],
            frames: 31,
            frames_per_block: 256,
            frame_duration: 1.0 / 30.0,
            duration: 1.0,
        },
    }
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

#[test]
fn the_reference_pose_reproduces_the_nif_rest_pose() {
    let b = Budget::new(1 << 20);
    let l = Limits::default();
    let bytes = skeleton_nif();
    let n = Nif::read(&bytes, "skel", Game::Fallout4, &l, &b).unwrap();
    let s = hk_skeleton();
    let rig = Rig::new(&n, "skel", &s, &l, &b).unwrap();
    assert_eq!(rig.matched(), 3);
    let rest = pose::Skeleton::read(&n, "skel", &l, &b).unwrap();
    let w = worlds(&s, &s.reference, "x").unwrap();
    let posed = rig.pose(&w);
    assert_eq!(posed.bones.len(), 4);
    for (name, a) in &rest.bones {
        assert!(
            close(a.translation, posed.bones[name].translation),
            "{name:?}"
        );
    }
    // Helper follows BoneA: (0, 2, 10); BoneB is the Havok bone at (0, 5, 10).
    assert!(close(
        posed.bones[&b"Helper"[..]].translation,
        [0., 2., 10.]
    ));
    assert!(close(posed.bones[&b"BoneB"[..]].translation, [0., 5., 10.]));
}

#[test]
fn a_clip_moves_tracked_bones_and_their_followers() {
    let b = Budget::new(1 << 20);
    let l = Limits::default();
    let bytes = skeleton_nif();
    let n = Nif::read(&bytes, "skel", Game::Fallout4, &l, &b).unwrap();
    let s = hk_skeleton();
    let c = clip();
    assert!(c.fits(&s));
    assert_eq!(c.duration(), 1.0);
    let rig = Rig::new(&n, "skel", &s, &l, &b).unwrap();
    // At 1 s BoneA is turned a half turn: x maps to -x.
    let locals = c.locals(&s, 1.0);
    assert_eq!(locals[0], s.reference[0]);
    assert_eq!(locals[2], s.reference[2]);
    let p = rig.pose(&worlds(&s, &locals, "x").unwrap());
    assert!(close(p.bones[&b"BoneB"[..]].translation, [-5., 0., 10.]));
    assert!(close(p.bones[&b"Helper"[..]].translation, [-2., 0., 10.]));
    // Halfway, an eighth turn further than the reference: (-5, 5, 0)/sqrt 2 + z 10.
    let p = rig.pose(&worlds(&s, &c.locals(&s, 0.5), "x").unwrap());
    let r = 5. * std::f64::consts::FRAC_1_SQRT_2;
    let got = p.bones[&b"BoneB"[..]].translation;
    // The control points are f32.
    assert!(
        (0..3).all(|k| (got[k] - [-r, r, 10.][k]).abs() < 1e-6),
        "{got:?}"
    );
    // Bone lengths stay what the reference pose gives.
    let a = p.bones[&b"BoneA"[..]].translation;
    let d = (0..3)
        .map(|k| (p.bones[&b"BoneB"[..]].translation[k] - a[k]).powi(2))
        .sum::<f64>();
    assert!((d.sqrt() - 5.).abs() < 1e-6);
}

#[test]
fn fit_scale_paths_and_rules() {
    let s = hk_skeleton();
    let mut c = clip();
    c.skeleton_name = b"Other".to_vec();
    assert!(!c.fits(&s));
    let mut c = clip();
    c.tracks = vec![3];
    assert!(!c.fits(&s));
    let mut c = clip();
    c.tracks = vec![1, 1];
    assert!(!c.fits(&s));
    let mut p = local([0.; 3], [0., 0., 0., 1.]);
    p.scale = [1.0, 1.0005, 1.0];
    assert_eq!(affine(&p, "x").unwrap().scale, 1.0);
    p.scale = [1.0, 1.5, 1.0];
    assert_eq!(affine(&p, "x").unwrap_err().code, "ActorClipScale");
    assert_eq!(
        skeleton_hkx_path(b"meshes/a/b/Skel.NIF"),
        Some(b"meshes/a/b/skel.hkx".to_vec())
    );
    assert_eq!(skeleton_hkx_path(b"meshes/a.kf"), None);
    let st = state(&Budget::new(1 << 20)).unwrap();
    assert!(st.ready_at(crate::RuleLevel::Hypotheses));
    assert!(!st.ready_at(crate::RuleLevel::Measured));
    assert_eq!(
        st.hypotheses(),
        [hypotheses::ACTOR_CLIP_POSE, hypotheses::ACTOR_CLIP_SKELETON]
    );
    let b = Budget::new(1 << 20);
    assert!(Clip::read(b"not a container", "x.hkx", &b).is_err());
    assert!(HkSkeleton::read(b"not a container", "x.hkx", &b).is_err());
}
