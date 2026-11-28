# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-26 (vault, cast, light, home, character graph, fight, live window, museum, live world, run library, seen, past-the-museum, faces, third-fight, poses, town-chain, layers, wire, rotate, exit, window, looks, moving, drawn, hands, person, way-out, gate, deck, early, board, gate-box, gate-frame, door-shut, draw-gate, corridor-fall, ninth-session, fourteenth-session, fifteenth-session, sixteenth-session and seventeenth-session steps). Nothing below has been checked against the running game: every behaviour is a pending
rule in `spec/hypotheses.tsv`, off by default.

## Script VM and quests (`crates/fo4-vm`)

The forced opening (`quest_run`, record in `spec/quest-run-opening.txt`) reaches 87 of 104 stages: the
prewar quest ends. The vault step carried the forced run through the postwar vault (`spec/quest-run-vault.txt`);
the home step walks it (`spec/quest-run-home.txt`); the fight step carries it through the first fight and on to
the town (`spec/quest-run-fight.txt`); the museum step walks the museum in the order its data forces and on
through the second fight and the leader's conversation (record in `spec/quest-run-museum.txt`, every stage of the
three quests mapped in `spec/quest-run-vault-stages.txt`). The order comes from `walk_order_census`
(`spec/walk-order-census.txt`, `fo4_vm::walk_order`): over the museum's navigation mesh, with the doors that
store a lock record closed, the leader's inner door alone shuts the stage 78, 86 and 88 triggers from the front
door; only the scene that stage 85 starts unlocks it, and 78's trigger is off from 85, so 78 can be set only by
arriving through the back door, whose outer side no navigation mesh reaches (a labelled variant). The fight
itself is still not modelled: the caller assumes the seven raiders' deaths, and five new readings behind their
own switches carry the run on (a property on an object without an instance of its script, globals the run
sets, locks the run holds and the caller's unlock with its event, a scene's response actions). The next main
quest reached 34 of its 54 stages there (before 29). The step past the museum (record in
`spec/quest-run-onward.txt`, census `spec/quest-run-onward-census.txt`) takes the museum's four stops in order,
each behind its own switch: the terminal's first item runs its fragment and its lock change raises the gate's
event (95, 100), the core the caller puts into the power armour reaches the alias whose init handler named it
(105), the two activations pass on the player's perk list and the caller's strength (110, 115), and the
leader's faction answer from his base record lets stage 90's fragment finish, so the walk out onto the roof and
along the street sets the museum quest's 75, 76 and 77 and the creature's death 79: the next main quest reaches
**40 of 54** (adds 95, 105, 110, 115, 120, 123), the robot's postwar dialogue quest 10 of 16, the vault dialogue
quest 10 of 18 and the opening 87, unchanged. The earlier recorded runs with `--auto-state-inherited` set the
same stages at the same seconds. The caller still gives the walk's waypoints, the ride up the elevator, the
conversations with the robot and their lines, the player's choices, the speech check's condition, the robot's
food event, the player's own events, the fights' events, the inner door's activation, the terminal's item (or
the gate's unlock), the core put into the power armour, the two activations, the player's strength and the walk
off the meshes from the balcony to the roof and down to the street. The third fight's step (record in
`spec/quest-run-third.txt`, census `spec/quest-run-third-census.txt`, `spec/vm.md`, The third fight): behind
their own switches a created alias holds a run-only reference (`--alias-created-refs`) and a borrowed alias what
the other quest's alias holds when read (`--alias-external`), the caller's deaths name an alias
(`EDITOR_ID#ALIAS`), and a line's begin and end reach the line's own scripts (`--line-script-events`); with the
five raiders' deaths, the caller's entry into the lobby trigger (whose stage 135 fragment starts the scene no
scan had named a starter for) and the caller's conversation with the town's leader, the next main quest reaches
**47 of 54** (adds 122, 125, 127, 128, 130, 135, 150). The town chain's step
(record in `spec/quest-run-chain.txt`, census `spec/quest-run-chain-census.txt`,
`spec/vm.md`, The town chain): the town chain's first quest's winning record
is an add-on override with record flag bit 0x4000 carrying only its script,
read from the occurrence before it (`--quest-partial-previous`); the caller's
musket pickup raises the container change (`--container-change-events`, 62);
the caller's leaving the museum's location raises the location change
(`--location-change-events`) and `IsChild` answers from the locations'
parents (`--location-parents`), so the town dialogue quest's 200 sets the next
main quest's 200: **49 of 54** (adds 62, 200). 140 is set only by other
quests' stage 0 and 1 fragments nothing reachable sets; 75 and 1000 have no
setter in the data; the timer budget per timer (`--timers-per-timer`) moves
no stage.
What is left:

1. After 200: 140 is set only by the town chain's first quest's stage 1 (nothing sets it) and the second town
   quest's stage 0 (set only by other quests' stage 0 fragments, back to the end game): nothing the run reaches
   sets it; 78 behind the locked inner door; 75 and 1000 no setter (1000 carries stage flag 0x04, read by prior
   knowledge as a stage set at shutdown, which no reading applies); the robot's 275 is the only line of a
   companion topic whose one scene nothing in the data starts. The town chain's first quest runs from 20 (with
   `--quest-partial-previous`); its next stages wait on its actors' location changes and the settlement's
   workshop events. The created reference's place (`ALCA`) and level (`ALCL`) are not read, and the borrowed
   alias is a live link against the knowledge that such fills are made at the quest's start. The run's default
   for a stage no record holds is unchanged (`quest_set_stage_absent_open_v1`: 2,312 constant setters still
   name one with the partial reading on).
2. The robot's 250: a fourth conversation after the next main quest's 55, back at its home (the run does not
   walk back); the follower lines' group also asks condition function 566, which the run does not answer: its
   5,986 conditions compare with 1 or 0 a first parameter that is zero, a small number or a record of 17 types,
   and nothing in the data says what is compared (`spec/quest-run-third-census.txt`, section 4; a labelled
   variant assuming both from the town sets 250: `spec/quest-run-museum.txt`, section 7).
3. The fights themselves: nothing aims, loses health or decides to attack; no active magic effect receives an
   event; `GetDeadCount` and health natives are not answered.
4. The walk is checked against the census by hand (a script in the step's notes): the run itself does not stop
   a track at a locked door, and `track_plan` does not avoid doors or triggers; a planner that avoids the
   volumes the census closes would let the walk come from the data.
5. The elevator ride (`spec/play.md`, The elevator ride): the play mode moves the collision of the
   references the caller names as the sequences the run's graph starts move their nodes (translations only);
   the outside descent leaves the walker 1,626 units below the shaft's door, which the end-door reading takes
   (the caller's recorded track rode only to the door's height); past the interior elevator the floor comes
   from the entrance room the caller names as a still reference (its collision is refused as the cell is
   built), and at 581.8 s the walker drops into the pit under the gear door, whose collision is refused the
   same way. Layers step (`spec/collision-placement.md`, Blockers off the collision chain;
   `spec/collision-ready-census.txt`): behind `readiness::Switches` (off by default) the cell build admits a
   refused collision whose model blockers lie off the blocks its bodies read
   (`collision_model_blockers_off_chain_v1`: the entrance room's floor among 12) and a reference's unread
   alpha (`refr_xalp_collision_unchanged_v1`: two pieces of the vault corridor's floor among 3); the gear
   door's data says closed at load and open after its load event (its script plays the graph event whose
   state plays a sequence that holds the open pose, `nif_sequence_interpolator_held_pose_v1`). With both
   switches and the door in that pose, `ride_check`'s walker follows the recorded walk's targets from 520.67 s
   to 700 s. A local probe of the requested `fo4-play` change (never versioned: its walking world with both
   switches on, its elevator reading following held translations, the gear door named with
   `--play-elevator`, the run with `--load-on-arrival`) matches the recorded run's **232 of 232 stages to
   700 s** in the same order (before: 222 of 232); without the door's load event 221 of 232, the switches
   alone 222 of 232. Wire step (`spec/play.md`, Past the gear door, out of the pod and back to it): those changes
   are `fo4-play` switches (`--play-collision-off-chain`, `--play-collision-reference-alpha`,
   `--play-elevator-held-pose`; the gear door named and `--load-on-arrival` in the opening's arguments): the
   headless walk matches **232 of 232 stages to 700 s** byte for byte with the probe. Past 700 s, with
   `--play-player-moveto` and `--play-player-sits` (the walker goes where the run's `MoveTo` and its sit in
   the pod of the postwar vault put the player) and the postwar gear door named, the walk out of the pod along
   the recorded track matches **262 of 262 stages to 930 s**. It stops at 929.1 s at the open postwar gear
   door: its walkway stands about 50 units above the floor on the console's side, more than the walker's
   step, because the parts' open-pose rotations are not applied (translations only). Rotate step
   (`spec/walk.md`, Platforms that turn): the turns are constant XYZ rotation keys of the held-pose sequences
   (the ramp 7.25 degrees about its hinge), which no translation reading takes; `nif_sequence_chain_pose_v1`
   (`fo4_scene::sequence::chain_pose`) poses a body's chain with every channel and
   `walk_platform_rigid_motion_v1` (`fo4_walk::Motion`, `World::copy_sources_moved`, `platform::RigidRide`)
   moves and rides a piece by a turn about a pivot and a translation, both behind `ride_check --pose-rigid`.
   A local probe of `fo4-play` (never versioned: its elevator parts grouped by the deepest node a sequence
   sets and moved by `RigidRide` along each sequence's chain motion) matches **265 of 265 stages to 946 s**:
   the walker goes up the ramp, across the walkway and down to the exit elevator's room (the vault dialogue
   quest's 97, 98 and 100 added). It stands there from 945.8 s: the next recorded stage (946.05 s) comes from
   a trigger box 109 to 214 units up the shaft that the recorded player enters riding the exit elevator up.
   Exit step (`spec/play.md`, Up the ramp, up the exit elevator and out): the rigid reading is a `fo4-play`
   switch (`--play-elevator-rigid`, `play_elevator_rigid_v1`); named as stored, the exit elevator blocks its
   doorway because its stored pose (its graph's start state) holds the gate leaf down and the platform at the
   top, and no script of the run sends it the descent whose end pose (platform down, gate raised) the ascent
   starts from: the caller's `--assume-graph-event` (`anim_graph_caller_event_v1`) sends it at 800 s, the run's
   own ascent then plays at 945.35 s and carries the walker up; the ride's end door takes it outside, where
   the exterior elevator (named, its rise played by the exterior trigger's script) holds it and lifts it to
   the surface, and `--play-elevator-door-onward` (`play_elevator_end_door_onward_v1`) keeps the rise's end
   from taking it back into the vault. **274 of 274 stages to 960 s**, 276 of 277 to 1,000 s; the walker
   walks the recorded exterior track from 968 s and stops at 998 s against a dirt cliff face of 45.6 degrees
   (the walker's provisional slope limit is 45), before the settlement's trigger (the next main quest's 20 at
   1,005 s). Next: that slope (a walker parameter, drawing side) or a route around it, then the window.
   Also open (run-side enable state, not done): under the pod the run seats the player in at 730.1 s, the
   dream sequence's cell has two floor pieces whose collision is refused only for `XESP.own_flag` (a record
   below the root of their enable chain carries its own initially-disabled flag). The readiness that refuses
   them is the drawing side's (`fo4_scene` readiness, `RuntimeEnableParent`). Looks step: the drawing side's
   switch exists (`Loader::set_collision_child_flag_agrees`, `xesp_child_flag_agrees_chain_v1`, off by
   default); with it a probe copy of the play mode (`--play-collision-child-flag-agrees`, three plumbing
   lines and the live world's reading of such a chain) seats the walker on the floor at z -41.4 from 730.1 s
   to the next sit (airborne ticks 4,016 to 144), every stage at the same second as without it. Next (run
   side): those lines in `fo4-play`; the viewer's play options then take the switch.
6. The player's own animation events and the robot's food event still come from the caller: reading character
   graphs (below) would let them come from the data.
7. `--cond-70-player-sex` over the whole run settles 31 line choices of the opening and times them by their audio,
   which moves the opening's scenes past the caller's track and gates (the opening then stops at 31 stages): the
   opening's track and gates would need retiming before the reading can apply everywhere.
8. The timeline's deaths and sits (`--timeline-deaths`, `--timeline-sits`, `spec/timeline.md`) are written as
   `event` lines and frames read them (the seen step, below); the run seats nobody but the player. With
   `--sit-end-walk UNITS` the player's sits end when the track walks away (18 s and 248.3 s with 150 units),
   and with `--timeline-sequences` the timeline writes `sequence REF NAME` event lines for the sequences the
   graphs start (the proposal of `spec/timeline.md`, The elevator second; the reader there does not read them
   yet).

Character behaviour graphs (`char_graph_census`, `spec/character-graph-census.txt`; `char_start_clip_census`,
`spec/character-graph.md`): a race names its behaviour project; the readers now follow a graph from its root
through nested machines, blenders, selectors, behaviour references and the race's subgraph entries to the clip
it starts in (79 of the opening's 84 resolved actors). Transition conditions and what raises an event are
still not read; following transitions on events is what would let actor animation events come from the data
instead of the caller.

Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state's transitional values (only sitting or not, `--sit-state`); combat; actors sitting
through packages (and sitting poses: `spec/character-graph.md`, Sitting); magic-effect receivers of furniture events; keyword data, `ResetKeyword`, keywords in
Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state's transitional values (only sitting or not, `--sit-state`); combat itself (only its events, from the caller); actors sitting
through packages; magic-effect receivers of furniture events; keyword data, `ResetKeyword`, keywords in
conditions, template flags and object modifications for keywords; collection aliases filled other than from
a location's rows; actors' animation after their start state (character graphs: transitions on events); a furniture's `OnActivate` on a script's activation;
use events sent to a furniture's graph by the caller's own sits and get-ups.

## Drawing (`crates/fo4-scene`, `apps/fo4-view`)

The coverage census (`spec/coverage.md`) leaves 2.63 % of the placed area undrawn under hypotheses at second 0
of the game clock with every clock switch off, but 28.77 % at any later second (the distant clouds and other
effects whose falloff or emissive tracks move, `UnsettledClockField`). With the effect switches on it leaves
2.46 % at every second; with every switch on (effect fields and moving geometry) 2.44 % at every second (283,164
placements omitted, 1,299 fewer).

The clock drives, each behind its own switch, off by default: the effect emissive multiple, emissive colour and
falloff fields (`--clock-emissive-multiple`, `--clock-emissive-colour`, `--clock-falloff`), and since this step
the nodes that free-running transform controllers move (`--clock-translation`, `--clock-rotation`,
`--clock-rotation-xyz`, `--clock-scale`; `spec/controller-clock.md`, Moving geometry; census
`spec/motion-census.txt`). Only the drawn geometry moves: the collision of a moving node (88 models, 1,175
placements) stays where it is stored, and the viewer's moving grid does not take `--time`. The families that
lead what is still refused:

1. Effect controllers run by a manager (`effect-idle`). This step's census of their sequences
   (`spec/motion-census.txt`, section 3; `spec/controller-clock.md`, Sequences of the manager-run effect models)
   found no data-only signal (list order, a flag, text keys, the placed reference, the base record) that picks
   one sequence to play from load: the signals disagree on most models. The one marker remains the behaviour
   graph's start state; a rule needs a sequence player (a sequence's own timing and its transform interpolators,
   which the transform track of this step can now evaluate) and settles `behavior-graph` at the same time.
2. Models whose sequence is chosen by a behaviour graph (same machinery as 1).
3. Moving geometry left out: TBC keys (93 varying tracks), moving root nodes (`UnsettledClockRootTransform`),
   path interpolators; moving collision with its node (the walking world kept in step) is not started.
4. Record runtime state on placements (several refused subrecord families, listed in the census).

Characters (`spec/character-graph.md`): behind four switches (`--actor-start-clip`, `--actor-subgraph-keywords`,
`--actor-clip-folder`, `--actor-start-clip-loop`) a timeline frame's characters stand in the clip their race's
graph reaches from its root (a standing idle) instead of the rest pose; the CPU render and the headless viewer
agree (vault frame at 560 s: coverage 1.000000, same tag on every pixel). What is left:

1. Leveled and templated actors (faces step, `spec/actor.md`, Picked looks): behind four switches
   (`--actor-template-bits`, `--actor-leveled-seed SEED`, `--actor-leveled-chance-none`,
   `--actor-leveled-use-all`) the template slot of bits 0, 8 and 12 gives the traits, outfit and keywords, and
   the caller's seed picks one entry of each leveled list (a caller assumption: only play knows the game's
   pick); of the furthest run's 226 actors those without a plan fall from 122 to 8. Of the six sheet seconds,
   1345 s and 1470 s keep 40 markers each: outfit pieces, hair and insects' wings whose controllers no rest
   reading settles (effect shaders run by a manager, behaviour graphs, lag bones, moving transforms). Next:
   those controller families (the `effect-idle` family of `spec/coverage.md`), a level filter for the pick (the
   LVLO levels are read, not used), and which seed or entry the run's world model chose, if it ever chooses.
   Poses step (`spec/poses-census.txt`): the 17 characters of the town frames in the rest pose are creatures of six
   races without subgraph entries of flag word 0; `--actor-subgraph-flag-one` takes their flag word 1 entries and
   poses 11 (the 6 dogs stop at an unread `hkbLayerGenerator`). Of the markers, the part models naming an effects
   graph whose start state plays no sequence are drawn as stored behind `--actor-model-graph-idle` (town frames 40
   to 35 markers; 4 more are refused by the build for effect falloff on a skinned shape). Next: the layer
   generator's layout (the dogs), posed normals for skinned effect surfaces, a sequence player for effect fields
   (the 15 flying insects' start sequence), the lag-bone controller's field (9 mole rats).
2. Sitting and death (seen step, `spec/timeline.md`, Deaths and furniture): behind switches a dead actor is
   left out or drawn lying (a placeholder: the race's death clips are chosen on events the run does not raise,
   the ragdoll is physics), and an actor the run seats stands at its furniture's nearest marker (FURN SNAM)
   posed by its race's furniture entry. The run seats only the player, so the sit pose is exercised by
   synthetic tests only; next: a package-driven sit in the run, and a death clip if an event reading ever
   picks one. Faces step: with the picked looks the museum's dead raiders lie as characters (6 of 7 at 1650 s;
   the placeholder pose, half under the floor); the dead insects stay lying boxes (their wings' effect-shader
   controllers).
3. Everything after the start state: transitions on events (dialogue, alarm, walking), blends of several
   children, bone-weighted and additive layers, the offset clip; the clip's start second is the run's second 0.
4. A viewer window path: done for the scripted play window (looks step: `--play-run ... --play-input --timeline-cast`
   with the start clip flags; the cast of the drawn cell built again at the run's second when the timeline's actor,
   visibility, death or sit events change, the run's timeline read at most every 5 run seconds). The pose is the one
   of the build's second (`--actor-start-clip-loop` does not animate in the window); a moving actor jumps from build
   to build; the keyboard play mode and the free window take no cast.

Open checks: the viewer window has still not been exercised by hand with `--time` (nor with any clock switch;
the per-frame path is tested headless through `GpuScene::at_time`); the CPU-against-viewer tag differences are
explained (coplanar overlapping placements and edges, `spec/render.md`, Instance tags of the CPU and the viewer)
but not removed. The viewer's GPU tests now pass in parallel (`spec/render.md`, Viewer GPU tests in parallel). Seen while checking timeline frames: in the opening's exterior house cell with `--terrain --around 1`, the CPU draws what lies beyond a window where the viewer draws the clear colour (coverage agreement 0.983 with or without a timeline; an interior vault frame agrees at 1.000000); not investigated.

## Toward a playable game

### Milestones

The ladder the steps are picked from. A step is chosen to move the lowest milestone that is not done; a step
that does not move one needs a reason written in its notes. "Done" here means built and tested on `main`
under pending rules, never "matches the game": that needs the measurements listed at the end of this page.

| # | Milestone | State |
|---|---|---|
| M0 | Read the game's data: archives, plugins, models, textures, collision, scripts | done |
| M1 | Draw a cell and walk it with collision (CPU render and viewer) | done |
| M2 | Run the quests of the opening, the vault, the town and the museum headless | mostly: assumed fights, a few stages open |
| M3 | Play the opening in the window from input, start to the vault exit | in the window from a scripted input, no person needed (window step: `fo4-view --play-run ... --play-input FILE` with every `play_run` play option drives the run with `play_run`'s walker, held to the window's clock; the window loads the walker's cell at each change of space and writes frames with `--play-shots`): with the exit step's switches and input the window run gives the headless run's outputs byte for byte (274 of 274 stages to 960 s, 276 to 1,000 s, out of the vault at 955.35 s, stopped at 998 s at the foot of a cliff face of about 80 units, off the navigation mesh since a 58-unit drop at a rock's edge about 2 s before); looks step: behind switches, off by default, the window draws the run's actors as their characters posed by their start clip from the run's own timeline as it grows (`--timeline-cast` and the headless frame's cast readings; rebuilt on a thread when an actor's place or presence changes, 51 builds of 3.4 s on average in the opening), lights the loaded cells and draws the sky (the light switches with `--assume-hour 11 --assume-weather 0`); the window run's outputs stay the headless run's byte for byte (6 of 6); 60 fps median but about 16 fps in the prewar vault and 10 in the postwar vault (379 and 817 placed lights as points, every light for every pixel); moving step: the play options read by one structure in the play library for `play_run` and the window (`fo4_play::options::Play`; outputs byte for byte the same); `--play-collision-child-flag-agrees` in the play mode's cell builds and live world: the walker stands on the floor where the run seats it at 730.11 s (276 of 276 stages to 1,010 s, headless and in the window, byte for byte); the named platforms' parts drawn where their collision stands (`Walker::platforms`, the draws' node chains behind `BuildOptions::chains`), `--exposure-eye` measured once per cell the window loads (frame rate unchanged, but measured where the walker arrives it leaves the prewar vault washed out and whitens the people in the room the run seats the player in); drawn step: the views census (`spec/opening-views-census.txt`), the room the run seats the player in drawn (`--draw-child-flag-agrees`), the named platforms drawn as stored (`--draw-platforms`; rigid shapes and skinned shapes on their own nodes follow their parts, effect surfaces and refused shapes left out), the lights bounded by an exact grid (60 fps median in every stretch of the opening, the vaults included, frames byte for byte), the exposure measured per view with a floor (`--exposure-eye-floor`); hands step: a person's keyboard and mouse drive `play_run`'s walker in the window (`--play-keys`: the same walker, play options and run thread as `--play-input`, the commands taken as the walker needs them, `--play-stand` the longest standing slice; E's target and the pause in the title bar, Escape the input's end), recorded as they arrive (`--play-record`) and replayed by `play_run` byte for byte; a synthetic person injected at Bevy's input (`--play-keys-from`, test only) played the opening start to exit, 276 of 276 stages, replayed byte for byte, 60 fps median, 2.50 GB peak; `apps/fo4-view/presets/play-opening.args` with `--args` (the game's record ids in a local file); person step (what a tester's first two sessions through a desktop-automation tool met): looks from the window's cursor in event order when raw motion gives none (a tool's drag carries its motion on the release), the arrow keys, Shift as the movement key's modifier, commands made while the play clock stands kept and given when it runs; the first view's white found to be the exposure measured on the centroids in view, mostly the town behind the walls (`--exposure-eye-visible`, `frame_exposure_seen_visible_v1`: the first frame shows the room); a crosshair, the E prompt, the subtitle and paused or loading in the viewport (`--play-hud`, the viewer's own ASCII font); rebuilds: the walking world just left kept (0.02 s instead of 2 to 3 s for a square crossed back) and the cell's drawing loaded while the walker builds; arriving late: the session's run waited on the nursery's activation the person's level view could not make, not on a timed assumption, and the shaft's ride waits for the person (200 s late: the descent 15.6 s after arrival, into the vault); the front door's timed open left to the person's activation (`--play-activate-opens`, `play_activate_opens_v1`); way-out step (what the third session met): E took the gear door from everywhere on the walkway because its object bounds hold the eye there (`--play-pick-first-met`, `play_activate_ray_first_met_v1`: what the eye stands inside comes last, the named platforms' boxes and boxes of bases with no model left out), the exit console and the device before it are the person's activations (`--play-activate-person`, `play_activate_person_v1`), and the walkway ended at a door left shut because the caller's two player animation events after the console, held until a stage, come at one second (the interpreter's gate keeps no offset: a `crates/fo4-vm` change); with those two given the late seconds by hand the synthetic person 200 s late goes from the pods up the exit elevator and out (276 stages, the headless run's order, follow-on quest 3 to 15); the 76 s stand after the ride down is the interpreter's load settling at the change of area (61 s headless: placement cells searched per reference, `fo4_vm::load::settle`, not cut); the viewport says loading while the play clock stands, the title shows the play clock, the prompt is an action and a name, subtitles wrap to three lines with no explanation text, the objective in a corner, a confirmation after E; the dark dream room is its own data under the drawing's readings (162 of 211 lights off at load); gate step: a caller action held until a stage keeps its offset from it (`quest_run --action-after-offset`, `caller_action_after_keeps_offset_v1`: the reference second is the written second of the caller's action that set the stage, else the earliest held action's), and the person's opens and activations stand for the timed ones they replace as written (`--play-action-after-offset`, `play_activate_written_second_v1`, in the preset): with the original arguments, no hand-moved second, the person 200 s and 400 s late gets the scripted run's 276 stages and rides the exit elevator up (the exit's two events 10 and 15 s after the person's use of the console; 260 stages and the gear door shut without the switch); the load settle looks a reference's cell up in the run's placement index (`track::Cells`; the vault's arrival 61.7 s to 0.10 s headless, 69.4 s to 0.10 s in the window; outputs byte for byte); the window's cell loads keep the game data open on one thread (about 1.7 s of each 5.6 to 8.1 s load); deck step: both person sessions of the gate build stopped on the raised platform because the caller's two moves of the spouse (written at 420 and 466 s) came before the stage whose fragment moves her to the escort marker, and the platform's descent needs all seven registered arrivals; with `quest_run --action-after-actor-moves` (`caller_actor_move_after_v1`, off by default) and the two moves gated on that stage in the run's arguments, both records reach the descent and the vault to the end of their input, the late runs keep the reference's 276 stages at the same seconds; a sweep of synthetic persons early, late and arriving early at every stop of the opening (`spec/play.md`, The platform waits for every arrival) finds the late ones through but one ride end, and lists the early stalls the rule does not cover (the checkpoint's escort scene when the person is at the meeting point before it starts or boards the platform before it ends, a trigger crossed before its stage after waking, the vault floor fallen through when the person is ahead of the story); the viewport shows a displayed objective whose text is empty by its quest's name; early step: of the deck sweep's early stalls, the checkpoint's hello entered before its line held is offered again after the quest state changes (`quest_run --hello-owed`, `dialogue_hello_owed_v1`), a stopped escort scene's scheduled package moves stand (`--package-moves-outlive-scene`, `pack_destination_outlives_scene_v1`), and a ride that ends with the walker in the air ends when it lands (`--play-elevator-door-landing`, `play_elevator_end_landing_v1`, in the preset): the sweep's late persons 25 of 25 through, arriving early 14 of 20 (before 24 and 12), the three human records replay as before; the walk through the checkpoint gate before the guard's line is the walking world's missing collision markers' boxes (`--play-marker-boxes`, `play_collision_marker_boxes_v1`, built, not in the preset: the front door's box stops every recorded input short of the door); the trigger entered before its prerequisite (the default trigger script) and the walk during the dream are the game's own waits or the sweep's; the fall in the prewar vault is a placed door whose collision is refused with nothing built behind it; the window keeps the scene assets of one cell load for the next (`load::KeptAssets`); board step: the third person session of the early build stopped on the platform with 495 and no 490 or 500 because its person walked out of the house's closed front door 33 s before the evacuation stage opens it, crossed the trigger of the street's stage before its prerequisite and never started the second quest's chain (the platform trigger's 490 needs its first stage, and five of the seven arrivals are the crowd that chain walks there), then also walked through the checkpoint gate's box 6.3 s early; a door the run's scripts close with its activation blocked now holds the walker (`--play-held-doors`, `play_door_closed_blocked_holds_v1`, in the preset): the same person waiting at the door and the gate reaches 500 and the vault, the record itself (made through the door) cannot replay past the door, the normal-pace records replay as before without the switch and the second one byte for byte with it (`spec/play.md`, A person out of a closed door); gate-box step: the front door's marker box that held the walker 107 units from the door is built as the data places it (in the room, one 17-degree tilt about the door's width axis, half extents; standing at the visit) but stores collision layer word 40 where the gate's box stores 35, and the vault exit's box 49: a box whose `XTRI` word is a layer a walking character passes (39 to 44, 49) stays out (`--play-marker-box-layers`, `play_collision_marker_box_layer_passed_v1`), and the marker boxes are in the preset (the on-time synthetic person: 276 stages in the same order, the checkpoint's seconds within 4.4 ms, held 6.7 s against the gate's box; the three person records replay as before with the switch off; the sweep loses nobody; a person sliding along the gate's box still goes round its end, where the gate frame's collision is refused: `spec/play.md`, The front door's box and its layer); gate-frame step: the gate frame's collision was refused not by its animation (its one transform controller turns a leaf node with no collision under it; the collision hangs on the model's root, so the off-chain reading already set the controller aside) but by four unread fields of its reference, `XACT`, `XLRT`, `XOWN` and `ONAM`; on a static's reference they no longer refuse its collision (`--play-collision-static-fields`, `refr_static_fields_collision_unchanged_v1`, in the preset; 36 references of statics in the load order admitted, 4,735 more store the fields with no collision): the three person records replay as before with the switch off, the on-time synthetic person keeps its 276 stages in the same order (checkpoint seconds within 48.7 ms), the gate case is held at the box and the frame until the stage that disables the box (422.41 s, the gate-box step's diagnostic byte for byte; no 500, the record's later commands were made by a person already through), the sweep loses nobody (37 of 40 the same stages in the same order; the three persons early at the gate are now held there until the stage that opens it, 196 stages as before); no rule places a collision chain that an animation moves (59 models with placements, 108 placements of statics); the passed marker box layers taken from the load order's collision layer records (`fo4_plugin::coll`, `--play-marker-box-layer-records`, `play_collision_marker_box_layer_records_v1`, not in the preset) differ from the constant: the character's layer (30 by prior knowledge) lists 39, 40, 41 and 43, so the front door's box would stand and the on-time person stops at the door (144 stages, as with every box standing) (`spec/play.md`, The checkpoint gate's frame and the layer records); door-shut step (what the seventh session met): the house front door placed no box before the visit ended because no script closes it before then; a door the person opens whose reference stores no `ONAM` counts as closed from load (`--play-held-doors-at-load`, `play_door_closed_at_load_v1`, in the preset with `--draw-held-doors`), so the front door stands from 3.0 s, the stage that blocks its activation (read for every door with no `ONAM`, the pods' and two vault doors are held too and the on-time person stops after waking, 252 of 276 stages); the on-time person and the first session's record keep their tracks and timelines byte for byte, the early session's record is held at the door at 10.43 s; the window draws a door the reading has held, shut while its box stands and posed at its model's sequence Open after (`--draw-held-doors`, `play_door_drawn_as_held_v1`); the checkpoint barrier is a collision marker's box (nothing to draw) and the gate frame static, refused at render readiness by its four reference fields and its leaf's controller (not drawn yet); the catwalk holds are the exit mechanism's walkway parts moved by the sequence the run starts at the console while their skinned grating is drawn still (`--play-report-walls`; `play_platform_part_holds_walker_v1`, behaviour unchanged) (`spec/play.md`, The front door shut from load, the checkpoint barrier and the catwalk holds); draw-gate step: the checkpoint gate's frame drawn: its reference's four fields set aside for render readiness (`--draw-static-fields`, `refr_static_fields_render_unchanged_v1`; alone it newly draws no other placement of the opening's eleven views) and its model's one clamped controller read as a leaf with two poses, stored and at its farthest key (`--draw-static-leaves`, `play_static_leaf_two_pose_v1`); in the model neither pose lies across the gap the invisible box holds, so the window draws the leaf standing out from the fence while the box stands and folded along it after (378.07 s for the on-time person), and the box itself stays invisible; the named platforms' skinned shapes whose bones go with several parts skinned again with each bone moved by its part (`--draw-platform-skins`, `play_platform_skin_follows_parts_v1`): the exterior elevator's deck, the prewar vault's walkway and the exit's walkway now drawn where their collision moves; with the three switches the window holds 60 fps median and the on-time person's and the first session's tracks and timelines stay byte for byte; the deck step's sweep of 40 synthetic persons sets the same stages at the same seconds, so the three switches are in the play preset (`spec/play.md`, The checkpoint gate drawn and the walkway's grating moved); corridor-fall step: the eighth session's fall after waking was a vault doorway with no collision between the room's floor and the hall's, 26 units apart, where the capsule sank between the edges; a door whose model carries its own frame now stands in its open pose, the frame as stored and the parts its sequence named Open moves at its last key (`--play-door-open-collision`, `play_door_collision_open_pose_v1`, in the preset): the doorway's floor is whole, the jamb stops a sideways walk, the record keeps every stage at its second and stays on the floor; the first record reaches the surface unchanged in its stages, the third's open-loop keys drift into a wall after a doorway crossed level (steered back to its own track it reaches the surface, every stage within 0.024 s); the front door's prompt reads close when the door is drawn open, and E before the doorbell is a named question (`play_door_activation_opens_held_before_visit_v1`) (`spec/play.md`, The post-wake corridor's fall and the front door's prompt); ninth-session step (two person sessions to the surface, nobody fell): the vault doorways' stops before E were the person's short presses (the walls report shows none), but the doors whose collision stood open were drawn nowhere and prompted open: with `--play-door-toggle` (`play_door_activation_toggles_v1`, in the preset) they are drawn open, prompt close, and E closes and opens them, the front door after the visit too; the invisible jambs and the floor floating against the clear colour are a hall-end static refused for its `XALP` alone, now drawn (`--draw-reference-alpha`, `refr_xalp_render_unchanged_v1`, in the preset); the intake arch's stops are a desk below the view; the checkpoint strip's stop is the data's marker box; the catwalk's second stop is the ramp part raised 48 units until the last second of its sequence, drawn where it stands; the street's 12.7 s stall is 3.95 s of waiting for a cast build on the play data thread, 8.58 s of cell load and 0.13 s of upload (`spec/play.md`, The vault doorways' drawing, collision and prompt); tenth-session step (two person sessions to the surface; the first step into each outdoor square stood 2.3 to 6.5 s, one 15.2 s): a cast build on the play data thread gives way to a cell load between its steps (the street's 15.2 s held 5.1 s of it), the play data thread reads the window's first cell again once the window draws so the first change of square starts from its assets (the street's square 5.3 s headless before, 2.1 s after), and the named platforms' cells are found once (0.6 s a load): headless with the sessions' arguments the six outdoor loads take 1.2 to 2.1 s after a cast's 0.1 to 1.0 s against 1.8 to 5.3 s after a cast's 1.5 to 2.3 s, the drawing and both records' replays unchanged (`spec/play.md`, The outdoor stalls); the lower floor both persons met beside the first lift's landing is the data's (the room model's collision: a 22.6-unit face between the landing strip and the floor the stairs rise from, above the walker's 20-unit step), its collision as read, but the room is not drawn at all (refused at render readiness: controllers, particles, a filtered material swap, blend factors), so the floors and the stairs there are dark (`spec/play.md`, The lift's landing and its lower floor); a door the person shut holds the walker only: the run gets the activation alone and moves its actors by scheduled moves that read no door, so they are drawn past it and the story completes (a synthetic test; `play_door_person_closed_actors_unchanged_v1`, pending, `spec/play.md`, Actors and a door the person shut); eleventh-session step (two more person sessions to the surface): the first lift's room is drawn by its unmoved parts behind `--draw-static-parts` (in the preset; `static_unmoved_parts_drawn_v1`, pending: 21 of its 26 shapes, the particle system, two shapes with undecoded extra data and three material-blended shapes left out and counted; its swap, blend factors and texture slot were already under pending rules, the graph and the particles refused it), 16 other placements of the route drawn by the same switch (`spec/play.md`, The first lift's room drawn by its unmoved parts); the people no longer vanish at a change of place: the new cell's cast is built on the play data thread right after the cell (from its kept assets), the old cell and, within one worldspace, its cast stay drawn until the new ones are, and an interior is revealed with its cast when it comes within 1.5 s (headless the cast now follows the geometry by 0.08 to 1.3 s against 1.0 to 2.5 s; `spec/play.md`, The cast through a change of place); the play data thread keeps the files it reads (512 MiB at most), so a square's draw list takes 0.1 to 0.9 s headless against 0.9 to 1.5 s, the old view stays until the new one is spawned (the bare sky of B's last approach was 4 frames of that swap) and play shots are encoded off the frame loop (the startup hitch was the first shot); the bridge's slower stall was the whole process running 2.2 to 2.7 times slower then, the run's own walker build too (`spec/play.md`, Pauses, the bare sky and the startup hitch); twelfth-session step (two more person sessions to the surface): the lower lift room's white floor was the window's exposure measured on the load's faces, where a named platform's skinned walkway stood at its built pose in front of the eye (0.47 measured, 1.29 on what is drawn): the window now measures on what it draws, hidden draws left out, moved ones moved, skinned ones as last skinned (`scene::DrawnNow`; no rule, headless frames unchanged); the dark recesses are drawn surfaces little light reaches under the light readings (no refused draw); the deck's crowd is five actors one package sends to one marker at the closed covers' height, 46 units under the raised deck, and the run moves nobody during the descent: `--draw-cast-on-platforms` (`play_cast_stands_on_platform_v1`, in the preset) stands the cast on the platform part under it and rides it down (where five sent to one marker stand apart is not in the data); the helmets' wearers lost their heads to the bone-weight covering clause: `--actor-head-slots` (`npc_head_hidden_by_head_slot_v1`, in the preset) lets the armour's BOD2 slots decide (the helmet holds hair slots, so faces stay); the actor gone at the crane was the run's assumed move at stage 475, not the drawing; the registry's statuses read once (a quarter of the dream's load: dream 2.77 to 2.29 s, dirt turn 1.34 to 1.26 s headless; the rest is inflating files no load kept); `FO4_VIEW_PLAY_WATCH=FILE` logs the cast's places and the held doors near the walker for the shut-door question (`spec/play.md`, the twelfth-session sections); thirteenth-session step (two more person sessions to the surface): the deck's crowd went down 33 units and stayed on the shaft's closed covers, the highest part top within a character's height: `--draw-cast-keeps-its-part` (`play_cast_keeps_its_part_v1`, in the preset) lets an actor ride the part it stands on, and in the window replay of session B the six ride the deck down the shaft 4 units above the walker; the five at one point are the data's (one package, one marker with a radius word of 150: recorded open, `pack_location_radius_one_point_open_v1`, nothing changed); the security staff drawn as markers were a helmet visor whose mask texture is in no archive, in slots the frame never samples: `--actor-unread-texture-slots` (`actor_texture_missing_unread_slot_v1`, in the preset) reads the actor's models as a placed reference's, and they draw with helmet and face; hair and skirts reaching to the floor were bones the skeleton lacks skinned by the identity while stored vertices are not in the actor's space: `--actor-missing-bone-nearest` (`actor_missing_bone_follows_nearest_v1`, in the preset) skins them as the nearest held bone of their skin; `--play-from SECONDS` runs a recorded input unpaced to a second before the window's pace (second 712 of session B's record in about 70 s with the files cached, 4 minutes on the first run); play shots are JPEG at quality 80 and the watch log samples the nearest actors every second (`spec/play.md`, The crowd's ride, the security staff, the hair and the late start); session A's stalls were every step slower while the disk was busy, not one step (report only); fourteenth-session step (neither person left the vault; the run's answers were the thirteenth build's, byte for byte on session B's record): session A used the exit console before taking the device and got the data's refusal; session B took the device through the activator it lies on, whose scripted activation with events the run read as a sit only, so the furniture's OnActivate (and the stage the console needs) never came and the player stayed seated, which the console's script refuses: `quest_run --activate-furniture-events-deliver` (`vm_furniture_activate_events_delivered_v1`) and `--sit-end-walk-events 64` (`vm_sit_events_activation_ended_by_track_v1`), both in the run's arguments, take session B's record through the gear with every earlier stage at its second; the nursery's and the exit's targets were never drawn (skinned, animated, stored ragdoll): the prompt now says "(not drawn)", the watch log writes it, and the confirmation after E no longer says "nothing happened" when the run acted, and the nursery's clutter refused for its scripts and its activator fields is drawn (`--draw-scripted-as-stored`, `refr_script_render_as_stored_v1`, `--draw-activator-fields`, `acti_fields_render_unchanged_v1`, both in the preset); each load step's archive read and inflate time is split out (`spec/play.md`, The exit's device, the hidden targets and the lab coat); fifteenth-session step: both persons reached the surface; the outdoor holds grew (11.9 and 8.5 s over six crossings) because the kept files (512 MiB) no longer held the first square and the casts between loads once two drawing switches drew more: a new asset's file is now kept from its visit, a kept file is refreshed on use and up to 1 GiB is kept (session A's record replayed in the window: 7.8 s to 5.5 s, the street 2.3 to 1.1 s); a console press refused because the run still had the player seated in the device's furniture (63.8 units from its place, under the 64 of the walk-away reading) now ends that sit first (`quest_run --sit-end-other-activation`, `vm_sit_events_activation_ended_by_other_activation_v1`, in the run's arguments), the confirmation after E says whether a stage or scene changed and names what the scripts activated, the device's prompt names the activator's linked furniture, and the gear door's hub pieces leave the doorway with the gear (`--draw-platform-nearest-part`, `play_platform_shape_nearest_part_v1`, in the preset) (`spec/play.md`, The outdoor holds, the console's refusal, the gear's hub and the nursery's props); what M3 still lacks: the nursery's and the exit's targets drawn (a skinned model in a static placement, an animated furniture, stored ragdoll data), the robot in the kitchen (a look from an object template), the device worn by the escort (a transform controller on a worn model), the leveled actors behind the postwar console (a pick), the lab coat's head (no head left out is reported; not the nearest-bone reading), the walker's slide against the shaft's bottom in the ride's last second (no wall stops it: pushed out of a surface it descends into); which layer reading holds at the front door (the two pending rules' measurement), the checkpoint's box drawn (it has no model; the frame's leaf never lies across the gap in the model), a door's swing, a person at the keyboard (a physical-device pass), the earlier vaults' timed caller activations, the window's own load of a grid square's drawing (models parsed and meshes built again for each draw list; their files are kept), the talks timed by the caller, the early-person stalls above (a walker held while the run seats the player), the one assumption on the exit elevator's descent; what the game's actors do at a door the player shut; the exit elevator's car dark; whether the first lift's room shows as its unmoved parts draw it (the rule's measurement); the prewar hall bright toward white in places; actors one package sends to one marker drawn at one point (open until measured); the other cast markers of the window (an effect falloff on skinned shapes, actors with no model, a transform controller); a grid square's models parsed and built again at each crossing and nothing of the next square read ahead; the nursery's changing table and other containers not drawn (fields not interpreted; the changing table also skinned and animated), so the linen and rattle on it show on nothing; the gear's hub pieces' own motion under the sequence; doors (every placed door refused at render readiness for its door record's FNAM layout; drawn only behind `--draw-held-doors`, and only those the held doors' reading has held) Sixteenth-session step (`spec/play.md`): the second session's six-minute stall in the vault came from a stage trigger entered 0.019 s before the stage it asks for, 3 s late because a dialogue action with no line that holds lasted the caller's line seconds; `scene_dialogue_no_line_holds_ends_v1` (`--no-line-holds-ends`, pending) ends such an action at once, and the record replays past the stall (14 s from the intake stage to the pod's stages instead of 364 s; the first record unchanged to the surface); the window names a place the walker passed whose trigger would now act (`Go back: N units`), says opened or closed for held doors and quotes the line a press started, prints each load's wall and CPU time on the data thread, and draws skinned placements on their own nodes in the play cast. Seventeenth-session step (`spec/play.md`, the seventeenth-session section): the deck crowd lost at the first lift's handoff in session B was the play cast standing on a named platform of another cell (the vault exit lift, whose deck stood at its top under the same interior coordinates before 800 s): the cast now stands only on platforms of its own cell's space, and B's crowd rides the deck to the bottom with the player; the house's pocket doors and the ruined front doorway store ONAM and have no frame, so nothing drew or held them while the prompt said open: `--play-door-stored-open` (`play_door_stored_open_v1`, in the preset) makes them open from load (drawn open, prompt close, E closes), and a door the reading never held carries "(not drawn)"; a door whose activation the run last blocked is left as it stands by E (`--play-door-toggle-blocked`, `play_door_toggle_blocked_unchanged_v1`, in the preset), but at the evacuation the scripts unblock the front door (BlockActivation(false, false) with the open), so E still closes it there, as the toggle rule reads; the nursery's crib is a persistent activator no load read: `--draw-world-persistent` (`world_persistent_refs_drawn_in_square_v1`) joins a worldspace's persistent references in the loaded squares and `--draw-activator-parts` (`acti_unmoved_parts_drawn_v1`) draws the animated crib by its unmoved parts, so the infant lies in it; the house's furniture refused for five FURN fields is drawn (`--draw-furniture-fields`, `furn_fields_render_unchanged_v1`); a head part with conditions is evaluated against the run's stages done (`--actor-head-conditions`, `npc_head_part_conditions_run_stages_v1`); the checkpoint's standing skeleton and the remains hanging under the exit's grating are ragdoll models drawn in their stored pose, their references storing XRGD (not read): left out (`--skinned-models-ragdoll-left-out`, `refr_ragdoll_pose_unread_left_out_v1`); the go-back cue names only places within 2,048 units, not one the feet stand in nor one whose hold changed without an event, measured to the volume's nearest point, and not while the window loads; a press whose target's scripts name what they activated is confirmed at once. All in the preset; the seventeenth sessions' records press E at pocket doors that did nothing then and now close them, so they replay their own tracks only without `--play-door-stored-open`. Next: the memo of per-model draw recipes (a second build of a square decodes nothing); the data thread's scheduling (measured, not changed); the tuft over the house frontage (not identified); the exit's device and remains (ragdoll data not read). |
| M4 | People: animated poses, movement between places, faces | start poses and faces drawn, in the headless frames and (looks step) in the scripted play window at the run's places; no clip playback over time, no movement between the cast's rebuilds |
| M5 | Fights: weapons, damage, death, enemy behaviour | not started (deaths are the caller's assumptions) |
| M6 | Screens: dialogue choices, inventory, the wrist device, containers, map | first reading (menus step, `crates/fo4-ui`, `spec/ui.md`): every interface movie read and counted, the dialogue and heads-up screens' host interface read statically, what a host must give the dialogue screen written down under pending rules, a static first frame drawn; no movie runs, no screen is shown in the window (subtitles in the title bar and the play HUD only) |
| M7 | Sound: voice, effects, music | not started (lines are timed by their files, nothing plays) |
| M8 | Saving and loading | not started |
| M9 | Packages and schedules: people living in the world away from quests | not started |
| M10 | The open world streamed at speed with distant terrain | streaming walks; frame rate and far terrain open |

Order of work from here: M3 (a person at a physical keyboard and mouse plays the window, `--play-keys`, after the person step's fixes; fix what they meet), then M4 movement, M6 dialogue choices
and M5, because those four are what stands between the headless run and a person playing the first hour.

Pad step: a GIP pad plays with the keys (`fo4-view --play-keys --play-pad`, a viewer built with `--features pad`;
spec/play.md, The pad): the same commands the keys give, so records and replays are unchanged. Its key map is viewer
policy, listed with the preset's pending rules in `docs/unmeasured.md`. Checked with a scripted pad (the synthetic
person's `pad` lines): a short window session's record replays by `play_run` with every output the same but
`play.txt`'s timing lines. Not yet run with the device: the owner's session, then `--frame-log` holds with the pad
on and off; the analog speed (a `move` command) is a later, optional step with its own pending rule.

Eighteenth-session fixes: the play preset draws the cast with `--actor-template-bits` and `--actor-leveled-seed 1`
(owner's order; pending, on `docs/unmeasured.md`), and keeps an actor drawn at the walker clear of it
(`play_cast_kept_clear_of_walker_v1`); a held door's E confirms `held`; the `Go back` cue follows the walker's
own trail; each play load prints its parses by kind and its grid squares. Open: the robot at the ruined house
(its look is an object template, `OBTE`/`OBTS` with object modifications, which nothing reads yet: a layout from a
public source first), the device pickup over the grate (no cue drawn for a reference left out), the draw-recipe
memo (plan 26c item 1, after the parse counts), the refusals census and the container rules, the push-out report
and the dream room's words. The bridge wash is the data's: the walker steps off a rock's edge beside the bridge
(70 to 80 units) into a wash whose banks are near-vertical steps; nothing is missing from the walking world there.

Nineteenth-session fixes: the keep-clear switch reads each actor's drawn body at the eye's height and eases the
move round the eye (`play_cast_kept_clear_of_walker_v1`, text updated); the `Go back` cue cuts the walker's loops
out of the way back; `--actor-effect-stored` (`actor_effect_controller_stored_v1`, pending, in the preset) draws the
eight glowing creatures at the neighbourhood objective that a manager-run effect-shader float controller refused.
Measured: the nineteenth's longer holds were not this round's cast switches nor the parse counters (the same
record replayed with and without the switches gives the same load walls, 3.17 s at the vault exit, as the
eighteenth's 3.16 s); the sessions' loads ran uniformly slower. Open: the fog edge on the new road (a full-height
boundary at one standing place, reproduced in a replay; no fog or volume placement near it), the steps of the west
house, actors give no E prompt (they stand in no pick world), the draw-recipe memo (plan 26c item 1), the robot's
object template, the device pickup cue, the refusals census, the push-out report, the dream room's words.

Twentieth-session fixes: `--actor-head-race-fill` (`npc_head_parts_race_fill_kinds_v1`, pending, in the preset)
gives an actor that lists its own head parts its race's parts of the types its list lacks, never the severed-head
type 7 (a capped child of the checkpoint listed only a mouth and a hair and drew a mouth under its cap); the cast log's
`faces:` line counts actors with a head, those with no face part and those of them wearing headgear (0 of 41 at the
checkpoint after the fix). The house cue points straight at the place, or at the way back's latest point in sight
at eye height in the walking world (`Walker::clear_line`), so it no longer says `behind` while the hall is open. A
`Use NAME (not drawn): N units SIDE, H below the eye` cue names the nearest unmet target of the person's E after 30
run seconds without a quest change. Found, not fixed: the white edge on the new road is a distant-cloud card (an
effect surface with Soft_Effect and Soft Falloff Depth 2048, falloff and vertex alpha) whose plane passes 15 units
from the eye; the soft depth fade is left out (`effect_soft_lighting_left_out_v1`) because the window's blended pass
has no scene depth (a depth prepass would need our own alpha-tested prepass fragment, since alpha-tested surfaces
must not write their whole quads). The floating woman at the checkpoint stands at her stored placement, 19 units
above the terrain on a 27-degree slope (the game's settling of a character onto the ground is not modelled). The
overlapping passengers on the descent deck are five actors the run gives one and the same place. The west house's
rear threshold is a 35-unit rise from the terrain to the foundation with no step collision, above the walk's
20-unit placeholder (`walk_step_height_v1`). Open: the soft depth fade, the draw-recipe memo (plan 26c item 1),
actors as pick targets, the robot's object template, the refusals census, the push-out report, the dream room.

Twenty-first-session fixes: the use cue retires a target E met, one a script activated (taking the device's holder
activates the device) and one the run made absent, so it no longer points back at the taken device. The house cue's
open line is the one the walker's body walks (`fo4_walk::Character::walks_to`, a copy of the character walked
straight at the point), not a sight line at eye height; the way back passes over a stretch in another space left and
entered at one door (`CUE_DOOR_JOIN`); with no way back a straight line the body does not walk says `(not straight: go
round)`. The session's two cues through shelving stay: our walking world has no collision for that shelving (the
record's walker stands inside it), so the body walks the line too. `--draw-cast-settles`
(`play_cast_settles_on_ground_v1`, pending, in the preset) stands an actor at a stored or package place on the walking
world's ground within 32 units above or 64 below (the checkpoint's woman 19 units above the slope now stands on it;
the window log's `settled:` census). `--draw-furniture-parts` (`furn_unmoved_parts_drawn_v1`, pending, off and not in the
preset) reads a furniture refused for its animation as a static by its unmoved parts; the exit's device stays refused
with it (to find: which of its shapes the controllers reach). No game setting of the data names a step
or climb height (`walk_step_height_v1` stays 20, pending). The pale band at the throat of the checkpoint's woman in the
green dress is drawn alike without `--actor-head-race-fill` (not from the fill; cause open). The platform passengers'
one place is the package location's one point (`pack_location_radius_one_point_open_v1`, left as is). Open: the soft
depth fade of effect surfaces, actors as pick targets, the draw-recipe memo, the shelving's collision, the robot's
object template, the refusals census, the dream room.

Twenty-second-session fixes (viewer policy, the run unchanged): the cues wait for quest progress, and progress now
counts only a stage set the first time, a quest started or stopped and an objective whose value changed (the 22nd
session's run logged stages set again and refusals from early on, so no use cue came in the vault); the window log
says each stall and why a use cue names a target or none. The house cue's fallback aim (nothing on the way walked
straight) stops at the end of the way's first straight stretch (`cue_straight_stretch`, the trail within 16 units of
the line), not 96 units along past the trail's turn at a door. With `--draw-cast-settles` an actor whose ground is not
answered is not drawn for up to 1.5 s, and the census names the actor dropped most. The person's window gets the named
platforms at each tick (`fo4_play::walker::Arrivals::platforms`), so riders and parts move with the feet instead of
the last slice's end (up to 60 units apart on the ride down). Left: the ground is still asked at the actor's origin
only (heels on slopes; per-foot rays from the posed feet would need a rule); a resting creature sunk in a slab was not
moved by the settle (its pose draws below its place); the soft fade, the parse memo, the device's refusal, the
shelving's collision, actors as pick targets and the throat band were not started (janela17 RESULTADO).

Twenty-third-session fixes: the cues' wait for quest progress is measured on the input's clock (the end of the last
slice handed to the run, `cue_clock`), not only on the run's second, which is that of its last event and stood still
after the console from 2225.8 to 2290.6 s (no stall was ever seen, so no use cue came). The walk probe the cues ask
(`fo4_walk::Character::walks_to`) moves its copy at most half a radius a tick, so it climbs a flight of steps the
walker climbs (at 0.1 s and the run speed a raised tick crossed two risers). The 22nd A house cue at 475 s is not that
case: its first trail point lies on top of a 25-unit sidewalk ledge the walker had dropped down, above the
provisional step height of 20, so the probe rightly refuses it and the cue still says `ahead` for one second there. `--draw-soft-effects`
(`effect_soft_depth_fade_linear_v1`, pending, in the play preset) fades a blended soft effect surface by its gap to
the opaque depth behind it over its stored soft falloff depth, on the CPU from its opaque depth and in the window from
a copy of the depth buffer taken after Bevy's opaque pass (`gpu::SoftDepthBuffer`; a view above 4096 x 2880 draws
without the fade). The window's loads keep up to 256 MiB of parsed models (`fo4_scene::Assets::keep_models`,
`FO4_VIEW_MODEL_MEMO_MIB`; a parse is shared through `fo4_nif::NifBody`): on the 21st A record's three loads from 437 to
553 s the walls were 0.65, 0.87 and 0.77 s with it and 0.98, 1.11 and 1.14 s without (17 models parsed at the bridge
load in place of 2,091). The soft fade showed no frame-time cost under the window's 60 Hz cap. Found, not fixed: the exit device is still
refused with `--draw-furniture-parts` because its model's manager-run transform controller names the hand node above
every shape and a manager-run visibility controller (with an undecoded blend bool interpolator) names the device's
node, so the parts reading keeps no shape, and the whole model also waits on a behaviour graph and two furniture
fields the furniture reading does not set aside (activate text, MODT); drawing it needs a rest rule for manager-run
visibility at least. The living-room shelving has its collision (shelf boards of the house's interior model in the
walking world); the 21st B walker stood about 84 units in front of it with no key held, not inside it. Left: actors as
pick targets, the throat band, the floating television, the creature in the slab, the robot's object template.

Twenty-fourth-session fixes: the fine vertical streaks a person met on the opening room's lit walls were in every
window since at least janela9 (the first frame of every session from then on is the same image), not new in round 23.
The window's face normal came from screen derivatives of the f32 world position, which some 90,000 units from the
origin steps by 1/128 unit while a close wall's pixel spans a few hundredths; on a wall along no world axis the
rounding is the same down a screen column. The normal now comes from the pixel's unprojected camera-relative position
(`surface.wesl`, `view_position`); a GPU test draws a turned wall that far out from 25 units and holds it to the CPU's
frame, and a plain test keeps world-position derivatives out of the shader. The soft card's tall edge against the sky
was the window's near plane at 15 units cutting a sheet whose plane passes 14.9 units from the eye (its own border
fades by vertex alpha): `--view-near UNITS` (`window_near_clip_assumed_v1`, pending, 5 in the play preset) sets the
window camera's near plane; the CPU reference keeps 15. `--play-pick-actors` (`play_activate_actor_box_v1`, pending,
off by default and not in the preset) lets the activation's ray meet the run's actors as upright 40 by 120 unit boxes
where the run last put them. Left: the exit device's four pieces (visibility rest state, the 7-byte blend bool
interpolator, the graph's start state, the activate-text field), the floating television, heels on slopes, the throat
band, the pale ground sheet, the robot's object template. Measured, not fixed: the east living room's creature is
stored 11.5 units above the terrain, the only walking surface under it (the house's collision there lies lower); why it
looks sunk into the drawn slab is not settled.

Twenty-fifth-session fixes: the black viewport at the ride's cut into the lower shaft was the window's: once the
loaded cell's scene spawned it replaced the drawn one at once, but the walker's new feet reach the window only when the
run goes on, so for about ten frames the camera stood at the upper shaft's eye with the upper shaft already gone. The
cell before a load of another space now stays drawn (its entities passed by the per-frame work) until the walker stands
in the new cell, or two wall seconds (viewer policy, `gpu::leaving_done`). The exit door's green and orange oval was a
heat-shimmer shape of the door's own model: a lighting shader setting Shader Flags 1 bit 15 (nifxml: Refraction), its
texture set's first slot a tangent-space normal map drawn as a colour; `--draw-refraction-left-out`
(`lighting_refraction_surface_left_out_v1`, pending, in the preset) leaves such shapes out. The two lace shapes below
the front window were the doilies lying on the stereo cabinet, refused for its unread receiver field (RADR), its
reference's location type (XLRT) and its glass, an untextured effect with an environment map:
`--draw-activator-receiver` (`acti_receiver_location_render_unchanged_v1`, pending) and
`--draw-effect-environment-left-out` (the existing `effect_environment_left_out_v1` and
`effect_environment_adds_to_colour_v1`, now for untextured effects too), both in the preset, draw it. The cue says
`ahead` only within 15 degrees of the heading, `ahead, bear left` or `bear right` to 45, and names a near aim apart from
the way's length (`Go back: 503 units; first 47 ahead, bear right`). Measured, not changed: the two movement stops on the
way to the second bridge are collision the data stores (a 58-unit coping on a stone retaining wall beside a walkable
top, met by the 16-unit capsule 15 units off the line walked; a concrete park table's 31 to 47-unit top and bench).
Measurement aids: `FO4_VIEW_HOLD_SHOTS=SECONDS` writes frames while the scripted window's scene is not ready and for a
second after each load, the frame log has the camera's height, `FO4_VIEW_BLENDED_ALL` and `FO4_VIEW_BLENDED_MIN` widen
the draw dump. Left: the exit device's four pieces, the television, heels on slopes, actors as pick targets in a
window replay, the throat band, the pale ground sheet, the robot's template (janela20 RESULTADO).

Twenty-sixth-session fixes: the rock at the end of the ride down had two sources. In the ride's last 1.7 s the terrain
over the shaft touched the riding walker's capsule and pushed it 177 units sideways (a temporary trace of the 25th
session's second record; no key held); `ride_check` had named the reading long ago and the play mode had not read it:
`--play-elevator-through-terrain` (`walk_ride_rider_passes_terrain_v1`, in the preset) now hides the terrain from a
rider, the record's stages unchanged. But the rock filling the view from about 6 s before the cut is the drawn terrain
itself: the data's terrain stands inside the shaft (under the deck's centre at about 5250, the ride ending about 425
lower, and 50 units east of the centre already rising steeply), and the eye goes down through it; nothing read holes
it (XCLC quad bits clear). Not fixed: the drawing needs a reading
of its own (the terrain within the ride's horizontal box left out while the walker rides, a pending rule). The
living room's stereo cabinet could be walked through: its model stores collision, refused at collision readiness by
the activator's unread PNAM, FNAM and RADR and the reference's XLRT; `--play-collision-activator-fields`
(`acti_fields_receiver_collision_unchanged_v1`, in the preset) sets them aside for collision as the drawing already
did. The 300-unit fall beyond the second bridge is the data's: the walker left the top of the large bridge structure
at its edge, the terrain 300 below (column probe). Left: the undrawn pods and a cue to the one the run waits on, the
road walk beyond the second bridge, the holds, the nursery prompt, the ten creatures, the exit device, the
television, heels, the throat band, the robot, the actor prompt's name (janela21 RESULTADO).

Twenty-seventh-session fixes: the rock drawn inside the shaft is the terrain's own cone (the cell's stored heights
lower it inside the entrance's opening to about 5,200 under the deck, which rides to about 4,990); every LAND field
of the cell and its quad bits were read again and nothing holes it, so `--draw-ride-cuts-terrain`
(`play_ride_terrain_cut_v1`, in the preset) leaves out of the window's drawing the terrain triangles over the moving
part the walker stands on while it stands there (`PartMotion::carries`, `box_now`); both 25th records' descents now
show the shaft to the cut and the ground at the top is unchanged (pixel for pixel before the walker steps on). The
vault's thirteen cryo pods are drawn: `--draw-furniture-template` (`furn_template_text_render_unchanged_v1`) sets
aside their object template (one entry naming no part) and activate text, `--draw-furniture-parts` (now in the
preset) with `--draw-furniture-at-rest` (`furn_parts_drawn_at_rest_v1`) draws their parts at rest, the door
sequences not run. The objective pointing at the player's pod is shown by stage 710's fragment only while the pod is
unused, and in the 25th second record the quest's chain stopped at stage 410 (the broadcast trigger entered only
before its prerequisite), which leaves the elevator's neighbours disabled in the vault: no fault found in the run, no
cue written. Left: a cue for that wait, the nursery prompt (reachable only from about 23 degrees down at the
reach of 150: the drawn infant lies 42 to 61 units above the floor), the ten creatures, the road walk beyond the
second bridge, round 25's items 1, 2 and 4 (janela22 RESULTADO).

Twenty-eighth-session fixes: the stale picture at the second bridge was not reproduced. Replayed in the window with
the session's own viewer and preset, every frame from the bridge's load (1.65 s, nothing loading after it) to the
record's end follows the walker; the person's last frame shows the walker standing on the bridge's broken far end,
not on the earlier deck, and a walk-on past the record's end loads the next square and follows the road. The
"loading" on that last frame is, read from the code, the status line's standing clock while the run finishes after
Escape (not observed); once the person's input is over the line now reads "ending", and the window's log names the
status line's causes each time they change. The cryo pods are
solid: `--play-collision-furniture-fields` (`furn_fields_collision_unchanged_v1`) sets aside a furniture's unread
fields, template, text, scripts and its reference's location fields for its collision, and
`--play-collision-furniture-at-rest` (`furn_collision_at_rest_v1`) places its bodies at their stored transforms
although the doors' sequences can move them (the pods' models store collision on one physics system). The pod the
run seats the player in no longer traps the walker: `--play-sit-passable`
(`play_sit_furniture_passable_until_left_v1`) hides its collision until the walker's feet leave its box, and
`--play-player-sit-marker` (`play_player_sit_marker_v1`) puts the walker at the pod's one SNAM marker, 63 units
toward the front window, in the dream's pod and on waking (the data gives no open pose for the drawing: the run's
graph moves the player's pod to its exit and unoccupied states, whose sequences the window does not run). The
dream's longer load was not reproduced headless: with and without round 27's three furniture drawing switches the
dream cell loads in the same 1.9 s after the session's earlier loads. Left: a drawn open pod, the nursery prompt from
the foot end (the infant's box is 138 to 160 units from that eye, past the reach of 150, a caller's number no data
gives), the exit span, the lower shaft's handoff (the load door's stored teleport, then the lower deck's sequence:
data both), round 25's items 2 and 4 (janela23 RESULTADO).

Twenty-ninth-round fixes (the twenty-seventh sessions): the sofa and the crib a person walked across are persistent
references of the worldspace's persistent cell, which the drawing joined and no walking world read:
`--play-collision-persistent` (`play_world_persistent_refs_walk_in_square_v1`, in the preset) joins them to each
walking world's build, and the 27th A record replayed headless gives the same 274 stages while the walker is stopped
by the sofa and by the crib's side. A census of the house's and the vault's furniture, movable statics, activators
and containers (`column_probe` with `COLUMN_PROBE_CENSUS`): the vault's benches and tables were refused for one unread
furniture subrecord, `FNPR`, now among the furniture fields both readings set aside; the containers (dressers,
changing table, washer, dryer, wet bars, the vault's lockers and desks) store collision but stand in no walking
world (container fields and drawer sequences not read; the house's are not drawn either); scripted activators and the pod opposite the player's (its reference's
`ONAM` and `XACT`) stay out. The exit deck's wait is the gear door's own sequence (its deck parts at their stored
transforms for 36 s of the 43.3 s opening, which starts about 15.7 s after E); the status line now says
`platform moving - ready in N s`, then `platform ready`. Driven headless at two pods of each row, the pods' fronts and
sides stop the capsule within 9 units of their bounds (three sides face a stand in the gap, the rears no floor). The dream's hold is the walker's walking world built in the
window while the window loads the same cell (8.2 s in the sessions, 2.9 s headless on the same build): not cut. Next:
a memo of parsed collision models across the walker's builds (the dream room shares the vault's models), the
containers' fields, the player's pod drawn open, the town creatures' close looks, the road past the second bridge.

Thirtieth-round fixes (the twenty-eighth sessions): the stop on open road before the larger bridge is not round
29's persistent join (the same with it off) but the road's own collision mesh, a dip of 8 to 10 units with a curb
beside it, where the step-up took a retry that did not climb every tick: `--play-step-lookahead`
(`walk_step_lookahead_unless_climbed_v1`, in the preset) tries shorter retries ahead first, and the capsule walks out
on the person's heading (spec/walk.md, spec/play.md). The stop on the line to the control-room stairs is another
cause, a vault table's collision in the vault cell whose drawing the window keeps but its frames do not show: not
settled, not changed. The cafeteria jamb, the sofa's and the crib's fit are the stored collision shapes (nothing
changed); the recreation desk is a container, neither drawn nor solid. The platform cue speaks only of the platform
carrying the walker or set moving after its activation. `fo4_core::Budget::refused` is the one test for the budget's
own refusals (collision build, persistent join: a reference's data limit is that reference's failure). Next: why the
vault table is not seen; the containers' fields (drawing and collision together); a memo of parsed collision models
for the dream's hold; the player's pod drawn open.

Thirty-third-round fixes (the thirty-first sessions): the held cue was on in the live window at every push the
person reported (the window log names the status line's causes at each change), but the person looked 3.5 to 6 s
after letting go, past the 2 s it stayed, and one push held the walker under the second it took: it now says
`blocked` after half a second, stays up to 8 s while the walker stands there, and has a row of its own below the
objective (a live window with injected keys at the control stairs shows it during and after the hold). The ground
that ended before the ruined town was the prewar worldspace's data: the next square's cell stores no LAND; the
follow grid had loaded it. `--play-land-edge` (`play_walk_held_at_land_edge_v1`, in the preset) holds the walker at
such an edge with `blocked - no ground beyond`. `fo4-view --draw-water` (`cell_water_height_plane_v1`, in the preset)
draws a flat translucent square at each loaded cell's stored water height (XCLW); the water type's colours, the
world's default water height and placed water planes are not read, walking is unchanged. Probed, not changed: the
bridge approach's and the house street's walker columns all stand on terrain (no gap in the walking world); the
street's road is not a placed reference there and the parked car's bounds stand some tens of units above the
terrain. Measured, not changed: at the first bridge the walker's walking world took 3.5 s (warm process) and the
window's drawing load 1.7 s plus 0.4 s of upload while the play clock stood. Next: build the next square's walking
world ahead on its own thread when the walker nears a square's edge (the clock then need not stand), upload a
square's draws in slices; the water type's colours (WATR, no public layout read yet); placed water planes; the
cone that clips the camera when looked down at; the parked cars' height; the containers' fields.

Thirty-second-round fixes (the thirtieth sessions): the platform cue reads a part's visible motion (a held pose's
keys repeat with noise) and speaks only of the part carrying the walker: moving, starting in N s, ready, else
nothing (`Platform::carried_moving`, `carried_starts`, `carried_until`; `play_run --play-report-platforms` logs it).
The title names the eye the frame draws from (it lagged a frame at the cut onto the rising surface deck). A movable
static at most 64 units along each axis has no collision under `--play-light-movables`
(`play_light_movable_static_passed_v1`, in the preset; the limit stands in for a mass this project does not read):
the walker passes the cone on the line to the lift's apron and the one by the barricade's rail; the crate beside the
first still stops it. `--play-held-cue` (in the preset, a viewer aid) says `blocked` (`- low obstacle` under 48
units) after a second held against a shape. Measured, not changed: the dream's and the first bridge's holds are
mostly the walking world's scene load (4.6 of 7.4 s, 8.0 of 9.2 s headless), not its collision models. Next: a
loader kept across walking world builds (the grid squares a build shares with the one before), the containers'
fields, the water planes, the player's pod drawn open, the town's creatures and the far road, the route from the
lift's apron over the bridges to the waterline and back to the house street (replayed past the 30th session's
end).

Thirty-first-round fixes (the twenty-ninth sessions): the new stops on clear ground are not round 30's step
look-ahead: the same record replayed with `--play-step-lookahead` off stops at the same three places within a unit,
each a drawn shape below the eye's frame (a 45-unit movable static wedging the walker against a barrier's rail or a
crate, a rock's mesh sliding it into a 52-degree terrain slope); nothing changed for them. The stop on the line to
the control-room stairs is the same kind: the vault table round 30 called invisible is drawn where its collision
stands (a narrow panel 72 units high, seen when the eye looks down; its texture's alpha passes the test on 99.7% of
its texels, so round 30's alpha reading is refuted); nothing changed. The eye's drop back into the shaft after the
first daylight was the handoff: the door to the surface puts the walker where the lift's deck arrives at its rise's
last key while the deck is still at the shaft's bottom; `--play-elevator-door-arrival`
(`play_elevator_door_arrival_on_ride_v1`, in the preset) puts the walker on the deck where it stands and it rides up
(the 29th A replayed: the same 274 stages, the eye rising without a drop). The platform cue on a settled deck read
the end of the deck's whole sequence (the grille's column still moving); it now counts the carrying part's own
motion (`Platform::carried_until`). Next: a status cue when the walker is held by a shape below the eye's frame (the
three stops above and the table), the containers' fields (drawing and collision together), the water planes, the
player's pod drawn open, a memo of parsed collision models for the dream's hold.

The opening run is now visible (`spec/timeline.md`), and since the cast step its people appear as people. `quest_run new-game --timeline FILE` writes what the run
did, second by second, in a neutral versioned text file (track points with their cells, actor places with the
rule or assumption behind each, references enabled, disabled and moved, scene lines with speaker, seconds and
text, stages, objectives); the writer only reads the run, so every other output stays byte for byte. The CPU
render and the headless viewer draw "the run at second S" from it (`--timeline FILE --at S --eye-height H`,
CELL `-`): the player's cell, the run's visibility changes and moves under pending rules, actors as
placeholder markers, the camera on the track (caller's eye height, looking along the track's movement) and an
overlay text beside the frame. A contact sheet of 13 frames along the 86-stage run (local only) shows the
house, the walk outside, the vault entrance with its crowd and the cryopods.

Cast step: with `--timeline-cast` (render and fo4-view) the actors a frame places are drawn as their skinned
characters in the skeleton's rest pose at the run's place, a marker only where an actor cannot be drawn, with
the reason in the overlay; facing from the data only (the stored rotation, `timeline_actor_stored_rotation_v1`,
or with `--timeline-target-rotation` the rotation of the one reference at a moved actor's place); a bone the
skeleton lacks skinned at its bind transform with `--actor-missing-bone-bind`. The 44 actors outside whose
presence was undecided are one class (own flag 0x800 agreeing with an opposite link under an enabled static
root; `spec/timeline-cast-census.txt`), decided by `--timeline-child-flag-agrees`
(`xesp_child_flag_agrees_chain_v1`) together with `--timeline-nowait`. The camera can look at a point, the
nearest drawn actor or the line's speaker (`--look-at`, `--look-at-actor`; presentation). On the contact
sheet with every switch on: 63 distinct actors as characters, 13 as markers; in the vault 23 characters and 6
markers where there were 24 markers; outside after the alarm 35 to 37 characters where there were 4 markers.
CPU and viewer agree (coverage 1.000000, same tag on every pixel at 560 s; a synthetic GPU test).

What the frames show is missing, in order of what blocks a playable opening:

1. Input instead of a file (live window step, `spec/play.md`, `crates/fo4-play`): the run is now stepped by
   input. The run is `fo4-vm`'s library run (`fo4_vm::run`, `quest_run`'s code; the example is a thin
   caller), its clock loop stepped with a horizon (`NewGame::advance_to`), the track folded point by point; a
   scripted input drives the walker over the start cell's collision (`play_run`) and its feet feed the run,
   E's ray activates what it meets (`--play-pick-bounds` for references with no collision, the front door
   among them). Checks: the run's own track replayed in slices gives `quest_run`'s outputs byte for byte on
   the first minute and with the whole furthest run in one slice; in 0.05 s slices the furthest run differs
   only by two waits logged when a scene waiting on a distance condition is offered again (every stage at the
   same second), and with `--play-recheck-marks` (the waiting scenes' marks computed again instead) by
   nothing: census, names, both next-blocker files and timeline byte for byte. The first minute walked: the same 160 stages in the same order, 10 of them 0.014 s later
   (the walker's capsule slides near one corner). The window (`fo4-view --play-run`, `--features
   script-event`) feeds the same loop from the keyboard and mouse, subtitles in the title bar; it has not
   been opened by anyone. Live world step: behind switches the walking world follows the run's Enable,
   Disable and moves each slice as a timeline frame does (`--play-live-world`; the window hides, shows and
   moves the drawn references too, not opened), a load door the activation meets takes the walker into its
   destination's walking world (`--play-load-doors`, checked on a postwar exterior door into its interior),
   the run reads triggers and actors about the walker (`--play-region-walker`), and the walking world follows
   the walker over the grid (`--play-follow-grid`). Doors: the data as read gives no door collision
   (`spec/door-census.txt`), so no plain door reading was implemented. The opening walked headless reaches
   the vault: 197 of the recorded run's 202 stages to 480 s in the same order, then the walker falls into the
   elevator shaft at 416.5 s (the platform has no placed collision). Next: the elevator platforms (their
   collision and their ride), moving collision with a node; the lines' audio. Run library step: the run's
   cost fell from 232 s to 51 to 72 s of wall time for the 480 s of input on this machine (stored positions,
   placements' cells, globals and bases read once; `spec/play.md`, Costs), now mostly the walking world's
   rebuilds about the walker (`--play-follow-grid`, decompressing the cells' models) and the condition
   evaluator's line choices: about real time over this walk headless, not yet in a window.
   elevator shaft at 416.5 s (the platform has no placed collision). Seen step: the shaft is an animated
   activator whose collision is refused with the walkway's static; placed as stored and split by the nodes its
   descent sequence moves (`spec/walk.md`, Platforms) they hold the walker on the platform at the top and carry
   it down 4,140 units from the second the run enters the descent's state (`ride_check`, headless; the second
   taken from the run's names file). Next: `fo4-play` building those pieces and starting the ride from the
   run's state (request in `spec/play.md`, The elevator ride), the timeline recording sequence starts, the
   load door at the shaft's bottom (faces step: the boxes' presence from the run's frame and the rider passing
   the terrain below the shaft are read behind `ride_check` switches, `spec/walk.md`, The ride's loose ends); the run's cost (`fo4-vm`'s distance dues search
   taken from the run's names file). Third fight step: `fo4-play` builds those pieces and rides them from the
   run's state (`--play-elevator`, `--play-elevator-also`, `--play-elevator-door`; `spec/play.md`, The elevator
   ride): the opening walked headless now matches the recorded run's **204 of 204 stages to 500 s** in the same
   order, rides down both elevators into the vault, then falls at about 520 s walking off the interior platform.
   Next: what is missing between the interior platform and the vault's floor, the enable state of
   collision-marker boxes, the elevator in the window; the run's cost (`fo4-vm`'s distance dues search
   every placement each turn: 407 s for 480 s of input); the lines' audio; then the library run in `fo4-vm`
   (`spec/play.md`, Request) so `fo4-play` stops compiling the example.
   Window step: the window runs the opening without a person. `fo4-view --play-run FILE --play-input INPUT`
   takes every `play_run` play option through one structure (`apps/fo4-view/src/play_switches.rs`, until
   `fo4-play` offers the parser in its library: requested), drives the run with `play_run`'s walker on the run's
   thread held to the window's clock, stands the camera at the walker's eye, loads the cell the walker's walking
   world was last built about at each change of space (11 loads on the opening, 4 to 7 s each, the run waiting)
   and writes the window's frames (`--play-shots DIR --play-shot-every S`). With the exit step's switches and
   input its outputs equal the headless run's byte for byte (track, censuses, names, next blockers, timeline: 276
   stages to 1,000 s); about 20 minutes of wall clock for the 1,015 s of input, 60 frames a second (median) on
   this machine. What the frames show (black sky, the elevators and doors not moving or not drawn, people in the
   rest pose, a fall into the void while the run seats the player in the cryo pod's cell) is in the window
   step's notes. The stop at 998 s, measured against the navigation meshes: the walker stands where no navmesh
   triangle lies, under a face of about 80 units whose top agrees with the navmesh within 3 units; it left the
   navmesh's heights about 2 s before, where a placed rock's collision ends in a 58-unit drop and the navmesh
   comes down at 38.8 degrees. The 45-degree limit was not changed: a higher one would not climb that face.
   Next: waypoints that keep to the navmesh's heights (or the rock's edge read against the navmesh), the drawn
   platforms moving with their collision (request to `fo4-play` for the platforms' motions), the walker's state
   read from `fo4-play` instead of its log, a sky.
2. Characters stand in the rest pose (arms out) and do not move between the run's places. Next: the run's
   animation events and packages to clips (the character graph classes the readers do not read yet,
   `spec/character-graph-census.txt`), a walk between places. 13 actors are still markers: 9 whose outfit
   cites a texture path the archives refuse with a missing texture (the whole actor is blocked; a rule that
   leaves only that surface out would draw them), 2 with effect-shader controllers run by a manager, 1 with
   a transform controller in its model, 1 with no skin and no outfit (not a character model).
3. The camera is the caller's: by default it looks along the last move and often faces a wall; with
   `--look-at-actor speaker` it looks at the speaker or the nearest drawn actor, which can still be behind a
   wall (no visibility test). The player has no facing in the run. The elevator second (505 s) still rides a
   straight line outside the geometry: the data holds the platform (an activator with NIF controller
   sequences and a behaviour graph), but the run records no event that starts it, and a sequence player,
   skinned non-actor models and riding a moving node are missing (`spec/timeline.md`, The elevator second).
   Faces step: the outside elevator's shaft model is drawn by its skinned shapes on its own nodes
   (`--timeline-ride-model`) and its platform at the descent's place from the caller's start second
   (`--timeline-ride SEQUENCE,START`; `spec/timeline.md`, Drawing the shaft and the ride); its lights and
   effects are left out; 322 models of the placed kinds are skinned the same way and still refused elsewhere.
   Poses step: `--skinned-models-own-nodes` draws every placement a frame refuses for its skin that way (on the six
   sheet seconds 182 of 197 such placements drawn; census of the 322 models by what moves their bones in
   `spec/poses-census.txt`, section 3); the shaft's two rigid effect shapes, visibility controller and material swap
   stay out (no pure format reading draws them). The viewer's block bound is 640 MiB and a path kept under both
   texture caps at one level is kept once (`spec/render.md`, Viewer block memory of a crowded frame): the town
   frames draw at the default caps.
4. Sound: the run times lines by their voice files but nothing plays them.
5. Lighting and sky (light step, `spec/light.md`): behind nine switches, off by default, frames are lit from the
   data under pending rules (interior cell light and its template's inherited fields, placed lights as points,
   fog; outside, the climate's one weather at the caller's `--assume-hour`: sky gradient, ambient cube, fog).
   Seen step: behind switches an exposure from the light on what the camera sees (`--exposure-eye`), the image
   space's white, cinematic values and tint (`--exposure-white`, `--exposure-cinematic`; labels are prior
   knowledge) and vertex normals (`--light-vertex-normals`): the vault at 560 s reads (scale 0.654), CPU and
   viewer agree. What is still wrong: no normal maps, specular or shadows, no sun or moon light (the data stores
   no direction for it), no clouds, spots drawn as points, no bloom or adaptation over time; the exposure's
   measure is chosen. Next: the light bench of `spec/light-measurement.md` would settle the falloff, a
   measurement of the game's response the exposure.
6. The run itself stops at stage 1000 (Script VM and quests, above).
7. Screens (menus step, `spec/ui.md`, `crates/fo4-ui`): every interface movie (344, all zlib, every tag parsed)
   is read by a bounded reader over the `swf` crate, counted (`spec/ui-census.txt`), and drawn as a static
   first frame (shapes and sprites, `ui_frame`, pictures in the notes only). A static reading of the dialogue
   and heads-up screens' AVM2 code (`ui_host`, `spec/ui-host.txt`) finds the one member the host fills, the
   three functions the dialogue movie calls on it, its eleven entry points and the subtitle component's two
   text fields the host writes by placement path; `spec/ui.md` says what a host must give the dialogue screen
   for the four choices `fo4-vm` already holds, under eight pending rules. Next, in order: a way for a
   stepping caller to give `fo4-vm`'s scene clock the player's choice at run time (today only
   `--assume-player-choice`); the player line's prompt text read by `fo4-plugin`; then either an AVM2 player
   of the project's own for these two movies (display list over time, text fields, the host member) or a
   drawn dialogue overlay in the window that follows the same protocol; the inventory, container and wrist
   device screens after that.

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
