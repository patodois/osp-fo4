//! Quests (`spec/vm.md`, Quests): the state the host keeps per quest
//! (running, current stage, stages done, objectives, alias fills set by a
//! run), the quest host that answers the quest natives from that state and
//! the records, and the driver that runs the stage fragments and quest
//! events the natives queue. Every behaviour here is a pending rule
//! (`rules::QUEST_*`); nothing is measured in the running game.
//!
//! A state item nothing settles is undetermined (`None`), and a native
//! whose answer depends on it answers undetermined: the run blocks where
//! it would steer control flow. A native whose effect on the state is not
//! settled (a stage the record does not hold, an undetermined stage, a
//! reset) makes the quest's whole state undetermined (`open`).
use crate::{
    bind,
    cond::{self, Combine, Facts, Truth},
    data::{self, key_record, record_object, DataHost},
    host::{Host, NativeCall, Reply},
    rules,
    world::{InstId, ObjId, Val, World},
    Outcome, Vm, VmError,
};
use fo4_core::Budget;
use fo4_plugin::{
    actor::Context, condition::Condition, quest::Quest, typed::Store, vmad::Vmad, FormKey,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
};

/// DNAM flag bit read as start-game enabled (`quest_start_game_enabled_v1`).
pub const START_GAME_ENABLED: u16 = 0x0001;
/// DNAM flag bit read as allowing a stage to be set again
/// (`quest_repeated_stage_v1`).
pub const ALLOW_REPEATED_STAGES: u16 = 0x0008;
/// INDX flag bit read as a startup stage (`quest_start_v1`).
pub const STARTUP_STAGE: u8 = 0x02;

/// One log entry of a stage, as the host needs it.
#[derive(Debug, Clone)]
pub struct EntryInfo {
    /// Whether the entry has conditions (CTDA).
    pub conditions: bool,
    /// The entry's conditions, evaluated by [`cond`] when the stage is set
    /// (`quest_stage_log_entry_v1`).
    pub list: Vec<Condition>,
    /// The fragment function the VMAD block names for this (stage, entry)
    /// (`quest_fragment_entry_v1`), lower case.
    pub fragment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StageInfo {
    pub stage: u16,
    pub flags: u8,
    pub entries: Vec<EntryInfo>,
}

/// What the records say about one quest.
#[derive(Debug, Clone)]
pub struct QuestInfo {
    pub key: FormKey,
    /// The load-order index of the plugin the record was read from, for
    /// resolving the FormIDs of its conditions.
    pub plugin: usize,
    /// The file the record was read from (for error texts).
    pub file: String,
    /// DNAM flags, when the record has DNAM.
    pub flags: Option<u16>,
    pub stages: Vec<StageInfo>,
    pub objectives: BTreeSet<u16>,
    /// Reference aliases (ALST) by id, with their ALFR when they have one.
    pub reference_aliases: BTreeMap<u32, Option<FormKey>>,
    /// The VMAD block's fragment script, lower case.
    pub fragment_script: Option<String>,
    /// Fragments naming a stage the record does not hold.
    pub unmapped_fragments: usize,
    /// ENAM: the story event code, as stored.
    pub event: Option<[u8; 4]>,
    /// The conditions after NEXT, which a story manager start evaluates
    /// (`sm_quest_node_start_v1`).
    pub event_conditions: Vec<Condition>,
}
impl QuestInfo {
    pub fn stage(&self, n: i32) -> Option<&StageInfo> {
        self.stages.iter().find(|s| i32::from(s.stage) == n)
    }
    pub fn start_game_enabled(&self) -> bool {
        self.flags.is_some_and(|f| f & START_GAME_ENABLED != 0)
    }
    /// Reads a quest's record and its VMAD block; `None` when the key
    /// names no winning QUST.
    pub fn load(store: &Store, k: &FormKey, b: &Budget) -> fo4_core::Result<Option<Self>> {
        if store.signature(k) != Some(*b"QUST") {
            return Ok(None);
        }
        let Some((r, plugin)) = store.raw_with_plugin(k, b)? else {
            return Ok(None);
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let q = Quest::read(&r, ctx, b)?;
        let mut fragments: BTreeMap<(u16, i32), String> = BTreeMap::new();
        let mut fragment_script = None;
        if let Some(i) = q.vmad {
            let s = &r.subrecords[i];
            let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)?;
            if let Some(block) = &v.quest {
                if !block.script.name.is_empty() {
                    fragment_script = Some(fo4_pex::index::lower(block.script.name));
                }
                for f in &block.fragments {
                    fragments.insert((f.stage, f.log_entry), fo4_pex::index::lower(f.function));
                }
            }
        }
        let mut stages = Vec::new();
        let mut mapped = 0;
        for s in &q.stages {
            let mut entries = Vec::new();
            for (i, e) in s.entries.iter().enumerate() {
                let fragment = fragments.get(&(s.stage, i as i32)).cloned();
                mapped += usize::from(fragment.is_some());
                entries.push(EntryInfo {
                    conditions: !e.conditions.is_empty(),
                    list: e.conditions.iter().map(Condition::from).collect(),
                    fragment,
                });
            }
            stages.push(StageInfo {
                stage: s.stage,
                flags: s.flags,
                entries,
            });
        }
        let reference_aliases = q
            .aliases
            .iter()
            .filter(|a| a.kind != fo4_plugin::quest::AliasKind::Location)
            .map(|a| (a.id, a.forced_reference().and_then(|f| f.key.clone())))
            .collect();
        Ok(Some(QuestInfo {
            key: k.clone(),
            plugin,
            file: r.source.file.clone(),
            flags: q.general.map(|g| g.flags),
            stages,
            objectives: q.objectives.iter().map(|o| o.objective).collect(),
            reference_aliases,
            fragment_script,
            unmapped_fragments: fragments.len() - mapped,
            event: q.event,
            event_conditions: q.event_conditions.iter().map(Condition::from).collect(),
        }))
    }
}

/// Which state quests start from when a run first reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scenario {
    /// Every state item is undetermined until a run sets it
    /// (`quest_state_isolated_v1`): a stage set in the middle of a game
    /// whose history is unknown.
    #[default]
    Isolated,
    /// The start of a new game (`quest_start_game_enabled_v1`): a quest is
    /// running exactly when its DNAM flags carry the start-game bit, no
    /// stage is done and no objective set; the current stage stays
    /// undetermined until a stage is set.
    NewGame,
}

/// Objective state kinds.
pub const DISPLAYED: u8 = 0;
pub const COMPLETED: u8 = 1;
pub const FAILED: u8 = 2;

/// The state of one quest.
#[derive(Debug, Clone, Default)]
pub struct QuestState {
    pub running: Option<bool>,
    pub current: Option<i32>,
    /// Stages whose done-ness a run set or the scenario fixed.
    pub done: BTreeMap<i32, bool>,
    /// Done-ness of every other stage.
    pub done_rest: Option<bool>,
    /// (kind, objective) as set.
    pub objectives: BTreeMap<(u8, i32), Option<bool>>,
    pub objectives_rest: Option<bool>,
    /// Alias fills a run set (ForceRefTo, Clear), by alias id.
    pub fills: BTreeMap<i32, Val>,
    /// Why the whole state became undetermined.
    pub open: Option<String>,
}
impl QuestState {
    pub fn is_done(&self, n: i32) -> Option<bool> {
        self.done.get(&n).copied().or(self.done_rest)
    }
}

/// A run a native brought: run at once inside the calling run by a
/// [`Runner`] (the candidate reading), or queued for [`run_jobs`].
#[derive(Debug, Clone)]
pub enum Job {
    /// A stage fragment: the function on the fragment script instance of
    /// the quest's handle.
    Fragment {
        quest: ObjId,
        stage: i32,
        entry: usize,
        function: String,
        /// Fragments between this one and the sweep's own stage.
        depth: usize,
    },
    /// An event delivered to every script instance of the quest's handle
    /// that has a function with instructions for it.
    Event {
        quest: ObjId,
        event: &'static str,
        depth: usize,
    },
    /// `OnStageSet(stage, item)` delivered to every script instance of
    /// the quest's handle that has a function with instructions for it
    /// (`quest_on_stage_set_v1`); `entry` is the log entry selected, `None`
    /// when the selection is open or no entry holds.
    StageEvent {
        quest: ObjId,
        stage: i32,
        entry: Option<usize>,
        depth: usize,
    },
    /// A run the rules leave undetermined (which fragment, whether an
    /// event runs).
    Open {
        quest: ObjId,
        why: String,
        depth: usize,
    },
    /// A custom event sent by `sender` (`vm_custom_events_v1`), delivered to
    /// the instances registered for it.
    Custom {
        sender: ObjId,
        event: String,
        args: Val,
        depth: usize,
    },
    /// `OnStoryScript` delivered to a quest the story manager started
    /// (`sm_quest_node_start_v1`), with the event's six values.
    Story {
        quest: ObjId,
        args: Vec<Val>,
        depth: usize,
    },
    /// A step of a playing scene (`crate::scene`).
    Scene {
        scene: ObjId,
        step: crate::scene::Step,
        depth: usize,
    },
}
impl Job {
    pub fn depth(&self) -> usize {
        match self {
            Job::Fragment { depth, .. }
            | Job::Event { depth, .. }
            | Job::StageEvent { depth, .. }
            | Job::Open { depth, .. }
            | Job::Custom { depth, .. }
            | Job::Story { depth, .. }
            | Job::Scene { depth, .. } => *depth,
        }
    }
    /// The quest the job runs on (for a custom event, its sender).
    pub fn quest(&self) -> ObjId {
        match self {
            Job::Fragment { quest, .. }
            | Job::Event { quest, .. }
            | Job::StageEvent { quest, .. }
            | Job::Open { quest, .. }
            | Job::Story { quest, .. } => *quest,
            Job::Custom { sender, .. } => *sender,
            Job::Scene { scene, .. } => *scene,
        }
    }
}

/// One change of quest state, as logged.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    StageSet(i32),
    StageRepeatRefused(i32),
    Started,
    /// Started by the story manager for a story event (logged after
    /// [`Change::Started`]).
    StartedByStory,
    Stopped,
    Objective(u8, i32, Option<bool>),
    Opened(String),
}

/// Every quest's state and the queued runs (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Quests {
    pub scenario: Scenario,
    /// By record key ([`bind::record_key`]).
    pub states: BTreeMap<String, QuestState>,
    pub infos: BTreeMap<String, Option<Rc<QuestInfo>>>,
    pub queue: VecDeque<Job>,
    /// (quest handle, change) in order.
    pub log: Vec<(ObjId, Change)>,
    /// The fragment script instance of each quest handle.
    pub fragment_instances: BTreeMap<ObjId, InstId>,
    /// Depth of the job running now (jobs it queues are one deeper).
    pub depth: usize,
    /// Jobs started so far.
    pub seq: u64,
    /// Jobs a [`Runner`] ran inside other runs.
    pub nested_runs: Vec<JobRun>,
}

impl Quests {
    fn initial(&self, info: &QuestInfo) -> QuestState {
        match self.scenario {
            Scenario::Isolated => QuestState::default(),
            Scenario::NewGame => QuestState {
                running: Some(info.start_game_enabled()),
                current: None,
                done: BTreeMap::new(),
                done_rest: Some(false),
                objectives: BTreeMap::new(),
                objectives_rest: Some(false),
                fills: BTreeMap::new(),
                open: None,
            },
        }
    }
    /// The state of a quest, created from the scenario when first read.
    pub fn state(&mut self, info: &QuestInfo) -> &mut QuestState {
        let k = bind::record_key(&info.key);
        if !self.states.contains_key(&k) {
            let s = self.initial(info);
            self.states.insert(k.clone(), s);
        }
        self.states.entry(k).or_default()
    }
}

/// The record behind a quest handle, cached in the world.
pub fn info(store: &Store, max_memory: u64, w: &mut World, o: ObjId) -> Option<Rc<QuestInfo>> {
    let key = w.objects.get(o)?.key.clone()?;
    if let Some(i) = w.quests.infos.get(&key) {
        return i.clone();
    }
    let k = key_record(&key)?;
    let b = Budget::new(max_memory);
    let i = QuestInfo::load(store, &k, &b).ok().flatten().map(Rc::new);
    w.quests.infos.insert(key, i.clone());
    i
}

/// The record of a quest by key, cached in the world as [`info`] caches it.
pub fn info_by_key(
    store: &Store,
    max_memory: u64,
    w: &mut World,
    k: &FormKey,
) -> Option<Rc<QuestInfo>> {
    let key = bind::record_key(k);
    if let Some(i) = w.quests.infos.get(&key) {
        return i.clone();
    }
    let b = Budget::new(max_memory);
    let i = QuestInfo::load(store, k, &b).ok().flatten().map(Rc::new);
    w.quests.infos.insert(key, i.clone());
    i
}

/// What a condition reads during a run: the quest state in the run's
/// world (created from the scenario when first read), and globals as the
/// data host answers them (`vm_host_global_fltv_v1`; undetermined once a
/// native of the run may have changed them).
pub struct RunFacts<'a, 's> {
    pub data: &'a DataHost<'s>,
    pub w: &'a mut World,
    /// The story event being offered, when conditions are evaluated for
    /// the story manager (`crate::story`).
    pub event: Option<&'a crate::story::StoryEvent>,
    /// The evaluator switches, scripts and caller assumptions.
    pub cond: &'a CondInputs<'s>,
    /// The quest whose aliases run-on 5 names.
    pub owner: Option<FormKey>,
}

/// What the quest host gives the condition evaluator besides the state:
/// which added evaluators are on, the compiled scripts for quest variables
/// not yet attached, and the values the caller assumes.
#[derive(Default)]
pub struct CondInputs<'s> {
    pub switches: cond::Switches,
    pub scripts: Option<std::cell::RefCell<Box<dyn crate::program::Library + 's>>>,
    /// Caller assumptions (`quest_run --assume-cond`): the value of every
    /// condition of a function index. Not a rule.
    pub assume: BTreeMap<u16, f32>,
    /// Conditions an assumption decided.
    pub assumed: std::cell::Cell<u64>,
}

impl<'a, 's> RunFacts<'a, 's> {
    /// The facts of a run of the quest host, for lists of `owner`.
    pub fn of(
        h: &'a QuestHost<'s>,
        w: &'a mut World,
        event: Option<&'a crate::story::StoryEvent>,
        owner: Option<FormKey>,
    ) -> Self {
        RunFacts {
            data: &h.data,
            w,
            event,
            cond: &h.cond,
            owner,
        }
    }
}
impl RunFacts<'_, '_> {
    fn state(&mut self, k: &FormKey) -> Result<&QuestState, &'static str> {
        let i = info_by_key(self.data.store, self.data.max_memory, self.w, k)
            .ok_or("the quest record could not be read")?;
        let s = self.w.quests.state(&i);
        if s.open.is_some() {
            return Err(OPEN);
        }
        Ok(s)
    }
}
impl Facts for RunFacts<'_, '_> {
    fn store(&self) -> &Store {
        self.data.store
    }
    fn quest_running(&mut self, q: &FormKey) -> Result<bool, &'static str> {
        self.state(q)?
            .running
            .ok_or("whether the quest runs is undetermined")
    }
    fn quest_stage(&mut self, q: &FormKey) -> Result<i32, &'static str> {
        self.state(q)?
            .current
            .ok_or("the current stage is undetermined")
    }
    fn stage_done(&mut self, q: &FormKey, n: i32) -> Result<bool, &'static str> {
        self.state(q)?
            .is_done(n)
            .ok_or("whether the stage is done is undetermined")
    }
    fn global(&mut self, g: &FormKey) -> Result<f32, &'static str> {
        if self.data.value_changed(self.w, g) {
            return Err("changed by this run");
        }
        data::global_fltv(self.data.store, g, self.data.max_memory)
    }
    fn event_member(&mut self, code: [u8; 2]) -> Result<Option<FormKey>, &'static str> {
        let e = self.event.ok_or("no story event is being offered")?;
        match &code {
            b"K1" => Ok(e.keyword.clone()),
            b"L1" => e.location.clone(),
            _ => Err("an event member function 576 is not evaluated for"),
        }
    }
    fn switches(&self) -> &cond::Switches {
        &self.cond.switches
    }
    fn max_memory(&self) -> u64 {
        self.data.max_memory
    }
    fn assumed(&self, function: u16) -> Option<f32> {
        let v = self.cond.assume.get(&function).copied();
        if v.is_some() {
            self.cond.assumed.set(self.cond.assumed.get() + 1);
        }
        v
    }
    fn owner(&self) -> Option<&FormKey> {
        self.owner.as_ref()
    }
    /// The alias as `ReferenceAlias.GetReference` answers it: the fill a
    /// run set, else the forced reference while the quest runs
    /// (`quest_alias_forced_reference_v1`).
    fn alias_reference(
        &mut self,
        quest: &FormKey,
        alias: i32,
    ) -> Result<Option<FormKey>, &'static str> {
        let i = info_by_key(self.data.store, self.data.max_memory, self.w, quest)
            .ok_or("the quest record could not be read")?;
        let s = self.state(quest)?;
        let (fill, running) = (s.fills.get(&alias).cloned(), s.running);
        if let Some(v) = fill {
            return match &v {
                Val::None => Ok(None),
                Val::Object(o) => self.w.objects[*o]
                    .key
                    .as_deref()
                    .and_then(data::key_record)
                    .map(Some)
                    .ok_or("the alias holds an object that is not a record"),
                _ => Err("the alias holds an undetermined value"),
            };
        }
        match running {
            None => Err("whether the quest runs is undetermined"),
            Some(false) => Ok(None),
            Some(true) => {
                let forced = u32::try_from(alias)
                    .ok()
                    .and_then(|a| i.reference_aliases.get(&a))
                    .ok_or("no reference alias of that index")?;
                forced
                    .clone()
                    .map(Some)
                    .ok_or("the alias has no forced reference")
            }
        }
    }
    /// The variable as the run holds it on the quest's attached script
    /// instances; before the run attached them, the value the records and
    /// compiled scripts give (`crate::vars`).
    fn quest_variable(&mut self, quest: &FormKey, name: &str) -> Result<f32, &'static str> {
        let key = bind::record_key(quest);
        let o = self
            .w
            .objects
            .iter()
            .position(|x| x.key.as_deref() == Some(key.as_str()) && x.attached);
        if let Some(o) = o {
            let mut found = self.w.objects[o]
                .instances
                .iter()
                .flat_map(|i| self.w.instances[*i].vars.iter())
                .filter(|v| v.name == name)
                .map(|v| crate::vars::number(&v.value));
            let first = found
                .next()
                .ok_or("no attached script of the quest declares the variable")??;
            for x in found {
                if x? != first {
                    return Err("more than one script of the quest declares the variable");
                }
            }
            return Ok(first);
        }
        let lib = self
            .cond
            .scripts
            .as_ref()
            .ok_or("the quest's scripts are not attached in this run")?;
        let b = Budget::new(self.data.max_memory);
        let mut lib = lib.borrow_mut();
        crate::vars::quest_variable_initial(self.data.store, &mut **lib, quest, name, &b)
    }
    fn scene_playing(&mut self, scene: &FormKey) -> Result<bool, &'static str> {
        if !self.w.scenes.on {
            return Err("scenes are not run");
        }
        match self.w.scenes.states.get(&bind::record_key(scene)) {
            Some(s) => s.playing.ok_or("whether the scene plays is undetermined"),
            None if self.w.quests.scenario == Scenario::NewGame => Ok(false),
            None => Err("whether the scene plays is undetermined"),
        }
    }
    /// The run completes no action of a scene (each waits on the world):
    /// an action is complete in no state the run settles
    /// (`cond_fn_550_scene_action_complete_v1`).
    fn scene_action_complete(&mut self, scene: &FormKey, _: u32) -> Result<bool, &'static str> {
        self.scene_playing(scene).map(|_| false)
    }
}

/// The quest host: the data host, plus the quest natives answered from
/// the quest state and the records.
pub struct QuestHost<'s> {
    pub data: DataHost<'s>,
    /// What the condition evaluator reads besides the state.
    pub cond: CondInputs<'s>,
    used: Vec<&'static str>,
    /// Answers by native and kind, as the data host counts its own.
    pub answers: BTreeMap<(String, data::Answer), u64>,
    /// Evaluate log entry conditions ([`cond`]); `false` leaves every
    /// conditioned entry undetermined, as before conditions were read.
    pub evaluate: bool,
    /// Deliver `OnStageSet` after a stage's fragment
    /// (`quest_on_stage_set_v1`); `false` delivers nothing.
    pub stage_event: bool,
    /// Log entry selections and condition lists by outcome, as text.
    pub conditions: BTreeMap<String, u64>,
    /// Function indexes of undetermined conditions in the lists that left
    /// a log entry selection open, once per selection.
    pub open_functions: BTreeMap<u16, u64>,
    /// The story manager's nodes: `Some` offers the script event raised by
    /// `Keyword.SendStoryEvent` to them (`sm_script_event_v1`); `None`
    /// records the call only, as before.
    pub story: Option<Rc<fo4_plugin::story::Tree>>,
    /// Story events offered and what they did, as text.
    pub story_counts: BTreeMap<String, u64>,
}

/// Natives the quest host answers, lower case `script.function`.
pub const NATIVES: [&str; 20] = [
    "quest.setcurrentstageid",
    "quest.getcurrentstageid",
    "quest.isstagedone",
    "quest.isrunning",
    "quest.start",
    "quest.stop",
    "quest.setobjectivedisplayed",
    "quest.setobjectivecompleted",
    "quest.setobjectivefailed",
    "quest.isobjectivedisplayed",
    "quest.isobjectivecompleted",
    "quest.isobjectivefailed",
    "quest.hasobjective",
    "quest.reset",
    "quest.completeallobjectives",
    "quest.failallobjectives",
    "referencealias.getreference",
    "referencealias.forcerefto",
    "referencealias.clear",
    "alias.getowningquest",
];

type Got = Result<(Val, data::Answer), &'static str>;

/// The answer of a native on a quest whose state is undetermined.
const OPEN: &str = "the quest's state is undetermined";

impl<'s> QuestHost<'s> {
    pub fn new(store: &'s Store, max_memory: u64) -> Self {
        QuestHost {
            data: DataHost::new(store, max_memory),
            cond: CondInputs::default(),
            used: Vec::new(),
            answers: BTreeMap::new(),
            evaluate: true,
            stage_event: true,
            conditions: BTreeMap::new(),
            open_functions: BTreeMap::new(),
            story: None,
            story_counts: BTreeMap::new(),
        }
    }
    pub(crate) fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    fn info_of(
        &self,
        w: &mut World,
        v: Option<&Val>,
    ) -> Result<(ObjId, Rc<QuestInfo>), &'static str> {
        let Some(Val::Object(o)) = v else {
            return Err("the quest is not an object");
        };
        let i = info(self.data.store, self.data.max_memory, w, *o)
            .ok_or("the receiver is not a quest record")?;
        Ok((*o, i))
    }
    fn int(v: Option<&Val>) -> Result<i32, &'static str> {
        match v {
            Some(Val::Int(n)) => Ok(*n),
            _ => Err("an argument is undetermined"),
        }
    }
    fn open(w: &mut World, o: ObjId, i: &QuestInfo, why: String) {
        let s = w.quests.state(i);
        if s.open.is_none() {
            s.open = Some(why.clone());
        }
        w.quests.log.push((o, Change::Opened(why)));
    }
    /// Starts a quest that is not running (`quest_start_v1`): running,
    /// OnQuestInit queued, then its startup stages set in stored order
    /// (all but `skip`).
    pub(crate) fn start_quest(
        &mut self,
        w: &mut World,
        o: ObjId,
        i: &Rc<QuestInfo>,
        skip: Option<i32>,
    ) {
        self.start_quest_jobs(w, o, i, skip);
        // A start by a stage set starts the scenes after that stage's own
        // fragment is queued (in `set_stage`).
        if skip.is_none() {
            crate::scene::quest_started(self, w, &i.key);
        }
    }
    fn start_quest_jobs(&mut self, w: &mut World, o: ObjId, i: &Rc<QuestInfo>, skip: Option<i32>) {
        self.using(rules::QUEST_START);
        self.using(rules::QUEST_ORDER);
        w.quests.state(i).running = Some(true);
        w.quests.log.push((o, Change::Started));
        let depth = w.quests.depth + 1;
        w.quests.queue.push_back(Job::Event {
            quest: o,
            event: "onquestinit",
            depth,
        });
        for s in &i.stages {
            let n = i32::from(s.stage);
            if s.flags & STARTUP_STAGE != 0 && Some(n) != skip {
                let _ = self.set_stage(w, o, i, n);
            }
        }
    }
    /// Sets a stage (`quest_set_stage_v1`); the answer of
    /// `SetCurrentStageID`.
    pub fn set_stage(&mut self, w: &mut World, o: ObjId, i: &Rc<QuestInfo>, n: i32) -> Got {
        self.using(rules::QUEST_SET_STAGE);
        if w.quests.state(i).open.is_some() {
            return Err(OPEN);
        }
        let Some(stage) = i.stage(n) else {
            Self::open(w, o, i, "a stage the record does not hold was set".into());
            return Err("the record holds no such stage");
        };
        let (done, running) = {
            let s = w.quests.state(i);
            (s.is_done(n), s.running)
        };
        let done = match (done, w.quests.scenario) {
            // The isolated scenario explores first settings: a stage this
            // run did not set counts as not done when it is set
            // (`quest_state_isolated_v1`).
            (None, Scenario::Isolated) => {
                self.using(rules::QUEST_ISOLATED);
                Some(false)
            }
            (d, _) => d,
        };
        match done {
            None => {
                Self::open(
                    w,
                    o,
                    i,
                    "a stage whose done-ness is undetermined was set".into(),
                );
                return Err("whether the stage is already done is undetermined");
            }
            Some(true) => {
                self.using(rules::QUEST_REPEATED_STAGE);
                let repeat = i.flags.is_some_and(|f| f & ALLOW_REPEATED_STAGES != 0);
                if !repeat {
                    w.quests.log.push((o, Change::StageRepeatRefused(n)));
                    return Ok((Val::Bool(false), data::Answer::Value));
                }
            }
            Some(false) => {}
        }
        let starts = running == Some(false);
        match running {
            Some(true) => {}
            Some(false) => self.start_quest(w, o, i, Some(n)),
            None => {
                // Running either way afterwards; whether this started it
                // (OnQuestInit, startup stages) is not settled.
                w.quests.state(i).running = Some(true);
                let depth = w.quests.depth + 1;
                w.quests.queue.push_back(Job::Open {
                    quest: o,
                    why: "whether setting the stage started the quest".into(),
                    depth,
                });
            }
        }
        let s = w.quests.state(i);
        s.done.insert(n, true);
        s.current = Some(n);
        w.quests.log.push((o, Change::StageSet(n)));
        // Which log entry's fragment runs (`quest_stage_log_entry_v1`):
        // the first entry whose conditions hold; one without conditions
        // holds; the conditions are evaluated in the state after the stage
        // is set (`cond_log_entry_state_v1`); an entry whose conditions are
        // undetermined leaves the choice open between it and what follows.
        self.using(rules::QUEST_LOG_ENTRY);
        self.using(rules::QUEST_ORDER);
        let mut choices: BTreeSet<Option<(usize, String)>> = BTreeSet::new();
        let mut settled = None;
        let mut open: Vec<cond::Why> = Vec::new();
        let mut evaluated = false;
        for (k, e) in stage.entries.iter().enumerate() {
            let t = if e.list.is_empty() {
                Truth::True
            } else if self.evaluate {
                evaluated = true;
                self.using(rules::QUEST_LOG_ENTRY_STATE);
                let ctx = Context {
                    order: self.data.store.order(),
                    plugin: i.plugin,
                };
                let mut used = BTreeSet::new();
                let mut facts = RunFacts::of(self, &mut *w, None, Some(i.key.clone()));
                let t = cond::eval_list_with(
                    &e.list,
                    ctx,
                    &i.file,
                    &mut facts,
                    Combine::OrGroups,
                    &mut used,
                );
                for u in used {
                    self.using(u);
                }
                *self
                    .conditions
                    .entry(format!("log entry condition lists\t{}", t.word()))
                    .or_default() += 1;
                t
            } else {
                Truth::Undetermined(Vec::new())
            };
            match t {
                Truth::False => {}
                Truth::True => {
                    choices.insert(e.fragment.clone().map(|f| (k, f)));
                    settled = Some(k);
                    break;
                }
                Truth::Undetermined(why) => {
                    choices.insert(e.fragment.clone().map(|f| (k, f)));
                    open.extend(why);
                }
            }
        }
        if settled.is_none() {
            choices.insert(None);
        }
        let funcs: BTreeSet<Option<&String>> =
            choices.iter().map(|c| c.as_ref().map(|x| &x.1)).collect();
        let depth = w.quests.depth + 1;
        let selection = if funcs.len() > 1 {
            w.quests.queue.push_back(Job::Open {
                quest: o,
                why: "which log entry's fragment runs depends on conditions".into(),
                depth,
            });
            let fs: BTreeSet<u16> = open.iter().map(|x| x.function).collect();
            for f in fs {
                *self.open_functions.entry(f).or_default() += 1;
            }
            "open"
        } else {
            if let Some(Some((entry, function))) = choices.into_iter().next() {
                self.using(rules::QUEST_FRAGMENT_ENTRY);
                w.quests.queue.push_back(Job::Fragment {
                    quest: o,
                    stage: n,
                    entry,
                    function,
                    depth,
                });
            }
            "settled"
        };
        if evaluated {
            *self
                .conditions
                .entry(format!(
                    "log entry selections with conditions evaluated\t{selection}"
                ))
                .or_default() += 1;
        }
        if self.stage_event {
            self.using(rules::QUEST_ON_STAGE_SET);
            w.quests.queue.push_back(Job::StageEvent {
                quest: o,
                stage: n,
                entry: settled.filter(|_| selection == "settled"),
                depth,
            });
        }
        if starts {
            crate::scene::quest_started(self, w, &i.key);
        }
        Ok((Val::Bool(true), data::Answer::Value))
    }
    fn objective(&mut self, c: &NativeCall, w: &mut World, kind: u8, set: bool) -> Got {
        self.using(rules::QUEST_OBJECTIVES);
        let (o, i) = self.info_of(w, c.receiver.as_ref())?;
        if w.quests.state(&i).open.is_some() {
            return if set {
                Ok((Val::None, data::Answer::NoRecord))
            } else {
                Err(OPEN)
            };
        }
        let n = Self::int(c.args.first());
        if !set {
            let n = n?;
            let s = w.quests.state(&i);
            return match s
                .objectives
                .get(&(kind, n))
                .copied()
                .unwrap_or(s.objectives_rest)
            {
                Some(v) => Ok((Val::Bool(v), data::Answer::Value)),
                None => Err("the objective's state is undetermined"),
            };
        }
        let change = match n {
            Ok(n) => {
                let v = match c.args.get(1) {
                    Some(Val::Bool(b)) => Some(*b),
                    _ => None,
                };
                w.quests.state(&i).objectives.insert((kind, n), v);
                Change::Objective(kind, n, v)
            }
            Err(_) => {
                let s = w.quests.state(&i);
                s.objectives.clear();
                s.objectives_rest = None;
                Change::Objective(kind, -1, None)
            }
        };
        w.quests.log.push((o, change));
        Ok((Val::None, data::Answer::NoRecord))
    }
    /// The quest and alias id behind an alias handle.
    fn alias_of(
        &self,
        w: &mut World,
        v: Option<&Val>,
    ) -> Result<(ObjId, Rc<QuestInfo>, i32), &'static str> {
        let Some(Val::Object(a)) = v else {
            return Err("the alias is not an object");
        };
        let key = w.objects[*a].key.clone().ok_or("the alias is opaque")?;
        let (q, id) = key.split_once("#alias:").ok_or("not an alias of a quest")?;
        let id: i32 = id
            .parse()
            .map_err(|_| "an alias index that is not a number")?;
        let qk = key_record(q).ok_or("not an alias of a quest record")?;
        let b = Budget::new(self.data.max_memory);
        let qo = record_object(w, self.data.store, &qk, "quest", &b)
            .map_err(|_| "the quest could not be read")?;
        let i = info(self.data.store, self.data.max_memory, w, qo).ok_or("not a quest record")?;
        Ok((qo, i, id))
    }
    fn answer(&mut self, c: &NativeCall, w: &mut World) -> Got {
        self.using(rules::QUEST_LAYOUT);
        let full = c.full_name();
        match full.as_str() {
            "quest.setcurrentstageid" => {
                let (o, i) = self.info_of(w, c.receiver.as_ref())?;
                match Self::int(c.args.first()) {
                    Ok(n) => self.set_stage(w, o, &i, n),
                    Err(e) => {
                        Self::open(w, o, &i, "an undetermined stage was set".into());
                        Err(e)
                    }
                }
            }
            "quest.getcurrentstageid" | "quest.isstagedone" | "quest.isrunning" => {
                let (_, i) = self.info_of(w, c.receiver.as_ref())?;
                let s = w.quests.state(&i);
                if s.open.is_some() {
                    return Err(OPEN);
                }
                let v = match c.name.as_str() {
                    "getcurrentstageid" => {
                        self.using(rules::QUEST_SET_STAGE);
                        s.current.map(Val::Int)
                    }
                    "isstagedone" => {
                        self.using(rules::QUEST_SET_STAGE);
                        let n = Self::int(c.args.first())?;
                        s.is_done(n).map(Val::Bool)
                    }
                    _ => {
                        self.using(rules::QUEST_START);
                        s.running.map(Val::Bool)
                    }
                };
                v.map(|v| (v, data::Answer::Value))
                    .ok_or("not settled by the quest's state")
            }
            "quest.start" => {
                let (o, i) = self.info_of(w, c.receiver.as_ref())?;
                self.using(rules::QUEST_START);
                let s = w.quests.state(&i);
                if s.open.is_some() {
                    return Err(OPEN);
                }
                match s.running {
                    Some(false) => {
                        self.start_quest(w, o, &i, None);
                        Ok((Val::Bool(true), data::Answer::Value))
                    }
                    Some(true) => Err("Start of a running quest"),
                    None => {
                        Self::open(
                            w,
                            o,
                            &i,
                            "Start of a quest whose running state is undetermined".into(),
                        );
                        Err("whether the quest was running is undetermined")
                    }
                }
            }
            "quest.stop" => {
                let (o, i) = self.info_of(w, c.receiver.as_ref())?;
                self.using(rules::QUEST_STOP);
                let depth = w.quests.depth + 1;
                let s = w.quests.state(&i);
                let was = s.running;
                s.running = Some(false);
                w.quests.log.push((o, Change::Stopped));
                match was {
                    Some(true) => w.quests.queue.push_back(Job::Event {
                        quest: o,
                        event: "onquestshutdown",
                        depth,
                    }),
                    Some(false) => {}
                    None => w.quests.queue.push_back(Job::Open {
                        quest: o,
                        why: "whether OnQuestShutdown runs (running state undetermined)".into(),
                        depth,
                    }),
                }
                Ok((Val::None, data::Answer::NoRecord))
            }
            "quest.setobjectivedisplayed" => self.objective(c, w, DISPLAYED, true),
            "quest.setobjectivecompleted" => self.objective(c, w, COMPLETED, true),
            "quest.setobjectivefailed" => self.objective(c, w, FAILED, true),
            "quest.isobjectivedisplayed" => self.objective(c, w, DISPLAYED, false),
            "quest.isobjectivecompleted" => self.objective(c, w, COMPLETED, false),
            "quest.isobjectivefailed" => self.objective(c, w, FAILED, false),
            "quest.hasobjective" => {
                self.using(rules::QUEST_OBJECTIVES);
                let (_, i) = self.info_of(w, c.receiver.as_ref())?;
                let n = Self::int(c.args.first())?;
                let has = u16::try_from(n).is_ok_and(|n| i.objectives.contains(&n));
                Ok((Val::Bool(has), data::Answer::Value))
            }
            "quest.reset" | "quest.completeallobjectives" | "quest.failallobjectives" => {
                let (o, i) = self.info_of(w, c.receiver.as_ref())?;
                if c.name == "reset" {
                    Self::open(w, o, &i, "Reset".into());
                } else {
                    let s = w.quests.state(&i);
                    s.objectives.clear();
                    s.objectives_rest = None;
                    w.quests
                        .log
                        .push((o, Change::Objective(DISPLAYED, -1, None)));
                }
                Ok((Val::None, data::Answer::NoRecord))
            }
            "referencealias.getreference" => {
                self.using(rules::QUEST_ALIAS_FORCED);
                let (_, i, id) = self.alias_of(w, c.receiver.as_ref())?;
                let s = w.quests.state(&i);
                if s.open.is_some() {
                    return Err(OPEN);
                }
                if let Some(v) = s.fills.get(&id) {
                    let kind = match v {
                        Val::None => data::Answer::NoRecord,
                        _ => data::Answer::Record,
                    };
                    return Ok((v.clone(), kind));
                }
                match s.running {
                    None => Err("whether the quest runs is undetermined"),
                    Some(false) => Ok((Val::None, data::Answer::NoRecord)),
                    Some(true) => {
                        let forced = u32::try_from(id)
                            .ok()
                            .and_then(|id| i.reference_aliases.get(&id))
                            .ok_or("no reference alias of that index")?;
                        let k = forced.as_ref().ok_or("the alias has no forced reference")?;
                        if self.data.store.signature(k).is_none() {
                            return Err("the forced reference names no winning record");
                        }
                        let b = Budget::new(self.data.max_memory);
                        let o = record_object(w, self.data.store, k, "objectreference", &b)
                            .map_err(|_| "the forced reference could not be read")?;
                        Ok((Val::Object(o), data::Answer::Record))
                    }
                }
            }
            "referencealias.forcerefto" | "referencealias.clear" => {
                self.using(rules::QUEST_ALIAS_FORCED);
                let (_, i, id) = self.alias_of(w, c.receiver.as_ref())?;
                let v = if c.name == "clear" {
                    Val::None
                } else {
                    c.args.first().cloned().unwrap_or(Val::None)
                };
                w.quests.state(&i).fills.insert(id, v);
                Ok((Val::None, data::Answer::NoRecord))
            }
            "alias.getowningquest" => {
                self.using(rules::ALIAS_VALUE);
                let (q, _, _) = self.alias_of(w, c.receiver.as_ref())?;
                Ok((Val::Object(q), data::Answer::Record))
            }
            _ => Err("not a quest native"),
        }
    }
}

impl Host for QuestHost<'_> {
    fn call(&mut self, c: &NativeCall, w: &mut World) -> Reply {
        let full = c.full_name();
        // The event natives, when their delivery is switched on; otherwise
        // they are recorded as any other native, as before.
        let on = if full.contains("remoteevent") {
            w.events.remote.then_some(rules::REMOTE_EVENTS)
        } else if full.contains("customevent") {
            w.events.custom.map(|_| rules::CUSTOM_EVENTS)
        } else {
            None
        };
        if let (true, Some(rule)) = (crate::events::NATIVES.contains(&full.as_str()), on) {
            let depth = w.quests.depth + 1;
            if let Some(r) = crate::events::native(c, w, depth) {
                self.using(rule);
                if full == "scriptobject.sendcustomevent" {
                    self.using(rules::CUSTOM_EVENT_ORDER);
                }
                return r;
            }
        }
        if self.story.is_some() && crate::story::NATIVES.contains(&full.as_str()) {
            return crate::story::send(self, c, w);
        }
        if w.scenes.on
            && (crate::scene::NATIVES.contains(&full.as_str())
                || (c.script == "scene" && c.name == crate::scene::STEP))
        {
            return crate::scene::call(self, c, w);
        }
        if !NATIVES.contains(&full.as_str()) {
            let r = self.data.call(c, w);
            for u in self.data.rules() {
                if !self.used.contains(u) {
                    self.used.push(u);
                }
            }
            return r;
        }
        let (reply, answer) = match self.answer(c, w) {
            Ok((v, a)) => (Reply::Value(v), a),
            Err(why) => {
                // A setter answers none even when the state is open.
                let a = data::Answer::Open(why);
                if c.return_type == "none" {
                    (Reply::Value(Val::None), a)
                } else {
                    (Reply::Undetermined, a)
                }
            }
        };
        *self.answers.entry((full, answer)).or_default() += 1;
        reply
    }
    fn rules(&self) -> &[&'static str] {
        &self.used
    }
}

/// The event a stage's setting delivers (`quest_on_stage_set_v1`).
pub const ON_STAGE_SET: &str = "onstageset";
/// The event a quest the story manager started for a script event
/// receives (`sm_quest_node_start_v1`).
pub const ON_STORY_SCRIPT: &str = "onstoryscript";

/// One job run by [`run_jobs`].
#[derive(Debug, Clone)]
pub struct JobRun {
    /// Order in which the job started.
    pub seq: u64,
    pub job: Job,
    /// (script, outcome) per delivery; empty for an open job or an event
    /// no script handles.
    pub outcomes: Vec<(String, Outcome)>,
    /// (script, outcome) per remote delivery of the event to the
    /// instances registered for it (`vm_remote_events_v1`).
    pub remote: Vec<(String, Outcome)>,
    /// The effects of this job: a range of `vm.effects` (with nested
    /// jobs, including theirs).
    pub effects: std::ops::Range<usize>,
    /// What the binding could not apply.
    pub notes: Vec<String>,
}

/// The fragment script instance on a quest's handle
/// (`quest_fragment_instance_v1`): created when first needed, after the
/// quest's own VMAD scripts are attached, with the block's property values.
pub fn fragment_instance(
    vm: &mut Vm<'_>,
    store: &Store,
    o: ObjId,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<InstId, VmError> {
    if let Some(i) = vm.world.quests.fragment_instances.get(&o) {
        return Ok(*i);
    }
    vm.attach_scripts(o)?;
    let key = vm.world.objects[o]
        .key
        .clone()
        .and_then(|k| key_record(&k))
        .ok_or_else(|| VmError::BadArgument {
            site: None,
            detail: "a fragment on an object that is not a quest record".into(),
        })?;
    let bad = |e: &dyn std::fmt::Display| VmError::BadArgument {
        site: None,
        detail: format!("reading the quest's VMAD: {e}"),
    };
    let (r, plugin) = bind::vmad_of(store, &key, b)
        .map_err(|e| bad(&e))?
        .ok_or_else(|| bad(&"no VMAD"))?;
    let s = r
        .subrecords
        .iter()
        .find(|s| s.signature == *b"VMAD")
        .ok_or_else(|| bad(&"no VMAD"))?;
    let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b).map_err(|e| bad(&e))?;
    let block = v.quest.as_ref().ok_or_else(|| bad(&"no quest block"))?;
    let name = fo4_pex::index::lower(block.script.name);
    vm.rules.insert(rules::QUEST_FRAGMENT_INSTANCE);
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
    bind::apply(vm, &cx, inst, &name, &block.script.properties, notes).map_err(|e| bad(&e))?;
    vm.world.quests.fragment_instances.insert(o, inst);
    Ok(inst)
}

/// Runs one job: a fragment on the quest's fragment script instance, an
/// event on every script instance of the quest that handles it, or
/// nothing for an open job. `nested` delivers inside the running
/// delivery ([`Vm::deliver_nested`]).
pub fn run_one(vm: &mut Vm<'_>, store: &Store, job: Job, nested: bool, b: &Budget) -> JobRun {
    vm.world.quests.seq += 1;
    let seq = vm.world.quests.seq;
    let saved = vm.world.quests.depth;
    vm.world.quests.depth = job.depth();
    let start = vm.effects.len();
    let mut outcomes = Vec::new();
    let mut remote = Vec::new();
    let mut notes = Vec::new();
    let deliver = |vm: &mut Vm<'_>, i: InstId, f: &str| {
        if nested {
            vm.deliver_nested(i, f, Vec::new())
        } else {
            vm.deliver(i, f, Vec::new())
        }
    };
    match &job {
        Job::Fragment {
            quest, function, ..
        } => {
            match fragment_instance(vm, store, *quest, &mut notes, b) {
                Err(e) => outcomes.push((String::new(), Outcome::Failed(e))),
                Ok(inst) => {
                    let script = vm.world.instances[inst].script.clone();
                    // A fragment function without instructions runs and
                    // returns at once.
                    let found = match vm.dispatch(inst, function) {
                        Ok(f) => Ok(f),
                        Err(crate::Stop::Error(e)) => Err(*e),
                        Err(crate::Stop::Blocked(bl)) => Err(VmError::BadArgument {
                            site: None,
                            detail: format!("dispatch blocked on {}", bl.on.text()),
                        }),
                    };
                    match found {
                        Ok(Some(f)) if f.params.is_empty() => {
                            let o = deliver(vm, inst, function);
                            outcomes.push((script, o));
                        }
                        Ok(Some(_)) => outcomes.push((
                            script,
                            Outcome::Failed(VmError::BadArgument {
                                site: None,
                                detail: format!("fragment {function} takes parameters"),
                            }),
                        )),
                        Ok(None) => outcomes.push((
                            script.clone(),
                            Outcome::Failed(VmError::MissingFunction {
                                script,
                                name: function.clone(),
                            }),
                        )),
                        Err(e) => outcomes.push((script, Outcome::Failed(e))),
                    }
                }
            }
        }
        Job::Event { quest, event, .. } => {
            if let Err(e) = vm.attach_scripts(*quest) {
                outcomes.push((String::new(), Outcome::Failed(e)));
            } else {
                vm.rules.insert(rules::QUEST_START);
                let insts = vm.world.objects[*quest].instances.clone();
                for i in insts {
                    let script = vm.world.instances[i].script.clone();
                    match vm.find_handler(i, event) {
                        Ok(Some(f)) if f.params.is_empty() => {
                            let o = deliver(vm, i, event);
                            outcomes.push((script, o));
                        }
                        Ok(_) => {}
                        Err(e) => outcomes.push((script, Outcome::Failed(e))),
                    }
                }
                let mut none = |_: &mut Vm<'_>, p: &[(String, String)]| p.is_empty().then(Vec::new);
                remote = crate::events::deliver_remote(vm, *quest, event, &mut none, nested);
            }
        }
        Job::StageEvent {
            quest,
            stage,
            entry,
            ..
        } => {
            if let Err(e) = vm.attach_scripts(*quest) {
                outcomes.push((String::new(), Outcome::Failed(e)));
            } else {
                vm.rules.insert(rules::QUEST_ON_STAGE_SET);
                let insts = vm.world.objects[*quest].instances.clone();
                for i in insts {
                    let script = vm.world.instances[i].script.clone();
                    match vm.find_handler(i, ON_STAGE_SET) {
                        Ok(Some(f)) => {
                            let ints = f.params.len() == 2
                                && f.params.iter().all(|(_, t)| t.as_str() == "int");
                            if !ints {
                                outcomes.push((
                                    script,
                                    Outcome::Failed(VmError::BadArgument {
                                        site: None,
                                        detail: "OnStageSet does not take two Int".into(),
                                    }),
                                ));
                                continue;
                            }
                            let item = match entry {
                                Some(k) => Val::Int(*k as i32),
                                None => vm.world.unknown(crate::Origin::Rule(
                                    rules::QUEST_ON_STAGE_SET,
                                    "the log entry selected is undetermined".into(),
                                )),
                            };
                            let args = vec![Val::Int(*stage), item];
                            let o = if nested {
                                vm.deliver_nested(i, ON_STAGE_SET, args)
                            } else {
                                vm.deliver(i, ON_STAGE_SET, args)
                            };
                            outcomes.push((script, o));
                        }
                        Ok(None) => {}
                        Err(e) => outcomes.push((script, Outcome::Failed(e))),
                    }
                }
                let (stage, entry) = (*stage, *entry);
                let mut two = |vm: &mut Vm<'_>, p: &[(String, String)]| {
                    let ints = p.len() == 2 && p.iter().all(|(_, t)| t.as_str() == "int");
                    ints.then(|| {
                        let item = match entry {
                            Some(k) => Val::Int(k as i32),
                            None => vm.world.unknown(crate::Origin::Rule(
                                rules::QUEST_ON_STAGE_SET,
                                "the log entry selected is undetermined".into(),
                            )),
                        };
                        vec![Val::Int(stage), item]
                    })
                };
                remote = crate::events::deliver_remote(vm, *quest, ON_STAGE_SET, &mut two, nested);
            }
        }
        Job::Custom {
            sender,
            event,
            args,
            ..
        } => {
            outcomes = crate::events::deliver_custom(vm, *sender, event, args.clone(), nested);
        }
        Job::Story { quest, args, .. } => {
            if let Err(e) = vm.attach_scripts(*quest) {
                outcomes.push((String::new(), Outcome::Failed(e)));
            } else {
                vm.rules.insert(rules::SM_QUEST_START);
                let insts = vm.world.objects[*quest].instances.clone();
                for i in insts {
                    let script = vm.world.instances[i].script.clone();
                    match vm.find_handler(i, ON_STORY_SCRIPT) {
                        Ok(Some(f)) if f.params.len() == args.len() => {
                            let o = if nested {
                                vm.deliver_nested(i, ON_STORY_SCRIPT, args.clone())
                            } else {
                                vm.deliver(i, ON_STORY_SCRIPT, args.clone())
                            };
                            outcomes.push((script, o));
                        }
                        Ok(Some(_)) => outcomes.push((
                            script,
                            Outcome::Failed(VmError::BadArgument {
                                site: None,
                                detail: "OnStoryScript does not take the event's six values".into(),
                            }),
                        )),
                        Ok(None) => {}
                        Err(e) => outcomes.push((script, Outcome::Failed(e))),
                    }
                }
            }
        }
        Job::Scene { scene, step, .. } => match step {
            crate::scene::Step::Fragment(function, _) => {
                match crate::scene::fragment_instance(vm, store, *scene, &mut notes, b) {
                    Err(e) => outcomes.push((String::new(), Outcome::Failed(e))),
                    Ok(inst) => {
                        let script = vm.world.instances[inst].script.clone();
                        let found = match vm.dispatch(inst, function) {
                            Ok(f) => Ok(f),
                            Err(crate::Stop::Error(e)) => Err(*e),
                            Err(crate::Stop::Blocked(bl)) => Err(VmError::BadArgument {
                                site: None,
                                detail: format!("dispatch blocked on {}", bl.on.text()),
                            }),
                        };
                        match found {
                            Ok(Some(f)) if f.params.is_empty() => {
                                let o = deliver(vm, inst, function);
                                outcomes.push((script, o));
                            }
                            Ok(Some(_)) => outcomes.push((
                                script,
                                Outcome::Failed(VmError::BadArgument {
                                    site: None,
                                    detail: format!("scene fragment {function} takes parameters"),
                                }),
                            )),
                            Ok(None) => outcomes.push((
                                script.clone(),
                                Outcome::Failed(VmError::MissingFunction {
                                    script,
                                    name: function.clone(),
                                }),
                            )),
                            Err(e) => outcomes.push((script, Outcome::Failed(e))),
                        }
                    }
                }
            }
            crate::scene::Step::Enter | crate::scene::Step::Complete => {
                let complete = *step == crate::scene::Step::Complete;
                let call = crate::scene::step_call(*scene, complete);
                let _ = vm.host.call(&call, &mut vm.world);
                for r in vm.host.rules() {
                    vm.rules.insert(r);
                }
            }
        },
        Job::Open { .. } => {}
    }
    vm.world.quests.depth = saved;
    JobRun {
        seq,
        job,
        outcomes,
        remote,
        effects: start..vm.effects.len(),
        notes,
    }
}

/// Runs the queued jobs in order, those they queue after them, until the
/// queue is empty or `max` jobs ran (`quest_stage_fragment_order_v1`,
/// queued reading). Returns the jobs run; jobs left are still in
/// `vm.world.quests.queue`. With a [`Runner`] in `vm.nested`, the jobs a
/// run queues run inside it instead and are listed in
/// `vm.world.quests.nested_runs`.
pub fn run_jobs(vm: &mut Vm<'_>, store: &Store, max: usize, b: &Budget) -> Vec<JobRun> {
    let mut out = Vec::new();
    while out.len() < max {
        if vm.world.quests.queue.is_empty() {
            release_later(vm);
        }
        let Some(job) = vm.world.quests.queue.pop_front() else {
            break;
        };
        // With a runner, the jobs queued beside this one are set aside
        // while it runs, so the runner runs inside it only the jobs it
        // brings (`quest_stage_fragment_order_v1`).
        let nested = vm.nested.is_some();
        let siblings = if nested {
            std::mem::take(&mut vm.world.quests.queue)
        } else {
            VecDeque::new()
        };
        let r = run_one(vm, store, job, false, &b.child());
        if nested {
            restore(vm, siblings);
        }
        out.push(r);
        release_later(vm);
    }
    out
}

/// Puts the custom events sent under the queued reading
/// (`custom_event_order_v1`) behind the jobs queued now, in sending order:
/// asked when an outermost delivery or job ends.
pub fn release_later(vm: &mut Vm<'_>) {
    let later = std::mem::take(&mut vm.world.events.later);
    vm.world.quests.queue.extend(later);
}

/// Puts the jobs set aside back behind the jobs left by the one that ran.
fn restore(vm: &mut Vm<'_>, siblings: VecDeque<Job>) {
    let q = &mut vm.world.quests.queue;
    q.extend(siblings);
}

/// The nested reading of `quest_stage_fragment_order_v1`: the jobs a
/// native queues run at once, inside the run that called it, before the
/// native returns, each one's own queued jobs inside it in turn. The
/// interpreter's step and call-depth limits bound the nesting.
pub struct Runner<'s> {
    pub store: &'s Store,
    pub max_memory: u64,
    /// Jobs this runner may run in all.
    pub max: usize,
}
impl<'s> Runner<'s> {
    pub fn new(store: &'s Store, max_memory: u64, max: usize) -> Self {
        Runner {
            store,
            max_memory,
            max,
        }
    }
}
impl crate::Nested for Runner<'_> {
    fn run(&mut self, vm: &mut Vm<'_>) {
        vm.rules.insert(rules::QUEST_ORDER);
        while vm.world.quests.nested_runs.len() < self.max {
            let Some(job) = vm.world.quests.queue.pop_front() else {
                break;
            };
            let b = Budget::new(self.max_memory);
            // The jobs queued beside this one wait until it ends: a run
            // brings inside it only the jobs it queues.
            let siblings = std::mem::take(&mut vm.world.quests.queue);
            let r = run_one(vm, self.store, job, true, &b);
            restore(vm, siblings);
            vm.world.quests.nested_runs.push(r);
        }
    }
}

/// Asks the host to set a stage as a script's `SetCurrentStageID` call
/// would, outside any script (the sweeps' entry point). In the isolated
/// scenario the quest is first taken to be running
/// (`quest_state_isolated_v1`): the sweep sets a stage of a running quest.
pub fn set_stage(vm: &mut Vm<'_>, store: &Store, quest: ObjId, stage: i32) -> Reply {
    let call = NativeCall {
        script: "quest".into(),
        name: "setcurrentstageid".into(),
        receiver: Some(Val::Object(quest)),
        instance: None,
        args: vec![Val::Int(stage)],
        param_types: vec!["int".into()],
        return_type: "bool".into(),
    };
    vm.world.quests.depth = 0;
    if vm.world.quests.scenario == Scenario::Isolated {
        if let Some(i) = info(store, store.limits.max_memory, &mut vm.world, quest) {
            let s = vm.world.quests.state(&i);
            if s.running.is_none() {
                s.running = Some(true);
                vm.rules.insert(rules::QUEST_ISOLATED);
            }
        }
    }
    let r = vm.host.call(&call, &mut vm.world);
    for u in vm.host.rules() {
        vm.rules.insert(u);
    }
    r
}

/// Asks the host to start a quest as a script's `Start` call would.
pub fn start(vm: &mut Vm<'_>, quest: ObjId) -> Reply {
    let call = NativeCall {
        script: "quest".into(),
        name: "start".into(),
        receiver: Some(Val::Object(quest)),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "bool".into(),
    };
    vm.world.quests.depth = 0;
    let r = vm.host.call(&call, &mut vm.world);
    for u in vm.host.rules() {
        vm.rules.insert(u);
    }
    r
}

/// The handle of a quest record, known to be a quest.
pub fn quest_object(
    vm: &mut Vm<'_>,
    store: &Store,
    k: &FormKey,
    b: &Budget,
) -> fo4_core::Result<ObjId> {
    record_object(&mut vm.world, store, k, "quest", b)
}

/// The start of a new game (`quest_start_game_enabled_v1`): the scenario
/// becomes [`Scenario::NewGame`] and every winning quest whose DNAM
/// carries the start-game bit is started as `Start` starts a quest
/// (`quest_start_v1`), in the store's order of winning records. Returns
/// each quest started with the host's answer; the runs it queues are left
/// for [`run_jobs`].
pub fn start_new_game(
    vm: &mut Vm<'_>,
    store: &Store,
    b: &Budget,
) -> fo4_core::Result<Vec<(FormKey, ObjId, Reply)>> {
    vm.world.quests.scenario = Scenario::NewGame;
    vm.rules.insert(rules::QUEST_START_GAME);
    let quests: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut out = Vec::new();
    for k in quests {
        let o = quest_object(vm, store, &k, b)?;
        let Some(i) = info(store, store.limits.max_memory, &mut vm.world, o) else {
            continue;
        };
        if !i.start_game_enabled() {
            continue;
        }
        // Running from the scenario's first instant; started now.
        vm.world.quests.state(&i).running = Some(false);
        let r = start(vm, o);
        out.push((k, o, r));
    }
    Ok(out)
}
