//! Scripted window runs, for measurements (`--path` in the window): the
//! walker is steered through waypoints instead of by the keys, the way the
//! headless walk steers it (`crate::path`), and the run's phases decide
//! when the frames to compare are captured. Pure logic; the window's
//! systems in `crate::gpu` apply it. A property of the script, not of the
//! controller or of the game.
use crate::path::{DETOURS, DETOUR_UNITS, STALL_TICKS, STALL_UNITS};
use fo4_walk::Input;

/// Steers towards each waypoint in turn at the run speed. A goal whose
/// distance shrank by less than [`STALL_UNITS`] over [`STALL_TICKS`]
/// ticks has stalled: the walker steps aside, to the left of its line by
/// [`DETOUR_UNITS`], then to the right by twice that, and so on, at most
/// [`DETOURS`] times, then gives the waypoint up, as the headless walk
/// does. A goal also ends after four times its straight run plus two
/// seconds.
#[derive(Debug, Clone)]
pub struct Steer {
    waypoints: Vec<[f64; 2]>,
    leg: usize,
    /// The point aside of a stalled leg, while heading for it.
    aside: Option<[f64; 2]>,
    detours: u32,
    best: f64,
    best_at: u64,
    /// Ticks spent on the current goal, and the most it may take.
    ticks: u64,
    cap: Option<u64>,
    /// Waypoints given up.
    pub missed: u32,
}

/// Ticks a goal may take: four times the straight run plus two seconds.
fn cap(distance: f64, speed: f64, tick: f64) -> u64 {
    (4.0 * distance / (speed * tick)).ceil() as u64 + 120
}

impl Steer {
    pub fn new(waypoints: Vec<[f64; 2]>) -> Self {
        Self {
            waypoints,
            leg: 0,
            aside: None,
            detours: 0,
            best: f64::INFINITY,
            best_at: 0,
            ticks: 0,
            cap: None,
            missed: 0,
        }
    }

    pub fn done(&self) -> bool {
        self.leg >= self.waypoints.len()
    }

    fn next_goal(&mut self) {
        self.best = f64::INFINITY;
        self.best_at = 0;
        self.ticks = 0;
        self.cap = None;
    }

    /// The input of the next tick for a walker whose feet are at `feet`,
    /// running at `run_speed` with ticks of `tick` seconds; `None` once
    /// every waypoint is reached or given up.
    pub fn input(&mut self, feet: [f64; 3], run_speed: f64, tick: f64) -> Option<Input> {
        loop {
            let leg = *self.waypoints.get(self.leg)?;
            let goal = self.aside.unwrap_or(leg);
            let (dx, dy) = (goal[0] - feet[0], goal[1] - feet[1]);
            let d = dx.hypot(dy);
            let cap = *self.cap.get_or_insert_with(|| cap(d, run_speed, tick));
            let stalled = if d < self.best - STALL_UNITS {
                self.best = d;
                self.best_at = self.ticks;
                false
            } else {
                self.ticks - self.best_at >= STALL_TICKS
            };
            if d < 1.0 || self.ticks >= cap || stalled {
                match (self.aside.take(), stalled) {
                    // Back from aside (reached or not): the waypoint again.
                    (Some(_), _) => {}
                    (None, true) if self.detours < DETOURS => {
                        self.detours += 1;
                        let d = d.max(1e-9);
                        let side = if self.detours % 2 == 1 { 1.0 } else { -1.0 };
                        let reach = DETOUR_UNITS * f64::from(self.detours) * side;
                        self.aside = Some([feet[0] - dy / d * reach, feet[1] + dx / d * reach]);
                    }
                    (None, _) => {
                        if d >= 1.0 {
                            self.missed += 1;
                        }
                        self.leg += 1;
                        self.detours = 0;
                    }
                }
                self.next_goal();
                continue;
            }
            self.ticks += 1;
            return Some(Input {
                forward: (d / (run_speed * tick)).min(1.0),
                right: 0.0,
                heading: dy.atan2(dx),
                run: true,
            });
        }
    }
}

/// Where a scripted run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Until the cells around the start are drawn.
    Loading,
    /// Frames counted before the start frame is captured.
    Start(u32),
    /// Walking the waypoints.
    Walking,
    /// Back at the start pose, until its cells are drawn again.
    Returning,
    /// Frames counted before the return frame is captured.
    Return(u32),
    /// Captured; the window closes.
    Done,
}
impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Loading => "loading",
            Phase::Start(_) => "start",
            Phase::Walking => "walking",
            Phase::Returning => "returning",
            Phase::Return(_) => "return",
            Phase::Done => "done",
        }
    }
}

/// Frames drawn with every cell in place before a frame is captured
/// (pipelines compile and textures arrive over a few frames).
pub const SETTLE_FRAMES: u32 = 120;

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_core::Budget;
    use fo4_walk::{Character, Params, Settings, World};

    fn floor(walls: &[[[f64; 3]; 3]]) -> World {
        let b = Budget::new(16 << 20);
        let mut w = World::new();
        let (a, c, d, e) = (
            [-5000.0, -5000.0, 0.0],
            [5000.0, -5000.0, 0.0],
            [5000.0, 5000.0, 0.0],
            [-5000.0, 5000.0, 0.0],
        );
        w.add_triangles(&[[a, c, d], [a, d, e]], "floor", &b)
            .unwrap();
        if !walls.is_empty() {
            w.add_triangles(walls, "wall", &b).unwrap();
        }
        w
    }

    fn walk(w: &World, path: Vec<[f64; 2]>) -> (Character, Steer, u64) {
        let p = Params::PROVISIONAL;
        let s = Settings::DEFAULT;
        let mut c = Character::new(p, s, [0., 0., 0.]).unwrap();
        c.settle(w, 600);
        let mut steer = Steer::new(path);
        let mut n = 0;
        while let Some(input) = steer.input(c.feet, p.run_speed, s.tick) {
            c.tick(w, input);
            n += 1;
            assert!(n < 100_000);
        }
        (c, steer, n)
    }

    #[test]
    fn the_walker_reaches_each_waypoint_in_turn() {
        let w = floor(&[]);
        let (c, steer, n) = walk(&w, vec![[1000., 0.], [1000., 800.], [0., 0.]]);
        assert!(steer.done() && steer.missed == 0);
        assert!(c.feet[0].hypot(c.feet[1]) < 1.0, "{:?}", c.feet);
        // About the straight run at the run speed.
        let run = 1000. + 800. + 1000f64.hypot(800.);
        let p = Params::PROVISIONAL;
        let ideal = run / (p.run_speed * Settings::DEFAULT.tick);
        assert!((n as f64) < ideal * 1.1 + 10., "{n} ticks for {ideal}");
    }

    #[test]
    fn a_wall_across_the_way_is_walked_around() {
        // A tall wall across x = 500, from y = -300 to 300.
        let (a, b, c, d) = (
            [500., -300., -10.],
            [500., 300., -10.],
            [500., 300., 1000.],
            [500., -300., 1000.],
        );
        let w = floor(&[[a, b, c], [a, c, d]]);
        let (c, steer, _) = walk(&w, vec![[1000., 0.]]);
        assert!(steer.done() && steer.missed == 0, "{steer:?}");
        assert!((c.feet[0] - 1000.).hypot(c.feet[1]) < 1.0);
    }

    #[test]
    fn nothing_to_walk_is_done_at_once() {
        let mut steer = Steer::new(Vec::new());
        assert!(steer.done());
        assert!(steer.input([0.; 3], 300., 1. / 60.).is_none());
        // A waypoint under the feet is reached without a tick.
        let mut steer = Steer::new(vec![[0.5, 0.]]);
        assert!(steer.input([0.; 3], 300., 1. / 60.).is_none());
        assert_eq!(steer.missed, 0);
        assert_eq!(Phase::Return(3).name(), "return");
    }
}
