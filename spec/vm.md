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
| `ObjectReference.IsDisabled` | the record header flag 0x800 (the store's initially disabled); for a reference with an enable parent (XESP), the state its chain gives under the enable parent rules, with this run's Enable and Disable of the chain's records; this run's last Enable or Disable of the receiver itself first | `vm_host_is_disabled_flag_v1`, `vm_host_is_disabled_chain_v1` and the `xesp_*` rules of [enable-parent.md](enable-parent.md) | 4,032 placed records carry it; read and indexed alike in all 2,001,939 | 133,317 |
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
  get nothing. Quest stage fragments run only through the quest host
  (Quests, below).
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

**Regenerated in the events step.** [vm-census.txt](vm-census.txt) was
stale since the enable parent step (it lacked the `enable children:`
lines); every scope was run again with each host, so the recording-host
sections are now this binary's `--recording` reading rather than main's
older one (over the placed references one handler moves from an open cast
to a native, and two rules more are named). With `IsDisabled` following
enable parent chains (`vm_host_is_disabled_chain_v1`), over the placed
references 24,661 handlers complete (24,655 before), 6,678 block on a
native's result (6,686) and 730 fail (728); the bases alone and the test
cells do not change. `--data --no-enable-chain` reproduces the earlier
data host census line for line. The quest host adds 412 custom events
sent inside handlers (none delivered to a handler: a run delivers one
event, so registrations made by another event's run are not there), 1,910
remote registrations (1,865 for OnActivate) and 20 story events offered,
all left open by node conditions (functions 59, 77, 310, 359, 560, and
485 in 17); its handler outcomes equal the data host's.

**Quest host** (since the conditions step, `script_run`'s default,
`bind::Mode::Quest`; `--data` runs the data host alone as above,
`--recording` the recording host alone). Quest natives are answered from
the run's quest state in the isolated scenario (`quest_state_isolated_v1`:
whether a quest runs and which stages are done are undetermined until
the run sets them); a stage a handler sets runs its fragment, the quest
events and OnStageSet inside the call (`quest::Runner`, up to
`bind::QUEST_JOBS` = 256 jobs per run). Over the three scopes **no
handler's outcome changes**: the same 16, 1,319 and 24,655 complete and
the same natives block them, because what the handlers ask of quests
(`IsStageDone` 1,247 calls over the placed references, `IsRunning` 109,
`GetCurrentStageID` 29, `GetReference` of a quest whose running state is
undetermined 138) stays undetermined in that scenario. What changes is
what runs inside them ([vm-census.txt](vm-census.txt), last three
sections): over the placed references, 192 stage settings answered, 182
stage fragments run (32 completed, 148 blocked, 132 of them on
`Quest.IsStageDone`, 2 failed), 191 OnStageSet deliveries (21 to a
handler, all completed), 179 open jobs on whether the setting started
the quest and 2 on OnQuestShutdown; over the bases alone 183 fragments
(73 completed, 110 blocked), 197 OnStageSet deliveries (53 to a handler,
all completed), 132 open jobs on whether the setting started the quest,
2 on OnQuestShutdown and one on a log entry choice. The test
cells' handlers set no stage. A rerun with `--data` reproduces the
earlier census except the drawability split of 2,577 enabled
references (7 drawable then, 112 now), which the scene loader decides,
not this step.

**Regenerated again in the scene-events step** (all nine sections, one
binary built from main before that step's changes; `script_run` is not
touched by it). Every count of handlers, outcomes, natives, rules, events
and enable children is unchanged; only the drawability split moves,
because the scene loader now settles more models (effect-shader surfaces,
`ce0bedd`, the one `fo4-scene` change since the events step). Over the
placed references (data and quest hosts) 192 of the 2,577 enabled
references the handlers enable are drawable (112 before); enable children
drawable under every pending rule: disabled to enabled 16 of 41 (data) and
38 of 117 (quest) against 14 and 34, enabled to disabled 11 of 40 and 36
of 83 against 5 and 27; the bases alone and the test cells move the same
way (for example 6 of 12 enabled to disabled in the test cells, 2 before),
and 1 to 2 more runs change two to four drawable placements in each scope.
The same totals split differently; no line appears or disappears except
those run counts.

**Regenerated again in the second opening step** (all nine sections, one
binary built from main before that step's changes). Six pairs of lines
move and nothing else, one in each section of the bases alone and of
the placed references: the drawable / not drawable split of the enable
children made enabled (bases alone) and of the enabled references
disabled (placed references), under each of the three hosts, each pair
with the same total, because the scene crates changed since the
scene-events step.

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

**Before enable parents were followed, no visible effect existed in the
installed data under the rules of that step.** The placed census classifies every visibility change of a placed
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

**Enable children** ([enable-parent.md](enable-parent.md)). The scene now
decides a placement with an XESP through its parent chain under pending
rules, and the viewer gives every instance (and placed actor) whose chain
names a reference the log changes the state its chain now gives
(`xesp_script_change_propagates_v1`, `vm_event::propagate` and
`propagate_actors`), printing each change. `script_run --census` follows
the children of every changed reference: in the test cells one child
changes, an editor marker; over every placed scripted reference six runs
whose handlers complete change at least one placement the scene draws
under every pending rule (one run 11 references, one 6 actors and 2
references, four one reference each). Three of them were rendered before
and after the event: 48,377, 6,939 and 2,831 pixels differ, all on the
children's instances. A visible effect of a script therefore exists in the
installed data under the pending rules, through enable parents only; the
statement below describes the step before.

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

## Quests

`fo4_vm::quest` keeps a state per quest in the run's world and answers
the quest natives from it and from the quest record
([quest.md](quest.md), read by `fo4_plugin::quest`). Every behaviour is a
pending rule; none is measured.

**State** (`QuestState`): running, current stage, stages done,
objectives displayed, completed and failed, alias fills a run set. Each
item is known or undetermined; a native whose answer depends on an
undetermined item answers undetermined, so the run blocks where it would
steer. A native whose effect the rules do not settle (setting a stage the
record does not hold or an undetermined stage, `Reset`) makes the whole
quest undetermined. Two scenarios fix the starting state:

- *isolated* (`quest_state_isolated_v1`, the stage sweep): everything is
  undetermined until a run sets it; the quest whose stage the sweep sets
  is running; a stage a run sets counts as not done before (first
  settings are explored); a quest whose running state is undetermined runs
  after one of its stages is set, and whether that started it is an open
  job.
- *new game* (`quest_start_game_enabled_v1`): the quests whose DNAM flags
  carry bit 0 are started, in the store's order; nothing else runs; no
  stage is done and no objective set before; the current stage is
  undetermined until a stage is set.

**Quest host** (`QuestHost`: the data host plus these natives):

| Native | Answer | Rule |
| --- | --- | --- |
| `Quest.SetCurrentStageID(n)` (the installed `setStage` calls it) | stage n done and current; quest started if it was not running; true; the fragment of the log entry selected runs; already done: false and nothing, unless DNAM bit 3 | `quest_set_stage_v1`, `quest_stage_log_entry_v1`, `quest_repeated_stage_v1` |
| `GetCurrentStageID` (and `GetStage`), `IsStageDone` (and `GetStageDone`), `IsRunning` | from the state | `quest_set_stage_v1`, `quest_start_v1` |
| `Start` | running; startup stages (INDX bit 1) set; OnQuestInit then their fragments run; true. On a running quest: undetermined | `quest_start_v1` |
| `Stop` | not running; OnQuestShutdown runs | `quest_stop_v1` |
| `SetObjectiveDisplayed`, `SetObjectiveCompleted`, `SetObjectiveFailed`; `IsObjective...`; `HasObjective` | set and answered from the state; `HasObjective` from QOBJ; `CompleteAllObjectives`, `FailAllObjectives` make objectives undetermined | `quest_objectives_v1` |
| `ReferenceAlias.GetReference` | running quest: the ALFR reference, else undetermined; stopped: none; after `ForceRefTo` or `Clear` in the run: what they set | `quest_alias_forced_reference_v1` |
| `Alias.GetOwningQuest` | the alias's quest | `vmad_alias_value_v1` |

**Fragments.** A stage's fragment is the function the VMAD block names
for (stage, log entry) (`quest_fragment_entry_v1`), run with no argument
on one instance of the block's fragment script on the quest's handle,
made after the quest's own VMAD scripts are attached, with the block's
property values (`quest_fragment_instance_v1`); its alias properties hold
alias handles, which `GetReference` answers through ALFR. Which entry runs
(`quest_stage_log_entry_v1`): the first whose conditions hold; one
without conditions holds; none when no entry holds. The conditions are
evaluated by `fo4_vm::cond` (Conditions, below) in the state after the
stage is marked done and current (`cond_log_entry_state_v1`); an entry
whose conditions are undetermined keeps it and the entries after it as
candidates, and the choice stays open (an open job) unless every
candidate names the same fragment. `QuestHost::evaluate = false`
(`quest_run --no-conditions`) leaves every conditioned entry
undetermined, as before conditions were read. A fragment function stored
without instructions runs and returns at once (394 of the stages the
sweep sets have one).

**OnStageSet** (`quest_on_stage_set_v1`). After a stage is set (not when
a setting is refused as already done), `OnStageSet(stage, item)` is
delivered to every script instance on the quest's handle that has a
function with instructions for it, after the stage's fragment, inside
the same call; `item` is the position of the log entry selected, an
undetermined value when the selection is open or no entry holds. A
handler that does not take two Int fails as a bad argument (none does).
Over every winning quest, 193 of the quests' own VMAD scripts, on 191
quests, have a handler, all taking two Int; no fragment script and no
alias script has one (alias scripts would receive nothing under the
rule). Scripts that register for another quest's OnStageSet as a remote
event receive it after the quest's own scripts (Events and the story
manager, below). `QuestHost::stage_event = false`
(`quest_run --no-stage-event`) delivers nothing.

**Order** (`quest_stage_fragment_order_v1`). The candidate is the
*nested* reading: the fragment and quest events a native brings run at
once inside that native call (`Vm::nested`, `quest::Runner`), within the
enclosing run's step and depth limits. The *queued* reading (they run
after the calling script, in order; `quest::run_jobs` without a runner,
`quest_run --queued`) was run as the alternative. Neither is measured; the
data argue for nested: the queued reading answers `GetReference` with
none 69 times against 41 (aliases of a quest the calling script stops
right after setting the stage, whose fragment then runs on a stopped
quest) and leaves 84 cascaded fragments failing on a call on none against
58. The new-game sweep gives the same counts under both. After the
conditions step (below) the comparison is 70 against 34 and 85 against
57.

Since the conditions step, the jobs queued beside the one a runner runs
are set aside while it runs (`quest::run_jobs` with a runner and
`quest::Runner`): a job runs inside it only the jobs it brings, as the
rule states; before, a job queued earlier (OnQuestInit's startup-stage
fragments, now OnStageSet) could run inside a sibling that called any
native. Alone, this changes the stage sweep by 2 cascaded fragments
completed instead of failing on a call on none and 8 fewer `GetReference`
answers of none (`quest_run stages --no-conditions --no-stage-event`
against the earlier census); the new-game run is unchanged.

### Stage sweep (`quest_run MANIFEST stages OUT_DIR`)

For every winning quest and every stage its record holds that a fragment
names (10,071 stages of 1,273 quests; 632 quests have none), one fresh
interpreter (quest host, isolated scenario, scripts attached on need)
sets the stage and runs what follows, up to 256 jobs.
[quest-run-stages.txt](quest-run-stages.txt) has every count (log entry
conditions evaluated, OnStageSet delivered). 13 to 16 s wall, peak RSS
about 800 MB.

| The stage's own fragment | Stages before conditions | Stages now |
| --- | ---: | ---: |
| completed | 5,537 | 5,553 |
| blocked on a native's result | 4,121 | 4,124 |
| blocked on an undetermined value (open rule) | 28 | 28 |
| blocked on an object without the script | 1 | 1 |
| failed | 219 | 219 |
| open: which log entry's fragment runs depends on conditions | 162 | 130 |
| no fragment (no entry holds, or the one that holds has none) | 3 | 16 |

Of the 162 stages the conditions left open, 32 close: 16 run their
fragment to completion, 3 run it until it blocks, 13 run none (no entry
holds); 130 stay open. Over the sweep 538 log entry lists were
evaluated: 80 true, 35 false, 423 undetermined; 289 selections met a
conditioned entry, 63 were settled and 226 stayed open (130 of the
stages' own, 96 in cascades, against 126 before). The function indexes
of the undetermined conditions in the open selections, once per
selection: 59 (191; stages of a quest the isolated scenario does not
know as done or not), 629 (28), 491 (14), 70 (12), 46 (7), 84 (4), 14
(3), 1, 58, 71 and 74 (1 each).

- Natives that blocked the stage's fragment, most first:
  `ReferenceAlias.GetReference` 1,619 (aliases filled at run time),
  `Quest.IsStageDone` 1,237 (stages the isolated state does not know),
  `RefCollectionAlias.GetCount` 227, `Quest.IsRunning` 178,
  `Quest.IsObjectiveDisplayed` 172, `Actor.IsInFaction` 114, `Is3DLoaded`
  80, `Actor.IsDead` 73; the full ranking is in
  [vm-blockers.txt](vm-blockers.txt).
- OnStageSet after the stage's own fragment: 3,087 of the 10,071 stage
  sets reach a handler, 3,165 handler runs: 3,111 completed, 51 blocked
  (26 on a native's result, 23 of them `Utility.GetCurrentGameTime`; 23
  on an array operation the rules leave open; 2 on the undetermined
  item), 3 failed; in cascades 1,644 handler runs (1,526 completed). The handlers add 60 objects placed, 85 scene calls, 43
  timers and 11 global sets to the effects; no stage set.
- Failures: a call on none (op 23) 164 and a property on none 1 (a
  property or alias the data leaves none, mostly `MoveTo` and
  `EnablePlayerControls` targets),
  `BadArgument` 23 (VMAD object values with an invalid master index,
  refused as the binding refuses them), array operations on none 21,
  `MissingFunction` 4, `MissingScript` 2, `IndexOutOfRange` 3, `Ambiguous`
  1.
- 4,607 stages ran with every job completed and no open job (4,617
  before; 4,646 with conditions and without OnStageSet: the handlers'
  own blocks take 39 away).
- Cascades: fragments run per stage set, the stage's own included: 1 in
  8,520 stages, 2 in 904, 3 to 9 in 408, 10 to 42 in 93, none in 146
  (the 130 open ones and 16 without a fragment); depth
  up to 8; no stage reached the job limit and no run reached the step or
  depth limit, so no cycle runs unbounded (141 settings of an already done
  stage were refused by `quest_repeated_stage_v1`). Cascaded fragments:
  3,955, 2,256 completed, 1,624 blocked, 75 failed; open jobs: 1,736
  (whether setting the stage started another quest), 188 (whether a
  stopped quest ran OnQuestShutdown), 96 (log entry conditions).
- Effects (native calls) by kind over all runs: stage set on another
  quest 4,383 and on the same quest 618, objectives displayed or hidden
  2,480, completed 2,432, failed 82, enable 2,006, disable 1,709, global
  set 1,728, scene 1,527, actor state 1,243, quest stopped 1,219, move
  1,092, items 849, event registration 812, alias fill changed 740, lock
  or open state 607, timer 613, actor value 462, faction 239, quest
  started 197, objects placed 155; reads and other natives are ranked in
  the file.

### New game (`quest_run MANIFEST new-game OUT_DIR`)

One interpreter starts the 457 quests flagged to start with the game
([quest-run-new-game.txt](quest-run-new-game.txt)); 1.7 to 2.5 s. Second
pass, with log entry conditions evaluated and OnStageSet delivered (the
first pass's numbers in brackets):

- 664 jobs ran (539): 459 OnQuestInit (457 starts, one `Start` and one
  stage set from scripts), 73 stage jobs, 125 OnStageSet, 7
  OnQuestShutdown. 436 OnQuestInit jobs found no handler; the other 23 ran
  24 handlers: 12 completed, 10 blocked, 2 failed.
- 125 stages set on 98 quests (108 by starts, 17 by scripts), as before;
  72 fragments: 59 completed, 10 blocked (`RefCollectionAlias.GetCount`,
  `Actor.GetLevel`, `Game.IsPluginInstalled`, `Scene.IsPlaying`,
  `Is3DLoaded`, `ReferenceAlias.GetReference`; one open rule), 3 failed;
  one stage still open by its log entry conditions (two lists, both
  undetermined by function 14).
- OnStageSet: 125 deliveries, 50 to a handler: 28 completed, 21 blocked
  (20 on `ObjectReference.AttachMod`, one on
  `RefCollectionAlias.GetCount`), 1 failed (an array of none). The
  handlers add 20 objects placed (29 against 9), 40 form list reads and
  one alias fill.
- 452 quests run at the end (7 stopped by scripts); **no objective is
  displayed**; three placed references, none flagged initially disabled,
  are enabled; nothing is disabled.
- Other effects: 17 stages set on other quests, 41 event registrations,
  15 alias fills, 12 lock or open state changes, 8 moves, 7 item changes,
  4 scene calls.

Where it stops, ranked (what the next steps can be chosen from):

1. **Events nobody delivers.** The 71 event registrations made by the
   start are never answered: 29 tracked-statistics events, 23 remote
   events (`OnLocationChange` 10, `OnLoad` 3, `OnUnload` 3,
   `OnPlayerLoadGame` 3, `OnStageSet` of another quest 2, two on an
   undetermined sender), 18 custom events of other scripts, one
   player-sleep registration (the census's 41 "event registration"
   effects are the remote and custom ones). Engine events that need no
   registration (the player's cell attaching and loading, triggers,
   scenes ending, dialogue) are not delivered either.
   The main quest of the game's opening is not among the 457 flagged
   to start (some of its helper quests are), so its start, and with it
   every objective, waits on something the model does not have: an
   engine event, a story-manager start or a script outside these runs.
2. **Natives whose result steers a run:** `ObjectReference.AttachMod` 20
   (all in OnStageSet handlers), `RefCollectionAlias.GetCount` 6,
   `Actor.GetLevel` 3, `Game.IsPluginInstalled` 3, `Scene.IsPlaying` 3,
   `Is3DLoaded` 2, `ReferenceAlias.GetReference` 2; and 33
   `GetReference` answers left undetermined that steered nothing
   (aliases without a forced reference: alias filling is not modelled).
3. **Conditions:** one log entry selection open, on function 14.
4. **Open rules and failures:** an array operation on none, a cast, a
   property on none, a missing function, a bad argument, two calls on
   none, one array of none in an OnStageSet handler.

This is the second picture of what the scripts do when a new game
starts, under every rule above: conditions change almost nothing here
(the new-game state settles all but one selection) and OnStageSet adds
50 handler runs; the opening still waits on events the interpreter does
not deliver (the player in the first cell, triggers, scenes ending,
dialogue, remote events) rather than on the quests' start.

## Events and the story manager

Since the events step a run keeps the registrations scripts make
(`fo4_vm::events`, in the run's world) and delivers the events scripts
send each other, but only where the interpreter raises the event itself;
the story manager's script event starts the quest a node selects
(`fo4_vm::story`, [story-manager.md](story-manager.md)). Each part is a
pending rule and each can be switched off (`quest_run --no-remote`,
`--no-custom`, `--no-story`; `script_run --no-events`, `--no-story`),
which reproduces the earlier censuses byte for byte (checked for both
quest sweeps).

| What | Rule | How |
| --- | --- | --- |
| `RegisterForRemoteEvent`, `UnregisterForRemoteEvent`, `UnregisterForAllRemoteEvents` | `vm_remote_events_v1` | the calling instance registers for (sender, event name); `RegisterForRemoteEvent` answers true for an object sender (undetermined otherwise, nothing registered) |
| delivery of a remote event | `vm_remote_events_v1` | when the run delivers OnStageSet, OnQuestInit or OnQuestShutdown to a quest, or a `script_run` event to the bound reference: every registered instance receives `::remote_<type>_<event>` (a type the sender is) with the sender and the event's arguments, after the sender's own scripts, in registration order |
| `RegisterForCustomEvent`, `UnregisterForCustomEvent`, `UnregisterForAllCustomEvents`, `SendCustomEvent` | `vm_custom_events_v1` | `SendCustomEvent(name, args)` on an object delivers `::remote_<name>(sender, args)` to every instance registered for (that object, name) |
| when a custom event runs | `vm_custom_event_order_v1` | queued: after the outermost delivery or job running when it was sent, in sending order (a `Job::Custom`); `--custom-nested` runs it inside the call instead |
| `Keyword.SendStoryEvent`, `SendStoryEventAndWait` | `sm_script_event_v1`, `sm_tree_walk_v1`, `sm_quest_node_start_v1`, `cond_fn_576_event_data_v1` | the event `SCPT` with the receiver as keyword is offered to its tree at once; a quest started is started as `Start` starts it, then receives `OnStoryScript` (a `Job::Story`); `AndWait` answers true, false or undetermined |
| `IsDisabled` of a reference with an enable parent | `vm_host_is_disabled_chain_v1`, `xesp_*` | the state its chain gives (data host; `--no-enable-chain` reads the header flag alone) |

The handler names are read from the compiled scripts: 683 functions are
named `::remote_<type>_<event>`, their first parameter the sender typed
by the type in the name; the 224 registering and 96 sending call sites of
custom events store the name as `<script>_<event>`, and the handlers take
the sender and a `Var` array. Events the world raises (location changes,
cells loading, combat, deaths, tracked statistics, sleep) and the other
17 story event codes are never raised: the registrations for them stay,
counted.

**New game, third pass** (`quest_run new-game`, events, story manager
and the enable chain on): **the run is the same as the second pass** in
every job, outcome, effect and blocker. 21 remote registrations are made
(onlocationchange 10, onload 3, onunload 3, onplayerloadgame 3,
onstageset 2) and 2 are not (an undetermined sender), 17 custom ones (one
not); at the end 2 wait for an event the run raises (OnStageSet of a
quest no run sets a stage of), 19 for an event of the world, 17 for a
custom event nobody sends. No script sends a custom event or a story
event. The queued and nested readings of custom events give the same
counts, and so does the enable chain switched off.

**Stage sweep** (`quest_run stages`): the stages' own fragments and
OnStageSet runs are unchanged. Custom events: 564 registrations, 346
events sent, 11 delivered to a handler (4 completed, 7 blocked on an array
operation the rules leave open); no remote delivery happens (the 7
OnStageSet registrations are for quests no later job in the same run sets
a stage of); 6 story events offered, all left open by conditions of the
nodes on the way (functions 59, 77, 310, 359, 560 on each, 14, 562 and
660 once). One sweep item fewer completes every run (4,606 against
4,607). Queued and nested custom events give the same counts; the enable
chain changes nothing here either.

**Where the opening stops.** The opening's main quest is not flagged to
start, no story manager node lists it, and the only starting call sites
the static scan resolves to it set stages in the hundreds from fragments
and scripts of quests that wait on world events (triggers, scenes,
activations); its first stage is set by nothing the data shows
([story-manager.md](story-manager.md)). Set by hand after the start
(`quest_run new-game --then`, an assumption of the caller, not a rule),
its stage 0 fragment runs to completion (character generation set up,
the player moved, a real-time timer of one second started); firing the
timers (`--timers`) stops at once, because a game-time timer of another
quest is queued beside it and `vm_timer_events_v1` does not order the two
clocks.

What would take the opening further, most first (counts from the runs
above and [vm-blockers.txt](vm-blockers.txt)):

1. **What sets the first stage of the opening's main quest**: not in any
   record read today; the fragment blocks of INFO (2,396 VMADs), SCEN
   (632), PACK (166) and PERK (32) stay raw, and 62 quests not flagged to
   start have neither a node nor a constant call. Reading those blocks is
   the next reader step.
2. **World events**: 19 registrations of the new game wait on location
   changes, loading and game loads; the story manager's other 17 event
   codes list 537 quests; tracked statistics (29) and sleep (1)
   registrations; the timers' two clocks.
3. **Natives that steer a run** (new game): `ObjectReference.AttachMod`
   20, `RefCollectionAlias.GetCount` 6, `Actor.GetLevel` 3,
   `Game.IsPluginInstalled` 3, `Scene.IsPlaying` 3, `Is3DLoaded` 2,
   `ReferenceAlias.GetReference` 2 (and 33 references of aliases without a
   forced reference left undetermined).
4. **Condition indexes**: 14 (one log entry selection of the new game);
   on the story manager's nodes 576 with selectors other than 0 `K1` and
   `L1` (59 conditions), 359 (93), 71 (45), 426 (28), 310 (25), 77 (24),
   560 (24).

## Scenes and fragments

Since the fragments step the fragment blocks of INFO, PACK, SCEN and PERK
are read and scene records are read far enough to place them
([scene-record.md](scene-record.md)). `fo4_vm::scene` keeps a state per
scene in the run's world and the quest host answers the scene natives
from it when scenes are on (`World::scenes.on`; `quest_run` turns it on,
`--no-scenes` leaves the natives to the data host as before; `script_run`
and the viewer keep it off). Each part is a pending rule and can be
switched off; with `--no-scenes` the two quest sweeps reproduce the
earlier censuses byte for byte, and `story_census --no-fragments` the
earlier story census (checked).

| What | Rule | How |
| --- | --- | --- |
| `Scene.Start`, `ForceStart` | `scene_start_v1` | a scene not playing whose owning quest (PNAM) runs plays: its begin fragment, then its phases; a stopped quest refuses it; Start of a playing scene does nothing; a scene or quest whose state is undetermined leaves the scene undetermined |
| `Scene.Stop`, `IsPlaying` | `scene_start_v1` | not playing, no fragment run; the state, false for every scene at the start of a new game, undetermined in the isolated scenario until the run sets it |
| phases | `scene_phase_progression_v1` | offered in stored order: start conditions false pass a phase over, true enter it (its begin fragment); it completes (its end fragment, then the next phase) only when no action of the scene spans it and its completion conditions are absent or true; after the last phase the scene's end fragment runs and it stops playing; anything else keeps it in the phase, counted by reason |
| FNAM bit 0x1 | `scene_begin_on_quest_start_v1` | the quest's scenes with the bit start when it starts, after its start jobs (and after the fragment of the stage whose setting started it) |
| `Scene.GetOwningQuest`, `TopicInfo.GetOwningQuest` | `vm_host_owning_quest_v1` | PNAM; the quest whose group of kind 10 holds the INFO |
| fragments | `vmad_fragment_instance_v1` | one instance of the block's script on the record's handle with the block's property values; scene fragments run as jobs (`Job::Scene`) in the order above, nested like stage fragments |
| two timer clocks | `vm_timer_time_scale_v1` | only when a caller fires timers (`quest_run --timers`): a game-time timer of h hours is due after h x 3600 / s real seconds, s the GLOB whose editor ID the caller names with `--time-scale-global` (its role an assumption): the value a run last set with `SetValue`, else its FLTV; s = 0 stops the game clock; without that switch the earlier stop is kept |

What a phase waits on is never advanced: an action spanning it (a spoken
line, kind 0; a package, 1; a timer, 2; player dialogue, 3; a start-scene
action, 4; a response line, 5; radio, 6), completion conditions that are
false, conditions the evaluator leaves undetermined. INFO, PACK and PERK
fragments are bound and runnable on request only: no world event (a line
spoken, a package starting, a perk entry firing) runs them.

**Fragment sweep** (`quest_run MANIFEST fragments OUT_DIR`,
[quest-run-fragments.txt](quest-run-fragments.txt)): every INFO, PACK and
PERK fragment in a fresh interpreter (isolated scenario), its parameters
undetermined (the speaker, actor or target is not known outside the
world). 2,779 runs, 2.7 s, peak RSS about 900 MB: INFO begin 1,154 (950
completed, 116 blocked on a native's result, 4 on an undetermined
parameter, 83 failed, 1 script missing), INFO end 1,401 (1,149 completed,
207 blocked, 28 on a parameter, 7 failed, 9 scripts missing, 1 function
missing), PACK 188 and PERK 36 (in the file). The INFO fragments set 456
stages and display 54 objectives in the isolated scenario; 80 of the 83 INFO begin
failures are one DLC script that calls `Play` on a property left none. Natives that
block them most: `Quest.IsStageDone` 116, `Utility.RandomInt` 48,
`ReferenceAlias.GetReference` 38, `ObjectReference.GetValuePercentage` 33, `GetDistance` 30,
`Package.GetOwningQuest` 28 (not answered) ([vm-blockers.txt](vm-blockers.txt)).

**Scene sweep** (`quest_run MANIFEST scenes OUT_DIR`,
[quest-run-scenes.txt](quest-run-scenes.txt)): every scene started in a
fresh interpreter, its quest taken as running and the scene as not playing
(as the stage sweep takes its quest). 4,773 scenes, 1.8 s: 4,601 phases
entered, 50 completed, 6 passed over; 32 scenes run to their end; the
others stop on an action (dialogue 2,495, player dialogue 689, package 660,
radio 400, timer 165, start-scene 25, response 11) or on conditions
(undetermined start conditions 190, completion 104, by function index in
the file; false completion conditions 2). Scene fragments: begin 119 (80
completed), phase begin 83, phase end 16, end 1.

**Stage sweep** with scenes on: the stages' own fragments and OnStageSet
are unchanged; in the isolated scenario 892 `Scene.Start` calls leave the
scene undetermined (whether it plays is unknown), 16 start one (14 phases
entered), 551 `Stop` calls are recorded.

**New game, fourth pass** (`quest_run new-game`, scenes on, the
time-scale global named by the caller with `--time-scale-global`;
[quest-run-new-game.txt](quest-run-new-game.txt)): 757 jobs (664), 93 of
them scene steps. 47 scenes start, 46 with their quest (FNAM bit 0x1)
and 1 from a script; 2 starts are refused (quest not running). 46 phases
are entered and none completes: the scenes wait on radio (26), packages
(8), spoken lines (5), timers (3) and conditions of functions 613, 614 and
67 (5). The three fragments blocked on `Scene.IsPlaying` before now
complete (62 fragments complete, against 59), and start three real-time
timers. Timers asked: real time 1,800 s once and 900 s twice, game time 4 h
once; the time-scale global stands at its stored 20. **No objective is
displayed.** With `--no-scenes-with-quest` 25 scenes start from scripts
and 713 jobs run.

**Where the opening stops now.** What starts the opening's main quest is
still in no record read: it is not flagged to start, no story manager node
lists it, no DOBJ entry names it, no constant `Start` or `SetStage` call of
any compiled script reaches its first stage, and no INFO, SCEN, PACK or
PERK fragment starts it or sets a stage of it (the static scan with the
fragment blocks, and the fragment sweep). Its first stage is a startup
stage (INDX flag 0x2): starting the quest sets it. A scan of every
subrecord of every winning record for its FormID finds it only as a
dialogue or package quest, in conditions, in scene owners, in the
properties of stage-setting triggers of the opening cells and of other
opening quests' scripts; the player reference (FormID 0x14) is in no
plugin. The placed scripted references of the opening's exterior cells
(`script_run --census`, the cell of the opening house with radius 1:
181 references, 491 handlers) have no OnInit or cell-load handler; their
23 trigger handlers set later stages of the quest and block on
`Quest.IsStageDone`. What starts it is therefore outside the data read:
the engine at a new game, which only the running game can show.

Started by hand (`--then` the quest's stage 0, an assumption of the
caller) with scenes on: its stage 0 fragment runs to completion (the
time-scale global set to 0, character generation set up, the player
moved, a one-second real-time timer started), and its first scene (FNAM
bit 0x1) starts with it and enters phase 0, whose completion condition
(function 555) the evaluator leaves undetermined. Firing timers
(`--timers 50`) under `vm_timer_time_scale_v1` now orders the two clocks:
the game clock stands still (time scale 0), the 4-hour game-time timer of
another quest never comes due, and the opening's one-second timer fires 50
times, restarting itself each time with one lighting call and nothing
else; no objective is displayed. Without the rule (no `--time-scale-global`) the
firing stops at once as before.

What would take the opening further, most first:

1. **The engine's start of the opening quest** (and of the 60 quests with
   no node and no constant call): a measurement of the running game at a
   new game (which quests run before the first frame, which stage is
   set), not a record.
2. **World events that end a scene's phase**: spoken lines (2,495 phase
   waits in the scene sweep), player dialogue (689), packages (660),
   radio (400), timers (165); in the opening, the first scene's phase 0
   waits on condition function 555 and the stage 0 timer polls with no
   state change (its loop ends on something the world does).
3. **Condition indexes** on phases (waits in the scene sweep naming the
   index): 1 (85), 59 (77), 629 (28), 555 (27), 14 (17), 613 (12), 614
   (11), 67 and 58 (10 each).
4. **Natives that steer a run** (new game): `ObjectReference.AttachMod`
   20, `RefCollectionAlias.GetCount` 6, `Actor.GetLevel` 3,
   `Game.IsPluginInstalled` 3, `Is3DLoaded` 2,
   `ReferenceAlias.GetReference` 2 (`Scene.IsPlaying` 3 is answered now).

## Scene clock

Since the scene-events step a scene's actions can let time pass
(`fo4_vm::scene`, `scene_clock_v1`), only when the caller asks for it
(`quest_run --scene-clock N`, `new-game` and `scenes`); without it every
census reproduces byte for byte (checked with `cmp`: the four
`quest-run-*.txt`, `conditions-census.txt`, `conditions-blockers.txt`,
`story-manager-census.txt`, `fragment-census.txt`, and the quest-host
sections of `vm-census.txt` for the test cells and the bases alone).

| What | Rule | How |
| --- | --- | --- |
| actions | `scene_clock_v1` | an action starts when its start phase is entered and holds every phase from its start to its end phase until it completes; a phase whose spanning actions are complete goes on to its completion conditions as before; an action whose time is not settled holds its phases with the earlier reason (by kind) |
| the clock | `scene_clock_v1` | the real-time clock timers count on (`World::now`); `scene::advance_clock` moves it to the next due action, completes every action due by then and queues each scene's completion step; the game-time clock moves with it by the time scale when the caller knows it. `quest_run` interleaves clock advances with timer firings (`--timers`), earliest first, timers first on a tie (`Vm::next_timer_wait` gives the wait the next firing takes) |
| timer actions (kind 2) | `scen_timer_action_seconds_v1` | the SNAM after the end phase as f32 seconds; undetermined with a TNAM (174 actions, all lower than SNAM) or any FNAM bit (5); `--no-timer-actions` turns the rule off |
| dialogue actions (kind 0) | `scene_dialogue_line_audio_v1` and the rules of [voice.md](voice.md) | the seconds of the line's audio: the first line of the topic whose conditions hold, its responses, the speaker's voice files, the seconds their headers declare; `--no-line-audio` turns it off |
| caller assumptions | none (counted as assumptions) | `--assume-line-seconds X`: a line whose audio does not settle its time (or every line with `--no-line-audio`) lasts X s; `--assume-player-choice N`: player dialogue (kind 3) lasts the player's line of choice N then its answer (`scene_player_dialogue_choice_v1`); `--assume-packages-complete`: a package action (kind 1) completes as it starts |

Start-scene (4), response (5) and radio (6) actions are not timed. The
census counts every action started by how its time was settled or why not
(`scene clock: action started`), every action completed, and every phase
that waits only on actions with a due time.

**Scene sweep with the clock** (`quest_run MANIFEST scenes OUT_DIR
--scene-clock 200`, [scene-clock-census.txt](scene-clock-census.txt)):
scenes run to their end, of 4,773 started:

| Variant | Run to their end | Phases entered | Phases completed |
| --- | ---: | ---: | ---: |
| no clock (as before) | 34 | 4,617 | 50 |
| timer actions only | 41 | 4,747 | 192 |
| line audio only | 599 | 6,034 | 2,054 |
| clock alone (both rules) | 651 | 6,306 | 2,387 |
| + `--assume-line-seconds 3` | 1,281 | 7,800 | 4,530 |
| + `--assume-player-choice 0` | 651 | 6,306 | 2,387 |
| + `--assume-packages-complete` | 866 | 7,630 | 3,957 |
| + line seconds and player choice | 2,117 | 10,106 | 7,756 |
| + line seconds and packages | 1,635 | 9,968 | 7,096 |
| + all three | 2,822 | 13,779 | 12,278 |

With the clock alone, 2,843 dialogue actions are timed by their audio and
317 timer actions by their seconds; 1,262 phase stops still wait on a line,
mostly because several voice types hold it and the speaker alias is filled
by the world (228), the topic holds no line (457), or which line plays is
undetermined (function 59 in 122, 426 in 20, others below 15). Player choice
alone changes nothing: the player's voice type is not in the records, so
the player's line is timed only with `--assume-line-seconds`. With every
assumption the stops left are radio (433), start-scene actions (436),
response lines (193), timers with a TNAM or a flag (54), conditions (118
false completion, 450 undetermined start, 354 undetermined completion) and
6 package actions whose start phase was passed over.

**New game with the clock** (`quest_run new-game --time-scale-global
<time-scale global> --scene-clock 200`, section 4 of the census): two
clock advances complete three timer actions and two phases of the 46
scenes that start; nothing else changes (no objective).

**The forced opening with the clock** (`quest_run new-game --then <opening
quest>:0 --timers 50 --time-scale-global <time-scale global>`, every stop in
order; the stage reached is the opening quest's stage 0 in every variant,
no objective is displayed in any, and the stage 0 timer polls every second
throughout):

1. Without `--assume-cond 555=1`, with or without the clock: the first
   scene stops in phase 0 on its completion conditions of function 555
   (undetermined).
2. With `--assume-cond 555=1` and the clock: phase 1 holds a 3-second timer
   action, which completes at 3 s, and a line whose topic holds no line
   (stored line count 0): it waits, a dialogue action not timed. Adding
   `--assume-player-choice 0` or `--assume-packages-complete` changes
   nothing there.
3. Adding `--assume-line-seconds 3`: phase 1 completes at 3 s; phases 2, 3
   and 4 each hold one line whose choice is undetermined (function 70) and
   complete at 6, 9 and 12 s on the assumed seconds; phase 4 then stops on
   its completion condition, function 131 of the scene itself compared
   with >= 50 (undetermined). The same with every assumption.
4. Exploring further with `--assume-cond 131=50` as well: phases 5 and 6
   (lines, function 70; then 70 and 72) complete at 15 and 18 s, phase 7 at
   once, phase 8's package action completes on the assumption, and phase 8
   stops on its completion condition, function 59: stage 20 of the opening
   quest done, false. No run sets that stage: the next stop is a world
   event (the opening quest's own script sets stages from animation and
   distance events, none of them stage 20; what sets it is not
   determined). Since the stage-setters step the table finds no setter
   of that stage anywhere in the data; Stage setters, below, continues
   the opening from there.

## Stage setters

Since the stage-setters step `fo4_vm::setters` builds a static table of
every place in the data that can set a quest stage, with what each place
needs to happen first; nothing runs to build it. The census tool is
`stage_census` ([stage-setter-census.txt](stage-setter-census.txt), counts
and kinds only; the quest by quest list with names goes to a file that is
never versioned). 9 to 30 s wall, peak RSS about 1.1 GB.

| Kind of setter | How it is found | Rule |
| --- | --- | --- |
| a call in a compiled script | every call of the scripted `setStage` or the native `SetCurrentStageID` on a quest receiver (the definitions on the quest type left out), its quest and stage followed back within the function to a literal, `self`, `GetOwningQuest()` on `self` or on a property, a property (resolved per attaching record by the values its VMAD stores, an int property without a stored value by its compiled initial value), a struct property's members together, any element of an array property, or a parameter, resolved at each call of the function up to three calls outward (the quest type's struct-taking function among them) | `setter_static_scan_v1` |
| what attaches the script | every winning VMAD: a quest's own scripts, its alias scripts, its stage fragment script; the TERM, INFO, PACK, SCEN and PERK fragment blocks with each fragment's slot; the own scripts of placed references, base objects, lines, scenes, packages, perks, magic effects; a script no read VMAD attaches is still scanned | `vmad_reference_scripts_v1`, `vmad_fragment_blocks_v1` |
| the event that runs it | an engine event (a function without instructions named `on...`, declared by a script that declares a native function: 159 names) runs the function of its name; `::remote_<type>_<event>` a remote event; another `::remote_` function a custom event; a fragment its slot; calls on `self` and parent calls carry an entry to the functions they call; a function no entry reaches is counted apart | `setter_event_entry_v1` |
| record fields | INFO TIQS, scene SCQS and phase SCQS: two i16, the stage of the owning quest set at the begin and at the end (negative: none) | `record_set_stage_fields_v1` |
| startup stages | a stage with INDX flag 0x2, set when its quest starts | `quest_start_v1` |

Measured over one installation (every count in the census):

- 26,361 setters (one per distinct place, attachment, stage and need):
  calls in quest stage fragments 5,233, quests' own scripts 4,294, alias
  scripts 4,116, placed references' own scripts 1,688, base objects'
  scripts 522, INFO fragments 513, INFO own scripts 130, terminal fragments
  164, scene fragments 146, package fragments 56, perk fragments 6, magic
  effects 4, scripts no read VMAD attaches 1,150; record fields 7,367 (INFO
  TIQS 5,875, phase SCQS 910, scene SCQS 582); startup stages 972. 3,521
  of the calls go through a function passing a parameter on (73 calls of
  such functions at depth 1, 147 at depth 2, none at 3).
- What they need first, by family: another stage or a quest start (a
  chain) 6,187, a dialogue line 6,388, an engine event 5,189, a player
  action on an object (activation, trigger enter or leave, a terminal
  item) 1,917, a scene's progress 1,638, a timer of the script 1,107, a
  custom event 799, a package 56, a perk entry 6; 3,074 sit in functions
  no event or fragment of their own script reaches. (Since the graph step the
  table follows those functions one call outward to other scripts'
  callers, and the committed census counts 28,922 setters, 2,368 of them
  in functions nothing reachable calls: Calls from other scripts in the
  stage-setter table, below; `stage_census --no-outer-calls` gives the
  counts above.)
- Resolution: the quest is a constant in 24,506 setters, the stage in
  24,646; 2,491 constant setters name a stage the quest record does not
  hold (1,051 pairs), mostly generic scripts whose int property has no
  stored value; of the record fields' non-negative halves 117 of 5,875
  TIQS, 6 of 582 scene SCQS and 7 of 910 phase SCQS name no stage of the
  owning quest (one 2-byte SCQS inside an action is counted and not read).
- **Stages**: of the 12,855 stages of the 1,905 winning quests with
  stages, **2,297 have no setter found at all** (none a startup stage);
  584 quests have no stage with a setter. 6,603 stages have one setter,
  1,961 two, 1,310 three or four, 684 five or more.
- Quest starts: 2,049 starters on 1,625 quests (457 flagged to start with
  the game, story manager nodes by event, 254 `Start` call sites).
- Location special references: 300 LCTN lists (LCSR) all divide into
  16-byte rows, every row an LCRT then a placed REFR, ACHR or PHZD; of the
  2,850 reference aliases filled from a location alias with a ref type,
  790 resolve to exactly one row (`quest_alias_location_ref_v1`).

**Reachability** (`quest_run new-game --next-blockers`): at the end of a
run, every scene stopped in a phase on its conditions is read for
conditions of function 59 (and 58 compared with `==` or `>=`) on a stage
the run has not set, and the quests the caller set a stage on add their
next three stages not done; for each such stage the table gives its
setters, by kind and need, with the need's state in the run (a chain stage
done or not, a scene point reached or not, a player action on a reference
or on the reference an alias holds, which the caller can assume, an event
the interpreter does not raise). `quest-run-next-blockers.txt` holds the
counts and kinds; the names file names the records and the switch that
would deliver a player action.

### Player-event assumptions and the readings behind switches

`fo4_vm::player` delivers the engine event of a player action on a placed
reference when the caller assumes it: `quest_run --assume-activate`,
`--assume-trigger-enter`, `--assume-trigger-leave`, `--assume-open`, each
with a reference as `plugin.esm:0001ABCD` given on the command line (never
stored), at once after the start or `@SECONDS` on the real-time clock
(with `--scene-clock`); `--then EDITOR_ID:STAGE@SECONDS` likewise. The
event runs on the reference's scripts (base VMAD, then its own), then on
the scripts of every quest alias holding it, then on the instances
registered for it as a remote event, each handler the function of the
event's name the compiled scripts declare with the arguments its
parameters ask for (none, or the player for one object parameter); nothing
else the action does in the game (a door opening, the default activation)
is performed. Every assumption is counted as one in the census.

| Switch | Reading | Rule |
| --- | --- | --- |
| `--assume-activate`, `--assume-trigger-enter`, `--assume-trigger-leave` | caller assumption; who receives the event | `vm_event_handlers_v1`, `vm_event_alias_scripts_v1` |
| `--assume-open` | caller assumption; OnOpen with the player as opener | `vm_event_open_v1`, `vm_event_alias_scripts_v1` |
| `--then Q:S@SECONDS` | caller assumption: a stage set from outside any script at that second | none |
| `--scene-recheck` | a scene stopped on its phase's conditions is offered that phase again after any change of quest state | `scene_condition_recheck_v1` |
| `--stage-fields` | scene and phase SCQS set stages when a scene starts or ends and a phase is entered or completed (INFO TIQS only with `--line-stage-fields`, below) | `record_set_stage_fields_v1` |
| `--cond-550-clock` | function 550 reads the actions the scene clock completed | `cond_fn_550_clock_actions_v1` |
| `--alias-location-refs` | a reference alias filled from a location alias with a ref type holds the location's special reference | `quest_alias_location_ref_v1` |
| `--assume-start-scene-actions` | caller assumption: a start-scene action completes as it starts; it starts the scene LCEP names | `scene_start_scene_action_v1` |

Each is off by default. With none of them every versioned census
reproduces byte for byte (checked with `cmp`: the four `quest-run-*.txt`,
`conditions-census.txt`, `conditions-blockers.txt`,
`conditions-blockers-before.txt`, `story-manager-census.txt`,
`fragment-census.txt`, `voice-census.txt`, `scene-clock-census.txt`; in
`vm-census.txt` every line but three pairs of drawable / not drawable
counts of enable children in the sections of the bases bound alone, which
main's own binary gives exactly as this step's: they were stale before it).

**The forced opening, stop by stop** (`quest_run new-game --then <opening
quest>:0 --timers 50 --time-scale-global <time-scale global>
--scene-clock 200`; the concrete records are in the step's notes). Each
stop and what passed it, in order:

1. The first scene's phase 0 on function 555: `--assume-cond 555=1`.
2. Phase 1's line whose topic holds no line: `--assume-line-seconds 3`.
3. Phase 4 on function 131: `--assume-cond 131=50`.
4. Phase 8's package action: `--assume-packages-complete`.
5. Phase 8 on function 59, a stage of the opening quest **for which the
   table finds no setter at all**: `--then <quest>:<stage>@18` with
   `--scene-recheck`; the scene ends. (With `--stage-fields` the scene's
   phase fields had already set three earlier stages.)
6. The quest's next stage: a trigger-enter handler of a placed trigger
   reference (through the shared set-stage helper): `--assume-trigger-enter
   <reference>@19`. The stage fragment starts a second scene and a helper
   quest; with `--stage-fields` the second scene's end sets a stage, a
   third scene starts, and two of its phase fields set two more.
7. The third scene's phase 4 on function 550 of its own timer action:
   `--cond-550-clock`. The scene ends; its end field sets a stage whose
   fragment sets another, whose fragment sets a third; a fourth scene
   starts and runs on its timers, lines and packages, two of its phase
   fields setting two stages.
8. The fourth scene's start-scene action: `--assume-start-scene-actions`;
   it starts a fifth scene that loops on a timer with a TNAM (time not
   settled), and the fourth scene ends.
9. The next stage's setter is the activation handler of an alias filled
   from a location alias by a ref type: `--alias-location-refs` fills it
   (one special reference), then `--assume-activate <that reference>@90`
   sets the stage; its fragment opens the reference by script and stops
   the two scenes.
10. The next stage's setter is the alias's OnOpen handler:
    `--assume-open <reference>@90` delivers it, and the handler **blocks
    on an open interpreter rule** (`pex_array_ops_v1`: the length of an
    array property left none, inside the shared set-stage helper). No
    caller assumption passes this stop.

Reached: **15 of the opening quest's 104 stages** (one of them set by the
caller), no objective displayed. Exploring past the stop with the stage
set by hand (`--then <quest>:<stage>@91`) and `--assume-player-choice 0`:
a sixth scene starts and runs through two player dialogue actions to its
phase whose field sets one more stage (17 stages); the next stages need a
dialogue line's TIQS, which no run applied then. The second opening step
continues from this stop (None arrays, Line stage fields and The forced
opening continued, below).

### None arrays

The stop above is an array property that no record stores. The compiled
helper the alias's open handler calls takes six arrays (references,
aliases, factions, forms, locations, location aliases) from the alias
script's array properties and reads the length of each to decide whether
any restriction applies; the alias's VMAD entry stores two of the
script's properties (an int and a bool) and none of its array
properties, the script declares them with no initial value, and no
instruction of any script stores into them. Every one of the 13 script
entries of that script leaves its three array properties so.

`array_census MANIFEST OUT.txt NAMES.txt` (an example of `fo4-vm`;
[array-census.txt](array-census.txt), counts only; 9 s wall, peak RSS
about 1.1 GB) reads every compiled script and every script entry of every
winning VMAD (own scripts, alias scripts, the TERM, QUST, INFO, PACK,
SCEN and PERK fragment blocks), and checks the binder by binding every
entry that stores an array value in a fresh interpreter and reading the
variable back:

- **Not a reader or binder gap.** The records store 2,440 array values
  for declared array properties (kinds 11 to 14 and 17, none empty); the
  binder carries all 2,440 as arrays of the stored length (13,028
  elements, 46 of them undetermined object values naming no winning
  record, 31 none). One stored value is kind 0 (none). No array value is
  stored for a property not declared as an array, and every VMAD reads.
- **The none is in the data.** No compiled array variable has an initial
  value other than none (733 array variables). Of the declared array
  properties of the script entries, 9,845 are stored by no record:
  3,489 in alias scripts and 6,356 in own scripts, 8,597 of them behind a
  variable no instruction of any script stores into, so they stay none
  until a script sets them (247 distinct properties, 213 of them never
  stored into by an instruction).
- **How the opcodes meet it.** Of the 4,161 array opcode sites, 1,512 read
  a script variable behind a property that no instruction stores into,
  456 a parameter (as in the helper above), 1,095 a local, 880 a
  variable an instruction stores into, 5 a variable behind no property no
  instruction stores into, 213 a temporary.
- **In the versioned censuses** (the four `quest-run-*.txt`, the three
  sections of [scene-clock-census.txt](scene-clock-census.txt), the
  forced opening and the nine sections of [vm-census.txt](vm-census.txt)),
  run outcomes that end on an array opcode meeting none: the length of
  none (blocked, undetermined) 933, opcode 32 (failed) 146, 34: 451, 40:
  5, 42: 423, 46: 111; 33, 43 and 45 meet none nowhere.

What an opcode does on none is engine behaviour that nothing read
settles. Four readings, each a pending rule behind its own switch, off by
default (with all four off every run is as before):

| Switch | Reading | Rule |
| --- | --- | --- |
| `--none-array-length` | opcode 31 on none stores 0 | `pex_array_none_length_v1` |
| `--none-array-read` | opcode 32 on none stores the element type's default and the run goes on | `pex_array_none_read_v1` |
| `--none-array-find` | opcodes 34 and 40 on none store -1 | `pex_array_none_find_v1` |
| `--none-array-change` | opcodes 33, 42, 43, 45, 46 on none change nothing and make no array | `pex_array_none_change_v1` |

`quest_run` takes them in every mode (`fo4_vm::NoneArrays`, a field of
the interpreter). With all four on, every run outcome of the new game and
the three sweeps that ended on an array opcode meeting none goes on: new
game 2 (both complete), stage sweep 139 (121 more outcomes complete, 15
more block on a native's result, 8 more fail on a call on none, and the
story manager sees 8 more events it cannot decide), fragment sweep 5 and
scene sweep 2 (all complete). These runs are in the step's notes, not
versioned.

### Line stage fields

`--line-stage-fields` (with `--scene-clock`; off by default,
`scene_line_stage_fields_v1`) applies the INFO TIQS of the line a dialogue
action plays when the scene clock times the action: kind 0, and kind 3
under `--assume-player-choice` (the player's line, then the answer from
the second the player's line ends). The line is known only when exactly
one line of the action's topic has conditions that hold and every other
line's are false; several lines that hold, or a line whose conditions are
undetermined, leave it undetermined, unless the caller passes
`--assume-first-line` (the first of them in stored order plays; counted as
an assumption). The first half is set when the line begins (at once at
the action's start), the second when it ends; a half due later is
scheduled on the clock (`scene::LineEvent`), the clock stops at it, and a
`Step::LineFields` job sets it ahead of the scene's completion step. A
line whose time is not settled sets only its first half. Each decision is
logged (`scene::Change::LineField`) and counted (`scenes / scene clock:
line stage fields`).

A switch apart from `--stage-fields`, not folded into it, because
`--stage-fields` already produces a versioned output
([quest-run-next-blockers.txt](quest-run-next-blockers.txt)) that must
reproduce, and because the line reading rests on a second rule (which
line plays) that the scene and phase fields do not need.

### The forced opening continued

(Each later opening step replaced [quest-run-opening.txt](quest-run-opening.txt)
with its own run; the earlier versions are in the history.) The second
opening step's file held the stops in order by kind, where the run ends, how far it gets, and
the run's needs by kind (since the world step the file holds the run with
a track, Player position below, its section 2 comparing the trigger stops
with this run's); the concrete records and the command line are in the
step's notes.
Continuing from stop 10 above with the switches of this step and the
caller assumptions that exist:

10. The open handler's stop: `--none-array-length`. The stage is set; the
    next scene starts and its first line's TIQS sets the next stage
    (`--line-stage-fields`).
11. Player dialogue in that scene: `--assume-player-choice 0`. The scene
    runs to its end; a line's TIQS end half sets a stage, whose fragment
    starts a scene whose end sets the next; a third scene's phase field
    sets one more.
12. The next stage's setter is the activation handler of an alias's
    reference: `--assume-activate`. Its fragment and the scenes it starts
    set seven more stages (one by a line's TIQS).
13. Six stages in turn, each set by a placed trigger's trigger-enter
    handler: `--assume-trigger-enter` six times; between them stage
    fragments, scene fields and one line's TIQS set seven more, and the
    quest's first objective is displayed.
14. A helper quest's trigger: `--assume-trigger-enter`; its stage fragment
    sets the opening quest's stage, which hides the objective.

There the run ends: no caller assumption passes. The last stage's fragment
blocks on a line-of-sight native; the next two stages are set only by the
TIQS of two lines no scene action names (topics of 4 and of 106 lines:
dialogue outside scenes), and the second of them is the stage whose
fragment registers the distance events; the stage after them is set only
by the quest's distance event, which the interpreter does not raise (and
whose registration the run never reaches); the two after that have no
setter in the data; a side stage needs a trigger only one alias's actor
may enter, the alias forced to the reference of a unique-actor alias,
which no reading resolves.

Reached: **42 of the opening quest's 104 stages** (one, as before, set by
the caller; four set by lines' TIQS), **one objective** displayed (and
hidden again by the last stage). The caller assumed 8 trigger entries, 2
activations, 1 opening, 1 player choice, 2 condition values (12
conditions), line seconds for 45 lines, 49 package actions complete, 2
start-scene actions complete and one stage. By kind, the run needs what
the interpreter has no model for at: player movement into triggers (8
stops) and player position and view (2 at the end), player interaction
(3: two activations, one door), UI menus (the dialogue choice in 6 player
dialogue actions; the stage with no setter, a candidate menu closing from
prior knowledge), actor movement (49 package actions; the side trigger),
dialogue outside scenes (2 lines at the end); inventory, combat and cell
loads stop nothing. Over the natives the opening quest's family calls,
those with no world model: position and movement 35, UI menus and player
controls 19 (the character creation and attribute menus among them),
inventory 5, combat 0, cells and loading 0, animation 38, sound, music,
video and imagespace 21.

## Player position

`fo4_vm::track` is a first world model of where the player is, for the
quest host. It replaces the hand-written "the player entered trigger X"
assumptions of the forced opening by one caller input, a track, and
derives the events from the plugins' own geometry. Everything it does is
off unless `quest_run new-game` is given `--track FILE` (with
`--scene-clock`); with no track every versioned output is as before
(checked with `cmp`, below).

**The track** (a caller assumption, never game behaviour) is a text file
outside the repository: lines `SECONDS SPACE X Y Z [FACING]`, the space a
worldspace or an interior cell as `plugin.esm:0001ABCD` (an exterior cell
names its worldspace), seconds of the real-time clock. Between two points
of one space the player moves in a straight line at constant speed; a
point in another space is reached at its second; before the first point
the player's position is open; after the last the player stays. Every
point is counted as an assumption. `track_plan MANIFEST SPACE PLAN
OUT_TRACK SPEED [LIFT]` (an example of `fo4-vm`) writes such a track from
waypoints (`SECONDS X Y Z`): it walks between them along the space's
navigation meshes (`fo4_nav::NavGraph`, edge links of kind 0, the funnel),
leaving each waypoint so as to reach the next at its second at SPEED units
per second, and lifts every point LIFT units above the mesh plane under
it; the plan, the speed and the lift are the caller's.

**What the model reads.** The `XPRM` of a placed reference is 32 bytes on
every one of the 30,028 placed references that carry one: three bound
words, three colour words in 0 to 1, a float in 0 to 1 and a kind word
1 to 5 ([world-census.txt](world-census.txt), section 1; a layout measured
here, kept in `fo4_vm::track::Primitive` without changing the plugin
layout table). Volumes are the placed **activators** with a primitive in
the track's interior cells and, for a worldspace, in its cells within one
grid square of the track's points and its persistent cells; the
reference's placement transform is `fo4_scene::transform::Transform::euler`
of its `DATA`, its scale `XSCL`.

| Switch | Reading | Rule |
| --- | --- | --- |
| `--trigger-box` | kind 1 is a box of half extents the bound words times the scale | `world_trigger_box_v1` |
| `--trigger-sphere` | kind 2 is a sphere of radius the first bound word times the scale | `world_trigger_sphere_v1` |
| `--trigger-point` | a trigger holds the player while the player's reference point is in its closed volume | `world_trigger_point_v1` |
| `--trigger-body HEIGHT` | the alternative: while a vertical segment HEIGHT units up from the reference point meets the volume (the height a caller assumption) | `world_trigger_body_v1` |
| `--trigger-instant` | enter and leave are raised at the second the hold changes, after timers and scene clock steps due then, before a caller's action due then | `world_trigger_instant_v1` |
| `--trigger-enabled` | a disabled (or deleted) trigger holds no one; enabling or disabling it with the player inside raises its event | `world_trigger_enabled_v1` |
| `--trigger-order` | several events at one second: leaves, then enters, each by reference key | `world_trigger_order_v1` |
| `--distance-events` | distance registrations raise their event on the registering instance at the first second the condition holds (at or below, at or above the threshold, between the two reference points) | `world_distance_events_v1` |
| `--distance-once` | a registration ends with its event | `world_distance_once_v1` |
| `--placed-positions` | a placed reference other than an actor stands at its `DATA` position until a native of the run moves it | `world_placed_position_v1` |
| `--position-natives` | `GetPositionX/Y/Z`, `GetDistance`, `GetAngleZ` from the positions | `world_position_natives_v1` |
| `--cell-natives` | `GetParentCell` (the exterior cell by the 4,096-unit grid), `GetWorldSpace`, `Cell.IsInterior` | `world_cell_natives_v1` |
| `--location-native` | `GetCurrentLocation` is the parent cell's `XLCN` | `world_location_native_v1` |
| `--assume-los true\|false` | caller assumption: every `HasDirectLOS` answers this value; no geometry test is made | none |

A trigger event is raised only when every rule it needs is on (its
shape, the point or body reading, the instant reading, the order reading
when several fall on one second); otherwise the change is counted with
the switch that was off and nothing is raised. The hold of a volume is
computed exactly along the track's straight moves (slab clipping for a
box, the sphere's quadratic, the box swept by the body segment clipped by
its face planes, the sphere against the body by a convex search); a move
that only touches a volume raises nothing. The event is delivered as
`--assume-trigger-enter` delivers it (`fo4_vm::player`), with the player
as the action reference. Whether a trigger is enabled is the host's
`IsDisabled` state (`Host::reference_disabled`, which counts no native
call); a reference a native deletes is gone. A distance registration made
by a native call is kept with the calling instance, the two objects (an
alias stands for the reference it holds) and the threshold; its due second
is solved on the track's moves against a placed reference standing still;
an actor's position is open (no actor movement model). A native that moves
a reference (`MoveTo`, `SetPosition`, `MoveToNode`, `TranslateTo` and two
more) makes its position open; a `MoveTo` of the player is compared with
the track and counted (the track is the caller's word). `quest_run`'s
clock loop asks the model for its next due second beside the timers and
the scene clock, and settles it after each step (`track::next_due`,
`track::settle`); every count goes to the census under `world model`.

**The data's own checks** ([world-census.txt](world-census.txt)):

- Kind words by base: 1 on 27,204 references (6,929 activators, the rest
  statics, decals, acoustic spaces and a few others), 2 on 220 (202
  activators) with three equal bound words on all 220, 3 on 1,940 statics,
  4 on 642 sound markers, 5 on 22 statics. The model reads kinds 1 and 2
  of activators only.
- Of 4,106 upright unscaled boxes of activators with a trigger handler,
  the bottom lies within 1 unit of the highest navigation mesh plane at or
  below the centre for 345 under the half-extent reading and for 10 under
  the full-extent reading; 1,576 reach into the ground under the
  half-extent reading (1,869 have no plane at or below the centre in
  their cell).
- The exterior grid: all 1,026,944 temporary placed references of
  exterior cells lie in their own cell's square of 4,096 units.
- Placed activators with a primitive: 4,395 boxes and 192 spheres with a
  trigger handler (enter, leave or both), 2,384 with no script; 181 are
  initially disabled, 500 have an enable parent, 471 are tilted.
- Scripts: 289 declare a trigger-enter handler with code and 84 a leave
  one; 25 a less-than and 12 a greater-than distance handler; call sites:
  42 less-than and 15 greater-than registrations (13 thresholds between
  100 and 500, 31 a variable), 19 unregistrations; position natives by
  name: `GetDistance` 201, `GetPositionX/Y/Z` 150, `GetAngleZ` 42,
  `GetParentCell` 69, `GetWorldSpace` 12, `GetCurrentLocation` 172,
  `HasDirectLOS` 26 (a caller assumption), `HasDetectionLOS` 79 and the
  line-of-sight registrations 12 (not modelled), `MoveTo` 1,425 (observed).

`world_census MANIFEST OUT.txt NAMES.txt` writes the census (counts only;
NAMES.txt is never versioned); 25 s wall, peak RSS about 1 GB.

**The forced opening with a track**
([quest-run-opening.txt](quest-run-opening.txt); the track, its plan and
the command lines are in the step's notes). The previous step's command
line, its eight `--assume-trigger-enter` removed, with a track of 63
points planned on the navigation meshes from 13 waypoints at 300 units per
second and every switch above (body reading, height 120):

- **All eight trigger stops are passed by the geometry**, none by an
  assumption (before: eight by assumption). Under the point reading seven
  are: one box's bottom lies 6 units above the mesh walked under it, and
  the run stops there (40 stages).
- The track raises 26 enters on 21 triggers, 13 of which the previous run
  never assumed: helper quests' triggers along the way, one of which sets
  one more stage of the opening quest; 14 triggers delete themselves; the
  track crosses one stage's trigger on its way to the previous stage's,
  so two stages are set in the other order.
- The last stage's fragment blocks on `HasDirectLOS`, passed only by the
  caller's `--assume-los true`. Reached: **43 of the 104 stages** (before
  42), one objective displayed then hidden.
- Next: the two stages set by lines spoken outside scenes, which the
  interpreter does not speak; one of them registers the distance events.
  Set by the caller (`--then`) at a second before the track steps onto
  the elevator, the registrations are made (seven), the player's is raised
  where the track comes within its threshold and sets the next stage (46
  stages); the six others name actors' aliases, whose positions are open.


## Dialogue outside scenes

`fo4_vm::talk` speaks the lines of topics that no scene action names, as
far as the records and the caller's assumptions settle them. Everything
is off unless a switch turns it on; with every switch of this section off
every versioned output is as before (checked with `cmp`, below). The
census tool is `talk_census MANIFEST OUT.txt NAMES.txt [--player-base
EDITOR_ID]` ([talk-census.txt](talk-census.txt), counts and kinds only;
NAMES.txt is never versioned; 4 s wall, peak RSS about 0.4 GB).

**The topic record** (`fo4_plugin::dial`, read additively, the plugin
layout table unchanged): EDID, FULL, PNAM, BNAM, QNAM, KNAM, DATA, SNAM
and TIFC, every one but EDID four bytes in all 46,373 winning topics; QNAM
names a QUST in all of them. SNAM is a four-letter code (91 codes); the
category byte of DATA (byte 1) differs within 45 codes and the last two
bytes vary within one code, so the subtype is read from SNAM alone and
the rest of DATA is kept as stored.

**The census** ([talk-census.txt](talk-census.txt)):

- Topics by whether a scene action names them (any field of an action
  resolving to the topic): 41,695 named (57,224 lines); 1,672 in a
  dialogue branch (BNAM) and named by none (9,423 lines); 3,006 neither
  (35,786 lines). Every greeting (`GREE`, 476 topics, 5,987 lines), hello
  (`HELO`, 460, 9,965) and idle (`IDLE`, 101, 5,731) topic is of the last
  kind.
- Lines with a stage field (TIQS with a non-negative half): 4,674 of
  named topics, 64 of branch topics, 1,052 of the others: 485 greeting
  lines (49 begin half, 435 end half, 1 both), 495 hello lines, 15 idle
  lines; the topic's quest holds every stage named for all but 24 of the
  1,052.
- Who function 72 names: greeting lines name the player's base as
  speaker (run-on 0) in 382 (170 with another actor base as listener)
  and another actor base in 1,708; hello lines name the player's base as
  speaker in 429, another actor base in 2,028; 3,795 greeting and 7,448
  hello lines name nobody.
- 411 greeting, hello and idle lines carry a script block (VMAD), whose
  fragments no line outside a scene runs.
- Eight game settings name greetings or hellos (six numbers; the names
  file lists their values).

**What makes a line play.** A greeting when the player starts a
conversation with an actor, which only a caller assumes; a hello when the
player comes near an actor; which line, by its conditions over the
running quests' topics of that subtype. Each step is a pending rule
behind its own switch:

| Switch | Reading | Rule |
| --- | --- | --- |
| (with either below) | a line's conditions run on the speaker (run-on 0) and on the one spoken to (run-on 1); a conversation's lines are tried with the actor and with the player speaking; function 72 reads the placed actor's base or the player's base the caller names (`--player-base`) | `dialogue_conversation_parties_v1` |
| (with either below) | the candidates are the lines of the subtype's topics whose owning quest runs; the line is known when exactly one holds and none is undetermined | `dialogue_outside_line_choice_v1` |
| `--dialogue-priority` | the alternative: lines ordered by quest priority, topic priority, stored order; the first that holds plays when every one before it is false | `dialogue_line_priority_v1` |
| `--greeting-on-talk` | a `GREE` line when the player starts a conversation (`--assume-talk REF@SECONDS`, a caller assumption, which also stands for the actor's being there when its enable state is undetermined) | `dialogue_greeting_on_talk_v1` |
| `--hello-lines` | a `HELO` line when the track comes within the distance of the game setting the caller names (`--hello-distance-setting`) of an enabled placed actor at its stored position; one line per entry | `dialogue_hello_proximity_v1` |
| `--line-stage-fields` | the line's TIQS: the first half as it begins, the second as it ends (seconds from its voice file for the speaker's voice type, or `--assume-line-seconds`) | `dialogue_line_stage_fields_v1` |
| `--assume-line PLUGIN:OBJECTID` | caller assumption: where the choice is not settled, that line plays when its own conditions hold | none |

The steps run as synthetic calls of the quest host (script `::talk`), as
the scene steps do; a line's second half is due on the real-time clock
and `quest_run`'s clock loop stops at it beside the world model's
seconds (`talk::next_due`, `talk::settle`). Counts go to the census under
`talk`; what was said and why to the names file.

### Unique actor aliases

The quest reader already held every alias value; `talk_census` section 7
counts the fills of the 16,286 aliases of every winning quest: of the
14,432 reference aliases, linked to another alias 3,469, from a location
alias 2,306, unique actor 2,115, created at run time 1,751, forced
reference 1,402, from the story event 1,214, no fill value 1,155, an alias
of another quest 508, a display name only 271, by conditions only 241
(884 collection and 970 location aliases apart). Of the 2,115 unique actor
aliases, 1,994 name a base with exactly one placed actor in the loaded
plugins, 117 the player's base (no placed actor), 4 a base with none,
none a base with several.

| Switch | Reading | Rule |
| --- | --- | --- |
| `--alias-unique-actors` | a unique actor alias without a forced reference holds the one placed actor of its base; none or several leave it without one; the player's base (`--player-base`) holds the player | `quest_alias_unique_actor_ref_v1` |
| `--player-no-record` | the player's handle is no record's handle: comparing them answers false | `vm_player_no_record_v1` |
| `--enable-child-flag-ignored` | a record below its enable chain's root that carries its own flag 0x800 takes its parent's state | `xesp_child_own_flag_ignored_v1` |

### Actor positions

| Switch | Reading | Rule |
| --- | --- | --- |
| `--actor-positions` (with `--placed-positions`) | a placed actor stands at its stored position until a native may have moved it (open), or the caller's assumption `--assume-actor-at REF SPACE X Y Z@SECONDS` puts it elsewhere from that second; the placed actors of the cells read near the track are loaded for hellos | `world_actor_placed_position_v1` |
| `--actor-triggers` | an assumed move raises trigger leave and enter as the player's crossing does, the actor as the action reference | `world_actor_trigger_v1` |

No AI package, path or animation is modelled: an actor is where it was
placed or where the caller put it. A distance registration between an
actor and the player, or between two objects that stand still, is solved
in the windows between the caller's moves (`track::first_time`); the
points of the moves join the track's in choosing the cells whose volumes
are read.

### The forced opening with dialogue

([quest-run-opening.txt](quest-run-opening.txt); the command lines, the
track and every record named are in the step's notes.) The world step's
command line on its plan with one change, the wait at the fifth trigger
held ten seconds longer so that the player passes the actor of the hello
stop after the scene before it ends (the later waypoints shifted by ten
seconds), plus the switches of this step:

- The greeting stop: the player talks to the actor (`--assume-talk`); the
  records do not settle the line (thousands of greeting lines of running
  quests hold and as many are undetermined; by priority the first is
  undetermined), so `--assume-line` names the line, which holds with the
  player speaking; its begin half sets the stage.
- The hello stop: the track passes the actor within the setting's
  distance; the actor's enable state is settled only with
  `--enable-child-flag-ignored`; `--assume-line` names the line, which
  holds with the actor speaking; its end half, timed by its voice file,
  sets the stage whose fragment registers the seven distance events.
- The player's distance event is raised by the track; the side stage's
  trigger accepts only the actor of an alias forced to a unique actor
  alias's reference: `--alias-unique-actors` resolves it,
  `--player-no-record` decides that the player is not that actor, and an
  assumed move of that actor into the volume (`--actor-triggers`) sets
  the stage; six assumed moves of actors onto the elevator raise the six
  other distance events, and the last sets the next stage.
- Reached: **49 of the 104 stages** (before 43), one objective. The run
  stops next on a stage set only by an animation event handler, an
  engine event the interpreter does not raise; the two stages with no
  setter are off the path.


## Animation events

`fo4_vm::anim`, behind four switches of `quest_run new-game`, all off by
default ([animation-event-census.txt](animation-event-census.txt) for the
census, [havok-behavior.md](havok-behavior.md) for the graph files):

| Switch | Reading | Rule |
| --- | --- | --- |
| `--anim-events` | `RegisterForAnimationEvent(sender, name)` keeps (instance, sender, name) until `UnregisterForAnimationEvent`; an animation event the run raises runs `OnAnimationEvent(sender, name)` on every registered instance, in registration order; the register call keeps its recorded, undetermined result | `vm_anim_event_registration_v1` |
| `--anim-event-case` | names compare ignoring case, in registrations and in a graph's event table; the handler gets the registration's spelling (without it, a handler comparing the name with its own text would be undetermined) | `anim_event_name_case_v1` |
| `--anim-graphs` (with `--scene-clock`) | a placed reference runs the behaviour graph its base model names, from the root machine's start state when the run first plays an animation on it; `PlayAnimation(event)` takes the current state's transition of that event, else a wildcard one, at once; a conditioned transition leaves the graph undetermined; `PlayAnimation` is still recorded as before | `anim_graph_model_transition_v1` |
| `--anim-graph-keys` (with `--anim-graphs`) | a state whose generator plays a Gamebryo sequence schedules the sequence's text keys that name events of the graph at the entry plus their time over the frequency; each is raised as an animation event of the reference and sent to its graph; leaving the state drops the rest | `anim_graph_sequence_keys_v1` |

The caller's alternative is `--assume-anim-event PLUGIN:OBJECTID EVENT[@SECONDS]`
(with `--anim-events`): the reference raises that event at that second,
delivered to the registrations only (not sent to a graph). The world
model's loop takes the keys' due seconds with the track's and the talk's
(`anim::next_due`, `anim::settle`); the names file logs each graph read,
state entered, key raised and handler run.

What the census finds ([animation-event-census.txt](animation-event-census.txt)):
105 compiled scripts register, 111 declare the handler; the sender is most
often the registering reference itself, the player, or an alias's
reference; of 139 distinct registered names, 125 occur in behaviour graph
event tables (4 more only in another case), 74 as clip annotation texts (5
more), 18 as NIF text keys (5 more), 5 as an idle record's ENAM and 1 in a
scene field; 10 occur in none of these. The two the forced opening waits on next occur only as NIF
text keys (of the two activators' sequences) and in their graphs' event
tables, not as clip annotations: no idle or scene action is involved, so
the data tie is `PlayAnimation` in a stage fragment and a script function
the fragment calls, through the activator's graph to its sequence's key.

## Package destinations

`fo4_plugin::pack` reads the package record as far as its data inputs
([package-census.txt](package-census.txt), section 7 for the layout table:
what is read and what is not established). `quest_run new-game
--package-destinations N` (with `--track` and `--actor-positions`) applies
`pack_location_destination_v1`: a scene action of kind 1 whose package
holds exactly one Location input naming a placed reference puts the actor
its alias holds at that reference's stored position N seconds (the
caller's) after the action starts, as an actor move of the world model
(`track::schedule_move`); the move is dropped if its scene stops first; no
path, no speed. 1,035 of the 3,292 scene package actions have such a
destination. Without the switch no package moves anyone.

## The forced opening with animation events and the vault

(The load step first regenerated [quest-run-opening.txt](quest-run-opening.txt)
from this run and its variants, then replaced it with its own run, Load
events below; the command lines, the tracks and every record named are in
the step's notes.) On the talk step's command line:

- The two stages the talk step stopped on, and a third the same handler
  sets, come from the opening quest's animation event handler. With the
  four switches above the stage fragment's `PlayAnimation` on two
  activators runs their graphs: one enters the state playing its only
  moving sequence, whose keys raise two of the awaited events (and three
  others its own control script waits on); the other, left by earlier
  fragments in a state whose transition the new event takes, enters a
  delay state whose last key moves it to the descent state, whose key
  raises the third. All three stages came from the data under the four
  rules; with any one of `--anim-events`, `--anim-graphs` or
  `--anim-graph-keys` off none of them is reached, and without
  `--anim-event-case` the elevator's is not (the script's name differs in
  case from the key's). With the caller's `--assume-anim-event` for the
  two events the talk step stopped on, instead of the graph switches,
  their stages are reached at the seconds the caller gives (the third,
  not assumed, is not).
- The track then follows the elevator down to the automatic load door
  under it (the caller's: riding the platform until the descent
  sequence's end), the door's stored destination in the interior (the
  caller's: the load takes no time), the interior elevator's descent
  until its sequence's finishing key, and a walk along the navigation mesh
  through three set-stage triggers. Reached: **59 of the 104 stages**
  (before 49), two objectives.
- With `--package-destinations 10` and the player held twice on the
  track (before a helper quest's trigger until the stage its trigger
  needs is done, and before the trigger that stops the elevator scene
  until the scene's package actions have taken the five neighbours to the
  elevator), five of the talk step's seven assumed actor moves come from
  the data (the elevator scene's package actions name the elevator's
  centre marker); the spouse's two remain assumed (her package names
  several placed references, and on the way she follows the player).
  The same 59 stages are reached (also with 5 s; with 30 s the elevator
  scene stops before the actors arrive, their moves are dropped and the
  run stops 12 stages earlier, as it does without the switch on that
  track).
- The run stops next on stages set by an alias script's handler of an
  animation event of the actor itself (registered only in the handler of
  the actor's 3D load, an event the interpreter does not raise), and on a
  vault scene phase whose completion condition reads how far one of its
  dialogue actions has run (function 131, which the command line's
  assumption answers with a fixed value); the details are in the step's
  notes.
- With every switch of this step off, the talk step's command line gives
  the talk step's run byte for byte (census, names and next-blocker
  files).

## Load events

`fo4_vm::load`, behind switches of `quest_run new-game` (with `--track`),
all off by default ([load-event-census.txt](load-event-census.txt) for the
census): a placed reference's `OnLoad` runs on its own scripts, the scripts
of the quest aliases holding it and the instances registered for the
remote event, as a caller's `--assume-load PLUGIN:OBJECTID[@SECONDS]`
delivers it (`player::Action::Load`, an assumption, not a player action).

| Switch | Reading | Rule |
| --- | --- | --- |
| `--moveto-target` | `MoveTo(target, 0, 0, 0, ...)` puts the receiver at the target's settled position and space at once (an offset, or an open target, leaves the receiver's position open as before) | `world_moveto_target_v1` |
| `--load-in-cell` | a reference moved into (by a moving native) or enabled in the interior cell the track is in loads at once | `world_load_reference_enters_v1` |
| `--load-on-arrival` | when the track reaches an interior cell (the instant load through a door), its enabled scripted references and the alias-held references standing there load at that second | `world_load_player_arrives_v1` |
| `--load-grid N` | in a worldspace, references within N grid squares of the player's load as they come into range | `world_load_exterior_grid_v1` |

A reference loads once until the player's area leaves it; `OnUnload`,
`OnCellLoad`, `OnCellAttach` and `OnCellDetach` are not raised; a
reference whose enable state is undetermined does not load. The loads of
a second are settled before and after the track's events of that second
(`load::settle` in the world model's loop, `load::next_due` the seconds the
track changes area); the names file logs each load and handler run, the
census counts them under `load`.

What the census finds ([load-event-census.txt](load-event-census.txt)):
402 compiled scripts declare `OnLoad` with code, 186 `OnUnload`, 54
`OnCellLoad`, 57 `OnCellAttach`, 42 `OnCellDetach`; 54 `OnLoad` handlers
sit in the script's auto state (a script waiting for its load, like the
one the opening needs), 25 register for animation events and 33 for remote
events; 2,977 attachments of a script with `OnLoad` sit on placed
references of interior cells and 3,783 of exterior cells, 81 on unique
actor aliases. The handler the opening needs moves its script out of its
waiting state and registers for an animation event of its actor when the
actor's parent cell is the interior the run is in.

## Scene action progress

Condition function 131 sits only in scene phase completion lists (52
conditions, [action-percent-census.txt](action-percent-census.txt)), always
`at least` an integer from 1 to 99, its first parameter the scene itself
and its second the index of one of its dialogue, response or player
dialogue actions, in 48 of 52 an action ending in that phase. Two readings,
each off by default:

| Switch | Reading | Rule |
| --- | --- | --- |
| `--cond-131-clock` (with `--scene-clock`) | the function is how far the named action has run on the scene clock, in percent: 0 not started, 100 completed, the elapsed share of a timed running action's seconds; a running untimed action is undetermined; a scene waiting on false completion conditions with such a condition is offered them again when the share reaches the value | `cond_fn_131_action_percent_v1` |
| `--phase-conditions-end` (with `--scene-clock`) | a phase with a non-empty completion list ends as soon as the list holds, its timed actions still running | `scene_phase_conditions_end_v1` |

`--assume-cond 131=VALUE` remains the caller's alternative (it takes
precedence over the reading). Without `--phase-conditions-end` the clock
still holds a phase until its actions complete, so the reading answers 100
there; with it the phase ends at the share.

## The forced opening with load events

([quest-run-opening.txt](quest-run-opening.txt); the command lines and
every record named are in the step's notes.) The animation step's command
line without `--assume-cond 131=50`, with `--moveto-target`,
`--load-in-cell` and `--cond-131-clock`:

- Function 131 from the clock passes, by data, the phases the fixed value
  passed and those it held below their threshold (two neighbour scenes
  outside and the vault scene); without the fixed value and without the
  reading the run stops at 42 stages, at the first scene's phase.
- The stage fragment run as the track reaches the vault moves three actors
  to markers in the vault: under `--moveto-target` they stand there, under
  `--load-in-cell` their 3D loads at once, and their alias script's
  `OnLoad` finds their parent cell to be the vault (`--cell-natives`) and
  registers for the actor's own animation event.
- That event (the end of an idle) is raised in the data only by a
  furniture's synchronised idle clip, whose last annotation names it,
  reached through the package's sit procedure, the furniture's keyword and
  the character's behaviour graph: a chain the run does not model. It
  stays the caller's (`--assume-anim-event`), at the second each actor's
  sit phase begins plus the clip's length.
- Reached: **65 of the 104 stages** (before 59); without the three
  assumed events, 60.
- The run stops next on a scene phase waiting on the player's distance to
  an alias's actor (function 1, an alias parameter; the alias fills from a
  location reference type whose two rows in the location's special
  references name the same actor, which the list reading counts as
  several: the load step's notes said the list did not hold the type, a
  misreading the sit step corrected),
  on a stage set when the player activates that actor, on two stages no
  setter in the data sets, and on the stage a later scene's phase sets:
  that scene is started only by a start-scene action of another scene,
  itself started by a stage fragment run when the player sits in a
  cryopod (an `OnSit` event of the player's alias), after the player
  activates the pod (whose fragment enters character creation).
- `--load-on-arrival` (the vault's scripted and alias-held references
  loading as the track arrives), `--load-grid 2` (several hundred loads
  outside) and `--phase-conditions-end` each leave the 65 stages as they
  are; the stages reached with each earlier assumption removed are in the
  record's section 11.
- With every switch of this step off, the command line gives the
  animation step's run byte for byte (census, names and next-blocker
  files).

## Alias fills from repeated and placed rows

The census tool is `alias_fill_census`
([alias-fill-census.txt](alias-fill-census.txt), counts and kinds only).
Of the 14,432 reference aliases of the winning quests, the readings that
exist fill 4,286 (a forced reference 1,402; a unique actor's one placed
actor 1,994 or the player 117; a location's one special reference row
773); the rest stay empty by kind: linked to another alias 3,469, created
at run time 1,751, from the story event 1,214, no fill value 1,155, an
alias of another quest 508, from a location alias whose location is not
forced 1,024, a display name only 271, by conditions only 241, a unique
actor with no placed actor 4, and from a forced location by a ref type
with no row of the type 279, with rows naming several distinct references
184, or with **several rows that all name one reference 46** (the opening's
suit giver among them: its two rows are byte-identical).

| Switch | Reading | Rule |
| --- | --- | --- |
| `--location-refs-distinct` (with `--alias-location-refs`) | rows of a location's special reference list naming the same reference count once | `quest_alias_location_ref_distinct_v1` |
| `--alias-location-placed-refs` (with `--alias-location-refs`) | where the list holds no row of the alias's ref type, the one placed reference of a cell whose `XLCN` names the location and whose own `XLRT` carries the type (`quest::LocationPlaced`, read once per run) | `quest_alias_location_ref_placed_v1` |

The second reading is tested against the list itself: of the 790 aliases
the list resolves to one row, the placed references of the location's
cells carrying the type name the same reference in 569 and never another
(221 none); the reference's own `XLCN` as the tie agrees far less. It
gives one reference to 210 of the 419 aliases (collection aliases
included) whose list holds no row of their type. Each alias reading after
the list's is used through `Quests::fill_location`; with both switches off
it is `QuestInfo::fill_of`.

## Distance conditions

`--cond-1-distance` (with `--track`) evaluates condition function 1
(`cond_fn_1_track_distance_v1`): the subject by the run-on word (2 the
reference word, 5 an alias of the owning quest), the other party by
parameter 1 (a placed reference, or with the aliases flag an alias of the
owning quest, the player included when a unique actor alias of the
player's base holds it), each where `track::spot` puts it at the run's
second; different spaces or an open position leave it undetermined. A
scene whose completion list is false and holds such a condition is
offered it again a microsecond after the second the distance on the track
crosses the value (`track::distance_first_time`, the marks of function
131). `--cond-player-word` (`cond_player_reference_word_v1`) reads a
reference word naming the first plugin's id 0x14, which no plugin
defines, as the player: the conditions census counts 7,725 run-on-2
reference words naming no record below 0x800.

## Activation of actors

`--assume-activate` on a placed actor (ACHR) needs `--activate-actors`
(`vm_event_activate_actor_v1`): the event goes where an activation of any
reference goes (the actor's scripts, the aliases holding it, the
registered instances), the player the activator; no conversation starts.

## Furniture use

`fo4_vm::sit`, behind `--sit-events` (`vm_event_sit_v1`), off by default:
`OnSit(furniture)` and `OnGetUp(furniture)` go to the sitter's receivers
(for the player, the scripts of the aliases holding the player and the
player's registered instances; for a placed actor, its own scripts too),
`OnExitFurniture(sitter)` to the furniture's own scripts, the aliases
holding it and its registered instances. A sit comes from the caller
(`--assume-sit REF[@SECONDS]`, `--assume-get-up REF[@SECONDS]`, a REFR of
a FURN, the player the sitter) or, with `--activate-furniture-sits`
(`vm_furniture_activate_sits_v1`), from a script's
`Activate(actor, true)` on a furniture reference, delivered after the
calling job; a sitter already in another furniture leaves it without an
event. `--furniture-in-use` (`vm_furniture_in_use_v1`) answers
`IsFurnitureInUse` from the run's sitters. `GetSitState` stays recorded
(what its values mean is not in the data). The exit step adds the
activation with events, the activation by the sitter and the furniture's
graph events (Furniture activation and the furniture's graph, below).

What the census finds ([sit-menu-census.txt](sit-menu-census.txt)): the
compiled scripts declare three furniture events; 18 `OnSit` handlers with
code, all taking the furniture, extend ReferenceAlias (15) or
ActiveMagicEffect (3), none a furniture's own script; 19 `OnGetUp`; 6
`OnExitFurniture`, attached to furniture and activator bases and to
aliases of furniture references. The handlers mostly test or set stages
(`GetStageDone`, `SetStage`, `GetOwningQuest`, `GetRef`). 38 `Activate`
call sites pass a literal true for default processing only, 352 a literal
false.

## Menus

`fo4_vm::menu`, behind `--menu-events` (`vm_menu_events_v1`), off by
default: `RegisterForMenuOpenCloseEvent(name)` and
`RegisterForLooksMenuEvent` keep their registrations (and the unregister
natives remove them); the caller's `--assume-menu NAME open|close[@SECONDS]`
runs `OnMenuOpenCloseEvent(name, opening)` on the instances registered for
that name, and `--assume-looks-menu-event N[@SECONDS]` runs
`OnLooksMenuEvent(N)`. No menu name is stored in the repository: the
caller passes it. Nothing a menu does to the player record is performed.
The census counts 25 menu natives and 3 menu events, 11 open-and-close
registrations (10 with a literal name, 6 distinct names) and 9 handlers;
the opening's family registers only for the appearance menu's event (its
handler plays comments) and enters character creation by
`SetInCharGen` 4 times (the census's section 6 reads the scripts the
quests attach, not their scenes' fragment scripts; in the run two scene
fragments call `ShowRaceMenu` and `ShowSpecialMenu` once each, recorded
as effects). No stage of the opening waits on a menu event: the early
stage no setter sets, which the caller gives with `--then`, has no
registered menu handler that could set it either. The caller can also answer
`Utility.IsInMenuMode` (`--assume-menu-mode`) and the button of
`Message.Show` (`--assume-message-button`), with or without
`--menu-events` (exit step).

## The player's base object

`--player-base-object` (with `--player-base`, `vm_player_base_object_v1`)
answers `GetBaseObject`, `GetActorBase` and `GetLeveledActorBase` on the
player's handle with the NPC_ the caller names; `ActorBase.GetSex` on it
answers `--assume-player-sex N` (the caller's: character creation chooses
it), else stays undetermined.

## The forced opening with furniture use

([quest-run-opening.txt](quest-run-opening.txt); the command lines, the
tracks and every record named are in the step's notes.) On the load step's
command line, with its track walking on (the caller's) to the suit giver
in the vault, along the corridor to the cryopods and to the player's pod:

- The vault scene's phase waiting on the distance to the suit giver
  passes from the data: the alias holds the actor of its two identical
  rows (`--location-refs-distinct`), the condition reads the player's word
  (`--cond-player-word`) and the distance from the track and the actor's
  stored position (`--cond-1-distance`), and the scene is offered again
  as the track crosses 350 units.
- The stages after it come from three assumed activations (the suit
  giver and the spouse, both actors under `--activate-actors`, and the
  player's cryopod), two trigger crossings of the walk, and stage, phase
  and line fields. The cryopod's stage fragment enters character creation
  and activates the pod for the player with default processing only;
  under `--sit-events` and `--activate-furniture-sits` the player sits,
  the player alias's `OnSit` handler sets the stage whose fragment starts
  the scene whose start-scene action starts the scene the first vault
  scene waited on.
- Reached: **82 of the 104 stages** (before 65). The run stops next where
  the last stage's fragment calls into the next sequence quest's script,
  which blocks on whether the spouse actor carries a keyword (keywords
  change at run time; the run keeps no keyword state); the two stages
  after it are set only in that script's functions no event or fragment
  of the script reaches, and two stages of the opening quest have no
  setter at all.
- With one thing changed (the record's section 11): without
  `--location-refs-distinct`, or without the suit giver's activation, 75
  stages; without `--activate-furniture-sits` or `--sit-events`, 75;
  without the cryopod's activation, 73; without the spouse's, 81; the
  caller's `--assume-sit` on the cryopod at the activation's second
  instead of the scripted sit, 82. Without `--cond-1-distance` (or the
  player word) the run still reaches 82: the caller's activation of the
  suit giver sets the stage the scene waits for whatever the scene's
  phase does, so the distance reading only lets the scene's phase pass
  from the data before it.
- With every switch of this step off, the load step's command line gives
  the load step's run byte for byte (census, names and next-blocker
  files).

### The two stages with no setter

Read statically (the stage-setter table, the stage records, every
condition, every compiled call): each has one log entry with no
conditions, an editor note and no log text, no flag, and one of them an
empty stage fragment. No set-stage call, record field or startup flag
names the opening quest and either stage as constants; no call on the
opening quest leaves its stage open; 5 calls passing one of the two
numbers leave their quest open, all in other quests' families; 53 calls
leave quest and stage open (base object, alias and unattached scripts);
no condition of any record tests either stage. Nothing in the data read
sets or reads them: what else could (the console, code outside the
compiled scripts) is not in the data.

## Keywords

`fo4_vm::keyword`, two readings each behind its own switch, both off by
default (the natives are then recorded and undetermined, as before):

| Switch | Reading | Rule |
| --- | --- | --- |
| `--keywords-records` | `HasKeyword` on a placed reference (and `Form.HasKeyword` with a placed reference as receiver) answers from its base's `KWDA`, an actor base's with its race's (`RNAM`); an absent keyword answers false only where the records read cover every source: an actor base with a template (`TPLT`, its flags not read), a leveled actor base, and weapon and armour bases (object modifications not read) leave it open. The player's keywords are its base's where the caller names it (`--player-base-object`). | `vm_keyword_records_v1` |
| `--keywords-runtime` | `AddKeyword` and `RemoveKeyword` change the receiver's keywords, the last change of a keyword winning over the records; a change with an undetermined keyword leaves the receiver's keywords open. The calls still answer none. | `vm_keyword_runtime_v1` |

The changes are kept whenever either switch is on, so that the records
reading alone leaves open a keyword the run changed. `ResetKeyword`,
`HasKeywordInFormList`, `WornHasKeyword`, the keyword data natives and the
condition functions on keywords are not read.

What the census finds ([keyword-census.txt](keyword-census.txt),
`keyword_census`; names read from the scripts): 13 natives whose name
holds keyword; 274 `HasKeyword` call sites over every script, 125
`AddKeyword`, 82 `RemoveKeyword`, 4 `ResetKeyword`, 53 `SetKeywordData`, 7
`GetKeywordData`, every keyword argument a variable or property. In the
records, 11,216 winning records of 24 types carry `KWDA` (actor bases
2,936, misc items 2,643, terminals 1,033, furniture 773, armour 692,
activators 678, …), every `KSIZ` agrees with its `KWDA` and every entry
names a winning `KYWD`; no placed record carries `KWDA` or `KSIZ` itself.
3,402 actor bases have a template (2,398 of them also their own `KWDA`).
Of the placed records, the reading settles 1,780,981 and leaves 11,309
open where a keyword is absent (9,715 placed actors whose base has a
template, 1,594 weapon and armour references). The opening's family
calls `HasKeyword` once, `AddKeyword` twice and `RemoveKeyword` five
times, all from quest scripts.

## Collection aliases from a location's rows

`--collection-location-refs` (`quest_collection_alias_location_refs_v1`,
off by default): a collection alias (`ALCS`) filled from a forced location
by a ref type holds, while its quest runs, the distinct references of the
location's special reference rows of that type, in first-row order (none
while the quest does not run); `RefCollectionAlias.GetCount`, `GetAt` and
`Find` answer from it, `AddRef`, `RemoveRef` and `RemoveAll` change it.
Collections filled any other way stay undetermined (counted). The
contents a run read are kept with the quest's state, as alias fills are.

## Scene actions whose start phase was skipped

With the scene clock, an action holds every phase it spans until it
completes, and starts with its start phase. An action whose start phase
the scene skips (start conditions false) therefore never started and held
every later phase it spans. `--scene-actions-skipped-start`
(`scene_action_skipped_start_v1`, off by default) starts it with the first
later phase it spans that the scene enters. Other readings (it never runs
and holds nothing) are not implemented; under
`--assume-packages-complete` they give the same phases for the opening.

## Animation event registrations answered by the caller

`RegisterForAnimationEvent` returns whether the registration was made; the
run keeps registrations (`--anim-events`) but leaves the returned value
undetermined, since whether the sender's graph knows the event (a
character's graph is not followed) is not read. `--assume-anim-registration
true|false` is a caller assumption: every registration the run makes
answers that value (a registration whose instance, sender or name is
undetermined still answers undetermined).

## Caller actions held until a stage

Caller actions (`--then …@S`, `--assume-activate`, `--assume-open`,
`--assume-talk`, `--assume-anim-event`, `--assume-sit`, the menu events)
happen at a fixed second of the real-time clock whatever path the run
took. In a run where an earlier step did not happen, a later action still
fires at its second and can set stages out of the story's order: in the
sit step's one-thing-changed table, the rows without the door's opening
(35) and without the second activation (39) counted stages set that way.
`--action-after TARGET@SECONDS EDITOR_ID:STAGE` (repeated; a caller
assumption, not a rule) holds every timed caller action on that target at
that second until that stage of that quest is done: it happens at its
second when the stage is done by then, else at the second the stage is
done, and never when the stage is never done (each outcome counted). The
target is the action's reference key, menu name, looks event number or
`EDITOR_ID:STAGE` for `--then`; a gate naming no caller action is counted
and ignored, so a command line keeps its gates when one action is left
out. Without `--action-after` actions fire at their seconds, as before.
The player's track and the caller's actor moves (`--assume-actor-at`)
are not caller actions of this kind and are not held: a trigger they
cross still raises its event at its second.

## The forced opening with keywords

([quest-run-opening.txt](quest-run-opening.txt); the command lines, the
tracks and every record named are in the step's notes.) On the sit step's
command line, with this step's readings and the caller's gates:

- The last stage's fragment calls into the next sequence quest's script,
  which tests whether the spouse actor carries a keyword: neither her base
  nor her race carries it, so `HasKeyword` answers false from the records
  (`--keywords-records`) and the script adds it (kept by
  `--keywords-runtime`). The script then blocks activation of the
  references of a collection alias filled from the location's rows
  (`--collection-location-refs`: the collection's script loops over
  `GetCount` and `GetAt`), enables and moves the sequence's actors,
  registers for 14 of the spouse's animation events (answered true by the
  caller's `--assume-anim-registration`, the result steers the script)
  and starts the sequence's main scene; the fragment then sets the next
  stage. Reached: **83 of the 104 stages** (before 82).
- The main scene skips its second phase while two package actions
  starting there span the next four; with
  `--scene-actions-skipped-start` they start with the third phase and
  complete under `--assume-packages-complete`, and phases 3 to 7 run on
  the scene clock. The 8th phase waits on a quest script variable that
  only the sequence script's handler of the spouse's animation events
  sets; most phases up to the 23rd wait on further values of it. The
  event names occur in the data as annotation texts of a clip and event
  names of a behaviour graph; a character's graph is not followed, so the
  events are not raised. A run in which the caller assumes the 14 events
  (seconds not from the data) runs the handler 14 times but the scene
  still waits: the run offers waiting scenes again when the quest state
  changes, not when a script variable does.
- Every timed caller action is held until the stage the story sets before
  it (`--action-after`); in the final run all eleven happen at their
  seconds, so the final run's stages are those of the same command line
  without the gates.
- With one thing changed (the record's section 11, every row with the
  gates): without `--keywords-records`, `--collection-location-refs` or
  `--assume-anim-registration`, 82 (each stops the sequence script before
  the fragment sets the stage); without `--keywords-runtime` or
  `--scene-actions-skipped-start`, 83 (the one keyword test is on a
  keyword nothing changed before it, and the scene sets no stage of the
  opening quest before its 8th phase). Without the gates, 83 as well; but
  without the gates and without the door's opening 36 stages (the sit
  step's 35 plus this step's), with them 17; without the second
  activation 40 against 22; the first activation, 82 either way. Rows
  where an early step fails fall the same way against the sit step's
  table: without `--location-refs-distinct` or the suit giver's
  activation 65 (sit step 75), without the caller's start stage 7 (82),
  without the assumed line seconds 4 (17), without package actions
  assumed complete 10 (24). The track and the caller's actor moves are not held:
  without the start stage, two stages set by trigger crossings of the
  track and of an assumed actor move still come at their seconds.
- With every switch of this step off, the sit step's command line gives
  the sit step's run byte for byte (census, names and next-blocker
  files).

### The two stages set in the sequence script's functions

Every compiled script was scanned for calls of the sequence script's
functions by name. The function setting the first of the two stages is
called once, by the begin fragment of the main scene's 22nd phase; the
function setting the second twice, by the end fragment of the main
scene's 27th phase and by the end fragment of another scene of the
sequence quest. Neither is called by the sequence script itself or by any
event handler; the function that starts the sequence has four callers
(the opening's stage fragment and three in two other quests' scripts),
and the script's eight other functions are called from fragments of its
scenes and dialogue lines (7) and from another quest's script (1). The
stage-setter table classes the two as reached by no engine event or
fragment of their own script because it does not follow calls from other
scripts' fragments into a quest script's functions. (Since the graph step it does: Calls from other scripts in the
stage-setter table, below.) No new rule is
needed to reach them: the scene fragment readings run phase fragments as
the phases begin and end. What stops the run short of them is the scene's
8th phase above. The stage after them is set by the second one's stage
fragment.

## Scenes offered again when a script variable is written

Until this step a scene stopped on its phase's conditions was offered that
phase again only when the run's quest state changed (`--scene-recheck`,
`scene_condition_recheck_v1`). A condition that reads a quest script
variable (function 629, `cond_fn_629_quest_variable_v1`) can change
without any quest state change: a script writes the variable.

What the census finds ([scene-variable-census.txt](scene-variable-census.txt),
`scene_variable_census`, counts and kinds only; the names go to a file
that is never versioned): of the 4,773 winning scenes' 21,285 phases, 104
scenes hold a condition of function 629 in a phase list: 535 conditions
(450 in start lists of 403 phases, 85 in completion lists of 73 phases),
471 on the scene's owning quest and 64 on another quest, most compared
with `==` (436) or `>=` (58). Every one names a variable that the quest's
attached scripts declare (int 403, bool 132; 488 behind a property). They
read 121 distinct (quest, variable) pairs. By the instructions that write
those variables: 476 conditions read a variable only functions, stage
fragments, accessors or other scripts' property writes change; 41 one that
both event handlers and functions change; 12 one that only event handlers
change; 6 one nothing writes. 32 conditions read a variable that an
`OnAnimationEvent` handler writes (the opening's 14 among them), 8 one
`OnQuestInit` writes, one each `OnTimer` and `OnTimerGameTime`. The other
function carrying a CIS2 text (660, 15 conditions in the phase lists of 8
scenes) is not evaluated.

| Switch | Reading | Rule |
| --- | --- | --- |
| `--scene-recheck-variables` | every write of an instance variable is kept (the object its instance is attached to, the variable's name); after each caller action, timer firing, scene clock advance and world event that ran scripts, a scene stopped on its start or completion conditions is offered that phase again when one of those conditions reads (function 629) a variable written since the last offer, on the quest its first parameter names; with `--scene-recheck` also on, not in a pass where the quest state reading has just offered every stopped scene | `scene_condition_recheck_variable_v1` |

Alternatives considered: offering every waiting scene again at each tick of
the clock (polling; with the readings that exist it gives the same phases,
since a condition's value changes only where a script, a quest state or a
clock mark changes it, at the cost of an offer per scene per tick and a
second of delay that depends on the tick); offering every waiting scene
again after any script returns (the same phases as the reading chosen,
with offers that cannot change anything); offering it after any variable
write whatever the conditions read (the same phases, more offers). The
reading chosen is the narrowest that gives those phases. Its basis is
weak: when the engine tests a waiting phase's conditions is not in the
data, and the only support is prior general knowledge that a playing
scene keeps testing them; the trigger is the project's reading.

`fo4_vm::scene::Scenes::variable_writes` (kept only while a caller sets
it) and `fo4_vm::scene::recheck_variables`; the interpreter records a
write from a variable store and from a property set backed by a variable.

## Character behaviour graphs

The 14 events the opening's sequence script registers on the spouse
actor are raised in the game by her character's animation. What the data
gives for an actor, as far as the repository's readers go
([character-graph-census.txt](character-graph-census.txt),
`char_graph_census` of `fo4-unpack`, with the actor and the 14 event
names given on the command line; the names go to a file that is never
versioned):

- **The chain from the actor.** The placed actor's base (an NPC_ without a
  template) names its race (RNAM); the race's MODL texts after its NAM3
  marker name a behaviour project file (106 of the 110 winning races carry
  two such texts, 74 the same file twice, 32 two files; 46 distinct
  project files, 45 of which resolve to a character and a behaviour graph
  with `fo4_havok`'s readers). The spouse's race names one project; its
  character's root graph has 4 states (3 modifier generators and a nested
  state machine) and an event table of 1,058 names that holds all 14.
- **Where the events occur.** 70 behaviour graph files hold the 14 in
  their event tables. Exactly one clip's annotations name them, each once,
  in the order of the script's numbering, all within the clip's 61.4 s
  (the first at 7.53 s, the last at 60.87 s).
- **What would tie the actor to that clip.** No behaviour, character or
  project file holds even the clip's last two path parts as a text (a byte
  search of the stored bytes, not a reading of any field; the file name
  alone occurs in 147 files and names nothing), and no subrecord of any
  winning record does. The readers do not read what selects a clip: clip
  generator fields, behaviour reference generators, nested machines below
  the root, transition conditions and graph variables, the character's
  animation list, nor how a furniture's or an actor's keywords select a
  graph state. In the run the spouse is placed in a multi-actor furniture
  by `SnapIntoInteraction` (recorded, not performed) after the script
  changes her keywords; the furniture base carries two keywords and marker
  entries; no scene action names an idle or an animation, no package of
  the scene names the clip, and no `PlayIdle`-like native is called on her.

**No rule is added**: which clip plays, and when it starts, cannot be
determined with the readers that exist. The events stay the caller's
(`--assume-anim-event`). The final run takes the caller's seconds from
the clip itself: the second of the `SnapIntoInteraction` call plus each
annotation's time (the start second and the choice of the clip are the
caller's assumption; the offsets are measured). What would make a rule
possible: reading the clip generators and behaviour references of the
character's graphs, the state machines below the root with their
transition conditions, and the furniture's keywords against them.

## Calls from other scripts in the stage-setter table

The table classed a set-stage call in a quest's own script as reached by
nothing when no engine event or fragment slot of that script's own chain
reaches its function. Under `setter_outer_call_entry_v1` it now follows
such a function one call outward: every call of it by name from another
script's function, on a receiver whose declared type is the function's
script or extends it (the quest's attached script being that type or
extending it), the receiver resolved to that quest by the caller's
attachment (`GetOwningQuest()` on a fragment or alias script of that
quest, a property whose stored value names it, or the owning quest of an
alias property of it). The setter then needs what reaches the caller: its
fragment slot (a scene phase, a stage, a dialogue line, a terminal item)
or its engine event. Calls whose receiver the scan does not resolve
(30,076: parameters, other calls' results, arrays), callers attached by no
read VMAD (256) and callers nothing reaches either (13) are counted and
left out.

**The committed census changed without a switch**:
[stage-setter-census.txt](stage-setter-census.txt) is regenerated with the
reading on (`stage_census --no-outer-calls` reproduces the earlier file
byte for byte), because it is a static scan whose earlier output named a
gap rather than a behaviour. What changed: 841 outer call sites followed;
setters 26,361 to 28,922 (one per caller reached); setters needing a
function no event or fragment of its own script reaches 3,074 to 2,368 (in
quest scripts 1,847 to 1,147), most of the rest now needing an alias
script's death or dying event (2,864), a quest stage fragment (203), a
scene's fragment (74) or a dialogue line (25); stages with a setter that
needs nothing reachable 636 to 401; the 2,297 stages with no setter at all
are unchanged. `quest_run --next-blockers` uses the reading only with
`--setters-outer-calls`, so every earlier next-blockers file reproduces.

In the opening, the two stages the keyword step found set only in the
sequence script's functions are reached, in the table, by the main
scene's phase fragments (a begin fragment of one phase, the end fragment
of another) and by another scene's end fragment.

## The forced opening with the spouse's animation events

([quest-run-opening.txt](quest-run-opening.txt); the command lines and
every record named are in the step's notes.) On the keyword step's command
line with `--scene-recheck-variables`, `--setters-outer-calls` and the
caller's 14 events of the spouse:

- The events are the caller's (`--assume-anim-event`): at the second the
  sequence script snaps the spouse into the furniture (730.036 s) plus each
  annotation time of the one clip that names them (Character behaviour
  graphs, above), each held until the stage set at that second
  (`--action-after`). The handler runs 14 times; the main scene's 8th to
  20th phases complete as their timed lines end, each finding the variable
  at its value; the begin fragment of the 22nd phase and the end fragment of
  the 27th call the two sequence functions, which set the next two stages;
  the second's stage fragment sets the next, whose fragment starts the next
  main quest and its exit scene. Reached: **86 of the 104 stages** (before
  83).
- The variable recheck is not relied on at these seconds: every event comes
  while the phase waiting for it still runs its lines, so the phase's own
  completion step finds the value (86 without `--scene-recheck-variables`
  too). With the events at invented later seconds (every 5 s from 765 s,
  after the scene has begun waiting at its 8th phase) the rule offers the
  waiting scene again 12 times and the run reaches 86; without it, 83 (the
  keyword step's stop). Without the 14 events, 83; without
  `--setters-outer-calls`, the same stages (only the next-blocker file of a
  run stopping at the keyword step's stop differs: the two waited stages'
  setters then need the scene's phase fragments, a scene point not
  reached, instead of an event the interpreter does not raise).
- With one thing changed (the record's section 11), every variant that
  reached the keyword step's stop now goes three stages further; with
  `--phase-conditions-end` the run stops at 83: the main scene stays in
  its 21st phase, held on the clock after its line ends (not investigated
  further).
- The run stops next on stage 1000, set only by the next main quest's
  handler of an animation event of the player's cryopod (a NIF text key and
  graph event of its model) that a fragment of the exit scene registers
  (the run raises no such event); on a vault dialogue stage a placed
  trigger or a menu closing sets, which the exit scene's looping successor
  waits on; and on stage 5000, set by fourteen placed triggers. Sixteen
  earlier stages are on no path the run takes, two of them with no setter.
- With every switch of this step off, the keyword step's command line
  gives the keyword step's run byte for byte (census, names and
  next-blocker files).

## Furniture activation and the furniture's graph

`fo4_vm::sit` and `fo4_vm::anim`, three readings, each behind its own
switch, off by default (with them off every earlier output reproduces byte
for byte):

| Switch | Reading | Rule |
| --- | --- | --- |
| `--activate-furniture-events` (with `--sit-events`) | a script's `Activate(actor, false)` (default processing with events) on a furniture reference the run does not have the actor sitting in sits the actor there, as `--activate-furniture-sits` reads the default-processing-only call; the furniture's `OnActivate` is not delivered (counted) | `vm_furniture_activate_events_sits_v1` |
| `--activate-furniture-get-up` (with `--sit-events`) | a script's `Activate(actor, true or false)` on the furniture the run has the actor sitting in gets it up (`OnGetUp`, `OnExitFurniture` as under `vm_event_sit_v1`), delivered after the calling job before that job's sits | `vm_furniture_activate_get_up_v1` |
| `--furniture-graph-events SIT GET_UP` (with `--anim-graphs` and a scripted activation reading) | the sits and get-ups a script's activation brings send the furniture reference's behaviour graph, within the call, the caller's first or second event name; the graph is read when first needed and takes the event as it takes `PlayAnimation`'s, its keys raised under `--anim-graph-keys`; caller-assumed sits send nothing | `anim_furniture_sit_graph_events_v1` |

No event name is stored in the repository: the caller names both. What the
data gives (a local probe over the winning FURN records with
`fo4_scene::anim_graph`, outputs outside the repository): 782 winning FURN,
309 distinct models, 93 of which chain to a behaviour graph (224 records;
557 records have no graph extra data, 1 a project naming no character).
Over the root machines' transitions two pairs of names recur as the on and
off events of use: the pair the forced opening uses is held by 20 graphs,
9 take its first name from their start state and 22 take its second from
at least one state (26 transitions); another pair is taken from the start
state by 12 graphs, with 12 off transitions. So the data shows that
furniture graphs wait on such events, not which pair a given furniture
receives nor that the engine sends one: the caller chooses, and the rule
is weak.

The player's postwar cryopod is the case the forced opening needs. Its
graph (13 events, 14 states, every state a Gamebryo sequence state) has
two families of states, an idle family and a cryo family. Its start state
is an unoccupied idle state; a wildcard transition on an instant-occupied
event (the one a stage fragment plays with `PlayAnimation`) enters the
occupied state of the game start, whose transitions take the get-up name
of the pair above to the game-start sequence (11.2 s, ending on a key that
moves the graph on) and from there to the exit sequence (5.33 s, ending on
the key the exit scene's fragment registers for). The exit sequence's end
key is therefore raised 16.53 s after the get-up event, from the data.

`--assume-menu-mode true|false` and `--assume-message-button N` are caller
assumptions (`fo4_vm::menu::menu_mode`): `Utility.IsInMenuMode` and
`Message.Show` answer the caller's value; without them both stay recorded
and undetermined, as before. `--assume-anim-event player EVENT[@SECONDS]`
lets the caller's animation event come from the player's handle.

## The forced opening to the end of the prewar quest

([quest-run-opening.txt](quest-run-opening.txt), its section 18; the
command lines and every record named are in the step's notes.) On the graph
step's command line with the scene clock's advances raised from 200 to 400
(a caller assumption: the run uses 196 of them either way), the three
readings above, the player's camera event and the menu-mode answer:

- **What plays the pod's sequence.** The pod's exit is not a scene action,
  a package, an idle or a `PlayAnimation`: the exit scene's actions are two
  lines, a 0.1 s timer and a start-scene action. The exit scene's second
  phase begins with a stage of the vault's dialogue quest whose fragment
  activates the pod for the player (default processing with events) while
  the player sits in it: the earlier fragment that moved the player to the
  postwar vault activated the same pod for the player (with events) and
  played the instant-occupied event on it. Why the graph readings did not
  reach the key before: neither activation was read (only
  default-processing-only activations sat anyone), and nothing sent the pod's
  graph the get-up event its occupied state waits on (`PlayAnimation` is the
  only graph input the host had). With the three readings the earlier
  activation sits the player in the pod, the exit scene's activation gets
  the player up and sends the get-up event, the graph runs its two
  sequences, and the exit key is raised from the data at 811.20 s: the
  next main quest's player alias handler (registered by both of the exit
  scene's phase-end fragments) sets that quest's stage 2.
- **What stage 1000 then needs.** Stage 2's fragment registers the next
  main quest for a camera event of the player; that quest's handler of it
  sets stage 1000 of the opening quest after a loop that waits while a menu
  is open. The camera event is a character's: it occurs as the annotation at
  the first instant of two short first-person clips in a folder of the
  pod's exit (`char_graph_census` with the event names: 5 clips annotate it,
  the other 3 paired animations; no behaviour, character or project file names
  the two clips). Which clip the player plays, and when, is not read (as for
  the spouse, Character behaviour graphs above). So the event is the
  caller's (`--assume-anim-event player`, at 812 s, held until stage 2 of
  the next quest is done with `--action-after`), and `IsInMenuMode` the
  caller's false. At the get-up's own second (794.67 s) the event would come
  before the registration and set nothing.
- Reached: **87 of the 104 stages** (before 86). Stage 1000's fragment
  completes the objectives, sets the helper quest's last stage and stops the
  opening quest: the prewar quest ends.
- With one thing changed (record section 11): every variant of the graph
  step's list that reached 86 now reaches 87, and six that missed earlier
  stages but still reached the pod (85, 85, 85, 82, 43 and 39 become 86, 86,
  86, 83, 44 and 40; the three without `--action-after` reach it because
  their caller actions happen at fixed seconds) also gain stage 1000; the
  others stop where they did. Without any
  one of `--activate-furniture-events`, `--activate-furniture-get-up` or
  `--furniture-graph-events`, without the player's event or without the menu
  answer: 86. With the other use event pair as the caller's names: 86 (the
  pod's graph takes neither). With the player's event at the get-up's second,
  not held: 86. With 200 advances: 87. With `--phase-conditions-end`: 87.
- What stops next: nothing of the opening quest is left on the run's way
  (The opening's stages off the run's path, below). The exit scene's looping
  successor waits on stage 100 of the vault's dialogue quest, and stage 5000
  remains a waited stage of a quest the caller set a stage on (both below).

### The exit scene's looping successor

The looping scene's second phase completes when the vault's dialogue quest
has stage 100. Its two setters are one script on one placed trigger: its
enter handler (the player, once) sets stage 98, waits a second and shows a
message box; the third button (index 2) calls the function that sets
stage 100, plays the exit elevator's event and resets the vault's lights; the
first two open the attributes or appearance menus, whose closing (the
registered menu event) shows the box again. So the data requires the player
to walk into that trigger and choose in a message box (a menu choice). No
enable parent gates the trigger; that in the game it lies past the vault's
door, which the next main quest's console sequence opens on the player's own
animation events (its stage fragments register for them), is prior
knowledge, not checked against the geometry. With the caller's trigger entry at 900 s (held until stage
1000) and button 2 (`--assume-message-button 2`), the looping scene ends and
the vault exit scene starts (record section 11, labelled variant): this
skips the next quest's walk through the vault, so it is not in the final
run, and it changes no stage of the opening quest.

### The failure stage

Stage 5000 is set by fourteen placed triggers around the prewar area; its
fragment sets off a blast and kills the player and the spouse. It is the
opening's failure for a player who leaves the play area: a run that plays
the opening is not meant to reach it, and the track does not walk there.

### Phase completion conditions and the clock's advances

The graph step's variant with `--phase-conditions-end` stopped at 83 stages
with the main sequence scene in its 21st phase. It is the caller's advance
cap, not a host bug and not evidence for or against the reading: with
`--scene-clock 200` that run used all 200 advances by 785.59 s (the phase's
3 s assumed line would have ended at 786.69 s), because completion
conditions that end phases at the scene clock's shares add due seconds.
With 400 advances the same variant uses 209 and reaches the graph step's 86
stages; on the exit step's command line it reaches the same stages as the
final run (record section 11). The reading changes when phases end, not
which stages the run reaches.

### The opening's stages off the run's path

[quest-run-opening-unset.txt](quest-run-opening-unset.txt) classes the 17
stages the best run does not set: one debug start (a log entry of the start
stage that the opening's debug global selects), four of the player's other
choices or actions (a conversation before an introduction, the refusal to
sign, looking away from the spouse, an optional farewell line), one whose
reaching depends on an order the data does not settle (a scene stopped by
the stage its line's end half sets, before the phase that sets this one),
nine with no setter in the stage-setter table (earlier records counted two,
the sit step's full static reading of two of them), one whose only setter is
a helper stage nothing sets, and the failure stage. None is a stage the run
should reach and does not, except possibly the ordering one.
## The run timeline

`quest_run new-game ... --timeline FILE` writes the run's timeline (`fo4_vm::timeline`, format and
frames in `spec/timeline.md`). The recorder is a reader: the example keeps it in a thread-local slot,
shows it the interpreter where the clock loop writes its `## timeline at` lines (after each caller
action, track step, timer and scene clock advance) and once more after the loop, and writes the file
before the jobs are counted. It keeps what the effect log, the quest log, the scene log and the world
model's `native_at` and `moved_at` grew by since the last look, stamped with the run's present second;
at the end it adds the caller's track points with their cells, the placed actors' stored positions
(second 0) and the actor moves applied (each at its own second: a package destination's when the
recorder saw it scheduled, else the caller's). A line's speaker is read without calling the quest
host (whose alias fill records which unique-actor aliases a run read): a fill the run set, the alias's
forced or location reference, the unique actor's one placed actor. Line texts and labels come from the
store at the end. Nothing else changes: on the forced opening the census, names, next-blockers and
next-blockers names files are the same byte for byte with and without `--timeline`.

The forced opening's timeline (local file, never committed): 102 track points in 2 spaces (6 exterior
cells and the vault's interior), 139 actor places (76 stored, 42 package destinations, 2 assumed by the
caller, 19 from MoveTo), 60 visibility changes (45 plain, 15 NoWait), 246 stages, 13 objective changes,
190 lines (143 with a resolved speaker, 107 with a text), 3,245 other scene changes.

## Conditions

`fo4_vm::cond` evaluates the condition lists read by
`fo4_plugin::condition` ([conditions.md](conditions.md)) to true, false
or undetermined. A condition is evaluated only when its function index is
one of ten (five since the second conditions step, each behind its own
switch), each a pending rule; every other condition is undetermined,
named by its function index. Three-valued AND and OR: a list is
undetermined only when its determined conditions do not settle it.

| Index | Value | Read from | Rule |
| --- | --- | --- | --- |
| 56 | 1 when the quest of parameter 1 runs, else 0 | the quest state | `cond_fn_56_quest_running_v1` |
| 58 | the current stage of the quest of parameter 1 | the quest state | `cond_fn_58_quest_stage_v1` |
| 59 | 1 when stage (parameter 2) of the quest of parameter 1 is done, else 0 | the quest state | `cond_fn_59_stage_done_v1` |
| 74 | the value of the GLOB of parameter 1 | FLTV, undetermined once the run may have changed it (as `GlobalVariable.GetValue`) | `cond_fn_74_global_value_v1` |
| 576 | selector 0 with member `K1` or `L1` (parameter 1): 1 when the story event's keyword or location is the record of parameter 2 | the story event being offered (`fo4_vm::story`); undetermined outside one | `cond_fn_576_event_data_v1` |
| 72 | 1 when the base (NAME) of the reference the run-on word names is the record of parameter 1 | run-on 2: the reference word; run-on 5: the alias (last word) of the list's quest, as `ReferenceAlias.GetReference` answers it; any other run-on word undetermined | `cond_fn_72_reference_base_v1`, `cond_run_on_subject_v1` |
| 84 | the number of dead actors of the NPC_ of parameter 1 | 0 in the new-game state before any script; undetermined in a run | `cond_fn_84_dead_count_v1` |
| 248 | 1 when the scene of parameter 1 plays | the run's scene state (none plays at a new game; undetermined in the isolated scenario until the run starts or stops it, and with scenes off) | `cond_fn_248_scene_playing_v1` |
| 550 | 1 when action INAM (parameter 2) of the scene of parameter 1 is complete | 0 wherever the scene's state is settled: no scene has played at a new game and a run completes no action | `cond_fn_550_scene_action_complete_v1` |
| 629 | the script variable CIS2 names of the quest of parameter 1 | the run's attached instances; before the quest is attached, the VMAD property value the variable backs, else its stored initial value (`fo4_vm::vars`; needs the compiled scripts: `quest_run` and the censuses pass them, `script_run` does not) | `cond_fn_629_quest_variable_v1` |

- Operator codes 0 to 5 compare the value with the comparison value
  (`cond_compare_ops_v1`); with flag bit 0x04 the comparison value is the
  global's value (`cond_use_global_v1`); with bit 0x02 or 0x08 the
  parameters name aliases or package data and the condition is
  undetermined (`cond_flag_parameters_v1`); a parameter that does not
  name a winning record of the expected type leaves it undetermined.
- Combination (`cond_list_or_groups_v1`): the AND of OR groups, a
  condition with bit 0x01 joined to the next by OR; `Combine::LeftToRight`
  is the alternative the census counts.
- What the functions read (`cond::Facts`): during a run, the quest and
  scene state of the run's world, the data host's globals, the run's
  script instances (`quest::RunFacts`, with `quest::CondInputs`: the
  switches, the compiled scripts, the caller's assumptions); for the
  census, the new-game state before any script runs (`cond::StaticFacts`).
  The owning quest of a list (for run-on 5) is the quest itself for a log
  entry or the conditions after NEXT, a scene's PNAM for its phases, none
  for a story manager node.
- Switches (`cond::Switches`): `--no-cond-fn-N` for each of 72, 84, 248,
  550 and 629 and `--no-cond-run-on` (`cond_census`, `cond_blockers`,
  `quest_run`, `script_run`); with all of them off every census written
  before reproduces byte for byte. `quest_run --assume-cond N=VALUE` is a
  caller assumption (every condition of index N takes that value), counted
  as such in the census.
- Used to choose a stage's log entry (Quests, above) and by the story
  manager (the nodes' conditions and the quest's conditions after NEXT,
  Events and the story manager, above); quest conditions before NEXT,
  objective target and alias conditions, and every other record type's
  conditions are read and counted, not used.

The census of every condition and of what the evaluator settles at the
start of a new game is [conditions-census.txt](conditions-census.txt)
(`cond_census MANIFEST OUT.txt`, 12 s, peak RSS about 510 MB); the
census of what blocks the lists a run evaluates, by context, is
[conditions-blockers.txt](conditions-blockers.txt) (`cond_blockers`,
[conditions.md](conditions.md), Blockers).

**Second conditions step: what the five added evaluators change.** With
every new switch off each census below reproduces the earlier file byte
for byte (checked with `cmp` for `conditions-census.txt`, the four
`quest-run-*.txt`, `story-manager-census.txt`, and for the quest-host
sections of `vm-census.txt` and `vm-blockers.txt` against the same
binary with the switches on). With them on:

- `quest_run new-game` is unchanged (no list it evaluates holds an added
  index whose input the new game settles); [quest-run-new-game.txt](quest-run-new-game.txt)
  is not regenerated.
- `quest_run stages` ([quest-run-stages.txt](quest-run-stages.txt),
  regenerated): log entry condition lists 82 false, 102 true, 351
  undetermined (35, 80, 423 before); selections settled 70 (63), open 219
  (226); no selection is left open by function 629 any more (28 before),
  191 by 59 become 187; 5,555 stage fragments complete (5,553); 4,612
  sweep items complete every run (4,606). Rules relied on now include
  `cond_fn_629_quest_variable_v1` (34 items) and `cond_fn_84_dead_count_v1`
  (4, left undetermined in the run).
- `quest_run scenes` ([quest-run-scenes.txt](quest-run-scenes.txt),
  regenerated): 4,617 phases entered (4,601), 32 passed over because their
  start conditions are false (6), 34 scenes run to their end (32), 9 stop
  on completion conditions that are false (2); no scene stops on function
  629 any more (28 before, alone or with 59 or 555).
- `quest_run fragments` ([quest-run-fragments.txt](quest-run-fragments.txt),
  regenerated): two story events are no longer left open by function 629.
- [vm-blockers.txt](vm-blockers.txt): its stage-sweep section of log entry
  selections changes as above (regenerated from the stage sweep with the
  events step's assembly; with the switches off the same assembly
  reproduces the file).
- `script_run --census` (quest host): handler outcomes are unchanged; the
  bases alone and the placed references gain one line each, the runs
  relying on `cond_fn_629_quest_variable_v1` (44 and 20), inserted into
  [vm-census.txt](vm-census.txt). The other lines of that file were kept
  as the events step wrote them (stale in their drawability split); the
  scene-events step regenerated the whole file (Census, above).
- The forced opening (`quest_run new-game --then <opening quest>:0
  --timers 50 --time-scale-global <time-scale global>`) is unchanged:
  its first scene still stops in phase 0 on two completion conditions of
  function 555 (the scene's alias 2 and the player reference), which the
  interpreter cannot evaluate (no 3D state, `cond_fn_555_3d_loaded_v1`
  proposed only), and the one-second timer still loops. Exploring with
  the caller assumption `--assume-cond 555=1`, phase 0 completes and phase
  1 is entered and waits on an action of kind 0 (a spoken line): the next
  stop is a world event, dialogue audio.

## Limits

- One installation, one game build; no measurement of the running game.
- Each delivery starts from a freshly bound object; event sequences are
  only those named on the command line; scripts attached on need start
  from their VMAD values and auto state, as if nothing had run on them.
- Natives are never performed; latent natives do not wait; the data host
  answers only the natives listed under Data host, from the data as at the
  start of a game.
- Stages are set and their fragments run by `quest_run` (Quests) and by
  `script_run` (quest host, isolated scenario); `bind::run` and the
  viewer keep the data host, so quest natives stay undetermined there.
  Conditions are evaluated for log entry selection, scene phases and
  the story manager, and only ten function indexes. Remote events are delivered
  only for the events a run raises itself (quest events, the events
  `script_run` names) and custom events as they are sent; the story
  manager receives only the script event; tracked-statistics, sleep,
  location, combat and every other world event are never delivered.
  INFO, PACK and PERK fragments run only in the fragment sweep, scenes
  only where a run starts them, phases only where the data decides
  ([scene-record.md](scene-record.md), Scenes and fragments above).
- Caller actions happen at fixed seconds unless the caller holds them
  until a stage (`--action-after`); the track and the actor moves the
  caller assumes (`--assume-actor-at`) always do. Scenes waiting on conditions are
  offered again when the quest state changes and, with
  `--scene-recheck-variables`, when a quest script variable they read is
  written; conditions on anything else are offered again only at the marks
  of functions 131 and 1.
- A character's behaviour graph is not followed: its root machine plays no
  Gamebryo sequence and the clips its states play are not read; actors'
  animation events (the player's among them) stay the caller's. A
  furniture's graph takes use events only from the sits and get-ups a
  script's activation brings, with names the caller gives.
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

## Provenance of the quest step

Recognised from prior general knowledge, used only as hypotheses that the
censuses could refute ([quest.md](quest.md), [quest-census.txt](quest-census.txt),
[quest-run-stages.txt](quest-run-stages.txt)):

- the QUST subrecord tags' roles and field layouts (`quest_qust_layout_v1`);
- that a VMAD stage fragment's log entry is the position of a QSDT under
  its INDX;
- the meanings of DNAM flag bits 0 (start-game enabled) and 3 (repeated
  stages allowed) and of INDX flag bit 1 (startup stage);
- what setting a stage, starting and stopping a quest, objectives and
  forced-reference aliases do, from the native and event names the
  installed Quest, Alias and ReferenceAlias scripts declare (their
  declarations, and the scripted `setStage`, `GetStage` and
  `GetStageDone` that call the natives, were read from the archives with
  `pex_dump`);
- that the first log entry whose conditions hold selects the fragment.

The order of nested fragments was chosen by the data-flow comparison
above, not recalled. One public documentation page of script-facing
behaviour was requested (the Creation Kit wiki page of the SetStage
function); the server refused it (HTTP 403) and nothing of it was read. No
editor, editor source or definition file, script source, decompiler,
other interpreter or the game executable was opened or searched for.

## Provenance of the conditions step

Recognised from prior general knowledge, used only as hypotheses that the
census could refute ([conditions.md](conditions.md),
[conditions-census.txt](conditions-census.txt)):

- the CTDA field layout and the CIS1 and CIS2 texts
  (`cond_ctda_layout_v1`);
- the operator codes, the OR, global, alias and package-data flag bits
  and how a list combines (`cond_compare_ops_v1`, `cond_use_global_v1`,
  `cond_flag_parameters_v1`, `cond_list_or_groups_v1`);
- the meanings of function indexes 56, 58, 59 and 74 (whether a quest
  runs, its current stage, whether a stage is done, a global's value);
  the census of their parameters supports each and names no other index;
- that log entry conditions are evaluated with the stage already set
  (`cond_log_entry_state_v1`);
- OnStageSet's delivery after the fragment and the meaning of its second
  parameter (`quest_on_stage_set_v1`); its name and its two Int
  parameters were read from the installed scripts' declarations and
  handlers.

Two public documentation pages were requested (the Fallout 4 Creation Kit
wiki's `Conditions` and `Condition_Functions`); both redirected to a
maintenance page and nothing of them was read; no copy elsewhere was
looked for. No function index is given a name: no citable index-to-name
table was read. No editor, editor source or definition file, script
source, decompiler, other interpreter or the game executable was opened
or searched for.

## Provenance of the events step

Recognised from prior general knowledge, used only as hypotheses that the
censuses could refute ([story-manager.md](story-manager.md),
[story-manager-census.txt](story-manager-census.txt), the quest sweeps
and the handler census):

- the roles of the story manager node tags (PNAM, SNAM, CITC, QNAM, NNAM,
  ENAM) and that the code `SCPT` is the script event;
- how an event walks a tree and what a quest node starts
  (`sm_tree_walk_v1`, `sm_quest_node_start_v1`);
- the meaning of condition function 576's first parameter (a selector and
  a member code) for selector 0 with `K1` and `L1`;
- that a custom event's sender is the object `SendCustomEvent` is called
  on, that custom events are delivered queued, and that remote handlers
  run after the sender's own scripts;
- that `IsDisabled` of an enable child follows its parent (built on the
  enable parent rules of [enable-parent.md](enable-parent.md)).

Read from the installed data with this project's own readers: the native,
event and parameter names of Keyword, Quest and ScriptObject, the handler
names `::remote_<type>_<event>` and `::remote_<script>_<event>`, and the
stored custom event names. Three UESP file format pages were requested on
2026-11-22 and answered HTTP 403; nothing was read from them. No editor,
editor source or definition file, script source, decompiler, other
interpreter, script extender or the game executable was opened or
searched for.

## Provenance of the fragments step

Recognised from prior general knowledge, used only as hypotheses that the
censuses could refute ([scene-record.md](scene-record.md),
[fragment-census.txt](fragment-census.txt),
[story-manager-census.txt](story-manager-census.txt)):

- that the flag bits of an INFO, PACK or SCEN block select begin, end and
  change fragments and a phase fragment's flag byte its begin or end
  fragment, and that a PERK fragment's index is a PRKE entry;
- the SCEN record's parts (phases with start and completion conditions,
  actors, actions with a kind, alias, start and end phase) and the meaning
  of the action kinds (checked against the record types their fields
  name);
- FNAM bit 0x1 of a scene as starting it with its quest;
- what Scene.Start, Stop and IsPlaying do and when a phase ends;
- the owning quest of a dialogue line as the quest that holds its topic;
- that a game-time timer's interval is in game hours and that the global
  conventionally called the time scale relates the two clocks, and that GlobalVariable.SetValue
  sets the value.

The block layouts were found by measurement (a probe printed the bytes of
every block and a tentative reader was run until every block was consumed
exactly). Two UESP file format pages were requested on 2026-11-22
(`Skyrim_Mod:Mod_File_Format/VMAD_Field`,
`Fallout4Mod:Mod_File_Format/SCEN`) and answered HTTP 403; nothing was
read from them. Native names and parameter types were read from the
installed scripts with this project's own readers. No editor, editor
source or definition file, script source, decompiler, other interpreter,
script extender or the game executable was opened or searched for.

## Provenance of the second conditions step

Recognised from prior general knowledge, used only as hypotheses that the
censuses could refute ([conditions.md](conditions.md), Blockers;
[conditions-blockers.txt](conditions-blockers.txt),
[conditions-census.txt](conditions-census.txt), the quest sweeps):

- the run-on words 2 (the reference word) and 5 (an alias of the list's
  quest, by the last word) as what a function that reads a subject reads
  (`cond_run_on_subject_v1`), and the owning quest of a scene's lists as
  its PNAM;
- the meanings of function indexes 72 (the subject's base), 84 (dead
  actors of a base), 248 (a scene plays), 550 (a scene action complete;
  the second parameter's match with the action's INAM is measured) and
  629 (a quest script variable named by CIS2);
- the proposed meanings of 1, 14, 71, 359, 426, 555, 560 and 561, which no
  code relies on;
- that a quest's script variables before any script runs hold the VMAD
  value of the property they back, else their stored initial value.

No public page was requested in this step. No editor, editor source or
definition file, script source, decompiler, other interpreter, script
extender or the game executable was opened or searched for.

## Provenance of the scene-events step

Recognised from prior general knowledge, used only as hypotheses that the
censuses could refute ([voice.md](voice.md),
[voice-census.txt](voice-census.txt),
[scene-clock-census.txt](scene-clock-census.txt)):

- that a scene action runs over the span of phases its start and end
  phases give and that a phase waits for the actions in it
  (`scene_clock_v1`); the real-time clock is this project's reading;
- that the kind-2 SNAM after the end phase is a duration in seconds and
  that a TNAM may bound a random time (`scen_timer_action_seconds_v1`);
- that a dialogue action lasts its line's audio and how a line, its
  responses, their voice files and the speaker's voice type are tied
  ([voice.md](voice.md), Prior knowledge used);
- that the `dpds` words count decoded bytes (`audio_xwma_dpds_seconds_v1`);
- the letters of the player dialogue topics (`scene_player_dialogue_choice_v1`);
- the comparison operator codes (`cond_compare_ops_v1`, earlier) to read
  the opening's phase 4 condition as `>= 50`.

No public page was requested in this step. No editor, editor source or
definition file, script source, decompiler, other interpreter, audio
decoder or tool, script extender or the game executable was opened or
searched for. The game was not run.

## Provenance of the stage-setters step

Recognised from prior general knowledge, used only as hypotheses the
census could refute ([stage-setter-census.txt](stage-setter-census.txt)):

- that engine events are the functions named `On...` the engine's own
  types declare without instructions, and that a remote event runs
  `::remote_<type>_<event>` (`setter_event_entry_v1`, with the earlier
  `vm_remote_events_v1`);
- that the INFO field TIQS and the SCEN fields SCQS hold the stages a
  line, a scene or a phase sets at its begin and end
  (`record_set_stage_fields_v1`);
- that a quest alias's scripts receive the events of the reference the
  alias holds (`vm_event_alias_scripts_v1`), and the name and parameter of
  OnOpen (`vm_event_open_v1`);
- that a playing scene keeps testing its phase conditions
  (`scene_condition_recheck_v1`);
- that a start-scene action starts the scene LCEP names
  (`scene_start_scene_action_v1`);
- the row layout of a location's special reference list (LCSR) and that a
  location alias's ref type picks a row (`quest_alias_location_ref_v1`);
- the quest type's struct of a quest and a stage and its global setter,
  read from the installed compiled script.

The table, the forced opening's stops and every count were measured with
the repository's own readers on the installed data. No public page was
requested in this step. No editor, editor source or definition file,
script source, decompiler, other interpreter, script extender or the game
executable was opened or searched for. The game was not run.

## Provenance of the second opening step

Recognised from prior general knowledge, used only as pending rules that
are off by default:

- that the length of an array never assigned reads as 0, and that an
  array operation on such an array is reported and the script goes on
  with a default element, -1 from a find, no change from a write
  (`pex_array_none_length_v1`, `pex_array_none_read_v1`,
  `pex_array_none_find_v1`, `pex_array_none_change_v1`);
- that a line spoken in a scene is one line of its topic chosen by its
  conditions, and that its TIQS stages are set as it begins and ends
  (`scene_line_stage_fields_v1`, with the earlier
  `record_set_stage_fields_v1` and `scene_dialogue_line_choice_v1`);
- in the notes only, as candidates for what the stops stand for: that a
  stage no record or script sets is set when a menu closes, and that an
  alias filled from a unique actor holds that actor's placed reference.

The array census, the stops of the forced opening and every count were
measured with the repository's own readers on the installed data; the
compiled scripts were read only with the repository's disassembler. No
public page was requested in this step. No editor, editor source or
definition file, script source, decompiler, other interpreter, script
extender or the game executable was opened or searched for. The game was
not run.

## Provenance of the world step

Recognised from prior general knowledge, used only as pending rules that
are off by default: that a primitive's bound words are half sizes and its
kind 1 a box and kind 2 a sphere (checked against the data as above);
that a trigger tests the player's body and that the reference point of an
actor is at its feet (the two readings `world_trigger_point_v1` and
`world_trigger_body_v1`); that a disabled reference has no collision; that
distance registrations are one-shot; that placed statics keep their
editor position; that a cell names its location; the meaning of the
position, cell and line-of-sight natives (their declarations are the
compiled scripts'). The caller's values in the opening's notes (a running
speed of 300 units per second, a body of 120 units, a lift of 1 unit, the
waypoints' seconds) are assumptions of the person running it.

The `XPRM` layout, the kind words, the ground check, the exterior grid and
every count were measured with the repository's own readers on the
installed data, the navigation meshes with `fo4-nav`, the compiled
scripts with the repository's reader. No public page was requested in
this step. No editor, editor source or definition file, script source,
decompiler, other interpreter, script extender, physics engine or the
game executable was opened or searched for. The game was not run.

## Provenance of the talk step

Recognised from prior general knowledge, used only as pending rules that
are off by default: the names and roles of the topic subrecords (checked
against every winning topic, above); that `GREE` and `HELO` are the
greeting and hello subtypes (two game settings' editor ids name a
Miscellaneous Greeting and Hello subtype text); that a greeting plays
when the player talks to an actor and a hello when the player comes near
one; that a dialogue condition runs on the speaker and the one spoken to;
that a quest's dialogue is available while it runs and that higher
priorities win; that a unique actor exists once in the world; that an
actor starts at its editor placement and that triggers report any actor;
that the editor ignores the initially-disabled flag of a record with an
enable parent; that the player is no record of the plugins. The setting
the hello distance is read from, the track's waits, the seconds of the
assumed talk and actor moves, the positions given to them and the lines
named by `--assume-line` are assumptions of the person running the
opening.

The topic layout, the census and every count were measured with the
repository's own readers on the installed data; the compiled scripts were
read only with the repository's disassembler. No public page was
requested in this step. No editor, editor source or definition file,
script source, decompiler, other interpreter, script extender or the game
executable was opened or searched for. The game was not run.

## Provenance of the animation step

Recognised from prior general knowledge, used only as pending rules that
are off by default: that `RegisterForAnimationEvent` delivers
`OnAnimationEvent` to the registered instance until it unregisters; that
event names are case-insensitive; that a reference's model runs the
behaviour graph its extra data names, starting in the start state; that
`PlayAnimation` sends the graph an event; that a transition is taken on
its event, local before wildcard; that a Gamebryo sequence generator plays
the NIF sequence of its name and that the sequence's text keys become
events at their times; that a package's Location input is where it takes
the actor. The meaning of the PACK subrecords' tags (PKCU's count and
template, ANAM type names, PLDT and PTDA) and of the behaviour classes were
recognised from their stored names and kept only as far as the censuses
hold. NIF field names come from the nifxml copy already cited. The waits
and the descent on the track, the load through the automatic door, the
seconds of package arrival and the events assumed by the caller are
assumptions of the person running the opening.

The behaviour layouts, the annotation layout, the PACK layout and every
count were measured with the repository's own readers on the installed
data (first proposed from local probes whose outputs stay outside the
repository); the compiled scripts were read only with the repository's
disassembler. No public page was requested. No editor, editor source or
definition file, Havok SDK, converter or format project, script source,
decompiler, other interpreter, script extender or the game executable was
opened or searched for. The game was not run.

## Provenance of the load step

Recognised from prior general knowledge, used only as pending rules that
are off by default: that `MoveTo` with zero offsets places the receiver at
its target; that a reference's 3D loads when it comes into, or the player
comes into, a loaded cell or grid and that `OnLoad` then runs; that
function 131 is how far a scene action has run, and that a phase's
completion conditions can end it while its actions run. That an animation
event named as an idle record's event text is raised at the end of a
furniture's synchronised idle clip whose last annotation carries it, and
that the package's sit procedure and the furniture's keyword lead to that
clip, is prior knowledge used only to search the data and to explain why
the event stays the caller's. Every count was measured with the
repository's own readers on the installed data (`load_census`,
`action_percent_census`; local probes whose outputs stay outside the
repository); the compiled scripts were read only with the repository's
disassembler. No public page was requested. No editor, editor source or
definition file, Havok SDK, converter or format project, script source,
decompiler, other interpreter, script extender or the game executable was
opened or searched for. The game was not run.

## Provenance of the sit step

Recognised from prior general knowledge, used only as pending rules that
are off by default: that function 1 is the distance between two
references and that the player's reference is the engine-made id 0x14;
that activating an actor raises its activation event; that sitting raises
`OnSit` and getting up `OnGetUp` and `OnExitFurniture`, that a furniture's
default activation by an actor makes the actor use it and that the
default-processing flag skips the activation event and its block; what
the menu events and their registrations are; that the player's base is
the NPC_ the caller names. That the player's sex is chosen in character
creation is prior knowledge used only to leave it to the caller. Every
count was measured with the repository's own readers on the installed
data (`alias_fill_census`, `sit_menu_census`; local probes whose outputs
stay outside the repository); the compiled scripts were read only with
the repository's disassembler. No public page was requested. No editor,
editor source or definition file, other interpreter, script extender,
decompiler or the game executable was opened or searched for. The game
was not run.

## Provenance of the keyword step

Recognised from prior general knowledge, used only as pending rules that
are off by default: that a placed reference carries its base object's
keywords and an actor its race's; what `HasKeyword`, `AddKeyword` and
`RemoveKeyword` do; that a collection alias filled from a location's ref
type holds the references of that type, and what its count, index, find,
add and remove natives do. That `RegisterForAnimationEvent` answers
whether the registration was made is prior knowledge used only to leave
the answer to the caller. The reading of a scene action whose start
phase was skipped has no source. Every count was measured with the
repository's own readers on the installed data (`keyword_census`; local
probes whose outputs stay outside the repository); the compiled scripts
were read only with the repository's disassembler. No public page was
requested. No editor, editor source or definition file, other
interpreter, script extender, decompiler or the game executable was
opened or searched for. The game was not run.

## Provenance of the graph step

Recognised from prior general knowledge, used only as pending rules that
are off by default or to search the data: that a playing scene keeps
testing its phase conditions while it waits; that a race names the
behaviour project of its characters (the MODL texts after NAM3 were found
by measurement to name project files); that a fragment's owning quest is
the quest whose group holds its record and that a call on it runs that
quest's script. That the spouse's animation events come from the clip
whose annotations name them, played while she uses the furniture the
script snaps her into, is prior knowledge used only to choose the
caller's seconds. Every count was measured with the repository's own
readers on the installed data (`scene_variable_census`, `stage_census`,
`char_graph_census`; local probes whose outputs stay outside the
repository); the compiled scripts were read only with the repository's
disassembler, the behaviour and animation files only with `fo4_havok`'s
existing readers and a byte search of their stored bytes. No public page
was requested. No behaviour or animation tool, converter or library of
any project, Havok SDK, editor, editor source or definition file, other
interpreter, script extender, decompiler or the game executable was
opened or searched for. The game was not run.

## Provenance of the exit step

Recognised from prior general knowledge, used only as pending rules that
are off by default, as caller assumptions or to search the data: that
activating a furniture makes an actor use it whether or not the
activation's events are raised, and that activating the furniture one sits
in stands one up; that a furniture's animation follows its user's (the
furniture graphs' on and off event pairs were then found by measurement);
that the player's camera event at the pod's exit comes from the player's
exit animation (two clips of the pod's exit folder annotate it at their
start; used only to choose the caller's second); that the trigger at the
vault's exit elevator lies past the vault's door. Every count was measured
with the repository's own readers on the installed data: the compiled
scripts with the repository's disassembler, records with its record dump,
the pod's graph and the furniture graphs with `fo4_scene::anim_graph`
(local probes whose outputs stay outside the repository), the player's
event with `char_graph_census`, the stages off the run's path with the
stage-setter table and a probe of conditions and literal call sites. No
public page was requested. No behaviour or animation tool, converter or
library of any project, Havok SDK, editor, editor source or definition
file, other interpreter, script extender, decompiler or the game executable
was opened or searched for. The game was not run.
## Provenance of the timeline step

Recognised from prior general knowledge and used only as pending rules that are off by default or as
presentation: that the NoWait forms of Enable and Disable differ from the plain natives in when the
script goes on, not in the end state (`timeline_nowait_visibility_v1`); that MoveTo's further argument
concerns the receiver's rotation (named only to say it is not read, `timeline_move_stored_rotation_v1`);
that a dialogue response's text is a localized string (looked up in the plugin's three string tables,
used only where exactly one holds the id). Everything else is read from the run itself and the
installed data with the repository's own readers. No public page was requested; no tool, library or
source of any other project was opened. The game was not run.
