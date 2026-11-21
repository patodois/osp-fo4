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
}

/// The rules walk mode relies on before any collision is placed: the
/// controller's own (every movement number, triangle sidedness).
pub fn walk_rules() -> Vec<&'static str> {
    let mut rules = fo4_walk::rules::PARAMS.to_vec();
    rules.push(fo4_walk::rules::TRIANGLE_SIDES);
    rules
}

pub fn cell(
    manifest: &str,
    data: &str,
    cell: &str,
    level: RuleLevel,
    walk: bool,
    terrain: bool,
) -> AnyResult<Loaded> {
    if walk {
        fo4_walk::accept(&walk_rules(), level, fo4_scene::hypotheses::status)?;
    }
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
    let budget = Budget::metered(limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &manifest.language, level, &budget);
    let scene = loader.by_editor_id(cell, &budget)?;
    let mut build = fo4_render::draw::build(
        &scene,
        &loader,
        limits,
        BuildOptions::reference(level),
        &budget,
    )?;
    // The terrain of the cell and, for walking, of its neighbours. Without
    // --terrain and --walk nothing is loaded and the draw list is unchanged.
    let ground = if walk || terrain {
        Some(fo4_walk::terrain::load_around(
            &store,
            &scene.cell.key,
            level,
            walk,
            &budget,
        )?)
    } else {
        None
    };
    let terrain_outcome = ground
        .as_ref()
        .and_then(|g| g.own())
        .map(|p| p.outcome.clone());
    if terrain {
        let own = ground
            .as_ref()
            .filter(|g| g.own_built())
            .and_then(|g| g.terrains.first());
        match own {
            Some(t) => {
                // Tagged after every placed instance, as the CPU render does.
                let tag = u32::try_from(build.instances.len())?;
                fo4_render::terrain::append_terrain(&mut build.list, t, tag, &budget)?;
                println!("terrain drawn: grid {:?}, tag {tag}", t.grid);
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
    Ok(Loaded {
        build,
        data_root: manifest.root.clone(),
        walk,
    })
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
