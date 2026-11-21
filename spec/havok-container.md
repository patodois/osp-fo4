# Havok container in Fallout 4 collision payloads

Second step after [collision.md](collision.md): the container level of the
payload that `bhkPhysicsSystem` and `bhkRagdollSystem` carry in their
ByteArray. `fo4_havok::Container::read` splits a payload into its header,
three named sections, the class-name entries and three tables at the end of
the data section. It locates objects and names their class; it decodes no
object, shape or body.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter or other reader
of this format was consulted (see [provenance](../docs/provenance.md)). The
names used for fields and tables here are this project's descriptive labels,
not names from any source.

Evidence tags:

- **[measured N]** holds for N payloads of the census below and fails for
  none unless a second count says so. A payload is one bhkSystem block, so a
  payload shared by several blocks counts once per block. N = 275,342 means
  all of them (275,154 physics and 188 ragdoll).
- **[hypothesis]** an interpretation consistent with the measurements that the
  stored bytes cannot decide. No code relies on it. These are readings of the
  format, not runtime behaviour, so they are not rows of
  [hypotheses.tsv](hypotheses.tsv); the one runtime rule involved is cited by
  name.
- **[open]** not established.

## Census scope

`fo4-unpack sweep --scope havok --manifest PROFILE --out DIR` reads every
winning NIF of the VFS, runs `fo4_nif::Nif::collision` and then
`fo4_havok::Container::read` on each payload, and writes `havok.tsv` beside
the usual reports. The committed copy is
[havok-container-census.tsv](havok-container-census.tsv): counts, small stored
header values, class names and their 4-byte signatures only. Its `census`
rows identify the same input as [collision.md](collision.md): build declared
as 1.10.163, 7 plugins, 62 archive mounts, VFS fingerprint `a4316448…bc100`;
224,368 winning `.nif` paths, 224,366 read (the two failures are the
`UnsupportedNifBsVersion` files of the collision census).

- Rows `havok.<kind>.check` give `holds` and `fails` for each property the
  reader does not enforce. Everything the reader enforces is counted by
  `havok.<kind>.read`: a payload that breaks one of those rules is a named
  error, counted under `havok.<kind>.error`.
- Run on the 16 GB Mac: 91.1 s wall, peak RSS 522 MB (`/usr/bin/time -l`),
  archives read entry by entry through the VFS.

## Result

`Container::read` accepts every payload: 275,154 of 275,154 physics and 188
of 188 ragdoll, no error **[measured 275,342]**. So each rule listed below as
enforced holds for all of them.

## Byte order and pointer slot (question 6 of collision.md)

- Little-endian. Every u32 the reader uses is read little-endian, and the
  bounds it checks (section starts and ends, table entries inside the object
  bytes, class-name offsets on a name, object-pointer destinations on an
  object) hold for every payload **[measured 275,342]**. Read big-endian, at
  least one of the three section start words exceeds the payload length in
  every payload (`record_starts_read_big_endian_in_bounds` holds 0 times)
  **[measured 275,342]**.
- Eight-byte slots. At every source of the three tables the eight stored
  bytes are zero; the sources of the first and second tables are multiples of
  8, distinct, disjoint between the two tables and at least 8 bytes apart
  **[measured 275,342]**. Header byte 16 is 8 in every payload
  **[measured 275,342]**. That the slots are 8-byte pointers written at load
  time, and that byte 16 declares that size, is **[hypothesis]**.

## Header: bytes 0 to 63 (question 1)

All sixteen u32 words are constant over the corpus **[measured 275,342]**, so
the census cannot tell a count from a constant. What the words relate to:

| Bytes | Value (u32 LE) | Measured relation | Reader |
| --- | --- | --- | --- |
| 0 to 7 | `57e0e057`, `10c0c010` | constant | required (`InvalidHavokMagic`) |
| 8 | 0 | none found **[open]** | kept |
| 12 | 11 | none found **[open]** | kept |
| 16 to 19 | bytes `08 01 00 01` | byte 16 equals the slot size above; bytes 17 to 19 **[open]** | required (`UnsupportedHavokLayout`) |
| 20 | 3 | equals the number of 64-byte named records that follow | required (`UnsupportedHavokSectionCount`) |
| 24 | 2 | index of `__data__` among the records; same value as the section field of every second-table entry | required |
| 28 | 0 | offset in `__data__` of an object of the third table, always the first object | required |
| 32 | 0 | index of `__classnames__`; same value as the section field of every third-table entry | required |
| 36 | 75 | offset in `__classnames__` of the class name of the object at word 28 (the fifth name) | required (`InvalidHavokRoot`) |
| 40 to 55 | `hk_2014.1.0-r1`, NUL, `0xff` | constant text | required (`UnsupportedHavokVersion`) |
| 56 | 0 | none found **[open]** | kept |
| 60 | 21 | none found **[open]** | kept |

- Word 20 being a record count, and words 24 to 36 being (section, offset)
  pairs that name the top-level object and its class, fit every payload
  **[measured 275,342]** but rest on constant values **[hypothesis]**. The
  object they name is `hknpPhysicsSystemData` in every physics payload and
  `hknpRagdollData` in every ragdoll payload **[measured 275,342]**.

## Section records: bytes 64 to 255 (question 2)

Three records of 64 bytes at 64, 128 and 192 **[measured 275,342]**:

| Record bytes | Content |
| --- | --- |
| 0 to 18 | NUL-terminated name, then zero bytes (`section_record_name_padding_zero`) |
| 19 | `0xff` |
| 20 | start: payload offset of the section's first byte |
| 24 to 44 | six offsets relative to the start, non-decreasing; the sixth is the section length |
| 48 to 63 | all `0xff` |

Byte 19 and bytes 48 to 63 are `0xff` in every record
(`section_records_byte19_and_48_to_63_are_ff`) **[measured 275,342]**; the
reader leaves them in the borrowed bytes without requiring them.

Enforced, each **[measured 275,342]**:

- names `__classnames__`, `__types__`, `__data__` in that order
  (`InvalidHavokSectionRecord`);
- the first start is 256 (64 + 3 × 64), each next start is the previous
  start plus its length, and the last section ends at the payload length:
  the sections are contiguous and cover the payload
  (`InvalidHavokSectionBounds`);
- the six offsets are non-decreasing (`InvalidHavokSectionBounds`);
- `__classnames__`: the six offsets are equal, all the section length;
- `__types__`: the six offsets are zero, so the section is empty;
- `__data__`: offsets 4, 5 and 6 (words at 228, 232, 236) are equal. Offset 1
  ends the object bytes and starts the first table, offset 2 starts the
  second table, offset 3 starts the third, and offset 4 ends it. Any other
  shape is `UnsupportedHavokSectionLayout`.

Not enforced: every start and offset is a multiple of 16
(`section_starts_and_offsets_multiple_of_16`) **[measured 275,342]**.

This answers the two remaining parts of question 2: record word 20 (84 for
`__classnames__`) is an absolute payload offset and the six words after it are
relative to it, and the words
at 228/232/236 are equal because `__data__` has nothing after its third
table. Whether offsets 4 and 5 would bound two more tables in other data is
**[hypothesis]**; nothing here uses them.

Question 3 of collision.md: `__types__` is empty everywhere, and every object
of `__data__` is named through `__classnames__` (third table below)
**[measured 275,342]**.

## `__classnames__` (question 4)

From the section start, entries follow each other with no gap
**[measured 275,342]**:

| Entry bytes | Content |
| --- | --- |
| 0 to 3 | u32 LE signature |
| 4 | marker byte, 9 in all 2,687,655 entries (`InvalidHavokClassName` otherwise) |
| 5 | NUL-terminated printable ASCII name |

- After the last entry the section is filled with `0xff` to its end
  (enforced). The fill is 0 to 15 bytes, so the section ends at the next
  multiple of 16 (`classnames_fill_below_16`) **[measured 275,342]**.
- The header word 36 and the third table refer to a class by the offset of the
  first character of its name, not of its entry (enforced: the offset must be
  a name start).
- Each name appears once per payload (`class_names_distinct`), and each name
  carries one signature over the whole corpus. Two names share a signature:
  `hknpStaticCompoundShape` and `hknpDynamicCompoundShape` (both `4620d11c`)
  **[measured 275,342]**. So the signature does not identify a name; what it
  is computed from is **[open]**.
- The first four entries are always `hkClass`, `hkClassMember`, `hkClassEnum`
  and `hkClassEnumItem` (at 261, 274, 293, 310 in collision.md), and no object
  of the third table has one of these classes
  (`first_four_class_names_never_objects`). The fifth entry is always the
  class of the object named by the header (`root_class_is_fifth_class_name`)
  **[measured 275,342]**.

29 distinct names occur. Per name, the committed TSV gives the payloads that
list it, the payloads with at least one object of it, the number of objects
and the signature. The 27 physics names, by payloads listing them:

| Name | Physics payloads | Objects |
| --- | ---: | ---: |
| `hkClass`, `hkClassMember`, `hkClassEnum`, `hkClassEnumItem` | 275,154 | 0 |
| `hknpPhysicsSystemData` | 275,154 | 275,154 |
| `hkRefCountedProperties` | 274,272 | 386,390 |
| `hknpCompressedMeshShape` | 154,809 | 167,916 |
| `hknpCompressedMeshShapeData` | 154,809 | 167,916 |
| `hknpBSMaterialProperties` | 154,809 | 167,916 |
| `hknpShapeMassProperties` | 142,263 | 218,474 |
| `hknpConvexPolytopeShape` | 142,160 | 218,364 |
| `hknpScaledConvexShape` | 94,063 | 99,438 |
| `hknpStaticCompoundShape` | 86,806 | 87,314 |
| `hknpStaticCompoundShapeData` | 86,806 | 87,314 |
| `hknpDynamicCompoundShape` | 8,540 | 8,905 |
| `hknpDynamicCompoundShapeData` | 8,540 | 8,905 |
| `hknpCapsuleShape` | 1,380 | 2,895 |
| `hkpPositionConstraintMotor` | 170 | 170 |
| `hknpSphereShape` | 166 | 189 |
| `hkpRagdollConstraintData` | 134 | 286 |
| `hkpLimitedHingeConstraintData` | 114 | 235 |
| `hknpConvexShape` | 110 | 110 |
| `hkpHingeConstraintData` | 9 | 18 |
| `hkpBallAndSocketConstraintData` | 9 | 284 |
| `hknpBreakableConstraintData` | 5 | 8 |
| `hkpPrismaticConstraintData` | 3 | 8 |
| `hkpStiffSpringConstraintData` | 1 | 2 |

The 188 ragdoll payloads list 17 names: the four `hkClass*` names,
`hknpRagdollData`, `hkaSkeleton` and `hkpPositionConstraintMotor` (all 188),
`hknpCapsuleShape` (183), `hkpRagdollConstraintData` (135),
`hkpLimitedHingeConstraintData` (117), `hknpSphereShape` (41),
`hkRefCountedProperties`, `hknpConvexPolytopeShape` and
`hknpShapeMassProperties` (31 each), `hknpDynamicCompoundShape` and its
`Data` (10 each) and `hknpBreakableConstraintData` (2). That a name is a C++
class and the object bytes are instances of it is **[hypothesis]**.

## `__data__`: object bytes and three tables (question 5)

Offsets in all three tables are relative to the `__data__` start, and every
source and destination lies inside the object bytes (before offset 1), with
room for an 8-byte slot at each source (enforced, `InvalidHavokFixup`)
**[measured 275,342]**. Each table is a run of fixed-size entries followed by
`0xff` up to its end offset (enforced, `InvalidHavokFixupTable`); a source is
never `0xffffffff` because it lies inside the object bytes.

### First table: (source, destination), 8 bytes

- 3,739,599 entries (3,737,598 physics, 2,001 ragdoll), up to 4,239 in one
  payload.
- Source a multiple of 8, destination a multiple of 16, sources distinct
  **[measured 275,342]**.
- The destination is never the offset of an object of the third table
  (`pointer_destination_not_an_object_offset`) **[measured 275,342]**.
- The source equals an object's offset only for `hkRefCountedProperties`
  objects, and does so for all 386,485 of them (386,390 physics, 95 ragdoll)
  **[measured 275,342]**.
- Sources are not sorted in 154,809 physics payloads, exactly those that list
  `hknpCompressedMeshShape`
  (`pointer_sources_increasing_iff_no_hknpCompressedMeshShape_listed`)
  **[measured 275,342]**.
- Fill: 0 or 8 bytes **[measured 275,342]**.

### Second table: (source, section, destination), 12 bytes

- 1,963,080 entries (1,955,242 physics, 7,838 ragdoll), up to 824 in one
  payload. Section is 2 (`__data__`) in every entry, and the destination is
  the offset of an object of the third table in every entry (enforced)
  **[measured 275,342]**.
- Sources: multiples of 8, distinct, never an object offset, never a
  first-table source **[measured 275,342]**; not sorted in 95,320 physics and
  all 188 ragdoll payloads.
- Fill: 0, 4, 8 or 12 bytes in 275,336 payloads; 24, 36, 76, 120, 144 and
  148 bytes in one physics payload each (`object_pointer_fill_below_16` fails
  6). The rule that sets the fill is **[open]**.
- Destination classes are counted in `object_pointer_target` rows.

### Third table: (offset, section, class-name offset), 12 bytes

- 1,901,884 entries (1,898,211 physics, 3,673 ragdoll), up to 619 in one
  payload. Section is 0 (`__classnames__`) and the class-name offset is the
  start of a name in every entry; offsets strictly increase (enforced)
  **[measured 275,342]**.
- Offsets are multiples of 16; the first entry is the header's top-level
  object at offset 0 (`root_is_first_object`) **[measured 275,342]**.
- Fill: 0, 4, 8 or 12 bytes **[measured 275,342]**.

Interpretation, **[hypothesis]** throughout: the first table lists slots that
point inside the object bytes (array or string storage), the second slots
that point to whole objects, the third where each object starts and its
class. Where an object ends, and which bytes between objects belong to which
object, is measured in [havok-objects.md](havok-objects.md): each object owns
the bytes up to the next one, and the reader now enforces that every
first-table pointer stays inside its object.

## Objects on a system and Body ID (question 7)

This one measurement looks inside an object, so it stays in the census; the
reader does not interpret it. Take the bytes of the top-level object from
offset 16 in 16-byte groups. For group g at offset 16 + 16g:

- the u32 at +12 is `0x80000000` OR the u32 at +8, in groups 0 to 5 of every
  physics and ragdoll payload, and in group 7 of every ragdoll payload
  **[measured 275,342]**;
- the group's first eight bytes are a first-table source exactly when the
  u32 at +8 is not zero, in groups 0 to 7 **[measured 275,342]**.

With k the number of `bhkNPCollisionObject` whose `Data` is the system:

In the TSV, `root_group` keys give the group offset and
`root_u32_eq_system_objects` keys give the offset of the compared u32 (group
offset + 8).

| Root class | Group offset | u32 at +8 equals k |
| --- | ---: | --- |
| `hknpPhysicsSystemData` | 16 | 275,151 of 275,154 |
| `hknpPhysicsSystemData` | 64 | 275,151 of 275,154 |
| `hknpPhysicsSystemData` | 96 | 275,151 of 275,154 |
| `hknpRagdollData` | 16 | 188 of 188 |
| `hknpRagdollData` | 48 | 188 of 188 |
| `hknpRagdollData` | 64 | 188 of 188 |
| `hknpRagdollData` | 96 | 188 of 188 |

Other groups match far less often (`root_u32_eq_system_objects` rows). The
three physics exceptions are the same payload bytes used by three blocks of
two static-collection models: they store 2 in those groups and each block has
one collision object, Body ID 0. So the stored count matches the number of
collision objects in 275,339 of 275,342 systems **[measured 275,342]**.

That a group is an array of k bodies, and that Body ID i selects element i,
stays **[hypothesis `nif_collision_body_id_selects_body_v1`]**; the three
exceptions show that a system can store more entries than it has objects.
How many objects of each class a payload holds against k is in the
`count_eq_system_objects` field of the `class` rows; no class count equals k
in every payload.

## Status of the open questions of collision.md

| collision.md | Status |
| --- | --- |
| 1. Header words | Relations above; word 20 as a count and the meanings of words 8, 12, 56, 60 and bytes 17 to 19 **[open]** |
| 2. Record words | Start absolute, six offsets relative, last is the length **[measured 275,342]** |
| 3. Empty `__types__` | Objects are named through `__classnames__` and the third table **[measured 275,342]** |
| 4. Class-name entries | Signature, marker 9, name; signature meaning **[open]** |
| 5. Locating objects | Third table; pointers in the first two tables; object extents in [havok-objects.md](havok-objects.md) |
| 6. Byte order | Little-endian, 8-byte slots **[measured 275,342]** |
| 7. Bodies and Body ID | Count matches in 275,339 of 275,342; selection **[hypothesis]** |

Questions 9 and 10 of collision.md (flags, walking) are untouched; question
8 is begun in [havok-objects.md](havok-objects.md).

## Reader errors

All carry the NIF file name and the absolute file offset (`base` passed to
`Container::read` plus the payload offset of the field):
`TruncatedHavokContainer`, `LimitExceeded` (payload over 4 GiB, or the
`Budget`), `InvalidHavokMagic`, `UnsupportedHavokLayout`,
`UnsupportedHavokVersion`, `UnsupportedHavokSectionCount`,
`InvalidHavokSectionRecord`, `InvalidHavokSectionBounds`,
`UnsupportedHavokSectionLayout`, `InvalidHavokClassName`,
`InvalidHavokFixupTable`, `InvalidHavokFixup`, `InvalidHavokRoot`, and
`InvalidHavokObjectSpan` from [havok-objects.md](havok-objects.md). None occurs
in the census. The synthetic tests cover each error, every truncation and
every single-byte change of a generated container.
