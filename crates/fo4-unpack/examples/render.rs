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
    const USAGE: &str = "usage: render MANIFEST DATA_DIRECTORY CELL_EDITOR_ID build/OUTPUT [--strict|--measured|--under-hypotheses] [--eye X,Y,Z --look X,Y,Z] [--instance-ids] [--collision-overlay] [--terrain] [--around N] [--texture-mib N] [--time SECONDS] [--clock-emissive-multiple] [--clock-emissive-colour] [--clock-falloff] [--clock-translation] [--clock-rotation] [--clock-rotation-xyz] [--clock-scale]";
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
    let mut rest = args[4..].iter();
    while let Some(arg) = rest.next() {
        if switches.set_flag(arg) {
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
    let mut scene = loader.by_editor_id(&args[2], &budget)?;
    // --around N: the placed references of the exterior cells within N grid
    // steps join the scene (and, with --terrain, their terrain the draw
    // list). Without the flag the scene is the cell's alone.
    let mut grid = None;
    if around > 0 {
        let a = fo4_scene::grid::around(&store, &scene.cell.key, around, &budget)?;
        let loads = fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        grid = Some((a, loads));
    }
    let mut built = fo4_render::draw::build(&scene, &loader, limits, build_options, &budget)?;
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
            "explicit_eye_and_look",
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
    let mut frame = fo4_render::render(
        &built.list,
        &camera,
        fo4_render::RenderOptions::REFERENCE,
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
