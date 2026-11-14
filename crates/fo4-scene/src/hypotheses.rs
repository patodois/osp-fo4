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
        ] {
            assert!(status(name).is_some(), "{name}");
        }
        assert_eq!(status("refr_rotation_order_v1"), Some(Status::Refuted));
        assert_eq!(status("not_a_registered_rule"), None);
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
}
