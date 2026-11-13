//! Fully synthetic SCOL: an untextured generated alternative and two textured parts.
use crate::draw::{build, BuildOptions};
use fo4_core::{Budget, Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use fo4_scene::{Loader, PathPolicy};
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
    for n in [3u32, 15u32.max(label.len() as u32), 0, 12] {
        v.extend(n.to_le_bytes());
    }
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
#[test]
fn scol_hypothesis_draws_two_textured_parts_without_the_generated_alternative() {
    let temp = TestDir::new("render-scol");
    let root = temp.join("Data");
    std::fs::create_dir_all(root.join("meshes")).unwrap();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(root.join("meshes/generated.nif"), nif(b"")).unwrap();
    std::fs::write(root.join("meshes/part.nif"), nif(b"textures/a.dds")).unwrap();
    std::fs::write(root.join("textures/a.dds"), dds()).unwrap();
    let mut scol = sub(b"MODL", b"generated.nif\0");
    scol.extend(sub(b"ONAM", &0x801u32.to_le_bytes()));
    let mut p = Vec::new();
    for z in [-0.4f32, 0.4] {
        for n in [0., 0., z, 0., 0., 0., 1.] {
            p.extend(n.to_le_bytes());
        }
    }
    scol.extend(sub(b"DATA", &p));
    let mut rf = sub(b"NAME", &0x802u32.to_le_bytes());
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
    let b = Budget::new(32 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", PathPolicy::Strict, &b);
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
fn stage17_untextured_effect_requires_hypothesis_and_refuses_unhandled_features() {
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
        let mut loader = Loader::new(&store, &vfs, "english", PathPolicy::Strict, &b);
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
