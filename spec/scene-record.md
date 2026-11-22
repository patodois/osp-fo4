# Scene records and the fragment blocks of INFO, PACK, SCEN and PERK

The VMAD of a dialogue line (INFO), a package (PACK), a scene (SCEN) and a
perk (PERK) ends with a block of fragments: functions of one compiled
script that the game runs at points of the record's life.
`fo4_plugin::vmad` reads these blocks to the last byte; `fo4_plugin::scen`
reads the scene record far enough to place a scene's fragments in order
(phases, their conditions, actions, the owning quest). The census tool is
`fragment_census` (in `fo4-vm`), its counts
[fragment-census.txt](fragment-census.txt); what runs them is in
[vm.md](vm.md) (Scenes and fragments). Nothing here is measured in the
running game: the slots, the action kinds and every behaviour are pending
rules in [hypotheses.tsv](hypotheses.tsv).

## Sources

- **Measurement alone.** Two public UESP file format pages were requested
  on 2026-11-22 (`Skyrim_Mod:Mod_File_Format/VMAD_Field` and
  `Fallout4Mod:Mod_File_Format/SCEN`); the server answered HTTP 403 to both
  and nothing of them was read. No other page was looked for.
- The block layouts were found by reading the bytes after the script list
  of every such VMAD (a probe printed them and a tentative reader was run
  over all of them until every block was consumed exactly), then checked
  by the census below. That a flag bit selects a begin, end or change
  fragment, that a SCEN record is made of phases, actors and actions, and
  the roles of the scene tags were recognised from prior general
  knowledge; the census keeps them only where it could have refuted them
  and did not (`vmad_fragment_blocks_v1`, `scen_record_layout_v1`).
- Function names and parameter types come from the compiled scripts, read
  with `fo4_pex`.

## Fragment blocks

Little-endian; a *text* is a u16 byte count and the bytes; a *script
entry* is laid out as in the VMAD script list ([vmad.md](vmad.md)); a
*fragment* is u8, text script, text function.

| Record | Layout after the script list | Census |
| --- | --- | --- |
| INFO | u8 version, u8 flags, script entry, one fragment per set flag bit (0x1, 0x2), lowest first | 2,396 blocks (3,215 VMADs end with the list): flags 0x01 995, 0x02 1,242, 0x03 159 |
| PACK | as INFO, flag bits 0x1, 0x2, 0x4 | 166 blocks: 0x01 18, 0x02 121, 0x03 8, 0x04 7, 0x05 10, 0x07 2 |
| SCEN | as INFO (bits 0x1, 0x2), then u16 count and phase fragments: u8 flag, u32 phase, fragment | 632 blocks: flags 0x00 307, 0x01 48, 0x02 206, 0x03 71; phase fragments 0 in 249 blocks, 1 in 249, up to 10-19 in 4 |
| PERK | u8 version, script entry (no flag byte), u16 count, per fragment u16 index, u16 word, fragment | 32 blocks, 28 with one fragment and 4 with two |

- Version byte 3 in all 3,226 blocks; the fragment byte 1 in all 3,913
  fragments (kept as stored, `UNSETTLED_FRAGMENT_BYTE`); the PERK word 0 in
  all 36; the fragment's script is the block's in all.
- A flag bit outside the measured ones is refused (`FragmentFlags`): it
  would change how many fragments follow.
- **Slots.** The function name ends with `_Begin` for bit 0x1, `_End` for
  0x2 and `_Change` for 0x4 in every INFO, PACK and SCEN fragment (2,555,
  188 and 396). Phase fragments: flag 1 with `_Begin` 350, flag 2 with
  `_End` 386, flag 0 in 2 whose names end with two digits instead (these
  two are not placed: their phase index is past the record's phases). The
  phase index plus one equals the two digits in the name in 736 of 738 and
  is below the record's phase count in 736. The PERK index equals the
  digits ending the name in 36 of 36 and is below the number of PRKE
  entries in 35 (the entry it names has first byte 2 in all 35).
- **Functions** (looked up in the empty state of the script and its
  parents): found for 2,544 of 2,555 INFO fragments, all taking one
  ObjectReference; 177 of 188 PACK, one Actor; 34 of 36 PERK,
  ObjectReference and Actor; 1,124 of 1,134 SCEN, none (2 take a
  ReferenceAlias, 98 have no instructions). The blocks whose script names
  no compiled script: INFO 10, PACK 5, SCEN 2, PERK 1.

After this step every VMAD of the installation is read to its last byte
(`spec/vmad-census.txt`: 4,403,029 of 4,403,029 bytes).

## Scene records

Order (every part required unless in brackets):

```text
[EDID] [VMAD] FNAM phase* actor* action* PNAM INAM [VNAM]
  [CNAM] [ACTV] [CRIS] condition* [KSIZ] [KWDA] [SCQS] [NNAM] [TNAM] [XNAM]
phase:  HNAM(empty) NAM0 condition* NEXT condition* NEXT WNAM [FNAM] [SCQS] HNAM(empty)
actor:  ALID LNAM DNAM
action: ANAM(2 bytes) NAM0 ALID INAM [FNAM] SNAM ENAM (field | condition)* ANAM(empty)
condition: CTDA [CIS1] [CIS2]   (fo4_plugin::condition)
```

| Part | Typed | Kept as stored (reason in `fo4_plugin::scen::UNSETTLED`) |
| --- | --- | --- |
| scene | EDID, FNAM (u32, bits counted), PNAM (the owning quest), conditions | INAM, VNAM, CNAM, ACTV, CRIS, KSIZ, KWDA, SCQS, NNAM, TNAM, XNAM |
| phase | NAM0, start conditions (before the first NEXT), completion conditions (between the two NEXT) | WNAM, FNAM, SCQS |
| actor | ALID (an alias id) | LNAM, DNAM |
| action | kind (the u16 of the opening ANAM), NAM0, ALID, INAM (index), FNAM (u32 kept, bits counted), SNAM (start phase), ENAM (end phase), conditions | every other field, in stored order, with any four-byte word resolved as a FormID on request (`ACTION_FIELD`) |

Named errors: `SceneType`, `SceneOrder` (expected, found, and the part),
`SceneFieldSize`, `SceneFieldText`, and the condition reader's own.

**Census** (4,773 winning SCEN): every record read to the last byte, none
refused; 1,074,725 bytes typed and 738,866 kept as stored.

- Phases per scene: 0 (16), 1 (1,055), 2 (762), 3-4 (1,255), 5-9 (1,269),
  10-19 (348), 20 or more (68). Of 21,285 phases, 18,490 have no
  condition; 1,765 have start conditions only, 928 completion conditions
  only, 102 both.
- Actors per scene: 0 (559), 1 (2,836), 2 (1,051), more (327).
- Actions: 24,958, kinds 0 (13,542), 1 (3,292), 2 (1,201), 3 (3,159), 4
  (986), 5 (502), 6 (2,276); start phase <= end phase < number of phases
  in all; ALID is one of the scene's actors' or -1 in 23,102, another
  alias id in 1,856; INAM distinct within a scene in 4,742 of 4,773.
- What the kinds' fields name, which supports reading the kinds as
  (`scen_record_layout_v1`): **0** a spoken line (DATA a DIAL in 13,503 of
  13,542), **1** a package (PNAM a PACK in 3,328 of 3,328), **2** a timer
  (SNAM and TNAM words that resolve to no record), **3** player dialogue
  (eight topic fields, each a DIAL in all but five), **4** a start-scene
  action (LCEP a SCEN in 1,069 of 1,069, with INTT, conditions and SSPN
  texts), **5** a response line (four topics, DIAL in all), **6** radio
  (DATA a DIAL in 1,861, HTID an SNDR in 360).
- PNAM names a QUST in 4,773 of 4,773 and equals the label of the group of
  kind 10 that holds the record in all. Every winning INFO (102,433) is
  held by a group of kind 10 labelled with a QUST: its owning quest under
  `vm_host_owning_quest_v1`.
- Scene FNAM bits set: 0x4 in 4,765, 0x20 in 2,119, 0x1000 in 854, 0x1 in
  632, 0x10 in 556, 0x2 in 479, 0x100 in 469, 0x40 in 119, 0x800 in 63,
  0x8 in 15. Bit 0x1 is read as starting the scene with its quest
  (`scene_begin_on_quest_start_v1`, prior knowledge); it is not the
  presence of a fragment block (41 of the 632 scenes with the bit have
  one). `story_census` (section 7) finds that of the 632 scenes with the
  bit, 539 are started by no constant `Scene.Start` or `ForceStart` call
  and no start-scene action the scan sees (85%), against 2,367 of the 4,141
  without it (57%).

INFO and PACK records themselves are not read here beyond what the
fragments need: an INFO's owning quest comes from the group that holds it;
its flags, conditions and response data, and a package's data, stay with
the generic readers. The INFO's topic (the group of kind 7 that holds it)
is not indexed by the store and is not read.

## Rules

| Rule | Covers |
| --- | --- |
| `vmad_fragment_blocks_v1` | the block layouts and the slot each fragment attaches to |
| `scen_record_layout_v1` | the scene record grammar and the action kinds' fields |
| `vmad_fragment_instance_v1` | the fragment script instance on the record's handle and the parameters in the sweeps |
| `scene_start_v1`, `scene_phase_progression_v1`, `scene_begin_on_quest_start_v1`, `vm_host_owning_quest_v1` | what a run does with them ([vm.md](vm.md)) |

## What is not known

- What the kept fields mean (WNAM, VNAM, the flag words of scenes, actors
  and actions beyond bit 0x1 of the scene, the timer action's words, the
  start-scene action's INTT and SSPN, the radio and dialogue action fields
  other than their topics).
- What the two phase fragments with flag 0 attach to.
- Which INFO a topic plays, in what order lines are chosen, and when a
  package's begin, end and change fragments run: these are world events.
