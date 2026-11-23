//! The graph step's readings over synthetic data (no game data): a waiting
//! scene offered again when a quest script variable its conditions read is
//! written, and the stage-setter table following a call from another
//! script's fragment into a quest script's function.
use super::quest::{conds, ctda, key, library, store};
use crate::{
    bind::record_key,
    quest::Job,
    rules,
    scene::{self, Change, PhaseInfo, SceneInfo, Step},
    setters::{self, Attachment, Need, Part, ScenePoint, Slot},
    NoHost, Outcome, Vm,
};
use std::{collections::BTreeMap, rc::Rc};

const QQ: u32 = 0x9a0;
const SC: u32 = 0x9a1;

/// Operator code 3 (`>=`), function 629 against 2, on quest QQ; CIS2 names
/// `name`.
fn reads(name: &str) -> Vec<fo4_plugin::condition::Condition> {
    let mut c = conds(&[ctda(3, 0, 629, 2.0f32.to_bits(), QQ, 0)]);
    c[0].cis2 = Some(name.as_bytes().to_vec());
    c
}

fn scene_info(completion: Vec<fo4_plugin::condition::Condition>) -> SceneInfo {
    SceneInfo {
        key: key(SC),
        plugin: 0,
        file: "test.esm".into(),
        flags: 0,
        quest: Some(key(QQ)),
        begin: None,
        end: None,
        phases: vec![PhaseInfo {
            start: Vec::new(),
            completion,
            begin: None,
            end: None,
            actions: Vec::new(),
            stage_fields: [-1, -1],
        }],
        actions: Vec::new(),
        stage_fields: [-1, -1],
        unplaced: 0,
    }
}

#[test]
fn a_write_of_a_variable_a_waiting_scene_reads_offers_it_again_only_under_its_rule() {
    let s = store();
    let mut l = library();
    l.add(super::assemble(
        "script QV extends Quest
         var Int n 0
         var Int other 0
         func Bump none
         13 n 2
         13 other 1",
    ));
    let f = l.scripts["qv"].function("", "bump").unwrap().clone();
    let mut host = NoHost;
    let mut vm = Vm::new(&mut l, &mut host);
    let q = vm.world.object("q", Some(record_key(&key(QQ))), &["quest"]);
    let i = vm.instantiate(Some(q), "qv").unwrap();
    // Off: no write is kept.
    assert!(matches!(
        vm.run(Some(i), f.clone(), Vec::new()),
        Outcome::Completed(_)
    ));
    assert!(vm.world.scenes.variable_writes.is_none());
    // On: the writes are kept by object and variable name.
    vm.world.scenes.variable_writes = Some(Default::default());
    assert!(matches!(
        vm.run(Some(i), f, Vec::new()),
        Outcome::Completed(_)
    ));
    let written = vm.world.scenes.variable_writes.take().unwrap();
    assert!(written.contains(&(q, "n".to_string())));
    assert!(written.contains(&(q, "other".to_string())));
    // A scene waiting on a completion condition reading `n`.
    let sc = vm
        .world
        .object("sc", Some(record_key(&key(SC))), &["scene"]);
    vm.world
        .scenes
        .infos
        .insert(record_key(&key(SC)), Some(Rc::new(scene_info(reads("n")))));
    vm.world.scenes.log.push((
        sc,
        Change::Waiting(0, "completion conditions false (waits on the world)".into()),
    ));
    let before = vm.world.quests.queue.len();
    assert_eq!(scene::recheck_variables(&s, &mut vm.world, &written), 1);
    assert!(matches!(
        vm.world.quests.queue.back(),
        Some(Job::Scene { scene, step: Step::Complete, .. }) if *scene == sc
    ));
    assert_eq!(vm.world.quests.queue.len(), before + 1);
    // A write of another variable, or of `n` on another object, does not.
    let other = [(q, "other".to_string())].into_iter().collect();
    assert_eq!(scene::recheck_variables(&s, &mut vm.world, &other), 0);
    let elsewhere = [(sc, "n".to_string())].into_iter().collect();
    assert_eq!(scene::recheck_variables(&s, &mut vm.world, &elsewhere), 0);
    // A scene waiting on an action is not offered.
    vm.world
        .scenes
        .log
        .push((sc, Change::Waiting(0, "an action runs in the phase".into())));
    assert_eq!(scene::recheck_variables(&s, &mut vm.world, &written), 0);
}

#[test]
fn the_setter_table_follows_a_fragments_call_into_a_quest_function_only_under_its_rule() {
    let s = store();
    let b = fo4_core::Budget::new(1 << 24);
    let mut lib = library();
    lib.add(super::assemble("script Scene extends Form"));
    lib.add(super::assemble(
        "script SeqQ extends Quest
         func Finish none
         local Bool ::r
         23 setStage self ::r 1 50",
    ));
    lib.add(super::assemble(
        "script SF extends Scene
         func Fragment_Phase_01_Begin none
         local SeqQ kmyquest
         local Quest ::q
         local none ::nonevar
         23 GetOwningQuest self ::q 0
         14 kmyquest ::q
         23 Finish kmyquest ::nonevar 0",
    ));
    let att = |record, signature, part, quest, script: &str, slots| Attachment {
        record,
        signature,
        part,
        quest,
        script: script.into(),
        objects: BTreeMap::new(),
        aliases: BTreeMap::new(),
        ints: BTreeMap::new(),
        structs: BTreeMap::new(),
        slots,
    };
    let atts = vec![
        att(
            key(QQ),
            *b"QUST",
            Part::Own,
            Some(key(QQ)),
            "seqq",
            Vec::new(),
        ),
        att(
            key(SC),
            *b"SCEN",
            Part::Fragment,
            Some(key(QQ)),
            "sf",
            vec![("fragment_phase_01_begin".to_string(), Slot::Phase(0, 1))],
        ),
    ];
    let t = setters::table(&lib, &atts, &s, None, &b).unwrap();
    let x = t.of_stage(&key(QQ), 50);
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].need, Need::Unreached);
    let t = setters::table_with(&lib, &atts, &s, None, &b, true).unwrap();
    let x = t.of_stage(&key(QQ), 50);
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].need, Need::Scene(key(SC), ScenePoint::Phase(0, 1)));
    assert_eq!(
        x[0].kind,
        "a call in a quest's own script, reached by a call from a scene fragment"
    );
    // A fragment of another quest's scene does not reach it.
    let mut other = atts.clone();
    other[1].quest = Some(key(0x9a2));
    let t = setters::table_with(&lib, &other, &s, None, &b, true).unwrap();
    assert_eq!(t.of_stage(&key(QQ), 50)[0].need, Need::Unreached);
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [rules::SCENE_RECHECK_VARIABLE, rules::SETTER_OUTER_CALL] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
