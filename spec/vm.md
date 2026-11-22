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
   determined).

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
