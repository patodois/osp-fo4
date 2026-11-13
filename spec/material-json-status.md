# Materiais JSON do Startup: limite da referência permitida

Na retomada de reviewed measurement decision, os cinco arquivos de Materials/Interface que davam
InvalidMaterialMagic foram lidos novamente pelo VFS, somente do archive
Fallout4 - Startup.ba2 identificado como vencedor na corrida integral.
Uma ferramenta local própria, offline, verificou o JSON genérico em memória;
não executou código da referência e não gravou o conteúdo dos assets.

Os cinco são objetos JSON válidos, de 1.322 a 1.396 bytes. Não há chave Version
nem Type, sem distinguir caixa. HUDGlass e HUDMeshRender têm 43 propriedades;
HUDGlassFlat, LoadScreenShadow e ModMenuShadow têm 45. Nomes, tipos JSON,
tamanhos e hashes estão em `local-measurement-artifact`; a ferramenta
e seu log também permanecem locais, ignorados pelo Git. Isso identifica o
enquadramento textual, sem atribuir a ele uma versão binária.

O conteúdo não corresponde integralmente aos DataMember da MaterialLib
fixada em `21411873f17454a2442d6785d533499e14c63adb`. Os seguintes nomes aparecem
nos cinco objetos, mas não nos contratos de BaseMaterialFile/BGEM desse SHA:

- bBaseTextureEnable;
- bEnvmapMaskTextureEnable;
- bEnvmapTextureEnable;
- bGrayscaleTextureEnable;
- bNormalTextureEnable;
- bRefractionFalloff;
- sGreyscaleTexture.

Há divergências verificáveis, não uma regra de alias fornecida pela fonte:
[BaseMaterialFile.cs:104](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BaseMaterialFile.cs#L104)
declara fRefractionFalloff, e
[BGEM.cs:51](https://github.com/ousnius/Material-Editor/blob/21411873f17454a2442d6785d533499e14c63adb/MaterialLib/BGEM.cs#L51)
declara sGrayscaleTexture. Os cinco campos Enable acima não têm equivalente
declarado nesses contratos. A árvore de MaterialLib foi conferida por nomes:
contém somente os três arquivos C# já autorizados e MaterialLib.csproj; não
há outro leitor JSON nessa pasta. Não se abriu a interface ou outro leitor
proibido para preencher a falta.

O leitor binário continua rejeitando essas cinco entradas com arquivo, offset,
assinatura esperada e bytes encontrados. Não as chama de JSON inválido e não
as promove a materiais completos por semelhança dos nomes. Não se inventou
Version=2, defaults, aliases ou semântica para os toggles Enable. As referências
internas desses cinco arquivos continuam incluídas em files_not_fully_checked.

A leitura semântica desse outro enquadramento fica **esgotada dentro da
referência e das permissões desta etapa**, conforme a alternativa autorizada
pelo pedido de continuação. O suporte aos layouts binários da MaterialLib e
os notices IEEE estão implementados; não se afirma suporte a esse schema JSON.
