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
this file on every object of its class, decodes every spline object, counts
the relations below and compares each file's first skeleton with the `.nif`
of the same path stem. The *race skeletons* are the `.hkx` files at the path
of a skeleton NIF that a RACE record names (ANAM, resolved under `meshes/`
as record models are) with `.hkx` for `.nif`: 45 files. The output, counts
and small stored values only, is committed as
[havok-animation-census.txt](havok-animation-census.txt). Same installation
as havok-container.md (build declared 1.10.163, 7 plugins, 62 archive
mounts): 17,508 winning `.hkx` files, 442,724,720 bytes; 8.0 s wall, peak
RSS 868 MB (two passes over the files and the plugins opened once). The object spans, leading sizes and array element sizes of
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

**Against the skeleton NIFs.** Of the 241 files holding a skeleton, 89 have
a `.nif` of the same path stem, among them the 45 race skeletons. The first
skeletons of those 45 hold 2,915 bones, 2,139 of them named like a node of
the NIF (all bones in 10 files). Composing the
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
translation for 1,962 of the 2,139 bones (1,853 within 0.001), and every
rotation element within 0.001 for 2,008. The three other readings measured
(R transposed; the w first, rows or transposed) put at most 181 bones
within 0.01 and at most 88 within 0.001 in rotation. Over all 89 files the
same reading gives 2,589 and 2,635 of 2,766 bones. So the stored order is
x, y, z, w and R acts on column vectors as in the NIF nodes **[measured
2,766 bones]**. Where the two hierarchies name the same parent for a bone
and all its ancestors (1,182 bones of the race skeletons), the largest
world translation difference per file is below 0.001 in 32 of the 45
files, below 0.1 in 41 and 1 or more in 3; the dog skeleton matches in all
65 bones (largest 0.00019, rotation 4e-6). The human skeleton names 74 of
its 95 bones in the NIF but shares the whole parent chain for only 15 of
them (largest difference 0.0036 there); in the race skeletons the NIF names
another parent for 163 bones. So a pose is compared and applied by bone
name in world space, not by parent index.

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
none); 15,861 fit at least one race skeleton, and 14,909 one in their own
folder (the first three path components). Which skeleton a clip is played
on is not stored in the clip.

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

## Track data of `hkaSplineCompressedAnimation`

`SplineAnimation::decode_block` reads the transform tracks of one block;
`decode` reads all blocks and `DecodedSpline::sample(t)` gives each track's
local translation, rotation and scale at clip time t. Established by byte
consumption over every block, then by the value checks below. Exploration
used a local copy of the `.hkx` files (`hkx_dump`) and throwaway scripts
outside the repository; every number here was then counted by
`anim_census` with the Rust reader over the whole installation. All 15,060
spline objects decode, 16,063 blocks; the 1,085 lossless objects and the
float tracks of the 4 spline objects that have one are not decoded.

### Bytes of a block

A block starts with 4 mask bytes per transform track and 4 per float track
(the u32 at +68). Then, track by track, translation, rotation and scale:

| Mask byte | Content |
| --- | --- |
| 0 | bits 0-1 translation code, 2-5 rotation code, 6-7 scale code: 69 (codes 1, 1, 1) in 1,287,665 track-blocks, 73 (1, 2, 1) in 135,732 |
| 1, 3 | translation, scale: bits 0-2 static x, y, z; bits 4-6 spline x, y, z; bits 3 and 7 never set; no component both static and spline (1,423,397 each) |
| 2 | rotation: low four bits static, high four bits spline |

- **Translation or scale without a spline bit**: one f32 per static
  component, in x, y, z order.
- **With a spline bit**: u16 m, u8 degree, m + degree + 2 knot bytes, zero
  bytes to a multiple of 4, then per component in x, y, z order two f32
  (minimum, maximum) if spline or one f32 if static, then m + 1 rows of one
  u16 per spline component, then zero bytes to a multiple of 4.
- **Rotation**: when any of the high four bits is set, u16 m, u8 degree,
  m + degree + 2 knot bytes, then m + 1 quantised rotations; otherwise, when
  any low bit is set, one quantised rotation; otherwise nothing. Then zero
  bytes to a multiple of 4. A quantised rotation is 5 bytes for code 1 and
  6 bytes for code 2. Before 6-byte spline control points the position is
  first taken to a multiple of 2.
- The transform data ends exactly at the block's u32 of the array at +104
  (the float offset), and the bytes from there to the block end are zero
  when there is no float track.

Under these rules the transform data of all 16,063 blocks ends exactly at
its float offset, with every alignment and trailing byte zero (enforced,
`InvalidHavokSplineBlock`). Alternatives fail: knots m + degree + 1 bytes
(1,259 blocks consumed exactly), m + degree + 3 (6,193), rotation sizes 6
and 5 swapped (593), 4 and 6 (1,565); a 4-byte alignment before rotation
data (235 of a 3,304-block sample). Consumption cannot see the 2-byte
alignment before 6-byte rotations (the end alignment to 4 absorbs it); the
values decide it: without it 199,997 of 1,416,402 6-byte control points
have stored components whose squares sum above 1 (impossible for a unit
quaternion, below), with it none. Every 6-byte static rotation already
starts at an even position, so whether the alignment applies there is not
decidable; the reader applies it, which changes nothing in the measured
data. Translation and scale code 1 is the only one stored; the reader
refuses other codes rather than guess their size.

Curves: degree 1 (473,496 rotation, 55,221 translation, 106 scale curves)
or 3 (130,019, 38,236, 389). In every curve the first degree + 1 knots are
0, the last degree + 1 equal the block's frames minus 1, and the knots do
not decrease (enforced): a clamped knot vector in frames of the block, with
as many knots as control points plus degree plus 1.

Track-blocks by kind: translation static 1,111,090, spline 93,457, absent
218,850; rotation static 508,907 (440,079 of 5 bytes), spline 603,515
(570,847 of 5 bytes), absent 310,975; scale absent 1,422,782, spline 495,
static 120.

### Values

- **Quantised rotation, 5 bytes** (40 bits, little-endian): three 12-bit
  fields at bits 0, 12 and 24 read as `(v - 2047) / 2047 / sqrt 2`; bits 36
  and 37 the index (x, y, z, w = 0 to 3) of the component the fields leave
  out, the fields filling the other three in order; bit 38 its sign; bit 39
  never set. The left-out component is `sqrt(1 - sum of squares)`.
- **Quantised rotation, 6 bytes**: three u16, the low 15 bits read as
  `(v - 16383) / 16383 / sqrt 2`; the top bits of the first two words are
  the index (first word low), the third word's top bit the sign.
- Evidence: in all 39,506,983 quantised rotations the fields' squares sum
  to at most 1 (largest norm deviation of the result 5.1e-8), and in the
  sampled control points below the left-out component is always the
  largest in magnitude. Over 7,549,551 5-byte
  control points of a sample, consecutive points of a curve differ by
  1 - |dot| above 0.1 in 0.050 % of pairs under this reading; with the sign
  at bit 39, no sign, the index at bits 37-38 or 38-39, the fields in cyclic
  or reverse order, the share is 0.28 % to 0.46 %, and fields at bits 4, 16
  and 28 give 71 %. For 6-byte points the chosen index and sign words give
  0.32 %, the other five assignments 1.07 % to 1.55 %. In the decoded idle
  and other clips below, 54,099 and 330,139 tracks are within 0.1 degree of
  the skeleton's reference rotation at frame 0. Whether the stored constants
  are exactly 2047, 16383 and 1/sqrt 2 in the writer's arithmetic is not
  established; they reproduce the reference rotations within the
  quantisation step.
- **Control points of translation and scale**: `min + (max - min) * v /
  65535` per spline component.
- **Curve**: the clamped B-spline of the stored degree, knots and control
  points evaluated by de Boor's recursion at u = frame within the block;
  rotations evaluated on the four components, then normalised. Only 4 of
  38,394,557 consecutive rotation control points have a negative dot
  product, so component-wise evaluation does not cross between q and -q.
- **Time**: frame `t / frame duration`, clamped to the clip; block
  `floor(frame / (frames per block - 1))` (at most the last); u = the frame
  minus the block's first frame. Consecutive blocks share their boundary
  frame.
- **Absent components**: translation 0, rotation identity, scale 1.

Checks of the values:

- **Block boundaries.** At every boundary of the 696 objects with more than
  one block, the last frame of block j against frame 0 of block j + 1
  (largest difference): rotation spline to spline 3.4e-6 degrees over
  37,372 boundaries, static or spline mixes at most 0.15 degrees, absent to
  anything 0; translation static to static 0.0024 units (64,872), spline to
  spline 0.092 (3,420), absent on one side at most 0.0020 (186); scale at
  most 0.0022. Since a component can change kind between blocks, the
  boundaries where one side is absent (157 rotation, 186 translation, 11
  scale) are what establishes the absent values above.
- **Interior of curves.** Pairs of spline objects with the same file name,
  frame count and track count were compared track by track where both store
  a rotation curve with different knots and the two agree at both block
  ends (19,577 track pairs): the evaluated rotations agree at every frame
  within 1 - |dot| below 1e-5 (0.5 degree) for 54.3 % of pairs with the
  stored knots and 28.3 % with uniformly spaced knots of the same count;
  2,393 pairs agree with the stored knots and not with uniform ones (above
  1e-4), 15 the reverse. Many such pairs are different motions, so this
  supports the stored-knot evaluation without proving it frame by frame.
- **Frame 0 against the reference pose**, for clips whose binding fits a
  race skeleton of their folder (the first that fits; 2,000 clips with
  "idle" in the file name, 11,956 others; one comparison per track):
  translation within 0.001 of the bone's reference translation for 137,565
  of 179,299 idle tracks (77 %), rotation within 1 degree for 57,932 (32 %)
  and 45 degrees or more for 28,513. A pose differs from the reference pose
  where the clip moves the bone, so this bounds nothing; it shows the static
  translations reproducing the bone offsets. In one dog idle of 65 tracks,
  57 translations equal the reference translation within 0.001 (not the
  body's spline, one body bone and six mouth bones) and 33 rotations are
  within 1 degree.

Not decoded: float tracks (the 4 objects with one store 20, 40 or 60 bytes
after the float offset), translation or scale codes other than 1 (none
stored), the lossless class, and the reference-frame object (an array
of 16-byte samples at +72 whose count equals the frame count in 4,922 of the
spline objects of a 6,000-object sample that link one; not read).
