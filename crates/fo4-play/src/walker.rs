//! The headless play mode's driver: the scripted input moves the walker
//! (`fo4_walk`) over the cell's collision, and each of the walker's ticks
//! gives the run the player's point (`play_player_point_walker_feet_v1`)
//! at the second the tick ends (`play_run_clock_input_seconds_v1`); the
//! activation key gives the run the activation of the reference the eye's
//! ray meets first (`play_activate_collision_ray_v1`, [`crate::pick`]).
//!
//! The walker steps in its fixed tick; a command between two ticks cuts
//! the tick at the command's second (a shorter tick), so the input's
//! seconds are kept exactly. While no key is held and the walker stands
//! still on the ground, the slice runs to the next command with no point:
//! the player stands at the last one.
use crate::input::{Command, Controls, Event, Script};
use crate::live::{Decision, Live, RunState};
use crate::world::LoadDoor;
use crate::{pick, Act, Driver, Next, PlayerAction, Point, Slice, View};
use fo4_core::Budget;
use fo4_plugin::FormKey;
use fo4_scene::timeline::norm;
use fo4_walk::{Character, Params, Settings, World};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A tick that moves the walker's feet less than this (world units) from
/// the last point gives no point: the ground probe's rounding is not a
/// move (a numerical setting).
pub const STILL: f64 = 1e-6;

/// `play_activate_actor_box_v1`: half the width of an actor's box (world
/// units; an assumption, no data of this step gives an actor's
/// activation shape).
pub const ACTOR_BOX_HALF_WIDTH: i16 = 20;
/// `play_activate_actor_box_v1`: an actor's box height above its place
/// (world units; an assumption).
pub const ACTOR_BOX_HEIGHT: i16 = 120;

/// Each actor's space and position (`play_activate_actor_box_v1`).
pub type ActorPlaces = BTreeMap<FormKey, (FormKey, [f64; 3])>;

/// `play_activate_actor_box_v1`: where the run last put each actor, read
/// without changing the run: its stored place, then the caller's or a
/// package's moves applied so far and the places a `MoveTo` with zero
/// offsets gave an actor, in the order of their seconds (keys as
/// [`norm`] gives them; the space and the position).
pub fn actor_places(
    place: &fo4_vm::track::Place,
    moved: &[(f64, FormKey, FormKey, [f64; 3])],
) -> ActorPlaces {
    let mut out = BTreeMap::new();
    for a in &place.actors {
        out.insert(norm(&a.key), (norm(&a.space), a.pos));
    }
    let applied = place.actor_cursor.min(place.actor_moves.len());
    let mut later: Vec<(f64, FormKey, FormKey, [f64; 3])> = place.actor_moves[..applied]
        .iter()
        .map(|m| (m.t, norm(&m.actor), norm(&m.space), m.pos))
        .collect();
    later.extend(moved.iter().map(|(t, k, s, p)| (*t, norm(k), norm(s), *p)));
    later.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, k, space, pos) in later {
        out.insert(k, (space, pos));
    }
    out
}

/// `play_activate_actor_box_v1`: the boxes of the actors of `places`
/// standing in `space` that `skip` keeps (a disabled actor is not met).
pub fn actor_boxes(
    places: &ActorPlaces,
    space: &FormKey,
    skip: &dyn Fn(&FormKey) -> bool,
) -> pick::Boxes {
    let mut boxes = pick::Boxes::default();
    let (w, h) = (ACTOR_BOX_HALF_WIDTH, ACTOR_BOX_HEIGHT);
    for (k, (s, pos)) in places {
        if s != space || skip(k) {
            continue;
        }
        let mut frame = fo4_scene::transform::Transform::IDENTITY;
        frame.translation = *pos;
        if boxes.push(k.clone(), frame, [-w, -w, 0, w, w, h]) {
            boxes.kinds.push(Some(*b"ACHR"));
            boxes.modelled.push(Some(true));
        }
    }
    boxes
}

/// The caller's choices for the walker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub params: Params,
    pub settings: Settings,
    /// How far the activation key reaches from the eye (world units): the
    /// caller's number (no data of this step gives it); `None` refuses an
    /// input with `activate`.
    pub reach: Option<f64>,
    /// Ticks the walker may take to settle on the ground under the start
    /// point before the input begins.
    pub settle_ticks: u64,
    /// The shortest slice given to the run (seconds; `--play-slice`): the
    /// walker's ticks are joined into one slice until it lasts this long,
    /// a command or an activation ends it (0: one tick a slice). The run's
    /// outcome does not depend on it; the live world and the walking world
    /// change only between slices.
    pub slice: f64,
    /// The collision readiness readings every walking world the walker
    /// builds again takes (a load door, the moving grid), as the first one
    /// took them ([`crate::world::Options::collision`]).
    pub collision: fo4_scene::readiness::Switches,
    /// `xesp_child_flag_agrees_chain_v1` in every walking world the walker
    /// builds again and in its live world's decisions, as the first one
    /// took it ([`crate::world::Options::child_flag_agrees`],
    /// [`Live::with_child_flag_agrees`];
    /// `--play-collision-child-flag-agrees`).
    pub child_flag_agrees: bool,
    /// The collision markers' boxes in every walking world the walker
    /// builds ([`crate::world::Options::marker_boxes`];
    /// `play_collision_marker_boxes_v1`, `--play-marker-boxes`).
    pub marker_boxes: bool,
    /// With `marker_boxes`, the boxes whose collision layer word is one a
    /// walking character passes left out in every walking world the walker builds
    /// ([`crate::world::Options::marker_box_layers`];
    /// `play_collision_marker_box_layer_passed_v1`, `--play-marker-box-layers`).
    pub marker_box_layers: bool,
    /// With `marker_box_layers`, the passed layers from the load order's
    /// collision layer records
    /// ([`crate::world::Options::marker_box_layer_records`];
    /// `play_collision_marker_box_layer_records_v1`,
    /// `--play-marker-box-layer-records`).
    pub marker_box_layer_records: bool,
    /// The loaded doors' boxes in every walking world the walker builds,
    /// standing while the run holds a door closed with its activation
    /// blocked ([`crate::world::Options::held_doors`];
    /// `play_door_closed_blocked_holds_v1`, `--play-held-doors`).
    pub held_doors: bool,
    /// With `held_doors`, a held door whose reference stores no `ONAM` is
    /// closed at load, and the walker's open of a door
    /// ([`Walker::with_opens`]) opens it
    /// ([`crate::world::Options::held_at_load`];
    /// `play_door_closed_at_load_v1`, `--play-held-doors-at-load`).
    pub held_at_load: bool,
    /// The loaded doors' collision in their open pose in every walking
    /// world the walker builds ([`crate::world::Options::door_open`];
    /// `play_door_collision_open_pose_v1`, `--play-door-open-collision`).
    pub door_open: bool,
    /// With `held_doors` and `door_open`: a door standing in its open pose
    /// is open for the held doors' reading, and the person's activation
    /// closes a door that reading shows open and opens again one the
    /// person closed ([`Walker::held_door_states`], [`door_toggle`];
    /// `play_door_activation_toggles_v1`, `--play-door-toggle`).
    pub door_toggle: bool,
    /// With `door_toggle`: a door whose activation the run last blocked is
    /// left as it stands by the person's activation while the held doors'
    /// reading shows it open ([`door_toggle`];
    /// `play_door_toggle_blocked_unchanged_v1`, `--play-door-toggle-blocked`).
    pub door_toggle_blocked: bool,
    /// With `door_toggle`: a held door whose reference stores `ONAM` is open
    /// for the held doors' reading from load ([`Walker::held_door_states`];
    /// `play_door_stored_open_v1`, `--play-door-stored-open`).
    pub door_stored_open: bool,
    /// The longest standing slice (seconds; `--play-stand`), and how far
    /// ahead the walker looks for a moving platform before it stands: a
    /// policy of the play mode, no game claim. `None` (the default): a
    /// standing slice runs to the next command and the walker looks that
    /// far. Commands that arrive while the walker runs ([`Arrivals`]) need
    /// it: the next command is not known beyond the present.
    pub stand: Option<f64>,
    /// The activation's ray meets what it enters first: a box or a body the
    /// eye stands inside only when it meets nothing else, and the named
    /// platforms through their collision, not their object bounds
    /// (`play_activate_ray_first_met_v1`, `--play-pick-first-met`).
    pub first_met: bool,
    /// `--play-pick-actors` (`play_activate_actor_box_v1`): the ray meets
    /// the run's actors as upright boxes ([`actor_boxes`]).
    pub pick_actors: bool,
    /// Record each stretch of blocked ticks with what blocked it
    /// ([`Log::walls`]; `--play-report-walls`, a report only).
    pub report_walls: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            params: Params::PROVISIONAL,
            settings: Settings::DEFAULT,
            reach: None,
            settle_ticks: 600,
            slice: 0.0,
            collision: fo4_scene::readiness::Switches::default(),
            child_flag_agrees: false,
            marker_boxes: false,
            marker_box_layers: false,
            marker_box_layer_records: false,
            held_doors: false,
            held_at_load: false,
            door_open: false,
            door_toggle: false,
            door_toggle_blocked: false,
            door_stored_open: false,
            stand: None,
            first_met: false,
            pick_actors: false,
            report_walls: false,
        }
    }
}

/// Commands that arrive while the walker runs, from a person at a window
/// (`fo4-view --play-keys`): the same commands as a scripted input's
/// ([`crate::input::Command`]), at seconds of the play clock, known for
/// good up to a second the source names (its horizon). The walker asks for
/// them only as far as its next step needs to know them, so it takes the
/// same ticks and gives the run the same slices as it does when the same
/// commands come from a file ([`Script`]): a source and a file holding its
/// commands drive one walker the same way.
pub trait Arrivals {
    /// Waits until every command up to `until` is known; gives the
    /// commands that arrived since the last call, in time order and each
    /// after the horizon the last call gave, and the horizon now (at least
    /// `until`). `None`: the source closed (the window closed) and the
    /// walker stops. `at` and `feet`: the walker's second and feet as it
    /// waits, for a source that shows them.
    fn wait(&mut self, until: f64, at: f64, feet: Option<[f64; 3]>) -> Option<(Vec<Event>, f64)>;

    /// The named platforms where their collision stands at the second
    /// [`Arrivals::wait`] gives next, before each wait while the walker has
    /// any (for a source that shows them with the feet: a window draws
    /// the moving parts and their riders at the tick the feet stand at,
    /// not at the last slice's end). Nothing by default; reads only.
    fn platforms(&mut self, _platforms: Vec<crate::elevator::Platform>) {}
}

/// Why the walker stopped when its commands' source closed.
pub const SOURCE_CLOSED: &str = "the commands' source closed";

/// One activation: (second, eye, direction, what the ray met: the
/// reference and the distance, or why nothing was activated).
pub type Activation = (f64, [f64; 3], [f64; 3], Result<(FormKey, f64), String>);

/// What the walker did, for the caller after the run.
#[derive(Debug, Clone, Default)]
pub struct Log {
    /// The start: the run's first point and the settled feet.
    pub start: Option<(Point, [f64; 3], u64)>,
    /// Ticks run (whole and cut).
    pub ticks: u64,
    /// Slices given to the run, and those with no point (standing).
    pub slices: u64,
    pub standing_slices: u64,
    /// Points given to the run.
    pub points: u64,
    /// The points, in order.
    pub track: Vec<Point>,
    /// Ticks the walker was blocked by a wall or climbed a step.
    pub blocked: u64,
    pub stepped: u64,
    /// Ticks the walker ended in the air.
    pub airborne: u64,
    /// Each activation: (second, eye, direction, what the ray met: the
    /// reference, the distance, or why nothing was activated).
    pub activations: Vec<Activation>,
    /// The controls after each command: (second, heading, pitch).
    pub looks: Vec<(f64, f64, f64)>,
    /// Each `toward` that arrived: (second, feet).
    pub arrivals: Vec<(f64, [f64; 3])>,
    /// Wall seconds spent in the walker's ticks.
    pub tick_seconds: f64,
    /// The live world's counts and costs, kept across load doors: (applies,
    /// references entered, left, moved, seconds, longest).
    pub live: Option<crate::live::Stats>,
    /// Each load door crossed: (second, door, the door it names, the
    /// space, the teleport position, the settled feet, wall seconds to
    /// build the new walking world, its report).
    pub doors: Vec<Crossed>,
    /// Each time the walking world was built again about the walker
    /// (`--play-follow-grid`): (second, cell, feet, wall seconds).
    pub grid: Vec<(f64, FormKey, [f64; 3], f64)>,
    /// Rules relied on.
    pub rules: std::collections::BTreeSet<&'static str>,
    /// The moving platforms (`--play-elevator`): their build report and
    /// what they did, (second, text).
    pub elevators: Vec<(f64, String)>,
    /// The run's `MoveTo` calls and sits of the player the walker did not
    /// follow (`--play-player-moveto`, `--play-player-sits`): (second, why).
    pub player_moves: Vec<(f64, String)>,
    /// The held doors' boxes (`--play-held-doors`): (second, door, whether
    /// its box entered the walking world or left it, and why).
    pub held_doors: Vec<(f64, FormKey, bool, HeldWhy)>,
    /// With [`Options::report_walls`]: each stretch of blocked ticks
    /// against the same source, (first second, last second, ticks, what
    /// blocked: [`Live::describe_source`]).
    pub walls: Vec<(f64, f64, u64, String)>,
}

/// Why a held door's box entered or left the walking world
/// ([`Log::held_doors`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldWhy {
    /// The run's scripts (or, for a box that left, anything but the
    /// person's activation under `--play-door-toggle`).
    Run,
    /// Closed at load: no script has set its open state
    /// (`--play-held-doors-at-load`).
    AtLoad,
    /// The person's activation closed or opened it (`--play-door-toggle`).
    Person,
}

/// One load door crossed, or one `MoveTo` of the player the walker
/// followed ([`Log::doors`]).
#[derive(Debug, Clone)]
pub struct Crossed {
    /// The rule that took the walker: [`crate::rules::LOAD_DOOR`] (`door`
    /// is the door activated), [`crate::rules::PLAYER_MOVETO`] (`door` is
    /// the `MoveTo`'s target) or [`crate::rules::PLAYER_SIT`] (`door` is the
    /// furniture).
    pub rule: &'static str,
    pub t: f64,
    pub door: FormKey,
    pub to: LoadDoor,
    pub cell: FormKey,
    pub feet: [f64; 3],
    pub seconds: f64,
    pub report: String,
}

/// How the walker crosses load doors (`--play-load-doors`,
/// `play_load_door_activation_v1`): the load doors of the walking world and
/// how to build the next one.
#[derive(Debug, Clone, Default)]
pub struct Doors {
    pub doors: BTreeMap<FormKey, LoadDoor>,
    /// Cells about the destination's cell (grid steps).
    pub around: u32,
}

/// The walking world follows the walker over a worldspace's grid
/// (`--play-follow-grid`): the cells `around` the walker's square, built
/// again when it stands in another square.
#[derive(Debug, Clone, Default)]
pub struct Grid {
    pub around: u32,
    centre: Option<(FormKey, [i32; 2])>,
}
impl Grid {
    pub fn new(around: u32) -> Grid {
        Grid {
            around,
            centre: None,
        }
    }
}

/// The moving platforms the caller names (`--play-elevator`,
/// [`crate::elevator`]).
#[derive(Debug, Clone, Default)]
pub struct Elevators {
    /// The references whose collision moves.
    pub references: Vec<FormKey>,
    /// Other references whose collision is placed as stored, still.
    pub also: Vec<FormKey>,
    /// Still references the caller gives to a named reference (the
    /// reference, the named one): they go into that one's walking worlds
    /// whatever their own space (`--play-elevator-also-with REF OWNER`).
    pub also_with: Vec<(FormKey, FormKey)>,
    /// A ride that carried the walker ends at the nearest load door within
    /// the reach (`--play-elevator-door`).
    pub door: bool,
    /// A node a sequence controls without translation keys moves to the
    /// translation the sequence holds it at (`--play-elevator-held-pose`,
    /// `play_elevator_held_pose_v1`).
    pub held_pose: bool,
    /// The end door leaves out the load doors that lead into the space the
    /// walker left by its last change of space (`--play-elevator-door-onward`,
    /// `play_elevator_end_door_onward_v1`).
    pub door_onward: bool,
    /// A ride that ended with the walker in the air ends for the walker
    /// when it next stands on the ride's piece (`--play-elevator-door-landing`,
    /// `play_elevator_end_landing_v1`).
    pub door_landing: bool,
    /// The parts move by the rigid motion their chain makes under each
    /// sequence, in place of the translations (`--play-elevator-rigid`,
    /// `play_elevator_rigid_v1`).
    pub rigid: bool,
    /// While the walker stands on a moving part, the walking world's
    /// terrain does not touch it (`--play-elevator-through-terrain`,
    /// `walk_ride_rider_passes_terrain_v1`): a deck whose shaft goes below
    /// the terrain carries its rider through it.
    pub through_terrain: bool,
}

/// The longest standing slice while moving platforms are named (seconds):
/// the run's graphs may start a sequence in it, which the platforms read
/// between slices (a setting of the play mode).
pub const ELEVATOR_STAND: f64 = 0.1;

/// Of the held doors closed at load, those the person opens
/// (`--play-activate-opens`; `play_door_closed_at_load_v1`): the reading
/// applies to them only. The run's scripts open other doors by natives the
/// held doors' reading does not read (a pod's door by its animation), so a
/// door closed at load that the person does not open keeps no state until
/// a script sets one.
pub fn persons_doors(
    closed: std::collections::BTreeSet<FormKey>,
    opens: &[FormKey],
) -> std::collections::BTreeSet<FormKey> {
    closed
        .into_iter()
        .filter(|k| opens.iter().any(|o| norm(o) == norm(k)))
        .collect()
}

/// What the person's activation of door `k` does under
/// `play_door_activation_toggles_v1` (`--play-door-toggle`): `Some(false)`
/// closes it, a door the held doors' reading shows open (`states`, as
/// [`Walker::held_door_states`] gives them: `false`, no box: a door
/// standing in its open pose, or a held door whose box left, the house's
/// front door among them); `Some(true)` opens it again, a door whose box
/// stands because the person closed it (`state`); `None` for anything else
/// (the activation as before). With `blocked_unchanged`
/// (`play_door_toggle_blocked_unchanged_v1`), `None` for a door whose
/// activation the run's scripts last blocked ([`door_blocked`]).
pub fn door_toggle(
    states: &std::collections::BTreeMap<FormKey, bool>,
    state: Option<&crate::live::DoorState>,
    k: &FormKey,
    blocked_unchanged: bool,
) -> Option<bool> {
    if blocked_unchanged && door_blocked(state) {
        return None;
    }
    match states.get(k) {
        Some(false) => Some(false),
        Some(true) if state.is_some_and(|d| d.person_closed) => Some(true),
        _ => None,
    }
}

/// Whether the run's scripts last blocked the door's activation
/// (`BlockActivation(true)`, [`crate::live::DoorState::blocked`]).
pub fn door_blocked(state: Option<&crate::live::DoorState>) -> bool {
    state.is_some_and(|d| d.blocked == Some(true))
}

/// The drawing side's rules the collision switches rely on.
fn collision_rules(o: &Options) -> std::collections::BTreeSet<&'static str> {
    let c = o.collision;
    let mut out = std::collections::BTreeSet::new();
    if o.child_flag_agrees {
        out.insert(fo4_scene::hypotheses::ENABLE_CHILD_FLAG_AGREES);
    }
    if o.marker_boxes {
        out.insert(crate::rules::MARKER_BOXES);
        out.insert(fo4_walk::platform::rules::COLLISION_MARKER_BOX);
    }
    if o.marker_boxes && o.marker_box_layers {
        out.insert(crate::rules::MARKER_BOX_LAYER_PASSED);
    }
    if o.marker_boxes && o.marker_box_layers && o.marker_box_layer_records {
        out.insert(crate::rules::MARKER_BOX_LAYER_RECORDS);
    }
    if o.held_doors {
        out.insert(crate::rules::HELD_DOORS);
    }
    if o.held_doors && o.held_at_load {
        out.insert(crate::rules::HELD_DOORS_AT_LOAD);
    }
    if o.held_doors && o.door_open && o.door_toggle {
        out.insert(crate::rules::DOOR_TOGGLE);
        if o.door_stored_open {
            out.insert(crate::rules::DOOR_STORED_OPEN);
        }
    }
    if o.door_open {
        out.insert(crate::rules::DOOR_OPEN_COLLISION);
        out.insert(fo4_scene::hypotheses::ANIMATED_COLLISION_AS_STORED);
        out.insert(crate::rules::ELEVATOR_RIGID);
    }
    if c.off_chain {
        out.insert(fo4_scene::hypotheses::COLLISION_OFF_CHAIN);
    }
    if c.reference_alpha {
        out.insert(fo4_scene::hypotheses::REFERENCE_ALPHA_COLLISION);
    }
    if c.static_fields {
        out.insert(fo4_scene::hypotheses::STATIC_REFERENCE_FIELDS_COLLISION);
    }
    if c.activator_fields {
        out.insert(fo4_scene::hypotheses::ACTIVATOR_FIELDS_COLLISION);
    }
    out
}

/// Where the run puts the player, for the walker to go to: (target,
/// place, rule).
pub type Place = (FormKey, LoadDoor, &'static str);

/// The place a `MoveTo` of the player gives (`play_player_moveto_v1`): its
/// target's settled spot when its offsets are zero; else why not.
pub fn move_place(m: &fo4_vm::track::PlayerMove) -> Result<Place, String> {
    match (&m.target, m.zero_offsets) {
        (
            fo4_vm::track::Spot::At {
                space, pos, record, ..
            },
            true,
        ) => {
            let to = LoadDoor {
                to: record.clone().unwrap_or_else(|| space.clone()),
                space: norm(space),
                pos: *pos,
                rot: [0.0; 3],
            };
            Ok((to.to.clone(), to, crate::rules::PLAYER_MOVETO))
        }
        (fo4_vm::track::Spot::At { .. }, false) => {
            Err("a MoveTo of the player not followed: offsets that are not zero".into())
        }
        (fo4_vm::track::Spot::Open(why), _) => Err(format!(
            "a MoveTo of the player not followed: target open: {why}"
        )),
    }
}

/// The place a sit of the player in the furniture reference `k` gives
/// (`play_player_sit_furniture_v1`): the furniture's stored place `at`
/// when it stands in another space than the walker's (`here`); in the
/// walker's own space the walk itself stands for the way to it, and the
/// sit is not followed.
pub fn sit_place(
    k: FormKey,
    at: Result<(FormKey, [f64; 3]), String>,
    here: Option<&FormKey>,
) -> Result<Place, String> {
    let (space, pos) =
        at.map_err(|e| format!("a sit of the player not followed: the furniture's place: {e}"))?;
    let space = norm(&space);
    if here.map(norm).as_ref() == Some(&space) {
        return Err(format!(
            "a sit of the player not followed: {} stands in the walker's space",
            fo4_vm::bind::record_key(&k)
        ));
    }
    Ok((
        k.clone(),
        LoadDoor {
            to: k,
            space,
            pos,
            rot: [0.0; 3],
        },
        crate::rules::PLAYER_SIT,
    ))
}

/// Notes for the log: (second, text).
pub type Notes = Vec<(f64, String)>;

/// Of the places the run gave the player since the last look, (second,
/// place or why not), the last in the run's order (a stable sort by
/// second keeps the order of equal seconds), and the notes to log: the
/// places not followed, and those a later one replaces.
pub fn latest_place(mut found: Vec<(f64, Result<Place, String>)>) -> (Option<(f64, Place)>, Notes) {
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut last: Option<(f64, Place)> = None;
    let mut notes = Vec::new();
    for (t, r) in found {
        match r {
            Ok(x) => {
                if let Some((t0, (k, _, rule))) = &last {
                    notes.push((
                        *t0,
                        format!(
                            "replaced by a later place at {t} s ({}, {rule})",
                            fo4_vm::bind::record_key(k)
                        ),
                    ));
                }
                last = Some((t, x));
            }
            Err(why) => notes.push((t, why)),
        }
    }
    (last, notes)
}

/// The walker driven by a scripted input.
pub struct Walker {
    elevator_options: Option<Elevators>,
    elevators: Vec<crate::elevator::Elevator>,
    grid: Option<Grid>,
    live: Live,
    /// The run's state as the live world reads it (`--play-live-world`).
    state: RunState,
    /// The held doors whose boxes the log last gave as standing.
    held_logged: std::collections::BTreeSet<FormKey>,
    /// The doors the person's activation opened since the log last read
    /// the boxes (`--play-door-toggle`).
    toggled_open: std::collections::BTreeSet<FormKey>,
    nowait: bool,
    doors: Option<Doors>,
    door: Option<(FormKey, LoadDoor)>,
    /// Follow the run's `MoveTo` calls on the player
    /// (`play_player_moveto_v1`): the calls read so far and the one to
    /// follow.
    player_moves: Option<usize>,
    /// Follow the run's sits of the player (`play_player_sit_furniture_v1`):
    /// the sits and get-ups read so far.
    player_sits: Option<usize>,
    /// Where the run last put the player, to go to: (target, place, rule).
    moved: Option<Place>,
    boxes: Option<pick::Boxes>,
    /// `--play-pick-actors`: the actors' places last read and the boxes
    /// the ray meets with them ([`Walker::boxes`] and the actors' boxes),
    /// built again when either changes.
    actor_pick: Option<(ActorPlaces, u64, pick::Boxes)>,
    /// Counts each change of [`Walker::boxes`].
    boxes_changed: u64,
    /// Boxes of statics left out (`play_activate_bounds_not_static_v1`),
    /// also in the walking worlds built later.
    statics_out: bool,
    script: Script,
    /// Commands arriving while the walker runs (`None`: the script holds
    /// every command) and the second up to which they are known (infinite
    /// with a script).
    arrivals: Option<Box<dyn Arrivals>>,
    known: f64,
    opts: Options,
    next_event: usize,
    controls: Controls,
    character: Option<Character>,
    space: Option<FormKey>,
    /// The space the walker left by its last change of space (a load
    /// door, a `MoveTo`, a sit), for `play_elevator_end_door_onward_v1`.
    left: Option<FormKey>,
    t: f64,
    last: Option<Point>,
    still: bool,
    visibility: crate::state::Visibility,
    end: f64,
    pub log: Rc<RefCell<Log>>,
    /// The walking worlds last built, as built (before the live world
    /// and the platforms take them), by cell and build options: the
    /// present one and the one before it. A world built again about a cell
    /// the walker just left is the kept one ([`Walker::load`]): the same
    /// build of the same records, so the same world.
    kept: [Option<(FormKey, crate::world::Options, crate::world::Built)>; 2],
    /// Told when a walking world is about to be built about a cell
    /// (`Some`) and when it is built (`None`): a window that draws what the
    /// walker walks in can start loading that cell at once.
    on_load: Option<OnLoad>,
    /// References whose activation also opens them
    /// (`play_activate_opens_v1`, `--play-activate-opens`).
    opens: Vec<FormKey>,
    /// References whose caller's timed activation the walker's activation
    /// stands for (`play_activate_person_v1`, `--play-activate-person`).
    persons: Vec<FormKey>,
    /// Whether those activations and opens stand for the caller's actions
    /// as written (`play_activate_written_second_v1`,
    /// `--play-action-after-offset`): only the rule is noted here, the run
    /// reads the seconds from its switches.
    written: bool,
}

/// What [`Walker::with_on_load`] calls: the cell a walking world is about
/// to be built about, then `None` when it is built.
pub type OnLoad = Box<dyn FnMut(Option<&FormKey>) + Send>;

impl Walker {
    /// `refs`: the placed reference of each source of `world`
    /// ([`pick::references`]). The input is over at its `end`, else at its
    /// last command.
    pub fn new(
        world: World,
        refs: Vec<Option<FormKey>>,
        script: Script,
        opts: Options,
    ) -> Result<Walker, String> {
        opts.params.validate().map_err(|e| e.to_string())?;
        opts.settings.validate().map_err(|e| e.to_string())?;
        if opts.stand.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
            return Err(
                "PlayStand: the longest standing slice is a positive number of seconds".into(),
            );
        }
        let activates = script.events.iter().any(|e| e.command == Command::Activate);
        if activates && !opts.reach.is_some_and(|r| r.is_finite() && r > 0.0) {
            return Err("PlayReach: the input activates; give the reach (--play-reach UNITS, the caller's assumption)".into());
        }
        let end = script
            .end()
            .or_else(|| script.events.last().map(|e| e.t))
            .unwrap_or(0.0);
        let live = Live::fixed(world, refs).map_err(|e| e.to_string())?;
        Ok(Walker {
            elevator_options: None,
            elevators: Vec::new(),
            grid: None,
            live,
            state: RunState::reading_doors(opts.held_doors),
            held_logged: Default::default(),
            toggled_open: Default::default(),
            nowait: false,
            doors: None,
            door: None,
            player_moves: None,
            player_sits: None,
            moved: None,
            boxes: None,
            actor_pick: None,
            boxes_changed: 0,
            statics_out: false,
            script,
            arrivals: None,
            known: f64::INFINITY,
            opts,
            next_event: 0,
            controls: Controls::default(),
            character: None,
            space: None,
            left: None,
            t: 0.0,
            last: None,
            still: true,
            visibility: Default::default(),
            end,
            log: Rc::new(RefCell::new(Log {
                rules: collision_rules(&opts),
                ..Log::default()
            })),
            kept: [None, None],
            on_load: None,
            opens: Vec::new(),
            persons: Vec::new(),
            written: false,
        })
    }

    /// The walker's activation of one of `opens` gives the run the open
    /// too, at the same second, after the activation
    /// (`play_activate_opens_v1`: the caller's timed open of it left out
    /// of the run's switches, `crate::options::Play::run_args`).
    pub fn with_opens(mut self, opens: Vec<FormKey>) -> Walker {
        self.opens = opens;
        self
    }

    /// Tells `f` when a walking world is about to be built again about a
    /// cell and when it is built (a grid square left, a load door, a
    /// `MoveTo`, a sit): what a window uses to load the cell's drawing
    /// while the walker builds. Changes nothing the walker does.
    pub fn with_on_load(mut self, f: OnLoad) -> Walker {
        self.on_load = Some(f);
        self
    }

    /// Lets the activation meet the object bounds of references with no
    /// collision (`play_activate_object_bounds_v1`, `--play-pick-bounds`).
    pub fn with_boxes(mut self, boxes: pick::Boxes) -> Walker {
        self.boxes = Some(self.platform_boxes_out(boxes));
        self.boxes_changed += 1;
        self
    }

    /// The walker's activation of one of `persons` is the run's activation
    /// of it in place of the caller's timed one, left out of the run's
    /// switches (`play_activate_person_v1`: `crate::options::Play::run_args`).
    pub fn with_persons(mut self, persons: Vec<FormKey>) -> Walker {
        self.persons = persons.iter().map(norm).collect();
        self
    }

    /// The activations and opens of `persons` and `opens` stand for the
    /// caller's actions they replace as written
    /// (`play_activate_written_second_v1`: the run's `--input-action-at`,
    /// `crate::options::Play::run_args`).
    pub fn with_written_seconds(mut self) -> Walker {
        self.written = true;
        self
    }

    /// With `first_met`, the object bounds of the named platforms (moving
    /// and still) left out: the walking world holds their collision
    /// (`play_activate_ray_first_met_v1`).
    fn platform_boxes_out(&self, boxes: pick::Boxes) -> pick::Boxes {
        match (&self.elevator_options, self.opts.first_met) {
            (Some(e), true) => {
                let mut keys: Vec<FormKey> = e.references.clone();
                keys.extend(e.also.iter().cloned());
                keys.extend(e.also_with.iter().map(|(k, _)| k.clone()));
                let keys: Vec<FormKey> = keys.iter().map(norm).collect();
                let kept = boxes.without_references(&keys);
                if kept.items.len() < boxes.items.len() {
                    self.log
                        .borrow_mut()
                        .rules
                        .insert(crate::rules::ACTIVATE_FIRST_MET);
                }
                kept
            }
            _ => boxes,
        }
    }

    /// Leaves out the object bounds of statics in the walking worlds built
    /// later too (`play_activate_bounds_not_static_v1`); the caller filters
    /// the boxes it gives [`Walker::with_boxes`].
    pub fn with_static_boxes_left_out(mut self) -> Walker {
        self.statics_out = true;
        self
    }

    /// Walks a live world instead (`--play-live-world`,
    /// `play_live_collision_follows_run_v1`): the run's state at each slice
    /// changes what the walker collides with and what the activation meets;
    /// `nowait` reads the NoWait forms as the plain natives.
    pub fn with_live(mut self, live: Live, nowait: bool) -> Walker {
        self.live = live;
        self.nowait = nowait;
        self
    }

    /// Crosses the load doors the activation meets
    /// (`play_load_door_activation_v1`).
    pub fn with_doors(mut self, doors: Doors) -> Walker {
        self.doors = Some(doors);
        self
    }

    /// Builds the walking world again about the walker's grid square when
    /// it leaves it (`--play-follow-grid`).
    pub fn with_grid(mut self, grid: Grid) -> Walker {
        self.grid = Some(grid);
        self
    }

    /// Moving platforms (`--play-elevator`, `play_elevator_run_sequence_v1`):
    /// built at the first slice beside the walking world.
    pub fn with_elevators(mut self, e: Elevators) -> Walker {
        self.elevator_options = Some(e);
        self
    }

    /// Follows the run's `MoveTo` calls on the player
    /// (`--play-player-moveto`, `play_player_moveto_v1`).
    pub fn with_player_moves(mut self) -> Walker {
        self.player_moves = Some(0);
        self
    }

    /// Follows the run's sits of the player (`--play-player-sits`,
    /// `play_player_sit_furniture_v1`).
    pub fn with_player_sits(mut self) -> Walker {
        self.player_sits = Some(0);
        self
    }

    /// Takes its commands from a source as they arrive (a person at a
    /// window) instead of the script, which must hold none. Needs the
    /// longest standing slice ([`Options::stand`]); the input is over at
    /// the first `end` that arrives.
    pub fn with_arrivals(mut self, source: Box<dyn Arrivals>) -> Result<Walker, String> {
        if self.opts.stand.is_none() {
            return Err("PlayStand: commands that arrive while the walker runs need the longest standing slice (--play-stand SECONDS)".into());
        }
        if !self.script.events.is_empty() {
            return Err("PlayInput: a scripted input and arriving commands together".into());
        }
        self.arrivals = Some(source);
        self.known = f64::NEG_INFINITY;
        self.end = f64::INFINITY;
        Ok(self)
    }

    /// Knows every command up to `until`: with arriving commands, waits for
    /// them (a script knows all of its own).
    fn know(&mut self, until: f64) -> Result<(), String> {
        while self.known < until {
            let feet = self.character.as_ref().map(|c| c.feet);
            let platforms = (!self.elevators.is_empty()).then(|| self.platforms());
            let source = self.arrivals.as_mut().ok_or("PlayNoSource")?;
            if let Some(p) = platforms {
                source.platforms(p);
            }
            let (events, horizon) = source
                .wait(until, self.t, feet)
                .ok_or_else(|| SOURCE_CLOSED.to_string())?;
            for e in events {
                let last = self.script.events.last().map_or(f64::NEG_INFINITY, |l| l.t);
                if !(e.t.is_finite() && e.t > self.known && e.t >= last) {
                    return Err(format!(
                        "PlayArrival: a command at {} s after the commands known up to {} s",
                        e.t, self.known
                    ));
                }
                if e.command == Command::End && self.end.is_infinite() {
                    self.end = e.t;
                }
                self.script.events.push(e);
            }
            if horizon <= self.known {
                return Err("PlayArrival: the source's horizon did not move".into());
            }
            self.known = horizon;
        }
        Ok(())
    }

    /// The second of the next command not applied (or the input's end),
    /// knowing the commands up to `bound` at least: past `bound` the value
    /// says only that no command comes before it, and callers read it no
    /// further.
    fn next_due(&mut self, bound: f64) -> Result<f64, String> {
        self.know(bound)?;
        Ok(self
            .script
            .events
            .get(self.next_event)
            .map_or(self.end, |e| e.t)
            .min(self.end))
    }

    /// The reference the activation key would meet now and how far, read
    /// as an activation reads it (nothing logged): what is under the
    /// crosshair. `None` before the walker stands somewhere.
    pub fn aim(&self) -> Option<pick::Hit> {
        self.ray().and_then(|(_, _, hit)| hit)
    }

    /// Reads the `MoveTo` calls the run made on the player
    /// (`play_player_moveto_v1`) and the sits it gave the player
    /// (`play_player_sit_furniture_v1`) since the last look, each under its
    /// switch: the last of them in the run's order is the place the walker
    /// goes to; a `MoveTo` with an offset or an open target, and a sit in a
    /// furniture with no stored place, are logged as not followed.
    /// `--play-pick-actors` (`play_activate_actor_box_v1`): the run's
    /// actors' places read again, and the boxes the ray meets built again
    /// when they or the walker's boxes changed.
    fn follow_actor_places(&mut self, run: &View<'_, '_>) {
        let w = &run.vm.world;
        let moved: Vec<(f64, FormKey, FormKey, [f64; 3])> = w
            .place
            .native_at
            .iter()
            .filter_map(|(id, (t, space, pos))| {
                let k = w
                    .objects
                    .get(*id)?
                    .key
                    .as_deref()
                    .and_then(fo4_vm::player::parse_key)?;
                (run.store.signature(&k) == Some(*b"ACHR")).then(|| (*t, k, space.clone(), *pos))
            })
            .collect();
        let places = actor_places(&w.place, &moved);
        let fresh = self
            .actor_pick
            .as_ref()
            .is_none_or(|(p, n, _)| *p != places || *n != self.boxes_changed);
        if !fresh {
            return;
        }
        let vis = &self.visibility;
        let mut boxes = match &self.space {
            Some(space) => actor_boxes(&places, space, &|k| vis.disabled(k)),
            None => pick::Boxes::default(),
        };
        if let Some(b) = &self.boxes {
            let mut all = b.clone();
            all.kinds.resize(all.items.len(), None);
            all.modelled.resize(all.items.len(), None);
            all.items.append(&mut boxes.items);
            all.kinds.append(&mut boxes.kinds);
            all.modelled.append(&mut boxes.modelled);
            boxes = all;
        }
        self.actor_pick = Some((places, self.boxes_changed, boxes));
    }

    fn follow_player_places(&mut self, run: &View<'_, '_>) {
        let mut found: Vec<(f64, Result<Place, String>)> = Vec::new();
        if let Some(seen) = self.player_moves {
            let moves = &run.vm.world.place.player_moves;
            for m in moves.iter().skip(seen) {
                let r = move_place(m);
                found.push((m.t, r));
            }
            self.player_moves = Some(moves.len());
        }
        if let Some(seen) = self.player_sits {
            let w = &run.vm.world;
            let changes = &w.sits.changes;
            for (t, sitter, furniture, sits) in changes.iter().skip(seen) {
                if !*sits || w.player != Some(*sitter) {
                    continue;
                }
                let key = w.objects[*furniture]
                    .key
                    .as_deref()
                    .and_then(fo4_vm::player::parse_key);
                let r = match key {
                    Some(k) => {
                        let at = fo4_vm::track::stored_position(
                            run.store,
                            run.store.limits.max_memory,
                            &k,
                        )
                        .map_err(|e| e.to_string());
                        sit_place(k, at, self.space.as_ref())
                    }
                    None => Err(
                        "a sit of the player not followed: the furniture is not a placed reference"
                            .into(),
                    ),
                };
                found.push((*t, r));
            }
            self.player_sits = Some(changes.len());
        }
        let mut log = self.log.borrow_mut();
        if self.character.is_none() {
            // Before the walker's start the caller's first point stands
            // for where the player is.
            for (t, _) in found {
                log.player_moves.push((
                    t,
                    "before the walker's start: the caller's first point stands for it".into(),
                ));
            }
            return;
        }
        let (last, notes) = latest_place(found);
        log.player_moves.extend(notes);
        if last.is_some() {
            if let (Some((k, _, rule)), Some((t, _))) = (&self.moved, &last) {
                log.player_moves.push((
                    *t,
                    format!(
                        "a later place replaces one not yet reached ({}, {rule})",
                        fo4_vm::bind::record_key(k)
                    ),
                ));
            }
            self.moved = last.map(|(_, x)| x);
        }
    }

    /// Whether a platform moves between two seconds.
    fn elevators_moving(&self, from: f64, to: f64) -> bool {
        self.elevators.iter().any(|e| e.moving(from, to))
    }

    /// Builds the platforms the caller named, once, and puts them into the
    /// walking world.
    fn build_elevators(&mut self, run: &View<'_, '_>) -> Result<(), String> {
        let Some(o) = self.elevator_options.clone() else {
            return Ok(());
        };
        if !self.elevators.is_empty() || o.references.is_empty() {
            return Ok(());
        }
        let (m, vfs) = run.sources;
        // The space the walking world stands in: the walker's, else the
        // run's first point's.
        let here = self.space.clone().or_else(|| {
            run.vm
                .world
                .place
                .track
                .as_ref()
                .and_then(|t| t.points.first())
                .map(|p| p.space.clone())
        });
        let here = here.map(|s| norm(&s));
        // Each still reference goes with the first named reference placed
        // in its own space; one in no named reference's space goes with
        // the first.
        let max = run.store.limits.max_memory;
        let space_of = |k: &FormKey| {
            fo4_vm::track::stored_position(run.store, max, k)
                .ok()
                .map(|(s, _)| norm(&s))
        };
        let spaces: Vec<Option<FormKey>> = o.references.iter().map(space_of).collect();
        let mut alsos: Vec<Vec<FormKey>> = vec![Vec::new(); o.references.len()];
        for k in &o.also {
            alsos[crate::elevator::still_owner(&spaces, &space_of(k))].push(k.clone());
        }
        for (k, owner) in &o.also_with {
            let i = o.references.iter().position(|r| r == owner).ok_or(
                "PlayElevator: --play-elevator-also-with names no --play-elevator reference",
            )?;
            alsos[i].push(k.clone());
        }
        for (i, k) in o.references.iter().enumerate() {
            let also: &[FormKey] = &alsos[i];
            let readings = crate::elevator::Readings {
                held_pose: o.held_pose,
                rigid: o.rigid,
            };
            let mut e =
                crate::elevator::Elevator::build(m, run.store, vfs, k, also, i as u32, readings)
                    .map_err(|e| format!("PlayElevator {}: {e}", fo4_vm::bind::record_key(k)))?;
            e.landing = o.door_landing;
            e.pass_terrain = o.through_terrain;
            if here.as_ref() == Some(&e.space) {
                e.insert(self.live.space_mut(), self.t)
                    .map_err(|e| e.to_string())?;
            }
            let mut log = self.log.borrow_mut();
            log.elevators.push((
                self.t,
                format!("{}: {}", fo4_vm::bind::record_key(k), e.summary()),
            ));
            log.rules.insert(crate::rules::ELEVATOR_RUN_SEQUENCE);
            log.rules
                .insert(fo4_scene::hypotheses::ANIMATED_COLLISION_AS_STORED);
            log.rules.insert(fo4_walk::platform::rules::PLATFORM_RIDE);
            if o.through_terrain {
                log.rules
                    .insert(fo4_walk::platform::rules::RIDE_PASSES_TERRAIN);
            }
            if o.rigid {
                log.rules.insert(crate::rules::ELEVATOR_RIGID);
                log.rules.insert(fo4_scene::hypotheses::SEQUENCE_CHAIN_POSE);
                log.rules.insert(fo4_walk::platform::rules::RIGID_MOTION);
            } else if o.held_pose {
                log.elevators.push((
                    self.t,
                    format!(
                        "{}: {} (node, sequence) track(s) from a held translation",
                        fo4_vm::bind::record_key(k),
                        e.held
                    ),
                ));
                log.rules.insert(crate::rules::ELEVATOR_HELD_POSE);
                log.rules.insert(fo4_scene::hypotheses::SEQUENCE_HELD_POSE);
            }
            drop(log);
            self.elevators.push(e);
        }
        Ok(())
    }

    /// Reads the sequences the run started on the platforms; with the end
    /// door, takes the walker through the nearest load door when a ride
    /// that carried it ended (`play_elevator_end_load_door_v1`).
    fn follow_elevators(&mut self, run: &View<'_, '_>) {
        let door = self.elevator_options.as_ref().is_some_and(|o| o.door);
        let onward = self
            .elevator_options
            .as_ref()
            .is_some_and(|o| o.door_onward);
        for e in &mut self.elevators {
            let before = e.log.len();
            e.follow(run.vm);
            let ended = e.ended.take();
            let mut log = self.log.borrow_mut();
            for (t, x) in &e.log[before..] {
                if x.ends_with("it landed on the ride") {
                    log.rules.insert(crate::rules::ELEVATOR_END_LANDING);
                }
                log.elevators.push((
                    *t,
                    format!("{}: {x}", fo4_vm::bind::record_key(&e.reference)),
                ));
            }
            let (Some(at), true, Some(c), Some(space), Some(doors)) =
                (ended, door, &self.character, &self.space, &self.doors)
            else {
                continue;
            };
            let (m, _) = run.sources;
            let reach = self.opts.reach.unwrap_or(0.0);
            let Some(bounds) = e.bounds() else {
                log.elevators.push((
                    at,
                    "the ride ended: the reference has no collision box".into(),
                ));
                continue;
            };
            // With the onward reading, the doors back into the space the
            // walker came from are left out.
            let onward_doors;
            let candidates = match (onward, &self.left) {
                (true, Some(left)) => {
                    onward_doors = crate::elevator::onward(&doors.doors, left);
                    let n = doors.doors.len() - onward_doors.len();
                    if n > 0 {
                        log.rules.insert(crate::rules::ELEVATOR_END_DOOR_ONWARD);
                        log.elevators.push((
                            at,
                            format!("the ride ended: {n} of the walking world's load doors lead back into the space the walker came from, left out"),
                        ));
                    }
                    &onward_doors
                }
                _ => &doors.doors,
            };
            match crate::elevator::nearest_door(
                run.store,
                m.limits.max_memory,
                candidates,
                space,
                c.feet,
                bounds,
                reach,
            ) {
                Some((k, d, dist)) => {
                    log.rules.insert(crate::rules::ELEVATOR_END_DOOR);
                    log.elevators.push((
                        at,
                        format!(
                            "the ride ended: load door {} at {dist:.1} units takes the walker",
                            fo4_vm::bind::record_key(k)
                        ),
                    ));
                    if self.door.is_none() {
                        self.door = Some((k.clone(), d.clone()));
                    }
                }
                None => log.elevators.push((
                    at,
                    "the ride ended: no load door inside the reference's collision box".into(),
                )),
            }
        }
    }

    /// Whether the person's activation toggles doors
    /// ([`Options::door_toggle`], with the held doors and the open pose).
    fn toggles(&self) -> bool {
        self.opts.door_toggle && self.opts.held_doors && self.opts.door_open
    }

    /// With [`Options::door_toggle_blocked`] (and the toggle): whether the
    /// person's activation of door `k` leaves it as it stands, the held
    /// doors' reading showing it open while the run's scripts last blocked
    /// its activation (`play_door_toggle_blocked_unchanged_v1`), for a
    /// window's prompt. Reading it changes nothing.
    pub fn door_kept(&self, k: &FormKey) -> bool {
        let k = norm(k);
        self.toggles()
            && self.opts.door_toggle_blocked
            && door_blocked(self.state.doors.get(&k))
            && self.held_door_states().get(&k) == Some(&false)
    }

    /// The walking world as it stands.
    pub fn live(&self) -> &Live {
        &self.live
    }

    /// The held doors the walker's walking worlds have held so far
    /// (`--play-held-doors`), each with whether its box stands now, for a
    /// window to draw them so. Reading them changes nothing.
    ///
    /// With [`Options::door_toggle`], the doors the present walking world
    /// stands in their open pose come first as open (no box), until the
    /// log says otherwise (`play_door_activation_toggles_v1`).
    pub fn held_door_states(&self) -> std::collections::BTreeMap<FormKey, bool> {
        let mut out = std::collections::BTreeMap::new();
        if self.toggles() {
            for k in self.live.open_pose() {
                out.insert(k.clone(), false);
            }
            if self.opts.door_stored_open {
                for k in self.live.stored_open() {
                    out.insert(k.clone(), false);
                }
            }
        }
        for (_, k, held, _) in &self.log.borrow().held_doors {
            out.insert(k.clone(), *held);
        }
        out
    }

    /// The collision markers' boxes of the walking world as loaded, with
    /// their bounds and whether each stands now
    /// ([`crate::live::Live::marker_boxes`]), for a window to draw what
    /// they hold (`play_static_leaf_two_pose_v1`). Reading them changes
    /// nothing.
    pub fn marker_box_states(
        &self,
    ) -> std::collections::BTreeMap<FormKey, crate::live::MarkerBoxState> {
        self.live.marker_boxes()
    }

    /// The named platforms' moving parts where their collision stands
    /// (`--play-elevator`; empty before the first slice builds them), for a
    /// window to draw them there: per named reference, its model, the
    /// nodes the parts are read by and each part's rigid motion
    /// ([`crate::elevator::Platform`]). Reading them changes nothing.
    /// Each part says whether the walker stands on it now
    /// ([`crate::elevator::PartMotion::carries`]).
    pub fn platforms(&self) -> Vec<crate::elevator::Platform> {
        let ground = self.character.as_ref().and_then(|c| {
            let g = c.ground.filter(|_| c.grounded)?;
            self.live.space().source(g.source).map(|(k, _)| *k)
        });
        self.elevators
            .iter()
            .map(|e| e.platform_carrying(ground))
            .collect()
    }

    /// Places the walker at the run's first point, settled on the ground.
    fn start(&mut self, now: f64, first: Option<&Point>) -> Result<Vec<Point>, String> {
        let first = first.ok_or("PlayNoTrack")?.clone();
        let mut c = Character::new(self.opts.params, self.opts.settings, first.pos)
            .map_err(|e| e.to_string())?;
        let ticks = c.settle(self.live.space(), self.opts.settle_ticks);
        c.ticks = 0;
        self.t = now;
        self.space = Some(first.space.clone());
        let mut log = self.log.borrow_mut();
        log.start = Some((first.clone(), c.feet, ticks));
        log.rules.insert(crate::rules::PLAYER_FEET);
        log.rules.insert(crate::rules::RUN_CLOCK);
        drop(log);
        let mut out = Vec::new();
        let p = Point {
            t: self.t,
            space: first.space.clone(),
            pos: c.feet,
            facing: None,
        };
        if p.pos != first.pos {
            out.push(p.clone());
        }
        self.last = Some(p);
        self.character = Some(c);
        Ok(out)
    }

    /// The references whose caller's timed activation the walker's
    /// activation stands for (`--play-activate-person`), normalised.
    pub fn persons(&self) -> &[FormKey] {
        &self.persons
    }

    /// Whether the straight segment from `from` to `to` meets no collision
    /// primitive of the walking world as the walker stands in it now (the
    /// sources shown, the references the run made absent passed through;
    /// terrain stops it). `None` before the walker has a world. Reads only;
    /// the window's cue asks it (viewer policy).
    pub fn clear_line(&self, from: [f64; 3], to: [f64; 3]) -> Option<bool> {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-6 {
            return Some(true);
        }
        let live = &self.live;
        let vis = &self.visibility;
        let hit = if live.follows() {
            live.pick(None, from, d, len, &|k| {
                live.decision(k) == Some(Decision::Absent)
            })
        } else {
            pick::met(
                live.base()?,
                live.base_refs(),
                None,
                from,
                d,
                len,
                &|_| true,
                &|k| vis.disabled(k),
                false,
            )
            .map(|(h, _)| h)
        };
        Some(hit.is_none())
    }

    /// Whether the walker's character, from where it stands now, walks the
    /// straight line on the ground to `to` in the walking world as it is
    /// (`fo4_walk::Character::walks_to`). `None` before the walker has a
    /// character. Reads only; the window's cue asks it (viewer policy).
    pub fn walks_to(&self, to: [f64; 3]) -> Option<bool> {
        let c = self.character.as_ref()?;
        Some(c.walks_to(self.live.space(), to))
    }

    /// The highest surface of the walking world as the walker stands in it
    /// now (hidden sources passed through) at or below `at` on the
    /// vertical line through it: its height and the upward part of its
    /// normal. `None` when nothing is there. Reads only; the window's cast
    /// asks it (`--draw-cast-settles`, viewer).
    pub fn ground_below(&self, at: [f64; 3]) -> Option<(f64, f64)> {
        self.live
            .space()
            .ray_down(at[0], at[1], at[2])
            .map(|h| (h.z, h.normal[2]))
    }

    /// Whether the run made the reference absent as the walker reads the
    /// run (the live world's decision when it follows, else the run's last
    /// enable or disable of it). Reads only; the window's use cue asks it
    /// (viewer policy).
    pub fn made_absent(&self, k: &FormKey) -> bool {
        if self.live.follows() {
            self.live.decision(k) == Some(Decision::Absent)
        } else {
            self.visibility.disabled(k) || self.visibility.disabled(&norm(k))
        }
    }

    /// The eye's height above the feet of the walker's character, when it
    /// stands in a world.
    pub fn eye_height(&self) -> Option<f64> {
        let c = self.character.as_ref()?;
        Some(c.eye()[2] - c.feet[2])
    }

    /// The eye, the look and what the activation's ray meets from there.
    fn ray(&self) -> Option<([f64; 3], [f64; 3], Option<pick::Hit>)> {
        let c = self.character.as_ref()?;
        let eye = c.eye();
        let dir = self.controls.look();
        let reach = self.opts.reach.unwrap_or(0.0);
        let vis = &self.visibility;
        let live = &self.live;
        let first_met = self.opts.first_met;
        let boxes = match &self.actor_pick {
            Some((_, _, b)) => Some(b),
            None => self.boxes.as_ref(),
        };
        let hit = if live.follows() {
            live.pick_ranked(
                boxes,
                eye,
                dir,
                reach,
                &|k| live.decision(k) == Some(Decision::Absent),
                first_met,
            )
        } else {
            pick::met(
                live.base()?,
                live.base_refs(),
                boxes,
                eye,
                dir,
                reach,
                &|_| true,
                &|k| vis.disabled(k),
                first_met,
            )
            .map(|(h, _)| h)
        };
        Some((eye, dir, hit))
    }

    fn activate(&mut self) -> Option<Act> {
        let (eye, dir, hit) = self.ray()?;
        if let Some(pick::Hit {
            via: pick::Via::Bounds { .. },
            ..
        }) = &hit
        {
            let mut log = self.log.borrow_mut();
            log.rules.insert(crate::rules::ACTIVATE_BOUNDS);
            if self.statics_out {
                log.rules.insert(crate::rules::BOUNDS_NOT_STATIC);
            }
        }
        let mut log = self.log.borrow_mut();
        log.rules.insert(crate::rules::ACTIVATE_RAY);
        if self.opts.first_met {
            log.rules.insert(crate::rules::ACTIVATE_FIRST_MET);
        }
        if let Some(k) = hit.as_ref().and_then(|h| h.reference.as_ref()) {
            if self
                .actor_pick
                .as_ref()
                .is_some_and(|(places, _, _)| places.contains_key(&norm(k)))
            {
                log.rules.insert(crate::rules::ACTIVATE_ACTOR_BOX);
            }
            if self.persons.contains(&norm(k)) {
                log.rules.insert(crate::rules::ACTIVATE_PERSON);
                if self.written {
                    log.rules.insert(crate::rules::ACTIVATE_WRITTEN_SECOND);
                }
            }
        }
        let (record, act) = match hit {
            None => (Err("nothing within reach".to_string()), None),
            Some(h) => match h.reference {
                None => (
                    Err(format!(
                        "a primitive of no placed reference at {:.1} units",
                        h.distance
                    )),
                    None,
                ),
                Some(k) => {
                    if let Some(d) = self.doors.as_ref().and_then(|d| d.doors.get(&norm(&k))) {
                        self.door = Some((k.clone(), d.clone()));
                    }
                    (
                        Ok((k.clone(), h.distance)),
                        Some(Act {
                            at: self.t,
                            action: PlayerAction::Activate,
                            reference: k,
                        }),
                    )
                }
            },
        };
        log.activations.push((self.t, eye, dir, record));
        act
    }

    /// The commands due by the walker's present second: the controls they
    /// change and the actions they make.
    fn commands(&mut self) -> Result<Vec<Act>, String> {
        self.know(self.t)?;
        let mut actions = Vec::new();
        while let Some(e) = self.script.events.get(self.next_event).copied() {
            if e.t > self.t {
                break;
            }
            self.next_event += 1;
            match e.command {
                Command::Activate => {
                    let act = self.activate();
                    // `play_door_activation_toggles_v1`: a door the held
                    // doors' reading shows open is closed, one the person
                    // closed is opened again.
                    // `play_door_toggle_blocked_unchanged_v1`: a door the
                    // reading shows open whose activation the run last
                    // blocked is left as it stands, no open given either.
                    let kept = match act.as_ref() {
                        Some(a) if self.toggles() && self.opts.door_toggle_blocked => {
                            let k = norm(&a.reference);
                            let kept = door_blocked(self.state.doors.get(&k))
                                && self.held_door_states().get(&k) == Some(&false);
                            if kept {
                                self.log
                                    .borrow_mut()
                                    .rules
                                    .insert(crate::rules::DOOR_TOGGLE_BLOCKED);
                            }
                            kept
                        }
                        _ => false,
                    };
                    let toggled = match act.as_ref() {
                        Some(a) if self.toggles() => {
                            let k = norm(&a.reference);
                            let t = door_toggle(
                                &self.held_door_states(),
                                self.state.doors.get(&k),
                                &k,
                                self.opts.door_toggle_blocked,
                            );
                            match t {
                                Some(false) => self.state.close_by_player(k),
                                Some(true) => {
                                    self.state.open_by_player(k.clone());
                                    self.toggled_open.insert(k);
                                }
                                None => {}
                            }
                            if t.is_some() {
                                self.log
                                    .borrow_mut()
                                    .rules
                                    .insert(crate::rules::DOOR_TOGGLE);
                            }
                            t
                        }
                        _ => None,
                    };
                    let opens = act
                        .as_ref()
                        .filter(|_| toggled != Some(false) && !kept)
                        .filter(|a| self.opens.contains(&norm(&a.reference)))
                        .map(|a| Act {
                            at: a.at,
                            action: PlayerAction::Open,
                            reference: a.reference.clone(),
                        });
                    actions.extend(act);
                    if let Some(o) = opens {
                        if self.opts.held_doors && self.opts.held_at_load {
                            // The open the run is given opens the door
                            // for the held doors' reading too.
                            self.state.open_by_player(norm(&o.reference));
                        }
                        let mut log = self.log.borrow_mut();
                        log.rules.insert(crate::rules::ACTIVATE_OPENS);
                        if self.written {
                            log.rules.insert(crate::rules::ACTIVATE_WRITTEN_SECOND);
                        }
                        drop(log);
                        actions.push(o);
                    }
                }
                Command::End => {}
                c => {
                    self.controls.apply(c);
                    self.still = false;
                    self.log.borrow_mut().looks.push((
                        e.t,
                        self.controls.heading,
                        self.controls.pitch,
                    ));
                }
            }
        }
        Ok(actions)
    }

    /// One slice: the walker from its present second to the next tick or
    /// command, with the commands due at the slice's end (an action at a
    /// second reaches the run in the slice that ends there, before the run
    /// does what else is due at it).
    ///
    /// `now` and `first`: the run's present second and its track's first
    /// point (read once, at the first slice).
    pub fn step(&mut self, now: f64, first: Option<&Point>) -> Result<Next, String> {
        let from = self.t;
        let mut out = match self.step_one(now, first)? {
            Next::Slice(s) => s,
            n => return Ok(n),
        };
        // Join ticks while the slice is short, nothing was activated and
        // the walker moves (a standing slice already runs to the next
        // command).
        while self.opts.slice > 0.0
            && out.actions.is_empty()
            && self.door.is_none()
            && out.until - from < self.opts.slice
            && !out.points.is_empty()
        {
            match self.step_one(out.until, first)? {
                Next::Slice(s) => {
                    out.points.extend(s.points);
                    out.actions = s.actions;
                    out.until = s.until;
                }
                _ => break,
            }
        }
        Ok(Next::Slice(out))
    }

    fn step_one(&mut self, now: f64, first: Option<&Point>) -> Result<Next, String> {
        let mut points = Vec::new();
        let mut actions = Vec::new();
        if self.character.is_none() {
            points = self.start(now, first)?;
            actions = self.commands()?;
        }
        if self.t >= self.end && points.is_empty() && actions.is_empty() {
            return Ok(Next::Finish);
        }
        let tick = self.opts.settings.tick;
        if self.controls.target.is_some() {
            let next_command = self.next_due(self.t + tick)?;
            if let Some(c) = &self.character {
                let speed = if self.controls.run {
                    self.opts.params.run_speed
                } else {
                    self.opts.params.walk_speed
                };
                let step = (next_command.max(self.t) - self.t).min(tick);
                if self.controls.steer(c.feet, 0.5 * speed * step.max(1e-9)) {
                    self.log.borrow_mut().arrivals.push((self.t, c.feet));
                }
            }
        }
        let keys = {
            let c = &self.controls;
            c.forward || c.back || c.left || c.right
        };
        let grounded = self.character.as_ref().is_some_and(|c| c.grounded);
        self.log.borrow_mut().slices += 1;
        // Standing: no key, on the ground and not moved by the last tick
        // (nor by a moving platform before the next command, or within the
        // longest standing slice).
        let may_stand = !keys && self.still && grounded;
        let bound = match (may_stand, self.opts.stand) {
            (false, _) => self.t + tick,
            (true, Some(s)) => self.t + s.max(tick),
            (true, None) => f64::INFINITY,
        };
        let next_command = self.next_due(bound)?;
        let ahead = match self.opts.stand {
            Some(s) => next_command.min(self.t + s),
            None => next_command,
        };
        if may_stand && !self.elevators_moving(self.t, ahead) {
            let mut until = next_command.max(self.t);
            if !self.elevators.is_empty() {
                until = until.min(self.t + ELEVATOR_STAND);
            }
            if let Some(s) = self.opts.stand {
                until = until.min(self.t + s);
            }
            self.t = until;
            actions.extend(self.commands()?);
            let mut log = self.log.borrow_mut();
            log.standing_slices += 1;
            log.points += points.len() as u64;
            log.track.extend(points.iter().cloned());
            return Ok(Next::Slice(Slice {
                points,
                until,
                actions,
            }));
        }
        let to = (self.t + tick).min(next_command.max(self.t + 1e-9));
        let dt = to - self.t;
        let c = self.character.as_mut().ok_or("PlayNoWalker")?;
        for e in &mut self.elevators {
            e.advance(self.live.space_mut(), c, to)
                .map_err(|e| e.to_string())?;
        }
        c.settings.tick = dt;
        let started = std::time::Instant::now();
        let s = c.tick(self.live.space(), self.controls.walker());
        let took = started.elapsed().as_secs_f64();
        c.settings.tick = tick;
        {
            let mut log = self.log.borrow_mut();
            log.tick_seconds += took;
            log.ticks += 1;
            log.blocked += u64::from(s.blocked);
            log.stepped += u64::from(s.stepped);
            log.airborne += u64::from(!s.grounded);
            if self.opts.report_walls && s.blocked {
                let what = s.wall.map_or_else(
                    || "no wall contact recorded".to_string(),
                    |w| {
                        let own = self.live.describe_source(w.source);
                        let platform = self.live.space().source(w.source).and_then(|(k, _)| {
                            self.elevators.iter().find_map(|e| e.describe_piece(*k))
                        });
                        // Rounded, and -0 read as 0, so that one
                        // stretch is not split by the normal's noise.
                        let n = w.normal.map(|x| (x * 10.0).round() / 10.0 + 0.0);
                        format!(
                            "{own}{}; normal ({:.1}, {:.1}, {:.1})",
                            platform.map(|p| format!(", {p}")).unwrap_or_default(),
                            n[0],
                            n[1],
                            n[2]
                        )
                    },
                );
                match log.walls.last_mut() {
                    Some(l) if l.3 == what && to - l.1 <= 3.0 * tick + 1e-9 => {
                        l.1 = to;
                        l.2 += 1;
                    }
                    _ => log.walls.push((to, to, 1, what)),
                }
            }
        }
        let space = self.space.clone().ok_or("PlayNoWalker")?;
        let last = self.last.clone().ok_or("PlayNoWalker")?;
        let moved = fo4_walk::geometry::length(fo4_walk::geometry::sub(s.feet, last.pos));
        if moved <= STILL {
            self.still = !keys && s.grounded;
        } else {
            if last.t < self.t {
                points.push(Point {
                    t: self.t,
                    ..last.clone()
                });
            }
            let p = Point {
                t: to,
                space,
                pos: s.feet,
                facing: None,
            };
            points.push(p.clone());
            self.last = Some(p);
            self.still = false;
        }
        self.t = to;
        actions.extend(self.commands()?);
        let mut log = self.log.borrow_mut();
        log.points += points.len() as u64;
        log.track.extend(points.iter().cloned());
        Ok(Next::Slice(Slice {
            points,
            until: to,
            actions,
        }))
    }
}

impl Walker {
    /// Applies the run's state to the live world when it changed.
    fn follow(&mut self, vm: &fo4_vm::Vm<'_>) -> Result<(), String> {
        if !self.live.follows() || !self.state.update(vm) {
            return Ok(());
        }
        self.apply_state()
    }

    fn apply_state(&mut self) -> Result<(), String> {
        let b = Budget::new(1 << 32);
        self.live
            .apply(&self.state, &b)
            .map_err(|e| e.to_string())?;
        let mut log = self.log.borrow_mut();
        log.rules.insert(crate::rules::LIVE_COLLISION);
        if self.opts.held_doors {
            // Changes of the held doors this walking world loaded, against
            // what the walker last logged (a load keeps a door's state).
            let now = self.live.holding();
            for k in self.live.held_doors() {
                let was = self.held_logged.contains(k);
                let is = now.contains(k);
                if was != is {
                    // Held with no script's `SetOpen`: closed at load
                    // (`play_door_closed_at_load_v1`); closed or opened by
                    // the person (`play_door_activation_toggles_v1`).
                    let d = self.state.doors.get(k);
                    let why = if is && d.is_some_and(|d| d.person_closed) {
                        HeldWhy::Person
                    } else if is && d.is_none_or(|d| d.open.is_none()) {
                        HeldWhy::AtLoad
                    } else if !is && self.toggled_open.contains(k) {
                        HeldWhy::Person
                    } else {
                        HeldWhy::Run
                    };
                    log.held_doors.push((self.t, k.clone(), is, why));
                    if is {
                        self.held_logged.insert(k.clone());
                    } else {
                        self.held_logged.remove(k);
                    }
                }
            }
            self.toggled_open.clear();
        }
        Ok(())
    }

    /// Through a load door (or to a `MoveTo`'s target, under `rule`): the
    /// walking world of the destination, the walker settled at the
    /// teleport position, the point given to the run at the walker's
    /// present second.
    fn cross(
        &mut self,
        run: &View<'_, '_>,
        door: FormKey,
        to: LoadDoor,
        rule: &'static str,
    ) -> Result<Next, String> {
        let started = std::time::Instant::now();
        let (m, _) = run.sources;
        let b = Budget::new(m.limits.max_memory);
        let cell = crate::world::start_cell(run.store, &to.space, to.pos, &b)
            .map_err(|e| e.to_string())?;
        let report = self.load(run, &cell)?;
        if let Some(g) = &mut self.grid {
            g.centre = (run.store.signature(&to.space) == Some(*b"WRLD"))
                .then(|| (to.space.clone(), crate::world::square(to.pos)));
        }
        let mut c = Character::new(self.opts.params, self.opts.settings, to.pos)
            .map_err(|e| e.to_string())?;
        c.settle(self.live.space(), self.opts.settle_ticks);
        c.ticks = 0;
        let p = Point {
            t: self.t,
            space: to.space.clone(),
            pos: c.feet,
            facing: None,
        };
        self.left = self.space.replace(to.space.clone());
        self.last = Some(p.clone());
        self.still = false;
        let mut log = self.log.borrow_mut();
        log.rules.insert(rule);
        log.doors.push(Crossed {
            rule,
            t: self.t,
            door,
            to,
            cell,
            feet: c.feet,
            seconds: started.elapsed().as_secs_f64(),
            report,
        });
        log.points += 1;
        log.track.push(p.clone());
        drop(log);
        self.character = Some(c);
        Ok(Next::Slice(Slice {
            points: vec![p],
            until: self.t,
            actions: Vec::new(),
        }))
    }

    /// With `--play-follow-grid`: when the walker's feet stand in another
    /// grid square of a worldspace than the one the walking world was
    /// built about, the walking world is built again about the cell
    /// there (a policy of the play mode, as the viewer's moving grid).
    fn follow_grid(&mut self, run: &View<'_, '_>) -> Result<(), String> {
        let (Some(g), Some(c), Some(space)) = (&self.grid, &self.character, &self.space) else {
            return Ok(());
        };
        if run.store.signature(space) != Some(*b"WRLD") {
            return Ok(());
        }
        let sq = crate::world::square(c.feet);
        let here = Some((space.clone(), sq));
        if g.centre.is_none() {
            if let Some(g) = &mut self.grid {
                g.centre = here;
            }
            return Ok(());
        }
        if g.centre == here {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let (m, _) = run.sources;
        let b = Budget::new(m.limits.max_memory);
        let space = space.clone();
        let feet = c.feet;
        let cell =
            crate::world::start_cell(run.store, &space, feet, &b).map_err(|e| e.to_string())?;
        self.load(run, &cell)?;
        if let Some(g) = &mut self.grid {
            g.centre = here;
        }
        self.log
            .borrow_mut()
            .grid
            .push((self.t, cell, feet, started.elapsed().as_secs_f64()));
        Ok(())
    }

    /// Builds the walking world about `cell` in place of the present one
    /// (the live world's state applied again); returns its report.
    fn load(&mut self, run: &View<'_, '_>, cell: &FormKey) -> Result<String, String> {
        let around = self
            .grid
            .as_ref()
            .map(|g| g.around)
            .or(self.doors.as_ref().map(|d| d.around))
            .unwrap_or(0);
        let (m, vfs) = run.sources;
        let follows = self.live.follows();
        let options = crate::world::Options {
            around,
            live: follows,
            doors: self.doors.is_some(),
            collision: self.opts.collision,
            child_flag_agrees: self.opts.child_flag_agrees,
            marker_boxes: self.opts.marker_boxes,
            marker_box_layers: self.opts.marker_box_layers,
            marker_box_layer_records: self.opts.marker_box_layer_records,
            held_doors: self.opts.held_doors,
            held_at_load: self.opts.held_at_load,
            door_open: self.opts.door_open,
            stored_open: self.opts.door_stored_open,
        };
        if let Some(f) = self.on_load.as_mut() {
            f(Some(cell));
        }
        let same = |k: &Option<(FormKey, crate::world::Options, crate::world::Built)>| {
            k.as_ref()
                .is_some_and(|(c, o, _)| c == cell && *o == options)
        };
        let mut built = match self.kept.iter().position(same) {
            Some(i) => self.kept[i].clone().map(|k| k.2),
            None => None,
        };
        if built.is_none() {
            let b = crate::world::build(m, run.store, vfs, cell, options);
            built = Some(b.map_err(|e| e.to_string())?);
        }
        let mut built = built.ok_or("PlayWalkWorld: no world")?;
        if !same(&self.kept[0]) {
            self.kept[1] = self.kept[0].take();
            self.kept[0] = Some((cell.clone(), options, built.clone()));
        }
        if let Some(f) = self.on_load.as_mut() {
            f(None);
        }
        let stats = self.live.stats.clone();
        self.live = match built.live.take() {
            Some((dormant, info)) => Live::new(
                built.world,
                built.refs,
                dormant,
                info,
                built.space.clone(),
                self.nowait,
            )
            .map(|l| {
                l.with_child_flag_agrees(self.opts.child_flag_agrees)
                    .with_unpicked(&built.unpicked)
                    .with_held_doors(std::mem::take(&mut built.held))
                    .with_closed_at_load(persons_doors(
                        std::mem::take(&mut built.closed_at_load),
                        &self.opens,
                    ))
                    .with_open_pose(std::mem::take(&mut built.open_pose))
                    .with_stored_open(std::mem::take(&mut built.stored_open))
            }),
            None => Live::fixed(built.world, built.refs),
        }
        .map_err(|e| e.to_string())?;
        self.live.stats = stats;
        if follows {
            self.apply_state()?;
        }
        if self.boxes.is_some() {
            let boxes = if self.statics_out {
                built.boxes.without_kinds(&crate::rules::STATIC_KINDS)
            } else {
                built.boxes
            };
            self.boxes = Some(self.platform_boxes_out(boxes));
            self.boxes_changed += 1;
        }
        if let Some(d) = &mut self.doors {
            d.doors = built.doors;
        }
        let t = self.t;
        let here = norm(&built.space);
        for e in &mut self.elevators {
            if e.space == here {
                e.insert(self.live.space_mut(), t)
                    .map_err(|e| e.to_string())?;
            } else {
                e.leave();
            }
        }
        Ok(built.report)
    }
}

impl Driver for Walker {
    fn next(&mut self, run: &View<'_, '_>) -> Next {
        let _ = self.visibility.update(run.vm);
        let followed = self
            .build_elevators(run)
            .and_then(|()| self.follow_grid(run))
            .and_then(|()| self.follow(run.vm));
        if followed.is_ok() {
            self.follow_elevators(run);
            self.follow_player_places(run);
            if self.opts.pick_actors {
                self.follow_actor_places(run);
            }
        }
        let next = match self.door.take() {
            Some((door, to)) => Some((door, to, crate::rules::LOAD_DOOR)),
            None => self.moved.take(),
        };
        let crossed = match (followed, next) {
            (Err(why), _) => Some(Err(why)),
            (Ok(()), Some((door, to, rule))) => Some(self.cross(run, door, to, rule)),
            (Ok(()), None) => None,
        };
        let stats = self.live.stats.clone();
        if self.live.follows() {
            self.log.borrow_mut().live = Some(stats);
        }
        match crossed {
            Some(Ok(n)) => return n,
            Some(Err(why)) => {
                self.log.borrow_mut().activations.push((
                    self.t,
                    [0.0; 3],
                    [0.0; 3],
                    Err(format!("the walker stopped: {why}")),
                ));
                return Next::Stop;
            }
            None => {}
        }
        let first = run
            .vm
            .world
            .place
            .track
            .as_ref()
            .and_then(|t| t.points.first());
        match self.step(run.now(), first) {
            Ok(n) => n,
            Err(why) => {
                self.log.borrow_mut().activations.push((
                    self.t,
                    [0.0; 3],
                    [0.0; 3],
                    Err(format!("the walker stopped: {why}")),
                ));
                Next::Stop
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closed_at_load_reading_keeps_the_doors_the_person_opens() {
        let k = |n: u32| FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        };
        let closed: std::collections::BTreeSet<FormKey> = [k(1), k(2)].into();
        let opens = [FormKey {
            plugin: "A.ESM".into(),
            object_id: 2,
        }];
        assert_eq!(persons_doors(closed.clone(), &opens), [k(2)].into());
        assert!(persons_doors(closed, &[]).is_empty());
    }
    use fo4_core::Budget;

    fn k(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    #[test]
    fn a_moveto_with_zero_offsets_and_a_settled_target_is_a_place() {
        let at = fo4_vm::track::Spot::At {
            space: k(1),
            pos: [1.0, 2.0, 3.0],
            facing: Some(90.0),
            record: Some(k(7)),
        };
        let m = fo4_vm::track::PlayerMove {
            t: 4.0,
            target: at.clone(),
            zero_offsets: true,
        };
        let (key, to, rule) = move_place(&m).unwrap();
        assert_eq!(key, k(7));
        assert_eq!(
            (to.to, to.space, to.pos),
            (k(7), norm(&k(1)), [1.0, 2.0, 3.0])
        );
        assert_eq!(rule, crate::rules::PLAYER_MOVETO);
        let off = fo4_vm::track::PlayerMove {
            zero_offsets: false,
            ..m.clone()
        };
        assert!(move_place(&off).unwrap_err().contains("offsets"));
        let open = fo4_vm::track::PlayerMove {
            target: fo4_vm::track::Spot::Open("no track"),
            ..m
        };
        assert!(move_place(&open).unwrap_err().contains("no track"));
    }

    #[test]
    fn a_sit_is_followed_only_into_another_space() {
        let (key, to, rule) = sit_place(k(5), Ok((k(2), [1.0, 2.0, 3.0])), Some(&k(1))).unwrap();
        assert_eq!(
            (key, to.space, to.pos, rule),
            (k(5), k(2), [1.0, 2.0, 3.0], crate::rules::PLAYER_SIT)
        );
        assert!(sit_place(k(5), Ok((k(2), [0.0; 3])), None).is_ok());
        let same = sit_place(k(5), Ok((k(1), [0.0; 3])), Some(&k(1))).unwrap_err();
        assert!(same.contains("the walker's space"), "{same}");
        let open = sit_place(k(5), Err("unplaced".into()), Some(&k(1))).unwrap_err();
        assert!(open.contains("unplaced"), "{open}");
    }

    #[test]
    fn the_last_place_in_the_runs_order_wins() {
        let place = |n: u32, rule: &'static str| {
            Ok((
                k(n),
                LoadDoor {
                    to: k(n),
                    space: k(1),
                    pos: [0.0; 3],
                    rot: [0.0; 3],
                },
                rule,
            ))
        };
        assert_eq!(latest_place(Vec::new()), (None, Vec::new()));
        // A MoveTo, then a sit 0.04 s later, given out of order; one
        // refusal in between.
        let found = vec![
            (2.04, place(9, crate::rules::PLAYER_SIT)),
            (2.0, place(8, crate::rules::PLAYER_MOVETO)),
            (2.02, Err("not followed".to_string())),
        ];
        let (last, notes) = latest_place(found);
        let (t, (key, _, rule)) = last.unwrap();
        assert_eq!((t, key, rule), (2.04, k(9), crate::rules::PLAYER_SIT));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], (2.02, "not followed".to_string()));
        assert_eq!(notes[1].0, 2.0);
        assert!(notes[1]
            .1
            .starts_with("replaced by a later place at 2.04 s"));
        // Equal seconds keep the run's order.
        let (last, _) = latest_place(vec![
            (3.0, place(1, crate::rules::PLAYER_MOVETO)),
            (3.0, place(2, crate::rules::PLAYER_SIT)),
        ]);
        assert_eq!(last.unwrap().1 .0, k(2));
    }

    #[test]
    fn the_collision_switches_are_off_by_default_and_name_their_rules() {
        let o = Options::default();
        assert!(!o.collision.any());
        assert_eq!(crate::world::Options::default().collision, o.collision);
        assert!(!o.child_flag_agrees && !crate::world::Options::default().child_flag_agrees);
        assert!(collision_rules(&o).is_empty());
        assert!(!o.door_open && !crate::world::Options::default().door_open);
        let doors = collision_rules(&Options {
            door_open: true,
            ..Options::default()
        });
        assert!(doors.contains(crate::rules::DOOR_OPEN_COLLISION));
        assert!(doors.contains(fo4_scene::hypotheses::ANIMATED_COLLISION_AS_STORED));
        assert!(doors.contains(crate::rules::ELEVATOR_RIGID));
        let w = Walker::new(
            floor(),
            Vec::new(),
            Script::parse("1 forward on").unwrap(),
            o,
        )
        .unwrap();
        assert!(w.log.borrow().rules.is_empty());
        let on = Options {
            collision: fo4_scene::readiness::Switches {
                off_chain: true,
                reference_alpha: true,
                static_fields: false,
                activator_fields: false,
            },
            child_flag_agrees: true,
            ..Options::default()
        };
        let w = Walker::new(
            floor(),
            Vec::new(),
            Script::parse("1 forward on").unwrap(),
            on,
        )
        .unwrap();
        let rules: Vec<&str> = w.log.borrow().rules.iter().copied().collect();
        let mut want = vec![
            fo4_scene::hypotheses::COLLISION_OFF_CHAIN,
            fo4_scene::hypotheses::REFERENCE_ALPHA_COLLISION,
            fo4_scene::hypotheses::ENABLE_CHILD_FLAG_AGREES,
        ];
        want.sort();
        assert_eq!(rules, want);
    }

    fn floor() -> World {
        let b = Budget::new(1 << 24);
        let mut w = World::new();
        let (lo, hi) = (-2000.0, 2000.0);
        w.add_triangles(
            &[
                [[lo, lo, 0.0], [hi, lo, 0.0], [hi, hi, 0.0]],
                [[lo, lo, 0.0], [hi, hi, 0.0], [lo, hi, 0.0]],
            ],
            "floor",
            &b,
        )
        .unwrap();
        // A post at x = 300 with a reference of its own.
        w.add_sphere([300.0, 0.0, 120.0], 20.0, "post", &b).unwrap();
        w.rebuild(256.0, &b).unwrap();
        w
    }

    fn key(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    fn slices(w: &mut Walker, start: &Point) -> Vec<Slice> {
        let mut out = Vec::new();
        let mut now = start.t;
        for _ in 0..10_000 {
            match w.step(now, Some(start)).unwrap() {
                Next::Slice(s) => {
                    now = s.until;
                    out.push(s);
                }
                Next::Finish => return out,
                Next::Stop => panic!("stopped"),
            }
        }
        panic!("no end")
    }

    #[test]
    fn the_script_walks_the_walker_and_keeps_its_seconds() {
        let script =
            Script::parse("0 face 0\n1 forward on\n2.005 forward off\n2.5 activate\n3 end\n")
                .unwrap();
        let opts = Options {
            reach: Some(250.0),
            ..Options::default()
        };
        let refs = vec![None, Some(key(7))];
        let mut w = Walker::new(floor(), refs, script, opts).unwrap();
        let start = Point {
            t: 0.0,
            space: key(1),
            pos: [0.0, 0.0, 5.0],
            facing: None,
        };
        let s = slices(&mut w, &start);
        // The walker settled on the floor: a point at second 0.
        assert_eq!(s[0].points[0].t, 0.0);
        assert!(s[0].points[0].pos[2].abs() < 1e-6);
        // It stood until the command at 1 s (one slice, no point).
        assert!(s.iter().any(|x| x.points.is_empty() && x.until == 1.0));
        let pts: Vec<&Point> = s.iter().flat_map(|x| &x.points).collect();
        // The stand's end, then the moves; every second the input names is kept.
        assert!(pts.iter().any(|p| p.t == 1.0 && p.pos[0] == 0.0));
        assert!(pts.iter().any(|p| p.t == 2.005));
        let last = pts.last().unwrap();
        assert_eq!(last.t, 2.005);
        // 1.005 s at the provisional walking speed.
        let speed = Params::PROVISIONAL.walk_speed;
        assert!((last.pos[0] - 1.005 * speed).abs() < 1e-6, "{last:?}");
        // Seconds never go back and slices end where their points end.
        for x in &s {
            assert!(x.points.iter().all(|p| p.t <= x.until));
        }
        // The activation at 2.5 s meets the post's sphere (its reference).
        let acts: Vec<&Act> = s.iter().flat_map(|x| &x.actions).collect();
        assert_eq!(acts.len(), 1);
        assert_eq!(acts[0].at, 2.5);
        assert_eq!(acts[0].reference, key(7));
        let log = w.log.borrow();
        assert_eq!(log.activations.len(), 1);
        assert!(log.rules.contains(crate::rules::ACTIVATE_RAY));
    }

    #[test]
    fn joined_slices_give_the_same_points_and_actions() {
        let text = "0 face 0\n1 forward on\n1.5 turn 30\n2.005 forward off\n2.5 activate\n3 toward 100 50\n5 end\n";
        let start = Point {
            t: 0.0,
            space: key(1),
            pos: [0.0, 0.0, 5.0],
            facing: None,
        };
        let run = |slice: f64| {
            let opts = Options {
                reach: Some(250.0),
                slice,
                ..Options::default()
            };
            let mut w = Walker::new(
                floor(),
                vec![None, Some(key(7))],
                Script::parse(text).unwrap(),
                opts,
            )
            .unwrap();
            let s = slices(&mut w, &start);
            let n = s.len();
            let pts: Vec<Point> = s.iter().flat_map(|x| x.points.clone()).collect();
            let acts: Vec<Act> = s.iter().flat_map(|x| x.actions.clone()).collect();
            (n, pts, acts)
        };
        let (n1, p1, a1) = run(0.0);
        let (n2, p2, a2) = run(0.25);
        assert_eq!(p1, p2);
        assert_eq!(a1, a2);
        assert!(n2 * 3 < n1, "{n1} {n2}");
    }

    /// A person at a window, without the window: frames of uneven length
    /// (some as long as a busy run lets the window's clock run ahead),
    /// commands stamped at the second the frame ends as the window stamps
    /// them, the horizon moved as the window's clock moves; the events the
    /// walker received kept as the text a recording writes.
    struct Window {
        plan: Vec<(f64, Vec<Command>)>,
        frame: usize,
        clock: f64,
        written: Rc<RefCell<String>>,
        waits: Rc<RefCell<u64>>,
        /// Platforms handed over before a wait.
        platforms: Rc<RefCell<u64>>,
    }

    impl Arrivals for Window {
        fn wait(&mut self, until: f64, _: f64, _: Option<[f64; 3]>) -> Option<(Vec<Event>, f64)> {
            *self.waits.borrow_mut() += 1;
            let mut out = Vec::new();
            while self.clock < until {
                let (dt, commands) = self
                    .plan
                    .get(self.frame)
                    .cloned()
                    .unwrap_or((1.0 / 60.0, Vec::new()));
                self.frame += 1;
                self.clock += dt;
                for command in commands {
                    out.push(Event {
                        t: self.clock,
                        command,
                    });
                }
            }
            let mut w = self.written.borrow_mut();
            for e in &out {
                w.push_str(&format!("{e}\n"));
            }
            Some((out, self.clock))
        }

        fn platforms(&mut self, _: Vec<crate::elevator::Platform>) {
            *self.platforms.borrow_mut() += 1;
        }
    }

    /// Frames of uneven length from a fixed pseudo-random sequence, and the
    /// commands a person gives: looks with the mouse while walking, keys
    /// held and let go, running, strafing, the activation, the end.
    fn person_plan() -> Vec<(f64, Vec<Command>)> {
        use crate::input::Key;
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut plan = Vec::new();
        for n in 0..260_u32 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let r = (seed >> 33) as f64 / f64::from(1u32 << 31);
            // Mostly 60 to 200 frames a second, now and then a long frame.
            let dt = if n % 37 == 36 {
                0.25
            } else {
                0.005 + 0.012 * r
            };
            let mut c = Vec::new();
            match n {
                3 => c.extend([Command::Face(10.0 + r), Command::Pitch(-5.0 - r)]),
                12 => c.push(Command::Key(Key::Forward, true)),
                20..=60 => c.push(Command::Face(10.0 + 0.37 * f64::from(n - 19) + r)),
                70 => c.push(Command::Key(Key::Run, true)),
                90 => c.push(Command::Key(Key::Left, true)),
                110 => c.extend([
                    Command::Key(Key::Left, false),
                    Command::Key(Key::Run, false),
                ]),
                130 => c.extend([Command::Key(Key::Forward, false), Command::Face(0.0)]),
                131 => c.push(Command::Activate),
                170 => c.push(Command::Key(Key::Back, true)),
                185 => c.extend([Command::Key(Key::Right, true), Command::Pitch(3.0)]),
                200 => c.extend([
                    Command::Key(Key::Back, false),
                    Command::Key(Key::Right, false),
                ]),
                201 => c.push(Command::Activate),
                250 => c.push(Command::End),
                _ => {}
            }
            plan.push((dt, c));
        }
        plan
    }

    #[test]
    fn commands_that_arrive_drive_the_walker_as_their_recording_does() {
        let start = Point {
            t: 0.0,
            space: key(1),
            pos: [0.0, 0.0, 5.0],
            facing: None,
        };
        let opts = Options {
            reach: Some(400.0),
            slice: 0.25,
            stand: Some(0.1),
            ..Options::default()
        };
        let refs = vec![None, Some(key(7))];
        let written = Rc::new(RefCell::new(String::new()));
        let waits = Rc::new(RefCell::new(0));
        let handed = Rc::new(RefCell::new(0));
        let window = Window {
            plan: person_plan(),
            frame: 0,
            clock: 0.0,
            written: written.clone(),
            waits: waits.clone(),
            platforms: handed.clone(),
        };
        let mut live = Walker::new(floor(), refs.clone(), Script::default(), opts)
            .unwrap()
            .with_arrivals(Box::new(window))
            .unwrap();
        let a = slices(&mut live, &start);
        // The recording, read back as a scripted input.
        let text = written.borrow().clone();
        let script = Script::parse(&text).unwrap();
        assert!(script.events.len() > 50, "{text}");
        assert_eq!(script.end(), script.events.last().map(|e| e.t));
        let mut replay = Walker::new(floor(), refs, script, opts).unwrap();
        let b = slices(&mut replay, &start);
        // The same slices: points, seconds and activations, bit for bit.
        assert_eq!(a, b);
        let (la, lb) = (live.log.borrow(), replay.log.borrow());
        assert_eq!(la.track, lb.track);
        assert_eq!(la.looks, lb.looks);
        assert_eq!(
            (la.ticks, la.slices, la.standing_slices, la.points),
            (lb.ticks, lb.slices, lb.standing_slices, lb.points)
        );
        assert_eq!(la.activations.len(), 2);
        // It walked, stood, and the walker waited for the window often.
        assert!(la.points > 20 && la.standing_slices > 5, "{la:?}");
        assert!(*waits.borrow() > 50);
        // No named platform: none handed to the window.
        assert_eq!(*handed.borrow(), 0);
        // Standing slices last 0.1 s at most.
        let mut now = 0.0;
        for s in &a {
            if s.points.is_empty() && s.actions.is_empty() {
                assert!(s.until - now <= 0.1 + 1e-12, "{} {}", now, s.until);
            }
            now = s.until;
        }
    }

    #[test]
    fn arriving_commands_need_the_standing_slice_and_no_script() {
        struct Never;
        impl Arrivals for Never {
            fn wait(&mut self, _: f64, _: f64, _: Option<[f64; 3]>) -> Option<(Vec<Event>, f64)> {
                None
            }
        }
        let w = Walker::new(floor(), vec![], Script::default(), Options::default()).unwrap();
        assert!(w.with_arrivals(Box::new(Never)).is_err());
        let opts = Options {
            stand: Some(0.1),
            ..Options::default()
        };
        let w = Walker::new(floor(), vec![], Script::parse("1 end").unwrap(), opts).unwrap();
        assert!(w.with_arrivals(Box::new(Never)).is_err());
        assert!(Walker::new(
            floor(),
            vec![],
            Script::default(),
            Options {
                stand: Some(0.0),
                ..Options::default()
            }
        )
        .is_err());
        // A source that closes stops the walker.
        let mut w = Walker::new(floor(), vec![], Script::default(), opts)
            .unwrap()
            .with_arrivals(Box::new(Never))
            .unwrap();
        let start = Point {
            t: 0.0,
            space: key(1),
            pos: [0.0, 0.0, 5.0],
            facing: None,
        };
        assert_eq!(w.step(0.0, Some(&start)).unwrap_err(), SOURCE_CLOSED);
    }

    #[test]
    fn a_door_standing_open_is_shown_open_and_the_persons_activation_toggles_it() {
        // `play_door_activation_toggles_v1`.
        let pose: std::collections::BTreeSet<FormKey> = [key(3)].into();
        let walker = |toggle: bool| {
            let opts = Options {
                reach: Some(250.0),
                held_doors: true,
                door_open: true,
                door_toggle: toggle,
                ..Options::default()
            };
            let script = Script::parse("1 end\n").unwrap();
            let live = Live::fixed(floor(), vec![])
                .unwrap()
                .with_open_pose(pose.clone());
            Walker::new(World::new(), vec![], script, opts)
                .unwrap()
                .with_live(live, false)
        };
        // A door standing in its open pose is shown open (no box) with the
        // switch, not without.
        let on = walker(true);
        assert_eq!(
            on.held_door_states(),
            std::collections::BTreeMap::from([(key(3), false)])
        );
        assert!(on.log.borrow().rules.contains(crate::rules::DOOR_TOGGLE));
        assert!(walker(false).held_door_states().is_empty());
        // Shown open and standing open: E closes it.
        let states = on.held_door_states();
        assert_eq!(door_toggle(&states, None, &key(3), false), Some(false));
        // Its box standing because the person closed it: E opens it.
        let closed = std::collections::BTreeMap::from([(key(3), true)]);
        let d = crate::live::DoorState {
            person_closed: true,
            ..Default::default()
        };
        assert_eq!(door_toggle(&closed, Some(&d), &key(3), false), Some(true));
        // Held by the run's scripts, not the person: E as before.
        let d = crate::live::DoorState {
            open: Some(false),
            blocked: Some(true),
            person_closed: false,
        };
        assert_eq!(door_toggle(&closed, Some(&d), &key(3), false), None);
        // A door the reading never showed, or no door: E as before.
        assert_eq!(door_toggle(&states, None, &key(4), false), None);
        // A held door whose box left (the house's front door opened by the
        // person or a script), shown open: E closes it too.
        let released = std::collections::BTreeMap::from([(key(5), false)]);
        let opened = crate::live::DoorState {
            open: Some(true),
            blocked: Some(true),
            person_closed: false,
        };
        assert_eq!(
            door_toggle(&released, Some(&opened), &key(5), false),
            Some(false)
        );
        // `play_door_toggle_blocked_unchanged_v1`: the same door, its
        // activation blocked by the run, is left as it stands.
        assert_eq!(door_toggle(&released, Some(&opened), &key(5), true), None);
        assert!(door_blocked(Some(&opened)));
        // Unblocked again: E closes it under the switch too.
        let unblocked = crate::live::DoorState {
            blocked: Some(false),
            ..opened
        };
        assert_eq!(
            door_toggle(&released, Some(&unblocked), &key(5), true),
            Some(false)
        );
        assert!(!door_blocked(None) && !door_blocked(Some(&unblocked)));
    }

    #[test]
    fn a_door_storing_open_by_default_is_shown_open_with_its_switch() {
        // `play_door_stored_open_v1`.
        let walker = |stored: bool| {
            let opts = Options {
                reach: Some(250.0),
                held_doors: true,
                door_open: true,
                door_toggle: true,
                door_stored_open: stored,
                ..Options::default()
            };
            let live = Live::fixed(floor(), vec![])
                .unwrap()
                .with_stored_open([key(8)].into());
            Walker::new(
                World::new(),
                vec![],
                Script::parse("1 end\n").unwrap(),
                opts,
            )
            .unwrap()
            .with_live(live, false)
        };
        let on = walker(true);
        assert_eq!(
            on.held_door_states(),
            std::collections::BTreeMap::from([(key(8), false)])
        );
        assert!(on
            .log
            .borrow()
            .rules
            .contains(crate::rules::DOOR_STORED_OPEN));
        assert!(walker(false).held_door_states().is_empty());
        assert!(!walker(false)
            .log
            .borrow()
            .rules
            .contains(crate::rules::DOOR_STORED_OPEN));
        // Shown open: E closes it.
        assert_eq!(
            door_toggle(&on.held_door_states(), None, &key(8), true),
            Some(false)
        );
    }

    #[test]
    fn closing_a_door_the_person_opens_gives_the_run_the_activation_alone() {
        // `play_door_activation_toggles_v1` with `play_activate_opens_v1`:
        // the post (reference 7) stands for a door the person opens, shown
        // open; E closes it and the run gets no open.
        let script =
            Script::parse("0 face 0\n1 forward on\n2.005 forward off\n2.5 activate\n3 end\n")
                .unwrap();
        let run = |toggle: bool| {
            let opts = Options {
                reach: Some(250.0),
                held_doors: true,
                door_open: true,
                door_toggle: toggle,
                ..Options::default()
            };
            let live = Live::fixed(floor(), vec![None, Some(key(7))])
                .unwrap()
                .with_open_pose([norm(&key(7))].into());
            let mut w = Walker::new(World::new(), vec![], script.clone(), opts)
                .unwrap()
                .with_live(live, false)
                .with_opens(vec![key(7)]);
            w.state = RunState::reading_doors(true);
            let start = Point {
                t: 0.0,
                space: key(1),
                pos: [0.0, 0.0, 5.0],
                facing: None,
            };
            let acts: Vec<PlayerAction> = slices(&mut w, &start)
                .into_iter()
                .flat_map(|x| x.actions)
                .map(|a| a.action)
                .collect();
            let closed = w
                .state
                .doors
                .get(&norm(&key(7)))
                .is_some_and(|d| d.person_closed);
            (acts, closed)
        };
        assert_eq!(run(true), (vec![PlayerAction::Activate], true));
        // Without the switch: the activation and the open, as before.
        assert_eq!(
            run(false),
            (vec![PlayerAction::Activate, PlayerAction::Open], false)
        );
    }

    #[test]
    fn a_blocked_open_door_is_left_as_it_stands_and_given_no_open() {
        // `play_door_toggle_blocked_unchanged_v1`: the post (reference 7)
        // stands for the house's front door after the evacuation's script
        // blocked its activation and opened it.
        let script =
            Script::parse("0 face 0\n1 forward on\n2.005 forward off\n2.5 activate\n3 end\n")
                .unwrap();
        let run = |blocked_unchanged: bool| {
            let opts = Options {
                reach: Some(250.0),
                held_doors: true,
                door_open: true,
                door_toggle: true,
                door_toggle_blocked: blocked_unchanged,
                ..Options::default()
            };
            let live = Live::fixed(floor(), vec![None, Some(key(7))])
                .unwrap()
                .with_open_pose([norm(&key(7))].into());
            let mut w = Walker::new(World::new(), vec![], script.clone(), opts)
                .unwrap()
                .with_live(live, false)
                .with_opens(vec![key(7)]);
            w.state = RunState::reading_doors(true);
            w.state.doors.insert(
                norm(&key(7)),
                crate::live::DoorState {
                    open: Some(true),
                    blocked: Some(true),
                    person_closed: false,
                },
            );
            let start = Point {
                t: 0.0,
                space: key(1),
                pos: [0.0, 0.0, 5.0],
                facing: None,
            };
            let acts: Vec<PlayerAction> = slices(&mut w, &start)
                .into_iter()
                .flat_map(|x| x.actions)
                .map(|a| a.action)
                .collect();
            let closed = w
                .state
                .doors
                .get(&norm(&key(7)))
                .is_some_and(|d| d.person_closed);
            let rule = w
                .log
                .borrow()
                .rules
                .contains(crate::rules::DOOR_TOGGLE_BLOCKED);
            (acts, closed, rule)
        };
        // With the switch: the activation alone, the door left open.
        assert_eq!(run(true), (vec![PlayerAction::Activate], false, true));
        // Without it: closed by the person, as before.
        assert_eq!(run(false), (vec![PlayerAction::Activate], true, false));
    }

    #[test]
    fn the_actors_boxes_stand_where_the_run_last_put_them_and_the_ray_meets_them() {
        // `play_activate_actor_box_v1`.
        let space = k(1);
        let mut place = fo4_vm::track::Place::default();
        place.actors.push(fo4_vm::track::PlacedActor {
            key: k(10),
            space: space.clone(),
            pos: [100.0, 0.0, 0.0],
        });
        place.actors.push(fo4_vm::track::PlacedActor {
            key: k(11),
            space: k(2),
            pos: [150.0, 0.0, 0.0],
        });
        place.actors.push(fo4_vm::track::PlacedActor {
            key: k(12),
            space: space.clone(),
            pos: [150.0, 50.0, 0.0],
        });
        // An applied move, then one not yet applied.
        for (t, x) in [(5.0, 200.0), (9.0, 900.0)] {
            place.actor_moves.push(fo4_vm::track::ActorMove {
                t,
                actor: k(10),
                space: space.clone(),
                pos: [x, 0.0, 0.0],
            });
        }
        place.actor_cursor = 1;
        let places = actor_places(&place, &[]);
        assert_eq!(places[&k(10)], (space.clone(), [200.0, 0.0, 0.0]));
        // A later MoveTo wins over an earlier move.
        let moved = [(7.0, k(10), space.clone(), [180.0, 0.0, 0.0])];
        assert_eq!(actor_places(&place, &moved)[&k(10)].1, [180.0, 0.0, 0.0]);
        // Only the walker's space; a disabled actor is not met.
        let boxes = actor_boxes(&places, &space, &|r| *r == k(12));
        assert_eq!(boxes.items.len(), 1);
        assert_eq!(boxes.items[0].0, k(10));
        assert_eq!(boxes.kinds, vec![Some(*b"ACHR")]);
        // The eye's ray along x at 60 units up meets the actor's box at its
        // near face (x 180); without the boxes it meets nothing in reach.
        let world = floor();
        let refs: Vec<Option<FormKey>> = vec![None; 8];
        let ray = |b: Option<&pick::Boxes>| {
            pick::met(
                &world,
                &refs,
                b,
                [0.0, 0.0, 60.0],
                [1.0, 0.0, 0.0],
                250.0,
                &|_| true,
                &|_| false,
                true,
            )
            .map(|(h, _)| h)
        };
        let hit = ray(Some(&boxes)).unwrap();
        assert_eq!(hit.reference, Some(k(10)));
        assert!((hit.distance - 180.0).abs() < 1e-6, "{}", hit.distance);
        assert_eq!(ray(None), None);
        // Off by default.
        assert!(!Options::default().pick_actors);
    }

    #[test]
    fn an_activation_needs_the_callers_reach() {
        let script = Script::parse("1 activate\n").unwrap();
        assert!(Walker::new(floor(), vec![], script, Options::default()).is_err());
    }
}
