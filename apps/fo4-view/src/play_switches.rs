//! The play mode's switches in the window: `play_run`'s play options
//! (`crates/fo4-play/examples/play_run.rs`, `spec/play.md`), read into one
//! structure for both of the window's drivers. With the keyboard only the
//! activation's (`--play-reach`, `--play-pick-bounds`) and the live world's
//! (`--play-live-world`, `--play-nowait`) apply; with a scripted input
//! (`--play-input FILE`) the window's run is driven by
//! `fo4_play::walker::Walker` built from all of them, as `play_run` builds
//! it.
//!
//! `play_run` parses the same options inside its example, not in the
//! library, so this module wraps what `fo4-play` makes public today
//! (`walker::Options`, `walker::Elevators`, `world::Options`,
//! `fo4_play::Options`). When `fo4-play` offers the parser and the walker's
//! assembly in its library (requested in the window step's notes), this
//! module goes and both callers share that one.
use crate::cli::finite;

/// Every play option of `play_run`, as given (off or absent by default).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Switches {
    /// `--play-around N`: cells about the start in the walking world
    /// (default 1).
    pub around: Option<u32>,
    /// `--play-reach UNITS`: the activation's reach (the caller's).
    pub reach: Option<f64>,
    /// `--walk-speed`, `--run-speed`: the caller's values of the walker's
    /// speed rules.
    pub walk_speed: Option<f64>,
    pub run_speed: Option<f64>,
    /// `--play-tick SECONDS`, `--play-slice SECONDS`.
    pub tick: Option<f64>,
    pub slice: Option<f64>,
    /// `--play-pick-bounds` (`play_activate_object_bounds_v1`),
    /// `--play-pick-bounds-activatable` (`play_activate_bounds_not_static_v1`).
    pub pick_bounds: bool,
    pub activatable: bool,
    /// `--play-live-world` (`play_live_collision_follows_run_v1`),
    /// `--play-nowait`.
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
    /// `--play-elevator-door`, `--play-elevator-held-pose`,
    /// `--play-elevator-rigid`, `--play-elevator-door-onward`.
    pub elevator_door: bool,
    pub held_pose: bool,
    pub rigid: bool,
    pub door_onward: bool,
    /// `--play-player-moveto` (`play_player_moveto_v1`),
    /// `--play-player-sits` (`play_player_sit_furniture_v1`).
    pub player_moveto: bool,
    pub player_sits: bool,
    /// `--play-collision-off-chain`, `--play-collision-reference-alpha`.
    pub off_chain: bool,
    pub reference_alpha: bool,
    /// `--play-collision-child-flag-agrees`
    /// (`xesp_child_flag_agrees_chain_v1`).
    pub child_flag_agrees: bool,
    /// `--play-elevator REF`, `--play-elevator-also REF`,
    /// `--play-elevator-also-with REF OWNER`, as given (`PLUGIN:OBJECTID`).
    pub elevators: Vec<String>,
    pub also: Vec<String>,
    pub also_with: Vec<(String, String)>,
}

/// A record key as `play_run` takes it: `PLUGIN:OBJECTID`, the id in hex.
fn key(v: &str, flag: &str) -> Result<String, String> {
    match v.rsplit_once(':') {
        Some((p, id)) if !p.is_empty() && u32::from_str_radix(id, 16).is_ok() => Ok(v.to_string()),
        _ => Err(format!("{flag} takes PLUGIN:OBJECTID")),
    }
}

impl Switches {
    /// Reads the play option that starts `args`, if it is one: how many
    /// arguments it took (0: not a play option).
    pub fn take(&mut self, args: &[String]) -> Result<usize, String> {
        let Some(flag) = args.first() else {
            return Ok(0);
        };
        let value = |n: usize| {
            args.get(n)
                .map(String::as_str)
                .ok_or_else(|| format!("{flag} takes a value"))
        };
        let number = |n: usize| -> Result<f64, String> { finite(value(n)?) };
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
            "--play-player-moveto" => &mut self.player_moveto,
            "--play-player-sits" => &mut self.player_sits,
            "--play-collision-off-chain" => &mut self.off_chain,
            "--play-collision-reference-alpha" => &mut self.reference_alpha,
            "--play-collision-child-flag-agrees" => &mut self.child_flag_agrees,
            "--play-around" => {
                let v = value(1)?;
                self.around = Some(
                    v.parse()
                        .map_err(|_| format!("--play-around takes cells: {v}"))?,
                );
                return Ok(2);
            }
            "--play-reach" => {
                let r = number(1)?;
                if r <= 0.0 {
                    return Err("--play-reach takes a positive distance".into());
                }
                self.reach = Some(r);
                return Ok(2);
            }
            "--walk-speed" => {
                self.walk_speed = Some(number(1)?);
                return Ok(2);
            }
            "--run-speed" => {
                self.run_speed = Some(number(1)?);
                return Ok(2);
            }
            "--play-tick" => {
                self.tick = Some(number(1)?);
                return Ok(2);
            }
            "--play-slice" => {
                let v = number(1)?;
                if v < 0.0 {
                    return Err("--play-slice takes seconds, 0 or more".into());
                }
                self.slice = Some(v);
                return Ok(2);
            }
            "--play-elevator" => {
                self.elevators.push(key(value(1)?, flag)?);
                return Ok(2);
            }
            "--play-elevator-also" => {
                self.also.push(key(value(1)?, flag)?);
                return Ok(2);
            }
            "--play-elevator-also-with" => {
                let usage = "--play-elevator-also-with takes PLUGIN:OBJECTID PLUGIN:OBJECTID";
                let k = key(value(1).map_err(|_| usage)?, flag).map_err(|_| usage)?;
                let owner = key(value(2).map_err(|_| usage)?, flag).map_err(|_| usage)?;
                self.also_with.push((k, owner));
                return Ok(3);
            }
            _ => return Ok(0),
        };
        *on = true;
        Ok(1)
    }

    /// Whether any play option was given.
    pub fn any(&self) -> bool {
        *self != Switches::default()
    }

    /// The options given that only the scripted walker reads (not the
    /// keyboard's driver): the activation's reach and object bounds and the
    /// live world apply to both.
    pub fn walker_only(&self) -> bool {
        Switches {
            reach: None,
            pick_bounds: false,
            live_world: false,
            nowait: false,
            ..self.clone()
        }
        .any()
    }

    /// `play_run`'s checks of how the options go together, in its order.
    pub fn check(&self) -> Result<(), String> {
        let named = |k: &String| self.elevators.iter().any(|e| same_key(e, k));
        if self.also_with.iter().any(|(_, owner)| !named(owner)) {
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
}

/// Two keys as `play_run` compares them: the same plugin (any case) and
/// object id.
fn same_key(a: &str, b: &str) -> bool {
    let split = |s: &str| {
        s.rsplit_once(':')
            .map(|(p, id)| (p.to_ascii_lowercase(), u32::from_str_radix(id, 16).ok()))
    };
    split(a) == split(b)
}

#[cfg(feature = "script-event")]
mod walker {
    use super::Switches;
    use fo4_play::{input::Script, walker, world};
    use fo4_plugin::FormKey;
    use std::fmt::Write as _;

    fn parse(k: &str) -> Result<FormKey, String> {
        fo4_vm::player::parse_key(k).ok_or_else(|| format!("not a record key: {k}"))
    }

    impl Switches {
        /// The drawing side's collision readiness readings every walking
        /// world's cell build takes.
        pub fn collision(&self) -> fo4_scene::readiness::Switches {
            fo4_scene::readiness::Switches {
                off_chain: self.off_chain,
                reference_alpha: self.reference_alpha,
            }
        }

        /// The play mode's own switches for the run.
        pub fn run_options(&self) -> fo4_play::Options {
            fo4_play::Options {
                region: self.region,
                recheck_marks: self.recheck_marks,
            }
        }

        /// How the walking world is built (`fo4_play::world::build`).
        pub fn world_options(&self) -> world::Options {
            world::Options {
                around: self.around.unwrap_or(1),
                live: self.live_world,
                doors: self.load_doors,
                collision: self.collision(),
                child_flag_agrees: self.child_flag_agrees,
            }
        }

        /// The named platforms.
        pub fn elevators(&self) -> Result<walker::Elevators, String> {
            let mut e = walker::Elevators {
                door: self.elevator_door,
                held_pose: self.held_pose,
                rigid: self.rigid,
                door_onward: self.door_onward,
                ..walker::Elevators::default()
            };
            for k in &self.elevators {
                e.references.push(parse(k)?);
            }
            for k in &self.also {
                e.also.push(parse(k)?);
            }
            for (k, owner) in &self.also_with {
                e.also_with.push((parse(k)?, parse(owner)?));
            }
            Ok(e)
        }

        /// The walker `play_run` builds from these options over the
        /// walking world `built`, driven by `script`; what it relies on is
        /// written to `report` in `play_run`'s words.
        pub fn walker(
            &self,
            built: world::Built,
            script: Script,
            report: &mut String,
        ) -> Result<walker::Walker, String> {
            let around = self.around.unwrap_or(1);
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
                ..
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
            let mut opts = walker::Options {
                reach: self.reach,
                collision: self.collision(),
                child_flag_agrees: self.child_flag_agrees,
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
                let _ = writeln!(
                    report,
                    "walker slices joined to {v} s at least (--play-slice)"
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
                    let live = fo4_play::live::Live::new(world, refs, dormant, info, space, nowait)
                        .map_err(|e| e.to_string())?
                        .with_child_flag_agrees(self.child_flag_agrees);
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
            let elevators = self.elevators()?;
            if !elevators.references.is_empty() {
                let _ = writeln!(report, "switch: the collision of {} named reference(s) moves as the sequences the run's graph starts on them move its nodes, {} other reference(s) placed as stored (--play-elevator, --play-elevator-also){}", elevators.references.len(), elevators.also.len() + elevators.also_with.len(), if elevators.door { "; a ride that carried the walker ends at the nearest load door within the reach (--play-elevator-door)" } else { "" });
                if elevators.held_pose {
                    let _ = writeln!(report, "switch: a node a sequence controls without translation keys moves to the translation the sequence holds it at (--play-elevator-held-pose)");
                }
                if elevators.door_onward {
                    let _ = writeln!(report, "switch: a ride's end leaves out the load doors back into the space the walker came from (--play-elevator-door-onward)");
                }
                if elevators.rigid {
                    let _ = writeln!(report, "switch: each part moves by the rigid motion the sequence gives the chain above it, every channel read, in place of the translations (--play-elevator-rigid)");
                }
                w = w.with_elevators(elevators);
            }
            for (k, v) in &counts {
                let _ = writeln!(report, "{k}\t{v}");
            }
            if self.pick_bounds {
                let _ = writeln!(report, "switch: the activation's ray meets the object bounds of references with no collision (--play-pick-bounds)");
                let boxes = if self.activatable {
                    let kept = boxes.without_kinds(&fo4_play::rules::STATIC_KINDS);
                    let _ = writeln!(report, "switch: object bounds of statics, static collections and movable statics left out (--play-pick-bounds-activatable): {} of {} boxes kept", kept.items.len(), boxes.items.len());
                    w = w.with_static_boxes_left_out();
                    kept
                } else {
                    boxes
                };
                w = w.with_boxes(boxes);
            }
            if self.recheck_marks {
                let _ = writeln!(report, "policy: the distance marks of scenes waiting on a function 1 condition are computed again as the track grows, no completion offered (--play-recheck-marks)");
            }
            Ok(w)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &str) -> Result<Switches, String> {
        let a: Vec<String> = line.split_whitespace().map(String::from).collect();
        let mut s = Switches::default();
        let mut i = 0;
        while i < a.len() {
            let n = s.take(&a[i..])?;
            if n == 0 {
                return Err(format!("not a play option: {}", a[i]));
            }
            i += n;
        }
        s.check()?;
        Ok(s)
    }

    #[test]
    fn reads_play_runs_options() {
        let s = read("--walk-speed 300 --play-reach 150 --play-pick-bounds --play-pick-bounds-activatable --play-load-doors --play-follow-grid --play-slice 0.25 --play-live-world --play-elevator a.esm:0000000A --play-elevator a.esm:B --play-elevator-also-with a.esm:C A.ESM:0000000A --play-elevator-door --play-elevator-held-pose --play-elevator-rigid --play-elevator-door-onward --play-collision-off-chain --play-collision-reference-alpha --play-collision-child-flag-agrees --play-player-moveto --play-player-sits --play-around 2 --play-tick 0.02 --run-speed 400 --play-region-walker --play-recheck-marks --play-nowait --play-elevator-also a.esm:D").unwrap();
        assert_eq!(s.walk_speed, Some(300.0));
        assert_eq!(s.reach, Some(150.0));
        assert_eq!(s.slice, Some(0.25));
        assert_eq!(s.around, Some(2));
        assert_eq!(s.elevators, ["a.esm:0000000A", "a.esm:B"]);
        assert_eq!(s.also, ["a.esm:D"]);
        assert_eq!(s.also_with.len(), 1);
        assert!(s.pick_bounds && s.activatable && s.load_doors && s.follow_grid);
        assert!(s.live_world && s.nowait && s.elevator_door && s.held_pose && s.rigid);
        assert!(s.door_onward && s.off_chain && s.reference_alpha && s.child_flag_agrees);
        assert!(s.player_moveto && s.player_sits && s.region && s.recheck_marks);
        assert!(s.walker_only());
        // Only what the keyboard's driver reads too.
        let k =
            read("--play-reach 150 --play-pick-bounds --play-live-world --play-nowait").unwrap();
        assert!(k.any() && !k.walker_only());
        assert!(!Switches::default().any());
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
            "--play-slice -1",
            "--play-elevator nokey",
            "--play-elevator-also-with a.esm:1",
            "--play-around x",
        ] {
            assert!(read(bad).is_err(), "{bad}");
        }
        assert!(read("--play-elevator a.esm:1 --play-elevator-door --play-load-doors --play-reach 1 --play-elevator-door-onward").is_ok());
    }
}
