//! Vector helpers and the closest points between two convex point sets.
//!
//! Every shape the controller meets is the convex hull of a few points,
//! widened by a radius: a triangle (three points, radius 0), a sphere (one
//! point), a capsule (two points), a convex piece (its vertices and radius)
//! and the character's own capsule (two points). The distance between two
//! such hulls is found with the general GJK iteration on their Minkowski
//! difference; the closest point of each candidate simplex is found by
//! trying every face of it. This is textbook geometry written for this
//! crate, not a reproduction of any engine.

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn finite(a: V3) -> bool {
    a.iter().all(|x| x.is_finite())
}
/// `a / |a|`, or `None` when the length is not a positive finite number.
pub fn unit(a: V3) -> Option<V3> {
    let n = length(a);
    (n > 0.0 && n.is_finite()).then(|| scale(a, 1.0 / n))
}

/// Axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}
impl Aabb {
    pub fn of(points: &[V3], radius: f64) -> Option<Self> {
        let first = *points.first()?;
        let mut b = Self {
            min: first,
            max: first,
        };
        for p in &points[1..] {
            b = b.union(Self { min: *p, max: *p });
        }
        Some(b.grow(radius))
    }
    pub fn grow(self, r: f64) -> Self {
        Self {
            min: self.min.map(|x| x - r),
            max: self.max.map(|x| x + r),
        }
    }
    pub fn union(self, o: Self) -> Self {
        Self {
            min: std::array::from_fn(|k| self.min[k].min(o.min[k])),
            max: std::array::from_fn(|k| self.max[k].max(o.max[k])),
        }
    }
    pub fn overlaps(&self, o: &Self) -> bool {
        (0..3).all(|k| self.min[k] <= o.max[k] && o.min[k] <= self.max[k])
    }
}

/// Closest points of two convex hulls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Closest {
    /// Distance between the hulls; 0 when they intersect.
    pub distance: f64,
    /// Closest point on the first hull.
    pub on_a: V3,
    /// Closest point on the second hull.
    pub on_b: V3,
}

#[derive(Clone, Copy)]
struct Vertex {
    w: V3,
    a: V3,
    b: V3,
}

/// Most iterations of the GJK loop; the point sets here have at most a few
/// hundred points and converge in far fewer.
const MAX_ITERATIONS: usize = 64;

fn support(points: &[V3], d: V3) -> V3 {
    let mut best = points[0];
    let mut value = dot(best, d);
    for p in &points[1..] {
        let v = dot(*p, d);
        if v > value {
            value = v;
            best = *p;
        }
    }
    best
}

/// Solves the n x n system `m x = r` (n <= 3) with partial pivoting;
/// `None` when it is singular.
fn solve(mut m: [[f64; 3]; 3], mut r: [f64; 3], n: usize) -> Option<[f64; 3]> {
    let scale_of = m
        .iter()
        .take(n)
        .flat_map(|row| row.iter().take(n))
        .fold(0.0f64, |a, x| a.max(x.abs()));
    if scale_of == 0.0 {
        return None;
    }
    for col in 0..n {
        let pivot = (col..n).max_by(|i, j| m[*i][col].abs().total_cmp(&m[*j][col].abs()))?;
        if m[pivot][col].abs() <= 1e-12 * scale_of {
            return None;
        }
        m.swap(col, pivot);
        r.swap(col, pivot);
        for row in col + 1..n {
            let f = m[row][col] / m[col][col];
            let pivot_row = m[col];
            for (k, x) in m[row].iter_mut().enumerate().take(n).skip(col) {
                *x -= f * pivot_row[k];
            }
            r[row] -= f * r[col];
        }
    }
    let mut x = [0.0; 3];
    for row in (0..n).rev() {
        let mut s = r[row];
        for k in row + 1..n {
            s -= m[row][k] * x[k];
        }
        x[row] = s / m[row][row];
    }
    Some(x)
}

/// Closest point to the origin of the hull of `s`: the point, the weights
/// of the vertices that span it (others 0) and whether the whole simplex
/// (a tetrahedron) contains the origin.
fn closest_on_simplex(s: &[Vertex]) -> (V3, [f64; 4], bool) {
    let n = s.len();
    let mut best: Option<(f64, V3, [f64; 4], usize)> = None;
    for mask in 1usize..(1 << n) {
        let mut idx = [0usize; 4];
        let mut k = 0;
        for i in (0..n).filter(|i| mask & (1 << i) != 0) {
            idx[k] = i;
            k += 1;
        }
        let w0 = s[idx[0]].w;
        let mut weights = [0.0; 4];
        let point = if k == 1 {
            weights[idx[0]] = 1.0;
            w0
        } else {
            let e: [V3; 3] = std::array::from_fn(|j| {
                if j + 1 < k {
                    sub(s[idx[j + 1]].w, w0)
                } else {
                    [0.0; 3]
                }
            });
            let mut m = [[0.0; 3]; 3];
            let mut r = [0.0; 3];
            for i in 0..k - 1 {
                for j in 0..k - 1 {
                    m[i][j] = dot(e[i], e[j]);
                }
                r[i] = -dot(e[i], w0);
            }
            let Some(mu) = solve(m, r, k - 1) else {
                continue;
            };
            let l0 = 1.0 - mu[..k - 1].iter().sum::<f64>();
            if l0 < 0.0 || mu[..k - 1].iter().any(|x| *x < 0.0) {
                continue;
            }
            weights[idx[0]] = l0;
            let mut p = scale(w0, l0);
            for (j, i) in idx[1..k].iter().enumerate() {
                weights[*i] = mu[j];
                p = add(p, scale(s[*i].w, mu[j]));
            }
            p
        };
        let d = dot(point, point);
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, point, weights, k));
        }
    }
    let (_, point, weights, k) = best.unwrap_or((0.0, s[0].w, [1.0, 0.0, 0.0, 0.0], 1));
    (point, weights, k == 4)
}

/// Closest points between the convex hulls of `a` and `b` (both non-empty).
pub fn closest(a: &[V3], b: &[V3]) -> Closest {
    let vertex = |d: V3| {
        let pa = support(a, scale(d, -1.0));
        let pb = support(b, d);
        Vertex {
            w: sub(pa, pb),
            a: pa,
            b: pb,
        }
    };
    let first = Vertex {
        w: sub(a[0], b[0]),
        a: a[0],
        b: b[0],
    };
    let mut simplex = vec![first];
    let mut weights = [1.0, 0.0, 0.0, 0.0];
    let mut v = first.w;
    let mut inside = false;
    for _ in 0..MAX_ITERATIONS {
        let vv = dot(v, v);
        if vv <= 1e-24 {
            inside = true;
            break;
        }
        let w = vertex(v);
        // The support point does not get closer to the origin than v by a
        // meaningful amount: v is the closest point.
        if vv - dot(v, w.w) <= 1e-12 * vv.max(1e-6) {
            break;
        }
        if simplex
            .iter()
            .any(|s| dot(sub(s.w, w.w), sub(s.w, w.w)) <= 1e-24)
        {
            break;
        }
        simplex.push(w);
        let (p, l, full) = closest_on_simplex(&simplex);
        if full {
            weights = l;
            inside = true;
            break;
        }
        if dot(p, p) >= vv {
            // No progress: keep the previous answer.
            simplex.pop();
            break;
        }
        let mut kept = Vec::with_capacity(4);
        // At most four vertices: the closest point keeps only the ones it uses.
        let mut kept_w = [0.0; 4];
        for (i, s) in simplex.iter().enumerate() {
            if l[i] > 0.0 {
                kept_w[kept.len()] = l[i];
                kept.push(*s);
            }
        }
        simplex = kept;
        weights = kept_w;
        v = p;
    }
    let mut on_a = [0.0; 3];
    let mut on_b = [0.0; 3];
    for (i, s) in simplex.iter().enumerate() {
        on_a = add(on_a, scale(s.a, weights[i]));
        on_b = add(on_b, scale(s.b, weights[i]));
    }
    Closest {
        distance: if inside { 0.0 } else { length(sub(on_a, on_b)) },
        on_a,
        on_b,
    }
}

/// Closest point of segment `[p, q]` to `x`.
pub fn closest_on_segment(p: V3, q: V3, x: V3) -> V3 {
    let d = sub(q, p);
    let dd = dot(d, d);
    if dd <= 0.0 {
        return p;
    }
    let t = (dot(sub(x, p), d) / dd).clamp(0.0, 1.0);
    add(p, scale(d, t))
}

/// Unit normal `(b - a) x (c - a)` of a triangle; `None` when degenerate.
pub fn triangle_normal([a, b, c]: [V3; 3]) -> Option<V3> {
    unit(cross(sub(b, a), sub(c, a)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn point_to_triangle_face_edge_and_vertex() {
        let t = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        let c = closest(&[[2.0, 2.0, 5.0]], &t);
        assert!(near(c.distance, 5.0));
        assert!(near(c.on_b[0], 2.0) && near(c.on_b[1], 2.0) && near(c.on_b[2], 0.0));
        let c = closest(&[[5.0, -3.0, 4.0]], &t);
        assert!(near(c.distance, 5.0), "{c:?}");
        let c = closest(&[[-3.0, -4.0, 0.0]], &t);
        assert!(near(c.distance, 5.0));
    }

    #[test]
    fn segments_parallel_crossing_and_intersecting() {
        let a = [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]];
        let b = [[3.0, -5.0, 4.0], [3.0, 5.0, 4.0]];
        let c = closest(&a, &b);
        assert!(near(c.distance, 3.0));
        assert!(near(c.on_a[2], 4.0));
        let b = [[2.0, 0.0, 1.0], [2.0, 0.0, 20.0]];
        assert!(near(closest(&a, &b).distance, 2.0));
        let b = [[-1.0, 0.0, 5.0], [1.0, 0.0, 5.0]];
        assert_eq!(closest(&a, &b).distance, 0.0);
    }

    #[test]
    fn segment_through_a_triangle_and_against_a_box() {
        let t = [[-5.0, -5.0, 0.0], [5.0, -5.0, 0.0], [0.0, 5.0, 0.0]];
        let s = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0]];
        assert_eq!(closest(&s, &t).distance, 0.0);
        let cube: Vec<V3> = (0..8)
            .map(|i| {
                [
                    f64::from(i & 1) * 2.0 - 1.0,
                    f64::from((i >> 1) & 1) * 2.0 - 1.0,
                    f64::from((i >> 2) & 1) * 2.0 - 1.0,
                ]
            })
            .collect();
        let s = [[4.0, 0.5, -0.5], [4.0, 0.5, 0.5]];
        let c = closest(&s, &cube);
        assert!(near(c.distance, 3.0), "{c:?}");
        let s = [[3.0, 3.0, 3.0], [3.0, 3.0, 9.0]];
        assert!(near(closest(&s, &cube).distance, (12.0f64).sqrt()));
        let s = [[0.0, 0.0, -9.0], [0.0, 0.0, 9.0]];
        assert_eq!(closest(&s, &cube).distance, 0.0);
    }
}
