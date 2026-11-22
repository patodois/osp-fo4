//! Synthetic actor chain: a cell with placed actors, their records, a
//! skeleton and a skinned model, all built here.
use super::*;
use crate::tests::{nif_blocks, record, sub};
use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use fo4_plugin::typed::Store;
use fo4_vfs::Vfs;
use std::fs;

fn group(kind: u32, id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = b"GRUP".to_vec();
    v.extend((data.len() as u32 + 24).to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend([0; 8]);
    v.extend(data);
    v
}
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
/// A node: name index, rows of its rotation, translation, children.
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
/// A quarter turn about z, as stored rows acting on column vectors.
const RZ: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
fn half(v: f32) -> u16 {
    if v == 0. {
        return 0;
    }
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    sign | ((exp as u16) << 10) | ((bits >> 13) & 0x3ff) as u16
}
/// Skeleton: Root, BoneA at z 10 turned a quarter about z, BoneB 5 along
/// BoneA's local x (world (0, 5, 10) when rows act on column vectors).
fn skeleton(b_local: [f32; 3]) -> Vec<u8> {
    nif_blocks(
        &[
            ("NiNode", node(0, I, [0.; 3], &[1])),
            ("NiNode", node(1, RZ, [0., 0., 10.], &[2])),
            ("NiNode", node(2, I, b_local, &[])),
        ],
        &[b"Root", b"BoneA", b"BoneB"],
    )
}
/// A model bound in the skeleton's pose: flat bone nodes at their world
/// transforms, one shape of three vertices, each bone's stored transform
/// the inverse of its node's world transform.
fn model() -> Vec<u8> {
    let mut shape = Vec::new();
    words(&mut shape, &[3, 0, u32::MAX, 0]);
    floats(
        &mut shape,
        &[0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.],
    );
    words(&mut shape, &[u32::MAX]);
    floats(&mut shape, &[0., 0., 0., 1.]);
    words(&mut shape, &[4, u32::MAX, u32::MAX]);
    shape.extend(((0x41u64 << 44) | (2 << 28) | 5).to_le_bytes());
    words(&mut shape, &[1]);
    shape.extend(3u16.to_le_bytes());
    words(&mut shape, &[3 * 20 + 6]);
    for (p, w, i) in [
        ([0f32, 0., 12.], [1f32, 0.], [0u8, 0]),
        ([0., 5., 12.], [1., 0.], [1, 0]),
        ([0., 2., 11.], [0.5, 0.5], [0, 1]),
    ] {
        for c in p {
            shape.extend(half(c).to_le_bytes());
        }
        shape.extend(0u16.to_le_bytes());
        for c in [w[0], w[1], 0., 0.] {
            shape.extend(half(c).to_le_bytes());
        }
        shape.extend([i[0], i[1], 0, 0]);
    }
    for t in [0u16, 1, 2] {
        shape.extend(t.to_le_bytes());
    }
    let mut instance = Vec::new();
    words(&mut instance, &[0, 5, 2, 1, 2, 0]);
    let mut data = Vec::new();
    words(&mut data, &[2]);
    // Inverse of a node at translation t with rows R: rows R^T, -R^T t.
    // BoneA: R = RZ, t = (0, 0, 10); BoneB: R = RZ, t = (0, 5, 10).
    let rt = [[0f32, 1., 0.], [-1., 0., 0.], [0., 0., 1.]];
    for t in [[0f32, 0., -10.], [-5., 0., -10.]] {
        floats(&mut data, &[0., 0., 0., 50.]);
        for r in rt {
            floats(&mut data, &r);
        }
        floats(&mut data, &t);
        floats(&mut data, &[1.]);
    }
    nif_blocks(
        &[
            ("NiNode", node(0, I, [0.; 3], &[1, 2, 3])),
            ("NiNode", node(1, RZ, [0., 0., 10.], &[])),
            ("NiNode", node(2, RZ, [0., 5., 10.], &[])),
            ("BSTriShape", shape),
            ("BSSkin::Instance", instance),
            ("BSSkin::BoneData", data),
        ],
        &[b"Root", b"BoneA", b"BoneB", b"Body"],
    )
}

struct Fixture {
    _temp: TestDir,
    manifest: Manifest,
}
fn id(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn fixture() -> Fixture {
    let temp = TestDir::new("actor-chain");
    let root = temp.join("Data");
    fs::create_dir_all(root.join("meshes/actors")).unwrap();
    fs::write(root.join("meshes/actors/skel.nif"), skeleton([5., 0., 0.])).unwrap();
    fs::write(root.join("meshes/actors/body.nif"), model()).unwrap();
    let acbs = |flags: u32, template: u16| {
        let mut v = vec![0u8; 20];
        v[0..4].copy_from_slice(&flags.to_le_bytes());
        v[14..16].copy_from_slice(&template.to_le_bytes());
        v
    };
    let mut placement = Vec::new();
    floats(&mut placement, &[100., 200., 300., 0., 0., 0.]);
    let achr = |rid: u32, base: u32| {
        let mut d = sub(b"NAME", &id(base));
        d.extend(sub(b"DATA", &placement));
        record(b"ACHR", rid, 0, &d)
    };
    let npc = |rid: u32, skin: u32, extra: &[u8], template: u16| {
        let mut d = sub(b"ACBS", &acbs(0, template));
        d.extend(sub(b"RNAM", &id(0x810)));
        d.extend(sub(b"WNAM", &id(skin)));
        d.extend(extra);
        record(b"NPC_", rid, 0, &d)
    };
    let mut race = sub(b"WNAM", &id(0x820));
    race.extend(sub(b"MNAM", &[]));
    race.extend(sub(b"ANAM", b"actors\\skel.nif\0"));
    race.extend(sub(b"FNAM", &[]));
    race.extend(sub(b"ANAM", b"actors\\skel.nif\0"));
    let armo = |rid: u32, addon: u32| {
        let mut d = sub(b"BOD2", &id(4));
        d.extend(sub(b"RNAM", &id(0x810)));
        d.extend(sub(b"INDX", &[0, 0]));
        d.extend(sub(b"MODL", &id(addon)));
        record(b"ARMO", rid, 0, &d)
    };
    let mut arma = sub(b"BOD2", &id(4));
    arma.extend(sub(b"RNAM", &id(0x810)));
    arma.extend(sub(b"MOD2", b"actors\\body.nif\0"));
    let lvlo = |level: u16, target: u32| {
        let mut v = level.to_le_bytes().to_vec();
        v.extend([0, 0]);
        v.extend(id(target));
        v.extend([1, 0, 0, 0]);
        sub(b"LVLO", &v)
    };
    let mut lvln = sub(b"LLCT", &[2]);
    lvln.extend(lvlo(1, 0x803));
    lvln.extend(lvlo(5, 0x806));
    let mut lvli = sub(b"LLCT", &[2]);
    lvli.extend(lvlo(1, 0x822));
    lvli.extend(lvlo(1, 0x823));
    let mut templated = sub(b"TPLT", &id(0x830));
    templated.extend(sub(b"TPTA", &[id(0x830), vec![0; 48]].concat()));
    let references = [achr(0x900, 0x803), achr(0x901, 0x804), achr(0x902, 0x805)].concat();
    let mut cell = sub(b"EDID", b"ActorRoom\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut body = group(
        2,
        0,
        &[
            record(b"CELL", 0x800, 0, &cell),
            group(6, 0x800, &group(9, 0x800, &references)),
        ]
        .concat(),
    );
    body.extend(npc(0x803, 0x820, &[], 0));
    body.extend(npc(0x806, 0x822, &[], 0));
    body.extend(npc(0x804, 0x820, &templated, 1));
    body.extend(npc(0x805, 0x820, &sub(b"DOFT", &id(0x840)), 0));
    body.extend(record(b"RACE", 0x810, 0, &race));
    body.extend(armo(0x820, 0x821));
    body.extend(armo(0x822, 0x821));
    body.extend(armo(0x823, 0x821));
    body.extend(record(b"ARMA", 0x821, 0, &arma));
    body.extend(record(b"LVLN", 0x830, 0, &lvln));
    body.extend(record(b"LVLI", 0x841, 0, &lvli));
    body.extend(record(b"OTFT", 0x840, 0, &sub(b"INAM", &id(0x841))));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
    plugin.extend(body);
    fs::write(root.join("base.esm"), plugin).unwrap();
    let manifest = Manifest {
        game: Game::Fallout4,
        archive_invalidation: None,
        root,
        language: "english".into(),
        plugins: vec![PluginSpec {
            name: "base.esm".into(),
            path: "base.esm".into(),
        }],
        mounts: vec![Mount::Loose {
            plugin: None,
            base: None,
            files: vec![
                "meshes/actors/skel.nif".into(),
                "meshes/actors/body.nif".into(),
            ],
        }],
        required_masters: vec![],
        excluded: vec![],
        build: Some("synthetic".into()),
        limits: Limits::default(),
    };
    Fixture {
        _temp: temp,
        manifest,
    }
}

#[test]
fn node_rotations_act_on_column_vectors_and_compose_down_the_tree() {
    let bytes = skeleton([5., 0., 0.]);
    let b = Budget::new(1 << 20);
    let n = fo4_nif::Nif::read(&bytes, "skel", Game::Fallout4, &Limits::default(), &b).unwrap();
    let s = pose::Skeleton::read(&n, "skel", &Limits::default(), &b).unwrap();
    assert_eq!(s.bones.len(), 3);
    let a = s.bones[&b"BoneB"[..]];
    assert!(
        (0..3).all(|k| (a.translation[k] - [0., 5., 10.][k]).abs() < 1e-12),
        "{a:?}"
    );
    // The static renderer's reading of the same words puts it at (0, -5, 10).
    let p = pose::Affine {
        rows: RZ.map(|r| r.map(f64::from)),
        translation: [0., 0., 10.],
        scale: 1.,
    };
    assert_eq!(p.apply([5., 0., 0.]), [0., 5., 10.]);
    assert_eq!(p.compose(&pose::Affine::IDENTITY), p);
}

#[test]
fn bind_pose_skinning_returns_the_stored_vertices_and_the_rest_pose_moves_them() {
    let b = Budget::new(1 << 20);
    let l = Limits::default();
    let bytes = model();
    let n = fo4_nif::Nif::read(&bytes, "body", Game::Fallout4, &l, &b).unwrap();
    let skin = fo4_nif::skin::shape_skin(&n, 3, "body", &l, &b)
        .unwrap()
        .unwrap();
    let fit = pose::bind_fit(&n, &skin, "body", &l, &b).unwrap();
    assert!(
        fit.rotation_error < 1e-12 && fit.translation_spread < 1e-12,
        "{fit:?}"
    );
    let same_bytes = skeleton([5., 0., 0.]);
    let same = fo4_nif::Nif::read(&same_bytes, "skel", Game::Fallout4, &l, &b).unwrap();
    let rest = pose::Skeleton::read(&same, "skel", &l, &b).unwrap();
    let m = pose::bone_matrices(&n, &skin, &rest, "body", &b).unwrap();
    let p = pose::skin_positions(&n, 3, &m, "body", &b).unwrap();
    assert_eq!(p, [[0., 0., 12.], [0., 5., 12.], [0., 2., 11.]]);
    // BoneB 2 units further along BoneA's x: in world, 2 along y.
    let moved_bytes = skeleton([7., 0., 0.]);
    let moved = fo4_nif::Nif::read(&moved_bytes, "skel", Game::Fallout4, &l, &b).unwrap();
    let rest = pose::Skeleton::read(&moved, "skel", &l, &b).unwrap();
    let m = pose::bone_matrices(&n, &skin, &rest, "body", &b).unwrap();
    let p = pose::skin_positions(&n, 3, &m, "body", &b).unwrap();
    assert_eq!(p, [[0., 0., 12.], [0., 7., 12.], [0., 3., 11.]]);
    // A skeleton without the bone refuses the shape.
    let mut partial = pose::Skeleton::default();
    partial
        .bones
        .insert(b"BoneA".to_vec(), pose::Affine::IDENTITY);
    assert_eq!(
        pose::bone_matrices(&n, &skin, &partial, "body", &b)
            .unwrap_err()
            .code,
        "ActorSkinBoneMissing"
    );
}

#[test]
fn placed_actors_resolve_to_a_skeleton_and_addon_models_or_say_why_not() {
    let f = fixture();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", crate::RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id("ActorRoom", &b).unwrap();
    let actors = resolve(&scene, &mut loader, &b).unwrap();
    assert_eq!(actors.actors.len(), 3);
    let plain = &actors.actors[0];
    let plan = plain.plan.as_ref().expect("resolved");
    assert_eq!(plan.skeleton, b"meshes/actors/skel.nif");
    assert_eq!(plan.section, 0);
    assert_eq!(plan.parts.len(), 1);
    assert_eq!(plan.parts[0].path, b"meshes/actors/body.nif");
    assert!(plan.parts[0].skin);
    assert!(
        plain.notes.iter().any(|n| n == "outfit: none declared"),
        "{:?}",
        plain.notes
    );
    let crate::State::ReadyUnderHypotheses { hypotheses } = &plain.state else {
        panic!("{:?} {:?}", plain.state, plain.blockers)
    };
    for rule in [
        hypotheses::ACTOR_PLACEMENT,
        hypotheses::ACTOR_SECTION,
        hypotheses::ACTOR_ADDONS,
        hypotheses::ACTOR_SKINNING,
    ] {
        assert!(hypotheses.contains(&rule), "{rule}");
    }
    assert!(!hypotheses.contains(&hypotheses::ACTOR_CANDIDATES));
    assert!(plain.state.ready_at(crate::RuleLevel::Hypotheses));
    assert!(!plain.state.ready_at(crate::RuleLevel::Measured));
    // A leveled template whose two entries wear different skins.
    let leveled = &actors.actors[1];
    assert!(leveled.plan.is_none());
    assert_eq!(leveled.notes, ["leveled list: skin: 2 candidates"]);
    // An outfit of a leveled item list giving two armours.
    let outfit = &actors.actors[2];
    assert!(outfit.plan.is_none());
    assert!(
        outfit
            .notes
            .iter()
            .any(|n| n == "outfit: unresolved; actor not drawn"),
        "{:?}",
        outfit.notes
    );
}
