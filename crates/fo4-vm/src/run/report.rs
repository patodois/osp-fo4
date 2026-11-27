//! Counts and texts of a quest run's outputs: the census lines, the job
//! texts of the names file, and the two files `write` puts in OUT_DIR.
use super::*;

pub(super) fn bucket(n: usize) -> &'static str {
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
pub(super) fn effect_kind(e: &Effect, w: &World) -> String {
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

pub(super) fn outcome_kind(o: &Outcome) -> String {
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
pub(super) fn job_text(vm: &Vm<'_>, r: &JobRun) -> String {
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
                Step::Enter { .. } => "step: offer the next phase".into(),
                Step::Complete { .. } => "step: complete the phase".into(),
                Step::LineFields => "step: line stage fields the clock reached".into(),
                Step::LineFragment {
                    line,
                    function,
                    slot,
                    ..
                } => format!("line fragment {function} ({slot}) of {line}"),
                Step::LineScript { line, event, .. } => {
                    format!("line event {event} to the line's own scripts of {line}")
                }
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
pub(super) fn count_jobs(c: &mut Counts, runs: &[JobRun], prefix: &str) {
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
            Job::Scene {
                step: Step::LineFragment { slot, .. },
                ..
            } => format!("line fragment ({slot})"),
            Job::Scene {
                step: Step::LineScript { event, .. },
                ..
            } => format!("line own scripts ({event})"),
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
pub(super) fn count_effects(c: &mut Counts, vm: &Vm<'_>, prefix: &str) {
    for e in &vm.effects {
        bump(c, format!("{prefix}effects\t{}", effect_kind(e, &vm.world)));
    }
}

/// The jobs run in order of starting: those the driver ran and those a
/// [`quest::Runner`] ran inside them.
pub(super) fn all_runs(vm: &mut Vm<'_>, mut top: Vec<JobRun>) -> Vec<JobRun> {
    top.append(&mut vm.world.quests.nested_runs);
    top.sort_by_key(|r| r.seq);
    top
}

pub(super) fn quests(store: &Store, b: &Budget) -> Result<Vec<(FormKey, QuestInfo)>, E> {
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

/// Counts the readings turned on by the switches of the second opening
/// step, when on.
pub(super) fn count_switches(c: &mut Counts, o: Options) {
    let n = o.none_arrays;
    for (on, what) in [
        (
            o.child_flag_ignored,
            "a flag 0x800 below an enable chain's root carries no state of its own (--enable-child-flag-ignored)",
        ),
        (
            n.length,
            "the length of a none array is 0 (--none-array-length)",
        ),
        (
            n.read,
            "an element of a none array is the element type's default (--none-array-read)",
        ),
        (
            n.find,
            "a find in a none array gives -1 (--none-array-find)",
        ),
        (
            n.change,
            "changing a none array does nothing (--none-array-change)",
        ),
        (
            o.line_fields,
            "the TIQS of the line a timed dialogue action plays set stages (--line-stage-fields)",
        ),
        (
            o.lines.fragments,
            "a line the run plays runs its begin and end fragments (--line-fragments)",
        ),
        (
            o.lines.start_scene,
            "a line the run plays starts the scene its TSCE names as it ends (--line-start-scene)",
        ),
        (
            o.action_percent,
            "condition function 131 reads how far a scene action has run on the clock (--cond-131-clock)",
        ),
        (
            o.conditions_end,
            "a phase with completion conditions ends when they hold, its timed actions running (--phase-conditions-end)",
        ),
        (
            o.texts_ignore_case,
            "== on texts that differ only in case is true (--text-equal-ignore-case)",
        ),
        (
            o.auto_state_inherited,
            "an instance of a script declaring no auto state starts in its nearest ancestor's (--auto-state-inherited)",
        ),
        (
            o.property_implicit_instance,
            "a property on an object without an instance of its script runs on one made for it when the script's chain declares no variable (--property-implicit-instance)",
        ),
    ] {
        if on {
            bump(c, format!("switch: {what}"));
        }
    }
}

/// Counts the caller's scene clock assumptions, when made.
pub(super) fn count_clock(c: &mut Counts, steps: usize, k: &scene::Clock) {
    if steps == 0 {
        return;
    }
    bump(
        c,
        "assumption from the command line: the scene clock advanced (--scene-clock)",
    );
    {
        if !k.timers {
            bump(c, "switch: timer actions not timed (--no-timer-actions)");
        }
        if k.voice.is_none() {
            bump(
                c,
                "switch: lines not timed by their audio (--no-line-audio)",
            );
        }
        if let Some(x) = k.assume_line_seconds {
            bump(
                c,
                format!("assumption from the command line: a line whose audio does not settle its time lasts {x} s"),
            );
        }
        if let Some(n) = k.assume_player_choice {
            bump(
                c,
                format!("assumption from the command line: the player picks choice {n}"),
            );
        }
        if let Some(n) = k.package_arrival {
            bump(
                c,
                format!("switch: a package action's actor is at its package's one placed Location reference {n} s (the caller's) after the action starts (--package-destinations)"),
            );
        }
        if k.package_after_stop {
            bump(
                c,
                "switch: a package action's scheduled actor move stands when a script stops its scene first (--package-moves-outlive-scene)",
            );
        }
        if k.assume_packages {
            bump(
                c,
                "assumption from the command line: package actions complete as they start",
            );
        }
        if k.assume_start_scenes {
            bump(
                c,
                "assumption from the command line: start-scene actions complete as they start",
            );
        }
        if k.start_skipped {
            bump(
                c,
                "switch: an action whose start phase was skipped starts with the first later phase it spans that is entered (--scene-actions-skipped-start)",
            );
        }
        if k.no_line_ends {
            bump(
                c,
                "switch: a dialogue action whose topic holds lines none of whose conditions hold ends as it starts (--no-line-holds-ends)",
            );
        }
        if k.npc_responses {
            bump(
                c,
                "switch: a response action plays the topic at the index of the player's assumed choice in its scene, timed as a line (--npc-response-lines)",
            );
        }
        if k.assume_first_line {
            bump(
                c,
                "assumption from the command line: where several lines of a topic may hold, the first plays (--assume-first-line)",
            );
        }
        // The scene or quest is named by its place among the caller's
        // names in key order, never by its record key.
        for (i, n) in k.choice_for.values().enumerate() {
            bump(
                c,
                format!("assumption from the command line: the player picks choice {n} in the scene or the scenes of the quest the caller names {} of {} (--assume-player-choice-for, in key order)", i + 1, k.choice_for.len()),
            );
        }
        for i in 0..k.first_line_for.len() {
            bump(
                c,
                format!("assumption from the command line: where several lines of a topic may hold in the scene or the scenes of the quest the caller names {} of {}, the first plays (--assume-first-line-for, in key order)", i + 1, k.first_line_for.len()),
            );
        }
    }
}

/// The counts of what scenes did in a run.
pub(super) fn count_scenes(c: &mut Counts, vm: &Vm<'_>, prefix: &str) {
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
            scene::Change::ActionStarted(_, _, k, how) => {
                format!("scene clock: action started\tkind {k}\t{how}")
            }
            scene::Change::ActionCompleted(_, k, _) => {
                format!("scene clock: action completed\tkind {k}")
            }
            scene::Change::OnClock(_) => {
                "scene clock: phase waits on actions with a due time".to_string()
            }
            scene::Change::LineField(_, what, _) => {
                format!("scene clock: line stage fields\t{what}")
            }
        };
        bump(c, format!("{prefix}scenes\t{what}"));
    }
}

/// The counts of event registrations and deliveries, and of story events.
pub(super) fn count_events(c: &mut Counts, vm: &Vm<'_>) {
    for (k, v) in &vm.world.events.counts {
        *c.entry(format!("events\t{k}")).or_default() += v;
    }
}
pub(super) fn count_story(c: &mut Counts, h: &QuestHost<'_>) {
    for (k, v) in &h.story_counts {
        *c.entry(format!("story\t{k}")).or_default() += v;
    }
}

/// The condition counts of a host.
pub(super) fn count_conditions(c: &mut Counts, h: &QuestHost<'_>) {
    for (k, v) in &h.conditions {
        *c.entry(k.clone()).or_default() += v;
    }
    for (f, v) in &h.open_functions {
        *c.entry(format!(
            "log entry selections left open, by function index of an undetermined condition\tfunction {f}"
        ))
        .or_default() += v;
    }
    // The quest is named by its place among the caller's quests in key
    // order, never by its record key.
    let quests = h.cond.assume_for.len();
    for (i, m) in h.cond.assume_for.values().enumerate() {
        for (f, x) in m {
            *c.entry(format!(
                "assumption from the command line: every condition of function {f} in the records of the quest the caller names {} of {quests} has the value {x} (--assume-cond-for, in key order)",
                i + 1
            ))
            .or_default() += 1;
        }
    }
    for (f, x) in &h.cond.assume {
        *c.entry(format!(
            "assumption from the command line: every condition of function {f} has the value {x}"
        ))
        .or_default() += 1;
    }
    if !h.cond.assume.is_empty() {
        *c.entry(
            "assumption from the command line: conditions decided by an assumed value".into(),
        )
        .or_default() += h.cond.assumed.get();
    }
}

/// Counts the outcome of one run and its blocking native or rule.
pub(super) fn count_outcome(c: &mut Counts, prefix: &str, o: &Outcome) {
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
pub(super) fn count_quest_log(c: &mut Counts, vm: &Vm<'_>, prefix: &str, names: &mut String) {
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

pub(super) fn write(out: &Path, c: &Counts, names: &str, scope: &str) -> Result<(), E> {
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
