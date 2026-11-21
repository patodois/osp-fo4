use super::*;
use fo4_core::{Game, Limits, Manifest, PluginSpec, TestDir};
use fo4_plugin::land::VHGT_SIZE;

fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    for n in [data.len() as u32, flags, id, 0] {
        v.extend(n.to_le_bytes());
    }
    v.extend(131u16.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(data);
    v
}
fn group(kind: u32, label: u32, data: &[u8]) -> Vec<u8> {
    let mut v = b"GRUP".to_vec();
    v.extend((data.len() as u32 + 24).to_le_bytes());
    v.extend(label.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend([0; 8]);
    v.extend(data);
    v
}
/// Offset 2, first column +1 per row, other columns +1 per step east:
/// stored value (r, c) accumulates to 2 + (r + 1) + c.
fn ramp() -> Vec<u8> {
    let mut v = 2.0f32.to_le_bytes().to_vec();
    v.extend(std::iter::repeat_n(1u8, POINTS));
    v.extend([0; 3]);
    assert_eq!(v.len(), VHGT_SIZE);
    v
}
fn cell(id: u32, x: i32, y: i32, lands: &[Vec<u8>]) -> Vec<u8> {
    let mut c = sub(b"EDID", format!("Ext{id:x}\0").as_bytes());
    let mut xclc = x.to_le_bytes().to_vec();
    xclc.extend(y.to_le_bytes());
    xclc.extend(0x0000_0005u32.to_le_bytes());
    c.extend(sub(b"XCLC", &xclc));
    let mut out = record(b"CELL", id, 0, &c);
    if !lands.is_empty() {
        out.extend(group(6, id, &group(9, id, &lands.concat())));
    }
    out
}
struct Fixture {
    _temp: TestDir,
    manifest: Manifest,
}
fn fixture() -> Fixture {
    let land = |id: u32, flags: u32, vhgt: bool| {
        let mut p = sub(b"DATA", &[0; 4]);
        if vhgt {
            p.extend(sub(b"VHGT", &ramp()));
            // Quadrant 0: base LTEX 0x870; quadrant 1: one layer of 0x873.
            p.extend(sub(b"BTXT", &[0x70, 8, 0, 0, 0, 0, 0xff, 0xff]));
            p.extend(sub(b"ATXT", &[0x73, 8, 0, 0, 1, 0, 0, 0]));
            let mut point = 3u16.to_le_bytes().to_vec();
            point.extend([0, 0]);
            point.extend(0.5f32.to_le_bytes());
            p.extend(sub(b"VTXT", &point));
        }
        record(b"LAND", id, flags, &p)
    };
    let tnam = |id: u32, target: Option<u32>| {
        let mut p = sub(b"EDID", format!("Ground{id:x}\0").as_bytes());
        if let Some(t) = target {
            p.extend(sub(b"TNAM", &t.to_le_bytes()));
        }
        record(b"LTEX", id, 0, &p)
    };
    let ltex = [
        tnam(0x870, Some(0x880)),
        tnam(0x871, Some(0x810)),
        tnam(0x872, None),
        tnam(0x873, Some(0x881)),
    ]
    .concat();
    let mut set = sub(b"EDID", b"GroundSet\0");
    set.extend(sub(b"TX00", b"landscape\\ground_d.dds\0"));
    let mut material_only = sub(b"EDID", b"GroundMaterial\0");
    material_only.extend(sub(b"MNAM", b"landscape\\ground.bgsm\0"));
    let txst = [
        record(b"TXST", 0x880, 0, &set),
        record(b"TXST", 0x881, 0, &material_only),
    ]
    .concat();
    let cells = [
        cell(0x810, 3, -2, &[land(0x811, 0, true)]),
        cell(0x820, 4, -2, &[]),
        cell(0x830, 5, -2, &[land(0x831, 0, true), land(0x832, 0, true)]),
        cell(0x840, 6, -2, &[land(0x841, fo4_plugin::DELETED, true)]),
        cell(0x850, 7, -2, &[land(0x851, 0, false)]),
    ]
    .concat();
    let world = [
        record(b"WRLD", 0x800, 0, &sub(b"EDID", b"World\0")),
        group(1, 0x800, &group(4, 0, &group(5, 0, &cells))),
    ]
    .concat();
    let mut interior = sub(b"EDID", b"Room\0");
    interior.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut body = group(0, u32::from_le_bytes(*b"LTEX"), &ltex);
    body.extend(group(0, u32::from_le_bytes(*b"TXST"), &txst));
    body.extend(group(0, u32::from_le_bytes(*b"WRLD"), &world));
    body.extend(group(
        0,
        u32::from_le_bytes(*b"CELL"),
        &group(2, 0, &group(3, 0, &record(b"CELL", 0x860, 0, &interior))),
    ));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
    plugin.extend(body);
    let temp = TestDir::new("scene-terrain");
    let root = temp.join("Data");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    Fixture {
        manifest: Manifest {
            game: Game::Fallout4,
            archive_invalidation: None,
            root,
            language: "english".into(),
            plugins: vec![PluginSpec {
                name: "base.esm".into(),
                path: "base.esm".into(),
            }],
            mounts: vec![],
            required_masters: vec![],
            excluded: vec![],
            build: Some("synthetic".into()),
            limits: Limits::default(),
        },
        _temp: temp,
    }
}
fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    }
}
fn refusal(o: Outcome) -> &'static str {
    match o {
        Outcome::Refused(r) => r.code,
        Outcome::Built(_) => "built",
    }
}

#[test]
fn accumulation_follows_rows_from_the_offset() {
    let mut gradient = vec![0i8; POINTS];
    gradient[0] = 3;
    gradient[SIDE] = -1; // start of row 1
    gradient[SIDE + 4] = 2;
    gradient[2 * SIDE + 1] = -5;
    let h = Heights {
        subrecord: 0,
        offset: 10.0,
        gradient,
        trailing: [0; 3],
    };
    let z = accumulate(&h, &Budget::new(1 << 16)).unwrap();
    assert_eq!(z[0], 13.0);
    assert_eq!(z[32], 13.0);
    assert_eq!(z[SIDE], 12.0);
    assert_eq!(z[SIDE + 3], 12.0);
    assert_eq!(z[SIDE + 4], 14.0);
    assert_eq!(z[SIDE + 32], 14.0);
    // Row 2 starts from the column sum of row 1, not from the end of row 1.
    assert_eq!(z[2 * SIDE], 12.0);
    assert_eq!(z[2 * SIDE + 1], 7.0);
    assert!(accumulate(&h, &Budget::new(8)).is_err());
}

#[test]
fn quad_heights_interpolate_on_the_chosen_diagonal() {
    let corners = [0.0, 0.0, 0.0, 8.0];
    for d in Diagonal::ALL {
        for (u, v, z) in [
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (1.0, 1.0, 8.0),
        ] {
            assert_eq!(quad_height(d, corners, u, v), z);
        }
    }
    // Centre: on the SW-NE diagonal it is half of NE; on the other it is 0.
    assert_eq!(
        quad_height(Diagonal::SouthwestNortheast, corners, 0.5, 0.5),
        4.0
    );
    assert_eq!(
        quad_height(Diagonal::NorthwestSoutheast, corners, 0.5, 0.5),
        0.0
    );
    assert_eq!(
        quad_height(Diagonal::NorthwestSoutheast, corners, 0.75, 0.75),
        4.0
    );
}

#[test]
fn diagonal_level_follows_the_registry() {
    use Status::*;
    let all = |s: Status| move |_: &str| Some(s);
    assert_eq!(default_diagonal(RuleLevel::Facts, all(Supported)), None);
    assert_eq!(default_diagonal(RuleLevel::Measured, all(Pending)), None);
    assert_eq!(
        default_diagonal(RuleLevel::Hypotheses, all(Pending)),
        Some(Diagonal::SouthwestNortheast)
    );
    let second = |n: &str| {
        Some(if n == hypotheses::LAND_DIAGONAL_NW_SE {
            Supported
        } else {
            Refuted
        })
    };
    assert_eq!(
        default_diagonal(RuleLevel::Measured, second),
        Some(Diagonal::NorthwestSoutheast)
    );
    assert_eq!(default_diagonal(RuleLevel::Hypotheses, all(Refuted)), None);
}

#[test]
fn terrain_is_placed_from_the_grid_only_when_its_rules_are_accepted() {
    let f = fixture();
    let b = Budget::new(64 << 20);
    let store = Store::open(&f.manifest, &b).unwrap();
    assert!(store.diagnostics.is_empty(), "{:?}", store.diagnostics);
    assert_eq!(store.world(&key(0x810)), Some(key(0x800)));
    assert_eq!(store.world(&key(0x860)), None);
    let at = |level, diagonal| Options { level, diagonal };
    assert_eq!(
        refusal(load(&store, &key(0x810), at(RuleLevel::Facts, None), &b).unwrap()),
        "RulesNotAccepted"
    );
    for level in [RuleLevel::Facts, RuleLevel::Measured] {
        let o = load(
            &store,
            &key(0x810),
            at(level, Some(Diagonal::SouthwestNortheast)),
            &b,
        );
        assert_eq!(refusal(o.unwrap()), "RulesNotAccepted");
    }
    let Outcome::Built(t) = load(&store, &key(0x810), at(RuleLevel::Hypotheses, None), &b).unwrap()
    else {
        panic!("terrain expected")
    };
    assert_eq!(t.land, key(0x811));
    assert_eq!(t.world, Some(key(0x800)));
    assert_eq!((t.grid, t.grid_flags), ([3, -2], 5));
    assert_eq!(t.diagonal, Diagonal::SouthwestNortheast);
    assert!(
        matches!(&t.state, State::ReadyUnderHypotheses { hypotheses } if hypotheses.len() == 6)
    );
    assert_eq!(t.origin(), [12288.0, -8192.0]);
    let z = |r: usize, c: usize| 8.0 * (2.0 + (r + 1) as f64 + c as f64);
    assert_eq!(t.vertex(0, 0), [12288.0, -8192.0, z(0, 0)]);
    assert_eq!(
        t.vertex(32, 1),
        [12288.0 + 128.0, -8192.0 + 4096.0, z(32, 1)]
    );
    // A plane: every diagonal and every point agrees.
    assert_eq!(
        t.height_at(12288.0 + 64.0, -8192.0 + 192.0),
        Some(z(0, 0) + 4.0 + 12.0)
    );
    assert_eq!(
        t.height_at(12288.0 + 4096.0, -8192.0 + 4096.0),
        Some(z(32, 32))
    );
    assert_eq!(t.height_at(12287.0, -8192.0), None);
    let triangles: Vec<_> = t.world_triangles().collect();
    assert_eq!(triangles.len(), 2 * QUADS * QUADS);
    for tri in &triangles {
        assert!(crate::collision::triangle_normal(*tri)[2] > 0.0);
    }
    let bx = t.bounds();
    assert_eq!((bx.min[2], bx.max[2]), (z(0, 0), z(32, 32)));
    for (cell, code) in [
        (0x820, "NoLand"),
        (0x830, "MultipleLand"),
        (0x840, "NoLand"),
        (0x850, "NoHeights"),
        (0x860, "NotExterior"),
    ] {
        let o = load(&store, &key(cell), at(RuleLevel::Hypotheses, None), &b).unwrap();
        assert_eq!(refusal(o), code, "{cell:x}");
    }
    let lands = land_children(&store, &key(0x840), &b).unwrap();
    assert_eq!((lands.candidates.len(), lands.deleted.len()), (0, 1));
}

#[test]
fn a_refuted_or_unregistered_rule_blocks_the_terrain() {
    let f = fixture();
    let b = Budget::new(64 << 20);
    let store = Store::open(&f.manifest, &b).unwrap();
    let options = Options {
        level: RuleLevel::Hypotheses,
        diagonal: Some(Diagonal::NorthwestSoutheast),
    };
    let refuted = |n: &str| {
        Some(if n == hypotheses::LAND_UNITS {
            Status::Refuted
        } else {
            Status::Pending
        })
    };
    let o = load_with(&store, &key(0x810), options, &refuted, &b).unwrap();
    assert_eq!(refusal(o), "RulesNotAccepted");
    let unknown = |_: &str| None;
    let o = load_with(&store, &key(0x810), options, &unknown, &b).unwrap();
    assert_eq!(refusal(o), "RulesNotAccepted");
    // Two supported diagonal levels leave no choice.
    let both = |_: &str| Some(Status::Supported);
    let o = load_with(
        &store,
        &key(0x810),
        Options {
            level: RuleLevel::Measured,
            diagonal: None,
        },
        &both,
        &b,
    );
    assert_eq!(refusal(o.unwrap()), "NoDiagonalLevel");
    let measured = |n: &str| {
        Some(if n == hypotheses::LAND_DIAGONAL_NW_SE {
            Status::Refuted
        } else {
            Status::Supported
        })
    };
    let Outcome::Built(t) = load_with(
        &store,
        &key(0x810),
        Options {
            level: RuleLevel::Measured,
            diagonal: None,
        },
        &measured,
        &b,
    )
    .unwrap() else {
        panic!("measured terrain expected")
    };
    assert!(matches!(t.state, State::ReadyMeasured { .. }));
    assert_eq!(t.diagonal, Diagonal::SouthwestNortheast);
}

#[test]
fn ground_textures_follow_the_registry_and_name_their_colour_texture() {
    let f = fixture();
    let b = Budget::new(64 << 20);
    let store = Store::open(&f.manifest, &b).unwrap();
    assert!(store.diagnostics.is_empty(), "{:?}", store.diagnostics);
    let options = Options {
        level: RuleLevel::Hypotheses,
        diagonal: None,
    };
    let Outcome::Built(t) = load(&store, &key(0x810), options, &b).unwrap() else {
        panic!("terrain expected")
    };
    let pending = |_: &str| Some(Status::Pending);
    let built = |level, status: &dyn Fn(&str) -> Option<Status>| {
        layers::load_with(&store, &t, level, status, &b).unwrap()
    };
    let layers::Outcome::Built(l) = built(RuleLevel::Hypotheses, &pending) else {
        panic!("texture weights expected")
    };
    assert!(
        matches!(&l.state, State::ReadyUnderHypotheses { hypotheses } if hypotheses.len() == 7)
    );
    use layers::Source;
    assert_eq!(l.quadrants[0].base, Some(Source::Ltex(key(0x870))));
    assert_eq!(l.quadrants[1].base, Some(Source::Default));
    let q1 = &l.quadrants[1].textures;
    assert_eq!(
        (q1[1].source.clone(), q1[1].weights[3], q1[0].weights[3]),
        (Source::Ltex(key(0x873)), 0.5, 0.5)
    );
    let code = |o: layers::Outcome| match o {
        layers::Outcome::Refused(r) => r.code,
        layers::Outcome::Built(_) => "built",
    };
    assert_eq!(
        code(built(RuleLevel::Measured, &pending)),
        "TextureRulesNotAccepted"
    );
    let refuted = |n: &str| {
        Some(if n == hypotheses::LAND_LAYER_WEIGHTS {
            Status::Refuted
        } else {
            Status::Supported
        })
    };
    assert_eq!(
        code(built(RuleLevel::Hypotheses, &refuted)),
        "TextureRulesNotAccepted"
    );
    let measured = |_: &str| Some(Status::Supported);
    assert_eq!(code(built(RuleLevel::Measured, &measured)), "built");
    let path = |id| layers::colour_texture_path(&store, &key(id), &b).unwrap();
    assert_eq!(path(0x870), Ok("landscape\\ground_d.dds".to_owned()));
    for (id, refusal) in [
        (0x871, "LandscapeTextureTnamNotTextureSet"),
        (0x872, "LandscapeTextureWithoutSingleTnam"),
        (0x873, "TextureSetWithoutTx00"),
        (0x880, "NotALandscapeTexture"),
        (0x8ff, "MissingLandscapeTexture"),
    ] {
        assert_eq!(path(id), Err(refusal), "{id:x}");
    }
}
