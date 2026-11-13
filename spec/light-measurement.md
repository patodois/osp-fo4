# Proposta de medição de luz — validação do leitor

Nenhuma equação abaixo é usada pelo motor. São famílias candidatas para uma
medição futura, sujeitas a veredito de medição autorizada e a câmera medida em bits.

A bancada 3 contém a mesma instalação em A/B/C: piso plano de 2048 por 2048,
quatro vértices, normal/tangente constantes, textura RGB128 uniforme, sem
emissão, specular ou environment mapping habilitados no BGSM. Esses valores
são escolhas de autoria nos layouts já documentados, não defaults do leitor.
Há backing de colisão do mestre abaixo da superfície visual; não usar o
assentamento do jogador para definir a câmera. Validar a câmera após posicionar.

A base LIGH selecionada tem raio nominal256 e RGB255, flags cruas1, sem Flicker
ou Pulse na palavra observada. Unknown0 e a classificação pontual/spot continuam
pendentes: medir classe RTTI/flags completos e forma do campo iluminado antes
de interpretar fisicamente qualquer curva. Não chamar ausência de tipo de
“point light”, nem assumir que raio nominal significa corte real.

Há seis REFR dessa base na mesma vertical, alturas R vezes 0,125/0,25/0,5/0,75/
1/1,5, inicialmente desabilitados. Habilitar exatamente um por captura e
retorná-lo a disabled antes do próximo; a condição off é controle. Evita-se
pressupor que uma luz vizinha tem contribuição nula fora do raio. A câmera
vertical permanece fixa, ângulos inteiros (90,0,0), com pares repetidos.
Repetir em três células com RGB ambiente autorado 0/64/96, seis direções iguais,
cor direcional e especular ambiente zero, Inherits0, sem LTMP. Os demais bytes
XCLL observados ficam transportados na proveniência, sem uma equação de herança.

Medir o centro do piso, diretamente abaixo da luz: a normal e o ângulo de
incidência permanecem constantes ao variar altura. Capturar também o perfil
radial, registrando distância geométrica e cosseno separadamente; esse perfil
testa anisotropia/spot, sem confundí-la com a queda radial. Repetir ordem de
alturas em ida/volta, fixar hora/estado, e registrar alterações de exposição,
tone mapping, bloom, adaptação e qualquer flicker observado. Um par estável
não demonstra ausência de variação lenta.

Exigir leitura linear/HDR calibrada, ou medir primeiro uma rampa neutra e a
função de exposição/tone mapping. RGB8 da foto, sozinho, não identifica uma
curva física. Os controles off medem ambiente+postprocessamento; subtração
só deve ocorrer depois dessa calibração, sem ajustar exposição por imagem.

Previsões normalizadas no centro, q=d/R, ancoradas em q=0,5:

| q | linear com corte max(1-q,0) | potência com corte, e=2 | inverso do quadrado sem corte |
|---:|---:|---:|---:|
| 0,125 | 1,75 | 3,0625 | 16 |
| 0,25 | 1,5 | 2,25 | 4 |
| 0,5 | 1 | 1 | 1 |
| 0,75 | 0,5 | 0,25 | 0,4444 |
| 1 | 0 | 0 | 0,25 |
| 1,5 | 0 | 0 | 0,1111 |

Inverso do quadrado com corte mantém a previsão interna e prevê zero em q>=1.
Expoente livre será um parâmetro medido, não tomado do nome Falloff Exponent.
No regime linear calibrado, ambiente aditivo prevê diferença on-off constante
entre ambientes; máximo prevê platô até L superar A; multiplicativo puro prevê
zero com A=0 e razões proporcionais ao ambiente. Modelos mistos exigem mais
condições, e as intensidades RGB de XCLL não são tratadas como luminância linear.
A combinação e o alcance efetivos continuam medições nomeadas, sem shading.
