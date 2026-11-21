# Havok bodies and simple shapes in Fallout 4 collision payloads

Fourth step after [collision.md](collision.md),
[havok-container.md](havok-container.md) and
[havok-objects.md](havok-objects.md): from a `bhkNPCollisionObject` to "this
body links to this shape object and stores this placement". It measures the
element size of each array slot, the array of 96-byte elements in the
top-level object, and the stored layout of the simple shape classes.
`fo4_havok::PhysicsSystem` reads the top-level object and its bodies;
`Sphere`, `Capsule`, `ConvexShape`, `ScaledConvex` and `Compound` read the
simple shapes.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter or other reader
of this format was consulted (see [provenance](../docs/provenance.md)). Field
names are this project's descriptive labels; class names are only the strings
stored in `__classnames__`. "Body", "translation", "rotation", "radius" and
"scale" label stored values; what the game does with them is
**[hypothesis]**.

Evidence tags as in havok-container.md: **[measured N]** holds for N and
fails for none unless a second count says so; **[hypothesis]** an
interpretation the bytes cannot decide, which no code relies on;
**[open]** not established. Objects, bodies and instances are counted once
per bhkSystem block, as in havok-objects.md.

## Census scope

Same input and command as havok-container.md (`fo4-unpack sweep --scope
havok`; build declared as 1.10.163, 7 plugins, 62 archive mounts, VFS
fingerprint `a4316448…bc100`). The committed
[havok-container-census.tsv](havok-container-census.tsv) gains these rows:

- `array_gap`, keyed `class|+slot`: for every first-table source in the
  object's leading bytes, the bytes from its destination to the next
  destination of the same object (or the span end), divided by the count at
  +8. `element_max` is the largest element size that fits every array;
  `element_min` the smallest that leaves fewer than 16 bytes after every
  array; `exact` the arrays whose gap is a multiple of `element_max`. Sources
  inside another array's storage are pooled under `class|inside_storage`;
- `view`, keyed `class|read` or `class|error code`: results of the new views;
- `object_check` rows for the rules below that are counted, not enforced;
- `body_shape`, `body_geometry`, `constraint_class`, `ragdoll_link120`,
  `scaled_convex_child`, `compound_child`, `compound_link192`,
  `compound_instances` and `u16_16_18` distributions;
- `body_id_below_body_count`, keyed `root class|objects` (per collision
  object) and `root class|systems`.

Run on the 16 GB Mac: 95.2 s wall, peak RSS 525 MB (`/usr/bin/time -l`). The
exit status is 1 because of the two `UnsupportedNifBsVersion` files already
reported by the collision census; every payload and every object of the
classes below reads without error.

## Array element sizes

At every first-table source the slot is 8 zero bytes, a count n and
`0x80000000 | n` (havok-objects.md). From the gaps, per slot
**[measured, every array of the slot]**:

| Class | Slot | Arrays | Largest n | Element size (min..max) |
| --- | ---: | ---: | ---: | --- |
| `hknpPhysicsSystemData` | +16 | 275,154 | 206 | 80 |
| `hknpPhysicsSystemData` | +32 | 10,749 | 206 | 64 |
| `hknpPhysicsSystemData` | +48 | 5,697 | 206 | 112 |
| `hknpPhysicsSystemData` | +64 | 275,154 | 206 | 96 |
| `hknpPhysicsSystemData` | +80 | 244 | 39 | 24 |
| `hknpPhysicsSystemData` | +96 | 275,154 | 206 | 8 |
| `hknpRagdollData` | +16 to +96 | 188 each | 65 | 80, 64, 112, 96, 24, 8 as above |
| `hknpRagdollData` | +128 | 188 | 65 | 4 |
| `hknpDynamicCompoundShape` | +96 | 8,918 | 120 | 128 (127..128 in the 13 ragdoll objects) |
| `hknpDynamicCompoundShapeData` | +16 | 8,918 | 241 | 32 |
| `hknpStaticCompoundShape` | +96 | 87,314 | 1 | 113..128 |
| `hknpStaticCompoundShapeData` | +16 | 87,314 | 1 | 1..16 |
| `hkRefCountedProperties` | +0 | 386,485 | 1 | 1..16 |
| `hknpBSMaterialProperties` | +16 | 167,916 | 41 | 24 |
| `hkaSkeleton` | +24, +40, +56 | 188 each | 65 | 2, 16, 48 |
| `hknpCompressedMeshShape` | +104, +128 | 167,916 each | 33,788 | 4, 4 |
| `hknpCompressedMeshShapeData` | +16 to +184 | see TSV | 324,797 | 5, 96, 4, 2, 4, 8, 4, 105..112 |

So the element size is one value per (class, slot) wherever some array of
the slot has more than one element. Where every array has one element the gap
only bounds it; the 128 of the static compound comes from its instance layout
(below), shared with the dynamic compound. All of this rests on the premise
that fewer than 16 bytes follow an array's storage. For the slots of the
top-level objects the bytes after the storage are zero and fewer than 16
(`array_<slot>_bytes_after_storage_zero_and_below_16`)
**[measured 275,342 objects]**. The compressed-mesh classes are not part of
this step.

`fo4_havok::Array` checks one slot for the views: the flag word, a
first-table entry exactly when n is not zero, and storage of n elements that
ends inside the span before the next storage block of the object
(`InvalidHavok…` of the calling view otherwise).

## The top-level object

`hknpPhysicsSystemData` (275,154 objects) and `hknpRagdollData` (188) share
their first 128 bytes. Offsets from the object's offset:

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 to 15 | zero **[measured 275,342]** | counted |
| 16, 32, 48, 64, 80, 96 | array slots of 80, 64, 112, 96, 24 and 8 bytes | enforced |
| 112 to 127 | zero **[measured 275,342]**; in a ragdoll bytes 120 to 127 are the source of a link to the `hkaSkeleton` object (188 of 188) | counted |
| 128 (ragdoll only) | array slot of 4 bytes; the physics object's storage starts here | enforced |

Counts: n(+16) = n(+64) = n(+96) in every object (enforced). Call this n
the body count. n(+32) is 0 or n in 274,928 of 275,154 physics objects;
n(+48) = n(+32) in 266,594; in a ragdoll n(+128) = n in 168 of 188. These
three are counted only; what the arrays at +16, +32, +48 and +128 hold is
**[open]**.

### Bodies: the array at +64

Element i, 96 bytes, offsets from the element:

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 | source of a second-table link: the body's shape object | enforced |
| 8 | u32 `0x7fffffff` **[measured 333,529 bodies]** | counted |
| 12 | u32 `0x7fffffff` or below n(+48) **[measured 333,529]**; meaning **[open]** | counted |
| 16, 17 | bytes `ff 00` **[measured 333,529]** | counted |
| 18 | u16: a position in the array at +96 | enforced |
| 20, 24 | u32 with varied values; meaning **[open]** | kept |
| 28 to 47 | zero **[measured 333,529]** | counted |
| 48 | three finite f32: `translation` | enforced |
| 60 | u32, zero in 318,838 bodies; read as f32, small values otherwise; **[open]** | counted |
| 64 | four finite f32 of unit norm: `rotation`, in stored order | enforced |
| 80 to 95 | zero **[measured 333,529]** | counted |

Enforced by `PhysicsSystem::read` (`InvalidHavokPhysicsSystem`), each
**[measured 275,342 objects, 333,529 bodies]**:

- every element has a link at byte 0;
- the u16 at 18 values form a permutation of 0..n, and position u16 of the
  array at +96 (8-byte elements, each the source of a link) links to the same
  object as byte 0. In physics objects the u16 equals i in every body; in
  ragdolls in 471 of 1,589 bodies;
- the four f32 at 64 have unit norm within 1e-5. The largest deviation
  stored is 1.9e-7 (`body_rotation_unit_within_1e-6` holds on every body).
  That they are a rotation quaternion, and which of the four is the scalar,
  is **[hypothesis]**; the f32 at 76 is non-negative in 332,235 bodies;
- the translation is finite.

No two bodies of a system link to the same object (`body_shapes_distinct`),
and every body links to an object whose class name ends in `Shape`
(`body_shape_is_a_shape_class`) **[measured 275,342 objects]**. Bodies by
shape class:

| Shape class | Physics bodies | Ragdoll bodies |
| --- | ---: | ---: |
| `hknpScaledConvexShape` | 99,438 | 0 |
| `hknpStaticCompoundShape` | 87,314 | 0 |
| `hknpCompressedMeshShape` | 78,703 | 0 |
| `hknpConvexPolytopeShape` | 56,294 | 81 |
| `hknpDynamicCompoundShape` | 8,905 | 13 |
| `hknpCapsuleShape` | 1,030 | 1,405 |
| `hknpSphereShape` | 154 | 90 |
| `hknpConvexShape` | 102 | 0 |
| total | 331,940 | 1,589 |

### Constraints: the array at +80

Each 24-byte element is the source of a link to a constraint object (classes
in `constraint_class`), then a u32 below the body count at +8, a u32 below
the body count or `0x7fffffff` at +12, and zero bytes 16 to 23
**[measured 2,106 elements]**. Counted only; that the two u32 name the two
bodies a constraint joins is **[hypothesis]**.

### Body ID

Every `Body ID` of the 333,526 `bhkNPCollisionObject` is below the body
count of its system (331,937 physics, 1,589 ragdoll; 275,342 of 275,342
systems) **[measured]**. With havok-container.md (Body IDs are exactly
0..k−1, and n = k in 275,339 systems) this is consistent with Body ID i
selecting body i, which stays
**[hypothesis `nif_collision_body_id_selects_body_v1`]**.

## Simple shapes

Offsets from the object's offset. The views check the class
(`UnexpectedHavokClass`) and the index (`InvalidHavokObjectSpan`).

### `hknpSphereShape` (279 objects)

Span 128 bytes; bytes 0 to 15, 28 to 47 and 52 to 63 zero; u16 pair at 48 =
(4, 16), so four rows at 64; row 0 tagged `0x3f000000`; rows 1 to 3 byte
copies of row 0; finite f32 at 20 that is not negative (one object stores 0)
**[measured 279]**. `Sphere::read` enforces all but the zero runs
(`InvalidHavokSphere`) and returns `center` (row 0, zero in 228 objects) and
`radius` (f32 at 20). That f32 at 20 is the radius is **[hypothesis]**: it is
the only stored size, and the same position holds a non-negative f32 in every
polytope and capsule.

### `hknpCapsuleShape` (4,327 objects)

Span 432; the four (count, offset) pairs at 48, 64, 68 and 72 are (8, 64),
(8, 176), (6, 300), (24, 328) in every object, so 8 vertex rows at 112, 8
plane rows at 240, 6 faces at 368 and 24 index bytes at 400
**[measured 4,327]**. Rows at 80 and 96 hold two points with fourth f32 1.0;
f32 at 20 is positive.

Enforced by `Capsule::read` (`InvalidHavokCapsule`), each
**[measured 4,327]**: span, the four pairs, finite points with fourth f32
1.0 and distinct, positive finite radius, vertex rows tagged `0x3f000000`
OR their index, and two consistency rules: both points lie inside the box of
the eight vertices, and every vertex lies within the radius of the segment
between the points. Counted: every vertex lies within 0.02 × radius of the
segment. A separate exploration script over the same payloads finds the
largest such distance at 0.0175 × radius and the vertex box equal to the
points' box widened by 0.0099 to 0.0171 × radius per side; the eight
vertices take exactly two values per axis in 2,113 objects (the others
differ in the last bits). Planes, faces and indices are
not read.

### `hknpConvexShape` (110 objects)

`ConvexShape::read` (`InvalidHavokConvexShape`) enforces the rules already
counted in havok-objects.md: vertex pair at 48 pointing to 64, 1 to 256 rows,
span 64 + 16 per row, every row tagged with its index, finite coordinates
**[measured 110]**.

### `hknpScaledConvexShape` (99,438 objects)

| Bytes | Content | Reader |
| --- | --- | --- |
| 0 to 15, 28 to 47, 56 to 63, 96 to 111 | zero **[measured 99,438]** | counted |
| 16, 18 | u16 (256, 256) in 99,386 objects, (0, 256) in 52 | counted |
| 20 | f32; equals the child polytope's f32 at 20 in 95,585 of 99,366 | counted |
| 24 | u32 with varied values; **[open]** | kept |
| 48 | source of a link: child shape | enforced |
| 64 | three positive finite f32: `scale`; u32 at 76 has high 24 bits `0x3f0000` | enforced / counted |
| 80 | three finite f32: `translation`; finite f32 at 92 **[open]** | enforced / counted |

Children: `hknpConvexPolytopeShape` 99,366, `hknpCapsuleShape` 71,
`hknpConvexShape` 1. The three scale values are equal in 203 objects. How
scale and translation combine with the child's vertices is **[hypothesis]**.

### `hknpStaticCompoundShape` (87,314) and `hknpDynamicCompoundShape` (8,918)

The two names share a class signature (havok-container.md) and the same
layout: the array slot at 96 with instances of 128 bytes stored from byte 208
to the end of the span (span = 208 + 128 n), and a link at 192 to the
matching `…ShapeData` object in every object **[measured 96,232]**. A static
compound has exactly one instance in every object; a dynamic one 1 to 120.
Each instance, from its start:

| Bytes | Content | Reader |
| --- | --- | --- |
| 0, 16, 32 | three rows of three finite f32, orthonormal within 1e-4, determinant positive (`rotation`); largest deviation 6.7e-6 | enforced |
| 12, 28, 44, 60 | u32 words with flag bits; **[open]** | kept |
| 48 | three finite f32: `translation` | enforced |
| 64 | three positive finite f32, all equal: `scale`; f32 at 76 is 1.0 | enforced / counted |
| 80 | source of a link: child shape | enforced |
| 84, 92 to 127 | zero; u32 at 88 `0xffffffff` | counted |

Instance rules hold on 153,794 instances (87,314 static, 66,480 dynamic), and
the number of links inside the instance storage equals n in every object
**[measured]**. Children: static, `hknpCompressedMeshShape` in every
instance; dynamic, `hknpConvexPolytopeShape` 62,704 + 14, capsule 1,794 + 27,
`hknpCompressedMeshShape` 1,899, sphere 35, `hknpConvexShape` 7 (physics +
ragdoll). Whether the rows are rows or columns of the rotation is
**[hypothesis]**. `Compound::read` enforces the rows marked enforced
(`InvalidHavokCompound`).

## How much of the corpus resolves

For each body the census follows the shape links (scaled convex to its child,
compound to every instance child) until it reaches a class with a vertex or
point view (`ConvexPolytope`, `ConvexShape`, `Sphere`, `Capsule`):

| Bodies | Count |
| --- | ---: |
| reach geometry with every link resolved | 166,559 of 333,529 (164,970 physics, all 1,589 ragdoll) |
| end in `hknpCompressedMeshShape` | 166,970 (78,703 directly, 87,314 through a static compound, 953 through a dynamic compound) |

So about half of the bodies now go body → shape → vertices or points with a
stored placement at each link, all by readers that enforce only rules that
held on every object. The other half wait for the compressed mesh, out of
scope here.

## Status

| Question | Status |
| --- | --- |
| Array element sizes | One per (class, slot) where measurable **[measured]**; table above |
| Which array holds the bodies | The 96-byte array at +64: shape link, translation, unit-norm four f32 **[measured 333,529]**, enforced |
| Body ID selects body i | Body ID < n for all 333,526 objects **[measured]**; selection **[hypothesis]** |
| Order and roles of the rotation f32, rows vs columns | **[hypothesis]** |
| Units, and the space of body placements relative to the Target node | **[open]** |
| Arrays at +16, +32, +48, +128 and body words 12, 20, 24, 60 | **[open]** |
| How scale, translation and instance placement compose with child vertices | **[hypothesis]** |
| Bytes 16 to 27 of each shape (u16 pair by class, f32, u32) | **[open]** |
| Compressed mesh | Out of scope for this step **[open]** |

## Reader errors

New codes, each with the NIF file name and the absolute file offset
(`base` + `__data__` start + object offset + field): `InvalidHavokPhysicsSystem`,
`InvalidHavokSphere`, `InvalidHavokCapsule`, `InvalidHavokConvexShape`,
`InvalidHavokScaledConvex`, `InvalidHavokCompound`, plus
`UnexpectedHavokClass`, `InvalidHavokObjectSpan` and `LimitExceeded` (the
`Budget`). None occurs in the census. The synthetic tests cover each error,
every shorter span of each generated object and every single-byte change of
it.
