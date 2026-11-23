# Run timeline

A quest run (`fo4_vm`, `quest_run new-game`) moves on its own real-time clock: the caller's player
track, actors at stored or assumed positions, references enabled, disabled and moved, scenes and their
spoken lines, stages and objectives. The renderers draw cells at a second of the game clock (`--time`).
The run timeline joins the two: the run writes what it did, second by second, in a neutral text file,
and the CPU render and the headless viewer draw "the run at second S" from it.

Code: `fo4_scene::timeline` (format, the state at a second, what a frame applies to a loaded scene, the
actors' presence, camera and overlay), `fo4_vm::timeline` (the writer), `fo4_render::timeline` (the
actors' placeholder markers), `crates/fo4-unpack/examples/render.rs` and `apps/fo4-view/src/timeline.rs`
(the two frame paths).

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
| An actor is present when its own last change is an enable, absent on a disable; else its enable chain with the run's changes; without a parent its initially-disabled flag | rule | `vm_disable_hides_reference_v1`, enable parent rules |
| Actors drawn as an upright magenta box (40 by 40, 128 high) with a yellow roof at their place, tagged after every other draw | presentation | none: not a character, not a pose, not the actor's size |
| Skinned actors are not drawn in a timeline frame | presentation | |
| Camera: eye H units above the track position (H the caller's), looking level along the track's current move, else its last move before S, else its next move after S, or along `--look-heading DEGREES` (counter-clockwise from +X, this format's convention) | caller assumption / presentation | |
| Overlay text beside the frame: second, place, camera, stages, objectives displayed and not completed, the line being spoken (else the last started), what was applied, markers with tags, actors left out, rules relied on | presentation | |

All the rules are pending: a frame that applies any change needs `--under-hypotheses` (refused with
`TimelineRuleLevel` otherwise). The run's second S has nothing to do with `--time` (seconds of the game
clock after a model loads); the two are given separately.

## What it does not do

- No character is drawn (markers only); no animation, no look direction of actors.
- The player's camera is not the game's: height and direction are the caller's.
- No sound; the line's text is shown, its audio is not played.
- The timeline is a file: the track is the caller's, not input-driven.
