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

## What it does not do

- Characters in the skeleton's rest pose, or with the character graph switches in the start state's clip
  ([character-graph.md](character-graph.md)): no transition is taken, nobody walks, sits or turns to look.
- The player's camera is not the game's: height and direction are the caller's.
- No sound; the line's text is shown, its audio is not played.
- The timeline is a file: the track is the caller's, not input-driven.
