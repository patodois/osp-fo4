# NIF — dados de cena e geometria da fase 2

Fonte de formato: [niftools/nifxml, nif.xml](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml), consultado como **especificação de dados**, conforme decisão do projeto. A cópia consultada tem SHA-256 `752c79786ceb217641fd7e94be4033898c97e9aa38025bf05f2cbee9cffbbf0d`, XML version 0.10.0.0. O [LICENSE](https://github.com/niftools/nifxml/blob/develop/LICENSE) declara GPLv3. Não se incorporou XML, parser, gerador, funções ou código de outro leitor; os fatos de disposição dos campos orientam este leitor próprio MIT OR Apache-2.0. A cópia de consulta fica somente nos relatórios locais ignorados. Não foram lidos nifly/NifSkope/PyNifly/OpenMW/ByroRedux ou outro leitor.

Elementos da especificação que sustentam os campos implementados:

- `Header`, `HeaderString`, `BSStreamHeader`, `Footer`: linha Gamebryo terminada em newline, versão numérica 20.2.0.7, little endian, user version, block count; BS version e export strings u8+bytes (três em 100, quatro em 130); tabela SizedString de tipos, índices u16, tamanhos u32; strings com contagem/comprimento máximo; grupos u32; blocos dimensionados e footer de referências roots. Export strings e strings ficam em bytes, sem codec inferido. Índices PhysX com flag superior não têm interpretação implementada: erro nomeado, não mascaramento.
- `NiObjectNET`, `NiAVObject`, `NiNode`: nome por índice, extra data list e controller, flags, translação, Matrix33 e escala, referência de colisão; filhos e, no SSE, efeitos. Subtipos `BSFadeNode`/`BSLeafAnimNode`, `NiBillboardNode`, `NiSwitchNode`, `BSMultiBoundNode`, `BSOrderedNode`, `BSRangeNode` usam os campos adicionais indicados por suas heranças/definições. Referências -1 são nulas; demais índices são conferidos, sem defaults. Ciclos e profundidade de filhos são limitados.
- `BSTriShape`, `BSSubIndexTriShape`, `BSMeshLODTriShape`, `BSDynamicTriShape`: bounding sphere, referências de skin/shader/alpha, vertex desc, contagens e data size; tri count u16 no SSE/u32 em FO4, vertex count u16, partículas SSE, segmentos/subsegmentos e SSF, três tamanhos LOD, dados dinâmicos Vector4.
- `BSVertexDesc`, `VertexAttribute`, `BSVertexData`, `BSVertexDataSSE`: stride em unidades de 4 bytes, offsets por atributo em nibbles de quatro bytes, atributos a partir do bit 44. FO4 tem posição half ou full conforme bit Full Precision; SSE usa posição float32. UV half, normais/tangentes normalizadas e seus bytes originais, bitangentes divididas, cores byte, eye float. A transformação linear do normbyte para [-1,1] é `2*b/255-1`; os bytes ficam disponíveis. Os campos skin por vértice permanecem ranges crus. UV2/land/instance não descritos nessas estruturas concretas geram `UnsupportedNifVertexAttributes`, nunca são pulados com tamanho presumido. Gaps e W não interpretado ficam crus e contados.
- `NiGeometry`, `MaterialData`, `NiGeometryData`, `NiTriBasedGeomData`, `NiTriShapeData`, `MatchGroup`: a geometria antiga usa arrays presentes conforme flags, CRC, normais/tangentes/UV, limites, consistência, triângulos e grupos de coincidência. Não se introduz skinning ou solver.
- `NiProperty`, `NiShadeProperty`, `BSShaderProperty`, `BSLightingShaderProperty`, `BSEffectShaderProperty`, `BSSPWetnessParams`, `BSShaderTextureSet`, `NiAlphaProperty`: herança e condições por BS version/tipo de shader, flags cruas, floats em bits (inclusive sentinelas), UV, texturas, cor/emissão, alpha/refração, glossiness/smoothness, specular, efeitos/wetness e campos específicos por shader type. Strings de textura SizedString ficam em bytes. Nenhuma conversão de iluminação/PBR é atribuída aos dados.
- `NiExtraData`, `NiStringExtraData`, `NiIntegerExtraData`, `NiFloatExtraData`, `NiBooleanExtraData`, arrays floats/integers/strings, `NiBinaryExtraData`, `NiTextKeyExtraData`, `BSXFlags`, `BSBound`, `BSInvMarker`, `BSBehaviorGraphExtraData`: os campos de cada definição; bytes não entendidos permanecem disponíveis.
- `NiTimeController`, `NiInterpController`, `NiSingleInterpController`, subclasses de transform/alpha/visibilidade e controllers float/color de shader: links, flags, frequência/fase/start/stop, interpolator e controlled variable. Isso lê metadados; não executa animação.

Só se aceitam perfis 20.2.0.7/user 12/BS 130 (FO4), 100 ou o legado 83 documentado no conjunto #SKY# (SSE), selecionados explicitamente pelo manifest. Tamanho/tabela/referência inválidos geram erro com offset. Falha dentro de um bloco preserva o bloco inteiro e seu diagnóstico sem perder a fronteira do próximo. Tipos desconhecidos, os demais `bhk*`, `BSSkin*` e demais skin ficam crus pelo tamanho declarado e entram no histograma por tipo. No perfil FO4 (BS 130), `bhkNPCollisionObject`, `bhkPhysicsSystem` e `bhkRagdollSystem` têm leitor tipado; o ByteArray embutido fica opaco e byte-exato: ver [collision.md](collision.md). Sobra de bloco conhecido também é crua/contada, sem certificação de interpretação completa. Geometria com data size zero tem arrays ausentes (`None`), não vértices inventados.

Aritmética e alocações seguem os limites compartilhados; dados emprestados apontam aos bytes do único asset solicitado. Nunca se carrega um archive inteiro. A conversão half é própria, IEEE 754 binary16, com zeros/subnormais/inf/NaN; posições e demais valores não finitos são preservados, e floats opacos de propriedades conservam seus bits.

A leitura real revelou a presença de BS 83 em um NIF do SSE. A versão `V20_2_0_7_SKY` do XML e suas condições sustentam esse caminho de NiNode/NiTriShape; teste sintético vermelho antecedeu a correção. BS 132 de dois arquivos FO4 continua fora do perfil, com erro nomeado.

Extensões de cobertura com a mesma fonte: `BSPackedCombinedSharedGeomDataExtra`, `BSPackedGeomObject`, `BSPackedSharedGeomData`, `BSPackedGeomDataCombined` (contagens, offsets, vertex desc, transformações/bounds; flags desconhecidas cruas); `BSConnectPoint::Parents/Children`, `BSConnectPoint`, `BSEyeCenterExtraData`, `BSBoneLODExtraData`, `BoneLOD`, `BSFurnitureMarker(Node)`, `FurniturePosition`; `BSValueNode`, `BSBlastNode`, `BSDamageStage`, `BSTreeNode`; `NiControllerManager`, `NiControllerSequence`/`NiSequence`/`ControlledBlock`, `NiMultiTargetTransformController`, `NiDefaultAVObjectPalette`/`AVObject`, interpolators de transform/float/point3/bool/timeline e controllers de alpha/FOV/luz. Os interpolators armazenam valores/sentinelas e links, sem avaliar keyframes ou animação.

Política numérica (decisão do projeto, substitui a da validação do leitor): NaN/inf são dados IEEE,
sem rejeitar o bloco. Cada Mesh expõe NonFiniteCounts por posição, UV, normal,
tangente, bitangente, cor float, eye, dynamic position e campos de metadados.
Nif::non_finite agrega as malhas e os FloatBits dos nós/propriedades lidos.
O CLI emite um notice por arquivo, com as contagens; totais e atributos estão
no summary. Bytes crus não interpretados não são presumidos como floats.
O JSON dos atributos não finitos usa ieee754_bits, sem convertê-los em null.
Tamanho, referência e índice de triângulo continuam erros estruturais.

A checagem de profundidade usa alturas dos descendentes, além da pilha DFS, para não depender da ordem de blocos (incluindo filhos que aparecem antes dos pais). Regressão sintética vermelha/verde em `scene_depth_is_checked_even_when_children_precede_parents`. `NiLightColorController` consome `LightColor` u16 conforme o XML, com regressão vermelha/verde. Cores de `NiTriShapeData` são expostas como float4, sem convertê-las para bytes.

## Quadro parado (validação do leitor)

O schema nifxml já citado sustenta herança e finalidade dos tipos na tabela
[static-blocks.tsv](static-blocks.tsv), compartilhada com o código. Somente essa
definição de dados foi consultada, sem implementação de leitor. NiCollisionObject
e bhkNPCollisionObject/bhkPhysicsSystem são colisão/física; BSMultiBound e seus
volumes não são malhas. BSMultiBoundNode continua NiNode, com filhos visuais.
NiTimeController conserva Next Controller, flags u16, quatro floats de timing e
Target Ptr. Target é ligação de retorno; o relatório separa existência do alvo
no fechamento visual de capacidade de executar o controlador. Ausência de
decoder não se transforma em target_in_visual=false.

Regra `bgsm_name_overrides_texture_set_v1`: testemunhos primários
https://www.reddit.com/r/FalloutMods/comments/nfssnh/ e
https://www.reddit.com/r/FalloutMods/comments/h047vm/ sustentam o material de
Name prevalecer sobre BSShaderTextureSet. Só o shader BSLightingShaderProperty
com Name estritamente resolvido no VFS suprime aquela aresta. BGEM inline não é
extrapolado. Paths absolutos permanecem recusados e as listas originais podem
ser auditadas, com donos mistos ou órfãos preservados como categorias próprias.
Detalhes e testes de política em [scene.md](scene.md).

A lista `Animation.targets` inclui Target e todos os Extra Targets decodificados.
O teste cobre o alvo principal fora do fechamento e um Extra Target dentro:
`target_in_visual` é true. Target/Extra Target e os Ptr de Manager/palette não
expandem o grafo como se fossem propriedade. Tipos com estrutura decodificada,
mas papel ainda unknown (por exemplo BSBoneLODExtraData), produzem
UnsupportedNifBlockSemantics; conhecer os tamanhos não certifica seu papel.

A ampliação da fatia trouxe BSProceduralLightningController,
NiPSysInitialRotSpeedCtlr e NiPSysInitialRotSpeedVarCtlr. A herança declarada no
mesmo nifxml chega a NiTimeController. A coluna layout_prefix da tabela única
marca esse prefixo: Next Controller Ref i32, Flags u16, Frequency/Phase/Start
Time/Stop Time f32, Target Ptr i32, total 26 bytes no perfil FO4 20.2.0.7. O
leitor próprio reutiliza apenas esses fatos comuns; o corpo derivado permanece
Value::Raw com diagnóstico, mesmo vazio. Não se afirma conhecer seu layout
completo. A regressão cobre alvos visuais e cada truncamento do prefixo. Valores
de timing são dados, não amostra de animação; não se adotam defaults do schema.

A tabela cobre também as demais classes cuja herança declarada inclui
NiTimeController, para percorrer os elos Next Controller com a mesma regra.
Quando nenhum tipo derivado acrescenta fields ao prefixo, o marcador
NiTimeController26Full conserva a leitura completa desses 26 bytes; não cria
corpo opaco fictício. NiPSysUpdateCtlr é coberto por regressão encadeada. Nos
outros tipos derivados, o corpo fica Raw e bloqueado. Essa ampliação usa só a
herança e os fields do schema, sem gerar nem copiar implementação de leitor.

## Limites geométricos para a bancada (validação do leitor)

A mesma especificação `nif.xml`, Matrix33 (linhas 1815–1825 da cópia fixada),
define armazenamento column-major OpenGL. NiAVObject (3488–3491) define
Translation/Rotation/Scale uniforme. O cálculo próprio compõe essas transformações
na hierarquia e limita as posições lidas do NIF; não usa OBND nem valores
substitutos. Malhas clássicas seguem a referência Data; compactas usam seu próprio
NiAVObject. Ausência de posição, geometria dinâmica, ciclos, não finitos e limites
de percurso geram erros nomeados. AABB e composição são matemática própria;
nenhum código de outro leitor foi consultado.
A leitura das nove palavras de rotação mudou por medição: ver "Node rotation
words" abaixo.

## Cor de efeitos sem textura (validação do leitor)

Fonte: https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml .
BSEffectShaderProperty declara Source Texture, Base Color (Color4) e Base Color
Scale. Fallout4ShaderPropertyFlags1 bit 3 é Vertex Alpha, bit 6 Use Falloff;
Fallout4ShaderPropertyFlags2 bit 5 é Vertex Colors. São fatos de campos/flags,
não a equação do shader. A equação proposta e seus guardas ficam explicitamente
em `effect_color_only_v1`; ver [nif-render-layouts.md](nif-render-layouts.md).

validação do leitor: grupos de chaves e blend Float/Point3 de FO4 são lidos conforme
[definições nifxml](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml),
com limites e bits preservados. Detalhes e limites em record-light-key-layouts.md.
Leitura estrutural não determina o estado runtime nem libera animação.

validação do leitor também lê o envelope NiParticleSystem BS100/130 e os dois controllers
Emitter/ModifierActive conforme a mesma fonte, preservando os links e flags.
A mudança de herança Bethesda é explícita; nenhuma simulação ou pose de partículas
é produzida.

## Colisão FO4 (leitor tipado)

`bhkNPCollisionObject` (Target Ptr, Flags u16, Data Ref, Body ID u32) e os dois
`bhkSystem` (ByteArray: Data Size u32 + bytes) são lidos só no perfil BS 130,
conforme o mesmo nif.xml. Os bytes do ByteArray não são interpretados. Fatos,
medidas e perguntas abertas em [collision.md](collision.md).

## Node rotation words (by measurement)

`fo4_scene::transform::Transform::from_fields` reads a node's nine rotation
words as the three rows of a matrix R acting on column vectors: the node
maps a child point p to `t + s * R * p`. Until this step it read them as
the columns of that matrix (the column-major order of the schema cited
above), which is R transposed. The two readings place a shape differently
whenever a rotation on its chain (the shape's own transform included) is
not symmetric; the difference also reaches the shape's translation when a
rotated node has a translated child, so depth two is not needed. That a
nine-word block can be stored either way was known in general terms; the
choice below was made only by the counts, which could have gone either way.

`static_rotation_census` (an example of `fo4-unpack`) walks every winning
NIF as the static renderer does (editor-marker subtrees and the outside and
state blocks left out, skinned shapes counted apart), composes both
readings and compares each model's box with the boxes stored in the data.
Output: [static-rotation-census.txt](static-rotation-census.txt); 224,366
files read, 2 not read, 46 s, 0.95 GB peak.

- **Scope.** 1,190,376 shapes reached (23,368 skinned). By the number of
  non-identity rotations on their chain: none 1,156,007, one 7,211, two
  2,976, three 357, four or more 457; 9,164 have a non-symmetric one. 8,651
  shapes move by more than 0.01 between the readings, in 1,789 models
  (1,662 by more than 1 unit); 1,026 of these models are named by a base
  record with a MODL and an OBND (986 moving by more than 1).
- **OBND.** 28,074 models are named by 36,757 such base records. Over the
  records whose model box differs by more than 1 unit between the readings
  or with the root node's own transform left out (four boxes), the OBND
  (six i16, `spec/plugin.md`) is within 1.01 of the row reading's box only
  for 327 records and of the column-major reading's box only for 2; 436
  match both (431 of them only with the root transform left out, where the
  two readings agree) and 372 neither. Of the records whose model moves by
  more than 1 unit, the shapes that move lie inside the OBND (widened by
  1.01) under the row reading only for 343 and under the column-major
  reading only for 32 (528 both, 611 neither). Per model, with the root
  transform applied: in the 4 models whose OBND fits the column-major
  box and not the row box, the row box misses by 1.11 to 12.9 units (a
  damage stage, a loading-screen model); in the 221 models where it is the
  other way round, the column-major box misses by 1.2 to 6,285 units.
- **Skeletons.** The bone nodes of the addon models equal the composed
  world translations of their race skeleton for 111,574 of 118,050 bones
  under the row reading and 4,217 under the column-major one
  ([actor.md](actor.md), Rest pose).
- **Other stored boxes.** 49 files carry a `BSBound` on a root and 2 a
  24-byte `BSMultiBoundAABB`; none of them is in a model whose readings
  differ, so they decide nothing.

The row reading is adopted; the measurement of the running game that could
refute it is `nif_matrix_runtime_row_bridge_v1` (pending). The bounds,
the static draw and the collision frames (`fo4_scene::collision`, whose
Target nodes compose the same way) follow it. Frames of the viewer under
hypotheses against the commit before, in the cases of
[actor.md](actor.md): an interior with clothed humanoids 7 of 921,600
pixels differ, an interior without actors 19, the test cell alone 0, with
`--terrain --around 1` 1,549 and from the creature's pose 4,235,
`--walk --terrain --around 2` 470 and the `--path` walk 63 (its walked
positions unchanged). Without `--under-hypotheses`: the creature's pose
and `--terrain --around 1` 6,026 and 6,741 pixels, the two interiors and
the test cell alone 0. (A first count of 1,019 pixels in the interior
without actors was run-to-run variation: that frame without
`--under-hypotheses` differs between two runs of one binary.)

**Open: the root's own transform.** 1,603 files have a rotated root node
(578 named by a base record). For 431 OBND records the box fits only when
the root's own transform is left out, and for none only when it is
applied. The renderer still applies it; whether the game replaces a root's
transform with the placement (a reading recognised from general knowledge)
is not established by editor bounds and is not registered or applied here.

## Skinned shapes (by measurement)

Read by `fo4_nif::skin` from the raw bytes of the blocks on request; the
block reader still leaves `BSSkin::Instance` and `BSSkin::BoneData` raw, so
static scenes read exactly as before. Unlike the rest of this file, no
schema was consulted: every layout below was established by a census of all
224,366 winning NIF files of one installation (the 2 refused BS 132 files
excepted) with `fo4-unpack`'s `skin_census` example. Counts are files,
shapes, bones or vertices as stated.

- **Skin link.** 23,368 shapes have a non-null Skin link (22,017
  `BSSubIndexTriShape`, 1,351 `BSTriShape`) and every one reaches a block of
  type `BSSkin::Instance`; every one of them has vertex attribute 64 (12
  bytes per vertex); 8 shapes have attribute 64 without a Skin link. The
  shape's own transform is exactly the identity in 23,367 of them; the
  drawing refuses the other one (`ActorSkinnedShapeTransform`) rather than
  guess whether it applies.
- **`BSSkin::Instance`.** Size 16 + 4n + 12m in all 23,368: word 0 a block
  link (always a node: the root block 0 in 6,485, another node in 16,883),
  word 4 a link to a `BSSkin::BoneData` (all), u32 n at 8, n node links (all
  166,208 reach nodes), then u32 m and m entries of 12 bytes (m from 0 to 13;
  0 in 6,933 instances). Word 0 and the m entries are not interpreted.
- **`BSSkin::BoneData`.** Size 4 + 68n in all, with n equal to the
  instance's n in all. Each bone: 4 f32 (a sphere: centre and radius), 9 f32
  (three rows), 3 f32 (translation), 1 f32 (scale). The rows are orthonormal
  within 0.001 for all 166,208 bones and the scale is 1.0 for all.
- **Bone space.** Of 35,828,078 vertex-bone pairs with a non-zero weight,
  99.85 % lie inside the bone's sphere when the vertex is mapped by rows x
  vertex + translation (scale applied), against 35 % and 30 % as stored and
  with the rows applied to a row vector: the 68 bytes hold the transform
  from the shape's skin space to the bone's space, with the rows acting on
  column vectors.
- **Vertex skin bytes.** Four binary16 weights, then four bytes indexing the
  instance's bones: the weights sum to 1 within 0.001 for 19,090,505 of
  19,090,725 vertices (the other 220 are all zero); every non-zero weight's
  index is below n (35,828,078); 21,457 zero weights have a non-zero index;
  the weights are non-increasing in 19,089,816 vertices.

How a skeleton's node transforms compose and how these blocks pose a shape
is in [actor.md](actor.md) (Rest pose and skinning).
