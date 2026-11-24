# Run timeline

A quest run (`fo4_vm`, `quest_run new-game`) moves on its own real-time clock: the caller's player
track, actors at stored or assumed positions, references enabled, disabled and moved, scenes and their
spoken lines, stages and objectives. The renderers draw cells at a second of the game clock (`--time`).
The run timeline joins the two: the run writes what it did, second by second, in a neutral text file,
and the CPU render and the headless viewer draw "the run at second S" from it.

Code: `fo4_scene::timeline` (format, the state at a second, what a frame applies to a loaded scene, the
actors' presence, camera and its aim, overlay), `fo4_scene::timeline::cast` (the actors as characters),
`fo4_vm::timeline` (the writer), `fo4_render::timeline` (the actors' placeholder markers, the markers that
stand in for actors the cast cannot draw), `crates/fo4-unpack/examples/render.rs` and
`apps/fo4-view/src/timeline.rs` (the two frame paths), `crates/fo4-unpack/examples/timeline_cast_census.rs`
(the census of the actors' presence, [timeline-cast-census.txt](timeline-cast-census.txt)).

## Writing it

`quest_run MANIFEST new-game OUT_DIR ... --timeline FILE` writes FILE after the run. The writer
(`fo4_vm::timeline::Recorder`) only reads the run: it looks at the interpreter after each step of the
caller's clock loop (where the names file's `## timeline at` lines are written) and once at the end, and
keeps what the effect log, the quest log, the scene log and the world model's native moves grew by,
stamped with the run's present second. At the end it adds what the world model keeps with its own
seconds: the caller's track points, the actors' stored positions (from second 0) and the actor moves
applied (the caller's `--assume-actor-at` and the package destinations, each at its own second). Labels
(editor ids) and line texts are read from the store. Without `--timeline` nothing of this runs; with it
every other output of the run is the same byte for byte (checked on the forced opening: census, names
and next-blockers files, with and without the flag).

## Format, version 1

UTF-8 text, one record per line, fields separated by one tab. The first line is
`osp-run-timeline<TAB>1`; a reader refuses another first line or another version. In text fields a
backslash, tab, line feed and carriage return are written `\\`, `\t`, `\n`, `\r`. A record key is
`plugin:XXXXXXXX` (plugin name lower case, eight hexadecimal digits). Seconds and coordinates are decimal
numbers that read back to the same double. `-` stands for "none". Event lines are in non-decreasing
second order (a reader refuses time going back); events of one second keep the order the run made them.

| Kind | Fields after the kind | Meaning |
| --- | --- | --- |
| `note` | TEXT | What the writer states about its choices. |
| `label` | KEY, TEXT | The record's editor id, for people reading the file. |
| `player` | T, SPACE, CELL or `-`, X, Y, Z, FACING or `-` | A point of the caller's track; CELL is the interior cell, or the exterior cell of the worldspace whose grid square holds the point. |
| `actor` | T, REF, SOURCE, SPACE, X, Y, Z | The actor stands there from T; SOURCE names the rule or caller assumption (`world_actor_placed_position_v1`, `pack_location_destination_v1`, `world_moveto_target_v1`, `caller assumption --assume-actor-at`). |
| `actor` | T, REF, SOURCE, `-`, `-`, `-`, `-`, WHY | The actor's place is open from T (a native may have moved it). |
| `move` | as `actor` | A reference other than an actor the run moved. |
| `visibility` | T, REF, `enabled` or `disabled`, `nowait` or `plain` | The run's Enable or Disable of a placed reference; `nowait` for their NoWait forms. |
| `stage` | T, QUEST, N | A stage set. |
| `objective` | T, QUEST, N, `displayed` / `completed` / `failed`, `true` / `false` / `-` | An objective change. |
| `quest` | T, QUEST, TEXT | Another quest change (started, stopped, ...). |
| `scene` | T, SCENE, TEXT | A scene change other than a line (started, phase entered, action started or completed, ended, ...). |
| `line` | T, SCENE, PHASE, ACTION, SPEAKER or `-`, SECONDS or `-`, TOPIC or `-`, HOW, TEXT or `-` | A dialogue or player-dialogue action as the scene clock started it: its timed seconds, the speaker alias's reference in the run where the writer could read it (a run fill, the forced or location reference, the unique actor's one placed actor), how the seconds and the speaker were settled; TEXT only where the topic holds one line, joined responses, each response's string id found in exactly one of the plugin's three string tables. |
| `event` | T, TEXT | Anything else kept (an Enable on an object that is no placed record). |

## A frame at second S

`Timeline::at_with(S, readings)` gives, for every record, the last change at or before S: visibility,
moves, actor places, the last stage of each quest, objective values, the lines whose timed seconds cover
S and the last line started. The player's position follows the track as the run's track model does
(straight moves at constant speed between two points of one space, a jump at the second of a point in
another space).

What a frame does with it (`render ... --timeline FILE --at S --eye-height H`, `fo4-view ... --headless
--timeline FILE --at S --eye-height H`; CELL `-` draws the player's cell):

| Piece | Kind | Basis |
| --- | --- | --- |
| The player's cell: the interior cell, or the exterior cell whose grid square of 4,096 units holds the track position | rule | `world_cell_natives_v1` (the run's own reading) |
| A reference whose last change is a disable is not drawn; an initially disabled one whose last change is an enable is loaded and drawn | rule | `vm_disable_hides_reference_v1` |
| Enable children of changed references take the state their chain now gives | rule | `xesp_script_change_propagates_v1` with the enable parent rules |
| The NoWait forms change presence as the plain natives | rule, switch `--timeline-nowait` (off: counted, nothing changes) | `timeline_nowait_visibility_v1` (new) |
| A moved reference is drawn at the new position with its stored rotation and scale; one moved to another space or to an open place is left out | rule; leaving out is presentation | `timeline_move_stored_rotation_v1` (new), its position from `world_moveto_target_v1` |
| An actor is present when its own last change is an enable, absent on a disable; else its enable chain with the run's changes; without a parent its initially-disabled flag (`fo4_scene::timeline::presence`) | rule | `vm_disable_hides_reference_v1`, enable parent rules |
| An actor whose chain the rules leave open only because it (or a record below the root) carries its own flag 0x800, every such flag agreeing with the chain's start state, takes its chain's state, the run's changes included | rule, switch `--timeline-child-flag-agrees` (off: undecided, left out) | `xesp_child_flag_agrees_chain_v1` (new) |
| Actors drawn as an upright magenta box (40 by 40, 128 high) with a yellow roof at their place, tagged after every other draw | presentation | none: not a character, not a pose, not the actor's size |
| Without `--timeline-cast` skinned actors are not drawn in a timeline frame (the cell's own placed actors never are) | presentation | |
| With `--timeline-cast` each present actor is resolved as a placed actor is (`fo4_scene::actor`: skeleton, addon models, head parts) and drawn skinned by `build_with_actors`, after the placed references, at the run's place | switch; the actor rules as for any placed actor | `achr_placement_as_refr_v1` and the other actor rules |
| The cast's actor takes the frame's presence: its chain's start state and the chain's open parts no longer leave it out | rule | the presence rules above |
| An actor at its stored place faces as its placement stores; one the run moved keeps its stored rotation | rule | `timeline_actor_stored_rotation_v1` (new) |
| With `--timeline-target-rotation` an actor the run moved faces as the one placed reference of the loaded cells standing exactly at its new place; none or several: stored rotation, said in the overlay | rule, switch (off) | `timeline_actor_target_rotation_v1` (new) |
| With `--actor-missing-bone-bind` a bone a skinned shape names that the skeleton lacks is skinned at its bind transform instead of leaving the actor out | rule, switch (off) | `actor_missing_bone_bind_pose_v1` (new) |
| The skeleton's rest pose, no animation | presentation | none: the run gives no clip |
| An actor the cast cannot draw (no plan, blocked state, a build refusal) is drawn as its marker, tagged after every other draw and every instance, with the reason in the overlay (`marker instead`) | presentation | |
| Camera: eye H units above the track position (H the caller's), looking level along the track's current move, else its last move before S, else its next move after S, or along `--look-heading DEGREES` (counter-clockwise from +X, this format's convention) | caller assumption / presentation | |
| Camera aim (`--look-at X,Y,Z`, `--look-at-actor nearest\|speaker`; `fo4_scene::timeline::Aim`, `camera`): from the same eye, at the caller's point, or H units above the place of the drawn actor (character or marker) nearest the eye, or of the speaker of the line being spoken (else of the last line started) where it is drawn, else the nearest, saying why; no drawn actor: along the track, saying so. An actor less than one unit from the eye horizontally is passed over | presentation only: where the camera looks is the caller's choice, not the player's view | none |
| Overlay text beside the frame: second, place, camera, stages, objectives displayed and not completed, the line being spoken (else the last started), what was applied, markers with tags, actors left out, rules relied on | presentation | |

All the rules are pending: a frame that applies any change needs `--under-hypotheses` (refused with
`TimelineRuleLevel` otherwise), and so does `--timeline-cast`. `--timeline-target-rotation` and
`--actor-missing-bone-bind` need `--timeline-cast`; every new flag needs `--timeline`. The run's second S has nothing to do with `--time` (seconds of the game
clock after a model loads); the two are given separately.

## The cast (`--timeline-cast`)

With `--timeline-cast` (render and fo4-view, at `--under-hypotheses`: `TimelineRuleLevel` otherwise) the
actors a frame marks are drawn as their characters: `fo4_scene::timeline::cast::resolve` resolves each
marker's reference with `fo4_scene::actor::resolve_reference` (its record's source span for the errors),
places it at the run's position with the facing above, and `build_with_actors` skins it in the skeleton's
rest pose. Its instance tags follow the placed references (so terrain tags move after them); a marker for an
actor the build could not draw is tagged from `fo4_render::timeline::first_free_tag` (after every draw and
every instance). The overlay adds `cast` (how many as characters and as markers), `character` (tag, place,
source, facing and whether a bone was skinned at its bind transform), `marker instead` (tag, place, source and
the reason: the actor's notes, its blocked state's open codes or the build's refusal) and `head left out`
lines; `summary.json` a `cast` key. Without the flag nothing of this runs and every output is as before.

Behind the character graph switches (`--actor-start-clip`, `--actor-subgraph-keywords`, `--actor-clip-folder`,
`--actor-start-clip-loop`; [character-graph.md](character-graph.md)) each character the build can draw is
posed by the clip its race's behaviour graph reaches from its root, at the clip generator's start time or,
with `--actor-start-clip-loop`, at the frame's second; an actor whose walk or clip is refused keeps the rest
pose. The `character` lines then end with how the actor is posed or why not, the `cast` line counts the posed
actors and `summary.json`'s `cast.pose` becomes an object; without the switches both are as before. The
opening, all four on with the cast and light switches: 79 of the 84 resolved actors reach a clip (7
distinct idles); on the contact sheet's 13 seconds every character drawn (3 to 37 a frame) is posed; the 5 refused
actors (a base with a template, the vertibird's graph) are not among those drawn there.

The opening (the playview step's timeline, the contact sheet's 13 seconds, every new switch and
`--timeline-nowait` on): 63 distinct actors drawn as characters at one second or another, 13 as markers
(9 with a texture path the archives refuse and a missing texture, 2 with effect-shader controllers run by a
manager, 1 with a transform controller, 1 with no skin and no outfit); 28 of the characters have a bone their
skeleton lacks (cloth and hair) skinned at its bind transform. Per frame: 3 characters and 1 marker in the
house (before: 4 markers), 35 to 37 characters and 3 to 6 markers outside after the alarm (before: 4 markers),
23 characters and 6 markers in the vault (before: 24 markers).

### The cast in the play window (the looks step)

The window's scripted play mode (`fo4-view --play-run ... --play-input FILE --timeline-cast`, with the cast
readings above: the start clip flags, `--timeline-child-flag-agrees`, the look, death and sit readings) draws the
same cast from the run's own timeline as it grows instead of a file: the run's driver reads it
(`fo4_play::View::timeline`, which changes nothing in the run) when the run's effects, quest or scene logs or
native moves grew, at most every 5 run seconds; a thread that keeps the game data open builds the drawn cell's
cast at the run's second (`timeline::cast_scene` in the viewer: `markers`, `cast::resolve`, `cast::start_poses`,
the build without the cell's placed references, `fo4_render::timeline::cast_markers`) each time the count of the
timeline's actor, visibility, death and sit events up to that second changes, and the window replaces the cast
it draws. `fo4_scene::timeline::apply` is not called: the live world shows, hides and moves the cell's
references. The cell's own placed actors are not drawn then. Presentation and plumbing only: no new rule. In the
opening: 51 builds, 3.4 s on average (8.2 s at most) on their thread; 35 characters on the hill and 23 in the
prewar vault drawn posed by their start clip; a moving actor stands where the last build put it.

## Presence census

[timeline-cast-census.txt](timeline-cast-census.txt) (`timeline_cast_census MANIFEST TIMELINE OUT NAMES
[--at S,...]`; names only in the unversioned NAMES file) classes the 93 actors the forced opening's timeline
places by how a frame decides their presence. 44 are undecided, all for one reason: the actor carries its own
flag 0x800 below an enable parent (`XESP.own_flag`), a chain of depth 1 whose link has the opposite bit and
whose root is a placed static without the flag (seven distinct parents, the run enables or disables every one
of them, all with the NoWait forms). In all 44 the flag agrees with the chain's start state (both start the
actor disabled), which is what `xesp_child_flag_agrees_chain_v1` reads. With it and `--timeline-nowait` the
44 are decided: absent before the alarm, present once the run disables their parent (38 of them outside at
420 s, the 5 moved inside the vault at 560 s), and absent again where the run enables it again (6 at 420 s). Neither reading alone
decides them: the run changes their parents only with the NoWait forms.

## The elevator second

At 505 s the caller's track rides straight down from z 1,506 to z -121 over 10.5 s in the vault cell, and the
eye leaves the geometry. The data holds a moving reference there: an activator about 600 units from the track
whose model has a behaviour graph and NIF controller sequences of 2 to 24 seconds (the platform's
descent among them, by their lengths), skinned and refused by the draw (`UnsupportedSkinning`,
`UnsupportedAnimationRuntime`). Outside, the lift is a static with a behaviour graph and no NIF sequence. The
timeline records no event that starts either. Drawing the ride from the data would need: the run raising the
animation event that starts the sequence (a scene action or a script call, recorded in the timeline), the
graph's choice of sequence (the classes the readers do not read yet), a sequence player over the controller
sequence's transform interpolators (`clock::transform_track` evaluates their keys), skinned non-actor models,
and the player standing on the moving node (the track following the platform's height). None of this is
done; the frame is left as it was and no path is invented.

What the seen step found (census section 3): the outside lift is not a static but the shaft model of the
activator above (its 9 controller sequences move a platform node, two hatches and lights; 13 collision objects,
the platform's under the moving node), placed 6,600 units below the platform's top; the static with a behaviour
graph holds the walkway's collision. The run plays the shaft's graph (its names file records the sequences'
text keys and states: the state that plays the 18.667 s descent is entered at 481.567 s in the museum step's
furthest run), but the timeline does not record it, so a frame cannot know when the platform moves. The
platform's collision rides in the walking world (`spec/walk.md`, Platforms) with the second given by its
caller; the shaft model is still refused by the draw (skinning, animation runtime), so no frame draws the ride.
For the timeline to carry it, `fo4-vm`'s writer would add an `event` line per sequence a placed reference's
graph starts, with a fixed grammar: `sequence REF NAME` (REF a record key, NAME the sequence's name as the
model stores it), read by this reader as the death and sit lines are.

### Drawing the shaft and the ride (the faces step)

The draw refuses the shaft model for several reasons at once (census section 4, `faces_census --shaft`): its five
skinned shapes (`UnsupportedSkinning` on each `BSTriShape`, `UnsupportedNifBlock` on each `BSSkin::Instance`: the
platform, the hatches and the lights are skinned to the model's own nodes), the controllers its graph and its
manager run that no rest reading settles (effect-shader controllers run by the manager, visibility controllers, the
behaviour graph's extra data), and record fields not read (ACTI PNAM and FNAM, REFR XLRT). The skinned shapes are
the visible parts: the model's other shapes are a sky dome cap and the platform's dirt; its walls are collision
only. Skinned models are not rare among placed records that are not actors: of the 27,193 models of the load
order's placed kinds read, 322 hold skinned shapes (activators 106, containers 84, movable statics 76, statics 55,
furniture 33 and others; 245 also run by a controller manager, 149 by a behaviour graph; census section 5), and
the coverage census counts 21,285 placements refused for `BSSkin::Instance` (`spec/coverage-census.txt`).

Two readings behind switches of the CPU render and the headless viewer (with `--timeline-cast`), off by default
(`fo4_scene::timeline::ride`):

| Switch | Rule | Reading |
| --- | --- | --- |
| `--timeline-ride-model PLUGIN:HEX` | `model_skinned_own_nodes_v1` | the named reference, when it is placed in the frame's space (in a worldspace, inside the horizontal box of the loaded cells' placed references) and the run's frame does not have it absent, is drawn by its skinned shapes, each skinned as an actor's shape is (`actor_skin_linear_blend_v1`) with the model itself as the skeleton, its nodes at their stored transforms; its rigid shapes, controllers and material swap are left out; drawn as one more actor of the cast (after the markers' actors), with an overlay `ride` line |
| `--timeline-ride SEQUENCE,START` | `timeline_ride_sequence_nodes_v1` | the nodes the model's sequence number SEQUENCE translates take its translation keys at `S - START` (clamped to the sequence; before START where it starts), START being the run's second the caller gives (the run knows it, the timeline does not record it yet) |

On the museum step's furthest run with sequence 8 from 481.567 s (the descent, the second the run's names file
gives for its graph state): at 416 s, looking from the track at the platform's box, the platform, its rails and
the hatches stand at the top of the shaft where the walking world's collision-marker box is (before: a hole with a
character standing over it); at 490 s, 8.4 s into the descent, the platform has gone 1,061 units down the shaft
and its rim stands at the eye (120 units above the track), while the caller's track has gone 1,138 units down: the
track and the sequence's keys differ by 77 units there (the keys do not move at a constant speed). The rest pose of the stored nodes would put the platform at the shaft's bottom (its node is
stored there): before the ride the sequence's start is used. A placeholder of how the model looks: its lights'
glow, its swap of materials and its moving lights are not drawn.

### Every skinned placement and the shaft's parts (the poses step)

What the skinned models of placed records other than actors are (census `poses_census --models`,
[poses-census.txt](poses-census.txt), section 3; read from each model alone): 322 models of the placed kinds hold
skinned shapes, used by 21,341 placed references (REFR) over the load order. By what moves their bones: a controller
manager every sequence of which moves a skin bone, without a behaviour graph, 124 models (14,232 placements, the
largest role; by a single base kind over all 322: activators 87, containers 73, movable statics 52, furniture 31, the rest fewer or of several kinds)
and with one 104 (1,106); a manager whose sequences move no bone 16 (157); some of its sequences 1 (157); a graph
without a manager 29 (411); nothing in the model 48 (5,278). No model holds a physics constraint or ragdoll block. Most have one to five skinned shapes on two to five
bones (section 3 by size).

`--skinned-models-own-nodes` (`model_skinned_own_nodes_every_placement_v1`, with `--timeline-cast`; render and
headless viewer; `fo4_scene::timeline::ride::add_skinned`) generalises `model_skinned_own_nodes_v1` to every placed
reference of a frame's loaded cells that the draw refuses with a skinned shape among its reasons: each is added to
the cast, after the ride, as one more actor drawn by its skinned shapes on its own nodes at their stored transforms
(rigid shapes, controllers, material swap left out; its other refusals not read), with an overlay line
`skinned models` (refused, added, drawn, the build's reasons for the rest). On the six sheet seconds, viewer,
default texture caps, with the seen and faces steps' switches:

| Second | Placements refused with a skinned shape (before: none drawn) | Drawn with the switch | Left out by the build |
| --- | ---: | ---: | --- |
| 17 | 12 | 12 | |
| 418 | 6 | 1 | 5 editor-marker subtrees (`UnverifiedActorModelMarker`) |
| 560 | 8 | 8 | |
| 1345 | 54 | 51 | 3 editor-marker subtrees |
| 1470 | 53 | 48 | 4 editor-marker subtrees, 1 shape with a vertex of zero weights |
| 1650 | 64 | 62 | 2 editor-marker subtrees |

A partial reading by design: the same references' rigid shapes stay undrawn, and the about 16,000 placements whose
manager or graph can move the bones are drawn where the stored nodes put them.

The shaft model's parts the ride reading leaves out (section 4, by block): of its seven shapes, the five skinned
ones (lighting shader, one with a lighting-shader float controller) are what `--timeline-ride-model` draws; the two
rigid ones are alpha-blended effect-shader shapes whose effect fields (one colour and three float controllers) its
manager's sequences run (`effect-idle`): rigid shapes are outside the ride reading and their effect fields are not
settled. The model holds no light block (what shines is the skinned shapes' geometry; placed lights are LIGH
references, drawn under the light rules); one visibility controller, whose target's visibility over time is not
read; nine sequences (one looping, eight clamped) driving bool, float, point and transform interpolators; the graph
extra data that starts them; a material swap on the reference (XMSP) and the base (MODS), which the ride reading does
not apply. None of these is a pure format reading (each needs a controller, sequence or swap reading), so none was
drawn.

## Deaths and furniture (`--timeline-deaths`, `--timeline-sits`)

Two switches of `quest_run new-game --timeline FILE`, off by default (with them off the file is byte for byte
what it was; with them on every other output of the run is unchanged, checked on the museum step's run), add `event` lines (format version 1, the `event` kind's TEXT field) with a fixed grammar of
space-separated words:

| TEXT | Switch | Meaning |
| --- | --- | --- |
| `death REF` | `--timeline-deaths` | From T the run has REF dead: the run raised its death (`fight_death_events_v1`, the caller's `--assume-death`). REF is a record key, `-` when the dead object is no record. |
| `sit WHO FURNITURE` | `--timeline-sits` | From T the run has WHO sitting in the furniture reference FURNITURE (WHO a record key or `player`): the run's sitters (`crate::sit`: the caller's `--assume-sit`, a script's activation of a furniture read as a sit under the sit step's rules). |
| `get-up WHO FURNITURE` | `--timeline-sits` | From T WHO no longer sits in FURNITURE. |

The writer looks at the run's dead and sitters after each step of the caller's clock loop, as it does for the
other logs, and stamps what changed with the run's present second. A death is never undone.

### Reading them in a frame (the seen step)

The reader keeps them as `event` lines (the format is unchanged) and a frame collects them
(`fo4_scene::timeline::body_note`, `Frame::dead`, `Frame::sitting`: who is dead from when, who sits in which
furniture from when, a `get-up` ending only its own `sit`), whatever the readings. What a frame does with them,
each behind its own switch of the CPU render and the headless viewer, off by default (with every switch off
every marker, character, overlay and frame is as before: `spec/seen-census.txt`'s step checked the earlier
frame sets byte for byte):

| Switch | Rule | Reading |
| --- | --- | --- |
| `--timeline-dead-left-out` | `timeline_dead_actor_left_out_v1` | a dead actor is not drawn (character or marker); the overlay lists it among the actors left out with its death's second |
| `--timeline-dead-lying` | `timeline_dead_actor_lying_placeholder_v1` | a dead actor is drawn lying at its last place: its character (in whatever pose the other switches give) or its marker box turned a quarter turn about the world's X axis after its stored rotation about the vertical (`lying_rotation`); its root stays at the place's height, so half of a lying marker box is under the floor. A placeholder |
| `--timeline-sit-marker` | `timeline_sit_marker_nearest_v1`, `furniture_marker_snam_v1` | an actor sitting in a placed furniture stands at the furniture's marker nearest its place (`fo4_scene::furniture::seat_near`: the base's SNAM entries as offset and heading, through the furniture placement's transform), facing the placement's rotation about the vertical plus the marker's heading. The player's sit adds one overlay line (`sitting`); the camera stays on the track |
| `--actor-sit-clip` (with `--actor-start-clip`) | `char_graph_furniture_entry_keywords_v1` | a seated actor is posed by the clip its race's subgraph entry of flag word 2 for the furniture's keywords reaches from its root (`fo4_scene::char_graph::furniture_clip`); refused, it keeps its start clip or rest pose with the reason |

What the data offers (census `seen_census`, [seen-census.txt](seen-census.txt), sections 1 and 2):

- A dead body: the human race's root behaviour graph holds 58 clip generators, 47 of them with the word for
  death in their clip or name, reached through events (35 of the graph's event names hold the word) and
  selectors the run does not set; ragdoll modifiers in the graph and ragdoll data in the skeleton's Havok file
  beside its model (one ragdoll object and 9 or 20 constraints). Which clip, or whether the ragdoll takes over,
  is the engine's choice on an event: nothing chooses it from the data alone. The insects' race offers no death
  clip. Hence a placeholder or leaving the body out, never a clip.
- A seat: FURN SNAM holds one entry of 20 or 24 bytes per marker in all 782 furniture records; 17 of the 579
  furniture and marker models hold a furniture marker block (not read). The run seats only the player (six
  sits in the museum step's furthest run: a sink, a crib, three cryopods, a console); their markers sit at the
  furniture's origin or 63 units in front of it with a heading of minus pi; the nearest marker lies 30 to 84
  units from the track's point at the sit's second (2,677 for a cryopod the track has left). No subgraph entry of
  flag word 2 fits these six furniture records for the player's race: the sit clip is exercised by synthetic
  tests only. Two of the player's six sits span seconds when the track walks elsewhere (0 to 190 s and 190 to
  700 s): the run's sit state is not ended by its walk, which is why the camera stays on the track.

What the run does not know: who sits or lies through a package (the scenes' and aliases' sit and sandbox
packages are not run; a package action completes under the caller's `--assume-packages-complete` without
seating anyone; `--package-destinations` only moves an actor to a placed location), which of a furniture's markers an actor takes, and when an actor an alias's package
moves gets up. In the museum step's furthest run the run's sitters are the player's alone: six sits and five
get-ups in the prewar house and the postwar vault (the exit step's readings of furniture activation), the last
sit (the vault's console, 870 s) never ended by the run; the people in the museum are drawn where their records
place them, and the thirteen deaths (the first fight's six insects, the second fight's seven raiders) appear as
`death` lines.

## What it does not do

- Characters in the skeleton's rest pose, or with the character graph switches in the start state's clip
  ([character-graph.md](character-graph.md)): no transition is taken, nobody walks or turns to look; a dead
  actor lies only as a placeholder and a sitter sits only where the run says it sits (no package seats anyone).
- The player's camera is not the game's: height and direction are the caller's.
- No sound; the line's text is shown, its audio is not played.
- The timeline is a file: the track is the caller's, not input-driven.
