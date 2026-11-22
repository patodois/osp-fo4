//! Collision of a loaded cell: for every placed reference the scene resolves
//! to a model, each `bhkNPCollisionObject` → physics system → body → shape,
//! placed in world space.
//!
//! The stored geometry comes from `fo4-havok`, whose readers enforce only
//! what held on every stored object (`spec/havok-*.md`). How that geometry is
//! placed is not a format fact: every step is a registered rule
//! (`spec/collision-placement.md`, `spec/hypotheses.tsv`) and an instance is
//! returned only at a [`RuleLevel`] that accepts all the rules it needs.
//! A reference is followed when its collision readiness
//! ([`Instance::collision`], [`crate::readiness`]) accepts the level; blockers
//! that only the renderer reads do not stop it.
//! The rule on the body's own stored translation and rotation is open and
//! has two levels, [`BodyPlacement::Ignored`] and [`BodyPlacement::Applied`];
//! every instance can be evaluated under either ([`CellCollision::world`]).
pub mod model;

use crate::bounds::Bounds;
use crate::hypotheses::{self, Status};
use crate::transform::Transform;
use crate::{AssetKind, Base, DecodedAsset, Instance, Loader, RuleLevel, Scene, State};
use fo4_core::{Budget, Error, Limits, Result};
use fo4_plugin::FormKey;
pub use model::{BodyCollision, ModelCollision, ModelRefusal, RenderRange};
use serde::Serialize;
use std::collections::HashMap;
use std::ops::Range;

/// World units per collision unit, the default candidate of
/// `havok_collision_units_69_99125_v1`.
pub const UNITS: f64 = 69.99125;

/// The open rule on the body's stored translation and rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyPlacement {
    /// The shape sits in the Target frame as stored; the body's translation
    /// and rotation do not move it.
    Ignored,
    /// The body's translation and rotation (x, y, z, w) move the shape inside
    /// the Target frame, before the unit factor.
    Applied,
}
impl BodyPlacement {
    /// Candidate order: the level the first live run ranked best comes first.
    pub const ALL: [Self; 2] = [Self::Ignored, Self::Applied];
    pub fn rule(self) -> &'static str {
        match self {
            Self::Ignored => hypotheses::BODY_PLACEMENT_IGNORED,
            Self::Applied => hypotheses::BODY_PLACEMENT_APPLIED,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Ignored => "ignored",
            Self::Applied => "applied",
        }
    }
}

/// The level the registry selects: none at [`RuleLevel::Facts`]; at
/// [`RuleLevel::Measured`] the one level whose rule is `supported`; at
/// [`RuleLevel::Hypotheses`] a supported level, otherwise the first
/// pending one in [`BodyPlacement::ALL`]. A refuted level is never chosen.
pub fn default_body_placement(
    level: RuleLevel,
    status: impl Fn(&str) -> Option<Status>,
) -> Option<BodyPlacement> {
    let with = |wanted: Status| {
        let found: Vec<_> = BodyPlacement::ALL
            .into_iter()
            .filter(|v| status(v.rule()) == Some(wanted))
            .collect();
        found
    };
    let supported = with(Status::Supported);
    match level {
        RuleLevel::Facts => None,
        RuleLevel::Measured => (supported.len() == 1).then(|| supported[0]),
        RuleLevel::Hypotheses => match supported.len() {
            1 => Some(supported[0]),
            0 => with(Status::Pending).first().copied(),
            _ => None,
        },
    }
}

/// Stored shape classes this module follows from a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ShapeClass {
    CompressedMesh,
    ConvexPolytope,
    Convex,
    Sphere,
    Capsule,
    ScaledConvex,
    StaticCompound,
    DynamicCompound,
}
impl ShapeClass {
    pub const LEAVES: [Self; 5] = [
        Self::CompressedMesh,
        Self::ConvexPolytope,
        Self::Convex,
        Self::Sphere,
        Self::Capsule,
    ];
    pub fn class(self) -> &'static str {
        use fo4_havok as hk;
        match self {
            Self::CompressedMesh => hk::COMPRESSED_MESH,
            Self::ConvexPolytope => hk::CONVEX_POLYTOPE,
            Self::Convex => hk::CONVEX,
            Self::Sphere => hk::SPHERE,
            Self::Capsule => hk::CAPSULE,
            Self::ScaledConvex => hk::SCALED_CONVEX,
            Self::StaticCompound => hk::STATIC_COMPOUND,
            Self::DynamicCompound => hk::DYNAMIC_COMPOUND,
        }
    }
    pub fn from_class(name: &str) -> Option<Self> {
        [
            Self::CompressedMesh,
            Self::ConvexPolytope,
            Self::Convex,
            Self::Sphere,
            Self::Capsule,
            Self::ScaledConvex,
            Self::StaticCompound,
            Self::DynamicCompound,
        ]
        .into_iter()
        .find(|c| c.class() == name)
    }
}

/// One stored link between a body and a leaf, values as stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Step {
    /// A compound instance (rows, translation, three equal scale values).
    Instance {
        rotation: [[f32; 3]; 3],
        translation: [f32; 3],
        scale: [f32; 3],
    },
    /// `hknpScaledConvexShape`: scale and translation.
    Scaled {
        scale: [f32; 3],
        translation: [f32; 3],
    },
}
impl Step {
    /// The point in the parent shape's frame (`havok_compound_instance_v1`,
    /// `havok_scaled_convex_v1`).
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Instance { .. } => self.instance().apply(p),
            Self::Scaled { scale, translation } => {
                std::array::from_fn(|i| f64::from(scale[i]) * p[i] + f64::from(translation[i]))
            }
        }
    }
    /// A compound instance as a similarity: stored rows are the images of
    /// the child axes, the first scale value is the uniform scale.
    fn instance(&self) -> Transform {
        match self {
            Self::Instance {
                rotation,
                translation,
                scale,
            } => Transform {
                rotation: rotation.map(|r| r.map(f64::from)),
                translation: translation.map(f64::from),
                scale: f64::from(scale[0]),
            },
            Self::Scaled { .. } => Transform::IDENTITY,
        }
    }
    fn rule(&self) -> &'static str {
        match self {
            Self::Instance { .. } => hypotheses::COMPOUND_INSTANCE,
            Self::Scaled { .. } => hypotheses::SCALED_CONVEX,
        }
    }
}

/// A vertex record of a compressed mesh: points and a radius, no faces.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub kind: u8,
    pub radius: f32,
    /// Range in the mesh's `record_vertices`.
    pub vertices: Range<u32>,
}

/// Stored leaf geometry in collision units. Polytopes and convex shapes
/// expose their vertices only: their faces are not established.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Shape {
    Mesh {
        vertices: Vec<[f32; 3]>,
        /// [a, b, c] and, for quads, [a, c, d] (`spec/havok-mesh.md`).
        triangles: Vec<[u32; 3]>,
        records: Vec<Record>,
        record_vertices: Vec<[f32; 3]>,
        placeholders: usize,
    },
    Polytope {
        vertices: Vec<[f32; 3]>,
        /// f32 at byte 20 (`havok_convex_radius_added_v1`).
        radius: f32,
    },
    Convex {
        vertices: Vec<[f32; 3]>,
    },
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    Capsule {
        points: [[f32; 3]; 2],
        radius: f32,
    },
}

/// A shape a body reaches, with the links from the body to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Leaf {
    /// Havok object index of the leaf.
    pub object: usize,
    /// Classes from the body's shape down to the leaf.
    pub path: Vec<ShapeClass>,
    /// Links from the body down to the leaf, outermost first.
    pub steps: Vec<Step>,
    pub shape: Shape,
}
impl Leaf {
    pub fn class(&self) -> ShapeClass {
        self.path
            .last()
            .copied()
            .unwrap_or(ShapeClass::CompressedMesh)
    }
    /// Leaf coordinates to body coordinates.
    pub fn to_body(&self, p: [f32; 3]) -> [f64; 3] {
        self.steps
            .iter()
            .rev()
            .fold(p.map(f64::from), |q, s| s.apply(q))
    }
    /// Product of the uniform scales of the compound instances; scaled convex
    /// links are left out (their scale is per axis).
    pub fn uniform_scale(&self) -> f64 {
        self.steps
            .iter()
            .map(|s| match s {
                Step::Instance { scale, .. } => f64::from(scale[0]),
                Step::Scaled { .. } => 1.0,
            })
            .product()
    }
    pub fn per_axis_scaled(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Scaled { .. }))
    }
}

/// A model placed by one reference (and SCOL part placement).
#[derive(Debug, Clone, Serialize)]
pub struct ModelPlacement {
    pub reference: FormKey,
    /// The base whose model this is: the reference's base, or a SCOL part.
    pub base: FormKey,
    pub model: usize,
    /// Reference transform composed with any SCOL part placement: the
    /// model's world frame, as the render walk uses it.
    pub frame: Transform,
    /// Rules the reference's collision readiness relies on
    /// ([`Instance::collision`]).
    pub scene_rules: Vec<&'static str>,
}

/// One placed leaf shape.
#[derive(Debug, Clone, Serialize)]
pub struct CollisionInstance {
    pub placement: usize,
    /// Index into the model's `bodies`.
    pub body: usize,
    pub leaf: usize,
    pub kind: ShapeClass,
    /// Body coordinates (collision units) to world, under the selected
    /// body placement level. Apply [`Leaf::to_body`] first.
    pub world: Transform,
    /// Every rule this instance relies on, scene rules included.
    pub rules: Vec<&'static str>,
}

/// A reference, model, body or leaf that yields no placed shape.
#[derive(Debug, Clone, Serialize)]
pub struct Unplaced {
    pub reference: FormKey,
    pub base: Option<FormKey>,
    pub model: Option<usize>,
    pub collision_object: Option<usize>,
    pub system: Option<usize>,
    pub body: Option<usize>,
    pub leaf: Option<usize>,
    pub code: String,
    pub detail: String,
}

/// A placed model whose file holds no collision object.
#[derive(Debug, Clone, Serialize)]
pub struct NoCollision {
    pub reference: FormKey,
    pub base: FormKey,
    pub model: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub level: RuleLevel,
    /// `None` selects [`default_body_placement`] from the registry.
    pub body_placement: Option<BodyPlacement>,
}

#[derive(Debug, Serialize)]
pub struct CellCollision {
    pub level: RuleLevel,
    pub body_placement: Option<BodyPlacement>,
    pub models: Vec<ModelCollision>,
    pub placements: Vec<ModelPlacement>,
    pub instances: Vec<CollisionInstance>,
    /// References that reach no model, and bodies or leaves not placed.
    pub unplaced: Vec<Unplaced>,
    pub without_collision: Vec<NoCollision>,
}

/// World shape of one instance; radii are scaled by the uniform part of the
/// placement only.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum WorldShape {
    Mesh {
        vertices: Vec<[f64; 3]>,
        triangles: Vec<[u32; 3]>,
        /// Unit normal of each triangle from its stored winding,
        /// (b - a) x (c - a) in world space; zero for a degenerate one.
        /// Which side blocks is open: nothing here assumes both do.
        normals: Vec<[f64; 3]>,
        records: Vec<WorldRecord>,
    },
    /// Polytope or convex vertices, without faces. `radius`: the
    /// polytope's f32 at byte 20, which the surface adds
    /// (`havok_convex_radius_added_v1`); `None` for `hknpConvexShape`,
    /// whose radius is not read.
    Points {
        vertices: Vec<[f64; 3]>,
        radius: Option<f64>,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Capsule {
        points: [[f64; 3]; 2],
        radius: f64,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorldRecord {
    pub kind: u8,
    pub radius: f64,
    pub points: Vec<[f64; 3]>,
}

/// Unit normal (b - a) x (c - a) of a triangle in its stored winding; zero
/// when degenerate or not finite.
pub fn triangle_normal([a, b, c]: [[f64; 3]; 3]) -> [f64; 3] {
    let u: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
    let v: [f64; 3] = std::array::from_fn(|k| c[k] - a[k]);
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
    if len > 0.0 && len.is_finite() {
        n.map(|x| x / len)
    } else {
        [0.0; 3]
    }
}

/// The four f32 at body byte 64 as (x, y, z, w), w last, as rows of the
/// active rotation in the [`Transform`] convention (row j is the image of
/// axis j). The reading is part of `havok_body_placement_applied_v1`.
pub fn quaternion_rows(q: [f32; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = q.map(f64::from);
    let m = [
        [
            1. - 2. * (y * y + z * z),
            2. * (x * y - z * w),
            2. * (x * z + y * w),
        ],
        [
            2. * (x * y + z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z - x * w),
        ],
        [
            2. * (x * z - y * w),
            2. * (y * z + x * w),
            1. - 2. * (x * x + y * y),
        ],
    ];
    std::array::from_fn(|j| std::array::from_fn(|i| m[i][j]))
}

/// Largest deviation from the identity placement still counted as identity:
/// the census found stored identity quaternions within 1e-6
/// (`spec/havok-mesh.md`, Units), for example w = 0.99999994.
pub const IDENTITY_TOLERANCE: f32 = 1e-6;

/// Whether the body stores translation 0 and the four f32 (0, 0, 0, ±1),
/// each within [`IDENTITY_TOLERANCE`].
pub fn body_placement_is_identity(body: &fo4_havok::Body) -> bool {
    let t = IDENTITY_TOLERANCE;
    body.translation.iter().all(|x| x.abs() <= t)
        && body.rotation[..3].iter().all(|x| x.abs() <= t)
        && (body.rotation[3].abs() - 1.0).abs() <= t
}

/// Body coordinates (collision units) to world for a model `frame`.
pub fn body_world(frame: Transform, body: &BodyCollision, v: BodyPlacement) -> Transform {
    body_world_root(frame, body, v, false)
}
/// [`body_world`], with the root node's own transform left out when
/// `root_left_out` (`nif_root_transform_replaced_v1`).
pub fn body_world_root(
    frame: Transform,
    body: &BodyCollision,
    v: BodyPlacement,
    root_left_out: bool,
) -> Transform {
    let units = Transform {
        scale: UNITS,
        ..Transform::IDENTITY
    };
    let chain = if root_left_out {
        body.below_root
    } else {
        body.target_local
    };
    let t = frame.compose(chain).compose(units);
    match v {
        BodyPlacement::Ignored => t,
        BodyPlacement::Applied => t.compose(Transform {
            rotation: quaternion_rows(body.body.rotation),
            translation: body.body.translation.map(f64::from),
            scale: 1.0,
        }),
    }
}

/// Standing of a set of rules, as `hypotheses::classify` would give it.
pub fn rules_state(rules: &[&'static str], status: impl Fn(&str) -> Option<Status>) -> State {
    let mut pending = false;
    for r in rules {
        match status(r) {
            Some(Status::Supported) => {}
            Some(Status::Pending) => pending = true,
            Some(Status::Refuted) | None => return State::Blocked,
        }
    }
    if rules.is_empty() {
        State::Ready
    } else if pending {
        State::ReadyUnderHypotheses {
            hypotheses: rules.to_vec(),
        }
    } else {
        State::ReadyMeasured {
            rules: rules.to_vec(),
        }
    }
}

impl CellCollision {
    pub fn placement(&self, i: &CollisionInstance) -> &ModelPlacement {
        &self.placements[i.placement]
    }
    pub fn model(&self, i: &CollisionInstance) -> &ModelCollision {
        &self.models[self.placements[i.placement].model]
    }
    pub fn body(&self, i: &CollisionInstance) -> &BodyCollision {
        &self.model(i).bodies[i.body]
    }
    pub fn leaf(&self, i: &CollisionInstance) -> &Leaf {
        &self.body(i).leaves[i.leaf]
    }
    /// Body coordinates to world under level `v`.
    pub fn world(&self, i: &CollisionInstance, v: BodyPlacement) -> Transform {
        body_world_root(
            self.placement(i).frame,
            self.body(i),
            v,
            i.rules.contains(&hypotheses::ROOT_TRANSFORM),
        )
    }
    /// A leaf point in world coordinates under level `v`.
    pub fn world_point(&self, i: &CollisionInstance, v: BodyPlacement, p: [f32; 3]) -> [f64; 3] {
        self.world(i, v).apply(self.leaf(i).to_body(p))
    }
    /// World-space triangles of a compressed-mesh instance; empty otherwise.
    pub fn world_triangles(
        &self,
        i: &CollisionInstance,
        v: BodyPlacement,
    ) -> impl Iterator<Item = [[f64; 3]; 3]> + '_ {
        let world = self.world(i, v);
        let leaf = self.leaf(i);
        let (vertices, triangles): (&[[f32; 3]], &[[u32; 3]]) = match &leaf.shape {
            Shape::Mesh {
                vertices,
                triangles,
                ..
            } => (vertices, triangles),
            _ => (&[], &[]),
        };
        triangles.iter().filter_map(move |t| {
            let mut out = [[0.0; 3]; 3];
            for (k, index) in t.iter().enumerate() {
                out[k] = world.apply(leaf.to_body(*vertices.get(*index as usize)?));
            }
            Some(out)
        })
    }
    /// The instance's typed world view.
    pub fn world_shape(
        &self,
        i: &CollisionInstance,
        v: BodyPlacement,
        b: &Budget,
    ) -> Result<WorldShape> {
        let world = self.world(i, v);
        let leaf = self.leaf(i);
        let file = &self.model(i).path;
        let place = |p: [f32; 3]| world.apply(leaf.to_body(p));
        let scale = world.scale * leaf.uniform_scale();
        let points = |src: &[[f32; 3]]| -> Result<Vec<[f64; 3]>> {
            let mut out = Vec::new();
            b.reserve(&mut out, src.len(), file, 0)?;
            out.extend(src.iter().map(|p| place(*p)));
            Ok(out)
        };
        Ok(match &leaf.shape {
            Shape::Mesh {
                vertices,
                triangles,
                records,
                record_vertices,
                ..
            } => {
                let mut t = Vec::new();
                b.reserve(&mut t, triangles.len(), file, 0)?;
                t.extend_from_slice(triangles);
                let mut r = Vec::new();
                b.reserve(&mut r, records.len(), file, 0)?;
                for m in records {
                    let src = record_vertices
                        .get(m.vertices.start as usize..m.vertices.end as usize)
                        .unwrap_or(&[]);
                    r.push(WorldRecord {
                        kind: m.kind,
                        radius: f64::from(m.radius) * scale,
                        points: points(src)?,
                    });
                }
                let vertices = points(vertices)?;
                let mut normals = Vec::new();
                b.reserve(&mut normals, t.len(), file, 0)?;
                normals.extend(t.iter().map(|tri| {
                    let at = |k: usize| vertices.get(tri[k] as usize).copied().unwrap_or([0.0; 3]);
                    triangle_normal([at(0), at(1), at(2)])
                }));
                WorldShape::Mesh {
                    vertices,
                    triangles: t,
                    normals,
                    records: r,
                }
            }
            Shape::Polytope { vertices, radius } => WorldShape::Points {
                vertices: points(vertices)?,
                radius: Some(f64::from(*radius) * scale),
            },
            Shape::Convex { vertices } => WorldShape::Points {
                vertices: points(vertices)?,
                radius: None,
            },
            Shape::Sphere { center, radius } => WorldShape::Sphere {
                center: place(*center),
                radius: f64::from(*radius) * scale,
            },
            Shape::Capsule { points: p, radius } => WorldShape::Capsule {
                points: [place(p[0]), place(p[1])],
                radius: f64::from(*radius) * scale,
            },
        })
    }
    /// World box of an instance under level `v`: vertices, record points
    /// widened by their radius, spheres and capsules widened by theirs.
    pub fn world_bounds(&self, i: &CollisionInstance, v: BodyPlacement) -> Option<Bounds> {
        let world = self.world(i, v);
        let leaf = self.leaf(i);
        let scale = world.scale * leaf.uniform_scale();
        let mut bx: Option<Bounds> = None;
        let mut grow = |p: [f64; 3], r: f64| {
            if p.iter().any(|x| !x.is_finite()) {
                return;
            }
            let lo = p.map(|x| x - r);
            let hi = p.map(|x| x + r);
            match &mut bx {
                None => bx = Some(Bounds { min: lo, max: hi }),
                Some(b) => {
                    for k in 0..3 {
                        b.min[k] = b.min[k].min(lo[k]);
                        b.max[k] = b.max[k].max(hi[k]);
                    }
                }
            }
        };
        let place = |p: [f32; 3]| world.apply(leaf.to_body(p));
        match &leaf.shape {
            Shape::Mesh {
                vertices,
                records,
                record_vertices,
                ..
            } => {
                for p in vertices {
                    grow(place(*p), 0.0);
                }
                for m in records {
                    let r = f64::from(m.radius) * scale;
                    for p in record_vertices
                        .get(m.vertices.start as usize..m.vertices.end as usize)
                        .unwrap_or(&[])
                    {
                        grow(place(*p), r);
                    }
                }
            }
            Shape::Polytope { vertices, radius } => {
                for p in vertices {
                    grow(place(*p), f64::from(*radius) * scale);
                }
            }
            Shape::Convex { vertices } => {
                for p in vertices {
                    grow(place(*p), 0.0);
                }
            }
            Shape::Sphere { center, radius } => grow(place(*center), f64::from(*radius) * scale),
            Shape::Capsule { points, radius } => {
                for p in points {
                    grow(place(*p), f64::from(*radius) * scale);
                }
            }
        }
        bx
    }
}

struct Builder<'l, 'a> {
    loader: &'l Loader<'a>,
    limits: &'l Limits,
    budget: &'l Budget,
    level: RuleLevel,
    placement: Option<BodyPlacement>,
    status: &'l dyn Fn(&str) -> Option<Status>,
    cache: HashMap<usize, usize>,
    out: CellCollision,
}

fn append<T>(v: &mut Vec<T>, item: T, b: &Budget) -> Result<()> {
    b.reserve(v, 1, "cell collision", 0)?;
    v.push(item);
    Ok(())
}

impl Builder<'_, '_> {
    fn skip(
        &mut self,
        i: &Instance,
        base: Option<&Base>,
        code: &str,
        detail: String,
    ) -> Result<()> {
        self.budget.charge(
            (code.len() + detail.len() + 128) as u64,
            "cell collision",
            0,
        )?;
        append(
            &mut self.out.unplaced,
            Unplaced {
                reference: i.reference.key.clone(),
                base: base.map(|b| b.key.clone()),
                model: None,
                collision_object: None,
                system: None,
                body: None,
                leaf: None,
                code: code.to_owned(),
                detail,
            },
            self.budget,
        )
    }

    fn model(&mut self, asset: usize) -> Result<usize> {
        if let Some(m) = self.cache.get(&asset) {
            return Ok(*m);
        }
        let path = String::from_utf8_lossy(&self.loader.assets.nodes[asset].path).into_owned();
        let scratch = self.budget.child();
        let limits = self.limits;
        let keep = self.budget;
        let decoded = self.loader.assets.with_decoded(asset, &scratch, |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(Error::new("CollisionAssetKind", &path, 0, "model", asset));
            };
            model::decode(&n, asset, &path, limits, keep)
        });
        drop(scratch);
        let m = match decoded {
            Ok(m) => m,
            Err(e) if e.code == "LimitExceeded" && self.budget.remaining() == 0 => return Err(e),
            Err(e) => ModelCollision {
                asset,
                path,
                collision_objects: 0,
                systems: 0,
                bodies: Vec::new(),
                refusals: Vec::new(),
                error: Some(e),
                render_points: Vec::new(),
                render: Vec::new(),
                render_error: None,
            },
        };
        let index = self.out.models.len();
        append(&mut self.out.models, m, self.budget)?;
        self.cache.insert(asset, index);
        Ok(index)
    }

    fn base(&mut self, i: &Instance, base: &Base, frame: Transform, depth: u64) -> Result<()> {
        fo4_core::limit(depth, self.limits.max_depth, "cell collision", 0)?;
        let parts = base.record.signature == *b"SCOL" && !base.parts.is_empty();
        if parts && !i.collision.hypotheses().contains(&hypotheses::SCOL) {
            return self.skip(
                i,
                Some(base),
                "UnverifiedScolGeometrySource",
                "constituent placements need scol_parts_static_frame_v1".into(),
            );
        }
        if !parts {
            let models: Vec<usize> = base
                .assets
                .iter()
                .copied()
                .filter(|a| self.loader.assets.nodes[*a].kind == AssetKind::Model)
                .collect();
            if models.is_empty() {
                return self.skip(i, Some(base), "NoModel", "base cites no model".into());
            }
            for asset in models {
                let m = self.model(asset)?;
                self.place(i, base, m, frame)?;
            }
        }
        for part in &base.parts {
            let Some(resolved) = part.resolved.as_ref() else {
                self.skip(i, Some(base), "MissingScolPartBase", String::new())?;
                continue;
            };
            let Some(placements) = part.declared.placements.as_ref() else {
                self.skip(i, Some(base), "MissingScolPartPlacements", String::new())?;
                continue;
            };
            for p in placements {
                match Transform::euler([p[0], p[1], p[2], p[3], p[4], p[5]], p[6]) {
                    Ok(local) => self.base(i, resolved, frame.compose(local), depth + 1)?,
                    Err(e) => self.skip(i, Some(resolved), &e.code, e.found)?,
                }
            }
        }
        Ok(())
    }

    fn place(&mut self, i: &Instance, base: &Base, m: usize, frame: Transform) -> Result<()> {
        let model = &self.out.models[m];
        let unplaced =
            |collision_object, system, body, leaf, code: &str, detail: String| Unplaced {
                reference: i.reference.key.clone(),
                base: Some(base.key.clone()),
                model: Some(m),
                collision_object,
                system,
                body,
                leaf,
                code: code.to_owned(),
                detail,
            };
        let mut rows = Vec::new();
        if let Some(e) = &model.error {
            append(
                &mut rows,
                unplaced(None, None, None, None, &e.code, e.found.clone()),
                self.budget,
            )?;
        } else if !model.has_collision() {
            append(
                &mut self.out.without_collision,
                NoCollision {
                    reference: i.reference.key.clone(),
                    base: base.key.clone(),
                    model: m,
                },
                self.budget,
            )?;
            return Ok(());
        }
        for r in &model.refusals {
            append(
                &mut rows,
                unplaced(
                    r.collision_object,
                    r.system,
                    r.body,
                    None,
                    &r.code,
                    r.detail.clone(),
                ),
                self.budget,
            )?;
        }
        let placement = self.out.placements.len();
        let mut instances = Vec::new();
        for (bi, body) in model.bodies.iter().enumerate() {
            let Some(v) = self.placement else {
                append(
                    &mut rows,
                    unplaced(
                        Some(body.collision_object),
                        Some(body.system),
                        Some(body.body.index),
                        None,
                        "BodyPlacementRuleNotAccepted",
                        format!("no body placement level is selectable at {:?}", self.level),
                    ),
                    self.budget,
                )?;
                continue;
            };
            // The root node's own transform is left out as the renderer
            // leaves it out, under the same rule and level.
            let root_left_out = body.root_moves
                && match (self.status)(hypotheses::ROOT_TRANSFORM) {
                    Some(Status::Supported) => self.level != RuleLevel::Facts,
                    Some(Status::Pending) => self.level == RuleLevel::Hypotheses,
                    _ => false,
                };
            let world = body_world_root(frame, body, v, root_left_out);
            for (li, leaf) in body.leaves.iter().enumerate() {
                let mut rules = Vec::new();
                let mut add = |r: &'static str, b: &Budget| -> Result<()> {
                    if !rules.contains(&r) {
                        append(&mut rules, r, b)?;
                    }
                    Ok(())
                };
                for r in i.collision.hypotheses() {
                    add(r, self.budget)?;
                }
                for r in [
                    hypotheses::COLLISION_UNITS,
                    hypotheses::TARGET_FRAME,
                    hypotheses::BODY_ID,
                    v.rule(),
                ] {
                    add(r, self.budget)?;
                }
                for s in &leaf.steps {
                    add(s.rule(), self.budget)?;
                }
                if root_left_out {
                    add(hypotheses::ROOT_TRANSFORM, self.budget)?;
                }
                match leaf.shape {
                    Shape::Mesh { .. } => add(hypotheses::MESH_TRIANGLES, self.budget)?,
                    Shape::Polytope { .. } => add(hypotheses::CONVEX_RADIUS, self.budget)?,
                    _ => {}
                }
                rules.sort_unstable();
                let state = rules_state(&rules, self.status);
                if !state.ready_at(self.level) {
                    let open: Vec<&str> = rules
                        .iter()
                        .copied()
                        .filter(|r| (self.status)(r) != Some(Status::Supported))
                        .collect();
                    append(
                        &mut rows,
                        unplaced(
                            Some(body.collision_object),
                            Some(body.system),
                            Some(body.body.index),
                            Some(li),
                            if matches!(state, State::Blocked) {
                                "CollisionRuleRefuted"
                            } else {
                                "CollisionRuleNotAccepted"
                            },
                            format!("{} at {:?}", open.join(","), self.level),
                        ),
                        self.budget,
                    )?;
                    continue;
                }
                append(
                    &mut instances,
                    CollisionInstance {
                        placement,
                        body: bi,
                        leaf: li,
                        kind: leaf.class(),
                        world,
                        rules,
                    },
                    self.budget,
                )?;
            }
        }
        for r in rows {
            self.budget.charge(
                (r.code.len() + r.detail.len() + 128) as u64,
                "cell collision",
                0,
            )?;
            append(&mut self.out.unplaced, r, self.budget)?;
        }
        if model.bodies.is_empty() {
            return Ok(());
        }
        append(
            &mut self.out.placements,
            ModelPlacement {
                reference: i.reference.key.clone(),
                base: base.key.clone(),
                model: m,
                frame,
                scene_rules: i.collision.hypotheses().to_vec(),
            },
            self.budget,
        )?;
        self.budget.reserve(
            &mut self.out.instances,
            instances.len(),
            "cell collision",
            0,
        )?;
        self.out.instances.extend(instances);
        Ok(())
    }

    fn instance(&mut self, i: &Instance) -> Result<()> {
        let base = i.base.as_ref();
        if i.editor_marker {
            return self.skip(
                i,
                base,
                "EditorMarkerReference",
                "collision of editor markers is not placed".into(),
            );
        }
        if !i.collision.ready_at(self.level) {
            return self.skip(
                i,
                base,
                "CollisionNotReady",
                format!("{:?} at {:?}", i.collision, self.level),
            );
        }
        if i.disabled_by_enable_parent() {
            return self.skip(
                i,
                base,
                "DisabledThroughEnableParent",
                hypotheses::ENABLE_FOLLOWS.into(),
            );
        }
        let Some(base) = base else {
            return self.skip(i, None, "MissingBase", String::new());
        };
        let Some(p) = i.reference.data.transform else {
            return self.skip(
                i,
                Some(base),
                "MissingReferenceTransform",
                "DATA absent".into(),
            );
        };
        let scale = match i.reference.data.scale {
            Some(s) => s,
            None if i.collision.hypotheses().contains(&hypotheses::SCALE) => 1.0,
            None => {
                return self.skip(
                    i,
                    Some(base),
                    "MissingDocumentedReferenceScale",
                    "XSCL absent without registered hypothesis".into(),
                )
            }
        };
        let frame = match Transform::euler(p, scale) {
            Ok(t) => t,
            Err(e) => return self.skip(i, Some(base), &e.code, e.found),
        };
        self.base(i, base, frame, 0)
    }
}

/// Collision of `scene` at `options.level`, with the rule standings of the
/// embedded registry.
pub fn cell(
    scene: &Scene,
    loader: &Loader<'_>,
    limits: &Limits,
    options: Options,
    b: &Budget,
) -> Result<CellCollision> {
    cell_with(scene, loader, limits, options, &hypotheses::status, b)
}

/// [`cell`] against an explicit status table.
pub fn cell_with(
    scene: &Scene,
    loader: &Loader<'_>,
    limits: &Limits,
    options: Options,
    status: &dyn Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<CellCollision> {
    let placement = match options.body_placement {
        Some(v) => Some(v),
        None => default_body_placement(options.level, status),
    };
    let mut builder = Builder {
        loader,
        limits,
        budget: b,
        level: options.level,
        placement,
        status,
        cache: HashMap::new(),
        out: CellCollision {
            level: options.level,
            body_placement: placement,
            models: Vec::new(),
            placements: Vec::new(),
            instances: Vec::new(),
            unplaced: Vec::new(),
            without_collision: Vec::new(),
        },
    };
    let mut order: Vec<&Instance> = Vec::new();
    b.reserve(&mut order, scene.instances.len(), "cell collision", 0)?;
    order.extend(scene.instances.iter());
    order.sort_by(|a, b| {
        (&a.reference.key.plugin, a.reference.key.object_id)
            .cmp(&(&b.reference.key.plugin, b.reference.key.object_id))
    });
    for i in order {
        builder.instance(i)?;
    }
    Ok(builder.out)
}

#[cfg(test)]
mod tests;
