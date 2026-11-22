//! Effect-shader surfaces of models whose effect controllers are settled at
//! rest (`nif_controller_rest_effect_first_key_v1`), under the effect
//! surface rules of `spec/effect-surfaces.md`: the texture (or a white
//! constant) times the base colour and its scale, the vertex colours where
//! the flags settle them, the base colour's alpha as opacity and the alpha
//! property's (or the material's) blend state. Every term the data does not
//! settle refuses the surface with a named error.
use super::{error, floats, Builder, Surface};
use crate::Alpha;
use fo4_core::Result;
use fo4_nif::{Field, Nif, Value};
use fo4_plugin::typed::{Loaded, Mswp};
use fo4_scene::{hypotheses as h, paths::Citation, rest, AssetKind, DecodedAsset};

/// Shader Flags 1 bits (nifxml Fallout4ShaderPropertyFlags1) whose terms
/// this frame does not draw: Temp_Refraction (2), GreyscaleToPalette_Color
/// (4), GreyscaleToPalette_Alpha (5), Use_Falloff (6), Environment_Mapping
/// (7), RGB_Falloff (8), Refraction (15), Fire_Refraction (16).
const PALETTE: u32 = 1 << 4 | 1 << 5;
const FALLOFF: u32 = 1 << 6 | 1 << 8;
const FEATURES: u32 = 1 << 2 | 1 << 7 | 1 << 15 | 1 << 16;
/// Vertex_Alpha (Shader Flags 1 bit 3), Vertex_Colors (Shader Flags 2 bit 5).
const VERTEX_ALPHA: u32 = 1 << 3;
const VERTEX_COLORS: u32 = 1 << 5;
/// Soft_Effect (flags 1 bit 30), Effect_Lighting (flags 2 bit 30).
const SOFT: u32 = 1 << 30;
const LIGHTING: u32 = 1 << 30;

/// What the frame takes from an effect shader and the material it names.
struct Terms {
    palette: bool,
    falloff: bool,
    feature: bool,
    left_out: bool,
    color: [f32; 3],
    scale: f32,
    alpha: Alpha,
    texture: Option<Vec<u8>>,
    alpha_tested: bool,
    offset: [f64; 2],
    uv_scale: [f64; 2],
    tile: Option<u32>,
    two_sided: Option<u8>,
}

fn byte(f: &[Field], name: &str) -> Option<u8> {
    f.iter().find_map(|x| match x.value {
        Value::U8(v) if x.name == name => Some(v),
        _ => None,
    })
}
fn text<'f>(f: &'f [Field], name: &str) -> Option<&'f [u8]> {
    f.iter().find_map(|x| match &x.value {
        Value::Bytes(v) if x.name == name => Some(v.as_slice()),
        _ => None,
    })
}
/// The material's value of a field an effect-shader controller drives (the
/// pairing of `rest::effect_field`).
fn material_field(m: &fo4_material::Material<'_>, color: bool, variable: u32) -> Option<Vec<f32>> {
    let fo4_material::Kind::Bgem(e) = &m.kind else {
        return None;
    };
    let c = &m.common;
    Some(match (color, variable) {
        (true, 0) => e.base_color.to_vec(),
        (false, 0) => vec![e.base_color_scale],
        (false, 1) => vec![e.falloff_start_angle],
        (false, 2) => vec![e.falloff_stop_angle],
        (false, 3) => vec![e.falloff_start_opacity],
        (false, 4) => vec![e.falloff_stop_opacity],
        (false, 5) => vec![c.alpha],
        (false, 6) => vec![c.u_offset],
        (false, 7) => vec![c.u_scale],
        (false, 8) => vec![c.v_offset],
        (false, 9) => vec![c.v_scale],
        _ => return None,
    })
}

/// Which vertex channels an effect surface takes (bit 0 RGB, bit 1 alpha),
/// or why the data does not settle it. RGB follows Vertex_Colors. Alpha:
/// with both flags set, or with neither, the readings "alpha follows
/// Vertex_Alpha" and "alpha follows Vertex_Colors" agree; with one flag
/// alone they differ unless every vertex alpha is 1.
pub(super) fn vertex_channels(flags1: u32, flags2: u32) -> (u8, bool) {
    let colors = flags2 & VERTEX_COLORS != 0;
    let alpha = flags1 & VERTEX_ALPHA != 0;
    match (colors, alpha) {
        (true, true) => (3, false),
        (false, false) => (0, false),
        (true, false) => (1, true),
        (false, true) => (0, true),
    }
}

impl Builder<'_, '_> {
    /// The effect surface of a shape whose model relies on
    /// `nif_controller_rest_effect_first_key_v1`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn effect_surface(
        &mut self,
        n: &Nif<'_>,
        shader: usize,
        f: &[Field],
        material: Option<&[u8]>,
        nif_alpha: Option<(u16, u8)>,
        flags: (Option<u32>, Option<u32>),
        swap: &[&Loaded<Mswp>],
        file: &str,
        geometry: Option<&fo4_nif::Mesh>,
    ) -> Result<Surface> {
        let at = n.blocks.get(shader).map_or(0, |b| b.data.start as u64);
        let flags1 = flags
            .0
            .ok_or_else(|| error("MissingEffectFlags", file, at, "Shader Flags 1"))?;
        let flags2 = flags
            .1
            .ok_or_else(|| error("MissingEffectFlags", file, at, "Shader Flags 2"))?;
        let scratch = self.budget.child();
        let terms = match material {
            Some(material) => {
                let path = self.resolve_path(Citation::NifMaterial, material, &scratch)?;
                let path = self.swap(path, swap, &scratch)?;
                let id = self.asset(AssetKind::Material, &path)?;
                let loader = self.loader;
                loader.assets.with_decoded(id, &scratch, |d| {
                    let DecodedAsset::Material(m) = d else {
                        return Err(error("RenderAssetKind", file, 0, id));
                    };
                    let fo4_material::Kind::Bgem(e) = &m.kind else {
                        return Err(error("UnsupportedEffectMaterialKind", file, at, m.magic()));
                    };
                    // The rest rule compared the NIF's stored fields with
                    // the keys; the frame draws the material's.
                    for (color, variable) in rest::effect_controlled(n, shader) {
                        let nif = rest::effect_field(f, color, variable);
                        let own = material_field(&m, color, variable);
                        let same = match (&nif, &own) {
                            (Some(a), Some(b)) => {
                                a.len() == b.len()
                                    && a.iter()
                                        .zip(b)
                                        .all(|(x, y)| (x - y).abs() <= rest::EFFECT_TOLERANCE)
                            }
                            _ => false,
                        };
                        if !same {
                            return Err(error(
                                "UnsettledEffectMaterialField",
                                file,
                                at,
                                format!("color={color} variable={variable}"),
                            ));
                        }
                    }
                    let c = &m.common;
                    let base = e.base_texture.bytes();
                    Ok(Terms {
                        palette: e.grayscale_to_palette_alpha.0 != 0
                            || c.grayscale_to_palette_color.0 != 0,
                        falloff: e.falloff_enabled.0 != 0 || e.falloff_color_enabled.0 != 0,
                        feature: c.refraction.0 != 0
                            || c.environment_mapping.is_some_and(|v| v.0 != 0)
                            || e.environment_mapping.is_some_and(|v| v.0 != 0)
                            || e.glass_enabled.is_some_and(|v| v.0 != 0)
                            || e.glowmap.is_some_and(|v| v.0 != 0)
                            || e.effect_pbr_specular.is_some_and(|v| v.0 != 0)
                            || e.blood_enabled.0 != 0
                            || e.emittance_color.is_some(),
                        left_out: e.soft_enabled.0 != 0
                            || e.effect_lighting_enabled.0 != 0
                            || e.lighting_influence != 1.,
                        color: e.base_color,
                        scale: e.base_color_scale,
                        alpha: Alpha {
                            enabled: c.alpha_blend.enabled,
                            source: c.alpha_blend.source,
                            destination: c.alpha_blend.destination,
                            test_enabled: c.alpha_test.0,
                            test_ref: c.alpha_test_ref,
                            nif_test_function: None,
                            opacity: c.alpha,
                            nif_flags: nif_alpha.map(|a| a.0),
                        },
                        texture: if base.is_empty() {
                            None
                        } else {
                            Some(self.resolve_path(Citation::MaterialTexture, base, &scratch)?)
                        },
                        alpha_tested: c.alpha_test.0 != 0,
                        offset: [c.u_offset as f64, c.v_offset as f64],
                        uv_scale: [c.u_scale as f64, c.v_scale as f64],
                        tile: Some(c.tile_flags),
                        two_sided: Some(c.two_sided.0),
                    })
                })?
            }
            None => {
                if !swap.is_empty() {
                    return Err(error("RenderSwapWithoutMaterial", file, at, shader));
                }
                let base = floats::<4>(f, "Base Color", file)?;
                let scale = floats::<1>(f, "Base Color Scale", file)?[0];
                let source = text(f, "Source Texture")
                    .ok_or_else(|| error("MissingDiffuseTexture", file, at, shader))?;
                let alpha = match nif_alpha {
                    Some((flags, threshold)) => Alpha {
                        enabled: (flags & 1) as u8,
                        source: ((flags >> 1) & 15) as u32,
                        destination: ((flags >> 5) & 15) as u32,
                        test_enabled: ((flags >> 9) & 1) as u8,
                        test_ref: threshold,
                        nif_test_function: Some(((flags >> 10) & 7) as u8),
                        opacity: base[3] as f32,
                        nif_flags: Some(flags),
                    },
                    None => Alpha {
                        enabled: 0,
                        source: 0,
                        destination: 0,
                        test_enabled: 0,
                        test_ref: 0,
                        nif_test_function: None,
                        opacity: base[3] as f32,
                        nif_flags: None,
                    },
                };
                Terms {
                    palette: flags1 & PALETTE != 0,
                    falloff: flags1 & FALLOFF != 0,
                    feature: flags1 & FEATURES != 0,
                    left_out: flags1 & SOFT != 0
                        || flags2 & LIGHTING != 0
                        || byte(f, "Lighting Influence") != Some(255),
                    color: [base[0] as f32, base[1] as f32, base[2] as f32],
                    scale: scale as f32,
                    alpha_tested: alpha.test_enabled != 0,
                    alpha,
                    texture: if source.is_empty() {
                        None
                    } else {
                        Some(self.resolve_path(Citation::NifTexture, source, &scratch)?)
                    },
                    offset: floats::<2>(f, "UV Offset", file)?,
                    uv_scale: floats::<2>(f, "UV Scale", file)?,
                    tile: None,
                    two_sided: None,
                }
            }
        };
        // Every term this frame does not settle, named together.
        let tint: [f32; 3] = std::array::from_fn(|i| terms.color[i] * terms.scale);
        let (channels, alpha_gate) = vertex_channels(flags1, flags2);
        let vertex_alpha_below_one = geometry
            .and_then(|m| m.vertices.as_ref())
            .is_some_and(|vs| {
                vs.iter().any(|v| {
                    v.color
                        .map(|c| c[3] != 255)
                        .or(v.color_float.map(|c| c[3] != 1.))
                        .unwrap_or(false)
                })
            });
        let gates: Vec<&str> = [
            (terms.palette, "palette"),
            (terms.feature, "feature"),
            (terms.falloff, "falloff"),
            (
                tint.iter()
                    .any(|v| !v.is_finite() || !(0. ..=1.).contains(v)),
                "colour-range",
            ),
            (
                alpha_gate && vertex_alpha_below_one,
                "vertex-alpha-one-flag",
            ),
        ]
        .into_iter()
        .filter_map(|(failed, name)| failed.then_some(name))
        .collect();
        if !gates.is_empty() {
            return Err(error("UnsupportedEffectSurface", file, at, gates.join("+")));
        }
        self.opacity_rule(&terms.alpha)?;
        terms.alpha.validate()?;
        for rule in [h::EFFECT_SHADING]
            .into_iter()
            .chain((terms.alpha.enabled != 0).then_some(h::EFFECT_BLEND))
            .chain((terms.alpha.enabled != 0).then_some(h::EFFECT_ORDER))
            .chain(terms.left_out.then_some(h::EFFECT_LEFT_OUT))
        {
            if !self.draw_rule(rule)? {
                return Err(error("UnverifiedEffectSurface", file, at, rule));
            }
        }
        let requires_uv = terms.texture.is_some();
        let texture = match terms.texture {
            Some(path) => self.texture(path, terms.alpha_tested)?,
            None => self.constant_texture([1.; 4], file, at)?,
        };
        Ok(Surface {
            channels,
            requires_uv,
            texture,
            alpha: terms.alpha,
            offset: terms.offset,
            scale: terms.uv_scale,
            tile: terms.tile,
            sidedness: crate::Sidedness {
                material_two_sided: terms.two_sided,
                nif_shader_flags_2: Some(flags2),
            },
            tint: Some(tint),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::vertex_channels;
    #[test]
    fn vertex_channels_are_settled_where_both_readings_agree() {
        // (Shader Flags 1, Shader Flags 2) -> (channels, alpha gate).
        assert_eq!(vertex_channels(1 << 3, 1 << 5), (3, false));
        assert_eq!(vertex_channels(0, 0), (0, false));
        assert_eq!(vertex_channels(0, 1 << 5), (1, true));
        assert_eq!(vertex_channels(1 << 3, 0), (0, true));
    }
}
