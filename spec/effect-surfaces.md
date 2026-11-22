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

## What stays refused, and why

An effect surface of a settled model is refused with
`UnsupportedEffectSurface`, its text naming every term the frame does not
settle, joined by `+`:

- `palette`: greyscale-to-palette colour or alpha (the lookup coordinates
  into the greyscale texture are not given by any source here; the distant
  clouds set the palette alpha flag);
- `feature`: environment map, refraction, glass, glow map, blood, emittance;
- `rgb-falloff`: RGB_Falloff or the material's falloff colour flag
  (nifxml does not describe it; whether the factor dims the colour is not
  settled);
- `falloff-fields`: an opacity outside 0 to 1, or two equal angles with
  different opacities (no factor is defined);
- `falloff-normals`: a shape with falloff without a normal on every
  vertex; a skinned mesh with falloff is refused too
  (`UnsupportedSkinnedFalloff`), its posed normals are not computed;
- `colour-range`: base colour x scale outside 0 to 1 (an emissive value
  above what the unlit frame can show; how the game maps it is unknown;
  the distant clouds have scale 1.5 on a white base colour);
- `vertex-alpha-flag-alone`: vertex alpha below 1 with Vertex_Alpha set and
  Vertex_Colors clear (no rule; no falloff shape has it).

The distant clouds are settled at rest by the degrees rule and are then
refused for `palette+colour-range`.

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
