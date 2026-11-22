//! Load events, `MoveTo` to a settled target and condition function 131
//! from the scene clock, over synthetic plugins (no game data).
use super::quest::{group, record, sub, Owned};
use super::world::{all_on, key, store, MARK, OFF, TRIG, WALK};
use super::{assemble, base};
use crate::{
    bind::RecordScripts,
    cond::{eval_term, Facts, Truth},
    data::record_object,
    load,
    player::Action,
    quest::QuestHost,
    rules,
    track::{self, Place, Spot, Switches, Track},
    NativeCall, Val, Vm,
};
use fo4_core::{Budget, Limits, ReadAt};
use fo4_plugin::{actor::Context, condition::Ctda, typed::Store, FormKey};
use std::collections::BTreeSet;

fn moveto(receiver: usize, target: usize, offset: f32) -> NativeCall {
    NativeCall {
        script: "objectreference".into(),
        name: "moveto".into(),
        receiver: Some(Val::Object(receiver)),
        instance: None,
        args: vec![
            Val::Object(target),
            Val::Float(offset),
            Val::Float(0.0),
            Val::Float(0.0),
            Val::Bool(true),
        ],
        param_types: Vec::new(),
        return_type: "none".into(),
    }
}

/// Where the trigger reference stands after a `MoveTo` to the marker.
fn after_moveto(sw: Switches, offset: f32) -> Spot {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = base();
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.world.place = Place::load(&s, Track::parse(WALK).unwrap(), sw, &b).unwrap();
    let trig = record_object(&mut vm.world, &s, &key(TRIG), "objectreference", &b).unwrap();
    let mark = record_object(&mut vm.world, &s, &key(MARK), "objectreference", &b).unwrap();
    track::observe(&s, 1 << 24, &moveto(trig, mark, offset), &mut vm.world);
    track::spot(&s, 1 << 24, &mut vm.world, Some(&Val::Object(trig)))
}

#[test]
fn moveto_with_zero_offsets_puts_the_reference_at_its_target_only_under_its_rule() {
    let on = Switches {
        moveto_target: true,
        ..all_on()
    };
    match after_moveto(on, 0.0) {
        Spot::At { pos, .. } => assert_eq!(pos, [3000.0, 1000.0, 0.0]),
        Spot::Open(why) => panic!("open: {why}"),
    }
    assert!(matches!(after_moveto(all_on(), 0.0), Spot::Open(_)));
    assert!(
        matches!(after_moveto(on, 5.0), Spot::Open(_)),
        "an offset is not read"
    );
}

/// Walks the track with the load switches given; returns how many times
/// the trigger's `OnLoad` ran, the references loaded and the counts.
fn walk_loads(sw: load::Switches) -> (Val, BTreeSet<FormKey>, Vec<(String, u64)>) {
    let s = store();
    let b = Budget::new(1 << 24);
    let mut l = base();
    l.add(assemble(
        "script Trig extends ObjectReference
         var Int loads 0
         func OnLoad none
         1 loads loads 1",
    ));
    let mut host = QuestHost::new(&s, 1 << 24);
    let mut attach = RecordScripts::new(&s, 1 << 24);
    let mut vm = Vm::new(&mut l, &mut host);
    vm.attach = Some(&mut attach);
    vm.world.place = Place::load(&s, Track::parse(WALK).unwrap(), all_on(), &b).unwrap();
    vm.world.loads.sw = sw;
    for _ in 0..16 {
        let _ = load::settle(&mut vm, &s, 64, &b);
        let Some(t) = load::next_due(&s, &vm.world) else {
            break;
        };
        let now = vm.world.now[crate::world::Clock::Real as usize];
        if t <= now {
            break;
        }
        vm.world.now[crate::world::Clock::Real as usize] = t;
    }
    let o = vm
        .world
        .objects
        .iter()
        .position(|x| x.key.as_deref() == Some("test.esm:00000810"));
    let loads = o
        .and_then(|o| vm.world.objects[o].instances.first().copied())
        .map(|i| {
            vm.world.instances[i]
                .vars
                .iter()
                .find(|v| v.name == "loads")
                .unwrap()
                .value
                .clone()
        })
        .unwrap_or(Val::None);
    let counts = vm
        .world
        .loads
        .counts
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    (loads, vm.world.loads.loaded.clone(), counts)
}

#[test]
fn the_grid_reading_loads_an_enabled_reference_once_and_leaves_a_disabled_one() {
    let (loads, loaded, counts) = walk_loads(load::Switches {
        grid: Some(0),
        ..load::Switches::default()
    });
    assert_eq!(loads, Val::Int(1));
    assert!(loaded.contains(&key(TRIG)));
    assert!(!loaded.contains(&key(OFF)));
    assert!(counts
        .iter()
        .any(|(k, v)| k == "references not loaded\tdisabled in the run" && *v == 1));
    // Off: nothing loads.
    let (loads, loaded, _) = walk_loads(load::Switches::default());
    assert_eq!(loads, Val::None);
    assert!(loaded.is_empty());
    // The interior readings do not load anything in a worldspace.
    let (_, loaded, _) = walk_loads(load::Switches {
        enters: true,
        arrival: true,
        grid: None,
    });
    assert!(loaded.is_empty());
}

#[test]
fn the_load_action_is_assumable_but_not_a_player_action() {
    assert_eq!(Action::from_switch("--assume-load"), Some(Action::Load));
    assert_eq!(Action::Load.handler(), "onload");
    assert!(!Action::ALL.contains(&Action::Load));
}

/// A plugin holding one scene record.
fn scene_store() -> Store {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(1u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, &sub(b"HEDR", &hedr));
    plugin[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
    plugin.extend(group(
        0,
        *b"SCEN",
        &record(b"SCEN", 0x900, &sub(b"EDID", b"S\0")),
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

struct F<'s> {
    store: &'s Store,
    on: bool,
    percent: f32,
    asked: Vec<(FormKey, u32)>,
}
impl Facts for F<'_> {
    fn store(&self) -> &Store {
        self.store
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
    fn action_percent_on(&self) -> bool {
        self.on
    }
    fn action_percent(&mut self, scene: &FormKey, action: u32) -> Result<f32, &'static str> {
        self.asked.push((scene.clone(), action));
        Ok(self.percent)
    }
}

#[test]
fn function_131_reads_the_scene_clock_only_with_its_switch() {
    let s = scene_store();
    let ctx = Context {
        order: s.order(),
        plugin: 0,
    };
    // At least 80 percent of action 14 of the scene.
    let c = Ctda {
        byte0: 3 << 5,
        unused: [0; 3],
        value: 80.0f32.to_bits(),
        function: 131,
        pad: [0; 2],
        param1: 0x900,
        param2: 14,
        run_on: 0,
        reference: 0,
        param3: -1,
    };
    let eval = |on: bool, percent: f32| {
        let mut f = F {
            store: &s,
            on,
            percent,
            asked: Vec::new(),
        };
        let mut used = BTreeSet::new();
        let t = eval_term(&c, ctx, "test.esm", &mut f, &mut used);
        (t, f.asked, used)
    };
    let (t, asked, used) = eval(false, 100.0);
    assert!(matches!(t, Truth::Undetermined(_)));
    assert!(asked.is_empty());
    assert!(!used.contains(rules::COND_FN_131));
    let (t, asked, used) = eval(true, 85.0);
    assert_eq!(t, Truth::True);
    assert_eq!(asked, vec![(key(0x900), 14)]);
    assert!(used.contains(rules::COND_FN_131));
    assert_eq!(eval(true, 50.0).0, Truth::False);
}

#[test]
fn every_rule_of_the_step_is_registered_pending() {
    for r in [
        rules::WORLD_MOVETO_TARGET,
        rules::WORLD_LOAD_ENTERS,
        rules::WORLD_LOAD_ARRIVES,
        rules::WORLD_LOAD_GRID,
        rules::COND_FN_131,
        rules::SCENE_CONDITIONS_END,
    ] {
        assert_eq!(rules::status(r), Some("pending"), "{r}");
        assert!(rules::ALL.contains(&r), "{r}");
    }
}
