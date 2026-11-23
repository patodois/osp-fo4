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
    fs::write(root.join("meshes/actors/head.nif"), model()).unwrap();
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
    let on_list = |list: u32| {
        let mut d = sub(b"TPLT", &id(list));
        d.extend(sub(b"TPTA", &[id(list), vec![0; 48]].concat()));
        d
    };
    // A list with LVLF 3 whose two entries are one NPC_, and one with LVLD 100.
    let mut agreeing = sub(b"LVLD", &[0]);
    agreeing.extend(sub(b"LVLM", &[0]));
    agreeing.extend(sub(b"LVLF", &[3]));
    agreeing.extend(sub(b"LLCT", &[2]));
    agreeing.extend(lvlo(1, 0x803));
    agreeing.extend(lvlo(7, 0x803));
    let mut chance = sub(b"LVLD", &[100]);
    chance.extend(sub(b"LVLM", &[0]));
    chance.extend(sub(b"LVLF", &[0]));
    chance.extend(sub(b"LLCT", &[1]));
    chance.extend(lvlo(1, 0x803));
    // Every template bit set, every slot the plain NPC_; its own skin differs.
    let mut all_bits = sub(b"TPLT", &id(0x803));
    let slots: Vec<u8> = (0..13)
        .flat_map(|k| {
            if super::TEMPLATE_BITS & (1 << k) != 0 {
                id(0x803)
            } else {
                vec![0; 4]
            }
        })
        .collect();
    all_bits.extend(sub(b"TPTA", &slots));
    let references = [
        achr(0x900, 0x803),
        achr(0x901, 0x804),
        achr(0x902, 0x805),
        achr(0x903, 0x807),
        achr(0x904, 0x808),
        achr(0x905, 0x809),
        achr(0x906, 0x80a),
        achr(0x907, 0x80b),
        achr(0x908, 0x80d),
        {
            // A placed activator whose model is the skinned body
            // (`crate::timeline::ride`).
            let mut d = sub(b"NAME", &id(0x860));
            d.extend(sub(b"DATA", &placement));
            record(b"REFR", 0x909, 0, &d)
        },
    ]
    .concat();
    let hdpt = |conditions: usize| {
        let mut d = sub(b"MODL", b"actors\\head.nif\0");
        d.extend(sub(b"DATA", &[3]));
        d.extend(sub(b"PNAM", &id(1)));
        for _ in 0..conditions {
            d.extend(sub(b"CTDA", &[0; 32]));
        }
        d
    };
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
    body.extend(npc(0x803, 0x820, &sub(b"PNAM", &id(0x850)), 0));
    body.extend(npc(0x807, 0x820, &sub(b"PNAM", &id(0x851)), 0));
    body.extend(npc(0x808, 0x820, &on_list(0x831), 1));
    body.extend(npc(0x809, 0x820, &on_list(0x832), 1));
    body.extend(npc(0x80a, 0x822, &all_bits, super::TEMPLATE_BITS));
    body.extend(record(b"LVLN", 0x831, 0, &agreeing));
    body.extend(record(b"LVLN", 0x832, 0, &chance));
    body.extend(record(b"HDPT", 0x850, 0, &hdpt(0)));
    body.extend(record(b"HDPT", 0x851, 0, &hdpt(1)));
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
    // Template bit 8 alone: the outfit through TPTA slot 8, the traits own.
    let mut bit8 = sub(b"TPLT", &id(0x80c));
    let slots: Vec<u8> = (0..13)
        .flat_map(|k| if k == 8 { id(0x80c) } else { vec![0; 4] })
        .collect();
    bit8.extend(sub(b"TPTA", &slots));
    body.extend(npc(0x80b, 0x822, &bit8, 1 << 8));
    body.extend(npc(0x80c, 0x820, &sub(b"DOFT", &id(0x842)), 0));
    body.extend(record(b"OTFT", 0x842, 0, &sub(b"INAM", &id(0x823))));
    // An outfit of a list giving every entry (LVLF 4) and a list with LVLD 100.
    body.extend(npc(0x80d, 0x820, &sub(b"DOFT", &id(0x847)), 0));
    body.extend(record(
        b"OTFT",
        0x847,
        0,
        &sub(b"INAM", &[id(0x844), id(0x843)].concat()),
    ));
    let mut every = sub(b"LVLF", &[4]);
    every.extend(sub(b"LLCT", &[2]));
    every.extend(lvlo(1, 0x845));
    every.extend(lvlo(1, 0x846));
    body.extend(record(b"LVLI", 0x844, 0, &every));
    for (list, armour) in [(0x845, 0x823), (0x846, 0x824)] {
        let mut one = sub(b"LLCT", &[1]);
        one.extend(lvlo(1, armour));
        body.extend(record(b"LVLI", list, 0, &one));
    }
    let mut none = sub(b"LVLD", &[100]);
    none.extend(sub(b"LLCT", &[1]));
    none.extend(lvlo(1, 0x822));
    body.extend(record(b"LVLI", 0x843, 0, &none));
    body.extend(armo(0x824, 0x821));
    body.extend(record(
        b"ACTI",
        0x860,
        0,
        &sub(b"MODL", b"actors\\body.nif\0"),
    ));
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
                "meshes/actors/head.nif".into(),
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
    // Under actor_missing_bone_bind_pose_v1 the missing bone stays at its
    // bind transform: its vertices keep their stored places.
    let (m, missing) = pose::bone_matrices_bound(&n, &skin, &partial, "body", true, &b).unwrap();
    assert_eq!(missing, 1);
    assert_eq!(m.len(), skin.bones.len());
    // Every vertex has a place under the blend.
    assert_eq!(
        pose::skin_positions(&n, 3, &m, "body", &b).unwrap().len(),
        3
    );
    assert!(m.contains(&pose::Affine::IDENTITY));
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
    assert_eq!(actors.actors.len(), 9);
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
    // Its listed head part, skinned like an addon, under its own rule.
    let head = plan.head.as_ref().expect("head");
    assert!(!head.from_race);
    assert_eq!(head.parts.len(), 1);
    assert_eq!(head.parts[0].path, b"meshes/actors/head.nif");
    assert_eq!(head.parts[0].kind, Some(1));
    let crate::State::ReadyUnderHypotheses { hypotheses: rules } = &head.state else {
        panic!("{:?} {:?}", head.state, head.blockers)
    };
    assert!(rules.contains(&hypotheses::ACTOR_HEAD));
    assert!(!hypotheses.contains(&hypotheses::ACTOR_HEAD));
    assert!(plain.notes.iter().any(|n| n == "head: 1 parts"));
    // A listed part with conditions leaves the head out, not the actor.
    let conditional = &actors.actors[3];
    let plan = conditional.plan.as_ref().expect("resolved");
    assert!(plan.head.is_none());
    assert!(conditional.state.ready_at(crate::RuleLevel::Hypotheses));
    assert!(
        conditional
            .notes
            .iter()
            .any(|n| n.starts_with("head: part ") && n.ends_with("has conditions; not drawn")),
        "{:?}",
        conditional.notes
    );
    // A list with LVLF not zero whose entries agree, under its own rule.
    let flagged = &actors.actors[4];
    assert!(flagged.plan.is_some(), "{:?}", flagged.notes);
    let crate::State::ReadyUnderHypotheses { hypotheses: rules } = &flagged.state else {
        panic!("{:?}", flagged.state)
    };
    assert!(rules.contains(&hypotheses::ACTOR_LEVELED_FLAGS));
    assert!(rules.contains(&hypotheses::ACTOR_CANDIDATES));
    assert!(!rules.contains(&hypotheses::ACTOR_TEMPLATE_ALL_BITS));
    // A list with LVLD not zero stays refused.
    let chance = &actors.actors[5];
    assert!(chance.plan.is_none());
    assert!(
        chance.notes[0].contains("LVLD or LVLM not zero"),
        "{:?}",
        chance.notes
    );
    // Every template bit set: the template's skin, not the own one.
    let templated = &actors.actors[6];
    let plan = templated.plan.as_ref().expect("resolved");
    assert_eq!(plan.parts[0].armor.object_id, 0x820);
    let crate::State::ReadyUnderHypotheses { hypotheses: rules } = &templated.state else {
        panic!("{:?}", templated.state)
    };
    assert!(rules.contains(&hypotheses::ACTOR_TEMPLATE_ALL_BITS));
    assert!(plan.head.is_some());
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

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    }
}

#[test]
fn the_pick_and_the_roll_are_the_seeds_and_stay_in_range() {
    for seed in 0..50u64 {
        let i = pick_index(seed, &key(0x900), &key(0x830), 3);
        assert!(i < 3);
        assert_eq!(i, pick_index(seed, &key(0x900), &key(0x830), 3));
        assert!(chance_roll(seed, &key(0x900), &key(0x830)) < 100);
    }
    // Different seeds reach different entries.
    let seen: BTreeSet<usize> = (0..50u64)
        .map(|s| pick_index(s, &key(0x900), &key(0x830), 3))
        .collect();
    assert_eq!(seen.len(), 3);
    assert_eq!(pick_index(7, &key(0x900), &key(0x830), 1), 0);
}

#[test]
fn template_bits_and_the_seeded_pick_settle_looks_the_candidates_leave_open() {
    let f = fixture();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", crate::RuleLevel::Hypotheses, &b);
    let src = SourceSpan::new("test", 0, 0);
    let with = |template_bits, seed: Option<u64>, items: bool| Readings {
        template_bits,
        leveled_seed: seed,
        chance_none: items,
        use_all: items,
    };
    let mut go =
        |id: u32, r: Readings| resolve_reference_with(&mut loader, &key(id), &src, r, &b).unwrap();
    let rules = |a: &Actor| match &a.state {
        crate::State::ReadyUnderHypotheses { hypotheses } => hypotheses.clone(),
        s => panic!("{s:?} {:?}", a.notes),
    };
    // Template bit 8 alone: own and template candidates disagree; the bit
    // reading takes the traits from the NPC_ and the outfit from slot 8.
    let off = go(0x907, Readings::default());
    assert!(off.plan.is_none());
    assert_eq!(
        off.notes,
        ["template: skin: 2 candidates, outfit: 2 candidates"]
    );
    assert!(off.look.is_none() && off.keyword_base.is_none());
    let bits = go(0x907, with(true, None, false));
    let plan = bits.plan.as_ref().expect("settled");
    assert_eq!(plan.parts.len(), 1);
    assert_eq!(plan.parts[0].armor, key(0x823));
    assert!(rules(&bits).contains(&hypotheses::ACTOR_TEMPLATE_BITS));
    assert!(!rules(&bits).contains(&hypotheses::ACTOR_LEVELED_PICK));
    assert_eq!(bits.keyword_base, Some((key(0x80b), true)));
    assert!(bits
        .look
        .as_deref()
        .is_some_and(|l| l.contains("template bits")));
    // A leveled template whose entries wear different skins: with the bits
    // and a seed, the entry picked; with the seed alone, settled only when
    // the picked entry agrees with the NPC_'s own skin.
    let entries = [0x820, 0x822];
    let seeds: Vec<(u64, usize)> = (0..64u64)
        .map(|s| (s, pick_index(s, &key(0x901), &key(0x830), 2)))
        .collect();
    for (want, skin) in entries.iter().enumerate() {
        let (seed, _) = *seeds.iter().find(|(_, i)| *i == want).unwrap();
        let a = go(0x901, with(true, Some(seed), false));
        let plan = a.plan.as_ref().expect("picked");
        assert_eq!(plan.parts[0].armor, key(*skin));
        assert!(rules(&a).contains(&hypotheses::ACTOR_LEVELED_PICK));
        assert!(a
            .look
            .as_deref()
            .is_some_and(|l| l.contains(&format!("entry {} of 2", want + 1))));
        let alone = go(0x901, with(false, Some(seed), false));
        assert_eq!(alone.plan.is_some(), want == 0, "{:?}", alone.notes);
    }
    // Leveled item lists under the outfit: LVLF 4 refused without the
    // readings; picked with the seed, where the LVLD 100 list stays
    // refused; rolled to nothing with the chance reading; every entry
    // with the use-all reading.
    let off = go(0x908, Readings::default());
    assert!(off.plan.is_none());
    let seed = go(0x908, with(false, Some(1), false));
    assert!(seed.plan.is_none());
    assert!(
        seed.notes
            .iter()
            .any(|n| n.contains("LVLD or LVLM not zero")),
        "{:?}",
        seed.notes
    );
    let chance = go(
        0x908,
        Readings {
            chance_none: true,
            ..with(false, Some(1), false)
        },
    );
    assert_eq!(chance.plan.as_ref().expect("rolled").parts.len(), 1);
    assert!(rules(&chance).contains(&hypotheses::ACTOR_LEVELED_CHANCE_NONE));
    assert!(!rules(&chance).contains(&hypotheses::ACTOR_LEVELED_USE_ALL));
    let all = go(0x908, with(false, Some(1), true));
    let plan = all.plan.as_ref().expect("every entry");
    let armours: Vec<u32> = plan.parts.iter().map(|p| p.armor.object_id).collect();
    assert_eq!(armours, [0x823, 0x824]);
    assert!(rules(&all).contains(&hypotheses::ACTOR_LEVELED_USE_ALL));
    assert!(all
        .look
        .as_deref()
        .is_some_and(|l| l.contains("gave nothing")));
}

#[test]
fn a_ride_model_is_drawn_on_its_own_nodes_and_a_moved_node_carries_its_children() {
    let f = fixture();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let mut loader = Loader::new(&store, &v, "english", crate::RuleLevel::Hypotheses, &b);
    let limits = Limits::default();
    let mut cast = crate::timeline::cast::Cast::default();
    let o = crate::timeline::ride::Options {
        reference: key(0x909),
        sequence: None,
    };
    let scene = loader.by_editor_id("ActorRoom", &b).unwrap();
    let frame = crate::timeline::Timeline::default().at(0.);
    let pose =
        crate::timeline::ride::add(&mut cast, &mut loader, &scene, &frame, &o, 0., &limits, &b)
            .unwrap();
    assert!(pose.is_none());
    assert_eq!(cast.actors.actors.len(), 1);
    let a = &cast.actors.actors[0];
    let plan = a.plan.as_ref().unwrap();
    assert_eq!(plan.skeleton_asset, plan.parts[0].asset);
    let crate::State::ReadyUnderHypotheses { hypotheses: rules } = &a.state else {
        panic!("{:?}", a.state)
    };
    assert!(rules.contains(&hypotheses::MODEL_OWN_NODES));
    assert!(cast
        .ride
        .as_deref()
        .is_some_and(|l| l.contains("1 skinned shapes")));
    assert!(crate::timeline::ride::report(&cast, Some(None)).starts_with("ride\tdrawn\t"));
    // A sequence the model does not hold: not added, said why.
    let mut cast = crate::timeline::cast::Cast::default();
    let o = crate::timeline::ride::Options {
        reference: key(0x909),
        sequence: Some((0, 5.)),
    };
    let pose =
        crate::timeline::ride::add(&mut cast, &mut loader, &scene, &frame, &o, 6., &limits, &b)
            .unwrap();
    assert!(pose.is_none() && cast.actors.actors.is_empty());
    assert!(cast
        .ride
        .as_deref()
        .is_some_and(|l| l.contains("no sequence 0")));
    // A reference outside the loaded cells: not drawn.
    let mut cast = crate::timeline::cast::Cast::default();
    let o = crate::timeline::ride::Options {
        reference: key(0x999),
        sequence: None,
    };
    crate::timeline::ride::add(&mut cast, &mut loader, &scene, &frame, &o, 0., &limits, &b)
        .unwrap();
    assert!(cast
        .ride
        .as_deref()
        .is_some_and(|l| l.contains("not placed in")));
    // The run disables it: not drawn.
    let t = crate::timeline::Timeline::parse(
        "osp-run-timeline\t1\nvisibility\t1\tbase.esm:00000909\tdisabled\tplain\n",
    )
    .unwrap();
    let mut cast = crate::timeline::cast::Cast::default();
    let o = crate::timeline::ride::Options {
        reference: key(0x909),
        sequence: None,
    };
    crate::timeline::ride::add(
        &mut cast,
        &mut loader,
        &scene,
        &t.at(2.),
        &o,
        2.,
        &limits,
        &b,
    )
    .unwrap();
    assert!(cast.ride.as_deref().is_some_and(|l| l.contains("absent")));
    // A node's translation replaced: its child follows it.
    let bytes = skeleton([5., 0., 0.]);
    let n = fo4_nif::Nif::read(&bytes, "skel.nif", Game::Fallout4, &limits, &b).unwrap();
    let rest = pose::Skeleton::read(&n, "skel.nif", &limits, &b).unwrap();
    let moved: BTreeMap<usize, [f64; 3]> = [(1usize, [0., 0., 30.])].into_iter().collect();
    let worlds = pose::node_worlds_with(&n, "skel.nif", &moved, &limits, &b).unwrap();
    let ride = pose::Skeleton::of_worlds(&n, worlds, "skel.nif", &b).unwrap();
    for name in [&b"BoneA"[..], b"BoneB"] {
        let (r, m) = (rest.bones[name].translation, ride.bones[name].translation);
        assert!(
            (m[2] - r[2] - 20.).abs() < 1e-9 && (m[0] - r[0]).abs() < 1e-9,
            "{r:?} {m:?}"
        );
    }
    assert_eq!(
        rest.bones[&b"Root"[..]].translation,
        ride.bones[&b"Root"[..]].translation
    );
}
