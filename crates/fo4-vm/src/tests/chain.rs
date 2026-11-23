//! The town chain's step over synthetic plugins and scripts (no game
//! data): a quest record whose winner carries flag bit 0x4000 and only
//! its script read from the occurrence before it, a container change and a
//! location change delivered from the caller, `Location.IsChild` from the
//! locations' parents, and the timer budget counted per timer.
use super::assemble;
use super::quest::{group, key, library, record, sub, text};
use crate::{
    chain,
    quest::{self, QuestHost, Scenario},
    rules,
    world::{Clock, Timer},
    NativeCall, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;

const QA: u32 = 0x900;
const QB: u32 = 0x910;
const QC: u32 = 0x911;
const ITEM_BASE: u32 = 0x920;
const NPC: u32 = 0x921;
const CELL: u32 = 0x930;
const ITEM: u32 = 0x931;
const WALKER: u32 = 0x932;
const L1: u32 = 0x940;
const L2: u32 = 0x941;
const L3: u32 = 0x942;

/// A plugin's bytes under a name of its own.
struct Named(&'static str, Vec<u8>);
impl ReadAt for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn len(&self) -> u64 {
        self.1.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> fo4_core::Result<()> {
        fo4_core::SliceSource {
            name: self.0,
            bytes: &self.1,
        }
        .read_exact_at(o, out)
    }
}

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn dnam(flags: u16) -> Vec<u8> {
    let mut d = flags.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    sub(b"DNAM", &d)
}

fn stage(n: u16, flags: u8) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([flags, 0]);
    [sub(b"INDX", &d), sub(b"QSDT", &[0])].concat()
}

/// A quest VMAD: no scripts of its own, a fragment block of `script` with
/// one fragment for stage 20.
fn quest_vmad(script: &str) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0];
    v.extend(0u16.to_le_bytes());
    v.push(3);
    v.extend(1u16.to_le_bytes());
    text(&mut v, script);
    v.push(0);
    v.extend(0u16.to_le_bytes());
    v.extend(20u16.to_le_bytes());
    v.extend([0, 0]);
    v.extend(0i32.to_le_bytes());
    v.push(1);
    text(&mut v, script);
    text(&mut v, "Fragment_Stage_0020_Item_00");
    v.extend(0u16.to_le_bytes());
    v
}

/// A VMAD attaching one script with no properties.
fn own_vmad(script: &str) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0];
    v.extend(1u16.to_le_bytes());
    text(&mut v, script);
    v.push(0);
    v.extend(0u16.to_le_bytes());
    v
}

fn tes4(masters: &[&str]) -> Vec<u8> {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(20u32.to_le_bytes());
    hedr.extend(0x990u32.to_le_bytes());
    let mut data = sub(b"HEDR", &hedr);
    for m in masters {
        let mut n = m.as_bytes().to_vec();
        n.push(0);
        data.extend(sub(b"MAST", &n));
        data.extend(sub(b"DATA", &[0; 8]));
    }
    let mut r = record(b"TES4", 0, &data);
    r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
    r
}

fn flagged(mut r: Vec<u8>, flags: u32) -> Vec<u8> {
    r[8..12].copy_from_slice(&flags.to_le_bytes());
    r
}

/// test.esm: QA with stages 10 (startup) and 20 and a fragment script,
/// QB with stage 5; an item and an actor placed in a cell; three locations
/// L3 -> L2 -> L1 by their parents. patch.esm: QA again with flag 0x4000,
/// only EDID and another fragment script; QB again with flag 0x4000, EDID
/// and DNAM.
fn store() -> Store {
    let mut master = tes4(&[]);
    let qa = [
        sub(b"EDID", b"QA\0"),
        sub(b"VMAD", &quest_vmad("QFOld")),
        dnam(0),
        sub(b"NEXT", &[]),
        stage(10, 0x02),
        stage(20, 0),
    ]
    .concat();
    let qb = [
        sub(b"EDID", b"QB\0"),
        dnam(0),
        sub(b"NEXT", &[]),
        stage(5, 0),
    ]
    .concat();
    let qc = [
        sub(b"EDID", b"QC\0"),
        sub(b"VMAD", &quest_vmad("QFKept")),
        dnam(0),
        sub(b"NEXT", &[]),
        stage(20, 0),
    ]
    .concat();
    master.extend(group(
        0,
        *b"QUST",
        &[
            record(b"QUST", QA, &qa),
            record(b"QUST", QB, &qb),
            record(b"QUST", QC, &qc),
        ]
        .concat(),
    ));
    master.extend(group(
        0,
        *b"MISC",
        &record(b"MISC", ITEM_BASE, &sub(b"EDID", b"Thing\0")),
    ));
    master.extend(group(
        0,
        *b"NPC_",
        &record(b"NPC_", NPC, &sub(b"EDID", b"Walker\0")),
    ));
    master.extend(group(
        0,
        *b"LCTN",
        &[
            record(b"LCTN", L1, &sub(b"EDID", b"Town\0")),
            record(
                b"LCTN",
                L2,
                &[sub(b"EDID", b"Museum\0"), sub(b"PNAM", &w(L1))].concat(),
            ),
            record(
                b"LCTN",
                L3,
                &[sub(b"EDID", b"Hall\0"), sub(b"PNAM", &w(L2))].concat(),
            ),
        ]
        .concat(),
    ));
    let data = {
        let mut d = Vec::new();
        for v in [0f32; 6] {
            d.extend(v.to_le_bytes());
        }
        d
    };
    let refs = [
        record(
            b"REFR",
            ITEM,
            &[
                sub(b"VMAD", &own_vmad("ItemWatch")),
                sub(b"NAME", &w(ITEM_BASE)),
                sub(b"DATA", &data),
            ]
            .concat(),
        ),
        record(
            b"ACHR",
            WALKER,
            &[
                sub(b"VMAD", &own_vmad("PlaceWatch")),
                sub(b"NAME", &w(NPC)),
                sub(b"DATA", &data),
            ]
            .concat(),
        ),
    ]
    .concat();
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    master.extend(group(
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
    let mut patch = tes4(&["test.esm"]);
    patch.extend(group(
        0,
        *b"QUST",
        &[
            flagged(
                record(
                    b"QUST",
                    QA,
                    &[sub(b"EDID", b"QA\0"), sub(b"VMAD", &quest_vmad("QFNew"))].concat(),
                ),
                0x4000,
            ),
            flagged(
                record(b"QUST", QB, &[sub(b"EDID", b"QB\0"), dnam(0)].concat()),
                0x4000,
            ),
            flagged(record(b"QUST", QC, &sub(b"EDID", b"QC\0")), 0x4000),
        ]
        .concat(),
    ));
    let sources: Vec<(String, Box<dyn ReadAt>)> = vec![
        ("test.esm".into(), Box::new(Named("test.esm", master))),
        ("patch.esm".into(), Box::new(Named("patch.esm", patch))),
    ];
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
    l.add(assemble("script Location extends Form"));
    l.add(assemble(
        "script Actor extends ObjectReference
         func IsDead Bool native",
    ));
    // Counts the container changes it receives and keeps the new holder.
    l.add(assemble(
        "script ItemWatch extends ObjectReference
         var Int moves 0
         var ObjectReference holder
         func OnContainerChanged none ObjectReference akNewContainer ObjectReference akOldContainer
         1 moves moves 1
         13 holder akNewContainer",
    ));
    // Counts the location changes it receives and keeps the new location.
    l.add(assemble(
        "script PlaceWatch extends Actor
         var Int changes 0
         var Location now
         func OnLocationChange none Location akOldLoc Location akNewLoc
         1 changes changes 1
         13 now akNewLoc",
    ));
    l
}

fn var(vm: &Vm<'_>, o: crate::ObjId, n: &str) -> Val {
    let i = vm.world.objects[o].instances[0];
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(n))
        .map(|v| v.value.clone())
        .unwrap_or(Val::None)
}

#[test]
fn a_partial_quest_record_is_read_from_the_occurrence_before_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    // Off: the winner as it stands, no stages.
    let a = quest::QuestInfo::load(&s, &key(QA), &b).unwrap().unwrap();
    assert!(a.stages.is_empty());
    assert!(a.flags.is_none());
    // On: the stages and flag word from test.esm, the fragment script from
    // the winner.
    let (a, merged) = quest::QuestInfo::load_with(&s, &key(QA), &b, true)
        .unwrap()
        .unwrap();
    assert!(merged);
    assert_eq!(
        a.stages.iter().map(|x| x.stage).collect::<Vec<_>>(),
        vec![10, 20]
    );
    assert_eq!(a.fragment_script.as_deref(), Some("qfnew"));
    assert_eq!(
        a.stage(20).unwrap().entries[0].fragment.as_deref(),
        Some("fragment_stage_0020_item_00")
    );
    assert!(a.file.ends_with("test.esm") || a.file == "test.esm");
    // A flagged winner carrying more than EDID and VMAD is read as it
    // stands.
    let (qb, merged) = quest::QuestInfo::load_with(&s, &key(QB), &b, true)
        .unwrap()
        .unwrap();
    assert!(!merged);
    assert!(qb.stages.is_empty());
    // A flagged winner carrying EDID only keeps the earlier occurrence's
    // VMAD too.
    let (qc, merged) = quest::QuestInfo::load_with(&s, &key(QC), &b, true)
        .unwrap()
        .unwrap();
    assert!(merged);
    assert_eq!(qc.fragment_script.as_deref(), Some("qfkept"));
    assert_eq!(qc.stages.len(), 1);
    // The run's cache under the switch, and the rule it relied on.
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = Scenario::NewGame;
    let off = quest::info_by_key(&s, 1 << 24, &mut vm.world, &key(QA)).unwrap();
    assert!(off.stages.is_empty());
    vm.world.quests.infos.clear();
    vm.world.quests.partial_previous = true;
    let on = quest::info_by_key(&s, 1 << 24, &mut vm.world, &key(QA)).unwrap();
    assert_eq!(on.stages.len(), 2);
    assert!(vm
        .world
        .quests
        .fill_used
        .borrow()
        .contains(rules::QUEST_PARTIAL_PREVIOUS));
    assert_eq!(
        vm.world.quests.partial_read,
        vec![crate::bind::record_key(&key(QA))]
    );
}

#[test]
fn a_stage_the_record_does_not_hold_leaves_the_quest_open_by_default() {
    let s = store();
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.quests.scenario = Scenario::NewGame;
    let qo =
        crate::data::record_object(&mut vm.world, &s, &key(QA), "quest", &Budget::new(1 << 24))
            .unwrap();
    // The winner holds no stage 20: setting it opens the quest's state.
    let i = quest::info_by_key(&s, 1 << 24, &mut vm.world, &key(QA)).unwrap();
    let mut qh = QuestHost::new(&s, 1 << 24);
    assert!(qh.set_stage(&mut vm.world, qo, &i, 20).is_err());
    assert!(vm.world.quests.state(&i).open.is_some());
}

#[test]
fn a_container_change_reaches_the_items_scripts_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = crate::bind::RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    let item =
        crate::data::record_object(&mut vm.world, &s, &key(ITEM), "objectreference", &b).unwrap();
    let p = vm.world.player("actor");
    let r = chain::container_changed(&mut vm, &s, item, Some(p), None, &b).unwrap();
    assert!(r.runs.is_empty());
    assert!(!vm.rules.contains(rules::CONTAINER_CHANGED_EVENT));
    vm.world.runstate.chain.container = true;
    let r = chain::container_changed(&mut vm, &s, item, Some(p), None, &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    assert!(vm.rules.contains(rules::CONTAINER_CHANGED_EVENT));
    assert_eq!(var(&vm, item, "moves"), Val::Int(1));
    assert_eq!(var(&vm, item, "holder"), Val::Object(p));
}

#[test]
fn a_location_change_reaches_the_actors_scripts_only_under_its_rule() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = crate::bind::RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    let a = crate::data::record_object(&mut vm.world, &s, &key(WALKER), "actor", &b).unwrap();
    let museum = crate::data::record_object(&mut vm.world, &s, &key(L2), "location", &b).unwrap();
    let town = crate::data::record_object(&mut vm.world, &s, &key(L1), "location", &b).unwrap();
    let r = chain::location_changed(&mut vm, &s, a, Some(museum), Some(town), &b).unwrap();
    assert!(r.runs.is_empty());
    vm.world.runstate.chain.location = true;
    let r = chain::location_changed(&mut vm, &s, a, Some(museum), Some(town), &b).unwrap();
    assert_eq!(r.runs.len(), 1);
    assert!(vm.rules.contains(rules::LOCATION_CHANGE_EVENT));
    assert_eq!(var(&vm, a, "changes"), Val::Int(1));
    assert_eq!(var(&vm, a, "now"), Val::Object(town));
}

#[test]
fn is_child_follows_the_other_locations_parents_only_under_its_rule() {
    let s = store();
    let max = 1 << 24;
    assert_eq!(chain::is_child(&s, max, &key(L1), &key(L3)), Ok(true));
    assert_eq!(chain::is_child(&s, max, &key(L2), &key(L3)), Ok(true));
    assert_eq!(chain::is_child(&s, max, &key(L3), &key(L1)), Ok(false));
    assert_eq!(chain::is_child(&s, max, &key(L2), &key(L2)), Ok(false));
    assert!(chain::is_child(&s, max, &key(L1), &key(ITEM)).is_err());
    let mut l = lib();
    let mut host = QuestHost::new(&s, max);
    let mut vm = Vm::new(&mut l, &mut host);
    let b = Budget::new(max);
    let town = crate::data::record_object(&mut vm.world, &s, &key(L1), "location", &b).unwrap();
    let hall = crate::data::record_object(&mut vm.world, &s, &key(L3), "location", &b).unwrap();
    let call = NativeCall {
        script: "location".into(),
        name: "ischild".into(),
        receiver: Some(Val::Object(town)),
        instance: None,
        args: vec![Val::Object(hall)],
        param_types: vec!["location".into()],
        return_type: "bool".into(),
    };
    assert!(chain::native(&s, max, &call, &mut vm.world).is_none());
    vm.world.runstate.chain.parents = true;
    assert!(matches!(
        chain::native(&s, max, &call, &mut vm.world),
        Some(crate::Reply::Value(Val::Bool(true)))
    ));
    assert!(vm
        .world
        .runstate
        .used
        .contains(&rules::LOCATION_IS_CHILD_PARENT));
}

#[test]
fn a_timer_that_used_up_its_own_budget_is_dropped_and_the_others_go_on() {
    let s = store();
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    let o = vm.world.object("q", None, &[]);
    let i = vm.world.instances.len();
    vm.world.instances.push(crate::world::Instance {
        object: Some(o),
        script: "quest".into(),
        state: String::new(),
        vars: Vec::new(),
    });
    for (seq, due, id) in [(0, 1.0, 1), (1, 2.0, 2)] {
        vm.world.timers.push(Timer {
            clock: Clock::Real,
            due: Some(due),
            seq,
            instance: i,
            id: Val::Int(id),
        });
    }
    let mut uses = std::collections::BTreeMap::new();
    // Neither has fired: nothing dropped, the first is due next.
    let first = crate::run::timer_key(&vm.world.timers[0]);
    assert_eq!(
        crate::run::drop_spent_timers(&mut vm, None, &uses, 2),
        (0, Some(first.clone()))
    );
    // The first used up its budget: it is dropped, the second comes next.
    uses.insert(first, 2);
    let second = crate::run::timer_key(&vm.world.timers[1]);
    assert_eq!(
        crate::run::drop_spent_timers(&mut vm, None, &uses, 2),
        (1, Some(second))
    );
    assert_eq!(vm.world.timers.len(), 1);
    assert!(vm.rules.contains(rules::TIMER_BUDGET_PER_TIMER));
}

#[test]
fn every_rule_of_the_chain_step_is_registered_pending() {
    for r in [
        rules::QUEST_PARTIAL_PREVIOUS,
        rules::QUEST_STAGE_ABSENT_OPEN,
        rules::CONTAINER_CHANGED_EVENT,
        rules::LOCATION_CHANGE_EVENT,
        rules::LOCATION_IS_CHILD_PARENT,
        rules::TIMER_BUDGET_PER_TIMER,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
