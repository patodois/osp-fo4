# Walking over the collision of a cell

Seventh step after [collision-placement.md](collision-placement.md): a first
walking controller that stands on, walks over and is blocked by the collision
that `fo4_scene::collision` places for a cell. Crate `fo4-walk`; report
`fo4-unpack walk`; walk mode of the GPU viewer (`fo4-view --walk`).

The controller is this project's own design, written from general
collide-and-slide geometry. **It does not claim to reproduce the game's
character controller.** It adds no format fact. Every number that describes
how the player moves is game behaviour that no source settles: each is a
registered rule in [hypotheses.tsv](hypotheses.tsv), `pending`, and the code
takes all of them from one struct, `fo4_walk::Params`, whose default
`Params::PROVISIONAL` is a set of round placeholders. No Havok SDK, character
controller source, decompilation or third-party implementation was consulted
(see [provenance](../docs/provenance.md)).

## The world

`World::build(&CellCollision, BodyPlacement, &Budget)` turns every placed
instance into convex primitives in world units, through the typed world views
of `CellCollision` (`world_triangles`, `world_shape`):

| Leaf | Primitive |
| --- | --- |
| compressed mesh | each triangle [a, b, c] (+ [a, c, d]) |
| convex polytope | hull of its vertices widened by the byte-20 radius |
| `hknpConvexShape` | hull of its vertices, radius 0 (no radius is read; counted) |
| sphere, capsule | as stored |
| mesh vertex records | **left out** and counted (`mesh_records_left_out`); their use as collision is open |

Primitives with a non-finite coordinate are left out and counted. The world
relies on every rule its instances rely on plus
`walk_triangles_block_both_sides_v1`; `World::rules` lists them, and the
report refuses to run unless the requested level accepts all of them.

The broad phase is a uniform grid over x and y (256-unit columns by default,
grown so that there are at most 2^20 columns); each column lists the
primitives whose box meets it. Allocations go through the caller's `Budget`.
`World::add_triangles` adds a labelled triangle set and rebuilds the grid;
`add_terrain` adds a cell's terrain (below); `add_sphere`, `add_capsule` and
`add_hull` do the same for single primitives.

A `WorldSet<K>` holds several worlds under ordered keys (the viewer uses
the grid of the cell each was built from) and is queried as one: the
controller walks in anything that implements `Space` (contacts, the source
of an entry, bounds), which `World` and `WorldSet` both do. Entries and
sources are numbered across the set in key order, the first world's from
zero, so the same pieces give the same contacts in the same order whatever
order they were added in, and a set of one world numbers exactly as that
world does. Adding or removing one world rebuilds no other world's broad
phase; it renumbers the set in O(worlds). `WorldSet::ray_down` and
`WorldSet::source` answer across the set. The set adds no rule.

`World::ray_down(x, y, z_from)` returns the highest surface at or below
`z_from` on the vertical line through (x, y): the plane of a triangle the line
crosses (either winding; normal turned up), or the top of a sphere, capsule or
widened hull (found on the line by a convex search). Edge-on triangles are not
hit.
`World::surfaces(x, y)` lists, highest first, the surface each primitive
gives on that line in the same way, with no starting height; `ray_down` is
the first of them at or below `z_from`.

Distances between the character and a primitive are the closest points of two
convex point sets (GJK on the Minkowski difference), minus both radii.

## Terrain

`fo4_walk::terrain::load_around(store, cell, level, neighbours, budget)`
loads the terrain of the cell with `fo4_scene::terrain::load` at the walk's
rule level ([land.md](land.md)): the six terrain rules, all pending, so
terrain exists only at `--under-hypotheses`, with the diagonal level the
registry selects. With `neighbours`, when the cell is exterior, has an XCLC
grid, belongs to a world and the level accepts the terrain rules, it then
finds the cells within one grid step with `fo4_scene::grid::around`, which
reads every other exterior cell of the same world once (through the
budget; [scene.md](scene.md), "Cells around an exterior cell"), and loads
each of the eight neighbours the same way. Each of the nine positions is a
`Piece` with its grid, cell, LAND and outcome: `built:<diagonal>`,
`refused:<code>` (the refusals of `terrain::load`), `no_cell` or
`ambiguous_grid` (two or more cells claim the grid; neither is used, as in
the `land` report). Other cells claiming the cell's own grid are counted
and ignored. `load_grid(.., radius, ..)` does the same within `radius`
steps (at most 3): the first nine pieces as above, then each further ring,
named `ring_2` or `ring_3`. `load_with(store, around, level, budget)` takes
a search already made, so that the viewer and the `render` example search
once for the placed references and the terrain of the same cells. The
`walk` report places only the cell's own collision; the neighbours
contribute terrain only. The viewer with `--around N` places the collision
of every cell within N steps (below).

`TerrainSet::add_to(world)` calls `World::add_terrain` for each built piece:
its 2,048 triangles under one source labelled `terrain` with the piece's
grid, the terrain's rules added to `World::rules`, both sides blocking
under `walk_triangles_block_both_sides_v1` (the terrain itself claims no
solid side). `TerrainSet::height_at(x, y)` is `Terrain::height_at` of the
first built piece containing the point. The terrain adds no movement rule
and no format fact; whether the game's player stands on exactly this
surface is what the terrain rules' live measurements decide.

## The character

A vertical capsule: radius `radius`, total height `height`; its feet are its
lowest point; the eye is `eye_height` above the feet. One fixed tick
(`Settings::tick`, 1/60 s):

1. On the ground, the horizontal velocity is the input direction (heading,
   forward, right; longer input normalised) times the walk or run speed; in
   the air it keeps its value (`walk_no_air_control_v1`) and gravity acts.
2. The displacement is applied in substeps of at most a quarter of the
   radius. After each substep every overlap is pushed out, deepest first, at
   most 12 times: a contact whose normal is within the slope limit
   (walkable) is resolved by moving up along z; a steeper up-facing contact by
   moving horizontally; a downward-facing one along its normal (a ceiling
   stops upward velocity). Sliding along walls follows from this.
3. Step-up: when a move on the ground is shortened by a steep contact, it is
   retried raised by the step height and lowered back onto walkable ground; if
   the tick's own move does not carry the capsule over the edge, the retry
   looks one radius ahead and is accepted only if it ends higher. A tick that
   climbs a step can therefore advance up to one radius further than its own
   displacement.
4. Ground: unless rising, the capsule is lowered by up to the step height (if
   it was on the ground or just stepped) or by 0.05 units (in the air) until a
   walkable contact holds it. This snaps it down stairs and ramps and lands it
   after a fall. The walkable primitive is reported as the ground. A probe
   substep that meets only steep contacts is tried again from the height
   where the capsule first touches (bisection to about 1e-9 units), and that
   contact's normal decides: sinking a whole substep into an edge tilts the
   normal, so the long probe on the ground and the short one in the air used
   to disagree about an edge and the character, perched on it, alternated
   between grounded and falling without moving.

`Character::tick` runs one tick; `Character::step(world, input, dt)` runs as
many whole ticks as `dt` covers (at most 30, the rest is dropped) and keeps the
remainder. Each tick returns a `State`: feet, velocity, grounded, ground
(entry, source, normal), whether it stepped, whether a steep contact kept it
to less than half its horizontal move (blocked), and the last wall touched.
Everything is f64 with a fixed iteration order, so the same world, start and
inputs give the same trace bit for bit.

Consequences of the shape, not separate claims: on a slope of angle θ the
capsule's lowest point stands `radius (1/cos θ − 1)` above the plane at its
axis; it can rest on an edge or straddle a gap narrower than its diameter.
Resting on an edge counts as grounded when the normal at the edge (from the
edge to the centre of the lower sphere) is within the slope limit, that is
when the axis is at most `radius sin(max slope)` (about 11.3 units) beyond
the edge; further out the capsule slides off.

## Parameters

| Field | Provisional value | Rule | Measurement that decides it |
| --- | --- | --- | --- |
| `radius` | 16 | `walk_capsule_radius_v1` | stop distance from a box face of known plane and from a thin pole |
| `height` | 128 | `walk_capsule_height_v1` | lowest slab clearance walked under, standing |
| `eye_height` | 120 | `walk_eye_height_v1` | marker of known height on the screen's centre row, looking level |
| `step_height` | 20 | `walk_step_height_v1` | staircase of boxes with risers 8 to 32 units |
| `max_slope_degrees` | 45 | `walk_max_slope_v1` | ramps of rotated boxes, 30 to 60 degrees |
| `gravity` | 9.81 × 69.99125 ≈ 686.6 units/s² | `walk_gravity_v1` | timed falls from ledges of known height |
| `walk_speed` | 100 units/s | `walk_speed_walk_v1` | timed walk between two markers |
| `run_speed` | 350 units/s | `walk_speed_run_v1` | the same, running |
| `no_air_control` | true | `walk_no_air_control_v1` | landing point with and without reversed input in the air |
| (world) triangles block from both sides | — | `walk_triangles_block_both_sides_v1` | single-plane mesh surfaces of both windings, from each side |

All values are provisional: none was taken from game data. Gravity is Earth
gravity times the pending unit rule `havok_collision_units_69_99125_v1`; the
others are round numbers for a human-sized capsule. Positions in a live
measurement are read with the console (`player.getpos x|y|z`), in at least two
launches, and compared with the trace of the same path; `getpos z` standing on
a surface of known height also tests the feet convention.

The numerical settings (`Settings`: tick, substep fraction, iterations,
ground margin) and the grid column size are integration choices, not claims
about the game.

### Game settings in the data

The plugins hold game settings (GMST records). Their DATA layout is not
established in [plugin.md](plugin.md) (the public format reference was not
readable), and what each setting does in the running game is not known. The
`walk` report therefore lists, as evidence only, the winning GMST records
whose EditorID contains one of a few movement words (`fo4_unpack` walk module,
`SETTING_KEYWORDS`), with their DATA bytes as stored and those bytes read as a
little-endian f32 when there are four. **No parameter is set from them.**
Adopting one would need a rule tying the setting to the behaviour and a
measurement. RACE and NPC_ height data are not read.

## Report

`fo4-unpack walk PROFILE.json EditorID --from X,Y,Z --path DX,DY[,DX,DY...]
[--run] [--out DIR] [--strict|--measured|--under-hypotheses]
[--overlay-plugin FILE]` loads the cell and its collision as `cell-collision`
does, builds the world under the body placement level the registry selects,
adds the terrain of the cell and its neighbours (`load_around` with
neighbours; above), drops the character with its feet at `--from`, lets it settle (no input, until
it is on the ground and has not moved for two ticks, at most 600 ticks), then
walks each leg: it heads for the leg's start plus (DX, DY) at walk (or run)
speed, slowing on the last tick to land on it. A leg ends when it is within
0.01 units of its target, when the distance shrank by less than 0.5 units over
30 ticks (stalled), or after four times its nominal tick count plus 120. At
most 1024 legs and 200,000 ticks.

The default level is `Measured`; while the controller's rules are pending the
command stops with `WalkRuleNotAccepted` naming them and writes no trace.
When the cell is not interior and its own terrain was not built (for
example `refused:NoLand`), the walk still runs and says so: the summary's
`without_cell_terrain` is true, `items.jsonl` has a `terrain_refused` row
and a line goes to standard error. An interior cell's `refused:NotExterior`
is not reported as missing ground.

Outputs, in a new directory:

- `trace.json` (deterministic, schema 2): level, body placement level, every
  rule relied on (the terrain rules among them when terrain was added), the
  parameters with rule, value, origin and registry status, the settings,
  world counts, the terrain pieces, start, path, the surface found under the
  start (`ray_down`), settle ticks, each leg (target, start, end, ticks,
  reached, stalled, remaining distance, grounded at the end, terrain height
  under the end) and every tick (leg, feet, terrain height under the feet,
  grounded, stepped, blocked, ground and last wall as source, instance,
  reference, base, model, leaf class, label and terrain grid: a ground with
  a `terrain` grid is terrain, one with an `instance` is placed collision);
- `summary.json` (`walk`, present only for this command): the same
  parameters and counts, the cell's terrain outcome, `without_cell_terrain`,
  the terrain pieces, the settled position and its height above the surface
  under the start and above the terrain, ticks grounded, stepped and blocked,
  ticks grounded on terrain, on placed instances and on other sets, ticks in
  the air while walking the legs, ticks more than one unit below the terrain
  and the deepest point below it, the number of game settings scanned and
  matching, and timings;
- `items.jsonl`: one `walk_leg` per leg, one `game_setting` per matching GMST
  record (at most 512), `terrain_refused` when it applies, and
  `walk_context`.

### Drops

`fo4-unpack walk PROFILE.json EditorID --drops FILE [--out DIR]
[--strict|--measured|--under-hypotheses] [--overlay-plugin FILE]` builds the
same world (same rules, same refusal below their level) and walks no path.
The world holds every reference whose collision is ready, drawn or not
([collision-placement.md](collision-placement.md), Collision readiness).
`FILE` lists points, one per line as `label x y z` separated by tabs or
spaces (blank lines and lines starting with `#` skipped; at most 4,096
points, 1 MiB, labels of at most 64 bytes without whitespace). For each
point the report gives:

- `surface`: the first surface at or below the point (`ray_down`);
- `surfaces`: every surface on the vertical line through it, highest first
  (at most 256), each with its primitive (`triangle`, `hull`, `sphere`,
  `capsule`), the side its stored winding faces (`up`, `down`, `neither`;
  `convex` for the others) and stored normal, its radius, its box and the
  instance, reference, base, model and leaf class it comes from;
- `ring`: the first surface below the point's height on 16 vertical lines
  20 units around it (a little more than the provisional radius, so that an
  edge the character could rest on shows as a spread between `min` and
  `max`);
- the character dropped with its feet at the point and settled with no input
  (`Character::settle`, at most 600 ticks): rest position, grounded, ticks,
  what it rests on with the contact normal, rest height minus `surface`, and
  the horizontal distance moved.

Outputs: `drops.tsv` (one row per point), one `walk_drop` item per point and
`walk_context` in `items.jsonl`, and the summary's `walk` block with
`drops`, `drops_grounded` and `drops_with_surface`; no `trace.json`. The
capsule is the provisional one: its rest height on a slope or an edge is a
consequence of `walk_capsule_radius_v1`, not a claim about the player.

## Viewer

`fo4-view MANIFEST DATA CELL --under-hypotheses --eye X,Y,Z --look X,Y,Z
--walk` opens the window in walk mode: the same world (built while loading),
the character's eye at the `--eye` point at the start, W/A/S/D along the view
heading, Shift to run, the left mouse button held to look; Tab switches to the
free-fly camera and back (a new character starts at the camera). In an
exterior cell the world includes the terrain of the cell and its neighbours
as in the report, and the outcome of each piece is printed. Without `--walk`
the viewer is unchanged.

`--terrain` (window or `--headless`) appends the cell's own terrain to the
draw list exactly as `render --terrain` does: its ground textures as
weighted passes over one opaque pass, untextured weight in flat grey per
triangle ([land.md](land.md), "Drawing"), tagged after the last placed
instance, with one line of what was textured and refused; without
`--around` the neighbours walked on are not drawn. A refused terrain is
printed and not drawn. Without the flag nothing is loaded and the draw list
is unchanged, so the CPU and GPU frames of one pose can be compared with or
without terrain by passing the flag to both.

`--around N` (1 to 3; window or `--headless`, and the `render` example)
joins the placed references of the exterior cells within N grid steps to
the cell's scene (`fo4_scene::grid`, [scene.md](scene.md)): they are drawn,
and with `--walk` their collision is placed in the walking world with the
cell's, under the same rules (with `--terrain`, in the window, as one piece per
cell that follows the player: [scene.md](scene.md), "The moving grid").
The terrain pieces are those of
`load_with` for the same search; with `--terrain` every built piece is
drawn, the cell first, each tagged after the last placed instance in piece
order, in both renderers. An interior cell is not searched. Which cells the
game keeps loaded around the player is not claimed.

Viewer conveniences, which claim nothing about the game:

- **Fall recovery.** While the walker is in the air with its feet below the
  lowest point of every primitive of the loaded world (or the world is
  empty), the time is counted; after more than one second (`RECOVER_AFTER`)
  it stands again, at rest, at the feet position of the last frame it spent
  on the ground (the start until it first lands).
- **R** returns to the start pose of `--eye`/`--look`, and to walking if
  the window started walking.
- **Title bar.** Mode (walk or fly), the speed asked for (walk or run, or
  the fly speed with Shift), the measured horizontal speed of the walker,
  the feet (walk) or eye (fly) position, whether the walker is on the
  ground and how many times it was put back. Updated at most five times a
  second.
- **Looking.** The left button turns the view by the raw mouse motion; when
  a frame has none (synthetic input, such as scripted drags on macOS, can
  move the cursor without raw motion), by the motion of the cursor in the
  window instead. The two are never added.
- **Waiting for a cell.** With the moving grid ([scene.md](scene.md),
  "The moving grid"), while the grid cell under the walker's feet has not
  arrived from the loading thread, the walker does not move: the mouse
  still turns the view, gravity and fall recovery wait, the frame's time
  is dropped, and the title bar says which cell it waits for. A grid
  position without a cell counts as arrived (nothing to wait for).
- **Scripted walk.** `--headless --walk --terrain --around N --path
  X,Y;...` settles the walker as below and walks it to each waypoint in
  turn at the run speed, one fixed tick at a time. A move whose distance
  shrank by less than 0.5 units over 60 ticks has stalled; a stalled leg
  steps aside, 512 units to the left of its line, then 1,024 to the right,
  and so on (at most 8 times), and then heads for the waypoint again. The
  detours are a property of the script, not of the controller.
- **Headless walk.** `--headless --walk` builds the same world, drops the
  walker with its eye at `--eye`, settles it without input (at most 600
  ticks) and draws the frame from its eye looking the same way; the settled
  feet, the ground under them and the pose to pass to the `render` example
  are printed.

System-level tests drive the window's input system in an app without a
window: Shift changes the walker's speed from the walk to the run speed,
and a drag with the left button turns the heading the walker then walks
along.

## Open

| Question | Status |
| --- | --- |
| Every parameter above | **[provisional]**, registered, pending |
| Whether the game's player is a capsule at all | open; the radius rule names the shape |
| Mesh vertex records as collision | left out |
| `hknpConvexShape` radius | not read; radius 0 |
| Triangle sidedness | both sides for the walker, by rule; open for the game |
| Terrain (LAND) | added under its own pending rules; the report places only the cell's collision, the viewer with `--around` also the neighbours' |
| XCLC hide-quad bits | not applied: hidden quads still carry the walker |
| Jumping, crouching, sprinting, swimming, ladders, doors, moving objects, actors | not implemented |
| Speed on slopes, acceleration, friction | not modelled: velocity is set from input each tick |
