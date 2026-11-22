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
pub const ALL: [&str; 77] = [
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

/// The registry the rules are checked against.
pub const REGISTRY: &str = include_str!("../../../spec/hypotheses.tsv");

/// Status column of a rule in the registry.
pub fn status(name: &str) -> Option<&'static str> {
    REGISTRY.lines().find_map(|row| {
        let mut c = row.split('\t');
        (c.next() == Some(name)).then(|| c.nth(4)).flatten()
    })
}
