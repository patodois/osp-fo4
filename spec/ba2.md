# BA2 PC — fatos e políticas (08/11/2026)

Implementação independente; nenhuma implementação de leitor foi incorporada.

- https://miere.ru/posts/ba2-archive-format/ : LE, sem alinhamento; prefixo de 24 bytes; GNRL de 36; DX10 de 24 com chunks de 24; tamanhos packed/unpacked; packed zero indica raw; payload zlib; offsets absolutos; nomes com comprimento u16. O typo “BDTX” dessa página é corrigido pelo magic BTDX da fonte abaixo. O comprimento u16 delimita os bytes; a política de nomes da validação do leitor abaixo rejeita NUL, inclusive final, conforme decisão de medição revisada, sem procurar além desse comprimento. Hashes, flags e byte desconhecido são preservados sem semântica.
- https://f76.world/modding/ : prefixo BTDX, versões FO4 7/8 conservam a estrutura/chunks/compressão da 1.
- https://baddogskyrim.github.io/BethesdaLibrary/game-specific/starfield/plugins/#33-file-records-chunks : campos isCubemap/tileMode, stride 0x18, sentinel 0xBAADF00D, dimensões/mips/DXGI. Usamos apenas a estrutura FO4 descrita; não a heurística packed==unpacked. Cube 0/1 e tileMode 8 constituem o perfil PC. Outros valores: UnsupportedTextureLayout.

Versões 2/3: UnsupportedBa2Version. A documentação pública consultada declara explicitamente não verificada a seleção do codec v3. Não habilitamos Starfield nem tentamos adivinhar zlib/LZ4 por assinatura. Portanto não há LZ4 executável nesta etapa.

Políticas por entrada, não regras atribuídas ao jogo: nomes em bytes com caixa ASCII normalizada, únicos;
payloads compartilhados não interpretados; ranges fora das tabelas e sentinel exato são estruturais; nomes terminam no
EOF; mip intervals contíguos cobrindo a textura; tamanhos calculados exatos;
limites do chamador. Os índices preservam todos os hashes/flags crus.

O armazenamento dos dados cube em mais de um chunk requer evidência adicional;
ver ressalva em dds.md. Não confundir validação sintética com certificação de corpus.


## Isolamento de falhas (validação do leitor)

Header, tamanho/stride das tabelas, ranges físicos contra arquivo/tabelas,
contagens de segurança e sentinel determinam se é possível indexar o archive.
Erro estrutural nesses campos pode interromper o archive com região não enumerável.
Uma entrada com nome/layout/formato/mips/tamanho derivado incompatíveis recebe
Unsupported* em Entry.diagnostics. Payloads compartilhados/sobrepostos recebem
UnsupportedSharedPayload nas entradas envolvidas; nomes equivalentes marcam as
entradas envolvidas com UnsupportedAmbiguousName. Nenhuma regra de ordenação de
faces ou semântica de compartilhamento é inventada. Bytes crus de nomes, chunks
e metadata continuam no índice; decode_chunk visita chunks estruturais mesmo
quando a interpretação do asset é unsupported. A entrada diagnosticada permanece
bloqueada na sua prioridade do VFS; nome realmente sem chave torna-o incompleto. O sweep contabiliza a entrada e tenta todos os seus chunks.

A materialização DDS apenas concatena intervalos 2D já validados. Cube suportado
é somente o de um chunk; a antiga reorganização inacessível de múltiplos chunks
foi removida. Testes 8×4 verificam width/height, flags, pitch, caps/caps2 e DX10.

O VFS também isola falha estrutural: um archive sem índice utilizável é omitido
com diagnóstico, sem apagar os índices dos demais archives. A conta temporária
da tentativa que falhou é liberada antes de continuar. Isso não afirma que as
entradas daquele archive puderam ser enumeradas; o sweep de archives mantém
non_enumerable_regions para ele.

## Nomes observados no jogo real (validação do leitor, 09/11/2026)

Evidência local autorizada, leitura por range: Steam build 24564252,
`Fallout4 - Voices.ba2`; nomes nos offsets de prefixo u16 2606201573,
2606201624 e 2606230099, com comprimentos 49, 49 e 51. Os bytes altos são
respectivamente ED na posição 41, ED na 41 e E1 na 39 (posições começam em zero).
São três nomes físicos, visitados duas vezes porque esse archive aparece nos
mounts 13 e 40 do manifest do usuário. Evidência completa, hex e exibição Windows-1252 permanecem em relatórios
locais não distribuídos. Nenhum payload de voz foi copiado.

decisão de medição revisada define a nova política: os bytes altos não exigem UTF-8 nem são
convertidos para construir a chave. `Entry.original_name` conserva cada byte;
`key_hex` é a chave serializada em hex, com slash/backslash normalizados e dobra
**somente A–Z**. ED não vira C3 AD; C0 e E0 continuam diferentes. Exibição separada:
`name` com `name_encoding=windows-1252`, usando a tabela factual Microsoft/Unicode
já registrada em spec/strings.md (agora compartilhada em fo4-core). Os cinco
bytes indefinidos de CP1252 continuam aceitos como identidade; se ocorrerem, a
exibição é hex explicitamente rotulado e contada como campo não interpretado,
sem substituição por U+FFFD ou bloqueio da chave válida.

Continuam sem chave nomes vazios, com componentes vazios/`.`/`..`, NUL, dois-pontos
ou além do limite max_string. Erro nomeado por entrada preserva bytes/spans e
marca VFS incompleto; falha estrutural da tabela também continua global. A regra
R4 é mantida para esses casos, não para simples bytes não UTF-8. Os testes geram
archives próprios com os três nomes e os montam duas vezes; só os bytes dos
nomes observados foram usados, sem copiar dados de áudio ou implementação alheia.

## Amostragem de prefixo (validação do leitor)

Não acrescenta fato binário: usa os mesmos offset/packed/unpacked GNRL e zlib
citados acima. `read_prefix` lê até 4096 bytes descomprimidos, por intervalos de
entrada de no máximo 4096, nunca extrai o asset inteiro para amostrar HKX.
Retorna `full_stream_validated=false` quando para antes do tamanho declarado:
não afirma checksum ou tamanho exato do restante não visitado. Quando o prefixo
cobre o fluxo inteiro, exige tamanho/EOF zlib exatos, sem bytes extras. BA2 DX10
e BSA têm erro nomeado nesta API de amostra; seus leitores integrais permanecem.
