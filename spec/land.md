# Terrain of an exterior cell: LAND layouts, rules and report

An exterior cell's ground is stored in a LAND record. `fo4_plugin::land`
decodes its subrecords; `fo4_scene::terrain` places the heights in world
space under registered rules; `fo4_render::terrain` draws them as plain
geometry behind the `render` example's `--terrain` flag; `fo4-unpack land`
reports one cell, its neighbours and, with `--census`, every exterior cell
of its worldspace. Landscape textures, normals and vertex colours are read
but not used for drawing.

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

## Drawing

`render ... --terrain` loads the rendered cell's terrain at the render's
level. When built, it is appended as one opaque double-sided draw with a
1 x 1 white texture and per-triangle grey vertex colours (0.30 to 0.85 by the
angle of the geometric normal to a fixed direction), tagged after the last
placed instance; `summary.json` gains a `terrain` key with the outcome or the
refusal. Without the flag the draw list and summary are unchanged; frames
were compared byte for byte before and after this change. With `--around N`
([scene.md](scene.md)) every built piece of the cells within N grid steps is
drawn the same way, the cell first, tagged in piece order after the last
placed instance, and the `terrain` key lists the pieces.

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
- The few unequal edges of the census, which are concentrated away from the
  map centre; whether they are seams the game also shows is not known.
- DATA bits, the trailing VHGT bytes, BTXT's raw bytes, ATXT's unknown byte,
  VTXT's unknown bytes, VNML, VCLR, the XCLC hide bits and an unknown LAND
  subrecord seen in a few records stay raw or unused.
