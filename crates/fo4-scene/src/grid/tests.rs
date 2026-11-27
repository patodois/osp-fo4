//! Synthetic plugins only: a world of exterior cells, each with placed
//! references, and an interior cell.
use super::*;
use crate::RuleLevel;
use fo4_core::{Game, Limits, Manifest, PluginSpec, TestDir};
use fo4_plugin::typed::Store;

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
fn reference(id: u32) -> Vec<u8> {
    let mut r = sub(b"NAME", &0x801u32.to_le_bytes());
    let mut tr = Vec::new();
    for f in [1.0f32, 2., 3., 0., 0., 0.] {
        tr.extend(f.to_le_bytes());
    }
    r.extend(sub(b"DATA", &tr));
    record(b"REFR", id, 0, &r)
}
/// An exterior cell at grid (x, y) with `refs` placed references.
fn cell(id: u32, x: i32, y: i32, refs: u32) -> Vec<u8> {
    let mut c = sub(b"EDID", format!("Ext{id:x}\0").as_bytes());
    let mut xclc = x.to_le_bytes().to_vec();
    xclc.extend(y.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    c.extend(sub(b"XCLC", &xclc));
    let mut out = record(b"CELL", id, 0, &c);
    if refs > 0 {
        let children: Vec<u8> = (1..=refs).flat_map(|k| reference(id + k)).collect();
        out.extend(group(6, id, &group(9, id, &children)));
    }
    out
}
struct Fixture {
    _temp: TestDir,
    manifest: Manifest,
}
/// Centre (0, 0) with two references; east (1, 0) with one; north (0, 1)
/// with three; west (-1, 0) claimed by two cells; (2, 2) with one; a far
/// cell at (5, 5); and an interior cell.
fn fixture() -> Fixture {
    let cells = [
        cell(0x810, 0, 0, 2),
        cell(0x820, 1, 0, 1),
        cell(0x830, 0, 1, 3),
        cell(0x840, -1, 0, 1),
        cell(0x850, -1, 0, 1),
        cell(0x860, 2, 2, 1),
        cell(0x870, 5, 5, 1),
    ]
    .concat();
    let world = [
        record(b"WRLD", 0x800, 0, &sub(b"EDID", b"World\0")),
        group(1, 0x800, &group(4, 0, &group(5, 0, &cells))),
    ]
    .concat();
    let mut interior = sub(b"EDID", b"Room\0");
    interior.extend(sub(b"DATA", &1u16.to_le_bytes()));
    let mut body = group(
        0,
        u32::from_le_bytes(*b"STAT"),
        &record(b"STAT", 0x801, 0, &[]),
    );
    body.extend(group(0, u32::from_le_bytes(*b"WRLD"), &world));
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
    let temp = TestDir::new("scene-grid");
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
    Fixture {
        _temp: temp,
        manifest,
    }
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
fn offsets_put_the_first_ring_first_and_cover_the_square() {
    assert!(offsets(0).is_empty());
    assert_eq!(offsets(1), RING_1.to_vec());
    for r in 1..=MAX_RADIUS {
        let o = offsets(r);
        let side = 2 * r as usize + 1;
        assert_eq!(o.len(), side * side - 1);
        let mut sorted = o.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), o.len());
        assert!(o.iter().all(|p| p[0].abs().max(p[1].abs()) <= r as i32));
        assert!(!o.contains(&[0, 0]));
    }
    // Rings grow outwards; within a ring, south to north, west to east.
    let o = offsets(2);
    assert_eq!(&o[8..11], &[[-2, -2], [-1, -2], [0, -2]]);
    assert_eq!(o[o.len() - 1], [2, 2]);
}

#[test]
fn neighbours_are_found_by_grid_in_the_same_world() {
    let f = fixture();
    let b = budget();
    let store = Store::open(&f.manifest, &b).unwrap();
    let a = around(&store, &key(0x810), 1, &b).unwrap();
    assert!(a.searched);
    assert_eq!(a.grid, Some([0, 0]));
    assert_eq!(a.cells_scanned, 6);
    assert_eq!(a.same_grid_cells, 0);
    assert_eq!(a.neighbours.len(), 8);
    let at = |o: [i32; 2]| a.neighbours.iter().find(|n| n.offset == o).unwrap();
    assert_eq!(at([1, 0]).outcome, "cell");
    assert_eq!(at([1, 0]).cell, Some(key(0x820)));
    assert_eq!(at([0, 1]).cell, Some(key(0x830)));
    assert_eq!(at([-1, 0]).outcome, "ambiguous_grid");
    assert_eq!(at([-1, 0]).cell, None);
    assert_eq!(at([1, 1]).outcome, "no_cell");
    // Radius 2 reaches (2, 2), never (5, 5).
    let a2 = around(&store, &key(0x810), 2, &b).unwrap();
    assert_eq!(a2.neighbours.len(), 24);
    let found: Vec<_> = a2
        .neighbours
        .iter()
        .filter_map(|n| n.cell.clone())
        .collect();
    assert_eq!(found, vec![key(0x820), key(0x830), key(0x860)]);
    // Radius 0, an interior cell and a radius above the limit.
    let a0 = around(&store, &key(0x810), 0, &b).unwrap();
    assert!(!a0.searched && a0.neighbours.is_empty() && a0.cells_scanned == 0);
    let room = around(&store, &key(0x8a0), 1, &b).unwrap();
    assert!(!room.searched && room.grid.is_none() && room.world.is_none());
    let e = around(&store, &key(0x810), MAX_RADIUS + 1, &b).unwrap_err();
    assert_eq!(e.code, "InvalidGridRadius");
}

#[test]
fn the_scene_takes_the_references_of_every_cell_around() {
    let f = fixture();
    let b = budget();
    let store = Store::open(&f.manifest, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&f.manifest, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Measured, &b);
    let mut scene = loader.cell(&key(0x810), &b).unwrap();
    assert_eq!(scene.instances.len(), 2);
    let a = around(&store, &key(0x810), 1, &b).unwrap();
    let loads = absorb_around(&mut loader, &mut scene, &a, &b).unwrap();
    assert_eq!(loads.len(), 9);
    assert_eq!((loads[0].offset, loads[0].instances), ([0, 0], 2));
    let total: usize = loads.iter().map(|l| l.instances).sum();
    assert_eq!(total, 6);
    assert_eq!(scene.instances.len(), 6);
    // The centre's record is kept; the ambiguous grid adds nothing.
    assert_eq!(scene.cell.key, key(0x810));
    let west = loads.iter().find(|l| l.offset == [-1, 0]).unwrap();
    assert_eq!((west.outcome, west.instances), ("ambiguous_grid", 0));
    let mut refs: Vec<u32> = scene
        .instances
        .iter()
        .map(|i| i.reference.key.object_id)
        .collect();
    refs.sort_unstable();
    assert_eq!(refs, vec![0x811, 0x812, 0x821, 0x831, 0x832, 0x833]);
    // Radius 0: the scene is unchanged.
    let mut alone = loader.cell(&key(0x810), &b).unwrap();
    let a0 = around(&store, &key(0x810), 0, &b).unwrap();
    let loads = absorb_around(&mut loader, &mut alone, &a0, &b).unwrap();
    assert_eq!(loads.len(), 1);
    assert_eq!(alone.instances.len(), 2);
}

/// A placed reference at `pos`.
fn reference_at(id: u32, pos: [f32; 3]) -> Vec<u8> {
    let mut r = sub(b"NAME", &0x801u32.to_le_bytes());
    let mut tr = Vec::new();
    for f in [pos[0], pos[1], pos[2], 0., 0., 0.] {
        tr.extend(f.to_le_bytes());
    }
    r.extend(sub(b"DATA", &tr));
    record(b"REFR", id, 0, &r)
}

/// The world's persistent cell holds two persistent references: one in
/// the centre's square (0, 0), one far away in square (12, 12).
fn persistent_fixture() -> (TestDir, fo4_core::Manifest) {
    let mut persistent = record(b"CELL", 0x8b0, 0, &sub(b"EDID", b"Persistent\0"));
    let kids = [
        reference_at(0x8b1, [100.0, 200.0, 0.0]),
        reference_at(0x8b2, [50_000.0, 50_000.0, 0.0]),
    ]
    .concat();
    persistent.extend(group(6, 0x8b0, &group(8, 0x8b0, &kids)));
    let cells = [cell(0x810, 0, 0, 2), cell(0x820, 1, 0, 1)].concat();
    let mut children = persistent;
    children.extend(group(4, 0, &group(5, 0, &cells)));
    let world = [
        record(b"WRLD", 0x800, 0, &sub(b"EDID", b"World\0")),
        group(1, 0x800, &children),
    ]
    .concat();
    let mut body = group(
        0,
        u32::from_le_bytes(*b"STAT"),
        &record(b"STAT", 0x801, 0, &[]),
    );
    body.extend(group(0, u32::from_le_bytes(*b"WRLD"), &world));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, 1, &sub(b"HEDR", &hedr));
    plugin.extend(body);
    let temp = TestDir::new("scene-grid-persistent");
    let root = temp.join("Data");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("base.esm"), plugin).unwrap();
    let mut manifest = fixture().manifest;
    manifest.root = root;
    (temp, manifest)
}

#[test]
fn the_persistent_references_of_the_loaded_squares_join_the_scene_once() {
    let (_temp, manifest) = persistent_fixture();
    let b = budget();
    let store = Store::open(&manifest, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&manifest, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Measured, &b);
    let mut scene = loader.cell(&key(0x810), &b).unwrap();
    let a = around(&store, &key(0x810), 1, &b).unwrap();
    absorb_around(&mut loader, &mut scene, &a, &b).unwrap();
    assert_eq!(scene.instances.len(), 3);
    let (active, disabled) = persistent_in(&store, &key(0x800), &[[0, 0]], &b).unwrap();
    assert_eq!(
        active.iter().map(|k| k.key.object_id).collect::<Vec<_>>(),
        [0x8b1]
    );
    assert!(disabled.is_empty());
    let j = absorb_persistent(&mut loader, &mut scene, &a, &b).unwrap();
    assert_eq!((j.added, j.disabled, j.already, j.failed), (1, 0, 0, 0));
    assert_eq!(scene.instances.len(), 4);
    assert!(scene
        .instances
        .iter()
        .any(|i| i.reference.key == key(0x8b1)));
    assert!(!scene
        .instances
        .iter()
        .any(|i| i.reference.key == key(0x8b2)));
    // Joined again: already there, nothing added.
    let j = absorb_persistent(&mut loader, &mut scene, &a, &b).unwrap();
    assert_eq!((j.added, j.already), (0, 1));
    assert_eq!(scene.instances.len(), 4);
}

/// code-4b F03: a persistent reference whose instance runs out of budget
/// is the budget's error, not a reference counted as failed.
#[test]
fn a_persistent_reference_short_of_budget_is_an_error_not_a_failed_reference() {
    let (_temp, manifest) = persistent_fixture();
    let b = budget();
    let store = Store::open(&manifest, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&manifest, &b).unwrap();
    let mut refused = 0;
    let mut scope = 0u64;
    loop {
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Measured, &b);
        let mut scene = loader.cell(&key(0x810), &b).unwrap();
        let a = around(&store, &key(0x810), 1, &b).unwrap();
        absorb_around(&mut loader, &mut scene, &a, &b).unwrap();
        let root = Budget::new(256 << 20).with_scope_limit(scope);
        match absorb_persistent(&mut loader, &mut scene, &a, &root) {
            Ok(j) => {
                assert_eq!((j.added, j.failed), (1, 0), "scope {scope}");
                break;
            }
            Err(e) => {
                assert_eq!(e.code, "LimitExceeded");
                refused += 1;
            }
        }
        assert!(scope < 1 << 24, "no scope loads it");
        scope += 64;
    }
    assert!(refused > 0);
}

/// Review follow-up: of a persistent reference's instance errors only the
/// budget's own refusals end the join; a limit of the data read (or any
/// other error) is the reference's, counted as failed.
#[test]
fn only_the_budgets_own_refusal_ends_the_persistent_join() {
    let root = Budget::new(1 << 20).with_scope_limit(256);
    let scope = root.child();
    // The scope's own limit and the root's ceiling: the load's.
    let past_scope = scope.charge(512, "test", 0).unwrap_err();
    assert_eq!(past_scope.code, "LimitExceeded");
    assert!(reference_error(past_scope, &scope).is_err());
    let past_ceiling = Budget::new(64).charge(128, "test", 0).unwrap_err();
    assert!(reference_error(past_ceiling, &Budget::new(64)).is_err());
    // A limit of the data (a count above its stated bound): the reference's.
    let data = fo4_core::limit(300, 299, "record", 24).unwrap_err();
    assert_eq!(data.code, "LimitExceeded");
    assert!(reference_error(data, &scope).is_ok());
    let other = Error::new("Truncated", "record", 24, "range end <= 8", 12);
    assert!(reference_error(other, &scope).is_ok());
}

/// Loads the centre and its ring with every account under `root`; returns
/// the root's credits after absorbing, or the error.
fn absorb_with(f: &Fixture, root: &Budget) -> fo4_core::Result<u64> {
    let open = budget();
    let store = Store::open(&f.manifest, &open).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&f.manifest, &open).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Measured, root);
    let mut scene = loader.cell(&key(0x810), root)?;
    let a = around(&store, &key(0x810), 2, root)?;
    absorb_around(&mut loader, &mut scene, &a, root)?;
    assert_eq!(scene.instances.len(), 7);
    Ok(root.used())
}

#[test]
fn each_cell_around_is_bounded_by_the_scope_limit_and_kept_by_the_root() {
    let f = fixture();
    // Without a scope limit: what the scene keeps.
    let kept = absorb_with(&f, &budget()).unwrap();
    // The smallest scope limit that loads every cell.
    let (mut lo, mut hi) = (0u64, kept);
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        let root = Budget::new(256 << 20).with_scope_limit(mid);
        match absorb_with(&f, &root) {
            Ok(_) => hi = mid,
            Err(e) => {
                assert_eq!(e.code, "LimitExceeded");
                lo = mid;
            }
        }
    }
    // One cell's work fits a scope well below the sum the scene keeps, and
    // the scene keeps the same credits as without the limit.
    assert!(hi < kept / 2, "scope {hi} of {kept}");
    let root = Budget::new(256 << 20).with_scope_limit(hi);
    assert_eq!(absorb_with(&f, &root).unwrap(), kept);
    let short = Budget::new(256 << 20).with_scope_limit(hi - 1);
    assert_eq!(absorb_with(&f, &short).unwrap_err().code, "LimitExceeded");
    // The root still bounds the sum, and says so.
    let small = Budget::new(kept - 1).with_scope_limit(hi).named("scene");
    let e = absorb_with(&f, &small).unwrap_err();
    assert!(e.expected.contains("(scene)"), "{e}");
}

#[test]
fn the_world_grid_agrees_with_the_search_around() {
    let f = fixture();
    let b = budget();
    let store = Store::open(&f.manifest, &b).unwrap();
    let g = world_grid(&store, &key(0x810), &b).unwrap();
    assert!(g.searched);
    assert_eq!(g.grid, Some([0, 0]));
    assert_eq!(g.cells_scanned, 6);
    assert_eq!(g.at([0, 0]), (Some(&key(0x810)), "cell"));
    assert_eq!(g.at([5, 5]), (Some(&key(0x870)), "cell"));
    assert_eq!(g.at([-1, 0]), (None, "ambiguous_grid"));
    assert_eq!(g.at([9, 9]), (None, "no_cell"));
    let a = around(&store, &key(0x810), MAX_RADIUS, &b).unwrap();
    for n in &a.neighbours {
        let (cell, outcome) = g.at(n.grid.unwrap());
        assert_eq!(
            (cell.cloned(), outcome),
            (n.cell.clone(), n.outcome),
            "{n:?}"
        );
    }
    let room = world_grid(&store, &key(0x8a0), &b).unwrap();
    assert!(!room.searched && room.cells.is_empty());
}
