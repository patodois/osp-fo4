# Fatos e regras de referência da validação do leitor

O formato e as regras de caminhos permanecem em `citation-roots.md`. As regras de normalização e seus vereditos medidos constam de
[hypotheses.tsv](hypotheses.tsv); previsões são congeladas antes do teste. Só se consulta o vencedor de uma chave VFS canônica,
com a precedência explícita do manifest. Arquivos sombreados não introduzem
ambiguidade de chave. Não se busca por basename ou fora do VFS. O censo de
formas cobre citações interpretadas de todos os plugins ativos e de todos os
NIF/BGSM/BGEM vencedores; slots opcionais vazios são contados separadamente.
Bytes não interpretados não são fingidos como caminhos.

Fonte: [nifxml, definições de dados](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml).
`Fallout4ShaderPropertyFlags2` nomeia Double_Sided no bit 4, especificamente FO4.
`StencilDrawMode` documenta CCW/CW/BOTH; isso não fecha a regra de prioridade
material/NIF no motor. A lista conserva a palavra NIF e o WireBool two_sided do
material (campo do port já atribuído em `material.md`). A opção candidata usa
material quando presente, senão o bit NIF; CCW visto pelo observador é frente
(área negativa em tela Y para baixo). A reflexão da transformação afeta a área
projetada naturalmente. `nif_backface_cull_v1` permanece hipótese, off por padrão.
Não se substitui declaração ausente por single-sided: cull sem dados é erro nomeado.

Fonte: [PNG W3C, terceira edição](https://www.w3.org/TR/png-3/), seções 5–9 e 11:
assinatura, framing, CRC de tag+conteúdo, IHDR, PLTE, tRNS, IDAT único stream zlib,
IEND e os cinco filtros. O leitor original aceita grey/RGB/indexed/grey-alpha/RGBA
8-bit não interlaced. Outros cabeçalhos/critical chunks são erros nomeados;
ancillary chunks são conservados crus. Range, produto, contagem, orçamento,
CRC, ordem e tamanho descomprimido exato são conferidos antes de publicar pixels.
Não se transforma a cor lida das fotos e não se interpreta perfil ausente como
medição de curva. Máscara de comparação exige grey opaco 0/255.

Fonte: [ICC, sRGB segundo IEC 61966-2-1](https://www.w3.org/Graphics/Color/srgb):
a transferência para escrita usa 12.92*x até 0.0031308 e
1.055*x^(1/2.4)-0.055 acima. Não há ajuste de exposição ou parâmetro aprendido.
São entregues PNG cru e a transferência padrão aplicada ao mesmo framebuffer.
Isso é uma variante de saída, não certificação de pipeline linear do jogo.
Por textura conservam-se FourCC e DXGI declarados; DDS legado sem declaração
sRGB recebe esse rótulo explícito em vez de ser chamado de linear.

Algoritmo próprio: o sampler nearest anterior permanece idêntico. Bilinear faz
repeat e interpola quatro centros de texels. A área de um pixel na textura vem
do determinante das derivadas analíticas do UV em perspectiva. LOD relativo é
round(0.5*log2(max(1,área))); escolhe o nível declarado disponível até esse LOD.
A lista prefere mip base até 256 e carrega os mips declarados mais grossos que
cabem no teto agregado de 96 MiB. Se não houver mip até 256, usa o menor nível
declarado, somente se couber nesse mesmo teto; um mip ausente não é criado. Não se inventam mips; níveis usados por textura
são contados por amostra. Não há anisotropia ou trilinear.

Algoritmo próprio de comparação, fixado antes das corridas: gradiente RGB é o
máximo absoluto entre canais e quatro vizinhos, limiar 32/255. Bordas de instância
são mudanças de índice; bordas de profundidade usam salto maior que
max(1 unidade, 1% da menor profundidade). Background/foreground também é borda.
Só pares de pixels incluídos pela máscara formam bordas. A distância é euclidiana
exata à borda mais próxima da foto, calculada por mínimos separáveis de dx²+dy²;
mediana usual e p95 por nearest-rank. Mapas, lado a lado e overlay conservam a
câmera e o registro. Vermelho=motor, ciano=foto, amarelo=coincidência; mapa de
distância satura só a visualização em 16 pixels, nunca os valores float ou métricas.
Cores médias por instância usam a região de índice do motor incluída pela máscara,
com contagem de pixels, nos dois PNGs, e não são critério de prontidão.

`F4RL0002` conserva o corpo v1 e acrescenta ao final, por textura, contagem de
mips, width/height/mip/count u32 LE e RGBA f32 LE; por draw, two_sided u8
(255 ausente) e tag 0/1 + flags NIF u32 se presente. `F4RL0001` continua legível,
com ausência explícita dessas declarações. Ambos rejeitam cauda e contagens/ranges
inválidos antes de reservar.


A hipótese adicional `nif_editor_marker_export_suffix_v1` foi anunciada após o
censo e antes dos novos quadros: há geometria com nome `EditorMarker` seguido de dígitos ASCII e/ou dois-pontos e dígitos, independente do nó homônimo, com Source Texture vazio. Não se
assume sem fonte que todo sufixo é mero nome de exportação: isso é hipótese
separada. A Scene conserva dois diagnósticos, exigindo também a hipótese E de
ocultação. O renderer só omite a forma se ambas estiverem no conjunto. Nenhum
outro prefixo de nome ou arquivo é generalizado.

Mapas compactos da bancada usam um stream zlib próprio, sem perda, de bytes
u32 LE ou float32 LE. O JSON declara a quantidade exata de bytes descomprimidos;
`.zlib` indica esse transporte. O mesmo leitor exato rejeita stream curto/longo,
checksum inválido e cauda. Não se quantiza profundidade nem índice.

O comparador legado de descontinuidades por profundidade descrito acima continua
disponível. A comparação atual por planos de triângulo e a seleção persistente
da foto são definidas separadamente em [photo-metrics.md](photo-metrics.md).
