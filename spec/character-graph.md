# A character's clip from its behaviour graph

Which clip a character's behaviour graph reaches from its root when nothing
has happened yet, read from the data alone, and the pose that clip gives a
run timeline's actors (`--timeline-cast`). `fo4_havok` reads the generator
classes, bindings and variables (`crates/fo4-havok/src/behavior/nodes.rs`);
`fo4_scene::char_graph` walks a race's graph to a clip; the cast poses its
actors with it (`fo4_scene::timeline::cast::start_poses`, the clip sampled as
[actor.md](actor.md), Clip pose, samples a named clip). Counts are from
`char_start_clip_census` ([character-start-clip-census.txt](character-start-clip-census.txt))
over the forced opening's timeline and every behaviour graph file of one
installation. Every behaviour is a pending rule behind its own switch, off by
default; with every switch off no output changes.

## Sources

- **Measurement alone.** No Havok SDK, header, converter, behaviour editor,
  editor definition, third-party reader or project of these formats was
  opened. Objects were printed field by field with a local probe (bytes,
  first-table pointers, second-table links), the offsets proposed from them
  and checked over all 450 behaviour graph files by the census (section 11).
- Class names are the strings stored in `__classnames__`; field labels are
  this project's. That the objects are state machines, blenders, selectors,
  references and clip generators, as their names say, and the meaning of a
  few fields (named below as readings) is prior general knowledge kept only as
  far as the checks could refute it.
- Record subrecords (RACE NAM3, MNAM, FNAM, MODL, SRAC, SAKD, STKD, SGNM, SAPT,
  SRAF; NPC_ and RACE KWDA; NPC_ TPLT) are the four-character signatures stored
  in the data, their layouts measured on the winning records.

## The chain to a graph

| Step | Read | Census |
| --- | --- | --- |
| race | the MODL text after the race's NAM3 marker, in the actor's section (after MNAM for 0, FNAM for 1), as ANAM is read for the skeleton (`actor_acbs_bit0_second_section_v1`) | 4 race sections of the run's 84 resolved actors, 3 races, 2 projects; all resolve |
| project, character, graph | as [havok-behavior.md](havok-behavior.md) (project +48, character +184, both relative to the project's folder) | |
| behaviour reference | the text at +136 of a reference generator, relative to the project's folder | 64 of 69 references mounted |
| subgraph | the SGNM text of a race's subgraph entry, under `meshes` | 5,205 entries, every file mounted |

## Objects read

| Class | Offset | Content | Check (450 files unless said) |
| --- | --- | --- | --- |
| any generator | +16 | link: an `hkbVariableBindingSet`, or none | |
| | +56 | text: the node's name (presentation only) | |
| `hkbVariableBindingSet` | +16 | array of 40-byte bindings | storage fits in all |
| binding | +0 | text: the member path (`startStateId`, `selectedGeneratorIndex`, `weight`, ...) | a text in all 6,494 |
| | +28 | i32: variable index | among the graph's variables in all 4,960 kind-0 bindings |
| | +32, +33 | i8 bit (-1 in all 6,494), u8 kind (0 in 4,960, 1 in 1,534) | |
| `hkbBehaviorGraphData` | +32 | array of 6-byte variable infos; the type byte at +4 | count equals the initial words and the names in 450 of 450 |
| | +96, +104 | links: the initial values, the string data | |
| `hkbVariableValueSet` | +16, +32, +48 | arrays: one 4-byte word per variable; 16-byte values; links | 16-byte count equals the variables of type byte 6 to 8, link count those of type 5, in 450 of 450 |
| `hkbBehaviorGraphStringData` | +48 | array of texts: variable names | |
| `hkbStateMachine` | +168, +208, +224 | as [havok-behavior.md](havok-behavior.md) | start state among the states in 561 of 561 machines of the run's graphs; 537 start states bound, all to a type-3 variable |
| `hkbModifierGenerator`, `BSiStateTaggingGenerator`, `DynamicAnimationTaggingGenerator` | +144 | link: the generator they hold | a link in all |
| `BSOffsetAnimationGenerator` | +144, +160 | links: a generator; a clip generator | |
| `hkbBlenderGenerator`, `hkbPoseMatchingGenerator` | +152, +154 | i16 and u16 flag word, kept as stored | |
| | +160 | array of links to `hkbBlenderGeneratorChild` | every element such a child in 1,028 blenders |
| `hkbBlenderGeneratorChild` | +48, +64 | link: generator; f32 weight | finite in all 2,727 |
| `hkbManualSelectorGenerator` | +136, +152 | array of generator links; i8 index | the index one of the generators in 134 of 134 |
| `hkbBehaviorReferenceGenerator` | +136 | text: behaviour file | a text in all 131 |
| `BSBehaviorGraphSwapGenerator` | | no link in any of the 47 | |
| `hkbClipGenerator` | +144 | text: the clip (`Animations\...\NAME.hkt`) | a text in 4,504 of 4,505 |
| | +152 | link: trigger array | |
| | +164, +168, +172, +176, +180, +184 | f32: crop start, crop end, start time, playback speed, enforced duration, user fraction | start time 0 in 4,498 |
| | +188, +190, +191 | i16 binding index (-1 in 4,504), u8 mode (0: 2,374, 1: 1,846, 2: 284), u8 flags | |

The variable type bytes seen are 0 to 5, 7 and 8; the walk reads 0 to 3 as
signed integers of 1, 1, 2 and 4 bytes and 4 as an f32 (a reading: the bound
start states are all type 3 and their stored values are states of their
machine in 248 of 252). Generators of other classes (`hkbLayerGenerator`,
`BSBoneSwitchGenerator`, `BSCyclicBlendTransitionGenerator`,
`hkbReferencePoseGenerator` and others: 1,682 objects) are not read.

## The race's subgraph entries

A race's records after its own subrecords hold runs of SAKD (actor keyword,
FormID), STKD (target keyword), SGNM (a behaviour file), SAPT (an animation
folder, repeated) closed by SRAF (a u32). The human race holds none and names
another race with SRAC, which holds 2,601 entries. By SRAF word: 0 for 61
entries of the movement graph (one without any keyword), 1 for weapon
behaviour files with target keywords, 2 for files the names say are
furniture, bit 16 for first-person folders (section 9). The root graph's
swap generator stores nothing that names an entry.

## The walk (`fo4_scene::char_graph::start_clip`)

From the graph's root: a machine's start state (or its bound variable's
stored value), a wrapper's generator, an offset generator's generator, a
blender's child of the highest weight (a bound weight its variable's stored
value; the first of equal weights), a selector's index (or its variable's), a
reference's file, a swap generator's subgraph entry, down to a clip
generator; then the clip file. Each step is recorded (`Step`) for the overlay
and the census. Refusals are named: `GraphStartSwitchOff`,
`GraphSwapAtRunTime`, `GraphClipFolderSwitchOff` (a switch off),
`GraphTransitionWithoutEvent`, `GraphBlendFlag` (flag bit 0x10: every blender
with a child weight outside 0 to 1 has it, 224 of 331 with the bit),
`GraphBindingCharacterProperty`, `GraphBindingBit`, `GraphVariableType`,
`GraphStartStateMissing`, `GraphSelectorIndex`, `GraphActorKeywords`,
`GraphSubgraphEntry`, `GraphClipFile`, `GraphUnreadClass`, and limits
(`MAX_STEPS`, `MAX_FILES`).

| Reading | Rule | Switch (render, fo4-view --headless) |
| --- | --- | --- |
| start states, stored initial values, highest weight, stored index, reference files; drawn at the generator's start time | `char_graph_start_state_clip_v1` | `--actor-start-clip` |
| a swap generator plays the race's keyword-matched entry of SRAF word 0 | `char_graph_swap_subgraph_keywords_v1` | `--actor-subgraph-keywords` |
| the clip's file name in the entry's first SAPT folder that holds it | `char_graph_clip_folder_order_v1` | `--actor-clip-folder` |
| drawn at the frame's second, folded by the generator's mode | `char_graph_clip_mode_loop_v1` | `--actor-start-clip-loop` |

All need `--timeline-cast` (and so `--under-hypotheses`). The pose also
relies on `actor_clip_skeleton_beside_nif_v1` and
`actor_clip_pose_by_bone_name_v1`. Actors that share a skeleton, a clip and a
clip time share one pose: each gets its pose's key (`Actor::pose`, keys
counted down from `usize::MAX`) and `build_with_posed_actors` skins it there;
an actor refused keeps the rest pose with the reason after its `character`
overlay line. The `cast` overlay line and the `summary.json` `cast.pose` key
change only with a switch.

## The opening's actors

With every switch on (census section 10): 79 of the 84 resolved actors reach
a clip; 3 are refused because their base has a template (its keywords are not
read) and 2 at an unread class (the vertibird's graph). Every reaching path
has 20 or more generators over 3 graph files (root, subgraph, a referenced
dialogue graph) and takes one tie (a blender whose second child, a
bone-weighted camera layer, shares weight 1). 48 actors pick an entry that
needs one of their keywords, 33 the entry without keywords. 7 distinct clips
are reached, all of mode 1, 4.0 to 12.9 s long, all fitting the human
skeleton.

## Sitting

The timeline says where an actor stands, never what it sits on: its event
kinds are player, actor, visibility, move, stage, objective, quest, scene,
line and note (section 12). The data would let a sit be read the same way:
SRAF word 2 entries (223 in the human subgraph race) carry the furniture's
keywords as target keywords; over the 782 furniture records, the entries of
word 2 without actor keywords whose target keywords all are the furniture's
give one entry with the most in 509, a tie in 4, none in 259, and in 10 only
entries that also need actor keywords. The furniture graph's start state is
a sitting idle (its stored path reaches a reference and a clip). Missing for
a sit: a timeline line saying who sits on what (the run's sitters are known
to the script run, `--sit-state`, but are not written to the timeline, whose
writer is `fo4-vm`'s), where on the furniture the actor sits (the furniture
model's markers are not read) and which of an entry's target keywords a
furniture needs at once. Nothing was drawn for it.

The seen step reads the sit lines the run's timeline now carries and the furniture's markers (FURN SNAM,
`spec/timeline.md`, Deaths and furniture) and adds `--actor-sit-clip`
(`char_graph_furniture_entry_keywords_v1`, `char_graph::furniture_clip`): a seated actor is posed by the clip
reached from the root of the race's flag word 2 entry without actor keywords whose target keywords all are the
furniture's, the one with the most (a tie or none refused). On real data no actor sits in the run and no such
entry fits the six furniture records of the player's sits (`spec/seen-census.txt`, section 2): synthetic
tests only.

## Races without entries of flag word 0 (the poses step)

On the museum step's furthest run, at the faces step's two town seconds (1345 s and 1470 s), 17 of the 44 characters
kept the rest pose, all refused at the swap generator (`GraphSubgraphEntry`). Census `poses_census`
([poses-census.txt](poses-census.txt), section 1; keys outside git): all 17 are creatures of six races (dogs,
insects, a quadruped and a crustacean among them), every one with its keywords settled by the template bits; none
of the six races holds an entry of flag word 0 without target keywords. Their movement entries are flag word 1
without target keywords (one of them without actor keywords in each race), their furniture entries flag word 2
and 3. Over the load order's 83 races holding entries: 9 hold entries of flag word 0 without target keywords, 43
hold none of those but entries of flag word 1 without target keywords, 5 only flag word 1 entries with target
keywords, 26 neither (furniture entries only). In the human subgraph race flag word 1 entries carry target
keywords (weapon behaviour files). That flag word 1 plays the role flag word 0 plays for the human race in these
races is read from that pattern and the behaviour files' names, not from a source: a pending rule.

| Switch | Rule | Reading |
| --- | --- | --- |
| `--actor-subgraph-flag-one` (with `--actor-subgraph-keywords`) | `char_graph_swap_subgraph_flag_one_v1` | a race whose entries hold none of flag word 0 without target keywords picks among its entries of flag word 1 without target keywords, by the actor's keywords as `char_graph_swap_subgraph_keywords_v1` does (`pick_entry_with`) |

With it, 11 of the 17 reach a clip (each race's idle) and are posed in the CPU render and the viewer; the other 6
(the dogs) stop at a `hkbLayerGenerator` (`GraphUnreadClass`): its layout is not read, and which layer a pose
would take is a further reading. No race with an entry of flag word 0 changes (the switch reads flag word 1 only
where flag word 0 has nothing).

## What it does not do

- No transition is ever taken: the pose is the start state's, whatever the run
  did to the actor (dialogue, alarm, combat, walking between places).
- Variables the engine may set per actor before the first frame keep their
  stored values (a sex flag and a player flag are bound in the run's graphs).
- One layer: of a blend, only the first child of the highest weight is drawn;
  the offset clip of an offset generator, bone-weighted layers and additive
  clips are not drawn.
- The actor's keywords are its base's and its race's only.
