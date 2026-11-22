//! Scenes: the scene natives, phase progression and scenes that start with
//! their quest, over a synthetic plugin (no game data).
use super::assemble;
use super::quest::{group, key, library, record, sub, text, Owned};
use crate::{
    quest::{self, QuestHost},
    rules, scene, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;

const QS: u32 = 0x900;
const SC: u32 = 0x901;
const SD: u32 = 0x902;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn frag(v: &mut Vec<u8>, script: &str, f: &str) {
    v.push(1);
    text(v, script);
    text(v, f);
}
/// A phase with no condition.
fn phase() -> Vec<u8> {
    [
        sub(b"HNAM", &[]),
        sub(b"NAM0", b"\0"),
        sub(b"NEXT", &[]),
        sub(b"NEXT", &[]),
        sub(b"WNAM", &w(500)),
        sub(b"HNAM", &[]),
    ]
    .concat()
}
/// A dialogue action (kind 0) in phase 1.
fn action() -> Vec<u8> {
    [
        sub(b"ANAM", &[0, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &w(0)),
        sub(b"INAM", &w(1)),
        sub(b"SNAM", &w(1)),
        sub(b"ENAM", &w(1)),
        sub(b"DATA", &w(0)),
        sub(b"ANAM", &[]),
    ]
    .concat()
}
/// SC: FNAM bit 0x1, begin fragment, phase 0 begin and end fragments,
/// two phases, a dialogue action in phase 1. SD: no flag, one phase, no
/// action, an end fragment.
fn store() -> Store {
    let mut vsc = vec![6, 0, 2, 0, 0, 0, 3, 1];
    text(&mut vsc, "SFS");
    vsc.extend([0, 0, 0]);
    frag(&mut vsc, "SFS", "Fragment_Begin");
    vsc.extend(2u16.to_le_bytes());
    for (f, name) in [
        (1u8, "Fragment_Phase_01_Begin"),
        (2, "Fragment_Phase_01_End"),
    ] {
        vsc.push(f);
        vsc.extend(0u32.to_le_bytes());
        frag(&mut vsc, "SFS", name);
    }
    let sc = [
        sub(b"EDID", b"SC\0"),
        sub(b"VMAD", &vsc),
        sub(b"FNAM", &w(0x1)),
        phase(),
        phase(),
        sub(b"ALID", &w(0)),
        sub(b"LNAM", &w(0)),
        sub(b"DNAM", &w(0)),
        action(),
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(2)),
    ]
    .concat();
    let mut vsd = vec![6, 0, 2, 0, 0, 0, 3, 2];
    text(&mut vsd, "SFD");
    vsd.extend([0, 0, 0]);
    frag(&mut vsd, "SFD", "Fragment_End");
    vsd.extend(0u16.to_le_bytes());
    let sd = [
        sub(b"EDID", b"SD\0"),
        sub(b"VMAD", &vsd),
        sub(b"FNAM", &w(0)),
        phase(),
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(0)),
    ]
    .concat();
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let qs = [sub(b"EDID", b"QS\0"), sub(b"DNAM", &d)].concat();
    let children = [record(b"SCEN", SC, &sc), record(b"SCEN", SD, &sd)].concat();
    let body = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &children),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(3u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(0, *b"QUST", &body));
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
        "script Scene extends Form
         func Start none native
         func Stop none native
         func IsPlaying Bool native",
    ));
    l.add(assemble(
        "script SFS extends Scene
         var Int seen 0
         func Fragment_Begin none
         13 seen 1
         func Fragment_Phase_01_Begin none
         13 seen 2
         func Fragment_Phase_01_End none
         13 seen 3",
    ));
    l.add(assemble(
        "script SFD extends Scene
         var Bool ended false
         func Fragment_End none
         13 ended true",
    ));
    l
}

fn var(vm: &Vm<'_>, o: usize, name: &str) -> Val {
    let i = vm.world.scenes.fragment_instances[&o];
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name == name)
        .unwrap()
        .value
        .clone()
}

fn call(vm: &mut Vm<'_>, o: usize, name: &str, ret: &str) -> crate::Reply {
    let c = crate::NativeCall {
        script: "scene".into(),
        name: name.into(),
        receiver: Some(Val::Object(o)),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: ret.into(),
    };
    vm.host.call(&c, &mut vm.world)
}

#[test]
fn a_scene_starts_with_its_quest_and_stops_where_an_action_runs() {
    let store = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut attach = crate::bind::RecordScripts::new(&store, 1 << 24);
    let mut runner = quest::Runner::new(&store, 1 << 24, 64);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.nested = Some(&mut runner);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    quest::start_new_game(&mut vm, &store, &b).unwrap();
    quest::run_jobs(&mut vm, &store, 64, &b);
    let sc = crate::data::record_object(&mut vm.world, &store, &key(SC), "scene", &b).unwrap();
    // Begin, phase 0 begin and end ran in order; phase 1 waits on its
    // dialogue action.
    assert_eq!(var(&vm, sc, "seen"), Val::Int(3));
    let log: Vec<&scene::Change> = vm
        .world
        .scenes
        .log
        .iter()
        .filter(|(o, _)| *o == sc)
        .map(|x| &x.1)
        .collect();
    assert_eq!(log[0], &scene::Change::StartedWithQuest);
    assert!(log.contains(&&scene::Change::PhaseCompleted(0)));
    assert!(matches!(log.last(), Some(scene::Change::Waiting(1, why)) if why.contains("kind 0")));
    assert!(matches!(
        call(&mut vm, sc, "isplaying", "bool"),
        crate::Reply::Value(Val::Bool(true))
    ));
    // SD has no flag: not started with the quest; a Start runs its one
    // phase and its end fragment, after which it no longer plays.
    let sd = crate::data::record_object(&mut vm.world, &store, &key(SD), "scene", &b).unwrap();
    assert!(matches!(
        call(&mut vm, sd, "isplaying", "bool"),
        crate::Reply::Value(Val::Bool(false))
    ));
    call(&mut vm, sd, "start", "none");
    quest::run_jobs(&mut vm, &store, 64, &b);
    assert_eq!(var(&vm, sd, "ended"), Val::Bool(true));
    assert!(matches!(
        call(&mut vm, sd, "isplaying", "bool"),
        crate::Reply::Value(Val::Bool(false))
    ));
    // Stop ends the playing one.
    call(&mut vm, sc, "stop", "none");
    assert!(matches!(
        call(&mut vm, sc, "isplaying", "bool"),
        crate::Reply::Value(Val::Bool(false))
    ));
    for r in [
        rules::SCENE_START,
        rules::SCENE_PHASES,
        rules::SCENE_BEGIN_ON_QUEST_START,
        rules::FRAGMENT_INSTANCE,
    ] {
        assert!(vm.rules.contains(r), "{r}");
    }
}

#[test]
fn scenes_off_change_nothing_and_isolated_scenes_stay_open() {
    let store = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    quest::start_new_game(&mut vm, &store, &b).unwrap();
    quest::run_jobs(&mut vm, &store, 64, &b);
    assert!(vm.world.scenes.log.is_empty());
    // Isolated scenario, scenes on: whether a scene plays is unknown, and
    // a Start leaves it so.
    let mut l = lib();
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.scenes.on = true;
    let sd = crate::data::record_object(&mut vm.world, &store, &key(SD), "scene", &b).unwrap();
    assert!(matches!(
        call(&mut vm, sd, "isplaying", "bool"),
        crate::Reply::Undetermined
    ));
    call(&mut vm, sd, "start", "none");
    assert!(vm.world.quests.queue.is_empty());
    assert!(matches!(vm.world.scenes.log[0].1, scene::Change::Opened(_)));
}
