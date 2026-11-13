use crate::{Path, Reader, Terrain, WireBool};
use fo4_core::Result;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Bgsm<'a> {
    pub diffuse_texture: Path<'a>,
    pub normal_texture: Path<'a>,
    pub smooth_spec_texture: Path<'a>,
    pub greyscale_texture: Path<'a>,
    pub glow_texture: Option<Path<'a>>,
    pub wrinkles_texture: Option<Path<'a>>,
    pub specular_texture: Option<Path<'a>>,
    pub lighting_texture: Option<Path<'a>>,
    pub flow_texture: Option<Path<'a>>,
    pub distance_field_alpha_texture: Option<Path<'a>>,
    pub envmap_texture: Option<Path<'a>>,
    pub glow_texture_legacy: Option<Path<'a>>,
    pub inner_layer_texture: Option<Path<'a>>,
    pub wrinkles_texture_legacy: Option<Path<'a>>,
    pub displacement_texture: Option<Path<'a>>,
    pub enable_editor_alpha_ref: WireBool,
    pub translucency: Option<WireBool>,
    pub translucency_thick_object: Option<WireBool>,
    pub translucency_mix_albedo_with_subsurface_color: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub translucency_subsurface_color: Option<[f32; 3]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub translucency_transmissive_scale: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub translucency_turbulence: Option<f32>,
    pub rim_lighting: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub rim_power: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub back_light_power: Option<f32>,
    pub subsurface_lighting: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub subsurface_lighting_rolloff: Option<f32>,
    pub specular_enabled: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub specular_color: [f32; 3],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub specular_mult: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub smoothness: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub fresnel_power: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_spec_scale: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_spec_power_scale: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_spec_minvar: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_env_map_scale: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_fresnel_power: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub wetness_control_metalness: f32,
    pub pbr: Option<WireBool>,
    pub custom_porosity: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub porosity_value: Option<f32>,
    pub root_material_path: Path<'a>,
    pub aniso_lighting: WireBool,
    pub emit_enabled: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub emittance_color: Option<[f32; 3]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub emittance_mult: f32,
    pub model_space_normals: WireBool,
    pub external_emittance: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub lum_emittance: Option<f32>,
    pub use_adaptative_emissive: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_exposure_offset: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_final_exposure_min: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_final_exposure_max: Option<f32>,
    pub back_lighting: Option<WireBool>,
    pub receive_shadows: WireBool,
    pub hide_secret: WireBool,
    pub cast_shadows: WireBool,
    pub dissolve_fade: WireBool,
    pub assume_shadowmask: WireBool,
    pub glowmap: WireBool,
    pub environment_mapping_window: Option<WireBool>,
    pub environment_mapping_eye: Option<WireBool>,
    pub hair: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub hair_tint_color: [f32; 3],
    pub tree: WireBool,
    pub facegen: WireBool,
    pub skin_tint: WireBool,
    pub tessellate: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub displacement_texture_bias: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub displacement_texture_scale: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub tessellation_pn_scale: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub tessellation_base_factor: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub tessellation_fade_distance: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub grayscale_to_palette_scale: f32,
    pub skew_specular_alpha: Option<WireBool>,
    pub terrain: Option<WireBool>,
    pub terrain_data: Option<Terrain>,
}
impl<'a> Bgsm<'a> {
    pub(crate) fn read(r: &mut Reader<'_, '_, 'a>) -> Result<Self> {
        let diffuse_texture = r.path()?;
        let normal_texture = r.path()?;
        let smooth_spec_texture = r.path()?;
        let greyscale_texture = r.path()?;
        let glow_texture = r.optional(r.version > 2, |r| r.path())?;
        let wrinkles_texture = r.optional(r.version > 2, |r| r.path())?;
        let specular_texture = r.optional(r.version > 2, |r| r.path())?;
        let lighting_texture = r.optional(r.version > 2, |r| r.path())?;
        let flow_texture = r.optional(r.version > 2, |r| r.path())?;
        let distance_field_alpha_texture = r.optional(r.version >= 17, |r| r.path())?;
        let envmap_texture = r.optional(r.version <= 2, |r| r.path())?;
        let glow_texture_legacy = r.optional(r.version <= 2, |r| r.path())?;
        let inner_layer_texture = r.optional(r.version <= 2, |r| r.path())?;
        let wrinkles_texture_legacy = r.optional(r.version <= 2, |r| r.path())?;
        let displacement_texture = r.optional(r.version <= 2, |r| r.path())?;
        let enable_editor_alpha_ref = r.boolean()?;
        let translucency = r.optional(r.version >= 8, |r| r.boolean())?;
        let translucency_thick_object = r.optional(r.version >= 8, |r| r.boolean())?;
        let translucency_mix_albedo_with_subsurface_color =
            r.optional(r.version >= 8, |r| r.boolean())?;
        let translucency_subsurface_color = r.optional(r.version >= 8, |r| r.color())?;
        let translucency_transmissive_scale = r.optional(r.version >= 8, |r| r.float())?;
        let translucency_turbulence = r.optional(r.version >= 8, |r| r.float())?;
        let rim_lighting = r.optional(r.version < 8, |r| r.boolean())?;
        let rim_power = r.optional(r.version < 8, |r| r.float())?;
        let back_light_power = r.optional(r.version < 8, |r| r.float())?;
        let subsurface_lighting = r.optional(r.version < 8, |r| r.boolean())?;
        let subsurface_lighting_rolloff = r.optional(r.version < 8, |r| r.float())?;
        let specular_enabled = r.boolean()?;
        let specular_color = r.color()?;
        let specular_mult = r.float()?;
        let smoothness = r.float()?;
        let fresnel_power = r.float()?;
        let wetness_control_spec_scale = r.float()?;
        let wetness_control_spec_power_scale = r.float()?;
        let wetness_control_spec_minvar = r.float()?;
        let wetness_control_env_map_scale = r.optional(r.version < 10, |r| r.float())?;
        let wetness_control_fresnel_power = r.float()?;
        let wetness_control_metalness = r.float()?;
        let pbr = r.optional(r.version > 2, |r| r.boolean())?;
        let custom_porosity = r.optional(r.version >= 9, |r| r.boolean())?;
        let porosity_value = r.optional(r.version >= 9, |r| r.float())?;
        let root_material_path = r.path()?;
        let aniso_lighting = r.boolean()?;
        let emit_enabled = r.boolean()?;
        let emittance_color = r.optional(emit_enabled.value(), |r| r.color())?;
        let emittance_mult = r.float()?;
        let model_space_normals = r.boolean()?;
        let external_emittance = r.boolean()?;
        let lum_emittance = r.optional(r.version >= 12, |r| r.float())?;
        let use_adaptative_emissive = r.optional(r.version >= 13, |r| r.boolean())?;
        let adaptative_emissive_exposure_offset = r.optional(r.version >= 13, |r| r.float())?;
        let adaptative_emissive_final_exposure_min = r.optional(r.version >= 13, |r| r.float())?;
        let adaptative_emissive_final_exposure_max = r.optional(r.version >= 13, |r| r.float())?;
        let back_lighting = r.optional(r.version < 8, |r| r.boolean())?;
        let receive_shadows = r.boolean()?;
        let hide_secret = r.boolean()?;
        let cast_shadows = r.boolean()?;
        let dissolve_fade = r.boolean()?;
        let assume_shadowmask = r.boolean()?;
        let glowmap = r.boolean()?;
        let environment_mapping_window = r.optional(r.version < 7, |r| r.boolean())?;
        let environment_mapping_eye = r.optional(r.version < 7, |r| r.boolean())?;
        let hair = r.boolean()?;
        let hair_tint_color = r.color()?;
        let tree = r.boolean()?;
        let facegen = r.boolean()?;
        let skin_tint = r.boolean()?;
        let tessellate = r.boolean()?;
        let displacement_texture_bias = r.optional(r.version < 3, |r| r.float())?;
        let displacement_texture_scale = r.optional(r.version < 3, |r| r.float())?;
        let tessellation_pn_scale = r.optional(r.version < 3, |r| r.float())?;
        let tessellation_base_factor = r.optional(r.version < 3, |r| r.float())?;
        let tessellation_fade_distance = r.optional(r.version < 3, |r| r.float())?;
        let grayscale_to_palette_scale = r.float()?;
        let skew_specular_alpha = r.optional(r.version >= 1, |r| r.boolean())?;
        let terrain = r.optional(r.version >= 3, |r| r.boolean())?;
        let terrain_data = r.optional(terrain.is_some_and(|v| v.value()), Terrain::read)?;
        Ok(Self {
            diffuse_texture,
            normal_texture,
            smooth_spec_texture,
            greyscale_texture,
            glow_texture,
            wrinkles_texture,
            specular_texture,
            lighting_texture,
            flow_texture,
            distance_field_alpha_texture,
            envmap_texture,
            glow_texture_legacy,
            inner_layer_texture,
            wrinkles_texture_legacy,
            displacement_texture,
            enable_editor_alpha_ref,
            translucency,
            translucency_thick_object,
            translucency_mix_albedo_with_subsurface_color,
            translucency_subsurface_color,
            translucency_transmissive_scale,
            translucency_turbulence,
            rim_lighting,
            rim_power,
            back_light_power,
            subsurface_lighting,
            subsurface_lighting_rolloff,
            specular_enabled,
            specular_color,
            specular_mult,
            smoothness,
            fresnel_power,
            wetness_control_spec_scale,
            wetness_control_spec_power_scale,
            wetness_control_spec_minvar,
            wetness_control_env_map_scale,
            wetness_control_fresnel_power,
            wetness_control_metalness,
            pbr,
            custom_porosity,
            porosity_value,
            root_material_path,
            aniso_lighting,
            emit_enabled,
            emittance_color,
            emittance_mult,
            model_space_normals,
            external_emittance,
            lum_emittance,
            use_adaptative_emissive,
            adaptative_emissive_exposure_offset,
            adaptative_emissive_final_exposure_min,
            adaptative_emissive_final_exposure_max,
            back_lighting,
            receive_shadows,
            hide_secret,
            cast_shadows,
            dissolve_fade,
            assume_shadowmask,
            glowmap,
            environment_mapping_window,
            environment_mapping_eye,
            hair,
            hair_tint_color,
            tree,
            facegen,
            skin_tint,
            tessellate,
            displacement_texture_bias,
            displacement_texture_scale,
            tessellation_pn_scale,
            tessellation_base_factor,
            tessellation_fade_distance,
            grayscale_to_palette_scale,
            skew_specular_alpha,
            terrain,
            terrain_data,
        })
    }
    pub fn non_finite_values(&self) -> u64 {
        fo4_core::ieee::count(&self.translucency_subsurface_color)
            + fo4_core::ieee::count(&self.translucency_transmissive_scale)
            + fo4_core::ieee::count(&self.translucency_turbulence)
            + fo4_core::ieee::count(&self.rim_power)
            + fo4_core::ieee::count(&self.back_light_power)
            + fo4_core::ieee::count(&self.subsurface_lighting_rolloff)
            + fo4_core::ieee::count(&self.specular_color)
            + fo4_core::ieee::count(&self.specular_mult)
            + fo4_core::ieee::count(&self.smoothness)
            + fo4_core::ieee::count(&self.fresnel_power)
            + fo4_core::ieee::count(&self.wetness_control_spec_scale)
            + fo4_core::ieee::count(&self.wetness_control_spec_power_scale)
            + fo4_core::ieee::count(&self.wetness_control_spec_minvar)
            + fo4_core::ieee::count(&self.wetness_control_env_map_scale)
            + fo4_core::ieee::count(&self.wetness_control_fresnel_power)
            + fo4_core::ieee::count(&self.wetness_control_metalness)
            + fo4_core::ieee::count(&self.porosity_value)
            + fo4_core::ieee::count(&self.emittance_color)
            + fo4_core::ieee::count(&self.emittance_mult)
            + fo4_core::ieee::count(&self.lum_emittance)
            + fo4_core::ieee::count(&self.adaptative_emissive_exposure_offset)
            + fo4_core::ieee::count(&self.adaptative_emissive_final_exposure_min)
            + fo4_core::ieee::count(&self.adaptative_emissive_final_exposure_max)
            + fo4_core::ieee::count(&self.hair_tint_color)
            + fo4_core::ieee::count(&self.displacement_texture_bias)
            + fo4_core::ieee::count(&self.displacement_texture_scale)
            + fo4_core::ieee::count(&self.tessellation_pn_scale)
            + fo4_core::ieee::count(&self.tessellation_base_factor)
            + fo4_core::ieee::count(&self.tessellation_fade_distance)
            + fo4_core::ieee::count(&self.grayscale_to_palette_scale)
    }
}
