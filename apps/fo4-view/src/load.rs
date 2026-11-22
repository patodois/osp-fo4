//! Cell loading and output files. The loading sequence is the `render`
//! example's (crates/fo4-unpack/examples/render.rs), so both renderers see the
//! same draw list; the output rules are that example's too.
use fo4_core::{Budget, Manifest};
use fo4_render::draw::{Build, BuildOptions};
use fo4_scene::{Loader, RuleLevel};
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
};

pub type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

pub struct Loaded {
    pub build: Build,
    pub data_root: PathBuf,
    /// The cell's placed collision as a walking world, when asked for,
    /// with the terrain of the cell and its neighbours when it loads.
    pub walk: Option<fo4_walk::World>,
    /// `--actor-clip`: what the window needs to pose the actors again.
    pub animation: Option<crate::clip::Animation>,
}

/// The manifest with its data root, the plugins and the mounted archives.
pub struct Sources {
    pub manifest: Manifest,
    pub store: fo4_plugin::typed::Store,
    pub vfs: fo4_vfs::Vfs,
}

/// Opens the plugins and mounts the archives of `manifest` with `data` as
/// its root.
pub fn open(manifest: &str, data: &str) -> AnyResult<Sources> {
    let mut manifest = Manifest::load(Path::new(manifest))?;
    manifest.root = Path::new(data).canonicalize()?;
    let limits = &manifest.limits;
    let store =
        fo4_plugin::typed::Store::open(&manifest, &Budget::new(limits.max_override_memory))?;
    if !store.diagnostics.is_empty() {
        return Err("PluginDiagnostics: inspect records with fo4-unpack before rendering".into());
    }
    let vfs = fo4_vfs::Vfs::mount(&manifest, &Budget::new(limits.max_vfs_memory))?;
    vfs.check_complete()?;
    Ok(Sources {
        manifest,
        store,
        vfs,
    })
}

/// The rules walk mode relies on before any collision is placed: the
/// controller's own (every movement number, triangle sidedness).
pub fn walk_rules() -> Vec<&'static str> {
    let mut rules = fo4_walk::rules::PARAMS.to_vec();
    rules.push(fo4_walk::rules::TRIANGLE_SIDES);
    rules
}

/// Stored bytes the viewer keeps as blocks at most, apart from the decoded
/// textures' budget of the reference options.
pub const BLOCK_BYTES: u64 = 512 << 20;

/// Build options of a texture policy at `level`.
pub fn build_options(textures: crate::cli::Textures, level: RuleLevel) -> BuildOptions {
    match textures {
        crate::cli::Textures::Reference => BuildOptions::reference(level),
        crate::cli::Textures::Blocks {
            cap,
            alpha_tested_cap,
        } => BuildOptions::with_blocks(
            level,
            fo4_render::draw::BlockOptions {
                max_dimension: cap,
                max_alpha_tested_dimension: alpha_tested_cap,
                max_bytes: BLOCK_BYTES,
            },
        ),
    }
}

/// Named in the error when the scene's credits exceed `--scene-memory`.
pub const SCENE_MEMORY: &str = "credits of the loaded scene; raise with --scene-memory MIB";

/// The viewer's allocation account: `scene_memory` bounds what the scene
/// keeps, and the manifest's `max_memory` each child scope (one cell read,
/// one asset decode, one terrain piece's textures).
pub fn scene_budget(scene_memory: u64, limits: &fo4_core::Limits) -> Budget {
    Budget::metered(scene_memory)
        .with_scope_limit(limits.max_memory)
        .named(SCENE_MEMORY)
}

/// `f` in a child scope of `budget`, whose charges `budget` keeps when `f`
/// succeeds: what one cell or one piece reads is bounded by the scope
/// limit, and what the scene keeps of it by the scene's ceiling.
fn scoped<T>(
    budget: &Budget,
    f: impl FnOnce(&Budget) -> fo4_core::Result<T>,
) -> fo4_core::Result<T> {
    let scope = budget.child();
    let out = f(&scope)?;
    scope.commit(budget)?;
    Ok(out)
}

/// One line: how many placed actors the scene has, how many are drawn and
/// how many are left out.
pub fn actor_summary(actors: &fo4_scene::actor::Actors, build: &fo4_render::draw::Build) -> String {
    let omitted: std::collections::BTreeSet<u32> =
        build.omitted.iter().map(|o| o.instance).collect();
    let drawn = build.actors.iter().filter(|i| !omitted.contains(i)).count();
    format!(
        "actors: {} placed ({} more initially disabled, {} distant), {} drawn, {} left out; {} rigid shapes of drawn models not drawn",
        actors.actors.len(),
        actors.disabled,
        actors.distant,
        drawn,
        build.actors.len() - drawn,
        build.actor_rigid_shapes,
    )
}
/// One line per placed actor: drawn with what, or why not.
pub fn actor_lines(
    actors: &fo4_scene::actor::Actors,
    build: &fo4_render::draw::Build,
) -> Vec<String> {
    let mut out = Vec::new();
    for (a, index) in actors.actors.iter().zip(&build.actors) {
        let id = format!("{}:{:06x}", a.reference.plugin, a.reference.object_id);
        let omission = build.omitted.iter().find(|o| o.instance == *index);
        let notes = if a.notes.is_empty() {
            String::new()
        } else {
            format!("; {}", a.notes.join("; "))
        };
        let head: Vec<String> = build
            .actor_head_omissions
            .iter()
            .filter(|o| o.instance == *index)
            .flat_map(|o| o.causes.iter().map(|e| format!("{} {}", e.code, e.found)))
            .collect();
        let head = if head.is_empty() {
            String::new()
        } else {
            format!("; head parts left out: {}", head.join(", "))
        };
        match (omission, &a.plan) {
            (None, Some(plan)) => out.push(format!(
                "actor {id} (tag {index}): drawn, section {}, {} parts{notes}{head}",
                plan.section + 1,
                plan.parts.len()
            )),
            (Some(o), Some(_)) => {
                let causes: Vec<String> = o
                    .causes
                    .iter()
                    .map(|e| match e.code.as_str() {
                        "ActorNotDrawable" => format!("{} at this level", e.code),
                        _ => format!("{} {}", e.code, e.found),
                    })
                    .collect();
                out.push(format!(
                    "actor {id} (tag {index}): not drawn: {}{notes}",
                    causes.join(", ")
                ))
            }
            (Some(_), None) => out.push(format!("actor {id} (tag {index}): not drawn{notes}")),
            (None, None) => out.push(format!("actor {id} (tag {index}): not drawn{notes}")),
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn cell(
    manifest: &str,
    data: &str,
    cell: &str,
    level: RuleLevel,
    walk: bool,
    terrain: bool,
    around: u32,
    options: BuildOptions,
    scene_memory: u64,
    actors: bool,
    clip: Option<&crate::clip::Options>,
) -> AnyResult<Loaded> {
    if walk {
        fo4_walk::accept(&walk_rules(), level, fo4_scene::hypotheses::status)?;
    }
    let Sources {
        manifest,
        store,
        vfs,
    } = open(manifest, data)?;
    let limits = &manifest.limits;
    let budget = scene_budget(scene_memory, limits);
    let mut loader = Loader::new(&store, &vfs, &manifest.language, level, &budget);
    let mut scene = scoped(&budget, |b| loader.by_editor_id(cell, b))?;
    // --around N: the placed references of the exterior cells within N grid
    // steps join the scene, as in the CPU render. Without it nothing changes.
    let grid = if around > 0 {
        let started = std::time::Instant::now();
        let a = scoped(&budget, |b| {
            fo4_scene::grid::around(&store, &scene.cell.key, around, b)
        })?;
        let loads = fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        let cells: Vec<String> = loads
            .iter()
            .map(|l| format!("{:?} {} {}", l.offset, l.outcome, l.instances))
            .collect();
        println!(
            "around {}: {} (searched {}, {} cells scanned, {:.2} s): {}",
            a.radius,
            if a.searched { "exterior" } else { "no grid" },
            a.searched,
            a.cells_scanned,
            started.elapsed().as_secs_f64(),
            cells.join(", ")
        );
        Some(a)
    } else {
        None
    };
    let mut animation = None;
    if clip.is_some() && !actors {
        return Err("ActorClipWithoutActors: --actor-clip poses the drawn actors".into());
    }
    let mut build = if actors {
        // Placed actors, drawn after the placed references (`--no-actors`
        // leaves them out and the draw list is the one before).
        let started = std::time::Instant::now();
        let actors = scoped(&budget, |b| {
            fo4_scene::actor::resolve(&scene, &mut loader, b)
        })?;
        let resolved = started.elapsed().as_secs_f64();
        let build = match clip {
            // Without --actor-clip the draw list is the rest-pose one.
            None => fo4_render::draw::build_with_actors(
                &scene, &actors, &loader, limits, options, &budget,
            )?,
            Some(o) => {
                let started = std::time::Instant::now();
                let (poses, mut a) =
                    crate::clip::prepare(o, &actors, &loader, &vfs, level, limits, &budget)?;
                let (build, recorded) = fo4_render::draw::build_with_posed_actors(
                    &scene, &actors, poses, &loader, limits, options, &budget,
                )?;
                a.meshes = recorded;
                println!(
                    "actor clip read, posed and skinned in {:.2} s",
                    started.elapsed().as_secs_f64()
                );
                animation = Some(a);
                build
            }
        };
        println!("{}", actor_summary(&actors, &build));
        for line in actor_lines(&actors, &build) {
            println!("  {line}");
        }
        println!("actors resolved and their models read in {resolved:.2} s");
        build
    } else {
        fo4_render::draw::build(&scene, &loader, limits, options, &budget)?
    };
    // The terrain of the cell and, for walking, of its neighbours. Without
    // --terrain and --walk nothing is loaded and the draw list is unchanged.
    let ground = match (&grid, walk || terrain) {
        (Some(a), true) => Some(fo4_walk::terrain::load_with(&store, a, level, &budget)?),
        (None, true) => Some(fo4_walk::terrain::load_around(
            &store,
            &scene.cell.key,
            level,
            walk,
            &budget,
        )?),
        (_, false) => None,
    };
    let terrain_outcome = ground
        .as_ref()
        .and_then(|g| g.own())
        .map(|p| p.outcome.clone());
    if let (true, Some(g)) = (terrain && grid.is_some(), &ground) {
        // Every built piece, the cell first, tagged after the last placed
        // instance in piece order, as the CPU render does.
        let first = u32::try_from(build.instances.len())?;
        let mut last = first;
        let mut cache = fo4_render::terrain::TextureCache::new(options);
        let mut textured = Vec::new();
        for (k, t) in g.terrains.iter().enumerate() {
            last = first
                .checked_add(u32::try_from(k)?)
                .ok_or("TerrainTagOverflow")?;
            let r = scoped(&budget, |b| {
                fo4_render::terrain::append_textured_terrain(
                    &mut build.list,
                    t,
                    &store,
                    &mut loader,
                    &mut cache,
                    last,
                    level,
                    limits,
                    b,
                )
            })?;
            textured.push(texture_line(&r));
        }
        let pieces: Vec<String> = g
            .pieces
            .iter()
            .map(|p| format!("{} {}", p.name, p.outcome))
            .collect();
        println!(
            "terrain drawn: {} pieces, tags {first} to {last}; {}",
            g.terrains.len(),
            pieces.join(", ")
        );
        println!(
            "terrain textures: {}; landscape textures {:?}; default textures {:?}",
            textured.join("; "),
            cache.outcomes(),
            cache.default_outcomes()
        );
    } else if terrain {
        let own = ground
            .as_ref()
            .filter(|g| g.own_built())
            .and_then(|g| g.terrains.first());
        match own {
            Some(t) => {
                // Tagged after every placed instance, as the CPU render does.
                let tag = u32::try_from(build.instances.len())?;
                let mut cache = fo4_render::terrain::TextureCache::new(options);
                let r = scoped(&budget, |b| {
                    fo4_render::terrain::append_textured_terrain(
                        &mut build.list,
                        t,
                        &store,
                        &mut loader,
                        &mut cache,
                        tag,
                        level,
                        limits,
                        b,
                    )
                })?;
                println!("terrain drawn: grid {:?}, tag {tag}", t.grid);
                println!(
                    "terrain textures: {}; landscape textures {:?}; default textures {:?}",
                    texture_line(&r),
                    cache.outcomes(),
                    cache.default_outcomes()
                );
            }
            None => println!(
                "terrain not drawn: {}",
                terrain_outcome.as_deref().unwrap_or("unknown")
            ),
        }
    }
    let walk = if walk {
        // The same collision and world the `walk` report uses.
        let options = fo4_scene::collision::Options {
            level,
            body_placement: None,
        };
        let c = fo4_scene::collision::cell(&scene, &loader, limits, options, &budget)?;
        let placement = c
            .body_placement
            .ok_or("BodyPlacementRuleNotAccepted: no body placement level at this rule level")?;
        let mut world = fo4_walk::World::build(&c, placement, &budget)?;
        if let Some(mut g) = ground {
            g.add_to(&mut world, &budget)?;
            let pieces: Vec<String> = g
                .pieces
                .iter()
                .map(|p| format!("{} {}", p.name, p.outcome))
                .collect();
            println!("walk terrain: {}", pieces.join(", "));
        }
        fo4_walk::accept(&world.rules, level, fo4_scene::hypotheses::status)?;
        println!(
            "walk world: {} triangles, {} hulls, {} spheres, {} capsules from {} instances ({} not placed)",
            world.stats.triangles,
            world.stats.hulls,
            world.stats.spheres,
            world.stats.capsules,
            world.stats.instances,
            c.unplaced.len(),
        );
        Some(world)
    } else {
        None
    };
    if around > 0 {
        let mib = |n: u64| n as f64 / f64::from(1u32 << 20);
        println!(
            "scene memory: {:.1} MiB of {:.0} MiB credits kept (--scene-memory), peak {:.1} MiB; each read within {:.0} MiB (manifest max_memory)",
            mib(budget.used()),
            mib(budget.max()),
            mib(budget.measurements().map_or(0, |m| m.peak)),
            mib(budget.scope_max()),
        );
    }
    Ok(Loaded {
        build,
        data_root: manifest.root.clone(),
        walk,
        animation,
    })
}

/// One line of what a terrain piece's texture passes drew.
fn texture_line(r: &fo4_render::terrain::TexturedReport) -> String {
    match &r.refusal {
        Some(refusal) => format!("{:?} untextured ({})", r.grid, refusal.code),
        None => format!(
            "{:?} {} quadrants textured, {} untextured {:?}, {} layers, {} passes, weight {:?}, texture refusals {:?}, default {}, vertex colours {}",
            r.grid,
            r.quadrants_textured,
            r.quadrants_untextured,
            r.quadrant_refusals,
            r.layers,
            r.passes,
            r.weight_share,
            r.texture_refusals,
            r.default_texture.as_deref().unwrap_or("none"),
            r.vertex_colors
        ),
    }
}

/// A new file for `arg`: under `./build`, no `..`, no symbolic link on the
/// way, outside the game data, never overwriting.
pub fn output_file(arg: &str, data_root: Option<&Path>) -> AnyResult<std::fs::File> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let path = Path::new(arg);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("RenderOutputParentTraversal".into());
    }
    let out = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    if !out.starts_with(cwd.join("build")) {
        return Err("RenderOutputOutsideBuild".into());
    }
    for ancestor in out.ancestors() {
        if let Ok(m) = std::fs::symlink_metadata(ancestor) {
            if m.file_type().is_symlink() {
                return Err("RenderOutputSymlink".into());
            }
        }
    }
    if data_root.is_some_and(|root| out.starts_with(root)) {
        return Err("RenderOutputInsideInputRoot".into());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .map_err(|e| format!("{}: {e}", out.display()))?)
}

pub fn write(arg: &str, data_root: Option<&Path>, bytes: &[u8]) -> AnyResult<()> {
    let mut f = output_file(arg, data_root)?;
    f.write_all(bytes)?;
    Ok(())
}

/// PNG of RGBA8 pixels (alpha dropped), through the reference encoder.
pub fn png_rgba8(size: [u32; 2], rgba: &[u8]) -> AnyResult<Vec<u8>> {
    let pixels: Vec<[f32; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p.map(|v| f32::from(v) / 255.))
        .collect();
    let budget = Budget::new(64 << 20);
    Ok(fo4_render::png::encode(size, &pixels, &budget)?)
}

pub fn read_png(path: &str) -> AnyResult<fo4_render::png::Png> {
    let bytes = std::fs::read(path)?;
    Ok(fo4_render::png::decode(
        &bytes,
        &fo4_core::Limits::default(),
        &Budget::new(256 << 20),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgba8_round_trips_through_the_reference_png_codec() {
        let rgba: Vec<u8> = (0..=255u8).flat_map(|v| [v, 255 - v, v / 2, 255]).collect();
        let png = png_rgba8([16, 16], &rgba).unwrap();
        let back =
            fo4_render::png::decode(&png, &fo4_core::Limits::default(), &Budget::new(16 << 20))
                .unwrap();
        assert_eq!(back.size, [16, 16]);
        assert_eq!(back.rgba, rgba);
    }
    #[test]
    fn output_outside_build_or_with_parent_steps_is_refused() {
        assert!(output_file("elsewhere/a.png", None).is_err());
        assert!(output_file("build/../a.png", None).is_err());
    }
}
