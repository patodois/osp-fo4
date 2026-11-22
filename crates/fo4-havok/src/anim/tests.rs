use crate::tests::{build, name_offset, read, Spec};
use crate::*;
use fo4_core::Budget;

const NAMES: [&str; 6] = [
    "hkClass",
    ROOT_LEVEL,
    ANIMATION_CONTAINER,
    SKELETON,
    BINDING,
    SPLINE,
];

/// A synthetic animation file laid out object by object. Offsets are in the
/// object area of `__data__`.
pub(crate) struct Lay {
    pub bytes: Vec<u8>,
    pub pointers: Vec<[u32; 2]>,
    pub links: Vec<[u32; 3]>,
    pub objects: Vec<[u32; 3]>,
}
impl Lay {
    fn put(&mut self, at: usize, v: u32) {
        self.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn putf(&mut self, at: usize, v: &[f32]) {
        for (k, x) in v.iter().enumerate() {
            self.put(at + 4 * k, x.to_bits());
        }
    }
    fn array(&mut self, at: usize, count: u32, storage: usize) {
        self.put(at + 8, count);
        self.put(at + 12, ARRAY_FLAG | count);
        if count > 0 {
            self.pointers.push([at as u32, storage as u32]);
        }
    }
    fn text(&mut self, source: usize, at: usize, s: &str) {
        self.bytes[at..at + s.len()].copy_from_slice(s.as_bytes());
        self.pointers.push([source as u32, at as u32]);
    }
    fn link(&mut self, source: usize, object: usize) {
        self.links.push([source as u32, 2, self.objects[object][0]]);
    }
}

/// Object offsets: root 0, container 96, skeleton 224, binding 560,
/// spline 688; object bytes end at 976.
pub(crate) const AT: [usize; 5] = [0, 96, 224, 560, 688];
const END: usize = 976;

pub(crate) fn lay() -> Lay {
    let names: Vec<(&'static str, u32)> = NAMES.iter().map(|n| (*n, 7)).collect();
    let mut l = Lay {
        bytes: vec![0u8; END],
        pointers: Vec::new(),
        links: Vec::new(),
        objects: (0..5)
            .map(|k| [AT[k] as u32, 0, name_offset(&names, k + 1)])
            .collect(),
    };
    // Root level container: one entry at +16.
    let r = AT[0];
    l.array(r, 1, r + 16);
    l.text(r + 16, r + 48, "A");
    l.text(r + 24, r + 64, ANIMATION_CONTAINER);
    l.link(r + 32, 1);
    // Animation container: one skeleton, animation and binding.
    let c = AT[1];
    for (k, (field, object)) in [(16, 2), (32, 4), (48, 3)].into_iter().enumerate() {
        l.array(c + field, 1, c + 96 + 8 * k);
        l.link(c + 96 + 8 * k, object);
    }
    for field in [64, 80] {
        l.array(c + field, 0, 0);
    }
    // Skeleton of two bones.
    let s = AT[2];
    l.text(s + 16, s + 144, "Root");
    l.array(s + 24, 2, s + 160);
    l.array(s + 40, 2, s + 176);
    l.array(s + 56, 2, s + 208);
    for field in [72, 88, 104, 120] {
        l.array(s + field, 0, 0);
    }
    l.put(s + 160, 0x0000_ffff);
    l.text(s + 176, s + 304, "Root");
    l.text(s + 192, s + 320, "Pelvis");
    l.bytes[s + 200] = 1;
    l.putf(
        s + 208,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    );
    l.putf(
        s + 256,
        &[0.0, 2.0, 60.0, 9.0, 0.6, 0.0, 0.0, 0.8, 1.0, 1.0, 1.0, 0.0],
    );
    // Binding of two tracks.
    let b = AT[3];
    l.text(b + 16, b + 96, "Root");
    l.link(b + 24, 4);
    l.array(b + 32, 2, b + 112);
    l.put(b + 112, 0x0001_0000);
    for field in [48, 64] {
        l.array(b + field, 0, 0);
    }
    l.bytes[b + 80] = 2;
    // Spline animation: 31 frames in one block, 2 tracks, 32 data bytes.
    let a = AT[4];
    l.put(a + 16, SPLINE_KIND);
    l.putf(a + 20, &[1.0]);
    l.put(a + 24, 2);
    l.array(a + 40, 2, a + 176);
    for (k, w) in [31u32, 1, 256, 8].into_iter().enumerate() {
        l.put(a + 56 + 4 * k, w);
    }
    let fd = 1.0f32 / 30.0;
    let bd = 255.0 * fd;
    l.putf(a + 72, &[bd, 1.0 / bd, fd]);
    l.array(a + 88, 1, a + 224);
    l.array(a + 104, 1, a + 240);
    l.put(a + 240, 24);
    for field in [120, 136] {
        l.array(a + field, 0, 0);
    }
    l.array(a + 152, 32, a + 256);
    for k in 0..32 {
        l.bytes[a + 256 + k] = k as u8;
    }
    l
}

pub(crate) fn file(l: &Lay) -> Vec<u8> {
    let names: Vec<(&'static str, u32)> = NAMES.iter().map(|n| (*n, 7)).collect();
    let root = name_offset(&names, 1);
    build(&Spec {
        names,
        object_bytes: END,
        content: vec![(0, l.bytes.clone())],
        pointers: l.pointers.clone(),
        links: l.links.clone(),
        objects: l.objects.clone(),
        root: [2, 0, 0, root],
    })
}

/// Absolute file offset of object byte `at`: base 1000, `__data__` start.
fn abs(bytes: &[u8], at: usize) -> u64 {
    let c = read(bytes).unwrap();
    1000 + c.data().unwrap().start as u64 + at as u64
}

fn b() -> Budget {
    Budget::new(1 << 20)
}

#[test]
fn reads_every_object_of_a_synthetic_file() {
    let bytes = file(&lay());
    let c = read(&bytes).unwrap();
    let e = root_entries(&c, 0, "x.hkx", 1000, &b()).unwrap();
    assert_eq!(
        e,
        vec![RootEntry {
            name: b"A",
            class_name: ANIMATION_CONTAINER.as_bytes(),
            object: 1
        }]
    );
    let a = AnimationContainer::read(&c, 1, "x.hkx", 1000, &b()).unwrap();
    assert_eq!(
        (a.skeletons, a.animations, a.bindings, a.unread_counts),
        (vec![2], vec![4], vec![3], [0, 0])
    );
    let s = Skeleton::read(&c, 2, "x.hkx", 1000, &b()).unwrap();
    assert_eq!(s.name, b"Root");
    assert_eq!(s.parents, vec![-1, 0]);
    assert_eq!(s.bones, vec![&b"Root"[..], b"Pelvis"]);
    assert_eq!(s.bone_flags, vec![0, 1]);
    assert_eq!(s.pose[1].translation, [0.0, 2.0, 60.0]);
    assert_eq!(s.pose[1].rotation, [0.6, 0.0, 0.0, 0.8]);
    assert_eq!(s.pose[1].scale, [1.0; 3]);
    let k = Binding::read(&c, 3, "x.hkx", 1000, &b()).unwrap();
    assert_eq!(
        (
            k.skeleton_name,
            k.animation,
            &k.transform_tracks[..],
            k.byte80
        ),
        (&b"Root"[..], 4, &[0i16, 1][..], 2)
    );
    assert!(k.fits(&s));
    let h = AnimationHeader::read(&c, 4, "x.hkx", 1000).unwrap();
    assert_eq!(
        (h.kind, h.duration, h.transform_tracks, h.float_tracks),
        (3, 1.0, 2, 0)
    );
    assert_eq!((h.reference_frame, h.annotation_tracks.count), (None, 2));
    let sp = SplineAnimation::read(&c, 4, "x.hkx", 1000, &b()).unwrap();
    assert_eq!(
        (sp.frames, sp.frames_per_block, sp.mask_bytes),
        (31, 256, 8)
    );
    assert_eq!(
        (sp.block_offsets.clone(), sp.float_offsets.clone()),
        (vec![0], vec![24])
    );
    assert_eq!(sp.data.len(), 32);
    assert_eq!(sp.block(0).map(|x| x[31]), Some(31));
    assert_eq!(sp.block(1), None);
}

#[test]
fn binding_fit_needs_the_name_and_distinct_bones_in_range() {
    let bytes = file(&lay());
    let c = read(&bytes).unwrap();
    let s = Skeleton::read(&c, 2, "x.hkx", 1000, &b()).unwrap();
    let k = Binding::read(&c, 3, "x.hkx", 1000, &b()).unwrap();
    let mut other = k.clone();
    other.skeleton_name = b"Other";
    assert!(!other.fits(&s));
    let mut other = k.clone();
    other.transform_tracks = vec![0, 2];
    assert!(!other.fits(&s));
    let mut other = k.clone();
    other.transform_tracks = vec![1, 1];
    assert!(!other.fits(&s));
    let mut other = k;
    other.transform_tracks = vec![-1];
    assert!(!other.fits(&s));
}

fn err(l: &Lay, f: impl Fn(&Container<'_>) -> fo4_core::Result<()>) -> (String, u64) {
    let bytes = file(l);
    let c = read(&bytes).unwrap();
    let e = f(&c).expect_err("must fail");
    (e.code, e.source.offset)
}

#[test]
fn skeleton_errors_are_named() {
    let s = AT[2];
    let skel = |c: &Container<'_>| Skeleton::read(c, 2, "x.hkx", 1000, &b()).map(|_| ());
    let bytes = file(&lay());
    // Bone 1 whose parent is itself.
    let mut l = lay();
    l.put(s + 160, 0x0001_ffff);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 162))
    );
    // Bone 0 with a parent.
    let mut l = lay();
    l.put(s + 160, 0x0000_0000);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 160))
    );
    // A rotation that is not of unit norm.
    let mut l = lay();
    l.putf(s + 272, &[0.6, 0.0, 0.0, 0.81]);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 272))
    );
    // A non-finite translation.
    let mut l = lay();
    l.putf(s + 256, &[f32::NAN]);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 256))
    );
    // Pose count unlike the bone count.
    let mut l = lay();
    l.put(s + 64, 1);
    l.put(s + 68, ARRAY_FLAG | 1);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 64))
    );
    // A bone name without its first-table entry.
    let mut l = lay();
    l.pointers.retain(|p| p[0] as usize != s + 192);
    assert_eq!(
        err(&l, skel),
        ("InvalidHavokSkeleton".into(), abs(&bytes, s + 192))
    );
    // Another class.
    let bytes = file(&lay());
    let c = read(&bytes).unwrap();
    let e = Skeleton::read(&c, 3, "x.hkx", 1000, &b()).unwrap_err();
    assert_eq!(e.code, "UnexpectedHavokClass");
}

#[test]
fn binding_container_and_root_errors_are_named() {
    let bytes = file(&lay());
    let bind = |c: &Container<'_>| Binding::read(c, 3, "x.hkx", 1000, &b()).map(|_| ());
    // Animation link to the skeleton.
    let mut l = lay();
    l.links.retain(|x| x[0] as usize != AT[3] + 24);
    l.link(AT[3] + 24, 2);
    assert_eq!(
        err(&l, bind),
        ("InvalidHavokBinding".into(), abs(&bytes, AT[3] + 24))
    );
    let mut l = lay();
    l.links.retain(|x| x[0] as usize != AT[3] + 24);
    assert_eq!(
        err(&l, bind),
        ("InvalidHavokBinding".into(), abs(&bytes, AT[3] + 24))
    );
    // A container list entry of the wrong class.
    let cont = |c: &Container<'_>| AnimationContainer::read(c, 1, "x.hkx", 1000, &b()).map(|_| ());
    let mut l = lay();
    l.links.retain(|x| x[0] as usize != AT[1] + 112);
    l.link(AT[1] + 112, 2);
    assert_eq!(
        err(&l, cont),
        (
            "InvalidHavokAnimationContainer".into(),
            abs(&bytes, AT[1] + 112)
        )
    );
    // A root entry whose class name is not its object's.
    let root = |c: &Container<'_>| root_entries(c, 0, "x.hkx", 1000, &b()).map(|_| ());
    let mut l = lay();
    l.bytes[64] = b'x';
    assert_eq!(
        err(&l, root),
        ("InvalidHavokRootLevel".into(), abs(&bytes, 32))
    );
}

#[test]
fn spline_errors_are_named() {
    let a = AT[4];
    let bytes = file(&lay());
    let sp = |c: &Container<'_>| SplineAnimation::read(c, 4, "x.hkx", 1000, &b()).map(|_| ());
    let code = "InvalidHavokAnimation";
    let mut l = lay();
    l.put(a + 16, 0);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 16)));
    let mut l = lay();
    l.putf(a + 20, &[-1.0]);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 20)));
    // 300 frames need two blocks of 256.
    let mut l = lay();
    l.put(a + 56, 300);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 60)));
    let mut l = lay();
    l.put(a + 68, 12);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 68)));
    let mut l = lay();
    l.put(a + 64, 1);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 56)));
    // A first block offset that is not 0.
    let mut l = lay();
    l.put(a + 224, 16);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 88)));
    // A non-empty array at +120.
    let mut l = lay();
    l.put(a + 128, 1);
    l.put(a + 132, ARRAY_FLAG | 1);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 128)));
    // Float offsets of another length.
    let mut l = lay();
    l.put(a + 112, 0);
    l.put(a + 116, ARRAY_FLAG);
    l.pointers.retain(|p| p[0] as usize != a + 104);
    assert_eq!(err(&l, sp), (code.into(), abs(&bytes, a + 112)));
}

#[test]
fn single_byte_changes_and_short_spans_never_panic() {
    let bytes = file(&lay());
    let d = read(&bytes).unwrap().data().unwrap().start as usize;
    for at in d..d + END {
        for value in [0u8, 1, 0x80, 0xff] {
            let mut m = bytes.clone();
            m[at] = value;
            let Ok(c) = read(&m) else { continue };
            let _ = root_entries(&c, 0, "x.hkx", 0, &b());
            let _ = AnimationContainer::read(&c, 1, "x.hkx", 0, &b());
            let _ = Skeleton::read(&c, 2, "x.hkx", 0, &b());
            let _ = Binding::read(&c, 3, "x.hkx", 0, &b());
            let _ = SplineAnimation::read(&c, 4, "x.hkx", 0, &b());
        }
    }
    // Every object index, including ones out of range, with every reader.
    let c = read(&bytes).unwrap();
    for i in 0..7 {
        let _ = root_entries(&c, i, "x.hkx", 0, &b());
        let _ = AnimationContainer::read(&c, i, "x.hkx", 0, &b());
        let _ = Skeleton::read(&c, i, "x.hkx", 0, &b());
        let _ = Binding::read(&c, i, "x.hkx", 0, &b());
        let _ = SplineAnimation::read(&c, i, "x.hkx", 0, &b());
    }
    // The budget bounds the skeleton's vectors.
    assert_eq!(
        Skeleton::read(&c, 2, "x.hkx", 0, &Budget::new(8))
            .unwrap_err()
            .code,
        "LimitExceeded"
    );
}
