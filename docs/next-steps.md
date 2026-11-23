# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-24 (vault, cast, light, home, character graph, fight, live window, museum, live world, run library, seen, past-the-museum, faces and third-fight steps). Nothing below has been checked against the running game: every behaviour is a pending
State on 2026-11-24 (vault, cast, light, home, character graph, fight, live window, museum, live world, run library, seen, past-the-museum, faces and poses steps). Nothing below has been checked against the running game: every behaviour is a pending
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
**47 of 54** (adds 122, 125, 127, 128, 130, 135, 150).
What is left:

1. After 150: 140 and 200, the town chain's next quests (their stage fragments set 140, 150 and 200) are not
   started in the run; the town's dialogue quest's 50 fragment sets a stage of one of them that its record does
   not hold (the quest's state is left undetermined: the run's treatment of a missing stage); 200 also from the
   town dialogue quest's 200 (an alias's location change event, not modelled); 62 a container change (the
   musket picked up); 78 behind the locked inner door; the robot's 275 a line's own script of a companion line
   the run does not play. The created reference's place (`ALCA`) and level (`ALCL`) are not read, and the
   borrowed alias is a live link against the knowledge that such fills are made at the quest's start.
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
   (the caller's recorded track rode only to the door's height); past the interior elevator the walker falls.
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
4. A viewer window path (the poses are headless only, as the rest of `--timeline`).

Open checks: the viewer window has still not been exercised by hand with `--time` (nor with any clock switch;
the per-frame path is tested headless through `GpuScene::at_time`); the CPU-against-viewer tag differences are
explained (coplanar overlapping placements and edges, `spec/render.md`, Instance tags of the CPU and the viewer)
but not removed. The viewer's GPU tests now pass in parallel (`spec/render.md`, Viewer GPU tests in parallel). Seen while checking timeline frames: in the opening's exterior house cell with `--terrain --around 1`, the CPU draws what lies beyond a window where the viewer draws the clear colour (coverage agreement 0.983 with or without a timeline; an interior vault frame agrees at 1.000000); not investigated.

## Toward a playable game

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

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
