# Editor helper visibility

NIF field layout and mesh/shader links are documented by
[nifxml](https://raw.githubusercontent.com/niftools/nifxml/develop/nif.xml):
NiAVObject.Name, BSTriShape, BSEffectShaderProperty and NiAlphaProperty. Names
are data, not visibility flags. The named rules in hypotheses.tsv remain pending.

A measured interior exposed green wire boxes. Pixel instance attribution and
NIF graph inspection isolate BSTriShape meshes named exactly
`VisibilityEditorMarker:0`, with an explicitly empty effect texture and green
base color. The separate, prospectively announced hypothesis
`nif_visibility_editor_marker_hidden_v1` omits only that literal mesh name in
hypothesis mode. Strict classification retains an unverified visibility error;
all bytes and offsets remain accessible. This does not decide the two distinct
`EditorMarker` subtree rules. The diagnostic build API can retain these helper
meshes explicitly for comparisons. There is no RGB filter or fabricated texture.
