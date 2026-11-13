# BGSM/BGEM: encerramento da pesquisa permitida

Registro histórico da validação do leitor. reviewed measurement decision autorizou depois a MaterialLib MIT com
crédito; o [leitor atribuído da validação do leitor](material.md) substitui a limitação de
implementação descrita aqui. As tentativas abaixo permanecem como histórico.

Pesquisa retomada em 09/11/2026, após o pedido para continuar até resolver ou
esgotar honestamente as possibilidades. O resultado é **ausência de uma
especificação binária suficiente entre as fontes permitidas identificadas**.
Isso não prova que tal documento inexista em toda a internet. Não autoriza
deduzir a ordem dos campos pelo corpus ou por uma implementação proibida.

## Critério para implementar

Um leitor de todos os campos precisa de ordem, tipos serializados/tamanhos,
codificação e enquadramento das strings, representação de flags e condições
por versão. Nomes de propriedades e tipos de uma API não definem esses fatos.
Não foi encontrado documento que fornecesse esse conjunto para BGSM/BGEM.
Nenhuma das referências abaixo foi usada para acrescentar campos ao código.

## Referências efetivamente verificadas na retomada

| Fonte pública | O que foi consultado | Resultado |
|---|---|---|
| [Creation Kit: Material File](https://falloutck.uesp.net/wiki/Material_File) | Wikitext pela API, página 2768 | Finalidade, extensões e ferramentas; sem tabela binária. |
| [API de pesquisa do Creation Kit](https://falloutck.uesp.net/w/api.php?action=query&list=search&srsearch=BGSM%20OR%20BGEM&srnamespace=0%7C4%7C12%7C14&srlimit=50&format=json) | Busca BGSM/BGEM e busca adicional por material | Zero hits BGSM/BGEM nos namespaces indicados. A busca por material retornou Material File, Material Swap e Material Object. |
| [UESP: namespaces](https://en.uesp.net/w/api.php?action=query&meta=siteinfo&siprop=namespaces&format=json) | Inventário seguido de busca em todos os namespaces de conteúdo, mais o principal | Zero hits para BGSM OR BGEM; não se limitou a busca ao namespace principal. |
| [Creation Kit antigo](https://www.creationkit.com/fallout4/index.php?title=Material_Editor) | Página documental | Redirecionou para manutenção em wiki.bethesda.net. O título Material Editor não existe na API do espelho FO4 consultado. |
| [Blender add-on: referência de materiais](https://github.com/POINTYTHRUNDRA654/Blender-add-on/blob/main/knowledge_base/fo4_bsm_format.md) | Somente esse Markdown | Tabela de propriedades e mapeamento para Blender, sem ordem/tamanhos binários ou gates de versão. Os helpers mencionados não foram abertos. |
| [xEdit: TwbBGSMFile.Create](https://github.com/fireundubh/xEditAPIDocs/blob/main/TwbBGSMFile_Create.md) | Somente a documentação da API | Descreve criação/uso de um handler, sem layout binário. Não se abriu sua implementação. |
| [FO4 material analysis report](https://github.com/C-SUDO-1/FO4_MAT_MOD_SHAD/blob/main/FO4_material_analysis_report.md) | Somente o relatório Markdown | Contagens de outro corpus e nomes de propriedades; não especifica serialização. Essas contagens não substituem as nossas varreduras. |
| [Schema de propriedades desse relatório](https://github.com/C-SUDO-1/FO4_MAT_MOD_SHAD/blob/main/schema.json) | JSON declarativo, sem código do leitor | 105 propriedades BGSM e 63 BGEM; chaves Name, Type, IsEnum, IsNullable, CanWrite e DefaultValue. Não há offsets, ordem de leitura, tipos no disco ou gates. Não se usaram seus defaults. |
| [Catálogo Kaitai Struct Formats](https://api.github.com/repos/kaitai-io/kaitai_struct_formats/git/trees/master?recursive=1) | Só metadados/nomes da árvore, não os parsers | Árvore completa retornada, 262 caminhos, nenhum nome BGSM/BGEM. Nenhum `.ksy` desse formato foi localizado pela busca adicional. |
| [Catálogo nifxml](https://api.github.com/repos/niftools/nifxml/git/trees/develop?recursive=1) | Só nomes de arquivos; busca textual no nif.xml já autorizado | Nenhum schema BGSM/BGEM. O XML menciona referências a materiais em shader properties, sem definir o container externo. |

Também se repetiram buscas por `BGSM binary layout`, `BGSM file format wiki`,
`BGEM format specification`, `BGSM field order`, `bgsm.ksy`, `BGSM Kaitai`,
e termos de formato binário em japonês e chinês. Não surgiu tabela binária
adicional. Resultados que remetiam a leitores, espelhos de seus fontes ou
resumos de implementações não foram abertos nem usados. Houve snippets
automáticos desses resultados; eles foram descartados como fonte.

## Evidência local e limites da coleta

Respostas HTTP, URLs exatas, erros, nomes das árvores e documentos consultados
ficam em `local-measurement-artifact`, ignorado pelo Git:

- `material-public-wiki-attempts.json`: APIs, páginas indisponíveis e redirects;
- `material-extra-documentation-attempts.json`: três documentos e consultas wiki;
- `material-catalog-attempts.json`: buscas ampliadas e catálogos sem blobs;
- `material-last-documents-attempts.json`: Material File e schema de propriedades;
- cópias locais dos documentos, respostas das APIs e catálogos completos.

SHA-256 das cópias documentais consultadas, para identificar o conteúdo da consulta documental sem afirmar que os branches públicos sejam imutáveis:

| Cópia local | SHA-256 |
|---|---|
| bgsm-reference.md | `3a9b0bec32487b49106e1c1ca9ec8248a30b0f860d8a0923cc1035a0e8c85447` |
| xedit-bgsm-api.md | `4179382b81c9acc42686202ab8bf3d18fe3a1d0b0383a38565e370fc88f879a8` |
| material-analysis.md | `ee3c0adfa7758defa865ddb6c7d3aa8e3055f0a017d9948181f5b064dee45cbb` |
| material-data-schema.json | `e179e80c5f41986119196c70f3747fe09f723efb96c521e69a453d868dc694c2` |
| falloutck-material-file.json | `086621be3bd53c9471b2aaf1699512742487fcd695a98b9a63ec087c86562e65` |

As consultas HTTP tiveram limites próprios de tamanho e timeout. Não se
baixaram implementações, pacotes adicionais, executáveis ou assets de terceiros.
Os relatórios de pesquisa são históricos; a lista atual de dependências e suas
licenças está em THIRD_PARTY.md.

## Decisão

O item de leitura integral BGSM/BGEM **não foi atendido**. Fica encerrado por
esgotamento documental nesta execução, conforme a alternativa autorizada pelo
último pedido do usuário. `UnverifiedMaterialLayout` e a preservação integral
dos bytes continuam corretos; remover o erro ou fabricar defaults ocultaria
a falta. Uma futura implementação precisa primeiro de uma especificação
binária pública suficiente. Não se afirma suporte completo a materiais.
