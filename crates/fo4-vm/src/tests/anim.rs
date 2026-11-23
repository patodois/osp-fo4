//! Animation events: registrations, delivery, names compared ignoring case,
//! a sequence graph's transitions and text keys, and the order of actor
//! moves the world model keeps (no game data).
use super::assemble;
use super::quest::{library, record, sub, Owned};
use crate::{anim, quest::QuestHost, rules, track, Val, Vm};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_havok::{Machine, MachineState, Transition};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::anim_graph::{ModelGraph, Sequence};
use std::rc::Rc;

/// A plugin with its header only: the host answers nothing from records.
fn store() -> Store {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, &sub(b"HEDR", &hedr));
    plugin[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
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
    let mut l = library();
    l.add(assemble(
        "script ScriptObject
         func OnInit none
         func RegisterForAnimationEvent Bool native ObjectReference akSender String asEventName
         func UnregisterForAnimationEvent none native ObjectReference akSender String asEventName",
    ));
    l.add(assemble(
        "script Listener extends ObjectReference
         var String got \"\"
         var Int count 0
         func Reg none String asName
         local Bool ::b
         local ObjectReference ::me
         14 ::me self
         23 RegisterForAnimationEvent self ::b 2 ::me asName
         func Unreg none String asName
         local none ::n
         local ObjectReference ::me
         14 ::me self
         23 UnregisterForAnimationEvent self ::n 2 ::me asName
         func OnAnimationEvent none ObjectReference akSource String asEventName
         13 got asEventName
         1 count count 1",
    ));
    l
}

fn var(vm: &Vm<'_>, i: crate::InstId, n: &str) -> Val {
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(n))
        .map(|v| v.value.clone())
        .unwrap_or(Val::None)
}

fn transition(event: i32, to_state: i32) -> Transition {
    Transition {
        effect: None,
        condition: None,
        event,
        to_state,
        words: [0; 4],
    }
}

/// Two states: A (start, sequence `SA`) goes to B on `Go` (a wildcard);
/// B (sequence `SB`, keys `Ping` at 0.5 s and `Back` at 2 s) goes back
/// to A on `Back`.
fn graph() -> Rc<ModelGraph> {
    let state = |id, name: &str, seq: &str, transitions| MachineState {
        id,
        name: name.into(),
        generator: fo4_havok::SEQUENCE_GENERATOR.into(),
        sequence: Some(seq.into()),
        transitions,
        notify: false,
    };
    let seq = |name: &str, keys: Vec<(f32, &str)>| Sequence {
        name: name.into(),
        cycle: 2,
        frequency: 1.0,
        start: 0.0,
        stop: 3.0,
        keys: keys.into_iter().map(|(t, k)| (t, k.to_string())).collect(),
    };
    Rc::new(ModelGraph {
        nif: String::new(),
        project: String::new(),
        character: String::new(),
        behavior: String::new(),
        machine: Machine {
            start: 0,
            states: vec![
                state(0, "A", "SA", Vec::new()),
                state(1, "B", "SB", vec![transition(1, 0)]),
            ],
            wildcards: vec![transition(0, 1)],
            events: vec!["Go".into(), "Back".into(), "Ping".into()],
        },
        sequences: vec![
            seq("SA", vec![(0.0, "start")]),
            seq("SB", vec![(0.5, "Ping"), (2.0, "Back"), (3.0, "end")]),
        ],
    })
}

#[test]
fn a_registered_instance_receives_the_event_and_case_is_a_switch() {
    let store = store();
    let mut lib = lib();
    let mut host = QuestHost::new(&store, 1 << 20);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.world.anims.on = true;
    let o = vm.world.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    let i = vm.instantiate(Some(o), "listener").unwrap();
    vm.deliver(i, "reg", vec![Val::Str("Hit".into())]);
    assert_eq!(vm.world.anims.registrations.len(), 1);
    // Off by default: a name differing in case is another name.
    assert!(anim::deliver(&mut vm, o, "hit").is_empty());
    vm.world.anims.ignore_case = true;
    let out = anim::deliver(&mut vm, o, "hit");
    assert_eq!(out.len(), 1);
    assert_eq!(var(&vm, i, "got"), Val::Str("Hit".into()));
    assert!(vm.rules.contains(rules::ANIM_EVENTS));
    assert!(vm.rules.contains(rules::ANIM_EVENT_CASE));
    // Unregistered, nothing is delivered; the other sender's events never.
    vm.deliver(i, "unreg", vec![Val::Str("HIT".into())]);
    assert!(vm.world.anims.registrations.is_empty());
    assert!(anim::deliver(&mut vm, o, "Hit").is_empty());
}

#[test]
fn a_graph_takes_its_transitions_and_raises_the_keys_of_the_state_it_plays() {
    let store = store();
    let mut lib = lib();
    let mut host = QuestHost::new(&store, 1 << 20);
    let mut vm = Vm::new(&mut lib, &mut host);
    let w = &mut vm.world;
    w.anims.on = true;
    w.anims.graphs = true;
    w.anims.keys = true;
    let o = w.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    let i = vm.instantiate(Some(o), "listener").unwrap();
    vm.deliver(i, "reg", vec![Val::Str("Ping".into())]);
    anim::begin(&mut vm.world, o, graph(), 0.0);
    // An event with no transition from the start state changes nothing.
    anim::graph_event(&mut vm.world, o, "Back", 0.0);
    assert_eq!(vm.world.anims.runs[&o].state, 0);
    anim::graph_event(&mut vm.world, o, "Go", 0.0);
    assert_eq!(vm.world.anims.runs[&o].state, 1);
    // `end` names no event of the graph: two keys are due.
    assert_eq!(vm.world.anims.due.len(), 2);
    assert_eq!(anim::next_due(&vm.world), Some(0.5));
    let b = Budget::new(1 << 20);
    vm.world.now[crate::world::Clock::Real as usize] = 0.5;
    let (raised, _) = anim::settle(&mut vm, &store, 16, &b);
    assert_eq!(raised.len(), 1);
    assert_eq!(var(&vm, i, "got"), Val::Str("Ping".into()));
    // At 2 s the key `Back` is raised (no one registered) and takes the
    // graph back to its first state.
    vm.world.now[crate::world::Clock::Real as usize] = 2.0;
    let (raised, _) = anim::settle(&mut vm, &store, 16, &b);
    assert_eq!(raised.len(), 1);
    assert!(raised[0].runs.is_empty());
    assert_eq!(vm.world.anims.runs[&o].state, 0);
    // Leaving a state drops the keys not yet raised.
    anim::graph_event(&mut vm.world, o, "Go", 3.0);
    anim::graph_event(&mut vm.world, o, "Back", 3.1);
    vm.world.now[crate::world::Clock::Real as usize] = 6.0;
    let (raised, _) = anim::settle(&mut vm, &store, 16, &b);
    assert!(raised.is_empty());
    assert_eq!(var(&vm, i, "count"), Val::Int(1));
    assert_eq!(
        vm.world.anims.counts["text keys dropped\tthe state was left first"],
        2
    );
}

#[test]
fn actor_moves_stay_in_time_order_after_the_applied_ones() {
    let mut w = crate::World::default();
    let k = |id: u32| FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    };
    let m = |t: f64| track::ActorMove {
        t,
        actor: k(0x801),
        space: k(0x802),
        pos: [t, 0.0, 0.0],
    };
    w.place.actor_moves = vec![m(1.0), m(5.0), m(9.0)];
    w.place.actor_cursor = 1;
    track::schedule_move(&mut w, m(0.5));
    track::schedule_move(&mut w, m(7.0));
    let ts: Vec<f64> = w.place.actor_moves.iter().map(|x| x.t).collect();
    // A move at a second already passed waits right after the applied one.
    assert_eq!(ts, [1.0, 0.5, 5.0, 7.0, 9.0]);
}

/// A graph source that reads nothing (every graph refused).
struct NoGraphs;
impl anim::GraphSource for NoGraphs {
    fn graph(&mut self, _: &str) -> Result<Rc<ModelGraph>, String> {
        Err("no graph in the test".into())
    }
}

#[test]
fn a_furniture_event_takes_the_graphs_transition_only_with_graphs_followed() {
    let store = store();
    let mut lib = lib();
    let mut host = QuestHost::new(&store, 1 << 20);
    let mut vm = Vm::new(&mut lib, &mut host);
    let w = &mut vm.world;
    let o = w.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    anim::begin(w, o, graph(), 0.0);
    // Graphs not followed: nothing is sent.
    anim::furniture_event(&store, 1 << 20, &mut NoGraphs, o, "Go", w);
    assert_eq!(w.anims.runs[&o].state, 0);
    w.anims.graphs = true;
    anim::furniture_event(&store, 1 << 20, &mut NoGraphs, o, "Go", w);
    assert_eq!(w.anims.runs[&o].state, 1);
    assert!(w.anims.used.contains(&rules::FURNITURE_GRAPH_EVENTS));
    // A reference whose graph cannot be read is refused once and counted.
    let other = w.object(
        "ref",
        Some("test.esm:00000802".into()),
        &["objectreference"],
    );
    anim::furniture_event(&store, 1 << 20, &mut NoGraphs, other, "Go", w);
    anim::furniture_event(&store, 1 << 20, &mut NoGraphs, other, "Go", w);
    assert_eq!(w.anims.counts["graphs refused"], 1);
    assert_eq!(
        w.anims.counts["furniture graph events not sent\tthe reference's graph was refused"],
        1
    );
}
