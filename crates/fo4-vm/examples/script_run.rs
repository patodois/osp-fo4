#![forbid(unsafe_code)]
//! Runs the scripts of one placed reference for chosen events and prints
//! the effect log (`spec/vm.md`). Every run is under pending rules.
//!
//! ```text
//! script_run MANIFEST CELL[@DX,DY] list
//! script_run MANIFEST CELL[@DX,DY] INDEX EVENT[,EVENT...] [--timers N]
//! script_run MANIFEST --census OUT_DIR (CELL[:RADIUS]... | ALL | PLACED)
//! ```
//!
//! Runs use the quest host (the data host plus the quest natives, stage
//! fragments, quest events and OnStageSet run inside the call that brought
//! them) and attach the scripts of quests, aliases and placed references
//! when first needed (`fo4_vm::bind::Mode::Quest`); `--data` anywhere on
//! the line runs the data host without the quest host, as before the
//! quest host (`Mode::Data`), and `--recording` the recording host alone,
//! as before the data host (`Mode::Recording`).
//!
//! CELL is an EditorID; `@DX,DY` takes the exterior cell that many grid
//! steps east and north of it. INDEX numbers the cell's placed references
//! as `list` prints them (active, then initially disabled). EVENT is one of
//! `fo4_vm::bind::EVENTS` or `fragment:N`. `--timers N` fires up to N
//! queued timers after the events.
//!
//! `--census` delivers, to every scripted DOOR, TERM and ACTI placed in the
//! cells (or, with `ALL`, every winning scripted DOOR, TERM and ACTI base,
//! bound alone; with `PLACED`, every winning placed reference of a
//! scripted DOOR, TERM or ACTI base, or with its own VMAD, in every cell),
//! each event that one of its scripts has a function with instructions
//! for, and each TERM fragment, one fresh run per event. It writes
//! `vm-census.txt` (counts only), `vm-census-names.txt` (names),
//! `vm-census-origins.txt` (blocking origins ranked, names) and
//! `vm-blockers.txt` (the natives whose result a blocked handler needed,
//! ranked by handlers, and the objects without a script instance by kind
//! and by the engine class of the type called), and `vm-visibility.txt`
//! (every visibility change of a placed REFR by handlers that completed,
//! names), and `vm-enable-children.txt` (names): every placed record whose
//! enable parent chain names a reference a run enables or disables, with
//! the state its chain gives before and after the run under the enable
//! parent rules (`spec/enable-parent.md`) and whether the scene would draw
//! it; their counts go to `vm-census.txt` (`enable children:` lines).
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
    mode: bind::Mode,
    opts: &bind::Options,
) -> Result<(), E> {
    let b = Budget::new(m.limits.max_memory);
    let mut lib = VfsLibrary::new(vfs, m.limits.max_memory);
    // (label, reference, base)
    let mut targets: Vec<(String, Option<FormKey>, FormKey)> = Vec::new();
    let mut counts = Counts::new();
    let mut loaded_keys = std::collections::BTreeSet::new();
    let mut enable_children: std::collections::HashMap<FormKey, u64> = Default::default();
    for spec in specs {
        if spec == "PLACED" {
            let placed: Vec<FormKey> = store
                .placement_cells()
                .filter(|(o, _)| !o.deleted && o.signature == *b"REFR")
                .map(|(o, _)| store.overrides().form_key(o.key, store.order()))
                .collect();
            for r in placed {
                loaded_keys.insert(bind::record_key(&r));
                let b = b.child();
                // Enable parents (XESP, first word): only counted, for the
                // visibility lines; the scene draws no reference that has one.
                if let Some((x, plugin)) = store.raw_with_plugin(&r, &b)? {
                    for s in x.subrecords.iter().filter(|s| s.signature == *b"XESP") {
                        let d = x.data(s);
                        if d.len() >= 4 {
                            let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                            if let Ok(Some(p)) =
                                store.order().remap(plugin, id, &x.source.file, s.offset)
                            {
                                *enable_children.entry(p).or_default() += 1;
                            }
                        }
                    }
                }
                let Some(base) = bind::base_of(store, &r, &b)? else {
                    continue;
                };
                if !store.signature(&base).is_some_and(|s| KINDS.contains(&s)) {
                    continue;
                }
                let scripted = [&base, &r].iter().any(|k| {
                    store
                        .raw(k, &b.child())
                        .ok()
                        .flatten()
                        .is_some_and(|x| x.subrecords.iter().any(|s| s.signature == *b"VMAD"))
                });
                if scripted {
                    targets.push((format!("placed {}", bind::record_key(&r)), Some(r), base));
                }
            }
            continue;
        }
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
    let mut visibility = String::new();
    let mut enable_lines = String::new();
    let enable_index = EnableIndex::build(store, &b)?;
    let cells_of: std::collections::HashMap<FormKey, FormKey> = store
        .placement_cells()
        .map(|(o, c)| (store.overrides().form_key(o.key, store.order()), c))
        .collect();
    let mut blockers = Counts::new();
    let mut handlers = 0u64;
    let cache = Budget::new(m.limits.max_memory);
    let mut loader = fo4_scene::Loader::new(
        store,
        vfs,
        &m.language,
        fo4_scene::RuleLevel::Hypotheses,
        &cache,
    );
    let mut checked = 0u64;
    for (label, reference, base) in &targets {
        let b = b.child();
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
            let r = bind::run_with(
                mode,
                opts,
                &mut lib,
                store,
                reference.as_ref(),
                base,
                std::slice::from_ref(ev),
                0,
                &b,
            )?;
            for ((native, answer), n) in &r.answers {
                let kind = match answer {
                    fo4_vm::data::Answer::Record => "a record".to_string(),
                    fo4_vm::data::Answer::NoRecord => "none".to_string(),
                    fo4_vm::data::Answer::Value => "a value".to_string(),
                    fo4_vm::data::Answer::Open(why) => format!("undetermined: {why}"),
                };
                *blockers
                    .entry(format!("data answer\t{native}\t{kind}"))
                    .or_default() += n;
            }
            for (k, n) in &r.event_counts {
                *counts.entry(format!("events: {k}")).or_default() += n;
            }
            for (kind, n) in &r.attached {
                *counts
                    .entry(format!("handles given scripts when first needed: {kind}"))
                    .or_default() += n;
            }
            // Quest jobs the handlers brought (`Mode::Quest`).
            for j in &r.quest_jobs {
                let what = match &j.job {
                    fo4_vm::quest::Job::Fragment { .. } => "stage fragment".to_string(),
                    fo4_vm::quest::Job::Event { event, .. } => format!("event {event}"),
                    fo4_vm::quest::Job::StageEvent { .. } => "event onstageset".to_string(),
                    fo4_vm::quest::Job::Open { why, .. } => format!("open: {why}"),
                    fo4_vm::quest::Job::Custom { .. } => "custom event".to_string(),
                    fo4_vm::quest::Job::Story { .. } => "event onstoryscript".to_string(),
                    fo4_vm::quest::Job::Scene { .. } => "scene step".to_string(),
                };
                if j.outcomes.is_empty() {
                    bump(
                        &mut counts,
                        format!("quest jobs run inside handlers: {what}"),
                    );
                }
                for (_, o) in &j.outcomes {
                    let k = match o {
                        Outcome::Completed(_) => "completed".to_string(),
                        Outcome::Blocked(b) => match &b.on {
                            fo4_vm::Origin::Native(n) => {
                                *blockers
                                    .entry(format!("quest job blocking native\t{n}"))
                                    .or_default() += 1;
                                "blocked on a native's result".to_string()
                            }
                            fo4_vm::Origin::Rule(rule, _) => format!("blocked on rule {rule}"),
                            fo4_vm::Origin::NoInstance(..) => {
                                "blocked on an object without the script".to_string()
                            }
                        },
                        Outcome::Failed(e) => format!("failed: {}", e.kind()),
                    };
                    bump(
                        &mut counts,
                        format!("quest jobs run inside handlers: {what}: {k}"),
                    );
                }
            }
            if r.quest_jobs_left > 0 {
                bump(&mut counts, "runs that left quest jobs queued (job limit)");
            }
            for n in &r.attach_notes {
                let kind = if n.contains("is not declared") {
                    "property not declared"
                } else if n.contains("no variable behind it") {
                    "property without a variable"
                } else if n.contains("not attached") {
                    "script not attached"
                } else {
                    "property value refused"
                };
                bump(&mut counts, format!("attach note: {kind}"));
            }
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
                        match &bl.on {
                            fo4_vm::Origin::Native(n) => {
                                bump(&mut blockers, format!("native\t{n}"))
                            }
                            fo4_vm::Origin::NoInstance(t, kind) => {
                                let class = engine_class(&mut lib, t);
                                bump(&mut blockers, format!("object kind\t{kind}"));
                                bump(
                                    &mut blockers,
                                    format!("object kind and type class\t{kind}\t{class}"),
                                );
                            }
                            fo4_vm::Origin::Rule(..) => {}
                        }
                        let what = match &bl.on {
                            fo4_vm::Origin::Native(_) => "blocked on a native result".to_string(),
                            fo4_vm::Origin::Rule(rule, _) => format!("blocked on rule {rule}"),
                            fo4_vm::Origin::NoInstance(..) => {
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
                // Whether the change could show in a frame: a handler
                // that completed, on a placed reference, enabling one that
                // starts disabled or disabling one that starts enabled.
                let completed = r
                    .outcomes
                    .iter()
                    .all(|o| matches!(o.2, Outcome::Completed(_)));
                let target = fo4_vm::data::key_record(&k);
                let start = target
                    .as_ref()
                    .and_then(|t| store.overrides().winner(t))
                    .filter(|w| !w.deleted && w.signature == *b"REFR")
                    .map(|w| w.disabled);
                let what = match (shown, start) {
                    (_, None) => "not a placed REFR",
                    (true, Some(true)) => "enable of an initially disabled REFR",
                    (true, Some(false)) => "enable of an enabled REFR",
                    (false, Some(false)) => "disable of an enabled REFR",
                    (false, Some(true)) => "disable of an initially disabled REFR",
                };
                bump(
                    &mut counts,
                    format!(
                        "visibility change: {what}, handlers {}",
                        if completed {
                            "completed"
                        } else {
                            "not all completed"
                        }
                    ),
                );
                if completed && start.is_some() {
                    // What the scene would hold against drawing the target:
                    // its base's record type and the reference subrecords
                    // that block drawing (VMAD, CTDA, XESP).
                    let mut traits = Vec::new();
                    if let Some(t) = &target {
                        let b = b.child();
                        if let Some(base) = bind::base_of(store, t, &b)? {
                            let sig = store.signature(&base).unwrap_or(*b"????");
                            traits.push(format!("base {}", String::from_utf8_lossy(&sig)));
                        }
                        if let Some(x) = store.raw(t, &b)? {
                            for sub in [b"VMAD", b"CTDA", b"XESP"] {
                                if x.subrecords.iter().any(|y| y.signature == *sub) {
                                    traits.push(String::from_utf8_lossy(sub).into_owned());
                                }
                            }
                        }
                        if Some(t) == reference.as_ref() {
                            traits.push("itself".into());
                        }
                    }
                    // Whether the scene would draw the target under every
                    // pending rule: its instance made as a cell load makes
                    // one (`fo4_scene::Loader::instance`).
                    let drawable = match &target {
                        Some(t) => {
                            checked += 1;
                            if checked.is_multiple_of(200) {
                                loader.reset_asset_cache(&cache);
                            }
                            let b = b.child();
                            match loader.instance(t, &b) {
                                Ok(i) if i.state.ready_at(fo4_scene::RuleLevel::Hypotheses) => {
                                    "drawable".to_string()
                                }
                                Ok(i) => {
                                    let mut codes: Vec<&str> =
                                        i.blockers.iter().map(|e| e.code.as_str()).collect();
                                    codes.sort_unstable();
                                    codes.dedup();
                                    format!("not drawable: {}", codes.join(" "))
                                }
                                Err(e) => format!("not drawable: {}", e.code),
                            }
                        }
                        None => "not drawable: no record".to_string(),
                    };
                    bump(
                        &mut counts,
                        format!(
                            "visibility change: {what}, handlers completed, target {}",
                            if drawable == "drawable" {
                                "drawable under every pending rule"
                            } else {
                                "not drawable"
                            }
                        ),
                    );
                    traits.push(drawable);
                    if let Some(n) = target.as_ref().and_then(|t| enable_children.get(t)) {
                        traits.push(format!("enable parent of {n} placed REFR"));
                        bump(
                            &mut counts,
                            format!("visibility change: {what}, handlers completed, target an enable parent"),
                        );
                    }
                    let cell = target.as_ref().and_then(|t| cells_of.get(t));
                    let bound_cell = reference.as_ref().and_then(|t| cells_of.get(t));
                    writeln!(
                        visibility,
                        "{what}\t{}\t{}\t{}\t{}\t{k}\t{}\t{}",
                        traits.join(" "),
                        label,
                        ev.name(),
                        if cell.is_some() && cell == bound_cell {
                            "same cell"
                        } else {
                            "other cell"
                        },
                        cell.map(bind::record_key).unwrap_or_default(),
                        r.world
                            .objects
                            .iter()
                            .find(|o| o.key.as_deref() == Some(&k))
                            .map(|o| o.label.clone())
                            .unwrap_or_default()
                    )?;
                }
            }
            let completed = r
                .outcomes
                .iter()
                .all(|o| matches!(o.2, Outcome::Completed(_)));
            let mut last: BTreeMap<String, bool> = BTreeMap::new();
            for (k, shown) in bind::visibility_changes(&r) {
                last.insert(k, shown);
            }
            if !last.is_empty() {
                checked += 1;
                if checked.is_multiple_of(200) {
                    loader.reset_asset_cache(&cache);
                }
                // Where to reproduce it: the bound reference's cell and its
                // index there (`script_run CELL INDEX EVENT`, `--script-event`).
                let place = match reference
                    .as_ref()
                    .and_then(|r| cells_of.get(r).map(|c| (r, c)))
                {
                    Some((r, c)) => {
                        let edid = store
                            .raw(c, &b)?
                            .and_then(|x| bind::editor_id(&x))
                            .unwrap_or_default();
                        let index = bind::placed(store, c, &b)?.iter().position(|k| k == r);
                        format!("{edid} {index:?}")
                    }
                    None => String::new(),
                };
                enable_index.effects(
                    store,
                    &mut loader,
                    &last,
                    completed,
                    &format!("{label}\t{}\t{place}", ev.name()),
                    &mut counts,
                    &mut enable_lines,
                    &b,
                )?;
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
    std::fs::write(out_dir.join("vm-visibility.txt"), visibility)?;
    std::fs::write(out_dir.join("vm-enable-children.txt"), enable_lines)?;
    // Names leave the counts file: origin and failure lines name natives
    // and scripts, so only their number of distinct values is kept there.
    let mut agg = String::new();
    let scope = if specs.iter().any(|s| s == "ALL") {
        "every winning scripted DOOR, TERM and ACTI base, bound alone"
    } else if specs.iter().any(|s| s == "PLACED") {
        "every placed reference of a DOOR, TERM or ACTI base with a VMAD on the base or the reference"
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
    let mut ranked: Vec<(u64, &String)> = blockers.iter().map(|(k, v)| (*v, k)).collect();
    ranked.sort_by(|a, b| {
        let section = |k: &str| k.split('\t').next().unwrap_or("").to_string();
        section(a.1)
            .cmp(&section(b.1))
            .then(b.0.cmp(&a.0))
            .then(a.1.cmp(b.1))
    });
    let mut text = String::new();
    for (v, k) in ranked {
        writeln!(text, "{v}\t{k}")?;
    }
    std::fs::write(out_dir.join("vm-blockers.txt"), text)?;
    Ok(())
}

/// Placed records by the enable parent their XESP names.
struct EnableIndex {
    children: std::collections::HashMap<FormKey, Vec<FormKey>>,
}
impl EnableIndex {
    fn build(store: &Store, b: &Budget) -> Result<Self, E> {
        let mut children: std::collections::HashMap<FormKey, Vec<FormKey>> = Default::default();
        let placed: Vec<FormKey> = store
            .placement_cells()
            .filter(|(o, _)| !o.deleted && matches!(&o.signature, b"REFR" | b"ACHR"))
            .map(|(o, _)| store.overrides().form_key(o.key, store.order()))
            .collect();
        for k in placed {
            let b = b.child();
            let Some((r, plugin)) = store.raw_with_plugin(&k, &b)? else {
                continue;
            };
            if let Ok(Some(l)) = fo4_plugin::enable::read(&r, plugin, store) {
                if let Some(p) = l.parent {
                    children.entry(p).or_default().push(k);
                }
            }
        }
        Ok(Self { children })
    }
    /// Counts what the visibility changes `last` of one run do to the
    /// enable children of the changed references, transitively.
    #[allow(clippy::too_many_arguments)]
    fn effects(
        &self,
        store: &Store,
        loader: &mut fo4_scene::Loader<'_>,
        last: &BTreeMap<String, bool>,
        completed: bool,
        label: &str,
        counts: &mut Counts,
        lines: &mut String,
        b: &Budget,
    ) -> Result<(), E> {
        let level = fo4_scene::RuleLevel::Hypotheses;
        let done = if completed {
            "handlers completed"
        } else {
            "not all completed"
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut queue: Vec<FormKey> = last
            .keys()
            .filter_map(|k| fo4_vm::data::key_record(k))
            .collect();
        let set = |k: &FormKey| last.get(&bind::record_key(k)).copied();
        let mut reached = 0u64;
        let mut drawable_changes = 0u64;
        while let Some(p) = queue.pop() {
            let Some(kids) = self.children.get(&p) else {
                continue;
            };
            for c in kids {
                if !seen.insert(c.clone()) {
                    continue;
                }
                queue.push(c.clone());
                reached += 1;
                let b = b.child();
                let sig = store.signature(c).unwrap_or(*b"????");
                let sig = String::from_utf8_lossy(&sig).into_owned();
                let (presence, drawable, why) = if sig == "ACHR" {
                    let Some(r) = store.raw(c, &b)? else { continue };
                    let a = fo4_scene::actor::resolve_reference(loader, c, &r.source, &b)?;
                    let ok = a.plan.is_some() && a.state.ready_at(level);
                    let why = if a.plan.is_none() {
                        "no plan".to_string()
                    } else {
                        uncovered(&a.blockers)
                    };
                    (a.enable, ok, why)
                } else {
                    match loader.instance(c, &b) {
                        Ok(i) => {
                            let ok = i.state.ready_at(level) && !i.editor_marker;
                            let why = if i.editor_marker {
                                "editor marker".to_string()
                            } else {
                                uncovered(&i.blockers)
                            };
                            (i.enable, ok, why)
                        }
                        Err(e) => (None, false, e.code),
                    }
                };
                let Some(p) = presence else {
                    bump(
                        counts,
                        format!("enable children: {done}, {sig} without a chain"),
                    );
                    continue;
                };
                let (before, after) = (p.enabled, p.with(&set));
                let change = match (before, after) {
                    (Some(x), Some(y)) if x == y => "state unchanged",
                    (Some(false), Some(true)) => "disabled -> enabled",
                    (Some(true), Some(false)) => "enabled -> disabled",
                    _ => "not decided",
                };
                bump(counts, format!("enable children: {done}, {sig}, {change}"));
                if before == after || before.is_none() || after.is_none() {
                    continue;
                }
                bump(
                    counts,
                    format!(
                        "enable children: {done}, {sig}, {change}, {}",
                        if drawable {
                            "drawable under every pending rule"
                        } else {
                            "not drawable"
                        }
                    ),
                );
                if drawable && completed {
                    drawable_changes += 1;
                }
                let at = store
                    .raw(c, &b)?
                    .and_then(|x| {
                        let d = x.subrecords.iter().find(|s| s.signature == *b"DATA")?;
                        let d = x.data(d);
                        (d.len() >= 12).then(|| {
                            [0, 4, 8]
                                .map(|o| f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]))
                        })
                    })
                    .map(|p| format!("{:.0},{:.0},{:.0}", p[0], p[1], p[2]))
                    .unwrap_or_default();
                writeln!(
                    lines,
                    "{label}\t{}\t{sig}\t{change}\t{}\t{why}\tdepth {}\t{at}",
                    bind::record_key(c),
                    if drawable { "drawable" } else { "not drawable" },
                    p.chain.depth()
                )?;
            }
        }
        bump(
            counts,
            format!(
                "enable children: runs ({done}) whose changes reach {} children",
                bucket(reached as usize)
            ),
        );
        if drawable_changes > 0 {
            bump(
                counts,
                format!(
                    "enable children: runs (handlers completed) changing {} drawable placements",
                    bucket(drawable_changes as usize)
                ),
            );
        }
        Ok(())
    }
}

/// The codes of blockers no registered rule covers.
fn uncovered(blockers: &[fo4_core::Error]) -> String {
    let mut v: Vec<String> = blockers
        .iter()
        .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
        .map(|e| {
            if e.code == "RuntimeEnableParent" || e.code == "UninterpretedVisualSubrecord" {
                format!("{} {}", e.code, e.found)
            } else {
                e.code.clone()
            }
        })
        .collect();
    v.sort();
    v.dedup();
    v.join(" ")
}

/// Engine types a blocked call's type is classified by: the first of these
/// in the type's chain, else `other`.
const ENGINE_CLASSES: [&str; 10] = [
    "referencealias",
    "refcollectionalias",
    "locationalias",
    "activemagiceffect",
    "actor",
    "objectreference",
    "quest",
    "scene",
    "alias",
    "form",
];
fn engine_class(lib: &mut VfsLibrary<'_>, t: &str) -> &'static str {
    let mut host = fo4_vm::NoHost;
    let mut vm = Vm::new(lib, &mut host);
    let chain = vm.chain(t).unwrap_or_default();
    for s in &chain {
        if let Some(c) = ENGINE_CLASSES.iter().find(|c| **c == s.name) {
            return c;
        }
    }
    "other"
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mode = if args.iter().any(|a| a == "--recording") {
        bind::Mode::Recording
    } else if args.iter().any(|a| a == "--data") {
        bind::Mode::Data
    } else {
        bind::Mode::Quest
    };
    let no_chain = args.iter().any(|a| a == "--no-enable-chain");
    let no_story = args.iter().any(|a| a == "--no-story");
    let no_events = args.iter().any(|a| a == "--no-events");
    args.retain(|a| {
        !matches!(
            a.as_str(),
            "--recording" | "--data" | "--no-enable-chain" | "--no-story" | "--no-events"
        )
    });
    let usage = "script_run MANIFEST CELL[@DX,DY] (list | INDEX EVENT[,EVENT...] [--timers N]) | script_run MANIFEST --census OUT_DIR (CELL[:RADIUS]... | ALL | PLACED); --data: the data host without quests; --recording: the recording host alone; --no-enable-chain: IsDisabled from the header flag alone; --no-story: no story manager; --no-events: no remote or custom event delivered";
    if args.len() < 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut opts = bind::Options::of(mode);
    opts.enable_chain &= !no_chain;
    if no_events {
        opts.remote = false;
        opts.custom = None;
    }
    if mode == bind::Mode::Quest && !no_story {
        let b = Budget::new(m.limits.max_memory);
        opts.story = Some(std::rc::Rc::new(fo4_plugin::story::Tree::read(&store, &b)?));
    }
    if args[1] == "--census" {
        return census(
            &m,
            &vfs,
            &store,
            Path::new(&args[2]),
            &args[3..],
            mode,
            &opts,
        );
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
    let run = bind::run_with(
        mode,
        &opts,
        &mut lib,
        &store,
        Some(r),
        &base,
        &events,
        timers,
        &b,
    )?;
    print!("{}", bind::effect_log(&run));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
