use super::*;
use fo4_nif::{Block, Field};

fn field(name: &'static str, value: Value) -> Field {
    Field {
        name,
        offset: 0,
        value,
    }
}

fn fl(name: &'static str, v: f32) -> Field {
    field(name, Value::FloatBits(v.to_bits()))
}

/// A node's fields: name, stored translation, rotation words of a turn of
/// `angle` about Z (rows acting on column vectors), scale 1 and a
/// collision link.
fn node_fields(name: u32, t: [f32; 3], angle: f32, collision: i32) -> Vec<Field> {
    let mut f = vec![field("Name", Value::StringIndex(name))];
    f.extend(t.map(|v| fl("Translation", v)));
    let (s, c) = angle.sin_cos();
    for v in [c, -s, 0., s, c, 0., 0., 0., 1.] {
        f.push(fl("Rotation", v));
    }
    f.push(fl("Scale", 1.));
    f.push(field("Collision", Value::Ref(collision)));
    f
}

/// Root (collision on the root) -> leaf at (1, 0, 0) stored turned by
/// `stored` about Z, and a second child; one transform controller on the
/// leaf, clamped from 0 to 2 s, its Z keys `z` at 0, 1 and 2 s (linear).
fn leaf_nif(stored: f32, z: [f32; 3], flags: u16) -> Nif<'static> {
    let u = |name: &'static str, v: u32| field(name, Value::U32(v));
    let node = |ty: u16, f: Vec<Field>, children: Vec<i32>| Block {
        type_index: ty,
        data: 0..0,
        decoded: Some(Data::Node {
            fields: f,
            children,
        }),
        raw_tail: 0..0,
        diagnostics: Vec::new(),
    };
    let block = |ty: u16, f: Vec<Field>| Block {
        type_index: ty,
        data: 0..0,
        decoded: Some(Data::Fields(f)),
        raw_tail: 0..0,
        diagnostics: Vec::new(),
    };
    let mut data = vec![u("Num Rotation Keys", 1), u("Rotation Type", 4)];
    for axis in 0..3 {
        if axis < 2 {
            data.extend([
                u("Num Keys", 1),
                u("Interpolation", 1),
                fl("Key Time", 0.),
                fl("Key Value", 0.),
            ]);
        } else {
            data.extend([u("Num Keys", 3), u("Interpolation", 1)]);
            for (t, v) in [0f32, 1., 2.].into_iter().zip(z) {
                data.extend([fl("Key Time", t), fl("Key Value", v)]);
            }
        }
    }
    data.extend([u("Num Keys", 0), u("Num Keys", 0)]);
    let unset = crate::rest::INVALID;
    Nif {
        version: 0x1402_0007,
        user_version: 12,
        bs_version: 130,
        exports: Vec::new(),
        block_types: vec![
            "NiNode".into(),
            "NiTransformController".into(),
            "NiTransformInterpolator".into(),
            "NiTransformData".into(),
        ],
        strings: vec![b"root".to_vec(), b"leaf".to_vec(), b"other".to_vec()],
        max_string_length: 5,
        groups: Vec::new(),
        roots: vec![0],
        blocks: vec![
            node(0, node_fields(0, [0.; 3], 0., 9), vec![1, 5]),
            node(0, node_fields(1, [1., 0., 0.], stored, -1), vec![]),
            block(
                1,
                vec![
                    field("Controller Flags", Value::U16(flags)),
                    fl("Frequency", 1.),
                    fl("Phase", 0.),
                    fl("Start Time", 0.),
                    fl("Stop Time", 2.),
                    field("Target", Value::Ref(1)),
                    field("Interpolator", Value::Ref(3)),
                ],
            ),
            block(
                2,
                vec![
                    fl("Translation", unset),
                    fl("Translation", unset),
                    fl("Translation", unset),
                    fl("Quaternion", unset),
                    fl("Quaternion", unset),
                    fl("Quaternion", unset),
                    fl("Quaternion", unset),
                    fl("Scale", unset),
                    field("Data", Value::Ref(4)),
                ],
            ),
            block(3, data),
            node(0, node_fields(2, [0., 5., 0.], 0., -1), vec![]),
        ],
        bytes: &[],
    }
}

const CLAMPED: u16 = 0x4c;
const QUARTER: f32 = std::f32::consts::FRAC_PI_2;

#[test]
fn a_clamped_turn_from_the_stored_pose_reads_as_a_leaf_with_its_farthest_key() {
    let b = Budget::new(1 << 20);
    let n = leaf_nif(0., [0., QUARTER, 0.1], CLAMPED);
    let l = read(&n, &b).unwrap().unwrap();
    assert_eq!((l.controller, l.node), (2, 1));
    assert_eq!(l.key_time, 1.);
    assert!((l.key_angle - f64::from(QUARTER)).abs() < 1e-6);
    // The leaf's draws turn about the leaf's origin, (1, 0, 0) in the
    // model: its point (1, 0, 0) of its own goes from (2, 0, 0) to (1, 1, 0).
    let m = key_motion(&n, &l, &[0, 1]).unwrap();
    let p = m.apply([2., 0., 0.]);
    assert!(
        (p[0] - 1.).abs() < 1e-6 && (p[1] - 1.).abs() < 1e-6,
        "{p:?}"
    );
    // The root alone, or another child: not moved by the leaf.
    assert_eq!(key_motion(&n, &l, &[0]), None);
    assert_eq!(key_motion(&n, &l, &[0, 5]), None);
}

#[test]
fn a_track_that_does_not_start_at_the_stored_pose_or_loops_or_moves_collision_is_no_leaf() {
    let b = Budget::new(1 << 20);
    // The first key away from the stored rotation.
    let n = leaf_nif(0.5, [0., QUARTER, 0.], CLAMPED);
    assert_eq!(
        read(&n, &b).unwrap(),
        Err("the start pose is not the stored pose")
    );
    // Looping (cycle type 0).
    let n = leaf_nif(0., [0., QUARTER, 0.], 0x48);
    assert_eq!(read(&n, &b).unwrap(), Err("cycle type not clamped"));
    // No key away from the stored pose.
    let n = leaf_nif(0., [0., 0., 0.], CLAMPED);
    assert!(read(&n, &b).unwrap().is_err());
    // The leaf bears collision.
    let mut n = leaf_nif(0., [0., QUARTER, 0.], CLAMPED);
    let Some(Data::Node { fields, .. }) = n.blocks[1].decoded.as_mut() else {
        unreachable!()
    };
    for f in fields.iter_mut().filter(|f| f.name == "Collision") {
        f.value = Value::Ref(9);
    }
    assert_eq!(
        read(&n, &b).unwrap(),
        Err("the turned node bears collision")
    );
    // A second controller.
    let mut n = leaf_nif(0., [0., QUARTER, 0.], CLAMPED);
    n.block_types.push("NiAlphaController".into());
    n.blocks.push(Block {
        type_index: 4,
        data: 0..0,
        decoded: Some(Data::Fields(Vec::new())),
        raw_tail: 0..0,
        diagnostics: Vec::new(),
    });
    assert_eq!(read(&n, &b).unwrap(), Err("not exactly one controller"));
}

#[test]
fn only_the_leafs_unruled_animation_blockers_of_the_model_are_set_aside() {
    let e = |code: &str, found: &str| Error::new(code, "m.nif", 0, "x", found);
    let model = readiness::Origin::Model;
    for t in LEAF_BLOCKS {
        let found = format!("{t};target_in_visual=Some(true);rest=transform-varies");
        assert!(aside(&e("UnsupportedAnimationRuntime", &found), model));
        // Not from the record, nor another code.
        assert!(!aside(
            &e("UnsupportedAnimationRuntime", &found),
            readiness::Origin::Record
        ));
        assert!(!aside(&e("UnsupportedNifBlock", &found), model));
    }
    assert!(!aside(
        &e(
            "UnsupportedAnimationRuntime",
            "NiAlphaController;target_in_visual=Some(true);rest=transform-varies"
        ),
        model
    ));
}

#[test]
fn a_transform_and_its_inverse_compose_to_the_identity() {
    let t = Transform::euler([10., -20., 30., 0.3, -0.2, 1.1], 1.5).unwrap();
    for m in [inverse(t).compose(t), t.compose(inverse(t))] {
        let id = Transform::IDENTITY;
        let near = (0..3).all(|i| {
            (0..3).all(|j| (m.rotation[i][j] - id.rotation[i][j]).abs() < 1e-5)
                && m.translation[i].abs() < 1e-4
        }) && (m.scale - 1.0).abs() < 1e-9;
        assert!(near, "{m:?}");
    }
}
