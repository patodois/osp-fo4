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
//! `--play-collision-static-fields` gives them the reading of a static's
//! reference fields `XACT`, `XLRT`, `XOWN` and `ONAM`
//! (`refr_static_fields_collision_unchanged_v1`).
//! `--play-collision-child-flag-agrees` gives them, and the live world's
//! decisions, the reading of enable chains left open only by own flags
//! that agree with the chain (`xesp_child_flag_agrees_chain_v1`).
//! The play options are read by the library (`fo4_play::options::Play`, the
//! window's scripted run reads them there too), which also builds the
//! walker and writes the switch lines of `play.txt`.
//! `--probe-ray X Y Z HEADING PITCH REACH --probe-ref KEY` prints what a ray
//! from that eye meets and where the reference's collision is, then stops
//! (an aid for choosing where an input aims).
use fo4_core::{Budget, Manifest};
use fo4_play::{input::Script, options::Play, Driver, Replay};
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

/// `--probe-ray X Y Z HEADING PITCH REACH [--probe-ref KEY]`: what the ray
/// from that eye meets in the walking world `built`, and the sources of
/// the reference the caller names (an aid for choosing where to aim).
fn probe(a: &[String], built: &fo4_play::world::Built, bounds: bool) -> Result<(), E> {
    let (world, refs, boxes) = (&built.world, &built.refs, &built.boxes);
    let v: Vec<f64> = a
        .get(1..7)
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
        world,
        refs,
        bounds.then_some(boxes),
        [v[0], v[1], v[2]],
        look,
        v[5],
        &|_| false,
    );
    println!("{}", built.report);
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
    Ok(())
}

fn run() -> Result<(), E> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let usage = "play_run MANIFEST (INPUT | --replay STEP) OUT_DIR [--play-around N] [--play-pick-bounds] [--play-reach UNITS] [--walk-speed UNITS] [--run-speed UNITS] [--play-tick SECONDS] -- [new-game switches...]";
    let split = all.iter().position(|x| x == "--").ok_or(usage)?;
    let (mine, switches) = (all[..split].to_vec(), all[split + 1..].to_vec());
    let mut a = mine;
    // The probe aid's arguments (numbers and a key) first.
    let probe_args: Vec<String> = match a.iter().position(|x| x == "--probe-ray") {
        Some(i) => {
            let end = a.len().min(i + 9);
            a.drain(i..end).collect()
        }
        None => Vec::new(),
    };
    let (play, mut a) = Play::parse(&a)?;
    let replay: Option<f64> = value(&mut a, "--replay")?;
    play.check()?;
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
    let args = play.run_args(args);
    let mut report = String::new();
    let started = std::time::Instant::now();
    let (driver, log): (Box<dyn Driver>, Option<_>) = match (replay, input) {
        (Some(step), _) => {
            let _ = writeln!(report, "replay: the run's own track in slices of {step} s");
            play.run_lines(&mut report);
            (Box::new(Replay::new(step)), None)
        }
        (None, Some(input)) => {
            let script = Script::parse(&std::fs::read_to_string(&input)?)?;
            let m = Manifest::load(Path::new(&manifest))?;
            let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
            let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
            let built = play.build_world(&m, &store, &vfs, &track)?;
            if !probe_args.is_empty() {
                return probe(&probe_args, &built, play.pick_bounds);
            }
            drop((store, vfs));
            let w = play.walker(built, script, &mut report)?;
            let log = w.log.clone();
            (Box::new(w), Some(log))
        }
        _ => return Err(usage.into()),
    };
    let prepared = started.elapsed().as_secs_f64();
    let (_, counts) = fo4_play::new_game_with(args, Some(driver), play.run_options())?;
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
        fo4_play::report::walker(&l, &mut report);
        let points = fo4_play::report::track_points(&track, &l);
        std::fs::write(
            Path::new(&out).join("play-track.txt"),
            fo4_play::report::track_text(&points),
        )?;
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
