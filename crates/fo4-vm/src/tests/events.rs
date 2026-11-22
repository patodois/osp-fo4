//! Remote and custom events, and the story manager's script event, over a
//! synthetic plugin (no game data).
use super::assemble;
use super::quest::{key, library, object, record, sub, text, Owned};
use crate::{
    events::CustomOrder,
    quest::{self, Job, QuestHost},
    rules, Outcome, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use std::rc::Rc;

const QA: u32 = 0x900;
const QB: u32 = 0x910;
const KW: u32 = 0x920;
const QC: u32 = 0x930;
const EV: u32 = 0x940;
const QN: u32 = 0x941;

/// A quest VMAD: own scripts with object properties, then a block of
/// `script` (empty: no fragment script) with its fragments.
fn vmad(own: &[(&str, &[(&str, u32)])], script: &str, frags: &[(u16, &str)]) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0];
    v.extend((own.len() as u16).to_le_bytes());
    for (name, props) in own {
        text(&mut v, name);
        v.push(0);
        v.extend((props.len() as u16).to_le_bytes());
        for (p, id) in props.iter() {
            object(&mut v, p, -1, *id);
        }
    }
    v.push(3);
    v.extend((frags.len() as u16).to_le_bytes());
    text(&mut v, script);
    if !script.is_empty() {
        v.push(0);
        v.extend(0u16.to_le_bytes());
    }
    for (stage, f) in frags {
        v.extend(stage.to_le_bytes());
        v.extend([0, 0]);
        v.extend(0i32.to_le_bytes());
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
fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// QA starts with the game and its script registers for QB's OnStageSet
/// and QB's custom event; QB's stage 10 fragment sends the custom event;
/// QC is listed by a quest node under the script event whose condition
/// (function 576, selector 0, K1) asks for keyword KW.
fn store() -> Store {
    let qa = [
        sub(b"EDID", b"QA\0"),
        sub(
            b"VMAD",
            &vmad(&[("QAS", &[("QB", QB), ("KW", KW)])], "", &[]),
        ),
        dnam(0x0001),
        sub(b"NEXT", &[]),
    ]
    .concat();
    let mut indx = 10u16.to_le_bytes().to_vec();
    indx.extend([0, 0]);
    let qb = [
        sub(b"EDID", b"QB\0"),
        sub(
            b"VMAD",
            &vmad(
                &[("QBS", &[])],
                "QFB",
                &[(10, "Fragment_Stage_0010_Item_00")],
            ),
        ),
        dnam(0),
        sub(b"INDX", &indx),
        sub(b"QSDT", &[0]),
    ]
    .concat();
    let qc = [
        sub(b"EDID", b"QC\0"),
        sub(b"VMAD", &vmad(&[("QCS", &[])], "", &[])),
        dnam(0),
        sub(b"ENAM", b"SCPT"),
        sub(b"NEXT", &[]),
    ]
    .concat();
    let ev = [
        sub(b"PNAM", &w(0x5b)),
        sub(b"SNAM", &w(0)),
        sub(b"CITC", &w(0)),
        sub(b"DNAM", &w(0)),
        sub(b"XNAM", &w(0)),
        sub(b"ENAM", b"SCPT"),
    ]
    .concat();
    let cond = fo4_plugin::condition::Ctda {
        byte0: 0,
        unused: [0; 3],
        value: 1f32.to_bits(),
        function: 576,
        pad: [0; 2],
        param1: u32::from_le_bytes([0, 0, b'K', b'1']),
        param2: KW,
        run_on: 0,
        reference: 0,
        param3: -1,
    }
    .to_bytes();
    let qn = [
        sub(b"EDID", b"N\0"),
        sub(b"PNAM", &w(EV)),
        sub(b"SNAM", &w(0)),
        sub(b"CITC", &w(1)),
        sub(b"CTDA", &cond),
        sub(b"DNAM", &w(0)),
        sub(b"XNAM", &w(0)),
        sub(b"QNAM", &w(1)),
        sub(b"NNAM", &w(QC)),
    ]
    .concat();
    let mut body = Vec::new();
    body.extend(record(b"QUST", QA, &qa));
    body.extend(record(b"QUST", QB, &qb));
    body.extend(record(b"QUST", QC, &qc));
    body.extend(record(b"KYWD", KW, &sub(b"EDID", b"KW\0")));
    body.extend(record(b"SMEN", EV, &ev));
    body.extend(record(b"SMQN", QN, &qn));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(6u32.to_le_bytes());
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

fn lib() -> crate::MapLibrary {
    let mut l = library();
    l.add(assemble(
        "script ScriptObject
         func OnInit none
         func RegisterForRemoteEvent Bool native ScriptObject akEventSource String asEventName
         func RegisterForCustomEvent none native ScriptObject akSender String asEventName
         func SendCustomEvent none native String asEvent Var[] akArgs",
    ));
    l.add(assemble(
        "script Keyword extends Form
         func SendStoryEventAndWait Bool native Location akLoc ObjectReference akRef1 ObjectReference akRef2 Int aiValue1 Int aiValue2",
    ));
    l.add(assemble("script Location extends Form"));
    l.add(assemble(
        "script QAS extends Quest
         var Quest ::QB_var
         prop Quest QB ::QB_var
         var Keyword ::KW_var
         prop Keyword KW ::KW_var
         var Int seen -1
         var Bool pinged false
         var Bool started false
         func OnQuestInit none
         local Bool ::b
         local none ::n
         23 RegisterForRemoteEvent self ::b 2 ::QB_var \"OnStageSet\"
         23 RegisterForCustomEvent self ::n 2 ::QB_var \"QBS_Ping\"
         func ::remote_Quest_OnStageSet none Quest akSender Int auiStageID Int auiItemID
         13 seen auiStageID
         func ::remote_QBS_Ping none QBS akSender Var[] akArgs
         13 pinged true
         func Ask none
         23 SendStoryEventAndWait ::KW_var started 5 none none none 7 0",
    ));
    l.add(assemble("script QBS extends Quest"));
    l.add(assemble(
        "script QFB extends Quest
         func Fragment_Stage_0010_Item_00 none
         local none ::n
         23 SendCustomEvent self ::n 2 \"QBS_Ping\" none",
    ));
    l.add(assemble(
        "script QCS extends Quest
         var Int got -1
         func OnStoryScript none Keyword akKeyword Location akLocation ObjectReference akRef1 ObjectReference akRef2 Int aiValue1 Int aiValue2
         13 got aiValue1",
    ));
    l
}

fn own(vm: &Vm<'_>, o: usize, script: &str, name: &str) -> Val {
    let i = vm.world.objects[o]
        .instances
        .iter()
        .copied()
        .find(|i| vm.world.instances[*i].script == script)
        .expect("instance");
    vm.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .value
        .clone()
}

/// Starts a new game with the events and story switches, sets QB's stage
/// 10 and returns the jobs run after it.
fn game(
    store: &Store,
    remote: bool,
    custom: Option<CustomOrder>,
    f: &dyn Fn(&mut Vm<'_>, Vec<quest::JobRun>),
) {
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(store, 1 << 24);
    host.story = Some(Rc::new(fo4_plugin::story::Tree::read(store, &b).unwrap()));
    let mut attach = crate::bind::RecordScripts::new(store, 1 << 24);
    let mut runner = quest::Runner::new(store, 1 << 24, 64);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.nested = Some(&mut runner);
    vm.world.events.remote = remote;
    vm.world.events.custom = custom;
    quest::start_new_game(&mut vm, store, &b).unwrap();
    quest::run_jobs(&mut vm, store, 64, &b);
    let qb = quest::quest_object(&mut vm, store, &key(QB), &b).unwrap();
    quest::set_stage(&mut vm, store, qb, 10);
    let mut runs = quest::run_jobs(&mut vm, store, 64, &b);
    runs.append(&mut vm.world.quests.nested_runs);
    runs.sort_by_key(|r| r.seq);
    f(&mut vm, runs);
}

#[test]
fn remote_and_custom_events_reach_the_registered_instance() {
    let store = store();
    let b = Budget::new(1 << 24);
    // Queued: the custom event runs after the stage's fragment and its
    // OnStageSet, as a job of its own.
    game(&store, true, Some(CustomOrder::Queued), &|vm, runs| {
        let qa = quest::quest_object(vm, &store, &key(QA), &b).unwrap();
        assert_eq!(own(vm, qa, "qas", "seen"), Val::Int(10));
        assert_eq!(own(vm, qa, "qas", "pinged"), Val::Bool(true));
        let stage = runs
            .iter()
            .position(|r| matches!(r.job, Job::StageEvent { stage: 10, .. }))
            .unwrap();
        let custom = runs
            .iter()
            .position(|r| matches!(r.job, Job::Custom { .. }))
            .unwrap();
        assert!(custom > stage);
        assert!(matches!(
            runs[stage].remote[..],
            [(_, Outcome::Completed(_))]
        ));
        assert!(matches!(runs[custom].job, Job::Custom { depth: 2, .. }));
        for r in [
            rules::REMOTE_EVENTS,
            rules::CUSTOM_EVENTS,
            rules::CUSTOM_EVENT_ORDER,
        ] {
            assert!(vm.rules.contains(r), "{r}");
        }
    });
    // Nested: the custom event runs inside the fragment, before
    // OnStageSet.
    game(&store, true, Some(CustomOrder::Nested), &|_, runs| {
        let stage = runs
            .iter()
            .position(|r| matches!(r.job, Job::StageEvent { stage: 10, .. }))
            .unwrap();
        let custom = runs
            .iter()
            .position(|r| matches!(r.job, Job::Custom { .. }))
            .unwrap();
        assert!(custom < stage);
    });
    // Off: nothing is delivered and the registrations are only recorded.
    game(&store, false, None, &|vm, runs| {
        let qa = quest::quest_object(vm, &store, &key(QA), &b).unwrap();
        assert_eq!(own(vm, qa, "qas", "seen"), Val::Int(-1));
        assert_eq!(own(vm, qa, "qas", "pinged"), Val::Bool(false));
        assert!(runs.iter().all(|r| r.remote.is_empty()));
        assert!(vm.world.events.registrations.is_empty());
    });
}

#[test]
fn a_script_event_starts_the_quest_its_node_lists() {
    let store = store();
    let b = Budget::new(1 << 24);
    game(&store, true, Some(CustomOrder::Queued), &|vm, _| {
        let qa = quest::quest_object(vm, &store, &key(QA), &b).unwrap();
        let inst = vm.world.objects[qa].instances[0];
        let o = vm.deliver(inst, "ask", Vec::new());
        assert!(matches!(o, Outcome::Completed(_)), "{o:?}");
        assert_eq!(own(vm, qa, "qas", "started"), Val::Bool(true));
        let qc = quest::quest_object(vm, &store, &key(QC), &b).unwrap();
        assert_eq!(own(vm, qc, "qcs", "got"), Val::Int(7));
        assert!(vm
            .world
            .quests
            .log
            .iter()
            .any(|(q, c)| *q == qc && *c == quest::Change::StartedByStory));
        for r in [
            rules::SM_SCRIPT_EVENT,
            rules::SM_TREE_WALK,
            rules::SM_QUEST_START,
            rules::COND_FN_576,
        ] {
            assert!(vm.rules.contains(r), "{r}");
        }
        // QC runs now: the node has nothing more to start.
        vm.deliver(inst, "ask", Vec::new());
        assert_eq!(own(vm, qa, "qas", "started"), Val::Bool(false));
    });
}

#[test]
fn function_576_reads_only_the_event_members_it_names() {
    use crate::cond::{eval_one, Facts, Truth};
    use fo4_plugin::{actor::Context, condition::Ctda, FormKey};
    struct F<'s>(&'s Store, Option<FormKey>);
    impl Facts for F<'_> {
        fn store(&self) -> &Store {
            self.0
        }
        fn quest_running(&mut self, _: &FormKey) -> Result<bool, &'static str> {
            Err("-")
        }
        fn quest_stage(&mut self, _: &FormKey) -> Result<i32, &'static str> {
            Err("-")
        }
        fn stage_done(&mut self, _: &FormKey, _: i32) -> Result<bool, &'static str> {
            Err("-")
        }
        fn global(&mut self, _: &FormKey) -> Result<f32, &'static str> {
            Err("-")
        }
        fn event_member(&mut self, code: [u8; 2]) -> Result<Option<FormKey>, &'static str> {
            match &code {
                b"K1" => Ok(self.1.clone()),
                _ => Err("-"),
            }
        }
    }
    let store = store();
    let ctx = Context {
        order: store.order(),
        plugin: 0,
    };
    let c = |sel: u16, code: &[u8; 2]| Ctda {
        byte0: 0,
        unused: [0; 3],
        value: 1f32.to_bits(),
        function: 576,
        pad: [0; 2],
        param1: u32::from_le_bytes([sel as u8, 0, code[0], code[1]]),
        param2: KW,
        run_on: 0,
        reference: 0,
        param3: -1,
    };
    let mut f = F(&store, Some(key(KW)));
    assert_eq!(eval_one(&c(0, b"K1"), ctx, "t", &mut f).0, Truth::True);
    let mut g = F(&store, Some(key(QA)));
    assert_eq!(eval_one(&c(0, b"K1"), ctx, "t", &mut g).0, Truth::False);
    assert!(matches!(
        eval_one(&c(3, b"K1"), ctx, "t", &mut f).0,
        Truth::Undetermined(_)
    ));
    assert!(matches!(
        eval_one(&c(0, b"V1"), ctx, "t", &mut f).0,
        Truth::Undetermined(_)
    ));
    // Outside a story event (the default): undetermined.
    let mut s = crate::cond::StaticFacts::new_game(&store, 1 << 24);
    assert!(matches!(
        eval_one(&c(0, b"K1"), ctx, "t", &mut s).0,
        Truth::Undetermined(_)
    ));
}
