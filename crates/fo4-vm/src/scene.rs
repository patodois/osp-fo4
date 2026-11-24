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
//!
//! With the scene clock on ([`Clock::on`], `scene_clock_v1`) actions start
//! when their start phase is entered and hold every phase they span until
//! they complete; an action whose time the records settle (a timer's
//! seconds, `scen_timer_action_seconds_v1`; a spoken line's audio,
//! `scene_dialogue_line_audio_v1`) or the caller assumes completes when
//! the caller advances the clock past it ([`advance_clock`]); every other
//! action waits as before.
use crate::{
    bind,
    cond::{self, Combine, Truth},
    data::{key_record, record_object},
    host::{NativeCall, Reply},
    quest::{self, Job, QuestHost, RunFacts},
    rules,
    voice::{self, LineTime, Speaker},
    world::{Clock as TimerClock, InstId, ObjId, Val, World},
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

/// The synthetic call of [`recheck_distance_marks`]: the function 1 marks
/// of a scene waiting on its completion conditions computed again (no
/// script calls it).
pub const MARKS: &str = "::distance marks";

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
    /// SCQS: the stages of the owning quest set when the phase begins and
    /// when it completes (`record_set_stage_fields_v1`), -1 for none.
    pub stage_fields: [i16; 2],
}

/// The player's topics of a player dialogue action (kind 3) with the
/// topic of the answer each pairs with, in the order
/// `--assume-player-choice` numbers them (prior knowledge of the tags'
/// letters: positive, negative, neutral, question; `spec/voice.md`).
pub const PLAYER_CHOICES: [([u8; 4], [u8; 4]); 4] = [
    (*b"PTOP", *b"NPOT"),
    (*b"NTOP", *b"NNGT"),
    (*b"NETO", *b"NNUT"),
    (*b"QTOP", *b"NQUT"),
];

/// One action, as the scene clock needs it.
#[derive(Debug, Clone, Default)]
pub struct ActionInfo {
    pub kind: u16,
    /// INAM: the action's index, as conditions of function 550 name it.
    pub index: u32,
    pub start: usize,
    pub end: usize,
    /// ALID: the speaker or actor alias, -1 for none.
    pub alias: i32,
    /// FNAM, as stored.
    pub flags: Option<u32>,
    /// Kind 2: the SNAM after the end phase as an f32 (seconds under
    /// `scen_timer_action_seconds_v1`) and the TNAM, when stored.
    pub seconds: Option<f32>,
    pub low: Option<f32>,
    /// Kind 0: the topic DATA names.
    pub topic: Option<FormKey>,
    /// Kind 4: the scene LCEP names.
    pub scene: Option<FormKey>,
    /// Kind 1: the package PNAM names.
    pub package: Option<FormKey>,
    /// Kind 3: the topics of [`PLAYER_CHOICES`], resolved.
    pub choices: Vec<(Option<FormKey>, Option<FormKey>)>,
    /// Kind 5: the topics its first four tags of [`PLAYER_CHOICES`] name,
    /// resolved, in that order.
    pub responses: Vec<Option<FormKey>>,
}
impl ActionInfo {
    fn of(a: &fo4_plugin::scen::Action) -> Self {
        let topic = |t: &[u8; 4]| {
            a.field(t)
                .and_then(|f| f.form.as_ref())
                .and_then(|f| f.key.clone())
        };
        ActionInfo {
            kind: a.kind,
            index: a.index,
            start: a.start as usize,
            end: a.end as usize,
            alias: a.alias,
            flags: a.flags,
            seconds: (a.kind == TIMER).then(|| a.float(b"SNAM")).flatten(),
            low: (a.kind == TIMER).then(|| a.float(b"TNAM")).flatten(),
            topic: (a.kind == LINE).then(|| topic(b"DATA")).flatten(),
            scene: (a.kind == START_SCENE).then(|| topic(b"LCEP")).flatten(),
            package: (a.kind == PACKAGE).then(|| topic(b"PNAM")).flatten(),
            choices: if a.kind == PLAYER_DIALOGUE {
                PLAYER_CHOICES
                    .iter()
                    .map(|(p, r)| (topic(p), topic(r)))
                    .collect()
            } else {
                Vec::new()
            },
            responses: if a.kind == NPC_RESPONSE {
                PLAYER_CHOICES.iter().map(|(p, _)| topic(p)).collect()
            } else {
                Vec::new()
            },
        }
    }
}

/// Action kinds the clock times (`scen_record_layout_v1`).
pub const LINE: u16 = 0;
pub const PACKAGE: u16 = 1;
pub const TIMER: u16 = 2;
pub const PLAYER_DIALOGUE: u16 = 3;
pub const START_SCENE: u16 = 4;
/// The response line kind (`scene_npc_response_choice_v1`).
pub const NPC_RESPONSE: u16 = 5;

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
    /// Every action, in stored order.
    pub actions: Vec<ActionInfo>,
    /// The scene's SCQS: the stages of the owning quest set when the scene
    /// begins and when it ends (`record_set_stage_fields_v1`), -1 for none.
    pub stage_fields: [i16; 2],
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
                stage_fields: p.scqs.as_deref().and_then(stage_pair).unwrap_or([-1, -1]),
            })
            .collect();
        let stage_fields = s
            .fields
            .iter()
            .find(|f| f.place == "scene" && f.tag == *b"SCQS")
            .and_then(|f| r.subrecords.get(f.index))
            .and_then(|x| stage_pair(r.data(x)))
            .unwrap_or([-1, -1]);
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
            actions: s.actions.iter().map(ActionInfo::of).collect(),
            unplaced,
            stage_fields,
        }))
    }
}

/// Two little-endian i16 of a four-byte stage field.
pub(crate) fn stage_pair(d: &[u8]) -> Option<[i16; 2]> {
    let d: [u8; 4] = d.try_into().ok()?;
    Some([
        i16::from_le_bytes([d[0], d[1]]),
        i16::from_le_bytes([d[2], d[3]]),
    ])
}

/// Sets a stage a scene or phase field names on the scene's owning quest,
/// when the fields are read (`Scenes::stage_fields`,
/// `record_set_stage_fields_v1`): as `SetCurrentStageID` would, the jobs
/// it brings queued behind the step running now.
fn field_stage(h: &mut QuestHost<'_>, w: &mut World, i: &SceneInfo, n: i16) {
    if !w.scenes.stage_fields || n < 0 {
        return;
    }
    let Some(q) = i.quest.clone() else { return };
    set_field_stage(h, w, &q, n);
}

/// Sets a stage a record field names on a quest, as `SetCurrentStageID`
/// would (`record_set_stage_fields_v1`).
pub(crate) fn set_field_stage(h: &mut QuestHost<'_>, w: &mut World, q: &FormKey, n: i16) {
    h.using(rules::SETTER_FIELDS);
    let b = Budget::new(h.data.max_memory);
    let Ok(qo) = record_object(w, h.data.store, q, "quest", &b) else {
        return;
    };
    let Some(qi) = quest::info(h.data.store, h.data.max_memory, w, qo) else {
        return;
    };
    let _ = h.set_stage(w, qo, &qi, i32::from(n));
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
    /// Set the line stage fields the clock has reached
    /// (`scene_line_stage_fields_v1`).
    LineFields,
    /// A spoken line's fragment (`dialogue_line_fragments_v1`): the
    /// function on the fragment script instance of the line (a record key),
    /// its slot (`begin` or `end`), the speaker's handle as its argument.
    /// The job's scene is the scene whose clock plays the line, or the
    /// line's own handle outside scenes.
    LineFragment {
        line: String,
        function: String,
        slot: String,
        speaker: Option<ObjId>,
    },
    /// A spoken line's event delivered to the scripts its record's VMAD
    /// attaches of its own (not the fragment block;
    /// `dialogue_line_script_events_v1`): the line (a record key), the
    /// event (`onbegin` or `onend`), the speaker's handle.
    LineScript {
        line: String,
        event: String,
        speaker: Option<ObjId>,
    },
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
    /// The scene clock (only with [`Clock::on`]): an action started in a
    /// phase (its index in the record, its kind, how its time was settled
    /// or why it is not), ...
    ActionStarted(usize, usize, u16, String),
    /// ... an action completed (index, kind, at that real-time second), ...
    ActionCompleted(usize, u16, f64),
    /// ... and a phase that waits only on actions with a due time.
    OnClock(usize),
    /// A dialogue action's line stage fields (`scene_line_stage_fields_v1`,
    /// only with [`Scenes::line_fields`]): the action's index and what
    /// happened, as a short text the census counts, and the stage set when
    /// one was.
    LineField(usize, String, Option<i16>),
}

/// The scene clock and the caller's assumptions (`quest_run
/// --scene-clock`, `--assume-line-seconds`, `--assume-player-choice`,
/// `--assume-packages-complete`). Off by default: phases wait on every
/// action as before.
#[derive(Debug, Clone, Default)]
pub struct Clock {
    /// `scene_clock_v1`: actions start, are timed and complete.
    pub on: bool,
    /// `scen_timer_action_seconds_v1`: a timer action lasts its seconds.
    pub timers: bool,
    /// `scene_dialogue_line_audio_v1`: a spoken line lasts its audio; the
    /// voice files and topics it is read from.
    pub voice: Option<Rc<voice::Voice>>,
    /// Caller assumption: a line whose audio does not settle its time
    /// lasts this many seconds.
    pub assume_line_seconds: Option<f64>,
    /// Caller assumption: the player picks this choice
    /// ([`PLAYER_CHOICES`]) in player dialogue.
    pub assume_player_choice: Option<usize>,
    /// Caller assumption: a package action completes as soon as it starts.
    pub assume_packages: bool,
    /// Caller assumption: a start-scene action (kind 4) completes as soon
    /// as it starts, and starts the scene its LCEP names
    /// (`scene_start_scene_action_v1`).
    pub assume_start_scenes: bool,
    /// Caller assumption: where several lines of a topic hold, the first
    /// in stored order plays, for its stage fields
    /// (`scene_line_stage_fields_v1` with `quest_run --assume-first-line`).
    pub assume_first_line: bool,
    /// Caller assumptions scoped to one scene or to the scenes of one
    /// quest (`quest_run --assume-player-choice-for SCENE|QUEST N`,
    /// `--assume-first-line-for SCENE|QUEST`), by the record key: the
    /// choice the player picks there (a scene's over its quest's, both over
    /// [`Clock::assume_player_choice`]), and the first of several lines
    /// that may hold plays there.
    pub choice_for: BTreeMap<String, usize>,
    pub first_line_for: BTreeSet<String>,
    /// `pack_location_destination_v1` (`quest_run --package-destinations
    /// N`): a package action whose package names one placed reference as
    /// a Location input puts the action's actor there this many seconds
    /// (the caller's) after the action starts, as an assumed actor move of
    /// the world model (`crate::track`). `None`: no actor moves.
    pub package_arrival: Option<f64>,
    /// `pack_destination_outlives_scene_v1` (`quest_run
    /// --package-moves-outlive-scene`): the actor moves of a scene's
    /// package actions already scheduled stand when a script stops the
    /// scene before they are applied. Off: they are dropped, as before.
    pub package_after_stop: bool,
    /// `scene_action_skipped_start_v1` (`quest_run
    /// --scene-actions-skipped-start`): an action whose start phase the
    /// scene skipped starts with the first later phase it spans that the
    /// scene enters. Off: such an action never starts and holds every
    /// phase it spans, as before.
    pub start_skipped: bool,
    /// `scene_npc_response_choice_v1` (`quest_run --npc-response-lines`): a
    /// response action (kind 5) plays the topic at the index of the choice
    /// the caller assumes the player picks in the scene, spoken by the
    /// action's alias and timed as a line. Off: not timed, as before.
    pub npc_responses: bool,
}
impl Clock {
    /// The choice the caller assumes the player picks in a scene's player
    /// dialogue: the one given for the scene, else for its quest, else the
    /// general one.
    pub fn choice_in(&self, i: &SceneInfo) -> Option<usize> {
        self.choice_for
            .get(&bind::record_key(&i.key))
            .copied()
            .or_else(|| {
                i.quest
                    .as_ref()
                    .and_then(|q| self.choice_for.get(&bind::record_key(q)).copied())
            })
            .or(self.assume_player_choice)
    }
    /// Whether the caller assumes the first of several lines that may hold
    /// plays in this scene (scoped to it or to its quest).
    pub fn first_line_in(&self, i: &SceneInfo) -> bool {
        self.first_line_for.contains(&bind::record_key(&i.key))
            || i.quest
                .as_ref()
                .is_some_and(|q| self.first_line_for.contains(&bind::record_key(q)))
    }
}

/// One started action of a playing scene.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRun {
    pub kind: u16,
    /// Real-time second it started at.
    pub start: f64,
    /// Real-time second it completes at, when settled.
    pub due: Option<f64>,
    pub done: bool,
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
    pub clock: Clock,
    /// Started actions by (scene handle, action index).
    pub actions: BTreeMap<(ObjId, usize), ActionRun>,
    /// Answer condition function 550 from the actions the clock completed
    /// (`cond_fn_550_clock_actions_v1`; `quest_run --cond-550-clock`). Off
    /// by default: no action counts as complete, as before.
    pub action_conditions: bool,
    /// Apply the scene's and its phases' SCQS stage fields
    /// (`record_set_stage_fields_v1`; `quest_run --stage-fields`). Off by
    /// default: no record field sets a stage, as before.
    pub stage_fields: bool,
    /// Apply the TIQS of the line a dialogue action the clock times plays,
    /// when the records leave a single line of its topic
    /// (`scene_line_stage_fields_v1`; `quest_run --line-stage-fields`).
    /// Off by default: no line sets a stage, as before.
    pub line_fields: bool,
    /// Line stage fields due later on the clock, in the order they were
    /// scheduled.
    pub line_events: Vec<LineEvent>,
    /// What the lines the run plays do besides their stage fields
    /// (`crate::line`; `quest_run --line-fragments`, `--line-start-scene`).
    /// Off by default: nothing, as before.
    pub lines: crate::line::Switches,
    /// Line ends due later on the clock (`crate::line`).
    pub line_ends: Vec<crate::line::End>,
    /// Counts of `crate::line`, by text.
    pub line_counts: BTreeMap<String, u64>,
    /// Answer condition function 131 from the scene clock: how far an
    /// action has run (`cond_fn_131_action_percent_v1`; `quest_run
    /// --cond-131-clock`). Off by default: the function is not evaluated,
    /// as before.
    pub action_percent: bool,
    /// A phase with completion conditions ends when they hold, its timed
    /// actions still running (`scene_phase_conditions_end_v1`; `quest_run
    /// --phase-conditions-end`). Off by default: a phase waits for its
    /// actions, as before.
    pub conditions_end: bool,
    /// Seconds at which a scene waiting on its completion conditions is
    /// offered them again, where a function 131 condition reaches its
    /// comparison value on the clock: (scene handle, second).
    pub percent_marks: Vec<(ObjId, f64)>,
    /// Script variables written since the caller last looked, as (the
    /// object the instance is attached to, the variable's lower-case
    /// name), kept only while a caller offers waiting scenes again when a
    /// variable their conditions read is written
    /// (`scene_condition_recheck_variable_v1`; `quest_run
    /// --scene-recheck-variables`). `None` by default: no write is kept,
    /// as before.
    pub variable_writes: Option<BTreeSet<(ObjId, String)>>,
    /// A line's begin and end delivered to the scripts its record attaches
    /// of its own (`dialogue_line_script_events_v1`; `quest_run
    /// --line-script-events`). Off by default: they receive nothing, as
    /// before.
    pub line_scripts: bool,
    /// The lines (record keys) whose end the run delivered to their own
    /// scripts: a later begin of one is told it has been said.
    pub line_scripts_said: BTreeSet<String>,
}

/// A line's TIQS half due at a second of the real-time clock
/// (`scene_line_stage_fields_v1`): set by the scene's next
/// [`Step::LineFields`] job once the clock reaches it.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEvent {
    pub scene: ObjId,
    pub due: f64,
    /// The line's owning quest and the stage its field names.
    pub quest: FormKey,
    pub stage: i16,
    /// The line, as a record key, and whether this is its end half.
    pub line: String,
    pub end: bool,
    /// The action's index in the scene record.
    pub action: usize,
    /// Whether a [`Step::LineFields`] job is queued for it.
    pub queued: bool,
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

pub(crate) fn groups(store: &Store, w: &mut World) -> Rc<Groups> {
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
    clear_actions(w, o, false);
    w.scenes.log.push((
        o,
        if with_quest {
            Change::StartedWithQuest
        } else {
            Change::Started
        },
    ));
    field_stage(h, w, i, i.stage_fields[0]);
    h.using(rules::SCENE_PHASES);
    if let Some(f) = &i.begin {
        queue(w, o, Step::Fragment(f.clone(), "begin".into()));
    }
    queue(w, o, Step::Enter);
    Ok(())
}

/// Starts a scene as [`start`] does, offering `phase` first when one is
/// given (`dialogue_line_start_scene_v1`).
pub(crate) fn start_at(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &Rc<SceneInfo>,
    phase: Option<usize>,
) -> Result<(), &'static str> {
    if let Some(p) = phase {
        if p >= i.phases.len() {
            return Err("the stored phase is past the scene's last phase");
        }
    }
    start(h, w, o, i, false)?;
    if let Some(p) = phase {
        state(w, i).next = p;
    }
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
        clear_actions(w, o, false);
        w.scenes.log.push((o, Change::Ended));
        field_stage(h, w, i, i.stage_fields[1]);
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
                field_stage(h, w, i, phase.stage_fields[0]);
                if w.scenes.clock.on {
                    start_actions(h, w, o, i, p);
                }
                if let Some(f) = &phase.begin {
                    queue(w, o, Step::Fragment(f.clone(), format!("phase {p} begin")));
                }
                queue(w, o, Step::Complete);
            }
        }
        return;
    }
    if w.scenes.clock.on {
        // Every action spanning the phase holds it until it completes
        // (`scene_clock_v1`); one without a due time waits as before.
        let mut untimed: Option<u16> = None;
        let mut timed = false;
        for (j, a) in i.actions.iter().enumerate() {
            if !(a.start <= p && p <= a.end) {
                continue;
            }
            match w.scenes.actions.get(&(o, j)) {
                Some(r) if r.done => {}
                Some(r) if r.due.is_some() => timed = true,
                _ => untimed = Some(untimed.map_or(a.kind, |k| k.min(a.kind))),
            }
        }
        if let Some(k) = untimed {
            wait(
                w,
                format!("an action runs in the phase: {}", action_kind(k)),
            );
            return;
        }
        if timed {
            // With `scene_phase_conditions_end_v1`, completion conditions
            // that hold end the phase while its timed actions run.
            let early = w.scenes.conditions_end
                && !phase.completion.is_empty()
                && matches!(eval(h, w, i, &phase.completion), Truth::True);
            if !early {
                if w.scenes.conditions_end && !phase.completion.is_empty() {
                    percent_marks(h, w, o, i, &phase.completion);
                }
                w.scenes.log.push((o, Change::OnClock(p)));
                return;
            }
            h.using(rules::SCENE_CONDITIONS_END);
        }
    } else if let Some(k) = phase.actions.iter().min() {
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
            field_stage(h, w, i, phase.stage_fields[1]);
            if let Some(f) = &phase.end {
                queue(w, o, Step::Fragment(f.clone(), format!("phase {p} end")));
            }
            queue(w, o, Step::Enter);
        }
        Truth::False => {
            percent_marks(h, w, o, i, &phase.completion);
            distance_marks(h, w, o, i, &phase.completion);
            wait(w, "completion conditions false (waits on the world)".into())
        }
        Truth::Undetermined(why) => wait(
            w,
            format!(
                "completion conditions undetermined (function {})",
                functions(&why)
            ),
        ),
    }
}

/// With `cond_fn_131_action_percent_v1`: the seconds at which each
/// function 131 condition of a list, naming a running action the clock
/// times, reaches its comparison value; each becomes a mark at which the
/// scene `o` is offered its completion again.
fn percent_marks(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &SceneInfo,
    list: &[Condition],
) {
    if !(w.scenes.clock.on && w.scenes.action_percent) {
        return;
    }
    let ctx = Context {
        order: h.data.store.order(),
        plugin: i.plugin,
    };
    let now = w.now[TimerClock::Real as usize];
    for c in list {
        let c = &c.ctda;
        if c.function != cond::ACTION_PERCENT {
            continue;
        }
        let fo4_plugin::condition::Comparison::Float(v) = c.comparison() else {
            continue;
        };
        let Ok(Some(scene)) = fo4_plugin::condition::form(ctx, c.param1, &i.file, 0) else {
            continue;
        };
        let key = bind::record_key(&scene);
        let Some(so) = w
            .objects
            .iter()
            .position(|x| x.key.as_deref() == Some(&key))
        else {
            continue;
        };
        let Some(Some(si)) = w.scenes.infos.get(&key).cloned() else {
            continue;
        };
        for (j, a) in si.actions.iter().enumerate() {
            if a.index != c.param2 {
                continue;
            }
            let Some(r) = w.scenes.actions.get(&(so, j)) else {
                continue;
            };
            let (false, Some(d)) = (r.done, r.due) else {
                continue;
            };
            let t = r.start + f64::from(v.clamp(0.0, 100.0)) / 100.0 * (d - r.start);
            if t > now && !w.scenes.percent_marks.contains(&(o, t)) {
                w.scenes.percent_marks.push((o, t));
            }
        }
    }
}

/// With `cond_fn_1_track_distance_v1`: the first second after now at
/// which the distance of each function 1 condition of a list reaches its
/// comparison value on the world model (the player's track and the moves
/// known now; `crate::track::distance_first_time`), plus a microsecond so
/// that the comparison holds there; each becomes a mark at which the
/// scene `o` is offered its completion again (the marks of function 131,
/// [`Scenes::percent_marks`]).
fn distance_marks(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &SceneInfo,
    list: &[Condition],
) {
    if !(w.scenes.clock.on && h.cond.distance && w.place.on) {
        return;
    }
    let ctx = Context {
        order: h.data.store.order(),
        plugin: i.plugin,
    };
    let (store, max) = (h.data.store, h.data.max_memory);
    let now = w.now[TimerClock::Real as usize];
    for c in list {
        let c = &c.ctda;
        if c.function != cond::DISTANCE {
            continue;
        }
        let fo4_plugin::condition::Comparison::Float(v) = c.comparison() else {
            continue;
        };
        let less = match c.operator() {
            4 | 5 => true,
            2 | 3 => false,
            _ => continue,
        };
        let mut used = BTreeSet::new();
        let parties = {
            let mut facts = RunFacts::of(h, w, None, i.quest.clone());
            cond::distance_parties(c, ctx, &i.file, &mut facts, &mut used)
        };
        let Ok((a, b)) = parties else { continue };
        let mut obj = |p: &cond::Party| -> Option<ObjId> {
            match p {
                cond::Party::Player => Some(w.player("actor")),
                cond::Party::Record(k) => {
                    record_object(w, store, k, "objectreference", &Budget::new(max)).ok()
                }
            }
        };
        let (Some(oa), Some(ob)) = (obj(&a), obj(&b)) else {
            continue;
        };
        if let Ok(Some(t)) =
            crate::track::distance_first_time(store, max, w, oa, ob, f64::from(v), less, now)
        {
            let t = t + 1e-6;
            if t > now && !w.scenes.percent_marks.contains(&(o, t)) {
                w.scenes.percent_marks.push((o, t));
                w.place
                    .count("distance conditions	marks at which a scene is offered again");
            }
        }
    }
}

/// Ends a scene's running actions; the actor moves of its package actions
/// not yet applied end with it, or stand with `keep_moves`
/// (`pack_destination_outlives_scene_v1`). Returns how many such moves
/// were pending.
fn clear_actions(w: &mut World, o: ObjId, keep_moves: bool) -> usize {
    w.scenes.actions.retain(|(s, _), _| *s != o);
    w.scenes.percent_marks.retain(|(s, _)| *s != o);
    w.scenes.line_events.retain(|e| e.scene != o);
    w.scenes.line_ends.retain(|e| e.owner != o);
    // The actor moves of the scene's package actions not yet applied end
    // with the scene (`pack_location_destination_v1`).
    let pending: Vec<crate::track::ActorMove> = w
        .place
        .package_moves
        .iter()
        .filter(|(s, _)| *s == o)
        .map(|(_, m)| m.clone())
        .collect();
    w.place.package_moves.retain(|(s, _)| *s != o);
    if keep_moves {
        // They stand: still in the world model's moves, no longer the
        // scene's.
        let cursor = w.place.actor_cursor;
        let kept = w.place.actor_moves[cursor.min(w.place.actor_moves.len())..]
            .iter()
            .filter(|m| pending.contains(m))
            .count();
        for _ in 0..kept {
            w.place.count(
                "package destinations\tactor moves kept: the scene stopped first (--package-moves-outlive-scene)",
            );
        }
        return kept;
    }
    let cursor = w.place.actor_cursor;
    let before = w.place.actor_moves.len();
    let mut i = cursor;
    while i < w.place.actor_moves.len() {
        if pending.contains(&w.place.actor_moves[i]) {
            w.place.actor_moves.remove(i);
        } else {
            i += 1;
        }
    }
    let dropped = before - w.place.actor_moves.len();
    for _ in 0..dropped {
        w.place
            .count("package destinations\tactor moves dropped: the scene stopped first");
    }
    dropped
}

/// The seconds of one line of a topic, or why they are not settled, with
/// the caller's assumption applied when it is given.
fn line_time(
    h: &mut QuestHost<'_>,
    w: &mut World,
    i: &SceneInfo,
    topic: Option<&FormKey>,
    speaker: &Speaker,
) -> (Option<f64>, String) {
    let clock = w.scenes.clock.clone();
    let audio = match (&clock.voice, topic) {
        (None, _) => LineTime::Undetermined("line audio off".into()),
        (Some(_), None) => LineTime::Undetermined("the action names no topic".into()),
        (Some(v), Some(t)) => {
            let store = h.data.store;
            let b = Budget::new(h.data.max_memory);
            let mut used = BTreeSet::new();
            let mut cond_used = BTreeSet::new();
            let quest = i.quest.clone();
            let mut eval = |info: &fo4_plugin::info::Info, plugin: usize, file: &str| {
                let ctx = Context {
                    order: store.order(),
                    plugin,
                };
                let mut facts = RunFacts::of(h, w, None, quest.clone());
                cond::eval_list_with(
                    &info.conditions,
                    ctx,
                    file,
                    &mut facts,
                    Combine::OrGroups,
                    &mut cond_used,
                )
            };
            let t = voice::line_seconds(store, v, t, speaker, &mut eval, &mut used, &b);
            for u in used.into_iter().chain(cond_used) {
                h.using(u);
            }
            t
        }
    };
    match (audio, clock.assume_line_seconds) {
        (LineTime::Seconds(x), _) => (Some(x), "seconds from its audio".into()),
        (LineTime::Undetermined(why), Some(x)) => (
            Some(x),
            format!("assumed seconds from the command line ({why})"),
        ),
        (LineTime::Undetermined(why), None) => (None, format!("undetermined: {why}")),
    }
}

/// How long an action lasts once started, or why that is not settled
/// (the second part: a short text the census counts).
pub(crate) fn action_time(
    h: &mut QuestHost<'_>,
    w: &mut World,
    i: &SceneInfo,
    a: &ActionInfo,
) -> (Option<f64>, String) {
    let clock = w.scenes.clock.clone();
    match a.kind {
        TIMER if clock.timers => {
            h.using(rules::TIMER_ACTION);
            match (a.seconds, a.low, a.flags.unwrap_or(0)) {
                (_, _, f) if f != 0 => (
                    None,
                    "timer: undetermined (a flag bit is set; no bit is read)".into(),
                ),
                (_, Some(_), _) => (
                    None,
                    "timer: undetermined (a second value, TNAM, is stored)".into(),
                ),
                (Some(x), None, _) if x.is_finite() && x >= 0.0 => {
                    (Some(f64::from(x)), "timer: seconds from the record".into())
                }
                _ => (None, "timer: undetermined (no seconds stored)".into()),
            }
        }
        LINE => {
            let speaker = match &i.quest {
                Some(q) => {
                    let b = Budget::new(h.data.max_memory);
                    voice::speaker(h.data.store, q, a.alias, &b)
                }
                None => Speaker::Unknown("the scene has no quest".into()),
            };
            let (t, how) = line_time(h, w, i, a.topic.as_ref(), &speaker);
            (t, format!("line: {how}"))
        }
        PACKAGE if clock.assume_packages => (
            Some(0.0),
            "package: assumed complete from the command line".into(),
        ),
        START_SCENE if clock.assume_start_scenes => (
            Some(0.0),
            "start-scene: assumed complete from the command line".into(),
        ),
        PLAYER_DIALOGUE => match clock.choice_in(i) {
            None => (None, "player dialogue: waits on the player".into()),
            Some(n) => {
                h.using(rules::PLAYER_CHOICE);
                let Some((player, answer)) = a.choices.get(n).cloned() else {
                    return (
                        None,
                        "player dialogue: the assumed choice is not stored".into(),
                    );
                };
                let me = Speaker::Unknown("the player's voice type is not in the records".into());
                let (p, ph) = line_time(h, w, i, player.as_ref(), &me);
                let speaker = match &i.quest {
                    Some(q) => {
                        let b = Budget::new(h.data.max_memory);
                        voice::speaker(h.data.store, q, a.alias, &b)
                    }
                    None => Speaker::Unknown("the scene has no quest".into()),
                };
                let (r, rh) = line_time(h, w, i, answer.as_ref(), &speaker);
                let how = format!(
                    "player dialogue: assumed choice {n} from the command line; player line {ph}; answer {rh}"
                );
                match (p, r) {
                    (Some(p), Some(r)) => (Some(p + r), how),
                    _ => (None, how),
                }
            }
        },
        NPC_RESPONSE if clock.npc_responses => match clock.choice_in(i) {
            None => (None, "response: waits on the player's choice".into()),
            Some(n) => {
                h.using(rules::NPC_RESPONSE_CHOICE);
                let Some(topic) = a.responses.get(n).cloned() else {
                    return (None, "response: the assumed choice is not stored".into());
                };
                let speaker = match &i.quest {
                    Some(q) => {
                        let b = Budget::new(h.data.max_memory);
                        voice::speaker(h.data.store, q, a.alias, &b)
                    }
                    None => Speaker::Unknown("the scene has no quest".into()),
                };
                let (t, how) = line_time(h, w, i, topic.as_ref(), &speaker);
                (
                    t,
                    format!("response: assumed choice {n} from the command line; line {how}"),
                )
            }
        },
        k => (None, format!("{}: not timed", action_kind(k))),
    }
}

/// The truth of every line of a topic in the run's state.
fn topic_truths(
    h: &mut QuestHost<'_>,
    w: &mut World,
    i: &SceneInfo,
    topic: &FormKey,
) -> Result<Vec<(FormKey, voice::LineTruth)>, String> {
    let Some(v) = w.scenes.clock.voice.clone() else {
        return Err("the topics are not read (line audio off)".into());
    };
    let store = h.data.store;
    let b = Budget::new(h.data.max_memory);
    let mut cond_used = BTreeSet::new();
    let quest = i.quest.clone();
    let mut eval = |info: &fo4_plugin::info::Info, plugin: usize, file: &str| {
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let mut facts = RunFacts::of(h, w, None, quest.clone());
        cond::eval_list_with(
            &info.conditions,
            ctx,
            file,
            &mut facts,
            Combine::OrGroups,
            &mut cond_used,
        )
    };
    let t = voice::line_truths(store, &v, topic, &mut eval, &b);
    for u in cond_used {
        h.using(u);
    }
    t
}

/// The lines a dialogue action plays, each with the second it begins at
/// after the action starts, the second it ends at when settled and whether
/// the player speaks it: the topic's line for kind 0; under an assumed
/// choice, the player's line then the answer for kind 3
/// (`scene_player_dialogue_choice_v1`).
pub(crate) fn line_segments(
    h: &mut QuestHost<'_>,
    w: &mut World,
    i: &SceneInfo,
    a: &ActionInfo,
) -> Vec<(Option<FormKey>, f64, Option<f64>, bool)> {
    let speaker = |h: &mut QuestHost<'_>| match &i.quest {
        Some(q) => {
            let b = Budget::new(h.data.max_memory);
            voice::speaker(h.data.store, q, a.alias, &b)
        }
        None => Speaker::Unknown("the scene has no quest".into()),
    };
    match a.kind {
        LINE => {
            let sp = speaker(h);
            let (t, _) = line_time(h, w, i, a.topic.as_ref(), &sp);
            vec![(a.topic.clone(), 0.0, t, false)]
        }
        PLAYER_DIALOGUE => {
            let Some(n) = w.scenes.clock.choice_in(i) else {
                return Vec::new();
            };
            let Some((player, answer)) = a.choices.get(n).cloned() else {
                return Vec::new();
            };
            let me = Speaker::Unknown("the player's voice type is not in the records".into());
            let (p, _) = line_time(h, w, i, player.as_ref(), &me);
            let mut out = vec![(player, 0.0, p, true)];
            if let Some(p) = p {
                let sp = speaker(h);
                let (r, _) = line_time(h, w, i, answer.as_ref(), &sp);
                out.push((answer, p, r.map(|r| p + r), false));
            }
            out
        }
        NPC_RESPONSE if w.scenes.clock.npc_responses => {
            let Some(n) = w.scenes.clock.choice_in(i) else {
                return Vec::new();
            };
            let Some(topic) = a.responses.get(n).cloned() else {
                return Vec::new();
            };
            let sp = speaker(h);
            let (t, _) = line_time(h, w, i, topic.as_ref(), &sp);
            vec![(topic, 0.0, t, false)]
        }
        _ => Vec::new(),
    }
}

/// The TIQS of the line each segment of a dialogue action plays, when the
/// records leave a single line of its topic or the caller assumes the
/// first (`scene_line_stage_fields_v1`): a half due at the action's start
/// is set at once, a later one is scheduled on the clock
/// ([`LineEvent`]); what stops it is logged.
fn line_fields(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &SceneInfo,
    j: usize,
    a: &ActionInfo,
    now: f64,
) {
    let log =
        |w: &mut World, what: String| w.scenes.log.push((o, Change::LineField(j, what, None)));
    for (topic, begin, end, by_player) in line_segments(h, w, i, a) {
        let Some(topic) = topic else {
            log(w, "no line: the action names no topic".into());
            continue;
        };
        let truths = match topic_truths(h, w, i, &topic) {
            Ok(t) => t,
            Err(why) => {
                log(w, format!("no line: {why}"));
                continue;
            }
        };
        let holds = truths
            .iter()
            .filter(|t| t.1 == voice::LineTruth::Holds)
            .count();
        let open: BTreeSet<u16> = truths
            .iter()
            .filter_map(|t| match &t.1 {
                voice::LineTruth::Undetermined(f) => Some(f.iter().copied()),
                _ => None,
            })
            .flatten()
            .collect();
        let first = truths.iter().find(|t| t.1 != voice::LineTruth::Fails);
        let line = match first {
            Some((k, voice::LineTruth::Holds)) if holds == 1 && open.is_empty() => {
                log(
                    w,
                    "line determined: the only line of its topic that holds".into(),
                );
                k.clone()
            }
            Some((k, voice::LineTruth::Holds))
                if !w.scenes.clock.assume_first_line && w.scenes.clock.first_line_in(i) =>
            {
                log(
                    w,
                    "line assumed: the first of several lines that may hold (--assume-first-line-for)"
                        .into(),
                );
                k.clone()
            }
            Some((k, voice::LineTruth::Holds)) if w.scenes.clock.assume_first_line => {
                log(
                    w,
                    "line assumed: the first of several lines that may hold (--assume-first-line)"
                        .into(),
                );
                k.clone()
            }
            _ if holds > 1 && open.is_empty() => {
                log(
                    w,
                    "line undetermined: several lines of its topic hold".into(),
                );
                continue;
            }
            _ if !open.is_empty() => {
                let f: Vec<String> = open.iter().map(|n| n.to_string()).collect();
                log(
                    w,
                    format!(
                        "line undetermined: whether a line holds is undetermined (function {})",
                        f.join(", ")
                    ),
                );
                continue;
            }
            _ => {
                log(w, "no line: no line of its topic holds".into());
                continue;
            }
        };
        // What the line does besides its stage fields (`crate::line`).
        if w.scenes.lines.fragments || w.scenes.lines.start_scene {
            let speaker = if by_player {
                Some(w.player("actor"))
            } else {
                i.quest.clone().and_then(|q| {
                    let k = alias_reference(h, w, &q, a.alias)?;
                    let b = Budget::new(h.data.max_memory);
                    record_object(w, h.data.store, &k, "objectreference", &b).ok()
                })
            };
            crate::line::play(
                h,
                w,
                o,
                &line,
                speaker,
                begin <= 0.0,
                now + begin,
                end.map(|e| now + e),
            );
        }
        if !w.scenes.line_fields {
            continue;
        }
        let b = Budget::new(h.data.max_memory);
        let tiqs = h.data.store.raw(&line, &b).ok().flatten().and_then(|r| {
            r.subrecords
                .iter()
                .find(|x| x.signature == *b"TIQS")
                .and_then(|x| stage_pair(r.data(x)))
        });
        let Some([b0, b1]) = tiqs else {
            log(w, "the line stores no TIQS".into());
            continue;
        };
        if b0 < 0 && b1 < 0 {
            log(w, "the line's TIQS names no stage".into());
            continue;
        }
        let key = bind::record_key(&line);
        let Some(q) = groups(h.data.store, w).info_owner.get(&key).cloned() else {
            log(w, "the line has no owning quest".into());
            continue;
        };
        h.using(rules::LINE_STAGE_FIELDS);
        for (n, at, is_end) in [(b0, Some(begin), false), (b1, end, true)] {
            if n < 0 {
                continue;
            }
            let half = if is_end { "end" } else { "begin" };
            match at {
                Some(x) if x <= 0.0 => {
                    w.scenes.log.push((
                        o,
                        Change::LineField(
                            j,
                            format!("{half} half set as the line {half}s"),
                            Some(n),
                        ),
                    ));
                    set_field_stage(h, w, &q, n);
                }
                Some(x) => {
                    log(w, format!("{half} half scheduled on the clock"));
                    w.scenes.line_events.push(LineEvent {
                        scene: o,
                        due: now + x,
                        quest: q.clone(),
                        stage: n,
                        line: key.clone(),
                        end: is_end,
                        action: j,
                        queued: false,
                    });
                }
                None => log(
                    w,
                    format!("{half} half not applied: the line's time is not settled"),
                ),
            }
        }
    }
}

/// Sets the line stage fields of a scene the clock has reached
/// ([`Step::LineFields`], `scene_line_stage_fields_v1`), in due order.
fn apply_line_events(h: &mut QuestHost<'_>, w: &mut World, o: ObjId) {
    let now = w.now[TimerClock::Real as usize];
    let mut due: Vec<LineEvent> = Vec::new();
    w.scenes.line_events.retain(|e| {
        if e.scene == o && e.due <= now {
            due.push(e.clone());
            false
        } else {
            true
        }
    });
    due.sort_by(|a, b| {
        a.due
            .partial_cmp(&b.due)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for e in due {
        let half = if e.end { "end" } else { "begin" };
        h.using(rules::LINE_STAGE_FIELDS);
        w.scenes.log.push((
            o,
            Change::LineField(
                e.action,
                format!("{half} half set on the clock"),
                Some(e.stage),
            ),
        ));
        set_field_stage(h, w, &e.quest, e.stage);
    }
}

/// The placed reference a scene's alias holds in the run: the fill a run
/// set, else the alias's fill from the records (`quest::Quests::fill`).
fn alias_reference(
    h: &QuestHost<'_>,
    w: &mut World,
    quest: &FormKey,
    alias: i32,
) -> Option<FormKey> {
    let key = bind::record_key(quest);
    if let Some(Val::Object(o)) = w.quests.states.get(&key).and_then(|s| s.fills.get(&alias)) {
        return w.objects[*o].key.as_deref().and_then(key_record);
    }
    let info = quest::info_by_key(h.data.store, h.data.max_memory, w, quest)?;
    match w.quests.fill(&info, u32::try_from(alias).ok()?) {
        Ok(Some(quest::Fill::Record(k))) => Some(k),
        _ => None,
    }
}

/// The destination of a package action (`pack_location_destination_v1`):
/// the one placed reference (REFR or ACHR) the package's Location inputs
/// name; `Err` with why there is none.
fn package_destination(
    store: &Store,
    max: u64,
    package: &FormKey,
) -> Result<FormKey, &'static str> {
    let b = Budget::new(max);
    let (r, plugin) = store
        .raw_with_plugin(package, &b)
        .ok()
        .flatten()
        .ok_or("the package is not a winning record")?;
    let ctx = Context {
        order: store.order(),
        plugin,
    };
    let p = fo4_plugin::pack::Package::read(&r, ctx, &b)
        .map_err(|_| "the package could not be read")?;
    let mut found: Vec<FormKey> = p
        .locations()
        .filter_map(|(_, l)| l.word.key.clone())
        .filter(|k| {
            matches!(
                store.signature(k).as_ref().map(|s| &s[..]),
                Some(b"REFR" | b"ACHR")
            )
        })
        .collect();
    found.dedup();
    match found.len() {
        0 => Err("no Location input names a placed reference"),
        1 => Ok(found.remove(0)),
        _ => Err("several Location inputs name placed references"),
    }
}

/// Under `pack_location_destination_v1`, the actor move a package action
/// brings, scheduled on the world model; counted either way.
fn package_move(
    h: &mut QuestHost<'_>,
    w: &mut World,
    o: ObjId,
    i: &SceneInfo,
    a: &ActionInfo,
    now: f64,
) {
    let Some(n) = w.scenes.clock.package_arrival else {
        return;
    };
    let why = |w: &mut World, x: &str| {
        w.place
            .count(format!("package destinations\tno actor move\t{x}"));
    };
    if !w.place.on || !w.place.sw.actors {
        return why(w, "the world model's actor positions are off");
    }
    let Some(pk) = a.package.clone() else {
        return why(w, "the action names no package");
    };
    let (store, max) = (h.data.store, h.data.max_memory);
    let dest = match package_destination(store, max, &pk) {
        Ok(d) => d,
        Err(e) => return why(w, e),
    };
    let Some(q) = i.quest.clone() else {
        return why(w, "the scene has no quest");
    };
    let Some(actor) = alias_reference(h, w, &q, a.alias) else {
        return why(w, "the action's alias holds no placed reference in the run");
    };
    if store.signature(&actor) != Some(*b"ACHR") {
        return why(w, "the action's alias holds no placed actor");
    }
    let Ok((space, pos)) = crate::track::stored_position(store, max, &dest) else {
        return why(w, "the destination's position is not stored");
    };
    h.using(rules::PACKAGE_DESTINATION);
    w.place.count("package destinations\tactor moves scheduled");
    let m = crate::track::ActorMove {
        t: now + n,
        actor,
        space,
        pos,
    };
    w.place.package_moves.push((o, m.clone()));
    crate::track::schedule_move(w, m);
}

/// Starts the actions whose start phase is `p` (`scene_clock_v1`).
fn start_actions(h: &mut QuestHost<'_>, w: &mut World, o: ObjId, i: &SceneInfo, p: usize) {
    h.using(rules::SCENE_CLOCK);
    for (j, a) in i.actions.iter().enumerate() {
        let late = w.scenes.clock.start_skipped
            && a.start < p
            && p <= a.end
            && !w.scenes.actions.contains_key(&(o, j));
        if a.start != p && !late {
            continue;
        }
        if late {
            h.using(rules::SCENE_ACTION_SKIPPED_START);
        }
        let (t, how) = action_time(h, w, i, a);
        let now = w.now[TimerClock::Real as usize];
        let done = t == Some(0.0);
        w.scenes
            .log
            .push((o, Change::ActionStarted(p, j, a.kind, how)));
        w.scenes.actions.insert(
            (o, j),
            ActionRun {
                kind: a.kind,
                start: now,
                due: t.map(|x| now + x),
                done,
            },
        );
        let lines_on = w.scenes.lines.fragments || w.scenes.lines.start_scene;
        let response = a.kind == NPC_RESPONSE && w.scenes.clock.npc_responses;
        if (w.scenes.line_fields || lines_on)
            && (matches!(a.kind, LINE | PLAYER_DIALOGUE) || response)
        {
            line_fields(h, w, o, i, j, a, now);
        }
        if a.kind == PACKAGE {
            package_move(h, w, o, i, a, now);
        }
        if done {
            w.scenes
                .log
                .push((o, Change::ActionCompleted(j, a.kind, now)));
        }
        // Under the caller's assumption a start-scene action starts the
        // scene its LCEP names (`scene_start_scene_action_v1`).
        if a.kind == START_SCENE && w.scenes.clock.assume_start_scenes {
            h.using(rules::START_SCENE_ACTION);
            let b = Budget::new(h.data.max_memory);
            let Some(k) = a.scene.clone() else { continue };
            let Ok(so) = record_object(w, h.data.store, &k, "scene", &b) else {
                continue;
            };
            if let Some(si) = info(h.data.store, h.data.max_memory, w, so) {
                let _ = start(h, w, so, &si, false);
            }
        }
    }
}

/// Offers again the phase of every scene stopped on its start or
/// completion conditions (`scene_condition_recheck_v1`): a completion step
/// for a scene waiting on its completion conditions, an entry step for one
/// waiting on its start conditions. A caller calls it after the run's
/// quest state changed (`quest_run --scene-recheck`); without it a scene
/// stopped on conditions stays stopped, as before. Returns the scenes
/// offered again.
pub fn recheck(w: &mut World) -> usize {
    let mut last: BTreeMap<ObjId, Option<Step>> = BTreeMap::new();
    for (o, ch) in &w.scenes.log {
        let step = match ch {
            Change::Waiting(_, why) if why.starts_with("completion conditions") => {
                Some(Step::Complete)
            }
            Change::Waiting(_, why) if why.starts_with("start conditions") => Some(Step::Enter),
            _ => None,
        };
        last.insert(*o, step);
    }
    let mut n = 0;
    for (o, step) in last {
        if let Some(step) = step {
            queue(w, o, step);
            n += 1;
        }
    }
    n
}

/// Offers again the phase of every scene stopped on its start or
/// completion conditions when one of those conditions reads a quest
/// script variable (function 629) that `written` names: the quest the
/// condition's first parameter names, the variable its CIS2 names
/// (`scene_condition_recheck_variable_v1`). A caller calls it with the
/// writes kept in [`Scenes::variable_writes`] since its last call
/// (`quest_run --scene-recheck-variables`). Returns the scenes offered
/// again.
pub fn recheck_variables(
    store: &Store,
    w: &mut World,
    written: &BTreeSet<(ObjId, String)>,
) -> usize {
    if written.is_empty() {
        return 0;
    }
    // The written variables by the record key of their object.
    let mut by_key: BTreeSet<(String, &str)> = BTreeSet::new();
    for (o, n) in written {
        if let Some(k) = w.objects.get(*o).and_then(|x| x.key.clone()) {
            by_key.insert((k, n.as_str()));
        }
    }
    let mut last: BTreeMap<ObjId, Option<(usize, Step)>> = BTreeMap::new();
    for (o, ch) in &w.scenes.log {
        let step = match ch {
            Change::Waiting(p, why) if why.starts_with("completion conditions") => {
                Some((*p, Step::Complete))
            }
            Change::Waiting(p, why) if why.starts_with("start conditions") => {
                Some((*p, Step::Enter))
            }
            _ => None,
        };
        last.insert(*o, step);
    }
    let mut n = 0;
    for (o, step) in last {
        let Some((p, step)) = step else { continue };
        let Some(key) = w.objects.get(o).and_then(|x| x.key.clone()) else {
            continue;
        };
        let Some(Some(i)) = w.scenes.infos.get(&key).cloned() else {
            continue;
        };
        let Some(phase) = i.phases.get(p) else {
            continue;
        };
        let list = if step == Step::Complete {
            &phase.completion
        } else {
            &phase.start
        };
        let ctx = Context {
            order: store.order(),
            plugin: i.plugin,
        };
        let reads = list.iter().any(|c| {
            if c.ctda.function != cond::QUEST_VARIABLE {
                return false;
            }
            let Some(name) = c.cis2.as_deref() else {
                return false;
            };
            let Ok(Some(q)) = fo4_plugin::condition::form(ctx, c.ctda.param1, &i.file, 0) else {
                return false;
            };
            by_key.contains(&(bind::record_key(&q), fo4_pex::index::lower(name).as_str()))
        });
        if reads {
            queue(w, o, step);
            n += 1;
        }
    }
    n
}

/// Real seconds from now until the next started action is due, when one
/// is (`scene_clock_v1`).
pub fn next_due(w: &World) -> Option<f64> {
    let now = w.now[TimerClock::Real as usize];
    w.scenes
        .actions
        .values()
        .filter(|r| !r.done)
        .filter_map(|r| r.due)
        .chain(
            w.scenes
                .line_events
                .iter()
                .filter(|e| !e.queued)
                .map(|e| e.due),
        )
        .chain(w.scenes.percent_marks.iter().map(|(_, t)| *t))
        .chain(crate::line::next_due(w))
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|d| (d - now).max(0.0))
}

/// Advances the real-time clock to the next due action (the game-time
/// clock by `scale` real seconds per game hour over 3600, when the caller
/// knows the time scale), completes every action due by then and queues
/// the completion step of each scene that had one. Returns the real-time
/// second reached, or `None` when no started action has a due time. A
/// caller assumption like firing timers: the caller says how far.
pub fn advance_clock(w: &mut World, scale: Option<f64>) -> Option<f64> {
    let dt = next_due(w)?;
    let now = w.now[TimerClock::Real as usize];
    let t = now + dt;
    w.now[TimerClock::Real as usize] = t;
    if let Some(s) = scale {
        w.now[TimerClock::GameTime as usize] += dt * s / 3600.0;
    }
    // Line stage fields reached first (`scene_line_stage_fields_v1`), one
    // job per scene, ahead of the scene's completion step.
    let mut fields = Vec::new();
    for e in w.scenes.line_events.iter_mut() {
        if !e.queued && e.due <= t {
            e.queued = true;
            if !fields.contains(&e.scene) {
                fields.push(e.scene);
            }
        }
    }
    for e in w.scenes.line_ends.iter_mut() {
        if !e.queued && e.due <= t {
            e.queued = true;
            if !fields.contains(&e.owner) {
                fields.push(e.owner);
            }
        }
    }
    for o in fields {
        queue(w, o, Step::LineFields);
    }
    let mut scenes = Vec::new();
    let mut done = Vec::new();
    for (&(o, j), r) in w.scenes.actions.iter_mut() {
        if !r.done && r.due.is_some_and(|d| d <= t) {
            r.done = true;
            done.push((o, Change::ActionCompleted(j, r.kind, t)));
            if !scenes.contains(&o) {
                scenes.push(o);
            }
        }
    }
    w.scenes.log.extend(done);
    // Function 131 marks reached (`cond_fn_131_action_percent_v1`).
    let marks = std::mem::take(&mut w.scenes.percent_marks);
    for (o, m) in marks {
        if m <= t {
            if !scenes.contains(&o) {
                scenes.push(o);
            }
        } else {
            w.scenes.percent_marks.push((o, m));
        }
    }
    for o in scenes {
        queue(w, o, Step::Complete);
    }
    Some(t)
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
    // Line ends whose owner is a line's own handle, outside scenes
    // (`crate::line`).
    if c.script == "scene"
        && c.name == STEP
        && matches!(c.args.first(), Some(Val::Str(_)))
        && !w.scenes.line_ends.is_empty()
        && info(h.data.store, h.data.max_memory, w, o).is_none()
    {
        crate::line::apply_due(h, w);
        return Reply::Value(Val::None);
    }
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
            let keep = w.scenes.clock.package_after_stop;
            if clear_actions(w, o, keep) > 0 && keep {
                h.using(rules::PACKAGE_MOVE_AFTER_STOP);
            }
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
        ("scene", n) if n == MARKS => {
            // The phase the scene waits in: its completion conditions'
            // function 1 marks on the world model as it stands now.
            let s = state(w, &i);
            let (true, p) = (s.playing == Some(true), s.next) else {
                return Reply::Value(Val::Bool(false));
            };
            let Some(phase) = i.phases.get(p).cloned() else {
                return Reply::Value(Val::Bool(false));
            };
            let before = w.scenes.percent_marks.len();
            distance_marks(h, w, o, &i, &phase.completion);
            Reply::Value(Val::Bool(w.scenes.percent_marks.len() > before))
        }
        ("scene", n) if n == STEP => {
            if matches!(c.args.first(), Some(Val::Str(_))) {
                apply_line_events(h, w, o);
                crate::line::apply_due(h, w);
                return Reply::Value(Val::None);
            }
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

/// An instance of the fragment block script of the INFO, PACK, SCEN, PERK
/// or TERM record behind a handle, with the block's property values
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
        .or(v.terminal.as_ref().map(|x| &x.script))
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

/// The synthetic call of a [`Step::LineFields`] job.
pub fn line_fields_call(scene: ObjId) -> NativeCall {
    NativeCall {
        script: "scene".into(),
        name: STEP.into(),
        receiver: Some(Val::Object(scene)),
        instance: None,
        args: vec![Val::Str("line fields".into())],
        param_types: vec!["string".into()],
        return_type: "none".into(),
    }
}

/// The synthetic call of [`recheck_distance_marks`] on one scene.
pub fn marks_call(scene: ObjId) -> NativeCall {
    NativeCall {
        script: "scene".into(),
        name: MARKS.into(),
        receiver: Some(Val::Object(scene)),
        instance: None,
        args: Vec::new(),
        param_types: Vec::new(),
        return_type: "bool".into(),
    }
}

/// The scenes waiting on false completion conditions with a function 1
/// condition and no mark left (`cond_fn_1_track_distance_v1`): when the
/// player's track grew after they began to wait (the play mode knows the
/// track only to its horizon, `spec/play.md`), their marks are computed
/// again on the longer track, as [`distance_marks`] computed them when
/// they began to wait, without offering their completion (no new wait
/// is logged). Returns the scenes that got a mark. The host's rules are
/// left for the caller to take.
pub fn recheck_distance_marks(host: &mut dyn crate::Host, w: &mut World) -> usize {
    let mut last: BTreeMap<ObjId, Option<usize>> = BTreeMap::new();
    for (o, ch) in &w.scenes.log {
        let p = match ch {
            Change::Waiting(p, why) if why.starts_with("completion conditions false") => Some(*p),
            _ => None,
        };
        last.insert(*o, p);
    }
    let mut marked = 0;
    for (o, p) in last {
        let Some(p) = p else { continue };
        if w.scenes.percent_marks.iter().any(|(s, _)| *s == o) {
            continue;
        }
        let Some(key) = w.objects.get(o).and_then(|x| x.key.clone()) else {
            continue;
        };
        let Some(Some(info)) = w.scenes.infos.get(&key).cloned() else {
            continue;
        };
        let distance = info.phases.get(p).is_some_and(|ph| {
            ph.completion
                .iter()
                .any(|x| x.ctda.function == cond::DISTANCE)
        });
        if !distance {
            continue;
        }
        if let Reply::Value(Val::Bool(true)) = host.call(&marks_call(o), w) {
            marked += 1;
        }
    }
    marked
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(t: f64, actor: u32) -> crate::track::ActorMove {
        let key = |id: u32| FormKey {
            plugin: "test.esm".into(),
            object_id: id,
        };
        crate::track::ActorMove {
            t,
            actor: key(actor),
            space: key(0x800),
            pos: [t, 0.0, 0.0],
        }
    }

    /// `pack_destination_outlives_scene_v1`: a scene that stops with its
    /// package actions' actor moves still ahead drops them; with the
    /// reading they stand. Another scene's move and a move already applied
    /// are left alone either way.
    #[test]
    fn a_stopped_scenes_pending_package_moves_stand_only_under_the_reading() {
        for keep in [false, true] {
            let mut w = World::default();
            let (o, other) = (7, 8);
            let (applied, ours, theirs) = (mv(1.0, 0x901), mv(12.0, 0x902), mv(13.0, 0x903));
            w.place.actor_moves = vec![applied.clone(), ours.clone(), theirs.clone()];
            w.place.actor_cursor = 1;
            w.place.package_moves = vec![
                (o, applied.clone()),
                (o, ours.clone()),
                (other, theirs.clone()),
            ];
            let n = clear_actions(&mut w, o, keep);
            assert_eq!(n, 1, "one move of the scene was still ahead");
            let expect = if keep {
                vec![applied.clone(), ours.clone(), theirs.clone()]
            } else {
                vec![applied.clone(), theirs.clone()]
            };
            assert_eq!(w.place.actor_moves, expect);
            // No longer the scene's either way; the other scene keeps its own.
            assert_eq!(w.place.package_moves, vec![(other, theirs.clone())]);
            let k = if keep {
                "package destinations\tactor moves kept: the scene stopped first (--package-moves-outlive-scene)"
            } else {
                "package destinations\tactor moves dropped: the scene stopped first"
            };
            assert_eq!(w.place.counts.get(k), Some(&1));
        }
    }
}
