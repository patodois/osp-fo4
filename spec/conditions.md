# Conditions (`CTDA`, `CIS1`, `CIS2`)

`fo4_plugin::condition` reads the condition subrecord to its last byte
and the two text subrecords that may follow it; `fo4_vm::cond` evaluates
what the data and the quest state settle ([vm.md](vm.md), Conditions).
The census tool is `cond_census` (in `fo4-vm`), its counts
[conditions-census.txt](conditions-census.txt). What a condition does in
the running game is behaviour: every meaning below is a pending `cond_*`
rule in [hypotheses.tsv](hypotheses.tsv); none is measured.

## Sources

- No public source of the layout or of the function indexes was read.
  The Creation Kit wiki pages on conditions and condition functions were
  requested on 2026-11-22 (the Fallout 4 wiki's `Conditions` and
  `Condition_Functions`); both redirected to a maintenance page, and no
  other copy was looked for. xEdit's definitions, which hold a function
  table, stay closed (AGENTS.md).
- The field layout below was recognised from prior general knowledge and
  is kept only because the census over every winning record could have
  refuted it and did not (`cond_ctda_layout_v1`).
- **Function indexes stay bare numbers.** No index is given a name
  anywhere in this repository: no citable index-to-name table was read.
  The functions the evaluator evaluates (56, 58, 59, 74, 576 and, since
  the second conditions step, 72, 84, 248, 550 and 629) carry a meaning
  recognised from prior knowledge, each a pending rule whose parameter
  census is below; the meanings proposed for 1, 14, 71, 359, 426, 555,
  560 and 561 are pending rules too, recognised the same way, that no
  code relies on (Blockers, below).
- The FormID contract (master index, remapping through the record's
  plugin) is that of [plugin.md](plugin.md).

## Layout

| Bytes | Field | Read as | Census (130,308 CTDA) |
| --- | --- | --- | --- |
| 0 | operator and flags | bits 5-7 operator code, bits 0-4 flag bits | codes 0 to 5 only; flag bits below |
| 1-3 | unused | kept as stored | not zero in 120,549: not padding with a value, no meaning read |
| 4-7 | comparison | f32, or a FormID when flag bit 0x04 is set | bit set: a winning GLOB in 10,005 of 10,005; clear: a finite f32 in 120,303 of 120,303, a GLOB in none |
| 8-9 | function index | u16 | 293 distinct indexes |
| 10-11 | unused | kept as stored | not zero in 96,718 |
| 12-15 | parameter 1 | u32 kept as stored; [`form`] resolves it on request | by function (census) |
| 16-19 | parameter 2 | u32 kept as stored | by function (census) |
| 20-23 | run-on | u32 kept as stored | values 0 to 10 |
| 24-27 | reference | u32 kept as stored | zero for every run-on value but 2 |
| 28-31 | last word | i32 kept as stored | -1 for most run-on values; an alias index of the quest for run-on 5 (287 of 287 in quests), a KYWD for run-on 4 (658) |

`CIS1` and `CIS2`, when present, follow their CTDA in that order, each a
text with one closing NUL (680 and 12,162; `CIS2` after every condition
of functions 629 and 660 and of no other, `CIS1` after every condition of
447, 659, 660 and 675 and of no other).

A **list** is a run of CTDA subrecords (each with its texts) not
interrupted by another tag; `condition::lists` returns every list of a
record with the tags around it. `CITC` occurs 3,190 times in ACTI, FACT,
FURN, MUST, PACK, SCEN, SMBN, SMEN and SMQN; the 1,371 that immediately
precede a list are 4 bytes and equal to the list's length in all, the
others are not followed by a CTDA.

Named errors: `ConditionSize` (a CTDA not of 32 bytes), `ConditionText`
(a text without one closing NUL), `ConditionStringOrder` (a CIS1 or CIS2
not after a CTDA, a CIS1 after a CIS2, or a second one), each with file
and offset. The census met none of them.

Flag bits (byte 0, bits 0-4): 0x01 in 8,393 conditions, 0x02 in 3,955,
0x04 in 10,005, 0x08 in 109, 0x10 in 30. Candidate meanings: 0x01 joins a
condition to the next by OR (`cond_list_or_groups_v1`); 0x04 makes the
comparison a global (`cond_use_global_v1`, the one the census checks
directly); 0x02 makes the parameters aliases (in quests, the first
parameter names an alias of the quest in 3,308 of 3,320), 0x08 package
data, 0x10 swaps subject and target (`cond_flag_parameters_v1`).

Operator codes: 0 (104,953), 1 (10,186), 2 (2,221), 3 (4,421), 4
(6,794), 5 (1,733); candidate meanings equal, not equal, greater, greater
or equal, less, less or equal (`cond_compare_ops_v1`).

## Census

Over the 2,189,079 winning records of the load order, 55,096 records in
27 record types carry 130,308 conditions in 65,783 lists. By host type,
most first: INFO 88,412, QUST 14,960, PACK 6,126, SCEN 4,688, IDLE 3,376,
COBJ 3,319, TERM 1,778, PERK 1,718 and 19 more types
([conditions-census.txt](conditions-census.txt) has each).

- Lists: 34,417 of one condition, 28,181 joined by AND only, 1,604 by OR
  only, 1,581 mixing both (where the combination rule matters); OR
  groups of 2 (2,169), 3 (370), 4 (503), 5 to 9 (148), 10 to 19 (12);
  3,093 lists end with the OR bit set on their last condition.
- Run-on word: 0 (113,273), 2 (7,953), 5 (3,423), 1 (2,828), 3 (948), 7
  (807), 4 (753), 8 (163), 6 (146), 10 (10), 9 (4). With 2, the reference
  word names a placed REFR, ACHR or PHZD in 227, and 7,725 times an
  object id below 0x800 that no record of the load order defines (the
  reserved range).
- Function indexes: 293 distinct; the most frequent are 59 (18,608), 72
  (17,964), 629 (11,716), 74 (10,818), 71 (8,698), 566 (5,986), 1
  (4,734), 561 (4,535), 426 (4,229), 70 (4,140). The census lists every
  index with its host types, operator codes, flags, run-on words,
  comparison values and texts, and what each parameter names when read as
  a FormID through the record's plugin (a record type, zero, a number
  below 0x800 that names nothing, a FormID naming no winning record, or a
  word whose master index is outside the plugin's masters).
- Quests: 593 log entry lists (617 conditions of function 59, 125 of 629,
  76 of 74, 32 of 84, 30 of 70, 22 of 46, 16 of 58, 7 of 491 and 7 more),
  140 lists before NEXT, 576 after it, 995 on objective targets, 4,838 in
  aliases.

The four evaluated functions:

| Index | Conditions | Parameter 1 | Parameter 2 | Comparison | Run-on |
| --- | ---: | --- | --- | --- | --- |
| 56 | 510 | a winning QUST in all | zero in all | 0 or 1 in all | 0 in all |
| 58 | 3,665 | a winning QUST in 3,655 (10 zero) | zero in all | a stage of that quest in 3,328 of 3,655; all six operators | 0 in 3,660 |
| 59 | 18,608 | a winning QUST in 18,592 (16 zero) | a stage of that quest in 17,902 of 18,592 | 1 (11,208), 0 (7,364), other integers 36 | 0 in 18,596 |
| 74 | 10,818 | a winning GLOB in all | zero in all | a global (3,434), 1, 0, other numbers | 0 in all |

## Evaluation over the data alone

In the new-game state before any script runs (a quest runs exactly when
its DNAM carries the start-game bit, no stage is done, the current stage
is undetermined, globals hold their stored value), the evaluator settles
16,542 lists false and 3,990 true and leaves 45,251 undetermined; for
QUST 800 false, 379 true, 5,963 undetermined. **Regenerated in the second
conditions step**: before the five added evaluators (with
`--no-cond-fn-72 --no-cond-fn-84 --no-cond-fn-248 --no-cond-fn-550
--no-cond-fn-629 --no-cond-run-on`, which reproduces the earlier file
byte for byte) the counts were 13,803 false, 2,998 true, 48,982
undetermined (QUST 700, 292, 6,150). Of the added functions' conditions,
629 settles 11,715 of 11,716 (8,644 true), 84 all 600 (456 true), 550
416 of 417 (415 false), 248 180 of 212 (the 32 with a zero parameter stay
open); 72 settles none in this state (its run-on 2 and 5 conditions name
aliases without a forced reference or of quests that do not run), so
its line counts it under its own reasons, which raises its per-function
count from 13,088 to 13,102 by the per-reason counting described next. The function indexes that
leave the most lists undetermined there: over all records 72 (13,088
lists), 71 (7,053), 566 (4,108), 70 (3,934), 1 (3,766), 561 (3,483), 629
(3,389), 426 (3,213), 650 (3,167), 14 (2,998), 58 (2,429); over quests
561 (3,460), 650 (3,165), 72 (569), 426 (375), 560 (308), 71 (307); most
of these read the subject or an alias, which only a running game fills.

Since the events step function 576 is evaluated too, but only while the
story manager offers an event (`cond_fn_576_event_data_v1`,
[story-manager.md](story-manager.md)); before any script runs there is
none, so it stays undetermined here. The per-function lines of the census
count each distinct reason an undetermined function gives in a list, so a
list whose 576 conditions now give two reasons (no story event, a selector
without a candidate) counts twice: 576 went from 64 to 66 (QUST) and 207 to
213 (all records) with nothing else changed.

Under the left-to-right reading instead of OR groups, 184 lists evaluate
differently in that state (9 false here and true there, 170 false here
and undetermined there, 5 undetermined here and true there; 195 before
the second conditions step: 8, 182, 5): these lists are where a
measurement can separate the two readings.

## Blockers

`cond_blockers MANIFEST OUT.txt [NAMES.txt]` ([conditions-blockers.txt](conditions-blockers.txt),
with every evaluator on; [conditions-blockers-before.txt](conditions-blockers-before.txt),
with the five added ones and the run-on reading off, as the ranking that
chose them) evaluates in the new-game state the lists a run evaluates:
scene phase start (1,867 lists) and completion (1,030) conditions, quest
stage log entry conditions (593), alias fill conditions (4,838, one list
per alias), a quest's story-event conditions after NEXT (576) and story
manager node conditions (366). A list is **blocked only by** an index
when every condition that leaves it open is of that index and the index
is not evaluated (`fo4_vm::blockers`): evaluating that one index decides
it, whatever it answers. Lists also left open by an evaluated function
whose input the state does not settle (58's current stage, 576 outside a
story event, 72 on a subject the interpreter does not know) are counted
apart, with the reason.

Ranking before the second conditions step (lists blocked only by the
index, over the six contexts): 629 (468: phase start 369, completion 59,
log entry 35, story event 5), 72 (464: alias fill 401, story event 50,
phase start 7), 1 (414: phase start 258, completion 154, log entry 1,
alias 1), 561 (276, all alias fill), 426 (223), 550 (210, all phase
completion), 71 (200), 14 (103: phase start 87, node 11, log entry 3,
completion 1, alias 1), 560 (93), 359 (85), 799 (76), 84 (52); 555 has 28
(all phase completion; it is among the blockers of 49).

| Context | Undetermined before | Undetermined after | False after | True after |
| --- | ---: | ---: | ---: | ---: |
| Scene phase start | 1,222 | 818 | 662 | 387 |
| Scene phase completion | 848 | 536 | 475 | 19 |
| Quest stage log entry | 132 | 70 | 356 | 167 |
| Quest alias fill | 4,812 | 4,809 | 25 | 4 |
| Quest story event (after NEXT) | 355 | 325 | 181 | 70 |
| Story manager node | 286 | 286 | 68 | 12 |

After it the ranking is 1 (457), 561 (276), 426 (223), 71 (203), 14
(111), 560 (94), 359 (89), 799 (76), 67 (51), 131 (50), 46 (49), 231 (48);
555 has 29. Function 72 leaves the ranking because it is now evaluated:
its alias fill and story-event conditions run on the fill candidate (run-on
0) or on event data (run-on 7), which the interpreter does not know (483
and 68 lists open for that reason).

### What the parameters of the top indexes and of 555, 1, 629, 14 reference

Measured over the six contexts (`conditions-blockers*.txt`, details
section) and over every record (`conditions-census.txt`). The meaning in
the last column is **prior knowledge**, kept as a pending rule; the
experiment that could refute each is in [hypotheses.tsv](hypotheses.tsv).

| Index | Parameter 1 | Parameter 2 | Run-on | Comparison values | Rule | Evaluated? |
| --- | --- | --- | --- | --- | --- | --- |
| 629 | QUST in 11,716 of 11,716 | an opaque word (not a FormID in 11,715); CIS2 names the variable (10,809 `::Name_var`, 907 plain names) | 0 in 11,713 | 0 to 60, globals (890) | `cond_fn_629_quest_variable_v1` | yes: a script variable of the quest (records, compiled scripts, or the run's instances) |
| 72 | a base record (NPC_ 17,736, FURN, STAT, ACTI, MISC and 9 more) | zero | 0 (16,180), 1, 5 (585), 3, 7, 2 | 1, 0 | `cond_fn_72_reference_base_v1`, `cond_run_on_subject_v1` | for run-on 2 and 5 only: the base of the reference or alias the run-on word names |
| 1 | the reserved id 0x14 or another id below 0x800 (3,943), zero (267), REFR, STAT, ACHR and more | zero | 0, 5, 2 | globals (3,682), 0 to 40,000 | `cond_fn_1_distance_v1` | no: positions of the player and moving actors are not in the state |
| 561 | LCRT (all 4,509 in quests) | zero | 0 | 1, 0 | `cond_fn_561_ref_type_v1` | no: all in alias fills, on the fill candidate |
| 426 | VTYP (402), FLST (105) in the six contexts | zero | 0, 7, 5 | 1, 0 | `cond_fn_426_voice_type_v1` | no: fill candidates and event data |
| 550 | SCEN in 417 | the INAM of an action of that scene in 412 of 416 (405 the list's own scene; 396 of those an action whose phases include the condition's phase); no action in 4 | 0 (411) | 1 | `cond_fn_550_scene_action_complete_v1` | yes: 0 wherever the run settles the scene's state (no action completes in a run) |
| 71 | FACT in 406 of 406 | zero | 0, 7, 5, 2 | 1, 0 | `cond_fn_71_in_faction_v1` | no: membership changes at run time; candidates and event data |
| 14 | AVIF (3,705), an id below 0x800 naming no record (371) | zero | 0, 4, 5, 2, 7 | globals, 0 to 7 | `cond_fn_14_actor_value_v1` | no: actor values are not in the state |
| 560 | KYWD in 632 of 632 | zero | 7, 0 | 0, 1 | `cond_fn_560_keyword_v1` | no: candidates and event data |
| 359 | LCTN in 168 of 168 | zero | 0, 2 (reference word 0x14 in all 40), 5, 7 | 1, 0 | `cond_fn_359_in_location_v1` | no: where references are is not in the state |
| 799 | zero | zero | 5 in all 144 | paired bands from 0 to 65,000 (>= one bound, < the next) on four scenes of one family | none: not recognised | no: nothing in the data says what it reads |
| 84 | NPC_ in 600 of 600 | zero | 0 | 0 (461), 1 (138) | `cond_fn_84_dead_count_v1` | yes in the new-game state (0); undetermined in a run (deaths are not kept) |
| 555 | zero in 157 | zero | 5 (117), 2 (28; the reserved id 0x14 in 18), 0 (12) | 1 (148), 0 (9) | `cond_fn_555_3d_loaded_v1` | no: the interpreter keeps no 3D state |
| 248 | SCEN (180), zero (32) | zero | 0 | 0, 1 | `cond_fn_248_scene_playing_v1` | yes: the run's scene state, none playing at a new game |

Every added evaluator has its own switch (`--no-cond-fn-N` on
`cond_census`, `cond_blockers`, `quest_run` and `script_run`, and
`--no-cond-run-on` for the run-on reading; `fo4_vm::cond::Switches`); with
all of them off every census written before this step reproduces byte
for byte. In `quest_run` a quest variable the run did not attach is read
from the records and compiled scripts; in `script_run` (whose library is
the interpreter's) it is undetermined until the run attaches the quest.

`quest_run --assume-cond N=VALUE` gives every condition of index N that
value: a caller assumption, not a rule, counted in the census as such
(used only to explore past a condition the state cannot decide).

## What is not known

- The meaning of bytes 1-3 and 10-11 (not zero in most CTDA; kept as
  stored), of the run-on values other than 0, 2 and 5, of the last word
  outside run-on 5, of the parameters of every function but the evaluated
  ones and those the Blockers table describes, and of every function
  index but those (whose meanings are themselves pending).
- What function 799 reads (no parameter, always on an alias of the
  scene's quest, compared against paired numeric bands); and the second
  parameter of 629, an opaque word that is not a FormID.
- Whether a list is evaluated with OR groups or left to right, and how
  the game treats the OR bit on the last condition.
- What the subject of a condition is in each host record type (a quest
  log entry, a dialogue line, a package, an alias fill): run-on 0 is left
  undetermined by every evaluated function that reads a subject (72); the
  others do not read it.
