# Referência CPU — validação do leitor

`fo4-render` é original, MIT OR Apache-2.0, sem pacote externo novo. Usa os
leitores existentes, `serde` e o `flate2` já aprovado. Sem GPU ou janela.

## Dados e hipóteses

A lista ordena referências por FormKey, conserva identidade, conjunto de
hipóteses, malha, transformações compostas, textura escolhida e alfa numérico.
Marcadores são omitidos sob a hipótese registrada. Referências bloqueadas
conservam as causas. Citação de modelo não UTF-8, geometria dinâmica e ausência de binding/UV/difusa/mip
produzem erro nomeado; não
há textura branca fabricada. Duas camadas de MSWP simultâneas são erro nomeado,
pois a ordem base/referência não foi estabelecida. Um único mapa BNAM/SNAM faz
substituição por identidade exata; remap de cor permanece cru na Scene e está
fora desta referência que mostra somente a textura.

Fatos de NIF: [nifxml](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml),
`Matrix33`, `NiAVObject`, `BSTriShape`, `NiTriShapeData`, `NiAlphaProperty`,
`AlphaFlags`, `AlphaFunction` e `TestFunction` sustentam campos, índices,
composição de atributos e funções de alfa do NIF. A numeração BGSM/BGEM usa a
hipótese registrada, não esse XML como prova de equivalência. Valores crus não
passam pelo rótulo histórico BlendMode. As transformações e projeções usam as
medições de medição autorizada citadas em `scene.md`. Euler e XSCL ausente são hipóteses.

Texturas: primeiro mip declarado que tenha ambos os lados <=256, array/face
zero e profundidade 1. Arrays/cubemaps como difusa são erro. Limite agregado
96 MiB de RGBA float32 decodificado, além do Budget geral. São preservados mip,
dimensões, path e flag sRGB. Nunca há resampling para inventar um mip ausente.

Blocos armazenados (só o visualizador GPU): com `BuildOptions::blocks`, uma
textura BC1, BC2, BC3, BC4 ou BC5 sem sinal, ou BC7, é guardada como os blocos
que o DDS armazena, sem decodificar: o primeiro nível com ambos os lados <=
`max_dimension` (ou `max_alpha_tested_dimension` quando o draw que a cita tem
teste de alpha), lados múltiplos de 4, e todos os níveis mais grossos
armazenados, contados num orçamento próprio (`max_bytes`) de bytes armazenados.
A textura fica na lista com `rgba` e `mips` vazios e os blocos em
`DrawList::blocks` pelo índice; o layout de cada nível é o do DDS
([dds.md](dds.md)). BC1 declarado sem alpha é marcado `opaque` (a referência
decodifica o quarto índice do modo de três cores como preto opaco); o
terreno marca todos os seus blocos `opaque`. O que não cabe (outro formato,
lados não múltiplos de 4, nível ausente) é decodificado com os limites de
referência acima. A rasterização CPU recusa uma lista com blocos
(`CompressedTextureNotDecoded`) e o snapshot também (`SnapshotBlockTexture`);
sem a opção nada muda: lista, quadros e `summary.json` da referência são
byte a byte os de antes. A GPU decodifica os blocos no hardware; num quadro de
teste com os mesmos níveis da referência, 99,9% dos pixels diferiram de no
máximo 2 passos de 8 bits, sem afirmação de igualdade exata.

## Regras do algoritmo de referência

Estas regras definem esta imagem CPU, não são afirmações sobre estados GPU de
FO4: nearest com repeat U/V, UV*scale+offset, valores de cor decodificados sem
curva sRGB ou iluminação, todos os lados, sem tint/vertex colors/BRDF/fog,
sem dithering ou estados de profundidade especiais do material. Os campos
originais continuam disponíveis. Alpha-test BGSM compara alpha*255 >= ref;
NIF usa a função numérica declarada. O operador implícito do engine BGSM não
foi medido. Opacidade numérica multiplica alpha; fatores ligados usam a hipótese.

Recorte Sutherland-Hodgman em seis planos de câmera, arrays de 16 vértices com
limite checado. Amostras (x+0.5,y+0.5), regra top-left em coordenadas Y para baixo,
sem MSAA/jitter. UV e forward são interpolados por recíproco de forward. Depth
float32 é forward linear positivo; não afirma reproduzir a linha Z de projeção
GPU não medida. Opaque usa LESS com empate pelo primeiro item ordenado.
Transparência vem depois, centróides de triângulos de trás para frente com
ordinal estável no empate. Interseções transparentes não são subdivididas.
Draws listados em `DrawList::overlays` (índices estritamente crescentes; só as
passadas de textura do terreno, [land.md](land.md)) aceitam também a amostra
com profundidade igual à opaca guardada; os demais exigem estritamente menor.
Um overlay nunca troca o índice por pixel. O snapshot recusa listas com
overlays (`SnapshotOverlayDraws`); o visualizador GPU usa GreaterEqual (Z
reverso) só para esses draws na passada de cor.

Índice por pixel identifica a instância contribuinte mais próxima; background
é u32::MAX. Depth é a distância dessa contribuição ou +infinito. Blends não
escrevem o z-buffer opaco. Há um worker de raster efetivo; pedir 1–4 produz os
mesmos bytes e não cria threads adicionais. O processo real tem também o monitor
de RAM. Não se promete equivalência de rasterização com o engine.

## PNG e artefatos

Fatos de framing vêm da [especificação W3C PNG](https://www.w3.org/TR/png-3/):
assinatura, chunks com comprimento big-endian, CRC32 de tipo+dados, IHDR truecolor
RGB8, filtro None por linha, zlib em IDAT e IEND. O escritor é próprio; não copia
código do apêndice. Exporta RGB já composto, sem o alpha do framebuffer ou
metadados variáveis. Índices u32 LE e depth float32 LE têm uma amostra por pixel,
na ordem de linhas do PNG; tamanho e hashes estão no JSON do quadro.

`scene.drawlist` é um protocolo nosso (`F4RL0001`): contagens u32 LE de malhas,
texturas e draws; por malha, contagens de vértices/triângulos, cinco f64 LE por
vértice (posição+UV), três u16 por triângulo; por textura, width/height/mip u32,
sRGB u8 0/1, contagem u32 e RGBA f32 LE. Cada draw contém instance/mesh/texture
u32, 17 f64 (R 3x3, t, scale, UV offset/scale), enabled u8, fatores u32, test
u8/ref u8, função NIF u8 (255 ausente), opacidade f32, opção de flags NIF (tag
0/1, u16 se presente), opção tile flags (tag 0/1, u32 se presente). Não aceita
cauda, contagens fora do limite, índices inválidos ou floats não finitos.
É gerado uma vez do VFS/Scene para medir cada câmera em processo separado,
sem repetir a indexação do jogo. Sua leitura valida ranges antes de reservar.

SCOL com partes só gera draws sob `scol_parts_static_frame_v1`, anunciada no
trabalho. Renderiza as partes ONAM/DATA com suas transformações, conservando o
MODL gerado como alternativa no grafo. Não procura textura substituta para slots
vazios do NIF gerado nem desenha as duas representações juntas. A equivalência
continua sendo hipótese, não um fato deduzido dos slots vazios.

Objetos e subárvores NIF com nome exato `EditorMarker` são omitidas apenas quando a Scene
propaga `nif_editor_marker_named_hidden_v1`. Mantêm-se os dados e diagnósticos no grafo;
a quantidade de subárvores omitidas vai no JSON da coleta. Um shader vazio de
helper não recebe textura fabricada nem invalida silenciosamente o modelo visível.

Na validação do leitor, efeitos explicitamente sem textura podem usar uma amostra constante
calculada dos campos de cor, sob `effect_color_only_v1`. Não há declaração DDS
nessa amostra. A hipótese inclui multiplicação e interpolação dos canais de
vértice habilitados; recursos ativos não cobertos e cor ausente quando habilitada
são erros. O transporte próprio F4RL0003 conserva esses canais opcionais e o
leitor continua aceitando F4RL0001/2; F4RZ0001 é um wrapper zlib sem perda com
tamanho decodificado exato. [nif-render-layouts.md](nif-render-layouts.md) registra o layout.

A regra literal VisibilityEditorMarker:0 é separada e pendente; veja
[helper-visibility.md](helper-visibility.md). A API de comparação pode reter
subárvores explicitamente sem modificar o estado ou os vereditos da Scene.

## Viewer frames that repeat (GPU)

Two causes made the headless GPU frames depend on the run and on the build,
both in the order in which Bevy issues opaque draws, which decides exact
depth ties (the pipelines use a strict depth test, so the first draw at a
tie wins, as in the CPU frame):

- **Run to run.** With indirect drawing (Bevy's default on this GPU) the
  draws of a batch are laid out by GPU preprocessing; without
  `--under-hypotheses` the lobby view differed in 36 to 1,019 pixels of one
  model's two coplanar shapes between runs of one binary (2 of 6 runs).
  The headless cameras now carry `NoIndirectDrawing`: 20 runs of the same
  view then gave one frame.
- **Build to build.** Bevy sorts opaque draws by pipeline, then by the
  slot each material's bind group got; the slots follow the order in which
  materials are prepared, a hash order of asset ids that includes the type
  id, which changes when the source tree is built from another path. Two
  builds of one source in two directories differed in 6 of the 8 test views
  (up to 69,164 pixels). Headless spawning now gives every surface a
  pipeline of its own (`MaterialKey::rank`, specialised in spawn order), so
  each pipeline holds one material and the material slot no longer decides
  anything; the tag pass shares the colour pass's per-surface materials and
  reads each instance's tag from its `MeshTag` instead of one material per
  tag. Exact ties between draws of one surface follow the mesh upload
  order and then the spawn order, both the draw list's.

The windowed viewer is unchanged (indirect drawing, shared pipelines).

Measured with the eight headless test views of the earlier steps: two
builds of one source from two directories now give byte-identical frames
and tag files in every view, with and without `--under-hypotheses`; a
second run of each view repeats the first; twenty runs of the lobby
without hypotheses give one frame and one tag file. Against the commit
before, the frames change only where exact ties now resolve to the first
draw of the list: under hypotheses the test cell 3,859 pixels, the dog
view 68, radius 1 3, the walk 24, the `--path` walk 31 (its walked
positions unchanged), the dog view without actors 70, lobby and vault 0;
without hypotheses the lobby 3,416, the test cell 6,042, radius 1 439, and
the tags (not the colours) of both dog views. A GPU test
(`exact_depth_ties_resolve_by_list_order_in_every_run`, ignored without a
GPU) draws two coplanar quads twice and with their textures swapped; a
test without GPU checks the ranks, materials and tags that spawning gives.

## Effect surfaces

The effect shaders of an instance whose state relies on
`nif_controller_rest_effect_first_key_v1` are drawn by `draw::effect`
([effect-surfaces.md](effect-surfaces.md)), under the pending rules
`effect_surface_texture_base_colour_v1`, `effect_blend_factors_add_v1`,
`effect_blend_after_opaque_back_to_front_v1` and
`effect_soft_lighting_left_out_v1`, which the instance's identity records.
Nothing new enters the draw list format: the base colour times its scale is
multiplied into the mesh's vertex colours (a mesh is shared only by draws
with the same channels and the same colour bits), the base colour's alpha
is the draw's opacity, and the blend factors are the alpha property's (or
the material's). Blended effect surfaces take the existing second pass:
after every opaque triangle, triangles sorted by centroid distance, far
first, the list order breaking ties, no depth write; the GPU viewer puts
them in Bevy's transparent phase (sorted by instance). Every other effect
surface is drawn as before.

Measured after this change: the eight headless test views (with and
without `--under-hypotheses`) and the two views of the newly drawn
placements give byte-identical frames and tag files in two runs of one
build and in a second build of the same commit from another directory (62
files).

## Swap scopes, missing materials and UV fields

Under the rules of [materials.md](materials.md); nothing new enters the draw
list, so the CPU rasterizer and the GPU viewer draw the same list as before.

- **Swap scopes.** The swaps that apply to a base are kept as a stack,
  outermost first (the reference's, the base's, a SCOL part base's). With
  two or more, before a base draws anything, every material its models'
  shaders name is passed through four readings (the outermost swap alone,
  the innermost alone, all from the outermost in, all from the innermost
  out); if they differ for any material the base is refused
  (`UnsupportedRenderSwapScopes readings differ`), and if they agree it is
  drawn with that material only under `mswp_scopes_readings_agree_v1`
  (`readings agree` otherwise). One swap is applied as before.
- **Missing material.** A lighting shader whose material node is missing
  is drawn with its own fields and texture set only when the instance's
  state relies on `nif_missing_material_embedded_shader_v1`.
- **Texture-transform controllers.** For an instance relying on
  `nif_controller_rest_texture_first_key_v1`, a shape whose shader names a
  material and whose UV field a controller drives is refused
  (`UnsettledTextureMaterialField`) where the material's value of that field
  differs from the NIF's.
- **UV fields.** The texture coordinate is stored UV x scale + offset per
  component in both renderers (the CPU per vertex, the GPU per fragment, an
  affine map in both). A textured surface whose offset is not 0 or whose
  scale is not 1 is now drawn only under `uv_scale_then_offset_v1`, which the
  instance's identity records; without `--under-hypotheses` such a surface
  is refused (`UnsupportedUvTransform`).

Measured after this change: the eight headless test views (with and
without `--under-hypotheses`) and two further views of the radius-2 grid
give byte-identical frames and tag files in two runs of one build and in a
second build of the same commit from another directory (31 files each); the
first run also equals the frames of the build before the commit's last
formatting and test-only edits.

## Falloff

An effect surface drawn with falloff ([effect-surfaces.md](effect-surfaces.md),
Falloff) carries `Draw::falloff` (`Falloff`: start and stop angle as
cosines, start and stop opacity) and its mesh keeps `Mesh::normals`
(model-space vertex normals from the nifxml `BSVertexData` normbytes,
`2*b/255-1`); a mesh is shared only by draws that agree on keeping
normals. The reference rasterizer turns each vertex normal by the draw's
rotation and then the camera's into camera axes, interpolates it with the
camera-space position by reciprocal depth (like UV, and linearly through
clipping), and at each sample takes the cosine between the interpolated
normal and the direction from the interpolated point to the eye (the
origin of camera axes), renormalising both. The factor
(`Falloff::factor`: absolute cosine, linear between the two angles,
constant beyond them, the start opacity when both angles are equal and so
are the opacities) multiplies the sample's alpha after the opacity and
before the alpha test and the blend. These are the reference's
conventions under the pending falloff rules, not claims about the game's
arithmetic. A falloff draw whose mesh lacks normals, or whose fields fail
`Falloff::validate`, is refused (`RenderFalloffNormals`,
`UnsupportedRenderFalloff`); the snapshot interchange has no field for
either and refuses the list (`SnapshotFalloffDraws`).

The GPU viewer first left falloff draws out (`GpuScene::falloff_left_out`,
removed). Since the palette step it draws them: the mesh carries its
normals as a vertex attribute (Bevy turns them into world space and
normalises them per vertex), the surface its four falloff fields, and the
fragment shader takes the cosine between the interpolated world normal
and the direction from the interpolated world position to the camera, by
its absolute value, and multiplies the alpha by `Falloff::factor` at the
same step as the CPU. The per-vertex normalisation is the only difference
of method; the synthetic comparison below shows no difference beyond one
8-bit step.

## Palette and colour above 1

An effect surface with a greyscale-to-palette lookup
([effect-surfaces.md](effect-surfaces.md), Palette and colour above 1)
carries `Draw::palette` (`Palette`: the palette's texture index and the
colour and alpha flags). The palette is always decoded texels (the draw
build loads it as the reference does even when the viewer keeps blocks;
the viewer refuses a palette kept as blocks, `RenderPaletteTexture`). Per
sample, after the texel is sampled and before the vertex colour: V is the
draw's opacity times the interpolated vertex alpha (1 without vertex
colours), clamped to 0..1; the colour lookup reads the palette at (texel
green, V) and replaces the texel RGB; the alpha lookup reads it at
(texel alpha, V) and, after the vertex colour and the opacity have been
applied, replaces the alpha; the falloff then multiplies it. Each lookup
is `Texture::sample_clamped`: bilinear on the palette's base level with
the coordinates clamped to 0..1 and texel indices clamped to the edge. The
GPU reads the same level with the surface's own sampler (repeat, linear)
at coordinates kept within the edge texel centres, which reads exactly the
edge texel at and beyond them, as the CPU's clamp does. The snapshot
interchange refuses palette draws (`SnapshotPaletteDraws`).

**Colour above 1.** Every sample's RGB is clamped to 0..1 before the alpha
test and the blend, in both renderers (the CPU explicitly; the GPU in the
shader, and its 8-bit unsigned normalised target would clamp a fragment's
output before blending anyway). Alpha is not clamped (an opacity above 1
without blending still scales the tested alpha). At or below 1 the clamp
changes nothing: the lobby and vault frames, under hypotheses and without,
are byte-identical to the commit before. Effect surfaces whose base colour
times scale exceeds 1 record `effect_colour_clamped_before_blend_v1`; with
SRC_ALPHA factors an additive layer then adds at most its alpha.

**CPU against GPU.** `cpu_and_gpu_frames_agree_on_falloff_palette_colour_and_alpha_clamp` (since renamed, see Vertex alpha of blended lighting surfaces)
(viewer, ignored without a GPU) draws a grey floor, a blended card whose
vertex normals tilt away toward its corners (falloff 1 to 0 between the
cosines 1 and 0.3) and an additive card with both palette lookups (a 2D
palette) and vertex RGB above 1, through `fo4_render::render` and through
`scene::convert` and the headless GPU frame: on this machine coverage
agreement 1.000000 (35,676 pixels drawn by both), mean channel difference
0.03 to 0.06 steps, 95th percentile 0 to 1, largest 1; removing the
falloff, the palette or the colour above 1 changes the CPU frame.

## Environment map, driven material fields and opacity above 1 with blending

**Environment map.** An effect surface that sets an environment map is
drawn as the same surface without the flag: its draw carries no
environment term, so neither renderer changes
(`effect_environment_adds_to_colour_v1`,
`effect_environment_left_out_v1`; [effect-surfaces.md](effect-surfaces.md),
Environment map and driven material fields). The instance records both
rules; nothing in `Draw` names the left-out term.

**Driven material fields.** Where the effect material and the NIF
disagree on a field an effect controller drives, the draw takes the
material's value, as it does for every other field
(`effect_material_over_controller_v1`); the draw is an ordinary effect
draw (opacity, UV offset), which both renderers already draw.

**Opacity above 1 with blending.** A surface with blending enabled whose
opacity is above 1 (the material's Alpha, or the NIF shader's alpha or
base colour alpha) is drawn under `blend_alpha_clamped_before_blend_v1`:
the alpha test compares the sampled alpha times the opacity as it stands
(as `bgsm_opacity_above_one_test_v1` does without blending), and the blend
reads that alpha clamped to 0..1. The reference rasterizer clamps the
source alpha at the start of every blend; the viewer clamps the output
alpha after the test and the zero-alpha discard (its 8-bit unsigned
normalised target would clamp it before blending anyway). At or below 1
both clamps change nothing: the lobby and vault frames, under hypotheses
and without, and a debris view without hypotheses, are byte-identical to
the commit before. `Alpha::validate` now accepts an opacity above 1 with
blending; the draw build gates it (`UnsupportedRenderAlpha` without the
rule, the same text as before).

`blend_opacity_census MANIFEST OUT.txt` (an example of `fo4-unpack`)
reads every winning BGSM and BGEM (9,281 read, 5 unreadable; 34 s, 0.6 GB
peak) and keeps those that blend with an opacity above 1, with the drawn
texture's alpha on the level the reference draws, then every winning NIF
for the shapes that name them. 103 materials: 59 BGSM, all SRC_ALPHA /
INV_SRC_ALPHA (8 with the alpha test), opacities from just above 1 to
above 3, 40 of them above 1.2 and at most 2; 44 BGEM (7 additive), 16 of them over a texture whose alpha is 1
everywhere. On the drawn texture the alpha times the opacity exceeds 1 on
part of the texels for most of them, where a clamp and no clamp differ.
6,019 models name them: 4,407 lighting shapes (on 3,517 the product
exceeds 1 on half or more of the texels, on 155 on none) and 2,101 effect
shapes (1,314 on none). Counts: [blend-opacity-census.txt](blend-opacity-census.txt); unversioned
listing: `OUT.txt.tsv`.

**CPU against GPU.** `cpu_and_gpu_frames_agree_on_falloff_palette_colour_and_alpha_clamp` (since renamed, see Vertex alpha of blended lighting surfaces)
(viewer, ignored without a GPU) adds to the scene of the palette step a
blended (SRC_ALPHA / INV_SRC_ALPHA) card with opacity 2 over a texture
whose alpha runs from 0.2 to 0.9, drawn with a V offset of 0.25: on this
machine (Apple M5, Metal) coverage agreement 1.000000 (35,676 pixels drawn
by both), mean channel difference 0.04 to 0.06 steps, 95th percentile 0
to 1, largest 1; setting its opacity to 1, or its offset to 0, changes the
CPU frame.

## Vertex alpha of blended lighting surfaces

The reference frame draws lighting surfaces without vertex colours (rules
of the reference algorithm, above). A blended ground layer under a debris
pile drew with a hard polygon edge: its shape (36 vertices) sets Shader
Flags 2 Vertex_Colors, not Shader Flags 1 Vertex_Alpha, and stores RGB 1
with alpha 0 to 0.93 at every vertex, the pattern of a decal faded toward
its edges. A census of every lighting shape with a stored vertex alpha
below 1 in the meshes of the installation (actor, LOD and terrain folders
left out; names kept outside the repository): 6,891 with Vertex_Colors
alone and 6,482 with both flags; on the blended ones the open boundary is
faded to 0 (boundary mean at least 0.25 below the interior's, half its
vertices at 0) on 233 of 702 with Vertex_Colors alone and 432 of 986 with
both.

Under `lighting_vertex_alpha_blend_v1` (pending) a blended lighting surface
(material or NIF alpha property) whose shape sets Vertex_Colors and stores a
vertex alpha below 1 multiplies its alpha by the vertex alpha before the
alpha test and the blend (the mesh carries vertex colours `[1, 1, 1, a]`,
channel mask 2): the vertex RGB stays out, and a lighting surface without
blending keeps the frame's policy. Without the rule such a surface is drawn
as before; the frames without hypotheses are unchanged (debris, inn, rock
pile, boxcar and lobby views, frame and instance buffers byte-identical to
the commit before). Under hypotheses the debris layer now fades into the
floor instead of ending in a hard edge.

**CPU against GPU.** The viewer test, renamed
`cpu_and_gpu_frames_agree_on_falloff_palette_colour_alpha_clamp_and_vertex_alpha`,
adds in front of the earlier cards a blended (SRC_ALPHA / INV_SRC_ALPHA)
textured card whose vertex alpha runs from 1 to 0 across it with RGB 1:
on this machine (Apple M5, Metal) coverage agreement 1.000000 (35,676
pixels drawn by both), mean channel difference 0.08 to 0.11 steps, 95th
percentile 1, largest 1; removing its vertex colours changes the CPU frame.

**Texture total of the `render` example.** `--texture-mib N` raises the
reference's decoded texture total (96 MiB) for a dense view; without it
the reference drops the placements past the total (`LimitExceeded`), and
which ones depends on what else is drawn, so a rule that draws more can
make a frame lose an unrelated placement. The frames of this step use
1024 MiB on both sides of each comparison.

## Texture coordinates that are not finite

The draw build refused a whole placement when one vertex of one of its
shapes held a value that is not finite (`RenderNonFiniteVertex`; 683
placements, 2.29 % of the omitted area as their only family). Every model
of those placements was read (local probe; names kept outside the
repository): the values are always texture coordinates, both components a
half-float NaN, never positions; they sit on 3 to 52 vertices of shapes of
hundreds or thousands (building siding, door frames, ground frames, and the
generated models of static collections that hold them), every such vertex
used by a triangle. None of those triangles is degenerate; they cover at
most 2.8 % of their shape's area, mostly below 0.5 %, and most of the
vertices share their position with a vertex whose coordinate is finite, as
the duplicates along a texture seam do. The reader decodes the other
vertices of the same shapes to ordinary values, so this is how the files
store them, not a reader gap.

Under `nif_uv_nonfinite_triangles_left_out_v1` (pending) the triangles that
use such a vertex are left out of the mesh, the vertex keeps an internal
zero coordinate that no remaining triangle reads, and the rest of the
shape is drawn; a position that is not finite stays refused. The CPU
reference and the viewer both draw the draw list's mesh, so they leave out
the same triangles; no shading changes. Without the rule the placement is
refused as before.
