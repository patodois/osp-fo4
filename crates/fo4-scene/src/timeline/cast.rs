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
//!   from the run).
//!
//! An actor that cannot be resolved is drawn as its marker by the caller,
//! with the reason ([`Cast::why_not`]).
use super::{key_text, norm, Marker};
use crate::{actor, hypotheses, Loader, RuleLevel, Scene};
use fo4_core::{Budget, Error, SourceSpan};
use fo4_plugin::FormKey;
use std::collections::BTreeSet;

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
/// `found` text, for one overlay line (presentation).
pub fn summary(errors: &[&Error]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for e in errors {
        if seen.iter().any(|s| s.starts_with(&format!("{} ", e.code))) {
            continue;
        }
        let found: String = e
            .found
            .split(';')
            .next()
            .unwrap_or("")
            .chars()
            .take(48)
            .collect();
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
        let mut a = actor::resolve_reference(loader, &k, &src, b)?;
        a.missing_bones_at_bind = options.missing_bones_at_bind;
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
    let _ = writeln!(
        s,
        "cast\t{} actors: {} drawn as characters in the skeleton's rest pose (presentation: no animation), {} as markers",
        cast.markers.len(),
        characters.len(),
        fallback.len()
    );
    for (k, tag) in characters {
        let bind = if bind_bones.contains(k) {
            format!(
                "; a bone its skeleton lacks skinned at its bind transform ({})",
                hypotheses::ACTOR_MISSING_BONE_BIND
            )
        } else {
            String::new()
        };
        let _ = writeln!(
            s,
            "character\ttag {tag}\t{}\tfacing {}{bind}",
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
