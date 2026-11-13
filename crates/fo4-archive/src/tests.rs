use super::*;
use std::{cell::RefCell, io::Write};
fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend(x.to_le_bytes());
}
fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend(x.to_le_bytes());
}
fn put64(v: &mut Vec<u8>, x: u64) {
    v.extend(x.to_le_bytes());
}
fn packed(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(data).unwrap();
    z.finish().unwrap()
}
fn general(version: u32, files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut table = Vec::new();
    let mut data = Vec::new();
    for (name, bytes, compressed) in files {
        put32(&mut table, 0x12345678);
        table.extend(b"bin\0");
        put32(&mut table, 0x9abcdef0);
        put32(&mut table, 0x1234);
        put64(&mut table, (24 + 36 * files.len() + data.len()) as u64);
        let raw = if *compressed {
            packed(bytes)
        } else {
            bytes.to_vec()
        };
        put32(&mut table, if *compressed { raw.len() as u32 } else { 0 });
        put32(&mut table, bytes.len() as u32);
        put32(&mut table, 0xBAADF00D);
        data.extend(raw);
        assert!(!name.is_empty());
    }
    let mut out = b"BTDX".to_vec();
    put32(&mut out, version);
    out.extend(b"GNRL");
    put32(&mut out, files.len() as u32);
    put64(&mut out, (24 + table.len() + data.len()) as u64);
    out.extend(table);
    out.extend(data);
    for (name, _, _) in files {
        put16(&mut out, name.len() as u16);
        out.extend(name.as_bytes());
    }
    out
}
fn texture(version: u32, cube: bool, compressed: bool, multi: bool) -> (Vec<u8>, Vec<u8>) {
    let mut table = Vec::new();
    put32(&mut table, 0x1234);
    table.extend(b"dds\0");
    put32(&mut table, 0x8765);
    table.push(0);
    table.push(if multi { 3 } else { 1 });
    put16(&mut table, 24);
    put16(&mut table, if cube { 8 } else { 4 });
    put16(&mut table, 8);
    table.push(3);
    table.push(71);
    table.push(cube as u8);
    table.push(8);
    let faces = if cube { 6 } else { 1 };
    let mut payload = Vec::new();
    let mut expected = Vec::new();
    let mut chunks = Vec::new();
    let sizes = if cube { [32, 8, 8] } else { [16, 8, 8] };
    for face in 0..faces {
        for (mip, size) in sizes.into_iter().enumerate() {
            expected.extend(vec![10 * face + mip as u8; size]);
        }
    }
    if multi {
        let mut idx = 0;
        for (mip, size) in sizes.into_iter().enumerate() {
            let raw = expected[idx..idx + size].to_vec();
            chunks.push((mip as u16, mip as u16, raw));
            idx += size;
        }
    } else {
        chunks.push((0, 2, expected.clone()));
    }
    let base = 24 + 24 + 24 * chunks.len();
    for (first, last, raw) in chunks {
        let stored = if compressed {
            packed(&raw)
        } else {
            raw.clone()
        };
        put64(&mut table, (base + payload.len()) as u64);
        put32(&mut table, if compressed { stored.len() as u32 } else { 0 });
        put32(&mut table, raw.len() as u32);
        put16(&mut table, first);
        put16(&mut table, last);
        put32(&mut table, 0xBAADF00D);
        payload.extend(stored);
    }
    let mut out = b"BTDX".to_vec();
    put32(&mut out, version);
    out.extend(b"DX10");
    put32(&mut out, 1);
    put64(&mut out, (24 + table.len() + payload.len()) as u64);
    out.extend(table);
    out.extend(payload);
    let name = b"Textures\\Test.dds";
    put16(&mut out, name.len() as u16);
    out.extend(name);
    (out, expected)
}
fn parse(bytes: &[u8]) -> Result<Archive> {
    Archive::read(
        &fo4_core::SliceSource {
            name: "synthetic.ba2",
            bytes,
        },
        &Limits::default(),
        &Budget::new(2 << 20),
    )
}
#[test]
fn gnrl_all_versions_raw_and_zlib() {
    for version in [1, 7, 8] {
        let bytes = general(
            version,
            &[
                ("A/One.bin", b"abcdef", false),
                ("B/Two.bin", b"defghijklmnopqrstuvwxyz", true),
            ],
        );
        let a = parse(&bytes).unwrap();
        assert_eq!(a.entries[0].key, b"a/one.bin");
        assert_eq!(a.entries[0].name_hash, 0x12345678);
        assert_eq!(a.entries[0].raw_flags, Some(0x1234));
        let s = fo4_core::SliceSource {
            name: "s",
            bytes: &bytes,
        };
        for (i, expected) in [b"abcdef".as_slice(), b"defghijklmnopqrstuvwxyz"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                a.read_entry(&s, i, &Limits::default(), &Budget::new(2 << 20))
                    .unwrap(),
                expected
            );
        }
    }
}
#[test]
fn dx10_versions_chunks_and_dds_fields() {
    for v in [1, 7, 8] {
        for compressed in [false, true] {
            for multi in [false, true] {
                let (bytes, expected) = texture(v, false, compressed, multi);
                let a = parse(&bytes).unwrap();
                let s = fo4_core::SliceSource {
                    name: "s",
                    bytes: &bytes,
                };
                let dds = a
                    .read_entry(&s, 0, &Limits::default(), &Budget::new(2 << 20))
                    .unwrap();
                assert_eq!(&dds[..4], b"DDS ");
                assert_eq!(&dds[84..88], b"DX10");
                assert_eq!(u32::from_le_bytes(dds[12..16].try_into().unwrap()), 4);
                for (offset, value) in [
                    (4, 124),
                    (8, 0xa1007),
                    (16, 8),
                    (20, 16),
                    (76, 32),
                    (80, 4),
                    (108, 0x401008),
                    (112, 0),
                    (132, 3),
                    (136, 0),
                    (140, 1),
                ] {
                    assert_eq!(
                        u32::from_le_bytes(dds[offset..offset + 4].try_into().unwrap()),
                        value,
                        "DDS field {offset}"
                    );
                }
                assert_eq!(u32::from_le_bytes(dds[28..32].try_into().unwrap()), 3);
                assert_eq!(u32::from_le_bytes(dds[128..132].try_into().unwrap()), 71);
                assert_eq!(&dds[148..], expected);
            }
        }
    }
}
#[test]
fn cube_dds_is_one_cube_six_faces() {
    let (bytes, expected) = texture(1, true, true, false);
    let a = parse(&bytes).unwrap();
    let dds = a
        .read_entry(
            &fo4_core::SliceSource {
                name: "s",
                bytes: &bytes,
            },
            0,
            &Limits::default(),
            &Budget::new(2 << 20),
        )
        .unwrap();
    assert_eq!(u32::from_le_bytes(dds[140..144].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(dds[136..140].try_into().unwrap()), 4);
    assert_eq!(&dds[148..], expected);
    assert_eq!(
        u32::from_le_bytes(dds[112..116].try_into().unwrap()),
        0xfe00
    );
    for face in 0..6 {
        assert_eq!(
            &dds[148 + 48 * face..148 + 48 * face + 32],
            &vec![10 * face as u8; 32]
        );
    }
}
#[test]
fn every_truncated_header_table_name_or_payload() {
    for bytes in [
        general(1, &[("one.bin", b"payload", true)]),
        texture(7, false, true, true).0,
    ] {
        for n in 0..bytes.len() {
            let result = parse(&bytes[..n]);
            assert!(result.is_err(), "accepted prefix {n}/{}", bytes.len());
            let err = result.unwrap_err();
            assert!(!err.code.is_empty());
            assert!(!err.expected.is_empty());
            assert!(!err.found.is_empty());
        }
    }
}
#[test]
fn mutate_every_header_and_table_byte_under_tiny_budget() {
    for (bytes, table_end) in [
        (general(8, &[("one.bin", b"payload", true)]), 60),
        (texture(8, false, false, true).0, 120),
    ] {
        for pos in 0..table_end {
            for value in [0, 0x7f, 0xff] {
                let mut bad = bytes.clone();
                bad[pos] = value;
                let b = Budget::new(32768);
                let _ = Archive::read(
                    &fo4_core::SliceSource {
                        name: "mutation",
                        bytes: &bad,
                    },
                    &Limits {
                        max_payload: 1024,
                        max_entries: 16,
                        max_chunks: 8,
                        ..Limits::default()
                    },
                    &b,
                );
                assert!(b.used() <= 32768, "{pos}");
            }
        }
    }
}
#[test]
fn unsupported_versions_and_closed_layout() {
    let bytes = general(1, &[("one.bin", b"x", false)]);
    for version in [0, 2, 3, 4, u32::MAX] {
        let mut bad = bytes.clone();
        bad[4..8].copy_from_slice(&version.to_le_bytes());
        assert_eq!(parse(&bad).unwrap_err().code, "UnsupportedBa2Version");
    }
    let mut bad = texture(1, false, false, false).0;
    bad[38..40].copy_from_slice(&25u16.to_le_bytes());
    assert_eq!(parse(&bad).unwrap_err().code, "UnsupportedChunkHeaderSize");
}
#[test]
fn counts_ranges_names_and_exact_sizes() {
    let bytes = general(1, &[("one.bin", b"abc", false)]);
    for (pos, data, code) in [
        (12, u32::MAX.to_le_bytes().to_vec(), "LimitExceeded"),
        (40, u64::MAX.to_le_bytes().to_vec(), "Overflow"),
        (56, 0u32.to_le_bytes().to_vec(), "InvalidBa2Sentinel"),
    ] {
        let mut bad = bytes.clone();
        bad[pos..pos + data.len()].copy_from_slice(&data);
        assert_eq!(parse(&bad).unwrap_err().code, code);
    }
    assert_eq!(
        parse(&general(1, &[("../x", b"a", false)]))
            .unwrap()
            .entries[0]
            .diagnostics[0]
            .code,
        "UnsupportedEntryName"
    );
    let duplicate = parse(&general(1, &[("A", b"a", false), ("a", b"b", false)])).unwrap();
    assert!(duplicate.entries.iter().all(|e| e
        .diagnostics
        .iter()
        .any(|d| d.code == "UnsupportedAmbiguousName")));
    let mut bomb = general(1, &[("x", &[b'a'; 4096], true)]);
    bomb[52..56].copy_from_slice(&2u32.to_le_bytes());
    let a = parse(&bomb).unwrap();
    assert_eq!(
        a.read_entry(
            &fo4_core::SliceSource {
                name: "bomb",
                bytes: &bomb
            },
            0,
            &Limits::default(),
            &Budget::new(1 << 20)
        )
        .unwrap_err()
        .code,
        "DecompressedSizeMismatch"
    );
}
struct Tracking<'a> {
    bytes: &'a [u8],
    reads: RefCell<Vec<(u64, usize)>>,
}
impl ReadAt for Tracking<'_> {
    fn name(&self) -> &str {
        "tracking"
    }
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> Result<()> {
        self.reads.borrow_mut().push((o, out.len()));
        fo4_core::SliceSource {
            name: "t",
            bytes: self.bytes,
        }
        .read_exact_at(o, out)
    }
}
#[test]
fn indexing_never_reads_payload_and_chunk_read_is_selective() {
    let bytes = general(1, &[("one", &[0; 1024], false), ("two", &[1; 1024], false)]);
    let s = Tracking {
        bytes: &bytes,
        reads: RefCell::new(Vec::new()),
    };
    let a = Archive::read(&s, &Limits::default(), &Budget::new(1 << 20)).unwrap();
    assert!(s.reads.borrow().iter().all(|(o, _)| *o < 96 || *o >= 2144));
    s.reads.borrow_mut().clear();
    a.decode_chunk(&s, 1, 0, &Limits::default(), &Budget::new(1 << 20))
        .unwrap();
    assert_eq!(s.reads.borrow().as_slice(), &[(1120, 1024)]);
}

#[test]
fn review_h2_bad_name_does_not_discard_archive() {
    let bytes = general(1, &[("good", b"yes", false), ("../bad", b"raw", false)]);
    let archive = parse(&bytes).unwrap();
    assert_eq!(archive.entries.len(), 2);
    let source = fo4_core::SliceSource {
        name: "mixed",
        bytes: &bytes,
    };
    assert_eq!(
        archive
            .read_entry(&source, 0, &Limits::default(), &Budget::new(1 << 20))
            .unwrap(),
        b"yes"
    );
}
#[test]
fn review_h2_unknown_texture_format_keeps_structural_index() {
    let (mut bytes, _) = texture(1, false, false, false);
    bytes[45] = 0xff;
    assert!(parse(&bytes).is_ok());
}

#[test]
fn review_l4_index_checks_metadata_at_operation_boundaries() {
    let dir = fo4_core::TestDir::new("file-io");
    let bytes = general(1, &[("first", b"a", false), ("second", b"b", false)]);
    let path = dir.join("index.ba2");
    std::fs::write(&path, bytes).unwrap();
    let source = fo4_core::FileSource::open(&path).unwrap();
    let a = Archive::read(&source, &Limits::default(), &Budget::new(1 << 20)).unwrap();
    assert_eq!(source.metadata_checks(), 2);
    assert_eq!(a.entries.len(), 2);
    std::fs::write(path, b"changed").unwrap();
    assert_eq!(
        a.decode_chunk(&source, 0, 0, &Limits::default(), &Budget::new(1 << 20))
            .unwrap_err()
            .code,
        "FileChanged"
    );
}

#[test]
fn review_h2_layout_and_sharing_policies_are_entry_local() {
    for (offset, value, code) in [
        (45, 255, "UnsupportedDxgiFormat"),
        (46, 3, "UnsupportedTextureLayout"),
        (64, 9, "UnsupportedMipRange"),
        (60, 0, "UnsupportedTextureSize"),
    ] {
        let (mut bytes, _) = texture(1, false, true, false);
        bytes[offset] = value;
        let archive = parse(&bytes).unwrap();
        assert!(
            archive.entries[0]
                .diagnostics
                .iter()
                .any(|d| d.code == code),
            "{code}: {:?}",
            archive.entries[0].diagnostics
        );
    }
    let mut bytes = general(
        1,
        &[
            ("one", b"a", false),
            ("two", b"b", false),
            ("good", b"c", false),
        ],
    );
    let shared = bytes[40..48].to_vec();
    bytes[76..84].copy_from_slice(&shared);
    let archive = parse(&bytes).unwrap();
    assert!(archive.entries[..2].iter().all(|e| e
        .diagnostics
        .iter()
        .any(|d| d.code == "UnsupportedSharedPayload")));
    assert!(archive.entries[2].diagnostics.is_empty());
    let mut invalid_utf8 = general(1, &[("a", b"a", false)]);
    *invalid_utf8.last_mut().unwrap() = 0xff;
    let archive = parse(&invalid_utf8).unwrap();
    assert_eq!(archive.entries[0].original_name, [0xff]);
    assert!(archive.entries[0].diagnostics.is_empty());
    assert_eq!(archive.entries[0].key, [0xff]);
    assert_eq!(archive.entries[0].name, "ÿ");
}

#[test]
fn bounded_prefix_raw_zlib_and_partial_validation() {
    use std::cell::Cell;
    struct Meter<'a> {
        bytes: &'a [u8],
        max: Cell<usize>,
        read: Cell<usize>,
    }
    impl ReadAt for Meter<'_> {
        fn name(&self) -> &str {
            "prefix.ba2"
        }
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }
        fn read_exact_at(&self, o: u64, out: &mut [u8]) -> Result<()> {
            self.max.set(self.max.get().max(out.len()));
            self.read.set(self.read.get() + out.len());
            fo4_core::SliceSource {
                name: self.name(),
                bytes: self.bytes,
            }
            .read_exact_at(o, out)
        }
    }
    let data: Vec<u8> = (0..200_000u32).flat_map(u32::to_le_bytes).collect();
    for packed in [false, true] {
        let bytes = general(8, &[("meshes/test.hkx", &data, packed)]);
        let source = Meter {
            bytes: &bytes,
            max: Cell::new(0),
            read: Cell::new(0),
        };
        let l = Limits::default();
        let b = Budget::new(1 << 20);
        let a = GameArchive::read(fo4_core::Game::Fallout4, &source, &l, &b).unwrap();
        source.max.set(0);
        source.read.set(0);
        let p = a.read_prefix(&source, 0, 16, &l, &b).unwrap();
        assert_eq!(p.bytes, &data[..16]);
        assert!(!p.full_stream_validated);
        assert_eq!(p.declared_size, data.len() as u64);
        assert!(source.max.get() <= 4096);
        assert!(source.read.get() < data.len() / 2);
        assert_eq!(
            a.read_prefix(&source, 0, 4097, &l, &b).unwrap_err().code,
            "LimitExceeded"
        );
    }
    let bytes = general(1, &[("tiny.hkx", b"abc", true)]);
    let source = fo4_core::SliceSource {
        name: "tiny.ba2",
        bytes: &bytes,
    };
    let b = Budget::new(1 << 20);
    let l = Limits::default();
    let a = GameArchive::read(fo4_core::Game::Fallout4, &source, &l, &b).unwrap();
    let p = a.read_prefix(&source, 0, 16, &l, &b).unwrap();
    assert_eq!(p.bytes, b"abc");
    assert!(p.full_stream_validated);
    assert_eq!(
        a.read_prefix(&source, 0, 16, &l, &Budget::new(2))
            .unwrap_err()
            .code,
        "LimitExceeded"
    );
}
