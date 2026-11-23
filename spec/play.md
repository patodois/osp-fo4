# The play mode: a quest run driven by input

`quest_run new-game --track FILE` runs the scripts along a track its caller wrote. The play mode
(`crates/fo4-play`) gives the run its track slice by slice instead: the walker (`fo4-walk`) moves over the
start cell's placed collision, driven by a scripted input (`play_run`, headless) or by a window's keyboard and
mouse (`fo4-view --play-run`), and the player's activations come from what the eye's ray meets. Nothing here was
checked against the running game: every behaviour reading is a pending rule of `hypotheses.tsv`.

## How the run is stepped

`fo4-vm`'s library holds the pieces of a run (the interpreter, the quest host, the scene clock, the world
model's `settle` and `next_due` functions); the new-game run itself (setting up the host and the world model
from the command line, the clock loop that interleaves timers, the scene clock, the caller's actions and the
world model's dues) lives in `quest_run`, an example, and takes the whole track up front:
`fo4_vm::track::Place::load_with` reads the volumes near the track and computes every trigger crossing of the
whole track at load, `fo4_vm::talk::load_hello` every hello crossing, and the clock loop runs to the end.

What the clock loop does between two track points: at each turn it takes the earliest of the next timer, the
next scene clock due, the next timed caller action and the world model's next due second (the next crossing,
a distance registration's first second on the track, an assumed actor move, a hello crossing, a line half, an
animation or load due), moves the run's clock there and runs what is due. The world model's dues are read from
the track at each turn (crossings from the list computed at load). So the run consumes the whole track up
front only through the crossing lists and through one lookahead (below); everything else is asked again each
turn.

Two ways to feed it input were possible:

- Re-run `quest_run` with the track walked so far. Measured on the opening's first minute (the furthest run's
  switches with the track cut at 60 s): 43 to 86 s per run on this machine (the whole furthest run: 179 to
  345 s). A window needs the run's state every frame: re-running is not usable.
- Step the run: the same clock loop with a horizon. This is what `fo4-play` does.

The stepped loop (`fo4-play`, `run::play`): the new-game code is `quest_run`'s own source, compiled into
`fo4-play` by its `build.rs`, which patches exactly three places (each must match once or the build stops,
naming it): the file's inner attributes are dropped, `run()` takes its arguments from the caller
(`run_with`), and `new_game` calls `play::play_loop` where it called `clock_loop`. With no driver installed,
`play_loop` calls `clock_loop` itself. With a driver:

- At the loop's start the caller's track is cut to the run's present second (second 0): its first point is the
  start; its points choose the cells the world model reads (as without play); its later points are replaced by
  the driver's.
- Each turn computes the same candidates as the clock loop. When the earliest lies after the horizon (the
  second the driver's last slice reached), or nothing is due, the driver is asked for the next slice: track
  points (none while the player stands still), the second `until` it reaches, and the player's actions. The
  player stands at the last point until `until`.
- New points are folded into the crossing lists move by move, exactly as `fo4_vm::track::intervals` folds the
  whole track (a unit test checks every prefix of a synthetic track against `intervals`); the crossings
  already applied must not change (counted: 0 in every run below).
- An action of a slice joins the caller's timed actions before those of the same second (the input first on
  a tie).
- `Finish` ends the input: the loop goes on as the clock loop does after the track's end. `Stop` (a window
  closing) ends the run where it is.

The one lookahead: with `--cond-1-distance` (`cond_fn_1_track_distance_v1`) a scene phase that starts waiting
on completion conditions with a function 1 condition gets a mark at the first second the whole track brings
the two parties within the distance. The stepped run knows only the track to the horizon then. So when the
track grows, each scene waiting on false completion conditions with a function 1 condition and no mark left
is offered its completion again (`fo4_vm::scene::step_call`), which reads its marks on the longer track; an
offer that finds the conditions still false logs one more wait than `quest_run` does.

### Request to `fo4-vm`

The play mode compiles an example's source and calls its private functions; a change to `quest_run.rs`
(another step is editing it) may break `fo4-play`'s build until the patch points or the helpers it calls
(`act`, `world_step`, `recheck`, `gate_done`, `timeline`, `time_scale`, `Then`, `Caller`) are matched again.
The fix belongs in `fo4-vm`:

1. A library run: `fo4_vm::run::NewGame` built from the parsed switches (today's `Then` and `Opts`, made
   public with their parser), owning the host, the library, the attachments and the runner, with
   `fn advance_to(&mut self, until: f64, points: &[track::Point], actions: &[(f64, player::Action, FormKey)])
   -> Result<(), Error>` (the stepped loop above), `fn finish(self) -> Outputs` (the census, the names and the
   timeline) and read access to `&Vm`. `quest_run new-game` would become one `advance_to(f64::INFINITY, ...)`.
2. `track::Place::extend(&mut self, points: &[Point])` folding crossings as the play mode does, and the same
   for `talk::Talk`'s hello crossings.
3. `scene::recheck_distance_marks(h, w)` recomputing the function 1 marks of the scenes waiting on completion
   conditions when the track grew, without logging a new wait when nothing changed.

## Checks of the stepped loop (outputs outside git)

The run's own track given back in slices (`play_run --replay STEP`, `fo4_play::Replay`, the track as the run
loaded it, with the points `--load-doors` adds):

| Run | Slices | Census | Names | Next blockers (2 files) | Timeline |
| --- | --- | --- | --- | --- | --- |
| first minute (furthest run's switches, track cut at 60 s), 0.1 s | 5 | SAME | SAME | SAME | SAME |
| furthest run (`spec/quest-run-home.txt`), whole track in one slice | 1 | SAME | SAME | SAME | SAME |
| furthest run, 0.05 s | 171 | 2 lines | 8 lines | SAME | 2 lines |

In the last row the differences are the two re-offers above (a greeting scene in the postwar vault waiting on a
distance condition, offered again at 567.125 s and 573.792 s): two more waits in the scene log; its phase then
completes at the same second as in `quest_run` (584.069 s), and every stage of the run (288) is set at the same
second.

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

## What the play mode does not do yet

- The walking world is the start cell's as loaded: the run's enables and disables do not change the
  collision (a disabled reference's collision is passed through by the activation only), references the run
  enables are not there, doors do not open, and the walker cannot change space (load doors).
- The window draws the cell as loaded: the run's visibility changes, moves and actors appear only in frames
  drawn from the timeline (`--timeline FILE --at S`).
- The reach of an activation, the walker's speed and its tick are the caller's; the activation key and the
  crosshair are the eye's ray (no prompt, no activation distance from the data).
- Sound, menus, combat, the player's animation events: still the caller's.

## Rules

| Rule | Switch | Basis | Strength |
| --- | --- | --- | --- |
| `play_player_point_walker_feet_v1` | the play mode | the walker is this project's controller; which point of the player triggers test is not in the data | weak |
| `play_run_clock_input_seconds_v1` | the play mode | none measured: frame time and script time are taken as equal | weak |
| `play_activate_collision_ray_v1` | the play mode's activation | a crosshair as an eye ray (prior knowledge); reach is the caller's | weak |
| `play_activate_object_bounds_v1` | `--play-pick-bounds` | OBND is a format fact; its use as the activation volume is a guess, needed because the door has no placed collision here | weak |
