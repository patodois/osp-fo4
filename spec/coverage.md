# Coverage: what is drawn, and why the rest is not

Measurement support for the static renderer and the placed actors. Nothing
here is a format fact; rules that the decisions below rely on are registered
in [hypotheses.tsv](hypotheses.tsv). Outputs with object names, editor IDs,
FormIDs or paths stay outside the repository; the counts below are
aggregates.

## The census (`coverage_census`)

An example of `fo4-unpack`. For every cell of the load order (each cell
alone, in the store's order, the loader's asset cache renewed every 32
cells and after a failed cell, one cell's scene and draw list held at a
time), or for one test view (a cell, with `--around N` the exterior cells
around it, as the viewer loads them), it loads the scene at the hypotheses
level, resolves the placed actors and builds the viewer's draw list
(`fo4_render::draw::build_with_actors`, the viewer's texture policy: stored
blocks up to 1024, alpha-tested up to 2048, 512 MiB). Every placement of
that build (placed references and placed actors, as the viewer counts
"placements") gets one outcome:

- **drawn**: drawn without `--under-hypotheses` (every rule it relies on,
  its state's and the draw build's, is supported);
- **drawn only under hypotheses**: drawn, relying on at least one pending
  rule (counted by rule and by set of rules);
- **hidden by a rule**: an editor marker under `editor_marker_hidden_v1`;
- **outside**: the base is outside the visual slice (sound markers, idle
  markers);
- **omitted**: with its reasons. For a placement whose state is blocked,
  the reasons are its blockers that no registered rule covers (or whose rule
  is refuted), each labelled by where it was raised (`record`, `swap`,
  `model`, `model citation`, `render asset`, `actor`) and by code, with the
  diagnostic's text only when that text is a record, field, block-type or
  rule label (never a path or an object name). For a placement whose state
  is drawable but whose draw failed, the reason is the draw build's refusal
  (`draw: CODE`). An actor whose look the data does not settle has the
  first word of its report line (`actor: not resolved (leveled list)`).

Reasons are also grouped into **families**: every animation-controller
block of a model (`UnsupportedAnimationRuntime`) is one family, the
particle-system blocks another, a record field is named by its record and
subrecord signatures only. Rankings: the **sole family** (resolving that
family alone would draw the placement under hypotheses), **every family**
(once per placement), the full **set of families**, and the same by exact
reason.

**Area proxy.** Each placement weighs the largest face of its base's OBND
box (six i16, `spec/plugin.md`) times the reference scale squared, so that
floors and walls weigh more than clutter. A placement without an OBND
weighs 0 and is counted. Effects with large bounds (mist volumes, light
beams) weigh as much as a room shell of the same size; the proxy is a
ranking aid, not a measure of what a frame shows.

Outputs: `summary.txt` (aggregates), `counts.tsv` (every tally, added up
across `--shard K/N` runs by `--merge`), and kept outside the repository
`bases.tsv` (reason by base record) and, for a view, `placements.tsv` (one
line per placement with its placement words).

## Before this step

Every cell of the load order, each alone (62,393 cells read; 1 deleted
cell; 3 shards of about 1,700 s, 0.9 GB peak each), with the census as
committed with the decisions below (the first count, in the census's own
commit, gave each cell the manifest's per-read memory limit and lost two
large cells to it; each cell now has eight times that):

| Outcome | Placements | Share | Area share |
| --- | ---: | ---: | ---: |
| drawn without hypotheses | 781,075 | 43.7 % | 24.1 % |
| drawn only under hypotheses | 194,061 | 10.9 % | 13.9 % |
| hidden by `editor_marker_hidden_v1` | 23,059 | 1.3 % | 0.8 % |
| outside the visual slice | 6,831 | 0.4 % | 0.0 % |
| omitted | 780,375 | 43.7 % | 61.3 % |

Of 1,785,401 placements. The largest sole families of the omitted (the
family alone keeps the placement out):

| Sole family | Placements | Area share of all omitted |
| --- | ---: | ---: |
| `swap: RuntimeMaterialSwapFilter` (an MSWP with FNAM) | 186,764 | 20.6 % |
| `record: STAT.PRPS` | 58,753 | 3.0 % |
| `record: REFR.XRFG` | 34,289 | 0.6 % |
| `draw: UnsupportedRenderAlpha` (opacity above 1; 14,400 without blending) | 35,121 | 9.6 % |
| `record: MISC.CVPA` | 28,301 | 0.0 % |
| `record: BaseTypeOutsideSlice TXST` (placed texture sets) | 26,763 | 0.0 % |
| `model: animation-controller blocks` | 24,545 | 48.6 % |
| `render asset: MissingTexture` | 19,065 | 0.5 % |
| `record: RuntimeEnableParent XESP` | 14,862 | 1.4 % |
| `draw: NoDrawableGeometry` | 7,113 | 0.0 % |

315,534 omitted placements have two or more families. Counted once per
placement, the swap filter reaches 277,543 omitted placements (31.4 % of
their area) and the animation controllers 76,011 (52.1 %).

The test views of the earlier steps (the viewer's own counts; the
exterior test cell alone and with radius 1 and 2, an interior lobby
without actors, an interior with clothed humanoids):

| View | Placements | Drawn | Under hypotheses only | Omitted (with hidden and outside) | Omitted area | Largest sole family |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| lobby | 352 | 158 | 135 | 59 | 37.6 % | animation controllers (84.5 % of omitted area) |
| vault interior | 988 | 111 | 66 | 811 | 94.7 % | swap filter (72), animation controllers (48) |
| test cell | 391 | 238 | 64 | 89 | 75.5 % | `STAT.PRPS` (48) |
| test cell, radius 1 | 2,541 | 1,500 | 294 | 747 | 62.2 % | `STAT.PRPS` (299) |
| test cell, radius 2 | 6,450 | 3,774 | 867 | 1,809 | 41.3 % | `STAT.PRPS` (642), opacity above 1 (169, 24.6 % of area) |


## Decisions on the largest reasons

Each decision was taken on counts over the winning records of the
installation (`swap_census`, `subrecord_probe`, `record_census`,
`record_dump`; outputs with names kept outside the repository). What the
data does not settle is drawn only under `--under-hypotheses`, by a
registered pending rule; nothing below changes a frame drawn without it.

**Material swaps (`mswp_fnam_folder_label_v1`).** 3,935 winning MSWP:
BNAM/SNAM entries one to 42 per record (25 records have none), each
optionally followed by one CNAM (950 of 9,059 entries); no record repeats a
BNAM, so the substitution is path for path and already applied by the draw
build for swaps without FNAM. 1,482 records carry 1,901 FNAM, stored before
the first BNAM (1,072) or right after an SNAM (829). The texts are 54
distinct words or backslash-separated paths of words (for example a game
area or a material folder), never four bytes and never a FormID; 1,148 of
the 1,901 occur inside a BNAM or SNAM path of the same record. Nothing in
the data reads them as a condition. 212,204 placed references name a swap
with FNAM by XMSP (27,322 one without), and 1,018 base records by MODS. An
FNAM is read as an editor folder label under the pending rule; the swap is
then applied as without FNAM. Swaps that stay refused: a deleted or missing
MSWP, a swap path that does not resolve (`swap: RejectedVfsPath`), a
reference swap together with a base swap (`UnsupportedRenderSwapScopes`,
their order is not established) and a swap on a model whose shader names
no material (`RenderSwapWithoutMaterial`). No swap that depends on runtime
state was found in the placed data; swaps applied by object modifications
are not placements.

**Opacity above 1 (`bgsm_opacity_above_one_test_v1`).** The draw build
refused every surface whose material or shader alpha (opacity) is above 1.
In the radius-2 test view 157 of the 169 placements so refused have
blending off, the alpha test on and alpha 1.7 (rock cliffs). Without
blending the opacity only scales the tested alpha, and clamping it to 1 or
not gives the same test, so the rule decides only whether the stored value
enters the test as values up to 1 already do in the reference. With
blending on (opacity 1.01 to 3.5) the clamp would matter: those stay
refused.

**Record properties (`base_prps_not_in_frame_v1`).** PRPS on STAT, ACTI,
FURN, CONT, FLOR, LIGH, MSTT and TERM bases (2,139 records): every one a
multiple of 8 bytes; of its 4,320 pairs the first word is a winning AVIF
record in 2,207 (editor IDs naming workshop placement properties: bounds
overrides, snapping, power, acoustic space) and a FormID below 0x800 with
no record in 2,113 (engine-defined actor values, whose meaning the data
does not give); the second word is a float. The road pieces of the
exterior test cell carry one pair, an engine-defined value set to 1. The
model is drawn as stored under the pending rule, which also covers the
collision of those bases.

**Components (`misc_cvpa_not_in_frame_v1`).** All 679 MISC CVPA are pairs
of a CMPO FormID (33 distinct, editor IDs naming crafting components) and
a count from 1 to 100.

**Reference groups (`refr_xrfg_not_in_frame_v1`).** All 70,763 XRFG are
4 bytes naming one of 9,372 RFGP records; an RFGP holds a short name
(NNAM, 7,666 of 9,368, mostly one generic word), one reference (RNAM,
7,905) and in 9 records a pack-in (PNAM), no transform, model or material.

**Unread NIF block.** `BSClothExtraData` (the only unread block type left
in placed actor models of the census: 97 actors for it alone) is, in the
pinned nifxml copy, a `BSExtraData` holding one byte array of cloth
physics; it is now classed with the other physics payloads as outside the
static frame. Placed actors are drawn in the skeleton's rest pose, where no
cloth simulation runs.

**Animation controllers: refused, unchanged.** 26,177 placements are
omitted for animation-controller blocks alone after this step (70.2 % of
the omitted area; room shells with flickering emissive panels, doors,
elevators, mist and light-beam effects). Deciding that a controller leaves
the static appearance at rest would need its interpolators' keys
evaluated against the targets' stored values (transform keys against node
transforms, float keys against shader fields), which nothing here reads
yet; no structural reader was added and these models stay refused.

**Other reasons that stay refused.** Missing materials and textures (the
files are not in the installation), placed texture sets (`TXST`, decals:
no projection is implemented), an enable parent (`XESP`, presence at run
time), records whose fields stay unread (FURN and ACTI markers and
workbench data, CONT inventory data, REFR XALP, XRGD, XATR, XPRM and
others), and the light references' LIGH.DATA flags, XLIG and XRDS (light
parameters with layouts not established).

## After the decisions

Same census, same cells (62,393 read; 1,820 s for the slowest shard):

| Outcome | Before | After | Area share after |
| --- | ---: | ---: | ---: |
| drawn without hypotheses | 781,075 | 781,075 | 24.1 % |
| drawn only under hypotheses | 194,061 | 527,821 | 31.9 % |
| omitted | 780,375 | 446,615 | 43.3 % |

333,760 placements moved from omitted to drawn under hypotheses (omitted
area from 61.3 % to 43.3 % of all); nothing changed without hypotheses. By
pending rule, the drawn-under-hypotheses placements now rely on
`mswp_fnam_folder_label_v1` 198,399 times, `base_prps_not_in_frame_v1`
64,207, `refr_xrfg_not_in_frame_v1` 54,486, `misc_cvpa_not_in_frame_v1`
28,230 and `bgsm_opacity_above_one_test_v1` 17,984 (each counted once per
placement, beside the rules of earlier steps). The largest sole families
left: missing materials (45,967) and textures (30,764), placed texture sets
(26,763), animation controllers (26,177; 70.2 % of the omitted area), the
enable parent (17,968), opacity above 1 with blending (27,610 across its
values), `NoDrawableGeometry` (7,531) and `RenderSwapWithoutMaterial`
(6,833); 209,371 omitted placements have two or more families, led by the
light references (LIGH.DATA, XLIG and XRDS together: 46,679).

| View | Placements | Drawn | Under hypotheses only, before → after | Omitted (with hidden and outside), before → after | Omitted area, before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| lobby | 352 | 158 | 135 → 139 | 59 → 55 | 37.6 % → 37.3 % |
| vault interior | 988 | 111 | 66 → 146 | 811 → 731 | 94.7 % → 81.6 % |
| test cell | 391 | 238 | 64 → 122 | 89 → 31 | 75.5 % → 63.8 % |
| test cell, radius 1 | 2,541 | 1,500 | 294 → 656 | 747 → 385 | 62.2 % → 52.8 % |
| test cell, radius 2 | 6,450 | 3,774 | 867 → 1,772 | 1,809 → 904 | 41.3 % → 25.1 % |

The remaining omitted area of the test cell is almost all one mist effect
with a very large OBND (animation controllers and an enable parent).

**Frames** (viewer, headless, 1280 x 720, against the commit before):
without `--under-hypotheses` every test view is byte-identical (lobby,
vault, test cell, dog, radius 1, dog without actors; the walk views need
the hypotheses). Under hypotheses: lobby 0 pixels differ, vault 738,642
(floors, walls and the vault door now drawn), test cell 133,188, dog view
523,900 (the road under the creature now drawn), radius 1 102,225,
`--walk --terrain --around 2` 87,412, `--path` walk 679,585 and dog view
without actors 536,605. The `--path` walk changes because the newly
covered bases also have collision under the same rules: the walked feet
stand 14.8 units higher on two legs (on a road), the waypoints are reached
as before.
