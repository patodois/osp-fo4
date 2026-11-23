//! Names of the pending rules (`spec/hypotheses.tsv`) the interpreter, the
//! recording host and the binding rely on. A run lists the ones it used.

/// Opcodes 1 to 9, 11, 12.
pub const ARITHMETIC: &str = "pex_ops_arithmetic_v1";
/// Opcodes 15 to 19.
pub const COMPARE: &str = "pex_ops_compare_v1";
/// Opcodes 10, 13, 14.
pub const NOT_COPY_CAST: &str = "pex_ops_not_copy_cast_v1";
/// Opcodes 20 to 22.
pub const JUMPS: &str = "pex_conditional_jump_sense_v1";
/// Opcodes 23 to 25.
pub const CALLS: &str = "pex_call_dispatch_v1";
/// Opcode 26.
pub const RETURN: &str = "pex_op_return_v1";
/// Opcode 27.
pub const CONCAT: &str = "pex_op_string_concat_v1";
/// Opcodes 28, 29.
pub const PROPERTIES: &str = "pex_property_get_set_v1";
/// Opcodes 30 to 34, 40, 42, 43, 45, 46.
pub const ARRAYS: &str = "pex_array_ops_v1";
/// Opcodes 36 to 39.
pub const STRUCTS: &str = "pex_struct_ops_v1";
/// Initial values of locals, of variables and members stored as none, and
/// of new array elements.
pub const DEFAULTS: &str = "pex_default_values_v1";
/// Truth of a value tested by a conditional jump, opcode 10 or a cast to
/// Bool.
pub const TRUTH: &str = "pex_truth_of_values_v1";
/// The `::State` identifier and the auto state.
pub const STATE: &str = "pex_state_identifier_v1";
/// Object identity: one handle per record, distinct records distinct.
pub const IDENTITY: &str = "vm_object_identity_v1";
/// The recording host's player handle.
pub const HOST_PLAYER: &str = "vm_host_player_handle_v1";
/// The recording host's timer queue.
pub const TIMERS: &str = "vm_timer_events_v1";
/// Which function an event name runs, with which arguments.
pub const EVENTS: &str = "vm_event_handlers_v1";
/// Scripts of a placed reference: base VMAD, then reference VMAD.
pub const VMAD_SCRIPTS: &str = "vmad_reference_scripts_v1";
/// A VMAD property value sets the property's variable before any event.
pub const VMAD_VALUES: &str = "vmad_property_sets_variable_v1";
/// The viewer hides a reference disabled by a run and shows one enabled.
pub const DISABLE_HIDES: &str = "vm_disable_hides_reference_v1";

/// The data host answers from the data as at the start of a game.
pub const DATA_STATE: &str = "vm_host_data_state_v1";
/// `GetLinkedRef` from XLKR.
pub const LINKED_REF: &str = "vm_host_linked_ref_xlkr_v1";
/// `GetLinkedRefChain`, `CountLinkedRefChain` by following XLKR.
pub const LINKED_REF_CHAIN: &str = "vm_host_linked_ref_chain_v1";
/// `IsDisabled` from the record's initially-disabled flag.
pub const IS_DISABLED: &str = "vm_host_is_disabled_flag_v1";
/// `GetBaseObject` from NAME.
pub const BASE_OBJECT: &str = "vm_host_base_object_name_v1";
/// `GetFormFromFile` through the load order.
pub const FORM_FROM_FILE: &str = "vm_host_form_from_file_v1";
/// `GlobalVariable.GetValue` from FLTV.
pub const GLOBAL_VALUE: &str = "vm_host_global_fltv_v1";
/// `FormList.HasForm`, `GetSize`, `GetAt` from LNAM.
pub const FORM_LIST: &str = "vm_host_formlist_lnam_v1";
/// A record handle is of its record type's engine type.
pub const SIGNATURE_TYPE: &str = "vm_record_signature_type_v1";
/// The scripts of a quest, an alias or a placed reference are attached to
/// its handle when a run first needs them.
pub const ATTACH: &str = "vm_record_scripts_attach_v1";
/// A VMAD object value with an alias index names that alias of the quest.
pub const ALIAS_VALUE: &str = "vmad_alias_value_v1";

/// The QUST record's subrecord order and field layouts (`fo4_plugin::quest`).
pub const QUEST_LAYOUT: &str = "quest_qust_layout_v1";
/// A VMAD stage fragment names a stage of the record and one of its log
/// entries by position.
pub const QUEST_FRAGMENT_ENTRY: &str = "quest_fragment_entry_v1";
/// The fragment script of a quest's VMAD block is one instance on the
/// quest's handle, with the block's property values.
pub const QUEST_FRAGMENT_INSTANCE: &str = "quest_fragment_instance_v1";
/// What setting a stage does: done, current, quest started, fragment.
pub const QUEST_SET_STAGE: &str = "quest_set_stage_v1";
/// Which log entry's fragment runs when a stage is set.
pub const QUEST_LOG_ENTRY: &str = "quest_stage_log_entry_v1";
/// Fragments and quest events run at once, inside the native call that
/// brought them (nested reading).
pub const QUEST_ORDER: &str = "quest_stage_fragment_order_v1";
/// Setting a stage already done.
pub const QUEST_REPEATED_STAGE: &str = "quest_repeated_stage_v1";
/// Starting a quest: running, OnQuestInit, startup stages.
pub const QUEST_START: &str = "quest_start_v1";
/// Stopping a quest: not running, OnQuestShutdown.
pub const QUEST_STOP: &str = "quest_stop_v1";
/// Objectives displayed, completed and failed as set.
pub const QUEST_OBJECTIVES: &str = "quest_objectives_v1";
/// A reference alias holds its forced reference while its quest runs.
pub const QUEST_ALIAS_FORCED: &str = "quest_alias_forced_reference_v1";
/// The quests running at the start of a new game.
pub const QUEST_START_GAME: &str = "quest_start_game_enabled_v1";
/// The stage sweep's state: everything it does not set is undetermined.
pub const QUEST_ISOLATED: &str = "quest_state_isolated_v1";

/// OnStageSet delivered to the quest's scripts after the stage's fragment.
pub const QUEST_ON_STAGE_SET: &str = "quest_on_stage_set_v1";
/// Log entry conditions are evaluated in the state after the stage is set.
pub const QUEST_LOG_ENTRY_STATE: &str = "cond_log_entry_state_v1";

/// The CTDA layout read by `fo4_plugin::condition`.
pub const COND_LAYOUT: &str = "cond_ctda_layout_v1";
/// How a condition list combines: AND of OR groups.
pub const COND_LIST: &str = "cond_list_or_groups_v1";
/// Operator codes 0 to 5 compare the function's value with the comparison
/// value.
pub const COND_OPS: &str = "cond_compare_ops_v1";
/// The global bit: the comparison value is the global's value.
pub const COND_GLOBAL: &str = "cond_use_global_v1";
/// Flag bits 0x02 and 0x08 make parameters name aliases or package data.
pub const COND_FLAGS: &str = "cond_flag_parameters_v1";
/// Function 56: whether the quest of parameter 1 runs.
pub const COND_FN_56: &str = "cond_fn_56_quest_running_v1";
/// Function 58: the current stage of the quest of parameter 1.
pub const COND_FN_58: &str = "cond_fn_58_quest_stage_v1";
/// Function 59: whether stage parameter 2 of the quest of parameter 1 is
/// done.
pub const COND_FN_59: &str = "cond_fn_59_stage_done_v1";
/// Function 74: the value of the global of parameter 1.
pub const COND_FN_74: &str = "cond_fn_74_global_value_v1";

/// Function 576 with selector 0: whether a member of the story event
/// being offered is the record of parameter 2.
pub const COND_FN_576: &str = "cond_fn_576_event_data_v1";

/// The run-on word: 2 the reference word, 5 an alias of the owning quest.
pub const COND_RUN_ON: &str = "cond_run_on_subject_v1";
/// Function 72: whether the base of the subject reference is parameter 1.
pub const COND_FN_72: &str = "cond_fn_72_reference_base_v1";
/// Function 84: how many actors of the base of parameter 1 died.
pub const COND_FN_84: &str = "cond_fn_84_dead_count_v1";
/// Function 248: whether the scene of parameter 1 plays.
pub const COND_FN_248: &str = "cond_fn_248_scene_playing_v1";
/// Function 550: whether action parameter 2 of the scene of parameter 1 is
/// complete.
pub const COND_FN_550: &str = "cond_fn_550_scene_action_complete_v1";
/// Function 629: the value of the quest variable CIS2 names.
pub const COND_FN_629: &str = "cond_fn_629_quest_variable_v1";

/// Remote events: registration and delivery of the events the run raises.
pub const REMOTE_EVENTS: &str = "vm_remote_events_v1";
/// Custom events: registration and delivery.
pub const CUSTOM_EVENTS: &str = "vm_custom_events_v1";
/// When a custom event is delivered relative to the sender (queued).
pub const CUSTOM_EVENT_ORDER: &str = "vm_custom_event_order_v1";

/// The story manager node records' order grammar and fields.
pub const SM_LAYOUT: &str = "sm_node_layout_v1";
/// How an event is offered to its tree of nodes.
pub const SM_TREE_WALK: &str = "sm_tree_walk_v1";
/// What a quest node does when the walk reaches it.
pub const SM_QUEST_START: &str = "sm_quest_node_start_v1";
/// `Keyword.SendStoryEvent` raises the script event.
pub const SM_SCRIPT_EVENT: &str = "sm_script_event_v1";

/// `IsDisabled` of a reference with an enable parent follows its chain.
pub const IS_DISABLED_CHAIN: &str = "vm_host_is_disabled_chain_v1";
/// The enable parent rules the chain answer relies on
/// (`spec/enable-parent.md`).
pub const XESP_FOLLOWS: &str = "xesp_child_follows_parent_v1";
pub const XESP_INITIAL: &str = "xesp_initial_state_chain_v1";
pub const XESP_OPPOSITE: &str = "xesp_flag_bit0_opposite_v1";
pub const XESP_BIT1: &str = "xesp_flag_bit1_not_presence_v1";
pub const XESP_PROPAGATES: &str = "xesp_script_change_propagates_v1";

/// Every rule above.
pub const ALL: [&str; 197] = [
    ARITHMETIC,
    COMPARE,
    NOT_COPY_CAST,
    JUMPS,
    CALLS,
    RETURN,
    CONCAT,
    PROPERTIES,
    ARRAYS,
    STRUCTS,
    DEFAULTS,
    TRUTH,
    STATE,
    IDENTITY,
    HOST_PLAYER,
    TIMERS,
    EVENTS,
    VMAD_SCRIPTS,
    VMAD_VALUES,
    DISABLE_HIDES,
    DATA_STATE,
    LINKED_REF,
    LINKED_REF_CHAIN,
    IS_DISABLED,
    BASE_OBJECT,
    FORM_FROM_FILE,
    GLOBAL_VALUE,
    FORM_LIST,
    SIGNATURE_TYPE,
    ATTACH,
    ALIAS_VALUE,
    QUEST_LAYOUT,
    QUEST_FRAGMENT_ENTRY,
    QUEST_FRAGMENT_INSTANCE,
    QUEST_SET_STAGE,
    QUEST_LOG_ENTRY,
    QUEST_ORDER,
    QUEST_REPEATED_STAGE,
    QUEST_START,
    QUEST_STOP,
    QUEST_OBJECTIVES,
    QUEST_ALIAS_FORCED,
    QUEST_START_GAME,
    QUEST_ISOLATED,
    QUEST_ON_STAGE_SET,
    QUEST_LOG_ENTRY_STATE,
    COND_LAYOUT,
    COND_LIST,
    COND_OPS,
    COND_GLOBAL,
    COND_FLAGS,
    COND_FN_56,
    COND_FN_58,
    COND_FN_59,
    COND_FN_74,
    COND_FN_576,
    COND_RUN_ON,
    COND_FN_72,
    COND_FN_84,
    COND_FN_248,
    COND_FN_550,
    COND_FN_629,
    REMOTE_EVENTS,
    CUSTOM_EVENTS,
    CUSTOM_EVENT_ORDER,
    SM_LAYOUT,
    SM_TREE_WALK,
    SM_QUEST_START,
    SM_SCRIPT_EVENT,
    IS_DISABLED_CHAIN,
    XESP_FOLLOWS,
    XESP_INITIAL,
    XESP_OPPOSITE,
    XESP_BIT1,
    XESP_PROPAGATES,
    FRAGMENT_BLOCKS,
    FRAGMENT_INSTANCE,
    SCEN_LAYOUT,
    SCENE_START,
    SCENE_PHASES,
    SCENE_BEGIN_ON_QUEST_START,
    OWNING_QUEST,
    TIME_SCALE,
    SCENE_CLOCK,
    TIMER_ACTION,
    LINE_AUDIO,
    LINE_CHOICE,
    VOICE_PATH,
    SPEAKER_VOICE_TYPE,
    SHARED_RESPONSE,
    DPDS_SECONDS,
    PLAYER_CHOICE,
    EVENT_ALIASES,
    SETTER_EVENT_ENTRY,
    SETTER_SCAN,
    SETTER_FIELDS,
    SCENE_RECHECK,
    COND_FN_550_CLOCK,
    START_SCENE_ACTION,
    ALIAS_LOCATION_REF,
    EVENT_OPEN,
    NONE_ARRAY_LENGTH,
    NONE_ARRAY_READ,
    NONE_ARRAY_FIND,
    NONE_ARRAY_CHANGE,
    LINE_STAGE_FIELDS,
    WORLD_TRIGGER_BOX,
    WORLD_TRIGGER_SPHERE,
    WORLD_TRIGGER_POINT,
    WORLD_TRIGGER_BODY,
    WORLD_TRIGGER_INSTANT,
    WORLD_TRIGGER_ENABLED,
    WORLD_TRIGGER_ORDER,
    WORLD_DISTANCE_EVENTS,
    WORLD_DISTANCE_ONCE,
    WORLD_PLACED_POSITION,
    WORLD_POSITION_NATIVES,
    WORLD_CELL_NATIVES,
    WORLD_LOCATION_NATIVE,
    ALIAS_UNIQUE_ACTOR,
    WORLD_ACTOR_POSITION,
    WORLD_ACTOR_TRIGGER,
    DIALOGUE_PARTIES,
    DIALOGUE_LINE_CHOICE,
    DIALOGUE_LINE_PRIORITY,
    DIALOGUE_GREETING,
    DIALOGUE_HELLO,
    DIALOGUE_LINE_FIELDS,
    PLAYER_NO_RECORD,
    XESP_CHILD_FLAG_IGNORED,
    ANIM_EVENTS,
    ANIM_EVENT_CASE,
    ANIM_GRAPH,
    ANIM_GRAPH_KEYS,
    PACKAGE_DESTINATION,
    WORLD_MOVETO_TARGET,
    WORLD_LOAD_ENTERS,
    WORLD_LOAD_ARRIVES,
    WORLD_LOAD_GRID,
    COND_FN_131,
    SCENE_CONDITIONS_END,
    ALIAS_LOCATION_DISTINCT,
    ALIAS_LOCATION_PLACED,
    COND_FN_1_TRACK,
    COND_PLAYER_WORD,
    EVENT_ACTIVATE_ACTOR,
    SIT_EVENTS,
    SIT_ACTIVATE,
    SIT_IN_USE,
    MENU_EVENTS,
    PLAYER_BASE_OBJECT,
    KEYWORD_RECORDS,
    KEYWORD_RUNTIME,
    COLLECTION_LOCATION,
    SCENE_ACTION_SKIPPED_START,
    SCENE_RECHECK_VARIABLE,
    SETTER_OUTER_CALL,
    SIT_ACTIVATE_EVENTS,
    SIT_ACTIVATE_GET_UP,
    FURNITURE_GRAPH_EVENTS,
    SIT_STATE,
    ANIM_GRAPH_START_SETTLED,
    TEXT_EQUAL_IGNORE_CASE,
    ANIM_SEQUENCE_CASE,
    WORLD_LOAD_DOOR,
    LINE_FRAGMENTS,
    LINE_START_SCENE,
    COND_FN_70_PLAYER_SEX,
    INFO_GROUPS,
    ALIAS_LOCATION_SHARED,
    HIT_EVENTS,
    HIT_INIT_REGISTRATION,
    COMBAT_STATE_EVENTS,
    DEATH_EVENTS,
    DEAD_STATE,
    COMBAT_STATE_NATIVES,
    AUTO_STATE_INHERITED,
    PROPERTY_IMPLICIT_INSTANCE,
    GLOBAL_RUNTIME_VALUE,
    LOCK_STATE,
    WALK_NAVMESH_SURFACE,
    WALK_DOOR_BOUNDS_BLOCK,
    WALK_DOOR_LOCK_RECORD_CLOSED,
    NPC_RESPONSE_CHOICE,
    LOCK_STATE_EVENT,
    GLOBAL_VALUE_UNDETERMINED,
    TERMINAL_FRAGMENT,
    LOCK_STATE_EVENT_SCRIPT,
    ITEM_ADDED_EVENT,
    INVENTORY_FILTER_INIT,
    ITEM_ADDED_COUNT,
    PERK_BASE_LIST,
    FACTION_BASE_LIST,
    ALIAS_CREATED_REFERENCE,
    ALIAS_EXTERNAL_LIVE,
    LINE_SCRIPT_EVENTS,
    SIT_ENDED_BY_TRACK,
];

/// The rule an opcode's meaning rests on; `None` for an opcode without a
/// candidate meaning (the run stops on it).
pub fn of_opcode(op: u8) -> Option<&'static str> {
    Some(match op {
        1..=9 | 11 | 12 => ARITHMETIC,
        10 | 13 | 14 => NOT_COPY_CAST,
        15..=19 => COMPARE,
        20..=22 => JUMPS,
        23..=25 => CALLS,
        26 => RETURN,
        27 => CONCAT,
        28 | 29 => PROPERTIES,
        30..=34 | 40 | 42 | 43 | 45 | 46 => ARRAYS,
        36..=39 => STRUCTS,
        _ => return None,
    })
}

/// The VMAD fragment blocks of INFO, PACK, SCEN and PERK and the slot
/// each fragment attaches to.
pub const FRAGMENT_BLOCKS: &str = "vmad_fragment_blocks_v1";
/// A fragment block's script instance on its record's handle, with the
/// block's property values.
pub const FRAGMENT_INSTANCE: &str = "vmad_fragment_instance_v1";
/// The SCEN record layout.
pub const SCEN_LAYOUT: &str = "scen_record_layout_v1";
/// `Scene.Start`, `ForceStart`, `Stop`, `IsPlaying` on the run's state.
pub const SCENE_START: &str = "scene_start_v1";
/// Phases offered in order, entered and completed where the data decides.
pub const SCENE_PHASES: &str = "scene_phase_progression_v1";
/// FNAM bit 0x1: the scene starts when its quest starts.
pub const SCENE_BEGIN_ON_QUEST_START: &str = "scene_begin_on_quest_start_v1";
/// `Scene.GetOwningQuest` from PNAM, `TopicInfo.GetOwningQuest` from the
/// group that holds the INFO.
pub const OWNING_QUEST: &str = "vm_host_owning_quest_v1";
/// A game-time timer due after its hours times 3600 over the time-scale
/// global, against real-time timers.
pub const TIME_SCALE: &str = "vm_timer_time_scale_v1";
/// The scene clock: actions start with their start phase, hold the phases
/// they span until they complete, and complete when the caller advances
/// the real-time clock past their due time.
pub const SCENE_CLOCK: &str = "scene_clock_v1";
/// A timer action (kind 2) lasts the seconds its SNAM after the end phase
/// holds.
pub const TIMER_ACTION: &str = "scen_timer_action_seconds_v1";
/// A dialogue action (kind 0) completes after the duration of its line's
/// audio.
pub const LINE_AUDIO: &str = "scene_dialogue_line_audio_v1";
/// The line a topic plays: the first in stored order whose conditions hold.
pub const LINE_CHOICE: &str = "scene_dialogue_line_choice_v1";
/// The voice file path of a response.
pub const VOICE_PATH: &str = "voice_path_convention_v1";
/// The speaker's voice type from the alias, its forced reference or its
/// unique actor.
pub const SPEAKER_VOICE_TYPE: &str = "voice_speaker_voice_type_v1";
/// A line without responses plays those of the line its DNAM names.
pub const SHARED_RESPONSE: &str = "info_shared_response_v1";
/// The seconds a WMA-family voice file declares: its last `dpds` word over
/// 2 x channels x rate (read by the caller with `fo4_audio`).
pub const DPDS_SECONDS: &str = "audio_xwma_dpds_seconds_v1";
/// Player dialogue under a chosen choice: the player's topic and the
/// answer topic it pairs with, played one after the other.
pub const PLAYER_CHOICE: &str = "scene_player_dialogue_choice_v1";

/// An event of a placed reference also runs on the scripts of every quest
/// alias holding it, after the reference's own scripts.
pub const EVENT_ALIASES: &str = "vm_event_alias_scripts_v1";
/// The stage-setter table: an engine event runs the script function of its
/// name, and calls on `self` and parent calls carry it to the functions
/// they call.
pub const SETTER_EVENT_ENTRY: &str = "setter_event_entry_v1";
/// The stage-setter table: a call's quest and stage resolved through the
/// attachment's VMAD values, struct members, array elements and the calls
/// of functions passing a parameter on.
pub const SETTER_SCAN: &str = "setter_static_scan_v1";
/// The INFO field TIQS and the SCEN and phase field SCQS set a stage of
/// the owning quest.
pub const SETTER_FIELDS: &str = "record_set_stage_fields_v1";

/// A scene stopped on its phase's conditions offers the phase again when
/// the run's quest state changes.
pub const SCENE_RECHECK: &str = "scene_condition_recheck_v1";

/// Function 550 answered from the actions the scene clock completed.
pub const COND_FN_550_CLOCK: &str = "cond_fn_550_clock_actions_v1";

/// A start-scene action (kind 4) starts the scene its LCEP names.
pub const START_SCENE_ACTION: &str = "scene_start_scene_action_v1";

/// A reference alias filled from a forced location's special reference of
/// its location ref type.
pub const ALIAS_LOCATION_REF: &str = "quest_alias_location_ref_v1";

/// A placed reference opening runs OnOpen with the opener on its scripts.
pub const EVENT_OPEN: &str = "vm_event_open_v1";

/// Opcode 31 on a none array stores 0.
pub const NONE_ARRAY_LENGTH: &str = "pex_array_none_length_v1";
/// Opcode 32 on a none array stores the default of the element type and
/// the run goes on.
pub const NONE_ARRAY_READ: &str = "pex_array_none_read_v1";
/// Opcodes 34 and 40 on a none array store -1.
pub const NONE_ARRAY_FIND: &str = "pex_array_none_find_v1";
/// Opcodes 33, 42, 43, 45 and 46 on a none array change nothing and the
/// run goes on.
pub const NONE_ARRAY_CHANGE: &str = "pex_array_none_change_v1";

/// A dialogue action the scene clock times sets the TIQS stages of the
/// line it plays when the records leave one line of its topic.
pub const LINE_STAGE_FIELDS: &str = "scene_line_stage_fields_v1";

/// The registry the rules are checked against.
pub const REGISTRY: &str = include_str!("../../../spec/hypotheses.tsv");

/// Status column of a rule in the registry.
pub fn status(name: &str) -> Option<&'static str> {
    REGISTRY.lines().find_map(|row| {
        let mut c = row.split('\t');
        (c.next() == Some(name)).then(|| c.nth(4)).flatten()
    })
}

/// An activator reference's `XPRM` of kind 1 is a box: half extents the
/// three bound words times the reference's scale, centred on the
/// reference, along its rotated axes (`crate::track`).
pub const WORLD_TRIGGER_BOX: &str = "world_trigger_box_v1";
/// An activator reference's `XPRM` of kind 2 is a sphere of radius the
/// first bound word times the scale, centred on the reference.
pub const WORLD_TRIGGER_SPHERE: &str = "world_trigger_sphere_v1";
/// A trigger holds the player while the player's reference point lies in
/// its closed volume; an instant's touch raises nothing.
pub const WORLD_TRIGGER_POINT: &str = "world_trigger_point_v1";
/// A trigger holds the player while a vertical segment from the
/// player's reference point up a caller-given height meets its closed
/// volume (the alternative to `WORLD_TRIGGER_POINT`).
pub const WORLD_TRIGGER_BODY: &str = "world_trigger_body_v1";
/// Trigger enter and leave are raised at the second the hold changes on
/// the real-time clock, before what is due later.
pub const WORLD_TRIGGER_INSTANT: &str = "world_trigger_instant_v1";
/// A disabled trigger holds no one; enabling or disabling it with the
/// player inside raises enter or leave.
pub const WORLD_TRIGGER_ENABLED: &str = "world_trigger_enabled_v1";
/// Several trigger events at one instant: leaves first, then enters,
/// each in reference key order.
pub const WORLD_TRIGGER_ORDER: &str = "world_trigger_order_v1";
/// Distance registrations raise their event on the registering instance
/// at the first second the condition holds.
pub const WORLD_DISTANCE_EVENTS: &str = "world_distance_events_v1";
/// A distance registration ends when its event is raised.
pub const WORLD_DISTANCE_ONCE: &str = "world_distance_once_v1";
/// A placed reference other than an actor stands at its stored position
/// until a native of the run may have moved it.
pub const WORLD_PLACED_POSITION: &str = "world_placed_position_v1";
/// Position, distance and facing natives answered from the positions.
pub const WORLD_POSITION_NATIVES: &str = "world_position_natives_v1";
/// Parent cell, worldspace and interior natives answered from the
/// position's space and the exterior grid.
pub const WORLD_CELL_NATIVES: &str = "world_cell_natives_v1";
/// The current location is the parent cell's XLCN.
pub const WORLD_LOCATION_NATIVE: &str = "world_location_native_v1";

/// A reference alias filled by a unique actor (ALUA) holds the one placed
/// actor (ACHR) of that base in the loaded plugins; none or several leave
/// it without a reference (`crate::quest::UniqueActors`).
pub const ALIAS_UNIQUE_ACTOR: &str = "quest_alias_unique_actor_ref_v1";
/// A placed actor stands at its stored (`DATA`) position until a native
/// of the run may have moved it or a caller's assumption moves it.
pub const WORLD_ACTOR_POSITION: &str = "world_actor_placed_position_v1";
/// An actor whose position changes (a caller's assumed move) enters and
/// leaves trigger volumes as the player does, the actor the event's
/// action reference.
pub const WORLD_ACTOR_TRIGGER: &str = "world_actor_trigger_v1";
/// A dialogue line's conditions run on the speaker (run-on 0) and the one
/// spoken to (run-on 1); a conversation's lines are tried with either
/// party speaking (`crate::talk`).
pub const DIALOGUE_PARTIES: &str = "dialogue_conversation_parties_v1";
/// A line outside a scene is one line of a topic of the subtype owned by
/// a running quest: known only when exactly one holds and no other is
/// undetermined.
pub const DIALOGUE_LINE_CHOICE: &str = "dialogue_outside_line_choice_v1";
/// The alternative choice: lines ordered by their quest's priority, the
/// topic's priority and stored order; the first that holds plays when
/// every line before it is false.
pub const DIALOGUE_LINE_PRIORITY: &str = "dialogue_line_priority_v1";
/// When the player starts a conversation with an actor, a line of a
/// greeting topic (SNAM `GREE`) plays.
pub const DIALOGUE_GREETING: &str = "dialogue_greeting_on_talk_v1";
/// When the player comes within the hello distance of an enabled placed
/// actor, a line of a hello topic (SNAM `HELO`) plays.
pub const DIALOGUE_HELLO: &str = "dialogue_hello_proximity_v1";
/// A line outside a scene sets its TIQS stages as it begins and ends.
pub const DIALOGUE_LINE_FIELDS: &str = "dialogue_line_stage_fields_v1";
/// The player's handle is not the handle of any record of the loaded
/// plugins (no record is the player's reference).
pub const PLAYER_NO_RECORD: &str = "vm_player_no_record_v1";
/// A placed record below the root of its enable parent chain that
/// carries its own flag 0x800 takes its parent's state as the other links
/// do: the flag carries no state of its own.
pub const XESP_CHILD_FLAG_IGNORED: &str = "xesp_child_own_flag_ignored_v1";

/// `RegisterForAnimationEvent` keeps (instance, sender, name) until
/// `UnregisterForAnimationEvent`; an animation event of the sender the run
/// raises runs `OnAnimationEvent(sender, name)` on every instance
/// registered for that name, in registration order (`crate::anim`).
pub const ANIM_EVENTS: &str = "vm_anim_event_registration_v1";
/// Animation event names compare ignoring ASCII case, in registrations
/// and in a graph's event table; the handler receives the name as the
/// registration stored it.
pub const ANIM_EVENT_CASE: &str = "anim_event_name_case_v1";
/// A placed reference runs the behaviour graph its base model names
/// (`fo4_scene::anim_graph`), from the root machine's start state;
/// `PlayAnimation(event)` takes the current state's transition of that
/// event, else a wildcard one, entering the target state at once.
pub const ANIM_GRAPH: &str = "anim_graph_model_transition_v1";
/// A state whose generator is a Gamebryo sequence generator plays the
/// model's controller sequence of that name from its start: each text key
/// naming an event of the graph is raised at the entry plus its time over
/// the frequency, as an animation event of the reference and as an event
/// of its graph; leaving the state drops the keys not yet raised.
pub const ANIM_GRAPH_KEYS: &str = "anim_graph_sequence_keys_v1";
/// A scene package action whose package names exactly one placed
/// reference as a Location input puts the action's actor at that
/// reference's stored position a caller's number of seconds after the
/// action starts (no path, no speed).
pub const PACKAGE_DESTINATION: &str = "pack_location_destination_v1";

/// `ObjectReference.MoveTo(target, 0, 0, 0, ...)` puts the reference at
/// the target's position, in the target's space, at once.
pub const WORLD_MOVETO_TARGET: &str = "world_moveto_target_v1";
/// A reference that comes into the interior cell the player is in (moved
/// there by a native, or enabled there) loads its 3D at once: `OnLoad`.
pub const WORLD_LOAD_ENTERS: &str = "world_load_reference_enters_v1";
/// When the player reaches an interior cell (the track's point in a new
/// space), every enabled reference standing in it loads at that second.
pub const WORLD_LOAD_ARRIVES: &str = "world_load_player_arrives_v1";
/// In a worldspace, a reference within N grid squares of the player's
/// square loads when it comes into that range.
pub const WORLD_LOAD_GRID: &str = "world_load_exterior_grid_v1";
/// Condition function 131 is how far, in percent, the scene action its
/// parameters name has run.
pub const COND_FN_131: &str = "cond_fn_131_action_percent_v1";
/// A scene phase with completion conditions ends as soon as they hold,
/// its timed actions still running.
pub const SCENE_CONDITIONS_END: &str = "scene_phase_conditions_end_v1";
/// Rows of a location's special reference list that name the same
/// reference count once: an alias filled from the location by a ref type
/// whose rows all name one reference holds it.
pub const ALIAS_LOCATION_DISTINCT: &str = "quest_alias_location_ref_distinct_v1";
/// An alias filled from a forced location by a ref type the location's
/// list holds no row of holds the one placed reference of the location's
/// cells that carries the type in its `XLRT`.
pub const ALIAS_LOCATION_PLACED: &str = "quest_alias_location_ref_placed_v1";
/// Condition function 1 is the distance between the subject and the
/// parameter as the world model places them (the track, stored and moved
/// positions); a scene waiting on it is offered its conditions again when
/// the distance crosses the value.
pub const COND_FN_1_TRACK: &str = "cond_fn_1_track_distance_v1";
/// A condition's reference word naming the first plugin's object id 0x14,
/// which no plugin defines, stands for the player.
pub const COND_PLAYER_WORD: &str = "cond_player_reference_word_v1";
/// The player's activation of a placed actor, as a caller assumes it,
/// raises `OnActivate` on the actor's scripts, the aliases holding it and
/// the registered instances, the player the activator; no conversation
/// starts by it.
pub const EVENT_ACTIVATE_ACTOR: &str = "vm_event_activate_actor_v1";
/// `OnSit(furniture)` and `OnGetUp(furniture)` go to the sitter's scripts,
/// the aliases holding it (the player's aliases for the player) and its
/// registered instances; `OnExitFurniture(sitter)` to the furniture's.
pub const SIT_EVENTS: &str = "vm_event_sit_v1";
/// A script's `Activate(actor, true)` (default processing only) of a
/// furniture reference makes that actor sit in it at once.
pub const SIT_ACTIVATE: &str = "vm_furniture_activate_sits_v1";
/// `IsFurnitureInUse` is true exactly while the run has someone sitting
/// in the furniture.
pub const SIT_IN_USE: &str = "vm_furniture_in_use_v1";
/// `RegisterForMenuOpenCloseEvent` and `RegisterForLooksMenuEvent` keep
/// their registrations; a menu event the caller assumes is delivered to
/// the registered instances.
pub const MENU_EVENTS: &str = "vm_menu_events_v1";
/// The base-object natives on the player's handle answer the NPC_ the
/// caller names as the player's base.
pub const PLAYER_BASE_OBJECT: &str = "vm_player_base_object_v1";
/// `HasKeyword` on a placed reference answers from the records: its base's
/// `KWDA`, an actor base's with its race's; an absent keyword stays open
/// where the reading does not cover every source (an actor base with a
/// template, a weapon or armour base).
pub const KEYWORD_RECORDS: &str = "vm_keyword_records_v1";
/// `AddKeyword` and `RemoveKeyword` on a reference change its keywords, the
/// last change of a keyword winning over the records; `HasKeyword` answers
/// the change.
pub const KEYWORD_RUNTIME: &str = "vm_keyword_runtime_v1";
/// A collection alias filled from a forced location by a ref type holds,
/// while its quest runs, the distinct references the location's special
/// reference rows of that type name, in row order; `GetCount`, `GetAt` and
/// `Find` answer from it and `AddRef`, `RemoveRef` and `RemoveAll` change
/// it.
pub const COLLECTION_LOCATION: &str = "quest_collection_alias_location_refs_v1";
/// A scene action whose start phase the scene skipped starts with the first
/// later phase it spans that the scene enters.
pub const SCENE_ACTION_SKIPPED_START: &str = "scene_action_skipped_start_v1";
/// A scene stopped on its phase's conditions offers the phase again when a
/// quest script variable one of those conditions reads (function 629) is
/// written.
pub const SCENE_RECHECK_VARIABLE: &str = "scene_condition_recheck_variable_v1";
/// The stage-setter table follows a call from another script's function
/// into a quest script's function that no entry of its own script reaches,
/// one call outward, where the caller's attachment names that quest.
pub const SETTER_OUTER_CALL: &str = "setter_outer_call_entry_v1";
/// A script's `Activate(actor, false)` (default processing and events) of
/// a furniture reference the actor is not in makes that actor sit in it,
/// as the default-processing-only activation does.
pub const SIT_ACTIVATE_EVENTS: &str = "vm_furniture_activate_events_sits_v1";
/// A script's `Activate(actor, ...)` of the furniture reference the run has
/// that actor sitting in makes the actor get up from it.
pub const SIT_ACTIVATE_GET_UP: &str = "vm_furniture_activate_get_up_v1";
/// A sit or get-up a script's activation brings sends the furniture
/// reference's behaviour graph an event, the caller naming which.
pub const FURNITURE_GRAPH_EVENTS: &str = "anim_furniture_sit_graph_events_v1";
/// `GetSitState` on the player or a placed actor answers from the run's
/// sitters: the sitting value while the run has it in a furniture, 0
/// otherwise.
pub const SIT_STATE: &str = "vm_sit_state_v1";
/// A graph read when first needed has played its start state's sequence to
/// its end since its reference loaded: the transitions that sequence's own
/// keys take are followed at once, raising nothing to scripts.
pub const ANIM_GRAPH_START_SETTLED: &str = "anim_graph_start_settled_v1";
/// `==` on two texts that differ only in ASCII case is true.
pub const TEXT_EQUAL_IGNORE_CASE: &str = "pex_text_equal_ignore_case_v1";
/// A graph state's sequence name finds the model's sequence of that name
/// ignoring ASCII case where none has the exact name.
pub const ANIM_SEQUENCE_CASE: &str = "anim_graph_sequence_name_case_v1";
/// Where the caller's track changes space, the player passes through the
/// load door of the old space whose `XTEL` names a door in the new space
/// (the one nearest the track's last point there) and stands at the
/// position and facing the `XTEL` stores, at that point's second.
pub const WORLD_LOAD_DOOR: &str = "world_load_door_xtel_v1";
/// A line the run plays runs its fragment block's begin fragment as it
/// begins and its end fragment as it ends, on the line's fragment script
/// instance, with the speaker's reference as the argument.
pub const LINE_FRAGMENTS: &str = "dialogue_line_fragments_v1";
/// A line the run plays that stores a `TSCE` starts that scene as it ends,
/// offering first the phase its `INTV` stores.
pub const LINE_START_SCENE: &str = "dialogue_line_start_scene_v1";
/// Condition function 70 run on the reference word that stands for the
/// player compares its first parameter with the caller's assumed sex of the
/// player: 1 when equal, 0 otherwise.
pub const COND_FN_70_PLAYER_SEX: &str = "cond_fn_70_player_sex_v1";
/// A dialogue line outside scenes whose `GNAM` names a group (an INFO of
/// its topic with no responses) holds only when the group's conditions
/// hold too; a line other lines name as their group is not a line that
/// plays.
pub const INFO_GROUPS: &str = "dialogue_info_group_conditions_v1";
/// Several reference aliases of a quest naming the same location and ref
/// type, its rows naming as many distinct references of the type: the
/// aliases hold them one each, by alias id and row order.
pub const ALIAS_LOCATION_SHARED: &str = "quest_alias_location_ref_shared_v1";
/// Hit event registrations are kept; a hit the caller assumes delivers
/// `OnHit` to the instances registered for the target or an alias holding
/// it, once per registration.
pub const HIT_EVENTS: &str = "fight_hit_registrations_v1";
/// A script of the target or of an alias holding it whose init handler
/// (never delivered by the run) registers for hit events on its own
/// object counts as registered before the first hit the run raises there.
pub const HIT_INIT_REGISTRATION: &str = "fight_hit_init_registration_v1";
/// A combat state change the caller assumes delivers
/// `OnCombatStateChanged(target, state)` to the actor's receivers.
pub const COMBAT_STATE_EVENTS: &str = "fight_combat_state_events_v1";
/// A death the caller assumes delivers `OnDying(killer)` then
/// `OnDeath(killer)` to the actor's receivers.
pub const DEATH_EVENTS: &str = "fight_death_events_v1";
/// `IsDead` on an actor whose death the run raised answers true.
pub const DEAD_STATE: &str = "fight_dead_state_v1";
/// `IsInCombat` on the player and on actors, and `GetCombatState` on
/// actors, answered from the run's assumed combat states and deaths.
pub const COMBAT_STATE_NATIVES: &str = "fight_combat_state_natives_v1";
/// An instance of a script that declares no auto state starts in the auto
/// state of its nearest ancestor that declares one.
pub const AUTO_STATE_INHERITED: &str = "pex_auto_state_inherited_v1";
/// A property read or written on an object that carries no instance of the
/// declared script runs the property's functions on an instance made for
/// it, when the declared script's chain declares no variable (as a scripted
/// method call does).
pub const PROPERTY_IMPLICIT_INSTANCE: &str = "pex_property_implicit_instance_v1";
/// `GlobalVariable.SetValue` changes the value `GetValue` answers from then
/// on in the run.
pub const GLOBAL_RUNTIME_VALUE: &str = "vm_global_runtime_value_v1";
/// `ObjectReference.Lock` changes the lock `IsLocked` answers; before any,
/// a placed reference is locked when its record stores a lock record.
pub const LOCK_STATE: &str = "vm_lock_state_v1";
/// The player walks only on a cell's navigation mesh triangles, joined
/// across shared edges and edge links of kind 0 (`walk_order_census`).
pub const WALK_NAVMESH_SURFACE: &str = "walk_navmesh_surface_v1";
/// A closed door blocks the walk where the standing player comes within a
/// radius of its base's object bounds placed by the reference
/// (`walk_order_census`).
pub const WALK_DOOR_BOUNDS_BLOCK: &str = "walk_door_bounds_block_v1";
/// A placed door whose record stores a lock record is closed to the
/// player's walk until a script unlocks it (`walk_order_census`).
pub const WALK_DOOR_LOCK_RECORD_CLOSED: &str = "walk_door_lock_record_closed_v1";
/// A scene's response action (kind 5) plays the topic of its four at the
/// index of the choice the player picks in the scene.
pub const NPC_RESPONSE_CHOICE: &str = "scene_npc_response_choice_v1";
/// A placed reference's lock changing raises `OnLockStateChanged()` on its
/// own scripts and the aliases holding it.
pub const LOCK_STATE_EVENT: &str = "vm_lock_state_event_v1";
/// A `GlobalVariable.SetValue` with an undetermined value leaves the
/// global undetermined for `GetValue` until a known value is set
/// (`quest_run --global-value-undetermined`, with `--global-values`).
pub const GLOBAL_VALUE_UNDETERMINED: &str = "vm_global_runtime_value_undetermined_v1";
/// The player's pick of a terminal's menu item runs the function the
/// terminal record's fragment block names for the item's id, on the
/// block's script instance, with the placed terminal as its argument.
pub const TERMINAL_FRAGMENT: &str = "terminal_menu_item_fragment_v1";
/// A script's `Lock` that changes the lock the run holds raises
/// `OnLockStateChanged()` on the reference's own scripts and the aliases
/// holding it, after the call that changed it has run.
pub const LOCK_STATE_EVENT_SCRIPT: &str = "vm_lock_state_event_script_v1";
/// The caller's item added to a holder raises `OnItemAdded(item, count,
/// none, source)` on the holder's scripts and the aliases holding it, for
/// each instance whose inventory event filter the run kept names the item
/// (or is none).
pub const ITEM_ADDED_EVENT: &str = "inventory_item_added_event_v1";
/// An init handler (`OnAliasInit`, `OnInit`) the run never delivers that
/// calls `AddInventoryEventFilter` on itself counts as having kept that
/// filter, with the value its argument holds when the event is raised.
pub const INVENTORY_FILTER_INIT: &str = "inventory_filter_init_registration_v1";
/// The caller's item added raises the holder's count of that item, which
/// `GetItemCount` answers, from a known count.
pub const ITEM_ADDED_COUNT: &str = "inventory_item_added_count_v1";
/// `HasPerk` on the player answers whether the player's base record lists
/// the perk (`PRKR`), as the run's `AddPerk` and `RemovePerk` on the
/// player change it.
pub const PERK_BASE_LIST: &str = "actor_perk_base_list_v1";
/// `IsInFaction` on a placed actor or the player answers whether its base
/// record lists the faction with a rank of 0 or more (`SNAM`), as the run's
/// `SetFactionRank`, `RemoveFromFaction` and `RemoveFromAllFactions` on it
/// change that; a base naming a template is not answered.
pub const FACTION_BASE_LIST: &str = "actor_faction_base_list_v1";
/// A reference alias created at run time (`ALCO`) holds, while its quest
/// runs, one reference the run makes for it when first read: no record, no
/// scripts of its own (its base's are not attached), no place or level
/// read (`ALCA`, `ALCL` kept as stored), alive until the run kills it
/// (`IsDead` answers false before a death the run raised, with
/// `--dead-state`).
pub const ALIAS_CREATED_REFERENCE: &str = "quest_alias_created_reference_v1";
/// A reference alias filled from another quest's alias (`ALEQ`, `ALEA`)
/// holds, whenever it is read, what that alias holds at that moment (a
/// live link, not a fill made once when its quest starts).
pub const ALIAS_EXTERNAL_LIVE: &str = "quest_alias_external_live_v1";
/// A spoken line the run plays delivers `OnBegin(speaker, said)` as it
/// begins and `OnEnd(speaker, said)` as it ends to the scripts its record's
/// VMAD attaches of its own (not the fragment block), on the line's handle,
/// with their property values; `said` is false until the run has delivered
/// the line's end once.
pub const LINE_SCRIPT_EVENTS: &str = "dialogue_line_script_events_v1";
/// The player's sit ends, as a get-up the run raises (`OnGetUp`, then
/// `OnExitFurniture`), once the caller's track takes the player farther
/// than the caller's distance from the furniture's placed position, or
/// into another space.
pub const SIT_ENDED_BY_TRACK: &str = "vm_sit_ended_by_track_v1";
