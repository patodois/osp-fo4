//! Synthetic movies built in memory with the `swf` writer; no game data.
use super::*;
use std::io::Write as _;
use swf::{
    Color, Compression as C, FillStyle, Fixed8, Header, PlaceObject, PlaceObjectAction, Rectangle,
    Shape, ShapeFlag, ShapeRecord, ShapeStyles, StyleChangeData, Tag, Twips,
};

fn header(frames: u16) -> Header {
    Header {
        compression: C::None,
        version: 17,
        stage_size: Rectangle {
            x_min: Twips::ZERO,
            x_max: Twips::from_pixels(100.0),
            y_min: Twips::ZERO,
            y_max: Twips::from_pixels(50.0),
        },
        frame_rate: Fixed8::from_f32(30.0),
        num_frames: frames,
    }
}

fn write(tags: &[Tag<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    swf::write_swf(&header(1), tags, &mut out).unwrap();
    out
}

/// The same movie with its body zlib-compressed (`CWS`).
fn compress(fws: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&fws[8..]).unwrap();
    let mut out = b"CWS".to_vec();
    out.extend_from_slice(&fws[3..8]);
    out.extend_from_slice(&z.finish().unwrap());
    out
}

fn square(id: u16, px: f64, colour: Color) -> Shape {
    let t = Twips::from_pixels(px);
    let edge = |dx: Twips, dy: Twips| ShapeRecord::StraightEdge {
        delta: swf::PointDelta::new(dx, dy),
    };
    Shape {
        version: 1,
        id,
        shape_bounds: Rectangle {
            x_min: Twips::ZERO,
            x_max: t,
            y_min: Twips::ZERO,
            y_max: t,
        },
        edge_bounds: Rectangle {
            x_min: Twips::ZERO,
            x_max: t,
            y_min: Twips::ZERO,
            y_max: t,
        },
        flags: ShapeFlag::empty(),
        styles: ShapeStyles {
            fill_styles: vec![FillStyle::Color(colour)],
            line_styles: vec![],
        },
        shape: vec![
            ShapeRecord::StyleChange(Box::new(StyleChangeData {
                move_to: Some(swf::Point::new(Twips::ZERO, Twips::ZERO)),
                fill_style_0: None,
                fill_style_1: Some(1),
                line_style: None,
                new_styles: None,
            })),
            edge(t, Twips::ZERO),
            edge(Twips::ZERO, t),
            edge(-t, Twips::ZERO),
            edge(Twips::ZERO, -t),
        ],
    }
}

fn place(id: u16, depth: u16, x: f64, y: f64) -> Tag<'static> {
    Tag::PlaceObject(Box::new(PlaceObject {
        version: 2,
        action: PlaceObjectAction::Place(id),
        depth,
        matrix: Some(swf::Matrix::translate(
            Twips::from_pixels(x),
            Twips::from_pixels(y),
        )),
        color_transform: None,
        ratio: None,
        name: None,
        clip_depth: None,
        class_name: None,
        filters: None,
        background_color: None,
        blend_mode: None,
        clip_actions: None,
        has_image: false,
        is_bitmap_cached: None,
        is_visible: None,
        amf_data: None,
    }))
}

const RED: Color = Color {
    r: 255,
    g: 0,
    b: 0,
    a: 255,
};

fn sample() -> Vec<u8> {
    let abc: &[u8] = &[1, 2, 3, 4, 5];
    write(&[
        Tag::FileAttributes(swf::FileAttributes::IS_ACTION_SCRIPT_3),
        Tag::SetBackgroundColor(Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        }),
        Tag::DefineShape(Box::new(square(1, 10.0, RED))),
        Tag::DefineSprite(swf::Sprite {
            id: 2,
            num_frames: 1,
            tags: vec![
                place(1, 1, 0.0, 0.0),
                Tag::FrameLabel(swf::FrameLabel {
                    label: "a".into(),
                    is_anchor: false,
                }),
                Tag::ShowFrame,
            ],
        }),
        Tag::ExportAssets(vec![swf::ExportedAsset {
            id: 2,
            name: "Thing".into(),
        }]),
        Tag::SymbolClass(vec![swf::SymbolClassLink {
            id: 0,
            class_name: "Root".into(),
        }]),
        Tag::DoAbc2(swf::DoAbc2 {
            flags: swf::DoAbc2Flag::LAZY_INITIALIZE,
            name: "".into(),
            data: abc,
        }),
        place(2, 1, 20.0, 10.0),
        Tag::ShowFrame,
    ])
}

#[test]
fn reads_header_census_and_definitions() {
    let b = Budget::new(64 << 20);
    let m = Movie::read(&sample(), "t", &Limits::default(), &b).unwrap();
    assert_eq!(&m.signature, b"FWS");
    assert_eq!(m.compression, Compression::None);
    assert_eq!(m.version, 17);
    assert_eq!(m.stage, [0, 2000, 0, 1000]);
    assert_eq!(m.frame_rate_raw, 30 * 256);
    assert_eq!(m.frames, 1);
    assert_eq!(m.attributes, Some(0x08));
    assert_eq!(m.background, Some([0, 0, 255, 255]));
    assert!(m.end_found);
    assert_eq!(m.trailing, 0);
    assert!(m.tag_errors.is_empty());
    assert_eq!(u64::from(m.declared_len), m.body_len + 8);
    // Root: attributes, background, shape, sprite, export, class, abc,
    // place, show; sprite: place, label, show.
    assert_eq!(m.tags[&1].count, 2);
    assert_eq!(m.tags[&26].count, 2);
    assert_eq!(m.tags[&SPRITE].count, 1);
    assert_eq!(m.exports, vec![(2, "Thing".to_string())]);
    assert_eq!(m.root_class(), Some("Root"));
    assert_eq!(m.shapes.len(), 1);
    assert_eq!(m.shapes[0].fills, [1, 0, 0, 0, 0]);
    assert_eq!(m.shapes[0].straight, 4);
    assert_eq!(
        m.sprites,
        vec![SpriteStat {
            id: 2,
            frames: 1,
            tags: 3,
            places: 1,
            labels: 1
        }]
    );
    assert_eq!(m.labels, vec!["a".to_string()]);
    assert_eq!(m.actions.len(), 1);
    let a = &m.actions[0];
    assert_eq!(
        (a.kind, a.len, a.lazy, a.sprite),
        (ActionKind::Abc, 5, true, None)
    );
    assert_eq!(m.action_bytes(a), Some(&[1u8, 2, 3, 4, 5][..]));
    assert_eq!(tag_name(82), "DoAbc2");
    assert_eq!(tag_name(1000), "unknown");
}

#[test]
fn zlib_movie_reads_as_the_plain_one() {
    let b = Budget::new(64 << 20);
    let plain = Movie::read(&sample(), "t", &Limits::default(), &b).unwrap();
    let z = Movie::read(&compress(&sample()), "t", &Limits::default(), &b).unwrap();
    assert_eq!(z.compression, Compression::Zlib);
    assert_eq!(z.data, plain.data);
    assert_eq!(z.tags, plain.tags);
    assert_eq!(z.body_len, plain.body_len);
}

#[test]
fn malformed_headers_are_named_errors() {
    let b = Budget::new(64 << 20);
    let l = Limits::default();
    let code = |bytes: &[u8]| Movie::read(bytes, "t", &l, &b).unwrap_err().code;
    assert_eq!(code(b"FWS"), "Truncated");
    assert_eq!(code(b"XYZ\x11\x10\0\0\0"), "BadSignature");
    assert_eq!(code(b"FWS\x11\x04\0\0\0"), "BadLength");
    assert_eq!(code(b"ZWS\x11\x20\0\0\0"), "Unsupported");
    assert_eq!(code(b"FWS\0\x20\0\0\0"), "BadVersion");
    let mut s = sample();
    s[3] = 17;
    // A declared length past the cap.
    s[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(code(&s), "LimitExceeded");
    // A zlib body whose stream does not give the declared length.
    let mut z = compress(&sample());
    let n = u32::from_le_bytes([z[4], z[5], z[6], z[7]]) + 1;
    z[4..8].copy_from_slice(&n.to_le_bytes());
    assert_eq!(code(&z), "DecompressedSizeMismatch");
    let small = Limits {
        max_stored: 10,
        ..Limits::default()
    };
    assert_eq!(
        Movie::read(&sample(), "t", &small, &b).unwrap_err().code,
        "LimitExceeded"
    );
    let few = Limits {
        max_tags: 3,
        ..Limits::default()
    };
    assert_eq!(
        Movie::read(&sample(), "t", &few, &b).unwrap_err().code,
        "LimitExceeded"
    );
}

#[test]
fn a_tag_running_past_the_end_is_reported_not_fatal() {
    let mut s = sample();
    // Cut the movie inside its last placement and fix the declared length.
    s.truncate(s.len() - 6);
    let n = s.len() as u32;
    s[4..8].copy_from_slice(&n.to_le_bytes());
    let b = Budget::new(64 << 20);
    let m = Movie::read(&s, "t", &Limits::default(), &b).unwrap();
    assert!(!m.end_found);
    assert_eq!(m.tag_errors.len(), 1);
    assert_eq!(m.tag_errors[0].code, 26);
    assert_eq!(m.shapes.len(), 1);
    // Cut inside a tag header.
    let mut s = sample();
    s.truncate(s.len() - 3);
    let n = s.len() as u32;
    s[4..8].copy_from_slice(&n.to_le_bytes());
    let m = Movie::read(&s, "t", &Limits::default(), &b).unwrap();
    assert_eq!(m.tag_errors.len(), 1);
    assert_eq!(m.tag_errors[0].code, NO_CODE);
    assert_eq!(m.trailing, 1);
}

#[test]
fn a_refused_tag_is_skipped_by_its_stored_length() {
    // A DefineShape whose body is too short to parse, then a ShowFrame.
    let mut body = Vec::new();
    // Stage rectangle (5-bit fields of zero), frame rate, frame count.
    body.extend_from_slice(&[0x00, 0x00, 0x1e, 0x00, 0x01]);
    body.extend_from_slice(&((2u16 << 6) | 3).to_le_bytes());
    body.extend_from_slice(&[1, 0, 0xff]);
    body.extend_from_slice(&(1u16 << 6).to_le_bytes());
    body.extend_from_slice(&[0, 0]);
    let mut s = b"FWS\x11".to_vec();
    s.extend_from_slice(&(body.len() as u32 + 8).to_le_bytes());
    s.extend_from_slice(&body);
    let b = Budget::new(64 << 20);
    let m = Movie::read(&s, "t", &Limits::default(), &b).unwrap();
    assert_eq!(m.tag_errors.len(), 1);
    assert_eq!(m.tag_errors[0].code, 2);
    assert_eq!(m.tags[&1].count, 1);
    assert!(m.end_found);
}

#[test]
fn noise_never_panics() {
    // Deterministic noise behind valid headers of both kinds.
    let b = Budget::new(256 << 20);
    let l = Limits::default();
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for round in 0..400 {
        let len = 9 + (round * 7) % 300;
        let mut body = Vec::with_capacity(len);
        for _ in 0..len {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            body.push(x as u8);
        }
        let mut s = b"FWS\x11".to_vec();
        s.extend_from_slice(&(body.len() as u32 + 8).to_le_bytes());
        s.extend_from_slice(&body);
        let _ = Movie::read(&s, "t", &l, &b);
        if let Ok(m) = Movie::read(&s, "t", &l, &b) {
            let _ = raster::first_frame(&m, 0.1, 1 << 16, &l, &b);
        }
        // Every prefix of a valid movie.
        let v = sample();
        let cut = 8 + round % (v.len() - 8);
        let mut p = v[..cut].to_vec();
        p[4..8].copy_from_slice(&(cut as u32).to_le_bytes());
        let _ = Movie::read(&p, "t", &l, &b);
        let _ = abc::read(&body, "t", &Default::default());
        let _ = abc::uses(&body, "t");
    }
}

#[test]
fn first_frame_fills_the_placed_sprite() {
    let b = Budget::new(64 << 20);
    let l = Limits::default();
    let m = Movie::read(&sample(), "t", &l, &b).unwrap();
    let (img, counts) = raster::first_frame(&m, 1.0, 1 << 20, &l, &b).unwrap();
    assert_eq!((img.width, img.height), (100, 50));
    assert_eq!(counts.shapes_drawn, 1);
    assert_eq!(counts.sprites_entered, 1);
    assert_eq!(counts.fills_solid, 1);
    let at = |x: usize, y: usize| img.pixels[y * 100 + x];
    // The square sits at (20, 10) to (30, 20): red inside, blue outside.
    assert_eq!(at(25, 15), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(at(19, 15), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(at(30, 15), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(at(25, 9), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        raster::first_frame(&m, 1.0, 10, &l, &b).unwrap_err().code,
        "LimitExceeded"
    );
}

#[test]
fn fill_style_zero_fills_the_left_side() {
    // The square traced the other way round with fill style 0: the same
    // area (SWF19: fill style 0 on the left of the edge).
    let mut sh = square(1, 10.0, RED);
    if let ShapeRecord::StyleChange(c) = &mut sh.shape[0] {
        c.fill_style_0 = Some(1);
        c.fill_style_1 = None;
    }
    let t = Twips::from_pixels(10.0);
    let edge = |dx: Twips, dy: Twips| ShapeRecord::StraightEdge {
        delta: swf::PointDelta::new(dx, dy),
    };
    sh.shape.truncate(1);
    sh.shape.extend([
        edge(Twips::ZERO, t),
        edge(t, Twips::ZERO),
        edge(Twips::ZERO, -t),
        edge(-t, Twips::ZERO),
    ]);
    let s = write(&[
        Tag::DefineShape(Box::new(sh)),
        place(1, 1, 0.0, 0.0),
        Tag::ShowFrame,
    ]);
    let b = Budget::new(64 << 20);
    let l = Limits::default();
    let m = Movie::read(&s, "t", &l, &b).unwrap();
    let (img, _) = raster::first_frame(&m, 1.0, 1 << 20, &l, &b).unwrap();
    assert_eq!(img.pixels[5 * 100 + 5], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(img.pixels[5 * 100 + 15][3], 0.0);
}

/// A tiny ABC block: class `Menu` with an untyped member `code` the movie
/// never gives a value, a public method `Show(a, b)` that calls
/// `this.code.Hello(a)`, and a private member `own` it assigns.
fn tiny_abc() -> Vec<u8> {
    use swf::avm2::types::*;
    let s = |i: u32| Index::<String>::new(i);
    let mn = |i: u32| Index::<Multiname>::new(i);
    // Strings 1..: "", "Menu", "code", "Show", "Hello", "Object", "own".
    let strings: Vec<Vec<u8>> = ["", "Menu", "code", "Show", "Hello", "Object", "own"]
        .iter()
        .map(|x| x.as_bytes().to_vec())
        .collect();
    let pool = ConstantPool {
        ints: vec![],
        uints: vec![],
        doubles: vec![],
        strings,
        // Namespace 1: the public package "".
        namespaces: vec![Namespace::Package(s(1))],
        namespace_sets: vec![],
        // Multinames 1..: Menu, code, Show, Hello, Object, own.
        multinames: [2, 3, 4, 5, 6, 7]
            .iter()
            .map(|&n| Multiname::QName {
                namespace: Index::new(1),
                name: s(n),
            })
            .collect(),
    };
    let method = |params: u32| Method {
        name: s(0),
        params: (0..params)
            .map(|_| MethodParam {
                name: None,
                kind: mn(0),
                default_value: None,
            })
            .collect(),
        return_type: mn(0),
        flags: MethodFlags::empty(),
        body: None,
    };
    let mut code = Vec::new();
    {
        let mut w = swf::avm2::write::Writer::new(&mut code);
        for op in [
            Op::GetLocal { index: 0 },
            Op::PushScope,
            Op::GetLocal { index: 0 },
            Op::PushByte { value: 7 },
            Op::InitProperty { index: mn(6) },
            Op::GetLocal { index: 0 },
            Op::GetProperty { index: mn(2) },
            Op::GetLocal { index: 1 },
            Op::CallPropVoid {
                index: mn(4),
                num_args: 1,
            },
            Op::ReturnVoid,
        ] {
            w.write_op(&op).unwrap();
        }
    }
    let empty = |m: u32| {
        let mut c = Vec::new();
        let mut w = swf::avm2::write::Writer::new(&mut c);
        w.write_op(&Op::ReturnVoid).unwrap();
        MethodBody {
            method: Index::new(m),
            max_stack: 1,
            num_locals: 1,
            init_scope_depth: 0,
            max_scope_depth: 1,
            code: c,
            exceptions: vec![],
            traits: vec![],
        }
    };
    let abc = AbcFile {
        major_version: 46,
        minor_version: 16,
        constant_pool: pool,
        // 0: instance init, 1: class init, 2: Show, 3: script init.
        methods: vec![method(0), method(0), method(2), method(0)],
        metadata: vec![],
        instances: vec![Instance {
            name: mn(1),
            super_name: mn(5),
            is_sealed: false,
            is_final: false,
            is_interface: false,
            protected_namespace: None,
            interfaces: vec![],
            init_method: Index::new(0),
            traits: vec![
                Trait {
                    name: mn(2),
                    kind: TraitKind::Slot {
                        slot_id: 0,
                        type_name: mn(5),
                        value: None,
                    },
                    metadata: vec![],
                    is_final: false,
                    is_override: false,
                },
                Trait {
                    name: mn(6),
                    kind: TraitKind::Slot {
                        slot_id: 0,
                        type_name: mn(0),
                        value: None,
                    },
                    metadata: vec![],
                    is_final: false,
                    is_override: false,
                },
                Trait {
                    name: mn(3),
                    kind: TraitKind::Method {
                        disp_id: 0,
                        method: Index::new(2),
                    },
                    metadata: vec![],
                    is_final: false,
                    is_override: false,
                },
            ],
        }],
        classes: vec![Class {
            init_method: Index::new(1),
            traits: vec![],
        }],
        scripts: vec![Script {
            init_method: Index::new(3),
            traits: vec![],
        }],
        method_bodies: vec![
            empty(0),
            empty(1),
            MethodBody {
                method: Index::new(2),
                max_stack: 3,
                num_locals: 3,
                init_scope_depth: 0,
                max_scope_depth: 1,
                code,
                exceptions: vec![],
                traits: vec![],
            },
            empty(3),
        ],
    };
    let mut out = Vec::new();
    swf::avm2::write::Writer::new(&mut out).write(abc).unwrap();
    out
}

#[test]
fn abc_reading_finds_the_host_object_and_entry_points() {
    let bytes = tiny_abc();
    let r = abc::read(&bytes, "t", &Default::default()).unwrap();
    assert_eq!(r.classes.len(), 1);
    assert_eq!(r.classes[0].name, "Menu");
    assert_eq!(
        r.host_objects,
        vec![("code".to_string(), "Object".to_string())]
    );
    assert_eq!(r.host_classes, vec!["Menu".to_string()]);
    assert_eq!(r.host_calls.len(), 1);
    let h = &r.host_calls[0];
    assert_eq!(h.via, abc::Via::Object("code".into()));
    assert_eq!((h.name.as_str(), h.args), ("Hello", 1));
    assert_eq!((h.class.as_str(), h.method.as_str()), ("Menu", "Show"));
    let pm = r.public_methods("Menu");
    assert_eq!(pm.len(), 1);
    assert_eq!((pm[0].1.name.as_str(), pm[0].1.params), ("Show", 2));
    // A placement named like the member makes it a display child.
    let named = ["code".to_string()].into_iter().collect();
    let r = abc::read(&bytes, "t", &named).unwrap();
    assert!(r.host_objects.is_empty() && r.host_calls.is_empty());
    let u = abc::uses(&bytes, "t").unwrap();
    assert!(u
        .iter()
        .any(|u| u.name == "own" && u.kind == abc::UseKind::Write));
    assert!(abc::listing(&bytes, "t", "Menu")
        .unwrap()
        .contains("== Menu.Show"));
    assert_eq!(
        abc::read(&bytes[..bytes.len() / 2], "t", &Default::default())
            .unwrap_err()
            .code,
        "BadAbc"
    );
}

#[test]
fn placement_paths_follow_named_placements() {
    let named = |id: u16, depth: u16, name: &'static str| {
        let mut p = place(id, depth, 0.0, 0.0);
        if let Tag::PlaceObject(p) = &mut p {
            p.name = Some(name.into());
        }
        p
    };
    let s = write(&[
        Tag::DefineShape(Box::new(square(1, 1.0, RED))),
        Tag::DefineSprite(swf::Sprite {
            id: 2,
            num_frames: 1,
            tags: vec![named(1, 1, "leaf"), Tag::ShowFrame],
        }),
        named(2, 1, "group"),
        Tag::ShowFrame,
    ]);
    let b = Budget::new(64 << 20);
    let m = Movie::read(&s, "t", &Limits::default(), &b).unwrap();
    assert_eq!(
        m.paths_to(1),
        vec![vec!["group".to_string(), "leaf".to_string()]]
    );
    assert_eq!(m.paths_to(2), vec![vec!["group".to_string()]]);
    assert!(m.paths_to(9).is_empty());
}
