# Controladores: contrafactual de transformações gravadas

Este inventário não libera animação. O contrafactual estreito retira somente
diagnósticos de manager, sequence, multi-target/transform/keyframe controller,
transform interpolator/data, default AV-object palette e text keys. Exige formas
decodificadas, não marcador inteiro, nenhuma outra causa e mantém à parte os
conjuntos de hipóteses já registrados. Float/bool/visibility, behavior graphs,
skinning, colisão e blocos desconhecidos não se tornam transformações estáticas.
Controladores de propriedade de shader são contados separadamente e excluídos,
mesmo quando coexistem com controladores de transformação.

Contagens de referências do censo de bases são usos de bases por records,
não uma certificação das colocações: DATA, escala, estado e campos de cada REFR
ainda precisam passar pela Scene. As contagens de interiores e usos ativos
também são preservadas; não se somam os subconjuntos.

Medição proposta: congelar
base, hash do NIF, load order, referência e estado pausado; guardar duas capturas
originais. Associar cada nó por nome, cadeia de pais e slot de filho, sem usar
apenas ponteiros ou nomes repetidos. Comparar as palavras float32 de cada
transformação **local** R/t/s do jogo com as do NIF. A transformação da raiz
externa de colocação é evidência separada; não confundir world e local, nem
interpretar rotação SDK como DATA. Guardar diferenças por componente e nós
ausentes, ambíguos, ilegíveis ou não finitos como falha/indisponibilidade.

Zero diferenças em duas pausas é evidência daqueles estados e instâncias,
não prova de que o controlador nunca alterará o objeto. Divergência rejeita o
contrafactual para aquela observação. Sem captura, nenhum candidato é aceito.
Shader controllers requerem outra medição de aparência e continuam fora.


A comparação publica duas ordens separadamente: palavras na sequência crua do
NIF e transposição das nove palavras de rotação. nifxml define Matrix33 como
column-major; a documentação do coletor define três linhas armazenadas do SDK,
sem estabelecer uma convenção de composição. A ponte é uma hipótese diagnóstica
pendente (`nif_matrix_runtime_row_bridge_v1`). Não altera `Transform::from_fields`
nem libera controladores. Diferenças de armazenamento não devem ser atribuídas
silenciosamente à execução de animação. Mudanças entre as duas pausas continuam
mudanças independentemente dessa normalização.
