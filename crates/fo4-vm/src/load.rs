//! Load events (`spec/vm.md`, Load events): `OnLoad` raised on a placed
//! reference when the run's world model says its 3D loads, from what the
//! run already models: the player's track (`crate::track`), the positions
//! of placed references and the moves of the run's natives.
//!
//! Each reading is a pending rule behind its own switch, all off by
//! default:
//!
//! - `world_load_reference_enters_v1` (`quest_run --load-in-cell`): a
//!   reference that comes into the interior cell the player's track is in
//!   (moved there by a native, or enabled there) loads at once;
//! - `world_load_player_arrives_v1` (`--load-on-arrival`): when the track
//!   reaches an interior cell (the instant load through a door: the
//!   track's point in the new space), every enabled reference standing in
//!   it whose scripts or holding aliases could handle the event loads at
//!   that second;
//! - `world_load_exterior_grid_v1` (`--load-grid N`): in a worldspace, a
//!   reference whose position lies within N grid squares of the player's
//!   loads when it comes into that range or the player's square changes.
//!
//! A load runs the reference's `OnLoad` handlers as a caller's
//! `--assume-load` does (`crate::player`: its own scripts, the aliases
//! holding it, the instances registered for the remote event). A reference
//! loads once until the player's area leaves it; `OnUnload`, `OnCellLoad`,
//! `OnCellAttach` and `OnCellDetach` are not raised (counted). Whether a
//! disabled reference loads is not modelled: a reference whose enable state
//! is undetermined does not load (counted). Nothing here is measured in
//! the running game.
use crate::{
    bind,
    data::key_record,
    player::{self, Action},
    quest, rules,
    track::{self, Spot, CELL_SIZE},
    world::{Clock, ObjId, Val, World},
    Vm,
};
use fo4_core::Budget;
use fo4_plugin::{
    typed::{Cell, CellKind, Refr, Store},
    FormKey,
};
use std::collections::{BTreeMap, BTreeSet};

/// The switches, each off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// `--load-in-cell` (`world_load_reference_enters_v1`).
    pub enters: bool,
    /// `--load-on-arrival` (`world_load_player_arrives_v1`).
    pub arrival: bool,
    /// `--load-grid N` (`world_load_exterior_grid_v1`).
    pub grid: Option<u32>,
}
impl Switches {
    pub fn any(&self) -> bool {
        self.enters || self.arrival || self.grid.is_some()
    }
}

/// Where the player is, as far as loading goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Area {
    Interior(FormKey),
    /// A worldspace and the player's grid square in it.
    Exterior(FormKey, i64, i64),
}

/// An interior cell, or a worldspace with one grid square.
type AreaKey = (FormKey, Option<(i64, i64)>);
/// A cell of a worldspace and its grid square, when it has one.
type GridCell = (FormKey, Option<(i64, i64)>);

/// The model's state in a run's world.
#[derive(Debug, Default)]
pub struct Loads {
    pub sw: Switches,
    /// References loaded and not left since.
    pub loaded: BTreeSet<FormKey>,
    /// The player's area at the last settling.
    pub area: Option<Area>,
    /// Objects a native moved or enabled since the last settling.
    pub checks: Vec<ObjId>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Placed references with a VMAD of their own or their base's, by
    /// interior cell or by (worldspace, grid square), read once.
    scripted: BTreeMap<AreaKey, Vec<(FormKey, [f64; 3])>>,
    /// The cells of each worldspace with their grid squares, read once.
    grids: BTreeMap<FormKey, Vec<GridCell>>,
    /// The scripted references of each worldspace's cells without a grid
    /// square, with their stored positions, read once.
    loose: BTreeMap<FormKey, Vec<(FormKey, [f64; 3])>>,
    /// Stored positions read so far, by reference.
    stored: BTreeMap<FormKey, Result<(FormKey, [f64; 3]), String>>,
}
impl Loads {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
}

/// Natives after which a reference is checked for a load.
const CHECKED: [&str; 7] = [
    "objectreference.moveto",
    "objectreference.setposition",
    "objectreference.movetonode",
    "objectreference.movetomyeditorlocation",
    "objectreference.enable",
    "objectreference.enablenowait",
    "objectreference.translateto",
];

/// Notes a native after which its receiver may load (a move or an
/// enable), before any host answers it.
pub fn observe(c: &crate::host::NativeCall, w: &mut World) {
    if !w.loads.sw.any() || !CHECKED.contains(&c.full_name().as_str()) {
        return;
    }
    if let Some(Val::Object(o)) = &c.receiver {
        if w.player != Some(*o) {
            w.loads.checks.push(*o);
        }
    }
}

fn grid(p: [f64; 3]) -> (i64, i64) {
    (
        (p[0] / CELL_SIZE).floor() as i64,
        (p[1] / CELL_SIZE).floor() as i64,
    )
}

/// The player's area at second `t` under the switches on: an interior
/// cell with `enters` or `arrival`, a worldspace square with `grid`.
pub fn area_at(store: &Store, w: &World, t: f64) -> Option<Area> {
    let track = w.place.track.as_ref()?;
    let (space, pos, _) = track.at(t)?;
    let sw = w.loads.sw;
    match store.signature(space).as_ref().map(|s| &s[..]) {
        Some(b"CELL") if sw.enters || sw.arrival => Some(Area::Interior(space.clone())),
        Some(b"WRLD") if sw.grid.is_some() => {
            let (i, j) = grid(pos);
            Some(Area::Exterior(space.clone(), i, j))
        }
        _ => None,
    }
}

/// Whether a position in a space lies in the area (within N squares of
/// an exterior area's square).
fn within(sw: Switches, area: &Area, space: &FormKey, pos: [f64; 3]) -> bool {
    match area {
        Area::Interior(c) => c == space,
        Area::Exterior(wld, i, j) => {
            let n = i64::from(sw.grid.unwrap_or(0));
            let (gi, gj) = grid(pos);
            wld == space && (gi - i).abs() <= n && (gj - j).abs() <= n
        }
    }
}

/// The earliest second after the run's present at which the player's
/// area changes along the track, when one is.
pub fn next_due(store: &Store, w: &World) -> Option<f64> {
    if !w.loads.sw.any() || !w.place.on {
        return None;
    }
    let track = w.place.track.clone()?;
    let now = w.now[Clock::Real as usize];
    let here = area_at(store, w, now);
    let mut best: Option<f64> = None;
    let mut take = |t: f64| {
        if t > now && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    };
    for pair in track.points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if b.t <= now {
            continue;
        }
        if a.space != b.space {
            take(b.t);
            continue;
        }
        if w.loads.sw.grid.is_none() || store.signature(&a.space) != Some(*b"WRLD") {
            continue;
        }
        // The first grid line the straight move crosses after now.
        let span = b.t - a.t;
        if span <= 0.0 {
            continue;
        }
        for axis in 0..2 {
            let (x0, x1) = (a.pos[axis], b.pos[axis]);
            if x0 == x1 {
                continue;
            }
            let (g0, g1) = ((x0 / CELL_SIZE).floor(), (x1 / CELL_SIZE).floor());
            let steps = (g1 - g0).abs() as i64;
            for k in 1..=steps.min(64) {
                let line = if x1 > x0 {
                    (g0 + k as f64) * CELL_SIZE
                } else {
                    (g0 - k as f64 + 1.0) * CELL_SIZE
                };
                let t = a.t + span * (line - x0) / (x1 - x0);
                if t > now {
                    take(t);
                    break;
                }
            }
        }
    }
    // A change already reached (the present second is the area's first)
    // is settled now.
    if here != w.loads.area && here.is_some() {
        return Some(now);
    }
    best
}

/// A placed reference's stored position (`DATA`), read without looking
/// for its cell.
fn placed_pos(store: &Store, k: &FormKey, b: &Budget) -> Option<[f64; 3]> {
    let t = match store.signature(k).as_ref().map(|s| &s[..]) {
        Some(b"REFR") => store.get::<Refr>(k, b).ok()??.data.transform?,
        Some(b"ACHR") => {
            let (r, plugin) = store.raw_with_plugin(k, b).ok()??;
            let ctx = fo4_plugin::actor::Context {
                order: store.order(),
                plugin,
            };
            fo4_plugin::actor::Achr::decode(&r, ctx, b)
                .ok()?
                .transform?
        }
        _ => return None,
    };
    Some([t[0], t[1], t[2]].map(f64::from))
}

/// Whether a placed reference or its base carries a VMAD.
fn has_vmad(store: &Store, k: &FormKey, b: &Budget) -> bool {
    let own = store
        .raw(k, &b.child())
        .ok()
        .flatten()
        .is_some_and(|r| r.subrecords.iter().any(|s| s.signature == *b"VMAD"));
    own || bind::base_of(store, k, &b.child())
        .ok()
        .flatten()
        .and_then(|base| store.raw(&base, &b.child()).ok().flatten())
        .is_some_and(|r| r.subrecords.iter().any(|s| s.signature == *b"VMAD"))
}

/// The scripted placed references (a VMAD of their own or their base's)
/// of the given cells, with their stored positions.
fn scripted_children(store: &Store, cells: &[FormKey], b: &Budget) -> Vec<(FormKey, [f64; 3])> {
    let mut out = Vec::new();
    for cell in cells {
        let Ok(ch) = store.children(cell, &b.child()) else {
            continue;
        };
        let actors = ch.excluded.iter().filter(|k| k.signature == *b"ACHR");
        for kid in ch.active.iter().chain(&ch.disabled).chain(actors) {
            if !has_vmad(store, &kid.key, b) {
                continue;
            }
            if let Some(p) = placed_pos(store, &kid.key, &b.child()) {
                out.push((kid.key.clone(), p));
            }
        }
    }
    out
}

/// The placed references of an interior cell, or of the exterior cells of
/// a worldspace square (and the worldspace's references in cells without a
/// square whose stored position lies in it), that carry a VMAD of their
/// own or their base's, with their stored positions.
fn scripted(store: &Store, w: &mut World, key: AreaKey) -> Vec<(FormKey, [f64; 3])> {
    if let Some(v) = w.loads.scripted.get(&key) {
        return v.clone();
    }
    let b = Budget::new(store.limits.max_memory);
    let (space, square) = key.clone();
    let mut out = match square {
        None => scripted_children(store, std::slice::from_ref(&space), &b),
        Some(sq) => {
            if !w.loads.grids.contains_key(&space) {
                let cells: Vec<GridCell> = store
                    .cells()
                    .filter(|c| store.world(c).as_ref() == Some(&space))
                    .map(|c| {
                        let g = store
                            .get::<Cell>(&c, &b.child())
                            .ok()
                            .flatten()
                            .and_then(|x| x.data.grid.map(|g| (i64::from(g.i), i64::from(g.j))));
                        (c, g)
                    })
                    .collect();
                // The references of the cells without a square, filtered by
                // position below.
                let loose: Vec<FormKey> = cells
                    .iter()
                    .filter(|(_, g)| g.is_none())
                    .map(|(c, _)| c.clone())
                    .collect();
                let persistent = scripted_children(store, &loose, &b);
                w.loads.grids.insert(space.clone(), cells);
                w.loads.loose.insert(space.clone(), persistent);
            }
            let cells: Vec<FormKey> = w.loads.grids[&space]
                .iter()
                .filter(|(_, g)| *g == Some(sq))
                .map(|(c, _)| c.clone())
                .collect();
            let mut v = scripted_children(store, &cells, &b);
            v.extend(
                w.loads.loose[&space]
                    .iter()
                    .filter(|(_, p)| grid(*p) == sq)
                    .cloned(),
            );
            v
        }
    };
    out.sort_by(|a, b| {
        (a.0.plugin.as_str(), a.0.object_id).cmp(&(b.0.plugin.as_str(), b.0.object_id))
    });
    out.dedup_by(|a, b| a.0 == b.0);
    w.loads.scripted.insert(key, out.clone());
    out
}

/// The references the aliases of the running quests hold (fills a run
/// set, forced, location and unique actor references), in quest and alias
/// order.
fn alias_held(store: &Store, w: &mut World) -> Vec<FormKey> {
    let max = store.limits.max_memory;
    let mut out = Vec::new();
    let keys: Vec<String> = w.quests.states.keys().cloned().collect();
    for qk in keys {
        let Some(q) = key_record(&qk) else { continue };
        let Some(info) = quest::info_by_key(store, max, w, &q) else {
            continue;
        };
        let s = &w.quests.states[&qk];
        if s.open.is_some() || s.running != Some(true) {
            continue;
        }
        let fills: Vec<(u32, Option<Val>)> = info
            .reference_aliases
            .keys()
            .map(|id| {
                (
                    *id,
                    i32::try_from(*id)
                        .ok()
                        .and_then(|i| s.fills.get(&i))
                        .cloned(),
                )
            })
            .collect();
        for (id, fill) in fills {
            match fill {
                Some(Val::Object(o)) => {
                    if let Some(k) = w.objects[o].key.as_deref().and_then(key_record) {
                        out.push(k);
                    }
                }
                Some(_) => {}
                None => {
                    if let Ok(Some(quest::Fill::Record(k))) = w.quests.fill(&info, id) {
                        out.push(k);
                    }
                }
            }
        }
    }
    out
}

fn area_space(a: &Area) -> &FormKey {
    match a {
        Area::Interior(c) => c,
        Area::Exterior(w, _, _) => w,
    }
}

fn is_object(w: &World, k: &FormKey) -> bool {
    let key = bind::record_key(k);
    w.objects
        .iter()
        .any(|x| x.key.as_deref() == Some(key.as_str()))
}

/// Where a reference stands now: through the world model for an object
/// of the run, its stored position for a record the run has not touched.
fn position(store: &Store, w: &mut World, k: &FormKey) -> Result<(FormKey, [f64; 3]), String> {
    let max = store.limits.max_memory;
    let key = bind::record_key(k);
    let object = w
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some(key.as_str()));
    if let Some(o) = object {
        // An object the run may have moved, or one the world model would
        // leave open, goes through the world model.
        let actor = store.signature(k) == Some(*b"ACHR");
        let moved = w.place.moved_at.contains_key(&o)
            || (actor && w.place.actor_moves.iter().any(|m| &m.actor == k));
        let off = if actor {
            !w.place.sw.actors
        } else {
            !w.place.sw.placed
        };
        if moved || off {
            return match track::spot(store, max, w, Some(&Val::Object(o))) {
                Spot::At { space, pos, .. } => Ok((space, pos)),
                Spot::Open(why) => Err(why.to_string()),
            };
        }
    }
    if let Some(x) = w.loads.stored.get(k) {
        return x.clone();
    }
    let x = track::stored_position(store, max, k)
        .map_err(|e| e.to_string())
        .and_then(|(space, pos)| {
            // An exterior cell's space is its worldspace.
            match store.kind(&space) {
                Some(CellKind::Exterior) => store
                    .world(&space)
                    .map(|wld| (wld, pos))
                    .ok_or_else(|| "an exterior cell without a worldspace".into()),
                _ => Ok((space, pos)),
            }
        });
    w.loads.stored.insert(k.clone(), x.clone());
    x
}

/// Raises the loads the model settles at the run's present second, in
/// order: when the player's area changed, the references standing in the
/// new area (`world_load_player_arrives_v1` for an interior,
/// `world_load_exterior_grid_v1` for a worldspace), then the objects
/// natives moved or enabled since the last settling
/// (`world_load_reference_enters_v1`, or the grid rule outside). The jobs
/// each load brings run before the next.
pub fn settle(
    vm: &mut Vm<'_>,
    store: &Store,
    max_jobs: usize,
    b: &Budget,
) -> (Vec<track::Raised>, Vec<quest::JobRun>) {
    let mut raised = Vec::new();
    let mut runs = Vec::new();
    if !vm.world.loads.sw.any() || !vm.world.place.on {
        return (raised, runs);
    }
    let sw = vm.world.loads.sw;
    let now = vm.world.now[Clock::Real as usize];
    let area = area_at(store, &vm.world, now);
    let mut candidates: Vec<(FormKey, &'static str, Option<[f64; 3]>)> = Vec::new();
    if area != vm.world.loads.area {
        // The references left behind unload; nothing is raised for them.
        let left: Vec<FormKey> = vm.world.loads.loaded.iter().cloned().collect();
        for k in left {
            let gone = match (&area, position(store, &mut vm.world, &k)) {
                (Some(a), Ok((space, pos))) => !within(sw, a, &space, pos),
                _ => true,
            };
            if gone {
                vm.world.loads.loaded.remove(&k);
                vm.world
                    .loads
                    .count("unloads: the player's area left the reference (OnUnload not raised)");
            }
        }
        match &area {
            Some(Area::Interior(c)) if sw.arrival => {
                vm.rules.insert(rules::WORLD_LOAD_ARRIVES);
                vm.world.loads.count("areas entered\tan interior cell");
                for (k, p) in scripted(store, &mut vm.world, (c.clone(), None)) {
                    candidates.push((k, "the player arrived in its cell", Some(p)));
                }
            }
            Some(Area::Exterior(wld, i, j)) => {
                vm.rules.insert(rules::WORLD_LOAD_GRID);
                vm.world
                    .loads
                    .count("areas entered\ta worldspace grid square");
                let n = i64::from(sw.grid.unwrap_or(0));
                for di in -n..=n {
                    for dj in -n..=n {
                        let key = (wld.clone(), Some((i + di, j + dj)));
                        for (k, p) in scripted(store, &mut vm.world, key) {
                            candidates.push((k, "the player's grid came within range", Some(p)));
                        }
                    }
                }
            }
            _ => {}
        }
        if area.is_some() && (sw.arrival || matches!(area, Some(Area::Exterior(..)))) {
            for k in alias_held(store, &mut vm.world) {
                candidates.push((k, "an alias holds it as the player's area changed", None));
            }
        }
        vm.world.loads.area = area.clone();
    }
    let checks = std::mem::take(&mut vm.world.loads.checks);
    let enters = match &area {
        Some(Area::Interior(_)) => sw.enters,
        Some(Area::Exterior(..)) => true,
        None => false,
    };
    if enters {
        for o in checks {
            if let Some(k) = vm.world.objects[o].key.as_deref().and_then(key_record) {
                candidates.push((k, "a native moved or enabled it", None));
            }
        }
    }
    let Some(area) = area else {
        return (raised, runs);
    };
    let mut seen = BTreeSet::new();
    for (k, why, known) in candidates {
        if !seen.insert(k.clone()) || vm.world.loads.loaded.contains(&k) {
            continue;
        }
        if !matches!(
            store.signature(&k).as_ref().map(|s| &s[..]),
            Some(b"REFR" | b"ACHR")
        ) {
            continue;
        }
        let at = match (known, area_space(&area)) {
            // A scripted reference of the area the run has not touched
            // stands at its stored position in the area's space.
            (Some(p), space) if !is_object(&vm.world, &k) => Ok((space.clone(), p)),
            _ => position(store, &mut vm.world, &k),
        };
        match at {
            Ok((space, pos)) if within(sw, &area, &space, pos) => {}
            Ok(_) => continue,
            Err(_) => {
                vm.world
                    .loads
                    .count("references not loaded\ttheir position is open");
                continue;
            }
        }
        match vm.host.reference_disabled(&mut vm.world, &k) {
            Some(Ok(false)) => {}
            Some(Ok(true)) => {
                vm.world
                    .loads
                    .count("references not loaded\tdisabled in the run");
                continue;
            }
            _ => {
                vm.world
                    .loads
                    .count("references not loaded\ttheir enable state is undetermined");
                continue;
            }
        }
        for r in vm.host.rules() {
            vm.rules.insert(r);
        }
        if why == "a native moved or enabled it" && matches!(area, Area::Interior(_)) {
            vm.rules.insert(rules::WORLD_LOAD_ENTERS);
        }
        vm.world.loads.loaded.insert(k.clone());
        vm.world.loads.count(format!("loads\t{why}"));
        let label = bind::record_key(&k);
        vm.world
            .loads
            .log
            .push((now, format!("{label} loads: {why}")));
        let delivered = player::deliver_by(vm, store, &k, Action::Load, None, b);
        let mut handler_runs = Vec::new();
        match delivered {
            Ok((ds, open)) => {
                if ds.is_empty() {
                    vm.world.loads.count("loads\tno handler ran");
                }
                if open > 0 {
                    vm.world
                        .loads
                        .count("loads\tquests left out: whether they run is undetermined");
                }
                for d in ds {
                    vm.world
                        .loads
                        .count(format!("loads\ta handler ran\t{}", d.receiver.kind()));
                    handler_runs.push((d.receiver.kind().to_string(), d.script, d.outcome));
                }
            }
            Err(e) => vm.world.loads.count(format!("loads\tnot delivered: {e}")),
        }
        runs.extend(quest::run_jobs(vm, store, max_jobs, b));
        if !handler_runs.is_empty() {
            raised.push(track::Raised {
                t: now,
                event: "onload",
                reference: Some(k),
                runs: handler_runs,
            });
        }
    }
    (raised, runs)
}
