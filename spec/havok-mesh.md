# Havok compressed meshes in Fallout 4 collision payloads

Fifth step after [collision.md](collision.md),
[havok-container.md](havok-container.md), [havok-objects.md](havok-objects.md)
and [havok-bodies.md](havok-bodies.md): the stored layout of
`hknpCompressedMeshShape` and of the `hknpCompressedMeshShapeData` object it
links to, which half of all bodies end in. `fo4_havok::CompressedMesh` reads
the shape, `fo4_havok::MeshData` the data object and its sections, and
`MeshData::geometry` decodes the vertices and the four-index primitives of
every section it can.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter, decompilation
or other reader of this format was consulted (see
[provenance](../docs/provenance.md)). Field names are this project's
descriptive labels ("section", "primitive", "shared vertex", "run", "tree");
class names are only the strings stored in `__classnames__`. What the game
does with any of these values is **[hypothesis]**.

Evidence tags as in havok-container.md: **[measured N]** holds for N and fails
for none unless a second count says so; **[hypothesis]** an interpretation the
bytes cannot decide, which no code relies on; **[open]** not established.
Objects are counted once per bhkSystem block. A few statistics come from a
1-in-20 sample of the 154,809 payloads that list the class (7,740 payloads,
8,350 shape objects) explored with a throwaway script; they are tagged
**[sample]** and are not in the TSV.

## Census scope

Same input and command as havok-container.md (`fo4-unpack sweep --scope
havok`; build declared as 1.10.163, 7 plugins, 62 archive mounts, VFS
fingerprint `a4316448…bc100`). The committed
[havok-container-census.tsv](havok-container-census.tsv) gains:

- `view` rows for both classes and for `geometry` (`read`, `complete`,
  `sections_skipped`);
- `object_check` rows `hknpCompressedMeshShape|…` and
  `hknpCompressedMeshShapeData|…` for the rules below that are counted, not
  enforced;
- `total` rows `mesh_…` (primitives, vertices, placeholders, skipped
  sections), the distributions `mesh_sections`, `mesh_section_flagged`,
  `mesh_section_bytes_92_93`, `mesh_objects_with_sections_byte92_or_93`,
  `mesh_primitive_kind_sections`,
  `mesh_properties_link`, `mesh_material_link`,
  `mesh_section_without_vertices_origin_scale_bits`,
  `mesh_special_record_low_byte` and `mesh_flagged_plain_shared_reference`;
- `body_reaching_compressed_mesh`: per body whose shape links reach a
  compressed mesh, `geometry` or the reason it does not resolve;
- `mesh_render_ratio_…`: the unit comparison below.

Run on the 16 GB Mac: 102.5 s wall, peak RSS 525 MB (`/usr/bin/time -l`).
The exit status is 1 because of the two `UnsupportedNifBsVersion` files
already reported by the collision census.

## Result

| | Count |
| --- | ---: |
| `hknpCompressedMeshShape` objects read by `CompressedMesh::read` | 167,916 of 167,916 |
| `hknpCompressedMeshShapeData` objects read by `MeshData::read` | 167,916 of 167,916 |
| data objects whose `geometry` decodes every section | 161,437 |
| data objects with sections left out (byte 92 or 93 not zero) | 6,479 |
| bodies that reach a compressed mesh | 166,970 |
| of them, every reached mesh fully decoded | 160,491 (72,224 direct, 87,314 through a static compound, 953 through a dynamic compound) |
| of them, a reached mesh has sections left out | 6,479 (all direct, all from `Meshes\PreCombined` or `Meshes\SCOL` in the sample) |
| other failures | 0 |

Sections: 753,882, of which 91,696 are left out by `geometry` (82,158 with
byte 93 = 1, 9,538 more with byte 92 not zero). Primitives decoded:
44,896,212 (14,105,175 in the 161,437 complete objects, 30,791,037 in the
decoded sections of the 6,479 others) plus 840 placeholders; vertices
decoded: 49,238,136. The 6,479 incomplete objects are few but large:
they hold 68% of the decoded primitives.

## `hknpCompressedMeshShape`

Offsets from the object's offset; every row **[measured 167,916]**.

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 to 15 | zero | counted |
| 16, 17, 19 | bytes 4, 2, 2 | counted |
| 18 | a byte equal to the data object's u32 at 68 (`key_bits`) | counted |
| 20 to 23, 28 to 31, 40 to 47, 52 to 55 | zero | counted |
| 24 | u32 with varied values (34 distinct **[sample]**); **[open]** | kept |
| 32 | source of a link to an `hkRefCountedProperties` object | enforced |
| 48 | `0xffffffff` | counted |
| 56, 72 | two empty array slots (8 zero bytes, count 0, `0x80000000`) | counted |
| 88 | `0xffffffff`; 92 to 95 zero | counted |
| 96 | source of a link to the object's `hknpCompressedMeshShapeData` | enforced |
| 104 | array slot of u32 words, storage at 160 | enforced |
| 120 | u32 `primitive_slots`; the slot at 104 holds ceil(it / 32) words | enforced |
| 128 | array slot of u32 words, storage right after the first, 16-aligned | enforced |
| 144 | u32 `key_slots`; the slot at 128 holds ceil(it / 32) words | enforced |
| 124 to 127, 148 to 159 | zero | counted |

The span ends at the end of the second storage block rounded up to 16 and
the padding is zero (enforced / counted). Both slots hold at least one word.
The `hkRefCountedProperties` object links at its byte 16 to a
`hknpBSMaterialProperties` object in all 167,916 cases
(`mesh_material_link`).

The two word arrays read as bit sets, bit k in word k / 32, bit k % 32:

- every set bit of the array at 104 falls on a primitive slot of a stored
  primitive (below), in 167,912 objects (4 fail); every set bit of the array
  at 128 falls on a key of a stored primitive, in all 167,916;
- bits past the count are not zero in every object (the single word of a
  one-primitive mesh stores `0x08f599d1` **[sample]**).

What a set bit means is **[open]**; `CompressedMesh::bit` returns it as
stored.

### Primitive slots and keys (counted)

With s the section index, p the primitive's position in its section, P the
number of bits of the largest primitive count of a section, a primitive has
the slot `(s << P) | p` and the keys `slot << 1`, plus `(slot << 1) | 1`
when its third and fourth bytes differ. Then:

| Rule | Holds | Fails |
| --- | ---: | ---: |
| shape u32 at 120 = last slot + 1 | 167,910 | 6 |
| data u32 at 72 = last key | 167,888 | 28 |
| shape u32 at 144 = data u32 at 72 + 1 | 167,916 | 0 |
| data u32 at 68 = bits(sections − 1) + P + 1 | 167,451 | 465 |
| shape byte 18 = data u32 at 68 | 167,916 | 0 |
| data u32 at 64 = primitives counted 2 when third ≠ fourth byte, else 1 | 166,281 | 1,635 |

That these are shape keys with a triangle bit, and that the u32 at 64 is a
triangle count, is **[hypothesis]**; the reader keeps the three u32 as
`MeshData::counts`.

## `hknpCompressedMeshShapeData`

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 to 15 | zero **[measured 167,916]** | counted |
| 16 | array slot, 5-byte nodes: the top tree, 2 × sections − 1 nodes | enforced |
| 32, 48 | three f32 each, the object box, min ≤ max; their exact union of the section boxes | enforced |
| 44, 60 | fourth f32 of each row: 1.0 in 138,889 objects; always the min and max of the sections' fourth f32 (`box_lanes_union_of_section_lanes`) | kept |
| 64, 68, 72 | u32 counts above | kept |
| 76 | zero **[measured 167,916]** | counted |
| 80 | array slot of 96-byte sections, at least one | enforced |
| 96 | array slot of 4-byte primitives | enforced |
| 112 | array slot of u16 shared indices (non-empty in 26,024 objects) | enforced |
| 128 | array slot of u32 section vertices (non-empty in 167,360) | enforced |
| 144 | array slot of 8-byte shared vertices (non-empty in 26,024) | enforced |
| 160 | array slot of u32 runs | enforced |
| 176 to 183, 200 to 207 | zero **[measured 167,916]** | counted |
| 184 | array slot of two 112-byte elements | enforced |

Storage blocks follow each other from byte 208 in the slot order 16, 80, the
section trees in section order, 96, 112, 128, 144, 160, 184; each starts on a
16-byte boundary, empty arrays take no block, and the span ends at the last
block rounded up to 16 (enforced **[measured 167,916]**). The padding is zero
(counted, all). This is the order that makes the first-table destinations of
this class not follow their sources (havok-objects.md): the section slots
come after the object's own slots in the bytes but their storage comes
before the primitive array.

The two elements at 184 hold, in all 167,916 objects, the same 224 bytes:
three pairs of rows of four f32 with bits `0x7f7fffee` then `0xff7fffee`,
then 16 zero bytes, twice (`tail_is_fixed_pattern`). The reader checks only
the count; the content is **[open]**.

### Sections: 96 bytes each (753,882)

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 | array slot of 4-byte nodes: the section tree, 2 × primitives − 1 nodes | enforced |
| 16, 32 | three f32 each, the section box, min ≤ max, inside the object box | enforced |
| 28, 44 | fourth f32 of each row: 1.0 in 190,532 sections; **[open]** | kept |
| 48 | three f32: `origin` of the section vertices | enforced finite when it has vertices |
| 60 | three f32: `scale`, finite and ≥ 0 when it has vertices | enforced |
| 72 | u32: first element of the section vertex array | enforced in range |
| 76 | u32: first shared index << 8 \| section vertex count | enforced in range |
| 80 | u32: first primitive << 8 \| primitive count (≥ 1) | enforced in range |
| 84 | u32: first run << 8 \| run count (≥ 1) | enforced in range |
| 88 | byte 88 equals the vertex count; byte 89 shared index count; u16 at 90 **[open]** | enforced / kept |
| 92 | byte 92: 0 in 742,701 sections, 1 to 4 in 11,181; **[open]** | kept |
| 93 | byte 93: 0 or 1 (1 in 82,158 sections: "flagged") | enforced |
| 94, 95 | zero | enforced |

Also enforced: section vertices plus shared indices at most 256 (the
primitives index them with one byte); the runs of a section start at
primitive 0 and follow each other with no gap up to the primitive count.
Counted: at most 128 primitives per section (all 753,882), the u16 at 90 is
zero exactly when the section has no shared index (751,948; 1,934 fail).

A section without vertices (3,830) stores origin bits `0x7f7fffee` and scale
`−inf` on each axis (`mesh_section_without_vertices_origin_scale_bits`).

The section-vertex, shared-index and primitive ranges of consecutive
sections follow each other from 0 and end at their array's count in all
167,916 objects (counted,
`section_vertex_shared_primitive_ranges_follow_each_other_and_cover_arrays`).
Run ranges do not: in 435 of the 8,350 sampled objects a section reuses runs
stored for another **[sample]**. The reader requires only that each range
lies inside its array.

## Vertices

**Section vertex**: a u32 w of the array at 128. Fields x = w & 0x7ff,
y = (w >> 11) & 0x7ff, z = w >> 22 (11, 11 and 10 bits); position
`origin + field × scale` per axis, computed in f32.

- In every section with vertices (750,052) the smallest field on each axis is
  0, and the largest is 2047, 2047, 1023 on every axis whose scale is not
  zero (`section_vertex_fields_reach_…`). So `origin` is the box of the
  section's own vertices and `scale` is its extent over 2047, 2047, 1023
  **[measured 750,052]**.
- Every decoded section vertex lies in the section box within 1e-5 ×
  max(1, largest box coordinate) (enforced, `geometry`).

**Shared vertex**: an 8-byte little-endian u64 w of the array at 144. Fields
x = w & 0x1fffff, y = (w >> 21) & 0x1fffff, z = w >> 42 (21, 21 and 22 bits);
position `min + field × (max − min) / (2^bits − 1)` per axis with the
object box, computed in f64 and stored as f32. A section reaches them through
its range of u16 shared indices: index byte v ≥ vertex count names shared
index `first + (v − vertex count)`, whose u16 is the position in the array at
144.

- Every shared index of a decoded section is below the shared vertex count
  and its vertex lies in the section box within the tolerance above
  (enforced).
- The box of the vertices that a section's primitives use equals the stored
  section box within 1e-5 relative in 662,181 of 662,186 decoded sections,
  and within 1e-6 in 624,826 (`section_box_is_box_of_used_vertices_…`). This
  equality is the evidence for the shared-vertex bit split and denominator:
  with 2^bits instead of 2^bits − 1 the box of all decoded vertices of a
  section matches its stored box within 1e-6 in 18,920 instead of 30,786 of
  the 36,091 sampled sections (flagged ones included) **[sample]**. The
  section-vertex split rests on the fields reaching 0 and 2047, 2047, 1023
  on every axis, above.

That this is the exact arithmetic of the game is not claimed; the reader
reproduces the stored boxes to the tolerance stated.

## Primitives

Each element of the array at 96 is four bytes. In a decoded section every
byte is below vertex count + shared index count (enforced), except the
placeholder `de ad de ad`, which `geometry` counts and leaves out (840 in
decoded sections, 865 in all unflagged sections). In all unflagged sections:
38,022,632 primitives with four distinct bytes, 7,607,534 whose third and
fourth bytes are equal, 865 placeholders, no other pattern
(`mesh_primitives_plain_…`).

`Primitive::indices` resolves the four bytes to positions in
`Geometry::vertices`. That a primitive with four distinct bytes is a quad,
that one whose last two bytes are equal is a triangle, and which diagonal
splits a quad are **[hypothesis]**; `geometry` returns the four indices
without forming triangles. 70 of 161,437 complete objects store a vertex no
primitive uses (178 counting the partially decoded objects).

**Runs** (array at 160): u32 = low u16 `run_value`, byte 2 first primitive
of the section, byte 3 count. Every run value of every object is below the
material count of its `hknpBSMaterialProperties` (the u32 at its byte 24)
**[measured 167,916]**; that it selects a material is **[hypothesis]**.

## Trees (counted, contents opaque)

**Top tree** (array at 16, 5-byte nodes, 2 × sections − 1 of them): in all
167,916 objects the nodes read in stored order form one binary tree in
preorder when bit 7 of byte 3 marks an inner node, and the other nodes carry
the values 0 … sections − 1 once each in `byte 4 | (byte 3 & 0x7f) << 8`
(`top_tree_preorder_inner_bit_and_leaves_number_each_section_once`). Bytes 0
to 2 are **[open]**: 176 to 182 distinct values each, entropy 6.3 bits;
byte 3 of inner nodes is `0x80` in most **[sample, 63,832 nodes]**.

**Section tree** (slot at section byte 0, 4-byte nodes, 2 × primitives − 1):
in all 753,882 sections the nodes form one preorder binary tree when an odd
byte 3 marks an inner node, and the other nodes carry byte 3 / 2 = 0 …
primitives − 1 once each (`section_tree_preorder_odd_byte3_inner_…`). For an
inner node, byte 3 / 2 equals the size of its left subtree minus one in
241,590 of 528,137 inner nodes **[sample]**, so its meaning is **[open]**.
Bytes 0 to 2 are **[open]**: 212 to 224 distinct values, entropy 6.8 bits;
0, 1, 16 and 17 are the most frequent values, which suggests two 4-bit fields
per byte **[hypothesis]**. That both trees are bounding-volume trees over
quantized boxes is **[hypothesis]**.

## Flagged sections (byte 93 = 1) and byte 92

82,158 sections in 6,479 objects (all of them direct bodies, all in
`Meshes\PreCombined` or `Meshes\SCOL` in the sample). `geometry` leaves them
out, together with the 9,538 unflagged sections whose byte 92 is not zero
(those occur only in objects that also have flagged sections). What was
measured, for a second team:

- They hold 6,380,239 primitives. Call a primitive *special* when its last
  three bytes are equal, differ from the first, and the first is at least the
  section's vertex count: 363,727 of them. For each, the u16 at shared-index
  position first byte − vertex count has a low byte of 0x52 (347,332), 0x51
  (13,191), 0x12 (2,867) or 0x50 (337) (`mesh_special_record_low_byte`).
  Reading that u16 and the next two as (tag, offset, third), tag >> 8 is a
  count of 4-byte words and `offset + ceil(count / 2)` lies inside the array
  at 144 in all 363,727 (`special_record_blob_inside_array_144`). So the
  array at 144 holds such blobs as well as shared vertices. In one sampled
  object the blob words, read with the 11/11/10-bit split, have fields from
  0 to near the maximum, and the third u16 takes values such as `0x3d4d`,
  `0x3c24`, `0x3db1` and `0x3c00`, which read as half floats between 1.0 and
  1.5; both readings are **[hypothesis]**. The fourth byte of a special
  primitive is below the vertex count in 165,676 and not in 198,051; its
  meaning is **[open]**.
- The other primitives of flagged sections index like unflagged ones:
  6,253,809 of their shared references land on a shared vertex inside the
  section box, 161,001 on one outside it, 6,328 past the shared range or
  the shared vertices, and 2,029 inside a special record
  (`mesh_flagged_plain_shared_reference`). Because 2.6% do not fit, flagged
  sections are not decoded.
- Byte 92 takes the values 1 to 4 in 11,181 sections
  (`mesh_section_bytes_92_93`), all in 7 objects that also have flagged
  sections (`mesh_objects_with_sections_byte92_or_93`: 6,472 objects with
  flagged sections only, 7 with both, none with byte 92 only); its meaning is
  **[open]**.

## Units: collision against render meshes

For a file with one bhkSystem whose node and mesh blocks all store
translation 0, the identity rotation and scale 1, and a single body that
stores translation 0 and four f32 (0, 0, 0, ±1) within 1e-6 and links a
fully decoded mesh directly, the census divides the largest extent of the
box of every render-mesh vertex position by the largest extent of the box of
the decoded collision vertices. 10,893 bodies qualify:

| Ratio render / collision | Bodies |
| --- | ---: |
| in [69.9905, 69.9915) | 4,682 |
| in [69.980, 70.000) | 5,903 |
| in [69.95, 70.05) | 7,675 |
| rounded to two significant digits, 70 | 8,938 |
| elsewhere (collision meshes that differ from the render mesh) | 1,955 |

Per axis, the three ratios agree within 5% in 6,595 of the 10,123 bodies
with three non-flat axes, 5,203 of them at 70 to two digits
(`mesh_render_ratio_per_axis`). So decoded collision coordinates are render
coordinates divided by about 69.991 **[measured mode]**, and the collision
axes are the render axes for these bodies. The exact constant, and whether
the game applies it to every shape class, is **[hypothesis]**.

## Opaque byte ranges

| Where | Bytes | What is known |
| --- | --- | --- |
| shape | 24 | u32, varied |
| shape | words at 104 and 128 | bit sets whose set bits fall on stored slots and keys; meaning open |
| data | 44, 60 and section 28, 44 | fourth f32 of the boxes; unions match; 1.0 in most |
| data | 184 storage | fixed 224-byte pattern |
| data | 16 storage (top tree) bytes 0 to 2 of each node | entropy 6.3 bits, distinct 176 to 182 |
| section | tree nodes bytes 0 to 2, byte 3 of inner nodes | entropy 6.8 bits; inner byte 3 odd |
| section | u16 at 90 | zero iff no shared index in 751,948 of 753,882 |
| section | byte 92 | 0 to 4 |
| flagged sections | special primitives, their records and blobs in the array at 144 | above |

## Status

| Question | Status |
| --- | --- |
| Layout of both classes, array slots and element sizes | **[measured 167,916]**, enforced |
| Section vertices (11/11/10 on the section's own box) and shared vertices (21/21/22 on the object box) | **[measured]** by box equality, enforced inside |
| Primitives: four vertex indices per element, placeholder | enforced on decoded sections |
| How a primitive forms triangles | **[hypothesis]** |
| Shape keys, bit sets | counted; meaning **[hypothesis]** / **[open]** |
| Trees | shape and leaf numbering **[measured]**; boxes **[open]** |
| Flagged sections, byte 92 | **[open]**, statistics above |
| Units | render / collision ≈ 69.991 **[measured mode]** |
| Runtime role of anything here | **[hypothesis]** |

## Reader errors

New codes, each with the NIF file name and the absolute file offset (`base`
+ `__data__` start + object offset + field): `InvalidHavokCompressedMesh`
(shape) and `InvalidHavokCompressedMeshData` (data object, sections and the
checks of `geometry`), plus `UnexpectedHavokClass`, `InvalidHavokObjectSpan`
and `LimitExceeded` (the `Budget`). None occurs in the census. Sections that
`geometry` cannot decode are not an error: they are listed in
`Geometry::skipped`, and the census reports the bodies that reach them as
`UnsupportedHavokMeshSection`. The synthetic tests cover each error, every
shorter span of both objects and every single-byte change of them.
