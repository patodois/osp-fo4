//! The stage-setter table, the player-event assumptions, and the scene
//! readings behind switches of the stage-setters step (stage fields, the
//! recheck of stopped scenes, function 550 from the clock, start-scene
//! actions, location alias fills), over a synthetic plugin (no game data).
use super::assemble;
use super::quest::{ctda, group, key, object, record, sub, text, Owned};
use crate::{
    bind,
    player::{self, Action, Receiver},
    quest::{self, Change, QuestHost},
    rules,
    scene::{self, Change as SceneChange},
    setters::{self, Attachment, Member, Need, On, Part, Resolved, ScenePoint, Slot},
    MapLibrary, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use std::collections::BTreeMap;

const QS: u32 = 0x900;
const CELL: u32 = 0x930;
const LOC: u32 = 0x940;
const LT: u32 = 0x941;
const ACT: u32 = 0x950;
const STAT: u32 = 0x951;
const TRIG: u32 = 0x952;
const R1: u32 = 0x960;
const R2: u32 = 0x961;
const R3: u32 = 0x962;
const R4: u32 = 0x963;
const SC: u32 = 0x970;
const SD: u32 = 0x971;
const SE: u32 = 0x972;
const SF: u32 = 0x973;
const DL: u32 = 0x980;
const IN: u32 = 0x981;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn pair(a: i16, b: i16) -> Vec<u8> {
    [a.to_le_bytes(), b.to_le_bytes()].concat()
}
fn int(v: &mut Vec<u8>, name: &str, n: i32) {
    text(v, name);
    v.extend([3, 1]);
    v.extend(n.to_le_bytes());
}
/// A script entry of a VMAD list: name, flags, no properties.
fn bare(v: &mut Vec<u8>, name: &str) {
    text(v, name);
    v.push(0);
    v.extend(0u16.to_le_bytes());
}
fn stage(n: u16, flags: u8) -> Vec<u8> {
    let mut d = n.to_le_bytes().to_vec();
    d.extend([flags, 0]);
    [sub(b"INDX", &d), sub(b"QSDT", &[0])].concat()
}
/// A phase with completion conditions and an optional SCQS.
fn phase(completion: &[Vec<u8>], scqs: Option<(i16, i16)>) -> Vec<u8> {
    let mut v = [sub(b"HNAM", &[]), sub(b"NAM0", b"\0"), sub(b"NEXT", &[])].concat();
    for c in completion {
        v.extend(sub(b"CTDA", c));
    }
    v.extend(sub(b"NEXT", &[]));
    v.extend(sub(b"WNAM", &w(500)));
    if let Some((a, b)) = scqs {
        v.extend(sub(b"SCQS", &pair(a, b)));
    }
    v.extend(sub(b"HNAM", &[]));
    v
}
fn action(kind: u8, index: u32, p: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut v = [
        sub(b"ANAM", &[kind, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &(-1i32).to_le_bytes()),
        sub(b"INAM", &w(index)),
        sub(b"SNAM", &w(p)),
        sub(b"ENAM", &w(p)),
    ]
    .concat();
    for f in fields {
        v.extend(f);
    }
    v.extend(sub(b"ANAM", &[]));
    v
}
fn scene(id: u32, edid: &str, flags: u32, body: &[Vec<u8>], tail: &[Vec<u8>]) -> Vec<u8> {
    let mut v = sub(b"EDID", format!("{edid}\0").as_bytes());
    v.extend(sub(b"FNAM", &w(flags)));
    for x in body {
        v.extend(x);
    }
    v.extend(sub(b"PNAM", &w(QS)));
    v.extend(sub(b"INAM", &w(9)));
    for x in tail {
        v.extend(x);
    }
    record(b"SCEN", id, &v)
}

/// QS starts with the game: stages 0 (startup), 10, 20, 30, 40; alias 0
/// forced to R1, alias 1 filled from location alias 2 (forced to LOC) by
/// the ref type LT; its own script QS, the fragment script QF (stage 10),
/// AliasAct on aliases 0 and 1. ACT (ACTI) carries Act (MyQuest QS,
/// Stage 20); R1 places ACT, R2 a static, R3 a trigger with its own Trig
/// (MyQuest QS). LOC lists R2 under LT. Scenes of QS: SC (with the quest;
/// phase 0 SCQS 30 at its begin; phase 1 waits on stage 10 done; scene
/// SCQS 40 at its end), SD (with the quest; a 1 s timer of INAM 5 in
/// phase 0; phase 1 completes on function 550 of that action), SE (with
/// the quest; a start-scene action naming SF), SF. INFO IN of topic DL
/// sets 10 at its end (TIQS).
fn store() -> Store {
    let mut qv = vec![6, 0, 2, 0, 1, 0];
    bare(&mut qv, "QS");
    qv.push(3);
    qv.extend(1u16.to_le_bytes());
    bare(&mut qv, "QF");
    qv.extend(10u16.to_le_bytes());
    qv.extend([0, 0]);
    qv.extend(0i32.to_le_bytes());
    qv.push(1);
    text(&mut qv, "QF");
    text(&mut qv, "Fragment_Stage_0010");
    qv.extend(2u16.to_le_bytes());
    for alias in [0i16, 1] {
        qv.extend([0, 0]);
        qv.extend(alias.to_le_bytes());
        qv.extend(QS.to_le_bytes());
        qv.extend([6, 0, 2, 0, 1, 0]);
        bare(&mut qv, "AliasAct");
    }
    let mut dnam = 0x0001u16.to_le_bytes().to_vec();
    dnam.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"VMAD", &qv),
        sub(b"DNAM", &dnam),
        stage(0, quest::STARTUP_STAGE),
        stage(10, 0),
        stage(20, 0),
        stage(30, 0),
        stage(40, 0),
        sub(b"ANAM", &w(3)),
        sub(b"ALST", &w(0)),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFR", &w(R1)),
        sub(b"ALED", &[]),
        sub(b"ALST", &w(1)),
        sub(b"ALID", b"B\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFA", &w(2)),
        sub(b"ALRT", &w(LT)),
        sub(b"ALED", &[]),
        sub(b"ALLS", &w(2)),
        sub(b"ALID", b"L\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALFL", &w(LOC)),
        sub(b"ALED", &[]),
    ]
    .concat();
    let one = 1.0f32.to_bits();
    let sc = scene(
        SC,
        "SC",
        0x1,
        &[
            phase(&[], Some((30, -1))),
            phase(&[ctda(0, 0, 59, one, QS, 10)], None),
        ],
        &[sub(b"VNAM", &w(0)), sub(b"SCQS", &pair(-1, 40))],
    );
    let sd = scene(
        SD,
        "SD",
        0x1,
        &[
            phase(&[], None),
            phase(&[ctda(0, 0, 550, one, SD, 5)], None),
            action(2, 5, 0, &[sub(b"SNAM", &1.0f32.to_le_bytes())]),
        ],
        &[sub(b"VNAM", &w(0))],
    );
    let se = scene(
        SE,
        "SE",
        0x1,
        &[phase(&[], None), action(4, 1, 0, &[sub(b"LCEP", &w(SF))])],
        &[sub(b"VNAM", &w(0))],
    );
    let sf = scene(SF, "SF", 0, &[phase(&[], None)], &[sub(b"VNAM", &w(0))]);
    let info = record(
        b"INFO",
        IN,
        &[sub(b"ENAM", &w(0)), sub(b"TIQS", &pair(-1, 10))].concat(),
    );
    let children = [
        sc,
        sd,
        se,
        sf,
        record(b"DIAL", DL, &sub(b"EDID", b"DL\0")),
        group(7, DL.to_le_bytes(), &info),
    ]
    .concat();
    let quests = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &children),
    ]
    .concat();
    let mut av = vec![6, 0, 2, 0, 1, 0];
    text(&mut av, "Act");
    av.push(0);
    av.extend(2u16.to_le_bytes());
    object(&mut av, "MyQuest", -1, QS);
    int(&mut av, "Stage", 20);
    let mut tv = vec![6, 0, 2, 0, 1, 0];
    text(&mut tv, "Trig");
    tv.push(0);
    tv.extend(1u16.to_le_bytes());
    object(&mut tv, "MyQuest", -1, QS);
    let refs = [
        record(b"REFR", R1, &sub(b"NAME", &w(ACT))),
        record(b"REFR", R2, &sub(b"NAME", &w(STAT))),
        record(
            b"REFR",
            R3,
            &[sub(b"NAME", &w(TRIG)), sub(b"VMAD", &tv)].concat(),
        ),
    ]
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
    let mut lcsr = w(LT);
    lcsr.extend(w(R2));
    lcsr.extend(w(CELL));
    lcsr.extend(w(0));
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
        *b"STAT",
        &record(b"STAT", STAT, &sub(b"EDID", b"S\0")),
    ));
    plugin.extend(group(
        0,
        *b"ACTI",
        &[
            record(
                b"ACTI",
                ACT,
                &[sub(b"EDID", b"Act\0"), sub(b"VMAD", &av)].concat(),
            ),
            record(b"ACTI", TRIG, &sub(b"EDID", b"Trig\0")),
        ]
        .concat(),
    ));
    plugin.extend(group(0, *b"QUST", &quests));
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

/// Engine types with natives (their empty `On...` functions are the
/// events), and the scripts the store attaches.
fn lib() -> MapLibrary {
    let mut l = super::base();
    l.add(assemble(
        "script ObjectReference extends Form
         func OnActivate none ObjectReference akActionRef
         func OnTriggerEnter none ObjectReference akActionRef
         func OnOpen none ObjectReference akActionRef
         func Disable none native Bool abFade",
    ));
    l.add(assemble(
        "script Quest extends Form
         struct QuestStage
         member Quest QuestToSet
         member Int StageToSet
         func SetCurrentStageID Bool native Int aiStageID
         func Start Bool native
         func OnStageSet none Int auiStageID Int auiItemID
         func setStage Bool Int aiStage
         local Bool ::r
         23 SetCurrentStageID self ::r 1 aiStage
         26 ::r
         func SetQuestStage none global quest#queststage qs
         local Quest ::q
         local Int ::s
         local Bool ::r
         38 ::q qs QuestToSet
         38 ::s qs StageToSet
         23 setStage ::q ::r 1 ::s",
    ));
    l.add(assemble(
        "script Alias extends ScriptObject
         func GetOwningQuest Quest native",
    ));
    l.add(assemble(
        "script ReferenceAlias extends Alias
         func GetReference ObjectReference native
         func OnActivate none ObjectReference akActionRef",
    ));
    l.add(assemble("script Activator extends Form"));
    l.add(assemble("script Static extends Form"));
    l.add(assemble(
        "script Act extends ObjectReference
         var Quest ::MyQuest_var
         prop Quest MyQuest ::MyQuest_var
         var Int ::Stage_var
         prop Int Stage ::Stage_var
         func OnActivate none ObjectReference akActionRef
         local Bool ::r
         23 setStage ::MyQuest_var ::r 1 ::Stage_var
         func OnFoo none
         func Unused none
         local Bool ::r
         23 setStage ::MyQuest_var ::r 1 25",
    ));
    l.add(assemble(
        "script AliasAct extends ReferenceAlias
         func OnActivate none ObjectReference akActionRef
         local Quest ::q
         local Bool ::r
         23 GetOwningQuest self ::q 0
         23 setStage ::q ::r 1 40",
    ));
    l.add(assemble(
        "script Helper
         func Try none global Quest q Int s
         local Bool ::r
         23 setStage q ::r 1 s",
    ));
    l.add(assemble(
        "script Trig extends ObjectReference
         var Quest ::MyQuest_var
         prop Quest MyQuest ::MyQuest_var
         func OnTriggerEnter none ObjectReference akActionRef
         local none ::nonevar
         25 Helper Try ::nonevar 2 ::MyQuest_var 30",
    ));
    l.add(assemble(
        "script StructUser extends ObjectReference
         var quest#queststage ::QS_var
         prop quest#queststage QS ::QS_var
         func OnActivate none ObjectReference akActionRef
         local none ::nonevar
         25 Quest SetQuestStage ::nonevar 1 ::QS_var",
    ));
    l.add(assemble("script QS extends Quest"));
    l.add(assemble(
        "script QF extends Quest
         func Fragment_Stage_0010 none
         local Bool ::r
         23 setStage self ::r 1 30",
    ));
    l
}

fn find(t: &setters::Table, stage: i32, f: impl Fn(&Need) -> bool) -> Vec<&setters::Setter> {
    t.of_stage(&key(QS), stage)
        .into_iter()
        .filter(|s| f(&s.need))
        .collect()
}

#[test]
fn the_table_finds_calls_fragments_fields_and_startup_stages() {
    let s = store();
    let b = Budget::new(1 << 24);
    let lib = lib();
    let (mut atts, counts) = setters::attachments(&s, &setters::owners(&s), &b).unwrap();
    assert!(counts.is_empty(), "{counts:?}");
    // The VMADs read: Act on the activator with its values, the fragment
    // script with its slot, AliasAct on two aliases, Trig on a reference.
    let act = atts.iter().find(|a| a.script == "act").unwrap();
    assert_eq!(act.signature, *b"ACTI");
    assert_eq!(act.objects["myquest"], [key(QS)]);
    assert_eq!(act.ints["stage"], [20]);
    let qf = atts.iter().find(|a| a.script == "qf").unwrap();
    assert_eq!(qf.part, Part::Fragment);
    assert_eq!(
        qf.slots,
        [("fragment_stage_0010".to_string(), Slot::Stage(10, 0))]
    );
    let parts: Vec<Part> = atts
        .iter()
        .filter(|a| a.script == "aliasact")
        .map(|a| a.part)
        .collect();
    assert_eq!(parts, [Part::Alias(0), Part::Alias(1)]);
    // A struct property value, as a VMAD stores one on a placed reference.
    atts.push(Attachment {
        record: key(R4),
        signature: *b"REFR",
        part: Part::Own,
        quest: None,
        script: "structuser".into(),
        objects: BTreeMap::new(),
        aliases: BTreeMap::new(),
        ints: BTreeMap::new(),
        structs: BTreeMap::from([(
            "qs".to_string(),
            vec![BTreeMap::from([
                ("questtoset".to_string(), Member::Record(key(QS))),
                ("stagetoset".to_string(), Member::Int(10)),
            ])],
        )]),
        slots: Vec::new(),
    });
    let t = setters::table(&lib, &atts, &s, None, &b).unwrap();
    let ev = setters::engine_events(&lib);
    assert!(
        ev.contains("onactivate") && ev.contains("ontriggerenter") && ev.contains("onstageset")
    );
    assert!(
        !ev.contains("onfoo"),
        "a script without natives declares no event"
    );
    // A call on a property with the attachment's values, in an event.
    let x = find(&t, 20, |n| {
        *n == Need::Event("onactivate".into(), On::Record(key(ACT), *b"ACTI"))
    });
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].kind, "a call in a base object's script (ACTI)");
    assert_eq!(x[0].depth, 0);
    // A function no event reaches.
    assert_eq!(find(&t, 25, |n| *n == Need::Unreached).len(), 1);
    // The owning quest of an alias script.
    assert_eq!(
        find(&t, 40, |n| *n
            == Need::Event("onactivate".into(), On::Alias(key(QS), 0)))
        .len(),
        1
    );
    // A stage fragment needs its stage.
    assert_eq!(find(&t, 30, |n| *n == Need::Stage(key(QS), 10)).len(), 1);
    // A global function passing its parameters on, called with constants.
    let x = find(&t, 30, |n| {
        *n == Need::Event("ontriggerenter".into(), On::Record(key(R3), *b"REFR"))
    });
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].depth, 1);
    assert!(x[0]
        .kind
        .ends_with("through a function passing a parameter on"));
    // The quest type's function taking a struct, with the struct value.
    let x = find(&t, 10, |n| {
        *n == Need::Event("onactivate".into(), On::Record(key(R4), *b"REFR"))
    });
    assert_eq!(x.len(), 1);
    // Record fields: a phase's SCQS, the scene's SCQS, an INFO's TIQS.
    assert_eq!(
        find(&t, 30, |n| *n
            == Need::Scene(key(SC), ScenePoint::PhaseField(0, false)))
        .len(),
        1
    );
    assert_eq!(
        find(&t, 40, |n| *n == Need::Scene(key(SC), ScenePoint::End)).len(),
        1
    );
    assert_eq!(find(&t, 10, |n| *n == Need::Line(key(IN), true)).len(), 1);
    // The startup stage and how the quest starts.
    assert_eq!(find(&t, 0, |n| *n == Need::QuestStart(key(QS))).len(), 1);
    assert!(t
        .of_quest(&key(QS))
        .iter()
        .any(|s| s.kind.starts_with("flagged to start")));
    // Every stage the quest holds, and none of the -1 halves.
    assert_eq!(t.stages[&bind::record_key(&key(QS))].1.len(), 5);
    assert!(t.setters.iter().all(|s| s.stage != Resolved::Const(-1)));
    // Distinct entries only.
    let mut seen = std::collections::BTreeSet::new();
    for s in &t.setters {
        assert!(seen.insert(format!(
            "{:?}",
            (&s.quest, &s.stage, &s.kind, &s.need, &s.site)
        )));
    }
}

#[test]
fn a_call_whose_values_no_attachment_gives_stays_open() {
    let s = store();
    let b = Budget::new(1 << 24);
    let lib = lib();
    // Act attached with no property values.
    let atts = vec![Attachment {
        record: key(ACT),
        signature: *b"ACTI",
        part: Part::Own,
        quest: None,
        script: "act".into(),
        objects: BTreeMap::new(),
        aliases: BTreeMap::new(),
        ints: BTreeMap::new(),
        structs: BTreeMap::new(),
        slots: Vec::new(),
    }];
    let t = setters::table(&lib, &atts, &s, None, &b).unwrap();
    let open: Vec<_> = t
        .setters
        .iter()
        .filter(|x| x.site.starts_with("act.onactivate"))
        .collect();
    assert_eq!(open.len(), 1);
    assert_eq!(
        open[0].quest,
        Resolved::Open("a property without a record value")
    );
    // The stage property's compiled initial value (none: 0).
    assert_eq!(open[0].stage, Resolved::Const(0));
    // Scripts no VMAD attaches are still scanned.
    assert!(t
        .setters
        .iter()
        .any(|x| x.kind == "a call in a script no read VMAD attaches"
            && x.site.starts_with("aliasact")));
}

/// A new game's interpreter over the store with the quest host, scripts
/// attached on need, nested stage fragments.
macro_rules! game {
    ($s:ident, $vm:ident) => {
        let mut lib_ = lib();
        let mut host_ = QuestHost::new(&$s, 1 << 24);
        let mut attach_ = bind::RecordScripts::new(&$s, 1 << 24);
        let mut runner_ = quest::Runner::new(&$s, 1 << 24, 256);
        let mut $vm = Vm::new(&mut lib_, &mut host_);
        $vm.attach = Some(&mut attach_);
        $vm.nested = Some(&mut runner_);
    };
}

fn start(vm: &mut Vm<'_>, s: &Store, b: &Budget) {
    quest::start_new_game(vm, s, b).unwrap();
    quest::run_jobs(vm, s, 256, b);
}

fn stages_set(vm: &Vm<'_>) -> Vec<i32> {
    vm.world
        .quests
        .log
        .iter()
        .filter_map(|(_, c)| match c {
            Change::StageSet(n) => Some(*n),
            _ => None,
        })
        .collect()
}

#[test]
fn an_activation_reaches_the_reference_then_the_aliases_holding_it() {
    let s = store();
    let b = Budget::new(1 << 24);
    game!(s, vm);
    start(&mut vm, &s, &b);
    assert_eq!(stages_set(&vm), [0]);
    let (runs, open) = player::deliver(&mut vm, &s, &key(R1), Action::Activate, &b).unwrap();
    quest::run_jobs(&mut vm, &s, 256, &b);
    assert_eq!(open, 0);
    let qs = crate::data::record_object(&mut vm.world, &s, &key(QS), "quest", &b).unwrap();
    let who: Vec<(Receiver, String)> = runs
        .iter()
        .map(|r| (r.receiver.clone(), r.script.clone()))
        .collect();
    assert_eq!(
        who,
        [
            (Receiver::Reference, "act".to_string()),
            (Receiver::Alias(qs, 0), "aliasact".to_string())
        ]
    );
    assert!(runs
        .iter()
        .all(|r| matches!(r.outcome, crate::Outcome::Completed(_))));
    // The reference's script first, with the base's property values.
    assert_eq!(stages_set(&vm), [0, 20, 40]);
    assert!(vm.rules.contains(rules::EVENTS) && vm.rules.contains(rules::EVENT_ALIASES));
    // The handler received the player.
    assert!(vm.world.player.is_some());
    // A trigger without a handler for the event: nothing runs.
    let (runs, _) = player::deliver(&mut vm, &s, &key(R1), Action::TriggerEnter, &b).unwrap();
    assert!(runs.is_empty());
    // A FormID that names no placed reference is refused.
    assert!(player::deliver(&mut vm, &s, &key(ACT), Action::Activate, &b).is_err());
    assert_eq!(
        player::parse_key("Test.esm:FF000960"),
        Some(key(R1)),
        "the load-order byte is ignored"
    );
    assert_eq!(Action::from_switch("--assume-open"), Some(Action::Open));
}

#[test]
fn a_location_alias_holds_its_special_reference_only_when_the_reading_is_on() {
    for on in [false, true] {
        let s = store();
        let b = Budget::new(1 << 24);
        game!(s, vm);
        vm.world.quests.location_fills = on;
        start(&mut vm, &s, &b);
        let (holders, _) = player::holders(&mut vm, &s, &key(R2));
        let (runs, _) = player::deliver(&mut vm, &s, &key(R2), Action::Activate, &b).unwrap();
        quest::run_jobs(&mut vm, &s, 256, &b);
        if on {
            assert_eq!(holders, [(key(QS), 1)]);
            assert_eq!(runs.len(), 1);
            assert_eq!(stages_set(&vm), [0, 40]);
        } else {
            assert!(holders.is_empty() && runs.is_empty());
            assert_eq!(stages_set(&vm), [0]);
        }
    }
    let s = store();
    let i = quest::QuestInfo::load(&s, &key(QS), &Budget::new(1 << 24))
        .unwrap()
        .unwrap();
    assert_eq!(i.location_refs.get(&1), Some(&Ok(key(R2))));
    assert_eq!(i.fill_of(1, false), Ok(None));
    assert_eq!(i.fill_of(1, true), Ok(Some(key(R2))));
    assert_eq!(i.fill_of(0, false), Ok(Some(key(R1))));
}

/// The log of one scene.
fn log_of(vm: &mut Vm<'_>, s: &Store, id: u32) -> Vec<SceneChange> {
    let b = Budget::new(1 << 24);
    let o = crate::data::record_object(&mut vm.world, s, &key(id), "scene", &b).unwrap();
    vm.world
        .scenes
        .log
        .iter()
        .filter(|(x, _)| *x == o)
        .map(|x| x.1.clone())
        .collect()
}

#[test]
fn stage_fields_waits_and_the_recheck_of_a_stopped_scene() {
    for fields in [false, true] {
        let s = store();
        let b = Budget::new(1 << 24);
        game!(s, vm);
        vm.world.scenes.on = true;
        vm.world.scenes.begin_on_quest_start = true;
        vm.world.scenes.stage_fields = fields;
        start(&mut vm, &s, &b);
        // Phase 1 of SC waits on stage 10.
        let log = log_of(&mut vm, &s, SC);
        assert_eq!(
            log.last(),
            Some(&SceneChange::Waiting(
                1,
                "completion conditions false (waits on the world)".into()
            ))
        );
        assert_eq!(stages_set(&vm).contains(&30), fields, "phase 0's SCQS");
        let waits = setters::scene_waits(&mut vm.world, &s);
        let w: Vec<(u16, i32, &str)> = waits
            .iter()
            .map(|x| (x.function, x.stage, x.list))
            .collect();
        assert_eq!(w, [(59, 10, "completion")]);
        // Setting the stage does not move the stopped scene by itself.
        let qs = quest::quest_object(&mut vm, &s, &key(QS), &b).unwrap();
        quest::set_stage(&mut vm, &s, qs, 10);
        quest::run_jobs(&mut vm, &s, 256, &b);
        assert!(matches!(
            log_of(&mut vm, &s, SC).last(),
            Some(SceneChange::Waiting(1, _))
        ));
        // Offered again, the phase completes and the scene ends.
        assert!(scene::recheck(&mut vm.world) >= 1);
        quest::run_jobs(&mut vm, &s, 256, &b);
        let log = log_of(&mut vm, &s, SC);
        assert!(log.contains(&SceneChange::PhaseCompleted(1)));
        assert_eq!(log.last(), Some(&SceneChange::Ended));
        assert_eq!(
            stages_set(&vm).contains(&40),
            fields,
            "the scene's SCQS at its end"
        );
        assert!(setters::scene_waits(&mut vm.world, &s).is_empty());
    }
}

#[test]
fn function_550_reads_the_clock_only_when_asked_and_start_scene_actions_on_assumption() {
    for (actions, starts) in [(false, false), (true, true)] {
        let s = store();
        let b = Budget::new(1 << 24);
        game!(s, vm);
        vm.world.scenes.on = true;
        vm.world.scenes.begin_on_quest_start = true;
        vm.world.scenes.action_conditions = actions;
        vm.world.scenes.clock = scene::Clock {
            on: true,
            timers: true,
            assume_start_scenes: starts,
            ..scene::Clock::default()
        };
        start(&mut vm, &s, &b);
        assert_eq!(scene::advance_clock(&mut vm.world, None), Some(1.0));
        quest::run_jobs(&mut vm, &s, 256, &b);
        let sd = log_of(&mut vm, &s, SD);
        if actions {
            assert_eq!(sd.last(), Some(&SceneChange::Ended));
        } else {
            assert_eq!(
                sd.last(),
                Some(&SceneChange::Waiting(
                    1,
                    "completion conditions false (waits on the world)".into()
                ))
            );
        }
        let se = log_of(&mut vm, &s, SE);
        let sf = log_of(&mut vm, &s, SF);
        if starts {
            assert_eq!(se.last(), Some(&SceneChange::Ended));
            assert!(sf.contains(&SceneChange::Started));
        } else {
            assert!(
                matches!(se.last(), Some(SceneChange::Waiting(0, why)) if why.contains("kind 4"))
            );
            assert!(sf.is_empty());
        }
        let mut used: std::collections::BTreeSet<&str> = vm.rules.iter().copied().collect();
        used.extend(vm.host.rules());
        assert_eq!(used.contains(rules::COND_FN_550_CLOCK), actions);
        assert_eq!(used.contains(rules::START_SCENE_ACTION), starts);
    }
}

#[test]
fn every_new_rule_is_registered_pending() {
    for r in [
        rules::EVENT_ALIASES,
        rules::SETTER_EVENT_ENTRY,
        rules::SETTER_SCAN,
        rules::SETTER_FIELDS,
        rules::SCENE_RECHECK,
        rules::COND_FN_550_CLOCK,
        rules::START_SCENE_ACTION,
        rules::ALIAS_LOCATION_REF,
        rules::EVENT_OPEN,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn a_long_chain_through_call_receivers_ends_without_deep_recursion() {
    // q1 is the owning quest of q0, q2 of q1, and so on: each link used to
    // be one more nested call of the scan, without a bound.
    let n = 4000;
    let mut src = String::from("script Deep extends Quest\n func F none\n");
    for i in 0..=n {
        src.push_str(&format!(" local Quest q{i}\n"));
    }
    src.push_str(" 23 GetOwningQuest self q0 0\n");
    for i in 1..=n {
        src.push_str(&format!(" 23 GetOwningQuest q{} q{i} 0\n", i - 1));
    }
    // A small stack: the old unbounded recursion overflows it.
    let got = std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(move || {
            let script = assemble(&src);
            let f = script.function("", "f").unwrap().clone();
            let mut lib = MapLibrary::default();
            lib.add(script);
            let at = f.code.len();
            let last = crate::program::Lit::Ident(format!("q{n}"));
            format!("{:?}", setters::trace(&lib, &f, at, &last))
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(
        got,
        format!(
            "{:?}",
            setters::Src::Other("the owning quest of another object")
        )
    );
}
