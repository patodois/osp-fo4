//! Synthetic tests: every byte is generated here. The payload builder writes
//! the container layout of `spec/havok-container.md` and the physics root and
//! sphere layouts of `spec/havok-bodies.md`.
use super::*;
use crate::tests::{fixture, nif_blocks, record, sub};
use fo4_havok::{
    ARRAY_FLAG, CLASS_NAME_MARKER, HEADER_LEN, LAYOUT, MAGIC, PHYSICS_SYSTEM, SECTION_NAMES,
    SPHERE, VERSION, VERTEX_TAG,
};
use fo4_plugin::typed::Store;
use fo4_vfs::Vfs;

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6 * (1.0 + b[k].abs()))
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn putf(b: &mut [u8], at: usize, v: &[f32]) {
    for (k, x) in v.iter().enumerate() {
        put(b, at + 4 * k, x.to_bits());
    }
}
fn pad16(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(16) {
        v.push(0xff);
    }
}

/// Payload: a physics root of 496 bytes with two bodies, each linking to
/// its own sphere (radius 0.5, centre `centers[i]`).
fn payload(centers: [[f32; 3]; 2]) -> Vec<u8> {
    let names: [(&str, u32); 3] = [("hkClass", 1), (PHYSICS_SYSTEM, 2), (SPHERE, 3)];
    let offset = |i: usize| -> u32 {
        names[..i]
            .iter()
            .map(|(n, _)| 5 + n.len() as u32 + 1)
            .sum::<u32>()
            + 5
    };
    let (bodies, list, end) = (288usize, 480usize, 496usize);
    let mut data = vec![0u8; end + 256];
    put(&mut data, 140, ARRAY_FLAG);
    for field in [16usize, 32, 48, 64, 80, 96] {
        let n = if matches!(field, 16 | 64 | 96) { 2 } else { 0 };
        put(&mut data, field + 8, n);
        put(&mut data, field + 12, ARRAY_FLAG | n);
    }
    for (i, center) in centers.iter().enumerate() {
        let e = bodies + 96 * i;
        put(&mut data, e + 8, 0x7fff_ffff);
        put(&mut data, e + 12, 0x7fff_ffff);
        data[e + 16] = 0xff;
        data[e + 18] = i as u8;
        putf(&mut data, e + 48, &[1.0 + i as f32, -2.0, 0.25]);
        let q = if i == 0 {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [0.6, 0.0, 0.0, 0.8]
        };
        putf(&mut data, e + 64, &q);
        let s = end + 128 * i;
        putf(&mut data, s + 20, &[0.5]);
        put(&mut data, s + 48, 4 | (16 << 16));
        for j in 0..4 {
            putf(&mut data, s + 64 + 16 * j, center);
            put(&mut data, s + 76 + 16 * j, VERTEX_TAG);
        }
    }
    let [a, b] = [end as u32, end as u32 + 128];
    let (b0, l0) = (bodies as u32, list as u32);
    let pointers = [[16u32, 128], [64, b0], [96, l0]];
    let links = [[b0, 2, a], [b0 + 96, 2, b], [l0, 2, a], [l0 + 8, 2, b]];
    let objects = [[0, 0, offset(1)], [a, 0, offset(2)], [b, 0, offset(2)]];
    let mut offs = [0u32; 6];
    offs[0] = data.len() as u32;
    pointers
        .iter()
        .flatten()
        .for_each(|w| data.extend(w.to_le_bytes()));
    pad16(&mut data);
    offs[1] = data.len() as u32;
    links
        .iter()
        .flatten()
        .for_each(|w| data.extend(w.to_le_bytes()));
    pad16(&mut data);
    offs[2] = data.len() as u32;
    objects
        .iter()
        .flatten()
        .for_each(|w| data.extend(w.to_le_bytes()));
    pad16(&mut data);
    offs[3] = data.len() as u32;
    offs[4] = offs[3];
    offs[5] = offs[3];
    let mut table = Vec::new();
    for (name, sig) in names {
        table.extend(sig.to_le_bytes());
        table.push(CLASS_NAME_MARKER);
        table.extend(name.as_bytes());
        table.push(0);
    }
    pad16(&mut table);
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(0u32.to_le_bytes());
    out.extend(11u32.to_le_bytes());
    out.extend(LAYOUT);
    out.extend(3u32.to_le_bytes());
    for w in [2u32, 0, 0, offset(1)] {
        out.extend(w.to_le_bytes());
    }
    out.extend(VERSION.as_bytes());
    out.push(0);
    out.push(0xff);
    out.extend(0u32.to_le_bytes());
    out.extend(21u32.to_le_bytes());
    assert_eq!(out.len(), HEADER_LEN);
    let names_start = 256u32;
    let types_start = names_start + table.len() as u32;
    let n = table.len() as u32;
    for (name, start, words) in [
        (SECTION_NAMES[0], names_start, [n; 6]),
        (SECTION_NAMES[1], types_start, [0; 6]),
        (SECTION_NAMES[2], types_start, offs),
    ] {
        let mut record = name.as_bytes().to_vec();
        record.resize(19, 0);
        record.push(0xff);
        record.extend(start.to_le_bytes());
        words.iter().for_each(|w| record.extend(w.to_le_bytes()));
        record.extend([0xff; 16]);
        out.extend(record);
    }
    out.extend(table);
    out.extend(data);
    out
}

/// NiNode: name, no extra data or controller, the transform, the collision
/// link and the children.
fn node(
    translation: [f32; 3],
    rows: [[f32; 3]; 3],
    scale: f32,
    collision: i32,
    children: &[i32],
) -> Vec<u8> {
    let mut v = Vec::new();
    for w in [u32::MAX, 0, u32::MAX, 0] {
        v.extend(w.to_le_bytes());
    }
    for f in translation
        .iter()
        .chain(rows.iter().flatten())
        .chain([&scale])
    {
        v.extend(f.to_bits().to_le_bytes());
    }
    v.extend(collision.to_le_bytes());
    v.extend((children.len() as u32).to_le_bytes());
    for c in children {
        v.extend(c.to_le_bytes());
    }
    v
}

const QUARTER_Z: [[f32; 3]; 3] = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
const IDENTITY_ROWS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Root node 0 with child node 1 (translation, quarter turn about z, scale
/// 2) that owns collision object 2 on system 3, Body ID 1.
fn model_nif(body_id: u32) -> Vec<u8> {
    let mut object = 1i32.to_le_bytes().to_vec();
    object.extend(0x80u16.to_le_bytes());
    object.extend(3i32.to_le_bytes());
    object.extend(body_id.to_le_bytes());
    let p = payload([[0.0; 3], [0.1, 0.2, 0.3]]);
    let mut system = (p.len() as u32).to_le_bytes().to_vec();
    system.extend(p);
    nif_blocks(
        &[
            ("NiNode", node([0.0; 3], IDENTITY_ROWS, 1.0, -1, &[1])),
            ("NiNode", node([10.0, 0.0, 5.0], QUARTER_Z, 2.0, 2, &[])),
            ("bhkNPCollisionObject", object),
            ("bhkPhysicsSystem", system),
        ],
        &[],
    )
}

fn decode_bytes(bytes: &[u8]) -> ModelCollision {
    let b = Budget::new(1 << 24);
    let n = fo4_nif::Nif::read(
        bytes,
        "synthetic.nif",
        fo4_core::Game::Fallout4,
        &Limits::default(),
        &b,
    )
    .unwrap();
    model::decode(&n, 0, "synthetic.nif", &Limits::default(), &b).unwrap()
}

#[test]
fn quaternion_rows_follow_the_transform_convention() {
    let id = quaternion_rows([0.0, 0.0, 0.0, 1.0]);
    assert_eq!(id, Transform::IDENTITY.rotation);
    // Half-angle 45 degrees about z: a quarter turn, x to y.
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let t = Transform {
        rotation: quaternion_rows([0.0, 0.0, h, h]),
        ..Transform::IDENTITY
    };
    assert!(close(t.apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
    assert!(close(t.apply([0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]));
    assert!(body_placement_is_identity(&fo4_havok::Body {
        index: 0,
        shape: 0,
        shape_slot: 0,
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, -1.0],
    }));
    let stored = |w: f32, t: f32| fo4_havok::Body {
        index: 0,
        shape: 0,
        shape_slot: 0,
        translation: [t, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, w],
    };
    assert!(body_placement_is_identity(&stored(0.99999994, 0.0)));
    assert!(!body_placement_is_identity(&stored(0.999, 0.0)));
    assert!(!body_placement_is_identity(&stored(1.0, 0.01)));
}

#[test]
fn body_placement_level_follows_the_registry() {
    use Status::*;
    let table = |ignored: Option<Status>, applied: Option<Status>| {
        move |name: &str| match name {
            hypotheses::BODY_PLACEMENT_IGNORED => ignored,
            hypotheses::BODY_PLACEMENT_APPLIED => applied,
            _ => None,
        }
    };
    let cases = [
        (
            Some(Pending),
            Some(Pending),
            [None, None, Some(BodyPlacement::Ignored)],
        ),
        (
            Some(Refuted),
            Some(Pending),
            [None, None, Some(BodyPlacement::Applied)],
        ),
        (
            Some(Refuted),
            Some(Supported),
            [
                None,
                Some(BodyPlacement::Applied),
                Some(BodyPlacement::Applied),
            ],
        ),
        (Some(Supported), Some(Supported), [None, None, None]),
        (Some(Refuted), Some(Refuted), [None, None, None]),
    ];
    for (i, a, expected) in cases {
        let got = [RuleLevel::Facts, RuleLevel::Measured, RuleLevel::Hypotheses]
            .map(|l| default_body_placement(l, table(i, a)));
        assert_eq!(got, expected, "{i:?} {a:?}");
    }
    // The embedded registry: both pending, the live run's best-ranked first.
    assert_eq!(
        default_body_placement(RuleLevel::Hypotheses, hypotheses::status),
        Some(BodyPlacement::Ignored)
    );
    assert_eq!(
        default_body_placement(RuleLevel::Measured, hypotheses::status),
        None
    );
}

#[test]
fn rule_state_needs_every_rule_registered() {
    let status = |name: &str| match name {
        "a" => Some(Status::Supported),
        "b" => Some(Status::Pending),
        "c" => Some(Status::Refuted),
        _ => None,
    };
    assert!(matches!(rules_state(&[], status), State::Ready));
    assert!(rules_state(&["a"], status).ready_at(RuleLevel::Measured));
    let pending = rules_state(&["a", "b"], status);
    assert!(!pending.ready_at(RuleLevel::Measured) && pending.ready_at(RuleLevel::Hypotheses));
    assert!(matches!(rules_state(&["a", "c"], status), State::Blocked));
    assert!(matches!(
        rules_state(&["a", "unknown"], status),
        State::Blocked
    ));
}

fn mesh_leaf(steps: Vec<Step>) -> Leaf {
    Leaf {
        object: 1,
        path: vec![ShapeClass::CompressedMesh],
        steps,
        shape: Shape::Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            records: vec![Record {
                kind: 0x50,
                radius: 0.5,
                vertices: 0..1,
            }],
            record_vertices: vec![[0.0, 0.0, 2.0]],
            placeholders: 0,
        },
    }
}

fn synthetic_cell(leaf: Leaf, frame: Transform, target_local: Transform) -> CellCollision {
    let body = BodyCollision {
        collision_object: 2,
        target: 1,
        target_name: None,
        flags: 0x80,
        system: 3,
        body_id: 0,
        body: fo4_havok::Body {
            index: 0,
            shape: 1,
            shape_slot: 0,
            translation: [1.0, 0.0, 0.0],
            rotation: [
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
        },
        target_local,
        chain: vec![0, 1],
        hidden_chain: false,
        leaves: vec![leaf],
        render: vec![],
    };
    let model = ModelCollision {
        asset: 0,
        path: "synthetic.nif".into(),
        collision_objects: 1,
        systems: 1,
        bodies: vec![body],
        refusals: vec![],
        error: None,
        render_points: vec![],
        render: vec![],
        render_error: None,
    };
    let key = FormKey {
        plugin: "synthetic.esm".into(),
        object_id: 1,
    };
    CellCollision {
        level: RuleLevel::Hypotheses,
        body_placement: Some(BodyPlacement::Ignored),
        models: vec![model],
        placements: vec![ModelPlacement {
            reference: key.clone(),
            base: key,
            model: 0,
            frame,
            scene_rules: vec![],
        }],
        instances: vec![CollisionInstance {
            placement: 0,
            body: 0,
            leaf: 0,
            kind: ShapeClass::CompressedMesh,
            world: Transform::IDENTITY,
            rules: vec![],
        }],
        unplaced: vec![],
        without_collision: vec![],
    }
}

#[test]
fn world_triangles_apply_units_frame_and_each_body_level() {
    let frame = Transform {
        translation: [100.0, 200.0, 300.0],
        scale: 2.0,
        ..Transform::IDENTITY
    };
    let c = synthetic_cell(mesh_leaf(vec![]), frame, Transform::IDENTITY);
    let i = &c.instances[0];
    let tri: Vec<_> = c.world_triangles(i, BodyPlacement::Ignored).collect();
    assert_eq!(tri.len(), 2);
    let u = 2.0 * UNITS;
    assert!(close(tri[0][1], [100.0 + u, 200.0, 300.0]));
    assert!(close(tri[1][2], [100.0 + u, 200.0 + u, 300.0]));
    // Applied: quarter turn about z, then translation (1, 0, 0), in collision units.
    let tri: Vec<_> = c.world_triangles(i, BodyPlacement::Applied).collect();
    assert!(close(tri[0][0], [100.0 + u, 200.0, 300.0]));
    assert!(close(tri[0][1], [100.0 + u, 200.0 + u, 300.0]));
    let b = c.world_bounds(i, BodyPlacement::Ignored).unwrap();
    // The record point (0, 0, 2) widened by its radius 0.5.
    assert!(close(b.max, [100.0 + u, 200.0 + u, 300.0 + 2.5 * u]));
    assert!(close(b.min, [100.0 - 0.5 * u, 200.0 - 0.5 * u, 300.0]));
    let shape = c
        .world_shape(i, BodyPlacement::Ignored, &Budget::new(1 << 20))
        .unwrap();
    let WorldShape::Mesh {
        vertices,
        triangles,
        normals,
        records,
    } = shape
    else {
        panic!("mesh");
    };
    assert_eq!((vertices.len(), triangles.len(), records.len()), (4, 2, 1));
    // Stored winding [0, 1, 2] in the xy plane: normal +z; the frame's
    // positive scale keeps it.
    assert!(close(normals[0], [0.0, 0.0, 1.0]));
    assert_eq!(triangle_normal([[0.0; 3]; 3]), [0.0; 3]);
    assert!((records[0].radius - 0.5 * u).abs() < 1e-9);
    assert!(c
        .world_shape(i, BodyPlacement::Ignored, &Budget::new(16))
        .is_err());
}

#[test]
fn compound_and_scaled_steps_apply_innermost_first() {
    let steps = vec![
        Step::Instance {
            rotation: QUARTER_Z,
            translation: [0.0, 0.0, 1.0],
            scale: [2.0; 3],
        },
        Step::Scaled {
            scale: [1.0, 3.0, 1.0],
            translation: [1.0, 0.0, 0.0],
        },
    ];
    let leaf = mesh_leaf(steps);
    // (1, 0, 0) -> scaled (2, 0, 0) -> instance: 2 * quarter turn = (0, 4, 0) + (0, 0, 1).
    assert!(close(leaf.to_body([1.0, 0.0, 0.0]), [0.0, 4.0, 1.0]));
    assert_eq!(leaf.uniform_scale(), 2.0);
    assert!(leaf.per_axis_scaled());
    let rules: Vec<_> = leaf.steps.iter().map(Step::rule).collect();
    assert_eq!(
        rules,
        [hypotheses::COMPOUND_INSTANCE, hypotheses::SCALED_CONVEX]
    );
    // Target node chain composes after the units: a node translation in world units.
    let target = Transform {
        translation: [5.0, 0.0, 0.0],
        ..Transform::IDENTITY
    };
    let c = synthetic_cell(mesh_leaf(vec![]), Transform::IDENTITY, target);
    let p = c.world_point(&c.instances[0], BodyPlacement::Ignored, [1.0, 0.0, 0.0]);
    assert!(close(p, [5.0 + UNITS, 0.0, 0.0]));
}

#[test]
fn model_decode_follows_body_id_chain_and_reports_unnamed_bodies() {
    let m = decode_bytes(&model_nif(1));
    assert!(m.error.is_none(), "{:?}", m.error);
    assert_eq!((m.collision_objects, m.systems, m.bodies.len()), (1, 1, 1));
    let body = &m.bodies[0];
    assert_eq!((body.target, body.body_id, body.body.index), (1, 1, 1));
    assert_eq!(body.chain, [0, 1]);
    assert!(!body.hidden_chain);
    assert_eq!(body.target_local.translation, [10.0, 0.0, 5.0]);
    assert_eq!(body.target_local.scale, 2.0);
    assert_eq!(body.leaves.len(), 1);
    assert_eq!(body.leaves[0].path, [ShapeClass::Sphere]);
    assert_eq!(
        body.leaves[0].shape,
        Shape::Sphere {
            center: [0.1, 0.2, 0.3],
            radius: 0.5
        }
    );
    assert_eq!(m.refusals.len(), 1);
    assert_eq!(m.refusals[0].code, "BodyWithoutCollisionObject");
    assert_eq!(m.refusals[0].body, Some(0));
    // A Body ID past the bodies is refused by name; body 0 and 1 stay unnamed.
    let m = decode_bytes(&model_nif(7));
    assert!(m.bodies.is_empty());
    let codes: Vec<_> = m.refusals.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "BodyIdOutOfRange",
            "BodyWithoutCollisionObject",
            "BodyWithoutCollisionObject"
        ]
    );
}

#[test]
fn model_decode_never_panics_on_changed_payload_bytes() {
    let good = model_nif(1);
    let start = good.len() - 8 - payload([[0.0; 3]; 2]).len();
    for at in (start..good.len() - 8).step_by(7) {
        let mut bytes = good.clone();
        bytes[at] ^= 0x5a;
        let b = Budget::new(1 << 24);
        if let Ok(n) = fo4_nif::Nif::read(
            &bytes,
            "synthetic.nif",
            fo4_core::Game::Fallout4,
            &Limits::default(),
            &b,
        ) {
            let _ = model::decode(&n, 0, "synthetic.nif", &Limits::default(), &b);
        }
    }
    let b = Budget::new(1 << 24);
    let n = fo4_nif::Nif::read(
        &good,
        "synthetic.nif",
        fo4_core::Game::Fallout4,
        &Limits::default(),
        &b,
    )
    .unwrap();
    assert!(model::decode(&n, 0, "synthetic.nif", &Limits::default(), &Budget::new(64)).is_err());
}

#[test]
fn cell_places_bodies_only_at_a_level_that_accepts_their_rules() {
    let f = fixture(false, false);
    std::fs::write(f.manifest.root.join("meshes/a.nif"), model_nif(1)).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    let limits = Limits::default();
    let options = Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let c = cell(&scene, &loader, &limits, options, &b).unwrap();
    assert_eq!(c.body_placement, Some(BodyPlacement::Ignored));
    assert_eq!(c.instances.len(), 1, "{:?}", c.unplaced);
    let i = &c.instances[0];
    assert_eq!(i.kind, ShapeClass::Sphere);
    for r in [
        hypotheses::COLLISION_UNITS,
        hypotheses::TARGET_FRAME,
        hypotheses::BODY_ID,
        hypotheses::BODY_PLACEMENT_IGNORED,
    ] {
        assert!(i.rules.contains(&r), "{r}");
    }
    assert!(!i.rules.contains(&hypotheses::MESH_TRIANGLES));
    let reference = Transform::euler([12.5, -2., 3., 0.1, 0.2, 0.3], 0.7).unwrap();
    let target = Transform {
        rotation: QUARTER_Z.map(|r| r.map(f64::from)),
        translation: [10.0, 0.0, 5.0],
        scale: 2.0,
    };
    let center = [0.1f32, 0.2, 0.3].map(|x| f64::from(x) * UNITS);
    let expected = reference.apply(target.apply(center));
    let WorldShape::Sphere { center, radius } =
        c.world_shape(i, BodyPlacement::Ignored, &b).unwrap()
    else {
        panic!("sphere");
    };
    assert!(close(center, expected), "{center:?} {expected:?}");
    assert!((radius - 0.5 * UNITS * 2.0 * f64::from(0.7f32)).abs() < 1e-9);
    // Applied moves the centre by the stored rotation and translation.
    let WorldShape::Sphere { center: moved, .. } =
        c.world_shape(i, BodyPlacement::Applied, &b).unwrap()
    else {
        panic!("sphere");
    };
    assert!(!close(moved, expected));
    assert_eq!(c.unplaced.len(), 1);
    assert_eq!(c.unplaced[0].code, "BodyWithoutCollisionObject");
    // Measured accepts none of the pending placement rules.
    let options = Options {
        level: RuleLevel::Measured,
        body_placement: None,
    };
    let c = cell(&scene, &loader, &limits, options, &b).unwrap();
    assert!(c.instances.is_empty());
    assert!(c
        .unplaced
        .iter()
        .any(|u| u.code == "BodyPlacementRuleNotAccepted"));
    // An explicit level at Measured still needs its rules accepted.
    let options = Options {
        level: RuleLevel::Measured,
        body_placement: Some(BodyPlacement::Applied),
    };
    let c = cell(&scene, &loader, &limits, options, &b).unwrap();
    assert!(c.instances.is_empty());
    assert!(c
        .unplaced
        .iter()
        .any(|u| u.code == "CollisionRuleNotAccepted"));
    // Every rule supported: placed at Measured.
    let all = |_: &str| Some(Status::Supported);
    let c = cell_with(&scene, &loader, &limits, options, &all, &b).unwrap();
    assert_eq!(c.instances.len(), 1);
    // A refuted rule blocks the leaf at every level.
    let refuted = |name: &str| {
        if name == hypotheses::COLLISION_UNITS {
            Some(Status::Refuted)
        } else {
            hypotheses::status(name)
        }
    };
    let options = Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let c = cell_with(&scene, &loader, &limits, options, &refuted, &b).unwrap();
    assert!(c.instances.is_empty());
    assert!(c.unplaced.iter().any(|u| u.code == "CollisionRuleRefuted"));
}

#[test]
fn a_material_swap_filter_blocks_the_render_and_not_the_collision() {
    let f = fixture(false, false);
    let root = f.manifest.root.clone();
    std::fs::write(root.join("meshes/a.nif"), model_nif(1)).unwrap();
    // The synthetic STAT, last in the file, now names a material swap whose
    // filter (FNAM) the scene does not evaluate.
    let path = root.join("base.esm");
    let bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(4).rposition(|s| s == b"STAT").unwrap();
    let mut bytes = bytes[..at].to_vec();
    let mut stat = sub(b"MODL", b"a.nif\0");
    stat.extend(sub(b"MODS", &0x820u32.to_le_bytes()));
    bytes.extend(record(b"STAT", 0x801, 0, &stat));
    bytes.extend(record(b"MSWP", 0x820, 0, &sub(b"FNAM", b"Filter\0")));
    std::fs::write(&path, bytes).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    let i = &scene.instances[0];
    assert!(matches!(i.state, State::Blocked));
    let k = i
        .blockers
        .iter()
        .position(|e| e.code == "RuntimeMaterialSwapFilter")
        .unwrap();
    assert_eq!(i.blocker_origins[k], crate::readiness::Origin::MaterialSwap);
    assert_eq!(i.blocker_origins.len(), i.blockers.len());
    assert!(i.collision.ready_at(RuleLevel::Hypotheses));
    assert!(!i.collision.ready_at(RuleLevel::Measured));
    assert!(i
        .collision
        .hypotheses()
        .contains(&hypotheses::SWAP_COLLISION));
    let options = Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let limits = Limits::default();
    let c = cell(&scene, &loader, &limits, options, &b).unwrap();
    assert_eq!(c.instances.len(), 1, "{:?}", c.unplaced);
    assert!(c.instances[0].rules.contains(&hypotheses::SWAP_COLLISION));
    assert!(c.placements[0]
        .scene_rules
        .contains(&hypotheses::SWAP_COLLISION));
    // Not at Measured while the swap rule is pending.
    let options = Options {
        level: RuleLevel::Measured,
        body_placement: Some(BodyPlacement::Ignored),
    };
    let c = cell(&scene, &loader, &limits, options, &b).unwrap();
    assert!(c.instances.is_empty());
    assert_eq!(c.unplaced[0].code, "CollisionNotReady");
}

#[test]
fn model_without_collision_is_listed_not_refused() {
    let f = fixture(false, false);
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    let options = Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let c = cell(&scene, &loader, &Limits::default(), options, &b).unwrap();
    assert!(c.instances.is_empty());
    assert_eq!(c.without_collision.len(), 1);
    assert!(c.unplaced.is_empty(), "{:?}", c.unplaced);
}

#[test]
fn polytope_radius_widens_bounds_and_scales_with_the_frame() {
    let leaf = Leaf {
        object: 1,
        path: vec![ShapeClass::ConvexPolytope],
        steps: vec![],
        shape: Shape::Polytope {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
            radius: 0.25,
        },
    };
    let frame = Transform {
        scale: 3.0,
        ..Transform::IDENTITY
    };
    let mut c = synthetic_cell(leaf, frame, Transform::IDENTITY);
    c.instances[0].kind = ShapeClass::ConvexPolytope;
    let i = &c.instances[0];
    let r = 0.25 * 3.0 * UNITS;
    let b = c.world_bounds(i, BodyPlacement::Ignored).unwrap();
    assert!(close(b.min, [-r; 3]));
    assert!(close(b.max, [3.0 * UNITS + r; 3]));
    let WorldShape::Points { vertices, radius } = c
        .world_shape(i, BodyPlacement::Ignored, &Budget::new(1 << 20))
        .unwrap()
    else {
        panic!("points");
    };
    assert_eq!(vertices.len(), 2);
    assert!((radius.unwrap() - r).abs() < 1e-9);
}
