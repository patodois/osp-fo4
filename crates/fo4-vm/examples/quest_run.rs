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
use fo4_plugin::story::Tree;
use fo4_plugin::{typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::events::CustomOrder;
use fo4_vm::{
    bind::{self, outcome_text, RecordScripts, VfsLibrary},
    data::Answer,
    quest::{self, Change, Job, JobRun, QuestHost, QuestInfo},
    scene::{self, Step},
    Effect, Origin, Outcome, Vm, World,
};
use std::{collections::BTreeMap, fmt::Write as _, path::Path, rc::Rc};

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
        Job::StageEvent {
            quest,
            stage,
            entry,
            depth,
        } => writeln!(
            s,
            "  [{depth}] event onstageset({stage}, {}) on {}",
            entry.map_or("undetermined".to_string(), |e| e.to_string()),
            label(*quest)
        ),
        Job::Open { quest, why, depth } => {
            writeln!(s, "  [{depth}] open on {}: {why}", label(*quest))
        }
        Job::Custom {
            sender,
            event,
            depth,
            ..
        } => writeln!(
            s,
            "  [{depth}] custom event {event} from {}",
            label(*sender)
        ),
        Job::Story { quest, depth, .. } => {
            writeln!(s, "  [{depth}] event onstoryscript on {}", label(*quest))
        }
        Job::Scene { scene, step, depth } => {
            let what = match step {
                Step::Fragment(f, slot) => format!("fragment {f} ({slot})"),
                Step::Enter => "step: offer the next phase".into(),
                Step::Complete => "step: complete the phase".into(),
            };
            writeln!(s, "  [{depth}] scene {what} on {}", label(*scene))
        }
    };
    for (script, o) in &r.outcomes {
        let _ = writeln!(s, "    {script}: {}", outcome_text(o));
    }
    for (script, o) in &r.remote {
        let _ = writeln!(s, "    remote {script}: {}", outcome_text(o));
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
            Job::StageEvent { .. } => "event onstageset".to_string(),
            Job::Open { why, .. } => {
                bump(c, format!("{prefix}open runs: {why}"));
                continue;
            }
            Job::Custom { .. } => "custom event".to_string(),
            Job::Story { .. } => "event onstoryscript".to_string(),
            Job::Scene {
                step: Step::Fragment(_, slot),
                ..
            } => {
                let slot = if slot.starts_with("phase") {
                    if slot.ends_with("begin") {
                        "phase begin"
                    } else {
                        "phase end"
                    }
                } else {
                    slot.as_str()
                };
                format!("scene fragment ({slot})")
            }
            Job::Scene { .. } => {
                bump(c, format!("{prefix}scene steps"));
                continue;
            }
        };
        if r.outcomes.is_empty() {
            bump(c, format!("{prefix}{what} runs with no handler"));
        }
        let remote: Vec<(String, Outcome)> = r
            .remote
            .iter()
            .map(|(s, o)| (format!("remote {s}"), o.clone()))
            .collect();
        for (script, o) in r.outcomes.iter().chain(&remote) {
            let what = if script.starts_with("remote ") {
                format!("remote delivery of {what}")
            } else {
                what.clone()
            };
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

/// Switches that reproduce earlier censuses.
#[derive(Clone, Copy)]
struct Opts {
    /// Nested reading of `quest_stage_fragment_order_v1` (else queued).
    nested: bool,
    /// Evaluate log entry conditions (else every conditioned entry is
    /// undetermined, as before conditions were read).
    conditions: bool,
    /// Deliver OnStageSet (`quest_on_stage_set_v1`).
    stage_event: bool,
    /// Deliver remote events the run raises (`vm_remote_events_v1`).
    remote: bool,
    /// Deliver custom events (`vm_custom_events_v1`), queued or nested
    /// (`vm_custom_event_order_v1`).
    custom: Option<CustomOrder>,
    /// Offer the script event to the story manager (`sm_script_event_v1`).
    story: bool,
    /// `IsDisabled` follows enable parent chains
    /// (`vm_host_is_disabled_chain_v1`).
    enable_chain: bool,
    /// Answer the scene natives and run scenes (`scene_start_v1`,
    /// `scene_phase_progression_v1`).
    scenes: bool,
    /// Start a quest's scenes with FNAM bit 0x1 with the quest
    /// (`scene_begin_on_quest_start_v1`).
    scenes_with_quest: bool,
}

/// The story manager's nodes, read once when the story manager is on.
fn story_tree(store: &Store, o: Opts) -> Result<Option<Rc<Tree>>, E> {
    if !o.story {
        return Ok(None);
    }
    let b = Budget::new(store.limits.max_memory);
    Ok(Some(Rc::new(Tree::read(store, &b)?)))
}

fn host<'s>(store: &'s Store, max: u64, o: Opts, tree: &Option<Rc<Tree>>) -> QuestHost<'s> {
    let mut h = QuestHost::new(store, max);
    h.evaluate = o.conditions;
    h.stage_event = o.stage_event;
    h.data.enable_chain = o.enable_chain;
    if o.story {
        h.story = tree.clone();
    }
    h
}

/// Switches the world's event delivery as the options say.
fn events_on(vm: &mut Vm<'_>, o: Opts) {
    vm.world.events.remote = o.remote;
    vm.world.events.custom = o.custom;
    vm.world.scenes.on = o.scenes;
    vm.world.scenes.begin_on_quest_start = o.scenes && o.scenes_with_quest;
    if o.scenes {
        GROUPS.with(|g| vm.world.scenes.groups = g.borrow().clone());
    }
}

thread_local! {
    /// The groups of kind 10, read once per process when scenes are on.
    static GROUPS: std::cell::RefCell<Option<Rc<scene::Groups>>> = const { std::cell::RefCell::new(None) };
}

/// The counts of what scenes did in a run.
fn count_scenes(c: &mut Counts, vm: &Vm<'_>, prefix: &str) {
    for (_, ch) in &vm.world.scenes.log {
        let what = match ch {
            scene::Change::Started => "scene started".to_string(),
            scene::Change::StartedWithQuest => "scene started with its quest".to_string(),
            scene::Change::PhaseEntered(_) => "phase entered".to_string(),
            scene::Change::PhaseSkipped(_) => {
                "phase passed over (start conditions false)".to_string()
            }
            scene::Change::PhaseCompleted(_) => "phase completed".to_string(),
            scene::Change::Waiting(_, why) => format!("scene stopped in a phase\t{why}"),
            scene::Change::Ended => "scene ended (last phase completed)".to_string(),
            scene::Change::Stopped => "scene stopped by a script".to_string(),
            scene::Change::Refused(why) => format!("scene start refused\t{why}"),
            scene::Change::Opened(why) => format!("scene state became undetermined\t{why}"),
        };
        bump(c, format!("{prefix}scenes\t{what}"));
    }
}

/// The counts of event registrations and deliveries, and of story events.
fn count_events(c: &mut Counts, vm: &Vm<'_>) {
    for (k, v) in &vm.world.events.counts {
        *c.entry(format!("events\t{k}")).or_default() += v;
    }
}
fn count_story(c: &mut Counts, h: &QuestHost<'_>) {
    for (k, v) in &h.story_counts {
        *c.entry(format!("story\t{k}")).or_default() += v;
    }
}

/// The condition counts of a host.
fn count_conditions(c: &mut Counts, h: &QuestHost<'_>) {
    for (k, v) in &h.conditions {
        *c.entry(k.clone()).or_default() += v;
    }
    for (f, v) in &h.open_functions {
        *c.entry(format!(
            "log entry selections left open, by function index of an undetermined condition\tfunction {f}"
        ))
        .or_default() += v;
    }
}

/// Counts the OnStageSet handlers of every winning quest: on its own VMAD
/// scripts (to which `quest_on_stage_set_v1` delivers the event), on its
/// fragment script, and on the scripts of its aliases (which receive
/// nothing here).
fn stage_event_handlers(lib: &mut VfsLibrary<'_>, store: &Store, b: &Budget) -> Result<Counts, E> {
    let mut c = Counts::new();
    let max = store.limits.max_memory;
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut host = fo4_vm::RecordingHost::default();
    let mut attach = RecordScripts::new(store, max);
    let mut vm = Vm::new(lib, &mut host);
    vm.attach = Some(&mut attach);
    // Whether a script has a handler, and whether it takes two Int.
    let has = |vm: &mut Vm<'_>, script: &str| -> Option<bool> {
        let i = vm.instantiate(None, script).ok()?;
        let f = vm.find_handler(i, quest::ON_STAGE_SET).ok()??;
        Some(f.params.len() == 2 && f.params.iter().all(|(_, t)| t == "int"))
    };
    for k in keys {
        let b = b.child();
        let o = quest::quest_object(&mut vm, store, &k, &b)?;
        let _ = vm.attach_scripts(o);
        let own: Vec<String> = vm.world.objects[o]
            .instances
            .iter()
            .map(|i| vm.world.instances[*i].script.clone())
            .collect();
        let mut q = false;
        for sc in &own {
            if let Some(ints) = has(&mut vm, sc) {
                q = true;
                bump(
                    &mut c,
                    format!(
                        "OnStageSet handlers\tquest VMAD scripts with one{}",
                        if ints {
                            ""
                        } else {
                            " (not two Int parameters)"
                        }
                    ),
                );
            }
        }
        if q {
            bump(
                &mut c,
                "OnStageSet handlers\tquests whose own scripts have one",
            );
        }
        let Some((r, _)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
            let v =
                fo4_plugin::vmad::Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b)?;
            let Some(block) = &v.quest else { continue };
            if !block.script.name.is_empty() {
                let name = fo4_pex::index::lower(block.script.name);
                if has(&mut vm, &name).is_some() {
                    bump(&mut c, "OnStageSet handlers\tfragment scripts with one");
                }
            }
            for a in &block.aliases {
                for sc in &a.scripts {
                    let name = fo4_pex::index::lower(sc.name);
                    if has(&mut vm, &name).is_some() {
                        bump(
                            &mut c,
                            "OnStageSet handlers\talias scripts with one (not delivered)",
                        );
                    }
                }
            }
        }
    }
    Ok(c)
}

fn stages(m: &Manifest, vfs: &Vfs, store: &Store, out: &Path, opts: Opts) -> Result<(), E> {
    let nested = opts.nested;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = if opts.stage_event {
        stage_event_handlers(&mut lib, store, &b)?
    } else {
        Counts::new()
    };
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
            let mut host = host(store, max, opts, &tree);
            let mut attach = RecordScripts::new(store, max);
            let mut runner = quest::Runner::new(store, max, MAX_JOBS);
            let mut vm = Vm::new(&mut lib, &mut host);
            vm.attach = Some(&mut attach);
            if nested {
                vm.nested = Some(&mut runner);
            }
            events_on(&mut vm, opts);
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
            // The stage's own run: the first job at depth 1 other than
            // OnStageSet; OnStageSet at depth 1 is counted apart; every
            // other job is a cascade.
            let own = runs
                .iter()
                .position(|r| r.job.depth() == 1 && !matches!(r.job, Job::StageEvent { .. }));
            let target = own.map(|i| &runs[i]);
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
            let (events, cascade): (Vec<JobRun>, Vec<JobRun>) = runs
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != own)
                .map(|(_, r)| r.clone())
                .partition(|r| r.job.depth() == 1 && matches!(r.job, Job::StageEvent { .. }));
            count_jobs(&mut c, &events, "stage OnStageSet ");
            count_jobs(&mut c, &cascade, "cascade ");
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
                    Change::StartedByStory => "quest started by the story manager",
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
            count_events(&mut c, &vm);
            if opts.scenes {
                count_scenes(&mut c, &vm, "");
            }
            drop(vm);
            count_conditions(&mut c, &host);
            count_story(&mut c, &host);
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

/// What `new-game` is told to do beyond the rules, on the command line:
/// stages set from outside any script after the start (`--then
/// EDITOR_ID:STAGE`, repeated) and queued timers fired after the jobs
/// (`--timers N`). Neither is a rule: they are assumptions of the person
/// running it, and the census says when they were made.
#[derive(Default)]
struct Then {
    stages: Vec<(String, i32)>,
    timers: usize,
    /// Order game-time timers against real-time ones through the
    /// time-scale global (`vm_timer_time_scale_v1`), when timers are fired:
    /// the editor ID of that global, named by the caller
    /// (`--time-scale-global`); `None` keeps the earlier stop.
    time_scale: Option<String>,
}

fn quest_by_editor_id(store: &Store, b: &Budget, edid: &str) -> Result<FormKey, E> {
    Ok(store
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
        .ok_or("no quest of that editor ID")?)
}

fn new_game(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    out: &Path,
    opts: Opts,
    then: &Then,
) -> Result<(), E> {
    let nested = opts.nested;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let mut host = host(store, max, opts, &tree);
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_GAME_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    events_on(&mut vm, opts);
    let started = quest::start_new_game(&mut vm, store, &b)?;
    for (_, _, r) in &started {
        bump(
            &mut c,
            format!("quests started with the game: Start answer {r:?}"),
        );
    }
    let mut top = quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b);
    for (edid, n) in &then.stages {
        bump(
            &mut c,
            "assumption from the command line: a stage set from outside any script after the start",
        );
        let k = quest_by_editor_id(store, &b, edid)?;
        let o = quest::quest_object(&mut vm, store, &k, &b)?;
        let reply = quest::set_stage(&mut vm, store, o, *n);
        let _ = writeln!(names, "## --then {edid}:{n}: {reply:?}");
        top.extend(quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b));
    }
    let scale_global = then
        .time_scale
        .as_deref()
        .and_then(|n| time_scale_global(store, &b, n));
    if then.timers > 0 {
        bump(
            &mut c,
            "assumption from the command line: queued timers fired after the jobs",
        );
        let fired: Vec<(usize, fo4_vm::Val, Outcome)> = if then.time_scale.is_some() {
            let mut v = Vec::new();
            for _ in 0..then.timers {
                let scale = scale_global
                    .as_ref()
                    .and_then(|g| time_scale(&vm, store, g, &b));
                let before = vm.effects.len();
                let Some(x) = vm.fire_timer_scaled(scale) else {
                    break;
                };
                let _ = writeln!(names, "## timer fired (time scale {scale:?})");
                for e in &vm.effects[before..] {
                    let _ = writeln!(
                        names,
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
                let failed = matches!(x.2, Outcome::Failed(_));
                v.push(x);
                if failed {
                    break;
                }
                // The jobs a timer's handler queued run before the next.
                top.extend(quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b));
            }
            v
        } else {
            vm.fire_timers(then.timers)
        };
        for (inst, id, o) in fired {
            let script = vm.world.instances[inst].script.clone();
            bump(&mut c, format!("timer outcome: {}", outcome_kind(&o)));
            if let Outcome::Blocked(bl) = &o {
                if let Origin::Native(n) = &bl.on {
                    bump(&mut c, format!("timer blocking native\t{n}"));
                }
            }
            let _ = writeln!(
                names,
                "## timer {} on {script}: {}",
                vm.world.render(&id),
                outcome_text(&o)
            );
            top.extend(quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b));
        }
    }
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
            Change::StartedByStory => "quest started by the story manager",
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
    count_events(&mut c, &vm);
    if opts.scenes {
        // What the two timer kinds are asked for (`vm_timer_events_v1`).
        for e in &vm.effects {
            if let Some(kind) = match e.native.as_str() {
                "scriptobject.starttimer" => Some("real time, seconds"),
                "scriptobject.starttimergametime" => Some("game time, hours"),
                _ => None,
            } {
                let interval = e.args.first().cloned().unwrap_or_default();
                let interval = if interval.parse::<f64>().is_ok() {
                    interval
                } else {
                    "undetermined".into()
                };
                bump(&mut c, format!("timers asked\t{kind}\tinterval {interval}"));
            }
        }
        if let Some(g) = scale_global.clone() {
            let s = time_scale(&vm, store, &g, &b);
            bump(
                &mut c,
                format!(
                    "time-scale global at the end (vm_timer_time_scale_v1)\t{}",
                    s.map_or("undetermined".to_string(), |v| v.to_string())
                ),
            );
        }
        count_scenes(&mut c, &vm, "");
        for (o, ch) in &vm.world.scenes.log {
            let _ = writeln!(
                names,
                "## scene change on {}: {ch:?}",
                vm.world.objects[*o].label
            );
        }
    }
    // Registrations left at the end, by what would raise their event.
    let raised = ["onstageset", "onquestinit", "onquestshutdown"];
    for r in &vm.world.events.registrations {
        let what = if r.custom {
            "custom event (delivered when its sender sends it)".to_string()
        } else if raised.contains(&r.event.as_str()) {
            format!("remote {}: raised by this run", r.event)
        } else {
            format!("remote {}: raised by the world, not delivered", r.event)
        };
        bump(&mut c, format!("registrations at the end\t{what}"));
    }
    drop(vm);
    count_conditions(&mut c, &host);
    count_story(&mut c, &host);
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

/// The GLOB whose editor ID the caller named: the role
/// `vm_timer_time_scale_v1` gives it is an assumption of the rule.
fn time_scale_global(store: &Store, b: &Budget, name: &str) -> Option<FormKey> {
    store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"GLOB")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(name))
        })
}

/// The time scale a run has: the value of the last
/// `GlobalVariable.SetValue` of the global with a determined argument, or
/// the stored FLTV when no native of the run touched the global; `None`
/// when another native touched it after that or the argument is
/// undetermined (`vm_timer_time_scale_v1`).
fn time_scale(vm: &Vm<'_>, store: &Store, g: &FormKey, _b: &Budget) -> Option<f64> {
    let key = bind::record_key(g);
    let touched: Vec<&Effect> = vm
        .effects
        .iter()
        .filter(|e| {
            e.receiver_object
                .and_then(|o| vm.world.objects[o].key.as_ref())
                .is_some_and(|k| *k == key)
                || e.args.iter().any(|a| a.contains(&key))
        })
        .collect();
    match touched.last() {
        None => fo4_vm::data::global_fltv(store, g, store.limits.max_memory)
            .ok()
            .map(f64::from),
        Some(e) if e.native == "globalvariable.setvalue" => {
            e.args.first().and_then(|a| a.parse::<f64>().ok())
        }
        Some(e) if e.native == "globalvariable.getvalue" => {
            // A read changes nothing: the value before it stands.
            let before: Vec<&&Effect> = touched
                .iter()
                .filter(|x| x.native != "globalvariable.getvalue")
                .collect();
            match before.last() {
                None => fo4_vm::data::global_fltv(store, g, store.limits.max_memory)
                    .ok()
                    .map(f64::from),
                Some(x) if x.native == "globalvariable.setvalue" => {
                    x.args.first().and_then(|a| a.parse::<f64>().ok())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Counts the outcome of one run and its blocking native or rule.
fn count_outcome(c: &mut Counts, prefix: &str, o: &Outcome) {
    bump(c, format!("{prefix}outcome: {}", outcome_kind(o)));
    match o {
        Outcome::Blocked(b) => match &b.on {
            Origin::Native(n) => bump(c, format!("{prefix}blocking native\t{n}")),
            Origin::Rule(r, _) => bump(c, format!("{prefix}blocking rule\t{r}")),
            Origin::NoInstance(_, kind) => bump(
                c,
                format!("{prefix}blocking object without the script\t{kind}"),
            ),
        },
        Outcome::Failed(e) => bump(c, format!("{prefix}failure\t{}", e.kind())),
        Outcome::Completed(_) => {}
    }
}

/// Counts the quest changes a run made, and which quests it started or
/// set a stage of (names to the names file).
fn count_quest_log(c: &mut Counts, vm: &Vm<'_>, prefix: &str, names: &mut String) {
    for (q, ch) in &vm.world.quests.log {
        let what = match ch {
            Change::StageSet(n) => {
                let _ = writeln!(names, "    stage {n} set on {}", vm.world.objects[*q].label);
                "stage set"
            }
            Change::Started => {
                let _ = writeln!(names, "    started {}", vm.world.objects[*q].label);
                "quest started"
            }
            Change::StageRepeatRefused(_) => "stage already done, not set again",
            Change::StartedByStory => "quest started by the story manager",
            Change::Stopped => "quest stopped",
            Change::Objective(quest::DISPLAYED, _, Some(true)) => "objective displayed",
            Change::Objective(quest::DISPLAYED, _, _) => "objective hidden or undetermined",
            Change::Objective(quest::COMPLETED, _, _) => "objective completed (or not)",
            Change::Objective(..) => "objective failed (or not)",
            Change::Opened(_) => "quest state became undetermined",
        };
        bump(c, format!("{prefix}quest changes\t{what}"));
    }
}

/// `fragments`: every fragment of every winning INFO, PACK and PERK block,
/// each in a fresh interpreter (isolated scenario, quest host, scenes on
/// as the options say): the fragment script instance on the record's
/// handle with the block's values (`vmad_fragment_instance_v1`), the
/// function called with an undetermined value for each parameter (the
/// speaker, actor or target is not known outside the world), then the
/// jobs it brings, up to [`MAX_JOBS`].
fn fragments(m: &Manifest, vfs: &Vfs, store: &Store, out: &Path, opts: Opts) -> Result<(), E> {
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let keys: Vec<(FormKey, [u8; 4])> = store
        .winners()
        .filter(|w| !w.deleted && matches!(&w.signature, b"INFO" | b"PACK" | b"PERK"))
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
            )
        })
        .collect();
    for (k, sig) in &keys {
        let t = String::from_utf8_lossy(sig).into_owned();
        let bb = b.child();
        let Some((r, _)) = store.raw_with_plugin(k, &bb)? else {
            continue;
        };
        let Some(sub) = r.subrecords.iter().find(|x| x.signature == *b"VMAD") else {
            continue;
        };
        let v = fo4_plugin::vmad::Vmad::read(
            r.data(sub),
            r.signature,
            &r.source.file,
            sub.offset,
            &bb,
        )?;
        let list: Vec<(String, String)> = match (&v.info, &v.package, &v.perk) {
            (Some(x), _, _) | (_, Some(x), _) => x
                .fragments
                .iter()
                .map(|(bit, f)| {
                    let slot = match *bit {
                        fo4_plugin::vmad::FRAGMENT_BEGIN => "begin",
                        fo4_plugin::vmad::FRAGMENT_END => "end",
                        _ => "change",
                    };
                    (slot.to_string(), fo4_pex::index::lower(f.function))
                })
                .collect(),
            (_, _, Some(p)) => p
                .fragments
                .iter()
                .map(|f| {
                    (
                        "perk entry".to_string(),
                        fo4_pex::index::lower(f.fragment.function),
                    )
                })
                .collect(),
            _ => continue,
        };
        let ty = match sig {
            b"INFO" => "topicinfo",
            b"PACK" => "package",
            _ => "perk",
        };
        for (slot, function) in list {
            let prefix = format!("{t} {slot} fragment ");
            bump(&mut c, format!("{prefix}runs"));
            let mut host = host(store, max, opts, &tree);
            let mut attach = RecordScripts::new(store, max);
            let mut runner = quest::Runner::new(store, max, MAX_JOBS);
            let mut vm = Vm::new(&mut lib, &mut host);
            vm.attach = Some(&mut attach);
            if opts.nested {
                vm.nested = Some(&mut runner);
            }
            events_on(&mut vm, opts);
            let o = fo4_vm::data::record_object(&mut vm.world, store, k, ty, &bb)?;
            let mut notes = Vec::new();
            let label = vm.world.objects[o].label.clone();
            let _ = writeln!(names, "## {label} {slot} {function}");
            let inst = match scene::record_fragment_instance(&mut vm, store, o, &mut notes, &bb) {
                Ok(i) => i,
                Err(e) => {
                    bump(&mut c, format!("{prefix}not bound\t{}", e.kind()));
                    let _ = writeln!(names, "    not bound: {e}");
                    continue;
                }
            };
            let f = match vm.find_handler(inst, &function) {
                Ok(Some(f)) => f,
                Ok(None) => {
                    bump(&mut c, format!("{prefix}function not found"));
                    let _ = writeln!(names, "    function not found");
                    continue;
                }
                Err(e) => {
                    bump(&mut c, format!("{prefix}function not found\t{}", e.kind()));
                    continue;
                }
            };
            let args: Vec<fo4_vm::Val> = f
                .params
                .iter()
                .map(|(_, ty)| {
                    vm.world.unknown(Origin::Rule(
                        fo4_vm::rules::FRAGMENT_INSTANCE,
                        format!("the {ty} a fragment receives is not known outside the world"),
                    ))
                })
                .collect();
            let start = vm.effects.len();
            let outcome = vm.deliver(inst, &function, args);
            count_outcome(&mut c, &prefix, &outcome);
            let _ = writeln!(names, "    {}", outcome_text(&outcome));
            for e in &vm.effects[start..] {
                let _ = writeln!(
                    names,
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
            let top = quest::run_jobs(&mut vm, store, MAX_JOBS, &bb);
            let runs = all_runs(&mut vm, top);
            count_jobs(&mut c, &runs, &format!("{prefix}job "));
            for r in &runs {
                names.push_str(&job_text(&vm, r));
            }
            count_effects(&mut c, &vm, &prefix);
            count_quest_log(&mut c, &vm, &prefix, &mut names);
            if opts.scenes {
                count_scenes(&mut c, &vm, &prefix);
            }
            count_events(&mut c, &vm);
            drop(vm);
            count_conditions(&mut c, &host);
            count_story(&mut c, &host);
        }
    }
    write(out, &c, &names, "fragments")
}

/// `scenes`: every winning scene started in a fresh interpreter (isolated
/// scenario, quest host): its owning quest taken as running and the scene
/// as not playing (`quest_state_isolated_v1`, as the stage sweep takes the
/// quest it sets a stage of), `Scene.Start` asked of the host, then the
/// steps and jobs it brings, up to [`MAX_JOBS`].
fn scenes(m: &Manifest, vfs: &Vfs, store: &Store, out: &Path, opts: Opts) -> Result<(), E> {
    if !opts.scenes {
        return Err("the scenes sweep needs scenes on".into());
    }
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"SCEN")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    for k in &keys {
        let bb = b.child();
        bump(&mut c, "scenes started by the sweep");
        let mut host = host(store, max, opts, &tree);
        let mut attach = RecordScripts::new(store, max);
        let mut runner = quest::Runner::new(store, max, MAX_JOBS);
        let mut vm = Vm::new(&mut lib, &mut host);
        vm.attach = Some(&mut attach);
        if opts.nested {
            vm.nested = Some(&mut runner);
        }
        events_on(&mut vm, opts);
        let o = fo4_vm::data::record_object(&mut vm.world, store, k, "scene", &bb)?;
        let Some(info) = scene::info(store, max, &mut vm.world, o) else {
            bump(&mut c, "scenes whose record does not read");
            continue;
        };
        if let Some(q) = &info.quest {
            if let Some(qi) = quest::info_by_key(store, max, &mut vm.world, q) {
                vm.world.quests.state(&qi).running = Some(true);
                vm.rules.insert(fo4_vm::rules::QUEST_ISOLATED);
            }
        }
        vm.world.scenes.states.insert(
            bind::record_key(k),
            scene::SceneState {
                playing: Some(false),
                next: 0,
            },
        );
        let call = fo4_vm::NativeCall {
            script: "scene".into(),
            name: "start".into(),
            receiver: Some(fo4_vm::Val::Object(o)),
            instance: None,
            args: Vec::new(),
            param_types: Vec::new(),
            return_type: "none".into(),
        };
        let _ = vm.host.call(&call, &mut vm.world);
        let _ = writeln!(names, "## {}", vm.world.objects[o].label);
        let top = quest::run_jobs(&mut vm, store, MAX_JOBS, &bb);
        let runs = all_runs(&mut vm, top);
        count_jobs(&mut c, &runs, "");
        for r in &runs {
            names.push_str(&job_text(&vm, r));
        }
        for (_, ch) in &vm.world.scenes.log {
            let _ = writeln!(names, "    scene change: {ch:?}");
        }
        count_effects(&mut c, &vm, "");
        count_quest_log(&mut c, &vm, "", &mut names);
        count_scenes(&mut c, &vm, "");
        let entered = vm
            .world
            .scenes
            .log
            .iter()
            .filter(|(s, ch)| *s == o && matches!(ch, scene::Change::PhaseEntered(_)))
            .count();
        bump(
            &mut c,
            format!("phases of the started scene entered\t{}", bucket(entered)),
        );
        if vm
            .world
            .scenes
            .log
            .iter()
            .any(|(s, ch)| *s == o && *ch == scene::Change::Ended)
        {
            bump(&mut c, "started scenes that ran to their end");
        }
        count_events(&mut c, &vm);
        drop(vm);
        count_conditions(&mut c, &host);
        count_story(&mut c, &host);
    }
    write(out, &c, &names, "scenes")
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
    opts: Opts,
) -> Result<(), E> {
    let nested = opts.nested;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
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
    let mut host = host(store, max, opts, &tree);
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    events_on(&mut vm, opts);
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
    let flag = |a: &mut Vec<String>, f: &str| {
        let on = a.iter().any(|x| x == f);
        a.retain(|x| x != f);
        on
    };
    let no_custom = flag(&mut a, "--no-custom");
    let custom_nested = flag(&mut a, "--custom-nested");
    let opts = Opts {
        nested: !flag(&mut a, "--queued"),
        conditions: !flag(&mut a, "--no-conditions"),
        stage_event: !flag(&mut a, "--no-stage-event"),
        remote: !flag(&mut a, "--no-remote"),
        custom: match (no_custom, custom_nested) {
            (true, _) => None,
            (false, true) => Some(CustomOrder::Nested),
            (false, false) => Some(CustomOrder::Queued),
        },
        story: !flag(&mut a, "--no-story"),
        enable_chain: !flag(&mut a, "--no-enable-chain"),
        scenes: !flag(&mut a, "--no-scenes"),
        scenes_with_quest: !flag(&mut a, "--no-scenes-with-quest"),
    };
    let mut then = Then::default();
    while let Some(i) = a
        .iter()
        .position(|x| x == "--then" || x == "--timers" || x == "--time-scale-global")
    {
        let v = a
            .get(i + 1)
            .cloned()
            .ok_or("--then, --timers and --time-scale-global take a value")?;
        if a[i] == "--then" {
            let (q, n) = v.split_once(':').ok_or("--then EDITOR_ID:STAGE")?;
            then.stages.push((q.to_string(), n.parse()?));
        } else if a[i] == "--timers" {
            then.timers = v.parse()?;
        } else {
            then.time_scale = Some(v);
        }
        a.drain(i..i + 2);
    }
    let usage = "quest_run MANIFEST (stages OUT_DIR | new-game OUT_DIR | fragments OUT_DIR | scenes OUT_DIR | QUEST STAGE) [--queued] [--no-conditions] [--no-stage-event] [--no-remote] [--no-custom | --custom-nested] [--no-story] [--no-enable-chain] [--no-scenes] [--no-scenes-with-quest]; fragments OUT_DIR and scenes OUT_DIR run every INFO, PACK and PERK fragment and start every scene in a fresh interpreter; new-game also takes --then EDITOR_ID:STAGE (repeated) --timers N and --time-scale-global EDITOR_ID, assumptions of the caller";
    if a.len() != 3 {
        return Err(usage.into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    if opts.scenes {
        let g = Rc::new(scene::Groups::read(&store));
        GROUPS.with(|x| *x.borrow_mut() = Some(g));
    }
    match a[1].as_str() {
        "stages" => stages(&m, &vfs, &store, Path::new(&a[2]), opts),
        "new-game" => new_game(&m, &vfs, &store, Path::new(&a[2]), opts, &then),
        "fragments" => fragments(&m, &vfs, &store, Path::new(&a[2]), opts),
        "scenes" => scenes(&m, &vfs, &store, Path::new(&a[2]), opts),
        q => one(&m, &vfs, &store, q, a[2].parse()?, opts),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
