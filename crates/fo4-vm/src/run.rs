//! A quest run as a library (`quest_run`'s code, `spec/vm.md`, Quests;
//! `spec/play.md`): every mode of the `quest_run` example, and the
//! new-game run as an object the caller steps ([`NewGame`]).
//!
//! ```text
//! quest_run MANIFEST stages OUT_DIR      every quest, every stage with a fragment
//! quest_run MANIFEST new-game OUT_DIR    the quests started with a new game
//! quest_run MANIFEST QUEST STAGE         one stage of one quest (EditorID), effect log
//! ```
//!
//! `stages` sets, for every winning quest and every stage of its record
//! that a fragment of its VMAD block names, that stage in a fresh
//! interpreter (isolated scenario, `quest_state_isolated_v1`) and runs the
//! fragments and quest events it queues, then those they queue, up to
//! [`MAX_JOBS`]. `new-game` starts every quest flagged to start with the
//! game in one interpreter (`quest_start_game_enabled_v1`) and runs what
//! that queues, up to [`MAX_GAME_JOBS`]. Both write `quest-run-census.txt`
//! (counts, type tags and native function names only) and
//! `quest-run-names.txt` (effect logs with names, never committed) to
//! OUT_DIR.
//!
//! `new-game` also takes what the caller makes happen from outside any
//! script, each counted as an assumption: `--then EDITOR_ID:STAGE`, and the
//! player actions `--assume-activate`, `--assume-trigger-enter`,
//! `--assume-trigger-leave` and `--assume-open` on a placed reference given
//! as `plugin.esm:0001ABCD` (`crate::player`); each at once after the
//! start, or with `@SECONDS` when the real-time clock reaches that second
//! (with `--scene-clock`). `--next-blockers` writes
//! `quest-run-next-blockers.txt` (counts and kinds) and
//! `quest-run-next-blockers-names.txt` (never committed): the stages the
//! stopped scenes and the caller's quests wait on and what in the data can
//! set them (`crate::setters`). The readings `--scene-recheck`,
//! `--stage-fields`, `--cond-550-clock`, `--alias-location-refs` and the
//! assumption `--assume-start-scene-actions` are off by default: without
//! them every earlier census reproduces byte for byte.
//!
//! The command line is read by [`parse`] into a [`Command`] (the switches
//! as [`Options`], [`Then`] and [`Settings`]) and run by [`Command::run`];
//! [`main_with`] does both. A new-game run with the scene clock on is a
//! [`NewGame`]: without a [`Stepper`] its clock loop runs to the end at
//! once (`quest_run`); a stepper (the play mode, `fo4-play`) gives it the
//! player's track and actions slice by slice with [`NewGame::advance_to`].
//! The voice files' seconds come from the caller ([`HeaderSeconds`]).
use crate::events::CustomOrder;
use crate::{
    bind::{self, outcome_text, RecordScripts, VfsLibrary},
    data::Answer,
    quest::{self, Change, Job, JobRun, QuestHost, QuestInfo},
    scene::{self, Step},
    voice, Effect, Origin, Outcome, Vm, World,
};
use fo4_core::{Budget, Manifest};
use fo4_plugin::story::Tree;
use fo4_plugin::{typed::Store, FormKey};
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, fmt::Write as _, path::Path, rc::Rc};

mod caller;
mod cli;
mod game;
mod next;
mod report;
mod setup;
mod sweep;

use caller::*;
use next::*;
use report::*;
use setup::*;
use sweep::*;

pub use cli::{main_with, parse, Command, Settings};
pub use game::{InputCounts, NewGame, Stepper, Stepping};

/// The error of a run: a typed error of a library or a named message.
pub type E = Box<dyn std::error::Error>;

/// Counts by text, written as the census.
pub type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

/// Jobs run after one stage of the sweep.
pub const MAX_JOBS: usize = 256;
/// Jobs run after a new game starts.
pub const MAX_GAME_JOBS: usize = 100_000;
/// The next stages `--next-blockers` lists for a quest the caller set a
/// stage on.
pub const NEXT_STAGES: usize = 3;

/// The seconds a voice file's header declares, read by the caller (the
/// library takes them from its caller: `quest_run` and the play mode use
/// `fo4_audio::header_seconds`, `audio_xwma_dpds_seconds_v1`): the file's
/// bytes, its name in the archive, the budget of the read.
pub type HeaderSeconds<'a> = &'a dyn Fn(&[u8], &str, &Budget) -> Option<f64>;

/// Switches that reproduce earlier censuses.
#[derive(Clone, Copy)]
pub struct Options {
    /// Nested reading of `quest_stage_fragment_order_v1` (else queued).
    pub nested: bool,
    /// Evaluate log entry conditions (else every conditioned entry is
    /// undetermined, as before conditions were read).
    pub conditions: bool,
    /// Deliver OnStageSet (`quest_on_stage_set_v1`).
    pub stage_event: bool,
    /// Deliver remote events the run raises (`vm_remote_events_v1`).
    pub remote: bool,
    /// Deliver custom events (`vm_custom_events_v1`), queued or nested
    /// (`vm_custom_event_order_v1`).
    pub custom: Option<CustomOrder>,
    /// Offer the script event to the story manager (`sm_script_event_v1`).
    pub story: bool,
    /// `IsDisabled` follows enable parent chains
    /// (`vm_host_is_disabled_chain_v1`).
    pub enable_chain: bool,
    /// Answer the scene natives and run scenes (`scene_start_v1`,
    /// `scene_phase_progression_v1`).
    pub scenes: bool,
    /// Start a quest's scenes with FNAM bit 0x1 with the quest
    /// (`scene_begin_on_quest_start_v1`).
    pub scenes_with_quest: bool,
    /// The SCQS fields of scenes and phases set stages
    /// (`record_set_stage_fields_v1`, `--stage-fields`; off by default).
    pub stage_fields: bool,
    /// Function 550 reads the actions the scene clock completed
    /// (`cond_fn_550_clock_actions_v1`, `--cond-550-clock`; off by
    /// default).
    pub action_conditions: bool,
    /// Reference aliases filled from their location's special references
    /// (`quest_alias_location_ref_v1`, `--alias-location-refs`; off by
    /// default).
    pub location_fills: bool,
    /// The TIQS of a line a timed dialogue action plays when the records
    /// leave one line (`scene_line_stage_fields_v1`, `--line-stage-fields`;
    /// off by default).
    pub line_fields: bool,
    /// What the lines the run plays do besides their stage fields: their
    /// fragments and their start scene (`dialogue_line_fragments_v1`,
    /// `dialogue_line_start_scene_v1`, `--line-fragments`,
    /// `--line-start-scene`; each off by default).
    pub lines: crate::line::Switches,
    /// Array opcodes on a none array (`pex_array_none_*_v1`,
    /// `--none-array-length`, `--none-array-read`, `--none-array-find`,
    /// `--none-array-change`; each off by default).
    pub none_arrays: crate::NoneArrays,
    /// A flag 0x800 below an enable chain's root carries no state of its
    /// own (`xesp_child_own_flag_ignored_v1`,
    /// `--enable-child-flag-ignored`; off by default).
    pub child_flag_ignored: bool,
    /// Function 131 reads how far an action has run on the scene clock
    /// (`cond_fn_131_action_percent_v1`, `--cond-131-clock`; off by
    /// default).
    pub action_percent: bool,
    /// A phase with completion conditions ends when they hold while its
    /// timed actions run (`scene_phase_conditions_end_v1`,
    /// `--phase-conditions-end`; off by default).
    pub conditions_end: bool,
    /// `==` on texts that differ only in ASCII case is true
    /// (`pex_text_equal_ignore_case_v1`, `--text-equal-ignore-case`; off by
    /// default).
    pub texts_ignore_case: bool,
    /// An instance of a script declaring no auto state starts in its
    /// nearest ancestor's (`pex_auto_state_inherited_v1`,
    /// `--auto-state-inherited`; off by default).
    pub auto_state_inherited: bool,
    /// A property on an object without an instance of its script runs on
    /// one made for it when the script's chain declares no variable
    /// (`pex_property_implicit_instance_v1`, `--property-implicit-instance`;
    /// off by default).
    pub property_implicit_instance: bool,
}

/// What `new-game` is told to do beyond the rules, on the command line:
/// stages set from outside any script after the start (`--then
/// EDITOR_ID:STAGE`, repeated) and queued timers fired after the jobs
/// (`--timers N`). Neither is a rule: they are assumptions of the person
/// running it, and the census says when they were made.
#[derive(Default)]
pub struct Then {
    /// What the caller makes happen from outside any script, in
    /// command-line order: stages set (`--then`) and player actions
    /// (`--assume-activate`, `--assume-trigger-enter`,
    /// `--assume-trigger-leave`), each at once after the start, or at a
    /// second of the real-time clock (`@SECONDS`, with the scene clock).
    pub actions: Vec<Caller>,
    /// Write the stages the stopped scenes wait on and what in the data
    /// can set them (`--next-blockers`).
    pub next_blockers: bool,
    /// Offer again the scenes stopped on conditions after the quest state
    /// changed (`--scene-recheck`, `scene_condition_recheck_v1`), with the
    /// length of the quest log at the last offer.
    pub recheck: Option<std::cell::Cell<usize>>,
    /// Offer again the scenes stopped on conditions after a quest script
    /// variable one of those conditions reads was written
    /// (`--scene-recheck-variables`, `scene_condition_recheck_variable_v1`).
    pub recheck_variables: bool,
    /// The stage-setter table of `--next-blockers` follows calls from other
    /// scripts into a quest script's functions (`--setters-outer-calls`,
    /// `setter_outer_call_entry_v1`).
    pub outer_calls: bool,
    pub timers: usize,
    /// Order game-time timers against real-time ones through the
    /// time-scale global (`vm_timer_time_scale_v1`), when timers are fired:
    /// the editor ID of that global, named by the caller
    /// (`--time-scale-global`); `None` keeps the earlier stop.
    pub time_scale: Option<String>,
    /// Advances of the scene clock (`--scene-clock N`), interleaved with
    /// the timers by due time; 0 leaves the clock off.
    pub scene_clock: usize,
    /// The caller's track of the player (`--track FILE`, a caller
    /// assumption) and the world model's switches (`crate::track`).
    pub track: Option<(crate::track::Track, crate::track::Switches)>,
    /// The caller's assumed actor moves (`--assume-actor-at`).
    pub actor_moves: Vec<crate::track::ActorMove>,
    /// Dialogue outside scenes (`crate::talk`): the switches, the
    /// player's base and the hello distance setting the caller names
    /// (`--player-base`, `--hello-distance-setting`).
    pub talk: crate::talk::Switches,
    pub player_base: Option<String>,
    pub hello_setting: Option<String>,
    /// Lines the caller assumes play where the choice is not settled
    /// (`--assume-line`).
    pub assume_lines: Vec<String>,
    /// Reference aliases filled by a unique actor
    /// (`quest_alias_unique_actor_ref_v1`, `--alias-unique-actors`).
    pub unique_fills: bool,
    /// The player is no record's handle (`vm_player_no_record_v1`,
    /// `--player-no-record`).
    pub player_no_record: bool,
    /// Animation events (`crate::anim`): registrations kept and
    /// delivered (`--anim-events`), names compared ignoring case
    /// (`--anim-event-case`), references' graphs followed
    /// (`--anim-graphs`) and their sequences' text keys raised
    /// (`--anim-graph-keys`).
    pub anim: [bool; 4],
    /// A graph read when first needed has played its start sequence
    /// (`anim_graph_start_settled_v1`, `--anim-graph-start-settled`), and a
    /// state's sequence name found ignoring case
    /// (`anim_graph_sequence_name_case_v1`, `--anim-sequence-case`).
    pub anim_start_settled: bool,
    pub anim_sequence_case: bool,
    /// Load events (`crate::load`): `--load-in-cell`,
    /// `--load-on-arrival`, `--load-grid N`.
    pub loads: crate::load::Switches,
    /// Repeated rows of a location's special references naming one
    /// reference count once (`quest_alias_location_ref_distinct_v1`,
    /// `--location-refs-distinct`).
    pub location_distinct: bool,
    /// Aliases sharing a location and ref type hold its distinct references
    /// one each (`quest_alias_location_ref_shared_v1`,
    /// `--location-refs-shared`).
    pub location_shared: bool,
    /// The fight's readings (`crate::fight`): hit registrations kept and the
    /// caller's hits delivered (`--hit-events`), registrations of init
    /// handlers the run never delivers read as made
    /// (`--hit-init-registrations`), the caller's combat state changes
    /// and deaths delivered (`--combat-state-events`, `--death-events`),
    /// `IsDead` on the run's dead (`--dead-state`), `IsInCombat` and
    /// `GetCombatState` from the run's states (`--combat-state-natives`).
    pub hit_events: bool,
    pub hit_init: bool,
    pub combat_events: bool,
    pub death_events: bool,
    pub dead_state: bool,
    pub combat_natives: bool,
    /// Globals the run sets answer their new value
    /// (`vm_global_runtime_value_v1`, `--global-values`), locks the run
    /// sets and the stored lock record (`vm_lock_state_v1`,
    /// `--lock-state`), and the caller's item counts
    /// (`--assume-item-count HOLDER|player ITEM N`) (`crate::runstate`).
    pub global_values: bool,
    /// A global set to an undetermined value answers undetermined until a
    /// known value is set (`vm_global_runtime_value_undetermined_v1`,
    /// `--global-value-undetermined`, with `--global-values`).
    pub global_value_undetermined: bool,
    /// The run state's counts and log (globals, locks, item counts) written
    /// to the census and the names file (`--census-run-state`; output
    /// only).
    pub census_run_state: bool,
    pub lock_state: bool,
    pub lock_events: bool,
    pub item_counts: Vec<((String, String), i32)>,
    /// A script's `Lock` that changes the held lock raises its event
    /// (`vm_lock_state_event_script_v1`, `--lock-events-script`, with
    /// `--lock-state`).
    pub lock_events_script: bool,
    /// The caller's terminal pick runs the item's fragment
    /// (`terminal_menu_item_fragment_v1`, `--terminal-fragments`;
    /// `crate::terminal`).
    pub terminal_fragments: bool,
    /// The inventory readings (`crate::inventory`): the caller's item added
    /// delivered to the filters kept (`inventory_item_added_event_v1`,
    /// `--item-added-events`), init handlers' filters read as kept
    /// (`inventory_filter_init_registration_v1`, `--inventory-filter-init`)
    /// and the holder's count raised (`inventory_item_added_count_v1`,
    /// `--item-added-counts`).
    pub item_added_events: bool,
    pub inventory_filter_init: bool,
    pub item_added_counts: bool,
    /// `HasPerk` on the player from its base's `PRKR`
    /// (`actor_perk_base_list_v1`, `--perks-base`, with
    /// `--player-base-object`).
    pub perks_base: bool,
    /// `IsInFaction` from the actor's base `SNAM` and the run's faction
    /// changes (`actor_faction_base_list_v1`, `--factions-base`).
    pub factions_base: bool,
    /// A created alias holds a reference the run makes for it
    /// (`quest_alias_created_reference_v1`, `--alias-created-refs`), an
    /// alias filled from another quest's alias what that alias holds when
    /// read (`quest_alias_external_live_v1`, `--alias-external`).
    pub alias_created: bool,
    pub alias_external: bool,
    /// A line's begin and end delivered to its own scripts
    /// (`dialogue_line_script_events_v1`, `--line-script-events`).
    pub line_script_events: bool,
    /// The caller's actor values (`--assume-actor-value HOLDER|player AVIF
    /// N`): what `GetValue` answers.
    pub actor_values: Vec<((String, String), f32)>,
    /// The timeline also writes the run's deaths and who sits where
    /// (`--timeline-deaths`, `--timeline-sits`; `spec/timeline.md`).
    pub timeline_deaths: bool,
    pub timeline_sits: bool,
    /// The timeline also writes the sequences the run's graphs start
    /// (`--timeline-sequences`; `spec/timeline.md`).
    pub timeline_sequences: bool,
    /// The player's sit ends when the track takes the player farther than
    /// this distance from the furniture's placed position
    /// (`vm_sit_ended_by_track_v1`, `--sit-end-walk UNITS`).
    pub sit_end_walk: Option<f64>,
    /// The placed references of a location's cells carrying the ref type
    /// the list holds no row of (`quest_alias_location_ref_placed_v1`,
    /// `--alias-location-placed-refs`).
    pub location_placed: bool,
    /// Function 1 from the world model's positions
    /// (`cond_fn_1_track_distance_v1`, `--cond-1-distance`).
    pub cond_distance: bool,
    /// The first plugin's id 0x14 as a condition's reference word is the
    /// player (`cond_player_reference_word_v1`, `--cond-player-word`).
    pub player_word: bool,
    /// Condition function 70 on the player from the caller's assumed sex
    /// (`cond_fn_70_player_sex_v1`, `--cond-70-player-sex`), or only in
    /// the records of the quests named (`--cond-70-player-sex-for QUEST`).
    pub cond_player_sex: bool,
    pub cond_player_sex_for: Vec<String>,
    /// The caller may assume the activation of a placed actor
    /// (`vm_event_activate_actor_v1`, `--activate-actors`).
    pub activate_actors: bool,
    /// Furniture use (`crate::sit`): a script's default-processing
    /// activation of a furniture sits the activator
    /// (`--activate-furniture-sits`), `IsFurnitureInUse` answered
    /// (`--furniture-in-use`).
    pub sit_events: bool,
    pub sit_activate: bool,
    pub sit_in_use: bool,
    /// A script's activation with events sits too
    /// (`vm_furniture_activate_events_sits_v1`, `--activate-furniture-events`),
    /// a script's activation of the furniture the actor sits in gets it up
    /// (`vm_furniture_activate_get_up_v1`, `--activate-furniture-get-up`), and
    /// those send the furniture's graph the caller's events
    /// (`anim_furniture_sit_graph_events_v1`, `--furniture-graph-events SIT GET_UP`).
    pub sit_activate_events: bool,
    pub sit_activate_get_up: bool,
    pub furniture_graph_events: Option<(String, String)>,
    /// `GetSitState` answered from the run's sitters (`vm_sit_state_v1`,
    /// `--sit-state`).
    pub sit_state: bool,
    /// The base-object natives on the player answer the caller's player
    /// base (`vm_player_base_object_v1`, `--player-base-object`), and the
    /// caller's assumed sex of it (`--assume-player-sex N`).
    pub player_base_object: bool,
    pub player_sex: Option<i32>,
    /// Menu event registrations kept and the caller's menu events
    /// delivered (`crate::menu`, `--menu-events`).
    pub menu_events: bool,
    /// Keywords of references (`crate::keyword`): read from the records
    /// (`vm_keyword_records_v1`, `--keywords-records`) and changed by the
    /// scripts (`vm_keyword_runtime_v1`, `--keywords-runtime`).
    pub keywords_records: bool,
    pub keywords_runtime: bool,
    /// Collection aliases filled from their location's rows
    /// (`quest_collection_alias_location_refs_v1`,
    /// `--collection-location-refs`).
    pub collection_fills: bool,
    /// The value every made animation event registration answers: a caller
    /// assumption (`--assume-anim-registration true|false`).
    pub anim_registration: Option<bool>,
    /// The value `Utility.IsInMenuMode` answers: a caller assumption
    /// (`--assume-menu-mode true|false`).
    pub menu_mode: Option<bool>,
    /// The button `Message.Show` answers: a caller assumption
    /// (`--assume-message-button N`).
    pub message_button: Option<i32>,
    /// The value `Actor.IsInCombat` answers on the player: a caller
    /// assumption (`--assume-player-in-combat true|false`).
    pub player_combat: Option<bool>,
    /// `--action-after` gates that named no timed caller action (counted).
    pub unmatched_gates: u64,
}

/// One thing the caller makes happen.
#[derive(Clone, Debug)]
pub enum What {
    /// A stage set from outside any script (`--then EDITOR_ID:STAGE`).
    Stage(String, i32),
    /// A player action on a placed reference given by its key
    /// (`plugin.esm:0001ABCD`).
    Player(crate::player::Action, String),
    /// The player starts a conversation with a placed actor
    /// (`--assume-talk`).
    Talk(String),
    /// A placed reference raises a named animation event
    /// (`--assume-anim-event REF EVENT`).
    Anim(String, String),
    /// The player sits in (`true`) or gets up from a furniture reference
    /// (`--assume-sit`, `--assume-get-up`).
    Sit(String, bool),
    /// A menu of the caller's name opens (`true`) or closes
    /// (`--assume-menu NAME open|close`).
    Menu(String, bool),
    /// The appearance menu raises its event with the caller's number
    /// (`--assume-looks-menu-event N`).
    Looks(i32),
    /// The player hits a placed actor (`--assume-hit REF`).
    Hit(String),
    /// A placed actor's combat state changes toward a target, the player or
    /// a placed actor (`--assume-combat-state REF TARGET STATE`).
    Combat(String, String, i32),
    /// A placed actor dies with a killer, the player or a placed actor
    /// (`--assume-death REF KILLER`).
    Death(String, String),
    /// The player unlocks a placed reference (`--assume-unlock REF`).
    Unlock(String),
    /// The player picks the menu item of an id on a placed terminal
    /// (`--assume-terminal-item REF ID`).
    Terminal(String, u32),
    /// The player puts a count of an item (a base form) into a holder, from
    /// a source (`player` or `none`) (`--assume-item-added HOLDER ITEM N
    /// SOURCE`).
    ItemAdded(String, String, i32, String),
}
#[derive(Clone, Debug)]
pub struct Caller {
    pub what: What,
    /// The real-time second it happens at; `None`: at once after the start.
    pub at: Option<f64>,
    /// The caller's gate (`--action-after`): the action happens at its
    /// second only when this stage of the quest of that editor ID is done
    /// by then, else as soon as it is done, never when it never is.
    pub after: Option<(String, i32)>,
}
impl Caller {
    /// What `--action-after` names the action by: the reference key, the
    /// menu name, the looks event number or `EDITOR_ID:STAGE`.
    pub fn target(&self) -> String {
        match &self.what {
            What::Stage(q, n) => format!("{q}:{n}"),
            What::Player(_, k)
            | What::Talk(k)
            | What::Anim(k, _)
            | What::Sit(k, _)
            | What::Hit(k)
            | What::Combat(k, _, _)
            | What::Death(k, _)
            | What::Unlock(k)
            | What::Terminal(k, _)
            | What::ItemAdded(k, ..) => k.clone(),
            What::Menu(m, _) => m.clone(),
            What::Looks(n) => n.to_string(),
        }
    }
}
