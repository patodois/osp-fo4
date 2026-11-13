# NIF — dados de cena e geometria da fase 2

Fonte de formato: [niftools/nifxml, nif.xml](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml), consultado como **especificação de dados**, conforme decisão de medição revisada. A cópia consultada tem SHA-256 `752c79786ceb217641fd7e94be4033898c97e9aa38025bf05f2cbee9cffbbf0d`, XML version 0.10.0.0. O [LICENSE](https://github.com/niftools/nifxml/blob/develop/LICENSE) declara GPLv3. Não se incorporou XML, parser, gerador, funções ou código de outro leitor; os fatos de disposição dos campos orientam este leitor próprio MIT OR Apache-2.0. A cópia de consulta fica somente nos relatórios locais ignorados. Não foram lidos nifly/NifSkope/PyNifly/OpenMW/ByroRedux ou outro leitor.

Elementos da especificação que sustentam os campos implementados:

- `Header`, `HeaderString`, `BSStreamHeader`, `Footer`: linha Gamebryo terminada em newline, versão numérica 20.2.0.7, little endian, user version, block count; BS version e export strings u8+bytes (três em 100, quatro em 130); tabela SizedString de tipos, índices u16, tamanhos u32; strings com contagem/comprimento máximo; grupos u32; blocos dimensionados e footer de referências roots. Export strings e strings ficam em bytes, sem codec inferido. Índices PhysX com flag superior não têm interpretação implementada: erro nomeado, não mascaramento.
- `NiObjectNET`, `NiAVObject`, `NiNode`: nome por índice, extra data list e controller, flags, translação, Matrix33 e escala, referência de colisão; filhos e, no SSE, efeitos. Subtipos `BSFadeNode`/`BSLeafAnimNode`, `NiBillboardNode`, `NiSwitchNode`, `BSMultiBoundNode`, `BSOrderedNode`, `BSRangeNode` usam os campos adicionais indicados por suas heranças/definições. Referências -1 são nulas; demais índices são conferidos, sem defaults. Ciclos e profundidade de filhos são limitados.
- `BSTriShape`, `BSSubIndexTriShape`, `BSMeshLODTriShape`, `BSDynamicTriShape`: bounding sphere, referências de skin/shader/alpha, vertex desc, contagens e data size; tri count u16 no SSE/u32 em FO4, vertex count u16, partículas SSE, segmentos/subsegmentos e SSF, três tamanhos LOD, dados dinâmicos Vector4.
- `BSVertexDesc`, `VertexAttribute`, `BSVertexData`, `BSVertexDataSSE`: stride em unidades de 4 bytes, offsets por atributo em nibbles de quatro bytes, atributos a partir do bit 44. FO4 tem posição half ou full conforme bit Full Precision; SSE usa posição float32. UV half, normais/tangentes normalizadas e seus bytes originais, bitangentes divididas, cores byte, eye float. A transformação linear do normbyte para [-1,1] é `2*b/255-1`; os bytes ficam disponíveis. Os campos skin por vértice permanecem ranges crus. UV2/land/instance não descritos nessas estruturas concretas geram `UnsupportedNifVertexAttributes`, nunca são pulados com tamanho presumido. Gaps e W não interpretado ficam crus e contados.
- `NiGeometry`, `MaterialData`, `NiGeometryData`, `NiTriBasedGeomData`, `NiTriShapeData`, `MatchGroup`: a geometria antiga usa arrays presentes conforme flags, CRC, normais/tangentes/UV, limites, consistência, triângulos e grupos de coincidência. Não se introduz skinning ou solver.
- `NiProperty`, `NiShadeProperty`, `BSShaderProperty`, `BSLightingShaderProperty`, `BSEffectShaderProperty`, `BSSPWetnessParams`, `BSShaderTextureSet`, `NiAlphaProperty`: herança e condições por BS version/tipo de shader, flags cruas, floats em bits (inclusive sentinelas), UV, texturas, cor/emissão, alpha/refração, glossiness/smoothness, specular, efeitos/wetness e campos específicos por shader type. Strings de textura SizedString ficam em bytes. Nenhuma conversão de iluminação/PBR é atribuída aos dados.
- `NiExtraData`, `NiStringExtraData`, `NiIntegerExtraData`, `NiFloatExtraData`, `NiBooleanExtraData`, arrays floats/integers/strings, `NiBinaryExtraData`, `NiTextKeyExtraData`, `BSXFlags`, `BSBound`, `BSInvMarker`, `BSBehaviorGraphExtraData`: os campos de cada definição; bytes não entendidos permanecem disponíveis.
- `NiTimeController`, `NiInterpController`, `NiSingleInterpController`, subclasses de transform/alpha/visibilidade e controllers float/color de shader: links, flags, frequência/fase/start/stop, interpolator e controlled variable. Isso lê metadados; não executa animação.

Só se aceitam perfis 20.2.0.7/user 12/BS 130 (FO4), 100 ou o legado 83 documentado no conjunto #SKY# (SSE), selecionados explicitamente pelo manifest. Tamanho/tabela/referência inválidos geram erro com offset. Falha dentro de um bloco preserva o bloco inteiro e seu diagnóstico sem perder a fronteira do próximo. Tipos desconhecidos, `bhk*`/`BSSkin*`/demais skin ficam crus pelo tamanho declarado e entram no histograma por tipo. Sobra de bloco conhecido também é crua/contada, sem certificação de interpretação completa. Geometria com data size zero tem arrays ausentes (`None`), não vértices inventados.

Aritmética e alocações seguem os limites compartilhados; dados emprestados apontam aos bytes do único asset solicitado. Nunca se carrega um archive inteiro. A conversão half é própria, IEEE 754 binary16, com zeros/subnormais/inf/NaN; posições e demais valores não finitos são preservados, e floats opacos de propriedades conservam seus bits.

A leitura real revelou a presença de BS 83 em um NIF do SSE. A versão `V20_2_0_7_SKY` do XML e suas condições sustentam esse caminho de NiNode/NiTriShape; teste sintético vermelho antecedeu a correção. BS 132 de dois arquivos FO4 continua fora do perfil, com erro nomeado.

Extensões de cobertura com a mesma fonte: `BSPackedCombinedSharedGeomDataExtra`, `BSPackedGeomObject`, `BSPackedSharedGeomData`, `BSPackedGeomDataCombined` (contagens, offsets, vertex desc, transformações/bounds; flags desconhecidas cruas); `BSConnectPoint::Parents/Children`, `BSConnectPoint`, `BSEyeCenterExtraData`, `BSBoneLODExtraData`, `BoneLOD`, `BSFurnitureMarker(Node)`, `FurniturePosition`; `BSValueNode`, `BSBlastNode`, `BSDamageStage`, `BSTreeNode`; `NiControllerManager`, `NiControllerSequence`/`NiSequence`/`ControlledBlock`, `NiMultiTargetTransformController`, `NiDefaultAVObjectPalette`/`AVObject`, interpolators de transform/float/point3/bool/timeline e controllers de alpha/FOV/luz. Os interpolators armazenam valores/sentinelas e links, sem avaliar keyframes ou animação.

Política numérica (reviewed measurement decision, substitui a da validação do leitor): NaN/inf são dados IEEE,
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
