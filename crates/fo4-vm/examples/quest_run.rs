#![forbid(unsafe_code)]
//! Sets quest stages and runs their fragments with `fo4_vm::quest`
//! (`spec/vm.md`, Quests). Every run is under pending rules.
//!
//! ```text
//! quest_run MANIFEST stages OUT_DIR      every quest, every stage with a fragment
//! quest_run MANIFEST new-game OUT_DIR    the quests started with a new game
//! quest_run MANIFEST QUEST STAGE         one stage of one quest (EditorID), effect log
//! ```
//!
//! `stages` sets, for every winning quest and every stage of its record
//! that a fragment of its VMAD block names, that stage in a fresh
//! interpreter (isolated scenario, `quest_state_isolated_v1`) and runs the
//! fragments and quest events it queues, then those they queue, up to
//! [`MAX_JOBS`]. `new-game` starts every quest flagged to start with the
//! game in one interpreter (`quest_start_game_enabled_v1`) and runs what
//! that queues, up to [`MAX_GAME_JOBS`]. Both write `quest-run-census.txt`
//! (counts, type tags and native function names only) and
//! `quest-run-names.txt` (effect logs with names, never committed) to
//! OUT_DIR.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind::{self, outcome_text, RecordScripts, VfsLibrary},
    data::Answer,
    quest::{self, Change, Job, JobRun, QuestHost, QuestInfo},
    Effect, Origin, Outcome, Vm, World,
};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

/// Jobs run after one stage of the sweep.
const MAX_JOBS: usize = 256;
/// Jobs run after a new game starts.
const MAX_GAME_JOBS: usize = 100_000;

fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        20..=99 => "20-99",
        _ => ">=100",
    }
}

/// The kind of an effect for the census (`native` lower case).
fn effect_kind(e: &Effect, w: &World) -> String {
    let n = e.native.as_str();
    let (script, name) = n.split_once('.').unwrap_or(("", n));
    let k = match (script, name) {
        ("quest", "setcurrentstageid") => {
            // The same quest when the caller is one of the quest's own
            // scripts (its fragment script among them).
            let caller = e.caller.split('.').next().unwrap_or("");
            let same = e.receiver_object.is_some_and(|q| {
                w.objects[q]
                    .instances
                    .iter()
                    .any(|i| w.instances[*i].script == caller)
            });
            if same {
                "stage set on the same quest"
            } else {
                "stage set on another quest"
            }
        }
        ("quest", "start") => "quest started",
        ("quest", "stop") => "quest stopped",
        ("quest", "setobjectivedisplayed") => "objective displayed or hidden",
        ("quest", "setobjectivecompleted") => "objective completed",
        ("quest", "setobjectivefailed") => "objective failed",
        ("quest", "setactive") => "quest made active",
        ("quest", "completequest") => "quest completed",
        (_, "enable" | "enablenowait") => "enable",
        (_, "disable" | "disablenowait") => "disable",
        (
            _,
            "moveto"
            | "movetonode"
            | "setposition"
            | "setangle"
            | "movetomyeditorlocation"
            | "movetonearestnavmeshlocation"
            | "translateto",
        ) => "move",
        (_, "additem" | "removeitem" | "removeallitems" | "equipitem" | "unequipitem") => {
            "items added, removed or equipped"
        }
        (_, "placeatme" | "placeactoratme" | "placeatnode") => "object placed",
        (_, "delete") => "reference deleted",
        ("scene", _) => "scene",
        ("message", _) | ("debug", "notification" | "messagebox") => "message",
        ("globalvariable", "setvalue" | "setvalueint" | "mod") => "global set",
        ("referencealias", "forcerefto" | "clear") | ("refcollectionalias", _) => {
            "alias fill changed"
        }
        (_, "setvalue" | "modvalue" | "damagevalue" | "restorevalue") => "actor value changed",
        (_, "addtofaction" | "removefromfaction" | "setfactionrank") => "faction",
        (_, "evaluatepackage" | "setrestrained" | "setunconscious" | "kill" | "resurrect") => {
            "actor state"
        }
        ("scriptobject", "registerforremoteevent" | "unregisterforremoteevent")
        | ("scriptobject", "registerforcustomevent" | "registerforhitevent")
        | ("scriptobject", "registerfordistancelessthanevent") => "event registration",
        ("scriptobject", "starttimer" | "starttimergametime" | "canceltimer") => "timer",
        ("objectreference", "lock" | "setopen" | "setlocklevel") => "lock or open state",
        _ => return format!("other: {n}"),
    };
    k.to_string()
}

fn outcome_kind(o: &Outcome) -> String {
    match o {
        Outcome::Completed(_) => "completed".into(),
        Outcome::Blocked(b) => match &b.on {
            Origin::Native(_) => "blocked on a native's result".into(),
            Origin::Rule(r, _) => format!("blocked on an undetermined value (rule {r})"),
            Origin::NoInstance(..) => {
                "blocked on a scripted call on an object without that script".into()
            }
        },
        Outcome::Failed(e) => format!("failed: {}", e.kind()),
    }
}

/// The text of one job and its effects for the names file.
fn job_text(vm: &Vm<'_>, r: &JobRun) -> String {
    let mut s = String::new();
    let label = |o: usize| vm.world.objects[o].label.clone();
    let _ = match &r.job {
        Job::Fragment {
            quest,
            stage,
            entry,
            function,
            depth,
        } => writeln!(
            s,
            "  [{depth}] fragment {function} of stage {stage} entry {entry} on {}",
            label(*quest)
        ),
        Job::Event {
            quest,
            event,
            depth,
        } => writeln!(s, "  [{depth}] event {event} on {}", label(*quest)),
        Job::Open { quest, why, depth } => {
            writeln!(s, "  [{depth}] open on {}: {why}", label(*quest))
        }
    };
    for (script, o) in &r.outcomes {
        let _ = writeln!(s, "    {script}: {}", outcome_text(o));
    }
    for n in &r.notes {
        let _ = writeln!(s, "    note: {n}");
    }
    for e in &vm.effects[r.effects.clone()] {
        let _ = writeln!(
            s,
            "    {}{}({}{}) = {}",
            "  ".repeat(e.depth),
            e.native,
            e.receiver
                .as_ref()
                .map(|x| format!("on {x}; "))
                .unwrap_or_default(),
            e.args.join(", "),
            e.result
        );
    }
    s
}

/// Counts what one list of jobs did.
fn count_jobs(c: &mut Counts, runs: &[JobRun], prefix: &str) {
    for r in runs {
        let what = match &r.job {
            Job::Fragment { .. } => "fragment".to_string(),
            Job::Event { event, .. } => format!("event {event}"),
            Job::Open { why, .. } => {
                bump(c, format!("{prefix}open runs: {why}"));
                continue;
            }
        };
        if r.outcomes.is_empty() {
            bump(c, format!("{prefix}{what} runs with no handler"));
        }
        for (_, o) in &r.outcomes {
            bump(c, format!("{prefix}{what} outcome: {}", outcome_kind(o)));
            match o {
                Outcome::Blocked(b) => match &b.on {
                    Origin::Native(n) => bump(c, format!("{prefix}blocking native\t{n}")),
                    Origin::Rule(rule, _) => bump(c, format!("{prefix}blocking rule\t{rule}")),
                    Origin::NoInstance(t, kind) => {
                        let _ = t;
                        bump(
                            c,
                            format!("{prefix}blocking object without the script\t{kind}"),
                        )
                    }
                },
                Outcome::Failed(e) => bump(c, format!("{prefix}failure\t{}", e.kind())),
                Outcome::Completed(_) => {}
            }
        }
    }
}

/// Counts every effect of a run once, by kind.
fn count_effects(c: &mut Counts, vm: &Vm<'_>, prefix: &str) {
    for e in &vm.effects {
        bump(c, format!("{prefix}effects\t{}", effect_kind(e, &vm.world)));
    }
}

/// The jobs run in order of starting: those the driver ran and those a
/// [`quest::Runner`] ran inside them.
fn all_runs(vm: &mut Vm<'_>, mut top: Vec<JobRun>) -> Vec<JobRun> {
    top.append(&mut vm.world.quests.nested_runs);
    top.sort_by_key(|r| r.seq);
    top
}

fn quests(store: &Store, b: &Budget) -> Result<Vec<(FormKey, QuestInfo)>, E> {
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut out = Vec::new();
    for k in keys {
        if let Some(i) = QuestInfo::load(store, &k, &b.child())? {
            out.push((k, i));
        }
    }
    Ok(out)
}

fn stages(m: &Manifest, vfs: &Vfs, store: &Store, out: &Path, nested: bool) -> Result<(), E> {
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let all = quests(store, &b)?;
    for (k, info) in &all {
        let with: Vec<u16> = info
            .stages
            .iter()
            .filter(|s| s.entries.iter().any(|e| e.fragment.is_some()))
            .map(|s| s.stage)
            .collect();
        bump(
            &mut c,
            format!("quests with {} stages with a fragment", bucket(with.len())),
        );
        for n in &with {
            bump(&mut c, "stages set");
            let b = b.child();
            let mut host = QuestHost::new(store, max);
            let mut attach = RecordScripts::new(store, max);
            let mut runner = quest::Runner::new(store, max, MAX_JOBS);
            let mut vm = Vm::new(&mut lib, &mut host);
            vm.attach = Some(&mut attach);
            if nested {
                vm.nested = Some(&mut runner);
            }
            let o = quest::quest_object(&mut vm, store, k, &b)?;
            let reply = quest::set_stage(&mut vm, store, o, i32::from(*n));
            bump(&mut c, format!("SetCurrentStageID answer: {reply:?}"));
            let top = quest::run_jobs(&mut vm, store, MAX_JOBS, &b);
            let runs = all_runs(&mut vm, top);
            count_effects(&mut c, &vm, "");
            let left = vm.world.quests.queue.len();
            if left > 0 {
                bump(
                    &mut c,
                    "stages whose runs reached the job limit (cascade cut)",
                );
            }
            // The stage's own run: the first job.
            let target = runs.first();
            let kind = match target {
                None => "no fragment queued".to_string(),
                Some(r) => match &r.job {
                    Job::Open { why, .. } => format!("open: {why}"),
                    _ => match r.outcomes.first() {
                        None => "no handler".into(),
                        Some((_, o)) => outcome_kind(o),
                    },
                },
            };
            bump(&mut c, format!("stage fragment\t{kind}"));
            if let Some(r) = target {
                count_jobs(&mut c, std::slice::from_ref(r), "stage fragment ");
            }
            if runs.len() > 1 {
                count_jobs(&mut c, &runs[1..], "cascade ");
            }
            // Cascades: fragments run, depth, quests touched.
            let fragments = runs
                .iter()
                .filter(|r| matches!(r.job, Job::Fragment { .. }))
                .count();
            let depth = runs.iter().map(|r| r.job.depth()).max().unwrap_or(0);
            bump(
                &mut c,
                format!("cascade fragments per stage set\t{}", bucket(fragments)),
            );
            let most = c
                .entry("most fragments run after one stage set".into())
                .or_default();
            *most = (*most).max(fragments as u64);
            let deepest = c
                .entry("deepest cascade after one stage set".into())
                .or_default();
            *deepest = (*deepest).max(depth as u64);
            bump(
                &mut c,
                format!("cascade depth per stage set\t{}", bucket(depth)),
            );
            let quests_set: std::collections::BTreeSet<usize> = vm
                .world
                .quests
                .log
                .iter()
                .filter(|(_, ch)| matches!(ch, Change::StageSet(_)))
                .map(|(q, _)| *q)
                .collect();
            bump(
                &mut c,
                format!(
                    "quests with a stage set per sweep item\t{}",
                    bucket(quests_set.len())
                ),
            );
            let sets = vm
                .world
                .quests
                .log
                .iter()
                .filter(|(_, ch)| matches!(ch, Change::StageSet(_)))
                .count();
            bump(
                &mut c,
                format!("stages set per sweep item\t{}", bucket(sets)),
            );
            for (_, ch) in &vm.world.quests.log {
                let what = match ch {
                    Change::StageSet(_) => "stage set",
                    Change::StageRepeatRefused(_) => "stage already done, not set again",
                    Change::Started => "quest started",
                    Change::Stopped => "quest stopped",
                    Change::Objective(quest::DISPLAYED, _, Some(true)) => "objective displayed",
                    Change::Objective(quest::DISPLAYED, _, _) => "objective hidden or undetermined",
                    Change::Objective(quest::COMPLETED, _, _) => "objective completed (or not)",
                    Change::Objective(..) => "objective failed (or not)",
                    Change::Opened(_) => "quest state became undetermined",
                };
                bump(&mut c, format!("quest changes\t{what}"));
            }
            if runs.iter().all(|r| {
                !matches!(r.job, Job::Open { .. })
                    && r.outcomes
                        .iter()
                        .all(|(_, o)| matches!(o, Outcome::Completed(_)))
            }) && left == 0
            {
                bump(&mut c, "sweep items whose every run completed");
            }
            for r in &vm.rules {
                bump(&mut c, format!("sweep items relying on\t{r}"));
            }
            let _ = writeln!(names, "## {} stage {n}: {kind}", vm.world.objects[o].label);
            for r in &runs {
                names.push_str(&job_text(&vm, r));
            }
            drop(vm);
            for ((native, a), v) in host.answers.iter().chain(host.data.answers.iter()) {
                let a = match a {
                    Answer::Record => "a record".to_string(),
                    Answer::NoRecord => "none".to_string(),
                    Answer::Value => "a value".to_string(),
                    Answer::Open(why) => format!("undetermined: {why}"),
                };
                *c.entry(format!("host answer\t{native}\t{a}")).or_default() += v;
            }
        }
    }
    write(out, &c, &names, "stages")
}

fn new_game(m: &Manifest, vfs: &Vfs, store: &Store, out: &Path, nested: bool) -> Result<(), E> {
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let mut host = QuestHost::new(store, max);
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_GAME_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    let started = quest::start_new_game(&mut vm, store, &b)?;
    for (_, _, r) in &started {
        bump(
            &mut c,
            format!("quests started with the game: Start answer {r:?}"),
        );
    }
    let top = quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b);
    let runs = all_runs(&mut vm, top);
    *c.entry("jobs left in the queue when the run ended".into())
        .or_default() += vm.world.quests.queue.len() as u64;
    *c.entry("jobs run".into()).or_default() += runs.len() as u64;
    for r in &runs {
        bump(&mut c, format!("jobs at depth\t{}", bucket(r.job.depth())));
        names.push_str(&job_text(&vm, r));
    }
    count_jobs(&mut c, &runs, "");
    count_effects(&mut c, &vm, "");
    let mut quests_set = std::collections::BTreeSet::new();
    for (q, ch) in &vm.world.quests.log {
        let what = match ch {
            Change::StageSet(_) => {
                quests_set.insert(*q);
                "stage set"
            }
            Change::StageRepeatRefused(_) => "stage already done, not set again",
            Change::Started => "quest started",
            Change::Stopped => "quest stopped",
            Change::Objective(quest::DISPLAYED, _, Some(true)) => "objective displayed",
            Change::Objective(quest::DISPLAYED, _, _) => "objective hidden or undetermined",
            Change::Objective(quest::COMPLETED, _, _) => "objective completed (or not)",
            Change::Objective(..) => "objective failed (or not)",
            Change::Opened(_) => "quest state became undetermined",
        };
        bump(&mut c, format!("quest changes\t{what}"));
    }
    *c.entry("quests with a stage set".into()).or_default() += quests_set.len() as u64;
    // References enabled or disabled: by the placed record type of the
    // receiver and whether the record starts disabled.
    for e in &vm.effects {
        let shown = match e.native.as_str() {
            "objectreference.enable" | "objectreference.enablenowait" => true,
            "objectreference.disable" | "objectreference.disablenowait" => false,
            _ => continue,
        };
        let target = e
            .receiver_object
            .and_then(|o| vm.world.objects[o].key.clone())
            .and_then(|k| fo4_vm::data::key_record(&k))
            .and_then(|k| {
                store
                    .overrides()
                    .winner(&k)
                    .map(|w| (w.signature, w.disabled))
            });
        let what = match target {
            Some((s, d)) => format!(
                "{} of a placed {} {}",
                if shown { "enable" } else { "disable" },
                String::from_utf8_lossy(&s),
                if d {
                    "flagged initially disabled"
                } else {
                    "not flagged"
                }
            ),
            None => format!(
                "{} of an object that is not a placed record (alias, undetermined or opaque)",
                if shown { "enable" } else { "disable" }
            ),
        };
        bump(&mut c, format!("visibility\t{what}"));
    }
    let objectives: std::collections::BTreeSet<(usize, i32)> = vm
        .world
        .quests
        .log
        .iter()
        .filter_map(|(q, ch)| match ch {
            Change::Objective(quest::DISPLAYED, n, Some(true)) => Some((*q, *n)),
            _ => None,
        })
        .collect();
    *c.entry("distinct objectives displayed".into()).or_default() += objectives.len() as u64;
    let running = vm
        .world
        .quests
        .states
        .values()
        .filter(|s| s.running == Some(true))
        .count();
    *c.entry("quests running at the end (of those the run read)".into())
        .or_default() += running as u64;
    for r in &vm.rules {
        bump(&mut c, format!("rules relied on\t{r}"));
    }
    drop(vm);
    for ((native, a), v) in host.answers.iter().chain(host.data.answers.iter()) {
        let a = match a {
            Answer::Record => "a record".to_string(),
            Answer::NoRecord => "none".to_string(),
            Answer::Value => "a value".to_string(),
            Answer::Open(why) => format!("undetermined: {why}"),
        };
        *c.entry(format!("host answer\t{native}\t{a}")).or_default() += v;
    }
    write(out, &c, &names, "new-game")
}

fn write(out: &Path, c: &Counts, names: &str, scope: &str) -> Result<(), E> {
    std::fs::create_dir_all(out)?;
    let mut s = String::new();
    writeln!(
        s,
        "# quest_run {scope}: counts, type tags and native names only (spec/vm.md, Quests)"
    )?;
    // Other natives are named; their counts are kept, ranked below.
    let mut other: Vec<(&u64, &String)> = Vec::new();
    for (k, v) in c {
        if k.contains("effects\tother: ") {
            other.push((v, k));
            continue;
        }
        writeln!(s, "{v}\t{k}")?;
    }
    other.sort_by(|a, b| b.0.cmp(a.0).then(a.1.cmp(b.1)));
    writeln!(s, "\n## effects of other natives, ranked")?;
    for (v, k) in other {
        writeln!(s, "{v}\t{k}")?;
    }
    std::fs::write(out.join("quest-run-census.txt"), s)?;
    std::fs::write(out.join("quest-run-names.txt"), names)?;
    Ok(())
}

fn one(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    edid: &str,
    stage: i32,
    nested: bool,
) -> Result<(), E> {
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let k = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(edid))
        })
        .ok_or("no quest of that editor ID")?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut host = QuestHost::new(store, max);
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    let o = quest::quest_object(&mut vm, store, &k, &b)?;
    let reply = quest::set_stage(&mut vm, store, o, stage);
    println!(
        "set stage {stage} on {}: {reply:?}",
        vm.world.objects[o].label
    );
    let top = quest::run_jobs(&mut vm, store, MAX_JOBS, &b);
    for r in all_runs(&mut vm, top) {
        print!("{}", job_text(&vm, &r));
    }
    let rules: Vec<&str> = vm.rules.iter().copied().collect();
    println!("rules (pending): {}", rules.join(" "));
    Ok(())
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let queued = a.iter().any(|x| x == "--queued");
    a.retain(|x| x != "--queued");
    let usage = "quest_run MANIFEST (stages OUT_DIR | new-game OUT_DIR | QUEST STAGE) [--queued]";
    if a.len() != 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    match a[1].as_str() {
        "stages" => stages(&m, &vfs, &store, Path::new(&a[2]), !queued),
        "new-game" => new_game(&m, &vfs, &store, Path::new(&a[2]), !queued),
        q => one(&m, &vfs, &store, q, a[2].parse()?, !queued),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
