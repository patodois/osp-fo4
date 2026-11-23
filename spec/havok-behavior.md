# Behaviour project, character and graph files

`fo4_havok` reads the `.hkx` behaviour files as far as a state machine of
Gamebryo sequence states needs (`fo4_havok::Machine` and its parts,
`crates/fo4-havok/src/behavior.rs`); `fo4_scene::anim_graph` follows the
chain of files from a model and reads the model's controller sequences;
`fo4_vm::anim` runs the graph under pending rules ([vm.md](vm.md),
Animation events). The container, its tables and the object spans are those
of [havok-container.md](havok-container.md) and
[havok-objects.md](havok-objects.md). Counts are from `anim_event_census`
([animation-event-census.txt](animation-event-census.txt)) over every
winning `.hkx` of one installation.

## Sources

- **Measurement alone.** No Havok SDK, header, converter, behaviour editor,
  third-party reader or project of these formats was opened. Two graphs were
  first printed object by object with a local probe (the bytes, the
  first-table pointers and the second-table links of each object), the
  offsets below were proposed from them and then checked over every
  behaviour file by the census.
- Class names are the strings stored in `__classnames__`. That the objects
  are a project, a character, a graph, a state machine, states,
  transitions, an event table and sequence generators, as the class names
  say, is prior general knowledge kept only as far as the checks below
  could refute it. The field labels are this project's.

## The chain of files

| Step | Read | Census |
| --- | --- | --- |
| NIF | `BSBehaviorGraphExtraData`, its `Behavior Graph File` text (nifxml field names, as `fo4_nif` reads them) | 1,189 NIFs name a graph file; under `meshes` it is a file holding `hkbProjectData` in 1,176 |
| project | `hkbProjectStringData` +48: array of 8-byte text pointers, the character files relative to the project's folder | 342 project files; 321 resolve a character whose behaviour file holds a graph; 13 name several characters, 2 none, 6 name a missing file |
| character | `hkbCharacterStringData` +184: a text pointer, the behaviour file relative to the project's folder | as above |
| graph | the file's one `hkbBehaviorGraph` | 450 behaviour graph files |

## Objects read

| Class | Offset | Content | Check (450 files) |
| --- | --- | --- | --- |
| `hkbBehaviorGraph` | +192 | link: the root generator | `hkbStateMachine` in all 450 read |
| | +200 | link: an `hkbBehaviorGraphData` | |
| `hkbBehaviorGraphData` | +104 | link: an `hkbBehaviorGraphStringData` | |
| `hkbBehaviorGraphStringData` | +16 | array of 8-byte text pointers: the graph's event names, by index | flag word `0x80000000 | count` in all |
| `hkbStateMachine` | +168 | i32: the start state id | among the machine's state ids in 449 of 450 root machines (2,074 of 2,075 machines of every depth, all 592 that have no state 0, in the exploration) |
| | +208 | array of 8-byte links, each to an `hkbStateMachineStateInfo` | all |
| | +224 | link: an `hkbStateMachineTransitionInfoArray`, the wildcard transitions, or none | |
| `hkbStateMachineStateInfo` | +64, +72 | links: enter and exit notification arrays, or none | |
| | +80 | link: its transitions, or none | |
| | +88 | link: the generator | |
| | +96 | text pointer: the state's name | |
| | +104 | i32: the state id | |
| `hkbStateMachineTransitionInfoArray` | +16 | array of 72-byte elements | storage fits in all 4,727 arrays |
| element | +32 | link: the transition effect | |
| | +40 | link: a condition, or none | a condition on 1 of 1,775 local transitions of the root machines |
| | +48 | i32: the event index | within the event table in 1,775 of 1,775 local and 954 of 954 wildcard transitions |
| | +52 | i32: the target state id | among the state ids in 1,775 of 1,775 |
| | +56..+72 | four words, kept as stored | **not read** |
| `BGSGamebryoSequenceGenerator` | +136 | text pointer: the NIF controller sequence it plays | a pointer in all 1,427 generators |

Root machines' states by generator class: `BGSGamebryoSequenceGenerator`
811, `hkbModifierGenerator` 459, `hkbStateMachine` 259, `hkbClipGenerator`
254, `hkbBlenderGenerator` 94, others 86; in 116 root machines every state
plays a Gamebryo sequence. Everything else of these classes (blend times,
flags, variables, the transition effects, nested machines) is **not read**.

## Errors

A missing graph, graph data or string data link, or a root generator that is
not a state machine, is a named error (`InvalidHavokBehaviorGraph`,
`UnsupportedBehaviorRoot`); an array whose flag word, storage or element
links do not fit is the view's named error, with the file and offset.

## Annotation tracks

The annotation tracks of the animation clips are in
[havok-animation.md](havok-animation.md) (Annotation tracks).

## Character graphs

A race's MODL texts after its NAM3 marker name a behaviour project file;
the project, character and graph files of 45 of the 46 such projects
resolve with the readers above, their root machines hold 127 states and
their event tables 15,845 names
([character-graph-census.txt](character-graph-census.txt), `char_graph_census`;
[vm.md](vm.md), Character behaviour graphs). A character's root machine
plays no Gamebryo sequence: its states run modifier generators and nested
machines, and which clip a state plays is held in classes these readers do
not read (clip generators, behaviour references, nested machines,
transition conditions and variables).

