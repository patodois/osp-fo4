//! A rigid motion of a piece of the walking world: a turn about a pivot
//! followed by a translation of the pivot
//! (`walk_platform_rigid_motion_v1`, [`crate::platform::rules::RIGID_MOTION`];
//! `spec/walk.md`, Platforms that turn). A point `p` of the piece goes to
//! `pivot + translation + R (p - pivot)`, `R` the rotation of a unit
//! quaternion acting on column vectors. Textbook geometry written for this
//! crate: which motion a piece makes is the caller's (a sequence's pose of
//! the nodes above its collision, `fo4_scene::sequence::chain_pose`).
use crate::geometry::{self as g, V3};
use fo4_scene::transform::Transform;
use serde::Serialize;

/// A turn about `pivot` by `rotation`, then `translation`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Motion {
    /// Unit quaternion (w, x, y, z) of the turn.
    pub rotation: [f64; 4],
    /// The point the turn is about, world units.
    pub pivot: V3,
    /// How far the pivot moves.
    pub translation: V3,
}

/// Largest deviation of a placement's rotation words from a rotation (rows
/// orthonormal) accepted by [`Motion::between`]; the stored words are
/// binary32.
pub const ORTHONORMAL_TOLERANCE: f64 = 1e-4;

impl Motion {
    pub const IDENTITY: Self = Self {
        rotation: [1., 0., 0., 0.],
        pivot: [0.; 3],
        translation: [0.; 3],
    };

    /// A translation by `d` alone.
    pub fn translation(d: V3) -> Self {
        Self {
            translation: d,
            ..Self::IDENTITY
        }
    }

    /// The rotation matrix, rows acting on column vectors.
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        let [w, x, y, z] = self.rotation;
        [
            [
                1. - 2. * (y * y + z * z),
                2. * (x * y - w * z),
                2. * (x * z + w * y),
            ],
            [
                2. * (x * y + w * z),
                1. - 2. * (x * x + z * z),
                2. * (y * z - w * x),
            ],
            [
                2. * (x * z - w * y),
                2. * (y * z + w * x),
                1. - 2. * (x * x + y * y),
            ],
        ]
    }

    /// `R v`.
    pub fn rotate(&self, v: V3) -> V3 {
        let m = self.matrix();
        std::array::from_fn(|i| g::dot(m[i], v))
    }

    /// `R^T v`.
    pub fn rotate_back(&self, v: V3) -> V3 {
        let m = self.matrix();
        std::array::from_fn(|i| (0..3).map(|k| m[k][i] * v[k]).sum())
    }

    /// Where the point `p` of the piece goes.
    pub fn apply(&self, p: V3) -> V3 {
        g::add(
            g::add(self.pivot, self.translation),
            self.rotate(g::sub(p, self.pivot)),
        )
    }

    /// The point of the piece that goes to `p` (the inverse of
    /// [`Motion::apply`]).
    pub fn unapply(&self, p: V3) -> V3 {
        g::add(
            self.pivot,
            self.rotate_back(g::sub(g::sub(p, self.pivot), self.translation)),
        )
    }

    /// The turn's angle in degrees, 0 to 180.
    pub fn angle_degrees(&self) -> f64 {
        let w = self.rotation[0].abs().min(1.);
        (2. * w.acos()).to_degrees()
    }

    /// The turn's unit axis (the right-hand sense of a positive angle);
    /// `None` for no turn.
    pub fn axis(&self) -> Option<V3> {
        let [w, x, y, z] = self.rotation;
        let s = if w < 0. { -1. } else { 1. };
        g::unit([s * x, s * y, s * z]).filter(|_| self.angle_degrees() > 1e-9)
    }

    /// The motion that takes what `stored` places to where `posed` places
    /// it: both map the same local coordinates into the world (a chain's
    /// stored and posed transforms under one placement). The pivot is
    /// `stored`'s origin. `None` when the two scales differ (by more than
    /// one part in a million), a rotation is not one (rows not orthonormal
    /// within [`ORTHONORMAL_TOLERANCE`]) or a word is not finite.
    pub fn between(stored: &Transform, posed: &Transform) -> Option<Self> {
        // `Transform::apply` multiplies by the transpose of its words:
        // its linear map is M[i][j] = rotation[j][i].
        let lin = |t: &Transform| -> [[f64; 3]; 3] {
            std::array::from_fn(|i| std::array::from_fn(|j| t.rotation[j][i]))
        };
        let (ms, mp) = (lin(stored), lin(posed));
        let finite = |t: &Transform| {
            t.scale.is_finite()
                && t.scale > 0.
                && g::finite(t.translation)
                && t.rotation.iter().all(|r| g::finite(*r))
        };
        if !finite(stored) || !finite(posed) {
            return None;
        }
        if (stored.scale - posed.scale).abs() > 1e-6 * stored.scale {
            return None;
        }
        let orthonormal = |m: &[[f64; 3]; 3]| {
            (0..3).all(|i| {
                (0..3).all(|j| {
                    let d = g::dot(m[i], m[j]) - f64::from(u8::from(i == j));
                    d.abs() <= ORTHONORMAL_TOLERANCE
                })
            })
        };
        if !orthonormal(&ms) || !orthonormal(&mp) {
            return None;
        }
        // R = Mp Ms^T.
        let r: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| (0..3).map(|k| mp[i][k] * ms[j][k]).sum())
        });
        Some(Self {
            rotation: quaternion_of(&r)?,
            pivot: stored.translation,
            translation: g::sub(posed.translation, stored.translation),
        })
    }

    /// The motion a fraction `k` (0 to 1) of the way from `a` to `b`: the
    /// turn by spherical interpolation along the shorter arc, pivot and
    /// translation straight.
    pub fn lerp(a: &Self, b: &Self, k: f64) -> Self {
        let line = |p: V3, q: V3| -> V3 { std::array::from_fn(|i| p[i] + k * (q[i] - p[i])) };
        Self {
            rotation: slerp(a.rotation, b.rotation, k),
            pivot: line(a.pivot, b.pivot),
            translation: line(a.translation, b.translation),
        }
    }

    /// Whether the motion moves no point by more than rounding: a turn
    /// below 1e-9 degrees and a translation below 1e-9 units.
    pub fn is_identity(&self) -> bool {
        self.angle_degrees() <= 1e-9 && self.translation.iter().all(|t| t.abs() <= 1e-9)
    }
}

/// The unit quaternion (w, x, y, z) of a rotation matrix (rows acting on
/// column vectors), with w >= 0; `None` when it is not finite.
fn quaternion_of(m: &[[f64; 3]; 3]) -> Option<[f64; 4]> {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let q = if tr > 0. {
        let s = (tr + 1.).sqrt() * 2.;
        [
            0.25 * s,
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1. + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.;
        [
            (m[2][1] - m[1][2]) / s,
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = (1. + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.;
        [
            (m[0][2] - m[2][0]) / s,
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
        ]
    } else {
        let s = (1. + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.;
        [
            (m[1][0] - m[0][1]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
        ]
    };
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if !(n.is_finite() && n > 0.) {
        return None;
    }
    let sign = if q[0] < 0. { -1. } else { 1. };
    Some(q.map(|v| sign * v / n))
}

/// Spherical interpolation of two unit quaternions along the shorter arc.
fn slerp(a: [f64; 4], b: [f64; 4], k: f64) -> [f64; 4] {
    let mut d: f64 = (0..4).map(|i| a[i] * b[i]).sum();
    let b = if d < 0. {
        d = -d;
        b.map(|v| -v)
    } else {
        b
    };
    let (p, q) = if d > 1. - 1e-12 {
        (1. - k, k)
    } else {
        let theta = d.min(1.).acos();
        let s = theta.sin();
        (((1. - k) * theta).sin() / s, (k * theta).sin() / s)
    };
    let r: [f64; 4] = std::array::from_fn(|i| p * a[i] + q * b[i]);
    let n = r.iter().map(|v| v * v).sum::<f64>().sqrt();
    if n > 0. && n.is_finite() {
        r.map(|v| v / n)
    } else {
        a
    }
}
