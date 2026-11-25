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

The game clock of controllers ([spec/controller-clock.md](../spec/controller-clock.md);
[spec/render.md](../spec/render.md), The game clock) uses the pinned
nifxml copy of spec/nif.md for `TimeControllerFlags` (Anim Type, Cycle
Type, Active, Play Backwards, Manager Controlled), the `NiTimeController`
fields and their descriptions (Frequency "usually 1.0", Phase "usually
0.0", Start Time, Stop Time), `CycleType` (LOOP, REVERSE, CLAMP) and
`KeyType` (linear; quadratic with forward and back tangents; TBC; XYZ;
constant, a "step function"). Nifxml names these without formulas.
Recognised from prior knowledge of Gamebryo controllers and used only as
pending rules that the `controller_census` counts could have refuted: the
controller time Start Time + Phase + Frequency x seconds from the model's
load, the three cycle foldings, the cubic Hermite reading of quadratic
keys with tangents taken per interval, and that a manager's sequences
play only when something starts them. No NIF reader, renderer, editor, MIT
reference or game executable was opened.
The sit step (`fo4_vm::sit`, `fo4_vm::menu`, [spec/vm.md](../spec/vm.md),
Alias fills from repeated and placed rows, Distance conditions, Activation
of actors, Furniture use, Menus, The player's base object;
[spec/alias-fill-census.txt](../spec/alias-fill-census.txt),
[spec/sit-menu-census.txt](../spec/sit-menu-census.txt)) reads no new
format: it counts quest aliases against the special reference lists and
the `XLRT` and `XLCN` words of placed references and cells (read as
FormIDs as the earlier steps read them), and the compiled scripts' furniture
and menu handlers, natives and registrations, with the repository's
readers. What function 1 measures, which id the player's reference has,
what activating an actor or a furniture raises, what the furniture and
menu events are, and what the player's base is were recognised from prior
general knowledge and are pending rules. No editor, editor source or
definition file, reader of another project, script source or game
executable was opened.

The keyword step (`fo4_vm::keyword`, [spec/vm.md](../spec/vm.md), Keywords,
Collection aliases from a location's rows, Scene actions whose start phase
was skipped, Caller actions held until a stage;
[spec/keyword-census.txt](../spec/keyword-census.txt)) reads no new format:
it reads `KWDA`, `KSIZ`, `RNAM` and `TPLT` of records as FormID words and
their presence (as earlier steps read FormID subrecords), the special
reference lists as the earlier location fills read them, and the compiled
scripts' keyword natives and their call sites, with the repository's
readers. That a reference carries its base's keywords and an actor its
race's, what the keyword natives and the collection natives do, and that
an animation event registration answers whether it was made were
recognised from prior general knowledge and are pending rules or caller
assumptions. No editor, editor source or definition file, reader of
another project, script source or game executable was opened.

The graph step (`fo4_vm::scene::recheck_variables`, `fo4_vm::setters::table_with`,
[spec/vm.md](../spec/vm.md), Scenes offered again when a script variable is
written, Character behaviour graphs, Calls from other scripts in the
stage-setter table; [spec/scene-variable-census.txt](../spec/scene-variable-census.txt),
[spec/character-graph-census.txt](../spec/character-graph-census.txt)) reads
no new format: scene conditions as the conditions step reads them, the
RACE MODL texts after the NAM3 marker as texts, the behaviour project,
character and graph files and the clips' annotation tracks with
`fo4_havok`'s existing readers, and a byte search of stored bytes for a
clip's path (no field interpreted). That a playing scene keeps testing its
phase conditions, that a race names its characters' behaviour project and
that a call on a fragment's owning quest runs that quest's script were
recognised from prior general knowledge and are pending rules or used only
to search the data. No behaviour or animation tool or library of any
project, Havok SDK, editor, editor source or definition file, script
source or game executable was opened or searched for.

The exit step (`fo4_vm::sit`, `fo4_vm::anim::furniture_event`,
`fo4_vm::menu::menu_mode`, [spec/vm.md](../spec/vm.md), Furniture
activation and the furniture's graph, The forced opening to the end of the
prewar quest; [spec/quest-run-opening-unset.txt](../spec/quest-run-opening-unset.txt))
reads no new format: furniture models' behaviour graphs through the chain
`fo4_scene::anim_graph` already reads, the scene and quest records and the
compiled scripts as earlier steps read them. That activating a furniture
makes an actor use it with or without the activation's events, that
activating the furniture one sits in stands one up and that a furniture's
animation follows its user's were recognised from prior general knowledge
and are pending rules; the use events' names are the caller's. No
behaviour or animation tool or library of any project, Havok SDK, editor,
editor source or definition file, script source or game executable was
opened or searched for.

The vault step (`fo4_vm::sit` `GetSitState`, `fo4_vm::menu::menu_mode`,
`fo4_vm::anim::settle_start`, the text equality of `fo4_vm`'s interpreter;
[spec/vm.md](../spec/vm.md), The forced run through the vault;
[spec/quest-run-vault-stages.txt](../spec/quest-run-vault-stages.txt),
[spec/quest-run-vault.txt](../spec/quest-run-vault.txt)) reads no new
format: the quest records, the compiled scripts and the behaviour graphs of
the door console, the vault door and the wrist computer's furniture as
earlier steps read them. The documented values of the sit state native,
that a NIF sequence of cycle type 2 plays once, that the engine's names and
the scripting language's text comparisons ignore case, and that the
player's elevator ride from the vault to the surface happens between the
exit trigger and the exterior trigger were recognised from prior general
knowledge and are pending rules or caller assumptions. No behaviour or
animation tool or library of any project, Havok SDK, editor, editor source
or definition file, script source or game executable was opened or
searched for.

The light step (`fo4_plugin::weather`, `fo4_scene::light`,
`fo4_render::render_lit`; [spec/light.md](../spec/light.md),
[spec/light-census.txt](../spec/light-census.txt)) reads CELL.XCLL, LGTM
DATA and DALC, LIGH DATA and FNAM, and REFR XRDS through the layouts
already sourced in [spec/record-layouts.tsv](../spec/record-layouts.tsv),
and the CLMT and WTHR subrecords the census measures on every winning
record of the load order: CLMT WLST as 12-byte entries whose first word
resolves to a WTHR, TNAM as 6 bytes; WTHR DALC as the LGTM.DALC layout
(same tag and size), NAM0 as four-byte colours in rows of one per DALC,
FNAM as f32 values, IMSP as FormIDs. No public source was consulted for
these two records (no network in this step); their shapes are
measurements of the data and the roles the light rules give their rows,
slots and values (sky, fog, time of day, the unit of the climate's times)
were recognised from prior general knowledge and are pending rules with
the data hints their rows record. No editor, editor source or definition
file, reader of another project, shader of another project or game
executable was opened.
The home step (`fo4_vm::track::teleport_of` and the load-door passage,
`fo4_vm::line`, condition function 70 on the player in `fo4_vm::cond`, info
groups in `fo4_vm::talk`; [spec/vm.md](../spec/vm.md), The walk home;
[spec/quest-run-home.txt](../spec/quest-run-home.txt),
[spec/quest-run-vault-stages.txt](../spec/quest-run-vault-stages.txt)) reads
the `XTEL` of placed doors past its established first word (three position
and three rotation words, a reading measured over the 1,204 stored values
and kept as a pending rule, not a layout fact: the plugin layout table is
unchanged), and the `TSCE`, `INTV` and `GNAM` words of dialogue lines as
FormIDs and a phase index under pending rules. That a load door moves the
player to its teleport marker, that a line's fragments run as it begins and
ends and its start-scene field starts the scene after it, what condition
function 70 compares, and that an info group's conditions apply to its lines
were recognised from prior general knowledge and are pending rules. No
behaviour or animation tool or library of any project, editor, editor source
or definition file, script source or game executable was opened or searched
for.

The character graph step (`fo4_havok` behaviour nodes, `fo4_scene::char_graph`,
`fo4_scene::timeline::cast::start_poses`; [spec/character-graph.md](../spec/character-graph.md),
[spec/character-start-clip-census.txt](../spec/character-start-clip-census.txt))
reads more of the behaviour graph `.hkx` files: the variable binding sets,
the graph data's variable types, initial words and names, and the modifier,
tagging, offset, blender (and blender child), manual selector, behaviour
reference, swap and clip generator classes, with offsets measured on the
installation's 450 behaviour graph files by printing objects field by field
(bytes, first-table pointers, second-table links) and checked over all of
them by `char_start_clip_census`. Class names are the stored ones; the field
labels are this project's. From RACE it reads the MODL after NAM3 by section
marker, SRAC, and the runs of SAKD, STKD, SGNM, SAPT closed by SRAF, and
the KWDA of NPC_ and RACE, measured on the winning records. No public
source was consulted (no network in this step). Recognised from prior
general knowledge, and kept as pending rules or named readings: that the
objects are state machines, blenders, selectors and clip generators as
their class names say; that a graph sits in its start states until an
event; that a variable keeps its stored value until set; the binding
element's field order (member path, variable index, bit, kind); the clip
generator's field order after its trigger link (crop, start time, speed,
duration, fraction, binding index, mode, flags), checked against value
ranges; that clip mode 1 loops and 0 plays once; that the blender flag bit
0x10 marks weights read as positions (checked: every blender with a weight
outside 0 to 1 has it); that a subgraph entry's SAPT texts are folders
searched for a clip and its SAKD and STKD actor and target keywords. No
Havok SDK or header, behaviour editor, converter, editor, editor source or
definition file, reader of another project, animation tool or library, or
game executable was opened or searched for.
The fight step (`fo4_vm::fight`, the shared location rows in
`fo4_vm::quest::Quests::fill_location`, the inherited auto state in
`Vm::instantiate`, the `fight_census` example; [spec/vm.md](../spec/vm.md),
The first fight; [spec/fight-census.txt](../spec/fight-census.txt),
[spec/quest-run-fight.txt](../spec/quest-run-fight.txt),
[spec/quest-run-vault-stages.txt](../spec/quest-run-vault-stages.txt)) reads
no new record layout: it reads the compiled scripts' event handlers,
registrations and auto states with the repository's own readers, and the
quest record's alias fills and location rows as the earlier steps do. That
hit events reach only registered scripts, once per registration, that an
alias's init event registers it when its quest fills it, which combat state
values mean what, that dying precedes death, that a killed actor answers
dead, that an actor fighting the player puts the player in combat, and that
a script without an auto state of its own starts in its parent's were
recognised from prior general knowledge and are pending rules. No combat,
behaviour or animation tool or library of any project, editor, editor source
or definition file, script source or game executable was opened or searched
for.

The play mode (`fo4-play`, [spec/play.md](../spec/play.md)) is this
project's own code over its own crates: the stepped clock loop is
`quest_run`'s loop with a horizon (first compiled from the example's source
through three patch points, since the run library step `fo4_vm::run`'s
`NewGame`); the crossing folds repeat `fo4_vm::track::intervals` move by
move; the scripted input format, the walker's slices, the eye ray against the
walking world's primitives (Möller and Trumbore's ray and triangle test, a
textbook method recalled from general knowledge; a ternary and bisection
search for widened convex bodies) and the object bounds box (OBND as
`spec/plugin.md` reads it) are local work. Recognised from prior general
knowledge and kept as pending rules: that a crosshair is a ray from the eye,
that activation picks the first object it meets within a reach, that script
time and frame time run together. No source was consulted (no network in
this step); no game executable, editor, other implementation or game run was
opened.
The museum step (`fo4_vm::walk_order`, `fo4_vm::runstate`, the response
actions in `fo4_vm::scene`, the timeline's deaths and sits, the
`walk_order_census` example; [spec/vm.md](../spec/vm.md), The museum's
order; [spec/walk-order-census.txt](../spec/walk-order-census.txt),
[spec/quest-run-museum.txt](../spec/quest-run-museum.txt),
[spec/timeline.md](../spec/timeline.md), Deaths and furniture) reads no new
record layout: the navigation mesh, the placed triggers' primitives and the
doors' object bounds are read as the earlier steps read them; of a door's
lock record (`XLOC`, 16 bytes in every placement) only its presence and its
first byte, printed as stored, are used. That a locked door refuses the
player until a script unlocks it, that a stored lock record means locked,
that a global's value changes at run time, that unlocking raises the lock
state change event, that a scene's response action answers the player's
last choice, that a form's engine script runs its property functions
without an attached instance and that the player walks on the navigation
meshes were recognised from prior general knowledge and are pending rules.
No navigation, combat or dialogue tool or library of any project, editor,
editor source or definition file, script source or game executable was
opened or searched for.

The live world step (`fo4_play::live`, `fo4_play::world`, the `door_census`
example, `WorldSet::set_hidden`, `World::copy_sources`; [spec/play.md](../spec/play.md),
[spec/door-census.txt](../spec/door-census.txt)) reads no new record layout:
the run's visibility changes and moves are read as the timeline's writer reads
them, decided as `fo4_scene::timeline::apply` decides a frame, and `XTEL` as
`fo4_vm::track::teleport_of` reads it; the door census counts subrecords, flag
bits and sizes with the repository's own readers. Recognised from prior general
knowledge and kept as pending rules: that collision goes with what is drawn,
that activating a load door moves the player at once, that statics are not
activated. That an empty `ONAM` on a placed door may mean "open by default"
was recalled from a field's published name and is not used. No editor, editor
source or definition file, other implementation, game executable or game run
was opened or searched for.

The run library step (`fo4_vm::run`, the `quest_run` example as a thin
caller, `fo4-play`'s `Stepper`; [spec/play.md](../spec/play.md),
[spec/vm.md](../spec/vm.md), The quest run as a library) moves the
repository's own code and adds caches of what the records store; it reads no
new record layout. The one new pending rule
(`vm_global_runtime_value_undetermined_v1`) is the conservative reading of a
global set to a value the run does not know, from no data and no source. The
hot spots were found with the operating system's sampling profiler on the
repository's own runs. No public page was requested; no editor, editor
source or definition file, other implementation, game executable or game run
was opened or searched for.
The seen step (`fo4_scene::furniture`, `fo4_scene::sequence`, the timeline's
death and sit readings, `collision::instance_as_stored`, the exposure and
vertex normals of `fo4_scene::light` and the renderers, `fo4_walk::platform`,
the `seen_census` and `ride_check` examples; [spec/seen-census.txt](../spec/seen-census.txt),
[spec/timeline.md](../spec/timeline.md), Deaths and furniture and The elevator
second, [spec/walk.md](../spec/walk.md), Platforms,
[spec/light.md](../spec/light.md), Exposure) measured two record shapes with
the repository's own readers: FURN `SNAM` (entries of 20 or 24 bytes, every
one of the 782 records read; the count against `MNAM`'s bits) and IMGS
`HNAM`, `CNAM`, `TNAM` (nine, three and four f32, ranges over the 334
records). The NIF blocks (controller sequences, transform data, furniture
marker blocks, collision objects) and the Havok objects (clip generators,
node names, event names, ragdoll classes counted by their stored class names)
are read by the existing readers. Recognised from prior general knowledge and
kept as pending rules: that the SNAM floats are a marker's offset and heading
and word 2 subgraph entries are furniture; that the image space's HNAM word 5
is a white point, CNAM saturation, brightness and contrast and TNAM a tint's
amount and colour, and the formulas applied with them (an extended
photographic tone curve, a grey-preserving saturation); that the eye adapts
to the light of what it sees; that a dead body lies on the ground and that
death clips and ragdolls are chosen by the engine on events; that a collision
marker's box is solid; that a platform carries what stands on it; that the
game lights surfaces with their vertex normals. No source was consulted (no
network), and no editor, editor source or definition file, other
implementation, game executable or game run was opened or searched for.

The step past the museum (`fo4_vm::terminal`, `fo4_vm::inventory`, additions
to `fo4_vm::runstate`, the `stops_census` example; [spec/vm.md](../spec/vm.md),
Past the museum, [spec/quest-run-onward-census.txt](../spec/quest-run-onward-census.txt))
reads the terminal record's menu items as stored (the subrecords from one
`ITXT` to the next) and its VMAD fragment block as `fo4-plugin` already read it,
the `PRKR` and `SNAM` lists of actor bases as 5-byte entries (a FormID and a
rank byte), and the alias and line fields `fo4-plugin` already reads; every
size was counted with the repository's own readers before it was used. The
census command line: `stops_census MANIFEST OUT NAMES --player-base <the
player's base> --quest <the next main quest>` (editor ids in the step's notes).
The museum step's walk order census was regenerated byte for byte with
`walk_order_census MANIFEST <the museum's cell> <the next main quest>,<the
museum's quest>,<the town's dialogue quest> OUT NAMES --stage-property
<the trigger scripts' stage property> --prereq-property <their prerequisite
property> --off-property <their turn-off property> --door-margin 16` (keys and
property names in the step's notes). Recognised from prior general knowledge
and kept as pending rules or caller assumptions: what a terminal fragment and
an inventory event filter are for, that a script's lock change raises the
lock event, that a negative faction rank is no membership, that the player's
strength is chosen at the start. No public page was requested; no editor,
editor source or definition file, other implementation, game executable or
game run was opened or searched for.
The faces step (`fo4_scene::actor::Readings` and `resolve_reference_with`,
`fo4_scene::timeline::ride`, `actor::pose::node_worlds_with`, the box presence
and terrain pass of `fo4_walk::platform`, the `faces_census` example and
`ride_check --trace`; [spec/faces-census.txt](../spec/faces-census.txt),
[spec/actor.md](../spec/actor.md), Picked looks,
[spec/timeline.md](../spec/timeline.md), Drawing the shaft and the ride,
[spec/walk.md](../spec/walk.md), The ride's loose ends) measured with the
repository's own readers only: the template slots and bits of NPC_ (which own
fields are present by bit, over the 3,279 records with TPTA), the bytes,
entries and levels of the LVLN and LVLI an opening's actors reach, the block
types of the 27,193 models of the placed kinds, the draw blockers of the
elevator's shaft model, and the walker's contacts during the ride. Recognised
from prior general knowledge and kept as pending rules: that template bit 0
carries an actor's traits, bit 8 its inventory and bit 12 its keywords; that
the game picks one entry of a leveled list (here the caller's seed stands for
its choice); that LVLD is a percent chance of nothing and LVLF bit 2 makes a
list give every entry; that a model's skinned shapes are bound to its own
nodes; that a platform's rider passes the terrain below a shaft. The pick's
hash is FNV-1a, a public-domain hash chosen for being deterministic and
simple, not taken from any implementation of the game. No source was
consulted (no network), and no editor, editor source or definition file,
other implementation, game executable or game run was opened or searched for.

The third fight's step (`fo4_vm::quest::held_more` and `alias_value`,
`fo4_vm::player::holders_of`, `fo4_vm::line`'s own-script events,
`fo4_vm::sit::end_by_track`, `fo4_vm::anim::Anims::started` and the timeline's
`sequence` lines, `fo4_play::elevator`, the `third_census` example;
[spec/vm.md](../spec/vm.md), The third fight, [spec/play.md](../spec/play.md),
The elevator ride, [spec/quest-run-third-census.txt](../spec/quest-run-third-census.txt))
reads the alias values `ALCO`, `ALCA`, `ALCL`, `ALEQ` and `ALEA` as
`fo4-plugin` already read them, a line record's VMAD script list as
`fo4-plugin` already read it, every winning record's subrecords as stored
(for words naming a record) and the CTDA fields as `spec/conditions.md`
reads them; the moving platforms use `fo4-scene`'s sequence and collision
readers and `fo4-walk`'s ride as the seen step left them. The census command
line: `third_census MANIFEST OUT NAMES --quest <the next main quest> --scene
<the lobby scene> --scene <its scene collection> --function 566` (keys in the
step's notes). Recognised from prior general knowledge and kept as pending
rules or caller assumptions: that a created alias holds a new reference of
its base, that a borrowed alias holds the other quest's alias's reference
(taken as a live link), that a line's own script receives the line's begin
and end with the speaker and whether it was said, that walking away ends a
sit, that a platform moves with the sequence its graph plays and carries its
rider, and that an elevator's end takes the player through its door. No
public page was requested; no editor, editor source or definition file,
other implementation, game executable or game run was opened or searched
for.
The poses step's facts were measured with `poses_census` on the installed game's data and the earlier steps'
frame overlays: the races' subgraph entries by flag word (SRAF as stored), the behaviour graph extra data of the
actors' part models with the start states of the graphs they name (read as the earlier steps read model graphs),
what the skinned models of placed records hold (skin bones, sequences naming them, behaviour graph and physics
blocks) and the shaft model's blocks; the viewer's texture and mesh bytes with `fo4-view --memory-report`.
Recognised from prior general knowledge and kept as pending rules: that a creature race's flag word 1 entries are
its movement graph; that a part model's own graph whose start state plays no sequence leaves the model as stored;
that a skinned placed model's bones rest at their stored nodes. The memory hash is FNV-1a, chosen for being simple.
No source was consulted (no network), and no editor, editor source or definition file, other implementation,
game executable or game run was opened or searched for.

The town chain's step (`fo4_vm::quest::QuestInfo::load_with`, `fo4_vm::chain`
with its container and location changes and `Location.IsChild`, the stepped
run's per-timer budget, `fo4_play::elevator::still_owner`, the
`chain_census` example; [spec/vm.md](../spec/vm.md), The town chain,
[spec/play.md](../spec/play.md), The elevator ride,
[spec/quest-run-chain-census.txt](../spec/quest-run-chain-census.txt))
reads the record header's flag word, the load order's occurrences of a
record (`fo4-plugin`'s override history) and a quest record's subrecords as
`fo4-plugin` already read them, a location record's `PNAM` as a FormID
(the field name is xEdit's published one), stage `INDX` flags as stored, and
the compiled scripts as `fo4-pex` reads them. The census command line:
`chain_census MANIFEST OUT NAMES --quest <the town chain's first quest>
--quest <its second> --quest <the main quest after the successor> --chain
<the next main quest>:140 --chain <the next main quest>:200 --stage <the
next main quest>:75 --stage <the next main quest>:1000 --line <the robot's
275 line>` (keys in the step's notes). Recognised from prior general
knowledge and kept as pending rules or caller assumptions: that record flag
bit 0x4000 marks an override carrying only its changed subrecords, that a
container change event carries the new and the old container, that a
location change event comes as an actor crosses into another location, the
direction `IsChild` asks, that timers run until stopped, and that stage flag
0x04 marks a stage set at shutdown (not applied). No public page was
requested; no editor, editor source or definition file, other
implementation, game executable or game run was opened or searched for.

The layers step's collision readiness switches and held poses
([spec/collision-placement.md](../spec/collision-placement.md), Blockers off
the collision chain, [spec/collision-ready-census.txt](../spec/collision-ready-census.txt))
read the NIF blocks, links and field names as `fo4-nif` already reads them
(nifxml for `Target`, `Extra Target`, the controlled blocks' `Node Name`
and the transform interpolator's `Translation`, `Quaternion` and `Data`),
the collision chains as `fo4_scene::collision::model` already builds them,
the behaviour graph as `fo4-havok` reads it, and the reference's VMAD and
the compiled script as `fo4-plugin` and `fo4-pex` read them. `XALP` is
named after xEdit's published field name (an alpha); its bytes stay
unread. The census command line: `collision_ready_census MANIFEST OUT
--around 1 --cell <the house's exterior cell> --cell <five cells the walk
crosses outside> --cell <the vault> --probe <the vault> X,Y,Z (thirteen
points on the recorded walk) --door <the entrance's exterior shell> --door
<the entrance room> --door <the gear door>` (keys in the step's notes).
Recognised from prior general knowledge and kept as pending rules: that a
reference's alpha is a fade of its drawing only, and that a transform
interpolator without keys holds its node at its own transform. No public
page was requested; no editor, editor source or definition file, other
implementation, game executable or game run was opened or searched for.

The menus step's interface reader (`fo4-ui`, [spec/ui.md](../spec/ui.md))
takes its format facts from Adobe's public SWF File Format Specification
(version 19: the file header, the tag header, `DefineSprite`, shape records
and the two fill styles of an edge, placement matrices and colour
transforms) and from Adobe's public AVM2 overview (multiname kinds and the
stack effect of the property, call and push instructions), cited with their
scope in the spec; neither was fetched during the step (no network), both
are cited from the published documents. Parsing is done by the `swf` crate
0.4.0 of the Ruffle project (MIT OR Apache-2.0, recorded in
[THIRD_PARTY.md](../THIRD_PARTY.md)) used as a dependency; the crate's
public types and its documentation were read to call it, and no Ruffle code
was copied. The tag walk, the bounds, the static reading of the host
interface (the host-filled member rule, the look-back), the census and the
scanline fill are this project's own. The census and host reading command
lines: `ui_census MANIFEST OUT_DIR` and `ui_host MANIFEST OUT_DIR 'dialogue
screen=<the dialogue movie>' 'heads-up screen (subtitles)=<the heads-up
movie>@subtitle'` (paths in the step's notes). Recognised from prior
general knowledge and kept as pending rules or stated as such: that the
game's interface player lets the host call a movie's methods and set its
members by name, that the subtitles are shown by the heads-up screen, and
that a dialogue line stores a short prompt shown on a choice. No interface
source repository, decompiled interface, other player or engine, game
executable or game run was opened or searched for.

The gate-frame step's layer record reader (`fo4_plugin::coll`,
[spec/record-layouts.md](../spec/record-layouts.md), section `coll`) takes
the names and roles of the collision layer record's fields (an index, a
debug colour, a flag word, a name, a count and the list of layer records a
layer collides with) from UESP's public file format page for the record
type (`https://en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format/COLL`), written
from prior knowledge during the step (no network) and checked against the
page, opened in a browser, at review on 25 November 2026; every
size, the count's relation to the list and the list's targets were checked
on the 57 records of the installation, and the relation the lists give was
found symmetric. That index 30 is the layer of a walking character's body,
and what a listed pair does to a box and a character, are prior general
knowledge, kept as pending rules (`play_collision_marker_box_layer_records_v1`).
The static reference fields reading (`refr_static_fields_collision_unchanged_v1`)
rests on the field names already cited in the layout table and on a
load-order census made with a temporary probe kept outside the repository.
No editor, editor source or definition file, other implementation, physics
library, game executable or game run was opened or searched for.
