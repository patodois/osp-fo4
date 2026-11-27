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
//!
//! Behind switches ([`Switches`], off by default) some blockers that
//! collision needs by their code are set aside: a model blocker that lies
//! off the blocks the model's placed bodies read ([`Bearing`],
//! [`hypotheses::COLLISION_OFF_CHAIN`]), a reference's unread alpha
//! field ([`hypotheses::REFERENCE_ALPHA_COLLISION`]) and four unread
//! fields of a static's reference ([`STATIC_REFERENCE_FIELDS`],
//! [`hypotheses::STATIC_REFERENCE_FIELDS_COLLISION`]); the collision cell
//! build asks them only of an instance whose collision is otherwise
//! refused.
use crate::collision::model::Reads;
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
    MaterialSwapEmptyEntry,
    MissingSwapPatternTarget,
    UnsupportedSwapPattern,
    AmbiguousMaterialSwap,
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
    MissingTextureUnreadSlot,
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
            | Self::MaterialSwapEmptyEntry
            | Self::MissingSwapPatternTarget
            | Self::UnsupportedSwapPattern
            | Self::AmbiguousMaterialSwap
            | Self::MissingMaterial
            | Self::MissingMaterialEmbeddedShader
            | Self::MissingMaterialParent
            | Self::MissingTexture
            | Self::MissingTextureUnreadSlot
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

/// Readings of collision readiness, each behind its own switch, off by
/// default (`Loader::set_collision_switches`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Switches {
    /// A model blocker that lies off every block the model's placed bodies
    /// read is set aside under [`hypotheses::COLLISION_OFF_CHAIN`]
    /// ([`bearing`]).
    pub off_chain: bool,
    /// A reference's unread `XALP` (two bytes; xEdit's published name:
    /// alpha) is set aside under [`hypotheses::REFERENCE_ALPHA_COLLISION`]
    /// ([`reference_alpha`]).
    pub reference_alpha: bool,
    /// On a reference whose base is a static (`STAT`), the record blockers
    /// of its `XACT`, `XLRT`, `XOWN` and `ONAM` subrecords
    /// ([`STATIC_REFERENCE_FIELDS`]; xEdit's published names: action flag,
    /// location ref type, owner, open by default) are set aside under
    /// [`hypotheses::STATIC_REFERENCE_FIELDS_COLLISION`]
    /// ([`static_reference_field`]).
    pub static_fields: bool,
    /// On a reference whose base is an activator (`ACTI`), the record
    /// blockers of the base's unread [`ACTIVATOR_FIELDS`] and
    /// [`ACTIVATOR_RECEIVER_FIELDS`] and of the reference's `XLRT` are set
    /// aside under [`hypotheses::ACTIVATOR_FIELDS_COLLISION`]
    /// ([`activator_field`], [`activator_receiver_field`],
    /// [`reference_location_type`]).
    pub activator_fields: bool,
    /// On a reference whose base is a furniture (`FURN`), the record
    /// blockers of the base's unread [`FURNITURE_FIELDS`] and
    /// [`FURNITURE_TEMPLATE_FIELDS`], of the reference's `XLRT` and `XIS2`
    /// and the one its scripts raise (`VMAD`, [`script_condition`]) are set
    /// aside under
    /// [`hypotheses::FURNITURE_FIELDS_COLLISION`] ([`furniture_field`],
    /// [`furniture_template_field`], [`furniture_reference_field`]).
    pub furniture_fields: bool,
    /// On a reference whose base is a furniture (`FURN`), an animation
    /// blocker of its model that lies where a controller or a controller
    /// sequence can move a block its placed bodies read
    /// ([`Bearing::Moved`]) is set aside under
    /// [`hypotheses::FURNITURE_COLLISION_AT_REST`]: the bodies stand at
    /// the stored transforms of their chains, no controller or sequence
    /// run ([`furniture_at_rest`]).
    pub furniture_at_rest: bool,
}

impl Switches {
    pub fn any(self) -> bool {
        self.off_chain
            || self.reference_alpha
            || self.static_fields
            || self.activator_fields
            || self.furniture_fields
            || self.furniture_at_rest
    }
}

/// Whether a model blocker of a furniture reference, lying as `bearing`
/// says against the model's placed bodies, is one
/// [`Switches::furniture_at_rest`] sets aside: an animation blocker
/// (`UnsupportedAnimationRuntime`) in a model where a controller or a
/// controller sequence can move a block a body reads ([`Bearing::Moved`]).
/// A blocker on a block a body reads itself ([`Bearing::OnChain`]), and any
/// other code, are not.
pub fn furniture_at_rest(e: &Error, bearing: Bearing) -> bool {
    bearing == Bearing::Moved && Code::of(&e.code) == Code::UnsupportedAnimationRuntime
}

/// The reference subrecords [`Switches::static_fields`] sets aside on a
/// static's reference.
pub const STATIC_REFERENCE_FIELDS: [&str; 4] = ["XACT", "XLRT", "XOWN", "ONAM"];

/// Whether `e` is the record blocker of one of a reference's
/// [`STATIC_REFERENCE_FIELDS`]: an unread subrecord
/// (`UninterpretedVisualSubrecord`) or a field read as run-time state
/// (`RuntimeRecordState`) whose found value names `REFR.` and the field.
/// Whether the reference's base is a static is the caller's
/// ([`static_base`]).
pub fn static_reference_field(e: &Error) -> bool {
    let mut parts = e.found.split('.');
    matches!(
        e.code.as_str(),
        "UninterpretedVisualSubrecord" | "RuntimeRecordState"
    ) && parts.next() == Some("REFR")
        && parts
            .next()
            .is_some_and(|f| STATIC_REFERENCE_FIELDS.contains(&f))
}

/// Whether `e` is the record blocker of a reference's `XLRT` (xEdit's
/// published name: location ref type), read as run-time state.
pub fn reference_location_type(e: &Error) -> bool {
    e.code == "RuntimeRecordState" && e.found == "REFR.XLRT"
}

/// Whether a base record of signature `signature` is one whose reference's
/// [`STATIC_REFERENCE_FIELDS`] [`Switches::static_fields`] sets aside: a
/// static (`STAT`).
pub fn static_base(signature: [u8; 4]) -> bool {
    signature == *b"STAT"
}

/// Whether an enable parent chain is left open by the rules only because a
/// record below its root carries its own flag 0x800, every such flag
/// agreeing with the chain's start state: the chains
/// `xesp_child_flag_agrees_chain_v1` decides
/// ([`crate::enable::Presence::with_agreeing_child_flags`]). The collision
/// cell build sets such a chain's `XESP.own_flag` blocker aside behind
/// `Loader::set_collision_child_flag_agrees` (off by default).
pub fn child_flag_agrees(p: &crate::enable::Presence) -> bool {
    let open: Vec<&'static str> = crate::enable::findings(&p.chain)
        .into_iter()
        .filter(|f| crate::enable::rule(f).is_none())
        .collect();
    open == [crate::enable::FOUND_OWN_FLAG] && crate::enable::child_flags_agree(&p.chain)
}

/// Whether `e` is a reference's `RuntimeEnableParent` blocker for a record
/// of its chain carrying its own flag 0x800 (`XESP.own_flag`).
pub fn own_flag(e: &Error) -> bool {
    e.code == Code::RuntimeEnableParent.name() && e.found == crate::enable::FOUND_OWN_FLAG
}

/// Whether `e` is the record blocker a reference's scripts (`VMAD`) raise:
/// its state may change at run time.
pub fn script_condition(e: &Error) -> bool {
    e.code == Code::RuntimeCondition.name() && e.found == "VMAD"
}

/// The activator fields [`activator_field`] names.
pub const ACTIVATOR_FIELDS: [&str; 3] = ["PNAM", "FNAM", "ATTX"];

/// Whether `e` is the record blocker of an activator's (`ACTI`) unread
/// [`ACTIVATOR_FIELDS`] (xEdit's published names: marker colour, flags,
/// activate text override).
pub fn activator_field(e: &Error) -> bool {
    let mut parts = e.found.split('.');
    e.code == Code::UninterpretedVisualSubrecord.name()
        && parts.next() == Some("ACTI")
        && parts.next().is_some_and(|f| ACTIVATOR_FIELDS.contains(&f))
}

/// An activator base's (`ACTI`) subrecord this project has not read, set
/// aside for drawing under `acti_receiver_location_render_unchanged_v1`
/// (xEdit's published name: radio receiver).
pub const ACTIVATOR_RECEIVER_FIELDS: [&str; 1] = ["RADR"];

/// Whether `e` is the record blocker of an activator's unread
/// [`ACTIVATOR_RECEIVER_FIELDS`].
pub fn activator_receiver_field(e: &Error) -> bool {
    let mut parts = e.found.split('.');
    e.code == Code::UninterpretedVisualSubrecord.name()
        && parts.next() == Some("ACTI")
        && parts
            .next()
            .is_some_and(|f| ACTIVATOR_RECEIVER_FIELDS.contains(&f))
}

/// A furniture base's (`FURN`) subrecords whose layout this project has
/// not established, set aside for drawing under
/// `furn_fields_render_unchanged_v1` (xEdit's published names: marker
/// colour, flags, active markers, sit sound, workbench data), and `FNPR`,
/// a four-byte subrecord of furniture whose model is a seat or a table
/// (round 29: the vault's benches and tables were refused for it alone).
pub const FURNITURE_FIELDS: [&str; 6] = ["PNAM", "FNAM", "MNAM", "SNAM", "WBDT", "FNPR"];

/// Whether `e` is the record blocker of a furniture's (`FURN`) unread
/// [`FURNITURE_FIELDS`].
pub fn furniture_field(e: &Error) -> bool {
    let mut parts = e.found.split('.');
    e.code == Code::UninterpretedVisualSubrecord.name()
        && parts.next() == Some("FURN")
        && parts.next().is_some_and(|f| FURNITURE_FIELDS.contains(&f))
}

/// A furniture base's (`FURN`) object template and activate text
/// subrecords, set aside for drawing under
/// `furn_template_text_render_unchanged_v1` (xEdit's published names:
/// object template count, object template entry, activate text override).
pub const FURNITURE_TEMPLATE_FIELDS: [&str; 3] = ["OBTE", "OBTS", "ATTX"];

/// Whether `e` is the record blocker of a furniture's (`FURN`)
/// [`FURNITURE_TEMPLATE_FIELDS`]: unread (`OBTS`, `ATTX`) or read as
/// run-time state (`OBTE`).
pub fn furniture_template_field(e: &Error) -> bool {
    let mut parts = e.found.split('.');
    (e.code == Code::UninterpretedVisualSubrecord.name() || e.code == "RuntimeRecordState")
        && parts.next() == Some("FURN")
        && parts
            .next()
            .is_some_and(|f| FURNITURE_TEMPLATE_FIELDS.contains(&f))
}

/// Whether `e` is a furniture reference's run-time field set aside with
/// [`furniture_field`]: its `XLRT` or `XIS2` (xEdit's published names:
/// location ref type, ignored by sandbox), read as run-time state.
pub fn furniture_reference_field(e: &Error) -> bool {
    e.code == "RuntimeRecordState" && matches!(e.found.as_str(), "REFR.XLRT" | "REFR.XIS2")
}

/// Whether `e` is the record blocker of a reference's unread `XALP`.
pub fn reference_alpha(e: &Error) -> bool {
    e.code == Code::UninterpretedVisualSubrecord.name() && e.found.starts_with("REFR.XALP.")
}

/// Where a blocker raised in a model file lies against the blocks that
/// model's placed bodies read ([`Reads`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Bearing {
    /// On a block no placed body reads; for an animation blocker, in a
    /// model where nothing moves a block a body reads.
    OffChain,
    /// On a block a placed body reads (its chain from the root to the
    /// Target, its collision object, its physics system).
    OnChain,
    /// An animation blocker in a model where a controller or a controller
    /// sequence can move a block a body reads.
    Moved,
    /// The model places no body.
    NoBody,
    /// The code is not one raised on a block, or its offset lies in no
    /// block of the file.
    Unlocated,
}

/// Codes the asset graph raises on one block of a model, at an offset
/// inside that block.
pub const BLOCK_CODES: [Code; 5] = [
    Code::UnsupportedNifBlockSemantics,
    Code::UnsupportedAnimationRuntime,
    Code::UnsupportedNifBlock,
    Code::UnsupportedNifBlockTail,
    Code::UnsupportedNifField,
];

/// Where `e`, raised in the model whose placed bodies read `reads`, lies.
pub fn bearing(e: &Error, reads: &Reads) -> Bearing {
    let code = Code::of(&e.code);
    if !BLOCK_CODES.contains(&code) {
        return Bearing::Unlocated;
    }
    if reads.read.is_empty() {
        return Bearing::NoBody;
    }
    let Some(k) = reads.block_at(e.source.offset) else {
        return Bearing::Unlocated;
    };
    if reads.read[k] {
        Bearing::OnChain
    } else if code == Code::UnsupportedAnimationRuntime && reads.moved() {
        Bearing::Moved
    } else {
        Bearing::OffChain
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
    collision_state_set_aside(blockers, origins, outside, cites_swap, |_| None, status, b)
}

/// [`collision_state_with`] where `aside(k)` names the rule under which the
/// switches set aside `blockers[k]`, a blocker collision needs by its code
/// ([`Switches`]): it is set aside under the better standing of its own
/// covering rule and that rule (the latter on a tie), as a render-only
/// blocker is.
pub fn collision_state_set_aside(
    blockers: &[Error],
    origins: &[Origin],
    outside: bool,
    cites_swap: bool,
    aside: impl Fn(usize) -> Option<&'static str>,
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
        let set_aside = match use_of(origin, e) {
            Use::Collision => aside(k),
            Use::RenderOnly => None,
        };
        let rule = match use_of(origin, e) {
            Use::Collision if set_aside.is_some() => {
                let general = set_aside.unwrap_or(hypotheses::COLLISION_OFF_CHAIN);
                match covering(e) {
                    Some(own) if rank(own) > rank(general) => own,
                    _ => general,
                }
            }
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
