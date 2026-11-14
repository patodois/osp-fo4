# Fontes e limites — validação do leitor

Decisão do projeto contém apenas estes novos vereditos: bit IsMarker supported com
uma amostra STAT; ocultação por nome pendente; MODT pendente com evidência de
baixo poder. Os números/analysis dos pacotes não são usados para fechar hipótese.
Separar a antiga regra combinada em bit e nome não introduz comportamento novo.

## MSTT.DATA

Dados autorizados: `field-measurements/layouts/MSTT.DATA.md` e
`saida/valores-conferidos.json`, `pairs.MSTT.DATA.examples[1]`: o byte 00 é
impresso como “On Local Map: False”. A layout abrange 961 ocorrências de 1 byte.
Na conferência das cinco colocações MSTT, os três exemplares correspondentes de
`saida/complementos.parsed.jsonl` imprimem “On Local Map: True”, e os respectivos
payloads crus têm byte01. Fecham-se somente 00=False e 01=True; 02..FF continuam
crus/nomeados, sem booleano por não-zero nem bitmask. O mapa local não altera
modelo e não autoriza executar controladores. As palavras e o dump selecionado
ficam na proveniência privada. URL de identificação da ferramenta:
https://tes5edit.github.io/ (a prova de valor/offset é o dado, não a ferramenta).

## REFR.XLIG parcial

A layout REFR.XLIG e as palavras completas dos exemplos de 16/20/24 bytes na
mesma saída sustentam Fade no offset4 somente nas variantes16/20 e Shadow Depth Bias no
offset12 nas variantes16/20/24, f32 LE.
Nos exemplares 16/20, seus valores não zero distinguem esses offsets dos demais;
a variante24 tem Shadow Depth Bias não zero, mas Fade igual a outros zeros,
portanto seu offset de Fade continua sem prova. Só esses campos são expostos; não se
atribui por ordem os zeros a FOV, End Distance, Near Clip ou Volumetric Intensity.
O envelope continua cru e bloqueante. Valores impressos de seis casas são
conferidos com tolerância de arredondamento 0,5e-6, não substituem os bits.

## Luz já documentada

LIGH.DATA, CELL.XCLL e os demais pares usam as layouts e variantes já sustentadas
em record-layouts.md/tsv. A Scene publica valores, ranges de payload, record de
origem e se o payload é descomprimido. Flags Unknown e tipos sem declaração
ficam lacunas nomeadas; nenhuma curva, unidade fotométrica ou default é deduzido.
Os campos de emissão e os da célula são descritos separadamente, sem sombrear.

## Chaves NIF, somente estrutura

Fonte: https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml
(revisão consultada nesta etapa; definições Key, KeyGroup, QuatKey, KeyType,
NiFloatData, NiPosData, NiBoolData, NiKeyframeData/NiTransformData,
InterpBlendItem, NiBlendInterpolator e derivados Float/Point3).
KeyGroup declara contagem u32, tipo u32 somente se não vazio, tempo e valor;
tipo2 adiciona tangentes, tipo3 três floats TBC, tipo5 não acrescenta campos.
Valores são float, Vector3 ou byte conforme o bloco. Rotação quaternion não
tem tangentes; XYZ exige uma chave declarada e três KeyGroups. Blend FO4 tem
flags/contagem byte e limiar float; bit0 suprime corpo gerenciado, cujo item
mede 17 bytes. Derivados acrescentam valor float/Vector3. Leitura guarda bits,
links e offsets, sem executar controladores nem interpretar sentinelas como pose.
Tipos/flags sem estrutura sustentada geram erro e bloco cru. Classificação
runtime continua bloqueante, mesmo com layout completo.

Ainda no mesmo nifxml: NiParticleSystem em BS100/130 usa NiAVObject seguido
por bound, skin/shader/alpha links, descriptor, quatro ushort de distâncias,
link Data, World Space e array de modificadores. Não se usa o ramo legado
NiGeometry. NiPSysModifierCtlr acrescenta interpolator e Modifier Name ao
prefixo temporal; Emitter acrescenta Visibility Interpolator, Active não tem
corpo adicional nesta versão. Esses três layouts são lidos sem emitir partículas
nem considerar controladores inativos. A necessidade de estado runtime permanece.
