# Collision of a cell: placement rules and report

Sixth step after [havok-mesh.md](havok-mesh.md): put the collision that
`fo4-havok` decodes into world space, for every placed reference of a cell.
`fo4_scene::collision::cell` returns the instances; `fo4-unpack
cell-collision` reports them. Nothing here runs physics.

The stored geometry is a format fact only to the extent of the havok specs:
each reader enforces what held on every stored object. **Placing** it is not:
every step below is a registered rule in [hypotheses.tsv](hypotheses.tsv),
`pending` until a measurement of the running game records otherwise. An
instance is returned only at a rule level that accepts every rule it relies
on (`fo4_scene::RuleLevel`, the same mechanism as the renderer); otherwise
the body is listed as not placed, with the rule names. No Havok SDK, header,
documentation, converter, decompilation or third-party reader was consulted
(see [provenance](../docs/provenance.md)).

## From reference to shape

For each reference whose collision readiness accepts the requested level
(below; not the renderer's readiness), the reference transform is
`Transform::euler` of DATA and XSCL, as for the render mesh; a SCOL with parts places its constituents with their part
placements under `scol_parts_static_frame_v1`, as the renderer does, and its
own model is then not used. Each model is decoded once per cell:

1. `Nif::collision` gives every `bhkNPCollisionObject` (Target, Flags, Data,
   Body ID) and its `bhkPhysicsSystem`. A `bhkRagdollSystem` is not placed
   (`RagdollSystem`).
2. The Target's chain is found by following node child links up to a footer
   root; a node with two parents (`AmbiguousCollisionTargetChain`), a cycle or
   a chain that does not end at a root is refused by name. The chain is
   composed exactly as the render walk composes node transforms (a block's
   stored transform when it has a `Translation` field).
3. Body ID i selects body i of the physics system
   (`nif_collision_body_id_selects_body_v1`); a Body ID past the bodies is
   `BodyIdOutOfRange`, a body that no collision object names is
   `BodyWithoutCollisionObject`.
4. The body's shape link is followed through `hknpStaticCompoundShape` and
   `hknpDynamicCompoundShape` instances and `hknpScaledConvexShape` to the
   leaves: `hknpCompressedMeshShape`, `hknpConvexPolytopeShape`,
   `hknpConvexShape`, `hknpSphereShape`, `hknpCapsuleShape`. Any other class
   is `UnsupportedCollisionShapeClass`; a compound instance whose three scale
   values differ by more than 1e-6 relative is
   `NonUniformCompoundInstanceScale`.

A leaf point p (collision units) reaches world space as

```
world = R ∘ chain(root → Target) ∘ S(69.99125) ∘ B ∘ steps(p)
```

with R the reference (and SCOL part) transform, S a uniform scale, B the
body placement of the selected level and `steps` the compound instances and
scaled convex links from the leaf up to the body.

| Rule | What it decides | Default candidate |
| --- | --- | --- |
| `havok_collision_units_69_99125_v1` | collision unit to world unit | × 69.99125 |
| `havok_shape_in_target_frame_v1` | the frame the shape sits in | Target node chain, reference transform as for render |
| `havok_body_placement_ignored_v1` / `havok_body_placement_applied_v1` | the body's stored translation and four f32 | two levels, see below |
| `nif_collision_body_id_selects_body_v1` | which body a collision object names | body i |
| `havok_compound_instance_v1` | compound instance rows, translation, scale | rows are the images of the child axes |
| `havok_scaled_convex_v1` | scaled convex scale and translation | scale × p + translation per axis |
| `havok_convex_radius_added_v1` | polytope f32 at byte 20 | a radius added to the hull, scaled with the placement |
| `havok_mesh_triangles_surface_v1` | compressed-mesh triangles as the surface | [a, b, c] (+ [a, c, d]); sidedness open |

### The open body placement rule

The body element stores a translation (bytes 48) and four f32 of unit norm
(bytes 64). Whether they move the shape is open, so the rule has two levels,
`BodyPlacement::Ignored` and `BodyPlacement::Applied`, each its own row.
`default_body_placement` reads the registry: no level at `Facts`; at
`Measured` the one level whose row is `supported`; at `Hypotheses` a
supported level, otherwise the first pending one in the candidate order
(ignored, then applied: the level the first live run ranked best comes
first). A refuted level is never chosen. Every instance can be evaluated
under either level (`CellCollision::world`), and the report always shows
both. `Applied` reads the four f32 as x, y, z, w with w last; the order
w, x, y, z was refuted by the live run, other orders stay open.

A body counts as storing the identity placement when its translation and
first three f32 are within 1e-6 of zero and the fourth within 1e-6 of ±1
(stored identity quaternions such as w = 0.99999994 occur). For such bodies
the two levels coincide.

## Collision readiness

A reference's collision needs less than its drawing: the reference and base
records, the placement rules they use (rotation, scale, SCOL part frames),
the model file and its collision payload. A blocker on an input only the
renderer reads (a material swap, a material or texture file, a shader or
visibility field) does not stop the collision. The scene keeps two states per
instance: `Instance::state`, the render readiness, unchanged and still the
only one the renderer reads, and `Instance::collision` (module
`fo4_scene::readiness`), which the collision assembly reads.

This was motivated by a live drop comparison in one interior cell: settled
player heights from drops at a list of points, against the vertical-ray
surface of the assembled collision. The assembly skipped every reference not
ready for rendering; one static collection, not drawn because its reference
carries a material swap with a conditional filter, was absent, although in
the running game the player stands on it. With that reference placed by a
throw-away diagnostic, 34 of 38 settled readings agreed with the surface
within 0.05 units, against 8 of 38 without it.

Each blocker is recorded with where it was raised
(`Instance::blocker_origins`, `readiness::Origin`):

| Origin | What | Collision |
| --- | --- | --- |
| `Record` | the reference, its base records, SCOL parts, the records' own model citation | by code (below) |
| `MaterialSwap` | a material swap of the reference or of a base: the MSWP record, its filter, its material citations | render only |
| `Model` | a model file: its read, blocks and fields | by code (below) |
| `ModelCitation` | a material or texture citation inside a model, or the file it names | render only |
| `RenderAsset` | a material or texture file, or anything it cites | render only |

For `Record` and `Model`, `readiness::Code::collision_use` classifies every
blocker code the loader raises in one exhaustive match, so a new code cannot
be added without a decision; a code it does not name (a reader's own error)
counts as needed. A test reads the loader's sources and fails on a code
literal that is not classified.

| Codes | Collision |
| --- | --- |
| `MissingPlacementTransform`, `RuntimeEnableParent`, `RuntimeCondition`, `MissingBaseForm`, `MissingRecord`, `BaseTypeOutsideSlice`, `ScolCycle`, `DeletedBaseForm`, `MissingModelPath` | needed: placement, presence at run time, the base |
| `UnverifiedEulerRotation`, `MissingDocumentedReferenceScale`, `UnverifiedScolGeometrySource`, `MissingScolPartBase`, `MissingScolPlacements` | needed: placement rules (rotation, absent XSCL, SCOL part frames) |
| `RuntimeRecordState`, `UnsupportedUsedFieldLayout`, `UninterpretedVisualSubrecord` | needed: an uninterpreted record field could bear on placement (MODT keeps its own rule) |
| `HypothesisAssetDataPrefixStrip`, `HypothesisAssetDataRelativeStrip`, `HypothesisAssetMissingRoot`, `RejectedVfsPath`, `PathCandidateUnavailable`, `PathCandidateWrongRoot` | needed when raised on a record (the path of the model itself); render only from the other origins |
| `MissingModel`, `AssetDependencyCycle`, `UnsupportedNifBlockSemantics`, `UnsupportedAnimationRuntime`, `UnsupportedNifBlock`, `UnsupportedNifBlockTail`, `UnsupportedNifField` | needed: the model file reaches the node chain and the collision blocks; an animation can move a collision Target |
| `AllocationFailed`, `AssetCacheUnavailable`, `InvalidAssetIndex`, `LimitExceeded`, `Overflow`, any other code | needed |
| `DeletedMaterialSwap`, `MissingMaterialSwap`, `RuntimeMaterialSwapFilter` | render only |
| `MissingMaterial`, `MissingTexture`, `UnsupportedMaterialFields`, `UnsupportedShaderMaterialName`, `UnverifiedEffectColorOnly`, `UnsupportedLightData`, `UnsupportedSkinning` | render only |
| `UnverifiedVisibilityEditorMarker`, `UnverifiedNifEditorMarkerVisibility`, `UnverifiedNifEditorMarkerSuffixVisibility` | render only (visibility of helper geometry) |

A needed blocker counts exactly as for rendering: its covering rule is relied
on, a refuted or missing one blocks. Setting a render-only blocker aside is
itself a claim about the game, so the collision then relies on a registered
rule, both `pending`:

| Rule | Relied on when |
| --- | --- |
| `mswp_collision_unchanged_v1` | the reference, its base or a SCOL part base names a material swap (whether or not the swap raised a blocker: collision never applies it), and for every blocker a swap raised |
| `render_inputs_collision_unchanged_v1` | any other render-only blocker, unless the blocker's own render rule stands better (supported against pending: a material or texture path read under a supported path rule keeps that rule, so readiness at `Measured` does not change) |

A rule that is refuted, or a render-only blocker with neither rule usable,
blocks the collision. Placement rules are gated exactly as before: a SCOL's
parts are placed only when the collision state relies on
`scol_parts_static_frame_v1`, an absent XSCL is read as 1 only under
`refr_absent_xscl_identity_v1`, and every rule of the collision state is
listed in each instance's rules and in the placement's `scene_rules`, which
now hold the collision state's rules, not the render state's. A reference
whose collision is not ready is listed as unplaced with
`CollisionNotReady` (formerly `SceneInstanceNotReady`, which reported the
render state) and its collision state as detail. Conservative choices: the
blockers of a SCOL's own generated model count as needed even when its parts
are placed instead; a swap's material files count as render inputs under
`render_inputs_collision_unchanged_v1` as well as the swap rule.

### Blockers off the collision chain

The opening's walk (`fo4-play`, `spec/play.md`) found holes in its walking
world where the cell build refuses a model's collision for blockers the
collision never reads: particle systems, unread blocks and fields, and
animation blocks that move nothing the bodies hang on. Behind switches, off
by default (`readiness::Switches`, `Loader::set_collision_switches`), the
cell build asks a second reading of an instance whose collision is refused,
and only of such an instance (an instance already ready keeps its state and
rules byte for byte):

- `off_chain` (`collision_model_blockers_off_chain_v1`): the model decode
  records which blocks its placed bodies read (`collision::model::Reads`:
  every node of each body's chain from a root to its Target, the
  `bhkNPCollisionObject`, its `bhkPhysicsSystem`) and the blocks that can
  move a chain node (an animation-state block whose `Target` or
  `Extra Target` names a chain block, the own `Target` of a controller
  manager or a multi-target transform controller left out as the object it
  hangs on; a controller sequence whose controlled block names a chain
  node). A model blocker of the five codes raised on a block
  (`UnsupportedNifBlock`, `UnsupportedNifBlockSemantics`,
  `UnsupportedNifBlockTail`, `UnsupportedNifField`,
  `UnsupportedAnimationRuntime`) lies off the chain when its offset falls in
  a block no body reads; an animation blocker only when nothing moves a
  chain node (`readiness::bearing`, `readiness::Bearing`: off-chain,
  on-chain, chain-moved, no body, unlocated). An off-chain blocker is set
  aside under the rule; every other blocker counts as before.
  The model a blocker was raised in is the instance's model node holding
  that blocker; the reads are computed only when a switch asks them, so a
  build with every switch off keeps its memory accounting byte for byte.
- `reference_alpha` (`refr_xalp_collision_unchanged_v1`): a reference's
  unread `XALP` (two bytes; xEdit's published name: alpha) is set aside.

A set-aside blocker relies on the better standing of its own covering rule
and the switch's rule, as a render-only blocker does
(`readiness::collision_state_set_aside`). The placed instance lists the rule
in its rules and the placement's `scene_rules`.

Census (`collision_ready_census`, `spec/collision-ready-census.txt`): the
seven cell loads of the opening's walk (the house and the cells the walk
crosses outside, each with the cells about it; the vault) refuse 900
references, 299 of them with collision. The off-chain switch settles 12 (11
statics, 1 movable static), among them the vault entrance room's floor past
the interior platform; the alpha switch 3 more, among them two pieces of the
vault corridor's floor. The other 284 stay refused: 268 by record fields
and script conditions alone (`UninterpretedVisualSubrecord` of activator,
furniture, door, container and terminal fields, `RuntimeCondition` of a
script, `RuntimeRecordState`), 16 by animation that moves a chain node
together with record fields (in each, a controller sequence names a chain
node and a multi-target controller has one among its extra targets).

The gear door at the vault's entrance (census section "Animated reference
3"): an activator whose model holds 24 placed bodies and four controller
sequences. The stored pose is the pose of the first sequence and the start
of the second; the second runs 43.3 s of translation keys to a pose 137 to
648 units away; the third and fourth link no translation keys and their
interpolators store exactly that end pose. Its graph's start state plays the
first sequence (its name matches ignoring case only); a wildcard transition
on the graph's last event enters the state that plays the fourth. The
reference's one script plays that event when its 3D loads (its load event in
its waiting state). What the data says, then: at load the door stands
closed, and its load event leads to the open pose (the fourth sequence
holds it). That an interpolator without keys holds its node at its own
stored transform is the pending rule
`nif_sequence_interpolator_held_pose_v1` (`sequence::held_pose`); the load
event, the graph and the moving collision are the run side's
(`fo4-play`'s elevator reading takes translation keys only).

`ride_check` (the walker over the vault cell, from the entrance room's floor
at 520.67 s along the recorded walk's targets to 700 s): with every switch
off the walker falls (no floor); with `--collision-off-chain` it reaches
the entrance room's edge and drops 190 units into the pit before the gear
door; with `--collision-off-chain --collision-reference-alpha --pose DOOR
3` (the door's collision with each chain node moved by the fourth
sequence's held translation, rotations not applied) it crosses the door's
bridge and the corridor and reaches every target of the recorded walk up to
700 s (the last at 679.8 s), grounded. The play mode does not set these
switches or follow held poses (`fo4-play` belongs to the run side): a local
probe of that change, outside the repository, matched the recorded run's
232 of 232 stages to 700 s with the door's load event raised
(`--load-on-arrival`).

## What the API returns

`CellCollision` holds the decoded models (`ModelCollision`: bodies with their
Target, chain, stored body, leaves and the render meshes of the Target
subtree; refusals with a code), one `ModelPlacement` per reference and model
(the model's world frame and the scene rules), the `CollisionInstance`s (one
per leaf: shape class, the body-to-world transform under the selected level,
every rule relied on), `Unplaced` rows (reference, model, collision object,
system, body, leaf, code, detail) and the placed models whose file holds no
collision object.

Typed world views (`world_shape`): compressed meshes give vertices,
triangles in stored winding with their unit normals (b − a) × (c − a), and
vertex records as points with a radius; polytopes give their vertices and the
byte-20 radius; `hknpConvexShape` its vertices only (no radius is read);
spheres a centre and radius; capsules two points and a radius. No faces are
formed for polytopes or convex shapes, whose faces are not established, and
nothing assumes a triangle blocks from both sides. Radii scale with the
uniform part of the placement only; a per-axis scaled convex leaves its
radius unscaled by that link (`Leaf::per_axis_scaled`).

## Report

`fo4-unpack cell-collision PROFILE.json EditorID [--out DIR]
[--strict|--measured|--under-hypotheses] [--overlay-plugin FILE]` writes
`summary.json` (`cell_collision`), `items.jsonl` and `cell_collision.tsv`:

- totals: references with a model, with collision, with a model and no
  collision; distinct models; placed bodies and instances; instances per
  leaf class and per class path; triangles, mesh vertices, records;
- `unplaced` per code; `collision_body` items per body (stored placement,
  Target, chain, leaves, rules, world box and agreement under each level);
- `triangle_normals`: world normals of the triangles under the selected
  level, up (z > 0.5), down, side, degenerate;
- `body_translation_length` and `body_translation_to_target_origin`: for
  bodies with a stored translation, |69.99125 t| and its distance to the
  Target's model-frame origin, in world units;
- per level: the rule's registry status, the union of the world boxes, and
  the agreement of each body's world collision box with the world box of the
  render meshes under its Target (or of the whole model when the subtree has
  none): the smallest per-axis overlap / union with both boxes widened by 0.5
  units on every side, the distance between the centres, and whether the
  collision box lies inside the render box widened by 1 unit plus 1%. The
  same distributions are given for the bodies whose stored placement is not
  the identity, where the levels differ.

The agreement is offline evidence about the open rules, not a proof: render
and collision meshes may legitimately differ.

The `render` example draws the placed instances as a magenta wireframe over
the frame with `--collision-overlay`, into a separate `collision.png`;
`frame.png` and `summary.json` are byte-identical with and without the flag
otherwise.

## First run

Same installation as the havok specs (build declared as 1.10.163, 7 plugins,
62 archive mounts, VFS fingerprint `a4316448…bc100`), at `--under-hypotheses`,
on one interior test cell and one exterior cell:

| | Interior | Exterior |
| --- | ---: | ---: |
| scene placements | 454 | 390 |
| not ready in the scene / editor marker | 113 / 1 | 79 / 0 |
| references with a model, with collision | 340, 340 | 311, 143 |
| references whose models hold no collision | 0 | 168 |
| distinct models, with collision | 14, 13 | 116, 64 |
| placed bodies / instances | 348 / 348 | 165 / 214 |
| compressed mesh / polytope instances | 340 / 8 | 133 / 81 (52 through a dynamic compound) |
| triangles | 5,983 | 6,953 |
| bodies with a stored placement | 14 | 7 |
| model errors, bodies refused by the decoder | 0, 0 | 0, 0 |

On the bodies with a stored placement, the collision box lies inside the
render box in 14 of 14 (interior) and 6 of 7 (exterior) bodies with the level
`ignored`, against 0 of 14 and 1 of 7 with `applied`; the centre offset is
below 8 units in 13 of 14 and 6 of 7 with `ignored`, in 8 of 14 and 4 of 7
with `applied`. This agrees with the live run's ranking and is not a
measurement of the game.

Decoding and placing took about 0.01 s for either cell after the scene load
(about 1.5 s); peak RSS of the whole command was 846 MB (interior) and 668 MB
(exterior), most of it the plugin index and the content fingerprint
(`/usr/bin/time -l` on the 16 GB Mac).

## Run with collision readiness

Same installation and cells as the first run, before and after collision
readiness replaced the render readiness in the assembly:

| | Interior before | after | Exterior before | after |
| --- | ---: | ---: | ---: | ---: |
| references not followed (`SceneInstanceNotReady` / `CollisionNotReady`), `UnderHypotheses` | 113 | 88 | 79 | 76 |
| references with a model, with collision | 340, 340 | 365, 349 | 311, 143 | 314, 146 |
| placed bodies / instances | 348 / 348 | 365 / 365 | 165 / 214 | 168 / 217 |
| compressed mesh / polytope instances | 340 / 8 | 357 / 8 | 133 / 81 | 136 / 81 |
| triangles | 5,983 | 9,878 | 6,953 | 8,118 |
| references not followed, `Measured` | 126 | 133 | 147 | 151 |
| references not followed, `Strict` | 453 | 453 | 390 | 390 |

At `UnderHypotheses` the newly followed references were blocked for
rendering only by render-only blockers (missing textures, effect and editor
marker visibility, a filtered material swap, a missing material); none
became unfollowed. At `Measured` nothing is placed either way (no body
placement level is supported); the references that name a material swap
(7 interior, 4 exterior) now wait for `mswp_collision_unchanged_v1` and so
move from `BodyPlacementRuleNotAccepted` to `CollisionNotReady`. At
`Strict` only the code changes. The rendered frames are byte for byte
the same; the render summary differs only in the memory accounting of the
new per-instance records. With the interior's live drop list, the walker's
drop report now agrees with the throw-away diagnostic at every point
(34 of 38 settled readings within 0.05 units of the ray surface).

## Open

| Question | Status |
| --- | --- |
| Units for classes other than a polytope box | **[hypothesis]** |
| Target node chain as the frame | indicated by the live run, **[hypothesis]** |
| Body placement | two levels; `ignored` measured for one polytope, indicated for meshes |
| Quaternion component order for `applied` | w, x, y, z refuted; otherwise open |
| Compound instance rows against columns | **[hypothesis]**; instances with the identity rotation do not decide it |
| Scaled convex order, radius under per-axis scale | **[hypothesis]** |
| Which side of a mesh triangle blocks | **[open]**: one plane of a two-plane mesh was passable from above |
| Vertex records (convex pieces with a radius) as collision | **[hypothesis]**; exposed as points and radius only |
| Editor-marker references and ragdoll systems | not placed, by name |
| Whether a material swap or a missing render input changes collision | **[hypothesis]**, two pending rules (Collision readiness) |
| Whether blocks off the collision chain, or a reference's alpha, change collision | **[hypothesis]**, two pending rules behind switches (Blockers off the collision chain) |
