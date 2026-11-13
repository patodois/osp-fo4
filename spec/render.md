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
