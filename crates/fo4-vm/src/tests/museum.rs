//! The museum step's readings over synthetic surfaces, plugins and scripts
//! (no game data): the walk order over a strip of triangles, a property on
//! an object without an instance, a global's value and a lock the run
//! holds, the caller's item counts, and the timeline's deaths and sits.
use super::assemble;
use super::quest::{group, key, record, sub, Owned};
use crate::{
    quest::QuestHost,
    rules, runstate,
    timeline::Recorder,
    track::{Shape, Volume},
    walk_order::{self as wo, Surface},
    NativeCall, Outcome, Reply, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use fo4_scene::transform::Transform;

const CELL: u32 = 0x930;
const DOOR: u32 = 0x931;
const PLAIN: u32 = 0x932;
const BASE: u32 = 0x940;
const GLOB: u32 = 0x950;
const ITEM: u32 = 0x951;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A strip of triangles along x from 0 to 400, 100 wide (y 0 to 100), at
/// height 0: square k (x from 100k) is two triangles, joined in order.
fn strip() -> Surface {
    let mut s = Surface::default();
    for k in 0..4 {
        let x0 = 100.0 * f64::from(k);
        let x1 = x0 + 100.0;
        s.tris
            .push([[x0, 0.0, 0.0], [x1, 0.0, 0.0], [x1, 100.0, 0.0]]);
        s.tris
            .push([[x0, 0.0, 0.0], [x1, 100.0, 0.0], [x0, 100.0, 0.0]]);
    }
    let n = s.tris.len();
    for t in 0..n {
        let mut next = [None; 3];
        // Lower triangle 2k: edge 2 (back to corner 0) is the diagonal to
        // 2k + 1; upper triangle 2k + 1: edge 0 is the diagonal, edge 2
        // (from (x0, 100) to (x0, 0)) joins the previous square's lower
        // triangle at its edge 1.
        if t % 2 == 0 {
            next[2] = Some(t + 1);
            if t + 2 < n {
                next[1] = Some(t + 3);
            }
        } else {
            next[0] = Some(t - 1);
            if t >= 3 {
                next[2] = Some(t - 3);
            }
        }
        s.next.push(next);
    }
    s
}

fn boxed(centre: [f64; 3], half: [f64; 3]) -> Volume {
    let mut frame = Transform::IDENTITY;
    frame.translation = centre;
    Volume {
        key: key(0x999),
        space: key(CELL),
        frame,
        shape: Shape::Box(half),
    }
}

#[test]
fn a_volume_across_the_strip_is_entered_first_and_one_beside_a_gap_is_not() {
    let s = strip();
    let h = 120.0;
    let start = [10.0, 50.0, 0.0];
    let goal = boxed([390.0, 50.0, 0.0], [10.0, 10.0, 10.0]);
    // Nothing blocks: the goal is reached.
    assert_eq!(wo::reach(&s, &[], start, &goal, h), Some(true));
    assert_eq!(wo::split(&s, &[], h).sizes(), vec![8]);
    // A wall across the strip at x 200: every walk enters it first.
    let wall = boxed([200.0, 50.0, 0.0], [5.0, 60.0, 10.0]);
    assert_eq!(wo::reach(&s, &[&wall], start, &goal, h), Some(false));
    assert_eq!(
        wo::entered_first(&s, &[], start, &goal, &[&wall], h),
        vec![0]
    );
    // A door 90 wide leaves 10 units free: passable; grown by 6 units it
    // still leaves 4; grown by 12 it closes the gap.
    let door = boxed([200.0, 45.0, 0.0], [5.0, 45.0, 10.0]);
    assert_eq!(wo::reach(&s, &[&door], start, &goal, h), Some(true));
    assert!(wo::entered_first(&s, &[], start, &goal, &[&door], h).is_empty());
    assert_eq!(
        wo::reach(&s, &[&wo::grown(&door, 6.0)], start, &goal, h),
        Some(true)
    );
    let closed = wo::grown(&door, 12.0);
    assert_eq!(wo::reach(&s, &[&closed], start, &goal, h), Some(false));
    let mut sp = wo::split(&s, &[&closed], h);
    assert_eq!(sp.sizes().len(), 2, "the strip falls into two pieces");
    assert_eq!(sp.multi, 0);
    // A volume high above the floor beyond the body's reach blocks nothing.
    let high = boxed([200.0, 50.0, 400.0], [5.0, 60.0, 10.0]);
    assert_eq!(wo::reach(&s, &[&high], start, &goal, h), Some(true));
    // An entry inside the blocking volume stands on no free surface only
    // when the whole triangle is covered; here it is on the near piece.
    assert!(wo::entry(&s, &mut sp, &[&closed], start, h).is_some());
}

/// A plugin with a cell holding a door with a lock record and a door
/// without, a global and an item.
fn store() -> Store {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(20u32.to_le_bytes());
    hedr.extend(0x990u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(
        0,
        *b"GLOB",
        &record(
            b"GLOB",
            GLOB,
            &[
                sub(b"EDID", b"G\0"),
                sub(b"FNAM", b"f"),
                sub(b"FLTV", &7.0f32.to_le_bytes()),
            ]
            .concat(),
        ),
    ));
    plugin.extend(group(
        0,
        *b"MISC",
        &record(b"MISC", ITEM, &sub(b"EDID", b"Item\0")),
    ));
    plugin.extend(group(
        0,
        *b"DOOR",
        &record(b"DOOR", BASE, &sub(b"EDID", b"D\0")),
    ));
    let mut xloc = vec![255u8];
    xloc.extend([0u8; 15]);
    let refs = [
        record(
            b"REFR",
            DOOR,
            &[sub(b"NAME", &w(BASE)), sub(b"XLOC", &xloc)].concat(),
        ),
        record(b"REFR", PLAIN, &sub(b"NAME", &w(BASE))),
    ]
    .concat();
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    plugin.extend(group(
        0,
        *b"CELL",
        &group(
            2,
            [0; 4],
            &[
                cell,
                group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs)),
            ]
            .concat(),
        ),
    ));
    let sources: Vec<(String, Box<dyn ReadAt>)> =
        vec![("test.esm".into(), Box::new(Owned(plugin)))];
    Store::from_sources(
        fo4_core::Game::Fallout4,
        sources,
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap()
}

fn lib() -> crate::MapLibrary {
    let mut l = super::quest::library();
    l.add(assemble(
        "script GlobalVariable extends Form
         func GetValue Float native
         func SetValue none native Float afValue
         getprop Float value
         local Float ::v
         23 GetValue self ::v 0
         26 ::v",
    ));
    l.add(assemble(
        "script Reader extends ObjectReference
         var GlobalVariable g
         var Float out
         func OnActivate none ObjectReference akActionRef
         28 value g out",
    ));
    l
}

fn call(script: &str, name: &str, receiver: Val, args: Vec<Val>, ret: &str) -> NativeCall {
    NativeCall {
        script: script.into(),
        name: name.into(),
        receiver: Some(receiver),
        instance: None,
        args,
        param_types: Vec::new(),
        return_type: ret.into(),
    }
}

#[test]
fn a_property_on_an_object_without_an_instance_runs_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let g =
        crate::data::record_object(&mut vm.world, &s, &key(GLOB), "globalvariable", &b).unwrap();
    vm.world.objects[g].attached = true;
    let r = vm.instantiate(None, "reader").unwrap();
    vm.set_property_variable(r, "g", Val::Object(g)).ok();
    if let Some(v) = vm.world.instances[r]
        .vars
        .iter_mut()
        .find(|v| v.name.eq_ignore_ascii_case("g"))
    {
        v.value = Val::Object(g);
    }
    // Off: no instance of the global's script; the run blocks.
    assert!(matches!(
        vm.deliver(r, "onactivate", vec![Val::None]),
        Outcome::Blocked(_)
    ));
    vm.property_implicit_instance = true;
    assert!(matches!(
        vm.deliver(r, "onactivate", vec![Val::None]),
        Outcome::Completed(_)
    ));
    let out = vm.world.instances[r]
        .vars
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case("out"))
        .map(|v| v.value.clone());
    assert_eq!(
        out,
        Some(Val::Float(7.0)),
        "the stored FLTV through GetValue"
    );
    assert!(vm.rules.contains(rules::PROPERTY_IMPLICIT_INSTANCE));
}

#[test]
fn a_global_a_lock_and_an_item_count_answer_the_run_only_under_their_switches() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let w = &mut vm.world;
    let g = crate::data::record_object(w, &s, &key(GLOB), "globalvariable", &b).unwrap();
    let d = crate::data::record_object(w, &s, &key(DOOR), "objectreference", &b).unwrap();
    let p = crate::data::record_object(w, &s, &key(PLAIN), "objectreference", &b).unwrap();
    let item = crate::data::record_object(w, &s, &key(ITEM), "form", &b).unwrap();
    let player = w.player("actor");
    let set = call(
        "globalvariable",
        "setvalue",
        Val::Object(g),
        vec![Val::Float(3.0)],
        "none",
    );
    let get = call(
        "globalvariable",
        "getvalue",
        Val::Object(g),
        vec![],
        "float",
    );
    let locked = |o| {
        call(
            "objectreference",
            "islocked",
            Val::Object(o),
            vec![],
            "bool",
        )
    };
    let count = call(
        "objectreference",
        "getitemcount",
        Val::Object(player),
        vec![Val::Object(item)],
        "int",
    );
    // Everything off: left to the other hosts.
    for c in [&set, &get, &locked(d), &count] {
        assert!(runstate::native(&s, 1 << 24, c, w).is_none());
    }
    w.runstate.globals = true;
    // Before a set, GetValue is left to the data host (the stored value).
    assert!(runstate::native(&s, 1 << 24, &get, w).is_none());
    assert!(matches!(
        runstate::native(&s, 1 << 24, &set, w),
        Some(Reply::Value(Val::None))
    ));
    assert!(matches!(
        runstate::native(&s, 1 << 24, &get, w),
        Some(Reply::Value(Val::Float(x))) if x == 3.0
    ));
    w.runstate.locks = true;
    assert!(matches!(
        runstate::native(&s, 1 << 24, &locked(d), w),
        Some(Reply::Value(Val::Bool(true)))
    ));
    assert!(matches!(
        runstate::native(&s, 1 << 24, &locked(p), w),
        Some(Reply::Value(Val::Bool(false)))
    ));
    let unlock = call(
        "objectreference",
        "lock",
        Val::Object(d),
        vec![Val::Bool(false), Val::Bool(false)],
        "none",
    );
    assert!(runstate::native(&s, 1 << 24, &unlock, w).is_some());
    assert!(matches!(
        runstate::native(&s, 1 << 24, &locked(d), w),
        Some(Reply::Value(Val::Bool(false)))
    ));
    // The caller's item count, for that holder and item only.
    let ik = w.objects[item].key.clone().unwrap();
    w.runstate.item_counts.insert(("player".into(), ik), 0);
    assert!(matches!(
        runstate::native(&s, 1 << 24, &count, w),
        Some(Reply::Value(Val::Int(0)))
    ));
    let other = call(
        "objectreference",
        "getitemcount",
        Val::Object(d),
        vec![Val::Object(item)],
        "int",
    );
    assert!(runstate::native(&s, 1 << 24, &other, w).is_none());
    let used = std::mem::take(&mut w.runstate.used);
    assert!(used.contains(&rules::GLOBAL_RUNTIME_VALUE));
    assert!(used.contains(&rules::LOCK_STATE));
}

#[test]
fn the_timeline_writes_deaths_and_sits_only_under_its_switches() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let a = crate::data::record_object(&mut vm.world, &s, &key(DOOR), "actor", &b).unwrap();
    let f =
        crate::data::record_object(&mut vm.world, &s, &key(PLAIN), "objectreference", &b).unwrap();
    let player = vm.world.player("actor");
    let notes = |r: &Recorder, vm: &Vm<'_>| -> Vec<String> {
        r.finish(vm, &s, None, &b)
            .events
            .iter()
            .filter_map(|e| match &e.what {
                fo4_scene::timeline::What::Note(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    };
    let mut off = Recorder::new();
    let mut on = Recorder::new();
    on.deaths = true;
    on.sits = true;
    vm.world.now[crate::world::Clock::Real as usize] = 5.0;
    vm.world.fights.dead.insert(a);
    vm.world.sits.sitting.insert(player, f);
    off.observe(&vm);
    on.observe(&vm);
    vm.world.now[crate::world::Clock::Real as usize] = 6.0;
    vm.world.sits.sitting.clear();
    off.observe(&vm);
    on.observe(&vm);
    assert!(notes(&off, &vm).is_empty());
    assert_eq!(
        notes(&on, &vm),
        vec![
            "death test.esm:00000931".to_string(),
            "sit player test.esm:00000932".to_string(),
            "get-up player test.esm:00000932".to_string(),
        ]
    );
}

#[test]
fn the_callers_unlock_holds_the_reference_unlocked_and_raises_the_event_only_under_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    l.add(assemble(
        "script Gate extends ObjectReference
         var Int changed 0
         func OnLockStateChanged none
         1 changed changed 1",
    ));
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let d =
        crate::data::record_object(&mut vm.world, &s, &key(DOOR), "objectreference", &b).unwrap();
    vm.world.objects[d].attached = true;
    let g = vm.instantiate(Some(d), "gate").unwrap();
    vm.world.runstate.locks = true;
    let r = runstate::unlock(&mut vm, &s, d, &b).unwrap();
    assert!(r.runs.is_empty(), "no event without --lock-events");
    let locked = call(
        "objectreference",
        "islocked",
        Val::Object(d),
        vec![],
        "bool",
    );
    assert!(matches!(
        runstate::native(&s, 1 << 24, &locked, &mut vm.world),
        Some(Reply::Value(Val::Bool(false)))
    ));
    vm.world.runstate.lock_events = true;
    let r = runstate::unlock(&mut vm, &s, d, &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    let changed = vm.world.instances[g]
        .vars
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case("changed"))
        .map(|v| v.value.clone());
    assert_eq!(changed, Some(Val::Int(1)));
    assert!(vm.rules.contains(rules::LOCK_STATE_EVENT));
}

#[test]
fn every_rule_of_the_museum_step_is_registered_pending() {
    for r in [
        rules::PROPERTY_IMPLICIT_INSTANCE,
        rules::GLOBAL_RUNTIME_VALUE,
        rules::LOCK_STATE,
        rules::LOCK_STATE_EVENT,
        rules::NPC_RESPONSE_CHOICE,
        rules::WALK_NAVMESH_SURFACE,
        rules::WALK_DOOR_BOUNDS_BLOCK,
        rules::WALK_DOOR_LOCK_RECORD_CLOSED,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn a_global_set_to_an_undetermined_value_answers_undetermined_only_under_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let w = &mut vm.world;
    let g = crate::data::record_object(w, &s, &key(GLOB), "globalvariable", &b).unwrap();
    let set = |v: Val| {
        call(
            "globalvariable",
            "setvalue",
            Val::Object(g),
            vec![v],
            "none",
        )
    };
    let get = call(
        "globalvariable",
        "getvalue",
        Val::Object(g),
        vec![],
        "float",
    );
    w.runstate.globals = true;
    runstate::native(&s, 1 << 24, &set(Val::Float(3.0)), w);
    // Off: an undetermined set drops the run's value; GetValue goes back
    // to the data host's stored value, as before.
    assert!(runstate::native(&s, 1 << 24, &set(Val::None), w).is_none());
    assert!(runstate::native(&s, 1 << 24, &get, w).is_none());
    assert!(!w.runstate.used.contains(&rules::GLOBAL_VALUE_UNDETERMINED));
    // On: undetermined until a known value is set.
    w.runstate.undetermined_values = true;
    assert!(runstate::native(&s, 1 << 24, &set(Val::None), w).is_none());
    assert!(matches!(
        runstate::native(&s, 1 << 24, &get, w),
        Some(Reply::Undetermined)
    ));
    runstate::native(&s, 1 << 24, &set(Val::Int(5)), w);
    assert!(matches!(
        runstate::native(&s, 1 << 24, &get, w),
        Some(Reply::Value(Val::Float(x))) if x == 5.0
    ));
    assert!(w.runstate.used.contains(&rules::GLOBAL_VALUE_UNDETERMINED));
    assert_eq!(
        rules::status(rules::GLOBAL_VALUE_UNDETERMINED),
        Some("pending")
    );
    assert!(rules::ALL.contains(&rules::GLOBAL_VALUE_UNDETERMINED));
}

#[test]
fn a_response_action_plays_the_topic_at_the_assumed_choice_only_under_its_switch() {
    use crate::scene::{self, ActionInfo, SceneInfo, NPC_RESPONSE};
    let s = store();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut w = crate::World::default();
    let topics = [key(0x9a0), key(0x9a1), key(0x9a2)];
    let a = ActionInfo {
        kind: NPC_RESPONSE,
        responses: vec![
            Some(topics[0].clone()),
            Some(topics[1].clone()),
            None,
            Some(topics[2].clone()),
        ],
        ..ActionInfo::default()
    };
    let i = SceneInfo {
        key: key(0x9b0),
        plugin: 0,
        file: "test.esm".into(),
        flags: 0,
        quest: None,
        begin: None,
        end: None,
        phases: Vec::new(),
        actions: vec![a.clone()],
        stage_fields: [-1, -1],
        unplaced: 0,
    };
    w.scenes.clock.on = true;
    w.scenes.clock.assume_line_seconds = Some(3.0);
    // Off: the response action is not timed and plays no line.
    let (t, why) = scene::action_time(&mut host, &mut w, &i, &a);
    assert_eq!(t, None, "{why}");
    assert!(scene::line_segments(&mut host, &mut w, &i, &a).is_empty());
    // On, without a choice: it waits on the player's choice.
    w.scenes.clock.npc_responses = true;
    let (t, why) = scene::action_time(&mut host, &mut w, &i, &a);
    assert_eq!(
        (t, why.as_str()),
        (None, "response: waits on the player's choice")
    );
    // On, with each choice: the topic at that index, timed as a line.
    for (n, want) in [
        (0, Some(&topics[0])),
        (1, Some(&topics[1])),
        (2, None),
        (3, Some(&topics[2])),
    ] {
        w.scenes.clock.assume_player_choice = Some(n);
        let (t, why) = scene::action_time(&mut host, &mut w, &i, &a);
        assert_eq!(t, Some(3.0), "{why}");
        assert!(
            why.starts_with(&format!("response: assumed choice {n} ")),
            "{why}"
        );
        let segs = scene::line_segments(&mut host, &mut w, &i, &a);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].0.as_ref(), want, "choice {n}");
        assert_eq!((segs[0].1, segs[0].2, segs[0].3), (0.0, Some(3.0), false));
    }
    assert!(crate::Host::rules(&host).contains(&rules::NPC_RESPONSE_CHOICE));
}
