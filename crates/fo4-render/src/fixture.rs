//! Synthetic files for tests of this crate and of the viewer (feature
//! `fixture`): a skinned model and its skeleton, a texture, and a plugin
//! with an interior cell holding placed actors and references. Every byte
//! is written here; nothing comes from a game.
use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec};
use std::path::Path;

fn sub(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn rec(tag: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut v = tag.to_vec();
    for n in [data.len() as u32, flags, id, 0] {
        v.extend(n.to_le_bytes());
    }
    v.extend(131u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(data);
    v
}
fn group(kind: u32, id: u32, body: &[u8]) -> Vec<u8> {
    let mut v = b"GRUP".to_vec();
    v.extend((body.len() as u32 + 24).to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend([0; 8]);
    v.extend(body);
    v
}
/// A 1024 x 1 DXT1 texture of one red colour.
pub fn dds() -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x21007;
    h[2] = 1;
    h[3] = 1024;
    h[6] = 1;
    h[18] = 32;
    h[19] = 4;
    h[20] = u32::from_le_bytes(*b"DXT1");
    h[26] = 0x401008;
    let mut v = b"DDS ".to_vec();
    for n in h {
        v.extend(n.to_le_bytes());
    }
    for _ in 0..256 {
        v.extend(0xf800u16.to_le_bytes());
        v.extend([0; 6]);
    }
    v
}
/// A textured skinned model: one bone "Bone" bound at z 10, and a
/// skeleton whose rest pose puts the bone at `rest_z`.
pub fn skinned_files(rest_z: f32) -> (Vec<u8>, Vec<u8>) {
    fn av(name: u32, t: [f32; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [name, 0, u32::MAX, 0] {
            v.extend(x.to_le_bytes());
        }
        for f in [t[0], t[1], t[2], 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
            v.extend(f.to_le_bytes());
        }
        v.extend(u32::MAX.to_le_bytes());
        v
    }
    fn file(blocks: &[(&str, Vec<u8>)], strings: &[&[u8]], root: u32) -> Vec<u8> {
        let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        v.extend(0x14020007u32.to_le_bytes());
        v.push(1);
        for n in [12u32, blocks.len() as u32, 130] {
            v.extend(n.to_le_bytes());
        }
        v.extend([0; 4]);
        v.extend((blocks.len() as u16).to_le_bytes());
        for (name, _) in blocks {
            v.extend((name.len() as u32).to_le_bytes());
            v.extend(name.as_bytes());
        }
        for i in 0..blocks.len() as u16 {
            v.extend(i.to_le_bytes());
        }
        for (_, b) in blocks {
            v.extend((b.len() as u32).to_le_bytes());
        }
        v.extend((strings.len() as u32).to_le_bytes());
        v.extend(8u32.to_le_bytes());
        for s in strings {
            v.extend((s.len() as u32).to_le_bytes());
            v.extend(*s);
        }
        v.extend(0u32.to_le_bytes());
        for (_, b) in blocks {
            v.extend(b);
        }
        v.extend(1u32.to_le_bytes());
        v.extend(root.to_le_bytes());
        v
    }
    let mut mesh = av(u32::MAX, [0.; 3]);
    for f in [0f32, 0., 0., 1.] {
        mesh.extend(f.to_le_bytes());
    }
    for n in [4u32, 1, u32::MAX] {
        mesh.extend(n.to_le_bytes());
    }
    // Position, UV and skin: stride 24 bytes, UV at 8, skin at 12.
    mesh.extend(((0x43u64 << 44) | (3 << 28) | (2 << 8) | 6).to_le_bytes());
    mesh.extend(1u32.to_le_bytes());
    mesh.extend(3u16.to_le_bytes());
    mesh.extend((3u32 * 24 + 6).to_le_bytes());
    // Positions (1,0,10), (1,1,10), (1,0,11) as binary16, UV zero, one bone.
    for xyz in [
        [0x3c00u16, 0, 0x4900],
        [0x3c00, 0x3c00, 0x4900],
        [0x3c00, 0, 0x4980],
    ] {
        for n in xyz {
            mesh.extend(n.to_le_bytes());
        }
        mesh.extend([0; 2]);
        mesh.extend([0; 4]);
        for w in [0x3c00u16, 0, 0, 0] {
            mesh.extend(w.to_le_bytes());
        }
        mesh.extend([0; 4]);
    }
    for i in [0u16, 1, 2] {
        mesh.extend(i.to_le_bytes());
    }
    let mut shader = Vec::new();
    for n in [0u32, u32::MAX, 0, u32::MAX, 0, 0] {
        shader.extend(n.to_le_bytes());
    }
    shader.extend([0; 16]);
    shader.extend(2i32.to_le_bytes());
    shader.extend([0; 16]);
    shader.extend(u32::MAX.to_le_bytes());
    shader.extend(0u32.to_le_bytes());
    shader.extend([0; 68]);
    let diffuse = b"textures/a.dds";
    let mut texture = 1u32.to_le_bytes().to_vec();
    texture.extend((diffuse.len() as u32).to_le_bytes());
    texture.extend(diffuse);
    let mut root = av(1, [0.; 3]);
    root.extend(2u32.to_le_bytes());
    root.extend(0u32.to_le_bytes());
    root.extend(5u32.to_le_bytes());
    let mut instance = Vec::new();
    for n in [3u32, 6, 1, 5, 0] {
        instance.extend(n.to_le_bytes());
    }
    let mut data = 1u32.to_le_bytes().to_vec();
    for f in [
        0f32, 0., 0., 9., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., -10., 1.,
    ] {
        data.extend(f.to_le_bytes());
    }
    let mut bone = av(0, [0., 0., 10.]);
    bone.extend(0u32.to_le_bytes());
    let model = file(
        &[
            ("BSTriShape", mesh),
            ("BSLightingShaderProperty", shader),
            ("BSShaderTextureSet", texture),
            ("NiNode", root),
            ("BSSkin::Instance", instance),
            ("NiNode", bone),
            ("BSSkin::BoneData", data),
        ],
        &[b"Bone", b"Root"],
        3,
    );
    let mut skeleton_root = av(1, [0.; 3]);
    skeleton_root.extend(1u32.to_le_bytes());
    skeleton_root.extend(1u32.to_le_bytes());
    let mut skeleton_bone = av(0, [0., 0., rest_z]);
    skeleton_bone.extend(0u32.to_le_bytes());
    let skeleton = file(
        &[("NiNode", skeleton_root), ("NiNode", skeleton_bone)],
        &[b"Bone", b"Root"],
        0,
    );
    (model, skeleton)
}

/// The editor id of [`actor_room`]'s cell.
pub const ACTOR_ROOM: &str = "SyntheticActorCell";
/// The record key of [`actor_room`]'s cell (`base.esm`, object 0x800).
pub const ACTOR_ROOM_CELL: u32 = 0x800;

/// The NPC_ of [`actor_room`]'s actors.
pub const ACTOR_BASE: u32 = 0x803;
/// The STAT (no model) of [`actor_room`]'s other references.
pub const MARKER_BASE: u32 = 0x830;

/// One placed record of [`actor_room`]: its object id, its base (NAME), its
/// DATA (position and rotation in radians) and its XSCL.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub id: u32,
    pub base: u32,
    pub data: [f32; 6],
    pub scale: Option<f32>,
}

/// Writes under `root` a plugin `base.esm` with the interior cell
/// [`ACTOR_ROOM`] (object 0x800) holding `actors` (ACHR; [`ACTOR_BASE`] is
/// an NPC_ whose race's skeleton is [`skinned_files`]' at rest z 12,
/// wearing its skinned model) and `markers` (REFR; [`MARKER_BASE`] is a
/// STAT without a model), with the loose files they need, and returns the
/// manifest. Object ids from 0x900 are free for placed records.
pub fn actor_room(root: &Path, actors: &[Placed], markers: &[Placed]) -> Manifest {
    std::fs::create_dir_all(root.join("meshes/actors")).expect("synthetic folder");
    std::fs::create_dir_all(root.join("textures")).expect("synthetic folder");
    let (model, skeleton) = skinned_files(12.);
    std::fs::write(root.join("meshes/actors/body.nif"), model).expect("synthetic file");
    std::fs::write(root.join("meshes/actors/skel.nif"), skeleton).expect("synthetic file");
    std::fs::write(root.join("textures/a.dds"), dds()).expect("synthetic file");
    let id = |v: u32| v.to_le_bytes().to_vec();
    let data = |p: &Placed| {
        let mut d = Vec::new();
        for f in p.data {
            d.extend(f.to_le_bytes());
        }
        let mut v = sub(b"DATA", &d);
        if let Some(s) = p.scale {
            v.extend(sub(b"XSCL", &s.to_le_bytes()));
        }
        v
    };
    let mut placed = Vec::new();
    for a in actors {
        let achr = [sub(b"NAME", &id(a.base)), data(a)].concat();
        placed.extend(rec(b"ACHR", a.id, 0, &achr));
    }
    for m in markers {
        let refr = [sub(b"NAME", &id(m.base)), data(m)].concat();
        placed.extend(rec(b"REFR", m.id, 0, &refr));
    }
    let npc = [
        sub(b"ACBS", &[0; 20]),
        sub(b"RNAM", &id(0x810)),
        sub(b"WNAM", &id(0x820)),
    ]
    .concat();
    let race = [
        sub(b"MNAM", &[]),
        sub(b"ANAM", b"actors\\skel.nif\0"),
        sub(b"FNAM", &[]),
        sub(b"ANAM", b"actors\\skel.nif\0"),
    ]
    .concat();
    let armo = [
        sub(b"RNAM", &id(0x810)),
        sub(b"INDX", &[0, 0]),
        sub(b"MODL", &id(0x821)),
    ]
    .concat();
    let arma = [
        sub(b"RNAM", &id(0x810)),
        sub(b"MOD2", b"actors\\body.nif\0"),
    ]
    .concat();
    let mut cell = sub(b"EDID", format!("{ACTOR_ROOM}\0").as_bytes());
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let body = [
        rec(b"NPC_", ACTOR_BASE, 0, &npc),
        rec(b"RACE", 0x810, 0, &race),
        rec(b"ARMO", 0x820, 0, &armo),
        rec(b"ARMA", 0x821, 0, &arma),
        rec(b"STAT", MARKER_BASE, 0, &[]),
        group(
            2,
            0,
            &[
                rec(b"CELL", ACTOR_ROOM_CELL, 0, &cell),
                group(6, ACTOR_ROOM_CELL, &group(9, ACTOR_ROOM_CELL, &placed)),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)), body].concat();
    std::fs::write(root.join("base.esm"), plugin).expect("synthetic file");
    Manifest {
        root: root.to_path_buf(),
        game: Game::Fallout4,
        archive_invalidation: None,
        language: "english".into(),
        plugins: vec![PluginSpec {
            name: "base.esm".into(),
            path: "base.esm".into(),
        }],
        mounts: vec![Mount::Loose {
            plugin: None,
            base: None,
            files: vec![
                "meshes/actors/body.nif".into(),
                "meshes/actors/skel.nif".into(),
                "textures/a.dds".into(),
            ],
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    }
}
