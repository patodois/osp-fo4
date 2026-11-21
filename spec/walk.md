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

`World::ray_down(x, y, z_from)` returns the highest surface at or below
`z_from` on the vertical line through (x, y): the plane of a triangle the line
crosses (either winding; normal turned up), or the top of a sphere, capsule or
widened hull (found on the line by a convex search). Edge-on triangles are not
hit.

Distances between the character and a primitive are the closest points of two
convex point sets (GJK on the Minkowski difference), minus both radii.

## Terrain

`fo4_walk::terrain::load_around(store, cell, level, neighbours, budget)`
loads the terrain of the cell with `fo4_scene::terrain::load` at the walk's
rule level ([land.md](land.md)): the six terrain rules, all pending, so
terrain exists only at `--under-hypotheses`, with the diagonal level the
registry selects. With `neighbours`, when the cell is exterior, has an XCLC
grid, belongs to a world and the level accepts the terrain rules, it then
reads every other exterior cell of the same world once (through the
budget), keeps those whose grid is within one step, and loads each of the
eight neighbours the same way. Each of the nine positions is a `Piece`
with its grid, cell, LAND and outcome: `built:<diagonal>`,
`refused:<code>` (the refusals of `terrain::load`), `no_cell` or
`ambiguous_grid` (two or more cells claim the grid; neither is used, as in
the `land` report). Other cells claiming the cell's own grid are counted
and ignored. Only the cell's own collision is placed; the neighbours
contribute terrain only.

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
   after a fall. The walkable primitive is reported as the ground.

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

## Viewer

`fo4-view MANIFEST DATA CELL --under-hypotheses --eye X,Y,Z --look X,Y,Z
--walk` opens the window in walk mode: the same world (built while loading),
the character's eye at the `--eye` point at the start, W/A/S/D along the view
heading, Shift to run, the left mouse button held to look; Tab switches to the
free-fly camera and back (a new character starts at the camera). In an
exterior cell the world includes the terrain of the cell and its neighbours
as in the report, and the outcome of each piece is printed. Without `--walk`
the viewer is unchanged, and `--walk` is refused with `--headless`.

`--terrain` (window or `--headless`) appends the cell's own terrain to the
draw list exactly as `render --terrain` does: one opaque double-sided draw,
flat grey per triangle, tagged after the last placed instance; the
neighbours walked on are not drawn. A refused terrain is printed and not
drawn. Without the flag nothing is loaded and the draw list is unchanged, so
the CPU and GPU frames of one pose can be compared with or without terrain
by passing the flag to both.

## Open

| Question | Status |
| --- | --- |
| Every parameter above | **[provisional]**, registered, pending |
| Whether the game's player is a capsule at all | open; the radius rule names the shape |
| Mesh vertex records as collision | left out |
| `hknpConvexShape` radius | not read; radius 0 |
| Triangle sidedness | both sides for the walker, by rule; open for the game |
| Terrain (LAND) | added under its own pending rules; the neighbours' collision is not placed, only their terrain |
| XCLC hide-quad bits | not applied: hidden quads still carry the walker |
| Jumping, crouching, sprinting, swimming, ladders, doors, moving objects, actors | not implemented |
| Speed on slopes, acceleration, friction | not modelled: velocity is set from input each tick |
