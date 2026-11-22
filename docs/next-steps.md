# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-22. Nothing below has been checked against the running game: every behaviour is a pending
rule in `spec/hypotheses.tsv`, off by default.

## Script VM and quests (`crates/fo4-vm`)

The forced new-game opening (`quest_run`, record in `spec/quest-run-opening.txt`) reaches 82 of 104 stages.
It stops at, in order:

1. A stage whose fragment calls into the next sequence's script, which waits on a keyword test of an actor.
   Keywords change at run time and the run keeps no keyword state. Needed: a census of the natives that add,
   remove and test keywords, and a keyword state per reference.
2. Two stages set only in functions of that script that no event or fragment reaches in the run.
3. Two stages with no setter and no reader anywhere in the data (documented in `spec/vm.md`).

Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state native; actors sitting
through packages; magic-effect receivers of furniture events. Caller actions fire at fixed seconds whatever
path the run took, which inflates some rows of the one-thing-changed table.

## Drawing (`crates/fo4-scene`, `apps/fo4-view`)

The coverage census (`spec/coverage.md`) leaves 2.63 % of the placed area undrawn under hypotheses.
The families that lead what is still refused:

1. Effect surfaces whose emissive fields are driven by free-running controllers (the draw clock carries only
   UV transform and opacity so far).
2. Effect controllers run by a manager (excluded by the existing idle rules).
3. Models whose sequence is chosen by a behaviour graph.
4. Moving geometry (transform controllers): needs world transforms over time and rotation key rules.
5. Record runtime state on placements (several refused subrecord families, listed in the census).

Open checks: the viewer window has not been exercised by hand with `--time`; the coverage census ran at
time 0 only; a CPU-against-viewer comparison on real data agreed on 96.9 % of instance tags and the rest
was not investigated.

## Toward a playable game

Not started: the viewer driven by the quest host (actors, track and scenes on screen), actors walking on the
navmesh, AI packages beyond destinations, sound, saving moved references.

## Measurements wanted

Each pending rule's row in `spec/hypotheses.tsv` says what observation of the running game would settle it.
A contribution that records such a measurement (method, game version, what was seen) is as useful as code.
