//! Scenes (`spec/vm.md`, Scenes; `spec/scene-record.md`): the state the
//! quest host keeps per scene, the natives it answers from that state
//! (`Scene.Start`, `ForceStart`, `Stop`, `IsPlaying`, `GetOwningQuest`),
//! and the progression of a scene's phases where the data decides it
//! without the world. Every behaviour here is a pending rule
//! (`rules::SCENE_*`); nothing is measured in the running game.
//!
//! A scene that starts runs its begin fragment, then offers its phases in
//! stored order (`scene_phase_progression_v1`): a phase whose start
//! conditions are false is passed over; one whose start conditions hold is
//! entered and runs its begin fragment; it completes at once only when no
//! action of the scene runs in it and its completion conditions are absent
//! or hold, and then runs its end fragment. Everything else (an action
//! that speaks a line, runs a package, waits on a timer, plays the radio,
//! waits on the player, starts another scene; completion conditions that
//! are false; conditions the evaluator leaves undetermined) stops the
//! scene where it is, counted by reason. After the last phase the scene's
//! end fragment runs and it stops playing.
use crate::{
    bind,
    cond::{self, Combine, Truth},
    data::{key_record, record_object},
    host::{NativeCall, Reply},
    quest::{self, Job, QuestHost, RunFacts},
    rules,
    world::{InstId, ObjId, Val, World},
    VmError,
};
use fo4_core::Budget;
use fo4_plugin::{
    actor::Context, condition::Condition, scen::Scene, typed::Store, vmad::Vmad, FormKey,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

/// FNAM bit read as starting the scene when its quest starts
/// (`scene_begin_on_quest_start_v1`).
pub const BEGIN_ON_QUEST_START: u32 = 0x0001;
/// The phase fragment flag byte read as a phase's begin fragment
/// (`vmad_fragment_blocks_v1`).
pub const PHASE_BEGIN: u8 = 1;
/// The phase fragment flag byte read as a phase's end fragment.
pub const PHASE_END: u8 = 2;

/// Natives the quest host answers when scenes are on, lower case.
pub const NATIVES: [&str; 6] = [
    "scene.start",
    "scene.forcestart",
    "scene.stop",
    "scene.isplaying",
    "scene.getowningquest",
    "topicinfo.getowningquest",
];

/// The synthetic call a [`Step::Enter`] or [`Step::Complete`] job makes on
/// the host (no script calls it).
pub const STEP: &str = "::step";

/// What an action kind waits on, as the census reads the kinds (the field
/// each kind stores names a DIAL, PACK or SCEN: `spec/fragment-census.txt`).
pub fn action_kind(kind: u16) -> &'static str {
    match kind {
        0 => "kind 0 (a dialogue line: audio)",
        1 => "kind 1 (a package: actor movement)",
        2 => "kind 2 (a timer: the clock)",
        3 => "kind 3 (player dialogue: the player)",
        4 => "kind 4 (starts another scene)",
        5 => "kind 5 (a response line: audio)",
        6 => "kind 6 (radio: audio)",
        _ => "another kind",
    }
}

/// One phase, as the host needs it.
#[derive(Debug, Clone)]
pub struct PhaseInfo {
    pub start: Vec<Condition>,
    pub completion: Vec<Condition>,
    /// Lower-case function names of the phase's begin and end fragments.
    pub begin: Option<String>,
    pub end: Option<String>,
    /// Kinds of the actions whose start and end phases include this one.
    pub actions: Vec<u16>,
}

/// What the records say about one scene.
#[derive(Debug, Clone)]
pub struct SceneInfo {
    pub key: FormKey,
    pub plugin: usize,
    pub file: String,
    pub flags: u32,
    pub quest: Option<FormKey>,
    pub begin: Option<String>,
    pub end: Option<String>,
    pub phases: Vec<PhaseInfo>,
    /// Phase fragments whose flag byte is neither begin nor end, or whose
    /// phase the record does not hold (not run, counted).
    pub unplaced: usize,
}
impl SceneInfo {
    /// Reads a scene record and its VMAD block; `None` when the key names
    /// no winning SCEN.
    pub fn load(store: &Store, k: &FormKey, b: &Budget) -> fo4_core::Result<Option<Self>> {
        if store.signature(k) != Some(*b"SCEN") {
            return Ok(None);
        }
        let Some((r, plugin)) = store.raw_with_plugin(k, b)? else {
            return Ok(None);
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let s = Scene::read(&r, ctx, b)?;
        let mut phases: Vec<PhaseInfo> = s
            .phases
            .iter()
            .enumerate()
            .map(|(i, p)| PhaseInfo {
                start: p.start.clone(),
                completion: p.completion.clone(),
                begin: None,
                end: None,
                actions: s
                    .actions
                    .iter()
                    .filter(|a| (a.start as usize) <= i && i <= a.end as usize)
                    .map(|a| a.kind)
                    .collect(),
            })
            .collect();
        let (mut begin, mut end, mut unplaced) = (None, None, 0);
        if let Some(i) = s.vmad {
            let sub = &r.subrecords[i];
            let v = Vmad::read(r.data(sub), r.signature, &r.source.file, sub.offset, b)?;
            if let Some(block) = &v.scene {
                let lower = |f: &fo4_plugin::vmad::Fragment<'_>| fo4_pex::index::lower(f.function);
                begin = block
                    .block
                    .slot(fo4_plugin::vmad::FRAGMENT_BEGIN)
                    .map(lower);
                end = block.block.slot(fo4_plugin::vmad::FRAGMENT_END).map(lower);
                for p in &block.phases {
                    let slot = phases.get_mut(p.phase as usize);
                    match (slot, p.flags) {
                        (Some(x), PHASE_BEGIN) => x.begin = Some(lower(&p.fragment)),
                        (Some(x), PHASE_END) => x.end = Some(lower(&p.fragment)),
                        _ => unplaced += 1,
                    }
                }
            }
        }
        Ok(Some(SceneInfo {
            key: k.clone(),
            plugin,
            file: r.source.file.clone(),
            flags: s.flags,
            quest: s.quest.key.clone(),
            begin,
            end,
            phases,
            unplaced,
        }))
    }
}

/// The state of one scene.
#[derive(Debug, Clone, Default)]
pub struct SceneState {
    /// Whether it plays; `None` when nothing settles it.
    pub playing: Option<bool>,
    /// The phase the next step offers.
    pub next: usize,
}

/// A step of a playing scene, run as a [`Job::Scene`].
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// A fragment function on the scene's fragment script instance, with
    /// what it is (`begin`, `end`, `phase N begin`, `phase N end`).
    Fragment(String, String),
    /// Offer the next phase: its start conditions decide whether it is
    /// entered or passed over.
    Enter,
    /// Decide whether the entered phase completes without the world.
    Complete,
}

/// One change of scene state, as logged.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Started,
    /// Started because its quest started (`scene_begin_on_quest_start_v1`).
    StartedWithQuest,
    PhaseEntered(usize),
    PhaseSkipped(usize),
    PhaseCompleted(usize),
    /// Stopped where it is, with the reason (waiting on the world, or an
    /// undetermined condition).
    Waiting(usize, String),
    Ended,
    Stopped,
    Refused(String),
    Opened(String),
}

/// Every scene's state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Scenes {
    /// Answer the scene natives and run scenes (`quest_run --no-scenes`
    /// leaves them to the data host, recorded and undetermined, as before).
    pub on: bool,
    /// Start a quest's scenes with FNAM bit 0x1 when the quest starts.
    pub begin_on_quest_start: bool,
    pub states: BTreeMap<String, SceneState>,
    pub infos: BTreeMap<String, Option<Rc<SceneInfo>>>,
    /// The fragment script instance of each scene handle.
    pub fragment_instances: BTreeMap<ObjId, InstId>,
    /// Winning scenes by owning quest key, and the owning quest of every
    /// winning INFO, from the groups of kind 10 that hold them; read once
    /// when needed.
    /// A caller running many interpreters may build it once
    /// ([`Groups::read`]) and share it.
    pub groups: Option<Rc<Groups>>,
    /// (scene handle, change) in order.
    pub log: Vec<(ObjId, Change)>,
}

/// What the groups of kind 10 hold: scenes by quest, INFO owners.
#[derive(Debug, Default)]
pub struct Groups {
    pub scenes: BTreeMap<String, Vec<FormKey>>,
    pub info_owner: BTreeMap<String, FormKey>,
}

impl Groups {
    /// Reads the groups of kind 10 of the store.
    pub fn read(store: &Store) -> Groups {
        let mut g = Groups::default();
        for (o, cell, kind) in store.placement_groups() {
            if kind != 10 || o.deleted {
                continue;
            }
            let k = store.overrides().form_key(o.key, store.order());
            match &o.signature {
                b"SCEN" => g.scenes.entry(bind::record_key(&cell)).or_default().push(k),
                b"INFO" => {
                    g.info_owner.insert(bind::record_key(&k), cell);
                }
                _ => {}
            }
        }
        g
    }
}

fn groups(store: &Store, w: &mut World) -> Rc<Groups> {
    w.scenes
        .groups
        .get_or_insert_with(|| Rc::new(Groups::read(store)))
        .clone()
}

/// The record behind a scene handle, cached in the world.
pub fn info(store: &Store, max_memory: u64, w: &mut World, o: ObjId) -> Option<Rc<SceneInfo>> {
    let key = w.objects.get(o)?.key.clone()?;
    if let Some(i) = w.scenes.infos.get(&key) {
        return i.clone();
    }
    let k = key_record(&key)?;
    let b = Budget::new(max_memory);
    let i = SceneInfo::load(store, &k, &b).ok().flatten().map(Rc::new);
    w.scenes.infos.insert(key, i.clone());
    i
}

fn state<'w>(w: &'w mut World, i: &SceneInfo) -> &'w mut SceneState {
    let k = bind::record_key(&i.key);
    let new_game = w.quests.scenario == quest::Scenario::NewGame;
    w.scenes.states.entry(k).or_insert_with(|| SceneState {
        // At the start of a new game no scene plays; in the isolated
        // scenario nothing is known (`scene_start_v1`).
        playing: new_game.then_some(false),
        next: 0,
    })
}

/// Queues a step of a scene at the depth of the job running now plus one.
fn queue(w: &mut World, scene: ObjId, step: Step) {
    let depth = w.quests.depth + 1;
    w.quests.queue.push_back(Job::Scene { scene, step, depth });
}

/// Whether the scene's owning quest runs, from the quest state.
fn quest_running(h: &QuestHost<'_>, w: &mut World, i: &SceneInfo) -> Option<bool> {
    let q = i.quest.as_ref()?;
    let qi = quest::info_by_key(h.data.store, h.data.max_memory, w, q)?;
    let s = w.quests.state(&qi);
    if s.open.is_some() {
        return None;
    }
    s.running
}

/// Starts a scene that is not playing (`scene_start_v1`): playing, begin
/// fragment, then the first phase. Returns why it did not start.
pub(crate) fn start(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &Rc<SceneInfo>,
    with_quest: bool,
) -> Result<(), &'static str> {
    h.using(rules::SCENE_START);
    match state(w, i).playing {
        Some(true) => return Err("Start of a scene that is playing"),
        None => {
            w.scenes.log.push((
                o,
                Change::Opened("Start of a scene whose state is undetermined".into()),
            ));
            return Err("whether the scene is playing is undetermined");
        }
        Some(false) => {}
    }
    match quest_running(h, w, i) {
        Some(true) => {}
        Some(false) => {
            w.scenes
                .log
                .push((o, Change::Refused("its quest is not running".into())));
            return Err("the scene's quest is not running");
        }
        None => {
            state(w, i).playing = None;
            w.scenes.log.push((
                o,
                Change::Opened("whether its quest runs is undetermined".into()),
            ));
            return Err("whether the scene's quest runs is undetermined");
        }
    }
    let s = state(w, i);
    s.playing = Some(true);
    s.next = 0;
    w.scenes.log.push((
        o,
        if with_quest {
            Change::StartedWithQuest
        } else {
            Change::Started
        },
    ));
    h.using(rules::SCENE_PHASES);
    if let Some(f) = &i.begin {
        queue(w, o, Step::Fragment(f.clone(), "begin".into()));
    }
    queue(w, o, Step::Enter);
    Ok(())
}

/// The scenes of a quest with FNAM bit 0x1, started after the quest
/// starts (`scene_begin_on_quest_start_v1`), in the store's order.
pub(crate) fn quest_started(h: &mut QuestHost<'_>, w: &mut World, quest: &FormKey) {
    if !(w.scenes.on && w.scenes.begin_on_quest_start) {
        return;
    }
    let keys = groups(h.data.store, w)
        .scenes
        .get(&bind::record_key(quest))
        .cloned()
        .unwrap_or_default();
    for k in keys {
        let b = Budget::new(h.data.max_memory);
        let Ok(o) = record_object(w, h.data.store, &k, "scene", &b) else {
            continue;
        };
        let Some(i) = info(h.data.store, h.data.max_memory, w, o) else {
            continue;
        };
        if i.flags & BEGIN_ON_QUEST_START == 0 {
            continue;
        }
        h.using(rules::SCENE_BEGIN_ON_QUEST_START);
        let _ = start(h, w, o, &i, true);
    }
}

/// Evaluates a condition list of a scene in the run's state.
fn eval(h: &mut QuestHost<'_>, w: &mut World, i: &SceneInfo, list: &[Condition]) -> Truth {
    if list.is_empty() {
        return Truth::True;
    }
    let ctx = Context {
        order: h.data.store.order(),
        plugin: i.plugin,
    };
    let mut used = BTreeSet::new();
    let mut facts = RunFacts::of(h, w, None, i.quest.clone());
    let t = cond::eval_list_with(list, ctx, &i.file, &mut facts, Combine::OrGroups, &mut used);
    for u in used {
        h.using(u);
    }
    t
}

fn functions(why: &[cond::Why]) -> String {
    let f: BTreeSet<u16> = why.iter().map(|x| x.function).collect();
    f.iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One step of a playing scene ([`Step::Enter`] or [`Step::Complete`]),
/// asked by the job through the host.
fn step(h: &mut QuestHost<'_>, w: &mut World, o: ObjId, i: &Rc<SceneInfo>, complete: bool) {
    h.using(rules::SCENE_PHASES);
    let s = state(w, i);
    if s.playing != Some(true) {
        return;
    }
    let p = s.next;
    let Some(phase) = i.phases.get(p).cloned() else {
        // Past the last phase: the end fragment, then not playing.
        if let Some(f) = &i.end {
            queue(w, o, Step::Fragment(f.clone(), "end".into()));
        }
        state(w, i).playing = Some(false);
        w.scenes.log.push((o, Change::Ended));
        return;
    };
    let wait = |w: &mut World, why: String| {
        w.scenes.log.push((o, Change::Waiting(p, why)));
    };
    if !complete {
        match eval(h, w, i, &phase.start) {
            Truth::False => {
                state(w, i).next = p + 1;
                w.scenes.log.push((o, Change::PhaseSkipped(p)));
                queue(w, o, Step::Enter);
            }
            Truth::Undetermined(why) => wait(
                w,
                format!(
                    "start conditions undetermined (function {})",
                    functions(&why)
                ),
            ),
            Truth::True => {
                w.scenes.log.push((o, Change::PhaseEntered(p)));
                if let Some(f) = &phase.begin {
                    queue(w, o, Step::Fragment(f.clone(), format!("phase {p} begin")));
                }
                queue(w, o, Step::Complete);
            }
        }
        return;
    }
    if let Some(k) = phase.actions.iter().min() {
        wait(
            w,
            format!("an action runs in the phase: {}", action_kind(*k)),
        );
        return;
    }
    match eval(h, w, i, &phase.completion) {
        Truth::True => {
            state(w, i).next = p + 1;
            w.scenes.log.push((o, Change::PhaseCompleted(p)));
            if let Some(f) = &phase.end {
                queue(w, o, Step::Fragment(f.clone(), format!("phase {p} end")));
            }
            queue(w, o, Step::Enter);
        }
        Truth::False => wait(w, "completion conditions false (waits on the world)".into()),
        Truth::Undetermined(why) => wait(
            w,
            format!(
                "completion conditions undetermined (function {})",
                functions(&why)
            ),
        ),
    }
}

/// The scene natives (and the steps), answered by the quest host when
/// scenes are on.
pub(crate) fn call(h: &mut QuestHost<'_>, c: &NativeCall, w: &mut World) -> Reply {
    let full = c.full_name();
    let none = |c: &NativeCall| {
        if c.return_type == "none" {
            Reply::Value(Val::None)
        } else {
            Reply::Undetermined
        }
    };
    if full == "topicinfo.getowningquest" {
        h.using(rules::OWNING_QUEST);
        let Some(Val::Object(o)) = &c.receiver else {
            return Reply::Undetermined;
        };
        let Some(k) = w.objects[*o].key.clone() else {
            return Reply::Undetermined;
        };
        let store = h.data.store;
        let q = groups(store, w).info_owner.get(&k).cloned();
        let b = Budget::new(h.data.max_memory);
        return match q.and_then(|q| record_object(w, store, &q, "quest", &b).ok()) {
            Some(q) => Reply::Value(Val::Object(q)),
            None => Reply::Undetermined,
        };
    }
    let Some(Val::Object(o)) = &c.receiver else {
        return none(c);
    };
    let o = *o;
    let Some(i) = info(h.data.store, h.data.max_memory, w, o) else {
        return none(c);
    };
    match (c.script.as_str(), c.name.as_str()) {
        ("scene", "start" | "forcestart") => {
            let _ = start(h, w, o, &i, false);
            none(c)
        }
        ("scene", "stop") => {
            h.using(rules::SCENE_START);
            let s = state(w, &i);
            s.playing = Some(false);
            w.scenes.log.push((o, Change::Stopped));
            none(c)
        }
        ("scene", "isplaying") => {
            h.using(rules::SCENE_START);
            match state(w, &i).playing {
                Some(p) => Reply::Value(Val::Bool(p)),
                None => Reply::Undetermined,
            }
        }
        ("scene", "getowningquest") => {
            h.using(rules::OWNING_QUEST);
            let b = Budget::new(h.data.max_memory);
            match i
                .quest
                .as_ref()
                .and_then(|q| record_object(w, h.data.store, q, "quest", &b).ok())
            {
                Some(q) => Reply::Value(Val::Object(q)),
                None => Reply::Undetermined,
            }
        }
        ("scene", n) if n == STEP => {
            let complete = matches!(c.args.first(), Some(Val::Bool(true)));
            step(h, w, o, &i, complete);
            Reply::Value(Val::None)
        }
        _ => none(c),
    }
}

/// The fragment script instance on a scene's handle
/// (`vmad_fragment_instance_v1`): created when first needed, with the
/// block's property values.
pub fn fragment_instance(
    vm: &mut crate::Vm<'_>,
    store: &Store,
    o: ObjId,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<InstId, VmError> {
    if let Some(i) = vm.world.scenes.fragment_instances.get(&o) {
        return Ok(*i);
    }
    let inst = record_fragment_instance(vm, store, o, notes, b)?;
    vm.world.scenes.fragment_instances.insert(o, inst);
    Ok(inst)
}

/// An instance of the fragment block script of the INFO, PACK, SCEN or
/// PERK record behind a handle, with the block's property values
/// (`vmad_fragment_instance_v1`).
pub fn record_fragment_instance(
    vm: &mut crate::Vm<'_>,
    store: &Store,
    o: ObjId,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<InstId, VmError> {
    let bad = |e: &dyn std::fmt::Display| VmError::BadArgument {
        site: None,
        detail: format!("reading the record's VMAD: {e}"),
    };
    let key = vm.world.objects[o]
        .key
        .clone()
        .and_then(|k| key_record(&k))
        .ok_or_else(|| bad(&"not a record"))?;
    let (r, plugin) = bind::vmad_of(store, &key, b)
        .map_err(|e| bad(&e))?
        .ok_or_else(|| bad(&"no VMAD"))?;
    let s = r
        .subrecords
        .iter()
        .find(|s| s.signature == *b"VMAD")
        .ok_or_else(|| bad(&"no VMAD"))?;
    let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b).map_err(|e| bad(&e))?;
    let script = v
        .info
        .as_ref()
        .or(v.package.as_ref())
        .or(v.scene.as_ref().map(|x| &x.block))
        .map(|x| &x.script)
        .or(v.perk.as_ref().map(|x| &x.script))
        .ok_or_else(|| bad(&"no fragment block"))?;
    let name = fo4_pex::index::lower(script.name);
    vm.rules.insert(rules::FRAGMENT_INSTANCE);
    let existing = vm.world.objects[o]
        .instances
        .iter()
        .copied()
        .find(|i| vm.world.instances[*i].script == name);
    let inst = match existing {
        Some(i) => i,
        None => vm.instantiate(Some(o), &name)?,
    };
    let cx = bind::Ctx {
        store,
        plugin,
        file: r.source.file.clone(),
        b,
    };
    bind::apply(vm, &cx, inst, &name, &script.properties, notes).map_err(|e| bad(&e))?;
    Ok(inst)
}

/// The synthetic call of a step job.
pub fn step_call(scene: ObjId, complete: bool) -> NativeCall {
    NativeCall {
        script: "scene".into(),
        name: STEP.into(),
        receiver: Some(Val::Object(scene)),
        instance: None,
        args: vec![Val::Bool(complete)],
        param_types: vec!["bool".into()],
        return_type: "none".into(),
    }
}
