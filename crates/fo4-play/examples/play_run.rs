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
//! fixed tick). `--replay STEP` gives the run its own track back in slices
//! of STEP seconds instead (the stepped loop's check). Writes `quest_run`'s
//! outputs to OUT_DIR and `play.txt` (what the walker did, the play counts)
//! and `play-track.txt` (the run's track as `--track` reads it).
//! `--probe-ray X Y Z HEADING PITCH REACH --probe-ref KEY` prints what a ray
//! from that eye meets and where the reference's collision is, then stops
//! (an aid for choosing where an input aims).
use fo4_core::{Budget, Manifest};
use fo4_play::{input::Script, walker, Driver, Replay};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use std::fmt::Write as _;
use std::path::Path;

type E = Box<dyn std::error::Error>;

/// The cell under a point: the interior cell itself, or the exterior cell
/// of the worldspace whose grid square holds it.
fn start_cell(store: &Store, space: &FormKey, pos: [f64; 3], b: &Budget) -> Result<FormKey, E> {
    match store.signature(space) {
        Some(s) if s == *b"CELL" => Ok(space.clone()),
        Some(s) if s == *b"WRLD" => {
            let g = [
                (pos[0] / 4096.0).floor() as i32,
                (pos[1] / 4096.0).floor() as i32,
            ];
            for c in store.cells() {
                if store.world(&c).as_ref() != Some(space) {
                    continue;
                }
                if fo4_scene::grid::grid_of(store, &c, &b.child())? == Some(g) {
                    return Ok(c);
                }
            }
            Err(format!("PlayStartCell: no exterior cell at grid {g:?}").into())
        }
        _ => Err("PlayStartCell: the track's space names no winning CELL or WRLD".into()),
    }
}

/// The walking world, the placed reference of each of its sources, the
/// object bounds of the references with no collision, and a report.
type WalkWorld = (
    fo4_walk::World,
    Vec<Option<FormKey>>,
    fo4_play::pick::Boxes,
    String,
);

/// The walking world of the start cell and the cells about it, with the
/// placed reference of each source.
fn walk_world(
    m: &Manifest,
    store: &Store,
    vfs: &Vfs,
    cell: &FormKey,
    around: u32,
) -> Result<WalkWorld, E> {
    let level = RuleLevel::Hypotheses;
    let limits = &m.limits;
    let budget = Budget::new(8 << 30).with_scope_limit(limits.max_memory);
    let mut loader = Loader::new(store, vfs, &m.language, level, &budget);
    let mut scene = loader.cell(cell, &budget)?;
    let mut grid = None;
    if around > 0 {
        let a = fo4_scene::grid::around(store, &scene.cell.key, around, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &a, &budget)?;
        grid = Some(a);
    }
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, limits, options, &budget)?;
    let placement = c
        .body_placement
        .ok_or("PlayWalkWorld: no body placement level")?;
    let mut world = fo4_walk::World::build(&c, placement, &budget)?;
    let refs_before = fo4_play::pick::references(&world, &c);
    let mut ground = match &grid {
        Some(a) => fo4_walk::terrain::load_with(store, a, level, &budget)?,
        None => fo4_walk::terrain::load_around(store, &scene.cell.key, level, true, &budget)?,
    };
    ground.add_to(&mut world, &budget)?;
    // Terrain sources come after the cell's: no reference.
    let mut refs = refs_before;
    refs.resize(world.sources.len(), None);
    // The object bounds of the placed references with no collision in the
    // world (used under --play-pick-bounds).
    let (boxes, no_bounds) = fo4_play::pick::object_bounds(&scene, &refs);
    let why: std::collections::BTreeMap<String, usize> =
        c.unplaced.iter().fold(Default::default(), |mut m, u| {
            *m.entry(u.code.clone()).or_default() += 1;
            m
        });
    let report = format!(
        "walk world: {} triangles, {} hulls, {} spheres, {} capsules from {} instances ({} not placed); {} sources, {} with a placed reference",
        world.stats.triangles,
        world.stats.hulls,
        world.stats.spheres,
        world.stats.capsules,
        world.stats.instances,
        c.unplaced.len(),
        refs.len(),
        refs.iter().filter(|r| r.is_some()).count()
    ) + &format!(
        "\nreferences with no collision in the world and object bounds: {} ({} without usable bounds); collision not placed, by code: {why:?}; placed models with no collision object: {}",
        boxes.items.len(),
        no_bounds,
        c.without_collision.len()
    );
    Ok((world, refs, boxes, report))
}

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
    let pick_bounds = a.iter().any(|x| x == "--play-pick-bounds");
    a.retain(|x| x != "--play-pick-bounds");
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
                _ => start_cell(&store, &space, first.pos, &b)?,
            };
            let (world, refs, boxes, text) = walk_world(&m, &store, &vfs, &cell, around)?;
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
                }
                return Ok(());
            }
            let _ = writeln!(
                report,
                "start cell {} (around {around})",
                fo4_vm::bind::record_key(&cell)
            );
            let _ = writeln!(report, "{text}");
            drop((store, vfs));
            let mut opts = walker::Options {
                reach,
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
            for (name, rule, v, origin) in opts.params.provenance() {
                let _ = writeln!(report, "walker {name} {v} ({rule}, {origin:?})");
            }
            let _ = writeln!(report, "walker tick {} s", opts.settings.tick);
            if let Some(r) = reach {
                let _ = writeln!(report, "activation reach {r} units (caller assumption)");
            }
            let mut w = walker::Walker::new(world, refs, script, opts)?;
            if pick_bounds {
                let _ = writeln!(report, "switch: the activation's ray meets the object bounds of references with no collision (--play-pick-bounds)");
                w = w.with_boxes(boxes);
            }
            let log = w.log.clone();
            (Box::new(w), Some(log))
        }
        _ => return Err(usage.into()),
    };
    let prepared = started.elapsed().as_secs_f64();
    let (_, counts) = fo4_play::new_game(args, Some(driver))?;
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
