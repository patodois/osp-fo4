# Compiled scripts (`.pex`)

First step of the scripts phase: the compiled script files that Fallout 4
stores under `scripts/` in its archives. `fo4_pex::Pex` reads a whole file
into typed structures (header, string table, debug information, user flags,
objects with their structs, variables, properties, states and functions,
and each function's parameters, locals and instructions with typed
arguments). `fo4_pex::disassemble` prints one file as text. Nothing here
runs a script.

Every statement below comes from measuring the stored bytes of one
installation. The overall layout was recognised from prior general
knowledge before it was measured (see [provenance](../docs/provenance.md));
it is kept only where the census below, which could have refuted it, holds.
No compiler, decompiler, disassembler, virtual machine, format
documentation or script source was opened or searched for; the archives
hold no script sources. Field names are this project's labels.

Evidence tags as in [havok-container.md](havok-container.md):
**[measured N]** holds for N items and fails for none unless a second count
says so; **[hypothesis]**; **[open]**.

## Census scope

`pex_census MANIFEST OUT_DIR` (an example of `fo4-pex`) reads every `.pex`
entry of every mounted archive of the manifest, overridden copies included,
with `Pex::read`, then runs the checks below over the winning entries and
tries alternative opcode arities. It writes counts only to `pex-census.txt`,
committed as [pex-census.txt](pex-census.txt), and the most called
functions by name to a second file that is not versioned. Same installation
as havok-container.md (build declared 1.10.163, 7 plugins, 62 archive
mounts): 10,282 `.pex` entries in 14 archives (7,875 in the base game
archives, 2,396 in the add-on archives, 11 in other archives), 10,282
distinct paths (no entry is overridden), 14,857,157 bytes; 3.8 s wall, peak
RSS about 540 MB (one pass reading, then the alternative-arity passes over
the kept bytes).

**Result: all 10,282 entries are read to the last byte** (14,857,157 of
14,857,157 bytes); no entry is refused. Bytes by part: header 1,144,129;
string table 6,693,495; debug 1,117,979; user flags 205,640; objects
5,695,914 (of which instructions 3,428,747).

## Encoding

Integers are little-endian. A *text* is a u16 byte count and the bytes,
without terminator. A *string* is a u16 index into the file's string table,
checked to be below its length. A *value* is a kind byte and its payload:

| Kind | Payload | Rust |
| ---: | --- | --- |
| 0 | none | `Value::None` |
| 1 | string (read as a name) | `Value::Ident` |
| 2 | string (read as text) | `Value::Str` |
| 3 | i32 | `Value::Int` |
| 4 | f32 (bits kept) | `Value::Float` |
| 5 | u8, 0 or 1 in every value | `Value::Bool` |

Kinds 6 and above occur in no file and are refused.

## Layout

| Part | Fields, in stored order | Evidence |
| --- | --- | --- |
| Header | u32 `0xFA57C0DE`; u8 3; u8 9; u16 2; u64 *compiled*; text *source*; text *user*; text *machine* | constant in 10,282 files; *compiled* between 1,456,966,841 and 1,574,705,094 (read as Unix seconds: 2016 to 2019); *source* ends `.psc` and its file stem equals the object name after its last `:` in 10,282 files; one distinct *user* and 7 distinct *machine* texts |
| String table | u16 n; n texts | every text ASCII and distinct within its file, and every string referenced by some field, in 10,282 files (430,153 strings, none unreferenced) |
| Debug | u8 presence (0 or 1); when 1: u64 *modified*, function entries, property groups, struct orders (below) | present in 10,043 files, absent in 239; *modified* <= *compiled* in 10,043 |
| User flags | u16 n; n times (string *name*, u8 *bit*) | the same six names with the same bits in every file (bits 0 to 5); every user-flag word below uses declared bits only (123,069 words) |
| Objects | u16 n; n objects | exactly one object in each of the 10,282 files |

**Object.** string *name*; u32 *size*; string *parent*; string *doc*; u8
(0 or 1); u32 user flags; string *auto state*; u16 n structs; u16 n
variables; u16 n properties; u16 n states, each list followed by its
elements. *size* counts the bytes from the size word to the object's end in
every file (enforced). The name has as many `:` separators as the entry
path has folders below `scripts/`, and the entry's file stem equals its
last part, in 10,282 files; it is string 0 in 9,847 files and another
index in 435. The parent is empty in one object and names another winning
script in the other 10,281. The auto state names one of the object's
states, and one state has the empty name, in 10,282 objects.

**Struct.** string *name*; u16 n; n members: string name, string type,
u32 user flags, value, u8 (0 in all 564 members), string doc
(130 structs).

**Variable.** string name, string type, u32 user flags, value, u8 (0 or 1).
The value is none or of the declared simple type (Int, Float, Bool, String)
for all 39,227 members and variables. The u8 is 1 in 32,395 variables
(31,925 of them named by a property) and 0 in 6,268 (3,831 named by a
property): it is kept, not interpreted.

**Property.** string name, string type, string doc, u32 user flags, u8
*flags* (1, 3 or 7 in every property). With bit 4 set, a string naming a
variable follows: 35,756 properties, every one with bits 1 and 2 also set,
and the variable belongs to the object and has the property's type.
Otherwise a function follows for bit 1 and then one for bit 2: the 77 bit-1
functions take no parameter and return the property's type; the 12 bit-2
functions take one parameter of that type and return None.

**State.** string name; u16 n; n times (string name, function). Every
function of a named state (1,022) also has a function of that name in the
empty state or in a parent script.

**Function.** string return type, string doc, u32 user flags, u8 *flags*
(only bits 1 and 2 occur: 29,496 with 0, 96 with 1, 700 with 2, 191 with
3), u16 n parameters (string name, string type), u16 n locals (same), u16 n
instructions. The 891 functions with bit 2 have no instructions and no
locals (`FUNCTION_NATIVE`, a label); 1,833 functions without it have no
instructions either. Every function with bit 1 (287) is in the empty state,
and every call by opcode 25 reaches a function with bit 1 while calls by
opcodes 23 and 24 never do (see Roles).

**Instruction.** u8 opcode; as many values as the opcode's fixed arity;
for the three counted opcodes, the last fixed value is an Int n >= 0 and n
more values follow.

### Debug

After *modified*:

- u16 n function entries: string object, string state, string name, u8
  *kind* (0, 1 or 2), u16 n, n u16 line numbers. Kind 1 and 2 entries (77
  and 12) name a property's bit-1 and bit-2 function with the empty state,
  and have one line per instruction. Of the 30,282 kind 0 entries, 29,987
  name a stored function at the state and name they give with one line per
  instruction; 295 give the empty state for a function stored only in a
  named state (paired with a same-named function of another state with that
  many instructions). Native functions have entries (891, with no lines).
  112 non-native functions have no entry they can be paired with. So the
  line lists are kept as stored; `Pex::debug_lines` pairs them as described,
  and nothing relies on them.
- u16 n property groups: string object (the file's object in 7,244), string
  name, string doc, u32 user flags, u16 n, n strings. Every listed name is a
  property of the object and each property is listed in exactly one group
  (35,833 properties).
- u16 n struct orders: string object, string name, u16 n, n strings. Every
  struct has one (122 in files with debug); its members are the struct's
  members as a set in 122, in stored order in only 31.

### User flags

| Name | Bit | Set on |
| --- | ---: | --- |
| hidden | 0 | 7,976 objects, 521 properties, 3 struct members |
| conditional | 1 | 420 objects, 1,154 variables |
| default | 2 | 144 objects |
| collapsedonref | 3 | 105 property groups |
| collapsedonbase | 4 | 3 property groups |
| mandatory | 5 | 9,887 properties |

The names are the stored strings; what the flags do is not read.

## Opcodes

267,237 instructions in 30,483 functions. Opcodes 1 to 46 except 35, 41
and 44 occur; 0, 35, 41, 44 and everything from 47 occur in no file and are
refused by the reader (`Opcode`), so the histogram is closed over the
measured files and nothing is read with an unmeasured arity.

**How the arities were established.** The table `OPCODES` was first filled
with candidates recognised from prior knowledge. For each opcode the census
then replaces its entry by each of 12 alternatives (fixed arity 0 to 6, and
a counted list after 1 to 6 fixed values, minus the entry itself), keeps
the rest of the table, and reads again every file holding that opcode. A
file read exactly means read to its last byte with every object size
holding. Under the table every file is read exactly. For 42 of the 43
opcodes that occur, no alternative reads every file holding the opcode, so
the arity is the only one of those tried that the data allows; for most of
them no alternative reads a single file (the census lists, per opcode, how
many files each partly working alternative reads). The exception is opcode
34: its slot 3 is the Int 0 in all 81 instructions, so the counted reading
`4+n` (with n = 0) consumes the same bytes and also reads all 43 files; the
reader takes 4 fixed values and an instruction with another slot 3 would
decide **[open]**.

**Argument kinds.** For every opcode and slot the census counts the value
kinds and, for identifiers, what they name in the function: a local, a
parameter, a variable of the object or of a parent, `self`, or another name
(a function, property, member or type). The type classes below are the
declared types of the identifiers that name a local, parameter or variable
(Int, Float, Bool, String, Var, object, array `[]`, struct `#`).

| Op | Arity | Instructions | Files | Slots as measured (identifier types; literal kinds) | Role |
| ---: | --- | ---: | ---: | --- | --- |
| 1, 3, 5, 7, 9 | 3 | 1,960; 366; 55; 37; 5 | 663; 165; 25; 13; 4 | Int, Int or int, Int or int | |
| 2, 4, 6, 8 | 3 | 504; 192; 252; 126 | 230; 81; 79; 57 | Float, Float or float, Float or float | |
| 10 | 2 | 4,644 | 1,140 | Bool; any type | |
| 11 / 12 | 2 | 8 / 29 | 2 / 12 | Int, Int / Float, Float | |
| 13 | 2 | 8,785 | 2,704 | any; any identifier or literal of every kind | |
| 14 | 2 | 40,929 | 7,677 | any; any (types of the two slots differ) | |
| 15 | 3 | 14,795 | 2,202 | Bool; any; any | |
| 16 to 19 | 3 | 2,105; 333; 1,192; 660 | 771; 187; 431; 338 | Bool; Int or Float; Int or Float | |
| 20 | 1 | 24,315 | 3,260 | int target | Jump |
| 21 | 2 | 1,390 | 415 | Bool; int target | ConditionalJump |
| 22 | 2 | 28,659 | 3,260 | Bool in 27,476 of 28,657 identifiers, 2 bool literals; int target | ConditionalJump |
| 23 | 4+n | 113,324 | 9,204 | name; receiver; destination; n; arguments | CallMethod |
| 24 | 3+n | 60 | 37 | name; destination; n; arguments | CallParent |
| 25 | 4+n | 8,627 | 2,400 | script; name; destination; n; arguments | CallStatic |
| 26 | 1 | 1,225 | 274 | any identifier or literal | |
| 27 | 3 | 283 | 23 | String, String, String | |
| 28 / 29 | 3 | 3,351 / 2,939 | 770 / 1,799 | property name; object receiver; value | Property |
| 30 | 2 | 203 | 93 | array; Int or int | |
| 31 | 2 | 1,071 | 347 | Int; array | |
| 32 | 3 | 2,478 | 448 | element; array; Int or int | |
| 33 | 3 | 237 | 46 | array; Int or int; element | |
| 34 | 4 | 81 | 43 | array; Int; value; int 0 | |
| 36 | 3 | 121 | 66 | Bool; object; a type name | |
| 37 | 1 | 15 | 13 | struct | |
| 38 | 3 | 1,415 | 116 | value; struct; member name | StructMember |
| 39 | 3 | 172 | 37 | struct; member name; value | StructMember |
| 40 | 5 | 37 | 25 | array of structs; Int; member name as a string literal; value; Int or int 0 | |
| 42 | 3 | 139 | 73 | array; value; int (never 0) | |
| 43 | 3 | 17 | 9 | array; value; Int or int 0 | |
| 45 | 3 | 54 | 37 | array; Int or int 0; int (never 0) | |
| 46 | 1 | 47 | 15 | array | |

### Roles

A role is claimed only where every instruction of the opcode passes a
check that names what its slots hold (`fo4_pex::ROLES`, `op_name`; the
disassembly prints the role name, the others `OpNN`). What an instruction
does when run is not claimed: the candidate meanings are the pending rules
listed after this table.

| Role | Opcodes | Check over every instruction |
| --- | --- | --- |
| Jump | 20 | slot 0 is an int; added to the instruction's index it lands in 0 to the instruction count, and it is never 0 **[measured 24,315]**. Int slots of other opcodes fail this (for example 608 of 3,916 in slot 1 of opcode 13). |
| ConditionalJump | 21, 22 | slot 1 as above **[measured 1,390 and 28,659]**; slot 0 is an identifier naming a local, parameter or variable in all but 2 (bool literals) |
| CallMethod | 23 | slot 0 names a function declared by the declared type of slot 1 or a parent, not global; the counted list has as many values as it has parameters; slot 2 is declared with its return type **[measured 113,324]** |
| CallParent | 24 | the same with the calling script's parent as the type, slot 1 as the destination **[measured 60]** |
| CallStatic | 25 | slot 0 names a script, slot 1 a function it or a parent declares, global; list and slot 2 as above **[measured 8,627]** |
| Property | 28, 29 | slot 0 names a property of slot 1's declared type or a parent; slot 2 is declared with the property's type **[measured 3,351 and 2,939]** |
| StructMember | 38, 39 | the struct slot (38: slot 1, 39: slot 0) is declared `Script#Struct` of a winning script and the member slot (38: 2, 39: 1) names one of its members **[measured 1,415 and 172]** |

Opcode 40 passes the same struct check with its array's element type and
its string literal (37 instructions), and slot 2 of opcode 36 names a
winning script or a simple type (121); they keep their numbers because one
check per opcode is too narrow a base. Name lookups ignore ASCII case,
find the nearest script of the parent chain that declares the name, and
take its empty-state declaration first (`fo4_pex::index`). Of the 122,011
calls, 64,537 resolve to a native function and 57,474 to a scripted one.

Pending rules for the meanings (`spec/hypotheses.tsv`):
`pex_ops_arithmetic_v1` (1 to 9, 11, 12), `pex_ops_compare_v1` (15 to 19),
`pex_ops_not_copy_cast_v1` (10, 13, 14), `pex_conditional_jump_sense_v1`
(20 to 22), `pex_call_dispatch_v1` (23 to 25), `pex_op_return_v1` (26),
`pex_op_string_concat_v1` (27), `pex_property_get_set_v1` (28, 29),
`pex_array_ops_v1` (30 to 34, 40, 42, 43, 45, 46), `pex_struct_ops_v1`
(36 to 39). Their candidate meanings were recognised from prior knowledge;
the slot types above are consistent with them and do not establish them.

## Disassembly

`pex_dump MANIFEST scripts/<path>.pex` (an example of `fo4-pex`) prints the
winning entry of that path: header texts, part offsets, the object and its
structs, variables, properties and states, and each function with its
locals and one line per instruction (index, role name or `OpNN`, fixed
values, counted values in parentheses, jump targets as absolute indices,
the paired debug line number).

## Limits

- One installation, one game build. A file with another version, game
  number, kind, opcode or count layout is refused with a named error.
- Arity of opcode 34 between `4` and `4+n` is open (above); opcodes 0, 35,
  41 and 44 are unmeasured.
- Debug line lists are kept as stored and paired by name and count; their
  meaning (source line numbers) is not used.
- The two u8 fields of objects and variables and the user flags are kept
  and not interpreted.
- No instruction is executed; every meaning beyond the roles is a pending
  rule.
