//! Reviewer-authorized, falsifiable rules. Original diagnostic bytes are retained.
use crate::State;
use fo4_core::{Budget, Error, Result};
pub const VISIBILITY_MARKER: &str = "nif_visibility_editor_marker_hidden_v1";
pub const MARKER_SUFFIX: &str = "nif_editor_marker_export_suffix_v1";
pub const PATH_STRIP: &str = "asset_path_data_prefix_strip_v1";
pub const PATH_ROOT: &str = "asset_path_missing_root_v1";
pub const PATH_DATA_RELATIVE: &str = "asset_path_data_relative_strip_v1";
pub const EFFECT_COLOR: &str = "effect_color_only_v1";
pub const CULL: &str = "nif_backface_cull_v1";
pub const MODT: &str = "modt_not_in_frame_v1";
pub const BLEND: &str = "bgsm_blend_factors_alpha_function_v1";
pub const ROTATION: &str = "refr_rotation_zyx_v2";
pub const SCOL: &str = "scol_parts_static_frame_v1";
pub const SCALE: &str = "refr_absent_xscl_identity_v1";
pub const REGISTRY: &str = include_str!("../../../spec/hypotheses.tsv");
pub const MARKER: &str = "editor_marker_hidden_v1";
pub const NAMED_MARKER: &str = "nif_editor_marker_named_hidden_v1";
/// Collision placement (`crate::collision`, `spec/collision-placement.md`).
pub const COLLISION_UNITS: &str = "havok_collision_units_69_99125_v1";
pub const TARGET_FRAME: &str = "havok_shape_in_target_frame_v1";
pub const BODY_ID: &str = "nif_collision_body_id_selects_body_v1";
pub const BODY_PLACEMENT_IGNORED: &str = "havok_body_placement_ignored_v1";
pub const BODY_PLACEMENT_APPLIED: &str = "havok_body_placement_applied_v1";
pub const COMPOUND_INSTANCE: &str = "havok_compound_instance_v1";
pub const SCALED_CONVEX: &str = "havok_scaled_convex_v1";
pub const MESH_TRIANGLES: &str = "havok_mesh_triangles_surface_v1";
pub const CONVEX_RADIUS: &str = "havok_convex_radius_added_v1";
/// Collision readiness apart from render readiness (`crate::readiness`).
pub const SWAP_COLLISION: &str = "mswp_collision_unchanged_v1";
/// Coverage rules (`spec/coverage.md`).
pub const SWAP_FOLDER: &str = "mswp_fnam_folder_label_v1";
pub const OPACITY_ABOVE_ONE: &str = "bgsm_opacity_above_one_test_v1";
pub const BASE_PROPERTIES: &str = "base_prps_not_in_frame_v1";
pub const MISC_COMPONENTS: &str = "misc_cvpa_not_in_frame_v1";
pub const REFERENCE_GROUP: &str = "refr_xrfg_not_in_frame_v1";
/// The root node's own transform of a placed model (`spec/nif.md`, Root
/// node transform).
pub const ROOT_TRANSFORM: &str = "nif_root_transform_replaced_v1";
/// Enable parents (`crate::enable`, `spec/enable-parent.md`).
pub const ENABLE_FOLLOWS: &str = "xesp_child_follows_parent_v1";
pub const ENABLE_INITIAL: &str = "xesp_initial_state_chain_v1";
pub const ENABLE_OPPOSITE: &str = "xesp_flag_bit0_opposite_v1";
pub const ENABLE_BIT1: &str = "xesp_flag_bit1_not_presence_v1";
/// Animation-controller blocks at rest (`crate::rest`,
/// `spec/controller-rest.md`).
pub const REST_MANAGER_IDLE: &str = "nif_controller_rest_manager_idle_v1";
pub const REST_IDLE_TRANSFORM: &str = "nif_controller_rest_idle_transform_v1";
pub const REST_IDLE_SHADER: &str = "nif_controller_rest_idle_shader_v1";
pub const REST_TRANSFORM_CONSTANT: &str = "nif_controller_rest_transform_constant_v1";
pub const REST_UNRENDERED: &str = "nif_controller_rest_unrendered_v1";
pub const REST_EFFECT_FIRST_KEY: &str = "nif_controller_rest_effect_first_key_v1";
/// Effect surfaces of models settled by `REST_EFFECT_FIRST_KEY`
/// (`fo4_render::draw`, `spec/effect-surfaces.md`).
pub const EFFECT_SHADING: &str = "effect_surface_texture_base_colour_v1";
pub const EFFECT_BLEND: &str = "effect_blend_factors_add_v1";
pub const EFFECT_ORDER: &str = "effect_blend_after_opaque_back_to_front_v1";
pub const EFFECT_LEFT_OUT: &str = "effect_soft_lighting_left_out_v1";
/// The soft depth fade of a blended effect surface
/// (`fo4_render::draw::BuildOptions::soft_effects`, `--draw-soft-effects`).
pub const EFFECT_SOFT_FADE: &str = "effect_soft_depth_fade_linear_v1";
/// Falloff of effect surfaces (`fo4_render::draw::effect`, the reference
/// rasterizer, `spec/effect-surfaces.md`, Falloff): the angle it reads,
/// the cosine's sign, the interpolation and what the factor multiplies.
pub const EFFECT_FALLOFF: &str = "effect_falloff_view_angle_v1";
pub const EFFECT_FALLOFF_ABSOLUTE: &str = "effect_falloff_cosine_absolute_v1";
pub const EFFECT_FALLOFF_LINEAR: &str = "effect_falloff_linear_in_cosine_v1";
pub const EFFECT_FALLOFF_ALPHA: &str = "effect_falloff_multiplies_alpha_v1";
/// Vertex alpha of an effect surface with Vertex_Colors set and
/// Vertex_Alpha clear.
pub const EFFECT_VERTEX_ALPHA_COLORS: &str = "effect_vertex_alpha_under_vertex_colors_v1";
/// Vertex alpha of a blended lighting surface with Vertex_Colors set
/// (`fo4_render::draw`); vertex RGB stays out of the unlit frame.
pub const LIGHTING_VERTEX_ALPHA: &str = "lighting_vertex_alpha_blend_v1";
/// Falloff angle controllers whose keys are degrees (`crate::rest`).
pub const REST_FALLOFF_DEGREES: &str = "nif_controller_falloff_angle_degrees_v1";
/// Greyscale-to-palette lookups of effect surfaces (`fo4_render::Palette`,
/// `spec/effect-surfaces.md`, Palette): how the palette is sampled, its
/// vertical coordinate, the colour and the alpha lookup's horizontal
/// coordinate and what each replaces.
pub const EFFECT_PALETTE_SAMPLED: &str = "effect_palette_sampled_clamped_v1";
pub const EFFECT_PALETTE_V: &str = "effect_palette_v_from_alpha_v1";
pub const EFFECT_PALETTE_COLOUR_U: &str = "effect_palette_colour_u_green_v1";
pub const EFFECT_PALETTE_COLOUR_RGB: &str = "effect_palette_colour_replaces_rgb_v1";
pub const EFFECT_PALETTE_ALPHA_U: &str = "effect_palette_alpha_u_alpha_v1";
pub const EFFECT_PALETTE_ALPHA_REPLACES: &str = "effect_palette_alpha_replaces_alpha_v1";
/// A surface colour above 1 (an effect's base colour times a scale above
/// 1), clamped before the alpha test and the blend.
pub const EFFECT_COLOUR_CLAMPED: &str = "effect_colour_clamped_before_blend_v1";
/// The environment map of an effect surface: what its term changes, and
/// that this unlit frame leaves it out (`spec/effect-surfaces.md`,
/// Environment map).
pub const EFFECT_ENVIRONMENT_ADDS: &str = "effect_environment_adds_to_colour_v1";
pub const EFFECT_ENVIRONMENT_LEFT_OUT: &str = "effect_environment_left_out_v1";
/// A field an effect-shader controller drives on which the shape's effect
/// material (as named, or as a swap replaces it) and the NIF disagree: the
/// material's value is drawn.
pub const EFFECT_MATERIAL_OVER_CONTROLLER: &str = "effect_material_over_controller_v1";
/// An opacity above 1 on a blended surface: the alpha is clamped to 1
/// after the alpha test and before the blend (`spec/render.md`).
pub const BLEND_OPACITY_CLAMPED: &str = "blend_alpha_clamped_before_blend_v1";
/// Materials, swap scopes and texture transforms (`crate::missing`,
/// `crate::rest`, `fo4_render::draw`, `spec/materials.md`).
pub const MISSING_MATERIAL_EMBEDDED: &str = "nif_missing_material_embedded_shader_v1";
pub const SWAP_SCOPES_AGREE: &str = "mswp_scopes_readings_agree_v1";
/// Swap scopes whose readings differ (`fo4_render::draw`,
/// `spec/materials.md`, Swap scopes): the outer scope's mapping of a
/// material the model names wins, and the outer swap replaces the inner
/// one as a whole.
pub const SWAP_OUTER_WINS: &str = "mswp_outer_scope_wins_v1";
pub const SWAP_OUTER_REPLACES: &str = "mswp_outer_scope_replaces_v1";
/// Swap scopes where one maps the material another put in place: the
/// chain is followed, each scope once (`fo4_render::draw`).
pub const SWAP_CHAIN: &str = "mswp_chain_followed_v1";
/// A swap over a shape whose shader names no material leaves the shape as
/// it is (`fo4_render::draw`).
pub const SWAP_WITHOUT_MATERIAL: &str = "mswp_shape_without_material_untouched_v1";
/// A swap entry with an empty BNAM or SNAM changes nothing (scene
/// `MaterialSwapEmptyEntry`, `fo4_render::draw`).
pub const SWAP_EMPTY_ENTRY: &str = "mswp_empty_entry_inert_v1";
/// Swap sources holding `*` (`crate::swap_pattern`, `fo4_render::draw`,
/// `spec/materials.md`, Swap patterns): read as patterns over the path the
/// shader names; a literal source of the same swap wins over a pattern;
/// a pattern whose target no archive holds changes nothing (scene
/// `MissingSwapPatternTarget`).
pub const SWAP_PATTERN: &str = "mswp_wildcard_pattern_v1";
pub const SWAP_PATTERN_LITERAL: &str = "mswp_wildcard_literal_wins_v1";
pub const SWAP_PATTERN_MISSING: &str = "mswp_wildcard_missing_target_inert_v1";
/// A triangle using a vertex whose texture coordinate is not a finite
/// number is left out of the draw (`fo4_render::draw`).
pub const UV_NONFINITE: &str = "nif_uv_nonfinite_triangles_left_out_v1";
/// A placed reference's primitive (`REFR.XPRM`) draws nothing and adds
/// nothing to collision; the base draws as it would without it.
pub const REFERENCE_PRIMITIVE: &str = "refr_xprm_primitive_not_drawn_v1";
pub const REST_TEXTURE_FIRST_KEY: &str = "nif_controller_rest_texture_first_key_v1";
/// The game clock of free-running controllers (`crate::clock`,
/// `spec/controller-clock.md`): a controller not run by a manager starts
/// when its model loads and runs on the game clock; its time folded by its
/// cycle type; its keys read by their key type.
pub const CLOCK_FREE_RUNNING: &str = "nif_controller_clock_free_running_v1";
pub const CLOCK_CYCLE_LOOP: &str = "nif_controller_cycle_loop_v1";
pub const CLOCK_CYCLE_REVERSE: &str = "nif_controller_cycle_reverse_v1";
pub const CLOCK_CYCLE_CLAMP: &str = "nif_controller_cycle_clamp_v1";
pub const CLOCK_KEY_LINEAR: &str = "nif_key_linear_v1";
pub const CLOCK_KEY_QUADRATIC: &str = "nif_key_quadratic_hermite_v1";
pub const CLOCK_KEY_STEP: &str = "nif_key_constant_step_v1";
/// A looping sequence of a controller manager that nothing starts leaves
/// its controlled blocks as stored (`crate::rest`).
pub const REST_LOOP_UNSTARTED: &str = "nif_controller_rest_loop_unstarted_v1";
/// Effect-shader fields other than alpha and UV that a free-running
/// controller drives, run on the game clock behind their switches
/// (`crate::clock::Switches`, off by default): the emissive multiple (the
/// base colour scale), the emissive colour (the base colour's RGB) and the
/// four falloff fields.
pub const CLOCK_EMISSIVE_MULTIPLE: &str = "effect_clock_emissive_multiple_v1";
pub const CLOCK_EMISSIVE_COLOUR: &str = "effect_clock_emissive_colour_v1";
pub const CLOCK_FALLOFF: &str = "effect_clock_falloff_v1";
/// Free-running transform controllers moving their node on the game clock,
/// one rule per channel and rotation key form, behind their switches
/// (`crate::clock::Switches`, off by default; `crate::clock::TransformTrack`).
pub const CLOCK_TRANSLATION: &str = "nif_clock_transform_translation_v1";
pub const CLOCK_ROTATION_QUATERNION: &str = "nif_clock_transform_rotation_quaternion_v1";
pub const CLOCK_ROTATION_XYZ: &str = "nif_clock_transform_rotation_xyz_v1";
pub const CLOCK_SCALE: &str = "nif_clock_transform_scale_v1";
pub const UV_SCALE_THEN_OFFSET: &str = "uv_scale_then_offset_v1";
pub const MISSING_PARENT: &str = "bgsm_missing_parent_own_fields_v1";
/// A missing texture cited only in slots the unlit frame never samples
/// (`crate::missing::unread_textures`).
pub const MISSING_TEXTURE_UNREAD: &str = "texture_missing_unread_slot_v1";
/// A script's Enable or Disable reaching enable children (the viewer's
/// `--script-event`).
pub const ENABLE_SCRIPT: &str = "xesp_script_change_propagates_v1";
/// A run's Disable or Enable of a placed reference (`fo4_vm`; a timeline
/// frame, `crate::timeline`).
pub const DISABLE_HIDES: &str = "vm_disable_hides_reference_v1";
/// The exterior cell whose grid square holds a position (`crate::timeline`).
pub const WORLD_CELL: &str = "world_cell_natives_v1";
/// A reference a run moved, drawn at its new position with its stored
/// rotation and scale (`crate::timeline`).
pub const TIMELINE_MOVE: &str = "timeline_move_stored_rotation_v1";
/// The NoWait forms of Enable and Disable read as the plain natives
/// (`crate::timeline::Readings::nowait`).
pub const TIMELINE_NOWAIT: &str = "timeline_nowait_visibility_v1";
/// A flag 0x800 below the root of an enable chain that agrees with the
/// chain's start state, the chain then read as any other
/// (`crate::enable::Presence::with_agreeing_child_flags`; a timeline
/// frame's `Readings::child_flag_agrees`).
pub const ENABLE_CHILD_FLAG_AGREES: &str = "xesp_child_flag_agrees_chain_v1";
/// An actor a run moved drawn as its character at the run's place with its
/// placement's stored rotation (`crate::timeline::cast`).
pub const TIMELINE_ACTOR_ROTATION: &str = "timeline_actor_stored_rotation_v1";
/// An actor a run moved facing as the one placed reference at its new
/// place (`crate::timeline::cast::Options::target_rotation`).
pub const TIMELINE_ACTOR_TARGET_ROTATION: &str = "timeline_actor_target_rotation_v1";
/// A bone a skinned shape names that the actor's skeleton lacks, skinned at
/// its bind transform (`crate::actor::Actor::missing_bones_at_bind`).
pub const ACTOR_MISSING_BONE_BIND: &str = "actor_missing_bone_bind_pose_v1";
/// With it, such a bone skinned as the nearest bone of its skin the
/// skeleton holds (`crate::actor::Actor::missing_bones_nearest`).
pub const ACTOR_MISSING_BONE_NEAREST: &str = "actor_missing_bone_follows_nearest_v1";
/// An interior cell's ambient and directional colour lighting its surfaces
/// (`crate::light`, `--light-cell`).
pub const LIGHT_CELL: &str = "cell_light_ambient_directional_v1";
/// The fields a cell's `Inherits` word names taken from its lighting
/// template (`crate::light`, `--light-inherits`).
pub const LIGHT_INHERITS: &str = "cell_light_template_inherits_v1";
/// Placed lights as points with a linear falloff to their radius
/// (`crate::light`, `--light-points`).
pub const LIGHT_POINTS: &str = "light_point_linear_cutoff_v1";
/// A placed light's radius: its base's plus its reference's XRDS
/// (`crate::light`, `--light-reference-radius`).
pub const LIGHT_REFERENCE_RADIUS: &str = "light_reference_radius_added_v1";
/// A placed light's colour times its base's fade value (`crate::light`,
/// `--light-fade`).
pub const LIGHT_FADE: &str = "light_fade_scales_colour_v1";
/// An interior cell's fog by distance (`crate::light`, `--light-fog`).
pub const LIGHT_CELL_FOG: &str = "cell_fog_linear_v1";
/// The lit colour: the surface colour times the summed light, clamped,
/// no tone mapping (`crate::light`, whenever a light switch lights).
pub const LIGHT_SUM: &str = "light_sum_clamped_v1";
/// A climate listing one weather: that weather applies (`crate::light`).
pub const CLIMATE_SINGLE_WEATHER: &str = "climate_single_weather_v1";
/// The weather slot a caller's hour reads from the climate's times
/// (`crate::light::hour_slot`).
pub const CLIMATE_HOUR_SLOT: &str = "climate_hour_slot_v1";
/// The weather's colour rows 0, 7, 8 as the sky's zenith, lower sky and
/// horizon (`crate::light`, `--sky-weather`).
pub const WEATHER_SKY: &str = "weather_sky_rows_v1";
/// The weather's directional ambient of the slot as an ambient cube
/// (`crate::light`, `--light-weather`).
pub const WEATHER_AMBIENT: &str = "weather_ambient_cube_v1";
/// The weather's fog row and fog distances (`crate::light`, `--fog-weather`).
pub const WEATHER_FOG: &str = "weather_fog_rows_v1";
/// Base types whose PRPS `BASE_PROPERTIES` covers: those measured.
const PROPERTY_BASES: [&str; 8] = [
    "STAT", "ACTI", "FURN", "CONT", "FLOR", "LIGH", "MSTT", "TERM",
];
pub const RENDER_INPUTS_COLLISION: &str = "render_inputs_collision_unchanged_v1";
/// A model blocker raised off the blocks the model's placed bodies read
/// leaves its collision as stored (`crate::readiness`, behind
/// `readiness::Switches::off_chain`).
pub const COLLISION_OFF_CHAIN: &str = "collision_model_blockers_off_chain_v1";
/// A sequence's transform interpolator that links no keys holds its node at
/// the interpolator's own transform (`crate::sequence::held_pose`).
pub const SEQUENCE_HELD_POSE: &str = "nif_sequence_interpolator_held_pose_v1";
/// A sequence poses a collision body's chain with the full local transform
/// it gives each node it sets (`crate::sequence::chain_pose`).
pub const SEQUENCE_CHAIN_POSE: &str = "nif_sequence_chain_pose_v1";
/// A reference's unread `XALP` leaves its collision as stored
/// (`crate::readiness`, behind `readiness::Switches::reference_alpha`).
pub const REFERENCE_ALPHA_COLLISION: &str = "refr_xalp_collision_unchanged_v1";
/// A reference's unread `XALP` leaves its drawing as stored
/// (`Loader::set_render_reference_alpha`, off by default).
pub const REFERENCE_ALPHA_RENDER: &str = "refr_xalp_render_unchanged_v1";
/// A reference whose record carries scripts (`VMAD`) is drawn as its model
/// stores it; what its scripts change is left to the caller's run
/// (`Loader::set_render_scripted`, off by default).
pub const REFERENCE_SCRIPT_RENDER: &str = "refr_script_render_as_stored_v1";
/// An activator's (`ACTI`) unread `PNAM`, `FNAM` and `ATTX` leave its
/// drawing as its model stores it (`Loader::set_render_activator_fields`,
/// off by default).
pub const ACTIVATOR_FIELDS_RENDER: &str = "acti_fields_render_unchanged_v1";
/// An activator's (`ACTI`) unread `RADR` and its reference's `XLRT` leave
/// its drawing as its model stores it
/// (`Loader::set_render_activator_receiver`, off by default).
pub const ACTIVATOR_RECEIVER_RENDER: &str = "acti_receiver_location_render_unchanged_v1";
/// A shape whose lighting shader sets the Refraction flag is left out of
/// this frame (`fo4_render::draw::BuildOptions::refraction_left_out`, off
/// by default).
pub const LIGHTING_REFRACTION_LEFT_OUT: &str = "lighting_refraction_surface_left_out_v1";
/// A furniture's (`FURN`) unread `PNAM`, `FNAM`, `MNAM`, `SNAM` and `WBDT`
/// leave its drawing as its model stores it
/// (`Loader::set_render_furniture_fields`, off by default).
pub const FURNITURE_FIELDS_RENDER: &str = "furn_fields_render_unchanged_v1";
/// A furniture's (`FURN`) object template (`OBTE`, `OBTS`) and activate
/// text (`ATTX`) leave its drawing as its model stores it
/// (`Loader::set_render_furniture_template`, off by default).
pub const FURNITURE_TEMPLATE_RENDER: &str = "furn_template_text_render_unchanged_v1";
/// A worldspace's persistent references whose stored position lies in a
/// loaded grid square join that square's scene (`grid::persistent_in`,
/// the viewer's `--draw-world-persistent`, off by default).
pub const WORLD_PERSISTENT_IN_SQUARE: &str = "world_persistent_refs_drawn_in_square_v1";
/// A static's reference fields `XACT`, `XLRT`, `XOWN` and `ONAM` leave its
/// collision as stored (`crate::readiness`, behind
/// `readiness::Switches::static_fields`).
pub const STATIC_REFERENCE_FIELDS_COLLISION: &str = "refr_static_fields_collision_unchanged_v1";
/// An activator base's unread `PNAM`, `FNAM`, `ATTX` and `RADR` and its
/// reference's `XLRT` leave its collision as stored (`crate::readiness`,
/// behind `readiness::Switches::activator_fields`).
pub const ACTIVATOR_FIELDS_COLLISION: &str = "acti_fields_receiver_collision_unchanged_v1";
/// A furniture base's unread fields, its object template and activate
/// text, and its reference's `XLRT` and `XIS2` leave its collision as
/// stored (`crate::readiness`, behind
/// `readiness::Switches::furniture_fields`).
pub const FURNITURE_FIELDS_COLLISION: &str = "furn_fields_collision_unchanged_v1";
/// A furniture's placed bodies stand at their chains' stored transforms
/// although a controller or a controller sequence of its model can move
/// them (`crate::readiness`, behind
/// `readiness::Switches::furniture_at_rest`).
pub const FURNITURE_COLLISION_AT_REST: &str = "furn_collision_at_rest_v1";
/// A static's reference fields `XACT`, `XLRT`, `XOWN` and `ONAM` leave its
/// drawing as stored (`Loader::set_render_static_fields`, off by default).
pub const STATIC_REFERENCE_FIELDS_RENDER: &str = "refr_static_fields_render_unchanged_v1";
/// A static whose model's one controller turns a node off the collision
/// chain from its stored pose and back is drawn with that node in one of
/// two poses, stored or at the key farthest from it (`crate::leaf`,
/// `Loader::set_render_static_leaves`, off by default; the play window
/// picks the pose by the collision marker boxes that meet the static).
pub const STATIC_LEAF_TWO_POSE: &str = "play_static_leaf_two_pose_v1";
/// A static refused for model blocks no rule settles is drawn by the
/// shapes those blocks and the model's controllers do not reach, the rest
/// left out and counted (`crate::parts`, `Loader::set_render_static_parts`,
/// off by default).
pub const STATIC_PARTS: &str = "static_unmoved_parts_drawn_v1";
/// An activator refused for model blocks no rule settles (its animation:
/// controllers, a behaviour graph) is drawn as `STATIC_PARTS` draws a
/// static, its reference's `XLRT` set aside (`Loader::set_render_activator_parts`,
/// with `Loader::set_render_static_parts`; off by default).
pub const ACTIVATOR_PARTS: &str = "acti_unmoved_parts_drawn_v1";
/// A furniture (`FURN`) placement refused for model blocks no rule
/// settles is drawn as `STATIC_PARTS` draws a static
/// (`Loader::set_render_furniture_parts`, with
/// `Loader::set_render_static_parts`; off by default).
pub const FURNITURE_PARTS: &str = "furn_unmoved_parts_drawn_v1";
/// A furniture (`FURN`) placement refused for model blocks no rule
/// settles is drawn at rest: as `FURNITURE_PARTS` draws it, but the shapes
/// a controller can drive kept at their stored transforms
/// (`crate::parts::read_with`; `Loader::set_render_furniture_at_rest`, with
/// `Loader::set_render_furniture_parts`; off by default).
pub const FURNITURE_AT_REST: &str = "furn_parts_drawn_at_rest_v1";
/// A skinned placement whose reference stores a ragdoll pose (`XRGD`) is
/// left out of the skinned own-nodes reading
/// (`timeline::ride::add_skinned_with`, `--skinned-models-ragdoll-left-out`).
pub const RAGDOLL_LEFT_OUT: &str = "refr_ragdoll_pose_unread_left_out_v1";
/// A named platform's skinned shape whose bones go with more than one of
/// its parts drawn skinned again with each bone moved by its part's motion
/// (`crate::platform::skin_at_rest`, the play window's
/// `--draw-platform-skins`, off by default).
pub const PLATFORM_SKIN_FOLLOWS_PARTS: &str = "play_platform_skin_follows_parts_v1";
/// A named platform's shape whose node chain's deepest set node holds no
/// moving part drawn with the part of the nearest set node above it (the
/// play window's `--draw-platform-nearest-part`, off by default).
pub const PLATFORM_SHAPE_NEAREST_PART: &str = "play_platform_shape_nearest_part_v1";
/// The run's actors drawn in the play window standing on the named
/// platforms' parts under them (the window's `--draw-cast-on-platforms`,
/// off by default).
pub const CAST_ON_PLATFORMS: &str = "play_cast_stands_on_platform_v1";
/// With it, each actor riding the part it stands on while that part is
/// under it (the window's `--draw-cast-keeps-its-part`, off by default).
pub const CAST_KEEPS_PART: &str = "play_cast_keeps_its_part_v1";
/// Terrain of an exterior cell (`crate::terrain`, `spec/land.md`).
pub const LAND_OF_CELL: &str = "land_single_cell_child_v1";
pub const LAND_ACCUMULATION: &str = "land_vhgt_row_accumulation_v1";
pub const LAND_UNITS: &str = "land_vhgt_units_8_v1";
pub const LAND_ORIGIN: &str = "land_grid_origin_southwest_v1";
pub const LAND_SPACING: &str = "land_grid_spacing_128_v1";
pub const LAND_DIAGONAL_SW_NE: &str = "land_quad_diagonal_sw_ne_v1";
pub const LAND_DIAGONAL_NW_SE: &str = "land_quad_diagonal_nw_se_v1";
/// Ground textures of a terrain (`crate::terrain::layers`, `spec/land.md`).
pub const LAND_QUADRANTS: &str = "land_quadrant_layout_v1";
pub const LAND_VTXT_POSITION: &str = "land_vtxt_position_grid_v1";
pub const LAND_LAYER_WEIGHTS: &str = "land_layer_weights_residual_v1";
pub const LAND_MISSING_BASE: &str = "land_missing_base_default_v1";
pub const LTEX_TEXTURE_SET: &str = "ltex_tnam_texture_set_tx00_v1";
pub const LAND_TEXTURE_REPEAT: &str = "land_texture_repeat_256_v1";
pub const LAND_VERTEX_COLORS: &str = "land_vclr_multiply_v1";
/// Optional ground texture rules, applied only when accepted at the level.
pub const LTEX_MATERIAL: &str = "ltex_texture_set_material_diffuse_v1";
pub const LAND_DEFAULT_NAMED: &str = "land_default_texture_world_named_v1";
/// Placed actors (`crate::actor`, `spec/actor.md`).
pub const ACTOR_PLACEMENT: &str = "achr_placement_as_refr_v1";
pub const ACTOR_CANDIDATES: &str = "npc_appearance_candidates_agree_v1";
pub const ACTOR_OUTFIT: &str = "npc_default_outfit_worn_v1";
pub const ACTOR_SECTION: &str = "actor_acbs_bit0_second_section_v1";
pub const ACTOR_ADDONS: &str = "armo_race_addons_drawn_v1";
pub const ACTOR_SLOTS: &str = "actor_worn_slots_hide_skin_v1";
pub const ACTOR_SKINNING: &str = "actor_skin_linear_blend_v1";
pub const ACTOR_HEAD: &str = "npc_head_parts_unmorphed_v1";
pub const ACTOR_LEVELED_FLAGS: &str = "npc_leveled_list_flags_v1";
pub const ACTOR_TEMPLATE_ALL_BITS: &str = "npc_template_all_bits_v1";
/// A templated NPC_'s look by template bit: bit 0 its traits (race, ACBS
/// bit 0, skin, head parts), bit 8 its outfit, bit 12 its keywords
/// (`crate::actor::Readings::template_bits`, `--actor-template-bits`).
pub const ACTOR_TEMPLATE_BITS: &str = "npc_template_bits_traits_inventory_v1";
/// What of an actor's head a worn armour hides, from its BOD2 slots
/// (`crate::actor::Readings::head_slots`, `--actor-head-slots`).
pub const ACTOR_HEAD_SLOTS: &str = "npc_head_hidden_by_head_slot_v1";
/// A head part with conditions drawn or left out by its condition list as
/// the caller evaluates it against the run's stages done
/// (`crate::actor::HeadConditions`, `--actor-head-conditions`).
pub const ACTOR_HEAD_CONDITIONS: &str = "npc_head_part_conditions_run_stages_v1";
/// An actor whose own head part list lacks a part type takes its race's
/// part of that type (`crate::actor::Readings::head_race_fill`,
/// `--actor-head-race-fill`).
pub const ACTOR_HEAD_RACE_FILL: &str = "npc_head_parts_race_fill_kinds_v1";
/// A run or placed actor's models read under texture_missing_unread_slot_v1
/// as a placed reference's are, rootless citations whose rooted file is
/// missing too (`crate::actor::Readings::unread_slots`,
/// `--actor-unread-texture-slots`).
pub const ACTOR_UNREAD_SLOTS: &str = "actor_texture_missing_unread_slot_v1";
/// One entry of each leveled list an actor reaches, picked by the caller's
/// seed (`crate::actor::Readings::leveled_seed`, `--actor-leveled-seed`).
pub const ACTOR_LEVELED_PICK: &str = "npc_leveled_entry_seeded_pick_v1";
/// A leveled item list's LVLD read as the percent chance it gives nothing,
/// drawn from the caller's seed (`crate::actor::Readings::chance_none`,
/// `--actor-leveled-chance-none`).
pub const ACTOR_LEVELED_CHANCE_NONE: &str = "npc_leveled_item_chance_none_seeded_v1";
/// A leveled item list whose LVLF has bit 2 set gives every entry
/// (`crate::actor::Readings::use_all`, `--actor-leveled-use-all`).
pub const ACTOR_LEVELED_USE_ALL: &str = "npc_leveled_item_flag_use_all_v1";
/// A model's skinned shapes skinned to its own nodes (`crate::timeline::ride`,
/// `--timeline-ride-model`).
pub const MODEL_OWN_NODES: &str = "model_skinned_own_nodes_v1";
/// The play run's named platforms drawn by their models as stored in the
/// play window, their unread record fields, behaviour graph and
/// controllers left unread (`crate::platform`).
pub const PLAY_PLATFORM_DRAWN: &str = "play_platform_drawn_v1";
/// The shapes of a named platform's model the draw refuses left out, the
/// rest drawn (`crate::platform`).
pub const PLAY_PLATFORM_LEFT_OUT: &str = "play_platform_shapes_left_out_v1";
/// The nodes a caller's sequence translates, at the frame's second
/// (`crate::timeline::ride`, `--timeline-ride`).
pub const TIMELINE_RIDE: &str = "timeline_ride_sequence_nodes_v1";
/// An actor's part model whose own behaviour graph's root start state plays
/// no sequence drawn as stored, its graph extra data and manager-run
/// effect-shader controllers settled (`crate::timeline::cast`,
/// `--actor-model-graph-idle`).
pub const ACTOR_MODEL_GRAPH_START_IDLE: &str = "actor_model_graph_start_no_sequence_v1";
/// An actor's part model's manager-run effect-shader float controller not
/// run, its target field drawn as stored (`crate::timeline::cast`,
/// `--actor-effect-stored`).
pub const ACTOR_EFFECT_STORED: &str = "actor_effect_controller_stored_v1";
/// Every placed reference of a frame refused for its skinned shapes drawn
/// by those shapes on its model's own nodes (`crate::timeline::ride::add_skinned`,
/// `--skinned-models-own-nodes`).
pub const MODEL_OWN_NODES_EVERY: &str = "model_skinned_own_nodes_every_placement_v1";
/// A clip posing an actor (`crate::actor::clip`, `spec/actor.md`, Clip pose).
pub const ACTOR_CLIP_SKELETON: &str = "actor_clip_skeleton_beside_nif_v1";
pub const ACTOR_CLIP_POSE: &str = "actor_clip_pose_by_bone_name_v1";
/// A character graph followed from its root by start states, stored
/// initial values, highest weights and stored indices to a clip, drawn at
/// the clip generator's start time (`crate::char_graph`, `--actor-start-clip`).
pub const CHAR_GRAPH_START: &str = "char_graph_start_state_clip_v1";
/// A swap generator playing the race's keyword-matched subgraph entry of
/// flag word 0 (`crate::char_graph`, `--actor-subgraph-keywords`).
pub const CHAR_GRAPH_SUBGRAPH: &str = "char_graph_swap_subgraph_keywords_v1";
/// A race without entries of flag word 0 (no target keywords) taking its
/// entries of flag word 1 without target keywords in their place
/// (`crate::char_graph::pick_entry_with`, `--actor-subgraph-flag-one`).
pub const CHAR_GRAPH_SUBGRAPH_FLAG_ONE: &str = "char_graph_swap_subgraph_flag_one_v1";
/// A clip text's file name found in the subgraph entry's animation folders
/// in order (`crate::char_graph`, `--actor-clip-folder`).
pub const CHAR_GRAPH_CLIP_FOLDER: &str = "char_graph_clip_folder_order_v1";
/// A start clip drawn at the frame's second, folded by the clip
/// generator's mode (`crate::char_graph::clip_time`, `--actor-start-clip-loop`).
pub const CHAR_GRAPH_CLIP_LOOP: &str = "char_graph_clip_mode_loop_v1";
/// A dead actor (the run's `death` event lines) not drawn from its death
/// on (`crate::timeline::Dead::LeftOut`, `--timeline-dead-left-out`).
pub const TIMELINE_DEAD_LEFT_OUT: &str = "timeline_dead_actor_left_out_v1";
/// A dead actor drawn lying at its last place, a placeholder
/// (`crate::timeline::Dead::Lying`, `--timeline-dead-lying`).
pub const TIMELINE_DEAD_LYING: &str = "timeline_dead_actor_lying_placeholder_v1";
/// A sitting actor (the run's `sit` event lines) at its furniture's marker
/// nearest its place (`crate::timeline::Readings::sit_marker`,
/// `--timeline-sit-marker`).
pub const TIMELINE_SIT_MARKER: &str = "timeline_sit_marker_nearest_v1";
/// A furniture's markers read from its FURN SNAM entries
/// (`crate::furniture`).
pub const FURNITURE_MARKER_SNAM: &str = "furniture_marker_snam_v1";
/// A sitter posed by the start clip of its race's subgraph entry of flag
/// word 2 matching the furniture's keywords (`crate::char_graph::furniture_clip`,
/// `--actor-sit-clip`).
pub const CHAR_GRAPH_FURNITURE: &str = "char_graph_furniture_entry_keywords_v1";
/// The collision of one animated model's placed reference placed as
/// stored, its nodes at their stored transforms
/// (`crate::collision::instance_as_stored`).
pub const ANIMATED_COLLISION_AS_STORED: &str = "walk_animated_model_collision_as_stored_v1";
/// A lit colour divided by the mean light falling on what the eye sees
/// (`crate::light::Light::expose_seen`, `--exposure-eye`).
pub const EXPOSURE_EYE: &str = "frame_exposure_seen_light_v1";
/// Under [`EXPOSURE_EYE`], a view whose seen light is below a floor takes
/// the floor's scale (`crate::light::EXPOSURE_FLOOR`).
pub const EXPOSURE_FLOOR: &str = "frame_exposure_seen_light_floor_v1";
/// Under [`EXPOSURE_EYE`], the light measured on what the eye sees: the
/// nearest lit face at each cell of a grid over the frame
/// (`crate::light::Exposure::visible`, the viewer's `--exposure-eye-visible`).
pub const EXPOSURE_VISIBLE: &str = "frame_exposure_seen_visible_v1";
/// A lit colour tone mapped with the white of the frame's image space
/// (`crate::light::Exposure::white`, `--exposure-white`).
pub const IMAGE_SPACE_WHITE: &str = "image_space_hdr_white_v1";
/// A lit colour's saturation, brightness, contrast and tint from the
/// frame's image space (`crate::light::Exposure`, `--exposure-cinematic`).
pub const IMAGE_SPACE_CINEMATIC: &str = "image_space_cinematic_tint_v1";
/// A lit surface lit with its mesh's vertex normals
/// (`--light-vertex-normals`).
pub const LIGHT_VERTEX_NORMALS: &str = "light_vertex_normals_v1";
/// A rule's standing in `spec/hypotheses.tsv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    /// Confirmed by measurement of the running game, within the recorded scope.
    Supported,
    Refuted,
}
pub fn status(name: &str) -> Option<Status> {
    statuses().get(name).copied()
}
/// The status of each rule of [`REGISTRY`], read once: the first row of a
/// name whose status column reads, as a scan of the rows in order finds it
/// (a load asks thousands of times; the registry is half a megabyte).
fn statuses() -> &'static std::collections::HashMap<&'static str, Status> {
    static READ: std::sync::OnceLock<std::collections::HashMap<&'static str, Status>> =
        std::sync::OnceLock::new();
    READ.get_or_init(|| {
        let mut out = std::collections::HashMap::new();
        for row in REGISTRY.lines() {
            let mut columns = row.split('\t');
            let Some(name) = columns.next() else {
                continue;
            };
            let status = match columns.nth(4) {
                Some("pending") => Status::Pending,
                Some("supported") => Status::Supported,
                Some("refuted") => Status::Refuted,
                _ => continue,
            };
            out.entry(name).or_insert(status);
        }
        out
    })
}
/// Whether a rule may be relied on at `level`: a supported rule above
/// the facts level, a pending one only under hypotheses (the draw build's
/// reading, `fo4_render::draw`).
pub fn usable_at(rule: &str, level: crate::RuleLevel) -> bool {
    match status(rule) {
        Some(Status::Supported) => level != crate::RuleLevel::Facts,
        Some(Status::Pending) => level == crate::RuleLevel::Hypotheses,
        Some(Status::Refuted) | None => false,
    }
}
pub fn covering(error: &Error) -> Option<&'static str> {
    if error.code == "UnverifiedVisibilityEditorMarker" {
        Some(VISIBILITY_MARKER)
    } else if error.code == "UnverifiedEffectColorOnly" {
        Some(EFFECT_COLOR)
    } else if error.code == "HypothesisAssetDataPrefixStrip" {
        Some(PATH_STRIP)
    } else if error.code == "HypothesisAssetDataRelativeStrip" {
        Some(PATH_DATA_RELATIVE)
    } else if error.code == "HypothesisAssetMissingRoot" {
        Some(PATH_ROOT)
    } else if matches!(
        error.code.as_str(),
        "UnsupportedUsedFieldLayout" | "UninterpretedVisualSubrecord"
    ) && error.found.split('.').nth(1) == Some("MODT")
    {
        Some(MODT)
    } else if error.code == "UnsupportedMaterialFields"
        && [
            "alpha_blend.enabled_factors_unsourced(1,6,7)",
            "alpha_blend.enabled_factors_unsourced(1,6,0)",
            "alpha_blend.enabled_factors_unsourced(1,4,1)",
        ]
        .contains(&error.found.as_str())
    {
        Some(BLEND)
    } else if error.code == "UnverifiedNifEditorMarkerSuffixVisibility" {
        Some(MARKER_SUFFIX)
    } else if error.code == "UnverifiedNifEditorMarkerVisibility" {
        Some(NAMED_MARKER)
    } else if error.code == "UnverifiedScolGeometrySource" {
        Some(SCOL)
    } else if error.code == "MissingDocumentedReferenceScale" {
        Some(SCALE)
    } else if error.code == "UnverifiedEulerRotation" {
        Some(ROTATION)
    } else if error.code == "MissingMaterialEmbeddedShader" {
        Some(MISSING_MATERIAL_EMBEDDED)
    } else if error.code == "MissingMaterialParent" {
        Some(MISSING_PARENT)
    } else if error.code == "MissingTextureUnreadSlot" {
        Some(MISSING_TEXTURE_UNREAD)
    } else if error.code == "RuntimeMaterialSwapFilter" {
        Some(SWAP_FOLDER)
    } else if error.code == "MaterialSwapEmptyEntry" {
        Some(SWAP_EMPTY_ENTRY)
    } else if error.code == "MissingSwapPatternTarget" {
        Some(SWAP_PATTERN_MISSING)
    } else if error.code == "RuntimeEnableParent" {
        crate::enable::rule(&error.found)
    } else if error.code == "UnsupportedAnimationRuntime" {
        error
            .found
            .rsplit_once(";rest=")
            .and_then(|(_, t)| crate::rest::rule(t))
    } else {
        record_field_rule(error).or_else(|| actor_rule(&error.code))
    }
}
/// Rules over single record fields, keyed by the `RECORD.FIELD` that opens
/// the diagnostic's `found` text.
fn record_field_rule(error: &Error) -> Option<&'static str> {
    let mut parts = error.found.split('.');
    let (record, field) = (parts.next()?, parts.next()?);
    match (error.code.as_str(), record, field) {
        ("UninterpretedVisualSubrecord" | "UnsupportedUsedFieldLayout", r, "PRPS")
            if PROPERTY_BASES.contains(&r) =>
        {
            Some(BASE_PROPERTIES)
        }
        ("UninterpretedVisualSubrecord", "MISC", "CVPA") => Some(MISC_COMPONENTS),
        ("RuntimeRecordState", "REFR", "XRFG") => Some(REFERENCE_GROUP),
        ("RuntimeRecordState", "REFR", "XPRM") => Some(REFERENCE_PRIMITIVE),
        _ => None,
    }
}
fn actor_rule(code: &str) -> Option<&'static str> {
    match code {
        "UnverifiedActorPlacement" => Some(ACTOR_PLACEMENT),
        "UnverifiedActorCandidates" => Some(ACTOR_CANDIDATES),
        "UnverifiedActorOutfit" => Some(ACTOR_OUTFIT),
        "UnverifiedActorSection" => Some(ACTOR_SECTION),
        "UnverifiedActorAddons" => Some(ACTOR_ADDONS),
        "UnverifiedActorSkinSlots" => Some(ACTOR_SLOTS),
        "UnverifiedActorSkinning" => Some(ACTOR_SKINNING),
        "UnverifiedActorHeadParts" => Some(ACTOR_HEAD),
        "UnverifiedActorLeveledFlags" => Some(ACTOR_LEVELED_FLAGS),
        "UnverifiedActorTemplateAllBits" => Some(ACTOR_TEMPLATE_ALL_BITS),
        "UnverifiedActorTemplateBits" => Some(ACTOR_TEMPLATE_BITS),
        "UnverifiedActorHeadSlots" => Some(ACTOR_HEAD_SLOTS),
        "UnverifiedActorHeadConditions" => Some(ACTOR_HEAD_CONDITIONS),
        "UnverifiedActorHeadRaceFill" => Some(ACTOR_HEAD_RACE_FILL),
        "UnverifiedActorUnreadSlots" => Some(ACTOR_UNREAD_SLOTS),
        "UnverifiedActorLeveledPick" => Some(ACTOR_LEVELED_PICK),
        "UnverifiedActorLeveledChanceNone" => Some(ACTOR_LEVELED_CHANCE_NONE),
        "UnverifiedActorLeveledUseAll" => Some(ACTOR_LEVELED_USE_ALL),
        "UnverifiedModelOwnNodes" => Some(MODEL_OWN_NODES),
        "UnverifiedPlayPlatform" => Some(PLAY_PLATFORM_DRAWN),
        "UnverifiedPlayPlatformLeftOut" => Some(PLAY_PLATFORM_LEFT_OUT),
        "UnverifiedModelOwnNodesEvery" => Some(MODEL_OWN_NODES_EVERY),
        "UnverifiedTimelineRide" => Some(TIMELINE_RIDE),
        "UnverifiedActorClipSkeleton" => Some(ACTOR_CLIP_SKELETON),
        "UnverifiedActorClipPose" => Some(ACTOR_CLIP_POSE),
        _ => None,
    }
}
pub fn classify(errors: &[Error], outside: bool, b: &Budget) -> Result<State> {
    classify_with(errors, outside, status, b)
}
/// `classify` against an explicit status table instead of the embedded registry.
pub fn classify_with(
    errors: &[Error],
    outside: bool,
    status: impl Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<State> {
    classify_aside_with(errors, outside, |_| None, status, b)
}
/// [`classify`] where `aside(k)` names the rule under which a switch sets
/// `errors[k]` aside: that rule then covers it in place of its own (a
/// switch's reading of a blocker no rule covered, such as
/// `Loader::set_render_child_flag_agrees`). With `aside` always `None` it
/// is [`classify`].
pub fn classify_aside(
    errors: &[Error],
    outside: bool,
    aside: impl Fn(usize) -> Option<&'static str>,
    b: &Budget,
) -> Result<State> {
    classify_aside_with(errors, outside, aside, status, b)
}
/// [`classify_aside`] against an explicit status table.
pub fn classify_aside_with(
    errors: &[Error],
    outside: bool,
    aside: impl Fn(usize) -> Option<&'static str>,
    status: impl Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<State> {
    if errors.is_empty() {
        return Ok(if outside {
            State::Outside
        } else {
            State::Ready
        });
    }
    let mut hypotheses = Vec::new();
    let mut pending = false;
    for (k, e) in errors.iter().enumerate() {
        let Some(h) = aside(k).or_else(|| covering(e)) else {
            return Ok(State::Blocked);
        };
        match status(h) {
            Some(Status::Refuted) => return Ok(State::Blocked),
            Some(Status::Supported) => {}
            Some(Status::Pending) => pending = true,
            None => {
                return Err(Error::new(
                    "UnregisteredHypothesis",
                    &e.source.file,
                    e.source.offset,
                    "registered hypothesis with a known status",
                    h,
                ))
            }
        }
        if !hypotheses.contains(&h) {
            b.reserve(&mut hypotheses, 1, &e.source.file, e.source.offset)?;
            hypotheses.push(h);
        }
    }
    hypotheses.sort_unstable();
    Ok(if pending {
        State::ReadyUnderHypotheses { hypotheses }
    } else {
        State::ReadyMeasured { rules: hypotheses }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hypotheses_are_narrow_and_never_erase_diagnostics() {
        let b = Budget::new(4096);
        // Explicit standing, so this test does not change with each new measurement.
        let table = |name: &str| match name {
            MODT => Some(Status::Supported),
            BLEND => Some(Status::Pending),
            ROTATION => Some(Status::Refuted),
            _ => None,
        };
        let mut e = vec![Error::new(
            "UninterpretedVisualSubrecord",
            "synthetic",
            13,
            "known",
            "STAT.MODT.layout_not_established",
        )];
        assert!(matches!(
            classify_with(&[], false, table, &b).unwrap(),
            State::Ready
        ));
        assert!(
            matches!(classify_with(&e, false, table, &b).unwrap(), State::ReadyMeasured{rules} if rules == [MODT])
        );
        e.push(Error::new(
            "UnsupportedMaterialFields",
            "synthetic",
            17,
            "known",
            "alpha_blend.enabled_factors_unsourced(1,6,7)",
        ));
        assert!(
            matches!(classify_with(&e, false, table, &b).unwrap(), State::ReadyUnderHypotheses{hypotheses} if hypotheses == [BLEND,MODT])
        );
        e.push(Error::new(
            "UnknownField",
            "synthetic",
            21,
            "known",
            "STAT.MODT",
        ));
        assert!(matches!(
            classify_with(&e, false, table, &b).unwrap(),
            State::Blocked
        ));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].source.offset, 13);
        assert!(classify_with(&e[..2], false, table, &Budget::new(0)).is_err());
        // A refuted rule blocks; an unregistered one is an error, never silent readiness.
        let rotation = [Error::new(
            "UnverifiedEulerRotation",
            "synthetic",
            1,
            "measured",
            "angles",
        )];
        assert!(matches!(
            classify_with(&rotation, false, table, &b).unwrap(),
            State::Blocked
        ));
        let scale = [Error::new(
            "MissingDocumentedReferenceScale",
            "synthetic",
            1,
            "measured",
            "absent",
        )];
        assert_eq!(
            classify_with(&scale, false, table, &b).unwrap_err().code,
            "UnregisteredHypothesis"
        );
        for text in [
            "STAT.MODTfake",
            "FAKE.STAT.MODT",
            "alpha_blend.enabled_factors_unsourced(1,9,9)",
        ] {
            assert!(covering(&Error::new(
                "UninterpretedVisualSubrecord",
                "test",
                0,
                "known",
                text
            ))
            .is_none());
        }
    }
    #[test]
    fn every_rule_the_code_names_has_a_standing_in_the_registry() {
        for name in [
            VISIBILITY_MARKER,
            MARKER_SUFFIX,
            PATH_STRIP,
            PATH_ROOT,
            PATH_DATA_RELATIVE,
            EFFECT_COLOR,
            CULL,
            MODT,
            BLEND,
            ROTATION,
            SCOL,
            SCALE,
            MARKER,
            NAMED_MARKER,
            COLLISION_UNITS,
            TARGET_FRAME,
            BODY_ID,
            BODY_PLACEMENT_IGNORED,
            BODY_PLACEMENT_APPLIED,
            COMPOUND_INSTANCE,
            SCALED_CONVEX,
            MESH_TRIANGLES,
            CONVEX_RADIUS,
            SWAP_COLLISION,
            RENDER_INPUTS_COLLISION,
            COLLISION_OFF_CHAIN,
            SEQUENCE_HELD_POSE,
            SEQUENCE_CHAIN_POSE,
            REFERENCE_ALPHA_COLLISION,
            REFERENCE_ALPHA_RENDER,
            REFERENCE_SCRIPT_RENDER,
            ACTIVATOR_FIELDS_RENDER,
            ACTIVATOR_RECEIVER_RENDER,
            LIGHTING_REFRACTION_LEFT_OUT,
            FURNITURE_FIELDS_RENDER,
            FURNITURE_TEMPLATE_RENDER,
            ACTIVATOR_PARTS,
            FURNITURE_PARTS,
            FURNITURE_AT_REST,
            RAGDOLL_LEFT_OUT,
            WORLD_PERSISTENT_IN_SQUARE,
            STATIC_REFERENCE_FIELDS_COLLISION,
            ACTIVATOR_FIELDS_COLLISION,
            FURNITURE_FIELDS_COLLISION,
            FURNITURE_COLLISION_AT_REST,
            STATIC_REFERENCE_FIELDS_RENDER,
            STATIC_LEAF_TWO_POSE,
            PLATFORM_SKIN_FOLLOWS_PARTS,
            PLATFORM_SHAPE_NEAREST_PART,
            CAST_ON_PLATFORMS,
            CAST_KEEPS_PART,
            LAND_OF_CELL,
            LAND_ACCUMULATION,
            LAND_UNITS,
            LAND_ORIGIN,
            LAND_SPACING,
            LAND_DIAGONAL_SW_NE,
            LAND_DIAGONAL_NW_SE,
            LTEX_MATERIAL,
            LAND_DEFAULT_NAMED,
            ACTOR_SKINNING,
            ACTOR_TEMPLATE_BITS,
            ACTOR_HEAD_SLOTS,
            ACTOR_HEAD_CONDITIONS,
            ACTOR_HEAD_RACE_FILL,
            ACTOR_UNREAD_SLOTS,
            ACTOR_LEVELED_PICK,
            ACTOR_LEVELED_CHANCE_NONE,
            ACTOR_LEVELED_USE_ALL,
            MODEL_OWN_NODES,
            PLAY_PLATFORM_DRAWN,
            PLAY_PLATFORM_LEFT_OUT,
            TIMELINE_RIDE,
            ACTOR_CLIP_SKELETON,
            ACTOR_CLIP_POSE,
            CHAR_GRAPH_START,
            CHAR_GRAPH_SUBGRAPH,
            CHAR_GRAPH_SUBGRAPH_FLAG_ONE,
            ACTOR_MODEL_GRAPH_START_IDLE,
            ACTOR_EFFECT_STORED,
            MODEL_OWN_NODES_EVERY,
            CHAR_GRAPH_CLIP_FOLDER,
            CHAR_GRAPH_CLIP_LOOP,
            CHAR_GRAPH_FURNITURE,
            TIMELINE_DEAD_LEFT_OUT,
            TIMELINE_DEAD_LYING,
            TIMELINE_SIT_MARKER,
            FURNITURE_MARKER_SNAM,
            ANIMATED_COLLISION_AS_STORED,
            EXPOSURE_EYE,
            EXPOSURE_FLOOR,
            EXPOSURE_VISIBLE,
            IMAGE_SPACE_WHITE,
            IMAGE_SPACE_CINEMATIC,
            LIGHT_VERTEX_NORMALS,
            SWAP_FOLDER,
            OPACITY_ABOVE_ONE,
            BASE_PROPERTIES,
            MISC_COMPONENTS,
            REFERENCE_GROUP,
            ROOT_TRANSFORM,
            ENABLE_FOLLOWS,
            ENABLE_INITIAL,
            ENABLE_OPPOSITE,
            ENABLE_BIT1,
            DISABLE_HIDES,
            WORLD_CELL,
            TIMELINE_MOVE,
            TIMELINE_NOWAIT,
            ENABLE_CHILD_FLAG_AGREES,
            TIMELINE_ACTOR_ROTATION,
            TIMELINE_ACTOR_TARGET_ROTATION,
            ACTOR_MISSING_BONE_BIND,
            ACTOR_MISSING_BONE_NEAREST,
            LIGHT_CELL,
            LIGHT_INHERITS,
            LIGHT_POINTS,
            LIGHT_REFERENCE_RADIUS,
            LIGHT_FADE,
            LIGHT_CELL_FOG,
            LIGHT_SUM,
            CLIMATE_SINGLE_WEATHER,
            CLIMATE_HOUR_SLOT,
            WEATHER_SKY,
            WEATHER_AMBIENT,
            WEATHER_FOG,
            ENABLE_SCRIPT,
            REST_MANAGER_IDLE,
            REST_IDLE_TRANSFORM,
            REST_IDLE_SHADER,
            REST_TRANSFORM_CONSTANT,
            REST_UNRENDERED,
            REST_EFFECT_FIRST_KEY,
            EFFECT_SHADING,
            EFFECT_BLEND,
            EFFECT_ORDER,
            EFFECT_LEFT_OUT,
            EFFECT_FALLOFF,
            EFFECT_FALLOFF_ABSOLUTE,
            EFFECT_FALLOFF_LINEAR,
            EFFECT_FALLOFF_ALPHA,
            EFFECT_VERTEX_ALPHA_COLORS,
            REST_FALLOFF_DEGREES,
            EFFECT_PALETTE_SAMPLED,
            EFFECT_PALETTE_V,
            EFFECT_PALETTE_COLOUR_U,
            EFFECT_PALETTE_COLOUR_RGB,
            EFFECT_PALETTE_ALPHA_U,
            EFFECT_PALETTE_ALPHA_REPLACES,
            EFFECT_COLOUR_CLAMPED,
            EFFECT_ENVIRONMENT_ADDS,
            EFFECT_ENVIRONMENT_LEFT_OUT,
            EFFECT_MATERIAL_OVER_CONTROLLER,
            BLEND_OPACITY_CLAMPED,
            MISSING_MATERIAL_EMBEDDED,
            MISSING_PARENT,
            SWAP_SCOPES_AGREE,
            REST_TEXTURE_FIRST_KEY,
            UV_SCALE_THEN_OFFSET,
            CLOCK_FREE_RUNNING,
            CLOCK_CYCLE_LOOP,
            CLOCK_CYCLE_REVERSE,
            CLOCK_CYCLE_CLAMP,
            CLOCK_KEY_LINEAR,
            CLOCK_KEY_QUADRATIC,
            CLOCK_KEY_STEP,
            REST_LOOP_UNSTARTED,
            CLOCK_EMISSIVE_MULTIPLE,
            CLOCK_EMISSIVE_COLOUR,
            CLOCK_FALLOFF,
            CLOCK_TRANSLATION,
            CLOCK_ROTATION_QUATERNION,
            CLOCK_ROTATION_XYZ,
            CLOCK_SCALE,
        ] {
            assert!(status(name).is_some(), "{name}");
        }
        assert_eq!(status("refr_rotation_order_v1"), Some(Status::Refuted));
        assert_eq!(status("not_a_registered_rule"), None);
    }
    #[test]
    fn record_field_rules_cover_only_the_measured_fields() {
        let e = |code: &str, found: &str| Error::new(code, "synthetic", 0, "known", found);
        for (code, found, rule) in [
            (
                "UninterpretedVisualSubrecord",
                "STAT.PRPS.layout_not_established",
                Some(BASE_PROPERTIES),
            ),
            (
                "UninterpretedVisualSubrecord",
                "FURN.PRPS",
                Some(BASE_PROPERTIES),
            ),
            (
                "UnsupportedUsedFieldLayout",
                "ACTI.PRPS.x",
                Some(BASE_PROPERTIES),
            ),
            ("UninterpretedVisualSubrecord", "DOOR.PRPS", None),
            ("UninterpretedVisualSubrecord", "STAT.PRPSX", None),
            ("UninterpretedVisualSubrecord", "PRPS", None),
            (
                "UninterpretedVisualSubrecord",
                "MISC.CVPA.layout_not_established",
                Some(MISC_COMPONENTS),
            ),
            ("UninterpretedVisualSubrecord", "CONT.CVPA", None),
            ("RuntimeRecordState", "REFR.XRFG", Some(REFERENCE_GROUP)),
            ("UninterpretedVisualSubrecord", "REFR.XRFG", None),
            ("RuntimeRecordState", "ACHR.XRFG", None),
            ("RuntimeRecordState", "REFR.XPRM", Some(REFERENCE_PRIMITIVE)),
            (
                "UninterpretedVisualSubrecord",
                "REFR.XPRM.layout_not_established",
                None,
            ),
            ("RuntimeRecordState", "ACHR.XPRM", None),
            ("RuntimeMaterialSwapFilter", "FNAM", Some(SWAP_FOLDER)),
            ("MissingSwapPatternTarget", "", Some(SWAP_PATTERN_MISSING)),
        ] {
            assert_eq!(covering(&e(code, found)), rule, "{code} {found}");
        }
    }
    #[test]
    fn readiness_levels_nest() {
        use crate::RuleLevel::{Facts, Hypotheses, Measured};
        let measured = State::ReadyMeasured { rules: vec![MODT] };
        let pending = State::ReadyUnderHypotheses {
            hypotheses: vec![BLEND, MODT],
        };
        for (state, expected) in [
            (&State::Ready, [true, true, true]),
            (&measured, [false, true, true]),
            (&pending, [false, false, true]),
            (&State::Blocked, [false, false, false]),
            (&State::Outside, [false, false, false]),
        ] {
            assert_eq!(
                [Facts, Measured, Hypotheses].map(|level| state.ready_at(level)),
                expected
            );
        }
        assert_eq!(measured.hypotheses(), [MODT]);
        assert_eq!(pending.hypotheses(), [BLEND, MODT]);
    }
    #[test]
    fn the_status_read_once_is_the_scan_of_the_rows() {
        let scan = |name: &str| {
            REGISTRY.lines().find_map(|row| {
                let mut columns = row.split('\t');
                if columns.next() != Some(name) {
                    return None;
                }
                match columns.nth(4) {
                    Some("pending") => Some(Status::Pending),
                    Some("supported") => Some(Status::Supported),
                    Some("refuted") => Some(Status::Refuted),
                    _ => None,
                }
            })
        };
        let mut n = 0;
        for row in REGISTRY.lines() {
            let name = row.split('\t').next().unwrap_or("");
            assert_eq!(status(name), scan(name), "{name}");
            n += 1;
        }
        assert!(n > 400);
        assert_eq!(status("no_such_rule_v1"), None);
        assert_eq!(status(MODT), Some(Status::Supported));
    }
    #[test]
    fn the_spec_index_lists_every_registered_rule_with_its_status() {
        let index = include_str!("../../../spec/README.md");
        let mut rules = 0;
        for row in REGISTRY.lines().filter(|row| !row.starts_with('#')) {
            let mut columns = row.split('\t');
            let (Some(name), Some(status)) = (columns.next(), columns.nth(4)) else {
                continue;
            };
            assert!(
                index.contains(&format!("| `{name}` | {status} |")),
                "spec/README.md is out of step with hypotheses.tsv for {name}"
            );
            rules += 1;
        }
        assert!(rules >= 16);
    }
}
