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
    /// `--timeline`: the frame's camera and overlay.
    pub timeline: Option<crate::timeline::Drawn>,
    /// The light switches: the frame's light and which draws it lights
    /// (`fo4_render::Lighting`); `None` with every switch off.
    pub light: Option<(fo4_scene::light::Light, Vec<bool>)>,
    /// With `--walk`, for the play mode's activation: the placed reference
    /// of each source of the walking world and the object bounds of the
    /// scene's references it holds no collision for.
    #[cfg(feature = "script-event")]
    pub pick: Option<(Vec<Option<fo4_plugin::FormKey>>, fo4_play::pick::Boxes)>,
    /// With `--play-live-world`: what the window's live world needs (the
    /// dormant collision, the references' knowledge, the space, the drawn
    /// references by instance tag).
    #[cfg(feature = "script-event")]
    pub live: Option<crate::play::LiveLoad>,
    /// `--draw-held-doors`: each draw of a loaded door's model, its door
    /// and its world transform posed open ([`held_door_draws`]).
    pub doors: Vec<(
        usize,
        fo4_plugin::FormKey,
        Option<fo4_scene::transform::Transform>,
    )>,
    /// `--draw-static-leaves`: each draw a static's leaf turns
    /// ([`leaf_draws`]).
    pub leaves: Vec<LeafDraw>,
    /// `--draw-platform-skins`: each skinned draw of a named platform with
    /// its skin read at rest ([`skin_draws`]).
    pub skins: Vec<SkinDraw>,
}

/// One skinned draw of a named platform's model, with its skin and each
/// bone's matrix at rest (`play_platform_skin_follows_parts_v1`,
/// `--draw-platform-skins`).
#[derive(Debug, Clone)]
pub struct SkinDraw {
    /// Index into the build's draw list.
    pub draw: usize,
    pub shape: fo4_scene::actor::pose::SkinnedShape,
    pub rest: Vec<fo4_scene::actor::pose::Affine>,
}

/// The largest distance, in model units, between a skinned draw's mesh
/// vertex and the same vertex skinned again at rest by [`skin_draws`]'s
/// reading for the reading to be used (else the draw stays as built).
pub const SKIN_REST_TOLERANCE: f64 = 1e-3;

/// `--draw-platform-skins` (`play_platform_skin_follows_parts_v1`): each
/// skinned draw of a named platform (a chain with bones,
/// `fo4_render::draw::DrawChain::bones`) with its skin and its bones'
/// matrices at rest (`fo4_scene::platform::skin_at_rest`), kept only when
/// skinning it again at rest gives the draw's mesh vertex for vertex within
/// [`SKIN_REST_TOLERANCE`]; one log line.
fn skin_draws(
    build: &Build,
    loader: &Loader<'_>,
    limits: &fo4_core::Limits,
    b: &Budget,
) -> fo4_core::Result<Vec<SkinDraw>> {
    let mut out = Vec::new();
    let mut refused = Vec::new();
    for c in build.chains.iter().filter(|c| !c.bones.is_empty()) {
        let (Some(d), Some(&shape)) = (build.list.draws.get(c.draw), c.blocks.last()) else {
            continue;
        };
        let Some(asset) = loader.assets.nodes.iter().position(|n| n.path == c.model) else {
            refused.push(format!("draw {}: model not among the assets", c.draw));
            continue;
        };
        let Some((skin, rest)) =
            fo4_scene::platform::skin_at_rest(loader, asset, shape, limits, b)?
        else {
            refused.push(format!("draw {}: no skin", c.draw));
            continue;
        };
        let Some(mesh) = build.list.meshes.get(d.mesh) else {
            continue;
        };
        let mut again = Vec::new();
        skin.positions(&rest, &mut again);
        let far = if again.len() == mesh.vertices.len() {
            again
                .iter()
                .zip(&mesh.vertices)
                .map(|(a, v)| {
                    (0..3)
                        .map(|i| (a[i] - v.position[i]).abs())
                        .fold(0.0, f64::max)
                })
                .fold(0.0, f64::max)
        } else {
            f64::INFINITY
        };
        if far > SKIN_REST_TOLERANCE {
            refused.push(format!(
                "draw {}: skinned again at rest {} of {} vertices, {far:.2e} units off",
                c.draw,
                again.len(),
                mesh.vertices.len()
            ));
            continue;
        }
        out.push(SkinDraw {
            draw: c.draw,
            shape: skin,
            rest,
        });
    }
    println!(
        "platforms: {} skinned draws read for their bones to follow the parts (play_platform_skin_follows_parts_v1), {} not{}",
        out.len(),
        refused.len(),
        if refused.is_empty() {
            String::new()
        } else {
            format!(": {}", refused.join("; "))
        }
    );
    Ok(out)
}

/// One draw of a static's model that its leaf turns
/// (`play_static_leaf_two_pose_v1`, `--draw-static-leaves`).
#[derive(Debug, Clone, PartialEq)]
pub struct LeafDraw {
    /// Index into the build's draw list.
    pub draw: usize,
    /// The placed static.
    pub reference: fo4_plugin::FormKey,
    /// The draw's world transform with the leaf at its keyed pose (the
    /// rotation key farthest from the stored one).
    pub keyed: fo4_scene::transform::Transform,
    /// The world-axis box of the static's object bounds (OBND) placed by
    /// its reference: the collision marker boxes that meet it decide the
    /// pose.
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// The mean of the static's turned draws' vertices in the world, with
    /// the leaf stored and keyed: the pose nearer the boxes that meet the
    /// static is the one that holds the way.
    pub stored_at: [f64; 3],
    pub keyed_at: [f64; 3],
}

/// `--draw-static-leaves` (`play_static_leaf_two_pose_v1`): for each
/// instance of `scene` drawn under the rule, its model's leaf
/// (`fo4_scene::leaf::read`), and each of its draws whose node chain passes
/// through the leaf's node with its world transform at the leaf's keyed
/// pose (`fo4_scene::leaf::key_motion`, turned into the world by the
/// static's placement); one log line per static.
fn leaf_draws(
    build: &Build,
    scene: &fo4_scene::Scene,
    loader: &Loader<'_>,
    b: &Budget,
) -> fo4_core::Result<Vec<LeafDraw>> {
    use fo4_scene::timeline::norm;
    use fo4_scene::transform::Transform;
    let mut placed = std::collections::BTreeMap::new();
    for i in &scene.instances {
        if !i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::STATIC_LEAF_TWO_POSE)
        {
            continue;
        }
        let (Some(base), Some(t)) = (&i.base, i.reference.data.transform) else {
            continue;
        };
        let Some(&asset) = base.assets.first() else {
            continue;
        };
        let Ok(r) = Transform::euler(t, i.reference.data.scale.unwrap_or(1.0)) else {
            continue;
        };
        // The object bounds placed: the box of their eight corners.
        let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        if let Some(o) = base.common.bounds {
            for k in 0..8 {
                let c = [0, 1, 2].map(|a| f64::from(o[a + if k & (1 << a) != 0 { 3 } else { 0 }]));
                let w = r.apply(c);
                for a in 0..3 {
                    min[a] = min[a].min(w[a]);
                    max[a] = max[a].max(w[a]);
                }
            }
        }
        placed.insert(norm(&i.reference.key), (asset, r, min, max));
    }
    if placed.is_empty() {
        return Ok(Vec::new());
    }
    let refs: std::collections::BTreeMap<u32, fo4_plugin::FormKey> = build
        .instances
        .iter()
        .map(|i| (i.index, norm(&i.reference)))
        .collect();
    let mut out = Vec::new();
    for (k, (asset, r, min, max)) in &placed {
        let chains: Vec<&fo4_render::draw::DrawChain> = build
            .chains
            .iter()
            .filter(|c| {
                build
                    .list
                    .draws
                    .get(c.draw)
                    .and_then(|d| refs.get(&d.instance))
                    == Some(k)
            })
            .collect();
        let read = loader.assets.with_decoded(*asset, &b.child(), |d| {
            let fo4_scene::DecodedAsset::Model(n) = d else {
                return Ok(None);
            };
            let Ok(leaf) = fo4_scene::leaf::read(&n, b)? else {
                return Ok(None);
            };
            let mut turned = Vec::new();
            for c in &chains {
                if !c.bones.is_empty() {
                    continue;
                }
                if let Some(m) = fo4_scene::leaf::key_motion(&n, &leaf, &c.blocks) {
                    turned.push((c.draw, m));
                }
            }
            Ok(Some((leaf.key_time, leaf.key_angle, turned)))
        })?;
        let Some((time, angle, turned)) = read else {
            println!(
                "leaves: {} drawn under play_static_leaf_two_pose_v1 but its leaf does not read: drawn as stored",
                fo4_scene::timeline::key_text(k)
            );
            continue;
        };
        // Each turned draw's world transform keyed, and the mean of the
        // turned draws' vertices stored and keyed.
        let mut posed = Vec::new();
        let (mut stored_sum, mut keyed_sum, mut n) = ([0.0; 3], [0.0; 3], 0usize);
        for (draw, m) in &turned {
            let Some(d) = build.list.draws.get(*draw) else {
                continue;
            };
            let keyed = r
                .compose(*m)
                .compose(fo4_scene::leaf::inverse(*r))
                .compose(d.world);
            if let Some(mesh) = build.list.meshes.get(d.mesh) {
                for v in &mesh.vertices {
                    let (a, b) = (d.world.apply(v.position), keyed.apply(v.position));
                    for i in 0..3 {
                        stored_sum[i] += a[i];
                        keyed_sum[i] += b[i];
                    }
                    n += 1;
                }
            }
            posed.push((*draw, keyed));
        }
        let mean = |v: [f64; 3]| v.map(|x| x / n.max(1) as f64);
        let (stored_at, keyed_at) = (mean(stored_sum), mean(keyed_sum));
        for (draw, keyed) in posed {
            out.push(LeafDraw {
                draw,
                reference: k.clone(),
                keyed,
                min: *min,
                max: *max,
                stored_at,
                keyed_at,
            });
        }
        println!(
            "leaves: {} drawn as stored, {} of its {} draws turned by {:.1} degrees (its farthest key, at {:.3} s) for its other pose; the turned draws' mean at ({:.0}, {:.0}, {:.0}) stored, ({:.0}, {:.0}, {:.0}) keyed; the collision marker boxes meeting its bounds ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0}) pick the pose (play_static_leaf_two_pose_v1)",
            fo4_scene::timeline::key_text(k),
            turned.len(),
            chains.len(),
            angle.to_degrees(),
            time,
            stored_at[0],
            stored_at[1],
            stored_at[2],
            keyed_at[0],
            keyed_at[1],
            keyed_at[2],
            min[0],
            min[1],
            min[2],
            max[0],
            max[1],
            max[2]
        );
    }
    Ok(out)
}

/// The inverse of a transform with an orthonormal rotation and a scale
/// that is not 0 (`Transform::apply` is `t + s R^T p`).
fn inverse(t: fo4_scene::transform::Transform) -> fo4_scene::transform::Transform {
    let r = t.rotation;
    let s = 1.0 / t.scale;
    fo4_scene::transform::Transform {
        rotation: std::array::from_fn(|i| std::array::from_fn(|j| r[j][i])),
        translation: std::array::from_fn(|i| {
            -s * (0..3).map(|j| r[i][j] * t.translation[j]).sum::<f64>()
        }),
        scale: s,
    }
}

/// The motion in the model's space that its sequence named `Open` gives a
/// draw's node chain at the sequence's last key (`play_door_drawn_as_held_v1`
/// over `nif_sequence_chain_pose_v1`): the chain posed, after the chain as
/// stored undone. `None` when the model has no such sequence or it sets no
/// block of the chain.
fn open_motion(
    loader: &Loader<'_>,
    asset: usize,
    chain: &[usize],
    b: &Budget,
) -> fo4_core::Result<Option<fo4_scene::transform::Transform>> {
    loader.assets.with_decoded(asset, &b.child(), |d| {
        let fo4_scene::DecodedAsset::Model(n) = d else {
            return Ok(None);
        };
        let seqs = fo4_scene::sequence::sequences(&n);
        let Some(s) = seqs.iter().find(|s| s.name == "Open") else {
            return Ok(None);
        };
        Ok(
            fo4_scene::sequence::chain_pose(&n, s, chain, false, f64::from(s.stop), b)?
                .map(|p| p.posed.compose(inverse(p.stored))),
        )
    })
}

/// `--draw-held-doors` (`play_door_drawn_as_held_v1`): each draw of a
/// door of `keys` with its node chain, the door, and the draw's world
/// transform with the chain posed open ([`open_motion`], turned into the
/// world by the door's placement); `None` when no open pose is read.
fn held_door_draws(
    build: &Build,
    scene: &fo4_scene::Scene,
    loader: &Loader<'_>,
    keys: &[fo4_plugin::FormKey],
    b: &Budget,
) -> fo4_core::Result<
    Vec<(
        usize,
        fo4_plugin::FormKey,
        Option<fo4_scene::transform::Transform>,
    )>,
> {
    use fo4_scene::timeline::norm;
    let wanted: std::collections::BTreeSet<fo4_plugin::FormKey> = keys.iter().map(norm).collect();
    let mut placed = std::collections::BTreeMap::new();
    for i in &scene.instances {
        let k = norm(&i.reference.key);
        if !wanted.contains(&k) {
            continue;
        }
        let (Some(base), Some(t)) = (&i.base, i.reference.data.transform) else {
            continue;
        };
        let Some(&asset) = base.assets.first() else {
            continue;
        };
        let Ok(r) =
            fo4_scene::transform::Transform::euler(t, i.reference.data.scale.unwrap_or(1.0))
        else {
            continue;
        };
        placed.insert(k, (asset, r));
    }
    let refs: std::collections::BTreeMap<u32, fo4_plugin::FormKey> = build
        .instances
        .iter()
        .map(|i| (i.index, norm(&i.reference)))
        .collect();
    let mut out = Vec::new();
    for c in &build.chains {
        let Some(d) = build.list.draws.get(c.draw) else {
            continue;
        };
        let Some(k) = refs.get(&d.instance).filter(|k| wanted.contains(*k)) else {
            continue;
        };
        let open = match placed.get(k) {
            Some((asset, r)) if c.bones.is_empty() => open_motion(loader, *asset, &c.blocks, b)?
                .map(|m| r.compose(m).compose(inverse(*r)).compose(d.world)),
            _ => None,
        };
        out.push((c.draw, k.clone(), open));
    }
    Ok(out)
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
/// textures' budget of the reference options. 640 MiB since the poses step:
/// the town frame of the museum step's furthest run at 1470 s keeps 539 MiB
/// of blocks with every actor drawn (`--memory-report`; 512 MiB refused 9
/// of its actors). A viewer bound, not a claim about the game.
pub const BLOCK_BYTES: u64 = 640 << 20;

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

/// The scene assets (models, materials and textures as read from the
/// archives) one cell load leaves for the next (viewer policy: the play
/// window's cell loads, `crate::play`): a load whose path policy and clock
/// switches are the same starts from them instead of an empty cache, so the
/// cells shared by neighbouring grid squares are not read again. What the
/// load builds is what an empty cache gives.
pub type KeptAssets<'s> = Option<fo4_scene::Assets<'s>>;

/// The most memory the kept assets may hold for the next load to start from
/// them (else the next load starts empty).
pub const KEPT_ASSETS_MAX: u64 = 768 << 20;

#[allow(clippy::too_many_arguments)]
pub fn cell<'s>(
    sources: Option<&'s Sources>,
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
    script_event: Option<&crate::vm_event::ScriptEvent>,
    timeline: Option<(&crate::timeline::Args, &crate::timeline::Prepared)>,
    light: (fo4_scene::light::Switches, fo4_scene::light::Assumptions),
    play_live: bool,
    drawing: crate::cli::Drawing,
    platforms: &[fo4_plugin::FormKey],
    kept: Option<&mut KeptAssets<'s>>,
) -> AnyResult<Loaded> {
    // The game data a caller keeps open, else opened for this load (and no
    // assets kept: they would outlive the data they were read from).
    let Some(src) = sources else {
        let opened = open(manifest, data)?;
        return crate::load::cell(
            Some(&opened),
            manifest,
            data,
            cell,
            level,
            walk,
            terrain,
            around,
            options,
            scene_memory,
            actors,
            clip,
            script_event,
            timeline,
            light,
            play_live,
            drawing,
            platforms,
            None,
        );
    };
    #[cfg(not(feature = "script-event"))]
    let _ = play_live;
    #[cfg(feature = "script-event")]
    let load_started = std::time::Instant::now();
    if walk {
        fo4_walk::accept(&walk_rules(), level, fo4_scene::hypotheses::status)?;
    }
    #[cfg(feature = "script-event")]
    if script_event.is_some() && level != RuleLevel::Hypotheses {
        return Err(format!(
            "ScriptEventRuleLevel: --script-event relies on the pending rules {:?} (--under-hypotheses)",
            crate::vm_event::rules()
        )
        .into());
    }
    #[cfg(not(feature = "script-event"))]
    if script_event.is_some() {
        return Err("--script-event needs a viewer built with --features script-event".into());
    }
    // The game data a caller keeps open, else opened for this load.
    let Sources {
        manifest,
        store,
        vfs,
    } = src;
    let limits = &manifest.limits;
    let budget = scene_budget(scene_memory, limits);
    let mut loader = Loader::new(store, vfs, &manifest.language, level, &budget);
    // The clock's switches (off by default) change how models are read.
    if options.clock.any() {
        loader.set_clock_switches(options.clock, &budget);
    }
    // `--draw-child-flag-agrees` (off by default): render readiness reads
    // the agreeing own flags' chains (`xesp_child_flag_agrees_chain_v1`).
    if drawing.child_flag_agrees {
        loader.set_render_child_flag_agrees(true);
    }
    // `--draw-static-fields` and `--draw-static-leaves` (off by default):
    // render readiness sets a static's four reference fields and a leaf
    // model's animation blockers aside (`refr_static_fields_render_unchanged_v1`,
    // `play_static_leaf_two_pose_v1`).
    if drawing.static_fields {
        loader.set_render_static_fields(true);
    }
    if drawing.static_leaves {
        loader.set_render_static_leaves(true);
    }
    // `--draw-reference-alpha` (off by default): render readiness sets a
    // reference's unread `XALP` aside (`refr_xalp_render_unchanged_v1`).
    if drawing.reference_alpha {
        loader.set_render_reference_alpha(true);
    }
    // The assets the previous load left, when they were read the same way.
    let mut kept = kept;
    if let Some(slot) = kept.as_deref_mut() {
        if let Some(a) = slot.take() {
            if a.clock == loader.assets.clock
                && a.path_policy == loader.assets.path_policy
                && !a.unavailable()
                && a.memory_used() <= KEPT_ASSETS_MAX
            {
                loader.assets = a;
            }
        }
    }
    let mut cell_rule = None;
    // CELL as a record key (`plugin:XXXXXXXX`, as the timeline writes
    // keys): that cell (the play mode's change of space loads cells so).
    let by_key = fo4_scene::timeline::parse_key(cell);
    let mut scene = match (timeline, by_key) {
        // CELL -: the cell the player's track is in at the second.
        (Some((_, p)), _) if cell == "-" => {
            let (key, rule) = fo4_scene::timeline::player_cell(store, &p.frame, &budget)?;
            cell_rule = rule;
            scoped(&budget, |b| loader.cell(&key, b))?
        }
        (_, Some(key)) => scoped(&budget, |b| loader.cell(&key, b))?,
        _ => scoped(&budget, |b| loader.by_editor_id(cell, b))?,
    };
    // --around N: the placed references of the exterior cells within N grid
    // steps join the scene, as in the CPU render. Without it nothing changes.
    let grid = if around > 0 {
        let started = std::time::Instant::now();
        let a = scoped(&budget, |b| {
            fo4_scene::grid::around(store, &scene.cell.key, around, b)
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
    // --timeline: the run's changes at the second, before anything is
    // built. Without it nothing changes.
    let applied = match timeline {
        Some((_, p)) => {
            let mut a =
                fo4_scene::timeline::apply(&mut scene, &mut loader, &p.frame, level, &budget)?;
            a.rules.extend(cell_rule);
            println!(
                "timeline: second {} in cell {}; {} hidden, {} shown, {} enable children changed, {} moved, {} moved out",
                p.frame.second,
                fo4_scene::timeline::key_text(&scene.cell.key),
                a.hidden.len(),
                a.shown.len(),
                a.chain_changed,
                a.moved.len(),
                a.moved_out.len()
            );
            Some(a)
        }
        None => None,
    };
    // --script-event: the references the run disables leave the scene
    // before anything is built. Without it nothing changes.
    #[cfg(feature = "script-event")]
    let mut script_changes = None;
    #[cfg(feature = "script-event")]
    if let Some(ev) = script_event {
        script_changes = Some(crate::vm_event::apply(
            ev,
            store,
            vfs,
            limits.max_memory,
            &mut scene,
            &mut loader,
            &budget,
            level,
        )?);
    }
    let mut animation = None;
    if clip.is_some() && !actors {
        return Err("ActorClipWithoutActors: --actor-clip poses the drawn actors".into());
    }
    // --timeline: the frame's actors (markers, and with --timeline-cast the
    // characters, drawn with the build).
    let timeline_cast = match (timeline, &applied) {
        (Some((args, p)), Some(_)) => Some(crate::timeline::cast(
            p,
            args,
            &scene,
            &mut loader,
            level,
            &budget,
        )?),
        _ => None,
    };
    let mut timeline_cast = timeline_cast;
    // With a start clip flag, the actors their graph's clip poses
    // (`fo4_scene::timeline::cast::start_poses`); without one the build is
    // the rest-pose one.
    let poses = match (
        timeline,
        timeline_cast.as_mut().and_then(|c| c.cast.as_mut()),
    ) {
        (Some((args, p)), Some(c)) if args.pose.any() => {
            Some(fo4_scene::timeline::cast::start_poses(
                c,
                &loader,
                vfs,
                &args.pose,
                p.frame.second,
                level,
                limits,
                &budget,
            )?)
        }
        _ => None,
    };
    // --play-live-world: the references the run may enable drawn too (the
    // window hides them until it does), taken out again after the build.
    #[cfg(feature = "script-event")]
    let dormant_drawn = if play_live && walk {
        Some(crate::play::draw_dormant(&mut scene, &mut loader, &budget)?)
    } else {
        None
    };
    // --draw-platforms: the named platforms drawn by their models as stored
    // (`fo4_scene::platform`), their skinned shapes as actors on their
    // models' own nodes. Without it nothing changes.
    let platform_actors = if drawing.platforms && !platforms.is_empty() {
        let (added, actors) =
            fo4_scene::platform::add(&mut scene, &mut loader, platforms, &budget)?;
        let keys = |v: &[fo4_plugin::FormKey]| {
            v.iter()
                .map(fo4_scene::timeline::key_text)
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!(
            "platforms: drawn as stored {} [{}], of them loaded for the drawing {} [{}], with skinned shapes on their own nodes {} [{}]",
            added.drawn.len(),
            keys(&added.drawn),
            added.loaded.len(),
            keys(&added.loaded),
            added.skinned.len(),
            keys(&added.skinned)
        );
        for (k, why) in &added.not {
            println!(
                "platforms: {} not drawn: {why}",
                fo4_scene::timeline::key_text(k)
            );
        }
        actors
    } else {
        Vec::new()
    };
    // --draw-held-doors: the loaded doors drawn by their models as stored
    // (`fo4_scene::platform`'s state, as a named platform), shown by the
    // window only while the walker's held doors' reading says so.
    let door_keys: Vec<fo4_plugin::FormKey> = if drawing.held_doors {
        scene
            .instances
            .iter()
            .filter(|i| {
                i.base
                    .as_ref()
                    .is_some_and(|b| b.record.signature == *b"DOOR")
            })
            .map(|i| i.reference.key.clone())
            .collect()
    } else {
        Vec::new()
    };
    if !door_keys.is_empty() {
        let (added, skinned) =
            fo4_scene::platform::add(&mut scene, &mut loader, &door_keys, &budget)?;
        println!(
            "doors: {} of {} loaded doors drawn as stored for the held doors' reading (play_door_drawn_as_held_v1), {} not ({} with skinned shapes left out)",
            added.drawn.len(),
            door_keys.len(),
            added.not.len(),
            skinned.len()
        );
    }
    // --timeline-ride-model: the ride's model as one more actor of the
    // cast, posed with --timeline-ride.
    let mut poses = poses;
    if let (Some((args, p)), Some(c)) = (
        timeline,
        timeline_cast.as_mut().and_then(|c| c.cast.as_mut()),
    ) {
        if let Some(o) = &args.ride {
            if let Some((k, pose)) = fo4_scene::timeline::ride::add(
                c,
                &mut loader,
                &scene,
                &p.frame,
                o,
                p.frame.second,
                limits,
                &budget,
            )? {
                poses.get_or_insert_with(Default::default).insert(k, pose);
            }
        }
        // --skinned-models-own-nodes: every placement refused for its
        // skinned shapes as one more actor, after the ride.
        if args.skinned_models {
            fo4_scene::timeline::ride::add_skinned(c, &loader, &scene, level, &budget)?;
        }
    }
    let cast_actors = timeline_cast.as_ref().and_then(|c| c.cast.as_ref());
    let mut build = if let Some(c) = cast_actors {
        let build = match poses {
            Some(poses) => {
                fo4_render::draw::build_with_posed_actors(
                    &scene, &c.actors, poses, &loader, limits, options, &budget,
                )?
                .0
            }
            None => fo4_render::draw::build_with_actors(
                &scene, &c.actors, &loader, limits, options, &budget,
            )?,
        };
        println!("timeline cast: {}", actor_summary(&c.actors, &build));
        if let Some(f) = timeline.and_then(|(a, _)| a.memory_report.as_ref()) {
            std::fs::write(f, fo4_render::memory::report(&build))?;
        }
        for line in actor_lines(&c.actors, &build) {
            println!("  {line}");
        }
        build
    } else if actors {
        // Placed actors, drawn after the placed references (`--no-actors`
        // leaves them out and the draw list is the one before).
        let started = std::time::Instant::now();
        #[allow(unused_mut)]
        let mut actors = scoped(&budget, |b| {
            fo4_scene::actor::resolve(&scene, &mut loader, b)
        })?;
        #[cfg(feature = "script-event")]
        if let Some(c) = &script_changes {
            crate::vm_event::propagate_actors(&mut actors, c, level);
        }
        actors.actors.extend(platform_actors);
        let resolved = started.elapsed().as_secs_f64();
        let build = match clip {
            // Without --actor-clip the draw list is the rest-pose one.
            None => fo4_render::draw::build_with_actors(
                &scene, &actors, &loader, limits, options, &budget,
            )?,
            Some(o) => {
                let started = std::time::Instant::now();
                let (poses, mut a) =
                    crate::clip::prepare(o, &actors, &loader, vfs, level, limits, &budget)?;
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
    } else if !platform_actors.is_empty() {
        let actors = fo4_scene::actor::Actors {
            actors: platform_actors,
            disabled: 0,
            distant: 0,
        };
        fo4_render::draw::build_with_actors(&scene, &actors, &loader, limits, options, &budget)?
    } else {
        fo4_render::draw::build(&scene, &loader, limits, options, &budget)?
    };
    // --draw-platforms: what the build left out of the named platforms.
    if drawing.platforms && !platforms.is_empty() {
        let mut codes: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for (_, c) in &build.left_out {
            *codes.entry(c.as_str()).or_default() += 1;
        }
        let named: Vec<_> = platforms.iter().map(fo4_scene::timeline::norm).collect();
        let omitted: Vec<String> = build
            .omitted
            .iter()
            .filter(|o| named.contains(&fo4_scene::timeline::norm(&o.reference)))
            .map(|o| {
                format!(
                    "{} {}",
                    fo4_scene::timeline::key_text(&o.reference),
                    o.causes.first().map_or("", |e| e.code.as_str())
                )
            })
            .collect();
        println!(
            "platforms: shapes left out by the build {} ({}); platforms the build refused: {}",
            build.left_out.len(),
            codes
                .iter()
                .map(|(c, n)| format!("{n} {c}"))
                .collect::<Vec<_>>()
                .join(", "),
            if omitted.is_empty() {
                "none".into()
            } else {
                omitted.join(", ")
            }
        );
        // Each platform draw: what it is and where it stands as built.
        let tags: std::collections::BTreeMap<u32, String> = build
            .instances
            .iter()
            .filter(|i| named.contains(&fo4_scene::timeline::norm(&i.reference)))
            .map(|i| (i.index, fo4_scene::timeline::key_text(&i.reference)))
            .collect();
        let skinned: std::collections::BTreeSet<usize> = build
            .chains
            .iter()
            .filter(|c| !c.bones.is_empty())
            .map(|c| c.draw)
            .collect();
        let effects: std::collections::BTreeSet<usize> =
            build.effect_draws.iter().copied().collect();
        for (k, d) in build.list.draws.iter().enumerate() {
            let Some(r) = tags.get(&d.instance) else {
                continue;
            };
            let Some(m) = build.list.meshes.get(d.mesh) else {
                continue;
            };
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for v in &m.vertices {
                let p = d.world.apply(v.position);
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            println!(
                "platforms: draw {k} of {r}: {} triangles, {}{}alpha {:?}, from ({:.0}, {:.0}, {:.0}) to ({:.0}, {:.0}, {:.0})",
                m.triangles.len(),
                if skinned.contains(&k) { "skinned on its own nodes, " } else { "rigid, " },
                if effects.contains(&k) { "effect surface, " } else { "" },
                d.alpha,
                lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
            );
        }
    }
    let doors = if door_keys.is_empty() {
        Vec::new()
    } else {
        let d = held_door_draws(&build, &scene, &loader, &door_keys, &budget)?;
        println!(
            "doors: {} draws of the held doors' models, {} with an open pose",
            d.len(),
            d.iter().filter(|x| x.2.is_some()).count()
        );
        d
    };
    let skins = if drawing.platform_skins && drawing.platforms && !platforms.is_empty() {
        skin_draws(&build, &loader, limits, &budget)?
    } else {
        Vec::new()
    };
    let leaves = if drawing.static_leaves {
        leaf_draws(&build, &scene, &loader, &budget)?
    } else {
        Vec::new()
    };
    #[cfg(feature = "script-event")]
    if let Some(d) = dormant_drawn {
        crate::play::undraw_dormant(&mut scene, d);
    }
    // The terrain of the cell and, for walking, of its neighbours. Without
    // --terrain and --walk nothing is loaded and the draw list is unchanged.
    let ground = match (&grid, walk || terrain) {
        (Some(a), true) => Some(fo4_walk::terrain::load_with(store, a, level, &budget)?),
        (None, true) => Some(fo4_walk::terrain::load_around(
            store,
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
                    store,
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
                        store,
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
    // Draws before this index (the build's and the terrain's) are lit by
    // the light switches, but for effect shader surfaces; the markers after
    // it are not (as the CPU render).
    let lit_until = build.list.draws.len();
    // --timeline: the actors' markers after every other draw, the camera
    // and the overlay.
    let mut drawn = match (timeline, &applied, timeline_cast) {
        (Some((args, p)), Some(a), Some(c)) => Some(crate::timeline::finish(
            p, args, c, level, a, &mut build, &budget,
        )?),
        _ => None,
    };
    // The light switches: the frame's light (`fo4_scene::light`).
    let light = match fo4_scene::light::frame(store, &scene, light.0, light.1, &budget)? {
        Some(mut l) => {
            let effects: std::collections::BTreeSet<usize> =
                build.effect_draws.iter().copied().collect();
            let lit: Vec<bool> = (0..build.list.draws.len())
                .map(|i| i < lit_until && !effects.contains(&i))
                .collect();
            // `--exposure-eye`: the mean light on what the timeline's
            // camera sees, measured with the reference frame's camera as the
            // CPU render measures it (with --eye: `main`).
            if let (true, Some(d)) = (l.exposure.eye, &drawn) {
                let size = fo4_scene::camera::REFERENCE_SIZE;
                let cam = fo4_scene::camera::look_at(
                    d.eye,
                    d.look,
                    fo4_scene::camera::reference_frustum(size),
                    size,
                )?;
                l.exposure.visible = drawing.exposure_visible;
                let points = if l.exposure.visible {
                    fo4_render::seen_visible(
                        &fo4_render::lit_faces(&build.list, &lit),
                        &cam,
                        fo4_render::VISIBLE_GRID,
                    )
                } else {
                    fo4_render::seen_lit_points(&build.list, &lit, &cam)
                };
                l.expose_seen(&points);
            }
            for line in &l.report {
                println!("light: {line}");
            }
            if let Some(d) = &mut drawn {
                d.overlay.push_str(&fo4_scene::light::overlay(&l));
            }
            Some((l, lit))
        }
        None => None,
    };
    #[cfg(feature = "script-event")]
    let mut pick = None;
    #[cfg(feature = "script-event")]
    let mut live = None;
    #[cfg(feature = "script-event")]
    let walk_started = std::time::Instant::now();
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
        #[cfg(feature = "script-event")]
        {
            let refs = fo4_play::pick::references(&world, &c);
            let (boxes, _) = fo4_play::pick::object_bounds(&scene, &refs);
            if play_live {
                let mut counts = std::collections::BTreeMap::new();
                let (dormant, info) = fo4_play::world::dormant(
                    &mut loader,
                    &mut scene,
                    limits,
                    &refs,
                    placement,
                    &mut counts,
                    &budget,
                )?;
                for (k, v) in &counts {
                    println!("play: {k}\t{v}");
                }
                live = Some(crate::play::LiveLoad {
                    dormant,
                    info,
                    space: fo4_scene::timeline::norm(&fo4_scene::timeline::scene_space(
                        store, &scene,
                    )),
                    tags: build
                        .instances
                        .iter()
                        .map(|i| (i.index, fo4_scene::timeline::norm(&i.reference)))
                        .collect(),
                    world: None,
                });
            }
            pick = Some((refs, boxes));
        }
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
        // The play window's cell loads: where the load's time goes
        // (measurement only).
        #[cfg(feature = "script-event")]
        if play_live {
            println!(
                "play: load phases: {:.2} s to the walking world, {:.2} s for it",
                walk_started.duration_since(load_started).as_secs_f64(),
                walk_started.elapsed().as_secs_f64()
            );
        }
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
    // The terrain's sources come after the cell's: no reference.
    #[cfg(feature = "script-event")]
    if let (Some((refs, _)), Some(w)) = (pick.as_mut(), walk.as_ref()) {
        refs.resize(w.sources.len(), None);
    }
    if let Some(slot) = kept {
        *slot = Some(std::mem::replace(
            &mut loader.assets,
            fo4_scene::Assets::with_policy(vfs, &budget, level),
        ));
    }
    Ok(Loaded {
        build,
        data_root: manifest.root.clone(),
        walk,
        animation,
        timeline: drawn,
        light,
        #[cfg(feature = "script-event")]
        pick,
        #[cfg(feature = "script-event")]
        live,
        doors,
        leaves,
        skins,
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
    fn a_placement_and_its_inverse_compose_to_the_identity() {
        // `play_door_drawn_as_held_v1`: the open pose is turned into the
        // world by the door's placement and its inverse.
        let t =
            fo4_scene::transform::Transform::euler([10., -20., 30., 0.3, -0.2, 1.1], 1.5).unwrap();
        // The placement's rotation is computed with binary32 sine and
        // cosine, so it is orthonormal to about 1e-7 only.
        for m in [inverse(t).compose(t), t.compose(inverse(t))] {
            let id = fo4_scene::transform::Transform::IDENTITY;
            let near = (0..3).all(|i| {
                (0..3).all(|j| (m.rotation[i][j] - id.rotation[i][j]).abs() < 1e-5)
                    && m.translation[i].abs() < 1e-4
            }) && (m.scale - 1.0).abs() < 1e-9;
            assert!(near, "{m:?}");
        }
    }
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
