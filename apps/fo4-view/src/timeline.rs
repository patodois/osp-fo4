//! `--timeline FILE --at SECONDS --eye-height UNITS` (headless): one frame
//! of a quest run's timeline (`fo4_scene::timeline`, `spec/timeline.md`),
//! drawn as the CPU render's `--timeline` draws it: the player's cell
//! (CELL `-`), the references the run disabled left out and those it
//! enabled or moved put in or moved (pending rules, `--under-hypotheses`),
//! the actors as placeholder markers (`fo4_render::timeline`), the camera on
//! the player's track and an overlay text beside the frame. The camera,
//! the markers and the overlay are presentation.
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
}

/// The timeline read and its frame at the second.
pub struct Prepared {
    pub timeline: Timeline,
    pub frame: Frame,
}

pub fn prepare(a: &Args) -> AnyResult<Prepared> {
    let timeline = Timeline::parse(&std::fs::read_to_string(&a.file)?)?;
    let frame = timeline.at_with(a.at, tl::Readings { nowait: a.nowait });
    Ok(Prepared { timeline, frame })
}

/// What the frame needs after the load: the camera and the overlay text.
#[derive(Debug, Clone)]
pub struct Drawn {
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub overlay: String,
}

/// Appends the frame's actor markers to the draw list and returns the
/// camera and the overlay.
pub fn finish(
    p: &Prepared,
    a: &Args,
    store: &fo4_plugin::typed::Store,
    scene: &fo4_scene::Scene,
    applied: &tl::Applied,
    list: &mut fo4_render::DrawList,
    budget: &fo4_core::Budget,
) -> AnyResult<Drawn> {
    let space = tl::scene_space(store, scene);
    let (markers, left, mut rules) = tl::markers(store, &p.frame, &space, budget)?;
    let at: Vec<[f64; 3]> = markers.iter().map(|m| m.pos).collect();
    let first = fo4_render::timeline::next_tag(list);
    let tags = fo4_render::timeline::append_markers(list, &at, first, budget)?;
    rules.extend(applied.rules.iter().copied());
    rules.extend(p.frame.rules.iter().copied());
    let (eye, look, from) = tl::eye_look(&p.frame, a.eye_height, a.heading)?;
    let camera = tl::camera_text(eye, look, a.eye_height, &from);
    let mut overlay = tl::overlay(&p.timeline, &p.frame, &camera);
    overlay.push_str(&tl::report(
        &p.frame, applied, &markers, &left, &rules, first, &tags,
    ));
    Ok(Drawn { eye, look, overlay })
}
