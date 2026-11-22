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

## Root node transform

Under `nif_root_transform_replaced_v1` ([nif.md](nif.md), Root node
transform) the draw build leaves a placed model's root node transform out
and collision follows. This changes how some placements are drawn under
hypotheses, never whether: the census counts them apart ("drawn with the
root transform left out") and keeps them in their outcome. Whole
installation: 16,842 placements (0.55 % of the area). Test views: lobby 13, vault 0, test
cell 2, radius 1 9, radius 2 24; their outcomes are unchanged.

**Frames** against the commit before: without `--under-hypotheses` every
test view repeats byte for byte except the lobby, which differs in 36
pixels of one instance, the run-to-run difference fixed next ([render.md](render.md), Viewer frames that repeat), not
this change. Under hypotheses: lobby 11,097 pixels differ (wall panels
and a door frame turned), test cell 663, dog view 15,256, radius 1 1,326,
`--walk --terrain --around 2` 288, dog view without actors 24,174; vault
and the `--path` walk byte-identical (the walked positions unchanged).

## Enable parents

Under the enable parent rules ([enable-parent.md](enable-parent.md)) a
placement with an XESP is no longer refused for it alone: its chain decides
whether it starts enabled, and one that starts disabled is counted
**hidden by `xesp_child_follows_parent_v1`**. The census now also counts
the placements with an XESP by the start state their chain gives and their
outcome. Every cell of the load order (62,393 cells, 3 shards of about
1,500 s, 1.1 GB peak each):

| Outcome | Before | After |
| --- | ---: | ---: |
| drawn without hypotheses | 781,075 | 781,075 |
| drawn only under hypotheses | 527,821 | 538,375 |
| hidden by `xesp_child_follows_parent_v1` | — | 6,857 |
| omitted | 446,615 | 429,204 |

10,665 references go from not drawn to drawn under hypotheses; 7,636
references whose chain starts disabled stay out, now as hidden rather than
omitted; 135 actors whose chain starts disabled go out (111 were drawn, 24
were refused by the draw build); 16,796 references and 299 actors with an
XESP stay omitted for other reasons; the chains of 470 references and 2
actors are not decided (a cycle, or a record of the chain with its own flag
0x800 beside its XESP). The sole family
`record: RuntimeEnableParent XESP` (17,968 placements) is gone; what is
left of it is `XESP.own_flag` and the other undecided texts.

Test views: lobby and vault unchanged (no XESP); test cell +1 drawn under
hypotheses, radius 1 +9, radius 2 +12. Frames without hypotheses are
byte-identical (below, Frames of enable parents and controllers at rest).

## Controllers at rest

Under the five rest rules of [controller-rest.md](controller-rest.md) the
animation-controller blocks of a model are no longer refused as one
family: each block carries a rest tag, five tags have a pending rule, and
the census splits the family by the tags left undecided
(`model: animation-controller blocks (TAG)`). Every cell of the load order
(3 shards of about 1,550 s, 1.06 GB peak), against the census with enable
parents alone:

| Outcome | Enable parents alone | And controllers at rest |
| --- | ---: | ---: |
| drawn only under hypotheses | 538,375 | 540,393 |
| hidden by `xesp_child_follows_parent_v1` | 6,857 | 6,902 |
| omitted | 429,204 | 427,141 |

2,018 references and 6 actors move from omitted to drawn under
hypotheses (omitted area from 42.39 % to 42.37 % of all); 45 placements
whose chain starts disabled, omitted before for their controllers, are now
counted as hidden by the chain rule. By rule, drawn placements rely on
`nif_controller_rest_transform_constant_v1` 1,409 times,
`nif_controller_rest_manager_idle_v1` and `..._idle_transform_v1` 568,
`..._unrendered_v1` 41 (`..._idle_shader_v1` never as the only open
reason). Most of the 22,607 placements whose models the rules decide stay
omitted for other reasons (doors' FNAM and VMAD, missing materials).

The area does not move: the largest share of the old family belongs to
large effects (mist volumes, light beams) whose controllers are left
undecided on purpose. As sole families the controller blocks now rank
`texture-transform` first (5,500 placements, 2.99 % of the omitted area),
then `transform-varies` (1,868), `effect-emissive` (950), `manager-loop`
(675), `alpha` (650), `behavior-graph` (341); `falloff` with
`texture-transform` holds 61.39 % of the omitted area in 407 placements,
and `effect-idle` with `manager-loop` 6.41 % in 13,391.

A first version of the rules, which counted every manager-run controller
and every sequence as idle, moved more (see controller-rest.md) but drew
the lobby and vault test views under a white veil of effects; it was
narrowed before this census.

Test views (drawn only under hypotheses, enable parents alone and with
controllers at rest): lobby 139 → 141 (two sliding doors), vault 146 →
146, test cell 123 → 123, radius 1 665 → 666, radius 2 1,784 → 1,789
(exterior light fixtures under `..._unrendered_v1`, debris under
`..._transform_constant_v1`).

### Frames of enable parents and controllers at rest

Viewer, headless, 1280 x 720, the eight test views, against the build of
the commit before this step (`b631d74`). Without `--under-hypotheses` the
six views that run without it (lobby, vault, test cell, dog view, radius 1,
dog view without actors) give byte-identical colour frames and tag files;
the two walk views refuse to run without the hypotheses
(`WalkRuleNotAccepted`), as before. Under hypotheses: lobby, vault, dog
view, dog view without actors and the `--path` walk are byte-identical;
the test cell and radius 1 differ in 45 pixels (a small piece of debris
on the ground whose enable parent chain says enabled), the
`--walk --terrain --around 2` view in 40 pixels (one newly drawn
placement); in each, every differing pixel carries the new instance's tag
and every other pixel keeps its tag. The first, wider version
of the rest rules changed 921,588 pixels of the lobby frame and 335,677 of
the vault frame (white effects in front of the camera); none of that
remains.

## Effect surfaces

Under `nif_controller_rest_effect_first_key_v1` and the effect surface
rules of [effect-surfaces.md](effect-surfaces.md), a model whose
effect-shader controllers all start at their stored fields is no longer
refused for them; its effect surfaces are then drawn, or refused by the
draw build with `UnsupportedEffectSurface` naming every term the frame does
not settle. Every cell of the load order (3 shards of about 1,900 s, 0.98
GB peak each, run beside other builds), against the census of controllers
at rest:

| Outcome | Controllers at rest | And effect surfaces |
| --- | ---: | ---: |
| drawn without hypotheses | 781,075 | 781,075 |
| drawn only under hypotheses | 540,393 | 540,395 |
| hidden by `xesp_child_follows_parent_v1` | 6,902 | 8,197 |
| omitted | 427,141 | 425,844 |

Two placements move from omitted to drawn under hypotheses (a skylight
effect and a static vault gear door); 1,295 placements whose models
are now settled and whose enable parent chain starts disabled are counted
as hidden by the chain rule instead of omitted (area 0.41 % of all, mostly
large mist volumes). Omitted area goes from 42.37 % to 41.96 % of all; the
drawn area does not change measurably. The effect refusals now rank, as
sole families by area: `falloff+vertex-alpha-one-flag` 10,989 placements
(4.83 % of the omitted area; the large mist volumes),
`colour-range+vertex-alpha-one-flag` 165 (1.37 %),
`falloff+colour-range+vertex-alpha-one-flag` 728 (0.36 %),
`feature+vertex-alpha-one-flag` 86 (0.32 %),
`palette+falloff+vertex-alpha-one-flag` 943 (0.03 %); a material whose
driven field differs from the NIF's (`UnsettledEffectMaterialField`) 104.
The largest sole families left by area: `model: animation-controller blocks
(falloff)` 407 placements, 61.99 % of the omitted area (distant clouds whose
falloff stop angle is stored outside its keys); missing materials 46,166
(11.69 %); the mist volumes above (4.83 %); swap scopes 3,316 (1.78 %);
`texture-transform` 4,962 (1.77 %); `RenderSwapWithoutMaterial` 7,181 (1.71
%); missing textures 31,760 (1.60 %); opacity above 1 with blending (1.56 %
at 2.0 alone).

Test views: drawn under hypotheses unchanged (lobby 141, vault 146, test
cell 123, radius 1 666, radius 2 1,789). The test cell's large mist volume
(62.80 % of its placement area) and four more in radius 1 and 2 are now
hidden by the chain rule instead of omitted (omitted area: test cell 63.77 %
→ 0.96 %, radius 1 52.81 % → 8.75 %, radius 2 25.09 % → 6.54 %); in the
lobby and the vault the mist volumes are refused as
`UnsupportedEffectSurface falloff+vertex-alpha-one-flag` (89.96 % and 37.05
% of their omitted area).

**Frames** (viewer, headless, 1280 x 720, the eight test views against the
build of the commit before, `522df72`): every view is byte-identical, colour
frames and tag files, with and without `--under-hypotheses` (the two walk
views run only with it). The two newly drawn placements, rendered apart:
the skylight shows, inside the roof opening, an opaque pale card (an
effect shape without an alpha property, which writes depth) under an
additive textured layer, where the frame before showed the clear colour and
objects placed beyond the roof (72,990 pixels differ); whether the game
shows those objects through the card is not known. The vault gear door
model is drawn whole (43,365 pixels); its effect shape has an emissive
multiple of 0 at its first key. Neither covers the room. The CPU and GPU frames of the skylight view agree as before
(coverage agreement 1.000000 against 0.999997; mean channel difference 2.8
to 3.0 steps against 3.0 to 3.2).

## Materials, swap scopes and texture transforms

The census now gives the sub-reasons of missing materials, swap scopes and
texture-transform controllers ([materials.md](materials.md); sections in
[materials-census.txt](materials-census.txt)). Missing material files the
draw build never reads are set aside (a lookup of this project, no rule),
and five pending rules cover what the data settles: swap scopes whose
readings agree, a material whose missing parent it never reads, a lighting
shader's own texture set for a missing material (used by no placement
here), lighting-shader UV controllers stored at their first key, and the
order of UV scale and offset. Every cell of the load order (3 shards of
about 2,000 s beside other builds, 1.09 GB peak), against the census of
effect surfaces:

| Outcome | Effect surfaces | And materials |
| --- | ---: | ---: |
| drawn without hypotheses | 781,075 (24.07 % of the area) | 779,237 (22.35 %) |
| drawn only under hypotheses | 540,395 (32.21 %) | 589,593 (38.73 %) |
| hidden by `xesp_child_follows_parent_v1` | 8,197 | 8,432 |
| omitted | 425,844 (41.96 %) | 378,249 (37.14 %) |

47,360 placements move from omitted to drawn under hypotheses and 235 to
hidden by the chain rule (omitted area from 41.96 % to 37.14 % of all);
1,838 placements drawn without hypotheses, whose surfaces have a UV offset
other than 0 or a UV scale other than 1, are now drawn only under
`uv_scale_then_offset_v1` (1.72 % of the area leaves the frames drawn
without `--under-hypotheses`). By rule, drawn placements rely on
`uv_scale_then_offset_v1` 22,290 times, `nif_controller_rest_texture_first_key_v1`
4,417, `mswp_scopes_readings_agree_v1` 1,153 and
`bgsm_missing_parent_own_fields_v1` 430 (counted once per placement).

`render asset: MissingMaterial` as a sole family goes from 46,166
placements (11.69 % of the omitted area) to 469 (0.04 %): swap targets
whose source a model cites and two effect-shader names. Texture-transform
controllers alone go from 4,962 to 246 (0.27 %; effect-shader UV
controllers stored away from their first key). Swap scopes whose readings
differ rise from 2,284 to 3,678 placements (2.98 % of the omitted area):
placements that missing swap files kept out before now reach the draw
build and meet the same refusal; `RenderSwapWithoutMaterial` likewise rises
from 6,846 to 8,792. The largest sole families left by area: the distant
clouds (`model: animation-controller blocks (falloff)`, 407 placements,
70.03 % of the omitted area), the mist volumes
(`UnsupportedEffectSurface falloff+vertex-alpha-one-flag`, 5.46 %), swap
scopes whose readings differ (2.98 %), `RenderSwapWithoutMaterial` (2.32 %),
opacity 2.0 with blending (2.03 %), missing textures (32,015 placements,
1.83 %), `colour-range+vertex-alpha-one-flag` (1.55 %).

Test views (drawn under hypotheses, before → after): lobby 141 → 141,
vault 146 → 146, test cell 123 → 123, radius 1 666 → 668 (omitted area
8.75 % → 8.68 %), radius 2 1,789 → 1,797 (6.54 % → 6.46 %; one placement
moves from drawn to drawn under the UV rule).

**Frames** (viewer, headless, 1280 x 720, the eight test views against the
build of `d7bab2e`): without `--under-hypotheses` every view is
byte-identical, colour frames and tag files (no placement that leaves the
frames drawn without hypotheses is in them). Under hypotheses: the dog view
and the dog view without actors differ in 3,943 pixels (a barrier beside the
road, kept out before by missing swap files no shape reads, now drawn), the
radius 1 view in 20 pixels (a distant small object of the same kind);
lobby, vault, test cell and both walk views are byte-identical; every
differing pixel carries the new instance's tag and every other pixel keeps
its tag. A further view of the radius-2 grid shows two puddle-like surfaces
newly drawn under the texture first-key rule (19,111 pixels). The CPU and
GPU frames of the dog view agree as before (coverage agreement 0.999857 →
0.999859, mean channel difference 0.19 steps both).

## Falloff and vertex alpha

Effect surfaces with falloff, and with vertex alpha under Vertex_Colors
alone, are drawn under six pending rules, and falloff angle controllers are
settled with their keys read as degrees
([effect-surfaces.md](effect-surfaces.md), Falloff). Every cell of the load
order (3 shards of about 1,700 s beside other builds, 1.06 GB peak),
against the census of the material step:

| Outcome | Materials | And falloff |
| --- | ---: | ---: |
| drawn without hypotheses | 779,237 (22.35 % of the area) | 779,237 (22.35 %) |
| drawn only under hypotheses | 589,593 (38.73 %) | 600,643 (40.75 %) |
| hidden by `xesp_child_follows_parent_v1` | 8,432 | 8,432 |
| omitted | 378,249 (37.14 %) | 367,199 (35.12 %) |

11,050 placements move from omitted to drawn under hypotheses (omitted area
from 37.14 % to 35.12 % of all); nothing drawn without hypotheses changes.
Drawn placements rely on the four falloff rules 10,845 times and on
`effect_vertex_alpha_under_vertex_colors_v1` 11,045 times (among them 205
placements without falloff whose only unsettled term was the vertex alpha
under one flag; 5 falloff placements need no vertex alpha rule);
`nif_controller_falloff_angle_degrees_v1` settles the distant clouds at
rest, but no placement that relies on it is drawn.

The sole families before → after: `UnsupportedEffectSurface
falloff+vertex-alpha-one-flag` (10,989 placements, 5.46 % of the omitted
area), `palette+falloff+vertex-alpha-one-flag` (943) and the plain
`vertex-alpha-one-flag` (205) leave the omitted list or become `palette`
(943 placements, 0.03 %); `falloff+colour-range+vertex-alpha-one-flag`
(728) and `colour-range+vertex-alpha-one-flag` (166) become `colour-range`
(1,308 placements, 2.09 %); `feature+falloff` and
`feature+vertex-alpha-one-flag` become `feature` (452, 0.39 %). The
distant clouds (`model: animation-controller blocks (falloff)`, 407
placements, 70.03 % of the omitted area before) now reach the draw build
and are refused for `palette+colour-range` (458 placements with 51 others,
74.06 % of the omitted area after, 26.0 % of all area): the greyscale-to-
palette alpha flag and a base colour scale of 1.5 on white. No placement of
this installation is refused for `rgb-falloff`, `falloff-fields`,
`falloff-normals` or `vertex-alpha-flag-alone` as its first draw refusal.

Test views (drawn under hypotheses, before → after): lobby 141 → 146
(omitted area 37.20 % → 3.73 %), vault 146 → 214 (81.62 % → 51.38 %), test
cell 123 → 123, radius 1 668 → 669 (8.68 % → 8.59 %), radius 2 1,797 →
1,798 (6.46 % → 6.42 %).

**Frames** (the `render` example, CPU, 1280 x 720, against the build of the
commit before): without `--under-hypotheses` the lobby and vault frames
are byte-identical. Under hypotheses the lobby differs in 919,872 pixels
(the camera stands among five mist volumes: a soft grey veil over the
room, hard-edged where the volumes cut walls and floor, since the soft
depth fade is left out) and the vault in 290,358 (light beams and mist
over the upper half; additive beams saturate to white near the ceiling
lights). Every differing pixel of the lobby, and all but 9 of the vault,
carries the tag of a newly drawn placement; the 9 are behind a blended
surface whose background placement the CPU reference now leaves out at
its decoded-texture limit (`LimitExceeded`; one placement in the vault
view, a limit of the reference renderer, not of the viewer's policy). The
GPU viewer leaves the falloff draws out ([render.md](render.md),
Falloff); its frames were not rendered for this step.
