//! What the crosshair points at: the first collision primitive the eye's
//! ray meets within the caller's reach, and the placed reference it was
//! built from (`play_activate_collision_ray_v1`). Triangles are met
//! exactly; spheres, capsules and hulls (their points widened by their
//! radius) where the distance to them along the ray first reaches zero,
//! found by search to [`TOLERANCE`] (the distance to a convex body along a
//! line is convex). Terrain and primitives of no placed reference are met
//! too and answer no reference: they stop the ray.
use fo4_plugin::FormKey;
use fo4_scene::collision::CellCollision;
use fo4_walk::geometry::{self as g, Aabb, V3};
use fo4_walk::world::Prim;
use fo4_walk::World;

/// How close (world units) the search places a hit on a body with a
/// radius (a numerical setting).
pub const TOLERANCE: f64 = 1e-3;

/// The placed reference of each source of a walking world built from a
/// cell's collision (`fo4_walk::World::build`), by source index.
pub fn references(world: &World, c: &CellCollision) -> Vec<Option<FormKey>> {
    world
        .sources
        .iter()
        .map(|s| {
            let inst = c.instances.get(s.instance?)?;
            c.placements
                .get(inst.placement)
                .map(|p| p.reference.clone())
        })
        .collect()
}

/// What the ray met.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// A collision primitive: the entry and source of the walking world.
    Collision { entry: u32, source: u32 },
    /// The object bounds of a reference with no collision ([`Boxes`]).
    Bounds { index: usize },
}

/// The object bounds of the scene's placed references that `refs` (the
/// walking world's references) holds no collision for, leaving out editor
/// markers and references an enable parent disables; and how many of the
/// rest have no usable bounds or placement.
pub fn object_bounds(scene: &fo4_scene::Scene, refs: &[Option<FormKey>]) -> (Boxes, u64) {
    let with: std::collections::BTreeSet<&FormKey> = refs.iter().flatten().collect();
    let mut boxes = Boxes::default();
    let mut left = 0u64;
    for i in &scene.instances {
        let k = &i.reference.key;
        if with.contains(k) || i.editor_marker || i.disabled_by_enable_parent() {
            continue;
        }
        let placed = match (
            i.base.as_ref().and_then(|b| b.common.bounds),
            i.reference.data.transform,
        ) {
            (Some(bounds), Some(t)) => {
                fo4_scene::transform::Transform::euler(t, i.reference.data.scale.unwrap_or(1.0))
                    .ok()
                    .map(|f| {
                        let pushed = boxes.push(k.clone(), f, bounds);
                        if pushed {
                            boxes.kinds.resize(boxes.items.len() - 1, None);
                            boxes
                                .kinds
                                .push(i.base.as_ref().map(|b| b.record.signature));
                        }
                        pushed
                    })
            }
            _ => None,
        };
        if placed != Some(true) {
            left += 1;
        }
    }
    (boxes, left)
}

/// A hit of the eye's ray.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Distance from the eye along the ray.
    pub distance: f64,
    pub via: Via,
    /// The placed reference the primitive was built from; `None` for
    /// terrain and primitives of no reference.
    pub reference: Option<FormKey>,
}

/// The object bounds (`OBND`, six i16: the low and high corner in the
/// model's units) of placed references the walking world holds no
/// collision for, each placed by its reference's transform: what the ray
/// meets of them under `play_activate_object_bounds_v1`.
#[derive(Debug, Clone, Default)]
pub struct Boxes {
    pub items: Vec<(FormKey, fo4_scene::transform::Transform, [f64; 3], [f64; 3])>,
    /// The record type of each item's base, where [`object_bounds`] read
    /// it (items pushed otherwise have none).
    pub kinds: Vec<Option<[u8; 4]>>,
}
impl Boxes {
    /// The box of a reference's base bounds placed by its transform; `None`
    /// for an empty box.
    pub fn push(
        &mut self,
        reference: FormKey,
        frame: fo4_scene::transform::Transform,
        bounds: [i16; 6],
    ) -> bool {
        let lo = [bounds[0], bounds[1], bounds[2]].map(f64::from);
        let hi = [bounds[3], bounds[4], bounds[5]].map(f64::from);
        if (0..3).any(|i| hi[i] < lo[i]) || frame.scale.is_nan() || frame.scale <= 0.0 {
            return false;
        }
        self.items.push((reference, frame, lo, hi));
        true
    }
    /// The boxes whose base is not of one of these record types
    /// (`--play-pick-bounds-activatable` leaves out the static types,
    /// `play_activate_bounds_not_static_v1`); a box of unknown type is kept.
    pub fn without_kinds(&self, kinds: &[[u8; 4]]) -> Boxes {
        let mut out = Boxes::default();
        for (n, item) in self.items.iter().enumerate() {
            let kind = self.kinds.get(n).copied().flatten();
            if kind.is_some_and(|k| kinds.contains(&k)) {
                continue;
            }
            out.items.push(item.clone());
            out.kinds.push(kind);
        }
        out
    }
    /// The first box the ray meets within `reach`: (distance, index).
    fn first(
        &self,
        eye: V3,
        d: V3,
        reach: f64,
        skip: &dyn Fn(&FormKey) -> bool,
    ) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, (k, f, lo, hi)) in self.items.iter().enumerate() {
            if skip(k) {
                continue;
            }
            // Into the box's frame: local = R (world - t) / s (the inverse
            // of `Transform::apply`), the ray's parameter kept.
            let to_local = |v: V3| -> V3 {
                std::array::from_fn(|i| {
                    (0..3).map(|j| f.rotation[i][j] * v[j]).sum::<f64>() / f.scale
                })
            };
            let o = to_local(g::sub(eye, f.translation));
            let dl = to_local(d);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for a in 0..3 {
                if dl[a].abs() < 1e-15 {
                    if o[a] < lo[a] || o[a] > hi[a] {
                        hit = false;
                        break;
                    }
                    continue;
                }
                let (mut u, mut v) = ((lo[a] - o[a]) / dl[a], (hi[a] - o[a]) / dl[a]);
                if u > v {
                    std::mem::swap(&mut u, &mut v);
                }
                t0 = t0.max(u);
                t1 = t1.min(v);
                if t0 > t1 {
                    hit = false;
                    break;
                }
            }
            if hit && best.is_none_or(|(b, _)| t0 < b) {
                best = Some((t0, i));
            }
        }
        best
    }
}

/// The first primitive the ray from `eye` along `dir` (any length, not
/// zero) meets within `reach`.
pub fn first(world: &World, refs: &[Option<FormKey>], eye: V3, dir: V3, reach: f64) -> Option<Hit> {
    first_with(world, refs, eye, dir, reach, &|_| false)
}

/// [`first`] passing through the primitives of the references `skip`
/// names (those the run disabled: a disabled reference has no collision
/// under `vm_disable_hides_reference_v1`).
pub fn first_with(
    world: &World,
    refs: &[Option<FormKey>],
    eye: V3,
    dir: V3,
    reach: f64,
    skip: &dyn Fn(&FormKey) -> bool,
) -> Option<Hit> {
    first_of(world, refs, None, eye, dir, reach, skip)
}

/// [`first_with`] that also meets the boxes of `boxes` (the nearer of the
/// two wins; a tie goes to the collision).
pub fn first_of(
    world: &World,
    refs: &[Option<FormKey>],
    boxes: Option<&Boxes>,
    eye: V3,
    dir: V3,
    reach: f64,
    skip: &dyn Fn(&FormKey) -> bool,
) -> Option<Hit> {
    first_where(world, refs, boxes, eye, dir, reach, &|_| true, skip)
}

/// [`first_of`] meeting only the primitives whose source `keep` keeps
/// (the sources a live world shows, `crate::live`).
#[allow(clippy::too_many_arguments)]
pub fn first_where(
    world: &World,
    refs: &[Option<FormKey>],
    boxes: Option<&Boxes>,
    eye: V3,
    dir: V3,
    reach: f64,
    keep: &dyn Fn(u32) -> bool,
    skip: &dyn Fn(&FormKey) -> bool,
) -> Option<Hit> {
    let hit = collision(world, refs, eye, dir, reach, keep, skip);
    let d = g::unit(dir)?;
    let boxed = boxes.and_then(|b| b.first(eye, d, reach, skip).map(|(t, i)| (t, i, b)));
    match (hit, boxed) {
        (Some(h), Some((t, _, _))) if h.distance <= t => Some(h),
        (_, Some((t, i, b))) => Some(Hit {
            distance: t,
            via: Via::Bounds { index: i },
            reference: Some(b.items[i].0.clone()),
        }),
        (h, None) => h,
    }
}

fn collision(
    world: &World,
    refs: &[Option<FormKey>],
    eye: V3,
    dir: V3,
    reach: f64,
    keep: &dyn Fn(u32) -> bool,
    skip: &dyn Fn(&FormKey) -> bool,
) -> Option<Hit> {
    let d = g::unit(dir)?;
    if !(reach.is_finite() && reach > 0.0 && g::finite(eye)) {
        return None;
    }
    let end = g::add(eye, g::scale(d, reach));
    let query = Aabb::of(&[eye, end], 0.0)?;
    let mut candidates = Vec::new();
    world.candidates(&query, &mut candidates);
    let mut best: Option<(f64, u32)> = None;
    for i in candidates {
        let e = &world.entries[i as usize];
        if !keep(e.source) {
            continue;
        }
        if refs
            .get(e.source as usize)
            .and_then(Option::as_ref)
            .is_some_and(skip)
        {
            continue;
        }
        let t = match &e.prim {
            Prim::Triangle(tri) => triangle(eye, d, *tri),
            Prim::Sphere { center, radius } => convex(eye, d, reach, &[*center], *radius),
            Prim::Capsule { points, radius } => convex(eye, d, reach, points, *radius),
            Prim::Hull {
                start,
                count,
                radius,
            } => convex(eye, d, reach, world.hull_points(*start, *count), *radius),
        };
        if let Some(t) = t.filter(|t| *t <= reach) {
            if best.is_none_or(|(b, _)| t < b) {
                best = Some((t, i));
            }
        }
    }
    let (distance, entry) = best?;
    let source = world.entries[entry as usize].source;
    Some(Hit {
        distance,
        via: Via::Collision { entry, source },
        reference: refs.get(source as usize).cloned().flatten(),
    })
}

/// Two-sided ray and triangle (Möller and Trumbore's test).
fn triangle(o: V3, d: V3, [a, b, c]: [V3; 3]) -> Option<f64> {
    let e1 = g::sub(b, a);
    let e2 = g::sub(c, a);
    let p = g::cross(d, e2);
    let det = g::dot(e1, p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = g::sub(o, a);
    let u = g::dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = g::cross(s, e1);
    let v = g::dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = g::dot(e2, q) * inv;
    (t >= 0.0).then_some(t)
}

/// The first distance along the ray at which the hull of `core` widened
/// by `radius` is met, within `reach`.
fn convex(o: V3, d: V3, reach: f64, core: &[V3], radius: f64) -> Option<f64> {
    if core.is_empty() {
        return None;
    }
    let f = |t: f64| {
        let p = g::add(o, g::scale(d, t));
        g::closest(&[p], core).distance - radius
    };
    if f(0.0) <= 0.0 {
        return Some(0.0);
    }
    // The minimum of a convex function on [0, reach].
    let (mut lo, mut hi) = (0.0, reach);
    while hi - lo > TOLERANCE {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if f(m1) <= f(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let tmin = 0.5 * (lo + hi);
    if f(tmin) > 0.0 {
        return None;
    }
    // The first zero before it.
    let (mut a, mut b) = (0.0, tmin);
    while b - a > TOLERANCE {
        let m = 0.5 * (a + b);
        if f(m) > 0.0 {
            a = m;
        } else {
            b = m;
        }
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_core::Budget;

    fn world() -> World {
        let b = Budget::new(1 << 24);
        let mut w = World::new();
        // A wall across x = 100 and a sphere at x = 50 off the axis.
        w.add_triangles(
            &[[
                [100.0, -50.0, -50.0],
                [100.0, 50.0, -50.0],
                [100.0, 0.0, 50.0],
            ]],
            "wall",
            &b,
        )
        .unwrap();
        w.add_sphere([50.0, 30.0, 0.0], 10.0, "ball", &b).unwrap();
        w.rebuild(64.0, &b).unwrap();
        w
    }

    #[test]
    fn the_ray_meets_the_first_primitive_within_reach() {
        let w = world();
        let refs = vec![
            None,
            Some(FormKey {
                plugin: "a.esm".into(),
                object_id: 0x10,
            }),
            None,
        ];
        // Straight along x: the wall at 100.
        let h = first(&w, &refs, [0.0; 3], [1.0, 0.0, 0.0], 500.0).unwrap();
        assert!((h.distance - 100.0).abs() < 1e-9);
        // Short reach: nothing.
        assert!(first(&w, &refs, [0.0; 3], [1.0, 0.0, 0.0], 99.0).is_none());
        // Towards the sphere: met at its surface, before the wall.
        let dir = [50.0, 30.0, 0.0];
        let h = first(&w, &refs, [0.0; 3], dir, 500.0).unwrap();
        let expect = g::length(dir) - 10.0;
        assert!((h.distance - expect).abs() < 2.0 * TOLERANCE, "{h:?}");
        let Via::Collision { source, .. } = h.via else {
            panic!("a collision hit")
        };
        assert_eq!(h.reference, refs[source as usize]);
    }

    #[test]
    fn object_bounds_are_met_where_no_collision_is_nearer() {
        let w = world();
        let door = FormKey {
            plugin: "a.esm".into(),
            object_id: 0x20,
        };
        let mut boxes = Boxes::default();
        // A box 20 units deep at x = 60, turned a quarter about z and
        // scaled twice: its local x spans -5 to 5 (world y -10 to 10), its
        // local y -5 to 5 (world x 50 to 70).
        let frame = fo4_scene::transform::Transform::euler(
            [60.0, 0.0, 0.0, 0.0, 0.0, std::f32::consts::FRAC_PI_2],
            2.0,
        )
        .unwrap();
        assert!(boxes.push(door.clone(), frame, [-5, -5, -5, 5, 5, 5]));
        let h = first_of(
            &w,
            &[],
            Some(&boxes),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            500.0,
            &|_| false,
        )
        .unwrap();
        assert_eq!(h.via, Via::Bounds { index: 0 });
        assert!((h.distance - 50.0).abs() < 1e-4, "{h:?}");
        assert_eq!(h.reference, Some(door.clone()));
        // Skipped (disabled by the run): the wall behind it.
        let h = first_of(
            &w,
            &[],
            Some(&boxes),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            500.0,
            &|k| *k == door,
        )
        .unwrap();
        assert!((h.distance - 100.0).abs() < 1e-9);
        // Beside it: the box is missed.
        assert!(boxes
            .first([0.0, 15.0, 0.0], [1.0, 0.0, 0.0], 500.0, &|_| false)
            .is_none());
    }

    #[test]
    fn a_ray_from_inside_meets_at_once() {
        let w = world();
        let h = first(&w, &[], [50.0, 30.0, 5.0], [0.0, 1.0, 0.0], 10.0).unwrap();
        assert_eq!(h.distance, 0.0);
        assert_eq!(h.reference, None);
    }
}
