#![forbid(unsafe_code)]
//! Render one explicitly selected cell from a user's own installation, without a GPU.
#[allow(dead_code)]
#[path = "support/mod.rs"]
mod files;
use fo4_core::{Budget, Manifest};
use fo4_scene::{camera::Camera, Loader, RuleLevel};
use std::path::Path;
type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
fn run() -> AnyResult<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(4..=5).contains(&args.len()) {
        return Err("usage: render MANIFEST DATA_DIRECTORY CELL_EDITOR_ID build/OUTPUT [--strict|--measured|--under-hypotheses]".into());
    }
    // Default: documented facts plus rules measured on the running game.
    let level_name = args.get(4).map_or("--measured", String::as_str);
    let policy = match level_name {
        "--under-hypotheses" => RuleLevel::Hypotheses,
        "--measured" => RuleLevel::Measured,
        "--strict" => RuleLevel::Facts,
        _ => return Err("UnknownRuleLevel".into()),
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
        fo4_render::draw::BuildOptions {
            max_texture_dimension: 256,
            max_texture_bytes: 96 << 20,
            level: policy,
        },
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
    // An authored overview, not a reconstruction of a measured player camera.
    let center: [f64; 3] = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
    let radius = (0..3)
        .map(|i| (max[i] - min[i]).powi(2))
        .sum::<f64>()
        .sqrt()
        .max(1.)
        * 0.5;
    let right = (40f64.to_radians()).tan();
    let top = right * 720. / 1280.;
    let distance = radius / top * 1.2 + radius;
    let camera = Camera {
        rotation: [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]],
        translation: [center[0], center[1] - distance, center[2]],
        scale: 1.,
        frustum: [-right, right, top, -top, 0.1, distance + radius * 3.],
        viewport: [0., 1., 1., 0.],
        size: [1280, 720],
    };
    let frame = fo4_render::render(
        &built.list,
        &camera,
        fo4_render::RenderOptions {
            workers: 1,
            bilinear: true,
            backface_cull: false,
            clear: [0.025, 0.025, 0.025, 1.],
        },
        limits,
        &budget,
    )?;
    let out = files::output_dir_away_from(&args[3], Some(&manifest.root))?;
    files::output_bytes(&out.join("frame.png"), &frame.png(&budget)?)?;
    files::json(
        &out.join("summary.json"),
        &serde_json::json!({"camera":camera,"camera_kind":"authored_bounds_overview","cell":scene.cell.key,"instances":built.instances,"omitted":built.omitted,"statistics":frame.statistics,"hypothesis_policy":level_name,"renderer":"CPU unlit diffuse, no runtime controllers or lighting","memory":budget.measurements()}),
    )?;
    println!("{}", out.join("frame.png").display());
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
