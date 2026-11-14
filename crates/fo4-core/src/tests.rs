use super::*;
use std::io::Write;
fn synthetic_numbers() -> Vec<u8> {
    let mut b = vec![0x13];
    b.extend(0x1234u16.to_le_bytes());
    b.extend(0x12345678u32.to_le_bytes());
    b.extend(0x123456789abcdef0u64.to_le_bytes());
    b.extend(1.25f32.to_le_bytes());
    b
}
#[test]
fn cursor_each_field_and_every_truncation() {
    let bytes = synthetic_numbers();
    for n in 0..=bytes.len() {
        let source = SliceSource {
            name: "numbers",
            bytes: &bytes[..n],
        };
        let parsed = (|| -> Result<()> {
            let mut c = Cursor::new(&source);
            assert_eq!(c.u8()?, 0x13);
            assert_eq!(c.u16()?, 0x1234);
            assert_eq!(c.u32()?, 0x12345678);
            assert_eq!(c.u64()?, 0x123456789abcdef0);
            assert_eq!(c.f32()?, 1.25);
            Ok(())
        })();
        assert_eq!(parsed.is_ok(), n == bytes.len());
        if let Err(e) = parsed {
            assert_eq!(e.source.file, "numbers");
            assert_eq!(e.code, "Truncated");
        }
    }
}
#[test]
fn overflow_and_memory_caps() {
    assert_eq!(add(u64::MAX, 1, "a", 5).unwrap_err().code, "Overflow");
    assert!(mul(u64::MAX, 2, "a", 0).is_err());
    let b = Budget::new(31);
    assert_eq!(b.bytes(32, "a", 0).unwrap_err().code, "LimitExceeded");
    assert_eq!(b.used(), 0);
    let mut v = Vec::<u64>::new();
    b.reserve(&mut v, 3, "a", 0).unwrap();
    v.resize(3, 0);
    assert!(b.reserve(&mut v, 1, "a", 0).is_err());
    assert!(b.used() <= 31);
    assert!(Limits {
        max_depth: 257,
        ..Limits::default()
    }
    .validate("limits")
    .is_err());
}
fn compressed(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
#[test]
fn exact_zlib_including_bombs_checksum_and_trailing() {
    let data = vec![b'a'; 8192];
    let packed = compressed(&data);
    let l = Limits {
        max_payload: 8192,
        ..Limits::default()
    };
    assert_eq!(
        zlib(&packed, 8192, &l, &Budget::new(9000), "z", 0).unwrap(),
        data
    );
    for n in [0, 8191, 8193, 1_000_000] {
        assert!(zlib(&packed, n, &l, &Budget::new(9000), "z", 0).is_err());
    }
    for n in 0..packed.len() {
        assert!(zlib(&packed[..n], 8192, &l, &Budget::new(9000), "z", 0).is_err());
    }
    let mut extra = packed.clone();
    extra.push(0);
    assert!(zlib(&extra, 8192, &l, &Budget::new(9000), "z", 0).is_err());
    let mut corrupt = packed;
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(zlib(&corrupt, 8192, &l, &Budget::new(9000), "z", 0).is_err());
}
#[test]
fn path_policy() {
    assert_eq!(path_key("Meshes\\A.NIF").unwrap(), "meshes/a.nif");
    for p in [
        "", ".", "../x", "a/../x", "/a", "C:\\x", "a//b", "a\0b", "á", "a/./b",
    ] {
        assert!(path_key(p).is_err(), "{p}");
    }
}
fn temp() -> TestDir {
    TestDir::new("core")
}
#[test]
fn stable_range_io_hash_and_change_detection() {
    let p = temp();
    let f = p.join("f");
    std::fs::write(&f, b"abcdef").unwrap();
    let s = FileSource::open(&f).unwrap();
    let mut b = [0; 2];
    s.read_exact_at(2, &mut b).unwrap();
    assert_eq!(&b, b"cd");
    assert!(s.read_exact_at(u64::MAX, &mut b).is_err());
    assert_eq!(digest(&s).unwrap(), digest_bytes(b"abcdef"));
    std::fs::write(&f, b"ABCDEF").unwrap();
    assert_eq!(s.check().unwrap_err().code, "FileChanged");
}
#[cfg(unix)]
#[test]
fn symlink_escape() {
    let p = temp();
    std::fs::create_dir(p.join("root")).unwrap();
    std::fs::write(p.join("outside"), b"generated").unwrap();
    std::os::unix::fs::symlink(p.join("outside"), p.join("root/escape")).unwrap();
    assert_eq!(
        within(&p.join("root"), "escape").unwrap_err().code,
        "PathTraversal"
    );
}
#[test]
fn manifest_requires_explicit_order() {
    let raw=br#"{"root":".","language":"english","plugins":[],"mounts":[],"required_masters":[],"excluded":[],"build":null}"#;
    let m: Manifest = serde_json::from_slice(raw).unwrap();
    assert_eq!(m.validate("m").unwrap_err().code, "LoadOrderRequired");
    let unknown=br#"{"root":".","language":"english","plugins":[],"mounts":[],"required_masters":[],"excluded":[],"build":null,"mtime_order":true}"#;
    assert!(serde_json::from_slice::<Manifest>(unknown).is_err());
    for n in 0..raw.len() {
        assert!(serde_json::from_slice::<Manifest>(&raw[..n]).is_err());
    }
}

#[test]
fn scoped_budgets_share_one_ceiling_and_release_only_their_charges() {
    let parent = Budget::new(1024);
    let retained = parent.bytes(768, "retained", 0).unwrap();
    {
        let transient = parent.child();
        assert_eq!(transient.remaining(), 256);
        assert!(transient.bytes(257, "too much", 0).is_err());
        let bytes = transient.bytes(256, "payload", 0).unwrap();
        assert_eq!(parent.used(), 1024);
        drop(bytes);
    }
    assert_eq!(parent.used(), 768);
    assert_eq!(retained.len(), 768);
}

#[test]
fn limits_can_exceed_initial_defaults() {
    let limits = Limits {
        max_records: 5_000_000,
        max_subrecords: 50_000_000,
        max_memory: 2 << 30,
        max_string: 1 << 20,
        ..Limits::default()
    };
    assert!(limits.validate("raised").is_ok());
}
#[test]
fn short_stream_does_not_allocate_its_large_declaration() {
    let packed = compressed(b"tiny");
    let budget = Budget::new(512 << 20);
    assert!(zlib(&packed, 256 << 20, &Limits::default(), &budget, "bomb", 0).is_err());
    assert!(
        budget.used() < 1 << 20,
        "charged {} for tiny stream",
        budget.used()
    );
}

#[test]
fn temporary_guard_cleans_on_unwind() {
    let mut path = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = TestDir::new("unwind");
        path = Some(dir.to_path_buf());
        assert!(dir.starts_with(
            std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
        ));
        std::fs::write(dir.join("bytes"), b"synthetic").unwrap();
        panic!("synthetic unwind");
    }));
    assert!(result.is_err());
    assert!(!path.unwrap().exists());
}

#[test]
fn incremental_output_crosses_many_chunks_without_exceeding_declaration() {
    let data: Vec<u8> = (0..(2 * 1024 * 1024 + 19))
        .map(|i| ((i * 17) % 251) as u8)
        .collect();
    let packed = compressed(&data);
    let budget = Budget::new(16 << 20);
    let out = zlib(
        &packed,
        data.len() as u64,
        &Limits::default(),
        &budget,
        "large-zlib",
        0,
    )
    .unwrap();
    assert_eq!(out, data);
    assert!(out.capacity() <= data.len());
    for n in [data.len() - 1, data.len() + 1] {
        assert!(zlib(
            &packed,
            n as u64,
            &Limits::default(),
            &Budget::new(16 << 20),
            "mismatch",
            0
        )
        .is_err());
    }
}

#[test]
fn position_recovers_after_partial_read_error() {
    struct Failing {
        inner: std::io::Cursor<Vec<u8>>,
        phase: u8,
    }
    impl std::io::Read for Failing {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.phase == 1 {
                self.phase = 2;
                return Err(std::io::Error::other("injected read failure"));
            }
            let n = if self.phase == 0 {
                self.phase = 1;
                out.len().min(2)
            } else {
                out.len()
            };
            std::io::Read::read(&mut self.inner, &mut out[..n])
        }
    }
    impl std::io::Seek for Failing {
        fn seek(&mut self, p: std::io::SeekFrom) -> std::io::Result<u64> {
            std::io::Seek::seek(&mut self.inner, p)
        }
    }
    let mut reader = PositionedReader::new(Failing {
        inner: std::io::Cursor::new(b"abcdefghijkl".to_vec()),
        phase: 0,
    });
    let mut out = [0; 4];
    assert!(reader.read_at(2, &mut out).is_err());
    let mut recovered = [0; 2];
    reader.read_at(4, &mut recovered).unwrap();
    assert_eq!(&recovered, b"ef");
}

#[test]
fn position_recovers_after_moving_seek_error() {
    struct FailingSeek {
        inner: std::io::Cursor<Vec<u8>>,
        seeks: u8,
    }
    impl std::io::Read for FailingSeek {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            std::io::Read::read(&mut self.inner, out)
        }
    }
    impl std::io::Seek for FailingSeek {
        fn seek(&mut self, p: std::io::SeekFrom) -> std::io::Result<u64> {
            self.seeks += 1;
            let position = std::io::Seek::seek(&mut self.inner, p)?;
            if self.seeks == 2 {
                Err(std::io::Error::other("seek moved then failed"))
            } else {
                Ok(position)
            }
        }
    }
    let mut reader = PositionedReader::new(FailingSeek {
        inner: std::io::Cursor::new(b"abcdefghijkl".to_vec()),
        seeks: 0,
    });
    let mut out = [0; 2];
    reader.read_at(0, &mut out).unwrap();
    assert!(reader.read_at(6, &mut out).is_err());
    reader.read_at(4, &mut out).unwrap();
    assert_eq!(&out, b"ef");
}

#[test]
fn invalidation_ini_requires_file_and_one_based_line() {
    let json = r#"{"root":"data","language":"en","plugins":[{"name":"A.esm","path":"A.esm"}],"mounts":[],"required_masters":[],"excluded":[],"archive_invalidation":{"enabled":true,"origin":{"kind":"ini","file":"Fallout4Custom.ini","line":0}}}"#;
    let mut m: Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(
        m.validate("manifest").unwrap_err().code,
        "InvalidConfigOrigin"
    );
    m.archive_invalidation.as_mut().unwrap().origin = InvalidationOrigin::Ini {
        file: "Fallout4Custom.ini".into(),
        line: 1,
    };
    m.validate("manifest").unwrap();
}

#[test]
fn byte_keys_fold_only_ascii_and_reject_unsafe_components() {
    assert_eq!(
        path_key_bytes(b"A\\Mar\xeda.fuz").unwrap(),
        b"a/mar\xeda.fuz"
    );
    assert_ne!(
        path_key_bytes(b"a/\xc0").unwrap(),
        path_key_bytes(b"a/\xe0").unwrap()
    );
    for name in [
        b"".as_slice(),
        b"a//b",
        b"a/./b",
        b"a/../b",
        b"a:b",
        b"a\0b",
        b"a\0",
        b"/a",
    ] {
        assert!(path_key_bytes(name).is_err());
    }
    for byte in 128..=255 {
        assert_eq!(path_key_bytes(&[byte]).unwrap(), [byte]);
    }
}

#[test]
fn manifest_game_is_explicit_and_legacy_defaults_to_fallout4() {
    let base = serde_json::json!({"root":"Data","language":"en","plugins":[{"name":"Base.esm","path":"Base.esm"}],"mounts":[],"required_masters":[],"excluded":[]});
    let legacy: Manifest = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(serde_json::to_value(&legacy).unwrap()["game"], "fallout4");
    let mut sse = base.clone();
    sse["game"] = "skyrim-se".into();
    let sse: Manifest = serde_json::from_value(sse).unwrap();
    assert_eq!(serde_json::to_value(&sse).unwrap()["game"], "skyrim-se");
    let mut unknown = base;
    unknown["game"] = "auto".into();
    assert!(serde_json::from_value::<Manifest>(unknown).is_err());
}

#[test]
fn cursor_preserves_nonfinite_ieee_bits() {
    for bits in [
        0x7fc12345u32,
        0xffc54321,
        0x7f800000,
        0xff800000,
        0x80000000,
    ] {
        let bytes = bits.to_le_bytes();
        let src = SliceSource {
            name: "ieee",
            bytes: &bytes,
        };
        assert_eq!(Cursor::new(&src).f32().unwrap().to_bits(), bits);
    }
}

#[test]
fn ieee_json_never_erases_nonfinite_payloads() {
    #[derive(serde::Serialize)]
    struct Sample {
        #[serde(serialize_with = "crate::ieee::serialize")]
        values: Option<Vec<[f32; 2]>>,
    }
    let s = Sample {
        values: Some(vec![
            [f32::from_bits(0xffc12345), f32::INFINITY],
            [1.25, f32::NEG_INFINITY],
        ]),
    };
    let value = serde_json::to_value(&s).unwrap();
    assert_eq!(value["values"][0][0]["ieee754_bits"], 0xffc12345u32);
    assert_eq!(value["values"][0][1]["ieee754_bits"], 0x7f800000u32);
    assert_eq!(value["values"][1][0], 1.25);
    assert_eq!(crate::ieee::count(&s.values), 3);
}

#[test]
fn allocation_meter_separates_live_credits_and_scoped_charges() {
    use crate::AllocationKind;
    let b = Budget::metered(1000);
    b.measure(AllocationKind::Reference, || b.charge(100, "synthetic", 1))
        .unwrap();
    {
        let scratch = b.child();
        scratch
            .measure(AllocationKind::AssetGraph, || {
                scratch.charge(800, "synthetic", 2)
            })
            .unwrap();
        assert_eq!(b.used(), 900);
        assert_eq!(
            scratch
                .measure(AllocationKind::Diagnostics, || scratch.charge(
                    101,
                    "synthetic",
                    3
                ))
                .unwrap_err()
                .code,
            "LimitExceeded"
        );
    }
    let m = b.measurements().unwrap();
    assert_eq!(m.used, 100);
    assert_eq!(m.peak, 900);
    assert_eq!(m.categories[5].live_credits, 0);
    assert_eq!(m.categories[5].cumulative_charged, 800);
    assert_eq!(m.rejected_category, Some("diagnostics"));
    assert_eq!(m.rejected_requested_bytes, Some(101));
}
