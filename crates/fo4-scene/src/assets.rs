use crate::model_policy::{self, Animation, ModelPolicy};
use crate::paths::Citation;
use fo4_core::{add, limit, path_key_bytes, Budget, Error, Result, SourceSpan};
use fo4_plugin::static_frame::Class;
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::HashMap;
#[cfg(test)]
#[path = "../../fo4-material/src/test_builder.rs"]
mod material_fixture;
struct Dependency {
    path: Vec<u8>,
    class: Citation,
    offset: u64,
    shader: Option<CitingShader>,
}
/// How a model cites one of its dependencies: the citation class and, for a
/// shader's material name, what the citing shader holds of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cited {
    pub class: Citation,
    pub shader: Option<CitingShader>,
}
/// The shader block whose `Name` cites a material: its type and whether it
/// carries a texture of its own (a lighting shader's texture set with a
/// non-empty first texture, an effect shader's non-empty source texture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CitingShader {
    pub effect: bool,
    pub texture: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AssetKind {
    Model,
    Material,
    Texture,
}
#[derive(Debug, Serialize)]
pub struct Asset {
    pub pending_live_proof: Vec<&'static str>,
    pub alpha: Option<MaterialAlpha>,
    pub kind: AssetKind,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub path: Vec<u8>,
    pub origin: Option<SourceSpan>,
    pub dependencies: Vec<usize>,
    /// How each of `dependencies` is cited (same order).
    #[serde(skip)]
    pub dependency_citations: Vec<Cited>,
    pub blockers: Vec<Error>,
    /// Indices into `blockers` raised while following this file's material
    /// and texture citations (`crate::readiness::Origin::ModelCitation`).
    #[serde(skip)]
    pub citation_blockers: Vec<usize>,
    pub uninterpreted: u64,
    pub outside_blocks: std::collections::BTreeMap<String, u64>,
    pub shapes: u64,
    pub cycles: Vec<Cycle>,
    pub animations: Vec<Animation>,
    pub suppressed_texture_sets: u64,
    pub rejections: Vec<Rejection>,
    pub path_resolutions: Vec<crate::paths::ResolvedPath>,
    pub texture_audit: Vec<model_policy::TextureCitation>,
    pub ready: bool,
    #[serde(skip)]
    loading: bool,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MaterialAlpha {
    pub enabled: u8,
    pub source: u32,
    pub destination: u32,
    pub alpha_test_enabled: u8,
    pub alpha_test_ref: u8,
    pub previous_mode: Option<&'static str>,
    pub previously_blocked: bool,
}
#[derive(Debug, Serialize)]
pub struct Rejection {
    pub class: Citation,
    pub offset: u64,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct Cycle {
    pub edges: Vec<CycleEdge>,
}
#[derive(Debug, Serialize)]
pub struct CycleEdge {
    pub from: usize,
    pub to: usize,
    pub class: Citation,
    pub offset: u64,
}
#[derive(Clone, Copy)]
struct Active {
    node: usize,
    incoming: Option<(Citation, u64)>,
}
// A fixed, bounded stack value avoids an otherwise unnecessary heap allocation.
#[allow(clippy::large_enum_variant)]
pub enum DecodedAsset<'a> {
    Model(fo4_nif::Nif<'a>),
    Material(fo4_material::Material<'a>),
    Texture(fo4_texture::Texture<'a>),
}
pub struct Assets<'a> {
    pub nodes: Vec<Asset>,
    pub path_policy: crate::RuleLevel,
    index: HashMap<Vec<u8>, usize>,
    vfs: &'a Vfs,
    allocation: Budget,
    unavailable: bool,
    active: Vec<Active>,
}
impl<'a> Assets<'a> {
    pub fn new(vfs: &'a Vfs, b: &Budget) -> Self {
        Self::with_policy(vfs, b, crate::RuleLevel::Facts)
    }
    pub fn with_policy(vfs: &'a Vfs, b: &Budget, path_policy: crate::RuleLevel) -> Self {
        Self {
            path_policy,
            nodes: Vec::new(),
            index: HashMap::new(),
            vfs,
            // The cache grows with every asset of the scene: an account of
            // `b`, bounded where `b` is, not a scope of one read.
            allocation: b.account(),
            unavailable: false,
            active: Vec::new(),
        }
    }
    pub fn resolve_path(
        &self,
        c: Citation,
        raw: &[u8],
        b: &Budget,
    ) -> Result<crate::paths::ResolvedPath> {
        crate::paths::resolve_with(c, raw, self.vfs, self.path_policy, b)
    }
    pub fn memory_used(&self) -> u64 {
        self.allocation.used()
    }
    pub fn unavailable(&self) -> bool {
        self.unavailable
    }
    pub fn load(&mut self, kind: AssetKind, path: &[u8], work: &Budget) -> Result<usize> {
        if self.unavailable {
            return Err(Error::new(
                "AssetCacheUnavailable",
                "asset graph",
                0,
                "new asset cache after interrupted inspection",
                "previous resource failure left unpublished entries",
            ));
        }
        let inserted = self.nodes.len();
        let result = self.visit(kind, path, work, 0, None);
        // Publication can fail after inserting an identity. Such an entry is not
        // an active recursive dependency once this top-level inspection ends.
        for node in &mut self.nodes[inserted..] {
            if node.loading {
                self.unavailable = true;
                node.loading = false;
            }
        }
        self.active.clear();
        result
    }
    fn visit(
        &mut self,
        kind: AssetKind,
        path: &[u8],
        parent: &Budget,
        depth: u64,
        incoming: Option<(Citation, u64)>,
    ) -> Result<usize> {
        let l = self.vfs.limits();
        limit(depth, l.max_depth, "asset graph", 0)?;
        let mut cache_key = Vec::new();
        let key_size = path.len().checked_add(1).ok_or_else(|| {
            Error::new(
                "Overflow",
                "asset graph",
                0,
                "path length + kind",
                path.len(),
            )
        })?;
        let lookup = parent.child();
        lookup.reserve(&mut cache_key, key_size, "asset graph", 0)?;
        cache_key.push(kind as u8);
        let normalized = path_key_bytes(path);
        cache_key.extend_from_slice(normalized.as_deref().unwrap_or(path));
        if let Some(i) = self.index.get(&cache_key).copied() {
            if self.nodes[i].loading {
                let start = self
                    .active
                    .iter()
                    .position(|f| f.node == i)
                    .ok_or_else(|| {
                        Error::new(
                            "AssetCacheUnavailable",
                            "asset graph",
                            0,
                            "loading node on active stack",
                            i,
                        )
                    })?;
                let owner = self.active.last().map(|f| f.node).ok_or_else(|| {
                    Error::new("AssetCacheUnavailable", "asset graph", 0, "active owner", i)
                })?;
                let mut edges = Vec::new();
                self.allocation
                    .reserve(&mut edges, self.active.len() - start, "cycle", 0)?;
                for j in start + 1..self.active.len() {
                    let (class, offset) = self.active[j].incoming.ok_or_else(|| {
                        Error::new(
                            "AssetCacheUnavailable",
                            "cycle",
                            0,
                            "citation on recursive edge",
                            j,
                        )
                    })?;
                    edges.push(CycleEdge {
                        from: self.active[j - 1].node,
                        to: self.active[j].node,
                        class,
                        offset,
                    });
                }
                let (class, offset) = incoming.ok_or_else(|| {
                    Error::new("AssetCacheUnavailable", "cycle", 0, "closing citation", i)
                })?;
                edges.push(CycleEdge {
                    from: owner,
                    to: i,
                    class,
                    offset,
                });
                let classes = edges
                    .iter()
                    .map(|e| e.class.rule().0)
                    .collect::<Vec<_>>()
                    .join(",");
                let label = format!("length={};edges={classes}", edges.len());
                self.allocation
                    .reserve(&mut self.nodes[owner].cycles, 1, "cycle", offset)?;
                self.nodes[owner].cycles.push(Cycle { edges });
                // Registered closure policy, not an observed engine behavior.
                // Only the material-parent self-edge adds no new file to this closure.
                if owner == i
                    && self.active.len() - start == 1
                    && class == Citation::MaterialParent
                    && kind == AssetKind::Material
                {
                    if !self.nodes[owner]
                        .pending_live_proof
                        .contains(&"bgsm_self_wetness_template_stop_v1")
                    {
                        self.allocation.reserve(
                            &mut self.nodes[owner].pending_live_proof,
                            1,
                            "cycle rule",
                            offset,
                        )?;
                        self.nodes[owner]
                            .pending_live_proof
                            .push("bgsm_self_wetness_template_stop_v1");
                    }
                    return Ok(i);
                }
                return Err(Error::new(
                    "AssetDependencyCycle",
                    "asset graph",
                    offset,
                    "acyclic dependency",
                    label,
                ));
            }
            return Ok(i);
        }
        limit(self.nodes.len() as u64 + 1, l.max_entries, "asset graph", 0)?;
        self.allocation.charge(
            (cache_key.len() + path.len() + 128) as u64,
            "asset graph",
            0,
        )?;
        self.index
            .try_reserve(1)
            .map_err(|e| Error::new("AllocationFailed", "asset graph", 0, "asset index", e))?;
        self.allocation
            .reserve(&mut self.nodes, 1, "asset graph", 0)?;
        let i = self.nodes.len();
        let mut owned = Vec::new();
        self.allocation
            .reserve(&mut owned, path.len(), "asset graph", 0)?;
        owned.extend_from_slice(path);
        self.nodes.push(Asset {
            pending_live_proof: Vec::new(),
            alpha: None,
            kind,
            path: owned,
            origin: None,
            dependencies: Vec::new(),
            dependency_citations: Vec::new(),
            blockers: Vec::new(),
            citation_blockers: Vec::new(),
            uninterpreted: 0,
            outside_blocks: std::collections::BTreeMap::new(),
            shapes: 0,
            cycles: Vec::new(),
            animations: Vec::new(),
            suppressed_texture_sets: 0,
            rejections: Vec::new(),
            path_resolutions: Vec::new(),
            texture_audit: Vec::new(),
            ready: false,
            loading: true,
        });
        // Hash capacity was reserved above; publish only after all owned node
        // allocations succeeded, so a failed reserve cannot leave an invalid ID.
        self.index.insert(cache_key, i);
        self.allocation
            .reserve(&mut self.active, 1, "asset stack", 0)?;
        self.active.push(Active { node: i, incoming });
        let work = parent.child();
        let attempt = (|| {
            normalized.map_err(|e| {
                Error::new(
                    "RejectedVfsPath",
                    &e.source.file,
                    e.source.offset,
                    &e.expected,
                    &e.found,
                )
            })?;
            let c = self.vfs.winner_bytes(path).map_err(|e| {
                if e.code == "MissingPath" {
                    Error::new(
                        match kind {
                            AssetKind::Model => "MissingModel",
                            AssetKind::Material => "MissingMaterial",
                            AssetKind::Texture => "MissingTexture",
                        },
                        &e.source.file,
                        e.source.offset,
                        "winning VFS entry",
                        fo4_core::HexBytes(path),
                    )
                } else {
                    e
                }
            })?;
            let origin = self.vfs.provenance(c)?;
            self.allocation.charge(
                (origin.file.len() + origin.archive_entry.as_ref().map_or(0, String::len)) as u64,
                &origin.file,
                origin.offset,
            )?;
            self.nodes[i].origin = Some(origin);
            let bytes = self.vfs.read_candidate(c, &work)?;
            let mut deps: Vec<Dependency> = Vec::new();
            match kind {
                AssetKind::Model => {
                    let n = fo4_nif::Nif::read(
                        &bytes,
                        &c.original,
                        fo4_core::Game::Fallout4,
                        l,
                        &work,
                    )?;
                    let policy = ModelPolicy::inspect_with(&n, self.vfs, self.path_policy, &work)?;
                    // What each animation block is at rest, by what uses it
                    // (`crate::rest`); each tag is its own blocker.
                    let rest = crate::rest::tags(&n, &work)?;
                    self.nodes[i].texture_audit = policy.texture_citations(&n, &self.allocation)?;
                    for (index, block) in n.blocks.iter().enumerate() {
                        if policy.visual[index] && n.block_types[block.type_index as usize] == "BSEffectShaderProperty"
                            && policy.material[index] != model_policy::MaterialStatus::Resolved
                            && block.decoded.as_ref().is_some_and(|d| model_policy::fields(d).iter().any(|f| f.name == "Source Texture" && matches!(&f.value, fo4_nif::Value::Bytes(p) if p.is_empty()))) {
                            self.block(i, Error::new("UnverifiedEffectColorOnly", &c.original, block.data.start as u64, "measured untextured effect equation", "explicit empty Source Texture"))?;
                        }
                    }

                    let mut stack = Vec::new();
                    let mut seen = Vec::new();
                    work.reserve(&mut seen, n.blocks.len(), &c.original, 0)?;
                    seen.resize(n.blocks.len(), false);
                    for root in &n.roots {
                        if *root >= 0 {
                            work.reserve(&mut stack, 1, &c.original, 0)?;
                            stack.push((*root as usize, 0));
                        }
                    }
                    while let Some((index, d)) = stack.pop() {
                        limit(d, l.max_depth, &c.original, 0)?;
                        if seen[index] {
                            continue;
                        }
                        seen[index] = true;
                        let block = &n.blocks[index];
                        let ty = &n.block_types[block.type_index as usize];
                        let class = model_policy::block_class(ty);
                        if model_policy::visibility_editor_marker(&n, index) {
                            self.block(
                                i,
                                Error::new(
                                    "UnverifiedVisibilityEditorMarker",
                                    &c.original,
                                    block.data.start as u64,
                                    "measured visibility of helper geometry",
                                    "VisibilityEditorMarker:0 mesh; raw data retained",
                                ),
                            )?;
                        }
                        if model_policy::editor_marker_object(&n, index)
                            || model_policy::editor_marker_suffix(&n, index)
                        {
                            self.block(
                                i,
                                Error::new(
                                    "UnverifiedNifEditorMarkerVisibility",
                                    &c.original,
                                    block.data.start as u64,
                                    "live visibility measurement of named editor subtree",
                                    "EditorMarker subtree retained; no hiding fact asserted",
                                ),
                            )?;
                        }
                        if model_policy::editor_marker_suffix(&n, index) {
                            self.block(
                                i,
                                Error::new(
                                    "UnverifiedNifEditorMarkerSuffixVisibility",
                                    &c.original,
                                    block.data.start as u64,
                                    "live visibility and node-identity measurement",
                                    "EditorMarker decimal suffix; interpretation unverified",
                                ),
                            )?;
                        }
                        if class == Class::Outside {
                            if !self.nodes[i].outside_blocks.contains_key(ty) {
                                self.allocation.charge(
                                    ty.len() as u64 + 128,
                                    &c.original,
                                    block.data.start as u64,
                                )?;
                            }
                            let count = self.nodes[i].outside_blocks.entry(ty.clone()).or_default();
                            *count = add(*count, 1, &c.original, block.data.start as u64)?;
                            continue;
                        }
                        if class == Class::Unknown && block.decoded.is_some() {
                            self.block(
                                i,
                                Error::new(
                                    "UnsupportedNifBlockSemantics",
                                    &c.original,
                                    block.data.start as u64,
                                    "documented static-frame role",
                                    ty,
                                ),
                            )?;
                        }
                        if class == Class::State {
                            let a = model_policy::animation(&n, &policy, index, &self.allocation)?;
                            let visual = a.target_in_visual;
                            self.allocation.reserve(
                                &mut self.nodes[i].animations,
                                1,
                                &c.original,
                                block.data.start as u64,
                            )?;
                            self.nodes[i].animations.push(a);
                            for tag in rest.get(index).map_or(&[][..], Vec::as_slice) {
                                self.block(
                                    i,
                                    Error::new(
                                        "UnsupportedAnimationRuntime",
                                        &c.original,
                                        block.data.start as u64,
                                        "static state without controller evaluation",
                                        format!("{ty};target_in_visual={visual:?};rest={tag}"),
                                    ),
                                )?;
                            }
                        }
                        for e in &block.diagnostics {
                            self.block(i, e.clone())?;
                        }
                        let Some(data) = &block.decoded else {
                            self.block(
                                i,
                                Error::new(
                                    "UnsupportedNifBlock",
                                    &c.original,
                                    block.data.start as u64,
                                    "decoded reached block",
                                    ty,
                                ),
                            )?;
                            continue;
                        };
                        if !block.raw_tail.is_empty() {
                            self.nodes[i].uninterpreted += 1;
                            self.block(
                                i,
                                Error::new(
                                    "UnsupportedNifBlockTail",
                                    &c.original,
                                    block.raw_tail.start as u64,
                                    "fully decoded reached block",
                                    ty,
                                ),
                            )?;
                        }
                        let fields = match data {
                            fo4_nif::Data::Node { fields, children } => {
                                for child in children {
                                    if *child >= 0 {
                                        work.reserve(
                                            &mut stack,
                                            1,
                                            &c.original,
                                            block.data.start as u64,
                                        )?;
                                        stack.push((*child as usize, d + 1));
                                    }
                                }
                                fields
                            }
                            fo4_nif::Data::Fields(f) => f,
                            fo4_nif::Data::Mesh(m) => {
                                self.nodes[i].shapes = add(
                                    self.nodes[i].shapes,
                                    1,
                                    &c.original,
                                    block.data.start as u64,
                                )?;
                                if m.vertices
                                    .as_ref()
                                    .is_some_and(|v| v.iter().any(|v| v.raw_skin.is_some()))
                                {
                                    self.block(
                                        i,
                                        Error::new(
                                            "UnsupportedSkinning",
                                            &c.original,
                                            block.data.start as u64,
                                            "static vertices",
                                            ty,
                                        ),
                                    )?;
                                }
                                &m.fields
                            }
                        };
                        for f in fields {
                            match &f.value {
                                fo4_nif::Value::Ref(v) if *v >= 0 => {
                                    // Target is a weak back pointer, not owned controller data.
                                    if class == Class::State && model_policy::weak_edge(ty, f.name)
                                    {
                                        continue;
                                    }
                                    if policy.suppress[index] && f.name == "Texture Set" {
                                        self.nodes[i].suppressed_texture_sets = add(
                                            self.nodes[i].suppressed_texture_sets,
                                            1,
                                            &c.original,
                                            f.offset,
                                        )?;
                                        continue;
                                    }
                                    work.reserve(&mut stack, 1, &c.original, f.offset)?;
                                    stack.push((*v as usize, d + 1));
                                }
                                fo4_nif::Value::Raw(_) => {
                                    self.nodes[i].uninterpreted += 1;
                                    self.block(
                                        i,
                                        Error::new(
                                            "UnsupportedNifField",
                                            &c.original,
                                            f.offset,
                                            "interpreted reached field",
                                            format!("{ty}.{}", f.name),
                                        ),
                                    )?;
                                }
                                fo4_nif::Value::StringIndex(v)
                                    if matches!(f.name, "Name" | "Root Material")
                                        && matches!(
                                            ty.as_str(),
                                            "BSLightingShaderProperty" | "BSEffectShaderProperty"
                                        ) =>
                                {
                                    if let Some(path) = n.strings.get(*v as usize) {
                                        if !path.is_empty() {
                                            if path[path.len().saturating_sub(5)..]
                                                .eq_ignore_ascii_case(b".bgsm")
                                                || path[path.len().saturating_sub(5)..]
                                                    .eq_ignore_ascii_case(b".bgem")
                                            {
                                                push_dep(
                                                    &mut deps,
                                                    if f.name == "Root Material" {
                                                        Citation::NifRootMaterial
                                                    } else {
                                                        Citation::NifMaterial
                                                    },
                                                    path,
                                                    &work,
                                                    &c.original,
                                                    f.offset,
                                                )?;
                                                if f.name == "Name" {
                                                    if let Some(d) = deps.last_mut() {
                                                        d.shader =
                                                            Some(citing_shader(&n, ty, fields));
                                                    }
                                                }
                                            } else {
                                                self.block(
                                                    i,
                                                    Error::new(
                                                        "UnsupportedShaderMaterialName",
                                                        &c.original,
                                                        f.offset,
                                                        "BGSM/BGEM path or empty name",
                                                        fo4_core::HexBytes(path),
                                                    ),
                                                )?;
                                            }
                                        }
                                    }
                                }
                                fo4_nif::Value::Bytes(path)
                                    if !path.is_empty()
                                        && ((ty == "BSShaderTextureSet"
                                            && f.name == "Texture")
                                            || (ty == "BSEffectShaderProperty"
                                                && matches!(
                                                    f.name,
                                                    "Source Texture"
                                                        | "Greyscale Texture"
                                                        | "Environment Map Texture"
                                                        | "Normal Texture"
                                                        | "Environment Mask Texture"
                                                ))) =>
                                {
                                    push_dep(
                                        &mut deps,
                                        Citation::NifTexture,
                                        path,
                                        &work,
                                        &c.original,
                                        f.offset,
                                    )?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                AssetKind::Material => {
                    let mat = fo4_material::Material::read(&bytes, &c.original, l, &work)?;
                    use fo4_material::BlendMode;
                    let blend = &mat.common.alpha_blend;
                    if let fo4_material::Kind::Bgem(effect) = &mat.kind {
                        if effect.base_texture.bytes().is_empty() {
                            self.block(
                                i,
                                Error::new(
                                    "UnverifiedEffectColorOnly",
                                    &c.original,
                                    effect.base_texture.offset,
                                    "measured untextured effect equation",
                                    "explicit empty BGEM base texture",
                                ),
                            )?;
                        }
                    }

                    self.nodes[i].alpha = Some(MaterialAlpha {
                        enabled: blend.enabled,
                        source: blend.source,
                        destination: blend.destination,
                        alpha_test_enabled: mat.common.alpha_test.0,
                        alpha_test_ref: mat.common.alpha_test_ref,
                        previous_mode: blend.mode.as_ref().map(|m| match m {
                            BlendMode::Unknown => "Unknown",
                            BlendMode::None => "None",
                            BlendMode::Standard => "Standard",
                            BlendMode::Additive => "Additive",
                            BlendMode::Multiplicative => "Multiplicative",
                        }),
                        previously_blocked: matches!(blend.mode, None | Some(BlendMode::Unknown)),
                    });
                    self.nodes[i].uninterpreted = mat.uninterpreted_fields();
                    for (component, count) in mat.uninterpreted_components() {
                        self.block(
                            i,
                            Error::new(
                                "UnsupportedMaterialFields",
                                &c.original,
                                0,
                                format!("interpreted component; {count} uninterpreted fields"),
                                if component == "alpha_blend.enabled_factors_unsourced" {
                                    format!(
                                        "alpha_blend.enabled_factors_unsourced({},{},{})",
                                        blend.enabled, blend.source, blend.destination
                                    )
                                } else {
                                    component.into()
                                },
                            ),
                        )?;
                    }
                    for (_, p) in mat.texture_paths() {
                        if !p.bytes().is_empty() {
                            push_dep(
                                &mut deps,
                                Citation::MaterialTexture,
                                p.bytes(),
                                &work,
                                &c.original,
                                p.offset,
                            )?;
                        }
                    }
                    if let fo4_material::Kind::Bgsm(m) = &mat.kind {
                        if !m.root_material_path.bytes().is_empty() {
                            push_dep(
                                &mut deps,
                                Citation::MaterialParent,
                                m.root_material_path.bytes(),
                                &work,
                                &c.original,
                                m.root_material_path.offset,
                            )?;
                        }
                    }
                }
                AssetKind::Texture => {
                    fo4_texture::Texture::read(&bytes, &c.original, l, &work)?;
                }
            }
            for dep in deps {
                let path = match crate::paths::resolve_with(
                    dep.class,
                    &dep.path,
                    self.vfs,
                    self.path_policy,
                    &work,
                ) {
                    Ok(resolved) => {
                        for e in resolved.diagnostics(&c.original, dep.offset, &work)? {
                            self.block_citation(i, e)?;
                        }
                        let mut key = Vec::new();
                        self.allocation.reserve(
                            &mut key,
                            resolved.key.len(),
                            &c.original,
                            dep.offset,
                        )?;
                        key.extend_from_slice(&resolved.key);
                        if !resolved.hypotheses.is_empty() {
                            self.allocation.charge(
                                (dep.path.len() + resolved.key.len() + 512) as u64,
                                &c.original,
                                dep.offset,
                            )?;
                            self.allocation.reserve(
                                &mut self.nodes[i].path_resolutions,
                                1,
                                &c.original,
                                dep.offset,
                            )?;
                            self.nodes[i].path_resolutions.push(resolved);
                        }
                        key
                    }
                    Err(mut e) => {
                        let mut raw = Vec::new();
                        self.allocation.reserve(
                            &mut raw,
                            dep.path.len(),
                            &c.original,
                            dep.offset,
                        )?;
                        raw.extend_from_slice(&dep.path);
                        self.allocation.reserve(
                            &mut self.nodes[i].rejections,
                            1,
                            &c.original,
                            dep.offset,
                        )?;
                        self.nodes[i].rejections.push(Rejection {
                            class: dep.class,
                            offset: dep.offset,
                            raw,
                        });
                        e.source.file = c.original.clone();
                        e.source.offset = dep.offset;
                        self.block_citation(i, e)?;
                        continue;
                    }
                };
                match self.visit(
                    dep.class.asset_kind(),
                    &path,
                    &work,
                    depth + 1,
                    Some((dep.class, dep.offset)),
                ) {
                    Ok(target) => {
                        if target == i
                            && dep.class == Citation::MaterialParent
                            && kind == AssetKind::Material
                            && self.nodes[i]
                                .pending_live_proof
                                .contains(&"bgsm_self_wetness_template_stop_v1")
                        {
                            // Keep the authored edge in cycles, but the terminal
                            // file-set closure must not depend on its own ready bit.
                            continue;
                        }
                        self.allocation.reserve(
                            &mut self.nodes[i].dependencies,
                            1,
                            &c.original,
                            0,
                        )?;
                        self.nodes[i].dependencies.push(target);
                        self.allocation.reserve(
                            &mut self.nodes[i].dependency_citations,
                            1,
                            &c.original,
                            0,
                        )?;
                        self.nodes[i].dependency_citations.push(Cited {
                            class: dep.class,
                            shader: dep.shader,
                        });
                    }
                    Err(e) => self.block_citation(i, e)?,
                }
            }
            Ok(())
        })();
        if let Err(e) = attempt {
            self.block(i, e)?;
        }
        if self.active.pop().is_none_or(|f| f.node != i) {
            return Err(Error::new(
                "AssetCacheUnavailable",
                "asset stack",
                0,
                "balanced active stack",
                i,
            ));
        }
        self.nodes[i].loading = false;
        self.nodes[i].ready = self.nodes[i].blockers.is_empty()
            && self.nodes[i]
                .dependencies
                .iter()
                .all(|d| self.nodes[*d].ready);
        Ok(i)
    }
    fn block(&mut self, i: usize, e: Error) -> Result<()> {
        self.allocation.charge(
            (e.code.len() + e.expected.len() + e.found.len() + e.source.file.len() + 128) as u64,
            &e.source.file,
            e.source.offset,
        )?;
        self.allocation.reserve(
            &mut self.nodes[i].blockers,
            1,
            &e.source.file,
            e.source.offset,
        )?;
        self.nodes[i].blockers.push(e);
        Ok(())
    }
    /// [`Self::block`] for an error met while following a citation of node `i`.
    fn block_citation(&mut self, i: usize, e: Error) -> Result<()> {
        let at = self.nodes[i].blockers.len();
        self.allocation.reserve(
            &mut self.nodes[i].citation_blockers,
            1,
            &e.source.file,
            e.source.offset,
        )?;
        self.block(i, e)?;
        self.nodes[i].citation_blockers.push(at);
        Ok(())
    }
    /// Typed, bounded re-read of any declared asset; no whole-archive extraction.
    /// The caller's budget owns allocations moved out by the callback.
    pub fn with_decoded<R>(
        &self,
        id: usize,
        b: &Budget,
        f: impl for<'v> FnOnce(DecodedAsset<'v>) -> Result<R>,
    ) -> Result<R> {
        let node = self.nodes.get(id).ok_or_else(|| {
            Error::new("InvalidAssetIndex", "asset graph", 0, self.nodes.len(), id)
        })?;
        let c = self.vfs.winner_bytes(&node.path)?;
        let bytes = self.vfs.read_candidate(c, b)?;
        let l = self.vfs.limits();
        let parsed = match node.kind {
            AssetKind::Model => DecodedAsset::Model(fo4_nif::Nif::read(
                &bytes,
                &c.original,
                fo4_core::Game::Fallout4,
                l,
                b,
            )?),
            AssetKind::Material => {
                DecodedAsset::Material(fo4_material::Material::read(&bytes, &c.original, l, b)?)
            }
            AssetKind::Texture => {
                DecodedAsset::Texture(fo4_texture::Texture::read(&bytes, &c.original, l, b)?)
            }
        };
        f(parsed)
    }
    pub fn closure(&self, roots: &[usize], b: &Budget) -> Result<Vec<usize>> {
        let scratch = b.child();
        let mut result = Vec::new();
        let mut pending = Vec::new();
        let mut seen = std::collections::HashSet::new();
        scratch.reserve(&mut pending, roots.len(), "asset graph", 0)?;
        pending.extend_from_slice(roots);
        while let Some(i) = pending.pop() {
            let node = self.nodes.get(i).ok_or_else(|| {
                Error::new("InvalidAssetIndex", "asset graph", 0, self.nodes.len(), i)
            })?;
            if seen.contains(&i) {
                continue;
            }
            scratch.charge(32, "asset graph", 0)?;
            seen.try_reserve(1).map_err(|e| {
                Error::new("AllocationFailed", "asset graph", 0, "reached index", e)
            })?;
            seen.insert(i);
            b.reserve(&mut result, 1, "asset graph", 0)?;
            result.push(i);
            scratch.reserve(&mut pending, node.dependencies.len(), "asset graph", 0)?;
            pending.extend_from_slice(&node.dependencies);
        }
        result.sort_unstable();
        Ok(result)
    }
}
/// What a shader block with `fields` holds of its own: an effect shader's
/// source texture, a lighting shader's texture set's first texture.
fn citing_shader(n: &fo4_nif::Nif<'_>, ty: &str, fields: &[fo4_nif::Field]) -> CitingShader {
    let bytes = |f: &[fo4_nif::Field], name: &str| {
        f.iter().find_map(|x| match &x.value {
            fo4_nif::Value::Bytes(p) if x.name == name => Some(!p.is_empty()),
            _ => None,
        })
    };
    if ty == "BSEffectShaderProperty" {
        return CitingShader {
            effect: true,
            texture: bytes(fields, "Source Texture").unwrap_or(false),
        };
    }
    let set = fields.iter().find_map(|x| match x.value {
        fo4_nif::Value::Ref(r) if x.name == "Texture Set" && r >= 0 => Some(r as usize),
        _ => None,
    });
    let texture = set
        .and_then(|i| n.blocks.get(i))
        .and_then(|b| b.decoded.as_ref())
        .and_then(|d| bytes(model_policy::fields(d), "Texture"))
        .unwrap_or(false);
    CitingShader {
        effect: false,
        texture,
    }
}
fn push_dep(
    deps: &mut Vec<Dependency>,
    class: Citation,
    path: &[u8],
    b: &Budget,
    file: &str,
    offset: u64,
) -> Result<()> {
    b.reserve(deps, 1, file, offset)?;
    let mut v = Vec::new();
    b.reserve(&mut v, path.len(), file, offset)?;
    v.extend_from_slice(path);
    deps.push(Dependency {
        path: v,
        class,
        offset,
        shader: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_self_material_publishes_ready_but_two_files_still_block() {
        fn wire(parent: &[u8]) -> Vec<u8> {
            let mut bytes = super::material_fixture::fixture(b"BGSM", 2, false, b"").v;
            bytes[32] = 0;
            let budget = Budget::new(1 << 20);
            let parsed = fo4_material::Material::read(
                &bytes,
                "synthetic",
                &fo4_core::Limits::default(),
                &budget,
            )
            .unwrap();
            let fo4_material::Kind::Bgsm(material) = parsed.kind else {
                panic!()
            };
            let at = material.root_material_path.offset as usize;
            let mut replacement = ((parent.len() + 1) as u32).to_le_bytes().to_vec();
            replacement.extend(parent);
            replacement.push(0);
            bytes.splice(at - 4..at + 1, replacement);
            bytes
        }
        let dir = fo4_core::TestDir::new("case14-material-terminal");
        std::fs::create_dir(dir.join("materials")).unwrap();
        for (name, parent) in [
            ("self.bgsm", b"self.bgsm".as_slice()),
            ("a.bgsm", b"b.bgsm"),
            ("b.bgsm", b"a.bgsm"),
            ("child.bgsm", b"absent.bgsm"),
        ] {
            std::fs::write(dir.join("materials").join(name), wire(parent)).unwrap();
        }
        let manifest = fo4_core::Manifest {
            game: fo4_core::Game::Fallout4,
            root: dir.to_path_buf(),
            language: "english".into(),
            plugins: vec![fo4_core::PluginSpec {
                name: "synthetic.esm".into(),
                path: "synthetic.esm".into(),
            }],
            mounts: vec![fo4_core::Mount::Loose {
                plugin: None,
                base: None,
                files: vec![
                    "materials/self.bgsm".into(),
                    "materials/a.bgsm".into(),
                    "materials/b.bgsm".into(),
                    "materials/child.bgsm".into(),
                ],
            }],
            required_masters: Vec::new(),
            excluded: Vec::new(),
            build: Some("synthetic".into()),
            limits: fo4_core::Limits::default(),
            archive_invalidation: None,
        };
        let vfs = Vfs::mount(&manifest, &Budget::new(1 << 20)).unwrap();
        let budget = Budget::new(4 << 20);
        let mut assets = Assets::new(&vfs, &budget);
        let id = assets
            .load(AssetKind::Material, b"materials/self.bgsm", &budget)
            .unwrap();
        assert!(assets.nodes[id].ready, "{:?}", assets.nodes[id].blockers);
        assert!(assets.nodes[id].dependencies.is_empty());
        assert_eq!(assets.closure(&[id], &budget).unwrap(), [id]);
        assert_eq!(assets.nodes[id].cycles[0].edges.len(), 1);
        let a = assets
            .load(AssetKind::Material, b"materials/a.bgsm", &budget)
            .unwrap();
        let closure = assets.closure(&[a], &budget).unwrap();
        assert_eq!(closure.len(), 2);
        assert!(!assets.nodes[a].ready);
        assert!(closure.iter().any(|id| assets.nodes[*id]
            .blockers
            .iter()
            .any(|e| e.code == "AssetDependencyCycle" && e.found.starts_with("length=2;"))));
        // A parent file that is absent is cited as a parent only: the draw
        // build reads the child's own fields (`missing`).
        let child = assets
            .load(AssetKind::Material, b"materials/child.bgsm", &budget)
            .unwrap();
        assert_eq!(
            assets.nodes[child].dependency_citations[0].class,
            Citation::MaterialParent
        );
        let closure = assets.closure(&[child], &budget).unwrap();
        let missing = crate::missing::classify(&assets, &closure, &[], &budget).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].roles.parent, 1);
        assert_eq!(
            missing[0].read,
            crate::missing::Read::Needed(crate::missing::PARENT_ONLY)
        );
    }
    #[test]
    fn self_parent_is_a_length_one_citation_cycle_not_a_cache_failure() {
        let dir = fo4_core::TestDir::new("case13-cycle");
        let manifest = fo4_core::Manifest {
            game: fo4_core::Game::Fallout4,
            root: dir.to_path_buf(),
            language: "english".into(),
            plugins: vec![fo4_core::PluginSpec {
                name: "synthetic.esm".into(),
                path: "synthetic.esm".into(),
            }],
            mounts: Vec::new(),
            required_masters: Vec::new(),
            excluded: Vec::new(),
            build: None,
            limits: fo4_core::Limits::default(),
            archive_invalidation: None,
        };
        let v = Vfs::mount(&manifest, &Budget::new(1 << 20)).unwrap();
        let b = Budget::new(1 << 20);
        let mut assets = Assets::new(&v, &b);
        let path = b"materials/self.bgsm".to_vec();
        let mut key = vec![AssetKind::Material as u8];
        key.extend_from_slice(&path);
        assets.index.insert(key, 0);
        assets.nodes.push(Asset {
            pending_live_proof: Vec::new(),
            alpha: None,
            kind: AssetKind::Material,
            path,
            origin: None,
            dependencies: Vec::new(),
            dependency_citations: Vec::new(),
            blockers: Vec::new(),
            citation_blockers: Vec::new(),
            uninterpreted: 0,
            outside_blocks: Default::default(),
            shapes: 0,
            cycles: Vec::new(),
            animations: Vec::new(),
            suppressed_texture_sets: 0,
            rejections: Vec::new(),
            path_resolutions: Vec::new(),
            texture_audit: Vec::new(),
            ready: false,
            loading: true,
        });
        assets.active.push(Active {
            node: 0,
            incoming: None,
        });
        let result = assets
            .visit(
                AssetKind::Material,
                b"materials/self.bgsm",
                &b,
                1,
                Some((Citation::MaterialParent, 12)),
            )
            .unwrap();
        assert_eq!(result, 0);
        assert_eq!(
            assets.nodes[0].pending_live_proof,
            ["bgsm_self_wetness_template_stop_v1"]
        );
        // A different edge class is not covered by the reviewer's decision.
        let error = assets
            .visit(
                AssetKind::Material,
                b"materials/self.bgsm",
                &b,
                1,
                Some((Citation::MaterialTexture, 14)),
            )
            .unwrap_err();
        assert_eq!(error.code, "AssetDependencyCycle");
        let cycle = &assets.nodes[0].cycles[0];
        assert_eq!(cycle.edges.len(), 1);
        assert_eq!(cycle.edges[0].from, 0);
        assert_eq!(cycle.edges[0].to, 0);
        assert_eq!(cycle.edges[0].class, Citation::MaterialParent);
        assert_eq!(cycle.edges[0].offset, 12);
        assets.nodes.push(Asset {
            pending_live_proof: Vec::new(),
            alpha: None,
            kind: AssetKind::Material,
            path: b"materials/other.bgsm".to_vec(),
            origin: None,
            dependencies: Vec::new(),
            dependency_citations: Vec::new(),
            blockers: Vec::new(),
            citation_blockers: Vec::new(),
            uninterpreted: 0,
            outside_blocks: Default::default(),
            shapes: 0,
            cycles: Vec::new(),
            animations: Vec::new(),
            suppressed_texture_sets: 0,
            rejections: Vec::new(),
            path_resolutions: Vec::new(),
            texture_audit: Vec::new(),
            ready: false,
            loading: true,
        });
        assets.active.push(Active {
            node: 1,
            incoming: Some((Citation::MaterialParent, 20)),
        });
        let error = assets
            .visit(
                AssetKind::Material,
                b"materials/self.bgsm",
                &b,
                2,
                Some((Citation::MaterialParent, 30)),
            )
            .unwrap_err();
        assert_eq!(error.code, "AssetDependencyCycle");
        assert!(error.found.starts_with("length=2;"));
        assert!(assets.nodes[1].pending_live_proof.is_empty());
        assert!(!assets.unavailable());
    }
}
