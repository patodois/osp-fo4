#![forbid(unsafe_code)]
//! Runs the scripts of one placed reference for chosen events and prints
//! the effect log (`spec/vm.md`). Every run is under pending rules.
//!
//! ```text
//! script_run MANIFEST CELL[@DX,DY] list
//! script_run MANIFEST CELL[@DX,DY] INDEX EVENT[,EVENT...] [--timers N]
//! script_run MANIFEST --census OUT_DIR (CELL[:RADIUS]... | ALL)
//! ```
//!
//! CELL is an EditorID; `@DX,DY` takes the exterior cell that many grid
//! steps east and north of it. INDEX numbers the cell's placed references
//! as `list` prints them (active, then initially disabled). EVENT is one of
//! `fo4_vm::bind::EVENTS` or `fragment:N`. `--timers N` fires up to N
//! queued timers after the events.
//!
//! `--census` delivers, to every scripted DOOR, TERM and ACTI placed in the
//! cells (or, with `ALL`, every winning scripted DOOR, TERM and ACTI base,
//! bound alone), each event that one of its scripts has a function with
//! instructions for, and each TERM fragment, one fresh run per event. It
//! writes `vm-census.txt` (counts only) and `vm-census-names.txt` (names).
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind::{self, Event, VfsLibrary},
    Outcome, Vm,
};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
const KINDS: [[u8; 4]; 3] = [*b"DOOR", *b"TERM", *b"ACTI"];

fn cell(store: &Store, spec: &str, b: &Budget) -> Result<FormKey, E> {
    let (id, off) = match spec.split_once('@') {
        Some((id, o)) => {
            let (x, y) = o.split_once(',').ok_or("CELL@DX,DY")?;
            (id, Some([x.parse::<i32>()?, y.parse::<i32>()?]))
        }
        None => (spec, None),
    };
    let key = store.find_cell(id, b)?.ok_or("no cell of that editor ID")?;
    let Some(off) = off else { return Ok(key) };
    if off == [0, 0] {
        return Ok(key);
    }
    let r = off[0].unsigned_abs().max(off[1].unsigned_abs());
    let a = fo4_scene::grid::around(store, &key, r, b)?;
    a.neighbours
        .into_iter()
        .find(|n| n.offset == off)
        .and_then(|n| n.cell)
        .ok_or_else(|| "no cell at that offset".into())
}

/// Events a bound object's scripts have functions with instructions for.
fn events_of(
    lib: &mut VfsLibrary<'_>,
    store: &Store,
    reference: Option<&FormKey>,
    base: &FormKey,
    b: &Budget,
) -> Result<Vec<Event>, E> {
    let mut host = fo4_vm::NoHost;
    let mut vm = Vm::new(lib, &mut host);
    let bound = match reference {
        Some(r) => bind::bind_reference(&mut vm, store, r, b)?.ok_or("no base")?,
        None => bind::bind_base(&mut vm, store, base, b)?,
    };
    let mut out = Vec::new();
    for (name, f) in bind::EVENTS {
        let mut any = false;
        for (_, i) in &bound.instances {
            if vm.find_handler(*i, f)?.is_some() {
                any = true;
            }
        }
        if any {
            out.push(Event::Named(name, f));
        }
    }
    if let Some((_, list)) = &bound.fragments {
        out.extend(list.iter().map(|x| Event::Fragment(x.0)));
    }
    Ok(out)
}

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

fn census(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    out_dir: &Path,
    specs: &[String],
) -> Result<(), E> {
    let b = Budget::new(m.limits.max_memory);
    let mut lib = VfsLibrary::new(vfs, m.limits.max_memory);
    // (label, reference, base)
    let mut targets: Vec<(String, Option<FormKey>, FormKey)> = Vec::new();
    let mut counts = Counts::new();
    let mut loaded_keys = std::collections::BTreeSet::new();
    for spec in specs {
        if spec == "ALL" {
            let bases: Vec<FormKey> = store
                .winners()
                .filter(|w| !w.deleted && KINDS.contains(&w.signature))
                .map(|w| store.overrides().form_key(w.key, store.order()))
                .collect();
            for base in bases {
                let has = store
                    .raw(&base, &b)?
                    .is_some_and(|r| r.subrecords.iter().any(|s| s.signature == *b"VMAD"));
                if has {
                    targets.push((format!("base {}", bind::record_key(&base)), None, base));
                }
            }
            continue;
        }
        let (c, radius) = match spec.split_once(':') {
            Some((c, r)) => (c, r.parse::<u32>()?),
            None => (spec.as_str(), 0),
        };
        let key = store.find_cell(c, &b)?.ok_or("no cell of that editor ID")?;
        let mut cells = vec![(key.clone(), format!("{c}@0,0"))];
        if radius > 0 {
            let a = fo4_scene::grid::around(store, &key, radius, &b)?;
            for n in a.neighbours {
                if let Some(k) = n.cell {
                    cells.push((k, format!("{c}@{},{}", n.offset[0], n.offset[1])));
                }
            }
        }
        for (ck, cname) in cells {
            for (i, r) in bind::placed(store, &ck, &b)?.into_iter().enumerate() {
                loaded_keys.insert(bind::record_key(&r));
                let Some(base) = bind::base_of(store, &r, &b)? else {
                    continue;
                };
                if !store.signature(&base).is_some_and(|s| KINDS.contains(&s)) {
                    continue;
                }
                let scripted = [&base, &r].iter().any(|k| {
                    store
                        .raw(k, &b)
                        .ok()
                        .flatten()
                        .is_some_and(|x| x.subrecords.iter().any(|s| s.signature == *b"VMAD"))
                });
                if scripted {
                    targets.push((format!("{cname} {i}"), Some(r), base));
                }
            }
        }
    }
    let mut names = String::new();
    let mut handlers = 0u64;
    for (label, reference, base) in &targets {
        bump(&mut counts, "objects bound");
        let events = match events_of(&mut lib, store, reference.as_ref(), base, &b) {
            Ok(e) => e,
            Err(e) => {
                bump(&mut counts, "objects whose binding failed");
                writeln!(names, "{label}\tbinding failed: {e}")?;
                continue;
            }
        };
        if events.is_empty() {
            bump(
                &mut counts,
                "objects without an event function with instructions",
            );
        }
        for ev in &events {
            let r = bind::run(
                &mut lib,
                store,
                reference.as_ref(),
                base,
                std::slice::from_ref(ev),
                0,
                &b,
            )?;
            writeln!(
                names,
                "## {label} {} event {} ({})",
                r.world.objects[r.bound.object].label,
                ev.name(),
                r.bound
                    .instances
                    .iter()
                    .map(|x| x.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
            names.push_str(&bind::effect_log(&r));
            for n in &r.bound.notes {
                let kind = if n.contains("is not declared") {
                    "property not declared"
                } else if n.contains("no variable behind it") {
                    "property without a variable"
                } else if n.contains("not attached") {
                    "script not attached"
                } else {
                    "property value refused"
                };
                bump(&mut counts, format!("binding note: {kind}"));
            }
            for (_, _, o) in &r.outcomes {
                handlers += 1;
                bump(
                    &mut counts,
                    format!(
                        "event {} handlers",
                        ev.name().split(':').next().unwrap_or("")
                    ),
                );
                match o {
                    Outcome::Completed(_) => bump(&mut counts, "outcome completed"),
                    Outcome::Blocked(bl) => {
                        let what = match &bl.on {
                            fo4_vm::Origin::Native(_) => "blocked on a native result".to_string(),
                            fo4_vm::Origin::Rule(rule, _) => format!("blocked on rule {rule}"),
                            fo4_vm::Origin::NoInstance(_) => {
                                "blocked on a scripted call or property of an object without that script".to_string()
                            }
                        };
                        bump(&mut counts, format!("outcome {what}"));
                        let op = bl.at.as_ref().map_or(0, |s| s.op);
                        bump(&mut counts, format!("blocked at opcode {op:02}"));
                        bump(&mut counts, format!("blocking origin: {}", bl.on.text()));
                    }
                    Outcome::Failed(e) => {
                        bump(&mut counts, format!("outcome failed: {}", e.kind()));
                        bump(&mut counts, format!("failure: {e}"));
                    }
                }
            }
            for (k, shown) in bind::visibility_changes(&r) {
                let place = if loaded_keys.contains(&k) {
                    "a reference of the loaded cells"
                } else if r.bound.object
                    == r.world
                        .objects
                        .iter()
                        .position(|o| o.key.as_deref() == Some(&k))
                        .unwrap_or(usize::MAX)
                {
                    "the bound object itself"
                } else {
                    "another record"
                };
                bump(
                    &mut counts,
                    format!(
                        "visibility change {} of {place}",
                        if shown { "enable" } else { "disable" }
                    ),
                );
            }
            bump(
                &mut counts,
                format!("runs with {} effects", bucket(r.effects.len())),
            );
            for rule in &r.rules {
                bump(&mut counts, format!("runs relying on {rule}"));
            }
        }
    }
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("vm-census-names.txt"), names)?;
    // Names leave the counts file: origin and failure lines name natives
    // and scripts, so only their number of distinct values is kept there.
    let mut agg = String::new();
    let scope = if specs.iter().any(|s| s == "ALL") {
        "every winning scripted DOOR, TERM and ACTI base, bound alone"
    } else {
        "scripted DOOR, TERM and ACTI placed in the cells named"
    };
    writeln!(agg, "{}\ttargets: {scope}", targets.len())?;
    writeln!(agg, "{handlers}\thandlers run")?;
    let origins = counts
        .keys()
        .filter(|k| k.starts_with("blocking origin: "))
        .count();
    let failures = counts.keys().filter(|k| k.starts_with("failure: ")).count();
    let mut top: Vec<(u64, &String)> = counts
        .iter()
        .filter(|(k, _)| k.starts_with("blocking origin: "))
        .map(|(k, v)| (*v, k))
        .collect();
    top.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    for (k, v) in &counts {
        if k.starts_with("blocking origin: ") || k.starts_with("failure: ") {
            continue;
        }
        writeln!(agg, "{v}\t{k}")?;
    }
    writeln!(agg, "{origins}\tdistinct blocking origins")?;
    writeln!(agg, "{failures}\tdistinct failures")?;
    let shares: Vec<String> = top.iter().take(10).map(|(v, _)| v.to_string()).collect();
    writeln!(
        agg,
        "{}\thandlers blocked by the ten most frequent origins ({})",
        top.iter().take(10).map(|x| x.0).sum::<u64>(),
        shares.join(", ")
    )?;
    std::fs::write(out_dir.join("vm-census.txt"), agg)?;
    let mut ranked = String::new();
    for (v, k) in top {
        writeln!(ranked, "{v}\t{k}")?;
    }
    for (k, v) in counts.iter().filter(|(k, _)| k.starts_with("failure: ")) {
        writeln!(ranked, "{v}\t{k}")?;
    }
    std::fs::write(out_dir.join("vm-census-origins.txt"), ranked)?;
    Ok(())
}

fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2..=4 => "2-4",
        5..=9 => "5-9",
        _ => ">=10",
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "script_run MANIFEST CELL[@DX,DY] (list | INDEX EVENT[,EVENT...] [--timers N]) | script_run MANIFEST --census OUT_DIR (CELL[:RADIUS]... | ALL)";
    if args.len() < 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    if args[1] == "--census" {
        return census(&m, &vfs, &store, Path::new(&args[2]), &args[3..]);
    }
    let b = Budget::new(m.limits.max_memory);
    let ck = cell(&store, &args[1], &b)?;
    let refs = bind::placed(&store, &ck, &b)?;
    if args[2] == "list" {
        for (i, r) in refs.iter().enumerate() {
            let Some(base) = bind::base_of(&store, r, &b)? else {
                continue;
            };
            let sig = store.signature(&base).unwrap_or(*b"????");
            let scripted = [&base, r].iter().any(|k| {
                store
                    .raw(k, &b)
                    .ok()
                    .flatten()
                    .is_some_and(|x| x.subrecords.iter().any(|s| s.signature == *b"VMAD"))
            });
            if scripted {
                let edid = store
                    .raw(&base, &b)?
                    .and_then(|r| bind::editor_id(&r))
                    .unwrap_or_default();
                println!(
                    "{i}\t{}\t{}\t{edid}",
                    String::from_utf8_lossy(&sig),
                    bind::record_key(r)
                );
            }
        }
        return Ok(());
    }
    let index: usize = args[2].parse()?;
    let r = refs.get(index).ok_or("reference index out of range")?;
    let base = bind::base_of(&store, r, &b)?.ok_or("reference without a base")?;
    let events: Vec<Event> = args
        .get(3)
        .ok_or(usage)?
        .split(',')
        .map(|e| Event::parse(e).ok_or_else(|| format!("unknown event {e}")))
        .collect::<Result<_, _>>()?;
    let timers = match args.get(4).map(String::as_str) {
        Some("--timers") => args.get(5).ok_or(usage)?.parse()?,
        Some(_) => return Err(usage.into()),
        None => 0,
    };
    let mut lib = VfsLibrary::new(&vfs, m.limits.max_memory);
    let run = bind::run(&mut lib, &store, Some(r), &base, &events, timers, &b)?;
    print!("{}", bind::effect_log(&run));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
