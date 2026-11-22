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

The quest record reader and the quest state (`fo4_plugin::quest`,
`fo4_vm::quest`, [spec/quest.md](../spec/quest.md),
[spec/vm.md](../spec/vm.md), Quests and Provenance of the quest step) add
no sourced format fact. The QUST subrecord tags' roles, the field layouts,
the meaning of the flag bits the interpreter relies on (DNAM bits 0 and 3,
INDX bit 1), the log-entry position of a VMAD fragment and what setting a
stage, starting and stopping a quest, objectives and forced-reference
aliases do were recognised from prior general knowledge and are kept only
as far as the censuses (`quest_census`, `quest_run`) could have refuted
them and did not; each is a pending rule in
[spec/hypotheses.tsv](../spec/hypotheses.tsv). The native and event names
come from the declarations of the installed scripts. One request for a
Creation Kit wiki page on SetStage was refused by the server (HTTP 403);
nothing was read from it. No editor, editor source or definition file,
script source, decompiler, other interpreter or the game executable was
opened or searched for.

The condition reader and evaluator (`fo4_plugin::condition`,
`fo4_vm::cond`, [spec/conditions.md](../spec/conditions.md),
[spec/vm.md](../spec/vm.md), Conditions and Provenance of the conditions
step) add no sourced format fact. The CTDA field layout, the operator
codes, the flag bits, how a list combines, the meanings of function
indexes 56, 58, 59 and 74 and OnStageSet's order were recognised from
prior general knowledge and are kept only as far as `cond_census` and
`quest_run` could have refuted them and did not; each is a pending rule in
[spec/hypotheses.tsv](../spec/hypotheses.tsv). Function indexes are kept
as bare numbers: no public index-to-name table was read. Two Creation Kit
wiki pages (Fallout 4, `Conditions` and `Condition_Functions`) were
requested on 2026-11-22 and redirected to a maintenance page; nothing was
read from them and no other copy was looked for. No editor, editor
source or definition file, script source, decompiler, other interpreter
or the game executable was opened or searched for.

The second conditions step (`fo4_vm::cond` evaluators of function indexes
72, 84, 248, 550 and 629 and the run-on reading, `fo4_vm::vars`,
`fo4_vm::blockers`, `cond_blockers`; [spec/conditions.md](../spec/conditions.md),
Blockers) adds no sourced format fact and read no public page. The
meanings of those five indexes, of the run-on words 2 and 5, and the
proposed meanings of 1, 14, 71, 359, 426, 555, 560 and 561 (which no code
relies on) were recognised from prior general knowledge and are kept only
as pending rules that `cond_census` and `cond_blockers` could refute; the
parameter types, value ranges and the match of function 550's second
parameter with the scene's action indexes (INAM) are measurements of the
installation. That an auto property's backing variable carries the name
`::Name_var` was read from the installed compiled scripts with `fo4-pex`.
Function indexes stay bare numbers. No editor, editor source or definition
file, script source, decompiler, other interpreter, script extender or the
game executable was opened or searched for.

The story manager reader and the events step (`fo4_plugin::story`,
`fo4_vm::events`, `fo4_vm::story`,
[spec/story-manager.md](../spec/story-manager.md),
[spec/vm.md](../spec/vm.md), Events and the story manager) add no sourced
format fact: the layout of SMEN, SMBN and SMQN comes from measurement
alone. Three public UESP file format pages were requested on 2026-11-22
(`Skyrim_Mod:Mod_File_Format/SMQN`, `Skyrim_Mod:Mod_File_Format`,
`Fallout4Mod:Mod_File_Format`); each answered HTTP 403 and nothing was read
from them. Recognised from prior general knowledge and kept only as far as
`story_census`, `quest_run` and `script_run` could have refuted them: the
roles of PNAM, SNAM, CITC, QNAM and NNAM, that the code `SCPT` is the
script event raised by `Keyword.SendStoryEvent`, how a tree is walked and
what a quest node starts, the meaning of condition function 576's first
parameter (a selector and an event member code) for selector 0, that a
custom event's sender is the object `SendCustomEvent` is called on, the
queued order of custom events, and the order of remote deliveries after
the sender's own scripts. The handler names (`::remote_<type>_<event>`,
`::remote_<script>_<event>`), the stored custom event names and the
native, event and parameter names come from the installed compiled
scripts read with `fo4-pex`. Each behaviour is a pending rule in
[spec/hypotheses.tsv](../spec/hypotheses.tsv). No editor, editor source or
definition file, script source, decompiler, other interpreter, script
extender or the game executable was opened or searched for.

The scene-events step (`fo4_vm::scene` scene clock, `fo4_vm::voice`,
`fo4_plugin::info`, `Store::topic_children`, `fo4_audio::header_seconds`;
[spec/voice.md](../spec/voice.md), [spec/vm.md](../spec/vm.md), Scene
clock) adds no sourced layout. The TRDA size and its response number byte,
the voice file path convention, the speaker's folder, the timer action's
SNAM and the fit of stored data bytes on the seconds the `dpds` table gives
were measured on one installation with `voice_census`; that scene actions
span phases and are waited for, that a timer action stores seconds, that a
dialogue action lasts its line's audio, how lines, responses, shared lines,
voice files and voice types are tied, that `dpds` words count decoded
bytes and the letters of the player dialogue topics were recognised from
prior general knowledge and are used only as pending rules the census and
the sweeps could refute. No page was requested; no editor, editor source
or definition file, script source, decompiler, other interpreter, audio
decoder or tool (including any voice container or lip tool), script
extender or game executable was opened or searched for.

The fragments step (`fo4_plugin::vmad` INFO, PACK, SCEN and PERK blocks,
`fo4_plugin::scen`, `fo4_vm::scene`; [spec/scene-record.md](../spec/scene-record.md),
[spec/vm.md](../spec/vm.md), Scenes and fragments) adds no sourced layout:
two public UESP file format pages were requested on 2026-11-22
(`Skyrim_Mod:Mod_File_Format/VMAD_Field` and
`Fallout4Mod:Mod_File_Format/SCEN`); each answered HTTP 403 and nothing was
read. The block layouts were found by measurement over every block of the
installation; the slot meanings, the scene record's parts and action
kinds, FNAM bit 0x1 of a scene, the scene natives' behaviour, the owning
quest of a dialogue line and the time-scale relation of the two timer
clocks were recognised from prior general knowledge and are used only as
pending rules that `fragment_census`, `story_census` and the sweeps could
refute. No editor, editor source or definition file, script source,
decompiler, other interpreter, script extender or game executable was
opened or searched for.

The save file reader (`fo4-save`, [spec/save.md](../spec/save.md)) was
established only by measuring the save files of one machine with the
`save_census` example and by resolving their form references against the
plugin data that `fo4-plugin` reads. No save editor, save reader or its
source, format write-up, wiki page, decompilation, script extender or game
executable was opened or searched for. Recognised from prior general
knowledge before it was measured, and kept only because the census could
have refuted it and did not: the magic and the header size word; the order
and meaning of the header fields (save number, character name, level,
location, play time, race, sex, experience, file time, screenshot width and
height); the screenshot as four bytes per pixel in RGBA order; the form
version byte and the game version text; the plugin block with a u8-counted
list of names and a u16-counted list of light plugins; the location table
(six offsets in the order form id array, closing table, global tables 1
and 2, change forms, global table 3; four counts; fifteen unused words);
global data entries as type and length words with types numbered from 0,
100 and 1000; the change-form entry (three-byte reference, flag word, type
byte whose top two bits select the width of the two lengths, version byte,
zlib when the second length is not zero); the three-byte reference with an
index into the form id array, a form of the first master or a created form
in its top two bits; the form id array and an array of worldspaces after
global table 3; a table at the end of the file; the numbering of
change-form types against record types; flag bit 1 of a placed reference
as a move whose payload starts with the cell or world, position and
rotation. The byte order of the reference, the base of its index and the
table of change-form types against record types were decided by counts
over alternative readings, not taken from memory. Two recalled details did
not hold or were not used: the count of global table 3 is exact (it was
recalled as possibly one short), and the closing table does not read as a
count followed by entries, so it is kept raw. Every meaning is a pending
rule in [spec/hypotheses.tsv](../spec/hypotheses.tsv); the save files and
anything read from them stay outside the repository.

The sound reader (`fo4-audio`, [spec/audio.md](../spec/audio.md)) was
established only by measuring the sound files of one installation with the
`audio_census` example, reading the archives through `fo4-vfs` and the
records through `fo4-plugin`. No audio decoder or its source (of any
codec), audio library, sound tool, format write-up, wiki page, editor
source or definition file, decompilation or game executable was opened or
searched for, and no decoder was reproduced from memory; no external
dependency was added. Recognised from prior general knowledge before it
was measured, and kept only because the census could have refuted it and
did not: RIFF chunking (the magic, the size word, chunk ids and u32 sizes,
the pad byte after an odd chunk, `LIST` holding a list type and
sub-chunks); the `WAVE` and `XWMA` form types and the chunk ids `fmt `,
`data`, `dpds`, `smpl`, `cue `, `LIST` and `JUNK`; the `fmt ` layout and
the format tag numbers 1 (integer PCM), 2 and 0x11 (ADPCM variants), 3,
0x161, 0x162 and 0xFFFE, of which only 1 and 0x161 occur; integer PCM as
interleaved little-endian two's-complement frames, decided over the
big-endian and offset-binary alternatives by counts, not taken from
memory; `dpds` as one u32 per packet counting decoded bytes and `XWMA`
data as packets of block-align bytes; the voice container as the `FUZE`
magic, a version, a lip length, the lip data and the audio data; the
`smpl` and `cue ` layouts; sound descriptors storing `.wav` paths, with a
leading `data` folder, for files stored under another extension; and the
record signatures and roles of the sound-family records (sound
descriptor, marker, category, output model, music type and track,
acoustic space, region sounds and music). The subrecord offsets and target
types of the links were measured by resolving every four-byte value at
every offset, not recalled; the `WEAP DNAM` offsets were found that way.
The WMA-family payload is not decoded. One dumped file was also opened with
the operating system's own file-information tool to check this project's
output; it was not used as a reference for the format. Every meaning is a
pending rule in [spec/hypotheses.tsv](../spec/hypotheses.tsv); sound files
and anything read from them stay outside the repository.
The coverage census and the decisions taken on its largest reasons
([spec/coverage.md](../spec/coverage.md)) add no format fact beyond the
pinned nifxml copy of spec/nif.md, whose `BSClothExtraData` definition
(a `BSExtraData` holding one byte array, "Fallout 4 Cloth data") is the
only new schema use: the block is now classed with the other physics
payloads as outside the static frame. Everything else rests on counts over
the winning records of one installation (`swap_census`, `subrecord_probe`,
`record_census`, `record_dump`) and on registered pending rules. Recognised
from general knowledge before it was measured: that an MSWP FNAM is an
editor folder label (the census of its 1,901 texts could have refuted it
and did not), and that a PRPS pair is an actor value with a value (its
first words resolve only to AVIF records or to FormIDs below 0x800 with no
record); neither the meaning of those engine-defined actor values nor any
behaviour of a reference group was taken from prior knowledge. No editor,
editor definition file, decompilation or third-party reader of these
records was opened.

Leaving a placed model's root node transform out under
`nif_root_transform_replaced_v1` ([spec/nif.md](../spec/nif.md), Root
node transform) adds no source: the reading that the placement replaces
the root's own transform was recognised from general knowledge and is
used only under hypotheses, supported only by the `root_transform_census`
counts of model boxes against the OBND boxes of one installation (editor
bounds, not the running game). No reader, editor, renderer or
documentation of the format beyond the pinned nifxml copy was opened.

The navigation mesh reader (`fo4-nav`, [spec/navmesh.md](../spec/navmesh.md))
was established only by measuring the `NAVM` and `NAVI` records of one
installation with the `nav_census` example, reading the plugins through
`fo4-plugin` and checking the result against the cells, placed doors,
terrain and collision the project already reads. No editor, editor source
or definition file, navmesh tool, format write-up, wiki page, other
implementation, decompilation or game executable was opened or searched
for, and no external dependency was added. Recognised from prior general
knowledge before it was measured, and kept only because the census could
have refuted it and did not: the record signatures `NAVM` and `NAVI` and
the subrecord names `NVNM`, `ONAM`, `NNAM`, `MNAM`, `NVER`, `NVMI`,
`NVPP`; `NVNM` opening with a version and a constant check word, then a
world word that is zero inside and the parent cell FormID or the grid as
two `i16`; the section order vertices, triangles, edge links, door
triangles, cover data, cover-to-triangle mappings, waypoints, grid, with
vertices as three `f32`; a triangle as three `u16` vertices and three
`i16` edge words with -1 for none followed by flag and cover fields (from
an older, shorter layout; the 21-byte size was measured); an edge link as a
type word, a navmesh FormID and a triangle; a door link as a triangle, a
word and a door reference; waypoints as a position, a triangle and flags;
cover entries as pairs of vertex indices with a data word; the grid as a
division count, two square sizes, bounds and one triangle list per square;
`NVMI` as navmesh, flags, an approximate position, a float, lists of
connected navmeshes and doors, an island flag with island data and the
parent world and cell; `NVPP` as preferred-pathing lists. Found by
measurement and not recalled: the triangle bits that mark edge links and
cover, the cover mapping packing, the edge byte of an edge link, which half
of the grid word is x, the grid's square numbering and the bracket its
lists satisfy, the island byte matching an `NVMI` flag, and the `ONAM`,
`NNAM` and `MNAM` framings. Two recalled details did not hold: cover
entries are not plain vertex pairs, and the grid does not list exactly the
triangles overlapping each square. The path search (`fo4_nav::path`: A*
over triangle centroids and a funnel) is this project's own, written from
general graph-search and geometry knowledge; it claims nothing about the
game's planner. Every meaning is a pending rule in
[spec/hypotheses.tsv](../spec/hypotheses.tsv); records and anything read
from them stay outside the repository.
Enable parents ([spec/enable-parent.md](../spec/enable-parent.md)) use the
XESP layout of the published record definitions already cited in
spec/plugin.md. Recognised from prior knowledge and used only as pending
rules that the `enable_parent_census` counts could have refuted: that a
record with an XESP is present exactly when its parent is enabled, that
flag bit 0 inverts the link ("opposite of parent") and bit 1 is a fade
choice, and that a script's Enable or Disable of a parent reaches its
children. No editor, editor source, script source or game executable was
opened.

Animation controllers at rest ([spec/controller-rest.md](../spec/controller-rest.md))
use the pinned nifxml copy of spec/nif.md for `TimeControllerFlags`,
`CycleType`, `NiQuatTransform` (quaternion stored w, x, y, z), the four
shader controlled-variable enums and the `NiControllerManager` description.
Recognised from prior knowledge and used only as pending rules that the
`controller_census` counts could refute: that a controller manager's
sequences play when started by name and are otherwise idle, that a keyframe
interpolator gives only its keys' values, and the standard rotation matrix
of a unit quaternion (its row reading was checked against the stored node
words of the installation, never the transpose alone). No other NIF reader,
renderer, editor or game executable was opened.

Effect surfaces ([spec/effect-surfaces.md](../spec/effect-surfaces.md)) use
the pinned nifxml copy of spec/nif.md for the `BSEffectShaderProperty`
fields of `bs_version` 130 and their descriptions (Base Color, Base Color
Scale "Multiplier for Base Color (RGB part)", the four falloff fields "At
this cosine of angle falloff will be equal to ..."), `AlphaFlags`,
`AlphaFunction`, `Fallout4ShaderPropertyFlags1` and `2`, and
`EffectShaderControlledVariable` and `EffectShaderControlledColor` (the
pairing of a controlled variable with a field is by name only). Recognised
from prior knowledge and used only as pending rules or as named refusals
that the `effect_census` counts could refute: that a sequence's clock starts
at its first key when a cell loads, that soft effects fade near geometry,
and that falloff depends on the angle between the view direction and the
surface normal. No NIF reader, renderer, editor, MIT reference or game
executable was opened.

The environment map, driven material field and blended opacity step
([spec/effect-surfaces.md](../spec/effect-surfaces.md), Environment map
and driven material fields; [spec/render.md](../spec/render.md), Opacity
above 1 with blending) adds no format fact: it uses the nifxml names of
the environment slots and scale already in spec/nif.md, the material
reader's fields, the DDS cube layout of spec/dds.md and, for the viewer,
that an unsigned normalised colour target clamps a fragment's output
before blending (the graphics API's documented behaviour, used only to
say the viewer's explicit clamp changes nothing). Recognised from prior
knowledge and kept as pending rules: that an environment map adds a
reflection to the colour, and that an opacity above 1 acts on a blend
through a clamped alpha. No NIF reader, renderer, editor, MIT reference
or game executable was opened.
The player position model (`fo4_vm::track`, [spec/vm.md](../spec/vm.md),
Player position) reads the `XPRM` subrecord of placed references as three
bound words, three colour words, one float and a kind word. That layout was
established only by measuring every placed reference of the installation
with `world_census` (32 bytes on all 30,028, colour and float words in 0 to
1, kind words 1 to 5; [spec/world-census.txt](../spec/world-census.txt)).
The plugin layout table, which then still listed `REFR.XPRM` as not
established, was left unchanged so that the coverage census did not move;
the wildcard swap step below establishes it there from the same
measurement. What the
words mean for an event (half extents, a box, a sphere) was recognised from
prior knowledge and is kept only as pending rules, checked against the
navigation meshes as far as the data allows. No editor, physics engine,
reader of another project or game executable was opened.

The step on differing swap scopes, swaps over shapes without a material,
empty swap paths, missing textures and the vertex alpha of blended
lighting surfaces ([spec/materials.md](../spec/materials.md), second step;
[spec/render.md](../spec/render.md), Vertex alpha of blended lighting
surfaces) adds no format fact. Texture slot names are those of this
project's material reader; texture set slot positions and the Shader Flags
bits (Vertex_Colors, Vertex_Alpha, the greyscale palette flags) come from
the pinned nifxml copy already cited. Everything else rests on counts over
the winning records and the meshes of one installation, taken with local
probes whose outputs stay outside the repository, and on registered pending
rules. Recognised from prior general knowledge and kept only as pending
rules: that a lighting shader reads vertex alpha under Vertex_Colors, and,
weakly, that a reference's material swap overrides its base's (the data
was weighed on both sides of it, spec/materials.md). No editor, editor
definition file, MIT reference, third-party reader of these formats or game
executable was opened.
The talk step (`fo4_plugin::dial`, `fo4_vm::talk`, [spec/vm.md](../spec/vm.md),
Dialogue outside scenes; [spec/talk-census.txt](../spec/talk-census.txt))
reads the dialogue topic record (DIAL) as EDID, FULL, PNAM (an f32),
BNAM, QNAM, KNAM, DATA, SNAM (a four-letter code) and TIFC. The names and
roles were recognised from prior knowledge and kept only where
`talk_census` holds them on every winning topic (every field but EDID four
bytes, QNAM a QUST in all, SNAM printable letters); the last two bytes of
DATA vary within one SNAM code and are kept as stored. The plugin layout
table is unchanged. What a subtype does in the game (a greeting when the
player talks, a hello when the player comes near), how a line is chosen,
how unique actor aliases fill and where actors stand are pending rules.
No editor, editor source or definition file, reader of another project,
script source or game executable was opened.
The animation step (`fo4_havok::Machine`, `fo4_havok::annotation_tracks`,
`fo4_scene::anim_graph`, `fo4_plugin::pack`, `fo4_vm::anim`;
[spec/havok-behavior.md](../spec/havok-behavior.md),
[spec/havok-animation.md](../spec/havok-animation.md), Annotation tracks,
[spec/vm.md](../spec/vm.md), Animation events and Package destinations)
reads the behaviour project, character and graph files, the annotation
tracks of the clips and the package record's data inputs. Every offset was
proposed from local probes of a few files and then counted over every file
or record of one installation (`anim_event_census`, `package_census`); the
class names are the stored ones and the NIF field names those of the
pinned nifxml copy already cited. The roles of the behaviour classes and
of the PACK subrecords, and every behaviour the run relies on (deliveries,
case, transitions, text keys as events, package destinations), were
recognised from prior general knowledge and are pending rules. No Havok
SDK, converter, behaviour editor or format project, no editor, editor
source or definition file, no other interpreter, script source or game
executable was opened.

The step on wildcard swap entries, the reference primitive, chained swap
scopes and non-finite texture coordinates
([spec/materials.md](../spec/materials.md), third step;
[spec/render.md](../spec/render.md), Texture coordinates that are not
finite; [spec/record-layouts.md](../spec/record-layouts.md), refr-xprm)
adds one layout to the plugin layout table: `REFR.XPRM` as 32 bytes, seven
floats and a kind word, established by the project's own measurement of
every placed reference with `world_census`
([spec/world-census.txt](../spec/world-census.txt), section 1), not by an
outside source; its labels (bound, colour, kind) are descriptive names
taken over from the player position model and claim no behaviour. Every
other statement rests on counts over the winning records and meshes of
one installation, taken with local probes whose outputs stay outside the
repository, and on registered pending rules. Recognised from prior general
knowledge and kept only as pending rules: that `*` in a path is a pattern
standing for any run of characters (and how a target's `*` would be
filled), and that a graphics processor gives no defined texel for a
texture coordinate that is not a number. That one interior cell whose
editor ID names navigation mesh generation holds 633 of the 637 placed
statics with a model and a primitive is read from the records, not from
any account of what that cell is for. No editor, editor definition file, MIT reference, third-party
reader of these formats or game executable was opened.
The load step (`fo4_vm::load`, [spec/vm.md](../spec/vm.md), Load events
and Scene action progress; [spec/load-event-census.txt](../spec/load-event-census.txt),
[spec/action-percent-census.txt](../spec/action-percent-census.txt)) reads
no new format: it counts the compiled scripts' load handlers and the VMAD
attachments, and the conditions of function 131 with the scene records
they name, with the repository's readers. When a reference loads, where a
`MoveTo` puts it, what function 131 means and when a phase's conditions
end it were recognised from prior general knowledge and are pending
rules. No editor, editor source or definition file, reader of another
project, Havok project, script source or game executable was opened.
