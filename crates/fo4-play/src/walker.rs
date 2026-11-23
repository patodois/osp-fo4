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
use crate::input::{Command, Controls, Script};
use crate::{pick, Act, Driver, Next, PlayerAction, Point, Slice, View};
use fo4_plugin::FormKey;
use fo4_walk::{Character, Params, Settings, World};
use std::cell::RefCell;
use std::rc::Rc;

/// A tick that moves the walker's feet less than this (world units) from
/// the last point gives no point: the ground probe's rounding is not a
/// move (a numerical setting).
pub const STILL: f64 = 1e-6;

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
}
impl Default for Options {
    fn default() -> Self {
        Self {
            params: Params::PROVISIONAL,
            settings: Settings::DEFAULT,
            reach: None,
            settle_ticks: 600,
        }
    }
}

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
    /// Rules relied on.
    pub rules: std::collections::BTreeSet<&'static str>,
}

/// The walker driven by a scripted input.
pub struct Walker {
    world: World,
    refs: Vec<Option<FormKey>>,
    boxes: Option<pick::Boxes>,
    script: Script,
    opts: Options,
    next_event: usize,
    controls: Controls,
    character: Option<Character>,
    space: Option<FormKey>,
    t: f64,
    last: Option<Point>,
    still: bool,
    visibility: crate::state::Visibility,
    end: f64,
    pub log: Rc<RefCell<Log>>,
}

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
        let activates = script.events.iter().any(|e| e.command == Command::Activate);
        if activates && !opts.reach.is_some_and(|r| r.is_finite() && r > 0.0) {
            return Err("PlayReach: the input activates; give the reach (--play-reach UNITS, the caller's assumption)".into());
        }
        let end = script
            .end()
            .or_else(|| script.events.last().map(|e| e.t))
            .unwrap_or(0.0);
        Ok(Walker {
            world,
            refs,
            boxes: None,
            script,
            opts,
            next_event: 0,
            controls: Controls::default(),
            character: None,
            space: None,
            t: 0.0,
            last: None,
            still: true,
            visibility: Default::default(),
            end,
            log: Rc::new(RefCell::new(Log::default())),
        })
    }

    /// Lets the activation meet the object bounds of references with no
    /// collision (`play_activate_object_bounds_v1`, `--play-pick-bounds`).
    pub fn with_boxes(mut self, boxes: pick::Boxes) -> Walker {
        self.boxes = Some(boxes);
        self
    }

    /// Places the walker at the run's first point, settled on the ground.
    fn start(&mut self, now: f64, first: Option<&Point>) -> Result<Vec<Point>, String> {
        let first = first.ok_or("PlayNoTrack")?.clone();
        let mut c = Character::new(self.opts.params, self.opts.settings, first.pos)
            .map_err(|e| e.to_string())?;
        let ticks = c.settle(&self.world, self.opts.settle_ticks);
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

    fn activate(&mut self) -> Option<Act> {
        let c = self.character.as_ref()?;
        let eye = c.eye();
        let dir = self.controls.look();
        let reach = self.opts.reach.unwrap_or(0.0);
        let vis = &self.visibility;
        let hit = pick::first_of(
            &self.world,
            &self.refs,
            self.boxes.as_ref(),
            eye,
            dir,
            reach,
            &|k| vis.disabled(k),
        );
        if let Some(pick::Hit {
            via: pick::Via::Bounds { .. },
            ..
        }) = &hit
        {
            self.log
                .borrow_mut()
                .rules
                .insert(crate::rules::ACTIVATE_BOUNDS);
        }
        let mut log = self.log.borrow_mut();
        log.rules.insert(crate::rules::ACTIVATE_RAY);
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
                Some(k) => (
                    Ok((k.clone(), h.distance)),
                    Some(Act {
                        at: self.t,
                        action: PlayerAction::Activate,
                        reference: k,
                    }),
                ),
            },
        };
        log.activations.push((self.t, eye, dir, record));
        act
    }

    /// The commands due by the walker's present second: the controls they
    /// change and the actions they make.
    fn commands(&mut self) -> Vec<Act> {
        let mut actions = Vec::new();
        while let Some(e) = self.script.events.get(self.next_event).copied() {
            if e.t > self.t {
                break;
            }
            self.next_event += 1;
            match e.command {
                Command::Activate => actions.extend(self.activate()),
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
        actions
    }

    /// One slice: the walker from its present second to the next tick or
    /// command, with the commands due at the slice's end (an action at a
    /// second reaches the run in the slice that ends there, before the run
    /// does what else is due at it).
    ///
    /// `now` and `first`: the run's present second and its track's first
    /// point (read once, at the first slice).
    pub fn step(&mut self, now: f64, first: Option<&Point>) -> Result<Next, String> {
        let mut points = Vec::new();
        let mut actions = Vec::new();
        if self.character.is_none() {
            points = self.start(now, first)?;
            actions = self.commands();
        }
        if self.t >= self.end && points.is_empty() && actions.is_empty() {
            return Ok(Next::Finish);
        }
        let next_command = self
            .script
            .events
            .get(self.next_event)
            .map_or(self.end, |e| e.t)
            .min(self.end);
        let keys = {
            let c = &self.controls;
            c.forward || c.back || c.left || c.right
        };
        let grounded = self.character.as_ref().is_some_and(|c| c.grounded);
        self.log.borrow_mut().slices += 1;
        // Standing: no key, on the ground and not moved by the last tick.
        if !keys && self.still && grounded {
            let until = next_command.max(self.t);
            self.t = until;
            actions.extend(self.commands());
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
        let tick = self.opts.settings.tick;
        let to = (self.t + tick).min(next_command.max(self.t + 1e-9));
        let dt = to - self.t;
        let c = self.character.as_mut().ok_or("PlayNoWalker")?;
        c.settings.tick = dt;
        let s = c.tick(&self.world, self.controls.walker());
        c.settings.tick = tick;
        {
            let mut log = self.log.borrow_mut();
            log.ticks += 1;
            log.blocked += u64::from(s.blocked);
            log.stepped += u64::from(s.stepped);
            log.airborne += u64::from(!s.grounded);
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
        actions.extend(self.commands());
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

impl Driver for Walker {
    fn next(&mut self, run: &View<'_, '_>) -> Next {
        let _ = self.visibility.update(run.vm);
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
    use fo4_core::Budget;

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
    fn an_activation_needs_the_callers_reach() {
        let script = Script::parse("1 activate\n").unwrap();
        assert!(Walker::new(floor(), vec![], script, Options::default()).is_err());
    }
}
