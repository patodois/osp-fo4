//! Fully synthetic SCOL: an untextured generated alternative and two textured parts.
use crate::draw::{build, BuildOptions};
use fo4_core::{Budget, Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use fo4_scene::{Loader, RuleLevel};
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
    for n in [u32::MAX, 1, u32::MAX] {
        mesh.extend(n.to_le_bytes());
    }
    mesh.extend(((3u64 << 44) | (2 << 8) | 3).to_le_bytes());
    mesh.extend(1u32.to_le_bytes());
    mesh.extend(3u16.to_le_bytes());
    mesh.extend(42u32.to_le_bytes());
    for xyz in [[0x3c00u16, 0, 0], [0x3c00, 0x3800, 0], [0x3c00, 0, 0x3800]] {
        for n in xyz {
            mesh.extend(n.to_le_bytes());
        }
        mesh.extend([0; 6]);
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
    if let Some(flags1) = effect {
        shader.clear();
        for n in [0u32, 0, u32::MAX, flags1, 0] {
            shader.extend(n.to_le_bytes());
        }
        for v in [0f32, 0., 1., 1.] {
            shader.extend(v.to_le_bytes());
        }
        shader.extend(0u32.to_le_bytes());
        shader.extend([3, 255, 0, 0]);
        for v in [1f32, 1., 0., 0., 0.2, 0.4, 0.6, 0.8, 2., 100.] {
            shader.extend(v.to_le_bytes());
        }
        for _ in 0..4 {
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
    let blocks = [
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
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, 5, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend(5u16.to_le_bytes());
    for (name, _) in &blocks {
        v.extend((name.len() as u32).to_le_bytes());
        v.extend(name.as_bytes());
    }
    for i in [0u16, 1, 2, 3, 4] {
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
fn dds() -> Vec<u8> {
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
    let mut cell = sub(b"EDID", b"SyntheticScolCell\0");
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let body = [
        rec(b"STAT", 0x801, 0, &sub(b"MODL", b"part.nif\0")),
        rec(b"SCOL", 0x802, 0, &scol),
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
            files: vec![
                "meshes/generated.nif".into(),
                "meshes/part.nif".into(),
                "textures/a.dds".into(),
            ],
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
        },
        &b,
    )
    .unwrap();
    assert!(output.omitted.is_empty(), "{:?}", output.omitted);
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

/// A textured skinned model: one bone "Bone" bound at z 10, and a
/// skeleton whose rest pose puts the bone at `rest_z`.
fn skinned_files(rest_z: f32) -> (Vec<u8>, Vec<u8>) {
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
    let temp = TestDir::new("effect-surface");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    let mut bytes = nif_kind(b"", Some(flags));
    let old: Vec<u8> = [2f32, 100.].iter().flat_map(|v| v.to_le_bytes()).collect();
    let at = bytes.windows(8).position(|w| w == old.as_slice()).unwrap();
    bytes[at..at + 4].copy_from_slice(&scale.to_le_bytes());
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
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
    let mut scene = loader.by_editor_id("EffectCell", &b).unwrap();
    let mut hypotheses = scene.instances[0].state.hypotheses().to_vec();
    hypotheses.push(fo4_scene::hypotheses::REST_EFFECT_FIRST_KEY);
    scene.instances[0].state = fo4_scene::State::ReadyUnderHypotheses { hypotheses };
    let options = BuildOptions {
        max_texture_dimension: 16,
        max_texture_bytes: 1024,
        level: fo4_scene::RuleLevel::Hypotheses,
        blocks: None,
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
    // Each unsettled term is named, together.
    for (flags, scale, found) in [
        (0, 2., "colour-range"),
        (1 << 6, 0.5, "falloff"),
        (1 << 4, 0.5, "palette"),
        (1 << 7 | 1 << 5, 2., "palette+feature+colour-range"),
    ] {
        let output = effect_surface_build(flags, scale);
        assert!(output.list.draws.is_empty());
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
