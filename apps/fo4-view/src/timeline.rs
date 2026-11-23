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
