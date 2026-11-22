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

/// Every rule above.
pub const ALL: [&str; 20] = [
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

/// The registry the rules are checked against.
pub const REGISTRY: &str = include_str!("../../../spec/hypotheses.tsv");

/// Status column of a rule in the registry.
pub fn status(name: &str) -> Option<&'static str> {
    REGISTRY.lines().find_map(|row| {
        let mut c = row.split('\t');
        (c.next() == Some(name)).then(|| c.nth(4)).flatten()
    })
}
