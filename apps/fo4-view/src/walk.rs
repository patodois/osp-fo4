//! Walk mode of the windowed viewer: the `fo4-walk` controller on the cell's
//! placed collision, steered by the same keys and mouse as the free-fly
//! camera, with the camera at the controller's eye height. Pure functions
//! of input deltas, like `fly`. Every movement number is a provisional
//! registered rule (`spec/walk.md`), not a game value.
//!
//! Fall recovery and the return to the start pose are viewer conveniences
//! (`spec/walk.md`, "Viewer"): they claim nothing about the game.
use crate::fly::{FlyInput, FlySettings, FlyState};
use fo4_walk::{Character, Input, Params, Settings, World};

/// Seconds the walker may stay in the air below the lowest collision of
/// the loaded world before it is put back at its last grounded position.
/// A viewer setting, not a game value.
pub const RECOVER_AFTER: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct WalkView {
    pub character: Character,
    /// Orientation of the view; its eye follows the character.
    pub look: FlyState,
    /// Feet at the end of the last frame spent on the ground (the start
    /// until the walker first lands).
    pub last_grounded: [f64; 3],
    /// Seconds spent in the air below the world's lowest collision.
    pub below_for: f64,
    /// Times the walker was put back after falling out of the world.
    pub recoveries: u32,
}

impl WalkView {
    /// A character whose eye is at the free-fly eye, looking the same way.
    pub fn from_fly(state: FlyState, params: Params, settings: Settings) -> Result<Self, String> {
        let feet = [state.eye[0], state.eye[1], state.eye[2] - params.eye_height];
        let character = Character::new(params, settings, feet).map_err(|e| e.to_string())?;
        let mut view = Self {
            character,
            look: state,
            last_grounded: feet,
            below_for: 0.0,
            recoveries: 0,
        };
        view.look.eye = view.character.eye();
        Ok(view)
    }

    /// One frame: the mouse turns the view as in fly mode; W/A/S/D walk
    /// along the view's heading on the ground, Shift runs. Then
    /// [`WalkView::recover`].
    pub fn step(&mut self, world: &World, input: FlyInput, settings: FlySettings, dt: f64) {
        let turn = FlyInput {
            forward: 0.0,
            right: 0.0,
            look: input.look,
            fast: false,
        };
        let turned = self.look.step(turn, settings, dt);
        self.character.step(
            world,
            Input {
                forward: input.forward,
                right: input.right,
                heading: turned.yaw,
                run: input.fast,
            },
            dt,
        );
        self.recover(world, dt);
        self.look = FlyState {
            eye: self.character.eye(),
            ..turned
        };
    }

    /// Fall recovery, a viewer convenience: while the walker is in the air
    /// with its feet below the lowest point of every primitive of `world`
    /// (or the world is empty), the time is counted; past
    /// [`RECOVER_AFTER`] seconds the walker stands again, at rest, at its
    /// last grounded position. Returns whether it was put back.
    pub fn recover(&mut self, world: &World, dt: f64) -> bool {
        let c = &self.character;
        if c.grounded {
            self.last_grounded = c.feet;
            self.below_for = 0.0;
            return false;
        }
        let below = world.bounds.is_none_or(|b| c.feet[2] < b.min[2]);
        if !below {
            self.below_for = 0.0;
            return false;
        }
        if dt.is_finite() && dt > 0.0 {
            self.below_for += dt;
        }
        if self.below_for <= RECOVER_AFTER {
            return false;
        }
        // The parameters were validated when the character was made and
        // the position is a finite earlier position, so this succeeds.
        if let Ok(back) = Character::new(c.params, c.settings, self.last_grounded) {
            self.character = back;
        }
        self.below_for = 0.0;
        self.recoveries += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_core::Budget;

    fn floor() -> World {
        let b = Budget::new(16 << 20);
        let mut w = World::new();
        let (a, c, d, e) = (
            [-1000.0, -1000.0, 0.0],
            [1000.0, -1000.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [-1000.0, 1000.0, 0.0],
        );
        w.add_triangles(&[[a, c, d], [a, d, e]], "floor", &b)
            .unwrap();
        w
    }
    const SETTINGS: FlySettings = FlySettings {
        speed: 300.,
        fast_speed: 1500.,
        radians_per_pixel: 0.01,
    };

    #[test]
    fn the_eye_settles_at_eye_height_and_walks_where_it_looks() {
        let w = floor();
        let p = Params::PROVISIONAL;
        let start = FlyState::from_eye_look([0., 0., 300.], [1., 0., 300.]).unwrap();
        let mut v = WalkView::from_fly(start, p, Settings::DEFAULT).unwrap();
        assert_eq!(v.look.eye, [0., 0., 300.]);
        for _ in 0..120 {
            v.step(&w, FlyInput::default(), SETTINGS, 1. / 60.);
        }
        assert!(v.character.grounded);
        assert!(
            (v.look.eye[2] - p.eye_height).abs() < 1e-6,
            "{:?}",
            v.look.eye
        );
        let w_key = FlyInput::from_keys([true, false, false, false], false, None);
        for _ in 0..60 {
            v.step(&w, w_key, SETTINGS, 1. / 60.);
        }
        assert!(
            (v.look.eye[0] - p.walk_speed).abs() < 1e-3,
            "{:?}",
            v.look.eye
        );
        // Looking up does not lift the walker; turning right turns the walk.
        let look = FlyInput::from_keys([false; 4], false, Some([0., -50.]));
        v.step(&w, look, SETTINGS, 1. / 60.);
        assert!(v.look.pitch > 0.);
        let turn = FlyInput::from_keys([false; 4], false, Some([157., 0.]));
        v.step(&w, turn, SETTINGS, 1. / 60.);
        let before = v.look.eye;
        for _ in 0..60 {
            v.step(&w, w_key, SETTINGS, 1. / 60.);
        }
        assert!(v.look.eye[1] < before[1] - 90., "{:?}", v.look.eye);
        assert!((v.look.eye[2] - p.eye_height).abs() < 1e-6);
    }

    #[test]
    fn shift_runs_at_the_run_speed() {
        let w = floor();
        let p = Params::PROVISIONAL;
        let start =
            FlyState::from_eye_look([0., 0., p.eye_height], [1., 0., p.eye_height]).unwrap();
        let mut v = WalkView::from_fly(start, p, Settings::DEFAULT).unwrap();
        for _ in 0..10 {
            v.step(&w, FlyInput::default(), SETTINGS, 1. / 60.);
        }
        assert!(v.character.grounded);
        let run = FlyInput::from_keys([true, false, false, false], true, None);
        let x0 = v.look.eye[0];
        for _ in 0..60 {
            v.step(&w, run, SETTINGS, 1. / 60.);
        }
        assert!(
            (v.look.eye[0] - x0 - p.run_speed).abs() < 1e-3,
            "{:?}",
            v.look.eye
        );
    }

    #[test]
    fn a_walker_falling_out_of_the_world_is_put_back_where_it_last_stood() {
        let b = Budget::new(16 << 20);
        let mut w = World::new();
        // A 200 x 200 platform at z 0, nothing below.
        let (a, c, d, e) = (
            [-100.0, -100.0, 0.0],
            [100.0, -100.0, 0.0],
            [100.0, 100.0, 0.0],
            [-100.0, 100.0, 0.0],
        );
        w.add_triangles(&[[a, c, d], [a, d, e]], "platform", &b)
            .unwrap();
        let p = Params::PROVISIONAL;
        let start =
            FlyState::from_eye_look([0., 0., p.eye_height], [1., 0., p.eye_height]).unwrap();
        let mut v = WalkView::from_fly(start, p, Settings::DEFAULT).unwrap();
        let dt = 1. / 60.;
        for _ in 0..10 {
            v.step(&w, FlyInput::default(), SETTINGS, dt);
        }
        assert!(v.character.grounded);
        // Run off the east edge and keep falling.
        let run = FlyInput::from_keys([true, false, false, false], true, None);
        let mut frames = 0;
        while v.recoveries == 0 {
            v.step(&w, run, SETTINGS, dt);
            frames += 1;
            assert!(frames < 600, "never recovered: {:?}", v.character.feet);
        }
        // Back at rest on the platform, at its last grounded position.
        assert_eq!(v.character.feet, v.last_grounded);
        assert!(v.last_grounded[0] > 90.0 && v.last_grounded[0] <= 100.0 + p.radius);
        assert_eq!(v.character.velocity, [0.0; 3]);
        assert_eq!(v.look.eye[2], v.last_grounded[2] + p.eye_height);
        for _ in 0..5 {
            v.step(&w, FlyInput::default(), SETTINGS, dt);
        }
        assert!(v.character.grounded, "{:?}", v.character.feet);
        // Above the lowest collision nothing is counted.
        let mut high = WalkView::from_fly(
            FlyState::from_eye_look([500., 0., 900.], [501., 0., 900.]).unwrap(),
            p,
            Settings::DEFAULT,
        )
        .unwrap();
        for _ in 0..30 {
            high.step(&w, FlyInput::default(), SETTINGS, dt);
        }
        assert_eq!(high.below_for, 0.0);
        assert_eq!(high.recoveries, 0);
    }
}
