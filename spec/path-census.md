# Hipóteses de caminho, só no censo

Fonte do contrato VFS: manifest-vfs.md e decisão de medição revisada §3. Estas transformações são
hipóteses de resolução registradas, NÃO fatos sobre como o Fallout resolve paths.
Nenhuma nova regra de formato ou regra do engine foi inferida delas.

A cena expõe `PathPolicy::Strict` (Default) e usa somente winner_bytes. O escopo
`paths` do CLI expõe lado a lado strict_state e `census_candidate_v2`; `unread`
também fornece a mesma classificação. São contados os nomes de material citados
pelos shaders NIF, uma vez por ocorrência de Field e com deduplicação por bytes.

O censo registra todos os traços, mas atribui uma classe primária nesta ordem:
absoluto de desenvolvimento, barra inicial, espaço final, extensão diferente, Data/materials/,
sem materials/, barra invertida, maiúsculas, outro. Assim classes não somam duas
vezes a mesma citação. `strict_invalid` e suas citações isolam o denominador dos
paths recusados; `strict_absent` isola os normalizados que não existem. Outros
paths válidos também ficam visíveis, para não chamar barra/caixa de erro do VFS.

Candidato: normalizar barras e caixa ASCII, em absoluto cortar somente até um
segmento materials/ existente, remover barras iniciais de relativo e espaços
finais, retirar exatamente Data/ de um prefixo Data/materials/, prefixar materials/ somente se o relativo for aceito pelo contrato.
Extensão diferente gera DUAS possibilidades (.bgsm/.bgem): ambas presentes é
ambiguous, nunca escolhemos a primeira. Traversal continua recusado. Caminho
absoluto sem segmento materials/ continua inválido. Listas de possibilidades em
hex e contagens de arquivos distintos/citações resolvidas/ausentes/invalidas/
ambíguas permitem medir a hipótese sem aplicá-la ao consumidor.

A hipótese Data/materials/ foi acrescentada na validação do leitor após conferir no CSV
local anterior 168.269 citações desse grupo. É data_folder_prefix, não uma
regra do engine: o teste confirma lookup estrito ausente, candidato presente e
traversal ainda inválido. O consumidor estrito continua recusando Data/.
