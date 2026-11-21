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

For each scene instance ready at the requested level (the renderer's own
test), the reference transform is `Transform::euler` of DATA and XSCL, as for
the render mesh; a SCOL with parts places its constituents with their part
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
