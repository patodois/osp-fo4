# STRINGS, DLSTRINGS, ILSTRINGS

- https://github.com/rehan-remade/universal-modder/blob/main/knowledge/techniques/bethesda-plugin-record-format-esm-esp-esl.md : documentação da estrutura, não implementação. Cabeçalho u32 count/u32 dataSize, diretório count × (u32 ID,u32 offset), offsets relativos ao começo dos dados após 8+8*count. STRINGS NUL-terminated; DLSTRINGS e ILSTRINGS têm comprimento u32.
- https://github.com/adamzgz/modtranslator : descrição documental do formato length-prefixed; comprimento inclui NUL e tabelas UTF-8. Não lemos/copiamos o parser.
- https://wiki.beyondskyrim.org/wiki/Arcane_University:File_Localization : três arquivos por plugin/idioma, nomes Plugin_idioma.ext; mesma convenção do desenho aprovado.
- https://baddogskyrim.github.io/BethesdaLibrary/file-formats/plugins/#string-tables : TES4 localized=0x80 controla string ID versus texto embutido; FULL usa STRINGS. Não inferir localização por tamanho do campo.

## Estrutura, codificação e diagnóstico (validação do leitor)

Header/diretório truncados ou dataSize inconsistente falham a estrutura.
Cada ID mantém um range nos bytes originais da tabela. Offset/comprimento/NUL
inválidos, IDs duplicados ou texto inválido geram diagnósticos por ID; outras
entradas continuam disponíveis. Os bytes completos da tabela são retidos sob o
orçamento de payload/memória. Cada diagnóstico guarda offset, comprimento da
região crua disponível e uma amostra de **no máximo 64 bytes**, inclusive quando
não há terminador. Offsets são relativos à tabela descomprimida, nunca vendidos
como posição física dentro do BA2. Offset fora dos dados usa o slot de diretório
como região crua; o erro conserva o offset inválido encontrado.

Fontes dos codecs, consultadas somente como fatos de mapeamento:

- https://raw.githubusercontent.com/TES5Edit/TES5Edit/dev-4.1.6/Core/wbInterface.pas — arquivo `Core/wbInterface.pas`, branch `dev-4.1.6`, tabela de `TwbGameContext.AddGameDefaultLEncodings`, linhas 28515–28528 na consulta: fora de TES3/TES4/FO3/FNV/TES5/Enderal (caso FO4), `en` recebe code page 1252. Não usamos a tabela de fallback de jogos antigos como regra para outros idiomas FO4, nem implementação do xEdit.
- https://raw.githubusercontent.com/MGuffin/xTranslator/HEAD/Data/Fallout4/codepage.txt — arquivo de dados `Data/Fallout4/codepage.txt`: `en=1252`; `fr`, `es`, `esmx`, `de`, `it`, `pl`, `ptbr`, `ru`, `cn`, `ja`, `hu`, `cs`, `ko`, `ar`, `tr`, `th` usam `utf8`. O comentário qualifica a afirmação: “fallout4 seems to encode everything in utf8”. É evidência comunitária com essa ressalva, não garantia Bethesda.
- https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/WINDOWS/CP1252.TXT — tabela pública de dados Microsoft/Unicode, CP1252 para Unicode: base para a tabela de 128 posições altas escrita no leitor. 0x81, 0x8D, 0x8F, 0x90 e 0x9D estão indefinidos e geram `InvalidStringEncoding`. ASCII é identidade; não há substituição desses cinco bytes por controles ou U+FFFD.
- https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-3/#G7404 — definição de UTF-8 e sequências bem formadas. Usamos `str::from_utf8` estrito da biblioteca padrão.

Decisão do projeto: `en` usa Windows-1252; os outros idiomas usam
UTF-8 estrito conforme a tabela do xTranslator. `english` é alias de configuração
para `en`, mantido para os manifests sintéticos anteriores. Na validação do leitor, tenta o nome
explicitamente solicitado `_english` e, se a chave não existir, o alias `_en`,
conforme os nomes observados nas tabelas reais do build 24564252. Chave existente
bloqueada não autoriza fallback. O contador usa o token efetivamente carregado. Outros tokens seguem a decisão UTF-8 do perfil, sem detecção,
fallback, stripping de BOM ou troca de caracteres. A API sem idioma
`StringTable::read` é uma conveniência UTF-8; quem tem idioma deve usar
`read_for_language`, como faz o cache. A política ASCII da validação do leitor foi substituída.

StringCache é retido durante o sweep, com chave (plugin, idioma, tipo de tabela).
Cada tabela é lida, descomprimida, indexada e decodificada uma vez, inclusive
falhas de carga cacheadas. Consultas usam busca binária pelo ID. FULL só conta
como interpretado quando a resolução tem sucesso. O summary informa
`strings_by_language: { idioma: { decoded, undecoded } }`, além dos totais de
entradas enumeradas/decodificadas/não decodificadas. Duplicatas não certificam
texto interpretado. Cada diagnóstico sai uma vez em `diagnostics.jsonl`, com
ID/idioma/offset/comprimento/amostra; usos de FULL podem produzir itens pequenos
`unresolved_string` com o ID, sem repetir os bytes. O orçamento próprio de saída
está em `spec/manifest-vfs.md`; ao esgotá-lo, a varredura é interrompida e não
promete contagens completas.

VFS resolve `Strings/<stem>_<language>.<ext>` seguindo a precedência de assets,
inclusive `WinnerUnsupported`, `VfsIncomplete` e `UnsupportedMountRule`.
MissingStringTable/MissingStringId preservam o contexto.
STRINGS/DLSTRINGS/ILSTRINGS continuam APIs públicas; o sweep inicial interpreta
FULL via STRINGS; diálogo fora da fase 1 permanece envelope.

## Observações de bytes por idioma (validação do leitor)

- https://www.unicode.org/faq/utf_bom.html — seção “Byte Order Mark (BOM) FAQ”, tabela de bytes por encoding: `EF BB BF` é a representação UTF-8 de U+FEFF. Esta é a assinatura observada; não autoriza remover bytes ou trocar o codec.
- decisão do projeto/R5 e Observação exigem contadores, mantendo a decisão de codificação anterior.

Cada tabela conta durante sua única indexação, por entrada do diretório com
fronteiras de texto válidas (comprimento, NUL final, sem NUL embutido), **antes**
de tentar o codec. O summary agrega em `strings_by_language[idioma]`:

- `bom_prefixed`: textos cujos bytes iniciais são exatamente `EF BB BF`, em qualquer idioma. BOM no meio não conta. Inclui texto que depois falha na decodificação; não conta um resto de tabela sem fronteiras comprovadas. Não remove prefixo: em UTF-8 o consumidor ainda recebe U+FEFF; em CP1252 recebe `ï»¿`.
- `english_utf8_multibyte`: somente `en` (e o alias `english`), texto com ao menos um byte >=0x80 e cujo conjunto de bytes, sem o NUL terminador, é UTF-8 estrito bem formado. ASCII, CP1252 isolado inválido como UTF-8, sequências truncadas/overlong/surrogates não contam. Um texto UTF-8 válido pode contar aqui mesmo que contenha um byte indefinido em CP1252 e permaneça não decodificado. Outros idiomas têm esse contador em zero.

São observações das entradas, não de consultas FULL: consultas repetidas ao
cache não aumentam contadores. Duplicatas do diretório contam como entradas
observadas; a rejeição por ID duplicado continua impedindo certificar texto
interpretado. Os três formatos têm a mesma regra, aplicada depois do prefixo
u32 de DLSTRINGS/ILSTRINGS. Um candidato UTF-8 não prova que essa era a codificação
original: o contador serve para examinar divergência no futuro corpus, sem
fallback, novo diagnóstico ou mudança do texto CP1252. Decoded/undecoded e bytes
originais mantêm seus contratos anteriores.

## Censo das tabelas montadas (validação do leitor)

Além do cache usado por FULL, o sweep visita os winners de STRINGS, DLSTRINGS e
ILSTRINGS pertencentes aos plugins ativos explícitos, em todos os idiomas
observados nos nomes montados. O nome deve casar com `Strings/<stem>_<token>.ext`;
token não vazio, ASCII alfanumérico/hífen, até 64 bytes (limite próprio de
relatório; outro nome gera `UnsupportedLocalizationName`). Tabelas já cacheadas
não contam duas vezes; as demais são lidas uma a uma em conta temporária e
liberadas, sem cache de todas as traduções. Não há descoberta de plugins ou
inferência de load order. `strings_by_language` inclui `tables`; os decoded e
undecoded referem-se a entradas das tabelas, não ao número de FULL resolvidos.
Idiomas observados fora da tabela histórica do xTranslator seguem a decisão
explícita do projeto para os demais tokens: UTF-8 estrito, sem palpite adicional.

## Skyrim SE (validação do leitor)

[Tome of xEdit, 18.21.12.2–3](https://tes5edit.github.io/docs/18-whatsnew.html#182112---improved-language-and-codepage-support)
distingue explicitamente Skyrim original de SSE: o original tem codepages por
idioma (russo 1251, francês 1252 etc.), enquanto SSE reconhece `english` em 1252
e usa UTF-8 para os demais. Isso **confirma** a política pedida; o exemplo
Skyrim_russian.STRINGS da mesma página é do Skyrim original e não sustenta mudar
a política SSE. O leitor compartilhado mantém `en`/`english` cp1252, demais
idiomas UTF-8 estrito, sem overrides automáticos, com contadores por idioma e
bytes inválidos retidos. Os três formatos passam pelo BSA/VFS, com a mesma
prioridade explícita e sem fallback de idioma.
