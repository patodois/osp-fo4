<h1 align="center"><img src="docs/title.png" alt="osp-fo4" width="243"></h1>

An open-source, clean-room reimplementation of Fallout 4's engine (Bethesda's
Creation Engine) in Rust, for people who own the game: readers for its data
files, a CPU reference renderer and a GPU viewer, a walking controller, and a
script interpreter that runs the opening quests without the original
executable. Nothing here contains game files or decompiled code: every
behaviour is either a documented format fact or a named pending rule waiting
for a measurement of the running game.

It is not a playable game yet. The data can be read, cells drawn and walked,
and the quests of the first hour run headless under stated assumptions; fights,
menus, sound, saving and character movement are not started. The milestones
and their state are in [docs/next-steps.md](docs/next-steps.md#milestones).

Looking for something else? The same formats serve Skyrim Special Edition only
in part (BSA 105 archives); the cross-game work lives in
[rewrite-bridge](https://github.com/patodois/rewrite-bridge), and the plugin
that measures the running game in
[osp-f4se-measure](https://github.com/patodois/osp-f4se-measure).

To continue the work, start at [docs/next-steps.md](docs/next-steps.md): it lists where each part stops and what is needed next.

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
  ([terrain](spec/land.md)). The default ground texture, which no record
  names, is drawn with the one texture set whose editor ID names it as its
  worldspace's default (a pending rule; grey elsewhere), texture sets that
  hold only a material are drawn with the material's diffuse texture, and
  the texture repeat is a placeholder.
- **Scripts**: every compiled script (`.pex`) in the archives read to the
  last byte into typed structures, with opcode arities established by the
  census and a text disassembly (`pex_dump`); call, jump, property and
  struct-member slots identified by name resolution, other opcode meanings
  pending ([scripts](spec/pex.md)). A small interpreter (`fo4-vm`) runs
  them under those pending rules: native functions are only recorded in an
  effect log, a native result that would steer control flow stops the run
  as blocked, and `script_run` binds a placed reference's scripts from VMAD
  and delivers one event; opcode meanings are supported by data-flow, idiom
  and execution checks, not measured ([interpreter](spec/vm.md)). Quest
  records are read to the last byte ([quests](spec/quest.md)), and
  `quest_run` sets every quest stage that has a fragment, or starts the
  quests flagged to start with a new game, and runs the stage fragments
  under pending rules. Conditions (CTDA) of every record are read field
  by field ([conditions](spec/conditions.md)), and a three-valued
  evaluator settles the few whose result the quest state and the data
  decide, to choose which log entry's fragment a stage runs. The story
  manager's nodes are read to the last byte ([story
  manager](spec/story-manager.md)); a run delivers the remote and custom
  events scripts send each other where it raises the event itself, and the
  script event a keyword sends starts the quest its node selects.
- **Saves**: save files (`.fos`) read, never written, to the last byte:
  header, plugin lists, the location table, the three global data tables
  and the change-form table, with every form reference resolved against the
  plugin data; payloads beyond a moved reference's place stay raw
  ([saves](spec/save.md)).
- **Sound**: every sound file in the archives read to the last byte: wave
  files, the WMA-family files and the voice container (its lip half kept
  raw), with their header fields and chunks; integer PCM (1.4% of the files,
  30% of the bytes) decoded, the WMA-family payload framed into packets but
  not decoded; the records that tie sounds to cells, regions, doors,
  activators and weapons, and how their stored paths resolve
  ([sound](spec/audio.md)). Nothing plays a sound yet.
- **Actors**: placed actors (ACHR) followed to their skeleton and the
  armour addon models they wear, with record layouts measured on the game's
  plugins, and drawn by the GPU viewer standing in the skeleton's rest pose,
  skinned once on the CPU under pending rules, with the head parts the
  data names drawn unmorphed (no face morphs or tints); an actor whose
  look the data does not settle (leveled lists, unresolved outfits) is
  left out with its reasons, and a head that is not settled leaves only
  the head out ([actors](spec/actor.md)). With
  `--actor-clip` a named Havok animation clip (spline-compressed, decoded
  from measured layouts: [Havok animation](spec/havok-animation.md)) poses
  the actors whose skeleton it fits, looping in the window.
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

The viewer uploads block-compressed textures (BC1 to BC5, BC7) as the
blocks the DDS stores, without decoding them, from the first stored level
whose sides are at most 1024 texels (2048 for a texture drawn with an alpha
test, such as foliage); `--texture-cap N` and `--alpha-tested-cap N` change
the limits. Other textures are decoded as the CPU render decodes them. With
`--around 1` at the test cell this keeps about 250 MiB of textures, draws
every object the CPU texture budget would drop, and stays near 1.3 GB peak
resident memory. `--reference-textures` decodes every texture as the CPU
render does (first level at most 256 texels), for comparing frames with it;
the CPU render never draws stored blocks.

The manifest's `max_memory` bounds each cell read and each asset decode;
what the viewer's scene keeps in all (records, draw list, stored blocks,
terrain, collision and walking world) is bounded by `--scene-memory MIB`,
2048 by default. With `--walk --terrain` at the test cell the scene keeps
about 0.5, 0.9 and 1.4 GiB of allocation credits at `--around` 1, 2 and 3,
loads in 9 to 12 s (debug build) and peaks near 1.3, 1.6 and 1.8 GB
resident memory headless; the credits are a conservative count, not
resident memory ([scene](spec/scene.md#memory-of-a-joined-scene)).

In the window, `--walk --terrain --around N` on an exterior cell moves the
loaded grid with the player: a second thread loads the cells within N grid
steps of the cell under the walker's feet (the eye when flying), nearest
first, and the cells more than N + 1 steps away are dropped, with their
GPU data, collision and credits; meshes and textures that several cells
draw are uploaded once and freed when no loaded cell uses them. Which cells
are loaded is a viewer policy, not a claim about the game
([scene](spec/scene.md#the-moving-grid)). A walker over a cell that has not
arrived waits in place. `--no-stream` loads the grid once, as before.
`--headless ... --path X,Y;X,Y` walks the settled walker through the
waypoints at the run speed with the same grid moves done synchronously,
prints each grid change (cells dropped and loaded, time per cell, credits
kept) and draws the frame where the walk ends (`--start-pose`: from the
start pose). In the window, `--script-path` walks the same way without
the keys, `--script-shots` captures the start pose before and after, and
`--frame-log` writes each frame's time ([walk](spec/walk.md)). At the test cell, walking
four cells east and back twice kept 357 to 401 MiB of credits at radius 1
and 584 to 674 MiB at radius 2, the same at every repeated grid change,
with 1.54 and 1.79 GB peak resident memory; a crossing took 2 to 3.3 s
(radius 1) and 3 to 4.5 s (radius 2) of loading. Headless frames without
`--path` are unchanged.

Placed actors are drawn by default when the level accepts their pending
rules (`--under-hypotheses`): each one stands at its placement in its
skeleton's rest pose with the skin and outfit models the data settles,
skinned once at load, and the viewer prints one line per actor saying what
was drawn or why not (for example `head: covered by worn armour`). `--no-actors` leaves
them out; the frames and tags are then byte-identical to those before
actors (checked on the headless cases above and at `--around` 1 and 2). At
the test cell with `--walk --terrain --around 1`, 1 of its 11 placed actors
is drawn, loading takes 5.99 s instead of 5.93 s, with 7.1 MiB more
credits and 14 MB more peak resident memory
([actors](spec/actor.md#results)). The CPU render does not draw actors, so
compare frames with `--no-actors`.

`--actor-clip ARCHIVE_PATH.hkx` poses every drawn actor whose skeleton's
`.hkx` (beside its skeleton NIF) the clip's binding fits, under two more
pending rules: at `--actor-clip-time SECONDS` (default 0) in a headless
frame, and looping from there in the window, CPU-skinned again every frame
on the main thread (0.05 ms a frame for the test cell's dog). The clip is
the one named, not the one the game would play; it is refused with the
moving grid (`--no-stream` loads the grid once) and with `--path`. Without
it the frames are byte-identical to those before
([Clip pose](spec/actor.md#clip-pose)).

`--time SECONDS` (and the CPU render's `--time SECONDS`) starts the game
clock at that second after load: free-running controllers of shader UV
fields and alpha (scrolling water debris, mist, glows) are drawn at its
time under pending clock rules, and the window's frame clock advances it.
Without it the models that need the clock are left out and every frame is
as before ([the game clock](spec/controller-clock.md)); it is refused with
the moving grid and with `--path`.

#### Play the opening

With `--features script-event` the window plays the opening's quests from
the keyboard and mouse (`--play-keys`, [play](spec/play.md#the-persons-input---play-keys)):
W/A/S/D move, Shift runs (also when it comes only as the movement key's
modifier), hold the left button and move the mouse to turn and look (a drag
the window sees only as cursor positions turns too), the arrow keys turn and
look up or down, E activates what the crosshair at the window's centre meets
(the line under the crosshair says what that is: a short action and the
thing's name, then for a moment what the press did), P pauses, Escape ends
the input (the run finishes and writes its outputs). The viewport shows the
crosshair, what E would activate, the line being spoken (up to three lines),
the objective the run shows (top left) and paused or loading (also while
the run is busy and the play clock stands). The switches the opening needs are kept in
`apps/fo4-view/presets/play-opening.args`; what names the game's records
(the start cell and pose, the opening's new-game switches file, the named
platforms) the repository does not keep, so it goes in a local argument
file of your own, given first, one argument a line:

```sh
cd apps/fo4-view && cargo build --release --features script-event && cd ../..
target/fo4-view/release/fo4-view --args opening.local.args \
  --args apps/fo4-view/presets/play-opening.args --play-record build/play/me.input
```

`opening.local.args` holds the manifest, the data folder and the start cell,
`--eye` and `--look`, `--play-run FILE` and `--play-out DIR`, the
`--play-elevator` and `--play-elevator-also-with` references, and
`--play-activate-opens` for the front door (the person opens it by
activating it; the run's timed open of it is left out) and
`--play-activate-person` for the exit console and the device on the
skeleton before it (the person's E uses them; the run's timed activations
of them are left out). The preset's `--play-pick-first-met` lets E reach the
console from inside the gear door's bounds, and its `--play-action-after-offset`
lets a person late against the run's timed actions keep their spacing: an
action held until a stage happens as long after the stage as the run's
switches wrote it after the action that sets the stage (the exit's two
events 10 and 15 s after your use of the console), not all at once.
`--play-record` writes your commands in `play_run`'s input format:
`play_run` with that file, the preset's play options and the same new-game
switches replays the same run.

The preset turns on pending rules: assumptions that let the opening be
played, **not measured game behaviour**. [docs/unmeasured.md](docs/unmeasured.md)
lists each of its switches with its rule; nothing they produce may be
called faithful before a recorded measurement of the running game. A GIP
pad plays with the keys when the viewer is built with `--features pad` and
started with `--play-pad` (close any other program that holds the pad
first): the left stick walks and runs, the right stick looks, A activates,
Menu pauses, View or B held a second ends. Its key map is our choice, also
listed there.

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
| `fo4-pex` | Compiled script (`.pex`) reader, opcode table and disassembly. |
| `fo4-save` | Read-only save file (`.fos`) reader: container to the last byte, change-form table, form references; census and dump examples. |
| `fo4-audio` | Read-only sound file reader: RIFF `WAVE` and `XWMA` and the voice container to the last byte, PCM decoding, WMA-family packets counted but not decoded; census and dump examples. |
| `fo4-nav` | Navigation meshes (`NAVM` and its `NVNM` geometry, `NAVI`) read to the last byte, consistency census, and this project's own path search over triangles; census and path examples. |
| `fo4-ui` | Read-only interface movie (SWF) reader over the `swf` crate: census, a static reading of a screen's host interface, a headless first frame. |
| `fo4-vm` | Interpreter over `fo4-pex` under pending rules, recording host, VMAD binding, `script_run` and `vm_checks`. |
| `fo4-scene` | Scene assembly, rule levels and the collision of a cell. |
| `fo4-walk` | Walking controller over the collision of a cell. |
| `fo4-play` | A quest run driven by input: the walker's positions and activations feed `quest_run`'s run slice by slice, and behind switches the walking world follows the run's state, load doors are crossed and triggers are read about the walker (`play_run`, `fo4-view --play-run`; [spec/play.md](spec/play.md)). |
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
