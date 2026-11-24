//! The play mode's options, read once for every caller: `play_run`
//! (headless) and the window's scripted run (`fo4-view --play-run ...
//! --play-input`) parse them with [`Play::take`] (or [`Play::parse`]),
//! check them with [`Play::check`], build the walking world with
//! [`Play::build_world`] and the walker with [`Play::walker`], which also
//! writes the switch lines of `play.txt`. Every behaviour an option turns
//! on is a pending rule named where the option is read (`spec/play.md`);
//! the options themselves are plumbing.
use crate::input::Script;
use crate::{walker, world};
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_vfs::Vfs;
use std::fmt::Write as _;

/// Every play option, as given (off or absent by default).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Play {
    /// `--play-around N`: cells about the start in the walking world
    /// (default 1).
    pub around: Option<u32>,
    /// `--play-reach UNITS`: the activation's reach (the caller's).
    pub reach: Option<f64>,
    /// `--walk-speed UNITS`, `--run-speed UNITS`: the caller's values of
    /// the walker's speed rules.
    pub walk_speed: Option<f64>,
    pub run_speed: Option<f64>,
    /// `--play-tick SECONDS`: the walker's fixed tick.
    pub tick: Option<f64>,
    /// `--play-slice SECONDS`: ticks joined into slices that long.
    pub slice: Option<f64>,
    /// `--play-stand SECONDS`: the longest standing slice, and how far the
    /// walker looks ahead for a moving platform before it stands (a policy
    /// of the play mode; needed by commands that arrive while the walker
    /// runs, `fo4-view --play-keys`).
    pub stand: Option<f64>,
    /// `--play-pick-bounds` (`play_activate_object_bounds_v1`),
    /// `--play-pick-bounds-activatable` (`play_activate_bounds_not_static_v1`).
    pub pick_bounds: bool,
    pub activatable: bool,
    /// `--play-live-world` (`play_live_collision_follows_run_v1`),
    /// `--play-nowait` (the NoWait forms read as the plain natives).
    pub live_world: bool,
    pub nowait: bool,
    /// `--play-load-doors` (`play_load_door_activation_v1`).
    pub load_doors: bool,
    /// `--play-region-walker` (`play_region_around_walker_v1`).
    pub region: bool,
    /// `--play-recheck-marks` (a policy of the play mode).
    pub recheck_marks: bool,
    /// `--play-follow-grid` (a policy of the play mode).
    pub follow_grid: bool,
    /// `--play-elevator-door` (`play_elevator_end_load_door_v1`),
    /// `--play-elevator-held-pose` (`play_elevator_held_pose_v1`),
    /// `--play-elevator-rigid` (`play_elevator_rigid_v1`),
    /// `--play-elevator-door-onward` (`play_elevator_end_door_onward_v1`),
    /// `--play-elevator-door-landing` (`play_elevator_end_landing_v1`).
    pub elevator_door: bool,
    pub held_pose: bool,
    pub rigid: bool,
    pub door_onward: bool,
    pub door_landing: bool,
    /// `--play-player-moveto` (`play_player_moveto_v1`),
    /// `--play-player-sits` (`play_player_sit_furniture_v1`).
    pub player_moveto: bool,
    pub player_sits: bool,
    /// `--play-collision-off-chain` (`collision_model_blockers_off_chain_v1`),
    /// `--play-collision-reference-alpha` (`refr_xalp_collision_unchanged_v1`),
    /// `--play-collision-child-flag-agrees` (`xesp_child_flag_agrees_chain_v1`).
    pub off_chain: bool,
    pub reference_alpha: bool,
    pub child_flag_agrees: bool,
    /// `--play-marker-boxes` (`play_collision_marker_boxes_v1`).
    pub marker_boxes: bool,
    /// `--play-elevator REF` (repeated; `play_elevator_run_sequence_v1`),
    /// `--play-elevator-also REF`, `--play-elevator-also-with REF OWNER`.
    pub elevators: Vec<FormKey>,
    pub also: Vec<FormKey>,
    pub also_with: Vec<(FormKey, FormKey)>,
    /// `--play-activate-opens REF` (repeated; `play_activate_opens_v1`):
    /// the caller's timed opens of these references are the walker's
    /// activations of them ([`Play::run_args`], [`walker::Walker::with_opens`]).
    pub opens: Vec<FormKey>,
    /// `--play-activate-person REF` (repeated; `play_activate_person_v1`):
    /// the caller's timed activations of these references are the walker's
    /// activations of them ([`Play::run_args`]).
    pub persons: Vec<FormKey>,
    /// `--play-pick-first-met` (`play_activate_ray_first_met_v1`).
    pub first_met: bool,
    /// `--play-action-after-offset` (`fo4_vm`'s
    /// `caller_action_after_keeps_offset_v1`, `play_activate_written_second_v1`):
    /// the run's `--action-after-offset`, and each timed open or
    /// activation left to the walker ([`Play::run_args`]) given to the run
    /// as the second it was written for (`--input-action-at`), so the
    /// actions held until the stage it sets keep their offset from the
    /// person's activation.
    pub action_offset: bool,
}

/// A record key as the play options take it: `PLUGIN:OBJECTID`, the id in
/// hex (`fo4_vm::player::parse_key`).
fn key(v: &str, usage: &str) -> Result<FormKey, String> {
    fo4_vm::player::parse_key(v).ok_or_else(|| usage.to_string())
}

/// A finite number.
fn number(flag: &str, v: &str) -> Result<f64, String> {
    v.parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or_else(|| format!("{flag} takes a finite number: {v}"))
}

/// Sets a single-valued option once.
fn once<T>(slot: &mut Option<T>, flag: &str, v: T) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{flag} given twice"));
    }
    *slot = Some(v);
    Ok(())
}

impl Play {
    /// Reads the play option that starts `args`, if it is one: how many
    /// arguments it took (0: not a play option), so that a caller with
    /// options of its own reads them in between.
    pub fn take(&mut self, args: &[String]) -> Result<usize, String> {
        let Some(flag) = args.first() else {
            return Ok(0);
        };
        let value = |n: usize| {
            args.get(n)
                .map(String::as_str)
                .ok_or_else(|| format!("{flag} takes a value"))
        };
        let on = match flag.as_str() {
            "--play-pick-bounds" => &mut self.pick_bounds,
            "--play-pick-bounds-activatable" => &mut self.activatable,
            "--play-live-world" => &mut self.live_world,
            "--play-nowait" => &mut self.nowait,
            "--play-load-doors" => &mut self.load_doors,
            "--play-region-walker" => &mut self.region,
            "--play-recheck-marks" => &mut self.recheck_marks,
            "--play-follow-grid" => &mut self.follow_grid,
            "--play-elevator-door" => &mut self.elevator_door,
            "--play-elevator-held-pose" => &mut self.held_pose,
            "--play-elevator-rigid" => &mut self.rigid,
            "--play-elevator-door-onward" => &mut self.door_onward,
            "--play-elevator-door-landing" => &mut self.door_landing,
            "--play-player-moveto" => &mut self.player_moveto,
            "--play-player-sits" => &mut self.player_sits,
            "--play-collision-off-chain" => &mut self.off_chain,
            "--play-collision-reference-alpha" => &mut self.reference_alpha,
            "--play-collision-child-flag-agrees" => &mut self.child_flag_agrees,
            "--play-marker-boxes" => &mut self.marker_boxes,
            "--play-pick-first-met" => &mut self.first_met,
            "--play-action-after-offset" => &mut self.action_offset,
            "--play-around" => {
                let v = value(1)?;
                let n = v
                    .parse()
                    .map_err(|_| format!("--play-around takes cells: {v}"))?;
                once(&mut self.around, flag, n)?;
                return Ok(2);
            }
            "--play-reach" => {
                let r = number(flag, value(1)?)?;
                if r <= 0.0 {
                    return Err("--play-reach takes a positive distance".into());
                }
                once(&mut self.reach, flag, r)?;
                return Ok(2);
            }
            "--walk-speed" => {
                once(&mut self.walk_speed, flag, number(flag, value(1)?)?)?;
                return Ok(2);
            }
            "--run-speed" => {
                once(&mut self.run_speed, flag, number(flag, value(1)?)?)?;
                return Ok(2);
            }
            "--play-tick" => {
                once(&mut self.tick, flag, number(flag, value(1)?)?)?;
                return Ok(2);
            }
            "--play-slice" => {
                let v = number(flag, value(1)?)
                    .ok()
                    .filter(|v| *v >= 0.0)
                    .ok_or("--play-slice takes seconds, 0 or more")?;
                once(&mut self.slice, flag, v)?;
                return Ok(2);
            }
            "--play-stand" => {
                let v = number(flag, value(1)?)
                    .ok()
                    .filter(|v| *v > 0.0)
                    .ok_or("--play-stand takes seconds, more than 0")?;
                once(&mut self.stand, flag, v)?;
                return Ok(2);
            }
            "--play-elevator" | "--play-elevator-also" => {
                let usage = "--play-elevator and --play-elevator-also take PLUGIN:OBJECTID";
                let k = key(value(1).map_err(|_| usage)?, usage)?;
                if flag == "--play-elevator" {
                    self.elevators.push(k);
                } else {
                    self.also.push(k);
                }
                return Ok(2);
            }
            "--play-activate-opens" => {
                let usage = "--play-activate-opens takes PLUGIN:OBJECTID";
                self.opens.push(key(value(1).map_err(|_| usage)?, usage)?);
                return Ok(2);
            }
            "--play-activate-person" => {
                let usage = "--play-activate-person takes PLUGIN:OBJECTID";
                self.persons.push(key(value(1).map_err(|_| usage)?, usage)?);
                return Ok(2);
            }
            "--play-elevator-also-with" => {
                let usage = "--play-elevator-also-with takes PLUGIN:OBJECTID PLUGIN:OBJECTID";
                let k = key(value(1).map_err(|_| usage)?, usage)?;
                let owner = key(value(2).map_err(|_| usage)?, usage)?;
                self.also_with.push((k, owner));
                return Ok(3);
            }
            _ => return Ok(0),
        };
        *on = true;
        Ok(1)
    }

    /// Reads every play option of `args` wherever it stands; the other
    /// arguments are given back in their order. Not checked
    /// ([`Play::check`]).
    pub fn parse(args: &[String]) -> Result<(Play, Vec<String>), String> {
        let mut p = Play::default();
        let mut rest = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match p.take(&args[i..])? {
                0 => {
                    rest.push(args[i].clone());
                    i += 1;
                }
                n => i += n,
            }
        }
        Ok((p, rest))
    }

    /// Whether any play option was given.
    pub fn any(&self) -> bool {
        *self != Play::default()
    }

    /// The options given that only the scripted walker reads (not a
    /// window's keyboard driver): the activation's reach and object bounds
    /// and the live world apply to both.
    pub fn walker_only(&self) -> bool {
        Play {
            reach: None,
            pick_bounds: false,
            live_world: false,
            nowait: false,
            ..self.clone()
        }
        .any()
    }

    /// How the options go together, in `play_run`'s order of checks.
    pub fn check(&self) -> Result<(), String> {
        if self
            .also_with
            .iter()
            .any(|(_, owner)| !self.elevators.contains(owner))
        {
            return Err(
                "--play-elevator-also-with names an owner that is no --play-elevator reference"
                    .into(),
            );
        }
        if self.elevators.is_empty() && (self.elevator_door || !self.also.is_empty()) {
            return Err(
                "--play-elevator-also and --play-elevator-door need --play-elevator".into(),
            );
        }
        if self.elevators.is_empty() && (self.held_pose || self.rigid) {
            return Err(
                "--play-elevator-held-pose and --play-elevator-rigid need --play-elevator".into(),
            );
        }
        if self.door_onward && !self.elevator_door {
            return Err("--play-elevator-door-onward needs --play-elevator-door".into());
        }
        if self.marker_boxes && !self.live_world {
            return Err("--play-marker-boxes needs --play-live-world".into());
        }
        if self.door_landing && !self.elevator_door {
            return Err("--play-elevator-door-landing needs --play-elevator-door".into());
        }
        if self.elevator_door && !(self.load_doors && self.reach.is_some()) {
            return Err("--play-elevator-door needs --play-load-doors and --play-reach".into());
        }
        if self.activatable && !self.pick_bounds {
            return Err("--play-pick-bounds-activatable needs --play-pick-bounds".into());
        }
        if self.nowait && !self.live_world {
            return Err("--play-nowait needs --play-live-world".into());
        }
        Ok(())
    }

    /// Cells about the start (and about a load door's destination).
    pub fn around(&self) -> u32 {
        self.around.unwrap_or(1)
    }

    /// The drawing side's collision readiness readings every walking
    /// world's cell build takes.
    pub fn collision(&self) -> fo4_scene::readiness::Switches {
        fo4_scene::readiness::Switches {
            off_chain: self.off_chain,
            reference_alpha: self.reference_alpha,
        }
    }

    /// The run's new-game switches as the play options take them: with
    /// `--play-activate-opens REF`, the caller's `--assume-open REF@SECONDS`
    /// and the `--action-after REF@SECONDS GATE` holding it are left out
    /// (the walker's activation of REF gives the run the open); with
    /// `--play-activate-person REF`, the caller's `--assume-activate
    /// REF@SECONDS` and its gate (the walker's activation of REF is the
    /// run's); every other switch kept in its order. Without them, the
    /// switches unchanged.
    ///
    /// With `--play-action-after-offset`, the run's `--action-after-offset`
    /// is added after the switches, and for each open or activation left
    /// out, `--input-action-at open|activate REF@SECONDS`: the walker's
    /// action on REF stands for the caller's action written at that second
    /// (`play_activate_written_second_v1`).
    pub fn run_args(&self, args: Vec<String>) -> Vec<String> {
        if self.opens.is_empty() && self.persons.is_empty() && !self.action_offset {
            return args;
        }
        // A caller action's target, `PLUGIN:OBJECTID@SECONDS`.
        let target = |t: &str| -> Option<(FormKey, f64)> {
            let (k, at) = t.rsplit_once('@')?;
            Some((fo4_vm::player::parse_key(k)?, at.parse::<f64>().ok()?))
        };
        let mut dropped: Vec<(FormKey, f64)> = Vec::new();
        let mut written: Vec<(&str, String)> = Vec::new();
        let mut out = Vec::with_capacity(args.len());
        let mut i = 0;
        while i < args.len() {
            let named = match args[i].as_str() {
                "--assume-open" => Some((&self.opens, "open")),
                "--assume-activate" => Some((&self.persons, "activate")),
                _ => None,
            };
            if let Some((named, action)) = named {
                if let Some(t) = args.get(i + 1).and_then(|t| target(t)) {
                    if named.contains(&t.0) {
                        dropped.push(t);
                        written.push((action, args[i + 1].clone()));
                        i += 2;
                        continue;
                    }
                }
            }
            out.push(args[i].clone());
            i += 1;
        }
        let mut kept = Vec::with_capacity(out.len());
        let mut i = 0;
        while i < out.len() {
            if out[i] == "--action-after"
                && i + 2 < out.len()
                && out
                    .get(i + 1)
                    .and_then(|t| target(t))
                    .is_some_and(|t| dropped.contains(&t))
            {
                i += 3;
                continue;
            }
            kept.push(out[i].clone());
            i += 1;
        }
        if self.action_offset {
            kept.push("--action-after-offset".to_string());
            for (action, t) in written {
                kept.extend(["--input-action-at".to_string(), action.to_string(), t]);
            }
        }
        kept
    }

    /// The play mode's own switches for the run.
    pub fn run_options(&self) -> crate::Options {
        crate::Options {
            region: self.region,
            recheck_marks: self.recheck_marks,
        }
    }

    /// How the walking world is built ([`world::build`]).
    pub fn world_options(&self) -> world::Options {
        world::Options {
            around: self.around(),
            live: self.live_world,
            doors: self.load_doors,
            collision: self.collision(),
            child_flag_agrees: self.child_flag_agrees,
            marker_boxes: self.marker_boxes,
        }
    }

    /// The named platforms.
    pub fn elevators(&self) -> walker::Elevators {
        walker::Elevators {
            references: self.elevators.clone(),
            also: self.also.clone(),
            also_with: self.also_with.clone(),
            door: self.elevator_door,
            held_pose: self.held_pose,
            rigid: self.rigid,
            door_onward: self.door_onward,
            door_landing: self.door_landing,
        }
    }

    /// The walker's options: the caller's speeds and tick, the slice, the
    /// reach and the collision readings of the walking worlds it builds
    /// again.
    pub fn walker_options(&self) -> walker::Options {
        let mut opts = walker::Options {
            reach: self.reach,
            collision: self.collision(),
            child_flag_agrees: self.child_flag_agrees,
            marker_boxes: self.marker_boxes,
            ..walker::Options::default()
        };
        if let Some(v) = self.walk_speed {
            opts.params.walk_speed = v;
        }
        if let Some(v) = self.run_speed {
            opts.params.run_speed = v;
        }
        if let Some(v) = self.tick {
            opts.settings.tick = v;
        }
        if let Some(v) = self.slice {
            opts.slice = v;
        }
        opts.stand = self.stand;
        opts.first_met = self.first_met;
        opts
    }

    /// The walking world about the run's start: the cell of the track's
    /// first point (an interior cell, or the exterior cell under it, the
    /// track's space resolved as the run resolves it), built with
    /// [`Play::world_options`].
    pub fn build_world(
        &self,
        m: &Manifest,
        store: &Store,
        vfs: &Vfs,
        track: &fo4_vm::track::Track,
    ) -> Result<world::Built, Box<dyn std::error::Error>> {
        let first = track.points.first().ok_or("an empty track")?;
        let b = Budget::new(m.limits.max_memory);
        // The track's space as the run resolves it.
        let mut resolved = track.clone();
        resolved.resolve(store)?;
        let space = resolved.points[0].space.clone();
        let cell = match store.signature(&first.space) {
            Some(s) if s == *b"CELL" => first.space.clone(),
            _ => world::start_cell(store, &space, first.pos, &b)?,
        };
        world::build(m, store, vfs, &cell, self.world_options())
    }

    /// The walker over the walking world `built`, driven by `script`, with
    /// every option; the switch lines of `play.txt` (what was built and
    /// every switch the walker relies on, then the run's own:
    /// [`Play::run_lines`]) are written to `report`.
    pub fn walker(
        &self,
        built: world::Built,
        script: Script,
        report: &mut String,
    ) -> Result<walker::Walker, String> {
        let around = self.around();
        let world::Built {
            world,
            refs,
            boxes,
            report: text,
            live,
            doors,
            counts,
            space,
            cell,
            unpicked,
        } = built;
        let _ = writeln!(
            report,
            "start cell {} (around {around})",
            fo4_vm::bind::record_key(&cell)
        );
        let _ = writeln!(report, "{text}");
        if self.off_chain {
            let _ = writeln!(report, "switch: a refused collision whose model blockers lie off the blocks its bodies read enters the walking world (--play-collision-off-chain, {})", fo4_scene::hypotheses::COLLISION_OFF_CHAIN);
        }
        if self.reference_alpha {
            let _ = writeln!(report, "switch: a reference's unread alpha does not refuse its collision (--play-collision-reference-alpha, {})", fo4_scene::hypotheses::REFERENCE_ALPHA_COLLISION);
        }
        if self.child_flag_agrees {
            let _ = writeln!(report, "switch: an enable chain left open only by records' own flags that agree with it decides their collision, at load and after the run's changes (--play-collision-child-flag-agrees, {})", fo4_scene::hypotheses::ENABLE_CHILD_FLAG_AGREES);
        }
        if self.marker_boxes {
            let _ = writeln!(report, "switch: the collision markers' boxes among the loaded references and the worldspace's persistent references nearby stand in the walking world, taken out when the run disables them (--play-marker-boxes, {})", crate::rules::MARKER_BOXES);
        }
        let opts = self.walker_options();
        if let Some(v) = self.slice {
            let _ = writeln!(
                report,
                "walker slices joined to {v} s at least (--play-slice)"
            );
        }
        if let Some(v) = self.stand {
            let _ = writeln!(
                report,
                "policy: a standing slice lasts {v} s at most, and the walker looks that far ahead for a moving platform before it stands (--play-stand)"
            );
        }
        for (name, rule, v, origin) in opts.params.provenance() {
            let _ = writeln!(report, "walker {name} {v} ({rule}, {origin:?})");
        }
        let _ = writeln!(report, "walker tick {} s", opts.settings.tick);
        if let Some(r) = self.reach {
            let _ = writeln!(report, "activation reach {r} units (caller assumption)");
        }
        let nowait = self.nowait;
        let mut w = match live {
            Some((dormant, info)) => {
                let _ = writeln!(
                    report,
                    "switch: the walking world follows the run's state (--play-live-world{})",
                    if nowait { ", --play-nowait" } else { "" }
                );
                let live = crate::live::Live::new(world, refs, dormant, info, space, nowait)
                    .map_err(|e| e.to_string())?
                    .with_child_flag_agrees(self.child_flag_agrees)
                    .with_unpicked(&unpicked);
                walker::Walker::new(fo4_walk::World::new(), Vec::new(), script, opts)?
                    .with_live(live, nowait)
            }
            None => walker::Walker::new(world, refs, script, opts)?,
        };
        if self.load_doors {
            let _ = writeln!(report, "switch: the activation crosses load doors (--play-load-doors): {} among the loaded references", doors.len());
            w = w.with_doors(walker::Doors { doors, around });
        }
        if self.player_moveto {
            let _ = writeln!(report, "switch: the walker goes where the run's MoveTo with zero offsets puts the player (--play-player-moveto)");
            w = w.with_player_moves();
        }
        if self.player_sits {
            let _ = writeln!(report, "switch: the walker goes to the furniture the run seats the player in (--play-player-sits)");
            w = w.with_player_sits();
        }
        if self.follow_grid {
            let _ = writeln!(report, "policy: the walking world is built again about the walker's grid square when it leaves it (--play-follow-grid, {around} cells about it)");
            w = w.with_grid(walker::Grid::new(around));
        }
        if self.region {
            let _ = writeln!(report, "switch: trigger volumes and placed actors are read about the walker's points (--play-region-walker)");
        }
        let elevators = self.elevators();
        if !elevators.references.is_empty() {
            let _ = writeln!(report, "switch: the collision of {} named reference(s) moves as the sequences the run's graph starts on them move its nodes, {} other reference(s) placed as stored (--play-elevator, --play-elevator-also){}", elevators.references.len(), elevators.also.len() + elevators.also_with.len(), if elevators.door { "; a ride that carried the walker ends at the nearest load door within the reach (--play-elevator-door)" } else { "" });
            if elevators.held_pose {
                let _ = writeln!(report, "switch: a node a sequence controls without translation keys moves to the translation the sequence holds it at (--play-elevator-held-pose)");
            }
            if elevators.door_onward {
                let _ = writeln!(report, "switch: a ride's end leaves out the load doors back into the space the walker came from (--play-elevator-door-onward)");
            }
            if elevators.door_landing {
                let _ = writeln!(report, "switch: a ride that ended with the walker in the air ends for it when it next stands on the ride (--play-elevator-door-landing)");
            }
            if elevators.rigid {
                let _ = writeln!(report, "switch: each part moves by the rigid motion the sequence gives the chain above it, every channel read, in place of the translations (--play-elevator-rigid)");
            }
            w = w.with_elevators(elevators);
        }
        for (k, v) in &counts {
            let _ = writeln!(report, "{k}\t{v}");
        }
        if !self.opens.is_empty() {
            let _ = writeln!(report, "switch: the caller's timed opens of {} named reference(s) are the walker's activations of them, the open given with the activation (--play-activate-opens, {})", self.opens.len(), crate::rules::ACTIVATE_OPENS);
            w = w.with_opens(self.opens.clone());
        }
        if !self.persons.is_empty() {
            let _ = writeln!(report, "switch: the caller's timed activations of {} named reference(s) are the walker's activations of them (--play-activate-person, {})", self.persons.len(), crate::rules::ACTIVATE_PERSON);
            w = w.with_persons(self.persons.clone());
        }
        if self.action_offset {
            let _ = writeln!(report, "switch: a caller action held until a stage keeps its offset from the stage, and the person's opens and activations stand for the caller's timed ones they replace (--play-action-after-offset, {})", crate::rules::ACTIVATE_WRITTEN_SECOND);
            w = w.with_written_seconds();
        }
        if self.first_met {
            let _ = writeln!(report, "switch: the activation's ray meets what it enters first, what the eye stands inside only when nothing else, the named platforms by their collision (--play-pick-first-met, {})", crate::rules::ACTIVATE_FIRST_MET);
        }
        if self.pick_bounds {
            let _ = writeln!(report, "switch: the activation's ray meets the object bounds of references with no collision (--play-pick-bounds)");
            let boxes = if self.activatable {
                let kept = boxes.without_kinds(&crate::rules::STATIC_KINDS);
                let _ = writeln!(report, "switch: object bounds of statics, static collections and movable statics left out (--play-pick-bounds-activatable): {} of {} boxes kept", kept.items.len(), boxes.items.len());
                w = w.with_static_boxes_left_out();
                kept
            } else {
                boxes
            };
            w = w.with_boxes(boxes);
        }
        self.run_lines(report);
        Ok(w)
    }

    /// The switch lines of the run's own options (`play.txt`), written by
    /// [`Play::walker`] after the walker's, and by a caller that drives the
    /// run otherwise (`play_run --replay`).
    pub fn run_lines(&self, report: &mut String) {
        if self.recheck_marks {
            let _ = writeln!(report, "policy: the distance marks of scenes waiting on a function 1 condition are computed again as the track grows, no completion offered (--play-recheck-marks)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &str) -> Result<Play, String> {
        let a: Vec<String> = line.split_whitespace().map(String::from).collect();
        let (p, rest) = Play::parse(&a)?;
        if let Some(x) = rest.first() {
            return Err(format!("not a play option: {x}"));
        }
        p.check()?;
        Ok(p)
    }

    fn k(id: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: id,
        }
    }

    #[test]
    fn reads_every_play_option() {
        let s = read("--play-stand 0.1 --walk-speed 300 --play-reach 150 --play-pick-bounds --play-pick-bounds-activatable --play-load-doors --play-follow-grid --play-slice 0.25 --play-live-world --play-elevator a.esm:0000000A --play-elevator a.esm:B --play-elevator-also-with a.esm:C A.ESM:0000000A --play-elevator-door --play-elevator-held-pose --play-elevator-rigid --play-elevator-door-onward --play-collision-off-chain --play-collision-reference-alpha --play-collision-child-flag-agrees --play-player-moveto --play-player-sits --play-around 2 --play-tick 0.02 --run-speed 400 --play-region-walker --play-recheck-marks --play-nowait --play-elevator-also a.esm:D").unwrap();
        assert_eq!(s.walk_speed, Some(300.0));
        assert_eq!(s.run_speed, Some(400.0));
        assert_eq!(s.tick, Some(0.02));
        assert_eq!(s.reach, Some(150.0));
        assert_eq!(s.slice, Some(0.25));
        assert_eq!(s.stand, Some(0.1));
        assert_eq!(s.around(), 2);
        assert_eq!(s.elevators, [k(0xA), k(0xB)]);
        assert_eq!(s.also, [k(0xD)]);
        assert_eq!(s.also_with, [(k(0xC), k(0xA))]);
        assert!(s.pick_bounds && s.activatable && s.load_doors && s.follow_grid);
        assert!(s.live_world && s.nowait && s.elevator_door && s.held_pose && s.rigid);
        assert!(s.door_onward && s.off_chain && s.reference_alpha && s.child_flag_agrees);
        assert!(s.player_moveto && s.player_sits && s.region && s.recheck_marks);
        assert!(s.walker_only());
        let w = s.world_options();
        assert!(w.live && w.doors && w.child_flag_agrees && w.collision.any());
        assert_eq!(w.around, 2);
        let o = s.walker_options();
        assert_eq!(o.params.walk_speed, 300.0);
        assert_eq!(o.params.run_speed, 400.0);
        assert_eq!(o.settings.tick, 0.02);
        assert_eq!(o.slice, 0.25);
        assert_eq!(o.stand, Some(0.1));
        assert_eq!(Play::default().walker_options().stand, None);
        assert!(o.child_flag_agrees);
        let e = s.elevators();
        assert!(e.door && e.held_pose && e.rigid && e.door_onward);
        assert_eq!(e.references.len(), 2);
        // Only what a keyboard's driver reads too.
        let k =
            read("--play-reach 150 --play-pick-bounds --play-live-world --play-nowait").unwrap();
        assert!(k.any() && !k.walker_only());
        assert!(!Play::default().any());
        assert_eq!(Play::default().around(), 1);
    }

    #[test]
    fn the_callers_timed_opens_of_named_references_leave_the_run() {
        let p = read("--play-activate-opens a.esm:00000001").unwrap();
        assert_eq!(p.opens, [k(1)]);
        let args: Vec<String> = "m new-game o --assume-open A.ESM:1@90 --x --action-after a.esm:00000001@90 Q:95 --assume-open a.esm:2@5 --action-after a.esm:2@5 Q:1 --assume-activate a.esm:1@3"
            .split(' ')
            .map(String::from)
            .collect();
        assert_eq!(
            p.run_args(args.clone()).join(" "),
            "m new-game o --x --assume-open a.esm:2@5 --action-after a.esm:2@5 Q:1 --assume-activate a.esm:1@3"
        );
        // Without the option the switches are unchanged.
        assert_eq!(Play::default().run_args(args.clone()), args);
        assert!(read("--play-activate-opens nokey").is_err());
    }

    #[test]
    fn the_callers_timed_activations_of_named_references_leave_the_run() {
        let p = read("--play-activate-person a.esm:00000002 --play-pick-first-met").unwrap();
        assert_eq!(p.persons, [k(2)]);
        assert!(p.first_met && p.walker_options().first_met);
        assert!(!Play::default().walker_options().first_met);
        let args: Vec<String> = "m new-game o --assume-activate a.esm:2@870 --action-after a.esm:2@870 Q:6 --assume-open a.esm:2@5 --assume-activate a.esm:3@9 --action-after a.esm:3@9 Q:1 --action-after a.esm:2@99 Q:2"
            .split(' ')
            .map(String::from)
            .collect();
        // The timed activation and its gate leave; an open of the same
        // reference, another reference's activation and a gate naming no
        // dropped action stay.
        assert_eq!(
            p.run_args(args).join(" "),
            "m new-game o --assume-open a.esm:2@5 --assume-activate a.esm:3@9 --action-after a.esm:3@9 Q:1 --action-after a.esm:2@99 Q:2"
        );
        assert!(read("--play-activate-person").is_err());
    }

    #[test]
    fn the_left_out_actions_stand_for_their_written_seconds_under_the_offset_switch() {
        let args: Vec<String> = "m new-game o --assume-activate a.esm:2@870 --action-after a.esm:2@870 Q:6 --assume-open a.esm:1@90 --assume-activate a.esm:3@9"
            .split(' ')
            .map(String::from)
            .collect();
        let p = read("--play-activate-person a.esm:00000002 --play-activate-opens a.esm:00000001 --play-action-after-offset").unwrap();
        assert!(p.action_offset);
        assert_eq!(
            p.run_args(args.clone()).join(" "),
            "m new-game o --assume-activate a.esm:3@9 --action-after-offset --input-action-at activate a.esm:2@870 --input-action-at open a.esm:1@90"
        );
        // The switch alone gives the run its offset switch only.
        let p = read("--play-action-after-offset").unwrap();
        assert_eq!(
            p.run_args(args.clone()).last().map(String::as_str),
            Some("--action-after-offset")
        );
        assert_eq!(p.run_args(args.clone()).len(), args.len() + 1);
    }

    #[test]
    fn leaves_other_arguments_in_their_order() {
        let a: Vec<String> = "m --replay 0.5 --play-live-world o --play-reach 9 x"
            .split_whitespace()
            .map(String::from)
            .collect();
        let (p, rest) = Play::parse(&a).unwrap();
        assert!(p.live_world);
        assert_eq!(p.reach, Some(9.0));
        assert_eq!(rest, ["m", "--replay", "0.5", "o", "x"]);
    }

    #[test]
    fn refuses_what_play_run_refuses() {
        for bad in [
            "--play-elevator-door --play-load-doors --play-reach 1",
            "--play-elevator-also a.esm:1",
            "--play-elevator a.esm:1 --play-elevator-door",
            "--play-elevator a.esm:1 --play-elevator-door --play-load-doors --play-reach 1 --play-elevator-door-onward --play-elevator-also-with a.esm:2 a.esm:3",
            "--play-elevator-rigid",
            "--play-elevator-door-onward",
            "--play-pick-bounds-activatable",
            "--play-nowait",
            "--play-reach 0",
            "--play-reach 1 --play-reach 2",
            "--play-slice -1",
            "--play-stand 0",
            "--play-stand 0.1 --play-stand 0.2",
            "--walk-speed nan",
            "--play-elevator nokey",
            "--play-elevator-also-with a.esm:1",
            "--play-around x",
            "--play-tick",
        ] {
            assert!(read(bad).is_err(), "{bad}");
        }
        assert!(read("--play-elevator a.esm:1 --play-elevator-door --play-load-doors --play-reach 1 --play-elevator-door-onward").is_ok());
    }

    #[test]
    fn the_run_lines_follow_the_run_options() {
        let mut r = String::new();
        Play::default().run_lines(&mut r);
        assert!(r.is_empty());
        let p = read("--play-recheck-marks").unwrap();
        p.run_lines(&mut r);
        assert!(r.contains("(--play-recheck-marks)"), "{r}");
        assert!(p.run_options().recheck_marks && !p.run_options().region);
    }
}
