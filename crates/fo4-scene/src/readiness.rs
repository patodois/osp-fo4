//! Collision readiness of a placed reference, kept apart from its render
//! readiness (`spec/collision-placement.md`, Collision readiness).
//!
//! The scene's [`State`] says whether a reference can be drawn: every input
//! the renderer reads must be covered by facts or registered rules. Its
//! collision reads less: the reference and base records, the placement
//! rules they use (rotation, scale, SCOL part frames), the model file and
//! its collision payload. A blocker on an input only the renderer reads
//! (a material swap, a material or texture file, a shader or visibility
//! field) does not stop the collision; setting it aside is itself a claim
//! about the game, so the collision then relies on a registered rule
//! instead: [`hypotheses::SWAP_COLLISION`] for material swaps,
//! [`hypotheses::RENDER_INPUTS_COLLISION`] for the other render inputs.
//!
//! Which blockers collision needs is decided by where the blocker comes
//! from ([`Origin`]) and, for the record and the model file, by its code
//! ([`Code::collision_use`], an exhaustive match). A code this crate does
//! not name ([`Code::Other`], for example a reader error) counts as needed.
use crate::hypotheses::{self, covering, Status};
use crate::State;
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Where a blocker of an instance was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Origin {
    /// The reference, its base records, SCOL parts and the records' own
    /// model citations.
    Record,
    /// A material swap of the reference or of a base: the MSWP record, its
    /// filter and its material citations.
    MaterialSwap,
    /// A model file: its own read, blocks and fields.
    Model,
    /// A material or texture citation inside a model file, or the file it
    /// names.
    ModelCitation,
    /// A material or texture file, or anything it cites.
    RenderAsset,
}

/// What a blocker concerns, for collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Use {
    /// Collision needs the input: the blocker counts as it does for render.
    Collision,
    /// Only the renderer reads the input: collision sets the blocker aside
    /// under a registered rule.
    RenderOnly,
}

macro_rules! codes {
    ($($name:ident),* $(,)?) => {
        /// Blocker codes the scene loader and the asset graph raise.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        pub enum Code {
            $($name,)*
            /// Any other code, such as an error of a file reader.
            Other,
        }
        impl Code {
            /// Every named code ([`Code::Other`] excluded).
            pub const ALL: &'static [Self] = &[$(Self::$name),*];
            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name),)*
                    Self::Other => "Other",
                }
            }
        }
    };
}
codes!(
    // Reference and base records.
    MissingPlacementTransform,
    RuntimeEnableParent,
    RuntimeCondition,
    RuntimeRecordState,
    UnsupportedUsedFieldLayout,
    UninterpretedVisualSubrecord,
    MissingBaseForm,
    MissingRecord,
    BaseTypeOutsideSlice,
    ScolCycle,
    DeletedBaseForm,
    MissingModelPath,
    UnsupportedLightData,
    UnverifiedScolGeometrySource,
    UnverifiedEulerRotation,
    MissingScolPartBase,
    MissingScolPlacements,
    MissingDocumentedReferenceScale,
    // Material swaps.
    DeletedMaterialSwap,
    MissingMaterialSwap,
    RuntimeMaterialSwapFilter,
    // Paths.
    HypothesisAssetDataPrefixStrip,
    HypothesisAssetDataRelativeStrip,
    HypothesisAssetMissingRoot,
    RejectedVfsPath,
    PathCandidateUnavailable,
    PathCandidateWrongRoot,
    // Files of the asset graph.
    MissingModel,
    MissingMaterial,
    MissingMaterialEmbeddedShader,
    MissingMaterialParent,
    MissingTexture,
    AssetDependencyCycle,
    UnverifiedEffectColorOnly,
    UnverifiedVisibilityEditorMarker,
    UnverifiedNifEditorMarkerVisibility,
    UnverifiedNifEditorMarkerSuffixVisibility,
    UnsupportedNifBlockSemantics,
    UnsupportedAnimationRuntime,
    UnsupportedNifBlock,
    UnsupportedNifBlockTail,
    UnsupportedNifField,
    UnsupportedSkinning,
    UnsupportedShaderMaterialName,
    UnsupportedMaterialFields,
    // Bounds and bookkeeping.
    AllocationFailed,
    AssetCacheUnavailable,
    InvalidAssetIndex,
    LimitExceeded,
    Overflow,
);

impl Code {
    pub fn of(code: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.name() == code)
            .unwrap_or(Self::Other)
    }
    /// Whether collision needs the input a blocker with this code concerns,
    /// when it is raised on a record or a model file. Every code is listed:
    /// a new one cannot be added without a decision here.
    pub fn collision_use(self) -> Use {
        use Use::{Collision, RenderOnly};
        match self {
            // Placement, presence at run time and the base itself.
            Self::MissingPlacementTransform
            | Self::RuntimeEnableParent
            | Self::RuntimeCondition
            | Self::MissingBaseForm
            | Self::MissingRecord
            | Self::BaseTypeOutsideSlice
            | Self::ScolCycle
            | Self::DeletedBaseForm
            | Self::MissingModelPath
            | Self::UnverifiedScolGeometrySource
            | Self::UnverifiedEulerRotation
            | Self::MissingScolPartBase
            | Self::MissingScolPlacements
            | Self::MissingDocumentedReferenceScale => Collision,
            // An uninterpreted record field could bear on placement; MODT
            // keeps its own rule.
            Self::RuntimeRecordState
            | Self::UnsupportedUsedFieldLayout
            | Self::UninterpretedVisualSubrecord => Collision,
            // Raised on a record, a path names the model itself.
            Self::HypothesisAssetDataPrefixStrip
            | Self::HypothesisAssetDataRelativeStrip
            | Self::HypothesisAssetMissingRoot
            | Self::RejectedVfsPath
            | Self::PathCandidateUnavailable
            | Self::PathCandidateWrongRoot => Collision,
            // The model file: its read, blocks and fields reach the node
            // chain and the collision blocks; an animation can move a
            // collision Target.
            Self::MissingModel
            | Self::AssetDependencyCycle
            | Self::UnsupportedNifBlockSemantics
            | Self::UnsupportedAnimationRuntime
            | Self::UnsupportedNifBlock
            | Self::UnsupportedNifBlockTail
            | Self::UnsupportedNifField => Collision,
            Self::AllocationFailed
            | Self::AssetCacheUnavailable
            | Self::InvalidAssetIndex
            | Self::LimitExceeded
            | Self::Overflow
            | Self::Other => Collision,
            // Read only by the renderer.
            Self::DeletedMaterialSwap
            | Self::MissingMaterialSwap
            | Self::RuntimeMaterialSwapFilter
            | Self::MissingMaterial
            | Self::MissingMaterialEmbeddedShader
            | Self::MissingMaterialParent
            | Self::MissingTexture
            | Self::UnsupportedLightData
            | Self::UnverifiedEffectColorOnly
            | Self::UnverifiedVisibilityEditorMarker
            | Self::UnverifiedNifEditorMarkerVisibility
            | Self::UnverifiedNifEditorMarkerSuffixVisibility
            | Self::UnsupportedSkinning
            | Self::UnsupportedShaderMaterialName
            | Self::UnsupportedMaterialFields => RenderOnly,
        }
    }
}

/// What a blocker raised at `origin` concerns. Everything raised by a
/// material swap, by a material or texture citation of a model, or by a
/// material or texture file is render only, whatever its code.
pub fn use_of(origin: Origin, e: &Error) -> Use {
    match origin {
        Origin::MaterialSwap | Origin::ModelCitation | Origin::RenderAsset => Use::RenderOnly,
        Origin::Record | Origin::Model => Code::of(&e.code).collision_use(),
    }
}

/// Collision readiness with the embedded registry; see [`collision_state_with`].
pub fn collision_state(
    blockers: &[Error],
    origins: &[Origin],
    outside: bool,
    cites_swap: bool,
    b: &Budget,
) -> Result<State> {
    collision_state_with(
        blockers,
        origins,
        outside,
        cites_swap,
        hypotheses::status,
        b,
    )
}

/// Collision readiness of an instance from its blockers and their origins
/// (`origins[i]` for `blockers[i]`; a missing origin counts as
/// [`Origin::Record`]). Needed blockers count as in
/// [`hypotheses::classify_with`]. A render-only blocker raised by a material
/// swap is set aside under [`hypotheses::SWAP_COLLISION`], any other under
/// the better standing of its own covering rule and
/// [`hypotheses::RENDER_INPUTS_COLLISION`] (the latter on a tie); with
/// neither usable the collision is blocked. `cites_swap`: the reference or
/// a base names a material swap, which collision does not apply, so the
/// swap rule is needed even without a blocker.
pub fn collision_state_with(
    blockers: &[Error],
    origins: &[Origin],
    outside: bool,
    cites_swap: bool,
    status: impl Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<State> {
    let rank = |rule: &str| match status(rule) {
        Some(Status::Supported) => 2,
        Some(Status::Pending) => 1,
        Some(Status::Refuted) | None => 0,
    };
    let mut rules: Vec<&'static str> = Vec::new();
    let mut needed = 0usize;
    for (k, e) in blockers.iter().enumerate() {
        let origin = origins.get(k).copied().unwrap_or(Origin::Record);
        let rule = match use_of(origin, e) {
            Use::Collision => {
                needed += 1;
                let Some(h) = covering(e) else {
                    return Ok(State::Blocked);
                };
                match status(h) {
                    Some(Status::Refuted) => return Ok(State::Blocked),
                    Some(_) => h,
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
            }
            Use::RenderOnly if origin == Origin::MaterialSwap => hypotheses::SWAP_COLLISION,
            Use::RenderOnly => {
                let general = hypotheses::RENDER_INPUTS_COLLISION;
                match covering(e) {
                    Some(own) if rank(own) > rank(general) => own,
                    _ => general,
                }
            }
        };
        if !rules.contains(&rule) {
            b.reserve(&mut rules, 1, &e.source.file, e.source.offset)?;
            rules.push(rule);
        }
    }
    if cites_swap && !rules.contains(&hypotheses::SWAP_COLLISION) {
        b.reserve(&mut rules, 1, "collision readiness", 0)?;
        rules.push(hypotheses::SWAP_COLLISION);
    }
    if needed == 0 && outside {
        return Ok(State::Outside);
    }
    if rules.iter().any(|r| rank(r) == 0) {
        return Ok(State::Blocked);
    }
    rules.sort_unstable();
    Ok(if rules.is_empty() {
        State::Ready
    } else if rules.iter().any(|r| rank(r) == 1) {
        State::ReadyUnderHypotheses { hypotheses: rules }
    } else {
        State::ReadyMeasured { rules }
    })
}

#[cfg(test)]
mod tests;
