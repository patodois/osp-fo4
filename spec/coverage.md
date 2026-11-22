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

Every cell of the load order, each alone (62,391 cells read; 3 not read:
one deleted cell, two over the manifest's memory limit; 3,016 s, 0.97 GB
peak): full table in [coverage-census.txt](coverage-census.txt).

| Outcome | Placements | Share | Area share |
| --- | ---: | ---: | ---: |
| drawn without hypotheses | 770,580 | 44.7 % | 30.1 % |
| drawn only under hypotheses | 191,276 | 11.1 % | 17.4 % |
| hidden by `editor_marker_hidden_v1` | 16,781 | 1.0 % | 0.4 % |
| outside the visual slice | 5,648 | 0.3 % | 0.0 % |
| omitted | 739,674 | 42.9 % | 52.0 % |

Of 1,723,959 placements (8,990 of them placed actors). The largest sole
families of the omitted (the family alone keeps the placement out):

| Sole family | Placements | Area share of all omitted |
| --- | ---: | ---: |
| `swap: RuntimeMaterialSwapFilter` (an MSWP with FNAM) | 181,983 | 30.3 % |
| `record: STAT.PRPS` | 58,195 | 4.4 % |
| `draw: UnsupportedRenderAlpha` | 34,815 | 14.2 % |
| `record: REFR.XRFG` | 33,371 | 0.9 % |
| `record: MISC.CVPA` | 27,953 | 0.0 % |
| `model: animation-controller blocks` | 23,701 | 25.9 % |
| `render asset: MissingTexture` | 18,875 | 0.8 % |
| `record: BaseTypeOutsideSlice TXST` (placed texture sets) | 16,998 | 0.0 % |
| `record: RuntimeEnableParent XESP` | 14,519 | 1.9 % |
| `draw: NoDrawableGeometry` | 6,677 | 0.0 % |

294,061 omitted placements have two or more families. Counted once per
placement, the swap filter reaches 268,312 omitted placements (46.3 % of
their area) and the animation controllers 71,926 (30.6 %).

The test views of the earlier steps (the viewer's own counts; the
exterior test cell alone and with radius 1 and 2, an interior lobby
without actors, an interior with clothed humanoids):

| View | Placements | Drawn | Under hypotheses only | Omitted (with hidden and outside) | Omitted area | Largest sole family |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| lobby | 352 | 158 | 135 | 59 | 37.6 % | animation controllers (84.5 % of omitted area) |
| vault interior | 988 | 111 | 66 | 811 | 94.7 % | swap filter (72), animation controllers (48) |
| test cell | 391 | 238 | 64 | 89 | 75.5 % | `STAT.PRPS` (48) |
| test cell, radius 1 | 2,541 | 1,500 | 294 | 747 | 62.2 % | `STAT.PRPS` (299) |
| test cell, radius 2 | 6,450 | 3,751 | 852 | 1,809 | 41.8 % | `STAT.PRPS` (642), opacity above 1 (169, 24.6 % of area) |

In the radius-2 view 38 placements are refused by the draw build's
stored-texture budget (`draw: LimitExceeded`, a viewer limit, not data).
