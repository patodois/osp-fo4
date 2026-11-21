//! One model's collision, in the model's own frame: every
//! `bhkNPCollisionObject`, the node chain from a root to its Target, the body
//! its Body ID names and every shape that body reaches. Shape coordinates
//! stay in the payload's own units; nothing here applies a placement rule.
use super::{Leaf, Record, Shape, ShapeClass, Step};
use crate::model_policy::{
    block_class, editor_marker_object, editor_marker_suffix, fields, visibility_editor_marker,
};
use crate::transform::Transform;
use fo4_core::{Budget, Error, Limits, Result};
use fo4_havok as hk;
use fo4_nif::{Data, Nif, SystemKind, Value};
use fo4_plugin::static_frame::Class;
use serde::Serialize;
use std::ops::Range;

/// Relative tolerance for the three stored scale values of a compound
/// instance to count as one uniform scale.
pub const UNIFORM_SCALE_TOLERANCE: f64 = 1e-6;
/// Deepest shape link chain followed from a body (compound, scaled convex).
pub const MAX_SHAPE_DEPTH: u64 = 8;

/// A body that a collision object of this model reaches, with its leaves.
#[derive(Debug, Clone, Serialize)]
pub struct BodyCollision {
    /// NIF block of the `bhkNPCollisionObject`.
    pub collision_object: usize,
    /// NIF block of its Target node.
    pub target: usize,
    pub target_name: Option<String>,
    /// Flags of the collision object, as stored.
    pub flags: u16,
    /// NIF block of the `bhkPhysicsSystem`.
    pub system: usize,
    pub body_id: u32,
    /// The body element Body ID selects (`nif_collision_body_id_selects_body_v1`).
    pub body: hk::Body,
    /// Composition of the node transforms from a root to the Target, Target
    /// included, in the order the render walk composes them.
    pub target_local: Transform,
    /// Blocks of the chain, root first, Target last.
    pub chain: Vec<usize>,
    /// A node of the chain is one whose subtree the renderer hides (editor
    /// marker, numeric marker label) or a block class outside the static frame.
    pub hidden_chain: bool,
    pub leaves: Vec<Leaf>,
    /// Indices into [`ModelCollision::render`] of the render meshes inside the
    /// Target's subtree.
    pub render: Vec<usize>,
}

/// A collision object or body of this model that yields no placed shape.
#[derive(Debug, Clone, Serialize)]
pub struct ModelRefusal {
    pub collision_object: Option<usize>,
    pub system: Option<usize>,
    pub body: Option<usize>,
    pub code: String,
    pub detail: String,
}

/// Render mesh positions in the model frame, as the render walk places them.
#[derive(Debug, Clone, Serialize)]
pub struct RenderRange {
    pub block: usize,
    /// Blocks from a root down to the mesh, mesh included.
    pub ancestors: Vec<usize>,
    pub points: Range<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCollision {
    pub asset: usize,
    pub path: String,
    /// Collision objects stored in the file.
    pub collision_objects: usize,
    /// Physics and ragdoll systems stored in the file.
    pub systems: usize,
    pub bodies: Vec<BodyCollision>,
    pub refusals: Vec<ModelRefusal>,
    /// The collision view of the file failed.
    pub error: Option<fo4_core::Error>,
    #[serde(skip)]
    pub render_points: Vec<[f64; 3]>,
    pub render: Vec<RenderRange>,
    pub render_error: Option<fo4_core::Error>,
}

impl ModelCollision {
    pub fn has_collision(&self) -> bool {
        self.collision_objects > 0
    }
    /// Model-frame render positions inside the Target subtree of `body`.
    pub fn render_points(&self, body: &BodyCollision) -> impl Iterator<Item = [f64; 3]> + '_ {
        let ranges: Vec<Range<u32>> = body
            .render
            .iter()
            .filter_map(|r| self.render.get(*r).map(|r| r.points.clone()))
            .collect();
        ranges.into_iter().flat_map(move |r| {
            self.render_points
                .get(r.start as usize..r.end as usize)
                .unwrap_or(&[])
                .iter()
                .copied()
        })
    }
}

fn name(n: &Nif<'_>, index: usize) -> Option<String> {
    let data = n.blocks.get(index)?.decoded.as_ref()?;
    fields(data).iter().find_map(|f| match (f.name, &f.value) {
        ("Name", Value::StringIndex(i)) => n
            .strings
            .get(*i as usize)
            .map(|s| String::from_utf8_lossy(s).into_owned()),
        _ => None,
    })
}

fn hidden(n: &Nif<'_>, index: usize) -> bool {
    let Some(block) = n.blocks.get(index) else {
        return true;
    };
    let ty = n
        .block_types
        .get(block.type_index as usize)
        .map_or("", String::as_str);
    visibility_editor_marker(n, index)
        || editor_marker_object(n, index)
        || editor_marker_suffix(n, index)
        || matches!(block_class(ty), Class::Outside | Class::State)
}

/// The render walk's own composition for one block: the stored transform
/// when the block has a `Translation` field, otherwise the parent's.
fn local(n: &Nif<'_>, index: usize, parent: Transform, file: &str) -> Result<Transform> {
    let Some(data) = n.blocks.get(index).and_then(|b| b.decoded.as_ref()) else {
        return Ok(parent);
    };
    let f = fields(data);
    if f.iter().any(|f| f.name == "Translation") {
        Ok(parent.compose(Transform::from_fields(f, file)?))
    } else {
        Ok(parent)
    }
}

/// Parent of every node child link; a block with two parents maps to
/// `Err(())`.
fn parents(
    n: &Nif<'_>,
    file: &str,
    b: &Budget,
) -> Result<Vec<Option<std::result::Result<usize, ()>>>> {
    let mut out = Vec::new();
    b.reserve(&mut out, n.blocks.len(), file, 0)?;
    out.resize(n.blocks.len(), None);
    for (i, block) in n.blocks.iter().enumerate() {
        if let Some(Data::Node { children, .. }) = &block.decoded {
            for c in children.iter().filter(|c| **c >= 0) {
                if let Some(slot) = out.get_mut(*c as usize) {
                    *slot = Some(match slot {
                        None => Ok(i),
                        Some(Ok(p)) if *p == i => Ok(i),
                        _ => Err(()),
                    });
                }
            }
        }
    }
    Ok(out)
}

fn refusal(
    object: Option<usize>,
    system: Option<usize>,
    body: Option<usize>,
    code: &str,
    detail: impl ToString,
) -> ModelRefusal {
    ModelRefusal {
        collision_object: object,
        system,
        body,
        code: code.to_owned(),
        detail: detail.to_string(),
    }
}

fn push<T>(v: &mut Vec<T>, item: T, b: &Budget, file: &str) -> Result<()> {
    b.reserve(v, 1, file, 0)?;
    v.push(item);
    Ok(())
}

/// Decodes the collision of one model. `keep` pays for what the result
/// retains; decoding scratch is released before returning.
pub fn decode(
    n: &Nif<'_>,
    asset: usize,
    file: &str,
    limits: &Limits,
    keep: &Budget,
) -> Result<ModelCollision> {
    let mut out = ModelCollision {
        asset,
        path: file.to_owned(),
        collision_objects: 0,
        systems: 0,
        bodies: Vec::new(),
        refusals: Vec::new(),
        error: None,
        render_points: Vec::new(),
        render: Vec::new(),
        render_error: None,
    };
    keep.charge(file.len() as u64 + 256, file, 0)?;
    let scratch = keep.child();
    let col = match n.collision(file, &scratch) {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(e);
            return Ok(out);
        }
    };
    out.collision_objects = col.objects.len();
    out.systems = col.systems.len();
    if col.objects.is_empty() {
        return Ok(out);
    }
    let up = parents(n, file, &scratch)?;
    let mut walk = RenderWalk {
        n,
        file,
        limits,
        keep,
        visits: 0,
        stack: Vec::new(),
        out: &mut out,
    };
    let walked = n
        .roots
        .iter()
        .filter(|r| **r >= 0)
        .try_for_each(|r| walk.visit(*r as usize, Transform::IDENTITY, 0));
    if let Err(e) = walked {
        out.render_error = Some(e);
        out.render.clear();
        out.render_points.clear();
    }
    // Containers and systems are read once per bhkSystem block.
    let mut systems: Vec<(usize, std::result::Result<SystemRead<'_>, Error>)> = Vec::new();
    let mut objects: Vec<&fo4_nif::CollisionObject> = col.objects.iter().collect();
    objects.sort_by_key(|o| o.block);
    for o in objects {
        let system = col.system(o.data);
        let Some(system) = system else {
            push(
                &mut out.refusals,
                refusal(
                    Some(o.block),
                    None,
                    None,
                    "CollisionObjectWithoutSystem",
                    o.data,
                ),
                keep,
                file,
            )?;
            continue;
        };
        if system.kind == SystemKind::Ragdoll {
            push(
                &mut out.refusals,
                refusal(
                    Some(o.block),
                    Some(system.block),
                    None,
                    "RagdollSystem",
                    "bhkRagdollSystem bodies are not placed",
                ),
                keep,
                file,
            )?;
            continue;
        }
        if !systems.iter().any(|(b, _)| *b == system.block) {
            let read = SystemRead::read(system.payload, file, system.payload_offset, &scratch);
            push(&mut systems, (system.block, read), &scratch, file)?;
        }
        let Some((_, read)) = systems.iter().find(|(b, _)| *b == system.block) else {
            continue;
        };
        let read = match read {
            Ok(r) => r,
            Err(e) => {
                push(
                    &mut out.refusals,
                    refusal(
                        Some(o.block),
                        Some(system.block),
                        None,
                        &e.code,
                        format!(
                            "at {}: expected {}, found {}",
                            e.source.offset, e.expected, e.found
                        ),
                    ),
                    keep,
                    file,
                )?;
                continue;
            }
        };
        let Some(body) = read.system.bodies.get(o.body_id as usize).copied() else {
            push(
                &mut out.refusals,
                refusal(
                    Some(o.block),
                    Some(system.block),
                    None,
                    "BodyIdOutOfRange",
                    format!(
                        "Body ID {} of {} bodies",
                        o.body_id,
                        read.system.bodies.len()
                    ),
                ),
                keep,
                file,
            )?;
            continue;
        };
        let reject = |code: &str, detail: String| {
            refusal(
                Some(o.block),
                Some(system.block),
                Some(body.index),
                code,
                detail,
            )
        };
        let chain = match chain(n, &up, o.target, file, limits, keep) {
            Ok(c) => c,
            Err(e) => {
                push(&mut out.refusals, reject(&e.code, e.found), keep, file)?;
                continue;
            }
        };
        let mut target_local = Transform::IDENTITY;
        let mut composed = Ok(());
        for b in &chain {
            match local(n, *b, target_local, file) {
                Ok(t) => target_local = t,
                Err(e) => {
                    composed = Err(e);
                    break;
                }
            }
        }
        if let Err(e) = composed {
            push(&mut out.refusals, reject(&e.code, e.found), keep, file)?;
            continue;
        }
        let mut leaves = Vec::new();
        let mut steps = Vec::new();
        let mut path = Vec::new();
        let resolved = read.leaves(
            body.shape,
            &mut Resolve {
                file,
                base: system.payload_offset,
                keep,
                scratch: &scratch,
                steps: &mut steps,
                path: &mut path,
                out: &mut leaves,
            },
            0,
        );
        if let Err(e) = resolved {
            push(
                &mut out.refusals,
                reject(
                    &e.code,
                    format!(
                        "at {}: expected {}, found {}",
                        e.source.offset, e.expected, e.found
                    ),
                ),
                keep,
                file,
            )?;
            continue;
        }
        let target = chain.last().copied().unwrap_or(0);
        let mut render = Vec::new();
        for (k, r) in out.render.iter().enumerate() {
            if r.ancestors.contains(&target) {
                push(&mut render, k, keep, file)?;
            }
        }
        let hidden_chain = chain.iter().any(|b| hidden(n, *b));
        let body_entry = BodyCollision {
            collision_object: o.block,
            target,
            target_name: name(n, target),
            flags: o.flags,
            system: system.block,
            body_id: o.body_id,
            body,
            target_local,
            chain,
            hidden_chain,
            leaves,
            render,
        };
        push(&mut out.bodies, body_entry, keep, file)?;
    }
    // Bodies of a physics system that no collision object names.
    for (block, read) in &systems {
        let Ok(read) = read else { continue };
        for body in &read.system.bodies {
            let named = col
                .objects
                .iter()
                .any(|o| o.data == *block as i32 && o.body_id as usize == body.index);
            if !named {
                push(
                    &mut out.refusals,
                    refusal(
                        None,
                        Some(*block),
                        Some(body.index),
                        "BodyWithoutCollisionObject",
                        "no bhkNPCollisionObject names this body",
                    ),
                    keep,
                    file,
                )?;
            }
        }
    }
    Ok(out)
}

/// Blocks from a root down to `target`, following the node child links up.
fn chain(
    n: &Nif<'_>,
    up: &[Option<std::result::Result<usize, ()>>],
    target: i32,
    file: &str,
    limits: &Limits,
    keep: &Budget,
) -> Result<Vec<usize>> {
    let err = |code: &str, found: String| Error::new(code, file, 0, "a single node chain", found);
    if target < 0 || target as usize >= n.blocks.len() {
        return Err(err("InvalidCollisionTarget", format!("Target {target}")));
    }
    let mut out = Vec::new();
    let mut at = target as usize;
    loop {
        fo4_core::limit(out.len() as u64, limits.max_depth, file, 0)?;
        if out.contains(&at) {
            return Err(err("CollisionTargetCycle", format!("block {at}")));
        }
        push(&mut out, at, keep, file)?;
        match up.get(at).copied().flatten() {
            None => break,
            Some(Ok(p)) => at = p,
            Some(Err(())) => {
                return Err(err(
                    "AmbiguousCollisionTargetChain",
                    format!("block {at} is a child of two nodes"),
                ))
            }
        }
    }
    let root = out.last().copied().unwrap_or(0);
    if !n.roots.iter().any(|r| *r >= 0 && *r as usize == root) {
        return Err(err(
            "CollisionTargetNotUnderRoot",
            format!("chain ends at block {root}, not a footer root"),
        ));
    }
    out.reverse();
    Ok(out)
}

struct RenderWalk<'a, 'n> {
    n: &'a Nif<'n>,
    file: &'a str,
    limits: &'a Limits,
    keep: &'a Budget,
    visits: u64,
    stack: Vec<usize>,
    out: &'a mut ModelCollision,
}
impl RenderWalk<'_, '_> {
    /// The render walk's geometry under hypotheses: hidden marker subtrees
    /// and blocks outside the static frame are left out, as the renderer
    /// does when the marker rules are accepted.
    fn visit(&mut self, i: usize, parent: Transform, depth: u64) -> Result<()> {
        fo4_core::limit(depth, self.limits.max_depth, self.file, 0)?;
        self.visits = fo4_core::add(self.visits, 1, self.file, 0)?;
        fo4_core::limit(self.visits, self.limits.max_entries, self.file, 0)?;
        if self.stack.contains(&i) {
            return Err(Error::new(
                "RenderNodeCycle",
                self.file,
                0,
                "acyclic nodes",
                i,
            ));
        }
        if hidden(self.n, i) {
            return Ok(());
        }
        let Some(data) = self.n.blocks.get(i).and_then(|b| b.decoded.as_ref()) else {
            return Ok(());
        };
        let world = local(self.n, i, parent, self.file)?;
        push(&mut self.stack, i, self.keep, self.file)?;
        let result = (|| {
            if let Data::Node { children, .. } = data {
                for c in children.iter().filter(|c| **c >= 0) {
                    self.visit(*c as usize, world, depth + 1)?;
                }
                return Ok(());
            }
            let mesh = match data {
                Data::Mesh(m) => Some(m),
                _ => fields(data)
                    .iter()
                    .find_map(|f| match (f.name, &f.value) {
                        ("Data", Value::Ref(r)) if *r >= 0 => Some(*r as usize),
                        _ => None,
                    })
                    .and_then(
                        |r| match self.n.blocks.get(r).and_then(|b| b.decoded.as_ref()) {
                            Some(Data::Mesh(m)) => Some(m),
                            _ => None,
                        },
                    ),
            };
            let Some(mesh) = mesh else { return Ok(()) };
            let Some(vertices) = mesh.vertices.as_ref() else {
                return Ok(());
            };
            let start = self.out.render_points.len();
            self.keep
                .reserve(&mut self.out.render_points, vertices.len(), self.file, 0)?;
            for p in vertices.iter().filter_map(|v| v.position) {
                let w = world.apply(p.map(f64::from));
                if w.iter().all(|x| x.is_finite()) {
                    self.out.render_points.push(w);
                }
            }
            let end = self.out.render_points.len();
            let to_u32 = |x: usize| {
                u32::try_from(x)
                    .map_err(|_| Error::new("RenderPointLimit", self.file, 0, "u32 positions", x))
            };
            let points = to_u32(start)?..to_u32(end)?;
            let mut ancestors = Vec::new();
            self.keep
                .reserve(&mut ancestors, self.stack.len(), self.file, 0)?;
            ancestors.extend_from_slice(&self.stack);
            push(
                &mut self.out.render,
                RenderRange {
                    block: i,
                    ancestors,
                    points,
                },
                self.keep,
                self.file,
            )
        })();
        self.stack.pop();
        result
    }
}

struct SystemRead<'a> {
    container: hk::Container<'a>,
    system: hk::PhysicsSystem<'a>,
}

struct Resolve<'r> {
    file: &'r str,
    base: u64,
    keep: &'r Budget,
    scratch: &'r Budget,
    steps: &'r mut Vec<Step>,
    path: &'r mut Vec<ShapeClass>,
    out: &'r mut Vec<Leaf>,
}

impl<'a> SystemRead<'a> {
    fn read(payload: &'a [u8], file: &str, base: u64, b: &Budget) -> Result<Self> {
        let container = hk::Container::read(payload, file, base, b)?;
        let root = container.header.root_offset();
        let index = container
            .objects
            .iter()
            .position(|o| o.offset == root)
            .ok_or_else(|| {
                Error::new(
                    "MissingHavokRootObject",
                    file,
                    base,
                    "an object at the root offset",
                    root,
                )
            })?;
        let system = hk::PhysicsSystem::read(&container, index, file, base, b)?;
        Ok(Self { container, system })
    }

    fn leaves(&self, index: usize, r: &mut Resolve<'_>, depth: u64) -> Result<()> {
        fo4_core::limit(depth, MAX_SHAPE_DEPTH, r.file, r.base)?;
        let c = &self.container;
        let class = c.objects.get(index).map(|o| o.class).ok_or_else(|| {
            Error::new("InvalidHavokObjectSpan", r.file, r.base, "an object", index)
        })?;
        let class = ShapeClass::from_class(class).ok_or_else(|| {
            Error::new(
                "UnsupportedCollisionShapeClass",
                r.file,
                r.base,
                "a shape class with a reader",
                class,
            )
        })?;
        push(r.path, class, r.scratch, r.file)?;
        let result = (|| match class {
            ShapeClass::ScaledConvex => {
                let s = hk::ScaledConvex::read(c, index, r.file, r.base)?;
                push(
                    r.steps,
                    Step::Scaled {
                        scale: s.scale,
                        translation: s.translation,
                    },
                    r.scratch,
                    r.file,
                )?;
                let inner = self.leaves(s.child, r, depth + 1);
                r.steps.pop();
                inner
            }
            ShapeClass::StaticCompound | ShapeClass::DynamicCompound => {
                let compound = hk::Compound::read(c, index, r.file, r.base, r.scratch)?;
                for (k, inst) in compound.instances.iter().enumerate() {
                    let s = inst.scale.map(f64::from);
                    let spread = (s[0] - s[1]).abs().max((s[0] - s[2]).abs());
                    if spread > UNIFORM_SCALE_TOLERANCE * s[0] {
                        return Err(Error::new(
                            "NonUniformCompoundInstanceScale",
                            r.file,
                            r.base,
                            format!("three equal scale values within {UNIFORM_SCALE_TOLERANCE}"),
                            format!("instance {k}: {:?}", inst.scale),
                        ));
                    }
                    push(
                        r.steps,
                        Step::Instance {
                            rotation: inst.rotation,
                            translation: inst.translation,
                            scale: inst.scale,
                        },
                        r.scratch,
                        r.file,
                    )?;
                    let inner = self.leaves(inst.child, r, depth + 1);
                    r.steps.pop();
                    inner?;
                }
                Ok(())
            }
            _ => {
                let shape = self.shape(class, index, r)?;
                let mut steps = Vec::new();
                r.keep.reserve(&mut steps, r.steps.len(), r.file, r.base)?;
                steps.extend_from_slice(r.steps);
                let mut path = Vec::new();
                r.keep.reserve(&mut path, r.path.len(), r.file, r.base)?;
                path.extend_from_slice(r.path);
                push(
                    r.out,
                    Leaf {
                        object: index,
                        path,
                        steps,
                        shape,
                    },
                    r.keep,
                    r.file,
                )
            }
        })();
        r.path.pop();
        result
    }

    fn shape(&self, class: ShapeClass, index: usize, r: &Resolve<'_>) -> Result<Shape> {
        let c = &self.container;
        let copy =
            |points: &mut dyn Iterator<Item = [f32; 3]>, n: usize| -> Result<Vec<[f32; 3]>> {
                let mut v = Vec::new();
                r.keep.reserve(&mut v, n, r.file, r.base)?;
                v.extend(points.take(n));
                Ok(v)
            };
        Ok(match class {
            ShapeClass::CompressedMesh => {
                let mesh = hk::CompressedMesh::read(c, index, r.file, r.base)?;
                let work = r.scratch.child();
                let data = hk::MeshData::read(c, mesh.data, r.file, r.base, &work)?;
                let g = data.geometry(&work)?;
                let vertices = copy(&mut g.vertices.iter().copied(), g.vertices.len())?;
                let count = g
                    .primitives
                    .iter()
                    .map(|p| if p.quad { 2usize } else { 1 })
                    .sum::<usize>();
                let mut triangles = Vec::new();
                r.keep.reserve(&mut triangles, count, r.file, r.base)?;
                triangles.extend(g.triangles());
                let mut records = Vec::new();
                r.keep
                    .reserve(&mut records, g.records.len(), r.file, r.base)?;
                records.extend(g.records.iter().map(|m| Record {
                    kind: m.kind,
                    radius: m.radius,
                    vertices: m.vertices.clone(),
                }));
                let record_vertices = copy(
                    &mut g.record_vertices.iter().copied(),
                    g.record_vertices.len(),
                )?;
                Shape::Mesh {
                    vertices,
                    triangles,
                    records,
                    record_vertices,
                    placeholders: g.placeholders,
                }
            }
            ShapeClass::ConvexPolytope => {
                let p = hk::ConvexPolytope::read(c, index, r.file, r.base)?;
                let vertices = copy(&mut p.vertices(), p.vertex_count)?;
                // f32 at byte 20: finite and not negative in every measured
                // polytope (spec/havok-objects.md); refused otherwise.
                let radius = p
                    .bytes()
                    .get(20..24)
                    .map(|w| f32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .filter(|x| x.is_finite() && *x >= 0.0)
                    .ok_or_else(|| {
                        Error::new(
                            "InvalidHavokConvexRadius",
                            r.file,
                            r.base,
                            "finite f32 >= 0 at polytope byte 20",
                            format!("object {index}"),
                        )
                    })?;
                Shape::Polytope { vertices, radius }
            }
            ShapeClass::Convex => {
                let p = hk::ConvexShape::read(c, index, r.file, r.base)?;
                let n = p.vertices().count();
                let vertices = copy(&mut p.vertices(), n)?;
                Shape::Convex { vertices }
            }
            ShapeClass::Sphere => {
                let s = hk::Sphere::read(c, index, r.file, r.base)?;
                Shape::Sphere {
                    center: s.center,
                    radius: s.radius,
                }
            }
            ShapeClass::Capsule => {
                let s = hk::Capsule::read(c, index, r.file, r.base)?;
                Shape::Capsule {
                    points: s.points,
                    radius: s.radius,
                }
            }
            ShapeClass::ScaledConvex | ShapeClass::StaticCompound | ShapeClass::DynamicCompound => {
                return Err(Error::new(
                    "CollisionShapeNotALeaf",
                    r.file,
                    r.base,
                    "a leaf shape class",
                    class.class(),
                ))
            }
        })
    }
}
