//! The fight step's readings over synthetic plugins and scripts (no game
//! data): aliases sharing a location's ref type, hit registrations and the
//! init reading, combat state changes, deaths and the natives that read
//! them back.
use super::assemble;
use super::quest::{group, key, record, sub, Owned};
use crate::{
    fight,
    quest::{self, QuestHost},
    rules, NativeCall, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;

const QS: u32 = 0x900;
const CELL: u32 = 0x930;
const LOC: u32 = 0x940;
const LT: u32 = 0x941;
const NPC: u32 = 0x951;
const A1: u32 = 0x960;
const A2: u32 = 0x961;
const A3: u32 = 0x962;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A quest whose aliases 0 to 2 name one location alias and one ref type,
/// the location's rows naming three distinct placed actors of the type (and
/// one of them twice); alias 3 names the type alone.
fn store() -> Store {
    let mut qs = vec![sub(b"EDID", b"QS\0"), sub(b"ANAM", &w(5))];
    for id in 0..3u32 {
        qs.extend([
            sub(b"ALST", &w(id)),
            sub(b"ALID", b"A\0"),
            sub(b"FNAM", &w(0)),
            sub(b"ALFA", &w(4)),
            sub(b"ALRT", &w(LT)),
            sub(b"ALED", &[]),
        ]);
    }
    qs.extend([
        sub(b"ALLS", &w(4)),
        sub(b"ALID", b"L\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFL", &w(LOC)),
        sub(b"ALED", &[]),
    ]);
    let qs = qs.concat();
    let refs = [A1, A2, A3]
        .iter()
        .map(|a| record(b"ACHR", *a, &sub(b"NAME", &w(NPC))))
        .collect::<Vec<_>>()
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
    let row = |r: u32| {
        let mut x = w(LT);
        x.extend(w(r));
        x.extend(w(CELL));
        x.extend(w(0));
        x
    };
    let lcsr = [row(A2), row(A1), row(A2), row(A3)].concat();
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
        &record(b"LCRT", LT, &sub(b"EDID", b"LT\0")),
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
        *b"NPC_",
        &record(b"NPC_", NPC, &sub(b"EDID", b"Fly\0")),
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
fn aliases_sharing_a_ref_type_take_its_distinct_references_in_order_only_under_their_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let i = quest::QuestInfo::load(&s, &key(QS), &b).unwrap().unwrap();
    assert_eq!(
        i.location_rows[&0].distinct(),
        [&key(A2), &key(A1), &key(A3)]
    );
    let mut q = quest::Quests {
        location_fills: true,
        ..Default::default()
    };
    for id in 0..3 {
        assert!(q.fill_location(&i, id).is_err(), "several rows: open");
    }
    // The distinct-rows reading alone does not settle three references.
    q.location_distinct = true;
    assert!(q.fill_location(&i, 0).is_err());
    q.location_shared = true;
    assert_eq!(q.fill_location(&i, 0), Ok(Some(key(A2))));
    assert_eq!(q.fill_location(&i, 1), Ok(Some(key(A1))));
    assert_eq!(q.fill_location(&i, 2), Ok(Some(key(A3))));
    assert!(q
        .location_used
        .borrow()
        .contains(rules::ALIAS_LOCATION_SHARED));
    // Without the list's reading nothing is filled.
    q.location_fills = false;
    assert_eq!(q.fill_location(&i, 0), Ok(None));
}

fn lib() -> crate::MapLibrary {
    let mut l = super::quest::library();
    l.add(assemble(
        "script ScriptObject
         func OnInit none
         func RegisterForHitEvent none native ScriptObject akTarget Form akAggressorFilter Form akSourceFilter Form akProjectileFilter Int aiPowerFilter Int aiSneakFilter Int aiBashFilter Int aiBlockFilter Bool abMatch
         func UnregisterForAllHitEvents none native ScriptObject akTarget",
    ));
    l.add(assemble(
        "script Actor extends ObjectReference
         func IsDead Bool native
         func IsInCombat Bool native
         func GetCombatState Int native",
    ));
    // Registers for its own hits in OnInit (never delivered by a run) and
    // again in OnHit; counts what it receives.
    l.add(assemble(
        "script Watcher extends Actor
         var Int hits 0
         var ObjectReference by
         var Int state 0
         var ObjectReference toward
         var Int dying 0
         var Int deaths 0
         var Bool dead false
         func OnInit none
         local ScriptObject ::me
         local none ::n
         14 ::me self
         23 RegisterForHitEvent self ::n 9 ::me none none none -1 -1 -1 -1 true
         func OnHit none ObjectReference akTarget ObjectReference akAggressor Form akSource Projectile akProjectile Bool abPower Bool abSneak Bool abBash Bool abBlocked String asMaterial
         local ScriptObject ::me
         local none ::n
         1 hits hits 1
         13 by akAggressor
         14 ::me self
         23 RegisterForHitEvent self ::n 9 ::me none none none -1 -1 -1 -1 true
         func OnCombatStateChanged none Actor akTarget Int aeCombatState
         13 state aeCombatState
         13 toward akTarget
         func OnDying none Actor akKiller
         1 dying dying 1
         func OnDeath none Actor akKiller
         1 deaths deaths 1
         23 IsDead self dead 0",
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

fn native(script: &str, name: &str, receiver: usize) -> NativeCall {
    NativeCall {
        script: script.into(),
        name: name.into(),
        receiver: Some(Val::Object(receiver)),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "bool".into(),
    }
}

#[test]
fn a_hit_reaches_registered_instances_once_and_init_registrations_only_under_their_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let a = crate::data::record_object(&mut vm.world, &s, &key(A1), "actor", &b).unwrap();
    vm.world.objects[a].attached = true;
    let me = vm.instantiate(Some(a), "watcher").unwrap();
    let player = vm.world.player("actor");
    vm.world.fights.hits = true;
    // No registration was made (the init handler never ran): nothing.
    let r = fight::hit(&mut vm, &s, a, player, &b).unwrap();
    assert!(r.runs.is_empty());
    assert_eq!(var(&vm, me, "hits"), Val::Int(0));
    // Under the init reading the init handler's registration counts once.
    vm.world.fights.init_read.clear();
    vm.world.fights.init_registrations = true;
    let r = fight::hit(&mut vm, &s, a, player, &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    assert_eq!(var(&vm, me, "hits"), Val::Int(1));
    assert_eq!(var(&vm, me, "by"), Val::Object(player));
    assert!(vm.rules.contains(rules::HIT_INIT_REGISTRATION));
    // The handler registered again (a native the host kept): the next hit
    // reaches it through that registration.
    assert_eq!(vm.world.fights.registrations.len(), 1);
    assert!(!vm.world.fights.registrations[0].from_init);
    fight::hit(&mut vm, &s, a, player, &b).unwrap();
    assert_eq!(var(&vm, me, "hits"), Val::Int(2));
    // Removed, it receives nothing.
    let mut un = native("scriptobject", "unregisterforallhitevents", a);
    un.instance = Some(me);
    un.args = vec![Val::None];
    un.return_type = "none".into();
    assert!(fight::native(&un, &mut vm.world).is_some());
    assert!(vm.world.fights.registrations.is_empty());
    fight::hit(&mut vm, &s, a, player, &b).unwrap();
    assert_eq!(var(&vm, me, "hits"), Val::Int(2));
}

#[test]
fn combat_states_and_deaths_reach_the_actor_and_the_natives_read_them_each_under_its_switch() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let a = crate::data::record_object(&mut vm.world, &s, &key(A1), "actor", &b).unwrap();
    vm.world.objects[a].attached = true;
    let me = vm.instantiate(Some(a), "watcher").unwrap();
    let player = vm.world.player("actor");
    // Off: the natives are left to the host.
    let in_combat = native("actor", "isincombat", player);
    assert!(fight::native(&in_combat, &mut vm.world).is_none());
    vm.world.fights.combat_natives = true;
    assert!(matches!(
        fight::native(&in_combat, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(false)))
    ));
    assert!(
        fight::native(&native("actor", "getcombatstate", a), &mut vm.world).is_none(),
        "an actor the run gave no state is left open"
    );
    fight::combat_state(&mut vm, &s, a, player, 1, &b).unwrap();
    assert_eq!(var(&vm, me, "state"), Val::Int(1));
    assert_eq!(var(&vm, me, "toward"), Val::Object(player));
    assert!(matches!(
        fight::native(&in_combat, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(true)))
    ));
    assert!(matches!(
        fight::native(&native("actor", "getcombatstate", a), &mut vm.world),
        Some(crate::Reply::Value(Val::Int(1)))
    ));
    // A death: dying then death; IsDead answers only with its switch.
    fight::death(&mut vm, &s, a, player, &b).unwrap();
    assert_eq!(var(&vm, me, "dying"), Val::Int(1));
    assert_eq!(var(&vm, me, "deaths"), Val::Int(1));
    assert_ne!(var(&vm, me, "dead"), Val::Bool(true));
    vm.world.fights.dead_state = true;
    assert!(matches!(
        fight::native(&native("actor", "isdead", a), &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(true)))
    ));
    // The dead actor fights no more.
    assert!(matches!(
        fight::native(&in_combat, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(false)))
    ));
    for r in [rules::COMBAT_STATE_EVENTS, rules::DEATH_EVENTS] {
        assert!(vm.rules.contains(r), "{r}");
    }
}

#[test]
fn every_rule_of_the_fight_step_is_registered_pending() {
    for r in [
        rules::ALIAS_LOCATION_SHARED,
        rules::HIT_EVENTS,
        rules::HIT_INIT_REGISTRATION,
        rules::COMBAT_STATE_EVENTS,
        rules::DEATH_EVENTS,
        rules::DEAD_STATE,
        rules::COMBAT_STATE_NATIVES,
        rules::AUTO_STATE_INHERITED,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn an_instance_starts_in_an_ancestors_auto_state_only_under_its_rule() {
    let s = store();
    let mut l = lib();
    l.add(assemble(
        "script Waiter extends ObjectReference auto Waiting
         var Int deaths 0
         state Waiting
         func OnDeath none Actor akKiller
         1 deaths deaths 1",
    ));
    l.add(assemble("script Child extends Waiter"));
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let i = vm.instantiate(None, "child").unwrap();
    assert_eq!(vm.world.instances[i].state, "");
    vm.auto_state_inherited = true;
    let j = vm.instantiate(None, "child").unwrap();
    assert_eq!(vm.world.instances[j].state, "waiting");
    assert!(vm.rules.contains(rules::AUTO_STATE_INHERITED));
    // A script declaring its own auto state keeps it.
    let k = vm.instantiate(None, "waiter").unwrap();
    assert_eq!(vm.world.instances[k].state, "waiting");
}
