//! What the caller makes happen from outside any script (`Caller`), its
//! gates, and the offers of stopped scenes again.
use super::*;

/// Whether a gated action's stage is done in the run (`--action-after`).
pub(super) fn gate_done(vm: &mut Vm<'_>, store: &Store, x: &Caller, b: &Budget) -> bool {
    let Some((q, n)) = &x.after else {
        return true;
    };
    let Ok(k) = quest_by_editor_id(store, b, q) else {
        return false;
    };
    vm.world
        .quests
        .states
        .get(&crate::bind::record_key(&k))
        .and_then(|s| s.is_done(*n))
        == Some(true)
}

/// With `--scene-recheck`: while the quest state changed since the last
/// offer, offers again the scenes stopped on conditions and runs the jobs
/// that brings (`scene_condition_recheck_v1`). With
/// `--scene-recheck-variables`: while scripts wrote quest script variables
/// since the last offer, offers again the scenes stopped on conditions
/// that read one of them (`scene_condition_recheck_variable_v1`; not when
/// the quest state reading has just offered every stopped scene).
pub(super) fn recheck(
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    c: &mut Counts,
    top: &mut Vec<JobRun>,
    b: &Budget,
) {
    if then.recheck.is_none() && !then.recheck_variables {
        return;
    }
    for _ in 0..16 {
        let mut n = 0;
        if let Some(seen) = &then.recheck {
            let now = vm.world.quests.log.len();
            if now != seen.get() {
                seen.set(now);
                n = scene::recheck(&mut vm.world);
                if n > 0 {
                    vm.rules.insert(crate::rules::SCENE_RECHECK);
                    *c.entry(
                        "scene recheck: scenes stopped on conditions offered again after a quest state change"
                            .into(),
                    )
                    .or_default() += n as u64;
                }
            }
        }
        if then.recheck_variables {
            let written = vm
                .world
                .scenes
                .variable_writes
                .as_mut()
                .map(std::mem::take)
                .unwrap_or_default();
            if n == 0 {
                let k = scene::recheck_variables(store, &mut vm.world, &written);
                if k > 0 {
                    vm.rules.insert(crate::rules::SCENE_RECHECK_VARIABLE);
                    *c.entry(
                        "scene recheck: scenes stopped on conditions offered again after a quest script variable they read was written"
                            .into(),
                    )
                    .or_default() += k as u64;
                }
                n = k;
            }
        }
        if n == 0 {
            break;
        }
        top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
    }
}

/// Does what the caller assumes: sets the stage or delivers the player
/// action's event, then runs the jobs it brings. Counted as an assumption.
#[allow(clippy::too_many_arguments)]
pub(super) fn act(
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    x: &Caller,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) -> Result<(), E> {
    let when = x.at.map(|t| format!("@{t}")).unwrap_or_default();
    match &x.what {
        What::Stage(edid, n) => {
            bump(
                c,
                if x.at.is_some() {
                    "assumption from the command line: a stage set from outside any script at a second of the real-time clock"
                } else {
                    "assumption from the command line: a stage set from outside any script after the start"
                },
            );
            let k = quest_by_editor_id(store, b, edid)?;
            let o = quest::quest_object(vm, store, &k, b)?;
            let reply = quest::set_stage(vm, store, o, *n);
            let _ = writeln!(names, "## --then {edid}:{n}{when}: {reply:?}");
        }
        What::Anim(key, event) => {
            bump(
                c,
                "assumption from the command line: a placed reference raises a named animation event (--assume-anim-event)",
            );
            // `player`: the player's handle raises it (the caller's).
            let o = if key == "player" {
                bump(
                    c,
                    "assumption from the command line: the player raises a named animation event (--assume-anim-event player)",
                );
                vm.world.player("actor")
            } else {
                let k = crate::player::parse_key(key)
                    .ok_or("a reference is given as plugin.esm:0001ABCD or player")?;
                if !matches!(
                    store.signature(&k).as_ref().map(|s| &s[..]),
                    Some(b"REFR" | b"ACHR")
                ) {
                    return Err("--assume-anim-event names no winning placed REFR or ACHR".into());
                }
                crate::data::record_object(&mut vm.world, store, &k, "objectreference", b)?
            };
            let before = vm.effects.len();
            let runs = crate::anim::deliver(vm, o, event);
            let _ = writeln!(
                names,
                "## --assume-anim-event {key} {event}{when}: {} handler run(s)",
                runs.len()
            );
            if runs.is_empty() {
                bump(
                    c,
                    "animation events\tassumed by the caller\tno registered instance",
                );
            }
            for (script, o) in &runs {
                bump(
                    c,
                    format!(
                        "animation events\tassumed by the caller\t{}",
                        outcome_kind(o)
                    ),
                );
                let _ = writeln!(names, "    {script}: {}", outcome_text(o));
            }
            top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
            for e in &vm.effects[before..] {
                let _ = writeln!(
                    names,
                    "      {}{}({}{}) = {}",
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
        }
        What::Talk(key) => {
            bump(
                c,
                "assumption from the command line: the player starts a conversation with a placed actor (--assume-talk)",
            );
            let k = crate::player::parse_key(key)
                .ok_or("a reference is given as plugin.esm:0001ABCD")?;
            if store.signature(&k) != Some(*b"ACHR") {
                return Err("--assume-talk names no winning placed actor (ACHR)".into());
            }
            let o = crate::data::record_object(&mut vm.world, store, &k, "actor", b)?;
            let before = (vm.world.talk.log.len(), vm.effects.len());
            let _ = vm.host.call(&crate::talk::talk_call(o), &mut vm.world);
            for r in vm.host.rules() {
                vm.rules.insert(r);
            }
            let _ = writeln!(names, "## --assume-talk {key}{when}");
            for (t, x) in &vm.world.talk.log[before.0..] {
                let _ = writeln!(names, "    talk at {t} s: {x}");
            }
            for e in &vm.effects[before.1..] {
                let _ = writeln!(
                    names,
                    "      {}{}({}{}) = {}",
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
        }
        What::Sit(key, sitting) => {
            let (switch, text) = if *sitting {
                (
                    "--assume-sit",
                    "the player sits in a placed furniture reference",
                )
            } else {
                (
                    "--assume-get-up",
                    "the player gets up from a placed furniture reference",
                )
            };
            bump(
                c,
                format!("assumption from the command line: {text} ({switch})"),
            );
            let k = crate::player::parse_key(key)
                .ok_or("a reference is given as plugin.esm:0001ABCD")?;
            let base = bind::base_of(store, &k, b)?;
            if store.signature(&k) != Some(*b"REFR")
                || base.and_then(|x| store.signature(&x)) != Some(*b"FURN")
            {
                return Err(format!(
                    "{switch} names no winning placed furniture (a REFR of a FURN)"
                )
                .into());
            }
            let f = crate::data::record_object(&mut vm.world, store, &k, "objectreference", b)?;
            let p = vm.world.player("actor");
            let raised = if *sitting {
                crate::sit::sit(vm, store, p, f, b)?
            } else {
                crate::sit::get_up(vm, store, p, f, b)?
            };
            let _ = writeln!(names, "## {switch} {key}{when}");
            sit_log(vm, &raised, c, names);
        }
        What::Hit(..) | What::Combat(..) | What::Death(..) | What::Unlock(_) => {
            fight_act(vm, store, &x.what, &when, c, names, b)?;
            top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
        }
        What::Terminal(..) | What::ItemAdded(..) => {
            stop_act(vm, store, &x.what, &when, c, names, b)?;
            top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
        }
        What::Menu(name, opening) => {
            bump(
                c,
                "assumption from the command line: a menu of the caller's name opens or closes (--assume-menu)",
            );
            let runs = crate::menu::open_close(vm, name, *opening);
            let _ = writeln!(
                names,
                "## --assume-menu {name} {}{when}: {} handler run(s)",
                if *opening { "open" } else { "close" },
                runs.len()
            );
            for (script, o) in &runs {
                bump(
                    c,
                    format!("menu events	assumed by the caller	{}", outcome_kind(o)),
                );
                let _ = writeln!(names, "    {script}: {}", outcome_text(o));
            }
            if runs.is_empty() {
                bump(
                    c,
                    "menu events	assumed by the caller	no registered instance",
                );
            }
        }
        What::Looks(n) => {
            bump(
                c,
                "assumption from the command line: the appearance menu raises its event with the caller's number (--assume-looks-menu-event)",
            );
            let runs = crate::menu::looks(vm, *n);
            let _ = writeln!(
                names,
                "## --assume-looks-menu-event {n}{when}: {} handler run(s)",
                runs.len()
            );
            for (script, o) in &runs {
                bump(
                    c,
                    format!("menu events	assumed by the caller	{}", outcome_kind(o)),
                );
                let _ = writeln!(names, "    {script}: {}", outcome_text(o));
            }
            if runs.is_empty() {
                bump(
                    c,
                    "menu events	assumed by the caller	no registered instance",
                );
            }
        }
        What::Player(action, key) => {
            use crate::player;
            bump(
                c,
                format!(
                    "assumption from the command line: {} ({})",
                    action.text(),
                    action.switch()
                ),
            );
            let k = player::parse_key(key).ok_or("a reference is given as plugin.esm:0001ABCD")?;
            if *action == player::Action::Activate && store.signature(&k) == Some(*b"ACHR") {
                if !then.activate_actors {
                    return Err(
                        "--assume-activate on a placed actor needs --activate-actors".into(),
                    );
                }
                vm.rules.insert(crate::rules::EVENT_ACTIVATE_ACTOR);
            }
            let before = vm.effects.len();
            let (runs, open) = player::deliver(vm, store, &k, *action, b)?;
            let _ = writeln!(
                names,
                "## {} {key}{when}: {} handler run(s)",
                action.switch(),
                runs.len()
            );
            if runs.is_empty() {
                bump(
                    c,
                    format!("player events	{}	no script has a handler", action.handler()),
                );
            }
            if open > 0 {
                *c.entry(format!(
                    "player events	{}	quests left out: whether they run is undetermined",
                    action.handler()
                ))
                .or_default() += open as u64;
            }
            for r in &runs {
                bump(
                    c,
                    format!(
                        "player events	{}	{}	{}",
                        action.handler(),
                        r.receiver.kind(),
                        outcome_kind(&r.outcome)
                    ),
                );
                let on = match r.receiver {
                    crate::player::Receiver::Alias(q, id) => {
                        format!(" (alias {id} of {})", vm.world.objects[q].label)
                    }
                    _ => String::new(),
                };
                let _ = writeln!(
                    names,
                    "    {} {}{on}: {}",
                    r.receiver.kind(),
                    r.script,
                    outcome_text(&r.outcome)
                );
            }
            for e in &vm.effects[before..] {
                let _ = writeln!(
                    names,
                    "      {}{}({}{}) = {}",
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
        }
    }
    top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
    lock_changes(vm, store, c, names, top, b)?;
    recheck(vm, store, then, c, top, b);
    Ok(())
}

/// Under `vm_lock_state_event_script_v1`: raises the lock state change
/// events the scripts' `Lock` calls left waiting, runs the jobs they bring,
/// and again for the changes those make (at most 16 rounds).
pub(super) fn lock_changes(
    vm: &mut Vm<'_>,
    store: &Store,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) -> Result<(), E> {
    for _ in 0..16 {
        if vm.world.runstate.lock_changed.is_empty() {
            break;
        }
        let before = vm.effects.len();
        let raised = crate::runstate::deliver_lock_changes(vm, store, b)?;
        log_raised(vm, &raised, "lock events", before, c, names);
        top.extend(quest::run_jobs(vm, store, MAX_GAME_JOBS, b));
    }
    Ok(())
}

/// Writes events raised outside any script (`Raised`) and their handler
/// runs under a counts prefix.
fn log_raised(
    vm: &Vm<'_>,
    raised: &[crate::fight::Raised],
    prefix: &str,
    before: usize,
    c: &mut Counts,
    names: &mut String,
) {
    for r in raised {
        let _ = writeln!(
            names,
            "## {prefix} at {} s: {}, {} handler run(s)",
            r.t,
            r.what,
            r.runs.len()
        );
        if r.runs.is_empty() {
            bump(c, format!("{prefix}\tno script has a handler"));
        }
        for d in &r.runs {
            bump(
                c,
                format!(
                    "{prefix}\t{}\t{}",
                    d.receiver.kind(),
                    outcome_kind(&d.outcome)
                ),
            );
            let on = match d.receiver {
                crate::player::Receiver::Alias(q, id) => {
                    format!(" (alias {id} of {})", vm.world.objects[q].label)
                }
                _ => String::new(),
            };
            let _ = writeln!(
                names,
                "    {} {}{on}: {}",
                d.receiver.kind(),
                d.script,
                outcome_text(&d.outcome)
            );
        }
    }
    for e in &vm.effects[before..] {
        let _ = writeln!(
            names,
            "      {}{}({}{}) = {}",
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
}

/// Does what a terminal pick or an item added assumes (`crate::terminal`,
/// `crate::inventory`) and writes the handler runs it brings.
pub(super) fn stop_act(
    vm: &mut Vm<'_>,
    store: &Store,
    what: &What,
    when: &str,
    c: &mut Counts,
    names: &mut String,
    b: &Budget,
) -> Result<(), E> {
    let before = vm.effects.len();
    let (raised, prefix) = match what {
        What::Terminal(key, id) => {
            bump(
                c,
                "assumption from the command line: the player picks a terminal's menu item (--assume-terminal-item)",
            );
            let k = crate::player::parse_key(key)
                .ok_or("--assume-terminal-item takes PLUGIN:OBJECTID")?;
            if store.signature(&k) != Some(*b"REFR") {
                return Err("--assume-terminal-item names no winning placed reference".into());
            }
            let o = crate::data::record_object(&mut vm.world, store, &k, "objectreference", b)?;
            let _ = writeln!(names, "## --assume-terminal-item {key} {id}{when}");
            (
                crate::terminal::pick(vm, store, o, *id, b)?,
                "terminal items",
            )
        }
        What::ItemAdded(holder, item, n, source) => {
            bump(
                c,
                "assumption from the command line: the player puts an item into a holder (--assume-item-added)",
            );
            let h = if holder == "player" {
                vm.world.player("actor")
            } else {
                let k = crate::player::parse_key(holder)
                    .ok_or("--assume-item-added takes PLUGIN:OBJECTID or player")?;
                if !matches!(
                    store.signature(&k).as_ref().map(|s| &s[..]),
                    Some(b"REFR" | b"ACHR")
                ) {
                    return Err("--assume-item-added names no winning placed holder".into());
                }
                crate::data::record_object(&mut vm.world, store, &k, "objectreference", b)?
            };
            let ik = crate::player::parse_key(item)
                .ok_or("--assume-item-added takes the item as PLUGIN:OBJECTID")?;
            let ty = store
                .signature(&ik)
                .and_then(crate::data::signature_type)
                .ok_or("--assume-item-added names no winning item record")?;
            let i = crate::data::record_object(&mut vm.world, store, &ik, ty, b)?;
            let s = (source == "player").then(|| vm.world.player("actor"));
            let _ = writeln!(
                names,
                "## --assume-item-added {holder} {item} {n} {source}{when}"
            );
            (
                crate::inventory::item_added(vm, store, h, i, *n, s, b)?,
                "items added",
            )
        }
        _ => return Ok(()),
    };
    log_raised(vm, &[raised], prefix, before, c, names);
    Ok(())
}

/// Writes the sits and get-ups delivered and their handler runs to the
/// names file and the counts.
/// A placed actor or the player, named as the fight assumptions name them
/// (`plugin.esm:0001ABCD` of a placed actor, or `player`).
pub(super) fn fight_object(
    vm: &mut Vm<'_>,
    store: &Store,
    key: &str,
    b: &Budget,
) -> Result<crate::ObjId, E> {
    if key == "player" {
        return Ok(vm.world.player("actor"));
    }
    if key.contains('#') {
        return alias_target(vm, store, key, b)?.ok_or_else(|| {
            format!("a fight assumption on {key}: the alias holds no reference").into()
        });
    }
    let k = crate::player::parse_key(key)
        .ok_or("an actor is given as plugin.esm:0001ABCD, EDITOR_ID#ALIAS or player")?;
    if store.signature(&k) != Some(*b"ACHR") {
        return Err("a fight assumption names no winning placed actor (ACHR)".into());
    }
    Ok(crate::data::record_object(
        &mut vm.world,
        store,
        &k,
        "actor",
        b,
    )?)
}

/// The reference an alias holds at this second, the alias named as
/// `EDITOR_ID#ALIAS` (a reference the run made has no record key to name
/// it by): `None` when it holds none or no reference.
pub(super) fn alias_target(
    vm: &mut Vm<'_>,
    store: &Store,
    key: &str,
    b: &Budget,
) -> Result<Option<crate::ObjId>, E> {
    let (edid, id) = key
        .split_once('#')
        .ok_or("an alias is given as EDITOR_ID#ALIAS")?;
    let q = quest_by_editor_id(store, b, edid)?;
    let id: i32 = id.parse()?;
    let v = crate::quest::alias_value(store, store.limits.max_memory, &mut vm.world, &q, id, 0);
    let used = vm.world.quests.fill_used.borrow().clone();
    vm.rules.extend(used);
    Ok(match v {
        Ok(Some(crate::Val::Object(o))) => Some(o),
        _ => None,
    })
}

/// Does what a fight assumption assumes (`crate::fight`) and writes the
/// handler runs it brings.
pub(super) fn fight_act(
    vm: &mut Vm<'_>,
    store: &Store,
    what: &What,
    when: &str,
    c: &mut Counts,
    names: &mut String,
    b: &Budget,
) -> Result<(), E> {
    let before = vm.effects.len();
    if let What::Hit(k) | What::Combat(k, ..) | What::Death(k, ..) = what {
        if k.contains('#') && alias_target(vm, store, k, b)?.is_none() {
            bump(
                c,
                "assumption from the command line: a fight event on an alias that holds no reference at that second: not raised",
            );
            let _ = writeln!(
                names,
                "## {k}{when}: the alias holds no reference; the fight event is not raised"
            );
            return Ok(());
        }
    }
    let (raised, switch) = match what {
        What::Hit(key) => {
            bump(
                c,
                "assumption from the command line: the player hits a placed actor (--assume-hit)",
            );
            let t = fight_object(vm, store, key, b)?;
            let p = vm.world.player("actor");
            (
                crate::fight::hit(vm, store, t, p, b)?,
                format!("--assume-hit {key}"),
            )
        }
        What::Combat(key, target, state) => {
            bump(
                c,
                format!("assumption from the command line: a placed actor's combat state changes to {state} toward {} (--assume-combat-state)", if target == "player" { "the player" } else { "a placed actor" }),
            );
            let a = fight_object(vm, store, key, b)?;
            let t = fight_object(vm, store, target, b)?;
            (
                crate::fight::combat_state(vm, store, a, t, *state, b)?,
                format!("--assume-combat-state {key} {target} {state}"),
            )
        }
        What::Death(key, killer) => {
            bump(
                c,
                format!("assumption from the command line: a placed actor dies, killed by {} (--assume-death)", if killer == "player" { "the player" } else { "a placed actor" }),
            );
            let a = fight_object(vm, store, key, b)?;
            let k = fight_object(vm, store, killer, b)?;
            (
                crate::fight::death(vm, store, a, k, b)?,
                format!("--assume-death {key} {killer}"),
            )
        }
        What::Unlock(key) => {
            bump(
                c,
                "assumption from the command line: the player unlocks a placed reference (--assume-unlock)",
            );
            let k = crate::player::parse_key(key).ok_or("--assume-unlock takes PLUGIN:OBJECTID")?;
            let o = crate::data::record_object(&mut vm.world, store, &k, "objectreference", b)?;
            (
                crate::runstate::unlock(vm, store, o, b)?,
                format!("--assume-unlock {key}"),
            )
        }
        _ => return Ok(()),
    };
    let _ = writeln!(
        names,
        "## {switch}{when} at {} s: {}, {} handler run(s)",
        raised.t,
        raised.what,
        raised.runs.len()
    );
    if raised.runs.is_empty() {
        bump(c, "fight events	no script has a handler");
    }
    for d in &raised.runs {
        bump(
            c,
            format!(
                "fight events	{}	{}",
                d.receiver.kind(),
                outcome_kind(&d.outcome)
            ),
        );
        let on = match d.receiver {
            crate::player::Receiver::Alias(q, id) => {
                format!(" (alias {id} of {})", vm.world.objects[q].label)
            }
            _ => String::new(),
        };
        let _ = writeln!(
            names,
            "    {} {}{on}: {}",
            d.receiver.kind(),
            d.script,
            outcome_text(&d.outcome)
        );
    }
    for e in &vm.effects[before..] {
        let _ = writeln!(
            names,
            "      {}{}({}{}) = {}",
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
    Ok(())
}

pub(super) fn sit_log(
    vm: &Vm<'_>,
    raised: &[crate::sit::Raised],
    c: &mut Counts,
    names: &mut String,
) {
    for r in raised {
        let _ = writeln!(
            names,
            "## furniture at {} s: {}, {} handler run(s)",
            r.t,
            r.what,
            r.runs.len()
        );
        if r.runs.is_empty() {
            bump(c, "furniture events	no script has a handler");
        }
        for d in &r.runs {
            bump(
                c,
                format!(
                    "furniture events	{}	{}",
                    d.receiver.kind(),
                    outcome_kind(&d.outcome)
                ),
            );
            let on = match d.receiver {
                crate::player::Receiver::Alias(q, id) => {
                    format!(" (alias {id} of {})", vm.world.objects[q].label)
                }
                _ => String::new(),
            };
            let _ = writeln!(
                names,
                "    {} {}{on}: {}",
                d.receiver.kind(),
                d.script,
                outcome_text(&d.outcome)
            );
        }
    }
}
