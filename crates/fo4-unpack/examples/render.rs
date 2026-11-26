#![forbid(unsafe_code)]
//! Render one explicitly selected cell from a user's own installation, without a GPU.
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod files;
use fo4_core::{Budget, Manifest};
use fo4_scene::{
    camera::{self, Camera},
    Loader, RuleLevel,
};
use std::path::Path;
type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
/// The terrain key of `summary.json` describes how the terrain was drawn.
const TERRAIN_SHADING: &str = "ground textures weighted per vertex under the pending terrain texture rules, times VCLR; untextured weight as flat neutral grey per triangle by geometric normal; unlit, not game lighting";
fn run() -> AnyResult<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    const USAGE: &str = "usage: render MANIFEST DATA_DIRECTORY CELL_EDITOR_ID build/OUTPUT [--strict|--measured|--under-hypotheses] [--eye X,Y,Z --look X,Y,Z] [--instance-ids] [--collision-overlay] [--terrain] [--around N] [--texture-mib N] [--time SECONDS] [--clock-emissive-multiple] [--clock-emissive-colour] [--clock-falloff] [--clock-translation] [--clock-rotation] [--clock-rotation-xyz] [--clock-scale] [--timeline FILE --at SECONDS --eye-height UNITS [--look-heading DEGREES] [--timeline-nowait] [--timeline-child-flag-agrees] [--timeline-dead-lying | --timeline-dead-left-out] [--timeline-sit-marker] [--timeline-cast [--timeline-target-rotation] [--actor-missing-bone-bind [--actor-missing-bone-nearest]] [--actor-start-clip] [--actor-subgraph-keywords] [--actor-clip-folder] [--actor-start-clip-loop] [--actor-sit-clip] [--actor-subgraph-flag-one] [--actor-model-graph-idle] [--actor-template-bits] [--actor-head-slots] [--actor-head-race-fill] [--actor-unread-texture-slots] [--actor-leveled-seed SEED [--actor-leveled-chance-none] [--actor-leveled-use-all]] [--timeline-ride-model PLUGIN:HEX [--timeline-ride SEQUENCE,START]] [--skinned-models-own-nodes]] [--look-at X,Y,Z | --look-at-actor nearest|speaker]] [--light-cell] [--light-inherits] [--light-points [--light-reference-radius] [--light-fade]] [--light-fog] [--sky-weather] [--light-weather] [--fog-weather] [--assume-hour H] [--assume-weather N] [--exposure-eye] [--exposure-white] [--exposure-cinematic] [--light-vertex-normals] (with --timeline, CELL_EDITOR_ID - draws the player's cell)";
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    // Default: documented facts plus rules measured on the running game.
    let mut level_name = "--measured";
    let mut eye = None;
    let mut look = None;
    let mut instance_ids = false;
    let mut collision_overlay = false;
    let mut terrain = false;
    let mut around = 0u32;
    // The reference's decoded texture total (96 MiB) unless raised for a
    // dense view; a larger total changes which placements the limit drops.
    let mut texture_mib: Option<u64> = None;
    // Seconds of the game clock after load at which free-running
    // controllers are evaluated (`BuildOptions::time`); without the flag
    // the build has no clock and the summary has no `time` key.
    let mut time: Option<f64> = None;
    // The clock's readings that are off by default (`clock::Switches`).
    let mut switches = fo4_scene::clock::Switches::default();
    // A run timeline (`fo4_scene::timeline`): the frame at second `--at` of
    // the run, the camera on the player's track; without it nothing below
    // changes.
    let mut timeline_file: Option<String> = None;
    let mut timeline_at: Option<f64> = None;
    let mut eye_height: Option<f64> = None;
    let mut look_heading: Option<f64> = None;
    let mut timeline_readings = fo4_scene::timeline::Readings::default();
    // --timeline-cast: the frame's actors as their characters
    // (`fo4_scene::timeline::cast`); the aim of the camera (presentation).
    // Without them the frame is drawn as before.
    let mut timeline_cast = false;
    let mut cast_options = fo4_scene::timeline::cast::Options::default();
    let mut aim = fo4_scene::timeline::Aim::Track;
    // --timeline-ride-model, --timeline-ride (`fo4_scene::timeline::ride`).
    let mut ride_model: Option<fo4_plugin::FormKey> = None;
    let mut ride_sequence: Option<(usize, f64)> = None;
    // --skinned-models-own-nodes (`fo4_scene::timeline::ride::add_skinned`).
    let mut skinned_models = false;
    // The light step's readings (`fo4_scene::light`), off by default, and
    // the caller's hour and weather for an exterior.
    let mut light_switches = fo4_scene::light::Switches::default();
    let mut light_assumptions = fo4_scene::light::Assumptions::default();
    let mut rest = args[4..].iter();
    while let Some(arg) = rest.next() {
        if switches.set_flag(arg) || light_switches.set_flag(arg) || cast_options.pose.set_flag(arg)
        {
            continue;
        }
        match arg.as_str() {
            "--strict" | "--measured" | "--under-hypotheses" => level_name = arg,
            "--instance-ids" => instance_ids = true,
            "--collision-overlay" => collision_overlay = true,
            "--terrain" => terrain = true,
            "--around" => {
                around = rest
                    .next()
                    .and_then(|v| v.parse::<u32>().ok())
                    .filter(|n| (1..=fo4_scene::grid::MAX_RADIUS).contains(n))
                    .ok_or(USAGE)?;
            }
            "--texture-mib" => {
                texture_mib = Some(
                    rest.next()
                        .and_then(|v| v.parse::<u64>().ok())
                        .filter(|n| (1..=4096).contains(n))
                        .ok_or(USAGE)?,
                );
            }
            "--time" => {
                time = Some(
                    rest.next()
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|t| t.is_finite() && *t >= 0.)
                        .ok_or(USAGE)?,
                );
            }
            "--assume-hour" => {
                light_assumptions.hour = Some(
                    rest.next()
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|h| (0. ..=24.).contains(h))
                        .ok_or(USAGE)?,
                );
            }
            "--assume-weather" => {
                light_assumptions.weather = Some(
                    rest.next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .ok_or(USAGE)?,
                );
            }
            "--timeline" => timeline_file = Some(rest.next().ok_or(USAGE)?.clone()),
            "--timeline-nowait" => timeline_readings.nowait = true,
            "--timeline-child-flag-agrees" => timeline_readings.child_flag_agrees = true,
            "--timeline-dead-lying" | "--timeline-dead-left-out" => {
                if timeline_readings.dead != fo4_scene::timeline::Dead::Standing {
                    return Err(format!(
                        "one of --timeline-dead-lying and --timeline-dead-left-out\n{USAGE}"
                    )
                    .into());
                }
                timeline_readings.dead = if arg == "--timeline-dead-lying" {
                    fo4_scene::timeline::Dead::Lying
                } else {
                    fo4_scene::timeline::Dead::LeftOut
                };
            }
            "--timeline-sit-marker" => timeline_readings.sit_marker = true,
            "--timeline-cast" => timeline_cast = true,
            "--timeline-target-rotation" => cast_options.target_rotation = true,
            "--actor-missing-bone-bind" => cast_options.missing_bones_at_bind = true,
            "--actor-missing-bone-nearest" => cast_options.missing_bones_nearest = true,
            "--actor-template-bits" => cast_options.look.template_bits = true,
            "--actor-head-slots" => cast_options.look.head_slots = true,
            "--actor-head-race-fill" => cast_options.look.head_race_fill = true,
            "--actor-unread-texture-slots" => cast_options.look.unread_slots = true,
            "--actor-model-graph-idle" => cast_options.model_graph_idle = true,
            "--skinned-models-own-nodes" => skinned_models = true,
            "--actor-leveled-chance-none" => cast_options.look.chance_none = true,
            "--actor-leveled-use-all" => cast_options.look.use_all = true,
            "--timeline-ride-model" => {
                ride_model =
                    Some(fo4_scene::timeline::parse_key(rest.next().ok_or(USAGE)?).ok_or(USAGE)?)
            }
            "--timeline-ride" => {
                let v = rest.next().ok_or(USAGE)?;
                let (k, s) = v.split_once(',').ok_or(USAGE)?;
                ride_sequence = Some((k.parse::<usize>()?, s.parse::<f64>()?));
            }
            "--actor-leveled-seed" => {
                cast_options.look.leveled_seed = Some(rest.next().ok_or(USAGE)?.parse::<u64>()?)
            }
            "--look-at" => aim = fo4_scene::timeline::Aim::Point(point(rest.next().ok_or(USAGE)?)?),
            "--look-at-actor" => {
                aim = match rest.next().map(String::as_str) {
                    Some("nearest") => fo4_scene::timeline::Aim::Nearest,
                    Some("speaker") => fo4_scene::timeline::Aim::Speaker,
                    _ => return Err(USAGE.into()),
                }
            }
            "--at" | "--eye-height" | "--look-heading" => {
                let v = rest
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite())
                    .ok_or(USAGE)?;
                *(match arg.as_str() {
                    "--at" => &mut timeline_at,
                    "--eye-height" => &mut eye_height,
                    _ => &mut look_heading,
                }) = Some(v);
            }
            "--eye" | "--look" => {
                let point = point(rest.next().ok_or(USAGE)?)?;
                *(if arg == "--eye" { &mut eye } else { &mut look }) = Some(point);
            }
            _ => return Err(USAGE.into()),
        }
    }
    if eye.is_some() != look.is_some() {
        return Err(USAGE.into());
    }
    let timeline = match (&timeline_file, timeline_at, eye_height) {
        (Some(f), Some(at), Some(_)) if eye.is_none() => {
            let t = fo4_scene::timeline::Timeline::parse(&std::fs::read_to_string(f)?)?;
            let frame = t.at_with(at, timeline_readings);
            Some((t, frame))
        }
        (None, None, None)
            if look_heading.is_none()
                && timeline_readings == fo4_scene::timeline::Readings::default()
                && !timeline_cast
                && cast_options == fo4_scene::timeline::cast::Options::default()
                && aim == fo4_scene::timeline::Aim::Track =>
        {
            None
        }
        _ => {
            return Err(format!(
                "--timeline needs --at and --eye-height and takes no --eye or --look; the other timeline flags need --timeline\n{USAGE}"
            )
            .into())
        }
    };
    if timeline.is_none() && args[2] == "-" {
        return Err(USAGE.into());
    }
    if (light_switches.reference_radius || light_switches.fade) && !light_switches.points
        || light_switches.inherits && !(light_switches.cell || light_switches.fog)
        || light_assumptions != fo4_scene::light::Assumptions::default()
            && !(light_switches.weather() || light_switches.image_space())
    {
        return Err(format!(
            "--light-reference-radius and --light-fade need --light-points, --light-inherits needs --light-cell or --light-fog, --assume-hour and --assume-weather need a weather switch\n{USAGE}"
        )
        .into());
    }
    if cast_options != fo4_scene::timeline::cast::Options::default() && !timeline_cast {
        return Err(format!(
            "--timeline-target-rotation, --actor-missing-bone-bind and the start clip flags need --timeline-cast\n{USAGE}"
        )
        .into());
    }
    if ((ride_model.is_some() || skinned_models) && !timeline_cast)
        || (ride_sequence.is_some() && ride_model.is_none())
        || ((cast_options.look.chance_none || cast_options.look.use_all)
            && cast_options.look.leveled_seed.is_none())
    {
        return Err(format!(
            "--timeline-ride-model and --skinned-models-own-nodes need --timeline-cast, --timeline-ride needs --timeline-ride-model, --actor-leveled-chance-none and --actor-leveled-use-all need --actor-leveled-seed\n{USAGE}"
        )
        .into());
    }
    let ride = ride_model.map(|reference| fo4_scene::timeline::ride::Options {
        reference,
        sequence: ride_sequence,
    });
    let policy = match level_name {
        "--under-hypotheses" => RuleLevel::Hypotheses,
        "--measured" => RuleLevel::Measured,
        _ => RuleLevel::Facts,
    };
    files::memory()?;
    if std::env::var_os("FO4_MIN_FREE_PERCENT").is_some() {
        std::thread::Builder::new()
            .name("memory-floor".into())
            .spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if let Err(e) = files::memory() {
                    eprintln!("{e}");
                    std::process::exit(3);
                }
            })?;
    }
    let mut build_options = fo4_render::draw::BuildOptions::reference(policy);
    if let Some(mib) = texture_mib {
        build_options.max_texture_bytes = mib << 20;
    }
    build_options.time = time;
    build_options.clock = switches;
    build_options.vertex_normals = light_switches.vertex_normals;
    let mut manifest = Manifest::load(Path::new(&args[0]))?;
    manifest.root = Path::new(&args[1]).canonicalize()?;
    let limits = &manifest.limits;
    let store =
        fo4_plugin::typed::Store::open(&manifest, &Budget::new(limits.max_override_memory))?;
    if !store.diagnostics.is_empty() {
        return Err("PluginDiagnostics: inspect records with fo4-unpack before rendering".into());
    }
    let vfs = fo4_vfs::Vfs::mount(&manifest, &Budget::new(limits.max_vfs_memory))?;
    vfs.check_complete()?;
    let budget = Budget::metered(limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &manifest.language, policy, &budget);
    if switches.any() {
        loader.set_clock_switches(switches, &budget);
    }
    let mut cell_rule = None;
    let mut scene = match &timeline {
        Some((_, frame)) if args[2] == "-" => {
            let (cell, rule) = fo4_scene::timeline::player_cell(&store, frame, &budget)?;
            cell_rule = rule;
            loader.cell(&cell, &budget)?
        }
        _ => loader.by_editor_id(&args[2], &budget)?,
    };
    // --around N: the placed references of the exterior cells within N grid
    // steps join the scene (and, with --terrain, their terrain the draw
    // list). Without the flag the scene is the cell's alone.
    let mut grid = None;
    if around > 0 {
        let a = fo4_scene::grid::around(&store, &scene.cell.key, around, &budget)?;
        let loads = fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        grid = Some((a, loads));
    }
    let applied = match &timeline {
        Some((_, frame)) => {
            let mut a =
                fo4_scene::timeline::apply(&mut scene, &mut loader, frame, policy, &budget)?;
            a.rules.extend(cell_rule);
            Some(a)
        }
        None => None,
    };
    // The frame's actors (markers), before the build so that the cast can
    // be drawn with it; without --timeline-cast the build is the plain one.
    let marked = match &timeline {
        Some((_, frame)) => {
            let space = fo4_scene::timeline::scene_space(&store, &scene);
            Some(fo4_scene::timeline::markers(
                &store, frame, &space, &budget,
            )?)
        }
        None => None,
    };
    let mut cast = match (&marked, timeline_cast) {
        (Some((markers, _, _)), true) => Some(fo4_scene::timeline::cast::resolve(
            &mut loader,
            &scene,
            markers,
            cast_options,
            policy,
            &budget,
        )?),
        _ => None,
    };
    // With a start clip switch, the actors their graph's clip poses
    // (`fo4_scene::timeline::cast::start_poses`); without one the build is
    // the rest-pose one.
    let mut poses = match &mut cast {
        Some(c) if cast_options.pose.any() => {
            let second = timeline.as_ref().map_or(0., |(_, f)| f.second);
            Some(fo4_scene::timeline::cast::start_poses(
                c,
                &loader,
                &vfs,
                &cast_options.pose,
                second,
                policy,
                limits,
                &budget,
            )?)
        }
        _ => None,
    };
    // --timeline-ride-model: the ride's model as one more actor of the cast,
    // posed with --timeline-ride.
    if let (Some(c), Some(o), Some((_, frame))) = (&mut cast, &ride, &timeline) {
        if let Some((k, pose)) = fo4_scene::timeline::ride::add(
            c,
            &mut loader,
            &scene,
            frame,
            o,
            frame.second,
            limits,
            &budget,
        )? {
            poses.get_or_insert_with(Default::default).insert(k, pose);
        }
    }
    // --skinned-models-own-nodes: every placement refused for its skinned
    // shapes as one more actor, after the ride.
    if let (Some(c), true) = (&mut cast, skinned_models) {
        fo4_scene::timeline::ride::add_skinned(c, &loader, &scene, policy, &budget)?;
    }
    let mut built = match (&cast, poses) {
        (Some(c), Some(poses)) => {
            fo4_render::draw::build_with_posed_actors(
                &scene,
                &c.actors,
                poses,
                &loader,
                limits,
                build_options,
                &budget,
            )?
            .0
        }
        (Some(c), None) => fo4_render::draw::build_with_actors(
            &scene,
            &c.actors,
            &loader,
            limits,
            build_options,
            &budget,
        )?,
        (None, _) => fo4_render::draw::build(&scene, &loader, limits, build_options, &budget)?,
    };
    // Without --terrain nothing below changes the draw list or the summary.
    let mut terrain_summary = None;
    if let (true, Some((a, _))) = (terrain, &grid) {
        // Every built piece, the cell first, each tagged after the last
        // placed instance in piece order.
        let set = fo4_walk::terrain::load_with(&store, a, policy, &budget)?;
        let first = u32::try_from(built.instances.len())?;
        let mut cache = fo4_render::terrain::TextureCache::new(build_options);
        let mut textures = Vec::new();
        for (k, t) in set.terrains.iter().enumerate() {
            let tag = first
                .checked_add(u32::try_from(k)?)
                .ok_or("TerrainTagOverflow")?;
            textures.push(fo4_render::terrain::append_textured_terrain(
                &mut built.list,
                t,
                &store,
                &mut loader,
                &mut cache,
                tag,
                policy,
                limits,
                &budget,
            )?);
        }
        terrain_summary = Some(
            serde_json::json!({"drawn":!set.terrains.is_empty(),"first_instance_tag":first,"pieces":set.pieces,"neighbours_searched":set.neighbours_searched,"textures":textures,"landscape_textures":cache.outcomes(),"default_textures":cache.default_outcomes(),"texture_rules":cache.rules,"texture_path_rules":cache.path_rules,"shading":TERRAIN_SHADING}),
        );
    } else if terrain {
        use fo4_scene::terrain::{self, Outcome};
        let options = terrain::Options {
            level: policy,
            diagonal: None,
        };
        terrain_summary = Some(
            match terrain::load(&store, &scene.cell.key, options, &budget)? {
                Outcome::Built(t) => {
                    // Tagged after every placed instance.
                    let tag = u32::try_from(built.instances.len())?;
                    let mut cache = fo4_render::terrain::TextureCache::new(build_options);
                    let textures = fo4_render::terrain::append_textured_terrain(
                        &mut built.list,
                        &t,
                        &store,
                        &mut loader,
                        &mut cache,
                        tag,
                        policy,
                        limits,
                        &budget,
                    )?;
                    serde_json::json!({"drawn":true,"instance_tag":tag,"land":t.land,"grid":t.grid,"diagonal":t.diagonal,"rules":t.rules,"state":t.state,"bounds":t.bounds(),"textures":textures,"landscape_textures":cache.outcomes(),"default_textures":cache.default_outcomes(),"texture_rules":cache.rules,"texture_path_rules":cache.path_rules,"shading":TERRAIN_SHADING})
                }
                Outcome::Refused(r) => serde_json::json!({"drawn":false,"refusal":r}),
            },
        );
    }
    // Draws before this index (the build's and the terrain's) are lit by
    // the light switches, but for effect shader surfaces; the markers after
    // it are not.
    let lit_until = built.list.draws.len();
    // The actors of the timeline's frame as placeholder markers, after
    // every other draw; with --timeline-cast only for the actors the build
    // could not draw as characters.
    let mut timeline_summary = None;
    if let (Some((t, frame)), Some(applied), Some((markers, left, mut rules))) =
        (&timeline, &applied, marked)
    {
        let (shown, first, tags, cast_text, drawn) = match &cast {
            Some(c) => {
                let d = fo4_render::timeline::cast_markers(&mut built, c, policy, &budget)?;
                let tags: Vec<u32> = d.fallback.iter().map(|f| f.1).collect();
                let text = fo4_scene::timeline::cast::report(
                    c,
                    &d.characters,
                    &d.fallback,
                    &d.heads,
                    &d.bind_bones,
                );
                // --timeline-ride-model: whether the build drew the ride.
                let text = text
                    + &fo4_scene::timeline::ride::report(c, &d.extras)
                    + &fo4_scene::timeline::ride::report_skinned(c, &d.extras);
                rules.extend(c.rules.iter().copied());
                if !d.bind_bones.is_empty() {
                    rules.insert(fo4_scene::hypotheses::ACTOR_MISSING_BONE_BIND);
                }
                (
                    d.markers,
                    d.first,
                    tags,
                    Some((
                        text,
                        d.characters.len(),
                        d.fallback.len(),
                        c.posed,
                        !c.poses.is_empty(),
                    )),
                    d.drawn,
                )
            }
            None => {
                let at: Vec<([f64; 3], bool)> = markers
                    .iter()
                    .map(|m| {
                        (
                            m.pos,
                            matches!(m.pose, fo4_scene::timeline::Pose::Lying { .. }),
                        )
                    })
                    .collect();
                let first = fo4_render::timeline::next_tag(&built.list);
                let tags = fo4_render::timeline::append_markers_posed(
                    &mut built.list,
                    &at,
                    first,
                    &budget,
                )?;
                let drawn = markers
                    .iter()
                    .map(|m| (m.reference.clone(), m.pos))
                    .collect();
                (markers, first, tags, None, drawn)
            }
        };
        rules.extend(applied.rules.iter().copied());
        rules.extend(frame.rules.iter().copied());
        let (e, l, camera) = fo4_scene::timeline::camera(
            frame,
            eye_height.unwrap_or_default(),
            look_heading,
            &aim,
            &drawn,
        )?;
        let mut overlay = fo4_scene::timeline::overlay(t, frame, &camera);
        overlay.push_str(&fo4_scene::timeline::report(
            frame, applied, &shown, &left, &rules, first, &tags,
        ));
        if let Some((text, ..)) = &cast_text {
            overlay.push_str(text);
        }
        eye = Some(e);
        look = Some(l);
        let mut json = serde_json::json!({"second":frame.second,"cell":scene.cell.key,"hidden":applied.hidden.len(),"shown":applied.shown.len(),"elsewhere":applied.elsewhere,"chain_changed":applied.chain_changed,"moved":applied.moved.len(),"moved_out":applied.moved_out.len(),"nowait_left":frame.nowait_left,"markers":tags,"actors_left_out":left,"rules":rules,"camera":"presentation: the caller's eye height above the player's track position, looking level along the track's movement or the caller's heading","overlay":"overlay.txt"});
        // The keys are absent without their flags, so the summary stays as before.
        if let (Some((_, characters, fallback, posed, poses)), Some(map)) =
            (&cast_text, json.as_object_mut())
        {
            let pose = if *poses {
                serde_json::json!({"start_clip":posed,"rest":"the skeleton's rest pose for the others (presentation)"})
            } else {
                serde_json::json!("the skeleton's rest pose (presentation)")
            };
            map.insert(
                "cast".into(),
                serde_json::json!({"characters":characters,"markers_instead":fallback,"pose":pose}),
            );
        }
        if let (true, Some(map)) = (aim != fo4_scene::timeline::Aim::Track, json.as_object_mut()) {
            map.insert(
                "aim".into(),
                serde_json::json!(
                    "presentation: the caller's point, or a drawn actor eye height above its place"
                ),
            );
        }
        timeline_summary = Some((overlay, json));
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for draw in &built.list.draws {
        for vertex in &built.list.meshes[draw.mesh].vertices {
            let p = draw.world.apply(vertex.position);
            for k in 0..3 {
                if !p[k].is_finite() {
                    return Err("NonFiniteCameraBounds".into());
                }
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    if min.iter().chain(&max).any(|v| !v.is_finite()) {
        return Err("NoDrawableGeometry".into());
    }
    let (right, top) = camera::reference_slopes(camera::REFERENCE_SIZE);
    let (camera, camera_kind) = match (eye, look) {
        (Some(eye), Some(look)) => (
            camera::look_at(
                eye,
                look,
                camera::reference_frustum(camera::REFERENCE_SIZE),
                camera::REFERENCE_SIZE,
            )?,
            if timeline_summary.is_some() {
                "timeline_track"
            } else {
                "explicit_eye_and_look"
            },
        ),
        _ => {
            // An overview from outside the bounds, not a reconstruction of a player camera.
            let center: [f64; 3] = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
            let radius = (0..3)
                .map(|i| (max[i] - min[i]).powi(2))
                .sum::<f64>()
                .sqrt()
                .max(1.)
                * 0.5;
            let distance = radius / top * 1.2 + radius;
            (
                Camera {
                    rotation: [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]],
                    translation: [center[0], center[1] - distance, center[2]],
                    scale: 1.,
                    frustum: [-right, right, top, -top, 0.1, distance + radius * 3.],
                    viewport: [0., 1., 1., 0.],
                    size: camera::REFERENCE_SIZE,
                },
                "authored_bounds_overview",
            )
        }
    };
    // The frame's light (`fo4_scene::light`); `None` with every light
    // switch off, and then the render is the unlit one.
    let light =
        fo4_scene::light::frame(&store, &scene, light_switches, light_assumptions, &budget)?;
    let mut light = light;
    let lit: Vec<bool> = match &light {
        Some(_) => {
            let effects: std::collections::BTreeSet<usize> =
                built.effect_draws.iter().copied().collect();
            (0..built.list.draws.len())
                .map(|i| i < lit_until && !effects.contains(&i))
                .collect()
        }
        None => Vec::new(),
    };
    // `--exposure-eye`: the mean light on what the camera sees
    // (`Light::expose_seen`).
    if let Some(l) = &mut light {
        if l.exposure.eye {
            l.expose_seen(&fo4_render::seen_lit_points(&built.list, &lit, &camera));
        }
    }
    let mut frame = fo4_render::render_lit(
        &built.list,
        &camera,
        fo4_render::RenderOptions::REFERENCE,
        light
            .as_ref()
            .map(|light| fo4_render::Lighting { light, lit: &lit }),
        limits,
        &budget,
    )?;
    let out = files::output_dir_away_from(&args[3], Some(&manifest.root))?;
    files::output_bytes(&out.join("frame.png"), &frame.png(&budget)?)?;
    if instance_ids {
        // Row-major u32 little-endian per pixel; u32::MAX is background.
        files::output_bytes(&out.join("instances.bin"), &frame.instance_bytes(&budget)?)?;
    }
    let mut overlay = None;
    if collision_overlay {
        // Diagnostic only: frame.png above is already written, unchanged.
        let options = fo4_scene::collision::Options {
            level: policy,
            body_placement: None,
        };
        let c = fo4_scene::collision::cell(&scene, &loader, limits, options, &budget)?;
        let drawn = draw_collision(&mut frame, &camera, &c)?;
        files::output_bytes(&out.join("collision.png"), &frame.png(&budget)?)?;
        overlay = Some(
            serde_json::json!({"body_placement":c.body_placement,"instances":c.instances.len(),"edges_drawn":drawn,"unplaced":c.unplaced.len(),"file":"collision.png"}),
        );
    }
    let mut summary = serde_json::json!({"camera":camera,"camera_kind":camera_kind,"cell":scene.cell.key,"instances":built.instances,"omitted":built.omitted,"statistics":frame.statistics,"hypothesis_policy":level_name,"renderer":"CPU unlit diffuse, no runtime controllers or lighting","memory":budget.measurements()});
    // The keys are absent without their flags, so the summary stays as before.
    if let (Some(t), Some(map)) = (time, summary.as_object_mut()) {
        map.insert(
            "time".into(),
            serde_json::json!({"seconds":t,"animated_draws":built.list.animations.len()}),
        );
    }
    if let (true, Some(map)) = (switches.any(), summary.as_object_mut()) {
        map.insert("clock_switches".into(), serde_json::json!(switches.flags()));
    }
    if let (Some(o), Some(map)) = (overlay, summary.as_object_mut()) {
        map.insert("collision_overlay".into(), o);
    }
    if let (Some(l), Some(map)) = (&light, summary.as_object_mut()) {
        map.insert(
            "light".into(),
            serde_json::json!({"switches":light_switches.flags(),"assumptions":light_assumptions,"rules":l.rules,"points":l.points.len(),"lit_draws":lit.iter().filter(|v| **v).count(),"sky":l.sky.is_some(),"fog":l.fog,"report":l.report}),
        );
    }
    if let (Some((mut overlay, t)), Some(map)) = (timeline_summary, summary.as_object_mut()) {
        // The light's lines are absent without its switches.
        if let Some(l) = &light {
            overlay.push_str(&fo4_scene::light::overlay(l));
        }
        files::output_bytes(&out.join("overlay.txt"), overlay.as_bytes())?;
        map.insert("timeline".into(), t);
    }
    if let (Some(t), Some(map)) = (terrain_summary, summary.as_object_mut()) {
        map.insert("terrain".into(), t);
    }
    if let (Some((a, loads)), Some(map)) = (&grid, summary.as_object_mut()) {
        map.insert(
            "around".into(),
            serde_json::json!({"radius":a.radius,"searched":a.searched,"grid":a.grid,"world":a.world,"cells_scanned":a.cells_scanned,"same_grid_cells":a.same_grid_cells,"cells":loads}),
        );
    }
    files::json(&out.join("summary.json"), &summary)?;
    println!("{}", out.join("frame.png").display());
    Ok(())
}
/// Wireframe of every placed collision instance over the frame: mesh
/// triangle edges, crosses at polytope and convex vertices and sphere
/// centres, capsule segments, in magenta, without depth test. Returns the
/// number of segments drawn.
fn draw_collision(
    frame: &mut fo4_render::Frame,
    camera: &Camera,
    c: &fo4_scene::collision::CellCollision,
) -> AnyResult<u64> {
    use fo4_scene::collision::{BodyPlacement, WorldShape};
    let v = c.body_placement.unwrap_or(BodyPlacement::Ignored);
    let [w, h] = frame.size;
    let near = camera.frustum[4];
    let mut drawn = 0u64;
    let mut segment = |frame: &mut fo4_render::Frame, a: [f64; 3], b: [f64; 3]| {
        let project = |p: [f64; 3]| {
            camera
                .project_coordinates(camera.coordinates(p))
                .filter(|q| q.forward >= near)
        };
        let (Some(pa), Some(pb)) = (project(a), project(b)) else {
            return;
        };
        let [x0, y0] = pa.pixel;
        let [x1, y1] = pb.pixel;
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().min(4096.0) as u32;
        for k in 0..=steps {
            let t = if steps == 0 {
                0.0
            } else {
                f64::from(k) / f64::from(steps)
            };
            let (x, y) = (x0 + t * (x1 - x0), y0 + t * (y1 - y0));
            if x >= 0.0 && y >= 0.0 && x < f64::from(w) && y < f64::from(h) {
                let i = y as usize * w as usize + x as usize;
                if let Some(px) = frame.color.get_mut(i) {
                    *px = [1.0, 0.0, 1.0, 1.0];
                }
            }
        }
        drawn += 1;
    };
    let cross = |p: [f64; 3], r: f64| {
        (0..3).map(move |k| {
            let (mut a, mut b) = (p, p);
            a[k] -= r;
            b[k] += r;
            (a, b)
        })
    };
    let scratch = Budget::new(256 << 20);
    for i in &c.instances {
        match c.world_shape(i, v, &scratch)? {
            WorldShape::Mesh {
                vertices,
                triangles,
                ..
            } => {
                for t in &triangles {
                    let p = t.map(|k| vertices.get(k as usize).copied().unwrap_or([f64::NAN; 3]));
                    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                        segment(frame, p[a], p[b]);
                    }
                }
            }
            WorldShape::Points { vertices, .. } => {
                for p in vertices {
                    for (a, b) in cross(p, 2.0) {
                        segment(frame, a, b);
                    }
                }
            }
            WorldShape::Sphere { center, radius } => {
                for (a, b) in cross(center, radius) {
                    segment(frame, a, b);
                }
            }
            WorldShape::Capsule { points, .. } => segment(frame, points[0], points[1]),
        }
    }
    Ok(drawn)
}
fn point(text: &str) -> AnyResult<[f64; 3]> {
    let mut values = text.split(',').map(str::parse::<f64>);
    let mut out = [0.; 3];
    for v in &mut out {
        *v = values.next().ok_or("ExpectedThreeCoordinates")??;
    }
    if values.next().is_some() || out.iter().any(|v| !v.is_finite()) {
        return Err("ExpectedThreeCoordinates".into());
    }
    Ok(out)
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
