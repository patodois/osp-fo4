use super::*;
use fo4_core::SliceSource;
use std::{cell::Cell, io::Write};
fn frame(data: &[u8], linked: bool, checks: bool, size: bool) -> Vec<u8> {
    use lz4_flex::frame::{BlockMode, BlockSize, FrameEncoder, FrameInfo};
    let info = FrameInfo::new()
        .block_mode(if linked {
            BlockMode::Linked
        } else {
            BlockMode::Independent
        })
        .block_size(BlockSize::Max64KB)
        .block_checksums(checks)
        .content_checksum(checks)
        .content_size(size.then_some(data.len() as u64));
    let mut e = FrameEncoder::with_frame_info(info, Vec::new());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
fn bsa(folder: &[u8], files: &[(&[u8], &[u8], bool)], default: bool, embedded: bool) -> Vec<u8> {
    let names: Vec<u8> = files
        .iter()
        .flat_map(|(n, _, _)| n.iter().copied().chain([0]))
        .collect();
    let data_start = 36 + 24 + 1 + folder.len() + 1 + 16 * files.len() + names.len();
    let mut data = Vec::new();
    let mut records = Vec::new();
    for (i, (name, bytes, compressed)) in files.iter().enumerate() {
        let mut payload = Vec::new();
        if embedded {
            payload.push(name.len() as u8);
            payload.extend(*name);
        }
        if *compressed {
            payload.extend((bytes.len() as u32).to_le_bytes());
            payload.extend(frame(bytes, false, true, true));
        } else {
            payload.extend(*bytes);
        }
        records.extend((i as u64 + 11).to_le_bytes());
        records.extend(
            ((payload.len() as u32)
                | if *compressed ^ default {
                    0x4000_0000
                } else {
                    0
                })
            .to_le_bytes(),
        );
        records.extend(((data_start + data.len()) as u32).to_le_bytes());
        data.extend(payload);
    }
    let mut out = b"BSA\0".to_vec();
    for n in [
        105,
        36,
        3 | if default { 4 } else { 0 } | if embedded { 0x100 } else { 0 },
        1,
        files.len() as u32,
        (folder.len() + 1) as u32,
        names.len() as u32,
        0x12345678,
    ] {
        out.extend(n.to_le_bytes());
    }
    out.extend(123u64.to_le_bytes());
    out.extend((files.len() as u32).to_le_bytes());
    out.extend([0xa5; 12]);
    out.push((folder.len() + 1) as u8);
    out.extend(folder);
    out.push(0);
    out.extend(records);
    out.extend(names);
    out.extend(data);
    out
}
fn parse(bytes: &[u8]) -> Result<BsaArchive> {
    BsaArchive::read(
        &SliceSource {
            name: "synthetic.bsa",
            bytes,
        },
        &Limits::default(),
        &Budget::new(8 << 20),
    )
}
#[test]
fn bsa_raw_frame_toggle_embedded_and_byte_names() {
    for default in [false, true] {
        for embedded in [false, true] {
            let bytes = bsa(
                b"Textures",
                &[
                    (b"A.dds", b"DDS raw header stays", false),
                    (
                        b"\x81\xc0.dds",
                        b"compressed bytes repeated repeated repeated",
                        true,
                    ),
                ],
                default,
                embedded,
            );
            let a = parse(&bytes).unwrap();
            let s = SliceSource {
                name: "synthetic.bsa",
                bytes: &bytes,
            };
            assert_eq!(a.entries[0].key, b"textures/a.dds");
            assert_eq!(a.entries[1].key, b"textures/\x81\xc0.dds");
            assert_eq!(a.folders[0].raw_locator, [0xa5; 12]);
            assert_eq!(a.file_flags, 0x12345678);
            assert!(a.uninterpreted_fields >= 5);
            assert_eq!(
                a.entries[1].name_encoding,
                "hex (undefined Windows-1252 byte)"
            );
            for (i, expected) in [
                b"DDS raw header stays".as_slice(),
                b"compressed bytes repeated repeated repeated",
            ]
            .iter()
            .enumerate()
            {
                assert_eq!(
                    &a.read_entry(&s, i, &Limits::default(), &Budget::new(1 << 20))
                        .unwrap(),
                    expected
                );
                assert_eq!(a.entries[i].embedded_name.is_some(), embedded);
            }
            assert!(GameArchive::read(
                fo4_core::Game::Fallout4,
                &s,
                &Limits::default(),
                &Budget::new(1 << 20)
            )
            .is_err());
            let a = GameArchive::read(
                fo4_core::Game::SkyrimSe,
                &s,
                &Limits::default(),
                &Budget::new(1 << 20),
            )
            .unwrap();
            assert!(a.entry(2).is_err());
            assert!(a
                .decode_chunk(&s, 0, 1, &Limits::default(), &Budget::new(1 << 20))
                .is_err());
        }
    }
}
#[test]
fn bsa_every_truncation_and_semantic_header_table_corruption() {
    let bytes = bsa(
        b"a",
        &[(b"b", b"raw", false), (b"c", b"compressed", true)],
        false,
        true,
    );
    for end in 0..bytes.len() {
        let b = Budget::new(16384);
        let s = SliceSource {
            name: "truncated.bsa",
            bytes: &bytes[..end],
        };
        assert!(
            BsaArchive::read(&s, &Limits::default(), &b).is_err(),
            "end {end}"
        );
        assert!(b.used() <= 16384);
    }
    // Every interpreted header/table scalar has a corruption case. Hash/locator/file flags stay raw.
    for (offset, value, code) in [
        (0, 0, "InvalidBsaMagic"),
        (4, 104, "UnsupportedBsaVersion"),
        (8, 35, "UnsupportedBsaHeaderOffset"),
        (12, 1, "UnsupportedBsaFlags"),
        (16, u32::MAX, "LimitExceeded"),
        (20, 0, "LimitExceeded"),
        (24, 0, "BsaFolderNameLengthMismatch"),
        (28, 0, "Truncated"),
        (44, 3, "LimitExceeded"),
        (71, 0x8000_0000, "UnsupportedBsaSizeFlags"),
        (75, 1, "BsaPayloadOverlapsIndex"),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let e = parse(&bad).unwrap_err();
        assert_eq!(e.code, code, "offset {offset}: {e}");
        assert!(!e.expected.is_empty() && !e.found.is_empty());
        assert_eq!(e.source.file, "synthetic.bsa");
    }
    let mut bad = bytes.clone();
    bad[62] = b'x';
    assert_eq!(parse(&bad).unwrap_err().code, "InvalidBsaFolderName");
    let mut bad = bytes.clone();
    bad[95..99].copy_from_slice(b"xxxx");
    assert!(parse(&bad).is_err());
    for max in [0, 1, 64, 256, 1024] {
        let b = Budget::new(max);
        let _ = BsaArchive::read(
            &SliceSource {
                name: "small.bsa",
                bytes: &bytes,
            },
            &Limits::default(),
            &b,
        );
        assert!(b.used() <= max);
    }
}
#[test]
fn bsa_limits_unsafe_names_missing_names_and_embedded_bounds() {
    let bytes = bsa(b"a", &[(b"b", b"x", true)], false, true);
    for l in [
        Limits {
            max_entries: 0,
            ..Limits::default()
        },
        Limits {
            max_payload: 4,
            ..Limits::default()
        },
        Limits {
            max_string: 1,
            ..Limits::default()
        },
    ] {
        assert!(BsaArchive::read(
            &SliceSource {
                name: "test",
                bytes: &bytes
            },
            &l,
            &Budget::new(1 << 20)
        )
        .is_err());
    }
    let mut bad = bytes.clone();
    let start = u32::from_le_bytes(bad[75..79].try_into().unwrap()) as usize;
    bad[start] = 255;
    assert!(parse(&bad).is_err());
    for name in [b"../bad".as_slice(), b"C:bad", b"/bad"] {
        let bytes = bsa(b"a", &[(name, b"x", false)], false, false);
        let a = parse(&bytes).unwrap();
        assert!(!a.entries[0].diagnostics.is_empty());
        assert!(a
            .read_entry(
                &SliceSource {
                    name: "test",
                    bytes: &bytes
                },
                0,
                &Limits::default(),
                &Budget::new(1 << 20)
            )
            .is_err());
    }
    for flags in [0u32, 1, 2, 3 | 0x40, 3 | 0x200] {
        let mut bad = bytes.clone();
        bad[12..16].copy_from_slice(&flags.to_le_bytes());
        assert_eq!(parse(&bad).unwrap_err().code, "UnsupportedBsaFlags");
    }
}
#[test]
fn lz4_independent_linked_sizes_checksums_and_exact_consumption() {
    let mut data = Vec::new();
    let mut state = 123u32;
    for _ in 0..65536 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        data.push((state >> 16) as u8);
    }
    let first = data.clone();
    data.extend(&first);
    data.extend(&first);
    for linked in [false, true] {
        for checks in [false, true] {
            for size in [false, true] {
                let raw = frame(&data, linked, checks, size);
                let out = lz4_frame::decode(
                    &raw,
                    data.len() as u64,
                    &Limits::default(),
                    &Budget::new(2 << 20),
                    "frame",
                    500,
                )
                .unwrap();
                assert_eq!(out, data);
            }
        }
    }
    let raw = frame(b"small small small small", false, true, true);
    for end in 0..raw.len() {
        assert!(lz4_frame::decode(
            &raw[..end],
            23,
            &Limits::default(),
            &Budget::new(1 << 20),
            "frame",
            99
        )
        .is_err());
    }
    for offset in [0, 4, 5, 6, 14, 15, raw.len() - 1] {
        let mut bad = raw.clone();
        bad[offset] ^= 0x80;
        let err = lz4_frame::decode(
            &bad,
            23,
            &Limits::default(),
            &Budget::new(1 << 20),
            "frame",
            99,
        )
        .unwrap_err();
        assert!(err.source.offset >= 99);
    }
    for declared in [0, 22, 24, 1 << 30] {
        let b = Budget::new(4096);
        assert!(lz4_frame::decode(&raw, declared, &Limits::default(), &b, "frame", 0).is_err());
        assert!(b.used() <= 4096);
    }
    let raw = frame(b"test", false, false, false);
    let mut tail = raw.clone();
    tail.push(0);
    assert_eq!(
        lz4_frame::decode(&tail, 4, &Limits::default(), &Budget::new(4096), "frame", 0)
            .unwrap_err()
            .code,
        "TrailingLz4Data"
    );
    for declared in [3, 5] {
        assert!(lz4_frame::decode(
            &raw,
            declared,
            &Limits::default(),
            &Budget::new(4096),
            "frame",
            0
        )
        .is_err());
    }
    let mut dict = raw.clone();
    dict[4] |= 1;
    assert_eq!(
        lz4_frame::decode(&dict, 4, &Limits::default(), &Budget::new(4096), "frame", 0)
            .unwrap_err()
            .code,
        "UnsupportedLz4Dictionary"
    );
}
#[test]
fn bsa_index_and_single_entry_use_only_ranges() {
    struct Counting<'a> {
        s: SliceSource<'a>,
        bytes: Cell<usize>,
    }
    impl ReadAt for Counting<'_> {
        fn name(&self) -> &str {
            self.s.name
        }
        fn len(&self) -> u64 {
            self.s.len()
        }
        fn read_exact_at(&self, o: u64, out: &mut [u8]) -> Result<()> {
            self.bytes.set(self.bytes.get() + out.len());
            self.s.read_exact_at(o, out)
        }
    }
    let large = vec![7; 1 << 20];
    let bytes = bsa(
        b"a",
        &[(b"large", &large, false), (b"small", b"ok", false)],
        false,
        false,
    );
    let source = Counting {
        s: SliceSource {
            name: "range.bsa",
            bytes: &bytes,
        },
        bytes: Cell::new(0),
    };
    let a = BsaArchive::read(&source, &Limits::default(), &Budget::new(1 << 20)).unwrap();
    assert!(source.bytes.get() < 512);
    assert_eq!(
        a.read_entry(&source, 1, &Limits::default(), &Budget::new(1024))
            .unwrap(),
        b"ok"
    );
    assert!(source.bytes.get() < 512);
}

#[test]
fn bsa_mutations_stay_bounded_and_duplicates_stay_blocked() {
    let bytes = bsa(
        b"folder",
        &[(b"a.bin", b"abcabcabcabcabcabc", true)],
        true,
        true,
    );
    for byte in 0..bytes.len() {
        for mask in [1, 0x80, 0xff] {
            let mut bad = bytes.clone();
            bad[byte] ^= mask;
            let b = Budget::new(16384);
            let s = SliceSource {
                name: "mutation.bsa",
                bytes: &bad,
            };
            if let Ok(a) = BsaArchive::read(&s, &Limits::default(), &b) {
                for index in 0..a.entries.len() {
                    let _ = a.read_entry(&s, index, &Limits::default(), &b);
                }
            }
            assert!(b.used() <= 16384);
        }
    }
    let duplicate = bsa(
        b"a",
        &[(b"B", b"first", false), (b"b", b"second", false)],
        false,
        false,
    );
    let a = parse(&duplicate).unwrap();
    assert!(a.entries.iter().all(|e| e
        .diagnostics
        .iter()
        .any(|d| d.code == "UnsupportedAmbiguousName")));
}
#[test]
fn lz4_empty_frame_and_exact_checksum_diagnostics() {
    fn header(flg: u8) -> Vec<u8> {
        let mut out = 0x184d2204u32.to_le_bytes().to_vec();
        out.extend([flg, 0x40]);
        out.push((twox_hash::xxhash32::Hasher::oneshot(0, &out[4..]) >> 8) as u8);
        out
    }
    let mut empty = header(0x60);
    empty.extend(0u32.to_le_bytes());
    assert_eq!(
        lz4_frame::decode(&empty, 0, &Limits::default(), &Budget::new(256), "empty", 0).unwrap(),
        b""
    );
    let mut raw = header(0x74);
    raw.extend(0x8000_0003u32.to_le_bytes());
    raw.extend(b"abc");
    let block = twox_hash::xxhash32::Hasher::oneshot(0, b"abc");
    raw.extend(block.to_le_bytes());
    raw.extend(0u32.to_le_bytes());
    raw.extend(block.to_le_bytes());
    assert_eq!(
        lz4_frame::decode(&raw, 3, &Limits::default(), &Budget::new(256), "raw", 0).unwrap(),
        b"abc"
    );
    for (o, code) in [
        (6, "InvalidLz4HeaderChecksum"),
        (14, "InvalidLz4BlockChecksum"),
        (22, "InvalidLz4ContentChecksum"),
    ] {
        let mut bad = raw.clone();
        bad[o] ^= 1;
        assert_eq!(
            lz4_frame::decode(&bad, 3, &Limits::default(), &Budget::new(256), "raw", 100)
                .unwrap_err()
                .code,
            code
        );
    }
    let mut wrong_block = header(0x60);
    wrong_block.extend(65537u32.to_le_bytes());
    assert_eq!(
        lz4_frame::decode(
            &wrong_block,
            3,
            &Limits::default(),
            &Budget::new(256),
            "raw",
            0
        )
        .unwrap_err()
        .code,
        "LimitExceeded"
    );
    let mut invalid = header(0x60);
    invalid.extend(1u32.to_le_bytes());
    invalid.push(0xff);
    invalid.extend(0u32.to_le_bytes());
    assert_eq!(
        lz4_frame::decode(&invalid, 3, &Limits::default(), &Budget::new(256), "raw", 0)
            .unwrap_err()
            .code,
        "InvalidLz4Block"
    );
}
#[test]
fn bsa_multiple_folders_and_opaque_fields() {
    let mut bytes = b"BSA\0".to_vec();
    for n in [105u32, 36, 3, 2, 2, 4, 4, 0] {
        bytes.extend(n.to_le_bytes());
    }
    for i in 0..2u64 {
        bytes.extend(i.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([255; 12]);
    }
    // Folder blocks at 84 and 103, flat names at 122, payload starts at 126.
    for (folder, offset) in [(b'a', 126u32), (b'z', 127u32)] {
        bytes.extend([2, folder, 0]);
        bytes.extend(0u64.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
    }
    bytes.extend(b"b\0c\0XY");
    let a = parse(&bytes).unwrap();
    assert_eq!(a.entries[0].original_name, b"a/b");
    assert_eq!(a.entries[1].original_name, b"z/c");
    let s = SliceSource {
        name: "multi",
        bytes: &bytes,
    };
    assert_eq!(
        a.read_entry(&s, 1, &Limits::default(), &Budget::new(512))
            .unwrap(),
        b"Y"
    );
}

#[test]
fn bsa_uninterpreted_name_table_tail_is_retained() {
    // Observed condition only: extra table bytes have no assumed padding semantics.
    // All bytes and payload below are generated, not copied from a game archive.
    let mut bytes = bsa(b"a", &[(b"b", b"payload", false)], false, false);
    let data_start = u32::from_le_bytes(bytes[75..79].try_into().unwrap()) as usize;
    let extra = [9, 8, 7, 6, 5, 4, 3, 2, 1];
    bytes.splice(data_start..data_start, extra);
    bytes[28..32].copy_from_slice(&11u32.to_le_bytes());
    bytes[75..79].copy_from_slice(&((data_start + 9) as u32).to_le_bytes());
    let a = parse(&bytes).unwrap();
    assert_eq!(a.raw_name_tail, extra);
    assert_eq!(
        a.read_entry(
            &SliceSource {
                name: "tail.bsa",
                bytes: &bytes
            },
            0,
            &Limits::default(),
            &Budget::new(1024)
        )
        .unwrap(),
        b"payload"
    );
    assert!(a.uninterpreted_fields >= 5);
}
