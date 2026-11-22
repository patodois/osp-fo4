# Missing materials, swap scopes and texture transforms

After the effect surfaces, three omission reasons came next by area in
[coverage.md](coverage.md) (the distant clouds, first by far, wait on a
measurement of the running game): missing materials (11.69 % of the omitted
area, 46,166 placements as their only family), material swaps with two or
more scopes (1.78 %, 3,316) and texture-transform controllers (1.77 %,
4,962). This file records why each placement was refused, what was a plain
lookup in this project and what is behaviour, the rules added, and what
stays refused. Nothing here has been measured in the running game. Counts
only; paths and names stay in unversioned outputs.

## Census (`coverage_census`, sub-reasons)

`coverage_census` now labels, for every omitted placement whose reasons
include one of the three, the precise sub-reason (`examples/coverage/detail.rs`;
sections `## Sub-reasons` of its summary, merged across shards). Labels are
classes of the data: the role of a file, the shape class of its path, a
field value bucket; never a path or a name. Counts below are from the whole
load order (62,393 cells, 3 shards of about 1,600 s, 0.99 GB peak), with the
census code alone, before any change below ([materials-census.txt](materials-census.txt)
holds the sections, before and after).

### Missing materials

A material file is missing when the asset graph follows a citation to a
path that no mounted archive holds (`MissingMaterial`). The asset graph now
keeps, for every dependency, how it was cited (`Asset::dependency_citations`:
the citation class and, for a shader's `Name`, the shader type and whether it
carries a texture of its own), and `fo4_scene::missing` classifies each
missing file of a placement by who cites it and whether the draw build reads
it.

- **Where the files are.** Every one of the 150,942 (placement, missing
  file) pairs is absent at its path. The data folder holds 93 loose files
  (none a material) and no archive that the manifest leaves unmounted. Case
  and slash spelling are already normalised by the virtual file system;
  no missing path exists with the other material extension or without a
  doubled root folder. The file name occurs under another folder for 7 pairs
  (all swap sources, which nothing reads); for all the others the file name
  occurs nowhere. There is no lookup bug to fix.
- **Who cites them.** Swap sources (BNAM) 115,209 pairs, swap targets (SNAM)
  whose BNAM no model of the placement cites 33,184, material parents (a
  BGSM's Root Material Path) 1,765, swap targets whose source a model cites
  755, effect-shader `Name` 29. No missing file is cited by a lighting
  shader's `Name`, or by a shader's Root Material.
- **By placement** (sole family, 46,166): 43,996 placements (11.03 % of the
  omitted area) have only missing files that the draw build never reads;
  1,395 (0.55 %) a missing material parent and nothing else read; 441 a
  missing swap target of a cited material; 2 an effect shader's name; the
  rest combinations.

The scene loader made every BNAM and SNAM path of a swap that applies to a
placement a root of its asset closure, and every missing file of the
closure a blocker. The draw build reads, per shape, the material its
shader's `Name` cites after the swaps (`draw::swap`): it compares BNAM
paths but never opens a BNAM file, and opens an SNAM file only when its
BNAM is the shape's material. A missing file the draw does not read cannot
change the frame, so the scene now sets those blockers aside: a lookup of
this project, not a claim about the game.

### Swap scopes

A placement is refused for swap scopes when two or more swaps apply to one
base: the reference's (XMSP) with the base's model swap (MODS), or a SCOL's
with a part base's. Of 3,316: reference and base 3,135; with a SCOL part
base 181. One MSWP record in both scopes in 892; FNAM present in most.
The draw build now computes, for every material a base's models name, four
readings of the order: the outermost swap alone (a reference swap replacing
the base's), the innermost alone, all from the outermost in, and from the
innermost out. They agree for every shape of 1,032 placements (0.81 % of
the omitted area) and differ for 2,284 (0.97 %). Where they differ the data
does not say which material shows.

### Texture-transform controllers

Placements omitted for `model: animation-controller blocks
(texture-transform)` (4,962 alone, 7,530 with other families):

- 4,716 (1.54 % of the omitted area) have lighting-shader float controllers
  (`BSLightingShaderPropertyFloatController`, nifxml
  LightingShaderControlledFloat 20 U Offset, 21 U Scale, 22 V Offset, 23 V
  Scale), none run by a manager (flag 0x20 clear) or named by a sequence.
  Every driven field stores 0 (an offset) or 1 (a scale), and each stored
  value equals the first key of its interpolator (2 controllers of placements
  with other families store a U offset of 1, also their first key). Where a
  controlled shader names a material, the material holds the same four UV
  values (14,943 shader rows); 760 rows name none, 47 of them with a stored
  scale other than 1 in a field no controller drives.
- 246 have effect-shader UV controllers (`BSEffectShaderPropertyFloatController`
  6 to 9) whose stored field is not their first key, so the effect rule
  already left them undecided; they stay refused.
- Two lighting controllers run by a manager with a field away from their
  first key; no `NiTextureTransformController` or `NiUVController` occurs.

**UV fields of drawn placements.** The same census counts, for drawn
placements, whether any draw has a UV offset other than 0 or a UV scale
other than 1: 1,838 placements drawn without hypotheses (1.72 % of all
area; mostly a material UV scale other than 1, 165 an offset) and 15,009
drawn under hypotheses. The reference renderer applied `uv x scale +
offset` to all of them as its own convention (render.md); nifxml names the
two fields but not their order.

## Rules

Registered in [hypotheses.tsv](hypotheses.tsv), each pending:

- `mswp_scopes_readings_agree_v1` (draw build): where the four readings
  agree for every shape of a base, that material is drawn; otherwise
  `UnsupportedRenderSwapScopes readings differ`.
- `bgsm_missing_parent_own_fields_v1` (scene, `MissingMaterialParent`): a
  material whose parent file is absent is drawn from its own fields, as
  every material is.
- `nif_missing_material_embedded_shader_v1` (scene
  `MissingMaterialEmbeddedShader`, draw build): a lighting shader whose
  material is absent is drawn with its own texture set. No placement of this
  installation needs it (above); it is the narrow named handling of a case
  the census looked for.
- `nif_controller_rest_texture_first_key_v1` (`fo4_scene::rest`, tag
  `texture-first-key`): a lighting-shader UV controller not run by a manager,
  or run by a looping sequence, shows its first key at the frame's instant;
  where every interpolator that drives it starts at the stored field, the
  field is drawn as stored. A manager-run controller in a sequence that does
  not loop keeps `nif_controller_rest_idle_shader_v1`. The draw build takes
  a material's UV fields where the shader names one, so it refuses a
  driven field whose material value differs from the NIF's
  (`UnsettledTextureMaterialField`).
- `uv_scale_then_offset_v1` (draw build): a textured surface whose UV
  offset is not 0 or whose UV scale is not 1 samples at stored UV x scale +
  offset; such a surface is drawn only under the rule
  (`UnsupportedUvTransform` otherwise). This makes explicit what the
  reference renderer did without a rule: the 1,838 placements above leave
  the frames drawn without `--under-hypotheses`.

## What stays refused, and why

- Swap scopes whose readings differ (2,284): which order the game applies
  is not settled by the data.
- A missing swap target whose source the model cites (441 alone): the draw
  would read it and it is absent; what the game draws then is not known.
- A missing effect-shader material name (29): the effect path reads the
  material's fields; not settled.
- Effect-shader UV controllers stored away from their first key (246).

## Coverage and frames

Whole load order, against the census before: 47,360 placements move from
omitted to drawn under hypotheses and 235 to hidden by the enable parent
chain (omitted area 41.96 % to 37.14 % of all); 1,838 move from drawn to
drawn under `uv_scale_then_offset_v1`. Missing materials as a sole family
fall from 46,166 placements to 469, texture-transform controllers from
4,962 to 246; swap scopes whose readings differ rise from 2,284 to 3,678
(placements no longer kept out by unread swap files). Details, test views
and frames: [coverage.md](coverage.md), Materials, swap scopes and texture
transforms.

## Provenance

LightingShaderControlledFloat, the shader UV fields and their descriptions:
the pinned nifxml copy of [nif.md](nif.md). Recognised from prior knowledge
and kept only as pending rules: that a controller's clock starts at its first
key when a cell loads, and that a shape whose material file is absent shows
its own texture set. No other NIF reader, editor, MIT reference or game
executable was opened for this step.

## Second step: differing swap scopes, swaps over shapes without material, missing textures

After the environment-map step the three largest families of the omitted
area were material swap scopes whose readings differ (19.11 %, 3,678
placements as their only family), swaps over a shape whose shader names no
material (`RenderSwapWithoutMaterial`, 14.90 %, 8,792) and missing textures
(11.71 %, 32,015); then rejected swap paths (2.36 %, 4,560). Each was
looked at with a local probe over the bases and references the census
named (names, paths and FormIDs kept outside the repository) and, for the
textures, with new `coverage_census` sub-reasons. Nothing here has been
measured in the running game.

### Swap scopes whose readings differ

For every reference of a refused base, every material path its models
name was put through each swap of its stack (outermost first) and the
four readings compared. Where they differ, the material falls in one of
three classes, counted once per placement (a placement can hold several):

- both scopes map the path, to different targets: 2,572 placements, about
  half of the family's area proxy;
- only the inner scope (the base's, or a SCOL part's) maps it: 779;
- only the outer scope (the reference's, or the SCOL's) maps it: 686;
- a swap maps a material that another swap of the stack puts in place
  (chained): 46.

Stacks: reference and base in most; reference and SCOL part base in about
280; a base and its SCOL part, or three scopes, in a handful. The reference
swaps are general swaps (also named by references of bases without swaps)
whose sources are the model's own material names, not the base swap's
targets; only the 46 chained stacks look written against an inner swap's
result.

Which material shows was settled under two pending rules, with their weak
basis stated in [hypotheses.tsv](hypotheses.tsv):

- `mswp_outer_scope_wins_v1`: each swap is compared with the path the model
  names; where the outermost swap maps it, its target is drawn. Against the
  inner-wins and inner-alone readings: under them the reference's entry for
  the shared material (2,572 placements) would never show.
- `mswp_outer_scope_replaces_v1`: where the outermost swap does not map a
  path, the path is drawn as the model names it (the outermost swap replaces
  the inner ones as a whole). The data points both ways. For it: of the 779
  placements with a material only an inner scope maps, 14 share their exact
  position with another placed reference (a door, lid or part of one
  object); in 5 the co-located part draws the inner swap's source material
  unswapped and in none only its target (a boxcar door whose base swaps blue
  paint to orange sits on a blue boxcar body; refrigerator parts; a forest
  cliff), so the replace reading keeps each object in one colour; 2 more
  show both. Against it: in 418 of the 779 the reference swap maps nothing
  in the model that the base swap does not map the same way, so their
  reference swap only undoes the base's; and the 46 chained stacks look
  written for a sequential application. Chained stacks stay refused
  (`UnsupportedRenderSwapScopes readings differ, chained`).

The draw build (`fo4_render::draw::settle`) applies the outermost swap alone
where the readings differ and no chain exists, records the rule each path
needs, and keeps the refusal where a rule is not usable at the build's
level.

**A finding left open: patterns in swap paths.** 68 of the 3,935 winning
MSWP records hold 131 BNAM/SNAM pairs with a `*` in a path (77 with one in
the source only, 35 in both, the rest with two or in the target only); every
source with a single `*` matches two or more mounted material files, and of
the 133 matches of the pairs with a `*` on both sides, 71 give a target
file that exists when the matched text is carried into the target. The
draw build compares swap paths byte for byte, so these entries are inert
in every frame, with and without hypotheses: a reading of the game's data
the project never registered. Most are the rock and ground swaps of two
landscape regions, so the frames there may show unswapped materials where
the game swaps them. Reading `*` as a pattern needs its own
rules (what it matches, which entry wins when a literal and a pattern both
match, what an absent substituted target does); none was added here.

### Swaps over a shape without a material

Every shape refused as `RenderSwapWithoutMaterial` (291 bases, 8,792
placements) is a lighting or effect shape with its own texture set or
source texture beside shapes that do name materials: branches on rock
piles, the bark of a tree, an ash layer on debris. Where the base has its
own swap the model cites every swap source in 4,063 placements, some in
1,575 and none in 162; in the rest the swap is the reference's. No such
shape's own first texture equals the diffuse of a swap's source or target
material. Swap entries pair material paths, and the draw build matches them
against the material the shader's `Name` holds; a shape without one offers
no key. `mswp_shape_without_material_untouched_v1`: the shape keeps its own
texture; the swap's entries are still read (their refusals stand) and apply
to the other shapes.

### Empty swap paths

The swap paths the scene rejects (`swap: RejectedVfsPath`) are empty BNAM
or SNAM paths: 37 pairs in 33 records (9 with both empty, 2 with an empty
source, 26 with an empty target). `mswp_empty_entry_inert_v1`: such an
entry changes nothing; the scene raises `MaterialSwapEmptyEntry` instead of
resolving the empty path, and does not read the source of a pair whose
target is empty.

### Missing textures

The asset graph cites every texture slot of a material (diffuse, normal,
smooth-specular, greyscale, glow, environment, ...), every slot of a NIF
texture set and the texture fields of an effect shader; a texture no
archive holds blocked the whole placement (`MissingTexture`), though the
unlit frame samples only a material's diffuse or base texture, a texture
set's first slot, an effect shader's source texture and, where a palette
flag is set, a greyscale texture. Probe over the 1,074 bases refused for it:
about 70 % of the family's area is three environment maps cited under a
cubemap folder that holds none while files of the same names lie under the
shared cubemap folder (921 citations); then smooth-specular, glow and normal
maps of materials, and texture set slots 1, 2 and 4; textures in sampled
slots are under a tenth of the area (missing effect source textures, a
handful of material diffuse textures, one of them present under another
folder). No reader gap was found: the paths are as stored (none sits under a
doubled root folder; one environment map is cited without an extension
while a `.dds` of that name exists, and whether the game adds it is
behaviour, in a slot the frame does not sample), and the data folder
holds no archive the manifest leaves unmounted (`coverage_census`, data
folder).

The asset graph now records the slot of every texture citation
(`fo4_scene::TextureSlot`); `fo4_scene::missing::unread_textures` sets apart
a missing texture whose every citation in the placement's closure is a slot
the frame never samples, which then carries `MissingTextureUnreadSlot`
under `texture_missing_unread_slot_v1` (what the game draws without the
file is behaviour). A missing texture cited from any sampled slot stays
`MissingTexture`. `coverage_census` labels both (`missing texture`
sub-reasons: sampled or not, citing slots, spelling class).

### Rules added in this step

All pending, in [hypotheses.tsv](hypotheses.tsv) with the measurement that
would settle each: `mswp_outer_scope_wins_v1`,
`mswp_outer_scope_replaces_v1`, `mswp_shape_without_material_untouched_v1`,
`mswp_empty_entry_inert_v1`, `texture_missing_unread_slot_v1`, and
`lighting_vertex_alpha_blend_v1` ([render.md](render.md), Vertex alpha of
blended lighting surfaces).
