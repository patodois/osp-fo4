//! Effect-shader surfaces of models whose effect controllers are settled at
//! rest (`nif_controller_rest_effect_first_key_v1`), under the effect
//! surface rules of `spec/effect-surfaces.md`: the texture (or a white
//! constant) times the base colour and its scale, the vertex colours where
//! the flags settle them or their rule decides, the base colour's alpha as
//! opacity, the falloff by view angle under its rules, and the alpha
//! property's (or the material's) blend state; the environment map's term
//! left out of this unlit frame, and the material's value of a field a
//! controller drives where the material and the NIF disagree, each under its
//! rule. Every term the data does not settle refuses the surface with a
//! named error.
use super::{error, floats, Builder, Surface};
use crate::Alpha;
use fo4_core::Result;
use fo4_nif::{Field, Nif, Value};
use fo4_plugin::typed::{Loaded, Mswp};
use fo4_scene::{hypotheses as h, paths::Citation, rest, AssetKind, DecodedAsset};

/// Shader Flags 1 bits (nifxml Fallout4ShaderPropertyFlags1):
/// GreyscaleToPalette_Color (4), GreyscaleToPalette_Alpha (5), Use_Falloff
/// (6), RGB_Falloff (8), and the features this frame does not draw, by the
/// name a refusal gives them: Temp_Refraction (2), Refraction (15),
/// Fire_Refraction (16).
const PALETTE_COLOUR: u32 = 1 << 4;
const PALETTE_ALPHA: u32 = 1 << 5;
const FALLOFF: u32 = 1 << 6;
const RGB_FALLOFF: u32 = 1 << 8;
const FEATURES: [(u32, &str); 3] = [
    (1 << 2, "temp-refraction"),
    (1 << 15, "refraction"),
    (1 << 16, "fire-refraction"),
];
/// Environment_Mapping (Shader Flags 1 bit 7): drawn without its term
/// under `effect_environment_left_out_v1`.
const ENVIRONMENT: u32 = 1 << 7;
/// Vertex_Alpha (Shader Flags 1 bit 3), Vertex_Colors (Shader Flags 2 bit 5).
const VERTEX_ALPHA: u32 = 1 << 3;
const VERTEX_COLORS: u32 = 1 << 5;
/// Soft_Effect (flags 1 bit 30), Effect_Lighting (flags 2 bit 30).
const SOFT: u32 = 1 << 30;
const LIGHTING: u32 = 1 << 30;

/// What the frame takes from an effect shader and the material it names.
struct Terms {
    palette_colour: bool,
    palette_alpha: bool,
    /// The greyscale (palette) texture, resolved; `None` when the slot is
    /// empty.
    greyscale: Option<Vec<u8>>,
    falloff: bool,
    rgb_falloff: bool,
    /// Falloff Start Angle, Stop Angle, Start Opacity, Stop Opacity.
    falloff_fields: [f32; 4],
    /// Every feature set that the frame does not draw, by name.
    features: Vec<&'static str>,
    /// The environment map flag (the shader's, or the material's).
    environment: bool,
    /// A field an effect-shader controller drives on which the material
    /// and the NIF disagree (`effect_material_over_controller_v1`).
    material_over_controller: bool,
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

/// Whether a driven field's material value differs from the NIF's, for
/// each (colour, variable, NIF value, material value) of the fields the
/// shader's effect controllers drive: `Ok(true)` when the two values of
/// some field differ (the material's is drawn under
/// `effect_material_over_controller_v1`), `Ok(false)` when every pair is
/// equal within the rest rule's tolerance, and the field when either value
/// is missing (no pairing to compare).
pub(super) fn driven_fields_differ(
    pairs: impl IntoIterator<Item = (bool, u32, Option<Vec<f32>>, Option<Vec<f32>>)>,
) -> std::result::Result<bool, (bool, u32)> {
    let mut differ = false;
    for (color, variable, nif, own) in pairs {
        let (Some(a), Some(b)) = (nif, own) else {
            return Err((color, variable));
        };
        if a.len() != b.len() {
            return Err((color, variable));
        }
        differ |= a
            .iter()
            .zip(&b)
            .any(|(x, y)| (x - y).abs() > rest::EFFECT_TOLERANCE);
    }
    Ok(differ)
}
/// What the vertex flags settle of an effect surface's vertex channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VertexAlpha {
    /// Both flags or neither: the readings agree.
    Settled,
    /// Vertex_Colors alone: vertex alpha is read under
    /// `effect_vertex_alpha_under_vertex_colors_v1` where it is below 1.
    UnderVertexColors,
    /// Vertex_Alpha alone: no rule; refused where a vertex alpha is below 1.
    AlphaFlagAlone,
}
/// Which vertex channels an effect surface takes (bit 0 RGB, bit 1 alpha)
/// where its vertex alpha is 1 everywhere, and what the flags settle of
/// the alpha. RGB follows Vertex_Colors. Alpha: with both flags set, or
/// with neither, the readings "alpha follows Vertex_Alpha" and "alpha
/// follows Vertex_Colors" agree; with one flag alone they differ unless
/// every vertex alpha is 1.
pub(super) fn vertex_channels(flags1: u32, flags2: u32) -> (u8, VertexAlpha) {
    let colors = flags2 & VERTEX_COLORS != 0;
    let alpha = flags1 & VERTEX_ALPHA != 0;
    match (colors, alpha) {
        (true, true) => (3, VertexAlpha::Settled),
        (false, false) => (0, VertexAlpha::Settled),
        (true, false) => (1, VertexAlpha::UnderVertexColors),
        (false, true) => (0, VertexAlpha::AlphaFlagAlone),
    }
}
/// Whether every vertex of `geometry` has a normal.
fn normals_on_every_vertex(geometry: Option<&fo4_nif::Mesh>) -> bool {
    geometry
        .and_then(|m| m.vertices.as_ref())
        .is_some_and(|vs| vs.iter().all(|v| v.normal.is_some()))
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
                    // the keys; the frame draws the material's, under its
                    // rule where the two disagree.
                    let material_over_controller =
                        driven_fields_differ(rest::effect_controlled(n, shader).into_iter().map(
                            |(color, variable)| {
                                (
                                    color,
                                    variable,
                                    rest::effect_field(f, color, variable),
                                    material_field(&m, color, variable),
                                )
                            },
                        ))
                        .map_err(|(color, variable)| {
                            error(
                                "UnsettledEffectMaterialField",
                                file,
                                at,
                                format!("color={color} variable={variable}"),
                            )
                        })?;
                    let c = &m.common;
                    let base = e.base_texture.bytes();
                    let grey = e.grayscale_texture.bytes();
                    Ok(Terms {
                        palette_colour: c.grayscale_to_palette_color.0 != 0,
                        palette_alpha: e.grayscale_to_palette_alpha.0 != 0,
                        greyscale: if grey.is_empty() {
                            None
                        } else {
                            Some(self.resolve_path(Citation::MaterialTexture, grey, &scratch)?)
                        },
                        falloff: e.falloff_enabled.0 != 0,
                        rgb_falloff: e.falloff_color_enabled.0 != 0,
                        falloff_fields: [
                            e.falloff_start_angle,
                            e.falloff_stop_angle,
                            e.falloff_start_opacity,
                            e.falloff_stop_opacity,
                        ],
                        environment: c.environment_mapping.is_some_and(|v| v.0 != 0)
                            || e.environment_mapping.is_some_and(|v| v.0 != 0),
                        material_over_controller,
                        features: [
                            (c.refraction.0 != 0, "refraction"),
                            (e.glass_enabled.is_some_and(|v| v.0 != 0), "glass"),
                            (e.glowmap.is_some_and(|v| v.0 != 0), "glow-map"),
                            (
                                e.effect_pbr_specular.is_some_and(|v| v.0 != 0),
                                "pbr-specular",
                            ),
                            (e.blood_enabled.0 != 0, "blood"),
                            (e.emittance_color.is_some(), "emittance-colour"),
                        ]
                        .into_iter()
                        .filter_map(|(on, name)| on.then_some(name))
                        .collect(),
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
                let falloff_fields = [
                    floats::<1>(f, "Falloff Start Angle", file)?[0] as f32,
                    floats::<1>(f, "Falloff Stop Angle", file)?[0] as f32,
                    floats::<1>(f, "Falloff Start Opacity", file)?[0] as f32,
                    floats::<1>(f, "Falloff Stop Opacity", file)?[0] as f32,
                ];
                let grey = text(f, "Greyscale Texture").unwrap_or(b"");
                Terms {
                    palette_colour: flags1 & PALETTE_COLOUR != 0,
                    palette_alpha: flags1 & PALETTE_ALPHA != 0,
                    greyscale: if grey.is_empty() {
                        None
                    } else {
                        Some(self.resolve_path(Citation::NifTexture, grey, &scratch)?)
                    },
                    falloff: flags1 & FALLOFF != 0,
                    rgb_falloff: flags1 & RGB_FALLOFF != 0,
                    falloff_fields,
                    features: FEATURES
                        .iter()
                        .filter(|(bit, _)| flags1 & bit != 0)
                        .map(|(_, name)| *name)
                        .collect(),
                    environment: flags1 & ENVIRONMENT != 0,
                    material_over_controller: false,
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
        let (mut channels, vertex_alpha) = vertex_channels(flags1, flags2);
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
        // Vertex alpha below 1 under Vertex_Colors alone is read under its
        // rule; with every vertex alpha 1 both readings agree.
        let vertex_alpha_rule =
            vertex_alpha == VertexAlpha::UnderVertexColors && vertex_alpha_below_one;
        if vertex_alpha_rule {
            channels = 3;
        }
        // The falloff the frame draws under its rules: the angle fields as
        // cosines, both opacities within 0 to 1, two different angles
        // unless the opacities are equal, a normal on every vertex.
        let [start_angle, stop_angle, start_opacity, stop_opacity] = terms.falloff_fields;
        let falloff = crate::Falloff {
            start_angle,
            stop_angle,
            start_opacity,
            stop_opacity,
        };
        let falloff_drawn = terms.falloff && !terms.rgb_falloff;
        let palette_drawn = terms.palette_colour || terms.palette_alpha;
        let gates: Vec<&str> = [
            (
                palette_drawn && terms.greyscale.is_none(),
                "palette-texture",
            ),
            (terms.rgb_falloff, "rgb-falloff"),
            (
                falloff_drawn && falloff.validate().is_err(),
                "falloff-fields",
            ),
            (
                falloff_drawn && !normals_on_every_vertex(geometry),
                "falloff-normals",
            ),
            (
                tint.iter().any(|v| !v.is_finite() || *v < 0.),
                "colour-negative",
            ),
            (
                vertex_alpha == VertexAlpha::AlphaFlagAlone && vertex_alpha_below_one,
                "vertex-alpha-flag-alone",
            ),
        ]
        .into_iter()
        .filter_map(|(failed, name)| failed.then_some(name))
        .chain(terms.features.iter().copied())
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
            .chain(vertex_alpha_rule.then_some(h::EFFECT_VERTEX_ALPHA_COLORS))
            .chain(
                [
                    h::EFFECT_FALLOFF,
                    h::EFFECT_FALLOFF_ABSOLUTE,
                    h::EFFECT_FALLOFF_LINEAR,
                    h::EFFECT_FALLOFF_ALPHA,
                ]
                .into_iter()
                .filter(|_| falloff_drawn),
            )
            .chain(
                [h::EFFECT_PALETTE_SAMPLED, h::EFFECT_PALETTE_V]
                    .into_iter()
                    .filter(|_| palette_drawn),
            )
            .chain(
                [h::EFFECT_PALETTE_COLOUR_U, h::EFFECT_PALETTE_COLOUR_RGB]
                    .into_iter()
                    .filter(|_| terms.palette_colour),
            )
            .chain(
                [h::EFFECT_PALETTE_ALPHA_U, h::EFFECT_PALETTE_ALPHA_REPLACES]
                    .into_iter()
                    .filter(|_| terms.palette_alpha),
            )
            .chain(
                tint.iter()
                    .any(|v| *v > 1.)
                    .then_some(h::EFFECT_COLOUR_CLAMPED),
            )
            .chain(
                [h::EFFECT_ENVIRONMENT_ADDS, h::EFFECT_ENVIRONMENT_LEFT_OUT]
                    .into_iter()
                    .filter(|_| terms.environment),
            )
            .chain(
                terms
                    .material_over_controller
                    .then_some(h::EFFECT_MATERIAL_OVER_CONTROLLER),
            )
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
        let palette = match terms.greyscale.filter(|_| palette_drawn) {
            Some(path) => Some(crate::Palette {
                texture: self.decoded_texture(path)?,
                colour: terms.palette_colour,
                alpha: terms.palette_alpha,
            }),
            None => None,
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
            falloff: falloff_drawn.then_some(falloff),
            palette,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{driven_fields_differ, vertex_channels, VertexAlpha};
    #[test]
    fn a_driven_field_the_material_and_the_nif_disagree_on_needs_its_rule() {
        let pair = |nif: f32, own: f32| (false, 5, Some(vec![nif]), Some(vec![own]));
        assert_eq!(driven_fields_differ([]), Ok(false));
        assert_eq!(driven_fields_differ([pair(1., 1.)]), Ok(false));
        assert_eq!(
            driven_fields_differ([pair(1., 1.), pair(1., 0.25)]),
            Ok(true)
        );
        // A field without a pairing is still refused by name.
        assert_eq!(
            driven_fields_differ([pair(1., 0.25), (false, 10, Some(vec![1.]), None)]),
            Err((false, 10))
        );
        assert_eq!(
            driven_fields_differ([(true, 0, Some(vec![1.; 3]), Some(vec![1.]))]),
            Err((true, 0))
        );
    }
    #[test]
    fn vertex_channels_are_settled_where_both_readings_agree() {
        // (Shader Flags 1, Shader Flags 2) -> (channels, vertex alpha).
        assert_eq!(vertex_channels(1 << 3, 1 << 5), (3, VertexAlpha::Settled));
        assert_eq!(vertex_channels(0, 0), (0, VertexAlpha::Settled));
        assert_eq!(
            vertex_channels(0, 1 << 5),
            (1, VertexAlpha::UnderVertexColors)
        );
        assert_eq!(vertex_channels(1 << 3, 0), (0, VertexAlpha::AlphaFlagAlone));
    }
}
