#![forbid(unsafe_code)]
//! The placed actors of a cell and the cells around it: what each is drawn
//! with or why not, and the rest-pose checks of `spec/actor.md` (bind fit,
//! seams between an actor's parts, feet against the placement and against
//! the collision under it). Writes a JSON report; draws nothing.
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
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
