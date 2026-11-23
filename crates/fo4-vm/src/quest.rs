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
    /// Reference aliases filled from a location alias with a location ref
    /// type (ALFA, ALRT): the reference the forced location's special
    /// reference list (LCSR) names for that type, or why none
    /// (`quest_alias_location_ref_v1`; read only when a run turns
    /// [`Quests::location_fills`] on).
    pub location_refs: BTreeMap<u32, Result<FormKey, &'static str>>,
    /// The same aliases' location, ref type and the references the rows
    /// of that type name, in row order (repeats kept): what the readings
    /// after the list's (`quest_alias_location_ref_distinct_v1`,
    /// `quest_alias_location_ref_placed_v1`) read.
    pub location_rows: BTreeMap<u32, LocationRows>,
    /// The ids of the collection aliases (ALCS).
    pub collections: BTreeSet<u32>,
    /// Reference aliases with a unique actor (ALUA): the NPC_ it names
    /// (`quest_alias_unique_actor_ref_v1`; read only when a run gives
    /// [`Quests::unique`]).
    pub unique_actors: BTreeMap<u32, FormKey>,
    /// DNAM byte 2, the priority (`dialogue_line_priority_v1` orders
    /// lines by it).
    pub priority: Option<u8>,
    /// The VMAD block's fragment script, lower case.
    pub fragment_script: Option<String>,
    /// Fragments naming a stage the record does not hold.
    pub unmapped_fragments: usize,
    /// ENAM: the story event code, as stored.
    pub event: Option<[u8; 4]>,
    /// The conditions after NEXT, which a story manager start evaluates
    /// (`sm_quest_node_start_v1`).
    pub event_conditions: Vec<Condition>,
    /// Reference aliases created at run time (`ALCO`, `ALCA`, `ALCL`), by
    /// alias id (`quest_alias_created_reference_v1`; read only when a run
    /// turns [`Quests::created_refs`] on).
    pub created: BTreeMap<u32, CreatedAlias>,
    /// Reference aliases filled from another quest's alias (`ALEQ`,
    /// `ALEA`): the quest and the alias index, by alias id
    /// (`quest_alias_external_live_v1`; read only when a run turns
    /// [`Quests::external_fills`] on).
    pub external: BTreeMap<u32, (FormKey, i32)>,
}

/// What a reference alias created at run time names: the base record to
/// create (`ALCO`), the alias of the same quest it is created at (`ALCA`
/// low half), the `ALCA` high half and the `ALCL` word, kept as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedAlias {
    pub object: Option<FormKey>,
    pub at: u16,
    pub word: u16,
    pub level: u32,
}

impl QuestInfo {
    /// The reference an alias holds while the quest runs and no run set
    /// its fill: its forced reference (`quest_alias_forced_reference_v1`),
    /// else, with `location` on, the location's special reference
    /// (`quest_alias_location_ref_v1`). `Ok(None)`: neither.
    pub fn fill_of(&self, id: u32, location: bool) -> Result<Option<FormKey>, &'static str> {
        let forced = self
            .reference_aliases
            .get(&id)
            .ok_or("no reference alias of that index")?;
        if let Some(k) = forced {
            return Ok(Some(k.clone()));
        }
        if location {
            if let Some(r) = self.location_refs.get(&id) {
                return r.clone().map(Some);
            }
        }
        Ok(None)
    }
    pub fn stage(&self, n: i32) -> Option<&StageInfo> {
        self.stages.iter().find(|s| i32::from(s.stage) == n)
    }
    pub fn start_game_enabled(&self) -> bool {
        self.flags.is_some_and(|f| f & START_GAME_ENABLED != 0)
    }
    /// Reads a quest's record and its VMAD block; `None` when the key
    /// names no winning QUST.
    pub fn load(store: &Store, k: &FormKey, b: &Budget) -> fo4_core::Result<Option<Self>> {
        Ok(Self::load_with(store, k, b, false)?.map(|x| x.0))
    }
    /// [`QuestInfo::load`]; with `partial` a winning record whose record
    /// flag word has [`PARTIAL_RECORD`] set and that carries no subrecord
    /// but `EDID` and `VMAD` is read from the occurrence before it, its own
    /// VMAD kept when it carries one (`quest_record_partial_previous_v1`).
    /// The flag is whether that reading was applied.
    pub fn load_with(
        store: &Store,
        k: &FormKey,
        b: &Budget,
        partial: bool,
    ) -> fo4_core::Result<Option<(Self, bool)>> {
        if store.signature(k) != Some(*b"QUST") {
            return Ok(None);
        }
        let Some((winner, winner_plugin)) = store.raw_with_plugin(k, b)? else {
            return Ok(None);
        };
        let before = if partial && partial_only_script(&winner) {
            match store.overrides().winner(k).and_then(|w| w.previous) {
                Some(p) => Some(store.raw_occurrence(p as usize, b)?),
                None => None,
            }
        } else {
            None
        };
        let merged = before.is_some();
        let (r, plugin, winner) = match before {
            Some((pr, pp)) => (pr, pp, Some(winner)),
            None => (winner, winner_plugin, None),
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let q = Quest::read(&r, ctx, b)?;
        let mut fragments: BTreeMap<(u16, i32), String> = BTreeMap::new();
        let mut fragment_script = None;
        // Under the partial reading the winner's VMAD when it carries one,
        // else the record's.
        let vmad = winner
            .as_ref()
            .and_then(|wr| {
                wr.subrecords
                    .iter()
                    .find(|s| s.signature == *b"VMAD")
                    .map(|s| (wr, s))
            })
            .or_else(|| q.vmad.map(|i| (&r, &r.subrecords[i])));
        if let Some((vr, s)) = vmad {
            let v = Vmad::read(vr.data(s), vr.signature, &vr.source.file, s.offset, b)?;
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
        let (location_refs, location_rows) = location_refs(store, &q, b)?;
        let collections = q
            .aliases
            .iter()
            .filter(|a| matches!(a.kind, fo4_plugin::quest::AliasKind::Collection { .. }))
            .map(|a| a.id)
            .collect();
        let unique_actors = q
            .aliases
            .iter()
            .filter(|a| a.kind != fo4_plugin::quest::AliasKind::Location)
            .filter_map(|a| {
                a.values.iter().find_map(|v| match v {
                    fo4_plugin::quest::AliasValue::UniqueActor(f) => {
                        f.key.clone().map(|k| (a.id, k))
                    }
                    _ => None,
                })
            })
            .collect();
        let mut created = BTreeMap::new();
        let mut external = BTreeMap::new();
        for a in q
            .aliases
            .iter()
            .filter(|a| a.kind == fo4_plugin::quest::AliasKind::Reference)
        {
            for v in &a.values {
                match v {
                    fo4_plugin::quest::AliasValue::CreateObject {
                        object,
                        alias,
                        word,
                        level,
                    } => {
                        created.entry(a.id).or_insert(CreatedAlias {
                            object: object.key.clone(),
                            at: *alias,
                            word: *word,
                            level: *level,
                        });
                    }
                    fo4_plugin::quest::AliasValue::External { quest, alias } => {
                        if let Some(qk) = &quest.key {
                            external.entry(a.id).or_insert((qk.clone(), *alias));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(Some((
            QuestInfo {
                key: k.clone(),
                plugin,
                file: r.source.file.clone(),
                flags: q.general.map(|g| g.flags),
                stages,
                objectives: q.objectives.iter().map(|o| o.objective).collect(),
                reference_aliases,
                location_refs,
                location_rows,
                collections,
                unique_actors,
                priority: q.general.map(|g| g.priority),
                fragment_script,
                unmapped_fragments: fragments.len() - mapped,
                event: q.event,
                event_conditions: q.event_conditions.iter().map(Condition::from).collect(),
                created,
                external,
            },
            merged,
        )))
    }
}

/// The record flag bit the partial reading follows
/// (`quest_record_partial_previous_v1`).
pub const PARTIAL_RECORD: u32 = 0x4000;

/// Whether a record carries [`PARTIAL_RECORD`] and no subrecord but `EDID`
/// and `VMAD`.
pub fn partial_only_script(r: &fo4_plugin::Record) -> bool {
    r.flags & PARTIAL_RECORD != 0
        && r.subrecords
            .iter()
            .all(|s| matches!(&s.signature, b"EDID" | b"VMAD"))
}

/// One row of a location's special reference list: (location ref type,
/// placed reference), each `None` when its word resolves to no record.
pub type SpecialReference = (Option<FormKey>, Option<FormKey>);

/// The rows of a location's special reference list (LCSR): 16 bytes each,
/// a location ref type, a placed reference, its world or cell, a grid word
/// (`quest_alias_location_ref_v1`); (ref type, reference) resolved through
/// the location's plugin. `None` when the list does not divide into rows.
pub fn special_references(
    store: &Store,
    location: &FormKey,
    b: &Budget,
) -> fo4_core::Result<Option<Vec<SpecialReference>>> {
    let Some((r, plugin)) = store.raw_with_plugin(location, b)? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"LCSR") {
        let d = r.data(s);
        if d.len() % 16 != 0 {
            return Ok(None);
        }
        for row in d.chunks(16) {
            let w = |o: usize| u32::from_le_bytes([row[o], row[o + 1], row[o + 2], row[o + 3]]);
            let f = |id: u32| {
                store
                    .order()
                    .remap(plugin, id, &r.source.file, s.offset)
                    .ok()
                    .flatten()
            };
            out.push((f(w(0)), f(w(4))));
        }
    }
    Ok(Some(out))
}

/// An alias filled from a forced location by a ref type: the location,
/// the ref type and the references the special reference rows of that
/// type name, in row order with repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationRows {
    pub location: FormKey,
    pub ref_type: FormKey,
    pub listed: Vec<FormKey>,
}
impl LocationRows {
    /// The distinct references the rows name, in first-row order.
    pub fn distinct(&self) -> Vec<&FormKey> {
        let mut out: Vec<&FormKey> = Vec::new();
        for k in &self.listed {
            if !out.contains(&k) {
                out.push(k);
            }
        }
        out
    }
}

type LocationFills = (
    BTreeMap<u32, Result<FormKey, &'static str>>,
    BTreeMap<u32, LocationRows>,
);

/// The location fills of a quest's reference aliases
/// (`quest_alias_location_ref_v1`), and the rows behind them.
fn location_refs(store: &Store, q: &Quest, b: &Budget) -> fo4_core::Result<LocationFills> {
    use fo4_plugin::quest::{AliasKind, AliasValue};
    let mut out = BTreeMap::new();
    let mut rows_of = BTreeMap::new();
    for a in q.aliases.iter().filter(|a| a.kind != AliasKind::Location) {
        let Some((l, t)) = a.values.iter().find_map(|v| match v {
            AliasValue::LocationAliasReference { alias, ref_type } => Some((*alias, ref_type)),
            _ => None,
        }) else {
            continue;
        };
        let Some(t) = t.as_ref().and_then(|t| t.key.clone()) else {
            out.insert(a.id, Err("the alias names no location ref type"));
            continue;
        };
        let loc = q
            .aliases
            .iter()
            .find(|x| x.kind == AliasKind::Location && i64::from(x.id) == i64::from(l))
            .and_then(|x| {
                x.values.iter().find_map(|v| match v {
                    AliasValue::ForcedLocation(f) => f.key.clone(),
                    _ => None,
                })
            });
        let Some(loc) = loc else {
            out.insert(a.id, Err("the location alias has no forced location"));
            continue;
        };
        let Some(rows) = special_references(store, &loc, b)? else {
            out.insert(a.id, Err("the location's special references do not read"));
            continue;
        };
        let refs: Vec<&FormKey> = rows
            .iter()
            .filter(|(rt, _)| rt.as_ref() == Some(&t))
            .filter_map(|(_, r)| r.as_ref())
            .collect();
        rows_of.insert(
            a.id,
            LocationRows {
                location: loc.clone(),
                ref_type: t.clone(),
                listed: refs.iter().map(|k| (*k).clone()).collect(),
            },
        );
        out.insert(
            a.id,
            match refs.as_slice() {
                [one] => Ok((*one).clone()),
                [] => Err("no special reference of that type in the location"),
                _ => Err("several special references of that type in the location"),
            },
        );
    }
    Ok((out, rows_of))
}

/// The placed references tied to a location by their cell, with the
/// location ref types their own `XLRT` carries
/// (`quest_alias_location_ref_placed_v1`): every winning placed reference
/// with an `XLRT` whose cell's `XLCN` names a location, read once when a
/// run turns the reading on.
#[derive(Debug, Default)]
pub struct LocationPlaced {
    /// Per location (record key): (placed reference, its ref types), in
    /// placement order.
    pub tagged: BTreeMap<String, Vec<(FormKey, Vec<FormKey>)>>,
}
impl LocationPlaced {
    /// Reads every winning cell's `XLCN`, then the `XLRT` of the placed
    /// references of the cells that have one.
    pub fn read(store: &Store, b: &Budget) -> LocationPlaced {
        let word = |r: &fo4_plugin::Record, plugin: usize, s: &fo4_plugin::Subrecord, d: &[u8]| {
            let w = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
            store
                .order()
                .remap(plugin, w, &r.source.file, s.offset)
                .ok()
                .flatten()
        };
        let mut location_of: BTreeMap<FormKey, FormKey> = BTreeMap::new();
        for k in store.cells() {
            let bb = b.child();
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                continue;
            };
            let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"XLCN") else {
                continue;
            };
            let d = r.data(s);
            if d.len() != 4 {
                continue;
            }
            if let Some(l) = word(&r, plugin, s, d) {
                location_of.insert(k, l);
            }
        }
        let mut tagged: BTreeMap<String, Vec<(FormKey, Vec<FormKey>)>> = BTreeMap::new();
        for (o, cell) in store.placement_cells() {
            if o.deleted {
                continue;
            }
            let Some(l) = location_of.get(&cell) else {
                continue;
            };
            let k = store.overrides().form_key(o.key, store.order());
            let bb = b.child();
            let Ok(Some((r, plugin))) = store.raw_with_plugin(&k, &bb) else {
                continue;
            };
            let mut types = Vec::new();
            for s in r.subrecords.iter().filter(|s| s.signature == *b"XLRT") {
                for c in r.data(s).as_chunks::<4>().0 {
                    if let Some(t) = word(&r, plugin, s, c) {
                        types.push(t);
                    }
                }
            }
            if !types.is_empty() {
                tagged
                    .entry(bind::record_key(l))
                    .or_default()
                    .push((k, types));
            }
        }
        LocationPlaced { tagged }
    }
    /// The one placed reference of a cell the location's `XLCN` names
    /// that carries the ref type in its `XLRT`; `Err` when none or several.
    pub fn fill(&self, location: &FormKey, ref_type: &FormKey) -> Result<FormKey, &'static str> {
        let found: Vec<&FormKey> = self
            .tagged
            .get(&bind::record_key(location))
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|(_, ts)| ts.contains(ref_type))
            .map(|(k, _)| k)
            .collect();
        match found.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err("no placed reference of the location's cells carries that ref type"),
            _ => Err("several placed references of the location's cells carry that ref type"),
        }
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
    /// Collection alias contents a run read or changed, by alias id
    /// (`quest_collection_alias_location_refs_v1`); `None` once a change
    /// with an undetermined value left them open.
    pub collections: BTreeMap<i32, Option<Vec<Val>>>,
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
    /// Fill reference aliases from their location's special references
    /// (`quest_alias_location_ref_v1`; `quest_run --alias-location-refs`).
    /// Off by default: such an alias has no reference, as before.
    pub location_fills: bool,
    /// The placed actors of each actor base, for reference aliases filled
    /// by a unique actor (`quest_alias_unique_actor_ref_v1`;
    /// `quest_run --alias-unique-actors`). `None` by default: such an
    /// alias has no reference, as before.
    pub unique: Option<Rc<UniqueActors>>,
    /// The aliases (quest record key, alias id) answered from a unique
    /// actor so far, and whether each resolved (the caller adds the rule
    /// when any was).
    pub unique_used: std::cell::RefCell<BTreeMap<(String, u32), bool>>,
    /// With [`Quests::location_fills`]: rows of the special reference list
    /// that name the same reference count once
    /// (`quest_alias_location_ref_distinct_v1`; `quest_run
    /// --location-refs-distinct`). Off by default.
    pub location_distinct: bool,
    /// With [`Quests::location_fills`]: an alias whose location lists no
    /// row of its ref type holds the one placed reference of the
    /// location's cells carrying the type
    /// (`quest_alias_location_ref_placed_v1`; `quest_run
    /// --alias-location-placed-refs`). `None` by default.
    pub location_placed: Option<Rc<LocationPlaced>>,
    /// With [`Quests::location_fills`]: where several reference aliases of
    /// a quest name the same location and ref type and its rows name as
    /// many distinct references of the type, the aliases hold them one
    /// each, the lowest alias id the first reference in row order
    /// (`quest_alias_location_ref_shared_v1`; `quest_run
    /// --location-refs-shared`). Off by default.
    pub location_shared: bool,
    /// The rules the location readings after the list's relied on (the
    /// caller adds them to the run's rules).
    pub location_used: std::cell::RefCell<BTreeSet<&'static str>>,
    /// Collection aliases hold the distinct references their forced
    /// location's special reference rows of their ref type name, and
    /// their natives answer from them
    /// (`quest_collection_alias_location_refs_v1`; `quest_run
    /// --collection-location-refs`). Off by default: the natives are
    /// recorded as before.
    pub collection_fills: bool,
    /// What the collection natives answered and why not, counted.
    pub collection_counts: BTreeMap<String, u64>,
    /// The references the run made for created aliases: (real-time
    /// second, quest record key, alias id), in order.
    pub created_made: Vec<(f64, String, u32)>,
    /// A reference alias created at run time (`ALCO`) holds, while its
    /// quest runs, a reference the run makes for it: no record, no scripts
    /// of its own, alive until the run kills it
    /// (`quest_alias_created_reference_v1`; `quest_run
    /// --alias-created-refs`). Off by default: such an alias has no
    /// reference, as before.
    pub created_refs: bool,
    /// A reference alias filled from another quest's alias (`ALEQ`) holds,
    /// whenever it is read, what that alias holds at that moment
    /// (`quest_alias_external_live_v1`; `quest_run --alias-external`). Off
    /// by default: such an alias has no reference, as before.
    pub external_fills: bool,
    /// The rules the two readings above relied on (the caller adds them to
    /// the run's rules).
    pub fill_used: std::cell::RefCell<BTreeSet<&'static str>>,
    /// A winning quest record whose flag word has [`PARTIAL_RECORD`] and
    /// that carries no subrecord but `EDID` and `VMAD` is read from the
    /// occurrence before it, its own VMAD kept
    /// (`quest_record_partial_previous_v1`; `quest_run
    /// --quest-partial-previous`). Off by default: the winner is read as
    /// it stands, as before.
    pub partial_previous: bool,
    /// The quests read that way, by record key, in the order read.
    pub partial_read: Vec<String>,
}

/// The key of the reference the run makes for a created alias
/// (`quest_alias_created_reference_v1`): no record key resolves from it.
pub fn created_key(quest: &FormKey, id: u32) -> String {
    format!("{}#created:{id}", bind::record_key(quest))
}

/// Whether an object is a reference the run made for a created alias.
pub fn is_created(w: &World, o: ObjId) -> bool {
    w.objects
        .get(o)
        .and_then(|x| x.key.as_deref())
        .is_some_and(|k| k.contains("#created:"))
}

/// The longest chain of aliases filled from other quests' aliases followed.
pub const MAX_ALIAS_CHAIN: usize = 8;

/// What a reference alias holds under the two fill readings past the
/// forced fill ([`Quests::created_refs`], [`Quests::external_fills`]):
/// `None` when neither applies to the alias (the caller keeps its earlier
/// answer); `Ok(None)` when it holds nothing; `Err` when that is
/// undetermined.
pub fn held_more(
    store: &Store,
    max_memory: u64,
    w: &mut World,
    info: &QuestInfo,
    id: u32,
    depth: usize,
) -> Option<Result<Option<Val>, &'static str>> {
    if w.quests.created_refs {
        if let Some(c) = info.created.get(&id) {
            w.quests
                .fill_used
                .borrow_mut()
                .insert(rules::ALIAS_CREATED_REFERENCE);
            let key = created_key(&info.key, id);
            if let Some(o) = w
                .objects
                .iter()
                .position(|x| x.key.as_deref() == Some(key.as_str()))
            {
                return Some(Ok(Some(Val::Object(o))));
            }
            let actor = c
                .object
                .as_ref()
                .and_then(|k| store.signature(k))
                .is_some_and(|s| matches!(&s, b"NPC_" | b"LVLN"));
            let label = format!(
                "created reference of alias {id} of {}",
                bind::label(store, &info.key, &Budget::new(max_memory))
                    .unwrap_or_else(|_| bind::record_key(&info.key))
            );
            let types: &[&str] = if actor {
                &["actor", "objectreference"]
            } else {
                &["objectreference"]
            };
            let o = w.object(label, Some(key), types);
            w.quests.created_made.push((
                w.now[crate::world::Clock::Real as usize],
                bind::record_key(&info.key),
                id,
            ));
            return Some(Ok(Some(Val::Object(o))));
        }
    }
    if w.quests.external_fills {
        if let Some((q, a)) = info.external.get(&id).cloned() {
            w.quests
                .fill_used
                .borrow_mut()
                .insert(rules::ALIAS_EXTERNAL_LIVE);
            if depth >= MAX_ALIAS_CHAIN {
                return Some(Err("a chain of aliases filled from other quests too long"));
            }
            return Some(alias_value(store, max_memory, w, &q, a, depth + 1));
        }
    }
    None
}

/// What a reference alias of a quest holds as `GetReference` would answer
/// it, with the fill readings past the forced fill the run turns on: the
/// fill a run set, nothing while the quest is stopped, the forced (or
/// location, or unique actor) fill, else [`held_more`]. `Ok(None)`: it
/// holds nothing.
pub fn alias_value(
    store: &Store,
    max_memory: u64,
    w: &mut World,
    quest: &FormKey,
    alias: i32,
    depth: usize,
) -> Result<Option<Val>, &'static str> {
    let info = info_by_key(store, max_memory, w, quest).ok_or("the quest named does not read")?;
    let s = w.quests.state(&info);
    if s.open.is_some() {
        return Err("the quest's state is undetermined");
    }
    if let Some(v) = s.fills.get(&alias) {
        return Ok(match v {
            Val::None => None,
            x => Some(x.clone()),
        });
    }
    match s.running {
        None => return Err("whether the quest runs is undetermined"),
        Some(false) => return Ok(None),
        Some(true) => {}
    }
    let id = u32::try_from(alias).map_err(|_| "no reference alias of that index")?;
    match w.quests.fill(&info, id)? {
        Some(Fill::Player) => Ok(Some(Val::Object(w.player("actor")))),
        Some(Fill::Record(k)) => {
            if store.signature(&k).is_none() {
                return Err("the forced reference names no winning record");
            }
            let o = record_object(w, store, &k, "objectreference", &Budget::new(max_memory))
                .map_err(|_| "the forced reference could not be read")?;
            Ok(Some(Val::Object(o)))
        }
        None => match held_more(store, max_memory, w, &info, id, depth) {
            Some(r) => r,
            None => Ok(None),
        },
    }
}

/// The placed actors (ACHR) of every actor base in the loaded plugins,
/// and the player's base as the caller names it
/// (`quest_alias_unique_actor_ref_v1`).
#[derive(Debug, Default)]
pub struct UniqueActors {
    /// Winning, not deleted ACHR by the record key of their base (NAME),
    /// in placement order.
    pub placed: BTreeMap<String, Vec<FormKey>>,
    /// The NPC_ the caller names as the player's base (`quest_run
    /// --player-base`, a caller assumption): an alias of that unique
    /// actor holds the player.
    pub player_base: Option<FormKey>,
}
impl UniqueActors {
    /// Reads every winning placed ACHR's base.
    pub fn read(store: &Store, b: &Budget) -> UniqueActors {
        let mut u = UniqueActors::default();
        for (o, _) in store.placement_cells() {
            if o.signature != *b"ACHR" || o.deleted {
                continue;
            }
            let k = store.overrides().form_key(o.key, store.order());
            if let Ok(Some(base)) = bind::base_of(store, &k, &b.child()) {
                u.placed.entry(bind::record_key(&base)).or_default().push(k);
            }
        }
        u
    }
    /// What a unique actor alias holds: the player for the player's base,
    /// else the one placed actor of that base; `Err` when none or several
    /// are placed.
    pub fn fill(&self, base: &FormKey) -> Result<Fill, &'static str> {
        if self.player_base.as_ref() == Some(base) {
            return Ok(Fill::Player);
        }
        match self.placed.get(&bind::record_key(base)).map(Vec::as_slice) {
            Some([one]) => Ok(Fill::Record(one.clone())),
            Some([_, _, ..]) => Err("several placed actors of the unique actor's base"),
            _ => Err("no placed actor of the unique actor's base"),
        }
    }
}

/// What a reference alias holds without a fill a run set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fill {
    Record(FormKey),
    /// The player (a unique actor alias of the player's base).
    Player,
}

impl Quests {
    /// The reference an alias holds while its quest runs and no run set
    /// its fill: [`QuestInfo::fill_of`], else, with [`Quests::unique`], the
    /// unique actor's placed reference (`quest_alias_unique_actor_ref_v1`).
    /// `Ok(None)`: none of them.
    pub fn fill(&self, info: &QuestInfo, id: u32) -> Result<Option<Fill>, &'static str> {
        let forced = self.fill_location(info, id);
        match (&forced, &self.unique, info.unique_actors.get(&id)) {
            (Ok(None), Some(u), Some(base)) => {
                let f = u.fill(base);
                self.unique_used
                    .borrow_mut()
                    .insert((bind::record_key(&info.key), id), f.is_ok());
                Ok(Some(f?))
            }
            _ => forced.map(|f| f.map(Fill::Record)),
        }
    }
}

impl Quests {
    /// [`QuestInfo::fill_of`] with the location readings the run turns on
    /// after the list's: repeated rows naming one reference
    /// (`quest_alias_location_ref_distinct_v1`), then, where the list has
    /// no row of the type, the placed references of the location's cells
    /// (`quest_alias_location_ref_placed_v1`). With both off it is
    /// [`QuestInfo::fill_of`].
    pub fn fill_location(
        &self,
        info: &QuestInfo,
        id: u32,
    ) -> Result<Option<FormKey>, &'static str> {
        let got = info.fill_of(id, self.location_fills);
        if got.is_ok() || !self.location_fills {
            return got;
        }
        let Some(rows) = info.location_rows.get(&id) else {
            return got;
        };
        if self.location_distinct && rows.listed.len() > 1 {
            if let [one] = rows.distinct().as_slice() {
                self.location_used
                    .borrow_mut()
                    .insert(rules::ALIAS_LOCATION_DISTINCT);
                return Ok(Some((*one).clone()));
            }
        }
        if self.location_shared {
            let distinct = rows.distinct();
            if distinct.len() > 1 {
                // The quest's aliases of the same location and ref type,
                // by id (the map's order).
                let sharing: Vec<u32> = info
                    .location_rows
                    .iter()
                    .filter(|(_, r)| r.location == rows.location && r.ref_type == rows.ref_type)
                    .map(|(a, _)| *a)
                    .collect();
                if sharing.len() == distinct.len() {
                    if let Some(i) = sharing.iter().position(|a| *a == id) {
                        self.location_used
                            .borrow_mut()
                            .insert(rules::ALIAS_LOCATION_SHARED);
                        return Ok(Some(distinct[i].clone()));
                    }
                }
            }
        }
        if let (Some(p), true) = (&self.location_placed, rows.listed.is_empty()) {
            self.location_used
                .borrow_mut()
                .insert(rules::ALIAS_LOCATION_PLACED);
            return p.fill(&rows.location, &rows.ref_type).map(Some);
        }
        got
    }
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
                collections: BTreeMap::new(),
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
    let i = load_cached(store, max_memory, w, &k);
    w.quests.infos.insert(key, i.clone());
    i
}

/// Reads a quest's record for the run's cache, under the partial reading
/// when the run turns it on ([`Quests::partial_previous`]).
fn load_cached(
    store: &Store,
    max_memory: u64,
    w: &mut World,
    k: &FormKey,
) -> Option<Rc<QuestInfo>> {
    let b = Budget::new(max_memory);
    let (i, merged) = QuestInfo::load_with(store, k, &b, w.quests.partial_previous)
        .ok()
        .flatten()?;
    if merged {
        w.quests
            .fill_used
            .borrow_mut()
            .insert(rules::QUEST_PARTIAL_PREVIOUS);
        w.quests.partial_read.push(bind::record_key(k));
    }
    Some(Rc::new(i))
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
    let i = load_cached(store, max_memory, w, k);
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
    /// The conversation a dialogue line is tried in, when it is
    /// (`crate::talk`, `dialogue_conversation_parties_v1`).
    pub parties: Option<crate::talk::Parties>,
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
    /// Caller assumptions scoped to the condition lists of one quest's
    /// records (`quest_run --assume-cond-for QUEST N=VALUE`), by the
    /// quest's record key; over [`CondInputs::assume`]. Not a rule.
    pub assume_for: BTreeMap<String, BTreeMap<u16, f32>>,
    /// Conditions an assumption decided.
    pub assumed: std::cell::Cell<u64>,
    /// Function 1 read from the world model's positions
    /// (`cond_fn_1_track_distance_v1`; `quest_run --cond-1-distance`). Off
    /// by default.
    pub distance: bool,
    /// The first plugin's id 0x14 as a reference word stands for the player
    /// (`cond_player_reference_word_v1`; `quest_run --cond-player-word`).
    /// Off by default.
    pub player_word: bool,
    /// Function 70 on the player read from the caller's assumed sex
    /// (`cond_fn_70_player_sex_v1`; `quest_run --cond-70-player-sex`, with
    /// `--player-base-object` and `--assume-player-sex`). Off by default.
    pub player_sex: bool,
    /// The same reading limited to the condition lists of these quests'
    /// records, by record key (`quest_run --cond-70-player-sex-for
    /// QUEST`, the caller's choice of where it applies).
    pub player_sex_for: std::collections::BTreeSet<String>,
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
            parties: None,
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
        self.data.global_stored(g)
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
    fn distance_on(&self) -> bool {
        self.cond.distance && self.w.place.on
    }
    fn player_word_on(&self) -> bool {
        self.cond.player_word
    }
    fn player_sex(&self) -> Option<u32> {
        let scoped = self.owner.as_ref().is_some_and(|q| {
            self.cond
                .player_sex_for
                .contains(&crate::bind::record_key(q))
        });
        if !(self.cond.player_sex || scoped) {
            return None;
        }
        self.w.player_sex.and_then(|n| u32::try_from(n).ok())
    }
    /// The alias's holder as [`Facts::alias_reference`] reads it, the
    /// player included (a unique actor alias of the player's base, or a
    /// fill a run set to the player).
    fn alias_party(
        &mut self,
        quest: &FormKey,
        alias: i32,
    ) -> Result<Option<cond::Party>, &'static str> {
        let player = self.w.player;
        let s = self.state(quest)?;
        if let (Some(Val::Object(o)), Some(p)) = (s.fills.get(&alias), player) {
            if *o == p {
                return Ok(Some(cond::Party::Player));
            }
        }
        match self.alias_reference(quest, alias) {
            Err("the alias holds the player") => Ok(Some(cond::Party::Player)),
            r => r.map(|k| k.map(cond::Party::Record)),
        }
    }
    /// Function 1 from the world model (`cond_fn_1_track_distance_v1`):
    /// where [`crate::track::spot`] places both parties at the present
    /// second, in one space.
    fn distance(&mut self, a: &cond::Party, b: &cond::Party) -> Result<f32, &'static str> {
        let (store, max) = (self.data.store, self.data.max_memory);
        let mut obj = |p: &cond::Party| -> Result<ObjId, &'static str> {
            match p {
                cond::Party::Player => Ok(self.w.player("actor")),
                cond::Party::Record(k) => {
                    let b = Budget::new(max);
                    record_object(self.w, store, k, "objectreference", &b)
                        .map_err(|_| "a reference could not be read")
                }
            }
        };
        let (oa, ob) = (obj(a)?, obj(b)?);
        let sa = crate::track::spot(store, max, self.w, Some(&Val::Object(oa)));
        let sb = crate::track::spot(store, max, self.w, Some(&Val::Object(ob)));
        match (sa, sb) {
            (
                crate::track::Spot::At {
                    space: s1, pos: p1, ..
                },
                crate::track::Spot::At {
                    space: s2, pos: p2, ..
                },
            ) => {
                if s1 != s2 {
                    return Err("the two are in different spaces");
                }
                let d =
                    ((p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2) + (p1[2] - p2[2]).powi(2))
                        .sqrt();
                Ok(d as f32)
            }
            (crate::track::Spot::Open(why), _) | (_, crate::track::Spot::Open(why)) => Err(why),
        }
    }
    fn max_memory(&self) -> u64 {
        self.data.max_memory
    }
    fn reference_base(&mut self, reference: &FormKey) -> Result<FormKey, &'static str> {
        self.data.base_stored(reference)
    }
    fn assumed(&self, function: u16) -> Option<f32> {
        let scoped = self
            .owner
            .as_ref()
            .and_then(|q| self.cond.assume_for.get(&crate::bind::record_key(q)))
            .and_then(|m| m.get(&function).copied());
        let v = scoped.or_else(|| self.cond.assume.get(&function).copied());
        if v.is_some() {
            self.cond.assumed.set(self.cond.assumed.get() + 1);
        }
        v
    }
    fn owner(&self) -> Option<&FormKey> {
        self.owner.as_ref()
    }
    /// The speaker's (run-on 0) or the listener's (run-on 1) base in a
    /// conversation: the placed actor's NAME, or the NPC_ the caller names
    /// as the player's base (`dialogue_conversation_parties_v1`).
    fn party_base(&mut self, run_on: u32) -> Option<Result<FormKey, &'static str>> {
        use crate::talk::Party;
        let p = self.parties.clone()?;
        let party = match run_on {
            0 => p.speaker,
            1 => p.listener,
            _ => return None,
        };
        Some(match party {
            Party::Player => self
                .w
                .talk
                .player_base
                .clone()
                .ok_or("the player's base is not named (--player-base)"),
            Party::Actor(k) => self.reference_base(&k),
        })
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
                let id = u32::try_from(alias).map_err(|_| "no reference alias of that index")?;
                match self.w.quests.fill(&i, id)? {
                    Some(Fill::Record(k)) => Ok(Some(k)),
                    Some(Fill::Player) => Err("the alias holds the player"),
                    None => Err("the alias has no forced reference"),
                }
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
    ///
    /// With the scene clock on and `Scenes::action_conditions`, an action
    /// the clock completed is complete, any other is not
    /// (`cond_fn_550_clock_actions_v1`).
    fn action_rule(&self) -> Option<&'static str> {
        (self.w.scenes.clock.on && self.w.scenes.action_conditions)
            .then_some(rules::COND_FN_550_CLOCK)
    }
    fn action_percent_on(&self) -> bool {
        self.w.scenes.clock.on && self.w.scenes.action_percent
    }
    /// Function 131 from the scene clock (`cond_fn_131_action_percent_v1`):
    /// an action of the scene not started reads 0, a completed one 100, a
    /// running one timed on the clock the share of its seconds already run;
    /// a running action the clock does not time is undetermined.
    fn action_percent(&mut self, scene: &FormKey, index: u32) -> Result<f32, &'static str> {
        let key = bind::record_key(scene);
        let Some(o) = self
            .w
            .objects
            .iter()
            .position(|x| x.key.as_deref() == Some(&key))
        else {
            return Ok(0.0);
        };
        let Some(Some(info)) = self.w.scenes.infos.get(&key).cloned() else {
            return Ok(0.0);
        };
        let now = self.w.now[crate::world::Clock::Real as usize];
        let mut found = false;
        let mut best: Option<f32> = None;
        for (j, a) in info.actions.iter().enumerate() {
            if a.index != index {
                continue;
            }
            found = true;
            let Some(r) = self.w.scenes.actions.get(&(o, j)) else {
                continue;
            };
            let v = if r.done {
                100.0
            } else {
                let d = r.due.ok_or("the action runs untimed on the scene clock")?;
                let span = d - r.start;
                if span <= 0.0 {
                    100.0
                } else {
                    (100.0 * (now - r.start) / span).clamp(0.0, 100.0) as f32
                }
            };
            best = Some(best.map_or(v, |b: f32| b.max(v)));
        }
        if !found {
            return Err("the scene holds no action of that index");
        }
        Ok(best.unwrap_or(0.0))
    }
    fn scene_action_complete(&mut self, scene: &FormKey, index: u32) -> Result<bool, &'static str> {
        if self.w.scenes.clock.on && self.w.scenes.action_conditions {
            let key = bind::record_key(scene);
            let Some(o) = self
                .w
                .objects
                .iter()
                .position(|x| x.key.as_deref() == Some(&key))
            else {
                return Ok(false);
            };
            let Some(Some(info)) = self.w.scenes.infos.get(&key).cloned() else {
                return Ok(false);
            };
            return Ok(info.actions.iter().enumerate().any(|(j, a)| {
                a.index == index && self.w.scenes.actions.get(&(o, j)).is_some_and(|r| r.done)
            }));
        }
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
    /// Reads the graphs of models for `crate::anim` (`rules::ANIM_GRAPH`);
    /// `None`: no graph is read.
    pub graphs: Option<Box<dyn crate::anim::GraphSource + 's>>,
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
            graphs: None,
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
                        let id =
                            u32::try_from(id).map_err(|_| "no reference alias of that index")?;
                        if w.quests.location_fills && i.location_refs.contains_key(&id) {
                            self.using(rules::ALIAS_LOCATION_REF);
                        }
                        let forced = match w.quests.fill(&i, id)? {
                            Some(Fill::Player) => {
                                self.using(rules::ALIAS_UNIQUE_ACTOR);
                                let p = w.player("actor");
                                return Ok((Val::Object(p), data::Answer::Record));
                            }
                            Some(Fill::Record(k)) => Some(k),
                            None => None,
                        };
                        if forced.is_some() && i.unique_actors.contains_key(&id) {
                            self.using(rules::ALIAS_UNIQUE_ACTOR);
                        }
                        if forced.is_none() {
                            let more =
                                held_more(self.data.store, self.data.max_memory, w, &i, id, 0);
                            let used = std::mem::take(&mut *w.quests.fill_used.borrow_mut());
                            for r in used.iter() {
                                self.using(r);
                            }
                            w.quests.fill_used.borrow_mut().extend(used);
                            if let Some(r) = more {
                                return Ok(match r? {
                                    Some(v) => (v, data::Answer::Record),
                                    None => (Val::None, data::Answer::NoRecord),
                                });
                            }
                        }
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

/// The natives that give an actor's base object.
pub const BASE_OBJECT_NATIVES: [&str; 3] = [
    "objectreference.getbaseobject",
    "actor.getactorbase",
    "actor.getleveledactorbase",
];

impl QuestHost<'_> {
    /// `GetBaseObject`, `GetActorBase` and `GetLeveledActorBase` on the
    /// player's handle answer the caller's player base
    /// (`vm_player_base_object_v1`); `ActorBase.GetSex` on that base answers
    /// the caller's assumed value.
    fn player_base_native(&mut self, c: &NativeCall, w: &mut World) -> Option<Reply> {
        let base = w.player_base_object.clone()?;
        let full = c.full_name();
        let Some(Val::Object(o)) = &c.receiver else {
            return None;
        };
        let o = *o;
        if BASE_OBJECT_NATIVES.contains(&full.as_str()) && Some(o) == w.player {
            self.using(rules::PLAYER_BASE_OBJECT);
            let b = Budget::new(self.data.max_memory);
            return record_object(w, self.data.store, &base, "actorbase", &b)
                .ok()
                .map(|x| Reply::Value(Val::Object(x)));
        }
        if full == "actorbase.getsex"
            && w.objects[o].key.as_deref() == Some(bind::record_key(&base).as_str())
        {
            return w.player_sex.map(|n| {
                self.using(rules::PLAYER_BASE_OBJECT);
                Reply::Value(Val::Int(n))
            });
        }
        None
    }
}

/// The natives on a collection alias answered under
/// `quest_collection_alias_location_refs_v1`.
pub const COLLECTION_NATIVES: [&str; 6] = [
    "refcollectionalias.getcount",
    "refcollectionalias.getat",
    "refcollectionalias.find",
    "refcollectionalias.addref",
    "refcollectionalias.removeref",
    "refcollectionalias.removeall",
];

impl QuestHost<'_> {
    fn collection_count(w: &mut World, k: impl Into<String>) {
        *w.quests.collection_counts.entry(k.into()).or_default() += 1;
    }
    /// A collection alias's contents: what the run read or changed, else
    /// (the quest running) the distinct references of its location rows.
    fn collection_contents(
        &mut self,
        w: &mut World,
        i: &QuestInfo,
        id: i32,
    ) -> Result<Vec<Val>, &'static str> {
        let s = w.quests.state(i);
        if s.open.is_some() {
            return Err(OPEN);
        }
        if let Some(v) = s.collections.get(&id) {
            return v.clone().ok_or("changed by an undetermined value");
        }
        let running = s.running;
        let rows = u32::try_from(id)
            .ok()
            .and_then(|u| i.location_rows.get(&u))
            .map(|r| r.distinct().into_iter().cloned().collect::<Vec<_>>());
        let items = match running {
            None => return Err("whether the quest runs is undetermined"),
            Some(false) => Vec::new(),
            Some(true) => {
                let rows = rows.ok_or("the collection is not filled from a location's rows")?;
                let b = Budget::new(self.data.max_memory);
                let mut out = Vec::new();
                for k in rows {
                    if self.data.store.signature(&k).is_none() {
                        return Err("a row names no winning record");
                    }
                    let o = record_object(w, self.data.store, &k, "objectreference", &b)
                        .map_err(|_| "a row's reference could not be read")?;
                    out.push(Val::Object(o));
                }
                out
            }
        };
        w.quests
            .state(i)
            .collections
            .insert(id, Some(items.clone()));
        Ok(items)
    }
    /// The collection natives (`quest_collection_alias_location_refs_v1`),
    /// when a caller turned the reading on; `None` leaves the call to be
    /// recorded as before.
    fn collection_native(&mut self, c: &NativeCall, w: &mut World) -> Option<Reply> {
        if !w.quests.collection_fills {
            return None;
        }
        let full = c.full_name();
        if !COLLECTION_NATIVES.contains(&full.as_str()) {
            return None;
        }
        let (_, i, id) = self.alias_of(w, c.receiver.as_ref()).ok()?;
        if !u32::try_from(id).is_ok_and(|u| i.collections.contains(&u)) {
            Self::collection_count(
                w,
                format!("{}\tnot answered: not a collection alias", c.name),
            );
            return None;
        }
        let name = c.name.clone();
        let cur = self.collection_contents(w, &i, id);
        let set = |w: &mut World, v: Option<Vec<Val>>| {
            w.quests.state(&i).collections.insert(id, v);
        };
        let reply = match (name.as_str(), cur) {
            ("removeall", _) => {
                set(w, Some(Vec::new()));
                Reply::Value(Val::None)
            }
            ("addref" | "removeref", Err(_)) => Reply::Value(Val::None),
            ("addref" | "removeref", Ok(mut items)) => {
                match c.args.first() {
                    Some(v @ Val::Object(_)) => {
                        let at = items.iter().position(|x| x == v);
                        match (name.as_str(), at) {
                            ("addref", None) => items.push(v.clone()),
                            ("removeref", Some(n)) => {
                                items.remove(n);
                            }
                            _ => {}
                        }
                        set(w, Some(items));
                    }
                    Some(Val::None) => {}
                    _ => set(w, None),
                }
                Reply::Value(Val::None)
            }
            (_, Err(why)) => {
                Self::collection_count(w, format!("{name}\tnot answered: {why}"));
                return None;
            }
            ("getcount", Ok(items)) => Reply::Value(Val::Int(items.len() as i32)),
            ("getat", Ok(items)) => match c.args.first() {
                Some(Val::Int(n)) if *n >= 0 && (*n as usize) < items.len() => {
                    Reply::Value(items[*n as usize].clone())
                }
                _ => {
                    Self::collection_count(
                        w,
                        format!("{name}\tnot answered: the index is undetermined or out of range"),
                    );
                    return None;
                }
            },
            ("find", Ok(items)) => match c.args.first() {
                Some(v @ (Val::Object(_) | Val::None)) => Reply::Value(Val::Int(
                    items.iter().position(|x| x == v).map_or(-1, |n| n as i32),
                )),
                _ => {
                    Self::collection_count(
                        w,
                        format!("{name}\tnot answered: the reference is undetermined"),
                    );
                    return None;
                }
            },
            _ => return None,
        };
        self.using(rules::COLLECTION_LOCATION);
        Self::collection_count(w, format!("{name}\tanswered"));
        Some(reply)
    }
}

impl Host for QuestHost<'_> {
    fn call(&mut self, c: &NativeCall, w: &mut World) -> Reply {
        let full = c.full_name();
        // Collection aliases, when a caller turned the reading on
        // (`quest_collection_alias_location_refs_v1`); off, recorded as
        // before.
        if let Some(r) = self.collection_native(c, w) {
            return r;
        }
        // Animation events, when a caller turned them on (`crate::anim`);
        // off, the natives are recorded as before.
        if w.anims.graphs && full == crate::anim::PLAY {
            if let Some(g) = self.graphs.as_mut() {
                let (store, max) = (self.data.store, self.data.max_memory);
                crate::anim::observe_play(store, max, g.as_mut(), c, w);
            }
        }
        // The player's base, when the caller names it and turns the reading
        // on (`vm_player_base_object_v1`), and its sex when the caller
        // assumes it; off, recorded as before.
        if let Some(r) = self.player_base_native(c, w) {
            return r;
        }
        // Menu registrations and furniture use, when a caller turned them
        // on (`crate::menu`, `crate::sit`); off, recorded as before.
        if let Some(r) = crate::menu::native(c, w) {
            self.using(rules::MENU_EVENTS);
            return r;
        }
        // Whether a menu is open and which message box button is pressed,
        // when the caller assumes them; off, recorded as before.
        if let Some(r) = crate::menu::menu_mode(c, w) {
            return r;
        }
        // The fight's registrations and the natives that read its events,
        // when a caller turned them on (`crate::fight`); off, recorded as
        // before.
        let fight_reply = crate::fight::native(c, w);
        for u in std::mem::take(&mut w.fights.used) {
            self.using(u);
        }
        if let Some(r) = fight_reply {
            return r;
        }
        // Globals, locks and the caller's item counts, when a caller turned
        // them on (`crate::runstate`); off, recorded as before.
        let rs_reply = crate::runstate::native(self.data.store, self.data.max_memory, c, w);
        for u in std::mem::take(&mut w.runstate.used) {
            self.using(u);
        }
        if let Some(r) = rs_reply {
            return r;
        }
        // Keywords of references, when a caller turned a reading on
        // (`crate::keyword`); off, recorded as before.
        let kw_reply = crate::keyword::native(self.data.store, self.data.max_memory, c, w);
        for u in std::mem::take(&mut w.keywords.used) {
            self.using(u);
        }
        if let Some(r) = kw_reply {
            return r;
        }
        let sit_reply = crate::sit::native(self.data.store, self.data.max_memory, c, w);
        for u in std::mem::take(&mut w.sits.used) {
            self.using(u);
        }
        // The furniture graph events those sits and get-ups send
        // (`anim_furniture_sit_graph_events_v1`), when the caller named them.
        for (f, event) in std::mem::take(&mut w.sits.graph_sends) {
            match self.graphs.as_mut() {
                Some(g) => {
                    let (store, max) = (self.data.store, self.data.max_memory);
                    crate::anim::furniture_event(store, max, g.as_mut(), f, &event, w);
                }
                None => w
                    .anims
                    .count("furniture graph events not sent\tno graph is read"),
            }
        }
        if let Some(r) = sit_reply {
            return r;
        }
        let anim_reply = crate::anim::native(c, w);
        for u in std::mem::take(&mut w.anims.used) {
            self.using(u);
        }
        if let Some(r) = anim_reply {
            return r;
        }
        // The player position model, when a caller gave a track
        // (`crate::track`); off, nothing changes.
        if w.place.on {
            let (store, max) = (self.data.store, self.data.max_memory);
            crate::load::observe(c, w);
            crate::track::observe(store, max, c, w);
            let got = crate::track::call(store, max, c, w);
            for u in std::mem::take(&mut w.place.used) {
                self.using(u);
            }
            if let Some((reply, rule)) = got {
                if let Some(r) = rule {
                    self.using(r);
                }
                return reply;
            }
        }
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
        if c.script == crate::talk::CALL {
            return crate::talk::call(self, c, w);
        }
        if w.scenes.on
            && (crate::scene::NATIVES.contains(&full.as_str())
                || (c.script == "scene"
                    && (c.name == crate::scene::STEP || c.name == crate::scene::MARKS)))
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
    fn reference_disabled(
        &mut self,
        w: &mut World,
        k: &FormKey,
    ) -> Option<Result<bool, &'static str>> {
        let r = self.data.reference_disabled(w, k);
        for u in self.data.rules() {
            if !self.used.contains(u) {
                self.used.push(u);
            }
        }
        r
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
            crate::scene::Step::LineFragment {
                line,
                function,
                speaker,
                ..
            } => {
                vm.rules.insert(rules::LINE_FRAGMENTS);
                let run = (|| -> Result<(String, Outcome), VmError> {
                    let key = crate::data::key_record(line).ok_or(VmError::BadArgument {
                        site: None,
                        detail: "a line fragment of no record".into(),
                    })?;
                    let o = crate::data::record_object(&mut vm.world, store, &key, "topicinfo", b)
                        .map_err(|_| VmError::BadArgument {
                            site: None,
                            detail: "the line's record could not be read".into(),
                        })?;
                    let inst = crate::scene::record_fragment_instance(vm, store, o, &mut notes, b)?;
                    let script = vm.world.instances[inst].script.clone();
                    let arg = speaker.map(Val::Object).unwrap_or(Val::None);
                    let o = if nested {
                        vm.deliver_nested(inst, function, vec![arg])
                    } else {
                        vm.deliver(inst, function, vec![arg])
                    };
                    Ok((script, o))
                })();
                match run {
                    Ok(x) => outcomes.push(x),
                    Err(e) => outcomes.push((String::new(), Outcome::Failed(e))),
                }
            }
            crate::scene::Step::LineScript {
                line,
                event,
                speaker,
            } => {
                vm.rules.insert(rules::LINE_SCRIPT_EVENTS);
                match line_script_instances(vm, store, line, &mut notes, b) {
                    Err(e) => outcomes.push((String::new(), Outcome::Failed(e))),
                    Ok(insts) => {
                        let said = vm.world.scenes.line_scripts_said.contains(line);
                        for inst in insts {
                            let script = vm.world.instances[inst].script.clone();
                            let f = match vm.find_handler(inst, event) {
                                Ok(Some(f)) => f,
                                Ok(None) => continue,
                                Err(e) => {
                                    outcomes.push((script, Outcome::Failed(e)));
                                    continue;
                                }
                            };
                            let speaker = speaker.map(Val::Object).unwrap_or(Val::None);
                            let args = match f.params.len() {
                                2 => vec![speaker, Val::Bool(said)],
                                1 => vec![speaker],
                                0 => Vec::new(),
                                _ => {
                                    outcomes.push((
                                        script,
                                        Outcome::Failed(VmError::BadArgument {
                                            site: None,
                                            detail: format!(
                                                "line event {event} takes parameters the event does not give"
                                            ),
                                        }),
                                    ));
                                    continue;
                                }
                            };
                            let o = if nested {
                                vm.deliver_nested(inst, event, args)
                            } else {
                                vm.deliver(inst, event, args)
                            };
                            outcomes.push((script, o));
                        }
                        if event == "onend" {
                            vm.world.scenes.line_scripts_said.insert(line.clone());
                        }
                    }
                }
            }
            crate::scene::Step::Enter
            | crate::scene::Step::Complete
            | crate::scene::Step::LineFields => {
                let call = match step {
                    crate::scene::Step::LineFields => crate::scene::line_fields_call(*scene),
                    _ => crate::scene::step_call(*scene, *step == crate::scene::Step::Complete),
                };
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

/// The instances of the scripts a line's record attaches of its own (its
/// VMAD's script list, not the fragment block) on the line's handle, made
/// with their property values the first time (`dialogue_line_script_events_v1`).
fn line_script_instances(
    vm: &mut Vm<'_>,
    store: &Store,
    line: &str,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<Vec<InstId>, VmError> {
    let bad = |d: String| VmError::BadArgument {
        site: None,
        detail: d,
    };
    let key = data::key_record(line).ok_or_else(|| bad("a line event of no record".into()))?;
    let o = record_object(&mut vm.world, store, &key, "topicinfo", b)
        .map_err(|_| bad("the line's record could not be read".into()))?;
    let (r, plugin) = bind::vmad_of(store, &key, b)
        .map_err(|e| bad(format!("reading the line's VMAD: {e}")))?
        .ok_or_else(|| bad("the line has no VMAD".into()))?;
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
        let v = fo4_plugin::vmad::Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)
            .map_err(|e| bad(format!("reading the line's VMAD: {e}")))?;
        for sc in &v.scripts {
            let name = fo4_pex::index::lower(sc.name);
            let existing = vm.world.objects[o]
                .instances
                .iter()
                .copied()
                .find(|i| vm.world.instances[*i].script == name);
            let inst = match existing {
                Some(i) => i,
                None => {
                    let i = vm.instantiate(Some(o), &name)?;
                    bind::apply_script(
                        vm,
                        store,
                        plugin,
                        &r.source.file,
                        i,
                        &name,
                        &sc.properties,
                        notes,
                        b,
                    )
                    .map_err(|e| bad(format!("{name}: {e}")))?;
                    i
                }
            };
            out.push(inst);
        }
    }
    Ok(out)
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
