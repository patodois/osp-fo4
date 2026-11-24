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
(left and right strafe); Shift holds run, whether it comes as a key of its own or only as a modifier of the
movement key's press (a tool that sends Shift+W as a flag on W, with no Shift key event of its own: the window
system's modifier change says Shift, `person::shift_of_modifiers`; the run lasts while a movement key is held);
the mouse turns and looks up or down while the left button is held (`--radians-per-pixel`, a viewer setting),
from the raw device motion when there is any, else from the window's cursor between its positions while the
button is down, read in the order the window's events came (`person::Drag`: a drag a tool sends as a warped
cursor, a press and a release that carries the cursor's end position, or all three in one frame, turns the view
too; both are never added); the arrow keys turn left and right at 90 degrees a second and look up and down at
60 (`person::ARROW_TURN_DEGREES_PER_SECOND`); each frame's look is sent as `face` and `pitch` (absolute degrees,
pitch within 89); E sends `activate`, after the frame's look; Escape sends `end`; P pauses (no command: the
window's clock stops, so the run and the walker wait). The walker has no jump, so no key jumps; `turn` and
`toward` have no key (the person's look gives absolute headings; `toward` is a scripted steering aid). The camera
looks where the person looks at once and stands at the walker's eye, which the walker shows the window at each
wait (each tick while it walks).

Commands made while the play clock stands (the run busy building a walking world past the 0.25 s it may be
owed, a cell loading; not while paused) are kept in order with the window's wall clock and given when the clock
runs again (`play::Gate::submit`): from that second, each gap between two of them kept up to 0.5 s
(`play::DEFERRED_GAP_MAX`), a look replaced by a later look, later commands after them. A key held across the
stand stays held; a press made during it moves the walker for as long as it was held (up to the gap) instead
of arriving at one second with its release. The recording holds them at the seconds given, so the replay is
the same.

On screen (viewer policy): the title bar as before (the run's second, the last stage, the lines being spoken,
`E:` and what the activation would meet, `paused`); and in the viewport (`--play-hud`, on with `--play-keys`,
asked for in the `--play-input` window, never in headless frames; `crate::hud`), a small crosshair at the
centre (the activation's ray), under it `E` and what the activation would meet when that is a thing E uses (not
a static's collision or box, not a collision of no reference) and nothing otherwise, near the bottom the line
being spoken, near the top `paused` and `loading` (a cell loading or uploading, the walker's walking world being
built). The text is drawn with the viewer's own 5 by 7 ASCII font (`hud_font`; the pinned Bevy compiles neither
a text renderer nor a font), characters outside it shown as their ASCII look-alike or a question mark, by
quads of a camera of their own drawn after the scene. No menus (M6).

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


### What the first session met (the person step)

A tester drove the window (`--play-keys`, the preset) through a desktop-automation tool in two sessions: its key
presses arrived, its mouse drags never turned the view, Shift with W never ran, the first view was white, the
outdoor rebuilds froze the picture and swallowed presses, and the run stopped at the opening's stage 475 with the
person on the shaft's platform. What each was and what changed (viewer policy unless a rule is named):

- **Looking.** The window read the left button's state at the frame's end and the cursor's motion as a sum over
  the frame. The window system sends a drag's motion with the release itself (a release brings the cursor's
  position before the button's change), and a tool that warps the cursor sends no motion between the press and
  the release, or all of it in one frame: the button was up when the motion was read, and both records hold no
  `face` or `pitch`. Now the button's press and release and the cursor's positions are read in the order the
  window's events came (`person::Drag`): the cursor's movement while the button is down turns the view when raw
  motion gives none (never both). The arrow keys turn at 90 and look at 60 degrees a second. Checked by a test
  that sends a tool's drag (no raw motion, press and release in one frame) to the person's system, and by the
  synthetic person's new `drag DX DY` and arrow keys in the window run below.
- **Shift.** The tool refuses a standalone Shift and sends Shift+W as a flag on W's press with no Shift key of its
  own: the window library then attests no left or right Shift, so no Shift key is pressed and `run` was never
  sent. Shift now also counts when the window system's modifier change says Shift while a movement key is held
  (`person::shift_of_modifiers`, `run_held`).
- **The first view.** Not the near plane and not the wall's texture: the light at the start's walls (2.6 to 3.0,
  the room's placed light at one or two units) is above 1, and the exposure is measured on the centroids of the
  lit triangles in view, mostly the town behind the walls (19,648 points, nine in ten under 0.66, scale 1.51).
  `--exposure-eye-visible` (`frame_exposure_seen_visible_v1`, in the preset) measures what the eye sees
  (`spec/light.md`, What the eye sees): the first frame shows the room.
- **On screen**: the crosshair, the prompt, the subtitle and paused or loading in the viewport (above).
- **Rebuilds.** A grid square left cost the walker's walking world (2 to 3 s on a quiet machine, 2.4 to 6.5 s
  during the session) and then the window's load of the cells' drawing (5.6 to 7.5 s on a quiet machine, 6.4 to
  15.8 s during the session: 7.2 s of it reading the drawing, 0.1 s the window's own walking world), one after
  the other, with the play clock standing; presses made meanwhile were stamped at the one standing second, so a
  press and its release arrived together and moved nothing. Now commands made while the clock stands are kept
  and given when it runs (above); the walker keeps the walking worlds it built last and takes the kept one for a
  cell it just left (`Walker::load`; 0.02 s instead of 2 to 3 s, 4 of the session's 10 rebuilds, outputs byte for
  byte); and the window starts loading the cell's drawing as the walker starts building (`Walker::with_on_load`,
  `Seen::next_cell`), so the two overlap. On the synthetic person's route (`k7` before, `L1` after, the window's
  frame log's stretches where the drawn place stood still): the five first crossings stood 10.5, 10.6, 10.7, 8.7
  and 7.7 s before, 7.6, 7.6, 7.7, 6.5 and 5.7 s after, the square crossed back 8.7 s before and 6.6 s after (the
  walker's 0.03 s, the rest the window's load); what is left is the window's load of the drawing, 5.6 to 7.5 s.
- **Arriving late.** The run in session B did not wait on a timed assumption: the person's E in the nursery met
  nothing (73 to 84 s: the view could not pitch toward the activator, and the run's stage that the activation
  follows came at 183 s), so the activation the scripted pace gives at 190 s never came and the stages that
  follow it (from 230) never did; 425 came from a trigger crossed on the street and 475 at 420 s from the
  caller's timed move of an actor. The ride itself is not timed to the scripted pace: walked 200 s late from the
  vault's site (headless, the exit step's arguments), the run sets the platform's stages when the person arrives
  (617.6 s), its graph starts the descent 15.6 s later and the walker rides it into the vault (stage 600 at
  652.0 s; scripted: 500.4 s). Of the opening's timed caller assumptions, the front door's open stands for the
  person opening the door, which the data names: with `--play-activate-opens REF` (`play_activate_opens_v1`)
  it leaves the run's switches and the person's activation of the door gives the open. The talks, the actor
  moves, the animation events and the activations in the vaults stay the caller's: the talks and the actor
  moves stand for what no player action of the run's carries (no talk action, actor moves not gateable by
  `--action-after`), and every one that follows a stage is already held until it (`--action-after`: at its
  second when the stage is done by then, else as soon as it is).

**The opening played late by the synthetic person** (`L1` in the step's notes; release build, audio off, the
drawn step's window switches with `--play-keys --play-stand 0.1 --exposure-eye-visible --play-activate-opens`
for the front door; the hands step's synthetic person made 200 s late before the walk from the vault's site to
the platform, its walk back and forth on the platform left out, a cursor drag and the arrow keys at the start):
the first frame shows the bathroom; the door's open came with the person's activation (stages 97, 100 and 110
at 90.16 s); at the site 200 s late the platform's stages came at the person's arrival (490 at 616.9 s, 495 and
500 at 617.6 s), the descent started at 633.2 s and the walker rode it into the vault (the window loaded the
vault's cell at 652.0 s), 260 stages in all (the input's later commands, timed for the scripted pace plus 200 s,
lose the rest of the walk as the headless late walk does); 6,928 commands; 1,413 s of wall clock for 1,215 s of
input; 59.96 fps median, first percentile 55.6, lowest 1.27 (a load); peak resident memory 2.20 GB (footprint
4.41 GB). The recording replayed by `play_run` with the same options: every output file byte for byte and
`play.txt` but its timing lines. The viewport's text was checked in a shorter run (`s2`) after a fix (a quad's
image changed after its material was prepared kept the first image): the subtitle and `E` with the door's label
under the crosshair are drawn.

### What the third session met (the way-out step)

A tester drove the window again (session B of the third play session: the whole opening, its last stage at
878 s, then the follow-on quest held at its stage 9 and the person could not leave the vault). What each was and
what changed (viewer policy unless a rule is named):

- **The way out.** Three causes, read from the session's record replayed by `play_run` with the ray's hits
  logged. (1) The pick: the postwar gear door's object bounds (959 by 660 by 735 units) hold the whole walkway
  before the exit console, so from every place on it the eye stood inside that box and the ray met it at
  distance 0; every activation went to the door, never to the console, whose own small box the ray meets from
  outside at about 50 units only when looking down at it. The door is a named platform, its collision placed by
  the elevator reading, so its box stood in the activation's list too. With `--play-pick-first-met`
  (`play_activate_ray_first_met_v1`) what the eye stands inside (a box holding the eye, a body met at distance 0)
  comes after everything the ray meets from outside, the named platforms' boxes are left out (their collision
  stands for them) and so is the box of a base that names no model (a light with no fixture hung over the device
  on the skeleton took the ray aimed at it). (2) The run: the console's activation and the device's on the
  skeleton were the caller's timed `--assume-activate` held until their stages; with `--play-activate-person
  REF` (`play_activate_person_v1`) they leave the run's switches as the front door's open did, and the person's
  E is the activation (an E before the reference's stage does what its handler then does: in the opening, the
  console before the device sets the quest's 5 and nothing opens). (3) The walkway: it ends at the gear door,
  which stood closed because the follow-on quest's 8 never came. The caller's two player animation events that
  follow the console's use (`--assume-anim-event player ...@880` and `@885`, both held until the quest's 7) were
  held for 30 s and then given at one second; the first plays the console's graph event and the second, at the
  same second, finds the graph between states and takes no transition, so the text key that sets 8 and opens the
  door never comes. That is the interpreter's gate (`--action-after` releases every held action of a gate at
  once; `crates/fo4-vm`, not edited here): what is missing is either a gate that keeps an action's offset from
  its stage (the two events 10 and 15 s after the console's use, as the caller's own seconds space them), or
  caller actions of every kind (animation events) given by the stepper with `NewGame::advance_to`, so the play
  mode could time them from the person's activation. The proof below gives the two events the person's late
  seconds by hand (a caller assumption of that run).
- **The 76 s after the ride down.** Not the window: sampled during the stand, the run's thread was inside the
  interpreter's load settling (`fo4_vm::load::settle` at the change of the player's area: every loaded
  reference's position, each placed actor's and moved object's through `track::stored_actor` and
  `track::stored_position`, whose `space_of` searches every placement of the load order for each reference;
  `track::Cells`, the run's index of placement cells, is not used on that path). Headless, the session's record
  replayed spends 61.3 s of the run's turn there (612.9 to 621.4 s of the run), 8.6 s at the postwar vault's
  arrival and 3.2 s at the dream's; the window's frames went on at 60 fps meanwhile, the play clock stood (the
  busy run is owed at most 0.25 s) and the title showed the run's last event's second. Not cut here (it is
  `crates/fo4-vm`'s: the index passed to those reads would cut it). The viewport now says `loading` whenever
  the play clock stands for more than 0.5 s while nothing is paused, and the title shows the play clock's
  second (the clock the walker goes by), not the run's last event's.
- **The seats at the pods.** The first is the furniture the run seats the player in (a pod placed in a cell of
  its own, the dream's; the reference headless run seats it there too, at 730.1 s); the second, the player's
  pod in the postwar vault. The window loads and draws that cell where the player is taken (6.3 s). It is dark
  by its own data under the drawing's readings: its ambient is near zero (0.004) and of its 211 placed lights
  162 are off at load under `xesp_child_flag_agrees_chain_v1` (`--draw-child-flag-agrees`), 107 of them
  children of one initially disabled light marker of a later quest's memory set that the run never enables; 49
  light the room (156 without that reading). No change: which reading holds is the pending rules'.
- **The viewport** (`--play-hud`): the prompt is a short action and a plain name from the record's type and
  its display name (`E  open <name>` for a door or a container, `E  use <name>` otherwise, the type when a
  door has no name: `fo4_view::play::prompt_of`), the record keys and distance in the title bar; subtitles
  show only the lines whose text the data settles, wrapped to three lines (what the timeline says about the
  others goes to the title); the objective the run displayed last and has not completed (the quest's own
  objective text, read through its string table) in the top left corner; after E, `opened`, `used` or
  `nothing happened` in the prompt's place for 1.5 s (the run's effects and logs before and after the slice the
  activation came in).
- **Markers and heads.** The bright shapes at the checkpoint and in the pod room are the cast's placeholder
  markers for actors the drawing refuses (the cast lines count them: 6 at the checkpoint, 19 to 20 in the pod
  room), not placed editor markers; the window's log now lists each marker's reference and reason and each
  character drawn without its head (`play: cast: ...`).

**The way out played late by the synthetic person** (`L8w` in the step's notes; release build; the person
step's late route with the person's E at the device on the skeleton at 1,061.6 s and at the console at
1,071.6 s, aimed at their boxes; the window switches of the preset plus `--play-pick-first-met`,
`--play-activate-person` for the two references, the two animation events at 1,081 and 1,086 s): the follow-on
quest's 3, 4, 6, 7, 9, 8 and, riding the exit elevator up, 10 and 15 at 1,155.4 s; the walker leaves the vault
and stops at the cliff face past it, as the scripted walk does; 276 stages, the headless run's (`L8c`) in the
same order, 139 at the same second, the rest within 0.18 s; 59.97 fps median, first percentile 54.7, 81 frames
over 250 ms (loads); peak resident memory 2.22 GB. The run stood 61 to 65 s at the vault's arrival (above).

### A late person keeps the caller's spacing (the gate step)

The interpreter's gate (`quest_run --action-after`) released every action it held at the second its stage came,
so a person late against the scripted pace got the caller's spaced events at one second. With
`quest_run --action-after-offset` (`caller_action_after_keeps_offset_v1`, off by default; `fo4_vm::run::NewGame`)
a held action keeps its offset from its gate: when the stage is done, it happens at the stage's second plus its
own second minus the gate's reference second (never before the stage), and the gate's actions still waiting
for their second move as much (never before it).

- **The reference second** is the written second of the caller's action in whose turn the stage was done, else,
  for a stage the run sets by itself, the earliest held action's second on that gate. Why: the opening's gates
  are written as a chain, each naming the stage the caller's previous action sets (the device on the skeleton
  sets the follow-on quest's 4 and the wrist computer's event 5 s later waits for 4; the console sets 7 and the
  two player events 10 and 15 s later wait for 7), so the caller's own seconds space each action from the one
  that set its stage; the earliest held action alone would put the first event at the console's second, not 10 s
  after it. A stage the run sets itself (the opening's 780, by the scene clock, before the spouse's 14 events)
  has no written second; the earliest held action stands for it, which keeps the spacing among the gate's actions
  and gives up the 7.5 s the reference run had between the stage and the first event.
- **The person's activation as written.** With `--play-action-after-offset` (`play_activate_written_second_v1`,
  off by default; `play_run` and the window, in the opening preset) the play mode gives the run
  `--action-after-offset` and, for each timed open or activation it leaves to the person
  (`--play-activate-opens`, `--play-activate-person`), `--input-action-at open|activate REF@SECONDS` with the
  second the caller wrote for it (`fo4_play::options::Play::run_args`): a stage the person's activation sets takes
  that second as its reference. The exit needs no hand timing: the two events come 10 and 15 s after the person
  uses the console, as the caller's seconds space them after the scripted use.
- **Late runs, the original arguments** (the exit step's switches, the agreeing flags, `--play-pick-first-met`,
  the device's and the console's activations the person's; no hand-moved second; headless, notes `b1L200`,
  `b1L400`): 200 s late, the follow-on quest's 4 at 1,061.6 s (the person's E), 6 at 1,066.6, 7 at 1,071.6 (E at
  the console), 9 at 1,086.6, 8 at 1,087.27 (the gear door opens), 10 and 15 at 1,155.36 riding the exit
  elevator up; 276 stages, the reference run's set; the walker leaves the vault and stops at the cliff face past
  it. 400 s late: the same 276 stages, the exit's 4, 6, 7, 9, 8 at 1,261.6 to 1,287.27 s, the ride up at
  1,355.36 s. Without the switch, 200 s late, the run reaches 260 stages: 7 and 9 at the console's second, no 8,
  the walker stays in the vault.
- **In the window** (notes `WL200`; release build; the way-out step's late synthetic person with its untouched
  seconds, the preset's window switches with `--play-action-after-offset`): the same 276 stages in the same order as
  the headless run, 136 at the same second, the rest within 0.22 s; the exit's 8 at 1,087.30 s, the ride up at
  1,155.39 s, the walker at the cliff face; 1,314 s of wall clock for 1,215 s of input (1,424 s in the way-out step's
  window proof), 59.97 fps median, first percentile 54.9, 42 frames over 250 ms (31.6 s, loads; 81 and 59.5 s
  before); peak resident memory 2.62 GB (footprint 4.74 GB). 400 s late (`WL400`, the same person with every command from 600 s on 200 s later): the same 276 stages in the
  headless run's order, 136 at the same second, the rest within 0.18 s; the exit's 8 at 1,287.29 s, the ride up at
  1,355.37 s, the walker at the cliff face; 1,519 s of wall clock for 1,415 s of input, 59.97 fps median, first
  percentile 55.0, 48 frames over 250 ms (36.5 s); peak resident memory 2.52 GB (footprint 4.77 GB).

**The settle at a change of area.** The interpreter's load settle (`fo4_vm::load::settle`, at each change of the
player's area) found each loaded reference's cell by searching every placement of the load order
(`track::stored_position` without an index); it now looks the cell up in the run's index of placement cells,
read once (`track::Cells`, `track::stored_position_with`), with the same answers (`Cells::read` keeps a record's
first membership, as the search finds it). Seconds of the settle at each change of area, headless (the exit
step's on-time run and session B's replay, timing builds that only print them): the vault's arrival 61.7 and
61.8 s before, 0.10 s after; the dream's cell 3.1 and 3.2 s, 0.10 s; the postwar vault 8.7 s, 0.15 s. The whole
run's turn 122.0 s before, 48.4 s after (session B's replay: 137.4 s, 64.2 s; the opening's whole `quest_run`, no
play, 117.0 s and 25.8 s of wall clock). Every output byte for byte. In the window (the exit step's scripted input
to 805 s, the window switches, timing builds): 69.4, 4.0 and 10.5 s before, 0.10, 0.12 and 0.17 s after; the run's
outputs and the 15 frames written byte for byte the same.

**The window's load at a crossing** (viewer policy). Timed by phase in the same window runs: of each 5.6 to 8.1 s
load of a grid square's drawing, 1.6 to 2.1 s went to opening the plugins and archives again, 1.0 to 2.4 s to
finding the cells about the square (every exterior cell of the world read for its grid), 0.7 to 2.3 s to the cell's
references and models, 0.4 to 1.2 s to the cast's, the dormant references' and the platforms' reads, 0.3 to 1.6 s to
the draw list, 0.2 s to the terrain and 0.3 s to the upload's conversion. The scripted window's cell loads now run on
the play data thread, which opens the game data once as the run starts and keeps it for the cast's builds and the
cell loads alike (`fo4_view::play::data_worker`, `load_scene_with`); a load gives what a load that opens the data
gives; a cell load waits for a cast build in progress. The nine loads of the same run: 7.64, 7.44, 7.70, 6.35, 5.71,
6.17, 3.60, 5.91 and 4.63 s before, 5.80, 5.64, 5.68, 4.58, 3.83, 5.30, 1.87, 5.76 and 2.91 s after; the run's
outputs and the 15 frames byte for byte the same; resident memory 2.28 GB before, 2.71 GB after, footprint 4.30 and
4.00 GB. One set of open archives for both: a first try with a thread of its own for the cell loads kept three
sets open (186 archive handles, 248 files, 285 at the start) and the window started from its app bundle, under
launchd's limit of 256 open files, lost its run at the start (its error was not kept: stdout and stderr went to one
file; the limit is the likely cause, not confirmed); with the shared thread the window holds two (124 handles, 178 files, 215 at
the start), and no load opens a third as each did before. Not cut: the grid search (a world's grid index read once,
`fo4_scene::grid::WorldGrid`, answers for one centre's own grid only, so it does not stand for `grid::around` at
every centre as it is), and the models both sets of cells share (the loader is made anew per load).

**The cast's placeholder shapes, by reason** (the window log's `play: cast:` lines of the 200 s late window run, the
whole opening; distinct actors per reason): a leveled list leaving two or more candidates (races, skins, outfits or
sections) 27, a template leaving two outfits 8 (the faces step's readings `--actor-leveled-seed` and
`--actor-template-bits` choose them; they are off in the preset), a model citing a material path without its root
folder together with a texture no archive holds 9 (`fo4_nif_material_data_relative_v1` rejects such a path by design,
`spec/citation-roots.md`: no repair), a skinned shape with an effect falloff 2 (posed normals: skinning work, not
started), no skin, no race skin and no outfit 2 (no model to draw), a transform controller the build does not run 1.
None is a defect against an existing rule; nothing changed. Whether the preset takes the faces step's two readings
is the owner's choice.

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
- A person's keys drive `play_run`'s walker in the window (`--play-keys`), checked by the synthetic person, the
  unit tests and a tester through a desktop-automation tool (the person and way-out steps), not yet by a person
  at a physical keyboard and mouse. Of the run's caller assumptions the front door's open and the exit's two
  activations are the person's (`--play-activate-opens`, `--play-activate-person`); the earlier vault
  activations, the talks, the actor moves and the player's animation events stay timed (held until their
  stages; with `--play-action-after-offset` each keeps its offset from its stage, so a late person gets them
  spaced as the caller wrote them; the actor moves and ungated actions stay at their seconds).
- A grid square's drawing is loaded whole by the window (2.0 to 5.9 s since the gate step, the game data kept open; before, 5.6 to 8.1 s, the store opened again; the cells
  scanned, the models and textures read), overlapped with the walker's build since the person step.
- The reach of an activation, the walker's speed and its tick are the caller's; the activation key and the
  crosshair are the eye's ray (the prompt names what the ray meets; no activation distance from the data).
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
| `play_activate_opens_v1` | `--play-activate-opens REF` | the run's own open event and the person's activation; that activating a door opens it is prior knowledge | weak |
| `frame_exposure_seen_visible_v1` | `fo4-view --exposure-eye-visible` | chosen: the data stores no exposure; that an eye adapts to what it sees is prior knowledge | weak |
| `play_activate_person_v1` | `--play-activate-person REF` | the run's own activation and the person's E; that activating a console uses it is prior knowledge | weak |
| `play_activate_ray_first_met_v1` | `--play-pick-first-met` | measured on the session's record: the gear door's bounds hold the walkway; that a crosshair names the surface in front of the eye is prior knowledge | weak |
| `caller_action_after_keeps_offset_v1` | `quest_run --action-after-offset` (`--play-action-after-offset`) | how the opening's gates are written (each names the stage the caller's previous action sets); a reading of the caller's own timed assumptions | weak |
| `play_activate_written_second_v1` | `--play-action-after-offset` | the person's activation and the caller's timed one are the same event of the story; that the caller's later events follow it by the written gaps is prior knowledge of how they were timed | weak |
