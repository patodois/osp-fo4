//! Which trigger volumes a walk over a cell's walkable surface must enter
//! before others (`spec/vm.md`, The museum's order; `walk_order_census`).
//!
//! The surface is a set of triangles joined across shared edges and across
//! the links the caller follows (`rules::WALK_NAVMESH_SURFACE`: a cell's
//! navigation meshes). The player is a vertical segment of `h` units
//! standing on it (`world_trigger_body_v1`); a volume blocks the surface
//! where the standing player meets it ([`Volume::span_body`]). Each volume
//! the player meets is convex (a box or a sphere swept down by the body),
//! so a triangle minus one volume falls into as many pieces as the free
//! arcs of its boundary: each triangle becomes one node per free arc, and
//! nodes join across the free parts of shared edges. This is exact for one
//! volume per triangle; triangles whose boundary two volumes meet are
//! counted ([`Split::multi`]), their arcs counted apart.
//!
//! A volume `u` is entered first on every walk from an entry to a volume
//! `t` when, with `u` blocking, no node the entry stands on joins a node
//! whose piece meets `t`. Nothing here is game behaviour beyond the rules
//! named; nothing is measured in the running game.
use crate::track::{Shape, Volume};
use std::collections::{BTreeMap, BTreeSet};

/// A walkable surface: triangles (three corners each), the triangle across
/// each edge (edge `e` runs from corner `e` to corner `e + 1`) and the
/// links followed between edges of two triangles.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub tris: Vec<[[f64; 3]; 3]>,
    pub next: Vec<[Option<usize>; 3]>,
    /// (triangle, edge) to (triangle, edge).
    pub links: Vec<((usize, usize), (usize, usize))>,
}

/// Union-find over nodes.
#[derive(Debug, Clone)]
struct Sets(Vec<usize>);
impl Sets {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra] = rb;
        }
    }
}

/// The surface split by blocking volumes.
#[derive(Debug, Clone)]
pub struct Split {
    /// Free arcs of each triangle's boundary (perimeter parameter, edge `e`
    /// covering `[e, e + 1)`; an arc across the start ends past 3).
    arcs: Vec<Vec<(f64, f64)>>,
    /// First node of each triangle.
    base: Vec<usize>,
    sets: Sets,
    /// Triangles whose boundary two different blocking volumes meet.
    pub multi: u64,
}
impl Split {
    /// The piece (a node's root) of a triangle's arc.
    pub fn root(&mut self, t: usize, arc: usize) -> usize {
        self.sets.find(self.base[t] + arc)
    }
    /// Sizes of the pieces (in free arcs), largest first.
    pub fn sizes(&mut self) -> Vec<u64> {
        let mut by: BTreeMap<usize, u64> = BTreeMap::new();
        for t in 0..self.arcs.len() {
            for j in 0..self.arcs[t].len() {
                let r = self.root(t, j);
                *by.entry(r).or_default() += 1;
            }
        }
        let mut v: Vec<u64> = by.values().copied().collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v
    }
    /// The size of every piece by its root.
    pub fn by_root(&mut self) -> BTreeMap<usize, u64> {
        let mut by: BTreeMap<usize, u64> = BTreeMap::new();
        for t in 0..self.arcs.len() {
            for j in 0..self.arcs[t].len() {
                let r = self.root(t, j);
                *by.entry(r).or_default() += 1;
            }
        }
        by
    }
}

fn lerp(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * s)
}

/// The part of edge `e` of triangle `c` (parameter 0 to 1) where the
/// standing player meets the volume.
pub fn edge_span(v: &Volume, c: &[[f64; 3]; 3], e: usize, h: f64) -> Option<(f64, f64)> {
    v.span_body(c[e], c[(e + 1) % 3], h)
        .map(|(a, b)| (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0)))
        .filter(|(a, b)| a <= b)
}

/// Whether the player standing at `p` meets the volume.
pub fn inside(v: &Volume, p: [f64; 3], h: f64) -> bool {
    v.span_body(p, p, h).is_some()
}

/// Points of a triangle on a barycentric grid about `step` units apart
/// (at most 257 along an edge).
pub fn samples(c: &[[f64; 3]; 3], step: f64) -> Vec<[f64; 3]> {
    let len = (0..3)
        .map(|e| {
            let (a, b) = (c[e], c[(e + 1) % 3]);
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        })
        .fold(0.0f64, f64::max);
    let n = ((len / step).ceil() as usize).clamp(1, 256);
    let mut out = Vec::new();
    for i in 0..=n {
        for j in 0..=(n - i) {
            let (u, w) = (i as f64 / n as f64, j as f64 / n as f64);
            out.push(std::array::from_fn(|k| {
                c[0][k] + (c[1][k] - c[0][k]) * u + (c[2][k] - c[0][k]) * w
            }));
        }
    }
    out
}

/// Whether the volume meets the triangle: an edge, or a sample of the
/// interior every `step` units.
pub fn meets(v: &Volume, c: &[[f64; 3]; 3], h: f64, step: f64) -> bool {
    (0..3).any(|e| edge_span(v, c, e, h).is_some())
        || samples(c, step).into_iter().any(|p| inside(v, p, h))
}

/// The free arcs of a triangle's boundary under the volumes; empty when the
/// whole boundary is blocked. Also whether two volumes meet the boundary.
fn free_arcs(vols: &[&Volume], c: &[[f64; 3]; 3], h: f64) -> (Vec<(f64, f64)>, bool) {
    let mut blocked: Vec<(f64, f64)> = Vec::new();
    let mut touching = 0usize;
    for v in vols {
        let mut met = false;
        for e in 0..3 {
            if let Some((a, b)) = edge_span(v, c, e, h) {
                blocked.push((e as f64 + a, e as f64 + b));
                met = true;
            }
        }
        touching += usize::from(met);
    }
    if blocked.is_empty() {
        return (vec![(0.0, 3.0)], false);
    }
    blocked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in blocked {
        match merged.last_mut() {
            Some(l) if a <= l.1 + 1e-9 => l.1 = l.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut free = Vec::new();
    for w in merged.windows(2) {
        if w[1].0 - w[0].1 > 1e-9 {
            free.push((w[0].1, w[1].0));
        }
    }
    let (first, last) = (merged[0], merged[merged.len() - 1]);
    if (3.0 - last.1) + first.0 > 1e-9 {
        free.push((last.1, 3.0 + first.0));
    }
    (free, touching > 1)
}

fn arc_at(arcs: &[(f64, f64)], s: f64) -> Option<usize> {
    arcs.iter().position(|&(a, b)| {
        (a - 1e-9..=b + 1e-9).contains(&s) || (a - 1e-9..=b + 1e-9).contains(&(s + 3.0))
    })
}

/// The free sub-intervals of edge `e` (parameter 0 to 1).
fn edge_free(vols: &[&Volume], c: &[[f64; 3]; 3], e: usize, h: f64) -> Vec<(f64, f64)> {
    let mut bl: Vec<(f64, f64)> = vols.iter().filter_map(|v| edge_span(v, c, e, h)).collect();
    bl.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = 0.0f64;
    for (a, b) in bl {
        if a - at > 1e-9 {
            out.push((at, a));
        }
        at = at.max(b);
    }
    if 1.0 - at > 1e-9 {
        out.push((at, 1.0));
    }
    out
}

fn same(x: [f64; 3], y: [f64; 3]) -> bool {
    (0..3).all(|k| (x[k] - y[k]).abs() < 1e-3)
}

/// Splits the surface by the blocking volumes.
pub fn split(s: &Surface, vols: &[&Volume], h: f64) -> Split {
    let mut arcs = Vec::with_capacity(s.tris.len());
    let mut base = Vec::with_capacity(s.tris.len());
    let mut n = 0usize;
    let mut multi = 0;
    for c in &s.tris {
        let all_in = vols.iter().any(|v| (0..3).all(|k| inside(v, c[k], h)));
        let (f, two) = if all_in {
            (Vec::new(), false)
        } else {
            free_arcs(vols, c, h)
        };
        multi += u64::from(two);
        base.push(n);
        n += f.len();
        arcs.push(f);
    }
    let mut sp = Split {
        arcs,
        base,
        sets: Sets((0..n).collect()),
        multi,
    };
    for (t, c) in s.tris.iter().enumerate() {
        for e in 0..3 {
            let Some(u) = s.next[t][e] else { continue };
            if u <= t || u >= s.tris.len() {
                continue;
            }
            let cu = s.tris[u];
            let (p, q) = (c[e], c[(e + 1) % 3]);
            let Some(f) = (0..3).find(|&f| {
                let (a, b) = (cu[f], cu[(f + 1) % 3]);
                (same(a, p) && same(b, q)) || (same(b, p) && same(a, q))
            }) else {
                continue;
            };
            let rev = same(cu[f], q);
            for (a, b) in edge_free(vols, c, e, h) {
                let x = (a + b) / 2.0;
                let y = if rev { 1.0 - x } else { x };
                let (Some(i), Some(j)) = (
                    arc_at(&sp.arcs[t], e as f64 + x),
                    arc_at(&sp.arcs[u], f as f64 + y),
                ) else {
                    continue;
                };
                let (na, nb) = (sp.base[t] + i, sp.base[u] + j);
                sp.sets.join(na, nb);
            }
        }
    }
    for &((ta, ea), (tb, eb)) in &s.links {
        if ta >= s.tris.len() || tb >= s.tris.len() {
            continue;
        }
        let widest = |v: Vec<(f64, f64)>| {
            v.into_iter()
                .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
        };
        let (Some(x), Some(y)) = (
            widest(edge_free(vols, &s.tris[ta], ea % 3, h)),
            widest(edge_free(vols, &s.tris[tb], eb % 3, h)),
        ) else {
            continue;
        };
        let (Some(i), Some(j)) = (
            arc_at(&sp.arcs[ta], (ea % 3) as f64 + (x.0 + x.1) / 2.0),
            arc_at(&sp.arcs[tb], (eb % 3) as f64 + (y.0 + y.1) / 2.0),
        ) else {
            continue;
        };
        let (na, nb) = (sp.base[ta] + i, sp.base[tb] + j);
        sp.sets.join(na, nb);
    }
    sp
}

/// The free arc of a triangle a free point lies in: an arc with a sample
/// the point sees along a straight segment no blocking volume meets.
fn arc_of_point(
    vols: &[&Volume],
    c: &[[f64; 3]; 3],
    arcs: &[(f64, f64)],
    p: [f64; 3],
    h: f64,
) -> Option<usize> {
    if arcs.len() == 1 {
        return Some(0);
    }
    for (i, &(a, b)) in arcs.iter().enumerate() {
        for k in 0..=16 {
            let sp = (a + (b - a) * f64::from(k) / 16.0) % 3.0;
            let e = (sp.floor() as usize).min(2);
            let q = lerp(c[e], c[(e + 1) % 3], sp - e as f64);
            if vols.iter().all(|v| v.span_body(p, q, h).is_none()) {
                return Some(i);
            }
        }
    }
    None
}

/// The height of the triangle's plane at (x, y) when its top-down
/// footprint holds the point.
fn height_in(c: &[[f64; 3]; 3], x: f64, y: f64) -> Option<f64> {
    let d = (c[1][1] - c[2][1]) * (c[0][0] - c[2][0]) + (c[2][0] - c[1][0]) * (c[0][1] - c[2][1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let a = ((c[1][1] - c[2][1]) * (x - c[2][0]) + (c[2][0] - c[1][0]) * (y - c[2][1])) / d;
    let b = ((c[2][1] - c[0][1]) * (x - c[2][0]) + (c[0][0] - c[2][0]) * (y - c[2][1])) / d;
    let g = 1.0 - a - b;
    let eps = -1e-9;
    (a >= eps && b >= eps && g >= eps).then(|| a * c[0][2] + b * c[1][2] + g * c[2][2])
}

/// The piece an entry point stands on: the triangle whose footprint holds
/// it with the nearest plane (else the nearest centroid within 128 units of
/// height), and the free arc it lies in; `None` when it stands on no free
/// surface.
pub fn entry(s: &Surface, sp: &mut Split, vols: &[&Volume], p: [f64; 3], h: f64) -> Option<usize> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (t, c) in s.tris.iter().enumerate() {
        if let Some(z) = height_in(c, p[0], p[1]) {
            let d = (z - p[2]).abs();
            if best.is_none_or(|(_, bd, _)| d < bd) {
                best = Some((t, d, z));
            }
        }
    }
    let (t, z) = match best {
        Some((t, _, z)) => (t, z),
        None => {
            let mut near: Option<(usize, f64, f64)> = None;
            for (t, c) in s.tris.iter().enumerate() {
                let q: [f64; 3] = std::array::from_fn(|i| (c[0][i] + c[1][i] + c[2][i]) / 3.0);
                if (q[2] - p[2]).abs() > 128.0 {
                    continue;
                }
                let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                if near.is_none_or(|(_, bd, _)| d < bd) {
                    near = Some((t, d, q[2]));
                }
            }
            let (t, _, z) = near?;
            (t, z)
        }
    };
    if sp.arcs[t].is_empty() {
        return None;
    }
    let arcs = sp.arcs[t].clone();
    let i = arc_of_point(vols, &s.tris[t], &arcs, [p[0], p[1], z], h)?;
    Some(sp.root(t, i))
}

/// The pieces that meet a volume: in a triangle split into several pieces,
/// those a free sample inside the volume lies in (samples every 4 units).
pub fn meeting(
    s: &Surface,
    sp: &mut Split,
    vols: &[&Volume],
    v: &Volume,
    h: f64,
) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for (t, c) in s.tris.iter().enumerate() {
        if sp.arcs[t].is_empty() || !meets(v, c, h, 4.0) {
            continue;
        }
        if sp.arcs[t].len() == 1 {
            out.insert(sp.root(t, 0));
            continue;
        }
        let arcs = sp.arcs[t].clone();
        for p in samples(c, 4.0) {
            if !inside(v, p, h) || vols.iter().any(|w| inside(w, p, h)) {
                continue;
            }
            if let Some(i) = arc_of_point(vols, c, &arcs, p, h) {
                out.insert(sp.root(t, i));
            }
        }
    }
    out
}

/// Whether a walk from the entry point can reach the target volume with
/// the blocking volumes in place; `None` when the entry stands on no free
/// surface.
pub fn reach(
    s: &Surface,
    vols: &[&Volume],
    from: [f64; 3],
    target: &Volume,
    h: f64,
) -> Option<bool> {
    let mut sp = split(s, vols, h);
    let e = entry(s, &mut sp, vols, from, h)?;
    Some(meeting(s, &mut sp, vols, target, h).contains(&e))
}

/// The volumes among `others` that every walk from the entry to the target
/// enters first (the target reached with none of them blocking, and not
/// with that one).
pub fn entered_first(
    s: &Surface,
    blocking: &[&Volume],
    from: [f64; 3],
    target: &Volume,
    others: &[&Volume],
    h: f64,
) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, u) in others.iter().enumerate() {
        let mut v: Vec<&Volume> = blocking.to_vec();
        v.push(u);
        if reach(s, &v, from, target, h) == Some(false) {
            out.push(i);
        }
    }
    out
}

/// A box volume grown by `margin` units on every half extent (a sphere's
/// radius likewise).
pub fn grown(v: &Volume, margin: f64) -> Volume {
    let mut g = v.clone();
    g.shape = match v.shape {
        Shape::Box(b) => Shape::Box(b.map(|x| x + margin)),
        Shape::Sphere(r) => Shape::Sphere(r + margin),
    };
    g
}
