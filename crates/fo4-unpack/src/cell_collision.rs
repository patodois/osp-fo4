//! `cell-collision`: the collision of one cell (`fo4_scene::collision`) as a
//! report. Counts per shape kind, triangles, world bounds, references whose
//! model holds no collision, bodies not placed and why, and, as offline
//! evidence only, how each body's world collision box agrees with the render
//! box of its Target subtree under each level of the open body placement
//! rule.
use crate::Reports;
use fo4_core::{Budget, Error, Manifest, Result};
use fo4_plugin::typed::Store;
use fo4_scene::bounds::Bounds;
use fo4_scene::collision::{
    self, body_placement_is_identity, BodyPlacement, CellCollision, CollisionInstance, Shape, Step,
    UNITS,
};
use fo4_scene::hypotheses;
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::BTreeMap;

/// World units each box is widened by on every side before the overlap is
/// measured, so that a flat collision plane and a thin render slab that
/// touch are not counted as disjoint.
pub const OVERLAP_SLACK: f64 = 0.5;

/// Agreement of one body's world collision box with its render box.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Agreement {
    /// Smallest over the three axes of overlap length / union length, both
    /// boxes widened by [`OVERLAP_SLACK`]; 0 when separated on an axis.
    pub min_axis_overlap: f64,
    /// Distance between the two box centres, world units.
    pub center_offset: f64,
    /// Collision box inside the render box widened by 1 unit and 1% per axis.
    pub inside_render: bool,
}

pub fn agreement(c: &Bounds, r: &Bounds) -> Agreement {
    let mut min_axis_overlap = f64::INFINITY;
    let mut inside = true;
    let mut d2 = 0.0;
    for k in 0..3 {
        let e = OVERLAP_SLACK;
        let lo = c.min[k].max(r.min[k]) - e;
        let hi = c.max[k].min(r.max[k]) + e;
        let union = c.max[k].max(r.max[k]) - c.min[k].min(r.min[k]) + 2.0 * e;
        let ratio = (hi - lo).max(0.0) / union;
        min_axis_overlap = min_axis_overlap.min(ratio);
        let slack = 1.0 + 0.01 * (r.max[k] - r.min[k]);
        inside &= c.min[k] >= r.min[k] - slack && c.max[k] <= r.max[k] + slack;
        let dc = 0.5 * (c.min[k] + c.max[k]) - 0.5 * (r.min[k] + r.max[k]);
        d2 += dc * dc;
    }
    Agreement {
        min_axis_overlap,
        center_offset: d2.sqrt(),
        inside_render: inside,
    }
}

fn overlap_bucket(v: f64) -> &'static str {
    match v {
        v if v >= 0.99 => "0:>=0.99",
        v if v >= 0.95 => "1:0.95-0.99",
        v if v >= 0.90 => "2:0.90-0.95",
        v if v >= 0.50 => "3:0.50-0.90",
        v if v > 0.0 => "4:0-0.50",
        _ => "5:disjoint",
    }
}

/// World units.
fn distance_bucket(v: f64) -> &'static str {
    match v {
        v if v < 0.5 => "0:<0.5",
        v if v < 2.0 => "1:0.5-2",
        v if v < 8.0 => "2:2-8",
        v if v < 32.0 => "3:8-32",
        v if v < 128.0 => "4:32-128",
        _ => "5:>=128",
    }
}

#[derive(Default, Serialize)]
pub struct Distribution {
    pub bodies: u64,
    pub min_axis_overlap: BTreeMap<&'static str, u64>,
    pub center_offset: BTreeMap<&'static str, u64>,
    pub inside_render: u64,
}
impl Distribution {
    fn add(&mut self, a: &Agreement) {
        self.bodies += 1;
        *self
            .min_axis_overlap
            .entry(overlap_bucket(a.min_axis_overlap))
            .or_default() += 1;
        *self
            .center_offset
            .entry(distance_bucket(a.center_offset))
            .or_default() += 1;
        self.inside_render += u64::from(a.inside_render);
    }
}

#[derive(Default, Serialize)]
pub struct Level {
    /// Registry status of the level's rule.
    pub rule: &'static str,
    pub status: String,
    /// Union of the world boxes of every placed instance.
    pub bounds: Option<Bounds>,
    /// Bodies compared with the render box of their Target subtree (or of
    /// the whole model when the subtree has no render mesh).
    pub all: Distribution,
    /// Only bodies whose stored placement is not the identity: the bodies
    /// where the two levels differ.
    pub non_identity: Distribution,
    pub compared_with_whole_model: u64,
    pub without_render_geometry: u64,
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub cell: Option<fo4_plugin::FormKey>,
    pub editor_id: Option<String>,
    pub cell_kind: Option<String>,
    pub level: Option<RuleLevel>,
    pub body_placement: Option<BodyPlacement>,
    pub units: f64,
    pub scene_instances: u64,
    /// References with at least one model placed (SCOL parts included).
    pub references_with_model: u64,
    pub references_with_collision: u64,
    /// Placed models whose file holds no collision object.
    pub model_placements_without_collision: u64,
    pub references_with_model_and_no_collision: u64,
    pub distinct_models: u64,
    pub distinct_models_with_collision: u64,
    pub model_errors: u64,
    pub placed_models: u64,
    pub placed_bodies: u64,
    pub instances: u64,
    pub instances_by_kind: BTreeMap<String, u64>,
    pub instances_by_path: BTreeMap<String, u64>,
    pub triangles: u64,
    pub mesh_vertices: u64,
    pub records: u64,
    pub records_by_kind: BTreeMap<String, u64>,
    pub placeholders: u64,
    pub unplaced_by_code: BTreeMap<String, u64>,
    pub bodies_with_identity_placement: u64,
    pub bodies_with_stored_placement: u64,
    pub bodies_with_hidden_chain: u64,
    pub compound_instances_with_rotation: u64,
    pub scaled_convex_links: u64,
    /// For bodies with a stored translation: |69.99125 t| and its distance to
    /// the Target node's model-frame origin, world units.
    pub body_translation_length: BTreeMap<&'static str, u64>,
    pub body_translation_to_target_origin: BTreeMap<&'static str, u64>,
    /// World normals of the mesh triangles under the selected level, from
    /// the stored winding: z above 0.5, below -0.5, between, degenerate.
    /// Evidence for the open sidedness question only.
    pub triangle_normals: BTreeMap<&'static str, u64>,
    pub levels: BTreeMap<&'static str, Level>,
    /// Plugin index, archive mounts and the content fingerprint.
    pub fingerprint_seconds: f64,
    pub scene_seconds: f64,
    pub collision_seconds: f64,
    pub report_seconds: f64,
}

fn union(a: Option<Bounds>, b: Bounds) -> Bounds {
    match a {
        None => b,
        Some(a) => Bounds {
            min: std::array::from_fn(|k| a.min[k].min(b.min[k])),
            max: std::array::from_fn(|k| a.max[k].max(b.max[k])),
        },
    }
}

fn points_box(points: impl Iterator<Item = [f64; 3]>) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for p in points.filter(|p| p.iter().all(|x| x.is_finite())) {
        out = Some(union(out, Bounds { min: p, max: p }));
    }
    out
}

fn status_name(rule: &str) -> String {
    match hypotheses::status(rule) {
        Some(s) => format!("{s:?}").to_lowercase(),
        None => "unregistered".into(),
    }
}

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}

pub fn run(
    m: &Manifest,
    editor_id: &str,
    r: &mut Reports,
    level: RuleLevel,
    overlay: Option<&std::path::Path>,
) -> Result<()> {
    r.items_enabled = true;
    r.summary.rule_level = Some(level);
    let started = std::time::Instant::now();
    let store = Store::open_overlay(m, overlay, &Budget::new(m.limits.max_override_memory))?;
    for error in &store.diagnostics {
        r.error(error)?;
    }
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    vfs.check_complete()?;
    r.summary.fingerprint = Some(vfs.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets+plugins".into());
    let fingerprint_seconds = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let budget = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
    let scene = loader.by_editor_id(editor_id, &budget)?;
    let scene_seconds = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let options = collision::Options {
        level,
        body_placement: None,
    };
    let c = collision::cell(&scene, &loader, &m.limits, options, &budget)?;
    let collision_seconds = started.elapsed().as_secs_f64();
    let started = std::time::Instant::now();
    let mut s = Summary {
        cell: Some(scene.cell.key.clone()),
        editor_id: Some(editor_id.to_owned()),
        cell_kind: scene.kind.map(|k| format!("{k:?}")),
        level: Some(level),
        body_placement: c.body_placement,
        units: UNITS,
        scene_instances: scene.instances.len() as u64,
        fingerprint_seconds,
        scene_seconds,
        collision_seconds,
        ..Summary::default()
    };
    report(&c, &mut s, r)?;
    s.report_seconds = started.elapsed().as_secs_f64();
    r.item(&serde_json::json!({"kind":"cell_context","cell":scene.cell.key,"editor_id":editor_id,"cell_kind":s.cell_kind,"level":level,"body_placement":c.body_placement,"units":UNITS}))?;
    r.summary.files_attempted = s.instances;
    r.summary.files_valid = s.instances;
    r.tsv = Some(("cell_collision.tsv", tsv(m, r, &s)));
    r.summary.cell_collision = s;
    Ok(())
}

fn report(c: &CellCollision, s: &mut Summary, r: &mut Reports) -> Result<()> {
    use std::collections::{BTreeSet, HashMap};
    for m in &c.models {
        s.distinct_models += 1;
        s.distinct_models_with_collision += u64::from(m.has_collision());
        if let Some(e) = &m.error {
            s.model_errors += 1;
            r.error(e)?;
        }
        if let Some(e) = &m.render_error {
            r.notice(e)?;
        }
    }
    let mut with_model = BTreeSet::new();
    let mut with_collision = BTreeSet::new();
    let mut without = BTreeSet::new();
    for p in &c.placements {
        with_model.insert(p.reference.clone());
        with_collision.insert(p.reference.clone());
    }
    for w in &c.without_collision {
        with_model.insert(w.reference.clone());
        without.insert(w.reference.clone());
        s.model_placements_without_collision += 1;
        r.item(&serde_json::json!({"kind":"model_without_collision","reference":w.reference,"base":w.base,"model":c.models[w.model].path}))?;
    }
    for u in &c.unplaced {
        if u.model.is_some() {
            with_model.insert(u.reference.clone());
            if c.models[u.model.unwrap_or(0)].has_collision() {
                with_collision.insert(u.reference.clone());
            }
        }
        bump(&mut s.unplaced_by_code, u.code.clone());
        r.item(&serde_json::json!({"kind":"unplaced","reference":u.reference,"base":u.base,"model":u.model.map(|m|&c.models[m].path),"collision_object":u.collision_object,"system":u.system,"body":u.body,"leaf":u.leaf,"code":u.code,"detail":u.detail}))?;
    }
    s.references_with_model = with_model.len() as u64;
    s.references_with_collision = with_collision.len() as u64;
    s.references_with_model_and_no_collision = without.difference(&with_collision).count() as u64;
    s.placed_models = c.placements.len() as u64;
    s.instances = c.instances.len() as u64;
    for v in BodyPlacement::ALL {
        s.levels.insert(
            v.name(),
            Level {
                rule: v.rule(),
                status: status_name(v.rule()),
                ..Level::default()
            },
        );
    }
    // Instances of one body, in order.
    let mut bodies: Vec<((usize, usize), Vec<&CollisionInstance>)> = Vec::new();
    let mut index: HashMap<(usize, usize), usize> = HashMap::new();
    for i in &c.instances {
        let key = (i.placement, i.body);
        let k = *index.entry(key).or_insert_with(|| {
            bodies.push((key, Vec::new()));
            bodies.len() - 1
        });
        bodies[k].1.push(i);
        bump(&mut s.instances_by_kind, i.kind.class().to_owned());
        let leaf = c.leaf(i);
        let path: Vec<&str> = leaf.path.iter().map(|p| p.class()).collect();
        bump(&mut s.instances_by_path, path.join(">"));
        for step in &leaf.steps {
            match step {
                Step::Instance { rotation, .. } => {
                    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
                    s.compound_instances_with_rotation += u64::from(*rotation != identity);
                }
                Step::Scaled { .. } => s.scaled_convex_links += 1,
            }
        }
        if let Shape::Mesh {
            vertices,
            triangles,
            records,
            placeholders,
            ..
        } = &leaf.shape
        {
            s.triangles += triangles.len() as u64;
            s.mesh_vertices += vertices.len() as u64;
            s.records += records.len() as u64;
            s.placeholders += *placeholders as u64;
            for rec in records {
                bump(&mut s.records_by_kind, format!("0x{:02x}", rec.kind));
            }
            let v = c.body_placement.unwrap_or(BodyPlacement::Ignored);
            for t in c.world_triangles(i, v) {
                let n = collision::triangle_normal(t);
                bump(
                    &mut s.triangle_normals,
                    match n[2] {
                        z if n == [0.0; 3] || !z.is_finite() => "degenerate",
                        z if z > 0.5 => "up",
                        z if z < -0.5 => "down",
                        _ => "side",
                    },
                );
            }
        }
    }
    s.placed_bodies = bodies.len() as u64;
    for ((placement, body_index), instances) in &bodies {
        let p = &c.placements[*placement];
        let model = &c.models[p.model];
        let body = &model.bodies[*body_index];
        let identity = body_placement_is_identity(&body.body);
        if identity {
            s.bodies_with_identity_placement += 1;
        } else {
            s.bodies_with_stored_placement += 1;
        }
        s.bodies_with_hidden_chain += u64::from(body.hidden_chain);
        if body.body.translation != [0.0; 3] {
            let t = body.body.translation.map(|x| f64::from(x) * UNITS);
            let len = t.iter().map(|x| x * x).sum::<f64>().sqrt();
            let o = body.target_local.translation;
            let d = (0..3).map(|k| (t[k] - o[k]).powi(2)).sum::<f64>().sqrt();
            bump(&mut s.body_translation_length, distance_bucket(len));
            bump(&mut s.body_translation_to_target_origin, distance_bucket(d));
        }
        let mut render = points_box(model.render_points(body).map(|q| p.frame.apply(q)));
        let mut whole = false;
        if render.is_none() {
            render = points_box(model.render_points.iter().map(|q| p.frame.apply(*q)));
            whole = render.is_some();
        }
        let mut per_level = serde_json::Map::new();
        for v in BodyPlacement::ALL {
            let mut bx = None;
            for i in instances {
                if let Some(b) = c.world_bounds(i, v) {
                    bx = Some(union(bx, b));
                }
            }
            let level = s.levels.get_mut(v.name()).ok_or_else(|| {
                Error::new(
                    "MissingReportLevel",
                    "cell collision",
                    0,
                    v.name(),
                    "absent",
                )
            })?;
            if let Some(b) = bx {
                level.bounds = Some(union(level.bounds, b));
            }
            let a = match (bx, render) {
                (Some(cb), Some(rb)) => {
                    let a = agreement(&cb, &rb);
                    level.all.add(&a);
                    if !identity {
                        level.non_identity.add(&a);
                    }
                    level.compared_with_whole_model += u64::from(whole);
                    Some(a)
                }
                _ => {
                    level.without_render_geometry += 1;
                    None
                }
            };
            per_level.insert(
                v.name().into(),
                serde_json::json!({"collision_bounds":bx,"agreement":a}),
            );
        }
        let leaves: Vec<_> = instances
            .iter()
            .map(|i| {
                let leaf = c.leaf(i);
                let (triangles, vertices, records) = match &leaf.shape {
                    Shape::Mesh {
                        triangles,
                        vertices,
                        records,
                        ..
                    } => (triangles.len(), vertices.len(), records.len()),
                    Shape::Polytope { vertices, .. } | Shape::Convex { vertices } => {
                        (0, vertices.len(), 0)
                    }
                    Shape::Sphere { .. } | Shape::Capsule { .. } => (0, 0, 0),
                };
                serde_json::json!({"leaf":i.leaf,"kind":i.kind.class(),"path":leaf.path.iter().map(|p|p.class()).collect::<Vec<_>>(),"steps":leaf.steps,"triangles":triangles,"vertices":vertices,"records":records,"rules":i.rules})
            })
            .collect();
        r.item(&serde_json::json!({"kind":"collision_body","reference":p.reference,"base":p.base,"model":model.path,"frame":p.frame,"target":body.target,"target_name":body.target_name,"chain":body.chain,"hidden_chain":body.hidden_chain,"collision_object":body.collision_object,"flags":body.flags,"system":body.system,"body_id":body.body_id,"body":body.body.index,"body_translation":body.body.translation,"body_rotation":body.body.rotation,"identity_placement":identity,"target_local":body.target_local,"render_bounds":render,"render_scope":if whole {"model"} else {"target_subtree"},"levels":per_level,"leaves":leaves}))?;
    }
    Ok(())
}

fn tsv(m: &Manifest, r: &Reports, s: &Summary) -> String {
    let mut out = String::from("# section\tkey\tvalue\n");
    let mut row = |section: &str, key: &str, value: String| {
        out.push_str(&format!("{section}\t{key}\t{value}\n"));
    };
    for (k, v) in [
        (
            "build",
            m.build.clone().unwrap_or_else(|| "undeclared".into()),
        ),
        ("plugins", m.plugins.len().to_string()),
        ("mounts", m.mounts.len().to_string()),
        (
            "vfs_fingerprint",
            r.summary.fingerprint.clone().unwrap_or_default(),
        ),
        ("editor_id", s.editor_id.clone().unwrap_or_default()),
        ("cell_kind", s.cell_kind.clone().unwrap_or_default()),
        ("level", format!("{:?}", s.level)),
        ("body_placement_default", format!("{:?}", s.body_placement)),
        ("units", s.units.to_string()),
    ] {
        row("census", k, v);
    }
    for (k, v) in [
        ("scene_instances", s.scene_instances),
        ("references_with_model", s.references_with_model),
        ("references_with_collision", s.references_with_collision),
        (
            "references_with_model_and_no_collision",
            s.references_with_model_and_no_collision,
        ),
        (
            "model_placements_without_collision",
            s.model_placements_without_collision,
        ),
        ("distinct_models", s.distinct_models),
        (
            "distinct_models_with_collision",
            s.distinct_models_with_collision,
        ),
        ("model_errors", s.model_errors),
        ("placed_models", s.placed_models),
        ("placed_bodies", s.placed_bodies),
        ("instances", s.instances),
        ("triangles", s.triangles),
        ("mesh_vertices", s.mesh_vertices),
        ("records", s.records),
        ("placeholders", s.placeholders),
        (
            "bodies_with_identity_placement",
            s.bodies_with_identity_placement,
        ),
        (
            "bodies_with_stored_placement",
            s.bodies_with_stored_placement,
        ),
        ("bodies_with_hidden_chain", s.bodies_with_hidden_chain),
        (
            "compound_instances_with_rotation",
            s.compound_instances_with_rotation,
        ),
        ("scaled_convex_links", s.scaled_convex_links),
    ] {
        row("total", k, v.to_string());
    }
    for (section, map) in [
        ("instances_by_kind", &s.instances_by_kind),
        ("instances_by_path", &s.instances_by_path),
        ("records_by_kind", &s.records_by_kind),
        ("unplaced", &s.unplaced_by_code),
    ] {
        for (k, v) in map {
            row(section, k, v.to_string());
        }
    }
    for (k, v) in &s.triangle_normals {
        row("triangle_normals", k, v.to_string());
    }
    for (section, map) in [
        ("body_translation_length", &s.body_translation_length),
        (
            "body_translation_to_target_origin",
            &s.body_translation_to_target_origin,
        ),
    ] {
        for (k, v) in map {
            row(section, k, v.to_string());
        }
    }
    for (name, l) in &s.levels {
        row("level", &format!("{name}|rule"), l.rule.to_owned());
        row("level", &format!("{name}|status"), l.status.clone());
        if let Some(b) = l.bounds {
            row(
                "level",
                &format!("{name}|bounds_min"),
                format!("{:?}", b.min),
            );
            row(
                "level",
                &format!("{name}|bounds_max"),
                format!("{:?}", b.max),
            );
        }
        row(
            "level",
            &format!("{name}|without_render_geometry"),
            l.without_render_geometry.to_string(),
        );
        row(
            "level",
            &format!("{name}|compared_with_whole_model"),
            l.compared_with_whole_model.to_string(),
        );
        for (scope, d) in [("all", &l.all), ("non_identity", &l.non_identity)] {
            let key = |k: &str| format!("{name}|{scope}|{k}");
            row("agreement", &key("bodies"), d.bodies.to_string());
            row(
                "agreement",
                &key("inside_render"),
                d.inside_render.to_string(),
            );
            for (k, v) in &d.min_axis_overlap {
                row(
                    "agreement",
                    &key(&format!("min_axis_overlap|{k}")),
                    v.to_string(),
                );
            }
            for (k, v) in &d.center_offset {
                row(
                    "agreement",
                    &key(&format!("center_offset|{k}")),
                    v.to_string(),
                );
            }
        }
    }
    for (k, v) in [
        ("fingerprint_seconds", s.fingerprint_seconds),
        ("scene_seconds", s.scene_seconds),
        ("collision_seconds", s.collision_seconds),
        ("report_seconds", s.report_seconds),
    ] {
        row("time", k, format!("{v:.3}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(min: [f64; 3], max: [f64; 3]) -> Bounds {
        Bounds { min, max }
    }
    #[test]
    fn agreement_is_per_axis_overlap_offset_and_containment() {
        let r = b([0.0; 3], [10.0; 3]);
        let a = agreement(&r, &r);
        assert_eq!(
            (a.min_axis_overlap, a.center_offset, a.inside_render),
            (1.0, 0.0, true)
        );
        let a = agreement(&b([5.0, 0.0, 0.0], [15.0, 10.0, 10.0]), &r);
        assert!((a.min_axis_overlap - 6.0 / 16.0).abs() < 1e-12);
        assert_eq!((a.center_offset, a.inside_render), (5.0, false));
        let a = agreement(&b([20.0; 3], [30.0; 3]), &r);
        assert_eq!(overlap_bucket(a.min_axis_overlap), "5:disjoint");
        // Flat boxes on one axis: equal planes overlap fully on it.
        let flat = b([0.0, 0.0, 1.0], [10.0, 10.0, 1.0]);
        assert_eq!(agreement(&flat, &flat).min_axis_overlap, 1.0);
        // A flat plane on the bottom of a thin slab touches it.
        let slab = b([0.0, 0.0, 1.0], [10.0, 10.0, 1.5]);
        assert!((agreement(&flat, &slab).min_axis_overlap - 1.0 / 1.5).abs() < 1e-12);
        assert_eq!(distance_bucket(0.1), "0:<0.5");
        assert_eq!(distance_bucket(500.0), "5:>=128");
    }
}
