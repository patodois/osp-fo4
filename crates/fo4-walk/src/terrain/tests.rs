//! Synthetic plugins only: a world of exterior cells with LAND records.
use super::*;
use crate::{Character, Input, Params, Settings};
use fo4_core::{Game, Limits, Manifest, PluginSpec, TestDir};
use fo4_plugin::land::{POINTS, VHGT_SIZE};

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
/// A flat LAND at offset 2: every vertex at 2 x 8 = 16 world units under
/// the candidate rules. Without heights, DATA only.
fn land(id: u32, heights: bool) -> Vec<u8> {
    let mut p = sub(b"DATA", &[0; 4]);
    if heights {
        let mut v = 2.0f32.to_le_bytes().to_vec();
        v.extend(std::iter::repeat_n(0u8, POINTS));
        v.extend([0; 3]);
        assert_eq!(v.len(), VHGT_SIZE);
        p.extend(sub(b"VHGT", &v));
    }
    record(b"LAND", id, 0, &p)
}
fn cell(id: u32, x: i32, y: i32, lands: &[Vec<u8>]) -> Vec<u8> {
    let mut c = sub(b"EDID", format!("Ext{id:x}\0").as_bytes());
    let mut xclc = x.to_le_bytes().to_vec();
    xclc.extend(y.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    c.extend(sub(b"XCLC", &xclc));
    let mut out = record(b"CELL", id, 0, &c);
    if !lands.is_empty() {
        out.extend(group(6, id, &group(9, id, &lands.concat())));
    }
    out
}
struct Fixture {
    _temp: TestDir,
    store: Store,
}
/// Self at (0, 0) and east at (1, 0) with flat LAND; north without LAND;
/// south with a LAND without heights; west claimed by two cells; a far
/// cell at (5, 5); and an interior cell.
fn fixture() -> Fixture {
    let cells = [
        cell(0x810, 0, 0, &[land(0x811, true)]),
        cell(0x820, 1, 0, &[land(0x821, true)]),
        cell(0x830, 0, 1, &[]),
        cell(0x840, 0, -1, &[land(0x841, false)]),
        cell(0x850, -1, 0, &[land(0x851, true)]),
        cell(0x860, -1, 0, &[land(0x861, true)]),
        cell(0x870, 5, 5, &[land(0x871, true)]),
    ]
    .concat();
    let world = [
        record(b"WRLD", 0x800, 0, &sub(b"EDID", b"World\0")),
        group(1, 0x800, &group(4, 0, &group(5, 0, &cells))),
    ]
    .concat();
    let mut interior = sub(b"EDID", b"Room\0");
    interior.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut body = group(0, u32::from_le_bytes(*b"WRLD"), &world);
    body.extend(group(
        0,
        u32::from_le_bytes(*b"CELL"),
        &group(2, 0, &group(3, 0, &record(b"CELL", 0x8a0, 0, &interior))),
    ));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
    plugin.extend(body);
    let temp = TestDir::new("walk-terrain");
    let root = temp.join("Data");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let manifest = Manifest {
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
    };
    let store = Store::open(&manifest, &Budget::new(64 << 20)).unwrap();
    Fixture { _temp: temp, store }
}
fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    }
}
fn budget() -> Budget {
    Budget::new(256 << 20)
}

#[test]
fn the_cell_and_its_neighbours_are_loaded_with_their_outcomes() {
    let f = fixture();
    let b = budget();
    let g = load_around(&f.store, &key(0x810), RuleLevel::Hypotheses, true, &b).unwrap();
    assert!(g.neighbours_searched);
    assert!(g.own_built());
    // Six other exterior cells of the world were read; none shares the grid.
    assert_eq!(g.cells_scanned, 6);
    assert_eq!(g.same_grid_cells, 0);
    let outcome = |name: &str| {
        let p = g.pieces.iter().find(|p| p.name == name).unwrap();
        (p.grid, p.outcome.as_str())
    };
    assert_eq!(outcome("self"), (Some([0, 0]), "built:sw_ne"));
    assert_eq!(outcome("east"), (Some([1, 0]), "built:sw_ne"));
    assert_eq!(outcome("north"), (Some([0, 1]), "refused:NoLand"));
    assert_eq!(outcome("south"), (Some([0, -1]), "refused:NoHeights"));
    assert_eq!(outcome("west"), (Some([-1, 0]), "ambiguous_grid"));
    assert_eq!(outcome("north_east"), (Some([1, 1]), "no_cell"));
    assert_eq!(g.pieces.len(), 9);
    assert_eq!(g.terrains.len(), 2);
    assert_eq!(g.height_at(100.0, 100.0), Some((16.0, [0, 0])));
    assert_eq!(g.height_at(5000.0, 100.0), Some((16.0, [1, 0])));
    assert_eq!(g.height_at(-100.0, 100.0), None);

    // Without neighbours, only the cell.
    let g = load_around(&f.store, &key(0x810), RuleLevel::Hypotheses, false, &b).unwrap();
    assert!(!g.neighbours_searched);
    assert_eq!(g.pieces.len(), 1);
    // At the measured level the pending terrain rules refuse every cell, so
    // no neighbour is searched.
    let g = load_around(&f.store, &key(0x810), RuleLevel::Measured, true, &b).unwrap();
    assert_eq!(g.pieces.len(), 1);
    assert_eq!(g.pieces[0].outcome, "refused:RulesNotAccepted");
    assert!(!g.neighbours_searched && !g.own_built());
    // An interior cell has no terrain and no grid.
    let g = load_around(&f.store, &key(0x8a0), RuleLevel::Hypotheses, true, &b).unwrap();
    assert_eq!(g.pieces[0].outcome, "refused:NotExterior");
    assert_eq!(g.pieces[0].grid, None);
    // A cell whose own LAND is missing still gets its neighbours.
    let g = load_around(&f.store, &key(0x830), RuleLevel::Hypotheses, true, &b).unwrap();
    assert_eq!(g.pieces[0].outcome, "refused:NoLand");
    assert_eq!(g.pieces[0].grid, Some([0, 1]));
    assert!(g.neighbours_searched);
    assert_eq!(g.terrains.len(), 2); // south (0, 0) and south_east (1, 0)
}

#[test]
fn terrain_and_collision_carry_the_walker_together() {
    let f = fixture();
    let b = budget();
    let mut g = load_around(&f.store, &key(0x810), RuleLevel::Hypotheses, true, &b).unwrap();
    // A box standing on the ground, lower than the step height, in the way.
    let mut w = World::new();
    let corners: Vec<[f64; 3]> = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { 1000.0 } else { 1200.0 },
                if i & 2 == 0 { -100.0 } else { 300.0 },
                if i & 4 == 0 { 0.0 } else { 26.0 },
            ]
        })
        .collect();
    let block = w.add_hull(&corners, 0.0, "box", &b).unwrap();
    g.add_to(&mut w, &b).unwrap();
    // The terrain's rules are now the world's.
    for r in &g.terrains[0].rules {
        assert!(w.rules.contains(r), "{r}");
    }
    assert!(w.rules.contains(&crate::rules::TRIANGLE_SIDES));
    assert_eq!(w.stats.triangles, 2 * 2048);
    let own = g.pieces[0].source.unwrap();
    let east = g.pieces.iter().find(|p| p.name == "east").unwrap();
    let east = east.source.unwrap();
    assert_eq!(w.sources[own as usize].terrain, Some([0, 0]));
    assert_eq!(w.sources[east as usize].terrain, Some([1, 0]));
    assert_eq!(w.sources[own as usize].label.as_deref(), Some("terrain"));

    let mut c =
        Character::new(Params::PROVISIONAL, Settings::DEFAULT, [500.0, 64.0, 200.0]).unwrap();
    c.settle(&w, 600);
    let st = c.state();
    assert!(st.grounded);
    assert!((st.feet[2] - 16.0).abs() < 1e-9, "{:?}", st.feet);
    assert_eq!(st.ground.map(|x| x.source), Some(own));
    // Run east over the box and across the cell edge.
    let mut seen = Vec::new();
    for _ in 0..1500 {
        let st = c.tick(
            &w,
            Input {
                forward: 1.0,
                right: 0.0,
                heading: 0.0,
                run: true,
            },
        );
        assert!(st.grounded, "{st:?}");
        let source = st.ground.unwrap().source;
        if seen.last() != Some(&source) {
            seen.push(source);
        }
        if st.feet[0] > 4600.0 {
            break;
        }
    }
    assert_eq!(seen, vec![own, block, own, east]);
    let feet = c.feet;
    assert!(feet[0] > 4600.0);
    assert!((feet[2] - g.height_at(feet[0], feet[1]).unwrap().0).abs() < 1e-9);
}

#[test]
fn a_larger_radius_keeps_the_first_ring_and_adds_the_next() {
    let f = fixture();
    let b = budget();
    let one = load_around(&f.store, &key(0x810), RuleLevel::Hypotheses, true, &b).unwrap();
    let g = load_grid(&f.store, &key(0x810), RuleLevel::Hypotheses, 1, &b).unwrap();
    let names = |s: &TerrainSet| -> Vec<(&str, Option<[i32; 2]>, String)> {
        s.pieces
            .iter()
            .map(|p| (p.name, p.grid, p.outcome.clone()))
            .collect()
    };
    assert_eq!(names(&g), names(&one));
    // Radius 2: the first nine pieces as before, then 16 of the second
    // ring, none of which has a cell here; (5, 5) stays out of reach.
    let two = load_grid(&f.store, &key(0x810), RuleLevel::Hypotheses, 2, &b).unwrap();
    assert_eq!(two.pieces.len(), 25);
    assert_eq!(names(&two)[..9], names(&one)[..]);
    assert!(two.pieces[9..]
        .iter()
        .all(|p| p.name == "ring_2" && p.outcome == "no_cell"));
    assert_eq!(two.pieces[9].grid, Some([-2, -2]));
    assert_eq!(two.terrains.len(), 2);
    // From a search done once for the scene, the same pieces.
    let around = fo4_scene::grid::around(&f.store, &key(0x810), 2, &b).unwrap();
    let with = load_with(&f.store, &around, RuleLevel::Hypotheses, &b).unwrap();
    assert_eq!(names(&with), names(&two));
    assert_eq!(with.cells_scanned, two.cells_scanned);
    // A refusal that holds for every cell still stops the neighbours.
    let m = load_with(&f.store, &around, RuleLevel::Measured, &b).unwrap();
    assert_eq!(m.pieces.len(), 1);
    assert!(!m.neighbours_searched);
    assert_eq!(m.cells_scanned, 0);
    assert_eq!(piece_name([0, 1]), "north");
    assert_eq!(piece_name([3, -1]), "ring_3");
}
