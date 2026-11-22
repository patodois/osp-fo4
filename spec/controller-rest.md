# Animation controllers at rest

Models that hold animation-controller blocks (the `state` class of
[static-blocks.tsv](static-blocks.tsv)) were refused whole: every reached
block of that class raised `UnsupportedAnimationRuntime`, and the coverage
census ranked them as the largest omission by area
([coverage.md](coverage.md)). This file records the census of those blocks,
the pending rules under which a model is drawn as it is at rest, split by
what each controller drives, and what stays refused and why. Nothing here
has been measured in the running game. Counts only; model paths stay in
unversioned outputs.

## How the code refused them

- `fo4-nif` reads the controller, interpolator, sequence and key-data blocks
  to their last byte where their nifxml layout is implemented (list below);
  it evaluates nothing.
- `fo4-scene` (`assets`) walks a model from its roots through children and
  every link, the weak back pointers of controller blocks (`Target`,
  `Extra Target`, a sequence's `Manager`, the palette's links) excepted, and
  raises `UnsupportedAnimationRuntime` for each reached block of the `state`
  class. A block the reader does not decode raises `UnsupportedNifBlock` on
  its own.
- `fo4-render` and the collision placement skip `state` blocks: a model
  that becomes ready is drawn and placed with the nodes' stored transforms
  and the shaders' stored fields.

## Census (`controller_census`)

`controller_census MANIFEST OUT.txt` (an example of `fo4-unpack`) reads
every winning NIF of the archives (224,366; 2 unreadable; 70 s, peak RSS
0.75 GB), walks each as the asset graph does and keeps the models with a
reached `state` block that is not a particle-system block. Counts:
[controller-census.txt](controller-census.txt).

- 4,070 such models; 1,237 named by the bases of placed references
  (76,008 enabled placed REFR; many of those are also omitted for other
  reasons); 563 also hold particle-system blocks.
- Block types and whether each is read to its last byte: every type is,
  except `NiBlendBoolInterpolator` (4,307 blocks: no layout in the reader),
  `BSLagBoneController` (77), `BSProceduralLightningController` (68),
  `NiPathInterpolator` (25) and `BSLightingShaderPropertyUShortController`
  (2). 3,431 models have every such block read to the last byte.
- What the controllers drive (by type, and for shader controllers by the
  controlled variable of the nifxml enums `EffectShaderControlledVariable`,
  `EffectShaderControlledColor`, `LightingShaderControlledFloat`,
  `LightingShaderControlledColor`): transform 15,811, texture transform
  4,500, alpha 2,466, emissive terms 1,346 (+767 effect-shader emissive
  colour), visibility 1,187, light 370, falloff 294, other lighting terms
  131, other (managers, lag bone, procedural lightning) 1,777.
- Controller flags (nifxml `TimeControllerFlags`): of 15,811 transform
  controllers 1,694 carry Manager Controlled (bit 5); 13,688 are active,
  not manager-run, cycle type clamp.
- Transform values. For each `NiTransformInterpolator` and its node (a
  controller's `Target`, or a sequence's controlled block by `Node Name`):
  the quaternion of the pose value, read as rows of the matrix of
  `rest::quaternion_rows`, equals the node's stored rotation words in
  2,168 controller and 5,034 sequence interpolators, its transpose alone in
  none (both readings, a symmetric matrix, in 376 and 1,840). Sequence
  interpolators: pose translation equal to the stored one in 10,337 of
  12,211, pose rotation in 6,874 of 7,244; key at the sequence start time
  equal to the stored translation in 3,701 of 6,062 (worst difference
  47,554 units). Controller interpolators: first key equal to the stored
  translation in 10,316 of 10,483 (worst 3,486). Tolerances: translation
  0.001, rotation word 0.001, scale 0.0001. XYZ rotation keys (942
  controller, 12,040 sequence interpolators) are not compared.

## Rules (`fo4_scene::rest`)

Every `state` block gets the tags of what uses it: a controller its own;
an interpolator, a blend interpolator or a key-data block the tags of the
controllers and sequence controlled blocks whose interpolator links reach
it. `UnsupportedAnimationRuntime` is raised once per tag, with `;rest=TAG`
at the end of its text. Five tags have a pending rule:

| Tag | Rule | Blocks |
| --- | --- | --- |
| `manager-idle` | `nif_controller_rest_manager_idle_v1` | `NiControllerManager`, `NiMultiTargetTransformController`, a `NiControllerSequence` that does not loop (cycle type other than 0), and `NiDefaultAVObjectPalette`, `NiTextKeyExtraData` in a model with a manager |
| `idle-transform` | `nif_controller_rest_idle_transform_v1` | a transform controller with Manager Controlled in a model with a manager; transform controlled blocks of a sequence that does not loop; what their interpolators reach |
| `idle-shader` | `nif_controller_rest_idle_shader_v1` | the same for alpha, texture transform, lighting-shader float and colour and light controllers (not effect-shader controllers) |
| `transform-constant` | `nif_controller_rest_transform_constant_v1` | a transform controller not run by a manager whose interpolator's set pose values and every key equal the node's stored transform |
| `unrendered` | `nif_controller_rest_unrendered_v1` | a controller not run by a manager that drives a light, or a lighting-shader term the unlit frame does not draw (`LightingShaderControlledFloat` 0, 8 to 11, `LightingShaderControlledColor` 0, 1) |

The other tags name why the rest state is not decided, and stay refused
under them: `behavior-graph` (`BSBehaviorGraphExtraData`: a graph decides
which sequence plays), `transform-varies` (a transform controller not run
by a manager with keys away from the stored transform: an animation that may
be running), `visibility` (the frame does not read the stored
`NiAVObject` flags, so a stored hidden node would be drawn), `alpha`,
`texture-transform`, `effect-emissive` and `falloff` (controllers not run by
a manager that change what the frame draws), `particle` and `other`
(controller types outside the lists, controlled blocks of unknown type, a
`NiTextKeyExtraData` or palette without a manager), `manager-loop` (a
sequence that loops, with what its controlled blocks reach: it may be
playing at rest; since the clock step covered by
`nif_controller_rest_loop_unstarted_v1`, below) and `effect-idle` (an
effect-shader controller run by a manager: the frame draws effects without
their falloff and soft terms, so the stored alpha and colour are not
settled to be what shows).

The last two were split off after a first version of the rules, in which
every manager-run controller and every sequence counted as idle, drew the
lobby and vault test views under a white veil: large ambient mist and
light-beam effects, driven by looping sequences on effect shaders, drawn
opaque with their stored values (921,588 and 335,677 pixels of the two
frames differed from the frames before; frames kept outside the
repository). That version decided 734 models (39,711 placements).

By the census: 545 models (22,607 enabled placed REFR) have only tags with
a rule; of the placements decided, 21,967 rely on `manager-idle` and
`idle-transform`, 164 on `idle-shader`, 605 on `transform-constant` and 91
on `unrendered`. Of the decided models, 395 have only clamped sequences
(cycle type 2) and 150 none. The largest groups left: `effect-idle` with
`manager-loop` (15,040 placements), `transform-varies` alone (1,033 models,
5,982 placements), `texture-transform` alone (174, 5,967), the same with a
behaviour graph (5,083), `behavior-graph` alone (331, 2,288),
`effect-emissive` alone (74, 2,162).

A sixth tag, `effect-first-key`, was added with the effect surfaces
([effect-surfaces.md](effect-surfaces.md)): an effect-shader controller
whose target field holds, as stored, the first key of every interpolator
that drives it (and a looping sequence of such controllers alone) is
covered by `nif_controller_rest_effect_first_key_v1`; it replaces
`effect-idle`, `manager-loop` and the effect tags only there. The draw
build then draws the model's effect shaders by the effect surface rules.

A seventh, `texture-first-key`, does the same for lighting-shader UV
controllers (`nif_controller_rest_texture_first_key_v1`).

## The clock and loops that nothing starts

The clock step ([controller-clock.md](controller-clock.md)) adds the tag
`clock`, with one cycle tag (`clock-loop`, `clock-reverse`, `clock-clamp`)
and one key tag (`clock-linear`, `clock-quadratic`, `clock-step`), each
under its pending rule: a free-running alpha or UV controller of an effect
or lighting shader that the first-key rules leave unsettled and that the
game clock can run (`fo4_scene::clock::track`) takes it in place of
`alpha` or `texture-transform`, with what its interpolator reaches. Such a
model is drawn only by a build with a time. A looping sequence
(`manager-loop`) is covered by `nif_controller_rest_loop_unstarted_v1`:
nothing starts it, so its controlled blocks other than effect-shader
controllers (which keep `effect-idle`) leave the model as stored; the
tension with the first-key rules' reading of a looping sequence is written
into the rule. `fo4_scene::rest::rule` maps every covered tag to its rule.

By the census after that step: 1,210 of the 4,070 models (48,398 enabled
placed references) have only covered tags, against 1,041 models (46,087)
with the rules before it; 941 of those placements rely on the clock and
1,370 on the unstarted loop rule.

## Coverage

See [coverage.md](coverage.md), Controllers at rest, Effect surfaces and
The game clock.

## Provenance

Layouts and enums: the pinned nifxml copy of [nif.md](nif.md)
(`TimeControllerFlags`, `CycleType`, `NiQuatTransform` with the quaternion
stored w, x, y, z, the four shader controlled-variable enums, the
`NiControllerManager` description). Recognised from prior knowledge and
kept only as the pending rules above: that a controller manager's
sequences are started by name and otherwise idle, that a keyframe
interpolator gives its keys' values and nothing else, and the standard
matrix of a unit quaternion (whose row reading the census checked against
the stored node words). No other NIF reader, editor or game executable was
opened.
