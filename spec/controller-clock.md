# The game clock of controllers

The rest rules ([controller-rest.md](controller-rest.md)) settle a
controller only where nothing moves: a sequence that is not started, or a
controller whose stored field is the first key of every interpolator that
drives it. The largest families they left refused were models whose
shaders carry alpha and texture-transform controllers that run by
themselves with a stored field away from their first key: drawing them
means choosing a moment. This file records the census of every
controller by what it drives and what runs it, the clock that the draw
build now takes (a time in seconds after the model loaded), the pending
rules the clock relies on, what a looping sequence that nothing starts
draws, and what stays refused. Nothing here has been measured in the
running game. Counts only; model paths stay in unversioned outputs.

## Census (`controller_census`, Clock)

The `controller_census` example ([controller-census.txt](controller-census.txt))
now also counts, for every model it keeps, each controller by the field it
drives (the nifxml controlled-variable enums, paired by name) and by what
runs it: free-running (Manager Controlled clear, or no manager in the
model, and named by no sequence), a manager's flag without a sequence, or
the sequences that name it with their cycle types. For the free-running
ones it reads the controller's timing (`fo4_scene::clock::Timing`) and,
where the clock can run it, its track (`fo4_scene::clock::track`): flags,
frequency, phase, start, key type and count, the first and last key
against Start Time and Stop Time, the seconds per cycle, the stored field
against the keys and against the clock's value at load, the step at the
seam of a loop, and the tangents of quadratic keys. Lines `clock:`,
`transform:` and `manager-loop:`.

Alpha and UV fields of effect and lighting shaders (EffectShaderControlledVariable
5 to 9, LightingShaderControlledFloat 12, 20 to 23):

- 2,999 free-running controllers. 2,886 have the Active flag, play
  forwards and have Anim Type APP_TIME; the other 113 are inactive with
  APP_INIT. Frequency is 1 and phase 0 on every one.
- 2,346 give a track. Left out: Start Time equal to Stop Time or above it
  (496: 348 with both 0, 148 others), inactive (113), TBC keys (44).
- On every track the first key sits on Start Time and the last on Stop
  Time, so the first key and the key at Start Time are the same key.
- Cycle and keys: loop 2,150 (quadratic 1,645, linear 496, constant 9),
  clamp 196 (quadratic 102, linear 90, constant 4), reverse none.
- The stored field equals the clock's value at load on 1,937 tracks; on
  the other 409 it lies inside the key range (equal to another key on
  62), never outside it.
- Seams: 1,226 looping UV offsets step by a whole number from Stop Time
  back to Start Time (a texture that scrolls by whole repeats), 856 tracks
  end on their first value, 68 step otherwise.
- Seconds per cycle: below 1 on 204, 1 to 5 on 556, 5 to 30 on 778, 30
  to 300 on 782, 300 or more on 26.
- 279 tracks' shaders name a material.

Quadratic keys: of 13,330 intervals of the free-running float tracks
(every field) whose time step is not 1 and whose value changes, 10,011
have a zero forward tangent, 361 a tangent equal to the interval's
difference, 246 equal to the difference per second, and 2,712 neither.

Models: 434 models with a UV or alpha track; 235 of them (7,419 enabled
placed references) are decided by the rest rules and have no other
free-running effect track that varies, 199 (1,016) are decided and also
have a varying free-running track of a field the draw's animation does
not carry (base colour scale, emissive colour, falloff), and 255 (3,284)
are not decided for another tag.

Looping sequences (`manager-loop`): 550 models carry the tag (11,640
placements); 104 of them (1,370 placements) have no other open tag. Of the
tagged models with a placement, 71 are named only by movable statics, 66
only by activators and 34 only by statics; 54 hold a single looping
sequence and no behaviour graph. The decided ones are placed through
movable statics (692 placements), miscellaneous items (340), statics
(198), containers (122) and a few others.

Transform controllers: 14,117 free-running, 13,823 of them clamped,
13,677 in models tagged `transform-varies`; the largest group (9,730)
moves a translation with linear keys and has no rotation keys, then
controllers with no key group the census reads (1,806), linear rotations
(480), XYZ rotations with or without quadratic scale keys; 368 have
collision under their node.

## The clock (`fo4_scene::clock`, `fo4_render::draw::BuildOptions::time`)

A build with a time (`render --time SECONDS`, `fo4-view --time SECONDS`)
evaluates every free-running controller the clock can run that drives a
shader's alpha or UV offset or scale, at that many seconds after the
models loaded:

1. controller time = Start Time + Phase + Frequency x seconds
   (`nif_controller_clock_free_running_v1`);
2. folded into Start Time to Stop Time by the cycle type: wrapped
   (`nif_controller_cycle_loop_v1`), back and forth
   (`nif_controller_cycle_reverse_v1`) or held at the ends
   (`nif_controller_cycle_clamp_v1`);
3. the value of the keys at that time, by key type: linear
   (`nif_key_linear_v1`), cubic Hermite with the stored tangents per
   interval (`nif_key_quadratic_hermite_v1`), step (`nif_key_constant_step_v1`);
   the first key's value before the first key and the last key's after
   the last.

The value replaces the field the draw takes: the UV offset or scale
component (under `uv_scale_then_offset_v1` for a textured surface) or the
opacity (the effect shader's base colour alpha, the lighting shader's
alpha; an opacity track's whole range must pass the opacity rules, and a
range below 0 is refused). The draw keeps the tracks
(`DrawList::animations`, one `fo4_render::Animation` per draw), so that
`DrawList::at_time` gives the same draw at any other time; the viewer's
window sets the animated surfaces' parameters from the same tracks every
frame. The instance records the clock rules it relied on.

Where a material holds a value of a driven field other than the NIF's:
for an effect shader `effect_material_over_controller_v1` keeps the
material's value at every time and the clock does not drive that field;
for a lighting shader the surface is refused (`UnsettledClockMaterialField`).

A field the draw's animation does not carry (an effect shader's base
colour scale, emissive colour, falloff fields) whose free-running track
gives at the build time a value other than the stored one is refused
(`UnsettledClockField`): a frame of the clock never mixes fields at the
stored value with fields at the clock's. At time 0 the first-key rules
already require the stored value, so those surfaces draw there; at other
times they are refused: the census at seconds 1.7, 7.3 and 45 refuses
1,206 to 1,219 placements so, the distant clouds among them, and the
omitted area rises from 2.63 % to 28.77 % of all ([coverage.md](coverage.md),
The census over time). The switches below carry those fields instead. The
window holds them at the start time's value while the UV fields and opacity
move.

Without a time nothing changes: an instance that relies on
`nif_controller_clock_free_running_v1` is omitted
(`UnsettledControllerTime`), and every other field is drawn as before.
Below the hypotheses level nothing with a controller is drawn, with or
without a time.

## Rules

| Rule | Covers | Basis and weakness |
| --- | --- | --- |
| `nif_controller_clock_free_running_v1` | rest tag `clock`: a free-running alpha or UV controller of an effect or lighting shader that is not settled by a first-key rule and that the clock can run | every free-running track starts on its Start Time; frequency 1 and phase 0 everywhere; the stored field is the clock's value at load on 1,937 of 2,346 tracks and inside the range on the rest. That the controller runs from its model's load, and the formula, are prior knowledge; the clock could as well be the game's absolute time, or start when the model is first seen. |
| `nif_controller_cycle_loop_v1` | `clock-loop` | 1,226 UV offsets step by whole repeats at the seam. The modulo is prior knowledge. |
| `nif_controller_cycle_reverse_v1` | `clock-reverse` | no alpha or UV track uses it; registered because the code names the reading. |
| `nif_controller_cycle_clamp_v1` | `clock-clamp` | 196 tracks; holding the last key is prior knowledge. |
| `nif_key_linear_v1` | `clock-linear` | nifxml's "linear interpolation". |
| `nif_key_quadratic_hermite_v1` | `clock-quadratic` | nifxml names forward and backward tangents; the cubic is prior knowledge and the tangents' scale is not settled by the data (10,011 zero, 361 per interval, 246 per second, 2,712 neither). |
| `nif_key_constant_step_v1` | `clock-step` | nifxml's "step function"; 13 tracks. |
| `nif_controller_rest_loop_unstarted_v1` | rest tag `manager-loop` | that a sequence plays only when something starts it is prior knowledge; nothing in the data weighs for or against the engine starting a looping sequence by itself. |

Tensions, written into the rule texts:

- The first-key rules (`nif_controller_rest_effect_first_key_v1`,
  `nif_controller_rest_texture_first_key_v1`) say a controller shows its
  first key "at the frame's instant". The clock makes that instant its
  time 0 for free-running controllers; at time 0 the two agree wherever
  the first-key rules settle a field, and the clock draws the first key,
  never the stored value, where they do not.
- For a looping sequence the first-key rules read the frame's instant as
  the sequence's start; `nif_controller_rest_loop_unstarted_v1` draws the
  stored field and never advances it. The two agree where the first-key
  rules apply (stored value = first key); an engine that starts a looping
  sequence by itself on load would refute the second rule only.
- `effect_material_over_controller_v1` says the material replaces a driven
  field and the controller does not override it; the clock follows it
  (the field is held at the material's value at every time).

## What stays refused

- Free-running alpha or UV controllers outside the clock: zero or negative
  span (496), inactive APP_INIT controllers (113), TBC keys (44); their
  models keep the `alpha` or `texture-transform` tag.
- Effect-shader controllers run by a manager (`effect-idle`): the effect
  surface rules exist now, but the idle rules leave effect controllers out
  on purpose ([controller-rest.md](controller-rest.md)); the
  loop-unstarted rule keeps that exclusion. No rule says what starts their
  sequences (below, Effect controllers run by a manager).
- Transform controllers whose keys move a node (`transform-varies`): the
  clock's time and keys would give their values, but a moving node changes
  a draw's world transform and the collision placement, not a surface
  parameter: the viewer's instance data, the walking world and the
  rotation interpolations (quaternion keys, XYZ Euler keys, TBC) would each
  need their own machinery and rules. Left refused.
- Free-running controllers of the effect emissive multiple, emissive
  colour and falloff fields are run by the clock only behind their
  switches (below, Effect fields on the clock); without them the
  first-key and rest rules decide them as before. Visibility and light
  controllers are not run by the clock.
- Behaviour graphs, visibility controllers, particle systems.

## Effect fields on the clock (switches, off by default)

`effect_clock_census MANIFEST OUT.txt` (an example of `fo4-unpack`;
[effect-clock-census.txt](effect-clock-census.txt), every winning NIF, 50 s)
counts every controller of an effect shader by the field it drives
(nifxml EffectShaderControlledVariable and EffectShaderControlledColor), by
what runs it, and for a free-running one by its clock track, with its rest
tag with the clock's switches off and on. Free-running controllers of the
three fields the draw's animation did not carry:

| Field | Controllers (placements) | Clock tracks | Vary | Stored is not the clock at load | Today's rest tag |
| --- | ---: | ---: | ---: | ---: | --- |
| emissive multiple (variable 0, Base Color Scale) | 314 (9,238) | 229 | 215 | 48 | first key 248, `effect-emissive` 56 |
| emissive colour (colour 0, Base Color RGB) | 257 (2,427) | 204 | 200 | 22 | first key 217, `effect-emissive` 30 |
| falloff (variables 1 to 4) | 292 (407) | 292 | 292 | 0 | first key 292 (all on the distant clouds) |

The controllers without a track: span not positive, inactive with
APP_INIT, TBC keys, another interpolator type (85 emissive multiple, 53
emissive colour). Most emissive multiple tracks reach values above 1 (201
of 215 varying ones); 99 emissive colour tracks have a component above 1;
none goes below 0. The falloff tracks are 146 start opacities (within 0 to
1) and 146 stop angles (63 to 73 degrees on 144, 25 to 123 on 2), all
looping quadratic keys of 30 to 300 s per cycle. Manager-run controllers of
these fields: 723 emissive multiple and 506 emissive colour controllers
named by sequences (below).

By model: 13 models (1,497 placements) have `effect-emissive` as their only
open tag and are decided with the switches on; 260 models (2,465
placements) are decided either way and hold a free-running emissive,
colour or falloff track that varies, so that without the switches the draw
refuses them at any time where the track has moved
(`UnsettledClockField`), and with them draws them; 69 models (424
placements) stay undecided for other tags.

**Switches** (`fo4_scene::clock::Switches`; `render`, `fo4-view` and
`coverage_census` flags `--clock-emissive-multiple`,
`--clock-emissive-colour`, `--clock-falloff`; the scene must be loaded with
the same switches, `Loader::set_clock_switches`). With a switch on:

- the rest tags (`rest::tags_with`): a free-running effect controller of
  that field that is not settled by the first-key rule and that the clock
  runs is tagged `clock` with its cycle and key tags and a field tag
  (`clock-emissive-multiple`, `clock-emissive-colour`, `clock-falloff`; an
  angle also `falloff-degrees`), each covered by its rule;
- the draw build: with a time, the field's track (first-key settled
  controllers included) joins the draw's animation (`fo4_render::Animation`
  slots 5 to 10). The emissive colour and multiple give the draw's colour
  (`Draw::colour` = base colour x multiple, each tracked or held, applied
  after the vertex colour in both renderers; the mesh's vertex colours then
  hold no tint), the falloff tracks give the draw's falloff fields (an
  angle's key read as degrees, the field its cosine). The colour's range
  over the whole track must stay at or above 0 (`colour-negative`
  otherwise) and records `effect_colour_clamped_before_blend_v1` where it
  can exceed 1; a falloff track's opacities must stay within 0 to 1
  (`falloff-fields` otherwise) and the falloff must pass its validation at
  each time; falloff tracks of a surface whose falloff is not drawn are
  dropped. A BGEM value of the field wins at every time
  (`effect_material_over_controller_v1`), as for alpha and UV.

With every switch off the rest tags, the draw lists, the frames and the
committed census outputs are those of before (see coverage.md).

| Rule | Basis and weakness |
| --- | --- |
| `effect_clock_emissive_multiple_v1` | the census above; that the clock runs this field as it runs alpha and UV is the same prior knowledge as `nif_controller_clock_free_running_v1`; the field's pairing with Base Color Scale is the first-key rule's by-name pairing. Weak. |
| `effect_clock_emissive_colour_v1` | the same, the pairing with Base Color RGB. Weak. |
| `effect_clock_falloff_v1` | the same; the degrees reading of angle keys is `nif_controller_falloff_angle_degrees_v1`'s. Weak. |

## Effect controllers run by a manager

Of the census's models, 465 (9,634 placements) hold an effect controller
run by a manager (`effect-idle`). What the data says about what starts
their sequences:

- No field of `NiControllerManager` or `NiControllerSequence` marks a
  default or idle sequence (nifxml: the manager holds a cumulative flag,
  its sequences and an object palette; a sequence its name, controlled
  blocks, weight, text keys, cycle type, frequency, start and stop times
  and an accumulation root name).
- 377 models have a behaviour graph (`BSBehaviorGraphExtraData`; 6,921
  placements): its root state machine's start state plays a sequence that
  names an effect controller in 295 of them (6,601 placements), plays no
  sequence in 79 (312), another sequence in 2; one graph is not read.
  These models also carry the open `behavior-graph` tag.
- 88 models have no graph (2,713 placements). In those with a single
  sequence (the manager's only one), the sequence loops in 14 models
  (2,253 placements) and drives effect UV and alpha controllers (1,811 of
  those placements also transform interpolators through the manager's
  multi-target transform controller); the names of the most placed of
  them are words for idle (local listing). The others
  (clamped single sequences, or two to twelve sequences with words for
  open, close, wind, on, off) have few or no placements.
- The sequence names of 251 of the 465 models are held, all or some, as
  strings by compiled scripts (6,365 placements); 5,332 placements carry a
  script on the reference or its base.
- The stored fields of the manager-run controllers match the first key of
  some of their sequences and the last key of others; no class of
  sequence holds the stored value throughout.

No rule was added. The one marker in the data is the behaviour graph's
start state, a statement of the graph format and not of the NIF: a rule
"the start state's sequence plays from load" would also have to settle the
`behavior-graph` tag, evaluate a sequence's own timing (its start, stop,
frequency and cycle type, apart from the controllers') and its transform
interpolators (moving geometry, below), and would contradict
`nif_controller_rest_loop_unstarted_v1` for the same kind of looping
sequence where the graph has none; a single looping sequence without a
graph, read as playing from load, contradicts that rule directly, and its
name (a word for idle) is a convention, not a field. The family stays
refused; the census lines `2 ...` keep the counts for a later step.

## Measurements that would settle the rules

Each rule row in [hypotheses.tsv](hypotheses.tsv) names its measurement;
in short: frames from the end of a loading screen of a scrolling effect
(start at load, period, seam, restart on reload), a clamped alpha fade
past its stop time, frames inside one interval of a quadratic pulse, and
a movable static with a single looping sequence and no script watched
without interacting.

## Provenance

nifxml (pinned copy of [nif.md](nif.md)): `TimeControllerFlags` (Anim
Type, Cycle Type, Active, Play Backwards, Manager Controlled), the
`NiTimeController` fields and their descriptions (Frequency "usually 1.0",
Phase "usually 0.0", Start Time, Stop Time), `CycleType` (LOOP, REVERSE,
CLAMP) and `KeyType` (linear, quadratic with forward and back tangents,
TBC, XYZ, constant "step function"). Nifxml gives names, not formulas.
Recognised from prior knowledge of Gamebryo controllers and kept only as
the pending rules above: the time formula, the three cycle foldings, the
cubic Hermite reading of quadratic keys with tangents per interval, and
that a manager's sequences play only when started. No NIF reader,
renderer, editor, MIT reference or game executable was opened.
