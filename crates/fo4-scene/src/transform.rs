//! NIF node rotation words read as rows acting on column vectors, placement
//! Euler angles, and parent/local composition.
use fo4_core::{Error, Result};
use fo4_nif::{Field, Value};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Transform {
    pub rotation: [[f64; 3]; 3],
    pub translation: [f64; 3],
    pub scale: f64,
}
fn multiply(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
impl Transform {
    pub const IDENTITY: Self = Self {
        rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        translation: [0.; 3],
        scale: 1.,
    };
    pub fn apply(self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            self.translation[i]
                + self.scale * (0..3).map(|j| self.rotation[j][i] * p[j]).sum::<f64>()
        })
    }
    /// Parent.compose(local): Rw=Rl*Rp; tw=tp+sp*Rp^T*tl; sw=sp*sl.
    pub fn compose(self, local: Self) -> Self {
        Self {
            rotation: multiply(local.rotation, self.rotation),
            translation: self.apply(local.translation),
            scale: self.scale * local.scale,
        }
    }
    /// Whether the transform moves a point by more than rounding: a
    /// rotation word more than 1e-6 from the identity's, a translation word
    /// above 1e-6 or a scale more than 1e-6 from 1.
    pub fn is_identity(&self) -> bool {
        (0..3).all(|i| {
            (0..3).all(|j| (self.rotation[i][j] - f64::from(u8::from(i == j))).abs() <= 1e-6)
        }) && self.translation.iter().all(|t| t.abs() <= 1e-6)
            && (self.scale - 1.).abs() <= 1e-6
    }
    /// Supported ZYX convention, evaluated approximately with binary32 sin/cos.
    /// Exact runtime arithmetic remains open; see spec/rotation-arithmetic.md.
    pub fn euler(placement: [f32; 6], scale: f32) -> Result<Self> {
        if placement.iter().any(|v| !v.is_finite()) || !scale.is_finite() || scale <= 0. {
            return Err(Error::new(
                "InvalidPlacementTransform",
                "placement",
                0,
                "finite angles/position and positive scale",
                format!("{placement:?}, {scale}"),
            ));
        }
        Ok(Self {
            rotation: euler_zyx([placement[3], placement[4], placement[5]], false)
                .map(|row| row.map(f64::from)),
            translation: [placement[0], placement[1], placement[2]].map(f64::from),
            scale: f64::from(scale),
        })
    }
    pub fn from_fields(fields: &[Field], file: &str) -> Result<Self> {
        fn floats<const N: usize>(fields: &[Field], name: &str, file: &str) -> Result<[f64; N]> {
            let mut out = [0.; N];
            let mut count = 0;
            for f in fields.iter().filter(|f| f.name == name) {
                let Value::FloatBits(bits) = f.value else {
                    return Err(Error::new(
                        "BoundsTransformType",
                        file,
                        f.offset,
                        "float bits",
                        name,
                    ));
                };
                if count >= N {
                    return Err(Error::new(
                        "BoundsTransformCount",
                        file,
                        f.offset,
                        N,
                        count + 1,
                    ));
                }
                let v = f32::from_bits(bits);
                if !v.is_finite() {
                    return Err(Error::new(
                        "NonFiniteBounds",
                        file,
                        f.offset,
                        "finite transform",
                        bits,
                    ));
                }
                out[count] = f64::from(v);
                count += 1;
            }
            if count != N {
                return Err(Error::new("BoundsTransformCount", file, 0, N, count));
            }
            Ok(out)
        }
        let translation = floats::<3>(fields, "Translation", file)?;
        let raw = floats::<9>(fields, "Rotation", file)?;
        let [scale] = floats::<1>(fields, "Scale", file)?;
        // The nine words are the rows of a matrix R acting on column vectors
        // (a node maps a child point p to t + s * R * p), measured offline in
        // spec/scene.md (Node rotation words) and spec/actor.md; `rotation`
        // holds R transposed because `apply` multiplies by its transpose.
        Ok(Self {
            translation,
            rotation: std::array::from_fn(|i| std::array::from_fn(|j| raw[j * 3 + i])),
            scale,
        })
    }
}
/// Alternative association for restricted fixture replay, not a universal arithmetic proof.
pub fn euler_zyx([x, y, z]: [f32; 3], right_associated: bool) -> [[f32; 3]; 3] {
    fn mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    }
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]];
    let ry = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
    let rz = [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]];
    if right_associated {
        mul(rz, mul(ry, rx))
    } else {
        mul(mul(rz, ry), rx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_composition_including_noncommuting_rotations() {
        let p = Transform {
            rotation: [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]],
            translation: [10., 20., 30.],
            scale: 2.,
        };
        let l = Transform {
            rotation: [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]],
            translation: [3., 4., 5.],
            scale: 3.,
        };
        let w = p.compose(l);
        assert_eq!(w.translation, [2., 26., 40.]);
        assert_eq!(w.scale, 6.);
        assert_eq!(w.rotation, [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]]);
        assert_eq!(w.apply([1., 2., 3.]), p.apply(l.apply([1., 2., 3.])));
    }
    #[test]
    fn node_words_match_the_actor_reading_under_nested_rotations() {
        fn fields(t: [f32; 3], r: [f32; 9], s: f32) -> Vec<Field> {
            let mut out = Vec::new();
            for (name, values) in [
                ("Translation", t.to_vec()),
                ("Rotation", r.to_vec()),
                ("Scale", vec![s]),
            ] {
                out.extend(values.into_iter().map(|v| Field {
                    name,
                    offset: 0,
                    value: Value::FloatBits(v.to_bits()),
                }));
            }
            out
        }
        // A quarter turn about z under a quarter turn about x, scaled.
        let a = fields([1., 2., 3.], [1., 0., 0., 0., 0., -1., 0., 1., 0.], 2.);
        let b = fields([4., 5., 6.], [0., -1., 0., 1., 0., 0., 0., 0., 1.], 0.5);
        let ours = Transform::from_fields(&a, "synthetic")
            .unwrap()
            .compose(Transform::from_fields(&b, "synthetic").unwrap());
        let pose = crate::actor::pose::Affine::from_fields(&a, "synthetic")
            .unwrap()
            .unwrap()
            .compose(
                &crate::actor::pose::Affine::from_fields(&b, "synthetic")
                    .unwrap()
                    .unwrap(),
            );
        for p in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [7., -3., 2.]] {
            let (x, y) = (ours.apply(p), pose.apply(p));
            assert!((0..3).all(|i| (x[i] - y[i]).abs() < 1e-12), "{x:?} {y:?}");
        }
        // Rows act on column vectors: the child's x axis goes to the parent's y.
        let one = Transform::from_fields(&b, "synthetic").unwrap();
        assert_eq!(one.apply([1., 0., 0.]), [4., 5.5, 6.]);
    }
    #[test]
    fn euler_is_explicit_hypothesis_and_rejects_nonfinite() {
        let t = Transform::euler([0., 0., 0., 0., 0., std::f32::consts::FRAC_PI_2], 1.).unwrap();
        let p = t.apply([1., 0., 0.]);
        assert!(p[0].abs() < 1e-7);
        assert!((p[1] + 1.).abs() < 1e-7);
        assert!(Transform::euler([f32::NAN; 6], 1.).is_err());
    }
}

#[cfg(test)]
mod zyx_tests {
    use super::*;
    #[test]
    fn unequal_negative_angles_follow_independent_zyx_coefficients() {
        let got = Transform::euler([3., -5., 7., 0.31, -1.07, 2.41], 0.5).unwrap();
        let expected = [
            [
                -0.35726589562000816,
                -0.43708936029725454,
                0.8254174525303969,
            ],
            [
                0.32074967673729254,
                -0.8874122995941508,
                -0.3310876853674014,
            ],
            [0.8772005042746817, 0.14646604263090418, 0.45723842101936973],
        ];
        for (a, b) in got.rotation.iter().flatten().zip(expected.iter().flatten()) {
            assert!((a - b).abs() < 3e-7);
        }
        assert_eq!(got.translation, [3., -5., 7.]);
        assert_eq!(got.scale, 0.5);
    }
}
