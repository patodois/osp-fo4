//! The home step's readings over synthetic plugins (no game data): the
//! player passing through a load door where the track changes space
//! (`world_load_door_xtel_v1`), and what a spoken line does besides its
//! stage fields: its end fragment (`dialogue_line_fragments_v1`) and the
//! scene its `TSCE` names, at the phase its `INTV` stores
//! (`dialogue_line_start_scene_v1`).
use super::assemble;
use super::quest::{group, key, record, sub, text, Owned};
use super::world::all_on;
use crate::{
    bind::RecordScripts,
    line,
    quest::{self, QuestHost},
    rules,
    scene::{self, Change},
    track::{self, Place, Track},
    voice::{self, VoiceFiles},
    Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use std::rc::Rc;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn data(pos: [f32; 3]) -> Vec<u8> {
    let mut d = Vec::new();
    for f in pos.into_iter().chain([0.0; 3]) {
        d.extend(f.to_le_bytes());
    }
    d
}

fn xtel(door: u32, pos: [f32; 3], yaw: f32) -> Vec<u8> {
    let mut d = w(door);
    for f in pos.into_iter().chain([0.0, 0.0, yaw]) {
        d.extend(f.to_le_bytes());
    }
    d.extend([0; 8]);
    d
}

fn sources(plugin: Vec<u8>) -> Store {
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

fn header(n: u32) -> Vec<u8> {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(n.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
    r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
    r
}

const WRLD: u32 = 0x800;
const EXT: u32 = 0x801;
const ACTI: u32 = 0x802;
const DOOR: u32 = 0x803;
const TRIG: u32 = 0x810;
const EXD: u32 = 0x811;
const ROOM: u32 = 0x830;
const IND: u32 = 0x831;
const FAR: u32 = 0x832;

/// A worldspace with one exterior cell holding a trigger box (half
/// extents 50, script `Trig`) at (1000, 1000, 0) and a door at
/// (1000, 1100, 0); an interior cell holding two doors whose `XTEL` names
/// the exterior door: one at the origin leading to (1000, 1000, 0) facing
/// 90 degrees, one at (500, 0, 0) leading to (3000, 1000, 0).
fn door_store() -> Store {
    let mut vmad = vec![6, 0, 2, 0];
    vmad.extend(1u16.to_le_bytes());
    text(&mut vmad, "Trig");
    vmad.push(0);
    vmad.extend(0u16.to_le_bytes());
    let mut xprm = Vec::new();
    for f in [50.0f32, 50.0, 50.0, 1.0, 0.0, 1.0, 0.3] {
        xprm.extend(f.to_le_bytes());
    }
    xprm.extend(1u32.to_le_bytes());
    let outside = [
        record(
            b"REFR",
            TRIG,
            &[
                sub(b"NAME", &w(ACTI)),
                sub(b"VMAD", &vmad),
                sub(b"XPRM", &xprm),
                sub(b"DATA", &data([1000.0, 1000.0, 0.0])),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            EXD,
            &[
                sub(b"NAME", &w(DOOR)),
                sub(b"XTEL", &xtel(IND, [0.0, 40.0, 0.0], 0.0)),
                sub(b"DATA", &data([1000.0, 1100.0, 0.0])),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut xclc = 0i32.to_le_bytes().to_vec();
    xclc.extend(0i32.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    let ext = record(
        b"CELL",
        EXT,
        &[
            sub(b"EDID", b"Ext\0"),
            sub(b"DATA", &2u16.to_le_bytes()),
            sub(b"XCLC", &xclc),
        ]
        .concat(),
    );
    let world = [
        record(b"WRLD", WRLD, &sub(b"EDID", b"W\0")),
        group(
            1,
            WRLD.to_le_bytes(),
            &[
                ext,
                group(6, EXT.to_le_bytes(), &group(9, EXT.to_le_bytes(), &outside)),
            ]
            .concat(),
        ),
    ]
    .concat();
    let inside = [
        record(
            b"REFR",
            IND,
            &[
                sub(b"NAME", &w(DOOR)),
                sub(
                    b"XTEL",
                    &xtel(EXD, [1000.0, 1000.0, 0.0], std::f32::consts::FRAC_PI_2),
                ),
                sub(b"DATA", &data([0.0, 0.0, 0.0])),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            FAR,
            &[
                sub(b"NAME", &w(DOOR)),
                sub(b"XTEL", &xtel(EXD, [3000.0, 1000.0, 0.0], 0.0)),
                sub(b"DATA", &data([500.0, 0.0, 0.0])),
            ]
            .concat(),
        ),
    ]
    .concat();
    let room = [
        record(
            b"CELL",
            ROOM,
            &[sub(b"EDID", b"Room\0"), sub(b"DATA", &1u16.to_le_bytes())].concat(),
        ),
        group(
            6,
            ROOM.to_le_bytes(),
            &group(9, ROOM.to_le_bytes(), &inside),
        ),
    ]
    .concat();
    let mut plugin = header(8);
    plugin.extend(group(
        0,
        *b"ACTI",
        &record(b"ACTI", ACTI, &sub(b"EDID", b"T\0")),
    ));
    plugin.extend(group(
        0,
        *b"DOOR",
        &record(b"DOOR", DOOR, &sub(b"EDID", b"D\0")),
    ));
    plugin.extend(group(0, *b"CELL", &group(2, [0; 4], &room)));
    plugin.extend(group(0, *b"WRLD", &world));
    sources(plugin)
}

/// In the room until 10 s near its first door, then outside at 20 s,
/// 500 units east of the trigger.
const THROUGH: &str =
    "0 test.esm:00000830 0 50 0\n10 test.esm:00000830 0 10 0\n20 test.esm:00000801 1500 1000 0\n";

#[test]
fn the_track_passes_through_the_nearest_load_door_only_with_its_switch() {
    let s = door_store();
    let b = Budget::new(1 << 24);
    // Off: the player reaches the caller's point at 20 s, outside the
    // trigger: no crossing, no passage, the track as given.
    let off = Place::load(&s, Track::parse(THROUGH).unwrap(), all_on(), &b).unwrap();
    assert!(off.doors.is_empty());
    assert!(off.crossings.is_empty(), "{:?}", off.crossings);
    assert_eq!(off.track.as_ref().unwrap().points.len(), 3);
    assert!(!off.counts.keys().any(|k| k.starts_with("load doors")));
    // On: at 10 s the player passes the nearer door (10 units away) and
    // stands where its XTEL leads, inside the trigger: entered at 10 s,
    // left at 11 s on the way to the caller's next point.
    let on = track::Switches {
        load_doors: true,
        ..all_on()
    };
    let p = Place::load(&s, Track::parse(THROUGH).unwrap(), on, &b).unwrap();
    assert_eq!(p.doors.len(), 1);
    let d = &p.doors[0];
    assert_eq!(
        (d.t, d.door.clone(), d.to.clone()),
        (10.0, key(IND), key(EXD))
    );
    assert_eq!((d.space.clone(), d.pos), (key(WRLD), [1000.0, 1000.0, 0.0]));
    assert!((d.reach - 10.0).abs() < 1e-9 && (d.facing - 90.0).abs() < 1e-4);
    let t = p.track.as_ref().unwrap();
    assert_eq!(t.points.len(), 4);
    assert_eq!(
        t.at(10.0).map(|x| (x.0.clone(), x.1)),
        Some((key(WRLD), [1000.0, 1000.0, 0.0]))
    );
    let c: Vec<(f64, bool)> = p.crossings.iter().map(|c| (c.t, c.enter)).collect();
    assert_eq!(c.len(), 2, "{c:?}");
    assert!((c[0].0 - 10.0).abs() < 1e-9 && c[0].1);
    assert!((c[1].0 - 11.0).abs() < 1e-9 && !c[1].1);
    assert_eq!(
        p.counts
            .get("load doors\tspace changes through a load door: the player placed at its teleport position\tthe door within 256 units of the track's last point"),
        Some(&1)
    );
    // The way back: the exterior door leads into the room.
    let back = "0 test.esm:00000801 1500 1000 0\n5 test.esm:00000830 0 10 0\n";
    let p = Place::load(&s, Track::parse(back).unwrap(), on, &b).unwrap();
    assert_eq!(p.doors.len(), 1);
    assert_eq!(p.doors[0].door, key(EXD));
    assert_eq!(p.doors[0].pos, [0.0, 40.0, 0.0]);
}

#[test]
fn the_xtel_reads_its_door_position_and_rotation_and_none_without_one() {
    let s = door_store();
    let b = Budget::new(1 << 24);
    let (r, plugin) = s.raw_with_plugin(&key(IND), &b).unwrap().unwrap();
    let tp = track::teleport_of(&s, &r, plugin).unwrap().unwrap();
    assert_eq!(tp.door, key(EXD));
    assert_eq!(tp.pos, [1000.0, 1000.0, 0.0]);
    assert!((tp.rot[2] - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    let (r, plugin) = s.raw_with_plugin(&key(TRIG), &b).unwrap().unwrap();
    assert_eq!(track::teleport_of(&s, &r, plugin), Ok(None));
    assert!(rules::ALL.contains(&rules::WORLD_LOAD_DOOR));
}

const QS: u32 = 0x900;
const SC: u32 = 0x901;
const SC2: u32 = 0x902;
const VT: u32 = 0x910;
const DL: u32 = 0x920;
const LN: u32 = 0x921;

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

fn trda(n: u8) -> Vec<u8> {
    let mut d = vec![0xff; 4];
    d.extend([n, 0, 0, 0, 0, 1, 0, 0]);
    d.extend([0xff; 8]);
    d
}

/// QS starts with the game. SC (FNAM bit 0x1) has one phase: a line of
/// topic DL spoken by alias 0 (voice type VT). DL holds one line LN whose
/// fragment block runs `LineFrag.Fragment_End` as it ends and whose TSCE
/// names SC2 at phase 1 (INTV). SC2 has two empty phases.
fn line_store() -> Store {
    let line_action = [
        sub(b"ANAM", &[0, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &w(0)),
        sub(b"INAM", &w(1)),
        sub(b"SNAM", &w(0)),
        sub(b"ENAM", &w(0)),
        sub(b"DATA", &w(DL)),
        sub(b"ANAM", &[]),
    ]
    .concat();
    let sc = [
        sub(b"EDID", b"SC\0"),
        sub(b"FNAM", &w(0x1)),
        phase(),
        sub(b"ALID", &w(0)),
        sub(b"LNAM", &w(0)),
        sub(b"DNAM", &w(0)),
        line_action,
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(2)),
    ]
    .concat();
    let sc2 = [
        sub(b"EDID", b"SC2\0"),
        sub(b"FNAM", &w(0)),
        phase(),
        phase(),
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(1)),
    ]
    .concat();
    let mut vmad = vec![6, 0, 2, 0, 0, 0, 3, 2];
    text(&mut vmad, "LineFrag");
    vmad.extend([0, 0, 0]);
    vmad.push(1);
    text(&mut vmad, "LineFrag");
    text(&mut vmad, "Fragment_End");
    let ln = [
        sub(b"VMAD", &vmad),
        sub(b"ENAM", &w(0)),
        sub(b"TSCE", &w(SC2)),
        sub(b"INTV", &w(1)),
        sub(b"TRDA", &trda(1)),
        sub(b"NAM1", &w(1)),
    ]
    .concat();
    let dl = [sub(b"EDID", b"DL\0"), sub(b"TIFC", &w(1))].concat();
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"DNAM", &d),
        sub(b"ANAM", &w(1)),
        sub(b"ALST", &w(0)),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &w(0)),
        sub(b"VTCK", &w(VT)),
        sub(b"ALED", &[]),
    ]
    .concat();
    let children = [
        record(b"SCEN", SC, &sc),
        record(b"SCEN", SC2, &sc2),
        record(b"DIAL", DL, &dl),
        group(7, DL.to_le_bytes(), &record(b"INFO", LN, &ln)),
    ]
    .concat();
    let quests = [
        record(b"QUST", QS, &qs),
        group(10, QS.to_le_bytes(), &children),
    ]
    .concat();
    let mut plugin = header(6);
    plugin.extend(group(
        0,
        *b"VTYP",
        &record(b"VTYP", VT, &sub(b"EDID", b"VoiceA\0")),
    ));
    plugin.extend(group(0, *b"QUST", &quests));
    sources(plugin)
}

fn lib() -> crate::MapLibrary {
    let mut l = super::scene::lib();
    l.add(assemble(
        "script LineFrag extends Form
         var Int ended 0
         func Fragment_End none ObjectReference akSpeakerRef
         13 ended 1",
    ));
    l
}

/// Starts a new game with the clock and the line switches, advances the
/// clock; returns SC2's log and whether the fragment ran.
fn run_lines(sw: line::Switches) -> (Vec<Change>, Val, Vec<&'static str>) {
    let s = line_store();
    let b = Budget::new(1 << 24);
    let mut files = VoiceFiles::default();
    assert!(files.add(b"sound/voice/test.esm/voicea/00000921_1.fuz", Some(2.0)));
    let clock = scene::Clock {
        on: true,
        timers: true,
        voice: Some(Rc::new(voice::Voice {
            files,
            topics: voice::Topics::read(&s),
        })),
        ..scene::Clock::default()
    };
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    vm.world.scenes.clock = clock;
    vm.world.scenes.lines = sw;
    quest::start_new_game(&mut vm, &s, &b).unwrap();
    quest::run_jobs(&mut vm, &s, 64, &b);
    for _ in 0..10 {
        if scene::advance_clock(&mut vm.world, None).is_none() {
            break;
        }
        quest::run_jobs(&mut vm, &s, 64, &b);
    }
    let sc2 = crate::data::record_object(&mut vm.world, &s, &key(SC2), "scene", &b).unwrap();
    let log = vm
        .world
        .scenes
        .log
        .iter()
        .filter(|(o, _)| *o == sc2)
        .map(|x| x.1.clone())
        .collect();
    let ln = vm
        .world
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some("test.esm:00000921"));
    let ended = ln
        .and_then(|o| {
            vm.world.objects[o]
                .instances
                .iter()
                .find(|i| vm.world.instances[**i].script == "linefrag")
                .copied()
        })
        .map(|i| {
            vm.world.instances[i]
                .vars
                .iter()
                .find(|v| v.name == "ended")
                .unwrap()
                .value
                .clone()
        })
        .unwrap_or(Val::None);
    let mut used: Vec<&'static str> = vm.rules.iter().copied().collect();
    used.extend(vm.host.rules());
    (log, ended, used)
}

#[test]
fn a_line_ending_runs_its_end_fragment_and_starts_its_scene_at_the_stored_phase() {
    // Off: the line plays (2 s) and nothing else happens.
    let (log, ended, used) = run_lines(line::Switches::default());
    assert!(log.is_empty(), "{log:?}");
    assert_eq!(ended, Val::None);
    assert!(!used.contains(&rules::LINE_FRAGMENTS) && !used.contains(&rules::LINE_START_SCENE));
    // Start scene only: SC2 starts as the line ends, offering phase 1 first.
    let (log, ended, used) = run_lines(line::Switches {
        start_scene: true,
        ..line::Switches::default()
    });
    assert_eq!(log.first(), Some(&Change::Started), "{log:?}");
    assert_eq!(log.get(1), Some(&Change::PhaseEntered(1)), "{log:?}");
    assert!(!log.contains(&Change::PhaseEntered(0)));
    assert_eq!(ended, Val::None);
    assert!(used.contains(&rules::LINE_START_SCENE));
    // Fragments only: the end fragment runs on the line's fragment
    // instance; SC2 does not start.
    let (log, ended, used) = run_lines(line::Switches {
        fragments: true,
        ..line::Switches::default()
    });
    assert!(log.is_empty(), "{log:?}");
    assert_eq!(ended, Val::Int(1));
    assert!(used.contains(&rules::LINE_FRAGMENTS));
}

#[test]
fn the_line_extras_read_the_fragment_slots_the_scene_and_the_phase() {
    let s = line_store();
    let x = line::extras(&s, &key(LN), &Budget::new(1 << 20));
    assert_eq!(x.begin, None);
    assert_eq!(x.end.as_deref(), Some("fragment_end"));
    assert_eq!(x.scene, Some(key(SC2)));
    assert_eq!(x.phase, Some(1));
    for r in [rules::LINE_FRAGMENTS, rules::LINE_START_SCENE] {
        assert!(rules::ALL.contains(&r));
    }
}

/// Facts with the player word on and the caller's assumed sex.
struct Sex<'s> {
    store: &'s Store,
    sex: Option<u32>,
}
impl crate::cond::Facts for Sex<'_> {
    fn store(&self) -> &Store {
        self.store
    }
    fn quest_running(&mut self, _: &fo4_plugin::FormKey) -> Result<bool, &'static str> {
        Err("no")
    }
    fn quest_stage(&mut self, _: &fo4_plugin::FormKey) -> Result<i32, &'static str> {
        Err("no")
    }
    fn stage_done(&mut self, _: &fo4_plugin::FormKey, _: i32) -> Result<bool, &'static str> {
        Err("no")
    }
    fn global(&mut self, _: &fo4_plugin::FormKey) -> Result<f32, &'static str> {
        Err("no")
    }
    fn player_word_on(&self) -> bool {
        true
    }
    fn player_sex(&self) -> Option<u32> {
        self.sex
    }
}

#[test]
fn function_70_on_the_player_reads_the_callers_sex_only_with_its_switch() {
    use crate::cond::{eval_term, Truth};
    use fo4_plugin::{actor::Context, condition::Ctda};
    let s = door_store();
    // Function 70 == 1 with parameter 1 (the second value), run on the
    // reference word 0x14.
    let c = Ctda {
        byte0: 0,
        unused: [0; 3],
        value: 1.0f32.to_bits(),
        function: 70,
        pad: [0; 2],
        param1: 1,
        param2: 0,
        run_on: 2,
        reference: 0x14,
        param3: -1,
    };
    let ctx = Context {
        order: s.order(),
        plugin: 0,
    };
    let eval = |sex: Option<u32>| {
        let mut f = Sex { store: &s, sex };
        let mut used = std::collections::BTreeSet::new();
        let t = eval_term(&c, ctx, "test.esm", &mut f, &mut used);
        (t, used.contains(rules::COND_FN_70_PLAYER_SEX))
    };
    let (t, used) = eval(None);
    assert!(
        matches!(t, Truth::Undetermined(_)) && !used,
        "off: as before"
    );
    assert_eq!(eval(Some(1)), (Truth::True, true));
    assert_eq!(eval(Some(0)), (Truth::False, true));
    // On another subject the reading does not apply.
    let other = Ctda {
        reference: TRIG,
        ..c
    };
    let mut f = Sex {
        store: &s,
        sex: Some(1),
    };
    let mut used = std::collections::BTreeSet::new();
    let t = eval_term(&other, ctx, "test.esm", &mut f, &mut used);
    assert!(matches!(t, Truth::Undetermined(_)));
}

const GQ: u32 = 0xA00;
const GD: u32 = 0xA01;
const G1: u32 = 0xA02;
const L1: u32 = 0xA03;
const G2: u32 = 0xA04;
const L2: u32 = 0xA05;
const GUARD: u32 = 0xA10;
const HERO: u32 = 0xA11;
const LONE: u32 = 0xA12;
const ACTOR: u32 = 0xA20;

/// A CTDA: function 72 (`==` 1) on a base, run on `run_on`.
fn is_id(base: u32, run_on: u32) -> Vec<u8> {
    let mut c = vec![0u8; 32];
    c[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    c[8..10].copy_from_slice(&72u16.to_le_bytes());
    c[12..16].copy_from_slice(&base.to_le_bytes());
    c[20..24].copy_from_slice(&run_on.to_le_bytes());
    c[28..32].copy_from_slice(&(-1i32).to_le_bytes());
    sub(b"CTDA", &c)
}

/// GQ starts with the game (stages 10 and 20). Its greeting topic GD
/// holds two groups (no responses): G1 when GUARD speaks, G2 when LONE
/// speaks; and two lines with no conditions of their own: L1 in G1 (TIQS
/// 10 at its begin), L2 in G2 (TIQS 20 at its begin). GUARD's actor is
/// placed in an exterior cell.
fn group_store() -> Store {
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let stage = |n: u16| {
        let mut d = n.to_le_bytes().to_vec();
        d.extend([0, 0]);
        [sub(b"INDX", &d), sub(b"QSDT", &[0])].concat()
    };
    let mut gq = [sub(b"EDID", b"GQ\0"), sub(b"DNAM", &d)].concat();
    gq.extend(stage(10));
    gq.extend(stage(20));
    let tiqs = |a: i16| sub(b"TIQS", &[a.to_le_bytes(), (-1i16).to_le_bytes()].concat());
    let resp = sub(b"TRDA", &trda(1));
    let infos = [
        record(
            b"INFO",
            G1,
            &[sub(b"ENAM", &w(0)), is_id(GUARD, 0)].concat(),
        ),
        record(
            b"INFO",
            L1,
            &[
                sub(b"ENAM", &w(0)),
                sub(b"GNAM", &w(G1)),
                resp.clone(),
                tiqs(10),
            ]
            .concat(),
        ),
        record(b"INFO", G2, &[sub(b"ENAM", &w(0)), is_id(LONE, 0)].concat()),
        record(
            b"INFO",
            L2,
            &[sub(b"ENAM", &w(0)), sub(b"GNAM", &w(G2)), resp, tiqs(20)].concat(),
        ),
    ]
    .concat();
    let gd = [
        sub(b"EDID", b"GD\0"),
        sub(b"PNAM", &50.0f32.to_le_bytes()),
        sub(b"QNAM", &w(GQ)),
        sub(b"DATA", &[0, 7, 0, 0]),
        sub(b"SNAM", b"GREE"),
        sub(b"TIFC", &w(4)),
    ]
    .concat();
    let children = [record(b"DIAL", GD, &gd), group(7, GD.to_le_bytes(), &infos)].concat();
    let quests = [
        record(b"QUST", GQ, &gq),
        group(10, GQ.to_le_bytes(), &children),
    ]
    .concat();
    let actor = record(
        b"ACHR",
        ACTOR,
        &[
            sub(b"NAME", &w(GUARD)),
            sub(b"DATA", &data([1000.0, 1100.0, 0.0])),
        ]
        .concat(),
    );
    let mut xclc = 0i32.to_le_bytes().to_vec();
    xclc.extend(0i32.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    let cell = record(
        b"CELL",
        EXT,
        &[
            sub(b"EDID", b"Ext\0"),
            sub(b"DATA", &2u16.to_le_bytes()),
            sub(b"XCLC", &xclc),
        ]
        .concat(),
    );
    let world = [
        record(b"WRLD", WRLD, &sub(b"EDID", b"W\0")),
        group(
            1,
            WRLD.to_le_bytes(),
            &[
                cell,
                group(6, EXT.to_le_bytes(), &group(9, EXT.to_le_bytes(), &actor)),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut plugin = header(12);
    plugin.extend(group(
        0,
        *b"NPC_",
        &[
            record(b"NPC_", GUARD, &sub(b"EDID", b"Guard\0")),
            record(b"NPC_", HERO, &sub(b"EDID", b"Hero\0")),
            record(b"NPC_", LONE, &sub(b"EDID", b"Lone\0")),
        ]
        .concat(),
    ));
    plugin.extend(group(0, *b"QUST", &quests));
    plugin.extend(group(0, *b"WRLD", &world));
    sources(plugin)
}

/// Greets GUARD's actor with the group reading as given; returns the
/// stages of GQ set and the talk counts.
fn greet(info_groups: bool) -> (Vec<i32>, std::collections::BTreeMap<String, u64>) {
    let s = group_store();
    let b = Budget::new(1 << 24);
    let mut l = super::quest::library();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.scenes.on = true;
    vm.world.scenes.clock = scene::Clock {
        on: true,
        assume_line_seconds: Some(2.0),
        ..scene::Clock::default()
    };
    vm.world.talk.on = true;
    vm.world.talk.sw = crate::talk::Switches {
        greeting: true,
        stage_fields: true,
        info_groups,
        ..crate::talk::Switches::default()
    };
    vm.world.talk.player_base = Some(key(HERO));
    quest::start_new_game(&mut vm, &s, &b).unwrap();
    quest::run_jobs(&mut vm, &s, 256, &b);
    let actor = crate::data::record_object(&mut vm.world, &s, &key(ACTOR), "actor", &b).unwrap();
    let _ = vm.host.call(&crate::talk::talk_call(actor), &mut vm.world);
    quest::run_jobs(&mut vm, &s, 256, &b);
    let stages = vm
        .world
        .quests
        .log
        .iter()
        .filter_map(|(_, c)| match c {
            quest::Change::StageSet(n) => Some(*n),
            _ => None,
        })
        .collect();
    (stages, vm.world.talk.counts.clone())
}

#[test]
fn a_greeting_line_holds_only_with_its_group_and_a_group_does_not_play() {
    // Off: both lines (and GUARD's group) hold: the line is undetermined.
    let (stages, counts) = greet(false);
    assert!(stages.is_empty(), "{stages:?}");
    assert_eq!(
        counts.get("greeting: line undetermined: several lines hold"),
        Some(&1)
    );
    // On: L1 holds with G1 when GUARD speaks; L2's group fails; the groups
    // are not candidates. L1 plays and its begin half sets stage 10.
    let (stages, counts) = greet(true);
    assert_eq!(stages, [10], "{counts:?}");
    assert_eq!(
        counts.get("greeting: line determined: the only line that holds"),
        Some(&1)
    );
    assert!(rules::ALL.contains(&rules::INFO_GROUPS));
}

#[test]
fn the_callers_choice_for_a_scene_comes_before_its_quests_and_the_general_one() {
    let info = |scene: u32| scene::SceneInfo {
        key: key(scene),
        plugin: 0,
        file: "test.esm".into(),
        flags: 0,
        quest: Some(key(QS)),
        begin: None,
        end: None,
        phases: Vec::new(),
        actions: Vec::new(),
        stage_fields: [-1, -1],
        unplaced: 0,
    };
    let mut c = scene::Clock {
        assume_player_choice: Some(0),
        ..scene::Clock::default()
    };
    assert_eq!(c.choice_in(&info(SC)), Some(0));
    c.choice_for.insert(crate::bind::record_key(&key(QS)), 3);
    assert_eq!(c.choice_in(&info(SC)), Some(3));
    c.choice_for.insert(crate::bind::record_key(&key(SC2)), 1);
    assert_eq!(c.choice_in(&info(SC2)), Some(1));
    assert_eq!(c.choice_in(&info(SC)), Some(3));
    assert!(!c.first_line_in(&info(SC)));
    c.first_line_for.insert(crate::bind::record_key(&key(SC2)));
    assert!(c.first_line_in(&info(SC2)) && !c.first_line_in(&info(SC)));
}
