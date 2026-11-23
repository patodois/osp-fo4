//! Moving platforms in the play mode (`--play-elevator REF`, `spec/play.md`,
//! The elevator ride): the collision of the references the caller names,
//! placed as stored beside the walking world
//! (`walk_animated_model_collision_as_stored_v1`), split by the model's
//! nodes that a controller sequence moves; each moving part follows the
//! translation keys of the sequence the run's animation graph starts on
//! the reference, from the second the run starts it
//! (`play_elevator_run_sequence_v1`), and carries a walker standing on it
//! (`walk_platform_sequence_ride_v1`). Before the run starts any sequence a
//! part stands at its stored transform; when a sequence does not move a
//! part, the part keeps the place the last one left it at. With
//! `--play-elevator-door`, when a ride that carried the walker ends, the
//! load door placed nearest the walker's feet among those placed inside the
//! named references' collision box (grown by the activation's reach) takes
//! the walker through (`play_elevator_end_load_door_v1`).
//!
//! The parts are pieces of the walking world's [`WorldSet`] under the key
//! kind [`KIND`]; they are put in again whenever the walking world is built
//! again (a load door, the moving grid). Only translations are read: a
//! sequence's rotation and scale keys are not applied.
use crate::live::Key;
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{AssetKind, DecodedAsset, Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::{platform::Ride, Character, World, WorldSet};
use std::collections::BTreeMap;

/// The key kind of the platforms' pieces in the walking world.
pub const KIND: u8 = 3;
/// Points a second of a sequence's translation is sampled at (a numerical
/// setting: the keys are joined by straight moves between samples).
pub const SAMPLES_PER_SECOND: f64 = 60.0;
/// Pieces one named reference may use in the key space.
const STRIDE: u32 = 10_000;

type E = Box<dyn std::error::Error>;
/// (second, translation) points.
pub type Track = Vec<(f64, [f64; 3])>;

/// One node of the model a sequence moves and some collision hangs under.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    /// The translation each sequence gives it, in world axes, from its
    /// stored translation: (seconds from the sequence's start, offset).
    pub tracks: BTreeMap<String, Track>,
    /// Where it stands now: absolute seconds.
    track: Track,
}

/// One moving part: the collision sources under the same moving nodes.
#[derive(Debug, Clone)]
struct Group {
    nodes: Vec<usize>,
    base: World,
}

/// One named reference's collision and rides.
#[derive(Debug)]
pub struct Elevator {
    pub reference: FormKey,
    /// The space the reference is placed in (its interior cell or its
    /// worldspace): its pieces go only into a walking world of that space.
    pub space: FormKey,
    index: u32,
    stills: World,
    also: Vec<World>,
    pub nodes: Vec<Node>,
    groups: Vec<Group>,
    rides: Vec<Ride<Key>>,
    /// The run's started sequences read so far.
    seen: usize,
    /// The last second the rides were advanced to.
    last: f64,
    /// What happened: (second, text).
    pub log: Vec<(f64, String)>,
    /// A ride that carried the walker ended at this second (taken by the
    /// walker for the end door).
    pub ended: Option<f64>,
}

fn sample(track: &Track, t: f64) -> [f64; 3] {
    let (Some(first), Some(last)) = (track.first(), track.last()) else {
        return [0.0; 3];
    };
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for w in track.windows(2) {
        let (a, z) = (w[0], w[1]);
        if t < z.0 {
            let k = if z.0 > a.0 {
                (t - a.0) / (z.0 - a.0)
            } else {
                1.0
            };
            return std::array::from_fn(|i| a.1[i] + k * (z.1[i] - a.1[i]));
        }
    }
    last.1
}

impl Elevator {
    /// Builds the collision of `reference` as stored and of the `also`
    /// references (still), and reads which nodes the model's sequences
    /// move. `index` keeps the pieces of several references apart.
    pub fn build(
        m: &Manifest,
        store: &Store,
        vfs: &Vfs,
        reference: &FormKey,
        also: &[FormKey],
        index: u32,
    ) -> Result<Elevator, E> {
        let level = RuleLevel::Hypotheses;
        let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
        let mut loader = Loader::new(store, vfs, &m.language, level, &budget);
        let options = fo4_scene::collision::Options {
            level,
            body_placement: None,
        };
        let inst = loader.instance(reference, &budget)?;
        let ec =
            fo4_scene::collision::instance_as_stored(&inst, &loader, &m.limits, options, &budget)?;
        let placement = ec
            .body_placement
            .ok_or("PlayElevator: no body placement level")?;
        let ew = World::build(&ec, placement, &budget)?;
        let model = inst
            .base
            .as_ref()
            .and_then(|b| b.common.model.path.clone())
            .ok_or("PlayElevator: the reference's base has no model")?;
        let place = inst
            .reference
            .data
            .transform
            .ok_or("PlayElevator: the reference has no placement")?;
        let frame =
            fo4_scene::transform::Transform::euler(place, inst.reference.data.scale.unwrap_or(1.))?;
        let want = format!("meshes/{}", model.replace('\\', "/")).to_ascii_lowercase();
        let id = inst
            .assets
            .iter()
            .copied()
            .find(|&i| {
                let a = &loader.assets.nodes[i];
                a.kind == AssetKind::Model
                    && String::from_utf8_lossy(&a.path)
                        .replace('\\', "/")
                        .to_ascii_lowercase()
                        == want
            })
            .ok_or("PlayElevator: the reference's model is not among its assets")?;
        let origin = frame.apply([0.0; 3]);
        // (node name, block, tracks by sequence) of every node a sequence
        // moves by translation keys.
        let read = loader.assets.with_decoded(id, &budget.child(), |d| {
            let DecodedAsset::Model(n) = d else {
                return Ok(Vec::new());
            };
            let seqs = fo4_scene::sequence::sequences(&n);
            let mut out: Vec<(String, usize, BTreeMap<String, Track>)> = Vec::new();
            for s in &seqs {
                let span = if s.frequency > 0.0 {
                    f64::from(s.stop - s.start) / f64::from(s.frequency)
                } else {
                    continue;
                };
                for (name, _) in &s.blocks {
                    let Some((block, stored)) = fo4_scene::sequence::node(&n, name) else {
                        continue;
                    };
                    let Some(keys) = fo4_scene::sequence::node_keys(&n, s, name, &budget.child())?
                    else {
                        continue;
                    };
                    let steps = (span * SAMPLES_PER_SECOND).ceil().max(1.0) as usize;
                    let mut track = Vec::new();
                    for k in 0..=steps {
                        let dt = (k as f64 / SAMPLES_PER_SECOND).min(span);
                        let st = f64::from(s.start) + dt * f64::from(s.frequency);
                        let Some(v) = keys.translation.sample(st) else {
                            track.clear();
                            break;
                        };
                        let local: [f64; 3] = std::array::from_fn(|i| f64::from(v[i] - stored[i]));
                        let w = frame.apply(local);
                        track.push((dt, std::array::from_fn(|i| w[i] - origin[i])));
                    }
                    if track.is_empty() {
                        continue;
                    }
                    match out.iter_mut().find(|x| x.0 == *name) {
                        Some(x) => {
                            x.2.insert(s.name.clone(), track);
                        }
                        None => out.push((
                            name.clone(),
                            block,
                            BTreeMap::from([(s.name.clone(), track)]),
                        )),
                    }
                }
            }
            Ok(out)
        })?;
        // A source goes with the moving nodes of its body's chain.
        let mut by_nodes: BTreeMap<Vec<usize>, Vec<u32>> = BTreeMap::new();
        for (si, src) in ew.sources.iter().enumerate() {
            let members: Vec<usize> = src
                .instance
                .map(|k| {
                    let i = &ec.instances[k];
                    let chain = &ec.model(i).bodies[i.body].chain;
                    read.iter()
                        .enumerate()
                        .filter(|(_, x)| chain.contains(&x.1))
                        .map(|(n, _)| n)
                        .collect()
                })
                .unwrap_or_default();
            by_nodes.entry(members).or_default().push(si as u32);
        }
        let stills = ew.copy_sources(
            by_nodes.get(&Vec::new()).map(Vec::as_slice).unwrap_or(&[]),
            [0.0; 3],
            &budget,
        )?;
        let mut groups = Vec::new();
        for (nodes, srcs) in &by_nodes {
            if nodes.is_empty() {
                continue;
            }
            groups.push(Group {
                nodes: nodes.clone(),
                base: ew.copy_sources(srcs, [0.0; 3], &budget)?,
            });
        }
        let nodes = read
            .into_iter()
            .map(|(name, _, tracks)| Node {
                name,
                tracks,
                track: vec![(0.0, [0.0; 3])],
            })
            .collect();
        let mut also_worlds = Vec::new();
        for k in also {
            let i = loader.instance(k, &budget)?;
            let c =
                fo4_scene::collision::instance_as_stored(&i, &loader, &m.limits, options, &budget)?;
            let p = c
                .body_placement
                .ok_or("PlayElevator: no body placement level")?;
            also_worlds.push(World::build(&c, p, &budget)?);
        }
        let (space, _) = fo4_vm::track::stored_position(store, m.limits.max_memory, reference)
            .map_err(|e| format!("PlayElevator: {e}"))?;
        Ok(Elevator {
            reference: reference.clone(),
            space: fo4_scene::timeline::norm(&space),
            index,
            stills,
            also: also_worlds,
            nodes,
            groups,
            rides: Vec::new(),
            seen: 0,
            last: 0.0,
            log: Vec::new(),
            ended: None,
        })
    }

    fn key(&self, n: u32) -> Key {
        (KIND, self.index * STRIDE + n)
    }

    /// The moving parts and the sources under them, for the report.
    pub fn summary(&self) -> String {
        format!(
            "{} still sources, {} moving parts ({} sources) under {} moved nodes, {} other references still",
            self.stills.sources.len(),
            self.groups.len(),
            self.groups.iter().map(|g| g.base.sources.len()).sum::<usize>(),
            self.nodes.len(),
            self.also.len()
        )
    }

    fn group_track(&self, g: &Group) -> Track {
        let mut times: Vec<f64> = g
            .nodes
            .iter()
            .flat_map(|n| self.nodes[*n].track.iter().map(|p| p.0))
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        times
            .into_iter()
            .map(|t| {
                let mut v = [0.0; 3];
                for n in &g.nodes {
                    let o = sample(&self.nodes[*n].track, t);
                    for i in 0..3 {
                        v[i] += o[i];
                    }
                }
                (t, v)
            })
            .collect()
    }

    /// Puts the pieces into a walking world at their place at `second`
    /// (again after the walking world was built anew).
    pub fn insert(&mut self, set: &mut WorldSet<Key>, second: f64) -> Result<(), E> {
        let b = Budget::new(1 << 32);
        set.insert(self.key(0), self.stills.clone())?;
        for (k, w) in self.also.iter().enumerate() {
            set.insert(self.key(STRIDE / 2 + k as u32), w.clone())?;
        }
        self.rides.clear();
        for gi in 0..self.groups.len() {
            let track = self.group_track(&self.groups[gi]);
            let base = self.groups[gi].base.clone();
            let r = Ride::new(set, self.key(1 + gi as u32), base, track, second, &b)?;
            self.rides.push(r);
        }
        self.last = second;
        Ok(())
    }

    /// Leaves the walking world (another space was loaded): the rides end
    /// where they stand; [`Elevator::insert`] puts the pieces in again.
    pub fn leave(&mut self) {
        self.rides.clear();
    }

    /// Reads the sequences the run's graph started on the reference since
    /// the last look: each moves the nodes it holds translation keys for
    /// from the second it started (`play_elevator_run_sequence_v1`).
    /// Returns whether one was read.
    pub fn follow(&mut self, vm: &fo4_vm::Vm<'_>) -> bool {
        let key = fo4_vm::bind::record_key(&self.reference);
        let mut any = false;
        let started = &vm.world.anims.started;
        for (t, o, name) in started.iter().skip(self.seen) {
            if vm.world.objects[*o].key.as_deref() != Some(key.as_str()) {
                continue;
            }
            any = true;
            let mut moved = 0;
            for n in &mut self.nodes {
                let here = sample(&n.track, *t);
                n.track = match n.tracks.get(name) {
                    Some(tr) => {
                        moved += 1;
                        tr.iter().map(|(dt, v)| (t + dt, *v)).collect()
                    }
                    None => vec![(*t, here)],
                };
            }
            self.log.push((
                *t,
                format!("the run's graph starts sequence {name}: {moved} moving node(s) keyed"),
            ));
        }
        self.seen = started.len();
        if any {
            for gi in 0..self.groups.len() {
                let track = self.group_track(&self.groups[gi]);
                if let Some(r) = self.rides.get_mut(gi) {
                    r.track = track;
                }
            }
        }
        any
    }

    /// Whether a part's track runs (a sequence plays on it) between the
    /// two seconds: the walker ticks through it, so a ride's end is seen.
    pub fn moving(&self, from: f64, to: f64) -> bool {
        self.rides
            .iter()
            .any(|r| match (r.track.first(), r.track.last()) {
                (Some(a), Some(z)) => r.track.len() > 1 && z.0 > from && a.0 < to,
                _ => false,
            })
    }

    /// Moves the parts to their place at `second`, carrying the walker
    /// standing on one; notes a ride that carried the walker ending.
    pub fn advance(
        &mut self,
        set: &mut WorldSet<Key>,
        walker: &mut Character,
        second: f64,
    ) -> Result<f64, E> {
        let b = Budget::new(1 << 32);
        let mut carried = 0.0;
        for r in &mut self.rides {
            carried += r.advance(set, walker, second, &b)?;
            let end = r.track.last().map_or(0.0, |p| p.0);
            let on = walker.grounded
                && walker
                    .ground
                    .and_then(|g| set.source(g.source))
                    .is_some_and(|(k, _)| *k == r.key);
            if self.last < end && second >= end && on && r.carried != 0.0 {
                self.ended = Some(end);
                self.log
                    .push((end, "a ride that carried the walker ended".into()));
            }
        }
        self.last = second;
        Ok(carried)
    }
}

impl Elevator {
    /// The box of the reference's collision as stored, the moving parts
    /// at their stored transforms and the still references with it.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut out: Option<([f64; 3], [f64; 3])> = None;
        let worlds = std::iter::once(&self.stills)
            .chain(self.groups.iter().map(|g| &g.base))
            .chain(&self.also);
        for w in worlds {
            let Some(b) = w.bounds else { continue };
            out = Some(match out {
                None => (b.min, b.max),
                Some((lo, hi)) => (
                    std::array::from_fn(|i| lo[i].min(b.min[i])),
                    std::array::from_fn(|i| hi[i].max(b.max[i])),
                ),
            });
        }
        out
    }
}

/// The load door nearest `feet` among `doors` whose stored position in
/// `space` lies inside `bounds` grown by `reach` on every side
/// (`play_elevator_end_load_door_v1`): (door, its teleport, distance).
pub fn nearest_door<'d>(
    store: &Store,
    max: u64,
    doors: &'d BTreeMap<FormKey, crate::world::LoadDoor>,
    space: &FormKey,
    feet: [f64; 3],
    bounds: ([f64; 3], [f64; 3]),
    reach: f64,
) -> Option<(&'d FormKey, &'d crate::world::LoadDoor, f64)> {
    let mut best: Option<(&FormKey, &crate::world::LoadDoor, f64)> = None;
    let (lo, hi) = bounds;
    for (k, d) in doors {
        let Ok((s, p)) = fo4_vm::track::stored_position(store, max, k) else {
            continue;
        };
        if fo4_vm::bind::record_key(&s) != fo4_vm::bind::record_key(space) {
            continue;
        }
        if (0..3).any(|i| p[i] < lo[i] - reach || p[i] > hi[i] + reach) {
            continue;
        }
        let dist = (0..3).map(|i| (p[i] - feet[i]).powi(2)).sum::<f64>().sqrt();
        if best.is_none_or(|x| dist < x.2) {
            best = Some((k, d, dist));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_walk::{Params, Settings};

    fn key(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    fn floor(z: f64, half: f64) -> World {
        let b = Budget::new(1 << 24);
        let mut w = World::new();
        w.add_triangles(
            &[
                [[-half, -half, z], [half, -half, z], [half, half, z]],
                [[-half, -half, z], [half, half, z], [-half, half, z]],
            ],
            "floor",
            &b,
        )
        .unwrap();
        w.rebuild(256.0, &b).unwrap();
        w
    }

    /// A platform (one moving part under one node) that a sequence named
    /// `Down` lowers 100 units over one second.
    fn elevator() -> Elevator {
        let down: Track = (0..=60)
            .map(|k| (k as f64 / 60.0, [0.0, 0.0, -100.0 * k as f64 / 60.0]))
            .collect();
        Elevator {
            reference: key(5),
            space: key(1),
            index: 0,
            stills: World::new(),
            also: Vec::new(),
            nodes: vec![Node {
                name: "platform".into(),
                tracks: BTreeMap::from([("Down".to_string(), down)]),
                track: vec![(0.0, [0.0; 3])],
            }],
            groups: vec![Group {
                nodes: vec![0],
                base: floor(0.0, 100.0),
            }],
            rides: Vec::new(),
            seen: 0,
            last: 0.0,
            log: Vec::new(),
            ended: None,
        }
    }

    #[test]
    fn a_platform_follows_the_sequence_the_run_starts_and_carries_the_walker() {
        let mut lib = fo4_vm::MapLibrary::default();
        let mut host = fo4_vm::NoHost;
        let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
        let o = vm.world.object(
            "the platform",
            Some(fo4_vm::bind::record_key(&key(5))),
            &["objectreference"],
        );
        let other = vm.world.object("other", Some("a.esm:00000009".into()), &[]);
        let mut e = elevator();
        let mut set: WorldSet<Key> = WorldSet::new();
        e.insert(&mut set, 0.0).unwrap();
        let mut c =
            Character::new(Params::PROVISIONAL, Settings::DEFAULT, [0.0, 0.0, 5.0]).unwrap();
        c.settle(&set, 600);
        assert!(c.grounded && c.feet[2].abs() < 1e-3);
        // Nothing started: the part stays at its stored place.
        e.advance(&mut set, &mut c, 1.0).unwrap();
        assert!(!e.moving(0.0, 10.0));
        // Another reference's sequence and an unknown name move nothing.
        vm.world.anims.started.push((1.0, other, "Down".into()));
        vm.world.anims.started.push((1.0, o, "Up".into()));
        assert!(e.follow(&vm));
        assert!(!e.moving(0.0, 10.0));
        // The run starts the descent at 2 s.
        vm.world.anims.started.push((2.0, o, "Down".into()));
        assert!(e.follow(&vm));
        assert!(e.moving(1.5, 2.5) && !e.moving(3.5, 9.0));
        let tick = Settings::DEFAULT.tick;
        let mut t = 1.0;
        while t < 2.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        assert!((c.feet[2] + 50.0).abs() < 3.0, "{:?}", c.feet);
        assert!(e.ended.is_none());
        while t < 3.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        assert!((c.feet[2] + 100.0).abs() < 1e-3, "{:?}", c.feet);
        assert_eq!(e.ended, Some(3.0));
        // Built again (another walking world of the same space): the part
        // stands where the ride left it.
        let mut again: WorldSet<Key> = WorldSet::new();
        e.insert(&mut again, t).unwrap();
        let bounds = again.get(&e.key(1)).unwrap().bounds.unwrap();
        assert!((bounds.max[2] + 100.0).abs() < 1.0, "{bounds:?}");
    }

    #[test]
    fn tracks_are_sampled_straight_and_the_box_holds_every_part() {
        assert!(sample(&Vec::new(), 1.0) == [0.0; 3]);
        let tr: Track = vec![(1.0, [0.0; 3]), (3.0, [0.0, 0.0, 10.0])];
        assert_eq!(sample(&tr, 0.0), [0.0; 3]);
        assert_eq!(sample(&tr, 2.0), [0.0, 0.0, 5.0]);
        assert_eq!(sample(&tr, 9.0), [0.0, 0.0, 10.0]);
        let e = elevator();
        let b = e.bounds().unwrap();
        assert!(b.0[0] <= -100.0 + 1e-6 && b.1[0] >= 100.0 - 1e-6);
    }
}
