# Effect surfaces

Models drawn by the effect shader (`BSEffectShaderProperty`: mist volumes,
light beams, glows, clouds) with controller blocks were the largest omission
by area ([coverage.md](coverage.md)): their controllers were left undecided
on purpose ([controller-rest.md](controller-rest.md)), after a first attempt
to draw them as stored covered two test views in white. This file records
the census of those models, the rules under which part of them is drawn,
what stays refused and why, and the frames. Nothing here has been measured
in the running game. Counts only; model paths stay in unversioned outputs.

## Why the first attempt was white

The draw build took an effect surface as its texture alone: the texel's RGB,
the texel's alpha times the base colour's alpha as opacity, and the blend
state of the alpha property (or of the material). The base colour's RGB and
scale, the vertex colours, the falloff and the greyscale palette were not
applied. The census below shows that the large ambient effects depend on
exactly those terms: most blend additively or with source alpha, their
textures are mostly bright, their vertex alpha fades to 0 at the edges and
their falloff takes their alpha to 0 at grazing angles. Without those terms
every layer stacks to white.

## Census (`effect_census`)

`effect_census MANIFEST OUT.txt [--omitted BASES.tsv]` (an example of
`fo4-unpack`) reads every winning NIF of the archives (224,366; 2
unreadable; 86 s, peak RSS 0.78 GB), walks each as the asset graph does,
computes the rest tags of [controller-rest.md](controller-rest.md) and keeps
a model when a reached `state` block (particle systems aside) has a tag
without an earlier rule and that tag is `effect-idle`, `manager-loop` or
`effect-first-key`, or the block is an effect-shader controller. With
`--omitted`, a coverage census's `bases.tsv` (the one of the commit before,
all cells) gives the omitted placements and area by model. Counts:
[effect-census.txt](effect-census.txt).

- 1,651 models in scope; 571 named by placed bases. They hold 5,308 effect
  shapes and 5,766 lighting shapes. Every effect shader block is read to
  its last byte (the nifxml `BSEffectShaderProperty` of `bs_version` 130:
  flags, UV, source texture, clamp mode, lighting influence, environment
  map LOD, the unused byte, four falloff floats, base colour and scale,
  soft falloff depth, greyscale texture, environment, normal and mask
  textures, environment map scale). 339 shapes name an effect material
  (BGEM), 3 a material that is missing or unreadable, the rest none.
- Alpha property, by flag word (nifxml `AlphaFlags`): blending with
  SRC_ALPHA / ONE on 2,830 shapes (103 with the alpha test), SRC_ALPHA /
  INV_SRC_ALPHA on 2,009 (119 with the test), ONE / SRC_COLOR on 23,
  DEST_COLOR / ZERO on 1, blending off with the test on 17, no alpha
  property on 428. The test function is GREATER (4) wherever the test is on;
  No Sorter (bit 13) is never set, Clone Unique (bit 14) on 4. The material
  tuples agree: (6,0) 117, (6,7) 154, (4,1) 1, blending off 67.
- Shader Flags 1 (nifxml `Fallout4ShaderPropertyFlags1`): ZBuffer_Test
  5,301, Soft_Effect 2,359, External_Emittance 2,007, GreyscaleToPalette
  Color 1,780 and Alpha 1,108, Use_Falloff 1,765, Vertex_Alpha 509,
  Environment_Mapping 377, RGB_Falloff 110, Decal and Dynamic_Decal 117,
  Refraction 19, Fire_Refraction 12. Shader Flags 2: Vertex_Colors 4,168,
  Double_Sided 1,883, Effect_Lighting 2,007, ZBuffer_Write 940. A shape
  that names a BGEM and its material disagree on the palette flag in 2 and
  on the soft flag in 1 (of 339).
- Fields (value histograms in the counts): base colour RGB white on 2,819
  shapes, grey 550, coloured 1,814, black 125; base colour alpha 1 on 2,544,
  0 on 941, between 0 and 1 on 1,545, above 1 on 241, negative on 37; base
  colour scale 1 on 2,100, above 1 on 2,705 (10 or more on 454), below 1 on
  503 including 0 on 132. With Use_Falloff set, the opacities
  are (1, 0) on 1,538 of 1,765 shapes. Soft falloff depth 100 on 2,789.
  Lighting influence (a byte) 255 on 3,600. The greyscale texture is set
  on 2,597 shapes (with neither palette flag on many of them).
- Drawn texture (the material's base texture, else the shader's source
  texture), decoded on a small mip: BC3 with varying alpha on 3,743 shapes,
  BC1 whose alpha is 1 on 684, no texture on 777, missing on 24.
- Vertex colours: Vertex_Colors and vertex alpha below 1 under it on
  2,583 shapes, of which 2,076 without Vertex_Alpha. The largest vertex
  alpha of a shape, against the two flags: every vertex alpha 0 on 8
  shapes without Vertex_Alpha and 1 with it; below 0.5 on 168 without and
  151 with; 1 on 2,165 without and 301 with. The data does not say whether
  vertex alpha applies when Vertex_Colors is set and Vertex_Alpha is not.
- Effect-shader controllers: 3,593 not run by a manager, 3 run by a
  manager outside any sequence, 4,557 named by sequences (2,683 looping,
  1,874 clamped). Driven fields (by the nifxml enums, paired with the
  fields by name): alpha 2,099, V offset 1,819, U offset 1,395, base colour
  scale (EmissiveMultiple) 1,038, base colour RGB (Emissive Color) 763, V
  scale 422, U scale 323, falloff start opacity 148 and stop angle 146 (the
  distant clouds). Of 7,635 controllers with keys, the stored field (the
  BGEM's where one is named) equals the first key in 5,890, another key in
  246, lies inside the key range at no key in 661 and outside it in 838
  (666 of those in looping sequences: export snapshots of a running
  animation). Where a BGEM is named the NIF and BGEM values of the driven
  field are equal in 344 of 350 comparisons.

### Classes

By the rest tags left after the rule below and by what the effect surface
rules cannot settle (`settled, effect surfaces fail: ...`), with
placements and area of every enabled placed REFR of the model, and the
omitted placements and their share of the omitted area of the census
before this step:

| Class | Models | Placements | Omitted | Share of omitted area |
| --- | ---: | ---: | ---: | ---: |
| rest not settled (another open tag, or a field stored away from its first key) | 1,340 | 17,752 | 17,747 | 66.55 % |
| of which `falloff` alone (distant clouds: falloff stop angle stored outside its keys) | 146 | 407 | 407 | 63.23 % |
| settled; falloff and vertex alpha without its flag (the large mist volumes) | 26 | 12,882 | 12,882 | 6.31 % |
| settled; colour above 1 and vertex alpha without its flag | 28 | 667 | 667 | 1.41 % |
| settled; colour above 1, falloff and vertex alpha without its flag | 18 | 883 | 883 | 0.36 % |
| settled; other combinations of palette, feature, falloff, colour, alpha range, missing material or texture, vertex alpha | 226 | 3,060 | 2,716 | 0.40 % |
| all classes | 1,651 | 35,653 | 34,904 | 75.03 % |
| settled; every effect surface passes | 13 | 409 | 9 | 0.00 % |

## Rules

Registered in [hypotheses.tsv](hypotheses.tsv), each pending, split so
that each can be refuted alone:

- `nif_controller_rest_effect_first_key_v1` (`fo4_scene::rest`, tag
  `effect-first-key`): an effect-shader controller shows its first key at
  the frame's instant; where every interpolator that drives it (its own,
  and the controlled block of every sequence that names it) starts at the
  stored field, the field is drawn as stored. A looping sequence whose
  controlled blocks are all such controllers is covered with them. The two
  readings "at rest = first key" and "at rest = stored value" agree only
  there; elsewhere the old tag stays (`effect-idle`, `manager-loop`,
  `alpha`, `texture-transform`, `effect-emissive`, `falloff`). For a shape
  that names a BGEM, the draw build compares the BGEM's value of each
  driven field with the NIF's and refuses a difference
  (`UnsettledEffectMaterialField`).
- `effect_surface_texture_base_colour_v1` (draw rule): colour = texel RGB x
  base colour RGB x scale x vertex RGB under Vertex_Colors; alpha = texel
  alpha x base colour alpha x vertex alpha under both vertex flags; the
  material's fields replace the NIF's; an empty texture is a white texel.
- `effect_blend_factors_add_v1` (draw rule, blending on): source x F(src)
  + destination x F(dst), clamped, F by nifxml `AlphaFunction`.
- `effect_blend_after_opaque_back_to_front_v1` (draw rule, blending on):
  after opaque surfaces, farthest first, no depth write (the CPU by
  triangle, the GPU by instance; `render.md`).
- `effect_soft_lighting_left_out_v1` (draw rule, when a shader or material
  sets them): soft depth fade, lighting influence and effect lighting do
  not change this unlit frame.
- `effect_falloff_view_angle_v1`: registered with the measurement that
  could refute it, not relied on by code.

The draw build applies the effect path only to instances whose state
relies on `nif_controller_rest_effect_first_key_v1`; every other effect
surface is drawn as before, so frames without hypotheses, and frames under
hypotheses of models without settled effect controllers, are unchanged.

## What stays refused, and why

An effect surface of a settled model is refused with
`UnsupportedEffectSurface`, its text naming every term the frame does not
settle, joined by `+`:

- `palette`: greyscale-to-palette colour or alpha (the lookup coordinates
  into the greyscale texture are not given by any source here);
- `feature`: environment map, refraction, glass, glow map, blood, emittance;
- `falloff`: Use_Falloff, RGB_Falloff or the material's falloff (nifxml
  gives the end points only; the rule above is registered, not drawn);
- `colour-range`: base colour x scale outside 0 to 1 (an emissive value
  above what the unlit frame can show; how the game maps it is unknown);
- `vertex-alpha-one-flag`: vertex alpha below 1 with Vertex_Colors or
  Vertex_Alpha set alone, where the two readings of the vertex flags
  disagree (census above).

The large mist volumes are refused for `falloff+vertex-alpha-one-flag`:
those are exactly the terms whose absence turned them white.

## Coverage

See [coverage.md](coverage.md), Effect surfaces.

## Provenance

Layouts, flag bits and enums: the pinned nifxml copy of [nif.md](nif.md)
(`BSEffectShaderProperty`, `AlphaFlags`, `AlphaFunction`,
`Fallout4ShaderPropertyFlags1` and `2`, `EffectShaderControlledVariable`,
`EffectShaderControlledColor`, the field descriptions of Base Color, Base
Color Scale and the falloff fields). The BGEM fields are the reader's
([material.md](material.md)). Recognised from prior knowledge and kept only
as pending rules or refusals: that a sequence's clock starts at its first
key when a cell loads, that soft effects fade near geometry and that
falloff depends on the angle between the view and the surface normal. No
other NIF reader, editor, MIT reference or game executable was opened for
this step.
