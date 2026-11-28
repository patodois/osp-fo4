//! The static world the controller walks in: the placed collision of a cell
//! as convex primitives, with a uniform grid over the horizontal plane as
//! the broad phase.
use crate::geometry::{self as g, Aabb, V3};
use crate::motion::Motion;
use crate::rules;
use fo4_core::{Budget, Error, Result};
use fo4_scene::collision::{BodyPlacement, CellCollision, ShapeClass, WorldShape};
use fo4_scene::terrain::Terrain;
use serde::Serialize;

const FILE: &str = "walk world";

/// One convex primitive in world units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Prim {
    /// A triangle; it blocks from both sides (`walk_triangles_block_both_sides_v1`).
    Triangle([V3; 3]),
    Sphere {
        center: V3,
        radius: f64,
    },
    Capsule {
        points: [V3; 2],
        radius: f64,
    },
    /// The hull of `count` points of [`World::points`] from `start`, widened
    /// by `radius`.
    Hull {
        start: u32,
        count: u32,
        radius: f64,
    },
}

/// Where a primitive comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Source {
    /// Index into [`CellCollision::instances`] when built from a cell.
    pub instance: Option<usize>,
    /// The instance's leaf shape class.
    pub class: Option<ShapeClass>,
    /// A label for triangle sets added with [`World::add_triangles`].
    pub label: Option<String>,
    /// XCLC grid of the cell whose terrain this is ([`World::add_terrain`]).
    pub terrain: Option<[i32; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Entry {
    pub prim: Prim,
    pub bounds: Aabb,
    /// Index into [`World::sources`].
    pub source: u32,
}

/// Counts of what was taken into the world and what was left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stats {
    pub instances: u64,
    pub triangles: u64,
    pub degenerate_triangles: u64,
    pub spheres: u64,
    pub capsules: u64,
    pub hulls: u64,
    pub hull_points: u64,
    /// `hknpConvexShape` hulls, whose radius is not read: taken with radius 0.
    pub hulls_without_radius: u64,
    /// Vertex records of compressed meshes: not part of the triangle set and
    /// not used as collision here (`spec/collision-placement.md`, Open).
    pub mesh_records_left_out: u64,
    /// Primitives with a coordinate that is not finite.
    pub non_finite_left_out: u64,
    pub grid_columns: [u64; 2],
    pub grid_cell: f64,
    pub grid_entries: u64,
}

/// Uniform grid over x and y; each column lists the primitives whose box
/// meets it, in increasing order.
#[derive(Debug, Clone, Default)]
struct Grid {
    origin: [f64; 2],
    cell: f64,
    n: [usize; 2],
    starts: Vec<u32>,
    items: Vec<u32>,
}

/// Largest number of grid columns; the column size grows to stay below it.
pub const MAX_GRID_COLUMNS: u64 = 1 << 20;
/// Column size of the broad phase, world units (a numerical setting).
pub const DEFAULT_GRID_CELL: f64 = 256.0;

#[derive(Debug, Clone, Default)]
pub struct World {
    pub entries: Vec<Entry>,
    pub points: Vec<V3>,
    pub sources: Vec<Source>,
    /// Every registered rule the world relies on: the placement rules of
    /// each instance and the controller's triangle rule, sorted.
    pub rules: Vec<&'static str>,
    pub stats: Stats,
    pub bounds: Option<Aabb>,
    grid: Grid,
}

/// A surface found by [`World::ray_down`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RayHit {
    pub z: f64,
    /// Upward-facing unit normal of the surface at the hit.
    pub normal: V3,
    pub entry: u32,
    pub source: u32,
}

/// Overlap of the character's capsule with one primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub entry: u32,
    /// How far the capsule reaches into the widened primitive.
    pub depth: f64,
    /// Unit normal from the primitive towards the capsule.
    pub normal: V3,
}

/// Most units between two samples along a capsule's core that
/// [`World::highest_steep_touch`] takes (bottom and top included): a shape's
/// top is found to within about this.
pub const TOUCH_STEP: f64 = 2.0;

fn grow<T>(v: &mut Vec<T>, n: usize, b: &Budget) -> Result<()> {
    b.reserve(v, n, FILE, 0)
}

impl World {
    /// An empty world; add primitives with the `add_*` methods.
    pub fn new() -> Self {
        Self::default()
    }

    /// The placed collision of a cell under body placement level `v`.
    /// Mesh triangles, polytope and convex hulls (with their radius when one
    /// is read), spheres and capsules become primitives; mesh vertex
    /// records are left out and counted.
    pub fn build(c: &CellCollision, v: BodyPlacement, b: &Budget) -> Result<Self> {
        let mut w = Self::new();
        w.add_rule(rules::TRIANGLE_SIDES, b)?;
        for (k, inst) in c.instances.iter().enumerate() {
            w.stats.instances += 1;
            for r in &inst.rules {
                w.add_rule(r, b)?;
            }
            let source = w.add_source(
                Source {
                    instance: Some(k),
                    class: Some(inst.kind),
                    label: None,
                    terrain: None,
                },
                b,
            )?;
            let work = b.child();
            match c.world_shape(inst, v, &work)? {
                WorldShape::Mesh { records, .. } => {
                    w.stats.mesh_records_left_out += records.len() as u64;
                    let n = c.world_triangles(inst, v).count();
                    grow(&mut w.entries, n, b)?;
                    for t in c.world_triangles(inst, v) {
                        w.push_triangle(t, source);
                    }
                }
                WorldShape::Points { vertices, radius } => {
                    if radius.is_none() {
                        w.stats.hulls_without_radius += 1;
                    }
                    w.push_hull(&vertices, radius.unwrap_or(0.0), source, b)?;
                }
                WorldShape::Sphere { center, radius } => {
                    w.push(Prim::Sphere { center, radius }, source, b)?
                }
                WorldShape::Capsule { points, radius } => {
                    w.push(Prim::Capsule { points, radius }, source, b)?
                }
            }
        }
        w.rebuild(DEFAULT_GRID_CELL, b)?;
        Ok(w)
    }

    fn add_rule(&mut self, r: &'static str, b: &Budget) -> Result<()> {
        if let Err(at) = self.rules.binary_search(&r) {
            grow(&mut self.rules, 1, b)?;
            self.rules.insert(at, r);
        }
        Ok(())
    }

    fn add_source(&mut self, s: Source, b: &Budget) -> Result<u32> {
        let index = u32::try_from(self.sources.len())
            .map_err(|_| Error::new("LimitExceeded", FILE, 0, "u32 sources", "more"))?;
        grow(&mut self.sources, 1, b)?;
        b.charge(s.label.as_ref().map_or(0, |l| l.len() as u64), FILE, 0)?;
        self.sources.push(s);
        Ok(index)
    }

    fn push_triangle(&mut self, t: [V3; 3], source: u32) {
        if !t.iter().all(|p| g::finite(*p)) {
            self.stats.non_finite_left_out += 1;
            return;
        }
        if g::triangle_normal(t).is_none() {
            self.stats.degenerate_triangles += 1;
        }
        self.stats.triangles += 1;
        if let Some(bounds) = Aabb::of(&t, 0.0) {
            // Capacity was reserved by the caller.
            self.entries.push(Entry {
                prim: Prim::Triangle(t),
                bounds,
                source,
            });
        }
    }

    fn push(&mut self, prim: Prim, source: u32, b: &Budget) -> Result<()> {
        let (bounds, ok) = match prim {
            Prim::Sphere { center, radius } => (
                Aabb::of(&[center], radius),
                g::finite(center) && radius.is_finite() && radius >= 0.0,
            ),
            Prim::Capsule { points, radius } => (
                Aabb::of(&points, radius),
                points.iter().all(|p| g::finite(*p)) && radius.is_finite() && radius >= 0.0,
            ),
            Prim::Triangle(t) => (Aabb::of(&t, 0.0), t.iter().all(|p| g::finite(*p))),
            Prim::Hull { .. } => (None, false),
        };
        let Some(bounds) = bounds.filter(|_| ok) else {
            self.stats.non_finite_left_out += 1;
            return Ok(());
        };
        match prim {
            Prim::Sphere { .. } => self.stats.spheres += 1,
            Prim::Capsule { .. } => self.stats.capsules += 1,
            _ => {}
        }
        grow(&mut self.entries, 1, b)?;
        self.entries.push(Entry {
            prim,
            bounds,
            source,
        });
        Ok(())
    }

    fn push_hull(&mut self, vertices: &[V3], radius: f64, source: u32, b: &Budget) -> Result<()> {
        if vertices.is_empty()
            || !vertices.iter().all(|p| g::finite(*p))
            || !radius.is_finite()
            || radius < 0.0
        {
            self.stats.non_finite_left_out += 1;
            return Ok(());
        }
        let too_many = || Error::new("LimitExceeded", FILE, 0, "u32 hull points", "more");
        let start = u32::try_from(self.points.len()).map_err(|_| too_many())?;
        let count = u32::try_from(vertices.len()).map_err(|_| too_many())?;
        start.checked_add(count).ok_or_else(too_many)?;
        grow(&mut self.points, vertices.len(), b)?;
        self.points.extend_from_slice(vertices);
        let bounds = Aabb::of(vertices, radius).ok_or_else(too_many)?;
        grow(&mut self.entries, 1, b)?;
        self.entries.push(Entry {
            prim: Prim::Hull {
                start,
                count,
                radius,
            },
            bounds,
            source,
        });
        self.stats.hulls += 1;
        self.stats.hull_points += vertices.len() as u64;
        Ok(())
    }

    /// Adds a set of world-space triangles (for example terrain) under one
    /// labelled source and rebuilds the broad phase. Returns the source.
    pub fn add_triangles(&mut self, triangles: &[[V3; 3]], label: &str, b: &Budget) -> Result<u32> {
        self.add_rule(rules::TRIANGLE_SIDES, b)?;
        let source = self.add_source(
            Source {
                instance: None,
                class: None,
                label: Some(label.to_owned()),
                terrain: None,
            },
            b,
        )?;
        grow(&mut self.entries, triangles.len(), b)?;
        for t in triangles {
            self.push_triangle(*t, source);
        }
        self.rebuild(self.cell_or_default(), b)?;
        Ok(source)
    }

    /// Adds the 2,048 triangles of a cell's terrain under one source
    /// labelled `terrain` with its grid, takes the terrain's rules
    /// (`spec/land.md`) into [`World::rules`] and rebuilds the broad phase.
    /// Returns the source.
    pub fn add_terrain(&mut self, t: &Terrain, b: &Budget) -> Result<u32> {
        self.add_rule(rules::TRIANGLE_SIDES, b)?;
        for r in &t.rules {
            self.add_rule(r, b)?;
        }
        let source = self.add_source(
            Source {
                instance: None,
                class: None,
                label: Some("terrain".to_owned()),
                terrain: Some(t.grid),
            },
            b,
        )?;
        let n = t.triangle_indices().count();
        grow(&mut self.entries, n, b)?;
        for tri in t.world_triangles() {
            self.push_triangle(tri, source);
        }
        self.rebuild(self.cell_or_default(), b)?;
        Ok(source)
    }

    /// Adds one sphere, capsule or hull under a labelled source (tests and
    /// tools) and rebuilds the broad phase.
    pub fn add_sphere(&mut self, center: V3, radius: f64, label: &str, b: &Budget) -> Result<u32> {
        let s = self.add_labelled(label, b)?;
        self.push(Prim::Sphere { center, radius }, s, b)?;
        self.rebuild(self.cell_or_default(), b)?;
        Ok(s)
    }
    pub fn add_capsule(
        &mut self,
        points: [V3; 2],
        radius: f64,
        label: &str,
        b: &Budget,
    ) -> Result<u32> {
        let s = self.add_labelled(label, b)?;
        self.push(Prim::Capsule { points, radius }, s, b)?;
        self.rebuild(self.cell_or_default(), b)?;
        Ok(s)
    }
    pub fn add_hull(
        &mut self,
        vertices: &[V3],
        radius: f64,
        label: &str,
        b: &Budget,
    ) -> Result<u32> {
        let s = self.add_labelled(label, b)?;
        self.push_hull(vertices, radius, s, b)?;
        self.rebuild(self.cell_or_default(), b)?;
        Ok(s)
    }
    fn add_labelled(&mut self, label: &str, b: &Budget) -> Result<u32> {
        self.add_source(
            Source {
                instance: None,
                class: None,
                label: Some(label.to_owned()),
                terrain: None,
            },
            b,
        )
    }
    fn cell_or_default(&self) -> f64 {
        if self.grid.cell > 0.0 {
            self.grid.cell
        } else {
            DEFAULT_GRID_CELL
        }
    }

    /// Rebuilds the broad phase with columns of `cell` world units (grown
    /// so that there are at most [`MAX_GRID_COLUMNS`]).
    pub fn rebuild(&mut self, cell: f64, b: &Budget) -> Result<()> {
        if !(cell.is_finite() && cell > 0.0) {
            return Err(Error::new(
                "InvalidGridCell",
                FILE,
                0,
                "positive finite",
                cell,
            ));
        }
        self.bounds = self.entries.iter().map(|e| e.bounds).reduce(Aabb::union);
        let Some(bx) = self.bounds else {
            self.grid = Grid {
                cell,
                ..Grid::default()
            };
            return Ok(());
        };
        let mut cell = cell;
        let columns = |cell: f64| -> [u64; 2] {
            // A large finite extent saturates the cast; the loop below
            // then grows the cell until the columns fit.
            std::array::from_fn(|k| {
                (((bx.max[k] - bx.min[k]) / cell).floor() as u64).saturating_add(1)
            })
        };
        while columns(cell)[0].saturating_mul(columns(cell)[1]) > MAX_GRID_COLUMNS {
            cell *= 2.0;
        }
        let n = columns(cell).map(|x| x as usize);
        let total = n[0] * n[1];
        let mut grid = Grid {
            origin: [bx.min[0], bx.min[1]],
            cell,
            n,
            starts: Vec::new(),
            items: Vec::new(),
        };
        grow(&mut grid.starts, total + 1, b)?;
        grid.starts.resize(total + 1, 0);
        let mut entries: u64 = 0;
        for e in &self.entries {
            let ([x0, y0], [x1, y1]) = grid.span(&e.bounds);
            let c = ((x1 - x0 + 1) * (y1 - y0 + 1)) as u64;
            entries = fo4_core::add(entries, c, FILE, 0)?;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.starts[y * n[0] + x + 1] += 1;
                }
            }
        }
        let entries_usize = usize::try_from(entries)
            .map_err(|_| Error::new("LimitExceeded", FILE, 0, "usize grid entries", entries))?;
        u32::try_from(entries)
            .map_err(|_| Error::new("LimitExceeded", FILE, 0, "u32 grid entries", entries))?;
        for i in 0..total {
            grid.starts[i + 1] += grid.starts[i];
        }
        grow(&mut grid.items, entries_usize, b)?;
        grid.items.resize(entries_usize, 0);
        let mut fill: Vec<u32> = Vec::new();
        let work = b.child();
        grow(&mut fill, total, &work)?;
        fill.extend_from_slice(&grid.starts[..total]);
        for (i, e) in self.entries.iter().enumerate() {
            let ([x0, y0], [x1, y1]) = grid.span(&e.bounds);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = y * n[0] + x;
                    grid.items[fill[c] as usize] = i as u32;
                    fill[c] += 1;
                }
            }
        }
        drop(fill);
        drop(work);
        self.stats.grid_columns = [n[0] as u64, n[1] as u64];
        self.stats.grid_cell = cell;
        self.stats.grid_entries = entries;
        self.grid = grid;
        Ok(())
    }

    /// A world of the primitives of `sources` moved by `delta` (world
    /// units), each source kept with its [`Source`] and numbered in the
    /// order given, with this world's rules and the same column size: the
    /// collision of a placed reference the run moved
    /// (`spec/play.md`, The live world). A source out of range is refused.
    pub fn copy_sources(&self, sources: &[u32], delta: V3, b: &Budget) -> Result<World> {
        self.copy_with(sources, &|p| g::add(p, delta), b)
    }

    /// A world of the primitives of `sources` moved by the rigid motion
    /// `motion` (`walk_platform_rigid_motion_v1`): every point of a
    /// triangle, sphere, capsule or hull goes where [`Motion::apply`] puts
    /// it, radii kept; otherwise as [`World::copy_sources`].
    pub fn copy_sources_moved(
        &self,
        sources: &[u32],
        motion: &Motion,
        b: &Budget,
    ) -> Result<World> {
        self.copy_with(sources, &|p| motion.apply(p), b)
    }

    /// The primitives of `sources`, each point taken through `at`.
    fn copy_with(&self, sources: &[u32], at: &dyn Fn(V3) -> V3, b: &Budget) -> Result<World> {
        let mut w = World::new();
        for r in &self.rules {
            w.add_rule(r, b)?;
        }
        let mut map: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
        for s in sources {
            let src = self
                .sources
                .get(*s as usize)
                .ok_or_else(|| Error::new("UnknownSource", FILE, 0, "a source of the world", *s))?;
            if map.contains_key(s) {
                continue;
            }
            if src.instance.is_some() {
                w.stats.instances += 1;
            }
            let n = w.add_source(src.clone(), b)?;
            map.insert(*s, n);
        }
        w.push_from(self, &map, at, b)?;
        w.rebuild(self.cell_or_default(), b)?;
        Ok(w)
    }

    /// Adds every source of `other` with its primitives as stored, each as
    /// a new source of this world labelled `label` (its shape class kept,
    /// no instance of this world's cell), takes `other`'s rules and
    /// rebuilds the broad phase. Returns the new sources in `other`'s
    /// order.
    pub fn append(&mut self, other: &World, label: &str, b: &Budget) -> Result<Vec<u32>> {
        for r in &other.rules {
            self.add_rule(r, b)?;
        }
        let mut map: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
        let mut out = Vec::new();
        for (k, src) in other.sources.iter().enumerate() {
            let n = self.add_source(
                Source {
                    instance: None,
                    class: src.class,
                    label: Some(label.to_owned()),
                    terrain: None,
                },
                b,
            )?;
            map.insert(k as u32, n);
            out.push(n);
        }
        self.push_from(other, &map, &|p| p, b)?;
        self.rebuild(self.cell_or_default(), b)?;
        Ok(out)
    }

    /// The primitives of `other`'s sources that `map` names, each point
    /// taken through `at`, under the mapped sources (no rebuild).
    fn push_from(
        &mut self,
        other: &World,
        map: &std::collections::BTreeMap<u32, u32>,
        at: &dyn Fn(V3) -> V3,
        b: &Budget,
    ) -> Result<()> {
        for e in &other.entries {
            let Some(&source) = map.get(&e.source) else {
                continue;
            };
            match e.prim {
                Prim::Triangle(t) => {
                    grow(&mut self.entries, 1, b)?;
                    self.push_triangle(t.map(at), source);
                }
                Prim::Sphere { center, radius } => self.push(
                    Prim::Sphere {
                        center: at(center),
                        radius,
                    },
                    source,
                    b,
                )?,
                Prim::Capsule { points, radius } => self.push(
                    Prim::Capsule {
                        points: points.map(at),
                        radius,
                    },
                    source,
                    b,
                )?,
                Prim::Hull {
                    start,
                    count,
                    radius,
                } => {
                    let pts: Vec<V3> = other
                        .hull_points(start, count)
                        .iter()
                        .map(|p| at(*p))
                        .collect();
                    self.push_hull(&pts, radius, source, b)?;
                }
            }
        }
        Ok(())
    }

    /// Points of a hull primitive.
    pub fn hull_points(&self, start: u32, count: u32) -> &[V3] {
        self.points
            .get(start as usize..(start as usize + count as usize))
            .unwrap_or(&[])
    }

    /// Core points and radius of a primitive.
    fn core(&self, p: &Prim) -> (Core<'_>, f64) {
        match p {
            Prim::Triangle(t) => (Core::Three(*t), 0.0),
            Prim::Sphere { center, radius } => (Core::One([*center]), *radius),
            Prim::Capsule { points, radius } => (Core::Two(*points), *radius),
            Prim::Hull {
                start,
                count,
                radius,
            } => (Core::Many(self.hull_points(*start, *count)), *radius),
        }
    }

    /// Entries whose box meets `query`, in increasing order, into `out`.
    pub fn candidates(&self, query: &Aabb, out: &mut Vec<u32>) {
        out.clear();
        let Some(bx) = self.bounds else { return };
        if !bx.overlaps(query) {
            return;
        }
        let ([x0, y0], [x1, y1]) = self.grid.span(query);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = y * self.grid.n[0] + x;
                let (s, e) = (
                    self.grid.starts[c] as usize,
                    self.grid.starts[c + 1] as usize,
                );
                for i in &self.grid.items[s..e] {
                    if self.entries[*i as usize].bounds.overlaps(query) {
                        out.push(*i);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Every primitive the capsule `segment` widened by `radius + margin`
    /// touches, with its depth (`radius - distance`, negative within the
    /// margin) and the normal towards the capsule.
    pub fn contacts(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        scratch: &mut Vec<u32>,
        out: &mut Vec<Contact>,
    ) {
        out.clear();
        self.push_contacts(segment, radius, margin, scratch, 0, out);
    }

    /// [`World::contacts`] appended to `out` without clearing it, each
    /// entry index raised by `base` (a [`crate::WorldSet`] numbers the
    /// entries of its worlds one after another).
    pub fn push_contacts(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        scratch: &mut Vec<u32>,
        base: u32,
        out: &mut Vec<Contact>,
    ) {
        let Some(q) = Aabb::of(&segment, radius + margin) else {
            return;
        };
        self.candidates(&q, scratch);
        for i in scratch.iter() {
            let e = &self.entries[*i as usize];
            let (core, r) = self.core(&e.prim);
            let c = g::closest(&segment, core.points());
            let reach = radius + r;
            if c.distance >= reach + margin {
                continue;
            }
            let normal = if c.distance > 1e-9 {
                g::scale(g::sub(c.on_a, c.on_b), 1.0 / c.distance)
            } else {
                self.fallback_normal(&e.prim, segment)
            };
            out.push(Contact {
                entry: base + *i,
                depth: reach - c.distance,
                normal,
            });
        }
    }

    /// The highest point at which a primitive too steep to stand on
    /// touches the capsule `segment` widened by `radius + margin`, among
    /// the primitives whose source `keep` keeps: the core is sampled from
    /// bottom to top at most [`TOUCH_STEP`] apart, each sample a sphere of
    /// that radius; where a primitive reaches a sample, the point of the
    /// primitive's surface nearest to it counts when the direction from
    /// that point to the sample is steeper than `walkable_cos` (its z below
    /// it). Unlike one closest pair of the whole capsule, which for a wall
    /// beside the capsule's core is any point along it, this finds how high
    /// the shapes that hold a character reach.
    pub fn highest_steep_touch(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        walkable_cos: f64,
        keep: &dyn Fn(u32) -> bool,
        scratch: &mut Vec<u32>,
    ) -> Option<f64> {
        let q = Aabb::of(&segment, radius + margin)?;
        self.candidates(&q, scratch);
        let span = g::length(g::sub(segment[1], segment[0]));
        let samples = (span / TOUCH_STEP).ceil().clamp(1.0, 4096.0) as usize + 1;
        let mut best: Option<f64> = None;
        for i in scratch.iter() {
            let e = &self.entries[*i as usize];
            if !keep(e.source) {
                continue;
            }
            let (core, r) = self.core(&e.prim);
            for k in 0..samples {
                let f = k as f64 / (samples - 1) as f64;
                let p = g::add(segment[0], g::scale(g::sub(segment[1], segment[0]), f));
                let c = g::closest(&[p], core.points());
                if c.distance >= radius + r + margin || c.distance <= 1e-9 {
                    continue;
                }
                let n = g::scale(g::sub(p, c.on_b), 1.0 / c.distance);
                if n[2] >= walkable_cos {
                    continue;
                }
                let z = c.on_b[2] + n[2] * r;
                if best.is_none_or(|b| z > b) {
                    best = Some(z);
                }
            }
        }
        best
    }

    /// Normal for a capsule whose core touches the primitive's core: the
    /// triangle's normal turned to face up (or towards the capsule), else the
    /// direction from the primitive's centre to the capsule, else up.
    fn fallback_normal(&self, p: &Prim, segment: [V3; 2]) -> V3 {
        let mid = g::scale(g::add(segment[0], segment[1]), 0.5);
        match p {
            Prim::Triangle(t) => match g::triangle_normal(*t) {
                Some(n) => {
                    let side = g::dot(g::sub(mid, t[0]), n);
                    if side > 1e-9 || (side.abs() <= 1e-9 && n[2] >= 0.0) {
                        n
                    } else {
                        g::scale(n, -1.0)
                    }
                }
                None => [0.0, 0.0, 1.0],
            },
            _ => {
                let (core, _) = self.core(p);
                let pts = core.points();
                let centre = g::scale(
                    pts.iter().fold([0.0; 3], |a, x| g::add(a, *x)),
                    1.0 / pts.len().max(1) as f64,
                );
                g::unit(g::sub(mid, centre)).unwrap_or([0.0, 0.0, 1.0])
            }
        }
    }

    /// The highest surface at or below `z_from` on the vertical line through
    /// `(x, y)`: tops of triangles (either winding), spheres, capsules and
    /// widened hulls. Triangles seen exactly edge-on are not hit.
    pub fn ray_down(&self, x: f64, y: f64, z_from: f64) -> Option<RayHit> {
        self.ray_down_where(x, y, z_from, &|_| true)
    }

    /// [`World::ray_down`] over the primitives whose source `keep` keeps.
    pub fn ray_down_where(
        &self,
        x: f64,
        y: f64,
        z_from: f64,
        keep: &dyn Fn(u32) -> bool,
    ) -> Option<RayHit> {
        let q = Aabb {
            min: [x, y, f64::NEG_INFINITY],
            max: [x, y, z_from],
        };
        let mut cand = Vec::new();
        self.candidates(&q, &mut cand);
        let mut best: Option<RayHit> = None;
        for i in cand {
            let e = &self.entries[i as usize];
            if !keep(e.source) {
                continue;
            }
            if let Some((z, normal)) = self.top_on_line(e, x, y) {
                if z <= z_from + 1e-9 && best.is_none_or(|b| z > b.z) {
                    best = Some(RayHit {
                        z,
                        normal,
                        entry: i,
                        source: e.source,
                    });
                }
            }
        }
        best
    }

    /// Every surface on the vertical line through `(x, y)`, highest first:
    /// for each primitive the line meets, the surface [`World::ray_down`]
    /// would find on it (a triangle's plane, the top of a convex primitive),
    /// with no starting height. Equal heights keep the entry order.
    pub fn surfaces(&self, x: f64, y: f64) -> Vec<RayHit> {
        let q = Aabb {
            min: [x, y, f64::NEG_INFINITY],
            max: [x, y, f64::INFINITY],
        };
        let mut cand = Vec::new();
        self.candidates(&q, &mut cand);
        let mut out: Vec<RayHit> = cand
            .into_iter()
            .filter_map(|i| {
                let e = &self.entries[i as usize];
                self.top_on_line(e, x, y).map(|(z, normal)| RayHit {
                    z,
                    normal,
                    entry: i,
                    source: e.source,
                })
            })
            .collect();
        out.sort_by(|a, b| b.z.total_cmp(&a.z).then(a.entry.cmp(&b.entry)));
        out
    }

    /// The surface of one primitive on the vertical line through `(x, y)`.
    fn top_on_line(&self, e: &Entry, x: f64, y: f64) -> Option<(f64, V3)> {
        match &e.prim {
            Prim::Triangle(t) => top_of_triangle(*t, x, y),
            _ => {
                let (core, r) = self.core(&e.prim);
                top_of_convex(core.points(), r, x, y, e.bounds.min[2], e.bounds.max[2])
            }
        }
    }
}

impl Grid {
    /// Inclusive column range covered by a box, clamped to the grid.
    fn span(&self, b: &Aabb) -> ([usize; 2], [usize; 2]) {
        let col = |v: f64, k: usize| -> usize {
            let c = ((v - self.origin[k]) / self.cell).floor();
            if c.is_nan() || c < 0.0 {
                0
            } else {
                (c as usize).min(self.n[k].saturating_sub(1))
            }
        };
        (
            [col(b.min[0], 0), col(b.min[1], 1)],
            [col(b.max[0], 0), col(b.max[1], 1)],
        )
    }
}

enum Core<'a> {
    One([V3; 1]),
    Two([V3; 2]),
    Three([V3; 3]),
    Many(&'a [V3]),
}
impl Core<'_> {
    fn points(&self) -> &[V3] {
        match self {
            Core::One(p) => p,
            Core::Two(p) => p,
            Core::Three(p) => p,
            Core::Many(p) => p,
        }
    }
}

/// z of a triangle's plane at (x, y) when the point lies over it, with the
/// normal turned to face up.
fn top_of_triangle([a, b, c]: [V3; 3], x: f64, y: f64) -> Option<(f64, V3)> {
    let n = g::triangle_normal([a, b, c])?;
    if n[2].abs() < 1e-9 {
        return None;
    }
    let d = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    let u = ((x - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (y - a[1])) / d;
    let v = ((b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1])) / d;
    let eps = 1e-9;
    if u < -eps || v < -eps || u + v > 1.0 + eps {
        return None;
    }
    let z = a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]);
    let up = if n[2] < 0.0 { g::scale(n, -1.0) } else { n };
    Some((z, up))
}

/// Distance from `p` to the hull of `pts`, minus `r`.
fn excess(pts: &[V3], r: f64, p: V3) -> f64 {
    g::closest(&[p], pts).distance - r
}

/// Highest z on the vertical line through (x, y) inside the hull of `pts`
/// widened by `r`, searched in [zlo, zhi]; the inside of a convex set along
/// a line is one interval and the distance along it is convex.
fn top_of_convex(pts: &[V3], r: f64, x: f64, y: f64, zlo: f64, zhi: f64) -> Option<(f64, V3)> {
    let at = |z: f64| excess(pts, r, [x, y, z]);
    let (mut lo, mut hi) = (zlo, zhi);
    for _ in 0..100 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if at(m1) <= at(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let inner = 0.5 * (lo + hi);
    let tol = 1e-7 * (1.0 + r);
    if at(inner) > tol {
        return None;
    }
    let (mut lo, mut hi) = (inner, zhi);
    for _ in 0..100 {
        let m = 0.5 * (lo + hi);
        if at(m) <= tol {
            lo = m;
        } else {
            hi = m;
        }
    }
    let z = lo;
    let p = [x, y, z];
    let c = g::closest(&[p], pts);
    let normal = if r > 0.0 && c.distance > 1e-9 {
        g::unit(g::sub(p, c.on_b))
    } else {
        None
    };
    Some((z, normal.unwrap_or([0.0, 0.0, 1.0])))
}
