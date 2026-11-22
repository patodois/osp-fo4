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

See [coverage.md](coverage.md), Materials, swap scopes and texture
transforms.

## Provenance

LightingShaderControlledFloat, the shader UV fields and their descriptions:
the pinned nifxml copy of [nif.md](nif.md). Recognised from prior knowledge
and kept only as pending rules: that a controller's clock starts at its first
key when a cell loads, and that a shape whose material file is absent shows
its own texture set. No other NIF reader, editor, MIT reference or game
executable was opened for this step.
