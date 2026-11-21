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
fn run() -> AnyResult<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    const USAGE: &str = "usage: render MANIFEST DATA_DIRECTORY CELL_EDITOR_ID build/OUTPUT [--strict|--measured|--under-hypotheses] [--eye X,Y,Z --look X,Y,Z] [--instance-ids]";
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    // Default: documented facts plus rules measured on the running game.
    let mut level_name = "--measured";
    let mut eye = None;
    let mut look = None;
    let mut instance_ids = false;
    let mut rest = args[4..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--strict" | "--measured" | "--under-hypotheses" => level_name = arg,
            "--instance-ids" => instance_ids = true,
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
    let scene = loader.by_editor_id(&args[2], &budget)?;
    let built = fo4_render::draw::build(
        &scene,
        &loader,
        limits,
        fo4_render::draw::BuildOptions::reference(policy),
        &budget,
    )?;
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
    let frame = fo4_render::render(
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
    files::json(
        &out.join("summary.json"),
        &serde_json::json!({"camera":camera,"camera_kind":camera_kind,"cell":scene.cell.key,"instances":built.instances,"omitted":built.omitted,"statistics":frame.statistics,"hypothesis_policy":level_name,"renderer":"CPU unlit diffuse, no runtime controllers or lighting","memory":budget.measurements()}),
    )?;
    println!("{}", out.join("frame.png").display());
    Ok(())
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
