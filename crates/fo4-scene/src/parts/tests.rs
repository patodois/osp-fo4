use super::*;
use fo4_nif::{Block, Field};

fn field(name: &'static str, value: Value) -> Field {
    Field {
        name,
        offset: 0,
        value,
    }
}

fn reference(name: &'static str, r: i32) -> Field {
    field(name, Value::Ref(r))
}

/// Block `k` of type `ty` spans bytes `100 k .. 100 k + 100`.
fn block(k: usize, ty: u16, decoded: Option<Data>) -> Block {
    Block {
        type_index: ty,
        data: 100 * k..100 * k + 100,
        decoded,
        raw_tail: 0..0,
        diagnostics: Vec::new(),
    }
}

const TYPES: [&str; 9] = [
    "NiNode",
    "BSTriShape",
    "BSLightingShaderProperty",
    "BSShaderTextureSet",
    "BSPositionData",
    "NiParticleSystem",
    "NiTransformController",
    "NiControllerSequence",
    "NiControllerManager",
];

/// Root (0) over: a plain shape (1) with its shader (2) and texture set
/// (3); a shape (4) whose extra data is an undecoded block (5); a node
/// named "moved" (6) over a shape (7); a particle system (8); a node named
/// "seq" (11) over a shape (12); a transform controller (9) on node 6 and
/// a controller sequence (10) naming "seq"; with `undecoded_controller`
/// block 9 is not decoded.
fn model(undecoded_controller: bool) -> Nif<'static> {
    let node = |name: u32, children: Vec<i32>| {
        Some(Data::Node {
            fields: vec![field("Name", Value::StringIndex(name))],
            children,
        })
    };
    let shape = |extra: i32| {
        Some(Data::Fields(vec![
            field("Name", Value::StringIndex(3)),
            reference("Extra Data", extra),
            reference("Controller", -1),
            reference("Collision", -1),
            reference("Shader Property", 2),
            reference("Alpha Property", -1),
        ]))
    };
    let fields = |f: Vec<Field>| Some(Data::Fields(f));
    Nif {
        version: 0x1402_0007,
        user_version: 12,
        bs_version: 130,
        exports: Vec::new(),
        block_types: TYPES.iter().map(|t| (*t).into()).collect(),
        strings: vec![
            b"root".to_vec(),
            b"moved".to_vec(),
            b"seq".to_vec(),
            b"shape".to_vec(),
        ],
        max_string_length: 5,
        groups: Vec::new(),
        roots: vec![0],
        blocks: vec![
            block(0, 0, node(0, vec![1, 4, 6, 8, 11])),
            block(1, 1, shape(-1)),
            block(2, 2, fields(vec![reference("Texture Set", 3)])),
            block(3, 3, fields(Vec::new())),
            block(4, 1, shape(5)),
            block(5, 4, None),
            block(6, 0, node(1, vec![7])),
            block(7, 1, shape(-1)),
            block(8, 5, fields(vec![reference("Shader Property", 2)])),
            block(
                9,
                6,
                (!undecoded_controller).then(|| Data::Fields(vec![reference("Target", 6)])),
            ),
            block(
                10,
                7,
                fields(vec![field("Node Name", Value::StringIndex(2))]),
            ),
            block(11, 0, node(2, vec![12])),
            block(12, 1, shape(-1)),
        ],
        bytes: &[],
    }
}

fn blocker(code: &str, offset: u64, found: &str) -> Error {
    Error::new(code, "model.nif", offset, "decoded reached block", found)
}

#[test]
fn a_shape_reading_an_open_blocker_or_driven_by_a_controller_is_left_out_with_particles() {
    let b = Budget::new(1 << 20);
    let n = model(false);
    // An undecoded extra data block, and a manager blocker its rest rule
    // covers (left to that rule, on the plain shape's shader).
    let blockers = [
        blocker("UnsupportedNifBlock", 510, "BSPositionData"),
        blocker(
            "UnsupportedAnimationRuntime",
            220,
            "NiControllerManager;target_in_visual=Some(true);rest=manager-idle",
        ),
    ];
    assert!(open(&blockers[0]) && !open(&blockers[1]));
    let p = read(&n, &blockers, &b).unwrap();
    assert_eq!(p.kept, 1, "{p:?}");
    assert_eq!(p.left_out(1), None);
    assert_eq!(p.left_out(4), Some("UnsupportedNifBlock"));
    assert_eq!(p.left_out(7), Some(MOVED));
    assert_eq!(p.left_out(8), Some(PARTICLE));
    assert_eq!(p.left_out(12), Some(MOVED));
    assert_eq!(p.left.len(), 4);
    // Without the blockers the shape with the undecoded extra data stays.
    let p = read(&n, &[], &b).unwrap();
    assert_eq!((p.kept, p.left_out(4)), (2, None));
}

#[test]
fn an_undecoded_controller_leaves_every_shape_out() {
    let b = Budget::new(1 << 20);
    let p = read(&model(true), &[], &b).unwrap();
    assert_eq!(p.kept, 0);
    assert_eq!(
        p.left.values().filter(|c| c.as_str() == MOVED).count(),
        4,
        "{p:?}"
    );
}

#[test]
fn only_open_model_blockers_are_set_aside() {
    use readiness::Origin;
    let unread = blocker("UnsupportedNifBlock", 0, "BSPositionData");
    let graph = blocker(
        "UnsupportedAnimationRuntime",
        0,
        "BSBehaviorGraphExtraData;target_in_visual=None;rest=behavior-graph",
    );
    let idle = blocker(
        "UnsupportedAnimationRuntime",
        0,
        "NiControllerManager;target_in_visual=Some(true);rest=manager-idle",
    );
    let record = blocker("UninterpretedVisualSubrecord", 0, "REFR.XALP.");
    assert!(aside(&unread, Origin::Model) && aside(&graph, Origin::Model));
    assert!(!aside(&unread, Origin::Record) && !aside(&unread, Origin::RenderAsset));
    assert!(!aside(&idle, Origin::Model) && !aside(&record, Origin::Model));
}
