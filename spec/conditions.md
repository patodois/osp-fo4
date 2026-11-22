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
  The four functions the evaluator evaluates (56, 58, 59, 74) carry a
  meaning recognised from prior knowledge, each a pending rule whose
  parameter census is below.
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
13,803 lists false and 2,998 true and leaves 48,982 undetermined; for
QUST 700 false, 292 true, 6,150 undetermined. The function indexes that
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

Under the left-to-right reading instead of OR groups, 195 lists evaluate
differently in that state (8 false here and true there, 182 false here
and undetermined there, 5 undetermined here and true there): these lists
are where a measurement can separate the two readings.

## What is not known

- The meaning of bytes 1-3 and 10-11 (not zero in most CTDA; kept as
  stored), of the run-on values other than 0, of the last word, of the
  parameters of every function but the four evaluated, and of every
  function index but those four (whose meanings are themselves pending).
- Whether a list is evaluated with OR groups or left to right, and how
  the game treats the OR bit on the last condition.
- What the subject of a condition is in each host record type (a quest
  log entry, a dialogue line, a package): the evaluated functions do not
  read it, which is why only they are evaluated.
