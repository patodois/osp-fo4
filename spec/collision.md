# Fallout 4 collision blocks

First step toward walking inside a cell: the NIF reader now carries the
collision blocks instead of skipping them, and a census measures what they
hold across one installation. Nothing here decodes the embedded Havok data or
runs physics.

Evidence tags used below:

- **[doc]** a format fact from nifxml, the schema already pinned in
  [nif.md](nif.md) (SHA-256 `752c7978…bbf0d`, XML 0.10.0.0). Only the data
  definitions are used, as for the rest of the NIF reader.
- **[measured N]** a count from the census, over N files or blocks of the
  installation described in [Census scope](#census-scope). It holds for that
  data only.
- **[hypothesis `name`]** a registered rule in [hypotheses.tsv](hypotheses.tsv),
  `pending` until a measurement of the running game decides it.
- **[open]** not established. Nothing in the code relies on it.

## Census scope

`fo4-unpack sweep --scope collision --manifest PROFILE --out DIR` writes the
usual `summary.json`, `diagnostics.jsonl` and `items.jsonl` (per-model and
per-record rows, local only), plus `collision.tsv`. The committed copy,
[collision-census.tsv](collision-census.tsv), holds counts and small stored
header values only. Its `census` rows identify the input: build declared as
1.10.163 in the manifest, 7 plugins (base game and six expansions), 62 archive
mounts (every BA2 of the installation), VFS fingerprint `a4316448…bc100`.

- 224,368 winning `.nif` paths, 224,366 read **[measured 224,368]**. The two
  failures are `UnsupportedNifBsVersion` (BS 132 and 139), outside the reader's
  profile.
- 176,065 winning non-placement records scanned; the 39,188 that belong to the
  17 static-frame base types were joined to their model. Model paths are read
  only where the plugin reader already interprets MODL (the static-frame base
  types), so WEAP, ARMO, ARMA, NPC_ and similar records are not covered.
- Run on the 16 GB Mac: 102 s wall, peak RSS 673 MB (`/usr/bin/time -l`);
  archives are read entry by entry through the VFS.

## Blocks and fields

| Block | Fields (order as stored) | Source |
| --- | --- | --- |
| `bhkNPCollisionObject` | Target (Ptr, i32), Flags (bhkCOFlags, u16), Data (Ref to bhkSystem, i32), Body ID (u32) | **[doc]** NiCollisionObject + bhkNPCollisionObject, `#FO4# #F76#` |
| `bhkPhysicsSystem` | Binary Data: ByteArray = Data Size (u32) + that many bytes | **[doc]** bhkSystem + ByteArray, `#FO4# #F76#` |
| `bhkRagdollSystem` | same ByteArray | **[doc]** |

- The layouts are read only in the FO4 stream (BS 130). In another profile the
  blocks stay raw and `Nif::collision` returns `UnsupportedNifCollisionProfile`.
- `Nif::collision` returns the objects field for field, the systems with the
  file offsets of `Data Size` and of the first payload byte and the payload as
  borrowed bytes, and every NiAVObject whose `Collision` link is set. A
  truncated or oversized ByteArray is a `Truncated` diagnostic on the block,
  with its offset; a `Data` link to a block that is not a bhkSystem subclass is
  `InvalidNifCollisionData`. Bytes after the ByteArray stay in the block's raw
  tail and are counted.
- bhkCOFlags names bit 3 `SET_LOCAL` and bit 7 `SYNC_ON_UPDATE` **[doc]**. The
  schema documents names, not behaviour; the reader keeps the raw u16.
- The three types are `outside` in [static-blocks.tsv](static-blocks.tsv): the
  CPU frame of the example interior is byte-identical before and after this
  change.

## What the census shows

### Which blocks occur

- The only collision-related block types in the data are the three above
  **[measured 224,366 files]**: 333,526 `bhkNPCollisionObject`, 275,154
  `bhkPhysicsSystem`, 188 `bhkRagdollSystem`. No `bhkCollisionObject`,
  `bhkRigidBody` or other `bhk*` block occurs. `BSClothExtraData` (409 blocks)
  also wraps a ByteArray but is cloth, not collision, and stays raw.
- 85,919 files have collision objects; every one also has at least one
  bhkSystem, and 138,447 have no collision block at all. 152 files have only a
  ragdoll system, 36 have both kinds **[measured 224,366 files]**.

### Links

- Every object's `Data` is set: 331,937 point to a `bhkPhysicsSystem`, 1,589
  to a `bhkRagdollSystem` **[measured 333,526 objects]**.
- Every object's `Target` is set and the target's `Collision` link points back
  to that object. Exactly one NiAVObject links to each object, and it is the
  target. Targets are `NiNode` (333,364), `BSLeafAnimNode` (160) and
  `BSMultiBoundNode` (2); 27,644 targets are a footer root **[measured 333,526
  objects]**.
- Every bhkSystem is used by at least one object. 239,839 physics systems have
  one object; the most shared has over 100 **[measured 275,342 systems]**.
- On every system, the Body IDs of its objects are distinct and below the
  number of objects on it, so they are exactly 0..k-1 **[measured 275,342
  systems]**. What a Body ID selects is
  **[hypothesis `nif_collision_body_id_selects_body_v1`]**.
- Flags take two values only: `0x0080` (91,134) and `0x0088` (242,392)
  **[measured 333,526 objects]**.

### The embedded payload

All statements below hold for both system kinds unless a count says otherwise
**[measured 275,154 physics and 188 ragdoll payloads]**.

- Sizes: physics 928 B to 8,536,496 B, median 2,880 B, 90th percentile 6,624 B,
  2.06 GB in total but only 53,827 distinct byte strings (SHA-256). Ragdoll
  2,848 B to 102,544 B, 167 distinct. No payload is empty, and no block has
  bytes after its ByteArray.
- The first 88 bytes are identical in every payload. The first four are
  `57 e0 e0 57`. Bytes 40 to 53 hold the NUL-terminated text `hk_2014.1.0-r1`.
  Word 20 (u32 LE) is 3.
- Three 64-byte records follow at offsets 64, 128 and 192. Each starts with a
  NUL-terminated name (`__classnames__`, `__types__`, `__data__`) followed by
  zero bytes; byte 19 of the record is `0xff`, bytes 20 to 47 are seven u32
  words, bytes 48 to 63 are all `0xff`. Word 84 is always 256 (64 + 3 × 64).
- The u32 LE at 148 equals the one at 212, and either plus each of the words at
  228, 232 and 236 equals the payload length, in every payload. The words at
  152 to 172 are always zero.
- Every payload contains the NUL-terminated names `hkClass`, `hkClassMember`,
  `hkClassEnum` and `hkClassEnumItem` at 261, 274, 293 and 310, then
  `hknpPhysicsSystemData` or `hknpRagdollData` at 331. Each of these names
  starts five bytes after the end of the previous one (261 = 256 + 5). Shape and other
  data names follow at varying offsets, for example `hknpCompressedMeshShape`
  (154,809 physics payloads), `hknpConvexPolytopeShape` (142,160),
  `hknpCapsuleShape` (183 ragdoll payloads) and constraint names in ragdolls.
  The committed TSV lists every name present in at least half the payloads of
  a kind.

These are measurements of stored bytes. The census does not decide that the
records are sections, that the words are offsets or sizes, or what the names
describe.

### Models and records

Of the 39,188 static-frame base records **[measured 39,188 records]**:

| | Records |
| --- | ---: |
| model with a bhkPhysicsSystem | 32,523 |
| model with no collision block | 4,811 |
| no model path | 1,528 |
| cited model absent from the VFS | 325 |
| model that failed to read | 1 |

No base model has only a ragdoll system or an object without a system. The
per-type split is in the `records` rows of the TSV; STAT has 3,642 bases whose
model has no collision block, MSTT 293, FURN 256, CONT 165, ACTI 156. Whether
such a model really lets the player through is
**[hypothesis `nif_physics_system_blocks_player_v1`]**.

## Open questions for a payload decoder

1. Is the 64-byte block at 0 a fixed file header? Its nonzero words (at 0,
   4, 12, 16, 20, 24, 36 and 60) are constant over the whole corpus, so the
   census cannot tell which are magic, version, counts or flags. Is word 20 the number
   of named records? **[open]**
2. For each named record at 64/128/192: which word is the start, which the end,
   and what the six words after the start mean. Word 84 equals 256 and word 148
   equals 256 + word 88 in the six most frequent values; is the start absolute
   and the rest relative to it? Why are the three words at 228/232/236 equal
   in every payload? **[open]**
3. `__types__` is empty in every payload (words 152 to 172 are zero). Does the
   format then rely on `__classnames__` alone to name the objects in
   `__data__`? **[open]**
4. How is `__classnames__` laid out: what are the five bytes before each
   name, and are they a per-class signature, an index or a length? **[open]**
5. How does `__data__` locate each object, its class and its pointers to
   other objects: are there fix-up tables after the object bytes, and where do
   they start? **[open]**
6. What is the byte order of the object data, and is it the same for every
   payload? The header words read as little-endian give small values; the
   first four bytes are a palindrome and do not decide it. **[open]**
7. Which bodies does a `hknpPhysicsSystemData` hold, in which order, and does
   that order match Body ID 0..k-1? **[hypothesis
   `nif_collision_body_id_selects_body_v1`]**
8. For `hknpCompressedMeshShape` and `hknpConvexPolytopeShape`: units,
   quantization, the space the vertices are in relative to the Target node,
   and the material per triangle (`hknpBSMaterialProperties`). **[open]**
9. What do Flags bit 3 (`SET_LOCAL` in the schema) and bit 7 change at
   runtime, given that only 0x80 and 0x88 occur? **[open]**
10. For walking: does a model without a collision block (4,811 bases) really
    let the player through, and does a ragdoll system collide while its actor
    is alive? **[hypothesis `nif_physics_system_blocks_player_v1`]**, **[open]**

A decoder must answer these from public documentation or from measurement of
the stored bytes and the running game. The Havok SDK, its headers and
third-party readers of the format stay off limits.
