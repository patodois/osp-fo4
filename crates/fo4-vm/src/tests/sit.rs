//! The sit step's readings over synthetic plugins (no game data): location
//! fills from repeated special reference rows and from placed references,
//! condition function 1 from positions with the player's reference word,
//! furniture use and menu registrations.
use super::assemble;
use super::quest::{group, key, record, sub, Owned};
use crate::{
    cond::{eval_term, Facts, Party, Truth},
    menu,
    quest::{self, QuestHost},
    rules, sit, NativeCall, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::{actor::Context, condition::Ctda, typed::Store, FormKey};
use std::collections::BTreeSet;

const QS: u32 = 0x900;
const CELL: u32 = 0x930;
const LOC: u32 = 0x940;
const LT2: u32 = 0x941;
const LT3: u32 = 0x942;
const STAT: u32 = 0x951;
const FURN: u32 = 0x952;
const R1: u32 = 0x960;
const R3: u32 = 0x962;
const CHAIR: u32 = 0x963;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A quest with two aliases filled from a forced location by a ref type:
/// alias 0 by a type whose two (identical) rows name one reference, alias
/// 1 by a type the list holds no row of, carried by one placed reference
/// of a cell of the location; and a chair (a REFR of a FURN).
fn store() -> Store {
    let qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"ANAM", &w(3)),
        sub(b"ALST", &w(0)),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFA", &w(2)),
        sub(b"ALRT", &w(LT2)),
        sub(b"ALED", &[]),
        sub(b"ALST", &w(1)),
        sub(b"ALID", b"B\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFA", &w(2)),
        sub(b"ALRT", &w(LT3)),
        sub(b"ALED", &[]),
        sub(b"ALLS", &w(2)),
        sub(b"ALID", b"L\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFL", &w(LOC)),
        sub(b"ALED", &[]),
    ]
    .concat();
    let refs = [
        record(b"REFR", R1, &sub(b"NAME", &w(STAT))),
        record(
            b"REFR",
            R3,
            &[sub(b"NAME", &w(STAT)), sub(b"XLRT", &w(LT3))].concat(),
        ),
        record(b"REFR", CHAIR, &sub(b"NAME", &w(FURN))),
    ]
    .concat();
    let cell = record(
        b"CELL",
        CELL,
        &[sub(b"EDID", b"Room\0"), sub(b"XLCN", &w(LOC))].concat(),
    );
    let cells = group(
        2,
        [0; 4],
        &[
            cell,
            group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs)),
        ]
        .concat(),
    );
    let mut row = w(LT2);
    row.extend(w(R1));
    row.extend(w(CELL));
    row.extend(w(0));
    let lcsr = [row.clone(), row].concat();
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
        *b"LCRT",
        &[
            record(b"LCRT", LT2, &sub(b"EDID", b"LT2\0")),
            record(b"LCRT", LT3, &sub(b"EDID", b"LT3\0")),
        ]
        .concat(),
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
        *b"FURN",
        &record(b"FURN", FURN, &sub(b"EDID", b"Chair\0")),
    ));
    plugin.extend(group(0, *b"QUST", &record(b"QUST", QS, &qs)));
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

#[test]
fn repeated_rows_and_placed_references_fill_location_aliases_only_under_their_rules() {
    let s = store();
    let b = Budget::new(1 << 24);
    let i = quest::QuestInfo::load(&s, &key(QS), &b).unwrap().unwrap();
    assert_eq!(i.location_rows[&0].listed, [key(R1), key(R1)]);
    assert_eq!(i.location_rows[&0].distinct(), [&key(R1)]);
    assert!(i.location_rows[&1].listed.is_empty());
    let mut q = quest::Quests {
        location_fills: true,
        ..Default::default()
    };
    // The list's own reading: several rows, and no row.
    assert!(q.fill_location(&i, 0).is_err());
    assert!(q.fill_location(&i, 1).is_err());
    q.location_distinct = true;
    assert_eq!(q.fill_location(&i, 0), Ok(Some(key(R1))));
    assert!(q.fill_location(&i, 1).is_err());
    q.location_placed = Some(std::rc::Rc::new(quest::LocationPlaced::read(&s, &b)));
    assert_eq!(q.fill_location(&i, 1), Ok(Some(key(R3))));
    let used = q.location_used.borrow().clone();
    assert!(used.contains(rules::ALIAS_LOCATION_DISTINCT));
    assert!(used.contains(rules::ALIAS_LOCATION_PLACED));
    // With the list's reading off, neither applies.
    q.location_fills = false;
    assert_eq!(q.fill_location(&i, 0), Ok(None));
    assert_eq!(q.fill_location(&i, 1), Ok(None));
}

/// Facts that place the player and one reference at fixed distances.
struct Near<'s> {
    store: &'s Store,
    on: bool,
    player_word: bool,
    seen: Vec<(Party, Party)>,
}
impl Facts for Near<'_> {
    fn store(&self) -> &Store {
        self.store
    }
    fn quest_running(&mut self, _: &FormKey) -> Result<bool, &'static str> {
        Err("no")
    }
    fn quest_stage(&mut self, _: &FormKey) -> Result<i32, &'static str> {
        Err("no")
    }
    fn stage_done(&mut self, _: &FormKey, _: i32) -> Result<bool, &'static str> {
        Err("no")
    }
    fn global(&mut self, _: &FormKey) -> Result<f32, &'static str> {
        Err("no")
    }
    fn owner(&self) -> Option<&FormKey> {
        None
    }
    fn distance_on(&self) -> bool {
        self.on
    }
    fn player_word_on(&self) -> bool {
        self.player_word
    }
    fn distance(&mut self, a: &Party, b: &Party) -> Result<f32, &'static str> {
        self.seen.push((a.clone(), b.clone()));
        Ok(300.0)
    }
}

#[test]
fn function_1_reads_positions_and_the_player_word_only_with_their_switches() {
    let s = store();
    // Run-on 2 on the reserved id 0x14, parameter 1 a placed reference,
    // distance at most 350.
    let c = Ctda {
        byte0: 5 << 5,
        unused: [0; 3],
        value: 350.0f32.to_bits(),
        function: 1,
        pad: [0; 2],
        param1: R1,
        param2: 0,
        run_on: 2,
        reference: 0x14,
        param3: -1,
    };
    let ctx = Context {
        order: s.order(),
        plugin: 0,
    };
    let eval = |on: bool, player_word: bool| {
        let mut f = Near {
            store: &s,
            on,
            player_word,
            seen: Vec::new(),
        };
        let mut used = BTreeSet::new();
        let t = eval_term(&c, ctx, "test.esm", &mut f, &mut used);
        (t, f.seen, used)
    };
    let (t, seen, _) = eval(false, true);
    assert!(matches!(t, Truth::Undetermined(_)) && seen.is_empty());
    let (t, seen, _) = eval(true, false);
    assert!(
        matches!(t, Truth::Undetermined(_)) && seen.is_empty(),
        "an id no record defines names no one without the player word"
    );
    let (t, seen, used) = eval(true, true);
    assert_eq!(t, Truth::True);
    assert_eq!(seen, [(Party::Player, Party::Record(key(R1)))]);
    assert!(used.contains(rules::COND_FN_1_TRACK) && used.contains(rules::COND_PLAYER_WORD));
}

fn furniture_lib() -> crate::MapLibrary {
    let mut l = super::quest::library();
    l.add(assemble(
        "script Sitter extends ObjectReference
         var ObjectReference sat
         var ObjectReference rose
         func OnSit none ObjectReference akFurniture
         13 sat akFurniture
         func OnGetUp none ObjectReference akFurniture
         13 rose akFurniture",
    ));
    l.add(assemble(
        "script Seat extends ObjectReference
         var ObjectReference left
         var ObjectReference used
         func OnExitFurniture none ObjectReference akActionRef
         13 left akActionRef
         func OnActivate none ObjectReference akActionRef
         13 used akActionRef",
    ));
    l.add(assemble(
        "script MenuUser extends ObjectReference
         var String name \"\"
         var Bool opening false
         var Int flavor 0
         func OnMenuOpenCloseEvent none String asMenuName Bool abOpening
         13 name asMenuName
         13 opening abOpening
         func OnLooksMenuEvent none Int aiFlavor
         13 flavor aiFlavor",
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

fn call(name: &str, receiver: usize, instance: Option<usize>, args: Vec<Val>) -> NativeCall {
    NativeCall {
        script: if name.starts_with("register") || name.starts_with("unregister") {
            "scriptobject".into()
        } else {
            "objectreference".into()
        },
        name: name.into(),
        receiver: Some(Val::Object(receiver)),
        instance,
        args,
        param_types: Vec::new(),
        return_type: "none".into(),
    }
}

#[test]
fn a_scripted_default_activation_sits_and_the_events_reach_sitter_and_furniture() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = furniture_lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let chair =
        crate::data::record_object(&mut vm.world, &s, &key(CHAIR), "objectreference", &b).unwrap();
    let seat = vm.instantiate(Some(chair), "seat").unwrap();
    let actor = vm
        .world
        .object("actor", Some("test.esm:00000970".into()), &["actor"]);
    let me = vm.instantiate(Some(actor), "sitter").unwrap();
    let player = vm.world.player("actor");
    let activate = |v: bool| {
        call(
            "activate",
            chair,
            None,
            vec![Val::Object(player), Val::Bool(v)],
        )
    };
    // Off: nothing is read.
    assert!(sit::native(&s, 1 << 24, &activate(true), &mut vm.world).is_none());
    assert!(vm.world.sits.pending.is_empty());
    vm.world.sits.activate = true;
    sit::native(&s, 1 << 24, &activate(false), &mut vm.world);
    assert!(vm.world.sits.pending.is_empty(), "with events: not read");
    sit::native(&s, 1 << 24, &activate(true), &mut vm.world);
    assert_eq!(vm.world.sits.pending, [(player, chair)]);
    assert!(vm.world.sits.used.contains(&rules::SIT_ACTIVATE));
    // In use: answered only with its switch.
    let in_use = call("isfurnitureinuse", chair, None, vec![Val::Bool(false)]);
    assert!(sit::native(&s, 1 << 24, &in_use, &mut vm.world).is_none());
    vm.world.sits.in_use = true;
    let (raised, _) = sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(raised.len(), 1);
    assert!(matches!(
        sit::native(&s, 1 << 24, &in_use, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(true)))
    ));
    // An actor's own scripts receive OnSit and OnGetUp; the furniture's,
    // OnExitFurniture with the actor.
    sit::sit(&mut vm, &s, actor, chair, &b).unwrap();
    assert_eq!(var(&vm, me, "sat"), Val::Object(chair));
    sit::get_up(&mut vm, &s, actor, chair, &b).unwrap();
    assert_eq!(var(&vm, me, "rose"), Val::Object(chair));
    assert_eq!(var(&vm, seat, "left"), Val::Object(actor));
    assert!(vm.rules.contains(rules::SIT_EVENTS));
}

#[test]
fn menu_registrations_are_kept_and_the_callers_events_delivered_only_with_the_switch() {
    let s = store();
    let mut l = furniture_lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let o = vm.world.object(
        "ref",
        Some("test.esm:00000801".into()),
        &["objectreference"],
    );
    let i = vm.instantiate(Some(o), "menuuser").unwrap();
    let reg = call(
        "registerformenuopencloseevent",
        o,
        Some(i),
        vec![Val::Str("SomeMenu".into())],
    );
    assert!(menu::native(&reg, &mut vm.world).is_none());
    assert!(menu::open_close(&mut vm, "somemenu", true).is_empty());
    vm.world.menus.on = true;
    assert!(menu::native(&reg, &mut vm.world).is_some());
    menu::native(
        &call("registerforlooksmenuevent", o, Some(i), Vec::new()),
        &mut vm.world,
    );
    let out = menu::open_close(&mut vm, "SOMEMENU", true);
    assert_eq!(out.len(), 1);
    assert_eq!(var(&vm, i, "name"), Val::Str("SomeMenu".into()));
    assert_eq!(var(&vm, i, "opening"), Val::Bool(true));
    assert_eq!(menu::looks(&mut vm, 7).len(), 1);
    assert_eq!(var(&vm, i, "flavor"), Val::Int(7));
    menu::native(
        &call(
            "unregisterforallmenuopencloseevents",
            o,
            Some(i),
            Vec::new(),
        ),
        &mut vm.world,
    );
    assert!(menu::open_close(&mut vm, "SomeMenu", false).is_empty());
    assert!(vm.rules.contains(rules::MENU_EVENTS));
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::ALIAS_LOCATION_DISTINCT,
        rules::ALIAS_LOCATION_PLACED,
        rules::COND_FN_1_TRACK,
        rules::COND_PLAYER_WORD,
        rules::EVENT_ACTIVATE_ACTOR,
        rules::SIT_EVENTS,
        rules::SIT_ACTIVATE,
        rules::SIT_IN_USE,
        rules::MENU_EVENTS,
        rules::PLAYER_BASE_OBJECT,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn an_activation_with_events_sits_and_one_by_the_sitter_gets_it_up_each_with_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = furniture_lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let chair =
        crate::data::record_object(&mut vm.world, &s, &key(CHAIR), "objectreference", &b).unwrap();
    let seat = vm.instantiate(Some(chair), "seat").unwrap();
    let player = vm.world.player("actor");
    let activate = |v: bool| {
        call(
            "activate",
            chair,
            None,
            vec![Val::Object(player), Val::Bool(v)],
        )
    };
    // The default-processing reading alone leaves an activation with events
    // unread, as before.
    vm.world.sits.activate = true;
    sit::native(&s, 1 << 24, &activate(false), &mut vm.world);
    assert!(!vm.world.sits.has_pending());
    vm.world.sits.activate_events = true;
    sit::native(&s, 1 << 24, &activate(false), &mut vm.world);
    assert_eq!(vm.world.sits.pending, [(player, chair)]);
    assert!(vm.world.sits.used.contains(&rules::SIT_ACTIVATE_EVENTS));
    // No graph event is noted unless the caller names them.
    assert!(vm.world.sits.graph_sends.is_empty());
    sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(vm.world.sits.sitting.get(&player), Some(&chair));
    // Activated again by its sitter: without the get-up reading it is read
    // as a sit in the same furniture (nothing changes)...
    sit::native(&s, 1 << 24, &activate(false), &mut vm.world);
    sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(vm.world.sits.sitting.get(&player), Some(&chair));
    assert_eq!(var(&vm, seat, "left"), Val::None);
    // ...with it the sitter gets up, OnExitFurniture reaching the furniture,
    // and the caller's get-up event is noted for the furniture's graph.
    vm.world.sits.activate_get_up = true;
    vm.world.sits.graph_events = Some(("Sit".into(), "Stand".into()));
    sit::native(&s, 1 << 24, &activate(true), &mut vm.world);
    assert!(vm.world.sits.pending.is_empty());
    assert_eq!(vm.world.sits.pending_get_ups, [(player, chair)]);
    assert_eq!(vm.world.sits.graph_sends, [(chair, "Stand".to_string())]);
    assert!(vm.world.sits.used.contains(&rules::SIT_ACTIVATE_GET_UP));
    assert!(vm.world.sits.used.contains(&rules::FURNITURE_GRAPH_EVENTS));
    let (raised, _) = sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(raised.len(), 1);
    assert!(!vm.world.sits.sitting.contains_key(&player));
    assert_eq!(var(&vm, seat, "left"), Val::Object(player));
    // Activated once more, it sits again and the sit event is noted.
    vm.world.sits.graph_sends.clear();
    sit::native(&s, 1 << 24, &activate(false), &mut vm.world);
    assert_eq!(vm.world.sits.graph_sends, [(chair, "Sit".to_string())]);
}

#[test]
fn an_activation_with_events_delivers_the_furnitures_activation_before_the_sit_only_with_its_switch(
) {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = furniture_lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let chair =
        crate::data::record_object(&mut vm.world, &s, &key(CHAIR), "objectreference", &b).unwrap();
    let seat = vm.instantiate(Some(chair), "seat").unwrap();
    let player = vm.world.player("actor");
    let activate = call(
        "activate",
        chair,
        None,
        vec![Val::Object(player), Val::Bool(false)],
    );
    // As before: the sit alone, the furniture's OnActivate not delivered.
    vm.world.sits.activate_events = true;
    sit::native(&s, 1 << 24, &activate, &mut vm.world);
    assert!(vm.world.sits.pending_activations.is_empty());
    sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(vm.world.sits.sitting.get(&player), Some(&chair));
    assert_eq!(var(&vm, seat, "used"), Val::None);
    assert!(!vm.rules.contains(rules::SIT_ACTIVATE_EVENTS_DELIVERED));
    // With the delivery: OnActivate(player) reaches the furniture's script,
    // then the sit (logged in that order).
    vm.world.sits.sitting.clear();
    vm.world.sits.log.clear();
    vm.world.sits.activate_events_deliver = true;
    sit::native(&s, 1 << 24, &activate, &mut vm.world);
    assert_eq!(vm.world.sits.pending_activations, [(player, chair)]);
    assert_eq!(vm.world.sits.from_events, [(player, chair)]);
    assert!(vm.world.sits.has_pending());
    let (raised, _) = sit::settle(&mut vm, &s, 16, &b);
    assert_eq!(raised.len(), 2);
    assert!(raised[0].what.contains("activates"), "{}", raised[0].what);
    assert!(raised[1].what.contains("sits in"), "{}", raised[1].what);
    assert_eq!(var(&vm, seat, "used"), Val::Object(player));
    assert_eq!(vm.world.sits.sitting.get(&player), Some(&chair));
    assert!(vm.rules.contains(rules::SIT_ACTIVATE_EVENTS_DELIVERED));
    // A default-processing-only activation delivers nothing more.
    vm.world.sits.sitting.clear();
    vm.world.sits.activate = true;
    let quiet = call(
        "activate",
        chair,
        None,
        vec![Val::Object(player), Val::Bool(true)],
    );
    vm.world.sits.from_events.clear();
    sit::native(&s, 1 << 24, &quiet, &mut vm.world);
    assert!(vm.world.sits.pending_activations.is_empty());
    assert!(vm.world.sits.from_events.is_empty());
}

#[test]
fn every_rule_of_the_exit_step_is_registered_pending() {
    for r in [
        rules::SIT_ACTIVATE_EVENTS,
        rules::SIT_ACTIVATE_GET_UP,
        rules::FURNITURE_GRAPH_EVENTS,
        rules::SIT_ACTIVATE_EVENTS_DELIVERED,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn menu_mode_and_message_buttons_answer_only_the_callers_assumptions() {
    let s = store();
    let mut l = furniture_lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let native = |script: &str, name: &str| NativeCall {
        script: script.into(),
        name: name.into(),
        receiver: None,
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "bool".into(),
    };
    let mode = native("utility", "isinmenumode");
    let show = native("message", "show");
    assert!(menu::menu_mode(&mode, &mut vm.world).is_none());
    assert!(menu::menu_mode(&show, &mut vm.world).is_none());
    vm.world.menus.assume_menu_mode = Some(false);
    vm.world.menus.assume_message_button = Some(2);
    assert!(matches!(
        menu::menu_mode(&mode, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(false)))
    ));
    assert!(matches!(
        menu::menu_mode(&show, &mut vm.world),
        Some(crate::Reply::Value(Val::Int(2)))
    ));
    // Other natives are left to the host.
    assert!(menu::menu_mode(&native("utility", "wait"), &mut vm.world).is_none());
}
