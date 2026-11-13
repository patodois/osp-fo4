use super::*;
use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use std::fs;
fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    for n in [data.len() as u32, flags, id, 0] {
        v.extend(n.to_le_bytes());
    }
    v.extend(131u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(data);
    v
}
fn group(kind: u32, id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = b"GRUP".to_vec();
    v.extend((data.len() as u32 + 24).to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend([0; 8]);
    v.extend(data);
    v
}
fn nif(unknown: bool) -> Vec<u8> {
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, if unknown { 1 } else { 0 }, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend((if unknown { 1u16 } else { 0 }).to_le_bytes());
    if unknown {
        let ty = b"FutureMystery";
        v.extend((ty.len() as u32).to_le_bytes());
        v.extend(ty);
        v.extend(0u16.to_le_bytes());
        v.extend(0u32.to_le_bytes());
    }
    for n in [0u32, 0, 0, if unknown { 1 } else { 0 }] {
        v.extend(n.to_le_bytes());
    }
    if unknown {
        v.extend(0u32.to_le_bytes());
    }
    v
}
struct Fixture {
    _temp: TestDir,
    manifest: Manifest,
}
fn fixture(runtime: bool, unknown: bool) -> Fixture {
    fixture_with_cell(runtime, unknown, &[])
}
fn fixture_with_cell(runtime: bool, unknown: bool, extra: &[u8]) -> Fixture {
    let temp = TestDir::new("scene-stage11");
    let root = temp.join("Data");
    fs::create_dir_all(root.join("meshes")).unwrap();
    fs::write(root.join("meshes/a.nif"), nif(unknown)).unwrap();
    let mut cell = sub(b"EDID", b"SyntheticRoom\0");
    cell.extend(sub(b"FULL", b"Room Name\0"));
    cell.extend(sub(b"DATA", &1u16.to_le_bytes()));
    cell.extend(sub(b"PCMB", &[1, 2, 3, 4]));
    cell.extend(extra);
    let base = record(b"STAT", 0x801, 0, &sub(b"MODL", b"a.nif\0"));
    let mut rf = sub(b"NAME", &0x801u32.to_le_bytes());
    let mut tr = Vec::new();
    for f in [12.5f32, -2., 3., 0.1, 0.2, 0.3] {
        tr.extend(f.to_le_bytes());
    }
    rf.extend(sub(b"DATA", &tr));
    rf.extend(sub(b"XSCL", &0.7f32.to_le_bytes()));
    if runtime {
        rf.extend(sub(b"XESP", &[0; 8]));
    }
    let mut references = record(b"REFR", 0x802, 0, &rf);
    references.extend(record(b"REFR", 0x803, fo4_plugin::DELETED, &rf));
    references.extend(record(b"REFR", 0x804, fo4_plugin::DISABLED, &rf));
    references.extend(record(b"ACHR", 0x805, 0, &[]));
    let mut body = group(
        2,
        0,
        &[
            record(b"CELL", 0x800, 0, &cell),
            group(6, 0x800, &group(9, 0x800, &references)),
        ]
        .concat(),
    );
    body.extend(base);
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
            files: vec!["meshes/a.nif".into()],
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
fn cell_by_id_transform_origin_visibility_and_exclusions() {
    let f = fixture(false, false);
    let index = Budget::new(8 << 20);
    let store = Store::open(&f.manifest, &index).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut loader = Loader::new(&store, &v, "english", PathPolicy::Strict, &cache);
    let b = Budget::new(8 << 20);
    let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    assert_eq!(scene.name.as_deref(), Some("Room Name"));
    assert_eq!(scene.kind, Some(CellKind::Interior));
    assert_eq!(scene.instances.len(), 1);
    assert_eq!(scene.deleted.len(), 1);
    assert_eq!(scene.disabled.len(), 1);
    assert_eq!(scene.exclusions[0].code, "UnsupportedPlacedRecordType");
    assert_eq!(scene.visibility.precombine.len(), 1);
    let inst = &scene.instances[0];
    assert!(
        matches!(inst.state, State::ReadyUnderHypotheses { ref hypotheses } if hypotheses == &[crate::hypotheses::ROTATION])
    );
    assert_eq!(
        inst.reference.data.transform,
        Some([12.5, -2., 3., 0.1, 0.2, 0.3])
    );
    assert_eq!(inst.reference.data.scale, Some(0.7));
    assert!(inst.reference.record.source.file.ends_with("base.esm"));
    assert!(loader.assets.nodes[inst.assets[0]]
        .origin
        .as_ref()
        .unwrap()
        .file
        .ends_with("meshes/a.nif"));
}
#[test]
fn reached_unknown_and_runtime_are_named_and_budgeted() {
    let f = fixture(true, true);
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        PathPolicy::Strict,
        &Budget::new(8 << 20),
    );
    let s = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    let i = &s.instances[0];
    assert!(matches!(i.state, State::Blocked));
    assert!(i.blockers.iter().any(|e| e.code == "RuntimeEnableParent"));
    assert!(i.blockers.iter().any(|e| e.code == "UnsupportedNifBlock"));
    assert!(loader
        .by_editor_id("SyntheticRoom", &Budget::new(1))
        .is_err());
}

fn sized(v: &mut Vec<u8>, text: &[u8]) {
    v.extend((text.len() as u32).to_le_bytes());
    v.extend(text);
}
fn nif_blocks(blocks: &[(&str, Vec<u8>)], strings: &[&[u8]]) -> Vec<u8> {
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, blocks.len() as u32, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend((blocks.len() as u16).to_le_bytes());
    for (ty, _) in blocks {
        sized(&mut v, ty.as_bytes());
    }
    for i in 0..blocks.len() {
        v.extend((i as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        v.extend((data.len() as u32).to_le_bytes());
    }
    v.extend((strings.len() as u32).to_le_bytes());
    v.extend(
        strings
            .iter()
            .map(|s| s.len() as u32)
            .max()
            .unwrap_or(0)
            .to_le_bytes(),
    );
    for s in strings {
        sized(&mut v, s);
    }
    v.extend(0u32.to_le_bytes());
    for (_, data) in blocks {
        v.extend(data);
    }
    v.extend(1u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v
}
fn effect_shader() -> Vec<u8> {
    let mut v = Vec::new();
    for n in [0u32, 0, u32::MAX, 0, 0] {
        v.extend(n.to_le_bytes());
    }
    for f in [0f32, 0., 1., 1.] {
        v.extend(f.to_le_bytes());
    }
    sized(&mut v, b"");
    v.extend([0; 4]);
    for _ in 0..10 {
        v.extend(1f32.to_le_bytes());
    }
    for _ in 0..4 {
        sized(&mut v, b"");
    }
    v.extend(1f32.to_le_bytes());
    v
}
fn effect_material(path: &[u8]) -> Vec<u8> {
    let mut v = b"BGEM".to_vec();
    v.extend(2u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    for _ in 0..5 {
        v.extend(1f32.to_le_bytes());
    }
    v.push(0);
    v.extend(0u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend([0; 12]);
    v.extend(1f32.to_le_bytes());
    v.push(0);
    v.extend(1f32.to_le_bytes());
    v.push(0);
    for i in 0..5 {
        let path = if i == 0 { path } else { b"" };
        v.extend((path.len() as u32 + 1).to_le_bytes());
        v.extend(path);
        v.push(0);
    }
    v.extend([0; 6]);
    for _ in 0..9 {
        v.extend(1f32.to_le_bytes());
    }
    v.push(0);
    v.extend(1f32.to_le_bytes());
    v
}
fn dds() -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x21007;
    h[2] = 4;
    h[3] = 4;
    h[6] = 1;
    h[18] = 32;
    h[19] = 4;
    h[20] = u32::from_le_bytes(*b"DXT1");
    h[26] = 0x401008;
    let mut v = b"DDS ".to_vec();
    for n in h {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 8]);
    v
}
#[test]
fn asset_chain_keeps_provenance_and_does_not_reach_unlinked_unknown() {
    let mut f = fixture(false, false);
    fs::create_dir(f.manifest.root.join("materials")).unwrap();
    fs::create_dir(f.manifest.root.join("textures")).unwrap();
    let model = nif_blocks(
        &[
            ("BSEffectShaderProperty", effect_shader()),
            ("FutureUnlinked", vec![1, 2]),
        ],
        &[b"materials/a.bgem"],
    );
    fs::write(f.manifest.root.join("meshes/a.nif"), model).unwrap();
    fs::write(
        f.manifest.root.join("materials/a.bgem"),
        effect_material(b"a.dds"),
    )
    .unwrap();
    fs::write(f.manifest.root.join("textures/a.dds"), dds()).unwrap();
    let Mount::Loose { files, .. } = &mut f.manifest.mounts[0] else {
        unreachable!()
    };
    files.extend(["materials/a.bgem".into(), "textures/a.dds".into()]);
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut assets = Assets::new(&v, &Budget::new(8 << 20));
    let b = Budget::new(8 << 20);
    let root = assets.load(AssetKind::Model, b"meshes/a.nif", &b).unwrap();
    assert!(
        assets.nodes[root].ready,
        "{:?}",
        assets.nodes[root].blockers
    );
    let closure = assets.closure(&[root], &b).unwrap();
    assert_eq!(closure.len(), 3);
    assert_eq!(assets.nodes[closure[1]].kind, AssetKind::Material);
    assert_eq!(assets.nodes[closure[2]].kind, AssetKind::Texture);
    for i in closure {
        assert!(assets.nodes[i].origin.is_some());
    }
    let cached = assets.load(AssetKind::Model, b"MESHES\\A.NIF", &b).unwrap();
    assert_eq!(root, cached);
    assert_eq!(assets.nodes.len(), 3);
    for (kind, path, expected) in [
        (
            AssetKind::Model,
            b"meshes/absent.nif".as_slice(),
            "MissingModel",
        ),
        (
            AssetKind::Material,
            b"materials/absent.bgsm",
            "MissingMaterial",
        ),
        (AssetKind::Texture, b"textures/absent.dds", "MissingTexture"),
        (AssetKind::Model, b"C:\\dev\\a.nif", "RejectedVfsPath"),
    ] {
        let id = assets.load(kind, path, &b).unwrap();
        assert!(!assets.nodes[id].ready);
        assert_eq!(assets.nodes[id].blockers[0].code, expected);
    }
}
#[test]
fn scol_parts_swaps_and_explicit_texture_set_keep_their_records() {
    let f = fixture(false, false);
    let path = f.manifest.root.join("base.esm");
    let bytes = fs::read(&path).unwrap();
    // Replace only this synthetic STAT envelope with a synthetic SCOL at the same ID.
    let marker = b"STAT";
    let offset = bytes.windows(4).position(|s| s == marker).unwrap();
    let mut bytes = bytes[..offset].to_vec();
    let mut scol = Vec::new();
    for id in [0x810u32, 0x811] {
        scol.extend(sub(b"ONAM", &id.to_le_bytes()));
        let mut placement = Vec::new();
        for f in [1f32, 2., 3., 4., 5., 6., 0.7] {
            placement.extend(f.to_le_bytes());
        }
        scol.extend(sub(b"DATA", &placement));
    }
    scol.extend(sub(b"MODS", &0x820u32.to_le_bytes()));
    bytes.extend(record(b"SCOL", 0x801, 0, &scol));
    for id in [0x810, 0x811] {
        bytes.extend(record(b"STAT", id, 0, &sub(b"MODL", b"a.nif\0")));
    }
    let mut swap = sub(b"BNAM", b"original.bgsm\0");
    swap.extend(sub(b"SNAM", b"replacement.bgsm\0"));
    bytes.extend(record(b"MSWP", 0x820, 0, &swap));
    bytes.extend(record(b"TXST", 0x821, 0, &sub(b"TX00", b"missing.dds\0")));
    fs::write(path, bytes).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        PathPolicy::Strict,
        &Budget::new(8 << 20),
    );
    let b = Budget::new(8 << 20);
    let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    assert_eq!(scene.instances.len(), 1);
    let base = scene.instances[0].base.as_ref().unwrap();
    assert_eq!(base.record.signature, *b"SCOL");
    assert_eq!(base.parts.len(), 2);
    assert!(
        matches!(base.parts[0].declared.placements,Some(ref p) if p[0]==[1.,2.,3.,4.,5.,6.,0.7])
    );
    assert_eq!(base.material_swap.as_ref().unwrap().key.object_id, 0x820);
    assert!(scene.instances[0]
        .blockers
        .iter()
        .any(|e| e.code == "MissingMaterial"));
    assert_eq!(scene.instances[0].assets.len(), 3);
    let (tx, assets) = loader
        .texture_set(
            &FormKey {
                plugin: "base.esm".into(),
                object_id: 0x821,
            },
            &b,
        )
        .unwrap();
    assert_eq!(tx.record.signature, *b"TXST");
    assert_eq!(
        loader.assets.nodes[assets[0]].blockers[0].code,
        "MissingTexture"
    );

    assert!(base
        .parts
        .iter()
        .all(|p| p.resolved.as_ref().unwrap().key.object_id >= 0x810));
}

#[test]
fn stage11_interrupted_cache_load_does_not_report_a_spurious_dependency_cycle() {
    let f = fixture(false, false);
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let budget = Budget::new(16 << 10);
    let mut assets = Assets::new(&v, &budget);
    // Cache can retain the identity but not a large malformed path error.
    let path = vec![b'x'; 32 << 10];
    let error = assets
        .load(AssetKind::Model, &path, &Budget::new(1 << 20))
        .unwrap_err();
    assert_eq!(error.code, "LimitExceeded");
    // Exercise a failure after node insertion, while publishing its diagnostic.
    let budget = Budget::new(4096);
    let mut assets = Assets::new(&v, &budget);
    let mut path = b"meshes/".to_vec();
    path.extend(vec![b'x'; 1100]);
    path.extend(b".nif");
    assert!(assets
        .load(AssetKind::Model, &path, &Budget::new(1 << 20))
        .is_err());
    let error = assets
        .load(AssetKind::Model, &path, &Budget::new(1 << 20))
        .unwrap_err();
    assert_eq!(error.code, "AssetCacheUnavailable");
}

#[test]
fn stage11_deleted_lighting_template_is_retained_but_not_certified() {
    let f = fixture_with_cell(false, false, &sub(b"LTMP", &0x900u32.to_le_bytes()));
    let path = f.manifest.root.join("base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"LGTM", 0x900, fo4_plugin::DELETED, &[]));
    fs::write(path, bytes).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        PathPolicy::Strict,
        &Budget::new(8 << 20),
    );
    let scene = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    assert!(scene.lighting_template.as_ref().unwrap().record.deleted());
    assert!(scene
        .diagnostics
        .iter()
        .any(|e| e.code == "DeletedLightingTemplate"));
}

#[test]
fn stage11_cache_identity_is_not_published_before_path_allocation() {
    let f = fixture(false, false);
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let path = b"meshes/allocation-window.nif";
    let cache_size = (2 * path.len() + 1 + 128 + std::mem::size_of::<Asset>()) as u64;
    let mut assets = Assets::new(&v, &Budget::new(cache_size));
    assert_eq!(
        assets
            .load(AssetKind::Model, path, &Budget::new(1 << 20))
            .unwrap_err()
            .code,
        "LimitExceeded"
    );
    assert!(assets.nodes.is_empty());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assets.load(AssetKind::Model, path, &Budget::new(1 << 20))
    }));
    assert!(
        result.is_ok(),
        "failed insertion must not leave an out-of-range cache identity"
    );
    assert_eq!(result.unwrap().unwrap_err().code, "LimitExceeded");
}

#[test]
fn stage13_cell_causes_are_separate_from_own_instance_readiness() {
    let f = fixture_with_cell(false, false, &sub(b"CCCC", &[1, 2, 3, 4]));
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut loader = Loader::new(&store, &v, "english", PathPolicy::Strict, &cache);
    let scene = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    assert!(scene.diagnostics.iter().any(|e| e.found == "CELL.CCCC"));
    assert!(scene.instances[0]
        .blockers
        .iter()
        .all(|e| e.code == "UnverifiedEulerRotation"));
    assert!(
        matches!(scene.instances[0].state, State::ReadyUnderHypotheses { ref hypotheses } if hypotheses == &[crate::hypotheses::ROTATION])
    );
}
#[test]
fn stage13_documented_audio_stays_raw_and_does_not_block_the_frame() {
    let f = fixture_with_cell(false, false, &sub(b"XCAS", &0x801u32.to_le_bytes()));
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut loader = Loader::new(&store, &v, "english", PathPolicy::Strict, &cache);
    let scene = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    assert!(scene
        .cell
        .data
        .common
        .uninterpreted
        .iter()
        .any(|s| s.signature == *b"XCAS"));
    assert!(scene.diagnostics.is_empty());
    assert!(
        matches!(scene.instances[0].state, State::ReadyUnderHypotheses { ref hypotheses } if hypotheses == &[crate::hypotheses::ROTATION])
    );
}

fn lighting_shader(texture: i32) -> Vec<u8> {
    let mut v = Vec::new();
    for value in [0u32, 0, 0, u32::MAX, 0, 0] {
        v.extend(value.to_le_bytes());
    }
    for _ in 0..4 {
        v.extend(0f32.to_le_bytes());
    }
    v.extend(texture.to_le_bytes());
    for _ in 0..4 {
        v.extend(0f32.to_le_bytes());
    }
    v.extend(u32::MAX.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    for _ in 0..17 {
        v.extend(0f32.to_le_bytes());
    }
    v
}
#[test]
fn stage13_bgsm_name_overrides_texture_set_v1_only_when_strictly_resolved() {
    use model_policy::{MaterialStatus, ModelPolicy, Ownership};
    let mut f = fixture(false, false);
    fs::create_dir_all(f.manifest.root.join("materials")).unwrap();
    fs::write(
        f.manifest.root.join("materials/a.bgsm"),
        b"bad material still resolves, its parse must block",
    )
    .unwrap();
    let Mount::Loose { files, .. } = &mut f.manifest.mounts[0] else {
        unreachable!()
    };
    files.push("materials/a.bgsm".into());
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    for (name, resolved) in [
        (b"materials/a.bgsm".as_slice(), true),
        (b"materials/missing.bgsm", false),
        (b"C:/dev/materials/a.bgsm", false),
    ] {
        let mut textures = 1u32.to_le_bytes().to_vec();
        sized(&mut textures, b"bare/a.dds");
        let bytes = nif_blocks(
            &[
                ("BSLightingShaderProperty", lighting_shader(1)),
                ("BSShaderTextureSet", textures),
            ],
            &[name],
        );
        let b = Budget::new(8 << 20);
        let n = fo4_nif::Nif::read(&bytes, "synthetic", Game::Fallout4, &Limits::default(), &b)
            .unwrap();
        assert!(
            n.blocks[0].decoded.is_some(),
            "{:?}",
            n.blocks[0].diagnostics
        );
        assert!(n.blocks[0].raw_tail.is_empty());
        let p = ModelPolicy::inspect(&n, &v, &b).unwrap();
        assert_eq!(p.suppress[0], resolved);
        assert_eq!(p.material[0] == MaterialStatus::Resolved, resolved);
        let audit = p.texture_citations(&n, &b).unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].suppressed, resolved);
        assert_eq!(
            audit[0].ownership,
            if resolved {
                Ownership::Material
            } else {
                Ownership::NoMaterial
            }
        );
        fs::write(f.manifest.root.join("meshes/a.nif"), &bytes).unwrap();
        let mounted = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut assets = Assets::new(&mounted, &Budget::new(8 << 20));
        let id = assets
            .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
            .unwrap();
        assert_eq!(
            assets.nodes[id]
                .rejections
                .iter()
                .any(|r| r.class == Citation::NifTexture),
            !resolved
        );
        assert!(!assets.nodes[id].ready);
    }
}
#[test]
fn stage13_collision_is_raw_outside_but_animation_and_skin_still_block() {
    for (ty, outside) in [
        ("bhkNPCollisionObject", true),
        ("BSMultiBoundAABB", true),
        ("NiTransformInterpolator", false),
        ("BSSkin::Instance", false),
    ] {
        let f = fixture(false, false);
        fs::write(
            f.manifest.root.join("meshes/a.nif"),
            nif_blocks(&[(ty, vec![0; 4])], &[]),
        )
        .unwrap();
        let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut a = Assets::new(&v, &Budget::new(8 << 20));
        let id = a
            .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
            .unwrap();
        assert_eq!(a.nodes[id].blockers.is_empty(), outside, "{ty}");
        if outside {
            assert_eq!(a.nodes[id].outside_blocks[ty], 1);
        } else {
            assert!(a.nodes[id].blockers.iter().any(|e| e.found.starts_with(ty)));
        }
    }
}

fn scene_node(controller: i32, child: Option<i32>) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend(u32::MAX.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend(controller.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    for _ in 0..12 {
        v.extend(0f32.to_le_bytes());
    }
    v.extend(1f32.to_le_bytes());
    v.extend((-1i32).to_le_bytes());
    v.extend(u32::from(child.is_some()).to_le_bytes());
    if let Some(i) = child {
        v.extend(i.to_le_bytes());
    }
    v
}
#[test]
fn stage13_animation_target_membership_is_known_only_for_decoded_targets() {
    for (target, expected) in [(0i32, Some(true)), (2, Some(false)), (-1, None)] {
        let f = fixture(false, false);
        let mut control = (-1i32).to_le_bytes().to_vec();
        control.extend(0u16.to_le_bytes());
        for _ in 0..4 {
            control.extend(0f32.to_le_bytes());
        }
        control.extend(target.to_le_bytes());
        control.extend((-1i32).to_le_bytes());
        let bytes = nif_blocks(
            &[
                ("NiNode", scene_node(1, None)),
                ("NiTransformController", control),
                ("NiNode", scene_node(-1, None)),
            ],
            &[],
        );
        fs::write(f.manifest.root.join("meshes/a.nif"), bytes).unwrap();
        let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut a = Assets::new(&v, &Budget::new(8 << 20));
        let id = a
            .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
            .unwrap();
        let animation = &a.nodes[id].animations[0];
        assert_eq!(animation.target, Some(target));
        assert_eq!(animation.target_in_visual, expected);
        assert!(a.nodes[id]
            .blockers
            .iter()
            .any(|e| e.code == "UnsupportedAnimationRuntime"));
    }
}
#[test]
fn stage13_new_visible_types_keep_their_models_and_audio_markers_are_outside() {
    let f = fixture(false, false);
    let mut bytes = fs::read(f.manifest.root.join("base.esm")).unwrap();
    let types = [
        *b"MISC", *b"CONT", *b"ALCH", *b"BOOK", *b"AMMO", *b"KEYM", *b"NOTE", *b"FLOR", *b"TERM",
        *b"TACT", *b"IDLM", *b"SOUN", *b"ASPC",
    ];
    for (j, ty) in types.iter().enumerate() {
        bytes.extend(record(
            ty,
            0x900 + j as u32,
            0,
            &sub(
                b"MODL",
                if j < 10 {
                    b"a.nif\0"
                } else {
                    b"missing_marker.nif\0"
                },
            ),
        ));
    }
    fs::write(f.manifest.root.join("base.esm"), bytes).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        PathPolicy::Strict,
        &Budget::new(8 << 20),
    );
    for j in 0..types.len() {
        let inspect = loader
            .inspect_base(
                &FormKey {
                    plugin: "base.esm".into(),
                    object_id: 0x900 + j as u32,
                },
                &Budget::new(8 << 20),
            )
            .unwrap();
        if j < 10 {
            assert!(matches!(inspect.state, State::Ready));
            assert_eq!(inspect.assets.len(), 1);
        } else {
            assert!(matches!(inspect.state, State::Outside));
            assert!(inspect.assets.is_empty());
        }
    }
}

#[test]
fn stage13_extra_animation_target_can_reach_visual_when_primary_does_not() {
    let f = fixture(false, false);
    let mut control = (-1i32).to_le_bytes().to_vec();
    control.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        control.extend(0f32.to_le_bytes());
    }
    control.extend(2i32.to_le_bytes());
    control.extend(1u16.to_le_bytes());
    control.extend(0i32.to_le_bytes());
    let bytes = nif_blocks(
        &[
            ("NiNode", scene_node(1, None)),
            ("NiMultiTargetTransformController", control),
            ("NiNode", scene_node(-1, None)),
        ],
        &[],
    );
    fs::write(f.manifest.root.join("meshes/a.nif"), bytes).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut a = Assets::new(&v, &Budget::new(8 << 20));
    let id = a
        .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
        .unwrap();
    let animation = &a.nodes[id].animations[0];
    assert_eq!(animation.target, Some(2));
    assert_eq!(animation.targets, vec![2, 0]);
    assert_eq!(animation.target_in_visual, Some(true));
}

#[test]
fn stage13_decoded_structure_does_not_certify_an_unknown_block_role() {
    let f = fixture(false, false);
    let mut data = u32::MAX.to_le_bytes().to_vec();
    data.extend(0u32.to_le_bytes());
    fs::write(
        f.manifest.root.join("meshes/a.nif"),
        nif_blocks(&[("BSBoneLODExtraData", data)], &[]),
    )
    .unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut a = Assets::new(&v, &Budget::new(8 << 20));
    let id = a
        .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
        .unwrap();
    assert!(!a.nodes[id].ready);
    assert!(a.nodes[id]
        .blockers
        .iter()
        .any(|e| e.code == "UnsupportedNifBlockSemantics" && e.found == "BSBoneLODExtraData"));
}

#[test]
fn stage13_misc_preview_and_keywords_stay_raw_without_blocking_its_model() {
    let f = fixture(false, false);
    let mut bytes = fs::read(f.manifest.root.join("base.esm")).unwrap();
    let mut data = sub(b"MODL", b"a.nif\0");
    for (tag, value) in [(b"PTRN", 0u32), (b"KSIZ", 1u32), (b"KWDA", 0u32)] {
        data.extend(sub(tag, &value.to_le_bytes()));
    }
    bytes.extend(record(b"MISC", 0x8f0, 0, &data));
    fs::write(f.manifest.root.join("base.esm"), bytes).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        PathPolicy::Strict,
        &Budget::new(8 << 20),
    );
    let inspect = loader
        .inspect_base(
            &FormKey {
                plugin: "base.esm".into(),
                object_id: 0x8f0,
            },
            &Budget::new(8 << 20),
        )
        .unwrap();
    assert!(matches!(inspect.state, State::Ready));
    assert_eq!(inspect.base.common.uninterpreted.len(), 2);
    assert_eq!(inspect.assets.len(), 1);
}

#[test]
fn stage13_additional_controller_prefixes_keep_targets_and_opaque_bodies() {
    for ty in [
        "BSProceduralLightningController",
        "NiPSysInitialRotSpeedCtlr",
        "NiPSysInitialRotSpeedVarCtlr",
    ] {
        let f = fixture(false, false);
        let mut control = (-1i32).to_le_bytes().to_vec();
        control.extend(0u16.to_le_bytes());
        for _ in 0..4 {
            control.extend(0f32.to_le_bytes());
        }
        control.extend(0i32.to_le_bytes());
        control.extend([1, 2, 3, 4]);
        fs::write(
            f.manifest.root.join("meshes/a.nif"),
            nif_blocks(&[("NiNode", scene_node(1, None)), (ty, control)], &[]),
        )
        .unwrap();
        let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut a = Assets::new(&v, &Budget::new(8 << 20));
        let id = a
            .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
            .unwrap();
        assert_eq!(
            model_policy::block_class(ty),
            fo4_plugin::still::Class::State
        );
        assert_eq!(a.nodes[id].animations[0].target, Some(0));
        assert_eq!(a.nodes[id].animations[0].target_in_visual, Some(true));
        assert!(
            a.nodes[id]
                .blockers
                .iter()
                .any(|e| e.code == "UnsupportedNifField"
                    && e.found == format!("{ty}.Controller Body"))
        );
        assert!(!a.nodes[id].ready);
    }
}

#[test]
fn stage13_common_controller_prefix_truncations_remain_raw_without_panic() {
    let mut control = (-1i32).to_le_bytes().to_vec();
    control.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        control.extend(0f32.to_le_bytes());
    }
    control.extend(0i32.to_le_bytes());
    assert_eq!(control.len(), 26);
    for ty in [
        "BSProceduralLightningController",
        "NiPSysInitialRotSpeedCtlr",
        "NiPSysInitialRotSpeedVarCtlr",
    ] {
        for end in 0..control.len() {
            let bytes = nif_blocks(
                &[
                    ("NiNode", scene_node(1, None)),
                    (ty, control[..end].to_vec()),
                ],
                &[],
            );
            let budget = Budget::new(1 << 20);
            let nif = fo4_nif::Nif::read(
                &bytes,
                "truncated-controller",
                Game::Fallout4,
                &Limits::default(),
                &budget,
            )
            .unwrap();
            assert!(nif.blocks[1].decoded.is_none(), "{ty} {end}");
            assert!(!nif.blocks[1].diagnostics.is_empty());
            assert!(budget.used() <= 1 << 20);
        }
    }
}

#[test]
fn stage13_next_controller_uses_the_same_sourced_prefix() {
    let f = fixture(false, false);
    let mut controller = 2i32.to_le_bytes().to_vec();
    controller.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        controller.extend(0f32.to_le_bytes());
    }
    controller.extend(0i32.to_le_bytes());
    controller.extend([1, 2, 3, 4]);
    let mut next = (-1i32).to_le_bytes().to_vec();
    next.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        next.extend(0f32.to_le_bytes());
    }
    next.extend(0i32.to_le_bytes());
    let bytes = nif_blocks(
        &[
            ("NiNode", scene_node(1, None)),
            ("NiPSysInitialRotSpeedCtlr", controller),
            ("NiPSysUpdateCtlr", next),
        ],
        &[],
    );
    fs::write(f.manifest.root.join("meshes/a.nif"), bytes).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut a = Assets::new(&v, &Budget::new(8 << 20));
    let id = a
        .load(AssetKind::Model, b"meshes/a.nif", &Budget::new(8 << 20))
        .unwrap();
    let target = a.nodes[id]
        .animations
        .iter()
        .find(|c| c.block_type == "NiPSysUpdateCtlr")
        .unwrap();
    assert_eq!(target.target, Some(0));
    assert_eq!(target.target_in_visual, Some(true));
    assert!(!a.nodes[id]
        .blockers
        .iter()
        .any(|e| e.code == "UnsupportedNifBlock" && e.found == "NiPSysUpdateCtlr"));
    assert!(!a.nodes[id]
        .blockers
        .iter()
        .any(|e| e.code == "UnsupportedNifField" && e.found == "NiPSysUpdateCtlr.Controller Body"));
}

#[test]
fn stage15_explicit_overlay_and_marker_visibility_keep_data_readiness_separate() {
    let f = fixture(false, false);
    let overlay = f.manifest.root.parent().unwrap().join("OwnOverlay.esp");
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut header = sub(b"HEDR", &hedr);
    header.extend(sub(b"MAST", b"base.esm\0"));
    header.extend(sub(b"DATA", &[0; 8]));
    let mut payload = sub(b"MODL", b"a.nif\0");
    payload.extend(sub(b"MODT", &[9, 8, 7]));
    let mut bytes = record(b"TES4", 0, 0, &header);
    bytes.extend(record(b"STAT", 0x801, 0x0080_0000, &payload));
    fs::write(&overlay, bytes).unwrap();
    let original = fs::read(f.manifest.root.join("base.esm")).unwrap();
    let store = Store::open_overlay(&f.manifest, Some(&overlay), &Budget::new(8 << 20)).unwrap();
    assert_eq!(store.order().headers().len(), 2);
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut l = Loader::new(&store, &v, "english", PathPolicy::Strict, &cache);
    let s = l
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    let i = &s.instances[0];
    assert!(i.editor_marker);
    assert_eq!(i.visibility_hypotheses, [crate::hypotheses::MARKER]);
    assert!(
        matches!(&i.state,State::ReadyUnderHypotheses{hypotheses} if hypotheses==&[crate::hypotheses::MODT,crate::hypotheses::ROTATION])
    );
    assert!(i.blockers.iter().any(|e| e.found.contains("STAT.MODT")));
    assert_eq!(
        fs::read(f.manifest.root.join("base.esm")).unwrap(),
        original
    );
    assert!(Store::open_overlay(
        &f.manifest,
        Some(&f.manifest.root.join("base.esm")),
        &Budget::new(8 << 20)
    )
    .is_err());
}

#[test]
fn stage15_absent_scale_stays_absent_and_is_only_usable_under_its_hypothesis() {
    let f = fixture(false, false);
    let path = f.manifest.root.join("base.esm");
    let raw = fs::read(&path).unwrap();
    let needle = [b"XSCL\x04\0".as_slice(), &0.7f32.to_le_bytes()].concat();
    let mut rewritten = Vec::new();
    let mut at = 0;
    // This synthetic fixture has one trailing base record; replace each XSCL
    // with an equal-size outside REFR field rather than corrupt group sizes.
    while let Some(found) = raw[at..].windows(needle.len()).position(|w| w == needle) {
        let start = at + found;
        rewritten.extend_from_slice(&raw[at..start]);
        rewritten.extend_from_slice(b"XLYR\x04\0");
        rewritten.extend_from_slice(&0u32.to_le_bytes());
        at = start + needle.len();
    }
    rewritten.extend_from_slice(&raw[at..]);
    assert_eq!(rewritten.len(), raw.len());
    fs::write(&path, rewritten).unwrap();
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut l = Loader::new(&store, &v, "english", PathPolicy::Strict, &cache);
    let s = l
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    let i = &s.instances[0];
    assert_eq!(i.reference.data.scale, None);
    assert!(i
        .blockers
        .iter()
        .any(|e| e.code == "MissingDocumentedReferenceScale"));
    assert!(
        matches!(&i.state,State::ReadyUnderHypotheses{hypotheses} if hypotheses==&[crate::hypotheses::SCALE,crate::hypotheses::ROTATION])
    );
    // Coverage never replaces the raw Option with 1.
    let e = i
        .blockers
        .iter()
        .find(|e| e.code == "MissingDocumentedReferenceScale")
        .unwrap();
    assert_eq!(
        crate::hypotheses::covering(e),
        Some(crate::hypotheses::SCALE)
    );
    for name in [
        crate::hypotheses::MODT,
        crate::hypotheses::BLEND,
        crate::hypotheses::ROTATION,
        crate::hypotheses::MARKER,
        crate::hypotheses::SCALE,
        crate::hypotheses::SCOL,
    ] {
        assert!(crate::hypotheses::REGISTRY
            .lines()
            .any(|r| r.split('\t').next() == Some(name)));
    }
}

#[test]
fn stage16_path_hypotheses_require_a_unique_existing_winner_and_preserve_strictness() {
    use paths::{candidate, resolve_with, Citation};
    let mut f = fixture(false, false);
    let root = &f.manifest.root;
    fs::create_dir_all(root.join("textures")).unwrap();
    fs::create_dir_all(root.join("materials")).unwrap();
    fs::write(root.join("textures/a.dds"), b"synthetic").unwrap();
    fs::write(root.join("materials/a.bgsm"), b"synthetic").unwrap();
    if let Mount::Loose { files, .. } = &mut f.manifest.mounts[0] {
        files.extend(["textures/a.dds".into(), "materials/a.bgsm".into()]);
    }
    let b = Budget::new(8 << 20);
    let v = Vfs::mount(&f.manifest, &b).unwrap();
    for (c, raw, key, hs) in [
        (
            Citation::NifTexture,
            b"a.dds".as_slice(),
            b"textures/a.dds".as_slice(),
            vec![hypotheses::PATH_ROOT],
        ),
        (
            Citation::NifMaterial,
            b"Z:\\old\\data\\wrong\\DATA\\Materials\\A.BGSM",
            b"materials/a.bgsm",
            vec![hypotheses::PATH_STRIP],
        ),
        (
            Citation::NifTexture,
            b"Z:/data/a.dds",
            b"textures/a.dds",
            vec![hypotheses::PATH_STRIP, hypotheses::PATH_ROOT],
        ),
    ] {
        assert!(resolve_with(c, raw, &v, PathPolicy::Strict, &b).is_err());
        let r = resolve_with(c, raw, &v, PathPolicy::UnderHypotheses, &b).unwrap();
        assert_eq!(r.key, key);
        assert_eq!(r.hypotheses, hs);
        assert_eq!(
            r.original_rejection.as_ref().unwrap().code,
            "RejectedVfsPath"
        );
        let errors = r.diagnostics("synthetic", 17, &b).unwrap();
        assert!(matches!(
            hypotheses::classify(&errors, false, &b).unwrap(),
            State::ReadyUnderHypotheses { .. }
        ));
    }
    for raw in [
        b"missing.dds".as_slice(),
        b"../a.dds",
        b"Z:/metadata/textures/a.dds",
        b"Z:/data/../textures/a.dds",
        b"Z:data/textures/a.dds",
        b"/a.dds",
        b"materials/a.dds",
    ] {
        assert!(resolve_with(
            Citation::NifTexture,
            raw,
            &v,
            PathPolicy::UnderHypotheses,
            &b
        )
        .is_err());
    }
    assert!(candidate(Citation::NifTexture, b"a.dds", &Budget::new(0)).is_err());
}

#[test]
fn stage16_numeric_marker_labels_are_a_narrow_hypothesis_not_a_filename_heuristic() {
    for n in [
        b"EditorMarker:27".as_slice(),
        b"EditorMarker042",
        b"EditorMarker042:9",
    ] {
        assert!(model_policy::marker_decimal_label(n));
    }
    for n in [
        b"EditorMarker".as_slice(),
        b"EditorMarker:",
        b"EditorMarker42:",
        b"EditorMarker:2:3",
        b"EditorMarkerA:9",
        b"editorMarker042",
        b"meshes/EditorMarker042.nif",
    ] {
        assert!(!model_policy::marker_decimal_label(n));
    }
}

#[test]
fn asset_cache_reset_preserves_explicit_path_hypotheses_and_strict_refusal() {
    let f = fixture(false, false);
    let b = Budget::new(32 << 20);
    let store = Store::open(&f.manifest, &b).unwrap();
    let vfs = Vfs::mount(&f.manifest, &b).unwrap();
    let raw = b"C:\\Build\\Data\\meshes\\a.nif";
    for policy in [PathPolicy::Strict, PathPolicy::UnderHypotheses] {
        let mut loader = Loader::new(&store, &vfs, "english", policy, &b);
        for _ in 0..3 {
            let resolved = loader
                .assets
                .resolve_path(paths::Citation::RecordModel, raw, &b);
            if policy == PathPolicy::Strict {
                assert!(resolved.is_err());
            } else {
                let resolved = resolved.unwrap();
                assert_eq!(resolved.key, b"meshes/a.nif");
                assert_eq!(resolved.hypotheses, [hypotheses::PATH_STRIP]);
            }
            loader.reset_asset_cache(&b);
        }
    }
}

#[test]
fn stage17_data_relative_path_is_explicit_unique_and_strict_stays_rejected() {
    let f = fixture(false, false);
    let b = Budget::new(8 << 20);
    let v = Vfs::mount(&f.manifest, &b).unwrap();
    let raw = b"DaTa\\Meshes\\a.nif";
    assert!(paths::resolve_with(
        paths::Citation::RecordModel,
        raw,
        &v,
        PathPolicy::Strict,
        &b
    )
    .is_err());
    let r = paths::resolve_with(
        paths::Citation::RecordModel,
        raw,
        &v,
        PathPolicy::UnderHypotheses,
        &b,
    )
    .unwrap();
    assert_eq!(r.key, b"meshes/a.nif");
    assert_eq!(r.hypotheses, [hypotheses::PATH_DATA_RELATIVE]);
    assert_eq!(
        r.diagnostics("synthetic", 19, &b).unwrap()[0].code,
        "HypothesisAssetDataRelativeStrip"
    );
    for raw in [
        b"data/meshes/missing.nif".as_slice(),
        b"data/../meshes/a.nif",
        b"data/data/meshes/a.nif",
    ] {
        assert!(paths::resolve_with(
            paths::Citation::RecordModel,
            raw,
            &v,
            PathPolicy::UnderHypotheses,
            &b
        )
        .is_err());
    }
}
#[test]
fn stage17_shared_model_without_bit_does_not_inherit_marker_visibility() {
    let f = fixture(false, false);
    let path = f.manifest.root.join("base.esm");
    let mut bytes = fs::read(&path).unwrap();
    for (i, sig) in [b"STAT", b"ACTI", b"DOOR"].into_iter().enumerate() {
        bytes.extend(record(
            sig,
            0x900 + i as u32,
            0x0080_0000,
            &sub(b"MODL", b"a.nif\0"),
        ));
    }
    fs::write(path, bytes).unwrap();
    let b = Budget::new(16 << 20);
    let store = Store::open(&f.manifest, &b).unwrap();
    assert_eq!(store.marker_models, [b"a.nif".to_vec()]);
    let v = Vfs::mount(&f.manifest, &b).unwrap();
    let mut loader = Loader::new(&store, &v, "english", PathPolicy::Strict, &b);
    let ordinary = loader
        .inspect_base(
            &FormKey {
                plugin: "base.esm".into(),
                object_id: 0x801,
            },
            &b,
        )
        .unwrap();
    assert!(!ordinary.base.editor_marker);
    for id in 0x900..=0x902 {
        assert!(
            loader
                .inspect_base(
                    &FormKey {
                        plugin: "base.esm".into(),
                        object_id: id
                    },
                    &b
                )
                .unwrap()
                .base
                .editor_marker
        );
    }
}

#[test]
fn stage18_placed_lights_expose_data_and_keep_unknown_type_and_modifiers() {
    for flags in [0u32, 1025, 0x8008] {
        let f = fixture_with_cell(false, false, &sub(b"XCLL", &[0; 136]));
        let path = f.manifest.root.join("base.esm");
        let mut bytes = fs::read(&path).unwrap();
        let mut data = vec![0; 64];
        data[0..4].copy_from_slice(&(-1i32).to_le_bytes());
        data[4..8].copy_from_slice(&256u32.to_le_bytes());
        data[8..12].copy_from_slice(&[20, 40, 80, 177]);
        data[12..16].copy_from_slice(&flags.to_le_bytes());
        bytes.extend(record(b"LIGH", 0x901, 0, &sub(b"DATA", &data)));
        let mut rf = sub(b"NAME", &0x901u32.to_le_bytes());
        rf.extend(sub(b"DATA", &[0; 24]));
        rf.extend(sub(b"XLIG", &[0; 16]));
        bytes.extend(group(
            6,
            0x800,
            &group(9, 0x800, &record(b"REFR", 0x902, 0, &rf)),
        ));
        bytes.extend(group(
            6,
            0x800,
            &group(9, 0x800, &record(b"REFR", 0x903, fo4_plugin::DISABLED, &rf)),
        ));
        fs::write(path, bytes).unwrap();
        let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut loader = Loader::new(
            &store,
            &v,
            "english",
            PathPolicy::Strict,
            &Budget::new(8 << 20),
        );
        let scene = loader
            .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
            .unwrap();
        assert_eq!(scene.lighting_data.placed.len(), 2);
        assert_eq!(
            scene.lighting_data.placed[1].reference_flags & fo4_plugin::DISABLED,
            fo4_plugin::DISABLED
        );
        assert!(scene.disabled.iter().any(|r| r.key.object_id == 0x903));
        assert!(!scene
            .instances
            .iter()
            .any(|r| r.reference.key.object_id == 0x903));
        assert_eq!(scene.lighting_data.inactive_records.len(), 1);
        let raw = &scene.lighting_data.inactive_records[0].reference;
        let sub = raw
            .subrecords
            .iter()
            .find(|s| s.signature == *b"XLIG")
            .unwrap();
        assert_eq!(raw.data(sub), [0; 16]);
        let light = &scene.lighting_data.placed[0];
        assert_eq!(light.nominal_radius, Some(256));
        assert_eq!(light.color_rgb, Some([20, 40, 80]));
        assert_eq!(light.unused_color_byte, Some(177));
        assert_eq!(light.time, Some(-1));
        assert_eq!(light.position_rotation, Some([0.; 6]));
        assert_eq!(light.reference_scale, None);
        assert_eq!(
            light.type_declarations,
            if flags == 1025 {
                vec!["Shadow Spotlight"]
            } else {
                vec![]
            }
        );
        if flags != 1025 {
            let g = light
                .gaps
                .iter()
                .find(|g| g.code == "UndeclaredLightType")
                .unwrap();
            assert_eq!(g.payload_bytes, Some(18..22));
            assert!(!g.payload_is_decompressed);
        }
        assert!(!light.fields_completely_described);
        assert!(!scene.lighting_data.lighting_inputs_completely_described);
        assert!(light.gaps.iter().any(|g| g.signature == *b"XLIG"));
        assert_eq!(light.reference_fields.iter().filter(|f|f.field.label.contains("Fade")||f.field.label.contains("Shadow Depth")).count(),2);
        assert!(!scene.lighting_data.cell_fields.is_empty());
        assert!(crate::lighting::describe(&scene, &store, &Budget::new(0)).is_err());
    }
}
