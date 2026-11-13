# Specification index

Format notes may be in Portuguese. Layout facts require cited sources; runtime
behavior and candidate rendering rules stay separate. Historical local reports
and measurement datasets are not distributed.

| File | Scope |
| --- | --- |
| [ba2.md](ba2.md) | BA2 headers, entry/chunk tables, version gates, codecs and DDS reconstruction. |
| [bsa.md](bsa.md) | Bounded BSA headers, folders, filenames, compression and version limits. |
| [citation-roots.md](citation-roots.md) | Which path roots are established for each citation type and which remain hypotheses. |
| [controller-measurements.md](controller-measurements.md) | Measurement design for comparing stored NIF locals with paused runtime nodes. |
| [dds.md](dds.md) | DDS headers, formats, surfaces, block layouts and supported decoding boundaries. |
| [helper-visibility.md](helper-visibility.md) | Narrow editor-helper classification and explicit diagnostic visibility variants. |
| [hypotheses.tsv](hypotheses.tsv) | Machine-readable candidate assertions, scope, status and falsification evidence. |
| [light-measurement.md](light-measurement.md) | Light and cell declarations separated from unestablished rendering equations. |
| [manifest-vfs.md](manifest-vfs.md) | Strict manifests, explicit load/mount order, virtual paths, fingerprints and budgets. |
| [material-json-status.md](material-json-status.md) | Material JSON representation and preservation of unknown or unsourced fields. |
| [material-research.md](material-research.md) | Material layout research, source boundaries and the attributed MIT exception. |
| [material.md](material.md) | BGSM/BGEM binary layouts, numeric fields, versions and unsupported semantics. |
| [nif-render-layouts.md](nif-render-layouts.md) | Additional NIF/texture/snapshot layouts used by the CPU reference renderer. |
| [nif.md](nif.md) | NIF framing, blocks, links, meshes, vertex layouts and bounded structural readers. |
| [path-census.md](path-census.md) | Path accounting and independent candidate-resolution comparisons. |
| [photo-metrics.md](photo-metrics.md) | Prospective forward silhouette and persistent reverse-photo metrics, with explicit limits. |
| [plugin.md](plugin.md) | Plugin envelopes, groups, subrecords, compression, FormIDs, overrides and flags. |
| [record-fichas.md](record-fichas.md) | Provenance and scope of independently checked typed-record field measurements. |
| [record-fichas.tsv](record-fichas.tsv) | Typed subrecord layouts tied to field/byte observations and raw preservation. |
| [record-flag-words.tsv](record-flag-words.tsv) | Observed record flag words with the supported, outside and unknown bit partitions. |
| [record-light-key-layouts.md](record-light-key-layouts.md) | Measured record/light fields and schema-backed animation-key structure only. |
| [reference-image-formats.md](reference-image-formats.md) | Camera, DDS/PNG and image-comparison data contracts. |
| [render.md](render.md) | Deterministic CPU rasterization, material bindings, maps and snapshot protocols. |
| [rotation-arithmetic.md](rotation-arithmetic.md) | Supported Euler convention, open exact arithmetic and the bounded numerical investigation. |
| [scene.md](scene.md) | Scene assembly, static-frame classification, source spans and explicit omissions. |
| [static-blocks.tsv](static-blocks.tsv) | NIF block roles in the selected static-frame scope. |
| [static-record-keys.txt](static-record-keys.txt) | Synthetic test contract for the typed-record dispatch keys. |
| [static-records.tsv](static-records.tsv) | Record/subrecord roles for the selected static-frame scope. |
| [strings.md](strings.md) | Localized STRINGS, DLSTRINGS and ILSTRINGS layouts and VFS lookup. |
| [unread.md](unread.md) | Accounting for uninterpreted asset families without claiming their semantics. |

## Hypotheses

Status is a reviewed measurement verdict, not permission to silently erase
diagnostics. Exact scope and reasons remain in the TSV. Pending rules are
explicit candidates; controller rules have not been released.

| Hypothesis | Status | Supporting measurement or next measurement |
| --- | --- | --- |
| `modt_not_in_frame_v1` | pending | Photograph same-model variants and original/control/absent/foreign MODT copies side by side; any repeatable change rejects it. |
| `bgsm_blend_factors_alpha_function_v1` | pending | Photograph the three tuples over known background; compare source/destination contributions and transparency. |
| `refr_rotation_order_v1` | refuted | Measure world matrices for X/Y/Z 30-degree and combined XYZ placements; compare every coefficient before photo fitting. |
| `editor_marker_hidden_v1` | supported | Flagged ACTI, DOOR and STAT controls absent in photographs; the unflagged shared-model twin is visible. |
| `refr_absent_xscl_identity_v1` | supported | Six bases, 18 raw transforms: absent XSCL and explicit 1.0 both 3F800000; explicit 0.5 is 3F000000. |
| `scol_parts_static_frame_v1` | pending | Compare loaded SCOL geometry, material bindings and world matrices with transformed constituent NIFs; any visual or geometric difference rejects equivalence. |
| `asset_path_data_prefix_strip_v1` | supported | Controlled object with an absolute Data-prefixed citation appears textured. |
| `asset_path_missing_root_v1` | supported | Weak sample: rootless-citation specimen is only available in an unqualified cropped capture. |
| `nif_backface_cull_v1` | pending | Only one tiny plate is visible in current views; closer front/back captures are required. |
| `nif_editor_marker_export_suffix_v1` | pending | Mixed ordinary/helper models are visible; compare variants with and without their suffixed subtree. |
| `refr_rotation_zyx_v2` | supported | Fourteen measured roots; unique among 24 order/sense/storage candidates below 1e-5; max error 2.1e-7, runner-up 1.43. |
| `asset_path_data_relative_strip_v1` | supported | Controlled object with a Data-relative citation appears textured. |
| `effect_color_only_v1` | pending | Compare instance032 and Bench2 light/dark controls; unsupported falloff/grayscale/refraction/soft effects remain refused. |
| `nif_editor_marker_named_hidden_v1` | pending | Mixed ordinary/helper models are visible; compare variants with and without their named subtree. |
| `nif_visibility_editor_marker_hidden_v1` | pending | Compare the same measured interior cameras with and without only these meshes; photograph absence and preserve differences. |
| `nif_matrix_runtime_row_bridge_v1` | pending | Both storage orders for 94 locals in two pauses; raw words and 18 nonunique requests retained; no renderer change. |
