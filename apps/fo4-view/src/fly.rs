//! Free-fly camera of the windowed mode, as pure functions of input deltas.
//! All positions and directions are in game coordinates (right-handed, Z up).
//! Speeds and sensitivity are viewer settings, not game values.

/// Pitch stays inside +-MAX_PITCH so the view never looks straight up or
/// down, where `fo4_scene::camera::look_at` has no right-hand direction.
pub const MAX_PITCH: f64 = 89. * std::f64::consts::PI / 180.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlySettings {
    /// Game units per second.
    pub speed: f64,
    /// Game units per second while the fast key is held.
    pub fast_speed: f64,
    /// Radians of turn per pixel of mouse motion.
    pub radians_per_pixel: f64,
}

/// One frame of input. `forward` and `right` are -1, 0 or 1 (W/S, D/A);
/// `look` is the mouse motion in window pixels (x right, y down), already
/// zero when looking is not engaged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyInput {
    pub forward: f64,
    pub right: f64,
    pub look: [f64; 2],
    pub fast: bool,
}

impl FlyInput {
    /// Keys held this frame and, while looking is engaged, the mouse motion.
    pub fn from_keys(
        [forward, back, left, right]: [bool; 4],
        fast: bool,
        look: Option<[f64; 2]>,
    ) -> Self {
        let axis = |plus: bool, minus: bool| f64::from(u8::from(plus)) - f64::from(u8::from(minus));
        Self {
            forward: axis(forward, back),
            right: axis(right, left),
            look: look.unwrap_or([0.; 2]),
            fast,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyState {
    pub eye: [f64; 3],
    /// Radians from +X towards +Y (counter-clockwise seen from above).
    pub yaw: f64,
    /// Radians above the horizontal plane.
    pub pitch: f64,
}

impl FlyState {
    pub fn from_eye_look(eye: [f64; 3], look: [f64; 3]) -> Result<Self, String> {
        let d: [f64; 3] = std::array::from_fn(|i| look[i] - eye[i]);
        let horizontal = d[0].hypot(d[1]);
        if horizontal < 1e-9 {
            return Err(
                "DegenerateCameraDirection: the free-fly start must not look straight up or down"
                    .into(),
            );
        }
        Ok(Self {
            eye,
            yaw: d[1].atan2(d[0]),
            pitch: d[2].atan2(horizontal).clamp(-MAX_PITCH, MAX_PITCH),
        })
    }
    pub fn forward(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [cp * cy, cp * sy, sp]
    }
    /// Horizontal right-hand direction, `forward x +Z` normalised.
    pub fn right(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        [sy, -cy, 0.]
    }
    /// A point one unit ahead, for `look_at`.
    pub fn look_point(&self) -> [f64; 3] {
        let f = self.forward();
        std::array::from_fn(|i| self.eye[i] + f[i])
    }
    /// Advance by one frame of `dt` seconds. Mouse right turns right, mouse
    /// up looks up; W moves along the view direction, D to the right.
    pub fn step(self, input: FlyInput, settings: FlySettings, dt: f64) -> Self {
        let yaw = (self.yaw - input.look[0] * settings.radians_per_pixel)
            .rem_euclid(std::f64::consts::TAU);
        let pitch =
            (self.pitch - input.look[1] * settings.radians_per_pixel).clamp(-MAX_PITCH, MAX_PITCH);
        let turned = Self {
            eye: self.eye,
            yaw,
            pitch,
        };
        let f = turned.forward();
        let r = turned.right();
        let speed = if input.fast {
            settings.fast_speed
        } else {
            settings.speed
        };
        let mut v: [f64; 3] = std::array::from_fn(|i| f[i] * input.forward + r[i] * input.right);
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n > 0. {
            v = v.map(|x| x / n * speed * dt);
        }
        Self {
            eye: std::array::from_fn(|i| self.eye[i] + v[i]),
            ..turned
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SETTINGS: FlySettings = FlySettings {
        speed: 100.,
        fast_speed: 400.,
        radians_per_pixel: 0.01,
    };
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn keys_map_to_axes() {
        let i = FlyInput::from_keys([true, false, true, false], true, None);
        assert_eq!(
            (i.forward, i.right, i.look, i.fast),
            (1., -1., [0., 0.], true)
        );
        let i = FlyInput::from_keys([true, true, false, true], false, Some([3., -4.]));
        assert_eq!((i.forward, i.right, i.look), (0., 1., [3., -4.]));
    }

    #[test]
    fn start_pose_faces_the_look_point() {
        let s = FlyState::from_eye_look([1., 2., 3.], [1., 12., 3.]).unwrap();
        assert!(close(s.forward(), [0., 1., 0.]));
        // Facing +Y with +Z up, the right-hand side is +X (right-handed).
        assert!(close(s.right(), [1., 0., 0.]));
        let up = FlyState::from_eye_look([0.; 3], [0., 0., 5.]);
        assert!(up.is_err());
    }

    #[test]
    fn keys_move_at_the_set_speed_and_shift_is_faster() {
        let s = FlyState::from_eye_look([0.; 3], [1., 0., 0.]).unwrap();
        let w = FlyInput {
            forward: 1.,
            ..Default::default()
        };
        assert!(close(s.step(w, SETTINGS, 0.5).eye, [50., 0., 0.]));
        let fast = FlyInput { fast: true, ..w };
        assert!(close(s.step(fast, SETTINGS, 0.5).eye, [200., 0., 0.]));
        let d = FlyInput {
            right: 1.,
            ..Default::default()
        };
        // Facing +X, right is -Y.
        assert!(close(s.step(d, SETTINGS, 1.).eye, [0., -100., 0.]));
        // Diagonal movement is not faster than straight movement.
        let wd = FlyInput {
            forward: 1.,
            right: 1.,
            ..Default::default()
        };
        let moved = s.step(wd, SETTINGS, 1.).eye;
        assert!((moved.iter().map(|x| x * x).sum::<f64>().sqrt() - 100.).abs() < 1e-9);
        assert_eq!(s.step(FlyInput::default(), SETTINGS, 1.), s);
    }

    #[test]
    fn mouse_right_turns_right_and_mouse_up_looks_up() {
        let s = FlyState::from_eye_look([0.; 3], [1., 0., 0.]).unwrap();
        let right = s.step(
            FlyInput {
                look: [10., 0.],
                ..Default::default()
            },
            SETTINGS,
            0.016,
        );
        // Turning right from +X swings the view towards -Y.
        assert!(right.forward()[1] < 0.);
        let up = s.step(
            FlyInput {
                look: [0., -10.],
                ..Default::default()
            },
            SETTINGS,
            0.016,
        );
        assert!(up.forward()[2] > 0.);
        let over = s.step(
            FlyInput {
                look: [0., -1e6],
                ..Default::default()
            },
            SETTINGS,
            0.016,
        );
        assert_eq!(over.pitch, MAX_PITCH);
        // The clamped pose is still a valid look_at pose.
        let frustum = [-1., 1., 0.5, -0.5, 15., 12000.];
        assert!(fo4_scene::camera::look_at(over.eye, over.look_point(), frustum, [64, 32]).is_ok());
    }
}
