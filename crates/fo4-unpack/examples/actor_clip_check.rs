#![forbid(unsafe_code)]
//! The checks of `spec/actor.md` (Clip pose) for the actors of a cell and
//! the cells around it posed by one clip, at every frame of the clip: bone
//! lengths (distance from each Havok bone to its Havok parent) against frame
//! 0, split by whether the bone's track translates; the lowest skinned
//! vertex and the lowest bone against the placement and the collision floor
//! under it; frame 0 against the skeleton NIF's rest pose per bone; and the
//! time spent posing and skinning one frame. Writes a JSON report.
//!
//! `actor_clip_check MANIFEST CELL_EDITOR_ID RADIUS CLIP.hkx OUT.json`
use fo4_core::{Budget, Manifest};
use fo4_scene::{
    actor::{self, clip, pose},
    DecodedAsset, Loader, RuleLevel,
};
use std::{collections::BTreeMap, path::Path};

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("actor_clip_check MANIFEST CELL RADIUS CLIP.hkx OUT.json".into());
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
    let actors = actor::resolve(&scene, &mut loader, &budget)?;
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
    let key = fo4_core::path_key(&args[3])?;
    let bytes = vfs.read(&key, &budget)?;
    let clip = clip::Clip::read(&bytes, &key, &budget)?;
    let frames = clip.animation.frames;
    let fd = clip.animation.frame_duration;
    let mut report = Vec::new();
    for a in actors.drawable(level) {
        let Some(plan) = &a.plan else { continue };
        let nif = loader.assets.nodes[plan.skeleton_asset].path.clone();
        let nif_name = String::from_utf8_lossy(&nif).into_owned();
        let hkx = String::from_utf8_lossy(&clip::skeleton_hkx_path(&nif).ok_or("not a .nif")?)
            .into_owned();
        let Ok(hb) = vfs.read(&hkx, &budget) else {
            report.push(
                serde_json::json!({"skeleton": nif_name, "posed": false, "why": "no .hkx beside the skeleton NIF"}),
            );
            continue;
        };
        let hk = clip::HkSkeleton::read(&hb, &hkx, &budget)?;
        if !clip.fits(&hk) {
            report.push(serde_json::json!({"skeleton": nif_name, "posed": false, "why": "binding does not fit"}));
            continue;
        }
        let (rig, rest) =
            loader
                .assets
                .with_decoded(plan.skeleton_asset, &budget.child(), |d| {
                    let DecodedAsset::Model(n) = d else {
                        return Err(fo4_core::Error::new(
                            "Kind", "skeleton", 0, "model", "other",
                        ));
                    };
                    Ok((
                        clip::Rig::new(&n, &nif_name, &hk, limits, &budget)?,
                        pose::Skeleton::read(&n, &nif_name, limits, &budget)?,
                    ))
                })?;
        // Skinned shapes of every part.
        let mut shapes: Vec<pose::SkinnedShape> = Vec::new();
        for part in &plan.parts {
            let path = String::from_utf8_lossy(&part.path).into_owned();
            let mut got = loader
                .assets
                .with_decoded(part.asset, &budget.child(), |d| {
                    let DecodedAsset::Model(n) = d else {
                        return Err(fo4_core::Error::new("Kind", "model", 0, "model", "other"));
                    };
                    let mut out = Vec::new();
                    for i in 0..n.blocks.len() {
                        if let Ok(Some(skin)) =
                            fo4_nif::skin::shape_skin(&n, i, &path, limits, &budget)
                        {
                            out.push(pose::SkinnedShape::read(&n, i, &skin, &path, &budget)?);
                        }
                    }
                    Ok(out)
                })?;
            shapes.append(&mut got);
        }
        let t = a.transform.ok_or("no placement")?;
        let placed = fo4_scene::transform::Transform::euler(t, a.scale.unwrap_or(1.))?;
        let floor = world
            .ray_down(f64::from(t[0]), f64::from(t[1]), f64::from(t[2]) + 100.)
            .map(|h| h.z);
        // Which Havok bones have a translating (spline) track.
        let translating: Vec<bool> = {
            let mut v = vec![false; hk.bones.len()];
            for block in &clip.animation.blocks {
                for (track, bone) in block.iter().zip(&clip.tracks) {
                    if track.translation.curve.is_some() {
                        v[*bone as usize] = true;
                    }
                }
            }
            v
        };
        let mut length0: BTreeMap<usize, f64> = BTreeMap::new();
        let mut length_dev = [0f64; 2];
        let mut lowest = (f64::INFINITY, f64::NEG_INFINITY);
        let mut feet = (f64::INFINITY, f64::NEG_INFINITY);
        let mut seconds = (0f64, 0f64);
        let mut frame0 = Vec::new();
        let mut matrices = Vec::new();
        let mut positions = Vec::new();
        let mut vertices = 0usize;
        for f in 0..frames {
            let time = f64::from(f) * fd;
            let clock = std::time::Instant::now();
            let locals = clip.locals(&hk, time);
            let worlds = clip::worlds(&hk, &locals, &hkx)?;
            let posed = rig.pose(&worlds);
            let mut low = f64::INFINITY;
            vertices = 0;
            for s in &shapes {
                s.matrices(&posed, &nif_name, &mut matrices)?;
                s.positions(&matrices, &mut positions);
                vertices += positions.len();
                for p in &positions {
                    low = low.min(placed.apply(*p)[2]);
                }
            }
            let spent = clock.elapsed().as_secs_f64();
            seconds.0 += spent;
            seconds.1 = seconds.1.max(spent);
            lowest = (lowest.0.min(low), lowest.1.max(low));
            // Bone lengths: each bone to its Havok parent, in the posed NIF.
            for (j, name) in hk.bones.iter().enumerate() {
                let Ok(p) = usize::try_from(hk.parents[j]) else {
                    continue;
                };
                let (Some(a), Some(b)) = (posed.bones.get(name), posed.bones.get(&hk.bones[p]))
                else {
                    continue;
                };
                let d = dist(a.translation, b.translation);
                let d0 = *length0.entry(j).or_insert(d);
                let k = usize::from(translating[j]);
                length_dev[k] = length_dev[k].max((d - d0).abs());
            }
            // The lowest Havok bone of the frame other than the root bone.
            let low_bone = hk
                .bones
                .iter()
                .zip(&hk.parents)
                .filter(|(_, p)| **p >= 0)
                .filter_map(|(name, _)| posed.bones.get(name))
                .map(|w| placed.apply(w.translation)[2])
                .fold(f64::INFINITY, f64::min);
            feet = (feet.0.min(low_bone), feet.1.max(low_bone));
            if f == 0 {
                for (name, w) in &posed.bones {
                    if let Some(r) = rest.bones.get(name) {
                        frame0.push((
                            String::from_utf8_lossy(name).into_owned(),
                            dist(w.translation, r.translation),
                        ));
                    }
                }
            }
        }
        frame0.sort_by(|a, b| b.1.total_cmp(&a.1));
        let within = |e: f64| frame0.iter().filter(|x| x.1 < e).count();
        report.push(serde_json::json!({
            "reference": format!("{}:{:06x}", a.reference.plugin, a.reference.object_id),
            "skeleton": nif_name,
            "havok_skeleton": hkx,
            "posed": true,
            "bones": hk.bones.len(),
            "nif_nodes_named_like_a_bone": rig.matched(),
            "frames": frames,
            "skinned_shapes": shapes.len(),
            "vertices": vertices,
            "bone_length_largest_change_from_frame_0": {"track_without_translation_curve": length_dev[0], "track_with_translation_curve": length_dev[1]},
            "placement_z": t[2],
            "collision_floor_z_under_placement": floor,
            "lowest_vertex_z_range": [lowest.0, lowest.1],
            "lowest_vertex_minus_placement_range": [lowest.0 - f64::from(t[2]), lowest.1 - f64::from(t[2])],
            "lowest_vertex_minus_floor_range": floor.map(|z| [lowest.0 - z, lowest.1 - z]),
            "lowest_bone_but_the_root_z_minus_placement_range": [feet.0 - f64::from(t[2]), feet.1 - f64::from(t[2])],
            "frame0_vs_rest": {
                "named_nodes": frame0.len(),
                "within_0.01": within(0.01),
                "within_0.1": within(0.1),
                "within_1": within(1.0),
                "largest": frame0.iter().take(8).map(|(n, d)| serde_json::json!([n, d])).collect::<Vec<_>>(),
            },
            "pose_and_skin_seconds_per_frame": {"mean": seconds.0 / f64::from(frames.max(1)), "largest": seconds.1},
        }));
    }
    let out = serde_json::json!({
        "cell": args[1],
        "radius": radius,
        "clip": key,
        "duration": clip.duration(),
        "frames": frames,
        "items": report,
    });
    std::fs::write(&args[4], serde_json::to_string_pretty(&out)?)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
