//! Renderer-independent draw list. Unknown bindings produce omissions, never white textures.
use crate::{append, Alpha, Draw, DrawList, Mesh, Texture, Vertex};
use fo4_core::{Budget, Error, Limits, Result};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_plugin::{
    typed::{Loaded, Mswp, SwapValue},
    FormKey,
};
use fo4_scene::{
    model_policy::{block_class, fields},
    paths::Citation,
    transform::Transform,
    AssetKind, Base, DecodedAsset, Loader, Scene,
};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Serialize)]
pub struct Omission {
    pub instance: u32,
    pub reference: FormKey,
    pub causes: Vec<Error>,
}
#[derive(Debug, Serialize)]
pub struct Identity {
    pub index: u32,
    pub reference: FormKey,
    pub base: Option<FormKey>,
    pub hypotheses: Vec<&'static str>,
    pub editor_marker: bool,
}
#[derive(Debug, Serialize)]
pub struct TextureChoice {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub mip: u32,
    pub srgb: bool,
    pub declared_dxgi: Option<u32>,
    pub declared_fourcc: Option<u32>,
    pub color_declaration: &'static str,
    pub coarse_mips: Vec<(u32, u32, u32)>,
}
#[derive(Debug)]
pub struct Build {
    pub skipped_editor_marker_subtrees: u64,
    /// Instance indices of the placed actors, after the placed references.
    pub actors: Vec<u32>,
    /// Shapes of drawn actor models without a skin, not drawn.
    pub actor_rigid_shapes: u64,
    /// Heads of drawn actors, or parts of them, left out, with the causes.
    pub actor_head_omissions: Vec<Omission>,
    pub list: DrawList,
    pub instances: Vec<Identity>,
    pub omitted: Vec<Omission>,
    pub texture_choices: Vec<TextureChoice>,
}
/// One skinned actor mesh of a posed build ([`build_with_posed_actors`]),
/// kept so that a caller can pose it again: the draw-list mesh, the skeleton
/// asset that poses it and the shape's skin.
#[derive(Debug, Clone)]
pub struct SkinnedMesh {
    pub mesh: usize,
    pub skeleton: usize,
    pub shape: fo4_scene::actor::pose::SkinnedShape,
}
#[derive(Clone, Copy)]
pub struct BuildOptions {
    pub max_texture_dimension: u32,
    pub max_texture_bytes: u64,
    /// Instances whose state is not ready at this level are omitted with a cause.
    pub level: fo4_scene::RuleLevel,
    /// Keep block-compressed textures as their stored blocks (for a GPU
    /// consumer) instead of decoding them; `None` in the reference.
    pub blocks: Option<BlockOptions>,
}
/// Limits of the textures kept as stored blocks ([`BuildOptions::blocks`]).
/// A texture whose format or chosen level cannot be kept as blocks is
/// decoded under the reference limits instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockOptions {
    /// Largest side of the first level kept.
    pub max_dimension: u32,
    /// The same for a texture drawn with an alpha test.
    pub max_alpha_tested_dimension: u32,
    /// Total stored bytes of the kept levels, apart from `max_texture_bytes`.
    pub max_bytes: u64,
}
impl BuildOptions {
    /// Texture limits of the reference renders (the `render` example, and the
    /// GPU viewer with `--reference-textures`).
    pub const fn reference(level: fo4_scene::RuleLevel) -> Self {
        Self {
            max_texture_dimension: 256,
            max_texture_bytes: 96 << 20,
            level,
            blocks: None,
        }
    }
    /// The reference limits, with block-compressed textures kept as blocks.
    pub const fn with_blocks(level: fo4_scene::RuleLevel, blocks: BlockOptions) -> Self {
        Self {
            blocks: Some(blocks),
            ..Self::reference(level)
        }
    }
    /// The largest first-level side of a texture kept as blocks, by whether
    /// it is drawn with an alpha test; `None` in the reference.
    pub fn block_dimension(&self, alpha_tested: bool) -> Option<u32> {
        self.blocks.map(|b| {
            if alpha_tested {
                b.max_alpha_tested_dimension
            } else {
                b.max_dimension
            }
        })
    }
}
fn error(code: &str, file: &str, offset: u64, found: impl std::fmt::Display) -> Error {
    Error::new(code, file, offset, "documented unlit draw binding", found)
}
fn raw_ref(fields: &[Field], name: &str) -> Option<i32> {
    fields.iter().find_map(|f| {
        if f.name == name {
            if let Value::Ref(i) = f.value {
                Some(i)
            } else {
                None
            }
        } else {
            None
        }
    })
}
fn floats<const N: usize>(fields: &[Field], name: &str, file: &str) -> Result<[f64; N]> {
    let mut out = [0.; N];
    let mut count = 0;
    for f in fields.iter().filter(|f| f.name == name) {
        if let Value::FloatBits(v) = f.value {
            if count == N {
                return Err(error("RenderFieldCount", file, f.offset, name));
            }
            out[count] = f32::from_bits(v) as f64;
            count += 1;
        } else {
            return Err(error("RenderFieldType", file, f.offset, name));
        }
    }
    if count != N || out.iter().any(|v| !v.is_finite()) {
        return Err(error(
            "RenderFieldCount",
            file,
            0,
            format!("{name}: {count}/{N}"),
        ));
    }
    Ok(out)
}
fn decoded<'a>(n: &'a Nif<'_>, i: usize, file: &str) -> Result<&'a Data> {
    n.blocks
        .get(i)
        .and_then(|b| b.decoded.as_ref())
        .ok_or_else(|| error("RenderOpaqueBlock", file, 0, i))
}
fn ni_alpha(n: &Nif<'_>, shape: &[Field], file: &str) -> Result<Option<(u16, u8)>> {
    let Some(index) = raw_ref(shape, "Alpha Property").filter(|i| *i >= 0) else {
        return Ok(None);
    };
    let f = fields(decoded(n, index as usize, file)?);
    let flags = f.iter().find_map(|v| {
        if v.name == "Alpha Flags" {
            if let Value::U16(x) = v.value {
                Some(x)
            } else {
                None
            }
        } else {
            None
        }
    });
    let threshold = f.iter().find_map(|v| {
        if v.name == "Threshold" {
            if let Value::U8(x) = v.value {
                Some(x)
            } else {
                None
            }
        } else {
            None
        }
    });
    match (flags, threshold) {
        (Some(f), Some(t)) => Ok(Some((f, t))),
        _ => Err(error("RenderAlphaProperty", file, 0, index)),
    }
}
fn effect_channels(flags1: Option<u32>, flags2: Option<u32>, file: &str) -> Result<u8> {
    let a = flags1.ok_or_else(|| error("MissingEffectFlags", file, 0, "Shader Flags 1"))?;
    let b = flags2.ok_or_else(|| error("MissingEffectFlags", file, 0, "Shader Flags 2"))?;
    Ok(u8::from(b & (1 << 5) != 0) | (u8::from(a & (1 << 3) != 0) << 1))
}
struct Surface {
    channels: u8,
    requires_uv: bool,
    texture: usize,
    alpha: Alpha,
    offset: [f64; 2],
    scale: [f64; 2],
    tile: Option<u32>,
    sidedness: crate::Sidedness,
}
struct Builder<'a, 's> {
    loader: &'a Loader<'s>,
    limits: &'a Limits,
    budget: &'a Budget,
    options: BuildOptions,
    output: Build,
    /// By model, shape, effect channels, UV use and, for a skinned mesh,
    /// the skeleton asset it is posed by.
    meshes: BTreeMap<(usize, usize, u8, bool, Option<usize>), usize>,
    skeletons: BTreeMap<usize, fo4_scene::actor::pose::Skeleton>,
    /// Skeleton assets posed by the caller, whose meshes are recorded.
    posed: std::collections::BTreeSet<usize>,
    recorded: Vec<SkinnedMesh>,
    /// By path and the block limit applied (0 when decoded).
    textures: BTreeMap<(Vec<u8>, u32), usize>,
    texture_bytes: u64,
    block_bytes: u64,
    visits: u64,
    allow_path_strip: bool,
    allow_path_root: bool,
    allow_path_relative: bool,
    allow_effect_color: bool,
    allow_scol_parts: bool,
    allow_editor_marker_subtrees: bool,
    allow_marker_suffix: bool,
    include_marker_subtrees: bool,
    allow_visibility_marker: bool,
    allow_swap_folder: bool,
    /// Registered rules the current instance's draws relied on beyond its
    /// state's (`Identity::hypotheses` gains them).
    draw_rules: Vec<&'static str>,
}
/// A diffuse texture decoded under the reference limits, with the DDS
/// declarations the summary reports.
pub struct DecodedTexture {
    /// With stored blocks, `rgba` and `mips` are empty and `blocks` holds
    /// the texels.
    pub texture: Texture,
    pub blocks: Option<crate::Blocks>,
    pub declared_dxgi: Option<u32>,
    pub declared_fourcc: u32,
}
/// Decodes texture asset `id` of `loader` as the draw-list builder does: the
/// first single-surface level not larger than `options.max_texture_dimension`
/// (else the smallest), then every coarser level while the running total
/// `texture_bytes` stays within `options.max_texture_bytes`. Returns the
/// texture before `Texture::validate`.
pub fn decode_texture(
    loader: &Loader<'_>,
    id: usize,
    options: BuildOptions,
    texture_bytes: &mut u64,
    limits: &Limits,
    budget: &Budget,
) -> Result<DecodedTexture> {
    let scratch = budget.child();
    let mut declared_dxgi = None;
    let mut declared_fourcc = 0;
    let texture = loader.assets.with_decoded(id, &scratch, |d| {
        let DecodedAsset::Texture(t) = d else {
            return Err(error("RenderAssetKind", "texture", 0, id));
        };
        declared_dxgi = t.dx10.map(|d| d[0]);
        declared_fourcc = t.header[20];
        if t.cubemap || t.array_size != 1 || t.depth != 1 {
            return Err(error(
                "RenderDiffuseTextureShape",
                "texture",
                0,
                format!(
                    "array={} cube={} depth={}",
                    t.array_size, t.cubemap, t.depth
                ),
            ));
        }
        let index = t
            .surfaces
            .iter()
            .position(|s| {
                s.array == 0
                    && s.face == 0
                    && s.depth == 1
                    && s.width <= options.max_texture_dimension
                    && s.height <= options.max_texture_dimension
            })
            .or_else(|| {
                t.surfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.array == 0 && s.face == 0 && s.depth == 1)
                    .min_by_key(|(_, s)| s.width as u64 * s.height as u64)
                    .map(|(i, _)| i)
            })
            .ok_or_else(|| {
                error(
                    "NoBudgetedDiffuseMip",
                    "texture",
                    0,
                    options.max_texture_dimension,
                )
            })?;
        let s = &t.surfaces[index];
        let bytes = fo4_core::mul(
            fo4_core::mul(s.width as u64, s.height as u64, "texture", 0)?,
            16,
            "texture",
            0,
        )?;
        let total = fo4_core::add(*texture_bytes, bytes, "diffuse texture budget", 0)?;
        fo4_core::limit(
            total,
            options.max_texture_bytes,
            "diffuse texture budget",
            0,
        )?;
        let rgba = t.decode(index, limits, budget)?;
        *texture_bytes = total;
        let mut mips = Vec::new();
        for (i, level) in t
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, m)| m.array == 0 && m.face == 0 && m.depth == 1 && m.mip > s.mip)
        {
            let size = fo4_core::mul(
                fo4_core::mul(level.width as u64, level.height as u64, "mip", 0)?,
                16,
                "mip",
                0,
            )?;
            let total = fo4_core::add(*texture_bytes, size, "mip budget", 0)?;
            if total > options.max_texture_bytes {
                break;
            }
            let rgba = t.decode(i, limits, budget)?;
            append(
                &mut mips,
                crate::Mip {
                    width: level.width,
                    height: level.height,
                    rgba,
                    mip: level.mip,
                },
                budget,
            )?;
            *texture_bytes = total;
        }
        Ok(Texture {
            mips,
            width: s.width,
            height: s.height,
            rgba,
            mip: s.mip,
            srgb: t.srgb,
        })
    })?;
    Ok(DecodedTexture {
        texture,
        blocks: None,
        declared_dxgi,
        declared_fourcc,
    })
}
/// Keeps texture asset `id` as its stored blocks: the first single-surface
/// level whose sides are at most `max_dimension`, and every coarser stored
/// level after it, counted in `block_bytes` against `max_bytes`. `Ok(None)`
/// when the texture cannot be kept so (not a 2D texture, a layout without a
/// GPU block format, no such level, or a level whose sides are not
/// multiples of 4); the caller then decodes it.
pub fn block_texture(
    loader: &Loader<'_>,
    id: usize,
    max_dimension: u32,
    max_bytes: u64,
    block_bytes: &mut u64,
    budget: &Budget,
) -> Result<Option<DecodedTexture>> {
    use fo4_texture::Format;
    let scratch = budget.child();
    loader.assets.with_decoded(id, &scratch, |d| {
        let DecodedAsset::Texture(t) = d else {
            return Err(error("RenderAssetKind", "texture", 0, id));
        };
        if t.cubemap || t.array_size != 1 || t.depth != 1 {
            return Ok(None);
        }
        let (format, opaque) = match t.format {
            Format::Bc1 => (crate::BlockFormat::Bc1, true),
            Format::Bc1Alpha => (crate::BlockFormat::Bc1Alpha, false),
            Format::Bc2 => (crate::BlockFormat::Bc2, false),
            Format::Bc3 => (crate::BlockFormat::Bc3, false),
            Format::Bc4 => (crate::BlockFormat::Bc4, false),
            Format::Bc5 => (crate::BlockFormat::Bc5, false),
            Format::Bc7 => (crate::BlockFormat::Bc7, false),
            _ => return Ok(None),
        };
        let plain = |s: &fo4_texture::Surface| s.array == 0 && s.face == 0 && s.depth == 1;
        let Some(index) = t
            .surfaces
            .iter()
            .position(|s| plain(s) && s.width <= max_dimension && s.height <= max_dimension)
        else {
            return Ok(None);
        };
        let s = &t.surfaces[index];
        if !s.width.is_multiple_of(4) || !s.height.is_multiple_of(4) {
            return Ok(None);
        }
        let mut chosen = Vec::new();
        for (i, level) in t.surfaces.iter().enumerate() {
            if !plain(level) || level.mip < s.mip {
                continue;
            }
            let k = level.mip - s.mip;
            if k as usize != chosen.len()
                || level.width != (s.width >> k.min(31)).max(1)
                || level.height != (s.height >> k.min(31)).max(1)
            {
                return Ok(None);
            }
            chosen.push(i);
        }
        let mut size = 0u64;
        for &i in &chosen {
            let level = &t.surfaces[i];
            let n = t
                .surface_bytes(i)
                .ok_or_else(|| error("RenderBlockSurface", "texture", 0, i))?
                .len() as u64;
            if n != format.level_bytes(level.width, level.height) {
                return Ok(None);
            }
            size = fo4_core::add(size, n, "block texture", 0)?;
        }
        let total = fo4_core::add(*block_bytes, size, "block texture budget", 0)?;
        fo4_core::limit(total, max_bytes, "block texture budget", 0)?;
        let mut bytes = Vec::new();
        budget.reserve(&mut bytes, size as usize, "block texture", 0)?;
        for &i in &chosen {
            bytes.extend_from_slice(t.surface_bytes(i).unwrap_or_default());
        }
        *block_bytes = total;
        Ok(Some(DecodedTexture {
            declared_dxgi: t.dx10.map(|d| d[0]),
            declared_fourcc: t.header[20],
            texture: Texture {
                mips: Vec::new(),
                width: s.width,
                height: s.height,
                rgba: Vec::new(),
                mip: s.mip,
                srgb: t.srgb,
            },
            blocks: Some(crate::Blocks {
                format,
                levels: chosen.len() as u32,
                bytes,
                opaque,
            }),
        }))
    })
}
impl Builder<'_, '_> {
    fn resolve_path(&self, c: Citation, raw: &[u8], b: &Budget) -> Result<Vec<u8>> {
        let r = self.loader.assets.resolve_path(c, raw, b)?;
        if r.hypotheses.iter().any(|h| {
            (*h == fo4_scene::hypotheses::PATH_STRIP && !self.allow_path_strip)
                || (*h == fo4_scene::hypotheses::PATH_ROOT && !self.allow_path_root)
                || (*h == fo4_scene::hypotheses::PATH_DATA_RELATIVE && !self.allow_path_relative)
        }) {
            return Err(error(
                "UnverifiedRenderPathNormalization",
                "citation",
                0,
                fo4_core::HexBytes(raw),
            ));
        }
        Ok(r.key)
    }
    fn asset(&self, kind: AssetKind, path: &[u8]) -> Result<usize> {
        self.loader
            .assets
            .nodes
            .iter()
            .position(|a| a.kind == kind && a.path == path)
            .ok_or_else(|| {
                error(
                    "RenderAssetNotInScene",
                    &fo4_core::HexBytes(path).to_string(),
                    0,
                    format!("{kind:?}"),
                )
            })
    }
    /// The draw-list texture of `path`; `alpha_tested` selects the block
    /// limit when textures are kept as blocks, and is ignored otherwise.
    fn texture(&mut self, path: Vec<u8>, alpha_tested: bool) -> Result<usize> {
        let cap = self.options.block_dimension(alpha_tested);
        if let Some(i) = self.textures.get(&(path.clone(), cap.unwrap_or(0))) {
            return Ok(*i);
        }
        let id = self.asset(AssetKind::Texture, &path)?;
        let kept = match (cap, self.options.blocks) {
            (Some(cap), Some(blocks)) => block_texture(
                self.loader,
                id,
                cap,
                blocks.max_bytes,
                &mut self.block_bytes,
                self.budget,
            )?,
            _ => None,
        };
        let loaded = match kept {
            Some(t) => t,
            None => decode_texture(
                self.loader,
                id,
                self.options,
                &mut self.texture_bytes,
                self.limits,
                self.budget,
            )?,
        };
        let (texture, blocks, declared_dxgi, declared_fourcc) = (
            loaded.texture,
            loaded.blocks,
            loaded.declared_dxgi,
            loaded.declared_fourcc,
        );
        match &blocks {
            Some(b) => texture.validate_blocks(b)?,
            None => texture.validate()?,
        }
        let index = self.output.list.textures.len();
        self.budget
            .charge(path.len() as u64 * 3 + 256, "texture identity", 0)?;
        append(
            &mut self.output.texture_choices,
            TextureChoice {
                path: fo4_core::HexBytes(&path).to_string(),
                width: texture.width,
                height: texture.height,
                mip: texture.mip,
                srgb: texture.srgb,
                declared_dxgi,
                declared_fourcc: Some(declared_fourcc),
                color_declaration: if declared_dxgi.is_none() {
                    "legacy_dds_no_srgb_declaration"
                } else if texture.srgb {
                    "dxgi_srgb"
                } else {
                    "dxgi_non_srgb"
                },
                coarse_mips: texture
                    .mips
                    .iter()
                    .map(|m| (m.mip, m.width, m.height))
                    .collect(),
            },
            self.budget,
        )?;
        append(&mut self.output.list.textures, texture, self.budget)?;
        if let Some(b) = blocks {
            self.budget.charge(64, "block texture", 0)?;
            self.output.list.blocks.insert(index, b);
        }
        self.textures.insert((path, cap.unwrap_or(0)), index);
        Ok(index)
    }
    fn constant_effect(&mut self, rgba: [f32; 4], file: &str, offset: u64) -> Result<usize> {
        if !self.allow_effect_color || rgba.iter().any(|v| !v.is_finite()) {
            return Err(error(
                "UnverifiedEffectColorOnly",
                file,
                offset,
                "finite declared color and registered hypothesis required",
            ));
        }
        let total = fo4_core::add(self.texture_bytes, 16, file, offset)?;
        fo4_core::limit(total, self.options.max_texture_bytes, file, offset)?;
        let mut pixels = Vec::new();
        self.budget.reserve(&mut pixels, 4, file, offset)?;
        pixels.extend_from_slice(&rgba);
        self.budget.charge(file.len() as u64 + 128, file, offset)?;
        let index = self.output.list.textures.len();
        append(
            &mut self.output.list.textures,
            Texture {
                mips: Vec::new(),
                width: 1,
                height: 1,
                rgba: pixels,
                mip: 0,
                srgb: false,
            },
            self.budget,
        )?;
        append(
            &mut self.output.texture_choices,
            TextureChoice {
                path: format!("computed declared effect {file}@{offset}"),
                width: 1,
                height: 1,
                mip: 0,
                srgb: false,
                declared_dxgi: None,
                declared_fourcc: None,
                color_declaration: "computed_effect_no_dds_no_transfer_declaration",
                coarse_mips: Vec::new(),
            },
            self.budget,
        )?;
        self.texture_bytes = total;
        Ok(index)
    }
    fn swap(
        &self,
        path: Vec<u8>,
        swap: Option<&Loaded<Mswp>>,
        scratch: &Budget,
    ) -> Result<Vec<u8>> {
        let Some(swap) = swap else {
            return Ok(path);
        };
        let mut from = None;
        let mut selected = None;
        for entry in &swap.data.entries {
            match (&entry.signature, &entry.value) {
                (b"BNAM", SwapValue::Path(p)) => {
                    if from.is_some() {
                        return Err(error("RenderSwapPair", "material swap", 0, "unpaired BNAM"));
                    }
                    from =
                        Some(self.resolve_path(Citation::SwapMaterial, p.as_bytes(), scratch)?);
                }
                (b"SNAM", SwapValue::Path(p)) => {
                    let source = from.take().ok_or_else(|| {
                        error("RenderSwapPair", "material swap", 0, "SNAM without BNAM")
                    })?;
                    if source == path {
                        if selected.is_some() {
                            return Err(error(
                                "AmbiguousMaterialSwap",
                                "material swap",
                                0,
                                "duplicate source",
                            ));
                        }
                        selected = Some(self.resolve_path(
                            Citation::SwapMaterial,
                            p.as_bytes(),
                            scratch,
                        )?);
                    }
                }
                // Color remap is retained in Scene. This unlit texture-only backend does not apply tints.
                (b"CNAM", SwapValue::Scalar(_)) => {}
                // An editor folder label under `mswp_fnam_folder_label_v1`.
                (b"FNAM", SwapValue::Path(_)) if self.allow_swap_folder => {}
                _ => {
                    return Err(error(
                        "UnsupportedRenderSwapField",
                        "material swap",
                        0,
                        String::from_utf8_lossy(&entry.signature),
                    ))
                }
            }
        }
        if from.is_some() {
            return Err(error("RenderSwapPair", "material swap", 0, "unpaired BNAM"));
        }
        Ok(selected.unwrap_or(path))
    }
    fn surface(
        &mut self,
        n: &Nif<'_>,
        shape: &[Field],
        swap: Option<&Loaded<Mswp>>,
        file: &str,
    ) -> Result<Surface> {
        let scratch = self.budget.child();
        let mut shaders = shape.iter().filter_map(|f| {
            if let Value::Ref(i) = f.value {
                if i >= 0 {
                    let bl = n.blocks.get(i as usize)?;
                    let ty = n.block_types.get(bl.type_index as usize)?;
                    if matches!(
                        ty.as_str(),
                        "BSLightingShaderProperty" | "BSEffectShaderProperty"
                    ) {
                        Some(i as usize)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        });
        let shader = shaders
            .next()
            .ok_or_else(|| error("MissingRenderShader", file, 0, "absent"))?;
        if shaders.next().is_some() {
            return Err(error("AmbiguousRenderShader", file, 0, shader));
        }
        let f = fields(decoded(n, shader, file)?);
        let name = f
            .iter()
            .find_map(|f| {
                if f.name == "Name" {
                    if let Value::StringIndex(i) = f.value {
                        Some(i)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .ok_or_else(|| error("MissingShaderNameField", file, 0, shader))?;
        let material = n
            .strings
            .get(name as usize)
            .filter(|s| !s.is_empty())
            .filter(|s| {
                s[s.len().saturating_sub(5)..].eq_ignore_ascii_case(b".bgsm")
                    || s[s.len().saturating_sub(5)..].eq_ignore_ascii_case(b".bgem")
            });
        let nif_alpha = ni_alpha(n, shape, file)?;
        let nif_flags1 = f.iter().find_map(|v| match (v.name, &v.value) {
            ("Shader Flags 1", Value::U32(x)) => Some(*x),
            _ => None,
        });
        let nif_flags2 = f.iter().find_map(|f| {
            if f.name == "Shader Flags 2" {
                if let Value::U32(v) = f.value {
                    Some(v)
                } else {
                    None
                }
            } else {
                None
            }
        });
        if let Some(material) = material {
            let path = self.resolve_path(Citation::NifMaterial, material, &scratch)?;
            let path = self.swap(path, swap, &scratch)?;
            let id = self.asset(AssetKind::Material, &path)?;
            let loader = self.loader;
            return loader.assets.with_decoded(id, &scratch, |d| {
                let DecodedAsset::Material(m) = d else {
                    return Err(error("RenderAssetKind", file, 0, id));
                };
                let (_, diffuse) = m
                    .texture_paths()
                    .find(|(name, _)| matches!(*name, "diffuse" | "base"))
                    .ok_or_else(|| error("MissingDiffuseSlot", file, 0, id))?;
                let color_only = diffuse.bytes().is_empty();
                let texture = if color_only {
                    let fo4_material::Kind::Bgem(e) = &m.kind else {
                        return Err(error(
                            "MissingDiffuseTexture",
                            file,
                            diffuse.offset,
                            "empty lighting diffuse",
                        ));
                    };
                    if e.falloff_enabled.0 != 0
                        || e.falloff_color_enabled.0 != 0
                        || e.soft_enabled.0 != 0
                        || e.blood_enabled.0 != 0
                        || e.effect_lighting_enabled.0 != 0
                        || e.grayscale_to_palette_alpha.0 != 0
                        || m.common.grayscale_to_palette_color.0 != 0
                        || m.common.environment_mapping.is_some_and(|v| v.0 != 0)
                        || m.common.refraction.0 != 0
                        || e.environment_mapping.is_some_and(|v| v.0 != 0)
                        || e.glass_enabled.is_some_and(|v| v.0 != 0)
                        || e.glowmap.is_some_and(|v| v.0 != 0)
                        || e.effect_pbr_specular.is_some_and(|v| v.0 != 0)
                        || e.emittance_color.is_some()
                        || m.texture_paths().any(|(_, p)| !p.bytes().is_empty())
                    {
                        return Err(error(
                            "UnsupportedColorOnlyEffectFeatures",
                            file,
                            diffuse.offset,
                            "active effect feature or secondary texture",
                        ));
                    }
                    self.constant_effect(
                        [
                            e.base_color[0] * e.base_color_scale,
                            e.base_color[1] * e.base_color_scale,
                            e.base_color[2] * e.base_color_scale,
                            1.,
                        ],
                        file,
                        diffuse.offset,
                    )?
                } else {
                    self.texture(
                        self.resolve_path(Citation::MaterialTexture, diffuse.bytes(), &scratch)?,
                        m.common.alpha_test.0 != 0,
                    )?
                };
                let c = &m.common;
                let alpha = Alpha {
                    enabled: c.alpha_blend.enabled,
                    source: c.alpha_blend.source,
                    destination: c.alpha_blend.destination,
                    test_enabled: c.alpha_test.0,
                    test_ref: c.alpha_test_ref,
                    nif_test_function: None,
                    opacity: c.alpha,
                    nif_flags: nif_alpha.map(|a| a.0),
                };
                self.opacity_rule(&alpha)?;
                alpha.validate()?;
                Ok(Surface {
                    requires_uv: !color_only,
                    channels: if color_only {
                        effect_channels(nif_flags1, nif_flags2, file)?
                    } else {
                        0
                    },
                    texture,
                    alpha,
                    offset: [c.u_offset as f64, c.v_offset as f64],
                    scale: [c.u_scale as f64, c.v_scale as f64],
                    tile: Some(c.tile_flags),
                    sidedness: crate::Sidedness {
                        material_two_sided: Some(c.two_sided.0),
                        nif_shader_flags_2: nif_flags2,
                    },
                })
            });
        }
        if swap.is_some() {
            return Err(error("RenderSwapWithoutMaterial", file, 0, shader));
        }
        let kind = &n.block_types[n.blocks[shader].type_index as usize];
        let raw = if kind == "BSLightingShaderProperty" {
            let set = raw_ref(f, "Texture Set")
                .filter(|i| *i >= 0)
                .ok_or_else(|| error("MissingRenderTextureSet", file, 0, shader))?;
            fields(decoded(n, set as usize, file)?)
                .iter()
                .find_map(|f| {
                    if f.name == "Texture" {
                        if let Value::Bytes(v) = &f.value {
                            Some(v.as_slice())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
        } else {
            f.iter().find_map(|f| {
                if f.name == "Source Texture" {
                    if let Value::Bytes(v) = &f.value {
                        Some(v.as_slice())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        }
        .ok_or_else(|| error("MissingDiffuseTexture", file, 0, shader))?;
        let color_only = raw.is_empty();
        let texture = if color_only {
            if kind != "BSEffectShaderProperty" {
                return Err(error(
                    "MissingNifDiffuseTexture",
                    file,
                    n.blocks[shader].data.start as u64,
                    "empty lighting texture",
                ));
            }
            let flags = nif_flags1.ok_or_else(|| error("MissingEffectFlags", file, 0, shader))?;
            let unsupported = (1 << 2)
                | (1 << 4)
                | (1 << 5)
                | (1 << 6)
                | (1 << 7)
                | (1 << 8)
                | (1 << 15)
                | (1 << 16)
                | (1 << 30);
            if flags & unsupported != 0
                || f.iter().any(|f| {
                    f.name.ends_with("Texture")
                        && matches!(&f.value, Value::Bytes(p) if !p.is_empty())
                })
            {
                return Err(error(
                    "UnsupportedColorOnlyEffectFeatures",
                    file,
                    n.blocks[shader].data.start as u64,
                    flags,
                ));
            }
            let c = floats::<4>(f, "Base Color", file)?;
            let scale = floats::<1>(f, "Base Color Scale", file)?[0];
            self.constant_effect(
                [
                    (c[0] * scale) as f32,
                    (c[1] * scale) as f32,
                    (c[2] * scale) as f32,
                    1.,
                ],
                file,
                n.blocks[shader].data.start as u64,
            )?
        } else {
            self.texture(
                self.resolve_path(Citation::NifTexture, raw, &scratch)?,
                nif_alpha.is_some_and(|(flags, _)| (flags >> 9) & 1 == 1),
            )?
        };
        let opacity = if kind == "BSLightingShaderProperty" {
            floats::<1>(f, "Alpha", file)?[0] as f32
        } else {
            floats::<4>(f, "Base Color", file)?[3] as f32
        };
        let alpha = match nif_alpha {
            Some((flags, threshold)) => Alpha {
                enabled: (flags & 1) as u8,
                source: ((flags >> 1) & 15) as u32,
                destination: ((flags >> 5) & 15) as u32,
                test_enabled: ((flags >> 9) & 1) as u8,
                test_ref: threshold,
                nif_test_function: Some(((flags >> 10) & 7) as u8),
                opacity,
                nif_flags: Some(flags),
            },
            None => Alpha {
                enabled: 0,
                source: 0,
                destination: 0,
                test_enabled: 0,
                test_ref: 0,
                nif_test_function: None,
                opacity,
                nif_flags: None,
            },
        };
        self.opacity_rule(&alpha)?;
        alpha.validate()?;
        Ok(Surface {
            requires_uv: !color_only,
            channels: if color_only {
                effect_channels(nif_flags1, nif_flags2, file)?
            } else {
                0
            },
            texture,
            alpha,
            offset: floats::<2>(f, "UV Offset", file)?,
            scale: floats::<2>(f, "UV Scale", file)?,
            tile: None,
            sidedness: crate::Sidedness {
                material_two_sided: None,
                nif_shader_flags_2: nif_flags2,
            },
        })
    }
    fn mesh(
        &mut self,
        path: &[u8],
        i: usize,
        m: &fo4_nif::Mesh,
        surface: &Surface,
        skinned: Option<(usize, &[[f64; 3]])>,
    ) -> Result<usize> {
        let model = self.asset(AssetKind::Model, path)?;
        let key = (
            model,
            i,
            surface.channels,
            surface.requires_uv,
            skinned.map(|s| s.0),
        );
        if let Some(index) = self.meshes.get(&key) {
            return Ok(*index);
        }
        if skinned.is_some_and(|(_, p)| m.vertices.as_ref().is_none_or(|v| v.len() != p.len())) {
            return Err(error("RenderSkinnedVertexCount", "mesh", 0, i));
        }
        if m.dynamic_vertices.is_some() {
            return Err(error("UnsupportedRenderDynamicGeometry", "mesh", 0, i));
        }
        let vertices = m
            .vertices
            .as_ref()
            .ok_or_else(|| error("MissingRenderVertices", "mesh", 0, i))?;
        let triangles = m
            .triangles
            .as_ref()
            .ok_or_else(|| error("MissingRenderTriangles", "mesh", 0, i))?;
        let mut mesh = Mesh {
            vertex_colors: None,
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        self.budget
            .reserve(&mut mesh.vertices, vertices.len(), "render vertices", 0)?;
        if surface.channels != 0 {
            let mut colors = Vec::new();
            self.budget
                .reserve(&mut colors, vertices.len(), "effect vertex colors", 0)?;
            for v in vertices {
                let c = v
                    .color
                    .map(|c| c.map(|v| v as f32 / 255.))
                    .or(v.color_float)
                    .ok_or_else(|| error("MissingEnabledEffectVertexColor", "mesh", 0, i))?;
                if c.iter().any(|v| !v.is_finite()) {
                    return Err(error("RenderNonFiniteVertexColor", "mesh", 0, i));
                }
                colors.push(std::array::from_fn(|j| {
                    if (j < 3 && surface.channels & 1 != 0) || (j == 3 && surface.channels & 2 != 0)
                    {
                        c[j]
                    } else {
                        1.
                    }
                }));
            }
            mesh.vertex_colors = Some(colors);
        }
        for (k, v) in vertices.iter().enumerate() {
            let position = match skinned {
                Some((_, p)) => p[k],
                None => v
                    .position
                    .ok_or_else(|| error("MissingRenderPosition", "mesh", 0, i))?
                    .map(f64::from),
            };
            // Constant surfaces do not sample UV; zero is an internal lookup coordinate.
            let uv = if surface.requires_uv {
                v.uv.ok_or_else(|| error("MissingRenderUv", "mesh", 0, i))?
                    .map(f64::from)
            } else {
                [0.; 2]
            };
            if position.iter().chain(&uv).any(|x| !x.is_finite()) {
                return Err(error("RenderNonFiniteVertex", "mesh", 0, i));
            }
            mesh.vertices.push(Vertex { position, uv });
        }
        self.budget
            .reserve(&mut mesh.triangles, triangles.len(), "render indices", 0)?;
        mesh.triangles.extend_from_slice(triangles);
        let index = self.output.list.meshes.len();
        append(&mut self.output.list.meshes, mesh, self.budget)?;
        self.budget
            .charge(path.len() as u64 + 256, "mesh identity", 0)?;
        self.meshes.insert(key, index);
        Ok(index)
    }
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        n: &Nif<'_>,
        path: &[u8],
        i: usize,
        parent: Transform,
        instance: u32,
        swap: Option<&Loaded<Mswp>>,
        active: &mut Vec<usize>,
        depth: u64,
    ) -> Result<()> {
        let file = std::str::from_utf8(path).map_err(|e| {
            error(
                "RenderPathEncoding",
                &fo4_core::HexBytes(path).to_string(),
                0,
                e,
            )
        })?;
        fo4_core::limit(depth, self.limits.max_depth, "render node", 0)?;
        self.visits = fo4_core::add(self.visits, 1, "render visits", 0)?;
        fo4_core::limit(self.visits, self.limits.max_entries, "render visits", 0)?;
        if active.contains(&i) {
            return Err(error("RenderNodeCycle", file, 0, i));
        }
        let block = n
            .blocks
            .get(i)
            .ok_or_else(|| error("RenderNodeIndex", file, 0, i))?;
        if !self.include_marker_subtrees && fo4_scene::model_policy::visibility_editor_marker(n, i)
        {
            if !self.allow_visibility_marker {
                return Err(error(
                    "UnverifiedVisibilityEditorMarker",
                    file,
                    block.data.start as u64,
                    "registered helper visibility hypothesis required",
                ));
            }
            self.output.skipped_editor_marker_subtrees = fo4_core::add(
                self.output.skipped_editor_marker_subtrees,
                1,
                file,
                block.data.start as u64,
            )?;
            return Ok(());
        }
        let marker_suffix = fo4_scene::model_policy::editor_marker_suffix(n, i);
        if !self.include_marker_subtrees
            && (fo4_scene::model_policy::editor_marker_object(n, i) || marker_suffix)
        {
            if marker_suffix && !self.allow_marker_suffix {
                return Err(error(
                    "UnverifiedNifEditorMarkerSuffixVisibility",
                    file,
                    block.data.start as u64,
                    "numeric label suffix needs registered hypothesis",
                ));
            }
            if !self.allow_editor_marker_subtrees {
                return Err(error(
                    "UnverifiedNifEditorMarkerVisibility",
                    file,
                    block.data.start as u64,
                    "named subtree needs registered visibility hypothesis",
                ));
            }
            self.output.skipped_editor_marker_subtrees = fo4_core::add(
                self.output.skipped_editor_marker_subtrees,
                1,
                file,
                block.data.start as u64,
            )?;
            return Ok(());
        }
        let ty = &n.block_types[block.type_index as usize];
        if matches!(
            block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return Ok(());
        }
        let data = decoded(n, i, file)?;
        let f = fields(data);
        let world = if f.iter().any(|f| f.name == "Translation") {
            let local = Transform::from_fields(f, file)?;
            // A root node's own transform is left out, the placement standing
            // in its place, only under `nif_root_transform_replaced_v1`; at
            // the other levels it composes as the schema's node transform.
            if depth == 0
                && !local.is_identity()
                && self.draw_rule(fo4_scene::hypotheses::ROOT_TRANSFORM)?
            {
                parent
            } else {
                parent.compose(local)
            }
        } else {
            parent
        };
        append(active, i, self.budget)?;
        let result = (|| {
            if let Data::Node { children, .. } = data {
                for c in children.iter().filter(|i| **i >= 0) {
                    self.node(
                        n,
                        path,
                        *c as usize,
                        world,
                        instance,
                        swap,
                        active,
                        depth + 1,
                    )?;
                }
                return Ok(());
            }
            let (geometry, mesh_index) = match data {
                Data::Mesh(m) => (Some(m), i),
                _ => {
                    if let Some(index) = raw_ref(f, "Data").filter(|i| *i >= 0) {
                        match decoded(n, index as usize, file)? {
                            Data::Mesh(m) => (Some(m), index as usize),
                            _ => (None, i),
                        }
                    } else {
                        (None, i)
                    }
                }
            };
            if let Some(mesh) = geometry {
                let surface = self.surface(n, f, swap, file)?;
                let mesh = self.mesh(path, mesh_index, mesh, &surface, None)?;
                append(
                    &mut self.output.list.draws,
                    Draw {
                        sidedness: surface.sidedness,
                        instance,
                        mesh,
                        texture: surface.texture,
                        world,
                        alpha: surface.alpha,
                        uv_offset: surface.offset,
                        uv_scale: surface.scale,
                        raw_tile_flags: surface.tile,
                    },
                    self.budget,
                )?;
            }
            Ok(())
        })();
        active.pop();
        result
    }
    fn base(
        &mut self,
        base: &Base,
        world: Transform,
        instance: u32,
        inherited: Option<&Loaded<Mswp>>,
        depth: u64,
    ) -> Result<()> {
        fo4_core::limit(depth, self.limits.max_depth, "render base", 0)?;
        if base.editor_marker {
            return Ok(());
        }
        if base.material_swap.is_some() && inherited.is_some() {
            return Err(error(
                "UnsupportedRenderSwapScopes",
                &base.record.source.file,
                base.record.source.offset,
                "base and reference/parent swaps both present",
            ));
        }
        let swap = base.material_swap.as_ref().or(inherited);
        let collection_parts = base.record.signature == *b"SCOL" && !base.parts.is_empty();
        if collection_parts && !self.allow_scol_parts {
            return Err(error(
                "UnverifiedScolGeometrySource",
                &base.record.source.file,
                base.record.source.offset,
                "constituent geometry requires registered hypothesis",
            ));
        }
        if !collection_parts {
            for id in &base.assets {
                if self.loader.assets.nodes[*id].kind != AssetKind::Model {
                    continue;
                }
                let scratch = self.budget.child();
                let loader = self.loader;
                let path = &loader.assets.nodes[*id].path;
                loader.assets.with_decoded(*id, &scratch, |d| {
                    let DecodedAsset::Model(n) = d else {
                        return Err(error("RenderAssetKind", "model", 0, id));
                    };
                    let mut active = Vec::new();
                    for root in n.roots.iter().filter(|r| **r >= 0) {
                        self.node(
                            &n,
                            path,
                            *root as usize,
                            world,
                            instance,
                            swap,
                            &mut active,
                            0,
                        )?;
                    }
                    Ok(())
                })?;
            }
        }
        for part in &base.parts {
            let resolved = part
                .resolved
                .as_ref()
                .ok_or_else(|| error("MissingRenderScolBase", "SCOL", 0, base.key.object_id))?;
            let placements = part.declared.placements.as_ref().ok_or_else(|| {
                error("MissingRenderScolPlacements", "SCOL", 0, base.key.object_id)
            })?;
            for p in placements {
                let local = Transform::euler([p[0], p[1], p[2], p[3], p[4], p[5]], p[6])?;
                self.base(resolved, world.compose(local), instance, swap, depth + 1)?;
            }
        }
        Ok(())
    }
}
impl Builder<'_, '_> {
    /// Whether a registered rule that the draw itself needs (not one of the
    /// instance's state) may be used at the build's level; a used rule is
    /// recorded for the instance.
    fn draw_rule(&mut self, rule: &'static str) -> Result<bool> {
        use fo4_scene::hypotheses::{status, Status};
        let usable = match status(rule) {
            Some(Status::Supported) => self.options.level != fo4_scene::RuleLevel::Facts,
            Some(Status::Pending) => self.options.level == fo4_scene::RuleLevel::Hypotheses,
            Some(Status::Refuted) | None => false,
        };
        if usable && !self.draw_rules.contains(&rule) {
            append(&mut self.draw_rules, rule, self.budget)?;
        }
        Ok(usable)
    }
    /// Adds the rules the current instance's draws relied on to its
    /// identity, the last one pushed.
    fn record_draw_rules(&mut self) -> Result<()> {
        if self.draw_rules.is_empty() {
            return Ok(());
        }
        let Some(id) = self.output.instances.last_mut() else {
            return Ok(());
        };
        for rule in &self.draw_rules {
            if !id.hypotheses.contains(rule) {
                self.budget
                    .reserve(&mut id.hypotheses, 1, "draw hypotheses", 0)?;
                id.hypotheses.push(rule);
            }
        }
        id.hypotheses.sort_unstable();
        Ok(())
    }
    /// An alpha whose opacity is above 1 on a surface without blending is
    /// drawn only under `bgsm_opacity_above_one_test_v1`.
    fn opacity_rule(&mut self, alpha: &Alpha) -> Result<()> {
        if alpha.enabled == 0
            && alpha.opacity.is_finite()
            && alpha.opacity > 1.
            && !self.draw_rule(fo4_scene::hypotheses::OPACITY_ABOVE_ONE)?
        {
            return Err(Error::new(
                "UnsupportedRenderAlpha",
                "draw list",
                0,
                "opacity at most 1 without a registered rule",
                format!("{alpha:?}"),
            ));
        }
        Ok(())
    }
    /// Sets the rule flags the shared walkers read from a state's rules.
    fn allow(&mut self, rules: &[&'static str]) {
        use fo4_scene::hypotheses as h;
        self.allow_effect_color = rules.contains(&h::EFFECT_COLOR);
        self.allow_path_strip = rules.contains(&h::PATH_STRIP);
        self.allow_path_root = rules.contains(&h::PATH_ROOT);
        self.allow_path_relative = rules.contains(&h::PATH_DATA_RELATIVE);
        self.allow_visibility_marker = rules.contains(&h::VISIBILITY_MARKER);
        self.allow_marker_suffix = rules.contains(&h::MARKER_SUFFIX);
        self.allow_swap_folder = rules.contains(&h::SWAP_FOLDER);
        self.allow_editor_marker_subtrees = rules.contains(&h::NAMED_MARKER);
        self.allow_scol_parts = rules.contains(&h::SCOL);
    }
    /// The rest pose of skeleton asset `id`, read once.
    fn skeleton(&mut self, id: usize) -> Result<()> {
        if self.skeletons.contains_key(&id) {
            return Ok(());
        }
        let loader = self.loader;
        let path = &loader.assets.nodes[id].path;
        let file = String::from_utf8_lossy(path).into_owned();
        let pose = loader.assets.with_decoded(id, &self.budget.child(), |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(error("RenderAssetKind", "skeleton", 0, id));
            };
            fo4_scene::actor::pose::Skeleton::read(&n, &file, self.limits, self.budget)
        })?;
        self.skeletons.insert(id, pose);
        Ok(())
    }
    /// One node of an actor model: skinned shapes are posed by `skeleton`
    /// and drawn at `world`; shapes without a skin are counted, not drawn.
    #[allow(clippy::too_many_arguments)]
    fn actor_node(
        &mut self,
        n: &Nif<'_>,
        path: &[u8],
        i: usize,
        world: Transform,
        instance: u32,
        skeleton: usize,
        active: &mut Vec<usize>,
        depth: u64,
    ) -> Result<()> {
        let file = String::from_utf8_lossy(path).into_owned();
        fo4_core::limit(depth, self.limits.max_depth, "render node", 0)?;
        self.visits = fo4_core::add(self.visits, 1, "render visits", 0)?;
        fo4_core::limit(self.visits, self.limits.max_entries, "render visits", 0)?;
        if active.contains(&i) {
            return Err(error("RenderNodeCycle", &file, 0, i));
        }
        let block = n
            .blocks
            .get(i)
            .ok_or_else(|| error("RenderNodeIndex", &file, 0, i))?;
        let at = block.data.start as u64;
        let marker = fo4_scene::model_policy::visibility_editor_marker(n, i);
        let suffix = fo4_scene::model_policy::editor_marker_suffix(n, i);
        let named = fo4_scene::model_policy::editor_marker_object(n, i);
        if !self.include_marker_subtrees && (marker || suffix || named) {
            let allowed = (!marker || self.allow_visibility_marker)
                && (!suffix || self.allow_marker_suffix)
                && (!named || self.allow_editor_marker_subtrees);
            if !allowed {
                return Err(error(
                    "UnverifiedActorModelMarker",
                    &file,
                    at,
                    "registered helper visibility rule",
                ));
            }
            self.output.skipped_editor_marker_subtrees =
                fo4_core::add(self.output.skipped_editor_marker_subtrees, 1, &file, at)?;
            return Ok(());
        }
        let ty = &n.block_types[block.type_index as usize];
        if matches!(
            block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return Ok(());
        }
        let data = decoded(n, i, &file)?;
        append(active, i, self.budget)?;
        let result = (|| {
            match data {
                Data::Node { children, .. } => {
                    for c in children.iter().filter(|c| **c >= 0) {
                        self.actor_node(
                            n,
                            path,
                            *c as usize,
                            world,
                            instance,
                            skeleton,
                            active,
                            depth + 1,
                        )?;
                    }
                }
                Data::Mesh(m) => {
                    let f = fields(data);
                    let Some(skin) =
                        fo4_nif::skin::shape_skin(n, i, &file, self.limits, self.budget)?
                    else {
                        self.output.actor_rigid_shapes =
                            fo4_core::add(self.output.actor_rigid_shapes, 1, &file, at)?;
                        return Ok(());
                    };
                    // 23,367 of 23,368 skinned shapes measured have exactly the
                    // identity as their own transform; whether another one
                    // applies is not established, so such a shape is refused.
                    let own = fo4_scene::actor::pose::Affine::from_fields(f, &file)?;
                    if own.is_some_and(|a| a != fo4_scene::actor::pose::Affine::IDENTITY) {
                        return Err(error(
                            "ActorSkinnedShapeTransform",
                            &file,
                            at,
                            "identity shape transform",
                        ));
                    }
                    let pose = &self.skeletons[&skeleton];
                    let matrices =
                        fo4_scene::actor::pose::bone_matrices(n, &skin, pose, &file, self.budget)?;
                    let positions = fo4_scene::actor::pose::skin_positions(
                        n,
                        i,
                        &matrices,
                        &file,
                        self.budget,
                    )?;
                    let surface = self.surface(n, f, None, &file)?;
                    let mesh = self.mesh(path, i, m, &surface, Some((skeleton, &positions)))?;
                    if self.posed.contains(&skeleton)
                        && !self.recorded.iter().any(|r| r.mesh == mesh)
                    {
                        let shape = fo4_scene::actor::pose::SkinnedShape::read(
                            n,
                            i,
                            &skin,
                            &file,
                            self.budget,
                        )?;
                        append(
                            &mut self.recorded,
                            SkinnedMesh {
                                mesh,
                                skeleton,
                                shape,
                            },
                            self.budget,
                        )?;
                    }
                    append(
                        &mut self.output.list.draws,
                        Draw {
                            sidedness: surface.sidedness,
                            instance,
                            mesh,
                            texture: surface.texture,
                            world,
                            alpha: surface.alpha,
                            uv_offset: surface.offset,
                            uv_scale: surface.scale,
                            raw_tile_flags: surface.tile,
                        },
                        self.budget,
                    )?;
                }
                Data::Fields(_) => {}
            }
            Ok(())
        })();
        active.pop();
        result
    }
    /// Draws one resolved actor as instance `index`.
    fn actor(&mut self, a: &fo4_scene::actor::Actor, index: u32) -> Result<()> {
        let src = &a.source;
        if !a.state.ready_at(self.options.level) {
            let why = if a.plan.is_none() {
                a.notes.join("; ")
            } else {
                format!("{:?} at rule level {:?}", a.state, self.options.level)
            };
            return Err(error("ActorNotDrawable", &src.file, src.offset, why));
        }
        if a.disabled_by_enable_parent() {
            return Err(error(
                "DisabledThroughEnableParent",
                &src.file,
                src.offset,
                fo4_scene::hypotheses::ENABLE_FOLLOWS,
            ));
        }
        let plan = a.plan.as_ref().ok_or_else(|| {
            error(
                "ActorNotDrawable",
                &src.file,
                src.offset,
                a.notes.join("; "),
            )
        })?;
        let p = a.transform.ok_or_else(|| {
            error(
                "MissingRenderTransform",
                &src.file,
                src.offset,
                "ACHR DATA absent",
            )
        })?;
        // Absent XSCL is 1 under achr_placement_as_refr_v1, which the state holds.
        let world = Transform::euler(p, a.scale.unwrap_or(1.))?;
        self.allow(a.state.hypotheses());
        self.skeleton(plan.skeleton_asset)?;
        let first = self.output.list.draws.len();
        for part in &plan.parts {
            self.actor_model(part.asset, world, index, plan.skeleton_asset)?;
        }
        if self.output.list.draws.len() == first {
            return Err(error(
                "NoDrawableGeometry",
                &src.file,
                src.offset,
                a.reference.object_id,
            ));
        }
        if let Some(head) = &plan.head {
            self.actor_head(a, head, world, index, plan.skeleton_asset)?;
        }
        Ok(())
    }
    /// Draws the skinned shapes of one actor model.
    fn actor_model(
        &mut self,
        asset: usize,
        world: Transform,
        index: u32,
        skeleton: usize,
    ) -> Result<()> {
        let scratch = self.budget.child();
        let loader = self.loader;
        let path = &loader.assets.nodes[asset].path;
        loader.assets.with_decoded(asset, &scratch, |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(error("RenderAssetKind", "actor model", 0, asset));
            };
            let mut active = Vec::new();
            for root in n.roots.iter().filter(|r| **r >= 0) {
                self.actor_node(
                    &n,
                    path,
                    *root as usize,
                    world,
                    index,
                    skeleton,
                    &mut active,
                    0,
                )?;
            }
            Ok(())
        })
    }
    /// Draws an actor's head parts (`npc_head_parts_unmorphed_v1`). A head
    /// not ready at the build's level, and each part that fails, is an
    /// entry of `Build::actor_head_omissions`; the body stays drawn.
    fn actor_head(
        &mut self,
        a: &fo4_scene::actor::Actor,
        head: &fo4_scene::actor::Head,
        world: Transform,
        index: u32,
        skeleton: usize,
    ) -> Result<()> {
        let mut causes = Vec::new();
        if !head.state.ready_at(self.options.level) {
            let src = &a.source;
            append(
                &mut causes,
                error(
                    "ActorHeadNotDrawable",
                    &src.file,
                    src.offset,
                    format!("{:?} at rule level {:?}", head.state, self.options.level),
                ),
                self.budget,
            )?;
            for e in head
                .blockers
                .iter()
                .filter(|x| fo4_scene::hypotheses::covering(x).is_none())
            {
                append(&mut causes, e.clone(), self.budget)?;
            }
        } else {
            self.allow(head.state.hypotheses());
            for part in &head.parts {
                let first = self.output.list.draws.len();
                if let Err(e) = self.actor_model(part.asset, world, index, skeleton) {
                    self.output.list.draws.truncate(first);
                    append(&mut causes, e, self.budget)?;
                }
            }
        }
        if !causes.is_empty() {
            append(
                &mut self.output.actor_head_omissions,
                Omission {
                    instance: index,
                    reference: a.reference.clone(),
                    causes,
                },
                self.budget,
            )?;
        }
        Ok(())
    }
}
pub fn build(
    scene: &Scene,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<Build> {
    build_with_marker_visibility(scene, loader, limits, options, false, b)
}
/// [`build`], then the scene's placed actors as further instances, in the
/// order of `actors` (`fo4_scene::actor::resolve`). An actor that cannot be
/// drawn at the options' level is an omission with its reasons.
pub fn build_with_actors(
    scene: &Scene,
    actors: &fo4_scene::actor::Actors,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<Build> {
    build_inner(scene, Some(actors), None, loader, limits, options, false, b).map(|x| x.0)
}
/// [`build_with_actors`], with the actors whose skeleton asset is a key of
/// `poses` skinned in that pose instead of the skeleton's rest pose; their
/// skinned meshes are returned with the build so that they can be posed
/// again. With no pose given, the draw list is that of
/// [`build_with_actors`].
#[allow(clippy::too_many_arguments)]
pub fn build_with_posed_actors(
    scene: &Scene,
    actors: &fo4_scene::actor::Actors,
    poses: BTreeMap<usize, fo4_scene::actor::pose::Skeleton>,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<(Build, Vec<SkinnedMesh>)> {
    build_inner(
        scene,
        Some(actors),
        Some(poses),
        loader,
        limits,
        options,
        false,
        b,
    )
}
/// Explicit diagnostic comparison. `include_marker_subtrees` retains named marker
/// geometry; it does not change the registered visibility policy or base flags.
pub fn build_with_marker_visibility(
    scene: &Scene,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    include_marker_subtrees: bool,
    b: &Budget,
) -> Result<Build> {
    build_inner(
        scene,
        None,
        None,
        loader,
        limits,
        options,
        include_marker_subtrees,
        b,
    )
    .map(|x| x.0)
}
#[allow(clippy::too_many_arguments)]
fn build_inner(
    scene: &Scene,
    actors: Option<&fo4_scene::actor::Actors>,
    poses: Option<BTreeMap<usize, fo4_scene::actor::pose::Skeleton>>,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    include_marker_subtrees: bool,
    b: &Budget,
) -> Result<(Build, Vec<SkinnedMesh>)> {
    if options.max_texture_dimension == 0
        || options.max_texture_bytes == 0
        || options.blocks.is_some_and(|b| {
            b.max_dimension == 0 || b.max_alpha_tested_dimension == 0 || b.max_bytes == 0
        })
    {
        return Err(error(
            "InvalidDrawOptions",
            "draw list",
            0,
            options.max_texture_dimension,
        ));
    }
    let mut builder = Builder {
        loader,
        limits,
        budget: b,
        options,
        output: Build {
            skipped_editor_marker_subtrees: 0,
            actors: Vec::new(),
            actor_rigid_shapes: 0,
            actor_head_omissions: Vec::new(),
            list: DrawList::default(),
            instances: Vec::new(),
            omitted: Vec::new(),
            texture_choices: Vec::new(),
        },
        meshes: BTreeMap::new(),
        posed: poses
            .as_ref()
            .map(|p| p.keys().copied().collect())
            .unwrap_or_default(),
        skeletons: poses.unwrap_or_default(),
        recorded: Vec::new(),
        textures: BTreeMap::new(),
        texture_bytes: 0,
        block_bytes: 0,
        visits: 0,
        allow_path_strip: false,
        allow_path_root: false,
        allow_path_relative: false,
        allow_effect_color: false,
        allow_scol_parts: false,
        allow_editor_marker_subtrees: false,
        allow_marker_suffix: false,
        include_marker_subtrees,
        allow_visibility_marker: false,
        allow_swap_folder: false,
        draw_rules: Vec::new(),
    };
    let scratch = b.child();
    let mut instances = Vec::new();
    scratch.reserve(
        &mut instances,
        scene.instances.len(),
        "draw instance order",
        0,
    )?;
    instances.extend(scene.instances.iter());
    instances.sort_unstable_by(|a, b| {
        (&a.reference.key.plugin, a.reference.key.object_id)
            .cmp(&(&b.reference.key.plugin, b.reference.key.object_id))
    });
    for (index, i) in instances.into_iter().enumerate() {
        let index =
            u32::try_from(index).map_err(|e| error("RenderInstanceLimit", "scene", 0, e))?;
        b.charge(
            (i.reference.key.plugin.len() + i.base.as_ref().map_or(0, |v| v.key.plugin.len()))
                as u64
                + 128,
            "draw identity",
            0,
        )?;
        let mut hypotheses = Vec::new();
        b.reserve(
            &mut hypotheses,
            i.state.hypotheses().len(),
            "draw hypotheses",
            0,
        )?;
        hypotheses.extend_from_slice(i.state.hypotheses());
        append(
            &mut builder.output.instances,
            Identity {
                index,
                reference: i.reference.key.clone(),
                base: i.base.as_ref().map(|v| v.key.clone()),
                hypotheses,
                editor_marker: i.editor_marker,
            },
            b,
        )?;
        builder.allow_effect_color = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::EFFECT_COLOR);
        let first = builder.output.list.draws.len();
        builder.draw_rules.clear();
        let src = &i.reference.record.source;
        let attempt = (|| {
            if i.editor_marker {
                return Err(error(
                    "EditorMarkerHiddenUnderHypothesis",
                    &src.file,
                    src.offset,
                    fo4_scene::hypotheses::MARKER,
                ));
            }
            if !i.state.ready_at(options.level) {
                return Err(error(
                    "SceneInstanceNotDrawable",
                    &src.file,
                    src.offset,
                    format!("{:?} at rule level {:?}", i.state, options.level),
                ));
            }
            // Ready only at the hypotheses level when it has an XESP: the
            // enable parent rules say whether it starts enabled.
            if i.disabled_by_enable_parent() {
                return Err(error(
                    "DisabledThroughEnableParent",
                    &src.file,
                    src.offset,
                    fo4_scene::hypotheses::ENABLE_FOLLOWS,
                ));
            }
            let base = i
                .base
                .as_ref()
                .ok_or_else(|| error("MissingRenderBase", &src.file, src.offset, index))?;
            let p = i.reference.data.transform.ok_or_else(|| {
                error(
                    "MissingRenderTransform",
                    &src.file,
                    src.offset,
                    "DATA absent",
                )
            })?;
            // No guessed optional-field default. The bench authors XSCL explicitly.
            let scale = match i.reference.data.scale {
                Some(scale) => scale,
                None if i.state.hypotheses().contains(&fo4_scene::hypotheses::SCALE) => 1.,
                None => {
                    return Err(error(
                        "MissingDocumentedReferenceScale",
                        &src.file,
                        src.offset,
                        "XSCL absent without registered hypothesis",
                    ))
                }
            };
            builder.allow_path_strip = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::PATH_STRIP);
            builder.allow_path_root = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::PATH_ROOT);
            builder.allow_path_relative = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::PATH_DATA_RELATIVE);
            builder.allow_visibility_marker = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::VISIBILITY_MARKER);
            builder.allow_marker_suffix = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::MARKER_SUFFIX);
            builder.allow_editor_marker_subtrees = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::NAMED_MARKER);
            builder.allow_scol_parts = i.state.hypotheses().contains(&fo4_scene::hypotheses::SCOL);
            builder.allow_swap_folder = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::SWAP_FOLDER);
            let world = Transform::euler(p, scale)?;
            builder.base(base, world, index, i.material_swap.as_ref(), 0)?;
            if builder.output.list.draws.len() == first {
                return Err(error(
                    "NoDrawableGeometry",
                    &src.file,
                    src.offset,
                    base.key.object_id,
                ));
            }
            Ok(())
        })();
        if attempt.is_ok() {
            builder.record_draw_rules()?;
        }
        if let Err(e) = attempt {
            builder.output.list.draws.truncate(first);
            let mut causes = Vec::new();
            for e in std::iter::once(&e).chain(i.blockers.iter()) {
                b.charge(
                    (e.code.len() + e.expected.len() + e.found.len() + e.source.file.len() + 256)
                        as u64,
                    "draw omission",
                    0,
                )?;
                append(&mut causes, e.clone(), b)?;
            }
            append(
                &mut builder.output.omitted,
                Omission {
                    instance: index,
                    reference: i.reference.key.clone(),
                    causes,
                },
                b,
            )?;
        }
    }
    let Some(actors) = actors else {
        return Ok((builder.output, builder.recorded));
    };
    let mut index = u32::try_from(builder.output.instances.len())
        .map_err(|e| error("RenderInstanceLimit", "scene", 0, e))?;
    for a in &actors.actors {
        b.charge(
            (a.reference.plugin.len() + a.base.as_ref().map_or(0, |v| v.plugin.len())) as u64 + 128,
            "draw identity",
            0,
        )?;
        let mut hypotheses = Vec::new();
        b.reserve(
            &mut hypotheses,
            a.state.hypotheses().len(),
            "draw hypotheses",
            0,
        )?;
        hypotheses.extend_from_slice(a.state.hypotheses());
        append(
            &mut builder.output.instances,
            Identity {
                index,
                reference: a.reference.clone(),
                base: a.base.clone(),
                hypotheses,
                editor_marker: false,
            },
            b,
        )?;
        append(&mut builder.output.actors, index, b)?;
        let first = builder.output.list.draws.len();
        builder.draw_rules.clear();
        let attempt = builder.actor(a, index);
        if attempt.is_ok() {
            builder.record_draw_rules()?;
        }
        if let Err(e) = attempt {
            builder.output.list.draws.truncate(first);
            let mut causes = Vec::new();
            for e in std::iter::once(&e).chain(
                a.blockers
                    .iter()
                    .filter(|x| fo4_scene::hypotheses::covering(x).is_none()),
            ) {
                b.charge(
                    (e.code.len() + e.expected.len() + e.found.len() + e.source.file.len() + 256)
                        as u64,
                    "draw omission",
                    0,
                )?;
                append(&mut causes, e.clone(), b)?;
            }
            append(
                &mut builder.output.omitted,
                Omission {
                    instance: index,
                    reference: a.reference.clone(),
                    causes,
                },
                b,
            )?;
        }
        index = index
            .checked_add(1)
            .ok_or_else(|| error("RenderInstanceLimit", "scene", 0, index))?;
    }
    Ok((builder.output, builder.recorded))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoded_alpha_property_fields_keep_the_raw_flags_and_threshold() {
        // Own single-block NIF builder; exercise the actual decoder's field names.
        let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        v.extend(0x14020007u32.to_le_bytes());
        v.push(1);
        for x in [12u32, 1, 130] {
            v.extend(x.to_le_bytes());
        }
        v.extend([0; 4]);
        v.extend(1u16.to_le_bytes());
        v.extend(15u32.to_le_bytes());
        v.extend(b"NiAlphaProperty");
        v.extend(0u16.to_le_bytes());
        v.extend(15u32.to_le_bytes());
        for x in [0u32, 0, 0] {
            v.extend(x.to_le_bytes());
        }
        for x in [u32::MAX, 0, u32::MAX] {
            v.extend(x.to_le_bytes());
        }
        let flags = 0x12edu16;
        v.extend(flags.to_le_bytes());
        v.push(129);
        v.extend(1u32.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        let b = Budget::new(1 << 20);
        let n = Nif::read(
            &v,
            "synthetic-alpha.nif",
            fo4_core::Game::Fallout4,
            &Limits::default(),
            &b,
        )
        .unwrap();
        let shape = [Field {
            name: "Alpha Property",
            offset: 0,
            value: Value::Ref(0),
        }];
        assert_eq!(
            ni_alpha(&n, &shape, "synthetic-alpha.nif").unwrap(),
            Some((flags, 129))
        );
    }
}
