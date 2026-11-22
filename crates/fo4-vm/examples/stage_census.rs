#![forbid(unsafe_code)]
//! Census of every place in the data that can set a quest stage
//! (`spec/vm.md`, Stage setters): `stage_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts, kinds, record type tags and engine event names
//! only (committed as `spec/stage-setter-census.txt`); NAMES.txt lists,
//! quest by quest and stage by stage, the setters with their scripts,
//! records and what each needs, and is never committed. The table comes
//! from `fo4_vm::setters`: a static scan, nothing runs.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{story::Tree, typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind,
    setters::{self, Need, On, Resolved, ScenePoint, Setter},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        _ => ">=20",
    }
}

/// The record a need waits on, with its label, for the names file.
fn need_target(n: &Need, label: &dyn Fn(&FormKey) -> String) -> String {
    match n {
        Need::Stage(q, s) => format!("{} stage {s}", label(q)),
        Need::Event(_, On::Record(k, _)) => label(k),
        Need::Event(_, On::Alias(q, i)) => format!("alias {i} of {}", label(q)),
        Need::Event(_, On::Unattached) | Need::Remote(..) | Need::Custom | Need::Unreached => {
            String::new()
        }
        Need::Scene(k, p) => format!(
            "{} {}",
            label(k),
            match p {
                ScenePoint::Begin => "begin".to_string(),
                ScenePoint::End => "end".to_string(),
                ScenePoint::Phase(i, f) => format!("phase {i} fragment flag {f}"),
                ScenePoint::PhaseField(i, e) =>
                    format!("phase {i} {}", if *e { "end" } else { "begin" }),
            }
        ),
        Need::Line(k, e) => format!("{} {}", label(k), if *e { "end" } else { "begin" }),
        Need::Package(k, b) => format!("{} flag {b}", label(k)),
        Need::Perk(k, e) => format!("{} entry {e}", label(k)),
        Need::Terminal(k, i) => format!("{} item {i}", label(k)),
        Need::QuestStart(k) => label(k),
    }
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("stage_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let b = Budget::new(m.limits.max_memory);
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let lib = setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let (atts, vmad_counts) = setters::attachments(&store, &setters::owners(&store), &b)?;
    let tree = Tree::read(&store, &b)?;
    let t = setters::table(&lib, &atts, &store, Some(&tree), &b)?;
    let label = |k: &FormKey| -> String {
        bind::label(&store, k, &b.child()).unwrap_or_else(|_| bind::record_key(k))
    };

    let mut c = Counts::new();
    // ---- 1 what the scan read ---------------------------------------------
    *c.entry("1 scripts read".into()).or_default() += lib.scripts.len() as u64;
    *c.entry("1 attachments (a script attached by one VMAD part)".into())
        .or_default() += atts.len() as u64;
    for x in &atts {
        bump(&mut c, format!("1 attachments by kind\t{}", x.kind()));
    }
    for (k, v) in &vmad_counts {
        *c.entry(format!("1 {k}")).or_default() += v;
    }
    *c.entry("1 engine events (functions without instructions named on..., declared by a script with a native function)".into())
        .or_default() += setters::engine_events(&lib).len() as u64;
    for (k, v) in &t.counts {
        *c.entry(format!("1 {k}")).or_default() += v;
    }

    // ---- 2 setters ----------------------------------------------------------
    *c.entry("2 setters".into()).or_default() += t.setters.len() as u64;
    let mut names_absent = String::new();
    let mut names_alias = String::new();
    for s in &t.setters {
        bump(&mut c, format!("2 setters by kind\t{}", s.kind));
        bump(
            &mut c,
            format!(
                "2 setters by kind and need\t{}\t{}",
                s.kind,
                s.need.text(&store)
            ),
        );
        bump(
            &mut c,
            format!("2 setters by the family of their need\t{}", s.need.family()),
        );
        bump(
            &mut c,
            format!(
                "2 setters: quest\t{}",
                match &s.quest {
                    Resolved::Const(_) => "a constant".to_string(),
                    Resolved::Open(w) => format!("not a constant: {w}"),
                }
            ),
        );
        bump(
            &mut c,
            format!(
                "2 setters: stage\t{}",
                match &s.stage {
                    Resolved::Const(_) => "a constant".to_string(),
                    Resolved::Open(w) => format!("not a constant: {w}"),
                }
            ),
        );
        if let (Some(q), Some(n)) = (s.quest.get(), s.stage.get()) {
            let held = t
                .stages
                .get(&bind::record_key(q))
                .map(|(_, v)| v.iter().any(|x| i32::from(x.0) == *n));
            let what = match held {
                Some(true) => "a stage the quest holds",
                Some(false) => "a stage the quest does not hold",
                None => "the quest is not a winning QUST",
            };
            bump(
                &mut c,
                format!("2 constant setters: the stage against the quest record\t{what}"),
            );
            if held != Some(true) {
                bump(
                    &mut c,
                    format!(
                        "2 constant setters naming no stage of a winning quest, by kind\t{}",
                        s.kind
                    ),
                );
                let _ = writeln!(
                    names_absent,
                    "  {} stage {n} | {} | {} {} | {}",
                    label(q),
                    s.kind,
                    s.need.text(&store),
                    need_target(&s.need, &label),
                    s.site
                );
            }
        }
    }

    // ---- 3 stages -------------------------------------------------------------
    let mut by_stage: BTreeMap<(String, i32), Vec<&Setter>> = BTreeMap::new();
    for s in &t.setters {
        if let (Some(q), Some(n)) = (s.quest.get(), s.stage.get()) {
            by_stage
                .entry((bind::record_key(q), *n))
                .or_default()
                .push(s);
        }
    }
    let open_quest: BTreeSet<String> = BTreeSet::new();
    let _ = open_quest;
    let mut names = String::new();
    let mut total = 0u64;
    for (qk, (q, stages)) in &t.stages {
        let _ = writeln!(names, "QUEST {}", label(q));
        for st in &t.of_quest(q) {
            let _ = writeln!(
                names,
                "  START {}{} [{}]",
                st.kind,
                st.need
                    .as_ref()
                    .map(|n| format!(": {} {}", n.text(&store), need_target(n, &label)))
                    .unwrap_or_default(),
                st.site
            );
        }
        for (n, flags) in stages {
            total += 1;
            let list = by_stage.get(&(qk.clone(), i32::from(*n)));
            let count = list.map_or(0, |l| l.len());
            bump(
                &mut c,
                format!("3 stages by number of setters\t{}", bucket(count)),
            );
            let fams: BTreeSet<&str> = list
                .into_iter()
                .flatten()
                .map(|s| s.need.family())
                .collect();
            let kinds: BTreeSet<String> = list
                .into_iter()
                .flatten()
                .map(|s| s.kind.split(',').next().unwrap_or("").to_string())
                .collect();
            if count == 0 {
                bump(
                    &mut c,
                    format!(
                        "3 stages with no setter found\t{}",
                        if flags & fo4_vm::quest::STARTUP_STAGE != 0 {
                            "a startup stage (cannot happen: counted as a setter)"
                        } else {
                            "not a startup stage"
                        }
                    ),
                );
            }
            for f in &fams {
                bump(&mut c, format!("3 stages with a setter whose need is\t{f}"));
            }
            for k in &kinds {
                bump(&mut c, format!("3 stages with a setter of kind\t{k}"));
            }
            let _ = writeln!(names, "  STAGE {n} (flags {flags:#04x}): {count} setter(s)");
            for s in list.into_iter().flatten() {
                let _ = writeln!(
                    names,
                    "    {} | {} {} | depth {} | {}",
                    s.kind,
                    s.need.text(&store),
                    need_target(&s.need, &label),
                    s.depth,
                    s.site
                );
            }
        }
    }
    *c.entry("3 stages of winning quests".into()).or_default() += total;
    *c.entry("3 quests with stages".into()).or_default() += t.stages.len() as u64;
    let mut quests_without = 0u64;
    for (qk, (_, stages)) in &t.stages {
        if !stages
            .iter()
            .any(|(n, _)| by_stage.contains_key(&(qk.clone(), i32::from(*n))))
        {
            quests_without += 1;
        }
    }
    *c.entry("3 quests with stages none of which has a setter".into())
        .or_default() += quests_without;
    let mut held = 0u64;
    for (qk, n) in by_stage.keys() {
        let ok = t
            .stages
            .get(qk)
            .is_some_and(|(_, v)| v.iter().any(|x| i32::from(x.0) == *n));
        if !ok {
            held += 1;
        }
    }
    *c.entry("3 (quest, stage) pairs a constant setter names that no quest record holds".into())
        .or_default() += held;

    // ---- 4 starters --------------------------------------------------------
    *c.entry("4 starters".into()).or_default() += t.starters.len() as u64;
    let mut started: BTreeSet<String> = BTreeSet::new();
    for s in &t.starters {
        bump(&mut c, format!("4 starters by kind\t{}", s.kind));
        if let Some(n) = &s.need {
            bump(&mut c, format!("4 Start calls by need\t{}", n.text(&store)));
        }
        match &s.quest {
            Resolved::Const(q) => {
                started.insert(bind::record_key(q));
            }
            Resolved::Open(w) => bump(
                &mut c,
                format!("4 Start calls whose quest is not a constant\t{w}"),
            ),
        }
    }
    *c.entry("4 quests with a starter".into()).or_default() +=
        t.stages.keys().filter(|k| started.contains(*k)).count() as u64;
    // Unattached scripts' setters, for the names file.
    let _ = writeln!(names, "## setters whose quest or stage is not a constant");
    for s in &t.setters {
        if s.quest.get().is_none() || s.stage.get().is_none() {
            let _ = writeln!(
                names,
                "  {} | quest {} | stage {} | {} {} | {}",
                s.kind,
                s.quest.get().map_or(s.quest.why().to_string(), label),
                s.stage
                    .get()
                    .map_or(s.stage.why().to_string(), |n| n.to_string()),
                s.need.text(&store),
                need_target(&s.need, &label),
                s.site
            );
        }
    }

    // ---- 5 reference aliases filled from a location's special references
    let lctn: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"LCTN")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    for k in &lctn {
        match fo4_vm::quest::special_references(&store, k, &b.child())? {
            None => bump(
                &mut c,
                "5 LCTN special reference lists (LCSR)\tnot 16-byte rows",
            ),
            Some(rows) => {
                if !rows.is_empty() {
                    bump(
                        &mut c,
                        "5 LCTN special reference lists (LCSR)\t16-byte rows",
                    );
                }
                for (rt, r) in rows {
                    let first = rt.as_ref().and_then(|x| store.signature(x));
                    let second = r.as_ref().and_then(|x| store.signature(x));
                    bump(
                        &mut c,
                        format!(
                            "5 LCSR rows: first word, second word\t{}, {}",
                            first.map_or("no winning record".into(), |x| String::from_utf8_lossy(
                                &x
                            )
                            .into_owned()),
                            second.map_or("no winning record".into(), |x| String::from_utf8_lossy(
                                &x
                            )
                            .into_owned())
                        ),
                    );
                }
            }
        }
    }
    for (q, _) in t.stages.values() {
        let Some(i) = fo4_vm::quest::QuestInfo::load(&store, q, &b.child())? else {
            continue;
        };
        for (id, r) in &i.location_refs {
            bump(
                &mut c,
                format!(
                    "5 reference aliases filled from a location alias with a ref type\t{}",
                    match r {
                        Ok(_) => "one special reference of that type",
                        Err(w) => w,
                    }
                ),
            );
            if let Ok(k) = r {
                let _ = writeln!(names_alias, "  {} alias {id}: {}", label(q), label(k));
            }
        }
    }
    let _ = writeln!(
        names,
        "## reference aliases filled from their location's special references"
    );
    names.push_str(&names_alias);
    let _ = writeln!(
        names,
        "## constant setters naming no stage of a winning quest"
    );
    names.push_str(&names_absent);
    let mut out = String::new();
    writeln!(
        out,
        "# stage_census: counts, kinds, record type tags and engine event names only (spec/vm.md, Stage setters)"
    )?;
    writeln!(
        out,
        "# 1 what the scan read; 2 every setter by kind, need and resolution; 3 every stage of every winning quest by its setters; 4 how quests start; 5 reference aliases filled from a location's special references"
    )?;
    for (k, v) in &c {
        writeln!(out, "{v}\t{k}")?;
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
