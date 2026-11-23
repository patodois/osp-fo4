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
    /// Reads what the run did since the last call; whether anything
    /// changed.
    pub fn update(&mut self, vm: &Vm<'_>) -> bool {
        let mut changed = false;
        for e in vm.effects.iter().skip(self.effects) {
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
/// the walking world.
pub fn decide(
    info: &RefInfo,
    key: &FormKey,
    shown: &dyn Fn(&FormKey) -> Option<bool>,
    mv: Option<&Move>,
    space: &FormKey,
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
}

/// The walking world with the run's state applied.
#[derive(Debug, Clone)]
pub struct Live {
    set: WorldSet<Key>,
    refs: BTreeMap<Key, Vec<Option<FormKey>>>,
    info: BTreeMap<FormKey, RefInfo>,
    space: FormKey,
    nowait: bool,
    decided: BTreeMap<FormKey, Decision>,
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
            decided: BTreeMap::new(),
            stats: Stats::default(),
        })
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
        let shown = |k: &FormKey| s.shown(k, nowait);
        let mut changed = Vec::new();
        for (k, i) in &self.info {
            let d = decide(i, k, &shown, s.moves.get(k).map(|(_, m)| m), &self.space);
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
        let took = started.elapsed().as_secs_f64();
        self.stats.applies += 1;
        self.stats.seconds += took;
        self.stats.longest = self.stats.longest.max(took);
        Ok(changed.len())
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
        let mut best: Option<pick::Hit> = None;
        for (k, w, _) in self.set.pieces() {
            let refs = self.refs.get(k).map(Vec::as_slice).unwrap_or_default();
            let keep = |s: u32| !self.set.is_hidden(k, s);
            let hit = pick::first_where(w, refs, None, eye, dir, reach, &keep, skip);
            if let Some(h) = hit {
                if best.as_ref().is_none_or(|x| h.distance < x.distance) {
                    best = Some(h);
                }
            }
        }
        let boxed = pick::first_where(&World::new(), &[], boxes, eye, dir, reach, &|_| true, skip);
        match (best, boxed) {
            (Some(h), Some(x)) if h.distance <= x.distance => Some(h),
            (_, Some(x)) => Some(x),
            (h, None) => h,
        }
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
        let off = decide(&i, &key(1), &shown_off, None, &key(100));
        let on = decide(&i, &key(1), &shown_on, None, &key(100));
        assert_eq!(on, Decision::Present([0.0; 3]));
        assert_eq!(off, Decision::Absent, "{:?}", i.enable);
    }
}
