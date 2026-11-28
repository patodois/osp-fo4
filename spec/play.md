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

**The pad (`--play-pad`).** Viewer policy, no rule; none of it is measured game behaviour (`docs/unmeasured.md`).
With `--play-pad` (a viewer built with `--features pad`) the window also reads a GIP pad over USB (the `gip-pad`
crate's reader, on its own thread; `crate::pad`), and turns it into the same held keys, looks and presses the key
map gives (`crate::person`), so the recording keeps its format and a pad session replays as a keyboard session
does. The numbers are our choice: the left stick past a radial dead zone of 0.24 (its length rescaled from there to
0..1) holds forward, back, left and right by its direction in eight sectors of 45 degrees (a key on past 0.383 of
the unit direction, off below 0.333, so a stick near a sector's edge does not flicker into the record) and run from
0.90 of full tilt (off below 0.80): a partial tilt walks, a full one runs, at the walker's two speeds; the stick's
speed between them is not the game's measured behaviour. The right stick past a dead zone of 0.20 turns at up to
120 degrees a second and looks up or down at up to 75, times the square of its deflection, sent as `face` and
`pitch` as the arrow keys' are, none while paused. A activates (on its press), Menu pauses and resumes, View or B
held one second ends the input (B is the thumb's resting button). Keyboard and pad play together: held keys of
either, looks added, presses of either. A window without the focus, or no pad, reads as nothing held or pressed,
and a button held through it presses again only after it is let go. The reader opens the pad with a USB reset and
claims it (`--play-pad-no-reset` opens it without the reset); one program at a time holds it, so any other that
does is closed first. No pad at the start, or one unplugged: the keyboard plays, the reader keeps looking on its
own thread, and one line tells each change (`play: pad: connected`, `play: pad: none (why)`).

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
near it, and `aim-at X Y Z` lets W go and keeps the view on a point from the walker's eye). Its lines `pad LX LY
RX RY [a] [b] [menu] [view]` (sticks in -1 to 1, x right, y up, the buttons named held) and `pad off` are a scripted
pad (`pad::FakePad`): each state holds from its second of the play clock, read where the device's state is read,
so the pad's key map is checked without a pad. Its events are timed
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
  objective text, read through its string table; when that text is empty, the quest's display name: the
  opening's four objectives with a text name string table entries that hold an empty string, 4 of the 15 empty
  entries among the main table's 43,730, so before the deck step the line stayed hidden from the evacuation to
  the platform; `fo4_play::state::Texts::objective_line`, viewer policy) in the top left corner; after E,
  `opened`, `used` or
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

### The platform waits for every arrival (the deck step)

Both play sessions after the gate step stopped on the raised descent platform: the person boarded it (the opening
quest's 495 at 860.3 and 633.3 s) and 500 never followed. The boarding handler counts the arrivals of the seven
references stage 470's fragment registers for distance events about the platform's centre (the player, the spouse
and five other actors) and sets 500 at the seventh. The spouse never arrived: the caller's two moves of her (`--assume-actor-at`, written at
420 and 466 s, where the reference run's spouse walks to the platform) came before the checkpoint's stage whose
fragment moves her to the escort marker (465, at 692.2 s in session A, 517.2 s in session B; 366 s in the reference
run), and a native move after an assumed one puts the actor where the native put her (`world_actor_placed_position_v1`),
so she stood at the escort marker for the rest of the run. Not a regression of the way-out or gate steps: the same
records replayed without `--play-action-after-offset`, and with the way-out step's `play_run`, skip 500 as well; the
person step's session B reached 500 only because its 465 came at 465.70 s, 0.3 s before the second move.

With `quest_run --action-after-actor-moves` (`caller_actor_move_after_v1`, off by default) an `--action-after` gate
also names the assumed moves of its actor at its second; the deck step's run arguments (`track/open3-graph.args` in its
notes) hold both moves of the spouse until 465. Under `--action-after-offset` each keeps its offset from the stage:
465 is done in the turn of the checkpoint talk written at 366 s, so the moves come 54 and 100 s after the person's 465.
Session B's record now gives 465 at 517.2 s, her moves at 571.2 and 617.2 s, 495 and 500 at 633.28 s, the descent
(510 at 642.3 s) and the vault, 680 and 690 at 743 s, to the end of the input; session A's 465 at 692.2 s, 495 and 500
at 860.29 s, 510 at 869.3 s and the vault to 970.7 s, to the end of its input. The on-time person is unchanged (the
same 276 stages at the same seconds), and so are the gate step's 200 s and 400 s late runs. Session B's record
replayed in the window gives the headless outputs byte for byte, down the platform at 633.28 s.

**The sweep.** From an on-time synthetic person (the front door, the device and the console the person's E), at each
of the 25 stops of the opening three persons: late (every command from the stop on 60 s later), early (every command
from the stop on earlier by the time the walker stood there, at most 60 s) and arriving early (only the walk to the
next stop earlier, the presses and the rest on the script's time); 66 headless runs. Late: 24 of 25 reach the reference
run's 276 stages and ride the exit elevator up; the one left (60 s late from the checkpoint's last stop) rides the
platform down but its ride ends without the load door taking the walker, a ride-end edge of the play mode (the same
person 8 s earlier at that stop goes through; their positions differ by a ten-thousandth of a unit). Arriving early:
12 of 20 go through, and two of the eight left are the sweep's own fault (it moved a walk away onto the second of the
device's or the console's E). Early: 3 of 20; most of the rest are the synthetic person's (below). Still stalling, not covered by the caller's moves: the escort scene at the checkpoint, whose
first phase waits on the player's distance, never completes for a person standing at the meeting point before the
checkpoint talk starts it (arriving 19 s early), and its later phases' package moves of two or five actors are
dropped when the person boards the platform before them (7.5 and 19.5 s early): fewer than seven arrivals, no
descent; a person ahead of the story after waking crosses the trigger of the follow-on quest's 3 before its stage
(60 s early) and never crosses it again; a person ahead of the story in the prewar vault falls through its floor (the
walk from the nursery or the evacuation's start 47 to 57 s early); a press of E before its stage (the crib, the device,
the console) is pressed once by the synthetic person. Early persons also leave the house before the broadcast and
never come back (the broadcast waits for the hallway, as session A's person found), and walk off the platform or into
the vault's closed inner gate with commands written for a later moment; neither is a stall of the run.

**The objective.** The run displays the opening quest's objective from the evacuation (296.97 s in session B) to the
platform; its text names a string table entry that holds an empty string, so the viewport drew nothing. It now shows
the quest's name in the top left corner (the frame at 315 s of session B's window replay). Before the evacuation the
run displays no objective: the nursery shows none.

**The first three crossings.** Their 3 by 3 squares hold 1,956, 1,911 and 1,879 placed instances (the house's square
alone 711) against 1,170, 779 and 1,170 for the next three. In session B's window replay the loads took 5.86, 5.64 and
5.72 s, then 4.55, 3.86 and 4.52 s; the step that grows with the instances is joining the cells around (the
`around 1` line: 1.89, 1.99 and 2.34 s against 1.06 to 1.08 s), then the draw list. The grid search itself takes 0.07
to 0.09 s (timed alone: the gate step's reading that it cost 1.0 to 2.4 s timed the search and the join together).
Session A's first crossing (11.0 s) also waited about 2.8 s for a cast build the window had asked for at the
crossing's second on the one play data thread (a race between the run's change and the walker's crossing; session B's
replay has none). Not cut: a window that asks for no cast build while a crossing is under way was tried and left out
(it moved cast builds by a frame and changed 3 of the 15 frames of the scripted window run, with no wait in the
replays to show for it); the join and the draw list of the overlapping cells, read again at each load (the loader is
per load); the store's types are not shared between threads, so the cast cannot get a thread of its own without a
second set of open files.

### A person ahead of the story (the early step)

The deck step's sweep left six early-arrival stalls and one missed ride end. Each was traced in the run's names file
and the walker's track; three are covered by new readings, each behind its own switch (off by default, on in the
play preset's run arguments or play options), and three are the game's own waits as far as the data tells.

- **At the checkpoint before the guard's talk ends** (arriving 19.4 s early). The escort scene's first phase completes
  on the stage the guard's hello line sets at its end; that line holds only from a stage the checkpoint's scene sets
  while the person waits (357.7 s). The early person entered the guard's hello distance at 351.3 s, before that stage
  (no line held), and stayed inside; a hello is offered only on an entry, so the line never played. `quest_run
  --hello-owed` (`dialogue_hello_owed_v1`): an entry that played no line stays owed while the player stays inside and
  is offered again at the first settle after the quest log grows. The line now plays at 357.7 s and its end sets the
  escort's stage at 364.7 s (the reference run: 377.7 s); the person goes through (the reference run's 276 stages).
- **Boarding the platform 19.5 s before the others.** The boarding trigger stops the escort scene; its second wave of
  package moves (two actors, due 1.3 s later under the caller's 10 s) was dropped with the scene, so six of the seven
  registered arrivals came. `quest_run --package-moves-outlive-scene` (`pack_destination_outlives_scene_v1`): moves
  already scheduled stand when a script stops the scene. The person now goes through (276 stages, the descent 19.5 s
  earlier).
- **The ride's end missed** (60 s late from the checkpoint's last stop). A temporary trace of the ride end showed the
  walker, pushing into the shaft wall, off the deck 0.25 s before the ride's last key and falling 2 units above it
  when the key passed: the end is noted only when the walker stands on the piece at that tick. `play_run
  --play-elevator-door-landing` (`play_elevator_end_landing_v1`): such a ride ends for the walker when it next stands
  on the piece. The person now goes through (276 stages).
- **The rider pushed into the shaft's rock** (the 25th session, both records, the last 2 s of the descent). A
  temporary trace of the second record's replay: the walker stood on the deck's rigid part the whole way, no key
  held, and from 1159.66 s the terrain over the shaft (the deck passes about 300 units below its surface) touched
  the capsule; its push-out moved the walker 177 units sideways in 1.7 s, the last 0.7 s in jumps of 3 to 30 units
  a tick between opposite terrain normals, and the eye went with it into the rock. `ride_check` had found the same
  in its own trace and named `walk_ride_rider_passes_terrain_v1`; the play mode had not read it. `play_run
  --play-elevator-through-terrain` (in the preset) sets `pass_terrain` on every ride the named platforms put into
  the walking world: while the walker stands on a piece the terrain is hidden, and shown again when it steps off.
- **The rock drawn inside the shaft** (the 25th session, both records, the descent's last 5 s; round 27). The
  drawn terrain itself: the cell's stored heights lower the terrain inside the entrance's opening into a cone about
  3,700 units deep whose bottom lies about 5,200 units high under the deck, and the deck rides down to about 4,990;
  the cone's walls stand inside the shaft model's walls. No read field holes it: the cell's quad bits are clear, the
  LAND holds no uninterpreted subrecord, its unknown bytes are the usual ones, and the quadrant's five landscape
  textures are plain ground. `fo4-view --draw-ride-cuts-terrain` (`play_ride_terrain_cut_v1`, in the preset): while
  the walker stands on a named platform's moving part (the ground its feet stand on is the part's ride piece:
  `fo4_play::elevator::PartMotion::carries`), every terrain triangle whose horizontal extent meets that part's
  collision box where it stands is left out of the window's drawing (`fo4-view`'s `scene::ride_cut` and
  `scene::terrain_kept`); the terrain is whole again once the walker stands on no moving part. The run, the walking
  world and the headless frames are unchanged.
- **The cryo aisle's empty bays** (the 25th session's second record; round 27). All thirteen pods of the aisle (twelve
  of one furniture base, the player's of another) were refused at render readiness, the preset's furniture switches
  on. Their record blockers besides the fields `--draw-furniture-fields` sets aside: OBTE (a 32-bit count of 1), OBTS
  (one 18-byte object template entry whose include and property counts are zero, closed by an empty STOP: it names no
  part to add) and, on the player's base, ATTX (a 4-byte localized string: text). `--draw-furniture-template`
  (`furn_template_text_render_unchanged_v1`) sets them aside for the drawing. Their model blockers (particle systems
  and their controllers, the door sequences, a behaviour graph's extra data, blocks the reader does not decode) are
  left to `--draw-furniture-parts` (`furn_unmoved_parts_drawn_v1`, now in the preset), whose reading leaves out every
  shape a controller or sequence can drive: on the twelve pods of one base the door sequences reach every shape, none
  was kept and they stayed refused. `--draw-furniture-at-rest` (`furn_parts_drawn_at_rest_v1`, with it) reads a
  furniture's parts at rest: the driven shapes drawn at their stored transforms, as the rest rules draw an idle
  manager's model; the particle systems and the shapes reading an open blocker still left out.
- **The objective that points at the player's pod is never shown** (the 25th session's second record; round 27,
  explained, nothing changed). The quest's stage fragments, in the game's order: stage 630 shows objective 130, 680
  completes it and shows 140, 700 completes 140 and shows 145, and 710 (the end of the comfort scene) completes 145
  and shows 150, whose target is the player's pod's alias, only when stage 720 (the player using the pod) is not done.
  In every recorded session 710 comes only from the pod's own activation handler, which sets 720 first, so 150 is
  completed and never shown. In the second record of the 25th session the chain stopped much earlier: the quest's
  stage 410 was never set (the person entered the broadcast trigger only before its prerequisite stage 400, set at
  685 s, and the default trigger script sets its stage only on an entry made after the prerequisite), so the hill
  quest's stage 10 and its stage 500 never came, the marker those stages disable stayed enabled, and the five
  neighbours that ride the elevator, children of that marker with the opposite-state flag, stayed disabled: in the
  vault their MoveTo does not load them (`world_load_reference_enters_v1` loads only an enabled reference, a temporary
  trace of the run's loads showed it), their OnLoad handlers never register the animation events the caller's
  timed actions wait on, stages 644 to 646 never come, and the caller's activation of the suit giver, gated on 646,
  never runs. That is the data and the person's path under the rules the run already reads, not a fault found in
  the run; no cue was written this round.
- **Walking through the checkpoint gate 7.5 s early.** The person crossed the two stage triggers past the gate before
  the guard's talk set their prerequisite, and the default trigger script sets its stage only on an entry made after
  the prerequisite: the escort's later phases wait for them forever. The data says the game would not let the person
  there: the gate stores a collision marker's box (a modelless static's box primitive), and the stage the guard's
  line sets disables it. The play mode's walking world places no such box. `--play-marker-boxes`
  (`play_collision_marker_boxes_v1`) places the boxes whose base carries the obstacle flag (the light volumes' base,
  which stores the same primitive, does not) and lets the live world take them out when the run disables them; the
  activation's ray passes through them. It is not in the preset: the house's front door has such a box too, removed
  when the door's opening stage comes, and every recorded input (the synthetic persons' and the three human
  sessions') walks up to the door through where the box stands, so the walker stops 70 units short and the E at the
  door meets nothing. With the switch off this stall stays. (The gate-box step: the front door's box stores another
  collision layer word than the gate's and is left out under `--play-marker-box-layers`, both now in the preset;
  below, The front door's box and its layer.)
- **Leaving the nursery or the evacuation's start 47 to 57 s early** (the "ahead of the story" pair). The person
  entered the hallway's trigger before its prerequisite stage and stood inside; the stage came with a later entry,
  and on the scripted time the person then crossed the street's stage triggers before the broadcast set theirs. Each
  is the default trigger script's rule (an entry before the prerequisite does nothing): the game also waits for a
  later entry. The fall out of the world that follows: with the story stalled the run never seats the player in the
  pod, the walker stays in the prewar vault and walks commands written for the postwar vault; at the end of the room
  east of the cryo hall it walks through a door into the part of the cell nobody built. The postwar cell places a
  hall end piece there; the prewar cell places only the door, whose collision is refused (the door record's `FNAM`
  layout is not read: every placed door, `spec/play.md`, Doors). Not a reading defect of this step; nothing changed.
- **After waking, 60 s early.** The sweep moved the walk 60 s back from the stop, to before the wake (812 s): the
  walker walked in the dream room while the run held the player in the pod, never reached the follow-on quest's
  trigger, and nothing was crossed before its stage. The same walk moved back only to the wake (21.65 s) goes through.
  The game holds the player in the pod; the play mode's walker is not held while the run seats the player (a gap,
  not covered here).

**The crossings.** The fifth session measured 4.7 to 9.6 s at the six outdoor crossings, the deck step's replay 4.6
to 7.1 s, with the same viewer code. The window logs show every phase slower in the session by about the same factor
(the cells-around join 3.6 to 4.1 s against 1.9 to 2.3 s, the rest of each load likewise) and the cast builds on the
same thread too (3.4 to 6.7 s against 2.0 to 2.8 s for the same work): the machine was busier during the session (the
tester's own tools, 41 % of memory free), not the build. Replayed on a quiet machine to 700 s and measured as the
session's report measures (from the first still frame before the scene's change to the first frame of the new one):
6.49, 6.10, 6.28, 4.84, 4.00 and 4.55 s. Each load read its 3 by 3 squares' models, materials and textures from the
archives again, six of the nine squares the same as the load before. The window's cell loads on the play data thread
now keep the scene assets one load read for the next (`load::KeptAssets`, viewer policy; a load whose path policy
and clock switches differ, or assets above 768 MiB, start empty): 6.11, 3.35, 3.17, 2.76, 2.22 and 2.30 s (the
cells-around join 0.08 to 0.41 s after the first); the run's outputs are the same byte for byte and the window's
resident peak 2.23 GB against 2.38 GB. Frames of a person's record replayed in the window are not a reference: their
exposure is measured at seconds that follow the window's frames (283.051 against 283.069 s in two runs), so five of
48 differ in brightness between any two runs.

### A person out of a closed door (the board step)

The third session of the day stopped on the raised descent platform with 495 and neither 490 nor 500; the same stall
as in the fourth attempt's report, which had a different cause (the spouse, the deck step). Replayed headless from its
record, byte for byte, the stall is upstream of the platform:

- **The platform needs the street's chain.** The platform's own trigger sets the second quest's 91, and with it the
  opening's 490, only when that quest's first stage is done; the boarding handler sets 495 on the player's arrival and
  500 when its count of arrivals reaches the seven that stage 470's fragment registers. The record gives two arrivals
  (the player at 446.3 s, the spouse at 466 s): the five others are the crowd, which the second quest's chain of
  scenes walks to the platform. That chain starts with its first stage, set only by the fragment of the opening's 410,
  which only the trigger in front of the house sets, on an entry after 400 (the default trigger script).
- **The person left the house 33 s before 400.** The house's front door is closed by the fragment of the stage that
  ends the salesman's visit (117.9 s in the session) and its activation blocked from the stages before; the
  fragment of 400 (248.07 s), the evacuation, opens it and unblocks it. The person walked through it at 214.9 s
  (the play mode places no collision for doors), stood in the trigger of 410 from 215.3 to 231.1 s, before its
  prerequisite, crossed the next two triggers of the street (the chain's 20 and 120) before the chain's first stage,
  and never came back: 410 and the whole chain were never set. A caller's assumed entry into that trigger at 249 s
  confirms it: 410, the chain from 10 and, at the platform, 490 at 445.7 s and 495 at 446.3 s; still no 500.
- **And walked through the checkpoint's gate 6.3 s before 470.** The two stage triggers past the gate (the chain's
  350 and 80, whose stages move the escort scene's actors to the platform) were crossed at 382.0 and 382.8 s,
  before 470 (388.3 s) set their prerequisite: the collision marker's box across the gate, which 470 disables, is
  not placed by the preset (`play_collision_marker_boxes_v1`, the early step).

`--play-held-doors` (`play_door_closed_blocked_holds_v1`, off by default, in the play preset): the play mode reads the
run's `SetOpen` and `BlockActivation` calls (`fo4_play::live::RunState::doors`), and a loaded door that the scripts last
closed with its activation blocked stands in the walking world as the box of its base's object bounds placed by the
reference (`world::Built::held`, kept in the live world's dormant piece and shown by `Live::apply`), until a script
opens it, unblocks it or disables it; the activation's ray passes through it. In the opening two doors are held: the
house's front door from the end of the salesman's visit to the evacuation (117.95 to 248.14 s in the session's
replay) and the nursery's door between the two stages that close and open it.

What the records give with the switch:

- The third session's record: the walker is held at the front door from 214.94 s (the first point where its track
  leaves the recorded one) until the door opens; the rest of the record, written for a person already down the
  street, then walks it back into the house: the run stops at 402, short of the stall it had. A record made through
  the door cannot replay past it.
- The same person waiting at the door (the record's commands from 214.93 s moved 34.1 s later, the keys released in
  between): 410 at 249.4 s, the street's chain, 490 at 479.8 s and 495 at 480.4 s; no 500, the gate crossed 6.4 s
  before 470. Waiting also at the gate (from 415.96 s, 8 s): 350 and 80 at 424.1 and 424.9 s, 490 at 487.8 s, 495 and
  500 at 488.4 s, the descent (510 at 497.4 s) and the vault below (600 at 522.9 s).
- The second session's record (normal pace) gives the same outputs byte for byte; only the play report gains the
  switch's lines. The first session's record leaves the recorded track at 329.98 s: its person stepped out of the
  closed door during the broadcast and came back (the session's report: a premature trip to the doorstep), and the
  record's later commands walk the held walker elsewhere. Without the switch every record replays as before.

Not covered here: the gate (the marker boxes stayed out of the preset: under them the on-time synthetic person is
stopped 144 units from the front door at the salesman's visit, beyond the activation's reach, so the door cannot be
opened; the gate-box step below reads why). Doors the scripts close without blocking their activation, and doors closed as
placed, still place nothing (the door's open state and a person's opening of it are not read).

### The front door's box and its layer (the gate-box step)

The board step left the checkpoint's box out of the preset because, under `--play-marker-boxes`, the house's front
door's own collision marker box held the on-time synthetic person 144 units from the door at the salesman's visit,
beyond the 150-unit reach the caller gives. Read from the records and this project's code only:

- **The box is built as the data places it.** Its position is its `DATA` position, 52 units from the door's centre
  and 49 units into the room along the door's normal: no rotation reading moves a box's centre, so the box stands in
  the room in front of the door whatever the order of its angles. Its three stored angles (about -13.5, -10.8 and
  322.1 degrees) turned under the measured placement convention (`refr_rotation_zyx_v2`) give one tilt of 17.2
  degrees about a horizontal axis along the door's width (the box's own x axis stays level), the plainest shape an
  author's turn makes; the other order gives a box tilted about two axes. `XPRM` holds the three bound words, a
  colour, one more float and the kind word: no offset of the primitive. Read as half extents (98 by 35 by 145, the
  reading the vault shaft's platform box and the trigger census support) it spans 4 to 111 units into the room at
  standing heights; the walker's capsule (radius 16) is held 107 units from the door's face. Read as full extents,
  or with the other rotation order, the box still covers the point every recorded person stood at to open the door
  (42 units from it).
- **It stands at the visit.** The reference is persistent, stores no enable parent and is not initially disabled;
  the run disables it at 243.5 s, after the visit (90 s).
- **The capsule and the reach are not the cause.** With radius 0 the walker would still stand 95 units from the
  door's face; from where it is held a person aiming at the door meets it within 150 units (110 to its near edge,
  136 to its centre: the activation's ray passes through marker boxes). The synthetic person's activation is
  written as a fixed look for its own standing point, inside the box, and from the held point that look leaves the
  door 220 units away.
- **It stores another collision layer than the gate's box.** The reference's `XTRI` word (xEdit's published name for
  the subrecord: the collision layer; `spec/record-layouts.tsv`) is 40 on the front door's box and 35 on the
  checkpoint gate's. Over the load order (`gate_probe` census, a temporary probe kept outside the repository), of
  the 2,234 boxes whose base carries the obstacle flag 942 store no `XTRI`; 35 is stored by 2, 40 by 2 (the door's
  and an initially disabled box beside it), 44 by 1, 49 by 860 (620 of them initially disabled or under an enable
  parent), 31 by 208 (131 of them tilted, against 270 of the 942 without a word), the rest 0, 1, 3, 15, 17, 22, 26,
  27, 28 and 53. In the engine's layer list as prior knowledge has it, 35 is the collision box, 40 the item pick (a
  layer only the activation's ray tests), 31 the stair helper, 49 the navigation cutter; the census's tilted stair
  helpers and enable-parented cutters (navigation cutters of buildable places) fit that numbering. With the door's
  box left out, the on-time person then stopped at the vault exit: a box 133 units tall across the way to the exit's
  elevator, word 49, whose editor id names a navigation cutter.

`--play-marker-box-layers` (`play_collision_marker_box_layer_passed_v1`, off by default, needs
`--play-marker-boxes`; `world::PASSED_LAYERS`, `world::marker_box_left_out`, `MarkerBox::layer`): a box whose
reference stores the word of a layer a walking character passes (the ray picks' 39 to 44 and the navigation cutter's
49) stays out of the walking world, counted in `play.txt`; a box storing no word or another word stands as before.
In the opening it leaves out the front door's box and the vault exit's; the gate's box (35) stands, and so do the
eleven boxes about the start that store no word. Both switches are now in the play preset.

What the records give (outputs outside git):

- The three person records with the new switch off: every output byte for byte as recorded (play.txt apart from
  its timings), as in the board step.
- The on-time synthetic person under the board step's preset and under this one: the same 276 stages in the same
  order to the vault exit. Six seconds at the checkpoint move by at most 4.4 ms: that person's own standing point
  there lies 4.5 units inside the gate's box, so the walker is held against the box from 370.97 s until the run
  disables it at 377.67 s (the stage that opens the way) and then takes the 4.5 units. Under `--play-marker-boxes`
  without the layer reading the same person stalls at the door (144 stages), and with the door's box alone left out
  (pick layers only) it stalled at the vault exit's box (263 stages).
- The board step's waiting person (the stalled session's record held 34.1 s at the door and 8 s at the gate) gives
  the board step's 220 stages byte for byte in their seconds, 500 at 488.42 s.
- The same person held at the door only (the gate case): the gate's box holds the walker from 416.07 s, but the
  walker slides along it, round its end, and enters the stage triggers past the gate from 417.51 s (the last at
  418.60 s), 3.8 s before the stage that disables the box (422.41 s): no 500. The box spans 148 of the gate frame's 262 units;
  the rest of the frame is a static whose model carries a transform controller (rest `transform-varies`), its
  collision refused, so nothing stands where the walker goes round (the refusal was read here as
  `UnsupportedAnimationRuntime`; the gate-frame step found it to be the reference's four unread fields, below).
  Placed as stored as a diagnostic (`--play-elevator` naming that static, a caller's option, not in the preset), it
  closes the gap: the walker is held until 422.41 s and crosses neither trigger early; the record's later commands,
  written for a person already through the gate, then walk elsewhere (192 stages, no 500), as the board step's
  record made through the door did. The deck step's synthetic person arriving early at the gate does the same under
  that diagnostic (held until 377.67 s, the two stages at 377.84 and 378.88 s, after the one that opens the way);
  its later timed steering, cutting toward a point it is now late for, sticks on the way to the platform.
- The deck step's sweep of 20 early and 20 arriving-early synthetic persons against the board step's runs:
  nobody who went through before is lost. Out of the vault: arriving
  early 14 of 20 and early 3 of 20, as before; the descent's stage reached by 17 and 15, as before. 37 of the 40
  set the same stages in the same order, their seconds at most 4.4 ms apart (the checkpoint hold above); the three
  that reach the gate before the stage that opens it (arriving early there, early from it and from the stop before)
  are held at the box, go round its end and, late for their timed steering, no longer reach the raised platform
  (196 stages instead of 197; none went through before).

### The checkpoint gate's frame and the layer records (the gate-frame step)

The gate-box step left the gate case open: a walker sliding along the checkpoint's box goes round its end, where
the gate's frame stands drawn but placed no collision. Read from the records, the model and this project's own
code only:

- **The frame's collision is not moved by its animation.** The frame is a static whose model holds one transform
  controller (free-running, active, clamped, from 0.2 to 4.33 s). Its keys turn one node, the gate's leaf, about
  its hinge: from the stored pose (the key at load equals the stored rotation) about 94 degrees and back to the
  stored pose at the last key. The model's one collision object hangs on the model's root, and its body reads the
  root alone: nothing moves a block of its chain. The controller's three blockers therefore lie off the collision
  chain (`readiness::Bearing::OffChain`), and `--play-collision-off-chain`, already in the preset, sets them aside.
  The collision holds the two side panels and one hull 110 units long standing across the fence line from the
  north panel's end, beyond the fence (the leaf's hinge is there). The 123-unit gap between the panels lies
  inside the box's span.
- **What kept the frame out is the reference.** Its reference stores four subrecords this project does not read
  as placement: `XACT` and `XOWN` (bytes unread; `UninterpretedVisualSubrecord`) and `XLRT` and `ONAM` (read as
  run-time state; `RuntimeRecordState`). No rule covers them, so the collision was refused. The gate-box step's
  attribution of the refusal to `UnsupportedAnimationRuntime` was wrong: that blocker was set aside already.
- **Census of animated models over the load order** (a temporary probe; placements are enabled placed references,
  counted through their bases). There are 161 models with a free transform controller whose keys leave the stored
  transform and a placement:
  - 49 place no collision.
  - 49 move only nodes with no collision under them: 2,489 placements, 1,450 of them statics. The gate's frame
    is one of these. The off-chain reading already admits them.
  - 4 have a controller off the chain while another block moves the chain.
  - 59 have a controller that moves a collision chain node: 1,541 placements, 108 of them statics in 7 models.
    Weighted by placements per moving controller: 1,679 start and settle at the stored transform (clamped),
    661 start there and settle elsewhere, 53 neither start nor settle there, 69 loop, and 9 have a track the
    clock does not read. Among the statics, 56 placements start and settle at the stored pose and 53 do
    neither.
  - A rule placing chain-moved collision at its stored transform would put solid shapes where such a model's
    animation stands elsewhere, so none was added. The frame does not need one.
- **Census of the four fields over the load order.** 4,772 placed references of statics store one of them: 4,663
  `XLRT` alone, 94 `XOWN` alone, 14 `XACT` with `ONAM`, and 1 all four (the frame). For 4,735 of them the model
  places no collision (markers). With the preset's other collision readings, 36 are refused by these fields
  alone: 25 `XLRT` alone, 6 `XOWN` alone, 4 `XACT` with `ONAM` (static door models) and the frame; 35 are enabled
  and 1 is under an enable parent. One more stays refused by an unread `XRGD`. Among the 36 are the vault
  entrance's exterior shell and the frame, both on the opening's path. For other base types the fields are far
  more common (doors: 1,200 references; containers, furniture and activators carry ownership and location
  types). The reading is limited to statics.

`--play-collision-static-fields` (`refr_static_fields_collision_unchanged_v1`, off by default;
`readiness::Switches::static_fields`, `readiness::static_reference_field`, `readiness::static_base`): on a
reference whose base is a static, the record blockers of those four fields are set aside in every walking
world's cell build. The collision is then placed as the model stores it, under the rule. A reference whose
collision is refused by anything else stays out. The switch is in the play preset since this step.

What the records give (outputs outside git):

- The three person records with the new switches off: every output byte for byte as recorded (play.txt apart
  from its timings).
- The on-time synthetic person under the preset with the new switch: 276 stages, the same quests and stages in the
  same order to the vault exit. Six seconds at the checkpoint move by at most 48.7 ms: after the stage that
  disables the box, the walker meets the end of the frame's south panel on its way through.
- The gate case (the stalled session's record held at the door only): the walker is held at the box from
  416.07 s, slides along it and stops against the frame's north panel; it stays east of the gate until the stage
  that disables the box (422.41 s) and enters no trigger past the gate early. The track and the timeline are byte
  for byte those of the gate-box step's diagnostic, which named the frame with `--play-elevator`. It does not
  reach the descent's stage (500): 192 stages. The record's later commands were made by a person who had walked
  through the gate before that stage, so the walker, late against them, walks elsewhere, as the board step's
  record made through the door did. The waiting variant (held at the door and the gate) keeps its 220 stages in the same order,
  500 at 488.36 s (488.42 s before; seconds at most 0.23 s apart).
- The deck step's sweep of 20 early and 20 arriving-early synthetic persons, against the gate-box step's runs:
  nobody who went through before is lost. Out of the vault: arriving early 14 of 20 and early 3 of 20, as
  before; the descent's stage reached by 17 and 15, as before. 37 of the 40 set the same stages in the same order,
  their seconds at most 48.7 ms apart. The three that reach the gate before the stage that opens it (arriving
  early there, early from it and from the stop before) are now held at the box and the frame until that stage.
  They set the two stages past the gate 0.17 and 1.2 s after it, where before they set them after walking round
  the box. They still set 196 stages and do not reach the raised platform: their later timed steering, written
  for an earlier pass, sticks on the way. None of them went through before.

**The passed layers from the layer records.** The load order holds 57 collision layer records (`COLL`, layout
in `spec/record-layouts.tsv`, read by `fo4_plugin::coll`). They have indexes 0 to 56 once each. Each record
lists the layer records it collides with, and the lists are symmetric. The record of index 30 is the layer of a
walking character's body by prior knowledge. Its list holds 37 layers, among them the collision box's 35 and
the stair helper's 31. Its list also holds 39, 40, 41 and 43, which the constant `world::PASSED_LAYERS` reads as
passed. It does not hold 42, 44 or 49 (passed by the constant) or 6, 8, 15, 18, 19, 21, 23, 25, 32, 34, 36,
37, 47, 48, 50, 51 and 53.

So the derived set is not the constant. For the marker boxes the data stores:
- Word 40 (the house front door's box) would stand.
- Words 15 and 53 would be passed.

`--play-marker-box-layer-records` (`play_collision_marker_box_layer_records_v1`, off by default, needs
`--play-marker-box-layers`; `world::passed_layers_from_records`, `world::Passed`, `world::CHARACTER_LAYER`)
derives the passed set from the records at each walking world's build, with the constant as the fallback when
the load order has no record of index 30 or the records do not read. `play.txt` says which was used and how the
set differs from the constant. Under it the on-time synthetic person stops at the house's front door: 144 stages,
the same list in the same seconds as with every box standing. The switch is not in the preset, and the
preset's behaviour is unchanged.

Whether a listed pair makes a box solid to a walking character, or only to rays and other bodies, is not
settled by the records. The front door measurement of `play_collision_marker_box_layer_passed_v1` decides
between the two readings.

### The front door shut from load, the checkpoint barrier and the catwalk holds (the door-shut step)

The seventh session (two people, the session's records replayed byte for byte) met three things. Read from the
records, the data and this project's code only (outputs outside git):

- **The front door was open until the visit ended.** Under `--play-held-doors` a door holds the walker only once
  the run's scripts have closed it (`SetOpen(false)`) and blocked its activation. The opening blocks the house
  front door's activation at its stage of 3.0 s, but no script closes the door before the stage that ends the
  salesman's visit (165.8 s in the first session), so the door had no open state and placed nothing until then:
  the early person walked out at 10.5 s, 43.6 s before the doorbell. The door's reference stores no `ONAM`
  (the field xEdit publishes as "Open by Default"; 6 of the 12 placed doors about the start store one). Nothing
  draws the door either: every placed door is refused at render readiness, so the doorway looks open all run.
- **Read as closed at load for every door, the reading goes too far.** With every held door storing no `ONAM`
  read as closed, the postwar vault's eight pod doors (from 730 s) and two vault doors (from 795 s) are held
  too: the run opens the pods by their animation, which the held doors' reading does not see, and the on-time
  synthetic person stops after waking (252 of 276 stages). The reading is therefore limited to the doors the
  person opens (`--play-activate-opens`): in the opening, the front door alone.

`--play-held-doors-at-load` (`play_door_closed_at_load_v1`, off by default, needs `--play-held-doors`;
`world::stores_onam`, `world::Built::closed_at_load`, `walker::persons_doors`, `Live::with_closed_at_load`,
`DoorState::holds_from_load`, `RunState::open_by_player`): a held door that the person opens and whose reference
stores no `ONAM` is closed until a script's `SetOpen` or the person's open of it (the activation that gives the run
the open) opens it. What the records give:

- The front door's box stands from 3.0 s, the stage that blocks its activation (from the run's first second the
  door is closed but its activation not yet blocked; the walker starts in another room).
- The on-time synthetic person: held 3.0 to 90 s (its activation at the bell opens it), again from 159.12 s (the
  visit's end) to 284.92 s (the evacuation); its track and timeline byte for byte those of the gate-frame step's run
  (276 stages to the surface).
- The first session's record (natural pace): track and timeline byte for byte the session's own, 272 stages, out
  of the vault (the follow-on quest's 10 at 1170.51 s); the door held 3.0 to 96.69 s, 165.83 to 378.04 s.
- The second session's record (early on purpose): the walker is held at the door at 10.43 s, the first point
  where its track leaves the recorded one; the record, made by a person already outside, then walks a held walker
  elsewhere and the run stops at stage 95 (143 stages), as the board step's record made through the door did.
- With the new switch off, the three records (both sessions and the on-time person) give every output byte for
  byte as before (`play.txt` apart from its timings).
- The deck step's sweep of 20 early and 20 arriving-early synthetic persons under the preset with the new switch,
  against the gate-frame step's runs: all 40 set the same stages in the same order at the same seconds; out of the
  vault arriving early 14 of 20 and early 3 of 20, the descent's stage reached by 17 and 15, as before. Nobody is
  lost, so `--play-held-doors-at-load` is in the play preset, with `--draw-held-doors`.

`--draw-held-doors` (`play_door_drawn_as_held_v1`, the window, off by default, needs the scripted play window with
`--play-held-doors`; `load::held_door_draws`, `scene::DoorInstance`, `gpu::follow_doors`,
`Walker::held_door_states`): each loaded door is drawn by its model as stored, through the named platforms'
drawing state (`fo4_scene::platform`), once the walker's held doors' reading has held it: as stored while its box
stands; after the box left, each shape posed at the last key of its model's sequence named Open, turned into the
world by the door's placement; a door the reading never held, or one the run disabled, stays undrawn. The front
door's model stores its leaf unturned; its sequence Open turns the leaf about its vertical axis to 1.560 rad (about
89 degrees) and Close back to 0.001 rad, so the stored pose is read as shut. In the window (the on-time synthetic person's input in the scripted window
with the preset and both new switches, frames every second about the door's changes): the front door is drawn shut
from 3.1 s, open from 90.1 s, shut from 159.2 s and open from 285.0 s, each the frame after its box changed; the
nursery door shut from 227.9 s and open from 243.6 s; all 12 loaded doors about the start are drawn for the reading
(34 draws, each with an open pose); 60 fps median. The open leaf stands in the room, across the point the person
stood at to press E (the frame at 91 s looks into it): the sequence's sign, not measured.

- **The checkpoint barrier is two things, neither drawn.** The centre is held by the collision marker's box (its
  reference stores layer word 35; the stage that opens the way disables it), the ends by the gate frame static's
  two side panels. The box is a collision marker: it has no drawn model at all. The frame is refused at render
  readiness by the same four reference fields as its collision was before the gate-frame step (`XACT`, `XLRT`,
  `XOWN`, `ONAM`: `--play-collision-static-fields` sets them aside for collision only) and by its transform
  controller, which turns the gate's leaf, a drawn node (`target_in_visual`). Not done here: drawing the frame needs
  a render reading of those fields and of the leaf's controller (which plays from load: its keys go from the
  stored pose about 94 degrees and back), and the box has nothing to draw.
- **The catwalk holds are the exit mechanism's walkway, moved by its sequence and drawn still.** With
  `--play-report-walls` (a report of the play mode, no behaviour: each stretch of blocked ticks with what blocked
  it, `Live::describe_source`, `Elevator::describe_piece`) the second session's record gives, against the named
  exit mechanism's rigid parts while the run's graph plays the sequence it starts on it at 1051.70 s: 1052.87 to
  1054.57 s its part 6, the third walkway segment's base node (a face toward the walker; the walker's feet at x
  -3383.08, its capsule's radius short of it); 1087.30 to 1088.37 s part 3,
  the first segment's base node; 1088.63 to 1093.93 s part 5, the ramp's node (the walker at x -3512.63), released
  1.1 s before the run starts its next sequence. The window draws that walkway's grating as skinned shapes whose bones go with
  more than one part, so they stay as built (spanning x -3900 to -3399 at the catwalk's height plus about 50): the
  person saw grating ahead and nothing moving. The collision follows this project's reading of the sequence
  (`play_elevator_rigid_v1`); whether the game's walkway also stops a player while it moves is not known. Not
  changed: `play_platform_part_holds_walker_v1` (pending) records the reading.

### The checkpoint gate drawn and the walkway's grating moved (the draw-gate step)

The door-shut step left two things a person cannot see: the checkpoint gate's frame, refused at render readiness,
and the exit mechanism's walkway grating, drawn as built while its collision moves. Read from the records, the
model and this project's own code only (outputs outside git):

- **The four reference fields, for the drawing.** `--draw-static-fields` (`refr_static_fields_render_unchanged_v1`,
  off by default; `Loader::set_render_static_fields`) sets `XACT`, `XLRT`, `XOWN` and `ONAM` aside for render
  readiness on a reference whose base is a static, as `--play-collision-static-fields` does for collision. The
  views census of the opening (the drawn step's eleven views, `coverage_census --views --render-child-flag-agrees`
  with and without `--render-static-fields`): the rule alone newly draws no placement in any view, and none is no
  longer drawn; the outcome counts are byte for byte the same. Of the 195 placed references of those views that
  store one of the fields, the ones the fields alone refuse are items (`XLRT`, the item base types) and idle
  markers with no model (`XOWN`), not statics; the frame is refused by its leaf's controller as well. So no other
  static is drawn by it in the opening, and there was nothing else to look at for floating, doubled or blocking
  shapes.
- **The leaf.** The frame's model holds one controller, a free-running, clamped transform controller (0.2 to
  4.33 s) on its leaf node, with XYZ keys that turn the node about its vertical axis only, the first key the stored
  rotation; no collision object hangs at or below the node. `fo4_scene::leaf` reads such a model as a leaf with two
  poses, the stored one and the pose of the rotation key farthest from it (here the key at 1.667 s, 94.3 degrees
  away), without running the clock. Placed, the turned shapes' vertices average 54 units west of the hinge
  stored (the leaf standing out from the fence line at the north end of the gap) and 54 units north of it keyed
  (folded back along the north panel). Neither pose lies across the gap: under this project's reading of the
  node's rotation words and keys (the first key reproduces the stored words), the model's leaf never closes the
  way. What holds the gap's middle is the collision marker box alone, which has nothing to draw.
- `--draw-static-leaves` (`play_static_leaf_two_pose_v1`, off by default; `Loader::set_render_static_leaves`,
  `leaf::read`, `leaf::aside`, `leaf::key_motion`; the window's `load::leaf_draws`, `scene::LeafInstance::pose`,
  `gpu::follow_leaves`, `Walker::marker_box_states`): the leaf's controller blockers are set aside for render
  readiness and the static is drawn as stored; in the scripted play window, the collision marker boxes of the
  walker's world that meet the static's object bounds pick the pose: while one stands, the pose whose turned
  vertices lie nearer the boxes' centre (here the stored one, 82 units from it against 113), and once the run has
  taken them all out, the other. Together with `--draw-static-fields` the frame is drawn in the four views that
  load it, the only placement the two rules newly draw in the opening. That the stored pose stands for the way
  held and the folded one for the way let through is a guess from the placement, not from any record; the game
  may well leave the leaf still, the guards and the invisible box holding the way.
- **The walkway's grating.** A named platform's skinned shape is drawn on the model's own nodes; a shape whose
  bones go with more than one of the platform's parts stayed as built. `--draw-platform-skins`
  (`play_platform_skin_follows_parts_v1`, off by default, needs `--draw-platforms`;
  `fo4_scene::platform::skin_at_rest`, the window's `load::skin_draws`, `scene::skin_matrices`,
  `scene::SkinInstance`, `gpu::follow_skins`) reads each skinned draw's skin (`BSSkin::Instance` and
  `BSSkin::BoneData` through `fo4_nif::skin`, the vertex weights as stored) and its bones' matrices at rest, keeps
  it only if skinning it again at rest gives the draw's mesh vertex for vertex within 0.001 units, and each time
  the parts move skins it again with each bone's matrix moved by the rigid motion of the part its node chain goes
  with (a bone under no part keeps its rest matrix), replacing the mesh. No public format page was needed: the
  skin binding is read by the readers the cast already uses.

What the records give (outputs outside git):

- The scripted window with the three switches, the on-time synthetic person: the frame is drawn from the first load
  of the gate's cells (322.31 s), as stored while the box stands, folded from 378.07 s (the box disabled at
  377.67 s; the walker's next slice); 276 stages to the surface, the track and the timeline byte for byte the
  headless run's, every other output too but `play.txt` (the wording below); 60.0 fps median, every minute of the
  run at 60.
- Session A's record (the seventh session's first, natural pace) in the same window with the three switches:
  272 stages, out of the vault (the follow-on quest's 10 at
  1170.51 s), the track and the timeline byte for byte the headless run's under the preset's play options (the
  door-shut step's run of the record), every other output too but `play.txt` (the wording below); the frame drawn
  from the first load of the gate's cells (448.75 s), folded from 555.60 s (the box disabled at 555.51 s); the
  walkway's rails and grating joined under the walker as it crosses during the mechanism's sequence (1113 s); 60.0
  fps median, every minute of the run at 60.
- The same window run of the on-time person with every new switch off: every output as with them, and its frames
  byte for byte those of the run with them wherever neither the frame nor a skinned platform shape is in view (60
  of 121 frames; one-pixel differences in three more).
- Headless frames of the gate's cells from three fixed points: with the switches off this step's viewer gives the
  door-shut step's frames byte for byte; with them the frame stands on the fence line between the fence's posts,
  its side panels and leaf where the collision census put them, nothing floating, doubled or across the path; a
  probe outside the repository drawing the keyed pose shows the leaf folded against the north panel. In both poses
  the gap stays open.
- The skins: at rest the 5 skinned draws of the exterior elevator's model, and the 17 of the vault's gear door
  mechanism and inner elevator (in the prewar and the postwar vault alike), are skinned again vertex for vertex
  within 0.001 units (none refused); 3 and 12 of them have bones under more than one part. Against the run with the switch off: during the exterior
  descent the deck no longer stays overhead as a flat roof over the walker; in the prewar vault the walkway's
  grating lies between its rails instead of beside them, and a clamp no longer hangs in the air; at the exit the
  walkway's rails and grating appear where the walker stands on them (924 to 952 s), where before the view showed
  the gear room's wall. The cast's actors on the descending deck still stand where the timeline last put them (a
  standing actor fills the view at 486 s with the switch on or off).
- With every new switch off: the on-time person and both seventh-session records give every output byte for byte
  headless (`play.txt` apart from its timings).
- The regression sweep (the deck step's 20 early and 20 arriving-early synthetic persons, headless, the preset's play
  options, this commit's `play_run`), against the door-shut step's runs: all 40 set the same stages in the same
  order at the same seconds; out of the vault arriving early 14 of 20 and early 3 of 20, the descent's stage reached
  by 17 and 15, as before. Nobody is lost.
- `play.txt`'s held door line for a box held with no script's `SetOpen` (the front door at 3.0 s under
  `--play-held-doors-at-load`) now says closed at load in place of closed by the run; with that switch off no line
  changes.

So `--draw-static-fields`, `--draw-static-leaves` and `--draw-platform-skins` are in the play preset
(`apps/fo4-view/presets/play-opening.args`). They are read by the window's drawing only; `play_run` takes none of them.

### The post-wake corridor's fall and the front door's prompt (the corridor-fall step)

The eighth session's second record ended in a fall: after waking, a sideways step along the post-wake corridor's
end took the walker out of the world (eye height down to -78,079). Replayed headless from the record (the
track and the timeline byte for byte the session's), with `--play-report-walls` and a probe of the walking world
(`column_probe`, outputs outside git):

- **Why.** The walker stood in a vault doorway between the room's floor (its collision ends at one side of the
  doorway) and the hall end's floor (it starts 26 units further). Nothing was placed between them: the doorway's
  door is refused for collision like every door (its controllers, record fields, materials). The capsule (radius
  16) rested on the two edges; moving along the doorway, the edges' contacts turned steeper than the slope limit,
  pushed only sideways and cancelled, and the capsule sank between them and fell.
- **What the door holds.** The door's model has five collision bodies. One compressed mesh hangs under no node a
  sequence sets: the doorway's floor with a slot 26 units wide and 19 deep along the leaves' line, the two jambs and
  a head at 190 units. Four convex bodies hang under nodes its sequences named Open and Close set; at Open's last
  key (1.30 s) the main leaf rises into the head, a lower leaf sinks under the floor, and two flaps beside the slot
  fold flat across it, their top at the floor's height. With the frame alone the capsule sank into the slot and
  stayed there; with the frame and the parts at Open's last key the doorway's floor is whole and the jamb stops a
  sideways walk.
- `--play-door-open-collision` (`play_door_collision_open_pose_v1`, off by default; `world::Options::door_open`,
  `elevator::open_door_collision`, `elevator::OpenDoor`, `elevator::OPEN_SEQUENCE`, `fo4_walk::World::append`): a
  loaded door whose collision is not placed and whose model's collision has a frame (a part no sequence sets) and a
  part its sequence named Open moves stands in every walking world in that open pose, under the door's reference
  (the live world hides it with the door, the activation's ray meets it as the door). The parts are those the
  named platforms' rigid reading groups (`play_elevator_rigid_v1`), read here only to tell what moves. A door of
  leaves alone stays out: read for every door with an Open part, the house's inner doors stood open, undrawn,
  across the second record's walk at 59.7 s. 21 of the post-wake vault's 37 loaded doors, 6 of the prewar vault's
  21 and 1 of the start's 12 are read; the held doors' boxes and the doors' object bounds are unchanged, and the
  window does not draw a door it does not hold.

What the records give with the switch (this commit's `play_run`, the preset's play options, outputs outside git):

- The second record: 254 stages, every one at the same second as the session's; at 1275.17 s the sideways step is
  stopped at the jamb and the walker stays on the floor (its feet no lower than -0.01 units from 1244 s to the
  record's end, against -101,828 before) and walks on; the track first differs at 1085.28 s, when the walker crosses the
  descent's doorway level instead of dipping 2.8 units.
- The first record: 273 stages to the surface (the follow-on quest's 10 at 1177.25 s), every stage at the same second,
  the same last point; 287 track points differ, each a doorway crossed level.
- The third record: its recorded keys do not reach the surface. At 915.05 s it crossed a doorway where, before, the
  capsule dropped 1.7 units into the gap and the step-up carried it 11 units further than its keys; level, it stays
  11 units behind, and from there the open-loop keys run it 26 units south of the doorway it took at 933 s, into
  that room's wall, and the exit console's activation at 963.43 s meets a wall. The same keys with the walk from
  915.9 s on steered back to the session's own track points (`toward` each point, the activations aimed along the
  session's own rays; a test aid outside git, not the person's input) give the session's 274 stages in the same
  order, each within 0.024 s, to the surface.
- With the switch off: the third record gives every output byte for byte (`play.txt` apart from its timings).

So `--play-door-open-collision` is in the play preset. The rule's measurement is in the game: stand in that
doorway with the door open, cross it and walk sideways along it.

**The front door's prompt.** The viewport's prompt now follows the door's drawn pose: a held door whose box left,
drawn open (`play_door_drawn_as_held_v1`), reads "close" and its name; any other door, held shut or not held,
"open" as before (`fo4-view` `play::prompt_posed`, a viewer policy; what E does is unchanged). Whether E opens the
front door before the doorbell, while the run blocks its activation, is not known: the session's person did so at
33.69 s, 15.9 s before the bell, and the play mode lets it; `play_door_activation_opens_held_before_visit_v1` names
that reading so a measurement can settle it.

### The vault doorways' drawing, collision and prompt (the ninth-session step)

The ninth sessions' person met vault sliding doorways that looked open, said "open" and, they reported, held them
until E. Replayed headless with `--play-report-walls` (outputs outside git): no stop is recorded at those doorways
before E; the person's W presses there were short (0.4 to 0.7 s) and ended before the doorway, and after E the next
press was on the doorway's middle line. What did disagree: the door's collision stood in its open pose
(`play_door_collision_open_pose_v1`) while the window drew no door at all (`--draw-held-doors` draws only the doors
the held reading has held), so the door's own frame and floor were missing from the drawing, and the prompt read
"open".

- `--play-door-toggle` (`play_door_activation_toggles_v1`, off by default, in the play preset; needs
  `--play-held-doors` and `--play-door-open-collision`): a door standing in its open pose is open for the held doors'
  reading from its walking world's load (`world::Built::open_pose`, `live::Live::with_open_pose`,
  `walker::Walker::held_door_states`), so the window draws it at Open's last key and the prompt reads close. E on a
  door that reading shows open and that stands in its open pose closes it (`live::RunState::close_by_player`,
  `live::DoorState::person_closed`): its held box stands whatever the run's blocked activation, the window draws it as
  stored and the prompt reads open; E again opens it (`walker::door_toggle`). The run is given the activation as
  before. `play.txt` says when the person's activation moved a box.

**The doorways' stops and the floating floor.** The walls report on the two records: at the intake arch the stops
are a desk's collision (a table static placed in front of the arch, its top at 55.7 units from 16 units ahead of the
walker's axis, which is below the window's lower edge at that distance: drawn where it stops, out of view), and the
return and right-side stops are the same desk and a second one. At the post-wake corridor's end and the cone doorway
the wide lines stop against a hall-end static whose only refused field is `XALP`: its collision stands under
`refr_xalp_collision_unchanged_v1` (in the play preset), but render readiness refused it, so its floor and jambs were
never drawn; the floor panel the person saw floating against the clear colour is the doorway's plate with the hall
end's floor and walls around it undrawn. The same piece stands at the prewar sliding doorway beyond the intake room.

- `fo4-view --draw-reference-alpha` (`refr_xalp_render_unchanged_v1`, off by default, in the play preset;
  `Loader::set_render_reference_alpha`): render readiness sets a reference's unread `XALP` aside as the collision
  switch does, and the reference is drawn as its model stores it. In the post-wake vault 6 more placements are
  drawn (the two hall ends among them); a headless frame at the cone doorway (outside git) shows the hall end's floor,
  walls and jambs around the plate where there was the clear colour.

**The front door's E.** After the visit the window drew the front door open and its prompt read close, but E gave
the run the activation and the open again and left the door open (first record, 137.68 s and 331.85 s). With
`--play-door-toggle` E on a held door whose box has left closes it as it closes a door standing open
(`walker::door_toggle`), and for a door the person opens (`--play-activate-opens`) the run is given the activation
without the open. Before the doorbell: the front door's reference stores no lock (`XLOC`), neither it nor its base a script (`VMAD`),
and the base's door flags word is 0; the run's stage
at 3 s blocks its activation, and whether the game refuses the player's key on a door whose activation a script
blocked is not in the data, so the reading stays `play_door_activation_opens_held_before_visit_v1` (pending) and the
early open is left as it is.

**What the records give (this branch's `play_run`, outputs outside git).** With the play options the sessions
recorded (no `--play-door-toggle`): both records 275 stages, every output byte for byte but `play.txt`'s timings. With
`--play-door-toggle` added the person's E now closes what the old prompt offered to open: the first record keeps its
275 stages (the first 223 at their seconds; the front door closed at 137.68 s and 331.85 s, the second reopened by the
evacuation stage's script at 383.94 s; the prewar slider closed at 1506.06 s holds the 1531 s crossing until the second
E at 1562.97 s, so the next stage comes 20.3 s later and the rest about 0.28 s later); the second record stops at 222
stages: its E at 664.61 s on the prewar slider, then shown open, closes it and the recorded keys never press E there
again. Records made under the new prompt are what tests the rule.

**The checkpoint strip and the catwalk's second stop (report only).** The walls report: the strip beside the guard
is held by the collision marker box the data places across the gap (`walk_collision_marker_box_v1`'s half extents,
148 units across, its stage disables it) and the gate frame's static; neither pose of the drawn leaf lies across that
gap (`play_static_leaf_two_pose_v1`), so the strip looks open while the box holds it: the data's box, not changed. The catwalk's second stop (both records) is the exit walkway's ramp part (`play_platform_part_holds_walker_v1`):
its near end stands 48 units above the walker's floor, more than the step, from its stored pose until about 42.2 s into
the sequence the console starts, and lies on the floor from 43 s; the skinned grating over it is drawn from the same
part motion each frame and spans 49 to 57 units at rest, so the drawing does not lag the collision. The edge is 16
units ahead of the walker's axis and 63 below the eye, under the window's lower edge, so the raised grating reads as
flat. Both persons released W before the ramp came down (first record 2070.07 s, ramp down at about 2072 s; second
2301.22 s, about 2302 s).

### The outdoor stalls (the tenth-session step)

Two person sessions reached the surface on the ninth-session build. The first step into each outdoor grid square
stood the walker while the window loaded the square's drawing (the run waits for it): A 6.5, 3.5, 3.4, 2.8, 2.3 and
2.5 s, B 15.2, 5.0, 3.2, 3.9, 2.9 and 4.5 s (the tester's frame ranges). The windows' logs give each as the load, from
the ask to the cell's arrival, plus its upload (0.07 to 0.22 s); B's first and last loads include 5.1 and 1.7 s of
waiting for a cast build the play data thread had started before the cell was asked; A waited for none. Measured
headless on the play data thread with the sessions' arguments and timelines (`play::tests::measure_the_windows_cell_loads`,
ignored, outputs outside git; a cast asked 0.5 s before each cell; the run's own walker build, which shares the
machine in the window, not run) and timed by step (`play: load steps` in the log): finding the cells about the
square takes 0.03 s; the street's square read every model and texture again (5.3 s against 1.8 to 2.7 s for the
squares after it), because the window loads its first cell itself, with data it opens for that load, so the play
data thread's kept assets start empty; and each load looked the named platforms up in the store's placements once
per name (0.6 s).

- A cast build gives way to a cell load (viewer policy): `PlayRun::load` sets a flag the play data thread reads
  between the cast build's steps (the cell, the actors, their poses, the draw list; `timeline::cast_scene` then gives
  nothing), the cell is loaded, and the new cell's cast is asked once it is drawn (none is asked while a cell loads;
  the old cell's cast left with it already).
- The first cell read again (viewer policy): once the window draws, the play data thread loads the window's first
  cell again when nothing else is asked (`DataAsk::Prepare`, 4.7 s headless, what it drew dropped), so the first
  change of cell starts from its assets. A cell asked during that read waits for it; the opening's first change of
  cell comes minutes later.
- The named platforms' cells are found in one pass over the placements and kept with the assets
  (`fo4_scene::platform::PlacedCells`, `add_with`; `add` as before).

Seconds from the cell's ask to its arrival, the cast's waiting first (the tester's stall includes the upload):

| Square | A stall | A window load (cast) | A headless before | A headless after | B stall | B window load (cast) | B headless before | B headless after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| street | 6.46 | 6.33 (0) | 2.25 + 5.27 | 0.98 + 2.09 | 15.24 | 14.79 (5.08) | 2.16 + 5.27 | 0.91 + 2.09 |
| trail | 3.46 | 3.33 (0) | 1.55 + 2.72 | 0.22 + 2.07 | 5.03 | 4.88 (0) | 1.52 + 2.71 | 0.22 + 2.11 |
| bridge | 3.36 | 3.21 (0) | 1.67 + 2.64 | 0.34 + 2.03 | 3.20 | 3.07 (0) | 1.66 + 2.62 | 0.34 + 2.01 |
| hill | 2.75 | 2.65 (0) | 1.60 + 2.23 | 0.10 + 1.61 | 3.93 | 3.68 (0) | 1.58 + 2.22 | 0.10 + 1.61 |
| rock bend | 2.28 | 2.18 (0) | 1.98 + 1.80 | 0.53 + 1.18 | 2.85 | 2.70 (0) | 1.97 + 1.79 | 0.53 + 1.18 |
| deck approach | 2.47 | 2.40 (0) | 1.66 + 2.11 | 0.20 + 1.52 | 4.51 | 4.42 (1.74) | 1.67 + 2.10 | 0.21 + 1.52 |

A cast now gives way within 0.1 to 1.0 s (the step it is in ends first). What is drawn is unchanged: the headless
loads print the same placements, omissions, meshes, textures, surfaces, instances, platforms, doors, terrain, lights
and walking world for every load, and the first cell read again prints what the window's first load prints. With
`play_run` built from this step both records replay as on the ninth-session build: 6 of 7 outputs byte for byte, the
timeline among them, and `play.txt` differing only in its timing lines (`seconds:`, `cost:`, the live world's and
the grid rebuilds' seconds), as a window session's always does from its replay. Not measured in a window: there the
run's walker build at each change of square (2 to 4 s in the logs) shares the machine with the load. The largest step
left is the square's draw list (0.9 to 1.5 s: each model is decoded again for each load).

### The lift's landing and its lower floor (the tenth-session step, nothing changed)

Both sessions, walking from the first lift's deck at the foot of its shaft toward the stairs, came off onto a lower
floor and went back to the lift before taking the stairs (A from 753 to 823 s of the run, B from 722 to 742 s). The
walls report on both records and column probes of the walking world (`column_probe`, outputs outside git): the room
the lift comes down into is one static, and its collision mesh, as read, holds a landing strip beside the shaft at
z -122.4, about 60 units wide, a vertical face 22.6 units high, and past it a lower floor at z -145.1 that runs to the
foot of the stairs, which rise from it (the stairs are the same model's collision). The face is higher than the
walker's 20-unit step (`walk_step_height_v1`, provisional): the walker comes down off the strip, and walking at the
face holds it. A came down onto the lower floor at 753.0 s and got back up at the floor's south edge at 753.2 s, came
down again at 778.9 s, was held 1.06 s against the face (779.47 to 780.53 s, the room's own collision, normal toward
the floor), stood 26 s, got back up at the face's north end at 806.6 s, went to the deck and from there across the
strip, down the face and up the stairs (820 to 822 s). B
was held 2.0 s on the strip by a rigid part of the lift's own collision (722.94 to 724.91 s), stood, went back to the
deck, crossed the strip, came down the face at 741.7 s and went up the stairs.

The collision agrees with the data: the strip, the face and the lower floor are the room model's collision mesh,
placed under `collision_model_blockers_off_chain_v1` (its animation and particle blocks are off its collision chain).
What disagrees with what the person saw is that the room is not drawn at all: render readiness refuses the static
whole (a behaviour graph, transform and particle controllers, a material swap with a filter, eight materials whose
alpha-blend factors the draw does not source, an unread texture slot), so the strip, the face, the lower floor and
the stairs are drawn nowhere and the window shows the clear colour under the walker there; the lift and the
machinery around are other references and are drawn. Nothing is changed: the ledge is the data's; drawing the room
needs its material swap's filter and its blend factors read, a render-readiness step of its own; whether the player in
the game steps up 22.6 units is for the step height rule's measurement.

### Actors and a door the person shut (the tenth-session step, the reading as it is)

With `--play-door-toggle` the person's E closes a door standing open. What the run's walking actors do there: the
person's close is held by the walker alone (`live::RunState::close_by_player`, its box stands, the window draws the
door shut), and the run is given the activation of the door and nothing else; the run moves its actors only by
scheduled moves (the caller's assumed moves and the scene packages' destinations), each putting the actor at its
destination at its second, and none reads a door. So an actor whose way crosses the shut door is drawn past it, as if
it passed through the leaf, and whatever waits on that actor comes as with the door open: the story stays
completable, and no actor opens the door, waits at it or is held by it. A synthetic test shows it at the run's level
(`fo4_vm` tests: an assumed move whose way crosses a door the player activated lands at the same second and place and
raises the same trigger event as without the activation; the play side's test that a close gives the run the
activation alone). The second tenth session closed route doors during the evacuation and after waking and reached the
surface; the evacuees were already past the first door when it was closed, so no session shows an actor meeting a
shut door. Nothing is changed; the reading is named `play_door_person_closed_actors_unchanged_v1` (pending): what the
game's actors do at a door the player closed is for a measurement of the running game.

### The first lift's room drawn by its unmoved parts (the eleventh-session step)

Both eleventh sessions met the room the first lift comes down into undrawn again (A walked off the landing's edge
at 990 s seeing nothing below the worker's boots; B spent four minutes on the way back finding the lift). Read
blocker by blocker at the hypotheses level the window draws at, the room static's 40 blockers split in two. Eighteen
are covered by pending rules every other placement uses: the material swap's `FNAM` (an editor folder label,
`mswp_fnam_folder_label_v1`), the eight materials whose blend factors are enabled
(`bgsm_blend_factors_alpha_function_v1`), a texture cited only in an unread slot (`texture_missing_unread_slot_v1`),
the controller manager, its object palette, text keys and multi-target transform controller at rest
(`nif_controller_rest_manager_idle_v1`), its one looping sequence (`nif_controller_rest_loop_unstarted_v1`), the
`MODT`, the rotation and the absent scale; so the swap, the blend state and the texture slot do not refuse it. The
other 22 have no rule: the behaviour graph's extra data (a generic stage graph), the particle system, its emitter and
update controllers with their five interpolator and data blocks (one interpolator also undecoded), ten particle modifier, emitter and data blocks the
reader does not decode, and two undecoded extra data blocks (`BSPositionData`) on two small shapes. What each needs:
the graph, which sequence it starts at load (a measurement); the particle system, a particle simulation; the two
extra data blocks, a layout. What the data says about their reach: the model's one sequence names two controlled
blocks, both the particle system's emitter (birth rate and active), and its multi-target transform controller names
no extra target (32 empty links), so nothing the graph can start moves a drawn node.

- `--draw-static-parts` (`static_unmoved_parts_drawn_v1`, pending; `fo4_scene::parts`, `Loader::set_render_static_parts`;
  off by default, in the play preset): a static with one model whose model blockers no usable rule covers is drawn
  by the shapes they do not reach. A shape reads the nodes from the root down to it, itself and every block its
  links reach other than nodes and collision; it is left out when an open blocker lies on one of those blocks (the
  blocker's code), when an animation-state block is among them or names one of them as its `Target` (other than a
  holder's own) or `Extra Target`, or a controller sequence names a node of its chain or the shape, or when a
  controller block is not decoded (`ControllerDriven`). Particle systems are left out (`ParticleSystem`), and so is a
  shape whose blend comes from a material file (`UnsourcedMaterialBlend`): its factor numbering has no source. Render
  readiness sets the open model blockers aside only when a shape stays; the draw counts what it leaves out
  (`Build::left_out`) and the window's log gives one `static parts:` line a load. `coverage_census` takes
  `--render-static-parts` (and `--render-reference-alpha`) with `--views`.
- The room: 21 of its 26 shapes drawn (the landing, the drop's face, the lower floor, the stairs, walls and
  fittings); left out the two shapes carrying the undecoded extra data, three material-blended shapes (two decals
  and an effect glass) and the particle system.
- On the opening's route (the ten views the eleventh sessions loaded, with the preset's other render switches)
  the switch draws 16 other placements: the same room's model placed again in the postwar vault (the way back to
  the lift), three houses of the street (a transform controller on an empty node; one blended shape each left
  out), the exterior vault entrance's frame twice (a behaviour graph), a capsule wall twice (two fans a controller
  turns left out) and eight scanners (the scanning part left out). Omitted placements per view: 347 to 344, 347 to
  344, 367 to 363, 375 to 370, 49 to 47, 37 to 35, 535 to 530, 733 (unchanged), 1,846 to 1,841, 229 to 227.
- Headless frames from A's pose at 990 s (on the landing, looking 35 degrees down): black below the worker's boots
  before, the landing's floor, its yellow-edged drop and the lift's base after; toward the stairs from the landing,
  the lower floor, the stairs and the platform above them. The window itself was not run with the switch.

### The cast through a change of place (the eleventh-session step)

Since the tenth-session step no cast is asked while a cell loads and the old cell's cast left with the old cell, so
at every change of place the people vanished until the new cell's cast was asked (once the cell was drawn), built
(the cast build reads the cell again with a loader of its own) and uploaded: the windows' logs give the cast 2.1 to
6.3 s after each new cell's geometry (A: street 3.1, trail 3.0, bridge 4.6, hill 4.6, rock bend 5.5, last approach
6.3, vault 3.5, dream 3.1, wake 3.2, surface 1.3; B 2.1 to 5.9). Viewer policy, the run's outputs unchanged:

- The new cell's cast is asked with the cell: the play data thread builds it right after the cell's load, at the
  second the cell was asked, before anything else asked of it but a newer cell (`CellAsk::cast`). A cast asked
  before it that comes back after it leaves it asked (`CastWork::signature`, one reading of the timeline for both).
- The cast build starts from the play data thread's kept assets (`load::take_kept`, `timeline::cast_scene`'s
  `kept`) and gives them back, so its read of the cell no longer reads every placement's models again.
- The cell drawn stays drawn until the new cell's drawing is spawned (`PlayDrawn::old`, despawned in
  `spawn_window_scene`), and so does its cast when the two cells lie in one worldspace (`Reloaded::keep_cast`: both
  exterior cells of it, so the actors stand where they stood); the new cast replaces it once uploaded, never before
  the cell it stands in (`spawn_cast` waits for the scene).
- A cell of another space (an interior, a change of worldspace) is held back until its cast comes, at most
  `CAST_REVEAL_WAIT` (1.5 s of the wall clock; the run waits meanwhile), so its drawing and its people appear
  together; past that the drawing comes alone and the cast after it.

Measured headless on the play data thread with the eleventh sessions' arguments and timelines
(`play::tests::measure_the_windows_cell_loads`, ignored, outputs outside git; `FO4_VIEW_PLAY_PROTOCOL=after` asks
each cast once its cell is back, as the window did): seconds from the ask to the geometry and from the geometry to
its cast, before (the step's parent) and after (with the next section's kept files):

| Change of place | A geometry before / after | A cast after geometry before / after | B cast after geometry before / after |
| --- | ---: | ---: | ---: |
| street | 2.18 / 0.97 | 2.13 / 1.27 | 2.12 / 1.29 |
| trail | 2.12 / 1.10 | 2.17 / 0.36 | 2.18 / 0.37 |
| bridge | 2.05 / 1.40 | 1.94 / 0.36 | 1.94 / 0.36 |
| hill | 1.63 / 0.56 | 2.52 / 0.74 | 2.53 / 0.74 |
| rock bend | 1.19 / 0.29 | 2.18 / 0.38 | 2.16 / 0.39 |
| back to the hill square | 1.51 / 0.44 | 2.50 / 0.37 | 2.51 / 0.38 (and 2.17 / 0.39, 2.50 / 0.38 for B's second crossing) |
| prewar vault | 1.12 / 1.03 | 1.77 / 0.40 | 1.92 / 0.45 |
| dream | 2.92 / 2.72 | 2.27 / 0.36 | 2.26 / 0.36 |
| postwar vault | 1.46 / 1.15 | 1.73 / 0.24 | 1.72 / 0.24 |
| surface | 3.44 / 2.85 | 0.98 / 0.08 | 0.99 / 0.08 |

Outdoors the cast before now stays drawn through those 0.4 to 1.3 s; at the three interiors the cast comes 0.24 to
0.45 s after the geometry, inside the reveal wait, so cell and people appear together. In the window the run's
walker build shares the machine, so the window's seconds are longer than these; the window itself was not run.

### Pauses, the bare sky and the startup hitch (the eleventh-session step)

- Every read of a model, material or texture went to its archive again (`Assets::with_decoded` extracts the file each
  call: the first cell's read again asks for a file 4,480 times, 3,879 of them one it had already read; each outdoor
  load after it about 5,000 times). The play data
  thread's kept assets now keep the files they read, up to `KEPT_FILES_MAX` (512 MiB of file bytes; the oldest
  leave first, a file above a quarter of the cap is not kept; `Assets::keep_files`, off unless a caller asks; a log
  line per load gives files, bytes and reads answered). With the cap at 256 MiB an outdoor square's draw list hardly
  shrank (1.1 to 1.2 s): its files fill 420 to 510 MiB. The draw list step, headless with the eleventh sessions'
  arguments, before / after (A): street 1.48 / 0.42 s, trail 1.41 / 0.56, bridge 1.35 / 0.85, hill 1.13 / 0.24, rock
  bend 0.86 / 0.12, back to the hill square 1.13 / 0.23, prewar vault 0.29 / 0.24, dream 0.73 / 0.58, postwar vault
  0.50 / 0.29, surface 1.46 / 1.09 (B alike). The process's peak resident memory, headless, 2.02 / 2.38 GB (A) and
  1.74 / 2.27 GB (B). What the draw holds is the same (a synthetic test builds twice from kept files and compares).
- The bridge got slower in the window only. Headless its load is what it was (2.05 s at the step's parent with the
  eleventh sessions' arguments). In the eleventh windows the run's own walking world build for that square, code
  unchanged since the tenth sessions and 3.2 s headless, took 7.99 and 8.59 s (3.62 and 3.69 s in the tenth
  sessions) while the window's draw list took 3.66 and 3.67 s (1.35 headless): the whole process ran 2.2 to 2.7
  times slower during that crossing, the same at A's rock bend (walker 5.36 s, 2.04 headless) and B's hill (5.26 s).
  The logs hold no wall clock or memory reading to say what took the machine then; the tester's readings show free
  memory at 33 to 36 % during the sessions against 55 % at the start. The kept files take most of the draw list's
  work out of every load, which is what the window can change.
- The bare sky (B at 528.6 s): the frame log shows the window held only the sky and the HUD for 4 frames (0.07 s)
  as the square crossed back was uploaded, the old cell's instances and cast removed when the load arrived and the
  new ones spawned after the texture upload's batches; every load of both sessions did so for 1 to 5 frames (0.02
  to 0.21 s). The record shows the person standing from 528.6 to 542.1 s, so the 11 s are the time to the tester's
  next look, not 11 s of sky in the window. Since the step above the old view stays until the new is spawned.
- The startup hitch (A frame 433 at 8.4 s, B frame 724 at 13.6 s of wall time, 834 ms): the frame whose update
  encoded and wrote the first `--play-shots` frame (2560 by 1440) as PNG on the frame loop; every shot after it did
  the same (0.7 to 1.5 s, one every 30 s of the run). The first cell's read again runs on the play data thread and no
  frame waits on it. Shots are now encoded and written on a thread of their own (joined before the window closes).
  The read again took 7.74 and 12.43 s in the windows (4.7 headless) and saved the street's square most of its
  stall (6.46 and 15.24 s in the tenth sessions, 4.16 and 5.42 s in the eleventh): kept.

### The deck's crowd, the helmets and the actor at the crane (the twelfth-session step)

Both persons of the twelfth session saw the people on the first lift's deck stand inside each other with their
feet below the deck, and the checkpoint's uniforms with no head under the helmet; in A one actor near the crane
was gone between 619.2 and 625.9 s.

**One marker for five, at the covers' height.** The run's timeline (both sessions) moves five actors of the
crowd at 603.8 and 605.7 s (A; 589.6 and 591.6 s in B) by `pack_location_destination_v1` to one point: their
scene action's package names one placed location reference, the same for all, so the run puts all five there.
Where the game spreads several actors sent to one marker is not in the data; they stay at one point (open).
That point's height, 8,925.06, is the top of the shaft's two covers as stored (closed); the deck's collision
stands 45.9 units higher once raised (the walker's feet 8,970.9 on it; the deck part's box as stored lies at
the shaft's bottom and its sequences raise it to a top of 8,971), and the run moves nobody during the descent,
so the drawn crowd stood in the deck and then stayed at the top as the deck went down. With
`--draw-cast-on-platforms` (`play_cast_stands_on_platform_v1`, pending, in the preset) the window draws each
actor of the cast at its place's x and y with its feet on the highest top of a named platform's part box under
it (the part's collision box as stored, now carried by `fo4_play::elevator::PartMotion::stored`, moved by its
motion), no higher than a character's height (128) above where its feet were drawn the frame before, else at
its place: raised, the five stand on the deck (45.9 up); going down, each follows the deck from where it stood;
a cover sliding from under an actor lets it back to its place. The run, its timeline and its outputs are not
touched. Boxes stand for surfaces: a part whose collision is not flat on top stands an actor on its highest
point.

**Helmets.** The head of a placed actor was left out whenever a worn outfit addon had vertices mostly on the
head parts' main bone (the covering clause of `npc_head_parts_unmorphed_v1`): the checkpoint's helmet does, so
its wearers lost face, eyes and hair. The helmet's armour stores BOD2 `0x00010001` (bits 0 and 16): not bit 2,
the slot xEdit's published names call 32, FaceGen Head. With `--actor-head-slots`
(`npc_head_hidden_by_head_slot_v1`, pending, in the preset; render and fo4-view) the worn outfit's BOD2 words
decide instead: the whole head hidden by bit 2, the hair parts (HDPT PNAM 3) by bit 0 or 1, else nothing. At
the gate (B, 569.1 s) the four helmeted actors keep 1 to 11 head parts each, one hair part hidden each, where
all four had none.

**A capped child without a face** (twentieth session). A child of the checkpoint crowd lists only two head parts
of its own: a mouth (HDPT PNAM 8) and a hair (3), hidden under its cap by the slots above. Its race's HEAD list
for its section holds a face (1), a head rear (9), eyes (2), a mouth and a hair. Drawn with its own list alone it
showed a cap and a mouth with scenery through the rest of the head. With `--actor-head-race-fill`
(`npc_head_parts_race_fill_kinds_v1`, pending, in the preset; render and fo4-view) an actor that lists its own
parts also takes its race's part of each type its list lacks, except type 7 (xEdit's published name: Meatcaps,
the cut surface of a severed head: taken, it drew a red ring at every adult's neck); its own parts stand in for the
race's of their types. The cast log's `faces:` line counts, at each cast build, the actors with a head, those that draw no face
part and those of them wearing headgear (named).

**The actor at the crane.** In A the run moved that actor: the caller's assumed move of the run's arguments
(`--assume-actor-at`, held to stage 475 by `--action-after-actor-moves`) comes at 621.06 s with the stage and
puts it about 3,100 units away by the deck; its presence does not change and the cast drew it at the new place.
The draw dropped nothing. In B the same move comes at 615.82 s; the crane group itself had gone to the deck
marker at 589.6 to 591.6 s.

### The dirt turn and the dream load (the twelfth-session step)

The twelfth session's longest outdoor stall was the dirt turn (2.27 and 2.33 wall s: load 2.12 s, its draw list
1.35 s, upload 0.13 s) and the dream took about 3 s (load 3.04 s, its cell step 2.04 s) before its cast's
0.42 s hold. Replayed headless on the play data thread as the window asks them (`measure_the_windows_cell_loads`
with session A's argument files and timeline), the dirt turn loads in 1.34 s (draw list 0.71 s: in the window
the frame loop and the cast build share the machine) and the dream in 2.77 s (cell step 1.97 s). Sampled
(`sample`, macOS), the dream's load spends most of its busy samples inflating archive entries it reads for the
first time (zlib) and about a quarter in the rule registry's lookup: `fo4_scene::hypotheses::status` scanned the
half-megabyte registry text for every question, thousands per load. The statuses are now read once
(`hypotheses::statuses`, the first row of a name with a status, as the scan found it; a test compares every row);
what is loaded is unchanged. After: dream 2.29 s (cell step 1.56 s), dirt turn 1.26 s, the vault's arrival 0.94
s against 1.08, the wake 1.03 against 1.20, the street 1.18 against 1.35 (one run each). What remains is reading
files no earlier load kept: a load that fetched a cell's files ahead of time, or several at once, would cut it
(not done).

### Watching the actors at a shut door (the twelfth-session step, measurement only)

The tester could not tell twice whether an actor walks through a door the person shut: the views are periodic
and a person cannot follow one actor and a door at once. With `FO4_VIEW_PLAY_WATCH=FILE` in the environment
(measurement only, off by default; nothing of the run or the drawing changes) the scripted play window writes a
tab-separated log, one line per change, each with the play clock's second:

| Line | Fields | Written |
| --- | --- | --- |
| `walker` | X Y Z of the feet | with every other line, and when the feet moved 8 units |
| `actor` | reference, `drawn` or `gone`, X Y Z where its draws stand (its place, raised by `--draw-cast-on-platforms`), distance to the walker | when it enters the drawn cast, moves 8 units, or leaves the cast |
| `door` | reference, `closed` (its box stands), `open` or `not-held`, X Y Z of its first drawn shape (`-` when not drawn), distance to the walker | when its state changes, for doors held by the reading within 2,048 units of the walker (any distance when not drawn) |
| `sample` | X Y Z of the feet | every second of the play clock, and with each `door` line, moved or not (the thirteenth-session step) |
| `near` | reference, X Y Z where its draws stand, distance to the walker | after each `sample`, for the eight actors of the drawn cast nearest the walker, moved or not |

A shut door and an actor on either side of it at two seconds, with no `open` between, answer the question from
the log: the actor's drawn place is the run's (`spec/play.md`, Actors and a door the person shut), so a crossing
there is the run's scheduled move, not a walk. A short check on the opening's first minute wrote the four actors
of the house at load and the front door closed at 3.1 s, 245 units from the walker, at 60 fps median.

### The crowd's ride, the security staff, the hair and the late start (the thirteenth-session step)

**The crowd left on the covers.** The thirteenth session's watch log shows the six riders going down with the deck
32.9 units (745.4 to 746.5 s) and then holding 8,926 while the walker went on down. Under
`play_cast_stands_on_platform_v1` each actor takes the highest part top under it within a character's height
above its feet: the shaft's two covers, closed as stored at a top of 8,925, came within that height as soon as the
deck passed them, so the crowd stepped onto them. With `--draw-cast-keeps-its-part`
(`play_cast_keeps_its_part_v1`, pending, in the preset) an actor rides the part it stood on the frame before,
wherever it goes, while that part's box still spans its place; only when it does not is a part looked for again.

**Five at one point.** The five actors' scene actions name one package; its one Location input names one placed
marker with a radius word of 150. No alias, linked reference, furniture or idle marker gives any of them a place of
its own, so the run keeps them at the marker; the sixth stands 100 units off by the caller's assumed move. Where
the game stands five actors sent to one marker within its radius is a measurement to make
(`pack_location_radius_one_point_open_v1`, recorded open; nothing changed).

**The security staff drawn as markers.** Their helmet's visor is an effect shader whose material file's
environment-mask slot cites a texture no archive holds, and whose own environment-mask field cites the same file
without its `textures` root (refused, and the root completion finds no file). Both are slots the frame never
samples; a placed reference's closure already reads such a missing texture under
`texture_missing_unread_slot_v1`, the actor path did not. With `--actor-unread-texture-slots`
(`actor_texture_missing_unread_slot_v1`, pending, in the preset; render and fo4-view) it does, and counts the
rootless citation whose rooted file is missing as that missing texture (`fo4_scene::missing::unread_rejections`).
The four security actors of the session's cast resolve with only their enable chain's blocker, which the cast
decides by the run.

**Hair and skirts to the floor.** The stored vertex positions of head parts and outfits are not in the actor's
space (a long hair's from -9 to 12, an outfit's from -121 to -4); the skin's bones the skeleton holds map them up
by about 120 at rest. `actor_missing_bone_bind_pose_v1` gave a bone the skeleton lacks (hair and cloth bones) the
identity, so every vertex on one stayed near the feet and the blend drew strips down to them. With
`--actor-missing-bone-nearest` (`actor_missing_bone_follows_nearest_v1`, pending, in the preset) such a bone takes
the composite matrix of the skin's held bone whose bind origin lies nearest it: kept at its bind place relative to
that bone. Measured at rest on the deck crowd: the long hair from 111.7 to 132.9 (before -9.1 to 132.9), a dress's
cloth vertices from 24 to 79 (before -97 to 75).

**Showing second N of a record.** `--play-from SECONDS` (with `--play-input`, test and measurement only) opens the
run's gate to that second at once, so the run goes as fast as it computes; the window loads the walker's cell when
it looks and writes no frame before it; there (the clock at the second, or the run waiting at it) it loads the
walker's cell and cast, holds the clock 4 wall seconds and goes on at its own pace. The run and its outputs are
those of the same input (the gate only orders the run against the window).

**Frames as JPEG, the watch's samples.** `--play-shots` frames are JPEG at quality 80 (the project's baseline
encoder, `apps/fo4-view/src/jpeg.rs`, ITU-T T.81 with its example tables; `tSECONDS.jpg`); the CPU frame and
`--headless` stay PNG. The watch log adds a `sample` line every second of the play clock and at each door line,
with the eight nearest actors of the drawn cast as `near` lines, moved or not.

**Session A's stalls.** A's six outdoor holds took about twice B's while about 11 GB were deleted on the same disk.
The load lines split a load into steps (cell, neighbours, references, draw list, terrain, walking world) and give
the upload apart, but no step separates reading archive bytes from decoding and building: A's draw list ran 1.4 to
2.8 times B's, the walking world 3.5 times (0.28 against 0.08 s on the street) and the upload to the GPU, which
reads no file, 1.7 times (0.24 against 0.14 s). Every step was slower then, not one; the logs cannot say how much
of it was the disk. Nothing in the code was changed for it.

### The exit's device, the hidden targets and the lab coat (the fourteenth-session step)

**Neither session left the vault.** The seven commits after the thirteenth build changed no answer of the run:
the window's three new switches are the drawing's alone (the play options `play_run` reads are the same), and the
thirteenth build's `play_run` replays session B's record to the same timeline byte for byte. What differed was
where the persons aimed. The exit's device lies on a placed activator (a skeleton) whose script, on the player's
activation, activates its linked furniture for the player with events; the furniture's own script and a quest
alias holding it act in `OnActivate`, and the alias sets the stage the console's script needs. Both thirteenth
persons' rays met the furniture's box; session A never aimed there and used the console first, so the console's
script set the stage of its refusal (the data's answer, the gear door shut as it should be); session B's ray met
the activator's box. The run read the activator's scripted activation as a sit only
(`vm_furniture_activate_events_sits_v1`): no `OnActivate` reached the furniture, so its stage was never set, and
the player stayed seated in the run, so the console's script, which acts only while `GetSitState` answers 0,
ignored every later use. Two readings, each pending, off by default and in the run's arguments:
`quest_run --activate-furniture-events-deliver` (`vm_furniture_activate_events_delivered_v1`) delivers the
furniture's `OnActivate` before the sit, and `--sit-end-walk-events 64`
(`vm_sit_events_activation_ended_by_track_v1`) ends only such a sit once the track is 64 units from the
furniture's stored place, also looked at before each caller or input action. With both, session B's record
takes the device at its first press on the activator (1744.2 s), the console's first use opens the gear (1815.2
s, stages 7, 9, 8), and the record's walk later passes where it stood against the gear; every stage before
1744 s keeps its second. The other sits an activation with events brings in the opening (the crib, the waking
pod) now end on the walk too, with no stage changed.

**The hidden targets.** The nursery's target (a skinned model with a behaviour graph), the exit's activator
(skinned, with stored ragdoll data no reading takes) and the device's furniture (an animated model) are refused
at render readiness, in the thirteenth build as now: they were never drawn. The window keeps the references its
loads leave out and the E prompt ends with "(not drawn)" when it names one (viewer policy); the watch log writes
the prompt, the aim and the confirmation at each change, for a recorded input as for a person's keys. The
confirmation after E said "nothing happened" where the run acted (the nursery, the activator): the walker logs an
activation while it builds the slice the run then takes, and the "before" counts were taken after that; they are
now the previous look's (`confirm_line`). Around the nursery target, a lamp, toys, linens, a book and a changing
table were refused only because their records carry scripts: `--draw-scripted-as-stored`
(`refr_script_render_as_stored_v1`, pending, in the preset) draws such a reference as its model stores it, the
run deciding its presence. `FO4_VIEW_OMITTED=FILE` (measurement only) lists each load's and each cast build's
omissions with their causes. Those activators were also refused for their base's unread marker colour, flags and
activate text: `--draw-activator-fields` (`acti_fields_render_unchanged_v1`, pending, in the preset) sets those
aside for the drawing. With both, the window replay of session B's record draws the nursery's lamp, linens and
toys at 176 s; the crib's place stays bare and the prompt there names the target and ends with "(not drawn)".

**The lab coat.** The coat beside the intake guard is a scientist with a plain base (no template bits, ten head
parts listed, an outfit of one armour whose slot word holds no head or hair bit). The window replay of session A
draws it without a head at 1693 s with the preset and with `--actor-missing-bone-nearest` left out alike, and the
cast build reports no head left out for it. Its cause is not found; nothing changed.

**Markers.** The four of each session are three readings no rule covers: the kitchen's robot (its race stores no
skin; its look is assembled from an object template), the escort down the freezing room's approach (it wears the
device, whose model carries a transform controller), and two leveled actors behind the postwar console (two and
three candidates; `--actor-leveled-seed` would pick one, not in the preset). Nothing changed.

**The descent's last second.** Session B's record replayed with the walls report: no input in the last 16 s of
the ride and no blocked tick, yet from 942.67 s the walker's feet slide 3.1 units a tick along x while the deck
drops 4.6 (y held to the hundredth), then from 943.25 s along y, about 100 and 130 units before the interior's
load door takes it at 943.99 s. That is the capsule pushed out of a surface it descends into near the shaft's
bottom (inferred: constant ratio to the descent, no wall reported); which surface, and whether the game's player
is pushed so, is not established. Nothing changed.

**The checkpoint hill's stall.** Each load step's time is now split into reading stored archive bytes,
inflating them and the rest (`fo4_archive::clock`, per thread, measurement only), on a second line after the
load steps. Why session A's load worker took 1.97 s against B's 0.77 s is not established; no replay of a
person session reaches that load with the split lines yet.

### The outdoor holds, the console's refusal, the gear's hub and the nursery's props (the fifteenth-session step)

Both fifteenth sessions (built at the fourteenth-session step) reached the surface. Their records replay on this
step's `play_run` with the sessions' own run arguments to the same outputs (6 of 7 byte for byte each, the timeline
among them; `play.txt` differs only in its timing lines, as a window session's always does from its replay).

**The outdoor holds.** The six first crossings held the walker 11.9 s (A) and 8.5 s (B) in the sessions against
9.0 and 7.0 s in the fourteenth. The kept files (the early step's `Assets::keep_files`, 512 MiB) explain it: the
play data thread's casts between the loads read through the same kept assets, and with the two drawing switches of
the fourteenth-session step the first outdoor square keeps 751 files (434.7 MiB) where the fourteenth kept 601
(423.8 MiB); the street's square (126 more placements drawn, 78 more meshes, 48 more textures) then read 960 files
from the archives again against the fourteenth's 258, the draw list inflating for 2.04 s. Three causes, measured
headless with the load measurement replaying each session's casts between its loads
(`play::tests::measure_the_windows_cell_loads` with `cast SECOND CELL` lines; it gives the session's kept-file
counts exactly):

- a new asset's visit read and inflated each file for its parse and the draw list read and inflated it again;
  the visit now keeps what it read (`Assets::visit`, with kept files on);
- a hit did not refresh a kept file, so the oldest went first even when in use; the least recently used now goes;
- the cap: at 512 MiB the first square and the casts fill it. At 1 GiB (`load::KEPT_FILES_MAX`) the headless
  replay reads about 140 files a load from the archives; 1.5 and 2 GiB read none and are no faster. The process's
  peak resident memory went from 2.40 to 2.52 GB for A (1.5 GiB: 2.93) and from 2.63 to 3.07 GB for B (against
  the 512 MiB cap with the first two changes). Session B's eight loads headless: 1.73, 1.66, 1.51, 1.44, 1.63, 0.91,
  0.31 and 0.41 s at 512 MiB with the first two changes, 0.86, 0.80, 0.58, 0.56, 0.74, 0.51, 0.30 and 0.40 s at 1 GiB.

Session A's record in the window (no person; run from 530 s at the window's pace, the session's preset and run
arguments before, this step's after), the hold as the fifteenth report measured it (from the first frame at the
held place to the first frame with the new square; seconds), and the load and upload from the window's log:

| Crossing | person session | replay before | replay after | load + upload before | load + upload after |
| --- | ---: | ---: | ---: | ---: | ---: |
| street | 4.083 | 2.319 | 1.097 | 2.17 + 0.13 | 0.91 + 0.16 |
| trail | 1.856 | 1.609 | 1.286 | 1.45 + 0.14 | 1.10 + 0.15 |
| bridge | 3.124 | 1.846 | 1.291 | 1.53 + 0.16 | 1.00 + 0.14 |
| hill | 1.742 | 0.885 | 0.668 | 0.76 + 0.09 | 0.55 + 0.08 |
| rock bend | 0.491 | 0.571 | 0.561 | 0.46 + 0.09 | 0.44 + 0.09 |
| last approach | 0.611 | 0.604 | 0.582 | 0.50 + 0.08 | 0.47 + 0.08 |
| all six | 11.907 | 7.834 | 5.485 | | |

The longest frame interval inside each hold stays 26 to 52 ms before and after. The draw list's inflate at the
street went from 1.05 s to none (its archive reads under 0.01 s). Why the person's session held longer than its
record replayed in the same window build (11.9 against 7.8 s) is not established (the machine's other load at the
time, inferred).

What is left of a crossing is the draw list's parse and build of every model (the rest), the references' visit of
the new assets and the walking world; a prefetch of the next square on the play data thread was not done (the
kept files now hold what it would read; parsing ahead would need the whole square's scene built ahead).

**The console's refusal.** In session A the console's E at 119.5 units showed the prompt and "E used" but nothing
opened; at 49.7 units it did. Not the reach: the run's effect log shows the console's script asking `GetSitState`
and getting 3. The person had taken the device 68 s before through its activator, whose script activates its
linked furniture with events, a sit the play mode does not follow (the walker never sits); the sit ends under
`vm_sit_events_activation_ended_by_track_v1` once the track is 64 units from the furniture's stored place, and the
walker stood 63.8 units from it (horizontally); before the later press it had gone 72 units away. Two changes:

- `quest_run --sit-end-other-activation` (`vm_sit_events_activation_ended_by_other_activation_v1`, pending, off by
  default, in the run's arguments): such a sit ends, as a get-up delivered first, before the caller's or the
  input's activation of any other reference. Session A's record with it: the console's press at 1830.18 s sets the
  follow-on quest's 7, then 9 and 8 at 1845.18 and 1845.85 s and the vault dialogue's 95 (before: 1924.18, 1939.18,
  1939.85 s); every other stage keeps its second, 276 stages to the surface. Session B's record: the timeline byte
  for byte as before (its presses came after the walk had ended the sit).
- The confirmation after E (viewer policy): "used" only when a stage or a scene changed; "used, no stage or scene
  changed" when only the scripts' calls grew (the press reached the target's scripts, which refused it); "opened"
  or "closed" for a held door the press turned (the fifteenth's pocket door said "nothing happened"); and the name
  of the first other reference the scripts activated, in parentheses, from the run's effect log.

**The pickup cue.** The device lies on an activator named in the data for what the ray meets (a skeleton); its
script hands the press on to the one keywordless linked reference (XLKR), a furniture whose base has its own name.
The prompt on a reference with scripts whose one keywordless link is a named placed furniture now adds that name
(`use NAME: LINKED`, viewer policy; the link read as `vm_host_linked_ref_xlkr_v1` reads it). The activator's
activate text override (`ATTX`) is not read: its layout is not established in this project's sources.

**The gear's hub in the opened doorway.** The gear door's model sets, under the node whose collision is the gear's
moving part, nine nodes of its own through the same sequences (twelve drawn shapes: the hub's ring pieces and an
upright post); none holds a collision body, so the rigid reading gave them no part and they stayed where built, in
the doorway (the platform report now lists each rigid shape that stays with its chain's set nodes).
`--draw-platform-nearest-part` (`play_platform_shape_nearest_part_v1`, pending, off by default, in the preset):
such a shape goes with the part of the nearest set node above it that holds one; its own node's motion is not
drawn. Session A's record in the window at 2039 s (the frame the person saw the ring in): the doorway clear, the hub's pieces on the gear rolled aside; of the gear door's rigid shapes 5 stay as built (none of them under a node a sequence sets) against 21, and the exit cell's named platforms draw 38 shapes on their parts and 18 still against 20 and 36. Nothing of the collision changed.

**The nursery's props.** The small piece at the sill (a baby's rattle) and the folded linen beside it are scripted
activators the fourteenth-session step's two switches draw; their stored places stand 3 units into the top of the
changing table they lie on (the activators' bounds from 7922, the table's to 7925), and the changing table is not
drawn: a container whose record's visual fields (`MODT`, `DATA`, `SNAM`, `QNAM`) are not interpreted and whose model
is skinned and animated (a controller manager). So they show resting on nothing in front of the window sill. The
red ball in the closet is a static whose bounds start at the floor's height (7853 against the floor's collision at
7852.7 to 7853.5): it stands on the floor; it reads as floating for want of a contact shadow (none is drawn).
Nothing changed: drawing the containers would need their fields set aside (a rule like
`acti_fields_render_unchanged_v1` for containers, not registered) and, for the changing table, a skinned and
animated model drawn as stored.

**Markers and undrawn actors.** The actor with no skin and no outfit is the kitchen's robot (its base stores an
object template, `OBTE`/`OBTS`): left out unseen (the analysis's `npc_no_skin_no_outfit_unseen_v1`) would hide what
the game shows, so no such rule was added; it waits for the object template's reading. The transform-controller
blocker of a cast marker now prints its rest tag (presentation), which names the rule family that could cover it;
`actor_part_transform_rigid_only_v1` and the skinned falloff's posed normals
(`actor_skinned_normals_linear_blend_v1`) were not done. The lab coat's missing head (no cause found) and the
undrawn crib occupant, remains and device (skinned or graph-animated models) are unchanged.

### The orientation stall, the lost announcement, E's words and the data thread's clock (the sixteenth-session step)

Both sixteenth sessions (built at the fifteenth-session step) reached the surface; their records replay on this
step's `play_run` with the sessions' own run arguments to the same outputs (6 of 7 byte for byte each, the timeline
among them; `play.txt` differs only in its timing lines).

**The second session's stall in the vault.** After the vault's intake stage the run set nothing for 364 s, until the
person used the pod themselves; the party was missing from the lift. Read from the run's own lines: the greeting
scene's phase waits on a stage that three crowd actors' animation-event handlers set; their handlers registered for
the event only when the actors load into the vault, and they never loaded there: they were disabled, children of the
street crowd's enable marker, which only the hill quest's stage enables; that stage's trigger needs the hill quest's
first stage, set by the main quest's evacuation stage, set by a trigger on the front steps that asks for the stage
the broadcast's end sets. The person stepped into that trigger 0.019 s before that stage was set and stood in it for
15 s; the trigger's script acts on entry only, so nothing of the evacuation chain was set for the rest of the run.
The first session crossed the same trigger again later and went through. The stage came 3 s late because the
broadcast scene's last action is a dialogue action whose topic holds lines none of whose conditions hold, and the
run gave it the caller's assumed line seconds. A delay of the trigger's event (tried, not kept) moves the scene too,
since the scene was started by an earlier trigger: the race stays.
`scene_dialogue_no_line_holds_ends_v1` (pending, `quest_run --no-line-holds-ends`, in the bundle's run arguments,
off by default): such an action ends as it starts. With it the first record reaches the same 273 stages (six 3 s
earlier) and the surface at the same second; the second record's person pressed E on the front door after the
script had now opened it, which closes it under the door toggle, so a copy of that record without that one press
(kept beside the bundle, the original untouched) replays through the evacuation chain, loads the crowd below, and
reaches the pod's stages 14 s after the intake stage instead of 364 s (the replay then leaves the person's later
input behind).

**The first session's lost announcement.** The broadcast starts when the player enters a trigger at the nursery's
door after the stage the spouse's scene sets on its end; the person had left the nursery before that scene ended
and crossed the trigger then (its handler asked and found the stage not done), and came back to it 325 s later. That
is what the data does. The window now says where to go (viewer policy): the run keeps, for each trigger the player
entered, the stages its handlers asked about and found not done; after 30 run seconds with no change of the quest
log, the objective corner names the nearest such place whose stage the run has set since, as `Go back: N units`
and its side from the walker's heading (`fo4_vm::track::Place::asking_again`, `play::cue_line`); the watch writes
a `cue` line at each change. The first record in the window with the session's own run arguments: from 590 s (30 s
after the quest log last grew) the corner reads `Go back: 473 units behind you` and onward as the walker leaves the
house; the frames written with `--play-shots` do not carry the HUD.

**E's words.** A press on a held door waits up to half a run second for the held doors' reading to take the toggle
(it does at the next walking-world update), so a pocket door says opened or closed. A press that set a stage or
scene waits as long for the line it starts and carries its text in the run's words (`used: "..."`): the console's
refusal reads as its subtitle does.

**After the eighteenth session (viewer policy unless a rule is named).** A press the walker leaves as it stands
(`play_door_toggle_blocked_unchanged_v1`) confirms `held: left as it stands` (`play::confirm_used`), not `opened`,
even when the caller's opens name the door: the eighteenth session's front door kept its earlier `opened`. The `Go
back` cue follows the walker's own track in the walker's space back to the latest point inside the place
(`play::cue_trail`): it gives the trail's length and aims at the trail's point 96 units back (`CUE_AIM_TRAIL`), so
it names a doorway the walker came through instead of the place's nearest point behind a wall (the house cue said
`150 units ahead` at a bookcase beside the open hall); with no such point in the track it keeps the straight line.
After the nineteenth session the way back cuts loops: from each point it goes on to the earliest-walked point of the
track within 32 units on the ground and 24 in height (`CUE_CUT`, `CUE_CUT_RISE`: nearer than a wall's two sides can be
for the walk's capsule), so a detour the walker came back from is left out and the length falls as the walker follows
the cue. The nineteenth session's house cue had grown from 245 to 908 units while the person walked to the hall after
a detour; replayed, the same record's cue says 240 to 371 units and falls to 153 and 72 on the way into the hall.
After the twentieth session the cue also looks ahead (`play::cue_sighted`): when the straight line at eye height
from the walker to the place's nearest point meets no collision of the walking world (`Walker::clear_line`), the cue
points at that point and its length is the straight line; else it points at the latest point of the way back in
sight (at most 24 tried, `CUE_SIGHT_TRIES`), its length the straight line to it and the way on from it; else the
way back as before. So the way back does not grow while the walker walks an open line toward the place (the
twentieth session's house cue said 285 ahead, then 323 and 412 behind, walking into the hall).
The same stall (no change of the quest log for 30 run seconds) also names where to use (`play::use_target`,
`play::cue_use_line`, viewer policy): of the references the person's E stands for (`--play-activate-person`), the
nearest one in the walker's space within 2,048 units on the ground that no E has met yet, its display name, `(not
drawn)` when the window's loads left it out, its distance and side, and how far below or above the eye its object
bounds' centre lies (`Use NAME (not drawn): 40 units ahead, 110 below the eye`). It comes before the `Go back` cue.
The twentieth session's person searched nine minutes for the device beside the vault console, which is not drawn
and lies on the floor about 110 units below the eye.
After the twenty-first session the open line is the one the walker's body walks, not a sight line: a copy of the
walker's character walks from the feet straight toward the point at the run speed in ticks of 0.1 s
(`fo4_walk::Character::walks_to`, `Walker::walks_to`, reads only) and the line counts as open when it gets as far
along it as the point (less half a radius) without a blocked tick, without sliding more than a radius off the line
and without dropping more than its height (the session's cue said 59 ahead into a bookcase the sight line passed
over). The way back passes over a stretch of the trail in another space when the walker left and came back within
128 units (`CUE_DOOR_JOIN`: one door; the session stepped out of the house and back, and the cue, with no way back,
pointed through a wall). A use target is retired once E met it, a script activated it (taking the device's holder
activates the device: the session's cue named the taken device for 300 seconds) or the run made it absent
(`play::used_targets`, `Walker::made_absent`). The watch's cue line carries after a second tab how the cue was found
(`walked to the place`, `walked to the way's point`, `along the way`, `straight, no way back`).
`--draw-cast-keeps-clear` (`play_cast_kept_clear_of_walker_v1`, pending, in the preset) keeps an actor's drawn
body clear of the eye: the cast build keeps, for each actor, the horizontal box of its draws' vertices by 8-unit
slices of height above its place (`scene::cast_body`); each frame the slices within 24 units of the eye's height
give a circle (the box's centre, its half diagonal plus a capsule radius) the eye must stay out of, and the actor is
drawn moved so that it does, eased from the frame before: carried round the eye at most 180 degrees a second and
nearer or farther at most 120 units a second, never inside the circle (`scene::ease_cast_clear`), back to its place
once nothing of it is at the eye's height. An actor that draws nothing keeps the earlier reading (two capsule radii
from the feet). The run is unchanged and the walker still passes through it (actors' capsules stand in no walking
world). The eighteenth session's eye entered a standing evacuee's head at 5.6 units; the nineteenth's, after a
180-degree turn, with the place 32 units away (the body at the eye's height reaches about 36 units from the place,
toward the walker) and the actor jumping 57 units to the walker's other side in 17 ms.
`--draw-cast-settles` (`play_cast_settles_on_ground_v1`, pending, in the preset) stands an actor drawn standing at
a place the run took from its stored placement or a package's location on the walking world's highest surface under
that place, from 32 units above it, when that surface lies within 32 units above or 64 below and is no steeper than 60
degrees (`scene::settle_z`; both reaches and the slope are assumptions). The window asks the play thread the ground
under each such actor's place (`Play::ask_ground`, `Seen::ground_asked`), the play thread answers at each publish
from the walker's world as it stands (`Walker::ground_below`, hidden sources passed through), and the frame's height
step (`ride_cast`) draws the actor there unless a named platform's part holds it. Seated or lying actors, MoveTo
places and the caller's places are not moved. The window log's `settled:` line counts at each change the actors drawn
on the ground, the most dropped and raised, and those kept at their place by reason. The twentieth session's woman
at the checkpoint stood 19 units above the terrain on a slope; with the switch she is drawn on it.
`--draw-furniture-parts` (`furn_unmoved_parts_drawn_v1`, pending, off by default and not in the preset, with
`--draw-static-parts`) reads a furniture refused for its model's animation as `--draw-static-parts` reads a static:
drawn by the shapes no controller reaches (`Loader::set_render_furniture_parts`). On the vault exit's device it draws
nothing: the placement stays refused with the switch (the twenty-first round's window replay), so the switch was
left out of the preset until round 27 (below).

**The data thread's clock.** Each cell load on the play data thread prints `play: load clocks: wall W s, the data
thread's CPU C s (P% of the wall)`: a load whose wall time is far above its CPU time waited for a core; both up
together ran slower. In the window replays of both records (this step's viewer) the loads ran at 80 to 100% of
the wall on the thread's CPU, and the holds were those of the person's build replayed (the second record's five
crossings 1.05 to 0.53 s before and 1.12 to 0.43 s after; the first record's 1.2 to 0.4 s both): a replay has no
contention to show, so the live session's 2 to 2.6 s holds wait on these lines. The thread's scheduling class is not raised: the platform call needs unsafe code, which this
repository forbids, and the system thread object's safe setter does not reach a thread it did not start. Keeping
built meshes across loads was not done.

**Skinned placements in the play cast.** `--skinned-models-own-nodes` (existing pending rule) is now read by the play
window's cast build and is in the preset: after the run's hides and shows at the cast's second, every placement of
the cell refused for its skinned shapes is one more actor drawn by them on its model's own nodes; a placement's
editor-marker refusals go with it, so those subtrees are set aside under the rules that cover them instead of
refusing the model. `FO4_VIEW_HEAD_NOTES=FILE` (measurement only) writes each cast actor's head as read, or that it
has none, with its notes. In the first record's window at the nursery the cast adds ten such placements in the
house, among them the crib's occupant and the changing table; the occupant stands at its stored place 49 units over
the floor, and no placement around it reads as a crib (its model holds only the occupant's skeleton and skin), so it
shows in the air. By the device on the grating a dark shape is added where the remains are. At the vault's intake,
three cast actors have no head, each because a listed head part has conditions (`head()` drops the whole head when one
part's conditions are unread); the lab coat is one of them. Drawing the other parts, or reading the conditions, is
behaviour not done.

**The suspended timber on the road home.** From the placements near the person's eye (`near_probe`), it is a fallen
branch static placed with a roll of 0.765 and a pitch of -0.502 rad: under the measured rotation reading its long
axis rises about 29 degrees, its lower end about 60 units under the ground in a hedge row beside a maple, its upper
end about 215 units above the paved bend, with nothing placed under it. It is drawn as stored; whether the game shows
it so is a photograph at that eye. The distant geometry at outdoor boundaries was not looked into.

**The dream room's light.** The light report adds what an interior's lighting stores and the frame does not read
(the directional ambient cube and its scale, the directional fade, the far fog colour, power and clip, the image
space). Nothing lights differently.

### The lift's crowd, the open-looking doorways, the crib, the furniture and the remains (the seventeenth-session step)

Two more person sessions reached the surface; session B went on to the house's yard. What they met, from data:

**The crowd left at the top of the first lift (session B).** At the cut into the vault's interior the run moves the
spouse and the evacuees to a marker above the interior deck (z 1,507.9) and the play cast stands each on the highest
named platform part under it within a character's height. The window's platforms are every named one, of every
space: the vault exit's lift is placed in another interior cell whose coordinates overlap, and before its sequence
starts (800 s) its deck stands at its top, 1,453.8. Session B reached the cut at 644.9 s, before that, so the crowd
stood on the other cell's deck and kept it; session A reached it at 1,281 s, after it, and rode. A play cast now
stands only on platforms placed in the space of the cell it was built for (a viewer fix, no rule): replayed in the
window, B's crowd rides the deck down, 121.8 units from the walker at the bottom. Each change of the part an actor
stands on is logged with the parts under it.

**Doorways that look open while the prompt says open.** The house's pocket doors and the ruined front doorway on the
surface store ONAM (published as open by default) and their models have no frame, so
`play_door_collision_open_pose_v1` left them out of the walking world, the held doors' reading never held them, and
the window, which shows a door only by that reading, drew no leaf: the doorway looked open, the prompt read open and E
changed nothing. `--play-door-stored-open` (`play_door_stored_open_v1`) makes such a door open from load (drawn at its
Open sequence's last key, prompt close, E closes it and opens it again), and a door the reading never held carries
"(not drawn)". The seventeenth sessions' persons pressed E at such doors when it did nothing; with the switch those
presses close them, so their records replay their own tracks only without it.

**E on the front door during the evacuation.** `--play-door-toggle-blocked` (`play_door_toggle_blocked_unchanged_v1`)
leaves a door the reading shows open as it stands when the run's scripts last blocked its activation, and its prompt
is the name alone. The case that asked for it is not such a door: at the evacuation's stages the scripts open the
front door and unblock it (`BlockActivation(false, false)` after `SetOpen(true)`), so a person's E closes it under the
toggle and a second E opens it again; the sixteenth session's record replays as before.

**The floating infant.** The occupant stands 48 units above the nursery's floor at its own stored place. The crib is a
persistent activator 5.7 units away at floor height, in the worldspace's persistent cell, which no load read (the play
crib furniture the run seats the player in is a persistent furniture whose model is an invisible marker). With
`--draw-world-persistent` (`world_persistent_refs_drawn_in_square_v1`, 107 persistent references joined at the house)
it joins the scene, refused for its animation (a controller manager, its sequences, a behaviour graph) and its
reference's XLRT; `--draw-activator-parts` (`acti_unmoved_parts_drawn_v1`) draws it by the shapes no controller
reaches. The infant lies in it. The prompt's "(not drawn)" now also follows what a play cast's build draws.

**The house's furniture.** Its 14 furniture placements were refused for FURN's PNAM, FNAM, MNAM, SNAM and WBDT alone
besides blockers already under rules: `--draw-furniture-fields` (`furn_fields_render_unchanged_v1`) draws them.

**Heads.** A head part with conditions is evaluated by the run's evaluator against the stages the run's timeline has
set by the frame's second (`--actor-head-conditions`, `npc_head_part_conditions_run_stages_v1`): a list that holds
draws the part, one that does not leaves that part out, an undetermined one keeps the head refused.

**Remains and the standing skeleton.** The checkpoint's skeletons and the remains beside the exit's device are
skinned ragdoll models (a node per bone with a collision object and a physics system) whose stored pose is upright
(object bounds 131 units tall); the window drew them so under their references' rotations, which for the exit's
remains turns the figure about 177 degrees about x, so it hangs under the grating. Every such reference stores XRGD
(ragdoll data, not read): `--skinned-models-ragdoll-left-out` (`refr_ragdoll_pose_unread_left_out_v1`) leaves them out
instead of drawing a pose the game does not show. The tuft over the house frontage was not identified: no placement
within 2,500 units of the eye, among the cells' and the worldspace's persistent references, has a hanging plant's
editor id above the yard.

**The go-back cue.** It names only places within 2,048 units of the feet (measured to the volume's nearest point),
not a volume the feet stand in nor one whose hold changed without an event, and nothing while the window loads; the
watch log's cue line carries the volume's key. Session A's 186-unit cue in the house stays; the 4,601-unit cue on the
hill is gone.

**The console's confirmation.** A press that changed a stage or scene waited up to ten seconds for a subtitle while
the previous confirmation stayed up; it is now shown at once when the target's scripts name what they activated.

### The pods' collision, the seat, and the status line at the end (the twenty-sixth-session step)

**The pods.** Both cryo pod bases' models store collision objects on one physics system; the bodies were refused at
collision readiness by the furniture's unread record fields (`PNAM`, `FNAM`, `MNAM`, `SNAM`, `WBDT`, the object
template `OBTE`/`OBTS`, `ATTX`), the reference's `XLRT`, one base's scripts (`VMAD`) and, in the model, animation
blockers where the doors' sequences can move the bodies' chains (`readiness::Bearing::Moved`). With
`--play-collision-furniture-fields` (`furn_fields_collision_unchanged_v1`) and `--play-collision-furniture-at-rest`
(`furn_collision_at_rest_v1`) the bodies stand at their stored transforms, as the drawing at rest shows the doors
closed. Every other furniture of the loaded cells gains its collision the same way.

**The seat.** The run seats the player in the dream's pod and, on waking, in the vault's pod (a fragment's
activation read as a sit; `play_player_sit_furniture_v1` follows it into the other space). Their base stores one SNAM
marker: offset (0, -63.1, 0) with heading -pi in the pod's axes, toward its front window. With
`--play-player-sit-marker` (`play_player_sit_marker_v1`) the walker stands there, not at the pod's stored position;
with `--play-sit-passable` (`play_sit_furniture_passable_until_left_v1`) the pod it sits in has no collision until the
walker's feet leave the pod's object bounds (OBND through the placement, widened by the capsule's radius), then has
it again. The run's behaviour graph of the vault's pod enters its exit state 11.3 s after the sit and its unoccupied
state 16.6 s after it; the window does not run the pods' sequences, so the pod stays drawn closed.

**The status line at the end.** After Escape the run goes on to its end and writes its outputs while the play clock
stands; the line said "loading" (the standing clock) on the last frame. It now says "ending" once the input is over,
and the window's log prints `play: viewport status at S s of the run: CAUSES` each time the causes change (a cell
loading, an upload, a cell held for its cast, a scene to spawn, the walker building its walking world, the play clock
standing, the input ended, paused).

### Persistent references, furniture census and the platform cue (the twenty-seventh-session step)

**The sofa and the crib.** The person walked across the living room's sofa and the nursery's crib. Neither is in the
house cell's own groups: the sofa is a persistent furniture and the crib a persistent activator of the worldspace's
persistent cell, which the drawing joins (`world_persistent_refs_drawn_in_square_v1`) and no walking world read. With
`--play-collision-persistent` (`play_world_persistent_refs_walk_in_square_v1`) each walking world's build joins the
persistent references standing in its loaded grid squares before its collision is read
(`fo4_scene::grid::absorb_persistent`, the same join as the drawing's), and their collision stands as any loaded
reference's; `play.txt` counts them (`persistent references of the loaded squares`). A capsule walked along the
person's two lines (`column_probe` with `COLUMN_PROBE_WALK`) now stops at both.

**The census.** `column_probe` with `COLUMN_PROBE_CENSUS=FURN,MSTT,ACTI,CONT` lists every placement of those bases
whose bounds meet a rectangle: whether its model stores collision (the bodies `collision::instance_as_stored`
places), whether that collision is in the walking world, and the blockers when it is not. In the house every large
furniture stands; the containers (dressers, changing table, washer, dryer, wet bars) store collision but are refused
for their unread container fields and drawer sequences, and are not drawn either; scripted activators (sink, tool
box, tricycle, broom, hamper) are refused for their scripts. In the vault the benches and tables were refused for one
unread four-byte furniture subrecord, `FNPR`, alone: it joins the furniture fields both readings set aside
(`furn_fields_render_unchanged_v1`, `furn_fields_collision_unchanged_v1`), so they are drawn and solid.

**The exit deck's wait.** After E at the exit's console the run's graph starts the gear door's opening sequence about
15.7 s later; the deck's two parts stay at their stored transforms, their near edge 53 to 55 units above the floor
(over the 20-unit step), for about 36 s of that sequence, then slide toward the door until its last key at 43.3 s;
the deck is walkable from then (session B crossed 0.4 s after it). The wait is the data's sequence. The status line
now says `platform moving - ready in N s` while a named platform near the walker (within 300 units of a moving
part's box, in its space; since round 30 only one that carries the walker or that the run set moving after the
walker's activation) moves under a sequence the run started, N the whole seconds to the last
key of the started sequences (`fo4_play::elevator::Platform::moves_until`), then `platform ready` for 4 s (viewer
policy, `hud::platform_status`); the window's log names it among the status line's causes.

### The road's dip, the stairs' table, the platform cue's platform (the twenty-eighth-session step)

**The stop before the larger bridge.** The person stood held on open asphalt on the way back (B, about 2152 to
2182 s). Replayed with `--play-report-walls` the walker is blocked by the road's own collision mesh, a placed
reference of the cell (a compressed mesh, drawn, enabled, not persistent); a capsule set at the session's feet
and heading (`column_probe` with `COLUMN_PROBE_WALK`, `COLUMN_PROBE_TRIS`) is held the same with
`--play-collision-persistent` off: round 29's join is not the cause. The mesh has a dip there about 8 to 10 units
deep with a curb beside it. Each tick the step-up's retry with the tick's own move landed back in the dip a few
hundredths of a unit further along its rim and was taken, so the retry one radius ahead was never tried (it lands
20.6 units up, on the curb's edge, past the step); half a radius ahead it lands on the rim. With
`--play-step-lookahead` (`walk_step_lookahead_unless_climbed_v1`, in the preset; spec/walk.md) a retry that does
not climb waits while the retries one radius, half and a quarter of a radius ahead are tried, and the capsule walks
out of the dip on the person's heading. Off, every walk is as before.

**The persistent references the walking worlds join.** Over the exterior cells the 28th sessions' walking worlds
were built about (`column_probe` with `COLUMN_PROBE_PERSISTENT=1`), the join adds 656 persistent references; 70
stand in the walking world: 60 statics (59 drawn by the loader's reading, one a collision marker's box, which is
solid without a drawn shape by `play_collision_marker_boxes_v1`), four furniture and two activators whose drawing
blockers the window's furniture and activator switches set aside (the sofa and the cribs are drawn in the sessions'
frames; a ruined chair, a couch and an ambush furniture were not checked in a frame), two doors the held doors'
reading draws, and two water planes near the ruined town (activators whose model the drawing refuses for its water
shader): solid and not drawn, met by neither session, left as they stand.

**The stop on the line to the control-room stairs.** Not the same cause: the walker meets the collision of a
vault table placed in the vault cell itself (a static, enabled, not persistent), a 72-unit panel and a low plate
standing between the walker and the stairs. The window's build keeps the table (it is not among the placements the
build leaves out) but its frames show the floor where it stands. Why the table is not seen is not settled; nothing
changed.

**Interior fit.** The cafeteria jamb: the wall piece's collision has a slanted face rising from the floor about 20
units into the doorway (from floor level to 56 units up), which holds the capsule (a 69-degree contact); the data's
shape, nothing changed. The recreation desk is a container: neither drawn nor solid (its container fields and
drawer sequences are not read); what stands under the eye there is the floor mat. The sofa's collision is a coarse
wedge whose front slopes 16 units in from the floor to the seat and whose top stays below the drawn back, so the
capsule stops at the collision while the eye, 16 units from it, sees the drawn cushions overhang: the data's
shapes and the provisional radius, nothing changed.

**The platform cue.** The status line's countdown spoke of any named platform near the walker; it now speaks of a
platform that carries the walker or whose sequence the run started at or after an activation by the walker in the
same space (`Platform::after_activation`): the exit deck after the console, not the lower grille after the descent
nor the surface lift after the ride.

**Round 31.** *Stops on clear ground.* The 29th session's B stops by the barricades, on the line to the surface lift
and beside the first bridge's rock are not the step-up's look-ahead (`walk_step_lookahead_unless_climbed_v1`): the
same record replayed with the switch off stops at the same three places within a unit. Each is a drawn shape below
the eye's frame: the walker wedged between a 45-unit movable static (its hulls taller than the step) and a barrier's
rail 24 units above the feet; between the same kind of movable static and a crate on the lift's apron; slid along a
rock's mesh into terrain sloping 52 degrees, past the slope limit. Nothing changed for them.

*The surface lift.* The vault's ride up ends at the load door to the surface at the second the run's graph starts
the surface lift's rise; the door's place is the top the lift's deck reaches at that sequence's last key, while at
the crossing the deck stands far down its shaft under the closed hatch. Put at the door's place, the walker stood on
the hatch, was carried sideways as its halves slid apart and fell onto the rising deck. `play_run
--play-elevator-door-arrival` (`play_elevator_door_arrival_on_ride_v1`, in the preset; with
`--play-elevator-rigid` and `--play-load-doors`; `Elevator::arrival`): a load door whose place lies where a rigid
part of a named reference of the new space stands at its track's last key, while that part's motion still changes,
puts the walker on the part where it stands at the crossing (the place carried back with the part); the part
carries it up to the door's place.

*The cue on a settled deck.* `Platform::moves_until` is now the last key at which a part's motion changes (a held
tail of keys stands), and `Platform::carried_until` the same for the part carrying the walker; the status line
counts down to the carrying part's own end while one carries the walker, so a settled deck says nothing while the
rest of its sequence (the lower grille's column) still moves.

**Round 32.** *The platform cue.* A sequence that holds a deck's pose repeats its keys with about a thousandth of a
unit of noise; read key by key that is motion, and the cue counted down on the surface lift's upper deck while it
stood (the 30th sessions). The cue now reads a part's visible motion (`fo4_play::elevator::CUE_STILL`, 0.05 units of
any corner of the part's box from where it stood; stretches of motion less than `CUE_JOIN`, 1 s, apart are one;
viewer policy, the rides and every rule keep reading every change) and speaks only of the part carrying the walker:
`platform moving - ready in N s` while that part moves (`Platform::carried_moving`, to `Platform::carried_until`),
`platform starts in N s` while it stands with a later start in a sequence the run has started
(`Platform::carried_starts`), `platform ready` for 4 s after, nothing when no part carries the walker (a platform the
person's activation set moving beside the walker, such as the exit ramp seen from the fixed floor, says nothing).
After the load door onto the rising surface deck the count is the deck's own rise, which moves from its sequence's
first key. `play_run --play-report-platforms` (a report, no behaviour) logs each change of a named platform's state
under the walker.

*The title at a cut.* The readout is built before the play mode sets the camera from the walker's feet of the frame;
at the cut onto the rising deck the title named the eye of the frame before (the walker standing at the door's
place on the hatch during the load) over a picture of the shaft. The play mode now writes the eye it sets the camera
to into the readout of the same frame.

*Small movable statics.* What the data stores for the movable static that held the walker on the line to the lift's
apron: its base record is a movable static (MSTT), its object bounds about 43 by 43 by 45 units, its model's
collision one physics system whose body links a dynamic compound shape of eight convex hulls, each with a mass
properties object, and whose system stores one element in the arrays at +32 and +48 that the static crate's system beside it
leaves empty (spec/havok-bodies.md); the layout of those objects, and so a mass, a motion type or a layer, is not
read. `play_run --play-light-movables` (`play_light_movable_static_passed_v1`, pending, in the preset;
`fo4_play::world::light_movable`, `leave_out_references`): a movable static whose bounds times its scale are at most
64 units along each axis (`LIGHT_MOVABLE_MAX`, the assumption in place of a mass) has no collision in the walking
world or its dormant piece; the drawing keeps it; nothing is pushed. Statics and larger movable statics keep
blocking: the crate beside it still stops the walker.

*Held below the frame.* `--play-held-cue` (a viewer aid, no rule): the walker keeps its hold
(`fo4_play::walker::Held`, `next_held`: a blocked tick with a direction key held, the feet within 8 units across
since) and the window's status line says `blocked` after a second, `blocked - low obstacle` when the top of the
primitive met last stands at most 48 units above the feet (`fo4_walk::WorldSet::entry_bounds`). It names nothing.

*Where a hold's time goes.* Built headless in a fresh process, the dream's walking world takes 7.4 s: 4.6 s loading
the cell's scene, 0.5 s reading its collision, 1.7 s for the markers and doors, 0.5 s for the dormant piece; a grid
square by the first bridge 9.2 s, 8.0 s of it loading the nine cells' scene. A memo of parsed collision models
would cut little; each walking world builds its scene with a new loader.

## What the play mode does not do yet

- The elevators ride only those the caller names (`--play-elevator`), headless and in the scripted window;
  the window draws a named reference's parts where its collision stands (`--draw-platforms` draws them as
  stored whatever their render readiness; with `--draw-platform-skins` their skinned shapes whose bones go with
  several parts are skinned again as the parts move, the draw-gate step). The exit elevator's descent before the player reaches it is the caller's assumption
  (`--assume-graph-event`): nothing in the run's data sends it.
- The exterior's collision platform that an enable marker holds while the elevator rises (the exterior
  trigger's script disables the marker after waiting 12 s; the interpreter does not time the wait) is not
  read: the walker stands on the exterior elevator's own hatch and platform instead.
- The checkpoint's gate frame stands only with `--play-collision-static-fields` (the gate-frame step) and is drawn
  only with `--draw-static-fields` and `--draw-static-leaves` (the draw-gate step), its leaf in one of two poses,
  never swinging; the checkpoint's box has nothing to draw. A static whose collision chain a controller moves is
  still refused, and the collision of any model whose animation moves a node a body reads is not posed by the
  clock.
- Plain doors: no collision is placed for any door except, with `--play-door-open-collision`, the open pose of a
  door with its own frame (the corridor-fall step), so no leaf blocks the walker; the open state is not read, but with
  `--play-held-doors` a door the run's scripts close with its activation blocked holds the walker (the board step),
  and with `--play-held-doors-at-load` a door the person opens that stores no `ONAM` counts as closed from load
  (the door-shut step). A door's opening by a person's activation is read only for the doors the caller names.
- Doors are drawn only with `--draw-held-doors`, and only those the held doors' reading has held; the swing is not
  played (shut, then at the sequence's last key).
- Moving collision with a node (an animated door, a platform) is modelled only for the references the caller
  names (above); the live world moves only what the run moves.
- A person's keys drive `play_run`'s walker in the window (`--play-keys`), checked by the synthetic person, the
  unit tests and a tester through a desktop-automation tool (the person and way-out steps), not yet by a person
  at a physical keyboard and mouse. Of the run's caller assumptions the front door's open and the exit's two
  activations are the person's (`--play-activate-opens`, `--play-activate-person`); the earlier vault
  activations, the talks, the actor moves and the player's animation events stay timed (held until their
  stages; with `--play-action-after-offset` each keeps its offset from its stage, so a late person gets them
  spaced as the caller wrote them; with `quest_run --action-after-actor-moves` in the run's arguments the
  spouse's two moves are held until the stage that sends her, the deck step; the exit elevator's descent event
  stays at its second).
- A grid square's drawing is loaded whole by the window (the early step keeps the scene assets of one load for the next,
  the tenth-session step reads the first cell again for them and lets a cast build give way, the eleventh-session step
  keeps the files read: 0.3 to 1.4 s headless for the opening's outdoor squares; the fifteenth-session step keeps
  what a new asset's visit read, refreshes a kept file on use and keeps up to 1 GiB), overlapped with the walker's
  build since the person step; its models are still parsed and its meshes built again for each load, and nothing
  of the next square is read ahead.
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
| `play_activate_actor_box_v1` | `--play-pick-actors` | chosen: the run's actors are in neither the walking world nor the object bounds, and no data of this step gives an actor's activation shape; the box (40 by 120 units, standing where the run last put the actor) is an assumption; off by default and not in the preset, so recorded inputs replay | weak |
| `caller_action_after_keeps_offset_v1` | `quest_run --action-after-offset` (`--play-action-after-offset`) | how the opening's gates are written (each names the stage the caller's previous action sets); a reading of the caller's own timed assumptions | weak |
| `play_activate_written_second_v1` | `--play-action-after-offset` | the person's activation and the caller's timed one are the same event of the story; that the caller's later events follow it by the written gaps is prior knowledge of how they were timed | weak |
| `caller_actor_move_after_v1` | `quest_run --action-after-actor-moves` (in the run's arguments) | how the opening's two moves of the spouse are written: 54 and 100 s after the stage whose fragment moves her to the escort marker in the reference run; that her walk follows that stage by the same time whatever the player's pace is prior knowledge of the caller's timing | weak |
| `dialogue_hello_owed_v1` | `quest_run --hello-owed` (in the run's arguments) | the hello line's own condition on the checkpoint talk's stage; that an actor greets a player already near it once its line holds is prior knowledge | weak |
| `pack_destination_outlives_scene_v1` | `quest_run --package-moves-outlive-scene` (in the run's arguments) | the escort's package actions and the boarding trigger's Stop; that an actor on its way goes on when its scene stops is prior knowledge, the arrival second the caller's | weak |
| `play_elevator_end_landing_v1` | `--play-elevator-door-landing` (in the preset) | the play mode's own trace of the missed ride end; a policy of the ride's end | weak |
| `play_elevator_door_arrival_on_ride_v1` | `--play-elevator-door-arrival` (in the preset since round 31) | the surface lift's rise starts at the crossing and its deck's last key reaches the door's place; that a rider arrives with its lift is prior knowledge | weak |
| `walk_ride_rider_passes_terrain_v1` | `--play-elevator-through-terrain` (in the preset since round 26) | `ride_check --trace` on the vault elevator and the 25th session's second record: the terrain over the shaft pushes the riding walker sideways off the deck; no read data holes the terrain there | weak |
| `play_ride_terrain_cut_v1` | `fo4-view --draw-ride-cuts-terrain` (in the preset since round 27) | every LAND field of the shaft's cell read and the cell's quad bits clear: no data holes the terrain cone inside the shaft; the cut's box and condition are this reading's | weak |
| `play_collision_marker_boxes_v1` | `--play-marker-boxes` (in the preset since the gate-box step) | the gate's and the front door's boxes and the stages that disable them; that the obstacle flag makes a box solid for the player is prior knowledge | weak |
| `play_collision_marker_box_layer_passed_v1` | `--play-marker-box-layers` (with `--play-marker-boxes`; in the preset) | the boxes' `XTRI` words over the load order: the gate's 35, the front door's 40, the vault exit's 49 with an editor id naming a navigation cutter; 620 of 860 boxes of 49 under enable parents, 131 of 208 boxes of 31 tilted; what each word means and that a character passes the pick and cutter layers is prior knowledge of the engine's layer list | weak |
| `play_door_closed_blocked_holds_v1` | `--play-held-doors` (in the preset) | the house door's closing, its blocked activation and the evacuation stage that opens it, as the run's scripts call them; the two natives' meaning from the Creation Kit's documentation; that a closed door is solid and its object bounds stand for its solid part is prior knowledge | weak |
| `refr_static_fields_collision_unchanged_v1` | `--play-collision-static-fields` (in the preset since the gate-frame step) | the published field names of `XACT`, `XLRT`, `XOWN` and `ONAM`; a load-order census of the statics storing them (36 admitted, among them the checkpoint gate's frame); that a static has no door, activation, ownership or location behaviour that moves its collision is prior knowledge | weak |
| `acti_fields_receiver_collision_unchanged_v1` | `--play-collision-activator-fields` (in the preset since round 26) | the published field names; the cabinet's model stores a collision object refused only by these record blockers | weak |
| `play_collision_marker_box_layer_records_v1` | `--play-marker-box-layer-records` (with `--play-marker-box-layers`; not in the preset) | the load order's collision layer records read under their published layout, their lists symmetric; that index 30 is a walking character's layer and what a listed pair does to a box are prior knowledge; the records list 39, 40, 41 and 43 with it, against the constant | weak |
| `play_door_closed_at_load_v1` | `--play-held-doors-at-load` (with `--play-held-doors`; in the preset since the door-shut step) | the front door stores no `ONAM`; the field's published name; prior knowledge that a closed door is solid | weak |
| `play_door_drawn_as_held_v1` | `fo4-view --draw-held-doors` (in the preset since the door-shut step) | the door model's stored leaf and its sequences named Open and Close | weak |
| `play_platform_part_holds_walker_v1` | `--play-elevator` (the behaviour as it is) | the walls report on the second session's record | weak |
| `refr_static_fields_render_unchanged_v1` | `fo4-view --draw-static-fields` (in the preset since the draw-gate step) | the render side of `refr_static_fields_collision_unchanged_v1`; the views census of the opening (no other placement newly drawn) | weak |
| `play_static_leaf_two_pose_v1` | `fo4-view --draw-static-leaves` (in the preset since the draw-gate step) | the frame model's one clamped transform controller and its keys; the collision marker box inside the frame's bounds; which pose holds the way is a guess | weak |
| `play_platform_skin_follows_parts_v1` | `fo4-view --draw-platform-skins` (with `--draw-platforms`) (in the preset since the draw-gate step) | the skins as stored, read by the cast's readers and checked against the built mesh at rest; each bone moved by its part as the collision is | weak |
| `play_door_collision_open_pose_v1` | `--play-door-open-collision` (in the preset since the corridor-fall step) | the vault door model's frame under no sequence and its parts at the last key of its sequence named Open; that this is the doorway's collision while the door stands open is prior knowledge | weak |
| `play_door_activation_opens_held_before_visit_v1` | the play mode's open of a held door (the behaviour as it is) | the eighth session's early E at the front door; whether the game refuses an activation a script blocked is not known | weak |
| `play_door_person_closed_actors_unchanged_v1` | `--play-door-toggle` (the behaviour as it is) | the play mode holds the person's close for the walker only; the run's actor moves read no door (synthetic tests); no session shows an actor meeting a shut door | weak |
| `static_unmoved_parts_drawn_v1` | `fo4-view --draw-static-parts` (in the preset since the eleventh-session step) | nifxml's structure (children, a shape's links, controller targets, sequences' controlled blocks by node name); the room's one sequence names only its particle emitter and its transform controller no extra target; that a graph acts only through the model's sequences is prior knowledge | weak |
| `vm_furniture_activate_events_delivered_v1` | `quest_run --activate-furniture-events-deliver` (in the run's arguments) | that the second argument false raises the activation's events is prior knowledge; the exit's furniture's script and alias act only in `OnActivate` | weak |
| `vm_sit_events_activation_ended_by_track_v1` | `quest_run --sit-end-walk-events 64` (in the run's arguments) | the walker never sits in a furniture in its own space; the distance is the caller's; the game's own end of the take is not read | weak |
| `refr_script_render_as_stored_v1` | `fo4-view --draw-scripted-as-stored` (in the preset since the fourteenth-session step) | VMAD attaches scripts (a format fact); the run applies their enable changes; a script's change of look is not drawn | weak |
| `acti_fields_render_unchanged_v1` | `fo4-view --draw-activator-fields` (in the preset since the fourteenth-session step) | that a marker colour, activator flags and an activate text leave the model drawn is prior knowledge | weak |
| `vm_sit_events_activation_ended_by_other_activation_v1` | `quest_run --sit-end-other-activation` (in the run's arguments since the fifteenth-session step) | a player seated in a furniture activates nothing else (prior knowledge); the walker never sits; the game's own end of the take is not read | weak |
| `play_platform_shape_nearest_part_v1` | `fo4-view --draw-platform-nearest-part` (in the preset since the fifteenth-session step) | the gear door's hub pieces on nodes the sequences set with no collision under them; that a child node moves with its parent is prior knowledge; the node's own motion is not drawn | weak |
| `furn_fields_collision_unchanged_v1` | `--play-collision-furniture-fields` (in the preset since round 28) | the published field names; the cryo pods' models store collision objects refused only by these record blockers (one base also by its scripts) and by animation blockers | weak |
| `furn_collision_at_rest_v1` | `--play-collision-furniture-at-rest` (in the preset since round 28) | the collision side of `furn_parts_drawn_at_rest_v1`: the pods' bodies at the stored transforms, as their doors are drawn | weak |
| `play_sit_furniture_passable_until_left_v1` | `--play-sit-passable` (with `--play-player-sits`; in the preset since round 28) | the run's sits of the player in the dream's pod and the vault's pod; that the player leaves the pod through its opened front is prior knowledge, the passage through the stored shell this reading's | weak |
| `play_player_sit_marker_v1` | `--play-player-sit-marker` (with `--play-player-sits`; in the preset since round 28) | the pods' base stores one SNAM marker 63 units toward the front window (read under `furniture_marker_snam_v1`); that a sitter stands at its marker is prior knowledge | weak |
| `play_world_persistent_refs_walk_in_square_v1` | `--play-collision-persistent` (in the preset since round 29) | the drawing's join of the same references (`world_persistent_refs_drawn_in_square_v1`); the sofa's and the crib's models store collision and stood in no walking world | weak |
