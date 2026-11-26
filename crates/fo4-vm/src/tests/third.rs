//! The third fight's step over synthetic plugins and scripts (no game data):
//! a reference alias created at run time and one filled from another
//! quest's alias, the aliases holding a created reference and its death,
//! a line's own scripts receiving its begin and end, the sequences the
//! graphs start, and the player's sit ended by the track.
use super::assemble;
use super::quest::{group, key, library, record, sub, text, Owned};
use crate::{
    fight, line, player,
    quest::{self, QuestHost, Scenario},
    rules, NativeCall, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;

const QA: u32 = 0x900;
const QB: u32 = 0x910;
const NPC: u32 = 0x920;
const MARKER_BASE: u32 = 0x921;
const CELL: u32 = 0x930;
const MARKER: u32 = 0x931;
const SEAT_BASE: u32 = 0x932;
const SEAT: u32 = 0x933;
const LINE: u32 = 0x940;
const PLAIN: u32 = 0x941;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn dnam(flags: u16) -> Vec<u8> {
    let mut d = flags.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    sub(b"DNAM", &d)
}

fn alias(id: u32, values: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![
        sub(b"ALST", &w(id)),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &w(0)),
    ];
    v.extend(values.iter().cloned());
    v.push(sub(b"ALED", &[]));
    v.concat()
}

/// QA (starts with the game): alias 0 created at run time (an actor base,
/// at alias 1), alias 1 a forced marker. QB: aliases 0 and 1 filled from
/// QA's 0 and 1. A cell with the marker and a seat; a line with a script
/// of its own and one without.
fn store() -> Store {
    let mut alca = 1u16.to_le_bytes().to_vec();
    alca.extend(0u16.to_le_bytes());
    let qa = [
        sub(b"EDID", b"QA\0"),
        dnam(0x0001),
        sub(b"ANAM", &w(2)),
        alias(
            0,
            &[
                sub(b"ALCO", &w(NPC)),
                sub(b"ALCA", &alca),
                sub(b"ALCL", &w(1)),
            ],
        ),
        alias(1, &[sub(b"ALFR", &w(MARKER))]),
    ]
    .concat();
    let qb = [
        sub(b"EDID", b"QB\0"),
        dnam(0),
        sub(b"ANAM", &w(2)),
        alias(0, &[sub(b"ALEQ", &w(QA)), sub(b"ALEA", &w(0))]),
        alias(1, &[sub(b"ALEQ", &w(QA)), sub(b"ALEA", &w(1))]),
    ]
    .concat();
    let mut line_vmad = vec![6, 0, 2, 0];
    line_vmad.extend(1u16.to_le_bytes());
    text(&mut line_vmad, "LineWatch");
    line_vmad.push(0);
    line_vmad.extend(0u16.to_le_bytes());
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
        *b"NPC_",
        &record(b"NPC_", NPC, &sub(b"EDID", b"Raider\0")),
    ));
    plugin.extend(group(
        0,
        *b"STAT",
        &record(b"STAT", MARKER_BASE, &sub(b"EDID", b"Mark\0")),
    ));
    plugin.extend(group(
        0,
        *b"FURN",
        &record(b"FURN", SEAT_BASE, &sub(b"EDID", b"Seat\0")),
    ));
    plugin.extend(group(
        0,
        *b"QUST",
        &[record(b"QUST", QA, &qa), record(b"QUST", QB, &qb)].concat(),
    ));
    plugin.extend(group(
        0,
        *b"INFO",
        &[
            record(b"INFO", LINE, &sub(b"VMAD", &line_vmad)),
            record(b"INFO", PLAIN, &sub(b"EDID", b"Plain\0")),
        ]
        .concat(),
    ));
    let data = |x: f32| {
        let mut d = Vec::new();
        for v in [x, 0.0, 0.0, 0.0, 0.0, 0.0] {
            d.extend(v.to_le_bytes());
        }
        d
    };
    let refs = [
        record(
            b"REFR",
            MARKER,
            &[sub(b"NAME", &w(MARKER_BASE)), sub(b"DATA", &data(0.0))].concat(),
        ),
        record(
            b"REFR",
            SEAT,
            &[sub(b"NAME", &w(SEAT_BASE)), sub(b"DATA", &data(10.0))].concat(),
        ),
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
    let mut l = library();
    l.add(assemble(
        "script Actor extends ObjectReference
         func IsDead Bool native",
    ));
    l.add(assemble("script TopicInfo extends Form"));
    // Counts the begins and ends it receives, with the speaker and whether
    // the line had been said.
    l.add(assemble(
        "script LineWatch extends TopicInfo
         var Int begins 0
         var Int ends 0
         var ObjectReference who
         var Bool said false
         func OnBegin none ObjectReference akSpeakerRef Bool abHasBeenSaid
         1 begins begins 1
         13 who akSpeakerRef
         13 said abHasBeenSaid
         func OnEnd none ObjectReference akSpeakerRef Bool abHasBeenSaid
         1 ends ends 1",
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

#[test]
fn created_and_borrowed_aliases_hold_their_references_only_under_their_rules() {
    let s = store();
    let b = Budget::new(1 << 24);
    let max = 1 << 24;
    let a = quest::QuestInfo::load(&s, &key(QA), &b).unwrap().unwrap();
    let c = &a.created[&0];
    assert_eq!(
        (c.object.clone(), c.at, c.word, c.level),
        (Some(key(NPC)), 1, 0, 1)
    );
    let bq = quest::QuestInfo::load(&s, &key(QB), &b).unwrap().unwrap();
    assert_eq!(bq.external[&0], (key(QA), 0));
    let mut l = lib();
    let mut host = QuestHost::new(&s, max);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = Scenario::NewGame;
    // Off: neither reading answers; the alias holds nothing.
    assert!(quest::held_more(&s, max, &mut vm.world, &a, 0, 0).is_none());
    assert_eq!(
        quest::alias_value(&s, max, &mut vm.world, &key(QB), 0, 0),
        Ok(None),
        "QB does not run"
    );
    vm.world.quests.state(&bq).running = Some(true);
    assert_eq!(
        quest::alias_value(&s, max, &mut vm.world, &key(QB), 0, 0),
        Ok(None)
    );
    // The created reading: one reference made for QA's alias 0, the same
    // at every read, no record behind it.
    vm.world.quests.created_refs = true;
    let Some(Ok(Some(Val::Object(o)))) = quest::held_more(&s, max, &mut vm.world, &a, 0, 0) else {
        panic!("a created reference")
    };
    assert!(quest::is_created(&vm.world, o));
    assert!(vm.world.objects[o].types.contains(&"actor".to_string()));
    assert_eq!(
        quest::held_more(&s, max, &mut vm.world, &a, 0, 0),
        Some(Ok(Some(Val::Object(o))))
    );
    assert_eq!(vm.world.quests.created_made.len(), 1);
    // Without the borrowed reading QB's alias still holds nothing.
    assert_eq!(
        quest::alias_value(&s, max, &mut vm.world, &key(QB), 0, 0),
        Ok(None)
    );
    vm.world.quests.external_fills = true;
    assert_eq!(
        quest::alias_value(&s, max, &mut vm.world, &key(QB), 0, 0),
        Ok(Some(Val::Object(o)))
    );
    // A borrowed forced reference is the record's handle.
    let Ok(Some(Val::Object(m))) = quest::alias_value(&s, max, &mut vm.world, &key(QB), 1, 0)
    else {
        panic!("the marker")
    };
    assert_eq!(
        vm.world.objects[m].key.as_deref(),
        Some(crate::bind::record_key(&key(MARKER)).as_str())
    );
    // A live link: once QA stops, QB's alias holds nothing.
    vm.world.quests.state(&a).running = Some(false);
    assert_eq!(
        quest::alias_value(&s, max, &mut vm.world, &key(QB), 0, 0),
        Ok(None)
    );
    vm.world.quests.state(&a).running = Some(true);
    // The aliases holding the created reference: QA's own and QB's.
    let (holders, open) = player::holders_of(&mut vm, &s, o);
    assert_eq!(open, 0);
    assert_eq!(holders, vec![(key(QA), 0), (key(QB), 0)]);
    let used = vm.world.quests.fill_used.borrow().clone();
    assert!(used.contains(rules::ALIAS_CREATED_REFERENCE));
    assert!(used.contains(rules::ALIAS_EXTERNAL_LIVE));
}

#[test]
fn a_created_reference_is_alive_until_the_run_kills_it() {
    let s = store();
    let b = Budget::new(1 << 24);
    let max = 1 << 24;
    let a = quest::QuestInfo::load(&s, &key(QA), &b).unwrap().unwrap();
    let mut l = lib();
    let mut host = QuestHost::new(&s, max);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = Scenario::NewGame;
    vm.world.quests.created_refs = true;
    vm.world.fights.death_events = true;
    let Some(Ok(Some(Val::Object(o)))) = quest::held_more(&s, max, &mut vm.world, &a, 0, 0) else {
        panic!("a created reference")
    };
    let is_dead = NativeCall {
        script: "actor".into(),
        name: "isdead".into(),
        receiver: Some(Val::Object(o)),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "bool".into(),
    };
    // Without the dead-state reading IsDead is left to the host.
    assert!(fight::native(&is_dead, &mut vm.world).is_none());
    vm.world.fights.dead_state = true;
    assert!(matches!(
        fight::native(&is_dead, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(false)))
    ));
    // The death reaches the aliases holding it (none with scripts here).
    let p = vm.world.player("actor");
    fight::death(&mut vm, &s, o, p, &b).unwrap();
    assert!(matches!(
        fight::native(&is_dead, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(true)))
    ));
    // Without the created reading the object would be no record to deliver to.
    vm.world.quests.created_refs = false;
    assert!(fight::death(&mut vm, &s, o, p, &b).is_err());
}

#[test]
fn a_lines_own_scripts_receive_its_begin_and_end_only_under_their_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    assert!(line::has_own_scripts(&s, &key(LINE), &b));
    assert!(!line::has_own_scripts(&s, &key(PLAIN), &b));
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let speaker = vm.world.object("speaker", None, &["objectreference"]);
    let owner = vm.world.object("scene", None, &[]);
    line::queue_script(&mut vm.world, owner, &key(LINE), "onbegin", Some(speaker));
    let runs = quest::run_jobs(&mut vm, &s, 16, &b);
    assert_eq!(runs.len(), 1);
    assert!(vm.rules.contains(rules::LINE_SCRIPT_EVENTS));
    let o = vm
        .world
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some(crate::bind::record_key(&key(LINE)).as_str()))
        .unwrap();
    let i = vm.world.objects[o].instances[0];
    assert_eq!(var(&vm, i, "begins"), Val::Int(1));
    assert_eq!(var(&vm, i, "who"), Val::Object(speaker));
    assert_eq!(var(&vm, i, "said"), Val::Bool(false));
    line::queue_script(&mut vm.world, owner, &key(LINE), "onend", Some(speaker));
    quest::run_jobs(&mut vm, &s, 16, &b);
    assert_eq!(var(&vm, i, "ends"), Val::Int(1));
    // Said once: a later begin is told so, on the same instance.
    line::queue_script(&mut vm.world, owner, &key(LINE), "onbegin", Some(speaker));
    quest::run_jobs(&mut vm, &s, 16, &b);
    assert_eq!(vm.world.objects[o].instances.len(), 1);
    assert_eq!(var(&vm, i, "begins"), Val::Int(2));
    assert_eq!(var(&vm, i, "said"), Val::Bool(true));
}

#[test]
fn the_players_sit_ends_when_the_track_walks_away_only_under_its_rule() {
    let s = store();
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let b = Budget::new(1 << 24);
    let p = vm.world.player("actor");
    let seat =
        crate::data::record_object(&mut vm.world, &s, &key(SEAT), "objectreference", &b).unwrap();
    vm.world.sits.sitting.insert(p, seat);
    let track = crate::track::Track::parse(&format!(
        "0 test.esm:{CELL:08x} 10 0 0\n5 test.esm:{CELL:08x} 10 0 0\n6 test.esm:{CELL:08x} 400 0 0\n"
    ))
    .unwrap();
    vm.world.place.track = Some(std::rc::Rc::new(track));
    vm.world.now[0] = 6.0;
    // Off: nothing ends the sit.
    assert!(!crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
    vm.world.sits.end_walk = Some(100.0);
    // Close by at 5 s, away at 6 s.
    vm.world.now[0] = 5.0;
    assert!(!crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
    vm.world.now[0] = 6.0;
    assert!(crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
    assert_eq!(vm.world.sits.pending_get_ups, vec![(p, seat)]);
    // Noted once.
    assert!(!crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
}

#[test]
fn only_a_sit_an_activation_with_events_brought_ends_under_the_events_reading() {
    let s = store();
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let b = Budget::new(1 << 24);
    let p = vm.world.player("actor");
    let seat =
        crate::data::record_object(&mut vm.world, &s, &key(SEAT), "objectreference", &b).unwrap();
    vm.world.sits.sitting.insert(p, seat);
    let track = crate::track::Track::parse(&format!(
        "0 test.esm:{CELL:08x} 10 0 0\n6 test.esm:{CELL:08x} 400 0 0\n"
    ))
    .unwrap();
    vm.world.place.track = Some(std::rc::Rc::new(track));
    vm.world.now[0] = 6.0;
    vm.world.sits.end_walk_events = Some(100.0);
    // A sit no activation with events brought (the caller's, a default
    // activation's) is left as before.
    assert!(!crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
    // One an activation with events brought ends once the track is away.
    vm.world.sits.from_events.push((p, seat));
    assert!(crate::sit::end_by_track(&s, 1 << 24, &mut vm.world));
    assert_eq!(vm.world.sits.pending_get_ups, vec![(p, seat)]);
    assert!(vm
        .world
        .sits
        .used
        .contains(&rules::SIT_EVENTS_ENDED_BY_TRACK));
    assert!(!vm.world.sits.used.contains(&rules::SIT_ENDED_BY_TRACK));
    // The get-up forgets where the sit came from.
    crate::sit::settle(&mut vm, &s, 16, &b);
    assert!(vm.world.sits.from_events.is_empty());
    assert!(!vm.world.sits.sitting.contains_key(&p));
}

/// `--sit-end-other-activation`: a sit an activation with events brought
/// ends before an activation of another reference, not of the furniture
/// itself; other sits are left; off, nothing ends.
#[test]
fn a_sit_from_events_ends_before_another_reference_is_activated() {
    let s = store();
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let b = Budget::new(1 << 24);
    let p = vm.world.player("actor");
    let seat =
        crate::data::record_object(&mut vm.world, &s, &key(SEAT), "objectreference", &b).unwrap();
    vm.world.sits.sitting.insert(p, seat);
    let other = "test.esm:00ABCDEF";
    let seat_key = format!("test.esm:{SEAT:08x}");
    // Off: nothing ends.
    assert!(!crate::sit::end_for_activation(&mut vm.world, other));
    vm.world.sits.end_other_activation = true;
    // A sit no activation with events brought is left as before.
    assert!(!crate::sit::end_for_activation(&mut vm.world, other));
    vm.world.sits.from_events.push((p, seat));
    // The furniture's own activation does not end it.
    assert!(!crate::sit::end_for_activation(&mut vm.world, &seat_key));
    assert!(crate::sit::end_for_activation(&mut vm.world, other));
    assert_eq!(vm.world.sits.pending_get_ups, vec![(p, seat)]);
    assert!(vm
        .world
        .sits
        .used
        .contains(&rules::SIT_EVENTS_ENDED_BY_OTHER_ACTIVATION));
    // Noted once.
    assert!(!crate::sit::end_for_activation(&mut vm.world, other));
    crate::sit::settle(&mut vm, &s, 16, &b);
    assert!(!vm.world.sits.sitting.contains_key(&p));
    assert_eq!(
        rules::status(rules::SIT_EVENTS_ENDED_BY_OTHER_ACTIVATION),
        Some("pending")
    );
}

#[test]
fn every_rule_of_the_third_fight_step_is_registered_pending() {
    for r in [
        rules::ALIAS_CREATED_REFERENCE,
        rules::ALIAS_EXTERNAL_LIVE,
        rules::LINE_SCRIPT_EVENTS,
        rules::SIT_ENDED_BY_TRACK,
        rules::SIT_EVENTS_ENDED_BY_TRACK,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn the_timeline_writes_the_sequences_the_graphs_start_only_with_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let m =
        crate::data::record_object(&mut vm.world, &s, &key(MARKER), "objectreference", &b).unwrap();
    let opaque = vm.world.object("opaque", None, &[]);
    vm.world.anims.started.push((2.5, m, "Descend".into()));
    vm.world.anims.started.push((3.0, opaque, "Idle".into()));
    let notes = |sequences: bool| {
        let mut rec = crate::timeline::Recorder::new();
        rec.sequences = sequences;
        rec.observe(&vm);
        rec.finish(&vm, &s, None, &b)
            .events
            .into_iter()
            .filter_map(|e| match e.what {
                fo4_scene::timeline::What::Note(n) => Some((e.t, n)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(notes(false).is_empty());
    assert_eq!(
        notes(true),
        vec![
            (2.5, format!("sequence test.esm:{MARKER:08x} Descend")),
            (3.0, "sequence - Idle".to_string()),
        ]
    );
}
