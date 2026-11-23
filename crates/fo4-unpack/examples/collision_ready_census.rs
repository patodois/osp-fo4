#![forbid(unsafe_code)]
//! Collision refused by readiness in the cells a walk loads, and why
//! (`spec/collision-ready-census.txt`, `spec/collision-placement.md`,
//! Blockers off the collision chain).
//!
//! For each cell the caller names, loaded as the play mode loads its walking
//! world (the cell and the cells about it within `--around`, at the
//! hypotheses level): the references whose collision the readiness refuses
//! (`CollisionNotReady`), whether their models place collision at all, every
//! blocker that refuses it by origin, code, block type and where it lies
//! against the blocks the model's placed bodies read (`readiness::Bearing`),
//! and what `readiness::Switches::off_chain`
//! (`collision_model_blockers_off_chain_v1`) settles. A reference loaded by
//! several cells is counted once. `--probe CELL X,Y,Z` casts a ray down at a
//! point of that cell's walking world with the switch off and on. `--door
//! REF` describes what the data holds about one animated reference's
//! collision at load: its sequences, which nodes of its collision chains
//! each moves, the stored pose against each sequence's first and last keys,
//! the behaviour graph's start state.
//!
//! Writes `collision-ready-census.txt` (counts and kinds only) and
//! `collision-ready-names.txt` (references, models; never versioned) into
//! OUT_DIR.
//!
//! `collision_ready_census MANIFEST OUT_DIR --cell PLUGIN:HEX [--cell ...] [--around N]
//!  [--probe PLUGIN:HEX X,Y,Z ...] [--door PLUGIN:HEX ...]`
use fo4_core::{Budget, Game, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::collision::{self, CellCollision, Options};
use fo4_scene::hypotheses::{self, Status};
use fo4_scene::readiness::{self, Bearing, Origin, Use};
use fo4_scene::{timeline as tl, DecodedAsset, Instance, Loader, RuleLevel};
use fo4_walk::World;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

type E = Box<dyn std::error::Error>;

const USAGE: &str = "collision_ready_census MANIFEST OUT_DIR --cell PLUGIN:HEX [--cell ...] [--around N] [--probe PLUGIN:HEX X,Y,Z ...] [--door PLUGIN:HEX ...]";

/// Whether `e` alone refuses the collision: collision needs it and no
/// usable rule covers it.
fn refusing(e: &fo4_core::Error, origin: Origin) -> bool {
    match readiness::use_of(origin, e) {
        Use::Collision => !matches!(
            hypotheses::covering(e).and_then(hypotheses::status),
            Some(Status::Pending | Status::Supported)
        ),
        Use::RenderOnly => false,
    }
}

/// The block type or field a blocker names: the text before the first `;`
/// of its found value (animation blockers carry `TYPE;target...;rest=TAG`),
/// with the rest tag kept for animation blockers.
fn kind_of(e: &fo4_core::Error) -> String {
    let mut parts = e.found.split(';');
    let head = parts.next().unwrap_or("").to_owned();
    let rest = e
        .found
        .split(';')
        .find_map(|p| p.strip_prefix("rest="))
        .map(|t| format!(" rest={t}"))
        .unwrap_or_default();
    let head = if head.len() > 60 {
        format!("{}...", &head[..60])
    } else {
        head
    };
    format!("{head}{rest}")
}

fn origin_name(o: Origin) -> &'static str {
    match o {
        Origin::Record => "record",
        Origin::MaterialSwap => "material-swap",
        Origin::Model => "model",
        Origin::ModelCitation => "model-citation",
        Origin::RenderAsset => "render-asset",
    }
}

fn bearing_name(b: Option<Bearing>) -> &'static str {
    match b {
        None => "not-in-a-model",
        Some(Bearing::OffChain) => "off-chain",
        Some(Bearing::OnChain) => "on-chain",
        Some(Bearing::Moved) => "chain-moved",
        Some(Bearing::NoBody) => "no-body",
        Some(Bearing::Unlocated) => "unlocated",
    }
}

fn refused(c: &CellCollision) -> BTreeSet<FormKey> {
    c.unplaced
        .iter()
        .filter(|u| u.code == "CollisionNotReady")
        .map(|u| u.reference.clone())
        .collect()
}

/// One reference refused with the switch off.
struct Row {
    key: FormKey,
    base: String,
    has_collision: bool,
    settled: bool,
    /// Settled by the off-chain switch alone.
    by_chain: bool,
    /// (origin, code, kind, bearing) of each refusing blocker.
    blockers: Vec<(String, String, String, &'static str)>,
    /// Block types that move a chain node, for chain-moved blockers.
    movers: BTreeSet<String>,
    models: BTreeSet<String>,
}

fn tally(m: &mut BTreeMap<String, usize>, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        return Err(USAGE.into());
    }
    let mut cells: Vec<FormKey> = Vec::new();
    let mut around = 1u32;
    let mut probes: Vec<(FormKey, [f64; 3])> = Vec::new();
    let mut doors: Vec<FormKey> = Vec::new();
    let mut it = a[2..].iter();
    while let Some(x) = it.next() {
        let mut val = || it.next().ok_or(USAGE);
        match x.as_str() {
            "--cell" => cells.push(tl::parse_key(val()?).ok_or(USAGE)?),
            "--around" => around = val()?.parse()?,
            "--probe" => {
                let k = tl::parse_key(val()?).ok_or(USAGE)?;
                let v: Vec<f64> = val()?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                let p: [f64; 3] = v.try_into().map_err(|_| USAGE)?;
                probes.push((k, p));
            }
            "--door" => doors.push(tl::parse_key(val()?).ok_or(USAGE)?),
            _ => return Err(USAGE.into()),
        }
    }
    let out = Path::new(&a[1]);
    std::fs::create_dir_all(out)?;
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let mut census = String::new();
    let mut names = String::new();
    let _ = writeln!(
        census,
        "# Collision refused by readiness in the cells a walk loads (collision_ready_census; counts and kinds only).\n\
         # Each cell loaded with the cells about it within {around}, at the hypotheses level, as the play mode's walking world.\n\
         # off: as main builds it; off-chain: with readiness::Switches::off_chain (collision_model_blockers_off_chain_v1);\n\
         # on: with off_chain and reference_alpha (refr_xalp_collision_unchanged_v1)."
    );
    let mut rows: Vec<Row> = Vec::new();
    let mut seen: BTreeSet<FormKey> = BTreeSet::new();
    for (ci, cell) in cells.iter().enumerate() {
        let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
        let mut scene = loader.cell(cell, &budget)?;
        let grid = fo4_scene::grid::around(&store, &scene.cell.key, around, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &grid, &budget)?;
        let options = Options {
            level,
            body_placement: None,
        };
        let off = collision::cell(&scene, &loader, &m.limits, options, &budget)?;
        loader.set_collision_switches(readiness::Switches {
            off_chain: true,
            reference_alpha: false,
        });
        let chain = collision::cell(&scene, &loader, &m.limits, options, &budget)?;
        loader.set_collision_switches(readiness::Switches {
            off_chain: true,
            reference_alpha: true,
        });
        let on = collision::cell(&scene, &loader, &m.limits, options, &budget)?;
        loader.set_collision_switches(readiness::Switches::default());
        let (r_off, r_chain, r_on) = (refused(&off), refused(&chain), refused(&on));
        let _ = writeln!(
            census,
            "\n## Cell load {} ({} instances)\n\
             collision instances placed\toff {}\toff-chain {}\ton {}\n\
             references refused (CollisionNotReady)\toff {}\toff-chain {}\ton {}\n\
             triangles, hulls of the placed collision\toff {}, {}\toff-chain {}, {}\ton {}, {}",
            ci + 1,
            scene.instances.len(),
            off.instances.len(),
            chain.instances.len(),
            on.instances.len(),
            r_off.len(),
            r_chain.len(),
            r_on.len(),
            world_stats(&off)?.0,
            world_stats(&off)?.1,
            world_stats(&chain)?.0,
            world_stats(&chain)?.1,
            world_stats(&on)?.0,
            world_stats(&on)?.1,
        );
        let _ = writeln!(names, "cell load {}: {}", ci + 1, tl::key_text(cell));
        // Probes of this cell.
        for (pk, p) in probes.iter().filter(|(k, _)| k == cell) {
            for (label, c) in [("off", &off), ("on", &on)] {
                let placement = c.body_placement.ok_or("no body placement level")?;
                let w = World::build(c, placement, &budget)?;
                let hit = w.ray_down(p[0], p[1], p[2]);
                let who = hit
                    .and_then(|h| w.sources.get(h.source as usize))
                    .and_then(|s| s.instance)
                    .map(|i| tl::key_text(&c.placements[c.instances[i].placement].reference));
                let _ = writeln!(
                    names,
                    "probe {} {:?} {label}: {}",
                    tl::key_text(pk),
                    p,
                    match hit {
                        Some(h) => format!(
                            "hit z {:.1} ({:.1} below) {}",
                            h.z,
                            p[2] - h.z,
                            who.unwrap_or_else(|| "terrain or other".into())
                        ),
                        None => "no hit".into(),
                    }
                );
                let _ = writeln!(
                    census,
                    "probe {} {label}\t{}",
                    probes
                        .iter()
                        .position(|q| q.0 == *pk && q.1 == *p)
                        .unwrap_or(0)
                        + 1,
                    match hit {
                        Some(h) => format!("hit {:.1} units below the point", p[2] - h.z),
                        None => "no hit".into(),
                    }
                );
            }
        }
        let by_key: BTreeMap<&FormKey, &Instance> = scene
            .instances
            .iter()
            .map(|i| (&i.reference.key, i))
            .collect();
        // Refused references whose collision, placed as stored, a probe's
        // ray meets: the holes under the walk's points.
        for (pi, (pk, p)) in probes.iter().enumerate().filter(|(_, (k, _))| k == cell) {
            let mut hits: Vec<(f64, &FormKey)> = Vec::new();
            for k in &r_off {
                let Some(i) = by_key.get(k) else { continue };
                let stored =
                    collision::instance_as_stored(i, &loader, &m.limits, options, &budget)?;
                let Some(placement) = stored.body_placement else {
                    continue;
                };
                let w = World::build(&stored, placement, &budget)?;
                if let Some(h) = w.ray_down(p[0], p[1], p[2]) {
                    hits.push((p[2] - h.z, k));
                }
            }
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            let _ = writeln!(
                census,
                "probe {} under the point, refused references placed as stored\t{}",
                pi + 1,
                hits.iter()
                    .map(|(d, k)| {
                        let i = by_key[k];
                        let kinds: BTreeSet<String> = i
                            .blockers
                            .iter()
                            .enumerate()
                            .filter(|(bk, e)| {
                                refusing(
                                    e,
                                    i.blocker_origins
                                        .get(*bk)
                                        .copied()
                                        .unwrap_or(Origin::Record),
                                )
                            })
                            .map(|(_, e)| e.code.clone())
                            .collect();
                        format!(
                            "{:.1} below: {} ({})",
                            d,
                            i.base
                                .as_ref()
                                .map_or("none".into(), |b| String::from_utf8_lossy(
                                    &b.record.signature
                                )
                                .into_owned()),
                            kinds.into_iter().collect::<Vec<_>>().join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let _ = writeln!(
                names,
                "probe {} {} {:?}: refused references under it: {}",
                pi + 1,
                tl::key_text(pk),
                p,
                hits.iter()
                    .map(|(d, k)| format!("{} {:.1} below", tl::key_text(k), d))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        for k in &r_off {
            if !seen.insert(k.clone()) {
                continue;
            }
            let Some(i) = by_key.get(k) else { continue };
            let stored = collision::instance_as_stored(i, &loader, &m.limits, options, &budget)?;
            let bearings = collision::bearings(i, &loader, &m.limits, options, &budget)?;
            let mut row = Row {
                key: k.clone(),
                base: i
                    .base
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(&b.record.signature).into_owned())
                    .unwrap_or_else(|| "none".into()),
                has_collision: !stored.instances.is_empty(),
                settled: !r_on.contains(k),
                by_chain: !r_chain.contains(k),
                blockers: Vec::new(),
                movers: BTreeSet::new(),
                models: BTreeSet::new(),
            };
            for model in &bearings.models {
                row.models.insert(model.path.clone());
            }
            for (bk, e) in i.blockers.iter().enumerate() {
                let origin = i.blocker_origins.get(bk).copied().unwrap_or(Origin::Record);
                if !refusing(e, origin) {
                    continue;
                }
                let b = bearings.of.get(bk).copied().flatten();
                row.blockers.push((
                    origin_name(origin).into(),
                    e.code.clone(),
                    kind_of(e),
                    bearing_name(b),
                ));
                if b == Some(Bearing::Moved) {
                    if let Some(asset) = bearings.assets.get(bk).copied().flatten() {
                        let model = bearings.models.iter().find(|x| x.asset == asset);
                        if let Some(model) = model {
                            let movers = model.reads.movers.clone();
                            let types =
                                loader.assets.with_decoded(asset, &budget.child(), |d| {
                                    let DecodedAsset::Model(n) = d else {
                                        return Ok(Vec::new());
                                    };
                                    Ok(movers
                                        .iter()
                                        .filter_map(|(b, link)| {
                                            n.blocks.get(*b).map(|x| {
                                                format!(
                                                    "{} by {link:?}",
                                                    n.block_types[x.type_index as usize]
                                                )
                                            })
                                        })
                                        .collect::<Vec<_>>())
                                })?;
                            row.movers.extend(types);
                        }
                    }
                }
            }
            rows.push(row);
        }
    }

    // Over every reference refused with the switch off, once each.
    let with: Vec<&Row> = rows.iter().filter(|r| r.has_collision).collect();
    let _ = writeln!(
        census,
        "\n## References refused, once each over the cell loads\n\
         refused (off)\t{}\n\
         of which their models place no collision\t{}\n\
         of which their models place collision (holes in the walking world)\t{}\n\
         of those, settled by the switches\t{}\n\
         of those, settled by the off-chain switch alone\t{}\n\
         of those, still refused with the switches\t{}",
        rows.len(),
        rows.len() - with.len(),
        with.len(),
        with.iter().filter(|r| r.settled).count(),
        with.iter().filter(|r| r.by_chain).count(),
        with.iter().filter(|r| !r.settled).count(),
    );
    let mut by_type: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    for r in &with {
        by_type.entry(r.base.clone()).or_default()[usize::from(!r.settled)] += 1;
    }
    let _ = writeln!(
        census,
        "\n### By base record type (with collision): settled, still refused"
    );
    for (t, [s, n]) in &by_type {
        let _ = writeln!(census, "{t}\t{s}\t{n}");
    }
    // Refusing blockers by kind: references counted once per kind.
    let mut kinds: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    for r in &with {
        let set: BTreeSet<String> = r
            .blockers
            .iter()
            .map(|(o, c, k, b)| format!("{b}\t{o}\t{c}\t{k}"))
            .collect();
        for k in set {
            kinds.entry(k).or_default()[usize::from(!r.settled)] += 1;
        }
    }
    let _ = writeln!(
        census,
        "\n### Refusing blockers (with collision), references per kind: bearing, origin, code, kind; settled, still refused"
    );
    for (k, [s, n]) in &kinds {
        let _ = writeln!(census, "{k}\t{s}\t{n}");
    }
    // Why the rest stay refused: the set of bearings of their refusing blockers
    // other than off-chain.
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    let mut movers: BTreeMap<String, usize> = BTreeMap::new();
    for r in with.iter().filter(|r| !r.settled) {
        let set: BTreeSet<String> = r
            .blockers
            .iter()
            .filter(|b| b.3 != "off-chain" && !b.2.starts_with("REFR.XALP."))
            .map(|(o, c, _, b)| {
                if *o == "model" {
                    format!("{b} {c}")
                } else {
                    format!("{o} {c}")
                }
            })
            .collect();
        tally(&mut why, set.into_iter().collect::<Vec<_>>().join(" + "));
        for t in &r.movers {
            tally(&mut movers, t.clone());
        }
    }
    let _ = writeln!(
        census,
        "\n### Still refused with the switches: what refuses them (references)"
    );
    for (k, n) in &why {
        let _ = writeln!(census, "{k}\t{n}");
    }
    let _ = writeln!(
        census,
        "\n### Block types moving a chain node in those models (references)"
    );
    for (k, n) in &movers {
        let _ = writeln!(census, "{k}\t{n}");
    }
    for r in &rows {
        let _ = writeln!(
            names,
            "{}\t{}\tcollision {}\tsettled {}\tmodels {}\tblockers {}\tmovers {}",
            tl::key_text(&r.key),
            r.base,
            r.has_collision,
            r.settled,
            r.models.iter().cloned().collect::<Vec<_>>().join(","),
            r.blockers
                .iter()
                .map(|(o, c, k, b)| format!("{b}/{o}/{c}/{k}"))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(" | "),
            r.movers.iter().cloned().collect::<Vec<_>>().join(","),
        );
    }

    // Animated references at load.
    for (di, d) in doors.iter().enumerate() {
        door(
            &store,
            &vfs,
            &m,
            d,
            di + 1,
            &mut census,
            &mut names,
            &budget,
        )?;
    }
    std::fs::write(out.join("collision-ready-census.txt"), census)?;
    std::fs::write(out.join("collision-ready-names.txt"), names)?;
    Ok(())
}

fn world_stats(c: &CellCollision) -> Result<(u64, u64), E> {
    let placement = c.body_placement.ok_or("no body placement level")?;
    let w = World::build(c, placement, &Budget::new(4 << 30))?;
    Ok((w.stats.triangles, w.stats.hulls))
}

/// What the data holds about one animated reference's collision at load.
#[allow(clippy::too_many_arguments)]
fn door(
    store: &Store,
    vfs: &fo4_vfs::Vfs,
    m: &Manifest,
    key: &FormKey,
    index: usize,
    census: &mut String,
    names: &mut String,
    budget: &Budget,
) -> Result<(), E> {
    let level = RuleLevel::Hypotheses;
    let mut loader = Loader::new(store, vfs, &m.language, level, budget);
    let i = loader.instance(key, budget)?;
    let options = Options {
        level,
        body_placement: None,
    };
    let _ = writeln!(census, "\n## Animated reference {index} at load");
    let _ = writeln!(names, "\nanimated reference {index}: {}", tl::key_text(key));
    let subs: Vec<String> = i
        .reference
        .record
        .subrecords
        .iter()
        .map(|s| String::from_utf8_lossy(&s.signature).into_owned())
        .collect();
    let _ = writeln!(
        census,
        "reference subrecords\t{}\nreference record flags\t{:#x}",
        subs.join(","),
        i.reference.record.flags
    );
    for sub in &i.reference.record.subrecords {
        if &sub.signature != b"VMAD" {
            continue;
        }
        let d = i.reference.record.data(sub);
        match fo4_plugin::vmad::Vmad::read(d, *b"REFR", "reference", sub.offset, budget) {
            Ok(v) => {
                let _ = writeln!(census, "reference scripts\t{}", v.scripts.len());
                for sc in &v.scripts {
                    let _ = writeln!(
                        names,
                        "script {} ({} properties)",
                        String::from_utf8_lossy(sc.name),
                        sc.properties.len()
                    );
                }
            }
            Err(e) => {
                let _ = writeln!(census, "reference scripts\tnot read ({})", e.code);
            }
        }
    }
    let Some(base) = i.base.as_ref() else {
        let _ = writeln!(census, "no base");
        return Ok(());
    };
    let bsubs: Vec<String> = base
        .record
        .subrecords
        .iter()
        .map(|s| String::from_utf8_lossy(&s.signature).into_owned())
        .collect();
    let _ = writeln!(
        census,
        "base record type\t{}\nbase subrecords\t{}\nbase record flags\t{:#x}\ncollision readiness\t{}",
        String::from_utf8_lossy(&base.record.signature),
        bsubs.join(","),
        base.record.flags,
        if i.collision.ready_at(level) { "ready" } else { "refused" }
    );
    let stored = collision::instance_as_stored(&i, &loader, &m.limits, options, budget)?;
    let _ = writeln!(
        census,
        "collision placed as stored\t{} instances from {} bodies",
        stored.instances.len(),
        stored.models.iter().map(|x| x.bodies.len()).sum::<usize>()
    );
    let Some(model) = base.common.model.path.clone() else {
        let _ = writeln!(census, "no model");
        return Ok(());
    };
    let _ = writeln!(names, "model {model}");
    let mk = format!("meshes/{}", model.replace('\\', "/")).to_ascii_lowercase();
    let bytes = vfs.read(&mk, &budget.child())?;
    let n = fo4_nif::Nif::read(&bytes, &mk, Game::Fallout4, &m.limits, &budget.child())?;
    let body_chains: Vec<(usize, Vec<usize>, Option<String>)> = stored
        .models
        .iter()
        .flat_map(|x| x.bodies.iter())
        .map(|b| (b.collision_object, b.chain.clone(), b.target_name.clone()))
        .collect();
    let name_of = |b: usize| -> Option<String> {
        let data = n.blocks.get(b)?.decoded.as_ref()?;
        fo4_scene::model_policy::fields(data)
            .iter()
            .find_map(|f| match (f.name, &f.value) {
                ("Name", fo4_nif::Value::StringIndex(s)) => n
                    .strings
                    .get(*s as usize)
                    .map(|x| String::from_utf8_lossy(x).into_owned()),
                _ => None,
            })
    };
    let anim = fo4_scene::anim_graph::nif_animation(&n);
    let _ = writeln!(
        census,
        "behaviour graph cited by the model\t{}",
        anim.graph_file.is_some()
    );
    match fo4_scene::anim_graph::model_graph(vfs, &model, &m.limits, budget) {
        Ok(g) => {
            let start = g.machine.states.iter().find(|s| s.id == g.machine.start);
            let seq = start.and_then(|s| s.sequence.clone());
            let pos = seq.as_ref().and_then(|x| {
                fo4_scene::sequence::sequences(&n)
                    .iter()
                    .position(|s| &s.name == x)
            });
            let _ = writeln!(
                census,
                "graph root machine\t{} states, start state generator {}, start state's sequence {}",
                g.machine.states.len(),
                start.map_or("none", |s| s.generator.as_str()),
                match (&seq, pos) {
                    (Some(_), Some(p)) => format!("the model's sequence {}", p + 1),
                    (Some(_), None) => "a name the model does not hold".into(),
                    (None, _) => "none".into(),
                }
            );
            let _ = writeln!(
                names,
                "graph start state {:?} sequence {:?}",
                start.map(|s| s.name.clone()),
                seq
            );
            // Every state: its sequence (the model's, by exact name or
            // ignoring case) and its transitions by event.
            let model_seqs = fo4_scene::sequence::sequences(&n);
            let index_of = |x: &str| -> String {
                if let Some(p) = model_seqs.iter().position(|s| s.name == x) {
                    format!("the model's sequence {}", p + 1)
                } else if let Some(p) = model_seqs
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(x))
                {
                    format!("the model's sequence {} ignoring case", p + 1)
                } else {
                    "a name the model does not hold".into()
                }
            };
            let state_index = |id: i32| g.machine.states.iter().position(|s| s.id == id);
            for (k, st) in g.machine.states.iter().enumerate() {
                let tr: Vec<String> = st
                    .transitions
                    .iter()
                    .map(|t| {
                        format!(
                            "event {} to state {}",
                            t.event,
                            state_index(t.to_state).map_or("?".into(), |x| (x + 1).to_string())
                        )
                    })
                    .collect();
                let _ = writeln!(
                    census,
                    "graph state {}{}\tsequence: {}; {}",
                    k + 1,
                    if st.id == g.machine.start {
                        " (start)"
                    } else {
                        ""
                    },
                    st.sequence.as_deref().map_or("none".into(), index_of),
                    if tr.is_empty() {
                        "no transitions".into()
                    } else {
                        tr.join(", ")
                    }
                );
                let _ = writeln!(
                    names,
                    "graph state {} {:?} sequence {:?}",
                    k + 1,
                    st.name,
                    st.sequence
                );
            }
            for t in &g.machine.wildcards {
                let _ = writeln!(
                    census,
                    "graph wildcard\tevent {} to state {}",
                    t.event,
                    state_index(t.to_state).map_or("?".into(), |x| (x + 1).to_string())
                );
            }
            for (k, e) in g.machine.events.iter().enumerate() {
                let _ = writeln!(names, "graph event {k}: {e}");
            }
        }
        Err(e) => {
            let _ = writeln!(census, "graph root machine\tnot read");
            let _ = writeln!(names, "graph: {e}");
        }
    }
    let seqs = fo4_scene::sequence::sequences(&n);
    let _ = writeln!(
        census,
        "controller sequences\t{}\ncollision bodies placed as stored\t{}",
        seqs.len(),
        body_chains.len()
    );
    for (si, s) in seqs.iter().enumerate() {
        let _ = writeln!(names, "sequence {}: {}", si + 1, s.name);
        let mut moved = 0usize;
        let mut notes: Vec<String> = Vec::new();
        // Each chain node the sequence controls through a transform
        // interpolator: its pose and its keys against the stored transform.
        let mut nodes: BTreeMap<usize, usize> = BTreeMap::new();
        for (_, chain, _) in &body_chains {
            for b in chain {
                *nodes.entry(*b).or_default() += 1;
            }
        }
        for (b, bodies) in &nodes {
            let Some(nm) = name_of(*b) else { continue };
            let Some(ip) = s
                .blocks
                .iter()
                .find(|(x, _)| *x == nm)
                .and_then(|(_, ip)| *ip)
            else {
                continue;
            };
            let Some(stored) = stored_of(&n, *b) else {
                continue;
            };
            let Some(f) = n.blocks.get(ip).and_then(|x| x.decoded.as_ref()) else {
                continue;
            };
            let f = fo4_scene::model_policy::fields(f);
            let floats = |name: &str| -> Vec<f64> {
                f.iter()
                    .filter(|x| x.name == name)
                    .filter_map(|x| match x.value {
                        fo4_nif::Value::FloatBits(v) => Some(f64::from(f32::from_bits(v))),
                        _ => None,
                    })
                    .collect()
            };
            let (t, q) = (floats("Translation"), floats("Quaternion"));
            let pose_t = (t.len() == 3 && t.iter().all(|v| v.abs() < 1e30))
                .then(|| distance(&stored.translation, &[t[0], t[1], t[2]]));
            let pose_r = (q.len() == 4 && q.iter().all(|v| v.abs() < 1e30))
                .then(|| angle(&stored.rotation, [q[0], q[1], q[2], q[3]]));
            let keys = fo4_scene::sequence::node_keys(&n, s, &nm, budget)?;
            let kt = keys.as_ref().and_then(|k| {
                let (a, z) = (k.translation.keys.first()?, k.translation.keys.last()?);
                let v = |x: [f32; 3]| [f64::from(x[0]), f64::from(x[1]), f64::from(x[2])];
                Some((
                    distance(&stored.translation, &v(a.value)),
                    distance(&stored.translation, &v(z.value)),
                    k.translation.keys.len(),
                ))
            });
            let kr = keys.as_ref().and_then(|k| match &k.rotation {
                Some(fo4_scene::clock::Rotation::Quaternion { keys, .. }) => {
                    let (a, z) = (keys.first()?, keys.last()?);
                    let w = |x: [f32; 4]| {
                        [
                            f64::from(x[0]),
                            f64::from(x[1]),
                            f64::from(x[2]),
                            f64::from(x[3]),
                        ]
                    };
                    Some(format!(
                        "rotation keys {:.1} then {:.1} degrees from stored ({} keys)",
                        angle(&stored.rotation, w(a.1)),
                        angle(&stored.rotation, w(z.1)),
                        keys.len()
                    ))
                }
                Some(fo4_scene::clock::Rotation::Xyz(_)) => Some("XYZ rotation keys".into()),
                None => None,
            });
            moved += bodies;
            notes.push(format!(
                "chain block {b} ({bodies} bodies): pose {}, {}; {}; {}",
                pose_t.map_or("no translation".into(), |d| format!(
                    "translation {d:.1} from stored"
                )),
                pose_r.map_or("no rotation".into(), |d| format!(
                    "rotation {d:.1} degrees from stored"
                )),
                kt.map_or("no translation keys".into(), |(a, z, k)| format!(
                    "translation keys {a:.1} then {z:.1} from stored ({k} keys)"
                )),
                kr.unwrap_or_else(|| "no rotation keys".into()),
            ));
        }
        let _ = writeln!(
            census,
            "sequence {}\tcycle {}, {:.3} s, {} controlled blocks, chain nodes controlled by a transform interpolator under {} body chain links",
            si + 1,
            s.cycle,
            f64::from(s.stop - s.start) / f64::from(s.frequency.max(f32::MIN_POSITIVE)),
            s.blocks.len(),
            moved
        );
        for x in notes {
            let _ = writeln!(census, "\t{x}");
        }
    }
    Ok(())
}

fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Degrees between a stored rotation and a quaternion (w, x, y, z), the
/// smaller of the two readings of the matrix's orientation.
fn angle(r: &[[f64; 3]; 3], q: [f64; 4]) -> f64 {
    let m =
        fo4_scene::collision::quaternion_rows([q[1] as f32, q[2] as f32, q[3] as f32, q[0] as f32]);
    let tr = |t: bool| -> f64 {
        (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| r[i][j] * if t { m[j][i] } else { m[i][j] })
            .sum()
    };
    let a = |x: f64| ((x - 1.) / 2.).clamp(-1., 1.).acos().to_degrees();
    a(tr(false)).min(a(tr(true)))
}

/// A node's stored transform.
fn stored_of(n: &fo4_nif::Nif<'_>, b: usize) -> Option<fo4_scene::transform::Transform> {
    let data = n.blocks.get(b)?.decoded.as_ref()?;
    fo4_scene::transform::Transform::from_fields(fo4_scene::model_policy::fields(data), "model")
        .ok()
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
