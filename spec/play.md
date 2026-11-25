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
- A grid square's drawing is loaded whole by the window (the early step keeps the scene assets of one load for the next:
  2.1 to 3.2 s after the first crossing, 5.9 s for the first), overlapped with the walker's build since the person step.
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
| `caller_actor_move_after_v1` | `quest_run --action-after-actor-moves` (in the run's arguments) | how the opening's two moves of the spouse are written: 54 and 100 s after the stage whose fragment moves her to the escort marker in the reference run; that her walk follows that stage by the same time whatever the player's pace is prior knowledge of the caller's timing | weak |
| `dialogue_hello_owed_v1` | `quest_run --hello-owed` (in the run's arguments) | the hello line's own condition on the checkpoint talk's stage; that an actor greets a player already near it once its line holds is prior knowledge | weak |
| `pack_destination_outlives_scene_v1` | `quest_run --package-moves-outlive-scene` (in the run's arguments) | the escort's package actions and the boarding trigger's Stop; that an actor on its way goes on when its scene stops is prior knowledge, the arrival second the caller's | weak |
| `play_elevator_end_landing_v1` | `--play-elevator-door-landing` (in the preset) | the play mode's own trace of the missed ride end; a policy of the ride's end | weak |
| `play_collision_marker_boxes_v1` | `--play-marker-boxes` (in the preset since the gate-box step) | the gate's and the front door's boxes and the stages that disable them; that the obstacle flag makes a box solid for the player is prior knowledge | weak |
| `play_collision_marker_box_layer_passed_v1` | `--play-marker-box-layers` (with `--play-marker-boxes`; in the preset) | the boxes' `XTRI` words over the load order: the gate's 35, the front door's 40, the vault exit's 49 with an editor id naming a navigation cutter; 620 of 860 boxes of 49 under enable parents, 131 of 208 boxes of 31 tilted; what each word means and that a character passes the pick and cutter layers is prior knowledge of the engine's layer list | weak |
| `play_door_closed_blocked_holds_v1` | `--play-held-doors` (in the preset) | the house door's closing, its blocked activation and the evacuation stage that opens it, as the run's scripts call them; the two natives' meaning from the Creation Kit's documentation; that a closed door is solid and its object bounds stand for its solid part is prior knowledge | weak |
| `refr_static_fields_collision_unchanged_v1` | `--play-collision-static-fields` (in the preset since the gate-frame step) | the published field names of `XACT`, `XLRT`, `XOWN` and `ONAM`; a load-order census of the statics storing them (36 admitted, among them the checkpoint gate's frame); that a static has no door, activation, ownership or location behaviour that moves its collision is prior knowledge | weak |
| `play_collision_marker_box_layer_records_v1` | `--play-marker-box-layer-records` (with `--play-marker-box-layers`; not in the preset) | the load order's collision layer records read under their published layout, their lists symmetric; that index 30 is a walking character's layer and what a listed pair does to a box are prior knowledge; the records list 39, 40, 41 and 43 with it, against the constant | weak |
| `play_door_closed_at_load_v1` | `--play-held-doors-at-load` (with `--play-held-doors`; in the preset since the door-shut step) | the front door stores no `ONAM`; the field's published name; prior knowledge that a closed door is solid | weak |
| `play_door_drawn_as_held_v1` | `fo4-view --draw-held-doors` (in the preset since the door-shut step) | the door model's stored leaf and its sequences named Open and Close | weak |
| `play_platform_part_holds_walker_v1` | `--play-elevator` (the behaviour as it is) | the walls report on the second session's record | weak |
| `refr_static_fields_render_unchanged_v1` | `fo4-view --draw-static-fields` (in the preset since the draw-gate step) | the render side of `refr_static_fields_collision_unchanged_v1`; the views census of the opening (no other placement newly drawn) | weak |
| `play_static_leaf_two_pose_v1` | `fo4-view --draw-static-leaves` (in the preset since the draw-gate step) | the frame model's one clamped transform controller and its keys; the collision marker box inside the frame's bounds; which pose holds the way is a guess | weak |
| `play_platform_skin_follows_parts_v1` | `fo4-view --draw-platform-skins` (with `--draw-platforms`) (in the preset since the draw-gate step) | the skins as stored, read by the cast's readers and checked against the built mesh at rest; each bone moved by its part as the collision is | weak |
| `play_door_collision_open_pose_v1` | `--play-door-open-collision` (in the preset since the corridor-fall step) | the vault door model's frame under no sequence and its parts at the last key of its sequence named Open; that this is the doorway's collision while the door stands open is prior knowledge | weak |
| `play_door_activation_opens_held_before_visit_v1` | the play mode's open of a held door (the behaviour as it is) | the eighth session's early E at the front door; whether the game refuses an activation a script blocked is not known | weak |
