# osp-fo4

Rust libraries that read Fallout 4's data files and draw its static scenes on
the CPU, for people who own the game. Nothing here contains game files or
decompiled code: every behaviour is either a documented format fact or a rule
checked against measurements of the running game.

It is a reference, not a playable engine. There is no lighting, animation,
physics or gameplay yet, apart from a first walking controller that moves a
capsule over a cell's collision under provisional, registered rules.

## What works

- **Archives**: BA2 (general and texture) indexes and bounded extraction; BSA
  version 105.
- **Plugins**: records, groups, compression, load order, FormID resolution,
  overrides, localized strings, and typed fields for the record types a static
  scene needs. A small writer produces test plugins.
- **Models, materials, textures**: a NIF subset, BGSM/BGEM, and DDS including
  the block-compressed formats.
- **Scenes**: a cell's placed references with their base records, models,
  materials and textures, each with the reasons it can or cannot be drawn.
- **Terrain**: the LAND heights of an exterior cell placed in world space
  under pending rules, drawn with `render --terrain` with its ground
  textures (base and alpha layers weighted per vertex, vertex colours) under
  further pending rules, and reported by `fo4-unpack land`
  ([terrain](spec/land.md)). The default ground texture no record names is
  drawn grey, and the texture repeat is a placeholder.
- **Rendering**: a deterministic unlit CPU rasterizer and tools that compare
  its frames with photographs of the game.
- **Walking**: a deterministic capsule controller (gravity, ground, walls,
  steps, slope limit) over the placed collision of a cell and, in an
  exterior cell, the terrain of the cell and its neighbours; every movement
  number is a placeholder awaiting measurement ([walk](spec/walk.md)).

Readers are bounded and never panic on malformed input by design: sizes,
counts and depths are checked, unknown bytes are preserved, and every error
names the file and offset.

## How much of the game it covers

Counts from the base game plus its six expansions (39,188 placeable base
records, November 2026):

| Level | Bases | Placements of those bases |
| --- | ---: | ---: |
| Documented facts only | 17 | — |
| Facts plus rules measured in the game | 17,797 | 1,012,007 |
| Also needs a rule that is still unmeasured | 7,544 | 278,643 |
| Blocked (animation, unknown fields, missing data) | 13,830 | — |

In the example interior used below (`Financial13`, 352 placements), 158 draw
on facts plus measured rules, 136 more need an unmeasured rule (mostly the
blend factors of translucent materials), and 56 are blocked.

## Try it

You need Rust 1.88 or newer and your own copy of the game.

```sh
export FO4_DATA="/path/to/Fallout 4/Data"
cargo run --release -p fo4-unpack --example render -- \
  examples/fallout4.json "$FO4_DATA" Financial13 build/lobby \
  --under-hypotheses --eye 0,153.6,104.49 --look 70.7,224.3,104.49
```

This writes `build/lobby/frame.png` and `summary.json` in a couple of seconds on a recent laptop.
The summary lists every placement that was drawn or omitted and why. Drop
`--eye/--look` for an overview from outside the scene. Use a new output
directory for each run; the tool refuses to write into the game directory.

The manifest [examples/fallout4.json](examples/fallout4.json) lists the base
game's plugin and archives explicitly. Load order and archive priority are
never inferred from directory listings; edit the manifest for your
installation ([manifest format](spec/manifest-vfs.md)).

### Rule levels

| Flag | Accepts |
| --- | --- |
| `--strict` | Documented format facts only. |
| `--measured` (default) | Also rules supported by measurement of the running game. |
| `--under-hypotheses` | Also registered rules that are still pending measurement. |

The rules, their status and the measurement behind each are in the
[spec index](spec/README.md#registered-rules).

### GPU viewer

`apps/fo4-view` draws the same draw list with Bevy on the GPU, unlit, and is
checked against the CPU frame. It has its own Cargo workspace so the crates
above build without a GPU stack.

```sh
cd apps/fo4-view && cargo build && cd ../..
# One offscreen frame, no window, at the pose of the example above:
target/fo4-view/debug/fo4-view examples/fallout4.json "$FO4_DATA" Financial13 \
  --under-hypotheses --headless --eye 0,153.6,104.49 --look 70.7,224.3,104.49 \
  --out build/view/gpu.png --tags build/view/gpu-tags.bin
# Compare with the CPU frame (render example with --instance-ids):
target/fo4-view/debug/fo4-view compare --cpu build/lobby/frame.png \
  --gpu build/view/gpu.png --cpu-tags build/lobby/instances.bin \
  --gpu-tags build/view/gpu-tags.bin --diff build/view/diff.png
# Free-fly window: W/A/S/D, Shift faster, hold the left button to look,
# R back to the start pose; the title bar shows mode, speed and position.
target/fo4-view/debug/fo4-view examples/fallout4.json "$FO4_DATA" Financial13 \
  --under-hypotheses --eye 0,153.6,104.49 --look 70.7,224.3,104.49
```

Add `--walk` to walk instead of fly: the camera rides the walking controller
at its eye height, Shift runs and Tab switches between walking and flying.
Add `--terrain` to draw an exterior cell's terrain with its ground
textures, as `render --terrain` does; in walk mode the controller stands on the terrain
with or without the flag. A walker that stays in the air below the lowest
collision for a second is put back where it last stood (a viewer
convenience). Add `--around N` (1 to 3), to the viewer or to the `render`
example, to load the exterior cells within N grid steps as well: their
objects are drawn and walked on and, with `--terrain`, their terrain is
drawn. `--headless --walk` settles the walker from `--eye` and draws one
frame from its eye.

The comparison prints coverage agreement and colour differences; it defines
no pass threshold.

### Command-line tool

`fo4-unpack` reports on an installation without rendering: `inventory`,
`resolve`, `sweep` (archives, records, models, materials, textures, cells,
bases, collision, havok), `check-cell`, `cell-collision` (the collision of
one cell placed in world space under the registered placement rules,
[collision placement](spec/collision-placement.md)), `land` (the terrain of
one exterior cell, [terrain](spec/land.md)) and `walk` (drops the walking
controller at a point of a cell, walks a path over its collision and
terrain and writes a deterministic trace, [walk](spec/walk.md)). Reports are JSON lines with source
offsets ([scene and report contracts](spec/scene.md)); the collision and Havok
censuses also write a TSV of counts ([collision blocks](spec/collision.md),
[Havok container](spec/havok-container.md), [Havok objects](spec/havok-objects.md),
[Havok bodies](spec/havok-bodies.md)).

## How facts are established

1. **Layouts** come from public format documentation and schemas, cited in
   [spec/](spec/README.md) and [docs/provenance.md](docs/provenance.md).
2. **Behaviour** that documentation does not settle (rotation order, default
   scale, which helper geometry is hidden) is registered as a named rule before
   it is used, then tested by reading transforms from the running game and by
   photographing purpose-built test cells.
3. A rule becomes `supported` only for the scope that was measured. Until then
   anything relying on it is reported as such and excluded by default.

No game executable is disassembled and no third-party implementation is
consulted, with one attributed exception described in
[THIRD_PARTY.md](THIRD_PARTY.md).

## Layout

| Crate | Purpose |
| --- | --- |
| `fo4-core` | Checked I/O, allocation budgets, errors, manifests. |
| `fo4-archive` | BA2 and BSA readers. |
| `fo4-vfs` | Explicit mount order and asset lookup. |
| `fo4-plugin` | Plugin records, load order, strings, typed fields. |
| `fo4-nif`, `fo4-material`, `fo4-texture` | Model, material and texture readers. |
| `fo4-havok` | Container and object spans of the Havok collision payload; bodies (shape link and placement); convex, sphere, capsule, scaled and compound shapes. |
| `fo4-scene` | Scene assembly, rule levels and the collision of a cell. |
| `fo4-walk` | Walking controller over the collision of a cell. |
| `fo4-render` | CPU renderer and image comparison. |
| `fo4-unpack` | Command-line tool and the `render` example. |

[AGENTS.md](AGENTS.md) has the working rules for contributors and coding agents.

## Tests

The tests build their own synthetic files and need neither the game nor the
network once dependencies are fetched:

```sh
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Licence

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option, with the
notices in [THIRD_PARTY.md](THIRD_PARTY.md). Fallout 4 is a trademark of its
owner; this project is not affiliated with or endorsed by it.

Most of this code was written with AI coding assistants (GPT and Claude
models) under human direction, and is checked by synthetic corruption tests
and by comparison with measurements of the game.
