//! Named static-frame rules. No path repair or fallback material selection.
use crate::paths::{resolve, Citation};
use fo4_core::{add, Budget, Error, Result};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_plugin::still::Class;
use fo4_vfs::Vfs;
use serde::Serialize;
pub const BLOCK_TABLE: &str = include_str!("../../../spec/static-blocks.tsv");
pub fn block_class(name: &str) -> Class {
    let row = BLOCK_TABLE
        .lines()
        .find(|s| s.split('\t').next() == Some(name));
    match row.and_then(|s| s.split('\t').nth(1)) {
        Some("used") => Class::Used,
        Some("outside") => Class::Outside,
        Some("state") => Class::State,
        _ => Class::Unknown,
    }
}
pub fn fields(data: &Data) -> &[Field] {
    match data {
        Data::Node { fields, .. } | Data::Fields(fields) => fields,
        Data::Mesh(m) => &m.fields,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MaterialStatus {
    Empty,
    Rejected,
    Missing,
    Blocked,
    Resolved,
    Unknown,
}
pub fn material_status(n: &Nif<'_>, index: usize, v: &Vfs, b: &Budget) -> Result<MaterialStatus> {
    material_status_with(n, index, v, crate::PathPolicy::Strict, b)
}
fn material_status_with(
    n: &Nif<'_>,
    index: usize,
    v: &Vfs,
    policy: crate::PathPolicy,
    b: &Budget,
) -> Result<MaterialStatus> {
    let Some(data) = &n.blocks[index].decoded else {
        return Ok(MaterialStatus::Unknown);
    };
    let value = fields(data)
        .iter()
        .find(|f| f.name == "Name")
        .map(|f| &f.value);
    let Some(Value::StringIndex(i)) = value else {
        return Ok(MaterialStatus::Unknown);
    };
    let Some(path) = n.strings.get(*i as usize) else {
        return Ok(MaterialStatus::Empty);
    };
    if path.is_empty() {
        return Ok(MaterialStatus::Empty);
    }
    if !path[path.len().saturating_sub(5)..].eq_ignore_ascii_case(b".bgsm")
        && !path[path.len().saturating_sub(5)..].eq_ignore_ascii_case(b".bgem")
    {
        return Ok(MaterialStatus::Rejected);
    }
    let key = match crate::paths::resolve_with(Citation::NifMaterial, path, v, policy, b) {
        Ok(k) => k.key,
        Err(e) if e.code == "RejectedVfsPath" => return Ok(MaterialStatus::Rejected),
        Err(e) => return Err(e),
    };
    Ok(match v.winner_bytes(&key) {
        Ok(_) => MaterialStatus::Resolved,
        Err(e) if e.code == "MissingPath" => MaterialStatus::Missing,
        Err(_) => MaterialStatus::Blocked,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Ownership {
    Material,
    NoMaterial,
    Mixed,
    Unreferenced,
}
pub struct ModelPolicy {
    pub material: Vec<MaterialStatus>,
    pub suppress: Vec<bool>,
    pub owners: Vec<u64>,
    pub resolved_owners: Vec<u64>,
    pub visual: Vec<bool>,
}
impl ModelPolicy {
    pub fn inspect(n: &Nif<'_>, v: &Vfs, b: &Budget) -> Result<Self> {
        Self::inspect_with(n, v, crate::PathPolicy::Strict, b)
    }
    pub fn inspect_with(
        n: &Nif<'_>,
        v: &Vfs,
        policy: crate::PathPolicy,
        b: &Budget,
    ) -> Result<Self> {
        let mut p = Self {
            material: Vec::new(),
            suppress: Vec::new(),
            owners: Vec::new(),
            resolved_owners: Vec::new(),
            visual: Vec::new(),
        };
        b.reserve(&mut p.material, n.blocks.len(), "shader policy", 0)?;
        p.material.resize(n.blocks.len(), MaterialStatus::Unknown);
        b.reserve(&mut p.suppress, n.blocks.len(), "shader policy", 0)?;
        p.suppress.resize(n.blocks.len(), false);
        b.reserve(&mut p.owners, n.blocks.len(), "shader policy", 0)?;
        p.owners.resize(n.blocks.len(), 0);
        b.reserve(&mut p.resolved_owners, n.blocks.len(), "shader policy", 0)?;
        p.resolved_owners.resize(n.blocks.len(), 0);
        b.reserve(&mut p.visual, n.blocks.len(), "shader policy", 0)?;
        p.visual.resize(n.blocks.len(), false);
        for (i, block) in n.blocks.iter().enumerate() {
            let ty = &n.block_types[block.type_index as usize];
            if !matches!(
                ty.as_str(),
                "BSLightingShaderProperty" | "BSEffectShaderProperty"
            ) {
                continue;
            }
            let work = b.child();
            p.material[i] = material_status_with(n, i, v, policy, &work)?;
            // bgsm_name_overrides_texture_set_v1; docs in spec/scene.md.
            p.suppress[i] =
                ty == "BSLightingShaderProperty" && p.material[i] == MaterialStatus::Resolved;
            if let Some(data) = &block.decoded {
                for f in fields(data).iter().filter(|f| f.name == "Texture Set") {
                    if let Value::Ref(target) = f.value {
                        if target >= 0 {
                            let t = target as usize;
                            p.owners[t] = add(p.owners[t], 1, "shader owners", f.offset)?;
                            p.resolved_owners[t] = add(
                                p.resolved_owners[t],
                                u64::from(p.material[i] == MaterialStatus::Resolved),
                                "shader owners",
                                f.offset,
                            )?;
                        }
                    }
                }
            }
        }
        let scratch = b.child();
        let mut stack = Vec::new();
        for root in &n.roots {
            if *root >= 0 {
                scratch.reserve(&mut stack, 1, "visual closure", 0)?;
                stack.push(*root as usize);
            }
        }
        while let Some(i) = stack.pop() {
            if p.visual[i] {
                continue;
            }
            let block = &n.blocks[i];
            let ty = &n.block_types[block.type_index as usize];
            if matches!(block_class(ty), Class::Outside | Class::State) {
                continue;
            }
            p.visual[i] = true;
            if let Some(data) = &block.decoded {
                if let Data::Node { children, .. } = data {
                    for child in children {
                        if *child >= 0 {
                            scratch.reserve(&mut stack, 1, "visual closure", 0)?;
                            stack.push(*child as usize);
                        }
                    }
                }
                for f in fields(data) {
                    if matches!(
                        f.name,
                        "Controller" | "Collision" | "Multi Bound" | "Extra Data"
                    ) || p.suppress[i] && f.name == "Texture Set"
                    {
                        continue;
                    }
                    if let Value::Ref(target) = f.value {
                        if target >= 0 {
                            scratch.reserve(&mut stack, 1, "visual closure", f.offset)?;
                            stack.push(target as usize);
                        }
                    }
                }
            }
        }
        Ok(p)
    }
    pub fn ownership(&self, index: usize) -> Ownership {
        match (self.owners[index], self.resolved_owners[index]) {
            (0, _) => Ownership::Unreferenced,
            (_, 0) => Ownership::NoMaterial,
            (a, b) if a == b => Ownership::Material,
            _ => Ownership::Mixed,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Animation {
    pub block_type: String,
    pub block: usize,
    pub target: Option<i32>,
    pub targets: Vec<i32>,
    pub target_in_visual: Option<bool>,
}
pub fn weak_edge(ty: &str, field: &str) -> bool {
    matches!(field, "Target" | "Extra Target")
        || ty == "NiControllerSequence" && field == "Manager"
        || ty == "NiDefaultAVObjectPalette" && matches!(field, "Scene" | "AV Object")
}
pub fn animation(n: &Nif<'_>, p: &ModelPolicy, index: usize, b: &Budget) -> Result<Animation> {
    let block = &n.blocks[index];
    let ty = &n.block_types[block.type_index as usize];
    let mut targets = Vec::new();
    let mut target = None;
    if let Some(data) = &block.decoded {
        for field in fields(data) {
            if matches!(field.name, "Target" | "Extra Target") {
                if let Value::Ref(v) = field.value {
                    if field.name == "Target" {
                        target = Some(v);
                    }
                    b.reserve(&mut targets, 1, "animation targets", field.offset)?;
                    targets.push(v);
                }
            }
        }
    }
    let mut target_in_visual = None;
    for target in targets.iter().filter(|v| **v >= 0) {
        target_in_visual = Some(target_in_visual.unwrap_or(false) || p.visual[*target as usize]);
    }
    b.charge(ty.len() as u64, "animation type", block.data.start as u64)?;
    let mut block_type = String::new();
    block_type.try_reserve_exact(ty.len()).map_err(|e| {
        Error::new(
            "AllocationFailed",
            "animation type",
            block.data.start as u64,
            ty.len(),
            e,
        )
    })?;
    block_type.push_str(ty);
    Ok(Animation {
        block_type,
        block: index,
        target,
        targets,
        target_in_visual,
    })
}

#[derive(Debug, Serialize)]
pub struct TextureCitation {
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub raw: Vec<u8>,
    pub offset: u64,
    pub ownership: Ownership,
    pub suppressed: bool,
}
impl ModelPolicy {
    /// Audits the original texture list even when its edge is superseded.
    /// Only paths rejected by the unchanged strict citation rule are retained.
    pub fn texture_citations(&self, n: &Nif<'_>, b: &Budget) -> Result<Vec<TextureCitation>> {
        let mut out = Vec::new();
        for (i, block) in n.blocks.iter().enumerate() {
            let ty = &n.block_types[block.type_index as usize];
            let Some(data) = &block.decoded else { continue };
            for field in fields(data) {
                if !(ty == "BSShaderTextureSet" && field.name == "Texture"
                    || ty == "BSEffectShaderProperty"
                        && matches!(
                            field.name,
                            "Source Texture"
                                | "Greyscale Texture"
                                | "Environment Map Texture"
                                | "Normal Texture"
                                | "Environment Mask Texture"
                        ))
                {
                    continue;
                }
                let Value::Bytes(raw) = &field.value else {
                    continue;
                };
                if raw.is_empty() {
                    continue;
                }
                let temp = b.child();
                match resolve(Citation::NifTexture, raw, &temp) {
                    Ok(_) => continue,
                    Err(e) if e.code == "RejectedVfsPath" => {}
                    Err(e) => return Err(e),
                }
                let ownership = if ty == "BSShaderTextureSet" {
                    self.ownership(i)
                } else if self.material[i] == MaterialStatus::Resolved {
                    Ownership::Material
                } else {
                    Ownership::NoMaterial
                };
                // In the supported schema only BSLightingShaderProperty owns a Texture Set.
                let suppressed = ty == "BSShaderTextureSet"
                    && self.owners[i] > 0
                    && self.owners[i] == self.resolved_owners[i];
                let mut owned = Vec::new();
                b.reserve(
                    &mut owned,
                    raw.len(),
                    "texture citation audit",
                    field.offset,
                )?;
                owned.extend_from_slice(raw);
                b.reserve(&mut out, 1, "texture citation audit", field.offset)?;
                out.push(TextureCitation {
                    raw: owned,
                    offset: field.offset,
                    ownership,
                    suppressed,
                });
            }
        }
        Ok(out)
    }
}

/// Exact FO4 AVObject data label. Hiding the object/subtree remains a hypothesis.
pub fn editor_marker_object(n: &Nif<'_>, index: usize) -> bool {
    let Some(data) = n.blocks.get(index).and_then(|b| b.decoded.as_ref()) else {
        return false;
    };
    let fields = fields(data);
    fields.iter().any(|f| f.name == "Translation")
        && fields.iter().any(|f| f.name == "Name" && matches!(f.value, Value::StringIndex(i) if n.strings.get(i as usize).is_some_and(|s|s==b"EditorMarker")))
}

/// Observed decimal labels; their visibility meaning remains an explicit hypothesis.
pub fn marker_decimal_label(name: &[u8]) -> bool {
    let Some(tail) = name.strip_prefix(b"EditorMarker") else {
        return false;
    };
    let digits = |v: &[u8]| !v.is_empty() && v.iter().all(u8::is_ascii_digit);
    if let Some(v) = tail.strip_prefix(b":") {
        return digits(v);
    }
    let mut parts = tail.split(|c| *c == b':');
    let Some(first) = parts.next() else {
        return false;
    };
    digits(first) && parts.next().is_none_or(digits) && parts.next().is_none()
}
pub fn editor_marker_suffix(n: &Nif<'_>, index: usize) -> bool {
    let Some(data) = n.blocks.get(index).and_then(|b| b.decoded.as_ref()) else {
        return false;
    };
    let f = fields(data);
    f.iter().any(|f|f.name=="Translation")&&f.iter().any(|f|f.name=="Name"&&matches!(f.value,Value::StringIndex(i) if n.strings.get(i as usize).is_some_and(|s|marker_decimal_label(s))))
}

/// Narrow measured helper label; its visibility remains a registered hypothesis.
pub fn visibility_editor_marker(n: &Nif<'_>, index: usize) -> bool {
    let Some(fo4_nif::Data::Mesh(m)) = n.blocks.get(index).and_then(|b| b.decoded.as_ref()) else {
        return false;
    };
    m.fields.iter().any(|f| f.name == "Name" && matches!(f.value, Value::StringIndex(i) if n.strings.get(i as usize).is_some_and(|s|s==b"VisibilityEditorMarker:0")))
}
