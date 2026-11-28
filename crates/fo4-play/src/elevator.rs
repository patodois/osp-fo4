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
//! sequence's rotation and scale keys are not applied. With
//! `--play-elevator-held-pose` a node a sequence controls without
//! translation keys moves to the translation the sequence's interpolator
//! holds it at ([`Source::Held`], `play_elevator_held_pose_v1`).
//!
//! With `--play-elevator-rigid` ([`Readings::rigid`],
//! `play_elevator_rigid_v1`) the translations are not read: the sources go
//! by the deepest node of their body's chain that any of the model's
//! sequences sets ([`Part`]), and each part moves by the rigid motion its
//! chain makes under the sequence the run starts, every channel of each
//! node the sequence sets read (`fo4_scene::sequence::chain_pose`,
//! `nif_sequence_chain_pose_v1`), riding as a `platform::RigidRide`
//! (`walk_platform_rigid_motion_v1`).
use crate::live::Key;
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{AssetKind, DecodedAsset, Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::{
    platform::{Ride, RigidRide},
    Character, Motion, World, WorldSet,
};
use std::collections::BTreeMap;

/// The key kind of the platforms' pieces in the walking world.
pub const KIND: u8 = 3;
/// Points a second of a sequence's translation is sampled at (a numerical
/// setting: the keys are joined by straight moves between samples).
pub const SAMPLES_PER_SECOND: f64 = 60.0;
/// Pieces one named reference may use in the key space.
const STRIDE: u32 = 10_000;
/// The most samples one sequence's track is read with (about 18 minutes
/// at [`SAMPLES_PER_SECOND`]); a longer or infinite span is a named error.
pub const MAX_SAMPLES: usize = 1 << 16;

/// The steps a span of `span` seconds is sampled in, as many as
/// [`SAMPLES_PER_SECOND`] gives (at least one); a span that needs more
/// than [`MAX_SAMPLES`] is an error naming the sequence.
fn sample_steps(s: &fo4_scene::sequence::Sequence, span: f64) -> fo4_core::Result<usize> {
    let n = (span * SAMPLES_PER_SECOND).ceil().max(1.0);
    // `n` is at least 1 (NaN reads as 1 through `max`); infinity is past.
    if n > MAX_SAMPLES as f64 {
        return Err(fo4_core::Error::new(
            "SequenceSpanTooLong",
            &s.name,
            0,
            format!("a span of at most {MAX_SAMPLES} samples"),
            format!("{span} s"),
        ));
    }
    Ok(n as usize)
}

type E = Box<dyn std::error::Error>;
/// (second, translation) points.
pub type Track = Vec<(f64, [f64; 3])>;

/// Which named reference a still reference (`--play-elevator-also`) goes
/// with: the first whose space is the still reference's own, else the
/// first (index 0). `spaces`: the named references' spaces, in order.
pub fn still_owner(spaces: &[Option<FormKey>], space: &Option<FormKey>) -> usize {
    spaces
        .iter()
        .position(|x| x.is_some() && x == space)
        .unwrap_or(0)
}

/// One node of the model a sequence moves and some collision hangs under.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    /// Its block in the reference's model.
    pub block: usize,
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

/// (second, motion) points of a part under the rigid reading.
pub type MotionTrack = Vec<(f64, Motion)>;

/// What a named reference's parts follow ([`Elevator::build`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readings {
    /// A node a sequence controls without translation keys moves to the
    /// translation the sequence holds it at (`play_elevator_held_pose_v1`,
    /// [`Source::of`]); read only without [`Readings::rigid`].
    pub held_pose: bool,
    /// The parts move by the rigid motion their chain makes under each
    /// sequence ([`Part`], `play_elevator_rigid_v1`), in place of the
    /// translation reading.
    pub rigid: bool,
}

/// One moving part under the rigid reading (`play_elevator_rigid_v1`): the
/// collision sources whose body chains have the same deepest node that any
/// of the model's sequences sets through a transform interpolator.
#[derive(Debug, Clone)]
pub struct Part {
    /// The deepest set node (a block index of the model).
    pub block: usize,
    /// The motion each sequence gives the part from its stored place, in
    /// world axes: (seconds from the sequence's start, motion); a sequence
    /// that sets no node of the part's chain has none.
    pub tracks: BTreeMap<String, MotionTrack>,
    /// Where it stands now: absolute seconds.
    track: MotionTrack,
    base: World,
}

impl Part {
    /// The part's collision as stored (read-only, for reports).
    pub fn base(&self) -> &World {
        &self.base
    }
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
    /// The second the run last started a sequence on the reference.
    started_at: Option<f64>,
    /// The last second the rides were advanced to.
    last: f64,
    /// What happened: (second, text).
    pub log: Vec<(f64, String)>,
    /// A ride that carried the walker ended at this second (taken by the
    /// walker for the end door).
    pub ended: Option<f64>,
    /// `play_elevator_end_landing_v1` (`--play-elevator-door-landing`): a
    /// ride that carried the walker and ended while the walker was in the
    /// air ends for the walker when it next stands on the ride's piece.
    pub landing: bool,
    /// `walk_ride_rider_passes_terrain_v1` (`--play-elevator-through-terrain`):
    /// while the walker stands on one of the rides' pieces, the walking
    /// world's terrain sources are hidden ([`Ride::pass_terrain`]).
    pub pass_terrain: bool,
    /// Under [`Elevator::landing`]: the rides that ended while the walker
    /// was in the air (their end second, their piece's key).
    pending: Vec<(f64, Key)>,
    /// The (node, sequence) tracks read from a held translation
    /// (`play_elevator_held_pose_v1`); 0 without it.
    pub held: usize,
    /// The parts under the rigid reading (`play_elevator_rigid_v1`); empty
    /// without it.
    pub parts: Vec<Part>,
    rigid_rides: Vec<RigidRide<Key>>,
    /// The reference's model, as its asset path reads (`meshes/...`, lower
    /// case, `/`), and the blocks the drawing reads parts by
    /// ([`Platform::set`]).
    pub model: String,
    set: Vec<usize>,
}

/// Where a moving node's translation comes from while a sequence plays.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The sequence's translation keys for the node
    /// (`play_elevator_run_sequence_v1`).
    Keys(fo4_scene::clock::Keys),
    /// The translation the sequence's interpolator holds the node at, for
    /// a node it controls without translation keys
    /// (`play_elevator_held_pose_v1` over the drawing side's
    /// `nif_sequence_interpolator_held_pose_v1`,
    /// `fo4_scene::sequence::held_pose`).
    Held([f32; 3]),
}

impl Source {
    /// The source of a node's translation in a sequence: its translation
    /// keys when the sequence's data holds some; else the held translation
    /// when the caller read one (only under `play_elevator_held_pose_v1`);
    /// else the data's empty key group, which gives no track, as before
    /// the held reading; `None` when the sequence links no data for it.
    pub fn of(keys: Option<fo4_scene::clock::NodeKeys>, held: Option<[f32; 3]>) -> Option<Source> {
        match (keys, held) {
            (Some(k), _) if !k.translation.keys.is_empty() => Some(Source::Keys(k.translation)),
            (_, Some(h)) => Some(Source::Held(h)),
            (Some(k), None) => Some(Source::Keys(k.translation)),
            (None, None) => None,
        }
    }

    /// The node's translation at key time `t` (the model's own axes).
    pub fn sample(&self, t: f64) -> Option<[f32; 3]> {
        match self {
            Source::Keys(k) => k.sample(t),
            Source::Held(h) => Some(*h),
        }
    }

    /// The node's offset from its stored translation over the sequence's
    /// span (seconds), sampled [`SAMPLES_PER_SECOND`] times a second, in
    /// world axes (turned and scaled by the placement `frame`, whose
    /// `origin` is taken off): (seconds from the sequence's start, offset).
    /// Empty when a sample has no value; an error when the span needs more
    /// than [`MAX_SAMPLES`].
    pub fn track(
        &self,
        s: &fo4_scene::sequence::Sequence,
        span: f64,
        stored: [f32; 3],
        frame: fo4_scene::transform::Transform,
        origin: [f64; 3],
    ) -> fo4_core::Result<Track> {
        let steps = sample_steps(s, span)?;
        let mut track = Vec::new();
        for k in 0..=steps {
            let dt = (k as f64 / SAMPLES_PER_SECOND).min(span);
            let st = f64::from(s.start) + dt * f64::from(s.frequency);
            let Some(v) = self.sample(st) else {
                return Ok(Vec::new());
            };
            let local: [f64; 3] = std::array::from_fn(|i| f64::from(v[i] - stored[i]));
            let w = frame.apply(local);
            track.push((dt, std::array::from_fn(|i| w[i] - origin[i])));
        }
        Ok(track)
    }
}

/// A rigid track's motion at `t` (absolute or relative seconds, as the
/// track's): the first before the first point, the last after the last,
/// between two points [`Motion::lerp`]; the identity for an empty track.
pub fn motion_at(track: &[(f64, Motion)], t: f64) -> Motion {
    let (Some(first), Some(last)) = (track.first(), track.last()) else {
        return Motion::IDENTITY;
    };
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for w in track.windows(2) {
        let (a, z) = (&w[0], &w[1]);
        if t < z.0 {
            let k = if z.0 > a.0 {
                (t - a.0) / (z.0 - a.0)
            } else {
                1.0
            };
            return Motion::lerp(&a.1, &z.1, k);
        }
    }
    last.1
}

/// Below this a key's motion is the one before it (world units, and the
/// quaternion's words): a sequence that holds its pose repeats its keys.
const SAME_MOTION: f64 = 1e-6;

/// The second of the last key of a rigid `track` whose motion differs from
/// the key before it: the part moves until then and stands after it.
/// `None` when every key holds the first key's motion.
pub fn last_change(track: &[(f64, Motion)]) -> Option<f64> {
    // Two motions are one when they turn alike and move the origin alike
    // (the pivot is a way of writing the same motion).
    let differ = |a: &Motion, b: &Motion| {
        let (oa, ob) = (a.apply([0.0; 3]), b.apply([0.0; 3]));
        (0..4).any(|i| (a.rotation[i] - b.rotation[i]).abs() > SAME_MOTION)
            || (0..3).any(|i| (oa[i] - ob[i]).abs() > SAME_MOTION)
    };
    track
        .windows(2)
        .rev()
        .find(|w| differ(&w[0].1, &w[1].1))
        .map(|w| w[1].0)
}

/// As [`last_change`] for a translation track.
pub fn last_translation_change(track: &[(f64, [f64; 3])]) -> Option<f64> {
    track
        .windows(2)
        .rev()
        .find(|w| (0..3).any(|i| (w[0].1[i] - w[1].1[i]).abs() > SAME_MOTION))
        .map(|w| w[1].0)
}

/// The world box of `stored` (a box in world axes) moved by `m`: its eight
/// corners moved and boxed again.
fn moved_box(stored: ([f64; 3], [f64; 3]), m: &Motion) -> ([f64; 3], [f64; 3]) {
    let (lo, hi) = stored;
    let mut out: Option<([f64; 3], [f64; 3])> = None;
    for k in 0..8 {
        let c: [f64; 3] = std::array::from_fn(|i| if k & (1 << i) != 0 { hi[i] } else { lo[i] });
        let p = m.apply(c);
        out = Some(match out {
            None => (p, p),
            Some((a, b)) => (
                std::array::from_fn(|i| a[i].min(p[i])),
                std::array::from_fn(|i| b[i].max(p[i])),
            ),
        });
    }
    out.unwrap_or(stored)
}

/// The parts of the rigid reading (`play_elevator_rigid_v1`) of the model
/// `d`: each source of `ew` built from a collision instance of `ec` goes
/// with the deepest node of its body's chain that any of the model's
/// sequences sets (`fo4_scene::sequence::chain_pose` at each sequence's
/// start). Each part has, per sequence, the rigid motions the sequence
/// gives its chain up to that node over the sequence's span, sampled
/// [`SAMPLES_PER_SECOND`] times a second: each the motion between the
/// chain as stored and as the sequence poses it at that key time, both
/// under the placement `frame` (none for a sequence that sets no node of
/// the chain or poses a sample as no rigid motion). Returns (deepest node,
/// sources, tracks) per part, in the order of their first source, the
/// still sources (no collision instance, or a chain no sequence sets), and
/// every node block of the model that some sequence sets through a
/// transform interpolator (a root whose own transform a collision chain
/// leaves out, `nif_root_transform_replaced_v1`, not among them), for the
/// drawing ([`Platform::set`]).
#[allow(clippy::type_complexity)]
fn rigid_parts(
    d: DecodedAsset<'_>,
    ec: &fo4_scene::collision::CellCollision,
    ew: &World,
    frame: fo4_scene::transform::Transform,
    b: &Budget,
) -> fo4_core::Result<(
    Vec<(usize, Vec<u32>, BTreeMap<String, MotionTrack>)>,
    Vec<u32>,
    Vec<usize>,
)> {
    let DecodedAsset::Model(n) = d else {
        return Ok((
            Vec::new(),
            (0..ew.sources.len() as u32).collect(),
            Vec::new(),
        ));
    };
    let seqs = fo4_scene::sequence::sequences(&n);
    let track_of = |s: &fo4_scene::sequence::Sequence,
                    prefix: &[usize],
                    rlo: bool|
     -> fo4_core::Result<MotionTrack> {
        if s.frequency.is_nan() || s.frequency <= 0.0 {
            return Ok(Vec::new());
        }
        let span = f64::from(s.stop - s.start) / f64::from(s.frequency);
        let steps = sample_steps(s, span)?;
        let mut track = Vec::new();
        for k in 0..=steps {
            let dt = (k as f64 / SAMPLES_PER_SECOND).min(span);
            let st = f64::from(s.start) + dt * f64::from(s.frequency);
            let Some(p) = fo4_scene::sequence::chain_pose(&n, s, prefix, rlo, st, &b.child())?
            else {
                return Ok(Vec::new());
            };
            let Some(m) = Motion::between(&frame.compose(p.stored), &frame.compose(p.posed)) else {
                return Ok(Vec::new());
            };
            track.push((dt, m));
        }
        Ok(track)
    };
    let mut parts: Vec<(usize, Vec<u32>, BTreeMap<String, MotionTrack>)> = Vec::new();
    let mut stills = Vec::new();
    for (si, src) in ew.sources.iter().enumerate() {
        let Some(k) = src.instance else {
            stills.push(si as u32);
            continue;
        };
        let inst = &ec.instances[k];
        let rlo = inst.rules.contains(&fo4_scene::hypotheses::ROOT_TRANSFORM);
        let chain = &ec.model(inst).bodies[inst.body].chain;
        let mut deepest: Option<usize> = None;
        for s in &seqs {
            let start = f64::from(s.start);
            if let Some(p) = fo4_scene::sequence::chain_pose(&n, s, chain, rlo, start, &b.child())?
            {
                let at = chain.iter().position(|x| *x == p.deepest).unwrap_or(0);
                if deepest.is_none_or(|d| at > d) {
                    deepest = Some(at);
                }
            }
        }
        let Some(at) = deepest else {
            stills.push(si as u32);
            continue;
        };
        let block = chain[at];
        if let Some(p) = parts.iter_mut().find(|p| p.0 == block) {
            p.1.push(si as u32);
            continue;
        }
        let mut tracks = BTreeMap::new();
        for s in &seqs {
            let t = track_of(s, &chain[..=at], rlo)?;
            if !t.is_empty() {
                tracks.insert(s.name.clone(), t);
            }
        }
        parts.push((block, vec![si as u32], tracks));
    }
    // The blocks some sequence sets: a chain of one block is posed when
    // the sequence sets that block (`chain_pose`'s own test).
    let mut set = Vec::new();
    for block in 0..n.blocks.len() {
        for s in &seqs {
            let start = f64::from(s.start);
            if fo4_scene::sequence::chain_pose(&n, s, &[block], false, start, &b.child())?.is_some()
            {
                set.push(block);
                break;
            }
        }
    }
    // A root the collision chains leave out of the pose is not a part's
    // node for the drawing either.
    for inst in &ec.instances {
        if inst.rules.contains(&fo4_scene::hypotheses::ROOT_TRANSFORM) {
            if let Some(root) = ec.model(inst).bodies[inst.body].chain.first() {
                set.retain(|x| x != root);
            }
        }
    }
    Ok((parts, stills, set))
}

/// A model path as the asset graph reads it (`meshes/...`, lower case,
/// `/`).
fn model_path(model: &str) -> String {
    format!("meshes/{}", model.replace('\\', "/")).to_ascii_lowercase()
}

/// The asset of the instance's model `want` ([`model_path`]).
fn model_asset(loader: &Loader<'_>, inst: &fo4_scene::Instance, want: &str) -> Option<usize> {
    inst.assets.iter().copied().find(|&i| {
        let a = &loader.assets.nodes[i];
        a.kind == AssetKind::Model
            && String::from_utf8_lossy(&a.path)
                .replace('\\', "/")
                .to_ascii_lowercase()
                == want
    })
}

/// A placed door's collision in its open pose
/// (`play_door_collision_open_pose_v1`): its model's collision placed as
/// stored (`fo4_scene::collision::instance_as_stored`), the sources whose
/// body chain no controller sequence of the model sets (the frame) where
/// they stand, and each part some sequence sets (the rigid reading's parts
/// of the named platforms, `play_elevator_rigid_v1`, grouped by the deepest
/// set node) moved by the rigid motion the sequence named `Open` gives its
/// chain at the sequence's last key (`nif_sequence_chain_pose_v1`), as
/// stored when that sequence sets no node of its chain.
#[derive(Debug, Clone)]
pub struct OpenDoor {
    /// The door's collision in the open pose: the still sources first,
    /// then each part's.
    pub world: World,
    /// How many parts some sequence moves, and how many of them `Open`
    /// moves.
    pub moving: usize,
    pub opened: usize,
    /// Each moving part as stored, with the rigid motions each sequence
    /// gives it by name (for reports).
    pub parts: Vec<(World, BTreeMap<String, MotionTrack>)>,
}

/// The name of the door sequence whose last key is the open pose (as
/// `fo4-view`'s held doors' drawing reads it, `play_door_drawn_as_held_v1`).
pub const OPEN_SEQUENCE: &str = "Open";

/// The collision of the door `inst` in its open pose ([`OpenDoor`]), when
/// its model's collision has a frame (a source no sequence sets) and its
/// sequence named [`OPEN_SEQUENCE`] moves at least one part of it: `None`
/// otherwise (the door is left out whole, as before; a door of one leaf
/// with no frame among them).
pub fn open_door_collision(
    loader: &Loader<'_>,
    inst: &fo4_scene::Instance,
    limits: &fo4_core::Limits,
    placement: fo4_scene::collision::BodyPlacement,
    b: &Budget,
) -> Result<Option<OpenDoor>, E> {
    let options = fo4_scene::collision::Options {
        level: RuleLevel::Hypotheses,
        body_placement: Some(placement),
    };
    let ec = fo4_scene::collision::instance_as_stored(inst, loader, limits, options, b)?;
    if ec.instances.is_empty() {
        return Ok(None);
    }
    let ew = World::build(&ec, placement, b)?;
    let Some(model) = inst.base.as_ref().and_then(|x| x.common.model.path.clone()) else {
        return Ok(None);
    };
    let Some(place) = inst.reference.data.transform else {
        return Ok(None);
    };
    let frame =
        fo4_scene::transform::Transform::euler(place, inst.reference.data.scale.unwrap_or(1.))?;
    let Some(id) = model_asset(loader, inst, &model_path(&model)) else {
        return Ok(None);
    };
    let (parts, still, _) = loader
        .assets
        .with_decoded(id, &b.child(), |d| rigid_parts(d, &ec, &ew, frame, b))?;
    let Some((world, opened)) = open_pose(&ew, &still, &parts, b)? else {
        return Ok(None);
    };
    let mut moving = Vec::new();
    for (_, srcs, tracks) in &parts {
        moving.push((ew.copy_sources(srcs, [0.0; 3], b)?, tracks.clone()));
    }
    Ok(Some(OpenDoor {
        world,
        moving: parts.len(),
        opened,
        parts: moving,
    }))
}

/// The open pose of a door's collision `ew` split into its `still`
/// sources and its moving `parts` (deepest set node, sources, rigid tracks
/// by sequence name): the still sources as stored, then each part moved by
/// the last motion of its [`OPEN_SEQUENCE`] track (as stored without one),
/// and how many parts that sequence moves. `None` without a still source
/// or a part the sequence moves.
#[allow(clippy::type_complexity)]
fn open_pose(
    ew: &World,
    still: &[u32],
    parts: &[(usize, Vec<u32>, BTreeMap<String, MotionTrack>)],
    b: &Budget,
) -> fo4_core::Result<Option<(World, usize)>> {
    let opened = parts
        .iter()
        .filter(|p| p.2.get(OPEN_SEQUENCE).is_some_and(|t| !t.is_empty()))
        .count();
    if opened == 0 || still.is_empty() {
        return Ok(None);
    }
    let mut world = ew.copy_sources(still, [0.0; 3], b)?;
    for (_, srcs, tracks) in parts {
        let last = tracks
            .get(OPEN_SEQUENCE)
            .and_then(|t| t.last())
            .map_or(Motion::IDENTITY, |x| x.1);
        let posed = ew.copy_sources_moved(srcs, &last, b)?;
        world.append(&posed, "open part", b)?;
    }
    Ok(Some((world, opened)))
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
    /// move. `index` keeps the pieces of several references apart. With
    /// [`Readings::held_pose`], a node a sequence controls without
    /// translation keys moves to the translation the sequence holds it at
    /// (`play_elevator_held_pose_v1`, [`Source::of`]); with
    /// [`Readings::rigid`] the parts are read as [`Part`]s instead
    /// (`play_elevator_rigid_v1`).
    pub fn build(
        m: &Manifest,
        store: &Store,
        vfs: &Vfs,
        reference: &FormKey,
        also: &[FormKey],
        index: u32,
        readings: Readings,
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
        let want = model_path(&model);
        let id = model_asset(&loader, &inst, &want)
            .ok_or("PlayElevator: the reference's model is not among its assets")?;
        if readings.rigid {
            let (read, still, set) = loader.assets.with_decoded(id, &budget.child(), |d| {
                rigid_parts(d, &ec, &ew, frame, &budget)
            })?;
            let mut parts = Vec::new();
            for (block, srcs, tracks) in read {
                parts.push(Part {
                    block,
                    tracks,
                    track: vec![(0.0, Motion::IDENTITY)],
                    base: ew.copy_sources(&srcs, [0.0; 3], &budget)?,
                });
            }
            let stills = ew.copy_sources(&still, [0.0; 3], &budget)?;
            return Self::finish(
                m,
                store,
                &mut loader,
                options,
                reference,
                also,
                index,
                stills,
                (Vec::new(), Vec::new(), 0),
                (parts, want, set),
                &budget,
            );
        }
        let origin = frame.apply([0.0; 3]);
        // (node name, block, tracks by sequence) of every node a sequence
        // moves by translation keys (or, with `held_pose`, holds).
        let held_pose = readings.held_pose;
        let mut held_tracks = 0;
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
                    let keys = fo4_scene::sequence::node_keys(&n, s, name, &budget.child())?;
                    let held = if held_pose {
                        fo4_scene::sequence::held_pose(&n, s, name, &budget.child())?
                            .and_then(|p| p.translation)
                    } else {
                        None
                    };
                    let Some(source) = Source::of(keys, held) else {
                        continue;
                    };
                    let track = source.track(s, span, stored, frame, origin)?;
                    if !track.is_empty() && matches!(source, Source::Held(_)) {
                        held_tracks += 1;
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
        let nodes: Vec<Node> = read
            .into_iter()
            .map(|(name, block, tracks)| Node {
                name,
                block,
                tracks,
                track: vec![(0.0, [0.0; 3])],
            })
            .collect();
        let set = {
            let mut v: Vec<usize> = nodes.iter().map(|n| n.block).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        Self::finish(
            m,
            store,
            &mut loader,
            options,
            reference,
            also,
            index,
            stills,
            (nodes, groups, held_tracks),
            (Vec::new(), want, set),
            &budget,
        )
    }

    /// The still references' collision and the reference's space, with
    /// the moving parts read by [`Elevator::build`].
    #[allow(clippy::too_many_arguments)]
    fn finish(
        m: &Manifest,
        store: &Store,
        loader: &mut Loader<'_>,
        options: fo4_scene::collision::Options,
        reference: &FormKey,
        also: &[FormKey],
        index: u32,
        stills: World,
        (nodes, groups, held): (Vec<Node>, Vec<Group>, usize),
        (parts, model, set): (Vec<Part>, String, Vec<usize>),
        budget: &Budget,
    ) -> Result<Elevator, E> {
        let mut also_worlds = Vec::new();
        for k in also {
            let i = loader.instance(k, budget)?;
            let c =
                fo4_scene::collision::instance_as_stored(&i, loader, &m.limits, options, budget)?;
            let p = c
                .body_placement
                .ok_or("PlayElevator: no body placement level")?;
            also_worlds.push(World::build(&c, p, budget)?);
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
            started_at: None,
            last: 0.0,
            log: Vec::new(),
            ended: None,
            landing: false,
            pass_terrain: false,
            pending: Vec::new(),
            held,
            parts,
            rigid_rides: Vec::new(),
            model,
            set,
        })
    }

    fn key(&self, n: u32) -> Key {
        (KIND, self.index * STRIDE + n)
    }

    /// What the piece under key `key` of this reference is, for a report
    /// (`--play-report-walls`): its still sources, a moving group, or a
    /// rigid part with the model block of its deepest set node; `None`
    /// when the key is not this reference's.
    pub fn describe_piece(&self, key: Key) -> Option<String> {
        if key.0 != KIND || key.1 / STRIDE != self.index {
            return None;
        }
        let n = key.1 % STRIDE;
        let first = 1 + self.groups.len() as u32;
        let what = if n == 0 {
            "still sources".to_string()
        } else if n < first {
            format!("moving group {}", n - 1)
        } else if let Some(p) = self.parts.get((n - first) as usize) {
            format!(
                "rigid part {} (deepest set node: model block {})",
                n - first,
                p.block
            )
        } else {
            format!("piece {n}")
        };
        Some(format!(
            "named platform {}: {what}",
            fo4_vm::bind::record_key(&self.reference)
        ))
    }

    /// The moving parts and the sources under them, for the report.
    pub fn summary(&self) -> String {
        if !self.parts.is_empty() {
            return format!(
                "{} still sources, {} rigid parts ({} sources) under {} set nodes, {} (part, sequence) motion tracks, {} other references still",
                self.stills.sources.len(),
                self.parts.len(),
                self.parts.iter().map(|p| p.base.sources.len()).sum::<usize>(),
                self.parts.len(),
                self.parts.iter().map(|p| p.tracks.len()).sum::<usize>(),
                self.also.len()
            );
        }
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
            let mut r = Ride::new(set, self.key(1 + gi as u32), base, track, second, &b)?;
            r.pass_terrain = self.pass_terrain;
            self.rides.push(r);
        }
        self.rigid_rides.clear();
        let first = 1 + self.groups.len() as u32;
        for (pi, p) in self.parts.iter().enumerate() {
            let key = self.key(first + pi as u32);
            let mut r = RigidRide::new(set, key, p.base.clone(), p.track.clone(), second, &b)?;
            r.pass_terrain = self.pass_terrain;
            self.rigid_rides.push(r);
        }
        self.last = second;
        Ok(())
    }

    /// Leaves the walking world (another space was loaded): the rides end
    /// where they stand; [`Elevator::insert`] puts the pieces in again.
    pub fn leave(&mut self) {
        self.rides.clear();
        self.rigid_rides.clear();
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
            self.started_at = Some(self.started_at.map_or(*t, |s| s.max(*t)));
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
            if self.parts.is_empty() {
                self.log.push((
                    *t,
                    format!("the run's graph starts sequence {name}: {moved} moving node(s) keyed"),
                ));
                continue;
            }
            let mut set = 0;
            for p in &mut self.parts {
                let here = motion_at(&p.track, *t);
                p.track = match p.tracks.get(name) {
                    Some(tr) => {
                        set += 1;
                        tr.iter().map(|(dt, m)| (t + dt, *m)).collect()
                    }
                    None => vec![(*t, here)],
                };
            }
            self.log.push((
                *t,
                format!("the run's graph starts sequence {name}: {set} rigid part(s) set"),
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
            for (p, r) in self.parts.iter().zip(&mut self.rigid_rides) {
                r.track = p.track.clone();
            }
        }
        any
    }

    /// Whether a part's track runs (a sequence plays on it) between the
    /// two seconds: the walker ticks through it, so a ride's end is seen.
    pub fn moving(&self, from: f64, to: f64) -> bool {
        let runs = |first: Option<f64>, last: Option<f64>, len: usize| match (first, last) {
            (Some(a), Some(z)) => len > 1 && z > from && a < to,
            _ => false,
        };
        self.rides.iter().any(|r| {
            runs(
                r.track.first().map(|p| p.0),
                r.track.last().map(|p| p.0),
                r.track.len(),
            )
        }) || self.rigid_rides.iter().any(|r| {
            runs(
                r.track.first().map(|p| p.0),
                r.track.last().map(|p| p.0),
                r.track.len(),
            )
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
        // Whether the walker stands on the piece of `key` after its ride
        // advanced.
        let on = |set: &WorldSet<Key>, walker: &Character, key: Key| {
            walker.grounded
                && walker
                    .ground
                    .and_then(|g| set.source(g.source))
                    .is_some_and(|(k, _)| *k == key)
        };
        let mut ended: Vec<f64> = Vec::new();
        let mut aloft: Vec<(f64, Key)> = Vec::new();
        for r in &mut self.rides {
            carried += r.advance(set, walker, second, &b)?;
            let end = r.track.last().map_or(0.0, |p| p.0);
            if self.last < end && second >= end && r.carried != 0.0 {
                if on(set, walker, r.key) {
                    ended.push(end);
                } else if !walker.grounded {
                    aloft.push((end, r.key));
                }
            }
        }
        for r in &mut self.rigid_rides {
            carried += r.advance(set, walker, second, &b)?;
            let end = r.track.last().map_or(0.0, |p| p.0);
            if self.last < end && second >= end && r.carried != 0.0 {
                if on(set, walker, r.key) {
                    ended.push(end);
                } else if !walker.grounded {
                    aloft.push((end, r.key));
                }
            }
        }
        for end in ended {
            self.ended = Some(end);
            self.log
                .push((end, "a ride that carried the walker ended".into()));
        }
        // `play_elevator_end_landing_v1`: a ride that ended with the walker
        // in the air ends for it when it next stands on the ride's piece;
        // standing on anything else first drops it.
        if self.landing {
            if walker.grounded && !self.pending.is_empty() {
                let landed = self
                    .pending
                    .iter()
                    .find(|(_, k)| on(set, walker, *k))
                    .map(|(t, _)| *t);
                self.pending.clear();
                if let Some(end) = landed {
                    self.ended = Some(second);
                    self.log.push((
                        second,
                        format!("a ride that carried the walker ended at {end} s with the walker in the air; it landed on the ride"),
                    ));
                }
            }
            self.pending.extend(aloft);
        }
        self.last = second;
        Ok(carried)
    }
}

/// Which drawn shapes of a named reference's model go with a moving part
/// (the same grouping its collision sources go by).
#[derive(Debug, Clone, PartialEq)]
pub enum Members {
    /// Under `play_elevator_rigid_v1`: the shapes whose node chain's
    /// deepest block among [`Platform::set`] is this block.
    Deepest(usize),
    /// Under the translation reading (`play_elevator_run_sequence_v1`):
    /// the shapes whose node chain holds exactly these blocks of
    /// [`Platform::set`] (increasing).
    Nodes(Vec<usize>),
}

/// A moving part as the drawing reads it: which shapes go with it and the
/// rigid motion its collision stands at, in world axes (a point of the part
/// as stored goes to `motion.apply(point)`).
#[derive(Debug, Clone, PartialEq)]
pub struct PartMotion {
    pub members: Members,
    pub motion: Motion,
    /// The box of the part's collision as stored (world axes), `None` when
    /// it holds none: where it stands now is this box moved by `motion`.
    pub stored: Option<([f64; 3], [f64; 3])>,
    /// Whether the walker stands on the part's piece now (grounded on one
    /// of its sources, as a ride carries it): the window's terrain cut
    /// (`play_ride_terrain_cut_v1`) reads it. False when the caller gave
    /// no ground ([`Elevator::platform`]).
    pub carries: bool,
}

impl PartMotion {
    /// The world box of the part's collision where it stands now: the
    /// stored box's eight corners moved by [`PartMotion::motion`], boxed
    /// again in world axes. `None` when the part holds no collision.
    pub fn box_now(&self) -> Option<([f64; 3], [f64; 3])> {
        Some(moved_box(self.stored?, &self.motion))
    }
}

/// A named reference's moving parts where its collision stands, for a
/// window to draw them there (read-only: nothing of the run changes).
#[derive(Debug, Clone, PartialEq)]
pub struct Platform {
    pub reference: FormKey,
    /// The space it is placed in.
    pub space: FormKey,
    /// Its model's asset path (`meshes/...`, lower case, `/`).
    pub model: String,
    /// The model's blocks the parts are read by, increasing: under the
    /// rigid reading every node some sequence sets through a transform
    /// interpolator; under the translation reading the nodes some sequence
    /// moves.
    pub set: Vec<usize>,
    /// The second the collision stands at (the rides' last advance).
    pub second: f64,
    pub parts: Vec<PartMotion>,
    /// The second the last motion a started sequence keys ends, when it is
    /// still to come (after [`Platform::second`]): the parts move until
    /// then and stand from then on. `None`: nothing moves after `second`.
    pub moves_until: Option<f64>,
    /// When a part carries the walker ([`PartMotion::carries`]): the last
    /// second that part's own motion changes, when it is still to come
    /// (after [`Platform::second`]). `None` when no part carries the walker
    /// or the carrying part stands from now on, whatever the other parts
    /// still do.
    pub carried_until: Option<f64>,
    /// The second the run last started a sequence on it, if it did.
    pub started: Option<f64>,
    /// Whether the run started that sequence at or after an activation of
    /// a reference by the walker, with no change of space between
    /// ([`crate::walker::Walker::platforms`]; `false` from
    /// [`Elevator::platform`]): the move the person's activation set off.
    pub after_activation: bool,
}

impl Platform {
    /// The part a drawn shape under the node chain `chain` (blocks, root
    /// first) goes with: `None` when its chain holds no node of
    /// [`Platform::set`] that a part is read by (it stays as stored).
    pub fn part_of(&self, chain: &[usize]) -> Option<usize> {
        let deepest = chain
            .iter()
            .rev()
            .find(|b| self.set.binary_search(b).is_ok());
        let mut held: Vec<usize> = chain
            .iter()
            .copied()
            .filter(|b| self.set.binary_search(b).is_ok())
            .collect();
        held.sort_unstable();
        held.dedup();
        self.parts.iter().position(|p| match &p.members {
            Members::Deepest(b) => deepest == Some(b),
            Members::Nodes(n) => !n.is_empty() && *n == held,
        })
    }
}

/// The rigid motion `m` as a placement transform: `m.apply(p)` equals
/// `transform(m).apply(p)` (rotation words the transpose of the turn, as
/// `Transform::apply` reads them; scale 1).
pub fn transform(m: &Motion) -> fo4_scene::transform::Transform {
    let r = m.matrix();
    let moved = m.apply([0.0; 3]);
    fo4_scene::transform::Transform {
        rotation: std::array::from_fn(|j| std::array::from_fn(|i| r[i][j])),
        translation: moved,
        scale: 1.0,
    }
}

impl Elevator {
    /// The moving parts where the collision stands now: the motion of each
    /// part (rigid) or group (translation) at the second the rides were
    /// last advanced to. No part carries the walker
    /// ([`Elevator::platform_carrying`] says which does).
    pub fn platform(&self) -> Platform {
        self.platform_carrying(None)
    }

    /// As [`Elevator::platform`], each part marked with whether `ground`,
    /// the key of the walking world's piece the walker stands on (`None`
    /// when it stands on nothing), is the part's ride piece
    /// ([`PartMotion::carries`]).
    pub fn platform_carrying(&self, ground: Option<Key>) -> Platform {
        let t = self.last;
        let mut parts = Vec::new();
        let first = 1 + self.groups.len() as u32;
        for (pi, p) in self.parts.iter().enumerate() {
            parts.push(PartMotion {
                members: Members::Deepest(p.block),
                motion: motion_at(&p.track, t),
                stored: p.base.bounds.map(|b| (b.min, b.max)),
                carries: ground == Some(self.key(first + pi as u32)),
            });
        }
        for (gi, g) in self.groups.iter().enumerate() {
            let mut blocks: Vec<usize> = g.nodes.iter().map(|n| self.nodes[*n].block).collect();
            blocks.sort_unstable();
            blocks.dedup();
            parts.push(PartMotion {
                members: Members::Nodes(blocks),
                motion: Motion::translation(sample(&self.group_track(g), t)),
                stored: g.base.bounds.map(|b| (b.min, b.max)),
                carries: ground == Some(self.key(1 + gi as u32)),
            });
        }
        // The last key of every part's and group's track: still to come,
        // the platform moves until then.
        // The last key at which each part's or group's motion changes:
        // still to come, the platform moves until then (a sequence's tail
        // that holds one pose is standing, not moving).
        let part_ends: Vec<Option<f64>> =
            self.parts.iter().map(|p| last_change(&p.track)).collect();
        let group_ends: Vec<Option<f64>> = self
            .groups
            .iter()
            .map(|g| last_translation_change(&self.group_track(g)))
            .collect();
        let latest = |e: &mut dyn Iterator<Item = f64>| {
            e.fold(None, |m: Option<f64>, e| {
                Some(m.map_or(e, |m: f64| m.max(e)))
            })
        };
        let moves_until = latest(&mut part_ends.iter().chain(&group_ends).flatten().copied());
        let carried_until = latest(
            &mut parts
                .iter()
                .zip(part_ends.iter().chain(&group_ends))
                .filter(|(m, _)| m.carries)
                .filter_map(|(_, e)| *e),
        );
        Platform {
            reference: self.reference.clone(),
            space: self.space.clone(),
            model: self.model.clone(),
            set: self.set.clone(),
            second: t,
            parts,
            moves_until: moves_until.filter(|e| *e > t),
            carried_until: carried_until.filter(|e| *e > t),
            started: self.started_at,
            after_activation: false,
        }
    }
}

impl Elevator {
    /// `play_elevator_door_arrival_on_ride_v1`: where a walker a load door
    /// puts at `pos` at `second` arrives when a rigid part of this
    /// reference is still on its way to that place: a part whose track's
    /// motion still changes after `second` and whose collision box at the
    /// track's last key holds `pos` across (x and y) with `pos` no lower
    /// than its bottom and at most `reach` above its top. The point is
    /// carried back with the part to where the part stands at `second` (the
    /// last key's motion undone, the motion at `second` applied): (the
    /// part's index, that point). Of two such parts, the one whose top at
    /// the last key is highest. `None` with no such part (the walker stands
    /// where the door puts it).
    pub fn arrival(&self, pos: [f64; 3], second: f64, reach: f64) -> Option<(usize, [f64; 3])> {
        let mut best: Option<(f64, usize, [f64; 3])> = None;
        for (i, p) in self.parts.iter().enumerate() {
            if last_change(&p.track).is_none_or(|e| e <= second) {
                continue;
            }
            let (Some(stored), Some(last)) = (p.base.bounds, p.track.last()) else {
                continue;
            };
            let (lo, hi) = moved_box((stored.min, stored.max), &last.1);
            let across = (0..2).all(|k| pos[k] >= lo[k] && pos[k] <= hi[k]);
            if !across || pos[2] < lo[2] || pos[2] > hi[2] + reach {
                continue;
            }
            let now = motion_at(&p.track, second);
            let at = now.apply(last.1.unapply(pos));
            if best.is_none_or(|(top, _, _)| hi[2] > top) {
                best = Some((hi[2], i, at));
            }
        }
        best.map(|(_, i, at)| (i, at))
    }

    /// The box of the reference's collision as stored, the moving parts
    /// at their stored transforms and the still references with it.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut out: Option<([f64; 3], [f64; 3])> = None;
        let worlds = std::iter::once(&self.stills)
            .chain(self.groups.iter().map(|g| &g.base))
            .chain(self.parts.iter().map(|p| &p.base))
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

/// The load doors of `doors` that do not lead into the space `left` (the
/// space the walker left by its last change of space): the candidates of a
/// ride's end door under `play_elevator_end_door_onward_v1`.
pub fn onward(
    doors: &BTreeMap<FormKey, crate::world::LoadDoor>,
    left: &FormKey,
) -> BTreeMap<FormKey, crate::world::LoadDoor> {
    let left = fo4_scene::timeline::norm(left);
    doors
        .iter()
        .filter(|(_, d)| fo4_scene::timeline::norm(&d.space) != left)
        .map(|(k, d)| (k.clone(), d.clone()))
        .collect()
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
                block: 3,
                tracks: BTreeMap::from([("Down".to_string(), down)]),
                track: vec![(0.0, [0.0; 3])],
            }],
            groups: vec![Group {
                nodes: vec![0],
                base: floor(0.0, 100.0),
            }],
            rides: Vec::new(),
            seen: 0,
            started_at: None,
            last: 0.0,
            log: Vec::new(),
            ended: None,
            landing: false,
            pass_terrain: false,
            pending: Vec::new(),
            held: 0,
            parts: Vec::new(),
            rigid_rides: Vec::new(),
            model: "meshes/a/platform.nif".into(),
            set: vec![3],
        }
    }

    /// `walk_ride_rider_passes_terrain_v1` (`--play-elevator-through-terrain`):
    /// the rides put into a walking world pass the terrain only when the
    /// platform asks for it, and again after a rebuild.
    #[test]
    fn the_rides_pass_the_terrain_only_under_the_switch() {
        for pass in [false, true] {
            let mut e = elevator();
            e.pass_terrain = pass;
            for _ in 0..2 {
                let mut set: WorldSet<Key> = WorldSet::new();
                e.insert(&mut set, 0.0).unwrap();
                assert_eq!(e.rides.len(), 1);
                assert!(e.rides.iter().all(|r| r.pass_terrain == pass));
                assert!(e.rigid_rides.iter().all(|r| r.pass_terrain == pass));
                e.leave();
            }
        }
        assert!(!elevator().pass_terrain);
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
        // Before the walker ticks through them, the started sequences'
        // last motion (the raise's end at 3 s) is still to come.
        assert_eq!(e.platform().moves_until, Some(3.0));
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
        // For the drawing: the part where its collision stands, and the
        // shapes under its node go with it.
        let p = e.platform();
        assert_eq!(p.parts.len(), 1);
        assert_eq!(p.parts[0].members, Members::Nodes(vec![3]));
        assert!((p.parts[0].motion.translation[2] + 100.0).abs() < 1e-9);
        assert_eq!(p.part_of(&[0, 3, 4]), Some(0));
        assert_eq!(p.part_of(&[0, 4]), None);
        assert_eq!((p.model.as_str(), p.second), ("meshes/a/platform.nif", t));
        // The walker stands on the part's piece: given its ground, the part
        // says it carries it (the window's terrain cut), and its box stands
        // where the ride left it.
        assert!(!p.parts[0].carries);
        let ground = c
            .ground
            .filter(|_| c.grounded)
            .and_then(|g| set.source(g.source))
            .map(|(k, _)| *k);
        let q = e.platform_carrying(ground);
        assert!(q.parts[0].carries);
        let (lo, hi) = q.parts[0].box_now().unwrap();
        let (slo, shi) = q.parts[0].stored.unwrap();
        assert!((hi[2] - (shi[2] - 100.0)).abs() < 1e-9, "{hi:?} {shi:?}");
        assert!((lo[0] - slo[0]).abs() < 1e-9 && (hi[1] - shi[1]).abs() < 1e-9);
        assert!(!e.platform_carrying(Some((KIND, 9))).parts[0].carries);
        // Built again (another walking world of the same space): the part
        // stands where the ride left it.
        let mut again: WorldSet<Key> = WorldSet::new();
        e.insert(&mut again, t).unwrap();
        let bounds = again.get(&e.key(1)).unwrap().bounds.unwrap();
        assert!((bounds.max[2] + 100.0).abs() < 1.0, "{bounds:?}");
    }

    /// `play_elevator_end_landing_v1`: a ride whose last key passes while
    /// the walker it carried is in the air is not an end for the walker;
    /// with the switch the end comes when the walker lands on the piece.
    #[test]
    fn a_ride_that_ends_with_the_walker_in_the_air_ends_when_it_lands_only_under_the_switch() {
        for landing in [false, true] {
            let mut lib = fo4_vm::MapLibrary::default();
            let mut host = fo4_vm::NoHost;
            let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
            let o = vm.world.object(
                "the platform",
                Some(fo4_vm::bind::record_key(&key(5))),
                &["objectreference"],
            );
            let mut e = elevator();
            e.landing = landing;
            let mut set: WorldSet<Key> = WorldSet::new();
            e.insert(&mut set, 0.0).unwrap();
            let mut c =
                Character::new(Params::PROVISIONAL, Settings::DEFAULT, [0.0, 0.0, 5.0]).unwrap();
            c.settle(&set, 600);
            vm.world.anims.started.push((2.0, o, "Down".into()));
            assert!(e.follow(&vm));
            let tick = Settings::DEFAULT.tick;
            let mut t = 1.0;
            while t < 2.9 {
                t += tick;
                e.advance(&mut set, &mut c, t).unwrap();
                c.tick(&set, fo4_walk::Input::default());
            }
            assert!(c.grounded);
            // The walker loses the piece just before its last key.
            c.feet[2] += 10.0;
            c.grounded = false;
            c.ground = None;
            let mut landed = None;
            while t < 4.0 {
                t += tick;
                e.advance(&mut set, &mut c, t).unwrap();
                c.tick(&set, fo4_walk::Input::default());
                if landed.is_none() && c.grounded {
                    landed = Some(t);
                }
            }
            let landed = landed.expect("the walker lands on the piece");
            assert!(landed > 3.0, "{landed}");
            if landing {
                let ended = e.ended.expect("an end on landing");
                assert!(
                    ended > 3.0 && ended <= landed + tick + 1e-9,
                    "{ended} {landed}"
                );
            } else {
                assert_eq!(e.ended, None);
            }
        }
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

    fn sequence(name: &str, seconds: f32) -> fo4_scene::sequence::Sequence {
        fo4_scene::sequence::Sequence {
            block: 0,
            name: name.into(),
            start: 0.0,
            stop: seconds,
            cycle: 0,
            frequency: 1.0,
            blocks: Vec::new(),
            text_keys: false,
        }
    }

    fn keys(points: &[(f32, [f32; 3])]) -> fo4_scene::clock::NodeKeys {
        fo4_scene::clock::NodeKeys {
            rotation: None,
            translation: fo4_scene::clock::Keys {
                components: 3,
                kind: if points.is_empty() {
                    0
                } else {
                    fo4_scene::clock::LINEAR
                },
                keys: points
                    .iter()
                    .map(|(time, value)| fo4_scene::clock::Key {
                        time: *time,
                        value: *value,
                        forward: [0.0; 3],
                        backward: [0.0; 3],
                        tbc: [0.0; 3],
                    })
                    .collect(),
            },
            scale: fo4_scene::clock::Keys {
                components: 1,
                kind: 0,
                keys: Vec::new(),
            },
        }
    }

    #[test]
    fn a_node_takes_its_keys_and_only_under_the_switch_its_held_translation() {
        let moving = keys(&[(0.0, [0.0; 3]), (1.0, [0.0, 0.0, 10.0])]);
        let empty = keys(&[]);
        let held = Some([5.0, 0.0, 0.0]);
        // Translation keys win over a held translation.
        assert!(matches!(
            Source::of(Some(moving.clone()), held),
            Some(Source::Keys(k)) if k.keys.len() == 2
        ));
        // Without translation keys: the held translation when the caller
        // read one (the switch), else the empty group as before.
        assert_eq!(
            Source::of(Some(empty.clone()), held),
            Some(Source::Held([5.0, 0.0, 0.0]))
        );
        assert_eq!(Source::of(None, held), Some(Source::Held([5.0, 0.0, 0.0])));
        let before = Source::of(Some(empty.clone()), None).unwrap();
        assert_eq!(before, Source::Keys(empty.translation.clone()));
        let frame = fo4_scene::transform::Transform::euler([0.0; 6], 1.0).unwrap();
        let s = sequence("Open", 1.0);
        assert!(before
            .track(&s, 1.0, [0.0; 3], frame, [0.0; 3])
            .unwrap()
            .is_empty());
        assert_eq!(Source::of(None, None), None);
    }

    #[test]
    fn a_held_translation_is_a_constant_offset_turned_by_the_placement() {
        // Placed at (100, 0, 0), turned a quarter about z, scale 2.
        let half_pi = std::f32::consts::FRAC_PI_2;
        let frame =
            fo4_scene::transform::Transform::euler([100.0, 0.0, 0.0, 0.0, 0.0, half_pi], 2.0)
                .unwrap();
        let origin = frame.apply([0.0; 3]);
        let s = sequence("Open", 0.5);
        let stored = [1.0, 2.0, 3.0];
        let tr = Source::Held([4.0, 2.0, 3.0])
            .track(&s, 0.5, stored, frame, origin)
            .unwrap();
        assert_eq!(tr.len(), 31);
        assert_eq!(tr.first().unwrap().0, 0.0);
        assert_eq!(tr.last().unwrap().0, 0.5);
        // The local offset (3, 0, 0) scaled by 2 and turned by the frame,
        // the same at every sample.
        let want = frame.apply([3.0, 0.0, 0.0]);
        let want: [f64; 3] = std::array::from_fn(|i| want[i] - origin[i]);
        assert!((want[0].hypot(want[1]) - 6.0).abs() < 1e-9 && want[2].abs() < 1e-9);
        for (_, v) in &tr {
            assert_eq!(*v, want);
        }
        // Keys are sampled over the span: half way at half the span.
        let k = Source::of(
            Some(keys(&[(0.0, [0.0; 3]), (1.0, [0.0, 0.0, 10.0])])),
            None,
        )
        .unwrap()
        .track(&sequence("Up", 1.0), 1.0, [0.0; 3], frame, origin)
        .unwrap();
        assert!((k[30].1[2] - 10.0).abs() < 1e-9, "{:?}", k[30]);
    }

    #[test]
    fn a_sequence_span_past_the_sample_ceiling_is_a_named_error() {
        let frame = fo4_scene::transform::Transform::euler([0.0; 6], 1.0).unwrap();
        let s = sequence("Open", 1.0);
        let held = Source::Held([0.0, 0.0, 1.0]);
        // At the ceiling it is read; one sample past it, or an infinite
        // span, is refused before any sample is taken.
        let at = (MAX_SAMPLES as f64) / SAMPLES_PER_SECOND;
        let tr = held.track(&s, at, [0.0; 3], frame, [0.0; 3]).unwrap();
        assert_eq!(tr.len(), MAX_SAMPLES + 1);
        for span in [at + 1.0, 1e9, f64::INFINITY] {
            let e = held.track(&s, span, [0.0; 3], frame, [0.0; 3]).unwrap_err();
            assert_eq!(e.code, "SequenceSpanTooLong");
        }
    }

    #[test]
    fn a_held_part_moves_when_the_run_starts_its_sequence_and_stays() {
        let frame = fo4_scene::transform::Transform::euler([0.0; 6], 1.0).unwrap();
        let open = Source::Held([0.0, 0.0, 40.0])
            .track(&sequence("Open", 1.0), 1.0, [0.0; 3], frame, [0.0; 3])
            .unwrap();
        let mut e = elevator();
        e.nodes[0].tracks = BTreeMap::from([("Open".to_string(), open)]);
        let mut lib = fo4_vm::MapLibrary::default();
        let mut host = fo4_vm::NoHost;
        let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
        let o = vm.world.object(
            "the door",
            Some(fo4_vm::bind::record_key(&key(5))),
            &["objectreference"],
        );
        let mut set: WorldSet<Key> = WorldSet::new();
        e.insert(&mut set, 0.0).unwrap();
        let mut c =
            Character::new(Params::PROVISIONAL, Settings::DEFAULT, [0.0, 0.0, 5.0]).unwrap();
        c.settle(&set, 600);
        // Closed (stored) until the run starts the sequence at 2 s.
        e.advance(&mut set, &mut c, 1.9).unwrap();
        assert!(set.get(&e.key(1)).unwrap().bounds.unwrap().max[2].abs() < 1e-6);
        vm.world.anims.started.push((2.0, o, "Open".into()));
        assert!(e.follow(&vm));
        for t in [2.0, 2.5, 3.0, 9.0] {
            e.advance(&mut set, &mut c, t).unwrap();
            let top = set.get(&e.key(1)).unwrap().bounds.unwrap().max[2];
            assert!((top - 40.0).abs() < 1e-6, "{t}: {top}");
        }
    }

    /// Under the rigid reading: a floor (part 0) that a sequence named `Up`
    /// raises 100 units over one second, and a small floor far below (the
    /// second part) that a sequence named `Turn` turns a quarter about the
    /// vertical through (500, 0) over one second.
    fn rigid_elevator() -> Elevator {
        let up: MotionTrack = (0..=60)
            .map(|k| {
                let f = k as f64 / 60.0;
                (f, Motion::translation([0.0, 0.0, 100.0 * f]))
            })
            .collect();
        let turn: MotionTrack = (0..=60)
            .map(|k| {
                let half = std::f64::consts::FRAC_PI_4 * k as f64 / 60.0;
                let m = Motion {
                    rotation: [half.cos(), 0.0, 0.0, half.sin()],
                    pivot: [500.0, 0.0, -500.0],
                    translation: [0.0; 3],
                };
                (k as f64 / 60.0, m)
            })
            .collect();
        let b = Budget::new(1 << 24);
        let far = floor(-500.0, 20.0)
            .copy_sources(&[0], [520.0, 0.0, 0.0], &b)
            .unwrap();
        let mut e = elevator();
        e.nodes.clear();
        e.groups.clear();
        e.set = vec![7, 9];
        e.parts = vec![
            Part {
                block: 7,
                tracks: BTreeMap::from([("Up".to_string(), up)]),
                track: vec![(0.0, Motion::IDENTITY)],
                base: floor(0.0, 100.0),
            },
            Part {
                block: 9,
                tracks: BTreeMap::from([("Turn".to_string(), turn)]),
                track: vec![(0.0, Motion::IDENTITY)],
                base: far,
            },
        ];
        e
    }

    #[test]
    fn a_door_stands_open_by_its_frame_and_the_parts_open_moves() {
        let b = Budget::new(1 << 24);
        // A frame (a floor with a gap) and two parts: a flap standing up
        // beside the gap that Open lays across it, a leaf Open lifts.
        let mut ew = World::new();
        ew.add_hull(
            &[
                [-50., -10., -5.],
                [-15., -10., -5.],
                [-50., 10., -5.],
                [-15., 10., -5.],
                [-50., -10., 0.],
                [-15., -10., 0.],
                [-50., 10., 0.],
                [-15., 10., 0.],
            ],
            0.,
            "frame",
            &b,
        )
        .unwrap();
        let cuboid = |lo: [f64; 3], hi: [f64; 3]| -> Vec<[f64; 3]> {
            (0..8)
                .map(|k| {
                    let p = |i: usize| if k & (1 << i) != 0 { hi[i] } else { lo[i] };
                    [p(0), p(1), p(2)]
                })
                .collect()
        };
        ew.add_hull(&cuboid([-15., -10., 0.], [-10., 10., 30.]), 0., "flap", &b)
            .unwrap();
        ew.add_hull(&cuboid([-15., -10., 40.], [15., 10., 150.]), 0., "leaf", &b)
            .unwrap();
        let tracks = |name: &str, m: Motion| {
            BTreeMap::from([(name.to_string(), vec![(0.0, Motion::IDENTITY), (1.3, m)])])
        };
        // The flap turned a quarter about its foot's line, laid across the
        // gap; the leaf lifted 200 units; a third part only Close moves.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let lay = Motion {
            rotation: [h, 0., h, 0.],
            pivot: [-15., 0., 0.],
            translation: [0.; 3],
        };
        let lift = Motion::translation([0., 0., 200.]);
        let parts = vec![
            (1, vec![1], tracks(OPEN_SEQUENCE, lay)),
            (2, vec![2], tracks(OPEN_SEQUENCE, lift)),
        ];
        let (open, opened) = open_pose(&ew, &[0], &parts, &b).unwrap().unwrap();
        assert_eq!((opened, open.sources.len()), (2, 3));
        // Where the gap was, the laid flap is the floor; the leaf is gone
        // from the doorway.
        let floor = open.ray_down(5., 0., 100.).unwrap();
        assert!(floor.z.abs() < 1e-6 && floor.source == 1, "{floor:?}");
        assert!(open.ray_down(5., 0., 180.).unwrap().z.abs() < 1e-6);
        assert!(open
            .surfaces(5., 0.)
            .iter()
            .any(|h| (h.z - 350.).abs() < 1e-6));
        assert!(ew.ray_down(5., 0., 30.).is_none());
        // No frame, or no part Open moves: the door stays out.
        assert!(open_pose(&ew, &[], &parts, &b).unwrap().is_none());
        let closing = vec![(1, vec![1], tracks("Close", lay))];
        assert!(open_pose(&ew, &[0], &closing, &b).unwrap().is_none());
        // A part Open does not set stands as stored.
        let mixed = vec![parts[1].clone(), (1, vec![1], tracks("Close", lay))];
        let (o2, n2) = open_pose(&ew, &[0], &mixed, &b).unwrap().unwrap();
        assert_eq!(n2, 1);
        assert!((o2.ray_down(-12., 0., 100.).unwrap().z - 30.).abs() < 1e-6);
    }

    #[test]
    fn a_rigid_track_is_sampled_by_its_motions() {
        assert!(motion_at(&[], 1.0).is_identity());
        let a = Motion::translation([0.0, 0.0, 10.0]);
        let z = Motion::translation([0.0, 0.0, 30.0]);
        let tr = vec![(1.0, a), (3.0, z)];
        assert_eq!(motion_at(&tr, 0.0), a);
        assert_eq!(motion_at(&tr, 9.0), z);
        assert!((motion_at(&tr, 2.0).translation[2] - 20.0).abs() < 1e-9);
        assert_eq!(
            Readings::default(),
            Readings {
                held_pose: false,
                rigid: false
            }
        );
    }

    #[test]
    fn a_rigid_part_follows_its_sequence_and_one_it_does_not_set_holds() {
        let mut lib = fo4_vm::MapLibrary::default();
        let mut host = fo4_vm::NoHost;
        let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
        let o = vm.world.object(
            "the platform",
            Some(fo4_vm::bind::record_key(&key(5))),
            &["objectreference"],
        );
        let mut e = rigid_elevator();
        assert!(e.summary().contains("2 rigid parts"), "{}", e.summary());
        let mut set: WorldSet<Key> = WorldSet::new();
        e.insert(&mut set, 0.0).unwrap();
        let mut c =
            Character::new(Params::PROVISIONAL, Settings::DEFAULT, [0.0, 0.0, 5.0]).unwrap();
        c.settle(&set, 600);
        assert!(c.grounded && c.feet[2].abs() < 1e-3);
        assert!(!e.moving(0.0, 10.0));
        // The run turns part 1 from 1 s: part 0 is not set and holds.
        vm.world.anims.started.push((1.0, o, "Turn".into()));
        assert!(e.follow(&vm));
        assert!(e.log.last().unwrap().1.ends_with("1 rigid part(s) set"));
        assert!(e.moving(1.5, 1.6));
        // Then raises part 0 from 2 s: part 1 holds the turn's end.
        vm.world.anims.started.push((2.0, o, "Up".into()));
        assert!(e.follow(&vm));
        assert!(e.moving(1.5, 2.5) && !e.moving(3.5, 9.0));
        let tick = Settings::DEFAULT.tick;
        let mut t = 1.0;
        while t < 2.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        assert!((c.feet[2] - 50.0).abs() < 3.0, "{:?}", c.feet);
        assert!(e.ended.is_none());
        while t < 3.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        assert!((c.feet[2] - 100.0).abs() < 1e-3, "{:?}", c.feet);
        assert_eq!(e.ended, Some(3.0));
        // Past it nothing moves any more.
        assert_eq!(e.platform().moves_until, None);
        assert!((e.rigid_rides[1].motion.angle_degrees() - 90.0).abs() < 1e-6);
        // For the drawing: each part's motion where its collision stands;
        // a shape goes with the deepest set node of its chain.
        let p = e.platform();
        assert_eq!(
            p.parts
                .iter()
                .map(|x| x.members.clone())
                .collect::<Vec<_>>(),
            [Members::Deepest(7), Members::Deepest(9)]
        );
        assert_eq!(p.part_of(&[0, 7, 8]), Some(0));
        assert_eq!(p.part_of(&[0, 7, 9, 10]), Some(1));
        assert_eq!(p.part_of(&[0, 2]), None);
        assert!((p.parts[0].motion.translation[2] - 100.0).abs() < 1e-9);
        // The walker stands on part 0 (raised), not on part 1.
        let ground = c
            .ground
            .filter(|_| c.grounded)
            .and_then(|g| set.source(g.source))
            .map(|(k, _)| *k);
        let q = e.platform_carrying(ground);
        assert_eq!(
            q.parts.iter().map(|x| x.carries).collect::<Vec<_>>(),
            [true, false]
        );
        let corner = p.parts[1].motion.apply([540.0, 0.0, -500.0]);
        assert!(
            (corner[0] - 500.0).abs() < 1e-6 && (corner[1] - 40.0).abs() < 1e-6,
            "{corner:?}"
        );
        // As a placement transform: the same points, and composed after a
        // draw's own transform, the motion after it.
        let w = fo4_scene::transform::Transform {
            rotation: [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [510.0, 5.0, -500.0],
            scale: 2.0,
        };
        for m in p.parts.iter().map(|x| x.motion) {
            for q in [[0.0; 3], [540.0, 0.0, -500.0], [1.0, -2.0, 3.0]] {
                let a = transform(&m).apply(q);
                let b = m.apply(q);
                assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-9), "{a:?} {b:?}");
                let a = transform(&m).compose(w).apply(q);
                let b = m.apply(w.apply(q));
                assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-9), "{a:?} {b:?}");
            }
        }
        // The far piece turned a quarter about (500, 0): from x 500..540
        // to y 0..40.
        let far = set.get(&e.key(2)).unwrap().bounds.unwrap();
        assert!(
            (far.min[0] - 480.0).abs() < 1e-6 && (far.max[1] - 40.0).abs() < 1e-6,
            "{far:?}"
        );
        // Built again: each part stands where its ride left it, and the box
        // holds the parts as stored.
        let mut again: WorldSet<Key> = WorldSet::new();
        e.insert(&mut again, t).unwrap();
        let top = again.get(&e.key(1)).unwrap().bounds.unwrap().max[2];
        assert!((top - 100.0).abs() < 1.0, "{top}");
        let b = e.bounds().unwrap();
        assert!(b.0[2] <= -500.0 + 1e-6 && b.1[0] >= 540.0 - 1e-6, "{b:?}");
        e.leave();
        assert!(!e.moving(0.0, 10.0));
    }

    /// `play_elevator_door_arrival_on_ride_v1`: a door's place where a
    /// part arrives at its track's end, while the part is on its way, is
    /// carried back to where the part stands now; before the run starts the
    /// part's sequence and after it ends, the place stands.
    #[test]
    fn a_door_place_a_part_is_on_its_way_to_is_carried_back_with_it() {
        let mut lib = fo4_vm::MapLibrary::default();
        let mut host = fo4_vm::NoHost;
        let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
        let o = vm.world.object(
            "the platform",
            Some(fo4_vm::bind::record_key(&key(5))),
            &["objectreference"],
        );
        let mut e = rigid_elevator();
        let place = [10.0, -20.0, 100.5];
        // Nothing started: no part is on its way.
        assert_eq!(e.arrival(place, 2.0, 20.0), None);
        // The run raises part 0 from 2 s to 3 s: its floor reaches z 100.
        vm.world.anims.started.push((2.0, o, "Up".into()));
        assert!(e.follow(&vm));
        let (part, at) = e.arrival(place, 2.0, 20.0).unwrap();
        assert_eq!(part, 0);
        assert!(
            (at[0] - 10.0).abs() < 1e-9
                && (at[1] + 20.0).abs() < 1e-9
                && (at[2] - 0.5).abs() < 1e-9,
            "{at:?}"
        );
        let (_, half) = e.arrival(place, 2.5, 20.0).unwrap();
        assert!((half[2] - 50.5).abs() < 1e-6, "{half:?}");
        // Too high above the arriving top, outside it across, or once the
        // part stands: the place stands.
        assert_eq!(e.arrival([10.0, -20.0, 121.0], 2.0, 20.0), None);
        assert_eq!(e.arrival([150.0, -20.0, 100.5], 2.0, 20.0), None);
        assert_eq!(e.arrival(place, 3.0, 20.0), None);
        // Put there, a walker rides the part up to the door's place.
        let mut set: WorldSet<Key> = WorldSet::new();
        e.insert(&mut set, 2.0).unwrap();
        let mut c = Character::new(Params::PROVISIONAL, Settings::DEFAULT, at).unwrap();
        c.settle(&set, 600);
        assert!(c.grounded && c.feet[2].abs() < 1e-3, "{:?}", c.feet);
        let tick = Settings::DEFAULT.tick;
        let mut t = 2.0;
        while t < 3.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        assert!((c.feet[2] - 100.0).abs() < 1e-3, "{:?}", c.feet);
    }

    /// The platform's countdown reads the last key that moves: a held tail
    /// of keys stands. A part that carries the walker and stands no longer
    /// counts, whatever another part still does.
    #[test]
    fn a_carried_walker_counts_only_its_own_part_while_it_moves() {
        let m = |z: f64| Motion::translation([0.0, 0.0, z]);
        let held = vec![(0.0, m(0.0)), (1.0, m(5.0)), (2.0, m(5.0)), (3.0, m(5.0))];
        assert_eq!(last_change(&held), Some(1.0));
        assert_eq!(last_change(&[(0.0, m(1.0)), (4.0, m(1.0))]), None);
        assert_eq!(
            last_translation_change(&[
                (0.0, [0.0; 3]),
                (2.0, [0.0, 1.0, 0.0]),
                (3.0, [0.0, 1.0, 0.0])
            ]),
            Some(2.0)
        );
        let mut lib = fo4_vm::MapLibrary::default();
        let mut host = fo4_vm::NoHost;
        let mut vm = fo4_vm::Vm::new(&mut lib, &mut host);
        let o = vm.world.object(
            "the platform",
            Some(fo4_vm::bind::record_key(&key(5))),
            &["objectreference"],
        );
        // One sequence, `Both`: part 0 (the floor) rises 100 units in its
        // first second and holds there to 1.5 s; part 1 turns over 1.5 s.
        let mut e = rigid_elevator();
        let both0: MotionTrack = (0..=90)
            .map(|k| {
                let f = k as f64 / 60.0;
                (f, m(100.0 * f.min(1.0)))
            })
            .collect();
        let both1 = e.parts[1].tracks["Turn"]
            .iter()
            .map(|(t, x)| (t * 1.5, *x))
            .collect();
        e.parts[0].tracks.insert("Both".into(), both0);
        e.parts[1].tracks.insert("Both".into(), both1);
        let mut set: WorldSet<Key> = WorldSet::new();
        e.insert(&mut set, 0.0).unwrap();
        let mut c =
            Character::new(Params::PROVISIONAL, Settings::DEFAULT, [0.0, 0.0, 5.0]).unwrap();
        c.settle(&set, 600);
        vm.world.anims.started.push((2.0, o, "Both".into()));
        assert!(e.follow(&vm));
        let tick = Settings::DEFAULT.tick;
        let mut t = 2.0;
        let ground = |set: &WorldSet<Key>, c: &Character| {
            c.ground
                .filter(|_| c.grounded)
                .and_then(|g| set.source(g.source))
                .map(|(k, _)| *k)
        };
        while t < 2.5 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        let p = e.platform_carrying(ground(&set, &c));
        assert!(p.parts[0].carries);
        assert_eq!(p.carried_until, Some(3.0));
        assert_eq!(p.moves_until, Some(3.5));
        while t < 3.2 {
            t += tick;
            e.advance(&mut set, &mut c, t).unwrap();
            c.tick(&set, fo4_walk::Input::default());
        }
        let p = e.platform_carrying(ground(&set, &c));
        assert!(p.parts[0].carries);
        assert_eq!(p.carried_until, None);
        assert_eq!(p.moves_until, Some(3.5));
        // Not carried: no part's own end counts.
        assert_eq!(e.platform().carried_until, None);
    }

    #[test]
    fn the_onward_doors_leave_out_those_back_into_the_space_left() {
        let door = |space: u32| crate::world::LoadDoor {
            to: key(100 + space),
            space: key(space),
            pos: [0.0; 3],
            rot: [0.0; 3],
        };
        let doors = BTreeMap::from([(key(10), door(1)), (key(11), door(2)), (key(12), door(1))]);
        let kept = onward(&doors, &key(1));
        assert_eq!(kept.keys().cloned().collect::<Vec<_>>(), [key(11)]);
        assert_eq!(onward(&doors, &key(3)).len(), 3);
    }

    #[test]
    fn a_still_reference_goes_with_the_named_reference_of_its_own_space() {
        let spaces = vec![Some(key(1)), Some(key(2)), None];
        assert_eq!(still_owner(&spaces, &Some(key(2))), 1);
        assert_eq!(still_owner(&spaces, &Some(key(1))), 0);
        // In no named reference's space, or unread: the first, as before.
        assert_eq!(still_owner(&spaces, &Some(key(3))), 0);
        assert_eq!(still_owner(&spaces, &None), 0);
    }
}
