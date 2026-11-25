//! Dialogue outside scenes (`crate::talk`), unique actor aliases
//! (`crate::quest::UniqueActors`), actor positions and actor trigger events
//! (`crate::track`) and the player's identity, over a synthetic plugin (no
//! game data).
use super::quest::{group, record, sub, text, Owned};
use super::{assemble, base};
use crate::{
    bind::RecordScripts,
    quest::{self, Change, Fill, QuestHost, UniqueActors},
    rules, scene, talk,
    track::{self, ActorMove, Place, Switches, Track},
    Outcome, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::{typed::Store, FormKey};

const WRLD: u32 = 0x800;
const CELL: u32 = 0x801;
const ACTI: u32 = 0x802;
const STAT: u32 = 0x803;
const QS: u32 = 0x900;
const D1: u32 = 0x920;
const L1: u32 = 0x921;
const D2: u32 = 0x922;
const L2: u32 = 0x923;
const GUARD: u32 = 0x930;
const HERO: u32 = 0x931;
const LONE: u32 = 0x932;
const ACTOR: u32 = 0x940;
const TRIG: u32 = 0x950;
const MARK: u32 = 0x951;
const DOOR: u32 = 0x804;
const LEAF: u32 = 0x952;

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    }
}
fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn data(pos: [f32; 3]) -> Vec<u8> {
    let mut d = Vec::new();
    for f in pos.into_iter().chain([0.0; 3]) {
        d.extend(f.to_le_bytes());
    }
    d
}
fn xprm(bounds: [f32; 3], kind: u32) -> Vec<u8> {
    let mut d = Vec::new();
    for f in bounds.into_iter().chain([1.0, 0.0, 1.0, 0.3]) {
        d.extend(f.to_le_bytes());
    }
    d.extend(kind.to_le_bytes());
    d
}
/// A CTDA: function 72 (`==` 1) on a base, run on `run_on`.
fn is_id(base: u32, run_on: u32) -> Vec<u8> {
    let mut c = vec![0u8; 32];
    c[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    c[8..10].copy_from_slice(&72u16.to_le_bytes());
    c[12..16].copy_from_slice(&base.to_le_bytes());
    c[20..24].copy_from_slice(&run_on.to_le_bytes());
    c[28..32].copy_from_slice(&(-1i32).to_le_bytes());
    sub(b"CTDA", &c)
}
fn tiqs(a: i16, b: i16) -> Vec<u8> {
    sub(b"TIQS", &[a.to_le_bytes(), b.to_le_bytes()].concat())
}
fn stage(n: u16) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([0, 0]);
    [sub(b"INDX", &d), sub(b"QSDT", &[0])].concat()
}

/// QS starts with the game (priority 30, stages 10 and 20); its alias 0
/// is filled by the unique actor GUARD, alias 1 by LONE (never placed).
/// Topic D1 (GREE) holds line L1: the player (HERO) speaking to GUARD,
/// TIQS 10 at its begin; topic D2 (HELO) holds line L2: GUARD speaking,
/// TIQS 20 at its end. One exterior cell holds GUARD's placed actor at
/// (1000, 1100, 0), a trigger box (half extents 50, script `Trig`) at
/// (3000, 1000, 0) and a marker at (3000, 1100, 0).
fn store() -> Store {
    store_with(None)
}

/// A CTDA: function 59 (`GetStageDone`) of QS's stage `n`, `==` 1.
fn stage_done(n: u32) -> Vec<u8> {
    let mut c = vec![0u8; 32];
    c[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    c[8..10].copy_from_slice(&59u16.to_le_bytes());
    c[12..16].copy_from_slice(&QS.to_le_bytes());
    c[16..20].copy_from_slice(&n.to_le_bytes());
    c[28..32].copy_from_slice(&(-1i32).to_le_bytes());
    sub(b"CTDA", &c)
}

/// [`store`], with the hello line L2 also asking that QS's stage
/// `hello_after` is done.
fn store_with(hello_after: Option<u32>) -> Store {
    store_full(hello_after, false)
}

/// [`store_with`], with `door` a door base and a placed door between the
/// actor's stored place and the trigger.
fn store_full(hello_after: Option<u32>, door: bool) -> Store {
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let alias = |id: u32, name: &[u8], npc: u32| {
        [
            sub(b"ALST", &w(id)),
            sub(b"ALID", name),
            sub(b"FNAM", &w(0)),
            sub(b"ALUA", &w(npc)),
            sub(b"ALED", &[]),
        ]
        .concat()
    };
    let mut qs = [sub(b"EDID", b"QS\0"), sub(b"DNAM", &d)].concat();
    for n in [10, 20] {
        qs.extend(stage(n));
    }
    qs.extend(sub(b"ANAM", &w(2)));
    qs.extend(alias(0, b"Guard\0", GUARD));
    qs.extend(alias(1, b"Lone\0", LONE));
    let dial = |e: &[u8], code: &[u8; 4]| {
        [
            sub(b"EDID", e),
            sub(b"PNAM", &50.0f32.to_le_bytes()),
            sub(b"QNAM", &w(QS)),
            sub(b"DATA", &[0, 7, 0, 0]),
            sub(b"SNAM", code),
            sub(b"TIFC", &w(1)),
        ]
        .concat()
    };
    let l1 = [
        sub(b"ENAM", &w(0)),
        is_id(HERO, 0),
        is_id(GUARD, 1),
        tiqs(10, -1),
    ]
    .concat();
    let mut l2 = [sub(b"ENAM", &w(0)), is_id(GUARD, 0)].concat();
    if let Some(n) = hello_after {
        l2.extend(stage_done(n));
    }
    l2.extend(tiqs(-1, 20));
    let children = [
        record(b"DIAL", D1, &dial(b"D1\0", b"GREE")),
        group(7, D1.to_le_bytes(), &record(b"INFO", L1, &l1)),
        record(b"DIAL", D2, &dial(b"D2\0", b"HELO")),
        group(7, D2.to_le_bytes(), &record(b"INFO", L2, &l2)),
    ]
    .concat();
    let quests = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &children),
    ]
    .concat();
    let mut vmad = vec![6, 0, 2, 0];
    vmad.extend(1u16.to_le_bytes());
    text(&mut vmad, "Trig");
    vmad.push(0);
    vmad.extend(0u16.to_le_bytes());
    let refs = [
        record(
            b"ACHR",
            ACTOR,
            &[
                sub(b"NAME", &w(GUARD)),
                sub(b"DATA", &data([1000.0, 1100.0, 0.0])),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            TRIG,
            &[
                sub(b"NAME", &w(ACTI)),
                sub(b"VMAD", &vmad),
                sub(b"XPRM", &xprm([50.0, 50.0, 50.0], 1)),
                sub(b"DATA", &data([3000.0, 1000.0, 0.0])),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            MARK,
            &[
                sub(b"NAME", &w(STAT)),
                sub(b"DATA", &data([3000.0, 1100.0, 0.0])),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut refs = refs;
    if door {
        refs.extend(record(
            b"REFR",
            LEAF,
            &[
                sub(b"NAME", &w(DOOR)),
                sub(b"DATA", &data([2000.0, 1050.0, 0.0])),
            ]
            .concat(),
        ));
    }
    let mut xclc = 0i32.to_le_bytes().to_vec();
    xclc.extend(0i32.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    let cell = record(
        b"CELL",
        CELL,
        &[
            sub(b"EDID", b"Ext\0"),
            sub(b"DATA", &2u16.to_le_bytes()),
            sub(b"XCLC", &xclc),
        ]
        .concat(),
    );
    let cell_children = group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs));
    let world = [
        record(b"WRLD", WRLD, &sub(b"EDID", b"W\0")),
        group(1, WRLD.to_le_bytes(), &[cell, cell_children].concat()),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(12u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(
        0,
        *b"ACTI",
        &record(b"ACTI", ACTI, &sub(b"EDID", b"T\0")),
    ));
    plugin.extend(group(
        0,
        *b"STAT",
        &record(b"STAT", STAT, &sub(b"EDID", b"M\0")),
    ));
    if door {
        plugin.extend(group(
            0,
            *b"DOOR",
            &record(b"DOOR", DOOR, &sub(b"EDID", b"D\0")),
        ));
    }
    plugin.extend(group(
        0,
        *b"NPC_",
        &[
            record(b"NPC_", GUARD, &sub(b"EDID", b"Guard\0")),
            record(b"NPC_", HERO, &sub(b"EDID", b"Hero\0")),
            record(b"NPC_", LONE, &sub(b"EDID", b"Lone\0")),
        ]
        .concat(),
    ));
    plugin.extend(group(0, *b"QUST", &quests));
    plugin.extend(group(0, *b"WRLD", &world));
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
    let mut l = base();
    l.add(assemble(
        "script Trig extends ObjectReference
         var Int entered 0
         var ObjectReference who none
         func OnTriggerEnter none ObjectReference akActionRef
         1 entered entered 1
         13 who akActionRef",
    ));
    l.add(assemble(
        "script Dist extends ScriptObject
         var Float got 0.0
         func OnDistanceLessThan none ObjectReference akObj1 ObjectReference akObj2 Float afDistance
         13 got afDistance",
    ));
    l.add(assemble(
        "script Same
         func Is Bool global ObjectReference a ObjectReference b
         local Bool r
         15 r a b
         26 r",
    ));
    l
}

fn world_switches() -> Switches {
    Switches {
        boxes: true,
        point: true,
        instant: true,
        enabled: true,
        order: true,
        distance: true,
        distance_once: true,
        placed: true,
        actors: true,
        actor_triggers: true,
        ..Switches::default()
    }
}

/// Stages of QS set so far, with the second each was reached at.
fn stages(vm: &Vm<'_>) -> Vec<(i32, f64)> {
    vm.world
        .quests
        .log
        .iter()
        .filter_map(|(_, c)| match c {
            Change::StageSet(n) => Some(*n),
            _ => None,
        })
        .map(|n| (n, 0.0))
        .collect()
}

#[test]
fn a_unique_actor_alias_holds_its_one_placed_actor_or_the_player() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut w = crate::World::default();
    let info = quest::info_by_key(&s, 1 << 24, &mut w, &key(QS)).unwrap();
    assert_eq!(info.unique_actors.get(&0), Some(&key(GUARD)));
    assert_eq!(info.priority, Some(30));
    // Off: no reference, as before.
    assert_eq!(w.quests.fill(&info, 0), Ok(None));
    let mut u = UniqueActors::read(&s, &b);
    assert_eq!(
        u.placed[&crate::bind::record_key(&key(GUARD))],
        vec![key(ACTOR)]
    );
    w.quests.unique = Some(std::rc::Rc::new(UniqueActors::read(&s, &b)));
    assert_eq!(w.quests.fill(&info, 0), Ok(Some(Fill::Record(key(ACTOR)))));
    assert!(w.quests.fill(&info, 1).is_err(), "LONE is never placed");
    u.player_base = Some(key(LONE));
    w.quests.unique = Some(std::rc::Rc::new(u));
    assert_eq!(w.quests.fill(&info, 1), Ok(Some(Fill::Player)));
    assert_eq!(w.quests.unique_used.borrow().len(), 2);
}

/// A new game with the talk model on as asked, the player's base HERO and
/// lines lasting 2 s by assumption.
fn talk_vm<'a, 's: 'a>(
    s: &'s Store,
    l: &'a mut crate::MapLibrary,
    host: &'a mut QuestHost<'s>,
    sw: talk::Switches,
) -> Vm<'a> {
    let b = Budget::new(1 << 24);
    let mut vm = Vm::new(l, host);
    vm.world.scenes.on = true;
    vm.world.scenes.clock = scene::Clock {
        on: true,
        assume_line_seconds: Some(2.0),
        ..scene::Clock::default()
    };
    vm.world.talk.on = true;
    vm.world.talk.sw = sw;
    vm.world.talk.player_base = Some(key(HERO));
    quest::start_new_game(&mut vm, s, &b).unwrap();
    quest::run_jobs(&mut vm, s, 256, &b);
    vm
}

#[test]
fn the_player_greets_the_actor_and_the_lines_begin_half_sets_its_stage() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let sw = talk::Switches {
        greeting: true,
        stage_fields: true,
        ..talk::Switches::default()
    };
    let mut vm = talk_vm(&s, &mut l, &mut host, sw);
    let actor = crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
    let _ = vm.host.call(&talk::talk_call(actor), &mut vm.world);
    quest::run_jobs(&mut vm, &s, 256, &b);
    assert!(
        stages(&vm).iter().any(|x| x.0 == 10),
        "{:?}",
        vm.world.talk.counts
    );
    assert_eq!(
        vm.world.talk.counts["greeting: line determined: the only line that holds"],
        1
    );
    assert_eq!(
        vm.world.talk.counts["greeting: lines played, spoken by the player"],
        1
    );
    for r in [
        rules::DIALOGUE_GREETING,
        rules::DIALOGUE_PARTIES,
        rules::DIALOGUE_LINE_CHOICE,
        rules::DIALOGUE_LINE_FIELDS,
    ] {
        assert!(vm.host.rules().contains(&r), "{r}");
    }
}

#[test]
fn with_the_greeting_switch_off_no_line_plays() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let sw = talk::Switches {
        stage_fields: true,
        ..talk::Switches::default()
    };
    let mut vm = talk_vm(&s, &mut l, &mut host, sw);
    let actor = crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
    let _ = vm.host.call(&talk::talk_call(actor), &mut vm.world);
    quest::run_jobs(&mut vm, &s, 256, &b);
    assert!(!stages(&vm).iter().any(|x| x.0 == 10));
    assert_eq!(
        vm.world.talk.counts["greeting: no line (its switch is off)"],
        1
    );
}

#[test]
fn the_track_comes_within_the_hello_distance_and_the_lines_end_sets_its_stage() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let sw = talk::Switches {
        hello: true,
        stage_fields: true,
        ..talk::Switches::default()
    };
    let mut vm = talk_vm(&s, &mut l, &mut host, sw);
    let track =
        Track::parse("0 test.esm:00000801 0 1000 0\n20 test.esm:00000801 2000 1000 0\n").unwrap();
    vm.world.place = Place::load(&s, track, world_switches(), &b).unwrap();
    assert_eq!(vm.world.place.actors.len(), 1);
    vm.world.talk.hello_distance = Some(200.0);
    talk::load_hello(&mut vm.world);
    // Within 200 of (1000, 1100): x from 1000 - sqrt(200^2 - 100^2).
    let enter = (1000.0 - (200.0f64.powi(2) - 100.0f64.powi(2)).sqrt()) / 100.0;
    assert!((vm.world.talk.crossings[0].t - enter).abs() < 1e-6);
    let mut at = Vec::new();
    for _ in 0..16 {
        track::settle(&mut vm, &s, 64, &b);
        let before = vm.world.quests.log.len();
        talk::settle(&mut vm, &s, 64, &b);
        if vm.world.quests.log[before..]
            .iter()
            .any(|(_, c)| *c == Change::StageSet(20))
        {
            at.push(vm.world.now[crate::world::Clock::Real as usize]);
        }
        let Some(t) = [
            track::next_due(&s, 1 << 24, &mut vm.world),
            talk::next_due(&vm.world),
        ]
        .into_iter()
        .flatten()
        .min_by(f64::total_cmp) else {
            break;
        };
        vm.world.now[crate::world::Clock::Real as usize] = t;
    }
    assert_eq!(at.len(), 1, "{:?}", vm.world.talk.counts);
    assert!((at[0] - (enter + 2.0)).abs() < 1e-6, "{at:?}");
    assert_eq!(
        vm.world.talk.counts["hello: lines played, spoken by the actor"],
        1
    );
    assert!(vm.host.rules().contains(&rules::DIALOGUE_HELLO));
}

/// `dialogue_hello_owed_v1`: the player enters the hello distance before
/// the stage the hello line asks for; the stage comes while the player is
/// still inside (10 s) or after leaving (15 s). Off: no line either way.
/// On: the hello is offered again after the stage and plays (its end half
/// sets 20 two seconds later), only while the player is inside.
#[test]
fn a_hello_entry_that_played_no_line_is_offered_again_after_a_stage_only_under_the_switch() {
    let s = store_with(Some(10));
    let b = Budget::new(1 << 24);
    for (owed, stage_at, expect) in [
        (false, 10.0, None),
        (true, 10.0, Some(12.0)),
        (true, 15.0, None),
    ] {
        let mut l = lib();
        let mut host = QuestHost::new(&s, 1 << 24);
        let sw = talk::Switches {
            hello: true,
            stage_fields: true,
            hello_owed: owed,
            ..talk::Switches::default()
        };
        let mut vm = talk_vm(&s, &mut l, &mut host, sw);
        let track =
            Track::parse("0 test.esm:00000801 0 1000 0\n20 test.esm:00000801 2000 1000 0\n")
                .unwrap();
        vm.world.place = Place::load(&s, track, world_switches(), &b).unwrap();
        vm.world.talk.hello_distance = Some(200.0);
        talk::load_hello(&mut vm.world);
        let qo = quest::quest_object(&mut vm, &s, &key(QS), &b).unwrap();
        let mut set = false;
        let mut at = Vec::new();
        for _ in 0..32 {
            let now = vm.world.now[crate::world::Clock::Real as usize];
            if !set && now >= stage_at {
                quest::set_stage(&mut vm, &s, qo, 10);
                quest::run_jobs(&mut vm, &s, 64, &b);
                set = true;
            }
            track::settle(&mut vm, &s, 64, &b);
            let before = vm.world.quests.log.len();
            talk::settle(&mut vm, &s, 64, &b);
            if vm.world.quests.log[before..]
                .iter()
                .any(|(_, c)| *c == Change::StageSet(20))
            {
                at.push(now);
            }
            let Some(t) = [
                track::next_due(&s, 1 << 24, &mut vm.world),
                talk::next_due(&vm.world),
                (!set).then_some(stage_at),
            ]
            .into_iter()
            .flatten()
            .filter(|t| *t > now)
            .min_by(f64::total_cmp) else {
                break;
            };
            vm.world.now[crate::world::Clock::Real as usize] = t;
        }
        assert!(set);
        match expect {
            None => assert!(at.is_empty(), "{owed} {stage_at}: {at:?}"),
            Some(e) => {
                assert_eq!(at.len(), 1, "{:?}", vm.world.talk.counts);
                assert!((at[0] - e).abs() < 1e-6, "{at:?}");
                assert!(vm.rules.contains(&rules::DIALOGUE_HELLO_OWED));
            }
        }
        assert_eq!(
            vm.world
                .talk
                .counts
                .contains_key("hello: offered again after a change of the quest state, the player still inside (--hello-owed)"),
            owed && stage_at < 12.0
        );
    }
}

#[test]
fn an_assumed_actor_move_raises_the_trigger_and_the_distance_event() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    let track =
        Track::parse("0 test.esm:00000801 0 1000 0\n20 test.esm:00000801 1500 1000 0\n").unwrap();
    let mv = ActorMove {
        t: 5.0,
        actor: key(ACTOR),
        space: key(WRLD),
        pos: [3000.0, 1000.0, 0.0],
    };
    vm.world.place = Place::load_with(&s, track, world_switches(), vec![mv], &b).unwrap();
    let actor = crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
    let mark =
        crate::data::record_object(&mut vm.world, &s, &key(MARK), "objectreference", &b).unwrap();
    let inst = vm.instantiate(None, "dist").unwrap();
    let call = crate::NativeCall {
        script: "scriptobject".into(),
        name: "registerfordistancelessthanevent".into(),
        receiver: None,
        instance: Some(inst),
        args: vec![Val::Object(actor), Val::Object(mark), Val::Float(200.0)],
        param_types: vec!["scriptobject".into(), "scriptobject".into(), "float".into()],
        return_type: "none".into(),
    };
    track::call(&s, 1 << 24, &call, &mut vm.world).unwrap();
    // The actor stands at its stored place 2000 units away until 5 s.
    let due = track::next_due(&s, 1 << 24, &mut vm.world).unwrap();
    assert!((due - 5.0).abs() < 1e-9, "{due}");
    vm.world.now[crate::world::Clock::Real as usize] = due;
    let (raised, _) = track::settle(&mut vm, &s, 64, &b);
    let events: Vec<&str> = raised.iter().map(|r| r.event).collect();
    assert_eq!(
        events,
        vec!["ontriggerenter", "ondistancelessthan"],
        "{raised:?}"
    );
    let trig = vm
        .world
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some("test.esm:00000950"))
        .unwrap();
    let i = vm.world.objects[trig].instances[0];
    let var = |n: &str| {
        vm.world.instances[i]
            .vars
            .iter()
            .find(|v| v.name == n)
            .unwrap()
            .value
            .clone()
    };
    assert_eq!(var("entered"), Val::Int(1));
    assert_eq!(var("who"), Val::Object(actor));
    assert!(vm.rules.contains(rules::WORLD_ACTOR_TRIGGER));
    // Without --actor-triggers the move raises no trigger event.
    let track = Track::parse("0 test.esm:00000801 0 1000 0\n").unwrap();
    let mv = ActorMove {
        t: 5.0,
        actor: key(ACTOR),
        space: key(WRLD),
        pos: [3000.0, 1000.0, 0.0],
    };
    let sw = Switches {
        actor_triggers: false,
        ..world_switches()
    };
    vm.world.place = Place::load_with(&s, track, sw, vec![mv], &b).unwrap();
    vm.world.now[crate::world::Clock::Real as usize] = 5.0;
    let (raised, _) = track::settle(&mut vm, &s, 64, &b);
    assert!(raised.is_empty());
    assert_eq!(
        vm.world.place.counts["actor trigger events not raised\tenter\t--actor-triggers off"],
        1
    );
}

/// The deck step's finding at the level of the world model: an assumed
/// move written before a native move of the same actor loses to it, so a
/// distance registration made after both never sees the actor arrive; the
/// same move applied after the native move (as a gate holding it until the
/// stage that made the native move does) stands, and the event comes.
#[test]
fn a_native_move_after_an_assumed_one_puts_the_actor_back_and_a_later_move_stands() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    let run = |vm: &mut Vm<'_>, move_at: f64| -> Vec<(&'static str, f64)> {
        let track = Track::parse("0 test.esm:00000801 0 1000 0\n30 test.esm:00000801 100 1000 0\n")
            .unwrap();
        let mv = ActorMove {
            t: move_at,
            actor: key(ACTOR),
            space: key(WRLD),
            pos: [3000.0, 1050.0, 0.0],
        };
        vm.world = crate::World::default();
        vm.world.place = Place::load_with(&s, track, world_switches(), vec![mv], &b).unwrap();
        let actor =
            crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
        let mark = crate::data::record_object(&mut vm.world, &s, &key(MARK), "objectreference", &b)
            .unwrap();
        let mut got = Vec::new();
        for t in [5.0, 8.0, 10.0, 12.0, 20.0] {
            vm.world.now[crate::world::Clock::Real as usize] = t;
            if t == 8.0 {
                // The stage's fragment moves the actor (a SetPosition: from
                // here its place is the native's, not the stored one).
                let call = crate::NativeCall {
                    script: "objectreference".into(),
                    name: "setposition".into(),
                    receiver: Some(Val::Object(actor)),
                    instance: None,
                    args: vec![Val::Float(0.0), Val::Float(0.0), Val::Float(0.0)],
                    param_types: vec!["float".into(), "float".into(), "float".into()],
                    return_type: "none".into(),
                };
                track::observe(&s, 1 << 24, &call, &mut vm.world);
            }
            if t == 10.0 {
                let inst = vm.instantiate(None, "dist").unwrap();
                let call = crate::NativeCall {
                    script: "scriptobject".into(),
                    name: "registerfordistancelessthanevent".into(),
                    receiver: None,
                    instance: Some(inst),
                    args: vec![Val::Object(actor), Val::Object(mark), Val::Float(200.0)],
                    param_types: vec!["scriptobject".into(), "scriptobject".into(), "float".into()],
                    return_type: "none".into(),
                };
                track::call(&s, 1 << 24, &call, &mut vm.world).unwrap();
            }
            let (raised, _) = track::settle(vm, &s, 64, &b);
            got.extend(
                raised
                    .iter()
                    .filter(|r| r.event == "ondistancelessthan")
                    .map(|r| (r.event, r.t)),
            );
        }
        got
    };
    // Written at 5 s, before the native move at 8 s: the actor never
    // arrives for the registration made at 10 s.
    assert!(run(&mut vm, 5.0).is_empty());
    // Applied at 12 s, after the native move: the event comes at 12 s.
    assert_eq!(run(&mut vm, 12.0), vec![("ondistancelessthan", 12.0)]);
}

#[test]
fn the_player_is_no_records_handle_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let f = l.scripts["same"].function("", "is").unwrap().clone();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let p = vm.world.player("actor");
    let a = crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
    let off = vm.run(None, f.clone(), vec![Val::Object(p), Val::Object(a)]);
    assert!(
        matches!(off, Outcome::Completed(Val::Unknown(_))),
        "{off:?}"
    );
    vm.world.player_no_record = true;
    let on = vm.run(None, f, vec![Val::Object(p), Val::Object(a)]);
    assert_eq!(on, Outcome::Completed(Val::Bool(false)));
    assert!(vm.rules.contains(rules::PLAYER_NO_RECORD));
}

#[test]
fn a_childs_own_flag_is_read_only_under_its_rule() {
    use fo4_plugin::enable::{Chain, ChainEnd, EnableLink, Node};
    let node = |id: u32, sig: &[u8; 4], flag: bool, link: Option<EnableLink>| Node {
        key: key(id),
        signature: *sig,
        initially_disabled: flag,
        deleted: false,
        link,
    };
    let ch = Chain {
        nodes: vec![
            node(
                ACTOR,
                b"ACHR",
                true,
                Some(EnableLink {
                    raw: MARK,
                    parent: Some(key(MARK)),
                    flags: 1,
                    reserved: [0; 3],
                    subrecord: 1,
                }),
            ),
            node(MARK, b"REFR", false, None),
        ],
        end: ChainEnd::Root,
    };
    let set = std::collections::HashMap::new();
    assert!(crate::data::chain_state(&ch, &set).is_err());
    let (on, used) = crate::data::chain_state_with(&ch, &set, true).unwrap();
    // The root is enabled and the link opposite: the child is disabled.
    assert!(!on);
    assert!(used.contains(&rules::XESP_CHILD_FLAG_IGNORED));
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::ALIAS_UNIQUE_ACTOR,
        rules::WORLD_ACTOR_POSITION,
        rules::WORLD_ACTOR_TRIGGER,
        rules::DIALOGUE_PARTIES,
        rules::DIALOGUE_LINE_CHOICE,
        rules::DIALOGUE_LINE_PRIORITY,
        rules::DIALOGUE_GREETING,
        rules::DIALOGUE_HELLO,
        rules::DIALOGUE_LINE_FIELDS,
        rules::PLAYER_NO_RECORD,
        rules::XESP_CHILD_FLAG_IGNORED,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

/// `--play-door-toggle` as it is (`spec/play.md`, Actors and a door the
/// person shut; `play_door_person_closed_actors_unchanged_v1`): the play
/// mode gives the run the person's E that shuts a door as an activation of
/// it and nothing else (`fo4_play::walker`'s
/// `closing_a_door_the_person_opens_gives_the_run_the_activation_alone`),
/// and the run's actors move by the moves scheduled for them, which read
/// no door: a move whose way crosses the shut door's place lands where and
/// when it does without the activation, the trigger it enters raises the
/// same event, and nothing of the run's own says the door is shut.
#[test]
fn an_actor_move_lands_past_a_door_the_player_activated() {
    let s = store_full(None, true);
    let run = |activate: bool| {
        let b = Budget::new(1 << 24);
        let mut l = lib();
        let mut host = QuestHost::new(&s, 1 << 24);
        let mut attach = RecordScripts::new(&s, 1 << 24);
        let mut vm = Vm::new(&mut l, &mut host);
        vm.attach = Some(&mut attach);
        let track =
            Track::parse("0 test.esm:00000801 0 1000 0\n20 test.esm:00000801 1500 1000 0\n")
                .unwrap();
        // From its stored place (1000, 1100) to the trigger at (3000,
        // 1000): the straight way passes the door at (2000, 1050).
        let mv = ActorMove {
            t: 5.0,
            actor: key(ACTOR),
            space: key(WRLD),
            pos: [3000.0, 1000.0, 0.0],
        };
        vm.world.place = Place::load_with(&s, track, world_switches(), vec![mv], &b).unwrap();
        if activate {
            vm.world.now[crate::world::Clock::Real as usize] = 3.0;
            let (runs, _) = crate::player::deliver(
                &mut vm,
                &s,
                &key(LEAF),
                crate::player::Action::Activate,
                &b,
            )
            .unwrap();
            // The door has no script: the activation runs nothing.
            assert!(runs.is_empty());
        }
        let due = track::next_due(&s, 1 << 24, &mut vm.world).unwrap();
        vm.world.now[crate::world::Clock::Real as usize] = due;
        let (raised, _) = track::settle(&mut vm, &s, 64, &b);
        let events: Vec<String> = raised.iter().map(|r| r.event.to_string()).collect();
        // Where the run has the actor now: its last move applied.
        let place = &vm.world.place;
        let at = place.actor_moves[..place.actor_cursor]
            .iter()
            .rev()
            .find(|m| m.actor == key(ACTOR))
            .map(|m| (m.space.clone(), m.pos));
        // No door state in the run: no open or close is made for the door.
        let door_effects = vm
            .effects
            .iter()
            .filter(|e| e.native.contains("setopen") || e.native.contains("lock"))
            .count();
        (due, events, at, door_effects)
    };
    let shut = run(true);
    assert_eq!(shut, run(false));
    let (due, events, at, door_effects) = shut;
    assert!((due - 5.0).abs() < 1e-9, "{due}");
    assert_eq!(events, vec!["ontriggerenter".to_string()]);
    assert_eq!(at, Some((key(WRLD), [3000.0, 1000.0, 0.0])));
    assert_eq!(door_effects, 0);
}
