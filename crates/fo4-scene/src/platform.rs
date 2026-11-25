//! The play run's named platforms drawn in the play window
//! (`fo4-view --draw-platforms`, with `--play-input` and `--play-elevator`;
//! `spec/play.md`, The window): each reference the run names as a platform
//! that stands in the loaded scene's space is drawn by its model as stored,
//! whatever its render readiness says, under two pending rules:
//!
//! - `play_platform_drawn_v1` ([`hypotheses::PLAY_PLATFORM_DRAWN`]): its
//!   rigid shapes at their stored transforms and its skinned shapes on the
//!   model's own nodes at theirs (`model_skinned_own_nodes_v1`, one more
//!   actor), the record fields, behaviour graph and controllers the draw
//!   does not read left unread: every shape takes its stored transform and
//!   stored material values; the window moves each drawn shape by the
//!   motion of its part (the walker's).
//! - `play_platform_shapes_left_out_v1`
//!   ([`hypotheses::PLAY_PLATFORM_LEFT_OUT`]): a shape the draw refuses
//!   (particle systems, a material a controller drives that no rest rule
//!   settles, any other refusal) is left out, counted by its code, and so
//!   is every effect-shader surface of the model (its glows and light
//!   shafts, whose fields its controllers drive)
//!   (`fo4_render::draw::Build::left_out`), the rest of the model drawn.
use crate::{actor, hypotheses, readiness, Instance, Loader, Scene};
use fo4_core::{Budget, Error};
use fo4_plugin::{typed::Store, FormKey};
use std::collections::{BTreeMap, BTreeSet};

fn rule(code: &str, src: &fo4_core::SourceSpan, found: &str) -> Error {
    Error::new(
        code,
        &src.file,
        src.offset,
        "registered platform rule",
        found,
    )
}

/// The render state of a named platform's instance: each blocker its own
/// rule covers keeps that rule; a record or model blocker no rule covers is
/// set aside under [`hypotheses::PLAY_PLATFORM_DRAWN`]; any other blocker
/// (a material, texture or swap the draw cannot read) still blocks.
pub fn state(i: &Instance, b: &Budget) -> Result<crate::State, Error> {
    let mut blockers = i.blockers.clone();
    let src = i.reference.record.source.clone();
    blockers.push(rule(
        "UnverifiedPlayPlatform",
        &src,
        "the named platform's model as stored",
    ));
    blockers.push(rule(
        "UnverifiedPlayPlatformLeftOut",
        &src,
        "the shapes the draw refuses left out",
    ));
    let usable = |e: &Error| {
        hypotheses::covering(e)
            .is_some_and(|h| hypotheses::status(h) != Some(hypotheses::Status::Refuted))
    };
    let origins = &i.blocker_origins;
    hypotheses::classify_aside(
        &blockers,
        false,
        |k| {
            let e = &blockers[k];
            let o = origins.get(k).copied().unwrap_or(readiness::Origin::Record);
            (k < i.blockers.len()
                && !usable(e)
                && matches!(o, readiness::Origin::Record | readiness::Origin::Model))
            .then_some(hypotheses::PLAY_PLATFORM_DRAWN)
        },
        b,
    )
}

/// What [`add`] did, one line each, for the window's log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Added {
    /// Instances of the scene given the platform state.
    pub drawn: Vec<FormKey>,
    /// Instances put into the scene for a platform the load did not hold.
    pub loaded: Vec<FormKey>,
    /// Platforms with skinned shapes, as one more actor each.
    pub skinned: Vec<FormKey>,
    /// Named platforms not drawn, and why.
    pub not: Vec<(FormKey, String)>,
}

/// The cell each named reference is placed in, by the first of the store's
/// winning placements that places it (`None`: none does), for the named
/// references a scene does not hold ([`add_with`]). One pass over the
/// placements finds every key not asked before, so a caller that keeps it
/// across loads (the play window's cell loads) reads the placements once
/// per key; what [`add_with`] does with it is what [`add`] does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlacedCells(BTreeMap<FormKey, Option<FormKey>>);

impl PlacedCells {
    /// Finds, in one pass over `store`'s placements, the cell of each of
    /// `keys` not asked before.
    pub fn find(&mut self, store: &Store, keys: &[FormKey]) {
        let wanted: BTreeSet<FormKey> = keys
            .iter()
            .map(crate::timeline::norm)
            .filter(|k| !self.0.contains_key(k))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let mut found: BTreeMap<FormKey, FormKey> = BTreeMap::new();
        for (w, c) in store.placement_cells() {
            if found.len() == wanted.len() {
                break;
            }
            let k = crate::timeline::norm(&store.overrides().form_key(w.key, store.order()));
            if wanted.contains(&k) && !found.contains_key(&k) {
                found.insert(k, c);
            }
        }
        for k in wanted {
            let c = found.remove(&k);
            self.0.insert(k, c);
        }
    }

    /// The cell found for `key` (normalised), if it was asked and placed.
    pub fn cell(&self, key: &FormKey) -> Option<&FormKey> {
        self.0.get(&crate::timeline::norm(key))?.as_ref()
    }

    /// Whether `key` was asked (found placed or not).
    pub fn asked(&self, key: &FormKey) -> bool {
        self.0.contains_key(&crate::timeline::norm(key))
    }
}

/// Gives each of `named` that stands in `scene`'s space the platform state
/// ([`state`]), loading its instance when the scene does not hold it (in a
/// worldspace, when it stands inside the horizontal box of the loaded
/// cells' placed references); returns the actors that draw the skinned
/// shapes of those that have some (on the model's own nodes,
/// `model_skinned_own_nodes_v1`).
pub fn add(
    scene: &mut Scene,
    loader: &mut Loader<'_>,
    named: &[FormKey],
    b: &Budget,
) -> Result<(Added, Vec<actor::Actor>), Error> {
    add_with(scene, loader, named, &mut PlacedCells::default(), b)
}

/// [`add`], the cells of the named references the scene does not hold
/// found through `placed` (kept by the caller across loads).
pub fn add_with(
    scene: &mut Scene,
    loader: &mut Loader<'_>,
    named: &[FormKey],
    placed: &mut PlacedCells,
    b: &Budget,
) -> Result<(Added, Vec<actor::Actor>), Error> {
    let store = loader.plugins;
    let held: BTreeSet<FormKey> = scene
        .instances
        .iter()
        .map(|i| crate::timeline::norm(&i.reference.key))
        .collect();
    let missing: Vec<FormKey> = named
        .iter()
        .filter(|k| !held.contains(&crate::timeline::norm(k)))
        .cloned()
        .collect();
    placed.find(store, &missing);
    let space = crate::timeline::scene_space(store, scene);
    let world = store.world(&scene.cell.key).is_some();
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
    let mut out = Added::default();
    let mut actors = Vec::new();
    for key in named {
        let wanted = crate::timeline::norm(key);
        let at = scene
            .instances
            .iter()
            .position(|i| crate::timeline::norm(&i.reference.key) == wanted);
        let k = match at {
            Some(k) => k,
            None => {
                let cell = placed.cell(&wanted).cloned();
                let s = cell.map(|c| crate::timeline::norm(&store.world(&c).unwrap_or(c)));
                if s.as_ref() != Some(&space) {
                    out.not
                        .push((wanted, "not placed in the loaded space".into()));
                    continue;
                }
                let inst = match loader.instance(key, b) {
                    Ok(i) => i,
                    Err(e) => {
                        out.not.push((wanted, format!("the reference: {}", e.code)));
                        continue;
                    }
                };
                let Some(t) = inst.reference.data.transform else {
                    out.not.push((wanted, "no placement".into()));
                    continue;
                };
                if world && !(0..2).all(|k| t[k] >= lo[k] && t[k] <= hi[k]) {
                    out.not
                        .push((wanted, "outside the loaded cells' placed references".into()));
                    continue;
                }
                b.reserve(&mut scene.assets, inst.assets.len(), "platform", 0)?;
                scene.assets.extend_from_slice(&inst.assets);
                scene.assets.sort_unstable();
                scene.assets.dedup();
                b.reserve(&mut scene.instances, 1, "platform", 0)?;
                scene.instances.push(inst);
                out.loaded.push(wanted.clone());
                scene.instances.len() - 1
            }
        };
        let st = state(&scene.instances[k], b)?;
        if !st.ready_at(crate::RuleLevel::Hypotheses) {
            out.not
                .push((wanted, "a blocker neither rule sets aside".into()));
            continue;
        }
        let inst = &mut scene.instances[k];
        inst.state = st;
        out.drawn.push(wanted.clone());
        // The skinned shapes, as one more actor on the model's own nodes.
        let (Some(base), Some(place)) = (&inst.base, inst.reference.data.transform) else {
            continue;
        };
        let Some(&asset) = base.assets.first() else {
            continue;
        };
        if loader.assets.nodes[asset].kind != crate::AssetKind::Model {
            continue;
        }
        let skinned = loader.assets.with_decoded(asset, &b.child(), |d| {
            Ok(match d {
                crate::DecodedAsset::Model(n) => (0..n.blocks.len())
                    .filter(|i| {
                        n.block_types[n.blocks[*i].type_index as usize] == "BSSkin::Instance"
                    })
                    .count(),
                _ => 0,
            })
        })?;
        if skinned == 0 {
            continue;
        }
        let path = loader.assets.nodes[asset].path.clone();
        let src = inst.reference.record.source.clone();
        let mut blockers = vec![
            rule(
                "UnverifiedModelOwnNodes",
                &src,
                "skinned shapes on the model's own nodes",
            ),
            rule(
                "UnverifiedPlayPlatform",
                &src,
                "the named platform's model as stored",
            ),
            rule(
                "UnverifiedPlayPlatformLeftOut",
                &src,
                "the shapes the draw refuses left out",
            ),
        ];
        for e in &inst.blockers {
            if e.code.starts_with("HypothesisAsset") {
                blockers.push(e.clone());
            }
        }
        let state = hypotheses::classify(&blockers, false, b)?;
        actors.push(actor::Actor {
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
            enable: inst.enable.clone(),
            missing_bones_at_bind: false,
            missing_bones_nearest: false,
            pose: None,
            look: None,
            keyword_base: None,
        });
        out.skinned.push(wanted);
    }
    Ok((out, actors))
}

/// A named platform's skinned shape, read for a window that moves its
/// bones with the platform's parts (`play_platform_skin_follows_parts_v1`,
/// [`hypotheses::PLATFORM_SKIN_FOLLOWS_PARTS`]): shape block `shape` of
/// model asset `asset` with its skin ([`actor::pose::SkinnedShape`]) and
/// each bone's matrix as the draw skins the shape at rest on the model's
/// own nodes ([`actor::pose::bone_matrices`] over
/// [`actor::pose::Skeleton::read`]). `None` when the asset is not a model
/// or the shape has no skin.
pub fn skin_at_rest(
    loader: &Loader<'_>,
    asset: usize,
    shape: usize,
    limits: &fo4_core::Limits,
    b: &Budget,
) -> Result<Option<(actor::pose::SkinnedShape, Vec<actor::pose::Affine>)>, Error> {
    let Some(node) = loader.assets.nodes.get(asset) else {
        return Ok(None);
    };
    if node.kind != crate::AssetKind::Model {
        return Ok(None);
    }
    let file = String::from_utf8_lossy(&node.path).into_owned();
    loader.assets.with_decoded(asset, &b.child(), |d| {
        let crate::DecodedAsset::Model(n) = d else {
            return Ok(None);
        };
        let Some(skin) = fo4_nif::skin::shape_skin(&n, shape, &file, limits, b)? else {
            return Ok(None);
        };
        let pose = actor::pose::Skeleton::read(&n, &file, limits, b)?;
        let matrices = actor::pose::bone_matrices(&n, &skin, &pose, &file, b)?;
        let s = actor::pose::SkinnedShape::read(&n, shape, &skin, &file, b)?;
        Ok(Some((s, matrices)))
    })
}
