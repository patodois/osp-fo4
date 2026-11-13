# Additional record and renderer layouts

The current reviewed verdicts are in [hypotheses.tsv](hypotheses.tsv).
ZYX is supported as a convention; exact arithmetic remains open. Both
EditorMarker name rules remain pending. The optional fixture replay uses
FO4_TRANSFORM_FIXTURE and covers only its recorded scope.

XTEL: fonte primária autorizada `field-measurements/fichas/REFR.XTEL.md`
e `saida/valores-conferidos.json`, chave pairs.REFR.XTEL.examples. No exemplar
publicado de tamanho 36, o primeiro u32 LE reproduz o FormID impresso como Door.
A ficha documenta 968 ocorrências de 36 bytes, três testemunhos xDump e nenhum
layout completo estabelecido. Só esse prefixo de quatro bytes passa a ser lido
para a seleção de interiores. Não se deduz posição, rotação, flags nem o campo
final. O envelope inteiro permanece cru e bloqueante na Scene. Usar o remapeador
normal de masters e comprovar a outra CELL pela associação indexada da REFR.
Fonte por dados/definições, sem abrir fonte ou scripts do xEdit.

A métrica de silhueta e os limiares prospectivos estão em [photo-metrics.md](photo-metrics.md);
são algoritmo próprio, não fato do formato nem ajuste fotográfico.

## Untextured effect fields

[nifxml data definitions](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml)
describe BSEffectShaderProperty Base Color (Color4), Base Color Scale and Source
Texture; Fallout 4 Shader Flags 1 bit 3 enables Vertex Alpha and bit 6 Use
Falloff; Shader Flags 2 bit 5 enables Vertex Colors. Vertex RGBA bytes and these
flags are input facts. The multiplication, perspective interpolation and color
transfer in `effect_color_only_v1` are a pending rendering hypothesis, announced
before implementation. A constant sample buffer is computed from explicit color
fields; it has no DDS declaration and is never a missing-texture fallback.
UV is unused for this constant surface. Active falloff, soft, palette, environment,
refraction and secondary texture effects are refused. Ordinary lighting shaders
with missing diffuse remain errors. Untextured effects receive the named
UnverifiedEffectColorOnly diagnostic even when a named helper later hides them.

Snapshot F4RL0003 is our own private transport: F4RL0002 followed by one presence
byte per mesh and finite little-endian f32 RGBA per vertex when present. Old
versions remain readable. Optional colors are validated before raster access.
