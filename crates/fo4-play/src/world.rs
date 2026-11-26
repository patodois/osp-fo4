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
use std::collections::{BTreeMap, BTreeSet};

type E = Box<dyn std::error::Error>;

/// The grid square of an exterior cell, world units.
pub const CELL_SIZE: f64 = 4096.0;

/// The record flag of a box's base that makes the box solid in the walking
/// world under `play_collision_marker_boxes_v1`: set on the collision
/// marker's base, absent from the base of the light volumes that store the
/// same box primitive.
pub const BOX_OBSTACLE_FLAG: u32 = 0x0200_0000;

/// The collision layer words (a placed reference's `XTRI`) read under
/// `play_collision_marker_box_layer_passed_v1` as layers a walking
/// character passes through: 39 to 44 and 49 in the engine's layer list as
/// prior knowledge has it (camera pick, item pick, line of sight, path
/// pick, two custom picks; the navigation cutter).
pub const PASSED_LAYERS: [u32; 7] = [39, 40, 41, 42, 43, 44, 49];

/// The collision layer index of a walking character's body under
/// `play_collision_marker_box_layer_records_v1`: the character
/// controller's layer in the engine's layer list as prior knowledge has it.
pub const CHARACTER_LAYER: u32 = 30;

/// The layers a box's collision layer word is passed on: the constant
/// [`PASSED_LAYERS`], or the set the load order's layer records give
/// ([`passed_layers_from_records`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Passed {
    Constant,
    Records(BTreeSet<u32>),
}

impl Passed {
    /// Whether a box storing layer word `w` stays out.
    pub fn contains(&self, w: u32) -> bool {
        match self {
            Passed::Constant => PASSED_LAYERS.contains(&w),
            Passed::Records(set) => set.contains(&w),
        }
    }
}

/// The passed layers the load order's collision layer records give under
/// `play_collision_marker_box_layer_records_v1`: the index of every layer
/// record that the record of index [`CHARACTER_LAYER`] does not list as one
/// it collides with (`CNAM`; `fo4_plugin::coll`). `None` when the load
/// order holds no layer record or none of that index; a record that does
/// not read is an error.
pub fn passed_layers_from_records(store: &Store, b: &Budget) -> Result<Option<BTreeSet<u32>>, E> {
    let layers = fo4_plugin::coll::read_all(store, b)?;
    Ok(passed_from_relation(&fo4_plugin::coll::relation(&layers)?))
}

/// [`passed_layers_from_records`] from the layers' relation by index
/// (`fo4_plugin::coll::relation`).
pub fn passed_from_relation(relation: &BTreeMap<u32, BTreeSet<u32>>) -> Option<BTreeSet<u32>> {
    let listed = relation.get(&CHARACTER_LAYER)?;
    Some(
        relation
            .keys()
            .copied()
            .filter(|i| !listed.contains(i))
            .collect(),
    )
}

/// How a records' passed set differs from [`PASSED_LAYERS`]: the constant's
/// words the records do not pass (boxes that would stand), and the words
/// the records pass that the constant does not (boxes that would stay out).
pub fn passed_difference(records: &BTreeSet<u32>) -> (Vec<u32>, Vec<u32>) {
    let constant: BTreeSet<u32> = PASSED_LAYERS.into_iter().collect();
    (
        constant.difference(records).copied().collect(),
        records.difference(&constant).copied().collect(),
    )
}

/// Why a collision marker's box `mb`, whose base's record flags are
/// `base_flags`, stays out of the walking world under
/// `play_collision_marker_boxes_v1` (its base lacks the obstacle flag, or
/// it is placed initially disabled or under an enable parent) and, with
/// `layers` (`play_collision_marker_box_layer_passed_v1`,
/// `--play-marker-box-layers`), when its reference's `XTRI` word is one of
/// [`PASSED_LAYERS`]; `None`: it stands. A box storing no `XTRI` stands.
pub fn marker_box_left_out(
    base_flags: Option<u32>,
    mb: &fo4_walk::platform::MarkerBox,
    layers: bool,
) -> Option<&'static str> {
    marker_box_left_out_with(base_flags, mb, layers.then_some(&Passed::Constant))
}

/// [`marker_box_left_out`] with the passed layers given: `None` reads no
/// layer word, [`Passed::Records`] the set the layer records give
/// (`play_collision_marker_box_layer_records_v1`).
pub fn marker_box_left_out_with(
    base_flags: Option<u32>,
    mb: &fo4_walk::platform::MarkerBox,
    passed: Option<&Passed>,
) -> Option<&'static str> {
    if base_flags.is_none_or(|f| f & BOX_OBSTACLE_FLAG == 0) {
        return Some("their base does not carry the obstacle flag");
    }
    if let Some(p) = passed {
        if mb.layer.is_some_and(|w| p.contains(w)) {
            return Some(match p {
                Passed::Constant => {
                    "their collision layer word is one a walking character passes (39 to 44, 49)"
                }
                Passed::Records(_) => "their collision layer word is one the layer records do not list with the character's layer",
            });
        }
    }
    if mb.initially_disabled || mb.enable_parent {
        return Some("initially disabled or under an enable parent");
    }
    None
}

/// What [`build`] reads besides the walking world itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Exterior cells about the cell (grid steps), with their terrain.
    pub around: u32,
    /// Read what the live world needs ([`Built::live`]).
    pub live: bool,
    /// Read the load doors among the loaded references ([`Built::doors`]).
    pub doors: bool,
    /// The drawing side's collision readiness readings the cell build
    /// takes (`fo4_scene::readiness::Switches`, given to the loader with
    /// `Loader::set_collision_switches`; `--play-collision-off-chain`,
    /// `--play-collision-reference-alpha`, `--play-collision-static-fields`).
    /// All off: the loader's default.
    pub collision: fo4_scene::readiness::Switches,
    /// An enable parent chain left open only by records below its root
    /// carrying their own flag 0x800, each agreeing with the chain, decides
    /// the reference's collision as for any other chain
    /// (`xesp_child_flag_agrees_chain_v1`, given to the loader with
    /// `Loader::set_collision_child_flag_agrees`;
    /// `--play-collision-child-flag-agrees`). Off: the loader's default.
    pub child_flag_agrees: bool,
    /// The boxes of the collision markers among the loaded references and
    /// the worldspace's persistent references nearby stand in the walking
    /// world as those references' collision (`play_collision_marker_boxes_v1`
    /// over `walk_collision_marker_box_v1`; `--play-marker-boxes`). Off: no
    /// box, as before.
    pub marker_boxes: bool,
    /// With [`Options::marker_boxes`], a box whose reference's collision
    /// layer word (`XTRI`) is one of [`PASSED_LAYERS`] is left out
    /// (`play_collision_marker_box_layer_passed_v1`,
    /// `--play-marker-box-layers`). Off: every box of the obstacle base
    /// stands, as before.
    pub marker_box_layers: bool,
    /// With [`Options::marker_box_layers`], the passed layers are those the
    /// load order's collision layer records give
    /// ([`passed_layers_from_records`],
    /// `play_collision_marker_box_layer_records_v1`,
    /// `--play-marker-box-layer-records`), [`PASSED_LAYERS`] when they give
    /// none. Off: [`PASSED_LAYERS`], as before.
    pub marker_box_layer_records: bool,
    /// The boxes of the loaded doors' object bounds, kept in the live
    /// world's dormant piece and shown while the run's scripts hold the
    /// door closed with its activation blocked
    /// (`play_door_closed_blocked_holds_v1`, `--play-held-doors`; with
    /// [`Options::live`]). Off: none, as before.
    pub held_doors: bool,
    /// With [`Options::held_doors`], the held doors whose reference stores
    /// no `ONAM` are read as closed at load ([`Built::closed_at_load`];
    /// `play_door_closed_at_load_v1`, `--play-held-doors-at-load`). Off:
    /// none, as before.
    pub held_at_load: bool,
    /// A loaded door whose collision the readiness refuses stands in the
    /// walking world by its model's collision in its open pose: the parts
    /// no controller sequence moves (its frame) as stored, the parts a
    /// sequence moves where the model's sequence named Open leaves them;
    /// a door with no frame stays out (`play_door_collision_open_pose_v1`,
    /// `--play-door-open-collision`,
    /// [`crate::elevator::open_door_collision`]). Off: no door collision,
    /// as before.
    pub door_open: bool,
    /// With [`Options::held_doors`], a held door whose reference stores
    /// `ONAM` (published as open by default) and whose collision is not in
    /// the walking world is open for the held doors' reading from load
    /// ([`Built::stored_open`]; `play_door_stored_open_v1`,
    /// `--play-door-stored-open`). Off: such a door has no state at load,
    /// as before.
    pub stored_open: bool,
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
#[derive(Clone)]
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
    /// Sources of `world` the activation's ray passes through: the
    /// collision markers' boxes (`play_collision_marker_boxes_v1`), which
    /// stop the walker, not the eye.
    pub unpicked: Vec<u32>,
    /// With [`Options::held_doors`]: each loaded door's box sources in the
    /// dormant piece ([`held_door_corners`]), by door.
    pub held: BTreeMap<FormKey, Vec<u32>>,
    /// With [`Options::held_at_load`]: of `held`, the doors whose
    /// reference stores no `ONAM` (closed at load).
    pub closed_at_load: std::collections::BTreeSet<FormKey>,
    /// With [`Options::door_open`]: the doors whose collision stands in
    /// `world` in its open pose (normalised keys).
    pub open_pose: std::collections::BTreeSet<FormKey>,
    /// With [`Options::stored_open`]: of `held`, the doors whose reference
    /// stores `ONAM` (normalised keys), open for the held doors' reading
    /// from load (`play_door_stored_open_v1`).
    pub stored_open: std::collections::BTreeSet<FormKey>,
}

/// Whether the placed reference `key` stores an `ONAM` subrecord (the
/// field xEdit publishes as "Open by Default"; `spec/record-layouts.tsv`,
/// `REFR.ONAM`, no bytes): `None` when the record does not read.
pub fn stores_onam(store: &Store, key: &FormKey, b: &Budget) -> Option<bool> {
    let (r, _) = store.raw_with_plugin(key, b).ok()??;
    Some(r.subrecords.iter().any(|s| &s.signature == b"ONAM"))
}

/// The eight corners of a door's box under `play_door_closed_blocked_holds_v1`:
/// its base's object bounds (`lo`, `hi`, the model's units) placed by the
/// reference's transform.
pub fn held_door_corners(
    frame: fo4_scene::transform::Transform,
    lo: [f64; 3],
    hi: [f64; 3],
) -> [[f64; 3]; 8] {
    std::array::from_fn(|k| {
        let pick = |i: usize| if k & (1 << i) != 0 { hi[i] } else { lo[i] };
        frame.apply([pick(0), pick(1), pick(2)])
    })
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

/// The centre of a placed reference's object bounds (its base's OBND, six
/// i16: the low and high corner in model units) placed by its stored
/// transform and scale; its stored position when the base has no bounds.
/// `None` when the reference has no placement.
pub fn bounds_centre(store: &Store, k: &FormKey, b: &Budget) -> Option<[f64; 3]> {
    let r = store.get::<fo4_plugin::typed::Refr>(k, b).ok()??;
    let t = r.data.transform?;
    let scale = r.data.scale.unwrap_or(1.0);
    let at = [t[0], t[1], t[2]].map(f64::from);
    let base = r.data.base.as_ref().and_then(|f| f.key.clone());
    let local = base
        .and_then(|base| store.raw(&base, b).ok().flatten())
        .and_then(|rec| {
            let s = rec.subrecords.iter().find(|s| s.signature == *b"OBND")?;
            let d = rec.data(s);
            (d.len() == 12).then(|| {
                let v: Vec<f64> = (0..6)
                    .map(|i| f64::from(i16::from_le_bytes([d[2 * i], d[2 * i + 1]])))
                    .collect();
                [(v[0] + v[3]) / 2.0, (v[1] + v[4]) / 2.0, (v[2] + v[5]) / 2.0]
            })
        });
    match local {
        Some(c) => fo4_scene::transform::Transform::euler(t, scale)
            .ok()
            .map(|f| f.apply(c)),
        None => Some(at),
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

/// The worldspace's persistent references (those of its cells' persistent
/// groups) standing within `radius` grid squares of the square `centre`.
pub fn persistent_refs(
    store: &Store,
    space: &FormKey,
    centre: [i32; 2],
    radius: i32,
    b: &Budget,
) -> Result<Vec<FormKey>, E> {
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
            let sq = square([t[0], t[1], t[2]].map(f64::from));
            if (sq[0] - centre[0]).abs() > radius || (sq[1] - centre[1]).abs() > radius {
                continue;
            }
            out.push(norm(&kid.key));
        }
    }
    Ok(out)
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
    if o.collision.any() {
        loader.set_collision_switches(o.collision);
    }
    if o.child_flag_agrees {
        loader.set_collision_child_flag_agrees(true);
    }
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
    let mut counts = BTreeMap::new();
    // The collision markers' boxes come after the terrain, each with its
    // reference (`play_collision_marker_boxes_v1`).
    let space_key = norm(&fo4_scene::timeline::scene_space(store, &scene));
    let mut marked: Vec<(FormKey, [f64; 3], u32)> = Vec::new();
    if o.marker_boxes {
        let passed = match (o.marker_box_layers, o.marker_box_layer_records) {
            (false, _) => None,
            (true, false) => Some(Passed::Constant),
            (true, true) => Some(match passed_layers_from_records(store, &budget) {
                Ok(Some(set)) => {
                    let (stand, out) = passed_difference(&set);
                    let words =
                        |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
                    let line = if stand.is_empty() && out.is_empty() {
                        "the same as the constant".to_string()
                    } else {
                        format!(
                            "not the constant: passed by the constant, not by the records: {}; passed by the records, not by the constant: {}",
                            words(&stand),
                            words(&out)
                        )
                    };
                    bump(
                        &mut counts,
                        &format!("marker box layers from the layer records\t{line}"),
                    );
                    Passed::Records(set)
                }
                Ok(None) => {
                    bump(
                        &mut counts,
                        "marker box layers from the constant\tno layer record of the character's layer",
                    );
                    Passed::Constant
                }
                Err(e) => {
                    bump(
                        &mut counts,
                        &format!("marker box layers from the constant\tthe layer records do not read: {e}"),
                    );
                    Passed::Constant
                }
            }),
        };
        let mut keys: Vec<FormKey> = scene
            .instances
            .iter()
            .map(|i| norm(&i.reference.key))
            .collect();
        if let (Some(s), Some(g)) = (
            store.signature(&space_key),
            fo4_scene::grid::grid_of(store, cell, &budget)?,
        ) {
            if s == *b"WRLD" {
                let r = o.around as i32 + 1;
                keys.extend(persistent_refs(store, &space_key, g, r, &budget)?);
            }
        }
        keys.sort();
        keys.dedup();
        for k in keys {
            let mb = match fo4_walk::platform::marker_box(store, &k, &budget.child()) {
                Ok(Some(mb)) => mb,
                Ok(None) => continue,
                Err(_) => {
                    bump(
                        &mut counts,
                        "marker boxes left out\tthe reference does not read",
                    );
                    continue;
                }
            };
            // Only a box whose base carries the obstacle flag: the record
            // flag a collision marker's base sets and a light volume's
            // base does not (both are statics with no model).
            let base_flags = store
                .get::<fo4_plugin::typed::Refr>(&k, &budget.child())?
                .and_then(|r| r.data.base.and_then(|f| f.key))
                .and_then(|base| store.raw(&base, &budget.child()).ok().flatten())
                .map(|r| r.flags);
            if let Some(why) = marker_box_left_out_with(base_flags, &mb, passed.as_ref()) {
                bump(&mut counts, &format!("marker boxes left out\t{why}"));
                continue;
            }
            let at = [mb.placement[0], mb.placement[1], mb.placement[2]].map(f64::from);
            let added = fo4_walk::platform::add_boxes(&mut world, &[mb], &budget)?;
            refs.resize(world.sources.len(), None);
            for s in added {
                refs[s as usize] = Some(k.clone());
                marked.push((k.clone(), at, s));
            }
            bump(&mut counts, "marker boxes in the walking world");
        }
    }
    // The object bounds of the placed references with no collision in the
    // world (used under --play-pick-bounds).
    let (mut boxes, no_bounds) = pick::object_bounds(&scene, &refs);
    // The loaded doors whose collision is not placed, in their open pose
    // (`play_door_collision_open_pose_v1`), after the boxes: a door keeps
    // its object bounds for the activation and the held doors.
    let mut open_pose = std::collections::BTreeSet::new();
    if o.door_open {
        let placed: std::collections::BTreeSet<FormKey> = refs.iter().flatten().map(norm).collect();
        for i in &scene.instances {
            let door = i.base.as_ref().map(|b| b.record.signature) == Some(*b"DOOR");
            let k = norm(&i.reference.key);
            if !door || i.editor_marker || i.disabled_by_enable_parent() || placed.contains(&k) {
                continue;
            }
            match crate::elevator::open_door_collision(&loader, i, limits, placement, &budget.child())
            {
                Ok(Some(open)) => {
                    let label = format!(
                        "door open {}:{:08x} ({})",
                        k.plugin,
                        k.object_id,
                        crate::rules::DOOR_OPEN_COLLISION
                    );
                    let added = world.append(&open.world, &label, &budget)?;
                    refs.resize(world.sources.len(), None);
                    for s in added {
                        refs[s as usize] = Some(k.clone());
                    }
                    open_pose.insert(k.clone());
                    bump(&mut counts, "doors in their open pose in the walking world");
                }
                Ok(None) => bump(
                    &mut counts,
                    "doors left out of the walking world\tno frame, or no collision part the sequence named Open moves",
                ),
                Err(_) => bump(
                    &mut counts,
                    "doors left out of the walking world\tthe collision does not read",
                ),
            }
        }
    }
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
    let space = space_key;
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
    let mut live = if o.live {
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
    // The loaded doors' boxes, hidden in the dormant piece until the run
    // holds the door (`play_door_closed_blocked_holds_v1`).
    let mut held: BTreeMap<FormKey, Vec<u32>> = BTreeMap::new();
    let mut closed_at_load = std::collections::BTreeSet::new();
    let mut stored_open = std::collections::BTreeSet::new();
    if o.held_doors {
        if let Some((piece, _)) = live.as_mut() {
            for (i, (k, frame, lo, hi)) in boxes.items.iter().enumerate() {
                if boxes.kinds.get(i).copied().flatten() != Some(*b"DOOR") {
                    continue;
                }
                let label = format!(
                    "held door box {}:{:08x} (play_door_closed_blocked_holds_v1)",
                    k.plugin, k.object_id
                );
                let corners = held_door_corners(*frame, *lo, *hi);
                match piece.world.add_hull(&corners, 0., &label, &budget) {
                    Ok(src) => {
                        piece.refs.resize(piece.world.sources.len(), None);
                        piece.refs[src as usize] = Some(norm(k));
                        held.entry(norm(k)).or_default().push(src);
                        bump(&mut counts, "held door boxes in the dormant piece");
                        if o.stored_open && stores_onam(store, k, &budget.child()) == Some(true) {
                            stored_open.insert(norm(k));
                            bump(
                                &mut counts,
                                "held doors storing ONAM open from load (play_door_stored_open_v1)",
                            );
                        }
                        if o.held_at_load {
                            match stores_onam(store, k, &budget.child()) {
                                Some(false) => {
                                    closed_at_load.insert(norm(k));
                                    bump(&mut counts, "held doors closed at load (no ONAM; play_door_closed_at_load_v1)");
                                }
                                Some(true) => {
                                    bump(&mut counts, "held doors storing ONAM (no state at load)")
                                }
                                None => bump(
                                    &mut counts,
                                    "held doors whose record does not read (no state at load)",
                                ),
                            }
                        }
                    }
                    Err(_) => bump(
                        &mut counts,
                        "held door boxes left out\tthe box does not build",
                    ),
                }
            }
        }
    }
    // A box whose reference is not one of the scene's instances (a
    // persistent reference) is known to the live world by its own record.
    if let Some((_, info)) = live.as_mut() {
        for (k, at, s) in &marked {
            let i = info.entry(k.clone()).or_insert_with(|| RefInfo {
                piece: 0,
                listed_disabled: false,
                enable: None,
                stored: Some(*at),
                sources: Vec::new(),
            });
            if !i.sources.contains(s) {
                i.sources.push(*s);
            }
        }
    }
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
        unpicked: marked.iter().map(|(_, _, s)| *s).collect(),
        held,
        closed_at_load,
        open_pose,
        stored_open,
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

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_walk::platform::{add_boxes, MarkerBox};

    fn marker(layer: Option<u32>) -> MarkerBox {
        MarkerBox {
            reference: FormKey {
                plugin: "base.esm".into(),
                object_id: 0x900,
            },
            placement: [0., 0., 0., 0., 0., 0.],
            scale: 1.,
            half: [50., 10., 100.],
            enable_parent: false,
            initially_disabled: false,
            layer,
        }
    }

    /// The boxes the walking world places, as `build` keeps them.
    fn placed(boxes: &[MarkerBox], layers: bool) -> usize {
        let b = Budget::new(1 << 24);
        let kept: Vec<MarkerBox> = boxes
            .iter()
            .filter(|m| marker_box_left_out(Some(BOX_OBSTACLE_FLAG), m, layers).is_none())
            .cloned()
            .collect();
        let mut w = World::new();
        add_boxes(&mut w, &kept, &b).unwrap().len()
    }

    /// `play_collision_marker_box_layer_passed_v1`: with the reading, a box
    /// whose reference stores the word of a layer a walking character
    /// passes stays out; a box with
    /// no word, or another word, stands; without it every box stands.
    #[test]
    fn a_box_on_a_passed_layer_stays_out_only_with_the_reading() {
        let item_pick = marker(Some(40));
        let collision_box = marker(Some(35));
        let none = marker(None);
        let why = marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &item_pick, true);
        assert_eq!(
            why,
            Some("their collision layer word is one a walking character passes (39 to 44, 49)")
        );
        assert_eq!(
            marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &item_pick, false),
            None
        );
        for w in [39, 44, 49] {
            assert!(marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &marker(Some(w)), true).is_some());
        }
        for w in [0, 1, 31, 35, 38, 45, 48, 50] {
            assert!(marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &marker(Some(w)), true).is_none());
        }
        let all = [item_pick, collision_box, none];
        assert_eq!(placed(&all, false), 3);
        assert_eq!(placed(&all, true), 2);
    }

    /// `play_collision_marker_box_layer_records_v1`: the passed set the
    /// layer records give is every record's index the character's layer
    /// does not list, read through `fo4_plugin::coll`'s relation; a box's
    /// word is left out by that set, not by the constant.
    #[test]
    fn the_layer_records_give_the_layers_the_characters_layer_does_not_list() {
        let records: BTreeSet<u32> = [6, 40, 49].into_iter().collect();
        let passed = Passed::Records(records.clone());
        for (w, out) in [(40, true), (49, true), (6, true), (39, false), (35, false)] {
            assert_eq!(
                marker_box_left_out_with(Some(BOX_OBSTACLE_FLAG), &marker(Some(w)), Some(&passed))
                    .is_some(),
                out,
                "{w}"
            );
        }
        assert_eq!(
            marker_box_left_out_with(Some(BOX_OBSTACLE_FLAG), &marker(Some(40)), Some(&passed)),
            Some("their collision layer word is one the layer records do not list with the character's layer")
        );
        // Without a reading of the word, every box of the obstacle base stands.
        assert!(
            marker_box_left_out_with(Some(BOX_OBSTACLE_FLAG), &marker(Some(40)), None).is_none()
        );
        // The constant reading is the old one.
        assert_eq!(
            marker_box_left_out_with(
                Some(BOX_OBSTACLE_FLAG),
                &marker(Some(41)),
                Some(&Passed::Constant)
            ),
            marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &marker(Some(41)), true)
        );
        // The difference from the constant, both ways.
        assert_eq!(
            passed_difference(&records),
            (vec![39, 41, 42, 43, 44], vec![6])
        );
        let same: BTreeSet<u32> = PASSED_LAYERS.into_iter().collect();
        assert_eq!(passed_difference(&same), (vec![], vec![]));
        // The set from the relation of three layer records.
        let k = |id: u32| FormKey {
            plugin: "a.esm".into(),
            object_id: id,
        };
        let layer = |index: u32, id: u32, list: &[u32]| fo4_plugin::coll::Layer {
            key: k(id),
            editor_id: Vec::new(),
            name: Vec::new(),
            description: None,
            index,
            color: [0; 4],
            flags: 0,
            count: list.len() as u32,
            raw_collides: list.to_vec(),
            collides: list.iter().map(|i| k(*i)).collect(),
        };
        let layers = [
            layer(CHARACTER_LAYER, 1, &[1, 2]),
            layer(35, 2, &[1]),
            layer(40, 3, &[]),
        ];
        let r = fo4_plugin::coll::relation(&layers).unwrap();
        assert_eq!(passed_from_relation(&r), Some(BTreeSet::from([40])));
        // No record of the character's layer: none (the constant stands).
        let r = fo4_plugin::coll::relation(&[layer(35, 2, &[]), layer(40, 3, &[])]).unwrap();
        assert_eq!(passed_from_relation(&r), None);
    }

    /// The readings the layer one leaves as they were: the obstacle flag of
    /// the base and the box's own state.
    #[test]
    fn a_box_without_the_obstacle_base_or_enabled_placement_stays_out_as_before() {
        let m = marker(None);
        assert_eq!(
            marker_box_left_out(None, &m, false),
            Some("their base does not carry the obstacle flag")
        );
        assert_eq!(
            marker_box_left_out(Some(0x0080_0000), &marker(Some(40)), true),
            Some("their base does not carry the obstacle flag")
        );
        let off = MarkerBox {
            initially_disabled: true,
            ..m.clone()
        };
        assert_eq!(
            marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &off, true),
            Some("initially disabled or under an enable parent")
        );
        let parent = MarkerBox {
            enable_parent: true,
            ..m
        };
        assert_eq!(
            marker_box_left_out(Some(BOX_OBSTACLE_FLAG), &parent, false),
            Some("initially disabled or under an enable parent")
        );
    }
}
