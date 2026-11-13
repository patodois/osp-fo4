# Manifest e VFS: contrato explícito

Fonte de requisitos: contrato explícito do projeto, seções 2.2/2.3/2.5, e decisão de medição revisada.
Não se pretende deduzir regras do executável. JSON estrito, raiz relativa ao
manifest ou absoluta, idioma explícito, plugins em ordem de carga e mounts em
prioridade explícita dentro de cada classe de recurso. Com archive invalidation
explicitamente ligada, loose vence archive; entre archives ou entre loose, o
último candidato vence. Nenhum field merge.
Loose declara cada arquivo, sem enumeração de diretórios. Archive declara path e
associação opcional a plugin ativo. Associação inativa não monta.

Paths de arquivos no manifest continuam relativos e ASCII. Nomes internos BA2
são bytes opacos, slash/backslash normalizados e case dobrado somente em ASCII;
bytes >=0x80 permanecem exatos. Componentes vazios, ponto, ponto-ponto, NUL e
dois-pontos são rejeitados. Nome original e bytes do BA2 são preservados. Symlinks só são aceitos se o alvo canônico continuar
na raiz. Dentro da mesma camada, nomes equivalentes com bytes diferentes são
AmbiguousPath. Entre camadas são conflitos resolvidos pela prioridade explícita.

Fingerprint SHA-256 inclui versão do contrato, idioma, ordem de plugins, ordem
ativa de mounts, paths e hashes dos assets (escopos detalhados abaixo). Não usa mtime como ordem. Metadados
são usados apenas para detectar alteração durante I/O. Hash do conteúdo é obtido
por ranges de 64 KiB. Nenhuma leitura de EXE ou rede no runtime.

Defaults de limites são política própria: 256 MiB/payload, 512 MiB por conta,
profundidade 64, 1 milhão de entradas/records, 2 milhões de subrecords, strings
16 KiB, 256 chunks/entrada; o manifest pode diminuí-los ou elevá-los até os tetos absolutos abaixo. Descompressão reserva no
máximo declaração+1 e exige término do stream, checksum e consumo completo.

## Exemplo de perfil artificial

```json
{
  "root": "artificial-data",
  "language": "english",
  "build": "synthetic-1",
  "archive_invalidation": {
    "enabled": true,
    "origin": {"kind": "explicit_profile"}
  },
  "plugins": [
    {"name": "Base.esm", "path": "Base.esm"},
    {"name": "Override.esp", "path": "Override.esp"}
  ],
  "required_masters": ["Base.esm"],
  "excluded": ["HighResolutionTexturePack01.ba2"],
  "mounts": [
    {"kind": "archive", "path": "base.ba2", "plugin": "Base.esm"},
    {"kind": "loose", "base": "mod-assets", "files": ["meshes/a.nif"], "plugin": "Override.esp"}
  ],
  "limits": {"max_payload": 1048576, "max_memory": 8388608}
}
```

`base` de loose é uma raiz relativa opcional dentro da raiz global. Os nomes no
VFS são relativos a ela, permitindo duas camadas com a mesma chave e conteúdos
diferentes. Os arquivos precisam existir: o exemplo não fabrica dados ausentes.
O teste de integração constrói oito plugins, incluindo os sete nomes do perfil
FO4 aprovado, dois archives e loose. O inventário só afirma presença dos masters
exigidos em `required_masters`, não de arquivos descobertos por diretório.

## Escala, precedência e I/O (validação do leitor)

Contas independentes: max_memory (headers/parser/cache de strings),
max_vfs_memory (índice VFS) e max_override_memory (índice de overrides), cada uma
com default 512 MiB e teto absoluto 16 GiB. O manifest pode elevar até esse teto.
Tetos absolutos adicionais: payload 2 GiB, entries 50 milhões, records+groups
100 milhões **por plugin**, subrecords 1 bilhão **por plugin**, strings 16 MiB,
depth 256, chunks 256. Isso é política de segurança, não censo retail.
A soma das contas pode ultrapassar uma conta individual; o relatório mostra cada
uma separada. Valores altos não são uma promessa de disponibilidade de RAM.

Crescimento de Vec usa try_reserve_exact e contabilização conservadora das
realocações. Zlib produz blocos de 32 KiB, reserva só saída efetiva até a declaração
e exige tamanho/checksum/fim/consumo exatos; declaração alta não força alocação.
JSON de configuração tem teto de 1 MiB. Contas não são um medidor de RSS.

- https://themidnightride.moddinglinked.com/lod.html : guia FO4, seção “BA2 Usage Information”: loose vence BA2 **com Archive Invalidation**. Não importamos descoberta/ordenação alfanumérica de archives: paths/ordem continuam entrada explícita.
- decisão de medição revisada exige modelar essa condição: `archive_invalidation` é opcional, com `enabled` booleano e `origin`. Perfil artificial explícito usa `{"kind":"explicit_profile"}`. Perfil derivado de instalação deve registrar `{"kind":"ini","file":"Fallout4Custom.ini","line":7}` (linha começa em 1; arquivo não vazio). A origem é declarada pelo autor do perfil, preservada no summary; o leitor não descobre nem lê INIs. Declarar um mount loose não liga invalidação. Com campo ausente ou disabled, colisão loose/BA2 produz `UnsupportedMountRule` e nenhum winner; chaves sem colisão continuam consultáveis.

## Integridade, fingerprint e relatórios (validação do leitor)

Candidato BA2 com diagnóstico continua no índice como `blocked`, na prioridade
original. Topo bloqueado produz `WinnerUnsupported`, com archive, entrada e
offset; nunca promove o perdedor. Nomes sem chave em bytes válida ficam preservados
como candidatos sem chave, bloqueados e enumerados; **também tornam o VFS
inteiro incompleto** (decisão de medição revisada/R4), pois seu nome utilizável é desconhecido.
Falha estrutural/ausência de archive declarado tem o mesmo efeito:
`chain`, `winner`, `read`, `read_candidate` e `hash` recusam com `VfsIncomplete`,
pois os nomes faltantes são desconhecidos. `resolve` enumera os candidatos
conhecidos sem afirmar winner. O sweep de archives ainda tenta cada chunk
estruturalmente enumerável e conserva diagnósticos por entrada.

Fingerprint `assets` inclui idioma, nomes/ordem explícita de plugins (ativação de
mounts), condição de invalidação, ordens/paths de mounts e hashes de assets; não
abre plugins. Estado incompleto também participa da chave, sem certificar
integridade. Só `sweep --scope records` lê/hasha conteúdo dos plugins e combina
esses hashes no fingerprint `assets+plugins`; `fingerprint_scope` diz qual foi
usado. Se não puder montar o VFS ou construir a ordem, fingerprint pode ser null.
Editar bytes de um plugin altera a chave de records, não a de assets. Inventário
e sweep de archives mantêm fingerprint null quando não constroem esse índice.
I/O de arquivo usa buffer de 64 KiB e posição mantida no lock, sem seek/stat por
campo. Erro de seek ou leitura invalida a posição; a próxima leitura faz seek
absoluto. Metadata é conferida antes/depois da operação de índice/payload/hash;
check_stable/check continuam disponíveis para fronteiras explícitas. Mutação
durante a operação ou entre índice e decode produz FileChanged. Não se promete
proteger toda corrida de filesystem adversarial.

Saída (validação do leitor): resolve o caminho inteiro antes de criar qualquer pasta ou
arquivo. Para componentes ainda inexistentes, encontra o ancestral existente
mais próximo, o canonicaliza e acrescenta o sufixo pendente. A comparação de
(dev, inode) percorre os ancestrais desse caminho **resolvido**, não os do alias
lexical. Assim links para Data e para qualquer subpasta de Data, inclusive com
novos diretórios intermediários, são recusados antes da criação. Identidades
também cobrem aliases/firmlinks. Link quebrado ou erro de metadata é erro de I/O,
sem adivinhar um destino alternativo.

Limite explícito (decisão de medição revisada/Q1): as conferências e operações são por **caminho**,
não relativas a descritores de diretório abertos. Quem puder escrever num
ancestral de `--out` enquanto o comando roda pode, em tese, trocar componentes
entre uma conferência e a operação seguinte e desviar a criação, inclusive para
Data. A reversão também resolve caminhos e pode atingir outro diretório nessa
corrida. As conferências não garantem proteção contra essa alteração concorrente.
O comando destina-se à máquina do próprio usuário, sem outro processo mexendo
na pasta de saída ou nos seus ancestrais durante a execução. O mecanismo foi
mantido nesta etapa; não se afirma uso de operações relativas a descritores.

Os pais ausentes são criados individualmente e registrados para reversão, com
conferência antes/depois de cada criação. Staging é conferido antes/depois de
criá-lo e antes das aberturas; os três arquivos usam create_new, sem truncar.
Publica o diretório por rename e confere novamente a identidade final. O guard
continua armado até essa última conferência: rejeição apaga staging ou publicação
e remove apenas os pais novos ainda vazios, em ordem inversa. Falha do próprio
filesystem ao remover retorna `ReportRollbackFailed`, sem alegar limpeza feita.
O destino de saída não pode existir: pasta existente, mesmo vazia, é recusada
com `ReportDirectoryExists` antes de criar staging ou pais, sem substituir seu
inode, permissões ou atributos. Outros destinos existentes também são recusados;
nenhum arquivo existente é truncado. Isso é atomicidade de criação,
não uma promessa de persistência contra queda de energia ou transação de toda
a varredura. Use diretório novo por execução.

As tags internas de fingerprint passam a `fo4-vfs:assets:7` e `assets+plugins:7`
para invalidar chaves anteriores à identidade em bytes (decisão de medição revisada). O achado
real em `Fallout4 - Voices.ba2`, build 24564252, está detalhado em spec/ba2.md:
três nomes físicos com ED/E1 deixam de invalidar todo o VFS. API str usa os bytes
UTF-8 do argumento, sem conversão implícita para CP1252; `chain_bytes`,
`winner_bytes` e `read_path_bytes` permitem a identidade opaca exata. Relatórios
com itens mostram `key_hex`, bytes originais em hex e exibição com codec rotulado.

## Relatórios agregados (validação do leitor)

Padrão: `items.jsonl` vazio; só `--items` habilita eventos individuais para
inventory, archives, resolve e records. Resolver sem itens conta candidatos,
chaves e erros, sem materializar/hasha cada asset para produzir uma linha.
Os hashes dos arquivos montados continuam no fingerprint. Sweep de archives
continua decodificando cada chunk mesmo sem produzir linhas individuais.

Records agrega todos os envelopes por assinatura de record e de subrecord,
inclusive fora da semântica da fase 1. Summary schema 2 inclui totais globais,
`records_by_plugin` (records/groups/subrecords, scan completo, overrides e
injeções), `override_records`, `unique_forms`, contadores de strings por idioma
e histogramas. Groups vazios também entram quando o scan termina. Uma falha
estrutural mantém o prefixo contado e marca o plugin incomplete; não vira censo
completo por falta de itens.

Diagnósticos são agrupados por (código, arquivo de origem, entrada de archive,
severidade). Cada grupo conserva o primeiro erro completo, contador `count` e
`last_offset`; strings levam no máximo uma amostra de 64 bytes, com ID e idioma.
Não se escreve a mesma amostra para cada uso. Há no máximo 4096 grupos retidos;
omissões são contadas. Histogramas têm no máximo 4096 buckets. Breakdown de
idioma tem limite 4096: excesso gera `LanguageSummaryLimitExceeded`, preserva
os totais globais e conta `language_entries_without_breakdown` explicitamente.

`limits.max_report_bytes`: default 64 MiB, teto 1 GiB. O CLI agora exige ao menos
8 MiB (`ReportBudgetTooSmall` antes de criar saída); reserva 4 MiB para erro
terminal e censo completo, cujo número de buckets é limitado. O restante é
compartilhado por items e grupos de diagnóstico. Medição por serialização em
contador limitado, antes de gravar, inclui JSON e quebras de linha. Se itens ou
diagnósticos não couberem, para de gravá-los, incrementa contadores de omissão e
**continua a enumeração**. Emite um `ReportLimitExceeded`, status 1, e conserva
as contagens no summary; não substitui mais o censo por um resumo mínimo sem
records. `report_bytes` continua sendo a soma dos três arquivos. Um orçamento
anterior de 2 KiB não comportava esse contrato e passa a ser recusado.

Na corrida real, o manifest original é mantido em ordem/escopo, inclusive mounts
repetidos. Só `max_report_bytes` foi reduzido de 512 para 64 MiB numa cópia local
em reports/. `FO4_MIN_FREE_PERCENT=20` habilita guard operacional macOS: consulta
`memory_pressure -Q` antes de iniciar e a cada dois segundos; percentual abaixo
do piso ou falha da consulta encerra o processo com código 3. Usa o percentual
informado pelo macOS nesse comando, em vez da contagem isolada de páginas livres
de vm_stat. É opt-in; uma interrupção por RAM não certifica censo completo.

Relatório distingue região não enumerável após falha estrutural e arquivos
indisponíveis quando não é possível construir a ordem. Items de referências
levam ID original e FormKey remapeada. Offsets de subrecords comprimidos são
explicitamente relativos ao payload descomprimido, junto do span físico do
record. Erros do parser incluem decompressed_offset quando aplicável.

## Perfis explícitos (validação do leitor)

`game` admite somente `fallout4` e `skyrim-se`; ausente significa `fallout4`
para compatibilidade. O perfil escolhe BA2 ou BSA e as regras de header/light dos
plugins; magic, extensão, data de arquivo e ordem de diretório nunca escolhem o
jogo nem a load order. Summary schema 3 inclui `game`. O fingerprint de assets
passa ao contrato 8 e inclui o perfil, além dos demais inputs já documentados.
As regras de precedência, bloqueio de winner e VfsIncomplete valem para BSA.
`Game::fixed_masters()` expõe os nomes fixos e `creation_list()` o nome da lista
CCC; nenhuma dessas funções descobre ou ativa arquivos. O manifest gravado é a
entrada de ordem, também quando preparado para uma corrida real autorizada.

## Sweeps de assets (fase 2)

`nif`, `materials` e `textures` filtram extensões de chaves em bytes e examinam
os winners do VFS, nunca promovem losers por erro de leitura. `shadowed_assets`
conta os candidatos encobertos. O manifest continua sendo a única entrada
de mounts/prioridade. O asset é materializado individualmente; DDS é
decodificado em faixas pequenas. Erros dentro de archives identificam arquivo
físico, entrada, offset do registro e `decompressed_offset` dentro do asset;
a versão em diagnóstico NIF é a BS version do payload. Totais `failure_codes`
continuam contando após o teto de 4096 grupos detalhados; se o próprio
breakdown de código não puder ser retido, `failures_without_code` explicita a
perda. Orçamento de saída permanece 64 MiB.


## Referências de assets e números IEEE (validação do leitor)

Os sweeps nif/materials conferem referências sem ler o payload do destino.
NIF FO4 usa o índice Name de BSLightingShaderProperty/BSEffectShaderProperty
quando termina em .bgsm/.bgem; materiais usam os slots tipados não vazios.
Cada ocorrência é contada; vazios têm contador próprio. Primeiro busca-se o
path normalizado exato. Somente se ele estiver ausente e não começar pela
classe, tenta-se materials/ ou textures/; present_with_class_prefix explicita
esse passo de política do censo. Não se consertam paths absolutos/traversal,
não se acrescenta extensão e não se promove um loser se o winner é bloqueado.
Esta conferência de presença não certifica resolução no executável.

material_references/texture_references separam cited, empty, present, missing,
invalid, blocked, unchecked, unique_unresolved, unresolved_details_omitted e
files_not_fully_checked. cited é a soma dos cinco estados de tentativa;
unchecked conserva a ocorrência se o limite interromper o lookup. Os destinos não
resolvidos são deduplicados por classe/status/bytes (normalizados quando válidos; crus quando
inválidos) num vetor
com reserva falível, teto de contagem e orçamento separado (1/4 de max_memory).
Saem como notices em diagnostics.jsonl, com path_hex, exibição cp1252 rotulada
e count; não estão sujeitos ao teto dos 4096 grupos comuns, mas continuam no
mesmo teto agregado de bytes de relatório. O total diagnostics inclui a
multiplicidade count desses notices agregados. Se esse teto for excedido, a perda
de detalhes e o erro de orçamento continuam explícitos. Contagens de campos
IEEE não finitos são notices por arquivo, e permanecem no summary por atributo.
