//! Fully synthetic SCOL: an untextured generated alternative and two textured parts.
use crate::draw::{build, BuildOptions};
use crate::fixture::{dds, skinned_files};
use fo4_core::{Budget, Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use fo4_scene::{Loader, RuleLevel};
#[path = "../../fo4-material/src/test_builder.rs"]
mod material_fixture;
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
fn nif(diffuse: &[u8]) -> Vec<u8> {
    nif_kind(diffuse, None)
}
fn nif_kind(diffuse: &[u8], effect: Option<u32>) -> Vec<u8> {
    nif_with_helper(diffuse, effect, b"EditorMarker:27")
}
fn nif_with_helper(diffuse: &[u8], effect: Option<u32>, label: &[u8]) -> Vec<u8> {
    nif_named(diffuse, effect, label, b"")
}
/// The model with string 0, the shader's `Name`, set to `material`.
fn nif_named(diffuse: &[u8], effect: Option<u32>, label: &[u8], material: &[u8]) -> Vec<u8> {
    nif_vertices(diffuse, effect, label, material, None)
}
/// [`nif_named`] whose shapes' vertices also carry, from `extra`, a normal
/// (four normbytes, nifxml `ByteVector3` and the bitangent's Y) and a
/// colour (RGBA bytes).
fn nif_vertices(
    diffuse: &[u8],
    effect: Option<u32>,
    label: &[u8],
    material: &[u8],
    extra: Option<[[u8; 8]; 3]>,
) -> Vec<u8> {
    nif_effect_textures(diffuse, effect, label, material, extra, (b"", b""), None)
}
/// [`nif_vertices`] whose effect shader names `textures` (source,
/// greyscale); with `lighting`, the lighting shader's Shader Flags 2 and
/// an NiAlphaProperty's flags (block 5, named by the shapes).
fn nif_effect_textures(
    diffuse: &[u8],
    effect: Option<u32>,
    label: &[u8],
    material: &[u8],
    extra: Option<[[u8; 8]; 3]>,
    textures: (&[u8], &[u8]),
    lighting: Option<(u32, u16)>,
) -> Vec<u8> {
    let blocks = model_blocks(diffuse, effect, extra, textures, lighting);
    nif_file(blocks, label, material)
}
/// The blocks of [`nif_effect_textures`]: shape (0), shader (1), texture
/// set (2), root node (3), helper shape (4), and with `lighting` an
/// NiAlphaProperty (5).
fn model_blocks(
    diffuse: &[u8],
    effect: Option<u32>,
    extra: Option<[[u8; 8]; 3]>,
    textures: (&[u8], &[u8]),
    lighting: Option<(u32, u16)>,
) -> Vec<(&'static str, Vec<u8>)> {
    let mut av = Vec::new();
    for x in [u32::MAX, 0, u32::MAX, 0] {
        av.extend(x.to_le_bytes());
    }
    for f in [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
        av.extend(f.to_le_bytes());
    }
    av.extend(u32::MAX.to_le_bytes());
    let mut mesh = av.clone();
    for f in [0f32, 0., 0., 1.] {
        mesh.extend(f.to_le_bytes());
    }
    let alpha_ref = if lighting.is_some() { 5 } else { u32::MAX };
    for n in [u32::MAX, 1, alpha_ref] {
        mesh.extend(n.to_le_bytes());
    }
    // Position and UV; with `extra`, normal at byte 12 and colour at 16.
    let desc = match extra {
        None => (3u64 << 44) | (2 << 8) | 3,
        Some(_) => (0x2bu64 << 44) | (4 << 24) | (3 << 16) | (2 << 8) | 5,
    };
    mesh.extend(desc.to_le_bytes());
    mesh.extend(1u32.to_le_bytes());
    mesh.extend(3u16.to_le_bytes());
    mesh.extend(if extra.is_some() { 66u32 } else { 42 }.to_le_bytes());
    for (k, xyz) in [[0x3c00u16, 0, 0], [0x3c00, 0x3800, 0], [0x3c00, 0, 0x3800]]
        .into_iter()
        .enumerate()
    {
        for n in xyz {
            mesh.extend(n.to_le_bytes());
        }
        mesh.extend([0; 6]);
        if let Some(extra) = extra {
            mesh.extend(extra[k]);
        }
    }
    for i in [0u16, 1, 2] {
        mesh.extend(i.to_le_bytes());
    }
    let mut shader = Vec::new();
    for n in [0u32, 0, 0, u32::MAX, 0, 0] {
        shader.extend(n.to_le_bytes());
    }
    shader.extend([0; 16]);
    shader.extend(2i32.to_le_bytes());
    shader.extend([0; 16]);
    shader.extend(u32::MAX.to_le_bytes());
    shader.extend(0u32.to_le_bytes());
    shader.extend([0; 68]);
    if let Some((flags2, _)) = lighting {
        shader[20..24].copy_from_slice(&flags2.to_le_bytes());
    }
    if let Some(flags1) = effect {
        shader.clear();
        for n in [0u32, 0, u32::MAX, flags1, 0] {
            shader.extend(n.to_le_bytes());
        }
        for v in [0f32, 0., 1., 1.] {
            shader.extend(v.to_le_bytes());
        }
        shader.extend((textures.0.len() as u32).to_le_bytes());
        shader.extend(textures.0);
        shader.extend([3, 255, 0, 0]);
        for v in [1f32, 1., 0., 0., 0.2, 0.4, 0.6, 0.8, 2., 100.] {
            shader.extend(v.to_le_bytes());
        }
        shader.extend((textures.1.len() as u32).to_le_bytes());
        shader.extend(textures.1);
        for _ in 0..3 {
            shader.extend(0u32.to_le_bytes());
        }
        shader.extend(1f32.to_le_bytes());
    }
    let mut texture = 1u32.to_le_bytes().to_vec();
    texture.extend((diffuse.len() as u32).to_le_bytes());
    texture.extend(diffuse);
    av[..4].copy_from_slice(&1u32.to_le_bytes());
    av.extend(1u32.to_le_bytes());
    av.extend(0u32.to_le_bytes());
    let mut helper_mesh = mesh.clone();
    helper_mesh[..4].copy_from_slice(&2u32.to_le_bytes());
    let mut blocks = vec![
        ("BSTriShape", mesh),
        (
            if effect.is_some() {
                "BSEffectShaderProperty"
            } else {
                "BSLightingShaderProperty"
            },
            shader,
        ),
        ("BSShaderTextureSet", texture),
        ("NiNode", av),
        ("BSTriShape", helper_mesh),
    ];
    if let Some((_, flags)) = lighting {
        let mut a = Vec::new();
        for n in [u32::MAX, 0, u32::MAX] {
            a.extend(n.to_le_bytes());
        }
        a.extend(flags.to_le_bytes());
        a.push(128);
        blocks.push(("NiAlphaProperty", a));
    }
    blocks
}
/// A NIF file of `blocks` (each its own block type), with the strings
/// `material` (index 0), `EditorMarker` and `label`, roots 0, 3 and 4.
fn nif_file(blocks: Vec<(&'static str, Vec<u8>)>, label: &[u8], material: &[u8]) -> Vec<u8> {
    let count = blocks.len() as u32;
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, count, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend((count as u16).to_le_bytes());
    for (name, _) in &blocks {
        v.extend((name.len() as u32).to_le_bytes());
        v.extend(name.as_bytes());
    }
    for i in 0..count as u16 {
        v.extend(i.to_le_bytes());
    }
    for (_, b) in &blocks {
        v.extend((b.len() as u32).to_le_bytes());
    }
    for n in [3u32, 15u32.max(label.len().max(material.len()) as u32)] {
        v.extend(n.to_le_bytes());
    }
    v.extend((material.len() as u32).to_le_bytes());
    v.extend(material);
    v.extend(12u32.to_le_bytes());
    v.extend(b"EditorMarker");
    v.extend((label.len() as u32).to_le_bytes());
    v.extend(label);
    v.extend(0u32.to_le_bytes());
    for (_, b) in blocks {
        v.extend(b);
    }
    v.extend(3u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend(3u32.to_le_bytes());
    v.extend(4u32.to_le_bytes());
    v
}
/// The SCOL cell with its loose files, `texture` as the parts' diffuse.
fn scol_fixture(name: &str, texture: &[u8]) -> (TestDir, Manifest) {
    scol_fixture_with(name, texture, nif(b"textures/a.dds"))
}
/// [`scol_fixture`] with `part` as the parts' model.
fn scol_fixture_with(name: &str, texture: &[u8], part: Vec<u8>) -> (TestDir, Manifest) {
    fixture_placing(name, texture, part, 0x802)
}
/// [`scol_fixture_with`] whose reference places `placed` (0x801 the part
/// STAT itself, 0x802 the SCOL).
fn fixture_placing(name: &str, texture: &[u8], part: Vec<u8>, placed: u32) -> (TestDir, Manifest) {
    fixture_records(name, texture, part, placed, &[], &[])
}
/// [`fixture_placing`] whose part STAT also holds `part_extra` subrecords,
/// with `records` added to the plugin.
fn fixture_records(
    name: &str,
    texture: &[u8],
    part: Vec<u8>,
    placed: u32,
    part_extra: &[u8],
    records: &[u8],
) -> (TestDir, Manifest) {
    fixture_files(name, texture, part, placed, (part_extra, &[]), records, &[])
}
/// [`fixture_records`] whose reference also holds `extra.1` subrecords
/// (the part STAT `extra.0`), with `files` (path under the data folder,
/// bytes) written and mounted too.
fn fixture_files(
    name: &str,
    texture: &[u8],
    part: Vec<u8>,
    placed: u32,
    extra: (&[u8], &[u8]),
    records: &[u8],
    files: &[(&str, Vec<u8>)],
) -> (TestDir, Manifest) {
    let (part_extra, reference_extra) = extra;
    let temp = TestDir::new(name);
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(root.join("meshes/generated.nif"), nif(b"")).unwrap();
    std::fs::write(root.join("meshes/part.nif"), part).unwrap();
    std::fs::write(root.join("textures/a.dds"), texture).unwrap();
    let mut scol = sub(b"MODL", b"generated.nif\0");
    scol.extend(sub(b"ONAM", &0x801u32.to_le_bytes()));
    let mut p = Vec::new();
    for z in [-0.4f32, 0.4] {
        for n in [0., 0., z, 0., 0., 0., 1.] {
            p.extend(n.to_le_bytes());
        }
    }
    scol.extend(sub(b"DATA", &p));
    let mut rf = sub(b"NAME", &placed.to_le_bytes());
    rf.extend(sub(b"DATA", &[0; 24]));
    rf.extend(sub(b"XSCL", &1f32.to_le_bytes()));
    rf.extend(reference_extra);
    let mut mounted: Vec<String> = vec![
        "meshes/generated.nif".into(),
        "meshes/part.nif".into(),
        "textures/a.dds".into(),
    ];
    for (path, bytes) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
        mounted.push((*path).into());
    }
    let mut cell = sub(b"EDID", b"SyntheticScolCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let body = [
        rec(
            b"STAT",
            0x801,
            0,
            &[sub(b"MODL", b"part.nif\0").as_slice(), part_extra].concat(),
        ),
        rec(b"SCOL", 0x802, 0, &scol),
        records.to_vec(),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(6, 0x800, &group(9, 0x800, &rec(b"REFR", 0x803, 0, &rf))),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)), body].concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
            files: mounted.iter().map(Into::into).collect(),
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    };
    (temp, m)
}
#[test]
fn scol_hypothesis_draws_two_textured_parts_without_the_generated_alternative() {
    let (_temp, m) = scol_fixture("render-scol", &dds());
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let mut scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    assert!(
        scene.instances[0]
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::SCOL),
        "{:?}",
        scene.instances[0].blockers
    );
    let too_small = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1024,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        &Budget::new(32 << 20),
    )
    .unwrap();
    assert!(too_small.list.draws.is_empty());
    assert_eq!(too_small.omitted[0].causes[0].code, "LimitExceeded");
    let output = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        &b,
    )
    .unwrap();
    assert!(output.omitted.is_empty(), "{:?}", output.omitted);
    // The draws' node chains only when asked, the draws unchanged.
    assert!(output.chains.is_empty());
    let chained = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: true,
        },
        &b,
    )
    .unwrap();
    assert_eq!(
        format!("{:?}", chained.list.draws),
        format!("{:?}", output.list.draws)
    );
    assert_eq!(chained.chains.len(), output.list.draws.len());
    for (k, c) in chained.chains.iter().enumerate() {
        assert_eq!(c.draw, k);
        assert!(!c.blocks.is_empty() && c.model.ends_with(b".nif"), "{c:?}");
    }
    assert_eq!(output.skipped_editor_marker_subtrees, 4);
    assert!(!scene.instances[0].editor_marker);
    assert_eq!(output.list.draws.len(), 2);
    assert_eq!(output.list.meshes.len(), 1);
    assert_eq!(output.list.textures.len(), 1);
    assert_eq!(
        output.list.draws[0].world.translation,
        [0., 0., -0.4f32 as f64]
    );
    assert_eq!(
        output.list.draws[1].world.translation,
        [0., 0., 0.4f32 as f64]
    );
    // Retaining helpers is an explicit diagnostic and never mutates the scene policy.
    let before = scene.instances[0].state.hypotheses().to_vec();
    let retained = crate::draw::build_with_marker_visibility(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        true,
        &Budget::new(32 << 20),
    )
    .unwrap();
    assert_eq!(retained.skipped_editor_marker_subtrees, 0);
    assert!(retained.list.draws.len() > output.list.draws.len());
    assert_eq!(scene.instances[0].state.hypotheses(), before);
    scene.instances[0].state = fo4_scene::State::ReadyUnderHypotheses {
        hypotheses: vec![fo4_scene::hypotheses::SCOL],
    };
    let original_hypotheses = match &scene.instances[0].state {
        fo4_scene::State::ReadyUnderHypotheses { hypotheses } => hypotheses.clone(),
        _ => panic!("expected hypotheses"),
    };
    let marker_guard = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        &b,
    )
    .unwrap();
    assert!(marker_guard.list.draws.is_empty());
    assert_eq!(
        marker_guard.omitted[0].causes[0].code,
        "UnverifiedNifEditorMarkerVisibility"
    );
    scene.instances[0].state = fo4_scene::State::ReadyUnderHypotheses {
        hypotheses: original_hypotheses,
    };
    if let fo4_scene::State::ReadyUnderHypotheses { hypotheses } = &mut scene.instances[0].state {
        if !hypotheses.contains(&fo4_scene::hypotheses::NAMED_MARKER) {
            hypotheses.push(fo4_scene::hypotheses::NAMED_MARKER);
        }
        hypotheses.retain(|h| *h != fo4_scene::hypotheses::MARKER_SUFFIX);
    }
    let suffix_guard = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 256,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        &Budget::new(32 << 20),
    )
    .unwrap();
    assert!(suffix_guard.list.draws.is_empty());
    assert_eq!(
        suffix_guard.omitted[0].causes[0].code,
        "UnverifiedNifEditorMarkerSuffixVisibility"
    );
    scene.instances[0].state = fo4_scene::State::Ready;
    let guarded = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        },
        &b,
    )
    .unwrap();
    assert!(guarded.list.draws.is_empty());
    assert_eq!(
        guarded.omitted[0].causes[0].code,
        "UnverifiedScolGeometrySource"
    );
}

#[test]
fn untextured_effect_requires_hypothesis_and_refuses_unhandled_features() {
    for (flags, expected_error) in [
        (0, None),
        (1 << 6, Some("UnsupportedColorOnlyEffectFeatures")),
        (1 << 3, Some("MissingEnabledEffectVertexColor")),
    ] {
        let temp = TestDir::new("effect-color");
        let root = temp.join("Data");
        std::fs::create_dir_all(root.join("meshes")).unwrap();
        std::fs::write(root.join("meshes/effect.nif"), nif_kind(b"", Some(flags))).unwrap();
        let mut reference = sub(b"NAME", &0x801u32.to_le_bytes());
        reference.extend(sub(b"DATA", &[0; 24]));
        reference.extend(sub(b"XSCL", &1f32.to_le_bytes()));
        let mut cell = sub(b"EDID", b"EffectCell\0");
        cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend([0; 8]);
        let plugin = [
            rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)),
            rec(b"STAT", 0x801, 0, &sub(b"MODL", b"effect.nif\0")),
            group(
                2,
                0,
                &[
                    rec(b"CELL", 0x800, 0, &cell),
                    group(
                        6,
                        0x800,
                        &group(9, 0x800, &rec(b"REFR", 0x802, 0, &reference)),
                    ),
                ]
                .concat(),
            ),
        ]
        .concat();
        std::fs::write(root.join("base.esm"), plugin).unwrap();
        let m = Manifest {
            root,
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
                files: vec!["meshes/effect.nif".into()],
            }],
            required_masters: vec![],
            excluded: vec![],
            limits: Limits::default(),
            build: Some("synthetic".into()),
        };
        let b = Budget::new(32 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
        let mut scene = loader.by_editor_id("EffectCell", &b).unwrap();
        assert!(
            scene.instances[0]
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::EFFECT_COLOR),
            "{:?}",
            scene.instances[0].blockers
        );
        let options = BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1024,
            level: fo4_scene::RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        };
        let output = build(&scene, &loader, &m.limits, options, &b).unwrap();
        if let Some(code) = expected_error {
            assert!(output.list.draws.is_empty());
            assert_eq!(output.omitted[0].causes[0].code, code);
        } else {
            assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
            assert_eq!(output.list.textures[0].rgba, [0.4, 0.8, 1.2, 1.]);
            assert_eq!(output.list.draws[0].alpha.opacity, 0.8);
            assert!(output.texture_choices[0].declared_fourcc.is_none());
            assert!(output.texture_choices[0].declared_dxgi.is_none());
            scene.instances[0].state = fo4_scene::State::ReadyUnderHypotheses {
                hypotheses: vec![
                    fo4_scene::hypotheses::NAMED_MARKER,
                    fo4_scene::hypotheses::MARKER_SUFFIX,
                ],
            };
            let rejected = build(&scene, &loader, &m.limits, options, &b).unwrap();
            assert!(rejected.list.draws.is_empty());
            assert_eq!(
                rejected.omitted[0].causes[0].code,
                "UnverifiedEffectColorOnly"
            );
        }
    }
}

#[test]
fn visibility_helper_rule_matches_only_the_literal_mesh_label() {
    for (label, expected) in [
        (b"VisibilityEditorMarker:0".as_slice(), true),
        (b"VisibilityEditorMarker:1".as_slice(), false),
        (b"visibilityEditorMarker:0".as_slice(), false),
        (b"EditorMarker:0".as_slice(), false),
    ] {
        let bytes = nif_with_helper(b"textures/a.dds", None, label);
        let n = fo4_nif::Nif::read(
            &bytes,
            "synthetic helper",
            Game::Fallout4,
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap();
        assert_eq!(
            fo4_scene::model_policy::visibility_editor_marker(&n, 4),
            expected
        );
        assert!(!fo4_scene::model_policy::visibility_editor_marker(&n, 3));
    }
}

/// An 8 x 8 DXT1 texture without alpha and its full chain down to 1 x 1.
fn dds_chain() -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x21007;
    h[2] = 8;
    h[3] = 8;
    h[6] = 4;
    h[18] = 32;
    h[19] = 4;
    h[20] = u32::from_le_bytes(*b"DXT1");
    h[26] = 0x401008;
    let mut v = b"DDS ".to_vec();
    for n in h {
        v.extend(n.to_le_bytes());
    }
    for k in 0..7u8 {
        v.extend(0xf800u16.to_le_bytes());
        v.extend([k; 6]);
    }
    v
}

#[test]
fn stored_blocks_are_kept_from_the_first_level_within_the_cap() {
    use crate::draw::BlockOptions;
    let blocks = |max_dimension| {
        BuildOptions::with_blocks(
            RuleLevel::Hypotheses,
            BlockOptions {
                max_dimension,
                max_alpha_tested_dimension: 2,
                max_bytes: 1 << 20,
            },
        )
    };
    let (_temp, m) = scol_fixture("render-blocks", &dds_chain());
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    // The NIF has no alpha property: the plain limit applies.
    let full = build(&scene, &loader, &m.limits, blocks(8), &b).unwrap();
    assert!(full.omitted.is_empty(), "{:?}", full.omitted);
    let t = &full.list.textures[0];
    let kept = &full.list.blocks[&0];
    assert_eq!((t.width, t.height, t.mip), (8, 8, 0));
    assert_eq!(
        (kept.format, kept.levels, kept.bytes.len()),
        (crate::BlockFormat::Bc1, 4, 56)
    );
    // DXT1 declared without alpha: the reference reads alpha 1.
    assert!(kept.opaque && t.rgba.is_empty() && t.mips.is_empty());
    full.list.validate_texture(0).unwrap();
    assert!(
        t.validate().is_err(),
        "without its blocks the texture has no texels"
    );
    let e = crate::render(
        &full.list,
        &fo4_scene::camera::look_at(
            [0., -5., 0.],
            [0., 0., 0.],
            fo4_scene::camera::reference_frustum([8, 8]),
            [8, 8],
        )
        .unwrap(),
        crate::RenderOptions::REFERENCE,
        &m.limits,
        &b,
    )
    .unwrap_err();
    assert_eq!(e.code, "CompressedTextureNotDecoded");
    let e = crate::snapshot::write(&full.list, &mut std::io::sink(), &m.limits).unwrap_err();
    assert_eq!(e.code, "SnapshotBlockTexture");
    let half = build(&scene, &loader, &m.limits, blocks(4), &b).unwrap();
    let t = &half.list.textures[0];
    let kept = &half.list.blocks[&0];
    assert_eq!(
        (t.width, t.mip, kept.levels, kept.bytes.len()),
        (4, 1, 3, 24)
    );
    assert_eq!(&kept.bytes[..2], &0xf800u16.to_le_bytes());
    assert_eq!(kept.bytes[2], 4, "level 1 is the fifth stored block");
    // The level a cap keeps, without keeping it (the sharing of one path's
    // entries between the plain and alpha-tested caps compares these).
    let id = loader
        .assets
        .nodes
        .iter()
        .position(|a| a.kind == fo4_scene::AssetKind::Texture)
        .unwrap();
    assert_eq!(
        crate::draw::block_level(&loader, id, 8, &b).unwrap(),
        Some(0)
    );
    assert_eq!(
        crate::draw::block_level(&loader, id, 4, &b).unwrap(),
        Some(1)
    );
    assert_eq!(crate::draw::block_level(&loader, id, 0, &b).unwrap(), None);
    // Where the memory goes: one texture of stored blocks, drawn by a
    // placed reference only.
    let report = crate::memory::report(&half);
    assert!(report.starts_with("textures\t1\t"), "{report}");
    assert!(report.contains("  as stored blocks\t0.0 MiB\n"), "{report}");
    assert!(
        report.contains("  drawn by placed references only\t1\t"),
        "{report}"
    );
    assert!(report.contains("paths kept more than once\t0;"), "{report}");
    assert!(report.contains("actors drawn\t0;"), "{report}");
    // A level whose sides are not multiples of 4 is decoded instead, under
    // the reference limits.
    let small = build(&scene, &loader, &m.limits, blocks(2), &b).unwrap();
    assert!(small.list.blocks.is_empty());
    assert_eq!(small.list.textures[0].width, 8);
    assert_eq!(small.list.textures[0].rgba.len(), 8 * 8 * 4);
    // The reference options never keep blocks.
    let reference = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions::reference(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(reference.list.blocks.is_empty());
    // Too small a block budget omits the object, as the decoded budget does.
    let mut tight = blocks(8);
    tight.blocks.as_mut().unwrap().max_bytes = 16;
    let omitted = build(&scene, &loader, &m.limits, tight, &b).unwrap();
    assert!(omitted.list.draws.is_empty());
    assert_eq!(omitted.omitted[0].causes[0].code, "LimitExceeded");
}

#[test]
fn block_textures_validate_their_level_sizes() {
    let texture = |w: u32, h: u32| crate::Texture {
        mips: Vec::new(),
        width: w,
        height: h,
        rgba: Vec::new(),
        mip: 0,
        srgb: false,
    };
    let blocks = |levels: u32, n: usize| crate::Blocks {
        format: crate::BlockFormat::Bc3,
        levels,
        bytes: vec![0; n],
        opaque: false,
    };
    // 8 x 4: two blocks, then 4 x 2 and 2 x 1 one block each, 16 bytes a block.
    texture(8, 4).validate_blocks(&blocks(3, 64)).unwrap();
    for (t, b) in [
        (texture(8, 4), blocks(3, 48)),
        (texture(6, 4), blocks(1, 32)),
        (texture(8, 4), blocks(0, 0)),
        (texture(8, 4), blocks(33, 64)),
    ] {
        assert_eq!(
            t.validate_blocks(&b).unwrap_err().code,
            "InvalidBlockTexture"
        );
    }
    let mut with_rgba = texture(8, 4);
    with_rgba.rgba = vec![0.0; 128];
    assert!(with_rgba.validate_blocks(&blocks(1, 32)).is_err());
    let mut list = crate::DrawList::default();
    list.textures.push(texture(8, 4));
    list.blocks.insert(0, blocks(1, 32));
    list.validate_texture(0).unwrap();
    assert_eq!(
        list.validate_texture(1).unwrap_err().code,
        "RenderTextureIndex"
    );
}

#[test]
fn a_placed_actor_is_drawn_skinned_in_its_skeleton_rest_pose_after_the_references() {
    let temp = TestDir::new("render-actor");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes/actors")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    let (model, skeleton) = skinned_files(12.);
    std::fs::write(root.join("meshes/actors/body.nif"), model).unwrap();
    std::fs::write(root.join("meshes/actors/skel.nif"), skeleton).unwrap();
    std::fs::write(root.join("textures/a.dds"), dds()).unwrap();
    let id = |v: u32| v.to_le_bytes().to_vec();
    let mut placement = Vec::new();
    for f in [100f32, 200., 300., 0., 0., 0.] {
        placement.extend(f.to_le_bytes());
    }
    let achr = [sub(b"NAME", &id(0x803)), sub(b"DATA", &placement)].concat();
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
    let mut cell = sub(b"EDID", b"SyntheticActorCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let body = [
        rec(b"NPC_", 0x803, 0, &npc),
        rec(b"RACE", 0x810, 0, &race),
        rec(b"ARMO", 0x820, 0, &armo),
        rec(b"ARMA", 0x821, 0, &arma),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(6, 0x800, &group(9, 0x800, &rec(b"ACHR", 0x804, 0, &achr))),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)), body].concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
    };
    let b = Budget::new(64 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id("SyntheticActorCell", &b).unwrap();
    let actors = fo4_scene::actor::resolve(&scene, &mut loader, &b).unwrap();
    assert_eq!(actors.actors.len(), 1);
    assert!(
        actors.actors[0].plan.is_some(),
        "{:?}",
        actors.actors[0].notes
    );
    // Without actors the draw list is the one of the scene alone.
    let plain = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions::reference(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(plain.instances.is_empty() && plain.list.draws.is_empty() && plain.actors.is_empty());
    let with = crate::draw::build_with_actors(
        &scene,
        &actors,
        &loader,
        &m.limits,
        BuildOptions::reference(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(with.omitted.is_empty(), "{:?}", with.omitted);
    assert_eq!(with.actors, [0]);
    assert_eq!(with.list.draws.len(), 1);
    let d = &with.list.draws[0];
    assert_eq!(d.world.translation, [100., 200., 300.]);
    // Bound at z 10, rest at 12: every vertex 2 higher than stored.
    let p: Vec<[f64; 3]> = with.list.meshes[d.mesh]
        .vertices
        .iter()
        .map(|v| v.position)
        .collect();
    assert_eq!(p, [[1., 0., 12.], [1., 1., 12.], [1., 0., 13.]]);
    // At the measured level the pending actor rules leave it out, with a cause.
    let measured = crate::draw::build_with_actors(
        &scene,
        &actors,
        &loader,
        &m.limits,
        BuildOptions::reference(RuleLevel::Measured),
        &b,
    )
    .unwrap();
    assert!(measured.list.draws.is_empty());
    assert_eq!(measured.omitted.len(), 1);
    assert_eq!(measured.omitted[0].causes[0].code, "ActorNotDrawable");
    // Posed: no pose given draws as without poses and records nothing.
    let options = BuildOptions::reference(RuleLevel::Hypotheses);
    let (same, recorded) = crate::draw::build_with_posed_actors(
        &scene,
        &actors,
        Default::default(),
        &loader,
        &m.limits,
        options,
        &b,
    )
    .unwrap();
    assert!(recorded.is_empty());
    let q: Vec<[f64; 3]> = same.list.meshes[same.list.draws[0].mesh]
        .vertices
        .iter()
        .map(|v| v.position)
        .collect();
    assert_eq!(q, p);
    // A pose with the bone 5 higher than at rest moves every vertex by 5,
    // and the recorded shape reproduces the drawn positions.
    let skeleton = actors.actors[0].plan.as_ref().unwrap().skeleton_asset;
    let mut pose = fo4_scene::actor::pose::Skeleton::default();
    let mut bone = fo4_scene::actor::pose::Affine::IDENTITY;
    bone.translation = [0., 0., 17.];
    pose.bones.insert(b"Bone".to_vec(), bone);
    pose.bones
        .insert(b"Root".to_vec(), fo4_scene::actor::pose::Affine::IDENTITY);
    let poses = [(skeleton, pose.clone())].into_iter().collect();
    let (posed, recorded) = crate::draw::build_with_posed_actors(
        &scene, &actors, poses, &loader, &m.limits, options, &b,
    )
    .unwrap();
    assert!(posed.omitted.is_empty());
    let d = &posed.list.draws[0];
    let r: Vec<[f64; 3]> = posed.list.meshes[d.mesh]
        .vertices
        .iter()
        .map(|v| v.position)
        .collect();
    assert_eq!(r, [[1., 0., 17.], [1., 1., 17.], [1., 0., 18.]]);
    assert_eq!(recorded.len(), 1);
    assert_eq!((recorded[0].mesh, recorded[0].skeleton), (d.mesh, skeleton));
    let mut m4 = Vec::new();
    recorded[0].shape.matrices(&pose, "skel", &mut m4).unwrap();
    let mut again = Vec::new();
    recorded[0].shape.positions(&m4, &mut again);
    assert_eq!(again, r);
    pose.bones.remove(&b"Bone"[..]);
    assert_eq!(
        recorded[0]
            .shape
            .matrices(&pose, "skel", &mut m4)
            .unwrap_err()
            .code,
        "ActorSkinBoneMissing"
    );
}

/// One placed effect model (`nif_kind` with Shader Flags 1 `flags`, its
/// Base Color Scale 2 replaced by `scale`), loaded at the hypotheses level,
/// its state given `nif_controller_rest_effect_first_key_v1` as a settled
/// controller would.
fn effect_surface_build(flags: u32, scale: f32) -> crate::draw::Build {
    effect_surface_build_with(EffectFixture {
        flags1: flags,
        scale,
        ..EffectFixture::default()
    })
}
/// What [`effect_surface_build_with`] changes in the placed effect model.
#[derive(Clone, Copy)]
struct EffectFixture {
    flags1: u32,
    flags2: u32,
    scale: f32,
    /// Falloff Start Angle, Stop Angle, Start Opacity, Stop Opacity.
    falloff: [f32; 4],
    /// Per vertex: normal bytes and colour bytes ([`nif_vertices`]).
    extra: Option<[[u8; 8]; 3]>,
    level: RuleLevel,
    /// Source and greyscale textures the shader names, each written as a
    /// one-row RGBA8 DDS of the given texels when named.
    source: Option<&'static [[u8; 4]]>,
    greyscale: Option<&'static [[u8; 4]]>,
}
impl Default for EffectFixture {
    fn default() -> Self {
        Self {
            flags1: 0,
            flags2: 0,
            scale: 2.,
            falloff: [1., 1., 0., 0.],
            extra: None,
            level: RuleLevel::Hypotheses,
            source: None,
            greyscale: None,
        }
    }
}
/// An RGBA8 DDS of one row of `texels` (legacy header, byte masks).
fn dds_rgba(texels: &[[u8; 4]]) -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x100f;
    h[2] = 1;
    h[3] = texels.len() as u32;
    h[4] = 4 * texels.len() as u32;
    h[6] = 1;
    h[18] = 32;
    h[19] = 0x41;
    h[21] = 32;
    h[22] = 0xff;
    h[23] = 0xff00;
    h[24] = 0xff0000;
    h[25] = 0xff00_0000;
    h[26] = 0x1000;
    let mut v = b"DDS ".to_vec();
    for n in h {
        v.extend(n.to_le_bytes());
    }
    v.extend(texels.iter().flatten());
    v
}
fn effect_surface_build_with(x: EffectFixture) -> crate::draw::Build {
    let temp = TestDir::new("effect-surface");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    let names: (&[u8], &[u8]) = (
        if x.source.is_some() {
            b"textures/src.dds"
        } else {
            b""
        },
        if x.greyscale.is_some() {
            b"textures/pal.dds"
        } else {
            b""
        },
    );
    let mut files = vec!["meshes/effect.nif".to_string()];
    std::fs::create_dir_all(root.join("textures")).unwrap();
    for (name, texels) in [("src.dds", x.source), ("pal.dds", x.greyscale)] {
        if let Some(texels) = texels {
            std::fs::write(root.join("textures").join(name), dds_rgba(texels)).unwrap();
            files.push(format!("textures/{name}"));
        }
    }
    let mut bytes = nif_effect_textures(
        b"",
        Some(x.flags1),
        b"EditorMarker:27",
        b"",
        x.extra,
        names,
        None,
    );
    let patch = |bytes: &mut Vec<u8>, old: &[f32], new: &[f32]| {
        let old: Vec<u8> = old.iter().flat_map(|v| v.to_le_bytes()).collect();
        let at = bytes
            .windows(old.len())
            .position(|w| w == old.as_slice())
            .unwrap();
        let new: Vec<u8> = new.iter().flat_map(|v| v.to_le_bytes()).collect();
        bytes[at..at + new.len()].copy_from_slice(&new);
    };
    patch(&mut bytes, &[2., 100.], &[x.scale]);
    patch(&mut bytes, &[1., 1., 0., 0., 0.2], &x.falloff);
    // Shader Flags 2 follows Shader Flags 1, before UV offset 0 and scale 1.
    let flags: Vec<u8> = [0, 0, u32::MAX, x.flags1, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .chain([0f32, 0., 1., 1.].iter().flat_map(|v| v.to_le_bytes()))
        .collect();
    let at = bytes
        .windows(36)
        .position(|w| w == flags.as_slice())
        .unwrap();
    bytes[at + 16..at + 20].copy_from_slice(&x.flags2.to_le_bytes());
    std::fs::write(root.join("meshes/effect.nif"), bytes).unwrap();
    let mut reference = sub(b"NAME", &0x801u32.to_le_bytes());
    reference.extend(sub(b"DATA", &[0; 24]));
    reference.extend(sub(b"XSCL", &1f32.to_le_bytes()));
    let mut cell = sub(b"EDID", b"EffectCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [
        rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)),
        rec(b"STAT", 0x801, 0, &sub(b"MODL", b"effect.nif\0")),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(
                    6,
                    0x800,
                    &group(9, 0x800, &rec(b"REFR", 0x802, 0, &reference)),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
            files: files.iter().map(Into::into).collect(),
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    };
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
    let mut scene = loader.by_editor_id("EffectCell", &b).unwrap();
    let mut hypotheses = scene.instances[0].state.hypotheses().to_vec();
    hypotheses.push(fo4_scene::hypotheses::REST_EFFECT_FIRST_KEY);
    scene.instances[0].state = fo4_scene::State::ReadyUnderHypotheses { hypotheses };
    let options = BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1024,
        level: x.level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    build(&scene, &loader, &m.limits, options, &b).unwrap()
}

#[test]
fn settled_effect_surfaces_take_the_base_colour_and_name_every_unsettled_term() {
    // Untextured, no vertex flags, base colour (0.2, 0.4, 0.6, 0.8) x 0.5.
    let output = effect_surface_build(0, 0.5);
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    let draw = &output.list.draws[0];
    assert_eq!(output.list.textures[draw.texture].rgba, [1.; 4]);
    assert_eq!(draw.alpha.opacity, 0.8);
    let colors = output.list.meshes[draw.mesh]
        .vertex_colors
        .as_ref()
        .unwrap();
    assert!(
        colors.iter().all(|c| *c == [0.1, 0.2, 0.3, 1.]),
        "{colors:?}"
    );
    let rules = &output.instances[0].hypotheses;
    assert!(
        rules.contains(&fo4_scene::hypotheses::EFFECT_SHADING),
        "{rules:?}"
    );
    assert!(!rules.contains(&fo4_scene::hypotheses::EFFECT_BLEND));
    // Vertex_Alpha alone with no vertex alpha below 1: settled, no channel.
    let output = effect_surface_build(1 << 3, 0.5);
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    // Base colour x scale above 1: drawn, clamped under its rule.
    let output = effect_surface_build(0, 2.);
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    let rules = &output.instances[0].hypotheses;
    assert!(rules.contains(&fo4_scene::hypotheses::EFFECT_COLOUR_CLAMPED));
    let output = effect_surface_build(0, 0.5);
    let rules = &output.instances[0].hypotheses;
    assert!(!rules.contains(&fo4_scene::hypotheses::EFFECT_COLOUR_CLAMPED));
    // An environment map: drawn without its term under its two rules,
    // otherwise as the same surface without the flag.
    let plain = effect_surface_build(0, 0.5);
    let output = effect_surface_build(1 << 7, 0.5);
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    let rules = &output.instances[0].hypotheses;
    for rule in [
        fo4_scene::hypotheses::EFFECT_ENVIRONMENT_ADDS,
        fo4_scene::hypotheses::EFFECT_ENVIRONMENT_LEFT_OUT,
    ] {
        assert!(rules.contains(&rule), "{rule} {rules:?}");
        assert!(!plain.instances[0].hypotheses.contains(&rule));
    }
    let (a, b) = (&output.list.draws[0], &plain.list.draws[0]);
    assert_eq!(a.alpha.opacity, b.alpha.opacity);
    assert_eq!(
        output.list.meshes[a.mesh].vertex_colors,
        plain.list.meshes[b.mesh].vertex_colors
    );
    assert_eq!(
        output.list.textures[a.texture].rgba,
        plain.list.textures[b.texture].rgba
    );
    // Each unsettled term is named, together.
    for (flags, scale, found) in [
        (0, -1., "colour-negative"),
        (1 << 6, 0.5, "falloff-normals"),
        (1 << 8, 0.5, "rgb-falloff"),
        (1 << 4, 0.5, "palette-texture"),
        (1 << 7 | 1 << 5, 2., "palette-texture"),
        (1 << 7 | 1 << 16, 2., "fire-refraction"),
        (1 << 15 | 1 << 16, 0.5, "refraction+fire-refraction"),
    ] {
        let output = effect_surface_build(flags, scale);
        assert!(output.list.draws.is_empty());
        let cause = &output.omitted[0].causes[0];
        assert_eq!(cause.code, "UnsupportedEffectSurface");
        assert_eq!(cause.found, found);
    }
}

/// Falloff and vertex alpha under Vertex_Colors alone: drawn under their
/// pending rules, which the instance records; every term they do not
/// settle still refuses the surface by name.
#[test]
fn effect_falloff_and_vertex_alpha_are_drawn_only_under_their_rules() {
    use fo4_scene::hypotheses as h;
    // Normals along +x (normbytes 255, 128, 128), colours white; the third
    // vertex's alpha is 0.
    let white = |a: u8| [255, 128, 128, 255, 255, 255, 255, a];
    let painted = Some([white(255), white(255), white(0)]);
    let (cos10, cos80) = (10f32.to_radians().cos(), 80f32.to_radians().cos());
    let falloff = EffectFixture {
        flags1: 1 << 6,
        flags2: 1 << 5,
        scale: 1.,
        falloff: [cos10, cos80, 1., 0.],
        extra: painted,
        ..EffectFixture::default()
    };
    let output = effect_surface_build_with(EffectFixture { ..falloff });
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    let draw = &output.list.draws[0];
    assert_eq!(
        draw.falloff,
        Some(crate::Falloff {
            start_angle: cos10,
            stop_angle: cos80,
            start_opacity: 1.,
            stop_opacity: 0.,
        })
    );
    let mesh = &output.list.meshes[draw.mesh];
    let normals = mesh.normals.as_ref().unwrap();
    assert!(normals
        .iter()
        .all(|n| n[0] == 1. && n[1].abs() < 0.01 && n[2].abs() < 0.01));
    // Vertex alpha read under Vertex_Colors alone, times the base colour.
    let colors = mesh.vertex_colors.as_ref().unwrap();
    assert_eq!(colors[2][3], 0.);
    assert_eq!(colors[0][3], 1.);
    let rules = &output.instances[0].hypotheses;
    for rule in [
        h::EFFECT_FALLOFF,
        h::EFFECT_FALLOFF_ABSOLUTE,
        h::EFFECT_FALLOFF_LINEAR,
        h::EFFECT_FALLOFF_ALPHA,
        h::EFFECT_VERTEX_ALPHA_COLORS,
    ] {
        assert!(rules.contains(&rule), "{rule} {rules:?}");
    }
    // Vertex alpha 1 everywhere: the readings agree, no vertex alpha rule,
    // no alpha channel; no falloff flag: no falloff and no normals.
    let output = effect_surface_build_with(EffectFixture {
        flags1: 0,
        extra: Some([white(255); 3]),
        ..falloff
    });
    let draw = &output.list.draws[0];
    assert_eq!(draw.falloff, None);
    assert!(output.list.meshes[draw.mesh].normals.is_none());
    let rules = &output.instances[0].hypotheses;
    assert!(!rules.contains(&h::EFFECT_VERTEX_ALPHA_COLORS));
    assert!(!rules.contains(&h::EFFECT_FALLOFF));
    // Equal angles with equal opacities: the factor is that opacity.
    let output = effect_surface_build_with(EffectFixture {
        falloff: [1., 1., 0., 0.],
        ..falloff
    });
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    // What stays refused, by name.
    for (fixture, found) in [
        (
            EffectFixture {
                falloff: [1., 1., 1., 0.],
                ..falloff
            },
            "falloff-fields",
        ),
        (
            EffectFixture {
                falloff: [cos10, cos80, 1.5, 0.],
                ..falloff
            },
            "falloff-fields",
        ),
        (
            EffectFixture {
                extra: None,
                flags2: 0,
                ..falloff
            },
            "falloff-normals",
        ),
        (
            EffectFixture {
                flags1: 1 << 6 | 1 << 8,
                ..falloff
            },
            "rgb-falloff",
        ),
        (
            EffectFixture {
                flags1: 1 << 3,
                flags2: 0,
                ..falloff
            },
            "vertex-alpha-flag-alone",
        ),
    ] {
        let output = effect_surface_build_with(fixture);
        assert!(output.list.draws.is_empty(), "{found}");
        let cause = &output.omitted[0].causes[0];
        assert_eq!(cause.code, "UnsupportedEffectSurface");
        assert_eq!(cause.found, found);
    }
}

/// A lighting shader naming a material file that is absent: drawn with its
/// own texture set only under `nif_missing_material_embedded_shader_v1`,
/// and only when the texture set has a texture.
#[test]
fn a_missing_material_falls_back_to_the_shaders_own_texture_only_under_its_rule() {
    use fo4_scene::hypotheses::{MISSING_MATERIAL_EMBEDDED, UV_SCALE_THEN_OFFSET};
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    let part = nif_named(
        b"textures/a.dds",
        None,
        b"EditorMarker:27",
        b"materials/absent.bgsm",
    );
    let (_temp, m) = scol_fixture_with("render-missing-material", &dds(), part);
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    let i = &scene.instances[0];
    assert!(i
        .blockers
        .iter()
        .any(|e| e.code == "MissingMaterialEmbeddedShader"));
    assert!(!i.blockers.iter().any(|e| e.code == "MissingMaterial"));
    assert!(i.state.hypotheses().contains(&MISSING_MATERIAL_EMBEDDED));
    let out = build(
        &scene,
        &loader,
        &m.limits,
        options(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert_eq!(out.list.draws.len(), 2);
    assert_eq!(out.list.textures.len(), 1);
    // The fixture's shader stores UV scale 0: the draw relies on the order rule.
    assert!(out.instances[0].hypotheses.contains(&UV_SCALE_THEN_OFFSET));
    let facts = build(&scene, &loader, &m.limits, options(RuleLevel::Facts), &b).unwrap();
    assert!(facts.list.draws.is_empty());

    // Without a texture of its own the material is still needed.
    let part = nif_named(b"", None, b"EditorMarker:27", b"materials/absent.bgsm");
    let (_temp, m) = scol_fixture_with("render-missing-bare", &dds(), part);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    let i = &scene.instances[0];
    assert!(i.blockers.iter().any(|e| e.code == "MissingMaterial"));
    assert!(matches!(i.state, fo4_scene::State::Blocked));
}

/// A surface whose UV scale is not 1 (the fixture's shader stores 0) is
/// drawn only under `uv_scale_then_offset_v1`: refused at the measured
/// level, drawn under hypotheses.
#[test]
fn a_surface_with_uv_fields_other_than_identity_needs_the_uv_order_rule() {
    use fo4_scene::hypotheses::UV_SCALE_THEN_OFFSET;
    let mut part = nif_with_helper(b"textures/a.dds", None, b"PlainLabel");
    // Rename the root node so that no editor-marker rule applies.
    let at = part.windows(12).position(|w| w == b"EditorMarker").unwrap();
    part[at + 11] = b'X';
    let (_temp, m) = fixture_placing("render-uv-rule", &dds(), part, 0x801);
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    assert!(
        scene.instances[0].state.ready_at(RuleLevel::Measured),
        "{:?}",
        scene.instances[0].blockers
    );
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    let measured = build(&scene, &loader, &m.limits, options(RuleLevel::Measured), &b).unwrap();
    assert!(measured.list.draws.is_empty());
    assert_eq!(measured.omitted[0].causes[0].code, "UnsupportedUvTransform");
    let under = build(
        &scene,
        &loader,
        &m.limits,
        options(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(under.omitted.is_empty(), "{:?}", under.omitted);
    assert_eq!(under.instances[0].hypotheses, [UV_SCALE_THEN_OFFSET]);
}

/// A greyscale-to-palette surface: drawn only under the palette rules,
/// which the instance records by variant, with the palette decoded; an
/// absent palette texture refuses it by name.
#[test]
fn palette_surfaces_are_drawn_only_under_their_rules() {
    use fo4_scene::hypotheses as h;
    const SOURCE: &[[u8; 4]] = &[[10, 64, 200, 191]];
    const PALETTE: &[[u8; 4]] = &[[0, 0, 0, 0], [255, 128, 0, 255]];
    let fixture = EffectFixture {
        flags1: 1 << 4,
        scale: 1.,
        source: Some(SOURCE),
        greyscale: Some(PALETTE),
        ..EffectFixture::default()
    };
    let output = effect_surface_build_with(fixture);
    assert_eq!(output.list.draws.len(), 1, "{:?}", output.omitted);
    let draw = &output.list.draws[0];
    let palette = draw.palette.unwrap();
    assert!(palette.colour && !palette.alpha);
    let t = &output.list.textures[palette.texture];
    assert_eq!((t.width, t.height), (2, 1));
    assert_eq!(&t.rgba[4..], [1., 128. / 255., 0., 1.]);
    assert_ne!(palette.texture, draw.texture);
    let rules = &output.instances[0].hypotheses;
    for rule in [
        h::EFFECT_PALETTE_SAMPLED,
        h::EFFECT_PALETTE_V,
        h::EFFECT_PALETTE_COLOUR_U,
        h::EFFECT_PALETTE_COLOUR_RGB,
    ] {
        assert!(rules.contains(&rule), "{rule} {rules:?}");
    }
    assert!(!rules.contains(&h::EFFECT_PALETTE_ALPHA_U));
    // The alpha variant records its own pair.
    let output = effect_surface_build_with(EffectFixture {
        flags1: 1 << 5,
        ..fixture
    });
    let palette = output.list.draws[0].palette.unwrap();
    assert!(!palette.colour && palette.alpha);
    let rules = &output.instances[0].hypotheses;
    assert!(rules.contains(&h::EFFECT_PALETTE_ALPHA_U));
    assert!(rules.contains(&h::EFFECT_PALETTE_ALPHA_REPLACES));
    assert!(!rules.contains(&h::EFFECT_PALETTE_COLOUR_U));
    // Without the hypotheses level the palette rules are not used.
    let output = effect_surface_build_with(EffectFixture {
        level: RuleLevel::Measured,
        ..fixture
    });
    assert!(output.list.draws.is_empty());
    // Named but absent: a missing texture, as for any texture.
    let output = effect_surface_build_with(EffectFixture {
        greyscale: None,
        ..fixture
    });
    assert_eq!(
        output.omitted[0].causes[0].found, "palette-texture",
        "{:?}",
        output.omitted
    );
}

/// A swap over a model whose only shader names no material: refused as
/// `RenderSwapWithoutMaterial` before, now drawn with the shader's own
/// texture set under `mswp_shape_without_material_untouched_v1`, the swap's
/// entries still read.
#[test]
fn a_swap_leaves_a_shape_without_material_to_its_own_texture_under_its_rule() {
    use fo4_scene::hypotheses::SWAP_WITHOUT_MATERIAL;
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    let run = |swap: &[u8]| {
        let part = nif_with_helper(b"textures/a.dds", None, b"PlainLabel");
        let records = rec(b"MSWP", 0x820, 0, swap);
        let (temp, m) = fixture_records(
            "render-swap-no-material",
            &dds(),
            part,
            0x801,
            &sub(b"MODS", &0x820u32.to_le_bytes()),
            &records,
        );
        let b = Budget::new(32 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
        let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
        assert!(scene.instances[0]
            .base
            .as_ref()
            .unwrap()
            .material_swap
            .is_some());
        let out = build(
            &scene,
            &loader,
            &m.limits,
            options(RuleLevel::Hypotheses),
            &b,
        )
        .unwrap();
        drop(temp);
        out
    };
    let mut swap = sub(b"BNAM", b"other.bgsm\0");
    swap.extend(sub(b"SNAM", b"replacement.bgsm\0"));
    let out = run(&swap);
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(!out.list.draws.is_empty());
    assert!(out.instances[0].hypotheses.contains(&SWAP_WITHOUT_MATERIAL));
    // Empty entries (a blank row, an empty target) change nothing under
    // `mswp_empty_entry_inert_v1`.
    let mut blank = swap.clone();
    blank.extend(sub(b"BNAM", b"\0"));
    blank.extend(sub(b"SNAM", b"\0"));
    blank.extend(sub(b"BNAM", b"other2.bgsm\0"));
    blank.extend(sub(b"SNAM", b"\0"));
    let out = run(&blank);
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(out.instances[0]
        .hypotheses
        .contains(&fo4_scene::hypotheses::SWAP_EMPTY_ENTRY));
    // An unpaired entry is still refused (by the scene or the swap reader).
    let out = run(&sub(b"BNAM", b"other.bgsm\0"));
    assert!(out.list.draws.is_empty());
    assert_eq!(out.omitted.len(), 1);
}

/// A blended lighting surface whose shape sets Vertex_Colors and stores a
/// vertex alpha below 1 is drawn with its vertex alpha (alpha channel only)
/// under `lighting_vertex_alpha_blend_v1`, and without it as before.
#[test]
fn a_blended_lighting_surface_takes_its_vertex_alpha_only_under_its_rule() {
    use fo4_scene::hypotheses::LIGHTING_VERTEX_ALPHA;
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    // Normal bytes, then RGBA: a vertex alpha of 0, 128 and 255.
    let colours = |a: [u8; 3]| a.map(|a| [128, 128, 255, 0, 200, 100, 50, a]);
    let run = |flags2: u32, alpha_flags: u16, a: [u8; 3]| {
        let part = nif_effect_textures(
            b"textures/a.dds",
            None,
            b"PlainLabel",
            b"",
            Some(colours(a)),
            (b"", b""),
            Some((flags2, alpha_flags)),
        );
        let (temp, m) = fixture_placing("render-lighting-vertex-alpha", &dds(), part, 0x801);
        let b = Budget::new(32 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
        let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
        let out = build(
            &scene,
            &loader,
            &m.limits,
            options(RuleLevel::Hypotheses),
            &b,
        )
        .unwrap();
        drop(temp);
        out
    };
    // Blending on (flags bit 0, SRC_ALPHA / INV_SRC_ALPHA), Vertex_Colors set.
    let blend = 1 | (6 << 1) | (7 << 5);
    let out = run(1 << 5, blend, [0, 128, 255]);
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(out.instances[0].hypotheses.contains(&LIGHTING_VERTEX_ALPHA));
    let colours = out.list.meshes[out.list.draws[0].mesh]
        .vertex_colors
        .clone()
        .unwrap();
    // The alpha only: RGB stays 1.
    assert_eq!(colours[0], [1., 1., 1., 0.]);
    assert_eq!(colours[2], [1., 1., 1., 1.]);
    // Without blending, without Vertex_Colors, or with every vertex alpha
    // 1, nothing changes and the rule is not used.
    for (flags2, alpha_flags, a) in [
        (1 << 5, 0, [0, 128, 255]),
        (0, blend, [0, 128, 255]),
        (1 << 5, blend, [255; 3]),
    ] {
        let out = run(flags2, alpha_flags, a);
        assert!(out.omitted.is_empty(), "{:?}", out.omitted);
        assert!(!out.instances[0].hypotheses.contains(&LIGHTING_VERTEX_ALPHA));
        assert!(out.list.meshes.iter().all(|m| m.vertex_colors.is_none()));
    }
}

/// A swap source holding `*` matches a material the model names only under
/// `mswp_wildcard_pattern_v1`; a target no archive holds changes nothing
/// under `mswp_wildcard_missing_target_inert_v1` (the scene raises
/// `MissingSwapPatternTarget`, and only where the pattern rule may be
/// relied on); a literal source of the same swap wins under
/// `mswp_wildcard_literal_wins_v1`.
#[test]
fn swap_patterns_match_names_only_under_their_rules() {
    use fo4_scene::hypotheses::{
        SWAP_EMPTY_ENTRY, SWAP_PATTERN, SWAP_PATTERN_LITERAL, SWAP_PATTERN_MISSING,
    };
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    let run = |swap: &[u8], policy: RuleLevel| {
        let part = nif_named(
            b"textures/a.dds",
            None,
            b"PlainLabel",
            b"materials/a/rock01.bgsm",
        );
        let records = rec(b"MSWP", 0x820, 0, swap);
        let (temp, m) = fixture_records(
            "render-swap-pattern",
            &dds(),
            part,
            0x801,
            &sub(b"MODS", &0x820u32.to_le_bytes()),
            &records,
        );
        let b = Budget::new(32 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", policy, &b);
        let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
        let codes: Vec<String> = scene.instances[0]
            .blockers
            .iter()
            .map(|e| e.code.clone())
            .collect();
        let state = scene.instances[0].state.hypotheses().to_vec();
        let out = build(&scene, &loader, &m.limits, options(policy), &b).unwrap();
        drop(temp);
        (codes, state, out)
    };
    let mut pattern = sub(b"BNAM", b"a\\Rock*.bgsm\0");
    pattern.extend(sub(b"SNAM", b"a\\cliff*.bgsm\0"));
    // Under hypotheses: the pattern matches, its target (`cliff01`) is
    // absent, and the shape keeps the material it names.
    let (codes, state, out) = run(&pattern, RuleLevel::Hypotheses);
    assert!(
        codes.iter().any(|c| c == "MissingSwapPatternTarget"),
        "{codes:?}"
    );
    assert!(state.contains(&SWAP_PATTERN_MISSING));
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(out.instances[0].hypotheses.contains(&SWAP_PATTERN));
    // At the facts level the scene loads no pattern target and the
    // pattern is never read.
    let (codes, state, _) = run(&pattern, RuleLevel::Facts);
    assert!(!codes.iter().any(|c| c == "MissingSwapPatternTarget"));
    assert!(!state.contains(&SWAP_PATTERN_MISSING));
    // A pattern that matches nothing is not relied on.
    let mut other = sub(b"BNAM", b"a\\stone*.bgsm\0");
    other.extend(sub(b"SNAM", b"a\\cliff*.bgsm\0"));
    let (codes, _, out) = run(&other, RuleLevel::Hypotheses);
    assert!(!codes.iter().any(|c| c == "MissingSwapPatternTarget"));
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(!out.instances[0].hypotheses.contains(&SWAP_PATTERN));
    // A literal source of the same swap (here with an empty target, which
    // leaves the material as named) wins over the pattern.
    let mut both = sub(b"BNAM", b"a\\rock01.bgsm\0");
    both.extend(sub(b"SNAM", b"\0"));
    both.extend(pattern.clone());
    let (codes, _, out) = run(&both, RuleLevel::Hypotheses);
    assert!(!codes.iter().any(|c| c == "MissingSwapPatternTarget"));
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    let rules = &out.instances[0].hypotheses;
    assert!(rules.contains(&SWAP_PATTERN_LITERAL), "{rules:?}");
    assert!(rules.contains(&SWAP_EMPTY_ENTRY));
    // A target with another count of `*` is refused by the scene.
    let mut odd = sub(b"BNAM", b"a\\rock*.bgsm\0");
    odd.extend(sub(b"SNAM", b"a\\*cliff*.bgsm\0"));
    let (codes, _, out) = run(&odd, RuleLevel::Hypotheses);
    assert!(
        codes.iter().any(|c| c == "UnsupportedSwapPattern"),
        "{codes:?}"
    );
    assert!(out.list.draws.is_empty());
}

/// A shape whose vertex holds a texture coordinate that is not finite:
/// refused as before without `nif_uv_nonfinite_triangles_left_out_v1`,
/// drawn without the triangles that use the vertex under it.
#[test]
fn triangles_on_a_non_finite_texture_coordinate_are_left_out_only_under_their_rule() {
    use fo4_scene::hypotheses::UV_NONFINITE;
    let options = |level| BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    let mut part = nif_with_helper(b"textures/a.dds", None, b"PlainLabel");
    // Vertex 0's position (1, 0, 0) then its first UV half: a NaN.
    let first: Vec<u8> = [0x3c00u16, 0, 0, 0, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut at = 0;
    let mut patched = 0;
    while let Some(k) = part[at..]
        .windows(first.len())
        .position(|w| w == first.as_slice())
    {
        let k = at + k;
        part[k + 8..k + 10].copy_from_slice(&0x7e00u16.to_le_bytes());
        patched += 1;
        at = k + first.len();
    }
    assert!(patched > 0);
    let (_temp, m) = fixture_placing("render-uv-nan", &dds(), part, 0x801);
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    let out = build(
        &scene,
        &loader,
        &m.limits,
        options(RuleLevel::Hypotheses),
        &b,
    )
    .unwrap();
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(out.instances[0].hypotheses.contains(&UV_NONFINITE));
    let mesh = &out.list.meshes[out.list.draws[0].mesh];
    assert!(mesh.triangles.is_empty());
    assert!(mesh
        .vertices
        .iter()
        .all(|v| v.uv.iter().all(|x| x.is_finite())));
    let measured = build(&scene, &loader, &m.limits, options(RuleLevel::Measured), &b).unwrap();
    // Pending rules are not used at the measured level (the fixture's UV
    // scale 0 is refused there too), and the mesh is never built.
    assert!(measured.list.draws.is_empty());
    assert!(measured.list.meshes.is_empty());
}

/// A free-running controller block of an effect shader (block 1): next
/// controller `next`, flags `flags` (Active, cycle in bits 1 and 2),
/// frequency 1, phase 0, start 0, stop 1, interpolator `interpolator`,
/// controlled variable `variable`.
fn effect_controller(next: i32, flags: u16, interpolator: i32, variable: u32) -> Vec<u8> {
    let mut c = next.to_le_bytes().to_vec();
    c.extend(flags.to_le_bytes());
    for v in [1f32, 0., 0., 1.] {
        c.extend(v.to_le_bytes());
    }
    for n in [1i32, interpolator] {
        c.extend(n.to_le_bytes());
    }
    c.extend(variable.to_le_bytes());
    c
}
/// An NiFloatInterpolator without a pose value and its NiFloatData of
/// linear keys.
fn float_keys(data: i32, keys: &[(f32, f32)]) -> [Vec<u8>; 2] {
    let mut ip = (-3.402_823_4e38f32).to_le_bytes().to_vec();
    ip.extend(data.to_le_bytes());
    let mut d = (keys.len() as u32).to_le_bytes().to_vec();
    d.extend(1u32.to_le_bytes());
    for (t, v) in keys {
        d.extend(t.to_le_bytes());
        d.extend(v.to_le_bytes());
    }
    [ip, d]
}

/// A textured effect model whose shader's V offset (stored 0) and alpha
/// (stored 0.8) two free-running looping controllers drive with linear
/// keys away from the stored values (0.25 to 1.25, and 0.2 to 0.6, over one
/// second): the scene tags them for the clock, the build omits the model
/// without a time (`UnsettledControllerTime`) and, with one, draws the
/// values at that time under the clock rules and keeps the tracks for any
/// other time; below the hypotheses level nothing is drawn.
#[test]
fn free_running_controllers_are_drawn_at_the_build_time_only_under_the_clock_rules() {
    use fo4_scene::hypotheses as h;
    let temp = TestDir::new("effect-clock");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(
        root.join("textures/src.dds"),
        dds_rgba(&[[255, 0, 0, 255], [0, 255, 0, 255]]),
    )
    .unwrap();
    let mut blocks = model_blocks(b"", Some(0), None, (b"textures/src.dds", b""), None);
    // The shader's Controller link (after Name and Num Extra Data).
    blocks[1].1[8..12].copy_from_slice(&5i32.to_le_bytes());
    let loop_active = 8u16;
    blocks.push((
        "BSEffectShaderPropertyFloatController",
        effect_controller(8, loop_active, 6, 8),
    ));
    let [ip, data] = float_keys(7, &[(0., 0.25), (1., 1.25)]);
    blocks.push(("NiFloatInterpolator", ip));
    blocks.push(("NiFloatData", data));
    blocks.push((
        "BSEffectShaderPropertyFloatController",
        effect_controller(-1, loop_active, 9, 5),
    ));
    let [ip, data] = float_keys(10, &[(0., 0.2), (1., 0.6)]);
    blocks.push(("NiFloatInterpolator", ip));
    blocks.push(("NiFloatData", data));
    let bytes = nif_file(blocks, b"EditorMarker:27", b"");
    std::fs::write(root.join("meshes/effect.nif"), bytes).unwrap();
    let mut reference = sub(b"NAME", &0x801u32.to_le_bytes());
    reference.extend(sub(b"DATA", &[0; 24]));
    reference.extend(sub(b"XSCL", &1f32.to_le_bytes()));
    let mut cell = sub(b"EDID", b"ClockCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [
        rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)),
        rec(b"STAT", 0x801, 0, &sub(b"MODL", b"effect.nif\0")),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(
                    6,
                    0x800,
                    &group(9, 0x800, &rec(b"REFR", 0x802, 0, &reference)),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
            files: vec!["meshes/effect.nif".into(), "textures/src.dds".into()],
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    };
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let run = |level: RuleLevel, time: Option<f64>| {
        let mut loader = Loader::new(&store, &vfs, "english", level, &b);
        let scene = loader.by_editor_id("ClockCell", &b).unwrap();
        let options = BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1024,
            level,
            blocks: None,
            time,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        };
        let state = scene.instances[0].state.hypotheses().to_vec();
        (
            state,
            build(&scene, &loader, &m.limits, options, &b).unwrap(),
        )
    };
    let (state, out) = run(RuleLevel::Hypotheses, None);
    for rule in [
        h::CLOCK_FREE_RUNNING,
        h::CLOCK_CYCLE_LOOP,
        h::CLOCK_KEY_LINEAR,
    ] {
        assert!(state.contains(&rule), "{rule} {state:?}");
    }
    assert!(out.list.draws.is_empty());
    assert_eq!(out.omitted[0].causes[0].code, "UnsettledControllerTime");
    let (_, out) = run(RuleLevel::Hypotheses, Some(0.5));
    assert_eq!(out.list.draws.len(), 1, "{:?}", out.omitted);
    let d = &out.list.draws[0];
    assert_eq!(d.uv_offset, [0., 0.75]);
    assert!((d.alpha.opacity - 0.4).abs() < 1e-6, "{}", d.alpha.opacity);
    let rules = &out.instances[0].hypotheses;
    for rule in [
        h::UV_SCALE_THEN_OFFSET,
        h::EFFECT_SHADING,
        h::CLOCK_FREE_RUNNING,
    ] {
        assert!(rules.contains(&rule), "{rule} {rules:?}");
    }
    assert_eq!(out.list.animations.len(), 1);
    let mut list = out.list;
    // A quarter second in, a whole loop later, and the load.
    for (t, offset, opacity) in [(0.25, 0.5, 0.3), (1.25, 0.5, 0.3), (0., 0.25, 0.2)] {
        list.at_time(t).unwrap();
        let d = &list.draws[0];
        assert!(
            (d.uv_offset[1] - offset).abs() < 1e-6,
            "{t} {:?}",
            d.uv_offset
        );
        assert!((d.alpha.opacity - opacity).abs() < 1e-6, "{t}");
    }
    // Without the hypotheses level nothing with a controller is drawn,
    // with or without a time.
    for time in [None, Some(0.5)] {
        let (_, out) = run(RuleLevel::Measured, time);
        assert!(out.list.draws.is_empty());
        assert!(out.list.animations.is_empty());
    }
    drop(temp);
}

/// An NiPoint3Interpolator without a pose value and its NiPosData of
/// linear keys.
fn point_keys(data: i32, keys: &[(f32, [f32; 3])]) -> [Vec<u8>; 2] {
    let mut ip = Vec::new();
    for _ in 0..3 {
        ip.extend((-3.402_823_4e38f32).to_le_bytes());
    }
    ip.extend(data.to_le_bytes());
    let mut d = (keys.len() as u32).to_le_bytes().to_vec();
    d.extend(1u32.to_le_bytes());
    for (t, v) in keys {
        d.extend(t.to_le_bytes());
        for x in v {
            d.extend(x.to_le_bytes());
        }
    }
    [ip, d]
}

/// A textured effect model (base colour 0.2, 0.4, 0.6, scale 2) whose
/// emissive multiple a free-running looping controller drives from 1 to 3
/// (stored 2, not its first key) and whose emissive colour another drives
/// from the stored RGB to 1, 0, 0 over one second: with the clock's
/// switches off the scene leaves the model refused (`effect-emissive`);
/// with them on it is tagged for the clock and drawn at the build time with
/// its colour (`Draw::colour`, base colour times multiple) and the tracks
/// for any other time, under the field rules; without a time it is omitted.
#[test]
fn effect_colour_and_multiple_follow_the_clock_behind_their_switches() {
    use fo4_scene::{clock::Switches, hypotheses as h};
    let temp = TestDir::new("effect-clock-colour");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(
        root.join("textures/src.dds"),
        dds_rgba(&[[255, 255, 255, 255], [128, 128, 128, 255]]),
    )
    .unwrap();
    let mut blocks = model_blocks(b"", Some(0), None, (b"textures/src.dds", b""), None);
    blocks[1].1[8..12].copy_from_slice(&5i32.to_le_bytes());
    let loop_active = 8u16;
    blocks.push((
        "BSEffectShaderPropertyFloatController",
        effect_controller(8, loop_active, 6, 0),
    ));
    let [ip, data] = float_keys(7, &[(0., 1.), (1., 3.)]);
    blocks.push(("NiFloatInterpolator", ip));
    blocks.push(("NiFloatData", data));
    blocks.push((
        "BSEffectShaderPropertyColorController",
        effect_controller(-1, loop_active, 9, 0),
    ));
    let [ip, data] = point_keys(10, &[(0., [0.2, 0.4, 0.6]), (1., [1., 0., 0.])]);
    blocks.push(("NiPoint3Interpolator", ip));
    blocks.push(("NiPosData", data));
    let bytes = nif_file(blocks, b"EditorMarker:27", b"");
    std::fs::write(root.join("meshes/effect.nif"), bytes).unwrap();
    let mut reference = sub(b"NAME", &0x801u32.to_le_bytes());
    reference.extend(sub(b"DATA", &[0; 24]));
    reference.extend(sub(b"XSCL", &1f32.to_le_bytes()));
    let mut cell = sub(b"EDID", b"ClockCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [
        rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)),
        rec(b"STAT", 0x801, 0, &sub(b"MODL", b"effect.nif\0")),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(
                    6,
                    0x800,
                    &group(9, 0x800, &rec(b"REFR", 0x802, 0, &reference)),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
            files: vec!["meshes/effect.nif".into(), "textures/src.dds".into()],
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    };
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let on = Switches {
        emissive_multiple: true,
        emissive_colour: true,
        ..Switches::OFF
    };
    let run = |level: RuleLevel, time: Option<f64>, clock: Switches| {
        let mut loader = Loader::new(&store, &vfs, "english", level, &b);
        loader.set_clock_switches(clock, &b);
        let scene = loader.by_editor_id("ClockCell", &b).unwrap();
        let options = BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1024,
            level,
            blocks: None,
            time,
            clock,
            vertex_normals: false,
            chains: false,
        };
        let state = (
            matches!(scene.instances[0].state, fo4_scene::State::Blocked),
            scene.instances[0].state.hypotheses().to_vec(),
        );
        (
            state,
            build(&scene, &loader, &m.limits, options, &b).unwrap(),
        )
    };
    // Switches off: the multiple's controller is not settled at rest.
    let (state, out) = run(RuleLevel::Hypotheses, Some(0.5), Switches::default());
    assert!(state.0, "{state:?}");
    assert!(out.list.draws.is_empty());
    // On: tagged for the clock, omitted without a time.
    let (state, out) = run(RuleLevel::Hypotheses, None, on);
    for rule in [
        h::CLOCK_FREE_RUNNING,
        h::CLOCK_EMISSIVE_MULTIPLE,
        h::CLOCK_KEY_LINEAR,
    ] {
        assert!(state.1.contains(&rule), "{rule} {state:?}");
    }
    assert_eq!(out.omitted[0].causes[0].code, "UnsettledControllerTime");
    let (_, out) = run(RuleLevel::Hypotheses, Some(0.5), on);
    assert_eq!(out.list.draws.len(), 1, "{:?}", out.omitted);
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
    // Colour (0.6, 0.2, 0.3) times multiple 2.
    let c = out.list.draws[0].colour.unwrap();
    assert!(close(c, [1.2, 0.4, 0.6]), "{c:?}");
    // The mesh's vertex colours hold no tint.
    let mesh = &out.list.meshes[out.list.draws[0].mesh];
    assert!(mesh
        .vertex_colors
        .as_ref()
        .unwrap()
        .iter()
        .all(|v| *v == [1.; 4]));
    let rules = &out.instances[0].hypotheses;
    for rule in [
        h::CLOCK_EMISSIVE_MULTIPLE,
        h::CLOCK_EMISSIVE_COLOUR,
        h::EFFECT_COLOUR_CLAMPED,
        h::CLOCK_FREE_RUNNING,
    ] {
        assert!(rules.contains(&rule), "{rule} {rules:?}");
    }
    let mut list = out.list;
    for (t, colour) in [(0., [0.2, 0.4, 0.6]), (0.25, [0.6, 0.45, 0.675])] {
        list.at_time(t).unwrap();
        let c = list.draws[0].colour.unwrap();
        assert!(close(c, colour), "{t} {c:?}");
    }
    // The CPU frame takes the colour: a draw at another time differs.
    // The shape lies in the plane x = 1, y and z from 0 to 0.5.
    let mut frustum = fo4_scene::camera::reference_frustum([32, 32]);
    frustum[4] = 0.1;
    let camera =
        fo4_scene::camera::look_at([-2., 0.15, 0.15], [1., 0.15, 0.15], frustum, [32, 32]).unwrap();
    let frame = |l: &crate::DrawList| {
        let f = crate::render(
            l,
            &camera,
            crate::RenderOptions::REFERENCE,
            &m.limits,
            &Budget::new(32 << 20),
        )
        .unwrap();
        assert!(f.instances.iter().any(|i| *i != u32::MAX));
        f.color
    };
    list.at_time(0.).unwrap();
    let a = frame(&list);
    list.at_time(0.5).unwrap();
    assert_ne!(a, frame(&list));
    // Below the hypotheses level nothing with a controller is drawn.
    let (_, out) = run(RuleLevel::Measured, Some(0.5), on);
    assert!(out.list.draws.is_empty());
    drop(temp);
}

/// A lighting model whose shape hangs under a node (block 5, stored at the
/// origin) that a free-running looping NiTransformController moves with
/// linear translation keys from the origin to (10, 0, 0) over one second.
/// With the clock's switches off the model keeps its `transform-varies`
/// tag and is refused; with the translation switch on the scene tags it for
/// the clock, the build omits it without a time and draws it at the time's
/// translation with one, keeping the chain for every other time; the
/// rotation switch alone does not cover a moving translation.
#[test]
fn a_moving_node_is_drawn_at_the_build_time_only_behind_its_switch() {
    use fo4_scene::{clock::Switches, hypotheses as h};
    let temp = TestDir::new("transform-clock");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(root.join("textures/a.dds"), dds()).unwrap();
    let mut blocks = model_blocks(b"textures/a.dds", None, None, (b"", b""), None);
    // The root (block 3, unnamed here) holds the moving node as its one
    // child, and is the file's only root.
    let r = &mut blocks[3].1;
    let n = r.len();
    r[n - 4..].copy_from_slice(&5u32.to_le_bytes());
    r[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut mover = Vec::new();
    for x in [u32::MAX, 0, 6, 0] {
        mover.extend(x.to_le_bytes());
    }
    for f in [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
        mover.extend(f.to_le_bytes());
    }
    for x in [u32::MAX, 1, 0] {
        mover.extend(x.to_le_bytes());
    }
    blocks.push(("NiNode", mover));
    let mut ctl = (-1i32).to_le_bytes().to_vec();
    ctl.extend(8u16.to_le_bytes());
    for v in [1f32, 0., 0., 1.] {
        ctl.extend(v.to_le_bytes());
    }
    for x in [5i32, 7] {
        ctl.extend(x.to_le_bytes());
    }
    blocks.push(("NiTransformController", ctl));
    let mut ip = Vec::new();
    for _ in 0..8 {
        ip.extend((-3.402_823_4e38f32).to_le_bytes());
    }
    ip.extend(8i32.to_le_bytes());
    blocks.push(("NiTransformInterpolator", ip));
    let mut data = 0u32.to_le_bytes().to_vec();
    for x in [2u32, 1] {
        data.extend(x.to_le_bytes());
    }
    for (t, x) in [(0f32, 0f32), (1., 10.)] {
        for v in [t, x, 0., 0.] {
            data.extend(v.to_le_bytes());
        }
    }
    data.extend(0u32.to_le_bytes());
    blocks.push(("NiTransformData", data));
    let mut bytes = nif_file(blocks, b"EditorMarker:27", b"");
    bytes.truncate(bytes.len() - 16);
    for x in [1u32, 3] {
        bytes.extend(x.to_le_bytes());
    }
    std::fs::write(root.join("meshes/mover.nif"), bytes).unwrap();
    let mut reference = sub(b"NAME", &0x801u32.to_le_bytes());
    reference.extend(sub(b"DATA", &[0; 24]));
    reference.extend(sub(b"XSCL", &1f32.to_le_bytes()));
    let mut cell = sub(b"EDID", b"MoverCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let plugin = [
        rec(b"TES4", 0, 1, &sub(b"HEDR", &hedr)),
        rec(b"STAT", 0x801, 0, &sub(b"MODL", b"mover.nif\0")),
        group(
            2,
            0,
            &[
                rec(b"CELL", 0x800, 0, &cell),
                group(
                    6,
                    0x800,
                    &group(9, 0x800, &rec(b"REFR", 0x802, 0, &reference)),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let m = Manifest {
        root,
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
            files: vec!["meshes/mover.nif".into(), "textures/a.dds".into()],
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: Some("synthetic".into()),
    };
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let run = |level: RuleLevel, time: Option<f64>, clock: Switches| {
        let mut loader = Loader::new(&store, &vfs, "english", level, &b);
        loader.set_clock_switches(clock, &b);
        let scene = loader.by_editor_id("MoverCell", &b).unwrap();
        let options = BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level,
            blocks: None,
            time,
            clock,
            vertex_normals: false,
            chains: false,
        };
        let state = scene.instances[0].state.hypotheses().to_vec();
        (
            state,
            build(&scene, &loader, &m.limits, options, &b).unwrap(),
        )
    };
    let translation = Switches {
        translation: true,
        ..Switches::OFF
    };
    // Switches off, or only the rotation switch: refused at every time.
    for clock in [
        Switches::OFF,
        Switches {
            rotation: true,
            rotation_xyz: true,
            scale: true,
            ..Switches::OFF
        },
    ] {
        let (state, out) = run(RuleLevel::Hypotheses, Some(0.25), clock);
        assert!(out.list.draws.is_empty(), "{clock:?}");
        assert!(!state.contains(&h::CLOCK_TRANSLATION));
        assert!(out.list.animations.is_empty());
    }
    let (state, out) = run(RuleLevel::Hypotheses, None, translation);
    for rule in [
        h::CLOCK_TRANSLATION,
        h::CLOCK_FREE_RUNNING,
        h::CLOCK_CYCLE_LOOP,
        h::CLOCK_KEY_LINEAR,
    ] {
        assert!(state.contains(&rule), "{rule} {state:?}");
    }
    assert!(out.list.draws.is_empty());
    assert_eq!(out.omitted[0].causes[0].code, "UnsettledControllerTime");
    let (_, out) = run(RuleLevel::Hypotheses, Some(0.25), translation);
    assert_eq!(out.list.draws.len(), 1, "{:?}", out.omitted);
    assert_eq!(out.list.draws[0].world.translation, [2.5, 0., 0.]);
    assert!(out.instances[0].hypotheses.contains(&h::CLOCK_TRANSLATION));
    assert_eq!(out.list.animations.len(), 1);
    let a = &out.list.animations[0];
    assert!(!a.has_tracks());
    assert_eq!(a.motion.as_ref().map(|m| m.steps.len()), Some(1));
    let mut list = out.list;
    // The same chain at other times, and a build at another time agrees.
    for (t, x) in [(0.75, 7.5), (1.25, 2.5), (0., 0.)] {
        list.at_time(t).unwrap();
        assert!(
            (list.draws[0].world.translation[0] - x).abs() < 1e-9,
            "{t} {:?}",
            list.draws[0].world
        );
    }
    list.at_time(0.75).unwrap();
    let (_, later) = run(RuleLevel::Hypotheses, Some(0.75), translation);
    assert_eq!(later.list.draws[0].world, list.draws[0].world);
    // Below the hypotheses level nothing with a controller is drawn.
    let (_, out) = run(RuleLevel::Measured, Some(0.25), translation);
    assert!(out.list.draws.is_empty());
    drop(temp);
}

/// A swap pattern that reaches a material another swap scope put in place:
/// the base swaps the model's material for a second one by a literal entry,
/// and the reference's swap holds a pattern that matches only that second
/// material. The scene forms the pattern's target from the base's target:
/// with no file for it, `MissingSwapPatternTarget` names it; with the file,
/// the scene loads it and the draw build (chained scopes, the pattern) draws
/// the shape without `RenderAssetNotInScene`.
#[test]
fn a_swap_pattern_reaches_the_material_another_scope_put_in_place() {
    use fo4_scene::hypotheses::{SWAP_CHAIN, SWAP_PATTERN};
    let bgsm = material_fixture::fixture(b"BGSM", 2, false, b"a.dds").v;
    let run = |with_target: bool| {
        let part = nif_named(
            b"textures/a.dds",
            None,
            b"PlainLabel",
            b"materials/a/rock01.bgsm",
        );
        let mut base_swap = sub(b"BNAM", b"a\\rock01.bgsm\0");
        base_swap.extend(sub(b"SNAM", b"a\\stone01.bgsm\0"));
        let mut reference_swap = sub(b"BNAM", b"a\\stone*.bgsm\0");
        reference_swap.extend(sub(b"SNAM", b"a\\cliff*.bgsm\0"));
        let records = [
            rec(b"MSWP", 0x820, 0, &base_swap),
            rec(b"MSWP", 0x821, 0, &reference_swap),
        ]
        .concat();
        let mut files = vec![("materials/a/stone01.bgsm", bgsm.clone())];
        if with_target {
            files.push(("materials/a/cliff01.bgsm", bgsm.clone()));
        }
        let (temp, m) = fixture_files(
            "render-swap-pattern-chain",
            &dds(),
            part,
            0x801,
            (
                &sub(b"MODS", &0x820u32.to_le_bytes()),
                &sub(b"XMSP", &0x821u32.to_le_bytes()),
            ),
            &records,
            &files,
        );
        let b = Budget::new(32 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
        let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
        let blockers: Vec<(String, String)> = scene.instances[0]
            .blockers
            .iter()
            .map(|e| (e.code.clone(), e.found.clone()))
            .collect();
        let loaded = loader
            .assets
            .nodes
            .iter()
            .any(|n| n.path == b"materials/a/cliff01.bgsm");
        let options = BuildOptions {
            max_texture_dimension: 16,
            max_texture_bytes: 1 << 20,
            level: RuleLevel::Hypotheses,
            blocks: None,
            time: None,
            clock: Default::default(),
            vertex_normals: false,
            chains: false,
        };
        let out = build(&scene, &loader, &m.limits, options, &b).unwrap();
        drop(temp);
        (blockers, loaded, out)
    };
    let cliff = fo4_core::HexBytes(b"materials/a/cliff01.bgsm").to_string();
    let (blockers, _, _) = run(false);
    assert!(
        blockers
            .iter()
            .any(|(code, found)| code == "MissingSwapPatternTarget" && *found == cliff),
        "{blockers:?}"
    );
    let (blockers, loaded, out) = run(true);
    assert!(
        !blockers
            .iter()
            .any(|(c, _)| c == "MissingSwapPatternTarget"),
        "{blockers:?}"
    );
    assert!(loaded);
    assert!(
        out.omitted
            .iter()
            .flat_map(|o| &o.causes)
            .all(|c| c.code != "RenderAssetNotInScene"),
        "{:?}",
        out.omitted
    );
    assert!(out.omitted.is_empty(), "{:?}", out.omitted);
    assert!(!out.list.draws.is_empty());
    let rules = &out.instances[0].hypotheses;
    assert!(rules.contains(&SWAP_PATTERN), "{rules:?}");
    assert!(rules.contains(&SWAP_CHAIN), "{rules:?}");
}

/// A run timeline's frame on a loaded cell (`fo4_scene::timeline`): the
/// placed reference is drawn as stored before the run changes it; a frame
/// after the run disabled it leaves it out (`vm_disable_hides_reference_v1`)
/// and needs the pending level; a frame after the run moved it draws it at
/// the new place with its stored rotation (`timeline_move_stored_rotation_v1`);
/// a move to another space or to an unknown place leaves it out.
#[test]
fn a_timeline_frame_leaves_out_what_the_run_disabled_and_moves_what_it_moved() {
    use fo4_scene::timeline::{self as tl, Event, Spot, Timeline, What};
    let mut part = nif_with_helper(b"textures/a.dds", None, b"PlainLabel");
    let at = part.windows(12).position(|w| w == b"EditorMarker").unwrap();
    part[at + 11] = b'X';
    let (_temp, m) = fixture_placing("render-timeline", &dds(), part, 0x801);
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let key = |id: u32| fo4_plugin::FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let event = |t: f64, what: What| Event { t, what };
    let timeline = |moved: Spot| Timeline {
        events: vec![
            event(
                1.0,
                What::Move {
                    reference: key(0x803),
                    at: moved,
                    source: "world_moveto_target_v1".into(),
                },
            ),
            event(
                2.0,
                What::Visibility {
                    reference: key(0x803),
                    shown: false,
                    nowait: false,
                },
            ),
        ],
        ..Timeline::default()
    };
    let here = Spot::At {
        space: key(0x800),
        pos: [50., 60., 70.],
    };
    let options = BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level: RuleLevel::Hypotheses,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    // The draws of the frame at second `s`, or the error applying it.
    let draws_at = |t: &Timeline, s: f64, level: RuleLevel| {
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
        let mut scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
        assert_eq!(tl::scene_space(&store, &scene), key(0x800));
        let applied = tl::apply(&mut scene, &mut loader, &t.at(s), level, &b)?;
        let built = build(&scene, &loader, &m.limits, options, &b).unwrap();
        Ok::<_, fo4_core::Error>((
            built
                .list
                .draws
                .iter()
                .map(|d| d.world.translation)
                .collect::<Vec<_>>(),
            applied,
        ))
    };
    let t = timeline(here.clone());
    let (before, applied) = draws_at(&t, 0.5, RuleLevel::Facts).unwrap();
    assert!(!before.is_empty());
    assert!(applied.rules.is_empty());
    // Moved: drawn the same, shifted by the new position.
    let (moved, applied) = draws_at(&t, 1.5, RuleLevel::Hypotheses).unwrap();
    assert_eq!(moved.len(), before.len());
    for (a, b) in before.iter().zip(&moved) {
        let d: Vec<f64> = (0..3).map(|i| b[i] - a[i]).collect();
        assert_eq!(d, vec![50., 60., 70.]);
    }
    assert_eq!(applied.moved.len(), 1);
    assert!(applied.rules.contains(fo4_scene::hypotheses::TIMELINE_MOVE));
    // The pending rules need the pending level.
    let refused = draws_at(&t, 1.5, RuleLevel::Measured).unwrap_err();
    assert_eq!(refused.code, "TimelineRuleLevel");
    // Disabled: not drawn.
    let (gone, applied) = draws_at(&t, 2.5, RuleLevel::Hypotheses).unwrap();
    assert!(gone.is_empty());
    assert_eq!(applied.hidden, vec![key(0x803)]);
    assert!(applied.rules.contains(fo4_scene::hypotheses::DISABLE_HIDES));
    // Moved to another space, or to a place the run leaves open: left out.
    let away = Spot::At {
        space: key(0x900),
        pos: [0.; 3],
    };
    for spot in [
        away,
        Spot::Open("a native of the run may have moved it".into()),
    ] {
        let (left, applied) = draws_at(&timeline(spot), 1.5, RuleLevel::Hypotheses).unwrap();
        assert!(left.is_empty());
        assert_eq!(applied.moved_out.len(), 1);
    }
}

/// `static_unmoved_parts_drawn_v1`: a static whose model's second shape
/// carries an extra data block the reader does not decode is refused
/// whole; behind `Loader::set_render_static_parts` it is drawn by its other
/// shape and the reached one is left out, counted by the blocker's code.
#[test]
fn a_static_refused_for_an_unread_block_is_drawn_by_its_other_parts_behind_the_switch() {
    use fo4_scene::hypotheses::STATIC_PARTS;
    let mut blocks = model_blocks(b"textures/a.dds", None, None, (b"", b""), None);
    // The second shape (block 4): one extra data link, to block 5.
    let helper = blocks[4].1.clone();
    let mut linked = helper[..4].to_vec();
    linked.extend(1u32.to_le_bytes());
    linked.extend(5u32.to_le_bytes());
    linked.extend(&helper[8..]);
    blocks[4].1 = linked;
    blocks.push(("BSPositionData", vec![0; 8]));
    let part = nif_file(blocks, b"PlainLabel", b"");
    let (_temp, m) = fixture_placing("render-static-parts", &dds(), part, 0x801);
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let options = || BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1 << 20,
        level: RuleLevel::Hypotheses,
        blocks: None,
        time: None,
        clock: Default::default(),
        vertex_normals: false,
        chains: false,
    };
    // Off by default: refused whole.
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    assert!(!loader.render_static_parts());
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    assert!(scene.instances[0]
        .blockers
        .iter()
        .any(|e| e.code == "UnsupportedNifBlock" && e.found == "BSPositionData"));
    let off = build(&scene, &loader, &m.limits, options(), &b).unwrap();
    assert_eq!(off.omitted.len(), 1, "{:?}", off.omitted);
    assert!(off.list.draws.is_empty() && off.left_out.is_empty());
    // On: drawn under the rule, the shape reading the block left out.
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Facts, &b);
    loader.set_render_static_parts(true);
    let scene = loader.by_editor_id("SyntheticScolCell", &b).unwrap();
    assert!(scene.instances[0]
        .state
        .hypotheses()
        .contains(&STATIC_PARTS));
    let on = build(&scene, &loader, &m.limits, options(), &b).unwrap();
    assert!(on.omitted.is_empty(), "{:?}", on.omitted);
    assert!(on.instances[0].hypotheses.contains(&STATIC_PARTS));
    assert!(!on.list.draws.is_empty());
    assert_eq!(
        on.left_out,
        vec![(on.instances[0].index, "UnsupportedNifBlock".to_string())]
    );
    // Every draw is the first shape's: the left-out shape has none.
    let meshes: std::collections::BTreeSet<usize> = on.list.draws.iter().map(|d| d.mesh).collect();
    assert_eq!(meshes.len(), 1);
    // At the measured level the pending rule is not used.
    let measured = build(
        &scene,
        &loader,
        &m.limits,
        BuildOptions {
            level: RuleLevel::Measured,
            ..options()
        },
        &b,
    )
    .unwrap();
    assert!(measured.list.draws.is_empty());
}
