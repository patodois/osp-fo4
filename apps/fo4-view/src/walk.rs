//! Walk mode of the windowed viewer: the `fo4-walk` controller on the cell's
//! placed collision, steered by the same keys and mouse as the free-fly
//! camera, with the camera at the controller's eye height. Pure functions
//! of input deltas, like `fly`. Every movement number is a provisional
//! registered rule (`spec/walk.md`), not a game value.
use crate::fly::{FlyInput, FlySettings, FlyState};
use fo4_walk::{Character, Input, Params, Settings, World};

#[derive(Debug, Clone)]
pub struct WalkView {
    pub character: Character,
    /// Orientation of the view; its eye follows the character.
    pub look: FlyState,
}

impl WalkView {
    /// A character whose eye is at the free-fly eye, looking the same way.
    pub fn from_fly(state: FlyState, params: Params, settings: Settings) -> Result<Self, String> {
        let feet = [state.eye[0], state.eye[1], state.eye[2] - params.eye_height];
        let character = Character::new(params, settings, feet).map_err(|e| e.to_string())?;
        let mut view = Self {
            character,
            look: state,
        };
        view.look.eye = view.character.eye();
        Ok(view)
    }

    /// One frame: the mouse turns the view as in fly mode; W/A/S/D walk
    /// along the view's heading on the ground, Shift runs.
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
        self.look = FlyState {
            eye: self.character.eye(),
            ..turned
        };
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
}
