# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-23 (timeline step). Nothing below has been checked against the running game: every behaviour is a pending
rule in `spec/hypotheses.tsv`, off by default.

## Script VM and quests (`crates/fo4-vm`)

The forced opening (`quest_run`, record in `spec/quest-run-opening.txt`) reaches 87 of 104 stages
(exit step; before it 86): the prewar quest ends. The player's postwar pod now opens from the data: a
scripted activation with events sits the player in it (`--activate-furniture-events`), the exit scene's
activation by the sitter gets the player up (`--activate-furniture-get-up`), and the get-up sends the pod's
behaviour graph a use event the caller names (`--furniture-graph-events`), whose two sequences raise the key
the exit scene's fragment registered for. The last stage needs the player's own camera event (a character's
animation, the caller's `--assume-anim-event player`) and `IsInMenuMode` answered (the caller's
`--assume-menu-mode false`). The graph step's `--phase-conditions-end` stop was the caller's cap of scene clock
advances, not a host bug. The 17 stages the run does not set are classed in
`spec/quest-run-opening-unset.txt`: a debug start, four other choices or actions of the player, one that
depends on an order the data does not settle, nine with no setter, one with no path, and the failure stage
(the player leaving the area dies). What is left:

1. The exit scene's looping successor waits on the vault dialogue quest's stage set by one trigger's
   script when the player walks in and picks the last button of a message box (`--assume-message-button`);
   in the game the trigger is past the vault door, which the next main quest's console sequence opens on the
   player's animation events. A labelled variant assumes the walk and the button and ends the scene. Next:
   the next main quest's stages (the wrist computer, the door console), which wait on the player's animation events.
2. The one stage whose reaching depends on order (a scene stopped by the stage its line's end half sets
   before the phase that would set it): a measurement or a reading of when a line's end-half stage is set
   against its phase's end would settle it.

Character behaviour graphs (`char_graph_census`, `spec/character-graph-census.txt`): a race names its
behaviour project, and the project, character and root graph resolve, but which clip an actor plays is held
in classes the readers do not read (clip generators, behaviour references, nested machines, transition
conditions and variables, how furniture keywords select a state). Reading those is what would let actor
animation events come from the data instead of the caller.

Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state native; actors sitting
through packages; magic-effect receivers of furniture events; keyword data, `ResetKeyword`, keywords in
conditions, template flags and object modifications for keywords; collection aliases filled other than from
a location's rows; actors' animation (character graphs); a furniture's `OnActivate` on a script's activation;
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

Open checks: the viewer window has still not been exercised by hand with `--time` (nor with any clock switch;
the per-frame path is tested headless through `GpuScene::at_time`); the CPU-against-viewer tag differences are
explained (coplanar overlapping placements and edges, `spec/render.md`, Instance tags of the CPU and the viewer)
but not removed. The viewer's GPU tests now pass in parallel (`spec/render.md`, Viewer GPU tests in parallel). Seen while checking timeline frames: in the opening's exterior house cell with `--terrain --around 1`, the CPU draws what lies beyond a window where the viewer draws the clear colour (coverage agreement 0.983 with or without a timeline; an interior vault frame agrees at 1.000000); not investigated.

## Toward a playable game

The opening run is now visible (`spec/timeline.md`). `quest_run new-game --timeline FILE` writes what the run
did, second by second, in a neutral versioned text file (track points with their cells, actor places with the
rule or assumption behind each, references enabled, disabled and moved, scene lines with speaker, seconds and
text, stages, objectives); the writer only reads the run, so every other output stays byte for byte. The CPU
render and the headless viewer draw "the run at second S" from it (`--timeline FILE --at S --eye-height H`,
CELL `-`): the player's cell, the run's visibility changes and moves under pending rules, actors as
placeholder markers, the camera on the track (caller's eye height, looking along the track's movement) and an
overlay text beside the frame. A contact sheet of 13 frames along the 86-stage run (local only) shows the
house, the walk outside, the vault entrance with its crowd and the cryopods.

What the frames show is missing, in order of what blocks a playable opening:

1. Input instead of a file: the track is the caller's. Next: a window mode that runs `fo4_vm` live, the
   walker (`fo4-walk`) giving the player's position each tick in place of the track, the run's clock driven
   by the frame clock; triggers and distance events then come from where the player walks.
2. Characters: actors are boxes. Of the 53 actors placed near the track outside, 4 are marked, 5 are
   disabled and 44 are not marked because their enable parent chains carry what the enable rules leave
   undecided (inside the vault 23 or 24 are marked, 5 undecided). Skinned actors exist in the viewer
   (rest pose, a named clip); joining them to the run's positions, and the run's animation events to clips,
   is the next drawing step.
3. The camera is the caller's: it looks along the last move, so it often faces a wall or a pod; the player
   has no facing in the run.
4. Sound: the run times lines by their voice files but nothing plays them.
5. Lighting and sky: frames are unlit and the exterior sky is the clear colour.
6. The run itself stops at stage 1000 (Script VM and quests, above).

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
