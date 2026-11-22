# Navigation meshes

What the plugins store for actors to move on, how much of it `fo4-nav`
reads, how far the reading is internally consistent, how it agrees with the
terrain and collision the project already reads, and a path search over it.
Nothing here moves an actor in the viewer yet.

`fo4_nav::NavmeshRecord::decode` reads a whole `NAVM` record: its `NVNM`
geometry subrecord (`fo4_nav::Navmesh::read`) and the `ONAM`, `NNAM` and
`MNAM` subrecords after it. `fo4_nav::NavInfo::decode` reads the `NAVI`
navigation-info record. Every byte of a subrecord belongs to a counted
section (`fo4_nav::mesh::Parts`) or the record is refused with a named
error and its offset; words whose meaning is not established are kept as
stored and only counted. FormIDs are kept as stored, local to the plugin
they were read from.

Every statement below comes from measuring the stored bytes of one
installation (seven plugins). Parts of the layout were recognised from
prior general knowledge before they were measured (listed at the end and in
[provenance](../docs/provenance.md)); each meaning is a pending rule in
[hypotheses.tsv](hypotheses.tsv) and is kept only where the census, which
could have refuted it, holds. No editor, tool, format write-up, wiki page,
source code of any other implementation, decompilation or the game
executable was opened or searched for. Field names are this project's
labels; record and subrecord signatures are the stored four-character
codes.

Evidence tags as in [havok-container.md](havok-container.md):
**[measured N]** holds for N items and fails for none unless a second count
says so; **[hypothesis]**; **[open]**.

## Census scope

`nav_census MANIFEST OUT_DIR` (an example of `fo4-nav`) opens the plugins
with `fo4-plugin`, rereads every `NAVM` and `NAVI` occurrence of the load
order one record at a time (winning and overridden, through
`Store::raw_occurrence`), decodes it, and then checks the winning live
meshes against themselves, against each other, against the cells, placed
doors and terrain. It writes counts only to `navmesh-census.txt`,
committed as [navmesh-census.txt](navmesh-census.txt), and refused records
with their file offsets to a second file that is not versioned (empty in
this run). It takes about 25 s and 570 MB.

`nav_path MANIFEST OUT_DIR CELL[:RADIUS]...` loads the navmeshes of the
project's test cells, joins them, counts components, runs a sample path
and compares vertex heights with the cell's placed collision; its counts
are committed as [navmesh-path.txt](navmesh-path.txt) (cells numbered, not
named; a top-down picture of mesh and path stays in the local notes).

## Records

| | Count |
| --- | ---: |
| `NAVM` occurrences | 12,565 (12,411 winning, 154 overridden) |
| deleted (all winning) | 8 |
| live occurrences read to the last byte | **12,557 of 12,557** (none refused) |
| record payload bytes read to the last byte | 218,520,167 of 218,520,167 |
| `NAVI` occurrences | 7 (one FormID, defined by the first master and overridden by every other plugin) |
| `NAVI` read to the last byte | **7 of 7**, 2,588,489 bytes |

Every `NAVM` record has version 131 and the compressed flag set. Subrecord
sequences: `NVNM MNAM` 12,457, `NVNM ONAM NNAM MNAM` 98, `NVNM NNAM MNAM`
2; the reader accepts exactly `NVNM [ONAM] [NNAM] MNAM` **[measured
12,557]**. The payload splits into `NVNM` 218,240,999 bytes, `MNAM`
118,070, `NNAM` 2,294, `ONAM` 392 and subrecord framing 158,412.

## `NVNM` layout

`nav_nvnm_layout_v1`. All little-endian; every count is a `u32`, checked
against the bytes left before anything is allocated.

| Section | Element | Bytes in the census |
| --- | --- | ---: |
| header | `u32` version (15 in all), `u32` magic (0xA5E9A03C in all), `u32` world, `u32` parent | 200,912 |
| vertices | three `f32` (12 bytes) | 52,977,716 |
| triangles | three `u16` vertices, three `i16` edge words, 4 bytes, 5 flag bytes (21 bytes) | 117,244,319 |
| edge links | `u32` kind, `u32` navmesh FormID, `u16` triangle, `u8` edge (11) | 4,739,880 |
| door links | `u16` triangle, `u32` word, `u32` door FormID (10) | 62,188 |
| cover segments | `u16`, `u16`, `u16` data, `u16` flags (8) | 14,379,796 |
| cover mappings | `u16` triangle x 4 + edge, `u16` segment (4) | 3,537,424 |
| waypoints | three `f32`, `u16` triangle, `u32` flags (18) | 323,756 |
| grid | `u32` divisions, two `f32` sizes, six `f32` bounds, then divisions squared lists of counted `u16` triangles | 24,775,008 |

Per mesh (all live occurrences): vertices 3 to 11,645 (4,410,624 in all),
triangles 1 to 14,988 (5,580,671), edge links up to 250 (426,332), door
links up to 19 (1,196), cover segments up to 4,122 (1,791,196), cover
mappings up to 3,059 (871,799), waypoints up to 426 (15,196), grid
divisions 1 to 12.

**Header** (`nav_header_parent_v1`). Word 2 is zero for the 906 meshes of
interior cells and non-zero for the 11,497 of exterior cells (winning
meshes, by the cell group that holds them). With word 2 zero, word 3 is the
FormID of that parent cell **[measured 906]**. Otherwise word 2 names the
world whose group holds the parent cell **[measured 11,497]** and word 3
holds the cell grid as two `i16`: x in the high half, y in the low half
**[measured 11,497]** (the swapped reading holds for only 360). Vertices
are world positions: 4,015,764 of 4,017,912 exterior vertices lie inside
the parent cell's 4096-unit square; of the 2,148 outside, 95% are within
2.1 units.

## Triangles

`nav_triangle_v1`. Edge `i` runs from vertex `i` to vertex `i + 1`. Its
word is `-1` for no neighbour, a triangle index of the same mesh, or, when
the link bit is set, an index into the edge links. Over the winning
meshes: 14,472,430 neighbour words, 1,625,095 open edges, 419,992 link
edges and no other word **[measured 16,517,517]**.

- Mutual neighbours: the neighbour names the triangle back in 14,472,412
  of 14,472,430 cases, across the same two vertices in reverse order in
  14,472,342 (any order 14,472,366); no triangle names itself.
- Byte 17, bits 0 to 2 (`nav_triangle_link_bits_v1`): the edge's word is
  an edge-link index. Found by testing each bit of the five flag bytes
  against the edge-link count. In the winning meshes the 419,992 flagged
  edges name each of the 419,992 edge links exactly once **[measured
  419,992]**. Bit 6 (0x40) of byte 17 is set on about 1.79 million
  triangles and is not interpreted.
- Byte 19, bits 0 to 2 (`nav_triangle_cover_bits_v1`): the edge has a
  cover mapping; bits 3 to 7 are zero in every triangle. The number of
  cover mappings equals the number of set bits in 12,097 of 12,403
  winning meshes.
- Bytes 12 to 15 read as an `f32` are the largest finite value in
  4,462,003 triangles and another finite value in 1,118,668; the finite
  values minus the triangle's highest vertex spread from below -4096 to
  above +4096 (median -461), so they are not a height above the triangle
  **[open]**.
- Byte 16 values (all occurrences): 0x00 3,962,532, 0x04 1,184,865, 0x10
  292,745, 0x02 105,692, 0x01 33,763 and 7 rarer values; byte 18: 0x08
  5,551,701, 0x00 27,741, 0x0C 1,161, 0x0A 50, 0x04 18; byte 20 is zero in
  all **[open]**.

## Edge links

`nav_edge_link_v1`. Over the winning meshes:

| Check | Holds | Fails |
| --- | ---: | ---: |
| names a winning live `NAVM` | 419,848 | 144 (no record) |
| its triangle is inside the linked mesh | 419,819 | 29 |
| mutual: the linked triangle links back to this triangle | 419,739 | 80 |
| mutual pairs: the edge byte is the linked triangle's edge that links back | 419,739 | 0 |
| mutual pairs: the same kind word both ways | 399,171 | 20,568 |

Kind words: 0 in 405,549 links, 1 in 10,392 and 2 in 10,391 (all
occurrences). Mutual pairs are kind 0 with 0 (399,166), 1 with 2 (10,284
each way), 1 with 1 (3) and 2 with 2 (2). Kind-0 links join different
meshes; 17,381 links of kinds 1 and 2 name their own mesh, and in every
such pair but three the two edges' end points are at least 16 units apart:
these join distant edges of one mesh. For links between meshes the two
edges' end points coincide exactly in 174,550 pairs, within 1 unit in
64,814 more, within 16 in 136,516 and farther in 26,478 (edges that only
partly overlap). What the kinds mean to an actor **[open]**.

## Door links

`nav_door_link_v1`. All 1,189 winning door links name a placed reference
whose base is a `DOOR` **[measured 1,189]**, with the triangle in range;
637 doors are placed in the mesh's own cell and 552 in another cell. The
door stands 16 to 256 units from its triangle's centroid in 1,100 cases
(median 79). The word between triangle and door is one value in all 1,196
occurrences and is kept raw.

## Cover

`nav_cover_segment_v1`, `nav_cover_map_v1`. A cover mapping's first word
is the triangle times four plus the edge (found by matching against the
byte-19 bits): 860,718 of 860,718 winning mappings name a segment in range
and an edge below 3, 860,369 a triangle in range, and 858,645 an edge with
its cover bit set (857,801 an open edge). 997,046 segments are named by no
mapping.

The segments are framed but not understood. The first segment of a mesh
has flags bit 15 in 11,345 of 11,345 meshes; a segment without bit 15
starts where the previous one ends in 1,223,173 cases and not in 259,439;
only 1,520,572 of 3,528,534 point words are below the vertex count, and
the others cluster at fractions of 65,535 (32,767, 21,845, 43,690, ...).
Data and flag words are kept raw (9,717 and 104 distinct values)
**[open]**.

## Waypoints

`nav_waypoint_v1`. 14,861 of 15,196 winning waypoints name a triangle in
range; 14,268 of those lie inside it seen from above, and their height is
within one unit of the triangle's plane for 11,751. Flags are zero in all.

## Search grid

`nav_grid_v1`. The sizes equal (maximum - minimum) / divisions in 12,403
of 12,403 winning meshes; the x and y bounds equal the vertex bounds in at
least 12,381; the z maximum never equals the vertex maximum (it equals the
larger of the vertex maximum and the largest triangle word of bytes 12-15
in 10,120). Squares are numbered row (y) times divisions plus column (x)
from the minimum corner: under that numbering 499,279 of 672,402 squares
list exactly the triangles that meet them, against 66,764 under the
column-first numbering.

"Each square lists exactly the triangles overlapping it" does **not** hold
under any criterion tried (counts over the 672,402 squares):

| Criterion for "meets" | Squares exact | Listed, not meeting | Meeting, not listed |
| --- | ---: | ---: | ---: |
| triangle overlaps the square's interior | 503,850 | 486,833 | **118** |
| triangle touches the square (0.001 slack) | 499,279 | 71,121 | 437,445 |
| triangle within 16 units | 280,527 | 51,920 | 1,251,164 |
| bounding box touches the square (0.001 slack) | 246,996 | **0** | 1,146,532 |
| bounding box overlaps the interior | 285,187 | 404,307 | 554,218 |

What holds is a bracket: every listed triangle's bounding box touches its
square (no exception), and every triangle overlapping a square's interior
is listed there (118 exceptions). The rule that picks between the two
**[open]**.

## `NAVM` companions

`nav_record_companions_v1`. `ONAM` (98 records) is one FormID naming a
`STAT` (91), `FURN` (6) or `ACTI` (1). `NNAM` (100) is a strictly
ascending list of vertex indices (1,147 entries, all in range). `MNAM` is
empty in 12,343 records; the other 214 are framed to the last byte as
groups of a FormID, a `u16` count and that many triangle indices: 4,310
group FormIDs name a placed `REFR` (92 no record, 42 do not resolve),
45,549 of 45,551 triangle indices are in range. Their roles **[open]**.

## `NAVI`

`navi_nvmi_layout_v1`, `navi_nvpp_layout_v1`. `NVER` is `u32` 15. Each
`NVMI`, in order: `u32` navmesh FormID, `u32` flags, three `f32`, one
`f32`, a counted list of FormIDs, a second counted list of FormIDs, a
counted list of (`u32` word, `u32` door FormID) pairs, a byte that is 1
exactly when flags bit 5 is set and then carries island data (two corners,
counted `u16` triangle triples, counted vertices), and last the magic,
world and parent words of the mesh's `NVNM` header. `NVPP` is a counted
list of sets of navmesh FormIDs and a counted list of (navmesh FormID,
set index) pairs.

| Check (12,567 `NVMI` in 7 occurrences) | Holds | Fails |
| --- | ---: | ---: |
| read to the last byte | 12,567 | 0 |
| names a winning live `NAVM` | 12,559 | 8 (deleted meshes) |
| world and parent equal the mesh's header | 12,559 | 0 |
| position inside the mesh's vertex box | 12,558 | 1 |
| pair door words = the mesh's door links | 12,555 | 4 |
| first list = the meshes its edge links name | 9,627 | 2,932 |
| both lists together = the meshes its edge links name | 10,401 | 2,158 |
| first list = other meshes of kind-0 links | 10,182 | 2,377 |
| second list = other meshes of kind-1 or kind-2 links | 9,766 | 2,793 |
| island vertex equals a vertex of the mesh | 3,941 | 74,176 |

The two lists name navmeshes in all 30,735 entries (11 deleted ones); what
selects them **[open]**. The island data is not a copy of the mesh's
geometry **[open]**. Flags: 0x20 7,506, 0x00 4,977, 0x40 72, 0x60 12.

`navi_occurrences_combined_v1` **[hypothesis]**: each plugin's `NAVI`
lists the navmeshes that plugin defines or overrides (12,449 of 12,559
`NVMI` name a mesh with an occurrence in the same plugin), so the winning
occurrence alone names only 1,933 of the 12,403 winning live meshes while
all occurrences together name 12,303. Taking only the last override, as
`fo4-plugin` does for every record, would drop most entries; nothing in
`fo4-nav` merges them. The seven `NVPP` hold 196 sets each, every member
and pair FormID naming a `NAVM` and every set index in range; their bytes
differ between occurrences.

## Agreement with terrain and collision

**Terrain.** For every vertex of a winning exterior mesh whose parent
cell's terrain builds (11,432 meshes; 61 cells have no `LAND`, 4 no
heights; 2,146 vertices fall outside the cell's terrain), the vertex
height minus the terrain height at its x, y (`fo4_scene::terrain`, parent
cell only, the registry's default diagonal; terrain rules as in
[land.md](land.md)):

| Vertex z - terrain z | Vertices |
| --- | ---: |
| below -256 | 992 |
| -256 to -64 | 4,274 |
| -64 to -16 | 29,187 |
| -16 to -4 | 93,888 |
| -4 to -1 | 142,711 |
| **-1 to 1** | **1,694,493** |
| 1 to 4 | 228,458 |
| 4 to 16 | 442,296 |
| 16 to 64 | 611,739 |
| 64 to 256 | 490,554 |
| 256 to 1024 | 203,865 |
| 1024 and above | 68,141 |

Of 4,010,598 vertices, 42% lie within one unit of the terrain and 52%
within four; 3% lie more than four units below it and 45% more than four
above it (quantiles: 5% -2.1, 25% 0.0, median 1.3, 75% 37, 95% 357). The
mesh follows the ground where it is on the ground and lies on placed
objects, floors and roofs elsewhere; the small negative tail is where the
mesh dips under the triangulated terrain. This agrees with the terrain
reading at the level of a unit; it does not single out the diagonal rule.

**Collision**, test cells only ([navmesh-path.txt](navmesh-path.txt)):
for every vertex of the cell's own meshes, the vertex height minus the
nearest surface on its vertical line in a `fo4_walk::World` built from the
cell's placed collision and the terrain around it. Interior test cell: 268
of 528 vertices within 0.25 units, 276 within 1, 320 within 4 (median 0,
75% 6.1); 7 vertices have no surface under or over them. Exterior test
cell: 625 of 1,229 within 0.25, 691 within 1, 806 within 4, 75% within 7.9
units; 61 lie more than 64 units above the nearest surface. Where the two
disagree by more than a few units the census does not say which side is
missing something (refused collision shapes, objects without collision, or
mesh over gaps) **[open]**.

## Path search

`fo4_nav::NavGraph` (`nav_path_search_v1`) is this project's own search;
it claims nothing about the game's. Nodes are triangles; arcs follow
neighbour words and the edge links whose kind the caller chooses to follow
and whose navmesh the caller resolves to a loaded mesh. A* runs over
triangle centroids (step cost and heuristic are centroid distances, so the
centroid route found is the shortest); the corridor's portals are then
pulled tight by a funnel in the horizontal plane, each corner keeping the
height of the portal vertex it sits on. Synthetic tests cover a straight
corridor, a detour around a hole (exact corners and length), a cut mesh
(two components, no path), two meshes joined by a mutual edge link, kinds
not followed, and malformed input.

Test cells ([navmesh-path.txt](navmesh-path.txt)):

| | Interior test cell | Exterior test cell, radius 1 |
| --- | ---: | ---: |
| navmeshes (cell / all cells) | 39 / 39 | 7 / 38 |
| triangles (cell / all cells) | 523 / 523 | 1,855 / 17,876 |
| edge links (all cells) | 0 | 1,419 (473 to meshes outside the 9 cells) |
| components, kind-0 links | 39 (largest 300 triangles) | 18 (largest 17,347) |
| components, every kind | 39 | 12 (largest 17,637) |
| sample path: straight line | 5,194 | 17,248 |
| corridor triangles, meshes crossed | 31, 1 | 257, 5 |
| polyline length (ratio to straight line) | 5,322 (1.025) | 17,793 (1.032) |
| nodes expanded | 151 | 9,533 |

The sample path joins the two triangles of the largest component found by
a double sweep over centroid distance. The interior test cell holds 39
separate navmeshes with no edge link between them, so each is its own
component; whether the game joins them in some other way **[open]**.

## Open

- What the kind words of edge links mean, and whether actors use kind 1
  and 2 links (they join distant edges of one mesh).
- Triangle bytes 12 to 15, byte 16, byte 17 bit 6, byte 18.
- Cover segments: the point words that are not vertex indices, the data
  and flag words, chains that do not continue.
- The exact grid listing rule (a bracket holds).
- `ONAM`, `NNAM`, `MNAM`; the two `NVMI` lists, its `f32`, the island
  data, `NVPP`; how the game combines `NAVI` occurrences.
- How the game's actors choose a path: not measurable here; nothing in
  this project runs the game's planner.

## Rules registered

`nav_nvnm_layout_v1`, `nav_header_parent_v1`, `nav_triangle_v1`,
`nav_triangle_link_bits_v1`, `nav_triangle_cover_bits_v1`,
`nav_edge_link_v1`, `nav_door_link_v1`, `nav_cover_segment_v1`,
`nav_cover_map_v1`, `nav_waypoint_v1`, `nav_grid_v1`,
`nav_record_companions_v1`, `navi_nvmi_layout_v1`, `navi_nvpp_layout_v1`,
`navi_occurrences_combined_v1`, `nav_path_search_v1`, all pending, in
[hypotheses.tsv](hypotheses.tsv) (`fo4_nav::rules`).

## Provenance

No format fact here has a cited source. Recognised from prior general
knowledge before measurement, and kept only because the census could have
refuted it and did not:

- the record signatures `NAVM` and `NAVI` and the subrecord names `NVNM`,
  `ONAM`, `NNAM`, `MNAM`, `NVER`, `NVMI`, `NVPP`, with `NVNM` as the
  geometry and `NAVI` as per-navmesh information;
- `NVNM` opening with a version and a constant check word, then a world
  word that is zero inside, then the parent cell FormID or the grid as two
  `i16` (which half is x was decided by the census);
- the section order vertices, triangles, edge links, door triangles, cover
  data, cover-to-triangle mappings, waypoints, grid, and vertices as three
  `f32`;
- a triangle as three `u16` vertices and three `i16` edge words with -1
  for none, followed by flag and cover fields (from an older, shorter
  layout; the 21-byte size, the `f32` and the five bytes were measured);
- an edge link as a type word, a navmesh FormID and a triangle (the edge
  byte and its meaning were measured);
- a door link as a triangle, a word and a door reference;
- waypoints as a position, a triangle and flags;
- cover entries as pairs of vertex indices with a data word;
- the grid as a division count, two square sizes, the bounds and one
  triangle list per square;
- `NVMI` as navmesh, flags, an approximate position, a float, lists of
  connected navmeshes and doors, an island flag with island data, and the
  parent world and cell; `NVPP` as preferred-pathing lists.

Found by measurement, not recalled: the byte-17 link bits, the byte-19
cover bits, the cover mapping packing (triangle times four plus edge), the
edge byte of an edge link as the linked triangle's edge that links back,
the x-high grid word, the grid's row-major numbering and the bracket it
satisfies, the `NVMI` island byte matching flags bit 5, the `ONAM`, `NNAM`
and `MNAM` framings, and every count above. Recalled details that did not
hold: cover entries are not plain vertex pairs (most point words are not
vertex indices), and the grid does not list exactly the triangles that
overlap each square.
