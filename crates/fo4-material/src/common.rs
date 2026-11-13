use crate::{Reader, WireBool};
use fo4_core::Result;
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Common {
    pub tile_flags: u32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub u_offset: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub v_offset: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub u_scale: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub v_scale: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub alpha: f32,
    pub alpha_blend: crate::AlphaBlend,
    pub alpha_test_ref: u8,
    pub alpha_test: WireBool,
    pub z_buffer_write: WireBool,
    pub z_buffer_test: WireBool,
    pub screen_space_reflections: WireBool,
    pub wetness_control_screen_space_reflections: WireBool,
    pub decal: WireBool,
    pub two_sided: WireBool,
    pub decal_no_fade: WireBool,
    pub non_occluder: WireBool,
    pub refraction: WireBool,
    pub refraction_falloff: WireBool,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub refraction_power: f32,
    pub environment_mapping: Option<WireBool>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub environment_mapping_mask_scale: Option<f32>,
    pub depth_bias: Option<WireBool>,
    pub grayscale_to_palette_color: WireBool,
    pub mask_writes: Option<u8>,
}
impl Common {
    pub(crate) fn read(r: &mut Reader<'_, '_, '_>) -> Result<Self> {
        let tile_flags = r.u32()?;
        let u_offset = r.float()?;
        let v_offset = r.float()?;
        let u_scale = r.float()?;
        let v_scale = r.float()?;
        let alpha = r.float()?;
        let alpha_blend = r.blend()?;
        let alpha_test_ref = r.byte()?;
        let alpha_test = r.boolean()?;
        let z_buffer_write = r.boolean()?;
        let z_buffer_test = r.boolean()?;
        let screen_space_reflections = r.boolean()?;
        let wetness_control_screen_space_reflections = r.boolean()?;
        let decal = r.boolean()?;
        let two_sided = r.boolean()?;
        let decal_no_fade = r.boolean()?;
        let non_occluder = r.boolean()?;
        let refraction = r.boolean()?;
        let refraction_falloff = r.boolean()?;
        let refraction_power = r.float()?;
        let environment_mapping = r.optional(r.version < 10, |r| r.boolean())?;
        let environment_mapping_mask_scale = r.optional(r.version < 10, |r| r.float())?;
        let depth_bias = r.optional(r.version >= 10, |r| r.boolean())?;
        let grayscale_to_palette_color = r.boolean()?;
        let mask_writes = r.optional(r.version >= 6, |r| r.byte())?;
        Ok(Self {
            tile_flags,
            u_offset,
            v_offset,
            u_scale,
            v_scale,
            alpha,
            alpha_blend,
            alpha_test_ref,
            alpha_test,
            z_buffer_write,
            z_buffer_test,
            screen_space_reflections,
            wetness_control_screen_space_reflections,
            decal,
            two_sided,
            decal_no_fade,
            non_occluder,
            refraction,
            refraction_falloff,
            refraction_power,
            environment_mapping,
            environment_mapping_mask_scale,
            depth_bias,
            grayscale_to_palette_color,
            mask_writes,
        })
    }
    pub fn non_finite_values(&self) -> u64 {
        fo4_core::ieee::count(&self.u_offset)
            + fo4_core::ieee::count(&self.v_offset)
            + fo4_core::ieee::count(&self.u_scale)
            + fo4_core::ieee::count(&self.v_scale)
            + fo4_core::ieee::count(&self.alpha)
            + fo4_core::ieee::count(&self.refraction_power)
            + fo4_core::ieee::count(&self.environment_mapping_mask_scale)
    }
}
