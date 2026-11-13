# BGSM/BGEM — port atribuído da MaterialLib (MIT)

reviewed measurement decision autoriza expressamente a referência Material Editor para BGSM/BGEM,
com crédito. Essa decisão substitui a restrição da validação do leitor somente para este
formato. O histórico da pesquisa da validação do leitor está em [material-research.md](material-research.md).
Não é mais correto chamar `fo4-material` de sala limpa: é um **port atribuído
(MIT)** do layout da MaterialLib, com checagens e estruturas Rust próprias.

Referência fixada em `21411873f17454a2442d6785d533499e14c63adb`, clone raso sem checkout
completo em `local-measurement-artifact`. Foram abertos somente
LICENSE e os três arquivos abaixo. Não se executou código da referência.
O [LICENSE](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/LICENSE) é MIT, Copyright (c) 2018 ousnius.
O texto integral e o crédito estão em [THIRD_PARTY.md](../THIRD_PARTY.md), e
no topo do módulo `fo4-material`.

## Procedência dos campos

Todas as linhas abaixo são do SHA fixado; Deserialize é a ordem de leitura.

| Grupo | Arquivo e linhas |
|---|---|
| Magic, versão, tiling, UV, alpha/blend e alpha test | [BaseMaterialFile.cs:179–203](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L179) |
| Z-buffer, SSR, decal, sidedness, occluder e refração | [BaseMaterialFile.cs:205–216](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L205) |
| Environment mapping antes de 10, depth bias desde 10, grayscale e mask writes desde 6 | [BaseMaterialFile.cs:218–233](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L218) |
| Comprimento u32 e terminador de string | [BaseMaterialFile.cs:326–351](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L326) |
| Tuplas de alpha blend e máscaras de escrita | [BaseMaterialFile.cs:354–386](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L354), [500–507](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L500) |
| RGB de três float32 | [BaseMaterialFile.cs:484–496](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L484) |
| BGSM: slots legados/novos, distance-field alpha desde 17 | [BGSM.cs:321–350](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L321) |
| BGSM: editor alpha, translucency desde 8, rim/back/subsurface anteriores | [BGSM.cs:352–371](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L352) |
| BGSM: specular, smoothness, Fresnel, wetness, PBR e porosity desde 9 | [BGSM.cs:373–400](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L373) |
| BGSM: root material, emissão condicional, normals, lum desde 12, adaptive desde 13 | [BGSM.cs:402–427](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L402) |
| BGSM: shadows/glow/hair/tree/facegen/skin/tessellation/skew | [BGSM.cs:429–470](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L429) |
| BGSM: terrain desde 3, inteiro desconhecido somente em 3 | [BGSM.cs:472–487](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGSM.cs#L472) |
| BGEM: slots básicos, extras desde 11, glass desde 21/22 | [BGEM.cs:178–213](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGEM.cs#L178) |
| BGEM: environment mapping desde 10, flags de efeito, cor, falloff, LOD/soft depth | [BGEM.cs:215–238](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGEM.cs#L215) |
| BGEM: emissão desde 11, adaptive desde 15, glow desde 16, PBR desde 20 | [BGEM.cs:240–260](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGEM.cs#L240) |

## Políticas do port

- Faixa fechada **0..=22** na família: 22 é o maior gate explicitamente
  descrito nesses arquivos; os ramos BGSM desde 17 não acrescentam campos
  até esse teto. A referência deixa os comparadores abertos; aqui acima de
  22 é `UnsupportedMaterialVersion`, não aplicação indefinida do último layout.
  Versão 0 tem layout anterior ao campo `SkewSpecularAlpha` introduzido em 1.
  Somente as versões observadas no corpus são conferidas nele; as demais são
  sintéticas. Essa faixa é limite explícito do leitor, não alegação de versões
  de jogo existentes para cada inteiro.
- Campos ausentes por versão/flag são `Option::None`; nenhum SetDefaults da
  referência foi portado. Bool guarda o u8 original e expõe o mesmo teste
  diferente de zero do ReadBoolean. Tiling/mask e blend preservam flags/tuplas
  desconhecidas e as contam como não interpretadas. A tupla (0,6,7), que a
  própria referência chama de BlendMode::Unknown, também conta uma ocorrência
  não interpretada; a identificação dessa tupla não certifica sua semântica.
- Strings são **bytes limitados por u32**, conforme a contrato de leitura do projeto.
  O buffer emprestado conserva inclusive o terminador; `bytes()` remove
  somente um NUL final para lookup. Não se executa ReadChars/decodificação .NET,
  nem se remove NUL interior. Exibição Windows-1252 é uma operação separada,
  falível e orçada; bytes indefinidos não viram substitutos silenciosos.
- Todos os arrays de cor mantêm float32, sem a quantização para RGB8 que a
  referência faz depois de Color.Read. NaN/inf e payload IEEE ficam disponíveis;
  JSON usa `ieee754_bits` em vez de null para valores não finitos.
- O FIXME em BGEM.cs:205 diz que a ordem semântica dos escalares glass pode
  estar errada. O port lê a ordem física, expõe `first`, `middle_v22`, `last`
  e conserva o range cru completo; esses 2/3 escalares são contados como não
  interpretados semanticamente. Não se certificam seus nomes de renderização.
  O inteiro terrain da versão 3 também continua cru e contado.
- Consumo exato: sobra gera `TrailingMaterialData`. Strings têm teto e ranges
  checados antes do acesso. O resultado tipado e as strings emprestadas não
  alocam heap: os bytes pertencem ao asset único, já orçado pelo VFS/chamador.
  Não há carregamento de pacote inteiro nem dependência nova.

Os cinco materiais textuais do Startup foram inspecionados na retomada:
são JSON válidos de schema não coberto integralmente pela referência.
[Estado desse enquadramento](material-json-status.md), com os nomes não
definidos nos contratos e a decisão de não fabricar aliases ou defaults.

## Diagnósticos de componentes (validação do leitor)

A leitura e as licenças acima não mudam. Uninterpreted é desdobrado em
`tile_flags.extra_bits`, `mask_writes.extra_bits`, `alpha_blend.mode`,
`terrain.unknown_v3` e `bgem.glass_scalars`. Valores e opções originais continuam
na estrutura Material. O teste verifica nomes e a soma exata dos componentes.

Relatos primários:
https://forums.nexusmods.com/topic/13113762-need-help-with-transparent-texture-dds-bgsm/
e https://forums.nexusmods.com/topic/13485563-go-ground-objects-that-are-translucent-are-invisible-in-previews/
associam Unknown a efeitos de alpha test/dithering, sem definir seu algoritmo.
Não se reclassifica como Disabled a partir do byte zero.
https://forums.nexusmods.com/topic/13485213-wetness-materials/ descreve
RootMaterialPath como template para wetness; não sustenta a terminação de um
self-parent. A proposta `bgsm_self_wetness_template_stop_v1` permanece inativa;
o teste do grafo exige um AssetDependencyCycle de uma aresta MaterialParent e
conserva o offset. Cadeias e classes são medidas, sem fabricar herança de shader.

## validação do leitor — decisão do revisor e fatores crus

reviewed measurement decision ativa `bgsm_self_wetness_template_stop_v1` exclusivamente no self-parent
MaterialParent de comprimento um. Decisão do revisor: o fechamento é conjunto
de arquivos; a aresta não adiciona bytes. Não é observação do consumo pelo motor.
A cadeia/offset é conservada e a dependência de regra é exposta; ciclos maiores bloqueiam.

`bgsm_blend_disabled_factors_ignored_v1`: enabled==0 não exige semântica dos fatores.
Fonte da decisão: reviewed measurement decision §4. A tupla e alpha-test permanecem tipados na cena;
mode é apenas conveniência. enabled!=0 usa causa contendo a tupla crua até haver
fonte para a numeração. As tabelas NIF/Direct3D não provam a correspondência BGSM.
Pesquisa em documentação pública CK/UESP/Beyond Skyrim não encontrou essa ligação;
nenhuma implementação proibida foi aberta. A antiga interpretação de tuplas do
port continua disponível, mas não é suficiente para certificar os fatores em geral.
https://learn.microsoft.com/en-us/windows/win32/direct3d11/d3d10-graphics-programming-guide-blend-state
sustenta a operação de blend habilitado/desabilitado, não a numeração BGSM.
