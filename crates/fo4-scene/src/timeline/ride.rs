//! A moving model drawn in a run timeline's frame (`--timeline-ride-model`,
//! `--timeline-ride`; `spec/timeline.md`, The elevator second): the placed
//! reference the caller names (an elevator's shaft model, refused by the
//! draw for its skinned shapes and the controllers its graph runs) drawn
//! by its skinned shapes, each skinned to the model's own nodes, and with a
//! sequence and its start second from the caller, those nodes where the
//! sequence has them at the frame's second.
//!
//! - `model_skinned_own_nodes_v1` ([`hypotheses::MODEL_OWN_NODES`]): the
//!   model's skinned shapes skinned by `actor_skin_linear_blend_v1`'s
//!   linear blend with the model itself as the skeleton (its nodes at their
//!   stored transforms); its rigid shapes, its controllers and its material
//!   swap are not drawn or applied, its record's open fields not read.
//! - `timeline_ride_sequence_nodes_v1` ([`hypotheses::TIMELINE_RIDE`]): the
//!   nodes the caller's sequence translates take its translation keys at
//!   `(S - START)` clamped to the sequence's span (before START: where the
//!   sequence starts), rotation and scale keys not read.
//!
//! The model is drawn as one more actor of the cast
//! ([`super::cast::Cast::actors`], after the markers' actors), so its
//! instance tag follows the cast's.
use super::cast::Cast;
use crate::{
    actor::{self, pose},
    hypotheses, sequence, DecodedAsset, Loader, Scene,
};
use fo4_core::{Budget, Error, Limits};
use fo4_plugin::FormKey;
use std::collections::BTreeMap;

/// What the caller names.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The placed reference whose model is drawn.
    pub reference: FormKey,
    /// `--timeline-ride SEQUENCE,START`: the sequence (its index among the
    /// model's controller sequences, in block order) and the run's second
    /// it starts at.
    pub sequence: Option<(usize, f64)>,
}

/// The key of the ride's pose among the poses given to
/// `fo4_render::draw::build_with_posed_actors` (the start poses count down
/// from `usize::MAX`).
pub const POSE_KEY: usize = usize::MAX / 2;

fn rule(code: &str, src: &fo4_core::SourceSpan, found: &str) -> Error {
    Error::new(code, &src.file, src.offset, "registered ride rule", found)
}

/// Adds the ride's model to `cast` as one more actor and writes the
/// overlay's line ([`Cast::ride`]); returns its pose when the caller gave a
/// sequence (for the posed build, under [`POSE_KEY`]). A model that cannot
/// be drawn, whose reference is placed in another space than the loaded
/// `scene`'s or (in a worldspace) outside the horizontal box of the loaded
/// cells' placed references, or that the run's `frame` has absent
/// (`super::presence`), is not added; the line says why.
#[allow(clippy::too_many_arguments)]
pub fn add(
    cast: &mut Cast,
    loader: &mut Loader<'_>,
    scene: &Scene,
    frame: &super::Frame,
    o: &Options,
    second: f64,
    limits: &Limits,
    b: &Budget,
) -> Result<Option<(usize, pose::Skeleton)>, Error> {
    let key = super::key_text(&o.reference);
    let store = loader.plugins;
    let wanted = super::norm(&o.reference);
    let cell = store
        .placement_cells()
        .find(|(w, _)| super::norm(&store.overrides().form_key(w.key, store.order())) == wanted)
        .map(|(_, c)| c);
    let space = cell.map(|c| super::norm(&store.world(&c).unwrap_or(c)));
    if space.as_ref() != Some(&super::scene_space(store, scene)) {
        cast.ride = Some(format!("{key}\tnot drawn: not placed in the frame's space"));
        return Ok(None);
    }
    if store.world(&scene.cell.key).is_some() {
        let Some(at) = store
            .get::<fo4_plugin::typed::Refr>(&o.reference, b)?
            .and_then(|r| r.data.transform)
        else {
            cast.ride = Some(format!("{key}\tnot drawn: no placement"));
            return Ok(None);
        };
        let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for t in scene
            .instances
            .iter()
            .filter_map(|i| i.reference.data.transform)
        {
            for k in 0..2 {
                lo[k] = lo[k].min(t[k]);
                hi[k] = hi[k].max(t[k]);
            }
        }
        if !(0..2).all(|k| at[k] >= lo[k] && at[k] <= hi[k]) {
            cast.ride = Some(format!(
                "{key}\tnot drawn: outside the loaded cells' placed references"
            ));
            return Ok(None);
        }
    }
    if super::presence(store, frame, &o.reference, b)?.present() == Some(false) {
        cast.ride = Some(format!("{key}\tnot drawn: absent in the run's frame"));
        return Ok(None);
    }
    let inst = match loader.instance(&o.reference, b) {
        Ok(i) => i,
        Err(e) => {
            cast.ride = Some(format!("{key}\tnot drawn: the reference, {}", e.code));
            return Ok(None);
        }
    };
    let Some(base) = &inst.base else {
        cast.ride = Some(format!("{key}\tnot drawn: no base"));
        return Ok(None);
    };
    let (Some(&asset), Some(place)) = (base.assets.first(), inst.reference.data.transform) else {
        cast.ride = Some(format!("{key}\tnot drawn: no model or no placement"));
        return Ok(None);
    };
    let path = loader.assets.nodes[asset].path.clone();
    let file = String::from_utf8_lossy(&path).into_owned();
    // The skinned shapes and, with a sequence, the moved nodes' pose.
    type Read = (usize, Option<(pose::Skeleton, String)>);
    let read: Result<Read, String> = loader.assets.with_decoded(asset, &b.child(), |d| {
        let DecodedAsset::Model(n) = d else {
            return Ok(Err("its model is not a model".to_string()));
        };
        let skinned = (0..n.blocks.len())
            .filter(|i| {
                n.block_types[n.blocks[*i].type_index as usize] == "BSSkin::Instance"
            })
            .count();
        let Some((k, start)) = o.sequence else {
            return Ok(Ok((skinned, None)));
        };
        let seqs = sequence::sequences(&n);
        let Some(s) = seqs.get(k) else {
            return Ok(Err(format!("no sequence {k} ({} in the model)", seqs.len())));
        };
        let span = f64::from(s.stop - s.start) / f64::from(s.frequency);
        let dt = (second - start).clamp(0., span.max(0.));
        let at = f64::from(s.start) + dt * f64::from(s.frequency);
        let mut moved: BTreeMap<usize, [f64; 3]> = BTreeMap::new();
        let mut largest = 0f64;
        for (name, _) in &s.blocks {
            let Some(keys) = sequence::node_keys(&n, s, name, b)? else {
                continue;
            };
            let (Some(v), Some((block, stored))) =
                (keys.translation.sample(at), sequence::node(&n, name))
            else {
                continue;
            };
            let v = v.map(f64::from);
            let d = (0..3)
                .map(|i| (v[i] - f64::from(stored[i])).powi(2))
                .sum::<f64>()
                .sqrt();
            largest = largest.max(d);
            moved.insert(block, v);
        }
        let worlds = pose::node_worlds_with(&n, &file, &moved, limits, b)?;
        let skeleton = pose::Skeleton::of_worlds(&n, worlds, &file, b)?;
        let text = format!(
            "sequence {k} from {start} s: at {second} s, {dt:.3} s of its {span:.3} s ({}): {} nodes at its translation keys, the farthest {largest:.1} units from its stored place",
            hypotheses::TIMELINE_RIDE,
            moved.len()
        );
        Ok(Ok((skinned, Some((skeleton, text)))))
    })?;
    let (skinned, posed) = match read {
        Ok(x) => x,
        Err(why) => {
            cast.ride = Some(format!("{key}\tnot drawn: {why}"));
            return Ok(None);
        }
    };
    let src = inst.reference.record.source.clone();
    let mut blockers = vec![rule(
        "UnverifiedModelOwnNodes",
        &src,
        "skinned shapes on the model's own nodes",
    )];
    if posed.is_some() {
        blockers.push(rule(
            "UnverifiedTimelineRide",
            &src,
            "the caller's sequence",
        ));
    }
    // The model path's own readings, as the instance carries them.
    for e in &inst.blockers {
        if e.code.starts_with("HypothesisAsset") {
            b.reserve(&mut blockers, 1, &src.file, src.offset)?;
            blockers.push(e.clone());
        }
    }
    let state = hypotheses::classify(&blockers, false, b)?;
    let text = format!(
        "{key}\t{skinned} skinned shapes on the model's own nodes ({}); its rigid shapes, controllers and material swap left out{}",
        hypotheses::MODEL_OWN_NODES,
        posed
            .as_ref()
            .map(|(_, t)| format!("; {t}"))
            .unwrap_or_else(|| "; nodes at their stored transforms".into())
    );
    let a = actor::Actor {
        reference: o.reference.clone(),
        source: src,
        base: Some(base.key.clone()),
        transform: Some(place),
        scale: inst.reference.data.scale,
        plan: Some(actor::Plan {
            race: base.key.clone(),
            section: 0,
            skeleton: path.clone(),
            skeleton_asset: asset,
            parts: vec![actor::Part {
                armor: base.key.clone(),
                addon: base.key.clone(),
                skin: false,
                path,
                asset,
            }],
            head: None,
        }),
        notes: Vec::new(),
        blockers,
        state,
        enable: None,
        missing_bones_at_bind: false,
        missing_bones_nearest: false,
        pose: posed.as_ref().map(|_| POSE_KEY),
        look: None,
        keyword_base: None,
    };
    b.reserve(&mut cast.actors.actors, 1, "timeline ride", 0)?;
    cast.ride_index = Some(cast.actors.actors.len());
    cast.actors.actors.push(a);
    cast.ride = Some(text);
    cast.rules.insert(hypotheses::MODEL_OWN_NODES);
    if posed.is_some() {
        cast.rules.insert(hypotheses::TIMELINE_RIDE);
    }
    Ok(posed.map(|(s, _)| (POSE_KEY, s)))
}

/// Whether an error is one of the draw's refusals of a skinned shape.
fn skin_refusal(e: &Error) -> bool {
    e.code == "UnsupportedSkinning"
        || (e.code == "UnsupportedNifBlock" && e.found.starts_with("BSSkin::"))
}

/// Whether a placement's refusal goes with it to its drawing on its own
/// nodes ([`add_skinned`]): an asset path's reading, and an editor marker
/// subtree's (a named marker, a marker suffix, a visibility marker), so the
/// actor walk sets that subtree aside under the rule that covers it as the
/// placement's own draw would, instead of refusing the whole model.
pub fn carried_to_own_nodes(e: &Error) -> bool {
    e.code.starts_with("HypothesisAsset")
        || matches!(
            e.code.as_str(),
            "UnverifiedNifEditorMarkerVisibility"
                | "UnverifiedNifEditorMarkerSuffixVisibility"
                | "UnverifiedVisibilityEditorMarker"
        )
}

/// `model_skinned_own_nodes_every_placement_v1` (`--skinned-models-own-nodes`):
/// every placed reference of the loaded `scene` that the draw refuses at
/// `level` with one of its skinned shapes among the reasons (not an editor
/// marker, not left out by its enable chain, not already in the cast) is
/// added to `cast` as one more actor drawn by its model's skinned shapes on
/// the model's own nodes at their stored transforms, as [`add`] draws the
/// ride's model without a sequence: its rigid shapes, its controllers and
/// its material swap are left out, its editor marker subtrees set aside
/// under the rules that cover them ([`carried_to_own_nodes`]), its other
/// refusals not read. Writes [`Cast::skinned`].
pub fn add_skinned(
    cast: &mut Cast,
    loader: &Loader<'_>,
    scene: &Scene,
    level: crate::RuleLevel,
    b: &Budget,
) -> Result<(), Error> {
    let first = cast.actors.actors.len();
    let (mut refused, mut other) = (0usize, 0usize);
    for inst in &scene.instances {
        if inst.state.ready_at(level)
            || inst.editor_marker
            || inst
                .enable
                .as_ref()
                .is_some_and(|p| p.enabled == Some(false))
            || !inst.blockers.iter().any(skin_refusal)
        {
            continue;
        }
        refused += 1;
        let key = super::norm(&inst.reference.key);
        if cast
            .actors
            .actors
            .iter()
            .any(|a| super::norm(&a.reference) == key)
        {
            continue;
        }
        let (Some(base), Some(place)) = (&inst.base, inst.reference.data.transform) else {
            other += 1;
            continue;
        };
        let Some(&asset) = base.assets.first() else {
            other += 1;
            continue;
        };
        if loader.assets.nodes[asset].kind != crate::AssetKind::Model {
            other += 1;
            continue;
        }
        let path = loader.assets.nodes[asset].path.clone();
        let src = inst.reference.record.source.clone();
        let mut blockers = vec![rule(
            "UnverifiedModelOwnNodesEvery",
            &src,
            "skinned shapes on the model's own nodes",
        )];
        for e in &inst.blockers {
            if carried_to_own_nodes(e) {
                b.reserve(&mut blockers, 1, &src.file, src.offset)?;
                blockers.push(e.clone());
            }
        }
        let state = hypotheses::classify(&blockers, false, b)?;
        let a = actor::Actor {
            reference: inst.reference.key.clone(),
            source: src,
            base: Some(base.key.clone()),
            transform: Some(place),
            scale: inst.reference.data.scale,
            plan: Some(actor::Plan {
                race: base.key.clone(),
                section: 0,
                skeleton: path.clone(),
                skeleton_asset: asset,
                parts: vec![actor::Part {
                    armor: base.key.clone(),
                    addon: base.key.clone(),
                    skin: false,
                    path,
                    asset,
                }],
                head: None,
            }),
            notes: Vec::new(),
            blockers,
            state,
            enable: None,
            missing_bones_at_bind: false,
            missing_bones_nearest: false,
            pose: None,
            look: None,
            keyword_base: None,
        };
        b.reserve(&mut cast.actors.actors, 1, "timeline skinned models", 0)?;
        cast.actors.actors.push(a);
    }
    let added = cast.actors.actors.len() - first;
    if added > 0 {
        cast.rules.insert(hypotheses::MODEL_OWN_NODES_EVERY);
    }
    cast.skinned = Some((
        first,
        added,
        format!(
            "{refused} placed references of the loaded cells refused with a skinned shape among the reasons; {added} added on their models' own nodes ({}), {other} without a model or a placement",
            hypotheses::MODEL_OWN_NODES_EVERY
        ),
    ));
    Ok(())
}

/// The overlay's line on the placed references [`add_skinned`] added
/// (presentation): how many the build drew and its reasons for the
/// others, counted. `extras` are `fo4_render`'s cast actors after the
/// markers' (cast index, why left out).
pub fn report_skinned(cast: &Cast, extras: &[(usize, Option<String>)]) -> String {
    let Some((first, added, text)) = &cast.skinned else {
        return String::new();
    };
    let mut drawn = 0usize;
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    for (k, w) in extras {
        if *k < *first || *k >= first + added {
            continue;
        }
        match w {
            None => drawn += 1,
            Some(w) => {
                let code = w.split(' ').next().unwrap_or("").to_string();
                *why.entry(code).or_default() += 1;
            }
        }
    }
    let left: Vec<String> = why.iter().map(|(c, n)| format!("{n} {c}")).collect();
    format!(
        "skinned models\t{text}; drawn {drawn}, left out by the build {}{}\n",
        added - drawn,
        if left.is_empty() {
            String::new()
        } else {
            format!(" ({})", left.join(", "))
        }
    )
}

/// The overlay's line on the ride (presentation): what [`add`] wrote, and
/// whether the build drew it or why not, from `fo4_render`'s cast actors
/// after the markers' (`extras`: cast index, why left out).
pub fn report(cast: &Cast, extras: &[(usize, Option<String>)]) -> String {
    let Some(line) = &cast.ride else {
        return String::new();
    };
    let drawn = cast
        .ride_index
        .and_then(|k| extras.iter().find(|(x, _)| *x == k))
        .map(|(_, w)| w.as_deref());
    match drawn {
        Some(None) => format!("ride\tdrawn\t{line}\n"),
        Some(Some(why)) => format!("ride\tleft out by the build: {why}\t{line}\n"),
        None => format!("ride\t{line}\n"),
    }
}
