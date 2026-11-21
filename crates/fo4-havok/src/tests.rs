use super::*;

/// Synthetic container description; every byte is generated here.
pub(crate) struct Spec {
    pub names: Vec<(&'static str, u32)>,
    pub object_bytes: usize,
    /// Bytes written into the object area at the given offsets.
    pub content: Vec<(usize, Vec<u8>)>,
    pub pointers: Vec<[u32; 2]>,
    pub links: Vec<[u32; 3]>,
    pub objects: Vec<[u32; 3]>,
    /// Header words 24, 28, 32, 36.
    pub root: [u32; 4],
}

pub(crate) fn name_offset(names: &[(&str, u32)], index: usize) -> u32 {
    let mut at = 0;
    for (name, _) in &names[..index] {
        at += 5 + name.len() as u32 + 1;
    }
    at + 5
}

fn spec() -> Spec {
    let names = vec![
        ("hkClass", 0x1111_1111),
        ("RootThing", 0x2222_2222),
        ("LeafThing", 0x3333_3333),
    ];
    let root_name = name_offset(&names, 1);
    let leaf_name = name_offset(&names, 2);
    Spec {
        object_bytes: 0x60,
        content: Vec::new(),
        pointers: vec![[0x10, 0x20], [0x48, 0x50]],
        links: vec![[0x18, 2, 0x30]],
        objects: vec![[0, 0, root_name], [0x30, 0, leaf_name]],
        root: [2, 0, 0, root_name],
        names,
    }
}

fn pad16(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(16) {
        v.push(0xff);
    }
}

pub(crate) fn build(s: &Spec) -> Vec<u8> {
    let mut names = Vec::new();
    for (name, sig) in &s.names {
        names.extend(sig.to_le_bytes());
        names.push(CLASS_NAME_MARKER);
        names.extend(name.as_bytes());
        names.push(0);
    }
    pad16(&mut names);
    let mut data = vec![0u8; s.object_bytes];
    for (at, bytes) in &s.content {
        data[*at..*at + bytes.len()].copy_from_slice(bytes);
    }
    let mut offs = [0u32; 6];
    offs[0] = data.len() as u32;
    for p in &s.pointers {
        p.iter().for_each(|w| data.extend(w.to_le_bytes()));
    }
    pad16(&mut data);
    offs[1] = data.len() as u32;
    for p in &s.links {
        p.iter().for_each(|w| data.extend(w.to_le_bytes()));
    }
    pad16(&mut data);
    offs[2] = data.len() as u32;
    for p in &s.objects {
        p.iter().for_each(|w| data.extend(w.to_le_bytes()));
    }
    pad16(&mut data);
    offs[3] = data.len() as u32;
    offs[4] = offs[3];
    offs[5] = offs[3];
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(0u32.to_le_bytes());
    out.extend(11u32.to_le_bytes());
    out.extend(LAYOUT);
    out.extend(3u32.to_le_bytes());
    s.root.iter().for_each(|w| out.extend(w.to_le_bytes()));
    out.extend(VERSION.as_bytes());
    out.push(0);
    out.push(0xff);
    out.extend(0u32.to_le_bytes());
    out.extend(21u32.to_le_bytes());
    assert_eq!(out.len(), HEADER_LEN);
    let names_start = 256u32;
    let types_start = names_start + names.len() as u32;
    let n = names.len() as u32;
    for (name, start, words) in [
        (SECTION_NAMES[0], names_start, [n; 6]),
        (SECTION_NAMES[1], types_start, [0; 6]),
        (SECTION_NAMES[2], types_start, offs),
    ] {
        let mut record = name.as_bytes().to_vec();
        record.resize(19, 0);
        record.push(0xff);
        record.extend(start.to_le_bytes());
        words.iter().for_each(|w| record.extend(w.to_le_bytes()));
        record.extend([0xff; 16]);
        out.extend(record);
    }
    out.extend(names);
    out.extend(data);
    out
}

pub(crate) fn read(bytes: &[u8]) -> Result<Container<'_>> {
    Container::read(bytes, "synthetic.nif", 1000, &Budget::new(1 << 20))
}

pub(crate) fn code(bytes: &[u8]) -> (String, u64) {
    let e = read(bytes).expect_err("must fail");
    (e.code, e.source.offset)
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn reads_a_synthetic_container() {
    let s = spec();
    let bytes = build(&s);
    let c = read(&bytes).unwrap();
    assert_eq!(c.version, VERSION);
    assert_eq!(c.header.section_count(), 3);
    let names: Vec<_> = c
        .sections
        .iter()
        .map(|x| (x.name, x.start, x.len()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("__classnames__", 256, 48),
            ("__types__", 304, 0),
            ("__data__", 304, 0xa0)
        ]
    );
    assert_eq!(c.data().map(|d| d.end() as usize), Some(bytes.len()));
    let cn: Vec<_> = c
        .class_names
        .iter()
        .map(|n| (n.offset, n.signature, n.name))
        .collect();
    assert_eq!(
        cn,
        vec![
            (5, 0x1111_1111, "hkClass"),
            (18, 0x2222_2222, "RootThing"),
            (33, 0x3333_3333, "LeafThing")
        ]
    );
    assert_eq!(c.fill, [5, 0, 4, 8]);
    assert_eq!(
        c.pointers,
        vec![
            Pointer {
                source: 0x10,
                destination: 0x20
            },
            Pointer {
                source: 0x48,
                destination: 0x50
            }
        ]
    );
    assert_eq!(
        c.object_pointers,
        vec![ObjectPointer {
            source: 0x18,
            section: 2,
            destination: 0x30
        }]
    );
    let objects: Vec<_> = c.objects.iter().map(|o| (o.offset, o.class)).collect();
    assert_eq!(objects, vec![(0, "RootThing"), (0x30, "LeafThing")]);
    assert_eq!(c.root().map(|r| r.class), Some("RootThing"));
    assert_eq!(c.object_bytes().len(), 0x60);
}

#[test]
fn every_truncation_is_a_named_error() {
    let bytes = build(&spec());
    for n in 0..bytes.len() {
        let e = read(&bytes[..n]).expect_err("truncated payload must fail");
        assert!(!e.code.is_empty());
        assert!(e.source.offset >= 1000, "{e}");
    }
}

#[test]
fn single_byte_changes_never_panic() {
    let bytes = build(&spec());
    for at in 0..bytes.len() {
        for value in [0u8, 1, 9, 0x7f, 0xff] {
            let mut m = bytes.clone();
            m[at] = value;
            let _ = read(&m);
        }
    }
}

#[test]
fn header_errors_are_named() {
    let good = build(&spec());
    let mut b = good.clone();
    b[0] = 0;
    assert_eq!(code(&b), ("InvalidHavokMagic".into(), 1000));
    let mut b = good.clone();
    b[16] = 4;
    assert_eq!(code(&b), ("UnsupportedHavokLayout".into(), 1016));
    let mut b = good.clone();
    b[44] = b'9';
    assert_eq!(code(&b), ("UnsupportedHavokVersion".into(), 1040));
    let mut b = good.clone();
    b[40..56].fill(b'x');
    assert_eq!(code(&b), ("UnsupportedHavokVersion".into(), 1040));
    let mut b = good.clone();
    put(&mut b, 20, 4);
    assert_eq!(code(&b), ("UnsupportedHavokSectionCount".into(), 1020));
    let mut b = good.clone();
    b[128 + 2] = b'X';
    assert_eq!(code(&b), ("InvalidHavokSectionRecord".into(), 1128));
    let mut b = good;
    put(&mut b, 28, 0x30);
    assert_eq!(code(&b), ("InvalidHavokRoot".into(), 1024));
}

#[test]
fn section_bound_errors_are_named() {
    let good = build(&spec());
    let mut b = good.clone();
    put(&mut b, 84, 272);
    assert_eq!(code(&b), ("InvalidHavokSectionBounds".into(), 1084));
    let mut b = good.clone();
    put(&mut b, 216, 0x90);
    assert_eq!(code(&b), ("InvalidHavokSectionBounds".into(), 1220));
    let mut b = good.clone();
    b.push(0xff);
    assert_eq!(code(&b), ("InvalidHavokSectionBounds".into(), 1000 + 236));
    let mut b = good.clone();
    put(&mut b, 236, 0x1000);
    put(&mut b, 232, 0x1000);
    put(&mut b, 228, 0x1000);
    assert_eq!(code(&b), ("InvalidHavokSectionBounds".into(), 1236));
    let mut b = good.clone();
    put(&mut b, 88, 32);
    assert_eq!(code(&b), ("UnsupportedHavokSectionLayout".into(), 1088));
    // A non-empty __types__, with __data__ moved after it.
    let mut b = good.clone();
    put(&mut b, 172, 16);
    put(&mut b, 212, 320);
    for (k, w) in [0x50, 0x60, 0x70, 0x90, 0x90, 0x90].into_iter().enumerate() {
        put(&mut b, 216 + 4 * k, w);
    }
    assert_eq!(code(&b), ("UnsupportedHavokSectionLayout".into(), 1152));
    let mut b = good;
    put(&mut b, 228, 0x90);
    assert_eq!(code(&b), ("UnsupportedHavokSectionLayout".into(), 1228));
}

#[test]
fn class_name_errors_are_named() {
    let good = build(&spec());
    let mut b = good.clone();
    b[256 + 4] = 8;
    assert_eq!(code(&b), ("InvalidHavokClassName".into(), 1260));
    let mut b = good.clone();
    b[256 + 6] = 0x80;
    assert_eq!(code(&b), ("InvalidHavokClassName".into(), 1261));
    // Fill that is not all 0xff after the last name.
    let mut b = good;
    b[256 + 47] = 0;
    assert_eq!(code(&b).0, "InvalidHavokClassName");
}

#[test]
fn table_errors_are_named() {
    let s = spec();
    let good = build(&s);
    let data = 304usize;
    // Fill shorter than an entry that is not all 0xff.
    let mut b = good.clone();
    b[data + 0x98 + 3] = 0;
    assert_eq!(
        code(&b),
        ("InvalidHavokFixupTable".into(), 1000 + data as u64 + 0x98)
    );
    let mut b = good.clone();
    b[data + 0x7c + 3] = 0;
    assert_eq!(
        code(&b),
        ("InvalidHavokFixupTable".into(), 1000 + data as u64 + 0x7c)
    );
    // A 0xffffffff word followed by bytes that are not fill.
    let mut t = spec();
    t.objects[1][0] = u32::MAX;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixupTable".into(), 1000 + data as u64 + 0x8c)
    );

    let mut t = spec();
    t.pointers[0][1] = 0x60;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x64)
    );
    let mut t = spec();
    t.pointers[1][0] = 0x5c;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x68)
    );
    let mut t = spec();
    t.links[0][1] = 0;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x74)
    );
    let mut t = spec();
    t.links[0][2] = 0x20;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x78)
    );
    let mut t = spec();
    t.objects[1][1] = 2;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x90)
    );
    let mut t = spec();
    t.objects[1][2] += 1;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x94)
    );
    let mut t = spec();
    t.objects.swap(0, 1);
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokFixup".into(), 1000 + data as u64 + 0x8c)
    );
}

#[test]
fn pointers_stay_in_their_object() {
    let s = spec();
    let data = 304u64;
    let bytes = build(&s);
    let c = read(&bytes).unwrap();
    let spans: Vec<_> = c
        .object_spans()
        .map(|x| (x.index, x.offset, x.end, x.class))
        .collect();
    assert_eq!(
        spans,
        vec![(0, 0, 0x30, "RootThing"), (1, 0x30, 0x60, "LeafThing")]
    );
    assert_eq!(c.object_at(0x2f), Some(0));
    assert_eq!(c.object_at(0x30), Some(1));
    assert_eq!(c.object_at(0x60), None);
    assert_eq!(c.object_span(2), None);
    assert_eq!(c.object_span(1).map(|x| c.span_bytes(&x).len()), Some(0x30));
    // Destination in the next object's span.
    let mut t = spec();
    t.pointers[0][1] = 0x40;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokObjectSpan".into(), 1000 + data + 0x64)
    );
    // Destination before the object holding the source.
    let mut t = spec();
    t.pointers[1][1] = 0x20;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokObjectSpan".into(), 1000 + data + 0x6c)
    );
    // Sources before the first object, which is not at offset 0 here.
    let mut t = spec();
    t.objects = vec![[0x10, 0, t.root[3]], [0x30, 0, t.objects[1][2]]];
    t.root[1] = 0x10;
    t.pointers[0] = [0x08, 0x20];
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokObjectSpan".into(), 1000 + data + 0x60)
    );
    let mut t = spec();
    t.objects = vec![[0x10, 0, t.root[3]], [0x30, 0, t.objects[1][2]]];
    t.root[1] = 0x10;
    t.links[0][0] = 0x08;
    assert_eq!(
        code(&build(&t)),
        ("InvalidHavokObjectSpan".into(), 1000 + data + 0x70)
    );
}

#[test]
fn budget_bounds_the_tables() {
    let bytes = build(&spec());
    let e = Container::read(&bytes, "synthetic.nif", 0, &Budget::new(16)).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
}
