//! Opcode roles claimed from the census (`spec/pex.md`, Roles). A role names
//! what each slot is shown to hold by checks over every instruction of the
//! installation; what the instruction does when run is not claimed here.
//! Candidate meanings beyond these are pending rules in `spec/hypotheses.tsv`.
use serde::Serialize;

/// What the census shows an opcode's slots to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Role {
    /// Nothing claimed: the opcode keeps its number.
    Unclaimed,
    /// Opcode 20. Slot 0, its only slot, is an integer that, added to the
    /// instruction's index, lands inside the function (index 0 to the
    /// instruction count) and is never 0.
    Jump,
    /// Opcodes 21 and 22. Slot 1 is a target as in [`Role::Jump`]; slot 0
    /// names a local, parameter or variable (a Bool in almost every
    /// instruction; two hold a bool literal).
    /// Which value of slot 0 takes the jump is pending
    /// (`pex_conditional_jump_sense_v1`).
    ConditionalJump,
    /// Opcode 23. Slot 0 names a function declared by the declared type of
    /// slot 1 or one of its parents, not marked global; the counted list has
    /// as many entries as that function has parameters; slot 2 is declared
    /// with the function's return type.
    CallMethod,
    /// Opcode 24. As [`Role::CallMethod`] with the calling script's parent as
    /// the type; slot 1 holds the return type.
    CallParent,
    /// Opcode 25. Slot 0 names a script and slot 1 a function it or a parent
    /// declares, marked global; list and slot 2 as [`Role::CallMethod`].
    CallStatic,
    /// Opcodes 28 and 29. Slot 0 names a property declared by the declared
    /// type of slot 1 or a parent; slot 2 is declared with the property's
    /// type. Which one reads and which one writes is pending
    /// (`pex_property_get_set_v1`).
    Property,
    /// Opcodes 38 and 39. One slot is declared with a struct type
    /// (`Script#Struct`) of a winning script and another names one of its
    /// members (38: struct in slot 1, member in slot 2; 39: struct in slot 0,
    /// member in slot 1). Direction pending (`pex_struct_ops_v1`).
    StructMember,
}

/// Role of opcodes 0 to 46.
pub const ROLES: [Role; 47] = {
    let mut r = [Role::Unclaimed; 47];
    r[20] = Role::Jump;
    r[21] = Role::ConditionalJump;
    r[22] = Role::ConditionalJump;
    r[23] = Role::CallMethod;
    r[24] = Role::CallParent;
    r[25] = Role::CallStatic;
    r[28] = Role::Property;
    r[29] = Role::Property;
    r[38] = Role::StructMember;
    r[39] = Role::StructMember;
    r
};

/// Name used by the disassembly: the claimed role, or `OpNN`.
pub fn op_name(op: u8) -> String {
    let n = match ROLES.get(usize::from(op)) {
        Some(Role::Jump) => "Jump",
        Some(Role::ConditionalJump) if op == 21 => "CondJump21",
        Some(Role::ConditionalJump) => "CondJump22",
        Some(Role::CallMethod) => "CallMethod",
        Some(Role::CallParent) => "CallParent",
        Some(Role::CallStatic) => "CallStatic",
        Some(Role::Property) if op == 28 => "Property28",
        Some(Role::Property) => "Property29",
        Some(Role::StructMember) if op == 38 => "StructMember38",
        Some(Role::StructMember) => "StructMember39",
        _ => return format!("Op{op:02}"),
    };
    n.to_string()
}

/// Slot of a relative instruction target, when the role has one.
pub fn target_slot(op: u8) -> Option<usize> {
    match ROLES.get(usize::from(op)) {
        Some(Role::Jump) => Some(0),
        Some(Role::ConditionalJump) => Some(1),
        _ => None,
    }
}
