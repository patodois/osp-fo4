//! Kinematic character: a vertical capsule moved by collide-and-slide at a
//! fixed time step.
//!
//! One tick: the horizontal velocity comes from the input (on the ground)
//! or is kept (in the air, `walk_no_air_control_v1`); gravity acts in the
//! air. The displacement is applied in substeps of at most a quarter of the
//! radius, and after each substep every overlap is pushed out, deepest
//! first: up along z for a walkable contact (normal within the slope
//! limit), horizontally for a steep one, along the normal otherwise. A
//! blocked move on the ground is retried raised by the step height and
//! lowered again onto a walkable surface (step-up; when the tick's move is
//! shorter than the radius the attempt looks one radius ahead, so a tick
//! that climbs a step can advance further than its own displacement).
//! Finally the capsule is
//! lowered by up to the step height (on the ground) or a small margin (in
//! the air) to find walkable ground (ground detection and snapping down
//! stairs and ramps). This is a general design written for this crate.
use crate::geometry::{self as g, V3};
use crate::set::Space;
use crate::world::Contact;
use crate::Params;
use fo4_core::{Error, Result};
use serde::Serialize;

/// Numerical settings of the controller. They are not claims about the
/// game; they bound the work and the error of the integration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Settings {
    /// Fixed time step, seconds.
    pub tick: f64,
    /// Largest substep as a fraction of the radius.
    pub substep_fraction: f64,
    /// Push-out iterations per substep.
    pub iterations: u32,
    /// Distance below the feet within which an airborne capsule counts as
    /// landed, world units.
    pub ground_margin: f64,
}
impl Settings {
    pub const DEFAULT: Self = Self {
        tick: 1.0 / 60.0,
        substep_fraction: 0.25,
        iterations: 12,
        ground_margin: 0.05,
    };
    pub fn validate(&self) -> Result<()> {
        let ok = self.tick.is_finite()
            && self.tick > 0.0
            && self.tick <= 0.1
            && self.substep_fraction.is_finite()
            && self.substep_fraction > 0.0
            && self.substep_fraction <= 1.0
            && (1..=64).contains(&self.iterations)
            && self.ground_margin.is_finite()
            && self.ground_margin >= 0.0;
        if ok {
            Ok(())
        } else {
            Err(Error::new(
                "InvalidWalkSettings",
                "walk settings",
                0,
                "0 < tick <= 0.1, 0 < substep_fraction <= 1, 1..=64 iterations, margin >= 0",
                format!("{self:?}"),
            ))
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One tick of input. `forward` and `right` are in [-1, 1] (longer
/// combined input is normalised); `heading` is the facing in radians from
/// +X towards +Y.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Input {
    pub forward: f64,
    pub right: f64,
    pub heading: f64,
    pub run: bool,
}

/// The walkable primitive the character stands on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Ground {
    /// Index into the entries of the [`Space`] (`World::entries`).
    pub entry: u32,
    /// Index into the sources of the [`Space`] (`World::sources`).
    pub source: u32,
    /// Contact normal (unit, towards the character).
    pub normal: V3,
}

/// The character after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct State {
    pub tick: u64,
    /// Lowest point of the capsule.
    pub feet: V3,
    pub velocity: V3,
    pub grounded: bool,
    pub ground: Option<Ground>,
    /// The tick climbed a step.
    pub stepped: bool,
    /// A steep contact kept the tick to less than half its horizontal move.
    pub blocked: bool,
    /// The last steep or downward-facing contact pushed out during the
    /// tick's move (a wall, a ceiling, a steep slope).
    pub wall: Option<Ground>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Flags {
    walkable: Option<Ground>,
    wall: bool,
    ceiling: bool,
    last_wall: Option<Ground>,
}

/// Overlap below this depth is accepted (world units).
const PENETRATION_TOLERANCE: f64 = 1e-6;
/// Seconds of one tick of [`Character::walks_to`] (the largest tick the
/// settings allow).
pub const WALK_PROBE_TICK: f64 = 0.1;
/// Most ticks one [`Character::walks_to`] runs.
pub const WALK_PROBE_MAX_TICKS: u64 = 400;
/// Most ticks one [`Character::step`] runs; a longer frame drops the rest.
pub const MAX_TICKS_PER_STEP: u32 = 30;
/// Most substeps of one move.
const MAX_SUBSTEPS: f64 = 4096.0;
/// Halvings of a ground-probe substep when looking for the first touch:
/// the substep shrinks by 2^-32 (under 1e-9 units for a radius below 16
/// units), well inside [`PENETRATION_TOLERANCE`].
const TOUCH_BISECTIONS: u32 = 32;

#[derive(Debug, Clone)]
pub struct Character {
    pub params: Params,
    pub settings: Settings,
    pub feet: V3,
    pub velocity: V3,
    pub grounded: bool,
    pub ground: Option<Ground>,
    pub ticks: u64,
    accumulator: f64,
    scratch: Vec<u32>,
    contacts: Vec<Contact>,
}

impl Character {
    /// A character standing (or floating) with its feet at `feet`, at rest.
    pub fn new(params: Params, settings: Settings, feet: V3) -> Result<Self> {
        params.validate()?;
        settings.validate()?;
        if !g::finite(feet) {
            return Err(Error::new(
                "InvalidWalkStart",
                "walk",
                0,
                "finite position",
                format!("{feet:?}"),
            ));
        }
        Ok(Self {
            params,
            settings,
            feet,
            velocity: [0.0; 3],
            grounded: false,
            ground: None,
            ticks: 0,
            accumulator: 0.0,
            scratch: Vec::new(),
            contacts: Vec::new(),
        })
    }

    /// Camera position: the feet raised by the eye height.
    pub fn eye(&self) -> V3 {
        g::add(self.feet, [0.0, 0.0, self.params.eye_height])
    }

    pub fn state(&self) -> State {
        State {
            tick: self.ticks,
            feet: self.feet,
            velocity: self.velocity,
            grounded: self.grounded,
            ground: self.ground,
            stepped: false,
            blocked: false,
            wall: None,
        }
    }

    /// Core segment of the capsule for feet at `f`.
    pub fn segment(&self, f: V3) -> [V3; 2] {
        let r = self.params.radius;
        [
            [f[0], f[1], f[2] + r],
            [f[0], f[1], f[2] + self.params.height - r],
        ]
    }

    fn cos_limit(&self) -> f64 {
        self.params.max_slope_degrees.to_radians().cos()
    }

    /// Pushes the capsule out of every overlap, deepest first.
    fn resolve<W: Space + ?Sized>(&mut self, world: &W, mut feet: V3, flags: &mut Flags) -> V3 {
        let cos = self.cos_limit();
        for _ in 0..self.settings.iterations {
            let seg = self.segment(feet);
            world.contacts(
                seg,
                self.params.radius,
                0.0,
                &mut self.scratch,
                &mut self.contacts,
            );
            let deepest = self
                .contacts
                .iter()
                .filter(|c| c.depth > PENETRATION_TOLERANCE)
                .max_by(|a, b| a.depth.total_cmp(&b.depth).then(b.entry.cmp(&a.entry)))
                .copied();
            let Some(c) = deepest else { break };
            let n = c.normal;
            let touched = Ground {
                entry: c.entry,
                source: world.source_of(c.entry),
                normal: n,
            };
            if n[2] < cos {
                flags.last_wall = Some(touched);
            }
            let push = if n[2] >= cos {
                flags.walkable = Some(touched);
                [0.0, 0.0, c.depth / n[2]]
            } else if n[2] > 0.0 {
                flags.wall = true;
                let h = n[0].hypot(n[1]);
                let d = c.depth / h;
                [n[0] / h * d, n[1] / h * d, 0.0]
            } else {
                if n[2] < -0.5 {
                    flags.ceiling = true;
                } else {
                    flags.wall = true;
                }
                g::scale(n, c.depth)
            };
            feet = g::add(feet, push);
        }
        feet
    }

    fn substeps(&self, distance: f64) -> u32 {
        let most = self.params.radius * self.settings.substep_fraction;
        (distance / most).ceil().clamp(1.0, MAX_SUBSTEPS) as u32
    }

    fn move_and_slide<W: Space + ?Sized>(
        &mut self,
        world: &W,
        mut feet: V3,
        disp: V3,
        flags: &mut Flags,
    ) -> V3 {
        let n = self.substeps(g::length(disp));
        let part = g::scale(disp, 1.0 / f64::from(n));
        for _ in 0..n {
            feet = g::add(feet, part);
            feet = self.resolve(world, feet, flags);
        }
        feet
    }

    fn overlapping<W: Space + ?Sized>(&mut self, world: &W, feet: V3) -> bool {
        let seg = self.segment(feet);
        world.contacts(
            seg,
            self.params.radius,
            0.0,
            &mut self.scratch,
            &mut self.contacts,
        );
        self.contacts
            .iter()
            .any(|c| c.depth > PENETRATION_TOLERANCE)
    }

    /// Lowers the capsule by up to `distance` until a walkable contact
    /// holds it. A substep that meets only steep contacts is tried again
    /// from where the capsule first touches ([`Self::first_touch`]).
    fn probe_ground<W: Space + ?Sized>(
        &mut self,
        world: &W,
        feet: V3,
        distance: f64,
    ) -> Option<(V3, Ground)> {
        let n = self.substeps(distance);
        let part = distance / f64::from(n);
        let mut f = feet;
        for _ in 0..n {
            let before = f;
            let mut flags = Flags::default();
            f[2] -= part;
            f = self.resolve(world, f, &mut flags);
            if let Some(ground) = flags.walkable {
                return Some((f, ground));
            }
            if flags.wall || flags.ceiling {
                if let Some(found) = self.first_touch(world, before, part) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// The capsule lowered from `from` (not overlapping) until it first
    /// touches something, at most `part`; the ground there if that contact
    /// is walkable. Sinking a whole substep into an edge or a vertex tilts
    /// the contact normal (the deeper, the steeper), so a capsule resting
    /// on an edge within the slope limit would otherwise be found steep by
    /// the long probe on the ground and walkable by the short one in the
    /// air, and alternate between the two.
    fn first_touch<W: Space + ?Sized>(
        &mut self,
        world: &W,
        from: V3,
        part: f64,
    ) -> Option<(V3, Ground)> {
        let at = |d: f64| [from[0], from[1], from[2] - d];
        if self.overlapping(world, from) || !self.overlapping(world, at(part)) {
            return None;
        }
        let (mut free, mut touching) = (0.0, part);
        for _ in 0..TOUCH_BISECTIONS {
            let mid = 0.5 * (free + touching);
            if self.overlapping(world, at(mid)) {
                touching = mid;
            } else {
                free = mid;
            }
        }
        let mut flags = Flags::default();
        let f = self.resolve(world, at(touching), &mut flags);
        flags.walkable.map(|ground| (f, ground))
    }

    /// Step-up: raised by the step height, moved, lowered onto walkable
    /// ground. First with the tick's own displacement; if that lands no
    /// further than `baseline`, again with the displacement lengthened to
    /// one radius, accepted only when it ends higher than `start` (a tick's
    /// move is often too short to carry the capsule over an edge).
    fn try_step<W: Space + ?Sized>(
        &mut self,
        world: &W,
        start: V3,
        horizontal: V3,
        baseline: f64,
    ) -> Option<V3> {
        let step = self.params.step_height;
        let up = g::add(start, [0.0, 0.0, step]);
        if step <= 0.0 || self.overlapping(world, up) {
            return None;
        }
        let progress = |f: V3| (f[0] - start[0]).hypot(f[1] - start[1]);
        let len = g::length(horizontal);
        let ahead = if len > 0.0 && len < self.params.radius {
            Some(g::scale(horizontal, self.params.radius / len))
        } else {
            None
        };
        for (h, lookahead) in [(Some(horizontal), false), (ahead, true)] {
            let Some(h) = h else { continue };
            let mut flags = Flags::default();
            let moved = self.move_and_slide(world, up, h, &mut flags);
            let reach = step + self.settings.ground_margin.max(1e-3);
            let Some((down, _)) = self.probe_ground(world, moved, reach) else {
                continue;
            };
            let higher = down[2] > start[2] + 1e-3;
            if down[2] <= start[2] + step + 1e-6
                && progress(down) > baseline + 1e-6
                && (!lookahead || higher)
            {
                return Some(down);
            }
        }
        None
    }

    /// Advances one fixed tick.
    pub fn tick<W: Space + ?Sized>(&mut self, world: &W, input: Input) -> State {
        let p = self.params;
        let dt = self.settings.tick;
        let clamp = |x: f64| {
            if x.is_finite() {
                x.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        let heading = if input.heading.is_finite() {
            input.heading
        } else {
            0.0
        };
        let (s, c) = heading.sin_cos();
        let mut wish = g::add(
            g::scale([c, s, 0.0], clamp(input.forward)),
            g::scale([s, -c, 0.0], clamp(input.right)),
        );
        let wl = g::length(wish);
        if wl > 1.0 {
            wish = g::scale(wish, 1.0 / wl);
        }
        let speed = if input.run { p.run_speed } else { p.walk_speed };
        let was_grounded = self.grounded;
        if was_grounded || !p.no_air_control {
            self.velocity[0] = wish[0] * speed;
            self.velocity[1] = wish[1] * speed;
        }
        if was_grounded {
            self.velocity[2] = 0.0;
        } else {
            self.velocity[2] -= p.gravity * dt;
        }
        let start = self.feet;
        let disp = g::scale(self.velocity, dt);
        let mut flags = Flags::default();
        let mut feet = self.move_and_slide(world, start, disp, &mut flags);
        let progress = |f: V3| (f[0] - start[0]).hypot(f[1] - start[1]);
        let want = disp[0].hypot(disp[1]);
        let mut stepped = false;
        if was_grounded && flags.wall && want > 0.0 && progress(feet) < want - 1e-9 {
            if let Some(alt) = self.try_step(world, start, [disp[0], disp[1], 0.0], progress(feet))
            {
                feet = alt;
                stepped = true;
            }
        }
        let mut ground = None;
        if self.velocity[2] <= 0.0 {
            let reach = if was_grounded || stepped {
                p.step_height.max(self.settings.ground_margin)
            } else {
                self.settings.ground_margin
            };
            if reach > 0.0 {
                if let Some((f, gr)) = self.probe_ground(world, feet, reach) {
                    feet = f;
                    ground = Some(gr);
                }
            }
        }
        if flags.ceiling && self.velocity[2] > 0.0 {
            self.velocity[2] = 0.0;
        }
        self.grounded = ground.is_some();
        if self.grounded {
            self.velocity[2] = 0.0;
        }
        let blocked = flags.wall && want > 0.0 && progress(feet) < 0.5 * want;
        self.feet = feet;
        self.ground = ground;
        self.ticks += 1;
        State {
            stepped,
            blocked,
            wall: flags.last_wall,
            ..self.state()
        }
    }

    /// Advances by `dt` seconds of wall time in whole fixed ticks (at most
    /// [`MAX_TICKS_PER_STEP`]); the remainder waits for the next call.
    pub fn step<W: Space + ?Sized>(&mut self, world: &W, input: Input, dt: f64) -> State {
        if dt.is_finite() && dt > 0.0 {
            self.accumulator += dt;
        }
        let tick = self.settings.tick;
        let mut last = self.state();
        let mut n = 0;
        while self.accumulator + 1e-9 >= tick {
            if n == MAX_TICKS_PER_STEP {
                self.accumulator = 0.0;
                break;
            }
            last = self.tick(world, input);
            self.accumulator -= tick;
            n += 1;
        }
        last
    }

    /// Whether a copy of the character, walked (at the run speed, in ticks
    /// of [`WALK_PROBE_TICK`] seconds or shorter, a tick's move at most
    /// half a radius) from its feet straight toward `to`
    /// on the ground, gets as far along the line as `to` (less half a radius) on the ground plane
    /// without being blocked, without sliding more than a radius off the
    /// straight line and without dropping more than its height below the
    /// lower of the two ends. Reads only: the character is not moved. A
    /// probe for a cue, not a rule of the game: the copy walks as the
    /// walker would (steps, slopes, walls), so furniture lower than the
    /// eye stops it where a sight line passes over.
    pub fn walks_to<W: Space + ?Sized>(&self, world: &W, to: V3) -> bool {
        let mut c = self.clone();
        // A tick moves the copy at most half a radius: a longer move
        // raised by the step height crosses more than one riser of a
        // flight of steps and is refused where the walker, at its own
        // tick, climbs (porch steps of 8 units).
        let speed = c.params.run_speed.max(c.params.walk_speed);
        let tick = if speed > 0.0 && c.params.radius > 0.0 {
            WALK_PROBE_TICK.min(0.5 * c.params.radius / speed)
        } else {
            WALK_PROBE_TICK
        };
        c.settings.tick = tick;
        c.accumulator = 0.0;
        c.velocity = [0.0; 3];
        let from = c.feet;
        let d = [to[0] - from[0], to[1] - from[1]];
        let len = d[0].hypot(d[1]);
        let reach = 0.5 * c.params.radius;
        if len <= reach {
            return true;
        }
        let dir = [d[0] / len, d[1] / len];
        let heading = dir[1].atan2(dir[0]);
        let input = Input {
            forward: 1.0,
            right: 0.0,
            heading,
            run: true,
        };
        let per_tick = speed * tick;
        if per_tick.is_nan() || per_tick <= 0.0 {
            return false;
        }
        let floor = from[2].min(to[2]) - c.params.height;
        let ticks = (len / per_tick).ceil() as u64 + 8;
        // As far as WALK_PROBE_MAX_TICKS ticks of WALK_PROBE_TICK go.
        let most = (WALK_PROBE_MAX_TICKS as f64 * WALK_PROBE_TICK / tick).ceil() as u64;
        for _ in 0..ticks.min(most) {
            let s = c.tick(world, input);
            let rel = [s.feet[0] - from[0], s.feet[1] - from[1]];
            let off = (rel[0] * dir[1] - rel[1] * dir[0]).abs();
            if s.blocked || off > c.params.radius || s.feet[2] < floor {
                return false;
            }
            // Reached: as far along the line as the target, less the
            // reach (a tick may step past it).
            if rel[0] * dir[0] + rel[1] * dir[1] >= len - reach {
                return true;
            }
        }
        false
    }

    /// Ticks without input until the character is on the ground and has not
    /// moved for two ticks, or `max_ticks` pass. Returns the ticks run.
    pub fn settle<W: Space + ?Sized>(&mut self, world: &W, max_ticks: u64) -> u64 {
        let mut still = 0;
        for n in 0..max_ticks {
            let before = self.feet;
            let s = self.tick(world, Input::default());
            let moved = g::length(g::sub(s.feet, before));
            if s.grounded && moved < 1e-9 {
                still += 1;
                if still == 2 {
                    return n + 1;
                }
            } else {
                still = 0;
            }
        }
        max_ticks
    }
}
