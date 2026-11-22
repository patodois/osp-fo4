//! A path search over one or more navmeshes (`nav_path_search_v1`).
//!
//! This is the project's own algorithm, built from general graph search and
//! funnel geometry; it does not claim to reproduce how the game's actors
//! choose a path, which nothing here measures.
//!
//! - Nodes are triangles. Two nodes are joined across an edge when the edge
//!   names a neighbour triangle of the same mesh, or names an edge link
//!   whose navmesh the caller resolves to a loaded mesh and whose triangle
//!   exists there. The shared edge (the "portal") is the crossing edge's two
//!   vertices in the mesh it leaves.
//! - A* runs over triangle centroids: the cost of a step is the distance
//!   between the two centroids and the heuristic the distance from a
//!   centroid to the goal triangle's centroid (consistent by the triangle
//!   inequality, so the centroid route found is the shortest one).
//! - The polyline is the corridor's portals pulled tight from the start
//!   point to the goal point by a funnel in the horizontal plane; each
//!   corner keeps the height of the portal vertex it sits on.
use crate::mesh::{Edge, Navmesh};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A triangle of one mesh of the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct NodeId {
    pub mesh: u32,
    pub triangle: u16,
}

/// How a step crosses into the next triangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Crossing {
    /// A neighbour word of the same mesh.
    Neighbour,
    /// An edge link of the given kind word.
    Link(u32),
}

#[derive(Debug, Clone, Copy)]
struct Arc {
    to: u32,
    portal: [[f32; 3]; 2],
    crossing: Crossing,
}

/// Counts of what the graph joined and left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct GraphStats {
    pub nodes: u64,
    pub neighbour_arcs: u64,
    pub link_arcs: u64,
    /// Edge links whose navmesh the resolver did not map to a loaded mesh.
    pub links_unresolved: u64,
    /// Edge links whose triangle is outside the linked mesh.
    pub links_bad_triangle: u64,
    /// Edge links of a kind word the caller chose not to follow.
    pub links_not_followed: u64,
    /// Edge words that are neither a neighbour, an open edge nor a link.
    pub invalid_edges: u64,
}

/// The triangles of a set of meshes and the arcs between them.
pub struct NavGraph<'a> {
    meshes: Vec<&'a Navmesh>,
    base: Vec<u32>,
    starts: Vec<u32>,
    arcs: Vec<Arc>,
    pub stats: GraphStats,
}

/// Why no path was returned.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PathError {
    /// The point lies on no triangle of the graph.
    StartNotOnMesh,
    GoalNotOnMesh,
    /// The two triangles are not connected.
    NoPath {
        expanded: u64,
    },
    /// A node outside the graph.
    BadNode(NodeId),
    Budget(Error),
}
impl From<Error> for PathError {
    fn from(e: Error) -> Self {
        Self::Budget(e)
    }
}

/// A found path: the triangle corridor, its portals and the pulled polyline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Corridor {
    pub nodes: Vec<NodeId>,
    /// `crossings[i]` and `portals[i]` lead from `nodes[i]` to `nodes[i + 1]`.
    pub crossings: Vec<Crossing>,
    /// Each portal as (left, right) seen in the direction of travel.
    pub portals: Vec<[[f64; 3]; 2]>,
    /// Start point, corners, goal point.
    pub points: Vec<[f64; 3]>,
    /// Length of `points` in three dimensions.
    pub length: f64,
    /// Length of the centroid route A* minimised.
    pub centroid_length: f64,
    /// Nodes A* took off the open set.
    pub expanded: u64,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn f64x3(p: [f32; 3]) -> [f64; 3] {
    p.map(f64::from)
}

/// Height of the plane of triangle `c` at `(x, y)` when `(x, y)` lies in its
/// top-down footprint (edges included, either winding); `None` otherwise or
/// for a triangle seen edge-on.
pub fn height_in(c: &[[f32; 3]; 3], x: f64, y: f64) -> Option<f64> {
    let [a, b, d] = c.map(f64x3);
    let det = (b[1] - d[1]) * (a[0] - d[0]) + (d[0] - b[0]) * (a[1] - d[1]);
    if det.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - d[1]) * (x - d[0]) + (d[0] - b[0]) * (y - d[1])) / det;
    let l2 = ((d[1] - a[1]) * (x - d[0]) + (a[0] - d[0]) * (y - d[1])) / det;
    let l3 = 1.0 - l1 - l2;
    let eps = -1e-9;
    (l1 >= eps && l2 >= eps && l3 >= eps).then(|| l1 * a[2] + l2 * b[2] + l3 * d[2])
}

#[derive(PartialEq)]
struct Open {
    f: f64,
    node: u32,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        // Smallest f first; ties broken by node index for determinism.
        o.f.total_cmp(&self.f).then(o.node.cmp(&self.node))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl<'a> NavGraph<'a> {
    /// Joins the triangles of `meshes`. `resolve(mesh, formid)` maps an edge
    /// link's stored navmesh FormID, read from mesh `mesh`, to the index of
    /// a loaded mesh, or `None` when that mesh is not loaded. `follow(kind)`
    /// says which edge-link kind words become arcs: what the kinds mean is
    /// not established (`spec/navmesh.md`: kind 0 joins touching edges of
    /// neighbouring meshes; kinds 1 and 2 pair up and mostly join distant
    /// edges of one mesh).
    pub fn build(
        meshes: Vec<&'a Navmesh>,
        resolve: impl Fn(usize, u32) -> Option<usize>,
        follow: impl Fn(u32) -> bool,
        b: &Budget,
    ) -> Result<Self> {
        let mut base = Vec::new();
        b.reserve(&mut base, meshes.len() + 1, "nav graph", 0)?;
        let mut total: u32 = 0;
        for m in &meshes {
            base.push(total);
            total = total
                .checked_add(m.triangles.len() as u32)
                .ok_or_else(|| Error::new("Overflow", "nav graph", 0, "u32 nodes", "more"))?;
        }
        base.push(total);
        let mut starts = Vec::new();
        b.reserve(&mut starts, total as usize + 1, "nav graph", 0)?;
        let mut arcs = Vec::new();
        let mut stats = GraphStats {
            nodes: u64::from(total),
            ..GraphStats::default()
        };
        for (mi, m) in meshes.iter().enumerate() {
            let nt = m.triangles.len();
            let nl = m.edge_links.len();
            for t in &m.triangles {
                starts.push(arcs.len() as u32);
                for e in 0..3 {
                    let [va, vb] = t.edge_vertices(e);
                    let portal = [m.vertices[usize::from(va)], m.vertices[usize::from(vb)]];
                    let (to, crossing) = match t.edge(e, nt, nl) {
                        Edge::Open => continue,
                        Edge::Invalid(_) => {
                            stats.invalid_edges += 1;
                            continue;
                        }
                        Edge::Triangle(u) => {
                            stats.neighbour_arcs += 1;
                            (base[mi] + u32::from(u), Crossing::Neighbour)
                        }
                        Edge::Link(k) => {
                            let link = m.edge_links[usize::from(k)];
                            if !follow(link.kind) {
                                stats.links_not_followed += 1;
                                continue;
                            }
                            let Some(mj) = resolve(mi, link.navmesh).filter(|j| *j < meshes.len())
                            else {
                                stats.links_unresolved += 1;
                                continue;
                            };
                            if usize::from(link.triangle) >= meshes[mj].triangles.len() {
                                stats.links_bad_triangle += 1;
                                continue;
                            }
                            stats.link_arcs += 1;
                            (
                                base[mj] + u32::from(link.triangle),
                                Crossing::Link(link.kind),
                            )
                        }
                    };
                    b.reserve(&mut arcs, 1, "nav graph", 0)?;
                    arcs.push(Arc {
                        to,
                        portal,
                        crossing,
                    });
                }
            }
        }
        starts.push(arcs.len() as u32);
        Ok(Self {
            meshes,
            base,
            starts,
            arcs,
            stats,
        })
    }

    pub fn meshes(&self) -> &[&'a Navmesh] {
        &self.meshes
    }
    pub fn node_count(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }
    fn index(&self, n: NodeId) -> Option<u32> {
        let m = self.meshes.get(n.mesh as usize)?;
        (usize::from(n.triangle) < m.triangles.len())
            .then(|| self.base[n.mesh as usize] + u32::from(n.triangle))
    }
    /// The node of a flat index.
    pub fn node(&self, i: u32) -> NodeId {
        let mesh = self.base.partition_point(|b| *b <= i) - 1;
        NodeId {
            mesh: mesh as u32,
            triangle: (i - self.base[mesh]) as u16,
        }
    }
    fn centroid(&self, i: u32) -> [f64; 3] {
        let n = self.node(i);
        self.meshes[n.mesh as usize]
            .centroid(usize::from(n.triangle))
            .unwrap_or_default()
    }
    fn out(&self, i: u32) -> &[Arc] {
        &self.arcs[self.starts[i as usize] as usize..self.starts[i as usize + 1] as usize]
    }

    /// The triangle under `p`: among the triangles whose top-down
    /// footprint holds `(x, y)`, the one whose plane is nearest to `z`.
    pub fn locate(&self, p: [f64; 3]) -> Option<NodeId> {
        let mut best: Option<(NodeId, f64)> = None;
        for (mi, m) in self.meshes.iter().enumerate() {
            if let Some(t) = m.locate(p) {
                let c = m.corners(t)?;
                let d = (height_in(&c, p[0], p[1])? - p[2]).abs();
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((
                        NodeId {
                            mesh: mi as u32,
                            triangle: t as u16,
                        },
                        d,
                    ));
                }
            }
        }
        best.map(|(n, _)| n)
    }

    /// Connected components, treating every arc as two-way: the component
    /// index of every node and the size of every component.
    pub fn components(&self, b: &Budget) -> Result<(Vec<u32>, Vec<u64>)> {
        let n = self.node_count();
        let mut parent: Vec<u32> = Vec::new();
        b.reserve(&mut parent, n, "nav components", 0)?;
        parent.extend(0..n as u32);
        fn find(p: &mut [u32], mut x: u32) -> u32 {
            while p[x as usize] != x {
                let up = p[p[x as usize] as usize];
                p[x as usize] = up;
                x = up;
            }
            x
        }
        for i in 0..n as u32 {
            for a in self.out(i) {
                let (x, y) = (find(&mut parent, i), find(&mut parent, a.to));
                if x != y {
                    parent[x.max(y) as usize] = x.min(y);
                }
            }
        }
        let mut label = Vec::new();
        b.reserve(&mut label, n, "nav components", 0)?;
        let mut sizes: Vec<u64> = Vec::new();
        let mut root_label: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for i in 0..n as u32 {
            let r = find(&mut parent, i);
            let next = root_label.len() as u32;
            let l = *root_label.entry(r).or_insert(next);
            if l as usize == sizes.len() {
                sizes.push(0);
            }
            sizes[l as usize] += 1;
            label.push(l);
        }
        Ok((label, sizes))
    }

    /// The path between two points on the graph.
    pub fn find_path(
        &self,
        start: [f64; 3],
        goal: [f64; 3],
        b: &Budget,
    ) -> std::result::Result<Corridor, PathError> {
        let s = self.locate(start).ok_or(PathError::StartNotOnMesh)?;
        let g = self.locate(goal).ok_or(PathError::GoalNotOnMesh)?;
        self.find_path_between(s, start, g, goal, b)
    }

    /// The path from point `start` on triangle `s` to point `goal` on
    /// triangle `g` (the points are taken as given).
    pub fn find_path_between(
        &self,
        s: NodeId,
        start: [f64; 3],
        g: NodeId,
        goal: [f64; 3],
        b: &Budget,
    ) -> std::result::Result<Corridor, PathError> {
        let si = self.index(s).ok_or(PathError::BadNode(s))?;
        let gi = self.index(g).ok_or(PathError::BadNode(g))?;
        let n = self.node_count();
        let mut cost: Vec<f64> = Vec::new();
        b.reserve(&mut cost, n, "nav search", 0)?;
        cost.resize(n, f64::INFINITY);
        let mut came: Vec<u32> = Vec::new();
        b.reserve(&mut came, n, "nav search", 0)?;
        came.resize(n, u32::MAX);
        let mut via: Vec<u32> = Vec::new();
        b.reserve(&mut via, n, "nav search", 0)?;
        via.resize(n, u32::MAX);
        let goal_c = self.centroid(gi);
        let mut open = BinaryHeap::new();
        cost[si as usize] = 0.0;
        open.push(Open {
            f: dist(self.centroid(si), goal_c),
            node: si,
        });
        let mut expanded = 0u64;
        let mut done: Vec<bool> = Vec::new();
        b.reserve(&mut done, n, "nav search", 0)?;
        done.resize(n, false);
        while let Some(Open { node, .. }) = open.pop() {
            if done[node as usize] {
                continue;
            }
            done[node as usize] = true;
            expanded += 1;
            if node == gi {
                break;
            }
            let c = self.centroid(node);
            for (k, a) in self.out(node).iter().enumerate() {
                if done[a.to as usize] {
                    continue;
                }
                let to_c = self.centroid(a.to);
                let ng = cost[node as usize] + dist(c, to_c);
                if ng < cost[a.to as usize] {
                    cost[a.to as usize] = ng;
                    came[a.to as usize] = node;
                    via[a.to as usize] = self.starts[node as usize] + k as u32;
                    open.push(Open {
                        f: ng + dist(to_c, goal_c),
                        node: a.to,
                    });
                }
            }
        }
        if !done[gi as usize] {
            return Err(PathError::NoPath { expanded });
        }
        let mut rev = vec![gi];
        let mut arcs_rev = Vec::new();
        let mut cur = gi;
        while cur != si {
            arcs_rev.push(via[cur as usize]);
            cur = came[cur as usize];
            rev.push(cur);
        }
        rev.reverse();
        arcs_rev.reverse();
        let nodes: Vec<NodeId> = rev.iter().map(|i| self.node(*i)).collect();
        let mut portals = Vec::with_capacity(arcs_rev.len());
        let mut crossings = Vec::with_capacity(arcs_rev.len());
        for (w, ai) in rev.windows(2).zip(&arcs_rev) {
            let a = &self.arcs[*ai as usize];
            crossings.push(a.crossing);
            let (from, to) = (self.centroid(w[0]), self.centroid(w[1]));
            let [p, q] = a.portal.map(f64x3);
            // Left is counter-clockwise from the direction of travel.
            let d = [to[0] - from[0], to[1] - from[1]];
            let side = d[0] * (p[1] - from[1]) - d[1] * (p[0] - from[0]);
            portals.push(if side >= 0.0 { [p, q] } else { [q, p] });
        }
        let points = funnel(start, goal, &portals);
        let length = points.windows(2).map(|w| dist(w[0], w[1])).sum();
        Ok(Corridor {
            nodes,
            crossings,
            portals,
            points,
            length,
            centroid_length: cost[gi as usize],
            expanded,
        })
    }
}

fn cross2(o: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}
fn same2(a: [f64; 3], b: [f64; 3]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

/// Distance in the horizontal plane from `p` to the segment `a`-`b`.
fn segment_distance2(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((a[0] + t * dx - p[0]).powi(2) + (a[1] + t * dy - p[1]).powi(2)).sqrt()
}

/// Pulls a corridor tight in the horizontal plane (a funnel over the
/// portals, each given as left then right). Leading portals that the start
/// lies on and trailing portals that the goal lies on are skipped: the
/// point is already on both sides of them.
pub fn funnel(start: [f64; 3], goal: [f64; 3], portals: &[[[f64; 3]; 2]]) -> Vec<[f64; 3]> {
    const ON: f64 = 1e-6;
    let mut lo = 0;
    while lo < portals.len() && segment_distance2(start, portals[lo][0], portals[lo][1]) < ON {
        lo += 1;
    }
    let mut hi = portals.len();
    while hi > lo && segment_distance2(goal, portals[hi - 1][0], portals[hi - 1][1]) < ON {
        hi -= 1;
    }
    let mut all = Vec::with_capacity(hi - lo + 2);
    all.push([start, start]);
    all.extend_from_slice(&portals[lo..hi]);
    all.push([goal, goal]);
    let mut points = vec![start];
    let mut apex = start;
    let (mut left, mut right) = (start, start);
    let (mut left_i, mut right_i) = (0usize, 0usize);
    let mut i = 1;
    while i < all.len() {
        let [l, r] = all[i];
        // Tighten the right side.
        if cross2(apex, right, r) >= 0.0 {
            if same2(apex, right) || cross2(apex, left, r) < 0.0 {
                right = r;
                right_i = i;
            } else {
                // The right side crosses the left: the left is a corner.
                apex = left;
                let apex_i = left_i;
                if !same2(*points.last().unwrap_or(&apex), apex) {
                    points.push(apex);
                }
                left = apex;
                right = apex;
                right_i = apex_i;
                i = apex_i + 1;
                continue;
            }
        }
        // Tighten the left side.
        if cross2(apex, left, l) <= 0.0 {
            if same2(apex, left) || cross2(apex, right, l) > 0.0 {
                left = l;
                left_i = i;
            } else {
                apex = right;
                let apex_i = right_i;
                if !same2(*points.last().unwrap_or(&apex), apex) {
                    points.push(apex);
                }
                left = apex;
                right = apex;
                left_i = apex_i;
                i = apex_i + 1;
                continue;
            }
        }
        i += 1;
    }
    if !same2(*points.last().unwrap_or(&goal), goal) || points.len() == 1 {
        points.push(goal);
    }
    points
}
