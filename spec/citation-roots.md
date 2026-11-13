# Raízes das citações FO4 — validação do leitor

`fo4-scene::paths::Citation` seleciona uma regra nomeada antes do VFS. O path cru
continua no record/material/NIF original; a chave composta não altera esses bytes.
Não há reparo de Data/, drive de desenvolvimento, barra inicial ou pasta duplicada.
O VFS mantém o contrato anterior. As tentativas de reparo ficam só no censo.

Fontes novas: documentação de autores de mods, sem abrir implementação de leitor.
BethesdaLibrary não foi usada como fonte nova. As fontes de layout já registradas
em plugin.md/nif.md/material.md continuam sendo a base dos campos binários.

| Citação | Regra estrita | Fonte e sustentação |
|---|---|---|
| MODL da fatia visual | relativo a meshes/ | [Tutorial do autor de Sim Settlements](https://simsettlements.com/web/wiki/index.php?title=Toolkit_Chapter_02_Creating_Your_First_Building), criação/sanitização de modelo: armazenamento em Data/Meshes e seleção no Static; [RedSav](https://redsavstudios.com/fallout-mods/modding-notes/mat-swaps/), exemplo de Model sem meshes/. Corpus: nenhum lookup cru, 30.927 citações só com raiz. |
| MSWP BNAM/SNAM | relativo a materials/ | [RedSav, Material Swaps](https://redsavstudios.com/fallout-mods/modding-notes/mat-swaps/), caminhos da janela MSWP começam depois de Data/Materials. Associação BNAM/SNAM vem do layout já registrado; ambos foram medidos separados. |
| BGSM/BGEM slots de textura | relativo a textures/ | [RedSav, Creating a New Material](https://redsavstudios.com/fallout-mods/modding-notes/mat-swaps/), DDS no editor é indicado depois de Textures. O leitor conserva slots sem path como vazios, sem inventar textura. |
| BGSM material-pai | relativo a materials/ | [robotized, Wetness materials](https://forums.nexusmods.com/topic/13485213-wetness-materials/): Root Material Path usa template/ArmorTemplate_Wet.bgsm, localizado em Materials.ba2/Template. Não se importou implementação. |
| TXST TX00..TX07 / MNAM | relativo a textures/ / materials/ | [documentação dos campos TXST](https://tes5edit.github.io/fopdoc/Fallout4/Records/TXST.html) identifica arquivos de textura/material; as raízes de recurso são as documentadas acima. A associação destas raízes a esses campos é corroborada pelo corpus: zero lookup cru, 852/909 citações TX e 185/186 MNAM só com raiz. É uma inferência explícita combinando a documentação e a medição, sem transpor MODS de FO3/FNV. |
| NIF shader Name | relativo a Data, com materials/ | [documentação prática de SammyMemento](https://forums.nexusmods.com/profile/120281728-sammymemento/content/?change_section=1&type=forums_topic_post), explica o Name como caminho relativo ao Data e exemplifica materials/. A medição separada não encontrou caso Name resolvido apenas com raiz. |
| NIF shader Root Material | relativo a materials/ | O campo de material-raiz já está documentado no layout nif.md; o mesmo tipo de recurso/template é descrito no relato de wetness acima. A medição separada é a corroboração: 76 paths/14.906 citações só com materials/, um path já prefixado. A composição é inferência declarada de classe de recurso; o path prefixado é anômalo para esta regra. |
| NIF BSShaderTextureSet / efeito | relativo a Data, com textures/ | [relato do autor de retextura](https://forums.nexusmods.com/topic/4656475-workflow/?comment=40402110&do=findComment) documenta path NIF textures/Props/... em vez do drive do PC; [exemplo FO4 do BSShaderTextureSet](https://forums.nexusmods.com/topic/13340835-adding-a-new-texture-using-an-existing-model-to-make-a-new-item/) confirma essa forma. O corpus é misto: não se prefixam automaticamente os relativos destas classes. |

A tabela de contagens completa tem distintos e citações para as-is-only,
prefix-only, both, neither, starts-with-root, invalid e empty em
local-measurement-artifact linhas em items.jsonl. Vazios são
uma categoria explícita dentro de neither, não evidência contra a raiz de um
path presente. MODL medido restringe-se aos tipos já decodificados da fatia visual.
Name e Root Material dos shaders são classes separadas: misturá-los esconderia
que o primeiro é relativo a Data e o segundo costuma ser relativo a Materials.

As regras correspondem aos nomes *_relative_v1 retornados por Citation::rule.
Class-relative rejeita o próprio prefixo já presente; data-relative exige a
pasta de recurso documentada. Missing após compor mantém a chave literal; não
se troca extensão, não se corta Data/ ou drive e não se promove loser.

## validação do leitor — subconjunto comparável e peso

A sonda de subconjunto de texturas usa ModelPolicy e só consulta
`textures/` + path normalizado para separar original ausente/candidato presente.
O candidato nunca entra em Assets/Loader. A mesma amostra de vencedores NIF
reproduziu os 9.007 paths e 3.077.858 citações prefixed_only de nif_texture_set da
validação do leitor. Material/NoMaterial/Mixed ficam separados; inline BGEM não é
suprimido. Arrays de paths da sonda ficam somente no stdout local ignorado.
Agregados detalhados e identidades de assets permanecem nos relatórios locais não distribuídos.

O cruzamento de citações com linhas de base usa paths DDS sem raízes reservadas,
sem colisão com classes que rejeitam paths de material/modelo. A conservação da
família completa confere 2.480 bases / 21.349 refs ativas contra cells; a parte
resolvível com prefixo confere 2.473 bases / 19.730 ativas. Nesse leitor, REFR
acrescenta dependências de material (swap), sem substituir o fechamento NIF da
base. Não se inferiu a união de colocações somando contagens de paths, que podem
se sobrepor. Sole do subconjunto é zero, limitado pelo zero da família inteira.
