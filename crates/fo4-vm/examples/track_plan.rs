#![forbid(unsafe_code)]
//! Writes a player track for `quest_run new-game --track` (`spec/vm.md`,
//! Player position) from a plan of waypoints, walking between them along
//! the navigation meshes with `fo4_nav`.
//!
//! ```text
//! track_plan MANIFEST SPACE PLAN OUT_TRACK SPEED [LIFT]
//! ```
//!
//! SPACE is a worldspace or an interior cell as `plugin.esm:0001ABCD`.
//! Each PLAN line is `SECONDS X Y Z` (`#` starts a comment): the player
//! stands at that point at that second. Between two waypoints the player
//! waits at the first and leaves so as to reach the second at its second,
//! walking at SPEED units per second along the path `NavGraph` finds over
//! the space's navigation meshes (edge links of kind 0 followed; for a
//! worldspace, the meshes of its cells within one grid square of the
//! plan's points), pulled tight by its funnel. Every point written is
//! lifted LIFT units (default 1) above the mesh's plane under it. A
//! waypoint that cannot be reached in time is reached late and reported.
//! The plan, the speed and the lift are the caller's assumptions about
//! the player's movement; nothing here is game behaviour.
use fo4_core::{Budget, Manifest};
use fo4_nav::{path::height_in, NavGraph, Navmesh, NavmeshRecord};
use fo4_plugin::{
    typed::{Cell, Store},
    FormKey,
};
use std::{collections::HashMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;

struct Loaded {
    key: FormKey,
    plugin: usize,
    mesh: Navmesh,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if !(5..=6).contains(&a.len()) {
        return Err("track_plan MANIFEST SPACE PLAN OUT_TRACK SPEED [LIFT]".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let space = fo4_vm::player::parse_key(&a[1]).ok_or("SPACE as plugin.esm:0001ABCD")?;
    let speed: f64 = a[4].parse()?;
    let lift: f64 = a.get(5).map(|s| s.parse()).transpose()?.unwrap_or(1.0);
    if !(speed.is_finite() && speed > 0.0) {
        return Err("SPEED must be positive".into());
    }
    let mut plan: Vec<(f64, [f64; 3])> = Vec::new();
    for (n, line) in std::fs::read_to_string(&a[2])?.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let w: Vec<f64> = line
            .split_whitespace()
            .map(|x| x.parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("plan line {}: numbers expected", n + 1))?;
        let [t, x, y, z] = w[..] else {
            return Err(format!("plan line {}: SECONDS X Y Z", n + 1).into());
        };
        plan.push((t, [x, y, z]));
    }
    if plan.is_empty() {
        return Err("an empty plan".into());
    }
    // The cells whose meshes are read.
    let b = Budget::new(m.limits.max_memory);
    let mut cells = Vec::new();
    if store.signature(&space) == Some(*b"CELL") {
        cells.push(space.clone());
    } else {
        let lo = [0, 1].map(|i| plan.iter().map(|p| p.1[i]).fold(f64::MAX, f64::min));
        let hi = [0, 1].map(|i| plan.iter().map(|p| p.1[i]).fold(f64::MIN, f64::max));
        let g0 = lo.map(|x| (x / fo4_vm::track::CELL_SIZE).floor() as i64 - 1);
        let g1 = hi.map(|x| (x / fo4_vm::track::CELL_SIZE).floor() as i64 + 1);
        for c in store.cells() {
            if store.world(&c).as_ref() != Some(&space) {
                continue;
            }
            let cb = b.child();
            let near = store
                .get::<Cell>(&c, &cb)?
                .and_then(|x| x.data.grid)
                .is_some_and(|g| {
                    (g0[0]..=g1[0]).contains(&i64::from(g.i))
                        && (g0[1]..=g1[1]).contains(&i64::from(g.j))
                });
            if near {
                cells.push(c);
            }
        }
    }
    let mut meshes = Vec::new();
    for c in &cells {
        let cb = b.child();
        for kid in store
            .children(c, &cb)?
            .excluded
            .iter()
            .filter(|k| k.signature == *b"NAVM")
        {
            let rb = b.child();
            let Some((r, plugin)) = store.raw_with_plugin(&kid.key, &rb)? else {
                continue;
            };
            let n = NavmeshRecord::decode(&r, &rb)?;
            meshes.push(Loaded {
                key: kid.key.clone(),
                plugin,
                mesh: n.mesh,
            });
        }
    }
    let index: HashMap<&FormKey, usize> = meshes
        .iter()
        .enumerate()
        .map(|(i, l)| (&l.key, i))
        .collect();
    let resolve = |mi: usize, id: u32| {
        let l: &Loaded = &meshes[mi];
        let k = store
            .order()
            .remap(l.plugin, id, &l.key.plugin, 0)
            .ok()
            .flatten()?;
        index.get(&k).copied()
    };
    let g = NavGraph::build(
        meshes.iter().map(|l| &l.mesh).collect(),
        resolve,
        |k| k == 0,
        &b,
    )?;
    let ground = |p: [f64; 3]| -> f64 {
        g.locate(p)
            .and_then(|n| g.meshes()[n.mesh as usize].corners(usize::from(n.triangle)))
            .and_then(|c| height_in(&c, p[0], p[1]))
            .unwrap_or(p[2])
    };
    // A waypoint off the meshes moves to the nearest triangle centroid
    // within 128 units of height (reported).
    let snap = |p: [f64; 3]| -> [f64; 3] {
        if g.locate(p).is_some() {
            return p;
        }
        let mut best: Option<([f64; 3], f64)> = None;
        for m in g.meshes() {
            for t in 0..m.triangles.len() {
                let Some(c) = m.corners(t) else { continue };
                let q: [f64; 3] =
                    std::array::from_fn(|i| c.iter().map(|v| f64::from(v[i])).sum::<f64>() / 3.0);
                if (q[2] - p[2]).abs() > 128.0 {
                    continue;
                }
                let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((q, d));
                }
            }
        }
        match best {
            Some((q, d)) => {
                eprintln!(
                    "waypoint {p:?} is off the meshes: moved {d:.1} units to a triangle centroid"
                );
                q
            }
            None => p,
        }
    };
    let lifted = |p: [f64; 3]| [p[0], p[1], ground(p) + lift];
    let key = fo4_vm::bind::record_key(&space);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# track_plan: {} meshes, speed {speed} units/s, lift {lift}; SECONDS SPACE X Y Z",
        meshes.len()
    );
    let mut now = plan[0].0;
    let mut at = lifted(snap(plan[0].1));
    let _ = writeln!(out, "{now:.3} {key} {:.2} {:.2} {:.2}", at[0], at[1], at[2]);
    for (t, p) in plan.iter().skip(1) {
        let goal = lifted(snap(*p));
        let c = g
            .find_path(at, goal, &b)
            .map_err(|e| format!("no path to the waypoint at {t} s: {e:?}"))?;
        let pts: Vec<[f64; 3]> = c.points.iter().map(|q| lifted(*q)).collect();
        let length: f64 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
        let leave = (t - length / speed).max(now);
        if leave > now {
            let _ = writeln!(
                out,
                "{leave:.3} {key} {:.2} {:.2} {:.2}",
                at[0], at[1], at[2]
            );
        }
        let mut clock = leave;
        for w in pts.windows(2) {
            clock += dist(w[0], w[1]) / speed;
            let q = w[1];
            let _ = writeln!(out, "{clock:.3} {key} {:.2} {:.2} {:.2}", q[0], q[1], q[2]);
        }
        if clock > *t + 1e-6 {
            eprintln!("waypoint at {t} s reached late, at {clock:.3} s ({length:.1} units)");
        }
        now = clock.max(*t);
        if now > clock {
            let _ = writeln!(
                out,
                "{now:.3} {key} {:.2} {:.2} {:.2}",
                goal[0], goal[1], goal[2]
            );
        }
        at = goal;
    }
    std::fs::write(&a[3], out)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
