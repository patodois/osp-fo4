# Terrain of an exterior cell: LAND layouts, rules and report

An exterior cell's ground is stored in a LAND record. `fo4_plugin::land`
decodes its subrecords; `fo4_scene::terrain` places the heights in world
space under registered rules; `fo4_render::terrain` draws them as plain
geometry behind the `render` example's `--terrain` flag;
`fo4_scene::terrain::layers` turns the base and alpha layers and the vertex
colours into per-vertex texture weights under further registered rules, and
`fo4_render::terrain` draws them ("Ground textures" below); `fo4-unpack land`
reports one cell, its neighbours and, with `--census`, every exterior cell
of its worldspace. Normals are read but not used.

## Sources

Only public format documentation was opened (see
[provenance](../docs/provenance.md)). The UESP pages are documentation of
Skyrim and Oblivion; no source describes LAND for Fallout 4. The byte layouts
below are therefore applied to Fallout 4 only at the sizes the documentation
states, and every other size stays raw.

- [UESP, Skyrim Mod: Mod File Format/LAND](https://en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format/LAND),
  read as wiki source (`action=raw`) because the rendered page answered
  HTTP 403: VHGT is 1,096 bytes (f32, 33 x 33 signed bytes in row-major
  order, three unused bytes); VNML and VCLR are 3,267 bytes; DATA is four
  bytes; BTXT and ATXT are a FormID, a quadrant byte (0 bottom left, 1 bottom
  right, 2 upper left, 3 upper right) and, for ATXT, one unknown byte and a
  u16 layer; VTXT is a list of 8-byte entries. The same page states the
  accumulation of heights, the factor 8 ("assumed from Oblivion"), row 0 at
  the south edge and column 0 at the west edge, and a 128-unit lattice over a
  4,096-unit cell: those statements are the origin of the rules below, not
  facts for Fallout 4.
- [UESP, Oblivion Mod: Mod File Format/LAND](https://en.uesp.net/wiki/Oblivion_Mod:Mod_File_Format/LAND),
  wiki source: DATA is four bytes whose first byte is a bit mask of the
  subrecords present; VTXT entries are a u16 position (0 to 288 in a 17 x 17
  quadrant grid), two unknown bytes and an f32 opacity; VTEX is a list of
  FormIDs; VCLR is a 24-bit RGB grid filled from the bottom left. The three
  bytes after BTXT's quadrant are described as uncertain.
- [fopdoc, Fallout 3 LAND](https://tes5edit.github.io/fopdoc/Fallout3/Records/LAND.html):
  the same subrecord list, VNML and VCLR as 33 x 33 three-byte entries, the
  BTXT/ATXT header and the VTXT entry. Its VHGT (an f32 offset, 33 rows of
  33 bytes, three unused bytes) agrees in size with UESP but types the bytes
  as unsigned; the signed reading of UESP is used, and the difference is
  part of `land_vhgt_row_accumulation_v1`.
- [UESP, Skyrim Mod: Mod File Format/CELL](https://en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format/CELL):
  XCLC is two i32 grid coordinates and u32 flags whose bits 0 to 3 are named
  "Force Hide Land Quad". The flags are kept as stored and not applied.
- [UESP, Skyrim Mod: Mod File Format](https://en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format):
  LAND is among the records found only in subgroups; group kind 1 is a
  world's children, 6 a cell's children, 9 its temporary children. Which of a
  cell's groups holds its LAND, and how many, is not stated.

The Fallout 4 FormID remapping, compression and group reading are the ones
of [plugin.md](plugin.md).

## Typed view (`fo4_plugin::land::Land`)

| Subrecord | Size decoded | Kept as |
| --- | --- | --- |
| DATA | 4 | four raw bytes |
| VHGT | 1,096 | f32 `offset`, 1,089 `i8` in stored order, three `trailing` bytes |
| VNML | 3,267 | 1,089 three-byte entries; signedness not claimed |
| VCLR | 3,267 | 1,089 three-byte entries; colour space not claimed |
| BTXT | 8 | texture FormID (zero is `None`), quadrant, three raw bytes |
| ATXT | 8 | texture FormID, quadrant, unknown byte, u16 layer |
| VTXT | multiple of 8, after an ATXT | u16 position, two unknown bytes, f32 opacity; attached to the ATXT immediately before it |
| VTEX | multiple of 4 | FormIDs |

Any other size of these signatures is raw with `open_field =
"unverified_size"`; a VTXT not preceded by an ATXT (or by a VTXT attached to
one) is raw as `vtxt_without_atxt`; any other signature is raw. A second
DATA, VHGT, VNML, VCLR or VTEX is `DuplicateLandField`. Errors carry the file
and, for compressed records, the payload position in `decompressed_offset`.
NaN and infinite offsets and opacities are preserved and counted. Stored
heights are not combined in this crate.

`Store::get_land` reads a winning LAND; `Store::world` gives the WRLD whose
children group holds the winning occurrence of an exterior CELL (recorded
while the index is built, one entry per exterior cell).

## Rules

| Rule | Decides | Candidate |
| --- | --- | --- |
| `land_single_cell_child_v1` | which LAND is the cell's | the single winning, non-deleted LAND among the cell's child groups 8, 9, 10; none or several is a refusal |
| `land_vhgt_row_accumulation_v1` | how bytes become heights | stored row by row; first value of a row adds to a running column sum from the offset; others add to the west neighbour |
| `land_vhgt_units_8_v1` | height unit | 8 world units per unit, offset included |
| `land_grid_origin_southwest_v1` | grid orientation | row 0 south, column 0 west; rows go north, columns east |
| `land_grid_spacing_128_v1` | placement | vertex (r, c) at x = 4096 X + 128 c, y = 4096 Y + 128 r, (X, Y) from XCLC |
| `land_quad_diagonal_sw_ne_v1` / `land_quad_diagonal_nw_se_v1` | triangulation | two levels: [SW, SE, NE] + [SW, NE, NW], or [SW, SE, NW] + [SE, NE, NW] |

All are `pending`. `terrain::load` first checks the five shared rules
against the requested `RuleLevel` (`RulesNotAccepted`), then selects the
diagonal level as `collision::default_body_placement` selects a body
placement: none at `Facts`, the single supported level at `Measured`, else
the first pending level (`NoDiagonalLevel` when none). Other refusals:
`NotExterior`, `NoGrid`, `NoLand`, `MultipleLand`, `NoHeights`,
`NonFiniteHeightOffset`. Until a rule is measured, terrain exists only at
`--under-hypotheses`.

## API for consumers

`Terrain` holds the 1,089 world heights (index `33 r + c`), the cell, LAND
and world keys, the XCLC grid and flags, the diagonal level, its rules and
their state. `vertex(r, c)`, `origin()`, `bounds()`, `height_at(x, y)` (on
the triangulated surface, `None` outside the footprint), `triangle_indices()`
and `world_triangles()` (2,048 `[[f64; 3]; 3]`, counter-clockwise seen from
above; which side is solid is not claimed) are plain functions.
`Terrain::from_heights` applies the candidates without consulting the
registry, for reports and tests.

## Ground textures

No source describes how Fallout 4 draws BTXT, ATXT, VTXT and VCLR, and the
LTEX record has no documented layout here. What the data shows was measured
offline over every LAND and LTEX of one installation (base game and six
expansions, one-off probes run through this repository's readers; the probes
are not part of the repository and their figures are kept here and in the
registry only):

- every one of 128 winning LTEX has one 4-byte TNAM whose FormID resolves to
  a TXST, a 4-byte MNAM resolving to a MATT, a 2-byte HNAM, a 1-byte SNAM
  and 4-byte GNAM entries resolving to GRAS; every BTXT (27,897) resolves to
  an LTEX, as do 91,603 ATXT, while 6,934 ATXT have FormID 0 and one does
  not resolve;
- of the 128 TXST, 96 have TX00 (file names ending `_d`, BC1 or BC3, mostly
  2048 wide, no sRGB declaration), TX01 (`_n`, BC5) and TX07 (`_s`, BC5);
  32 have no texture and an MNAM material;
- of 173,928 quadrants, 140,298 have neither BTXT nor ATXT, 5,733 have ATXT
  without BTXT, and none has two BTXT; ATXT layer numbers run from 0 to 8,
  are contiguous from 0 and stored ascending in every quadrant;
- 8,246,713 VTXT entries: positions 0 to 288, never repeated within a layer,
  every opacity a multiple of 1/255 between 1/255 and 1; per vertex, the
  opacities of a quadrant's layers sum to at most 1 at all but 61 of
  9,087,604 vertices;
- seams: per-texture weights (below) at vertices that two quadrants of a
  cell share agree within 2.5/255 for 84.3% of 322,445 pairs (both bases
  present) under the candidate mapping, 51.4% or less under the two mirrored
  mappings and 21.2% or less under the five others; with over-composition
  instead of residual weights, 80.2%. Across the shared edges of
  neighbouring cells, 80.6% of 304,007 pairs agree under the candidate and
  49.2% or less under the seven other symmetries of the grid;
- a quadrant without BTXT next to one with a FormID-zero layer: in 32,299
  shared pairs where the missing base has weight, putting the neighbour's
  FormID-zero layer in its place makes 16,770 agree; the best single LTEX,
  1,147;
- VCLR (2,376 LAND): on shared cell edges, 68,705 of 69,149 vertex pairs
  that are not white on both sides are byte-equal in the VHGT frame, 47.2%
  or less under the other symmetries; 151,456 pairs are white on both sides.

These indications are why the candidates below were chosen; none is a
measurement of the running game, and all the rules are `pending`:

| Rule | Decides | Candidate |
| --- | --- | --- |
| `land_quadrant_layout_v1` | which vertices a quadrant covers | quadrant q: rows 16 floor(q/2) to +16, columns 16 (q mod 2) to +16; edge vertices shared |
| `land_vtxt_position_grid_v1` | where a VTXT entry lies | position p: quadrant row floor(p/17), column p mod 17; unnamed vertices opacity 0 |
| `land_layer_weights_residual_v1` | how layers combine | opacity is the layer's weight; base weight 1 minus their sum (0 above 1); weights interpolated across triangles; colour = sum of weight times texture colour; texture alpha unused |
| `land_missing_base_default_v1` | a missing base, a FormID-zero layer | both stand for one default texture no record names; its identity is the optional rule below |
| `ltex_tnam_texture_set_tx00_v1` | which file draws an LTEX | TX00 of the TXST its TNAM names; a TXST without TX00 is refused |
| `land_texture_repeat_256_v1` | texture coordinates | placeholder: one repeat per 256 world units, cell-aligned, u east, v south |
| `land_vclr_multiply_v1` | vertex colours | VCLR bytes / 255 multiply the colour, interpolated, in the stored values |

Two further rules are optional: each applies only where the level accepts
it (today `--under-hypotheses`), and what it covers stays refused
otherwise, as before it existed.

| Rule | Decides | Candidate |
| --- | --- | --- |
| `ltex_texture_set_material_diffuse_v1` | a texture set with no TX00 and an MNAM material | the material's diffuse texture, if the material is a BGSM with a non-empty diffuse path; its UV and tiling fields are not applied |
| `land_default_texture_world_named_v1` | which texture the default is | the colour (TX00, or the material's diffuse) of the single texture set that some LTEX's TNAM names and whose editor ID contains, ignoring ASCII case, the cell's worldspace editor ID and `default`; none or several is a refusal |

What the installation says about the default (offline, one installation,
one-off probes through this repository's readers; the probes are not part
of the repository):

- no record names it: none of 2,049 GMST is a landscape or default ground
  setting; the installation's default and quality `.ini` files have no such
  key; no WRLD subrecord, and no 4-byte-aligned value of any other
  non-LAND record (2,146,031 scanned), resolves to an LTEX, apart from 16
  scattered values in INFO, MESG, TERM and NAVM data;
- of the 74 distinct texture sets that LTEX name, one has `default` in its
  editor ID; that editor ID contains the editor ID of one of the 13
  worldspaces (the main one), and of no other. The set has no TX00; its
  material's diffuse decodes. Its two LTEX carry 20 vertex weights in all
  LAND, while the default carries 83% of all ground weight;
- the data cannot confirm it: merging the default with that set changes
  none of 4,473,014 shared seam vertex pairs where the default has weight
  in that worldspace; comparing the terrain LOD tiles over 131,563
  default-only quadrants ranks it first of 74 sets, but by 0.0001 RMS, and
  the same comparison finds a single-texture quadrant's own texture for
  only 5 to 10 of 349 of them, so it does not discriminate;
- in the test exterior cell, 64 placed draws (ground meshes and decals) use
  the set's diffuse texture.

The other twelve worldspaces have no such texture set; their default stays
grey (`NoNamedDefaultTexture`). Of the 128 LTEX, the 32 whose texture set
holds only a material all resolve to a BGSM whose diffuse decodes (18
distinct texture sets).

The repeat is a placeholder in the sense of the walk rules: the placed
objects of one exterior cell have an area-weighted median of 0.148 world
units per full-resolution texel (quartiles 0.100 and 0.188), about 303 units
for a 2048-texel landscape texture; 256 is the nearest whole number of quads.
Nothing in the data states it, and the image's mirror is not decided.

`layers::load(store, terrain, level, budget)` refuses with
`TextureRulesNotAccepted` unless the level accepts the seven rules (only
`--under-hypotheses` today), or `NoLand`. Otherwise `Layers` holds, per
quadrant, its distinct sources (`Ltex(FormKey)` or `Default`) with their
weights at the 289 quadrant vertices, base first and layers by ascending
number, a source named twice summed into one; the count of vertices whose
layer opacities exceed 1; and the VCLR multipliers. A quadrant with two
BTXT, a VTXT position above 288, a repeated position, an opacity outside
[0, 1] or a FormID that does not resolve is refused alone
(`MultipleBaseLayers`, `LayerPositionOutsideQuadrant`,
`DuplicateLayerPosition`, `LayerOpacityOutsideUnitRange`,
`UnresolvedBaseTexture`, `UnresolvedLayerTexture`).
`layers::colour_texture` returns an LTEX's colour, `TextureSet` (the TX00
path) or `Material` (the MNAM path of a set without TX00), or one of
`MissingLandscapeTexture`, `NotALandscapeTexture`,
`LandscapeTextureWithoutSingleTnam`, `LandscapeTextureTnamSize`,
`LandscapeTextureNullTnam`, `LandscapeTextureTnamNotTextureSet`,
`TextureSetWithoutColour`; `colour_texture_path` keeps TX00 only
(`TextureSetWithoutTx00` for a material). `layers::default_texture_set`
returns the texture set named as a world's default, or `MissingWorld`,
`NotAWorld`, `WorldWithoutEditorId`, `NoNamedDefaultTexture`,
`SeveralNamedDefaultTextures`; `texture_set_colour` reads a set's colour.

## Drawing

`render ... --terrain` loads the rendered cell's terrain at the render's
level. When built, `fo4_render::terrain::append_textured_terrain` draws it
with its ground textures, tagged after the last placed instance:

- the TX00 of each LTEX is resolved as a TXST texture citation (path rules
  as for objects, reported in `texture_path_rules`); under
  `ltex_texture_set_material_diffuse_v1` a set without TX00 is read through
  its MNAM (a TXST material citation) with `fo4-material`, and the
  material's diffuse is resolved as a material texture citation, refused
  as `LandscapeMaterialNotLighting` or `LandscapeMaterialWithoutDiffuse`
  when it cannot draw, and as `TextureSetWithoutTx00` when the rule is not
  accepted. Each resolved path is decoded once per draw list, whatever
  record names it, with the limits of the build options (in the reference,
  first level at most 256 texels wide, coarser levels while a separate
  96 MiB terrain texture budget lasts; the viewer keeps stored blocks, see
  [render.md](render.md)); its alpha is set to 1 (stored blocks are marked
  opaque);
- under `land_default_texture_world_named_v1` the default weight of the
  terrain's worldspace is drawn like any texture with the texture its
  world names, once per world; `default_texture` in the piece's report says
  `drawn` or why not (`DefaultTextureRuleNotAccepted`, `TerrainWithoutWorld`
  and the refusals above), and the summary's `default_textures` counts the
  worlds by outcome, `texture_rules` the optional rules a drawn texture
  used;
- each quadrant is drawn as passes over its 512 triangles: the first pass
  opaque, its vertex colour the VCLR multiplier times the texture's weight;
  each further texture an additive overlay (source alpha times source plus
  destination, CPU blend codes 6 and 0) whose vertex colour is the VCLR
  multiplier with the weight in alpha, without the triangles whose three
  weights are zero. The frame then holds the weighted sum whatever the pass
  order. Overlay draws are listed in `DrawList::overlays`: their samples
  are kept at exactly the stored opaque depth (every other draw needs to be
  strictly nearer), and their tag never replaces a stored one;
- a default its world does not name, a refused LTEX (`landscape_textures`
  and `texture_refusals` give the codes) and a refused quadrant are drawn
  as the plain grey below with their weight, so the untextured share stays
  visible;
- if `layers::load` refuses, the piece is drawn plain grey as before.

`append_terrain`, the plain drawing, is one opaque double-sided draw with a
1 x 1 white texture and per-triangle grey vertex colours (0.30 to 0.85 by
the angle of the geometric normal to a fixed direction). `summary.json`
gains a `terrain` key with the outcome or the refusal and, per piece,
`textures`: quadrants textured and untextured with reasons, layers, passes,
overlay triangles, `weight_share` (summed weight per quadrant vertex of
`texture`, `default_texture` (the default drawn with its named texture),
`default` (drawn grey) and `refused`), texture refusals, vertices above
unity and whether VCLR was used. Without the flag the draw list and summary are
unchanged; frames were compared byte for byte before and after this change.
With `--around N` ([scene.md](scene.md)) every built piece of the cells
within N grid steps is drawn the same way, the cell first, tagged in piece
order after the last placed instance, sharing one texture cache, and the
`terrain` key lists the pieces.

## Report: `fo4-unpack land PROFILE EditorID [--out DIR] [--strict|--measured|--under-hypotheses] [--census]`

Writes `summary.json`, `items.jsonl`, `diagnostics.jsonl` and `land.tsv`:

- the outcome of `terrain::load` at the level, the rules and their status;
- the cell's LAND: subrecords and sizes, decoded fields, raw bytes, height
  range under the candidates;
- the eight neighbours by XCLC in the same world (a grid claimed by two
  cells is ambiguous and neither is used) and, for the four that share an
  edge, the largest height gap;
- edge continuity: for the candidate accumulation and a transposed control
  (first row along columns, then each column), under each of the eight
  symmetries of the square, how many shared edges agree within 0.001 world
  units. Edges between two entirely flat cells agree under every variant and
  are counted separately. One edge direction cannot distinguish a mirror
  along it, so east and north edges are needed together;
- objects against ground, for the cell and its eight neighbours under
  hypotheses: per placed reference with collision, the lowest point of its
  collision box, of its render points and its origin, minus the ground under
  the box centre. Distributions are reported for the candidate and, for the
  collision bottom, for height factors 4 and 16, the other seven
  orientations and the other diagonal. This tests the height unit and
  orientation without any measurement of the game; it cannot separate the
  diagonals at this resolution;
- `--census`: every exterior cell of the world, one LAND at a time (only
  offset and stored bytes are kept per cell): LAND per cell, group kinds,
  subrecord sizes, raw byte histograms (pooled after 64 values), XCLC hide
  bits, height range, continuity of every shared edge and corner, and one
  item per unequal candidate edge with the plugins of both LAND winners.

The figures of the first run are kept with the local reports, not here.

## Open

- Live measurements in the `live_measurement` column of each rule.
- Which texture the default is: the candidate rests on an editor ID alone,
  and nothing names a default for the other twelve worldspaces. The
  material of a material-only texture set is read for its diffuse only;
  its UV transform, tiling distance and other textures are not applied.
- The texture repeat and its mirror (placeholder); the LTEX fields other
  than TNAM (MNAM, HNAM, SNAM, GNAM) are not used.
- The seam disagreements that remain under the candidates (15.7% of
  quadrant seams, 19.4% of cell edges); whether the game shows them is not
  known. Each quadrant is drawn with its own weights, so a disagreement
  shows as a straight tone step along the quadrant or cell border, as
  stored: at the centre vertex of the test exterior cell three quadrants
  give one texture full weight and the fourth gives it none, and one
  quadrant paints a texture up to its edge that its neighbour has no layer
  for. Copying one side's weights onto the shared vertices, as an offline
  diagnostic only, removes the step in both renderers; no blending across
  quadrants is applied, since nothing in the data or the registry states
  one.
- The few unequal edges of the census, which are concentrated away from the
  map centre; whether they are seams the game also shows is not known.
- DATA bits, the trailing VHGT bytes, BTXT's raw bytes, ATXT's unknown byte,
  VTXT's unknown bytes, VNML, the XCLC hide bits and an unknown LAND
  subrecord seen in a few records stay raw or unused.
