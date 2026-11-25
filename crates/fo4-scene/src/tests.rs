use super::*;
use fo4_core::{Game, Limits, Manifest, Mount, PluginSpec, TestDir};
use std::fs;
pub(crate) fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
pub(crate) fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    for n in [data.len() as u32, flags, id, 0] {
        v.extend(n.to_le_bytes());
    }
    v.extend(131u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(data);
    v
}
pub(crate) fn group(kind: u32, id: u32, data: &[u8]) -> Vec<u8> {
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
pub(crate) struct Fixture {
    _temp: TestDir,
    pub(crate) manifest: Manifest,
}
pub(crate) fn fixture(runtime: bool, unknown: bool) -> Fixture {
    fixture_with_cell(runtime, unknown, &[])
}
fn fixture_with_cell(runtime: bool, unknown: bool, extra: &[u8]) -> Fixture {
    fixture_with(unknown, extra, |rf| {
        let mut rf = rf.to_vec();
        if runtime {
            rf.extend(sub(b"XESP", &[0; 8]));
        }
        let mut references = record(b"REFR", 0x802, 0, &rf);
        references.extend(record(b"REFR", 0x803, fo4_plugin::DELETED, &rf));
        references.extend(record(b"REFR", 0x804, fo4_plugin::DISABLED, &rf));
        references.extend(record(b"ACHR", 0x805, 0, &[]));
        group(9, 0x800, &references)
    })
}
/// A fixture whose cell children groups are `children(rf)`, `rf` being a
/// placed reference's NAME, DATA and XSCL.
pub(crate) fn fixture_with(
    unknown: bool,
    extra: &[u8],
    children: impl FnOnce(&[u8]) -> Vec<u8>,
) -> Fixture {
    let temp = TestDir::new("scene-case11");
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
    let references = children(&rf);
    let mut body = group(
        2,
        0,
        &[
            record(b"CELL", 0x800, 0, &cell),
            group(6, 0x800, &references),
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
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Facts, &cache);
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
        inst.state.ready_at(crate::RuleLevel::Hypotheses)
            && inst.state.hypotheses() == [crate::hypotheses::ROTATION]
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
        RuleLevel::Facts,
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

pub(crate) fn sized(v: &mut Vec<u8>, text: &[u8]) {
    v.extend((text.len() as u32).to_le_bytes());
    v.extend(text);
}
pub(crate) fn nif_blocks(blocks: &[(&str, Vec<u8>)], strings: &[&[u8]]) -> Vec<u8> {
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
    effect_material_slots([path, b"", b"", b"", b""])
}
/// [`effect_material`] with its five first texture slots (base, grayscale,
/// envmap, normal, envmap mask).
fn effect_material_slots(paths: [&[u8]; 5]) -> Vec<u8> {
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
    for path in paths {
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
/// A missing texture cited only from slots the frame never samples keeps
/// `MissingTextureUnreadSlot` under its pending rule; one cited from the
/// base slot stays `MissingTexture`.
#[test]
fn a_missing_texture_only_in_unsampled_slots_is_set_apart_under_its_rule() {
    for (base, unread) in [(b"a.dds".as_slice(), true), (b"absent_d.dds", false)] {
        let mut f = fixture(false, false);
        fs::create_dir(f.manifest.root.join("materials")).unwrap();
        fs::create_dir(f.manifest.root.join("textures")).unwrap();
        let model = nif_blocks(
            &[("BSEffectShaderProperty", effect_shader())],
            &[b"materials/a.bgem"],
        );
        fs::write(f.manifest.root.join("meshes/a.nif"), model).unwrap();
        fs::write(
            f.manifest.root.join("materials/a.bgem"),
            effect_material_slots([base, b"absent_g.dds", b"absent_e.dds", b"absent_n.dds", b""]),
        )
        .unwrap();
        fs::write(f.manifest.root.join("textures/a.dds"), dds()).unwrap();
        let Mount::Loose { files, .. } = &mut f.manifest.mounts[0] else {
            unreachable!()
        };
        files.extend(["materials/a.bgem".into(), "textures/a.dds".into()]);
        let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
        let mut loader = Loader::new(
            &store,
            &v,
            "english",
            RuleLevel::Facts,
            &Budget::new(8 << 20),
        );
        let b = Budget::new(8 << 20);
        let scene = loader.by_editor_id("SyntheticRoom", &b).unwrap();
        let i = &scene.instances[0];
        let codes: Vec<&str> = i
            .blockers
            .iter()
            .map(|e| e.code.as_str())
            .filter(|c| c.starts_with("MissingTexture"))
            .collect();
        let unread_ids = crate::missing::unread_textures(&loader.assets, &i.assets, &b).unwrap();
        let slots: Vec<_> = i
            .assets
            .iter()
            .flat_map(|n| loader.assets.nodes[*n].dependency_citations.iter())
            .filter_map(|c| c.slot)
            .collect();
        assert!(slots.contains(&crate::TextureSlot::Material {
            slot: "normal",
            palette: false
        }));
        if unread {
            // Grayscale (no palette flag), environment and normal maps.
            assert_eq!(codes, ["MissingTextureUnreadSlot"; 3]);
            assert_eq!(unread_ids.len(), 3);
            assert!(i
                .state
                .hypotheses()
                .contains(&crate::hypotheses::MISSING_TEXTURE_UNREAD));
        } else {
            assert_eq!(codes.iter().filter(|c| **c == "MissingTexture").count(), 1);
            assert_eq!(unread_ids.len(), 3);
            assert!(matches!(i.state, crate::State::Blocked));
        }
    }
    let sampled = |s: crate::TextureSlot| s.sampled_by_frame();
    assert!(sampled(crate::TextureSlot::TextureSet(0)));
    assert!(!sampled(crate::TextureSlot::TextureSet(1)));
    assert!(sampled(crate::TextureSlot::Effect {
        field: "Greyscale Texture",
        palette: true
    }));
    assert!(!sampled(crate::TextureSlot::Effect {
        field: "Greyscale Texture",
        palette: false
    }));
    assert!(sampled(crate::TextureSlot::Material {
        slot: "diffuse",
        palette: false
    }));
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
        RuleLevel::Facts,
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
    // Both swap files are missing and no model cites the swap's source: the
    // draw build reads neither, so neither blocks (`missing`).
    assert!(!scene.instances[0]
        .blockers
        .iter()
        .any(|e| e.code.starts_with("MissingMaterial")));
    let i = &scene.instances[0];
    let missing = crate::missing::classify(
        &loader.assets,
        &i.assets,
        &crate::missing::instance_swaps(i),
        &b,
    )
    .unwrap();
    let reads: Vec<_> = missing.iter().map(|m| m.read).collect();
    assert_eq!(
        reads,
        [
            crate::missing::Read::NotRead("swap source only"),
            crate::missing::Read::NotRead("swap target of a material no model cites")
        ]
    );
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
fn interrupted_cache_load_does_not_report_a_spurious_dependency_cycle() {
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
fn deleted_lighting_template_is_retained_but_not_certified() {
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
        RuleLevel::Facts,
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
fn cache_identity_is_not_published_before_path_allocation() {
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
fn cell_causes_are_separate_from_own_instance_readiness() {
    let f = fixture_with_cell(false, false, &sub(b"CCCC", &[1, 2, 3, 4]));
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Facts, &cache);
    let scene = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    assert!(scene.diagnostics.iter().any(|e| e.found == "CELL.CCCC"));
    assert!(scene.instances[0]
        .blockers
        .iter()
        .all(|e| e.code == "UnverifiedEulerRotation"));
    assert!(
        scene.instances[0]
            .state
            .ready_at(crate::RuleLevel::Hypotheses)
            && scene.instances[0].state.hypotheses() == [crate::hypotheses::ROTATION]
    );
}
#[test]
fn documented_audio_stays_raw_and_does_not_block_the_frame() {
    let f = fixture_with_cell(false, false, &sub(b"XCAS", &0x801u32.to_le_bytes()));
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let cache = Budget::new(8 << 20);
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Facts, &cache);
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
        scene.instances[0]
            .state
            .ready_at(crate::RuleLevel::Hypotheses)
            && scene.instances[0].state.hypotheses() == [crate::hypotheses::ROTATION]
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
fn bgsm_name_overrides_texture_set_v1_only_when_strictly_resolved() {
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
fn collision_is_raw_outside_but_animation_and_skin_still_block() {
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
fn animation_target_membership_is_known_only_for_decoded_targets() {
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
fn new_visible_types_keep_their_models_and_audio_markers_are_outside() {
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
        RuleLevel::Facts,
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
fn extra_animation_target_can_reach_visual_when_primary_does_not() {
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
fn decoded_structure_does_not_certify_an_unknown_block_role() {
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
fn misc_preview_and_keywords_stay_raw_without_blocking_its_model() {
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
        RuleLevel::Facts,
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
fn additional_controller_prefixes_keep_targets_and_opaque_bodies() {
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
            fo4_plugin::static_frame::Class::State
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
fn common_controller_prefix_truncations_remain_raw_without_panic() {
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
fn next_controller_uses_the_same_sourced_prefix() {
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
fn explicit_overlay_and_marker_visibility_keep_data_readiness_separate() {
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
    let mut l = Loader::new(&store, &v, "english", RuleLevel::Facts, &cache);
    let s = l
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    let i = &s.instances[0];
    assert!(i.editor_marker);
    assert_eq!(i.visibility_hypotheses, [crate::hypotheses::MARKER]);
    assert!(
        i.state.ready_at(crate::RuleLevel::Hypotheses)
            && i.state.hypotheses() == [crate::hypotheses::MODT, crate::hypotheses::ROTATION]
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
fn absent_scale_stays_absent_and_is_only_usable_under_its_hypothesis() {
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
    let mut l = Loader::new(&store, &v, "english", RuleLevel::Facts, &cache);
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
        i.state.ready_at(crate::RuleLevel::Hypotheses)
            && i.state.hypotheses() == [crate::hypotheses::SCALE, crate::hypotheses::ROTATION]
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
fn path_hypotheses_require_a_unique_existing_winner_and_preserve_strictness() {
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
        assert!(resolve_with(c, raw, &v, RuleLevel::Facts, &b).is_err());
        let r = resolve_with(c, raw, &v, RuleLevel::Hypotheses, &b).unwrap();
        assert_eq!(r.key, key);
        assert_eq!(r.hypotheses, hs);
        assert_eq!(
            r.original_rejection.as_ref().unwrap().code,
            "RejectedVfsPath"
        );
        let errors = r.diagnostics("synthetic", 17, &b).unwrap();
        let state = hypotheses::classify(&errors, false, &b).unwrap();
        assert!(!state.hypotheses().is_empty() && !state.ready_at(crate::RuleLevel::Facts));
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
        assert!(resolve_with(Citation::NifTexture, raw, &v, RuleLevel::Hypotheses, &b).is_err());
    }
    assert!(candidate(Citation::NifTexture, b"a.dds", &Budget::new(0)).is_err());
}

#[test]
fn numeric_marker_labels_are_a_narrow_hypothesis_not_a_filename_heuristic() {
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
    for policy in [RuleLevel::Facts, RuleLevel::Hypotheses] {
        let mut loader = Loader::new(&store, &vfs, "english", policy, &b);
        for _ in 0..3 {
            let resolved = loader
                .assets
                .resolve_path(paths::Citation::RecordModel, raw, &b);
            if policy == RuleLevel::Facts {
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
fn data_relative_path_is_explicit_unique_and_strict_stays_rejected() {
    let f = fixture(false, false);
    let b = Budget::new(8 << 20);
    let v = Vfs::mount(&f.manifest, &b).unwrap();
    let raw = b"DaTa\\Meshes\\a.nif";
    assert!(
        paths::resolve_with(paths::Citation::RecordModel, raw, &v, RuleLevel::Facts, &b).is_err()
    );
    let r = paths::resolve_with(
        paths::Citation::RecordModel,
        raw,
        &v,
        RuleLevel::Hypotheses,
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
            RuleLevel::Hypotheses,
            &b
        )
        .is_err());
    }
}
#[test]
fn shared_model_without_bit_does_not_inherit_marker_visibility() {
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
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Facts, &b);
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
fn placed_lights_expose_data_and_keep_unknown_type_and_modifiers() {
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
            RuleLevel::Facts,
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

#[test]
fn enable_parent_chains_decide_presence_only_under_their_rules() {
    use crate::hypotheses::{ENABLE_FOLLOWS, ENABLE_INITIAL, ENABLE_OPPOSITE};
    let xesp = |rf: &[u8], parent: u32, flags: u8| {
        let mut v = rf.to_vec();
        let mut d = parent.to_le_bytes().to_vec();
        d.extend([flags, 0, 0, 0]);
        v.extend(sub(b"XESP", &d));
        v
    };
    // Persistent parents: 0x810 enabled, 0x811 initially disabled.
    // Children: 0x812 of 0x810, 0x813 of 0x811, 0x814 of 0x811 opposite,
    // 0x815 of 0x813 (depth 2), 0x816 in a cycle with itself.
    let f = fixture_with(false, &[], |rf| {
        let persistent = [
            record(b"REFR", 0x810, 0, rf),
            record(b"REFR", 0x811, fo4_plugin::DISABLED, rf),
        ]
        .concat();
        let temporary = [
            record(b"REFR", 0x812, 0, &xesp(rf, 0x810, 0)),
            record(b"REFR", 0x813, 0, &xesp(rf, 0x811, 0)),
            record(b"REFR", 0x814, 0, &xesp(rf, 0x811, 1)),
            record(b"REFR", 0x815, 0, &xesp(rf, 0x813, 0)),
            record(b"REFR", 0x816, 0, &xesp(rf, 0x816, 0)),
        ]
        .concat();
        [group(8, 0x800, &persistent), group(9, 0x800, &temporary)].concat()
    });
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let mut loader = Loader::new(
        &store,
        &v,
        "english",
        RuleLevel::Hypotheses,
        &Budget::new(8 << 20),
    );
    let s = loader
        .by_editor_id("SyntheticRoom", &Budget::new(8 << 20))
        .unwrap();
    let get = |id: u32| {
        s.instances
            .iter()
            .find(|i| i.reference.key.object_id == id)
            .unwrap()
    };
    assert_eq!(s.disabled.len(), 1);
    assert!(get(0x810).enable.is_none());
    let start = |id: u32| get(id).enable.as_ref().unwrap().enabled;
    assert_eq!(start(0x812), Some(true));
    assert_eq!(start(0x813), Some(false));
    assert_eq!(start(0x814), Some(true));
    assert_eq!(start(0x815), Some(false));
    assert_eq!(start(0x816), None);
    assert!(get(0x813).disabled_by_enable_parent());
    assert!(!get(0x812).disabled_by_enable_parent());
    // Ready only under hypotheses, with each part's rule.
    let child = get(0x814);
    assert!(child.state.ready_at(RuleLevel::Hypotheses));
    assert!(!child.state.ready_at(RuleLevel::Measured));
    for rule in [ENABLE_FOLLOWS, ENABLE_INITIAL, ENABLE_OPPOSITE] {
        assert!(child.state.hypotheses().contains(&rule), "{rule}");
        assert!(child.collision.hypotheses().contains(&rule), "{rule}");
    }
    assert!(!get(0x812).state.hypotheses().contains(&ENABLE_OPPOSITE));
    // The cycle stays blocked by a text no rule covers.
    let cycle = get(0x816);
    assert!(matches!(cycle.state, State::Blocked));
    assert!(cycle
        .blockers
        .iter()
        .any(|e| e.code == "RuntimeEnableParent" && e.found == crate::enable::FOUND_CYCLE));
    // A script enabling the disabled root reaches the depth-2 child.
    let root = fo4_plugin::FormKey {
        plugin: "base.esm".into(),
        object_id: 0x811,
    };
    let p = get(0x815).enable.as_ref().unwrap();
    assert!(p.depends_on(&root));
    assert_eq!(p.with(&|k| (*k == root).then_some(true)), Some(true));
}

#[test]
fn agreeing_own_flags_decide_collision_presence_only_behind_their_switch() {
    use crate::hypotheses::ENABLE_CHILD_FLAG_AGREES;
    let xesp = |rf: &[u8], parent: u32, flags: u8| {
        let mut v = rf.to_vec();
        let mut d = parent.to_le_bytes().to_vec();
        d.extend([flags, 0, 0, 0]);
        v.extend(sub(b"XESP", &d));
        v
    };
    // Root 0x820 enabled. 0x821 carries its own flag 0x800 under the root
    // with the opposite bit: the chain starts it disabled too (agrees).
    // 0x824 carries the flag under the root without the bit (disagrees).
    // Children: 0x822 of 0x821 (disabled), 0x823 of 0x821 opposite
    // (enabled), 0x825 of 0x824 (undecided either way).
    let f = fixture_with(false, &[], |rf| {
        let persistent = [
            record(b"REFR", 0x820, 0, rf),
            record(b"REFR", 0x821, fo4_plugin::DISABLED, &xesp(rf, 0x820, 1)),
            record(b"REFR", 0x824, fo4_plugin::DISABLED, &xesp(rf, 0x820, 0)),
        ]
        .concat();
        let temporary = [
            record(b"REFR", 0x822, 0, &xesp(rf, 0x821, 0)),
            record(b"REFR", 0x823, 0, &xesp(rf, 0x821, 1)),
            record(b"REFR", 0x825, 0, &xesp(rf, 0x824, 0)),
        ]
        .concat();
        [group(8, 0x800, &persistent), group(9, 0x800, &temporary)].concat()
    });
    let store = Store::open(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let v = Vfs::mount(&f.manifest, &Budget::new(8 << 20)).unwrap();
    let b = Budget::new(64 << 20);
    let limits = Limits::default();
    let options = collision::Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let unplaced = |c: &collision::CellCollision, id: u32| {
        c.unplaced
            .iter()
            .find(|u| u.reference.object_id == id)
            .map(|u| u.code.clone())
    };
    for on in [false, true] {
        let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
        assert!(!loader.collision_child_flag_agrees());
        loader.set_collision_child_flag_agrees(on);
        let s = loader.by_editor_id("SyntheticRoom", &b).unwrap();
        let get = |id: u32| {
            s.instances
                .iter()
                .find(|i| i.reference.key.object_id == id)
                .unwrap()
        };
        let start = |id: u32| get(id).enable.as_ref().unwrap().enabled;
        let agrees = |id: u32| readiness::child_flag_agrees(get(id).enable.as_ref().unwrap());
        assert!(agrees(0x822) && agrees(0x823) && !agrees(0x825));
        for id in [0x822, 0x823, 0x825] {
            assert!(get(id).blockers.iter().any(readiness::own_flag), "{id:x}");
            assert!(!get(id).collision.ready_at(RuleLevel::Hypotheses));
        }
        let c = collision::cell(&s, &loader, &limits, options, &b).unwrap();
        // The disagreeing chain stays open and refused either way.
        assert_eq!(start(0x825), None);
        assert_eq!(unplaced(&c, 0x825).as_deref(), Some("CollisionNotReady"));
        if !on {
            // Off by default: undecided and refused, as before.
            assert_eq!((start(0x822), start(0x823)), (None, None));
            assert_eq!(unplaced(&c, 0x822).as_deref(), Some("CollisionNotReady"));
            assert_eq!(unplaced(&c, 0x823).as_deref(), Some("CollisionNotReady"));
            continue;
        }
        // On: each takes its chain's state; the disabled one is left out
        // as any chain-disabled reference, the enabled one passes
        // readiness under the rule.
        assert_eq!((start(0x822), start(0x823)), (Some(false), Some(true)));
        assert!(get(0x822).disabled_by_enable_parent());
        assert_eq!(
            unplaced(&c, 0x822).as_deref(),
            Some("DisabledThroughEnableParent")
        );
        assert_ne!(unplaced(&c, 0x823).as_deref(), Some("CollisionNotReady"));
        if let Some(p) = c.placements.iter().find(|p| p.reference.object_id == 0x823) {
            assert!(p.scene_rules.contains(&ENABLE_CHILD_FLAG_AGREES));
        }
        // Render readiness is unchanged: the blocker still stops the draw.
        assert!(!get(0x823).state.ready_at(RuleLevel::Hypotheses));
    }
    // The render switch: the same presence, and the render state takes the
    // rule for the agreeing chain's own flag; the disagreeing chain stays
    // refused, and the switch off leaves every state as it was.
    let states = |on: bool| {
        let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
        assert!(!loader.render_child_flag_agrees());
        loader.set_render_child_flag_agrees(on);
        let s = loader.by_editor_id("SyntheticRoom", &b).unwrap();
        [0x822, 0x823, 0x825].map(|id| {
            let i = s
                .instances
                .iter()
                .find(|i| i.reference.key.object_id == id)
                .unwrap();
            (
                (
                    i.state.ready_at(RuleLevel::Hypotheses),
                    i.state.hypotheses().to_vec(),
                    format!("{:?}", i.state),
                ),
                i.enable.as_ref().unwrap().enabled,
                format!("{:?}", i.collision),
            )
        })
    };
    let (off, on) = (states(false), states(true));
    for (id, (state, start, _)) in [0x822, 0x823, 0x825].iter().zip(&off) {
        assert!(!state.0, "{id:x}");
        assert_eq!(*start, None);
    }
    assert_eq!((on[0].1, on[1].1, on[2].1), (Some(false), Some(true), None));
    for k in 0..2 {
        assert!(on[k].0 .0);
        assert!(on[k].0 .1.contains(&ENABLE_CHILD_FLAG_AGREES));
        // Collision readiness is not the render switch's.
        assert_eq!(on[k].2, off[k].2);
    }
    assert_eq!(on[2], off[2]);
    // A named platform (`crate::platform`): its uncovered record blocker is
    // set aside under the platform rules; a name not placed is reported.
    let mut loader = Loader::new(&store, &v, "english", RuleLevel::Hypotheses, &b);
    let mut s = loader.by_editor_id("SyntheticRoom", &b).unwrap();
    let plugin = s.instances[0].reference.key.plugin.clone();
    let key = |id: u32| fo4_plugin::FormKey {
        plugin: plugin.clone(),
        object_id: id,
    };
    let (named, absent) = (key(0x825), key(0x999));
    let n = s.instances.len();
    let (added, actors) =
        crate::platform::add(&mut s, &mut loader, &[named.clone(), absent.clone()], &b).unwrap();
    assert_eq!(s.instances.len(), n, "already loaded: nothing put in");
    assert_eq!(added.drawn, [crate::timeline::norm(&named)]);
    assert!(added.loaded.is_empty() && added.skinned.is_empty() && actors.is_empty());
    assert_eq!(added.not.len(), 1);
    assert_eq!(added.not[0].0, crate::timeline::norm(&absent));
    let i = s
        .instances
        .iter()
        .find(|i| i.reference.key.object_id == 0x825)
        .unwrap();
    assert!(i.state.ready_at(RuleLevel::Hypotheses));
    for r in [
        crate::hypotheses::PLAY_PLATFORM_DRAWN,
        crate::hypotheses::PLAY_PLATFORM_LEFT_OUT,
    ] {
        assert!(i.state.hypotheses().contains(&r), "{r}");
    }
    // The same with the cells kept across loads (`PlacedCells`): only the
    // name the scene does not hold is looked up, once, and found nowhere;
    // a name looked up for itself is found in the room's cell.
    let mut placed = crate::platform::PlacedCells::default();
    for _ in 0..2 {
        let mut again = loader.by_editor_id("SyntheticRoom", &b).unwrap();
        let (kept, kept_actors) = crate::platform::add_with(
            &mut again,
            &mut loader,
            &[named.clone(), absent.clone()],
            &mut placed,
            &b,
        )
        .unwrap();
        assert_eq!(kept, added);
        assert!(kept_actors.is_empty());
    }
    assert!(placed.asked(&absent) && placed.cell(&absent).is_none());
    assert!(!placed.asked(&named));
    placed.find(&store, std::slice::from_ref(&named));
    assert_eq!(
        placed.cell(&named).map(crate::timeline::norm),
        Some(crate::timeline::norm(&s.cell.key))
    );
}
