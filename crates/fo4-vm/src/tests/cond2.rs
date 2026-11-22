//! The condition evaluators added in the second conditions step
//! (functions 72, 84, 248, 550, 629 and the run-on reading), their
//! switches, caller assumptions and the census's reader paths
//! (`crate::blockers`), over synthetic plugins (no game data).
use super::assemble;
use super::quest::{
    self as q, conds, ctda, group, key, library, record, sub, Owned, BOX, Q1, Q2, STAT,
};
use super::scene::{self as s, QS, SC, SD};
use crate::{
    blockers::{self, Blockers},
    cond::{
        eval_condition, eval_list, eval_one, Combine, Facts, StaticFacts, Switches, Truth, Why,
        NOT_EVALUATED,
    },
    quest::{self, QuestHost, RunFacts},
    rules, vars, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::{
    actor::Context,
    condition::{Condition, Ctda},
    typed::Store,
};
use std::collections::BTreeSet;

const NPC: u32 = 0x8a0;
const SCN: u32 = 0x8b0;
const QX: u32 = 0x8c0;

fn one() -> u32 {
    1f32.to_bits()
}

/// A CTDA with a run-on word, reference word and last word.
fn on(function: u16, value: f32, p1: u32, run_on: u32, reference: u32, last: i32) -> Ctda {
    Ctda {
        byte0: 0,
        unused: [0; 3],
        value: value.to_bits(),
        function,
        pad: [0; 2],
        param1: p1,
        param2: 0,
        run_on,
        reference,
        param3: last,
    }
}

fn cond_with_cis2(c: Vec<u8>, cis2: &str) -> Condition {
    Condition {
        index: 0,
        ctda: Ctda::from_bytes(c[..].try_into().unwrap()),
        cis1: None,
        cis2: Some(cis2.as_bytes().to_vec()),
    }
}

fn ctx(store: &Store) -> Context<'_> {
    Context {
        order: store.order(),
        plugin: 0,
    }
}

fn why(t: &Truth) -> &'static str {
    match t {
        Truth::Undetermined(w) => w[0].why,
        _ => "decided",
    }
}

#[test]
fn function_72_reads_the_base_of_the_reference_or_alias_the_run_on_word_names() {
    let store = q::store();
    let cx = ctx(&store);
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    // Run-on 2: the reference word names the placed box, whose base is STAT.
    let (t, used) = eval_one(&on(72, 1.0, STAT, 2, BOX, -1), cx, "t", &mut f);
    assert_eq!(t, Truth::True);
    assert!(used.contains(rules::COND_FN_72) && used.contains(rules::COND_RUN_ON));
    assert_eq!(
        eval_one(&on(72, 1.0, Q1, 2, BOX, -1), cx, "t", &mut f).0,
        Truth::False
    );
    // A reference word that names no placed reference.
    assert_eq!(
        why(&eval_one(&on(72, 1.0, STAT, 2, STAT, -1), cx, "t", &mut f).0),
        "the reference word names no placed reference"
    );
    // Run-on 5 without an owning quest; with Q1 (runs at a new game,
    // alias 0 forced to the box); with Q2 (does not run: empty alias).
    assert_eq!(
        why(&eval_one(&on(72, 1.0, STAT, 5, 0, 0), cx, "t", &mut f).0),
        "the list has no owning quest"
    );
    f.set_owner(Some(key(Q1)));
    assert_eq!(
        eval_one(&on(72, 1.0, STAT, 5, 0, 0), cx, "t", &mut f).0,
        Truth::True
    );
    f.set_owner(Some(key(Q2)));
    assert_eq!(
        why(&eval_one(&on(72, 1.0, STAT, 5, 0, 0), cx, "t", &mut f).0),
        "the alias holds no reference"
    );
    // Run-on 0: the host's subject, which the interpreter does not know.
    assert_eq!(
        why(&eval_one(&on(72, 1.0, STAT, 0, 0, -1), cx, "t", &mut f).0),
        "the condition runs on a subject the interpreter does not know"
    );
    // The switches: the run-on reading off, then function 72 off.
    let mut sw = Switches::all();
    sw.run_on_off = true;
    let mut g = StaticFacts::new_game(&store, 1 << 24).with_switches(sw);
    assert_eq!(
        why(&eval_one(&on(72, 1.0, STAT, 2, BOX, -1), cx, "t", &mut g).0),
        "the run-on reading is off"
    );
    let mut sw = Switches::all();
    sw.off(72);
    let mut g = StaticFacts::new_game(&store, 1 << 24).with_switches(sw);
    let (t, used) = eval_one(&on(72, 1.0, STAT, 2, BOX, -1), cx, "t", &mut g);
    assert_eq!(
        t,
        Truth::Undetermined(vec![Why {
            function: 72,
            why: NOT_EVALUATED
        }])
    );
    assert!(!used.contains(rules::COND_FN_72) && !used.contains(rules::COND_FLAGS));
}

#[test]
fn function_629_reads_initial_values_and_the_runs_variables() {
    let store = q::store();
    let cx = ctx(&store);
    let c = |cis2: &str, v: f32| cond_with_cis2(ctda(0, 0, 629, v.to_bits(), Q1, 0), cis2);
    let ev = |f: &mut dyn Facts, c: &Condition| eval_condition(c, cx, "t", f, &mut BTreeSet::new());
    // Without compiled scripts: undetermined.
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    assert_eq!(
        why(&ev(&mut f, &c("seenStage", -1.0))),
        "compiled scripts are not read here"
    );
    // With them: QS1's seenStage starts at -1, QF1's stage at its type's
    // default; a name no script declares; the condition without its CIS2.
    let mut f = StaticFacts::new_game(&store, 1 << 24).with_scripts(Box::new(library()));
    assert_eq!(ev(&mut f, &c("seenStage", -1.0)), Truth::True);
    assert_eq!(ev(&mut f, &c("stage", 0.0)), Truth::True);
    assert_eq!(ev(&mut f, &c("stage", 1.0)), Truth::False);
    assert_eq!(
        why(&ev(&mut f, &c("nothing", 0.0))),
        "no script of the quest declares the variable"
    );
    let bare = conds(&[ctda(0, 0, 629, 0, Q1, 0)]);
    assert_eq!(why(&ev(&mut f, &bare[0])), "the condition has no CIS2");
    // In a run: after stage 10, QF1's fragment stored the current stage.
    let b = Budget::new(1 << 24);
    let mut lib = library();
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut attach = crate::bind::RecordScripts::new(&store, 1 << 24);
    let mut vm = Vm::new(&mut lib, &mut host);
    vm.attach = Some(&mut attach);
    quest::start_new_game(&mut vm, &store, &b).unwrap();
    quest::run_jobs(&mut vm, &store, 64, &b);
    let o = quest::quest_object(&mut vm, &store, &key(Q1), &b).unwrap();
    quest::set_stage(&mut vm, &store, o, 10);
    quest::run_jobs(&mut vm, &store, 64, &b);
    let mut world = std::mem::take(&mut vm.world);
    drop(vm);
    let mut rf = RunFacts::of(&host, &mut world, None, Some(key(Q1)));
    assert_eq!(ev(&mut rf, &c("stage", 10.0)), Truth::True);
    assert_eq!(ev(&mut rf, &c("seenStage", 10.0)), Truth::True);
    // Q1's stage 10 fragment set Q2's stage 20, so Q2 is attached: its
    // fragment stored whether stage 20 is done.
    let c2 = |v: f32| cond_with_cis2(ctda(0, 0, 629, v.to_bits(), Q2, 0), "done20");
    assert_eq!(ev(&mut rf, &c2(1.0)), Truth::True);
    // A world where nothing is attached: without a library undetermined,
    // with the host's library the initial value (false).
    let mut fresh = crate::World::default();
    let mut rf = RunFacts::of(&host, &mut fresh, None, None);
    assert_eq!(
        why(&ev(&mut rf, &c2(0.0))),
        "the quest's scripts are not attached in this run"
    );
    let mut host2 = QuestHost::new(&store, 1 << 24);
    host2.cond.scripts = Some(std::cell::RefCell::new(Box::new(library())));
    let mut rf = RunFacts::of(&host2, &mut fresh, None, None);
    assert_eq!(ev(&mut rf, &c2(0.0)), Truth::True);
}

#[test]
fn quest_variables_take_the_vmad_value_then_the_stored_initial_value() {
    use fo4_plugin::vmad::{Property, PropertyValue};
    let mut lib = super::base();
    lib.add(assemble(
        "script VA extends Form
         var Int ::Count_var
         prop Int Count ::Count_var
         var Float rate 2.5
         var Bool flag
         var ObjectReference thing",
    ));
    lib.add(assemble(
        "script VB extends VA
         var Int ::Count_var 4
         prop Int Count ::Count_var",
    ));
    let given = [Property {
        name: b"Count",
        kind: 3,
        flags: 1,
        value: PropertyValue::Int(7),
    }];
    let scripts = |p: &'static [Property<'static>]| vec![("va".to_string(), p)];
    let empty: &[Property<'_>] = &[];
    assert_eq!(
        vars::declared(&mut lib, &[("va".into(), &given[..])], "::count_var"),
        Ok(7.0)
    );
    assert_eq!(
        vars::declared(&mut lib, &scripts(&[]), "::count_var"),
        Ok(0.0)
    );
    assert_eq!(vars::declared(&mut lib, &scripts(&[]), "rate"), Ok(2.5));
    assert_eq!(vars::declared(&mut lib, &scripts(&[]), "flag"), Ok(0.0));
    assert_eq!(
        vars::declared(&mut lib, &scripts(&[]), "thing"),
        Err("the variable holds neither a number nor a truth value")
    );
    // VB and its parent VA both declare ::count_var, with 4 and 0.
    assert_eq!(
        vars::declared(&mut lib, &[("vb".into(), empty)], "::count_var"),
        Err("more than one script of the quest declares the variable")
    );
    assert_eq!(
        vars::declared(&mut lib, &[("missing".into(), empty)], "x"),
        Err("a script of the quest is not in the archives")
    );
    assert_eq!(vars::number(&Val::Bool(true)), Ok(1.0));
    assert!(vars::number(&Val::None).is_err());
}

#[test]
fn scene_functions_read_the_scene_state() {
    let store = s::store();
    let cx = ctx(&store);
    let playing = |sc: u32| ctda(0, 0, 248, one(), sc, 0);
    let action = |sc: u32, a: u32| ctda(0, 0, 550, one(), sc, a);
    let t = |f: &mut dyn Facts, c: Vec<u8>| {
        eval_one(&Ctda::from_bytes(c[..].try_into().unwrap()), cx, "t", f).0
    };
    // New game before any script: nothing plays, no action is complete.
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    assert_eq!(t(&mut f, playing(SC)), Truth::False);
    assert_eq!(t(&mut f, action(SC, 1)), Truth::False);
    assert_eq!(
        why(&t(&mut f, playing(QS))),
        "a parameter names no winning SCEN"
    );
    // A run with scenes on: SC starts with its quest and waits in phase 1;
    // SD does not play; no action is ever complete.
    let b = Budget::new(1 << 24);
    let mut l = s::lib();
    let mut host = QuestHost::new(&store, 1 << 24);
    let mut attach = crate::bind::RecordScripts::new(&store, 1 << 24);
    let mut runner = quest::Runner::new(&store, 1 << 24, 64);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.nested = Some(&mut runner);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    quest::start_new_game(&mut vm, &store, &b).unwrap();
    quest::run_jobs(&mut vm, &store, 64, &b);
    let mut world = std::mem::take(&mut vm.world);
    drop(vm);
    let mut rf = RunFacts::of(&host, &mut world, None, None);
    assert_eq!(t(&mut rf, playing(SC)), Truth::True);
    assert_eq!(t(&mut rf, playing(SD)), Truth::False);
    assert_eq!(t(&mut rf, action(SC, 1)), Truth::False);
    // Scenes off: undetermined; isolated scenario: a scene the run did not
    // touch is undetermined.
    world.scenes.on = false;
    let mut rf = RunFacts::of(&host, &mut world, None, None);
    assert_eq!(why(&t(&mut rf, playing(SC))), "scenes are not run");
    let mut iso = crate::World::default();
    iso.scenes.on = true;
    let mut rf = RunFacts::of(&host, &mut iso, None, None);
    assert_eq!(
        why(&t(&mut rf, playing(SD))),
        "whether the scene plays is undetermined"
    );
}

/// A plugin with one NPC_ and nothing else.
fn npc_store() -> Store {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(1u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(
        0,
        *b"NPC_",
        &record(b"NPC_", NPC, &sub(b"EDID", b"N\0")),
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

#[test]
fn function_84_is_zero_at_a_new_game_and_open_in_a_run() {
    let store = npc_store();
    let cx = ctx(&store);
    let c = Ctda::from_bytes(ctda(0, 0, 84, 0, NPC, 0)[..].try_into().unwrap());
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    let (t, used) = eval_one(&c, cx, "t", &mut f);
    assert_eq!(t, Truth::True);
    assert!(used.contains(rules::COND_FN_84));
    let host = QuestHost::new(&store, 1 << 24);
    let mut w = crate::World::default();
    let mut rf = RunFacts::of(&host, &mut w, None, None);
    assert_eq!(
        why(&eval_one(&c, cx, "t", &mut rf).0),
        "deaths are not kept by a run"
    );
}

#[test]
fn switches_off_give_the_earlier_answers_and_assumptions_decide_a_function() {
    let store = q::store();
    let cx = ctx(&store);
    // Every added index off: each is the earlier "not evaluated".
    let none = Switches::none();
    assert!(!none.is_all() && Switches::all().is_all());
    assert_eq!(
        none.describe(),
        "function 72 off, function 84 off, function 248 off, function 550 off, function 629 off, run-on reading off"
    );
    let mut f = StaticFacts::new_game(&store, 1 << 24).with_switches(none);
    for (n, _) in crate::cond::ADDED {
        let c = Ctda::from_bytes(ctda(0, 0, n, one(), Q1, 0)[..].try_into().unwrap());
        assert_eq!(
            eval_one(&c, cx, "t", &mut f).0,
            Truth::Undetermined(vec![Why {
                function: n,
                why: NOT_EVALUATED
            }]),
            "{n}"
        );
    }
    // A caller assumption: function 555 takes the value 1.
    let mut host = QuestHost::new(&store, 1 << 24);
    host.cond.assume.insert(555, 1.0);
    let mut w = crate::World::default();
    let list = conds(&[ctda(0, 0, 555, one(), 0, 0), ctda(0, 0, 555, 0, 0, 0)]);
    let mut rf = RunFacts::of(&host, &mut w, None, None);
    assert_eq!(
        eval_list(&list[..1], cx, "t", &mut rf, Combine::OrGroups),
        Truth::True
    );
    assert_eq!(
        eval_list(&list, cx, "t", &mut rf, Combine::OrGroups),
        Truth::False
    );
    assert_eq!(host.cond.assumed.get(), 3);
}

/// A quest with a conditioned log entry, an alias with a condition and
/// conditions after NEXT, and a scene of that quest with a start and a
/// completion condition.
fn census_store() -> Store {
    let c = |f: u16| sub(b"CTDA", &ctda(0, 0, f, one(), 0, 0));
    let mut d = 0u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut indx = 10u16.to_le_bytes().to_vec();
    indx.extend([0, 0]);
    let qx = [
        sub(b"EDID", b"QX\0"),
        sub(b"DNAM", &d),
        sub(b"NEXT", &[]),
        c(555),
        c(1),
        sub(b"INDX", &indx),
        sub(b"QSDT", &[0]),
        c(629),
        sub(b"ANAM", &1u32.to_le_bytes()),
        sub(b"ALST", &0u32.to_le_bytes()),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &[0; 4]),
        c(72),
        c(14),
        sub(b"ALED", &[]),
    ]
    .concat();
    let w = |v: u32| v.to_le_bytes().to_vec();
    let scn = [
        sub(b"EDID", b"SX\0"),
        sub(b"FNAM", &w(0)),
        sub(b"HNAM", &[]),
        sub(b"NAM0", b"\0"),
        c(1),
        sub(b"NEXT", &[]),
        c(555),
        sub(b"NEXT", &[]),
        sub(b"WNAM", &w(500)),
        sub(b"HNAM", &[]),
        sub(b"PNAM", &w(QX)),
        sub(b"INAM", &w(0)),
    ]
    .concat();
    let body = [
        record(b"QUST", QX, &qx),
        group(10, QX.to_le_bytes(), &record(b"SCEN", SCN, &scn)),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(2u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(0, *b"QUST", &body));
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
fn the_census_reads_lists_by_context_and_names_what_alone_blocks_them() {
    let store = census_store();
    let b = Budget::new(1 << 24);
    let ql = blockers::of_quest(&store, &key(QX), &b).unwrap();
    let parts: Vec<(&str, String, usize)> = ql
        .iter()
        .map(|l| (l.context, l.place.clone(), l.list.len()))
        .collect();
    assert_eq!(
        parts,
        [
            ("quest stage log entry", "stage 10 entry 0".to_string(), 1),
            ("quest alias fill", "alias 0".to_string(), 2),
            (
                "quest story event (after NEXT)",
                "after NEXT".to_string(),
                2
            ),
        ]
    );
    assert!(ql
        .iter()
        .all(|l| l.owner == Some(key(QX)) && l.host == *b"QUST"));
    let sl = blockers::of_scene(&store, &key(SCN), &b).unwrap();
    assert_eq!(sl.len(), 2);
    assert_eq!(
        (sl[0].context, sl[1].context),
        ("scene phase start", "scene phase completion")
    );
    assert_eq!(sl[0].owner, Some(key(QX)));
    // Not a scene, not a quest: nothing.
    assert!(blockers::of_scene(&store, &key(QX), &b).unwrap().is_empty());
    assert!(blockers::of_quest(&store, &key(SCN), &b)
        .unwrap()
        .is_empty());
    // A node with conditions, one without.
    let node = |conditions: Vec<Condition>| fo4_plugin::story::Node {
        kind: fo4_plugin::story::Kind::Quest,
        plugin: 0,
        file: "test.esm".into(),
        editor_id: None,
        parent: fo4_plugin::typed::FormRef {
            raw: 0,
            key: None,
            subrecord: 0,
        },
        previous: fo4_plugin::typed::FormRef {
            raw: 0,
            key: None,
            subrecord: 0,
        },
        condition_count: conditions.len() as u32,
        conditions,
        dnam: 0,
        xnam: 0,
        event: None,
        mnam: None,
        hnam: None,
        quest_count: None,
        quests: Vec::new(),
        fields: Vec::new(),
    };
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(key(0x9a0), node(conds(&[ctda(0, 0, 77, 0, 0, 0)])));
    nodes.insert(key(0x9a1), node(Vec::new()));
    let tree = fo4_plugin::story::Tree::from_nodes(nodes, Vec::new());
    let tl = blockers::of_tree(&tree);
    assert_eq!(tl.len(), 1);
    assert_eq!(
        (tl[0].context, tl[0].host),
        ("story manager node", *b"SMQN")
    );
    // Blockers: the log entry (629 without scripts: open evaluated); the
    // alias (72 on the subject: open, 14 not evaluated); after NEXT (555
    // and 1, two not evaluated); the scene's start (1 alone).
    let mut f = StaticFacts::new_game(&store, 1 << 24);
    let mut bl = |l: &blockers::ListAt| {
        f.set_owner(l.owner.clone());
        Blockers::of(&eval_list(
            &l.list,
            ctx(&store),
            "t",
            &mut f,
            Combine::OrGroups,
        ))
    };
    let entry = bl(&ql[0]).unwrap();
    assert_eq!(entry.sole(), None);
    assert_eq!(
        entry.shape(),
        "only evaluated functions whose input the state leaves open"
    );
    assert_eq!(entry.open_functions(), BTreeSet::from([629]));
    let alias = bl(&ql[1]).unwrap();
    assert_eq!(alias.sole(), None);
    assert_eq!(alias.not_evaluated, BTreeSet::from([14]));
    assert_eq!(
        alias.shape(),
        "one function index not evaluated, with open evaluated functions"
    );
    let next = bl(&ql[2]).unwrap();
    assert_eq!(next.shape(), "two or more function indexes not evaluated");
    assert_eq!(next.sole(), None);
    assert_eq!(bl(&sl[0]).unwrap().sole(), Some(1));
    assert_eq!(bl(&sl[1]).unwrap().sole(), Some(555));
    assert_eq!(Blockers::of(&Truth::True), None);
}
