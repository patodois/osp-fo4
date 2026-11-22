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
hypotheses. The second header variant of the `.hkx` files (16 bytes before
the section records, declared by the upper half of header word 60) was
established the same way, by running the same sweep over every winning
`.hkx` file of the installation. The animation and skeleton objects of those
files (`fo4_havok::anim`, [spec/havok-animation.md](../spec/havok-animation.md))
were established the same way, from the container's own tables and counts
over every object, and the skeleton's rotation words were identified by
comparing composed reference poses with the skeleton NIFs stored beside
them. No Havok SDK, header, documentation, converter, decompilation or
third-party reader or schema of these classes or of animation compression
was opened or searched for; layouts recognised from general knowledge were
kept only where a census that could refute them held.

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
reader, editor or renderer of the format was opened for them. The two
optional ground texture rules add no source either: reading a material-only
texture set through its MNAM uses the existing `fo4-material` reader (the
attributed MaterialLib port above) for the diffuse slot only, and the
default texture's candidate comes from the editor IDs, game settings and
`.ini` files of one installation and from offline seam and terrain LOD
comparisons through this repository's readers, all recorded in
[spec/land.md](../spec/land.md); the game executable was not opened.

The viewer's stored texture blocks use the DDS layouts already cited in
[spec/dds.md](../spec/dds.md) and the BC formats of the GPU API in use
(wgpu through Bevy, already dependencies); the GPU decodes them, and no
other BC decoder or texture streaming implementation was consulted.

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

The actor records (`fo4_plugin::actor`), the skinned-shape blocks
(`fo4_nif::skin`) and the rest pose (`fo4_scene::actor::pose`) were
established only by measuring the installed game's winning records and NIF
files ([spec/actor.md](../spec/actor.md), [spec/nif.md](../spec/nif.md),
Skinned shapes): candidate layouts were counted over every record or file
and kept only where they held. No schema, format documentation, editor,
decompilation or third-party reader of these records, of NIF skinning or of
Havok animation was opened or searched for at this step; record and field
names are the stored four-character signatures, and the block and class
names are the strings stored in the files.

Posing an actor by a clip (`fo4_scene::actor::clip`, [spec/actor.md](../spec/actor.md),
Clip pose) adds no format fact beyond [spec/havok-animation.md](../spec/havok-animation.md):
which Havok skeleton belongs to an actor and how a clip's bones replace the
skeleton NIF's nodes are registered pending rules whose candidates come from
the offline comparisons recorded there. No animation runtime, behaviour-graph
reader, SDK or third-party implementation was opened or searched for.

The compiled script reader (`fo4-pex`, [spec/pex.md](../spec/pex.md)) was
established only by measuring every `.pex` entry of the installed game's
archives with the `pex_census` example. The archives hold no script
sources, and none were read. No compiler, decompiler, disassembler, script
virtual machine, format documentation, wiki page or third-party reader of
this format was opened or searched for. The overall layout (the header
words, the string table of u16-prefixed texts, the debug block, the user
flag table, the object with its size word, structs, variables, properties
and states, the function with its parameters, locals and instructions, the
six value kinds, and an opcode table with three counted call opcodes) was
recognised from prior general knowledge before it was measured. It is kept
because the census could have refuted it and did not: all 10,282 entries
are read to the last byte with every object size holding, and each opcode
arity was then tested against twelve alternatives, of which none reads
every file holding the opcode, except one case recorded as open in the
spec. Which slots name functions, properties, struct members and jump
targets was checked by name resolution and target ranges over every
instruction; the candidate meanings of the opcodes, also recognised from
prior knowledge, are registered pending rules, not facts. Field and role
names are labels chosen here; the user flag names are the stored strings.

The VMAD reader (`fo4_plugin::vmad`, [spec/vmad.md](../spec/vmad.md)) was
established the same way, by measuring every VMAD subrecord of the winning
records with the `vmad_census` example and cross-checking its script and
property names and kinds against the `.pex` declarations read above. Its
layout (the version and object-format words, the script list with flags and
properties, the property kinds with the three-word object value, structs and
arrays, and the TERM fragment block) was recognised from prior general
knowledge before it was measured, and is kept because that census could
have refuted it and did not. The fragment blocks of the other record types
are kept as stored bytes. No editor, editor source or definition file,
format documentation, wiki page or third-party reader of this subrecord was
opened or searched for. The `spec/static-records.tsv` rows that mark VMAD as
raw state predate this reader and keep their scope (static frames do not run
scripts).
The reading of a NIF node's nine rotation words by the static renderer
(`fo4_scene::transform::Transform::from_fields`, [spec/nif.md](../spec/nif.md),
Node rotation words) was changed by measurement only: both readings were
composed over every winning NIF and compared with the OBND boxes of the
base records that name the models, which the plugin reader already decodes,
and with the skeleton comparison of the actor step. That a block of nine
words may be stored by rows or by columns was known in general terms and
decided by those counts. No reader, editor, renderer or documentation of
the format beyond the sources already cited in spec/nif.md was opened.

The head parts (`fo4_plugin::actor::Hdpt`, the RACE HEAD sections, and
their drawing under `npc_head_parts_unmorphed_v1`, [spec/actor.md](../spec/actor.md),
Heads) were established the same way as the actor records: subrecord
sizes, values and FormID targets counted over every winning record, and
the head part models fitted against the race skeletons with the existing
skinning code. No schema, editor, face-generation reader or documentation
of these records was opened or searched for.

Following leveled actor lists whose LVLF byte is not zero, and leaving out
the own values of an NPC_ that sets every template bit
([spec/actor.md](../spec/actor.md), Leveled bases and disagreeing
candidates), add no source: they rest on counts over the winning records
and on candidate agreement. General knowledge suggests readings of LVLD,
LVLM and LVLF (a chance, a count, flags) and of the template bits (one
property each); none of them is used: lists whose LVLD or LVLM is not zero
stay refused, LVLF is never interpreted, and no template bit is mapped to
a property.

The script interpreter (`fo4-vm`, [spec/vm.md](../spec/vm.md)) adds no
format fact. Its structure (values, frames, instances with a current
state, dispatch from the instance's script up its parents, properties
backed by variables or functions, arrays and structs by handle) and the
candidate meaning of every opcode were recognised from prior general
knowledge of this kind of script engine; so were the role of the `::State`
identifier, the event function names and their link to game actions
(activation, triggers, loading), the timer natives and their link to the
timer events, the native that returns the player, and the meaning of the
natives that disable and enable a reference. Each enters only as a named
pending rule in [spec/hypotheses.tsv](../spec/hypotheses.tsv), whose text
says which check supports it; the checks (`vm_checks`: declared slot types,
data flow of compiler temporaries under each destination reading, object
guards, loop and array-loop shapes, execution of the functions that reach
no native under the candidate and under exchanged readings) were run over
the stored scripts of one installation and could have refuted them; where
none could, the rule says so. Native functions are not implemented: the
recording host logs every call and supplies one value (the player handle),
and leaves every other result undetermined. Binding a reference's scripts
uses the VMAD reader above; which scripts a placed reference runs, how
property values reach variables and which object identity holds are
pending rules of the same kind. No compiler, decompiler, disassembler,
script virtual machine (including Champollion, Caprica, psc-tools and the
Skymp Papyrus VM), engine reimplementation, script extender or its
library, editor documentation, wiki page, script source or the game
executable was opened or searched for at this step, and none was
reproduced from memory.

The data host and the QUST block (`fo4_vm::data`, `fo4_plugin::vmad`,
[spec/vm.md](../spec/vm.md), Data host and Provenance of this step;
[spec/vmad.md](../spec/vmad.md), QUST block) add no sourced format fact.
The QUST block layout, the meaning of an alias-indexed object value, the
linked-reference subrecord (first guessed as LNKR, refuted by the census,
then XLKR), NAME, FLTV, LNAM, the header flag 0x800, the native meanings
the host answers and the record-type-to-script-type table were recognised
from prior general knowledge and are kept only as far as the censuses
(`vmad_census`, `host_data_census`, `script_run --census`) could have
refuted them and did not; each behaviour is a pending rule in
[spec/hypotheses.tsv](../spec/hypotheses.tsv). No editor, editor source
or definition file, wiki page, script source, decompiler, other
interpreter or the game executable was opened or searched for.
