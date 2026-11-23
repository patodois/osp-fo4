# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-23. Nothing below has been checked against the running game: every behaviour is a pending
rule in `spec/hypotheses.tsv`, off by default.

## Script VM and quests (`crates/fo4-vm`)

The forced opening (`quest_run`, record in `spec/quest-run-opening.txt`) reaches 86 of 104 stages
(graph step; before it 83). The next sequence quest's main scene now runs to its end: the spouse's 14
animation events are the caller's (`--assume-anim-event`), at the second the script snaps her into the
furniture plus each annotation time of the one clip that names them; the scene's phase fragments call the
two sequence functions that set the next two stages, and the second's stage fragment sets the next, which
starts the next main quest and its exit scene. A scene waiting on a quest script variable is now offered
again when a script writes it (`--scene-recheck-variables`); at the clip's seconds the run does not need it
(each phase finds the value when its lines end), at later seconds it does. The stage-setter table follows
calls from other scripts into quest script functions (`stage_census`; `--setters-outer-calls` for
`--next-blockers`). The run stops at, in order:

1. Stage 1000: set only by the next main quest's handler of an animation event of the player's cryopod
   (a NIF text key and graph event of the pod), registered by the exit scene's fragment; the run raises no
   such event. Next: find what plays the pod's sequence in the exit scene (a scene fragment, a package, the
   activation) and whether the activator graph reading reaches the key, before assuming the event.
2. The exit scene's looping successor waits on a vault dialogue quest stage set by a placed trigger (the
   player entering it) or a menu closing; stage 5000 by fourteen placed triggers: both are track or caller
   actions.
3. Sixteen earlier stages are on no path the run takes (two of them have no setter in the data).

Character behaviour graphs (`char_graph_census`, `spec/character-graph-census.txt`): a race names its
behaviour project, and the project, character and root graph resolve, but which clip an actor plays is held
in classes the readers do not read (clip generators, behaviour references, nested machines, transition
conditions and variables, how furniture keywords select a state). Reading those is what would let actor
animation events come from the data instead of the caller.

Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state native; actors sitting
through packages; magic-effect receivers of furniture events; keyword data, `ResetKeyword`, keywords in
conditions, template flags and object modifications for keywords; collection aliases filled other than from
a location's rows; actors' animation (character graphs).

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
but not removed. The viewer's GPU tests now pass in parallel (`spec/render.md`, Viewer GPU tests in parallel).

## Toward a playable game

Not started: the viewer driven by the quest host (actors, track and scenes on screen), actors walking on the
navmesh, AI packages beyond destinations, sound, saving moved references.

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
