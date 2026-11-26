//! The player position model (`crate::track`): track parsing, volumes and
//! their crossings, the trigger and distance events and the position
//! natives over synthetic geometry and a synthetic plugin (no game data).
use super::quest::{group, record, sub, text, Owned};
use super::{assemble, base};
use crate::{
    bind::RecordScripts,
    quest::QuestHost,
    rules,
    track::{self, Place, Primitive, Shape, Switches, Track, Volume},
    NativeCall, Reply, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::{typed::Store, FormKey};
use fo4_scene::transform::Transform;

pub(super) const WRLD: u32 = 0x800;
pub(super) const CELL: u32 = 0x801;
const ACTI: u32 = 0x802;
const STAT: u32 = 0x803;
pub(super) const TRIG: u32 = 0x810;
pub(super) const MARK: u32 = 0x811;
pub(super) const OFF: u32 = 0x812;

pub(super) fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    }
}

fn upright(t: [f64; 3], yaw: f32) -> Transform {
    Transform::euler([t[0] as f32, t[1] as f32, t[2] as f32, 0.0, 0.0, yaw], 1.0).unwrap()
}

#[test]
fn a_track_parses_and_moves_in_straight_lines_jumping_between_spaces() {
    let t = Track::parse(
        "# a comment\n0 test.esm:00000800 0 0 0\n10 test.esm:00000800 100 0 0 90\n\n20 test.esm:00000801 5 5 5\n",
    )
    .unwrap();
    assert_eq!(t.points.len(), 3);
    assert!(t.at(-1.0).is_none());
    assert_eq!(t.at(5.0).unwrap().1, [50.0, 0.0, 0.0]);
    // Before the point in another space the player stays at the earlier one.
    assert_eq!(t.at(15.0).unwrap().1, [100.0, 0.0, 0.0]);
    assert_eq!(t.at(15.0).unwrap().2, Some(90.0));
    assert_eq!(t.at(25.0).unwrap().0, &key(CELL));
    assert!(Track::parse("5 test.esm:00000800 0 0 0\n1 test.esm:00000800 0 0 0").is_err());
    assert!(Track::parse("0 test.esm:00000800 0 0").is_err());
    assert!(Track::parse("").is_err());
}

#[test]
fn the_primitive_reads_its_32_bytes() {
    let mut d = Vec::new();
    for f in [10.0f32, 20.0, 30.0, 1.0, 0.0, 1.0, 0.3] {
        d.extend(f.to_le_bytes());
    }
    d.extend(2u32.to_le_bytes());
    let p = Primitive::read(&d).unwrap();
    assert_eq!(p.bounds, [10.0, 20.0, 30.0]);
    assert_eq!(p.kind, 2);
    assert!(Primitive::read(&d[..28]).is_none());
    let on = Switches {
        boxes: true,
        ..Switches::default()
    };
    assert!(Switches::default().shape(&p, 1.0).is_err());
    assert!(on.shape(&p, 1.0).is_err(), "a sphere needs its own switch");
    let sw = Switches {
        spheres: true,
        ..Switches::default()
    };
    assert_eq!(sw.shape(&p, 2.0).unwrap(), Shape::Sphere(20.0));
}

#[test]
fn a_rotated_box_is_crossed_where_its_faces_are() {
    // A box 10 x 50 x 20 (half extents) turned a quarter about z: its long
    // side lies along world x.
    let v = Volume {
        key: key(TRIG),
        space: key(WRLD),
        frame: upright([100.0, 0.0, 0.0], std::f32::consts::FRAC_PI_2),
        shape: Shape::Box([10.0, 50.0, 20.0]),
    };
    assert!(v.contains([140.0, 0.0, 0.0]));
    assert!(!v.contains([100.0, 20.0, 0.0]));
    let (lo, hi) = v.span([0.0, 0.0, 0.0], [200.0, 0.0, 0.0]).unwrap();
    assert!(
        (lo - 0.25).abs() < 1e-6 && (hi - 0.75).abs() < 1e-6,
        "{lo} {hi}"
    );
    assert!(v.span([0.0, 30.0, 0.0], [200.0, 30.0, 0.0]).is_none());
    // A point under the box misses it; a body 50 units tall meets it.
    assert!(v.span([0.0, 0.0, -30.0], [200.0, 0.0, -30.0]).is_none());
    let (lo, hi) = v
        .span_body([0.0, 0.0, -30.0], [200.0, 0.0, -30.0], 50.0)
        .unwrap();
    assert!(
        (lo - 0.25).abs() < 1e-6 && (hi - 0.75).abs() < 1e-6,
        "{lo} {hi}"
    );
    assert!(v
        .span_body([0.0, 0.0, -80.0], [200.0, 0.0, -80.0], 50.0)
        .is_none());
}

#[test]
fn a_sphere_is_crossed_by_a_point_and_by_a_body() {
    let v = Volume {
        key: key(TRIG),
        space: key(WRLD),
        frame: upright([0.0, 0.0, 0.0], 0.0),
        shape: Shape::Sphere(10.0),
    };
    let (lo, hi) = v.span([-20.0, 0.0, 0.0], [20.0, 0.0, 0.0]).unwrap();
    assert!((lo - 0.25).abs() < 1e-9 && (hi - 0.75).abs() < 1e-9);
    assert!(v.span([-20.0, 0.0, -15.0], [20.0, 0.0, -15.0]).is_none());
    let (lo, hi) = v
        .span_body([-20.0, 0.0, -15.0], [20.0, 0.0, -15.0], 30.0)
        .unwrap();
    assert!(
        (lo - 0.25).abs() < 1e-6 && (hi - 0.75).abs() < 1e-6,
        "{lo} {hi}"
    );
}

#[test]
fn intervals_join_across_points_and_drop_touches() {
    let v = Volume {
        key: key(TRIG),
        space: key(WRLD),
        frame: upright([50.0, 0.0, 0.0], 0.0),
        shape: Shape::Box([10.0, 10.0, 10.0]),
    };
    let t = Track::parse(
        "0 test.esm:00000800 0 0 0\n5 test.esm:00000800 50 0 0\n10 test.esm:00000800 100 0 0\n20 test.esm:00000800 100 10 0\n30 test.esm:00000800 0 10 0",
    )
    .unwrap();
    let iv = track::intervals(&t, &v, None);
    // In from 4 to 6 (joined over the point at 5), then along the face
    // y = 10 from 24 to 26: closed, so held.
    assert_eq!(iv.len(), 2, "{iv:?}");
    assert!((iv[0].0 - 4.0).abs() < 1e-9 && (iv[0].1 - 6.0).abs() < 1e-9);
    assert!((iv[1].0 - 24.0).abs() < 1e-9 && (iv[1].1 - 26.0).abs() < 1e-9);
    // Only touching a corner is no hold.
    let t2 = Track::parse("0 test.esm:00000800 40 -20 0\n10 test.esm:00000800 20 0 0").unwrap();
    assert!(track::intervals(&t2, &v, None).is_empty());
    // After the last point the player stays: no end.
    let t3 = Track::parse("0 test.esm:00000800 0 0 0\n10 test.esm:00000800 50 0 0").unwrap();
    let iv = track::intervals(&t3, &v, None);
    assert_eq!(iv.len(), 1);
    assert!(iv[0].1.is_infinite());
}

fn data(pos: [f32; 3]) -> Vec<u8> {
    let mut d = Vec::new();
    for f in pos.into_iter().chain([0.0; 3]) {
        d.extend(f.to_le_bytes());
    }
    d
}

fn xprm(bounds: [f32; 3], kind: u32) -> Vec<u8> {
    let mut d = Vec::new();
    for f in bounds.into_iter().chain([1.0, 0.0, 1.0, 0.3]) {
        d.extend(f.to_le_bytes());
    }
    d.extend(kind.to_le_bytes());
    d
}

/// A worldspace with one exterior cell at grid (0, 0) holding a trigger
/// box (half extents 50) at (1000, 1000, 0) whose script `Trig` handles
/// enter and leave, an initially disabled second trigger at (1000, 2000,
/// 0) and a marker at (3000, 1000, 0).
pub(super) fn store() -> Store {
    let mut vmad = vec![6, 0, 2, 0];
    vmad.extend(1u16.to_le_bytes());
    text(&mut vmad, "Trig");
    vmad.push(0);
    vmad.extend(0u16.to_le_bytes());
    let trig = |id: u32, at: [f32; 3], flags: u32| {
        let mut r = record(
            b"REFR",
            id,
            &[
                sub(b"NAME", &ACTI.to_le_bytes()),
                sub(b"VMAD", &vmad),
                sub(b"XPRM", &xprm([50.0, 50.0, 50.0], 1)),
                sub(b"DATA", &data(at)),
            ]
            .concat(),
        );
        r[8..12].copy_from_slice(&flags.to_le_bytes());
        r
    };
    let refs = [
        trig(TRIG, [1000.0, 1000.0, 0.0], 0),
        trig(OFF, [1000.0, 2000.0, 0.0], 0x800),
        record(
            b"REFR",
            MARK,
            &[
                sub(b"NAME", &STAT.to_le_bytes()),
                sub(b"DATA", &data([3000.0, 1000.0, 0.0])),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut xclc = 0i32.to_le_bytes().to_vec();
    xclc.extend(0i32.to_le_bytes());
    xclc.extend(0u32.to_le_bytes());
    let cell = record(
        b"CELL",
        CELL,
        &[
            sub(b"EDID", b"Ext\0"),
            sub(b"DATA", &2u16.to_le_bytes()),
            sub(b"XCLC", &xclc),
        ]
        .concat(),
    );
    let cell_children = group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs));
    let world = [
        record(b"WRLD", WRLD, &sub(b"EDID", b"W\0")),
        group(1, WRLD.to_le_bytes(), &[cell, cell_children].concat()),
    ]
    .concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(6u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(
        0,
        *b"ACTI",
        &record(b"ACTI", ACTI, &sub(b"EDID", b"T\0")),
    ));
    plugin.extend(group(
        0,
        *b"STAT",
        &record(b"STAT", STAT, &sub(b"EDID", b"M\0")),
    ));
    plugin.extend(group(0, *b"WRLD", &world));
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
    let mut l = base();
    l.add(assemble(
        "script Trig extends ObjectReference
         var Int entered 0
         var Int left 0
         func OnTriggerEnter none ObjectReference akActionRef
         1 entered entered 1
         func OnTriggerLeave none ObjectReference akActionRef
         1 left left 1",
    ));
    l.add(assemble(
        "script Dist extends ScriptObject
         var Float got 0.0
         func OnDistanceLessThan none ObjectReference akObj1 ObjectReference akObj2 Float afDistance
         13 got afDistance",
    ));
    l
}

pub(super) fn all_on() -> Switches {
    Switches {
        boxes: true,
        spheres: true,
        point: true,
        instant: true,
        enabled: true,
        order: true,
        distance: true,
        distance_once: true,
        placed: true,
        position_natives: true,
        cell_natives: true,
        location_native: true,
        ..Switches::default()
    }
}

pub(super) const WALK: &str = "0 test.esm:00000801 0 1000 0\n20 test.esm:00000801 2000 1000 0\n40 test.esm:00000801 3000 1000 0\n";

/// Walks the track, settling at every due second; returns the raised
/// events with their seconds and the trigger script's two counters.
fn walk(sw: Switches) -> (Vec<(f64, &'static str)>, Val, Val, Place) {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.world.place = Place::load(&s, Track::parse(WALK).unwrap(), sw, &b).unwrap();
    let mut out = Vec::new();
    for _ in 0..32 {
        let (raised, _) = track::settle(&mut vm, &s, 64, &b);
        out.extend(raised.iter().map(|r| (r.t, r.event)));
        let Some(t) = track::next_due(&s, 1 << 24, &mut vm.world) else {
            break;
        };
        vm.world.now[crate::world::Clock::Real as usize] = t;
    }
    let o = vm
        .world
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some("test.esm:00000810"));
    let var = |vm: &Vm<'_>, n: &str| {
        o.and_then(|o| vm.world.objects[o].instances.first().copied())
            .map(|i| {
                vm.world.instances[i]
                    .vars
                    .iter()
                    .find(|v| v.name == n)
                    .unwrap()
                    .value
                    .clone()
            })
            .unwrap_or(Val::None)
    };
    let (e, l2) = (var(&vm, "entered"), var(&vm, "left"));
    let place = std::mem::take(&mut vm.world.place);
    (out, e, l2, place)
}

#[test]
fn the_cell_resolves_to_its_worldspace_and_the_trigger_volumes_load() {
    let s = store();
    assert_eq!(s.world(&key(CELL)), Some(key(WRLD)));
    let p = Place::load(
        &s,
        Track::parse(WALK).unwrap(),
        all_on(),
        &Budget::new(1 << 24),
    )
    .unwrap();
    assert_eq!(p.volumes.len(), 2);
    assert_eq!(p.track.as_ref().unwrap().points[0].space, key(WRLD));
    // The enabled trigger's box is entered at x = 950 (9.5 s) and left at
    // x = 1050 (10.5 s); the disabled one is never crossed.
    let c: Vec<(f64, bool)> = p.crossings.iter().map(|c| (c.t, c.enter)).collect();
    assert_eq!(c.len(), 2, "{c:?}");
    assert!((c[0].0 - 9.5).abs() < 1e-9 && c[0].1);
    assert!((c[1].0 - 10.5).abs() < 1e-9 && !c[1].1);
}

#[test]
fn crossing_a_trigger_raises_enter_then_leave_with_every_rule_on() {
    let (ev, entered, left, place) = walk(all_on());
    let triggers: Vec<_> = ev.iter().filter(|e| e.1.starts_with("ontrigger")).collect();
    assert_eq!(triggers.len(), 2, "{ev:?}");
    assert_eq!(triggers[0].1, "ontriggerenter");
    assert!((triggers[0].0 - 9.5).abs() < 1e-9);
    assert_eq!(triggers[1].1, "ontriggerleave");
    assert_eq!(entered, Val::Int(1));
    assert_eq!(left, Val::Int(1));
    assert_eq!(
        place.counts["trigger events raised\tontriggerenter\tthe track crossed the volume"],
        1
    );
}

#[test]
fn with_a_needed_rule_off_nothing_is_raised_and_the_reason_is_counted() {
    let sw = Switches {
        instant: false,
        ..all_on()
    };
    let (ev, entered, _, place) = walk(sw);
    assert!(ev.iter().all(|e| !e.1.starts_with("ontrigger")), "{ev:?}");
    assert_ne!(entered, Val::Int(1));
    assert_eq!(
        place.counts["trigger events not raised\tenter\t--trigger-instant off"],
        1
    );
    // Without a shape switch no volume loads at all.
    let p = Place::load(
        &store(),
        Track::parse(WALK).unwrap(),
        Switches {
            boxes: false,
            ..all_on()
        },
        &Budget::new(1 << 24),
    )
    .unwrap();
    assert!(p.volumes.is_empty());
    assert_eq!(
        p.counts["activator references near the track not modelled\ta box (--trigger-box off)"],
        2
    );
}

#[test]
fn a_distance_registration_fires_once_where_the_track_comes_within_the_threshold() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = lib();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    // No trigger volumes: the distance alone is due.
    let sw = Switches {
        boxes: false,
        ..all_on()
    };
    vm.world.place = Place::load(&s, Track::parse(WALK).unwrap(), sw, &b).unwrap();
    let player = vm.world.player("actor");
    let mark =
        crate::data::record_object(&mut vm.world, &s, &key(MARK), "objectreference", &b).unwrap();
    let inst = vm.instantiate(None, "dist").unwrap();
    let call = NativeCall {
        script: "scriptobject".into(),
        name: "registerfordistancelessthanevent".into(),
        receiver: None,
        instance: Some(inst),
        args: vec![Val::Object(player), Val::Object(mark), Val::Float(500.0)],
        param_types: vec!["scriptobject".into(), "scriptobject".into(), "float".into()],
        return_type: "none".into(),
    };
    let (r, rule) = track::call(&s, 1 << 24, &call, &mut vm.world).unwrap();
    assert!(matches!(r, Reply::Value(Val::None)));
    assert_eq!(rule, Some(rules::WORLD_DISTANCE_EVENTS));
    // 500 units from the marker at x = 2500: 20 s + 500 / 50 units per s.
    let due = track::next_due(&s, 1 << 24, &mut vm.world).unwrap();
    assert!((due - 30.0).abs() < 1e-6, "{due}");
    vm.world.now[crate::world::Clock::Real as usize] = due;
    let (raised, _) = track::settle(&mut vm, &s, 64, &b);
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].event, "ondistancelessthan");
    let got = vm.world.instances[inst]
        .vars
        .iter()
        .find(|v| v.name == "got")
        .unwrap()
        .value
        .clone();
    assert_eq!(got, Val::Float(500.0));
    assert!(vm.world.place.registrations.is_empty());
    assert!(vm.rules.contains(rules::WORLD_DISTANCE_ONCE));
}

#[test]
fn position_natives_answer_from_the_track_and_the_stored_positions() {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut w = crate::World {
        place: Place::load(&s, Track::parse(WALK).unwrap(), all_on(), &b).unwrap(),
        ..crate::World::default()
    };
    w.now[crate::world::Clock::Real as usize] = 10.0;
    let player = w.player("actor");
    let mark = crate::data::record_object(&mut w, &s, &key(MARK), "objectreference", &b).unwrap();
    let native = |name: &str, recv: usize, args: Vec<Val>, ret: &str| NativeCall {
        script: "objectreference".into(),
        name: name.into(),
        receiver: Some(Val::Object(recv)),
        instance: None,
        args,
        param_types: Vec::new(),
        return_type: ret.into(),
    };
    let (r, _) = track::call(
        &s,
        1 << 24,
        &native("getpositionx", player, vec![], "float"),
        &mut w,
    )
    .unwrap();
    assert!(matches!(r, Reply::Value(Val::Float(x)) if x == 1000.0));
    let (r, _) = track::call(
        &s,
        1 << 24,
        &native("getdistance", player, vec![Val::Object(mark)], "float"),
        &mut w,
    )
    .unwrap();
    assert!(matches!(r, Reply::Value(Val::Float(x)) if x == 2000.0));
    let (r, _) = track::call(
        &s,
        1 << 24,
        &native("getparentcell", player, vec![], "cell"),
        &mut w,
    )
    .unwrap();
    let Reply::Value(Val::Object(c)) = r else {
        panic!("{r:?}")
    };
    assert_eq!(w.objects[c].key.as_deref(), Some("test.esm:00000801"));
    // Line of sight only as the caller's assumption.
    assert!(track::call(
        &s,
        1 << 24,
        &native("hasdirectlos", player, vec![Val::Object(mark)], "bool"),
        &mut w
    )
    .is_none());
    // A moved reference is no longer at its stored place.
    track::observe(
        &s,
        1 << 24,
        &native("moveto", mark, vec![Val::Object(player)], "none"),
        &mut w,
    );
    let (r, _) = track::call(
        &s,
        1 << 24,
        &native("getpositionx", mark, vec![], "float"),
        &mut w,
    )
    .unwrap();
    assert!(matches!(r, Reply::Undetermined));
    // Off: the model answers nothing.
    let mut off = crate::World::default();
    assert!(track::call(
        &s,
        1 << 24,
        &native("getpositionx", player, vec![], "float"),
        &mut off
    )
    .is_none());
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::WORLD_TRIGGER_BOX,
        rules::WORLD_TRIGGER_SPHERE,
        rules::WORLD_TRIGGER_POINT,
        rules::WORLD_TRIGGER_BODY,
        rules::WORLD_TRIGGER_INSTANT,
        rules::WORLD_TRIGGER_ENABLED,
        rules::WORLD_TRIGGER_ORDER,
        rules::WORLD_DISTANCE_EVENTS,
        rules::WORLD_DISTANCE_ONCE,
        rules::WORLD_PLACED_POSITION,
        rules::WORLD_POSITION_NATIVES,
        rules::WORLD_CELL_NATIVES,
        rules::WORLD_LOCATION_NATIVE,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}

#[test]
fn a_trigger_whose_asked_stage_was_set_since_is_named_for_another_visit() {
    use crate::quest::Change;
    use crate::track::asked_not_done;
    let e = |native: &str, quest: Option<usize>, arg: &str, result: &str| crate::Effect {
        depth: 1,
        caller: "s.f".into(),
        native: native.into(),
        receiver: None,
        receiver_object: quest,
        args: vec![arg.into()],
        result: result.into(),
    };
    // The handler asked whether stages 30 and 20 of quest object 7 were
    // done: 30 no, 20 no; 10 yes (not kept); an ask on no quest object is
    // not kept.
    let effects = vec![
        e("quest.isstagedone", Some(7), "30", "false"),
        e("quest.isstagedone", Some(7), "20", "false"),
        e("quest.getstagedone", Some(7), "10", "true"),
        e("quest.isstagedone", None, "40", "false"),
        e("objectreference.delete", Some(7), "1", "none"),
    ];
    let asked = asked_not_done(&effects, 2);
    assert_eq!(
        asked
            .iter()
            .map(|a| (a.quest, a.stage, a.log_at))
            .collect::<Vec<_>>(),
        vec![(7, 30, 2), (7, 20, 2)]
    );
    let mut p = Place::default();
    p.volumes.push(Volume {
        key: key(0x810),
        space: key(WRLD),
        frame: Transform::IDENTITY,
        shape: track::Shape::Box([1.0; 3]),
    });
    p.asked.insert(0, asked);
    // Nothing set since: no volume. Stage 20 set before the ask (index 1)
    // does not count; set after it, the volume is named.
    let mut log = vec![(7, Change::StageSet(5)), (7, Change::StageSet(20))];
    assert!(p.asking_again(&log).is_empty());
    log.push((8, Change::StageSet(20)));
    assert!(p.asking_again(&log).is_empty());
    log.push((7, Change::StageSet(20)));
    assert_eq!(p.asking_again(&log), vec![0]);
    // Held now, or deleted: not named.
    p.held.insert(0);
    assert!(p.asking_again(&log).is_empty());
    p.held.clear();
    p.deleted.insert(key(0x810));
    assert!(p.asking_again(&log).is_empty());
}
