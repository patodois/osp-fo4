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
- `effect_soft_depth_fade_linear_v1` (draw rule, `BuildOptions::soft_effects`,
  the viewer's `--draw-soft-effects`; a blended surface whose shader sets
  Soft_Effect or whose material sets its soft flag): the sample's alpha,
  after the falloff and before the alpha test, times the gap from the
  sample to the opaque depth behind it (camera axis) over the stored soft
  falloff depth, clamped to 0..1 (`Draw::soft_depth`); no fade where
  nothing opaque lies behind. The CPU reads its `opaque_depth`; the window
  copies its depth buffer after Bevy's opaque pass into a buffer every
  material binds (`gpu::SoftDepthBuffer`). An unblended soft surface keeps
  the left-out rule.
- `effect_falloff_view_angle_v1` and its companions: see Falloff below.

The draw build applies the effect path only to instances whose state
relies on `nif_controller_rest_effect_first_key_v1`; every other effect
surface is drawn as before, so frames without hypotheses, and frames under
hypotheses of models without settled effect controllers, are unchanged.

## Falloff

The coverage census of the material step left two families of effect
surfaces as the largest omissions by area: the distant clouds, refused at
rest for their falloff controllers (`falloff` tag; 407 placements, 70.03 %
of the omitted area), and the mist volumes and light beams, refused at the
draw for `falloff+vertex-alpha-one-flag` (10,989 placements, 5.46 %).

### Census

`effect_census` now writes a section `# Falloff` (shapes, placements of
their models, omitted placements by key) over every effect shape in scope
whose drawn fields (the BGEM's where one is named) enable falloff or RGB
falloff, and labels the relation of falloff angle controllers to the
stored field ([effect-census.txt](effect-census.txt)):

- **Angle controllers.** All 146 falloff stop angle controllers in scope
  (the distant clouds; not run by a manager; keys between 10 and 110)
  store exactly the cosine of their first key read as degrees, and none
  stores the first key itself. No start angle controller is in scope. The
  pinned nifxml copy describes the enum options as "Falloff Start Angle
  (degrees)" and "Falloff Stop Angle (degrees)" and the shader fields as
  "At this cosine of angle falloff will be equal to ...". The 144 falloff
  start opacity controllers of the clouds store their first key.
- **Fields.** 1,778 shapes enable falloff or RGB falloff (1,654 through
  the NIF, 124 through a BGEM); 109 NIF shapes and 1 material set RGB
  falloff (155 placements of their models). Every angle but 8
  is the cosine of a whole degree. Start cosine above stop (opaque facing
  the eye) on 1,674 shapes, below (opaque at grazing angles) on 96, equal
  on 8 (both opacities 0). The commonest pairs: 10 to 80 degrees (opacity
  1 to 0), 0 to 70, 20 to 70, 0 to 90, 0 to 80. 509 shapes (9,735
  placements of their models) store a negative cosine (an angle above 90
  degrees). One shape (no placement) stores an opacity below 0.
- **Blend.** SRC_ALPHA / ONE on 1,041 shapes, SRC_ALPHA / INV_SRC_ALPHA on
  737; blending is on everywhere (the alpha test on 50 of them).
- **Normals.** Every vertex has a normal on 1,763 shapes; 15 shapes (no
  placement) have no vertex data.
- **Vertex alpha.** Of the falloff shapes, 1,054 have Vertex_Colors alone
  and a vertex alpha below 1, and none Vertex_Alpha alone. Over every
  effect shape in scope with Vertex_Colors alone and a vertex alpha below
  1 (2,075 shapes), the vertex alpha against the mesh's open boundary
  (edges used by one triangle): of the 1,725 shapes with such a boundary,
  1,026 (37,418 placements of their models) have the boundary's mean
  vertex alpha at least 0.25 below the interior's and half or more of the
  boundary vertices at 0, 284 a lower boundary otherwise, 213 no lower
  boundary; 350 are cards whose every vertex lies on the boundary.
- **Other terms of the same shapes.** 896 falloff shapes also set a
  greyscale-to-palette flag (892 of them with a greyscale texture), 995 a
  colour above 1, 175 a feature (environment map, refraction, ...), 1,115
  the soft flag, 984 two-sidedness.

### Rules

Pending, each refutable alone ([hypotheses.tsv](hypotheses.tsv)):

- `nif_controller_falloff_angle_degrees_v1` (`fo4_scene::rest`, tag
  `falloff-degrees`, always beside `effect-first-key`): a falloff angle
  controller's key and pose values are degrees and the field their cosine;
  under the first-key rule such a field is settled where the stored cosine
  equals the cosine of every driving interpolator's first key.
- `effect_falloff_view_angle_v1` (narrowed from the earlier registration,
  which no code relied on): the angle is the one between the surface
  normal and the direction from the surface point to the eye; the angle
  fields are its cosines.
- `effect_falloff_cosine_absolute_v1`: the cosine is taken by its absolute
  value (a face seen from behind as from the front). The census's negative
  cosines speak partly against it.
- `effect_falloff_linear_in_cosine_v1`: the factor is the start opacity at
  the start cosine, the stop opacity at the stop cosine, linear in the
  cosine between them and constant beyond; with equal angles the
  opacities must be equal and the factor is their value.
- `effect_falloff_multiplies_alpha_v1`: the factor multiplies the alpha
  (texel x base colour alpha x vertex alpha where read) before the alpha
  test and the blend, never the colour.
- `effect_vertex_alpha_under_vertex_colors_v1`: with Vertex_Colors set and
  Vertex_Alpha clear, the vertex alpha multiplies the alpha as when both
  are set.

The draw build gives such a surface a `Falloff` (the four fields) and keeps
its mesh's normals; the reference rasterizer evaluates the factor per
sample ([render.md](render.md), Falloff). The four falloff rules are
recorded together on every instance that draws a falloff; the vertex
alpha rule only where a vertex alpha below 1 is read under Vertex_Colors
alone (with every vertex alpha 1 both readings agree, as before).

## Palette and colour above 1

After the falloff step the distant clouds (458 placements, 74.06 % of the
omitted area, 26.0 % of all area) were refused for `palette+colour-range`,
and `colour-range` alone (1,308 placements), `palette` alone (943) and
`feature` (452) were the other effect refusals.

### Census

`effect_census` now writes a section `# Palette, colour above 1 and
features` over every effect shape in scope whose drawn fields (the BGEM's
where one is named) set a greyscale-to-palette flag, a base colour times
scale above 1, or a feature ([effect-census.txt](effect-census.txt)). It
decodes the level the reference would draw (the first with both sides at
most 256) of the palette and of the source texture; an unversioned
listing (`OUT.txt.textures.tsv`) holds each texture's 8 x 8 grid of
luminance and alpha means.

- **Variants.** 2,302 shapes set a palette flag: colour alone 1,196
  (21,314 placements of their models; 114 through a BGEM), alpha alone 499
  (1,595; 3 through a BGEM, the distant clouds among the NIF ones), both
  607 (5,834). 9 shapes (15 placements) name no greyscale texture, 7 (none
  placed) one that is missing.
- **Palette textures.** BGRA8 on 1,801 shapes (32 x 32 on 1,397), BC3 on
  386 (the clouds' 256 x 512), BC1 with alpha on 99; at most 512 x 512.
  They are two-dimensional: 2,218 shapes have palettes whose rows and
  columns both differ by more than 32 of 255 steps, 67 vary only along V,
  one is constant. On the coarse grid luminance and alpha never fall
  along U nor along V for 640 of the 1,196 colour-only shapes and 268 of
  the 499 alpha-only ones: dark and transparent at U = 0, V = 0, bright
  and opaque toward U = 1, V = 1. The clouds' palette is white with an
  alpha that rises toward the bottom right across a diagonal edge.
- **Source textures.** BC3 on 2,071 shapes, BC1 with alpha on 225, 6
  missing. Nearly grey: RGB spread within 32 steps on 2,132, more on 164.
  Which channel carries the image: on the colour-only shapes the green
  channel's deviation exceeds 32 steps on 773 of 1,196; on the alpha-only
  shapes the alpha's on 365 of 499, while green is flat on many (the
  clouds' source is RGB about 0.8 throughout with alpha 0.1 to 0.7; a glow
  sprite's RGB is 1 throughout).
- **Base colour, blend.** White base colour on most palette shapes, scale
  above 1 on 1,657 of 2,302 (10 or more on 345); additive (SRC_ALPHA, ONE)
  on 1,140, (SRC_ALPHA, INV_SRC_ALPHA) on 869. The commonest combination:
  colour palette, colour above 1, falloff, Vertex_Colors alone (260
  shapes, 9,555 placements).
- **Colour above 1.** 2,615 effect shapes; mostly white base colours
  with scale 2 to 10, additive (1,607 shapes), (SRC_ALPHA,
  INV_SRC_ALPHA) on 671, without blending on 106.
- **Features.** Environment map on 392 shapes (6,301 placements; 308
  through the NIF flag, 84 through a BGEM; with an environment texture and
  a scale of 0 to 100), refraction 19, fire refraction 10; no glass, glow
  map, PBR specular, blood or emittance colour on a shape in scope.

### Rules

Pending ([hypotheses.tsv](hypotheses.tsv)), each refutable alone:

- `effect_palette_sampled_clamped_v1`: the palette is read bilinearly at
  the base level the frame loads, coordinates clamped to 0..1 and texel
  indices to its edges.
- `effect_palette_v_from_alpha_v1`: its vertical coordinate is the alpha
  before the palette, the base colour alpha times the vertex alpha where
  read, clamped.
- `effect_palette_colour_u_green_v1` and
  `effect_palette_colour_replaces_rgb_v1`: the colour lookup's horizontal
  coordinate is the texel's green, and the palette RGB takes the texel
  RGB's place in the colour equation (times base colour, scale and vertex
  RGB).
- `effect_palette_alpha_u_alpha_v1` and
  `effect_palette_alpha_replaces_alpha_v1`: the alpha lookup's horizontal
  coordinate is the texel's alpha, and the palette alpha takes the place
  of the alpha before the falloff.
- `effect_colour_clamped_before_blend_v1`: a colour above 1 is clamped
  per channel before the test and the blend (the frame's range; alpha is
  not clamped).

The draw build gives a palette surface a `Palette` (the palette texture,
decoded even when the viewer keeps blocks, and the two variant flags) and
records the sampling and V rules with the variant's pair; the clamp rule
where base colour times scale exceeds 1 in a channel. The reference
rasterizer and the viewer apply them per sample ([render.md](render.md),
Palette and colour above 1).

## Environment map and driven material fields

After the palette step, effect surfaces that set an environment map
(`environment-map`, 453 placements, 1.63 % of the omitted area) and
surfaces whose material disagrees with the NIF on a field a controller
drives (`UnsettledEffectMaterialField`, variables 5 and 8: alpha and V
offset; 635 placements, 0.38 % of the omitted area) were the effect
refusals left among sole reasons.

### Census

`effect_census` now writes two more sections
([effect-census.txt](effect-census.txt)):

- **Environment map** (shapes in scope whose drawn fields, the BGEM's where
  one is named, set environment mapping): 394 shapes (310 through the NIF
  flag, 84 through a BGEM; 17 of the BGEM ones set it in the material
  while the NIF flag is clear). Every environment slot names a cube map
  with 6 faces that the texture reader decodes (BGRA8 128 x 128 with 8
  levels on 363 shapes, BC1 32 x 32 on 31). The mask slot is empty on
  314 shapes; the 80 masks are BC1, BC3 or BC5 with very different
  channel use (grey masks, a mask in alpha, two-channel BC5 maps). 317
  shapes also name a normal map. The scale (the NIF's Environment Map
  Scale or the material's environment mapping mask scale) is never 0:
  below 0.1 on 34 shapes, 0.1 to 0.25 on 50, above 0.25 and below 1 on
  99, exactly 1 on 102, above 1 up to 5 on 78, above 5 (up to 100) on 31.
  Blend: SRC_ALPHA / INV_SRC_ALPHA on 291 shapes, additive (SRC_ALPHA /
  ONE) on 102, one multiplicative (ZERO / SRC_COLOR); many also carry a
  palette, a falloff or a colour above 1 drawn under their rules.
- **Controlled fields under material swaps**: the material swaps (MSWP)
  whose source material is the BGEM of a shape with effect controllers,
  with the replacement's value of each driven field against the NIF's
  stored value (which the first-key rule settled). 88 swap pairs; of the
  driven-field comparisons, 31 differ: 12 alpha fields, every one below
  the first key and every one driven by a controller whose keys hold one
  value (7 from pairs that change nothing but the alpha, 4 from pairs
  that change the alpha and the base colour scale), 9 U and 9 V offsets
  (0.005 and 0.035 against 0, by controllers that scroll), 1 base colour
  scale. Without a swap, the NIF and its own BGEM differ on 6 of 350
  driven fields (5 base colour scales, 1 base colour). An unversioned
  listing (`OUT.txt.swaps.tsv`) names the swaps and materials.

### Rules

Pending ([hypotheses.tsv](hypotheses.tsv)):

- `effect_environment_adds_to_colour_v1`: the environment term only adds
  light to the colour; it changes neither the alpha, the coverage, the
  test and blend, nor the other colour terms.
- `effect_environment_left_out_v1`: this unlit frame leaves the term out
  and draws the surface with its other terms; under the first rule that
  is a lower bound of the colour. Chosen over drawing the reflection: the
  cube maps decode, but a reflection needs at least five conventions no
  source gives (the world-to-cube axes, the reflection's normal, the mask
  channel, the weight of the scale, the combination with the base
  colour), and 317 of the 394 shapes name a normal map, which this frame
  never reads, so a reflection taken from vertex normals would contradict
  their own data. The frame already draws lighting-shader surfaces from
  their diffuse alone, without their environment maps.
- `effect_material_over_controller_v1`: where the material (as named, or
  as a swap replaces it) and the NIF disagree on a driven field, the
  material's value is drawn. The swaps that change only a driven alpha
  held constant by its controller would draw exactly as the unswapped
  model under the opposite reading; the opposite reading (a running
  controller writes its first key) is the first-key rule's own premise,
  so the two rules pull against each other there and a measurement
  decides.

A field without a pairing to a material field still refuses the surface
(`UnsettledEffectMaterialField`); refraction, fire refraction, temp
refraction, glass, glow map, PBR specular, blood and emittance colour stay
refused by name.

## What stays refused, and why

An effect surface of a settled model is refused with
`UnsupportedEffectSurface`, its text naming every term the frame does not
settle, joined by `+`:

- `palette-texture`: a greyscale-to-palette flag with an empty greyscale
  slot (no palette to read; before the palette step every palette surface
  was refused as `palette`);
- each feature by name: `refraction`, `fire-refraction`,
  `temp-refraction`, `glass`, `glow-map`, `pbr-specular`, `blood`,
  `emittance-colour` (before: `feature`). The environment map is no
  longer refused: it is drawn without its term under
  `effect_environment_left_out_v1` (Environment map and driven material
  fields above);
- `rgb-falloff`: RGB_Falloff or the material's falloff colour flag
  (nifxml does not describe it; whether the factor dims the colour is not
  settled);
- `falloff-fields`: an opacity outside 0 to 1, or two equal angles with
  different opacities (no factor is defined);
- `falloff-normals`: a shape with falloff without a normal on every
  vertex; a skinned mesh with falloff is refused too
  (`UnsupportedSkinnedFalloff`), its posed normals are not computed;
- `colour-negative`: base colour x scale below 0 or not finite (above 1
  is drawn under the clamp rule; before: `colour-range` for both);
- `vertex-alpha-flag-alone`: vertex alpha below 1 with Vertex_Alpha set and
  Vertex_Colors clear (no rule; no falloff shape has it).

The distant clouds are settled at rest by the degrees rule and are drawn
since the palette step under its rules (before: refused for
`palette+colour-range`).

## Coverage

See [coverage.md](coverage.md), Effect surfaces, Falloff and vertex alpha,
Palette and colour above 1, and Environment map, driven material fields
and blended opacity above 1.

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
this step. For the falloff step, further: the enum option descriptions of
`EffectShaderControlledVariable` 1 and 2 ("(degrees)"), the field
descriptions of the four falloff fields and of the opacities ("Alpha
falloff multiplier"), and the `BSVertexData` normal layout already in
[nif.md](nif.md). Recognised from prior knowledge and kept as pending
rules: that the falloff angle is taken against the direction to the eye,
that two-sided surfaces turn the normal toward the eye (the absolute
cosine), and that the effect shader reads vertex alpha whenever vertex
colours are on. Nothing else was opened. For the palette step: the
nifxml description of the Greyscale Texture field ("used as palette")
and of Base Color Scale; recognised from prior knowledge and kept as
pending rules: that the palette's second coordinate is the effect's alpha
(base colour alpha times vertex alpha), that the colour lookup reads the
texel's green channel, that the alpha lookup reads the texel's alpha and
that its result replaces the alpha, that the palette colour is then tinted
by the base colour, its scale and the vertex colour. The clamp before the
blend is a policy of this frame. No MIT reference, other reader, editor
or game executable was opened. For the environment map
and driven material fields step: the nifxml field names of the
environment slots and scale, the material reader's fields, and the DDS
cube layout already sourced in [dds.md](dds.md). Recognised from prior
knowledge and kept as pending rules: that an environment map is a
reflection added to the colour (and, unregistered because the frame does
not draw it, that it is read along a reflection of the view direction
about the normal, weighted by a mask channel and the scale). The choice of
the material over the controller rests on the census of swaps, not on
prior knowledge. No MIT reference, other reader, editor or game executable
was opened.
