# Havok animation and skeleton objects in Fallout 4 `.hkx` files

Fourth Havok step, after [havok-container.md](havok-container.md) (which
now reads every winning `.hkx` file) and [havok-objects.md](havok-objects.md):
the objects of the animation clips and skeleton files. `fo4_havok::anim`
reads the top-level container's entries, the animation container's lists,
the skeleton, the binding, the header shared by the two compressed animation
classes, and the block table of `hkaSplineCompressedAnimation`.

Every statement below comes from measuring the stored bytes of one
installation. No Havok SDK, header, documentation, converter, decompilation
or other reader of this format or of animation compression was opened or
searched for (see [provenance](../docs/provenance.md)). Field names are this
project's labels; class names are the strings stored in `__classnames__`.
Where a reading was recognised from general knowledge before it was
measured, it enters here only with the measurement that could have refuted
it.

Evidence tags as in havok-container.md: **[measured N]** holds for N objects
and fails for none unless a second count says so; **[hypothesis]**;
**[open]**.

## Census scope

`anim_census MANIFEST OUT` (an example of `fo4-unpack`) reads every winning
`.hkx` file of the VFS through the container reader, runs every reader of
this file on every object of its class, counts the relations below and
compares each `skeleton.hkx` with the `skeleton.nif` beside it. The output,
counts and small stored values only, is committed as
[havok-animation-census.txt](havok-animation-census.txt). Same installation
as havok-container.md (build declared 1.10.163, 7 plugins, 62 archive
mounts): 17,508 winning `.hkx` files, 442,724,720 bytes; 2.3 s wall, peak
RSS 529 MB. The object spans, leading sizes and array element sizes of
these classes are the `havok.hkx.*` rows of
[havok-container-census.tsv](havok-container-census.tsv).

| Class | Files | Objects | Leading size | Read |
| --- | ---: | ---: | ---: | --- |
| `hkRootLevelContainer` | 17,508 | 17,508 | 16 | all, `root_entries` |
| `hkaAnimationContainer` | 16,388 | 16,388 | 96 | all, `AnimationContainer` |
| `hkaAnimationBinding` | 16,145 | 16,145 | 96 | all, `Binding` |
| `hkaSplineCompressedAnimation` | 15,060 | 15,060 | 176 | all, `SplineAnimation` |
| `hkaLosslessCompressedAnimation` | 1,085 | 1,085 | 224 | header only, `AnimationHeader` |
| `hkaDefaultAnimatedReferenceFrame` | 15,188 | 15,188 | 96 | not read |
| `hkMemoryResourceContainer` | 13,172 | 13,172 | 64 | not read |
| `hkaSkeleton` | 241 | 284 | 144 | all, `Skeleton` |
| `hkaSkeletonMapper` | 43 | 86 | 192 | not read |

## Slots

Offsets are from the object's offset. An *array* is the slot measured in
havok-objects.md: 8 bytes that a first-table entry points to the storage,
u32 count n at +8 and `0x80000000 | n` at +12 (no entry when n is 0),
checked by the readers with the element size given. A *text* is an 8-byte
slot that a first-table entry points to NUL-terminated bytes inside the
object's span; it has no count, which is why the array-header rule of the
collision payloads fails in these files. A *link* is a second-table entry.

### `hkRootLevelContainer`

Array at +0 of 24-byte entries: text (entry name) at +0, text (a class name)
at +8, link at +16. The linked object's class equals the stored class name
in every entry (enforced) **[measured 17,508 files]**. Entry names by class:
`Merged Animation Container` for every `hkaAnimationContainer` (16,388),
`Resource Data` (13,172), `SyncAnimOffset` (242), `SkeletonMapper` (86),
physics and cloth entries in skeleton files, and the behaviour, character
and project entries of the 1,120 behaviour files.

### `hkaAnimationContainer`

Arrays of links at +16 (element 8, every link to an `hkaSkeleton`), +32
(to one of the two animation classes) and +48 (to an `hkaAnimationBinding`),
enforced **[measured 16,388]**. Arrays at +64 and +80 are empty in every
object; their element size is unknown and they are not read. Lists
measured: 16,145 objects with one animation and one binding and no
skeleton; 198 with one skeleton, 43 with two; 2 empty.

### `hkaSkeleton`

| Bytes | Content | Reader |
| --- | --- | --- |
| 16 | text: the skeleton's name; bone 0's name in 274 of 284 | read |
| 24 | array of i16, one per bone: the parent | enforced |
| 40 | array of 16-byte bones: text (name) at +0, a byte at +8 (0 or 1; 3,444 of 11,896 bones are 1), bytes 9 to 15 zero | name and byte read |
| 56 | array of 48-byte reference-pose entries | enforced |
| 72, 88 | arrays with one element in two skeletons, empty in 282; element size unknown | count kept |
| 104, 120 | empty arrays | not read |

Enforced, each **[measured 284 skeletons, 11,896 bones]**: the three arrays
have the same count, at least 1; bone 0's parent is -1 and every other
bone's parent is at least 0 and below its own index (so one root, and
parents precede children); every pose word is finite.

A reference-pose entry is three groups of four f32: translation x, y, z
and a fourth word with varying values (not read); four f32 of unit norm
(largest deviation 2.9e-7, enforced within 1e-5); scale x, y, z (each within
2e-4 of 1 in every bone) and a fourth word 0 or 1 (not read).

**Against the skeleton NIFs.** Of the 190 `skeleton.hkx` files, 40 have a
`skeleton.nif` beside them. Their first skeletons hold 2,785 bones, 2,248 of
them named like a node of the NIF (all bones in 9 files). Composing the
reference pose from the root with the measured node convention of
[actor.md](actor.md) (a bone maps a child point p to `t + s R p`, with s
the first scale word) and reading the four f32 as a quaternion (x, y, z, w),
R being

```
[1-2(y²+z²)  2(xy-wz)    2(xz+wy) ]
[2(xy+wz)    1-2(x²+z²)  2(yz-wx) ]
[2(xz-wy)    2(yz+wx)    1-2(x²+y²)]
```

the bone's world translation is within 0.01 of the NIF node's world
translation for 2,080 of the 2,248 bones (1,919 within 0.001), and every
rotation element within 0.001 for 2,126. The three other readings measured
(R transposed; the w first, rows or transposed) put at most 180 bones
within 0.01 and at most 87 within 0.001 in rotation. So the stored order is
x, y, z, w and R acts on column vectors as in the NIF nodes **[measured
2,248 bones]**. Where the two hierarchies name the same parent for a bone
and all its ancestors (1,191 bones), the largest world translation
difference per file is below 0.001 in 28 files, below 0.1 in 37 and 1 or
more in 2; the dog skeleton matches in all 65 bones (largest 0.00019,
rotation 4e-6). The human skeleton names 74 of its 95 bones in the NIF but
shares the whole parent chain for only 15 of them (largest difference 0.0036
there); over all files the NIF names another parent for 154 bones. So a
pose is compared and applied by bone name in world space, not by parent
index.

### `hkaAnimationBinding`

| Bytes | Content | Reader |
| --- | --- | --- |
| 16 | text: a skeleton name | read |
| 24 | link to the animation object (spline or lossless) | enforced |
| 32 | array of i16: transform track i drives bone `track[i]` | read |
| 48 | array of i16 (one element in 4 bindings) | read |
| 64 | empty array | count kept |
| 80 | a byte: 0 (15,096), 1 (108) or 2 (941); then zeros | kept |

Measured **[16,145 bindings]**: every transform-track value is not negative;
the transform-track count equals the animation's u32 at +24 and the +48
count its u32 at +28. `Binding::fits(skeleton)` requires the binding's
name to equal the skeleton's name and every track to name a distinct bone
below the skeleton's bone count. Against the skeletons of all files: 16,142
bindings have at least one skeleton of their name that they fit (3 have
none); for 13,734 the `characterassets/skeleton.hkx` of their actor folder
fits, for 15 it has their name and does not fit. Which skeleton a clip is
played on is not stored in the clip.

### Animation header, both classes

| Bytes | Content |
| --- | --- |
| 16 | u32: 3 in every spline object, 0 in every lossless object (enforced) |
| 20 | f32 duration, finite, not negative (enforced) |
| 24 | u32 transform-track count |
| 28 | u32 float-track count (1 in 4 spline objects, 0 elsewhere) |
| 32 | link to an `hkaDefaultAnimatedReferenceFrame` (14,218 spline, 970 lossless) or nothing |
| 40 | array of 24-byte annotation tracks, as many as transform tracks in every object; not read further |

### `hkaSplineCompressedAnimation`: block table

| Bytes | Content | Measured relation (15,060 objects) |
| --- | --- | --- |
| 56 | u32 frames | `round(duration / frame duration) + 1` |
| 60 | u32 blocks | `max(1, ceil((frames - 1) / (frames per block - 1)))` (enforced) |
| 64 | u32 frames per block | 256 in every object |
| 68 | u32 | 4 × (transform + float tracks) (enforced) |
| 72 | f32 block duration | `(frames per block - 1) × frame duration`, bit-exact as f32 |
| 76 | f32 | `1 / block duration` within 1e-6 |
| 80 | f32 frame duration | 1/30 in 14,717; 25 other values |
| 84 | u32 | 0 |
| 88 | array of u32 block offsets, one per block | 0 first, increasing, multiples of 16, below the data length (enforced) |
| 104 | array of u32, one per block | **[open]** |
| 120, 136 | empty arrays (enforced; element size unknown) | |
| 152 | array of bytes: the data | length a multiple of 16; the storage ends the object's span |

Blocks per object: 1 in 14,364, 2 in 603, 3 to 9 or more in 93. Block i is
the data from its offset to the next block's offset, or to the end.
