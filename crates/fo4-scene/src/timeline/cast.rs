//! The actors of a run timeline's frame as their characters
//! (`--timeline-cast`, `spec/timeline.md`): each actor a frame marks
//! ([`super::markers`]) resolved as a placed actor is ([`crate::actor`]:
//! skeleton, addon models, head parts, skinning in the skeleton's rest
//! pose), standing where the run puts it at the second.
//!
//! What comes from where:
//!
//! - presence: the frame's ([`super::presence_with_rules`]), so the
//!   actor's own start state through its enable chain is replaced by it
//!   (the chain's open parts no longer block the actor);
//! - position: the run's;
//! - facing: the rotation the actor's placement stores where the run's
//!   place is the stored one; elsewhere the stored rotation kept under
//!   `timeline_actor_stored_rotation_v1`, or with
//!   [`Options::target_rotation`] the rotation of the one placed reference
//!   standing exactly at the run's place (the move's target) under
//!   `timeline_actor_target_rotation_v1`;
//! - pose: the skeleton's rest pose (presentation: no animation is read
//!   from the run), or behind the character graph switches
//!   ([`Options::pose`], [`start_poses`]) the clip the actor's behaviour
//!   graph reaches from its root (`crate::char_graph`), at its start time
//!   or at the frame's second.
//!
//! An actor that cannot be resolved is drawn as its marker by the caller,
//! with the reason ([`Cast::why_not`]).
use super::{key_text, norm, Marker};
use crate::{actor, char_graph, hypotheses, Loader, RuleLevel, Scene};
use fo4_core::{Budget, Error, Limits, SourceSpan};
use fo4_plugin::FormKey;
use std::collections::{BTreeMap, BTreeSet};

/// The cast's switches beyond `--timeline-cast` itself, each off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// `timeline_actor_target_rotation_v1` (`--timeline-target-rotation`):
    /// an actor the run moved faces as the one placed reference standing
    /// exactly at its new place does.
    pub target_rotation: bool,
    /// `actor_missing_bone_bind_pose_v1` (`--actor-missing-bone-bind`): a
    /// bone a skinned shape names that the actor's skeleton lacks is skinned
    /// at its bind transform instead of failing the actor.
    pub missing_bones_at_bind: bool,
    /// `actor_missing_bone_follows_nearest_v1` (`--actor-missing-bone-nearest`,
    /// with `missing_bones_at_bind`): such a bone skinned as the nearest
    /// bone of its skin the skeleton holds, in place of the identity.
    pub missing_bones_nearest: bool,
    /// The character graph's readings (`crate::char_graph::Switches`):
    /// with any on, [`start_poses`] poses the actors by their start clip.
    pub pose: char_graph::Switches,
    /// The look's readings of a templated or leveled actor
    /// (`crate::actor::Readings`: `--actor-template-bits`,
    /// `--actor-leveled-seed`).
    pub look: actor::Readings,
    /// `actor_model_graph_start_no_sequence_v1` (`--actor-model-graph-idle`):
    /// a part model of the actor (skin or outfit piece) whose own behaviour
    /// graph's root start state plays no sequence ([`graph_start_idle`]) has
    /// its graph extra data and its manager-run effect-shader controllers
    /// settled: the model is drawn as stored.
    pub model_graph_idle: bool,
    /// `actor_effect_controller_stored_v1` (`--actor-effect-stored`): an
    /// effect-shader float controller a manager runs in a part model of the
    /// actor (the `effect-idle` blocker left after
    /// [`Options::model_graph_idle`]) is not run: its target field is
    /// drawn as stored ([`effect_stored`]).
    pub effect_stored: bool,
}

/// The frame's actors resolved as characters, in the order of the markers.
#[derive(Debug, Default)]
pub struct Cast {
    /// One per marker, same order; to draw with
    /// `fo4_render::draw::build_with_actors`.
    pub actors: actor::Actors,
    /// The markers the actors stand for (place and source).
    pub markers: Vec<Marker>,
    /// How each actor's facing was settled, one line per actor.
    pub facing: Vec<String>,
    /// Rules the cast relied on (beyond each actor's own state).
    pub rules: BTreeSet<&'static str>,
    /// With [`Options::effect_stored`]: the actors whose effect-shader float
    /// controllers it settled ([`effect_stored`]), and how many controllers.
    pub effect_stored: (usize, usize),
    /// With [`Options::pose`]: how each actor is posed, one text per actor
    /// ([`start_poses`]); empty without it.
    pub poses: Vec<String>,
    /// Actors [`start_poses`] posed by their start clip.
    pub posed: usize,
    /// With `--timeline-ride-model`: the overlay's text on the ride's model
    /// (`super::ride::add`), which then follows the markers' actors in
    /// [`Cast::actors`].
    pub ride: Option<String>,
    /// The cast index of the ride's model when [`super::ride::add`] added it.
    pub ride_index: Option<usize>,
    /// With `--skinned-models-own-nodes`: the first cast index of the
    /// placed references added by `super::ride::add_skinned` (they follow
    /// the markers' actors and the ride), how many were added, and the
    /// overlay's text on them.
    pub skinned: Option<(usize, usize, String)>,
}

impl Cast {
    /// Why actor `k` of the cast cannot be drawn as a character before the
    /// draw build is tried (no plan, or not ready at `level`); `None` when
    /// the build may draw it.
    pub fn why_not(&self, k: usize, level: RuleLevel) -> Option<String> {
        let a = self.actors.actors.get(k)?;
        if a.plan.is_none() {
            return Some(if a.notes.is_empty() {
                "not resolved".into()
            } else {
                a.notes.join("; ")
            });
        }
        if !a.state.ready_at(level) {
            let open: Vec<&Error> = a
                .blockers
                .iter()
                .filter(|e| hypotheses::covering(e).is_none())
                .collect();
            return Some(format!("{:?}: {}", a.state, summary(&open)));
        }
        None
    }
}

/// The distinct codes of `errors`, each with the first part of its first
/// `found` text, for one overlay line (presentation). An animation blocker
/// keeps its rest tag (`rest=TAG`, `crate::rest`), which names the rule
/// family that could cover it.
pub fn summary(errors: &[&Error]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for e in errors {
        if seen.iter().any(|s| s.starts_with(&format!("{} ", e.code))) {
            continue;
        }
        let mut found: String = e
            .found
            .split(';')
            .next()
            .unwrap_or("")
            .chars()
            .take(48)
            .collect();
        if e.code == "UnsupportedAnimationRuntime" {
            if let Some(rest) = e.found.split(';').find(|p| p.starts_with("rest=")) {
                found.push(' ');
                found.extend(rest.chars().take(40));
            }
        }
        seen.push(format!("{} {found}", e.code));
    }
    seen.join("; ")
}

/// The record's source span, for the actor's error locations.
fn span(store: &fo4_plugin::typed::Store, k: &FormKey) -> SourceSpan {
    match store.overrides().winner(k) {
        Some(w) => SourceSpan::new(
            &store.order().headers()[w.plugin].source.file,
            w.offset,
            w.length,
        ),
        None => SourceSpan::new("timeline", 0, 0),
    }
}

/// The rotation of the one placed reference of the loaded scene (other
/// than an actor) whose stored position is exactly `pos`, with its key;
/// else why none is taken.
fn target_rotation(
    loader: &Loader<'_>,
    scene: &Scene,
    pos: [f64; 3],
    b: &Budget,
) -> Result<Result<(FormKey, [f32; 3]), String>, Error> {
    let at = |t: &[f32; 6]| (0..3).all(|i| f64::from(t[i]) == pos[i]);
    let mut found: Vec<(FormKey, [f32; 3])> = Vec::new();
    for i in &scene.instances {
        if let Some(t) = i.reference.data.transform.as_ref().filter(|t| at(t)) {
            found.push((i.reference.key.clone(), [t[3], t[4], t[5]]));
        }
    }
    let others = scene
        .disabled
        .iter()
        .chain(scene.exclusions.iter().map(|e| &e.child))
        .filter(|c| c.signature == *b"REFR");
    for c in others {
        let work = b.child();
        let Some(r) = loader
            .plugins
            .get::<fo4_plugin::typed::Refr>(&c.key, &work)?
        else {
            continue;
        };
        if let Some(t) = r.data.transform.as_ref().filter(|t| at(t)) {
            found.push((c.key.clone(), [t[3], t[4], t[5]]));
        }
    }
    Ok(match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err("no placed reference of the loaded cells stands exactly at the place".into()),
        n => Err(format!(
            "{n} placed references of the loaded cells stand exactly at the place"
        )),
    })
}

/// A path lower case with forward slashes, for comparing a blocker's file
/// with a part's model.
fn path_text(p: &[u8]) -> String {
    String::from_utf8_lossy(p)
        .to_ascii_lowercase()
        .replace('\\', "/")
}

/// Under [`Options::model_graph_idle`]: for each part model of `a` (skin or
/// outfit piece) with a behaviour graph blocker, the root start state of
/// the graph the model names (`crate::anim_graph::model_graph`); where it
/// plays no sequence, the model's `behavior-graph` and `effect-idle`
/// blockers take the tag `graph-start-idle`
/// (`actor_model_graph_start_no_sequence_v1`) and the actor's state is
/// classified again. A graph that cannot be read leaves the blockers.
fn graph_start_idle(loader: &Loader<'_>, a: &mut actor::Actor, b: &Budget) -> Result<(), Error> {
    use crate::rest;
    let Some(plan) = &a.plan else {
        return Ok(());
    };
    let tagged = |e: &Error, tag: &str| e.found.ends_with(&format!(";rest={tag}"));
    let mut idle: Vec<String> = Vec::new();
    for p in &plan.parts {
        let file = path_text(&loader.assets.nodes[p.asset].path);
        let has_graph = a.blockers.iter().any(|e| {
            tagged(e, rest::BEHAVIOR_GRAPH) && path_text(e.source.file.as_bytes()) == file
        });
        if !has_graph || idle.contains(&file) {
            continue;
        }
        let model = file.strip_prefix("meshes/").unwrap_or(&file);
        let Ok(g) = crate::anim_graph::model_graph(loader.vfs(), model, &loader.plugins.limits, b)
        else {
            continue;
        };
        if g.machine
            .state(g.machine.start)
            .is_some_and(|s| s.sequence.is_none())
        {
            b.reserve(&mut idle, 1, "timeline cast", 0)?;
            idle.push(file);
        }
    }
    if retag_graph_start_idle(&mut a.blockers, &idle) {
        a.state = hypotheses::classify(&a.blockers, false, b)?;
    }
    Ok(())
}

/// Under [`Options::effect_stored`]: the `effect-idle` blockers of an
/// effect-shader float controller (`BSEffectShaderPropertyFloatController`),
/// and in the same model file those of the float interpolators and data
/// that feed such a controller (`NiBlendFloatInterpolator`,
/// `NiFloatInterpolator`, `NiFloatData`) and of its behaviour graph extra
/// data (`behavior-graph`: the graph is not run), retagged `actor-effect-stored`
/// (`actor_effect_controller_stored_v1`: the controller is not run, its
/// target field drawn as stored); how many controllers.
fn effect_stored(blockers: &mut [Error]) -> usize {
    use crate::rest;
    const CONTROLLER: &str = "BSEffectShaderPropertyFloatController;";
    const FEEDS: [&str; 3] = [
        "NiBlendFloatInterpolator;",
        "NiFloatInterpolator;",
        "NiFloatData;",
    ];
    let idle = |e: &Error| {
        e.found
            .rsplit_once(";rest=")
            .filter(|(_, t)| *t == rest::EFFECT_IDLE)
            .map(|(h, _)| h.to_string())
    };
    let files: BTreeSet<String> = blockers
        .iter()
        .filter(|e| idle(e).is_some_and(|h| h.starts_with(CONTROLLER)))
        .map(|e| path_text(e.source.file.as_bytes()))
        .collect();
    let mut n = 0;
    for e in blockers {
        // The model's own behaviour graph, which would run the sequences
        // driving the controller: not run either.
        if let Some((head, rest::BEHAVIOR_GRAPH)) = e.found.rsplit_once(";rest=") {
            if files.contains(&path_text(e.source.file.as_bytes())) {
                e.found = format!("{head};rest={}", rest::ACTOR_EFFECT_STORED);
            }
            continue;
        }
        let Some(head) = idle(e) else {
            continue;
        };
        let controller = head.starts_with(CONTROLLER);
        if controller
            || (FEEDS.iter().any(|f| head.starts_with(f))
                && files.contains(&path_text(e.source.file.as_bytes())))
        {
            e.found = format!("{head};rest={}", rest::ACTOR_EFFECT_STORED);
            n += usize::from(controller);
        }
    }
    n
}

/// The `behavior-graph` and `effect-idle` blockers of the files `idle`
/// (lower case, forward slashes) retagged `graph-start-idle`; whether any was.
fn retag_graph_start_idle(blockers: &mut [Error], idle: &[String]) -> bool {
    use crate::rest;
    let mut any = false;
    for e in blockers {
        let tag = e
            .found
            .rsplit_once(";rest=")
            .map(|(h, t)| (h.to_string(), t));
        let Some((head, t)) = tag else { continue };
        if (t == rest::BEHAVIOR_GRAPH || t == rest::EFFECT_IDLE)
            && idle.contains(&path_text(e.source.file.as_bytes()))
        {
            e.found = format!("{head};rest={}", rest::GRAPH_START_IDLE);
            any = true;
        }
    }
    any
}

/// Resolves each marker's actor as a character standing at the marker's
/// place (see the module). Needs `RuleLevel::Hypotheses`: every rule of
/// the cast is pending.
pub fn resolve(
    loader: &mut Loader<'_>,
    scene: &Scene,
    markers: &[Marker],
    options: Options,
    level: RuleLevel,
    b: &Budget,
) -> Result<Cast, Error> {
    resolve_heads(loader, scene, markers, options, level, None, b)
}

/// [`resolve`] with the caller's answer for head parts with conditions
/// (`actor::HeadConditions`, `npc_head_part_conditions_run_stages_v1`);
/// `None` is [`resolve`].
pub fn resolve_heads(
    loader: &mut Loader<'_>,
    scene: &Scene,
    markers: &[Marker],
    options: Options,
    level: RuleLevel,
    mut heads: Option<actor::HeadConditions<'_>>,
    b: &Budget,
) -> Result<Cast, Error> {
    if level != RuleLevel::Hypotheses {
        return Err(Error::new(
            "TimelineRuleLevel",
            "timeline",
            0,
            "--under-hypotheses",
            format!(
                "the cast relies on the pending rules {} and the actor rules",
                hypotheses::TIMELINE_ACTOR_ROTATION
            ),
        ));
    }
    let mut out = Cast::default();
    for m in markers {
        let k = norm(&m.reference);
        let src = span(loader.plugins, &k);
        let answer: Option<actor::HeadConditions<'_>> = match heads.as_mut() {
            Some(f) => Some(&mut **f),
            None => None,
        };
        let mut a = actor::resolve_reference_heads(loader, &k, &src, options.look, answer, b)?;
        if a.notes
            .iter()
            .any(|n| n.contains("conditions hold") || n.contains("conditions do not hold"))
        {
            out.rules.insert(hypotheses::ACTOR_HEAD_CONDITIONS);
        }
        if a.look.is_some() && a.plan.is_some() {
            if a.look
                .as_deref()
                .is_some_and(|l| l.contains(hypotheses::ACTOR_LEVELED_PICK))
            {
                out.rules.insert(hypotheses::ACTOR_LEVELED_PICK);
            }
            if a.look
                .as_deref()
                .is_some_and(|l| l.contains(hypotheses::ACTOR_TEMPLATE_BITS))
            {
                out.rules.insert(hypotheses::ACTOR_TEMPLATE_BITS);
            }
        }
        a.missing_bones_at_bind = options.missing_bones_at_bind;
        a.missing_bones_nearest = options.missing_bones_at_bind && options.missing_bones_nearest;
        if options.model_graph_idle {
            graph_start_idle(loader, &mut a, b)?;
        }
        if options.effect_stored {
            let n = effect_stored(&mut a.blockers);
            if n > 0 {
                a.state = hypotheses::classify(&a.blockers, false, b)?;
                out.effect_stored.0 += 1;
                out.effect_stored.1 += n;
            }
        }
        let mut facing = "not placed (no stored placement)".to_string();
        if let Some(stored) = a.transform {
            let here = (0..3).all(|i| f64::from(stored[i]) == m.pos[i]);
            let mut rotation = [stored[3], stored[4], stored[5]];
            facing = if here {
                "its placement's stored rotation (it stands at its stored place)".into()
            } else if options.target_rotation {
                match target_rotation(loader, scene, m.pos, b)? {
                    Ok((t, r)) => {
                        rotation = r;
                        out.rules.insert(hypotheses::TIMELINE_ACTOR_TARGET_ROTATION);
                        format!(
                            "the rotation of {}, the one placed reference at its place ({})",
                            key_text(&t),
                            hypotheses::TIMELINE_ACTOR_TARGET_ROTATION
                        )
                    }
                    Err(why) => {
                        out.rules.insert(hypotheses::TIMELINE_ACTOR_ROTATION);
                        format!(
                            "its placement's stored rotation, kept ({}): {why}",
                            hypotheses::TIMELINE_ACTOR_ROTATION
                        )
                    }
                }
            } else {
                out.rules.insert(hypotheses::TIMELINE_ACTOR_ROTATION);
                format!(
                    "its placement's stored rotation, kept at the run's place ({})",
                    hypotheses::TIMELINE_ACTOR_ROTATION
                )
            };
            match &m.pose {
                super::Pose::Standing => {}
                super::Pose::Lying { .. } => {
                    rotation = super::lying_rotation(rotation);
                    facing = format!(
                        "{facing}, then laid down a quarter turn ({}, a placeholder)",
                        hypotheses::TIMELINE_DEAD_LYING
                    );
                }
                super::Pose::Seated { heading, seat, .. } => {
                    rotation = [0., 0., *heading];
                    facing = format!(
                        "the heading of its furniture's marker {seat} ({})",
                        hypotheses::TIMELINE_SIT_MARKER
                    );
                }
            }
            a.transform = Some([
                m.pos[0] as f32,
                m.pos[1] as f32,
                m.pos[2] as f32,
                rotation[0],
                rotation[1],
                rotation[2],
            ]);
        }
        // The frame decided the actor is present: its start state through
        // the chain, and the chain's open parts, no longer apply.
        if let Some(p) = &mut a.enable {
            p.enabled = Some(true);
        }
        let before = a.blockers.len();
        a.blockers
            .retain(|e| !(e.code == "RuntimeEnableParent" && hypotheses::covering(e).is_none()));
        if a.blockers.len() != before && a.plan.is_some() {
            a.state = hypotheses::classify(&a.blockers, false, b)?;
        }
        b.reserve(&mut out.actors.actors, 1, "timeline cast", 0)?;
        out.actors.actors.push(a);
        b.reserve(&mut out.markers, 1, "timeline cast", 0)?;
        out.markers.push(m.clone());
        b.reserve(&mut out.facing, 1, "timeline cast", 0)?;
        out.facing.push(facing);
    }
    Ok(out)
}

/// The poses of the cast's actors under the character graph switches
/// (`crate::char_graph`, [`Options::pose`]): for each actor drawable at
/// `level`, the clip its race's graph reaches from its root, read and
/// sampled as `crate::actor::clip` does (`actor_clip_skeleton_beside_nif_v1`,
/// `actor_clip_pose_by_bone_name_v1`) at the clip generator's start time,
/// or under `char_graph_clip_mode_loop_v1` at `second`. Each posed actor
/// gets the key of its pose ([`actor::Actor::pose`]; one pose per skeleton,
/// clip and time, keys counted down from `usize::MAX`); the returned map is
/// for `fo4_render::draw::build_with_posed_actors`. An actor whose walk or
/// clip is refused keeps the rest pose, with the reason in
/// [`Cast::poses`]. With every switch off nothing changes and the map is
/// empty.
#[allow(clippy::too_many_arguments)]
pub fn start_poses(
    cast: &mut Cast,
    loader: &Loader<'_>,
    vfs: &fo4_vfs::Vfs,
    switches: &char_graph::Switches,
    second: f64,
    level: RuleLevel,
    limits: &Limits,
    b: &Budget,
) -> Result<BTreeMap<usize, actor::pose::Skeleton>, Error> {
    use actor::clip::{self, Clip, HkSkeleton, Rig};
    let mut out = BTreeMap::new();
    if !switches.any() {
        return Ok(out);
    }
    let n = cast.actors.actors.len();
    cast.poses = vec![String::new(); n];
    // Per skeleton asset: the Havok skeleton beside it and the rig, or why not.
    let mut rigs: BTreeMap<usize, Result<(String, HkSkeleton, Rig), String>> = BTreeMap::new();
    let mut clips: BTreeMap<String, Result<Clip, String>> = BTreeMap::new();
    let mut groups: Vec<(usize, String, u64)> = Vec::new();
    for k in 0..n {
        let a = &cast.actors.actors[k];
        let Some(plan) = &a.plan else { continue };
        if !a.state.ready_at(level) {
            continue;
        }
        let keywords = match (&a.keyword_base, &a.base) {
            (Some((npc, by_bits)), _) => {
                char_graph::npc_keywords(loader.plugins, npc, &plan.race, *by_bits, b)?
            }
            (None, Some(base)) => char_graph::actor_keywords(loader.plugins, base, &plan.race, b)?,
            (None, None) => Err("no base".to_string()),
        };
        let input = char_graph::Actor {
            race: &plan.race,
            section: plan.section,
            keywords: keywords.as_ref().map_err(|s| s.as_str()),
        };
        // An actor added after the markers' (a ride's model, a skinned
        // placement) has no marker to say how it stands: not posed.
        let Some(marker) = cast.markers.get(k) else {
            cast.poses[k] =
                "rest pose: added after the cast's markers, no marker to pose it from".into();
            continue;
        };
        // A seated actor under --actor-sit-clip: its furniture's clip.
        let seated = match &marker.pose {
            super::Pose::Seated { base, .. } if switches.sit => Some(base.clone()),
            _ => None,
        };
        let walked = match &seated {
            Some(f) => char_graph::furniture_clip(vfs, loader.plugins, &input, f, switches, b)?,
            None => char_graph::start_clip(vfs, loader.plugins, &input, switches, b)?,
        };
        let reached = match walked {
            Ok(x) => x,
            Err(r) => {
                cast.poses[k] = format!(
                    "rest pose: its start clip refused, {}: {}",
                    r.code, r.detail
                );
                continue;
            }
        };
        let asset = plan.skeleton_asset;
        let rig = rigs.entry(asset).or_insert_with(|| {
            let nif = &loader.assets.nodes[asset].path;
            let nif_name = String::from_utf8_lossy(nif).into_owned();
            let hkx = clip::skeleton_hkx_path(nif)
                .map(|p| String::from_utf8_lossy(&p).into_owned())
                .ok_or_else(|| format!("{nif_name} is not a .nif path"))?;
            let hk = vfs
                .read(&hkx, &b.child())
                .and_then(|bytes| HkSkeleton::read(&bytes, &hkx, &b.child()))
                .map_err(|e| format!("{hkx}: {}", e.code))?;
            let rig = loader
                .assets
                .with_decoded(asset, &b.child(), |d| match d {
                    crate::DecodedAsset::Model(m) => Rig::new(&m, &nif_name, &hk, limits, b),
                    _ => Err(Error::new(
                        "ActorClipSkeletonKind",
                        &nif_name,
                        0,
                        "model",
                        "other asset",
                    )),
                })
                .map_err(|e| format!("{nif_name}: {}", e.code))?;
            Ok((hkx, hk, rig))
        });
        let (hkx, hk, rig) = match rig {
            Ok(x) => (&x.0, &x.1, &x.2),
            Err(why) => {
                cast.poses[k] = format!("rest pose: its skeleton's Havok file, {why}");
                continue;
            }
        };
        let read = clips.entry(reached.clip.clone()).or_insert_with(|| {
            vfs.read(&reached.clip, &b.child())
                .and_then(|bytes| Clip::read(&bytes, &reached.clip, &b.child()))
                .map_err(|e| e.code.to_string())
        });
        let c = match read {
            Ok(c) if c.fits(hk) => c,
            Ok(_) => {
                cast.poses[k] = format!(
                    "rest pose: its start clip {} does not fit {hkx}",
                    reached.clip
                );
                continue;
            }
            Err(code) => {
                cast.poses[k] =
                    format!("rest pose: its start clip {} refused, {code}", reached.clip);
                continue;
            }
        };
        let t =
            match char_graph::clip_time(&reached.generator, c.duration(), second, switches.cycle) {
                Ok(t) => t,
                Err(why) => {
                    cast.poses[k] = format!("rest pose: the second's clip time refused, {why}");
                    continue;
                }
            };
        let group = (asset, reached.clip.clone(), t.to_bits());
        let at = match groups.iter().position(|g| *g == group) {
            Some(i) => i,
            None => {
                let locals = c.locals(hk, t);
                let worlds = clip::worlds(hk, &locals, hkx)?;
                b.reserve(&mut groups, 1, "start poses", 0)?;
                groups.push(group);
                out.insert(usize::MAX - (groups.len() - 1), rig.pose(&worlds));
                groups.len() - 1
            }
        };
        cast.actors.actors[k].pose = Some(usize::MAX - at);
        cast.posed += 1;
        cast.rules.extend(reached.rules.iter().copied());
        cast.rules.insert(hypotheses::ACTOR_CLIP_SKELETON);
        cast.rules.insert(hypotheses::ACTOR_CLIP_POSE);
        if switches.cycle {
            cast.rules.insert(hypotheses::CHAR_GRAPH_CLIP_LOOP);
        }
        cast.poses[k] =
            format!(
            "posed by {} {} at {t:.3} s of {:.3} s (mode {}{}; {} generators from the root{}{})",
            if seated.is_some() {
                "its furniture's clip"
            } else {
                "its start clip"
            },
            reached.clip,
            c.duration(),
            reached.generator.mode,
            if switches.cycle { ", the frame's second" } else { ", its start time" },
            reached.steps.len(),
            match &reached.subgraph {
                Some(e) => format!(", subgraph entry {}", e.index),
                None => String::new(),
            },
            if reached.ties > 0 {
                format!(", {} blenders at a tie took their first child", reached.ties)
            } else {
                String::new()
            }
        );
    }
    Ok(out)
}

/// The overlay's lines on the cast (presentation): each actor drawn as its
/// character with its tag and facing (and whether a bone its skeleton
/// lacks was skinned at its bind transform, `bind_bones`), each drawn as
/// its marker with the reason, each head left out.
pub fn report(
    cast: &Cast,
    characters: &[(usize, u32)],
    fallback: &[(usize, u32, String)],
    heads: &[(usize, String)],
    bind_bones: &[usize],
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let place = |k: usize| {
        let m = &cast.markers[k];
        format!(
            "{}\t{:.1} {:.1} {:.1}\t{}",
            key_text(&m.reference),
            m.pos[0],
            m.pos[1],
            m.pos[2],
            m.source
        )
    };
    if cast.poses.is_empty() {
        let _ = writeln!(
            s,
            "cast\t{} actors: {} drawn as characters in the skeleton's rest pose (presentation: no animation), {} as markers",
            cast.markers.len(),
            characters.len(),
            fallback.len()
        );
    } else {
        let _ = writeln!(
            s,
            "cast\t{} actors: {} drawn as characters, {} of the actors posed by the clip their behaviour graph reaches from its root, the others in the skeleton's rest pose, {} as markers",
            cast.markers.len(),
            characters.len(),
            cast.posed,
            fallback.len()
        );
    }
    for (k, tag) in characters {
        let bind = if bind_bones.contains(k) {
            format!(
                "; a bone its skeleton lacks skinned at its bind transform ({})",
                hypotheses::ACTOR_MISSING_BONE_BIND
            )
        } else {
            String::new()
        };
        let pose = match cast.poses.get(*k) {
            Some(p) if !p.is_empty() => format!("; {p}"),
            _ => String::new(),
        };
        let look = match &cast.actors.actors[*k].look {
            Some(l) => format!("; look: {l}"),
            None => String::new(),
        };
        let _ = writeln!(
            s,
            "character\ttag {tag}\t{}\tfacing {}{bind}{pose}{look}",
            place(*k),
            cast.facing[*k]
        );
    }
    for (k, tag, why) in fallback {
        let _ = writeln!(s, "marker instead\ttag {tag}\t{}\t{why}", place(*k));
    }
    for (k, why) in heads {
        let _ = writeln!(
            s,
            "head left out\t{}\t{why}",
            key_text(&cast.markers[*k].reference)
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_effect_float_controller_is_drawn_as_stored_with_its_switch() {
        // `actor_effect_controller_stored_v1`: only the manager-run
        // effect-shader float controller's `effect-idle` blocker.
        let e = |found: &str| {
            Error::new(
                "UnsupportedAnimationRuntime",
                "Meshes\\A.nif",
                0,
                "x",
                found,
            )
        };
        let mut blockers = vec![
            e("BSEffectShaderPropertyFloatController;target_in_visual=Some(true);rest=effect-idle"),
            e("BSEffectShaderPropertyColorController;target_in_visual=Some(true);rest=effect-idle"),
            e("BSEffectShaderPropertyFloatController;target_in_visual=Some(true);rest=behavior-graph"),
            e("NiTransformController;target_in_visual=Some(true);rest=transform-varies"),
            e("NiBlendFloatInterpolator;target_in_visual=None;rest=effect-idle"),
        ];
        assert_eq!(effect_stored(&mut blockers), 1);
        assert_eq!(
            hypotheses::covering(&blockers[4]),
            Some(hypotheses::ACTOR_EFFECT_STORED),
            "its interpolator in the same file"
        );
        let mut graph = vec![
            e("BSEffectShaderPropertyFloatController;target_in_visual=Some(true);rest=effect-idle"),
            e("BSBehaviorGraphExtraData;target_in_visual=None;rest=behavior-graph"),
        ];
        assert_eq!(effect_stored(&mut graph), 1);
        assert_eq!(
            hypotheses::covering(&graph[1]),
            Some(hypotheses::ACTOR_EFFECT_STORED)
        );
        let mut alone = vec![e(
            "NiBlendFloatInterpolator;target_in_visual=None;rest=effect-idle",
        )];
        assert_eq!(effect_stored(&mut alone), 0);
        assert_ne!(
            hypotheses::covering(&alone[0]),
            Some(hypotheses::ACTOR_EFFECT_STORED)
        );
        let covered: Vec<Option<&str>> = blockers.iter().map(hypotheses::covering).collect();
        assert_eq!(covered[0], Some(hypotheses::ACTOR_EFFECT_STORED));
        assert_ne!(covered[1], Some(hypotheses::ACTOR_EFFECT_STORED));
        // A behaviour graph blocker of the same file: the graph is not run.
        assert_eq!(covered[2], Some(hypotheses::ACTOR_EFFECT_STORED));
        assert_eq!(effect_stored(&mut blockers), 0, "once");
    }

    #[test]
    fn a_model_whose_graph_starts_idle_has_its_graph_and_effect_blockers_settled() {
        let e = |file: &str, tag: &str| {
            Error::new(
                "UnsupportedAnimationRuntime",
                file,
                0,
                "x",
                format!(
                    "BSEffectShaderPropertyFloatController;target_in_visual=Some(true);rest={tag}"
                ),
            )
        };
        let mut blockers = vec![
            e("Meshes\\A\\Body.nif", "behavior-graph"),
            e("Meshes\\A\\Body.nif", "effect-idle"),
            e("Meshes\\A\\Body.nif", "transform-varies"),
            e("Meshes\\B\\Hat.nif", "effect-idle"),
        ];
        assert!(!retag_graph_start_idle(&mut blockers, &[]));
        assert!(retag_graph_start_idle(
            &mut blockers,
            &["meshes/a/body.nif".to_string()]
        ));
        let covered: Vec<Option<&str>> = blockers.iter().map(hypotheses::covering).collect();
        assert_eq!(
            covered,
            [
                Some(hypotheses::ACTOR_MODEL_GRAPH_START_IDLE),
                Some(hypotheses::ACTOR_MODEL_GRAPH_START_IDLE),
                None,
                None
            ]
        );
        let state = hypotheses::classify(&blockers[..2], false, &Budget::new(1 << 20)).unwrap();
        assert!(state
            .hypotheses()
            .contains(&hypotheses::ACTOR_MODEL_GRAPH_START_IDLE));
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    /// The animation blocker's rest tag stays in the overlay's summary;
    /// other codes keep only the first part of their text.
    #[test]
    fn the_summary_keeps_an_animation_blockers_rest_tag() {
        let a = Error::new(
            "UnsupportedAnimationRuntime",
            "m.nif",
            0,
            "static state",
            "NiTransformController;target_in_visual=Some(true);rest=transform-varies",
        );
        let b = Error::new("MissingModel", "m.nif", 0, "x", "a;b");
        assert_eq!(
            summary(&[&a, &b]),
            "UnsupportedAnimationRuntime NiTransformController rest=transform-varies; MissingModel a"
        );
    }
}
