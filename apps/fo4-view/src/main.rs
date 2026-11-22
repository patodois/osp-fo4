#![forbid(unsafe_code)]
//! Interactive GPU viewer for one cell, checked against the CPU reference.
//! See `cli::USAGE` and the README section "GPU viewer".
mod cli;
mod compare;
mod coords;
mod fly;
mod gpu;
mod load;
mod scene;
mod walk;

use cli::Command;
use fo4_render::RenderOptions;
use fo4_scene::camera;
use load::AnyResult;

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn load_scene(
    s: &cli::Scene,
    walk: bool,
) -> AnyResult<(scene::GpuScene, RenderOptions, load::Loaded)> {
    let started = std::time::Instant::now();
    let mut loaded = load::cell(
        &s.manifest,
        &s.data,
        &s.cell,
        s.level,
        walk,
        s.terrain,
        s.around,
        load::build_options(s.textures, s.level),
        s.scene_memory,
    )?;
    let options = RenderOptions {
        backface_cull: s.backface_cull,
        ..RenderOptions::REFERENCE
    };
    let gpu = scene::convert(&mut loaded.build.list, &options)?;
    println!(
        "cell {}: {} placements, {} omitted; GPU {} meshes, {} textures, {} surfaces, {} instances ({} blended)",
        s.cell,
        loaded.build.instances.len(),
        loaded.build.omitted.len(),
        gpu.meshes.len(),
        gpu.textures.len(),
        gpu.surfaces.len(),
        gpu.instances.len(),
        gpu.blended_instances(),
    );
    // Not with --reference-textures, whose output is the reference's.
    if s.textures != cli::Textures::Reference {
        let mib = |n: usize| n as f64 / f64::from(1u32 << 20);
        let (blocks, decoded): (Vec<_>, Vec<_>) = gpu
            .textures
            .iter()
            .partition(|t| matches!(t.format, scene::TexelFormat::Blocks(_)));
        println!(
            "textures {:?}: {} as stored blocks ({:.1} MiB), {} decoded to RGBA16F ({:.1} MiB)",
            s.textures,
            blocks.len(),
            mib(blocks.iter().map(|t| t.bytes.len()).sum()),
            decoded.len(),
            mib(decoded.iter().map(|t| t.bytes.len()).sum()),
        );
    }
    // Only with --around, so the default output is unchanged.
    if s.around > 0 {
        println!(
            "loaded and converted in {:.2} s",
            started.elapsed().as_secs_f64()
        );
    }
    Ok((gpu, options, loaded))
}

/// Headless walk: the walker dropped with its eye at `eye`, settled
/// without input, and the eye and look point it ends with (same direction).
fn settled_pose(
    world: &fo4_walk::World,
    eye: [f64; 3],
    look: [f64; 3],
) -> AnyResult<([f64; 3], [f64; 3])> {
    let start = fly::FlyState::from_eye_look(eye, look)?;
    let mut view = walk::WalkView::from_fly(
        start,
        fo4_walk::Params::PROVISIONAL,
        fo4_walk::Settings::DEFAULT,
    )?;
    let ticks = view.character.settle(world, 600);
    let c = &view.character;
    let ground = c.ground.and_then(|g| world.sources.get(g.source as usize));
    println!(
        "walk: settled in {ticks} ticks, feet {:.3},{:.3},{:.3}, {}, ground {}",
        c.feet[0],
        c.feet[1],
        c.feet[2],
        if c.grounded { "on ground" } else { "in air" },
        match ground {
            None => "none".into(),
            Some(s) => match (s.terrain, s.instance) {
                (Some(grid), _) => format!("terrain {grid:?}"),
                (None, Some(i)) => format!("instance {i}"),
                (None, None) => s.label.clone().unwrap_or_default(),
            },
        }
    );
    let eye = view.character.eye();
    let d: [f64; 3] = std::array::from_fn(|i| look[i] - start.eye[i]);
    println!(
        "walk pose: --eye {:.3},{:.3},{:.3} --look {:.3},{:.3},{:.3}",
        eye[0],
        eye[1],
        eye[2],
        eye[0] + d[0],
        eye[1] + d[1],
        eye[2] + d[2]
    );
    Ok((eye, std::array::from_fn(|i| eye[i] + d[i])))
}

fn run() -> AnyResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse(&args)? {
        Command::Headless {
            scene: s,
            size,
            out,
            tags,
            walk,
        } => {
            let (mut gpu, options, loaded) = load_scene(&s, walk)?;
            let root = Some(loaded.data_root.as_path());
            // Open the outputs first: a refused path fails before rendering.
            let mut out_file = load::output_file(&out, root)?;
            let mut tags_file = tags
                .as_deref()
                .map(|t| load::output_file(t, root))
                .transpose()?;
            let (eye, look) = match &loaded.walk {
                Some(world) => settled_pose(world, s.eye, s.look)?,
                None => (s.eye, s.look),
            };
            drop(loaded);
            let cam = camera::look_at(eye, look, camera::reference_frustum(size), size)?;
            let frames = gpu::headless(&mut gpu, &cam, options.clear, tags.is_some())?;
            use std::io::Write;
            out_file.write_all(&load::png_rgba8(size, &frames.color)?)?;
            println!("{out}");
            if let (Some(file), Some(bytes), Some(path)) = (tags_file.as_mut(), &frames.tags, &tags)
            {
                file.write_all(bytes)?;
                println!("{path}");
            }
            Ok(())
        }
        Command::Window {
            scene: s,
            fly,
            walk,
        } => {
            let (gpu, options, loaded) = load_scene(&s, walk)?;
            // Keep only the walking world; the draw list was converted.
            let load::Loaded {
                walk: world, build, ..
            } = loaded;
            drop(build);
            let start = fly::FlyState::from_eye_look(s.eye, s.look)?;
            gpu::window(gpu, start, fly, world, options.clear)?;
            Ok(())
        }
        Command::Compare {
            cpu,
            gpu,
            diff,
            tags,
        } => {
            let a = load::read_png(&cpu)?;
            let b = load::read_png(&gpu)?;
            if a.size != b.size {
                return Err(
                    format!("ComparisonSizeMismatch: CPU {:?} GPU {:?}", a.size, b.size).into(),
                );
            }
            let tags = match &tags {
                Some((x, y)) => Some((
                    compare::tags_from_bytes(&std::fs::read(x)?)?,
                    compare::tags_from_bytes(&std::fs::read(y)?)?,
                )),
                None => None,
            };
            let report = compare::compare(&compare::Inputs {
                size: a.size,
                cpu: &a.rgba,
                gpu: &b.rgba,
                tags: tags.as_ref().map(|(x, y)| (x.as_slice(), y.as_slice())),
                clear: compare::clear_rgb(RenderOptions::REFERENCE.clear),
            })?;
            let pct = |n: u64| 100. * n as f64 / report.pixels as f64;
            println!(
                "pixels {} ({}x{}); coverage from {}",
                report.pixels,
                a.size[0],
                a.size[1],
                if report.coverage_from_tags {
                    "instance tags"
                } else {
                    "colour differs from the clear colour"
                }
            );
            println!(
                "both drew {} ({:.3}%), neither {} ({:.3}%), CPU only {} ({:.3}%), GPU only {} ({:.3}%)",
                report.both,
                pct(report.both),
                report.neither,
                pct(report.neither),
                report.cpu_only,
                pct(report.cpu_only),
                report.gpu_only,
                pct(report.gpu_only)
            );
            println!("coverage agreement {:.6}", report.agreement());
            if let Some(same) = report.same_tag {
                println!(
                    "same instance tag where both drew: {same} of {} ({:.3}%)",
                    report.both,
                    100. * same as f64 / report.both.max(1) as f64
                );
            }
            let [mr, mg, mb] = report.mean_abs;
            let [pr, pg, pb] = report.p95_abs;
            let [xr, xg, xb] = report.max_abs;
            println!("|CPU-GPU| where both drew, 8-bit steps: mean R {mr:.3} G {mg:.3} B {mb:.3}; p95 R {pr} G {pg} B {pb}; max R {xr} G {xg} B {xb}");
            let rgba: Vec<u8> = report
                .image
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect();
            load::write(&diff, None, &load::png_rgba8(a.size, &rgba)?)?;
            println!("{diff}: black neither drew, red CPU only, blue GPU only, grey 4 x largest channel difference");
            Ok(())
        }
    }
}
