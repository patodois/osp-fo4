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
pub const UV_SCALE_THEN_OFFSET: &str = "uv_scale_then_offset_v1";
pub const MISSING_PARENT: &str = "bgsm_missing_parent_own_fields_v1";
/// A missing texture cited only in slots the unlit frame never samples
/// (`crate::missing::unread_textures`).
pub const MISSING_TEXTURE_UNREAD: &str = "texture_missing_unread_slot_v1";
/// A script's Enable or Disable reaching enable children (the viewer's
/// `--script-event`).
pub const ENABLE_SCRIPT: &str = "xesp_script_change_propagates_v1";
/// Base types whose PRPS `BASE_PROPERTIES` covers: those measured.
const PROPERTY_BASES: [&str; 8] = [
    "STAT", "ACTI", "FURN", "CONT", "FLOR", "LIGH", "MSTT", "TERM",
];
pub const RENDER_INPUTS_COLLISION: &str = "render_inputs_collision_unchanged_v1";
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
/// A clip posing an actor (`crate::actor::clip`, `spec/actor.md`, Clip pose).
pub const ACTOR_CLIP_SKELETON: &str = "actor_clip_skeleton_beside_nif_v1";
pub const ACTOR_CLIP_POSE: &str = "actor_clip_pose_by_bone_name_v1";
/// A rule's standing in `spec/hypotheses.tsv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    /// Confirmed by measurement of the running game, within the recorded scope.
    Supported,
    Refuted,
}
pub fn status(name: &str) -> Option<Status> {
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
    if errors.is_empty() {
        return Ok(if outside {
            State::Outside
        } else {
            State::Ready
        });
    }
    let mut hypotheses = Vec::new();
    let mut pending = false;
    for e in errors {
        let Some(h) = covering(e) else {
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
            ACTOR_CLIP_SKELETON,
            ACTOR_CLIP_POSE,
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
