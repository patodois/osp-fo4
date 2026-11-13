#![forbid(unsafe_code)]
//! Attributed Rust port of the MaterialLib layout (MIT), copyright (c) 2018 ousnius.
//! Reference: ousnius/Material-Editor at 21411873f17454a2442d6785d533499e14c63adb.
//! See THIRD_PARTY.md for the MIT notice and spec/material.md for field provenance.
mod bgem;
mod bgsm;
mod common;
mod reader;
pub use bgem::Bgem;
pub use bgsm::Bgsm;
pub use common::Common;
use fo4_core::{Budget, Cursor, Error, Limits, Result, SliceSource};
use reader::Reader;
use serde::Serialize;
/// Highest explicitly versioned gate in the pinned MaterialLib family.
/// Close the open-ended C# comparisons instead of guessing future formats.
pub const MAX_VERSION: u32 = 22;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct WireBool(pub u8);
impl WireBool {
    pub fn value(self) -> bool {
        self.0 != 0
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub enum BlendMode {
    Unknown,
    None,
    Standard,
    Additive,
    Multiplicative,
}
#[derive(Debug, Serialize)]
pub struct AlphaBlend {
    pub enabled: u8,
    pub source: u32,
    pub destination: u32,
    pub mode: Option<BlendMode>,
}
impl AlphaBlend {
    /// bgsm_blend_disabled_factors_ignored_v1: mode is only a derived label.
    pub fn requires_factor_semantics(&self) -> bool {
        self.enabled != 0
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Path<'a> {
    pub offset: u64,
    pub raw: &'a [u8],
}
impl Path<'_> {
    pub fn bytes(&self) -> &[u8] {
        self.raw.strip_suffix(&[0]).unwrap_or(self.raw)
    }
    pub fn display_cp1252(&self, file: &str, b: &Budget) -> Result<String> {
        fo4_core::decode_cp1252(self.bytes(), file, self.offset, b)
    }
}
#[derive(Debug, Serialize)]
pub struct Terrain {
    pub unknown_v3: Option<u32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub threshold_falloff: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub tiling_distance: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub rotation_angle: f32,
}
impl Terrain {
    fn read(r: &mut Reader<'_, '_, '_>) -> Result<Self> {
        Ok(Self {
            unknown_v3: r.optional(r.version == 3, |r| r.u32())?,
            threshold_falloff: r.float()?,
            tiling_distance: r.float()?,
            rotation_angle: r.float()?,
        })
    }
    fn non_finite_values(&self) -> u64 {
        fo4_core::ieee::count(&[
            self.threshold_falloff,
            self.tiling_distance,
            self.rotation_angle,
        ])
    }
}
/// MaterialLib marks the semantic order of these glass scalars as uncertain.
/// Preserve their disk order, bits and bytes without asserting those names.
#[derive(Debug, Serialize)]
pub struct Glass<'a> {
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub fresnel_color: [f32; 3],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub first: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub middle_v22: Option<f32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub last: f32,
    pub raw: &'a [u8],
}
impl<'a> Glass<'a> {
    fn read(r: &mut Reader<'_, '_, 'a>) -> Result<Self> {
        let start = r.c.pos;
        let fresnel_color = r.color()?;
        let first = r.float()?;
        let middle_v22 = r.optional(r.version >= 22, |r| r.float())?;
        let last = r.float()?;
        Ok(Self {
            fresnel_color,
            first,
            middle_v22,
            last,
            raw: &r.bytes[start as usize..r.c.pos as usize],
        })
    }
    fn non_finite_values(&self) -> u64 {
        fo4_core::ieee::count(&self.fresnel_color)
            + fo4_core::ieee::count(&[self.first, self.last])
            + fo4_core::ieee::count(&self.middle_v22)
    }
}
// Fixed, stack-resident variants avoid an infallible boxed allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
pub enum Kind<'a> {
    Bgsm(Bgsm<'a>),
    Bgem(Bgem<'a>),
}
#[derive(Debug, Serialize)]
pub struct Material<'a> {
    pub version: u32,
    pub common: Common,
    pub kind: Kind<'a>,
    #[serde(skip)]
    pub raw: &'a [u8],
}
impl<'a> Material<'a> {
    pub fn read(bytes: &'a [u8], file: &str, l: &Limits, _b: &Budget) -> Result<Self> {
        l.validate(file)?;
        fo4_core::limit(bytes.len() as u64, l.max_payload, file, 0)?;
        let source = SliceSource { name: file, bytes };
        let mut c = Cursor::new(&source);
        let magic = c.take::<4>()?;
        if magic != *b"BGSM" && magic != *b"BGEM" {
            return Err(Error::new(
                "InvalidMaterialMagic",
                file,
                0,
                "BGSM or BGEM",
                format!("{magic:?}"),
            ));
        }
        let version = c.u32()?;
        if version > MAX_VERSION {
            return Err(Error::new(
                "UnsupportedMaterialVersion",
                file,
                4,
                format!("MaterialLib family versions 0..={MAX_VERSION}"),
                version,
            ));
        }
        let mut r = Reader {
            c: &mut c,
            bytes,
            version,
            l,
        };
        let common = Common::read(&mut r)?;
        let kind = if magic == *b"BGSM" {
            Kind::Bgsm(Bgsm::read(&mut r)?)
        } else {
            Kind::Bgem(Bgem::read(&mut r)?)
        };
        if c.pos != c.end {
            return Err(Error::new(
                "TrailingMaterialData",
                file,
                c.pos,
                "exact material EOF",
                c.end - c.pos,
            ));
        }
        Ok(Self {
            version,
            common,
            kind,
            raw: bytes,
        })
    }
    pub fn magic(&self) -> &'static str {
        match self.kind {
            Kind::Bgsm(_) => "BGSM",
            Kind::Bgem(_) => "BGEM",
        }
    }
    pub fn texture_paths(&self) -> impl Iterator<Item = (&'static str, &Path<'a>)> {
        let (bgsm, bgem) = match &self.kind {
            Kind::Bgsm(v) => (Some(v), None),
            Kind::Bgem(v) => (None, Some(v)),
        };
        bgsm.into_iter()
            .flat_map(Bgsm::texture_paths)
            .chain(bgem.into_iter().flat_map(Bgem::texture_paths))
    }
    pub fn non_finite_values(&self) -> u64 {
        self.common.non_finite_values()
            + match &self.kind {
                Kind::Bgsm(v) => {
                    v.non_finite_values()
                        + v.terrain_data
                            .as_ref()
                            .map_or(0, Terrain::non_finite_values)
                }
                Kind::Bgem(v) => {
                    v.non_finite_values() + v.glass.as_ref().map_or(0, Glass::non_finite_values)
                }
            }
    }
    pub fn uninterpreted_components(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        let terrain = match &self.kind {
            Kind::Bgsm(v) => u64::from(
                v.terrain_data
                    .as_ref()
                    .is_some_and(|v| v.unknown_v3.is_some()),
            ),
            _ => 0,
        };
        let glass = match &self.kind {
            Kind::Bgem(v) => v
                .glass
                .as_ref()
                .map_or(0, |g| 2 + u64::from(g.middle_v22.is_some())),
            _ => 0,
        };
        [
            (
                "tile_flags.extra_bits",
                u64::from(self.common.tile_flags & !3 != 0),
            ),
            (
                "mask_writes.extra_bits",
                u64::from(self.common.mask_writes.is_some_and(|v| v & !63 != 0)),
            ),
            (
                "alpha_blend.enabled_factors_unsourced",
                u64::from(self.common.alpha_blend.requires_factor_semantics()),
            ),
            ("terrain.unknown_v3", terrain),
            ("bgem.glass_scalars", glass),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
    }
    pub fn uninterpreted_fields(&self) -> u64 {
        self.uninterpreted_components().map(|(_, n)| n).sum()
    }
}
impl<'a> Bgsm<'a> {
    pub fn texture_paths(&self) -> impl Iterator<Item = (&'static str, &Path<'a>)> {
        [
            ("diffuse", Some(&self.diffuse_texture)),
            ("normal", Some(&self.normal_texture)),
            ("smooth_spec", Some(&self.smooth_spec_texture)),
            ("greyscale", Some(&self.greyscale_texture)),
            (
                "glow",
                self.glow_texture
                    .as_ref()
                    .or(self.glow_texture_legacy.as_ref()),
            ),
            (
                "wrinkles",
                self.wrinkles_texture
                    .as_ref()
                    .or(self.wrinkles_texture_legacy.as_ref()),
            ),
            ("specular", self.specular_texture.as_ref()),
            ("lighting", self.lighting_texture.as_ref()),
            ("flow", self.flow_texture.as_ref()),
            (
                "distance_field_alpha",
                self.distance_field_alpha_texture.as_ref(),
            ),
            ("envmap", self.envmap_texture.as_ref()),
            ("inner_layer", self.inner_layer_texture.as_ref()),
            ("displacement", self.displacement_texture.as_ref()),
        ]
        .into_iter()
        .filter_map(|(slot, p)| p.map(|p| (slot, p)))
    }
}
impl<'a> Bgem<'a> {
    pub fn texture_paths(&self) -> impl Iterator<Item = (&'static str, &Path<'a>)> {
        [
            ("base", Some(&self.base_texture)),
            ("grayscale", Some(&self.grayscale_texture)),
            ("envmap", Some(&self.envmap_texture)),
            ("normal", Some(&self.normal_texture)),
            ("envmap_mask", Some(&self.envmap_mask_texture)),
            ("specular", self.specular_texture.as_ref()),
            ("lighting", self.lighting_texture.as_ref()),
            ("glow", self.glow_texture.as_ref()),
            (
                "glass_roughness_scratch",
                self.glass_roughness_scratch.as_ref(),
            ),
            ("glass_dirt_overlay", self.glass_dirt_overlay.as_ref()),
        ]
        .into_iter()
        .filter_map(|(slot, p)| p.map(|p| (slot, p)))
    }
}
#[cfg(test)]
mod tests;
