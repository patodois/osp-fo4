//! `--next-blockers`: the stages the stopped scenes wait on and what in
//! the data can set them.
use super::*;

/// The stages the stopped scenes wait on and what in the data can set each
/// (`crate::setters`), with the state each setter's need has in the run:
/// `quest-run-next-blockers.txt` (counts and kinds only) and
/// `quest-run-next-blockers-names.txt` (never committed).
#[allow(clippy::too_many_arguments)]
pub(super) fn next_blockers(
    vm: &mut Vm<'_>,
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    opts: Options,
    quests: &[String],
    outer_calls: bool,
    out: &Path,
) -> Result<(), E> {
    use crate::setters::{self, Need, On, ScenePoint};
    let b = Budget::new(m.limits.max_memory);
    let mut waits = setters::scene_waits(&mut vm.world, store);
    // The quests the caller set stages on: their next stages not done, in
    // stage order after the highest one done (up to NEXT_STAGES).
    let mut seen = std::collections::BTreeSet::new();
    for edid in quests {
        let k = quest_by_editor_id(store, &b, edid)?;
        if !seen.insert(k.clone()) {
            continue;
        }
        let Some(info) = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, &k)
        else {
            continue;
        };
        let o = quest::quest_object(vm, store, &k, &b)?;
        let st = vm.world.quests.state(&info).clone();
        let top = info
            .stages
            .iter()
            .filter(|s| st.is_done(i32::from(s.stage)) == Some(true))
            .map(|s| s.stage)
            .max();
        for s in info
            .stages
            .iter()
            .filter(|s| top.is_none_or(|t| s.stage > t))
            .filter(|s| st.is_done(i32::from(s.stage)) != Some(true))
            .take(NEXT_STAGES)
        {
            waits.push(setters::Wait {
                scene: o,
                phase: usize::MAX,
                list: "next stage of a quest the caller set a stage on",
                function: 0,
                quest: k.clone(),
                stage: i32::from(s.stage),
                done: st.is_done(i32::from(s.stage)),
            });
        }
    }
    let lib = setters::read_all_scripts(vfs, m.limits.max_memory)?;
    let (atts, _) = setters::attachments(store, &setters::owners(store), &b)?;
    let tree = story_tree(store, opts)?;
    let t = setters::table_with(&lib, &atts, store, tree.as_deref(), &b, outer_calls)?;
    let label =
        |k: &FormKey| bind::label(store, k, &b.child()).unwrap_or_else(|_| bind::record_key(k));
    let mut c = Counts::new();
    let mut names = String::new();
    *c.entry("waits: stages a stopped scene phase or the caller's quests wait on".into())
        .or_default() += waits.len() as u64;
    for w in &waits {
        let what = if w.phase == usize::MAX {
            w.list.to_string()
        } else {
            format!("{} conditions\tfunction {}", w.list, w.function)
        };
        bump(
            &mut c,
            format!(
                "waits\t{what}\tthe stage {}",
                match w.done {
                    Some(false) => "is not done in the run",
                    _ => "is undetermined in the run",
                }
            ),
        );
        let list = t.of_stage(&w.quest, w.stage);
        bump(
            &mut c,
            format!("setters of a waited stage\t{}", bucket(list.len())),
        );
        if w.phase == usize::MAX {
            let _ = writeln!(
                names,
                "## {} next stage {}: {} setter(s)",
                label(&w.quest),
                w.stage,
                list.len()
            );
        } else {
            let _ = writeln!(
                names,
                "## {} phase {} waits ({} conditions, function {}) on {} stage {}: {} setter(s)",
                vm.world.objects[w.scene].label,
                w.phase,
                w.list,
                w.function,
                label(&w.quest),
                w.stage,
                list.len()
            );
        }
        if list.is_empty() {
            bump(
                &mut c,
                "waited stages with no setter in the data: only a caller assumption passes them (--then)",
            );
        }
        for st in list {
            // The need's state in this run, and the switch that delivers
            // it when it is a player action.
            let (status, hint) = match &st.need {
                Need::Stage(q, n) => {
                    let qi = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, q);
                    let done = qi.and_then(|i| vm.world.quests.state(&i).is_done(*n));
                    (
                        match done {
                            Some(true) => "a chain: the stage is done in the run",
                            Some(false) => "a chain: the stage is not done in the run",
                            None => "a chain: the stage is undetermined in the run",
                        }
                        .to_string(),
                        format!("{} stage {n}", label(q)),
                    )
                }
                Need::QuestStart(q) => {
                    let qi = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, q);
                    let r = qi.and_then(|i| vm.world.quests.state(&i).running);
                    (
                        format!("the quest runs in the run: {r:?}"),
                        label(q),
                    )
                }
                Need::Event(e, On::Record(k, sg)) => {
                    let assumable = crate::player::Action::ALL
                        .into_iter()
                        .find(|a| a.handler() == e);
                    match (assumable, &sg[..]) {
                        (Some(a), b"REFR" | b"ACHR") => (
                            format!("a player action a caller can assume ({})", a.switch()),
                            format!("{} {}", a.switch(), bind::record_key(k)),
                        ),
                        (Some(_), _) => (
                            "a player action on a placement of a base object (which placement is the caller's)".into(),
                            label(k),
                        ),
                        _ => (event_not_raised(&vm.world, e), label(k)),
                    }
                }
                Need::Event(e, On::Alias(q, i)) => {
                    let assumable = crate::player::Action::ALL
                        .into_iter()
                        .find(|a| a.handler() == e);
                    let qi = quest::info_by_key(store, store.limits.max_memory, &mut vm.world, q);
                    let held = qi.and_then(|info| {
                        let s = vm.world.quests.state(&info);
                        match s.fills.get(&i32::from(*i)) {
                            Some(crate::Val::Object(o)) => {
                                vm.world.objects[*o].key.clone()
                            }
                            Some(_) => None,
                            None if s.running == Some(true) => vm
                                .world
                                .quests
                                .fill_location(&info, u32::try_from(*i).unwrap_or(u32::MAX))
                                .ok()
                                .flatten()
                                .map(|k| bind::record_key(&k)),
                            None => None,
                        }
                    });
                    match (assumable, held) {
                        (Some(a), Some(r)) => (
                            format!(
                                "a player action on the reference an alias holds in the run, a caller can assume ({})",
                                a.switch()
                            ),
                            format!("{} {r}", a.switch()),
                        ),
                        (Some(_), None) => (
                            "a player action on an alias that holds no reference in the run".into(),
                            format!("alias {i} of {}", label(q)),
                        ),
                        _ => (
                            event_not_raised(&vm.world, e),
                            format!("alias {i} of {}", label(q)),
                        ),
                    }
                }
                Need::Scene(k, p) => {
                    let o = crate::data::record_object(&mut vm.world, store, k, "scene", &b)?;
                    let changes: Vec<&scene::Change> = vm
                        .world
                        .scenes
                        .log
                        .iter()
                        .filter(|(x, _)| *x == o)
                        .map(|x| &x.1)
                        .collect();
                    let reached = match p {
                        ScenePoint::Begin => changes
                            .iter()
                            .any(|c| matches!(c, scene::Change::Started | scene::Change::StartedWithQuest)),
                        ScenePoint::End => changes.iter().any(|c| matches!(c, scene::Change::Ended)),
                        ScenePoint::Phase(i, _) | ScenePoint::PhaseField(i, false) => changes
                            .iter()
                            .any(|c| matches!(c, scene::Change::PhaseEntered(x) if *x == *i as usize)),
                        ScenePoint::PhaseField(i, true) => changes
                            .iter()
                            .any(|c| matches!(c, scene::Change::PhaseCompleted(x) if *x == *i as usize)),
                    };
                    (
                        if changes.is_empty() {
                            "a scene the run never started".to_string()
                        } else if reached {
                            "a scene point the run reached".to_string()
                        } else {
                            "a scene point the run has not reached".to_string()
                        },
                        need_target_text(&st.need, &label),
                    )
                }
                n @ Need::Line(..) => (
                    "a dialogue line: the interpreter speaks no line itself (only scene actions time them)".into(),
                    need_target_text(n, &label),
                ),
                n => (
                    "an event the interpreter does not raise".into(),
                    need_target_text(n, &label),
                ),
            };
            bump(
                &mut c,
                format!(
                    "setters of a waited stage by kind and need\t{}\t{}",
                    st.kind,
                    st.need.text(store)
                ),
            );
            bump(
                &mut c,
                format!("setters of a waited stage: the need in this run\t{status}"),
            );
            let _ = writeln!(
                names,
                "    {} | {} | {status} | {hint} | {}",
                st.kind,
                st.need.text(store),
                st.site
            );
        }
    }
    // Every scene stopped at the end, by reason; for a stop on conditions,
    // the function indexes of its list.
    for (o, (p, why)) in setters::scene_stops(&vm.world) {
        let reason = why.split(" (").next().unwrap_or("").to_string();
        bump(&mut c, format!("scenes stopped at the end\t{reason}"));
        let funcs = scene::info(store, store.limits.max_memory, &mut vm.world, o)
            .and_then(|i| {
                let ph = i.phases.get(p)?;
                let list = if why.starts_with("start") {
                    &ph.start
                } else {
                    &ph.completion
                };
                Some(
                    list.iter()
                        .map(|c| c.ctda.function.to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .unwrap_or_default();
        let _ = writeln!(
            names,
            "# stopped: {} phase {p}: {why}; condition functions of the list: [{funcs}]",
            vm.world.objects[o].label
        );
    }
    let mut s = String::new();
    writeln!(
        s,
        "# quest_run new-game --next-blockers: counts and kinds only (spec/vm.md, Stage setters)"
    )?;
    for (k, v) in &c {
        writeln!(s, "{v}\t{k}")?;
    }
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("quest-run-next-blockers.txt"), s)?;
    std::fs::write(out.join("quest-run-next-blockers-names.txt"), names)?;
    Ok(())
}

/// The record a setter's need names, with its label.
/// The need of an engine event no caller assumption delivers: with the
/// world model's distance events on, a distance event is raised from the
/// track once its registration is made.
pub(super) fn event_not_raised(w: &World, e: &str) -> String {
    if w.place.on && w.place.sw.distance && e.starts_with("ondistance") {
        "an engine event the world model raises from the track once registered (--distance-events)"
            .into()
    } else {
        "an engine event the interpreter does not raise".into()
    }
}

pub(super) fn need_target_text(
    n: &crate::setters::Need,
    label: &dyn Fn(&FormKey) -> String,
) -> String {
    use crate::setters::{Need, ScenePoint};
    match n {
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
        Need::Package(k, _) | Need::Perk(k, _) | Need::Terminal(k, _) | Need::QuestStart(k) => {
            label(k)
        }
        Need::Stage(k, n) => format!("{} stage {n}", label(k)),
        _ => String::new(),
    }
}
