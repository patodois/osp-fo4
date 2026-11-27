//! The sweeps (`stages`, `fragments`, `scenes`) and one stage of one
//! quest, each set in fresh interpreters.
use super::*;

/// Counts the OnStageSet handlers of every winning quest: on its own VMAD
/// scripts (to which `quest_on_stage_set_v1` delivers the event), on its
/// fragment script, and on the scripts of its aliases (which receive
/// nothing here).
pub(super) fn stage_event_handlers(
    lib: &mut VfsLibrary<'_>,
    store: &Store,
    b: &Budget,
) -> Result<Counts, E> {
    let mut c = Counts::new();
    let max = store.limits.max_memory;
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut host = crate::RecordingHost::default();
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

pub(super) fn stages(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    out: &Path,
    opts: Options,
    settings: &Settings,
) -> Result<(), E> {
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
    count_switches(&mut c, opts);
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
            let mut host = host(store, vfs, max, opts, &tree, settings);
            let mut attach = RecordScripts::new(store, max);
            let mut runner = quest::Runner::new(store, max, MAX_JOBS);
            let mut vm = Vm::new(&mut lib, &mut host);
            vm.attach = Some(&mut attach);
            if nested {
                vm.nested = Some(&mut runner);
            }
            events_on(&mut vm, opts, settings);
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

/// `fragments`: every fragment of every winning INFO, PACK and PERK block,
/// each in a fresh interpreter (isolated scenario, quest host, scenes on
/// as the options say): the fragment script instance on the record's
/// handle with the block's values (`vmad_fragment_instance_v1`), the
/// function called with an undetermined value for each parameter (the
/// speaker, actor or target is not known outside the world), then the
/// jobs it brings, up to [`MAX_JOBS`].
pub(super) fn fragments(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    out: &Path,
    opts: Options,
    settings: &Settings,
) -> Result<(), E> {
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    count_switches(&mut c, opts);
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
            let mut host = host(store, vfs, max, opts, &tree, settings);
            let mut attach = RecordScripts::new(store, max);
            let mut runner = quest::Runner::new(store, max, MAX_JOBS);
            let mut vm = Vm::new(&mut lib, &mut host);
            vm.attach = Some(&mut attach);
            if opts.nested {
                vm.nested = Some(&mut runner);
            }
            events_on(&mut vm, opts, settings);
            let o = crate::data::record_object(&mut vm.world, store, k, ty, &bb)?;
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
            let args: Vec<crate::Val> = f
                .params
                .iter()
                .map(|(_, ty)| {
                    vm.world.unknown(Origin::Rule(
                        crate::rules::FRAGMENT_INSTANCE,
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
pub(super) fn scenes(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    out: &Path,
    opts: Options,
    steps: usize,
    settings: &Settings,
) -> Result<(), E> {
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
    count_clock(&mut c, steps, &settings.clock);
    count_switches(&mut c, opts);
    for k in &keys {
        let bb = b.child();
        bump(&mut c, "scenes started by the sweep");
        let mut host = host(store, vfs, max, opts, &tree, settings);
        let mut attach = RecordScripts::new(store, max);
        let mut runner = quest::Runner::new(store, max, MAX_JOBS);
        let mut vm = Vm::new(&mut lib, &mut host);
        vm.attach = Some(&mut attach);
        if opts.nested {
            vm.nested = Some(&mut runner);
        }
        events_on(&mut vm, opts, settings);
        let o = crate::data::record_object(&mut vm.world, store, k, "scene", &bb)?;
        let Some(info) = scene::info(store, max, &mut vm.world, o) else {
            bump(&mut c, "scenes whose record does not read");
            continue;
        };
        if let Some(q) = &info.quest {
            if let Some(qi) = quest::info_by_key(store, max, &mut vm.world, q) {
                vm.world.quests.state(&qi).running = Some(true);
                vm.rules.insert(crate::rules::QUEST_ISOLATED);
            }
        }
        vm.world.scenes.states.insert(
            bind::record_key(k),
            scene::SceneState {
                playing: Some(false),
                ..Default::default()
            },
        );
        let call = crate::NativeCall {
            script: "scene".into(),
            name: "start".into(),
            receiver: Some(crate::Val::Object(o)),
            instance: None,
            args: Vec::new(),
            param_types: Vec::new(),
            return_type: "none".into(),
        };
        let _ = vm.host.call(&call, &mut vm.world);
        let _ = writeln!(names, "## {}", vm.world.objects[o].label);
        let mut top = quest::run_jobs(&mut vm, store, MAX_JOBS, &bb);
        // The scene clock, advanced up to `steps` times (an assumption of
        // the caller, as firing timers is); the jobs each brings run
        // before the next.
        let mut advanced = 0;
        while advanced < steps && top.len() < MAX_JOBS {
            if scene::advance_clock(&mut vm.world, None).is_none() {
                break;
            }
            vm.rules.insert(crate::rules::SCENE_CLOCK);
            advanced += 1;
            top.extend(quest::run_jobs(&mut vm, store, MAX_JOBS, &bb));
        }
        if steps > 0 {
            bump(
                &mut c,
                format!("scene clock advances per scene\t{}", bucket(advanced)),
            );
            if scene::next_due(&vm.world).is_some() {
                bump(
                    &mut c,
                    "scenes with actions still due when the advances ran out",
                );
            }
        }
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

pub(super) fn one(
    m: &Manifest,
    vfs: &Vfs,
    store: &Store,
    edid: &str,
    stage: i32,
    opts: Options,
    settings: &Settings,
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
    let mut host = host(store, vfs, max, opts, &tree, settings);
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    events_on(&mut vm, opts, settings);
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
