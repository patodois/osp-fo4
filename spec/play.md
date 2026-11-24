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
switches for subtitles). The run's outputs are written when the window closes. The keyboard's driver has not
been driven by a person; its pieces are tested without a window (`Frames`, the title, the pick, the walker).

**The scripted window** (the window step). `--play-input FILE` drives the run with `play_run`'s walker and
that input: every play option is read, checked and applied by the play library's one structure
(`fo4_play::options::Play`, since the moving step; `Play::build_world` and `Play::walker` build the walking
world and the walker and write `play.txt`'s switch lines for `play_run` and the window alike, and
`fo4_play::report` the walker's lines and `play-track.txt`). The run is held to the window's clock
(`play::Gate`), the camera stands at the walker's eye, the window loads the cell the walker's walking world was
last built about at each change of space, and `--play-shots DIR --play-shot-every S` (with
`--play-shots-between FROM TO EVERY` for denser stretches) writes frames. With the exit step's switches and
input the window run gives the headless run's outputs byte for byte (274 of 274 stages to 960 s, 276 to
1,000 s, all at the same second).

The looks step, behind the timeline's and the light's switches (off by default): the window draws the run's
actors as their characters from the run's own timeline as it grows (`--timeline-cast` and a headless frame's
cast readings; rebuilt on a thread when an actor's place or presence changes) and lights the loaded cells and
draws the sky (the light switches with `--assume-hour 11 --assume-weather 0`); 60 fps median, but 10 to 16 fps
in the vaults (every placed light summed for every pixel) and their rooms washed out without an exposure.

The moving step:

- `--play-collision-child-flag-agrees` (`xesp_child_flag_agrees_chain_v1`, the drawing side's collision
  reading of an enable chain left open only by records' own flags that agree with it): every walking world's
  cell build takes it (`world::Options::child_flag_agrees`, the walker's rebuilds too), and the live world
  reads such a chain through its agreeing flags at the start and after the run's changes
  (`live::decide`, `Live::with_child_flag_agrees`). Headless with the exit step's switches: 276 of 276 stages
  to 1,010 s at the same second, and where the run seats the player at 730.11 s the walker stands on the
  floor (z -41.4) until 794.63 s instead of falling (airborne ticks 4,016 to 144).
- The named platforms drawn where their collision stands: `Walker::platforms()` gives, per named reference
  (`--play-elevator`), its model, the nodes its parts go by and each part's rigid motion at the second the
  rides were last advanced to (`elevator::Platform`); the window keeps each model shape draw's node chain for
  those references (`fo4_render::draw::BuildOptions::chains`, off by default) and each frame draws a shape
  whose chain goes with a moving part at its built transform moved by the part's motion
  (`Platform::part_of`, `elevator::transform`; shapes under no moving part stay as built). The drawing
  repeats the collision's motion under `play_elevator_rigid_v1` (or the translation reading) and reads
  nothing of its own: no rule. **In the opening it moves nothing**: none of the named references is drawn
  (their models are refused at render readiness: animation runtime of the behaviour graph, effect and
  visibility controllers, particle systems, skinned shapes, the reference's runtime record state), so the
  camera rides the collision past undrawn platforms; checked on synthetic draws and chains only.
- `--exposure-eye` in the scripted window: the light switches' exposure measured once per cell the window
  loads, from the walker's eye at the load (a viewer policy; the rule is the light step's
  `frame_exposure_seen_light_v1`), not per frame.

The opening in the window with the looks step's arguments and `--play-collision-child-flag-agrees` (`w1` in
the step's notes; audio off, release build): the outputs are the headless run's byte for byte (6 of 6 files,
and `play.txt` but for its timing lines, now that both write it through the library), 276 of 276 stages at
the same second; 1,344 s of wall clock for 1,015 s of input, 11 cell loads after the first; 59.8 fps median (60.0 in the
house and outside, 57.5 in the prewar vault, 12.1 in the postwar vault), first percentile 9.7, lowest 0.74
(a frame capture); peak resident memory 2.40 GB. What the frames show: from 730.1 s the camera stands at
floor height in the room the run seats the player in (the floor's markings at its feet, the run's people
standing about) but the room itself is not drawn: its collision is the agreeing chain's, its drawing is
refused at render readiness (1,097 of the cell's 1,923 placements omitted), so the frame stays black but for
the people and the markings. On both rides the camera rides down and up the shafts with the collision; the
platforms are not drawn. With `--exposure-eye` too (`w2`): the outputs again byte for byte and the frame rate unchanged (59.8 median; 57.4 and 12.1 fps in the two vaults), the exposure measured at the start and at each of the 11 loads; but where the walker arrives the prewar vault scales its lit colours by 1.01 (its entrance room stays washed out), the postwar vault by 1.24, and the room the run seats the player in, almost nothing lit in view, by 765, its people drawn white: measured once per load from the arrival's view, the exposure does not tame the vaults.

The drawn step, each behind its own switch (off by default; default outputs byte for byte):

- `--draw-child-flag-agrees` (`xesp_child_flag_agrees_chain_v1` for render readiness,
  `Loader::set_render_child_flag_agrees`): the room the run seats the player in at 730 s is drawn (its walls,
  raised floors, stairs, vents, crates and fixtures: 59 placements more; 34 more are hidden through their
  chains). The census of the eleven views the window loads is `spec/opening-views-census.txt`.
- `--draw-platforms` (`play_platform_drawn_v1`, `play_platform_shapes_left_out_v1`; with `--play-elevator`):
  each named platform standing in the loaded cell's space is drawn by its model as stored (loaded for the
  drawing when the cell load lacks it, as the exterior platform is), its record fields, behaviour graph and
  controllers left unread; its rigid shapes follow their parts' motions as before, its skinned shapes are
  drawn on the model's own nodes (`model_skinned_own_nodes_v1`) and follow the part every one of their bones
  goes with (a shape whose bones go with several parts stays where it was built); shapes the draw refuses
  and every effect surface of the model are left out, counted in the log, which also lists each platform
  draw. The skinned shapes are not posed by the sequence itself (`timeline_ride_sequence_nodes_v1` poses
  translation keys at one second, the collision's parts follow every channel): they follow the same rigid
  motions as the collision.
- The surface shader sums only the placed lights a grid lists at the sample (an exact bound, no rule;
  `spec/light.md`, The points' grid).
- `--exposure-eye` is measured per view (at each load, then when the view moved 64 units or turned 10
  degrees, at most four times a second; a viewer policy), and `--exposure-eye-floor`
  (`frame_exposure_seen_light_floor_v1`) gives a view whose seen light is under 0.25 the scale 4.

The opening in the window with every switch of this and the earlier steps (`w8` in the step's notes: the
moving step's `w1` arguments, `--play-collision-child-flag-agrees`, `--draw-child-flag-agrees`,
`--draw-platforms`, `--exposure-eye`, `--exposure-eye-floor`; audio off, release build): the outputs are the
headless run's byte for byte (6 of 6 files, `play.txt` but for its timing lines), 276 of 276 stages at the same
second; 1,264 s of wall clock; 60.0 fps median in every one of the twelve stretches between cell loads (before:
57.5 in the prewar vault, 12.1 in the postwar vault), first percentile 52.8 to 56.9 per stretch (before 5.5 and
10.0 in the vaults); lowest single frame 1.25 fps (a load), 7.3 without the 101 frames over 250 ms; peak
resident memory 2.44 GB (before 2.40). The frames: the room of 730 s is drawn (the tanks, their frames, pipes,
the lit floor, the run's people standing), the prewar vault's gear door, its bridge and the interior elevator's
cage are drawn, the postwar vault's gear door and machinery read clearly; the exterior elevator is drawn but
its deck (skinned shapes whose bones go with several parts) stays at the top while the walker rides down, and
the exit elevator's car stays dark; the exposure measured 741 times, the floor six times (the room's first
view, two views with nothing lit, the exit); the prewar vault's hall is still bright toward white in places.

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
recorded track, fell; past 520 s the walk was lost (220 of 232 stages to
700 s, the rest after the fall).

**Past the interior platform** (the chain step, `efin2` in its notes). The
fall's reason: the vault entrance room that holds the floor beyond the
platform (a static placed at the interior elevator's own position) has its
collision refused as the cell is built (`CollisionNotReady`: its readiness
is blocked by the model's controllers, particles and material fields, as
520 of the interior cell's references are), so the walking world has no
floor past the platform. `--play-elevator-also` places a reference's
collision as stored, but the still references went with the first named
reference only (the outside shaft), so one in the vault's cell never entered
its walking world: each now goes with the first named reference placed in
its own space (`elevator::still_owner`; the earlier runs, whose still
reference is in the first one's space, are unchanged byte for byte). With
the entrance room named as a still reference the walker steps off the
platform onto the floor, crosses the entrance and walks the recorded track
to 581.8 s: **all 222 stages the recorded run sets to 600 s, in the same
order** (167 at the same second, the others from 1.74 s earlier to 0.66 s
later; to 520 s 209 of 210, the opening's stage 632 coming 0.66 s after
520 s). At 581.8 s it drops about 190 units into the pit under the vault's
gear door: the gear door (an activator with an animated model) holds the
walkway there and its collision is refused the same way; placed as stored
it stands closed across the path (the run's graph starts no sequence on it
before 700 s). To 700 s 222 of 232 (the ten after the pit). The refusal is
the drawing side's readiness reading (`fo4-scene`, `fo4-walk`), not the play
mode's.

**Past the gear door, out of the pod and back to it** (the wire step, `w1` and
`w7` in its notes). Five switches, each off by default:

1. `--play-collision-off-chain` and `--play-collision-reference-alpha` give
   every walking world's cell build the drawing side's collision readiness
   readings (`collision_model_blockers_off_chain_v1`,
   `refr_xalp_collision_unchanged_v1`; `fo4_scene::readiness::Switches`
   through `Loader::set_collision_switches`); the walker carries them into
   the worlds it builds again (a load door, the moving grid). The vault's
   entrance room floor then comes from the cell build, not from a still
   reference the caller names.
2. `--play-elevator-held-pose`: a node a sequence controls through a transform
   interpolator without translation keys moves, from the second the run's
   graph starts that sequence, to the translation the interpolator holds it
   at (`nif_sequence_interpolator_held_pose_v1`), and stays there
   (`play_elevator_held_pose_v1`, `elevator::Source::Held`). Keyed nodes
   follow their keys as before; the two elevators then key 6 and 9 nodes
   instead of 2 and 3, with no change in the stages.
3. The gear door at the opening is named as a moving reference
   (`--play-elevator`) and the run takes `quest_run`'s existing
   `--load-on-arrival`: the door stands closed as stored at load; its load
   event, raised as the walker arrives in the vault (500.36 s), plays the
   graph event whose state plays the held-pose sequence, and its parts stand
   open from then (no new flag: the opening's arguments gain those two).
4. `--play-player-moveto`: a `MoveTo` the run calls on the player with zero
   offsets and a settled target puts the walker there (the walking world of
   the target's cell built), at the end of the slice the call ran in
   (`play_player_moveto_v1`; `fo4_vm::track::Place::player_moves`).
5. `--play-player-sits`: a sit the run gives the player in a furniture
   placed in another space than the walker's puts the walker at the
   furniture's stored place (`play_player_sit_furniture_v1`;
   `fo4_vm::sit::Sits::changes`); in the walker's own space the walk stands
   for the way there. When both fall in one slice the later wins; before
   the walker's start neither is followed.

The opening walked with 1 to 3 (the chain step's command line with the gear
door named, the entrance room no longer named, `--load-on-arrival`; input
to 700 s): against the recorded run (the live window step's furthest run),
**232 of 232 stages to 700 s in the same order** (before: 222 of 232, the
walker in the pit at 581.8 s), 172 at the same second, the rest from 1.74 s
earlier to 0.66 s later; the run, standing at the input's end, goes on to
245 of 245 to 800 s and to 814.3 s (250 stages, all the recorded run's to
that second). It equals the drawing side's local probe of these changes
byte for byte (track, census, names, timeline).

With 4 and 5 and the postwar gear door named too, the input carried past
700 s by `toward` commands along the recorded track in the postwar vault
(from 834 s; each held back until a walker at 250 units a second can have
reached the point before, so a short leg is not cut across a corner): at
730.1 s the run seats the player in the pod of the dream sequence's cell
(the walker drops there: no floor under that pod's place), at 794.6 s a
`MoveTo` puts it at a marker in that cell and 0.04 s later the vault
dialogue quest's fragment seats it in its own pod in the postwar vault,
where the walker stands; it walks out with the recorded track, the
overseer's console sits it in its own space (not followed), and the run's
graph opens the postwar gear door (its opening sequence from 885.7 s, the
held pose from 929.0 s). **262 of 262 stages to 930 s in the same order**
(199 at the same second, the rest from 1.74 s earlier to 2.50 s later).
At 929.1 s the walker stops at the open door's edge (x about -3,513): the
door's walkway stands about 50 units above the floor on the console's side
(the prewar walk came down it), more than the walker's 20-unit step; only
the parts' translations are applied, and the open pose's rotations (which
would tilt or roll the parts) are not. The recorded run's next stage is at
934.4 s (the vault dialogue quest's 97), then the exit elevator's ride up
(945 to 955 s) and the exterior at 968 s: 262 of 274 to 960 s.

**Up the ramp, up the exit elevator and out** (the exit step, `e5` in its
notes). Three play switches and one caller assumption of the run, each off
or absent by default:

1. `--play-elevator-rigid` (`play_elevator_rigid_v1`, `elevator::Part`,
   `elevator::Readings`): in place of the translations, a named reference's
   collision sources go by the deepest node of their body's chain that any
   of the model's sequences sets through a transform interpolator (the
   drawing side's `sequence::chain_pose` at each sequence's start); sources
   no sequence sets stay still. For each part and sequence the motion the
   sequence gives the chain up to that node (the chain as stored against the
   chain as posed, under the reference's placement; `fo4_walk::Motion::between`)
   is sampled 60 times a second over the span; the part rides those motions
   as a `fo4_walk::platform::RigidRide` from the second the run's graph
   starts the sequence, and holds its motion under a sequence that does not
   set it. Moving, the end of a ride that carried the walker, leaving and the
   box read the rigid rides as they read the translation rides. The held pose
   switch is not read under it (the chain pose holds the held translation).
   On the opening's four named references the rides change no stage to
   930 s; the postwar gear door's ramp turns down to the floor and the walk
   goes on as the drawing side's local probe did, byte for byte in track,
   censuses, names and timeline (only the report's summary lines differ).
2. The postwar vault's exit elevator named (`--play-elevator`). Named as
   stored, its collision blocks the way into it at x -5,161.6 (the walk
   loses the vault dialogue quest's 98 and 100): the stored pose, which is
   also the pose of its graph's start state, holds its platform near the top
   of the shaft and a gate leaf (three sources under one node) down across
   the doorway. Its graph's start state takes only the descent event; the
   descent's state and, through the text key at its end, the idle state hold
   the platform 1,574 units lower and the gate leaf raised 177 units, and
   both take the ascent event the exit trigger's button function plays. No
   script of the run sends the descent, so the run's `PlayAnimation` of the
   ascent took no transition. The caller's assumption `quest_run
   --assume-graph-event REF EVENT[@SECONDS]` (`anim_graph_caller_event_v1`,
   `spec/vm.md`, The caller's graph event) sends the descent at 800 s, after
   the run puts the player into the postwar vault: the platform descends by
   823.8 s and stands open; the button function's ascent at 945.35 s is then
   the graph's own transition, and the walker on the platform rides 9.87 s
   up the shaft. The ride ends at 955.22 s and `--play-elevator-door` takes
   the walker through the shaft top's automatic load door (176.5 units from
   its feet) to the exterior's teleport position.
3. The vault's exterior elevator named (`--play-elevator`), and the
   walkway's still reference given to the outside shaft by name
   (`--play-elevator-also-with REF OWNER`, a caller option: the walkway is
   placed in the worldspace the exterior elevator stands in, so naming that
   elevator would otherwise take the walkway away from the prewar shaft and
   lose the prewar walk). The teleport position stands over the open shaft:
   without the exterior elevator the walker falls through the terrain's pit
   and out of the world. With it the walker settles on its closed hatch
   (46 units below the teleport position); the exterior trigger's script
   plays the rise at 955.35 s (a wildcard transition), the hatch opens, the
   walker drops onto the rising platform and is carried to the surface
   (z 8,966.8 at 966.8 s, the recorded run's 8,968.4 at 968 s).
4. `--play-elevator-door-onward` (`play_elevator_end_door_onward_v1`): when
   the exterior rise ends with the walker on it, the nearest load door in its
   box (1,067 units from the walker's feet) leads back into the postwar vault
   (the walking world holds two load doors into it); with the switch, doors that lead into the space the walker left by its last
   change of space are left out, so the ride's end takes the walker nowhere
   and it stays outside. Without it the walker is taken back into the vault
   at 967.2 s.

The opening walked with all of them (the wire step's arguments, the walkway
given by name, the input carried on from 955 s by `toward` commands along the
recorded exterior track to 1,015 s): against the recorded run, **274 of 274
stages to 960 s in the same order** (199 at the same second, the rest from
1.74 s earlier to 2.91 s later: the tutorial stage set riding up the shaft
comes 2.9 s later than on the caller's straight ride), 276 of 277 to 1,000 s (the tutorial quest's 360 and
370 at 980.66 s; its second 360 at 999.3 s missing). The walker leaves the
platform at 968 s along the recorded track and stops at 998 s
(x -83,776, y 91,159) against a dirt cliff piece whose face rises at 45.6
degrees, just past the walker's provisional 45-degree slope limit; the
recorded run's next stage (the next main quest's 20, at 1,005 s, from the
settlement's trigger) is not reached. This is the whole of M3's headless
walk: from the start to the exterior.

## The person's input (`--play-keys`)

`fo4-view --play-run ... --play-keys` drives the same walker (`fo4_play::walker::Walker`), built by the same
play options (`fo4_play::options::Play::walker`), on the same run thread as `--play-input FILE`; only where the
walker's commands come from differs. A scripted input is a list the walker reads ahead in; a person's commands
arrive while the walker runs (`walker::Arrivals`, `Walker::with_arrivals`). The window turns the keyboard and
the mouse into the scripted input's own commands (`fo4_play::input::Command`) at the play clock's second of the
frame they happen in, and the walker waits for them only as far as its next step needs to know them:

- before a tick ending at `t + tick` it waits until every command up to then is known, so the tick is cut at a
  command's second exactly as with a file and never at a frame's end;
- before standing it waits until every command up to `t + S` is known, where `S` is the longest standing slice,
  `--play-stand SECONDS` (a play option, a policy of the play mode, no game claim, off by default): a standing
  slice then lasts `S` at most and the walker looks no further than `S` ahead for a moving platform before it
  stands. A person's next command is not known beyond the present, so `--play-keys` needs `--play-stand`; give
  `play_run` the same value to replay. Without `--play-stand` the walker is as before (a standing slice runs to
  the next command), so every earlier output is unchanged;
- the window stamps a frame's commands at the second the walker's clock reaches with that frame (`play::Gate`:
  the horizon, or the horizon and what a busy run is owed), under the lock that moves the clock, so every
  command lies after every second the walker was told it knows; in a frame that adds no time (paused, a cell
  loading) no command is sent and the person's state waits for the next frame that does.

The key map (viewer policy, no rule; `crate::person` in the viewer): W, S, A, D hold forward, back, left, right
(left and right strafe); Shift holds run; the mouse turns and looks up or down while the left button is held, as
in the viewer's walk mode (`--radians-per-pixel`, a viewer setting), each frame's look sent as `face` and `pitch`
(absolute degrees, pitch within 89); E sends `activate`, after the frame's look; Escape sends `end`; P pauses
(no command: the window's clock stops, so the run and the walker wait). The walker has no jump, so no key jumps;
`turn` and `toward` have no key (the person's mouse gives absolute headings; `toward` is a scripted steering
aid). The camera looks where the person looks at once and stands at the walker's eye, which the walker shows the
window at each wait (each tick while it walks).

On screen (the title bar, viewer policy): the run's second, the last stage and the lines being spoken (the
subtitles, as before), then `E:` and what the activation key would meet from the walker's eye now
(`Walker::aim`, the activation's own ray read without acting: the base record's type and editor ID and the
reference's key, or "nothing within reach"; read at each slice), `paused` while paused, and after Escape that
the run goes on to its end. No crosshair is drawn: the activation's ray is the window's centre. No menus (M6).

Quitting: Escape is the input's end. The walker stands to that second, the run goes on to its end as `play_run`'s
does after an input's end, writes its outputs, and the window closes. Closing the window instead stops the run
where it is (outputs as they stand); the recording then has no `end` and its replay is not the same past the
last command.

### The recording (`--play-record FILE`) and what "the same" means

`--play-record FILE` writes each command the walker takes, as it takes it, in the scripted input's format
(`Event`'s text: numbers written as Rust writes an `f64`, the shortest text that reads back to the same value,
so every second and degree reads back bit for bit), after three comment lines naming the window's command line.
`play_run` with that file and the same play options (`--play-stand` among them) and new-game switches gives the
same run: the walker's ticks are on its own grid from the start second, cut at the commands' seconds, and those
seconds are the recorded ones, so the replay takes the same ticks, joins the same slices and gives the run the
same points, seconds and activations; the run is a function of its slices (what the window reads from it,
`View`, changes nothing). What is the same: every output file byte for byte (track, census, names, the two
next-blocker files, the timeline) and `play.txt` but its timing lines. What is not: the frames' timing itself
(the play clock's second of a command is a frame's end, not a tick's: the walker cuts its tick there, in the
session and in the replay alike), the wall clock, and what the window drew. Checked by a test
(`walker::tests::commands_that_arrive_drive_the_walker_as_their_recording_does`: frames of uneven length and
busy stretches, looks while walking, keys, run, strafe, activations, the end; the walker fed as they arrive and
the walker fed the recording give the same slices bit for bit) and by the window runs below.

### The synthetic person (`--play-keys-from FILE`, test and measurement only)

A file of timed events at seconds of the window's play clock (`key W|A|S|D|E|P|Escape|Shift down|up|tap`,
`button down|up|tap`, `mouse DX DY`, and four that read the window as a person does: `turn-to DEGREES` and
`pitch-to DEGREES` move the mouse with the left button held until the view has that heading or pitch,
`steer X Y` holds W and moves the mouse each frame toward the point from the walker's feet as drawn, letting W go
near it, and `aim-at X Y Z` lets W go and keeps the view on a point from the walker's eye). Its events are timed
by the play clock the window stamps commands with, which stands while a busy run is owed its most (timed by the
window's frames instead, its events came early while the walker's world was built again, and it lost the walk
at 313 s). The window writes them as the key, button and motion messages a keyboard and a mouse send, before the
input systems read them (`gpu::person_inject`, before `bevy::input::InputSystems`), so they go through the key
map as a person's presses do; held keys are pressed again each frame. It is an aid for checks and measurements,
not a way to play. A scripted input becomes one by `toward` → `steer`, `face` → `turn-to`, `pitch` →
`pitch-to`, `activate` → a tap of E, `end` → a tap of Escape; a tap ends a frame's events, so what follows it at
the same second comes a frame later. For the opening an activation became what a person does: look at the point
the scripted activation's ray met (`aim-at`), E 0.1 s later, the look back 0.15 s later.

**The opening played by the synthetic person** (the hands step, `k7` in its notes; release build, audio off,
the drawn step's window switches with `--play-keys --play-stand 0.1`, the scripted input of the exit step made
synthetic as above): 7,070 commands reached the walker (looks each frame while steering, keys, two activations,
the end at 1,015 s); **276 of 276 stages**, the scripted window run's, 188 at the same second, the others from
0.39 s earlier to 0.19 s later, three in another order where their seconds shifted; the walker's track within 65
units of the scripted walker's at every tenth second, and it stops where the scripted walk stops (the cliff face
past the vault's exterior, x about -83,777). 1,228 s of wall clock; 59.97 fps median (60.0 in every stretch
between loads, first percentile 54.6 to 56.1), lowest 1.27 fps at a load; peak resident memory 2.50 GB. The
recording replayed by `play_run` with the same options: every output file byte for byte (track, census, names,
both next-blocker files, timeline) and `play.txt` but its timing lines. An earlier whole session (`k6`) whose
synthetic person turned and pitched to the scripted degrees at 190 s instead of looking at the activator, about
25 units behind the scripted walker, met nothing within reach: the platform outside the vault was never raised,
the walker stood at its edge, 151 of 276 stages; its replay was byte for byte the same too. Where the synthetic
person differs from the scripted input it is a person's: it acts at a frame's end, a frame or two late, and
aims by eye.


## What the play mode does not do yet

- The elevators ride only those the caller names (`--play-elevator`), headless and in the scripted window;
  the window draws a named reference's parts where its collision stands, but in the opening none of them is
  drawn (refused at render readiness). The exit elevator's descent before the player reaches it is the caller's assumption
  (`--assume-graph-event`): nothing in the run's data sends it.
- The exterior's collision platform that an enable marker holds while the elevator rises (the exterior
  trigger's script disables the marker after waiting 12 s; the interpreter does not time the wait) is not
  read: the walker stands on the exterior elevator's own hatch and platform instead.
- Plain doors: no collision is placed for any door, so none blocks the walker; the open state is not read.
- Moving collision with a node (an animated door, a platform) is modelled only for the references the caller
  names (above); the live world moves only what the run moves.
- A person's keys drive `play_run`'s walker in the window (`--play-keys`, the hands step), checked so far by
  the synthetic person and the unit test, not yet by a person at the keyboard. The run's caller assumptions
  (`--assume-*` with their seconds, the opening's new-game switches) are timed for the scripted input's pace: a
  person who walks the opening at another pace meets them at other places.
- The reach of an activation, the walker's speed and its tick are the caller's; the activation key and the
  crosshair are the eye's ray (no prompt, no activation distance from the data); no crosshair is drawn (the
  title bar names what E would activate).
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
| `play_elevator_held_pose_v1` | `--play-elevator-held-pose` | the held pose is the drawing side's reading (`nif_sequence_interpolator_held_pose_v1`, weak); that the collision goes there at once and stays is this reading's | weak |
| `play_player_moveto_v1` | `--play-player-moveto` | the run's own `MoveTo` calls on the player; that `MoveTo` places at once is the API's meaning (prior knowledge); the slice-end delay and settling are this reading's | weak |
| `play_player_sit_furniture_v1` | `--play-player-sits` | the run's own sits (a fragment's activation of the player's pod); that an activated sitter goes to the furniture at once is prior knowledge; the reference's place standing for the marker is this reading's | weak |
| `collision_model_blockers_off_chain_v1`, `refr_xalp_collision_unchanged_v1` | `--play-collision-off-chain`, `--play-collision-reference-alpha` | the drawing side's rules (`spec/collision-placement.md`), given to the play mode's cell builds | weak |
| `xesp_child_flag_agrees_chain_v1` | `--play-collision-child-flag-agrees` | the drawing side's rule (`spec/enable-parent.md`), given to the play mode's cell builds and to the live world's decisions after the run's changes | weak to medium |
| `play_elevator_rigid_v1` | `--play-elevator-rigid` | the drawing side's `nif_sequence_chain_pose_v1` (weak to medium) and `walk_platform_rigid_motion_v1` (weak) given to the named references; the grouping by the deepest set node and the part holding its motion are this reading's | weak |
| `play_elevator_end_door_onward_v1` | `--play-elevator-door-onward` | the recorded track stays outside after the exterior rise; that a ride's end never takes the walker back is this reading's | weak |
| `anim_graph_caller_event_v1` | `quest_run --assume-graph-event REF EVENT[@SECONDS]` | a caller assumption: no script of the run sends the exit elevator its descent; that it waits at the bottom open is prior knowledge | weak |
