# Plugins FO4 — fatos utilizados

Leitor e builders próprios. Nenhum parser alheio foi traduzido ou incorporado.

- https://tes5edit.github.io/fopdoc/Fallout4/Records.html : header de record de 24 bytes (tag, dataSize, flags, FormID, revision, version u16, unknown u16), subrecords tag+u16, TES4 com FormID zero e payload comprimido com tamanho u32 seguido de zlib. Flags master/light.
- https://tes5edit.github.io/fopdoc/FalloutNV/Records/TES4.html : HEDR contém float version, u32 número de records+groups sem o TES4 e u32 próximo ID; MAST é filename NUL seguido de DATA de oito bytes. Layout compartilhado confirmado pelas definições FO4 abaixo; não adotamos a versão HEDR de FNV.
- https://tes5edit.github.io/fopdoc/Fallout3/Groups.html : GRUP de 24 bytes, tamanho inclui header, label de quatro bytes dependente de kind; 1 world children, 6 cell children, 8 persistent, 9 temporary, 10 distant. Cauda crua preservada. Sem impor a ordem dos grupos FO3 a FO4.
- https://baddogskyrim.github.io/BethesdaLibrary/file-formats/plugins/ : XXXX tem tamanho 4 e substitui o u16 do próximo campo por u32; flags compressed=0x40000, localized=0x80; IDs no disco relativos a MAST. A descrição de faixa light dessa fonte foi substituída pelo CHANGELOG abaixo. Último override é um record inteiro, sem mesclar campos.
- https://raw.githubusercontent.com/TES5Edit/TES5Edit/dev-4.1.6/Core/wbDefinitionsFO4.pas : consultadas somente as **definições declarativas** de TES4/CELL/REFR/WRLD/STAT/SCOL/MSTT/ACTI/DOOR/FURN/LIGH/LGTM/MSWP/TXST e do model. TES4 bits ESM=0, localized=7, ESL=9, update=20. REF initially disabled=11; NAME FormID; DATA posição xyz/rotação xyz; XESP FormID, flags byte, três bytes reservados. CELL DATA u16; LTMP/água/location FormIDs. WRLD DATA byte, WNAM/CNAM/NAM2/LTMP FormIDs. MODL string e MODS FormID material swap. MSWP BNAM/SNAM/FNAM strings, CNAM float. TX00–TX07/MNAM strings de textura/material. LIGH DATA de 64 bytes: tempo i32, raio u32, cor de quatro bytes, flags u32, parâmetros float, value u32 e weight float; não convertemos fotometria. LGTM WGDR é FormID. Demais campos preservados.
- https://raw.githubusercontent.com/TES5Edit/TES5Edit/dev-4.1.6/Core/wbDefinitionsCommon.pas : HEDR, OBND seis i16, XCLC coordenadas i32 e flags u32; SCOL DATA é array de placements (posição xyz, rotação xyz, escala float), referenciado por ONAM no record FO4. Esses fatos de estrutura são usados, não as callbacks/funções do xEdit.
- https://en.uesp.net/wiki/Skyrim_Mod:Mod_File_Format : referência de formato, inacessível na consulta documentada (HTTP 403); **não usada como evidência lida**.

Políticas do perfil: HEDR 0.95/1.0; update-plugin recusado por
UnsupportedPluginFlags; regras de extensão FO4 sustentadas abaixo. Full slots 00–FD, até 4096
slots light FE:xxx:yyy; identidade persistente é plugin definidor + object ID.
Um ID em disco usa o índice de MAST ou o índice self (quantidade de masters),
inclusive quando o alvo é light. FE é slot de runtime, não atalho para MAST.
Flag deleted=0x20 remove o winner da visão ativa, sem apagar seu histórico;
disabled é independente. Nenhum merge parcial ou default para campos ausentes.

Semântica inicial **parcial**, restrita aos 13 tipos acima; MODL é campo de model,
não um tipo de record inventado. FULL só é interpretado com o contexto localized
do TES4. Campo desconhecido continua em payload/subrecord range e conta como Raw;
record fora da lista fica integralmente envelope, mesmo se tiver EDID/FULL.
Revision, unknown, bits não interpretados e grupos desconhecidos são mantidos.
Sem certificação de iluminação/VM/quests/precombine/scene nesta etapa.

LGTM DATA e CELL XCLL: interpretamos somente o prefixo documentado de 40 bytes
(cores ambient/directional/fog, fog near/far, duas rotações i32, fade/clip/power).
O restante mantém range cru. Esse campo é contado como parcialmente interpretado
e permanece no denominador de não interpretados; não se afirma iluminação completa.

## Correções da revisão (validação do leitor)

- https://github.com/Ortham/esplugin/blob/master/CHANGELOG.md : somente o CHANGELOG foi consultado, nunca a implementação. As entradas 3.3.0 e 4.1.1 sustentam os limites FO4 light: HEDR <1.0 usa 0x800–0xFFF; HEDR >=1.0 usa 0x001–0xFFF. Tanto remap quanto runtime_id aplicam a versão do plugin **definidor**, inclusive quando referenciado por outro plugin.
- Requisitos explícitos de decisão de medição revisada: master-first, injected records válidos, divergência de contagem HEDR sem falha estrutural. Isso não depende de supor conteúdo retail. Injected mantém o namespace do master e a origem física do plugin que o criou; overrides posteriores continuam a cadeia. O índice guarda números de plugin/offset/length e IDs compactos; nomes e paths são reconstruídos pelos headers, sem repetição por record.
- A definição declarativa FO4 de LIGH citada acima sustenta os 64 bytes completos. Nenhuma regra para campos ausentes de variantes 48/52/56/60 foi estabelecida: essas variantes agora são Raw, sem preenchimento ou interpretação de prefixos opcionais.
- As definições dos placed references em wbDefinitionsFO4.pas / sigReferences sustentam bit 11 como Initially Disabled em REFR, ACHR, PGRE, PMIS, PHZD, PARW, PBAR, PBEA, PCON e PFLA. Esses tipos permanecem envelopes se fora da fase 1; a flag é contada independentemente da semântica do payload.

## Proveniência e extensões (validação do leitor)

- https://raw.githubusercontent.com/Ortham/libloadorder/master/doc/src/plugin_types.md — **somente documentação**, seção “Skyrim Special Edition, Skyrim VR, Fallout 4 and Fallout 4 VR”, linhas 15–22 consultadas. `.esm` ou flag master tornam o plugin master; `.esl` ou flags master+light tornam light master; `.esp` com flag light é light nonmaster. Essa seção específica FO4 sustenta aplicar master por `.esm`/`.esl` e light por `.esl`, independentemente da presença dos bits. A regra usa somente essa documentação; nenhuma implementação do libloadorder foi usada como evidência ou incorporada.

`Header::is_master/is_light` calculam o tipo efetivo a partir dessas regras;
`Header.flags` preserva os bits originais. A partição master-first, os slots e a
validação/remapeamento light usam o tipo efetivo. Correção R2 (decisão de medição revisada): o
bit light isolado **não** torna `.esm` light; light master exige `.esl` ou os
dois bits master+light. Light nonmaster exige `.esp` e bit light. Matriz
sintética conferida em slots/runtime ID e faixa de objeto, com flags cruas
preservadas:

| Nome | Flags | Master efetivo | Light efetivo |
|---|---|---|---|
| A.esm | 0 | sim | não |
| B.esm | 0x200 | sim | não |
| C.esm | 0x201 | sim | sim |
| D.esl | 0 | sim | sim |
| E.esp | 0x1 | sim | não |
| F.esp | 0x201 | sim | sim |
| G.esp | 0x200 | não | sim |

Fonte factual é a mesma seção FO4 de `doc/src/plugin_types.md` citada acima,
consultada novamente somente como documentação. `UnverifiedEslExtensionRule`
foi removido; `UnverifiedMasterCountRule` acima de 253 MAST permanece notice:
o byte de índice é validado contra os masters representáveis, sem inventar um
limite estrutural de contagem do header.

`LoadOrder` mantém headers e slots privados, com acesso somente por slice
imutável. Não se pode desalinhar headers/masters/names após a construção.
`Overrides::read_winner` escolhe a fonte no catálogo pelo índice do plugin do
Override; verifica o nome contra o header. Catálogo curto gera
`SourceUnavailable`, fonte trocada `SourceMismatch`, ambos com origem/offset.

Conclusão de índice é explícita via `mark_complete`, chamada pelo CLI somente
após percorrer integralmente o plugin sem erro de inserção no índice. Um record
sem predecessor no namespace de outro plugin só é `injected` se esse definidor
estiver completamente indexado; senão `BaseUnavailable`, sem inserir o record.
Base existente conhecida pode receber override mesmo após outro erro de scan.
O summary conta definições novas injetadas (`injected_records`), não cada
subsequente override que herda essa marca. Falha de texto não torna o índice de
identidades incompleto se todos os envelopes foram indexados.

Contagem de records+groups e subrecords é por plugin, não pela load order toda.
HEDR count_matches=false retorna stats completos; o CLI emite divergência e
mantém non_enumerable_regions=0 se a estrutura inteira foi percorrida.

## Perfil Skyrim Special Edition (validação do leitor)

Seleção por `game: skyrim-se`, sem inferência pelos bytes. `Header::read` conserva
seu contrato FO4; `read_for_game` recebe o perfil explícito. Headers de jogos
diferentes numa mesma load order geram `MixedGameLoadOrder`.

- [Tome of xEdit, 18.13.1](https://tes5edit.github.io/docs/18-whatsnew.html#18131---extended-formid-range-in-skyrim-special-edition): HEDR SSE 1.71 amplia IDs light para 001–7FF; antes disso o intervalo começa em 800. O teto continua FFF. A mesma seção alerta que quem referencia esses novos IDs também precisa de HEDR 1.71. O remapeamento valida o definidor e esse requisito do referenciador. FO4 mantém 0.95/1.0 e seu limiar próprio.
- [Documentação TES4/HEDR](https://tes5edit.github.io/fopdoc/FalloutNV/Records/TES4.html), tabela HEDR: versão float e referência a 1.7 em Update.esm. A aceitação SSE está restrita a 1.7/1.71; outras versões geram `UnsupportedPluginVersion`, sem coerção para o perfil FO4.
- [Discussão de formato, zilav e hishutup](https://stepmodifications.org/forum/topic/11699-about-file-header/): SSE moderno usa form version 44, mas Skyrim.esm conserva records de 14 a 44. Não se rejeitam envelopes antigos nem se escolhe layout de payload só pela versão. `Header.form_version` e `Record.version` preservam u16, e os histogramas de records já expõem suas versões.
- [Documentação de tipos efetivos](https://raw.githubusercontent.com/Ortham/libloadorder/master/doc/src/plugin_types.md), seção SSE/VR/FO4: mesmas regras `.esm`, `.esl`, flags master/light já descritas acima. Foi consultada apenas a documentação.
- [Step: arquivos do SSE](https://stepmodifications.org/wiki/Guide:Skyrim_SE_Game_Files): os cinco masters Skyrim.esm, Update.esm, Dawnguard.esm, HearthFires.esm e Dragonborn.esm; Skyrim.ccc lista conteúdo Creation Club. `Game::fixed_masters/creation_list` fornece esses nomes ao chamador. Nenhum diretório, timestamp ou `.ccc` altera a ordem explícita do manifest. `required_masters` continua sendo a lista declarada pelo usuário: não se acrescentam DLCs automaticamente.
- [Documentação comum de plugins](https://baddogskyrim.github.io/BethesdaLibrary/file-formats/plugins/): envelopes TES4/GRUP, XXXX, zlib, localized/Full Name, masters e FormIDs se aplicam ao SSE. Os layouts semânticos FO4 da fase 1 **não** são atribuídos ao SSE: payload/subrecords ficam crus, com contagem de envelopes e de campos não interpretados. FULL utiliza o contexto localized explícito e o mesmo leitor de strings; não se supõe que quatro bytes de texto sejam IDs sem essa flag.

Não há leitura de código de xEdit ou de outro leitor nesta etapa. As referências
a definições Pascal acima registram apenas as etapas anteriores.

Corroboração de FULL no SSE: [documentação do Dynamic String Distributor](https://moddingforge.com/docs/dsd), seção de tipos, documenta `XXXX FULL` como nome de qualquer record. Foi lida apenas a documentação, sem código, e não se implementa nem executa o distribuidor.

Achado do corpus SSE: `Skyrim.esm` contém um GMST com FormID cujo índice de master está fora de MAST. A discussão pública [Step, Bunch of Missing String Errors](https://stepmodifications.org/forum/topic/16669-bunch-of-missing-string-errors) descreve o mesmo HITME em `iDaysToRespawnVendor`. Isso sustenta reportar o achado, **não** autoriza corrigir os bits ou escolher outro definidor. O leitor mantém `InvalidFormId`; o índice incompleto conserva `BaseUnavailable` para possíveis injeções sem base certificada. O envelope e os demais records continuam sendo enumerados.


## Valores IEEE (validação do leitor)

A decisão de reviewed measurement decision substitui a rejeição anterior de floats não finitos:
Cursor::f32 conserva todos os bits. Campos Float/Transform, Light, posições
SCOL e prefixos de iluminação preservam NaN/inf. Cada Field possui
non_finite_values; o CLI soma por record/plugin e emite NonFiniteRecordValues
como notice. O JSON conserva payloads não finitos em ieee754_bits. Os perfis
de versão HEDR e todos os limites estruturais continuam validados; não se
atribui a NaN a semântica de uma versão de formato suportada.

## API tipada e pertencimento (validação do leitor)

A API tipada usa o mesmo `Record::fields` do censo, uma vez por leitura. Ausências
continuam Option; sobras conservam índice e intervalo em Record.payload, inclusive
a cauda de iluminação. MOD* não decodificados são companions crus. SCOL conserva
cada ONAM/DATA na ordem e não mistura placements de partes distintas; MSWP mantém
a sequência BNAM/SNAM/FNAM/CNAM, sem deduzir aplicação no renderer.

- https://baddogskyrim.github.io/BethesdaLibrary/file-formats/plugins/ : documentação
  de GRUP já citada acima; tipos 2/3 identificam contexto interior, 1/4/5 contexto
  exterior, 8/9/10 filhos persistentes/temporários/visíveis distantes. A classificação
  de CELL usa contexto declarado, não inventa bits de DATA. Sem contexto, None.
  A associação de filhos é a da ocorrência vencedora, nunca da referência perdedora.
- https://www.nexusmods.com/fallout4/articles/4316 : documentação “Precombine Previs
  Deep Dive”, do autor PJMail, sustenta as declarações CELL XCRI/PCMB (precombine),
  RVIS/VISI/XPRI (previs). Só classificamos os tags; payloads ficam crus. Nenhuma
  regra de seleção da geometria ou estado do engine foi implementada.
- https://tes5edit.github.io/fopdoc/Fallout4/Records/TXST.html : página de documentação
  (não fonte de leitor), sustenta TX00..TX07 como caminhos de texturas, MNAM como
  material e DNAM flags. Não transferimos layouts de REFR de Fallout NV para FO4.

- https://github.com/TES5Edit/TES5Edit/issues/231 : relato factual de fireundubh,
  “FO4Edit: Form ID Subrecords” (28/11/2015), não fonte de implementação: XMSP
  referencia MSWP; RVIS referencia CELL; XPRI contém referências. Não copiamos
  código nem abrimos os arquivos fonte do xEdit.
- https://gist.github.com/lojack5/759e10177ad7a5099e1e : levantamento de envelopes
  de Fallout4.esm por lojack5 (10/11/2015), não implementação: XMSP tem tamanho
  4 nos REFR. Junto à relação documentada acima e ao FormID little endian já
  sustentado, permite decodificar REFR.XMSP como FormID. Outro tamanho é erro.

Pesquisa adicional da ligação TXST na retomada:
https://tes5edit.github.io/fopdoc/Fallout4/Records/Subrecords/Model.html é uma
página de documentação, não implementação. Apesar do diretório Fallout4, seu
cabeçalho declara Fallout 3/New Vegas; descreve MODS como array de texturas
alternativas com referências TXST. Isso não sustenta substituir o MODS FormID
MSWP específico FO4 já registrado acima, nem inferir uma ligação TXST nos
companions FO4 crus. A consulta não alterou layouts; ausência de fonte FO4
suficiente continua explícita. Nenhum fonte de leitor ou do xEdit foi aberto.

Erros de validação tipada em records comprimidos conservam o offset no payload
mesmo quando seu número coincide com o offset físico do record; igualdade
numérica não muda o espaço de coordenadas. O teste de DuplicateTypedField
confere os dois offsets. Falhas de reserva no contexto físico do envelope
continuam identificadas como falhas de orçamento, sem reinterpretar bytes.

## Contrato de quadro parado (validação do leitor)

A classificação factual por RECORD.TAG está em
[static-records.tsv](static-records.tsv), compartilhada com `fo4_plugin::static_frame`;
[scene.md](scene.md) define o enquadramento. As fontes de cada exclusão estão na
própria linha. Os layouts anteriores acima não foram reabertos em código de
xEdit nesta etapa. CELL.XCIM e XGDR passam pelo mesmo contrato FormID LE de
quatro bytes e remapeamento por MAST; ausência continua None. Identificação dos
campos por autor FO4: https://www.nexusmods.com/fallout4/articles/4762 ; efeito
visual: https://falloutck.uesp.net/wiki/Cell . Não decodificamos IMGS/GDRY alvo.

As dez bases adicionais usam o contrato comum MODL/MODS/OBND anterior; as páginas
CK MiscItem, Container, Ingestible, Book, Ammo, Key, Holotape, Flora, Terminal e
TalkingActivator documentam sua representação por modelo. Exemplos de URLs:
https://falloutck.uesp.net/wiki/MiscItem e
https://falloutck.uesp.net/wiki/TalkingActivator . A tabela conserva a URL
específica para cada tipo. IDLM é marcador de idles/edição de atores
(https://falloutck.uesp.net/wiki/IdleMarker); SOUN/ASPC são áudio
(https://falloutck.uesp.net/wiki/Sound e
https://falloutck.uesp.net/wiki/Acoustic_Space). Permanecem bytes originais.

Lacuna explícita: https://forums.nexusmods.com/topic/5730837-colorizing-grey-base-textures/
documenta MODC como índice de remapeamento de cor, mas não estabelece sua
representação binária FO4. Os floats citados para valores de OMOD não comprovam
o tipo no disco de MODC. Ele continua unknown, não é convertido por palpite.

Os companions comuns PTRN, KSIZ/KWDA e DEST/DSTD/DSTF/DMDL/DMDT têm suas linhas
outside também nos dez tipos novos, não apenas nas chaves que já apareciam nas
causas da validação do leitor. A prévia revelou MISC.PTRN caindo no fallback Unknown;
a regressão reproduziu esse erro e passou após completar a tabela. Não houve
interpretação dos bytes desses campos nem reparo de caminho.

## Escrita de envelopes (validação do leitor)

O escritor próprio usa os mesmos fatos e fontes de cabeçalhos, GRUP, XXXX e
zlib documentados acima. FormIDs/ordem são fornecidos explicitamente pelo chamador;
não se criam números por data/ordem de diretório. compress é explícito e controla
COMPRESSED. Reescrita retém o payload original, incluindo transportadores XXXX
não canônicos; zlib garante igualdade do conteúdo descomprimido, não do stream.
HEDR/MAST são subrecords do modelo de entrada, sem defaults escondidos. Não escreve
arquivos por conta própria, não resolve FormIDs para outro load order nem salva jogo.
Limites: payload, memória, contagem, profundidade, u32 de tamanhos e orçamento
compartilhado; compressor em streaming com espaço de trabalho conservador orçado.
`flate2` já era dependência conhecida do workspace (MIT OR Apache-2.0); foi promovida
de dev-dependency também para dependência normal de fo4-plugin, para zlib opcional.

## validação do leitor — bancada fora de Data

A observação própria por ranges encontrou a menor CELL interior não apagada por
comprimento de envelope e, separadamente, a menor que possui GRUP de filhos.
A segunda tem um grupo 6 com grupo 9 vazio. A bancada copia os subrecords
estruturais DATA/LTMP/XCLW desta última, substitui EDID por identidade própria e
XCLL pelos bytes de outra CELL nomeada. Grupos 0/2/3/6/9 conservam a cauda observada;
o grupo 8 usa outra ocorrência observada em interior. Labels 6/8/9 passam a
conter nosso FormID de CELL. O componente de objeto da nova CELL é o da célula
modelo, com identidade do nosso plugin, conservando também os labels 2/3.

A explicação primária do autor do xEdit sobre os labels de interior é documental,
[não código nem script](https://stepmodifications.org/forum/topic/8203-xedit-guide/page/3/):
retira o byte de plugin, usa a unidade decimal para block e a dezena para
sub-block. A conferência FO4 dos metadados xDump cobriu 1.194/1.194 CELL interiores, sem divergência; os detalhes ficam no relatório local; não se
transferem outros detalhes do jogo descrito nesse texto para o nosso leitor.
O escritor releu todos os campos de envelope e cada payload cru da bancada;
os bytes de subrecords das nove cópias, exceto MODT, são comparados separadamente.

## Prefixo parcial REFR.XTEL (validação do leitor)

As observações autorizadas da layout REFR.XTEL, tamanho 36, sustentam somente
o primeiro u32 LE como FormID Door. Remapear pelos masters explícitos. O restante
do envelope permanece cru e bloqueante; outros tamanhos não herdam esse campo.
Evidências, limites da observação e testes estão em
[nif-render-layouts.md](nif-render-layouts.md); fonte de identificação da ferramenta
https://tes5edit.github.io/ (o site não prova o offset).

validação do leitor: transporte zlib de tamanho escolhido no escritor, restrito a payload
até 65535 bytes. [RFC1951 §3.2.3–3.2.6](https://www.rfc-editor.org/rfc/rfc1951)
sustenta blocos vazios stored de 5 bytes e a sequência fixed-EOB + stored vazio
de 6 bytes. Ambos decodificam zero bytes e permitem igualdade de tamanho sem
inventar subrecords, mudar EDID nem acrescentar dados após o stream.
[RFC1950 §2.2](https://www.rfc-editor.org/rfc/rfc1950) sustenta cabeçalho 78 01,
sem dicionário, e Adler32 big-endian. O payload final usa stored BFINAL=1.
Não é hipótese de render; o leitor independente confere todos os bytes após
expansão e rejeita tamanho declarado, truncamento ou checksum incorretos.
