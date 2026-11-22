# Specification index

Several notes are written in Portuguese. Layout facts cite their sources;
runtime behaviour and candidate rendering rules are kept apart from them.
Measurement datasets and photographs of the game are not distributed.

| File | Scope |
| --- | --- |
| [actor.md](actor.md) | Placed actors: ACHR, NPC_, RACE, OTFT, ARMO, ARMA and leveled-list layouts measured on the winning records, the chain from a placed actor to a skeleton and addon models, the skeleton's rest pose and skinning, their checks and the counts of drawn and left-out actors; the pose given by a named animation clip, its rules, checks and cost. |
| [ba2.md](ba2.md) | BA2 headers, entry/chunk tables, version gates, codecs and DDS reconstruction. |
| [bsa.md](bsa.md) | Bounded BSA headers, folders, filenames, compression and version limits. |
| [citation-roots.md](citation-roots.md) | Which path roots are established for each citation type and which remain hypotheses. |
| [collision.md](collision.md) | FO4 collision blocks, the census of their opaque Havok payloads and the open questions for a decoder. |
| [collision-census.tsv](collision-census.tsv) | Counts and small stored header values from the collision census of one installation. |
| [havok-container.md](havok-container.md) | Container level of the Havok collision payload and of the `.hkx` files (header and its two variants, sections, class names, data tables), measured over one installation. |
| [havok-objects.md](havok-objects.md) | Object spans, per-class leading sizes and the convex polytope layout of the Havok collision payload, measured over one installation. |
| [havok-bodies.md](havok-bodies.md) | Array element sizes, the body array of the top-level object (shape link and placement), Body ID against it, and the sphere, capsule, convex, scaled convex and compound shape layouts, measured over one installation. |
| [havok-mesh.md](havok-mesh.md) | `hknpCompressedMeshShape` and its data object: array slots, sections, quantized section and shared vertices, byte 92, four-index primitives and their triangles, box trees, vertex records, units and triangles against render meshes, measured over one installation. |
| [collision-placement.md](collision-placement.md) | Placing decoded collision in world space for a cell: collision readiness apart from render readiness, the Target node chain, units, the open body placement rule with its two levels, the API, the `cell-collision` report and its runs. |
| [walk.md](walk.md) | First walking controller over a cell's placed collision (`fo4-walk`): capsule, collide-and-slide, step-up, slope limit, terrain under the walker, the `walk` report (paths and `--drops` point lists) and viewer walk mode (`--terrain` drawing, `--around` neighbour cells, fall recovery, waiting for a cell of the moving grid, scripted `--path` walks); a set of worlds walked as one; every movement number a provisional registered rule. |
| [havok-animation.md](havok-animation.md) | Objects of the `.hkx` animation and skeleton files: top-level entries, animation container, skeleton (bones, parents, reference pose against the skeleton NIFs), binding, animation header, the spline block table and its track data (masks, curves, quantised rotations, values at a time), measured over one installation. |
| [havok-animation-census.txt](havok-animation-census.txt) | Counts and small stored values from the animation census (`anim_census`). |
| [havok-container-census.tsv](havok-container-census.tsv) | Counts, holds/fails of container, object, body, shape and compressed-mesh properties, class names and signatures from the Havok census, for the collision payloads and the `.hkx` files. |
| [controller-measurements.md](controller-measurements.md) | Measurement design for comparing stored NIF locals with paused runtime nodes. |
| [dds.md](dds.md) | DDS headers, formats, surfaces, block layouts and supported decoding boundaries. |
| [helper-visibility.md](helper-visibility.md) | Narrow editor-helper classification and explicit diagnostic visibility variants. |
| [hypotheses.tsv](hypotheses.tsv) | Machine-readable candidate assertions, scope, status and falsification evidence. |
| [land.md](land.md) | LAND subrecord layouts and sources, the terrain rules, the ground texture rules and drawing, the `land` report and its census of one worldspace. |
| [light-measurement.md](light-measurement.md) | Light and cell declarations separated from unestablished rendering equations. |
| [manifest-vfs.md](manifest-vfs.md) | Strict manifests, explicit load/mount order, virtual paths, fingerprints and budgets. |
| [material-json-status.md](material-json-status.md) | Material JSON representation and preservation of unknown or unsourced fields. |
| [material-research.md](material-research.md) | Material layout research, source boundaries and the attributed MIT exception. |
| [material.md](material.md) | BGSM/BGEM binary layouts, numeric fields, versions and unsupported semantics. |
| [nif-render-layouts.md](nif-render-layouts.md) | Additional NIF/texture/snapshot layouts used by the CPU reference renderer. |
| [pex.md](pex.md) | Compiled scripts (`.pex`): header, string table, debug information, user flags, objects, structs, variables, properties, states, functions and instructions as measured over every entry of one installation; opcode arities established by alternative readings, argument kinds, the claimed opcode roles and their checks, the disassembly. |
| [pex-census.txt](pex-census.txt) | Counts from the `.pex` census (`pex_census`). |
| [nif.md](nif.md) | NIF framing, blocks, links, meshes, vertex layouts, node rotation words and bounded structural readers. |
| [vm.md](vm.md) | The script interpreter `fo4-vm`: what runs, the candidate meaning of each opcode with the checks that could refute it, the recording host and its assumptions, the data host (natives answered from the records), binding by VMAD and events with scripts of quests, aliases and references attached on need, the census of handlers and of what blocks them, the viewer's `--script-event`. |
| [vm-checks.txt](vm-checks.txt) | Counts from the opcode checks (`vm_checks`): slot types, data flow, idioms, execution under each reading. |
| [vm-census.txt](vm-census.txt) | Counts from the handler census (`script_run --census`): the test cells, every scripted DOOR, TERM and ACTI base, and every placed scripted reference, each before and after the data host. |
| [vm-blockers.txt](vm-blockers.txt) | The natives whose result blocked handlers, ranked, the kinds of objects without a script instance, and the data host's answers, from the handler census, before and after the data host. |
| [vm-host-data-census.txt](vm-host-data-census.txt) | Counts from the census of the data the data host reads (`host_data_census`): XLKR, NAME, the initially-disabled flag, FLTV, LNAM, record types against declared property types. |
| [save.md](save.md) | Save files (`.fos`): the container read to the last byte (header, screenshot span, versions, plugin lists, location table, three global data tables, change-form table, form id array, second array, closing block), change-form entries by type, version, compression and flag bits, form references resolved against the plugin data under alternative readings, the moved-reference prefix checked against plugin placements; what stays raw and open. |
| [save-census.txt](save-census.txt) | Counts from the save census (`save_census`): no content of any save. |
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
| [scene.md](scene.md) | Scene assembly, static-frame classification, source spans and explicit omissions; exterior cells around a cell joined into one scene; the viewer's moving grid (a viewer policy) and its credits. |
| [static-rotation-census.txt](static-rotation-census.txt) | Counts from the census of node rotations in drawn models and their OBND boxes (`static_rotation_census`). |
| [static-blocks.tsv](static-blocks.tsv) | NIF block roles in the selected static-frame scope. |
| [static-record-keys.txt](static-record-keys.txt) | Synthetic test contract for the typed-record dispatch keys. |
| [static-records.tsv](static-records.tsv) | Record/subrecord roles for the selected static-frame scope. |
| [strings.md](strings.md) | Localized STRINGS, DLSTRINGS and ILSTRINGS layouts and VFS lookup. |
| [vmad.md](vmad.md) | VMAD: the scripts attached to records and their property values, the TERM fragment block, the QUST block of stage fragments and aliases, object values with an alias index, cross-checks against the `.pex` declarations, and the survey of scripts on doors, terminals and activators with the native functions their code reaches. |
| [vmad-census.txt](vmad-census.txt) | Counts from the VMAD census (`vmad_census`). |
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
| `nif_matrix_runtime_row_bridge_v1` | pending | Compare both storage orders for all 94 uniquely matched node locals in two pauses; retain raw words, 18 nonunique requests and independent changing nodes. Offline point evidence (OBND boxes, skeleton bones) moved the renderer to the row reading; photograph one placed model with a shape under two non-commuting node rotations beside its drawing: the other reading must misplace that shape. |
| `nif_physics_system_blocks_player_v1` | pending | In one purpose-built interior, place a STAT whose model has a physics system and a STAT whose model has none; walk the player into each and record whether the player position stops at the surface or passes through. Repeat in a second launch. |
| `nif_collision_body_id_selects_body_v1` | pending | In a model with several objects on one bhkPhysicsSystem, displace each body in the running game (hit or push it) and record which node's runtime transform follows; every Body ID must map to its own Target node. |
| `havok_collision_units_69_99125_v1` | pending | Measured for one polytope box (top at 176 and 352 at reference scales 1 and 2, 70 excluded); repeat for compressed meshes, spheres, capsules and compounds. |
| `havok_shape_in_target_frame_v1` | pending | Reference scale and rotation sense indicated for a polytope; rest an object on a model whose Target is a child node with its own transform. |
| `havok_body_placement_ignored_v1` | pending | Default level. Measured for one polytope slab, indicated for meshes; rest objects on mesh and compound bodies with a stored placement and compare both levels. |
| `havok_body_placement_applied_v1` | pending | Contradicted for one polytope slab; order w, x, y, z refuted. Same measurement as the ignored level. |
| `havok_compound_instance_v1` | pending | Rest an object on a compound instance with a non-identity rotation; the transposed rotation must predict worse. |
| `havok_scaled_convex_v1` | pending | Rest an object on a scaled convex with unequal scale or non-zero translation; the other order must predict worse. |
| `havok_mesh_triangles_surface_v1` | pending | Contact heights along a non-planar quad from above and below; sidedness open (one plane of a two-plane mesh was passable from above). |
| `havok_convex_radius_added_v1` | pending | Measured for one polytope slab (rests at 96.000, not 95.300; scales with the reference); repeat with other radii and scales. |
| `mswp_collision_unchanged_v1` | pending | Indicated by one live drop comparison (one filtered swap); two references of one model, with and without a filtered swap: drop and walk into both. |
| `render_inputs_collision_unchanged_v1` | pending | One model placed as shipped and as a copy whose material citations name missing files: drop and walk into both. |
| `land_single_cell_child_v1` | pending | Census of LAND count and group kind per exterior cell; live: ground with the cell's LAND deleted or duplicated by an overlay plugin. |
| `land_vhgt_row_accumulation_v1` | pending | Offline edge continuity against transposed and mirrored alternatives; live: `player.getpos z` after settling at several points of one exterior cell. |
| `land_vhgt_units_8_v1` | pending | Offline: placed objects' lowest collision point against the ground under them for factors 4, 8 and 16; live: `player.getpos z` at several points. |
| `land_grid_origin_southwest_v1` | pending | Offline edge continuity under the eight symmetries of the square; live: `player.getpos` at the four corners of one cell. |
| `land_grid_spacing_128_v1` | pending | Offline objects against ground; live: `player.getpos` at a cell corner and at a quad centre. |
| `land_quad_diagonal_sw_ne_v1` | pending | Live: settle at the two off-diagonal points of a non-planar quad; the other level must predict worse. |
| `land_quad_diagonal_nw_se_v1` | pending | Same measurement as the sw_ne level. |
| `land_quadrant_layout_v1` | pending | Top-down screenshot of a cell whose four quadrants have distinct base textures; offline: quadrant seams agree for 84.3% under the candidate, 51.4% or less otherwise. |
| `land_vtxt_position_grid_v1` | pending | Photograph a layer painted in an asymmetric pattern; offline: cell edges agree for 80.6% under the candidate, 49.2% or less under the other symmetries. |
| `land_layer_weights_residual_v1` | pending | Photograph two layers of flat colours at 0.5 and 0.5, then 0.25 and 0.25, over a third base; offline: opacities sum to at most 1 at all but 61 of 9,087,604 vertices. |
| `land_missing_base_default_v1` | pending | Photograph a quadrant without BTXT and a FormID-zero layer at full opacity in one launch; which texture the default is is the next rule's candidate. |
| `ltex_tnam_texture_set_tx00_v1` | pending | Photograph a cell before and after an overlay plugin points an LTEX TNAM at another TXST. |
| `ltex_texture_set_material_diffuse_v1` | pending | Photograph ground of a material-only texture set before and after an overlay plugin gives its material a distinctive diffuse; offline: all 18 such materials resolve and decode. |
| `land_default_texture_world_named_v1` | pending | Photograph default ground, then point the named texture set at a distinctive material with an overlay plugin; offline evidence is the editor ID only (seams and LOD tiles do not discriminate). |
| `land_texture_repeat_256_v1` | pending | Placeholder. Top-down screenshot of flat ground: count texture repeats over a known distance and check the image's orientation. |
| `land_vclr_multiply_v1` | pending | Photograph ground across a VCLR gradient and the same cell with VCLR removed; offline: shared cell edges byte-equal for 99.4% of non-white pairs. |
| `walk_capsule_radius_v1` | pending | Walk the player into a box face of known plane and into a thin pole; the stop distances equal the radius. |
| `walk_capsule_height_v1` | pending | Slabs over a corridor at decreasing clearances; the lowest clearance walked under standing. |
| `walk_eye_height_v1` | pending | Look level at markers of known height on a far wall from a floor of known height; the marker on the centre row. |
| `walk_step_height_v1` | pending | Staircase of boxes with risers 8 to 32 units; the highest riser walked onto, getpos z on each tread. |
| `walk_max_slope_v1` | pending | Ramps of rotated boxes 30 to 60 degrees; the steepest walked up and whether the next slides. |
| `walk_gravity_v1` | pending | Falls from ledges of known height timed from frame captures; compare with 0.5 g t^2. |
| `walk_speed_walk_v1` | pending | Walk between two markers a known distance apart; time from frame captures, getpos at both ends. |
| `walk_speed_run_v1` | pending | Same as the walk speed, running. |
| `walk_no_air_control_v1` | pending | Walk off a high ledge holding the opposite direction in the air; compare landing points. |
| `walk_triangles_block_both_sides_v1` | pending | Stand on, under and walk into single-plane mesh surfaces of both windings from each side. |
| `achr_placement_as_refr_v1` | pending | Two copies of one actor base with XSCL 1 and 2 and one with a changed NAM6: compare getpos and head heights; only XSCL may change the size. |
| `npc_appearance_candidates_agree_v1` | pending | Toggle each template bit of an actor whose race differs from its template's; place a leveled actor list whose entries differ only in skin and check one entry appears. |
| `npc_default_outfit_worn_v1` | pending | An actor with a DOFT outfit of one ARMO and another SOFT outfit, photographed standing: the DOFT armour must be worn. |
| `actor_acbs_bit0_second_section_v1` | pending | Two actors of one race and outfit differing only in ACBS bit 0: the one with the bit set must show the MOD3 models. |
| `armo_race_addons_drawn_v1` | pending | An ARMO with two addons for the actor's race, each model replaced in turn by an overlay plugin: both must show. |
| `actor_worn_slots_hide_skin_v1` | pending | A body armour worn, then its BOD2 bits cleared by an overlay plugin: the skin body must appear only in the second. |
| `actor_skin_linear_blend_v1` | pending | A posed actor photographed against a render of the same pose from bone transforms read in game; another vertex blend shows at bent joints. |
| `actor_clip_skeleton_beside_nif_v1` | pending | A race's skeleton .hkx replaced by an overlay copy with permuted bone order: a known idle must break as the permutation predicts. |
| `actor_clip_pose_by_bone_name_v1` | pending | An actor frozen in a known idle captured beside a render of the same clip time: silhouettes and extremities at two times. |
| `pex_ops_arithmetic_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_ops_compare_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_ops_not_copy_cast_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_conditional_jump_sense_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_call_dispatch_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_op_return_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_op_string_concat_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_property_get_set_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_array_ops_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `pex_struct_ops_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) runs the opcodes on known operands and shows each result through a notification call when activated; any result other than the rule's rejects it. |
| `npc_head_parts_unmorphed_v1` | pending | Photograph, in one launch, a placed actor without morphs (its NPC_ with no face morph or tint fields) from the front beside its drawing: face, eyes, hair and beard must sit where drawn within a few units; then the same actor with a hat whose addon is skinned to the head bone: whether the hair is still drawn decides the covering clause. |
| `npc_leveled_list_flags_v1` | pending | Place, by an overlay plugin, an actor whose base is an LVLF 3 list of two NPC_ that differ only in skin; reload the cell several times: if one of the entries always appears, the rule's premise (one entry, its look) holds; then a list of identical entries must always show that look. |
| `npc_template_all_bits_v1` | pending | Place, by an overlay plugin, an NPC_ with every template bit set whose own RNAM, WNAM and DOFT differ from its template's; photograph it: it must show the template's race, skin and outfit. |
| `pex_default_values_v1` | pending | A hand-assembled test script (a loose .pex attached by an overlay plugin to an activator in a purpose-built cell) reads an unwritten local, a none-valued variable and a new array element of each simple type and shows them through a notification call when activated; any other value rejects the rule. |
| `pex_truth_of_values_v1` | pending | A hand-assembled test script branches on each kind of value (zero and non-zero numbers, empty and non-empty text, none and a handle, an empty and a filled array) and shows the branch taken through a notification call when activated. |
| `pex_state_identifier_v1` | pending | Attach a hand-assembled script with two states whose OnActivate differs to an activator in a purpose-built cell; activate twice with a state change between, and record which notification appears. |
| `vm_object_identity_v1` | pending | Compare in a hand-assembled script a property holding a reference with the activating reference and with another reference, and show the results through a notification call. |
| `vm_host_player_handle_v1` | pending | In a purpose-built cell, a hand-assembled script compares Game.GetPlayer() with the activator's akActionRef when the player activates it and shows the result through a notification call. |
| `vm_timer_events_v1` | pending | A hand-assembled script starts two timers with different ids and intervals in OnActivate and shows each OnTimer id with its time through a notification call. |
| `vm_event_handlers_v1` | pending | Attach a hand-assembled script defining each handler to an activator and a trigger in a purpose-built cell; perform each action and record which notifications appear, in which order. |
| `vmad_reference_scripts_v1` | pending | Place two references of one scripted base, one with a reference VMAD that overrides a property; activate both and show the property value through a notification call. |
| `vmad_property_sets_variable_v1` | pending | A hand-assembled script shows in OnInit the values of properties set by VMAD on a placed reference through a notification call. |
| `vm_disable_hides_reference_v1` | pending | Activate in the running game a placed reference whose script calls Disable on itself, and photograph the place before and after. |
| `vm_host_data_state_v1` | pending | In a purpose-built cell, change a linked reference, a global and a form list from one script, then run a second script that reads them, and show what it reads through a notification call. |
| `vm_host_linked_ref_xlkr_v1` | pending | Place a reference with two XLKR pairs (no keyword, a keyword) and show GetLinkedRef() and GetLinkedRef(keyword) through a notification call. |
| `vm_host_linked_ref_chain_v1` | pending | Chain three references by one keyword, one more by another, and show the chain's length and order through a notification call. |
| `vm_host_is_disabled_flag_v1` | pending | Show IsDisabled of an initially disabled and an initially enabled reference through a notification call before anything enables them. |
| `vm_host_base_object_name_v1` | pending | Show GetBaseObject of a placed reference compared with a property holding its base through a notification call. |
| `vm_host_form_from_file_v1` | pending | Call GetFormFromFile for a record of a loaded master, of a plugin not loaded, and an id with a load-order byte set, and show the results through a notification call. |
| `vm_host_global_fltv_v1` | pending | Show GetValue of a global never set by a script through a notification call, then of one set in an earlier session. |
| `vm_host_formlist_lnam_v1` | pending | Show GetSize, GetAt(0) and HasForm of a stored list through a notification call. |
| `vm_record_signature_type_v1` | pending | Test in a script a placed activator against Furniture and Activator and a quest against Quest, and show the results through a notification call. |
| `vm_record_scripts_attach_v1` | pending | A hand-assembled activator script reads a property of a quest script and of an alias script and shows them through a notification call. |
| `vmad_alias_value_v1` | pending | Give a script a property naming an alias of a running quest and show the alias's reference through a notification call. |
| `save_container_layout_v1` | pending | Have the running game write new saves in several situations (new game, interior, exterior, after installing and after removing a light plugin) and read each: any file refused or framed differently rejects the rule. |
| `save_header_fields_v1` | pending | Write two saves of one character a level apart in the running game and compare each header field with the load menu and the character screen. |
| `save_screenshot_rgba_v1` | pending | Save in front of a scene with one large area of a known primary colour and compare the decoded pixels with the load-menu thumbnail. |
| `save_plugin_lists_v1` | pending | Install two light plugins, place one object from each in a test cell, move both in the running game and save: the change forms must name ids whose slots follow the second list's order. |
| `save_change_form_entry_v1` | pending | Read a save written after moving one placed object in the running game: exactly one more entry naming that object must appear, framed the same way. |
| `save_change_form_zlib_v1` | pending | Same saves as save_change_form_entry_v1: the payload of a heavily changed actor must inflate exactly before and after it changes size class. |
| `save_ref_id_kinds_v1` | pending | In a purpose-built cell, move a placed object of the first master, a placed object of a DLC and drop an item, then save: the three change forms must name those forms under the rule (kinds 1, 0 and 2). |
| `save_form_id_array_v1` | pending | Same saves as save_ref_id_kinds_v1: the DLC object's entry must carry that DLC's index in the first list. |
| `save_change_form_type_record_v1` | pending | Change one form of each record type in the running game (move a reference, change a cell, finish a dialogue line, advance a quest) and save: each new entry must carry its type. |
| `save_refr_move_layout_v1` | pending | Move a placed object by a known offset in a known cell with the console and save: the payload's first bytes must name that cell and hold the position and rotation the console reports. |
