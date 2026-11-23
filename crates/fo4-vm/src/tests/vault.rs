//! The vault step's readings over synthetic data (no game data): the sit
//! state from the run's sitters, the caller's answer of the player's
//! combat state, a graph's start sequence settled when first read, a
//! state's sequence found ignoring case, and texts equal ignoring case.
use super::assemble;
use super::quest::{library, record, sub, Owned};
use crate::{anim, menu, quest::QuestHost, rules, sit, NativeCall, Reply, Val, Vm};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_havok::{Machine, MachineState, Transition};
use fo4_plugin::typed::Store;
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

fn native(script: &str, name: &str, receiver: Option<usize>, ret: &str) -> NativeCall {
    NativeCall {
        script: script.into(),
        name: name.into(),
        receiver: receiver.map(Val::Object),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: ret.into(),
    }
}

#[test]
fn the_sit_state_answers_from_the_runs_sitters_only_with_its_switch() {
    let s = store();
    let mut l = library();
    let mut host = QuestHost::new(&s, 1 << 20);
    let mut vm = Vm::new(&mut l, &mut host);
    let player = vm.world.player("actor");
    let chair = vm.world.object(
        "ref",
        Some("test.esm:00000802".into()),
        &["objectreference"],
    );
    let ask = native("actor", "getsitstate", Some(player), "int");
    // Off: recorded as before.
    assert!(sit::native(&s, 1 << 20, &ask, &mut vm.world).is_none());
    vm.world.sits.sit_state = true;
    assert!(matches!(
        sit::native(&s, 1 << 20, &ask, &mut vm.world),
        Some(Reply::Value(Val::Int(0)))
    ));
    vm.world.sits.sitting.insert(player, chair);
    assert!(matches!(
        sit::native(&s, 1 << 20, &ask, &mut vm.world),
        Some(Reply::Value(Val::Int(sit::SIT_STATE_SITTING)))
    ));
    assert!(vm.world.sits.used.contains(&rules::SIT_STATE));
    // A receiver that is neither the player nor a placed actor is left.
    let other = native("actor", "getsitstate", Some(chair), "int");
    assert!(sit::native(&s, 1 << 20, &other, &mut vm.world).is_none());
}

#[test]
fn the_players_combat_state_is_only_the_callers_assumption() {
    let s = store();
    let mut l = library();
    let mut host = QuestHost::new(&s, 1 << 20);
    let mut vm = Vm::new(&mut l, &mut host);
    let player = vm.world.player("actor");
    let other = vm
        .world
        .object("actor", Some("test.esm:00000803".into()), &["actor"]);
    let ask = native("actor", "isincombat", Some(player), "bool");
    assert!(menu::menu_mode(&ask, &mut vm.world).is_none());
    vm.world.menus.assume_player_combat = Some(false);
    assert!(matches!(
        menu::menu_mode(&ask, &mut vm.world),
        Some(Reply::Value(Val::Bool(false)))
    ));
    // Another actor's combat state is not the caller's to answer.
    let theirs = native("actor", "isincombat", Some(other), "bool");
    assert!(menu::menu_mode(&theirs, &mut vm.world).is_none());
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

/// A console-like graph: `One` (start, sequence `seqone` played once, key
/// `finish`) goes to `OneLoop` on `finish`; `OneLoop` (a looping sequence) goes
/// to `Two` on `two`; `Two` plays `SeqTwo` (key `Ping` at 0.5 s). The
/// states name their sequences in lower case, the model in mixed case.
fn graph() -> Rc<ModelGraph> {
    let state = |id, name: &str, seq: &str, transitions| MachineState {
        id,
        name: name.into(),
        generator: fo4_havok::SEQUENCE_GENERATOR.into(),
        sequence: Some(seq.into()),
        transitions,
        notify: false,
    };
    let seq = |name: &str, cycle: u32, keys: Vec<(f32, &str)>| Sequence {
        name: name.into(),
        cycle,
        frequency: 1.0,
        start: 0.0,
        stop: 1.0,
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
                state(0, "One", "seqone", vec![transition(0, 1)]),
                state(1, "OneLoop", "seqoneloop", vec![transition(1, 2)]),
                state(2, "Two", "seqtwo", Vec::new()),
            ],
            wildcards: Vec::new(),
            events: vec!["finish".into(), "two".into(), "Ping".into()],
        },
        sequences: vec![
            seq("SeqOne", 2, vec![(0.0, "begin"), (0.03, "finish")]),
            seq("SeqOneLoop", 0, vec![(0.0, "begin"), (1.0, "finish")]),
            seq("SeqTwo", 2, vec![(0.5, "Ping")]),
        ],
    })
}

#[test]
fn a_graphs_start_sequence_is_settled_only_with_its_switch_and_names_found_ignoring_case() {
    let s = store();
    let mut l = library();
    let mut host = QuestHost::new(&s, 1 << 20);
    let mut vm = Vm::new(&mut l, &mut host);
    let w = &mut vm.world;
    w.anims.graphs = true;
    w.anims.keys = true;
    let a = w.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    let b = w.object(
        "ref",
        Some("test.esm:00000802".into()),
        &["objectreference"],
    );
    // Off: the graph waits in its start state, which does not take `two`.
    anim::begin(w, a, graph(), 0.0);
    anim::graph_event(w, a, "two", 0.0);
    assert_eq!(w.anims.runs[&a].state, 0);
    // On: the start state's key `finish` (exact name) is taken at once, the
    // sequence found ignoring case; the loop's sequence plays on, so the
    // graph stays in the loop state, from which `two` is taken.
    w.anims.start_settled = true;
    anim::begin(w, b, graph(), 0.0);
    assert_eq!(
        w.anims.runs[&b].state, 0,
        "exact sequence names only: no sequence"
    );
    w.anims.sequence_case = true;
    anim::begin(w, b, graph(), 0.0);
    assert_eq!(w.anims.runs[&b].state, 1);
    assert!(w.anims.used.contains(&rules::ANIM_GRAPH_START_SETTLED));
    assert!(w.anims.used.contains(&rules::ANIM_SEQUENCE_CASE));
    // Nothing was scheduled by the settling.
    assert!(w.anims.due.is_empty());
    anim::graph_event(w, b, "two", 1.0);
    assert_eq!(w.anims.runs[&b].state, 2);
    // The entered state's sequence is found ignoring case: its key is due.
    assert_eq!(anim::next_due(w), Some(1.5));
}

#[test]
fn texts_differing_only_in_case_are_equal_only_with_the_switch() {
    let mut l = library();
    l.add(assemble(
        "script Texts extends ObjectReference
         var Bool same false
         func Check none String a
         15 same a \"Hello\"",
    ));
    let s = store();
    let mut host = QuestHost::new(&s, 1 << 20);
    let mut vm = Vm::new(&mut l, &mut host);
    let o = vm.world.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    let i = vm.instantiate(Some(o), "texts").unwrap();
    let get = |vm: &Vm<'_>| {
        vm.world.instances[i]
            .vars
            .iter()
            .find(|v| v.name == "same")
            .map(|v| v.value.clone())
    };
    vm.deliver(i, "check", vec![Val::Str("hello".into())]);
    assert!(matches!(get(&vm), Some(Val::Unknown(_))));
    vm.texts_ignore_case = true;
    vm.deliver(i, "check", vec![Val::Str("hello".into())]);
    assert_eq!(get(&vm), Some(Val::Bool(true)));
    assert!(vm.rules.contains(rules::TEXT_EQUAL_IGNORE_CASE));
    // Texts that differ otherwise stay unequal.
    vm.deliver(i, "check", vec![Val::Str("hellos".into())]);
    assert_eq!(get(&vm), Some(Val::Bool(false)));
}

#[test]
fn every_rule_of_the_vault_step_is_registered_pending() {
    for r in [
        rules::SIT_STATE,
        rules::ANIM_GRAPH_START_SETTLED,
        rules::ANIM_SEQUENCE_CASE,
        rules::TEXT_EQUAL_IGNORE_CASE,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
