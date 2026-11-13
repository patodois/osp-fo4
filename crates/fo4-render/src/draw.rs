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
    pub list: DrawList,
    pub instances: Vec<Identity>,
    pub omitted: Vec<Omission>,
    pub texture_choices: Vec<TextureChoice>,
}
#[derive(Clone, Copy)]
pub struct BuildOptions {
    pub max_texture_dimension: u32,
    pub max_texture_bytes: u64,
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
    meshes: BTreeMap<(usize, usize, u8, bool), usize>,
    textures: BTreeMap<Vec<u8>, usize>,
    texture_bytes: u64,
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
    fn texture(&mut self, path: Vec<u8>) -> Result<usize> {
        if let Some(i) = self.textures.get(&path) {
            return Ok(*i);
        }
        let id = self.asset(AssetKind::Texture, &path)?;
        let scratch = self.budget.child();
        let loader = self.loader;
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
                        && s.width <= self.options.max_texture_dimension
                        && s.height <= self.options.max_texture_dimension
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
                        self.options.max_texture_dimension,
                    )
                })?;
            let s = &t.surfaces[index];
            let bytes = fo4_core::mul(
                fo4_core::mul(s.width as u64, s.height as u64, "texture", 0)?,
                16,
                "texture",
                0,
            )?;
            let total = fo4_core::add(self.texture_bytes, bytes, "diffuse texture budget", 0)?;
            fo4_core::limit(
                total,
                self.options.max_texture_bytes,
                "diffuse texture budget",
                0,
            )?;
            let rgba = t.decode(index, self.limits, self.budget)?;
            self.texture_bytes = total;
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
                let total = fo4_core::add(self.texture_bytes, size, "mip budget", 0)?;
                if total > self.options.max_texture_bytes {
                    break;
                }
                let rgba = t.decode(i, self.limits, self.budget)?;
                append(
                    &mut mips,
                    crate::Mip {
                        width: level.width,
                        height: level.height,
                        rgba,
                        mip: level.mip,
                    },
                    self.budget,
                )?;
                self.texture_bytes = total;
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
        texture.validate()?;
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
        self.textures.insert(path, index);
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
                    self.texture(self.resolve_path(
                        Citation::MaterialTexture,
                        diffuse.bytes(),
                        &scratch,
                    )?)?
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
            self.texture(self.resolve_path(Citation::NifTexture, raw, &scratch)?)?
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
    ) -> Result<usize> {
        let model = self.asset(AssetKind::Model, path)?;
        if let Some(index) = self
            .meshes
            .get(&(model, i, surface.channels, surface.requires_uv))
        {
            return Ok(*index);
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
        for v in vertices {
            let position = v
                .position
                .ok_or_else(|| error("MissingRenderPosition", "mesh", 0, i))?
                .map(f64::from);
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
        self.meshes
            .insert((model, i, surface.channels, surface.requires_uv), index);
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
            fo4_plugin::still::Class::Outside | fo4_plugin::still::Class::State
        ) {
            return Ok(());
        }
        let data = decoded(n, i, file)?;
        let f = fields(data);
        let world = if f.iter().any(|f| f.name == "Translation") {
            parent.compose(Transform::from_fields(f, file)?)
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
                let mesh = self.mesh(path, mesh_index, mesh, &surface)?;
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
pub fn build(
    scene: &Scene,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<Build> {
    build_with_marker_visibility(scene, loader, limits, options, false, b)
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
    if options.max_texture_dimension == 0 || options.max_texture_bytes == 0 {
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
            list: DrawList::default(),
            instances: Vec::new(),
            omitted: Vec::new(),
            texture_choices: Vec::new(),
        },
        meshes: BTreeMap::new(),
        textures: BTreeMap::new(),
        texture_bytes: 0,
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
            if !i.state.drawable_readiness() {
                return Err(error(
                    "SceneInstanceNotDrawable",
                    &src.file,
                    src.offset,
                    format!("{:?}", i.state),
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
    Ok(builder.output)
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
