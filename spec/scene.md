# CELL como dado (validação do leitor)

O contrato de Scene é política do projeto, não um formato binário novo.
Fatos binários usados vêm de plugin.md, nif.md, material.md, dds.md e
manifest-vfs.md. As novas fontes de XMSP e declarações de visibilidade estão em
plugin.md. Nenhum código de leitor foi consultado nesta etapa.

`fo4-plugin::typed::Store` abre a lista explícita do manifest, valida a load order,
indexa envelopes e memberships e relê somente o vencedor solicitado. `get<T>`
chama o decodificador de Field uma vez; move os valores para a estrutura e guarda
subrecord/range dos bytes restantes. FULL é decodificado com o flag localizado
do plugin que contém o vencedor. Não se procura FULL/MODL em um override anterior.

Filhos usam o label do GRUP 8/9; pertencimento é da ocorrência vencedora. Deleted
tem precedência sobre disabled na classificação. Grupo 10 é exclusão distante,
não é instância persistente/temporária adicional. Outros tipos colocados são
exclusões por tag. Sem grupo/contexto suficiente não inferimos associação/espécie
de CELL pelo nome ou pela posição. O índice incompleto desabilita candidatas.

Scene conserva Record.payload junto de cada view, os FormRefs remapeados e todas
as origens. Position/rotation e XSCL são os números IEEE originais; escala ausente
é None, não 1. SCOL conserva partes com seus placements e a base resolvida de cada
parte; somente REFR é instância. Não há mesh combinado carregado por declaração
PCMB/XCRI e não calculamos quais referências seriam ocultas pelo engine.

Assets são um grafo cacheado, com limite próprio, separado do orçamento temporário.
NIF parte dos roots declarados e segue links já decodificados, com visited/depth;
bloco/tail/campo desconhecido alcançado bloqueia. Bloco desconhecido não alcançado
não vira dependência visual por suposição. Controllers/sequence/interpolators e
skin alcançados bloqueiam execução estática; física não é sintetizada. Materiais
conservam parent, slots e flags; DDS é validado pelo leitor existente. Nenhum
shader, BRDF, luz física, collider ou unidade é convertido nesta etapa.

MSWP base (MODS) e REFR (XMSP) são resolvidos separadamente, com origem e sequência
BNAM/SNAM/FNAM/CNAM; os materiais SNAM entram como dependências declaradas. Não se
funde base/ref nem se simula a escolha do engine; FNAM bloqueia como condição não
avaliada. `Loader::texture_set` resolve um FormKey TXST explícito e seus slots,
com os records/paths/origens. Um companion de modelo cru NÃO é interpretado como
TXST por tentativa; falta uma fonte para essa ligação FO4 na fatia atual.
`UninterpretedVisualSubrecord` conserva essa lacuna e bloqueia a certificação.

`ready` significa fechamento dos dados conhecidos dessa fatia, sem blockers.
Não é certificação do renderer nem do comportamento do executável. Raw requerido,
XESP, VMAD/CTDA, fora da fatia, dependências ausentes/ciclos/paths recusados têm
causas nomeadas. Uma candidata exige interior, zero bloqueio/exclusão/diagnóstico,
ao menos um LIGH ativo, nenhuma declaração precombine e índice completo. Não se
relaxa critério quando a lista é vazia. Nome localizado indisponível continua None
com erro e ID original, nunca nome fabricado.

Censo `cells`: `summary.json` agrega e `items.jsonl` contém todas as linhas de
células e exclusões. `--items` acrescenta instâncias completas e assets por ID
com origem; o padrão evita repetir payloads de bases para cada REFR. `references` inclui ativos, deleted,
disabled e exclusões; ready/blocked cobrem os ativos. by_base cobre todas essas
categorias, com buckets unresolved explicitamente nomeados. Causas contam uma
vez por instância/causa; uma instância pode ter várias causas. Modelos/materiais/
texturas distintos contam só nodes com entrada vencedora, inclusive entrada cuja
leitura falhou. Candidatas são no máximo 20, por número total de referências e
FormKey como desempate; IDs nunca são constantes no código.

## Conferência da retomada

Espécie da CELL vem somente do contexto GRUP da ocorrência vencedora; um
vencedor sem contexto não herda a espécie de um perdedor. REFR sem grupo filho
explicitamente associado produz UnassignedCellReference e índice incompleto,
sem inventar a célula anterior. LGTM apagado fica preservado como dado, mas
DeletedLightingTemplate impede certificá-lo como iluminação válida.

Se o orçamento impede publicar um diagnóstico após inserir um node do cache,
a inspeção devolve o erro original e o cache passa a AssetCacheUnavailable.
Não se transforma estado interrompido em AssetDependencyCycle. Outra inspeção
exige novo cache; o VFS e o índice de plugins podem ser mantidos. A detecção só
percorre os nodes inseridos na chamada, sem varrer todos os assets já cacheados.

A publicação da identidade no índice do cache também ocorre somente depois da
alocação do path e do node: reserva recusada antes disso deixa o índice sem essa
entrada, permitindo nova chamada sem índice fora de faixa. O teste força
exatamente essa fronteira do orçamento e repete o lookup sem panic.

## validação do leitor: observabilidade e orçamento de censo

Raízes por classe estão em citation-roots.md. O histórico cru não muda.
UninterpretedVisualSubrecord informa RECORD.TAG; erros de NIF informam tipo de
bloco e, para campo cru, o nome do campo. CELL/LGTM e outros diagnósticos comuns
bloqueiam também cada placement que depende da inspeção da célula, preservando
a rejeição anterior da candidata. Não se avalia o runtime para removê-los.

O censo usa blocos de até 32 CELL, limitados por max_memory/max_entries, com
renovação antecipada acima de 75% do orçamento ou das entradas. Mede
pico contabilizado e entradas; RSS é medição operacional separada. LimitExceeded,
AllocationFailed e AssetCacheUnavailable são falhas de infraestrutura, com o
erro original conservado no evento da célula, sem contaminar as causas de dados.
Após falha de infraestrutura, o próximo CELL recebe novo cache. Contagens globais de distintos usam conjuntos
separados de chaves vencedoras, não IDs do cache renovado.

Causas detalhadas contam placements uma vez por causa, células uma vez por causa,
citações de dependência e declarações comuns separadas. sole_cause_instances
exige exatamente uma causa detalhada, inclusive dependências comuns da célula.
Placed types excluídos também são placements bloqueados na tabela detalhada;
ready/blocked da tabela principal continuam restritos aos REFR ativos.
A lista closest tem interiores com denominador ready+blocked positivo, ordenados
pela fração exata (produtos u128), depois menos causas distintas; quantidade de
referências e FormKey desempatem. Ela não muda a lista estrita de candidatas.
Store::kind usa HashMap por identidade; winners usa vetor de índices construído
uma vez, sem percorrer repetidamente o histórico de overrides.

## validação do leitor: contrato do quadro parado

Quadro parado é a imagem da célula como foi autorada: sem tempo decorrido,
sem script executado, sem ator; cada referência conserva o estado inicial dos
dados. Não se avaliam condições, enable parents, animação, dano futuro ou
seleção dinâmica de materiais. `ready` continua sendo fechamento de dados,
não equivalência fotométrica com o executável.

A tabela canônica de RECORD.TAG é [static-records.tsv](static-records.tsv),
incorporada literalmente em `fo4_plugin::static_frame`. Cada linha tem uma das quatro
classes `used`, `outside`, `state`, `unknown`, layout, fonte e justificativa.
[static-record-keys.txt](static-record-keys.txt) fixa os 191 pares que motivaram
a revisão; o teste exige cobertura única de todos eles. A tabela inclui também
campos tipados anteriores e CELL/LGTM. Chave ausente é `unknown`. `outside`
conserva payload/range e é contada sem bloquear; `state` produz condição nomeada;
`unknown` bloqueia. `used` que permaneça cru produz
`UnsupportedUsedFieldLayout`, nunca desaparece por estar na tabela.

Os layouts aprovados nas etapas anteriores continuam em [plugin.md](plugin.md).
As URLs de definições na tabela são proveniência histórica, não nova leitura do
fonte do xEdit. FULL/EDID identificam registros, não alteram a imagem; continuam
acessíveis tipados junto dos bytes originais. A documentação FO4 pública do
fopdoc está incompleta (o próprio https://tes5edit.github.io/ seção 1.5 o avisa);
as páginas CELL/REFR/STAT/LIGH/LGTM não existem. Não transplantamos layouts de
FO3/NV/Skyrim. Semântica conhecida no editor não certifica tamanho, ordem,
variante ou unidade no disco: essas lacunas estão explicitadas como unknown.

Novos campos usados: CELL.XCIM (ImageSpace) e CELL.XGDR (God Rays), ambos
FormRef opcional com os quatro bytes LE do contrato de FormID, remapeados por
MAST sem valor padrão. O tutorial do autor em
https://www.nexusmods.com/fallout4/articles/4762 identifica essas referências;
https://falloutck.uesp.net/wiki/Cell descreve seu efeito visual. O enquadramento
como referência usa esse texto e o contrato comum de FormID de plugin.md;
não pressupõe o layout dos records alvo IMGS/GDRY. São entregues em
`Scene.cell.data.image_space/god_rays`. O mesmo record guarda dados e lacunas de
iluminação, água e clima; nenhuma cauda desconhecida recebe preenchimento.

A fatia de bases visíveis inclui MISC/CONT/ALCH/BOOK/AMMO/KEYM/NOTE/FLOR/TERM/TACT.
As páginas oficiais do CK MiscItem, Container, Ingestible, Book, Ammo, Key,
Holotape, Flora, Terminal e TalkingActivator documentam seu model; os campos
comuns MODL/MODS/OBND usam os layouts anteriores, sem adivinhar os companions.
IDLM usa marker model para edição e idles de atores (CK IdleMarker); SOUN e
ASPC são configurações de áudio (CK Sound, Acoustic Space), conservadas como
bases `outside`, sem carregar modelo de marcador. Outros tipos não são
promovidos: `BaseTypeOutsideSlice:TIPO` conserva o bloqueio.

### NIF e acesso tipado

[static-blocks.tsv](static-blocks.tsv) é a tabela incorporada por
`fo4_scene::model_policy`. Fonte de estrutura: nifxml, mesma revisão e hash de
[nif.md](nif.md); apenas o schema de dados, nenhum leitor. bhk/NiCollisionObject
são colisão/física, BSMultiBound/AABB/OBB/Sphere e BSBound são volumes limite;
esses payloads ficam crus e contados em `outside_blocks`, sem percorrer seu
back pointer Target. BSMultiBoundNode continua nó visual: seus filhos são
percorridos. Marcadores de inventário/mobília ficam fora do quadro sem atores
nem menus. Behavior graph permanece estado, pois pode dirigir animação.

Controladores/interpoladores/chaves/partículas permanecem bloqueados por tipo;
`Asset.animations` conserva block index, Target quando decodificado e
`target_in_visual: Option<bool>`. None significa alvo desconhecido/não aplicável,
nunca false por falta de decoder. O fechamento visual exclui arestas Controller,
Collision, Multi Bound e Extra Data; o fechamento de diagnóstico continua
inspecionando controles alcançados. Target de controlador é ponteiro fraco,
não aresta de propriedade. Pele, água e luz embutida continuam bloqueadas por
tipo; não se amostra um frame de animação para contornar o bloqueio.

`Assets::with_decoded` relê por range um asset declarado sob orçamento do
chamador e entrega Nif/Material/Texture tipado a um callback. Não se retêm todas
as geometrias no cache. Os campos usados, inclusive os já expostos pelo leitor,
ficam assim acessíveis a quem possui Scene + Loader, com bytes e proveniência.

### Regra bgsm_name_overrides_texture_set_v1

Relatos primários de autores em
https://www.reddit.com/r/FalloutMods/comments/nfssnh/ e
https://www.reddit.com/r/FalloutMods/comments/h047vm/ descrevem o material do
Name substituindo BSShaderTextureSet. A regra só suprime a aresta Texture Set
quando o Name de BSLightingShaderProperty é BGSM/BGEM e resolve estritamente no
VFS. Arquivo material inválido continua bloqueando sua própria leitura.
Name vazio, ausente, absoluto, recusado ou não resolvido não suprime a lista.
Não há reparo de path. Texturas inline de BSEffectShaderProperty não são
suprimidas: falta evidência equivalente para aquela representação.

O audit mantém listas originais, inclusive suprimidas, com ownership Material,
NoMaterial, Mixed ou Unreferenced. Conjuntos compartilhados só são integralmente
suprimidos se todos os seus donos resolvem. O escopo auxiliar shader-citations
conta o universo dos NIF vencedores; não confunde listas órfãs com formas sem
material. `bare` exige path relativo normalizável cuja raiz não é textures.

### Material, ciclo e duas camadas

UnsupportedMaterialFields separa tile_flags.extra_bits, mask_writes.extra_bits,
alpha_blend.mode, terrain.unknown_v3 e bgem.glass_scalars. Os valores crus e as
estruturas tipadas anteriores são preservados. Discussões de autores
https://forums.nexusmods.com/topic/13113762-need-help-with-transparent-texture-dds-bgsm/
e https://forums.nexusmods.com/topic/13485563-go-ground-objects-that-are-translucent-are-invisible-in-previews/
relatam alpha test/dithering do modo Unknown; não sustentam convertê-lo em
Disabled nem um algoritmo de composição completo. Permanece bloqueado.

AssetDependencyCycle contém comprimento e classe de cada aresta. Os IDs das
arestas em Asset.cycles referem-se ao cache local; o censo canonicaliza paths e
classes para não contar a mesma cadeia a cada renovação. A discussão primária
https://forums.nexusmods.com/topic/13485213-wetness-materials/ descreve
RootMaterialPath como template de wetness; não estabelece parada recursiva de
self-parent. Não aplicamos regra de quebra sem fonte; ciclos permanecem dados
bloqueados, distintos de interrupções de cache (AssetCacheUnavailable).

Scene.diagnostics e CellTotals.cell_causes são a camada CELL/LGTM. Instance.state
considera apenas REFR/base/assets. Falha ao ler um filho pertence à exclusão
correspondente, não à iluminação da célula. ready_with_cell projeta ambas;
closest usa ready/(ready+blocked+outside), somente interiores com pelo menos dez
REFR ativos, e mostra luzes, precombine e cell_causes. O critério estrito de
candidata anterior é preservado. Campos fora do quadro e bases de marcador são
contados separadamente. Sole cause nas duas camadas tem denominadores distintos.

`sweep --scope bases` inspeciona todos os vencedores dos 17 tipos visíveis,
inclusive apagados (DeletedBaseForm bloqueia), sem carregar CELL. Publica linha
local por base com EditorID, MODL, formas, assets e refs; summary tem totais por
tipo/causa. Referências contam vencedores REFR, incluindo deleted/disabled;
active_references é separado. Interiores vêm do pertencimento explícito da
ocorrência vencedora, nunca da posição. Falhas de leitura permanecem contadas.
Nenhum EditorID, path de asset ou FormKey real precisa entrar no Git.

Orçamento: tabelas Field/consumed, lookup de cache e stacks/visited de closure
usam children temporários do Budget, liberados ao sair. Payload/valores movidos,
Scene e diagnósticos retidos continuam cobrados. Budget::metered mede créditos
vivos, cargas cumulativas e pico por fase; não é medidor de RSS. Os limites não
foram aumentados para contornar a célula grande.

Decisão do projeto ativa `bgsm_self_wetness_template_stop_v1`, por decisão explícita do
revisor: uma aresta MaterialParent para o mesmo arquivo não acrescenta bytes ao
conjunto do fechamento. Não se afirma semântica medida de herança de wetness.
A aresta/offset permanece na auditoria de ciclos; a dependência terminal não
aponta ao próprio ready bit. Duas ou mais identidades ainda geram
AssetDependencyCycle. Testes sintéticos com BGSM completos provam publicação
ready no auto-parentesco e bloqueio de uma cadeia de dois arquivos.

validação do leitor entrega campos das layouts em Common.layout_fields: nomes e tipos, FormRef
remapeado, campos opcionais ausentes e ranges de textos/bytes no record original.
Somente os tamanhos conferidos são aceitos. As partes abertas conservam causas
no segundo nível. Palavras inteiras de flags observadas na layout expõem os rótulos
impressos; nenhum bit é inferido. Palavras não observadas/Unknown bloqueiam. STAT.MNAM é usado e exposto como quatro nomes de LOD; não foi
promovido a Outside por analogia com Skyrim.

O alfa sempre entrega enabled/source/destination e alpha-test; mode não controla
prontidão. `bgsm_blend_disabled_factors_ignored_v1` aceita enabled==0 por decisão
do revisor. Enabled diferente de zero mantém causa com a tupla, pois não há fonte
permitida ligando a numeração BGSM à enumeração NIF/Direct3D.

`check-cell MANIFEST EditorID [--out DIR]` exige Ready de todas as instâncias da
cena. Preserva causas de CELL/LGTM separadamente; seu sucesso não certifica
iluminação nem renderização. O escritor de envelopes não instala arquivos.

## validação do leitor — hipóteses e marcadores

`spec/hypotheses.tsv` registra cada hipótese anunciada, seu escopo e veredito.
`Ready` continua exigindo ausência de causas. `ReadyUnderHypotheses` exige que
cada causa tenha cobertura exata no registro; conserva todos os diagnósticos.
Ângulos de referência/SCOL diferentes de zero recebem causa explícita de rotação.
Campos MODT de outro nome e tuplas alfa não registradas continuam bloqueados.

Fonte factual do rótulo de cabeçalho: **xEdit 4.1.5f, executado como caixa-preta;
conferido 24.379 de 24.379 registros do conjunto disponível no Fallout4.esm**.
Os rótulos `Is Marker` em ACTI/DOOR/STAT coincidem com `0x00800000`; são 457
registros rotulados, sem divergência. Fotografias controladas sustentam a
ocultação pelo bit nesses três tipos: três espécimes com bit ausentes e controle
sem bit visível. `editor_marker` depende do bit. Compartilhar MODL com um marcador
não oculta outra base: essa inferência foi refutada pelo controle sem bit.
Nomes de nós NIF são hipóteses separadas, ainda pendentes. Não se generaliza
por prefixo de nome nem por BSXFlags documentado somente para outros jogos.

Busca documental: [CK Static](https://falloutck.uesp.net/wiki/Static),
[CK GetAngleX](https://falloutck.uesp.net/wiki/GetAngleX_-_ObjectReference) e
[CK SetAngle](https://falloutck.uesp.net/wiki/SetAngle_-_ObjectReference) não ficaram
acessíveis nesta consulta; não fundamentam ordem/sentido de Euler ou ocultação.
A documentação de outro jogo não é transferida por analogia.

A composição de linhas `Rw=Rl*Rp; tw=tp+sp*Rp^T*tl; sw=sp*sl` é sustentada
por matrizes medidas. A projeção usa componentes forward/up/right, slopes de
frustum e viewport explícitos, conforme `fo4_scene::camera` e seus testes.
Isso não estabelece cobertura de pixels, jitter ou profundidade GPU. A ordem
Euler ZYX é supported no escopo medido; não se afirma aritmética exata por bits.
[rotation-arithmetic.md](rotation-arithmetic.md) registra essa distinção.

O `check-cell --under-hypotheses` aceita também o terceiro estado e entrega o
conjunto em cada linha; o modo estrito continua falhando nele. `--overlay-plugin`
abre um único plugin adicional explícito, somente para leitura, no fim da ordem,
sem mudar os mounts ou instalar arquivo. O root de Data continua protegido pelas
checagens de identidade de saída. Não se transforma um ancestral comum de Data e
workspace em root de saída permissiva. O rótulo REFR `Persistent` foi conferido
em 67/67 registros da amostra xDump, 20 com o bit `0x00000400` ligado; o marcador
de chegada autorado usa esse bit e grupo persistente observado.

`refr_absent_xscl_identity_v1` sustenta escala unitária quando XSCL está
ausente, com seis bases e 18 dumps de controle: ausência e 1,0 têm palavra
3F800000, 0,5 tem 3F000000. A ausência continua representada como `None` e
causa nomeada; o rasterizador depende explicitamente da hipótese propagada.

`scol_parts_static_frame_v1` continua pendente.
Uma SCOL com partes recebe `UnverifiedScolGeometrySource`, coberta apenas por essa
hipótese. O renderer usa as instâncias ONAM/DATA e seus NIFs nessa condição;
não desenha também o MODL gerado. Este continua carregado, auditado e contado
no grafo. A equivalência de aparência/geometria/material com o SCOL em runtime
não foi estabelecida pelo layout ONAM/DATA e exige a medição do registro.

A hipótese separada `nif_editor_marker_named_hidden_v1` cobre objetos NIF de cena (nó ou geometria) com nome exatamente `EditorMarker`,
observado diretamente em NIFs FO4 (saída local em
`local-measurement-artifact`, campos Name/Translation e refs de shader;
sem interpretação de código externo). Esse objeto pode coexistir com geometria visível;
não transforma a base inteira em `editor_marker`. O grafo conserva e inspeciona
seus dados, acrescenta `UnverifiedNifEditorMarkerVisibility` e a Scene depende da
hipótese. Só o renderer sob essa hipótese omite a subárvore e conta as omissões.
Não se infere isso do bit BSX descrito para Skyrim. Os slots vazios encontrados
nesses helpers não são evidência sobre a representação SCOL ou difusas principais.

validação do leitor: `Scene.lighting_data` declara as luzes LIGH colocadas da célula,
transform/XSCL/flags do REFR, raio nominal, RGB e quarto byte explicitamente
não usado como alfa. `base_fields`, `reference_fields`, `cell_fields` e
`template_fields` transportam campos sustentados, ranges do payload e origem;
`gaps` conserva nomes e ranges não interpretados. Flags observadas são palavras
completas, não uma bitmask generalizada. Ausência de declaração de tipo não
vira luz pontual. `fields_completely_described` e o estado separado da célula
só certificam cobertura dos dados; não resolvem herança, tempo, exposição,
unidade fotométrica, atenuação nem sombreamento.
Luzes com REFR inicialmente disabled também são consultadas como dados, sem
carregar modelos/materiais e sem adicioná-las a `Scene.instances`/draws. As
flags do record distinguem esse estado inicial de uma avaliação runtime.
Falhas ao classificar essas referências ficam em `unreadable_inactive_placements`;
ausência de NAME ou de base nunca vira ausência presumida de luz. Deleted
continua no envelope separado de `Scene.deleted`.
Os payloads/ranges dessas luzes disabled ficam retidos em `inactive_records`,
com crédito de memória transferido do decoder temporário; campos crus e strings
não apontam para records já descartados.

## Cells around an exterior cell

`fo4_scene::grid` finds the exterior cells around a cell and joins their
placed references to its scene. It adds no format fact and no rule, and it
claims nothing about which cells the game keeps loaded: drawing and walking
over a block of cells is a viewing choice.

- `around(store, cell, radius, budget)` (radius 0 to `MAX_RADIUS` = 3) reads
  the cell's XCLC grid and, for an exterior cell of a world, every other
  exterior cell of the same world once, keeping those within `radius` grid
  steps on both axes. Each position is `cell`, `no_cell` or
  `ambiguous_grid` (two or more non-deleted cells claim it; neither is
  used, as in the `land` report). Positions come in `offsets(radius)` order:
  the first ring as east, west, north, south, north-east, north-west,
  south-east, south-west, then each further ring from south to north and
  west to east. An interior cell, a cell without a grid or radius 0 is not
  searched.
- `absorb_around(loader, scene, around, budget)` loads each found cell with
  the same `Loader` and appends its instances, deleted, disabled and
  excluded children, diagnostics, precombine and previs declarations and
  assets to the centre's `Scene`, which keeps the centre's record, name,
  kind and `lighting_data`. Asset indices belong to the loader, and both the
  draw list and the collision of a cell order references by plugin and
  object ID, so the joined scene is drawn and collided as one placement
  list: instance tags run over every cell's references. Each cell is read
  in a child scope of the budget that the budget then keeps
  (`Budget::commit`); joining it to the scene is charged to the budget
  itself.

### Memory of a joined scene

The manifest's `max_memory` is a safety limit on one read of possibly
hostile input, not a scene size: the credits that 25 or 49 legitimately
bounded cells keep add up past it (at the test cell, with `--walk
--terrain`, about 476 MiB of credits at radius 1, 881 MiB at radius 2 and
1,477 MiB at radius 3, of which the viewer's stored texture blocks are
250 to 412 MiB). A `Budget` therefore has two ceilings
(`fo4_core::Budget::with_scope_limit`): the root's bounds the sum of what
the scene keeps, and the scope limit bounds every child scope, that is
one cell read, one asset decode, one terrain piece's textures, with all
their own children. `commit` keeps a child's charges in its parent;
dropping it releases them. A long-lived cache that grows with the scene
(the loader's asset graph) is an `account` of the root, bounded by the
root. A root made with `Budget::new` has a scope limit equal to its
ceiling, so the `render` example and the reports behave as before: their
whole run stays within `max_memory`.

The GPU viewer's root is `--scene-memory MIB` (default 2048) with
`max_memory` as the scope limit; exceeding the root fails with
`LimitExceeded` naming the option. Credits are conservative (a vector's
growth keeps its old charge, records read and dropped stay charged) and
are not resident memory: at radius 3 the headless viewer peaked at about
1.8 GB resident.

The `render` example and the GPU viewer take it behind `--around N` (1 to
3), with the walking world built from the joined scene's collision and,
with `--terrain`, every built terrain piece drawn ([walk](walk.md),
[land](land.md)). Without the flag nothing is searched and the draw list,
frame and summary are unchanged.

## The moving grid

The GPU viewer's window with `--walk --terrain --around N` in an exterior
cell keeps the loaded grid around the player instead of loading it once.
It adds no format fact and no game rule: **which cells are loaded and when
is a viewer policy**, chosen for walking without loading and dropping the
same cells over and over; it claims nothing about which cells the game
keeps loaded around the player, or when it loads them.

- **Index.** `fo4_scene::grid::world_grid(store, cell, budget)` reads every
  exterior cell of the cell's world once (as `around` does) into a map from
  XCLC grid to `Claim::Cell` or `Claim::Ambiguous` (two or more cells claim
  it; neither is used). The cell's own grid is the cell's; other cells
  claiming it are counted and ignored, as in `around`. Its lookups agree
  with `around` for every radius.
- **The cell under the player.** The walker's feet (the eye while flying)
  divided by 4,096 and rounded down: the cell whose terrain footprint holds
  the point under the pending rule `land_grid_spacing_128_v1`
  ([land.md](land.md)). The window and `--path` refuse to move the grid at
  a level that does not accept it.
- **Viewer policy.** Load every grid position within N steps (on the
  farther axis) of the cell under the player, nearest first (by steps, then
  by distance between cell centres, then south to north and west to east);
  drop loaded cells more than N + 1 steps away. With the extra step,
  standing on a border loads one row once and drops nothing (a test walks
  a border twenty times: one row loaded, nothing dropped; with N for both
  distances the same walk drops 60 cells).
- **Loading thread.** One thread owns the plugins, the archives, the
  credits and a loader; the main thread never reads game data while the
  window runs. It loads one cell at a time and checks for a newer centre
  after each: what the new centre no longer needs within N steps leaves the
  queue, and what lies past N + 1 steps is dropped first. Each cell is read
  alone with the same calls as the joined scene (`Loader::cell`, the draw
  list, the terrain with its ground textures tagged after the cell's own
  instances, the cell's collision and walking world with its terrain) and
  converted to GPU data in the thread; only plain data crosses to the main
  thread. Messages are applied in the order they were sent.
- **Main thread.** A received cell's walking world joins the walker's
  `fo4_walk::WorldSet` under the cell's grid at once (before any of its
  drawing data is uploaded); its meshes and textures are uploaded in
  batches of at most 64 MiB a frame, as at start-up, and its instances are
  spawned when all are on the GPU. A dropped cell leaves the set and is
  despawned. While the cell under the walker has not arrived the walker
  waits ([walk.md](walk.md), Viewer).
- **Shared data.** Meshes and textures are identified by their content
  (two 64-bit hashes, the byte count and the kind): equal data drawn by
  several cells is sent and uploaded once and held while any loaded cell
  uses it, by a count on each side. The loading side decides whether a
  key's data travels with a cell from the same message order the main
  thread applies, so both counts agree without a reply.

### Credits of the moving grid

The root is `--scene-memory` with the manifest's `max_memory` as the scope
limit, as for the joined scene. The rule:

- each loaded cell has an account of the root charged with its walking
  world and with what the main thread keeps of it apart from shared data
  (instances, surfaces, keys); dropping the cell drops the account;
- each mesh or texture held by content has its own account charged with
  its bytes when the first cell using it is loaded, dropped when the last
  cell using it is dropped;
- a cell is read in a child scope of the root, released once the cell is
  converted;
- the loader's asset graph is an account of the root that lives while
  cells wait to load and is dropped, with its credits, when the queue is
  empty, so walking far does not grow it without bound;
- the grid index is an account kept for the whole run.

A synthetic test walks away and back over a grid of hand-built cells and
finds exactly the credits of the first visit whenever the same cells are
kept. Measured at the test cell with `--path` (two round trips of four
cells east and back, 24 grid changes, run speed): at radius 1 the start
keeps 9 cells and 313.7 MiB, the walk 13 to 16 cells and 357 to 401 MiB,
peak credits 563 MiB while a batch loads, peak resident memory 1.54 GB;
at radius 2, 25 cells and 503.5 MiB at the start, 33 to 36 cells and 584
to 674 MiB while walking, peak credits 856 MiB, 1.79 GB resident. Every
grid change of the second round trip keeps exactly the credits of the
same change in the first (radius 1: 362.9 MiB back at the start cell,
with the four cells the extra step keeps; radius 2: 591.9 MiB). Loading
takes 0.3 to 1.6 s a cell in the viewer's build, so a crossing at radius
1 (three cells) takes 2 to 3.3 s of the loading thread and one at radius
2 (five cells) 3 to 4.5 s; applying a cell on the receiving side takes
under 2 ms, and in the window each cell was uploaded within one or two
frames.

Without the flags, with `--no-stream`, `--around 0`, in an interior cell or
headless without `--path`, nothing of this runs: the scene is loaded once
as before and the frames and tags of `--headless` are byte-identical to
those before the moving grid (checked at `--around` 0, 1 and 2).
