# Havok compressed meshes in Fallout 4 collision payloads

Fifth step after [collision.md](collision.md),
[havok-container.md](havok-container.md), [havok-objects.md](havok-objects.md)
and [havok-bodies.md](havok-bodies.md): the stored layout of
`hknpCompressedMeshShape` and of the `hknpCompressedMeshShapeData` object it
links to, which half of all bodies end in. `fo4_havok::CompressedMesh` reads
the shape, `fo4_havok::MeshData` the data object and its sections,
`MeshData::section_boxes` and `MeshData::primitive_boxes` decode the two box
trees, and `MeshData::geometry` decodes the vertices, the four-index
primitives and the vertex records of every section of every stored object.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter, decompilation
or other reader of this format was consulted (see
[provenance](../docs/provenance.md)). Field names are this project's
descriptive labels ("section", "primitive", "shared vertex", "run", "tree",
"record", "frame", "radius");
class names are only the strings stored in `__classnames__`. What the game
does with any of these values is **[hypothesis]**.

Evidence tags as in havok-container.md: **[measured N]** holds for N and fails
for none unless a second count says so; **[hypothesis]** an interpretation the
bytes cannot decide, which no code relies on; **[open]** not established.
Objects are counted once per bhkSystem block. A few statistics come from a
1-in-20 sample of the 154,809 payloads that list the class (7,740 payloads,
8,350 shape objects) explored with a throwaway script; they are tagged
**[sample]** and are not in the TSV. The records, trees and byte 92 were
explored the same way on a local dump of the 6,479 data objects with byte 92
or 93 set and of 3,977 single-mesh files; every number below that is not
tagged **[sample]** comes from the committed census.

## Census scope

Same input and command as havok-container.md (`fo4-unpack sweep --scope
havok`; build declared as 1.10.163, 7 plugins, 62 archive mounts, VFS
fingerprint `a4316448…bc100`). The committed
[havok-container-census.tsv](havok-container-census.tsv) gains:

- `view` rows for both classes and for `geometry` (`read` or the error
  code);
- `object_check` rows `hknpCompressedMeshShape|…` and
  `hknpCompressedMeshShapeData|…` for the rules below that are counted, not
  enforced;
- `total` rows `mesh_…` (primitives, triangles, vertices, records, record
  vertices, placeholders, array use), the distributions `mesh_sections`,
  `mesh_section_flagged`, `mesh_section_bytes_92_93`,
  `mesh_objects_with_sections_byte92_or_93`, `mesh_primitive_kind_sections`,
  `mesh_properties_link`, `mesh_material_link`,
  `mesh_section_without_vertices_origin_scale_bits`,
  `mesh_record_kind_words`, `mesh_record_repeated_byte`,
  `mesh_record_odd_count_left_over_word` and
  `mesh_byte92_shared_reference_without_high_part`;
- `body_reaching_compressed_mesh`: per body whose shape links reach a
  compressed mesh, `geometry` or the reason it does not resolve;
- `mesh_render_ratio_…` and `mesh_render_triangles`: the comparisons with
  the render meshes below.

Run on the 16 GB Mac: 153.2 s wall, peak RSS 480 MB (`/usr/bin/time -l`).
The exit status is 1 because of the two `UnsupportedNifBsVersion` files
already reported by the collision census.

## Result

| | Count |
| --- | ---: |
| `hknpCompressedMeshShape` objects read by `CompressedMesh::read` | 167,916 of 167,916 |
| `hknpCompressedMeshShapeData` objects read by `MeshData::read` | 167,916 of 167,916 |
| data objects whose `geometry` decodes every section | 167,916 of 167,916 |
| bodies that reach a compressed mesh | 166,970 |
| of them, every reached mesh decoded | 166,970 (78,703 direct, 87,314 through a static compound, 953 through a dynamic compound) |
| failures | 0 |

Sections: 753,882, all decoded (82,158 with byte 93 = 1, 11,181 with byte
92 not zero). Stored primitives: 52,011,270 = 51,642,713 four-index
primitives (94,856,119 triangles by the rule below) + 367,517 vertex records
+ 1,040 placeholders. Vertices: 56,393,119 section and shared vertices and
6,745,028 record vertices. The 6,479 objects that had sections left out
before this step are now complete; they hold 59% of the four-index
primitives and every record.

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
| 16 | array slot, 5-byte nodes: the top tree, 2 × sections − 1 nodes (see Trees) | enforced |
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
| 0 | array slot of 4-byte nodes: the section tree, 2 × primitives − 1 nodes (see Trees) | enforced |
| 16, 32 | three f32 each, the section box, min ≤ max, inside the object box | enforced |
| 28, 44 | fourth f32 of each row: 1.0 in 190,532 sections; **[open]** | kept |
| 48 | three f32: `origin` of the section vertices | enforced finite when it has vertices |
| 60 | three f32: `scale`, finite and ≥ 0 when it has vertices | enforced |
| 72 | u32: first element of the section vertex array | enforced in range |
| 76 | u32: first shared index << 8 \| section vertex count | enforced in range |
| 80 | u32: first primitive << 8 \| primitive count (≥ 1) | enforced in range |
| 84 | u32: first run << 8 \| run count (≥ 1) | enforced in range |
| 88 | byte 88 equals the vertex count; byte 89 shared index count; u16 at 90 **[open]** | enforced / kept |
| 92 | byte 92: high part of shared positions (see Byte 92); 0 in 742,701 sections, 1 to 4 in 11,181 | used |
| 93 | byte 93: 0 or 1 (1 in 82,158 sections: "flagged", see Vertex records) | enforced |
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
index `first + (v − vertex count)`, whose u16 plus byte 92 << 16 (next
section) is the position in the array at 144.

- Every shared index of a decoded section that is not part of a record is
  below the shared vertex count and its vertex lies in the section box
  within the tolerance above (enforced).
- In the 671,724 sections without records the box of the vertices the
  primitives use equals the stored section box within 1e-5 relative in
  671,719 and within 1e-6 in 632,715
  (`section_box_is_box_of_used_vertices_…`). This equality is the evidence
  for the shared-vertex bit split and denominator: with 2^bits instead of
  2^bits − 1 the box of all decoded vertices of a section matches its stored
  box within 1e-6 in 18,920 instead of 30,786 of the 36,091 sampled sections
  **[sample]**. The section-vertex split rests on the fields reaching 0 and
  2047, 2047, 1023 on every axis, above.

That this is the exact arithmetic of the game is not claimed; the reader
reproduces the stored boxes to the tolerance stated.

### Byte 92

Byte 92 of a section is the high part of every position the section reads
through its u16 shared indices: shared vertex `u16 + byte92 << 16`, and the
same for record offsets (below). It is not zero only in 7 objects, whose
arrays at 144 hold 80,047 to 298,140 elements, and there it never decreases
from one section to the next **[sample: all 7]**. Evidence:

- read with the high part, every shared reference of those sections lands on
  a vertex inside the section box (enforced by `geometry`, all 11,181
  sections); read without it, 1,292,889 of 1,293,790 references land outside
  the box (`mesh_byte92_shared_reference_without_high_part`);
- record words read with the high part never fall on an element that a
  four-index primitive uses as a vertex (`array_144_element_is_…`, 6,479 of
  6,479 objects); read without it, 64,683 record elements do
  (`mesh_record_words_on_shared_vertices_without_byte92`).

## Primitives

Each element of the array at 96 is four bytes. Outside records (next
section) every byte is below vertex count + shared index count (enforced),
except the placeholder `de ad de ad`, which `geometry` counts and leaves out
(1,040). In all unflagged sections: 38,022,632 primitives with four distinct
bytes, 7,607,534 whose third and fourth bytes are equal, 865 placeholders, no
other pattern (`mesh_primitives_plain_…`). 201 of 167,916 objects store a
vertex that no four-index primitive uses (`every_vertex_used_by_a_primitive`).

`Primitive::indices` resolves the four bytes to positions in
`Geometry::vertices`; `Primitive::quad` is true when the third and fourth
stored bytes differ.

### Triangles

`Primitive::triangles` returns [a, b, c] for a primitive whose third and
fourth bytes are equal, and [a, b, c] and [a, c, d] otherwise. Evidence:

- **Render meshes.** For each body that stores no placement in a file with
  one bhkSystem, identity node transforms and a render / collision extent
  ratio in [69.9, 70.1] (7,956 bodies), the census maps each decoded vertex
  to the nearest render position scaled by that ratio, within 2.5 steps of a
  10-bit axis of the widest section, and looks the candidate triangles up
  among the render triangles of the file (`mesh_render_triangles`):

  | Primitives | Count |
  | --- | ---: |
  | triangle [a, b, c] found with the render winding | 50,264 |
  | triangle found with the opposite winding | 264 |
  | triangle not found | 19,220 |
  | quad: only the split [a, b, c] + [a, c, d] found | 338,865 |
  | quad: only the split [a, b, d] + [b, c, d] found | 9,607 |
  | quad: both splits found | 2,734 |
  | quad: neither found | 163,555 |
  | of the quads whose fourth vertex lies off the plane of the first three by more than 1% of the longer diagonal: only [a, c] found / only [b, d] found | 38,469 / 155 |
  | of the 338,865 [a, c]-only quads: both triangles with the render winding / both opposite | 337,203 / 1,628 |
  | a vertex without a render position in range / two vertices on one position | 253,424 / 1,093 |

  The split decides the surface only for a non-planar quad; there the
  render mesh has [a, c] 248 times as often as [b, d].
- **Stored counts.** The u32 at 64 of the data object equals the number of
  primitives counting 2 for third ≠ fourth byte and 1 otherwise in 166,281
  of 167,916 objects (above), which fits two triangles per quad.

So the split and the winding are **[measured]** as the ones that coincide
with the render mesh; that the game's collision uses exactly these
triangles is **[hypothesis]**.

**Runs** (array at 160): u32 = low u16 `run_value`, byte 2 first primitive
of the section, byte 3 count. Every run value of every object is below the
material count of its `hknpBSMaterialProperties` (the u32 at its byte 24)
**[measured 167,916]**; that it selects a material is **[hypothesis]**.
Records carry the run value of their primitive.

## Trees

**Top tree** (array at 16, 5-byte nodes, 2 × sections − 1 of them): in all
167,916 objects the nodes read in stored order form one binary tree in
preorder when bit 7 of byte 3 marks an inner node, and the other nodes carry
the values 0 … sections − 1 once each in `byte 4 | (byte 3 & 0x7f) << 8`
(`top_tree_preorder_inner_bit_and_leaves_number_each_section_once`).

**Section tree** (slot at section byte 0, 4-byte nodes, 2 × primitives − 1):
in all 753,882 sections the nodes form one preorder binary tree when an odd
byte 3 marks an inner node, and the other nodes carry byte 3 / 2 = 0 …
primitives − 1 once each (`section_tree_preorder_odd_byte3_inner_…`). For an
inner node, byte 3 / 2 equals the size of its left subtree minus one in
241,590 of 528,137 inner nodes **[sample]**, so its meaning is **[open]**.

**Boxes.** In both trees bytes 0, 1 and 2 of a node belong to x, y and z.
The high nibble a raises the minimum of the parent's box by a² / 226 of the
parent's extent on that axis, the low nibble b lowers the maximum by b² /
226. The root's parent box is the object box (top tree) or the section box
(section tree); the reader computes in f64 from the stored f32 boxes
(`fo4_havok::shrink`, `TREE_SHRINK_DENOMINATOR`).

- Every top-tree leaf box contains its section box within 1e-6 relative, in
  all 167,916 objects (`top_tree_leaf_boxes_contain_section_boxes_rel_1e-6`).
- Every section-tree leaf box contains the vertices of its four-index
  primitive within 1e-6 relative: 51,642,514 of 51,642,713 primitives with
  226, against 6,167,888 with 225
  (`section_tree_leaf_box_contains_primitive_vertices_d22…`).
- The denominator comes from the depth-1 nodes of the section trees, whose
  parent is the section box **[sample, 1 in 5 of the 3,977 single-mesh
  files]**: for each nibble n the smallest fraction of the parent that the
  child really leaves out is at least n² / 226, and for most n the largest
  is below (n + 1)² / 226, which bounds the denominator to about [225.99,
  226.03). Only nibbles 2 and 9 have a largest fraction above 9 / 226 and
  100 / 226, by at most 4e-4: a smaller nibble than possible, which keeps
  the box conservative.
- The stored nibbles are the largest that keep the vertices inside in
  44,391,002 of 51,642,713 leaves (`section_tree_leaf_nibbles_maximal_d226`);
  the encoder's own arithmetic is not reproduced exactly.

Byte 3 of an inner node beyond its odd bit, and byte 3 / 2 for inner nodes
of the top tree, are **[open]**. That the game uses these boxes for culling
or queries is **[hypothesis]**.

## Vertex records (sections with byte 93 = 1)

82,158 sections in 6,479 objects have byte 93 = 1 (all in
`Meshes\PreCombined` or `Meshes\SCOL` in the sample). In them, and only
there, a primitive (p0, r, r, r) whose last three bytes are equal and whose
first byte is at least the vertex count addresses a **record** at
shared-index position p0 − vertex count instead of four vertices: 367,517
primitives, at least one in every byte-93 section. No other section stores a primitive
whose last three bytes are equal and whose first byte is at least its vertex
count (`mesh_primitives_plain_last_three_equal_first_at_least_vertex_count`
is 0, the flagged count 367,517).

**Layout.** At the record position the u16 shared indices hold:

| u16 | Content |
| --- | --- |
| 0 | low byte: kind, 0x12, 0x50, 0x51 or 0x52 (`RECORD_KINDS`, enforced); high byte: word count, at least 1 (enforced) |
| 1 | offset of the words, in 8-byte elements of the array at 144, plus byte 92 << 16; offset + ceil(count / 2) inside the array (enforced) |
| 2 | only when bit 0x40 of the kind is set: the high half of an f32, the **radius** (finite and ≥ 0, enforced) |

A kind-0x12 record is two u16 long: the u16 after it is an ordinary shared
index, which 1,956 references of four-index primitives use, all landing
inside their section box **[sample, every reference]**. No four-index
primitive reaches a position inside a record (enforced). Records and the
vertices of four-index primitives share the array at 144 without overlap
(`array_144_element_is_shared_vertex_or_record_word_not_both`, 6,479 of
6,479 objects); 602 elements, all in the 7 byte-92 objects, are neither
(`mesh_array_144_elements_unused_in_objects_with_records`).

| Kind | Words | Records |
| --- | --- | ---: |
| 0x50 | 1 | 342 |
| 0x51 | 2 | 13,343 |
| 0x52 | 4 to 8 | 239,093 (236,895 with 8) |
| 0x52 | 9 to 32 / 33 or more | 57,468 / 54,369 |
| 0x12 | 4 / 8 / 9 to 32 | 118 / 2,733 / 51 |

(`mesh_record_kind_words`; no 0x52 record with fewer than 4 words.)

**Words.** Word i of a record is the u32 at 4 × (2 × offset + i) of the
array at 144, except that with an odd count the last word is the high half
of the last element and its low half is left over. The left-over u32 is 0
in 26,318, below 65,536 in 2,896 and other in 7,386 records
(`mesh_record_odd_count_left_over_word`); values such as `0x40000397` and
ASCII text suggest stale memory **[hypothesis]**. Each word has the fields
x = w & 0x7ff, y = (w >> 11) & 0x7ff, z = w >> 22, and its position is
`lo + field × (hi − lo) / (2047, 2047, 1023)` where (lo, hi) is the box of
the record's primitive in the section tree (**frame**); `geometry` enforces
lo ≤ hi.

**Evidence.** For each of the 82,158 sections with records the census
compares the section box with the box of the vertices of its four-index
primitives and of every record vertex widened by the record's radius,
allowing on a record side half a step of its frame plus 1/128 of the radius
(the lost low half): 81,931 sections agree within 1e-5 relative beyond that
allowance and 82,157 within 1e-3 relative without it
(`section_box_is_box_of_primitive_vertices_and_record_vertices_plus_radius_…`).
Changing one reading at a time breaks the agreement
(`record_contrast_…`): no radius, 52,738 sections fail; the words of an odd
count read in stored order, 6,530 fail; the section box as frame, 47,723
fail. Two independent checks **[sample]**: the 8-word kind-0x52 records are
centrally symmetric point sets (parallelepipeds) in 95% of 53,268 sampled,
and with the frame above their edges are mutually perpendicular within a
cosine of 0.03 in 98% of 42,195 (34% with the section box as frame); and in
all 174 sections whose only primitive is a record, every axis of the section
box exceeds the box of the record vertices by the radius on both sides,
within one step plus 1/128 of the radius.

What a record is (a convex piece with a radius, a sphere for kind 0x50 with
one word, a capsule for kind 0x51 with two) is **[hypothesis]**; the reader
returns the words as points, the radius and the kind (`MeshRecord`) and
forms no faces. The repeated byte r is below the vertex count in 165,676
records, names a shared position in 173,424 and is past the section's
indices in 28,417 (`mesh_record_repeated_byte`), so it is not an index; its
meaning is **[open]** (`MeshRecord::repeated`).

## Units: collision against render meshes

For a file with one bhkSystem whose node and mesh blocks all store
translation 0, the identity rotation and scale 1, and a single body that
stores translation 0 and four f32 (0, 0, 0, ±1) within 1e-6 and links a
mesh directly, the census divides the largest extent of the
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
(`mesh_render_ratio_per_axis`). The triangle comparison above uses the
bodies of this table whose ratio lies in [69.9, 70.1]. So decoded collision coordinates are render
coordinates divided by about 69.991 **[measured mode]**, and the collision
axes are the render axes for these bodies. The exact constant, and whether
the game applies it to every shape class, is **[hypothesis]**.

## Opaque byte ranges

| Where | Bytes | What is known |
| --- | --- | --- |
| shape | 24 | u32, varied |
| shape | words at 104 and 128 | bit sets whose set bits fall on stored slots and keys; meaning open |
| data | 44, 60 and section 28, 44 | fourth f32 of the boxes; unions match; 1.0 in most objects, not in most sections |
| data | 184 storage | fixed 224-byte pattern |
| top tree | byte 3 bits 0 to 6 of inner nodes | open |
| section tree | byte 3 / 2 of inner nodes | equals left subtree size − 1 in 46% **[sample]** |
| section | u16 at 90 | zero iff no shared index in 751,948 of 753,882 |
| records | the repeated byte of the primitive; the left-over low half of the last element for odd counts | statistics above |

## Status

| Question | Status |
| --- | --- |
| Layout of both classes, array slots and element sizes | **[measured 167,916]**, enforced |
| Section vertices (11/11/10 on the section's own box) and shared vertices (21/21/22 on the object box) | **[measured]** by box equality, enforced inside |
| Byte 92 as the high part of shared positions | **[measured]**, enforced inside |
| Tree boxes (nibbles, a² / 226) | **[measured]**: every leaf contains its contents |
| Records: layout, words on the tree frame, radius | **[measured]** by section box equality; contrasts fail |
| Primitives as triangles [a, b, c] (+ [a, c, d]) | **[measured]** against render meshes; runtime use **[hypothesis]** |
| Shape keys, bit sets | counted; meaning **[hypothesis]** / **[open]** |
| What a record is (convex, sphere, capsule) | **[hypothesis]** |
| Units | render / collision ≈ 69.991 **[measured mode]** |
| Runtime role of anything here | **[hypothesis]** |

## Reader errors

New codes, each with the NIF file name and the absolute file offset (`base`
+ `__data__` start + object offset + field): `InvalidHavokCompressedMesh`
(shape) and `InvalidHavokCompressedMeshData` (data object, sections, trees,
records and the checks of `geometry`), plus `UnexpectedHavokClass`,
`InvalidHavokObjectSpan` and `LimitExceeded` (the `Budget`). None occurs in
the census; the census gives each mesh its own budget of 256 MiB. The
synthetic tests cover each error, every shorter span of both objects, every
single-byte change of them and of a section with a record, the tree
arithmetic and the triangle rule.
