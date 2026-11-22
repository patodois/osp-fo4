#![forbid(unsafe_code)]
//! The placed actors of a cell and the cells around it: what each is drawn
//! with or why not, and the rest-pose checks of `spec/actor.md` (bind fit,
//! seams between an actor's parts, feet against the placement and against
//! the collision under it, the terrain and the render triangles of the
//! placed references under it). Writes a JSON report; draws nothing.
//!
//! `actor_check MANIFEST CELL_EDITOR_ID RADIUS OUT.json`
use fo4_core::{Budget, Manifest};
use fo4_scene::{
    actor::{self, pose},
    DecodedAsset, Loader, RuleLevel,
};
use std::path::Path;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("actor_check MANIFEST CELL RADIUS OUT.json".into());
    }
    let manifest = Manifest::load(Path::new(&args[0]))?;
    let limits = &manifest.limits;
    let store =
        fo4_plugin::typed::Store::open(&manifest, &Budget::new(limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&manifest, &Budget::new(limits.max_vfs_memory))?;
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(4 << 30).with_scope_limit(limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &manifest.language, level, &budget);
    let mut scene = loader.by_editor_id(&args[1], &budget)?;
    let radius: u32 = args[2].parse()?;
    let mut grid = None;
    if radius > 0 {
        let a = fo4_scene::grid::around(&store, &scene.cell.key, radius, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        grid = Some(a);
    }
    let started = std::time::Instant::now();
    let actors = actor::resolve(&scene, &mut loader, &budget)?;
    let resolve_seconds = started.elapsed().as_secs_f64();
    // Collision of the scene, for the floor under each actor.
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, limits, options, &budget)?;
    let placement = c.body_placement.ok_or("no body placement level")?;
    let mut world = fo4_walk::World::build(&c, placement, &budget)?;
    let mut ground = match &grid {
        Some(a) => fo4_walk::terrain::load_with(&store, a, level, &budget)?,
        None => fo4_walk::terrain::load_around(&store, &scene.cell.key, level, true, &budget)?,
    };
    ground.add_to(&mut world, &budget)?;
    let mut report = Vec::new();
    for a in &actors.actors {
        let mut item = serde_json::json!({
            "reference": format!("{}:{:06x}", a.reference.plugin, a.reference.object_id),
            "base": a.base.as_ref().map(|b| format!("{}:{:06x}", b.plugin, b.object_id)),
            "transform": a.transform,
            "scale": a.scale,
            "state": format!("{:?}", a.state),
            "notes": a.notes,
            "blockers": a.blockers.iter().filter(|e| fo4_scene::hypotheses::covering(e).is_none()).map(|e| format!("{} {}", e.code, e.found)).collect::<Vec<_>>(),
        });
        if let Some(plan) = &a.plan {
            let skel_path = String::from_utf8_lossy(&plan.skeleton).into_owned();
            let skeleton =
                loader
                    .assets
                    .with_decoded(plan.skeleton_asset, &budget.child(), |d| {
                        let DecodedAsset::Model(n) = d else {
                            return Err(fo4_core::Error::new(
                                "Kind", "skeleton", 0, "model", "other",
                            ));
                        };
                        pose::Skeleton::read(&n, &skel_path, limits, &budget.child())
                    })?;
            let world_t = fo4_scene::transform::Transform::euler(
                a.transform.unwrap(),
                a.scale.unwrap_or(1.),
            )?;
            let mut parts = Vec::new();
            let mut lowest = f64::INFINITY;
            let mut points: Vec<(usize, [f64; 3], [f64; 3])> = Vec::new();
            // Body parts, then head parts (index 100 and up).
            let mut items: Vec<(usize, usize, &Vec<u8>, serde_json::Value)> = plan
                .parts
                .iter()
                .enumerate()
                .map(|(pi, part)| {
                    (
                        pi,
                        part.asset,
                        &part.path,
                        serde_json::json!({
                            "armor": format!("{}:{:06x}", part.armor.plugin, part.armor.object_id),
                            "addon": format!("{}:{:06x}", part.addon.plugin, part.addon.object_id),
                            "skin": part.skin,
                        }),
                    )
                })
                .collect();
            if let Some(head) = &plan.head {
                for (k, h) in head.parts.iter().enumerate() {
                    items.push((
                        100 + k,
                        h.asset,
                        &h.path,
                        serde_json::json!({
                            "head_part": format!("{}:{:06x}", h.part.plugin, h.part.object_id),
                            "kind": h.kind,
                        }),
                    ));
                }
            }
            for (pi, asset, part_path, label) in items {
                let path = String::from_utf8_lossy(part_path).into_owned();
                let info = loader.assets.with_decoded(asset, &budget.child(), |d| {
                    let DecodedAsset::Model(n) = d else { return Err(fo4_core::Error::new("Kind", "model", 0, "model", "other")) };
                    let scratch = budget.child();
                    let mut shapes = Vec::new();
                    for (i, blk) in n.blocks.iter().enumerate() {
                        let Some(fo4_nif::Data::Mesh(m)) = &blk.decoded else { continue };
                        let skin = match fo4_nif::skin::shape_skin(&n, i, &path, limits, &scratch) {
                            Ok(Some(s)) => s,
                            Ok(None) => {
                                shapes.push(serde_json::json!({"shape": i, "skin": false}));
                                continue;
                            }
                            Err(e) => {
                                shapes.push(serde_json::json!({"shape": i, "error": e.code}));
                                continue;
                            }
                        };
                        let fit = pose::bind_fit(&n, &skin, &path, limits, &scratch).ok();
                        let matrices = match pose::bone_matrices(&n, &skin, &skeleton, &path, &scratch) {
                            Ok(m) => m,
                            Err(e) => {
                                shapes.push(serde_json::json!({"shape": i, "error": e.code, "found": e.found}));
                                continue;
                            }
                        };
                        let positions = match pose::skin_positions(&n, i, &matrices, &path, &scratch) {
                            Ok(p) => p,
                            Err(e) => {
                                shapes.push(serde_json::json!({"shape": i, "error": e.code}));
                                continue;
                            }
                        };
                        // Rest against bind: how far the rest pose moves each vertex
                        // from its bind position (stored + the shape's bind translation).
                        let mut moved: f64 = 0.;
                        if let Some(f) = &fit {
                            for (v, p) in m.vertices.as_ref().unwrap().iter().zip(&positions) {
                                let s = v.position.unwrap().map(f64::from);
                                let bind = [s[0] + f.translation[0], s[1] + f.translation[1], s[2] + f.translation[2]];
                                moved = moved.max((0..3).map(|k| (bind[k] - p[k]).powi(2)).sum::<f64>().sqrt());
                            }
                        }
                        let t = fit.map_or([0.; 3], |f| f.translation);
                        for (v, p) in m.vertices.as_ref().unwrap().iter().zip(&positions) {
                            let wp = world_t.apply(*p);
                            lowest = lowest.min(wp[2]);
                            let s = v.position.unwrap().map(f64::from);
                            points.push((pi * 10000 + i, [s[0] + t[0], s[1] + t[1], s[2] + t[2]], wp));
                        }
                        shapes.push(serde_json::json!({
                            "shape": i,
                            "vertices": positions.len(),
                            "bones": skin.bones.len(),
                            "bind_rotation_error": fit.map(|f| f.rotation_error),
                            "bind_translation_spread": fit.map(|f| f.translation_spread),
                            "bind_translation": fit.map(|f| f.translation),
                            "rest_moves_bind_by_at_most": moved,
                        }));
                    }
                    Ok(shapes)
                })?;
                let mut label = label;
                label["model"] = serde_json::json!(path);
                label["shapes"] = serde_json::json!(info);
                parts.push(label);
            }
            // Seams: vertices of different shapes at the same bind position
            // (stored plus the shape's bind translation, within 0.05 on each
            // axis: the binary16 spacing is 0.03125 between 32 and 64 units)
            // and their distance after posing.
            let mut seam_pairs = 0u64;
            let mut seam_max: f64 = 0.;
            let mut seam_bind_max: f64 = 0.;
            // The same between a head part and a body part.
            let (mut neck_pairs, mut neck_max, mut neck_bind_max) = (0u64, 0f64, 0f64);
            points.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
            for i in 0..points.len() {
                let mut j = i + 1;
                while j < points.len() && points[j].1[0] - points[i].1[0] <= 0.05 {
                    if points[j].0 != points[i].0
                        && (0..3).all(|k| (points[j].1[k] - points[i].1[k]).abs() <= 0.05)
                    {
                        seam_pairs += 1;
                        let d = (0..3)
                            .map(|k| (points[j].2[k] - points[i].2[k]).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        seam_max = seam_max.max(d);
                        let e = (0..3)
                            .map(|k| (points[j].1[k] - points[i].1[k]).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        seam_bind_max = seam_bind_max.max(e);
                        if (points[j].0 / 10000 >= 100) != (points[i].0 / 10000 >= 100) {
                            neck_pairs += 1;
                            neck_max = neck_max.max(d);
                            neck_bind_max = neck_bind_max.max(e);
                        }
                    }
                    j += 1;
                }
            }
            let t = a.transform.unwrap();
            let floor = world.ray_down(f64::from(t[0]), f64::from(t[1]), f64::from(t[2]) + 100.);
            let (px, py, pz) = (f64::from(t[0]), f64::from(t[1]), f64::from(t[2]));
            // Render triangles of the placed references straight under the
            // placement (from 100 above it down to 300 below), drawn or not.
            let mut under = Vec::new();
            for inst in &scene.instances {
                let (Some(base), Some(rt)) = (&inst.base, inst.reference.data.transform) else {
                    continue;
                };
                let d = ((f64::from(rt[0]) - px).powi(2) + (f64::from(rt[1]) - py).powi(2)).sqrt();
                if d > 6000. {
                    continue;
                }
                let Ok(placed) = fo4_scene::transform::Transform::euler(
                    rt,
                    inst.reference.data.scale.unwrap_or(1.),
                ) else {
                    continue;
                };
                for id in &base.assets {
                    if loader.assets.nodes[*id].kind != fo4_scene::AssetKind::Model {
                        continue;
                    }
                    let hits = loader.assets.with_decoded(*id, &budget.child(), |dd| {
                        let DecodedAsset::Model(n) = dd else {
                            return Ok(Vec::new());
                        };
                        let mut hits = Vec::new();
                        for r in n.roots.iter().filter(|r| **r >= 0) {
                            render_hits(&n, *r as usize, placed, px, py, 0, &mut hits);
                        }
                        Ok(hits)
                    });
                    for z in hits.unwrap_or_default() {
                        if z <= pz + 100. && z >= pz - 300. {
                            under.push(serde_json::json!({
                                "z": z,
                                "z_minus_placement": z - pz,
                                "reference": format!("{}:{:06x}", inst.reference.key.plugin, inst.reference.key.object_id),
                                "base_type": String::from_utf8_lossy(&base.record.signature),
                                "model": String::from_utf8_lossy(&loader.assets.nodes[*id].path),
                                "ready_under_hypotheses": inst.state.ready_at(RuleLevel::Hypotheses),
                                "blockers": inst.blockers.iter().filter(|e| fo4_scene::hypotheses::covering(e).is_none()).map(|e| e.code.clone()).collect::<std::collections::BTreeSet<_>>(),
                            }));
                        }
                    }
                }
            }
            under.sort_by(|a, b| {
                b["z"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&a["z"].as_f64().unwrap())
            });
            let terrain = ground.height_at(px, py).map(|h| h.0);
            item["plan"] = serde_json::json!({
                "race": format!("{}:{:06x}", plan.race.plugin, plan.race.object_id),
                "section": plan.section + 1,
                "skeleton": skel_path,
                "skeleton_bones": skeleton.bones.len(),
                "parts": parts,
                "lowest_vertex_z": lowest,
                "placement_z": t[2],
                "lowest_minus_placement": lowest - f64::from(t[2]),
                "collision_floor_z_under_placement": floor.map(|h| h.z),
                "lowest_minus_floor": floor.map(|h| lowest - h.z),
                "terrain_z_under_placement": terrain,
                "render_triangles_under_placement": under,
                "seam_vertex_pairs": seam_pairs,
                "seam_max_distance_in_bind": seam_bind_max,
                "seam_max_distance_after_posing": seam_max,
                "head_body_seam_vertex_pairs": neck_pairs,
                "head_body_seam_max_distance_in_bind": neck_bind_max,
                "head_body_seam_max_distance_after_posing": neck_max,
            });
        }
        report.push(item);
    }
    let out = serde_json::json!({
        "cell": args[1],
        "radius": radius,
        "actors": actors.actors.len(),
        "disabled": actors.disabled,
        "distant": actors.distant,
        "resolve_seconds": resolve_seconds,
        "items": report,
    });
    std::fs::write(&args[3], serde_json::to_string_pretty(&out)?)?;
    Ok(())
}
/// Heights of the triangles of the model subtree `i` (placed by `parent`)
/// that a vertical line at (x, y) crosses, walking nodes as the static
/// renderer does (editor markers and outside blocks left out).
fn render_hits(
    n: &fo4_nif::Nif<'_>,
    i: usize,
    parent: fo4_scene::transform::Transform,
    x: f64,
    y: f64,
    depth: u32,
    out: &mut Vec<f64>,
) {
    use fo4_nif::Data;
    if depth > 64 {
        return;
    }
    let Some(block) = n.blocks.get(i) else { return };
    if fo4_scene::model_policy::visibility_editor_marker(n, i)
        || fo4_scene::model_policy::editor_marker_object(n, i)
        || fo4_scene::model_policy::editor_marker_suffix(n, i)
    {
        return;
    }
    let ty = &n.block_types[block.type_index as usize];
    if matches!(
        fo4_scene::model_policy::block_class(ty),
        fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
    ) {
        return;
    }
    let Some(data) = block.decoded.as_ref() else {
        return;
    };
    let f = fo4_scene::model_policy::fields(data);
    let w = if f.iter().any(|f| f.name == "Translation") {
        match fo4_scene::transform::Transform::from_fields(f, "check") {
            Ok(l) => parent.compose(l),
            Err(_) => return,
        }
    } else {
        parent
    };
    let mesh = match data {
        Data::Node { children, .. } => {
            for c in children.iter().filter(|c| **c >= 0) {
                render_hits(n, *c as usize, w, x, y, depth + 1, out);
            }
            return;
        }
        Data::Mesh(m) => Some(m),
        Data::Fields(ff) => ff
            .iter()
            .find_map(|f| match f.value {
                fo4_nif::Value::Ref(r) if f.name == "Data" && r >= 0 => Some(r as usize),
                _ => None,
            })
            .and_then(|r| match n.blocks.get(r).and_then(|b| b.decoded.as_ref()) {
                Some(Data::Mesh(m)) => Some(m),
                _ => None,
            }),
    };
    let Some(mesh) = mesh else { return };
    let (Some(vs), Some(ts)) = (&mesh.vertices, &mesh.triangles) else {
        return;
    };
    let p: Vec<Option<[f64; 3]>> = vs
        .iter()
        .map(|v| v.position.map(|p| w.apply(p.map(f64::from))))
        .collect();
    for t in ts {
        let (Some(Some(a)), Some(Some(b)), Some(Some(c))) = (
            p.get(t[0] as usize),
            p.get(t[1] as usize),
            p.get(t[2] as usize),
        ) else {
            continue;
        };
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-9 {
            continue;
        }
        let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / det;
        let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / det;
        let l3 = 1. - l1 - l2;
        if l1 >= 0. && l2 >= 0. && l3 >= 0. {
            out.push(l1 * a[2] + l2 * b[2] + l3 * c[2]);
        }
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
