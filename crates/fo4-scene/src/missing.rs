//! Material files that an instance's assets cite but that no mounted archive
//! or loose folder holds (`MissingMaterial`): who cites each one, and
//! whether the draw build reads it (`spec/materials.md`).
//!
//! The draw build reads, per shape, the material its shader's `Name` cites,
//! after the material swaps that apply (`fo4_render::draw`). It does not
//! read a swap's BNAM file (only its path, to compare), a swap's SNAM file
//! whose BNAM no model of the placement cites, or a `Name` material that a
//! swap of the placement replaces. Those missing files are [`Read::NotRead`]:
//! a fact of the draw build, not a claim about the game. A `Name` material
//! that is read, cited only by lighting shaders that carry a texture of
//! their own, is [`Read::EmbeddedShader`]: drawing it with the shader's own
//! fields is the pending rule [`crate::hypotheses::MISSING_MATERIAL_EMBEDDED`].
//! Everything else stays [`Read::Needed`].
//!
//! Texture files that no mounted archive or loose folder holds
//! (`MissingTexture`) are sorted by the slots that cite them
//! ([`unread_textures`]): one cited only where the draw build never samples
//! (`crate::TextureSlot::sampled_by_frame`) keeps a narrower code that the
//! pending rule [`crate::hypotheses::MISSING_TEXTURE_UNREAD`] covers.
use crate::paths::Citation;
use crate::{AssetKind, Assets};
use fo4_core::{Budget, Result};
use fo4_plugin::typed::{Loaded, Mswp, SwapValue};

/// Who cites a missing material, counted by citation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    /// A lighting shader's `Name` whose texture set has a first texture.
    pub name_lighting_textured: u32,
    /// A lighting shader's `Name` without a texture of its own.
    pub name_lighting_bare: u32,
    /// An effect shader's `Name`.
    pub name_effect: u32,
    /// A shader's `Root Material`.
    pub root_material: u32,
    /// A material's parent (`Root Material Path` of a BGSM).
    pub parent: u32,
    /// A swap's BNAM (the material it replaces).
    pub swap_from: u32,
    /// A swap's SNAM whose BNAM a model of the placement cites.
    pub swap_to_cited: u32,
    /// A swap's SNAM whose BNAM no model of the placement cites.
    pub swap_to_uncited: u32,
    /// Any other root of the instance (a texture set's material).
    pub other: u32,
    /// A swap of the placement replaces this path (it is a BNAM with an SNAM).
    pub replaced: bool,
}
impl Roles {
    fn names(&self) -> u32 {
        self.name_lighting_textured + self.name_lighting_bare + self.name_effect
    }
}

/// Whether the draw build reads a missing material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// Never read; the reason in words.
    NotRead(&'static str),
    /// Read only as a lighting shader's `Name`, every such shader with a
    /// texture of its own.
    EmbeddedShader,
    /// Read; the narrowest reason in words.
    Needed(&'static str),
}

/// [`Read::Needed`] for a file cited only as a material's parent (and
/// otherwise not read): the draw build reads the material's own fields.
pub const PARENT_ONLY: &str = "material parent";

#[derive(Debug, Clone)]
pub struct Missing {
    /// Asset node index.
    pub node: usize,
    pub roles: Roles,
    pub read: Read,
}

fn is_missing(assets: &Assets, i: usize) -> bool {
    assets.nodes.get(i).is_some_and(|n| {
        n.kind == AssetKind::Material && n.blockers.iter().any(|e| e.code == "MissingMaterial")
    })
}

/// (BNAM key, SNAM key) pairs of a swap, resolved as the draw build
/// resolves them; entries that do not resolve are left out (they raise
/// their own blockers).
pub fn swap_pairs(
    assets: &Assets,
    swap: &Loaded<Mswp>,
    b: &Budget,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut from: Option<Vec<u8>> = None;
    for e in &swap.data.entries {
        let SwapValue::Path(p) = &e.value else {
            continue;
        };
        let key = match assets.resolve_path(Citation::SwapMaterial, p.as_bytes(), b) {
            Ok(r) => Some(r.key),
            Err(e) if matches!(e.code.as_str(), "AllocationFailed" | "LimitExceeded") => {
                return Err(e)
            }
            Err(_) => None,
        };
        match &e.signature {
            b"BNAM" => from = key,
            b"SNAM" => {
                if let (Some(f), Some(t)) = (from.take(), key) {
                    b.reserve(&mut out, 1, "material swap", 0)?;
                    out.push((f, t));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The missing materials among `nodes` (an instance's asset closure), with
/// the swaps that apply to the instance (the reference's, the base's and
/// its SCOL parts' bases').
pub fn classify(
    assets: &Assets,
    nodes: &[usize],
    swaps: &[&Loaded<Mswp>],
    b: &Budget,
) -> Result<Vec<Missing>> {
    let mut out: Vec<Missing> = Vec::new();
    for i in nodes {
        if is_missing(assets, *i) {
            b.reserve(&mut out, 1, "missing materials", 0)?;
            out.push(Missing {
                node: *i,
                roles: Roles::default(),
                read: Read::Needed("not classified"),
            });
        }
    }
    if out.is_empty() {
        return Ok(out);
    }
    // Keys every model of the instance cites by a shader's `Name`.
    let mut names: Vec<&[u8]> = Vec::new();
    let mut cited_anywhere = vec![false; out.len()];
    for i in nodes {
        let Some(node) = assets.nodes.get(*i) else {
            continue;
        };
        for (d, c) in node.dependencies.iter().zip(&node.dependency_citations) {
            if c.class == Citation::NifMaterial {
                if let Some(t) = assets.nodes.get(*d) {
                    b.reserve(&mut names, 1, "missing materials", 0)?;
                    names.push(&t.path);
                }
            }
            let Some(k) = out.iter().position(|m| m.node == *d) else {
                continue;
            };
            cited_anywhere[k] = true;
            let r = &mut out[k].roles;
            match (c.class, c.shader) {
                (Citation::NifMaterial, Some(s)) if s.effect => r.name_effect += 1,
                (Citation::NifMaterial, Some(s)) if s.texture => r.name_lighting_textured += 1,
                (Citation::NifMaterial, _) => r.name_lighting_bare += 1,
                (Citation::NifRootMaterial, _) => r.root_material += 1,
                (Citation::MaterialParent, _) => r.parent += 1,
                _ => r.other += 1,
            }
        }
    }
    let mut pairs = Vec::new();
    for s in swaps {
        let p = swap_pairs(assets, s, b)?;
        b.reserve(&mut pairs, p.len(), "missing materials", 0)?;
        pairs.extend(p);
    }
    for (k, m) in out.iter_mut().enumerate() {
        let key = assets.nodes[m.node].path.as_slice();
        let mut in_swap = false;
        for (from, to) in &pairs {
            if from.as_slice() == key {
                m.roles.swap_from += 1;
                m.roles.replaced = true;
                in_swap = true;
            }
            if to.as_slice() == key {
                in_swap = true;
                if names.contains(&from.as_slice()) {
                    m.roles.swap_to_cited += 1;
                } else {
                    m.roles.swap_to_uncited += 1;
                }
            }
        }
        if !cited_anywhere[k] && !in_swap {
            m.roles.other += 1;
        }
        let r = &m.roles;
        let name_read = r.names() > 0 && !r.replaced;
        m.read = if r.root_material > 0 {
            Read::Needed("root material")
        } else if r.other > 0 {
            Read::Needed("other citation")
        } else if r.swap_to_cited > 0 {
            Read::Needed("swap target of a cited material")
        } else if name_read && (r.name_effect > 0 || r.name_lighting_bare > 0) {
            Read::Needed(if r.name_effect > 0 {
                "effect shader name"
            } else {
                "lighting shader name without its own texture"
            })
        } else if name_read && r.parent > 0 {
            Read::Needed("material parent and shader name")
        } else if r.parent > 0 {
            Read::Needed(PARENT_ONLY)
        } else if name_read {
            Read::EmbeddedShader
        } else if r.names() > 0 {
            Read::NotRead("shader name replaced by a swap")
        } else if r.swap_to_uncited > 0 {
            Read::NotRead("swap target of a material no model cites")
        } else {
            Read::NotRead("swap source only")
        };
    }
    Ok(out)
}

/// The missing textures among `nodes` (an instance's asset closure) that
/// every citation in `nodes` cites from a slot the frame never samples; a
/// texture with no citation in `nodes` counts as sampled.
pub fn unread_textures(assets: &Assets, nodes: &[usize], b: &Budget) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for t in nodes {
        let missing = assets.nodes.get(*t).is_some_and(|n| {
            n.kind == AssetKind::Texture && n.blockers.iter().any(|e| e.code == "MissingTexture")
        });
        if !missing {
            continue;
        }
        let mut cited = false;
        let mut sampled = false;
        for i in nodes {
            let Some(node) = assets.nodes.get(*i) else {
                continue;
            };
            for (d, c) in node.dependencies.iter().zip(&node.dependency_citations) {
                if d == t {
                    cited = true;
                    sampled |= c.slot.is_none_or(|s| s.sampled_by_frame());
                }
            }
        }
        if cited && !sampled {
            b.reserve(&mut out, 1, "missing textures", 0)?;
            out.push(*t);
        }
    }
    Ok(out)
}

/// The refused texture citations among `nodes` (an instance's asset
/// closure) that cite a file missing whichever reading from a slot the
/// frame never samples: a rootless path no archive or loose folder holds
/// with its citation's root added (`Rejection::rooted_missing`), cited from
/// a slot `TextureSlot::sampled_by_frame` says the frame does not read.
/// Each as the citing node and the citation's offset, the offset its
/// `RejectedVfsPath` blocker carries.
pub fn unread_rejections(
    assets: &Assets,
    nodes: &[usize],
    b: &Budget,
) -> Result<Vec<(usize, u64)>> {
    let mut out = Vec::new();
    for i in nodes {
        let Some(node) = assets.nodes.get(*i) else {
            continue;
        };
        for r in &node.rejections {
            if r.class.asset_kind() == AssetKind::Texture
                && r.rooted_missing
                && r.slot.is_some_and(|s| !s.sampled_by_frame())
            {
                b.reserve(&mut out, 1, "missing textures", 0)?;
                out.push((*i, r.offset));
            }
        }
    }
    Ok(out)
}

/// The swaps that apply to an instance: the reference's, the base's and,
/// recursively, those of its SCOL parts' bases.
pub fn instance_swaps(i: &crate::Instance) -> Vec<&Loaded<Mswp>> {
    swaps_of(i.material_swap.as_ref(), i.base.as_ref())
}
/// [`instance_swaps`] from the parts of an instance being built.
pub fn swaps_of<'a>(
    reference: Option<&'a Loaded<Mswp>>,
    base: Option<&'a crate::Base>,
) -> Vec<&'a Loaded<Mswp>> {
    fn walk<'a>(v: &'a crate::Base, out: &mut Vec<&'a Loaded<Mswp>>) {
        if let Some(s) = &v.material_swap {
            out.push(s);
        }
        for p in &v.parts {
            if let Some(r) = &p.resolved {
                walk(r, out);
            }
        }
    }
    let mut out = Vec::new();
    out.extend(reference);
    if let Some(v) = base {
        walk(v, &mut out);
    }
    out
}
