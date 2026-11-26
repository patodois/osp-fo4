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
    /// `--actor-positions` (`world_actor_placed_position_v1`): placed
    /// actors stand at their stored position, or where the caller's
    /// assumption moved them.
    pub actors: bool,
    /// `--actor-triggers` (`world_actor_trigger_v1`): an actor's assumed
    /// move raises trigger enter and leave as the player's does.
    pub actor_triggers: bool,
    /// `--moveto-target` (`world_moveto_target_v1`): `MoveTo(target)` with
    /// zero offsets puts the reference at the target's position at once.
    pub moveto_target: bool,
    /// `--load-doors` (`world_load_door_xtel_v1`): where the track changes
    /// space, the player passes through the load door of the old space
    /// that leads to the new one and stands at its teleport position.
    pub load_doors: bool,
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

/// One volume's intervals over a track's finite moves so far, folded as
/// [`intervals`] folds them (before its final stay and its filter): the
/// crossings of a track that grows point by point (`spec/play.md`, the
/// stepped run; [`Place::extend`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Folded(pub Vec<(f64, f64)>);
impl Folded {
    /// Folds one move (`step_of`) as [`intervals`] does.
    pub fn push(&mut self, v: &Volume, body: Option<u32>, mv: Move<'_>) {
        let (t0, t1, space, a, b) = mv;
        if *space != v.space {
            return;
        }
        let span = match body {
            Some(h) => v.span_body(a, b, f64::from(h)),
            None => v.span(a, b),
        };
        let Some((s0, s1)) = span else {
            return;
        };
        let (e, l) = if t1.is_infinite() {
            (t0, f64::INFINITY)
        } else {
            (t0 + s0 * (t1 - t0), t0 + s1 * (t1 - t0))
        };
        match self.0.last_mut() {
            Some(last) if last.1 >= e => last.1 = last.1.max(l),
            _ => self.0.push((e, l)),
        }
    }
    /// The intervals with the stay after the last point, as [`intervals`]
    /// returns them for the track folded so far.
    pub fn finish(&self, v: &Volume, body: Option<u32>, last: &Point) -> Vec<(f64, f64)> {
        let mut out = self.clone();
        out.push(
            v,
            body,
            (last.t, f64::INFINITY, &last.space, last.pos, last.pos),
        );
        out.0.retain(|(e, l)| l > e);
        out.0
    }
}

/// One move of a track between two points, as the track's moves read it:
/// a move to a point in another space is a stay at the earlier point.
pub fn step_of<'a>(a: &'a Point, b: &Point) -> Move<'a> {
    if a.space == b.space {
        (a.t, b.t, &a.space, a.pos, b.pos)
    } else {
        (a.t, b.t, &a.space, a.pos, a.pos)
    }
}

/// The crossings of every volume on a track, ordered as
/// [`Place::load_with`] orders them, from their folds.
fn crossings_of(
    volumes: &[Volume],
    folds: &[Folded],
    body: Option<u32>,
    last: &Point,
) -> Vec<Crossing> {
    let mut out = Vec::new();
    for (i, (f, v)) in folds.iter().zip(volumes).enumerate() {
        for (e, l) in f.finish(v, body, last) {
            out.push(Crossing {
                t: e,
                volume: i,
                enter: true,
            });
            if l.is_finite() {
                out.push(Crossing {
                    t: l,
                    volume: i,
                    enter: false,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.t.total_cmp(&b.t)
            .then(a.enter.cmp(&b.enter))
            .then(a.volume.cmp(&b.volume))
    });
    out
}

/// The folds of a track the run grows point by point ([`Place::cut`],
/// [`Place::extend`]).
#[derive(Debug, Clone, Default)]
pub struct Folds {
    /// One per volume, as [`Place::volumes`].
    pub volumes: Vec<Folded>,
    /// Times the crossings already applied changed when the track was cut
    /// or grown (they should not: counted to check it).
    pub prefix_changed: u64,
}

/// The exterior grid squares and interior cells whose references a run
/// has read ([`Place::load_with`] reads the cells about the track's
/// points), so that the cells about a new point are read once.
#[derive(Debug, Clone, Default)]
pub struct Region {
    pub squares: BTreeMap<FormKey, BTreeSet<(i64, i64)>>,
    pub interiors: BTreeSet<FormKey>,
}
impl Region {
    /// What [`Place::load_with`] read about these points: each interior
    /// cell, and each worldspace's squares within one of the points' box.
    pub fn of(store: &Store, points: &[Point]) -> Region {
        let mut r = Region::default();
        let mut boxes: BTreeMap<FormKey, ([i64; 2], [i64; 2])> = BTreeMap::new();
        for p in points {
            if store.signature(&p.space) == Some(*b"CELL") {
                r.interiors.insert(p.space.clone());
                continue;
            }
            let g = square(p.pos);
            let e = boxes
                .entry(p.space.clone())
                .or_insert(([i64::MAX; 2], [i64::MIN; 2]));
            for i in 0..2 {
                e.0[i] = e.0[i].min([g.0, g.1][i]);
                e.1[i] = e.1[i].max([g.0, g.1][i]);
            }
        }
        for (space, (lo, hi)) in boxes {
            let set = r.squares.entry(space).or_default();
            for i in lo[0] - 1..=hi[0] + 1 {
                for j in lo[1] - 1..=hi[1] + 1 {
                    set.insert((i, j));
                }
            }
        }
        r
    }
    /// Whether the cells about a point have been read; if not, marks them
    /// read.
    pub fn claim(&mut self, store: &Store, p: &Point) -> bool {
        if store.signature(&p.space) == Some(*b"CELL") {
            return !self.interiors.insert(p.space.clone());
        }
        let g = square(p.pos);
        let set = self.squares.entry(p.space.clone()).or_default();
        let around: Vec<(i64, i64)> = (-1..=1)
            .flat_map(|i| (-1..=1).map(move |j| (g.0 + i, g.1 + j)))
            .collect();
        if around.iter().all(|x| set.contains(x)) {
            return true;
        }
        set.extend(around);
        false
    }
}

/// The exterior grid square of a position.
fn square(p: [f64; 3]) -> (i64, i64) {
    (
        (p[0] / CELL_SIZE).floor() as i64,
        (p[1] / CELL_SIZE).floor() as i64,
    )
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

/// A caller's assumed move of a placed actor (`quest_run
/// --assume-actor-at`): from that second the actor stands there.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorMove {
    pub t: f64,
    pub actor: FormKey,
    pub space: FormKey,
    pub pos: [f64; 3],
}

/// One passage of the track through a load door
/// (`world_load_door_xtel_v1`, read with `--load-doors`): at the second of
/// the track's last point in the old space the player stands at the door's
/// teleport position in the new space.
#[derive(Debug, Clone, PartialEq)]
pub struct DoorPassage {
    pub t: f64,
    /// The placed door of the old space and the door its `XTEL` names.
    pub door: FormKey,
    pub to: FormKey,
    pub from_space: FormKey,
    pub space: FormKey,
    /// The teleport position and facing (degrees) the `XTEL` stores.
    pub pos: [f64; 3],
    pub facing: f64,
    /// Distance from the track's last point in the old space to the door's
    /// stored position.
    pub reach: f64,
}

/// An `XTEL` of 36 bytes read as the door it names (the first word, the
/// established prefix), then three position words and three rotation words
/// (`world_load_door_xtel_v1`; the census of the 1,204 stored values is in
/// `spec/vm.md`, The walk home); the last two words are not read.
#[derive(Debug, Clone, PartialEq)]
pub struct Teleport {
    pub door: FormKey,
    pub pos: [f64; 3],
    pub rot: [f64; 3],
}

/// The size of every stored `XTEL`.
pub const XTEL_BYTES: usize = 36;

/// The `XTEL` of a placed reference read in its plugin's master context:
/// `Ok(None)` without one.
pub fn teleport_of(
    store: &Store,
    r: &Record,
    plugin: usize,
) -> Result<Option<Teleport>, &'static str> {
    let mut it = r.subrecords.iter().filter(|s| s.signature == *b"XTEL");
    let s = match (it.next(), it.next()) {
        (None, _) => return Ok(None),
        (Some(s), None) => s,
        _ => return Err("two XTEL subrecords"),
    };
    let d = r.data(s);
    if d.len() != XTEL_BYTES {
        return Err("an XTEL that is not 36 bytes");
    }
    let word = |i: usize| f64::from(f32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]));
    let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    let door = store
        .order()
        .remap(plugin, id, &r.source.file, s.offset)
        .ok()
        .flatten()
        .ok_or("an XTEL door that does not resolve")?;
    let pos = [word(4), word(8), word(12)];
    let rot = [word(16), word(20), word(24)];
    if pos.iter().chain(&rot).any(|x| !x.is_finite()) {
        return Err("an XTEL position or rotation that is not finite");
    }
    Ok(Some(Teleport { door, pos, rot }))
}

/// The cells of a space whose placed references are read near a point: an
/// interior cell itself; for a worldspace, its cells within one grid square
/// of the point and its cells holding persistent references.
fn cells_near_point(
    store: &Store,
    space: &FormKey,
    p: [f64; 3],
    b: &Budget,
) -> Result<Vec<FormKey>, String> {
    if store.signature(space) == Some(*b"CELL") {
        return Ok(vec![space.clone()]);
    }
    let g = [p[0], p[1]].map(|x| (x / CELL_SIZE).floor() as i64);
    let mut out = Vec::new();
    for c in store.cells() {
        if store.world(&c).as_ref() != Some(space) {
            continue;
        }
        let cb = b.child();
        let grid = store
            .get::<Cell>(&c, &cb)
            .map_err(|e| e.to_string())?
            .and_then(|x| x.data.grid.map(|g| (i64::from(g.i), i64::from(g.j))));
        let near = grid.is_some_and(|(i, j)| (i - g[0]).abs() <= 1 && (j - g[1]).abs() <= 1);
        let persistent = store
            .children(&c, &cb)
            .map_err(|e| e.to_string())?
            .active
            .iter()
            .any(|k| k.group_kind == 8);
        if near || persistent {
            out.push(c);
        }
    }
    Ok(out)
}

/// Where the track changes space, the passage through the load door of
/// the old space whose `XTEL` names a door in the new space, the one
/// nearest the track's last point in the old space (persistent doors of a
/// worldspace within two grid squares of it); a point at the teleport
/// position is put after that last point, at its second
/// (`world_load_door_xtel_v1`). What was found is counted.
fn pass_load_doors(
    store: &Store,
    track: &mut Track,
    counts: &mut BTreeMap<String, u64>,
    b: &Budget,
) -> Result<Vec<DoorPassage>, String> {
    let mut bump = |k: &str| *counts.entry(format!("load doors\t{k}")).or_default() += 1;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < track.points.len() {
        let (a, next) = (&track.points[i], &track.points[i + 1]);
        if a.space == next.space {
            i += 1;
            continue;
        }
        bump("space changes of the track");
        let (from, to, p, t) = (a.space.clone(), next.space.clone(), a.pos, a.t);
        let mut best: Option<(f64, FormKey, Teleport)> = None;
        for cell in cells_near_point(store, &from, p, b)? {
            let cb = b.child();
            let ch = store.children(&cell, &cb).map_err(|e| e.to_string())?;
            for kid in ch.active.iter().chain(&ch.disabled) {
                if kid.signature != *b"REFR" {
                    continue;
                }
                let rb = b.child();
                let Some((r, plugin)) = store
                    .raw_with_plugin(&kid.key, &rb)
                    .map_err(|e| e.to_string())?
                else {
                    continue;
                };
                let tp = match teleport_of(store, &r, plugin) {
                    Ok(Some(tp)) => tp,
                    Ok(None) => continue,
                    Err(why) => {
                        bump(&format!("doors left out\t{why}"));
                        continue;
                    }
                };
                let base = bind::base_of(store, &kid.key, &rb).ok().flatten();
                if base.and_then(|k| store.signature(&k)) != Some(*b"DOOR") {
                    bump("references with an XTEL left out\tthe base is not a door");
                    continue;
                }
                let Some(at) = store
                    .get::<Refr>(&kid.key, &rb)
                    .map_err(|e| e.to_string())?
                    .and_then(|x| x.data.transform)
                else {
                    continue;
                };
                let at = [at[0], at[1], at[2]].map(f64::from);
                let far = (0..2).any(|k| (at[k] - p[k]).abs() > 2.0 * CELL_SIZE);
                if kid.group_kind == 8 && store.signature(&from) == Some(*b"WRLD") && far {
                    continue;
                }
                if space_of(store, &tp.door).as_ref() != Some(&to) {
                    continue;
                }
                let d = (0..3).map(|k| (at[k] - p[k]).powi(2)).sum::<f64>().sqrt();
                let better = match &best {
                    None => true,
                    Some((bd, bk, _)) => {
                        d < *bd
                            || (d == *bd
                                && (kid.key.plugin.as_str(), kid.key.object_id)
                                    < (bk.plugin.as_str(), bk.object_id))
                    }
                };
                if better {
                    best = Some((d, kid.key.clone(), tp));
                }
            }
        }
        match best {
            None => {
                bump("space changes with no load door of the old space leading to the new: the caller's next point kept");
                i += 1;
            }
            Some((reach, door, tp)) => {
                bump(&format!(
                    "space changes through a load door: the player placed at its teleport position\tthe door {} the track's last point",
                    if reach <= 256.0 {
                        "within 256 units of"
                    } else {
                        "farther than 256 units from"
                    }
                ));
                let facing = tp.rot[2].to_degrees();
                out.push(DoorPassage {
                    t,
                    door,
                    to: tp.door.clone(),
                    from_space: from,
                    space: to.clone(),
                    pos: tp.pos,
                    facing,
                    reach,
                });
                track.points.insert(
                    i + 1,
                    Point {
                        t,
                        space: to,
                        pos: tp.pos,
                        facing: Some(facing),
                    },
                );
                i += 2;
            }
        }
    }
    Ok(out)
}

/// A placed actor near the track, at its stored position
/// (`world_actor_placed_position_v1`, read with `--actor-positions`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedActor {
    pub key: FormKey,
    pub space: FormKey,
    pub pos: [f64; 3],
}

/// A `MoveTo` the run called on the player ([`Place::player_moves`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerMove {
    /// The run's second of the call.
    pub t: f64,
    /// Where the target stands then.
    pub target: Spot,
    /// Whether the three offsets after the target are all zero.
    pub zero_offsets: bool,
}

/// A stage a trigger's enter handlers asked about and found not done:
/// the quest's object, the stage, and the quest log's length then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked {
    pub quest: ObjId,
    pub stage: i32,
    pub log_at: usize,
}

/// The stages `effects` show asked about and answered not done
/// (`IsStageDone` and `GetStageDone` on a quest): the asks a trigger's
/// handlers made, for [`Place::asking_again`].
pub fn asked_not_done(effects: &[crate::Effect], log_at: usize) -> Vec<Asked> {
    let mut out = Vec::new();
    for e in effects {
        if !matches!(
            e.native.as_str(),
            "quest.isstagedone" | "quest.getstagedone"
        ) {
            continue;
        }
        let (Some(quest), Some(Ok(stage))) = (
            e.receiver_object,
            e.args.first().map(|a| a.trim().parse::<i32>()),
        ) else {
            continue;
        };
        if e.result.trim() != "false" {
            continue;
        }
        let a = Asked {
            quest,
            stage,
            log_at,
        };
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
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
    /// The second of the last native that may have moved each object.
    pub moved_at: BTreeMap<ObjId, f64>,
    /// Where a `MoveTo` with zero offsets put an object, at the second of
    /// that native (`world_moveto_target_v1`): (second, space, position).
    pub native_at: BTreeMap<ObjId, (f64, FormKey, [f64; 3])>,
    /// The run's `MoveTo` calls on the player, in order (kept for a caller
    /// that steps the run: the play mode follows them under its own
    /// switch; the run itself only compares them with the track).
    pub player_moves: Vec<PlayerMove>,
    /// The caller's assumed actor moves, earliest first.
    pub actor_moves: Vec<ActorMove>,
    /// Actor moves applied so far (`world_actor_trigger_v1`).
    pub actor_cursor: usize,
    /// Actors with an assumed move a gate holds as a caller action
    /// (`--action-after-actor-moves`, `caller_actor_move_after_v1`): moved
    /// actors, as those with a move in [`Place::actor_moves`].
    pub actors_held_moves: BTreeSet<FormKey>,
    /// The moves scene package actions scheduled, by scene handle
    /// (`pack_location_destination_v1`): those not yet applied end with
    /// their scene.
    pub package_moves: Vec<(ObjId, ActorMove)>,
    /// Volumes holding each moved actor, by actor key.
    pub actor_held: BTreeMap<FormKey, BTreeSet<usize>>,
    /// Placed actors of the cells read near the track (with
    /// `--actor-positions`).
    pub actors: Vec<PlacedActor>,
    /// References a native of the run deleted (`Delete`): they hold no
    /// one (`world_trigger_enabled_v1`).
    pub deleted: BTreeSet<FormKey>,
    /// Presentation only (nothing in the run reads it): per volume, the
    /// stages its last enter's handlers asked about and found not done
    /// ([`Place::asking_again`]).
    pub asked: BTreeMap<usize, Vec<Asked>>,
    /// The track's passages through load doors (`world_load_door_xtel_v1`).
    pub doors: Vec<DoorPassage>,
    /// Counts by text.
    pub counts: BTreeMap<String, u64>,
    /// Rules a native's answer relied on, taken by the host after the
    /// call.
    pub used: BTreeSet<&'static str>,
    /// Stored positions read so far: (space, position, facing in degrees)
    /// or why there is none.
    stored: BTreeMap<FormKey, Stored>,
    /// Placed actors' stored positions read so far, as `stored` (the
    /// records do not change during a run; reading one searches every
    /// placement for its cell, so it is read once).
    stored_actors: BTreeMap<FormKey, Stored>,
    /// The placements' cells, read when first needed.
    cells: Option<Rc<Cells>>,
    /// The folds of the track when it grows point by point
    /// ([`Place::cut`], [`Place::extend`]); `None` when the whole track was
    /// given at load.
    pub folds: Option<Folds>,
    /// Exterior cells found by worldspace and grid square.
    grid_cells: BTreeMap<(FormKey, i64, i64), Result<FormKey, &'static str>>,
}
impl Place {
    /// Presentation only: the volumes whose last enter's handlers asked
    /// about a stage, found it not done, and the run has set it since,
    /// that are neither deleted nor holding the player now. Entering one
    /// again asks its handlers again with the stage done.
    pub fn asking_again(&self, log: &[(ObjId, crate::quest::Change)]) -> Vec<usize> {
        let mut out = Vec::new();
        for (v, asks) in &self.asked {
            let Some(vol) = self.volumes.get(*v) else {
                continue;
            };
            if self.deleted.contains(&vol.key) || self.held.contains(v) {
                continue;
            }
            let set_since = asks.iter().any(|a| {
                log.get(a.log_at..)
                    .unwrap_or(&[])
                    .iter()
                    .any(|(o, c)| *o == a.quest && *c == crate::quest::Change::StageSet(a.stage))
            });
            if set_since {
                out.push(*v);
            }
        }
        out
    }
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    /// The placements' cells ([`Cells`]), read once.
    pub(crate) fn cells(&mut self, store: &Store) -> Rc<Cells> {
        self.cells
            .get_or_insert_with(|| Rc::new(Cells::read(store)))
            .clone()
    }
    /// The volumes of a space holding a standing point under the point or
    /// body reading (none when neither is on).
    pub fn holding(&self, space: &FormKey, p: [f64; 3]) -> BTreeSet<usize> {
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, v)| &v.space == space)
            .filter(|(_, v)| match (self.sw.point, self.sw.body) {
                (true, _) => v.contains(p),
                (false, Some(h)) => v.span_body(p, p, f64::from(h)).is_some(),
                _ => false,
            })
            .map(|(i, _)| i)
            .collect()
    }
    /// The seconds of the caller's assumed moves of an actor.
    fn move_times(&self, actor: &FormKey) -> Vec<f64> {
        self.actor_moves
            .iter()
            .filter(|m| &m.actor == actor)
            .map(|m| m.t)
            .collect()
    }
    /// Cuts the track at second `now` and folds what is kept: from here
    /// the track grows only by [`Place::extend`] (`spec/play.md`, the
    /// stepped run). Returns the points dropped and whether any point was
    /// kept; `None` without a track. With no point kept the track is left
    /// as it is.
    pub fn cut(&mut self, now: f64) -> Option<(usize, bool)> {
        let track = self.track.clone()?;
        let mut folds = Folds {
            volumes: vec![Folded::default(); self.volumes.len()],
            prefix_changed: 0,
        };
        let kept: Vec<Point> = track
            .points
            .iter()
            .filter(|p| p.t <= now)
            .cloned()
            .collect();
        let dropped = track.points.len() - kept.len();
        if kept.is_empty() {
            self.folds = Some(folds);
            return Some((dropped, false));
        }
        for pair in kept.windows(2) {
            let mv = step_of(&pair[0], &pair[1]);
            for (f, v) in folds.volumes.iter_mut().zip(&self.volumes) {
                f.push(v, self.sw.body, mv);
            }
        }
        self.folds = Some(folds);
        self.track = Some(Rc::new(Track { points: kept }));
        self.rebuild();
        Some((dropped, true))
    }
    /// Appends points to a cut track ([`Place::cut`]), folds their moves
    /// into the volumes' crossings as [`intervals`] folds a whole track and
    /// rebuilds the crossing list, keeping the cursor (the crossings
    /// already applied must not change: counted in
    /// [`Folds::prefix_changed`]). Returns the new moves. A point before
    /// the track's last second is refused.
    pub fn extend(&mut self, points: &[Point]) -> Result<Vec<(Point, Point)>, String> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let body = self.sw.body;
        let Some(rc) = self.track.as_mut() else {
            return Err(
                "PlayNoTrack: the play mode needs the run's --track (its first point is the start)"
                    .into(),
            );
        };
        let track = Rc::make_mut(rc);
        let mut prev = track.points.last().cloned();
        for p in points {
            if !p.t.is_finite() || prev.as_ref().is_some_and(|q| p.t < q.t) {
                return Err(format!(
                    "PlayTrackBack: a point at {} s before the track's last second",
                    p.t
                ));
            }
            prev = Some(p.clone());
        }
        let mut last = track.points.last().cloned();
        let mut moves = Vec::new();
        for p in points {
            if let Some(a) = &last {
                moves.push((a.clone(), p.clone()));
            }
            track.points.push(p.clone());
            last = Some(p.clone());
        }
        let folds = self.folds.get_or_insert_with(|| Folds {
            volumes: vec![Folded::default(); self.volumes.len()],
            prefix_changed: 0,
        });
        for (a, b) in &moves {
            let mv = step_of(a, b);
            for (f, v) in folds.volumes.iter_mut().zip(&self.volumes) {
                f.push(v, body, mv);
            }
        }
        self.rebuild();
        Ok(moves)
    }
    /// Adds the volumes this place does not hold (by reference), folding
    /// the whole track so far into their crossings; the crossing list is
    /// rebuilt by the next [`Place::rebuild`]. Returns how many were added.
    pub fn add_volumes(&mut self, volumes: Vec<Volume>) -> usize {
        let have: BTreeSet<FormKey> = self.volumes.iter().map(|v| v.key.clone()).collect();
        let points = self
            .track
            .as_ref()
            .map(|t| t.points.clone())
            .unwrap_or_default();
        let folds = self.folds.get_or_insert_with(Folds::default);
        let mut added = 0;
        for v in volumes {
            if have.contains(&v.key) {
                continue;
            }
            let mut f = Folded::default();
            for pair in points.windows(2) {
                f.push(&v, self.sw.body, step_of(&pair[0], &pair[1]));
            }
            self.volumes.push(v);
            folds.volumes.push(f);
            added += 1;
        }
        added
    }
    /// The crossing list from the folds of a cut track, as
    /// [`Place::load_with`] orders it, the cursor kept.
    pub fn rebuild(&mut self) {
        let Some(track) = self.track.clone() else {
            return;
        };
        let Some(last) = track.points.last() else {
            return;
        };
        let Some(folds) = self.folds.as_mut() else {
            return;
        };
        let crossings = crossings_of(&self.volumes, &folds.volumes, self.sw.body, last);
        let n = self.cursor.min(self.crossings.len());
        if crossings.get(..n) != Some(&self.crossings[..n]) {
            folds.prefix_changed += 1;
        }
        self.crossings = crossings;
    }
    /// Loads the volumes near the track and their crossings. The placed
    /// references read are those of the track's interior cells and, for a
    /// worldspace, of its cells whose grid square lies within one cell of
    /// the track's points there, and of its persistent cells.
    pub fn load(store: &Store, track: Track, sw: Switches, b: &Budget) -> Result<Place, String> {
        Place::load_with(store, track, sw, Vec::new(), b)
    }
    /// [`Place::load`] with the caller's assumed actor moves: their points
    /// join the track's in choosing the cells read.
    pub fn load_with(
        store: &Store,
        mut track: Track,
        sw: Switches,
        mut moves: Vec<ActorMove>,
        b: &Budget,
    ) -> Result<Place, String> {
        track.resolve(store)?;
        for m in &mut moves {
            if store.signature(&m.space) == Some(*b"CELL") {
                if let Some(w) = store.world(&m.space) {
                    m.space = w;
                }
            }
        }
        moves.sort_by(|a, b| a.t.total_cmp(&b.t));
        let mut place = Place {
            on: true,
            sw,
            ..Place::default()
        };
        if sw.load_doors {
            place.doors = pass_load_doors(store, &mut track, &mut place.counts, b)?;
        }
        // Bounding boxes of the points by space.
        let mut boxes: BTreeMap<FormKey, ([f64; 2], [f64; 2])> = BTreeMap::new();
        let points = track
            .points
            .iter()
            .map(|p| (&p.space, p.pos))
            .chain(moves.iter().map(|m| (&m.space, m.pos)));
        for (space, pos) in points {
            let e = boxes
                .entry(space.clone())
                .or_insert(([f64::MAX; 2], [f64::MIN; 2]));
            for (i, x) in pos.iter().take(2).enumerate() {
                e.0[i] = e.0[i].min(*x);
                e.1[i] = e.1[i].max(*x);
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
            // Placed actors (outside the active list, which holds REFR
            // only), with `--actor-positions`.
            let actors = ch
                .excluded
                .iter()
                .filter(|k| sw.actors && k.signature == *b"ACHR");
            for kid in ch.active.iter().chain(&ch.disabled).chain(actors) {
                let rb = b.child();
                let Some(r) = store.raw(&kid.key, &rb).map_err(|e| e.to_string())? else {
                    continue;
                };
                if sw.actors && r.signature == *b"ACHR" {
                    if let Ok((sp, pos, _)) = stored_actor(
                        store,
                        store.limits.max_memory,
                        &kid.key,
                        Some(&place.cells(store)),
                    ) {
                        let (lo, hi) = boxes[space];
                        let out = kid.group_kind == 8
                            && (0..2).any(|i| pos[i] < lo[i] - margin || pos[i] > hi[i] + margin);
                        if !out && &sp == space {
                            place.actors.push(PlacedActor {
                                key: kid.key.clone(),
                                space: sp,
                                pos,
                            });
                        }
                    }
                    continue;
                }
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
        place.actors.sort_by(|a, b| {
            (a.key.plugin.as_str(), a.key.object_id).cmp(&(b.key.plugin.as_str(), b.key.object_id))
        });
        // The volumes each moved actor holds at its stored position: held
        // without an event (`world_actor_trigger_v1`).
        if sw.actors {
            let mut seen = BTreeSet::new();
            for m in &moves {
                if !seen.insert(m.actor.clone()) {
                    continue;
                }
                if let Ok((sp, pos, _)) = stored_actor(
                    store,
                    store.limits.max_memory,
                    &m.actor,
                    Some(&place.cells(store)),
                ) {
                    let held = place.holding(&sp, pos);
                    if !held.is_empty() {
                        place.count("actors standing in a trigger volume at their stored position: held without an event");
                    }
                    place.actor_held.insert(m.actor.clone(), held);
                }
            }
        }
        place.actor_moves = moves;
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
            let k = match w.quests.fill(&info, id)? {
                Some(quest::Fill::Record(k)) => k,
                Some(quest::Fill::Player) => return Ok(Some(Val::Object(w.player("actor")))),
                None => return Err("the alias has no forced reference"),
            };
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
    let now = w.now[Clock::Real as usize];
    placed_at(store, max, w, o, now)
}

/// [`placed`] at second `t`: a placed actor, with `--actor-positions`
/// (`world_actor_placed_position_v1`), where the caller's last assumed
/// move at or before `t` put it, else at its stored position; a native
/// of the run that may have moved it after that makes it open.
fn placed_at(store: &Store, max: u64, w: &mut World, o: ObjId, t: f64) -> Spot {
    let Some(k) = w.objects[o].key.as_deref().and_then(key_record) else {
        return Spot::Open(if w.place.moved.contains(&o) {
            "a native of the run may have moved it"
        } else {
            "not a record"
        });
    };
    if w.place.sw.actors && store.signature(&k) == Some(*b"ACHR") {
        let assumed = w
            .place
            .actor_moves
            .iter()
            .rfind(|m| m.actor == k && m.t <= t)
            .cloned();
        let native = w.place.moved_at.get(&o).copied();
        if native.is_some_and(|n| assumed.as_ref().is_none_or(|m| n > m.t)) {
            return moved_to(w, o, k);
        }
        w.place.used.insert(rules::WORLD_ACTOR_POSITION);
        if let Some(m) = assumed {
            return Spot::At {
                space: m.space,
                pos: m.pos,
                facing: None,
                record: Some(k),
            };
        }
        let got = match w.place.stored_actors.get(&k) {
            Some(x) => x.clone(),
            None => {
                let cells = w.place.cells(store);
                let x = stored_actor(store, max, &k, Some(&cells));
                w.place.stored_actors.insert(k.clone(), x.clone());
                x
            }
        };
        return match got {
            Ok((space, pos, facing)) => Spot::At {
                space,
                pos,
                facing: Some(facing),
                record: Some(k),
            },
            Err(why) => Spot::Open(why),
        };
    }
    if w.place.moved.contains(&o) {
        return moved_to(w, o, k);
    }
    let got = match w.place.stored.get(&k) {
        Some(x) => x.clone(),
        None => {
            let cells = w.place.cells(store);
            let x = stored(store, max, &k, Some(&cells));
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

/// Where the last native that moved an object put it: the target of a
/// `MoveTo` with zero offsets (`world_moveto_target_v1`), else open.
fn moved_to(w: &mut World, o: ObjId, k: FormKey) -> Spot {
    let last = w.place.moved_at.get(&o).copied();
    match w.place.native_at.get(&o) {
        Some((t, space, pos)) if Some(*t) == last => {
            w.place.used.insert(rules::WORLD_MOVETO_TARGET);
            Spot::At {
                space: space.clone(),
                pos: *pos,
                facing: None,
                record: Some(k),
            }
        }
        _ => Spot::Open("a native of the run may have moved it"),
    }
}

/// A placed reference's stored space, position and facing (`DATA`, the
/// third rotation word in degrees).
fn stored(store: &Store, max: u64, k: &FormKey, cells: Option<&Cells>) -> Stored {
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
    let space = match cells {
        Some(c) => c.space(store, k),
        None => space_of(store, k),
    }
    .ok_or("the reference's cell is not known")?;
    Ok((
        space,
        [t[0], t[1], t[2]].map(f64::from),
        f64::from(t[5]).to_degrees(),
    ))
}

/// The stored space (its worldspace, or its interior cell) and position
/// of a placed reference or actor.
pub fn stored_position(
    store: &Store,
    max: u64,
    k: &FormKey,
) -> Result<(FormKey, [f64; 3]), &'static str> {
    stored_position_with(store, max, k, None)
}

/// [`stored_position`] with the placements' cells looked up in an index
/// read once ([`Cells`]) instead of searched for each reference: the same
/// answers ([`Cells::read`] keeps a record's first membership, as
/// [`space_of`] finds it).
pub(crate) fn stored_position_with(
    store: &Store,
    max: u64,
    k: &FormKey,
    cells: Option<&Cells>,
) -> Result<(FormKey, [f64; 3]), &'static str> {
    let got = if store.signature(k) == Some(*b"ACHR") {
        stored_actor(store, max, k, cells)
    } else {
        stored(store, max, k, cells)
    };
    got.map(|(space, pos, _)| (space, pos))
}

/// Adds an actor move to the world model, after the moves already applied
/// and in time order among those waiting (a move at a second already
/// passed applies at the next settling).
pub fn schedule_move(w: &mut World, m: ActorMove) {
    let at = w.place.actor_cursor
        + w.place.actor_moves[w.place.actor_cursor..]
            .iter()
            .take_while(|x| x.t <= m.t)
            .count();
    w.place.actor_moves.insert(at, m);
}

/// A placed actor's stored space, position and facing (`DATA` as a
/// placed reference's, `fo4_plugin::actor::Achr`).
fn stored_actor(store: &Store, max: u64, k: &FormKey, cells: Option<&Cells>) -> Stored {
    if store.signature(k) != Some(*b"ACHR") {
        return Err("not a placed actor");
    }
    let b = Budget::new(max);
    let (r, plugin) = store
        .raw_with_plugin(k, &b)
        .ok()
        .flatten()
        .ok_or("the actor could not be read")?;
    let ctx = fo4_plugin::actor::Context {
        order: store.order(),
        plugin,
    };
    let a =
        fo4_plugin::actor::Achr::decode(&r, ctx, &b).map_err(|_| "the actor could not be read")?;
    let t = a.transform.ok_or("no placement")?;
    let space = match cells {
        Some(c) => c.space(store, k),
        None => space_of(store, k),
    }
    .ok_or("the actor's cell is not known")?;
    Ok((
        space,
        [t[0], t[1], t[2]].map(f64::from),
        f64::from(t[5]).to_degrees(),
    ))
}

/// The parent cell of every winning placement, by its winning record, read
/// once: finding one placement's cell searches every placement
/// (`space_of`), and a run asks for many.
#[derive(Debug, Default)]
pub struct Cells(std::collections::HashMap<usize, FormKey>);
impl Cells {
    pub fn read(store: &Store) -> Cells {
        let mut m = std::collections::HashMap::new();
        for (r, c) in store.placement_cells() {
            // The first membership of a record, as `space_of` finds it.
            m.entry(std::ptr::from_ref(r) as usize).or_insert(c);
        }
        Cells(m)
    }
    /// [`space_of`] through the index.
    fn space(&self, store: &Store, k: &FormKey) -> Option<FormKey> {
        let w = store.overrides().winner(k)?;
        let cell = self.0.get(&(std::ptr::from_ref(w) as usize))?.clone();
        match store.world(&cell) {
            Some(world) => Some(world),
            None => Some(cell),
        }
    }
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
            let zero_offsets = c
                .args
                .get(1..4)
                .is_some_and(|x| x.iter().all(|v| matches!(v, Val::Float(f) if *f == 0.0)));
            w.place.player_moves.push(PlayerMove {
                t: w.now[Clock::Real as usize],
                target: target.clone(),
                zero_offsets,
            });
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
    let now = w.now[Clock::Real as usize];
    // `MoveTo(target, 0, 0, 0)` with the target settled
    // (`world_moveto_target_v1`), its spot taken before the move is noted.
    let zero = c
        .args
        .get(1..4)
        .is_some_and(|x| x.iter().all(|v| matches!(v, Val::Float(f) if *f == 0.0)));
    let target = (w.place.sw.moveto_target && full == "objectreference.moveto" && zero)
        .then(|| spot(store, max, w, c.args.first()));
    w.place.moved_at.insert(o, now);
    match target {
        Some(Spot::At { space, pos, .. }) => {
            w.place.native_at.insert(o, (now, space, pos));
            w.place
                .count("references moved by a native\tMoveTo to a settled target with zero offsets: placed there");
        }
        Some(Spot::Open(why)) => {
            w.place.native_at.remove(&o);
            w.place.count(format!(
                "references moved by a native\tMoveTo whose target is open: {why}"
            ));
        }
        None => {
            w.place.native_at.remove(&o);
        }
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
    if let Some(m) = w.place.actor_moves.get(w.place.actor_cursor) {
        if m.t > now && best.is_none_or(|b| m.t < b) {
            best = Some(m.t);
        }
    }
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
/// track; the other must stand still, or (an actor under
/// `world_actor_placed_position_v1`) stand still between the caller's
/// assumed moves.
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
    first_time(store, max, w, a, b, r.threshold, r.less, from)
}

/// [`first_time`] for a caller outside the registrations (a function 1
/// condition, `cond_fn_1_track_distance_v1`): either object may be an
/// alias handle or the player.
#[allow(clippy::too_many_arguments)]
pub fn distance_first_time(
    store: &Store,
    max: u64,
    w: &mut World,
    a: ObjId,
    b: ObjId,
    threshold: f64,
    less: bool,
    from: f64,
) -> Result<Option<f64>, &'static str> {
    let a = reference_of(store, max, w, a)?;
    let b = reference_of(store, max, w, b)?;
    first_time(store, max, w, a, b, threshold, less, from)
}

/// The first second at or after `from` at which the distance between the
/// reference points of `a` and `b` is at or below (`less`) or at or above
/// the threshold; see [`due`].
#[allow(clippy::too_many_arguments)]
fn first_time(
    store: &Store,
    max: u64,
    w: &mut World,
    a: ObjId,
    b: ObjId,
    threshold: f64,
    less: bool,
    from: f64,
) -> Result<Option<f64>, &'static str> {
    let player = w.player;
    let (moving, still) = match (Some(a) == player, Some(b) == player) {
        (true, true) => return Err("the player against itself"),
        (true, false) => (true, b),
        (false, true) => (true, a),
        (false, false) => (false, a),
    };
    if !w.place.sw.placed {
        return Err("a placed reference (--placed-positions off)");
    }
    // Windows between the caller's assumed moves of the objects, in which
    // every object but the player stands still.
    let mut cuts = vec![from];
    for o in [a, b] {
        if let Some(k) = w.objects[o].key.as_deref().and_then(key_record) {
            cuts.extend(w.place.move_times(&k).into_iter().filter(|t| *t > from));
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts.push(f64::INFINITY);
    let holds = |d: f64| if less { d <= threshold } else { d >= threshold };
    for win in cuts.windows(2) {
        let (c0, c1) = (win[0], win[1]);
        if !moving {
            let (
                Spot::At {
                    space: s1, pos: p1, ..
                },
                Spot::At {
                    space: s2, pos: p2, ..
                },
            ) = (
                placed_at(store, max, w, a, c0),
                placed_at(store, max, w, b, c0),
            )
            else {
                return Err("a placed reference's position is open");
            };
            if s1 != s2 {
                return Err("two spaces");
            }
            if holds(dist(p1, p2)) {
                return Ok(Some(c0));
            }
            continue;
        }
        let Spot::At { space, pos: q, .. } = placed_at(store, max, w, still, c0) else {
            return Err("the other object's position is open");
        };
        let Some(track) = w.place.track.clone() else {
            return Err("no track");
        };
        for (t0, t1, s, p0, p1) in track.moves() {
            if t1 < c0 || *s != space {
                continue;
            }
            if t0 >= c1 {
                break;
            }
            let dt = t1 - t0;
            let a = [p0[0] - q[0], p0[1] - q[1], p0[2] - q[2]];
            let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let s_from = if dt.is_finite() && dt > 0.0 {
                ((c0 - t0) / dt).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let at = |s: f64| {
                if dt.is_finite() && dt > 0.0 {
                    t0 + s * dt
                } else {
                    t0.max(c0)
                }
            };
            let found = if d == [0.0; 3] {
                holds(dist(p0, q)).then(|| at(s_from).max(c0))
            } else {
                // The closed ball of the threshold's radius about the still
                // object, met along this move.
                let ball = sphere_span(a, d, threshold);
                let s = if less {
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
                s.map(|s| at(s).max(c0))
            };
            match found {
                Some(t) if t < c1 => return Ok(Some(t)),
                Some(_) => break,
                None => {}
            }
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

/// Applies the caller's assumed actor moves due by `now`: with
/// `--actor-triggers` (`world_actor_trigger_v1`, and the trigger readings
/// the player's events need) the volumes the actor leaves and enters
/// raise their events with the actor as the one who acts, leaves first,
/// each by volume. Returns whether anything was raised.
fn actor_moves(
    vm: &mut Vm<'_>,
    store: &Store,
    now: f64,
    raised: &mut Vec<Raised>,
    runs: &mut Vec<quest::JobRun>,
    max_jobs: usize,
    b: &Budget,
) -> bool {
    let mut any = false;
    while let Some(m) = vm
        .world
        .place
        .actor_moves
        .get(vm.world.place.actor_cursor)
        .cloned()
    {
        if m.t > now {
            break;
        }
        vm.world.place.actor_cursor += 1;
        vm.world
            .place
            .count("actor moves assumed by the caller applied");
        let sw = vm.world.place.sw;
        if !sw.actors {
            continue;
        }
        let new = vm.world.place.holding(&m.space, m.pos);
        let old = vm
            .world
            .place
            .actor_held
            .get(&m.actor)
            .cloned()
            .unwrap_or_default();
        let mut changes: Vec<(usize, bool)> = old
            .difference(&new)
            .map(|v| (*v, false))
            .chain(new.difference(&old).map(|v| (*v, true)))
            .collect();
        changes.retain(|(v, _)| {
            !vm.world
                .place
                .deleted
                .contains(&vm.world.place.volumes[*v].key)
        });
        vm.world.place.actor_held.insert(m.actor.clone(), new);
        if changes.is_empty() {
            continue;
        }
        let why = if !sw.actor_triggers {
            Some("--actor-triggers off")
        } else if !sw.point && sw.body.is_none() {
            Some("--trigger-point and --trigger-body off")
        } else if !sw.instant {
            Some("--trigger-instant off")
        } else if changes.len() > 1 && !sw.order {
            Some("several at one instant (--trigger-order off)")
        } else {
            None
        };
        if let Some(why) = why {
            for (_, enter) in &changes {
                vm.world.place.count(format!(
                    "actor trigger events not raised\t{}\t{why}",
                    if *enter { "enter" } else { "leave" }
                ));
            }
            continue;
        }
        changes.sort_by_key(|(v, e)| (*e, *v));
        let Ok(actor) = record_object(&mut vm.world, store, &m.actor, "actor", b) else {
            vm.world
                .place
                .count("actor trigger events not raised\tthe actor could not be read");
            continue;
        };
        for (v, enter) in changes {
            let key = vm.world.place.volumes[v].key.clone();
            match enabled(vm, &key) {
                Ok(true) => {}
                Ok(false) => {
                    vm.world
                        .place
                        .count("actor trigger events not raised\ta disabled trigger");
                    continue;
                }
                Err(_) => {
                    vm.world.place.count(
                        "actor trigger events not raised\tthe trigger's enable state is undetermined",
                    );
                    continue;
                }
            }
            vm.rules.insert(rules::WORLD_ACTOR_TRIGGER);
            vm.rules.insert(rules::WORLD_ACTOR_POSITION);
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
            let action = if enter {
                player::Action::TriggerEnter
            } else {
                player::Action::TriggerLeave
            };
            vm.world
                .place
                .count(format!("actor trigger events raised\t{}", action.handler()));
            let mut handler_runs = Vec::new();
            match player::deliver_by(vm, store, &key, action, Some(actor), b) {
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
    }
    any
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
            let e0 = vm.effects.len();
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
            if enter {
                let log_at = vm.world.quests.log.len();
                let asked = asked_not_done(vm.effects.get(e0..).unwrap_or(&[]), log_at);
                vm.world.place.asked.insert(v, asked);
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
        if actor_moves(vm, store, now, &mut raised, &mut runs, max_jobs, b) {
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
