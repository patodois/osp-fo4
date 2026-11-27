#![forbid(unsafe_code)]
//! What stands under a stretch of a cell's walking world, for finding why a
//! walker falls (a report of the play mode, no rule): the walking world of
//! CELL built with the play options as `play_run` builds it, the surfaces
//! each column along the line from (X, Y) to (X2, Y2) meets (in the world
//! as loaded and in the live world's dormant piece), the world's sources
//! near the line, and every placed reference of the scene whose object
//! bounds come within RADIUS of the line, with whether its collision is in
//! the world and, when it is not, why.
//!
//! ```text
//! column_probe MANIFEST CELL X Y Z X2 Y2 RADIUS [play options]
//! column_probe MANIFEST --platform REF SEQUENCE SECONDS
//! ```
//!
//! CELL may also be `WORLDSPACE@` (a worldspace's key and `@`): the cell of
//! that worldspace whose grid square holds (X, Y).
//!
//! The second form prints each rigid part of a named platform
//! (`play_elevator_rigid_v1`): its collision box as stored, at SECONDS of
//! SEQUENCE and at the last key of each sequence.
//!
//! Prints record keys and positions: for notes, never versioned.
use fo4_core::{Budget, Manifest};
use fo4_play::options::Play;
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::geometry::Aabb;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type E = Box<dyn std::error::Error>;

fn key(k: &FormKey) -> String {
    fo4_vm::bind::record_key(k)
}

/// The world box of a placed reference's object bounds.
fn placed_box(i: &fo4_scene::Instance) -> Option<Aabb> {
    let b = i.base.as_ref()?.common.bounds?;
    let t = i.reference.data.transform?;
    let f =
        fo4_scene::transform::Transform::euler(t, i.reference.data.scale.unwrap_or(1.0)).ok()?;
    let lo = [b[0], b[1], b[2]].map(f64::from);
    let hi = [b[3], b[4], b[5]].map(f64::from);
    let mut out: Option<Aabb> = None;
    for k in 0..8 {
        let pick = |n: usize| if k & (1 << n) != 0 { hi[n] } else { lo[n] };
        let p = f.apply([pick(0), pick(1), pick(2)]);
        let a = Aabb { min: p, max: p };
        out = Some(out.map_or(a, |o| o.union(a)));
    }
    out
}

/// Horizontal distance from a box to the segment from `a` to `b`, sampled.
fn near_line(bx: &Aabb, a: [f64; 2], b: [f64; 2], r: f64) -> bool {
    (0..=64).any(|k| {
        let t = k as f64 / 64.0;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        (0..2).all(|n| p[n] >= bx.min[n] - r && p[n] <= bx.max[n] + r)
    })
}

/// `column_probe MANIFEST --platform REF SEQUENCE SECONDS`: each rigid part
/// of a named platform (`play_elevator_rigid_v1`), its collision box as
/// stored, at SECONDS of SEQUENCE and at each sequence's last key.
fn platform(a: &[String]) -> Result<(), E> {
    let [manifest, _, reference, seq, at] = a else {
        return Err("column_probe MANIFEST --platform REF SEQUENCE SECONDS".into());
    };
    let at: f64 = at.parse()?;
    let m = Manifest::load(Path::new(manifest))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let k = fo4_vm::player::parse_key(reference).ok_or("REF")?;
    let readings = fo4_play::elevator::Readings {
        held_pose: false,
        rigid: true,
    };
    let e = fo4_play::elevator::Elevator::build(&m, &store, &vfs, &k, &[], 0, readings)?;
    let b = Budget::new(1 << 30);
    let rounded = |w: &fo4_walk::World| {
        w.bounds
            .map(|x| (x.min.map(|v| v.round()), x.max.map(|v| v.round())))
    };
    for (n, p) in e.parts.iter().enumerate() {
        let base = p.base();
        let all: Vec<u32> = (0..base.sources.len() as u32).collect();
        println!("part {n} (block {}): stored {:?}", p.block, rounded(base));
        if let Some(t) = p.tracks.get(seq.as_str()) {
            let m = fo4_play::elevator::motion_at(t, at);
            let w = base.copy_sources_moved(&all, &m, &b)?;
            println!("    {seq} at {at} s: {:?}", rounded(&w));
        }
        for (name, t) in &p.tracks {
            if let Some((last, m)) = t.last() {
                let w = base.copy_sources_moved(&all, m, &b)?;
                println!("    {name} last key {last:.2} s: {:?}", rounded(&w));
            }
        }
    }
    Ok(())
}

fn run() -> Result<(), E> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.get(1).map(String::as_str) == Some("--platform") {
        return platform(&all);
    }
    let usage = "column_probe MANIFEST CELL X Y Z X2 Y2 RADIUS [play options]";
    let (play, rest) = Play::parse(&all)?;
    if rest.len() != 8 {
        return Err(usage.into());
    }
    let n: Vec<f64> = rest[2..]
        .iter()
        .map(|x| x.parse::<f64>())
        .collect::<Result<_, _>>()?;
    let (a, z, b, r) = ([n[0], n[1]], n[2], [n[3], n[4]], n[5]);
    let m = Manifest::load(Path::new(&rest[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let cell = match rest[1].strip_suffix('@') {
        Some(space) => {
            let space = fo4_vm::player::parse_key(space).ok_or(usage)?;
            let b = Budget::new(m.limits.max_memory);
            fo4_play::world::start_cell(&store, &space, [a[0], a[1], z], &b)?
        }
        None => fo4_vm::player::parse_key(&rest[1]).ok_or(usage)?,
    };
    println!("cell {}", key(&cell));
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let built = fo4_play::world::build(&m, &store, &vfs, &cell, play.world_options())?;
    println!("{}", built.report);
    for (k, v) in &built.counts {
        println!("count\t{k}\t{v}");
    }
    let world = &built.world;
    let label = |w: &fo4_walk::World, refs: &[Option<FormKey>], s: u32| {
        let src = &w.sources[s as usize];
        format!(
            "source {s} ref {} label {:?} class {:?}",
            refs.get(s as usize)
                .cloned()
                .flatten()
                .map(|k| key(&k))
                .unwrap_or_else(|| "-".into()),
            src.label,
            src.class
        )
    };
    // COLUMN_PROBE_WALK=SPEED: a character standing at the line's start
    // (settled from Z) walks the line at SPEED with the probe of the cues
    // (`Character::walks_to`) and, tick by tick, at the walker's own tick,
    // under the controller settings the play options give.
    if let Some(speed) = std::env::var("COLUMN_PROBE_WALK")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        let params = fo4_walk::Params {
            walk_speed: speed,
            run_speed: speed,
            ..fo4_walk::Params::PROVISIONAL
        };
        // The controller's settings as the play options give them (the
        // step-up switch among them).
        let settings = play.walker_options().settings;
        let mut c = fo4_walk::Character::new(params, settings, [a[0], a[1], z])?;
        c.settle(world, 600);
        let to = world
            .ray_down(b[0], b[1], c.feet[2] + 100.0)
            .map_or(z, |h| h.z);
        println!(
            "\n# walk from {:?} to [{}, {}, {to:.2}]: walks_to {}",
            c.feet,
            b[0],
            b[1],
            c.walks_to(world, [b[0], b[1], to])
        );
        let heading = (b[1] - c.feet[1]).atan2(b[0] - c.feet[0]);
        let input = fo4_walk::Input {
            forward: 1.0,
            right: 0.0,
            heading,
            run: true,
        };
        // COLUMN_PROBE_TICKS=N ticks (12 by default).
        let ticks = std::env::var("COLUMN_PROBE_TICKS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(12);
        for k in 0..ticks {
            let s = c.tick(world, input);
            println!(
                "tick {k}: feet [{:.2}, {:.2}, {:.2}] grounded {} blocked {} stepped {}{}",
                s.feet[0],
                s.feet[1],
                s.feet[2],
                s.grounded,
                s.blocked,
                s.stepped,
                s.wall
                    .map(|w| format!(
                        " wall {} normal {:?}",
                        label(world, &built.refs, w.source),
                        w.normal.map(|v| (v * 1000.0).round() / 1000.0)
                    ))
                    .unwrap_or_default()
            );
        }
    }
    // COLUMN_PROBE_TRIS=R: every triangle of the walking world whose box
    // comes within R of the line's start (X, Y, Z), with its source.
    if let Some(reach) = std::env::var("COLUMN_PROBE_TRIS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        println!("\n# triangles within {reach} of [{}, {}, {z}]", a[0], a[1]);
        let p = [a[0], a[1], z];
        for e in &world.entries {
            let fo4_walk::world::Prim::Triangle(t) = e.prim else {
                continue;
            };
            let d = (0..3)
                .map(|n| {
                    (e.bounds.min[n] - p[n])
                        .max(p[n] - e.bounds.max[n])
                        .max(0.0)
                })
                .fold(0.0f64, |s, v| s.hypot(v));
            if d <= reach {
                println!(
                    "triangle {} {:?}",
                    label(world, &built.refs, e.source),
                    t.map(|q| q.map(|v| (v * 100.0).round() / 100.0))
                );
            }
        }
    }
    let steps = 40;
    println!("\n# columns from {a:?} to {b:?}: every surface (z, source)");
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        let hits = world.surfaces(x, y);
        let down = world.ray_down(x, y, z + 100.0);
        println!(
            "column {x:.1} {y:.1}: ray down from {:.1}: {}",
            z + 100.0,
            down.map(|h| format!("z {:.2} {}", h.z, label(world, &built.refs, h.source)))
                .unwrap_or_else(|| "NOTHING".into())
        );
        for h in hits {
            println!(
                "    surface z {:.2} normal {:?} {}",
                h.z,
                h.normal.map(|v| (v * 1000.0).round() / 1000.0),
                label(world, &built.refs, h.source)
            );
        }
        if let Some((piece, _)) = &built.live {
            for h in piece.world.surfaces(x, y) {
                println!(
                    "    dormant surface z {:.2} {}",
                    h.z,
                    label(&piece.world, &piece.refs, h.source)
                );
            }
        }
    }
    // The sources whose entries come near the line.
    let mut near: BTreeMap<u32, Aabb> = BTreeMap::new();
    for e in &world.entries {
        let bx = e.bounds;
        if bx.min[2] > z + r || bx.max[2] < z - 4.0 * r {
            continue;
        }
        if near_line(&bx, a, b, r) {
            near.entry(e.source)
                .and_modify(|o| *o = o.union(bx))
                .or_insert(bx);
        }
    }
    println!("\n# world sources near the line");
    for (s, bx) in &near {
        println!(
            "{} bounds {:?} {:?}",
            label(world, &built.refs, *s),
            bx.min.map(|v| v.round()),
            bx.max.map(|v| v.round())
        );
    }
    // The scene again, as the build reads it, for the references near the
    // line and why their collision is or is not in the world.
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
    let wo = play.world_options();
    if wo.collision.any() {
        loader.set_collision_switches(wo.collision);
    }
    if wo.child_flag_agrees {
        loader.set_collision_child_flag_agrees(true);
    }
    let mut scene = loader.cell(&cell, &budget)?;
    // As the walking world's build reads them: the cells about it and,
    // with `--play-collision-persistent`, the worldspace's persistent
    // references in the loaded squares.
    let mut joined_from = None;
    if wo.around > 0 {
        let a = fo4_scene::grid::around(&store, &scene.cell.key, wo.around, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        joined_from = Some(scene.instances.len());
        if wo.persistent && a.searched {
            let j = fo4_scene::grid::absorb_persistent(&mut loader, &mut scene, &a, &budget)?;
            println!("\n# persistent references joined: {j:?}");
        }
    }
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, &m.limits, options, &budget)?;
    let in_world: BTreeSet<FormKey> = built.refs.iter().flatten().cloned().collect();
    let mut why: BTreeMap<&FormKey, Vec<String>> = BTreeMap::new();
    for u in &c.unplaced {
        why.entry(&u.reference)
            .or_default()
            .push(format!("{}: {}", u.code, u.detail));
    }
    let without: BTreeSet<&FormKey> = c.without_collision.iter().map(|w| &w.reference).collect();
    println!("\n# placed references whose object bounds come within {r} of the line");
    let show = |i: &fo4_scene::Instance, what: &str| {
        let Some(bx) = placed_box(i) else { return };
        if bx.min[2] > z + r || bx.max[2] < z - 4.0 * r || !near_line(&bx, a, b, r) {
            return;
        }
        let k = &i.reference.key;
        let sig = i
            .base
            .as_ref()
            .map(|b| String::from_utf8_lossy(&b.record.signature).into_owned())
            .unwrap_or_default();
        let model = i
            .base
            .as_ref()
            .and_then(|b| b.common.model.path.clone())
            .unwrap_or_default();
        println!(
            "{what} {} base {} {sig} model {model:?} box {:?} {:?}\n    in world {} no collision object {} enable {:?} collision {:?}\n    render {:?} editor marker {}\n    unplaced {:?}\n    blockers {:?}",
            key(k),
            i.base.as_ref().map(|b| key(&b.key)).unwrap_or_default(),
            bx.min.map(|v| v.round()),
            bx.max.map(|v| v.round()),
            in_world.contains(k),
            without.contains(k),
            i.enable.as_ref().map(|p| p.enabled),
            i.collision,
            i.state,
            i.editor_marker,
            why.get(k),
            i.blockers
                .iter()
                .zip(&i.blocker_origins)
                .map(|(e, o)| format!("{} {:?} {:?} ({o:?})", e.code, e.found, e.source))
                .collect::<Vec<_>>()
        );
    };
    for i in &scene.instances {
        show(i, "instance");
    }
    // COLUMN_PROBE_PERSISTENT=1: every persistent reference the build
    // joined (`--play-collision-persistent`), one line each: base and
    // signature, model, whether its collision is in the walking world, the
    // drawing's readiness as the loader gives it (no draw switch of the
    // window applied), editor marker, enable state and stored place.
    if std::env::var_os("COLUMN_PROBE_PERSISTENT").is_some() {
        println!("\n# persistent\tref\tbase\tsig\tmodel\tin world\trender\teditor marker\tenabled\tplace\tblockers");
        for i in &scene.instances[joined_from.unwrap_or(scene.instances.len())..] {
            let base = i.base.as_ref();
            let render = match &i.state {
                fo4_scene::State::Blocked => "blocked".to_string(),
                s => format!("{s:?}")
                    .split([' ', '{'])
                    .next()
                    .unwrap_or("")
                    .to_string(),
            };
            let codes: BTreeSet<String> = i
                .blockers
                .iter()
                .map(|e| {
                    format!(
                        "{} {}",
                        e.code,
                        e.found.chars().take(40).collect::<String>()
                    )
                })
                .collect();
            println!(
                "persistent\t{}\t{}\t{}\t{:?}\t{}\t{render}\t{}\t{:?}\t{:?}\t{}",
                key(&i.reference.key),
                base.map(|b| key(&b.key)).unwrap_or_default(),
                base.map(|b| String::from_utf8_lossy(&b.record.signature).into_owned())
                    .unwrap_or_default(),
                base.and_then(|b| b.common.model.path.clone())
                    .unwrap_or_default(),
                in_world.contains(&i.reference.key),
                i.editor_marker,
                i.enable.as_ref().map(|p| p.enabled),
                i.reference
                    .data
                    .transform
                    .map(|t| [t[0], t[1], t[2]].map(|v| v.round())),
                codes.into_iter().collect::<Vec<_>>().join("; "),
            );
        }
    }
    // COLUMN_PROBE_CENSUS=SIGNATURES (comma separated, for example
    // FURN,MSTT): every placement of those bases whose object bounds meet
    // the rectangle from (X, Y) to (X2, Y2) between Z - 4 RADIUS and
    // Z + RADIUS, one line each: whether its model stores collision (the
    // bodies its model places as stored, `collision::instance_as_stored`),
    // whether that collision is in the walking world, and why not.
    if let Ok(sigs) = std::env::var("COLUMN_PROBE_CENSUS") {
        let sigs: Vec<&str> = sigs.split(',').collect();
        let (lo, hi) = (
            [a[0].min(b[0]), a[1].min(b[1])],
            [a[0].max(b[0]), a[1].max(b[1])],
        );
        println!("\n# census\tref\tbase\tsig\tmodel\tsize\tstored collision\tin world\twhy not");
        for i in &scene.instances {
            let Some(base) = i.base.as_ref() else {
                continue;
            };
            let sig = String::from_utf8_lossy(&base.record.signature).into_owned();
            if !sigs.contains(&sig.as_str()) {
                continue;
            }
            let Some(bx) = placed_box(i) else { continue };
            if bx.max[0] < lo[0] || bx.min[0] > hi[0] || bx.max[1] < lo[1] || bx.min[1] > hi[1] {
                continue;
            }
            if bx.min[2] > z + r || bx.max[2] < z - 4.0 * r {
                continue;
            }
            let k = &i.reference.key;
            let o = fo4_scene::collision::Options {
                level,
                body_placement: c.body_placement,
            };
            let stored =
                match fo4_scene::collision::instance_as_stored(i, &loader, &m.limits, o, &budget) {
                    Ok(one) if !one.instances.is_empty() => {
                        format!("yes ({} bodies)", one.instances.len())
                    }
                    Ok(one) if !one.without_collision.is_empty() => {
                        "no (no collision object)".to_string()
                    }
                    Ok(one) => format!(
                        "no ({} unplaced: {:?})",
                        one.unplaced.len(),
                        one.unplaced
                            .iter()
                            .map(|u| u.code.clone())
                            .collect::<BTreeSet<_>>()
                    ),
                    Err(e) => format!("unread ({e})"),
                };
            let size = [0, 1, 2].map(|n| (bx.max[n] - bx.min[n]).round());
            println!(
                "census\t{}\t{}\t{sig}\t{:?}\t{size:?}\t{stored}\t{}\t{}",
                key(k),
                key(&base.key),
                base.common.model.path.clone().unwrap_or_default(),
                in_world.contains(k),
                if in_world.contains(k) {
                    String::new()
                } else if without.contains(k) {
                    "no collision object in its model".to_string()
                } else {
                    let codes: BTreeSet<String> = i
                        .blockers
                        .iter()
                        .map(|e| format!("{} {:?}", e.code, e.found))
                        .collect();
                    format!(
                        "{}; blockers {}",
                        why.get(k)
                            .map(|v| v.join("; "))
                            .unwrap_or_else(|| "-".into()),
                        codes.into_iter().collect::<Vec<_>>().join(", ")
                    )
                }
            );
        }
    }
    // Each door near the line split into its still part and the parts its
    // sequences move, each moving part posed at the last key of each
    // sequence: the surfaces under the line in every pose.
    for i in &scene.instances {
        let is_door = i.base.as_ref().map(|b| b.record.signature) == Some(*b"DOOR");
        let Some(bx) = placed_box(i) else { continue };
        if !is_door || !near_line(&bx, a, b, r) {
            continue;
        }
        let Some(placement) = c.body_placement else {
            continue;
        };
        let Ok(Some(still)) =
            fo4_play::elevator::open_door_collision(&loader, i, &m.limits, placement, &budget)
        else {
            println!(
                "\n# door {}: no part its Open sequence moves",
                key(&i.reference.key)
            );
            continue;
        };
        println!(
            "\n# door {} split: {} moving parts, {} moved by Open",
            key(&i.reference.key),
            still.moving,
            still.opened
        );
        for (n, (w, tracks)) in still.parts.iter().enumerate() {
            for (name, track) in tracks {
                let Some((t, last)) = track.last() else {
                    continue;
                };
                let moved = w.copy_sources_moved(
                    &(0..w.sources.len() as u32).collect::<Vec<_>>(),
                    last,
                    &budget,
                )?;
                println!(
                    "    part {n} sequence {name:?} last key {t:.2} s: bounds {:?} (stored {:?})",
                    moved
                        .bounds
                        .map(|x| (x.min.map(|v| v.round()), x.max.map(|v| v.round()))),
                    w.bounds
                        .map(|x| (x.min.map(|v| v.round()), x.max.map(|v| v.round())))
                );
            }
        }
    }
    // The collision of each door near the line as its model stores it
    // (`collision::instance_as_stored`: every node at its stored pose):
    // what it would put under the line.
    for i in &scene.instances {
        let is_door = i.base.as_ref().map(|b| b.record.signature) == Some(*b"DOOR");
        let Some(bx) = placed_box(i) else { continue };
        if !is_door || in_world.contains(&i.reference.key) || !near_line(&bx, a, b, r) {
            continue;
        }
        let o = fo4_scene::collision::Options {
            level,
            body_placement: c.body_placement,
        };
        let one = match fo4_scene::collision::instance_as_stored(i, &loader, &m.limits, o, &budget)
        {
            Ok(one) => one,
            Err(e) => {
                println!("door {} as stored: {e:?}", key(&i.reference.key));
                continue;
            }
        };
        let Some(placement) = one.body_placement else {
            continue;
        };
        let w = fo4_walk::World::build(&one, placement, &budget)?;
        println!(
            "\n# door {} as stored: {} instances, {} unplaced {:?}; {} triangles {} hulls; bounds {:?}",
            key(&i.reference.key),
            one.instances.len(),
            one.unplaced.len(),
            one.unplaced.iter().map(|u| u.code.clone()).collect::<Vec<_>>(),
            w.stats.triangles,
            w.stats.hulls,
            w.bounds
        );
        for (n, ci) in one.instances.iter().enumerate() {
            let mut bb: Option<Aabb> = None;
            for e in w.entries.iter().filter(|e| e.source as usize == n) {
                bb = Some(bb.map_or(e.bounds, |o| o.union(e.bounds)));
            }
            println!(
                "    body {} leaf {} {:?}: bounds {:?}",
                ci.body,
                ci.leaf,
                ci.kind,
                bb.map(|x| (x.min.map(|v| v.round()), x.max.map(|v| v.round())))
            );
        }
        for e in w.entries.iter().take(96) {
            if let fo4_walk::world::Prim::Triangle(t) = e.prim {
                println!(
                    "    triangle of {}: {:?}",
                    e.source,
                    t.map(|p| p.map(|v| (v * 10.0).round() / 10.0))
                );
            }
        }
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
            let hits: Vec<String> = w
                .surfaces(x, y)
                .iter()
                .map(|h| format!("{:.1}@{}", h.z, h.source))
                .collect();
            println!("    column {x:.1} {y:.1}: {}", hits.join(" "));
        }
    }
    for child in &scene.disabled {
        if let Ok(i) = loader.instance(&child.key, &budget.child()) {
            show(&i, "initially disabled");
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
