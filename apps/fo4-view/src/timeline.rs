//! `--timeline FILE --at SECONDS --eye-height UNITS` (headless): one frame
//! of a quest run's timeline (`fo4_scene::timeline`, `spec/timeline.md`),
//! drawn as the CPU render's `--timeline` draws it: the player's cell
//! (CELL `-`), the references the run disabled left out and those it
//! enabled or moved put in or moved (pending rules, `--under-hypotheses`),
//! the actors as placeholder markers (`fo4_render::timeline`), or with
//! `--timeline-cast` as their characters (`fo4_scene::timeline::cast`), the
//! camera on the player's track (aimed with `--look-at` or
//! `--look-at-actor`) and an overlay text beside the frame. The camera,
//! the markers, the rest pose and the overlay are presentation.
use crate::load::AnyResult;
use fo4_scene::timeline::{self as tl, Frame, Timeline};

/// What the command line names.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub file: String,
    /// Second of the run's real-time clock.
    pub at: f64,
    /// The caller's eye height above the track's position.
    pub eye_height: f64,
    /// The caller's heading in degrees (counter-clockwise from +X), else
    /// the track's direction of movement.
    pub heading: Option<f64>,
    /// `--timeline-nowait` (`timeline_nowait_visibility_v1`).
    pub nowait: bool,
    /// `--overlay FILE`: the overlay text, under `./build`.
    pub overlay: Option<String>,
    /// `--timeline-child-flag-agrees` (`xesp_child_flag_agrees_chain_v1`).
    pub child_flag_agrees: bool,
    /// `--timeline-cast`: the actors as their characters.
    pub cast: bool,
    /// `--timeline-target-rotation` (`timeline_actor_target_rotation_v1`).
    pub target_rotation: bool,
    /// `--actor-missing-bone-bind` (`actor_missing_bone_bind_pose_v1`).
    pub missing_bones_at_bind: bool,
    /// The start clip flags (`fo4_scene::char_graph::Switches`).
    pub pose: fo4_scene::char_graph::Switches,
    /// `--look-at`, `--look-at-actor` (presentation).
    pub aim: tl::Aim,
    /// `--timeline-dead-lying`, `--timeline-dead-left-out`
    /// (`fo4_scene::timeline::Dead`).
    pub dead: tl::Dead,
    /// `--timeline-sit-marker` (`timeline_sit_marker_nearest_v1`).
    pub sit_marker: bool,
    /// `--actor-template-bits`, `--actor-leveled-seed`
    /// (`fo4_scene::actor::Readings`).
    pub look: fo4_scene::actor::Readings,
    /// `--actor-model-graph-idle` (`actor_model_graph_start_no_sequence_v1`).
    pub model_graph_idle: bool,
    /// `--skinned-models-own-nodes` (`model_skinned_own_nodes_every_placement_v1`).
    pub skinned_models: bool,
    /// `--memory-report FILE`: where the frame build's memory goes
    /// (`fo4_render::memory::report`; measurement only).
    pub memory_report: Option<String>,
    /// `--timeline-ride-model`, `--timeline-ride`
    /// (`fo4_scene::timeline::ride`).
    pub ride: Option<tl::ride::Options>,
}

/// The timeline read and its frame at the second.
pub struct Prepared {
    pub timeline: Timeline,
    pub frame: Frame,
}

pub fn prepare(a: &Args) -> AnyResult<Prepared> {
    let timeline = Timeline::parse(&std::fs::read_to_string(&a.file)?)?;
    let frame = timeline.at_with(
        a.at,
        tl::Readings {
            nowait: a.nowait,
            child_flag_agrees: a.child_flag_agrees,
            dead: a.dead,
            sit_marker: a.sit_marker,
        },
    );
    Ok(Prepared { timeline, frame })
}

/// What the frame needs after the load: the camera and the overlay text.
#[derive(Debug, Clone)]
pub struct Drawn {
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub overlay: String,
}

/// The frame's actors before the build: the markers, the actors left out
/// and the rules (`fo4_scene::timeline::markers`), and with `--timeline-cast`
/// the cast resolved as characters, to draw with
/// `fo4_render::draw::build_with_actors`.
pub struct Cast {
    pub markers: tl::Markers,
    pub cast: Option<tl::cast::Cast>,
}

/// [`Cast`] for a loaded scene (after the run's changes are applied).
pub fn cast(
    p: &Prepared,
    a: &Args,
    scene: &fo4_scene::Scene,
    loader: &mut fo4_scene::Loader<'_>,
    level: fo4_scene::RuleLevel,
    budget: &fo4_core::Budget,
) -> AnyResult<Cast> {
    let space = tl::scene_space(loader.plugins, scene);
    let markers = tl::markers(loader.plugins, &p.frame, &space, budget)?;
    let cast = if a.cast {
        let options = tl::cast::Options {
            target_rotation: a.target_rotation,
            missing_bones_at_bind: a.missing_bones_at_bind,
            pose: a.pose,
            look: a.look,
            model_graph_idle: a.model_graph_idle,
        };
        Some(tl::cast::resolve(
            loader, scene, &markers.0, options, level, budget,
        )?)
    } else {
        None
    };
    Ok(Cast { markers, cast })
}

/// Appends the frame's actor markers (with `--timeline-cast` only those of
/// the actors the build could not draw) to the draw list and returns the
/// camera and the overlay.
#[allow(clippy::too_many_arguments)]
pub fn finish(
    p: &Prepared,
    a: &Args,
    c: Cast,
    level: fo4_scene::RuleLevel,
    applied: &tl::Applied,
    build: &mut fo4_render::draw::Build,
    budget: &fo4_core::Budget,
) -> AnyResult<Drawn> {
    let (markers, left, mut rules) = c.markers;
    let (shown, first, tags, cast_text, drawn) = match &c.cast {
        Some(cast) => {
            let d = fo4_render::timeline::cast_markers(build, cast, level, budget)?;
            let tags: Vec<u32> = d.fallback.iter().map(|f| f.1).collect();
            let text = tl::cast::report(cast, &d.characters, &d.fallback, &d.heads, &d.bind_bones);
            let text = text
                + &tl::ride::report(cast, &d.extras)
                + &tl::ride::report_skinned(cast, &d.extras);
            rules.extend(cast.rules.iter().copied());
            if !d.bind_bones.is_empty() {
                rules.insert(fo4_scene::hypotheses::ACTOR_MISSING_BONE_BIND);
            }
            (d.markers, d.first, tags, Some(text), d.drawn)
        }
        None => {
            let at: Vec<([f64; 3], bool)> = markers
                .iter()
                .map(|m| (m.pos, matches!(m.pose, tl::Pose::Lying { .. })))
                .collect();
            let first = fo4_render::timeline::next_tag(&build.list);
            let tags =
                fo4_render::timeline::append_markers_posed(&mut build.list, &at, first, budget)?;
            let drawn = markers
                .iter()
                .map(|m| (m.reference.clone(), m.pos))
                .collect();
            (markers, first, tags, None, drawn)
        }
    };
    rules.extend(applied.rules.iter().copied());
    rules.extend(p.frame.rules.iter().copied());
    let (eye, look, camera) = tl::camera(&p.frame, a.eye_height, a.heading, &a.aim, &drawn)?;
    let mut overlay = tl::overlay(&p.timeline, &p.frame, &camera);
    overlay.push_str(&tl::report(
        &p.frame, applied, &shown, &left, &rules, first, &tags,
    ));
    if let Some(text) = cast_text {
        overlay.push_str(&text);
    }
    Ok(Drawn { eye, look, overlay })
}

/// What [`cast_scene`] built for the window's scripted play mode: the
/// cast's draws alone, converted, and one line on it.
#[cfg(feature = "script-event")]
pub struct CastScene {
    pub gpu: crate::scene::GpuScene,
    pub line: String,
}

/// The run's actors of `cell` at `second` of the run's own `timeline`, for
/// the window's scripted play mode (`--play-run ... --timeline-cast`): the
/// frame's markers in the cell's space (`fo4_scene::timeline::markers`,
/// the readings of `a`), resolved as characters
/// (`fo4_scene::timeline::cast::resolve`), posed by the start clip
/// switches at the frame's second (`cast::start_poses`), skinned and drawn
/// without the cell's placed references, a marker for each actor the
/// build left out (`fo4_render::timeline::cast_markers`), lit by the
/// cell's light under the light switches as the cell's own draws are.
/// The same code path as a headless `--timeline --timeline-cast` frame,
/// with the run's timeline as it stands instead of a file and without the
/// changes `fo4_scene::timeline::apply` makes to the cell (the live world
/// makes them in the window).
#[cfg(feature = "script-event")]
#[allow(clippy::too_many_arguments)]
pub fn cast_scene(
    sources: &crate::load::Sources,
    s: &crate::cli::Scene,
    a: &Args,
    cell: &fo4_plugin::FormKey,
    timeline: &Timeline,
    second: f64,
) -> AnyResult<CastScene> {
    let started = std::time::Instant::now();
    let limits = &sources.manifest.limits;
    let budget = crate::load::scene_budget(s.scene_memory, limits);
    let mut loader = fo4_scene::Loader::new(
        &sources.store,
        &sources.vfs,
        &sources.manifest.language,
        s.level,
        &budget,
    );
    if s.clock.any() {
        loader.set_clock_switches(s.clock, &budget);
    }
    let scope = budget.child();
    let mut scene = loader.cell(cell, &scope)?;
    scope.commit(&budget)?;
    let frame = timeline.at_with(
        second,
        tl::Readings {
            nowait: a.nowait,
            child_flag_agrees: a.child_flag_agrees,
            dead: a.dead,
            sit_marker: a.sit_marker,
        },
    );
    let prepared = Prepared {
        timeline: Timeline::default(),
        frame,
    };
    let Cast { markers, cast } = self::cast(&prepared, a, &scene, &mut loader, s.level, &budget)?;
    let mut cast = cast.ok_or("PlayCast: no cast")?;
    let poses = if a.pose.any() {
        Some(tl::cast::start_poses(
            &mut cast,
            &loader,
            &sources.vfs,
            &a.pose,
            prepared.frame.second,
            s.level,
            limits,
            &budget,
        )?)
    } else {
        None
    };
    // The cell's light, before its references leave the build.
    let light = fo4_scene::light::frame(
        &sources.store,
        &scene,
        s.light,
        s.light_assumptions,
        &budget,
    )?;
    let options = fo4_render::draw::BuildOptions {
        time: s.time,
        clock: s.clock,
        vertex_normals: s.light.vertex_normals,
        ..crate::load::build_options(s.textures, s.level)
    };
    let placed = std::mem::take(&mut scene.instances);
    let built = match poses {
        Some(p) => fo4_render::draw::build_with_posed_actors(
            &scene,
            &cast.actors,
            p,
            &loader,
            limits,
            options,
            &budget,
        )
        .map(|x| x.0),
        None => fo4_render::draw::build_with_actors(
            &scene,
            &cast.actors,
            &loader,
            limits,
            options,
            &budget,
        ),
    };
    scene.instances = placed;
    let mut build = built?;
    let lit_until = build.list.draws.len();
    let d = fo4_render::timeline::cast_markers(&mut build, &cast, s.level, &budget)?;
    let lit = light.as_ref().map(|_| {
        let effects: std::collections::BTreeSet<usize> =
            build.effect_draws.iter().copied().collect();
        (0..build.list.draws.len())
            .map(|i| i < lit_until && !effects.contains(&i))
            .collect::<Vec<bool>>()
    });
    let render = fo4_render::RenderOptions {
        backface_cull: s.backface_cull,
        ..fo4_render::RenderOptions::REFERENCE
    };
    let gpu =
        crate::scene::convert_lit(&mut build.list, &render, light.as_ref().zip(lit.as_deref()))?;
    let line = format!(
        "{} actors of the run in the cell's space at {second:.3} s ({} left out), {} drawn as characters ({} posed by their start clip), {} as markers; {} GPU instances; built in {:.2} s",
        cast.actors.actors.len(),
        markers.1.len(),
        d.characters.len(),
        cast.posed,
        d.fallback.len(),
        gpu.instances.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(CastScene { gpu, line })
}

#[cfg(all(test, feature = "script-event"))]
mod tests {
    use fo4_render::fixture::{actor_room, Placed, ACTOR_BASE};
    use fo4_scene::timeline::{Event, Spot, Timeline, What};

    /// The window's scripted play cast (`--play-run ... --timeline-cast`):
    /// the run's actors of a cell at a second of its timeline, drawn alone
    /// (the cell's placed references left out), a marker for the actor
    /// whose base is missing; no actor the timeline does not place.
    #[test]
    fn the_play_cast_draws_the_runs_actors_of_the_cell_alone() {
        let temp = fo4_core::TestDir::new("view-play-cast");
        let root = temp.join("Data");
        let m = actor_room(
            &root,
            &[
                Placed {
                    id: 0x900,
                    base: ACTOR_BASE,
                    data: [0., 0., 0., 0., 0., std::f32::consts::FRAC_PI_2],
                    scale: Some(30.),
                },
                Placed {
                    id: 0x901,
                    base: 0x8ff,
                    data: [150., 100., 0., 0., 0., 0.],
                    scale: None,
                },
            ],
            &[],
        );
        let b = fo4_core::Budget::new(256 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let sources = crate::load::Sources {
            manifest: m,
            store,
            vfs,
        };
        let line = "m d C --under-hypotheses --walk --eye 0,0,0 --look 1,0,0 --play-run s --play-out o --play-input i --timeline-cast";
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        let crate::cli::Command::Window { scene, .. } = crate::cli::parse(&args).unwrap() else {
            panic!()
        };
        let a = scene.play_cast.clone().unwrap();
        let key = |id: u32| fo4_plugin::FormKey {
            plugin: "base.esm".into(),
            object_id: id,
        };
        let room = key(0x800);
        let actor = |t: f64, id: u32, pos: [f64; 3]| Event {
            t,
            what: What::Actor {
                reference: key(id),
                at: Spot::At {
                    space: room.clone(),
                    pos,
                },
                source: "test".into(),
            },
        };
        let timeline = Timeline {
            events: vec![
                actor(0., 0x900, [0., 0., 0.]),
                actor(0., 0x901, [150., 100., 0.]),
                actor(1., 0x900, [0., 100., 0.]),
            ],
            ..Timeline::default()
        };
        let c = super::cast_scene(&sources, &scene, &a, &room, &timeline, 5.).unwrap();
        assert!(
            c.line.starts_with("2 actors of the run in the cell's space at 5.000 s (0 left out), 1 drawn as characters (0 posed by their start clip), 1 as markers"),
            "{}",
            c.line
        );
        assert!(!c.gpu.instances.is_empty());
        // No actor placed by the timeline: nothing drawn.
        let none =
            super::cast_scene(&sources, &scene, &a, &room, &Timeline::default(), 5.).unwrap();
        assert!(none.gpu.instances.is_empty(), "{}", none.line);
    }
}
