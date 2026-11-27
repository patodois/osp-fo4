//! Quest state, the quest host and stage fragments over a synthetic
//! plugin (no game data).
use super::{assemble, base};
use crate::{
    quest::{self, Change, Job, QuestHost, Scenario},
    rules, Outcome, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt, SliceSource};
use fo4_plugin::{typed::Store, FormKey};

pub(super) fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
pub(super) fn record(sig: &[u8; 4], id: u32, data: &[u8]) -> Vec<u8> {
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
pub(super) fn group(kind: u32, label: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend(((24 + data.len()) as u32).to_le_bytes());
    out.extend(label);
    out.extend(kind.to_le_bytes());
    out.extend([0; 8]);
    out.extend(data);
    out
}
pub(super) fn text(v: &mut Vec<u8>, t: &str) {
    v.extend((t.len() as u16).to_le_bytes());
    v.extend(t.as_bytes());
}
pub(super) fn object(v: &mut Vec<u8>, name: &str, alias: i16, id: u32) {
    text(v, name);
    v.extend([1, 1, 0, 0]);
    v.extend(alias.to_le_bytes());
    v.extend(id.to_le_bytes());
}

pub(super) struct Owned(pub(super) Vec<u8>);
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

pub(super) const STAT: u32 = 0x803;
const CELL: u32 = 0x810;
pub(super) const BOX: u32 = 0x812;
pub(super) const Q1: u32 = 0x820;
pub(super) const Q2: u32 = 0x830;
pub(super) const G1: u32 = 0x840;

/// The VMAD of a quest: no scripts, then a block of `script` with
/// properties and fragments (stage, log entry, function).
fn quest_vmad(script: &str, props: &[(&str, i16, u32)], frags: &[(u16, i32, &str)]) -> Vec<u8> {
    quest_vmad_with(&[], script, props, frags)
}
/// As [`quest_vmad`], with the quest's own scripts (no properties).
fn quest_vmad_with(
    own: &[&str],
    script: &str,
    props: &[(&str, i16, u32)],
    frags: &[(u16, i32, &str)],
) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0];
    v.extend((own.len() as u16).to_le_bytes());
    for o in own {
        text(&mut v, o);
        v.push(0);
        v.extend(0u16.to_le_bytes());
    }
    v.push(3);
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

/// A CTDA: operator code, flag bits, function, comparison word, the two
/// parameters; run-on 0, reference 0, last word -1.
pub(super) fn ctda(op: u8, flags: u8, function: u16, value: u32, p1: u32, p2: u32) -> Vec<u8> {
    fo4_plugin::condition::Ctda {
        byte0: (op << 5) | flags,
        unused: [0; 3],
        value,
        function,
        pad: [0; 2],
        param1: p1,
        param2: p2,
        run_on: 0,
        reference: 0,
        param3: -1,
    }
    .to_bytes()
    .to_vec()
}
/// A stage whose entries carry the given conditions (none: unconditioned).
fn stage_with(n: u16, entries: &[&[Vec<u8>]]) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([0, 0]);
    let mut v = sub(b"INDX", &d);
    for conds in entries {
        v.extend(sub(b"QSDT", &[0]));
        for c in conds.iter() {
            v.extend(sub(b"CTDA", c));
        }
    }
    v
}

/// Q1: started with the game; stage 0 (startup), stage 10, stage 20 with
/// a conditioned first entry (function 0, not evaluated), stage 30 whose
/// first entry holds when stage 10 is done (function 59), stage 40 whose
/// first entry holds when G1 >= G1 (function 74 against the global bit) or
/// Q2 runs (function 56); objective 10; alias 0 forced to a placed box; its own
/// script QS1 handles OnStageSet. Q2: stage 20 only.
pub(super) fn store() -> Store {
    let q1 = [
        sub(b"EDID", b"Q1\0"),
        sub(
            b"VMAD",
            &quest_vmad_with(
                &["QS1"],
                "QF1",
                &[("Alias_A", 0, Q1), ("Other", -1, Q2)],
                &[
                    (0, 0, "Fragment_Stage_0000_Item_00"),
                    (10, 0, "Fragment_Stage_0010_Item_00"),
                    (20, 0, "Fragment_Stage_0020_Item_00"),
                    (30, 0, "Fragment_Stage_0030_Item_00"),
                    (30, 1, "Fragment_Stage_0030_Item_01"),
                    (40, 0, "Fragment_Stage_0030_Item_00"),
                    (40, 1, "Fragment_Stage_0030_Item_01"),
                ],
            ),
        ),
        dnam(0x0001),
        sub(b"NEXT", &[]),
        stage(0, 0x02, &[false]),
        stage(10, 0, &[false]),
        stage(20, 0, &[true, false]),
        stage_with(30, &[&[ctda(0, 0, 59, 1f32.to_bits(), Q1, 10)], &[]]),
        stage_with(
            40,
            &[
                &[
                    ctda(3, 0x05, 74, G1, G1, 0),
                    ctda(0, 0, 56, 1f32.to_bits(), Q2, 0),
                ],
                &[],
            ],
        ),
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
    // The box carries a script whose activation sets Q1's stage 10.
    let mut lever = vec![6, 0, 2, 0];
    lever.extend(1u16.to_le_bytes());
    text(&mut lever, "Lever");
    lever.push(0);
    lever.extend(1u16.to_le_bytes());
    object(&mut lever, "Q", -1, Q1);
    let refs = record(
        b"REFR",
        BOX,
        &[sub(b"NAME", &STAT.to_le_bytes()), sub(b"VMAD", &lever)].concat(),
    );
    let cell = record(b"CELL", CELL, &sub(b"EDID", b"Room\0"));
    let children = group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs));
    let mut body = group(2, [0; 4], &[cell, children].concat());
    body.extend(record(b"STAT", STAT, &sub(b"EDID", b"Box\0")));
    body.extend(record(b"QUST", Q1, &q1));
    body.extend(record(b"QUST", Q2, &q2));
    body.extend(record(
        b"GLOB",
        G1,
        &[sub(b"EDID", b"G1\0"), sub(b"FLTV", &3f32.to_le_bytes())].concat(),
    ));
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

pub(super) fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    }
}

pub(super) fn library() -> crate::MapLibrary {
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
         23 IsRunning self running 0
         var Int picked
         func Fragment_Stage_0030_Item_00 none
         13 picked 0
         func Fragment_Stage_0030_Item_01 none
         13 picked 1",
    ));
    lib.add(assemble(
        "script Lever extends ObjectReference
         var Quest ::Q_var
         prop Quest Q ::Q_var
         func OnActivate none ObjectReference akActionRef
         local Bool ::b
         23 setStage ::Q_var ::b 1 10",
    ));
    lib.add(assemble(
        "script QS1 extends Quest
         var Int seenStage -1
         var Int seenItem -1
         func OnStageSet none Int auiStageID Int auiItemID
         13 seenStage auiStageID
         13 seenItem auiItemID",
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

/// A variable of the quest's own script QS1.
fn own(vm: &Vm<'_>, quest: usize, name: &str) -> Val {
    let i = vm.world.objects[quest]
        .instances
        .iter()
        .copied()
        .find(|i| vm.world.instances[*i].script == "qs1")
        .expect("qs1");
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .value
        .clone()
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
    let mut attach = crate::bind::RecordScripts::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    let q1 = quest::quest_object(&mut vm, &store, &key(Q1), &b).unwrap();
    assert!(matches!(
        quest::set_stage(&mut vm, &store, q1, 10),
        crate::Reply::Value(Val::Bool(true))
    ));
    let all = quest::run_jobs(&mut vm, &store, 16, &b);
    // OnStageSet after each fragment; no script here handles it.
    let events: Vec<&Job> = all
        .iter()
        .map(|r| &r.job)
        .filter(|j| matches!(j, Job::StageEvent { .. }))
        .collect();
    assert!(matches!(
        events[..],
        [
            Job::StageEvent {
                stage: 10,
                entry: Some(0),
                ..
            },
            Job::StageEvent { stage: 20, .. }
        ]
    ));
    assert!(matches!(all[1].job, Job::StageEvent { stage: 10, .. }));
    // Q1's own script handles it, with the stage and the entry; Q2 has
    // no script that does.
    assert!(matches!(all[1].outcomes[..], [(_, Outcome::Completed(_))]));
    assert_eq!(own(&vm, q1, "seenstage"), Val::Int(10));
    assert_eq!(own(&vm, q1, "seenitem"), Val::Int(0));
    assert!(all.last().is_some_and(|r| r.outcomes.is_empty()));
    let runs: Vec<_> = all
        .into_iter()
        .filter(|r| !matches!(r.job, Job::StageEvent { .. }))
        .collect();
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
    assert!(matches!(runs[2].job, Job::StageEvent { stage: 0, .. }));
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
    // OnQuestInit, the fragment, OnStageSet.
    assert_eq!(runs.len(), 3);
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
    // Q1's fragment and then its OnStageSet ran from the queue; Q2's
    // jobs ran inside the fragment, during its setStage call, so their
    // effects lie inside Q1's; Q1's OnStageSet did not run inside its own
    // fragment.
    assert_eq!(top.len(), 2);
    assert!(matches!(top[1].job, Job::StageEvent { stage: 10, .. }));
    assert!(top[0].effects.end <= top[1].effects.start);
    let nested = std::mem::take(&mut vm.world.quests.nested_runs);
    assert_eq!(nested.len(), 3, "{nested:?}");
    let inner = &nested[1];
    assert!(matches!(inner.job, Job::Fragment { stage: 20, .. }));
    assert!(top[0].seq < inner.seq);
    assert!(top[0].effects.start <= inner.effects.start && inner.effects.end <= top[0].effects.end);
    assert!(matches!(top[0].outcomes[..], [(_, Outcome::Completed(_))]));
    assert_eq!(var(&vm, q1, "stage"), Val::Int(10));
    assert_eq!(var(&vm, inner.job.quest(), "done20"), Val::Bool(true));
}

pub(super) fn conds(list: &[Vec<u8>]) -> Vec<fo4_plugin::condition::Condition> {
    list.iter()
        .enumerate()
        .map(|(i, c)| fo4_plugin::condition::Condition {
            index: i,
            ctda: fo4_plugin::condition::Ctda::from_bytes(c[..].try_into().unwrap()),
            cis1: None,
            cis2: None,
        })
        .collect()
}

#[test]
fn conditions_are_three_valued_and_or_binds_tighter() {
    use crate::cond::{eval_list, eval_one, Combine, StaticFacts, Truth, Why};
    let store = store();
    let ctx = fo4_plugin::actor::Context {
        order: store.order(),
        plugin: 0,
    };
    // New game: Q1 runs (start-game bit), Q2 does not; no stage done.
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    let one = 1f32.to_bits();
    let q1 = ctda(0, 0, 56, one, Q1, 0);
    let q2 = ctda(0, 0, 56, one, Q2, 0);
    let q2_or = ctda(0, 0x01, 56, one, Q2, 0);
    let unknown = ctda(0, 0, 0, 0, 0, 0);
    let ev = |f: &mut StaticFacts<'_>, l: &[Vec<u8>], c: Combine| {
        eval_list(&conds(l), ctx, "test.esm", f, c)
    };
    let g = Combine::OrGroups;
    assert_eq!(ev(&mut f, &[], g), Truth::True);
    assert_eq!(ev(&mut f, &[q1.clone(), q2.clone()], g), Truth::False);
    // (Q2 OR Q1) AND unknown: undetermined, naming function 0.
    assert_eq!(
        ev(&mut f, &[q2_or.clone(), q1.clone(), unknown.clone()], g),
        Truth::Undetermined(vec![Why {
            function: 0,
            why: "the function is not evaluated"
        }])
    );
    // A false condition settles an AND whatever the undetermined one is.
    assert_eq!(ev(&mut f, &[unknown.clone(), q2.clone()], g), Truth::False);
    // Q2 AND Q2 OR Q1: false with OR groups, true left to right.
    let l = [q2.clone(), q2_or.clone(), q1.clone()];
    assert_eq!(ev(&mut f, &l, g), Truth::False);
    assert_eq!(ev(&mut f, &l, Combine::LeftToRight), Truth::True);
    // The OR bit of the last condition joins nothing.
    assert_eq!(ev(&mut f, &[q1.clone(), q2_or.clone()], g), Truth::False);
    // Stage done (59) is false at a new game; the current stage (58) is
    // undetermined; operator codes 2 and 5; the global bit; the flag bits
    // that make parameters aliases; an operator code without a meaning.
    let t = |f: &mut StaticFacts<'_>, c: Vec<u8>| {
        let c = fo4_plugin::condition::Ctda::from_bytes(c[..].try_into().unwrap());
        eval_one(&c, ctx, "test.esm", f)
    };
    assert_eq!(t(&mut f, ctda(0, 0, 59, 0, Q1, 10)).0, Truth::True);
    assert!(matches!(
        t(&mut f, ctda(0, 0, 58, 0, Q1, 0)).0,
        Truth::Undetermined(_)
    ));
    assert_eq!(
        t(&mut f, ctda(2, 0, 74, 2f32.to_bits(), G1, 0)).0,
        Truth::True
    );
    assert_eq!(
        t(&mut f, ctda(5, 0, 74, 2f32.to_bits(), G1, 0)).0,
        Truth::False
    );
    let (v, used) = t(&mut f, ctda(0, 0x04, 74, G1, G1, 0));
    assert_eq!(v, Truth::True);
    for r in [
        rules::COND_LAYOUT,
        rules::COND_GLOBAL,
        rules::COND_OPS,
        rules::COND_FN_74,
    ] {
        assert!(used.contains(r), "{r}");
    }
    assert!(matches!(
        t(&mut f, ctda(0, 0x02, 56, one, Q1, 0)).0,
        Truth::Undetermined(w) if w[0].why == "the parameters name aliases or package data"
    ));
    assert!(matches!(
        t(&mut f, ctda(6, 0, 56, one, Q1, 0)).0,
        Truth::Undetermined(_)
    ));
    // A parameter that names no quest.
    assert!(matches!(
        t(&mut f, ctda(0, 0, 56, one, G1, 0)).0,
        Truth::Undetermined(w) if w[0].why == "a parameter names no winning QUST"
    ));
}

#[test]
fn log_entry_conditions_choose_the_fragment() {
    let store = store();
    let b = Budget::new(1 << 24);
    let fresh = |lib: &mut crate::MapLibrary, host: &mut QuestHost<'_>, f: &dyn Fn(&mut Vm<'_>)| {
        let mut attach = crate::bind::RecordScripts::new(&store, 1 << 24);
        let mut vm = Vm::new(lib, host);
        vm.attach = Some(&mut attach);
        f(&mut vm);
    };
    // Isolated: whether stage 10 is done is undetermined, so the choice
    // between the two entries of stage 30 stays open.
    let mut lib = library();
    let mut host = QuestHost::new(&store, 1 << 24);
    fresh(&mut lib, &mut host, &|vm| {
        let q1 = quest::quest_object(vm, &store, &key(Q1), &b).unwrap();
        quest::set_stage(vm, &store, q1, 30);
        assert!(matches!(
            vm.world.quests.queue.front(),
            Some(Job::Open { .. })
        ));
        // The item OnStageSet receives is undetermined then.
        assert!(matches!(
            vm.world.quests.queue.get(1),
            Some(Job::StageEvent { entry: None, .. })
        ));
    });
    assert_eq!(host.open_functions.get(&59), Some(&1));
    // Stage 10 set first: the first entry holds.
    let mut host = QuestHost::new(&store, 1 << 24);
    fresh(&mut lib, &mut host, &|vm| {
        let q1 = quest::quest_object(vm, &store, &key(Q1), &b).unwrap();
        quest::set_stage(vm, &store, q1, 10);
        quest::run_jobs(vm, &store, 16, &b);
        quest::set_stage(vm, &store, q1, 30);
        let runs = quest::run_jobs(vm, &store, 16, &b);
        assert!(matches!(
            runs[0].job,
            Job::Fragment {
                stage: 30,
                entry: 0,
                ..
            }
        ));
        assert_eq!(var(vm, q1, "picked"), Val::Int(0));
        assert_eq!(own(vm, q1, "seenitem"), Val::Int(0));
        assert!(vm.rules.contains(rules::COND_FN_59));
        assert!(vm.rules.contains(rules::QUEST_LOG_ENTRY_STATE));
    });
    // New game: no stage done, so the second entry runs; stage 40's first
    // entry holds by the global even though nothing settles... Q2 runs
    // here (not flagged), and the OR group holds by its first condition.
    let mut host = QuestHost::new(&store, 1 << 24);
    fresh(&mut lib, &mut host, &|vm| {
        quest::start_new_game(vm, &store, &b).unwrap();
        quest::run_jobs(vm, &store, 16, &b);
        let q1 = quest::quest_object(vm, &store, &key(Q1), &b).unwrap();
        quest::set_stage(vm, &store, q1, 30);
        quest::run_jobs(vm, &store, 16, &b);
        assert_eq!(var(vm, q1, "picked"), Val::Int(1));
        assert_eq!(own(vm, q1, "seenstage"), Val::Int(30));
        assert_eq!(own(vm, q1, "seenitem"), Val::Int(1));
        quest::set_stage(vm, &store, q1, 40);
        quest::run_jobs(vm, &store, 16, &b);
        assert_eq!(var(vm, q1, "picked"), Val::Int(0));
        for r in [rules::COND_GLOBAL, rules::COND_FN_74, rules::COND_LIST] {
            assert!(vm.rules.contains(r), "{r}");
        }
    });
    // Isolated, stage 40: Q2's running state is undetermined, but the
    // global already settles the OR group.
    let mut host = QuestHost::new(&store, 1 << 24);
    fresh(&mut lib, &mut host, &|vm| {
        let q1 = quest::quest_object(vm, &store, &key(Q1), &b).unwrap();
        quest::set_stage(vm, &store, q1, 40);
        assert!(matches!(
            vm.world.quests.queue.front(),
            Some(Job::Fragment {
                stage: 40,
                entry: 0,
                ..
            })
        ));
    });
    // Conditions not evaluated (as before they were read): open.
    let mut host = QuestHost::new(&store, 1 << 24);
    host.evaluate = false;
    host.stage_event = false;
    fresh(&mut lib, &mut host, &|vm| {
        let q1 = quest::quest_object(vm, &store, &key(Q1), &b).unwrap();
        quest::set_stage(vm, &store, q1, 40);
        assert!(matches!(
            vm.world.quests.queue.front(),
            Some(Job::Open { .. })
        ));
        assert_eq!(vm.world.quests.queue.len(), 1);
    });
}

#[test]
fn script_run_in_quest_mode_runs_the_fragment_inside_the_handler() {
    let store = store();
    let b = Budget::new(1 << 24);
    let ev = [crate::bind::Event::parse("activate").unwrap()];
    let run = |mode| {
        let mut lib = library();
        crate::bind::run_in(
            mode,
            &mut lib,
            &store,
            Some(&key(BOX)),
            &key(STAT),
            &ev,
            0,
            &b,
        )
        .unwrap()
    };
    // Quest host: the stage is set, its fragment and OnStageSet run inside
    // the activation, which completes.
    let r = run(crate::bind::Mode::Quest);
    assert!(
        matches!(r.outcomes[..], [(_, _, Outcome::Completed(_))]),
        "{:?}",
        r.outcomes
    );
    assert!(r
        .quest_jobs
        .iter()
        .any(|j| matches!(j.job, Job::Fragment { stage: 10, .. })
            && matches!(j.outcomes[..], [(_, Outcome::Completed(_))])));
    assert!(r
        .quest_jobs
        .iter()
        .any(|j| matches!(j.job, Job::StageEvent { stage: 10, .. })));
    assert!(r.rules.contains(rules::QUEST_SET_STAGE));
    assert_eq!(r.quest_jobs_left, 0);
    // Data host (the earlier behaviour): the stage set is only recorded.
    let r = run(crate::bind::Mode::Data);
    assert!(r.quest_jobs.is_empty());
    assert!(!r.rules.contains(rules::QUEST_SET_STAGE));
}

#[test]
fn a_stage_number_stored_twice_counts_its_fragment_once() {
    // One fragment (stage 10, entry 0) and two stages numbered 10: the
    // second maps the same key, which used to count it twice and underflow
    // the unmapped count.
    let q = [
        sub(b"EDID", b"Q1\0"),
        sub(
            b"VMAD",
            &quest_vmad("QF1", &[], &[(10, 0, "Fragment_Stage_0010_Item_00")]),
        ),
        dnam(0),
        stage(10, 0, &[false]),
        stage(10, 0, &[false]),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(1u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(record(b"QUST", Q1, &q));
    let sources: Vec<(String, Box<dyn ReadAt>)> =
        vec![("test.esm".into(), Box::new(Owned(plugin)))];
    let b = Budget::new(8 << 20);
    let s = Store::from_sources(fo4_core::Game::Fallout4, sources, Limits::default(), &b).unwrap();
    let i = quest::QuestInfo::load(&s, &key(Q1), &b).unwrap().unwrap();
    assert_eq!(i.stages.len(), 2);
    assert_eq!(i.unmapped_fragments, 0);
}
