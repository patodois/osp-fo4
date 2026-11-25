//! The live world (`--play-live-world`, `play_live_collision_follows_run_v1`):
//! the walking world follows the run's state at each of the walker's
//! slices, as a timeline frame at that second follows it
//! (`fo4_scene::timeline::apply`): a reference whose last change is a
//! disable leaves the walking world, an initially disabled one whose last
//! change is an enable enters it, an instance whose enable parent chain
//! names a changed reference takes the state the chain now gives, and a
//! reference the run moved stands at its new position with its stored
//! rotation and scale (moved to another space or to an open place: it
//! leaves). The NoWait forms count only with the caller's switch, as
//! `--timeline-nowait` does for frames.
//!
//! The walking world is a [`WorldSet`]: the world `play_run` always built
//! (piece `(0, 0)`), the collision of the references the run may enable
//! built once at load and hidden (piece `(1, 0)`), and one piece per moved
//! reference (`(2, n)`), its primitives moved by the move's translation.
//! Leaving is hiding the reference's sources; nothing is rebuilt but the
//! moved pieces.
use crate::pick;
use fo4_core::{Budget, Result};
use fo4_plugin::FormKey;
use fo4_scene::enable::Presence;
use fo4_scene::timeline::norm;
use fo4_vm::world::ObjId;
use fo4_vm::Vm;
use fo4_walk::{World, WorldSet};
use std::collections::BTreeMap;

/// A piece's key: (kind, index): `(0, 0)` the world as loaded, `(1, 0)`
/// the dormant collision, `(2, n)` a moved reference.
pub type Key = (u8, u32);
/// The world as loaded.
pub const BASE: Key = (0, 0);
/// The collision of the references the run may enable.
pub const DORMANT: Key = (1, 0);

/// A walking world and the placed reference of each of its sources.
#[derive(Debug, Clone, Default)]
pub struct Piece {
    pub world: World,
    pub refs: Vec<Option<FormKey>>,
}

/// What the live world knows of a loaded reference.
#[derive(Debug, Clone)]
pub struct RefInfo {
    /// 0: its collision is in the world as loaded; 1: in the dormant piece
    /// (an initially disabled reference, or an instance its enable parent
    /// disables at load).
    pub piece: u8,
    /// One of the cell's initially disabled references (not an instance
    /// of the loaded scene): it enters only when its own last change is
    /// an enable.
    pub listed_disabled: bool,
    /// Its enable parent chain and the state the chain gives at load.
    pub enable: Option<Presence>,
    /// Its stored position.
    pub stored: Option<[f64; 3]>,
    /// Its sources in its piece.
    pub sources: Vec<u32>,
}

/// Where the run moved a reference.
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    At {
        space: FormKey,
        pos: [f64; 3],
    },
    /// A native may have moved it; where is open.
    Open,
}

/// The run's visibility changes and moves so far, read from the run as
/// the timeline's writer reads them (`fo4_vm::timeline::Recorder`): the
/// `Enable` and `Disable` natives and their NoWait forms on placed
/// records, and the world model's native moves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunState {
    effects: usize,
    native_at: BTreeMap<ObjId, (f64, FormKey, [f64; 3])>,
    moved_at: BTreeMap<ObjId, f64>,
    /// Per reference: its last plain change and its last change of either
    /// form.
    pub visibility: BTreeMap<FormKey, (Option<bool>, bool)>,
    /// Per reference: the second and place of its last move.
    pub moves: BTreeMap<FormKey, (f64, Move)>,
    /// Read the doors' open and activation states
    /// ([`crate::rules::HELD_DOORS`], `--play-held-doors`): off, the
    /// `SetOpen` and `BlockActivation` natives are not read.
    pub doors_read: bool,
    /// Per reference, with `doors_read`: the last `SetOpen` argument (open)
    /// and the last `BlockActivation` first argument (activation blocked).
    pub doors: BTreeMap<FormKey, DoorState>,
}

/// What the run's scripts last did to a reference's open state and its
/// activation ([`crate::rules::HELD_DOORS`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoorState {
    /// The last `SetOpen`: `Some(false)` closed by a script.
    pub open: Option<bool>,
    /// The last `BlockActivation`: `Some(true)` the activation blocked.
    pub blocked: Option<bool>,
}

impl DoorState {
    /// Whether the door holds the walker: closed by a script and its
    /// activation blocked (a person's activation would not open it).
    pub fn holds(&self) -> bool {
        self.open == Some(false) && self.blocked == Some(true)
    }
}

/// The door state change a native call makes ([`crate::rules::HELD_DOORS`]):
/// `SetOpen(abOpen = true)` sets the open state, `BlockActivation(abBlocked
/// = true, ...)` the activation's (the defaults the Creation Kit's
/// documentation of the two functions gives); `None` for any other call or
/// an argument that is not a boolean.
pub fn door_change(native: &str, args: &[String]) -> Option<(bool, bool)> {
    let first = match args.first().map(String::as_str) {
        None => true,
        Some("true") => true,
        Some("false") => false,
        Some(_) => return None,
    };
    match native {
        "objectreference.setopen" => Some((true, first)),
        "objectreference.blockactivation" => Some((false, first)),
        _ => None,
    }
}

fn object_key(vm: &Vm<'_>, o: ObjId) -> Option<FormKey> {
    vm.world
        .objects
        .get(o)?
        .key
        .as_deref()
        .and_then(fo4_vm::data::key_record)
        .map(|k| norm(&k))
}

impl RunState {
    /// An empty state that reads the doors' open and activation states
    /// when `doors` ([`RunState::doors_read`]).
    pub fn reading_doors(doors: bool) -> RunState {
        RunState {
            doors_read: doors,
            ..RunState::default()
        }
    }

    /// Reads what the run did since the last call; whether anything
    /// changed.
    pub fn update(&mut self, vm: &Vm<'_>) -> bool {
        let mut changed = false;
        for e in vm.effects.iter().skip(self.effects) {
            if self.doors_read {
                if let Some((is_open, v)) = door_change(&e.native, &e.args) {
                    if let Some(k) = e.receiver_object.and_then(|o| object_key(vm, o)) {
                        let d = self.doors.entry(k).or_default();
                        if is_open {
                            d.open = Some(v);
                        } else {
                            d.blocked = Some(v);
                        }
                        changed = true;
                    }
                    continue;
                }
            }
            let shown = match e.native.as_str() {
                "objectreference.enable" | "objectreference.enablenowait" => true,
                "objectreference.disable" | "objectreference.disablenowait" => false,
                _ => continue,
            };
            let nowait = e.native.ends_with("nowait");
            if let Some(k) = e.receiver_object.and_then(|o| object_key(vm, o)) {
                let v = self.visibility.entry(k).or_insert((None, shown));
                v.1 = shown;
                if !nowait {
                    v.0 = Some(shown);
                }
                changed = true;
            }
        }
        self.effects = vm.effects.len();
        let w = &vm.world.place;
        for (o, at) in &w.native_at {
            if self.native_at.get(o) == Some(at) {
                continue;
            }
            self.native_at.insert(*o, at.clone());
            if let Some(k) = object_key(vm, *o) {
                self.set_move(
                    k,
                    at.0,
                    Move::At {
                        space: norm(&at.1),
                        pos: at.2,
                    },
                );
                changed = true;
            }
        }
        for (o, t) in &w.moved_at {
            if self.moved_at.get(o) == Some(t) {
                continue;
            }
            self.moved_at.insert(*o, *t);
            let placed = w.native_at.get(o).is_some_and(|x| x.0 == *t);
            if let (false, Some(k)) = (placed, object_key(vm, *o)) {
                self.set_move(k, *t, Move::Open);
                changed = true;
            }
        }
        changed
    }

    fn set_move(&mut self, k: FormKey, t: f64, m: Move) {
        match self.moves.get(&k) {
            Some((t0, _)) if *t0 > t => {}
            _ => {
                self.moves.insert(k, (t, m));
            }
        }
    }

    /// A reference's last change as a frame reads it: the last plain one,
    /// or with `nowait` the last of either form.
    pub fn shown(&self, k: &FormKey, nowait: bool) -> Option<bool> {
        let v = self.visibility.get(k)?;
        if nowait {
            Some(v.1)
        } else {
            v.0
        }
    }
}

/// What the run's state makes of a loaded reference's collision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    Absent,
    /// Present, moved by this translation (zero where it was placed).
    Present([f64; 3]),
}

/// The decision for one reference, as `fo4_scene::timeline::apply` decides
/// what a frame draws: `shown` gives each reference's last change (by
/// normalised key), `mv` this reference's last move, `space` the space of
/// the walking world. With `child_flag_agrees`
/// (`xesp_child_flag_agrees_chain_v1`, `--play-collision-child-flag-agrees`)
/// an enable chain left open only by records below its root carrying their
/// own flag 0x800, each agreeing with the chain, is read as any other chain
/// (`Presence::with_agreeing_child_flags`), with or without the run's
/// changes: the state the loader gave it at the start under the same
/// switch, then the state its records' changes give.
pub fn decide(
    info: &RefInfo,
    key: &FormKey,
    shown: &dyn Fn(&FormKey) -> Option<bool>,
    mv: Option<&Move>,
    space: &FormKey,
    child_flag_agrees: bool,
) -> Decision {
    let own = shown(key);
    let in_scene = if info.listed_disabled {
        own == Some(true)
    } else {
        own != Some(false)
    };
    if !in_scene {
        return Decision::Absent;
    }
    let enabled = match &info.enable {
        Some(p) if child_flag_agrees && fo4_scene::readiness::child_flag_agrees(p) => {
            p.with_agreeing_child_flags(&|k| shown(&norm(k)))
        }
        Some(p) if p.chain.nodes.iter().any(|n| shown(&norm(&n.key)).is_some()) => {
            p.with(&|k| shown(&norm(k)))
        }
        Some(p) => p.enabled,
        None => None,
    };
    if enabled == Some(false) {
        return Decision::Absent;
    }
    match mv {
        None => Decision::Present([0.0; 3]),
        Some(Move::At { space: s, pos }) if norm(s) == norm(space) => match info.stored {
            Some(st) => {
                Decision::Present(std::array::from_fn(|k| f64::from(pos[k] as f32) - st[k]))
            }
            None => Decision::Present([0.0; 3]),
        },
        Some(_) => Decision::Absent,
    }
}

/// Counts and costs of the live world.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Times the run's state changed and was applied.
    pub applies: u64,
    /// Reference decisions that changed, by what they became.
    pub entered: u64,
    pub left: u64,
    pub moved: u64,
    /// Seconds spent applying (wall time).
    pub seconds: f64,
    /// The most seconds one apply took.
    pub longest: f64,
    /// Held doors' boxes that entered and left the walking world
    /// ([`crate::rules::HELD_DOORS`]).
    pub held: u64,
    pub released: u64,
}

/// The walking world with the run's state applied.
#[derive(Debug, Clone)]
pub struct Live {
    set: WorldSet<Key>,
    refs: BTreeMap<Key, Vec<Option<FormKey>>>,
    info: BTreeMap<FormKey, RefInfo>,
    space: FormKey,
    nowait: bool,
    /// `xesp_child_flag_agrees_chain_v1` in the decisions ([`decide`]).
    child_flag_agrees: bool,
    decided: BTreeMap<FormKey, Decision>,
    /// Sources of the world as loaded that the activation's ray passes
    /// through ([`crate::world::Built::unpicked`]).
    unpicked: std::collections::BTreeSet<u32>,
    /// The doors' boxes in the dormant piece, by door
    /// ([`crate::rules::HELD_DOORS`], [`Live::with_held_doors`]): shown
    /// while the run's state holds the door; the activation's ray passes
    /// through them.
    held: BTreeMap<FormKey, Vec<u32>>,
    /// The doors whose boxes are shown.
    holding: std::collections::BTreeSet<FormKey>,
    pub stats: Stats,
}

impl Live {
    /// A walking world that does not follow the run (the play mode without
    /// `--play-live-world`): the world as loaded, as one piece.
    pub fn fixed(world: World, refs: Vec<Option<FormKey>>) -> Result<Live> {
        let mut set = WorldSet::new();
        set.insert(BASE, world)?;
        Ok(Live {
            set,
            refs: BTreeMap::from([(BASE, refs)]),
            info: BTreeMap::new(),
            space: FormKey {
                plugin: String::new(),
                object_id: 0,
            },
            nowait: false,
            child_flag_agrees: false,
            decided: BTreeMap::new(),
            unpicked: Default::default(),
            held: BTreeMap::new(),
            holding: Default::default(),
            stats: Stats::default(),
        })
    }

    /// The sources of the world as loaded that the activation's ray passes
    /// through (the collision markers' boxes).
    pub fn with_unpicked(mut self, sources: &[u32]) -> Live {
        self.unpicked = sources.iter().copied().collect();
        self
    }

    /// The doors' boxes ([`crate::rules::HELD_DOORS`], `--play-held-doors`):
    /// per door (normalised key), its sources in the dormant piece, hidden
    /// until [`Live::apply`] finds the run's state holding the door.
    pub fn with_held_doors(mut self, held: BTreeMap<FormKey, Vec<u32>>) -> Live {
        for srcs in held.values() {
            for s in srcs {
                self.set.set_hidden(&DORMANT, *s, true);
            }
        }
        self.held = held;
        self.holding.clear();
        self
    }

    /// The doors whose boxes this live world holds (shown or not).
    pub fn held_doors(&self) -> impl Iterator<Item = &FormKey> {
        self.held.keys()
    }

    /// The doors whose boxes stand in the walking world now.
    pub fn holding(&self) -> &std::collections::BTreeSet<FormKey> {
        &self.holding
    }

    /// Decides the chains left open only by agreeing own flags as any
    /// other (`xesp_child_flag_agrees_chain_v1`,
    /// `--play-collision-child-flag-agrees`; off by default): the walking
    /// world built under the same switch ([`crate::world::Options`]) took
    /// their start state from the loader, and the run's changes reach them
    /// through their chain.
    pub fn with_child_flag_agrees(mut self, on: bool) -> Live {
        self.child_flag_agrees = on;
        self
    }

    /// Whether [`Live::with_child_flag_agrees`] is on.
    pub fn child_flag_agrees(&self) -> bool {
        self.child_flag_agrees
    }

    /// The live world: the world as loaded, the dormant piece (hidden), the
    /// references' knowledge and the space; `nowait` counts the NoWait
    /// forms.
    pub fn new(
        world: World,
        refs: Vec<Option<FormKey>>,
        dormant: Piece,
        info: BTreeMap<FormKey, RefInfo>,
        space: FormKey,
        nowait: bool,
    ) -> Result<Live> {
        let mut live = Live::fixed(world, refs)?;
        let n = dormant.world.sources.len() as u32;
        live.set.insert(DORMANT, dormant.world)?;
        live.refs.insert(DORMANT, dormant.refs);
        for s in 0..n {
            live.set.set_hidden(&DORMANT, s, true);
        }
        live.space = space;
        live.nowait = nowait;
        for (k, i) in &info {
            let d = if i.piece == 0 {
                Decision::Present([0.0; 3])
            } else {
                Decision::Absent
            };
            live.decided.insert(k.clone(), d);
        }
        live.info = info;
        Ok(live)
    }

    /// The walking world to walk and pick in.
    pub fn space(&self) -> &WorldSet<Key> {
        &self.set
    }

    /// The walking world, to put pieces of its own into (the moving
    /// platforms, [`crate::elevator`]).
    pub fn space_mut(&mut self) -> &mut WorldSet<Key> {
        &mut self.set
    }

    /// The world as loaded.
    pub fn base(&self) -> Option<&World> {
        self.set.get(&BASE)
    }

    /// The placed reference of each source of the world as loaded.
    pub fn base_refs(&self) -> &[Option<FormKey>] {
        self.refs.get(&BASE).map(Vec::as_slice).unwrap_or_default()
    }

    /// Whether the live world follows the run.
    pub fn follows(&self) -> bool {
        !self.info.is_empty()
    }

    /// The decision now standing for a reference (`None`: not one the
    /// live world knows).
    pub fn decision(&self, k: &FormKey) -> Option<Decision> {
        self.decided.get(&norm(k)).copied()
    }

    /// Applies the run's state: hides and shows sources, rebuilds the
    /// moved pieces when a move changed. Returns how many references'
    /// decisions changed.
    pub fn apply(&mut self, s: &RunState, b: &Budget) -> Result<usize> {
        let started = std::time::Instant::now();
        let nowait = self.nowait;
        let agrees = self.child_flag_agrees;
        let shown = |k: &FormKey| s.shown(k, nowait);
        let mut changed = Vec::new();
        for (k, i) in &self.info {
            let mv = s.moves.get(k).map(|(_, m)| m);
            let d = decide(i, k, &shown, mv, &self.space, agrees);
            if self.decided.get(k) != Some(&d) {
                changed.push((k.clone(), d));
            }
        }
        let mut moves_changed = false;
        for (k, d) in &changed {
            let before = self.decided.insert(k.clone(), *d);
            let i = &self.info[k];
            let piece = if i.piece == 0 { BASE } else { DORMANT };
            let at_home = *d == Decision::Present([0.0; 3]);
            for src in &i.sources {
                self.set.set_hidden(&piece, *src, !at_home);
            }
            let was_moved = matches!(before, Some(Decision::Present(v)) if v != [0.0; 3]);
            let is_moved = matches!(d, Decision::Present(v) if *v != [0.0; 3]);
            moves_changed |= was_moved || is_moved;
            match d {
                Decision::Absent => self.stats.left += 1,
                Decision::Present(v) if *v == [0.0; 3] => self.stats.entered += 1,
                Decision::Present(_) => self.stats.moved += 1,
            }
        }
        if moves_changed {
            self.rebuild_moved(b)?;
        }
        // A held door's box: while the run's scripts hold it closed with
        // its activation blocked and the reference itself is not disabled.
        let mut held_changed = 0usize;
        for (k, srcs) in &self.held {
            let want = s.doors.get(k).is_some_and(DoorState::holds)
                && s.shown(k, nowait) != Some(false)
                && self.decided.get(k) != Some(&Decision::Absent);
            if want == self.holding.contains(k) {
                continue;
            }
            for src in srcs {
                self.set.set_hidden(&DORMANT, *src, !want);
            }
            if want {
                self.holding.insert(k.clone());
                self.stats.held += 1;
            } else {
                self.holding.remove(k);
                self.stats.released += 1;
            }
            held_changed += 1;
        }
        let took = started.elapsed().as_secs_f64();
        self.stats.applies += 1;
        self.stats.seconds += took;
        self.stats.longest = self.stats.longest.max(took);
        Ok(changed.len() + held_changed)
    }

    fn rebuild_moved(&mut self, b: &Budget) -> Result<()> {
        let old: Vec<Key> = self.set.keys().filter(|k| k.0 == 2).copied().collect();
        for k in old {
            self.set.remove(&k);
            self.refs.remove(&k);
        }
        let mut n = 0u32;
        for (k, d) in &self.decided {
            let Decision::Present(v) = d else { continue };
            if *v == [0.0; 3] {
                continue;
            }
            let i = &self.info[k];
            if i.sources.is_empty() {
                continue;
            }
            let from = if i.piece == 0 { BASE } else { DORMANT };
            let Some(w) = self.set.get(&from) else {
                continue;
            };
            let moved = w.copy_sources(&i.sources, *v, b)?;
            let refs = vec![Some(k.clone()); moved.sources.len()];
            self.set.insert((2, n), moved)?;
            self.refs.insert((2, n), refs);
            n += 1;
        }
        Ok(())
    }

    /// Makes `set` (a walking world whose piece `[0, 0]` is this live
    /// world's world as loaded, the viewer's window) answer as this live
    /// world does: the same sources of `[0, 0]` hidden, the dormant piece
    /// under `[i32::MAX, 0]` and the moved pieces under `[i32::MAX, n + 1]`.
    pub fn mirror_into(&self, set: &mut WorldSet<[i32; 2]>) -> Result<()> {
        let moved: Vec<[i32; 2]> = set.keys().filter(|k| k[0] == i32::MAX).copied().collect();
        for k in moved {
            if k[1] > 0 {
                set.remove(&k);
            }
        }
        if let Some(d) = self.set.get(&DORMANT) {
            if !set.contains(&[i32::MAX, 0]) {
                set.insert([i32::MAX, 0], d.clone())?;
            }
        }
        for (key, to) in [(BASE, [0, 0]), (DORMANT, [i32::MAX, 0])] {
            let n = self.set.get(&key).map_or(0, |w| w.sources.len() as u32);
            for s in 0..n {
                set.set_hidden(&to, s, self.set.is_hidden(&key, s));
            }
        }
        for (k, w, _) in self.set.pieces() {
            if k.0 == 2 {
                set.insert([i32::MAX, k.1 as i32 + 1], w.clone())?;
            }
        }
        Ok(())
    }

    /// The first primitive (or, with `boxes`, object bounds) the eye's ray
    /// meets within `reach` among the sources shown, passing through the
    /// references `skip` names.
    pub fn pick(
        &self,
        boxes: Option<&pick::Boxes>,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
        skip: &dyn Fn(&FormKey) -> bool,
    ) -> Option<pick::Hit> {
        self.pick_ranked(boxes, eye, dir, reach, skip, false)
    }

    /// [`Live::pick`] in the order `pick::rank` gives: with `first_met`
    /// (`play_activate_ray_first_met_v1`) what the eye stands inside comes
    /// after everything the ray meets from outside.
    pub fn pick_ranked(
        &self,
        boxes: Option<&pick::Boxes>,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
        skip: &dyn Fn(&FormKey) -> bool,
        first_met: bool,
    ) -> Option<pick::Hit> {
        let mut best: Option<(pick::Hit, bool)> = None;
        for (k, w, _) in self.set.pieces() {
            let refs = self.refs.get(k).map(Vec::as_slice).unwrap_or_default();
            let keep = |s: u32| {
                !self.set.is_hidden(k, s)
                    && !(*k == BASE && self.unpicked.contains(&s))
                    && !(*k == DORMANT && self.held.values().any(|v| v.contains(&s)))
            };
            let hit = pick::met(w, refs, None, eye, dir, reach, &keep, skip, first_met);
            // The earlier piece keeps a tie.
            best = pick::nearer(first_met, best, hit);
        }
        let boxed = pick::met(
            &World::new(),
            &[],
            boxes,
            eye,
            dir,
            reach,
            &|_| true,
            skip,
            first_met,
        );
        pick::nearer(first_met, best, boxed).map(|(h, _)| h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_plugin::enable::{Chain, ChainEnd, EnableLink, Node};

    fn key(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    fn info(piece: u8, listed: bool, sources: Vec<u32>) -> RefInfo {
        RefInfo {
            piece,
            listed_disabled: listed,
            enable: None,
            stored: Some([100.0, 0.0, 0.0]),
            sources,
        }
    }

    /// A floor (source 0), a wall across x = 100 (source 1, reference 1)
    /// and, dormant, a wall across x = 60 (reference 2).
    fn live() -> Live {
        let b = Budget::new(1 << 24);
        let mut w = World::new();
        w.add_triangles(
            &[
                [
                    [-500.0, -500.0, 0.0],
                    [500.0, -500.0, 0.0],
                    [500.0, 500.0, 0.0],
                ],
                [
                    [-500.0, -500.0, 0.0],
                    [500.0, 500.0, 0.0],
                    [-500.0, 500.0, 0.0],
                ],
            ],
            "floor",
            &b,
        )
        .unwrap();
        let wall = |x: f64| {
            [
                [[x, -200.0, 0.0], [x, 200.0, 0.0], [x, 200.0, 300.0]],
                [[x, -200.0, 0.0], [x, 200.0, 300.0], [x, -200.0, 300.0]],
            ]
        };
        w.add_triangles(&wall(100.0), "wall", &b).unwrap();
        let mut d = World::new();
        d.add_triangles(&wall(60.0), "dormant wall", &b).unwrap();
        let mut infos = BTreeMap::new();
        infos.insert(key(1), info(0, false, vec![1]));
        let mut two = info(1, true, vec![0]);
        two.stored = Some([60.0, 0.0, 0.0]);
        infos.insert(key(2), two);
        Live::new(
            w,
            vec![None, Some(key(1))],
            Piece {
                world: d,
                refs: vec![Some(key(2))],
            },
            infos,
            key(100),
            false,
        )
        .unwrap()
    }

    fn ray(l: &Live) -> Option<(f64, Option<FormKey>)> {
        l.pick(None, [0.0, 0.0, 50.0], [1.0, 0.0, 0.0], 1000.0, &|_| false)
            .map(|h| (h.distance, h.reference))
    }

    fn state(vis: &[(u32, Option<bool>, bool)], moves: &[(u32, Move)]) -> RunState {
        let mut s = RunState::default();
        for (k, plain, any) in vis {
            s.visibility.insert(key(*k), (*plain, *any));
        }
        for (k, m) in moves {
            s.moves.insert(key(*k), (1.0, m.clone()));
        }
        s
    }

    #[test]
    fn the_run_takes_references_out_puts_them_in_and_moves_them() {
        let b = Budget::new(1 << 24);
        let mut l = live();
        assert_eq!(ray(&l), Some((100.0, Some(key(1)))));
        // Disabled: the ray passes to nothing.
        let n = l
            .apply(&state(&[(1, Some(false), false)], &[]), &b)
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(ray(&l), None);
        // The dormant one enabled: it stands at x = 60.
        l.apply(
            &state(&[(1, Some(false), false), (2, Some(true), true)], &[]),
            &b,
        )
        .unwrap();
        assert_eq!(ray(&l), Some((60.0, Some(key(2)))));
        // Moved 30 units on: met at x = 90; the stored one passed through.
        let to = Move::At {
            space: key(100),
            pos: [90.0, 0.0, 0.0],
        };
        l.apply(
            &state(
                &[(1, Some(false), false), (2, Some(true), true)],
                &[(2, to)],
            ),
            &b,
        )
        .unwrap();
        let (d, r) = ray(&l).unwrap();
        assert!((d - 90.0).abs() < 1e-9 && r == Some(key(2)), "{d}");
        assert_eq!(
            l.decision(&key(2)),
            Some(Decision::Present([30.0, 0.0, 0.0]))
        );
        // Moved to another space: it leaves.
        let away = Move::At {
            space: key(101),
            pos: [90.0, 0.0, 0.0],
        };
        l.apply(
            &state(
                &[(1, Some(false), false), (2, Some(true), true)],
                &[(2, away)],
            ),
            &b,
        )
        .unwrap();
        assert_eq!(ray(&l), None);
        assert_eq!(l.stats.applies, 4);
    }

    #[test]
    fn a_mirrored_set_collides_as_the_live_world() {
        let b = Budget::new(1 << 24);
        let mut l = live();
        let mut set = WorldSet::new();
        set.insert([0, 0], l.base().unwrap().clone()).unwrap();
        let to = Move::At {
            space: key(100),
            pos: [90.0, 0.0, 0.0],
        };
        l.apply(
            &state(
                &[(1, Some(false), false), (2, Some(true), true)],
                &[(2, to)],
            ),
            &b,
        )
        .unwrap();
        l.mirror_into(&mut set).unwrap();
        let contacts = |s: &dyn fo4_walk::Space| {
            let (mut scratch, mut out) = (Vec::new(), Vec::new());
            let mut hit = Vec::new();
            for x in [55.0, 85.0, 95.0] {
                s.contacts(
                    [[x, 0.0, 20.0], [x, 0.0, 100.0]],
                    16.0,
                    0.0,
                    &mut scratch,
                    &mut out,
                );
                hit.push(out.len());
            }
            hit
        };
        let a = contacts(l.space());
        assert_eq!(a, contacts(&set));
        // Only the moved wall at x = 90 is met (the floor below z = 4).
        assert_eq!(a, vec![0, 1, 1]);
        // Back home: the moved piece goes from the mirror too.
        l.apply(&state(&[(2, Some(true), true)], &[]), &b).unwrap();
        l.mirror_into(&mut set).unwrap();
        assert_eq!(contacts(l.space()), contacts(&set));
        assert!(set.keys().all(|k| *k != [i32::MAX, 1]));
    }

    /// `play_door_closed_blocked_holds_v1`: [`live`] with, in the dormant
    /// piece, door 3's box across x = 30 to 40 (source 1).
    fn live_with_door() -> Live {
        let b = Budget::new(1 << 24);
        let mut l = live();
        let frame = fo4_scene::transform::Transform {
            translation: [35.0, 0.0, 0.0],
            ..fo4_scene::transform::Transform::IDENTITY
        };
        let corners =
            crate::world::held_door_corners(frame, [-5.0, -50.0, 0.0], [5.0, 50.0, 200.0]);
        let d = l.set.get(&DORMANT).unwrap().clone();
        let mut d = d;
        let src = d.add_hull(&corners, 0.0, "door", &b).unwrap();
        assert_eq!(src, 1);
        l.set.insert(DORMANT, d).unwrap();
        // The dormant wall (reference 2) stays hidden, as `Live::new` left it.
        l.set.set_hidden(&DORMANT, 0, true);
        l.refs.get_mut(&DORMANT).unwrap().push(Some(key(3)));
        l.with_held_doors(BTreeMap::from([(key(3), vec![src])]))
    }

    fn touches_at(s: &dyn fo4_walk::Space, x: f64) -> usize {
        let (mut scratch, mut out) = (Vec::new(), Vec::new());
        s.contacts(
            [[x, 0.0, 20.0], [x, 0.0, 100.0]],
            16.0,
            0.0,
            &mut scratch,
            &mut out,
        );
        out.len()
    }

    fn door(open: Option<bool>, blocked: Option<bool>) -> RunState {
        let mut s = RunState::reading_doors(true);
        s.doors.insert(key(3), DoorState { open, blocked });
        s
    }

    #[test]
    fn a_door_the_run_closes_and_blocks_holds_the_walker_until_it_opens() {
        let b = Budget::new(1 << 24);
        let mut l = live_with_door();
        // Not touched by the run: no box (doors place no collision).
        assert_eq!(l.apply(&RunState::reading_doors(true), &b).unwrap(), 0);
        assert_eq!(touches_at(l.space(), 35.0), 0);
        // Closed only (a person's activation would open it): no box.
        l.apply(&door(Some(false), None), &b).unwrap();
        assert_eq!(touches_at(l.space(), 35.0), 0);
        // Closed with its activation blocked: the box stops the walker,
        // and the eye's ray passes through it to the wall at x = 100.
        assert_eq!(l.apply(&door(Some(false), Some(true)), &b).unwrap(), 1);
        assert_eq!(touches_at(l.space(), 35.0), 1);
        assert!(l.holding().contains(&key(3)));
        assert_eq!(ray(&l), Some((100.0, Some(key(1)))));
        let mut set = WorldSet::new();
        set.insert([0, 0], l.base().unwrap().clone()).unwrap();
        l.mirror_into(&mut set).unwrap();
        assert_eq!(touches_at(&set, 35.0), 1);
        // Opened by a script: the way is free again.
        l.apply(&door(Some(true), Some(true)), &b).unwrap();
        assert_eq!(touches_at(l.space(), 35.0), 0);
        // Closed and blocked again, then its activation unblocked.
        l.apply(&door(Some(false), Some(true)), &b).unwrap();
        assert_eq!(touches_at(l.space(), 35.0), 1);
        l.apply(&door(Some(false), Some(false)), &b).unwrap();
        assert_eq!(touches_at(l.space(), 35.0), 0);
        // Held but disabled by the run: no box.
        let mut s = door(Some(false), Some(true));
        s.visibility.insert(key(3), (Some(false), false));
        l.apply(&s, &b).unwrap();
        assert_eq!(touches_at(l.space(), 35.0), 0);
        assert_eq!((l.stats.held, l.stats.released), (2, 2));
    }

    #[test]
    fn the_door_natives_read_as_their_documented_defaults() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            door_change("objectreference.setopen", &a(&["false"])),
            Some((true, false))
        );
        assert_eq!(
            door_change("objectreference.setopen", &a(&[])),
            Some((true, true))
        );
        assert_eq!(
            door_change("objectreference.blockactivation", &a(&["true", "true"])),
            Some((false, true))
        );
        assert_eq!(
            door_change("objectreference.blockactivation", &a(&["false", "false"])),
            Some((false, false))
        );
        assert_eq!(
            door_change("objectreference.setopen", &a(&["?3(native x)"])),
            None
        );
        assert_eq!(door_change("objectreference.enable", &a(&[])), None);
        assert!(DoorState {
            open: Some(false),
            blocked: Some(true)
        }
        .holds());
        assert!(!DoorState {
            open: None,
            blocked: Some(true)
        }
        .holds());
    }

    #[test]
    fn nowait_changes_count_only_with_the_reading() {
        let b = Budget::new(1 << 24);
        // Only a NoWait disable: without the reading nothing changes.
        let s = state(&[(1, None, false)], &[]);
        let mut l = live();
        assert_eq!(l.apply(&s, &b).unwrap(), 0);
        assert_eq!(ray(&l), Some((100.0, Some(key(1)))));
        l.nowait = true;
        assert_eq!(l.apply(&s, &b).unwrap(), 1);
        assert_eq!(ray(&l), None);
    }

    #[test]
    fn a_changed_enable_parent_decides_its_children() {
        // Reference 1's chain names parent 9: the parent disabled, the
        // child leaves; enabled again, it comes back.
        let node = |id: u32, parent: Option<u32>| Node {
            key: key(id),
            signature: *b"REFR",
            initially_disabled: false,
            deleted: false,
            link: parent.map(|p| EnableLink {
                raw: p,
                parent: Some(key(p)),
                flags: 0,
                reserved: [0; 3],
                subrecord: 1,
            }),
        };
        let chain = Chain {
            nodes: vec![node(1, Some(9)), node(9, None)],
            end: ChainEnd::Root,
        };
        let mut i = info(0, false, vec![1]);
        i.enable = Some(Presence::of(chain));
        let shown_off = |k: &FormKey| (*k == key(9)).then_some(false);
        let shown_on = |k: &FormKey| (*k == key(9)).then_some(true);
        let off = decide(&i, &key(1), &shown_off, None, &key(100), false);
        let on = decide(&i, &key(1), &shown_on, None, &key(100), false);
        assert_eq!(on, Decision::Present([0.0; 3]));
        assert_eq!(off, Decision::Absent, "{:?}", i.enable);
    }

    #[test]
    fn a_chain_open_only_for_agreeing_own_flags_decides_behind_its_switch() {
        // The opening's class: reference 1 carries its own flag 0x800, its
        // link to root 9 is opposite (bit 0), the root lacks the flag:
        // the chain and the flag both start it disabled.
        let chain = Chain {
            nodes: vec![
                Node {
                    key: key(1),
                    signature: *b"REFR",
                    initially_disabled: true,
                    deleted: false,
                    link: Some(EnableLink {
                        raw: 9,
                        parent: Some(key(9)),
                        flags: 1,
                        reserved: [0; 3],
                        subrecord: 1,
                    }),
                },
                Node {
                    key: key(9),
                    signature: *b"REFR",
                    initially_disabled: false,
                    deleted: false,
                    link: None,
                },
            ],
            end: ChainEnd::Root,
        };
        let mut i = info(0, false, vec![1]);
        i.enable = Some(Presence::of(chain));
        let none = |_: &FormKey| None;
        let root_off = |k: &FormKey| (*k == key(9)).then_some(false);
        let root_on = |k: &FormKey| (*k == key(9)).then_some(true);
        let d = |shown: &dyn Fn(&FormKey) -> Option<bool>, on: bool| {
            decide(&i, &key(1), shown, None, &key(100), on)
        };
        // Off: undecided, as before (present where it was loaded).
        assert_eq!(d(&none, false), Decision::Present([0.0; 3]));
        assert_eq!(d(&root_off, false), Decision::Present([0.0; 3]));
        // On: absent at the start, present once the run disables the
        // root (the opposite link), absent when it enables it again.
        assert_eq!(d(&none, true), Decision::Absent);
        assert_eq!(d(&root_off, true), Decision::Present([0.0; 3]));
        assert_eq!(d(&root_on, true), Decision::Absent);
        // The live world reads it so only with the switch: the wall of
        // reference 1 stays met without it; with it, the run's enable of
        // the root takes it out and its disable puts it back.
        let b = Budget::new(1 << 24);
        for on in [false, true] {
            let mut l = live().with_child_flag_agrees(on);
            assert_eq!(l.child_flag_agrees(), on);
            l.info.insert(key(1), i.clone());
            l.apply(&state(&[(9, Some(true), true)], &[]), &b).unwrap();
            let met = ray(&l).is_some();
            assert_eq!(met, !on, "{on}");
            l.apply(&state(&[(9, Some(false), false)], &[]), &b)
                .unwrap();
            assert_eq!(ray(&l), Some((100.0, Some(key(1)))), "{on}");
        }
    }
}
