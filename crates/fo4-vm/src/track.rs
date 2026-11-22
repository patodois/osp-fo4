//! A first world model of the player's position for the quest host
//! (`spec/vm.md`, Player position).
//!
//! The caller gives the player's movement as a track: points of (second
//! on the real-time clock, cell or worldspace, position, optional facing),
//! with the player moving in a straight line at constant speed between two
//! points of one space and reaching a point in another space at its second
//! (a caller assumption, never game behaviour). From it and the plugins'
//! own geometry the host derives what the caller used to assume by hand:
//!
//! - trigger enter and leave events, from the primitive (`XPRM`) of placed
//!   activator references and the reference's placement transform, each
//!   reading a pending rule behind its own switch (`world_trigger_*`);
//! - distance events registered by `RegisterForDistanceLessThanEvent` and
//!   `RegisterForDistanceGreaterThanEvent` (`world_distance_*`);
//! - the answers of position natives (`world_position_natives_v1`,
//!   `world_cell_natives_v1`, `world_location_native_v1`) from the track
//!   and the placed references' stored positions
//!   (`world_placed_position_v1`);
//! - line of sight only as a caller assumption (`--assume-los`): no
//!   geometry test is made.
//!
//! The `XPRM` layout (32 bytes: three bound words, three colour words, one
//! more float, a kind word) is measured over every placed reference
//! (`world_census`, `spec/world-census.txt`); what the bounds mean for an
//! event is the rules' business.
use crate::{
    bind,
    data::{key_record, record_object},
    host::{NativeCall, Reply},
    player, quest, rules,
    world::{Clock, InstId, ObjId, Val, World},
    Outcome, Vm,
};
use fo4_core::Budget;
use fo4_plugin::{
    typed::{Cell, Refr, Store},
    FormKey, Record,
};
use fo4_scene::transform::Transform;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// The side of an exterior cell's grid square in world units, as the
/// placed references of exterior cells measure it (`world_census`).
pub const CELL_SIZE: f64 = 4096.0;
/// The primitive kind word of a box (`world_trigger_box_v1`).
pub const KIND_BOX: u32 = 1;
/// The primitive kind word of a sphere (`world_trigger_sphere_v1`).
pub const KIND_SPHERE: u32 = 2;
/// Rounds of raising and running jobs at one instant before
/// [`settle`] gives up (counted).
pub const MAX_ROUNDS: usize = 64;

/// One `XPRM` as stored: words 0 to 2 the bounds, 3 to 5 a colour, 6 a
/// float, 7 the kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primitive {
    pub bounds: [f32; 3],
    pub color: [f32; 3],
    pub word6: f32,
    pub kind: u32,
}
impl Primitive {
    /// Reads the 32 bytes; `None` for another size.
    pub fn read(d: &[u8]) -> Option<Primitive> {
        let (words, rest) = d.as_chunks::<4>();
        if words.len() != 8 || !rest.is_empty() {
            return None;
        }
        let f = |i: usize| f32::from_le_bytes(words[i]);
        Some(Primitive {
            bounds: [f(0), f(1), f(2)],
            color: [f(3), f(4), f(5)],
            word6: f(6),
            kind: u32::from_le_bytes(words[7]),
        })
    }
}

/// The record's one `XPRM`: `Ok(None)` without one.
pub fn primitive_of(r: &Record) -> Result<Option<Primitive>, &'static str> {
    let mut it = r.subrecords.iter().filter(|s| s.signature == *b"XPRM");
    match (it.next(), it.next()) {
        (None, _) => Ok(None),
        (Some(s), None) => Primitive::read(r.data(s))
            .map(Some)
            .ok_or("an XPRM is not 32 bytes"),
        _ => Err("two XPRM subrecords"),
    }
}

/// A volume's shape in its reference's frame (before the scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Half extents along the local axes (`world_trigger_box_v1`).
    Box([f64; 3]),
    /// Radius (`world_trigger_sphere_v1`).
    Sphere(f64),
}

/// The switches of this model, each off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// `--trigger-box` (`world_trigger_box_v1`).
    pub boxes: bool,
    /// `--trigger-sphere` (`world_trigger_sphere_v1`).
    pub spheres: bool,
    /// `--trigger-point` (`world_trigger_point_v1`).
    pub point: bool,
    /// `--trigger-body HEIGHT` (`world_trigger_body_v1`): the player is a
    /// vertical segment of that many units up from the reference point;
    /// the alternative to `point`.
    pub body: Option<u32>,
    /// `--trigger-instant` (`world_trigger_instant_v1`).
    pub instant: bool,
    /// `--trigger-enabled` (`world_trigger_enabled_v1`).
    pub enabled: bool,
    /// `--trigger-order` (`world_trigger_order_v1`).
    pub order: bool,
    /// `--distance-events` (`world_distance_events_v1`).
    pub distance: bool,
    /// `--distance-once` (`world_distance_once_v1`).
    pub distance_once: bool,
    /// `--placed-positions` (`world_placed_position_v1`).
    pub placed: bool,
    /// `--position-natives` (`world_position_natives_v1`).
    pub position_natives: bool,
    /// `--cell-natives` (`world_cell_natives_v1`).
    pub cell_natives: bool,
    /// `--location-native` (`world_location_native_v1`).
    pub location_native: bool,
    /// `--assume-los true|false`: a caller assumption, every
    /// `HasDirectLOS` answers this value.
    pub assume_los: Option<bool>,
}
impl Switches {
    /// Whether a shape of this kind is read (its switch on).
    pub fn shape(&self, p: &Primitive, scale: f64) -> Result<Shape, &'static str> {
        let b = p.bounds.map(f64::from);
        if b.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err("bounds that are not finite and non-negative");
        }
        match p.kind {
            KIND_BOX if self.boxes => Ok(Shape::Box(b.map(|x| x * scale))),
            KIND_SPHERE if self.spheres => Ok(Shape::Sphere(b[0] * scale)),
            KIND_BOX => Err("a box (--trigger-box off)"),
            KIND_SPHERE => Err("a sphere (--trigger-sphere off)"),
            _ => Err("a kind no rule reads"),
        }
    }
}

/// One point of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Second of the real-time clock.
    pub t: f64,
    /// The cell (interior) or worldspace the point is in.
    pub space: FormKey,
    pub pos: [f64; 3],
    /// Facing about the vertical axis in degrees, when given.
    pub facing: Option<f64>,
}

/// One straight move of a track: (start second, end second, space, from,
/// to).
type Move<'a> = (f64, f64, &'a FormKey, [f64; 3], [f64; 3]);

/// A placed reference's stored space, position and facing, or why there
/// is none.
type Stored = Result<(FormKey, [f64; 3], f64), &'static str>;

/// The caller's track: points in non-decreasing time order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub points: Vec<Point>,
}
impl Track {
    /// Parses `SECONDS SPACE X Y Z [FACING]` lines (`#` starts a comment,
    /// blank lines are skipped), the space a record key as
    /// `plugin.esm:0001ABCD`.
    pub fn parse(text: &str) -> Result<Track, String> {
        let mut points: Vec<Point> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| format!("track line {}: {why}", n + 1);
            let w: Vec<&str> = line.split_whitespace().collect();
            if !(5..=6).contains(&w.len()) {
                return Err(bad("expected SECONDS SPACE X Y Z [FACING]"));
            }
            let num = |s: &str| -> Result<f64, String> {
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| bad("a number that is not finite"))
            };
            let t = num(w[0])?;
            let space = player::parse_key(w[1]).ok_or_else(|| bad("a space as plugin:objectid"))?;
            let pos = [num(w[2])?, num(w[3])?, num(w[4])?];
            let facing = match w.get(5) {
                Some(s) => Some(num(s)?),
                None => None,
            };
            if points.last().is_some_and(|p| p.t > t) {
                return Err(bad("time goes back"));
            }
            points.push(Point {
                t,
                space,
                pos,
                facing,
            });
        }
        if points.is_empty() {
            return Err("an empty track".into());
        }
        Ok(Track { points })
    }
    /// Replaces an exterior cell named as a space by its worldspace.
    pub fn resolve(&mut self, store: &Store) -> Result<(), String> {
        for p in &mut self.points {
            match store.signature(&p.space).as_ref().map(|s| &s[..]) {
                Some(b"WRLD") => {}
                Some(b"CELL") => {
                    if let Some(w) = store.world(&p.space) {
                        p.space = w;
                    }
                }
                _ => {
                    return Err(format!(
                        "{} names no winning CELL or WRLD",
                        bind::record_key(&p.space)
                    ))
                }
            }
        }
        Ok(())
    }
    /// Where the player is at second `t`: `None` before the first point.
    pub fn at(&self, t: f64) -> Option<(&FormKey, [f64; 3], Option<f64>)> {
        let first = self.points.first()?;
        if t < first.t {
            return None;
        }
        for w in self.points.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t < b.t {
                if a.space != b.space || b.t <= a.t {
                    return Some((&a.space, a.pos, a.facing));
                }
                let s = (t - a.t) / (b.t - a.t);
                let pos = std::array::from_fn(|i| a.pos[i] + s * (b.pos[i] - a.pos[i]));
                return Some((&a.space, pos, a.facing));
            }
        }
        let last = self.points.last()?;
        Some((&last.space, last.pos, last.facing))
    }
    /// The track as moves: (start second, end second, space, from, to); a
    /// move to a point in another space is a stay at the earlier point
    /// until the later point's second; after the last point a stay with
    /// no end.
    fn moves(&self) -> Vec<Move<'_>> {
        let mut out = Vec::new();
        for w in self.points.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if a.space == b.space {
                out.push((a.t, b.t, &a.space, a.pos, b.pos));
            } else {
                out.push((a.t, b.t, &a.space, a.pos, a.pos));
            }
        }
        if let Some(l) = self.points.last() {
            out.push((l.t, f64::INFINITY, &l.space, l.pos, l.pos));
        }
        out
    }
}

/// A placed reference's volume.
#[derive(Debug, Clone)]
pub struct Volume {
    pub key: FormKey,
    pub space: FormKey,
    pub frame: Transform,
    pub shape: Shape,
}
impl Volume {
    /// A world point in the reference's frame, before its scale (the
    /// scale is in the shape): `R^T (p - t) / s` inverted from
    /// [`Transform::apply`].
    fn local(&self, p: [f64; 3]) -> [f64; 3] {
        let f = &self.frame;
        let d: [f64; 3] = std::array::from_fn(|i| p[i] - f.translation[i]);
        std::array::from_fn(|j| (0..3).map(|i| f.rotation[j][i] * d[i]).sum::<f64>())
    }
    /// Whether the closed volume holds a world point.
    pub fn contains(&self, p: [f64; 3]) -> bool {
        let l = self.local(p);
        match self.shape {
            Shape::Box(h) => (0..3).all(|i| l[i].abs() <= h[i]),
            Shape::Sphere(r) => l.iter().map(|x| x * x).sum::<f64>() <= r * r,
        }
    }
    /// The part of the segment `a`→`b` (parameter 0 to 1) inside the
    /// closed volume.
    pub fn span(&self, a: [f64; 3], b: [f64; 3]) -> Option<(f64, f64)> {
        let la = self.local(a);
        let lb = self.local(b);
        let d: [f64; 3] = std::array::from_fn(|i| lb[i] - la[i]);
        match self.shape {
            Shape::Box(h) => {
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for i in 0..3 {
                    if d[i] == 0.0 {
                        if la[i].abs() > h[i] {
                            return None;
                        }
                        continue;
                    }
                    let t0 = (-h[i] - la[i]) / d[i];
                    let t1 = (h[i] - la[i]) / d[i];
                    lo = lo.max(t0.min(t1));
                    hi = hi.min(t0.max(t1));
                    if lo > hi {
                        return None;
                    }
                }
                Some((lo, hi))
            }
            Shape::Sphere(r) => sphere_span(la, d, r),
        }
    }
}

impl Volume {
    /// The part of the move `a`→`b` (parameter 0 to 1) during which the
    /// vertical segment from the moving point up `h` units meets the
    /// closed volume (`world_trigger_body_v1`). A box: the point lies in
    /// the box swept down by the segment, a zonotope clipped by its face
    /// planes; a sphere: the point's distance to the segment swept down is
    /// convex along the move, its interval found by search.
    pub fn span_body(&self, a: [f64; 3], b: [f64; 3], h: f64) -> Option<(f64, f64)> {
        let la = self.local(a);
        let lb = self.local(b);
        let d: [f64; 3] = std::array::from_fn(|i| lb[i] - la[i]);
        let r = &self.frame.rotation;
        // The world's up direction in the local frame.
        let up: [f64; 3] = std::array::from_fn(|j| r[j][2]);
        match self.shape {
            Shape::Box(hx) => {
                let g4: [f64; 3] = up.map(|x| x * h / 2.0);
                let c: [f64; 3] = up.map(|x| -x * h / 2.0);
                let gens = [[hx[0], 0.0, 0.0], [0.0, hx[1], 0.0], [0.0, 0.0, hx[2]], g4];
                let cross = |u: [f64; 3], v: [f64; 3]| {
                    [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ]
                };
                let dot = |u: [f64; 3], v: [f64; 3]| (0..3).map(|i| u[i] * v[i]).sum::<f64>();
                let mut normals: Vec<[f64; 3]> =
                    vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
                for g in gens.iter().take(3) {
                    let n = cross(*g, g4);
                    if dot(n, n) > 1e-18 {
                        normals.push(n);
                    }
                }
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for n in normals {
                    let w: f64 = gens.iter().map(|g| dot(n, *g).abs()).sum();
                    let p0 = dot(n, la) - dot(n, c);
                    let dn = dot(n, d);
                    if dn.abs() < 1e-12 {
                        if p0.abs() > w {
                            return None;
                        }
                        continue;
                    }
                    let t0 = (-w - p0) / dn;
                    let t1 = (w - p0) / dn;
                    lo = lo.max(t0.min(t1));
                    hi = hi.min(t0.max(t1));
                    if lo > hi {
                        return None;
                    }
                }
                Some((lo, hi))
            }
            Shape::Sphere(rad) => {
                // Distance from the moving point to the segment from the
                // centre down `h` (the sphere met by the body).
                let f = |s: f64| {
                    let q: [f64; 3] = std::array::from_fn(|i| la[i] + s * d[i]);
                    let u = (-(0..3).map(|i| q[i] * up[i]).sum::<f64>()).clamp(0.0, h);
                    (0..3)
                        .map(|i| (q[i] + up[i] * u).powi(2))
                        .sum::<f64>()
                        .sqrt()
                };
                let (mut x0, mut x1) = (0.0f64, 1.0f64);
                for _ in 0..100 {
                    let m1 = x0 + (x1 - x0) / 3.0;
                    let m2 = x1 - (x1 - x0) / 3.0;
                    if f(m1) <= f(m2) {
                        x1 = m2;
                    } else {
                        x0 = m1;
                    }
                }
                let sm = (x0 + x1) / 2.0;
                if f(sm) > rad {
                    return None;
                }
                let edge = |mut inside: f64, mut outside: f64| {
                    if f(outside) <= rad {
                        return outside;
                    }
                    for _ in 0..100 {
                        let m = (inside + outside) / 2.0;
                        if f(m) <= rad {
                            inside = m;
                        } else {
                            outside = m;
                        }
                    }
                    inside
                };
                Some((edge(sm, 0.0), edge(sm, 1.0)))
            }
        }
    }
}

/// The parameters in [0, 1] where `p + s d` is within `r` of the origin.
fn sphere_span(p: [f64; 3], d: [f64; 3], r: f64) -> Option<(f64, f64)> {
    let a: f64 = d.iter().map(|x| x * x).sum();
    let c: f64 = p.iter().map(|x| x * x).sum::<f64>() - r * r;
    if a == 0.0 {
        return (c <= 0.0).then_some((0.0, 1.0));
    }
    let bq: f64 = 2.0 * (0..3).map(|i| p[i] * d[i]).sum::<f64>();
    let disc = bq * bq - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let lo = ((-bq - sq) / (2.0 * a)).max(0.0);
    let hi = ((-bq + sq) / (2.0 * a)).min(1.0);
    (lo <= hi).then_some((lo, hi))
}

/// The time intervals (closed, by second) a volume holds the player on a
/// track, joined where they touch; intervals of no length are dropped
/// (`world_trigger_point_v1`).
pub fn intervals(track: &Track, v: &Volume, body: Option<u32>) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (t0, t1, space, a, b) in track.moves() {
        if *space != v.space {
            continue;
        }
        let span = match body {
            Some(h) => v.span_body(a, b, f64::from(h)),
            None => v.span(a, b),
        };
        let Some((s0, s1)) = span else {
            continue;
        };
        let (e, l) = if t1.is_infinite() {
            (t0, f64::INFINITY)
        } else {
            (t0 + s0 * (t1 - t0), t0 + s1 * (t1 - t0))
        };
        match out.last_mut() {
            Some(last) if last.1 >= e => last.1 = last.1.max(l),
            _ => out.push((e, l)),
        }
    }
    out.retain(|(e, l)| l > e);
    out
}

/// One change of a volume's geometric hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub t: f64,
    /// Index in [`Place::volumes`].
    pub volume: usize,
    pub enter: bool,
}

/// One distance registration.
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceReg {
    pub instance: InstId,
    pub a: ObjId,
    pub b: ObjId,
    pub threshold: f64,
    /// Less-than (true) or greater-than.
    pub less: bool,
    /// Second it was made.
    pub since: f64,
    /// Raised once already.
    pub fired: bool,
    /// Counted as undetermined already.
    pub counted: bool,
}

/// The model's state in a run's world; off (`on == false`) unless a caller
/// gives a track.
#[derive(Debug, Default)]
pub struct Place {
    pub on: bool,
    pub sw: Switches,
    pub track: Option<Rc<Track>>,
    pub volumes: Vec<Volume>,
    /// Geometric changes, earliest first, then by volume.
    pub crossings: Vec<Crossing>,
    /// Crossings applied so far.
    pub cursor: usize,
    /// Volumes holding the player geometrically.
    pub geo: BTreeSet<usize>,
    /// Volumes whose enter was raised and leave not.
    pub held: BTreeSet<usize>,
    /// Volumes whose hold changed without an event (counted once).
    pub blocked: BTreeSet<usize>,
    pub registrations: Vec<DistanceReg>,
    /// Objects a native of the run may have moved.
    pub moved: BTreeSet<ObjId>,
    /// References a native of the run deleted (`Delete`): they hold no
    /// one (`world_trigger_enabled_v1`).
    pub deleted: BTreeSet<FormKey>,
    /// Counts by text.
    pub counts: BTreeMap<String, u64>,
    /// Rules a native's answer relied on, taken by the host after the
    /// call.
    pub used: BTreeSet<&'static str>,
    /// Stored positions read so far: (space, position, facing in degrees)
    /// or why there is none.
    stored: BTreeMap<FormKey, Stored>,
    /// Exterior cells found by worldspace and grid square.
    grid_cells: BTreeMap<(FormKey, i64, i64), Result<FormKey, &'static str>>,
}
impl Place {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    /// Loads the volumes near the track and their crossings. The placed
    /// references read are those of the track's interior cells and, for a
    /// worldspace, of its cells whose grid square lies within one cell of
    /// the track's points there, and of its persistent cells.
    pub fn load(
        store: &Store,
        mut track: Track,
        sw: Switches,
        b: &Budget,
    ) -> Result<Place, String> {
        track.resolve(store)?;
        let mut place = Place {
            on: true,
            sw,
            ..Place::default()
        };
        // Bounding boxes of the points by space.
        let mut boxes: BTreeMap<FormKey, ([f64; 2], [f64; 2])> = BTreeMap::new();
        for p in &track.points {
            let e = boxes
                .entry(p.space.clone())
                .or_insert(([f64::MAX; 2], [f64::MIN; 2]));
            for i in 0..2 {
                e.0[i] = e.0[i].min(p.pos[i]);
                e.1[i] = e.1[i].max(p.pos[i]);
            }
        }
        let mut cells: Vec<(FormKey, FormKey)> = Vec::new();
        for (space, (lo, hi)) in &boxes {
            if store.signature(space) == Some(*b"CELL") {
                cells.push((space.clone(), space.clone()));
                continue;
            }
            let g0 = lo.map(|x| (x / CELL_SIZE).floor() as i64 - 1);
            let g1 = hi.map(|x| (x / CELL_SIZE).floor() as i64 + 1);
            for c in store.cells() {
                if store.world(&c).as_ref() != Some(space) {
                    continue;
                }
                let cb = b.child();
                let grid = store
                    .get::<Cell>(&c, &cb)
                    .map_err(|e| e.to_string())?
                    .and_then(|x| x.data.grid.map(|g| (i64::from(g.i), i64::from(g.j))));
                let near = grid.is_some_and(|(i, j)| {
                    (g0[0]..=g1[0]).contains(&i) && (g0[1]..=g1[1]).contains(&j)
                });
                let persistent = store
                    .children(&c, &cb)
                    .map_err(|e| e.to_string())?
                    .active
                    .iter()
                    .any(|k| k.group_kind == 8);
                if near || persistent {
                    cells.push((c, space.clone()));
                }
            }
        }
        let margin = CELL_SIZE * 2.0;
        for (cell, space) in &cells {
            let cb = b.child();
            let ch = store.children(cell, &cb).map_err(|e| e.to_string())?;
            for kid in ch.active.iter().chain(&ch.disabled) {
                let rb = b.child();
                let Some(r) = store.raw(&kid.key, &rb).map_err(|e| e.to_string())? else {
                    continue;
                };
                let p = match primitive_of(&r) {
                    Ok(Some(p)) => p,
                    Ok(None) => continue,
                    Err(why) => {
                        place.count(format!("references near the track left out\t{why}"));
                        continue;
                    }
                };
                let base = bind::base_of(store, &kid.key, &rb).ok().flatten();
                if base.and_then(|k| store.signature(&k)) != Some(*b"ACTI") {
                    place.count(format!(
                        "references near the track with a primitive\tnot an activator\tkind {}",
                        p.kind
                    ));
                    continue;
                }
                let Some(refr) = store
                    .get::<Refr>(&kid.key, &rb)
                    .map_err(|e| e.to_string())?
                else {
                    continue;
                };
                let Some(t) = refr.data.transform else {
                    place.count("references near the track left out\tno placement");
                    continue;
                };
                if kid.group_kind == 8 {
                    let (lo, hi) = boxes[space];
                    let out = (0..2).any(|i| {
                        f64::from(t[i]) < lo[i] - margin || f64::from(t[i]) > hi[i] + margin
                    });
                    if out {
                        continue;
                    }
                }
                let scale = refr.data.scale.unwrap_or(1.0);
                let shape = match sw.shape(&p, f64::from(scale)) {
                    Ok(s) => s,
                    Err(why) => {
                        place.count(format!(
                            "activator references near the track not modelled\t{why}"
                        ));
                        continue;
                    }
                };
                let Ok(frame) = Transform::euler(t, 1.0) else {
                    place.count(
                        "references near the track left out\ta placement that is not finite",
                    );
                    continue;
                };
                place.count(format!(
                    "activator references near the track modelled\t{}",
                    match shape {
                        Shape::Box(_) => "box",
                        Shape::Sphere(_) => "sphere",
                    }
                ));
                place.volumes.push(Volume {
                    key: kid.key.clone(),
                    space: space.clone(),
                    frame,
                    shape,
                });
            }
        }
        place.volumes.sort_by(|a, b| {
            (a.key.plugin.as_str(), a.key.object_id).cmp(&(b.key.plugin.as_str(), b.key.object_id))
        });
        for (i, v) in place.volumes.iter().enumerate() {
            for (e, l) in intervals(&track, v, sw.body) {
                place.crossings.push(Crossing {
                    t: e,
                    volume: i,
                    enter: true,
                });
                if l.is_finite() {
                    place.crossings.push(Crossing {
                        t: l,
                        volume: i,
                        enter: false,
                    });
                }
            }
        }
        place.crossings.sort_by(|a, b| {
            a.t.total_cmp(&b.t)
                .then(a.enter.cmp(&b.enter))
                .then(a.volume.cmp(&b.volume))
        });
        place.track = Some(Rc::new(track));
        Ok(place)
    }
}

/// Where an object stands, for the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    /// In a space at a position, with a facing in degrees about the
    /// vertical axis when known and the record behind it when it is a
    /// placed reference.
    At {
        space: FormKey,
        pos: [f64; 3],
        facing: Option<f64>,
        record: Option<FormKey>,
    },
    /// Not settled: why.
    Open(&'static str),
}

/// The reference an alias handle holds in the run, as
/// `ReferenceAlias.GetReference` answers it (fills a run set, else the
/// forced or location reference of a running quest): `Ok(None)` for none.
fn alias_holds(
    store: &Store,
    max: u64,
    w: &mut World,
    key: &str,
) -> Result<Option<Val>, &'static str> {
    let (q, id) = key.split_once("#alias:").ok_or("not an alias")?;
    let id: i32 = id
        .parse()
        .map_err(|_| "an alias index that is not a number")?;
    let qk = key_record(q).ok_or("not an alias of a quest record")?;
    let info = quest::info_by_key(store, max, w, &qk).ok_or("not a quest record")?;
    let location = w.quests.location_fills;
    let s = w.quests.state(&info);
    if s.open.is_some() {
        return Err("the quest's state is undetermined");
    }
    if let Some(v) = s.fills.get(&id) {
        return Ok(Some(v.clone()));
    }
    match s.running {
        None => Err("whether the quest runs is undetermined"),
        Some(false) => Ok(None),
        Some(true) => {
            let id = u32::try_from(id).map_err(|_| "no reference alias of that index")?;
            let k = info
                .fill_of(id, location)?
                .ok_or("the alias has no forced reference")?;
            let b = Budget::new(max);
            let o = record_object(w, store, &k, "objectreference", &b)
                .map_err(|_| "the forced reference could not be read")?;
            Ok(Some(Val::Object(o)))
        }
    }
}

/// The object an alias stands for, or the object itself: `Err` when not
/// settled.
pub fn reference_of(
    store: &Store,
    max: u64,
    w: &mut World,
    o: ObjId,
) -> Result<ObjId, &'static str> {
    let key = w.objects[o].key.clone();
    match key {
        Some(k) if k.contains("#alias:") => match alias_holds(store, max, w, &k)? {
            Some(Val::Object(r)) => Ok(r),
            Some(Val::None) | None => Err("the alias holds no reference"),
            Some(_) => Err("the alias holds an undetermined value"),
        },
        _ => Ok(o),
    }
}

/// The stored position of a placed reference (`world_placed_position_v1`):
/// its `DATA` position and rotation in its cell's space; an actor, a
/// reference the run may have moved, or a record without placement is
/// open.
fn placed(store: &Store, max: u64, w: &mut World, o: ObjId) -> Spot {
    if w.place.moved.contains(&o) {
        return Spot::Open("a native of the run may have moved it");
    }
    let Some(k) = w.objects[o].key.as_deref().and_then(key_record) else {
        return Spot::Open("not a record");
    };
    let got = match w.place.stored.get(&k) {
        Some(x) => x.clone(),
        None => {
            let x = stored(store, max, &k);
            w.place.stored.insert(k.clone(), x.clone());
            x
        }
    };
    match got {
        Ok((space, pos, facing)) => {
            w.place.used.insert(rules::WORLD_PLACED_POSITION);
            Spot::At {
                space,
                pos,
                facing: Some(facing),
                record: Some(k),
            }
        }
        Err(why) => Spot::Open(why),
    }
}

/// A placed reference's stored space, position and facing (`DATA`, the
/// third rotation word in degrees).
fn stored(store: &Store, max: u64, k: &FormKey) -> Stored {
    match store.signature(k).as_ref().map(|s| &s[..]) {
        Some(b"REFR") => {}
        Some(b"ACHR") => return Err("an actor (no actor movement model)"),
        _ => return Err("not a placed reference"),
    }
    let b = Budget::new(max);
    let r = store
        .get::<Refr>(k, &b)
        .ok()
        .flatten()
        .ok_or("the reference could not be read")?;
    let t = r.data.transform.ok_or("no placement")?;
    let space = space_of(store, k).ok_or("the reference's cell is not known")?;
    Ok((
        space,
        [t[0], t[1], t[2]].map(f64::from),
        f64::from(t[5]).to_degrees(),
    ))
}

/// The space (interior cell or worldspace) of a placed reference's cell.
fn space_of(store: &Store, k: &FormKey) -> Option<FormKey> {
    let w = store.overrides().winner(k)?;
    let cell = store
        .placement_cells()
        .find(|(r, _)| std::ptr::eq(*r, w))
        .map(|(_, c)| c)?;
    match store.world(&cell) {
        Some(world) => Some(world),
        None => Some(cell),
    }
}

/// Where an object stands at the run's real-time second.
pub fn spot(store: &Store, max: u64, w: &mut World, v: Option<&Val>) -> Spot {
    let Some(Val::Object(o)) = v else {
        return Spot::Open("not an object");
    };
    let o = match reference_of(store, max, w, *o) {
        Ok(o) => o,
        Err(why) => return Spot::Open(why),
    };
    if w.player == Some(o) {
        let now = w.now[Clock::Real as usize];
        let Some(track) = w.place.track.clone() else {
            return Spot::Open("no track");
        };
        return match track.at(now) {
            Some((space, pos, facing)) => Spot::At {
                space: space.clone(),
                pos,
                facing,
                record: None,
            },
            None => Spot::Open("before the track's first point"),
        };
    }
    if !w.place.sw.placed {
        return Spot::Open("a placed reference (--placed-positions off)");
    }
    placed(store, max, w, o)
}

/// Natives the model answers or observes, lower case `script.function`.
pub const NATIVES: [&str; 16] = [
    "scriptobject.registerfordistancelessthanevent",
    "scriptobject.registerfordistancegreaterthanevent",
    "scriptobject.unregisterfordistanceevents",
    "objectreference.getdistance",
    "objectreference.getpositionx",
    "objectreference.getpositiony",
    "objectreference.getpositionz",
    "objectreference.getanglez",
    "objectreference.getparentcell",
    "objectreference.getworldspace",
    "objectreference.getcurrentlocation",
    "cell.isinterior",
    "objectreference.hasdirectlos",
    "objectreference.moveto",
    "objectreference.setposition",
    "objectreference.movetonode",
];

/// Natives after which the receiver's position is no longer the stored
/// one.
const MOVES: [&str; 6] = [
    "objectreference.moveto",
    "objectreference.setposition",
    "objectreference.movetonode",
    "objectreference.movetonearestnavmeshlocation",
    "objectreference.movetomyeditorlocation",
    "objectreference.translateto",
];

/// Notes what a native does to positions before any host answers it:
/// the receiver of a moving native is no longer at its stored position;
/// a `MoveTo` of the player is compared with the track (counted only: the
/// track is the caller's word).
pub fn observe(store: &Store, max: u64, c: &NativeCall, w: &mut World) {
    if !w.place.on {
        return;
    }
    let full = c.full_name();
    if full == "objectreference.delete" {
        if let Some(Val::Object(o)) = &c.receiver {
            if let Some(k) = w.objects[*o].key.as_deref().and_then(key_record) {
                if w.place.volumes.iter().any(|v| v.key == k) && w.place.deleted.insert(k) {
                    w.place.count("trigger references deleted by a native");
                }
            }
        }
        return;
    }
    if !MOVES.contains(&full.as_str()) {
        return;
    }
    let Some(Val::Object(o)) = c.receiver.clone() else {
        return;
    };
    if w.player == Some(o) {
        if full == "objectreference.moveto" {
            let target = spot(store, max, w, c.args.first());
            let here = spot(store, max, w, Some(&Val::Object(o)));
            let what = match (here, target) {
                (
                    Spot::At {
                        space: s1, pos: p1, ..
                    },
                    Spot::At {
                        space: s2, pos: p2, ..
                    },
                ) if s1 == s2 => {
                    if dist(p1, p2) <= 256.0 {
                        "within 256 units of the target"
                    } else {
                        "farther than 256 units from the target"
                    }
                }
                (Spot::At { .. }, Spot::At { .. }) => "in another space than the target",
                _ => "the track or the target is not settled",
            };
            w.place
                .count(format!("player MoveTo against the track\t{what}"));
        } else {
            w.place.count(format!(
                "player moved by a native the track does not follow\t{full}"
            ));
        }
        return;
    }
    if w.place.moved.insert(o) {
        w.place
            .count(format!("references moved by a native\t{full}"));
    }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// Answers a native of [`NATIVES`] when its switch is on: `None` leaves it
/// to the other hosts, as before. The rule relied on is returned with the
/// reply.
pub fn call(
    store: &Store,
    max: u64,
    c: &NativeCall,
    w: &mut World,
) -> Option<(Reply, Option<&'static str>)> {
    if !w.place.on {
        return None;
    }
    let sw = w.place.sw;
    let full = c.full_name();
    let undetermined = |w: &mut World, why: &str| {
        w.place
            .count(format!("position natives\t{full}\tundetermined: {why}"));
        Reply::Undetermined
    };
    match full.as_str() {
        "scriptobject.registerfordistancelessthanevent"
        | "scriptobject.registerfordistancegreaterthanevent"
            if sw.distance =>
        {
            let less = full.contains("lessthan");
            let obj = |v: Option<&Val>| match v {
                Some(Val::Object(o)) => Some(*o),
                _ => None,
            };
            let thr = match c.args.get(2) {
                Some(Val::Float(f)) => Some(f64::from(*f)),
                Some(Val::Int(n)) => Some(f64::from(*n)),
                _ => None,
            };
            match (c.instance, obj(c.args.first()), obj(c.args.get(1)), thr) {
                (Some(i), Some(a), Some(b), Some(t)) => {
                    let since = w.now[Clock::Real as usize];
                    w.place.registrations.push(DistanceReg {
                        instance: i,
                        a,
                        b,
                        threshold: t,
                        less,
                        since,
                        fired: false,
                        counted: false,
                    });
                    w.place.count(format!(
                        "distance registrations\t{}\tmade",
                        if less { "less than" } else { "greater than" }
                    ));
                }
                _ => w.place.count(
                    "distance registrations\tnot made: an argument or the instance is undetermined",
                ),
            }
            Some((Reply::Value(Val::None), Some(rules::WORLD_DISTANCE_EVENTS)))
        }
        "scriptobject.unregisterfordistanceevents" if sw.distance => {
            let obj = |v: Option<&Val>| match v {
                Some(Val::Object(o)) => Some(*o),
                _ => None,
            };
            if let Some(i) = c.instance {
                let (a, b) = (obj(c.args.first()), obj(c.args.get(1)));
                let before = w.place.registrations.len();
                w.place.registrations.retain(|r| {
                    !(r.instance == i
                        && (a.is_none() || a == Some(r.a))
                        && (b.is_none() || b == Some(r.b)))
                });
                let gone = before - w.place.registrations.len();
                for _ in 0..gone {
                    w.place.count("distance registrations\tremoved by a script");
                }
            }
            Some((Reply::Value(Val::None), Some(rules::WORLD_DISTANCE_EVENTS)))
        }
        "objectreference.getdistance" if sw.position_natives => {
            let a = spot(store, max, w, c.receiver.as_ref());
            let b = spot(store, max, w, c.args.first());
            let r = match (a, b) {
                (
                    Spot::At {
                        space: s1, pos: p1, ..
                    },
                    Spot::At {
                        space: s2, pos: p2, ..
                    },
                ) => {
                    if s1 == s2 {
                        w.place.count(format!("position natives\t{full}\tanswered"));
                        Reply::Value(Val::Float(dist(p1, p2) as f32))
                    } else {
                        undetermined(w, "two spaces")
                    }
                }
                (Spot::Open(why), _) | (_, Spot::Open(why)) => undetermined(w, why),
            };
            Some((r, Some(rules::WORLD_POSITION_NATIVES)))
        }
        "objectreference.getpositionx"
        | "objectreference.getpositiony"
        | "objectreference.getpositionz"
        | "objectreference.getanglez"
            if sw.position_natives =>
        {
            let r = match spot(store, max, w, c.receiver.as_ref()) {
                Spot::At { pos, facing, .. } => {
                    let v = match c.name.as_str() {
                        "getpositionx" => Some(pos[0]),
                        "getpositiony" => Some(pos[1]),
                        "getpositionz" => Some(pos[2]),
                        _ => facing,
                    };
                    match v {
                        Some(v) => {
                            w.place.count(format!("position natives\t{full}\tanswered"));
                            Reply::Value(Val::Float(v as f32))
                        }
                        None => undetermined(w, "no facing"),
                    }
                }
                Spot::Open(why) => undetermined(w, why),
            };
            Some((r, Some(rules::WORLD_POSITION_NATIVES)))
        }
        "objectreference.getparentcell" | "objectreference.getworldspace" if sw.cell_natives => {
            let r = match spot(store, max, w, c.receiver.as_ref()) {
                Spot::At { space, pos, .. } => {
                    let world = store.signature(&space) == Some(*b"WRLD");
                    let got = if c.name == "getworldspace" {
                        Ok(world.then_some(space))
                    } else if world {
                        exterior_cell(store, max, w, &space, pos).map(Some)
                    } else {
                        Ok(Some(space))
                    };
                    match got {
                        Ok(None) => {
                            w.place.count(format!("position natives\t{full}\tanswered"));
                            Reply::Value(Val::None)
                        }
                        Ok(Some(k)) => {
                            let b = Budget::new(max);
                            match record_object(w, store, &k, &c.return_type, &b) {
                                Ok(o) => {
                                    w.place.count(format!("position natives\t{full}\tanswered"));
                                    Reply::Value(Val::Object(o))
                                }
                                Err(_) => undetermined(w, "the record could not be read"),
                            }
                        }
                        Err(why) => undetermined(w, why),
                    }
                }
                Spot::Open(why) => undetermined(w, why),
            };
            Some((r, Some(rules::WORLD_CELL_NATIVES)))
        }
        "cell.isinterior" if sw.cell_natives => {
            let k = match c.receiver.as_ref() {
                Some(Val::Object(o)) => w.objects[*o].key.as_deref().and_then(key_record),
                _ => None,
            };
            let r = match k.and_then(|k| store.kind(&k)) {
                Some(kind) => {
                    w.place.count(format!("position natives\t{full}\tanswered"));
                    Reply::Value(Val::Bool(kind == fo4_plugin::typed::CellKind::Interior))
                }
                None => undetermined(w, "not a cell the store indexes"),
            };
            Some((r, Some(rules::WORLD_CELL_NATIVES)))
        }
        "objectreference.getcurrentlocation" if sw.location_native && sw.cell_natives => {
            let r = match spot(store, max, w, c.receiver.as_ref()) {
                Spot::At { space, pos, .. } => {
                    let cell = if store.signature(&space) == Some(*b"WRLD") {
                        exterior_cell(store, max, w, &space, pos)
                    } else {
                        Ok(space)
                    };
                    let b = Budget::new(max);
                    let loc = cell.and_then(|k| {
                        store
                            .get::<Cell>(&k, &b)
                            .ok()
                            .flatten()
                            .and_then(|x| x.data.location.and_then(|l| l.key))
                            .ok_or("the cell names no location")
                    });
                    match loc {
                        Ok(k) => match record_object(w, store, &k, &c.return_type, &b) {
                            Ok(o) => {
                                w.place.count(format!("position natives\t{full}\tanswered"));
                                Reply::Value(Val::Object(o))
                            }
                            Err(_) => undetermined(w, "the record could not be read"),
                        },
                        Err(why) => undetermined(w, why),
                    }
                }
                Spot::Open(why) => undetermined(w, why),
            };
            Some((r, Some(rules::WORLD_LOCATION_NATIVE)))
        }
        "objectreference.hasdirectlos" => {
            let v = sw.assume_los?;
            w.place
                .count("line of sight answered by the caller's assumption (--assume-los)");
            Some((Reply::Value(Val::Bool(v)), None))
        }
        _ => None,
    }
}

/// The exterior cell of a worldspace whose grid square holds a position
/// (`world_cell_natives_v1`).
pub fn exterior_cell(
    store: &Store,
    max: u64,
    w: &mut World,
    world: &FormKey,
    pos: [f64; 3],
) -> Result<FormKey, &'static str> {
    let g = [(pos[0] / CELL_SIZE).floor(), (pos[1] / CELL_SIZE).floor()];
    let key = (world.clone(), g[0] as i64, g[1] as i64);
    if let Some(x) = w.place.grid_cells.get(&key) {
        return x.clone();
    }
    let x = find_exterior_cell(store, max, world, g);
    w.place.grid_cells.insert(key, x.clone());
    x
}

fn find_exterior_cell(
    store: &Store,
    max: u64,
    world: &FormKey,
    g: [f64; 2],
) -> Result<FormKey, &'static str> {
    let mut found = None;
    for c in store.cells() {
        if store.world(&c).as_ref() != Some(world) {
            continue;
        }
        let b = Budget::new(max);
        let Some(x) = store.get::<Cell>(&c, &b).ok().flatten() else {
            continue;
        };
        if let Some(gr) = x.data.grid {
            if f64::from(gr.i) == g[0] && f64::from(gr.j) == g[1] {
                if found.is_some() {
                    return Err("two cells of the worldspace at one grid square");
                }
                found = Some(c);
            }
        }
    }
    found.ok_or("no cell of the worldspace at that grid square")
}

/// What [`settle`] raised.
#[derive(Debug, Clone)]
pub struct Raised {
    /// The real-time second.
    pub t: f64,
    /// `ontriggerenter`, `ontriggerleave`, `ondistancelessthan` or
    /// `ondistancegreaterthan`.
    pub event: &'static str,
    /// The trigger reference (trigger events).
    pub reference: Option<FormKey>,
    /// Handler runs: (receiver kind, script, outcome).
    pub runs: Vec<(String, String, Outcome)>,
}

/// The earliest second after the run's present at which the model has
/// something to raise: the next crossing or a distance registration's
/// due second. `None` when nothing is due later.
pub fn next_due(store: &Store, max: u64, w: &mut World) -> Option<f64> {
    if !w.place.on {
        return None;
    }
    let now = w.now[Clock::Real as usize];
    let mut best = w
        .place
        .crossings
        .get(w.place.cursor)
        .map(|c| c.t)
        .filter(|t| *t > now);
    if w.place.sw.distance {
        for i in 0..w.place.registrations.len() {
            if let Ok(Some(t)) = due(store, max, w, i, now) {
                if t > now && best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
    }
    best
}

/// The first second at or after `from` (and the registration's own
/// second) at which registration `i`'s condition holds: the distance
/// between the two objects' reference points at or below the threshold
/// (less than), at or above it (greater than). `Ok(None)`: never on the
/// track; `Err`: a position is open. One side the player follows the
/// track; the other must stand still.
fn due(
    store: &Store,
    max: u64,
    w: &mut World,
    i: usize,
    from: f64,
) -> Result<Option<f64>, &'static str> {
    let r = w.place.registrations[i].clone();
    if r.fired {
        return Ok(None);
    }
    let from = from.max(r.since);
    let a = reference_of(store, max, w, r.a)?;
    let b = reference_of(store, max, w, r.b)?;
    let player = w.player;
    let (moving, still) = match (Some(a) == player, Some(b) == player) {
        (true, true) => return Err("the player against itself"),
        (true, false) => (true, b),
        (false, true) => (true, a),
        (false, false) => (false, a),
    };
    if !moving {
        if !w.place.sw.placed {
            return Err("a placed reference (--placed-positions off)");
        }
        let (
            Spot::At {
                space: s1, pos: p1, ..
            },
            Spot::At {
                space: s2, pos: p2, ..
            },
        ) = (placed(store, max, w, a), placed(store, max, w, b))
        else {
            return Err("a placed reference's position is open");
        };
        if s1 != s2 {
            return Err("two spaces");
        }
        let d = dist(p1, p2);
        let holds = if r.less {
            d <= r.threshold
        } else {
            d >= r.threshold
        };
        return Ok(holds.then_some(from));
    }
    if !w.place.sw.placed {
        return Err("a placed reference (--placed-positions off)");
    }
    let Spot::At { space, pos: q, .. } = placed(store, max, w, still) else {
        return Err("the other object's position is open");
    };
    let Some(track) = w.place.track.clone() else {
        return Err("no track");
    };
    for (t0, t1, s, p0, p1) in track.moves() {
        if t1 < from || *s != space {
            continue;
        }
        let dt = t1 - t0;
        let a = [p0[0] - q[0], p0[1] - q[1], p0[2] - q[2]];
        let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let s_from = if dt.is_finite() && dt > 0.0 {
            ((from - t0) / dt).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let at = |s: f64| {
            if dt.is_finite() && dt > 0.0 {
                t0 + s * dt
            } else {
                t0.max(from)
            }
        };
        if d == [0.0; 3] {
            let dd = dist(p0, q);
            let holds = if r.less {
                dd <= r.threshold
            } else {
                dd >= r.threshold
            };
            if holds {
                return Ok(Some(at(s_from).max(from)));
            }
            continue;
        }
        // The closed ball of the threshold's radius about the still
        // object, met along this move.
        let ball = sphere_span(a, d, r.threshold);
        let s = if r.less {
            ball.filter(|(_, hi)| *hi >= s_from)
                .map(|(lo, _)| lo.max(s_from))
        } else {
            match ball {
                None => Some(s_from),
                Some((lo, _)) if s_from <= lo => Some(s_from),
                Some((_, hi)) if s_from <= hi => Some(hi),
                Some(_) => Some(s_from),
            }
        };
        if let Some(s) = s {
            return Ok(Some(at(s).max(from)));
        }
    }
    Ok(None)
}

/// Whether a trigger reference is enabled in the run (the host's
/// `IsDisabled` state without counting a native call).
fn enabled(vm: &mut Vm<'_>, k: &FormKey) -> Result<bool, &'static str> {
    if vm.world.place.deleted.contains(k) {
        return Ok(false);
    }
    match vm.host.reference_disabled(&mut vm.world, k) {
        Some(Ok(d)) => Ok(!d),
        Some(Err(why)) => Err(why),
        None => Err("the host does not keep the enable state"),
    }
}

/// Applies what is due at the run's present second and raises it, running
/// the quest jobs each delivery brings before the next round: crossings
/// up to now, then trigger enter and leave events where a volume's hold
/// changed, then distance events whose condition holds now. Repeats while
/// anything was raised, up to [`MAX_ROUNDS`]. Returns what was raised and
/// the job runs.
pub fn settle(
    vm: &mut Vm<'_>,
    store: &Store,
    max_jobs: usize,
    b: &Budget,
) -> (Vec<Raised>, Vec<quest::JobRun>) {
    let mut raised = Vec::new();
    let mut runs = Vec::new();
    if !vm.world.place.on {
        return (raised, runs);
    }
    let max = store.limits.max_memory;
    for round in 0.. {
        if round == MAX_ROUNDS {
            vm.world
                .place
                .count("instants left with more to raise after the round limit");
            break;
        }
        let now = vm.world.now[Clock::Real as usize];
        let mut geo_changed: BTreeSet<usize> = BTreeSet::new();
        while let Some(c) = vm.world.place.crossings.get(vm.world.place.cursor).copied() {
            if c.t > now {
                break;
            }
            vm.world.place.cursor += 1;
            if c.enter {
                vm.world.place.geo.insert(c.volume);
            } else {
                vm.world.place.geo.remove(&c.volume);
            }
            geo_changed.insert(c.volume);
        }
        let sw = vm.world.place.sw;
        // Changes of hold: (volume, enter).
        let mut changes: Vec<(usize, bool)> = Vec::new();
        let cand: BTreeSet<usize> = vm
            .world
            .place
            .geo
            .union(&vm.world.place.held)
            .chain(&geo_changed)
            .copied()
            .collect();
        for v in cand {
            let key = vm.world.place.volumes[v].key.clone();
            if vm.world.place.deleted.contains(&key) {
                // A deleted reference is gone: no event either way
                // (`world_trigger_enabled_v1` when on; nothing assumed
                // otherwise, and nothing raised).
                let label = if sw.enabled {
                    vm.rules.insert(rules::WORLD_TRIGGER_ENABLED);
                    "--trigger-enabled"
                } else {
                    "nothing assumed (--trigger-enabled off)"
                };
                if vm.world.place.held.remove(&v) {
                    vm.world.place.count(format!(
                        "triggers deleted with the player inside: no event\t{label}"
                    ));
                }
                if geo_changed.contains(&v) {
                    vm.world.place.count(format!(
                        "trigger crossings of a deleted reference: no event\t{label}"
                    ));
                }
                continue;
            }
            let inside = vm.world.place.geo.contains(&v);
            let held = vm.world.place.held.contains(&v);
            let geometric = geo_changed.contains(&v);
            let en = if inside || held {
                enabled(vm, &key)
            } else {
                Ok(true)
            };
            let want = inside && en == Ok(true);
            if geometric && inside && en == Ok(false) {
                if sw.enabled {
                    vm.rules.insert(rules::WORLD_TRIGGER_ENABLED);
                }
                vm.world.place.count(format!(
                    "trigger crossings of a disabled reference: no event\t{}",
                    if sw.enabled {
                        "--trigger-enabled"
                    } else {
                        "nothing assumed (--trigger-enabled off)"
                    }
                ));
            }
            if want == held {
                vm.world.place.blocked.remove(&v);
                continue;
            }
            if vm.world.place.blocked.contains(&v) && !geometric {
                continue;
            }
            let why = if let Err(e) = en {
                Some(format!("the enable state is undetermined: {e}"))
            } else if !sw.point && sw.body.is_none() {
                Some("--trigger-point and --trigger-body off".into())
            } else if !sw.instant {
                Some("--trigger-instant off".into())
            } else if !geometric && !sw.enabled {
                Some("enabled or disabled with the player inside (--trigger-enabled off)".into())
            } else {
                None
            };
            match why {
                Some(why) => {
                    vm.world.place.blocked.insert(v);
                    vm.world.place.count(format!(
                        "trigger events not raised\t{}\t{why}",
                        if want { "enter" } else { "leave" }
                    ));
                }
                None => changes.push((v, want)),
            }
        }
        if changes.len() > 1 && !sw.order {
            for (v, want) in &changes {
                vm.world.place.blocked.insert(*v);
                vm.world.place.count(format!(
                    "trigger events not raised\t{}\tseveral at one instant (--trigger-order off)",
                    if *want { "enter" } else { "leave" }
                ));
            }
            changes.clear();
        }
        // Leaves first, then enters, each by reference key
        // (`world_trigger_order_v1`; the volumes are in key order).
        changes.sort_by_key(|(v, e)| (*e, *v));
        let mut any = false;
        for (v, enter) in changes {
            let key = vm.world.place.volumes[v].key.clone();
            vm.rules.insert(if sw.body.is_some() {
                rules::WORLD_TRIGGER_BODY
            } else {
                rules::WORLD_TRIGGER_POINT
            });
            vm.rules.insert(rules::WORLD_TRIGGER_INSTANT);
            vm.rules.insert(match vm.world.place.volumes[v].shape {
                Shape::Box(_) => rules::WORLD_TRIGGER_BOX,
                Shape::Sphere(_) => rules::WORLD_TRIGGER_SPHERE,
            });
            if !geo_changed.contains(&v) {
                vm.rules.insert(rules::WORLD_TRIGGER_ENABLED);
            }
            if sw.order {
                vm.rules.insert(rules::WORLD_TRIGGER_ORDER);
            }
            if enter {
                vm.world.place.held.insert(v);
            } else {
                vm.world.place.held.remove(&v);
            }
            vm.world.place.blocked.remove(&v);
            let action = if enter {
                player::Action::TriggerEnter
            } else {
                player::Action::TriggerLeave
            };
            let cause = if geo_changed.contains(&v) {
                "the track crossed the volume"
            } else {
                "enabled or disabled with the player inside"
            };
            vm.world.place.count(format!(
                "trigger events raised\t{}\t{cause}",
                action.handler()
            ));
            let mut handler_runs = Vec::new();
            match player::deliver(vm, store, &key, action, b) {
                Ok((ds, _)) => {
                    for d in ds {
                        handler_runs.push((d.receiver.kind().to_string(), d.script, d.outcome));
                    }
                }
                Err(e) => vm
                    .world
                    .place
                    .count(format!("trigger events not delivered\t{e}")),
            }
            raised.push(Raised {
                t: now,
                event: action.handler(),
                reference: Some(key),
                runs: handler_runs,
            });
            runs.extend(quest::run_jobs(vm, store, max_jobs, b));
            any = true;
        }
        if sw.distance {
            let mut i = 0;
            while i < vm.world.place.registrations.len() {
                let d = due(store, max, &mut vm.world, i, now);
                let r = vm.world.place.registrations[i].clone();
                match d {
                    Ok(Some(t)) if t <= now => {
                        vm.rules.insert(rules::WORLD_DISTANCE_EVENTS);
                        if sw.placed {
                            vm.rules.insert(rules::WORLD_PLACED_POSITION);
                        }
                        let event = if r.less {
                            "ondistancelessthan"
                        } else {
                            "ondistancegreaterthan"
                        };
                        if sw.distance_once {
                            vm.rules.insert(rules::WORLD_DISTANCE_ONCE);
                            vm.world.place.registrations.remove(i);
                        } else {
                            vm.world.place.registrations[i].fired = true;
                            vm.world.place.count(
                                "distance registrations\tkept after their event, not raised again (--distance-once off)",
                            );
                            i += 1;
                        }
                        let a = reference_of(store, max, &mut vm.world, r.a).unwrap_or(r.a);
                        let bb = reference_of(store, max, &mut vm.world, r.b).unwrap_or(r.b);
                        let outcome = vm.deliver(
                            r.instance,
                            event,
                            vec![
                                Val::Object(a),
                                Val::Object(bb),
                                Val::Float(r.threshold as f32),
                            ],
                        );
                        vm.world
                            .place
                            .count(format!("distance events raised\t{event}"));
                        let script = vm.world.instances[r.instance].script.clone();
                        raised.push(Raised {
                            t: now,
                            event,
                            reference: None,
                            runs: vec![("the registering instance".to_string(), script, outcome)],
                        });
                        runs.extend(quest::run_jobs(vm, store, max_jobs, b));
                        any = true;
                    }
                    Err(why) => {
                        if !r.counted {
                            vm.world.place.registrations[i].counted = true;
                            vm.world
                                .place
                                .count(format!("distance registrations\tundetermined: {why}"));
                        }
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
        }
        if !any {
            break;
        }
    }
    (raised, runs)
}
