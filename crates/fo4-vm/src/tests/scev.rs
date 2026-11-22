//! The scene clock, timer and dialogue actions, voice paths and the
//! speaker's voice type, over a synthetic plugin (no game data).
use super::quest::{group, key, record, sub, Owned};
use crate::{
    cond::Truth,
    quest::{self, QuestHost},
    rules,
    scene::{self, Change},
    voice::{self, LineTime, Speaker, VoiceFiles},
    Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::typed::Store;
use std::{collections::BTreeSet, rc::Rc};

const QS: u32 = 0x900;
const SC: u32 = 0x901;
const VT: u32 = 0x910;
const DL: u32 = 0x920;
const LN: u32 = 0x921;

fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
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
fn action(kind: u8, alias: i32, index: u32, p: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut v = [
        sub(b"ANAM", &[kind, 0]),
        sub(b"NAM0", b"\0"),
        sub(b"ALID", &(alias as u32).to_le_bytes()),
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
fn trda(n: u8) -> Vec<u8> {
    let mut d = vec![0xff; 4];
    d.extend([n, 0, 0, 0, 0, 1, 0, 0]);
    d.extend([0xff; 8]);
    d
}

/// QS starts with the game; alias 0 has a voice type (VT), alias 1 none.
/// SC (FNAM bit 0x1) has three phases: a timer of 2 s (with a TNAM when
/// `tnam`) in phase 0, a line of topic DL spoken by alias `speaker` in
/// phase 1, a package in phase 2. DL holds one line LN of two responses.
fn store(tnam: bool, speaker: i32) -> Store {
    let mut timer = vec![sub(b"SNAM", &2.0f32.to_le_bytes())];
    if tnam {
        timer.push(sub(b"TNAM", &1.0f32.to_le_bytes()));
    }
    let sc = [
        sub(b"EDID", b"SC\0"),
        sub(b"FNAM", &w(0x1)),
        phase(),
        phase(),
        phase(),
        sub(b"ALID", &w(0)),
        sub(b"LNAM", &w(0)),
        sub(b"DNAM", &w(0)),
        action(2, -1, 1, 0, &timer),
        action(0, speaker, 2, 1, &[sub(b"DATA", &w(DL))]),
        action(1, 0, 3, 2, &[sub(b"PNAM", &w(0))]),
        sub(b"PNAM", &w(QS)),
        sub(b"INAM", &w(4)),
    ]
    .concat();
    let ln = [
        sub(b"ENAM", &w(0)),
        sub(b"TRDA", &trda(1)),
        sub(b"NAM1", &w(1)),
        sub(b"TRDA", &trda(2)),
        sub(b"NAM1", &w(2)),
    ]
    .concat();
    let dl = [sub(b"EDID", b"DL\0"), sub(b"TIFC", &w(1))].concat();
    let mut d = 0x0001u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let qs = [
        sub(b"EDID", b"QS\0"),
        sub(b"DNAM", &d),
        sub(b"ANAM", &w(2)),
        sub(b"ALST", &w(0)),
        sub(b"ALID", b"A\0"),
        sub(b"FNAM", &w(0)),
        sub(b"VTCK", &w(VT)),
        sub(b"ALED", &[]),
        sub(b"ALST", &w(1)),
        sub(b"ALID", b"B\0"),
        sub(b"FNAM", &w(0)),
        sub(b"ALED", &[]),
    ]
    .concat();
    let children = [
        record(b"SCEN", SC, &sc),
        record(b"DIAL", DL, &dl),
        group(7, DL.to_le_bytes(), &record(b"INFO", LN, &ln)),
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
    plugin.extend(group(
        0,
        *b"VTYP",
        &record(b"VTYP", VT, &sub(b"EDID", b"VoiceA\0")),
    ));
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

/// The voice files: both responses of LN under the speaker's voice type
/// (1.5 s and 0.5 s), the first also under another voice type.
fn voice(store: &Store) -> voice::Voice {
    let mut files = VoiceFiles::default();
    assert!(files.add(b"sound/voice/test.esm/voicea/00000921_1.fuz", Some(1.5)));
    assert!(files.add(b"sound/voice/test.esm/voicea/00000921_2.fuz", Some(0.5)));
    assert!(files.add(b"sound/voice/test.esm/voiceb/00000921_1.fuz", Some(9.0)));
    assert!(files.add(b"sound/voice/test.esm/voiceb/00000921_2.fuz", Some(9.0)));
    voice::Voice {
        files,
        topics: voice::Topics::read(store),
    }
}

/// Starts a new game with the clock as given and runs the jobs; returns
/// the scene's log.
fn run(store: &Store, clock: scene::Clock, steps: usize) -> (Vec<Change>, BTreeSet<&'static str>) {
    let b = Budget::new(1 << 24);
    let mut l = super::scene::lib();
    let mut host = QuestHost::new(store, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.scenes.on = true;
    vm.world.scenes.begin_on_quest_start = true;
    vm.world.scenes.clock = clock;
    quest::start_new_game(&mut vm, store, &b).unwrap();
    quest::run_jobs(&mut vm, store, 64, &b);
    for _ in 0..steps {
        if scene::advance_clock(&mut vm.world, None).is_none() {
            break;
        }
        quest::run_jobs(&mut vm, store, 64, &b);
    }
    let sc = crate::data::record_object(&mut vm.world, store, &key(SC), "scene", &b).unwrap();
    let mut used: BTreeSet<&'static str> = vm.rules.iter().copied().collect();
    used.extend(vm.host.rules());
    let log = vm
        .world
        .scenes
        .log
        .iter()
        .filter(|(o, _)| *o == sc)
        .map(|x| x.1.clone())
        .collect();
    (log, used)
}

fn waits(log: &[Change]) -> Option<(usize, String)> {
    match log.last() {
        Some(Change::Waiting(p, why)) => Some((*p, why.clone())),
        _ => None,
    }
}

#[test]
fn without_the_clock_phases_wait_on_every_action_as_before() {
    let s = store(false, 0);
    let (log, _) = run(&s, scene::Clock::default(), 5);
    let (p, why) = waits(&log).unwrap();
    assert_eq!(p, 0);
    assert!(why.contains("kind 2"), "{why}");
    assert!(!log.iter().any(|c| matches!(c, Change::ActionStarted(..))));
}

#[test]
fn the_clock_times_the_timer_and_the_line_then_waits_on_the_package() {
    let s = store(false, 0);
    let clock = scene::Clock {
        on: true,
        timers: true,
        voice: Some(Rc::new(voice(&s))),
        ..scene::Clock::default()
    };
    // Before any advance: the timer is due at 2 s, phase 0 waits on it.
    let (log, _) = run(&s, clock.clone(), 0);
    assert_eq!(log.last(), Some(&Change::OnClock(0)));
    // The timer completes at 2 s, the line (1.5 + 0.5 s of the speaker's
    // voice type) at 4 s; the package is not timed.
    let (log, used) = run(&s, clock.clone(), 10);
    let done: Vec<(usize, f64)> = log
        .iter()
        .filter_map(|c| match c {
            Change::ActionCompleted(j, _, t) => Some((*j, *t)),
            _ => None,
        })
        .collect();
    assert_eq!(done, [(0, 2.0), (1, 4.0)]);
    assert!(log.contains(&Change::PhaseCompleted(1)));
    let (p, why) = waits(&log).unwrap();
    assert_eq!(p, 2);
    assert!(why.contains("kind 1"), "{why}");
    for r in [
        rules::SCENE_CLOCK,
        rules::TIMER_ACTION,
        rules::LINE_AUDIO,
        rules::LINE_CHOICE,
        rules::VOICE_PATH,
        rules::SPEAKER_VOICE_TYPE,
        rules::DPDS_SECONDS,
    ] {
        assert!(used.contains(r), "{r}");
    }
    // Assuming packages complete, the scene runs to its end.
    let (log, _) = run(
        &s,
        scene::Clock {
            assume_packages: true,
            ..clock.clone()
        },
        10,
    );
    assert_eq!(log.last(), Some(&Change::Ended));
    // Timer actions off: the timer waits as before.
    let (log, _) = run(
        &s,
        scene::Clock {
            timers: false,
            ..clock
        },
        10,
    );
    assert!(waits(&log).unwrap().1.contains("kind 2"));
}

#[test]
fn unsettled_times_wait_unless_the_caller_assumes_them() {
    // A TNAM beside SNAM: the timer's time is not settled.
    let s = store(true, 0);
    let clock = scene::Clock {
        on: true,
        timers: true,
        voice: Some(Rc::new(voice(&s))),
        ..scene::Clock::default()
    };
    let (log, _) = run(&s, clock, 10);
    assert!(log
        .iter()
        .any(|c| matches!(c, Change::ActionStarted(0, 0, 2, how) if how.contains("TNAM"))));
    assert!(waits(&log).unwrap().1.contains("kind 2"));
    // A speaker alias without a voice type and two voice types holding the
    // line: undetermined, then timed by the caller's assumption.
    let s = store(false, 1);
    let clock = scene::Clock {
        on: true,
        timers: true,
        voice: Some(Rc::new(voice(&s))),
        ..scene::Clock::default()
    };
    let (log, _) = run(&s, clock.clone(), 10);
    assert!(log.iter().any(|c| matches!(
        c,
        Change::ActionStarted(1, 1, 0, how) if how.contains("several voice types")
    )));
    assert_eq!(waits(&log).unwrap().0, 1);
    let (log, _) = run(
        &s,
        scene::Clock {
            assume_line_seconds: Some(7.0),
            ..clock
        },
        10,
    );
    assert!(log.contains(&Change::ActionCompleted(1, 0, 9.0)));
    // Lines not timed by their audio and no assumption: the line waits.
    let (log, _) = run(
        &s,
        scene::Clock {
            on: true,
            timers: true,
            ..scene::Clock::default()
        },
        10,
    );
    assert!(log.iter().any(
        |c| matches!(c, Change::ActionStarted(1, 1, 0, how) if how.contains("line audio off"))
    ));
}

#[test]
fn voice_paths_follow_the_convention_or_are_left_out() {
    let p = voice::parse_path(b"sound/voice/a.esm/vt/0001abcd_12.fuz").unwrap();
    assert_eq!(
        (
            p.plugin.as_str(),
            p.voice_type.as_str(),
            p.object_id,
            p.response
        ),
        ("a.esm", "vt", 0x1abcd, 12)
    );
    for bad in [
        &b"sound/voice/a.esm/vt/0001abcd_1_alt.fuz"[..],
        b"sound/voice/a.esm/vt/0001abcd.fuz",
        b"sound/voice/a.esm/vt/001abcd_1.fuz",
        b"sound/voice/a.esm/vt/sub/0001abcd_1.fuz",
        b"sound/voice/a.esm/vt/0001abcd_1.wav",
        b"sound/fx/a.esm/vt/0001abcd_1.fuz",
        b"sound/voice/a.esm/vt/name_m.fuz",
    ] {
        assert!(voice::parse_path(bad).is_none(), "{bad:?}");
    }
    let mut f = VoiceFiles::default();
    assert!(!f.add(b"sound/voice/a.esm/vt/name_m.fuz", None));
    assert!(f.add(b"sound/voice/a.esm/vt/0001abcd_1.fuz", None));
    let k = fo4_plugin::FormKey {
        plugin: "A.esm".into(),
        object_id: 0x1abcd,
    };
    assert_eq!(f.get(&k, 1).len(), 1);
    assert!(f.get(&k, 2).is_empty());
    assert_eq!(f.named, 1);
}

#[test]
fn the_speaker_and_the_line_come_from_the_records() {
    let s = store(false, 0);
    let b = Budget::new(1 << 24);
    assert_eq!(
        voice::speaker(&s, &key(QS), 0, &b),
        Speaker::VoiceType("voicea".into(), "the alias's voice type")
    );
    assert!(
        matches!(voice::speaker(&s, &key(QS), 1, &b), Speaker::Unknown(w) if w.contains("filled by the world"))
    );
    assert!(matches!(
        voice::speaker(&s, &key(QS), 7, &b),
        Speaker::Unknown(_)
    ));
    assert!(matches!(
        voice::speaker(&s, &key(QS), -1, &b),
        Speaker::Unknown(_)
    ));
    let v = voice(&s);
    assert_eq!(v.topics.of(&key(DL)), [key(LN)]);
    let mut used = BTreeSet::new();
    let sp = Speaker::VoiceType("voiceb".into(), "test");
    let mut yes = |_: &fo4_plugin::info::Info, _: usize, _: &str| Truth::True;
    assert_eq!(
        voice::line_seconds(&s, &v, &key(DL), &sp, &mut yes, &mut used, &b),
        LineTime::Seconds(18.0)
    );
    // Conditions that do not hold, or are undetermined.
    let mut no = |_: &fo4_plugin::info::Info, _: usize, _: &str| Truth::False;
    assert!(matches!(
        voice::line_seconds(&s, &v, &key(DL), &sp, &mut no, &mut used, &b),
        LineTime::Undetermined(w) if w.contains("no line")
    ));
    let mut open = |_: &fo4_plugin::info::Info, _: usize, _: &str| {
        Truth::Undetermined(vec![crate::cond::Why {
            function: 70,
            why: "test",
        }])
    };
    assert!(matches!(
        voice::line_seconds(&s, &v, &key(DL), &sp, &mut open, &mut used, &b),
        LineTime::Undetermined(w) if w.contains("function 70")
    ));
    // A topic without lines.
    assert!(matches!(
        voice::line_seconds(&s, &v, &key(QS), &sp, &mut yes, &mut used, &b),
        LineTime::Undetermined(w) if w.contains("no line")
    ));
}
