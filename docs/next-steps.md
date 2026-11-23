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
of the game clock, but 28.77 % at any later second: the distant clouds and other effects whose falloff or
emissive tracks move are refused there (`UnsettledClockField`). With the three switches below the census is the
same at every time and leaves 2.46 %.

The clock now also drives the effect emissive multiple, emissive colour and falloff fields, each behind its
own switch, off by default (`--clock-emissive-multiple`, `--clock-emissive-colour`, `--clock-falloff`;
`spec/controller-clock.md`, Effect fields on the clock). The families that lead what is still refused:

1. Effect controllers run by a manager (`effect-idle`). The census (`spec/effect-clock-census.txt`, section 2)
   found no NIF field marking a default sequence; the one marker is the behaviour graph's start state (295 of 465
   models). A rule needs a sequence player (a sequence's own timing, its transform interpolators) and settles
   `behavior-graph` at the same time; it contradicts the unstarted-loop rule (`spec/controller-clock.md`, Effect
   controllers run by a manager).
2. Models whose sequence is chosen by a behaviour graph (same machinery as 1).
3. Moving geometry (transform controllers): needs world transforms over time, rotation key rules, the viewer's
   instance data per frame and the walking world kept in step. Not started.
4. Record runtime state on placements (several refused subrecord families, listed in the census).

Open checks: the viewer window has still not been exercised by hand with `--time` (nor with the new switches);
the CPU-against-viewer tag differences are explained (coplanar overlapping placements and edges, `spec/render.md`,
Instance tags of the CPU and the viewer) but not removed. The viewer's GPU tests fail at random when run in
parallel (shader loading between concurrent apps, also on the commit before); run them with
`cargo test -- --ignored --test-threads=1`.

## Toward a playable game

Not started: the viewer driven by the quest host (actors, track and scenes on screen), actors walking on the
navmesh, AI packages beyond destinations, sound, saving moved references.

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
