# The play mode: a quest run driven by input

`quest_run new-game --track FILE` runs the scripts along a track its caller wrote. The play mode
(`crates/fo4-play`) gives the run its track slice by slice instead: the walker (`fo4-walk`) moves over the
start cell's placed collision, driven by a scripted input (`play_run`, headless) or by a window's keyboard and
mouse (`fo4-view --play-run`), and the player's activations come from what the eye's ray meets. Nothing here was
checked against the running game: every behaviour reading is a pending rule of `hypotheses.tsv`.

## How the run is stepped

The run is `fo4-vm`'s library run (`fo4_vm::run`, the code of every `quest_run` mode): the command line read
into a `Command` (`Options`, `Then`, `Settings`; `fo4_vm::run::parse`), the new-game setup (the host and the
world model from the switches), and the clock loop as an object, `fo4_vm::run::NewGame`. Each turn of the
loop takes the earliest of the next timer, the next scene clock due, the next timed caller action and the
world model's next due second (the next crossing, a distance registration's first second on the track, an
assumed actor move, a hello crossing, a line half, an animation or load due), moves the run's clock there and
runs what is due. `quest_run new-game` runs the loop to its end at once (`NewGame::run_to_end`) over the
whole track the caller gave: `fo4_vm::track::Place::load_with` computes every trigger crossing of the track
at load, `fo4_vm::talk::load_hello` every hello crossing. The `quest_run` example is a thin caller
(`fo4_vm::run::main_with`); the voice files' seconds come from the caller (`HeaderSeconds`), which reads them
with `fo4-audio`.

Two ways to feed the run input were possible:

- Re-run `quest_run` with the track walked so far. Measured on the opening's first minute (the furthest run's
  switches with the track cut at 60 s): 43 to 86 s per run on this machine before this step's caches. A
  window needs the run's state every frame: re-running is not usable.
- Step the run: the same loop with a horizon. This is what `fo4-play` does.

The stepped run (`fo4_vm::run::NewGame`, driven by `fo4-play`'s `Driver` through a `fo4_vm::run::Stepper`;
`quest_run`'s own new-game code, no copy):

- `begin_input` cuts the caller's track to the run's present second (second 0): its first point is the
  start; its points chose the cells the world model read (as without play); its later points are replaced
  by the driver's (`Place::cut`, `Talk::cut`). The loop then runs what is due at the present second.
- Each turn computes the same candidates as `quest_run`'s loop. When the earliest lies after the horizon
  (the second the last slice reached), or nothing is due, the loop returns and the driver is asked for the
  next slice: track points (none while the player stands still), the second `until` it reaches, and the
  player's actions. `advance_to(until, points, actions)` gives them to the run; the player stands at the
  last point until `until`.
- New points are folded into the crossing lists move by move, exactly as `fo4_vm::track::intervals` folds the
  whole track (`Place::extend`, `Talk::extend`; a unit test checks every prefix of a synthetic track, and a
  cut track grown in slices, against the whole track's crossings and hello crossings); the crossings already
  applied must not change (counted: 0 in every run below).
- An action of a slice joins the caller's timed actions before those of the same second (the input first on
  a tie).
- `finish_input` ends the input: the loop goes on as `quest_run`'s does after the track's end. `stop` (a
  window closing) ends the run where it is.

The one lookahead: with `--cond-1-distance` (`cond_fn_1_track_distance_v1`) a scene phase that starts waiting
on completion conditions with a function 1 condition gets a mark at the first second the whole track brings
the two parties within the distance. The stepped run knows only the track to the horizon then. So when the
track grows, each scene waiting on false completion conditions with a function 1 condition and no mark left
is, by default, offered its completion again (`fo4_vm::scene::step_call`), which reads its marks on the
longer track; an offer that finds the conditions still false logs one more wait than `quest_run` does. With
`--play-recheck-marks` (`fo4_play::Options::recheck_marks`, `fo4_vm::run::Stepping::recheck_marks`; a policy
of the play mode, no game claim) the marks of those scenes are computed again on the longer track
(`fo4_vm::scene::recheck_distance_marks`) and nothing is offered, so no wait is logged that `quest_run` does
not log (checked below).

### What `fo4-vm` gives the play mode

The request the earlier steps wrote here is done: the play mode no longer compiles `quest_run`'s source
(the patching `build.rs` is gone) and calls only the library:

1. `fo4_vm::run::NewGame`, built from the parsed switches inside `fo4_vm::run::main_with` (a run object
   borrows the host, the library, the attachments and the runner that the run's setup owns, so a caller
   steps it from a `Stepper` instead of owning it), with `begin_input(Stepping)`, `advance_to(until, points,
   actions)` (actions as `(second, fo4_vm::player::Action, FormKey)`), `finish_input`, `stop`, read access
   to the run (`vm`, `store`, `sources`, `now`, `loaded_track`, `timeline_now`) and the input counts
   (`take_input_counts`, the play mode's `play.txt`). `quest_run new-game` is `run_to_end`, the loop over the
   whole track; it is not `advance_to(f64::INFINITY, ...)`, which would cut and fold the track (the same
   crossings, checked on the furthest run as one slice by the earlier step, but not the same census line of
   the track's points).
2. `track::Place::cut`, `Place::extend`, `Place::add_volumes`, `Place::rebuild` and `talk::Talk::cut`,
   `Talk::extend`, `Talk::add_actor`, `Talk::rebuild` (the hello crossings), with `track::Folded`,
   `track::step_of`, `track::Region` (the cells read about points, for `--play-region-walker`).
3. `scene::recheck_distance_marks(host, world)`: the function 1 marks of the scenes waiting on completion
   conditions, computed again when the track grew, without logging a new wait.
4. The cache of stored positions: placed actors' stored positions are read once per run, and the
   placements' cells are indexed once (`track::Cells`); before, each question searched every placement.

## Checks of the stepped loop (outputs outside git)

The run's own track given back in slices (`play_run --replay STEP`, `fo4_play::Replay`, the track as the run
loaded it, with the points `--load-doors` adds):

| Run | Slices | Census | Names | Next blockers (2 files) | Timeline |
| --- | --- | --- | --- | --- | --- |
| first minute (furthest run's switches, track cut at 60 s), 0.1 s | 5 | SAME | SAME | SAME | SAME |
| furthest run (`spec/quest-run-home.txt`), whole track in one slice | 1 | SAME | SAME | SAME | SAME |
| furthest run, 0.05 s | 171 | 2 lines | 8 lines | SAME | 2 lines |
| furthest run, 0.05 s, `--play-recheck-marks` | 171 | SAME | SAME | SAME | SAME |

In the third row the differences are the two re-offers above (a greeting scene in the postwar vault waiting on a
distance condition, offered again at 567.125 s and 573.792 s): two more waits in the scene log; its phase then
completes at the same second as in `quest_run` (584.069 s), and every stage of the run (288) is set at the same
second. With `--play-recheck-marks` (run library step) the scene's mark is computed again once on the grown
track instead, and the stepped run writes `quest_run`'s files byte for byte (against `quest_run` on main's
binaries; the census wording of the earlier step's files has changed since).

## The walker as input (`play_run`)

`play_run MANIFEST INPUT OUT_DIR [options] -- [new-game switches]` builds the walking world of the start
cell (`--play-around N` cells about it, default 1, with their terrain), puts the walker at the run's first
point and settles it on the ground (that point is given to the run at second 0), and runs the scripted input
(`fo4_play::input`): one command per line, `SECONDS forward|back|left|right|run on|off`, `face|turn|pitch
DEGREES`, `activate`, `end`. The walker steps in its fixed tick (1/60 s by default, `--play-tick`); a command
between ticks cuts the tick at its second, so the input's seconds are kept exactly; commands due at a slice's
end are applied there, an activation reaching the run in the slice that ends at its second. A tick that moves
the feet less than a micro-unit gives no point (`walker::STILL`, numerical). Speeds are `fo4-walk`'s
provisional rules unless the caller gives `--walk-speed`/`--run-speed`. Outputs: `quest_run`'s files, the
timeline (`--timeline`), `play.txt` (the walker's log and the play counts) and `play-track.txt` (the run's
track, readable by `--track`).

### The opening's first minute walked

The caller's track of the first minute (6 points: standing at the start, a walk of four straight moves at
about 300 units per second between 17.614 s and 19 s) was turned into a scripted input (heading set at each
move's start, forward held, `--walk-speed 300`). Against `quest_run` with that track and the same switches:

- the same 160 stages in the same order over the whole run (138 in the first minute);
- 150 at the same second; 10 later by 0.014383 s: the trigger the walk crosses (stage 30 at 18.864 s in
  `quest_run`, 18.878 s walked) and the stages timed after it;
- why not exact: the walker's capsule (radius 16, provisional) slides along collision near the third move,
  ending that move 5.8 units short of the caller's point, and its feet stand 1 unit lower than the caller's
  navmesh point; the trigger is then crossed 14 ms later.

`quest_run` given the walked track (`play-track.txt`, 89 points) writes the same names, next-blocker files and
timeline as the play run, byte for byte; the census differs in one line (the count of the caller's track
points, 6 against 89).

### Activation

Over the first two minutes with the caller's activation of the front door at 90 s left out and `activate` in
the input at 90 s (heading 10.07, pitch -15, `--play-reach 150`, the caller's number):

- the door has no placed collision under this project's collision rules (`CollisionNotReady`), so the
  collision ray (`play_activate_collision_ray_v1`) met nothing within reach;
- with `--play-pick-bounds` (`play_activate_object_bounds_v1`) the ray met the door's object bounds at 99.1
  units and the run took the activation: the same 160 stages in the same order as with the caller's
  `--assume-activate`, the same seconds but for the 14 ms of the walk above.

The walker reached the door 21 units short of the caller's point (blocked twice near the door).

## The window (`fo4-view --play-run`)

With `--walk --under-hypotheses` (not the moving grid), `--play-run FILE` (the new-game switches, one per
line) and `--play-out DIR`, the viewer puts the walker at the run's first point, starts the run on its own
thread (`fo4_play::link`) and each frame sends it the frame's seconds and the walker's feet
(`fo4_play::link::Frames`, the same slices as `play_run`'s walker), and with E what the eye's ray meets within
`--play-reach` (with `--play-pick-bounds` the object bounds too); the title bar shows the run's second, the last
stage and the subtitles of the lines being spoken (from the run's timeline, so `--timeline` must be among the
switches for subtitles). The run's outputs are written when the window closes. Not exercised: no one could
sit at the machine and no window was opened; the pieces are tested without a window (`Frames`, the title, the
pick, the walker) and the run's path is `play_run`'s.

## The live world (`--play-live-world`)

With `play_live_collision_follows_run_v1` (`play_run --play-live-world`, `fo4-view --play-live-world`; off by
default) the walking world follows the run's state at each slice the walker gives, as a timeline frame at
that second follows it (`fo4_scene::timeline::apply`, whose readings are reused here: a reference whose last
change is a disable is gone, an initially disabled one whose last change is an enable is there, an instance
whose enable parent chain names a changed reference takes the state the chain now gives, a moved reference
stands at its new place with its stored rotation and scale, and one moved to another space or to an open place
is gone; the NoWait forms count only with `--play-nowait`, as `--timeline-nowait` for frames).

`fo4_play::live`: the run's state is read from the run as the timeline's writer reads it (the `Enable` and
`Disable` natives and their NoWait forms on placed records, the world model's native moves; `RunState`), and
decided per loaded reference (`decide`). The walking world is a `WorldSet` of pieces: the world `play_run`
always built (unchanged), the collision of the references the run may enable (the cell's initially disabled
references, loaded as any placed reference, and the instances an enable parent disables at load, built as if
enabled), hidden at the start, and one piece per moved reference (its primitives copied moved by the move's
translation, `World::copy_sources`). A reference that leaves has its sources hidden (`WorldSet::set_hidden`);
nothing else is rebuilt. The activation ray meets what the walking world then holds, and passes through the
object bounds of absent references. The decisions are taken only when the run's state changed, at the
start of a slice: within a slice of `--play-slice` seconds the walking world lags the run by that much at most.

Cost, measured on the opening's walk (`o4`, below, 480 s of input): 26 applies of the run's state, 2
references entered, 7 left, none moved; 7.9 ms for all of them, the longest 0.57 ms. The walker's ticks cost
11.8 microseconds each on average with the live world (7,965 ticks, 0.094 s in all); the dormant piece is 10
sources (27 initially disabled references loaded, 14 not loaded because they are actors, 66 instances an
enable parent disables, most without placed collision). Building the walking world with the dormant piece:
about 9 to 12 s for the opening's start cell and the cells about it, once. The live world is not what makes the
run slow; the run itself is (Costs, below).

What the opening's run changes near the walker did not change the walk: the same input with the live world
off gives the same walker track (`o4off`, below); the mechanism is checked on synthetic worlds
(`live::tests`: a disabled wall passed, an enabled one met, a moved one met where it was moved, a chain child
following its parent, the NoWait reading, the window's mirror colliding as the live world).

The window (`fo4-view --play-run ... --play-live-world`): the references the run may enable are drawn too
(`play::draw_dormant`, taken out of the scene again before the walking world is built), and each frame in
which the live world changed, every drawn instance of a reference it knows is hidden where the reference is
absent and moved by the move's translation where it moved; the walking world mirrors the live world
(`Live::mirror_into`). Not exercised: no window was opened.

## Doors

What the data holds about the placed doors along the opening's walk (`door_census`,
[door-census.txt](door-census.txt)): the start cell and the cells about it hold 12 placed doors, the prewar
vault 21 (one more initially disabled), the postwar vault 38; none of their collision is placed under this
project's rules (every one `CollisionNotReady`: their models' collision sits under an animation runtime,
`UnsupportedAnimationRuntime`, and most carry runtime record state, `RuntimeRecordState`), so no door blocks
the walker, open or closed. About half carry an empty `ONAM` subrecord (6 of 12, 3 of 21, 9 of 38); reading it
as "open at load" would come from a field's published name, not from the data, and it would change nothing
here since the doors have no collision and are not drawn (their readiness is blocked). The door records' `FNAM`
holds one byte (00, 10, 40, 42 seen). **No plain door reading is implemented**: the data as this project reads
it gives a closed door no collision to remove and an open door no motion to apply; the door's sequences (how
it swings) are behind the same animation runtime. The opening's front door is a plain door (no `XTEL`).

Load doors: 1 in the prewar vault, 2 in the postwar vault, none among the grid cells' references outside; a
worldspace keeps its load doors among its persistent references, so `fo4_play::world` also reads those within
the cells about the walking world and one more (`persistent_load_doors`), with their object bounds for the
activation. With `play_load_door_activation_v1` (`--play-load-doors`) an activation that meets a load door
gives the run the activation, then puts the walker at the `XTEL`'s position in the space of the door it names
(read as `world_load_door_xtel_v1` reads it), builds that space's walking world (the interior cell, or the
exterior cell under the position and the cells about it) and gives the run a point there at the same second;
the facing the `XTEL` stores is not applied.

Checked on a load door of the postwar exterior (`d1`; a one-point track beside it, the opening's other
switches): at 1 s the activation met the door's object bounds at 79.5 units; the walker stood in the
interior cell the door names at the stored position, settled 2.5 units below it on the floor; the interior's
walking world (3,295 triangles, 71 hulls from 143 instances) was built in 1.8 s; the walker then walked 5 s
inside (100 ticks blocked by walls); the run's track and timeline carry the change of space at 1 s. The
opening's own walk meets no load door it could reach: the vault's are at the top and bottom of the elevator
shafts.

## Where the run reads triggers and actors (`--play-region-walker`)

The run reads trigger volumes and placed actors about the caller's `--track` at load. With
`play_region_around_walker_v1` (`--play-region-walker`) the stepped loop also reads them about each walker
point whose cells it has not read (`Place::load_with` about that one point: its interior cell, or the exterior
cells within one grid square), adds the volumes and actors it does not hold (after those it holds), folds the
whole track so far into their crossings and, with hello lines, the new actors' hello crossings, and rebuilds
the crossing lists. With a one-point track (only the start) this is what gives the run the triggers of the
walk; after a load door it reads the new space.

Checked: the opening walked (`o4r`, the input and switches of `o4` below) with a track of the start point
only: the run read 42 trigger volumes and 48 placed actors at load (114 and 116 about the caller's whole
track, which reaches the vaults and the postwar settlement), then 10 regions about the walker, adding 2
volumes and 6 actors; the 197 stages to 480 s are those of `o4`, at the same seconds, and the walker's track
is the same byte for byte. The caller's assumed actor moves (near the vault) also choose cells at load, so
the vault's own volumes were read at load in both runs. After the load door (`d2`, `d1` with the switch)
the interior cell's one trigger volume was read when the walker arrived.

## The walking world follows the walker (`--play-follow-grid`), joined slices (`--play-slice`), `toward`

Three policies of the play mode, no rule: with `--play-follow-grid` the walking world is built again about
the walker's grid square when it stands in another square of a worldspace (as the viewer's moving grid; 4 to
8 s a time on this machine); with `--play-slice S` the walker's ticks are joined into slices of S seconds
(commands and activations still cut them at their seconds; the points and actions are the same, checked by a
unit test), which divides the number of times the run looks for its next due; the input's `toward X Y`
steers the walker to a point (heading to it each tick, forward held until it is within half a tick's move).

## The opening walked (the scripted proof, headless)

Input (`toward` commands built from the caller's track of the furthest run up to its change of space at
500 s, activations at 90 s and 190 s aimed from the walker's place, end at 480 s), the furthest run's switches
with the caller's two activations removed, `--walk-speed 300 --play-reach 150 --play-pick-bounds
--play-pick-bounds-activatable --play-live-world --play-load-doors --play-follow-grid --play-slice 0.25`
(`o4`). Against the recorded run of the caller's track (`spec/quest-run-home.txt`'s run):

- the walker leaves the bathroom at 17.6 s, walks the house, activates the front door at 90 s (its object
  bounds at 71 units), activates the activator the caller activates at 190 s (114.7 units; only with
  `--play-pick-bounds-activatable`: without it the object bounds of a static with no collision, which hold the
  activator whole, take the ray at 65 units and the run stops at stage 220's place), walks out of the front
  door at about 310 s (the door has no collision), down the street, across the town and up the hill to the
  vault (4 blocked ticks, 1 step in 7,965 ticks);
- up to 480 s, 197 of the recorded run's 202 stages, in the same order; 152 at the same second, the others
  between 0.096 s earlier and 0.003 s later (the walker's points differ from the caller's straight moves);
- **where it stops**: at 416.5 s the walker walks off the edge into the vault's elevator shaft and falls (it is
  still falling at 480 s): the elevator platform has no placed collision (an activator whose collision is not
  ready; a ray straight down from above it meets only its object bounds); the recorded run's stage set on the
  platform at 417.6 s and the four set after the ride (466 to 478 s) are missing. The shaft's load door at its
  bottom is a persistent reference the falling walker passes; nothing activates it (in the run it is passed by
  the caller's track under `world_load_door_xtel_v1`).

### Costs

The live world step measured 407 s of wall time for the 480 s of input (`o4`, machine busy with other work).
Where the time went (sampled): every turn of the loop asked each distance registration for its first due
second, which read placed actors' stored positions, each read searching every placement for its cell. The run
library step reads them once per run, and, profiled again, the next two hot spots as well: the placements'
cells indexed once (the world model's load read every actor near the track the same way), and the condition
evaluator's globals' stored values and references' bases read once per run (with a changed global looked up
among the run's changed facts). On this machine, the two runs at once, before and after (main's binaries
and the step's final ones, the lower figure on a quieter machine; outputs byte for byte the same):

| Run | Before | After |
| --- | --- | --- |
| the opening walked (`o4`, 480 s of input, 512 slices) | 232 s wall (215 s user) | 51 to 72 s wall (42 to 59 s user) |
| the museum step's furthest run (`quest_run`, about 1800 s of the run's clock) | 288 s wall (276 s user) | 24 to 35 s wall (16 to 25 s user) |
| the home step's furthest run given back in 0.05 s slices (171 slices) | 316 s wall | 52 s wall |

What is left in the opening walk: most of the time is the walking world built again about the walker's grid
square (`--play-follow-grid`: reading and decompressing the cells' models, `fo4_play::world::build`), then
the run's line choices outside scenes (the conditions of every candidate line at each hello and greeting) and
the record reads behind them (each read checks its plugin file on disk: about two thirds of the replay's wall
time is system time). The headless walk now runs at about the speed of its input; a window adds drawing.

## The elevator ride (`--play-elevator`)

The seen step showed the walking world can carry the walker on the vault
elevator's platform (`spec/walk.md`, Platforms; `ride_check`). The play mode
now does it from the run's state, headless (the window does not take the
switches; `crate::elevator`):

1. `--play-elevator REF` (repeated) builds, beside the walking world of the
   reference's space, its collision placed as stored
   (`walk_animated_model_collision_as_stored_v1`), split by the nodes its
   model's controller sequences move by translation keys: a source goes with
   the moving nodes in its body's chain (their offsets added), the rest stay
   still; `--play-elevator-also REF` places another reference's collision as
   stored, still (the walkway's static). The pieces are put in again whenever
   the walking world is built again in that space (the moving grid, a load
   door back) and left out of another space's.
2. The run records the sequences its graphs start (`fo4_vm`'s
   `anims.started`, the same list `--timeline-sequences` writes): when it
   starts one on a named reference, each moving node follows that sequence's
   translation keys from that second (sampled 60 times a second; the stored
   translation taken off; turned by the placement) and holds its last key
   after; a node the sequence does not key keeps its place; before any, a
   node stands at its stored transform (`play_elevator_run_sequence_v1`). A
   grounded walker on a moving part is carried (`walk_platform_sequence_ride_v1`).
   While a platform is named, standing slices last at most 0.1 s (the run's
   graph may start a sequence inside one; the platforms read it between
   slices), and the walker ticks while a part moves under it.
3. `--play-elevator-door` (with `--play-load-doors` and `--play-reach`): when
   a part that carried the walker reaches its track's end with the walker on
   it, the load door nearest the walker's feet among the walking world's
   load doors placed inside the named reference's collision box (grown by
   the reach) takes the walker through as its activation would; the run gets
   no activation (`play_elevator_end_load_door_v1`).

**The opening walked again** (`efin` in the step's notes; the live world step's
`o4` command line plus `--play-elevator` on the outside shaft and the
interior elevator, `--play-elevator-also` on the walkway, `--play-elevator-door`;
its input carried on past 480 s, and from 510.7 s to 700 s by `toward`
commands along the caller's recorded track inside the vault): the walker
steps onto the platform at 417 s (it waited at the top: the run's graph
started the shaft's sequences at 190, 377.7 and 381.6 s, which raise it
from its stored place), stands, is carried down 4,140 units from 481.6 s
(the descent the run's graph starts) to 500.2 s, where the shaft's door
(1,626 units above: the caller's recorded track rode only to its height)
takes it into the vault's elevator; the interior elevator, whose descent the
run's graph starts as the walker arrives (500.2 s), carries it down to its
bottom by 510.1 s. Against the recorded run (the live window step's furthest
run): **204 of 204 stages to 500 s in the same order** (before: 197 of 202 to
480 s, the walker falling into the shaft at 416.5 s), 158 at the same second,
the others from 0.096 s earlier to 0.033 s later; to 520 s 209 of 210 (one
opening stage the recorded run sets before 520 s is missing). At about 520 s the
walker, walking off the interior platform toward the vault along the
recorded track, falls (not investigated: a gap between the platform and the
vault's floor in the walking world, perhaps the elevator's own gate or the
vault's first door, which hold no collision here); past 520 s the walk is
lost (220 of 232 stages to 700 s, the rest after the fall).

## What the play mode does not do yet

- The elevators ride only headless, and only those the caller names (`--play-elevator`); only a sequence's
  translations are read; the window does not take the switches; past the interior elevator the walker falls
  (above).
- Plain doors: no collision is placed for any door, so none blocks the walker; the open state is not read.
- Moving collision with a node (an animated door, a platform) is not modelled; the live world moves only what
  the run moves.
- The window has not been opened by anyone; its live drawing is untested beyond its pieces.
- The reach of an activation, the walker's speed and its tick are the caller's; the activation key and the
  crosshair are the eye's ray (no prompt, no activation distance from the data).
- Sound, menus, combat, the player's animation events: still the caller's.
- Terminal menus, putting items into a container and actor values: the run's readings past the museum
  (`spec/vm.md`, Past the museum) take them from the command line (`--assume-terminal-item`,
  `--assume-item-added`, `--assume-actor-value`); a driver's input carries player actions only, so the stepped
  run gets them from its command line as `quest_run` does.

## Rules

| Rule | Switch | Basis | Strength |
| --- | --- | --- | --- |
| `play_player_point_walker_feet_v1` | the play mode | the walker is this project's controller; which point of the player triggers test is not in the data | weak |
| `play_run_clock_input_seconds_v1` | the play mode | none measured: frame time and script time are taken as equal | weak |
| `play_activate_collision_ray_v1` | the play mode's activation | a crosshair as an eye ray (prior knowledge); reach is the caller's | weak |
| `play_activate_object_bounds_v1` | `--play-pick-bounds` | OBND is a format fact; its use as the activation volume is a guess, needed because the door has no placed collision here | weak |
| `play_live_collision_follows_run_v1` | `--play-live-world` | the frame readings say what is drawn; that collision goes with drawing is prior knowledge | weak |
| `play_load_door_activation_v1` | `--play-load-doors` | `XTEL` read as `world_load_door_xtel_v1` reads it; that activation teleports at once is prior knowledge; facing not applied | weak |
| `play_region_around_walker_v1` | `--play-region-walker` | the run's own bound about the caller's track, kept about the walker; the game's loaded area is not modelled | weak |
| `play_activate_bounds_not_static_v1` | `--play-pick-bounds-activatable` | that statics are not activated is prior knowledge; needed because a static's box holds the activator whole | weak |
| `play_elevator_run_sequence_v1` | `--play-elevator REF` (with `--play-elevator-also REF`) | the run's graph starts the named reference's sequences (`anims.started`); the parts follow their translation keys; that a platform moves with the sequence its graph plays and carries its rider is prior knowledge | weak |
| `play_elevator_end_load_door_v1` | `--play-elevator-door` | the descent ends at the second the caller's recorded track changes space at the shaft's door; which door and when are this reading's | weak |
