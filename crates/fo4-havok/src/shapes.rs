//! Typed views of the simple shape classes: `hknpSphereShape`,
//! `hknpCapsuleShape`, `hknpConvexShape`, `hknpScaledConvexShape` and the
//! instances of `hknpStaticCompoundShape` and `hknpDynamicCompoundShape`.
//!
//! Every rule enforced here held on every stored object of its class in the
//! measured installation; `spec/havok-bodies.md` gives the counts. Offsets
//! are from the object's offset. Field names are this project's labels:
//! "radius", "scale", "rotation" and "translation" name stored values whose
//! role at runtime is a hypothesis.
use crate::view::{Array, View};
use crate::{Container, VERTEX_TAG};
use fo4_core::{Budget, Result};
use serde::Serialize;

pub const SPHERE: &str = "hknpSphereShape";
pub const CAPSULE: &str = "hknpCapsuleShape";
pub const CONVEX: &str = "hknpConvexShape";
pub const SCALED_CONVEX: &str = "hknpScaledConvexShape";
pub const STATIC_COMPOUND: &str = "hknpStaticCompoundShape";
pub const DYNAMIC_COMPOUND: &str = "hknpDynamicCompoundShape";
pub const SPHERE_SPAN: usize = 128;
pub const CAPSULE_SPAN: usize = 432;
pub const SCALED_CONVEX_SPAN: usize = 112;
/// Offset of a compound's instance storage and size of one instance.
pub const COMPOUND_FIXED: usize = 208;
pub const INSTANCE: usize = 128;
/// Largest stored deviation of an instance rotation from orthonormal was 6.7e-6.
pub const ORTHONORMAL_TOLERANCE: f64 = 1e-4;
/// The (count, offset from the pair) u16 pairs at 48 to 76 of every capsule.
pub const CAPSULE_PAIRS: [(u16, u16); 4] = [(8, 64), (8, 176), (6, 300), (24, 328)];
const RADIUS_AT: usize = 20;
const PAIR_AT: usize = 48;
const ROW: usize = 16;

fn tag(v: &View<'_, '_>, at: usize, expected: u32) -> Result<()> {
    let t = v.u32(at)?;
    if t != expected {
        return Err(v.err(at, format!("row tag {expected:08x}"), format!("{t:08x}")));
    }
    Ok(())
}

fn pair(v: &View<'_, '_>, at: usize, expected: (u16, u16)) -> Result<()> {
    let found = (v.u16(at)?, v.u16(at + 2)?);
    if found != expected {
        return Err(v.err(
            at,
            format!("count and offset {expected:?}"),
            format!("{found:?}"),
        ));
    }
    Ok(())
}

/// `hknpSphereShape`: four equal rows at 64 tagged `0x3f000000` and the f32
/// at 20.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Sphere {
    pub object: usize,
    /// The first three f32 of the rows at 64.
    pub center: [f32; 3],
    /// f32 at byte 20, finite and not negative.
    pub radius: f32,
}
impl Sphere {
    pub fn read(c: &Container<'_>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(c, index, file, base, &[SPHERE], "InvalidHavokSphere")?;
        v.span_len(SPHERE_SPAN)?;
        let [radius] = v.finite::<1>(RADIUS_AT, "radius")?;
        if radius < 0.0 {
            return Err(v.err(RADIUS_AT, "radius >= 0", radius));
        }
        pair(&v, PAIR_AT, (4, 16))?;
        let center = v.finite::<3>(64, "center")?;
        tag(&v, 76, VERTEX_TAG)?;
        let row = v.bytes.get(64..80);
        for j in 1..4 {
            let at = 64 + ROW * j;
            let copy = v.bytes.get(at..at + ROW);
            if copy != row {
                return Err(v.err(
                    at,
                    "a copy of the row at 64",
                    copy.map(crate::hex).unwrap_or_default(),
                ));
            }
        }
        Ok(Self {
            object: index,
            center,
            radius,
        })
    }
}

/// `hknpCapsuleShape`: two points at 80 and 96 (fourth f32 1.0), the f32 at
/// 20, and eight tagged vertex rows at 112 that enclose both points and lie
/// within the radius of the segment between them. Planes, faces and indices
/// follow at the fixed positions of [`CAPSULE_PAIRS`] and are not read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Capsule {
    pub object: usize,
    pub points: [[f32; 3]; 2],
    /// f32 at byte 20, finite and positive.
    pub radius: f32,
    pub vertices: [[f32; 3]; 8],
}
impl Capsule {
    pub fn read(c: &Container<'_>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(c, index, file, base, &[CAPSULE], "InvalidHavokCapsule")?;
        v.span_len(CAPSULE_SPAN)?;
        let [radius] = v.finite::<1>(RADIUS_AT, "radius")?;
        if radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Err(v.err(RADIUS_AT, "radius > 0", radius));
        }
        for (at, p) in [48, 64, 68, 72].into_iter().zip(CAPSULE_PAIRS) {
            pair(&v, at, p)?;
        }
        let mut points = [[0f32; 3]; 2];
        for (k, p) in points.iter_mut().enumerate() {
            let at = 80 + ROW * k;
            let [x, y, z, w] = v.finite::<4>(at, "point")?;
            if w != 1.0 {
                return Err(v.err(at + 12, "1.0", w));
            }
            *p = [x, y, z];
        }
        if points[0] == points[1] {
            return Err(v.err(96, "a second point distinct from the first", "equal"));
        }
        let mut vertices = [[0f32; 3]; 8];
        for (j, p) in vertices.iter_mut().enumerate() {
            let at = 112 + ROW * j;
            *p = v.finite::<3>(at, "vertex")?;
            tag(&v, at + 12, VERTEX_TAG | j as u32)?;
        }
        for k in 0..3 {
            let lo = vertices.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min);
            let hi = vertices
                .iter()
                .map(|p| p[k])
                .fold(f32::NEG_INFINITY, f32::max);
            for (j, p) in points.iter().enumerate() {
                if p[k] < lo || p[k] > hi {
                    return Err(v.err(
                        80 + ROW * j + 4 * k,
                        format!("a point inside the vertex box [{lo}, {hi}]"),
                        p[k],
                    ));
                }
            }
        }
        for (j, p) in vertices.iter().enumerate() {
            let d = segment_distance(*p, points[0], points[1]);
            if d > f64::from(radius) {
                return Err(v.err(
                    112 + ROW * j,
                    format!("a vertex within the radius {radius} of the segment"),
                    d,
                ));
            }
        }
        Ok(Self {
            object: index,
            points,
            radius,
            vertices,
        })
    }
}

/// Distance in f64 from `p` to the segment from `a` to `b`.
pub fn segment_distance(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f64 {
    let f = |v: [f32; 3]| v.map(f64::from);
    let (p, a, b) = (f(p), f(a), f(b));
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    let len = ab.iter().map(|x| x * x).sum::<f64>();
    let t = if len > 0.0 {
        (ab.iter().zip(&ap).map(|(x, y)| x * y).sum::<f64>() / len).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..3)
        .map(|k| (ap[k] - t * ab[k]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// `hknpConvexShape`: the vertex pair at 48 points to 64 and every row is
/// tagged `0x3f000000` OR its index.
#[derive(Debug, Clone, Serialize)]
pub struct ConvexShape<'a> {
    pub object: usize,
    pub vertex_count: usize,
    #[serde(skip)]
    bytes: &'a [u8],
}
impl<'a> ConvexShape<'a> {
    pub fn read(c: &Container<'a>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(c, index, file, base, &[CONVEX], "InvalidHavokConvexShape")?;
        let rows = v.u16(PAIR_AT)? as usize;
        let rel = v.u16(PAIR_AT + 2)?;
        if rel != 16 {
            return Err(v.err(PAIR_AT + 2, "vertex rows at 64 (offset 16)", rel));
        }
        if rows == 0 || rows > crate::MAX_VERTEX_ROWS {
            return Err(v.err(
                PAIR_AT,
                format!("1 to {} vertex rows", crate::MAX_VERTEX_ROWS),
                rows,
            ));
        }
        v.span_len(64 + ROW * rows)?;
        for j in 0..rows {
            let at = 64 + ROW * j;
            v.finite::<3>(at, "vertex")?;
            tag(&v, at + 12, VERTEX_TAG | j as u32)?;
        }
        Ok(Self {
            object: index,
            vertex_count: rows,
            bytes: v.bytes,
        })
    }
    pub fn vertex(&self, i: usize) -> Option<[f32; 3]> {
        if i >= self.vertex_count {
            return None;
        }
        let at = 64 + ROW * i;
        let f = |k: usize| {
            let w = self.bytes.get(at + 4 * k..at + 4 * k + 4)?;
            Some(f32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        };
        Some([f(0)?, f(1)?, f(2)?])
    }
    pub fn vertices(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        (0..self.vertex_count).filter_map(|i| self.vertex(i))
    }
}

/// `hknpScaledConvexShape`: a link at 48 to another shape, three positive f32
/// at 64 and three f32 at 80.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScaledConvex {
    pub object: usize,
    /// Third-table index of the linked object.
    pub child: usize,
    pub scale: [f32; 3],
    pub translation: [f32; 3],
}
impl ScaledConvex {
    pub fn read(c: &Container<'_>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[SCALED_CONVEX],
            "InvalidHavokScaledConvex",
        )?;
        v.span_len(SCALED_CONVEX_SPAN)?;
        let child = v.required_link(48, "child shape")?;
        let scale = v.finite::<3>(64, "scale")?;
        if let Some(k) = scale.iter().position(|s| *s <= 0.0) {
            return Err(v.err(64 + 4 * k, "scale > 0", scale[k]));
        }
        let translation = v.finite::<3>(80, "translation")?;
        Ok(Self {
            object: index,
            child,
            scale,
            translation,
        })
    }
}

/// One 128-byte instance of a compound shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Instance {
    /// Third-table index of the object linked from instance byte 80.
    pub child: usize,
    /// Rows of three f32 at instance bytes 0, 16 and 32: orthonormal within
    /// [`ORTHONORMAL_TOLERANCE`], determinant positive.
    pub rotation: [[f32; 3]; 3],
    /// Three f32 at instance byte 48.
    pub translation: [f32; 3],
    /// Three positive f32 at instance byte 64.
    pub scale: [f32; 3],
}

/// `hknpStaticCompoundShape` or `hknpDynamicCompoundShape`: the array at 96
/// with instances of 128 bytes stored from byte 208 to the end of the span.
#[derive(Debug, Clone, Serialize)]
pub struct Compound<'a> {
    pub object: usize,
    pub class: &'a str,
    pub array: Array,
    pub instances: Vec<Instance>,
}
impl<'a> Compound<'a> {
    pub fn read(
        c: &Container<'a>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[STATIC_COMPOUND, DYNAMIC_COMPOUND],
            "InvalidHavokCompound",
        )?;
        let array = v.array(96, INSTANCE as u32)?;
        let n = array.count as usize;
        if n == 0 {
            return Err(v.err(104, "at least one instance", 0));
        }
        if array.storage as usize != COMPOUND_FIXED {
            return Err(v.err(
                96,
                format!("instances stored from byte {COMPOUND_FIXED}"),
                array.storage,
            ));
        }
        let expected = n
            .checked_mul(INSTANCE)
            .and_then(|x| x.checked_add(COMPOUND_FIXED));
        if expected != Some(v.bytes.len()) {
            return Err(v.err(
                104,
                format!("{n} instances ending the {}-byte span", v.bytes.len()),
                n,
            ));
        }
        let mut instances = Vec::new();
        b.reserve(&mut instances, n, file, v.abs(104))?;
        for i in 0..n {
            let e = COMPOUND_FIXED + INSTANCE * i;
            let mut rotation = [[0f32; 3]; 3];
            for (r, row) in rotation.iter_mut().enumerate() {
                *row = v.finite::<3>(e + ROW * r, "rotation")?;
            }
            let error = orthonormal_error(&rotation);
            if error > ORTHONORMAL_TOLERANCE {
                return Err(v.err(
                    e,
                    format!("orthonormal rows within {ORTHONORMAL_TOLERANCE}"),
                    error,
                ));
            }
            if determinant(&rotation) <= 0.0 {
                return Err(v.err(e, "rows with a positive determinant", "a reflection"));
            }
            let translation = v.finite::<3>(e + 48, "translation")?;
            let scale = v.finite::<3>(e + 64, "scale")?;
            if let Some(k) = scale.iter().position(|s| *s <= 0.0) {
                return Err(v.err(e + 64 + 4 * k, "scale > 0", scale[k]));
            }
            let child = v.required_link(e + 80, "instance shape")?;
            instances.push(Instance {
                child,
                rotation,
                translation,
                scale,
            });
        }
        Ok(Self {
            object: index,
            class: v.span.class,
            array,
            instances,
        })
    }
}

/// Largest |r_i · r_j − δ_ij| over the rows, in f64.
pub fn orthonormal_error(r: &[[f32; 3]; 3]) -> f64 {
    let mut e = 0f64;
    for i in 0..3 {
        for j in 0..3 {
            let dot: f64 = (0..3)
                .map(|k| f64::from(r[i][k]) * f64::from(r[j][k]))
                .sum();
            e = e.max((dot - if i == j { 1.0 } else { 0.0 }).abs());
        }
    }
    e
}

fn determinant(r: &[[f32; 3]; 3]) -> f64 {
    let m = r.map(|row| row.map(f64::from));
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

#[cfg(test)]
mod tests;
