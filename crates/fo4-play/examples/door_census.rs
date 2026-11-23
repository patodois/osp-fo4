#![forbid(unsafe_code)]
//! Census of what the data says about the placed doors of a cell and the
//! cells about it, for the play mode's doors (`spec/play.md`, Doors):
//! their collision under this project's rules (placed, or why not), the
//! subrecords and header flag bits of the placed references and of their
//! door records, the stored values of the door record's `FNAM`, and the
//! load doors (`XTEL`) among them. Counts only: no names.
//!
//! ```text
//! door_census MANIFEST CELL_KEY [AROUND] [--list-load-doors]
//! ```
//!
//! `--list-load-doors` also prints each load door's reference, the door its
//! `XTEL` names, that door's space and the teleport position to standard
//! error (for notes; never versioned).
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

type E = Box<dyn std::error::Error>;

fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let list = a.iter().any(|x| x == "--list-load-doors");
    a.retain(|x| x != "--list-load-doors");
    let usage = "door_census MANIFEST CELL_KEY [AROUND]";
    let (manifest, cell) = match a.as_slice() {
        [m, c] | [m, c, _] => (m, c),
        _ => return Err(usage.into()),
    };
    let around: u32 = a.get(2).map(|x| x.parse()).transpose()?.unwrap_or(1);
    let m = Manifest::load(Path::new(manifest))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    // CELL_KEY, or SPACE@X,Y: the cell under a point of a worldspace.
    let cell = match cell.split_once('@') {
        Some((space, at)) => {
            let space = fo4_vm::player::parse_key(space).ok_or(usage)?;
            let v: Vec<f64> = at.split(',').map(str::parse).collect::<Result<_, _>>()?;
            let p = [
                v.first().copied().unwrap_or(0.0),
                v.get(1).copied().unwrap_or(0.0),
                0.0,
            ];
            fo4_play::world::start_cell(&store, &space, p, &Budget::new(m.limits.max_memory))?
        }
        None => fo4_vm::player::parse_key(cell).ok_or(usage)?,
    };
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, level, &budget);
    let mut scene = loader.cell(&cell, &budget)?;
    if around > 0 {
        let g = fo4_scene::grid::around(&store, &scene.cell.key, around, &budget)?;
        fo4_scene::grid::absorb_around(&mut loader, &mut scene, &g, &budget)?;
    }
    let options = fo4_scene::collision::Options {
        level,
        body_placement: None,
    };
    let c = fo4_scene::collision::cell(&scene, &loader, &m.limits, options, &budget)?;
    let placed: BTreeSet<&FormKey> = c.placements.iter().map(|p| &p.reference).collect();
    let mut unplaced: BTreeMap<&FormKey, BTreeSet<String>> = BTreeMap::new();
    for u in &c.unplaced {
        unplaced
            .entry(&u.reference)
            .or_default()
            .insert(u.code.clone());
    }
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut bump = |k: String| *counts.entry(k).or_default() += 1;
    let mut doors: BTreeSet<FormKey> = BTreeSet::new();
    let b = Budget::new(m.limits.max_memory);
    for i in &scene.instances {
        let Some(base) = &i.base else { continue };
        if store.signature(&base.key) != Some(*b"DOOR") {
            continue;
        }
        let k = &i.reference.key;
        doors.insert(base.key.clone());
        bump("1 placed doors (loaded instances)".into());
        let outcome = if placed.contains(k) {
            "collision placed".to_string()
        } else if let Some(codes) = unplaced.get(k) {
            format!(
                "collision not placed: {}",
                codes.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        } else {
            "no collision object in its model".into()
        };
        bump(format!("2 collision\t{outcome}"));
        let blockers: BTreeSet<&str> = i.blockers.iter().map(|e| e.code.as_str()).collect();
        for code in &blockers {
            bump(format!("2 collision\tblocker of the instance\t{code}"));
        }
        let collision = match &i.collision {
            fo4_scene::State::Ready => "ready",
            fo4_scene::State::ReadyMeasured { .. } => "ready (measured rules)",
            fo4_scene::State::ReadyUnderHypotheses { .. } => "ready under pending rules",
            fo4_scene::State::Blocked => "blocked",
            fo4_scene::State::Outside => "outside",
        };
        bump(format!("2 collision readiness\t{collision}"));
        let Some(r) = store.raw(k, &b.child())? else {
            continue;
        };
        let subs: BTreeSet<String> = r.subrecords.iter().map(|s| sig(s.signature)).collect();
        for s in &subs {
            bump(format!("3 placed door subrecords\t{s}"));
        }
        for bit in 0..32 {
            if r.flags & (1 << bit) != 0 {
                bump(format!(
                    "3 placed door header flag bits\t0x{:08X}",
                    1u32 << bit
                ));
            }
        }
        for s in r
            .subrecords
            .iter()
            .filter(|s| matches!(&s.signature, b"XLOC" | b"ONAM" | b"XACT" | b"XTEL"))
        {
            bump(format!(
                "3 placed door subrecord sizes\t{} {} bytes",
                sig(s.signature),
                r.data(s).len()
            ));
        }
        let open = r.subrecords.iter().any(|s| s.signature == *b"ONAM");
        bump(format!(
            "6 placed doors by ONAM and collision\tONAM {}\t{}",
            if open { "present" } else { "absent" },
            if placed.contains(k) {
                "collision placed"
            } else {
                "collision not placed"
            }
        ));
        match fo4_play::world::load_door(&store, k, &b.child()) {
            Ok(Some(d)) => {
                if list {
                    let at = i.reference.data.transform.map(|t| [t[0], t[1], t[2]]);
                    eprintln!(
                        "load door {} at {at:?} -> {} in {} at {:?}",
                        fo4_vm::bind::record_key(k),
                        fo4_vm::bind::record_key(&d.to),
                        fo4_vm::bind::record_key(&d.space),
                        d.pos
                    );
                }
                bump("4 load doors (XTEL naming a placed door)".into())
            }
            Ok(None) => bump("4 plain doors (no XTEL)".into()),
            Err(why) => bump(format!("4 XTEL refused\t{why}")),
        }
    }
    for k in &scene.disabled {
        if fo4_vm::bind::base_of(&store, &k.key, &b.child())
            .ok()
            .flatten()
            .and_then(|x| store.signature(&x))
            == Some(*b"DOOR")
        {
            bump("1 placed doors (initially disabled, not loaded)".into());
        }
    }
    for d in &doors {
        let Some(r) = store.raw(d, &b.child())? else {
            continue;
        };
        bump("5 door records".into());
        let subs: BTreeSet<String> = r.subrecords.iter().map(|s| sig(s.signature)).collect();
        for s in &subs {
            bump(format!("5 door record subrecords\t{s}"));
        }
        for bit in 0..32 {
            if r.flags & (1 << bit) != 0 {
                bump(format!(
                    "5 door record header flag bits\t0x{:08X}",
                    1u32 << bit
                ));
            }
        }
        for s in r.subrecords.iter().filter(|s| s.signature == *b"FNAM") {
            let v: Vec<String> = r.data(s).iter().map(|x| format!("{x:02X}")).collect();
            bump(format!("5 door record FNAM bytes\t{}", v.join(" ")));
        }
    }
    // A worldspace keeps its load doors among its persistent references.
    if let (Some(space), Some(g)) = (
        store.world(&scene.cell.key),
        fo4_scene::grid::grid_of(&store, &scene.cell.key, &b)?,
    ) {
        let mut c2 = BTreeMap::new();
        let near = fo4_play::world::persistent_load_doors(
            &store,
            &space,
            g,
            around as i32 + 1,
            &mut c2,
            &b,
        )?;
        for (k, at, d) in &near {
            bump("7 load doors among the worldspace's persistent references within the cells about the start and one more".into());
            if list {
                eprintln!(
                    "persistent load door {} at {at:?} -> {} in {} at {:?}",
                    fo4_vm::bind::record_key(k),
                    fo4_vm::bind::record_key(&d.to),
                    fo4_vm::bind::record_key(&d.space),
                    d.pos
                );
            }
        }
        for (k, v) in c2 {
            *counts.entry(format!("7 {k}")).or_default() += v;
        }
    }
    let mut out = String::from(
        "# door_census: the placed doors of the start cell and the cells about it (spec/play.md, Doors); counts only\n",
    );
    let _ = writeln!(out, "# cells about the start cell: {around}");
    for (k, v) in &counts {
        let _ = writeln!(out, "{v}\t{k}");
    }
    print!("{out}");
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
