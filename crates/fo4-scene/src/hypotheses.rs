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
    } else {
        None
    }
}
pub fn classify(errors: &[Error], outside: bool, b: &Budget) -> Result<State> {
    if errors.is_empty() {
        return Ok(if outside {
            State::Outside
        } else {
            State::Ready
        });
    }
    let mut hypotheses = Vec::new();
    for e in errors {
        let Some(h) = covering(e) else {
            return Ok(State::Blocked);
        };
        if !REGISTRY
            .lines()
            .any(|row| row.split('\t').next() == Some(h))
        {
            return Err(Error::new(
                "UnregisteredHypothesis",
                &e.source.file,
                e.source.offset,
                "registered hypothesis",
                h,
            ));
        }
        if !hypotheses.contains(&h) {
            b.reserve(&mut hypotheses, 1, &e.source.file, e.source.offset)?;
            hypotheses.push(h);
        }
    }
    hypotheses.sort_unstable();
    Ok(State::ReadyUnderHypotheses { hypotheses })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hypotheses_are_narrow_and_never_erase_diagnostics() {
        let b = Budget::new(4096);
        let mut e = vec![Error::new(
            "UninterpretedVisualSubrecord",
            "synthetic",
            13,
            "known",
            "STAT.MODT.layout_not_established",
        )];
        assert!(matches!(classify(&[], false, &b).unwrap(), State::Ready));
        assert!(
            matches!(classify(&e, false, &b).unwrap(), State::ReadyUnderHypotheses{hypotheses} if hypotheses == [MODT])
        );
        e.push(Error::new(
            "UnsupportedMaterialFields",
            "synthetic",
            17,
            "known",
            "alpha_blend.enabled_factors_unsourced(1,6,7)",
        ));
        assert!(
            matches!(classify(&e, false, &b).unwrap(), State::ReadyUnderHypotheses{hypotheses} if hypotheses == [BLEND,MODT])
        );
        e.push(Error::new(
            "UnknownField",
            "synthetic",
            21,
            "known",
            "STAT.MODT",
        ));
        assert!(matches!(classify(&e, false, &b).unwrap(), State::Blocked));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].source.offset, 13);
        assert!(classify(&e[..2], false, &Budget::new(0)).is_err());
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
}
