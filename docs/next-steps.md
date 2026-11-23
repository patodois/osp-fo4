# Where to pick up

This page is the hand-over point. Everything on `main` builds and passes its tests; anyone (a person or an
agent) can continue from here. Read `AGENTS.md` first: it holds the rules of evidence and the clean-room rules.

State on 2026-11-22. Nothing below has been checked against the running game: every behaviour is a pending
rule in `spec/hypotheses.tsv`, off by default.

## Script VM and quests (`crates/fo4-vm`)

The forced new-game opening (`quest_run`, record in `spec/quest-run-opening.txt`) reaches 83 of 104 stages
(keyword step; before it 82). The keyword test that stopped the sit step's run now passes from the records
(`--keywords-records`, `--keywords-runtime`), the next sequence quest's collection alias is filled from its
location's rows (`--collection-location-refs`), and that quest's main scene starts and runs to its 8th phase.
The run stops at, in order:

1. The main scene's 8th phase waits on a quest script variable that only the handler of the spouse's
   animation events sets (14 events, whose names occur in the data as clip annotations and behaviour graph events). Needed: either
   a character's behaviour graph followed to the clip's annotation times, or the caller's assumed events;
   and, in both cases, waiting scenes offered again when a quest script variable changes (today the
   recheck follows quest state only: a run assuming the 14 events still stops there).
2. Two stages set in two functions of the sequence script that the scene's later phase fragments call,
   then the stage the second one's fragment sets. No new rule is needed for the calls; the stage-setter
   table should follow calls from other scripts' fragments into quest script functions.
3. Two stages with no setter and no reader anywhere in the data (documented in `spec/vm.md`).

Caller actions can now wait for the stage the story sets before them (`--action-after`); the final run gates
all eleven timed ones, so the one-thing-changed table no longer counts stages those actions set out of order. The player's
track and the actor moves (`--assume-actor-at`) still happen at fixed seconds.

Not modelled yet: unload, cell-load, cell-attach and cell-detach events; the sit state native; actors sitting
through packages; magic-effect receivers of furniture events; keyword data, `ResetKeyword`, keywords in
conditions, template flags and object modifications for keywords; collection aliases filled other than from
a location's rows.

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
