# Inventário de formatos sem leitor (validação do leitor)

Sem semântica binária de HKX/animtextdata. Decisão do projeto §5 define as medidas.
Tamanho é lógico declarado (DX10 inclui os 148 bytes DDS reconstruídos);
quantidade/bytes são de vencedores, sem incluir losers. Nome recusado é contado
separadamente. Falha de tamanho não é byte zero: é sizes_failed e diagnóstico.

HKX: primeiros 16 bytes em hex e origem, amostra limitada aos primeiros 256 para
observar texto ASCII começando por hk_ ou Havok-. Não é identificação/validação
de versão Havok: ausência de texto é explicitamente '<none observed>'. O indicador
full_stream_validated distingue prefixo de fluxo verificado. A árvore soma níveis
1/2 das pastas; o root não é deduzido do nome de arquivo.

animtextdata: nomes/pastas/extensões e bytes, leitura por asset limitada. 'utf8'
exige UTF-8 válido sem controles além de TAB/CR/LF; UTF-16 exige BOM e unidades/
surrogates válidos. Outros conteúdos permanecem binary_or_other_encoding, não
são convertidos. Falha de leitura é unchecked_read_failure, não 'binary'.

NIF: quantidade de ARQUIVOS distintos com cada tipo de controller/sequence/
interpolator e tipos de dados animados explicitamente nomeados pelo leitor. Tag
é a string da tabela NIF, não interpretação de sua animação. Plugins: histograma
de TODOS os tags de envelopes (inclui IDLE/AACT/ANIO/KSSM/STAG se presentes), sem
novo decoder de campos. Assim tipos adicionais permanecem no denominador.

## Skin and actor data read only in part (by measurement)

- `BSSkin::Instance` word 0 (a node link) and its trailing m entries of 12
  bytes (m up to 13; [nif.md](nif.md), Skinned shapes) are kept raw.
- Actor records: the ACBS bytes other than the u32 at 0 and the u16 at 14,
  NPC_ NAM4/NAM6, SOFT, the head parts (PNAM, RACE HEAD) and everything they
  lead to, RACE DATA and GNAM, LVLD/LVLM/LVLF, LVLO bytes 2, 3 and 8 to 11,
  ARMO INDX values, ARMA MOD4/MOD5 and ACHR XRGD are not interpreted
  ([actor.md](actor.md)).
- `.hkx` animation files: 16,388 of 17,508 winning files are refused by the
  container reader at their first section record (their header word at
  byte 60 is `0x00100015`, against `0x00000015` in the 1,120 accepted
  behaviour and project files and in every collision payload); no animation
  object is read.
