#![forbid(unsafe_code)]
//! A headless check of a moving platform in the walking world
//! (`fo4_walk::platform`, `spec/walk.md`, Platforms): the walking world of
//! an exterior cell and the cells about it as the play mode builds it
//! (placed collision and terrain), with the collision-marker boxes
//! (`walk_collision_marker_box_v1`) and the collision of one animated
//! reference placed as stored (`walk_animated_model_collision_as_stored_v1`)
//! when asked; a walker that walks toward points at given seconds, then
//! stands; from `--ride-from` the parts of the reference's collision under
//! a node move as a sequence moves that node (`walk_platform_sequence_ride_v1`),
//! carrying a walker standing on them. Writes the walker's track and a
//! summary into OUT_DIR (outside git). The sequence, node and seconds are
//! the caller's.
//!
//! `ride_check MANIFEST OUT_DIR --cell PLUGIN:HEX --start X,Y,Z [--toward S,X,Y ...]
//!  --end S [--walk-speed U] [--boxes] [--also PLUGIN:HEX ...] [--elevator PLUGIN:HEX --sequence N --node NAME
//!  --ride-from S [--ride-boxes]]`
//!
//! `--ride-boxes` makes the collision-marker boxes inside the moving node's
//! reach ride too (instead of the node's own collision alone).
use fo4_core::{Budget, Game, Manifest};
use fo4_plugin::typed::Store;
use fo4_scene::{timeline as tl, Loader, RuleLevel};
use fo4_walk::{platform, Character, Input, Params, Settings, World, WorldSet};
use std::{fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
/// (second, translation) points of a ride.
type Track = Vec<(f64, [f64; 3])>;

fn v3(s: &str) -> Result<Vec<f64>, E> {
    Ok(s.split(',').map(str::parse).collect::<Result<_, _>>()?)
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str = "ride_check MANIFEST OUT_DIR --cell PLUGIN:HEX --start X,Y,Z [--toward S,X,Y ...] --end S [--walk-speed U] [--boxes] [--also PLUGIN:HEX ...] [--elevator PLUGIN:HEX --sequence N --node NAME [--node NAME ...] --ride-from S [--ride-boxes]]";
    if a.len() < 2 {
        return Err(USAGE.into());
    }
    let (mut cell, mut start, mut end) = (None, None, None);
    let mut toward: Vec<(f64, f64, f64)> = Vec::new();
    let (mut boxes, mut ride_boxes) = (false, false);
    let (mut elevator, mut sequence, mut ride_from) = (None, None, None);
    let mut nodes: Vec<String> = Vec::new();
    let mut speed = None;
    let mut also: Vec<fo4_plugin::FormKey> = Vec::new();
    let mut it = a[2..].iter();
    while let Some(x) = it.next() {
        let mut val = || it.next().ok_or(USAGE);
        match x.as_str() {
            "--cell" => cell = tl::parse_key(val()?),
            "--start" => start = Some(v3(val()?)?),
            "--toward" => {
                let v = v3(val()?)?;
                toward.push((v[0], v[1], v[2]));
            }
            "--end" => end = Some(val()?.parse::<f64>()?),
            "--walk-speed" => speed = Some(val()?.parse::<f64>()?),
            "--boxes" => boxes = true,
            "--also" => also.extend(tl::parse_key(val()?)),
            "--ride-boxes" => ride_boxes = true,
            "--elevator" => elevator = tl::parse_key(val()?),
            "--sequence" => sequence = Some(val()?.parse::<usize>()?),
            "--node" => nodes.push(val()?.clone()),
            "--ride-from" => ride_from = Some(val()?.parse::<f64>()?),
            _ => return Err(USAGE.into()),
        }
    }
    let (Some(cell), Some(start), Some(end)) = (cell, start, end) else {
        return Err(USAGE.into());
    };
    let out = Path::new(&a[1]);
    std::fs::create_dir_all(out)?;
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let mut report = String::new();

    // The walking world as the play mode builds it.
    let t0 = std::time::Instant::now();
    let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
    let mut scene = loader.cell(&cell, &budget)?;
    let around = fo4_scene::grid::around(&store, &scene.cell.key, 1, &budget)?;
    fo4_scene::grid::absorb_around(&mut loader, &mut scene, &around, &budget)?;
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, &m.limits, options, &budget)?;
    let placement = c.body_placement.ok_or("no body placement level")?;
    let mut base = World::build(&c, placement, &budget)?;
    let mut ground = fo4_walk::terrain::load_with(&store, &around, level, &budget)?;
    ground.add_to(&mut base, &budget)?;
    let _ = writeln!(
        report,
        "walking world: {} triangles, {} hulls from {} instances, {:.1} s",
        base.stats.triangles,
        base.stats.hulls,
        base.stats.instances,
        t0.elapsed().as_secs_f64()
    );
    let mut set: WorldSet<u32> = WorldSet::new();
    set.insert(0, base)?;

    // Other references whose collision is placed as stored (still).
    for (ai, k) in also.iter().enumerate() {
        let inst = loader.instance(k, &budget)?;
        let ac =
            fo4_scene::collision::instance_as_stored(&inst, &loader, &m.limits, options, &budget)?;
        let aw = World::build(&ac, placement, &budget)?;
        let _ = writeln!(
            report,
            "also {}: {} collision instances placed as stored ({} not placed)",
            tl::key_text(k),
            ac.instances.len(),
            ac.unplaced.len()
        );
        set.insert(10 + ai as u32, aw)?;
    }
    // The animated reference's collision as stored, split by the nodes the
    // caller names (a source goes with the first named node in its chain).
    let mut moving: Vec<(World, Track)> = Vec::new();
    let mut node_reach: Option<([f64; 3], [f64; 3])> = None;
    if let Some(ek) = &elevator {
        let inst = loader.instance(ek, &budget)?;
        let ec =
            fo4_scene::collision::instance_as_stored(&inst, &loader, &m.limits, options, &budget)?;
        let ew = World::build(&ec, placement, &budget)?;
        if let Some(bx) = ew.bounds {
            node_reach = Some((bx.min, bx.max));
        }
        let model = inst
            .base
            .as_ref()
            .and_then(|b| b.common.model.path.clone())
            .ok_or("the reference's base has no model")?;
        let place = inst.reference.data.transform.ok_or("no placement")?;
        let frame =
            fo4_scene::transform::Transform::euler(place, inst.reference.data.scale.unwrap_or(1.))?;
        let mk = format!("meshes/{}", model.replace('\\', "/")).to_ascii_lowercase();
        let bytes = vfs.read(&mk, &budget.child())?;
        let n = fo4_nif::Nif::read(&bytes, &mk, Game::Fallout4, &m.limits, &budget.child())?;
        let seqs = fo4_scene::sequence::sequences(&n);
        let s = seqs
            .get(sequence.ok_or("--sequence")?)
            .ok_or("no such sequence")?;
        let from = ride_from.ok_or("--ride-from")?;
        let span = f64::from(s.stop - s.start) / f64::from(s.frequency);
        let blocks: Vec<usize> = nodes
            .iter()
            .map(|x| {
                fo4_scene::sequence::node(&n, x)
                    .map(|v| v.0)
                    .ok_or("a node is not in the model")
            })
            .collect::<Result<_, _>>()?;
        let mut groups: Vec<Vec<u32>> = vec![Vec::new(); nodes.len() + 1];
        for (si, src) in ew.sources.iter().enumerate() {
            let g = src.instance.and_then(|k| {
                let i = &ec.instances[k];
                let chain = &ec.model(i).bodies[i.body].chain;
                blocks.iter().position(|nb| chain.contains(nb))
            });
            groups[g.unwrap_or(nodes.len())].push(si as u32);
        }
        let _ = writeln!(
            report,
            "animated reference: {} collision instances placed as stored ({} not placed), {} sources under the named nodes ({}), {} elsewhere",
            ec.instances.len(),
            ec.unplaced.len(),
            groups[..nodes.len()].iter().map(Vec::len).sum::<usize>(),
            groups[..nodes.len()].iter().map(|g| g.len().to_string()).collect::<Vec<_>>().join(", "),
            groups[nodes.len()].len()
        );
        set.insert(1, ew.copy_sources(&groups[nodes.len()], [0.; 3], &budget)?)?;
        for (ni, name) in nodes.iter().enumerate() {
            let (_, stored) =
                fo4_scene::sequence::node(&n, name).ok_or("a node is not in the model")?;
            let keys = fo4_scene::sequence::node_keys(&n, s, name, &budget)?
                .ok_or("the sequence does not move a named node")?;
            // The node's translation from its stored one: before the ride
            // it stands where the ride's sequence starts (the parts wait
            // at the start of their ride), then as the sequence moves it;
            // model axes turned into the world by the placement.
            let origin = frame.apply([0.; 3]);
            let mut track = Vec::new();
            let steps = (span * 60.).ceil() as usize;
            for k in 0..=steps {
                let dt = (k as f64 / 60.).min(span);
                let st = f64::from(s.start) + dt * f64::from(s.frequency);
                let v = keys.translation.sample(st).ok_or("keys not sampled")?;
                let local: [f64; 3] = std::array::from_fn(|i| f64::from(v[i] - stored[i]));
                let w = frame.apply(local);
                track.push((from + dt, std::array::from_fn(|i| w[i] - origin[i])));
            }
            let (a, z) = (track[0].1, track[track.len() - 1].1);
            let _ = writeln!(
                report,
                "node {ni}: {} sources; from its stored place moved by {:.2} {:.2} {:.2} before the ride, {:.2} {:.2} {:.2} at its end ({span:.3} s from {from} s)",
                groups[ni].len(),
                a[0], a[1], a[2], z[0], z[1], z[2]
            );
            moving.push((ew.copy_sources(&groups[ni], [0.; 3], &budget)?, track));
        }
    }
    // Collision-marker boxes about the start, or about the animated reference's collision.
    let mut rides: Vec<platform::Ride<u32>> = Vec::new();
    if boxes {
        let (lo, hi) = match node_reach {
            Some((lo, hi)) => (
                [lo[0] - 1000., lo[1] - 1000., lo[2] - 1000.],
                [hi[0] + 1000., hi[1] + 1000., hi[2] + 1000.],
            ),
            None => (
                [start[0] - 2000., start[1] - 2000., start[2] - 2000.],
                [start[0] + 2000., start[1] + 2000., start[2] + 2000.],
            ),
        };
        let found = platform::marker_boxes(&store, lo, hi, &budget)?;
        let mut bw = World::new();
        let srcs = platform::add_boxes(&mut bw, &found, &budget)?;
        let _ = writeln!(report, "collision-marker boxes: {}", srcs.len());
        for mb in &found {
            let _ = writeln!(
                report,
                "  box {}: centre {:.1} {:.1} {:.1}, half {:?}, enable parent {}, initially disabled {}",
                tl::key_text(&mb.reference),
                mb.placement[0],
                mb.placement[1],
                mb.placement[2],
                mb.half,
                mb.enable_parent,
                mb.initially_disabled
            );
        }
        if ride_boxes && !moving.is_empty() {
            // The boxes ride as the first named node does, from where they
            // stand.
            let track = &moving[0].1;
            let first = track[0].1;
            let rel: Vec<(f64, [f64; 3])> = track
                .iter()
                .map(|(t, v)| (*t, std::array::from_fn(|i| v[i] - first[i])))
                .collect();
            rides.push(platform::Ride::new(&mut set, 4, bw, rel, 0., &budget)?);
        } else {
            set.insert(2, bw)?;
        }
    }
    for (k, (mw, track)) in moving.into_iter().enumerate() {
        rides.push(platform::Ride::new(
            &mut set,
            5 + k as u32,
            mw,
            track,
            0.,
            &budget,
        )?);
    }
    for (k, w, _) in set.pieces() {
        if let Some(bx) = w.bounds {
            let _ = writeln!(
                report,
                "piece {k}: {} sources, bounds {:.1} {:.1} {:.1} to {:.1} {:.1} {:.1}",
                w.sources.len(),
                bx.min[0],
                bx.min[1],
                bx.min[2],
                bx.max[0],
                bx.max[1],
                bx.max[2]
            );
        }
    }

    // The walker.
    let mut params = Params::PROVISIONAL;
    if let Some(s) = speed {
        params.walk_speed = s;
    }
    let settings = Settings::DEFAULT;
    let dt = settings.tick;
    let mut w = Character::new(params, settings, [start[0], start[1], start[2]])?;
    w.settle(&set, 600);
    let mut t = toward.first().map_or(0., |x| x.0);
    let mut lines = String::from("# SECONDS X Y Z GROUNDED GROUND_PIECE CARRIED\n");
    let mut aim: Option<(f64, f64)> = None;
    let mut next = 0;
    let mut min_z = w.feet[2];
    let mut carried = 0.;
    let mut log_at = t;
    while t < end {
        while next < toward.len() && toward[next].0 <= t + 1e-9 {
            aim = Some((toward[next].1, toward[next].2));
            next += 1;
        }
        let mut input = Input::default();
        if let Some((x, y)) = aim {
            let (dx, dy) = (x - w.feet[0], y - w.feet[1]);
            if dx.hypot(dy) > params.walk_speed * dt * 0.5 {
                input.forward = 1.;
                input.heading = dy.atan2(dx);
            } else {
                aim = None;
            }
        }
        for r in &mut rides {
            carried += r.advance(&mut set, &mut w, t + dt, &budget)?;
        }
        let s = w.tick(&set, input);
        t += dt;
        min_z = min_z.min(s.feet[2]);
        if t >= log_at - 1e-9 {
            let piece = s
                .ground
                .and_then(|g| set.source(g.source))
                .map_or("-".into(), |(k, _)| k.to_string());
            let _ = writeln!(
                lines,
                "{t:.3} {:.2} {:.2} {:.2} {} {piece} {carried:.2}",
                s.feet[0], s.feet[1], s.feet[2], s.grounded
            );
            log_at += 0.25;
        }
    }
    let _ = writeln!(
        report,
        "walker at {end} s: feet {:.1} {:.1} {:.1}, grounded {}, lowest {:.1}, carried {:.1} in all",
        w.feet[0], w.feet[1], w.feet[2], w.grounded, min_z, carried
    );
    std::fs::write(out.join("ride-track.txt"), lines)?;
    std::fs::write(out.join("ride.txt"), &report)?;
    print!("{report}");
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
