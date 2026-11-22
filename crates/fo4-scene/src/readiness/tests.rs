use super::*;
use crate::hypotheses::{BLEND, PATH_STRIP, RENDER_INPUTS_COLLISION, ROTATION, SWAP_COLLISION};

fn e(code: &str, found: &str) -> Error {
    Error::new(code, "synthetic", 1, "known", found)
}
/// Explicit standings, so the tests do not change with each measurement.
fn table(swap: Status, render: Status, blend: Status) -> impl Fn(&str) -> Option<Status> {
    move |name: &str| match name {
        SWAP_COLLISION => Some(swap),
        RENDER_INPUTS_COLLISION => Some(render),
        BLEND => Some(blend),
        ROTATION | PATH_STRIP => Some(Status::Supported),
        _ => None,
    }
}
fn state(
    blockers: &[(Error, Origin)],
    swap: bool,
    status: impl Fn(&str) -> Option<Status>,
) -> State {
    let errors: Vec<Error> = blockers.iter().map(|(e, _)| e.clone()).collect();
    let origins: Vec<Origin> = blockers.iter().map(|(_, o)| *o).collect();
    collision_state_with(
        &errors,
        &origins,
        false,
        swap,
        status,
        &Budget::new(1 << 20),
    )
    .unwrap()
}
fn rules(s: &State) -> Vec<&'static str> {
    s.hypotheses().to_vec()
}

#[test]
fn render_only_blockers_are_set_aside_under_a_rule() {
    use Status::{Pending, Refuted, Supported};
    let pending = table(Pending, Pending, Pending);
    // No blocker, no swap: ready on facts alone, as for render.
    assert!(matches!(state(&[], false, &pending), State::Ready));
    // A material swap is not applied to collision: its rule is needed even
    // when the swap itself raised nothing.
    let s = state(&[], true, &pending);
    assert!(matches!(s, State::ReadyUnderHypotheses { .. }) && rules(&s) == [SWAP_COLLISION]);
    let s = state(&[], true, table(Supported, Pending, Pending));
    assert!(matches!(s, State::ReadyMeasured { .. }));
    assert!(matches!(
        state(&[], true, table(Refuted, Pending, Pending)),
        State::Blocked
    ));
    // The swap's FNAM needs its own rule for render (refuted there, it
    // blocks), and only the swap rule for collision.
    let filter = (e("RuntimeMaterialSwapFilter", "FNAM"), Origin::MaterialSwap);
    let errors = [filter.0.clone()];
    let folder = |refuted: bool| {
        move |name: &str| match name {
            hypotheses::SWAP_FOLDER if refuted => Some(Status::Refuted),
            hypotheses::SWAP_FOLDER => Some(Status::Pending),
            _ => None,
        }
    };
    assert!(matches!(
        hypotheses::classify_with(&errors, false, folder(true), &Budget::new(1 << 20)).unwrap(),
        State::Blocked
    ));
    assert!(matches!(
        hypotheses::classify_with(&errors, false, folder(false), &Budget::new(1 << 20)).unwrap(),
        State::ReadyUnderHypotheses { .. }
    ));
    let s = state(std::slice::from_ref(&filter), true, &pending);
    assert!(s.ready_at(crate::RuleLevel::Hypotheses) && rules(&s) == [SWAP_COLLISION]);
    assert!(!s.ready_at(crate::RuleLevel::Measured));
    // Whatever a swap raises is the swap's, even a code collision needs elsewhere.
    let s = state(
        &[(e("RejectedVfsPath", "x"), Origin::MaterialSwap)],
        true,
        &pending,
    );
    assert_eq!(rules(&s), [SWAP_COLLISION]);
    // Material and texture files, and a model's citations of them.
    for (code, origin) in [
        ("MissingTexture", Origin::RenderAsset),
        ("SomeReaderError", Origin::RenderAsset),
        ("RejectedVfsPath", Origin::ModelCitation),
        ("UnverifiedEffectColorOnly", Origin::Model),
        ("UnsupportedShaderMaterialName", Origin::Model),
        ("UnsupportedSkinning", Origin::Model),
        ("UnverifiedNifEditorMarkerVisibility", Origin::Model),
        ("UnsupportedLightData", Origin::Record),
    ] {
        let s = state(&[(e(code, "x"), origin)], false, &pending);
        assert_eq!(rules(&s), [RENDER_INPUTS_COLLISION], "{code}");
        assert!(matches!(
            state(
                &[(e(code, "x"), origin)],
                false,
                table(Pending, Refuted, Pending)
            ),
            State::Blocked
        ));
    }
    // A render-only blocker with its own rule: the better standing wins,
    // the collision rule on a tie, so a supported render rule keeps the
    // collision at Measured.
    let blend = (
        e(
            "UnsupportedMaterialFields",
            "alpha_blend.enabled_factors_unsourced(1,6,7)",
        ),
        Origin::RenderAsset,
    );
    let s = state(std::slice::from_ref(&blend), false, &pending);
    assert_eq!(rules(&s), [RENDER_INPUTS_COLLISION]);
    let s = state(
        std::slice::from_ref(&blend),
        false,
        table(Pending, Pending, Supported),
    );
    assert!(matches!(&s, State::ReadyMeasured { rules } if rules == &[BLEND]));
    let s = state(&[blend], false, table(Pending, Supported, Supported));
    assert!(matches!(&s, State::ReadyMeasured { rules } if rules == &[RENDER_INPUTS_COLLISION]));
}

#[test]
fn blockers_collision_needs_count_as_for_render() {
    let pending = table(Status::Pending, Status::Pending, Status::Pending);
    // A rule-covered placement blocker keeps its rule.
    let s = state(
        &[(e("UnverifiedEulerRotation", "angles"), Origin::Record)],
        false,
        &pending,
    );
    assert!(matches!(&s, State::ReadyMeasured { rules } if rules == &[ROTATION]));
    // The path rule of the record's own model citation is needed.
    let s = state(
        &[(e("HypothesisAssetDataPrefixStrip", "x"), Origin::Record)],
        false,
        &pending,
    );
    assert_eq!(rules(&s), [PATH_STRIP]);
    // Uncovered blockers of the model, the record and unknown codes block.
    for (code, origin) in [
        ("UnsupportedNifBlock", Origin::Model),
        ("UnsupportedAnimationRuntime", Origin::Model),
        ("SomeReaderError", Origin::Model),
        ("RuntimeEnableParent", Origin::Record),
        ("MissingModel", Origin::Model),
    ] {
        let s = state(
            &[
                (e("MissingTexture", "x"), Origin::RenderAsset),
                (e(code, "x"), origin),
            ],
            true,
            &pending,
        );
        assert!(matches!(s, State::Blocked), "{code}");
    }
    // An unregistered covering rule is an error, as in `classify_with`.
    let none = |_: &str| None;
    let errors = [e("UnverifiedEulerRotation", "angles")];
    assert_eq!(
        collision_state_with(
            &errors,
            &[Origin::Record],
            false,
            false,
            none,
            &Budget::new(1 << 20)
        )
        .unwrap_err()
        .code,
        "UnregisteredHypothesis"
    );
    // A missing origin counts as the record's.
    assert!(matches!(
        collision_state_with(
            &[e("UnsupportedSkinning", "x"), e("UnsupportedNifBlock", "x")],
            &[Origin::Model],
            false,
            false,
            &pending,
            &Budget::new(1 << 20)
        )
        .unwrap(),
        State::Blocked
    ));
    // Outside the static frame with nothing collision needs.
    assert!(matches!(
        collision_state_with(&[], &[], true, false, &pending, &Budget::new(1 << 20)).unwrap(),
        State::Outside
    ));
    assert!(collision_state_with(
        &errors,
        &[Origin::Record],
        false,
        false,
        &pending,
        &Budget::new(0)
    )
    .is_err());
}

#[test]
fn every_blocker_code_the_loader_raises_is_classified() {
    let mut names: Vec<&str> = Code::ALL.iter().map(|c| c.name()).collect();
    assert!(names.iter().all(|n| Code::of(n) != Code::Other));
    names.sort_unstable();
    let len = names.len();
    names.dedup();
    assert_eq!(names.len(), len);
    assert_eq!(Code::of("NotACode"), Code::Other);
    // Literal codes in the files that raise instance blockers: each is a
    // named code (and so classified by the exhaustive match), or one of
    // these, which never become a blocker of an instance.
    const NOT_INSTANCE_BLOCKERS: &[&str] = &[
        // Cell diagnostics and exclusions.
        "MissingCellEditorId",
        "MissingCell",
        "DeletedCell",
        "MissingLightingTemplate",
        "DeletedLightingTemplate",
        "MissingReference",
        "UnreadableReference",
        "UnsupportedDistantReferenceGroup",
        "UnsupportedPlacedRecordType",
        // Serde names and codes compared, not raised.
        "UnderHypotheses",
        "MissingPath",
        "FutureMystery",
    ];
    for (file, text) in [
        ("lib.rs", include_str!("../lib.rs")),
        ("assets.rs", include_str!("../assets.rs")),
        ("paths.rs", include_str!("../paths.rs")),
    ] {
        let mut found = 0;
        for literal in text.split('"').skip(1).step_by(2) {
            let mut humps = literal.split(|c: char| c.is_ascii_uppercase());
            let camel = literal.len() > 1
                && literal.chars().all(|c| c.is_ascii_alphanumeric())
                && literal.starts_with(|c: char| c.is_ascii_uppercase())
                && literal
                    .chars()
                    .nth(1)
                    .is_some_and(|c| c.is_ascii_lowercase())
                && humps.nth(2).is_some();
            if !camel {
                continue;
            }
            found += 1;
            assert!(
                Code::of(literal) != Code::Other || NOT_INSTANCE_BLOCKERS.contains(&literal),
                "{file}: {literal} is not classified for collision readiness"
            );
        }
        assert!(found > 0, "{file}");
    }
}
