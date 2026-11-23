//! The walking world of a cell for the play mode: the placed collision of
//! the cell and the cells about it with their terrain (what `play_run` has
//! walked on since the live window step), and, for the live world
//! ([`crate::live`]), the collision of the references the run may enable
//! (the cell's initially disabled references and the instances an enable
//! parent disables at load) kept apart, each reference's enable chain and
//! stored position, and the load doors among the loaded references.
use crate::live::{Piece, RefInfo};
use crate::pick;
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::timeline::norm;
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::World;
use std::collections::BTreeMap;

type E = Box<dyn std::error::Error>;

/// The grid square of an exterior cell, world units.
pub const CELL_SIZE: f64 = 4096.0;

/// What [`build`] reads besides the walking world itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Exterior cells about the cell (grid steps), with their terrain.
    pub around: u32,
    /// Read what the live world needs ([`Built::live`]).
    pub live: bool,
    /// Read the load doors among the loaded references ([`Built::doors`]).
    pub doors: bool,
}

/// A placed door whose `XTEL` names a door: where it takes the player
/// (`world_load_door_xtel_v1`'s reading of the subrecord).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadDoor {
    /// The door the `XTEL` names.
    pub to: FormKey,
    /// The space of that door: its interior cell, or the worldspace of its
    /// exterior cell.
    pub space: FormKey,
    /// The teleport position and rotation the `XTEL` stores.
    pub pos: [f64; 3],
    pub rot: [f64; 3],
}

/// The walking world of a cell and what goes with it.
pub struct Built {
    /// The cell the world was built about and its space (the worldspace of
    /// an exterior cell, else the cell).
    pub cell: FormKey,
    pub space: FormKey,
    /// The walking world, exactly as `play_run` built it before the live
    /// world: the cell's placed collision, then the terrain.
    pub world: World,
    /// The placed reference of each source of `world`.
    pub refs: Vec<Option<FormKey>>,
    /// The object bounds of the references with no collision in `world`.
    pub boxes: pick::Boxes,
    /// A report of what was built.
    pub report: String,
    /// With [`Options::live`]: the collision of the references the run may
    /// enable (one piece) and every reference's chain and stored place.
    pub live: Option<(Piece, BTreeMap<FormKey, RefInfo>)>,
    /// With [`Options::doors`]: the load doors among the loaded references
    /// (instances and initially disabled ones), by reference.
    pub doors: BTreeMap<FormKey, LoadDoor>,
    /// Counts of what was read, by text.
    pub counts: BTreeMap<String, u64>,
}

/// The cell under a point: the interior cell itself, or the exterior cell
/// of the worldspace whose grid square holds it.
pub fn start_cell(store: &Store, space: &FormKey, pos: [f64; 3], b: &Budget) -> Result<FormKey, E> {
    match store.signature(space) {
        Some(s) if s == *b"CELL" => Ok(space.clone()),
        Some(s) if s == *b"WRLD" => {
            let g = square(pos);
            for c in store.cells() {
                if store.world(&c).as_ref() != Some(space) {
                    continue;
                }
                if fo4_scene::grid::grid_of(store, &c, &b.child())? == Some(g) {
                    return Ok(c);
                }
            }
            Err(format!("PlayStartCell: no exterior cell at grid {g:?}").into())
        }
        _ => Err("PlayStartCell: the track's space names no winning CELL or WRLD".into()),
    }
}

/// The exterior grid square holding a position.
pub fn square(p: [f64; 3]) -> [i32; 2] {
    [
        (p[0] / CELL_SIZE).floor() as i32,
        (p[1] / CELL_SIZE).floor() as i32,
    ]
}

/// The space a placed reference stands in: its interior cell, or the
/// worldspace of its exterior cell.
pub fn space_of(store: &Store, k: &FormKey) -> Option<FormKey> {
    let w = store.overrides().winner(k)?;
    let cell = store
        .placement_cells()
        .find(|(r, _)| std::ptr::eq(*r, w))
        .map(|(_, c)| c)?;
    match store.world(&cell) {
        Some(world) => Some(world),
        None => Some(cell),
    }
}

fn bump(counts: &mut BTreeMap<String, u64>, k: &str) {
    *counts.entry(k.to_string()).or_default() += 1;
}

/// The load door a placed reference is: a door base and an `XTEL` whose
/// door resolves to a placed reference with a space; `Ok(None)` when it is
/// not one.
pub fn load_door(store: &Store, key: &FormKey, b: &Budget) -> Result<Option<LoadDoor>, String> {
    let Some((r, plugin)) = store.raw_with_plugin(key, b).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let Some(tp) = fo4_vm::track::teleport_of(store, &r, plugin).map_err(String::from)? else {
        return Ok(None);
    };
    let base = fo4_vm::bind::base_of(store, key, b).ok().flatten();
    if base.and_then(|k| store.signature(&k)) != Some(*b"DOOR") {
        return Err("an XTEL on a reference whose base is not a door".into());
    }
    let space = space_of(store, &tp.door).ok_or("the XTEL's door is not placed in a cell")?;
    Ok(Some(LoadDoor {
        to: tp.door,
        space: norm(&space),
        pos: tp.pos,
        rot: tp.rot,
    }))
}

/// The load doors among a worldspace's persistent references (those of its
/// cells' persistent groups) standing within `radius` grid squares of the
/// square `centre`: (reference, stored position, the door). A worldspace's
/// load doors are kept there, not among its grid cells' references.
pub fn persistent_load_doors(
    store: &Store,
    space: &FormKey,
    centre: [i32; 2],
    radius: i32,
    counts: &mut BTreeMap<String, u64>,
    b: &Budget,
) -> Result<Vec<(FormKey, [f64; 3], LoadDoor)>, E> {
    let mut out = Vec::new();
    for c in store.cells() {
        if store.world(&c).map(|w| norm(&w)) != Some(norm(space)) {
            continue;
        }
        let cb = b.child();
        let ch = store.children(&c, &cb)?;
        for kid in ch.active.iter().chain(&ch.disabled) {
            if kid.group_kind != 8 || kid.signature != *b"REFR" {
                continue;
            }
            let rb = b.child();
            let Some(r) = store.get::<fo4_plugin::typed::Refr>(&kid.key, &rb)? else {
                continue;
            };
            let Some(t) = r.data.transform else { continue };
            let at = [t[0], t[1], t[2]].map(f64::from);
            let sq = square(at);
            if (sq[0] - centre[0]).abs() > radius || (sq[1] - centre[1]).abs() > radius {
                continue;
            }
            match load_door(store, &kid.key, &rb) {
                Ok(Some(d)) => out.push((kid.key.clone(), at, d)),
                Ok(None) => {}
                Err(why) => bump(counts, &format!("load doors left out\t{why}")),
            }
        }
    }
    Ok(out)
}

/// The walking world of `cell` and the cells about it.
pub fn build(
    m: &Manifest,
    store: &Store,
    vfs: &Vfs,
    cell: &FormKey,
    o: Options,
) -> Result<Built, E> {
    let level = RuleLevel::Hypotheses;
    let limits = &m.limits;
    let budget = Budget::new(8 << 30).with_scope_limit(limits.max_memory);
    let mut loader = Loader::new(store, vfs, &m.language, level, &budget);
    let mut scene = loader.cell(cell, &budget)?;
    let mut grid = None;
    if o.around > 0 {
        let a = fo4_scene::grid::around(store, &scene.cell.key, o.around, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        grid = Some(a);
    }
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, limits, options, &budget)?;
    let placement = c
        .body_placement
        .ok_or("PlayWalkWorld: no body placement level")?;
    let mut world = World::build(&c, placement, &budget)?;
    let refs_before = pick::references(&world, &c);
    let mut ground = match &grid {
        Some(a) => fo4_walk::terrain::load_with(store, a, level, &budget)?,
        None => fo4_walk::terrain::load_around(store, &scene.cell.key, level, true, &budget)?,
    };
    ground.add_to(&mut world, &budget)?;
    // Terrain sources come after the cell's: no reference.
    let mut refs = refs_before;
    refs.resize(world.sources.len(), None);
    // The object bounds of the placed references with no collision in the
    // world (used under --play-pick-bounds).
    let (mut boxes, no_bounds) = pick::object_bounds(&scene, &refs);
    let why: BTreeMap<String, usize> = c.unplaced.iter().fold(Default::default(), |mut m, u| {
        *m.entry(u.code.clone()).or_default() += 1;
        m
    });
    let report = format!(
        "walk world: {} triangles, {} hulls, {} spheres, {} capsules from {} instances ({} not placed); {} sources, {} with a placed reference",
        world.stats.triangles,
        world.stats.hulls,
        world.stats.spheres,
        world.stats.capsules,
        world.stats.instances,
        c.unplaced.len(),
        refs.len(),
        refs.iter().filter(|r| r.is_some()).count()
    ) + &format!(
        "\nreferences with no collision in the world and object bounds: {} ({} without usable bounds); collision not placed, by code: {why:?}; placed models with no collision object: {}",
        boxes.items.len(),
        no_bounds,
        c.without_collision.len()
    );
    let space = norm(&fo4_scene::timeline::scene_space(store, &scene));
    let mut counts = BTreeMap::new();
    let mut doors = BTreeMap::new();
    if o.doors {
        let keys: Vec<FormKey> = scene
            .instances
            .iter()
            .map(|i| i.reference.key.clone())
            .chain(scene.disabled.iter().map(|c| c.key.clone()))
            .collect();
        for k in keys {
            match load_door(store, &k, &budget.child()) {
                Ok(Some(d)) => {
                    bump(&mut counts, "load doors among the loaded references");
                    doors.insert(norm(&k), d);
                }
                Ok(None) => {}
                Err(why) => bump(&mut counts, &format!("load doors left out\t{why}")),
            }
        }
        // A worldspace keeps its load doors among its persistent
        // references: those near the walking world, with their object
        // bounds for the activation (they have no collision here).
        if let (Some(s), Some(g)) = (
            store.signature(&space),
            fo4_scene::grid::grid_of(store, cell, &budget)?,
        ) {
            if s == *b"WRLD" {
                let r = o.around as i32 + 1;
                for (k, _, d) in persistent_load_doors(store, &space, g, r, &mut counts, &budget)? {
                    let nk = norm(&k);
                    if doors.contains_key(&nk) {
                        continue;
                    }
                    bump(
                        &mut counts,
                        "load doors among the worldspace's persistent references nearby",
                    );
                    let scope = budget.child();
                    if let Ok(i) = loader.instance(&k, &scope) {
                        if let (Some(bounds), Some(t)) = (
                            i.base.as_ref().and_then(|b| b.common.bounds),
                            i.reference.data.transform,
                        ) {
                            if let Ok(f) = fo4_scene::transform::Transform::euler(
                                t,
                                i.reference.data.scale.unwrap_or(1.0),
                            ) {
                                if boxes.push(k.clone(), f, bounds) {
                                    boxes.kinds.resize(boxes.items.len() - 1, None);
                                    boxes.kinds.push(Some(*b"DOOR"));
                                }
                            }
                        }
                    }
                    doors.insert(nk, d);
                }
            }
        }
    }
    let live = if o.live {
        Some(dormant(
            &mut loader,
            &mut scene,
            limits,
            &refs,
            placement,
            &mut counts,
            &budget,
        )?)
    } else {
        None
    };
    Ok(Built {
        cell: cell.clone(),
        space,
        world,
        refs,
        boxes,
        report,
        live,
        doors,
        counts,
    })
}

/// The live world's pieces for a loaded scene whose walking world is
/// built (`refs`: the placed reference of each of its sources): the
/// collision of the cell's initially disabled references (loaded as any
/// placed reference) and of the instances an enable parent disables at
/// load (built as if enabled), in one world, and every reference's chain,
/// stored position and sources. The scene is left as it was.
pub fn dormant(
    loader: &mut Loader<'_>,
    scene: &mut fo4_scene::Scene,
    limits: &fo4_core::Limits,
    refs: &[Option<FormKey>],
    placement: fo4_scene::collision::BodyPlacement,
    counts: &mut BTreeMap<String, u64>,
    budget: &Budget,
) -> Result<(Piece, BTreeMap<FormKey, RefInfo>), E> {
    let stored = |i: &fo4_scene::Instance| {
        i.reference
            .data
            .transform
            .map(|t| [t[0], t[1], t[2]].map(f64::from))
    };
    let mut info: BTreeMap<FormKey, RefInfo> = BTreeMap::new();
    for i in &scene.instances {
        info.insert(
            norm(&i.reference.key),
            RefInfo {
                piece: 0,
                listed_disabled: false,
                enable: i.enable.clone(),
                stored: stored(i),
                sources: Vec::new(),
            },
        );
    }
    for (s, r) in refs.iter().enumerate() {
        if let Some(x) = r.as_ref().and_then(|k| info.get_mut(&norm(k))) {
            x.sources.push(s as u32);
        }
    }
    // The dormant scene: the initially disabled references, loaded, and
    // the instances an enable parent disables, enabled for the build.
    let mut dormant = Vec::new();
    for child in &scene.disabled {
        let scope = budget.child();
        match loader.instance(&child.key, &scope) {
            Ok(i) => {
                scope.commit(budget)?;
                bump(counts, "live world: initially disabled references loaded");
                dormant.push(i);
            }
            Err(e) => bump(
                counts,
                &format!(
                    "live world: initially disabled references not loaded\t{}",
                    e.code
                ),
            ),
        }
    }
    let loaded = dormant.len();
    let all = std::mem::take(&mut scene.instances);
    let mut slots: Vec<Option<fo4_scene::Instance>> = Vec::with_capacity(all.len());
    let mut moved: Vec<(usize, Option<bool>)> = Vec::new();
    for mut i in all {
        if i.disabled_by_enable_parent() {
            let was = i.enable.as_ref().and_then(|p| p.enabled);
            i.set_enabled(Some(true));
            moved.push((slots.len(), was));
            slots.push(None);
            dormant.push(i);
        } else {
            slots.push(Some(i));
        }
    }
    let options = fo4_scene::collision::Options {
        level: RuleLevel::Hypotheses,
        body_placement: Some(placement),
    };
    scene.instances = dormant;
    let built = fo4_scene::collision::cell(scene, loader, limits, options, budget);
    // Put the scene back as it was, whatever the build gave.
    let mut dormant = std::mem::take(&mut scene.instances);
    let mut back: Vec<fo4_scene::Instance> = Vec::new();
    for (k, (_, was)) in moved.iter().enumerate().rev() {
        let mut i = dormant.remove(loaded + k);
        i.set_enabled(*was);
        back.push(i);
    }
    back.reverse();
    let mut back = back.into_iter();
    scene.instances = slots
        .into_iter()
        .map(|s| s.unwrap_or_else(|| back.next().expect("a moved instance")))
        .collect();
    let dc = built?;
    let world = World::build(&dc, placement, budget)?;
    let drefs = pick::references(&world, &dc);
    for i in &dormant {
        info.insert(
            norm(&i.reference.key),
            RefInfo {
                piece: 1,
                listed_disabled: true,
                enable: i.enable.clone(),
                stored: stored(i),
                sources: Vec::new(),
            },
        );
    }
    for (k, _) in &moved {
        if let Some(i) = scene.instances.get(*k) {
            if let Some(x) = info.get_mut(&norm(&i.reference.key)) {
                x.piece = 1;
            }
        }
    }
    for (s, r) in drefs.iter().enumerate() {
        if let Some(x) = r.as_ref().and_then(|k| info.get_mut(&norm(k))) {
            if x.piece == 1 {
                x.sources.push(s as u32);
            }
        }
    }
    *counts
        .entry("live world: dormant collision sources".into())
        .or_default() += world.sources.len() as u64;
    *counts
        .entry("live world: instances an enable parent disables at load, built as enabled".into())
        .or_default() += moved.len() as u64;
    Ok((Piece { world, refs: drefs }, info))
}
