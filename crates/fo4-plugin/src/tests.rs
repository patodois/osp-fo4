use super::*;
use fo4_core::{Manifest, Mount, PluginSpec};
use fo4_vfs::Vfs;
use std::io::Write;
#[test]
fn writer_preserves_payload_transport_and_explicit_compression() {
    use crate::writer::{self, Node, RecordModel, Subrecord};
    let limits = Limits::default();
    let large = vec![0x9a; 70_001];
    let fields = [
        Subrecord {
            signature: *b"EDID",
            data: b"Synthetic\0",
            extended: true,
        },
        Subrecord {
            signature: *b"ZZZZ",
            data: &large,
            extended: false,
        },
    ];
    for compress in [false, true] {
        let model = RecordModel {
            signature: *b"STAT",
            flags: COMPRESSED | DELETED,
            form_id: 0x800,
            revision: 0x01020304,
            version: 131,
            unknown: 0x55aa,
            subrecords: &fields,
            compress,
        };
        let record_bytes = writer::encode_record(&model, &limits, &Budget::new(8 << 20)).unwrap();
        let parsed = record_at(
            &SliceSource {
                name: "synthetic",
                bytes: &record_bytes,
            },
            0,
            record_bytes.len() as u64,
            &limits,
            &Budget::new(8 << 20),
        )
        .unwrap();
        assert_eq!(parsed.payload_is_decompressed, compress);
        assert_eq!(
            parsed.flags,
            DELETED | if compress { COMPRESSED } else { 0 }
        );
        assert_eq!(parsed.unknown, 0x55aa);
        assert!(parsed.subrecords.iter().all(|s| s.extended));
        assert_eq!(parsed.data(&parsed.subrecords[1]), large);
        let rewritten = writer::rewrite(&parsed, false, &limits, &Budget::new(8 << 20)).unwrap();
        let roundtrip = record_at(
            &SliceSource {
                name: "rewritten",
                bytes: &rewritten,
            },
            0,
            rewritten.len() as u64,
            &limits,
            &Budget::new(8 << 20),
        )
        .unwrap();
        assert_eq!(roundtrip.payload, parsed.payload);
        if !compress {
            assert_eq!(rewritten, record_bytes);
        }
        let children = [Node::Record(model)];
        let grouped = writer::encode(
            &[Node::Group {
                label: *b"STAT",
                kind: 0,
                raw_tail: [0x77; 8],
                children: &children,
            }],
            &limits,
            &Budget::new(8 << 20),
        )
        .unwrap();
        let bytes = plugin(0, &["Base.esm"], 1, &grouped);
        let h = header("Own.esp", &bytes).unwrap();
        assert_eq!(h.masters, ["Base.esm"]);
        let mut seen = 0;
        scan(
            &SliceSource {
                name: "Own.esp",
                bytes: &bytes,
            },
            &h,
            &limits,
            |r, groups| {
                assert_eq!(r.payload, parsed.payload);
                assert_eq!(groups[0].raw_tail, [0x77; 8]);
                seen += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, 1);
    }
    let forbidden = [Subrecord {
        signature: *b"XXXX",
        data: &[],
        extended: false,
    }];
    let model = RecordModel {
        signature: *b"STAT",
        flags: 0,
        form_id: 0x800,
        revision: 0,
        version: 131,
        unknown: 0,
        subrecords: &forbidden,
        compress: false,
    };
    assert_eq!(
        writer::encode_record(&model, &limits, &Budget::new(8 << 20))
            .unwrap_err()
            .code,
        "ReservedSubrecordSignature"
    );
    let fields = [Subrecord {
        signature: *b"ZZZZ",
        data: &large,
        extended: false,
    }];
    let model = RecordModel {
        subrecords: &fields,
        ..model
    };
    assert!(writer::encode_record(&model, &limits, &Budget::new(64)).is_err());
    let short = Limits {
        max_payload: 1024,
        ..limits
    };
    assert!(writer::encode_record(&model, &short, &Budget::new(8 << 20)).is_err());
    let node = [Node::Record(model)];
    let groups = [Node::Group {
        label: *b"STAT",
        kind: 0,
        raw_tail: [0; 8],
        children: &node,
    }];
    assert!(writer::encode(
        &groups,
        &Limits {
            max_records: 1,
            ..Limits::default()
        },
        &Budget::new(8 << 20)
    )
    .is_err());
    assert!(writer::encode(
        &groups,
        &Limits {
            max_depth: 0,
            ..Limits::default()
        },
        &Budget::new(8 << 20)
    )
    .is_err());
}
#[test]
fn writer_tes4_master_order_and_raw_master_data_round_trip() {
    let masters = [
        writer::Master {
            name: "First.esm",
            data: [0x12; 8],
        },
        writer::Master {
            name: "Second.esm",
            data: [0x34; 8],
        },
    ];
    let model = writer::HeaderModel {
        flags: LOCALIZED | LIGHT,
        revision: 123,
        version: 131,
        unknown: 7,
        hedr_version: 0.95,
        declared_records: 0,
        next_object_id: 0x901,
        masters: &masters,
        others: &[],
    };
    let bytes = writer::header(&model, &Limits::default(), &Budget::new(1 << 20)).unwrap();
    let parsed = header("Own.esl", &bytes).unwrap();
    assert_eq!(parsed.masters, ["First.esm", "Second.esm"]);
    assert_eq!(parsed.master_data, [[0x12; 8], [0x34; 8]]);
    assert_eq!(parsed.flags, LOCALIZED | LIGHT);
    assert_eq!(parsed.next_object_id, 0x901);
    let bad = [writer::Master {
        name: "bad\0name",
        data: [0; 8],
    }];
    assert_eq!(
        writer::header(
            &writer::HeaderModel {
                masters: &bad,
                ..model
            },
            &Limits::default(),
            &Budget::new(1 << 20)
        )
        .unwrap_err()
        .code,
        "WriterMasterName"
    );
}
fn u16b(v: &mut Vec<u8>, n: u16) {
    v.extend(n.to_le_bytes());
}
fn u32b(v: &mut Vec<u8>, n: u32) {
    v.extend(n.to_le_bytes());
}
fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    u16b(&mut v, data.len() as u16);
    v.extend(data);
    v
}
fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut out = sig.to_vec();
    let data = if flags & COMPRESSED != 0 {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(data).unwrap();
        let mut compressed = (data.len() as u32).to_le_bytes().to_vec();
        compressed.extend(z.finish().unwrap());
        compressed
    } else {
        data.to_vec()
    };
    u32b(&mut out, data.len() as u32);
    u32b(&mut out, flags);
    u32b(&mut out, id);
    u32b(&mut out, 0x01020304);
    u16b(&mut out, 131);
    u16b(&mut out, 0);
    out.extend(data);
    out
}
fn group(kind: u32, label: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    u32b(&mut out, (24 + data.len()) as u32);
    out.extend(label);
    u32b(&mut out, kind);
    out.extend([0; 8]);
    out.extend(data);
    out
}
fn plugin(flags: u32, masters: &[&str], count: u32, body: &[u8]) -> Vec<u8> {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    u32b(&mut hedr, count);
    u32b(&mut hedr, 0x900);
    let mut data = sub(b"HEDR", &hedr);
    for m in masters {
        let mut name = m.as_bytes().to_vec();
        name.push(0);
        data.extend(sub(b"MAST", &name));
        data.extend(sub(b"DATA", &[0; 8]));
    }
    let mut out = record(b"TES4", 0, flags, &data);
    out.extend(body);
    out
}
fn header(name: &str, bytes: &[u8]) -> Result<Header> {
    Header::read(
        name,
        &SliceSource { name, bytes },
        &Limits::default(),
        &Budget::new(1 << 20),
    )
}
fn inspect(bytes: &[u8]) -> Result<ScanStats> {
    let h = header("synthetic.esp", bytes)?;
    scan(
        &SliceSource {
            name: "synthetic.esp",
            bytes,
        },
        &h,
        &Limits::default(),
        |r, _| {
            r.fields(&Limits::default(), &Budget::new(1 << 20))?;
            Ok(())
        },
    )
}
#[test]
fn tes4_mast_flags_and_raw_reserved() {
    let bytes = plugin(MASTER | LOCALIZED | LIGHT, &["base.esm"], 0, &[]);
    let h = header("small.esl", &bytes).unwrap();
    assert_eq!(h.masters, vec!["base.esm"]);
    assert_eq!(h.master_data, vec![[0; 8]]);
    assert_eq!(h.flags, 0x281);
    assert_eq!(h.hedr_version, 0.95);
    assert_eq!(h.next_object_id, 0x900);
}
#[test]
fn nested_cells_persistent_temporary_compressed_and_xxxx() {
    let mut transform = Vec::new();
    for n in [1., 2., 3., 0.2, 0.4, 0.6] {
        transform.extend(f32::to_le_bytes(n));
    }
    let mut payload = sub(b"NAME", &0x801u32.to_le_bytes());
    payload.extend(sub(b"DATA", &transform));
    let refr = record(b"REFR", 0x802, COMPRESSED | DISABLED, &payload);
    let mut extended = sub(b"XXXX", &70000u32.to_le_bytes());
    extended.extend(b"ZZZZ");
    u16b(&mut extended, 0);
    extended.extend(vec![0x75; 70000]);
    let unknown = record(b"QUST", 0x803, 0, &extended);
    let mut children = group(8, 0x800u32.to_le_bytes(), &refr);
    children.extend(group(9, 0x800u32.to_le_bytes(), &unknown));
    let body = group(0, *b"CELL", &group(6, 0x800u32.to_le_bytes(), &children));
    let bytes = plugin(MASTER, &[], 6, &body);
    let h = header("A.esm", &bytes).unwrap();
    let mut parents = Vec::new();
    let stats = scan(
        &SliceSource {
            name: "A.esm",
            bytes: &bytes,
        },
        &h,
        &Limits::default(),
        |r, groups| {
            parents.push(groups.last().unwrap().kind);
            if r.signature == *b"REFR" {
                assert!(r.disabled());
                assert!(!r.deleted());
                let f = r.fields(&Limits::default(), &Budget::new(1 << 20))?;
                assert!(matches!(f[0].value, Value::FormId(0x801)));
                assert!(matches!(f[1].value, Value::Transform(_)));
            } else {
                assert_eq!(r.subrecords[0].data.len(), 70000);
                assert!(r.subrecords[0].extended);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(parents, vec![8, 9]);
    assert_eq!(stats.groups, 4);
    assert_eq!(stats.records, 2);
    assert_eq!(stats.envelope_records, 1);
    assert_eq!(stats.compressed, 1);
}
#[test]
fn three_plugins_master_remap_override_deleted_no_field_merge() {
    let a = plugin(
        MASTER,
        &[],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"MODL", b"a.nif\0")),
    );
    let b = plugin(
        0,
        &["A.esm"],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"EDID", b"override\0")),
    );
    let c = plugin(
        0,
        &["A.esm", "B.esp"],
        1,
        &record(b"STAT", 0x800, DELETED, &[]),
    );
    let order = LoadOrder::new(
        vec![
            header("A.esm", &a).unwrap(),
            header("B.esp", &b).unwrap(),
            header("C.esp", &c).unwrap(),
        ],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let key = FormKey {
        plugin: "a.esm".into(),
        object_id: 0x800,
    };
    assert_eq!(
        order.remap(2, 0x00000800, "C", 0).unwrap(),
        Some(key.clone())
    );
    assert_eq!(
        order.remap(2, 0x01000800, "C", 0).unwrap().unwrap().plugin,
        "b.esp"
    );
    assert_eq!(
        order.remap(2, 0x02000800, "C", 0).unwrap().unwrap().plugin,
        "c.esp"
    );
    let mut overrides = Overrides::new();
    for (i, data) in [&a, &b, &c].into_iter().enumerate() {
        scan(
            &SliceSource {
                name: "plugin",
                bytes: data,
            },
            &order.headers[i],
            &Limits::default(),
            |r, _| {
                if i == 1 {
                    assert!(r.subrecords.iter().all(|s| s.signature != *b"MODL"));
                }
                overrides.insert(&order, i, &r, &Budget::new(1 << 20))
            },
        )
        .unwrap();
    }
    let winner = overrides.winner(&key).unwrap();
    assert_eq!(winner.plugin, 2);
    assert!(winner.deleted);
    assert!(!winner.disabled);
    assert_eq!(winner.previous, Some(1));
    assert_eq!(order.runtime_id(&key).unwrap(), 0x800);
}
#[test]
fn light_slots_and_light_masters_use_local_indices() {
    let base = plugin(MASTER, &[], 0, &[]);
    let light = plugin(MASTER | LIGHT, &["Base.esm"], 0, &[]);
    let modp = plugin(0, &["Base.esm", "Small.esl"], 0, &[]);
    let order = LoadOrder::new(
        vec![
            header("Base.esm", &base).unwrap(),
            header("Small.esl", &light).unwrap(),
            header("Mod.esp", &modp).unwrap(),
        ],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let key = order.remap(2, 0x01000801, "Mod", 0).unwrap().unwrap();
    assert_eq!(key.plugin, "small.esl");
    assert_eq!(order.runtime_id(&key).unwrap(), 0xfe000801);
    assert_eq!(
        order
            .runtime_id(&order.remap(2, 0x02000800, "Mod", 0).unwrap().unwrap())
            .unwrap(),
        0x01000800
    );
    for id in [0x01000001, 0x01001000, 0xfe000801, 0x03000800] {
        assert_eq!(
            order.remap(2, id, "Mod", 4).unwrap_err().code,
            "InvalidFormId"
        );
    }
    assert!(order.remap(2, 0, "Mod", 4).unwrap().is_none());
    let extension_master = header("MissingFlag.esl", &base).unwrap();
    assert!(extension_master.is_master());
    assert!(extension_master.is_light());
    assert!(extension_master.diagnostics.is_empty());
}
#[test]
fn missing_tardy_self_master_and_flag_errors() {
    let a = plugin(0, &["B.esm"], 0, &[]);
    let b = plugin(MASTER, &[], 0, &[]);
    assert_eq!(
        LoadOrder::new(vec![header("A.esp", &a).unwrap()], &Budget::new(1 << 20))
            .err()
            .unwrap()
            .code,
        "MissingMaster"
    );
    assert_eq!(
        LoadOrder::new(
            vec![header("A.esp", &a).unwrap(), header("B.esm", &b).unwrap()],
            &Budget::new(1 << 20)
        )
        .err()
        .unwrap()
        .code,
        "MasterAfterDependent"
    );
    let selfp = plugin(MASTER, &["A.esm"], 0, &[]);
    assert_eq!(
        LoadOrder::new(
            vec![header("A.esm", &selfp).unwrap()],
            &Budget::new(1 << 20)
        )
        .err()
        .unwrap()
        .code,
        "CyclicMaster"
    );
    assert_eq!(
        header("update.esp", &plugin(0x100000, &[], 0, &[]))
            .unwrap_err()
            .code,
        "UnsupportedPluginFlags"
    );
}
#[test]
fn truncate_every_byte_of_headers_groups_records_subrecords() {
    let rec = record(
        b"REFR",
        0x800,
        COMPRESSED,
        &sub(b"NAME", &0x801u32.to_le_bytes()),
    );
    let bytes = plugin(0, &["Base.esm"], 2, &group(0, *b"REFR", &rec));
    for n in 0..bytes.len() {
        let result = inspect(&bytes[..n]);
        if n == header("x.esp", &bytes).unwrap().end as usize {
            assert!(!result.unwrap().count_matches);
        } else {
            assert!(result.is_err(), "prefix {n}");
        }
    }
}
#[test]
fn mutate_each_header_and_table_byte_cannot_panic_or_exceed_budget() {
    let bytes = plugin(
        0,
        &["Base.esm"],
        2,
        &group(
            0,
            *b"REFR",
            &record(b"REFR", 0x800, 0, &sub(b"NAME", &0x801u32.to_le_bytes())),
        ),
    );
    for i in 0..bytes.len() {
        for value in [0, 0x7f, 0xff] {
            let mut bad = bytes.clone();
            bad[i] = value;
            let b = Budget::new(32768);
            let l = Limits {
                max_payload: 1024,
                max_records: 32,
                max_subrecords: 32,
                ..Limits::default()
            };
            let src = SliceSource {
                name: "mutant",
                bytes: &bad,
            };
            if let Ok(h) = Header::read("mutant.esp", &src, &l, &b) {
                let _ = scan(&src, &h, &l, |r, _| {
                    r.fields(&l, &Budget::new(32768))?;
                    Ok(())
                });
            }
            assert!(b.used() <= 32768);
        }
    }
}
#[test]
fn explicit_depth_count_xxxx_and_compression_failures() {
    let mut body = record(b"STAT", 0x800, 0, &[]);
    for _ in 0..5 {
        body = group(0, *b"STAT", &body);
    }
    let bytes = plugin(0, &[], 6, &body);
    let h = header("a", &bytes).unwrap();
    assert_eq!(
        scan(
            &SliceSource {
                name: "a",
                bytes: &bytes
            },
            &h,
            &Limits {
                max_depth: 4,
                ..Limits::default()
            },
            |_, _| Ok(())
        )
        .unwrap_err()
        .code,
        "LimitExceeded"
    );
    let bad = sub(b"XXXX", &100u32.to_le_bytes());
    assert_eq!(
        inspect(&plugin(0, &[], 1, &record(b"STAT", 0x800, 0, &bad)))
            .unwrap_err()
            .code,
        "DanglingXxxx"
    );
    let mut compressed = record(b"STAT", 0x800, COMPRESSED, &sub(b"EDID", b"x\0"));
    compressed[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        inspect(&plugin(0, &[], 1, &compressed)).unwrap_err().code,
        "DecompressedSizeMismatch"
    );
}
fn table(kind: StringKind) -> Vec<u8> {
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (id, text) in [(17, b"First\0".as_slice()), (91, b"Second\0")] {
        u32b(&mut directory, id);
        u32b(&mut directory, data.len() as u32);
        if kind != StringKind::Strings {
            u32b(&mut data, text.len() as u32);
        }
        data.extend(text);
    }
    let mut out = 2u32.to_le_bytes().to_vec();
    u32b(&mut out, data.len() as u32);
    out.extend(directory);
    out.extend(data);
    out
}
#[test]
fn three_string_formats_every_prefix_and_directory_byte() {
    for kind in [
        StringKind::Strings,
        StringKind::DlStrings,
        StringKind::IlStrings,
    ] {
        let bytes = table(kind);
        let t = StringTable::read(
            "table",
            kind,
            bytes.clone(),
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap();
        assert_eq!(t.get(17, &Limits::default()).unwrap(), "First");
        assert_eq!(t.get(91, &Limits::default()).unwrap(), "Second");
        assert_eq!(
            t.get(4, &Limits::default()).unwrap_err().code,
            "MissingStringId"
        );
        for n in 0..bytes.len() {
            assert!(StringTable::read(
                "table",
                kind,
                bytes[..n].to_vec(),
                &Limits::default(),
                &Budget::new(4096)
            )
            .is_err());
        }
        for pos in 0..bytes.len() {
            for val in [0, 0xff] {
                let mut bad = bytes.clone();
                bad[pos] = val;
                let b = Budget::new(4096);
                let _ = StringTable::read(
                    "table",
                    kind,
                    bad,
                    &Limits {
                        max_entries: 16,
                        max_payload: 1024,
                        ..Limits::default()
                    },
                    &b,
                );
                assert!(b.used() <= 4096);
            }
        }
    }
}
#[test]
fn localized_flag_never_guessed_from_four_byte_text() {
    let bytes = plugin(
        0,
        &[],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"FULL", b"ABC\0")),
    );
    let h = header("a", &bytes).unwrap();
    scan(
        &SliceSource {
            name: "a",
            bytes: &bytes,
        },
        &h,
        &Limits::default(),
        |r, _| {
            assert_eq!(
                r.full_name(false, &Limits::default(), &Budget::new(4096))?,
                Some(Value::Text("ABC".into()))
            );
            assert!(matches!(
                r.full_name(true, &Limits::default(), &Budget::new(4096))?,
                Some(Value::Localized(_))
            ));
            Ok(())
        },
    )
    .unwrap();
}
fn temp() -> fo4_core::TestDir {
    let p = fo4_core::TestDir::new("plugin");
    std::fs::create_dir_all(p.join("Strings")).unwrap();
    p
}
#[test]
fn localized_tables_resolved_through_vfs_without_language_fallback() {
    let root = temp();
    std::fs::write(root.join("A.esm"), plugin(MASTER, &[], 0, &[])).unwrap();
    let mut files = Vec::new();
    for kind in [
        StringKind::Strings,
        StringKind::DlStrings,
        StringKind::IlStrings,
    ] {
        let file = format!("Strings/A_english.{}", kind.extension());
        std::fs::write(root.join(&file), table(kind)).unwrap();
        files.push(file);
    }
    let m = Manifest {
        game: fo4_core::Game::Fallout4,
        archive_invalidation: Some(fo4_core::ArchiveInvalidation {
            enabled: true,
            origin: fo4_core::InvalidationOrigin::ExplicitProfile,
        }),
        root: root.to_path_buf(),
        language: "english".into(),
        plugins: vec![PluginSpec {
            name: "A.esm".into(),
            path: "A.esm".into(),
        }],
        mounts: vec![Mount::Loose {
            base: None,
            files,
            plugin: None,
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: None,
    };
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    for kind in [
        StringKind::Strings,
        StringKind::DlStrings,
        StringKind::IlStrings,
    ] {
        assert_eq!(
            resolve_string(&v, "A.esm", "english", kind, 91, &Budget::new(1 << 20)).unwrap(),
            "Second"
        );
    }
    assert_eq!(
        resolve_string(
            &v,
            "A.esm",
            "french",
            StringKind::Strings,
            17,
            &Budget::new(1 << 20)
        )
        .unwrap_err()
        .code,
        "MissingStringTable"
    );
}
#[test]
fn all_interpreted_types_offer_partial_fields_unknown_records_stay_raw() {
    for ty in INTERPRETED_TYPES {
        let bytes = plugin(
            0,
            &[],
            1,
            &record(&ty, 0x800, 0, &sub(b"EDID", b"synthetic\0")),
        );
        inspect(&bytes).unwrap();
    }
    let unknown = plugin(
        0,
        &[],
        1,
        &record(b"QUST", 0x800, 0, &sub(b"EDID", b"raw\0")),
    );
    let h = header("a", &unknown).unwrap();
    scan(
        &SliceSource {
            name: "a",
            bytes: &unknown,
        },
        &h,
        &Limits::default(),
        |r, _| {
            assert!(matches!(
                r.fields(&Limits::default(), &Budget::new(4096))?[0].value,
                Value::Raw
            ));
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn two_plugin_cycle_and_aggregate_record_allocations_are_rejected() {
    let a = plugin(0, &["B.esp"], 0, &[]);
    let b = plugin(0, &["A.esp"], 0, &[]);
    assert_eq!(
        LoadOrder::new(
            vec![header("A.esp", &a).unwrap(), header("B.esp", &b).unwrap()],
            &Budget::new(1 << 20)
        )
        .err()
        .unwrap()
        .code,
        "CyclicMaster"
    );
    let bytes = plugin(
        0,
        &[],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"EDID", b"tiny\0")),
    );
    let h = header("a", &bytes).unwrap();
    let budget = Budget::new(1024);
    budget.charge(1000, "retained", 0).unwrap();
    assert_eq!(
        scan_bounded(
            &SliceSource {
                name: "a",
                bytes: &bytes
            },
            &h,
            &Limits::default(),
            &budget,
            |_, _, _| Ok(())
        )
        .unwrap_err()
        .code,
        "LimitExceeded"
    );
    assert_eq!(budget.used(), 1000);
}

#[test]
fn interpreted_typed_fields_and_uninterpreted_lighting_tail() {
    let mut transform = Vec::new();
    for f in [1., -2., 3., 0.25, 0.5, -0.75, 1.5] {
        transform.extend(f32::to_le_bytes(f));
    }
    let mut lighting = vec![0x35; 12];
    lighting.extend(3.5f32.to_le_bytes());
    lighting.extend(7.25f32.to_le_bytes());
    lighting.extend((-123i32).to_le_bytes());
    lighting.extend(456i32.to_le_bytes());
    for f in [0.75f32, 200., 2.] {
        lighting.extend(f.to_le_bytes());
    }
    lighting.extend([0x91; 8]);
    let mut scol = sub(b"ONAM", &0x801u32.to_le_bytes());
    scol.extend(sub(b"DATA", &transform));
    let mut txst = sub(b"TX00", b"textures/a.dds\0");
    txst.extend(sub(b"DNAM", &3u16.to_le_bytes()));
    let mut mswp = sub(b"BNAM", b"materials/old.bgsm\0");
    mswp.extend(sub(b"SNAM", b"materials/new.bgsm\0"));
    mswp.extend(sub(b"CNAM", &2.5f32.to_le_bytes()));
    for (ty, payload) in [
        (*b"LGTM", sub(b"DATA", &lighting)),
        (*b"SCOL", scol),
        (*b"TXST", txst),
        (*b"MSWP", mswp),
    ] {
        let bytes = plugin(0, &[], 1, &record(&ty, 0x800, 0, &payload));
        let h = header("a", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "a",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                let fields = r.fields(&Limits::default(), &Budget::new(1 << 20))?;
                match &fields[0].value {
                    Value::LightingPrefix {
                        fog_near,
                        rotation,
                        uninterpreted_tail,
                        ..
                    } => {
                        assert_eq!(*fog_near, 3.5);
                        assert_eq!(*rotation, [-123, 456]);
                        assert_eq!(&r.payload[uninterpreted_tail.clone()], &[0x91; 8]);
                    }
                    Value::FormId(id) => assert_eq!(*id, 0x801),
                    Value::Text(t) => assert!(!t.is_empty()),
                    _ => panic!("unexpected typed field"),
                };
                Ok(())
            },
        )
        .unwrap();
    }
}

#[test]
fn flags_and_model_subrecords_are_interpreted_in_context() {
    for (ty, disabled) in [(*b"STAT", false), (*b"REFR", true)] {
        let bytes = plugin(0, &[], 1, &record(&ty, 0x800, DISABLED, &[]));
        let h = header("a", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "a",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                assert_eq!(r.disabled(), disabled);
                Ok(())
            },
        )
        .unwrap();
    }
    let bytes = plugin(
        0,
        &[],
        1,
        &record(b"CELL", 0x800, 0, &sub(b"MODL", b"unexpected\0")),
    );
    let h = header("a", &bytes).unwrap();
    scan(
        &SliceSource {
            name: "a",
            bytes: &bytes,
        },
        &h,
        &Limits::default(),
        |r, _| {
            assert!(matches!(
                r.fields(&Limits::default(), &Budget::new(1 << 20))?[0].value,
                Value::Raw
            ));
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn light_range_follows_hedr_version() {
    let mut bytes = plugin(MASTER | LIGHT, &[], 0, &[]);
    bytes[30..34].copy_from_slice(&1f32.to_le_bytes());
    let order = LoadOrder::new(
        vec![header("new.esl", &bytes).unwrap()],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let key = order.remap(0, 1, "new.esl", 0).unwrap().unwrap();
    assert_eq!(order.runtime_id(&key).unwrap(), 0xfe000001);
    assert!(order.remap(0, 0x1000, "new.esl", 0).is_err());
}
#[test]
fn bad_text_does_not_discard_good_string() {
    let mut bytes = table(StringKind::Strings);
    *bytes.last_mut().unwrap() = 0xff;
    let t = StringTable::read(
        "mixed",
        StringKind::Strings,
        bytes,
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap();
    assert_eq!(t.get(17, &Limits::default()).unwrap(), "First");
    assert!(t.get(91, &Limits::default()).is_err());
}
#[test]
fn injected_record_and_later_override() {
    let a = plugin(MASTER, &[], 0, &[]);
    let b = plugin(MASTER, &["A.esm"], 1, &record(b"STAT", 0x900, 0, &[]));
    let c = plugin(
        0,
        &["A.esm", "B.esm"],
        1,
        &record(b"STAT", 0x900, DELETED, &[]),
    );
    let order = LoadOrder::new(
        vec![
            header("A.esm", &a).unwrap(),
            header("B.esm", &b).unwrap(),
            header("C.esp", &c).unwrap(),
        ],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let mut index = Overrides::new();
    scan(
        &SliceSource {
            name: "A.esm",
            bytes: &a,
        },
        &order.headers[0],
        &Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    index
        .mark_complete(0, &order, &Budget::new(1 << 20))
        .unwrap();
    for (i, bytes) in [(1, &b), (2, &c)] {
        scan(
            &SliceSource {
                name: "injected",
                bytes,
            },
            &order.headers[i],
            &Limits::default(),
            |r, _| index.insert(&order, i, &r, &Budget::new(1 << 20)),
        )
        .unwrap();
    }
    assert!(index.history.iter().all(|entry| entry.injected));
    assert!(
        index
            .winner(&FormKey {
                plugin: "a.esm".into(),
                object_id: 0x900
            })
            .unwrap()
            .deleted
    );
}
#[test]
fn hedr_mismatch_is_not_structural() {
    let bytes = plugin(MASTER, &[], 9, &record(b"STAT", 0x800, 0, &[]));
    let h = header("A.esm", &bytes).unwrap();
    let stats = scan(
        &SliceSource {
            name: "A.esm",
            bytes: &bytes,
        },
        &h,
        &Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(stats.records, 1);
}
#[test]
fn nonmaster_before_master_is_named_error() {
    let a = plugin(0, &[], 0, &[]);
    let b = plugin(MASTER, &[], 0, &[]);
    let result = LoadOrder::new(
        vec![header("A.esp", &a).unwrap(), header("B.esm", &b).unwrap()],
        &Budget::new(1 << 20),
    );
    assert_eq!(result.err().unwrap().code, "NonMasterBeforeMaster");
}
#[test]
fn undocumented_light_size_remains_raw() {
    for size in [0, 40, 48, 52, 56, 60, 64, 65] {
        let bytes = plugin(
            MASTER,
            &[],
            1,
            &record(b"LIGH", 0x800, 0, &sub(b"DATA", &vec![0; size])),
        );
        let h = header("A.esm", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "A.esm",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                let fields = r.fields(&Limits::default(), &Budget::new(1 << 20))?;
                assert_eq!(
                    matches!(fields[0].value, Value::Raw),
                    !matches!(size, 48 | 60 | 64)
                );
                Ok(())
            },
        )
        .unwrap();
    }
}
#[test]
fn extension_policy_is_not_a_header_rejection() {
    assert!(header("flagless.esl", &plugin(0, &[], 0, &[])).is_ok());
}
#[test]
fn more_than_253_masters_keeps_header() {
    let names: Vec<String> = (0..254).map(|i| format!("M{i}.esm")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        header("many.esp", &plugin(0, &refs, 0, &[]))
            .unwrap()
            .masters
            .len(),
        254
    );
}
#[test]
fn reference_envelopes_count_disabled() {
    for ty in [
        *b"ACHR", *b"PGRE", *b"PMIS", *b"PHZD", *b"PARW", *b"PBAR", *b"PBEA", *b"PCON", *b"PFLA",
    ] {
        let bytes = plugin(MASTER, &[], 1, &record(&ty, 0x800, DISABLED, &[]));
        let h = header("A.esm", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "A.esm",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                assert!(r.disabled(), "{ty:?}");
                Ok(())
            },
        )
        .unwrap();
    }
}

#[test]
fn cache_reads_once_per_plugin_language_and_kind() {
    let root = temp();
    std::fs::write(root.join("A.esm"), plugin(MASTER, &[], 0, &[])).unwrap();
    let file = "Strings/A_english.STRINGS";
    std::fs::write(root.join(file), table(StringKind::Strings)).unwrap();
    let m = Manifest {
        game: fo4_core::Game::Fallout4,
        archive_invalidation: Some(fo4_core::ArchiveInvalidation {
            enabled: true,
            origin: fo4_core::InvalidationOrigin::ExplicitProfile,
        }),
        root: root.to_path_buf(),
        language: "english".into(),
        plugins: vec![PluginSpec {
            name: "A.esm".into(),
            path: "A.esm".into(),
        }],
        mounts: vec![Mount::Loose {
            base: None,
            files: vec![file.into()],
            plugin: None,
        }],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: None,
    };
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    let b = Budget::new(1 << 20);
    let mut cache = StringCache::new(&b);
    assert_eq!(
        cache
            .resolve(&v, "A.esm", "english", StringKind::Strings, 17)
            .unwrap(),
        "First"
    );
    std::fs::write(root.join(file), b"now truncated").unwrap();
    for _ in 0..1000 {
        assert_eq!(
            cache
                .resolve(&v, "A.esm", "english", StringKind::Strings, 91)
                .unwrap(),
            "Second"
        );
    }
    assert_eq!(cache.loads, 1);
    assert!(cache
        .resolve(&v, "A.esm", "unknown-language", StringKind::Strings, 17)
        .is_err());
    assert_eq!(cache.loads, 2);
}
#[test]
fn high_bytes_preserve_raw_id_and_diagnostic() {
    let mut bytes = table(StringKind::Strings);
    let offset = bytes.len() - 4;
    bytes[offset] = 0xe9;
    let table = StringTable::read(
        "mixed",
        StringKind::Strings,
        bytes.clone(),
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap();
    assert_eq!(table.get(17, &Limits::default()).unwrap(), "First");
    assert_eq!(
        table.get(91, &Limits::default()).unwrap_err().code,
        "InvalidStringEncoding"
    );
    assert_eq!(table.bytes, bytes);
    assert_eq!(table.diagnostics[0].id, 91);
    assert_eq!(table.raw(91).unwrap(), &bytes[table.entries[1].1.clone()]);
}

#[test]
fn winner_reload_never_merges_absent_fields() {
    let a = plugin(
        MASTER,
        &[],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"MODL", b"base-only.nif\0")),
    );
    let b = plugin(
        0,
        &["A.esm"],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"EDID", b"winning-editor-id\0")),
    );
    let order = LoadOrder::new(
        vec![header("A.esm", &a).unwrap(), header("B.esp", &b).unwrap()],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let mut index = Overrides::new();
    for (i, bytes) in [&a, &b].into_iter().enumerate() {
        scan(
            &SliceSource {
                name: "records",
                bytes,
            },
            &order.headers[i],
            &Limits::default(),
            |r, _| index.insert(&order, i, &r, &Budget::new(1 << 20)),
        )
        .unwrap();
    }
    let key = FormKey {
        plugin: "a.esm".into(),
        object_id: 0x800,
    };
    assert_eq!(index.winner(&key).unwrap().plugin, 1);
    let winner = index
        .read_winner(
            &key,
            &order,
            &[
                &SliceSource {
                    name: "A.esm",
                    bytes: &a,
                },
                &SliceSource {
                    name: "B.esp",
                    bytes: &b,
                },
            ],
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap()
        .unwrap();
    assert_eq!(winner.payload, sub(b"EDID", b"winning-editor-id\0"));
    assert!(!winner.subrecords.iter().any(|s| s.signature == *b"MODL"));
}
#[test]
#[ignore = "explicit synthetic scale measurement"]
fn measure_override_index() {
    let bytes = plugin(MASTER, &[], 0, &[]);
    let order = LoadOrder::new(
        vec![header("A.esm", &bytes).unwrap()],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let raw = record(b"REFR", 0x800, 0, &[]);
    let mut r = record_at(
        &SliceSource {
            name: "A.esm",
            bytes: &raw,
        },
        0,
        raw.len() as u64,
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap();
    for n in [100_000, 900_000, 1_500_000] {
        let budget = Budget::new(512 << 20);
        let mut index = Overrides::new();
        let start = std::time::Instant::now();
        for id in 0..n {
            r.form_id = 0x800 + id;
            index.insert(&order, 0, &r, &budget).unwrap();
        }
        let (capacity, winners) = index.capacities();
        println!("OVERRIDE n={n} struct={} budget={} bytes_per_record={:.3} records_per_decimal_GB={:.0} history_capacity={capacity} winners_capacity={winners} ms={}",std::mem::size_of::<Override>(),budget.used(),budget.used() as f64/n as f64,1e9*n as f64/budget.used() as f64,start.elapsed().as_millis());
        assert!(budget.used() / (n as u64) < 200);
    }
    let budget = Budget::new(1_000_000_000);
    let mut index = Overrides::new();
    for id in 0..12_000_000 {
        r.form_id = 0x800 + id;
        match index.insert(&order, 0, &r, &budget) {
            Ok(()) => {}
            Err(error) => {
                assert_eq!(error.code, "LimitExceeded");
                println!(
                    "OVERRIDE_CAP decimal_GB=1 records={} charged={} bytes_per_record={:.3}",
                    index.history.len(),
                    budget.used(),
                    budget.used() as f64 / index.history.len() as f64
                );
                assert!(index.history.len() > 4_000_000);
                return;
            }
        }
    }
    panic!("expected the 1 GB allocation ceiling");
}

#[test]
fn string_diagnostic_raw_is_at_most_64_bytes() {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend(100_000u32.to_le_bytes());
    bytes.extend(17u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(vec![b'x'; 100_000]);
    let t = StringTable::read(
        "hostile",
        StringKind::Strings,
        bytes,
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap();
    assert!(t.diagnostics[0].raw.len() <= 64);
}
#[test]
fn valid_utf8_is_decoded() {
    let text = "Café — 日本語\0".as_bytes();
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend((text.len() as u32).to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(text);
    let t = StringTable::read(
        "Base_fr.STRINGS",
        StringKind::Strings,
        bytes,
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap();
    assert_eq!(t.get(1, &Limits::default()).unwrap(), "Café — 日本語");
}
#[test]
fn unscanned_base_is_not_an_injection() {
    let a = plugin(MASTER, &[], 0, &[]);
    let b = plugin(MASTER, &["A.esm"], 1, &record(b"STAT", 0x900, 0, &[]));
    let order = LoadOrder::new(
        vec![header("A.esm", &a).unwrap(), header("B.esm", &b).unwrap()],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let mut index = Overrides::new();
    let mut code = None;
    scan(
        &SliceSource {
            name: "B.esm",
            bytes: &b,
        },
        &order.headers[1],
        &Limits::default(),
        |r, _| {
            code = index
                .insert(&order, 1, &r, &Budget::new(1 << 20))
                .err()
                .map(|e| e.code);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(code.as_deref(), Some("BaseUnavailable"));
}
#[test]
fn extension_masters_are_recognized() {
    let a = plugin(0, &[], 0, &[]);
    let b = plugin(MASTER, &[], 0, &[]);
    assert!(LoadOrder::new(
        vec![
            header("Flagless.esm", &a).unwrap(),
            header("B.esm", &b).unwrap()
        ],
        &Budget::new(1 << 20)
    )
    .is_ok());
}

#[test]
fn language_codecs_are_strict_and_preserve_bytes() {
    for kind in [
        StringKind::Strings,
        StringKind::DlStrings,
        StringKind::IlStrings,
    ] {
        for (language, text, expected) in [
            ("en", b"Caf\xe9 \x80\x92\x97".as_slice(), Some("Café €’—")),
            ("en", b"bad\x81".as_slice(), None),
            ("fr", "été 日本語".as_bytes(), Some("été 日本語")),
            ("fr", b"bad\xe9".as_slice(), None),
        ] {
            let mut body = text.to_vec();
            body.push(0);
            if kind != StringKind::Strings {
                let mut prefixed = (body.len() as u32).to_le_bytes().to_vec();
                prefixed.extend(body);
                body = prefixed;
            }
            let mut bytes = 1u32.to_le_bytes().to_vec();
            bytes.extend((body.len() as u32).to_le_bytes());
            bytes.extend(71u32.to_le_bytes());
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(body);
            let t = StringTable::read_for_language(
                "table",
                kind,
                bytes.clone(),
                language,
                &Limits::default(),
                &Budget::new(1 << 20),
            )
            .unwrap();
            assert_eq!(t.bytes, bytes);
            match expected {
                Some(text) => {
                    assert_eq!(t.get(71, &Limits::default()).unwrap(), text);
                    assert!(t.diagnostics.is_empty());
                }
                None => {
                    assert_eq!(
                        t.get(71, &Limits::default()).unwrap_err().code,
                        "InvalidStringEncoding"
                    );
                    assert_eq!(t.diagnostics.len(), 1);
                }
            }
        }
    }
}

#[test]
fn winner_source_catalog_rejects_wrong_or_missing_source() {
    let a = plugin(
        MASTER,
        &[],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"EDID", b"base\0")),
    );
    let b = plugin(
        0,
        &["A.esm"],
        1,
        &record(b"STAT", 0x800, 0, &sub(b"EDID", b"winner\0")),
    );
    let order = LoadOrder::new(
        vec![header("A.esm", &a).unwrap(), header("B.esp", &b).unwrap()],
        &Budget::new(1 << 20),
    )
    .unwrap();
    let mut index = Overrides::new();
    for (i, bytes) in [&a, &b].into_iter().enumerate() {
        scan(
            &SliceSource {
                name: "records",
                bytes,
            },
            &order.headers[i],
            &Limits::default(),
            |r, _| index.insert(&order, i, &r, &Budget::new(1 << 20)),
        )
        .unwrap();
    }
    let key = FormKey {
        plugin: "a.esm".into(),
        object_id: 0x800,
    };
    let wrong = SliceSource {
        name: "A.esm",
        bytes: &a,
    };
    assert_eq!(
        index
            .read_winner(
                &key,
                &order,
                &[&wrong, &wrong],
                &Limits::default(),
                &Budget::new(1 << 20)
            )
            .unwrap_err()
            .code,
        "SourceMismatch"
    );
    assert_eq!(
        index
            .read_winner(
                &key,
                &order,
                &[&wrong],
                &Limits::default(),
                &Budget::new(1 << 20)
            )
            .unwrap_err()
            .code,
        "SourceUnavailable"
    );
}

#[test]
fn flagless_esl_is_light_without_rewriting_flags() {
    let raw = plugin(0, &[], 0, &[]);
    let h = header("Flagless.esl", &raw).unwrap();
    assert_eq!(h.flags, 0);
    let order = LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap();
    let key = order.remap(0, 0x800, "Flagless.esl", 0).unwrap().unwrap();
    assert_eq!(order.runtime_id(&key).unwrap(), 0xfe000800);
    assert_eq!(order.headers()[0].flags, 0);
}

#[test]
fn seven_documented_plugin_types_get_correct_slots_and_ranges() {
    for (name, flags, master, light) in [
        ("A.esm", 0, true, false),
        ("B.esm", LIGHT, true, false),
        ("C.esm", MASTER | LIGHT, true, true),
        ("D.esl", 0, true, true),
        ("E.esp", MASTER, true, false),
        ("F.esp", MASTER | LIGHT, true, true),
        ("G.esp", LIGHT, false, true),
    ] {
        let bytes = plugin(flags, &[], 0, &[]);
        let h = header(name, &bytes).unwrap();
        assert_eq!(h.is_master(), master, "{name}");
        assert_eq!(h.is_light(), light, "{name}");
        assert_eq!(h.flags, flags);
        let order = LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap();
        let key = order.remap(0, 0x800, name, 0).unwrap().unwrap();
        assert_eq!(
            order.runtime_id(&key).unwrap(),
            if light { 0xfe000800 } else { 0x800 },
            "{name}"
        );
        assert_eq!(order.remap(0, 1, name, 0).is_ok(), !light, "{name}");
    }
}

#[test]
fn and_observation_count_delimited_raw_text_once_in_all_tables() {
    for kind in [
        StringKind::Strings,
        StringKind::DlStrings,
        StringKind::IlStrings,
    ] {
        for language in ["en", "english", "fr"] {
            let texts: &[&[u8]] = &[
                b"\xef\xbb\xbfBOM",
                b"inside\xef\xbb\xbf",
                b"\xef\xbb\xbf\x81",
                b"ascii",
                b"Caf\xe9",
                b"\xc0\xaf",
            ];
            let mut data = Vec::new();
            let mut directory = Vec::new();
            for (id, text) in texts.iter().enumerate() {
                directory.extend((id as u32).to_le_bytes());
                directory.extend((data.len() as u32).to_le_bytes());
                if kind != StringKind::Strings {
                    data.extend(((text.len() + 1) as u32).to_le_bytes());
                }
                data.extend(*text);
                data.push(0);
            }
            let mut bytes = (texts.len() as u32).to_le_bytes().to_vec();
            bytes.extend((data.len() as u32).to_le_bytes());
            bytes.extend(directory);
            bytes.extend(data);
            let t = StringTable::read_for_language(
                "table",
                kind,
                bytes.clone(),
                language,
                &Limits::default(),
                &Budget::new(1 << 20),
            )
            .unwrap();
            assert_eq!(t.bom_prefixed, 2); // Includes delimited text whose codec fails.
            assert_eq!(
                t.english_utf8_multibyte,
                if language == "fr" { 0 } else { 2 }
            );
            assert_eq!(t.bytes, bytes);
            let expected = if language == "fr" {
                "\u{feff}BOM"
            } else {
                "ï»¿BOM"
            };
            for _ in 0..3 {
                assert_eq!(t.get(0, &Limits::default()).unwrap(), expected);
            }
            assert_eq!(t.bom_prefixed, 2);
        }
    }
}

fn sse_header(name: &str, version: f32, flags: u32, masters: &[&str]) -> Header {
    let mut bytes = plugin(flags, masters, 0, &[]);
    bytes[20..22].copy_from_slice(&44u16.to_le_bytes());
    bytes[30..34].copy_from_slice(&version.to_le_bytes());
    Header::read_for_game(
        fo4_core::Game::SkyrimSe,
        name,
        &SliceSource {
            name,
            bytes: &bytes,
        },
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap()
}
#[test]
fn sse_header_profiles_light_ranges_and_effective_type() {
    for (version, min) in [(1.7, 0x800), (1.71, 1)] {
        let h = sse_header("Light.esl", version, 0, &[]);
        assert_eq!(h.form_version, 44);
        assert!(h.is_master() && h.is_light());
        let o = LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap();
        assert!(o.remap(0, min, "Light.esl", 32).unwrap().is_some());
        if min > 1 {
            assert!(o.remap(0, min - 1, "Light.esl", 32).is_err());
        } else {
            assert_eq!(o.remap(0, 0, "Light.esl", 32).unwrap(), None);
        }
        assert!(o.remap(0, 0x1000, "Light.esl", 32).is_err());
        let key = o.remap(0, 0xfff, "Light.esl", 32).unwrap().unwrap();
        assert_eq!(o.runtime_id(&key).unwrap(), 0xfe000fff);
    }
    assert!(!sse_header("Light.esm", 1.7, LIGHT, &[]).is_light());
    assert!(sse_header("Light.esp", 1.7, LIGHT, &[]).is_light());
    let modern = sse_header("Modern.esl", 1.71, 0, &[]);
    let old = sse_header("Old.esp", 1.7, 0, &["Modern.esl"]);
    let order = LoadOrder::new(vec![modern, old], &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        order.remap(1, 1, "Old.esp", 32).unwrap_err().code,
        "InvalidFormId"
    );
    let mixed = vec![
        header("FO4.esm", &plugin(MASTER, &[], 0, &[])).unwrap(),
        sse_header("SSE.esm", 1.7, MASTER, &[]),
    ];
    assert_eq!(
        LoadOrder::new(mixed, &Budget::new(1 << 20))
            .err()
            .unwrap()
            .code,
        "MixedGameLoadOrder"
    );
}
#[test]
fn sse_header_versions_truncation_and_record_layout_separation() {
    let mut bytes = plugin(MASTER, &[], 0, &[]);
    bytes[20..22].copy_from_slice(&44u16.to_le_bytes());
    bytes[30..34].copy_from_slice(&1.7f32.to_le_bytes());
    let l = Limits::default();
    assert_eq!(
        header("WrongGame.esm", &bytes).unwrap_err().code,
        "UnsupportedPluginVersion"
    );
    for end in 0..bytes.len() {
        let b = Budget::new(16384);
        assert!(Header::read_for_game(
            fo4_core::Game::SkyrimSe,
            "test.esm",
            &SliceSource {
                name: "test.esm",
                bytes: &bytes[..end]
            },
            &l,
            &b
        )
        .is_err());
        assert!(b.used() <= 16384);
    }
    bytes[30..34].copy_from_slice(&1.72f32.to_le_bytes());
    assert_eq!(
        Header::read_for_game(
            fo4_core::Game::SkyrimSe,
            "test.esm",
            &SliceSource {
                name: "test.esm",
                bytes: &bytes
            },
            &l,
            &Budget::new(1 << 20)
        )
        .unwrap_err()
        .code,
        "UnsupportedPluginVersion"
    );
    let mut raw = record(b"CELL", 0x800, COMPRESSED, &sub(b"DATA", &[1, 2, 3]));
    raw[20..22].copy_from_slice(&14u16.to_le_bytes());
    let r = record_at(
        &SliceSource {
            name: "old-form.esm",
            bytes: &raw,
        },
        0,
        raw.len() as u64,
        &l,
        &Budget::new(1 << 20),
    )
    .unwrap();
    assert_eq!(r.version, 14);
    assert!(r.fields(&l, &Budget::new(1 << 20)).is_err());
    assert!(r
        .fields_for_game(fo4_core::Game::SkyrimSe, &l, &Budget::new(1 << 20))
        .unwrap()
        .iter()
        .all(|f| f.value == Value::Raw));
}

#[test]
fn nonfinite_scalar_and_transform_remain_interpreted() {
    for (sig, values) in [
        (*b"XSCL", vec![0x7fc12345u32]),
        (*b"DATA", vec![0x7f800000, 0xff800000, 0x7fc12345, 0, 0, 0]),
    ] {
        let mut data = Vec::new();
        for bits in &values {
            data.extend(bits.to_le_bytes());
        }
        let body = record(b"REFR", 0x801, 0, &sub(&sig, &data));
        let bytes = plugin(MASTER, &[], 1, &body);
        let h = header("nonfinite.esm", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "nonfinite.esm",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                let fields = r.fields(&Limits::default(), &Budget::new(65536)).unwrap();
                match &fields[0].value {
                    Value::Float(v) => assert_eq!(v.to_bits(), values[0]),
                    Value::Transform(v) => {
                        for (actual, expected) in v.iter().zip(&values) {
                            assert_eq!(actual.to_bits(), *expected)
                        }
                    }
                    _ => panic!("float lost"),
                }
                Ok(())
            },
        )
        .unwrap();
    }
}

#[test]
fn nonfinite_light_lighting_and_scol_keep_typed_counts() {
    for (kind, size, changes, expected) in [
        (
            *b"LIGH",
            64usize,
            vec![(16usize, 0x7fc12345u32), (60, 0xff800000)],
            2u64,
        ),
        (
            *b"LGTM",
            40,
            vec![
                (12, 0x7f800000),
                (16, 0x7fc12345),
                (28, 0xff800000),
                (32, 0xffc12345),
            ],
            4,
        ),
        (*b"SCOL", 28, vec![(0, 0x7fc12345), (24, 0x7f800000)], 2),
    ] {
        let mut data = vec![0; size];
        for (o, bits) in changes {
            data[o..o + 4].copy_from_slice(&bits.to_le_bytes());
        }
        let bytes = plugin(1, &[], 1, &record(&kind, 0x800, 0, &sub(b"DATA", &data)));
        let h = header("ieee.esm", &bytes).unwrap();
        scan(
            &SliceSource {
                name: "ieee.esm",
                bytes: &bytes,
            },
            &h,
            &Limits::default(),
            |r, _| {
                let fields = r.fields(&Limits::default(), &Budget::new(65536))?;
                assert!(!matches!(fields[0].value, Value::Raw));
                assert_eq!(fields[0].non_finite_values, expected);
                assert_eq!(fields[0].value.non_finite_values(), expected);
                Ok(())
            },
        )
        .unwrap();
    }
}

#[test]
fn typed_winners_cell_membership_and_raw_ranges() {
    use crate::typed::{Cell, Refr, Stat, Store};
    let mut p = sub(b"EDID", b"TestRoom\0");
    p.extend(sub(b"DATA", &1u16.to_le_bytes()));
    p.extend(sub(b"ZZZZ", &[9, 8, 7]));
    let cell = record(b"CELL", 0x800, 0, &p);
    let mut placed = sub(b"NAME", &0x801u32.to_le_bytes());
    placed.extend(sub(b"XESP", &[0; 8]));
    let refs = group(
        6,
        0x800u32.to_le_bytes(),
        &group(
            9,
            0x800u32.to_le_bytes(),
            &record(b"REFR", 0x802, DISABLED, &placed),
        ),
    );
    let mut body = group(2, [0; 4], &[cell, refs].concat());
    body.extend(record(b"STAT", 0x801, 0, &sub(b"MODL", b"test.nif\0")));
    let base = plugin(MASTER, &[], 0, &body);
    let patch = plugin(
        0,
        &["base.esm"],
        0,
        &group(
            6,
            0x800u32.to_le_bytes(),
            &group(
                8,
                0x800u32.to_le_bytes(),
                &record(b"REFR", 0x802, DELETED, &placed),
            ),
        ),
    );
    let sources: Vec<(String, Box<dyn ReadAt>)> = vec![
        ("base.esm".into(), Box::new(OwnedSource(base))),
        ("patch.esp".into(), Box::new(PatchSource(patch))),
    ];
    let index = Budget::new(8 << 20);
    let store =
        Store::from_sources(fo4_core::Game::Fallout4, sources, Limits::default(), &index).unwrap();
    let key = FormKey {
        plugin: "base.esm".into(),
        object_id: 0x800,
    };
    let work = Budget::new(1 << 20);
    // Every occurrence, overridden ones included, reads back from its plugin.
    let refr: Vec<usize> = store
        .overrides()
        .history
        .iter()
        .enumerate()
        .filter(|(_, o)| o.signature == *b"REFR")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(refr.len(), 2);
    let (first, plugin) = store.raw_occurrence(refr[0], &work).unwrap();
    assert_eq!(plugin, 0);
    assert!(!first.deleted() && first.disabled());
    let (second, plugin) = store.raw_occurrence(refr[1], &work).unwrap();
    assert_eq!(plugin, 1);
    assert!(second.deleted());
    assert_eq!(
        store.raw_occurrence(usize::MAX, &work).unwrap_err().code,
        "InvalidOccurrence"
    );
    let c = store.get::<Cell>(&key, &work).unwrap().unwrap();
    assert_eq!(c.data.common.editor_id.as_deref(), Some("TestRoom"));
    assert_eq!(c.data.common.uninterpreted.len(), 1);
    assert_eq!(
        c.record
            .data(&c.record.subrecords[c.data.common.uninterpreted[0].subrecord]),
        &[9, 8, 7]
    );
    let children = store.children(&key, &work).unwrap();
    assert_eq!(children.deleted.len(), 1);
    assert!(children.disabled.is_empty());
    assert!(children.active.is_empty());
    assert_eq!(children.deleted[0].group_kind, 8);
    let refkey = &children.deleted[0].key;
    let r = store.get::<Refr>(refkey, &work).unwrap().unwrap();
    assert_eq!(
        r.data
            .base
            .as_ref()
            .unwrap()
            .key
            .as_ref()
            .unwrap()
            .object_id,
        0x801
    );
    assert!(r.data.enable_parent.is_some());
    assert_eq!(
        store.get::<Stat>(&key, &work).unwrap_err().code,
        "RecordTypeMismatch"
    );
    assert_eq!(store.find_cell("TestRoom", &work).unwrap(), Some(key));
}

#[test]
fn a_cell_that_does_not_decode_fails_only_its_own_lookup() {
    use crate::typed::Store;
    let mut body = record(b"CELL", 0x800, 0, &sub(b"EDID", b"Good\0"));
    // Two EDID: the typed reading refuses it.
    body.extend(record(
        b"CELL",
        0x810,
        0,
        &[sub(b"EDID", b"Bad\0"), sub(b"EDID", b"Bad\0")].concat(),
    ));
    let base = plugin(MASTER, &[], 0, &group(0, *b"CELL", &body));
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![("base.esm".into(), Box::new(OwnedSource(base)))],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let b = Budget::new(1 << 20);
    assert_eq!(
        store.find_cell("good", &b).unwrap().map(|k| k.object_id),
        Some(0x800)
    );
    assert!(store.find_cell("Bad", &b).is_err());
    assert_eq!(store.find_cell("Other", &b).unwrap(), None);
}

struct OwnedSource(Vec<u8>);
impl ReadAt for OwnedSource {
    fn name(&self) -> &str {
        "base.esm"
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> Result<()> {
        SliceSource {
            name: self.name(),
            bytes: &self.0,
        }
        .read_exact_at(o, out)
    }
}
struct PatchSource(Vec<u8>);
impl ReadAt for PatchSource {
    fn name(&self) -> &str {
        "patch.esp"
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> Result<()> {
        SliceSource {
            name: self.name(),
            bytes: &self.0,
        }
        .read_exact_at(o, out)
    }
}

#[test]
fn typed_slice_options_scol_swap_txst_and_partial_light() {
    use crate::typed::*;
    let mut body = Vec::new();
    for (i, sig) in [
        b"CELL", b"WRLD", b"REFR", b"STAT", b"SCOL", b"MSTT", b"ACTI", b"DOOR", b"FURN", b"LIGH",
        b"LGTM", b"MSWP", b"TXST",
    ]
    .iter()
    .enumerate()
    {
        let mut data = Vec::new();
        match *sig {
            b"SCOL" => {
                data.extend(sub(b"ONAM", &0x803u32.to_le_bytes()));
                data.extend(sub(b"DATA", &[0; 28]));
                data.extend(sub(b"ONAM", &0x805u32.to_le_bytes()));
                data.extend(sub(b"DATA", &[0; 56]));
            }
            b"REFR" => data.extend(sub(b"XMSP", &0x80bu32.to_le_bytes())),
            b"LIGH" => data.extend(sub(b"DATA", &[0; 48])),
            b"LGTM" => data.extend(sub(b"DATA", &[0; 44])),
            b"MSWP" => {
                for _ in 0..2 {
                    data.extend(sub(b"BNAM", b"materials/a.bgsm\0"));
                    data.extend(sub(b"SNAM", b"materials/b.bgsm\0"));
                }
            }
            b"TXST" => {
                data.extend(sub(b"TX00", b"textures/a.dds\0"));
                data.extend(sub(b"DNAM", &[1, 0]));
            }
            _ => {}
        }
        data.extend(sub(b"ZZZZ", &[1, 2]));
        body.extend(record(sig, 0x800 + i as u32, 0, &data));
    }
    let source = plugin(MASTER, &[], 0, &body);
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![("base.esm".into(), Box::new(OwnedSource(source)))],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let b = Budget::new(8 << 20);
    let key = |i: u32| FormKey {
        plugin: "base.esm".into(),
        object_id: 0x800 + i,
    };
    assert!(store
        .get::<Cell>(&key(0), &b)
        .unwrap()
        .unwrap()
        .data
        .flags
        .is_none());
    assert!(store
        .get::<Wrld>(&key(1), &b)
        .unwrap()
        .unwrap()
        .data
        .parent
        .is_none());
    assert_eq!(
        store
            .get::<Refr>(&key(2), &b)
            .unwrap()
            .unwrap()
            .data
            .material_swap
            .unwrap()
            .key
            .unwrap(),
        key(11)
    );
    assert!(store
        .get::<Stat>(&key(3), &b)
        .unwrap()
        .unwrap()
        .data
        .common
        .model
        .path
        .is_none());
    let s = store.get::<Scol>(&key(4), &b).unwrap().unwrap();
    assert_eq!(s.data.parts.len(), 2);
    assert!(matches!(&s.data.parts[1].placements,Some(p) if p.len()==2));
    assert!(store.get::<Mstt>(&key(5), &b).unwrap().is_some());
    assert!(store.get::<Acti>(&key(6), &b).unwrap().is_some());
    assert!(store.get::<Door>(&key(7), &b).unwrap().is_some());
    assert!(store.get::<Furn>(&key(8), &b).unwrap().is_some());
    let l = store.get::<Ligh>(&key(9), &b).unwrap().unwrap();
    assert!(l.data.light.is_some());
    assert_eq!(l.data.common.uninterpreted.len(), 1);
    let l = store.get::<Lgtm>(&key(10), &b).unwrap().unwrap();
    assert_eq!(l.data.common.uninterpreted.len(), 2);
    assert_eq!(l.data.common.uninterpreted[0].bytes.len(), 44);
    assert_eq!(
        l.data.common.uninterpreted[0].open_field,
        Some("unverified_size")
    );
    let swap = store.get::<Mswp>(&key(11), &b).unwrap().unwrap();
    assert_eq!(swap.data.entries.len(), 4);
    assert_eq!(swap.data.common.uninterpreted.len(), 1);
    let tx = store.get::<Txst>(&key(12), &b).unwrap().unwrap();
    assert_eq!(tx.data.textures[0].as_deref(), Some("textures/a.dds"));
    assert!(tx.data.textures[1].is_none());
    assert_eq!(tx.data.flags, Some(1));
    assert_eq!(
        store
            .get::<Cell>(&key(0), &Budget::new(1))
            .unwrap_err()
            .code,
        "LimitExceeded"
    );
}

#[test]
fn typed_duplicate_and_corrupt_new_reference_field_fail_named() {
    use crate::typed::{Refr, Store};
    for data in [
        sub(b"XMSP", &[1, 2, 3]),
        [
            sub(b"XMSP", &0u32.to_le_bytes()),
            sub(b"XMSP", &0u32.to_le_bytes()),
        ]
        .concat(),
    ] {
        let source = plugin(MASTER, &[], 0, &record(b"REFR", 0x800, 0, &data));
        let store = Store::from_sources(
            fo4_core::Game::Fallout4,
            vec![("base.esm".into(), Box::new(OwnedSource(source)))],
            Limits::default(),
            &Budget::new(8 << 20),
        )
        .unwrap();
        let error = store
            .get::<Refr>(
                &FormKey {
                    plugin: "base.esm".into(),
                    object_id: 0x800,
                },
                &Budget::new(1 << 20),
            )
            .unwrap_err();
        assert!(matches!(
            error.code.as_str(),
            "InvalidFieldSize" | "DuplicateTypedField"
        ));
        assert_eq!(error.source.file, "base.esm");
        assert!(error.source.offset > 0);
    }
}

#[test]
fn relocated_winner_belongs_only_to_new_cell_and_bad_label_is_named() {
    use crate::typed::Store;
    let key = |id| FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    for bad in [false, true] {
        let rf = record(b"REFR", 0x802, 0, &sub(b"NAME", &0x801u32.to_le_bytes()));
        let mut body = record(b"CELL", 0x800, 0, &[]);
        body.extend(record(b"CELL", 0x810, 0, &[]));
        body.extend(group(
            6,
            0x800u32.to_le_bytes(),
            &group(9, 0x800u32.to_le_bytes(), &rf),
        ));
        let base = plugin(MASTER, &[], 0, &group(2, [0; 4], &body));
        let patch = plugin(
            0,
            &["base.esm"],
            0,
            &group(
                6,
                0x810u32.to_le_bytes(),
                &group(9, (if bad { 0x800u32 } else { 0x810 }).to_le_bytes(), &rf),
            ),
        );
        let store = Store::from_sources(
            fo4_core::Game::Fallout4,
            vec![
                ("base.esm".into(), Box::new(OwnedSource(base))),
                ("patch.esp".into(), Box::new(PatchSource(patch))),
            ],
            Limits::default(),
            &Budget::new(8 << 20),
        )
        .unwrap();
        if bad {
            assert!(store
                .diagnostics
                .iter()
                .any(|e| e.code == "CellGroupLabelMismatch"));
        } else {
            assert!(store
                .children(&key(0x800), &Budget::new(1 << 20))
                .unwrap()
                .active
                .is_empty());
            assert_eq!(
                store
                    .children(&key(0x810), &Budget::new(1 << 20))
                    .unwrap()
                    .active[0]
                    .key,
                key(0x802)
            );
        }
    }
}

#[test]
fn a_bad_group_label_is_named_and_the_rest_of_the_plugin_is_indexed() {
    use crate::typed::Store;
    let key = |p: &str, id| FormKey {
        plugin: p.into(),
        object_id: id,
    };
    let rf = record(b"REFR", 0x802, 0, &sub(b"NAME", &0x801u32.to_le_bytes()));
    let mut body = record(b"CELL", 0x800, 0, &[]);
    body.extend(record(b"CELL", 0x810, 0, &[]));
    let base = plugin(MASTER, &[], 0, &group(2, [0; 4], &body));
    // The patch's first group names another cell than its parent; a new
    // static follows it.
    let mut patch_body = group(
        6,
        0x810u32.to_le_bytes(),
        &group(9, 0x800u32.to_le_bytes(), &rf),
    );
    patch_body.extend(group(0, *b"STAT", &record(b"STAT", 0x0100_0900, 0, &[])));
    let patch = plugin(0, &["base.esm"], 0, &patch_body);
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![
            ("base.esm".into(), Box::new(OwnedSource(base))),
            ("patch.esp".into(), Box::new(PatchSource(patch))),
        ],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    assert!(store
        .diagnostics
        .iter()
        .any(|e| e.code == "CellGroupLabelMismatch"));
    assert_eq!(store.signature(&key("patch.esp", 0x900)), Some(*b"STAT"));
    // A plugin whose scan stops part-way (its last group cut short) is not
    // opened on the part read.
    let base = plugin(MASTER, &[], 0, &group(2, [0; 4], &body));
    let mut cut = plugin(0, &["base.esm"], 0, &patch_body);
    cut.truncate(cut.len() - 4);
    let e = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![
            ("base.esm".into(), Box::new(OwnedSource(base))),
            ("patch.esp".into(), Box::new(PatchSource(cut))),
        ],
        Limits::default(),
        &Budget::new(8 << 20),
    );
    assert!(e.is_err());
}

#[test]
fn cell_kind_uses_only_winning_group_context() {
    use crate::typed::Store;
    let base = plugin(
        MASTER,
        &[],
        0,
        &group(2, [0; 4], &record(b"CELL", 0x800, 0, &[])),
    );
    let patch = plugin(0, &["base.esm"], 0, &record(b"CELL", 0x800, 0, &[]));
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![
            ("base.esm".into(), Box::new(OwnedSource(base))),
            ("patch.esp".into(), Box::new(PatchSource(patch))),
        ],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let key = FormKey {
        plugin: "base.esm".into(),
        object_id: 0x800,
    };
    assert_eq!(
        store.kind(&key),
        None,
        "must not inherit interior from a losing CELL"
    );
}

#[test]
fn unassigned_winning_reference_has_named_diagnostic() {
    use crate::typed::Store;
    let rf = record(b"REFR", 0x802, 0, &sub(b"NAME", &0x801u32.to_le_bytes()));
    let base = plugin(
        MASTER,
        &[],
        0,
        &group(
            2,
            [0; 4],
            &[
                record(b"CELL", 0x800, 0, &[]),
                group(
                    6,
                    0x800u32.to_le_bytes(),
                    &group(9, 0x800u32.to_le_bytes(), &rf),
                ),
            ]
            .concat(),
        ),
    );
    let patch = plugin(0, &["base.esm"], 0, &rf);
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![
            ("base.esm".into(), Box::new(OwnedSource(base))),
            ("patch.esp".into(), Box::new(PatchSource(patch))),
        ],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    assert!(store
        .diagnostics
        .iter()
        .any(|e| e.code == "UnassignedCellReference"));
}

#[test]
fn compressed_typed_error_keeps_decompressed_offset_on_numeric_collision() {
    use crate::typed::{Cell, Store};
    let mut data = sub(b"EDID", b"A\0");
    data.extend(sub(b"ZZZZ", &[1; 28]));
    assert_eq!(data.len(), 42);
    data.extend(sub(b"EDID", b"B\0"));
    let source = plugin(MASTER, &[], 0, &record(b"CELL", 0x800, COMPRESSED, &data));
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![("base.esm".into(), Box::new(OwnedSource(source)))],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let error = store
        .get::<Cell>(
            &FormKey {
                plugin: "base.esm".into(),
                object_id: 0x800,
            },
            &Budget::new(1 << 20),
        )
        .unwrap_err();
    assert_eq!(error.code, "DuplicateTypedField");
    assert_eq!(error.source.offset, 66);
    assert_eq!(error.source.decompressed_offset, Some(42));
}

#[test]
fn static_cell_references_are_decoded_not_guessed_or_defaulted() {
    let data = [
        sub(b"XCIM", &0x800u32.to_le_bytes()),
        sub(b"XGDR", &0x801u32.to_le_bytes()),
    ]
    .concat();
    let bytes = plugin(MASTER, &[], 1, &record(b"CELL", 0x802, 0, &data));
    let h = header("Base.esm", &bytes).unwrap();
    scan(
        &SliceSource {
            name: "Base.esm",
            bytes: &bytes,
        },
        &h,
        &Limits::default(),
        |r, _| {
            let b = Budget::new(1 << 20);
            let fields = r.fields(&Limits::default(), &b)?;
            for field in fields {
                assert!(matches!(field.value, Value::FormId(_)));
            }
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn contract_covers_every_case12_cause_once_with_evidence_or_gap() {
    use crate::static_frame::{rule, Class, TABLE};
    let keys = include_str!("../../../spec/static-record-keys.txt");
    let mut n = 0;
    for key in keys
        .lines()
        .filter(|s| !s.starts_with('#') && !s.is_empty())
    {
        n += 1;
        let record: &[u8; 4] = key.as_bytes()[..4].try_into().unwrap();
        let tag: &[u8; 4] = key.as_bytes()[5..].try_into().unwrap();
        let r = rule(record, tag).unwrap_or_else(|| panic!("missing {key}"));
        assert!(!r.reason.is_empty());
        if r.class != Class::Unknown {
            assert!(!r.source.is_empty());
            assert_ne!(r.layout, "none");
        }
        assert_eq!(
            TABLE
                .lines()
                .filter(|s| s.split('\t').next() == Some(key))
                .count(),
            1
        );
    }
    assert_eq!(n, 191);
    assert_eq!(
        crate::static_frame::classify(b"STAT", b"ZZZZ"),
        Class::Unknown
    );
}

#[test]
fn common_outside_tags_cover_the_new_visible_types() {
    use crate::static_frame::{classify, Class};
    for record in [
        b"MISC", b"CONT", b"ALCH", b"BOOK", b"AMMO", b"KEYM", b"NOTE", b"FLOR", b"TERM", b"TACT",
    ] {
        for tag in [
            b"PTRN", b"KSIZ", b"KWDA", b"DEST", b"DSTD", b"DSTF", b"DMDL", b"DMDT",
        ] {
            assert_eq!(
                classify(record, tag),
                Class::Outside,
                "{}.{}",
                String::from_utf8_lossy(record),
                String::from_utf8_lossy(tag)
            );
        }
    }
}

#[test]
fn marker_model_catalog_uses_only_winning_labelled_types() {
    use crate::typed::Store;
    let original = plugin(
        MASTER,
        &[],
        0,
        &[
            record(
                b"STAT",
                0x800,
                0x0080_0000,
                &sub(b"MODL", b"Synthetic\\Marker.nif\0"),
            ),
            record(b"STAT", 0x801, 0x0080_0000, &sub(b"MODL", b"Removed.nif\0")),
            record(
                b"LIGH",
                0x802,
                0x0080_0000,
                &sub(b"MODL", b"DifferentMeaning.nif\0"),
            ),
            record(b"STAT", 0x803, 0, &sub(b"MODL", b"synthetic/marker.NIF\0")),
        ]
        .concat(),
    );
    let patch = plugin(
        0,
        &["base.esm"],
        0,
        &record(b"STAT", 0x801, 0, &sub(b"MODL", b"Replacement.nif\0")),
    );
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![
            ("base.esm".into(), Box::new(OwnedSource(original))),
            ("patch.esp".into(), Box::new(PatchSource(patch))),
        ],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    assert_eq!(store.marker_models, [b"synthetic/marker.nif".to_vec()]);
}

#[test]
fn equal_zlib_lengths_preserve_different_payloads_without_new_fields() {
    let l = Limits::default();
    let mut sizes = Vec::new();
    for n in [0usize, 1, 12, 93, 255] {
        let r = Record {
            signature: *b"STAT",
            flags: 0x80,
            form_id: 0x800,
            revision: 7,
            version: 131,
            unknown: 0,
            source: fo4_core::SourceSpan::new("synthetic", 0, n as u64),
            payload: vec![0x37; n],
            subrecords: vec![],
            payload_is_decompressed: false,
        };
        for target in 300..320 {
            let b = Budget::new(1 << 20);
            let encoded = writer::rewrite_sized_zlib(&r, target, &l, &b).unwrap();
            assert_eq!(encoded.len(), target + 28);
            assert_eq!(
                u32::from_le_bytes(encoded[8..12].try_into().unwrap()),
                0x80 | COMPRESSED
            );
            assert_eq!(
                fo4_core::zlib(
                    &encoded[28..],
                    n as u64,
                    &l,
                    &Budget::new(1 << 20),
                    "synthetic",
                    28
                )
                .unwrap(),
                r.payload
            );
            let mut corrupt = encoded[28..].to_vec();
            *corrupt.last_mut().unwrap() ^= 1;
            assert!(fo4_core::zlib(
                &corrupt,
                n as u64,
                &l,
                &Budget::new(1 << 20),
                "synthetic",
                28
            )
            .is_err());
            for cut in [0, 1, 2, target / 2, target - 1] {
                assert!(fo4_core::zlib(
                    &encoded[28..28 + cut],
                    n as u64,
                    &l,
                    &Budget::new(1 << 20),
                    "synthetic",
                    28
                )
                .is_err());
            }
            sizes.push(encoded.len());
            assert!(b.used() <= 1 << 20);
        }
        assert!(writer::rewrite_sized_zlib(&r, n + 10, &l, &Budget::new(1 << 20)).is_err());
        assert!(writer::rewrite_sized_zlib(&r, 300, &l, &Budget::new(0)).is_err());
    }
    assert_eq!(sizes.len(), 100);
}

#[test]
fn envelope_scan_matches_the_full_scan_without_reading_payloads() {
    let stat = record(b"STAT", 0x800, COMPRESSED, &sub(b"EDID", b"A\0"));
    let mut broken = record(b"STAT", 0x801, COMPRESSED | DELETED, &sub(b"EDID", b"B\0"));
    // Corrupt the zlib stream only; the 24-byte header still delimits the record.
    let last = broken.len() - 1;
    broken[last] ^= 0xff;
    let refr = record(
        b"REFR",
        0x802,
        DISABLED,
        &sub(b"NAME", &0x800u32.to_le_bytes()),
    );
    let mut body = group(0, *b"STAT", &stat);
    body.extend(group(0, *b"REFR", &refr));
    let good = plugin(0, &[], 4, &body);
    let h = header("synthetic.esp", &good).unwrap();
    let source = SliceSource {
        name: "synthetic.esp",
        bytes: &good,
    };
    let limits = Limits::default();
    let identity = |r: &Record, groups: &[Group]| {
        (
            r.signature,
            r.form_id,
            r.flags,
            r.source.offset,
            r.source.length,
            r.deleted(),
            r.disabled(),
            groups.len(),
        )
    };
    let mut full = Vec::new();
    let full_stats = scan_bounded(&source, &h, &limits, &Budget::new(1 << 20), |r, g, _| {
        assert!(!r.subrecords.is_empty());
        full.push(identity(&r, g));
        Ok(())
    })
    .unwrap();
    let mut envelopes = Vec::new();
    let stats = scan_envelopes(&source, &h, &limits, &Budget::new(1 << 20), |r, g, _| {
        assert!(r.payload.is_empty() && r.subrecords.is_empty() && !r.payload_is_decompressed);
        envelopes.push(identity(&r, g));
        Ok(())
    })
    .unwrap();
    assert_eq!(envelopes, full);
    assert_eq!(
        (
            stats.groups,
            stats.records,
            stats.compressed,
            stats.count_matches
        ),
        (
            full_stats.groups,
            full_stats.records,
            full_stats.compressed,
            full_stats.count_matches
        )
    );
    assert_eq!(stats.subrecords, 0);

    let mut body = group(0, *b"STAT", &[stat, broken].concat());
    body.extend(group(0, *b"REFR", &refr));
    let bad = plugin(0, &[], 5, &body);
    let h = header("synthetic.esp", &bad).unwrap();
    let source = SliceSource {
        name: "synthetic.esp",
        bytes: &bad,
    };
    assert!(scan_bounded(
        &source,
        &h,
        &limits,
        &Budget::new(1 << 20),
        |_, _, _| Ok(())
    )
    .is_err());
    let mut seen = Vec::new();
    scan_envelopes(&source, &h, &limits, &Budget::new(1 << 20), |r, _, _| {
        seen.push((r.form_id, r.deleted()));
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, [(0x800, false), (0x801, true), (0x802, false)]);
    // Truncating a header is still a structural error for the envelope scan.
    let cut = &bad[..bad.len() - refr.len() + 10];
    let source = SliceSource {
        name: "synthetic.esp",
        bytes: cut,
    };
    assert!(scan_envelopes(
        &source,
        &h,
        &limits,
        &Budget::new(1 << 20),
        |_, _, _| Ok(())
    )
    .is_err());
}

#[test]
fn writer_refuses_what_the_reader_would_reject() {
    use crate::writer::{self, HeaderModel, Master, RecordModel};
    let limits = Limits::default();
    let budget = Budget::new(1 << 20);
    for signature in [*b"GRUP", *b"TES4"] {
        let model = RecordModel {
            signature,
            flags: 0,
            form_id: 0x800,
            revision: 0,
            version: 131,
            unknown: 0,
            subrecords: &[],
            compress: false,
        };
        assert_eq!(
            writer::encode_record(&model, &limits, &budget)
                .unwrap_err()
                .code,
            "WriterRecordSignature"
        );
    }
    let data = [0u8; 8];
    for names in [
        &["dir/A.esm"][..],
        &["dir\\A.esm"],
        &["\u{e9}.esm"],
        &["A.esm", "a.ESM"],
    ] {
        let masters: Vec<Master<'_>> = names.iter().map(|name| Master { name, data }).collect();
        let model = HeaderModel {
            flags: 0,
            revision: 0,
            version: 131,
            unknown: 0,
            hedr_version: 1.0,
            declared_records: 0,
            next_object_id: 0x800,
            masters: &masters,
            others: &[],
        };
        assert_eq!(
            writer::header(&model, &limits, &budget).unwrap_err().code,
            "WriterMasterName",
            "{names:?}"
        );
    }
}

/// code-3a P3: a header read and written again keeps its other
/// subrecords where they stood, byte for byte; the writer refuses a version,
/// a flag and a subrecord the reader would refuse.
#[test]
fn a_header_read_then_written_keeps_its_other_subrecords() {
    use crate::writer::{self, HeaderModel, Master, Subrecord};
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(4u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let body = [
        sub(b"HEDR", &hedr),
        sub(b"CNAM", b"author\0"),
        sub(b"MAST", b"First.esm\0"),
        sub(b"DATA", &[0x12; 8]),
        sub(b"ONAM", &[1, 2, 3, 4]),
        sub(b"MAST", b"Second.esm\0"),
        sub(b"DATA", &[0x34; 8]),
        sub(b"INTV", &7u32.to_le_bytes()),
        sub(b"INCC", &2u32.to_le_bytes()),
    ]
    .concat();
    let original = record(b"TES4", 0, MASTER, &body);
    let h = header("Own.esm", &original).unwrap();
    assert_eq!(h.raw_subrecords.len(), 4);
    let others = writer::header_others(&h, &original).unwrap();
    assert_eq!(
        others.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        [1, 2, 3, 3]
    );
    let masters: Vec<Master<'_>> = h
        .masters
        .iter()
        .zip(&h.master_data)
        .map(|(name, data)| Master { name, data: *data })
        .collect();
    let model = HeaderModel {
        flags: h.flags,
        // The record header's revision word, which `Header` does not keep.
        revision: 0x0102_0304,
        version: h.form_version,
        unknown: 0,
        hedr_version: h.hedr_version,
        declared_records: h.declared_records,
        next_object_id: h.next_object_id,
        masters: &masters,
        others: &others,
    };
    let limits = Limits::default();
    let budget = Budget::new(1 << 20);
    let written = writer::header(&model, &limits, &budget).unwrap();
    assert_eq!(written, original);
    // What the reader would refuse is refused.
    let bad = [(
        1,
        Subrecord {
            signature: *b"MAST",
            data: b"Third.esm\0",
            extended: false,
        },
    )];
    for (m, code) in [
        (
            HeaderModel {
                hedr_version: 2.0,
                ..model
            },
            "WriterHeaderVersion",
        ),
        (
            HeaderModel {
                flags: 0x100000,
                ..model
            },
            "WriterHeaderFlags",
        ),
        (
            HeaderModel {
                others: &bad,
                ..model
            },
            "WriterHeaderSubrecord",
        ),
        (
            HeaderModel {
                others: &[(
                    4,
                    Subrecord {
                        signature: *b"INTV",
                        data: &[0; 4],
                        extended: false,
                    },
                )],
                ..model
            },
            "WriterHeaderSubrecord",
        ),
    ] {
        let e = writer::header(&m, &limits, &budget).unwrap_err();
        assert_eq!(e.code, code);
    }
}

#[test]
fn enable_parent_chains_are_read_to_the_last_byte_and_end_by_name() {
    use crate::enable::{chain, ChainEnd};
    use crate::typed::Store;
    let xesp = |parent: u32, flags: u8| {
        let mut d = parent.to_le_bytes().to_vec();
        d.extend([flags, 7, 8, 9]);
        sub(b"XESP", &d)
    };
    let placed = |extra: Vec<u8>| {
        let mut p = sub(b"NAME", &0x801u32.to_le_bytes());
        p.extend(extra);
        p
    };
    // 0x802 -> 0x803 (opposite) -> 0x804 (root, disabled); 0x805 <-> 0x806 cycle;
    // 0x807 names a missing record; 0x808 a short XESP.
    let children = [
        record(b"REFR", 0x802, 0, &placed(xesp(0x803, 0))),
        record(b"REFR", 0x803, 0, &placed(xesp(0x804, 1))),
        record(b"REFR", 0x804, DISABLED, &placed(Vec::new())),
        record(b"REFR", 0x805, 0, &placed(xesp(0x806, 0))),
        record(b"REFR", 0x806, 0, &placed(xesp(0x805, 0))),
        record(b"REFR", 0x807, 0, &placed(xesp(0x8ff, 0))),
        record(b"REFR", 0x808, 0, &placed(sub(b"XESP", &[1, 2, 3]))),
    ]
    .concat();
    let cell = record(b"CELL", 0x800, 0, &sub(b"DATA", &1u16.to_le_bytes()));
    let refs = group(
        6,
        0x800u32.to_le_bytes(),
        &group(9, 0x800u32.to_le_bytes(), &children),
    );
    let mut body = group(2, [0; 4], &[cell, refs].concat());
    body.extend(record(b"STAT", 0x801, 0, &sub(b"MODL", b"test.nif\0")));
    let sources: Vec<(String, Box<dyn ReadAt>)> = vec![(
        "base.esm".into(),
        Box::new(OwnedSource(plugin(MASTER, &[], 0, &body))),
    )];
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        sources,
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let k = |id| FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let b = Budget::new(1 << 20);
    let c = chain(&store, &k(0x802), &b).unwrap().unwrap();
    assert_eq!(c.end, ChainEnd::Root);
    assert_eq!(c.depth(), 2);
    let link = c.nodes[0].link.as_ref().unwrap();
    assert_eq!((link.raw, link.flags, link.reserved), (0x803, 0, [7, 8, 9]));
    assert_eq!(c.nodes[1].link.as_ref().unwrap().flags, 1);
    assert!(c.nodes[2].initially_disabled && !c.nodes[0].initially_disabled);
    let c = chain(&store, &k(0x805), &b).unwrap().unwrap();
    assert_eq!(c.end, ChainEnd::Cycle(k(0x805)));
    assert_eq!(c.depth(), 1);
    let c = chain(&store, &k(0x807), &b).unwrap().unwrap();
    assert_eq!(c.end, ChainEnd::MissingParent(k(0x8ff)));
    assert_eq!(
        chain(&store, &k(0x808), &b).unwrap_err().code,
        "EnableParentSize"
    );
    assert!(chain(&store, &k(0x8fe), &b).unwrap().is_none());
    let groups: Vec<u32> = store.placement_groups().map(|(_, _, g)| g).collect();
    assert_eq!(groups, [9; 7]);
}

#[test]
fn topic_children_keep_their_topic_and_stored_order() {
    use crate::typed::Store;
    // A quest whose children group (kind 10) holds two topics, each with
    // a children group of kind 7 holding its lines.
    let topic_a = group(
        7,
        0x901u32.to_le_bytes(),
        &[
            record(b"INFO", 0x903, 0, &sub(b"ENAM", &[0; 4])),
            record(b"INFO", 0x902, 0, &sub(b"ENAM", &[0; 4])),
        ]
        .concat(),
    );
    let topic_b = group(
        7,
        0x904u32.to_le_bytes(),
        &record(b"INFO", 0x905, 0, &sub(b"ENAM", &[0; 4])),
    );
    let children = [
        record(b"DIAL", 0x904, 0, &sub(b"PNAM", &[0; 4])),
        topic_b,
        record(b"DIAL", 0x901, 0, &sub(b"PNAM", &[0; 4])),
        topic_a,
    ]
    .concat();
    let mut body = record(b"QUST", 0x900, 0, &sub(b"EDID", b"Q\0"));
    body.extend(group(10, 0x900u32.to_le_bytes(), &children));
    let body = group(0, *b"QUST", &body);
    let sources: Vec<(String, Box<dyn ReadAt>)> = vec![(
        "base.esm".into(),
        Box::new(OwnedSource(plugin(MASTER, &[], 0, &body))),
    )];
    let store = Store::from_sources(
        fo4_core::Game::Fallout4,
        sources,
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let got: Vec<(u32, u32)> = store
        .topic_children()
        .map(|(o, t)| (o.key as u32, t.object_id))
        .collect();
    // Grouped by topic (lower key first), stored order within a topic.
    assert_eq!(got, [(0x903, 0x901), (0x902, 0x901), (0x905, 0x904)]);
    // The quest group still owns every record below it.
    let owners: Vec<(u32, u32)> = store
        .placement_groups()
        .map(|(_, cell, g)| (cell.object_id, g))
        .collect();
    assert_eq!(owners, [(0x900, 10); 5]);
}

#[test]
fn layer_records_are_read_to_the_last_byte_with_their_list_resolved() {
    let layer = |id: u32, index: u32, list: &[u32], count: usize, extra: &[u8]| {
        let mut d = sub(b"EDID", b"LayerA\0");
        d.extend(sub(b"DESC", &7u32.to_le_bytes()));
        d.extend(sub(b"BNAM", &index.to_le_bytes()));
        d.extend(sub(b"FNAM", &[1, 2, 3, 0]));
        d.extend(sub(b"GNAM", &4u32.to_le_bytes()));
        d.extend(sub(b"MNAM", b"LayerA\0"));
        d.extend(sub(b"INTV", &(count as u32).to_le_bytes()));
        if !list.is_empty() {
            let words: Vec<u8> = list.iter().flat_map(|w| w.to_le_bytes()).collect();
            d.extend(sub(b"CNAM", &words));
        }
        d.extend(extra);
        record(b"COLL", id, 0, &d)
    };
    let mut records = layer(0x800, 0, &[0x800, 0x801], 2, &[]);
    records.extend(layer(0x801, 3, &[0x800], 1, &[]));
    records.extend(layer(0x802, 4, &[], 0, &[]));
    // A list one word short of its count, and a field the layout does not name.
    records.extend(layer(0x803, 5, &[0x800], 2, &[]));
    records.extend(layer(0x804, 6, &[], 0, &sub(b"ZZZZ", &[1])));
    let body = group(0, *b"COLL", &records);
    let source = plugin(MASTER, &[], 0, &body);
    let store = crate::typed::Store::from_sources(
        fo4_core::Game::Fallout4,
        vec![("base.esm".into(), Box::new(OwnedSource(source)))],
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    let b = Budget::new(8 << 20);
    let key = |id: u32| FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let one = |id: u32| {
        let (r, p) = store.raw_with_plugin(&key(id), &b).unwrap().unwrap();
        crate::coll::read(&r, key(id), p, &store, &b)
    };
    let a = one(0x800).unwrap();
    assert_eq!(a.index, 0);
    assert_eq!(a.editor_id, b"LayerA");
    assert_eq!(a.name, b"LayerA");
    assert_eq!(a.description, Some(7));
    assert_eq!(a.color, [1, 2, 3, 0]);
    assert_eq!(a.flags, 4);
    assert_eq!(a.count, 2);
    assert_eq!(a.raw_collides, [0x800, 0x801]);
    assert_eq!(a.collides, [key(0x800), key(0x801)]);
    assert!(one(0x802).unwrap().collides.is_empty());
    assert_eq!(one(0x803).unwrap_err().code, "LayerListCount");
    assert_eq!(one(0x804).unwrap_err().code, "UnknownLayerField");
    // The load order's layers: the first refusal stops the read.
    assert_eq!(
        crate::coll::read_all(&store, &b).unwrap_err().code,
        "LayerListCount"
    );
    let three: Vec<_> = [0x800, 0x801, 0x802].map(|id| one(id).unwrap()).to_vec();
    let r = crate::coll::relation(&three).unwrap();
    assert_eq!(r[&0], std::collections::BTreeSet::from([0, 3]));
    assert_eq!(r[&3], std::collections::BTreeSet::from([0]));
    assert!(r[&4].is_empty());
    assert!(crate::coll::one_way(&r).is_empty());
}
