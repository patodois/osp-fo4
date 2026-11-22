//! The readings of the second opening step: array opcodes on a none array
//! (`pex_array_none_*_v1`) and the TIQS of the line a timed dialogue
//! action plays (`scene_line_stage_fields_v1`), over synthetic scripts and
//! a synthetic plugin (no game data).
use super::quest::{group, record, sub, Owned};
use super::{assemble, base};
use crate::{
    quest::{self, Change, QuestHost},
    rules,
    scene::{self, Change as SceneChange},
    voice, NoHost, NoneArrays, Outcome, Val, Vm, VmError,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use std::rc::Rc;

/// One function per array opcode family, each on a local array never
/// assigned (none).
fn none_lib() -> crate::MapLibrary {
    let mut lib = base();
    lib.add(assemble(
        "script NoneArr
         struct Pair
         member Int a
         func Len Int global
         local Int[] a
         local Int n
         31 n a
         26 n
         func ReadInt Int global
         local Int[] a
         local Int x
         32 x a 0
         26 x
         func ReadText String global
         local String[] a
         local String x
         32 x a 3
         26 x
         func ReadObject ObjectReference global
         local ObjectReference[] a
         local ObjectReference x
         32 x a 0
         26 x
         func Find Int global
         local Int[] a
         local Int k
         34 a k 5 0
         26 k
         func FindMember Int global
         local NoneArr#Pair[] a
         local Int k
         40 a k \"a\" 5 0
         26 k
         func Change Int global
         local Int[] a
         local Int n
         42 a 9 1
         43 a 4 0
         33 a 0 1
         45 a 0 1
         46 a
         31 n a
         26 n",
    ));
    lib
}

fn call(lib: &mut crate::MapLibrary, on: NoneArrays, name: &str) -> (Outcome, Ran) {
    let f = lib.scripts["nonearr"].function("", name).unwrap().clone();
    let mut host = NoHost;
    let mut vm = Vm::new(lib, &mut host);
    vm.none_arrays = on;
    let o = vm.run(None, f, vec![]);
    (
        o,
        Ran {
            rules: vm.rules.clone(),
            arrays: vm.world.arrays.len(),
        },
    )
}

/// What a run relied on and made.
struct Ran {
    rules: std::collections::BTreeSet<&'static str>,
    arrays: usize,
}

fn failed_on_none(o: &Outcome) -> bool {
    matches!(o, Outcome::Failed(VmError::NoneOperand { .. }))
}

#[test]
fn array_opcodes_on_none_keep_the_earlier_reading_with_every_switch_off() {
    let off = NoneArrays::default();
    let (o, vm) = call(&mut none_lib(), off, "len");
    assert!(matches!(o, Outcome::Completed(Val::Unknown(_))), "{o:?}");
    assert!(vm.rules.contains(rules::ARRAYS));
    assert!(!vm.rules.contains(rules::NONE_ARRAY_LENGTH));
    for f in ["readint", "find", "findmember", "change"] {
        let (o, vm) = call(&mut none_lib(), off, f);
        assert!(failed_on_none(&o), "{f}: {o:?}");
        for r in [
            rules::NONE_ARRAY_READ,
            rules::NONE_ARRAY_FIND,
            rules::NONE_ARRAY_CHANGE,
        ] {
            assert!(!vm.rules.contains(r), "{f} {r}");
        }
    }
}

#[test]
fn the_length_of_none_is_zero_under_its_rule_alone() {
    let on = NoneArrays {
        length: true,
        ..NoneArrays::default()
    };
    let (o, vm) = call(&mut none_lib(), on, "len");
    assert_eq!(o, Outcome::Completed(Val::Int(0)));
    assert!(vm.rules.contains(rules::NONE_ARRAY_LENGTH));
    // The other opcodes still fail: each rule has its own switch.
    let (o, _) = call(&mut none_lib(), on, "readint");
    assert!(failed_on_none(&o));
}

#[test]
fn an_element_of_none_is_the_element_types_default() {
    let on = NoneArrays {
        read: true,
        ..NoneArrays::default()
    };
    let (o, vm) = call(&mut none_lib(), on, "readint");
    assert_eq!(o, Outcome::Completed(Val::Int(0)));
    assert!(vm.rules.contains(rules::NONE_ARRAY_READ));
    let (o, _) = call(&mut none_lib(), on, "readtext");
    assert_eq!(o, Outcome::Completed(Val::Str(String::new())));
    let (o, _) = call(&mut none_lib(), on, "readobject");
    assert_eq!(o, Outcome::Completed(Val::None));
}

#[test]
fn a_find_in_none_gives_minus_one() {
    let on = NoneArrays {
        find: true,
        ..NoneArrays::default()
    };
    for f in ["find", "findmember"] {
        let (o, vm) = call(&mut none_lib(), on, f);
        assert_eq!(o, Outcome::Completed(Val::Int(-1)), "{f}");
        assert!(vm.rules.contains(rules::NONE_ARRAY_FIND));
    }
}

#[test]
fn changing_none_does_nothing_and_makes_no_array() {
    // Without the length rule the run goes on through every change and
    // stops on the length of what is still none.
    let on = NoneArrays {
        change: true,
        ..NoneArrays::default()
    };
    let (o, vm) = call(&mut none_lib(), on, "change");
    assert!(matches!(o, Outcome::Completed(Val::Unknown(_))), "{o:?}");
    assert!(vm.rules.contains(rules::NONE_ARRAY_CHANGE));
    assert!(vm.arrays == 0);
    let all = NoneArrays {
        change: true,
        length: true,
        ..NoneArrays::default()
    };
    let (o, _) = call(&mut none_lib(), all, "change");
    assert_eq!(o, Outcome::Completed(Val::Int(0)));
}

// ---- the TIQS of the line a timed dialogue action plays -------------------

const QS: u32 = 0x900;
const SC: u32 = 0x901;
const D1: u32 = 0x920;
const L1: u32 = 0x921;
const D2: u32 = 0x922;
const L2: u32 = 0x923;
const L3: u32 = 0x924;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn pair(a: i16, b: i16) -> Vec<u8> {
    [a.to_le_bytes(), b.to_le_bytes()].concat()
}
fn stage(n: u16) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([0, 0]);
    [sub(b"INDX", &d), sub(b"QSDT", &[0])].concat()
}
fn phase() -> Vec<u8> {
    [
        sub(b"HNAM", &[]),
        sub(b"NAM0", b"\0"),
        sub(b"NEXT", &[]),
        sub(b"NEXT", &[]),
        sub(b"WNAM", &w(500)),
        sub(b"HNAM", &[]),
    ]
    .concat()
}
fn line_action(index: u32, p: u32, topic: u32) -> Vec<u8> {
    [
        sub(b"ANAM", &[0, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &(-1i32).to_le_bytes()),
        sub(b"INAM", &w(index)),
        sub(b"SNAM", &w(p)),
        sub(b"ENAM", &w(p)),
        sub(b"DATA", &w(topic)),
        sub(b"ANAM", &[]),
    ]
    .concat()
}

/// QS starts with the game (stages 10, 20, 30, 40). Its scene SC (FNAM
/// 0x1) has two phases: phase 0 speaks topic D1, whose only line L1 (no
/// conditions) has TIQS 10 at its begin and 20 at its end; phase 1 speaks
/// topic D2, whose two lines L2 (TIQS 30, -1) and L3 (TIQS 40, -1) both
/// hold. No line has a voice file: the caller assumes their seconds.
fn store() -> Store {
    let sc = [
        sub(b"EDID", b"SC\0"),
        sub(b"FNAM", &w(0x1)),
        phase(),
        phase(),
        sub(b"ALID", &w(0)),
        sub(b"LNAM", &w(0)),
        sub(b"DNAM", &w(0)),
        line_action(1, 0, D1),
        line_action(2, 1, D2),
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(3)),
    ]
    .concat();
    let info = |a: i16, b: i16| [sub(b"ENAM", &w(0)), sub(b"TIQS", &pair(a, b))].concat();
    let dial = |e: &[u8]| [sub(b"EDID", e), sub(b"TIFC", &w(1))].concat();
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut qs = [sub(b"EDID", b"QS\0"), sub(b"DNAM", &d)].concat();
    for n in [10, 20, 30, 40] {
        qs.extend(stage(n));
    }
    let children = [
        record(b"SCEN", SC, &sc),
        record(b"DIAL", D1, &dial(b"D1\0")),
        group(7, D1.to_le_bytes(), &record(b"INFO", L1, &info(10, 20))),
        record(b"DIAL", D2, &dial(b"D2\0")),
        group(
            7,
            D2.to_le_bytes(),
            &[
                record(b"INFO", L2, &info(30, -1)),
                record(b"INFO", L3, &info(40, -1)),
            ]
            .concat(),
        ),
    ]
    .concat();
    let quests = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &children),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(5u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(0, *b"QUST", &quests));
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

/// A new game with the clock (every line lasts 2 s by assumption), the
/// line fields as asked, the clock advanced until nothing is due. Returns
/// the stages set with the real-time second of each, the scene's line
/// field log and whether the rule was relied on.
fn run(fields: bool, first: bool) -> (Vec<(i32, f64)>, Vec<String>, bool) {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = super::scene::lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    vm.world.scenes.line_fields = fields;
    vm.world.scenes.clock = scene::Clock {
        on: true,
        timers: true,
        voice: Some(Rc::new(voice::Voice {
            topics: voice::Topics::read(&s),
            ..voice::Voice::default()
        })),
        assume_line_seconds: Some(2.0),
        assume_first_line: first,
        ..scene::Clock::default()
    };
    let mut stages = Vec::new();
    let mut seen = 0;
    let note = |vm: &Vm<'_>, seen: &mut usize, out: &mut Vec<(i32, f64)>| {
        let now = vm.world.now[crate::world::Clock::Real as usize];
        for (_, c) in &vm.world.quests.log[*seen..] {
            if let Change::StageSet(n) = c {
                out.push((*n, now));
            }
        }
        *seen = vm.world.quests.log.len();
    };
    quest::start_new_game(&mut vm, &s, &b).unwrap();
    quest::run_jobs(&mut vm, &s, 256, &b);
    note(&vm, &mut seen, &mut stages);
    for _ in 0..8 {
        if scene::advance_clock(&mut vm.world, None).is_none() {
            break;
        }
        quest::run_jobs(&mut vm, &s, 256, &b);
        note(&vm, &mut seen, &mut stages);
    }
    let log = vm
        .world
        .scenes
        .log
        .iter()
        .filter_map(|(_, c)| match c {
            SceneChange::LineField(_, what, _) => Some(what.clone()),
            _ => None,
        })
        .collect();
    let used = vm.host.rules().contains(&rules::LINE_STAGE_FIELDS)
        || vm.rules.contains(rules::LINE_STAGE_FIELDS);
    (stages, log, used)
}

#[test]
fn no_line_sets_a_stage_without_the_switch() {
    let (stages, log, used) = run(false, false);
    assert!(stages.iter().all(|x| x.0 == 0 || x.0 < 10), "{stages:?}");
    assert!(log.is_empty());
    assert!(!used);
}

#[test]
fn the_only_line_that_holds_sets_its_begin_then_its_end_on_the_clock() {
    let (stages, log, used) = run(true, false);
    assert!(used);
    assert!(stages.contains(&(10, 0.0)), "{stages:?}");
    assert!(stages.contains(&(20, 2.0)), "{stages:?}");
    // The second topic holds two lines: undetermined, nothing set.
    assert!(!stages.iter().any(|x| x.0 == 30 || x.0 == 40), "{stages:?}");
    assert!(log.contains(&"line determined: the only line of its topic that holds".to_string()));
    assert!(log.contains(&"line undetermined: several lines of its topic hold".to_string()));
    assert!(log.contains(&"end half set on the clock".to_string()));
}

#[test]
fn several_lines_need_the_callers_assumption_of_the_first() {
    let (stages, log, _) = run(true, true);
    assert!(stages.contains(&(30, 2.0)), "{stages:?}");
    assert!(!stages.iter().any(|x| x.0 == 40), "{stages:?}");
    assert!(log.iter().any(|x| x.starts_with("line assumed")));
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::NONE_ARRAY_LENGTH,
        rules::NONE_ARRAY_READ,
        rules::NONE_ARRAY_FIND,
        rules::NONE_ARRAY_CHANGE,
        rules::LINE_STAGE_FIELDS,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
