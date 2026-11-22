# Script interpreter (`fo4-vm`)

`fo4_vm` runs the compiled scripts read by `fo4_pex` ([pex.md](pex.md))
under pending rules, with a host that records native calls instead of
performing them and answers only those whose result the installed data
stores (the data host). It is the smallest interpreter that runs one real
interaction, and every result it gives is labelled with the rules it relied
on. Nothing here has been measured in the running game: every opcode
meaning, every binding rule and every host answer below is a registered
`pending` rule in [hypotheses.tsv](hypotheses.tsv), and the checks recorded
here support some of them without establishing any.

Evidence tags as in [pex.md](pex.md). Names of scripts, objects, records and
cells stay out of this file; the census tools write them to files that are
not versioned. The exception is the handful of engine interface names the
code itself binds to (listed under Recording host, Data host and Binding), without
which the assumptions could not be stated.

## What runs

- **Values**: none, Int (i32), Float (f32), Bool, String, object handle,
  array, struct, and *undetermined*. An undetermined value carries its
  origin (a native function's result, or a conversion or comparison that no
  rule settles); it may be stored, passed and logged.
- **Scripts** are loaded by name from a `Library` (the archives' winning
  `.pex` entries, read when first named, or an in-memory set in tests) and
  copied with names lower-cased (`program`), as every name resolution
  measured in [pex.md](pex.md) ignores case.
- **Instances**: a script attached to an object holds the variables of the
  script and of every parent, and a current state, starting in the script's
  auto state.
- **Frames**: parameters, then locals initialised to their type's default
  (`pex_default_values_v1`); identifiers resolve to a parameter or local,
  `self`, `::State`, or a variable of the running function's script or a
  parent.
- **Calls**: opcode 23 on an object dispatches by the receiver's instance
  (current state first, then the empty state, from the instance's script up
  its parents); opcode 24 starts from the calling script's parent; opcode 25
  calls a global function. A native function goes to the host. A scripted
  function of a script without variables in its chain runs on an object
  that carries no instance of it; otherwise a scripted call or property on
  an object without that script **blocks**.
- **Properties** (28, 29): a property with a variable reads or writes it; a
  property with functions calls them.
- **Timers** are a queue in the run's world, filled by the recording host
  and fired only when asked (`vm_timer_events_v1`).
- **Limits**: 200,000 instructions per delivery, 64 nested calls, 4,096
  elements per array, 2^20 arrays, structs and undetermined values per run.
  Runs are deterministic and single-threaded (a native that waits returns
  at once).

A run ends in one of three ways:

- **completed**, with the returned value;
- **blocked**, where an undetermined value would steer control flow: a
  conditional jump on it, a scripted call or property on it, an array
  operation on it as array, index, size or count, or the truth of an empty
  array. The block names the origin (for example the native whose result
  was needed) and the instruction;
- **failed**, with a typed error: an opcode without a registered meaning
  (`UnmeasuredOpcode`; opcodes 0, 35, 41, 44 and 47 up are also refused by
  the reader), a missing script or function, a native the host does not
  provide, a type mismatch on a store (every store is checked against the
  declared type of its slot), an index out of range, an integer division by
  zero, an operation on none, the step or depth limit.

Nothing is guessed and nothing is skipped silently. Where a candidate
meaning leaves a case open, the result is undetermined rather than chosen:
integer overflow; an integer quotient or remainder whose sign convention
would matter; a float quotient by zero; conversions of Float, Bool, objects
and other kinds to String, of String to numbers, and of a Float with a
fraction to Int; equality of two texts that differ only in case; the order
of two texts; the identity of an opaque handle with another handle; a cast
of an object to a type its known types do not include; the length of none.

## Opcodes

Meanings are the candidates recognised from prior knowledge (see
[provenance](../docs/provenance.md)), each a pending rule. The checks are
those of `vm_checks` below; the column says what each check could have
refuted and whether anything failed.

| Opcodes | Candidate meaning (rule) | Checks run, and what they could have refuted |
| --- | --- | --- |
| 1, 3, 5, 7, 9; 2, 4, 6, 8; 11, 12 | Int sum, difference, product, quotient, remainder; Float sum, difference, product, quotient; Int and Float negation, into slot 0 (`pex_ops_arithmetic_v1`) | Slot 0 declared Int or Float as the operation says in every instruction (could refute the result types). Data flow: with slot 0 as destination no temporary is read before it is written; with no destination or another slot, 2 to 720 such reads per opcode (could refute the destination). Array loops advance their index by opcode 1 with 1 (555 of 562; one by opcode 3, two otherwise). Executing 343 functions: exchanging 1 with 3 and 2 with 4 makes 6 runs grow an array without bound, the candidate none. **Not refuted by anything run: exchanging product and quotient (5/7, 6/8)** changes 8 results and fails no check. |
| 15 to 19 | Equal, less, at most, greater, at least, into the Bool slot 0 (`pex_ops_compare_v1`) | Slot 0 Bool in every instruction. 12,746 of 19,085 results are read by the next instruction, a conditional jump, and 4,888 are copied first. Array loops: all 562 loops that index an array by a counter compared with that array's length exit by 22, on `index < length` (opcode 16, index in slot 1) in 558 and on opcode 17 in 4; with 16 read as greater the 558 loops would never run, with 16 and 17 exchanged they would index one past the end. 13 guards compare an object with none by 15 and then call it where it is not none under the candidate. |
| 10, 13, 14 | Not of slot 1's truth; copy; conversion to slot 0's declared type (`pex_ops_not_copy_cast_v1`) | 13: both slots have one declared type in all 8,785 (a conversion would need two). 14: types differ in 29,724 of 40,929; the 11,205 others are Bool to Bool in 11,070, object 120, Float 10, Int 5. Executing with 14 read as a copy: 73 runs store an Int in a Float slot or the reverse. Two guards test an object through 10 and call it only where it is not none. |
| 20, 21, 22 | Jump; jump when slot 0 is true (21) or false (22) (`pex_conditional_jump_sense_v1`) | 718 guards test an object (the object itself 574, its cast to Bool 129, its equality with none 13, its negation 2) and call a method or property on it in the instructions that run next when the jump is not taken: under the candidate the object is not none there in all 718, and none in all 718 if the senses were exchanged. Executing with the senses exchanged: 3 runs never end. Every loop's exit test is 22 (1,482). |
| 23, 24, 25 | Call by receiver, parent, script (`pex_call_dispatch_v1`) | Roles measured in [pex.md](pex.md). The census below runs 2,552 handlers through them. |
| 26 | Return slot 0; past the last instruction, the return type's default (`pex_op_return_v1`) | 35 of 572 functions returning a value can run past their last instruction. |
| 27 | Concatenate (`pex_op_string_concat_v1`) | Slot 0 String (283). Data flow: other destinations give 264 to 410 undefined reads. |
| 28, 29 | Read a property into slot 2; write slot 2 into it (`pex_property_get_set_v1`) | Data flow: 28 with slot 2 as destination: no undefined read; with no destination or slot 0 or 1: 5,921 to 6,108. 29 holds no temporary in its slots, so the check cannot speak for it. |
| 30 to 34, 40, 42, 43, 45, 46 | New array; length; get; set; find from an index (-1 when absent); find by struct member; add n times; insert; remove n; clear (`pex_array_ops_v1`) | Element and Int types as the meanings say in every instruction of 31 to 34, 40, 42, 43. Data flow separates destinations for 30, 31, 32, 34, 40 (4 to 4,224 undefined reads under other destinations); 33, 42, 43, 45 and 46 hold no temporary, so it cannot speak for them. |
| 36 to 39 | Is-of-type into Bool; new struct; get member; set member (`pex_struct_ops_v1`) | Member types as the meanings say (1,415 and 172). Data flow separates 36, 37, 38 (4 to 2,458); 39 cannot be checked so. |

Two limits apply to the whole table:

- The checks fix the readings relative to each other. A reading that
  negates every Bool at once (both jump senses, every comparison, the truth
  of every value and the Bool literals) behaves identically on every path
  the data contains; only Bool values seen outside branches (native
  arguments, two `if` statements on the literal true, which would make their
  then-branch dead) argue against it, and only the running game can settle
  it.
- An exchanged reading that changes results without failing a check is not
  refuted: of the runs that complete under both, 172 results differ with
  the jump senses exchanged, 165 with less and greater exchanged, 15 with
  strict and non-strict exchanged, 37 with equality negated, 37 with sum
  and difference exchanged, 8 with product and quotient exchanged, none
  with the property directions exchanged; no check says which result is
  right.

## Checks (`vm_checks`)

`vm_checks MANIFEST OUT_DIR` (an example of `fo4-vm`) copies every winning
`.pex` of the archives (10,282 scripts, 30,483 functions) and writes
[vm-checks.txt](vm-checks.txt), counts only. 1.5 s wall, peak RSS about
650 MB.

- **A1 slot types**: for each opcode the declared types its candidate
  meaning requires of its slots, counted over every instruction.
- **A2 data flow**: for every function, which temporaries (`::temp` locals)
  are written on every path before each read, with the candidate
  destination slot of every opcode, then with each opcode in turn given no
  destination or another identifier slot as destination. Under the
  candidate, 15 reads fail, all of temporaries that no instruction of their
  function writes at all (test scripts whose writing calls are absent from
  the compiled code). Also, for every candidate destination that is a
  temporary, whether a later instruction reads it.
- **A3 idioms**: what reads a comparison result first; the types tested by
  conditional jumps (21: Bool only; 22: Bool 27,476, object 1,110, Int 24,
  struct 21, array 17, String 8, Float 1, the literal true 2); object guards
  (above); loops (1,482 backward jumps, each with an exit test by 22 just
  after it); array loops (above); `::State` (declared by no script; read by
  13 and 26 and written by 13 in three instructions, all in the two
  state functions of the root script); functions that can run past their
  last instruction; simple-type variables whose stored value is none (2,045
  of 5,917).
- **B execution**: the functions with code, in the empty state, whose
  parameters are Int, Float, Bool, String or arrays of them, and which are
  statically pure: every call on `self` resolves, in every state of the
  instance's script chain, to scripted pure functions, every static or
  parent call to one; no call or property on another object; no variable or
  property of another type (an instance made without VMAD holds none
  there). 343 functions, each run with at most 24 argument tuples from fixed
  samples, with no host (a native would stop the run), under the candidate
  and under each exchanged reading (`fo4_vm::Variant`). Under the candidate
  every run completes (one with an undetermined result: a Float converted
  to String). The failures of the other readings are listed above.

## Recording host

`RecordingHost` performs no game behaviour. The interpreter logs every
native call it makes (caller, `script.function`, receiver, arguments,
result). The host answers:

| Native | Answer | Rule |
| --- | --- | --- |
| Any declared to return none | none | none needed: nothing is returned |
| `Game.GetPlayer` | the player handle: one opaque object, the same on every call and the same as the actor the delivered player event names, known to be of the native's declared return type (`Actor`); equal only to itself | `vm_host_player_handle_v1` |
| `ScriptObject.StartTimer`, `StartTimerGameTime` | none; queues `OnTimer(id)` or `OnTimerGameTime(id)` for the receiver's instance after the interval on its clock | `vm_timer_events_v1` |
| `ScriptObject.CancelTimer`, `CancelTimerGameTime` | none; removes the queued timers of that id | `vm_timer_events_v1` |
| Every other native returning a value | undetermined, naming the native | — |

Object identity (`vm_object_identity_v1`): one handle per record (FormID
resolved through the load order) and one per quest alias
(`vmad_alias_value_v1`); handles of different records or aliases compare
unequal; whether an opaque handle (the player's) is the same object as
another handle is left undetermined.

Timers fire only when asked (`script_run ... --timers N`), in due order; the
firing stops with an error when the queue does not order them (an
undetermined interval, both clocks queued, or two timers of one id on one
instance).

## Data host

`fo4_vm::data::DataHost` answers as the recording host does, except for
the natives below, which it answers by a lookup over the records the
plugin store already reads. It is what `bind::run` and `script_run` use;
`script_run --recording` runs the recording host alone. Each answer is a
pending rule; the counts are from `host_data_census`
([vm-host-data-census.txt](vm-host-data-census.txt), every winning record
of the installation) and from the census below (calls answered).

| Native | Answer read from | Rule | Data census | Calls answered (placed references) |
| --- | --- | --- | --- | --- |
| `ObjectReference.GetLinkedRef(kw)` | the receiver's XLKR pair whose first word is `kw` (0 for none): its second word; none when no pair has it; undetermined when two do | `vm_host_linked_ref_xlkr_v1` | 88,491 XLKR on 76,079 placed records, all 8 bytes; first words 0 or a KYWD; second words a REFR, ACHR or PHZD (2 name no winning record); first words distinct within every record | 5,035 a reference, 1,057 none |
| `GetLinkedRefChain(kw, max)`, `CountLinkedRefChain(kw, max)` | GetLinkedRef(kw) followed from the receiver until none; undetermined on a loop or past `max` | `vm_host_linked_ref_chain_v1` | as above | 288 chains, 69 empty, 52 counts; 2 loops open |
| `ObjectReference.IsDisabled` | the record header flag 0x800 (the store's initially disabled), or this run's last Enable or Disable of the receiver | `vm_host_is_disabled_flag_v1` | 4,032 placed records carry it; read and indexed alike in all 2,001,939 | 133,317 |
| `ObjectReference.GetBaseObject` | NAME; for a base bound alone, that base | `vm_host_base_object_name_v1` | one 4-byte NAME naming a winning record in all 1,789,365 placed REFR and ACHR | 161 a record |
| `Game.GetFormFromFile(id, file)` | the load order: the plugin of that name (any case), object id `id` masked to 24 bits (12 for a light plugin); none when that plugin is not loaded or has no such record | `vm_host_form_from_file_v1` | (the manifest's load order) | 394 a record |
| `GlobalVariable.GetValue` | the GLOB's one FLTV (f32) | `vm_host_global_fltv_v1` | one 4-byte finite FLTV in all 1,916 GLOB; 549 carry header flag 0x40, whose meaning is not read | 670 |
| `FormList.GetSize`, `GetAt(i)`, `HasForm(f)` | the FLST's LNAM entries in stored order; HasForm undetermined when not found in a list that holds lists | `vm_host_formlist_lnam_v1` | 12,548 LNAM in 1,060 FLST, all 4 bytes, 12,539 naming a winning record (94 a FLST) | none answered: every GetSize follows an AddForm in the same run, every HasForm gets an undetermined form |

What every answer assumes (`vm_host_data_state_v1`): the data's state is
the state at the start of a game, before any script ran. A fact another
script may have changed earlier in play is read as stored. Within one run,
Enable and Disable set what IsDisabled answers; SetLinkedRef on a
reference makes its links undetermined; any other native that receives a
global or a form list (as receiver or argument), except the reads above,
makes its value or its entries undetermined (in the census this is why
FormList.GetSize is never answered: the handlers add a form first).

A record handle the host or the binding creates is of its record type's
engine type (`vm_record_signature_type_v1`: ACHR Actor, REFR
ObjectReference, QUST Quest, GLOB GlobalVariable, KYWD Keyword, FLST
FormList and 31 more, `fo4_vm::data::SIGNATURE_TYPES`), with that type's
parents and the scripts attached to it, and of no other: a type test (36)
against another type is false, where it was undetermined. A cast (14) to
another type stays open (`pex_ops_not_copy_cast_v1`), and a cast of an
array of objects to an array of another object type is now undetermined
instead of a type mismatch (such arrays first appear with
GetLinkedRefChain). The census of VMAD object values checks the table: of
35,901 values naming a record of a listed type, the declared type of the
property is that type, a parent or a script extending it for 35,882; the
19 others are 13 REFR in Scene properties, 2 REFR in Quest, one REFR each
in Faction and Perk, one QUST in ObjectReference and one NPC_ in Actor;
2,502 values name records of types not listed, whose handles get no type
from their record type.

What stays blocked, and why (ranked by placed handlers after this step,
[vm-blockers.txt](vm-blockers.txt)): quest stages and running state
(`Quest.IsStageDone` 1,115, `IsRunning` 109; runtime), 3D loading
(`Is3DLoaded` 886; runtime), `GetActorsLinkedToMe` (759; the stored XLKR
of actors could answer it, but which actors are present is runtime),
`GetOpenState` (657), `IsDestroyed` (556) and the destruction stage (196),
followers (358), trigger occupancy (228), `FormList.GetSize` after an
AddForm (355), `Scene.IsPlaying` (123), actor values and states (dead,
sneaking, mounted, factions, perks, inventory), workshop production,
`GetFactionOwner` (73; ownership may come from the reference or its cell,
and which one decides is not in the data), `ReferenceAlias.GetReference`
(61; aliases are filled at run time), distances, random numbers and
time. None of these has a default.

## Binding

`fo4_vm::bind` connects runs to the game data.

- A **placed reference** is bound by creating one handle for it and an
  instance of each script of its base's VMAD and then of its own VMAD, one
  instance per script name; the base's property values are applied first,
  the reference's over them (`vmad_reference_scripts_v1`). Each value is
  stored in the variable behind the declared property of that name
  (`vmad_property_sets_variable_v1`); a property the script does not
  declare, or one without a variable, receives nothing and is listed as a
  binding note. Object values become record handles; FormID 0 is none; an
  object value that names no winning record is undetermined.
- A **TERM** fragment block's script is instantiated on a handle for the
  terminal base; event `fragment:N` runs the function of fragment N with the
  reference as its argument.
- **Events** (`vm_event_handlers_v1`): `activate`, `trigger-enter`,
  `trigger-leave`, `load`, `unload`, `init`, `reset`, `cell-attach`,
  `cell-detach` run `OnActivate`, `OnTriggerEnter`, `OnTriggerLeave`,
  `OnLoad`, `OnUnload`, `OnInit`, `OnReset`, `OnCellAttach`, `OnCellDetach`
  on each instance that has a function of that name with instructions, in
  VMAD order, dispatched in the instance's current state. One object
  parameter receives the player handle; a handler with other parameters is
  refused. Each delivery starts from the bound state: no `OnInit` or other
  event runs before it unless named first (events may be listed, comma
  separated).
- **Alias values** (`vmad_alias_value_v1`): an object value with an alias
  index names the alias of that index of the quest the FormID names, one
  handle per quest and index, typed by the property's declared type (the
  VMAD census finds every such property declared an alias type,
  [vmad.md](vmad.md)). Before this step it was an opaque handle.
- **Scripts attached on need** (`vm_record_scripts_attach_v1`): when a run
  first needs an instance on a record handle (a call, a property, a cast or
  a type test on it), `bind::RecordScripts` attaches the scripts of the
  record behind it: a QUST's VMAD scripts, an alias's scripts from its
  quest's VMAD block (read to the last byte, [vmad.md](vmad.md)), a placed
  REFR's or ACHR's scripts as for a bound reference; each with its VMAD
  property values, in its auto state, no event run on it. Other records
  get nothing. Quest stage fragments are read but not run.
- **Reference index**: the placed references of a cell are numbered as
  `fo4_vm::bind::placed` lists them: the active ones, then the initially
  disabled ones, each in stored order.

`script_run MANIFEST CELL[@DX,DY] list` prints the scripted placed
references with their indices; `script_run MANIFEST CELL[@DX,DY] INDEX
EVENT[,EVENT...] [--timers N]` prints the effect log. `@DX,DY` takes the
exterior cell that many grid steps east and north.

## Census

`script_run MANIFEST --census OUT_DIR CELL[:RADIUS]... | ALL | PLACED`
delivers to each scripted DOOR, TERM and ACTI, one fresh run per event,
every event that one of its scripts has a function with instructions for,
and every TERM fragment. `ALL` binds every winning scripted base alone (a
placement without a reference); `PLACED` binds every winning placed REFR
of a DOOR, TERM or ACTI base with a VMAD on the base or the reference, in
every cell. The counts are [vm-census.txt](vm-census.txt), each scope
before this step (recording host, nothing attached: measured with this
step's census before the data host was written; its counts for the two
old scopes are main's) and after it; the blockers ranked are
[vm-blockers.txt](vm-blockers.txt); names go to files that are not
versioned.

| Scope | Objects | Handlers | Completed | Blocked on a native's result | Blocked on an object without that script | Blocked on an open rule | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Test cells, before | 10 placed ACTI | 22 | 16 | 6 | 0 | 0 | 0 |
| Test cells, after | 10 | 22 | 16 | 6 | 0 | 0 | 0 |
| Every scripted base alone, before | 1,251 | 2,530 | 1,113 | 902 | 335 | 77 | 103 |
| Every scripted base alone, after | 1,251 | 2,530 | 1,319 | 1,034 | 0 | 57 | 120 |
| Every placed scripted reference, before | 16,718 | 32,548 | 20,671 | 9,802 | 1,040 | 449 | 586 |
| Every placed scripted reference, after | 16,718 | 32,548 | 24,655 | 6,686 | 1 | 478 | 728 |

### Blockers

Before this step, over every placed scripted reference, the natives whose
result blocked the most handlers were `ObjectReference.GetLinkedRef`
(3,492), `Quest.IsStageDone` (1,051), `Is3DLoaded` (736), `IsDisabled`
(689), `GetOpenState` (497), `GetActorsLinkedToMe` (382),
`Game.GetFormFromFile` (380), `Game.GetPlayerFollowers` (358),
`GetTriggerObjectCount` (228) and `GetLinkedRefChain` (205); the objects
without a script instance were quests (889 handlers) and placed references
(148), plus 2 opaque alias handles and the player once. Over the bases
bound alone: `GetLinkedRef` (306), `IsStageDone` (83), `Is3DLoaded` (46),
`GetFormFromFile`, `GlobalVariable.GetValue` and `CountLinkedRefChain`
(43 each); objects without a script: quests (316), references (18), one
opaque alias.

After it, the placed references' blocking natives are all runtime state
(listed under Data host); no handler blocks on `GetLinkedRef`,
`IsDisabled`, `GetFormFromFile`, `GetLinkedRefChain` or `GetValue`; one
blocks on an object without a script (the player). Over the bases bound
alone `GetLinkedRef` still leads (408): there its receiver is the
placement standing for the base, which has no record, so the data cannot
answer; the bases' natives blocked rise from 902 to 1,034 because the 335
handlers that stopped at a quest without a script now run into the
quest's own natives.

- Failures rise (586 to 728 placed, 103 to 120 bases) because handlers run
  further: a linked reference the data says is none, then called (op 23,
  281 to 359 placed), 12 runs reaching the step limit in a loop that waits
  on IsDisabled answered from the data, one attached reference whose VMAD
  names a FormID with an invalid master index (refused, as the binding
  refuses it).
- Open rules: casts of objects to a type their known types do not include
  (115 placed), array lengths of none, opaque identities.
- The test cells' six blocked handlers block on runtime natives after this
  step as before (`ReferenceAlias.GetReference` three times, an item count,
  a quest stage, a form list's size after AddForm); the data host answered
  a linked reference, a form from a file and a disabled flag on the way.

## Visible effect (`fo4-view --script-event`)

`fo4-view ... --under-hypotheses --headless --script-event INDEX:EVENT`
(a viewer built with `--features script-event`; the default build does
not link the interpreter and refuses the option) binds the cell's placed
reference INDEX, delivers the events with the data host, prints the effect
log, and for each reference of the loaded cells takes its last visibility
change in the log (`vm_disable_hides_reference_v1`): a `Disable` takes a
drawn reference out of the scene; an `Enable` of a reference flagged
initially disabled puts it in, its instance made as the cell load makes
one for an active reference (`fo4_scene::Loader::instance`), and drawn if
it is ready at the level. Every other native stays log-only. Without the
option nothing changes.

**No visible effect exists in the installed data under the current
rules.** The placed census classifies every visibility change of a placed
REFR made by handlers that all completed (4,175): 1,540 disables of an
enabled REFR, 33 of an initially disabled one, 2,577 enables of an enabled
one, 25 of an initially disabled one. For each, the target's instance is
made as the viewer would make it: 15 are drawable under every pending rule
(3 disables of an enabled REFR, 7 enables of an enabled one, 5 enables of
an initially disabled one), on 12 distinct references: 10 are editor
markers (EnableMarker or XMarker bases), which the viewer leaves out of
the frame under its marker rules, and 2 are references of a console panel
static that load handlers enable while they are already enabled (no
change). Every other target carries a blocker no rule covers: most often an
uninterpreted visual subrecord, a VMAD on the reference (a scripted
reference is never drawn), an enable parent (XESP), an animation or an
unsupported NIF block. 21 of the changes fall on 14 distinct targets that
are enable parents of other placed references (from 1 to 19 each); those
children carry XESP and are never drawn either, so following enable
parents would be the next step for a visible effect.

One case shows the path end to end: activating a door in an interior cell
runs a handler to completion that reads the door's linked reference by
keyword from XLKR and enables it, an initially disabled enable marker. In
the viewer the marker becomes a scene instance (one placement more), is
omitted from the draw list as an editor marker (one omission more), the
colour frames before and after are byte-identical, and the instance tags
map one to one (each tag to itself or to itself plus one, from the first
tag after the inserted instance). Activating a code button in another
interior cell completes and disables a placed activator found through its
linked references; the scene removes it, but it was not drawable before
(uninterpreted visual subrecords, material fields).

Byte identity of the viewer without the option is not decidable by frame
comparison on this machine (spec of the previous step: two builds of one
source differ in frames); the printed load reports are the check.

## Limits

- One installation, one game build; no measurement of the running game.
- Each delivery starts from a freshly bound object; event sequences are
  only those named on the command line; scripts attached on need start
  from their VMAD values and auto state, as if nothing had run on them.
- Natives are never performed; latent natives do not wait; the data host
  answers only the natives listed under Data host, from the data as at the
  start of a game.
- Quest stage fragments are read but no stage is ever set or run; INFO,
  PACK, PERK and SCEN fragment blocks stay raw ([vmad.md](vmad.md)).
- Opcode 34's arity between `4` and `4+n` stays open ([pex.md](pex.md));
  the interpreter reads its fourth slot as the start index.

## Provenance of this step

Recognised from prior knowledge, used only as hypotheses that a census
over the data could refute (each counted above or in
[vmad.md](vmad.md)):

- the QUST VMAD block layout (version byte, fragment count, fragment
  script entry, stage fragments of stage, word, log entry, byte and two
  texts, alias entries of an object value, version, object format and a
  script list); measured to the last byte in all 1,353 QUST VMADs, after
  one wrong first guess (a two-byte fragment field) that the census
  refuted;
- an alias-indexed object value naming a quest's alias;
- the linked-reference subrecord: first guessed as LNKR, which no placed
  record carries (refuted by the census); XLKR, keyword then reference;
- the initially-disabled header flag 0x800 (already read by `fo4_plugin`),
  NAME as the base, FLTV as a global's value, LNAM as a form list's
  entries, INDX as a quest stage and ALST as a quest alias index (the
  last two only as census checks);
- the meanings of the natives named under Data host, from their names and
  stored parameter names: GetLinkedRef, GetLinkedRefChain,
  CountLinkedRefChain, IsDisabled, GetBaseObject, GetFormFromFile (the
  24-bit and 12-bit masks, none for a plugin not loaded), GetValue,
  GetSize, GetAt, HasForm; and which natives change those facts (Enable,
  Disable, SetLinkedRef, AddForm and others receiving a list or a global);
- the record type to engine type table (`SIGNATURE_TYPES`), checked
  against the declared types of VMAD properties;
- that the natives ranked as runtime state (quest stages, 3D loading,
  open state, destruction, followers, triggers) depend on the running
  simulation; this only keeps them blocked.

No editor, editor source or definition file, wiki page, script source,
decompiler, other interpreter or the game executable was opened or
searched for.
