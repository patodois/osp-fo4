# DDS derivado de BA2 DX10

- https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header : tamanho 124, flags/caps e campos do header.
- https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-pixelformat : tamanho 32, FOURCC DX10.
- https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header-dxt10 : extensão de 20 bytes, enum DXGI, dimensão TEXTURE2D=3, cube misc flag=4, arraySize conta cubos (1 para um cubo).
- https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dx-graphics-dds-pguide : magic DDS, total 148 bytes com DX10, pitch BC em blocos 4x4, BC1/4 de 8 bytes e outros BC de 16; não BC por bits por pixel.
- https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format : números e componentes dos formatos; a implementação admite formatos lineares de bytes inteiros e BC1–7; packed/planar/1-bit não implementados retornam UnsupportedDxgiFormat.
- https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-file-layout-for-cubic-environment-maps : faces +X,-X,+Y,-Y,+Z,-Z, cada face com todos os mips; cubo quadrado, arraySize=1.

Mip count válido limitado pelas dimensões é política de validação. Campos DDS
reservados zero são exigência do container, não conteúdo de jogo fabricado.
Cube com múltiplos chunks é bloqueado por UnsupportedCubeChunkLayout enquanto
não houver fonte independente para a ordem de faces dentro dos chunks BA2.

## Leitor DDS da fase 2

Fontes primárias: [DDS_HEADER](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header), [DDS_PIXELFORMAT](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-pixelformat), [DDS_HEADER_DXT10](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-header-dxt10), [DXGI_FORMAT](https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format), [layout de texturas/mips](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-file-layout-for-textures), [cubemaps](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-file-layout-for-cubic-environment-maps), [guia DDS](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dx-graphics-dds-pguide).

Essas tabelas sustentam magic DDS, header 124/pixel format 32, dimensões, mips, flags/caps, fourcc e máscaras; extensão DX10 de cinco u32, resource dimension, misc cube, array size e alpha mode; tamanhos BC por blocos 4x4, mínimo de um bloco, ordem de array/face/mip, faces +X/-X/+Y/-Y/+Z/-Z, faces ausentes no formato antigo, depth slices em volumes. Mip count zero significa uma superfície sem cadeia adicional. Flags reservadas e campos do header ficam disponíveis crus; formato não suportado é erro, não fallback.

`fo4-texture` lê BC1–BC7 (incluindo signed BC4/5 e BC6H), sRGB como metadado, RGBA/BGRA/BGRX/R/RG e floats RGBA16/32 conforme IDs/máscaras documentados. Pitch não compacto em formatos crus é erro nomeado; não se infere alinhamento. Dados extras ou truncados e dimensões/contagens incoerentes são erros. O decoder usa exclusivamente [bcn-decode 0.1.0](https://docs.rs/bcn-decode/0.1.0/bcn_decode/) (MIT OR Apache-2.0), com licença permissiva, com a API de buffers fornecidos. f32 preserva HDR/signed; não há conversão automática de espaço de cor.

O CLI decodifica todos os mips/faces/slices em faixas de até quatro linhas, liberando/reutilizando o buffer limitado, sem reter uma imagem inteira. A API também permite materializar uma superfície individual sob limite. Texturas BA2 DX10 usam a reconstrução DDS existente, sem alterar os campos derivados do registro. Não se utilizou código de leitores DDS/DirectXTex.

A regressão BGR24, encontrada nos LUTs FO4 e tintmasks SSE, utiliza apenas as máscaras documentadas em DDS_PIXELFORMAT e a ordem little endian: R=0xFF0000, G=0xFF00, B=0xFF, 24 bits. Teste vermelho `documented_24_bit_bgr_masks_decode_in_byte_order` antecede o suporte; não se usou código de leitor.
