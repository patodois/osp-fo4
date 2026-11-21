# Havok objects in Fallout 4 collision payloads

Third step after [collision.md](collision.md) and
[havok-container.md](havok-container.md): where each object of the third
table ends, which bytes between objects belong to which object, and the
stored layout of `hknpConvexPolytopeShape`, the most common shape.
`fo4_havok::Container` now enforces that every first-table pointer stays
inside the object that holds its source and gives each object's span;
`fo4_havok::ConvexPolytope` reads one polytope.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter or other reader
of this format was consulted (see [provenance](../docs/provenance.md)). Field
names are this project's descriptive labels; class names are only the strings
stored in `__classnames__`.

Evidence tags, as in havok-container.md:

- **[measured N]** holds for N of the census and fails for none unless a
  second count says so. N counts payloads (275,342 = all; one per bhkSystem
  block) or objects, as stated. Objects are counted once per block, like the
  `objects` field of the `class` rows.
- **[hypothesis]** an interpretation the stored bytes cannot decide. No code
  relies on it.
- **[open]** not established.

## Census scope

Same input and command as havok-container.md (`fo4-unpack sweep --scope havok`;
build declared as 1.10.163, 7 plugins, 62 archive mounts, VFS fingerprint
`a4316448…bc100`). The committed
[havok-container-census.tsv](havok-container-census.tsv) gains these rows:

- `havok.<kind>.check` rows for the new payload-level rules;
- `object_check` rows, keyed `class|rule`, counting objects;
- `object_lead` rows, keyed `class|leading size` (or `none`);
- `span_min`, `span_max` and `span_distinct` in each `class` row (more than
  1,024 distinct values print as `1024+`);
- `convex_polytope` (reads and error codes), `convex_polytope_u16_16_18`,
  `convex_polytope_link32`, `convex_polytope_padding_rows` and
  `max_per_object` rows.

Run on the 16 GB Mac: 92.0 s wall, peak RSS 541 MB (`/usr/bin/time -l`). The
exit status is 1 because of the two `UnsupportedNifBsVersion` files already
reported by the collision census; every payload reads.

## Object spans

The span of object i of the third table is the bytes from its offset to the
offset of object i + 1, or to the end of the object bytes (offset 1 of
`__data__`) for the last one. The first object is at offset 0 in every
payload, so the spans cover the object bytes with no gap.

| Rule | Result | Reader |
| --- | --- | --- |
| The destination of every first-table pointer lies in the span of the object that holds its source | 3,739,599 pointers, 0 fail **[measured 275,342 payloads]** | enforced, `InvalidHavokObjectSpan` |
| Every second-table source lies in an object's span | **[measured 275,342 payloads]** | enforced, `InvalidHavokObjectSpan` |
| Every first-table destination is after its source (`pointer_destination_after_source`) | **[measured 275,342 payloads]** | counted |
| First-table destinations are distinct (`pointer_destinations_distinct`) | **[measured 275,342 payloads]** | counted |
| At every first-table source, the u32 at +8 is a non-zero n and the u32 at +12 is `0x80000000` OR n (`pointer_source_u32_8_nonzero_and_u32_12_is_0x80000000_or_it`) | **[measured 275,342 payloads]** | counted |
| Within an object, destinations are in the order of their sources (`destinations_follow_source_order`) | holds for every object of 24 classes; fails for all 167,916 `hknpCompressedMeshShapeData` objects **[measured, per object]** | counted |
| Each class with first-table destinations has one leading size | 11 of 11 classes **[measured, per object]** | counted |

So the bytes between two objects, which the first table points into, belong
to the object before them. That an object is a fixed-size record followed by
the storage of its arrays, and that the slot at a source (8 zero bytes, then n
and `0x80000000 | n`) is an array header with n elements, is
**[hypothesis]**: it fits every pointer, but the census measures no element
size. The rule in the second-to-last row ties the unsorted first table (see
havok-container.md) to that one class.

### Per class

The leading size is the smallest first-table destination owned by the object
minus its offset: where the first storage block starts. Objects of the other
classes own no first-table destination (`none`). Counts are physics plus
ragdoll objects **[measured]**:

| Class | Objects | Leading size | Span (bytes) |
| --- | ---: | ---: | --- |
| `hkRefCountedProperties` | 386,485 | 16 | 32 |
| `hknpPhysicsSystemData` | 275,154 | 128 | 320 to 74,288, 133 values |
| `hknpBSMaterialProperties` | 167,916 | 32 | 64 to 1,024, 38 values |
| `hknpCompressedMeshShape` | 167,916 | 160 | 192 to 202,896, 692 values |
| `hknpCompressedMeshShapeData` | 167,916 | 208 | 608 to 654,352, over 1,024 values |
| `hknpStaticCompoundShape` | 87,314 | 208 | 336 |
| `hknpStaticCompoundShapeData` | 87,314 | 64 | 80 |
| `hknpDynamicCompoundShape` | 8,918 | 208 | 336 to 15,568 |
| `hknpDynamicCompoundShapeData` | 8,918 | 64 | 160 to 7,776 |
| `hknpRagdollData` | 188 | 144 | 848 to 25,360, 41 values |
| `hkaSkeleton` | 188 | 144 | 288 to 4,448, 31 values |
| `hknpShapeMassProperties` | 218,569 | none | 48 |
| `hknpConvexPolytopeShape` | 218,459 | none | 240 to 15,264 (below) |
| `hknpScaledConvexShape` | 99,438 | none | 112 |
| `hknpCapsuleShape` | 4,327 | none | 432 |
| `hkpRagdollConstraintData` | 1,128 | none | 416 |
| `hkpLimitedHingeConstraintData` | 666 | none | 304 |
| `hkpPositionConstraintMotor` | 358 | none | 48 |
| `hkpBallAndSocketConstraintData` | 284 | none | 112 |
| `hknpSphereShape` | 279 | none | 128 |
| `hknpConvexShape` | 110 | none | 128 or 192 (below) |
| `hkpHingeConstraintData` | 18 | none | 224 |
| `hknpBreakableConstraintData` | 11 | none | 48 |
| `hkpPrismaticConstraintData` | 8 | none | 288 |
| `hkpStiffSpringConstraintData` | 2 | none | 112 |

The four `hkClass*` names have no objects. Of the 14 classes without
destinations, 12 have one span length; `hknpConvexPolytopeShape` and
`hknpConvexShape` vary because they keep their arrays inside the object,
addressed by offsets stored in the object rather than by the first table.
Whether a leading size or a constant span is the size of a fixed record is
**[hypothesis]**.

## `hknpConvexPolytopeShape`

Offsets are from the object's offset. The reader's rules are measured on all
218,459 objects (218,364 physics, 95 ragdoll) and enforced by
`ConvexPolytope::read`; the other rows are counted per object.

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 to 15 | zero **[measured 218,459]** | counted |
| 16, 18 | two u16: (323, 256) in 213,942 objects, (259, 256) in 4,246, (67, 256) in 268, (3, 256) in 3; meaning **[open]** | counted |
| 20 | an f32, finite and not negative **[measured 218,459]**; meaning **[open]** | counted |
| 24 | a u32 with many values; meaning **[open]** | kept |
| 28 to 47 | zero **[measured 218,459]**; 32 is a second-table source pointing to an `hkRefCountedProperties` object in every polytope (`convex_polytope_link32`) | counted |
| 48 | u16 vertex rows V, u16 offset of the rows from byte 48 (32, so rows start at 80) | enforced |
| 52 to 63 | zero **[measured 218,459]** | counted |
| 64 | u16 plane rows P, u16 offset of the rows from byte 64 | enforced |
| 68 | u16 face count F, u16 offset of the face entries from byte 68 | enforced |
| 72 | u16 index count I, u16 offset of the index bytes from byte 72 | enforced |
| 76 to 79 | zero **[measured 218,459]** | counted |
| 80 | V vertex rows of 16 bytes, then P plane rows of 16 bytes, then P face entries of 4 bytes, then I index bytes | enforced |

Enforced (`InvalidHavokConvexPolytope` unless stated), each
**[measured 218,459 objects]**:

- the four arrays follow each other with no gap from byte 80, in that order,
  and the face array holds P entries, not F;
- the span length is the end of the index bytes rounded up to 16; the bytes
  after the indices are zero (counted, `bytes_after_indices_zero`);
- V is a non-zero multiple of 4, at most 256 (largest stored: 252);
- P is F rounded up to a multiple of 4 (largest P: 484; largest I: 1,462);
- a vertex row is three finite f32 and a u32 tag whose high 24 bits are
  `0x3f0000`. The leading rows carry their own index in the low byte; the
  rows after them, 0 to 3 of them, are byte-for-byte copies of the last
  tagged row, and V is the tagged count rounded up to 4. Padding rows:
  0 in 127,081 objects, 1 in 10,860, 2 in 71,517, 3 in 9,001;
- every index byte is below the tagged vertex count;
- the class is `hknpConvexPolytopeShape` (`UnexpectedHavokClass`) and the
  object index exists (`InvalidHavokObjectSpan`).

So `ConvexPolytope` exposes the tagged vertices as `[f32; 3]` (10,109,199 in
all) and keeps the padding rows out of them. Whether the tag (read as f32,
0.5 plus a few units in the last place) has any role beyond numbering is
**[hypothesis]**.

### Planes and faces: not enforced

Face entry j is a u16 first position in the index bytes, a u8 count and a u8
whose meaning is **[open]**. The entries and plane rows after the first F are
padding: the face padding is zero in every object
(`padding_face_entries_zero`); the plane padding holds varied bytes, some of
them not finite as f32, and is not read.

| Rule (object checks) | Holds | Fails |
| --- | ---: | ---: |
| `face_first_is_running_sum_of_counts` | 218,451 | 8 |
| `face_counts_sum_to_index_count` | 218,451 | 8 |
| `face_count_at_least_3` | 218,459 | 0 |
| `plane_normals_unit_within_1e-4` (first three f32 of rows 0 to F−1) | 218,451 | 8 |
| `face_vertices_on_own_plane_rel_1e-2`: for each face j and each vertex it indexes, abs(n·v + d) ≤ 0.01 × max(1, largest coordinate) with plane row j = (n, d) | 218,451 | 8 |
| `vertices_behind_face_planes_rel_1e-3`: n·v + d ≤ 0.001 × max(1, largest coordinate) for every plane row j < F and every vertex | 218,458 | 1 |
| `every_vertex_in_some_face` | 218,292 | 167 |

The same eight objects (eight physics payloads, each used by one block) fail
the face and plane rules. Each stores four plane rows and four faces; its
vertices differ by at most 0.004 in their third coordinate, so the shape is
flat; one or two of its plane rows have a zero normal and a fourth value near
−1.8e19; its face entries do not chain. With
tighter tolerances, a separate script over the same payloads finds 56 objects
above 1e-3 on the own-plane rule and 18 above 1e-4 on the behind-planes rule.
So reading plane row j as the plane of face j, with the polytope behind every
plane, fits all but eight objects but cannot be enforced; it stays
**[hypothesis]**, and `ConvexPolytope::plane` and `face` return the stored
values without checking them.

### `hknpConvexShape`

The 110 objects store the same vertex pair at byte 48 pointing to 64, end
after the vertex rows, and tag every row with its index
(`vertex_pair_at_48_points_to_64`, `span_is_64_plus_16_per_vertex_row`,
`vertex_rows_tagged_3f0000_and_index`) **[measured 110 objects]**. There is no
reader for this class.

## Status of the open questions

| Question | Status |
| --- | --- |
| Object extents (question 5 of collision.md) | Each object owns the bytes up to the next object; pointers stay in their object **[measured 275,342]**, enforced |
| Leading size per class | One per class for the 11 classes with destinations **[measured]**; record size **[hypothesis]** |
| Array headers at pointer sources | n and `0x80000000 | n` at every source **[measured 275,342]**; element sizes **[open]** |
| Convex polytope vertices (question 8) | Layout, count and padding enforced **[measured 218,459]**; units and the space they are in relative to the Target node **[open]** |
| Convex polytope planes and faces | Consistent on 218,451 of 218,459 objects; **[hypothesis]**, not enforced |
| Bytes 16 to 27 of a polytope, fourth face byte | **[open]** |
| Other shape classes, materials, compressed meshes | Untouched **[open]** |

## Reader errors

New codes, with the NIF file name and the absolute file offset (`base` +
`__data__` start + field): `InvalidHavokObjectSpan` (from `Container::read`,
and for an object index out of range), `UnexpectedHavokClass` and
`InvalidHavokConvexPolytope`. None occurs in the census. The synthetic tests
cover each error, every shorter span of a generated polytope, and every
single-byte change of it.
