//! Continuous client pixels using the measured reference-photo convention.
use fo4_core::{Error, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Camera {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
    pub scale: f64,
    /// Left, right, top, bottom slopes; positive near and far forward distances.
    pub frustum: [f64; 6],
    /// Left, right, top, bottom normalized viewport bounds.
    pub viewport: [f64; 4],
    pub size: [u32; 2],
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Projected {
    pub pixel: [f64; 2],
    pub forward: f64,
}
impl Camera {
    pub fn validate(&self) -> Result<()> {
        let [l, r, t, b, n, f] = self.frustum;
        let [vl, vr, vt, vb] = self.viewport;
        if self
            .rotation
            .iter()
            .flatten()
            .chain(&self.translation)
            .chain(&self.frustum)
            .chain(&self.viewport)
            .any(|v| !v.is_finite())
            || !self.scale.is_finite()
            || self.scale <= 0.
            || l >= r
            || b >= t
            || n <= 0.
            || f <= n
            || vl < 0.
            || vr > 1.
            || vl >= vr
            || vb < 0.
            || vt > 1.
            || vb >= vt
            || self.size.contains(&0)
        {
            return Err(Error::new(
                "InvalidRenderCamera",
                "camera",
                0,
                "finite measured pose, ordered slopes/planes, viewport in [0,1], positive size",
                format!("{self:?}"),
            ));
        }
        Ok(())
    }
    /// Components are forward, up, right, in that order.
    pub fn coordinates(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            (0..3)
                .map(|j| self.rotation[i][j] * (p[j] - self.translation[j]))
                .sum::<f64>()
                / self.scale
        })
    }
    pub fn project_coordinates(&self, a: [f64; 3]) -> Option<Projected> {
        if a.iter().any(|v| !v.is_finite()) || a[0] <= 0. {
            return None;
        }
        let [l, r, t, b, _, _] = self.frustum;
        let [vl, vr, vt, vb] = self.viewport;
        let u = (a[2] / a[0] - l) / (r - l);
        let v = (a[1] / a[0] - b) / (t - b);
        Some(Projected {
            pixel: [
                f64::from(self.size[0]) * (vl + u * (vr - vl)),
                f64::from(self.size[1]) * (1. - vb - v * (vt - vb)),
            ],
            forward: a[0],
        })
    }
    pub fn project(&self, p: [f64; 3]) -> Result<Option<Projected>> {
        self.validate()?;
        Ok(self.project_coordinates(self.coordinates(p)))
    }
}
/// Frame size of the reference renders (the `render` example and the GPU viewer).
pub const REFERENCE_SIZE: [u32; 2] = [1280, 720];
/// Near and far forward distances of the reference explicit pose, in game units.
/// Viewer choices for comparable frames, not a measured game camera.
pub const REFERENCE_NEAR: f64 = 15.;
pub const REFERENCE_FAR: f64 = 12000.;
/// Right and top slopes of the reference frame: a 40 degree horizontal
/// half-angle, with the vertical slope following the frame's aspect ratio.
/// A viewer choice, not a measured game field of view.
pub fn reference_slopes(size: [u32; 2]) -> (f64, f64) {
    let right = (40f64.to_radians()).tan();
    (right, right * f64::from(size[1]) / f64::from(size[0]))
}
/// Left, right, top, bottom slopes and near, far distances of the reference
/// explicit pose for a frame size.
pub fn reference_frustum(size: [u32; 2]) -> [f64; 6] {
    let (right, top) = reference_slopes(size);
    [-right, right, top, -top, REFERENCE_NEAR, REFERENCE_FAR]
}
/// Z-up camera at `eye` facing `look`: rows are forward, up and right.
pub fn look_at(eye: [f64; 3], look: [f64; 3], frustum: [f64; 6], size: [u32; 2]) -> Result<Camera> {
    let unit = |v: [f64; 3]| -> Result<[f64; 3]> {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n < 1e-9 {
            return Err(Error::new(
                "DegenerateCameraDirection",
                "camera",
                0,
                "eye and look apart, not looking straight up or down",
                format!("{eye:?} -> {look:?}"),
            ));
        }
        Ok(v.map(|x| x / n))
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let forward = unit(std::array::from_fn(|i| look[i] - eye[i]))?;
    let right = unit(cross(forward, [0., 0., 1.]))?;
    let up = cross(right, forward);
    Ok(Camera {
        rotation: [forward, up, right],
        translation: eye,
        scale: 1.,
        frustum,
        viewport: [0., 1., 1., 0.],
        size,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_formula_with_oblique_pose_and_viewport() {
        let c = Camera {
            rotation: [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]],
            translation: [10., 20., 30.],
            scale: 2.,
            frustum: [-1., 1., 1., -1., 2., 200.],
            viewport: [0.1, 0.9, 0.8, 0.2],
            size: [100, 200],
        };
        let p = c.project([14., 28., 32.]).unwrap().unwrap();
        assert!((p.pixel[0] - 70.).abs() < 1e-12);
        assert!((p.pixel[1] - 85.).abs() < 1e-12);
        assert_eq!(p.forward, 4.);
        assert!(c.project([0., 19., 0.]).unwrap().is_none());
        let mut bad = c;
        bad.frustum[1] = bad.frustum[0];
        assert_eq!(bad.validate().unwrap_err().code, "InvalidRenderCamera");
    }
    #[test]
    fn look_at_is_right_handed_z_up_and_centres_the_target() {
        let (right, top) = reference_slopes(REFERENCE_SIZE);
        assert_eq!(top, right * 720. / 1280.);
        let frustum = reference_frustum(REFERENCE_SIZE);
        assert_eq!(frustum, [-right, right, top, -top, 15., 12000.]);
        let c = look_at([1., 2., 3.], [11., 2., 3.], frustum, REFERENCE_SIZE).unwrap();
        // Facing +X with +Z up, the right-hand side is -Y.
        assert_eq!(c.rotation, [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]]);
        let centre = c.project([11., 2., 3.]).unwrap().unwrap();
        assert_eq!(centre.pixel, [640., 360.]);
        let to_the_right = c.project([11., 1., 3.]).unwrap().unwrap();
        assert!(to_the_right.pixel[0] > 640.);
        let above = c.project([11., 2., 4.]).unwrap().unwrap();
        assert!(above.pixel[1] < 360.);
        let straight_up = look_at([0.; 3], [0., 0., 1.], frustum, REFERENCE_SIZE);
        assert_eq!(straight_up.unwrap_err().code, "DegenerateCameraDirection");
    }
}
