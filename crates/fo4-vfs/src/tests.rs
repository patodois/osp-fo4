use super::*;
use fo4_core::{Limits, PluginSpec};

fn temp() -> fo4_core::TestDir {
    fo4_core::TestDir::new("vfs")
}
fn archive(path: &str, data: &[u8]) -> Vec<u8> {
    let mut b = b"BTDX".to_vec();
    b.extend(1u32.to_le_bytes());
    b.extend(b"GNRL");
    b.extend(1u32.to_le_bytes());
    b.extend((60u64 + data.len() as u64).to_le_bytes());
    b.extend(123u32.to_le_bytes());
    b.extend(b"bin\0");
    b.extend(456u32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(60u64.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(0xBAADF00Du32.to_le_bytes());
    b.extend(data);
    b.extend((path.len() as u16).to_le_bytes());
    b.extend(path.as_bytes());
    b
}
fn installation() -> (Manifest, fo4_core::TestDir) {
    let root = temp();
    std::fs::write(root.join("A.esm"), b"synthetic plugin bytes").unwrap();
    std::fs::write(root.join("a.ba2"), archive("Meshes\\A.bin", b"archive a")).unwrap();
    std::fs::write(root.join("b.ba2"), archive("meshes/a.bin", b"archive b")).unwrap();
    for (base, data) in [("one", b"loose one"), ("two", b"loose two")] {
        std::fs::create_dir_all(root.join(base).join("meshes")).unwrap();
        std::fs::write(root.join(base).join("meshes/a.bin"), data).unwrap();
    }
    let manifest = Manifest {
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
        mounts: vec![
            Mount::Archive {
                path: "a.ba2".into(),
                plugin: None,
            },
            Mount::Archive {
                path: "b.ba2".into(),
                plugin: Some("A.esm".into()),
            },
            Mount::Loose {
                base: Some("one".into()),
                files: vec!["meshes/a.bin".into()],
                plugin: None,
            },
            Mount::Loose {
                base: Some("two".into()),
                files: vec!["meshes/a.bin".into()],
                plugin: None,
            },
        ],
        required_masters: vec![],
        excluded: vec![],
        limits: Limits::default(),
        build: None,
    };
    (manifest, root)
}
#[test]
fn two_archives_two_loose_layers_and_inversion() {
    let (mut m, _dir) = installation();
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.chain("MESHES\\a.BIN").unwrap().len(), 4);
    assert_eq!(
        v.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap(),
        b"loose two"
    );
    let before = v.fingerprint().unwrap().to_owned();
    m.mounts[..2].reverse();
    m.mounts[2..].reverse();
    let w = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        w.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap(),
        b"loose one"
    );
    assert_ne!(before, w.fingerprint().unwrap());
    assert_eq!(
        v.hash(&v.chain("meshes/a.bin").unwrap()[0]).unwrap(),
        digest_bytes(b"archive a")
    );
}
#[test]
fn inactive_archive_does_not_need_to_exist() {
    let (mut m, _dir) = installation();
    m.mounts.push(Mount::Archive {
        path: "missing.ba2".into(),
        plugin: Some("inactive.esp".into()),
    });
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.inactive_mounts, vec![4]);
}
#[test]
fn separate_directories_and_missing_names() {
    let (mut m, _dir) = installation();
    std::fs::create_dir_all(m.root.join("two/other")).unwrap();
    std::fs::write(m.root.join("two/other/a.bin"), b"other").unwrap();
    m.mounts.push(Mount::Loose {
        base: Some("two".into()),
        files: vec!["other/a.bin".into()],
        plugin: None,
    });
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        v.read("other/a.bin", &Budget::new(1 << 20)).unwrap(),
        b"other"
    );
    assert_eq!(v.winner("missing").unwrap_err().code, "MissingPath");
    assert!(v.chain("../x").is_err());
    assert!(v.chain("").is_err());
}
#[test]
fn malformed_archive_every_prefix_propagates_named_error() {
    let (m, _dir) = installation();
    let full = archive("x.bin", b"artificial");
    for n in 0..full.len() {
        std::fs::write(m.root.join("a.ba2"), &full[..n]).unwrap();
        let b = Budget::new(1 << 20);
        let v = Vfs::mount(&m, &b).unwrap();
        assert_eq!(v.diagnostics.len(), 1);
        assert_eq!(v.diagnostics[0].code, "Truncated");
        assert!(v.diagnostics[0].source.file.ends_with("a.ba2"));
        assert_eq!(v.archives.len(), 1);
        assert_eq!(v.candidates.len(), 3);
        assert!(b.used() <= 1 << 20);
    }
}
#[test]
fn metadata_count_and_budget_limits() {
    let (mut m, _dir) = installation();
    m.limits.max_entries = 1;
    assert_eq!(
        Vfs::mount(&m, &Budget::new(1 << 20)).err().unwrap().code,
        "LimitExceeded"
    );
    let (m, _dir) = installation();
    assert_eq!(
        Vfs::mount(&m, &Budget::new(16)).err().unwrap().code,
        "LimitExceeded"
    );
}
#[cfg(unix)]
#[test]
fn loose_symlink_outside_root_is_rejected() {
    let (mut m, _dir) = installation();
    std::os::unix::fs::symlink(m.root.join("a.ba2"), m.root.join("one/escape")).unwrap();
    m.mounts = vec![Mount::Loose {
        base: Some("one".into()),
        files: vec!["escape".into()],
        plugin: None,
    }];
    assert_eq!(
        Vfs::mount(&m, &Budget::new(1 << 20)).err().unwrap().code,
        "PathTraversal"
    );
}

#[test]
fn review_n8_asset_fingerprint_ignores_plugin_content() {
    let (m, _dir) = installation();
    let first = Vfs::mount(&m, &Budget::new(1 << 20))
        .unwrap()
        .fingerprint()
        .unwrap()
        .to_owned();
    std::fs::write(m.root.join("A.esm"), b"changed plugin").unwrap();
    let second = Vfs::mount(&m, &Budget::new(1 << 20))
        .unwrap()
        .fingerprint()
        .unwrap()
        .to_owned();
    assert_eq!(first, second);
}
#[test]
fn review_m3_loose_precedence_survives_cross_class_reordering() {
    let (mut m, _dir) = installation();
    m.mounts.reverse();
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        v.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap(),
        b"loose one"
    );
}
#[test]
fn review_h2_bad_entry_does_not_remove_other_archive_assets() {
    let (mut m, _dir) = installation();
    std::fs::write(m.root.join("a.ba2"), shared_archive()).unwrap();
    m.mounts.truncate(2);
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.diagnostics.len(), 2);
    assert_eq!(
        v.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap(),
        b"archive b"
    );
}

#[test]
#[ignore = "explicit synthetic scale measurement"]
fn measure_vfs_index() {
    let directory = fo4_core::TestDir::new("vfs-scale");
    std::fs::write(directory.join("A.esm"), b"synthetic plugin").unwrap();
    for n in [10_000usize, 100_000] {
        let mut names = Vec::new();
        let mut index = Vec::new();
        for i in 0..n {
            index.extend([0u8; 16]);
            index.extend(((24 + 36 * n + i) as u64).to_le_bytes().iter().copied());
            index.extend(0u32.to_le_bytes());
            index.extend(1u32.to_le_bytes());
            index.extend(0xBAADF00Du32.to_le_bytes());
            let name = format!("meshes/synthetic/{i:08}.nif");
            names.extend((name.len() as u16).to_le_bytes());
            names.extend(name.as_bytes());
        }
        let mut bytes = b"BTDX".to_vec();
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(b"GNRL");
        bytes.extend((n as u32).to_le_bytes());
        bytes.extend(((24 + 36 * n + n) as u64).to_le_bytes());
        bytes.extend(index);
        bytes.extend(vec![0; n]);
        bytes.extend(names);
        std::fs::write(directory.join("scale.ba2"), &bytes).unwrap();
        let m = Manifest {
            game: fo4_core::Game::Fallout4,
            archive_invalidation: Some(fo4_core::ArchiveInvalidation {
                enabled: true,
                origin: fo4_core::InvalidationOrigin::ExplicitProfile,
            }),
            root: directory.to_path_buf(),
            language: "english".into(),
            plugins: vec![PluginSpec {
                name: "A.esm".into(),
                path: "A.esm".into(),
            }],
            mounts: vec![Mount::Archive {
                path: "scale.ba2".into(),
                plugin: None,
            }],
            limits: Limits::default(),
            required_masters: vec![],
            excluded: vec![],
            build: None,
        };
        let budget = Budget::new(512 << 20);
        let start = std::time::Instant::now();
        let vfs = Vfs::mount(&m, &budget).unwrap();
        println!(
            "VFS n={n} budget={} bytes_per_entry={:.3} index_ms={} metadata_checks={}",
            budget.used(),
            budget.used() as f64 / n as f64,
            start.elapsed().as_millis(),
            vfs.archives[0].source.metadata_checks()
        );
        assert_eq!(vfs.candidates.len(), n);
    }
}

fn shared_archive() -> Vec<u8> {
    let mut b = b"BTDX".to_vec();
    b.extend(1u32.to_le_bytes());
    b.extend(b"GNRL");
    b.extend(2u32.to_le_bytes());
    b.extend(99u64.to_le_bytes());
    for _ in 0..2 {
        b.extend([0; 16]);
        b.extend(96u64.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        b.extend(0xBAADF00Du32.to_le_bytes());
    }
    b.extend(b"MOD");
    for name in ["meshes/a.bin", "meshes/b.bin"] {
        b.extend((name.len() as u16).to_le_bytes());
        b.extend(name.as_bytes());
    }
    b
}
#[test]
fn review_n1_blocked_winner_never_promotes_loser() {
    let (mut m, _dir) = installation();
    m.mounts.truncate(2);
    std::fs::write(m.root.join("b.ba2"), shared_archive()).unwrap();
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.chain("meshes/a.bin").unwrap().len(), 2);
    assert_eq!(
        v.winner("meshes/a.bin").unwrap_err().code,
        "WinnerUnsupported"
    );
    assert_eq!(
        v.read("meshes/a.bin", &Budget::new(1 << 20))
            .unwrap_err()
            .code,
        "WinnerUnsupported"
    );
}
#[test]
fn review_n1_structural_failure_taints_every_lookup() {
    let (m, _dir) = installation();
    std::fs::write(m.root.join("b.ba2"), b"BTDX").unwrap();
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.winner("meshes/a.bin").unwrap_err().code, "VfsIncomplete");
    assert_eq!(v.chain("unknown").unwrap_err().code, "VfsIncomplete");
}
#[test]
fn review_n4_collision_requires_explicit_invalidation() {
    let (mut m, _dir) = installation();
    m.archive_invalidation = None;
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(
        v.winner("meshes/a.bin").unwrap_err().code,
        "UnsupportedMountRule"
    );
}
#[test]
fn review_n8_asset_resolution_does_not_require_plugin_files() {
    let (m, _dir) = installation();
    std::fs::remove_file(m.root.join("A.esm")).unwrap();
    assert!(Vfs::mount(&m, &Budget::new(1 << 20)).is_ok());
}

#[test]
fn review_r4_unkeyed_blocked_entries_taint_every_lookup() {
    for name in [
        "meshes/a:bin".to_string(),
        "meshes/./a.bin".to_string(),
        format!("meshes/{}.bin", "x".repeat(64)),
    ] {
        let (mut m, _dir) = installation();
        m.mounts.truncate(2);
        m.limits.max_string = 32;
        std::fs::write(m.root.join("a.ba2"), archive(&name, b"unknown name")).unwrap();
        let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
        assert!(v.candidates.iter().any(|c| c.key.is_none() && c.blocked));
        assert_eq!(
            v.read("meshes/a.bin", &Budget::new(1 << 20))
                .unwrap_err()
                .code,
            "VfsIncomplete"
        );
        assert_eq!(v.winner("unrelated").unwrap_err().code, "VfsIncomplete");
        assert!(v.incomplete);
    }
}

#[test]
fn stage7_observed_names_use_original_bytes_and_ascii_case_only() {
    for (name, display) in [
        (
            b"Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\Mar\xeda_F.fuz".as_slice(),
            "Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\María_F.fuz",
        ),
        (
            b"Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\Mar\xeda_M.fuz".as_slice(),
            "Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\María_M.fuz",
        ),
        (
            b"Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\S\xe1nchez_F.fuz".as_slice(),
            "Sound\\Voice\\Fallout4.esm\\RobotMrHandy\\Sánchez_F.fuz",
        ),
    ] {
        let (mut m, _dir) = installation();
        m.mounts.truncate(1);
        m.mounts.push(Mount::Archive {
            path: "a.ba2".into(),
            plugin: Some("A.esm".into()),
        });
        let placeholder = String::from_utf8(
            name.iter()
                .map(|b| if *b >= 128 { b'x' } else { *b })
                .collect(),
        )
        .unwrap();
        let mut bytes = archive(&placeholder, b"synthetic sound");
        let start = bytes.len() - name.len();
        bytes[start..].copy_from_slice(name);
        std::fs::write(m.root.join("a.ba2"), bytes).unwrap();
        let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
        assert!(!v.incomplete);
        assert_eq!(
            v.read_path_bytes(name, &Budget::new(1 << 20)).unwrap(),
            b"synthetic sound"
        );
        assert_eq!(v.candidates[0].original, display);
        assert_eq!(v.candidates[0].original_encoding, "windows-1252");
        assert_eq!(v.original_bytes(&v.candidates[0]).unwrap(), name);
        assert_eq!(
            v.chain_bytes(&name.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>())
                .unwrap()
                .len(),
            2
        );
    }
}

#[test]
fn stage8_game_profile_is_part_of_asset_fingerprint() {
    let (mut m, _dir) = installation();
    // Loose-only profiles have identical asset bytes under both game selections.
    m.mounts
        .retain(|mount| matches!(mount, Mount::Loose { .. }));
    let a = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    m.game = fo4_core::Game::SkyrimSe;
    let b = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_ne!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
    assert_eq!(
        a.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap(),
        b.read("meshes/a.bin", &Budget::new(1 << 20)).unwrap()
    );
    assert_eq!(m.game.creation_list(), "Skyrim.ccc");
    assert_eq!(m.game.fixed_masters().len(), 5);
}

#[test]
fn deferred_fingerprint_equals_the_eagerly_assembled_stamp() {
    fn frame(stamp: &mut Vec<u8>, bytes: &[u8]) {
        stamp.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        stamp.extend_from_slice(bytes);
    }
    let (m, _dir) = installation();
    let file = |relative: &str| digest_bytes(&std::fs::read(m.root.join(relative)).unwrap());
    let mut stamp = Vec::new();
    stamp.extend_from_slice(b"bethesda-vfs:assets:8\0");
    frame(&mut stamp, b"fallout4");
    frame(&mut stamp, &[1]);
    frame(&mut stamp, b"english");
    frame(&mut stamp, b"A.esm");
    for (layer, archive) in ["a.ba2", "b.ba2"].into_iter().enumerate() {
        frame(&mut stamp, &layer.to_le_bytes());
        frame(&mut stamp, archive.as_bytes());
        frame(&mut stamp, file(archive).as_bytes());
    }
    for (layer, base) in [(2usize, "one"), (3, "two")] {
        frame(&mut stamp, &layer.to_le_bytes());
        frame(&mut stamp, base.as_bytes());
        frame(&mut stamp, b"meshes/a.bin");
        frame(&mut stamp, file(&format!("{base}/meshes/a.bin")).as_bytes());
    }
    frame(&mut stamp, &[0]);
    let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_eq!(v.fingerprint().unwrap(), digest_bytes(&stamp));
    // Cached: the same value without rereading, even after the bytes change on disk.
    let first = v.fingerprint().unwrap().to_owned();
    std::fs::write(m.root.join("two/meshes/a.bin"), b"loose 2!!").unwrap();
    assert_eq!(v.fingerprint().unwrap(), first);
    // A new mount sees the changed content.
    let w = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
    assert_ne!(w.fingerprint().unwrap(), first);
}
