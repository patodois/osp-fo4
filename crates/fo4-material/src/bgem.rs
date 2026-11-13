use crate::{Glass, Path, Reader, WireBool};
use fo4_core::Result;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Bgem<'a> {
    pub base_texture: Path<'a>,
    pub grayscale_texture: Path<'a>,
    pub envmap_texture: Path<'a>,
    pub normal_texture: Path<'a>,
    pub envmap_mask_texture: Path<'a>,
    pub specular_texture: Option<Path<'a>>,
    pub lighting_texture: Option<Path<'a>>,
    pub glow_texture: Option<Path<'a>>,
    pub glass_roughness_scratch: Option<Path<'a>>,
    pub glass_dirt_overlay: Option<Path<'a>>,
    pub glass_enabled: Option<WireBool>,
    pub glass: Option<Glass<'a>>,
    pub environment_mapping: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub environment_mapping_mask_scale: Option<f32>,
    pub blood_enabled: WireBool,
    pub effect_lighting_enabled: WireBool,
    pub falloff_enabled: WireBool,
    pub falloff_color_enabled: WireBool,
    pub grayscale_to_palette_alpha: WireBool,
    pub soft_enabled: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub base_color: [f32; 3],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub base_color_scale: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub falloff_start_angle: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub falloff_stop_angle: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub falloff_start_opacity: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub falloff_stop_opacity: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub lighting_influence: f32,
    pub envmap_min_lod: u8,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub soft_depth: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub emittance_color: Option<[f32; 3]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_exposure_offset: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_final_exposure_min: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub adaptative_emissive_final_exposure_max: Option<f32>,
    pub glowmap: Option<WireBool>,
    pub effect_pbr_specular: Option<WireBool>,
}
impl<'a> Bgem<'a> {
    pub(crate) fn read(r: &mut Reader<'_, '_, 'a>) -> Result<Self> {
        let base_texture = r.path()?;
        let grayscale_texture = r.path()?;
        let envmap_texture = r.path()?;
        let normal_texture = r.path()?;
        let envmap_mask_texture = r.path()?;
        let specular_texture = r.optional(r.version >= 11, |r| r.path())?;
        let lighting_texture = r.optional(r.version >= 11, |r| r.path())?;
        let glow_texture = r.optional(r.version >= 11, |r| r.path())?;
        let glass_roughness_scratch = r.optional(r.version >= 21, |r| r.path())?;
        let glass_dirt_overlay = r.optional(r.version >= 21, |r| r.path())?;
        let glass_enabled = r.optional(r.version >= 21, |r| r.boolean())?;
        let glass = r.optional(glass_enabled.is_some_and(|v| v.value()), Glass::read)?;
        let environment_mapping = r.optional(r.version >= 10, |r| r.boolean())?;
        let environment_mapping_mask_scale = r.optional(r.version >= 10, |r| r.float())?;
        let blood_enabled = r.boolean()?;
        let effect_lighting_enabled = r.boolean()?;
        let falloff_enabled = r.boolean()?;
        let falloff_color_enabled = r.boolean()?;
        let grayscale_to_palette_alpha = r.boolean()?;
        let soft_enabled = r.boolean()?;
        let base_color = r.color()?;
        let base_color_scale = r.float()?;
        let falloff_start_angle = r.float()?;
        let falloff_stop_angle = r.float()?;
        let falloff_start_opacity = r.float()?;
        let falloff_stop_opacity = r.float()?;
        let lighting_influence = r.float()?;
        let envmap_min_lod = r.byte()?;
        let soft_depth = r.float()?;
        let emittance_color = r.optional(r.version >= 11, |r| r.color())?;
        let adaptative_emissive_exposure_offset = r.optional(r.version >= 15, |r| r.float())?;
        let adaptative_emissive_final_exposure_min = r.optional(r.version >= 15, |r| r.float())?;
        let adaptative_emissive_final_exposure_max = r.optional(r.version >= 15, |r| r.float())?;
        let glowmap = r.optional(r.version >= 16, |r| r.boolean())?;
        let effect_pbr_specular = r.optional(r.version >= 20, |r| r.boolean())?;
        Ok(Self {
            base_texture,
            grayscale_texture,
            envmap_texture,
            normal_texture,
            envmap_mask_texture,
            specular_texture,
            lighting_texture,
            glow_texture,
            glass_roughness_scratch,
            glass_dirt_overlay,
            glass_enabled,
            glass,
            environment_mapping,
            environment_mapping_mask_scale,
            blood_enabled,
            effect_lighting_enabled,
            falloff_enabled,
            falloff_color_enabled,
            grayscale_to_palette_alpha,
            soft_enabled,
            base_color,
            base_color_scale,
            falloff_start_angle,
            falloff_stop_angle,
            falloff_start_opacity,
            falloff_stop_opacity,
            lighting_influence,
            envmap_min_lod,
            soft_depth,
            emittance_color,
            adaptative_emissive_exposure_offset,
            adaptative_emissive_final_exposure_min,
            adaptative_emissive_final_exposure_max,
            glowmap,
            effect_pbr_specular,
        })
    }
    pub fn non_finite_values(&self) -> u64 {
        fo4_core::ieee::count(&self.environment_mapping_mask_scale)
            + fo4_core::ieee::count(&self.base_color)
            + fo4_core::ieee::count(&self.base_color_scale)
            + fo4_core::ieee::count(&self.falloff_start_angle)
            + fo4_core::ieee::count(&self.falloff_stop_angle)
            + fo4_core::ieee::count(&self.falloff_start_opacity)
            + fo4_core::ieee::count(&self.falloff_stop_opacity)
            + fo4_core::ieee::count(&self.lighting_influence)
            + fo4_core::ieee::count(&self.soft_depth)
            + fo4_core::ieee::count(&self.emittance_color)
            + fo4_core::ieee::count(&self.adaptative_emissive_exposure_offset)
            + fo4_core::ieee::count(&self.adaptative_emissive_final_exposure_min)
            + fo4_core::ieee::count(&self.adaptative_emissive_final_exposure_max)
    }
}
