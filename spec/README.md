# Specification index

Several notes are written in Portuguese. Layout facts cite their sources;
runtime behaviour and candidate rendering rules are kept apart from them.
Measurement datasets and photographs of the game are not distributed.

| File | Scope |
| --- | --- |
| [ba2.md](ba2.md) | BA2 headers, entry/chunk tables, version gates, codecs and DDS reconstruction. |
| [bsa.md](bsa.md) | Bounded BSA headers, folders, filenames, compression and version limits. |
| [citation-roots.md](citation-roots.md) | Which path roots are established for each citation type and which remain hypotheses. |
| [collision.md](collision.md) | FO4 collision blocks, the census of their opaque Havok payloads and the open questions for a decoder. |
| [collision-census.tsv](collision-census.tsv) | Counts and small stored header values from the collision census of one installation. |
| [havok-container.md](havok-container.md) | Container level of the Havok collision payload (header, sections, class names, data tables), measured over one installation. |
| [havok-objects.md](havok-objects.md) | Object spans, per-class leading sizes and the convex polytope layout of the Havok collision payload, measured over one installation. |
| [havok-bodies.md](havok-bodies.md) | Array element sizes, the body array of the top-level object (shape link and placement), Body ID against it, and the sphere, capsule, convex, scaled convex and compound shape layouts, measured over one installation. |
| [havok-mesh.md](havok-mesh.md) | `hknpCompressedMeshShape` and its data object: array slots, sections, quantized section and shared vertices, byte 92, four-index primitives and their triangles, box trees, vertex records, units and triangles against render meshes, measured over one installation. |
| [havok-container-census.tsv](havok-container-census.tsv) | Counts, holds/fails of container, object, body, shape and compressed-mesh properties, class names and signatures from the Havok census. |
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
| [record-layouts.md](record-layouts.md) | Provenance and scope of independently checked typed-record field measurements. |
| [record-layouts.tsv](record-layouts.tsv) | Typed subrecord layouts tied to field/byte observations and raw preservation. |
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

## Registered rules

Every behaviour that is not a documented format fact is a named rule in
[hypotheses.tsv](hypotheses.tsv). A rule is `pending` until a measurement of
the running game supports or refutes it, and `supported` only within the scope
its row records. The code reads this file: supported rules count at the
`Measured` level, pending ones only at the `Hypotheses` level, and a refuted
rule blocks whatever relied on it. A test keeps the table below in step with
the registry.

| Rule | Status | Supporting measurement or next measurement |
| --- | --- | --- |
| `modt_not_in_frame_v1` | supported | Three bases photographed with original, absent and foreign MODT in separate launches plus a repeat of the original: identical root world words; object pixels differ no more than between the two original launches once the whole-frame brightness offset between launches is removed. |
| `bgsm_blend_factors_alpha_function_v1` | pending | Photograph the three tuples over known background; compare source/destination contributions and transparency. |
| `refr_rotation_order_v1` | refuted | Measure world matrices for X/Y/Z 30-degree and combined XYZ placements; compare every coefficient before photo fitting. |
| `editor_marker_hidden_v1` | supported | Flagged ACTI, DOOR and STAT controls absent in photographs; the unflagged shared-model twin is visible. |
| `refr_absent_xscl_identity_v1` | supported | Six bases, 18 raw transforms: absent XSCL and explicit 1.0 both 3F800000; explicit 0.5 is 3F000000. |
| `scol_parts_static_frame_v1` | pending | Compare loaded SCOL geometry, material bindings and world matrices with transformed constituent NIFs; any visual or geometric difference rejects equivalence. |
| `asset_path_data_prefix_strip_v1` | supported | Controlled object with an absolute Data-prefixed citation appears textured. |
| `asset_path_missing_root_v1` | supported | Weak sample: rootless-citation specimen is only available in an unqualified cropped capture. |
| `nif_backface_cull_v1` | pending | Single-sided sign photographed close from both sides in one launch, in two placements: drawn from the front, absent from the back. The two-sided case is unmeasured: the glass specimen was invisible from both sides. |
| `nif_editor_marker_export_suffix_v1` | supported | One mixed ordinary/helper model photographed and drawn with and without the suffixed subtree: the photograph matches the drawing without it. |
| `refr_rotation_zyx_v2` | supported | Fourteen measured roots; unique among 24 order/sense/storage candidates below 1e-5; max error 2.1e-7, runner-up 1.43. |
| `asset_path_data_relative_strip_v1` | supported | Controlled object with a Data-relative citation appears textured. |
| `effect_color_only_v1` | pending | Compare instance032 and Bench2 light/dark controls; unsupported falloff/grayscale/refraction/soft effects remain refused. |
| `nif_editor_marker_named_hidden_v1` | supported | One mixed ordinary/helper model photographed and drawn with and without the named subtree: the photograph matches the drawing without it. |
| `nif_visibility_editor_marker_hidden_v1` | supported | One measured interior with four placements of two shrub models: the helper boxes drawn from this mesh are absent in the photographs. |
| `nif_matrix_runtime_row_bridge_v1` | pending | Compare both storage orders for all 94 uniquely matched node locals in two pauses; retain raw words, 18 nonunique requests and independent changing nodes. Independent point/pose evidence is needed before changing the renderer. |
| `nif_physics_system_blocks_player_v1` | pending | In one purpose-built interior, place a STAT whose model has a physics system and a STAT whose model has none; walk the player into each and record whether the player position stops at the surface or passes through. Repeat in a second launch. |
| `nif_collision_body_id_selects_body_v1` | pending | In a model with several objects on one bhkPhysicsSystem, displace each body in the running game (hit or push it) and record which node's runtime transform follows; every Body ID must map to its own Target node. |
