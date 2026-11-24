#![forbid(unsafe_code)]
//! The headless play mode (`spec/play.md`): a scripted input drives the
//! walker over the start cell's collision, the walker's points and the
//! input's activations drive `quest_run new-game` slice by slice.
//!
//! ```text
//! play_run MANIFEST (INPUT | --replay STEP) OUT_DIR [play options] -- [new-game switches...]
//! ```
//!
//! The new-game switches are `quest_run`'s, `--track FILE` among them (its
//! first point is the start; its points choose the cells the world model
//! reads) and `--timeline FILE` for the timeline the input produced. Play
//! options: `--play-around N` (cells about the start whose collision the
//! walker uses, default 1), `--play-reach UNITS` (the activation's reach,
//! the caller's), `--play-pick-bounds` (the activation's ray also meets the
//! object bounds of references with no collision,
//! `play_activate_object_bounds_v1`), `--walk-speed UNITS` and `--run-speed UNITS` (the caller's
//! values of the walker's speed rules), `--play-tick SECONDS` (the walker's
//! fixed tick), `--play-slice SECONDS` (ticks joined into slices that long,
//! for speed: the run's outcome does not depend on it). `--replay STEP` gives the run its own track back in slices
//! of STEP seconds instead (the stepped loop's check). Writes `quest_run`'s
//! outputs to OUT_DIR and `play.txt` (what the walker did, the play counts)
//! and `play-track.txt` (the run's track as `--track` reads it).
//! `--play-live-world` makes the walking world follow the run's state
//! (`play_live_collision_follows_run_v1`; `--play-nowait` reads the NoWait
//! forms as the plain natives, as `--timeline-nowait` does for frames);
//! `--play-load-doors` crosses the load doors the activation meets
//! (`play_load_door_activation_v1`); `--play-region-walker` reads trigger
//! volumes and placed actors about the walker's points
//! (`play_region_around_walker_v1`); `--play-follow-grid` builds the
//! walking world again about the walker's grid square when it leaves it (a
//! policy of the play mode, as the viewer's moving grid).
//! `--play-pick-bounds-activatable` leaves out the object bounds of
//! statics (`play_activate_bounds_not_static_v1`). `--play-recheck-marks`
//! computes again the distance marks of scenes waiting on a function 1
//! condition when the track grows, instead of offering them their
//! completion again (a policy of the play mode: the stepped run then logs
//! no wait `quest_run` does not log).
//! `--play-elevator REF` (repeated) moves the collision of a reference,
//! placed as stored and split by the nodes its model's sequences move, as
//! the sequences the run's graph starts on it move those nodes
//! (`play_elevator_run_sequence_v1`); `--play-elevator-also REF` places
//! another reference's collision as stored, still, with the named reference
//! of its own space (else the first); `--play-elevator-also-with REF OWNER`
//! with the named reference OWNER whatever its space; `--play-elevator-door`
//! takes the walker through the nearest load door within the reach when a
//! ride that carried it ends (`play_elevator_end_load_door_v1`), and with
//! `--play-elevator-door-onward` not through a door back into the space
//! the walker came from (`play_elevator_end_door_onward_v1`);
//! `--play-player-moveto` puts the walker where a `MoveTo` the run calls on
//! the player with zero offsets puts it (`play_player_moveto_v1`), and
//! `--play-player-sits` at the furniture the run seats the player in
//! (`play_player_sit_furniture_v1`).
//! `--play-elevator-held-pose` moves a node a sequence controls without
//! translation keys to the translation the sequence holds it at
//! (`play_elevator_held_pose_v1`). `--play-elevator-rigid` moves each part
//! instead by the rigid motion the sequence gives the chain above it, every
//! channel read (`play_elevator_rigid_v1`). `--play-collision-off-chain` and
//! `--play-collision-reference-alpha` give every walking world's cell build
//! the drawing side's collision readiness readings
//! (`collision_model_blockers_off_chain_v1`,
//! `refr_xalp_collision_unchanged_v1`; `fo4_scene::readiness::Switches`).
//! `--play-collision-child-flag-agrees` gives them, and the live world's
//! decisions, the reading of enable chains left open only by own flags
//! that agree with the chain (`xesp_child_flag_agrees_chain_v1`).
//! `--probe-ray X Y Z HEADING PITCH REACH --probe-ref KEY` prints what a ray
//! from that eye meets and where the reference's collision is, then stops
//! (an aid for choosing where an input aims).
use fo4_core::{Budget, Manifest};
use fo4_play::{input::Script, walker, world, Driver, Replay};
use fo4_plugin::typed::Store;
use fo4_vfs::Vfs;
use std::fmt::Write as _;
use std::path::Path;

type E = Box<dyn std::error::Error>;

fn value<T: std::str::FromStr>(a: &mut Vec<String>, flag: &str) -> Result<Option<T>, E>
where
    T::Err: std::error::Error + 'static,
{
    let Some(i) = a.iter().position(|x| x == flag) else {
        return Ok(None);
    };
    let v = a
        .get(i + 1)
        .ok_or(format!("{flag} takes a value"))?
        .parse()?;
    a.drain(i..i + 2);
    Ok(Some(v))
}

fn track_text(points: &[fo4_play::Point]) -> String {
    let mut s = String::from("# the run's track in the play mode: SECONDS SPACE X Y Z\n");
    for p in points {
        let _ = writeln!(
            s,
            "{} {} {} {} {}",
            p.t,
            fo4_vm::bind::record_key(&p.space),
            p.pos[0],
            p.pos[1],
            p.pos[2]
        );
    }
    s
}

fn run() -> Result<(), E> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let usage = "play_run MANIFEST (INPUT | --replay STEP) OUT_DIR [--play-around N] [--play-pick-bounds] [--play-reach UNITS] [--walk-speed UNITS] [--run-speed UNITS] [--play-tick SECONDS] -- [new-game switches...]";
    let split = all.iter().position(|x| x == "--").ok_or(usage)?;
    let (mine, switches) = (all[..split].to_vec(), all[split + 1..].to_vec());
    let mut a = mine;
    let replay: Option<f64> = value(&mut a, "--replay")?;
    let around: u32 = value(&mut a, "--play-around")?.unwrap_or(1);
    let reach: Option<f64> = value(&mut a, "--play-reach")?;
    let walk_speed: Option<f64> = value(&mut a, "--walk-speed")?;
    let run_speed: Option<f64> = value(&mut a, "--run-speed")?;
    let tick: Option<f64> = value(&mut a, "--play-tick")?;
    let slice: Option<f64> = value(&mut a, "--play-slice")?;
    let flag = |a: &mut Vec<String>, f: &str| {
        let on = a.iter().any(|x| x == f);
        a.retain(|x| x != f);
        on
    };
    let pick_bounds = flag(&mut a, "--play-pick-bounds");
    let live_world = flag(&mut a, "--play-live-world");
    let nowait = flag(&mut a, "--play-nowait");
    let load_doors = flag(&mut a, "--play-load-doors");
    let region = flag(&mut a, "--play-region-walker");
    let recheck_marks = flag(&mut a, "--play-recheck-marks");
    let follow_grid = flag(&mut a, "--play-follow-grid");
    let activatable = flag(&mut a, "--play-pick-bounds-activatable");
    let elevator_door = flag(&mut a, "--play-elevator-door");
    let held_pose = flag(&mut a, "--play-elevator-held-pose");
    let rigid = flag(&mut a, "--play-elevator-rigid");
    let door_onward = flag(&mut a, "--play-elevator-door-onward");
    let player_moveto = flag(&mut a, "--play-player-moveto");
    let player_sits = flag(&mut a, "--play-player-sits");
    let collision = fo4_scene::readiness::Switches {
        off_chain: flag(&mut a, "--play-collision-off-chain"),
        reference_alpha: flag(&mut a, "--play-collision-reference-alpha"),
    };
    let child_flag_agrees = flag(&mut a, "--play-collision-child-flag-agrees");
    let mut elevators = walker::Elevators {
        door: elevator_door,
        held_pose,
        rigid,
        door_onward,
        ..walker::Elevators::default()
    };
    while let Some(i) = a
        .iter()
        .position(|x| x == "--play-elevator" || x == "--play-elevator-also")
    {
        let k = a
            .get(i + 1)
            .and_then(|v| fo4_vm::player::parse_key(v))
            .ok_or("--play-elevator and --play-elevator-also take PLUGIN:OBJECTID")?;
        if a[i] == "--play-elevator" {
            elevators.references.push(k);
        } else {
            elevators.also.push(k);
        }
        a.drain(i..i + 2);
    }
    while let Some(i) = a.iter().position(|x| x == "--play-elevator-also-with") {
        let usage = "--play-elevator-also-with takes PLUGIN:OBJECTID PLUGIN:OBJECTID";
        let key = |j: usize| a.get(i + j).and_then(|v| fo4_vm::player::parse_key(v));
        let (k, owner) = (key(1).ok_or(usage)?, key(2).ok_or(usage)?);
        if !elevators.references.contains(&owner) {
            return Err(
                "--play-elevator-also-with names an owner that is no --play-elevator reference"
                    .into(),
            );
        }
        elevators.also_with.push((k, owner));
        a.drain(i..i + 3);
    }
    if elevators.references.is_empty() && (elevator_door || !elevators.also.is_empty()) {
        return Err("--play-elevator-also and --play-elevator-door need --play-elevator".into());
    }
    if elevators.references.is_empty() && (held_pose || rigid) {
        return Err(
            "--play-elevator-held-pose and --play-elevator-rigid need --play-elevator".into(),
        );
    }
    if door_onward && !elevator_door {
        return Err("--play-elevator-door-onward needs --play-elevator-door".into());
    }
    if elevator_door && !(load_doors && reach.is_some()) {
        return Err("--play-elevator-door needs --play-load-doors and --play-reach".into());
    }
    if activatable && !pick_bounds {
        return Err("--play-pick-bounds-activatable needs --play-pick-bounds".into());
    }
    if nowait && !live_world {
        return Err("--play-nowait needs --play-live-world".into());
    }
    let probe: Vec<String> = match a.iter().position(|x| x == "--probe-ray") {
        Some(i) => {
            let end = a.len().min(i + 9);
            a.drain(i..end).collect()
        }
        None => Vec::new(),
    };
    let (manifest, input, out) = match (replay, a.as_slice()) {
        (Some(_), [m, o]) => (m.clone(), None, o.clone()),
        (None, [m, i, o]) => (m.clone(), Some(i.clone()), o.clone()),
        _ => return Err(usage.into()),
    };
    let track_file = switches
        .iter()
        .position(|x| x == "--track")
        .and_then(|i| switches.get(i + 1))
        .ok_or("the new-game switches need --track FILE (its first point is the start)")?;
    let track = fo4_vm::track::Track::parse(&std::fs::read_to_string(track_file)?)?;
    std::fs::create_dir_all(&out)?;
    let mut args = vec![manifest.clone(), "new-game".to_string(), out.clone()];
    args.extend(switches);
    let mut report = String::new();
    let started = std::time::Instant::now();
    let (driver, log): (Box<dyn Driver>, Option<_>) = match (replay, input) {
        (Some(step), _) => {
            let _ = writeln!(report, "replay: the run's own track in slices of {step} s");
            (Box::new(Replay::new(step)), None)
        }
        (None, Some(input)) => {
            let script = Script::parse(&std::fs::read_to_string(&input)?)?;
            let m = Manifest::load(Path::new(&manifest))?;
            let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
            let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
            let first = track.points.first().ok_or("an empty track")?;
            let b = Budget::new(m.limits.max_memory);
            // The track's space as the run resolves it.
            let mut resolved = track.clone();
            resolved.resolve(&store)?;
            let space = resolved.points[0].space.clone();
            let cell = match store.signature(&first.space) {
                Some(s) if s == *b"CELL" => first.space.clone(),
                _ => world::start_cell(&store, &space, first.pos, &b)?,
            };
            let built = world::build(
                &m,
                &store,
                &vfs,
                &cell,
                world::Options {
                    around,
                    live: live_world,
                    doors: load_doors,
                    collision,
                    child_flag_agrees,
                },
            )?;
            let world::Built {
                world,
                refs,
                boxes,
                report: text,
                live,
                doors,
                counts: built_counts,
                space: built_space,
                ..
            } = built;
            // --probe-ray X Y Z HEADING PITCH REACH: what the ray from that
            // eye meets, and the sources of the references the caller names
            // (--probe-ref KEY), then stop (an aid for choosing where to aim).
            let a = &probe;
            if let Some(i) = a.iter().position(|x| x == "--probe-ray") {
                let v: Vec<f64> = a
                    .get(i + 1..i + 7)
                    .ok_or("--probe-ray X Y Z HEADING PITCH REACH")?
                    .iter()
                    .map(|x| x.parse::<f64>())
                    .collect::<Result<_, _>>()?;
                let look = fo4_play::input::Controls {
                    heading: v[3],
                    pitch: v[4],
                    ..Default::default()
                }
                .look();
                let hit = fo4_play::pick::first_of(
                    &world,
                    &refs,
                    pick_bounds.then_some(&boxes),
                    [v[0], v[1], v[2]],
                    look,
                    v[5],
                    &|_| false,
                );
                println!("{text}");
                println!("probe ray from {:?} along {look:?}: {hit:?}", &v[..3]);
                for k in a.iter().skip_while(|x| *x != "--probe-ref").skip(1).take(1) {
                    let key = fo4_vm::player::parse_key(k).ok_or("--probe-ref PLUGIN:ID")?;
                    for (s, r) in refs.iter().enumerate() {
                        if r.as_ref() == Some(&key) {
                            let n = world
                                .entries
                                .iter()
                                .filter(|e| e.source as usize == s)
                                .count();
                            let b = world
                                .entries
                                .iter()
                                .filter(|e| e.source as usize == s)
                                .fold(None, |acc: Option<fo4_walk::geometry::Aabb>, e| {
                                    Some(acc.map_or(e.bounds, |x| x.union(e.bounds)))
                                });
                            println!("source {s} of {k}: {n} entries, bounds {b:?}");
                        }
                    }
                    for (r, f, lo, hi) in &boxes.items {
                        if *r == key {
                            println!(
                                "object bounds of {k}: low {lo:?} high {hi:?}, placed at {:?} scale {} rotation {:?}",
                                f.translation, f.scale, f.rotation
                            );
                        }
                    }
                }
                return Ok(());
            }
            let _ = writeln!(
                report,
                "start cell {} (around {around})",
                fo4_vm::bind::record_key(&cell)
            );
            let _ = writeln!(report, "{text}");
            if collision.off_chain {
                let _ = writeln!(report, "switch: a refused collision whose model blockers lie off the blocks its bodies read enters the walking world (--play-collision-off-chain, {})", fo4_scene::hypotheses::COLLISION_OFF_CHAIN);
            }
            if collision.reference_alpha {
                let _ = writeln!(report, "switch: a reference's unread alpha does not refuse its collision (--play-collision-reference-alpha, {})", fo4_scene::hypotheses::REFERENCE_ALPHA_COLLISION);
            }
            if child_flag_agrees {
                let _ = writeln!(report, "switch: an enable chain left open only by records' own flags that agree with it decides their collision, at load and after the run's changes (--play-collision-child-flag-agrees, {})", fo4_scene::hypotheses::ENABLE_CHILD_FLAG_AGREES);
            }
            drop((store, vfs));
            let mut opts = walker::Options {
                reach,
                collision,
                child_flag_agrees,
                ..walker::Options::default()
            };
            if let Some(v) = walk_speed {
                opts.params.walk_speed = v;
            }
            if let Some(v) = run_speed {
                opts.params.run_speed = v;
            }
            if let Some(v) = tick {
                opts.settings.tick = v;
            }
            if let Some(v) = slice {
                if !(v.is_finite() && v >= 0.0) {
                    return Err("--play-slice takes seconds, 0 or more".into());
                }
                opts.slice = v;
                let _ = writeln!(
                    report,
                    "walker slices joined to {v} s at least (--play-slice)"
                );
            }
            for (name, rule, v, origin) in opts.params.provenance() {
                let _ = writeln!(report, "walker {name} {v} ({rule}, {origin:?})");
            }
            let _ = writeln!(report, "walker tick {} s", opts.settings.tick);
            if let Some(r) = reach {
                let _ = writeln!(report, "activation reach {r} units (caller assumption)");
            }
            let mut w = match live {
                Some((dormant, info)) => {
                    let _ = writeln!(
                        report,
                        "switch: the walking world follows the run's state (--play-live-world{})",
                        if nowait { ", --play-nowait" } else { "" }
                    );
                    let live =
                        fo4_play::live::Live::new(world, refs, dormant, info, built_space, nowait)?
                            .with_child_flag_agrees(child_flag_agrees);
                    walker::Walker::new(fo4_walk::World::new(), Vec::new(), script, opts)?
                        .with_live(live, nowait)
                }
                None => walker::Walker::new(world, refs, script, opts)?,
            };
            if load_doors {
                let _ = writeln!(report, "switch: the activation crosses load doors (--play-load-doors): {} among the loaded references", doors.len());
                w = w.with_doors(walker::Doors { doors, around });
            }
            if player_moveto {
                let _ = writeln!(report, "switch: the walker goes where the run's MoveTo with zero offsets puts the player (--play-player-moveto)");
                w = w.with_player_moves();
            }
            if player_sits {
                let _ = writeln!(report, "switch: the walker goes to the furniture the run seats the player in (--play-player-sits)");
                w = w.with_player_sits();
            }
            if follow_grid {
                let _ = writeln!(report, "policy: the walking world is built again about the walker's grid square when it leaves it (--play-follow-grid, {around} cells about it)");
                w = w.with_grid(walker::Grid::new(around));
            }
            if region {
                let _ = writeln!(report, "switch: trigger volumes and placed actors are read about the walker's points (--play-region-walker)");
            }
            if !elevators.references.is_empty() {
                let _ = writeln!(report, "switch: the collision of {} named reference(s) moves as the sequences the run's graph starts on them move its nodes, {} other reference(s) placed as stored (--play-elevator, --play-elevator-also){}", elevators.references.len(), elevators.also.len() + elevators.also_with.len(), if elevators.door { "; a ride that carried the walker ends at the nearest load door within the reach (--play-elevator-door)" } else { "" });
                if elevators.held_pose {
                    let _ = writeln!(report, "switch: a node a sequence controls without translation keys moves to the translation the sequence holds it at (--play-elevator-held-pose)");
                }
                if elevators.door_onward {
                    let _ = writeln!(report, "switch: a ride's end leaves out the load doors back into the space the walker came from (--play-elevator-door-onward)");
                }
                if elevators.rigid {
                    let _ = writeln!(report, "switch: each part moves by the rigid motion the sequence gives the chain above it, every channel read, in place of the translations (--play-elevator-rigid)");
                }
                w = w.with_elevators(elevators.clone());
            }
            for (k, v) in &built_counts {
                let _ = writeln!(report, "{k}\t{v}");
            }
            if pick_bounds {
                let _ = writeln!(report, "switch: the activation's ray meets the object bounds of references with no collision (--play-pick-bounds)");
                let boxes = if activatable {
                    let kept = boxes.without_kinds(&fo4_play::rules::STATIC_KINDS);
                    let _ = writeln!(report, "switch: object bounds of statics, static collections and movable statics left out (--play-pick-bounds-activatable): {} of {} boxes kept", kept.items.len(), boxes.items.len());
                    w = w.with_static_boxes_left_out();
                    kept
                } else {
                    boxes
                };
                w = w.with_boxes(boxes);
            }
            let log = w.log.clone();
            (Box::new(w), Some(log))
        }
        _ => return Err(usage.into()),
    };
    let prepared = started.elapsed().as_secs_f64();
    if recheck_marks {
        let _ = writeln!(report, "policy: the distance marks of scenes waiting on a function 1 condition are computed again as the track grows, no completion offered (--play-recheck-marks)");
    }
    let (_, counts) = fo4_play::new_game_with(
        args,
        Some(driver),
        fo4_play::Options {
            region,
            recheck_marks,
        },
    )?;
    let _ = writeln!(
        report,
        "seconds: walk world {prepared:.2}, run {:.2}",
        started.elapsed().as_secs_f64() - prepared
    );
    for (k, v) in &counts {
        let _ = writeln!(report, "{k}\t{v}");
    }
    if let Some(log) = log {
        let l = log.borrow();
        if let Some((p, feet, ticks)) = &l.start {
            let _ = writeln!(
                report,
                "start: track point {:?} at {} s; walker settled in {ticks} ticks at {:?}",
                p.pos, p.t, feet
            );
        }
        let _ = writeln!(
            report,
            "walker: {} ticks, {} slices ({} standing), {} points, {} blocked, {} stepped, {} airborne",
            l.ticks, l.slices, l.standing_slices, l.points, l.blocked, l.stepped, l.airborne
        );
        let _ = writeln!(
            report,
            "cost: walker ticks {:.6} s in all, {:.3} us a tick",
            l.tick_seconds,
            1e6 * l.tick_seconds / (l.ticks.max(1) as f64)
        );
        for (t, feet) in &l.arrivals {
            let _ = writeln!(report, "toward: arrived at {t} s at {feet:?}");
        }
        if let Some(s) = &l.live {
            let _ = writeln!(
                report,
                "live world: {} applies of the run's state, {} references entered (or came back), {} left, {} moved; {:.6} s in all, longest {:.6} s",
                s.applies, s.entered, s.left, s.moved, s.seconds, s.longest
            );
        }
        for (t, cell, feet, secs) in &l.grid {
            let _ = writeln!(
                report,
                "grid: at {t} s the walking world built again about cell {} (feet {feet:?}) in {secs:.2} s",
                fo4_vm::bind::record_key(cell)
            );
        }
        for d in l
            .doors
            .iter()
            .filter(|d| d.rule != fo4_play::rules::LOAD_DOOR)
        {
            let _ = writeln!(
                report,
                "player {} followed at {} s: to {} in {}: cell {}, position {:?}, feet settled at {:?}; walking world built in {:.2} s\n{}",
                if d.rule == fo4_play::rules::PLAYER_SIT { "sit" } else { "MoveTo" },
                d.t,
                fo4_vm::bind::record_key(&d.to.to),
                fo4_vm::bind::record_key(&d.to.space),
                fo4_vm::bind::record_key(&d.cell),
                d.to.pos,
                d.feet,
                d.seconds,
                d.report
            );
        }
        for (t, why) in &l.player_moves {
            let _ = writeln!(report, "player place at {t} s: {why}");
        }
        for d in l
            .doors
            .iter()
            .filter(|d| d.rule == fo4_play::rules::LOAD_DOOR)
        {
            let _ = writeln!(
                report,
                "load door at {} s: {} to {} in {}: cell {}, teleport {:?}, feet settled at {:?}; walking world built in {:.2} s\n{}",
                d.t,
                fo4_vm::bind::record_key(&d.door),
                fo4_vm::bind::record_key(&d.to.to),
                fo4_vm::bind::record_key(&d.to.space),
                fo4_vm::bind::record_key(&d.cell),
                d.to.pos,
                d.feet,
                d.seconds,
                d.report
            );
        }
        for (t, x) in &l.elevators {
            let _ = writeln!(report, "elevator at {t} s: {x}");
        }
        for r in &l.rules {
            let _ = writeln!(report, "rule relied on\t{r}");
        }
        for (t, h, p) in &l.looks {
            let _ = writeln!(report, "look at {t} s: heading {h} pitch {p}");
        }
        for (t, eye, dir, what) in &l.activations {
            let what = match what {
                Ok((k, d)) => format!("{} at {d:.1} units", fo4_vm::bind::record_key(k)),
                Err(why) => why.clone(),
            };
            let _ = writeln!(
                report,
                "activate at {t} s from {eye:?} along {dir:?}: {what}"
            );
        }
        let mut points: Vec<fo4_play::Point> = track
            .points
            .iter()
            .filter(|p| p.t <= 0.0)
            .cloned()
            .collect();
        points.extend(l.track.iter().cloned());
        std::fs::write(Path::new(&out).join("play-track.txt"), track_text(&points))?;
    }
    std::fs::write(Path::new(&out).join("play.txt"), report)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
