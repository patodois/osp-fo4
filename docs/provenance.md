# Provenance and method

The archive, plugin, NIF, VFS, scene and CPU renderer code is original project
work. Public documentation and schemas establish binary facts; each spec records
its sources and the fields they support. No decompiled game code, game executable,
redistributed game asset or third-party game-reader implementation is a source.
Permitted schemas include nifxml; documentation includes format references,
UESP/modding documentation and independently checked field/byte measurements.
An external tool's printed data is evidence for the observed fields, not permission
to read or translate that tool's code.

`fo4-material` contains an explicitly attributed Rust port of MaterialLib's
BGSM/BGEM layouts at the exact commit recorded in THIRD_PARTY.md. This existing
MIT-licensed exception predates the current source restrictions. Its license
notice remains intact. No additional Material Editor implementation was consulted
for this work. The bounds, diagnostics, borrowing, IEEE preservation and synthetic
tests are local work.

The Fallout 4 collision readers use only the nifxml definitions of
`NiCollisionObject`, `bhkCOFlags`, `bhkNPCollisionObject`, `bhkSystem`,
`bhkPhysicsSystem`, `bhkRagdollSystem` and `ByteArray` (same pinned copy as
spec/nif.md). The embedded Havok payload is described only by counts measured
over the installed game ([spec/collision.md](../spec/collision.md)); no Havok
SDK, header, converter or other reader of that format was consulted.

`fo4-havok` reads the container level of that payload (header, three named
sections, class-name entries and the three tables of the data section). Its
layout and every rule it enforces were established only by measuring the
stored payloads of the installed game with `fo4-unpack sweep --scope havok`
([spec/havok-container.md](../spec/havok-container.md)). No Havok SDK, header,
documentation, converter, decompilation or third-party reader of the format
was opened or searched for; the field and table names are descriptive labels
chosen here. Interpretations the bytes cannot decide are marked as
hypotheses.

The object spans and the `hknpConvexPolytopeShape` view of `fo4-havok`
([spec/havok-objects.md](../spec/havok-objects.md)) were established the same
way: candidate rules were written down and counted over every stored payload
of the installation, and only those that held on every payload or object are
enforced. No Havok source of any kind was opened or searched for at this step
either.

The array element sizes, the body array of the top-level object and the
sphere, capsule, convex, scaled convex and compound shape views
([spec/havok-bodies.md](../spec/havok-bodies.md)) were established the same
way, by counting candidate rules and internal consistency checks (unit norm,
orthonormal rows, points inside the vertex box) over every stored payload.
Names such as body, rotation, radius and scale are labels for stored values;
their runtime meaning stays a hypothesis. No Havok SDK, header, documentation,
converter, decompilation or third-party reader was opened or searched for.

The `hknpCompressedMeshShape` and `hknpCompressedMeshShapeData` views
([spec/havok-mesh.md](../spec/havok-mesh.md)) were established the same way:
array slots from the first table, then candidate layouts counted over every
stored object, with the decoded vertices checked against the boxes stored
beside them and the decoded meshes compared with the render meshes of the
same files. Exploration used a sample of the stored payloads and a throwaway
script kept outside the repository; every enforced rule was then counted over
the whole installation. The box trees, byte 92 and the vertex records of
byte-93 sections were decoded the same way, from a local dump of the objects
that use them, by testing candidate readings against the stored section and
object boxes and against the render meshes; the census counts each reading
and its rejected alternatives. No Havok SDK, header, documentation,
converter, decompilation or third-party reader or schema of this format was
opened or searched for.

Placing that collision in world space for a cell
([spec/collision-placement.md](../spec/collision-placement.md),
`fo4_scene::collision`) adds no format fact: the unit factor, the Target node
frame, the two levels of the body's stored placement, the compound and
scaled convex links, the polytope radius and the use of mesh triangles are
registered rules, pending, whose defaults follow a first live run of the
owned game and the stored-byte censuses above. The offline comparison of
collision and render boxes in the `cell-collision` report is evidence only.
No Havok SDK, header, documentation, converter, decompilation or
third-party reader or schema was opened or searched for.

The LAND view (`fo4_plugin::land`, [spec/land.md](../spec/land.md)) uses
only the UESP Skyrim and Oblivion LAND, CELL and file-format pages (read as
wiki source) and the fopdoc Fallout 3 LAND page; none describes Fallout 4,
so layouts are applied only at the documented sizes and other sizes stay
raw. How the heights are accumulated, scaled, oriented, spaced and
triangulated, and which LAND belongs to a cell, are registered pending rules
whose candidates come from those pages; the edge-continuity and
objects-against-ground figures of the `land` report are offline evidence
only. No xEdit source or definition file, OpenMW, Mutagen or other reader
of the format was opened.

The ground textures (`fo4_scene::terrain::layers`, [spec/land.md](../spec/land.md),
"Ground textures") use no further source. The quadrant numbering and the
17 x 17 VTXT position grid come from the same UESP pages; the LTEX TNAM link
to a TXST, the use of TX00, the weight combination, the meaning of a missing
base, the texture repeat and the VCLR multiplication are registered pending
rules whose candidates come from offline measurement of the stored data of
one installation through this repository's readers (counts, seam and edge
continuity, texel density of placed objects), recorded in the spec. No
reader, editor or renderer of the format was opened for them.

The walking controller (`fo4-walk`, [spec/walk.md](../spec/walk.md)) is
original work from general geometry (closest points of convex sets,
collide-and-slide with substeps, step-up and ground snapping); it adds no
format fact and does not claim to reproduce the game's controller. Every
movement number is a registered pending rule with a provisional value. No
character-controller source, physics SDK, decompilation or third-party
implementation was opened or searched for. The game settings it lists are
read from the plugins as raw bytes and are not applied. The terrain it
stands on is the terrain of [spec/land.md](../spec/land.md), under the same
pending rules; walking on it adds no source.

Runtime experiments measure an owned running copy through separately collected
camera/transform words and repeated photographs. Their raw data stays local.
Behavior not established by a format source is registered in
[the hypothesis table](../spec/hypotheses.tsv) before use, with a falsifiable
measurement and an explicit status. Support for a convention does not establish
bit-exact arithmetic, a whole renderer, or unmeasured record types. Stable values
in two pauses do not prove absence of animation or authorize controller execution.

All default tests generate their own bytes. The optional transform fixture replay
is ignored unless explicitly requested; its input is not distributed. Real
installations are read-only. Images, extracted fragments, measurements and local
reports belong in ignored output folders and are never part of source artifacts.

Cargo.lock pins the dependency graph. THIRD_PARTY.md lists the package versions
and declared SPDX licenses verified against offline Cargo metadata. This is a
source and notice inventory, not a claim that unknown format behavior has been
established. AI assistance was used for implementation and documentation; test
results, limitations and measurement scope remain explicit.
