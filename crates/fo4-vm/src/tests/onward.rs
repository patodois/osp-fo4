//! The readings past the museum over synthetic plugins and scripts (no game
//! data): a terminal's menu item fragment, a script's lock change and its
//! event, an item added with its inventory filter and count, the player's
//! perks and an actor's factions from their base records, the caller's
//! actor values, and the run library's new-game object stepped over a
//! synthetic manifest.
use super::assemble;
use super::quest::{group, key, record, sub, text, Owned};
use crate::{
    inventory,
    quest::QuestHost,
    rules,
    run::{NewGame, Stepper, Stepping},
    runstate, terminal,
    track::Point,
    NativeCall, Outcome, Reply, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt, TestDir};
use fo4_plugin::typed::Store;

const CELL: u32 = 0x930;
const TERM: u32 = 0x940;
const TERMREF: u32 = 0x941;
const DOOR: u32 = 0x942;
const GATE: u32 = 0x943;
const BOX: u32 = 0x944;
const HOLDER: u32 = 0x945;
const ITEM: u32 = 0x950;
const OTHER: u32 = 0x951;
const PERK: u32 = 0x960;
const PERK2: u32 = 0x961;
const FACT: u32 = 0x962;
const FACT2: u32 = 0x963;
const NPC: u32 = 0x964;
const ACTOR: u32 = 0x965;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A TERM VMAD: no scripts, then the block: version 3, the fragment script
/// with no properties, one fragment of index 1.
fn term_vmad() -> Vec<u8> {
    let mut d = vec![6, 0, 2, 0, 0, 0, 3];
    text(&mut d, "TermFrag");
    d.extend([0, 0, 0, 1, 0, 1, 0, 0, 0, 1]);
    text(&mut d, "TermFrag");
    text(&mut d, "Fragment_Terminal_01");
    d
}

/// A rank-carrying list entry (`PRKR`, `SNAM`): a FormID, then a rank byte.
fn ranked(sig: &[u8; 4], id: u32, rank: i8) -> Vec<u8> {
    let mut d = w(id);
    d.push(rank as u8);
    sub(sig, &d)
}

/// A plugin with a cell holding a terminal, a locked gate, a holder and a
/// placed actor; items, perks, factions and an actor base.
fn store() -> Store {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(30u32.to_le_bytes());
    hedr.extend(0x990u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    for (sig, id, edid) in [
        (b"MISC", ITEM, "Item"),
        (b"MISC", OTHER, "Other"),
        (b"PERK", PERK, "P1"),
        (b"PERK", PERK2, "P2"),
        (b"FACT", FACT, "F1"),
        (b"FACT", FACT2, "F2"),
        (b"DOOR", DOOR, "D"),
        (b"CONT", BOX, "B"),
    ] {
        let mut e = edid.as_bytes().to_vec();
        e.push(0);
        plugin.extend(group(0, *sig, &record(sig, id, &sub(b"EDID", &e))));
    }
    plugin.extend(group(
        0,
        *b"TERM",
        &record(
            b"TERM",
            TERM,
            &[sub(b"EDID", b"T\0"), sub(b"VMAD", &term_vmad())].concat(),
        ),
    ));
    plugin.extend(group(
        0,
        *b"NPC_",
        &record(
            b"NPC_",
            NPC,
            &[
                sub(b"EDID", b"Someone\0"),
                ranked(b"PRKR", PERK, 1),
                ranked(b"SNAM", FACT, 0),
                ranked(b"SNAM", FACT2, -1),
            ]
            .concat(),
        ),
    ));
    let mut xloc = vec![255u8];
    xloc.extend([0u8; 15]);
    let refs = [
        record(b"REFR", TERMREF, &sub(b"NAME", &w(TERM))),
        record(
            b"REFR",
            GATE,
            &[sub(b"NAME", &w(DOOR)), sub(b"XLOC", &xloc)].concat(),
        ),
        record(b"REFR", HOLDER, &sub(b"NAME", &w(BOX))),
        record(b"ACHR", ACTOR, &sub(b"NAME", &w(NPC))),
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
    l.add(assemble("script Terminal extends Form"));
    l.add(assemble(
        "script TermFrag extends Terminal
         var ObjectReference got
         func Fragment_Terminal_01 none ObjectReference akTerminalRef
         14 got akTerminalRef",
    ));
    l.add(assemble(
        "script Gate extends ObjectReference
         var Int changed 0
         func OnLockStateChanged none
         1 changed changed 1",
    ));
    l.add(assemble(
        "script Box extends ObjectReference
         var Form wanted
         var Int added 0
         var ObjectReference from
         func OnInit none
         local none ::nonevar
         local Form ::f
         14 ::f wanted
         23 AddInventoryEventFilter self ::nonevar 1 ::f
         func OnItemAdded none Form akBaseItem Int aiItemCount ObjectReference akItemReference ObjectReference akSourceContainer
         1 added added aiItemCount
         14 from akSourceContainer",
    ));
    l
}

fn var(vm: &Vm<'_>, inst: usize, name: &str) -> Option<Val> {
    vm.world.instances[inst]
        .vars
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(name))
        .map(|v| v.value.clone())
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
fn a_terminal_pick_runs_the_fragment_of_the_items_id_only_under_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    assert_eq!(
        terminal::fragment_function(&s, &key(TERM), 1, &b).unwrap(),
        Some("fragment_terminal_01".to_string())
    );
    assert_eq!(
        terminal::fragment_function(&s, &key(TERM), 2, &b).unwrap(),
        None
    );
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let t = crate::data::record_object(&mut vm.world, &s, &key(TERMREF), "objectreference", &b)
        .unwrap();
    // Off: the pick is logged, nothing runs.
    let r = terminal::pick(&mut vm, &s, t, 1, &b).unwrap();
    assert!(r.runs.is_empty());
    assert!(!vm.rules.contains(rules::TERMINAL_FRAGMENT));
    // On: the fragment runs on the block's instance with the terminal.
    vm.world.runstate.terminals = true;
    let r = terminal::pick(&mut vm, &s, t, 1, &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    assert!(matches!(r.runs[0].outcome, Outcome::Completed(_)));
    let form = crate::data::record_object(&mut vm.world, &s, &key(TERM), "terminal", &b).unwrap();
    let inst = vm.world.objects[form].instances[0];
    assert_eq!(var(&vm, inst, "got"), Some(Val::Object(t)));
    assert!(vm.rules.contains(rules::TERMINAL_FRAGMENT));
    // An id no fragment names runs nothing.
    let r = terminal::pick(&mut vm, &s, t, 3, &b).unwrap();
    assert!(r.runs.is_empty());
}

#[test]
fn a_scripts_lock_change_raises_its_event_only_under_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let d =
        crate::data::record_object(&mut vm.world, &s, &key(GATE), "objectreference", &b).unwrap();
    vm.world.objects[d].attached = true;
    let g = vm.instantiate(Some(d), "gate").unwrap();
    let lock = |v: bool| {
        call(
            "objectreference",
            "lock",
            Val::Object(d),
            vec![Val::Bool(v), Val::Bool(false)],
            "none",
        )
    };
    vm.world.runstate.locks = true;
    // Off: the lock changes, no event waits.
    runstate::native(&s, 1 << 24, &lock(false), &mut vm.world);
    assert!(vm.world.runstate.lock_changed.is_empty());
    // On: locking it again is a change (it was unlocked); locking a locked
    // reference is none.
    vm.world.runstate.lock_events_script = true;
    runstate::native(&s, 1 << 24, &lock(true), &mut vm.world);
    runstate::native(&s, 1 << 24, &lock(true), &mut vm.world);
    assert_eq!(vm.world.runstate.lock_changed, vec![d]);
    let raised = runstate::deliver_lock_changes(&mut vm, &s, &b).unwrap();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].runs.len(), 1);
    assert_eq!(var(&vm, g, "changed"), Some(Val::Int(1)));
    assert!(vm.rules.contains(rules::LOCK_STATE_EVENT_SCRIPT));
    assert!(vm.world.runstate.lock_changed.is_empty());
}

#[test]
fn an_item_added_reaches_an_instance_whose_filter_names_it_and_raises_a_known_count() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let h =
        crate::data::record_object(&mut vm.world, &s, &key(HOLDER), "objectreference", &b).unwrap();
    let item = crate::data::record_object(&mut vm.world, &s, &key(ITEM), "miscobject", &b).unwrap();
    let other =
        crate::data::record_object(&mut vm.world, &s, &key(OTHER), "miscobject", &b).unwrap();
    let player = vm.world.player("actor");
    vm.world.objects[h].attached = true;
    let bx = vm.instantiate(Some(h), "box").unwrap();
    vm.set_property_variable(bx, "wanted", Val::Object(item))
        .ok();
    if let Some(v) = vm.world.instances[bx]
        .vars
        .iter_mut()
        .find(|v| v.name.eq_ignore_ascii_case("wanted"))
    {
        v.value = Val::Object(item);
    }
    // Every reading off: logged only.
    let r = inventory::item_added(&mut vm, &s, h, item, 1, Some(player), &b).unwrap();
    assert!(r.runs.is_empty());
    // The event on, the init reading off: the instance keeps no filter.
    vm.world.runstate.inventory.events = true;
    let r = inventory::item_added(&mut vm, &s, h, item, 1, Some(player), &b).unwrap();
    assert!(r.runs.is_empty());
    assert_eq!(var(&vm, bx, "added"), Some(Val::Int(0)));
    // The init reading on: its filter names the item; another item is not
    // delivered.
    vm.world.runstate.inventory.filter_init = true;
    let r = inventory::item_added(&mut vm, &s, h, other, 1, Some(player), &b).unwrap();
    assert!(r.runs.is_empty());
    let r = inventory::item_added(&mut vm, &s, h, item, 2, Some(player), &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    assert_eq!(var(&vm, bx, "added"), Some(Val::Int(2)));
    assert_eq!(var(&vm, bx, "from"), Some(Val::Object(player)));
    assert!(vm.rules.contains(rules::ITEM_ADDED_EVENT));
    assert!(vm.rules.contains(rules::INVENTORY_FILTER_INIT));
    // A filter the run's own call adds and removes.
    let add = NativeCall {
        instance: Some(bx),
        ..call(
            "scriptobject",
            "addinventoryeventfilter",
            Val::Object(h),
            vec![Val::Object(other)],
            "none",
        )
    };
    assert!(inventory::native(&add, &mut vm.world).is_some());
    let r = inventory::item_added(&mut vm, &s, h, other, 1, None, &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    let all = NativeCall {
        args: Vec::new(),
        name: "removeallinventoryeventfilters".into(),
        ..add.clone()
    };
    assert!(inventory::native(&all, &mut vm.world).is_some());
    let r = inventory::item_added(&mut vm, &s, h, item, 1, None, &b).unwrap();
    assert!(r.runs.is_empty(), "every filter of the instance removed");
    // The count: raised only from a known count, under its switch.
    let hk = vm.world.objects[h].key.clone().unwrap();
    let ik = vm.world.objects[item].key.clone().unwrap();
    vm.world
        .runstate
        .item_counts
        .insert((hk.clone(), ik.clone()), 0);
    inventory::item_added(&mut vm, &s, h, item, 1, None, &b).unwrap();
    assert_eq!(vm.world.runstate.item_counts[&(hk.clone(), ik.clone())], 0);
    vm.world.runstate.inventory.counts_added = true;
    inventory::item_added(&mut vm, &s, h, item, 1, None, &b).unwrap();
    assert_eq!(vm.world.runstate.item_counts[&(hk, ik)], 1);
    assert!(vm.rules.contains(rules::ITEM_ADDED_COUNT));
}

#[test]
fn perks_factions_and_actor_values_answer_from_the_base_and_the_caller_only_under_their_switches() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let w = &mut vm.world;
    let player = w.player("actor");
    w.player_base_object = Some(key(NPC));
    let a = crate::data::record_object(w, &s, &key(ACTOR), "actor", &b).unwrap();
    let o = |w: &mut crate::World, id: u32, ty: &str| {
        crate::data::record_object(w, &s, &key(id), ty, &b).unwrap()
    };
    let (p1, p2, f1, f2) = (
        o(w, PERK, "perk"),
        o(w, PERK2, "perk"),
        o(w, FACT, "faction"),
        o(w, FACT2, "faction"),
    );
    let has = |p| {
        call(
            "actor",
            "hasperk",
            Val::Object(player),
            vec![Val::Object(p)],
            "bool",
        )
    };
    let member = |who, f| {
        call(
            "actor",
            "isinfaction",
            Val::Object(who),
            vec![Val::Object(f)],
            "bool",
        )
    };
    let yes = |r: Option<Reply>| matches!(r, Some(Reply::Value(Val::Bool(true))));
    let no = |r: Option<Reply>| matches!(r, Some(Reply::Value(Val::Bool(false))));
    // Off: left to the other hosts.
    assert!(runstate::native(&s, 1 << 24, &has(p1), w).is_none());
    assert!(runstate::native(&s, 1 << 24, &member(a, f1), w).is_none());
    w.runstate.perks = true;
    assert!(yes(runstate::native(&s, 1 << 24, &has(p1), w)));
    assert!(no(runstate::native(&s, 1 << 24, &has(p2), w)));
    let add = call(
        "actor",
        "addperk",
        Val::Object(player),
        vec![Val::Object(p2), Val::Bool(false)],
        "none",
    );
    runstate::native(&s, 1 << 24, &add, w);
    assert!(yes(runstate::native(&s, 1 << 24, &has(p2), w)));
    // Not the player: not answered.
    let other = call(
        "actor",
        "hasperk",
        Val::Object(a),
        vec![Val::Object(p1)],
        "bool",
    );
    assert!(runstate::native(&s, 1 << 24, &other, w).is_none());
    w.runstate.factions = true;
    // Rank 0 is a member, rank -1 is not, on the placed actor's base and
    // the player's.
    assert!(yes(runstate::native(&s, 1 << 24, &member(a, f1), w)));
    assert!(no(runstate::native(&s, 1 << 24, &member(a, f2), w)));
    assert!(yes(runstate::native(&s, 1 << 24, &member(player, f1), w)));
    let join = call(
        "actor",
        "setfactionrank",
        Val::Object(a),
        vec![Val::Object(f2), Val::Int(0)],
        "none",
    );
    runstate::native(&s, 1 << 24, &join, w);
    assert!(yes(runstate::native(&s, 1 << 24, &member(a, f2), w)));
    let leave = call(
        "actor",
        "removefromallfactions",
        Val::Object(a),
        vec![],
        "none",
    );
    runstate::native(&s, 1 << 24, &leave, w);
    assert!(no(runstate::native(&s, 1 << 24, &member(a, f1), w)));
    // The caller's actor value, for that holder and value only.
    let av = o(w, PERK, "actorvalue");
    let get = |who| {
        call(
            "objectreference",
            "getvalue",
            Val::Object(who),
            vec![Val::Object(av)],
            "float",
        )
    };
    assert!(runstate::native(&s, 1 << 24, &get(player), w).is_none());
    let k = w.objects[av].key.clone().unwrap();
    w.runstate.actor_values.insert(("player".into(), k), 9.0);
    assert!(matches!(
        runstate::native(&s, 1 << 24, &get(player), w),
        Some(Reply::Value(Val::Float(x))) if x == 9.0
    ));
    assert!(runstate::native(&s, 1 << 24, &get(a), w).is_none());
    let used = std::mem::take(&mut w.runstate.used);
    assert!(used.contains(&rules::PERK_BASE_LIST));
    assert!(used.contains(&rules::FACTION_BASE_LIST));
}

#[test]
fn every_rule_past_the_museum_is_registered_pending() {
    for r in [
        rules::TERMINAL_FRAGMENT,
        rules::LOCK_STATE_EVENT_SCRIPT,
        rules::ITEM_ADDED_EVENT,
        rules::INVENTORY_FILTER_INIT,
        rules::ITEM_ADDED_COUNT,
        rules::PERK_BASE_LIST,
        rules::FACTION_BASE_LIST,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

/// QS starts with the game and holds stages 10 and 20.
fn game_plugin() -> Vec<u8> {
    game_plugin_with(&[10, 20])
}

/// QS starts with the game and holds these stages.
fn game_plugin_with(stages: &[u16]) -> Vec<u8> {
    game_plugin_full(stages, &[])
}

/// [`game_plugin_with`] with these records among the room's temporary
/// references.
fn game_plugin_full(stages: &[u16], refs: &[u8]) -> Vec<u8> {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(2u32.to_le_bytes());
    hedr.extend(0x990u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    let mut dnam = crate::quest::START_GAME_ENABLED.to_le_bytes().to_vec();
    dnam.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let stage = |n: u16| {
        let mut indx = n.to_le_bytes().to_vec();
        indx.extend([0, 0]);
        [sub(b"INDX", &indx), sub(b"QSDT", &[0])].concat()
    };
    let mut qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"DNAM", &dnam),
        sub(b"NEXT", &[]),
    ]
    .concat();
    for n in stages {
        qs.extend(stage(*n));
    }
    plugin.extend(group(0, *b"QUST", &record(b"QUST", 0x900, &qs)));
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    plugin.extend(group(
        0,
        *b"CELL",
        &group(
            2,
            [0; 4],
            &[
                cell,
                group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), refs)),
            ]
            .concat(),
        ),
    ));
    plugin
}

/// Steps the run: begins the input, gives two slices and ends it, noting
/// the second and the stages done after each.
struct Steps {
    seen: Vec<(f64, Option<bool>, Option<bool>, bool)>,
}
impl Steps {
    fn note(&mut self, run: &NewGame<'_, '_>) {
        let s = run.vm().world.quests.states.get("test.esm:00000900");
        self.seen.push((
            run.now(),
            s.and_then(|s| s.is_done(10)),
            s.and_then(|s| s.is_done(20)),
            run.ended(),
        ));
    }
}
impl Stepper for Steps {
    fn step(&mut self, run: &mut NewGame<'_, '_>) -> Result<(), Box<dyn std::error::Error>> {
        run.begin_input(Stepping::default())?;
        self.note(run);
        let p = Point {
            t: 3.0,
            space: key(CELL),
            pos: [30.0, 0.0, 0.0],
            facing: None,
        };
        run.advance_to(3.0, &[p], &[])?;
        self.note(run);
        run.advance_to(6.0, &[], &[])?;
        self.note(run);
        run.finish_input()?;
        self.note(run);
        Ok(())
    }
}

#[test]
fn the_new_game_object_advances_slice_by_slice_over_a_synthetic_manifest() {
    let dir = TestDir::new("vm-newgame");
    std::fs::write(dir.join("test.esm"), game_plugin()).unwrap();
    let manifest = format!(
        "{{\"game\":\"fallout4\",\"root\":{:?},\"language\":\"english\",\"build\":\"synthetic\",\"plugins\":[{{\"name\":\"test.esm\",\"path\":\"test.esm\"}}],\"required_masters\":[],\"mounts\":[],\"excluded\":[]}}",
        dir.to_str().unwrap()
    );
    std::fs::write(dir.join("m.json"), manifest).unwrap();
    std::fs::write(
        dir.join("t.track"),
        "0 test.esm:00000930 0 0 0\n9 test.esm:00000930 90 0 0\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let arg = |p: &str| dir.join(p).to_str().unwrap().to_string();
    let args: Vec<String> = vec![
        arg("m.json"),
        "new-game".into(),
        arg("out"),
        "--scene-clock".into(),
        "10".into(),
        "--no-line-audio".into(),
        "--track".into(),
        arg("t.track"),
        "--then".into(),
        "QS:10@5".into(),
        "--then".into(),
        "QS:20@8".into(),
    ];
    let none = |_: &[u8], _: &str, _: &Budget| None;
    let mut steps = Steps { seen: Vec::new() };
    crate::run::main_with(args, &none, Some(&mut steps)).unwrap();
    // The start: the quest runs, nothing done. Slice to 3 s: nothing is
    // due, the run's clock stays at 0. Slice to 6 s: the caller's stage at
    // 5 s is done (the clock at 5 s), the one at 8 s, past the input, not.
    // The input over: the run goes on to the end, 20 done at 8 s.
    assert_eq!(
        steps.seen,
        vec![
            (0.0, Some(false), Some(false), false),
            (0.0, Some(false), Some(false), false),
            (5.0, Some(true), Some(false), false),
            (8.0, Some(true), Some(true), true),
        ]
    );
    // The library wrote the run's outputs after the stepper returned.
    let census = std::fs::read_to_string(dir.join("out/quest-run-census.txt")).unwrap();
    assert!(census.contains("a stage set from outside any script at a second"));
}

/// Runs `quest_run new-game` over the synthetic manifest with QS's stages
/// 5, 10, 20 and 30 and these switches; the second each stage was set at,
/// read from the names file's timeline.
fn gate_run(name: &str, more: &[&str]) -> std::collections::BTreeMap<i32, f64> {
    let dir = TestDir::new(name);
    std::fs::write(dir.join("test.esm"), game_plugin_with(&[5, 10, 20, 30])).unwrap();
    let manifest = format!(
        "{{\"game\":\"fallout4\",\"root\":{:?},\"language\":\"english\",\"build\":\"synthetic\",\"plugins\":[{{\"name\":\"test.esm\",\"path\":\"test.esm\"}}],\"required_masters\":[],\"mounts\":[],\"excluded\":[]}}",
        dir.to_str().unwrap()
    );
    std::fs::write(dir.join("m.json"), manifest).unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let arg = |p: &str| dir.join(p).to_str().unwrap().to_string();
    let mut args: Vec<String> = vec![
        arg("m.json"),
        "new-game".into(),
        arg("out"),
        "--scene-clock".into(),
        "10".into(),
        "--no-line-audio".into(),
    ];
    args.extend(more.iter().map(|x| x.to_string()));
    let none = |_: &[u8], _: &str, _: &Budget| None;
    crate::run::main_with(args, &none, None).unwrap();
    let names = std::fs::read_to_string(dir.join("out/quest-run-names.txt")).unwrap();
    let mut at = std::collections::BTreeMap::new();
    let mut now = None;
    for l in names.lines() {
        if let Some(t) = l
            .strip_prefix("## timeline at ")
            .and_then(|r| r.split(' ').next())
        {
            now = t.parse::<f64>().ok();
        } else if let (Some(t), Some(n)) = (
            now,
            l.split("StageSet(")
                .nth(1)
                .and_then(|r| r.split(')').next()),
        ) {
            at.entry(n.parse::<i32>().unwrap()).or_insert(t);
        }
    }
    at
}

/// A late chain: 5 comes at 30 s (the caller's late action), 10 was written
/// at 10 s and is held until 5, 20 and 30 were written 5 and 8 s after 10
/// and are held until 10.
const GATES: [&str; 18] = [
    "--then",
    "QS:5@30",
    "--then",
    "QS:10@10",
    "--action-after",
    "QS:10@10",
    "QS:5",
    "--then",
    "QS:20@15",
    "--action-after",
    "QS:20@15",
    "QS:10",
    "--then",
    "QS:30@18",
    "--action-after",
    "QS:30@18",
    "QS:10",
    "--timers",
];

#[test]
fn held_actions_keep_their_offset_from_their_gate_only_under_the_switch() {
    let base: Vec<&str> = GATES.iter().copied().chain(["0"]).collect();
    // Off: every held action happens at the second its gate is done.
    let off = gate_run("vm-gate-off", &base);
    assert_eq!(off.get(&5), Some(&30.0));
    assert_eq!(off.get(&10), Some(&30.0));
    assert_eq!(off.get(&20), Some(&30.0));
    assert_eq!(off.get(&30), Some(&30.0));
    // On: 10's gate (5) was done by an action written at 30 s, so 10 keeps
    // no offset (written before it) and comes at 30; 10 was done by the
    // action written at 10 s, so 20 and 30 come 5 and 8 s after it.
    let mut on = base.clone();
    on.push("--action-after-offset");
    let on = gate_run("vm-gate-on", &on);
    assert_eq!(on.get(&5), Some(&30.0));
    assert_eq!(on.get(&10), Some(&30.0));
    assert_eq!(on.get(&20), Some(&35.0));
    assert_eq!(on.get(&30), Some(&38.0));
}

#[test]
fn a_gate_without_a_written_second_keeps_the_spacing_from_its_earliest_held_action() {
    use crate::run::{gate_reference, held_due, waiting_due};
    // The opening's exit: the console's use written at 870 s sets 7 at
    // 1071.6 s; the two events held on 7 were written at 880 and 885 s.
    let r = gate_reference(Some(870.0), 880.0);
    assert_eq!(held_due(1071.6, 880.0, r), 1081.6);
    assert_eq!(held_due(1071.6, 885.0, r), 1086.6);
    // A stage the run sets itself: the earliest held action comes at the
    // stage's second, the others keep their spacing from it.
    let r = gate_reference(None, 737.5);
    assert_eq!(held_due(930.0, 737.5, r), 930.0);
    assert_eq!(held_due(930.0, 741.5, r), 934.0);
    // Never before the stage; a waiting action never before its second.
    assert_eq!(held_due(100.0, 50.0, 60.0), 100.0);
    assert_eq!(waiting_due(100.0, 120.0, 110.0), 120.0);
    assert_eq!(waiting_due(100.0, 120.0, 90.0), 130.0);
}

#[test]
fn the_offset_switch_and_its_input_seconds_are_checked() {
    let parse = |extra: &[&str]| {
        let mut a: Vec<String> = ["m", "new-game", "o", "--scene-clock", "5"]
            .iter()
            .map(|x| x.to_string())
            .collect();
        a.extend(extra.iter().map(|x| x.to_string()));
        crate::run::parse(a).map(|c| (c.then.action_offset, c.then.input_written))
    };
    assert!(parse(&["--input-action-at", "activate", "a.esm:1@3"]).is_err());
    assert!(parse(&[
        "--action-after-offset",
        "--input-action-at",
        "use",
        "a.esm:1@3"
    ])
    .is_err());
    let (on, w) = parse(&[
        "--action-after-offset",
        "--input-action-at",
        "activate",
        "A.ESM:0001@870",
        "--input-action-at",
        "open",
        "a.esm:2@90",
    ])
    .unwrap();
    assert!(on);
    assert_eq!(
        w,
        vec![
            (
                crate::player::Action::Activate,
                "a.esm:00000001".to_string(),
                870.0
            ),
            (
                crate::player::Action::Open,
                "a.esm:00000002".to_string(),
                90.0
            ),
        ]
    );
    assert!(rules::ALL.contains(&rules::ACTION_AFTER_OFFSET));
    assert_eq!(rules::status(rules::ACTION_AFTER_OFFSET), Some("pending"));
}

/// Runs `quest_run new-game` over a room holding one placed actor (at
/// 0, 0, 0) on the player's track, with these switches; the seconds of the
/// actor's moves in the timeline, and each stage's second.
fn actor_gate_run(name: &str, more: &[&str]) -> (Vec<f64>, std::collections::BTreeMap<i32, f64>) {
    let dir = TestDir::new(name);
    let mut d = Vec::new();
    for f in [0.0f32; 6] {
        d.extend(f.to_le_bytes());
    }
    let achr = record(
        b"ACHR",
        ACTOR,
        &[sub(b"NAME", &NPC.to_le_bytes()), sub(b"DATA", &d)].concat(),
    );
    std::fs::write(dir.join("test.esm"), game_plugin_full(&[5, 10], &achr)).unwrap();
    let manifest = format!(
        "{{\"game\":\"fallout4\",\"root\":{:?},\"language\":\"english\",\"build\":\"synthetic\",\"plugins\":[{{\"name\":\"test.esm\",\"path\":\"test.esm\"}}],\"required_masters\":[],\"mounts\":[],\"excluded\":[]}}",
        dir.to_str().unwrap()
    );
    std::fs::write(dir.join("m.json"), manifest).unwrap();
    std::fs::write(
        dir.join("t.track"),
        "0 test.esm:00000930 0 0 0\n60 test.esm:00000930 90 0 0\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let arg = |p: &str| dir.join(p).to_str().unwrap().to_string();
    let mut args: Vec<String> = vec![
        arg("m.json"),
        "new-game".into(),
        arg("out"),
        "--scene-clock".into(),
        "10".into(),
        "--no-line-audio".into(),
        "--timers".into(),
        "0".into(),
        "--track".into(),
        arg("t.track"),
        "--placed-positions".into(),
        "--actor-positions".into(),
        "--timeline".into(),
        arg("t.timeline"),
    ];
    args.extend(more.iter().map(|x| x.to_string()));
    let none = |_: &[u8], _: &str, _: &Budget| None;
    crate::run::main_with(args, &none, None).unwrap();
    let timeline = std::fs::read_to_string(dir.join("t.timeline")).unwrap();
    let actor = "test.esm:00000965";
    let moves = timeline
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.first() == Some(&"actor") && f.get(2) == Some(&actor) && f.get(1) != Some(&"0"))
                .then(|| f[1].parse::<f64>().unwrap())
        })
        .collect();
    let names = std::fs::read_to_string(dir.join("out/quest-run-names.txt")).unwrap();
    let mut at = std::collections::BTreeMap::new();
    let mut now = None;
    for l in names.lines() {
        if let Some(t) = l
            .strip_prefix("## timeline at ")
            .and_then(|r| r.split(' ').next())
        {
            now = t.parse::<f64>().ok();
        } else if let (Some(t), Some(n)) = (
            now,
            l.split("StageSet(")
                .nth(1)
                .and_then(|r| r.split(')').next()),
        ) {
            at.entry(n.parse::<i32>().unwrap()).or_insert(t);
        }
    }
    (moves, at)
}

/// A person late at the stage that sends an actor, as in both play
/// sessions of the deck step: 5 (the stage whose fragment would move the
/// actor) was written at 8 s and is held until 10, which the person sets
/// at 30 s; the caller wrote the actor's arrival at 12 s, 4 s after 5.
const LATE_SENDER: [&str; 14] = [
    "--then",
    "QS:5@8",
    "--action-after",
    "QS:5@8",
    "QS:10",
    "--then",
    "QS:10@30",
    "--assume-actor-at",
    "test.esm:00000965",
    "test.esm:00000930",
    "500",
    "0",
    "0@12",
    "--action-after",
];

#[test]
fn an_assumed_actor_move_waits_for_the_stage_that_sends_the_actor_only_under_the_switch() {
    let base: Vec<&str> = LATE_SENDER
        .iter()
        .copied()
        .chain(["test.esm:00000965@12", "QS:5"])
        .collect();
    // Off: the move happens at its written second, 18 s before the stage
    // that sends the actor (a later native move of that stage would put
    // the actor back where it sends it, and the actor never arrives).
    let (moves, at) = actor_gate_run("vm-move-gate-off", &base);
    assert_eq!(at.get(&5), Some(&30.0));
    assert_eq!(moves, vec![12.0]);
    // On: the move waits for 5 and happens when it is done.
    let mut on = base.clone();
    on.push("--action-after-actor-moves");
    let (moves, at) = actor_gate_run("vm-move-gate-on", &on);
    assert_eq!(at.get(&5), Some(&30.0));
    assert_eq!(moves, vec![30.0]);
    // With the offset: 5 was done in the turn of its action written at
    // 8 s, so the move keeps its 4 s after it.
    on.push("--action-after-offset");
    let (moves, at) = actor_gate_run("vm-move-gate-offset", &on);
    assert_eq!(at.get(&5), Some(&30.0));
    assert_eq!(moves, vec![34.0]);
    // On time: 10 at 4 s, 5 at its 8 s, the move at its 12 s as written.
    let mut early: Vec<&str> = on.clone();
    let i = early.iter().position(|x| *x == "QS:10@30").unwrap();
    early[i] = "QS:10@4";
    let (moves, at) = actor_gate_run("vm-move-gate-early", &early);
    assert_eq!(at.get(&5), Some(&8.0));
    assert_eq!(moves, vec![12.0]);
    assert!(rules::ALL.contains(&rules::ACTOR_MOVE_AFTER));
    assert_eq!(rules::status(rules::ACTOR_MOVE_AFTER), Some("pending"));
}
