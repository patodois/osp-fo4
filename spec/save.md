# Save files (`.fos`)

`fo4_save::Save` reads a whole save file, read-only, into typed tables:
header, screenshot span, version fields, the two plugin lists, the location
table, three global data tables, the change-form table, the form id array,
the array after it and the closing block. Change-form payloads, global data
bodies and the screenshot stay in the file as spans; `ChangeForm::payload`
returns one payload on request (inflated when it is stored compressed).
Nothing here writes a save, decodes the screenshot or interprets a payload.

Every statement below comes from measuring the stored bytes of the save files
of one machine. The overall container layout was recognised from prior
general knowledge before it was measured (see [provenance](../docs/provenance.md)
and the list at the end); it is kept only where the census below, which
could have refuted it, holds. No save editor, format write-up, wiki page,
decompilation or game executable was opened or searched for. Field names
are this project's labels; every meaning given to a field is a pending rule
in [hypotheses.tsv](hypotheses.tsv).

Evidence tags as in [havok-container.md](havok-container.md):
**[measured N]** holds for N items and fails for none unless a second count
says so; **[hypothesis]**; **[open]**.

Saves are the owner's personal data. This file and the census carry only
counts, sizes, field widths, type codes and flag-bit histograms: no name,
text, play time, form id, path or screenshot content of any save.

## Census scope

`save_census MANIFEST SAVE_DIR OUT.txt` (an example of `fo4-save`) reads
every `*.fos` file directly in one folder, opens the manifest's plugins with
`fo4_plugin::typed::Store` and writes counts only, committed as
[save-census.txt](save-census.txt). `save_dump SAVE.fos [MANIFEST] [--all]`
prints one file for a person; its output holds personal data and is never
committed. The save folder is only listed and read.

Corpus: 8 save files, 41,556,124 bytes, written by the same installation as
[pex.md](pex.md) (manifest build declared 1.10.163, 7 plugins). Two files
carry form version 68 and a game version text that starts with the declared
build; six carry form version 69 and a later game version text (written
before the installation was moved to the declared build). 2.5 s wall for the
census, plugin index included; peak RSS about 390 MB.

**Result: all 8 files are read to the last byte** (41,556,124 of 41,556,124
bytes framed; no file refused). Of these, 898,440 bytes (2.2 %) are read
into typed fields and 40,657,684 are kept raw inside framed parts:

| Part | Bytes | Kept as |
| --- | ---: | --- |
| Magic and header size word | 128 | fields |
| Header | 893 | fields |
| Screenshot | 7,864,320 | raw span |
| Form version, game version, plugin block | 2,560 | fields |
| Location table | 800 | fields |
| Global data tables 1 and 2 | 221,007 | type and length words; bodies raw |
| Change-form entry headers | 640,283 | fields |
| Change-form payloads | 9,689,087 | raw spans (inflated on request) |
| Global data table 3 | 22,885,430 | type and length words; bodies raw |
| Form id array | 251,496 | fields |
| Second array | 56 | fields |
| Closing block | 64 | raw bytes |

Raw by kind: global data bodies 23,104,213 (of which table 3 type 1001
alone 22,859,359), change-form payloads 9,689,087, screenshot 7,864,320,
closing block 64.

## Encoding

Integers are little-endian. A *text* is a u16 byte count and the bytes,
without terminator; the reader keeps the bytes as stored.

## Layout

| Part | Fields, in stored order | Evidence |
| --- | --- | --- |
| Preamble | 12 bytes `FO4_SAVEGAME`; u32 *header size* | **[measured 8]** |
| Header | u32 *version* (15); u32; text; u32; text; text; text; u16; f32; f32; u64; u32 *width*; u32 *height* | the fields end exactly at the header size in 8 files (103 to 115 bytes); version 15 in 8 |
| Screenshot | *width* x *height* x 4 bytes | 640 x 384 in 8 files; every fourth byte is 255 in 1,966,080 of 1,966,080 pixels; content not read |
| Versions | u8 *form version*; text *game version* | form version 68 in 2 files, 69 in 6; the text is four dot-separated decimal numbers in 8 |
| Plugin block | u32 *size*; then u8 n, n texts (first list); u16 m, m texts (second list) | the lists end exactly at *size* in 8 files; the u16 count is present in both form versions (0 in the 2 files of form version 68, 9 in the 6 of 69) |
| Location table | u32 offsets of: form id array, closing block, global table 1, global table 2, change-form table, global table 3; u32 counts of: global table 1, 2, 3, change forms; 15 u32 *reserved* | each offset equals the position the sequential reading reaches, 6 x 8 times (enforced); the reserved words are 0 in 8 files (kept, not enforced) |
| Global table 1 | count x (u32 *type*, u32 *length*, *length* bytes) | count 12 in 8 files, types 0 to 11 |
| Global table 2 | same | count 15 (types 100 to 103, 105, 106, 109 to 111, 113 to 118) in the 6 files of form version 69, 14 (the same without 118) in the 2 of 68 |
| Change-form table | count entries (below) | 55,319 entries |
| Global table 3 | same as table 1 | count 8 in 8 files, types 1000 to 1007 |
| Form id array | u32 n; n u32 | 62,866 entries |
| Second array | u32 n; n u32 | n is 1 in 6 files and 0 in 2 |
| Closing block | the bytes from its declared offset to the end of the file | 8 bytes in 8 files: u32 4, u32 0 **[open]** |

The layout as a whole is rule `save_container_layout_v1`; the roles of the
two plugin lists are rule `save_plugin_lists_v1`.

Within each global table the types are ascending and distinct in every
file. The global table counts are exact: reading count entries of table 3
lands on the form id array's declared offset in 8 files. The lengths per
type are in the census; no body is interpreted **[open]**.

## Change forms

**Entry.** 3 bytes *reference*; u32 *flags*; u8 *type byte*; u8 *version*;
then two lengths whose width the top two bits of the type byte select
(0: u8, 1: u16, 2: u32); then *length1* stored bytes. Width code 3 occurs
in no file and is refused (`UnmeasuredChangeFormWidth`). Rule
`save_change_form_entry_v1`.

| Check | Holds | Fails |
| --- | ---: | ---: |
| entries end exactly at global table 3's declared offset | 8 files | 0 |
| width code is the smallest that holds both lengths | 55,319 | 0 |
| a reference appears once per file | 55,319 | 0 |
| second length not 0: the payload is an exact zlib stream whose output length is the second length | 17,305 | 0 |
| second length not 0: stored length below output length | 17,305 | 0 |
| second length 0: the payload does not start with the zlib header byte 0x78 | 38,014 | 0 |

Width codes: 0 in 39,432 entries, 1 in 15,887, 2 in none. Compressed
payloads: 4,013,986 stored bytes inflate to 8,472,459. Rule
`save_change_form_zlib_v1`.

**Version byte.** 69 in all 40,298 entries of the form-version-69 files;
in the form-version-68 files 68 in 12,091 entries and 69 in 2,930
**[open]**: which entries keep 69 is not explained.

**Reference.** The three bytes read as a big-endian 24-bit word: the top
two bits a kind, the low 22 bits a value. Kind 0: a 1-based index into
the form id array; kind 1: the value is a runtime form id with top byte 0;
kind 2: the value with top byte `0xFF`. Kind 3 occurs in no entry. Rule
`save_ref_id_kinds_v1`. Kinds: 13,376 kind 0, 40,570 kind 1, 1,373 kind 2.
Every kind-0 index lands inside the array and on a non-zero entry
(13,376 of 13,376). Runtime form ids are mapped to the plugin data through
the save's plugin lists (rule `save_form_id_array_v1`, below).

Alternative readings of the three bytes against the same plugin data
(entries naming a winning record; of those, with the same record type as
the candidate):

| Reading | Name a winning record | Same record type |
| --- | ---: | ---: |
| big-endian, 1-based index (candidate) | 53,466 | 53,466 |
| big-endian, 0-based index | 53,431 | 50,919 |
| little-endian, 1-based index | 4,936 | 2,259 |
| little-endian, 0-based index | 4,936 | 2,259 |

Under the candidate, of 55,319 entries: 53,466 name a winning record,
1,373 a created form (`0xFF`), 472 a light-plugin slot (`0xFE`) whose plugin
is not installed, and 8 (one per file, all of type 1 and reference kind 1;
the payload is the largest of its file in 7 of the 8) name an id with no
winning record in the plugin data **[open]**.

**Type.** The low six bits of the type byte. Each type names one record
type under the candidate reference reading, with no exception among the
entries that name a winning record (rule `save_change_form_type_record_v1`):

| Type | Entries | Record type named | Other targets |
| ---: | ---: | --- | --- |
| 0 | 31,765 | REFR 30,404 | created 1,196; light slot 165 |
| 1 | 14,962 | ACHR 14,523 | light slot 255; created 176; no winning record 8 |
| 4 | 1 | none | created 1 |
| 6 | 437 | CELL 437 | |
| 7 | 2,736 | INFO 2,735 | light slot 1 |
| 8 | 3,807 | QUST 3,771 | light slot 36 |
| 9 | 638 | NPC_ 638 | |
| 28 | 12 | ECZN 12 | |
| 30 | 128 | FACT 128 | |
| 35 | 18 | SMQN 18 | |
| 36 | 393 | SCEN 391 | light slot 2 |
| 39 | 76 | PHZD 75 | light slot 1 |
| 42 | 270 | FLST 270 | |
| 43 | 12 | none | light slot 12 |
| 44 | 40 | LVLI 40 | |
| 49 | 24 | INNR 24 | |

Type 7 entries have length 0 in all 2,736. Stored lengths, compression and
flag-bit histograms by type are in the census. The flag bits are counted,
not named **[open]**.

**Moved references.** For types 0 and 1 with flag bit 1 set (1,274 and
7,648 entries), the payload is read from byte 0 as a reference (three bytes,
same encoding), three f32 and three f32 (rule `save_refr_move_layout_v1`):

| Check | Holds | Fails |
| --- | ---: | ---: |
| payload at least 27 bytes | 8,922 | 0 |
| the first three f32 finite, below 2^20 in magnitude | 8,922 | 0 |
| the next three f32 finite, at most 2 pi + 0.01 in magnitude | 8,922 | 0 |

The reference names a CELL in 892 type-0 and 3,316 type-1 entries, a WRLD
in 382 and 4,291, a created form in 11 type-1 entries and a light slot of
a plugin not installed in 30. Against the plugin data, where the change
form names a placed record of the plugins (78 REFR, 7,424 ACHR; the other
REFR are created forms):

- REFR: the plugin cell is exterior in 78 of 78 and the reference names its
  world in 78; the first three f32 are at distance 0 from the plugin's
  DATA position in 40, below 1 in 24, below 128 in 14; divided by 4096 they
  fall in the plugin cell's grid square in 72 and 5 or more squares away
  in 6.
- ACHR: plugin cell interior (or outside a world group) in 3,435: the
  reference names that cell in 3,123 and another form in 312; exterior in
  3,989: the reference names its world in 3,918 and another form in 71.
  Distance to the DATA position: 0 in 377, below 1 in 54, below 128 in
  2,392, below 4096 in 4,202, 4096 or more in 399 (actors move).

Nothing else of a payload is read.

## Plugin lists and the form id array

| Check | Holds | Fails |
| --- | ---: | ---: |
| first-list name at the same index of the manifest's load order | 56 | 0 |
| first-list name is a file in the data folder | 56 | 0 |
| first-list name ends `.esm` | 56 | 0 |
| second-list name ends `.esl` | 54 | 0 |
| second-list name is a file in the data folder | 0 | 54 |
| second list empty iff form version 68 | 8 | 0 |

The nine second-list names of the form-version-69 files are not installed:
the installation was moved to the declared build after those files were
written, so every id they lead to stays unresolved here.

**Form id array.** Each word is read as a runtime form id: top byte below
the first list's length selects that plugin, `0xFE` a slot of the second
list (bits 12 to 23, as `fo4_plugin::LoadOrder::runtime_id` builds it), the
low 24 bits the object (rule `save_form_id_array_v1`). Of 62,866 entries:
60,405 name a winning record of the plugin data (56 record types; REFR
28,034, INFO 4,269, GLOB 4,096, QUST 3,760, ACHR 3,520 lead), 1,731 a light
slot of a plugin not installed (all in form-version-69 files), 730 are zero
(all in form-version-68 files, 365 in each) and none fails to resolve
otherwise. Non-zero entries are distinct within a file (62,136 of 62,136).
13,376 entries are indexed by a change form. Why the form-version-68 files
hold zero words where the others hold light-slot ids is **[open]**.

**Second array.** One word in 6 files, none in 2; the word names a WRLD in
6 of 6. Its role is not claimed **[open]**.

## Header

Fields by position, with the candidate meaning of rule
`save_header_fields_v1`: word 1 save number; text 1 character name (0 to 12
bytes); word 3 level (at least 1 in 8); text 2 location name (9 to 15
bytes); text 3 play time (33 to 35 bytes, begins
`<digits>d.<digits>h.<digits>m.` in 8); text 4 race editor id (ASCII
letters and digits in 8); u16 sex (0 or 1 in 8); two f32 experience and
experience for the next level (finite, at least 0, the second larger in
8); u64 a Windows file time (within 10 s of the file's modification time
in 8 of 8); width and height of the screenshot (measured by framing). The
screenshot's channel order is rule `save_screenshot_rgba_v1`; the image is
never decoded or written by this project.

## Open

- The bodies of all 35 global data types (23,104,213 bytes), above all
  table 3 type 1001 (22,859,359 bytes, 55 % of the corpus).
- Every change-form payload beyond the moved-reference prefix, and the
  meaning of every flag bit.
- The closing block (u32 4, u32 0 in every file).
- Change-form version 69 entries in form-version-68 files.
- The 8 type-1 entries naming an id with no winning record.
- Zero words in the form id array of the form-version-68 files.
- The second array's role.
- Change-form types 4 and 43 name no record of the installed plugins.

## Prior knowledge used as hypotheses

Recognised from general knowledge before measuring, and kept only where the
census holds (also listed in [provenance](../docs/provenance.md)):

- the magic, the header size word and the order and meaning of the header
  fields, with the screenshot dimensions at its end;
- the screenshot as width x height x 4 bytes in RGBA order;
- the form version byte and the game version text after the screenshot;
- the plugin block: size word, u8 count of names, then a u16 count of light
  plugin names;
- the location table: six offsets in the order form id array, a table at
  the end, global tables 1 and 2, change forms, global table 3, then four
  counts and fifteen unused words;
- global data entries as type and length words, with types numbered from 0,
  100 and 1000 in the three tables;
- the change-form entry: three-byte reference, flag word, type byte whose
  top two bits select the length width, version byte, two lengths, zlib
  when the second is not zero;
- the three-byte reference: top two bits select an index into the form id
  array, a form of the first master or a created form;
- the form id array and an array of worldspaces after global table 3, and
  a further table at the end of the file;
- the numbering of change-form types against record types (type 0 placed
  references, type 1 placed actors and so on);
- flag bit 1 of a placed reference as a move, with the cell or world,
  position and rotation at the start of its payload.

Two recalled details were refuted or not used: the count of global table 3
was recalled as possibly one short of the entries stored, but it is exact
in 8 files; the closing table was recalled as a count followed by entries
read from its offset, which does not fit the measured 8 bytes (its first
word is 4 and the block is kept raw). Whether the index of kind-0
references starts at 0 or 1 was not known and was decided by the census.
