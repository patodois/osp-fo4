# Inventário de formatos sem leitor (validação do leitor)

Sem semântica binária de HKX/animtextdata. decisão de medição revisada §5 define as medidas.
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
