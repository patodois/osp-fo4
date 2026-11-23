#![forbid(unsafe_code)]
//! Read-only CELL dependency closure, in Bethesda coordinates. No renderer or runtime.
pub mod actor;
pub mod anim_graph;
mod assets;
pub mod bounds;
pub mod camera;
pub mod char_graph;
pub mod clock;
pub mod collision;
pub mod enable;
pub mod grid;
pub mod hypotheses;
pub mod light;
pub mod lighting;
pub mod missing;
pub mod model_policy;
pub mod paths;
pub mod readiness;
pub mod rest;
pub mod swap_pattern;
pub mod terrain;
pub mod timeline;
pub mod transform;
pub use assets::{
    Asset, AssetKind, Assets, Cited, CitingShader, Cycle, CycleEdge, DecodedAsset, TextureSlot,
};
use fo4_core::{limit, AllocationKind, Budget, Error, Result, SourceSpan};
use fo4_plugin::{typed::*, FormKey, Record};
use fo4_vfs::Vfs;
use paths::Citation;
use serde::Serialize;

/// How much the caller accepts beyond documented format facts.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub enum RuleLevel {
    /// Documented format facts only.
    #[default]
    #[serde(rename = "Strict")]
    Facts,
    /// Facts plus rules the registry marks `supported` by measurement of the running game.
    Measured,
    /// Also rules that are registered but still pending measurement.
    #[serde(rename = "UnderHypotheses")]
    Hypotheses,
}
/// Candidate transformations deliberately exist only in the census module/CLI.
#[derive(Debug, Serialize)]
pub struct Declaration {
    pub signature: [u8; 4],
    pub subrecord: usize,
    pub bytes: std::ops::Range<usize>,
}
#[derive(Debug, Serialize, Default)]
pub struct Visibility {
    pub precombine: Vec<Declaration>,
    pub previs: Vec<Declaration>,
}
#[derive(Debug, Serialize)]
pub struct Part {
    pub declared: ScolPart,
    pub resolved: Option<Base>,
}
#[derive(Debug, Serialize)]
pub struct Base {
    pub editor_marker: bool,
    pub frame_role: fo4_plugin::static_frame::Class,
    pub key: FormKey,
    pub record: Record,
    pub common: Common,
    pub parts: Vec<Part>,
    pub light: Option<LightData>,
    pub material_swap: Option<Loaded<Mswp>>,
    pub texture_set: Option<Loaded<Txst>>,
    pub assets: Vec<usize>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Every input is covered by documented format facts.
    Ready,
    /// Also needs registered rules, all of them supported by measurement.
    ReadyMeasured {
        rules: Vec<&'static str>,
    },
    /// Needs at least one registered rule that is still pending. `hypotheses`
    /// lists every rule relied on, measured ones included.
    ReadyUnderHypotheses {
        hypotheses: Vec<&'static str>,
    },
    Blocked,
    Outside,
}
impl Instance {
    /// Whether the enable parent rules say this instance starts (or, after
    /// [`Instance::set_enabled`], is) disabled.
    pub fn disabled_by_enable_parent(&self) -> bool {
        self.enable
            .as_ref()
            .is_some_and(|p| p.enabled == Some(false))
    }
    /// Replaces the enabled state the chain gives (`xesp_script_change_propagates_v1`).
    pub fn set_enabled(&mut self, enabled: Option<bool>) {
        if let Some(p) = &mut self.enable {
            p.enabled = enabled;
        }
    }
}
impl State {
    pub fn drawable_readiness(&self) -> bool {
        self.ready_at(RuleLevel::Hypotheses)
    }
    /// Whether an instance in this state may be used at the given level.
    pub fn ready_at(&self, level: RuleLevel) -> bool {
        match self {
            Self::Ready => true,
            Self::ReadyMeasured { .. } => level != RuleLevel::Facts,
            Self::ReadyUnderHypotheses { .. } => level == RuleLevel::Hypotheses,
            Self::Blocked | Self::Outside => false,
        }
    }
    /// Every registered rule this state relies on, measured or pending.
    pub fn hypotheses(&self) -> &[&'static str] {
        match self {
            Self::ReadyMeasured { rules } => rules,
            Self::ReadyUnderHypotheses { hypotheses } => hypotheses,
            _ => &[],
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Instance {
    /// For a reference with an XESP: its chain and whether the rules say it
    /// starts enabled ([`enable`]). The draw build and collision leave out
    /// an instance whose `enabled` is `Some(false)`.
    pub enable: Option<enable::Presence>,
    pub editor_marker: bool,
    pub visibility_hypotheses: Vec<&'static str>,
    pub reference: Loaded<Refr>,
    pub base: Option<Base>,
    pub material_swap: Option<Loaded<Mswp>>,
    pub visibility: Visibility,
    pub state: State,
    pub blockers: Vec<Error>,
    /// Where each of `blockers` was raised (same order).
    pub blocker_origins: Vec<readiness::Origin>,
    /// Readiness of the reference's collision: `state` without the blockers
    /// only the renderer reads, which it sets aside under registered rules
    /// ([`readiness`]).
    pub collision: State,
    pub assets: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub struct Exclusion {
    pub code: &'static str,
    pub child: Child,
    pub diagnostics: Vec<Error>,
}
#[derive(Debug, Serialize)]
pub struct Scene {
    pub lighting_data: lighting::LightingData,
    pub cell: Loaded<Cell>,
    pub kind: Option<CellKind>,
    pub name: Option<String>,
    pub lighting_template: Option<Loaded<Lgtm>>,
    pub visibility: Visibility,
    pub instances: Vec<Instance>,
    pub deleted: Vec<Child>,
    pub disabled: Vec<Child>,
    pub exclusions: Vec<Exclusion>,
    pub diagnostics: Vec<Error>,
    pub shared_diagnostic_count: usize,
    pub assets: Vec<usize>,
    pub path_policy: RuleLevel,
}
#[derive(Debug, Serialize)]
pub struct BaseInspection {
    pub base: Base,
    pub assets: Vec<usize>,
    pub blockers: Vec<Error>,
    pub state: State,
}
pub const BASE_TYPES: [[u8; 4]; 17] = [
    *b"STAT", *b"SCOL", *b"MSTT", *b"ACTI", *b"DOOR", *b"FURN", *b"LIGH", *b"MISC", *b"CONT",
    *b"ALCH", *b"BOOK", *b"AMMO", *b"KEYM", *b"NOTE", *b"FLOR", *b"TERM", *b"TACT",
];
pub struct Loader<'a> {
    pub plugins: &'a Store,
    pub assets: Assets<'a>,
    vfs: &'a Vfs,
    pub path_policy: RuleLevel,
    language: &'a str,
}
impl<'a> Loader<'a> {
    pub fn new(
        plugins: &'a Store,
        vfs: &'a Vfs,
        language: &'a str,
        path_policy: RuleLevel,
        cache: &Budget,
    ) -> Self {
        Self {
            plugins,
            assets: Assets::with_policy(vfs, cache, path_policy),
            vfs,
            path_policy,
            language,
        }
    }
    /// Drop decoded asset state while retaining the explicit path interpretation policy.
    pub fn reset_asset_cache(&mut self, cache: &Budget) {
        let clock = self.assets.clock;
        self.assets = Assets::with_policy(self.vfs, cache, self.path_policy);
        self.assets.clock = clock;
    }
    /// Sets the clock's switches the models are read with
    /// (`clock::Switches`; all off by default) and renews the asset cache,
    /// so that no model read before keeps the other reading.
    pub fn set_clock_switches(&mut self, switches: clock::Switches, cache: &Budget) {
        self.assets = Assets::with_policy(self.vfs, cache, self.path_policy);
        self.assets.clock = switches;
    }
    fn resolve_path(
        &self,
        c: Citation,
        raw: &[u8],
        source: &SourceSpan,
        blockers: &mut Vec<Error>,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        let resolved = paths::resolve_with(c, raw, self.vfs, self.path_policy, b)?;
        for e in resolved.diagnostics(&source.file, source.offset, b)? {
            push_error(blockers, e, b)?;
        }
        Ok(resolved.key)
    }
    pub fn by_editor_id(&mut self, id: &str, b: &Budget) -> Result<Scene> {
        let key = self
            .plugins
            .find_cell(id, b)?
            .ok_or_else(|| Error::new("MissingCellEditorId", "plugins", 0, "CELL EditorID", id))?;
        self.cell(&key, b)
    }
    pub fn cell(&mut self, key: &FormKey, b: &Budget) -> Result<Scene> {
        b.measure(AllocationKind::CellContext, || self.cell_impl(key, b))
    }
    fn cell_impl(&mut self, key: &FormKey, b: &Budget) -> Result<Scene> {
        let cell = self.plugins.get::<Cell>(key, b)?.ok_or_else(|| {
            Error::new("MissingCell", &key.plugin, 0, "CELL winner", key.object_id)
        })?;
        let mut diagnostics = Vec::new();
        if cell.record.deleted() {
            return Err(Error::new(
                "DeletedCell",
                &cell.record.source.file,
                cell.record.source.offset,
                "nondeleted CELL",
                key.object_id,
            ));
        }
        unhandled(&cell.record, &cell.data.common, &mut diagnostics, b)?;
        runtime(&cell.record, &mut diagnostics, b)?;
        let name = match &cell.data.common.full_name {
            Some(FullName::Inline(text)) => {
                b.charge(
                    text.len() as u64,
                    &cell.record.source.file,
                    cell.record.source.offset,
                )?;
                Some(text.clone())
            }
            Some(FullName::Localized(id)) => {
                let plugin = &self.plugins.order().headers()[cell.plugin_index].name;
                match fo4_plugin::resolve_string(
                    self.vfs,
                    plugin,
                    self.language,
                    fo4_plugin::StringKind::Strings,
                    *id,
                    b,
                ) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        push_error(&mut diagnostics, e, b)?;
                        None
                    }
                }
            }
            _ => None,
        };
        let lighting_template = match cell
            .data
            .lighting_template
            .as_ref()
            .and_then(|r| r.key.as_ref())
        {
            Some(key) => match self.plugins.get::<Lgtm>(key, b) {
                Ok(Some(v)) => Some(v),
                Ok(None) => {
                    push_error(
                        &mut diagnostics,
                        Error::new(
                            "MissingLightingTemplate",
                            &key.plugin,
                            0,
                            "LGTM winner",
                            key.object_id,
                        ),
                        b,
                    )?;
                    None
                }
                Err(e) => {
                    push_error(&mut diagnostics, e, b)?;
                    None
                }
            },
            None => None,
        };
        if let Some(template) = &lighting_template {
            if template.record.deleted() {
                push_error(
                    &mut diagnostics,
                    error(
                        "DeletedLightingTemplate",
                        &template.record.source,
                        "nondeleted LGTM winner",
                        "deleted",
                    ),
                    b,
                )?;
            }
            unhandled(&template.record, &template.data.common, &mut diagnostics, b)?;
        }
        let children = b.measure(AllocationKind::Children, || self.plugins.children(key, b))?;
        let visibility = visibility(&cell.record, b)?;
        let kind = self.plugins.kind(key);
        let mut scene = Scene {
            lighting_data: lighting::LightingData::default(),
            cell,
            kind,
            name,
            lighting_template,
            visibility,
            instances: Vec::new(),
            deleted: children.deleted,
            disabled: children.disabled,
            exclusions: Vec::new(),
            shared_diagnostic_count: diagnostics.len(),
            diagnostics,
            assets: Vec::new(),
            path_policy: self.path_policy,
        };
        for child in children.excluded {
            b.reserve(
                &mut scene.exclusions,
                1,
                &child.source.file,
                child.source.offset,
            )?;
            scene.exclusions.push(Exclusion {
                code: if child.group_kind == 10 {
                    "UnsupportedDistantReferenceGroup"
                } else {
                    "UnsupportedPlacedRecordType"
                },
                child,
                diagnostics: Vec::new(),
            });
        }
        for child in children.active {
            let attempt = self.instance(&child.key, b);
            match attempt {
                Ok(instance) => {
                    b.reserve(
                        &mut scene.assets,
                        instance.assets.len(),
                        &child.source.file,
                        child.source.offset,
                    )?;
                    scene.assets.extend_from_slice(&instance.assets);
                    b.reserve(
                        &mut scene.instances,
                        1,
                        &child.source.file,
                        child.source.offset,
                    )?;
                    scene.instances.push(instance);
                }
                Err(e) => {
                    let mut diagnostics = Vec::new();
                    push_error(&mut diagnostics, e, b)?;
                    b.reserve(
                        &mut scene.exclusions,
                        1,
                        &child.source.file,
                        child.source.offset,
                    )?;
                    scene.exclusions.push(Exclusion {
                        code: "UnreadableReference",
                        child,
                        diagnostics,
                    });
                }
            }
        }
        scene.assets.sort_unstable();
        scene.assets.dedup();
        scene.lighting_data = lighting::describe(&scene, self.plugins, b)?;
        Ok(scene)
    }
    /// The instance of one placed reference, made as the cell load makes
    /// one for each active child. The viewer's `--script-event` uses it for
    /// an initially disabled reference a script enables.
    pub fn instance(&mut self, key: &FormKey, b: &Budget) -> Result<Instance> {
        let reference = b
            .measure(AllocationKind::Reference, || {
                self.plugins.get::<Refr>(key, b)
            })?
            .ok_or_else(|| {
                Error::new(
                    "MissingReference",
                    &key.plugin,
                    0,
                    "REFR winner",
                    key.object_id,
                )
            })?;
        let mut blockers = Vec::new();
        let src = &reference.record.source;
        if reference.data.transform.is_none() {
            push_error(
                &mut blockers,
                error("MissingPlacementTransform", src, "REFR.DATA", "absent"),
                b,
            )?;
        }
        // Presence through the enable parent chain, every part under its
        // pending rule (`enable`).
        let enable = match &reference.data.enable_parent {
            None => None,
            Some(_) => match fo4_plugin::enable::chain(self.plugins, key, b)? {
                Some(chain) => {
                    enable::blockers(&chain, src, &mut blockers, b)?;
                    Some(enable::Presence::of(chain))
                }
                None => None,
            },
        };
        b.measure(AllocationKind::Diagnostics, || {
            runtime(&reference.record, &mut blockers, b)?;
            unhandled(&reference.record, &reference.data.common, &mut blockers, b)
        })?;
        let mut ancestry = Vec::new();
        let mut swaps = Vec::new();
        let base = match reference.data.base.as_ref().and_then(|r| r.key.as_ref()) {
            Some(key) => match b.measure(AllocationKind::BaseAssets, || {
                self.base(key, b, &mut ancestry, 0, &mut blockers, &mut swaps)
            }) {
                Ok(v) => Some(v),
                Err(e) => {
                    push_error(&mut blockers, e, b)?;
                    None
                }
            },
            None => {
                push_error(
                    &mut blockers,
                    error("MissingBaseForm", src, "non-null NAME", "absent/null"),
                    b,
                )?;
                None
            }
        };
        let at = blockers.len();
        let material_swap = self.swap(reference.data.material_swap.as_ref(), &mut blockers, b)?;
        let mut roots = Vec::new();
        if let Some(base) = &base {
            collect_base_assets(base, &mut roots, b)?;
        }
        if let Some(swap) = &material_swap {
            self.swap_assets(swap, &mut roots, &mut blockers, b)?;
        }
        // Swap sources holding `*` are read as patterns only where the
        // rule may be relied on at the loader's level.
        if hypotheses::usable_at(hypotheses::SWAP_PATTERN, self.path_policy) {
            if let Some(base) = &base {
                self.pattern_assets(material_swap.as_ref(), base, &mut roots, &mut blockers, b)?;
            }
        }
        mark(&mut swaps, at..blockers.len(), src, b)?;
        let mut origins = Vec::new();
        b.reserve(&mut origins, blockers.len(), &src.file, src.offset)?;
        origins.resize(blockers.len(), readiness::Origin::Record);
        for range in &swaps {
            for o in &mut origins[range.clone()] {
                *o = readiness::Origin::MaterialSwap;
            }
        }
        let assets = b.measure(AllocationKind::AssetGraph, || {
            self.assets.closure(&roots, b)
        })?;
        // Missing materials the draw build does not read are set aside;
        // those read only by lighting shaders with their own texture, or
        // only as a material's parent, keep a narrower code that a pending
        // rule covers (`missing`).
        let missing = missing::classify(
            &self.assets,
            &assets,
            &missing::swaps_of(material_swap.as_ref(), base.as_ref()),
            b,
        )?;
        // Missing textures cited only in slots the frame never samples keep
        // a narrower code that a pending rule covers (`missing`).
        let unread = missing::unread_textures(&self.assets, &assets, b)?;
        for i in &assets {
            let node = &self.assets.nodes[*i];
            let read = missing.iter().find(|m| m.node == *i).map(|m| m.read);
            for (k, e) in node.blockers.iter().enumerate() {
                let mut e = e.clone();
                if e.code == "MissingMaterial" {
                    match read {
                        Some(missing::Read::NotRead(_)) => continue,
                        Some(missing::Read::EmbeddedShader) => {
                            e.code = "MissingMaterialEmbeddedShader".into();
                        }
                        Some(missing::Read::Needed(missing::PARENT_ONLY)) => {
                            e.code = "MissingMaterialParent".into();
                        }
                        _ => {}
                    }
                }
                if e.code == "MissingTexture" && unread.contains(i) {
                    e.code = "MissingTextureUnreadSlot".into();
                }
                b.measure(AllocationKind::Diagnostics, || {
                    push_error(&mut blockers, e, b)
                })?;
                b.reserve(&mut origins, 1, &src.file, src.offset)?;
                origins.push(match node.kind {
                    AssetKind::Model if node.citation_blockers.contains(&k) => {
                        readiness::Origin::ModelCitation
                    }
                    AssetKind::Model => readiness::Origin::Model,
                    AssetKind::Material | AssetKind::Texture => readiness::Origin::RenderAsset,
                });
            }
        }
        if base
            .as_ref()
            .is_some_and(|v| v.frame_role == fo4_plugin::static_frame::Class::Used)
            && reference
                .data
                .transform
                .as_ref()
                .is_some_and(|t| t[3..].iter().any(|a| *a != 0.))
        {
            push_error(
                &mut blockers,
                error(
                    "UnverifiedEulerRotation",
                    src,
                    "live world-matrix measurement",
                    "nonzero REFR.DATA Euler angles",
                ),
                b,
            )?;
        }
        if reference.data.scale.is_none()
            && base
                .as_ref()
                .is_some_and(|v| v.frame_role == fo4_plugin::static_frame::Class::Used)
        {
            push_error(
                &mut blockers,
                error(
                    "MissingDocumentedReferenceScale",
                    src,
                    "explicit XSCL or measured absence semantics",
                    "XSCL absent; no default asserted",
                ),
                b,
            )?;
        }
        let outside = base
            .as_ref()
            .is_some_and(|v| v.frame_role == fo4_plugin::static_frame::Class::Outside);
        let state = hypotheses::classify(&blockers, outside, b)?;
        let more = blockers.len().saturating_sub(origins.len());
        b.reserve(&mut origins, more, &src.file, src.offset)?;
        origins.resize(blockers.len(), readiness::Origin::Record);
        let cites_swap = reference
            .data
            .material_swap
            .as_ref()
            .is_some_and(|r| r.key.is_some())
            || base.as_ref().is_some_and(base_cites_swap);
        let collision = readiness::collision_state(&blockers, &origins, outside, cites_swap, b)?;
        let editor_marker = base.as_ref().is_some_and(|v| v.editor_marker);
        let mut visibility_hypotheses = Vec::new();
        if editor_marker {
            b.reserve(&mut visibility_hypotheses, 1, &src.file, src.offset)?;
            visibility_hypotheses.push(hypotheses::MARKER);
        }
        if state.hypotheses().contains(&hypotheses::NAMED_MARKER) {
            b.reserve(&mut visibility_hypotheses, 1, &src.file, src.offset)?;
            visibility_hypotheses.push(hypotheses::NAMED_MARKER);
        }
        let visibility = visibility(&reference.record, b)?;
        Ok(Instance {
            enable,
            editor_marker,
            visibility_hypotheses,
            reference,
            base,
            material_swap,
            visibility,
            state,
            blockers,
            blocker_origins: origins,
            collision,
            assets,
        })
    }
    fn swap(
        &self,
        reference: Option<&FormRef>,
        blockers: &mut Vec<Error>,
        b: &Budget,
    ) -> Result<Option<Loaded<Mswp>>> {
        let Some(key) = reference.and_then(|r| r.key.as_ref()) else {
            return Ok(None);
        };
        match self.plugins.get::<Mswp>(key, b) {
            Ok(Some(v)) => {
                if v.record.deleted() {
                    push_error(
                        blockers,
                        error(
                            "DeletedMaterialSwap",
                            &v.record.source,
                            "nondeleted MSWP",
                            "deleted",
                        ),
                        b,
                    )?;
                }
                Ok(Some(v))
            }
            Ok(None) => {
                push_error(
                    blockers,
                    Error::new(
                        "MissingMaterialSwap",
                        &key.plugin,
                        0,
                        "MSWP winner",
                        key.object_id,
                    ),
                    b,
                )?;
                Ok(None)
            }
            Err(e) => {
                push_error(blockers, e, b)?;
                Ok(None)
            }
        }
    }
    pub fn inspect_base(&mut self, key: &FormKey, b: &Budget) -> Result<BaseInspection> {
        let mut blockers = Vec::new();
        let mut ancestry = Vec::new();
        let base = self.base(key, b, &mut ancestry, 0, &mut blockers, &mut Vec::new())?;
        let mut roots = Vec::new();
        collect_base_assets(&base, &mut roots, b)?;
        let assets = self.assets.closure(&roots, b)?;
        for id in &assets {
            for e in &self.assets.nodes[*id].blockers {
                push_error(&mut blockers, e.clone(), b)?;
            }
        }
        let state = hypotheses::classify(
            &blockers,
            base.frame_role == fo4_plugin::static_frame::Class::Outside,
            b,
        )?;
        Ok(BaseInspection {
            base,
            assets,
            blockers,
            state,
        })
    }
    fn base(
        &mut self,
        key: &FormKey,
        b: &Budget,
        ancestry: &mut Vec<FormKey>,
        depth: u64,
        blockers: &mut Vec<Error>,
        swaps: &mut Vec<std::ops::Range<usize>>,
    ) -> Result<Base> {
        limit(depth, self.plugins.limits.max_depth, &key.plugin, 0)?;
        if ancestry.contains(key) {
            return Err(Error::new(
                "ScolCycle",
                &key.plugin,
                0,
                "acyclic SCOL parts",
                key.object_id,
            ));
        }
        b.reserve(ancestry, 1, &key.plugin, 0)?;
        b.charge(key.plugin.len() as u64, &key.plugin, 0)?;
        ancestry.push(key.clone());
        let w = self.plugins.overrides().winner(key).ok_or_else(|| {
            Error::new(
                "MissingBaseForm",
                &key.plugin,
                0,
                "base winner",
                key.object_id,
            )
        })?;
        let signature = w.signature;
        let (record, common, declared, light, texture_set) = match &signature {
            b"SCOL" => {
                let v = required(self.plugins.get::<Scol>(key, b)?, key)?;
                (v.record, v.data.common, v.data.parts, None, None)
            }
            b"LIGH" => {
                let v = required(self.plugins.get::<Ligh>(key, b)?, key)?;
                (v.record, v.data.common, Vec::new(), v.data.light, None)
            }
            s if MODEL_TYPES.contains(s) => {
                let v = required(self.plugins.get_common(key, signature, b)?, key)?;
                (v.record, v.data, Vec::new(), None, None)
            }
            _ => {
                return Err(Error::new(
                    "BaseTypeOutsideSlice",
                    &self.plugins.order().headers()[w.plugin].source.file,
                    w.offset,
                    "STAT/SCOL/MSTT/ACTI/DOOR/FURN/LIGH",
                    String::from_utf8_lossy(&signature),
                ))
            }
        };
        if record.deleted() {
            push_error(
                blockers,
                error(
                    "DeletedBaseForm",
                    &record.source,
                    "nondeleted base",
                    "deleted",
                ),
                b,
            )?;
        }
        if matches!(&signature, b"IDLM" | b"SOUN" | b"ASPC") {
            return Ok(Base {
                editor_marker: false,
                frame_role: fo4_plugin::static_frame::Class::Outside,
                key: key.clone(),
                record,
                common,
                parts: Vec::new(),
                light: None,
                material_swap: None,
                texture_set: None,
                assets: Vec::new(),
            });
        }
        runtime(&record, blockers, b)?;
        unhandled(&record, &common, blockers, b)?;
        let at = blockers.len();
        let material_swap = self.swap(common.model.material_swap.as_ref(), blockers, b)?;
        mark(swaps, at..blockers.len(), &record.source, b)?;
        let mut assets = Vec::new();
        if let Some(path) = &common.model.path {
            b.reserve(&mut assets, 1, &record.source.file, record.source.offset)?;
            let temporary = b.child();
            match self.resolve_path(
                Citation::RecordModel,
                path.as_bytes(),
                &record.source,
                blockers,
                &temporary,
            ) {
                Ok(key) => assets.push(self.assets.load(AssetKind::Model, &key, &temporary)?),
                Err(mut e) => {
                    e.source = Box::new(record.source.clone());
                    push_error(blockers, e, b)?;
                }
            }
        } else if signature != *b"SCOL" && signature != *b"LIGH" {
            push_error(
                blockers,
                error("MissingModelPath", &record.source, "MODL", "absent"),
                b,
            )?;
        }
        if signature == *b"LIGH" && light.is_none() {
            push_error(
                blockers,
                error(
                    "UnsupportedLightData",
                    &record.source,
                    "supported LIGH.DATA",
                    "raw/absent",
                ),
                b,
            )?;
        }
        if let Some(swap) = &material_swap {
            let at = blockers.len();
            self.swap_assets(swap, &mut assets, blockers, b)?;
            mark(swaps, at..blockers.len(), &record.source, b)?;
        }
        if signature == *b"SCOL" && !declared.is_empty() {
            push_error(
                blockers,
                error(
                    "UnverifiedScolGeometrySource",
                    &record.source,
                    "measured generated MODL versus constituent equivalence",
                    "ONAM/DATA parts; generated MODL retained raw",
                ),
                b,
            )?;
        }
        let mut parts = Vec::new();
        for part in declared {
            if part
                .placements
                .as_ref()
                .is_some_and(|v| v.iter().any(|p| p[3..6].iter().any(|a| *a != 0.)))
            {
                push_error(
                    blockers,
                    error(
                        "UnverifiedEulerRotation",
                        &record.source,
                        "live world-matrix measurement",
                        "nonzero SCOL.DATA Euler angles",
                    ),
                    b,
                )?;
            }
            let resolved = match part.base.as_ref().and_then(|r| r.key.as_ref()) {
                Some(key) => {
                    let len = ancestry.len();
                    let result = self.base(key, b, ancestry, depth + 1, blockers, swaps);
                    ancestry.truncate(len);
                    match result {
                        Ok(v) => Some(v),
                        Err(e) => {
                            push_error(blockers, e, b)?;
                            None
                        }
                    }
                }
                None => {
                    push_error(
                        blockers,
                        error("MissingScolPartBase", &record.source, "ONAM", "absent/null"),
                        b,
                    )?;
                    None
                }
            };
            if part.placements.is_none() {
                push_error(
                    blockers,
                    error(
                        "MissingScolPlacements",
                        &record.source,
                        "SCOL.DATA",
                        "absent",
                    ),
                    b,
                )?;
            }
            b.reserve(&mut parts, 1, &record.source.file, record.source.offset)?;
            parts.push(Part {
                declared: part,
                resolved,
            });
        }
        ancestry.pop();
        b.charge(key.plugin.len() as u64, &key.plugin, record.source.offset)?;
        Ok(Base {
            editor_marker: matches!(&record.signature, b"STAT" | b"ACTI" | b"DOOR")
                && record.flags & 0x0080_0000 != 0,
            frame_role: fo4_plugin::static_frame::Class::Used,
            key: key.clone(),
            record,
            common,
            parts,
            light,
            material_swap,
            texture_set,
            assets,
        })
    }
    fn swap_assets(
        &mut self,
        swap: &Loaded<Mswp>,
        assets: &mut Vec<usize>,
        blockers: &mut Vec<Error>,
        b: &Budget,
    ) -> Result<()> {
        for (k, e) in swap.data.entries.iter().enumerate() {
            if matches!(&e.signature, b"BNAM" | b"SNAM") {
                if let SwapValue::Path(path) = &e.value {
                    // The source of a pair whose target is empty is not read:
                    // the pair changes nothing under the rule below.
                    let next = swap.data.entries[k + 1..]
                        .iter()
                        .find(|n| matches!(&n.signature, b"BNAM" | b"SNAM"));
                    if e.signature == *b"BNAM"
                        && next.is_some_and(|n| {
                            n.signature == *b"SNAM"
                                && matches!(&n.value, SwapValue::Path(p) if p.is_empty())
                        })
                    {
                        continue;
                    }
                    // An empty source or target names no file: under
                    // `mswp_empty_entry_inert_v1` the entry changes nothing.
                    if path.is_empty() {
                        push_error(
                            blockers,
                            error(
                                "MaterialSwapEmptyEntry",
                                &swap.record.source,
                                "nonempty swap path",
                                &String::from_utf8_lossy(&e.signature),
                            ),
                            b,
                        )?;
                        continue;
                    }
                    b.reserve(
                        assets,
                        1,
                        &swap.record.source.file,
                        swap.record.source.offset,
                    )?;
                    match self.resolve_path(
                        Citation::SwapMaterial,
                        path.as_bytes(),
                        &swap.record.source,
                        blockers,
                        b,
                    ) {
                        Ok(key) => assets.push(self.assets.load(AssetKind::Material, &key, b)?),
                        Err(mut e) => {
                            e.source = Box::new(swap.record.source.clone());
                            push_error(blockers, e, b)?;
                        }
                    }
                }
            }
        }
        if swap.data.entries.iter().any(|e| e.signature == *b"FNAM") {
            push_error(
                blockers,
                error(
                    "RuntimeMaterialSwapFilter",
                    &swap.record.source,
                    "unconditional material swap",
                    "FNAM",
                ),
                b,
            )?;
        }
        Ok(())
    }
    /// The targets that swap patterns give the materials the models under
    /// each swap cite (`swap_pattern`, `mswp_wildcard_pattern_v1`): each
    /// one an archive holds joins `roots`; each one none holds raises
    /// `MissingSwapPatternTarget` (`mswp_wildcard_missing_target_inert_v1`)
    /// and is kept out of the instance's assets. A material a literal source
    /// of the same swap maps is left to that entry
    /// (`mswp_wildcard_literal_wins_v1`).
    fn pattern_assets(
        &mut self,
        reference: Option<&Loaded<Mswp>>,
        base: &Base,
        roots: &mut Vec<usize>,
        blockers: &mut Vec<Error>,
        b: &Budget,
    ) -> Result<()> {
        fn has_pattern(s: &Loaded<Mswp>) -> bool {
            s.data.entries.iter().any(|e| {
                e.signature == *b"BNAM" && matches!(&e.value, SwapValue::Path(p) if p.contains('*'))
            })
        }
        fn scopes<'a>(
            v: &'a Base,
            out: &mut Vec<(&'a Loaded<Mswp>, &'a Base)>,
            b: &Budget,
        ) -> Result<()> {
            if let Some(s) = v.material_swap.as_ref().filter(|s| has_pattern(s)) {
                b.reserve(out, 1, &v.record.source.file, v.record.source.offset)?;
                out.push((s, v));
            }
            for p in &v.parts {
                if let Some(r) = &p.resolved {
                    scopes(r, out, b)?;
                }
            }
            Ok(())
        }
        let mut list = Vec::new();
        if let Some(r) = reference.filter(|r| has_pattern(r)) {
            b.reserve(&mut list, 1, &r.record.source.file, r.record.source.offset)?;
            list.push((r, base));
        }
        scopes(base, &mut list, b)?;
        // Every material another scope's swap can put in place: literal
        // targets of the instance's swaps, and what patterns form from them.
        let mut placed: Vec<Vec<u8>> = Vec::new();
        for swap in missing::swaps_of(reference, Some(base)) {
            for (_, t) in missing::swap_pairs(&self.assets, swap, b)? {
                if !t.is_empty() && !swap_pattern::is_pattern(&t) && !placed.contains(&t) {
                    b.reserve(&mut placed, 1, &swap.record.source.file, 0)?;
                    placed.push(t);
                }
            }
        }
        for (swap, under) in &list {
            let work = b.child();
            let src = &swap.record.source;
            let mut models = Vec::new();
            collect_base_assets(under, &mut models, &work)?;
            let closure = self.assets.closure(&models, &work)?;
            let mut materials: Vec<Vec<u8>> = Vec::new();
            for i in &closure {
                let node = &self.assets.nodes[*i];
                if node.kind != AssetKind::Model {
                    continue;
                }
                for (d, c) in node.dependencies.iter().zip(&node.dependency_citations) {
                    let path = &self.assets.nodes[*d].path;
                    if c.class == Citation::NifMaterial && !materials.contains(path) {
                        work.reserve(&mut materials, 1, &src.file, src.offset)?;
                        materials.push(path.clone());
                    }
                }
            }
            let formed = self.pattern_targets(swap, &materials, true, roots, blockers, b)?;
            for t in formed {
                if !placed.contains(&t) {
                    b.reserve(&mut placed, 1, &src.file, src.offset)?;
                    placed.push(t);
                }
            }
        }
        // The same for materials other scopes put in place, as the draw
        // build's readings of two or more scopes can reach them.
        for _ in 0..4 {
            let mut more = Vec::new();
            for (swap, _) in &list {
                for t in self.pattern_targets(swap, &placed, true, roots, blockers, b)? {
                    if !placed.contains(&t) && !more.contains(&t) {
                        b.reserve(&mut more, 1, &swap.record.source.file, 0)?;
                        more.push(t);
                    }
                }
            }
            if more.is_empty() {
                break;
            }
            b.reserve(&mut placed, more.len(), "material swap", 0)?;
            placed.extend(more);
        }
        Ok(())
    }
    /// The targets the patterns of `swap` form for `materials` (a material
    /// a literal source of the swap maps is left to that entry): each one
    /// an archive holds joins `roots`; for one none holds, with `blocker`,
    /// `MissingSwapPatternTarget` is raised. Returns every target formed.
    fn pattern_targets(
        &mut self,
        swap: &Loaded<Mswp>,
        materials: &[Vec<u8>],
        blocker: bool,
        roots: &mut Vec<usize>,
        blockers: &mut Vec<Error>,
        b: &Budget,
    ) -> Result<Vec<Vec<u8>>> {
        let work = b.child();
        let src = &swap.record.source;
        let pairs = missing::swap_pairs(&self.assets, swap, &work)?;
        // Literal sources, an empty target's included.
        let mut literal: Vec<Vec<u8>> = Vec::new();
        for e in &swap.data.entries {
            if let (b"BNAM", SwapValue::Path(p)) = (&e.signature, &e.value) {
                if let Ok(r) = self
                    .assets
                    .resolve_path(Citation::SwapMaterial, p.as_bytes(), &work)
                {
                    if !swap_pattern::is_pattern(&r.key) {
                        work.reserve(&mut literal, 1, &src.file, src.offset)?;
                        literal.push(r.key);
                    }
                }
            }
        }
        let mut formed = Vec::new();
        for m in materials {
            if literal.contains(m) {
                continue;
            }
            for (f, t) in &pairs {
                if !swap_pattern::is_pattern(f) {
                    continue;
                }
                let target = match swap_pattern::apply(f, t, m, &work) {
                    Ok(Some(t)) => t,
                    Ok(None) => continue,
                    Err(e) if matches!(e.code.as_str(), "AllocationFailed" | "LimitExceeded") => {
                        return Err(e)
                    }
                    Err(mut e) => {
                        if blocker {
                            e.source = Box::new(src.clone());
                            push_error(blockers, e, b)?;
                        }
                        continue;
                    }
                };
                let id = self.assets.load(AssetKind::Material, &target, &work)?;
                let absent = self.assets.nodes[id]
                    .blockers
                    .iter()
                    .any(|e| e.code == "MissingMaterial");
                if absent {
                    if blocker {
                        push_error(
                            blockers,
                            error(
                                "MissingSwapPatternTarget",
                                src,
                                "a target file for the pattern",
                                &fo4_core::HexBytes(&target).to_string(),
                            ),
                            b,
                        )?;
                    }
                } else if !roots.contains(&id) {
                    b.reserve(roots, 1, &src.file, src.offset)?;
                    roots.push(id);
                }
                if !formed.contains(&target) {
                    b.reserve(&mut formed, 1, &src.file, src.offset)?;
                    formed.push(target);
                }
            }
        }
        Ok(formed)
    }
    /// Explicit TXST resolution for consumers holding a documented TXST reference.
    /// Unknown model companions are never guessed to be FormIDs.
    pub fn texture_set(&mut self, key: &FormKey, b: &Budget) -> Result<(Loaded<Txst>, Vec<usize>)> {
        let value = required(self.plugins.get::<Txst>(key, b)?, key)?;
        let mut assets = Vec::new();
        for path in value.data.textures.iter().flatten() {
            b.reserve(
                &mut assets,
                1,
                &value.record.source.file,
                value.record.source.offset,
            )?;
            let key = paths::resolve(Citation::TextureSetTexture, path.as_bytes(), b).map_err(
                |mut e| {
                    *e.source = value.record.source.clone();
                    e
                },
            )?;
            assets.push(self.assets.load(AssetKind::Texture, &key, b)?);
        }
        if let Some(path) = &value.data.material {
            b.reserve(
                &mut assets,
                1,
                &value.record.source.file,
                value.record.source.offset,
            )?;
            let key = paths::resolve(Citation::TextureSetMaterial, path.as_bytes(), b).map_err(
                |mut e| {
                    *e.source = value.record.source.clone();
                    e
                },
            )?;
            assets.push(self.assets.load(AssetKind::Material, &key, b)?);
        }
        Ok((value, assets))
    }
}
fn required<T>(v: Option<T>, key: &FormKey) -> Result<T> {
    v.ok_or_else(|| {
        Error::new(
            "MissingRecord",
            &key.plugin,
            0,
            "winning record",
            key.object_id,
        )
    })
}
fn error(code: &str, src: &SourceSpan, expected: &str, found: &str) -> Error {
    let mut e = Error::new(code, &src.file, src.offset, expected, found);
    e.source = Box::new(src.clone());
    e
}
fn push_error(v: &mut Vec<Error>, e: Error, b: &Budget) -> Result<()> {
    b.charge(
        (e.code.len() + e.expected.len() + e.found.len() + e.source.file.len() + 128) as u64,
        &e.source.file,
        e.source.offset,
    )?;
    b.reserve(v, 1, &e.source.file, e.source.offset)?;
    v.push(e);
    Ok(())
}
fn runtime(record: &Record, blockers: &mut Vec<Error>, b: &Budget) -> Result<()> {
    for s in &record.subrecords {
        if matches!(&s.signature, b"VMAD" | b"CTDA") {
            push_error(
                blockers,
                error(
                    "RuntimeCondition",
                    &record.source,
                    "runtime-independent instance",
                    &String::from_utf8_lossy(&s.signature),
                ),
                b,
            )?;
        }
    }
    Ok(())
}
fn unhandled(
    record: &Record,
    common: &Common,
    blockers: &mut Vec<Error>,
    b: &Budget,
) -> Result<()> {
    for raw in &common.uninterpreted {
        if matches!(
            &raw.signature,
            b"PCMB" | b"XCRI" | b"VISI" | b"RVIS" | b"XPRI" | b"VMAD" | b"CTDA"
        ) {
            continue;
        }
        use fo4_plugin::static_frame::{classify_size, Class};
        let class = classify_size(
            &record.signature,
            &raw.signature,
            record.data(&record.subrecords[raw.subrecord]).len(),
        );
        if class == Class::Outside {
            continue;
        }
        let s = &record.subrecords[raw.subrecord];
        let mut e = error(
            match class {
                Class::State => "RuntimeRecordState",
                Class::Used => "UnsupportedUsedFieldLayout",
                _ => "UninterpretedVisualSubrecord",
            },
            &record.source,
            "interpreted required visual data",
            &format!(
                "{}.{}{}{}",
                String::from_utf8_lossy(&record.signature),
                String::from_utf8_lossy(&raw.signature),
                if raw.open_field.is_some() { "." } else { "" },
                raw.open_field.unwrap_or("")
            ),
        );
        if record.payload_is_decompressed {
            e.source.decompressed_offset = Some(s.offset);
        } else {
            e.source.offset = s.offset;
        }
        push_error(blockers, e, b)?;
    }
    Ok(())
}
fn visibility(r: &Record, b: &Budget) -> Result<Visibility> {
    let mut result = Visibility::default();
    for (i, s) in r.subrecords.iter().enumerate() {
        let target = match &s.signature {
            b"XCRI" | b"PCMB" => &mut result.precombine,
            b"RVIS" | b"VISI" | b"XPRI" => &mut result.previs,
            _ => continue,
        };
        b.reserve(target, 1, &r.source.file, s.offset)?;
        target.push(Declaration {
            signature: s.signature,
            subrecord: i,
            bytes: s.data.clone(),
        });
    }
    Ok(result)
}
/// Records `range` of blockers as raised by a material swap.
fn mark(
    swaps: &mut Vec<std::ops::Range<usize>>,
    range: std::ops::Range<usize>,
    src: &SourceSpan,
    b: &Budget,
) -> Result<()> {
    if !range.is_empty() {
        b.reserve(swaps, 1, &src.file, src.offset)?;
        swaps.push(range);
    }
    Ok(())
}
/// Whether the base or any resolved SCOL part base names a material swap.
fn base_cites_swap(base: &Base) -> bool {
    base.common
        .model
        .material_swap
        .as_ref()
        .is_some_and(|r| r.key.is_some())
        || base
            .parts
            .iter()
            .filter_map(|p| p.resolved.as_ref())
            .any(base_cites_swap)
}
fn collect_base_assets(base: &Base, out: &mut Vec<usize>, b: &Budget) -> Result<()> {
    b.reserve(
        out,
        base.assets.len(),
        &base.record.source.file,
        base.record.source.offset,
    )?;
    out.extend_from_slice(&base.assets);
    for part in &base.parts {
        if let Some(base) = &part.resolved {
            collect_base_assets(base, out, b)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
