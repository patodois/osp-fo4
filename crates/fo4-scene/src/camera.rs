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
}
