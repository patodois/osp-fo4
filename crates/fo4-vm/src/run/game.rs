//! The new-game run: its setup from the switches, its clock loop as an
//! object a caller steps ([`NewGame`]), and its outputs.
use super::*;
use crate::timeline::Recorder;
use crate::track::{Point, Region, Track};
use std::collections::VecDeque;
use std::path::PathBuf;

/// `--timeline FILE`: the run's timeline recorder and its file
/// (`crate::timeline`); it only reads the run.
type Timeline = Option<(PathBuf, Recorder)>;

/// Writes the quest and scene changes logged since `seen` to the names
/// file with the real-time second they were reached at (the timeline of
/// a clocked run); the timeline recorder looks at the run.
pub(super) fn timeline(
    vm: &Vm<'_>,
    rec: &mut Timeline,
    seen: &mut (usize, usize),
    names: &mut String,
    what: &str,
) {
    if let Some((_, r)) = rec {
        r.observe(vm)
    }
    let now = vm.world.now[crate::world::Clock::Real as usize];
    let q = &vm.world.quests.log[seen.0..];
    let sc = &vm.world.scenes.log[seen.1..];
    if q.is_empty() && sc.is_empty() {
        return;
    }
    let _ = writeln!(names, "## timeline at {now} s after {what}");
    for (o, ch) in q {
        let _ = writeln!(names, "    quest {}: {ch:?}", vm.world.objects[*o].label);
    }
    for (o, ch) in sc {
        let _ = writeln!(names, "    scene {}: {ch:?}", vm.world.objects[*o].label);
    }
    *seen = (vm.world.quests.log.len(), vm.world.scenes.log.len());
}

/// With a track: raises what the world model has due at the run's present
/// second (`crate::track::settle`), runs the jobs it brings and offers
/// stopped scenes again; writes what was raised to the names file. Off,
/// nothing happens.
pub(super) fn world_step(
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) {
    sit_step(vm, store, then, c, names, top, b);
    if !vm.world.place.on && !vm.world.talk.on && !vm.world.anims.on && !vm.world.anims.graphs {
        return;
    }
    world_step_rest(vm, store, then, c, names, top, b);
}

/// The sit part of [`world_step`]: the player's sit the track walks away
/// from (`--sit-end-walk`, `--sit-end-walk-events`), then the sits and
/// get-ups natives brought.
pub(super) fn sit_step(
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) {
    if crate::sit::end_by_track(store, store.limits.max_memory, &mut vm.world) {
        let rule = if vm.world.sits.end_walk.is_some() {
            crate::rules::SIT_ENDED_BY_TRACK
        } else {
            crate::rules::SIT_EVENTS_ENDED_BY_TRACK
        };
        vm.rules.insert(rule);
    }
    // Sits a script's furniture activation brought (`crate::sit`).
    if vm.world.sits.has_pending() {
        let before = vm.effects.len();
        let (raised, runs) = crate::sit::settle(vm, store, MAX_GAME_JOBS, b);
        sit_log(vm, &raised, c, names);
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
        top.extend(runs);
        recheck(vm, store, then, c, top, b);
    }
}

fn world_step_rest(
    vm: &mut Vm<'_>,
    store: &Store,
    then: &Then,
    c: &mut Counts,
    names: &mut String,
    top: &mut Vec<JobRun>,
    b: &Budget,
) {
    let before = vm.effects.len();
    let loaded = vm.world.loads.log.len();
    let (mut raised, mut runs) = crate::load::settle(vm, store, MAX_GAME_JOBS, b);
    let (track_raised, track_runs) = crate::track::settle(vm, store, MAX_GAME_JOBS, b);
    raised.extend(track_raised);
    runs.extend(track_runs);
    let logged = vm.world.anims.log.len();
    let (anim_raised, anim_runs) = crate::anim::settle(vm, store, MAX_GAME_JOBS, b);
    runs.extend(anim_runs);
    for (t, x) in &vm.world.anims.log[logged..] {
        let _ = writeln!(names, "## animation at {t} s: {x}");
    }
    for r in &anim_raised {
        let _ = writeln!(
            names,
            "## animation event at {} s: {} on {} ({}), {} handler run(s)",
            r.t,
            r.name,
            vm.world.objects[r.sender].label,
            r.from,
            r.runs.len()
        );
        for (script, o) in &r.runs {
            let _ = writeln!(names, "    {script}: {}", outcome_text(o));
            bump(
                c,
                format!(
                    "animation events\traised from the data\t{}",
                    outcome_kind(o)
                ),
            );
        }
        if r.runs.is_empty() {
            bump(
                c,
                "animation events\traised from the data\tno registered instance",
            );
        }
    }
    let talked = vm.world.talk.log.len();
    runs.extend(crate::talk::settle(vm, store, MAX_GAME_JOBS, b));
    for (t, x) in &vm.world.talk.log[talked..] {
        let _ = writeln!(names, "## talk at {t} s: {x}");
    }
    // Loads the steps above brought (moves and enables of their natives).
    let (load_raised, load_runs) = crate::load::settle(vm, store, MAX_GAME_JOBS, b);
    raised.extend(load_raised);
    runs.extend(load_runs);
    for (t, x) in &vm.world.loads.log[loaded..] {
        let _ = writeln!(names, "## load at {t} s: {x}");
    }
    for r in &raised {
        let what = r
            .reference
            .as_ref()
            .map(|k| format!(" on {}", bind::record_key(k)))
            .unwrap_or_default();
        let _ = writeln!(names, "## world model at {} s: {}{what}", r.t, r.event);
        for (kind, script, o) in &r.runs {
            let _ = writeln!(names, "    {kind} {script}: {}", outcome_text(o));
            bump(
                c,
                format!(
                    "world model\thandler runs\t{}\t{kind}\t{}",
                    r.event,
                    outcome_kind(o)
                ),
            );
        }
    }
    if !raised.is_empty() || !anim_raised.is_empty() {
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
    top.extend(runs);
    if let Err(e) = lock_changes(vm, store, c, names, top, b) {
        let _ = writeln!(names, "## lock events: {e}");
    }
    recheck(vm, store, then, c, top, b);
}

pub(super) fn new_game(
    (m, vfs, store): (&Manifest, &Vfs, &Store),
    out: &Path,
    opts: Options,
    then: &Then,
    settings: &Settings,
    stepper: Option<&mut dyn Stepper>,
) -> Result<(), E> {
    let nested = opts.nested;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let tree = story_tree(store, opts)?;
    let mut lib = VfsLibrary::new(vfs, max);
    let mut c = Counts::new();
    let mut names = String::new();
    let mut host = host(store, vfs, max, opts, &tree, settings);
    let mut recorder: Timeline = settings.timeline.clone().map(|f| (f, Recorder::new()));
    host.cond.distance = then.cond_distance;
    host.cond.player_word = then.player_word;
    host.cond.player_sex = then.cond_player_sex;
    host.cond.player_sex_for = then.cond_player_sex_for.iter().cloned().collect();
    for (on, what) in [
        (then.cond_distance, "condition function 1 reads the distance from the world model's positions (--cond-1-distance)"),
        (then.player_word, "a condition's reference word naming the first plugin's id 0x14 is the player (--cond-player-word)"),
        (then.cond_player_sex, "condition function 70 on the player compares its parameter with the caller's assumed sex (--cond-70-player-sex)"),
        (!then.cond_player_sex_for.is_empty(), "condition function 70 on the player compares its parameter with the caller's assumed sex in the records of the quests the caller names (--cond-70-player-sex-for)"),
        (then.location_distinct, "repeated rows of a location's special references naming one reference count once (--location-refs-distinct)"),
        (then.location_placed, "an alias whose location lists no row of its ref type holds the one placed reference of the location's cells carrying it (--alias-location-placed-refs)"),
        (then.location_shared, "aliases of a quest sharing a location and ref type hold its distinct references one each, by alias id and row order (--location-refs-shared)"),
        (then.hit_events, "hit event registrations kept; the caller's hits delivered to the registered instances, once per registration (--hit-events)"),
        (then.hit_init, "a receiver whose init handler the run never delivers registers on its own object counts as registered (--hit-init-registrations)"),
        (then.combat_events, "the caller's combat state changes delivered to the actor's receivers (--combat-state-events)"),
        (then.death_events, "the caller's deaths delivered to the actor's receivers, dying then death (--death-events)"),
        (then.dead_state, "IsDead on an actor the run killed answers true (--dead-state)"),
        (then.combat_natives, "IsInCombat and GetCombatState answered from the run's combat states (--combat-state-natives)"),
        (then.global_values, "a global the run sets answers its new value (--global-values)"),
        (then.global_value_undetermined, "a global set to an undetermined value answers undetermined until a known value is set (--global-value-undetermined)"),
        (then.lock_state, "IsLocked answers the run's Lock calls, before any whether the record stores a lock (--lock-state)"),
        (!then.item_counts.is_empty(), "the caller assumes item counts (--assume-item-count)"),
        (then.lock_events, "the caller's unlock raises the lock state change event on the reference's receivers (--lock-events)"),
        (then.activate_actors, "the caller may assume the player activates a placed actor (--activate-actors)"),
        (then.lock_events_script, "a script's Lock that changes the held lock raises the lock state change event on the reference's receivers (--lock-events-script)"),
        (then.terminal_fragments, "the caller's pick of a terminal's menu item runs the fragment of the item's id (--terminal-fragments)"),
        (then.item_added_events, "inventory event filters kept; the caller's item added delivered to the instances whose filter names it (--item-added-events)"),
        (then.inventory_filter_init, "an init handler the run never delivers that adds an inventory filter on itself counts as having added it (--inventory-filter-init)"),
        (then.item_added_counts, "the caller's item added raises the holder's known count of it (--item-added-counts)"),
        (then.perks_base, "HasPerk on the player answers from its base record's perk list and the run's perk changes (--perks-base)"),
        (then.factions_base, "IsInFaction answers from the actor's base record's faction list and the run's faction changes (--factions-base)"),
        (then.alias_created, "a reference alias created at run time holds a reference the run makes for it, with no record, while its quest runs (--alias-created-refs)"),
        (then.alias_external, "a reference alias filled from another quest's alias holds what that alias holds whenever it is read (--alias-external)"),
        (then.line_script_events, "a line the run plays delivers its begin and end to the scripts its record attaches of its own (--line-script-events)"),
        (then.sit_end_walk.is_some(), "the player's sit ends, as a get-up, when the track takes the player farther than the caller's distance from the furniture's placed position (--sit-end-walk)"),
        (then.sit_end_other_activation, "the player's sit an activation with events brought ends, as a get-up, before the caller's or the input's activation of another reference (--sit-end-other-activation)"),
        (then.sit_end_walk_events.is_some(), "the player's sit an activation with events brought ends, as a get-up, when the track takes the player farther than the caller's distance from the furniture's placed position, looked at before each action too (--sit-end-walk-events)"),
        (then.quest_partial_previous, "a winning quest record with flag bit 0x4000 that carries only EDID and VMAD is read from the occurrence before it, its own VMAD kept (--quest-partial-previous)"),
        (then.container_events, "the caller's container change raises OnContainerChanged on the reference's receivers (--container-change-events)"),
        (then.location_events, "the caller's location change raises OnLocationChange on the actor's receivers (--location-change-events)"),
        (then.location_parents, "Location.IsChild answers from the chain of the other location's parent locations (--location-parents)"),
        (then.timers_per_timer, "the timer budget counts per timer; a timer that used it up is dropped when it comes due again (--timers-per-timer)"),
        (!then.actor_values.is_empty(), "the caller assumes actor values (--assume-actor-value)"),
    ] {
        if on {
            bump(&mut c, format!("switch: {what}"));
        }
    }
    if then.anim[2] {
        host.graphs = Some(Box::new(VfsGraphs {
            vfs,
            cache: BTreeMap::new(),
        }));
    }
    let mut attach = RecordScripts::new(store, max);
    let mut runner = quest::Runner::new(store, max, MAX_GAME_JOBS);
    let mut vm = Vm::new(&mut lib, &mut host);
    let [anim_on, anim_case, anim_graphs, anim_keys] = then.anim;
    vm.world.anims.on = anim_on;
    vm.world.anims.ignore_case = anim_case;
    vm.world.anims.graphs = anim_graphs;
    vm.world.anims.keys = anim_keys;
    vm.world.anims.start_settled = then.anim_start_settled;
    vm.world.anims.sequence_case = then.anim_sequence_case;
    for (on, what) in [
        (anim_on, "animation event registrations kept and delivered where the run raises the event (--anim-events)"),
        (anim_case, "animation event names compared ignoring case (--anim-event-case)"),
        (anim_graphs, "a placed reference runs its base model's behaviour graph; PlayAnimation takes a transition (--anim-graphs)"),
        (anim_keys, "the text keys of the sequence a graph state plays are raised (--anim-graph-keys)"),
        (then.anim_start_settled, "a graph read when first needed has played its start state's sequence: its keys' transitions are taken at once (--anim-graph-start-settled)"),
        (then.anim_sequence_case, "a graph state's sequence name finds the model's sequence ignoring case (--anim-sequence-case)"),
    ] {
        if on {
            bump(&mut c, format!("switch: {what}"));
        }
    }
    vm.attach = Some(&mut attach);
    if nested {
        vm.nested = Some(&mut runner);
    }
    events_on(&mut vm, opts, settings);
    vm.world.quests.location_distinct = then.location_distinct;
    vm.world.quests.location_shared = then.location_shared;
    vm.world.fights.hits = then.hit_events;
    vm.world.fights.init_registrations = then.hit_init;
    vm.world.fights.combat_events = then.combat_events;
    vm.world.fights.death_events = then.death_events;
    vm.world.fights.dead_state = then.dead_state;
    vm.world.fights.combat_natives = then.combat_natives;
    if let Some((_, rec)) = recorder.as_mut() {
        rec.deaths = then.timeline_deaths;
        rec.sits = then.timeline_sits;
        rec.sequences = then.timeline_sequences;
    }
    vm.world.runstate.globals = then.global_values;
    vm.world.runstate.undetermined_values = then.global_value_undetermined;
    vm.world.runstate.locks = then.lock_state;
    vm.world.runstate.lock_events = then.lock_events;
    vm.world.runstate.item_counts = then.item_counts.iter().cloned().collect();
    vm.world.runstate.lock_events_script = then.lock_events_script;
    vm.world.runstate.terminals = then.terminal_fragments;
    vm.world.runstate.inventory.events = then.item_added_events;
    vm.world.runstate.inventory.filter_init = then.inventory_filter_init;
    vm.world.runstate.inventory.counts_added = then.item_added_counts;
    vm.world.runstate.perks = then.perks_base;
    vm.world.runstate.factions = then.factions_base;
    vm.world.quests.created_refs = then.alias_created;
    vm.world.quests.external_fills = then.alias_external;
    vm.world.scenes.line_scripts = then.line_script_events;
    vm.world.sits.end_walk = then.sit_end_walk;
    vm.world.sits.end_walk_events = then.sit_end_walk_events;
    vm.world.sits.end_other_activation = then.sit_end_other_activation;
    vm.world.quests.partial_previous = then.quest_partial_previous;
    vm.world.runstate.chain.container = then.container_events;
    vm.world.runstate.chain.location = then.location_events;
    vm.world.runstate.chain.parents = then.location_parents;
    vm.world.runstate.actor_values = then.actor_values.iter().cloned().collect();
    vm.world.sits.activate = then.sit_activate;
    vm.world.sits.in_use = then.sit_in_use;
    vm.world.sits.activate_events = then.sit_activate_events;
    vm.world.sits.activate_events_deliver = then.sit_activate_events_deliver;
    vm.world.sits.activate_get_up = then.sit_activate_get_up;
    vm.world.sits.graph_events = then.furniture_graph_events.clone();
    vm.world.sits.sit_state = then.sit_state;
    vm.world.menus.assume_player_combat = then.player_combat;
    vm.world.menus.on = then.menu_events;
    vm.world.keywords.records = then.keywords_records;
    vm.world.keywords.runtime = then.keywords_runtime;
    vm.world.quests.collection_fills = then.collection_fills;
    vm.world.anims.assume_registration = then.anim_registration;
    vm.world.menus.assume_menu_mode = then.menu_mode;
    vm.world.menus.assume_message_button = then.message_button;
    if then.recheck_variables {
        vm.world.scenes.variable_writes = Some(Default::default());
    }
    let gated = then
        .actions
        .iter()
        .filter(|x| x.after.is_some() && !matches!(x.what, What::Move(_)))
        .count() as u64;
    let moves_gated = then
        .actions
        .iter()
        .filter(|x| matches!(x.what, What::Move(_)))
        .count() as u64;
    if then.actor_move_gates {
        bump(
            &mut c,
            "switch: an --action-after gate also holds the assumed actor moves of its actor at its second (--action-after-actor-moves)",
        );
    }
    if moves_gated > 0 {
        *c.entry("assumption from the command line: an assumed actor move waits for a stage before it happens (--action-after, --action-after-actor-moves)".into())
            .or_default() += moves_gated;
    }
    if gated > 0 {
        *c.entry("assumption from the command line: a timed caller action waits for a stage before it happens (--action-after)".into())
            .or_default() += gated;
    }
    if then.action_offset {
        bump(
            &mut c,
            "switch: a caller action held until a stage keeps its offset from the stage (--action-after-offset)",
        );
    }
    if !then.input_written.is_empty() {
        *c.entry("assumption from the command line: an input action stands for a caller action written at a second (--input-action-at)".into())
            .or_default() += then.input_written.len() as u64;
    }
    if then.unmatched_gates > 0 {
        *c.entry(
            "caller actions held until a stage: gates naming no timed caller action".into(),
        )
        .or_default() += then.unmatched_gates;
    }
    if let Some(n) = then.message_button {
        bump(
            &mut c,
            format!("assumption from the command line: Message.Show answers button {n} (--assume-message-button)"),
        );
    }
    if let Some(v) = then.player_combat {
        bump(
            &mut c,
            format!("assumption from the command line: IsInCombat on the player answers {v} (--assume-player-in-combat)"),
        );
    }
    if let Some(v) = then.menu_mode {
        bump(
            &mut c,
            format!(
                "assumption from the command line: IsInMenuMode answers {v} (--assume-menu-mode)"
            ),
        );
    }
    if let Some(v) = then.anim_registration {
        bump(
            &mut c,
            format!("assumption from the command line: a made animation event registration answers {v} (--assume-anim-registration)"),
        );
    }
    for (on, what) in [
        (then.sit_events, "sit and get-up events delivered to the sitter's and the furniture's receivers (--sit-events)"),
        (then.sit_activate, "a script's default-processing activation of a furniture reference sits the activator in it (--activate-furniture-sits)"),
        (then.sit_in_use, "IsFurnitureInUse answered from the run's sitters (--furniture-in-use)"),
        (then.sit_activate_events, "a script's activation with events of a furniture reference sits the activator in it (--activate-furniture-events)"),
        (then.sit_activate_events_deliver, "such an activation also delivers the furniture's OnActivate first (--activate-furniture-events-deliver)"),
        (then.sit_activate_get_up, "a script's activation of the furniture reference the activator sits in gets it up (--activate-furniture-get-up)"),
        (then.furniture_graph_events.is_some(), "those sits and get-ups send the furniture's graph the caller's events (--furniture-graph-events)"),
        (then.sit_state, "GetSitState on the player or a placed actor answered from the run's sitters (--sit-state)"),
        (then.menu_events, "menu event registrations kept and the caller's menu events delivered (--menu-events)"),
        (then.keywords_records, "HasKeyword on a placed reference answers its base's keywords, an actor's with its race's (--keywords-records)"),
        (then.keywords_runtime, "AddKeyword and RemoveKeyword change a reference's keywords, HasKeyword answering the change (--keywords-runtime)"),
        (then.collection_fills, "a collection alias holds the distinct references of its location's rows of its ref type (--collection-location-refs)"),
    ] {
        if on {
            bump(&mut c, format!("switch: {what}"));
        }
    }
    if then.location_placed {
        vm.world.quests.location_placed = Some(Rc::new(quest::LocationPlaced::read(store, &b)));
    }
    if let Some((track, sw)) = &then.track {
        vm.world.place = crate::track::Place::load_with(
            store,
            track.clone(),
            *sw,
            then.actor_moves.clone(),
            &b,
        )?;
        // The assumed moves a gate holds (`--action-after-actor-moves`)
        // wait as caller actions: they leave the world model's list (each
        // comes back when it happens, `crate::track::schedule_move`); their
        // actors stay among the moved ones.
        for x in &then.actions {
            if let What::Move(m) = &x.what {
                let p = &mut vm.world.place;
                if let Some(i) = p.actor_moves.iter().position(|y| y == m) {
                    p.actor_moves.remove(i);
                }
                p.actors_held_moves.insert(m.actor.clone());
            }
        }
        if !vm.world.place.doors.is_empty() {
            vm.rules.insert(crate::rules::WORLD_LOAD_DOOR);
        }
        for d in &vm.world.place.doors {
            let label = |k: &FormKey| {
                crate::bind::label(store, k, &b).unwrap_or_else(|_| crate::bind::record_key(k))
            };
            let _ = writeln!(
                names,
                "## track: at {} s the player passes through {} ({:.1} units from the track's last point in {}) to {} in {} at {:.2} {:.2} {:.2} facing {:.1}",
                d.t,
                label(&d.door),
                d.reach,
                label(&d.from_space),
                label(&d.to),
                label(&d.space),
                d.pos[0],
                d.pos[1],
                d.pos[2],
                d.facing
            );
        }
        vm.world.loads.sw = then.loads;
        if let Some(n) = then.loads.grid {
            bump(
                &mut c,
                format!("assumption from the command line: references within {n} grid squares of the player's load (--load-grid)"),
            );
        }
        if !then.actor_moves.is_empty() {
            *c.entry(
                "assumption from the command line: a placed actor moved to a position at a second (--assume-actor-at)"
                    .into(),
            )
            .or_default() += then.actor_moves.len() as u64;
        }
        bump(
            &mut c,
            "assumption from the command line: the player follows a track (--track)",
        );
        *c.entry("assumption from the command line: points of the player's track".into())
            .or_default() += track.points.len() as u64;
        if let Some(h) = sw.body {
            bump(
                &mut c,
                format!("assumption from the command line: the player's body reaches {h} units up from the reference point (--trigger-body)"),
            );
        }
        if let Some(v) = sw.assume_los {
            bump(
                &mut c,
                format!("assumption from the command line: every HasDirectLOS answers {v} (--assume-los)"),
            );
        }
        for (on, what) in [
            (sw.boxes, "an activator's primitive of kind 1 is a box of half extents its bounds (--trigger-box)"),
            (sw.spheres, "an activator's primitive of kind 2 is a sphere (--trigger-sphere)"),
            (sw.point, "a trigger holds the player's reference point in its closed volume (--trigger-point)"),
            (sw.body.is_some(), "a trigger holds the player while a vertical segment up from the reference point meets its closed volume (--trigger-body)"),
            (sw.instant, "trigger events are raised at the second the hold changes (--trigger-instant)"),
            (sw.enabled, "a disabled trigger holds no one; enabling or disabling it with the player inside raises its event (--trigger-enabled)"),
            (sw.order, "trigger events at one second: leaves, then enters, by reference (--trigger-order)"),
            (sw.distance, "distance registrations raise their event when the condition holds (--distance-events)"),
            (sw.distance_once, "a distance registration ends with its event (--distance-once)"),
            (sw.placed, "placed references other than actors stand at their stored position (--placed-positions)"),
            (sw.position_natives, "position and distance natives answered from the positions (--position-natives)"),
            (sw.cell_natives, "parent cell, worldspace and interior natives answered (--cell-natives)"),
            (sw.location_native, "the current location is the parent cell's (--location-native)"),
            (sw.actors, "placed actors stand at their stored position or where the caller moved them (--actor-positions)"),
            (sw.actor_triggers, "an actor's assumed move raises trigger events as the player's (--actor-triggers)"),
            (sw.moveto_target, "MoveTo with zero offsets puts the reference at its target's position (--moveto-target)"),
            (sw.load_doors, "where the track changes space the player passes through the old space's load door to its teleport position (--load-doors)"),
            (then.loads.enters, "a reference that comes into the player's interior cell loads (--load-in-cell)"),
            (then.loads.arrival, "the references of an interior cell load when the player's track reaches it (--load-on-arrival)"),
            (then.loads.grid.is_some(), "references within the caller's grid of the player's square load (--load-grid)"),
        ] {
            if on {
                bump(&mut c, format!("switch: {what}"));
            }
        }
    }
    if then.player_no_record {
        vm.world.player_no_record = true;
        bump(
            &mut c,
            "switch: the player is no record's handle (--player-no-record)",
        );
    }
    // Reference aliases filled by a unique actor, and the player's base
    // as the caller names it.
    let player_base = match &then.player_base {
        Some(e) => {
            bump(
                &mut c,
                "assumption from the command line: the NPC_ of that editor ID is the player's base (--player-base)",
            );
            Some(npc_by_editor_id(store, &b, e)?)
        }
        None => None,
    };
    if then.player_base_object {
        vm.world.player_base_object = player_base.clone();
        vm.world.player_sex = then.player_sex;
        bump(
            &mut c,
            "switch: the base-object natives on the player answer the player's base the caller names (--player-base-object)",
        );
        if let Some(n) = then.player_sex {
            bump(
                &mut c,
                format!("assumption from the command line: ActorBase.GetSex on the player's base answers {n} (--assume-player-sex)"),
            );
        }
    }
    if then.unique_fills {
        bump(
            &mut c,
            "switch: reference aliases filled by a unique actor hold its one placed actor (--alias-unique-actors)",
        );
        let mut u = quest::UniqueActors::read(store, &b);
        u.player_base = player_base.clone();
        vm.world.quests.unique = Some(Rc::new(u));
    }
    // Dialogue outside scenes.
    let t = then.talk;
    if t.greeting || t.hello || t.priority {
        vm.world.talk.on = true;
        vm.world.talk.sw = crate::talk::Switches {
            stage_fields: opts.line_fields,
            ..t
        };
        vm.world.talk.player_base = player_base.clone();
        for l in &then.assume_lines {
            let k = crate::player::parse_key(l).ok_or("a line is given as plugin.esm:0001ABCD")?;
            vm.world.talk.assume_lines.insert(k);
            bump(
                &mut c,
                "assumption from the command line: a line plays where the choice outside a scene is not settled (--assume-line)",
            );
        }
        for (on, what) in [
            (t.greeting, "a greeting line plays when the player starts a conversation (--greeting-on-talk)"),
            (t.hello, "a hello line plays when the player comes within the hello distance of a placed actor (--hello-lines)"),
            (t.priority, "a line outside a scene is chosen by priority (--dialogue-priority)"),
            (t.info_groups, "a line outside a scene holds only when the group it names holds too; a group does not play (--info-groups)"),
            (t.hello_owed, "an entry into the hello distance that played no line is offered again after each change of the quest state while the player stays inside (--hello-owed)"),
        ] {
            if on {
                bump(&mut c, format!("switch: {what}"));
            }
        }
        if let Some(name) = &then.hello_setting {
            bump(
                &mut c,
                "assumption from the command line: the hello distance is the value of the game setting named (--hello-distance-setting)",
            );
            let d = game_setting_float(store, &b, name)?;
            vm.world.talk.hello_distance = Some(f64::from(d));
        }
        crate::talk::load_hello(&mut vm.world);
        *c.entry("talk	hello crossings of the track".into())
            .or_default() += vm.world.talk.crossings.len() as u64;
        *c.entry("talk	placed actors near the track".into())
            .or_default() += vm.world.place.actors.len() as u64;
    }
    let started = quest::start_new_game(&mut vm, store, &b)?;
    for (_, _, r) in &started {
        bump(
            &mut c,
            format!("quests started with the game: Start answer {r:?}"),
        );
    }
    let mut top = quest::run_jobs(&mut vm, store, MAX_GAME_JOBS, &b);
    if opts.location_fills {
        bump(
            &mut c,
            "switch: reference aliases filled from their location's special references (--alias-location-refs)",
        );
    }
    if opts.action_conditions {
        bump(
            &mut c,
            "switch: condition function 550 reads the actions the scene clock completed (--cond-550-clock)",
        );
    }
    if opts.stage_fields {
        bump(
            &mut c,
            "switch: the stage fields of scenes and phases set stages (--stage-fields)",
        );
    }
    if let Some(seen) = &then.recheck {
        bump(
            &mut c,
            "switch: scenes stopped on conditions offered again after a quest state change (--scene-recheck)",
        );
        seen.set(vm.world.quests.log.len());
    }
    if then.recheck_variables {
        bump(
            &mut c,
            "switch: scenes stopped on conditions offered again after a quest script variable they read was written (--scene-recheck-variables)",
        );
    }
    for x in then.actions.iter().filter(|x| x.at.is_none()) {
        act(&mut vm, store, then, x, &mut c, &mut names, &mut top, &b)?;
    }
    world_step(&mut vm, store, then, &mut c, &mut names, &mut top, &b);
    let scale_global = then
        .time_scale
        .as_deref()
        .and_then(|n| time_scale_global(store, &b, n));
    count_clock(&mut c, then.scene_clock, &settings.clock);
    count_switches(&mut c, opts);
    if then.scene_clock > 0 {
        if then.timers > 0 {
            bump(
                &mut c,
                "assumption from the command line: queued timers fired after the jobs",
            );
        }
        let mut run = NewGame::new(
            (m, vfs, store),
            &mut vm,
            then,
            &scale_global,
            (&mut c, &mut names, &mut top),
            &mut recorder,
            &b,
        );
        match stepper {
            Some(s) => {
                s.step(&mut run)?;
                if !run.ended {
                    run.finish_input()?;
                }
            }
            None => run.run_to_end()?,
        }
        let fired = std::mem::take(&mut run.fired);
        drop(run);
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
        }
    } else if then.timers > 0 {
        bump(
            &mut c,
            "assumption from the command line: queued timers fired after the jobs",
        );
        let fired: Vec<(usize, crate::Val, Outcome)> = if then.time_scale.is_some() {
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
    // --timeline: one more look at the end, then the file (reads only).
    if let Some((file, mut rec)) = recorder.take() {
        rec.observe(&vm);
        let t = rec.finish(&vm, store, Some((vfs, &m.language)), &Budget::new(max));
        std::fs::write(&file, t.write())?;
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
            .and_then(|k| crate::data::key_record(&k))
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
    let location_used = vm.world.quests.location_used.borrow().clone();
    vm.rules.extend(location_used);
    let fill_used = vm.world.quests.fill_used.borrow().clone();
    vm.rules.extend(fill_used);
    let made = vm.world.quests.created_made.len();
    if made > 0 {
        *c.entry("references the run made for created aliases".into())
            .or_default() += made as u64;
    }
    let unique = vm.world.quests.unique_used.borrow().clone();
    if !unique.is_empty() {
        vm.rules.insert(crate::rules::ALIAS_UNIQUE_ACTOR);
    }
    for ok in unique.values() {
        bump(
            &mut c,
            format!(
                "unique actor aliases read by the run\t{}",
                if *ok {
                    "resolved to one placed actor or the player"
                } else {
                    "not resolved: none or several placed actors"
                }
            ),
        );
    }
    for r in &vm.rules {
        bump(&mut c, format!("rules relied on\t{r}"));
    }
    for (k, v) in &vm.world.place.counts {
        *c.entry(format!("world model\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.talk.counts {
        *c.entry(format!("talk\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.scenes.line_counts {
        *c.entry(format!("lines\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.scenes.step_counts {
        *c.entry(format!("scenes\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.anims.counts {
        *c.entry(format!("animation\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.loads.counts {
        *c.entry(format!("load\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.sits.counts {
        *c.entry(format!("furniture\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.menus.counts {
        *c.entry(format!("menu\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.fights.counts {
        *c.entry(format!("fight\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.keywords.counts {
        *c.entry(format!("keyword\t{k}")).or_default() += v;
    }
    for (k, v) in &vm.world.quests.collection_counts {
        *c.entry(format!("collection alias\t{k}")).or_default() += v;
    }
    if then.census_run_state {
        for (k, v) in &vm.world.runstate.counts {
            *c.entry(format!("run state\t{k}")).or_default() += v;
        }
        if !vm.world.runstate.log.is_empty() {
            let _ = writeln!(names, "## run state changes the run kept");
            for (t, what) in &vm.world.runstate.log {
                let _ = writeln!(names, "    {t} s: {what}");
            }
        }
    }
    if !vm.world.keywords.log.is_empty() {
        let _ = writeln!(names, "## keyword changes the run kept");
        for (t, what) in &vm.world.keywords.log {
            let _ = writeln!(names, "    {t} s: {what}");
        }
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
    if then.next_blockers {
        let quests: Vec<String> = then
            .actions
            .iter()
            .filter_map(|x| match &x.what {
                What::Stage(q, _) => Some(q.clone()),
                What::Player(..)
                | What::Talk(_)
                | What::Anim(..)
                | What::Graph(..)
                | What::Sit(..)
                | What::Menu(..)
                | What::Looks(_)
                | What::Hit(_)
                | What::Combat(..)
                | What::Death(..)
                | What::Unlock(_)
                | What::Terminal(..)
                | What::ItemAdded(..)
                | What::ContainerChange(..)
                | What::LocationChange(..)
                | What::Move(_) => None,
            })
            .collect();
        next_blockers(&mut vm, m, vfs, store, opts, &quests, then.outer_calls, out)?;
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

/// A caller that steps a new-game run ([`NewGame`]): the play mode gives it
/// the player's track and actions slice by slice. Called once the run is
/// set up, when its clock loop would start (only with the scene clock on,
/// `--scene-clock N`); a run the stepper leaves unfinished is finished as
/// if its input were over ([`NewGame::finish_input`]).
pub trait Stepper {
    fn step(&mut self, run: &mut NewGame<'_, '_>) -> Result<(), E>;
}

/// Counts of the stepped run's input by text, the play mode's `play.txt`
/// (none in `quest_run`'s outputs).
pub type InputCounts = Counts;

/// What a stepped run reads beyond the caller's track
/// ([`NewGame::begin_input`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stepping {
    /// Read trigger volumes and placed actors about the new points as they
    /// come (`Place::load_with` about each point whose cells were not
    /// read), relying on the rule named.
    pub region: Option<&'static str>,
    /// When the track grows, compute again the function 1 marks of the
    /// scenes waiting on false completion conditions with no mark left
    /// ([`scene::recheck_distance_marks`]: no wait logged), in place of
    /// offering them their completion again (one more wait logged each
    /// time the conditions are still false).
    pub recheck_marks: bool,
}

/// Under `--action-after-offset` (`caller_action_after_keeps_offset_v1`):
/// a gate's reference second, the written second of the caller's action in
/// whose turn its stage was done, else the earliest held action's second.
pub(crate) fn gate_reference(written: Option<f64>, earliest_held: f64) -> f64 {
    written.unwrap_or(earliest_held)
}

/// A held action's second: the stage's second plus the action's own second
/// minus the gate's reference, never before the stage.
pub(crate) fn held_due(done: f64, at: f64, reference: f64) -> f64 {
    done + (at - reference).max(0.0)
}

/// A gate's action still waiting for its own second when the gate's held
/// actions are given theirs: moved as much as they are, never before its
/// own second.
pub(crate) fn waiting_due(done: f64, at: f64, reference: f64) -> f64 {
    at.max(held_due(done, at, reference))
}

/// A gated action held until its stage (`--action-after`), and under
/// `--action-after-offset` the second its gate gave it.
struct Held {
    x: Caller,
    due: Option<f64>,
}

/// The input's state of a stepped run.
struct Input {
    /// The second the input reached.
    horizon: f64,
    /// More input may come (until [`NewGame::finish_input`]).
    open: bool,
    /// The caller's track as the run loaded it.
    loaded: Option<Rc<Track>>,
    /// With [`Stepping::region`]: what the run has read and the rule.
    region: Option<(Region, &'static str)>,
    recheck_marks: bool,
}

/// What one turn of the clock loop did.
enum Turn {
    /// Something ran; turn again.
    Ran,
    /// What is due first lies past the input's horizon, or nothing is due
    /// while input may come: the caller gives the next slice.
    NeedInput,
    /// Nothing is due: the clocks stopped.
    Stopped,
}

/// A timer fired by the clock loop: the instance, the timer's id, the
/// outcome.
type Fired = Vec<(usize, crate::Val, Outcome)>;

/// A new-game run's clock loop as an object (`spec/play.md`): each turn
/// takes the earliest of the next timer, the next scene clock due, the
/// next timed caller action and the world model's next due second, moves
/// the run's clock there and runs what is due. [`NewGame::run_to_end`] is
/// `quest_run`'s loop over the whole track. A stepped run
/// ([`NewGame::begin_input`]) knows the player's track only to the second
/// the input reached: [`NewGame::advance_to`] gives it the next slice (the
/// points, the second they reach, the player's actions) and runs what is
/// due up to there; [`NewGame::finish_input`] ends the input (the player
/// stays at the last point) and runs on to the end; [`NewGame::stop`]
/// ends the run where it is.
pub struct NewGame<'r, 'v> {
    vm: &'r mut Vm<'v>,
    m: &'r Manifest,
    vfs: &'r Vfs,
    store: &'r Store,
    then: &'r Then,
    scale_global: &'r Option<FormKey>,
    c: &'r mut Counts,
    names: &'r mut String,
    top: &'r mut Vec<JobRun>,
    recorder: &'r mut Timeline,
    b: &'r Budget,
    fired: Fired,
    timers: usize,
    steps: usize,
    /// The caller's timed actions, earliest first (command-line order on a
    /// tie), each with whether the stepped input gave it.
    pending: VecDeque<(Caller, bool)>,
    /// Gated actions whose second came before their stage
    /// (`--action-after`).
    held: Vec<Held>,
    /// Under `--action-after-offset`: every gate the caller's actions name
    /// (`caller_action_after_keeps_offset_v1`), and the written second of
    /// the caller's action in whose turn each was done.
    gates: Vec<(String, i32)>,
    gate_written: BTreeMap<(String, i32), f64>,
    seen: (usize, usize),
    input: Option<Input>,
    ended: bool,
    counts: InputCounts,
    /// Firings so far of each timer (instance, clock, id) under
    /// `--timers-per-timer` (`vm_timer_budget_per_timer_v1`).
    timer_uses: BTreeMap<String, usize>,
}

impl<'r, 'v> NewGame<'r, 'v> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        (m, vfs, store): (&'r Manifest, &'r Vfs, &'r Store),
        vm: &'r mut Vm<'v>,
        then: &'r Then,
        scale_global: &'r Option<FormKey>,
        (c, names, top): (&'r mut Counts, &'r mut String, &'r mut Vec<JobRun>),
        recorder: &'r mut Timeline,
        b: &'r Budget,
    ) -> Self {
        let mut pending: Vec<Caller> = then
            .actions
            .iter()
            .filter(|x| x.at.is_some())
            .cloned()
            .collect();
        pending.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        let mut gates: Vec<(String, i32)> = Vec::new();
        if then.action_offset {
            for g in then.actions.iter().filter_map(|x| x.after.clone()) {
                if !gates.contains(&g) {
                    gates.push(g);
                }
            }
        }
        let mut run = NewGame {
            vm,
            m,
            vfs,
            store,
            then,
            scale_global,
            c,
            names,
            top,
            recorder,
            b,
            fired: Vec::new(),
            timers: then.timers,
            steps: then.scene_clock,
            pending: pending.into_iter().map(|x| (x, false)).collect(),
            held: Vec::new(),
            gates,
            gate_written: BTreeMap::new(),
            seen: (0, 0),
            input: None,
            ended: false,
            counts: InputCounts::new(),
            timer_uses: BTreeMap::new(),
        };
        run.note("the start and the stages set from the command line");
        run
    }

    /// The run's interpreter and world.
    pub fn vm(&self) -> &Vm<'v> {
        self.vm
    }
    /// The records the run reads.
    pub fn store(&self) -> &'r Store {
        self.store
    }
    /// The manifest and the mounted archives the run reads.
    pub fn sources(&self) -> (&'r Manifest, &'r Vfs) {
        (self.m, self.vfs)
    }
    /// The run's present second.
    pub fn now(&self) -> f64 {
        self.vm.world.now[crate::world::Clock::Real as usize]
    }
    /// The caller's track as the run loaded it (`--track`, with the points
    /// `--load-doors` adds), before [`NewGame::begin_input`] cut it.
    pub fn loaded_track(&self) -> Option<&Track> {
        self.input.as_ref().and_then(|i| i.loaded.as_deref())
    }
    /// Whether the loop has ended (the clocks stopped or the run stopped).
    pub fn ended(&self) -> bool {
        self.ended
    }
    /// The run's timeline so far with line texts
    /// (`crate::timeline::Recorder::finish`); `None` without
    /// `--timeline`. It reads labels and strings: ask when something
    /// changed, not every slice.
    pub fn timeline_now(&self) -> Option<fo4_scene::timeline::Timeline> {
        self.recorder.as_ref().map(|(_, rec)| {
            rec.finish(
                self.vm,
                self.store,
                Some((self.vfs, &self.m.language)),
                &Budget::new(self.m.limits.max_memory),
            )
        })
    }
    /// Takes the counts of the input so far ([`InputCounts`]).
    pub fn take_input_counts(&mut self) -> InputCounts {
        std::mem::take(&mut self.counts)
    }

    fn count(&mut self, k: impl Into<String>, n: u64) {
        *self.counts.entry(k.into()).or_default() += n;
    }
    fn note(&mut self, what: &str) {
        timeline(self.vm, self.recorder, &mut self.seen, self.names, what);
    }
    fn act(&mut self, x: &Caller) -> Result<(), E> {
        act(
            self.vm, self.store, self.then, x, self.c, self.names, self.top, self.b,
        )
    }
    fn world_step(&mut self) {
        world_step(
            self.vm, self.store, self.then, self.c, self.names, self.top, self.b,
        );
    }

    /// `quest_run`'s clock loop over the whole track: runs to the end.
    pub fn run_to_end(&mut self) -> Result<(), E> {
        if self.ended {
            return Ok(());
        }
        if self.input.is_some() {
            return self.finish_input();
        }
        while let Turn::Ran = self.turn()? {}
        self.close()
    }

    /// Starts the input: the caller's track is cut to the run's present
    /// second (its first point is the start; its points choose the cells
    /// the world model read), its crossings folded so far
    /// ([`crate::track::Place::cut`], [`crate::talk::Talk::cut`]), and the
    /// loop runs what is due at the present second. The run needs its
    /// `--track`.
    pub fn begin_input(&mut self, stepping: Stepping) -> Result<(), E> {
        if self.input.is_some() || self.ended {
            return Err("PlayBegun: the input has already begun".into());
        }
        if !self.vm.world.place.on {
            return Err(
                "PlayNoTrack: the play mode needs the run's --track (its first point is the start)"
                    .into(),
            );
        }
        let now = self.now();
        let w = &mut self.vm.world;
        let loaded = w.place.track.clone();
        let mut region = None;
        let mut dropped = 0;
        if let Some((d, kept)) = w.place.cut(now) {
            dropped = d;
            if kept {
                w.talk.cut(&w.place);
                region = stepping.region.map(|r| (Region::default(), r));
            }
        }
        self.count(
            "play: points of the caller's track after the start, replaced by the input's",
            dropped as u64,
        );
        self.input = Some(Input {
            horizon: now,
            open: true,
            loaded,
            region,
            recheck_marks: stepping.recheck_marks,
        });
        self.run_until_input()
    }

    /// Gives the run the input up to second `until`: the player's track
    /// points after the last one it has (none while the player stands
    /// still; the player stands at the last point until `until`) and the
    /// player's actions (each at its second, before the command line's
    /// actions of that second, in the given order), then runs everything
    /// due up to `until`. Starts the input first when it has not begun.
    pub fn advance_to(
        &mut self,
        until: f64,
        points: &[Point],
        actions: &[(f64, crate::player::Action, FormKey)],
    ) -> Result<(), E> {
        if self.input.is_none() {
            self.begin_input(Stepping::default())?;
        }
        let Some(input) = self.input.as_ref() else {
            return Err("PlayNotBegun: the input has not begun".into());
        };
        if self.ended || !input.open {
            return Err("PlayInputOver: the input is over".into());
        }
        let horizon = input.horizon;
        let now = self.now();
        if until.is_nan() || until < horizon {
            return Err(format!(
                "PlayHorizonBack: a slice until {until} s after the horizon {horizon} s"
            )
            .into());
        }
        if points.last().is_some_and(|p| p.t > until) {
            return Err("PlaySliceBeyond: a point after the slice's second".into());
        }
        let moved = !points.is_empty();
        let w = &mut self.vm.world;
        let moves = w.place.extend(points)?;
        if !moves.is_empty() {
            w.talk.extend(&w.place, &moves);
        }
        if moved {
            if let Some((v, a, rule)) = self.grow(points)? {
                if v + a > 0 {
                    self.vm.rules.insert(rule);
                    let _ = writeln!(
                        self.names,
                        "## play: {v} trigger volumes and {a} placed actors read about the walker"
                    );
                }
            }
        }
        if moved && self.then.cond_distance {
            let recheck = self.input.as_ref().is_some_and(|i| i.recheck_marks);
            let n = if recheck {
                self.recheck_marks()
            } else {
                self.reoffer_distance_waits()
            };
            if n > 0 && !recheck {
                self.note("a scene offered again as the track grew");
            }
        }
        self.count("play: slices of input", 1);
        self.count("play: track points from the input", points.len() as u64);
        if let Some(i) = self.input.as_mut() {
            i.horizon = until;
        }
        // In reverse, each before the command line's actions of its
        // second: the slice's own order is kept and comes first on a tie.
        for (at, action, reference) in actions.iter().rev() {
            if !(*at >= now && *at <= until) {
                return Err(format!(
                    "PlayActionTime: an action at {at} s outside the slice ({now} to {until} s)"
                )
                .into());
            }
            let key = crate::bind::record_key(reference);
            if *action == crate::player::Action::Activate
                && self.store.signature(reference) == Some(*b"ACHR")
                && !self.then.activate_actors
            {
                self.count(
                    "play: activations of a placed actor left out (--activate-actors off)",
                    1,
                );
                continue;
            }
            self.count(
                format!("play: player actions from the input\t{}", action.handler()),
                1,
            );
            let x = Caller {
                what: What::Player(*action, key),
                at: Some(*at),
                after: None,
            };
            let pos = self
                .pending
                .iter()
                .position(|p| p.0.at.is_some_and(|t| t >= *at))
                .unwrap_or(self.pending.len());
            self.pending.insert(pos, (x, true));
        }
        self.run_until_input()
    }

    /// Ends the input: the player stays at the last point and the run goes
    /// on to its end as `quest_run` does after the track's end.
    pub fn finish_input(&mut self) -> Result<(), E> {
        if self.ended {
            return Ok(());
        }
        let Some(input) = self.input.as_mut() else {
            return self.run_to_end();
        };
        input.open = false;
        self.count(
            "play: input over, the run goes on as with the track's last point held",
            1,
        );
        while let Turn::Ran = self.turn()? {}
        self.close()
    }

    /// Ends the run now (a window closed): what is due later does not
    /// happen; the caller actions left are counted.
    pub fn stop(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.count("play: input stopped the run", 1);
        self.count_prefix();
        let left = self.pending.len() as u64 + self.held.len() as u64;
        self.count(
            "play: caller actions left when the input stopped the run",
            left,
        );
    }

    /// Runs turns until the input is needed or the clocks stop.
    fn run_until_input(&mut self) -> Result<(), E> {
        loop {
            match self.turn()? {
                Turn::Ran => {}
                Turn::NeedInput => return Ok(()),
                Turn::Stopped => return self.close(),
            }
        }
    }

    fn count_prefix(&mut self) {
        if self.input.is_none() {
            return;
        }
        let w = &self.vm.world;
        let n = w.place.folds.as_ref().map_or(0, |f| f.prefix_changed) + w.talk.prefix_changed;
        self.count(
            "play: crossings already applied that changed when the track grew (expected 0)",
            n,
        );
    }

    /// One turn of the clock loop.
    fn turn(&mut self) -> Result<Turn, E> {
        let (store, then, b) = (self.store, self.then, self.b);
        if then.action_offset {
            // Gates whose stage is now done give their held actions their
            // seconds; one whose second has come happens now.
            self.release_gates();
            let now = self.now();
            if let Some((pos, _)) = self.next_held().filter(|&(_, t)| t <= now) {
                self.act_held(pos)?;
                return Ok(Turn::Ran);
            }
        } else if let Some(pos) = self
            .held
            .iter()
            .position(|h| gate_done(self.vm, store, &h.x, b))
        {
            // A held action whose stage is now done happens at once.
            let x = self.held.remove(pos).x;
            bump(self.c, "caller actions held until a stage: happened when the stage was done, after their second");
            self.act(&x)?;
            self.world_step();
            self.note("a caller's held action");
            return Ok(Turn::Ran);
        }
        let scale = self
            .scale_global
            .as_ref()
            .and_then(|g| time_scale(self.vm, store, g, b));
        // Per timer: a timer that used up its own budget is dropped when it
        // comes due again (`vm_timer_budget_per_timer_v1`).
        let mut next_key = None;
        if self.then.timers_per_timer && self.timers > 0 {
            let (dropped, key, full) =
                drop_spent_timers(self.vm, scale, &self.timer_uses, self.timers);
            next_key = key;
            if full {
                // The per-timer budget has no total of its own: past
                // MAX_TIMER_KEYS distinct timers no more fire.
                self.timers = 0;
                bump(
                    self.c,
                    "timers stopped: the distinct timers that may fire used up (--timers-per-timer)",
                );
                let _ = writeln!(
                    self.names,
                    "## timers stopped at {} s: {} distinct timers fired",
                    self.now(),
                    super::MAX_TIMER_KEYS
                );
            }
            for _ in 0..dropped {
                bump(
                    self.c,
                    "timers dropped when due again: their own budget used up (--timers-per-timer)",
                );
                let _ = writeln!(
                    self.names,
                    "## timer dropped at {} s: its own budget of {} firings used up",
                    self.now(),
                    self.timers
                );
            }
        }
        let timer = (self.timers > 0 && !self.vm.world.timers.is_empty())
            .then(|| self.vm.next_timer_wait(scale));
        let due = if self.steps > 0 {
            scene::next_due(&self.vm.world)
        } else {
            None
        };
        let now = self.now();
        let pending_wait = self
            .pending
            .front()
            .and_then(|x| x.0.at)
            .map(|t| (t - now).max(0.0));
        // Under `--action-after-offset`: a held action given its second
        // comes before a waiting one of the same second.
        let held_next = if then.action_offset {
            self.next_held().map(|(p, t)| (p, (t - now).max(0.0)))
        } else {
            None
        };
        let wait = match (pending_wait, held_next) {
            (Some(p), Some((_, h))) => Some(p.min(h)),
            (p, h) => p.or(h.map(|x| x.1)),
        };
        // A caller's action comes after timers and the scene clock on a tie,
        // and when neither has anything due.
        let next_known = match (timer, due) {
            (Some(Some(t)), Some(d)) => Some(t.min(d)),
            (Some(Some(t)), None) => Some(t),
            (None, Some(d)) => Some(d),
            _ => None,
        };
        let stuck = matches!(timer, Some(None));
        // The world model's next due second: after timers and the scene
        // clock on a tie, before a caller's action on a tie
        // (`world_trigger_instant_v1`).
        let max = store.limits.max_memory;
        let track_wait = [
            crate::track::next_due(store, max, &mut self.vm.world),
            crate::talk::next_due(&self.vm.world),
            crate::anim::next_due(&self.vm.world),
            crate::load::next_due(store, &self.vm.world),
        ]
        .into_iter()
        .flatten()
        .min_by(f64::total_cmp)
        .map(|t| (t - now).max(0.0));
        // A stepped run: what is due first lies past the input's horizon
        // (or nothing is due): the caller gives the next slice first.
        if let Some(input) = self.input.as_ref().filter(|i| i.open) {
            if !stuck {
                let earliest = [next_known, wait, track_wait]
                    .into_iter()
                    .flatten()
                    .min_by(f64::total_cmp)
                    .map(|w| now + w);
                if earliest.is_none_or(|t| t > input.horizon) {
                    return Ok(Turn::NeedInput);
                }
            }
        }
        if let Some(tw) = track_wait {
            if !stuck && next_known.is_none_or(|n| tw < n) && wait.is_none_or(|wa| tw <= wa) {
                self.vm.world.now[crate::world::Clock::Real as usize] = now + tw;
                if let Some(s) = scale {
                    self.vm.world.now[crate::world::Clock::GameTime as usize] += tw * s / 3600.0;
                }
                self.world_step();
                self.note("the player's track");
                return Ok(Turn::Ran);
            }
        }
        if let Some(wa) = wait {
            if !stuck && next_known.is_none_or(|n| wa < n) {
                let t = now + wa;
                self.vm.world.now[crate::world::Clock::Real as usize] = t;
                if let Some(s) = scale {
                    self.vm.world.now[crate::world::Clock::GameTime as usize] += wa * s / 3600.0;
                }
                if let Some((pos, _)) =
                    held_next.filter(|&(_, h)| pending_wait.is_none_or(|p| h <= p))
                {
                    self.act_held(pos)?;
                    return Ok(Turn::Ran);
                }
                let (x, input) = self.pending.pop_front().expect("a pending action");
                if x.after.is_some() {
                    if !gate_done(self.vm, store, &x, b) {
                        let _ = writeln!(
                            self.names,
                            "## caller's action on {} held at {t} s: its stage is not done",
                            x.target()
                        );
                        self.held.push(Held { x, due: None });
                        return Ok(Turn::Ran);
                    }
                    bump(
                        self.c,
                        "caller actions held until a stage: happened at their second",
                    );
                }
                let open = self.gates_open();
                // A sit an activation with events brought that the track
                // already left ends before the action (`--sit-end-walk-events`).
                if self.vm.world.sits.end_walk_events.is_some() {
                    sit_step(
                        self.vm, self.store, self.then, self.c, self.names, self.top, self.b,
                    );
                }
                self.end_sit_for(&x);
                self.act(&x)?;
                self.world_step();
                let written = self.written_of(&x, input);
                self.gates_done(&open, written);
                self.note("a caller's action");
                return Ok(Turn::Ran);
            }
        }
        let fire = match (timer, due) {
            (None, None) => return Ok(Turn::Stopped),
            // A queue that cannot be ordered: fire to record why, then stop.
            (Some(None), _) => true,
            (Some(Some(t)), Some(d)) => t <= d,
            (Some(Some(_)), None) => true,
            (None, Some(_)) => false,
        };
        if fire {
            let before = self.vm.effects.len();
            let Some(x) = self.vm.fire_timer_scaled(scale) else {
                self.timers = 0;
                return Ok(Turn::Ran);
            };
            let _ = writeln!(self.names, "## timer fired (time scale {scale:?})");
            for e in &self.vm.effects[before..] {
                let _ = writeln!(
                    self.names,
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
            self.timers = if matches!(x.2, Outcome::Failed(_)) {
                0
            } else if self.then.timers_per_timer {
                if let Some(k) = next_key {
                    *self.timer_uses.entry(k).or_default() += 1;
                }
                self.timers
            } else {
                self.timers - 1
            };
            self.fired.push(x);
            self.top
                .extend(quest::run_jobs(self.vm, store, MAX_GAME_JOBS, b));
            recheck(self.vm, store, then, self.c, self.top, b);
            self.world_step();
            self.note("a timer fired");
        } else {
            let t = scene::advance_clock(&mut self.vm.world, scale);
            self.vm.rules.insert(crate::rules::SCENE_CLOCK);
            bump(self.c, "scene clock advances");
            let _ = writeln!(self.names, "## scene clock advanced to {t:?} s");
            self.steps -= 1;
            self.top
                .extend(quest::run_jobs(self.vm, store, MAX_GAME_JOBS, b));
            recheck(self.vm, store, then, self.c, self.top, b);
            self.world_step();
            self.note("the scene clock advanced");
        }
        Ok(Turn::Ran)
    }

    /// After the clocks stopped: the caller's actions left, in order, each
    /// at its second (a gated one only once its stage is done).
    fn close(&mut self) -> Result<(), E> {
        self.ended = true;
        self.count_prefix();
        let (store, b) = (self.store, self.b);
        if self.then.action_offset {
            return self.close_offset();
        }
        loop {
            if let Some(pos) = self
                .held
                .iter()
                .position(|h| gate_done(self.vm, store, &h.x, b))
            {
                let x = self.held.remove(pos).x;
                bump(self.c, "caller actions held until a stage: happened when the stage was done, after their second");
                self.act(&x)?;
                self.world_step();
                self.note("a caller's held action");
                continue;
            }
            let Some((x, _)) = self.pending.pop_front() else {
                break;
            };
            let now = self.now();
            let t = x.at.unwrap_or(now).max(now);
            self.vm.world.now[crate::world::Clock::Real as usize] = t;
            if x.after.is_some() {
                if !gate_done(self.vm, store, &x, b) {
                    let _ = writeln!(
                        self.names,
                        "## caller's action on {} held at {t} s: its stage is not done",
                        x.target()
                    );
                    self.held.push(Held { x, due: None });
                    continue;
                }
                bump(
                    self.c,
                    "caller actions held until a stage: happened at their second",
                );
            }
            self.act(&x)?;
            self.world_step();
            self.note("a caller's action");
        }
        self.never_happened();
        Ok(())
    }

    /// The held actions whose stage was never done, counted and named.
    fn never_happened(&mut self) {
        for h in &self.held {
            bump(
                self.c,
                "caller actions held until a stage: never happened (the stage was never done)",
            );
            let _ = writeln!(
                self.names,
                "## caller's action on {} never happened: its stage was never done",
                h.x.target()
            );
        }
    }

    /// [`NewGame::close`] under `--action-after-offset`: the caller's
    /// actions left in the order of their seconds, a held one at the
    /// second its gate gave it.
    fn close_offset(&mut self) -> Result<(), E> {
        let (store, b) = (self.store, self.b);
        loop {
            self.release_gates();
            let now = self.now();
            let held = self.next_held();
            let first = self.pending.front().map(|x| x.0.at.unwrap_or(now).max(now));
            if let Some((pos, t)) = held.filter(|&(_, t)| first.is_none_or(|p| t <= p)) {
                self.vm.world.now[crate::world::Clock::Real as usize] = t.max(now);
                self.act_held(pos)?;
                continue;
            }
            let Some((x, input)) = self.pending.pop_front() else {
                break;
            };
            let t = x.at.unwrap_or(now).max(now);
            self.vm.world.now[crate::world::Clock::Real as usize] = t;
            if x.after.is_some() {
                if !gate_done(self.vm, store, &x, b) {
                    let _ = writeln!(
                        self.names,
                        "## caller's action on {} held at {t} s: its stage is not done",
                        x.target()
                    );
                    self.held.push(Held { x, due: None });
                    continue;
                }
                bump(
                    self.c,
                    "caller actions held until a stage: happened at their second",
                );
            }
            let open = self.gates_open();
            self.act(&x)?;
            self.world_step();
            let written = self.written_of(&x, input);
            self.gates_done(&open, written);
            self.note("a caller's action");
        }
        self.never_happened();
        Ok(())
    }

    /// Under `--action-after-offset`: the held action due first that its
    /// gate gave a second (the earlier held on a tie), and that second.
    fn next_held(&self) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for (i, h) in self.held.iter().enumerate() {
            if let Some(t) = h.due {
                if best.is_none_or(|(_, b)| t < b) {
                    best = Some((i, t));
                }
            }
        }
        best
    }

    /// Under `--action-after-offset`: a held action at the second its gate
    /// gave it.
    /// Under `--sit-end-other-activation`: a sit an activation with events
    /// brought ends, as a get-up delivered now, before the caller's or the
    /// input's activation of another reference than the furniture.
    fn end_sit_for(&mut self, x: &Caller) {
        let What::Player(crate::player::Action::Activate, k) = &x.what else {
            return;
        };
        if crate::sit::end_for_activation(&mut self.vm.world, k) {
            self.vm
                .rules
                .insert(crate::rules::SIT_EVENTS_ENDED_BY_OTHER_ACTIVATION);
            sit_step(
                self.vm, self.store, self.then, self.c, self.names, self.top, self.b,
            );
        }
    }

    fn act_held(&mut self, pos: usize) -> Result<(), E> {
        let h = self.held.remove(pos);
        bump(
            self.c,
            "caller actions held until a stage: happened at their offset from the stage (--action-after-offset)",
        );
        let open = self.gates_open();
        self.end_sit_for(&h.x);
        self.act(&h.x)?;
        self.world_step();
        self.gates_done(&open, h.x.at);
        self.note("a caller's held action");
        Ok(())
    }

    /// Under `--action-after-offset` (`caller_action_after_keeps_offset_v1`):
    /// for each gate whose stage is now done and that holds actions not
    /// given a second yet, the reference second (the written second of the
    /// caller's action in whose turn the stage was done, else the earliest
    /// held action's second); each held action then happens at the present
    /// second plus its own second minus the reference (never negative), and
    /// the gate's actions still waiting for their second move with them
    /// (at their second plus as much, never before it).
    fn release_gates(&mut self) {
        let (store, b) = (self.store, self.b);
        let now = self.now();
        let mut gates: Vec<(String, i32)> = Vec::new();
        for h in self.held.iter().filter(|h| h.due.is_none()) {
            if let Some(g) = &h.x.after {
                if !gates.contains(g) {
                    gates.push(g.clone());
                }
            }
        }
        for g in gates {
            if !stage_done(self.vm, store, &g.0, g.1, b) {
                continue;
            }
            let on_gate = |h: &Held| h.due.is_none() && h.x.after.as_ref() == Some(&g);
            let earliest = self
                .held
                .iter()
                .filter(|h| on_gate(h))
                .filter_map(|h| h.x.at)
                .fold(f64::INFINITY, f64::min);
            let written = self.gate_written.get(&g).copied();
            let r = gate_reference(written, earliest);
            let from = if written.is_some() {
                "the written second of the caller's action that did it"
            } else {
                "the earliest held action's second"
            };
            self.vm.rules.insert(crate::rules::ACTION_AFTER_OFFSET);
            let _ = writeln!(
                self.names,
                "## caller's gate {}:{} done at {now} s: its held actions keep their offset from {r} s ({from})",
                g.0, g.1
            );
            for h in self.held.iter_mut().filter(|h| on_gate(h)) {
                h.due = Some(held_due(now, h.x.at.unwrap_or(now), r));
            }
            let mut k = 0;
            while k < self.pending.len() {
                if self.pending[k].0.after.as_ref() == Some(&g) {
                    let (x, _) = self.pending.remove(k).expect("an index in range");
                    let at = x.at.unwrap_or(now);
                    bump(
                        self.c,
                        "caller actions held until a stage: waiting for their second, moved with their gate's held actions (--action-after-offset)",
                    );
                    self.held.push(Held {
                        x,
                        due: Some(waiting_due(now, at, r)),
                    });
                } else {
                    k += 1;
                }
            }
        }
    }

    /// Under `--action-after-offset`: the gates not done (and not given a
    /// written second) before a caller's action.
    fn gates_open(&mut self) -> Vec<usize> {
        let mut open = Vec::new();
        if !self.then.action_offset {
            return open;
        }
        let (store, b) = (self.store, self.b);
        for (i, (q, n)) in self.gates.iter().enumerate() {
            if !self.gate_written.contains_key(&(q.clone(), *n))
                && !stage_done(self.vm, store, q, *n, b)
            {
                open.push(i);
            }
        }
        open
    }

    /// After a caller's action and its world step: the gates its turn did
    /// take the second it was written for, when it has one.
    fn gates_done(&mut self, open: &[usize], written: Option<f64>) {
        let Some(w) = written else {
            return;
        };
        let (store, b) = (self.store, self.b);
        for &i in open {
            let (q, n) = self.gates[i].clone();
            if stage_done(self.vm, store, &q, n, b) {
                self.gate_written.insert((q, n), w);
            }
        }
    }

    /// The second a caller's action was written for: a command-line
    /// action's own; an input action's, the second `--input-action-at`
    /// names for its action and reference, else none.
    fn written_of(&self, x: &Caller, input: bool) -> Option<f64> {
        if let What::Player(action, key) = &x.what {
            if let Some((_, _, w)) = self
                .then
                .input_written
                .iter()
                .find(|(a, k, _)| a == action && k.eq_ignore_ascii_case(key))
            {
                return Some(*w);
            }
        }
        if input {
            None
        } else {
            x.at
        }
    }

    /// With [`Stepping::region`]: reads the trigger volumes and placed
    /// actors about each new point whose cells the run has not read yet
    /// (`Place::load_with` about that one point), appends those it does not
    /// hold, folds the whole track so far into their crossings and rebuilds
    /// the crossing lists. Returns how many volumes and actors were added,
    /// and the rule; `None` without the reading.
    fn grow(&mut self, points: &[Point]) -> Result<Option<(usize, usize, &'static str)>, E> {
        let Some((mut region, rule)) = self.input.as_mut().and_then(|i| i.region.take()) else {
            return Ok(None);
        };
        let store = self.store;
        if region.squares.is_empty() && region.interiors.is_empty() {
            if let Some(t) = self.input.as_ref().and_then(|i| i.loaded.clone()) {
                region = Region::of(store, &t.points);
            }
        }
        let mut added = (0, 0);
        for p in points {
            if region.claim(store, p) {
                continue;
            }
            self.count("play: regions read about the walker's points", 1);
            let w = &mut self.vm.world;
            let around = crate::track::Place::load_with(
                store,
                Track {
                    points: vec![p.clone()],
                },
                w.place.sw,
                Vec::new(),
                self.b,
            )?;
            added.0 += w.place.add_volumes(around.volumes);
            let actors: std::collections::BTreeSet<FormKey> =
                w.place.actors.iter().map(|a| a.key.clone()).collect();
            for a in around.actors {
                if actors.contains(&a.key) {
                    continue;
                }
                w.talk.add_actor(&w.place, &a);
                w.place.actors.push(a);
                added.1 += 1;
            }
        }
        if let Some(i) = self.input.as_mut() {
            i.region = Some((region, rule));
        }
        if added != (0, 0) {
            self.count(
                "play: trigger volumes read about the walker's points",
                added.0 as u64,
            );
            self.count(
                "play: placed actors read about the walker's points",
                added.1 as u64,
            );
            let w = &mut self.vm.world;
            w.place.rebuild();
            w.talk.rebuild(&w.place);
        }
        Ok(Some((added.0, added.1, rule)))
    }

    /// With `--cond-1-distance` (`cond_fn_1_track_distance_v1`) a scene
    /// whose phase waits on completion conditions with a function 1
    /// condition gets its marks from the track as known when it began to
    /// wait; `quest_run` knows the whole track then, a stepped run only the
    /// track to the horizon. So when the track grows, each such scene with
    /// no mark left is offered its completion again (`scene::step_call`),
    /// which reads the marks on the longer track. Each offer that finds the
    /// conditions still false logs one more wait than `quest_run` does.
    fn reoffer_distance_waits(&mut self) -> u64 {
        let vm = &mut *self.vm;
        let mut last: BTreeMap<crate::world::ObjId, Option<usize>> = BTreeMap::new();
        for (o, ch) in &vm.world.scenes.log {
            let p = match ch {
                scene::Change::Waiting(p, why)
                    if why.starts_with("completion conditions false") =>
                {
                    Some(*p)
                }
                _ => None,
            };
            last.insert(*o, p);
        }
        let mut offered = 0u64;
        for (o, p) in last {
            let Some(p) = p else { continue };
            if vm.world.scenes.percent_marks.iter().any(|(s, _)| *s == o) {
                continue;
            }
            let Some(key) = vm.world.objects.get(o).and_then(|x| x.key.clone()) else {
                continue;
            };
            let Some(Some(info)) = vm.world.scenes.infos.get(&key).cloned() else {
                continue;
            };
            let distance = info.phases.get(p).is_some_and(|ph| {
                ph.completion
                    .iter()
                    .any(|x| x.ctda.function == crate::cond::DISTANCE)
            });
            if !distance {
                continue;
            }
            let step = scene::complete_step(&vm.world, o, p);
            let _ = vm.host.call(&scene::step_call(o, &step), &mut vm.world);
            for r in vm.host.rules() {
                vm.rules.insert(r);
            }
            offered += 1;
            let _ = writeln!(
                self.names,
                "## play: scene {} offered its completion again on the grown track",
                vm.world.objects[o].label
            );
        }
        if offered > 0 {
            self.count(
                "play: scenes waiting on a distance condition offered their completion again as the track grew",
                offered,
            );
            self.top
                .extend(quest::run_jobs(self.vm, self.store, MAX_GAME_JOBS, self.b));
        }
        offered
    }

    /// With [`Stepping::recheck_marks`]: the marks of the scenes waiting on
    /// a function 1 condition computed again on the grown track
    /// ([`scene::recheck_distance_marks`]); nothing is offered.
    fn recheck_marks(&mut self) -> u64 {
        let vm = &mut *self.vm;
        let n = scene::recheck_distance_marks(&mut *vm.host, &mut vm.world);
        for r in vm.host.rules() {
            vm.rules.insert(r);
        }
        if n > 0 {
            self.count(
                "play: scenes waiting on a distance condition given marks again as the track grew",
                n as u64,
            );
        }
        n as u64
    }
}

/// A timer's identity for the per-timer budget: its instance, clock and id.
pub(crate) fn timer_key(t: &crate::world::Timer) -> String {
    format!("{}\t{:?}\t{:?}", t.instance, t.clock, t.id)
}

/// Under `vm_timer_budget_per_timer_v1`: drops from the queue each timer
/// due next that has fired `budget` times (`uses`, by [`timer_key`]),
/// until the one due next has budget left. Returns how many were dropped,
/// the key of the timer due next, and whether that timer is refused
/// because [`super::MAX_TIMER_KEYS`] distinct timers have already fired
/// (then the key is `None`).
pub(crate) fn drop_spent_timers(
    vm: &mut Vm<'_>,
    scale: Option<f64>,
    uses: &BTreeMap<String, usize>,
    budget: usize,
) -> (usize, Option<String>, bool) {
    let mut dropped = 0;
    while let Some(i) = vm.next_timer_index(scale) {
        let key = timer_key(&vm.world.timers[i]);
        match uses.get(&key).copied() {
            None if uses.len() >= super::MAX_TIMER_KEYS => return (dropped, None, true),
            Some(n) if n >= budget => {}
            _ => return (dropped, Some(key), false),
        }
        vm.world.timers.remove(i);
        vm.rules.insert(crate::rules::TIMER_BUDGET_PER_TIMER);
        dropped += 1;
    }
    (dropped, None, false)
}
