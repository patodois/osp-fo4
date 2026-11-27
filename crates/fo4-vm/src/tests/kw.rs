//! The keyword step's readings over synthetic plugins (no game data):
//! keywords of references from the records and from the run's changes,
//! collection aliases filled from a location's rows, the caller's answer
//! of animation event registrations, and scene actions whose start phase
//! was skipped.
use super::quest::{group, key, record, sub, Owned};
use crate::{
    anim, keyword,
    quest::{self, QuestHost},
    rules,
    scene::{self, Change},
    NativeCall, Reply, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;

const QS: u32 = 0x900;
const SC: u32 = 0x901;
const CELL: u32 = 0x930;
const LOC: u32 = 0x940;
const LT: u32 = 0x941;
const STAT: u32 = 0x951;
const RACE: u32 = 0x952;
const NPC: u32 = 0x953;
const TNPC: u32 = 0x954;
const K1: u32 = 0x970;
const K2: u32 = 0x971;
const K3: u32 = 0x972;
const A1: u32 = 0x980;
const AT: u32 = 0x981;
const R1: u32 = 0x982;
const R2: u32 = 0x983;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn kwda(ks: &[u32]) -> Vec<u8> {
    [
        sub(b"KSIZ", &w(ks.len() as u32)),
        sub(b"KWDA", &ks.iter().flat_map(|k| w(*k)).collect::<Vec<u8>>()),
    ]
    .concat()
}
fn ctda59(stage: u32) -> Vec<u8> {
    let mut d = vec![0u8; 32];
    d[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    d[8..10].copy_from_slice(&59u16.to_le_bytes());
    d[12..16].copy_from_slice(&w(QS));
    d[16..20].copy_from_slice(&w(stage));
    d[28..32].copy_from_slice(&(-1i32).to_le_bytes());
    d
}

/// QS starts with the game and has a collection alias (0) filled from a
/// forced location (alias 1) by a ref type whose rows name R1, R2 and R1
/// again. SC starts with QS: phase 0 plain, phase 1 with a start condition
/// that is false (stage 10 done), phase 2 plain; one package action spans
/// phases 1 and 2. A race with K2; an actor base with K1 of that race; a
/// templated actor base with K1; one placed actor of each.
fn store() -> Store {
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"DNAM", &d),
        sub(b"ANAM", &w(2)),
        sub(b"ALCS", &w(0)),
        sub(b"ALMI", &[0]),
        sub(b"ALST", &w(0)),
        sub(b"ALID", b"C\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFA", &w(1)),
        sub(b"ALRT", &w(LT)),
        sub(b"ALED", &[]),
        sub(b"ALLS", &w(1)),
        sub(b"ALID", b"L\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFL", &w(LOC)),
        sub(b"ALED", &[]),
    ]
    .concat();
    let phase = |start: Option<u32>| {
        let mut v = [sub(b"HNAM", &[]), sub(b"NAM0", b"\0")].concat();
        if let Some(st) = start {
            v.extend(sub(b"CTDA", &ctda59(st)));
        }
        v.extend(
            [
                sub(b"NEXT", &[]),
                sub(b"NEXT", &[]),
                sub(b"WNAM", &w(500)),
                sub(b"HNAM", &[]),
            ]
            .concat(),
        );
        v
    };
    let package = [
        sub(b"ANAM", &[1, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &w(0)),
        sub(b"INAM", &w(1)),
        sub(b"SNAM", &w(1)),
        sub(b"ENAM", &w(2)),
        sub(b"PNAM", &w(0)),
        sub(b"ANAM", &[]),
    ]
    .concat();
    let sc = [
        sub(b"EDID", b"SC\0"),
        sub(b"FNAM", &w(0x1)),
        phase(None),
        phase(Some(10)),
        phase(None),
        sub(b"ALID", &w(0)),
        sub(b"LNAM", &w(0)),
        sub(b"DNAM", &w(0)),
        package,
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(2)),
    ]
    .concat();
    let quests = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &record(b"SCEN", SC, &sc)),
    ]
    .concat();
    let refs = [
        record(b"REFR", R1, &sub(b"NAME", &w(STAT))),
        record(b"REFR", R2, &sub(b"NAME", &w(STAT))),
        record(b"ACHR", A1, &sub(b"NAME", &w(NPC))),
        record(b"ACHR", AT, &sub(b"NAME", &w(TNPC))),
    ]
    .concat();
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    let cells = group(
        2,
        [0; 4],
        &[
            cell,
            group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs)),
        ]
        .concat(),
    );
    let row = |r: u32| [w(LT), w(r), w(CELL), w(0)].concat();
    let lcsr = [row(R1), row(R2), row(R1)].concat();
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
        *b"KYWD",
        &[
            record(b"KYWD", K1, &sub(b"EDID", b"K1\0")),
            record(b"KYWD", K2, &sub(b"EDID", b"K2\0")),
            record(b"KYWD", K3, &sub(b"EDID", b"K3\0")),
        ]
        .concat(),
    ));
    plugin.extend(group(
        0,
        *b"LCRT",
        &record(b"LCRT", LT, &sub(b"EDID", b"LT\0")),
    ));
    plugin.extend(group(
        0,
        *b"RACE",
        &record(b"RACE", RACE, &[sub(b"EDID", b"R\0"), kwda(&[K2])].concat()),
    ));
    plugin.extend(group(
        0,
        *b"LCTN",
        &record(
            b"LCTN",
            LOC,
            &[sub(b"EDID", b"Loc\0"), sub(b"LCSR", &lcsr)].concat(),
        ),
    ));
    plugin.extend(group(
        0,
        *b"STAT",
        &record(b"STAT", STAT, &sub(b"EDID", b"S\0")),
    ));
    plugin.extend(group(
        0,
        *b"NPC_",
        &[
            record(
                b"NPC_",
                NPC,
                &[sub(b"EDID", b"N\0"), sub(b"RNAM", &w(RACE)), kwda(&[K1])].concat(),
            ),
            record(
                b"NPC_",
                TNPC,
                &[
                    sub(b"EDID", b"T\0"),
                    sub(b"TPLT", &w(NPC)),
                    sub(b"RNAM", &w(RACE)),
                    kwda(&[K1]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    ));
    plugin.extend(group(0, *b"QUST", &quests));
    plugin.extend(group(0, *b"CELL", &cells));
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

fn call(script: &str, name: &str, receiver: usize, args: Vec<Val>) -> NativeCall {
    NativeCall {
        script: script.into(),
        name: name.into(),
        receiver: Some(Val::Object(receiver)),
        instance: None,
        args,
        param_types: Vec::new(),
        return_type: "none".into(),
    }
}

fn answer(r: Option<Reply>) -> Option<bool> {
    match r {
        Some(Reply::Value(Val::Bool(v))) => Some(v),
        _ => None,
    }
}

#[test]
fn keywords_come_from_the_records_and_the_runs_changes_only_under_their_rules() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = super::quest::library();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let obj = |vm: &mut Vm<'_>, k: u32, t: &str| {
        crate::data::record_object(&mut vm.world, &s, &key(k), t, &b).unwrap()
    };
    let (a1, at) = (obj(&mut vm, A1, "actor"), obj(&mut vm, AT, "actor"));
    let [k1, k2, k3] = [K1, K2, K3].map(|k| Val::Object(obj(&mut vm, k, "keyword")));
    let has = |vm: &mut Vm<'_>, o: usize, k: &Val| {
        answer(keyword::native(
            &s,
            1 << 24,
            &call("objectreference", "haskeyword", o, vec![k.clone()]),
            &mut vm.world,
        ))
    };
    let change = |vm: &mut Vm<'_>, name: &str, o: usize, k: &Val| {
        keyword::native(
            &s,
            1 << 24,
            &call("objectreference", name, o, vec![k.clone()]),
            &mut vm.world,
        )
    };
    // Off: nothing is answered or kept.
    assert_eq!(has(&mut vm, a1, &k1), None);
    assert!(change(&mut vm, "addkeyword", a1, &k3).is_none());
    assert!(vm.world.keywords.changes.is_empty());
    // The records: the base's and its race's; a templated base leaves an
    // absent keyword open.
    vm.world.keywords.records = true;
    assert_eq!(has(&mut vm, a1, &k1), Some(true));
    assert_eq!(has(&mut vm, a1, &k2), Some(true));
    assert_eq!(has(&mut vm, a1, &k3), Some(false));
    assert_eq!(has(&mut vm, at, &k1), Some(true));
    assert_eq!(has(&mut vm, at, &k3), None);
    assert!(vm.world.keywords.used.contains(&rules::KEYWORD_RECORDS));
    // A change the run made: open with the records alone, answered with
    // the run-time reading; the call itself answers as before.
    assert!(change(&mut vm, "removekeyword", a1, &k1).is_none());
    assert!(change(&mut vm, "addkeyword", a1, &k3).is_none());
    assert_eq!(has(&mut vm, a1, &k1), None);
    vm.world.keywords.runtime = true;
    assert_eq!(has(&mut vm, a1, &k1), Some(false));
    assert_eq!(has(&mut vm, a1, &k3), Some(true));
    assert_eq!(has(&mut vm, a1, &k2), Some(true));
    assert!(vm.world.keywords.used.contains(&rules::KEYWORD_RUNTIME));
    // The run-time reading alone answers only what the run changed.
    vm.world.keywords.records = false;
    assert_eq!(has(&mut vm, a1, &k2), None);
    assert_eq!(has(&mut vm, a1, &k3), Some(true));
    // A change with an undetermined keyword may be any of them: the
    // earlier change no longer answers; a later determined one does.
    let open = vm
        .world
        .unknown(crate::world::Origin::Native("test.keyword".into()));
    assert!(change(&mut vm, "removekeyword", a1, &open).is_none());
    assert_eq!(has(&mut vm, a1, &k3), None);
    assert!(change(&mut vm, "addkeyword", a1, &k3).is_none());
    assert_eq!(has(&mut vm, a1, &k3), Some(true));
}

#[test]
fn a_collection_alias_holds_its_locations_distinct_rows_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = super::quest::library();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = quest::Scenario::NewGame;
    let i = quest::QuestInfo::load(&s, &key(QS), &b).unwrap().unwrap();
    assert!(i.collections.contains(&0));
    let alias = vm.world.keyed_object(
        "alias",
        &format!("{}#alias:0", crate::bind::record_key(&key(QS))),
        "refcollectionalias",
    );
    let r1 =
        crate::data::record_object(&mut vm.world, &s, &key(R1), "objectreference", &b).unwrap();
    let r2 =
        crate::data::record_object(&mut vm.world, &s, &key(R2), "objectreference", &b).unwrap();
    let c = |vm: &mut Vm<'_>, name: &str, args: Vec<Val>| {
        let mut x = call("refcollectionalias", name, alias, args);
        x.return_type = "int".into();
        vm.host.call(&x, &mut vm.world)
    };
    // Off: recorded as before (undetermined).
    assert!(matches!(
        c(&mut vm, "getcount", vec![]),
        Reply::Undetermined
    ));
    vm.world.quests.collection_fills = true;
    assert!(matches!(
        c(&mut vm, "getcount", vec![]),
        Reply::Value(Val::Int(2))
    ));
    assert!(
        matches!(c(&mut vm, "getat", vec![Val::Int(1)]), Reply::Value(Val::Object(o)) if o == r2)
    );
    assert!(matches!(
        c(&mut vm, "find", vec![Val::Object(r1)]),
        Reply::Value(Val::Int(0))
    ));
    c(&mut vm, "removeref", vec![Val::Object(r1)]);
    assert!(matches!(
        c(&mut vm, "getcount", vec![]),
        Reply::Value(Val::Int(1))
    ));
    assert!(matches!(
        c(&mut vm, "find", vec![Val::Object(r1)]),
        Reply::Value(Val::Int(-1))
    ));
    c(&mut vm, "removeall", vec![]);
    assert!(matches!(
        c(&mut vm, "getcount", vec![]),
        Reply::Value(Val::Int(0))
    ));
    assert!(vm.host.rules().contains(&rules::COLLECTION_LOCATION));
}

#[test]
fn a_collection_read_before_its_quest_runs_fills_once_it_runs() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = super::quest::library();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = quest::Scenario::NewGame;
    vm.world.quests.collection_fills = true;
    let i = quest::QuestInfo::load(&s, &key(QS), &b).unwrap().unwrap();
    let alias = vm.world.keyed_object(
        "alias",
        &format!("{}#alias:0", crate::bind::record_key(&key(QS))),
        "refcollectionalias",
    );
    let count = |vm: &mut Vm<'_>| {
        let mut x = call("refcollectionalias", "getcount", alias, vec![]);
        x.return_type = "int".into();
        vm.host.call(&x, &mut vm.world)
    };
    vm.world.quests.state(&i).running = Some(false);
    assert!(matches!(count(&mut vm), Reply::Value(Val::Int(0))));
    // The quest starts: the empty answer was not kept.
    vm.world.quests.state(&i).running = Some(true);
    assert!(matches!(count(&mut vm), Reply::Value(Val::Int(2))));
}

#[test]
fn the_callers_registration_answer_applies_only_to_made_registrations() {
    let s = store();
    let mut l = super::quest::library();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let o = vm.world.object(
        "ref",
        Some("test.esm:00000982".into()),
        &["objectreference"],
    );
    let mut reg = call(
        "scriptobject",
        "registerforanimationevent",
        o,
        vec![Val::Object(o), Val::Str("Talk01".into())],
    );
    vm.world.anims.on = true;
    reg.instance = Some(0);
    assert!(matches!(
        anim::native(&reg, &mut vm.world),
        Some(Reply::Undetermined)
    ));
    vm.world.anims.assume_registration = Some(true);
    assert!(matches!(
        anim::native(&reg, &mut vm.world),
        Some(Reply::Value(Val::Bool(true)))
    ));
    // A registration the run could not make stays undetermined.
    reg.args[1] = Val::None;
    assert!(matches!(
        anim::native(&reg, &mut vm.world),
        Some(Reply::Undetermined)
    ));
}

fn scene_log(clock: scene::Clock) -> (Vec<Change>, Vec<&'static str>) {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = super::scene::lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    vm.world.scenes.clock = clock;
    quest::start_new_game(&mut vm, &s, &b).unwrap();
    quest::run_jobs(&mut vm, &s, 64, &b);
    for _ in 0..5 {
        if scene::advance_clock(&mut vm.world, None).is_none() {
            break;
        }
        quest::run_jobs(&mut vm, &s, 64, &b);
    }
    let sc = crate::data::record_object(&mut vm.world, &s, &key(SC), "scene", &b).unwrap();
    let mut used: Vec<&'static str> = vm.rules.iter().copied().collect();
    used.extend(vm.host.rules());
    let log = vm
        .world
        .scenes
        .log
        .iter()
        .filter(|(o, _)| *o == sc)
        .map(|x| x.1.clone())
        .collect();
    (log, used)
}

#[test]
fn an_action_whose_start_phase_was_skipped_starts_later_only_under_its_rule() {
    let clock = scene::Clock {
        on: true,
        assume_packages: true,
        ..scene::Clock::default()
    };
    let (log, _) = scene_log(clock.clone());
    assert!(log.contains(&Change::PhaseSkipped(1)));
    assert!(matches!(log.last(), Some(Change::Waiting(2, why)) if why.contains("kind 1")));
    let (log, used) = scene_log(scene::Clock {
        start_skipped: true,
        ..clock
    });
    assert!(log
        .iter()
        .any(|c| matches!(c, Change::ActionStarted(2, 0, 1, _))));
    assert_eq!(log.last(), Some(&Change::Ended));
    assert!(used.contains(&rules::SCENE_ACTION_SKIPPED_START));
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::KEYWORD_RECORDS,
        rules::KEYWORD_RUNTIME,
        rules::COLLECTION_LOCATION,
        rules::SCENE_ACTION_SKIPPED_START,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
