#![forbid(unsafe_code)]
//! A quest run driven by input (`spec/play.md`). `quest_run new-game` runs
//! the scripts along a track its caller wrote; here the track comes slice
//! by slice from a [`Driver`] (the walker over the cell's collision, fed by
//! a scripted input or by a window's keyboard and mouse), with the
//! player's activations, and the run advances to each slice's second
//! before the next is asked for.
//!
//! The new-game code is `quest_run`'s own, `fo4_vm::run`'s library run;
//! a driver steps its clock loop with a horizon
//! (`fo4_vm::run::NewGame::advance_to`). Without a driver [`new_game`] is
//! `quest_run` itself. Every behaviour reading the play
//! mode adds is a pending rule of `spec/hypotheses.tsv` ([`rules`]); the
//! reach of an activation and the walker's speed are the caller's.
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::timeline::Timeline;
use fo4_vm::Vm;

pub mod input;
pub mod link;
pub mod live;
pub mod pick;
mod run;
pub mod state;
pub mod walker;
pub mod world;

pub use fo4_vm::player::Action as PlayerAction;
pub use fo4_vm::track::Point;

/// Names of the play mode's rules in `spec/hypotheses.tsv`.
pub mod rules {
    /// The run's player point is the walker's feet, sampled at each of the
    /// walker's ticks and joined by straight moves.
    pub const PLAYER_FEET: &str = "play_player_point_walker_feet_v1";
    /// The run's real-time clock is the play clock: one second of input
    /// (a scripted second, a window frame's wall time) is one second of
    /// the run.
    pub const RUN_CLOCK: &str = "play_run_clock_input_seconds_v1";
    /// The activation input activates the first placed reference whose
    /// collision the eye's ray meets within the caller's reach.
    pub const ACTIVATE_RAY: &str = "play_activate_collision_ray_v1";
    /// The ray also meets the object bounds (OBND) of placed references
    /// the walking world holds no collision for, placed by the reference's
    /// transform (`--play-pick-bounds`).
    pub const ACTIVATE_BOUNDS: &str = "play_activate_object_bounds_v1";
    /// The walking world follows the run's state at each slice as a
    /// timeline frame at that second does: the collision of a reference
    /// the frame would not draw leaves, of one it would draw enters, of a
    /// moved one moves by the move's translation (`--play-live-world`).
    pub const LIVE_COLLISION: &str = "play_live_collision_follows_run_v1";
    /// The activation of a placed door whose `XTEL` names a door puts the
    /// walker at the `XTEL` position in that door's space at the
    /// activation's second (`--play-load-doors`).
    pub const LOAD_DOOR: &str = "play_load_door_activation_v1";
    /// The run reads trigger volumes and placed actors about the walker's
    /// points as it reaches them, as it reads them about the caller's track
    /// (`--play-region-walker`).
    pub const REGION: &str = "play_region_around_walker_v1";
    /// Under the object bounds reading, the boxes of references whose base
    /// is a static, a static collection or a movable static are left out
    /// (`--play-pick-bounds-activatable`).
    pub const BOUNDS_NOT_STATIC: &str = "play_activate_bounds_not_static_v1";
    /// The record types `BOUNDS_NOT_STATIC` leaves out.
    pub const STATIC_KINDS: [[u8; 4]; 3] = [*b"STAT", *b"SCOL", *b"MSTT"];
    /// Every rule above.
    pub const ALL: [&str; 8] = [
        PLAYER_FEET,
        RUN_CLOCK,
        ACTIVATE_RAY,
        ACTIVATE_BOUNDS,
        LIVE_COLLISION,
        LOAD_DOOR,
        REGION,
        BOUNDS_NOT_STATIC,
    ];
}

/// One player action of a slice: an activation (or another action of
/// `fo4_vm::player`) of a placed reference at a second of the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub at: f64,
    pub action: PlayerAction,
    pub reference: FormKey,
}

/// The input up to a second: the player's track points after the last one
/// the run has (none when the player stood still), the player's actions,
/// and the second `until` the run may advance to. The player stands at the
/// last point from its second until `until`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slice {
    pub points: Vec<Point>,
    pub until: f64,
    pub actions: Vec<Act>,
}

/// What a driver gives when the run asks for more input.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// More input.
    Slice(Slice),
    /// The input is over: the player stays at the last point and the run
    /// goes on to its end as `quest_run` does after the track's end.
    Finish,
    /// End the run now (a window closed): what is due later does not
    /// happen; the outputs are written as they stand.
    Stop,
}

/// The run as a driver sees it when the run asks for input.
pub struct View<'a, 'v> {
    pub vm: &'a Vm<'v>,
    pub store: &'a Store,
    pub(crate) sources: (&'a fo4_core::Manifest, &'a fo4_vfs::Vfs),
    pub(crate) loaded: Option<&'a fo4_vm::track::Track>,
    pub(crate) timeline: &'a dyn Fn() -> Option<Timeline>,
}
impl View<'_, '_> {
    /// The run's present second.
    pub fn now(&self) -> f64 {
        self.vm.world.now[fo4_vm::world::Clock::Real as usize]
    }
    /// The caller's track as the run loaded it (`--track`, with the points
    /// `--load-doors` adds), before the play mode cut it at the start.
    pub fn loaded_track(&self) -> Option<&fo4_vm::track::Track> {
        self.loaded
    }
    /// The second of the last point of the run's track.
    pub fn track_end(&self) -> Option<f64> {
        self.vm
            .world
            .place
            .track
            .as_ref()
            .and_then(|t| t.points.last())
            .map(|p| p.t)
    }
    /// The run's timeline so far with line texts
    /// (`fo4_vm::timeline::Recorder::finish`); `None` without
    /// `--timeline`. It reads labels and strings: call it when something
    /// changed, not every slice.
    pub fn timeline(&self) -> Option<Timeline> {
        (self.timeline)()
    }
}

/// Gives the run its input.
pub trait Driver {
    /// The next slice: called when everything due before the last slice's
    /// second has run.
    fn next(&mut self, run: &View<'_, '_>) -> Next;
}

/// The play mode's counts by text.
pub type Counts = std::collections::BTreeMap<String, u64>;

/// What a run gives back: its driver and the play counts.
pub type Played = (Option<Box<dyn Driver>>, Counts);

/// The play mode's own switches for the run (the walker's are its
/// driver's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Read trigger volumes and placed actors about the walker's points as
    /// it reaches them (`--play-region-walker`,
    /// `play_region_around_walker_v1`).
    pub region: bool,
    /// When the track grows, compute again the function 1 marks of the
    /// scenes waiting on a distance condition
    /// (`fo4_vm::scene::recheck_distance_marks`) instead of offering them
    /// their completion again (`--play-recheck-marks`; a policy of the
    /// play mode, no game claim: it makes the stepped run log what
    /// `quest_run` logs).
    pub recheck_marks: bool,
}

/// [`new_game`] with the play mode's own switches.
pub fn new_game_with(
    args: Vec<String>,
    driver: Option<Box<dyn Driver>>,
    options: Options,
) -> Result<Played, Box<dyn std::error::Error>> {
    run::new_game(args, driver, options)
}

/// Runs `quest_run` with these arguments (`MANIFEST new-game OUT_DIR
/// [switches...]`): with a driver, its clock loop is stepped by the
/// driver's input; without, it is `quest_run` unchanged. The run's
/// `--track FILE` gives the start (its first point) and the area the world
/// model reads (the cells about its points, as without play); its points
/// after the start are replaced by the driver's. Returns the driver and
/// the play counts.
pub fn new_game(
    args: Vec<String>,
    driver: Option<Box<dyn Driver>>,
) -> Result<Played, Box<dyn std::error::Error>> {
    run::new_game(args, driver, Options::default())
}

/// A driver that gives the run its own track back slice by slice (the
/// play mode's check: the stepped run of a track against `quest_run` with
/// it): the caller's track as the run loaded it ([`View::loaded_track`]).
/// Each slice holds the points up to `step` seconds after the run's
/// present second, and at least one point; it ends at its last point.
#[derive(Debug, Clone)]
pub struct Replay {
    pub step: f64,
    points: Option<Vec<Point>>,
    next: usize,
}
impl Replay {
    pub fn new(step: f64) -> Replay {
        Replay {
            step,
            points: None,
            next: 0,
        }
    }
}
impl Driver for Replay {
    fn next(&mut self, run: &View<'_, '_>) -> Next {
        let now = run.now();
        if self.points.is_none() {
            // The points the run kept at its start are its own.
            let end = run.track_end().unwrap_or(f64::NEG_INFINITY);
            let points = run
                .loaded_track()
                .map(|t| t.points.clone())
                .unwrap_or_default();
            self.next = points.iter().take_while(|p| p.t <= end).count();
            self.points = Some(points);
        }
        let points_all = self.points.as_deref().unwrap_or_default();
        let Some(first) = points_all.get(self.next) else {
            return Next::Finish;
        };
        let end = first.t.max(now + self.step);
        let mut points = Vec::new();
        while let Some(p) = points_all.get(self.next) {
            if p.t > end && !points.is_empty() {
                break;
            }
            points.push(p.clone());
            self.next += 1;
        }
        let until = points.last().map_or(end, |p| p.t);
        Next::Slice(Slice {
            points,
            until,
            actions: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_play_rule_is_registered_pending_with_seven_columns() {
        let registry = include_str!("../../../spec/hypotheses.tsv");
        let index = include_str!("../../../spec/README.md");
        for rule in super::rules::ALL {
            let rows: Vec<&str> = registry
                .lines()
                .filter(|l| l.split('\t').next() == Some(rule))
                .collect();
            assert_eq!(rows.len(), 1, "{rule} registered once");
            let cols: Vec<&str> = rows[0].split('\t').collect();
            assert_eq!(cols.len(), 7, "{rule}");
            assert_eq!(cols[5], "pending", "{rule}");
            assert!(index.contains(&format!("| `{rule}` | pending |")), "{rule}");
        }
    }
}
