//! Synthetic `.hkx` files for tests of this crate and of the crates that
//! read behaviour graphs (feature `fixture`). Every byte is written here;
//! nothing comes from a game.
use crate::{CLASS_NAME_MARKER, HEADER_LEN, LAYOUT, MAGIC, SECTION_NAMES, VERSION};

/// A container described by its parts.
pub struct Spec {
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

/// Offset of class name `index` in `__classnames__` as [`build`] writes it.
pub fn name_offset(names: &[(&str, u32)], index: usize) -> u32 {
    let mut at = 0;
    for (name, _) in &names[..index] {
        at += 5 + name.len() as u32 + 1;
    }
    at + 5
}

fn pad16(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(16) {
        v.push(0xff);
    }
}

/// The bytes of the container `s` describes.
pub fn build(s: &Spec) -> Vec<u8> {
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

/// A file built object by object: each object gets a span of its own
/// size (rounded up to 16 bytes) after the previous one; fields are
/// offsets from the object's start. The first object is the root.
#[derive(Default)]
pub struct Objects {
    classes: Vec<&'static str>,
    objects: Vec<(usize, usize)>,
    bytes: Vec<u8>,
    pointers: Vec<[u32; 2]>,
    links: Vec<[u32; 3]>,
}
impl Objects {
    pub fn new() -> Self {
        Self::default()
    }
    /// A new object of `class` with a span of `len` bytes; its index.
    pub fn object(&mut self, class: &'static str, len: usize) -> usize {
        let at = self.bytes.len();
        let len = len.div_ceil(16) * 16;
        self.bytes.resize(at + len, 0);
        let class = match self.classes.iter().position(|c| *c == class) {
            Some(k) => k,
            None => {
                self.classes.push(class);
                self.classes.len() - 1
            }
        };
        self.objects.push((class, at));
        self.objects.len() - 1
    }
    fn at(&self, object: usize, field: usize) -> usize {
        self.objects[object].1 + field
    }
    pub fn put(&mut self, object: usize, field: usize, v: u32) {
        let at = self.at(object, field);
        self.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put_f32(&mut self, object: usize, field: usize, v: f32) {
        self.put(object, field, v.to_bits());
    }
    pub fn put_u8(&mut self, object: usize, field: usize, v: u8) {
        let at = self.at(object, field);
        self.bytes[at] = v;
    }
    /// An array slot at `field` of `count` elements stored at `storage`
    /// (both in the same object).
    pub fn array(&mut self, object: usize, field: usize, count: u32, storage: usize) {
        self.put(object, field + 8, count);
        self.put(object, field + 12, 0x8000_0000 | count);
        if count > 0 {
            let (s, d) = (self.at(object, field), self.at(object, storage));
            self.pointers.push([s as u32, d as u32]);
        }
    }
    /// A text pointer at `field` to `s` written at `at` (same object).
    pub fn text(&mut self, object: usize, field: usize, at: usize, s: &str) {
        let (f, d) = (self.at(object, field), self.at(object, at));
        self.bytes[d..d + s.len()].copy_from_slice(s.as_bytes());
        self.bytes[d + s.len()] = 0;
        self.pointers.push([f as u32, d as u32]);
    }
    /// A link at `field` to object `target`.
    pub fn link(&mut self, object: usize, field: usize, target: usize) {
        let f = self.at(object, field);
        self.links
            .push([f as u32, 2, self.objects[target].1 as u32]);
    }
    /// The file's bytes.
    pub fn file(&self) -> Vec<u8> {
        let mut names: Vec<(&'static str, u32)> = vec![("hkClass", 7)];
        names.extend(self.classes.iter().map(|c| (*c, 7)));
        let mut pointers = self.pointers.clone();
        pointers.sort_unstable();
        let mut links = self.links.clone();
        links.sort_unstable();
        let objects = self
            .objects
            .iter()
            .map(|(c, at)| [*at as u32, 0, name_offset(&names, c + 1)])
            .collect();
        let root_class = self.objects.first().map_or(0, |o| o.0 + 1);
        build(&Spec {
            object_bytes: self.bytes.len(),
            content: vec![(0, self.bytes.clone())],
            pointers,
            links,
            objects,
            root: [2, 0, 0, name_offset(&names, root_class)],
            names: names.clone(),
        })
    }
}
