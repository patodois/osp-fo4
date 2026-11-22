# Contributor and agent guide

Read [README.md](README.md) first for what the project is and how to run it.

## Map

| Crate | Purpose | Start here |
| --- | --- | --- |
| `fo4-core` | Checked I/O (`ReadAt`, `Cursor`), allocation `Budget` (children are scopes released on drop or kept with `commit`; an optional scope limit below the root's ceiling), `Error`, `Manifest`. | `src/lib.rs` |
| `fo4-archive` | BA2 and BSA indexes and bounded extraction. | `src/lib.rs`, `src/bsa.rs` |
| `fo4-vfs` | Explicit mounts, winner and loser per path, content fingerprint. | `src/lib.rs` |
| `fo4-plugin` | Records, load order, overrides, strings, typed views, subrecord layouts; the actor chain's records (`actor`). | `src/lib.rs`, `src/typed.rs`, `src/layout.rs`, `src/actor.rs` |
| `fo4-nif`, `fo4-material`, `fo4-texture` | Model, material and texture readers; skinned shapes (`BSSkin::*` blocks, vertex weights) read on request. | `src/lib.rs` of each, `fo4-nif/src/skin.rs` |
| `fo4-havok` | Havok collision payload and `.hkx` files: container (sections, class names, data tables, two header variants), object spans, bodies, simple shape views and compressed meshes (trees, triangles, vertex records); animation objects (skeleton, binding, animation header, spline block table) and the spline-compressed track data (quantised rotations, curves, sampling at a time). | `src/lib.rs`, `src/objects.rs`, `src/bodies.rs`, `src/shapes.rs`, `src/mesh.rs`, `src/mesh/geometry.rs`, `src/mesh/tree.rs`, `src/anim.rs`, `src/anim/spline.rs` |
| `fo4-scene` | Cell loading, asset graph, rule levels and the rule registry; collision readiness apart from render readiness; collision of a cell placed in world space; exterior cells around a cell by grid, joined into one scene; every exterior cell of a world by grid (`world_grid`) for the viewer's moving grid; placed actors resolved to a skeleton and addon models, rest pose and skinning (`actor`). | `src/lib.rs`, `src/hypotheses.rs`, `src/readiness.rs`, `src/collision.rs`, `src/collision/model.rs`, `src/grid.rs`, `src/actor.rs`, `src/actor/pose.rs` |
| Terrain (LAND) | `fo4-plugin` LAND view, `fo4-scene` terrain in world space and its ground texture weights under their rules, `fo4-render` terrain draw (texture passes with overlay draws, material-only texture sets through their material, the default texture its worldspace names, plain grey for what is refused), `fo4-unpack land` report and census. | `fo4-plugin/src/land.rs`, `fo4-scene/src/terrain.rs`, `fo4-scene/src/terrain/layers.rs`, `fo4-render/src/terrain.rs`, `fo4-unpack/src/land.rs` |
| `fo4-walk` | Walking controller over a cell's placed collision: world of convex primitives with a grid broad phase, `ray_down`, vertical capsule with collide-and-slide, step-up and slope limit; terrain of the cells around a cell under the walker; a `WorldSet` of worlds queried as one (`Space`), one piece added or removed without rebuilding the rest; every movement number a provisional rule. | `src/lib.rs`, `src/controller.rs`, `src/world.rs`, `src/geometry.rs`, `src/terrain.rs` |
| `fo4-render` | Draw list, CPU rasterizer, PNG, image comparison; optionally stored texture blocks for a GPU consumer (`DrawList::blocks`), which the CPU refuses; placed actors skinned into the draw list after the references (`build_with_actors`). | `src/draw.rs`, `src/raster.rs` |
| `fo4-unpack` | Command-line reports (including `cell-collision` and `walk`) and the `render` example. | `src/lib.rs`, `src/cell_collision.rs`, `src/walk.rs`, `examples/render.rs` |
| `apps/fo4-view` | GPU viewer (Bevy), own workspace; checked against the CPU frame; free-fly and walk modes; `--terrain` draws the terrain with its ground textures; `--around N` adds the cells around an exterior cell; block-compressed textures uploaded as stored blocks (`--texture-cap`, `--alpha-tested-cap`; `--reference-textures` for the CPU's decoded textures); `--scene-memory` bounds what the scene keeps, the manifest's `max_memory` each read (`Budget::with_scope_limit`); with `--walk --terrain --around N` the window's grid follows the player (viewer policy: load within N, drop past N + 1; a loading thread owns the game data and sends plain data; shared meshes and textures counted by content; per-cell credit accounts), and `--headless --path` walks waypoints with the same grid moves done synchronously; the moving grid's cells share materials by surface and its instance data is built on the CPU; `--script-path`, `--script-shots` and `--frame-log` script, capture and time a window walk for measurements; placed actors drawn by default in their rest pose, `--no-actors` to leave them out. | `src/scene.rs`, `src/coords.rs`, `src/surface.wesl`, `src/fly.rs`, `src/walk.rs`, `src/stream.rs`, `src/cells.rs`, `src/live.rs`, `src/path.rs`, `src/script.rs` |

Format layouts, their sources and the rule registry are indexed in
[spec/README.md](spec/README.md).

## Rules of evidence

- A **format fact** needs a public source: cite the URL and its exact scope in
  the relevant spec file and in [docs/provenance.md](docs/provenance.md).
- Any **behaviour** not settled by a source is a named rule in
  [spec/hypotheses.tsv](spec/hypotheses.tsv). Register it as `pending`, with the
  measurement that could refute it, before code relies on it. Only a recorded
  measurement of the running game changes its status. The code reads the
  registry: see `fo4_scene::RuleLevel` and `fo4_scene::hypotheses`.
- Never turn an unsupported assumption into a default. Preserve unknown bytes,
  or return a named error with file, offset, expected and found.
- Keep a measured convention separate from claims about exact floating-point
  arithmetic.

## Sources that are off limits

Do not open, translate or paraphrase copyleft or mixed-licence implementations
of these formats (for example nifly, NifSkope, PyNifly, BodySlide, OpenMW,
xEdit source or scripts, Mutagen), and never inspect game executables or
decompiled code. Public format documentation and schemas such as nifxml are
allowed. Citing xEdit's published record and field names as a naming source,
as `spec/plugin.md` does, is allowed; its source stays closed.

MIT-licensed projects may be used as a reference with credit. Today that means
fo76utils and Material Editor. Before opening one, confirm that the file you
read is itself under MIT (bundled components can carry other terms), then
record the project, the files, the commit and its MIT notice in
[THIRD_PARTY.md](THIRD_PARTY.md) in the same change. What the reference says
is still a hypothesis until a measurement on game data confirms it.

## What must never be committed

Game assets, images containing game textures, photographs of the game,
executable dumps, measurement datasets, local machine paths and secrets. Put
local artifacts under `build/` or `reports/`, which Git ignores. A test
(`fo4-unpack/tests/repository_guard.rs`) rejects game file types and machine
paths in versioned files.

## Code conventions

- `#![forbid(unsafe_code)]` everywhere.
- Checked arithmetic (`fo4_core::add`, `mul`, `range`, `limit`), `try_reserve`
  through a `Budget`, and bounded counts, depths and decompressed sizes for
  anything read from a file.
- Load order and archive priority come only from the manifest.
- Tests use synthetic bytes and must pass without the game. A test that needs
  real data is `#[ignore]` with the reason in the attribute.
- Game installations are read-only: tools write only inside their output
  directory.

## Checks before a change is done

```sh
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

The GPU viewer is outside that workspace: run `cargo test`, `cargo fmt --check`
and `cargo clippy --all-targets -- -D warnings` inside `apps/fo4-view` when it
or a crate it uses changes.

If the change could alter output, render a cell before and after and compare
the files byte for byte; say so in the commit message when a difference is
intended.
