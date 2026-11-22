# Script interpreter (`fo4-vm`)

`fo4_vm` runs the compiled scripts read by `fo4_pex` ([pex.md](pex.md))
under pending rules, with a host that records native calls instead of
performing them. It is the smallest interpreter that runs one real
interaction, and every result it gives is labelled with the rules it relied
on. Nothing here has been measured in the running game: every opcode
meaning, every binding rule and every host answer below is a registered
`pending` rule in [hypotheses.tsv](hypotheses.tsv), and the checks recorded
here support some of them without establishing any.

Evidence tags as in [pex.md](pex.md). Names of scripts, objects, records and
cells stay out of this file; the census tools write them to files that are
not versioned. The exception is the handful of engine interface names the
code itself binds to (listed under Recording host and Binding), without
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
resolved through the load order); handles of different records compare
unequal; whether an opaque handle (the player's, or a VMAD object value
carrying an alias index, whose meaning is not read) is the same object as
another handle is left undetermined.

Timers fire only when asked (`script_run ... --timers N`), in due order; the
firing stops with an error when the queue does not order them (an
undetermined interval, both clocks queued, or two timers of one id on one
instance).

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
- **Reference index**: the placed references of a cell are numbered as
  `fo4_vm::bind::placed` lists them: the active ones, then the initially
  disabled ones, each in stored order.

`script_run MANIFEST CELL[@DX,DY] list` prints the scripted placed
references with their indices; `script_run MANIFEST CELL[@DX,DY] INDEX
EVENT[,EVENT...] [--timers N]` prints the effect log. `@DX,DY` takes the
exterior cell that many grid steps east and north.

## Census

`script_run MANIFEST --census OUT_DIR CELL[:RADIUS]... | ALL` delivers to
each scripted DOOR, TERM and ACTI, one fresh run per event, every event
that one of its scripts has a function with instructions for, and every
TERM fragment. The counts are [vm-census.txt](vm-census.txt); the names go
to files that are not versioned.

| Scope | Objects | Handlers run | Completed | Blocked on a native's result | Blocked on an object without that script | Blocked on an open rule | Failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| The two test cells of the survey (one interior, one exterior with radius 1) | 10 placed ACTI | 22 | 16 | 6 | 0 | 0 | 0 |
| Every winning scripted DOOR, TERM and ACTI base, bound alone (no reference VMAD) | 1,251 | 2,530 | 1,113 | 902 | 335 | 77 | 103 |

- In the test cells, the 6 blocked handlers block on four natives (one of
  them three times), each at a conditional jump or a call that needs its
  result; no handler fails.
- Over all bases, blocks happen at opcode 22 (840), 23 (424), 29 (26), 28
  (15) and 21 (9). 152 distinct origins; the ten most frequent account for
  776 blocked handlers (306, 89, 83, 46, 45, 44, 43, 43, 43, 34): six are
  natives returning a value, four are scripted calls on records whose
  scripts are not instantiated (quests and other records named by
  properties). Open rules: casts of objects to a type their known types do
  not include (60), the length of none (7), texts equal but for case (5),
  the identity of an opaque handle (5).
- Failures over all bases: a call, property or array operation on none (99:
  mostly properties that only a placed reference's VMAD would fill, as bases
  are bound alone), three TERM fragments naming a function the fragment
  script does not declare ([vmad.md](vmad.md) counts 11 such), and one
  index one past the end of an array in a loop guarded by `index <= length`
  (opcode 17), consistent with a mistaken bound in its source under the
  candidate.

## Visible effect (`fo4-view --script-event`)

`fo4-view ... --under-hypotheses --headless --script-event INDEX:EVENT`
(a viewer built with `--features script-event`; the default build does not
link the interpreter and refuses the option) binds the cell's placed reference INDEX, delivers the events, prints the
effect log, and leaves out of the scene each reference of the loaded cells
whose last visibility change in the log is a `Disable`
(`vm_disable_hides_reference_v1`); an `Enable` of a drawn reference changes
nothing, and of an initially disabled one is only reported (drawing it is
not implemented). Every other native stays log-only. Without the option
nothing changes.

In the test cells one interaction produces a determined visibility change:
activating one scripted activator of the exterior cell runs its handler to
completion, which logs an item given to the player, `Disable` on the
activator itself, and its deletion. The viewer does not draw that
activator in the first place (its reference has seven blockers, among them
a runtime condition and uninterpreted visual subrecords), so hiding it
changes no pixel: the colour frames before and after are byte-identical,
the instance tags differ only by the renumbering of the instances after the
removed one (every tag maps one to one, to itself or to itself minus one),
and the build counts one omitted placement fewer. No
other test-cell interaction disables or enables a drawn reference (one
other completed handler enables a reference that is already drawn). **No
visible effect is shown.**

Byte identity of the viewer without the option is not decidable by frame
comparison on this machine: the eight headless regression cases of the
previous steps, rendered by main's viewer and by main's unchanged source
rebuilt in a second target directory, differ in 7 of their 15 frame and
tag files (each binary repeats its own output byte for byte). The default
build of this change differs from main in 6 of those 7 files and nowhere
else; built with the feature, in the same 7 and one more tag file. The
printed load reports (placements, omissions, meshes, textures, instances)
are identical in all of them.

## Limits

- One installation, one game build; no measurement of the running game.
- Each delivery starts from a freshly bound object; event sequences are
  only those named on the command line; no other record's scripts
  (quests, aliases, other references) are instantiated, so calls into them
  block.
- Natives are never performed, latent natives do not wait, and the host
  supplies one value (the player handle).
- Opcode 34's arity between `4` and `4+n` stays open ([pex.md](pex.md));
  the interpreter reads its fourth slot as the start index.
