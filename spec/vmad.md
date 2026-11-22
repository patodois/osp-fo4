# VMAD: scripts attached to records

The VMAD subrecord names the compiled scripts ([pex.md](pex.md)) attached to
a record and the values of their properties. `fo4_plugin::vmad::Vmad` reads
its script list for every record type, for TERM the fragment block that
follows, and for QUST the block of stage fragments and aliases; for the
other record types the bytes after the script list are kept as stored
(`tail`). Nothing here runs a script or applies a property.

Every statement below comes from measuring the VMAD subrecords of the
winning records of one installation. The layout was recognised from prior
general knowledge before it was measured (see
[provenance](../docs/provenance.md)); it is kept only where the census
below, which could have refuted it, holds. No editor, editor source or
definition file, format documentation, wiki page or third-party reader of
this subrecord was opened or searched for. Field names are this project's
labels. Evidence tags as in [pex.md](pex.md).

## Census scope

`vmad_census MANIFEST OUT.txt` (an example of `fo4-unpack`) reads every
winning, not deleted record of the manifest's plugins, reads each VMAD with
`Vmad::read`, and cross-checks script and property names against every
`.pex` of the archives read with `fo4_pex`. The output, counts only, is
committed as [vmad-census.txt](vmad-census.txt). Same installation as
[pex.md](pex.md): 20,147 records hold one VMAD each (never two), 4,403,029
bytes, on 26 record types; 8.1 s wall, peak RSS about 850 MB.

**Result: every VMAD is read without error** for all 26 types. The script
list and the TERM and QUST blocks cover 3,859,236 bytes. The other 543,793
bytes are the blocks after the script list of INFO (2,396 of 5,611
records), PACK (166 of 167), PERK (32 of 35) and SCEN (632 of 634), which
are kept, not read **[open]**. Every other VMAD ends exactly where its
script list (or TERM or QUST block) ends, including all 8,767 REFR, 931
ACTI, 301 TERM, 19 DOOR and 1,353 QUST records.

## Layout

Little-endian; a *text* is a u16 byte count and the bytes.

| Field | Content | Evidence |
| --- | --- | --- |
| u16 | 6 (`VERSION`) | 20,146; one QUST has 5 and reads the same way |
| u16 | 2 (`OBJECT_FORMAT`; another value is refused) | 20,147 |
| u16 | script count | 0 in 3,813 VMADs, 1 in 14,590, 2 in 1,550, 3 in 125, 4 in 44, 5 or more in 25 |
| scripts | text *name*, u8 *flags*, u16 property count, properties | 18,385 scripts; flags 0 (14,052), 1 (4,188), 3 (145) |

The script name is the object name of a `.pex` of the archives, compared
without case, for 18,384 of 18,385 (and 290 of 294 TERM block scripts).

**Property.** text *name*, u8 *kind*, u8 *flags* (1 in 42,916 of 42,960
properties, 3 in 44), then the value by kind:

| Kind | Value | Properties |
| ---: | --- | ---: |
| 0 | nothing | 1 |
| 1 | object: u16 (0 in all 36,123 object values), i16 (-1 in 32,220; non-negative in 3,903, 3,375 of them on QUST), u32 FormID | 23,002 |
| 2 | text | 695 |
| 3 | i32 | 3,872 |
| 4 | f32 | 799 |
| 5 | u8, 0 or 1 | 12,315 |
| 7 | struct: u32 member count, members laid out as properties | 36 |
| 11 to 15 | array: u32 count, values of kind 1 to 5 | 1,333; 108; 21; 78; 0 |
| 17 | array of structs: u32 count, each as kind 7 | 700 |

Other kinds occur in no VMAD and are refused. Struct members (8,879) have
kinds 1 to 5 only, flags 1. Against the `.pex` declarations: the property
name is a property of the script or a parent for 42,119 of 42,313 list
properties (194 are not declared by the script any more) and 627 of 643
TERM block properties; and the kind fits the declared type (1 an object
type, 2 to 5 String, Int, Float, Bool, 7 a struct, 11 to 15 and 17 arrays
of those) for all 42,746 that are declared, kind 0 excepted. An object
FormID with i16 -1 and not 0 names a winning record, resolved through the
record's plugin, for 31,570 of 32,103 values; 117 values hold FormID 0.

### TERM block

After the script list of 294 of the 301 TERM VMADs (the other 7 end with
the list): u8 3; one script laid out as a list entry (flags 0 in all 294);
u16 fragment count; per fragment u32 *index* (at least 1, distinct within a
block), u8 1, text *script* (the block's script in all 893 fragments), text
*function*. The function is declared by the block's script, with one
parameter and no return value, for 882 of 893 fragments; the other 11 name
a function the script does not declare. Which terminal entry an index
selects is not read here **[open]**.

### QUST block

After the script list of 1,300 of the 1,353 QUST VMADs (the other 53 end
with the list); every one read to its last byte, none refused:

| Field | Content | Evidence |
| --- | --- | --- |
| u8 | 3 | 1,300 |
| u16 | fragment count | 0 in 21 blocks |
| text | the fragment script; when not empty, followed by u8 *flags* and the properties of a script entry; when empty, nothing | empty in 18 blocks, all with no fragment; not empty in 1,282 (3 of them with no fragment), flags 0 in all |
| fragments | u16 *stage*, u16 (0 in all), i32 *log entry*, u8 (1 in all), text *script*, text *function* | 10,506 fragments; the script is the block's in all; the function is declared by it, with no parameter and no return value, for 10,500 (6 not declared); the stage is the first u16 of an INDX subrecord of the quest for 10,375 (131 not); 130 blocks have two fragments of one stage |
| u16 | alias count | 3,226 aliases |
| aliases | an object value (u16 0, i16 *alias index*, u32 the quest's own FormID), u16 6, u16 2, then a script list | index not negative and FormID the quest's own in all 3,226; indices distinct within every block; the index is the value of an ALST subrecord of the quest for 3,216 (10 not); 1 to 5 scripts each |

The fragment block script names a `.pex` object for 1,279 of 1,282, and
its 14,575 properties fit their declared types (55 not declared); the
alias scripts name a `.pex` object in all 4,057 cases, with all 7,796
properties declared and fitting. Which stage or event runs a fragment is
not read here **[open]**.

### Object values with an alias index

An object value whose i16 is not negative (9,756 values, 9,228 of them on
QUST records) names a winning QUST in all 9,756; the quest's ALST values
hold the index in 9,362. The declared type of the property holding it is
ReferenceAlias (7,536 properties), RefCollectionAlias (816), LocationAlias
(178) or Alias (7), never an object reference type. The quest's VMAD block
has an alias entry of that index for 3,043 (aliases without scripts have
no entry). `fo4_vm` reads such a value as the alias of that index of that
quest (`vmad_alias_value_v1`).

## Survey

`script_survey MANIFEST OUT_DIR CELL[:RADIUS]... | ALL` (an example of
`fo4-unpack`) takes the placed references of the given cells, follows NAME
to DOOR, TERM and ACTI bases, collects the scripts of the base VMAD, the
reference VMAD and the TERM block, and follows every call that resolves
(`fo4_pex::index`) from all functions of those scripts and of their parents
up to the first script that declares a native function. It writes the
names to a file that is not versioned; the counts are below.

| Scope | Placements or bases | Scripted | Distinct scripts | Reached functions (scripts) | Native call sites | Distinct natives (called directly) | Unresolved calls |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| The two test cells (one interior; one exterior with radius 1, 9 cells) | 14 DOOR, 2 TERM, 21 ACTI placed | 0 DOOR, 0 TERM, 10 ACTI | 6 | 62 (14) | 90 | 42 (28) | 0 |
| Every winning DOOR, TERM and ACTI base | 442 DOOR, 1,069 TERM, 2,195 ACTI | 19, 301, 931 | 759 | 3,174 (859) | 7,988 | 351 (299) | 0 |

These are static reaches of the stored code, not what runs: events,
states and conditions decide that at run time.
