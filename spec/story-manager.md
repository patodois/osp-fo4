# Story manager (`SMEN`, `SMBN`, `SMQN`)

The story manager records are nodes of trees: an event node (SMEN) per
event code, branch nodes (SMBN) under it, quest nodes (SMQN) that list
quests. `fo4_plugin::story` reads every node to its last byte and builds
the tree; `fo4_vm::story` offers the one event a run can raise itself (the
script event `Keyword.SendStoryEvent` raises) to its tree and starts the
quest a node selects. The census tool is `story_census` (in `fo4-vm`), its
counts [story-manager-census.txt](story-manager-census.txt). Nothing here
is measured in the running game: every behaviour is a pending `sm_*` rule
in [hypotheses.tsv](hypotheses.tsv).

## Sources

- **Measurement alone.** The public UESP file format pages were requested
  on 2026-11-22 (`Skyrim_Mod:Mod_File_Format/SMQN`,
  `Skyrim_Mod:Mod_File_Format` and `Fallout4Mod:Mod_File_Format`); the
  server answered HTTP 403 to each and nothing of them was read. No other
  page was looked for.
- The tags' roles (parent, previous sibling, condition count, quest list,
  event code) were recognised from prior general knowledge and are kept
  only because the census over every winning node could have refuted
  them and did not (`sm_node_layout_v1`).
- The event codes are the four bytes stored in SMEN ENAM and QUST ENAM;
  only `SCPT` is given a meaning (the script event, `sm_script_event_v1`),
  recognised from prior general knowledge. The other codes stay bare.
- The native and event names (`SendStoryEvent`, `SendStoryEventAndWait`,
  `OnStoryScript` and its six parameters) were read from the installed
  Keyword and Quest scripts' declarations with `pex_dump`.

## Layout

Order (every part required unless in brackets):

```text
[EDID] PNAM SNAM CITC (CTDA [CIS1] [CIS2])* DNAM XNAM
  SMEN: ENAM
  SMQN: [MNAM] [HNAM] QNAM (NNAM [RNAM])*
```

| Tag | Bytes | Read as | Census (473 nodes) |
| --- | --- | --- | --- |
| EDID, CIS1, CIS2 | text | bytes before one closing NUL | EDID in 454 (8 of 18 SMEN) |
| PNAM | 4 | FormID: the parent node | SMEN: the word 0x5b (below 0x800, no record) in 18 of 18; SMBN: SMBN 76, SMEN 44; SMQN: SMBN 303, SMEN 32 |
| SNAM | 4 | FormID: the previous sibling, or zero | zero 137; a node of the same parent 335; a node of another parent 1 |
| CITC | 4 | u32 | equals the number of conditions after it in 473 of 473 |
| CTDA | 32 | `fo4_plugin::condition` ([conditions.md](conditions.md)) | 764 conditions |
| DNAM | 4 | u32 kept as stored (two u16 halves counted) | SMBN low half 0 or 1; SMEN 0 or 2; SMQN low 0 to 3, high 0 to 6 |
| XNAM | 4 | u32 kept as stored | 0, 1 or 2 |
| ENAM (SMEN) | 4 | four-byte code | 18 codes, one node each |
| MNAM, HNAM (SMQN) | 4 | u32 kept as stored | MNAM 1 to 3 in 6; HNAM a finite f32 (0.5 to 140) in 36 |
| QNAM (SMQN) | 4 | u32 | equals the number of NNAM in 335 of 335 |
| NNAM (SMQN) | 4 | FormID | a winning QUST in 1,308 of 1,308 |
| RNAM (SMQN) | 4 | u32 kept as stored, after a NNAM | 357 after a quest (a finite f32, 7.2 to 9,600), 951 quests without |

DNAM, XNAM, MNAM, HNAM and RNAM are kept as stored words, each with its
reason (`fo4_plugin::story::UNSETTLED`): no meaning of theirs is read.
Every other byte is typed. Named errors: `StoryNodeType`,
`StoryNodeOrder` (expected, found), `StoryFieldSize`, `StoryFieldText`,
and the condition reader's own; the census met none.

## The tree

`Tree::read` reads every winning node; a node's children are the nodes
whose PNAM names it, ordered by their SNAM links when those make one chain
(one child without a previous sibling among them, every other naming
exactly one, none named twice), otherwise kept in key order and marked
`Order::Ambiguous`.

- Depth below the event node: 0 (18), 1 (76), 2 (154), 3 (95), 4 (90), 5
  (37), 6 (3); every node reaches an event node.
- Sibling order: 131 parents' children make one chain (115 under an SMBN,
  16 under an SMEN); 5 do not (2 under an SMBN, 2 under an SMEN, and the
  18 event nodes themselves under the word 0x5b).
- Quests per quest node: none 2, one 181, two 36, three or four 37, five
  to nine 49, ten to 19 17, 20 to 49 13. 1,013 distinct quests are listed,
  each under exactly one event code.
- Nodes, quest entries and distinct quests by event code are in the
  census; the largest trees are `SCPT` (52 branch and 150 quest nodes, 476
  quests), `ADIA` (31, 60, 367) and `CLOC` (20, 79, 97).
- Conditions on nodes, most frequent function indexes first: 576 (249),
  359 (93), 74 (81), 59 (63), 71 (45), 426 (28), 14 and 310 (25 each), 77
  and 560 (24 each); the census lists every index by node type.
- Function 576's first parameter is not a FormID: a u16 selector and two
  characters, 0 `K1` (153), 0 `L1` (37), 0 `L2` (19), 2 `V1` (7), 2 `V2`
  (1), 3 `F1` (9), 3 `L1` (15), 3 `L2` (8) on nodes.

Quests against the nodes: of 1,905 quests, 987 carry an ENAM equal to the
event code of every node that lists them; 26 listed quests carry no ENAM;
3 carry `SCPT` and are listed by no node; 889 have neither. 457 quests are
flagged to start with the game; 2 of them are also listed by a node.

## What starts a quest

`story_census` also scans every winning compiled script (10,282) for the
call sites of `Quest.Start`, `Quest.SetCurrentStageID`, the scripted
`Quest.setStage` and `SetAllStages` (a stage set on a stopped quest starts
it, `quest_set_stage_v1`), and `Keyword.SendStoryEvent` and
`SendStoryEventAndWait`. A call's receiver is followed back within its
function to `self`, a property (its variable or a property read on
`self`), `GetOwningQuest()` on `self`, a parameter, or something else; a
property resolves to the object values that the VMAD of every record
attaching the script (or a script extending it) stores under that name.

What the scan cannot see: receivers that come from a parameter, a call's
result, an array or another object's property; quests started by the
engine, by the console or by events of the world; and whether a call site
runs at all. Before the fragments step it could not see the scripts of the
fragment blocks of INFO (2,396 VMADs), SCEN (632), PACK (166) and PERK
(32) either; since then they are attachments with their properties, and
an INFO's or SCEN's owning quest is the quest whose group of kind 10 holds
it (`vm_host_owning_quest_v1`, [scene-record.md](scene-record.md)), so
`GetOwningQuest()` in their fragments resolves. `story_census
--no-fragments` leaves them out and reproduces the earlier census byte for
byte.

- Call sites: `setStage` 8,495 (receiver a property 4,399, `self` 3,371,
  `GetOwningQuest()` 697, other 28), `Start` 254, `SetCurrentStageID` 1
  (the scripted `setStage` itself), `SendStoryEvent` 60,
  `SendStoryEventAndWait` 30.
- Receivers that resolve to a constant record (with the fragment blocks;
  before them in brackets): 7,271 `setStage` sites (6,591), 220 `Start`
  sites (206), 30 story event sites (21; 47 keywords, 40 of them tested by
  a node of the `SCPT` tree with function 576 selector 0 `K1`, which tests
  47 keywords). Calls whose constant receiver is the calling script's own
  quest are not counted as starts. Of the 1,861 `setStage` sites in
  scripts no read VMAD attached, 707 are now attached: 502 in INFO, 143 in
  SCEN, 56 in PACK and 6 in PERK fragment scripts.
- 565 quests (554) are the constant receiver of a starting call of a
  script that does not belong to them.

The 1,448 quests not flagged to start, by what could start them: a story
node only 956 (957), a script call only 377 (375), both 55 (54),
**unknown 60** (62; no node, no constant call: started by something the
scan cannot see). By way (a quest may have several): story event `SCPT`
476, `ADIA` 366, `CLOC` 96, `AOBJ` and `LCLD` 16 each, and 13 other
codes; a stage set from a quest stage fragment script 298, a quest script
118, a terminal fragment script 43, a placed reference's script 39 (and 2
of a placed actor), a dialogue line (INFO) fragment script 34, an
activator base's script 33, a scene fragment script 30, a package
fragment script 8; `Start` from a stage fragment 130, from a quest script
15, from an INFO fragment 6, from a scene fragment 1, and the rest in the
census. Two of the 62 quests are now reached (by fragment scripts).

The opening's main quest is among the 377 a script call could start: only
fragments and scripts of quests that themselves wait on world events set
its later stages; no INFO, SCEN, PACK or PERK fragment starts it or sets a
stage of it (its own scenes' fragments set its stages, which needs it
running), and nothing the scan sees sets its first stage. Its first stage
is a startup stage (INDX flag 0x2), so starting the quest sets it
([vm.md](vm.md), Where the opening stops; names in the step's notes,
outside the repository).

**Scenes** (section 7, since the fragments step): 1,626 `Scene.Start` and
16 `ForceStart` call sites, 1,610 and 15 with a property receiver; of the
4,773 scenes, 1,329 are the constant receiver of such a call, 683 the
target of a start-scene action (kind 4) of another scene, and 2,906
neither; FNAM bit 0x1 against these ways is in
[scene-record.md](scene-record.md).

## Rules (behaviour at run time, `fo4_vm::story`)

| Rule | Covers |
| --- | --- |
| `sm_node_layout_v1` | the order grammar and fields above |
| `sm_script_event_v1` | `SendStoryEvent(AndWait)` raises `SCPT` with the receiver as keyword, offered at once inside the call; `AndWait` answers true, false or undetermined; no other event code is raised |
| `sm_tree_walk_v1` | conditions false: skip the node and its subtree; true: children in previous-link order, the first that starts a quest ends the walk; undetermined, or two children of an unordered parent that would both act: undetermined; the stored words are not read |
| `sm_quest_node_start_v1` | the first listed quest not running whose conditions after NEXT hold is started as `Start` starts a quest, then receives `OnStoryScript` with the event's six values |
| `cond_fn_576_event_data_v1` | function 576 selector 0 with `K1` or `L1`: 1 when the event's keyword or location is the second parameter; every other case undetermined |

What the run counts: the events offered, and for each whether a quest
started, none did, or why it is open, with the function indexes of the
undetermined conditions ([vm.md](vm.md), Events and the story manager).

## What is not known

- The meaning of DNAM, XNAM, MNAM, HNAM and RNAM (flags, limits, timers
  are plausible readings; none is used), of the 17 event codes other than
  `SCPT`, and of function 576's other selectors and member codes.
- Whether the walk is depth-first as above, whether a node may start more
  than one quest, how the game orders siblings whose links do not make one
  chain, and how alias filling decides whether a start succeeds.
- What starts the 60 quests with no node and no constant call, and the
  opening's main quest.
