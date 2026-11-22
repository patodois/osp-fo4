# Quest records (`QUST`)

`fo4_plugin::quest::Quest::read` reads a QUST record to its last byte:
every subrecord is placed by the order grammar below and either given a
field (typed) or kept as stored with its tag and size (raw). What the
fields do in the running game is not a format fact; where code relies on
it, it is a pending rule in [hypotheses.tsv](hypotheses.tsv) (the
`quest_*` rules, [vm.md](vm.md), Quests).

## Sources

- No public source was read for this record. The subrecord tags are the
  four-character tags stored in the data. The field names and the layout
  below were recognised from prior general knowledge (the published record
  and field names of xEdit are an allowed naming source, as
  [plugin.md](plugin.md) records, but no page of it was opened at this
  step); every one is kept only because the census over the data
  (`quest_census`, [quest-census.txt](quest-census.txt)) could have refuted
  it and did not. The rule `quest_qust_layout_v1` holds the whole layout
  as pending.
- The FormID contract, localized strings and the record envelope are those
  of [plugin.md](plugin.md).

## Layout

Order (every part optional unless stated; `*` repeats):

```text
EDID  VMAD  FULL  DNAM  ENAM  LNAM  XNAM  QTGL*  FLTR
condition*  NEXT  condition*
stage*        INDX  (QSDT condition* NAM2 CNAM)*
objective*    QOBJ FNAM NNAM  (QSTA condition*)*
ANAM
alias*        [ALCS ALMI] ALST | ALLS ; ALID FNAM value* ALED
GNAM  SNAM
condition:    CTDA [CIS1] [CIS2]
```

| Tag | Bytes | Read as | Census |
| --- | --- | --- | --- |
| EDID, FLTR, NAM2, ALID, CIS1, CIS2, SNAM | text | bytes before one closing NUL | all end with one NUL |
| FULL, CNAM, NNAM | 4 | string table id (localized plugin), else text | all 4 bytes |
| VMAD | any | read by `fo4_plugin::vmad` ([vmad.md](vmad.md)) | 1,353 of 1,353 to the last byte |
| DNAM | 12 | u16 flags, u8 priority, u8 byte, u32 word (zero in all), u32 (0 to 7) | 1,898 |
| ENAM | 4 | four-byte code, kept as stored: the story event code of the nodes listing the quest ([story-manager.md](story-manager.md)) | 990; equal to the event code of every node listing the quest in 987 of 987 quests with both |
| LNAM, XNAM, QTGL, GNAM | 4 | FormID | LCTN, GLOB, GLOB, KYWD in every one |
| CTDA | 32 | kept as stored here; field by field by `fo4_plugin::condition` ([conditions.md](conditions.md)) | 14,960 |
| NEXT, ALED | 0 | marker | |
| INDX | 4 | u16 stage, u8 flags, u8 byte kept as stored | 12,855 |
| QSDT | 1 | u8 flags | 13,393 |
| QOBJ | 2 | u16 objective | 1,884 |
| FNAM | 4 | u32 flags (objective or alias) | |
| QSTA | 12 | i32 alias index, u32 flags, FormID (zero or KYWD) | every alias index names a reference alias of the quest |
| ANAM | 4 | u32 | above every alias id in 1,670 of 1,670 quests with aliases and ANAM |
| ALST, ALLS, ALCS | 4 | u32 alias id | |
| ALMI | 1 | u8 | after every ALCS |
| ALFR, ALUA, ALFL, ALCO, ALEQ, ALRT, ALDN, ALFC, ALSP, ALPC, ECOR, SPOR, ALFV, ALDI, VTCK | 4 | FormID | ALFR: REFR or ACHR (17 name no winning record); ALUA NPC_; ALFL LCTN; ALEQ QUST; ALRT LCRT; ALDN MESG; ALFC FACT; ALSP SPEL; ALPC PACK; ECOR, SPOR FLST; ALFV VTYP; ALDI LVLI; VTCK zero or VTYP, FLST, FACT, NPC_ |
| ALFA (+ ALRT), ALEA (after ALEQ), ALFI, ALNA (+ ALNT) | 4 | i32 alias index | each names an alias of its quest (ALEA: of the ALEQ quest) |
| ALCA (after ALCO), ALCL (after ALCA) | 4 | u16 alias index and u16 word; u32 | ALCA's index names an alias in 1,927 of 1,927 |
| ALFE, ALFD | 4 | four-byte codes, kept as stored | ALFD after every ALFE |
| ALCC | 4 | u32, kept as stored | |
| KSIZ + KWDA | 4 + 4n | count, then n KYWD | counts agree |
| COCT + CNTO* | 4 + 8 each | count, then (FormID, i32 count) | counts agree |
| ALLA | 8n | (FormID zero or KYWD, i32 alias index) pairs | 3,777 of 3,806 indices name an alias of the quest |

Of 3,892,827 bytes in the 1,905 winning QUST records, 3,414,107 are typed
(2,227,761 of them VMAD bytes read by the VMAD reader) and 478,720 are
raw, all of them CTDA (14,960 conditions, every byte of which
`fo4_plugin::condition` now places in a field, kept as stored where the
census gives no meaning: [conditions.md](conditions.md)). The census lists every tag followed by every tag it
was seen followed by ([quest-census.txt](quest-census.txt), subrecord
order observed): the grammar above admits every observed sequence. A
record that does not fit is refused with `QuestSubrecordOrder` (the
expected tags, the tag found), `QuestFieldSize`, `QuestFieldText` or
`QuestCount`, with file and offset.

A log entry's "next quest" field, which the task description anticipated,
does not occur in this data: no subrecord follows CNAM inside an entry.

## What the census shows

- 1,905 winning QUST records, every one read; 7 without DNAM.
- Stages: 543 quests have none; 12,419 of 12,855 stages have one log entry,
  74 none; 330 log entries with conditions are followed by another entry
  of their stage.
- Flags as stored (histograms in the census): DNAM bit 0 in 457 quests,
  bit 3 in 78; INDX flags 0x00 (11,209), 0x02 (966), 0x04 (654), 0x08 (20),
  0x0a (6); QSDT flags 0x00, 0x01, 0x02.
- Aliases: 14,432 reference, 884 collection, 970 location aliases. Fill
  subrecords present on reference aliases, most frequent first: ALNA with
  conditions 3,460, ALFA 2,155, ALUA 2,073, none 1,424, ALFR 1,379, ALCO
  1,320, ALFE with conditions 789, ALEQ 505; the census lists every
  combination for each kind.
- Forced references: 1,402 ALFR; 1,328 name a winning placed REFR and 57 a
  winning placed ACHR, none deleted; 17 name no winning record.
- Stage fragments ([vmad.md](vmad.md), QUST block): of 10,506 fragments in
  1,300 quest blocks, 10,375 map to a stage of their record and a log
  entry index below that stage's entry count, 131 name a stage the record
  does not hold; no (stage, entry) pair repeats. 9,953 stages have one
  fragment, 244 two or more (one per entry). The block's fragment script
  is absent from the record's VMAD script list in 1,282 of 1,300 blocks
  (18 have an empty name). The fragment script's properties hold 5,428
  alias values; 334 name an alias of the same quest that has an ALFR.

## What is not known

- The meaning of every flag bit (DNAM, INDX, QSDT, QOBJ FNAM, QSTA, alias
  FNAM), of DNAM byte 3 and the u32 at 8, of INDX byte 3, of ALCA's high
  half, ALCL, ALCC, ALNT, ALMI, ALFE and ALFD; of ENAM, only the code
  `SCPT` is read (`sm_script_event_v1`).
- What starts a quest that is not flagged to start: the story manager
  nodes and the starting call sites of the compiled scripts are counted in
  [story-manager.md](story-manager.md); 62 of the 1,448 such quests have
  neither. The bits the
  interpreter relies on (DNAM bit 0 and bit 3, INDX bit 1) are named in
  pending rules (`quest_start_game_enabled_v1`, `quest_repeated_stage_v1`,
  `quest_start_v1`).
- Conditions (CTDA) stay raw in this reader's byte accounting; their
  fields are read by `fo4_plugin::condition` ([conditions.md](conditions.md)),
  and `fo4_vm::cond` evaluates a stage's log entry conditions when the
  stage is set ([vm.md](vm.md), Conditions); the conditions after NEXT
  are evaluated when the story manager offers the quest an event
  (`sm_quest_node_start_v1`, [story-manager.md](story-manager.md)); the
  conditions before NEXT, objective target and alias conditions are
  counted there, not used.
- How an alias is filled in the running game, except that the interpreter
  takes an ALFR alias of a running quest to hold its reference
  (`quest_alias_forced_reference_v1`).
