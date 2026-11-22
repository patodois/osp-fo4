use super::*;
use fo4_core::SliceSource;

fn text(v: &mut Vec<u8>, s: &[u8]) {
    v.extend((s.len() as u16).to_le_bytes());
    v.extend(s);
}
fn u32s(v: &mut Vec<u8>, w: &[u32]) {
    for x in w {
        v.extend(x.to_le_bytes());
    }
}

/// A tiny synthetic save: 2x1 screenshot, two plugins and one light plugin,
/// one entry per global table, two change forms (one stored, one zlib),
/// a form id array of two words, an empty second array and the closing block.
fn synthetic(compressed: &[u8], plain_len: u32) -> Vec<u8> {
    let mut h = Vec::new();
    u32s(&mut h, &[HEADER_VERSION, 7]);
    text(&mut h, b"A");
    u32s(&mut h, &[3]);
    text(&mut h, b"Somewhere");
    text(&mut h, b"0d.0h.1m");
    text(&mut h, b"RaceId");
    h.extend(1u16.to_le_bytes());
    h.extend(1.5f32.to_le_bytes());
    h.extend(2.5f32.to_le_bytes());
    h.extend(42u64.to_le_bytes());
    u32s(&mut h, &[2, 1]);

    let mut v = MAGIC.to_vec();
    u32s(&mut v, &[h.len() as u32]);
    v.extend(&h);
    v.extend([1, 2, 3, 255, 4, 5, 6, 255]);
    v.push(68);
    text(&mut v, b"1.0");
    let mut p = vec![2];
    text(&mut p, b"First.esm");
    text(&mut p, b"Second.esm");
    p.extend(1u16.to_le_bytes());
    text(&mut p, b"Light.esl");
    u32s(&mut v, &[p.len() as u32]);
    v.extend(&p);

    let table = v.len();
    v.extend([0u8; LOCATION_TABLE_BYTES as usize]);
    let g1 = v.len() as u32;
    u32s(&mut v, &[0, 2]);
    v.extend([9, 9]);
    let g2 = v.len() as u32;
    u32s(&mut v, &[100, 0]);
    let cf = v.len() as u32;
    // Stored payload, u8 lengths, a reference into the array (index 2).
    v.extend([0x00, 0x00, 0x02]);
    u32s(&mut v, &[0x8000_0002]);
    v.extend([0x00, 68, 3, 0]);
    v.extend([7, 8, 9]);
    // Compressed payload, u16 lengths, a direct reference.
    v.extend([0x40, 0x12, 0x34]);
    u32s(&mut v, &[4]);
    v.extend([0x41, 69]);
    v.extend((compressed.len() as u16).to_le_bytes());
    v.extend((plain_len as u16).to_le_bytes());
    v.extend(compressed);
    let g3 = v.len() as u32;
    u32s(&mut v, &[1000, 1]);
    v.push(5);
    let fid = v.len() as u32;
    u32s(&mut v, &[2, 0x0100_0800, 0x0000_0014]);
    u32s(&mut v, &[0]);
    let closing = v.len() as u32;
    u32s(&mut v, &[4, 0]);
    let mut t = Vec::new();
    u32s(&mut t, &[fid, closing, g1, g2, cf, g3, 1, 1, 1, 2]);
    v[table..table + t.len()].copy_from_slice(&t);
    v
}

// zlib stream of the bytes 1, 2, 3, 4 (stored block), with its checksum.
const ZLIB_1234: [u8; 15] = [
    0x78, 0x01, 0x01, 0x04, 0x00, 0xfb, 0xff, 1, 2, 3, 4, 0x00, 0x18, 0x00, 0x0b,
];

fn read(bytes: &[u8]) -> Result<Save> {
    let s = SliceSource {
        name: "synthetic.fos",
        bytes,
    };
    Save::read(&s, &Limits::default(), &Budget::new(1 << 24))
}

#[test]
fn reads_a_synthetic_save_to_the_last_byte() {
    let bytes = synthetic(&ZLIB_1234, 4);
    let s = read(&bytes).unwrap();
    assert_eq!(s.parts.total(), bytes.len() as u64);
    assert_eq!(s.header.character, b"A");
    assert_eq!(s.header.shot_width * s.header.shot_height, 2);
    assert_eq!(s.screenshot.length, 8);
    assert_eq!(s.form_version, 68);
    assert_eq!(
        s.plugins,
        vec![b"First.esm".to_vec(), b"Second.esm".to_vec()]
    );
    assert_eq!(s.light_plugins, vec![b"Light.esl".to_vec()]);
    assert_eq!(s.globals.each_ref().map(|g| g.len()), [1, 1, 1]);
    assert_eq!(s.globals[1][0].kind, 100);
    assert_eq!(s.change_forms.len(), 2);
    assert_eq!(s.form_ids, vec![0x0100_0800, 0x14]);
    assert!(s.second_array.is_empty());
    assert_eq!(s.closing_bytes, [4, 0, 0, 0, 0, 0, 0, 0]);

    let a = s.change_forms[0];
    assert_eq!((a.kind(), a.width_code(), a.version), (0, 0, 68));
    assert_eq!(a.form.kind(), RefKind::Indexed);
    assert_eq!(s.resolve(a.form), Some(0x14));
    let b = s.change_forms[1];
    assert_eq!((b.kind(), b.width_code(), b.version), (1, 1, 69));
    assert_eq!(s.resolve(b.form), Some(0x0000_1234));
    let src = SliceSource {
        name: "synthetic.fos",
        bytes: &bytes,
    };
    let budget = Budget::new(1 << 20);
    let l = Limits::default();
    assert_eq!(a.payload(&src, &l, &budget).unwrap(), [7, 8, 9]);
    assert_eq!(b.payload(&src, &l, &budget).unwrap(), [1, 2, 3, 4]);
}

#[test]
fn reference_kinds_and_indices() {
    let s = read(&synthetic(&ZLIB_1234, 4)).unwrap();
    assert_eq!(s.resolve(RefId([0, 0, 0])), None);
    assert_eq!(s.resolve(RefId([0, 0, 1])), Some(0x0100_0800));
    assert_eq!(s.resolve(RefId([0, 0, 3])), None);
    assert_eq!(s.resolve(RefId([0x80, 0, 5])), Some(0xff00_0005));
    assert_eq!(s.resolve(RefId([0xc0, 0, 5])), None);
    assert_eq!(RefId([0x7f, 0xff, 0xff]).value(), 0x3f_ffff);
}

#[test]
fn a_wrong_declared_offset_is_a_named_error() {
    let mut bytes = synthetic(&ZLIB_1234, 4);
    let s = read(&bytes).unwrap();
    let table = (s.plugin_block.end()) as usize;
    bytes[table + 16] ^= 1; // change-form offset
    let e = read(&bytes).unwrap_err();
    assert_eq!(e.code, "SaveOffsetMismatch");
}

#[test]
fn a_wrong_decompressed_length_is_refused_on_request() {
    let bytes = synthetic(&ZLIB_1234, 5);
    let s = read(&bytes).unwrap();
    let src = SliceSource {
        name: "synthetic.fos",
        bytes: &bytes,
    };
    let e = s.change_forms[1]
        .payload(&src, &Limits::default(), &Budget::new(1 << 20))
        .unwrap_err();
    assert_eq!(e.code, "DecompressedSizeMismatch");
}

#[test]
fn truncation_magic_and_width_three_are_refused() {
    let bytes = synthetic(&ZLIB_1234, 4);
    for cut in [0, 5, 20, 60, bytes.len() - 9] {
        assert!(read(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut bad = bytes.clone();
    bad[0] = b'X';
    assert_eq!(read(&bad).unwrap_err().code, "SaveMagic");
    let s = read(&bytes).unwrap();
    let mut wide = bytes.clone();
    wide[s.change_forms[0].offset as usize + 7] |= 0xc0;
    assert_eq!(read(&wide).unwrap_err().code, "UnmeasuredChangeFormWidth");
    let mut header = bytes;
    header[12] += 1;
    assert!(read(&header).is_err());
}
