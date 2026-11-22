//! Quest state, the quest host and stage fragments over a synthetic
//! plugin (no game data).
use super::{assemble, base};
use crate::{
    quest::{self, Change, Job, QuestHost, Scenario},
    rules, Outcome, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt, SliceSource};
use fo4_plugin::{typed::Store, FormKey};

fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
fn record(sig: &[u8; 4], id: u32, data: &[u8]) -> Vec<u8> {
    let mut out = sig.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(131u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(data);
    out
}
fn group(kind: u32, label: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend(((24 + data.len()) as u32).to_le_bytes());
    out.extend(label);
    out.extend(kind.to_le_bytes());
    out.extend([0; 8]);
    out.extend(data);
    out
}
fn text(v: &mut Vec<u8>, t: &str) {
    v.extend((t.len() as u16).to_le_bytes());
    v.extend(t.as_bytes());
}
fn object(v: &mut Vec<u8>, name: &str, alias: i16, id: u32) {
    text(v, name);
    v.extend([1, 1, 0, 0]);
    v.extend(alias.to_le_bytes());
    v.extend(id.to_le_bytes());
}

struct Owned(Vec<u8>);
impl ReadAt for Owned {
    fn name(&self) -> &str {
        "test.esm"
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> fo4_core::Result<()> {
        SliceSource {
            name: self.name(),
            bytes: &self.0,
        }
        .read_exact_at(o, out)
    }
}

const STAT: u32 = 0x803;
const CELL: u32 = 0x810;
const BOX: u32 = 0x812;
const Q1: u32 = 0x820;
const Q2: u32 = 0x830;

/// The VMAD of a quest: no scripts, then a block of `script` with
/// properties and fragments (stage, log entry, function).
fn quest_vmad(script: &str, props: &[(&str, i16, u32)], frags: &[(u16, i32, &str)]) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0, 0, 0, 3];
    v.extend((frags.len() as u16).to_le_bytes());
    text(&mut v, script);
    v.push(0);
    v.extend((props.len() as u16).to_le_bytes());
    for (n, a, id) in props {
        object(&mut v, n, *a, *id);
    }
    for (stage, entry, f) in frags {
        v.extend(stage.to_le_bytes());
        v.extend([0, 0]);
        v.extend(entry.to_le_bytes());
        v.push(1);
        text(&mut v, script);
        text(&mut v, f);
    }
    v.extend(0u16.to_le_bytes());
    v
}
fn dnam(flags: u16) -> Vec<u8> {
    let mut d = flags.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    sub(b"DNAM", &d)
}
fn stage(n: u16, flags: u8, entries: &[bool]) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([flags, 0]);
    let mut v = sub(b"INDX", &d);
    for conditioned in entries {
        v.extend(sub(b"QSDT", &[0]));
        if *conditioned {
            v.extend(sub(b"CTDA", &[0; 32]));
        }
    }
    v
}

/// Q1: started with the game; stage 0 (startup), stage 10, stage 20 with
/// a conditioned first entry; objective 10; alias 0 forced to a placed
/// box. Q2: stage 20 only.
fn store() -> Store {
    let q1 = [
        sub(b"EDID", b"Q1\0"),
        sub(
            b"VMAD",
            &quest_vmad(
                "QF1",
                &[("Alias_A", 0, Q1), ("Other", -1, Q2)],
                &[
                    (0, 0, "Fragment_Stage_0000_Item_00"),
                    (10, 0, "Fragment_Stage_0010_Item_00"),
                    (20, 0, "Fragment_Stage_0020_Item_00"),
                ],
            ),
        ),
        dnam(0x0001),
        sub(b"NEXT", &[]),
        stage(0, 0x02, &[false]),
        stage(10, 0, &[false]),
        stage(20, 0, &[true, false]),
        sub(b"QOBJ", &10u16.to_le_bytes()),
        sub(b"FNAM", &[0; 4]),
        sub(b"ANAM", &1u32.to_le_bytes()),
        sub(b"ALST", &0u32.to_le_bytes()),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &[0; 4]),
        sub(b"ALFR", &BOX.to_le_bytes()),
        sub(b"ALED", &[]),
    ]
    .concat();
    let q2 = [
        sub(b"EDID", b"Q2\0"),
        sub(
            b"VMAD",
            &quest_vmad("QF2", &[], &[(20, 0, "Fragment_Stage_0020_Item_00")]),
        ),
        dnam(0),
        stage(20, 0, &[false]),
    ]
    .concat();
    let refs = record(b"REFR", BOX, &sub(b"NAME", &STAT.to_le_bytes()));
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    let children = group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs));
    let mut body = group(2, [0; 4], &[cell, children].concat());
    body.extend(record(b"STAT", STAT, &sub(b"EDID", b"Box\0")));
    body.extend(record(b"QUST", Q1, &q1));
    body.extend(record(b"QUST", Q2, &q2));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(8u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(body);
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

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    }
}

fn library() -> crate::MapLibrary {
    let mut lib = base();
    lib.add(assemble(
        "script Quest extends Form
         func SetCurrentStageID Bool native Int aiStageID
         func GetCurrentStageID Int native
         func IsStageDone Bool native Int aiStage
         func IsRunning Bool native
         func Start Bool native
         func Stop none native
         func SetObjectiveDisplayed none native Int aiObjective Bool abDisplayed Bool abForce
         func IsObjectiveDisplayed Bool native Int aiObjective
         func OnQuestInit none
         func setStage Bool Int aiStage
         local Bool ::temp
         23 SetCurrentStageID self ::temp 1 aiStage
         26 ::temp",
    ));
    lib.add(assemble("script Alias extends ScriptObject"));
    lib.add(assemble(
        "script ReferenceAlias extends Alias
         func GetReference ObjectReference native",
    ));
    lib.add(assemble(
        "script QF1 extends Quest
         var ReferenceAlias ::Alias_A_var
         prop ReferenceAlias Alias_A ::Alias_A_var
         var Quest ::Other_var
         prop Quest Other ::Other_var
         var ObjectReference got
         var Int stage
         var Bool shown
         var Bool running
         func Fragment_Stage_0000_Item_00 none
         23 IsRunning self running 0
         func Fragment_Stage_0010_Item_00 none
         local none ::nonevar
         local Bool ::b
         23 SetObjectiveDisplayed self ::nonevar 3 10 true false
         23 IsObjectiveDisplayed self shown 1 10
         23 GetReference ::Alias_A_var got 0
         23 setStage ::Other_var ::b 1 20
         23 GetCurrentStageID self stage 0
         func Fragment_Stage_0020_Item_00 none
         23 IsRunning self running 0",
    ));
    lib.add(assemble(
        "script QF2 extends Quest
         var Bool done20
         var Bool done10
         func Fragment_Stage_0020_Item_00 none
         23 IsStageDone self done20 1 20
         23 IsStageDone self done10 1 10",
    ));
    lib
}

fn var(vm: &Vm<'_>, quest: usize, name: &str) -> Val {
    let i = vm.world.quests.fragment_instances[&quest];
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .value
        .clone()
}

#[test]
fn a_stage_runs_its_fragment_on_the_quest_and_cascades() {
    let store = store();
    let mut lib = library();
    let b = Budget::new(1 << 24);
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut lib, &mut host);
    let q1 = quest::quest_object(&mut vm, &store, &key(Q1), &b).unwrap();
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q1, 10),
        crate::Reply::Value(Val::Bool(true))
    ));
    let runs = quest::run_jobs(&mut vm, &store, 16, &b);
    // Q1's fragment, then for Q2 (whose running state nothing settled)
    // an open run (whether the stage started it) and its fragment.
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert!(matches!(runs[1].job, Job::Open { .. }));
    for r in [&runs[0], &runs[2]] {
        assert!(
            matches!(r.outcomes[..], [(_, Outcome::Completed(_))]),
            "{r:?}"
        );
    }
    // The forced reference of the alias, the stage just set, the
    // objective just displayed.
    let got = var(&vm, q1, "got");
    assert!(
        matches!(got, Val::Object(o) if vm.world.objects[o].key.as_deref() == Some("test.esm:00000812"))
    );
    assert_eq!(var(&vm, q1, "stage"), Val::Int(10));
    assert_eq!(var(&vm, q1, "shown"), Val::Bool(true));
    // The second quest's fragment ran after the first (queued), with its
    // stage done; the stages it never saw set stay undetermined.
    let q2 = runs[2].job.quest();
    assert!(matches!(
        runs[2].job,
        Job::Fragment {
            stage: 20,
            depth: 2,
            ..
        }
    ));
    assert_eq!(var(&vm, q2, "done20"), Val::Bool(true));
    assert!(matches!(var(&vm, q2, "done10"), Val::Unknown(_)));
    assert!(vm.world.quests.log.contains(&(q2, Change::StageSet(20))));
    for r in [
        rules::QUEST_SET_STAGE,
        rules::QUEST_LOG_ENTRY,
        rules::QUEST_ORDER,
        rules::QUEST_FRAGMENT_ENTRY,
        rules::QUEST_FRAGMENT_INSTANCE,
        rules::QUEST_OBJECTIVES,
        rules::QUEST_ALIAS_FORCED,
    ] {
        assert!(vm.rules.contains(r), "{r}");
    }
    // A conditioned log entry first: which fragment runs is open.
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q1, 20),
        crate::Reply::Value(Val::Bool(true))
    ));
    assert!(matches!(
        vm.world.quests.queue.front(),
        Some(Job::Open { .. })
    ));
    // A stage the record does not hold makes the quest's state open.
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q1, 5),
        crate::Reply::Undetermined
    ));
    assert!(vm.world.quests.states["test.esm:00000820"].open.is_some());
}

#[test]
fn a_new_game_starts_the_flagged_quests_with_their_startup_stage() {
    let store = store();
    let mut lib = library();
    let b = Budget::new(1 << 24);
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut lib, &mut host);
    let started = quest::start_new_game(&mut vm, &store, &b).unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(vm.world.quests.scenario, Scenario::NewGame);
    let q1 = started[0].1;
    let runs = quest::run_jobs(&mut vm, &store, 16, &b);
    // OnQuestInit (no script handles it), then the startup stage.
    assert!(matches!(
        runs[0].job,
        Job::Event {
            event: "onquestinit",
            ..
        }
    ));
    assert!(matches!(runs[1].job, Job::Fragment { stage: 0, .. }));
    assert_eq!(var(&vm, q1, "running"), Val::Bool(true));
    // A stage already done is not set again without the repeat flag.
    let s = vm.world.quests.states["test.esm:00000820"].clone();
    assert_eq!(
        (s.current, s.is_done(0), s.is_done(10)),
        (Some(0), Some(true), Some(false))
    );
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q1, 0),
        crate::Reply::Value(Val::Bool(false))
    ));
    // Setting a stage of a quest that is not running starts it.
    let q2 = quest::quest_object(&mut vm, &store, &key(Q2), &b).unwrap();
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q2, 20),
        crate::Reply::Value(Val::Bool(true))
    ));
    assert!(vm.world.quests.log.contains(&(q2, Change::Started)));
    let runs = quest::run_jobs(&mut vm, &store, 16, &b);
    assert_eq!(var(&vm, q2, "done10"), Val::Bool(false));
    assert_eq!(runs.len(), 2);
    assert!(vm.rules.contains(rules::QUEST_START_GAME));
    assert!(vm.rules.contains(rules::QUEST_REPEATED_STAGE));
}

#[test]
fn nested_jobs_run_inside_the_call_that_set_the_stage() {
    let store = store();
    let mut lib = library();
    let b = Budget::new(1 << 24);
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut runner = quest::Runner::new(&store, 1 << 24, 16);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.nested = Some(&mut runner);
    let q1 = quest::quest_object(&mut vm, &store, &key(Q1), &b).unwrap();
    quest::set_stage(&mut vm, &store, q1, 10);
    let top = quest::run_jobs(&mut vm, &store, 16, &b);
    // Only Q1's fragment ran from the queue; Q2's ran inside it, during
    // its setStage call, so its effects lie inside Q1's.
    assert_eq!(top.len(), 1);
    let nested = std::mem::take(&mut vm.world.quests.nested_runs);
    assert_eq!(nested.len(), 2, "{nested:?}");
    let inner = &nested[1];
    assert!(matches!(inner.job, Job::Fragment { stage: 20, .. }));
    assert!(top[0].seq < inner.seq);
    assert!(top[0].effects.start <= inner.effects.start && inner.effects.end <= top[0].effects.end);
    assert!(matches!(top[0].outcomes[..], [(_, Outcome::Completed(_))]));
    assert_eq!(var(&vm, q1, "stage"), Val::Int(10));
    assert_eq!(var(&vm, inner.job.quest(), "done20"), Val::Bool(true));
}
