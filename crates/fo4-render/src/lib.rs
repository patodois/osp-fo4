#![forbid(unsafe_code)]
//! Deterministic, unlit CPU reference. Policies are specified in spec/render.md.
pub mod compare;
pub mod draw;
pub mod maps;
pub mod persistent;
pub mod png;
mod raster;
pub mod silhouette;
pub mod snapshot;
use fo4_core::{Budget, Error, Result};
use fo4_scene::transform::Transform;
pub use raster::{render, Frame, RenderOptions, Statistics};
use serde::Serialize;
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f64; 3],
    pub uv: [f64; 2],
}
#[derive(Debug)]
pub struct Mesh {
    /// Enabled effect channels; absent on ordinary textured geometry.
    pub vertex_colors: Option<Vec<[f32; 4]>>,
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u16; 3]>,
}
#[derive(Debug)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<f32>,
    pub mip: u32,
}
#[derive(Debug)]
pub struct Texture {
    pub mips: Vec<Mip>,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<f32>,
    pub mip: u32,
    pub srgb: bool,
}
impl Texture {
    pub fn validate(&self) -> Result<()> {
        let n = fo4_core::mul(
            fo4_core::mul(self.width as u64, self.height as u64, "render texture", 0)?,
            4,
            "render texture",
            0,
        )?;
        if self.width == 0
            || self.height == 0
            || n != self.rgba.len() as u64
            || self.rgba.iter().any(|v| !v.is_finite())
        {
            return Err(Error::new(
                "InvalidRenderTexture",
                "render texture",
                0,
                "finite RGBA with width*height*4 elements",
                self.rgba.len(),
            ));
        }
        let mut previous = self.mip;
        for m in &self.mips {
            let delta = m.mip.checked_sub(self.mip).ok_or_else(|| {
                Error::new("InvalidMipChain", "texture", 0, "increasing mip", m.mip)
            })?;
            if m.mip <= previous
                || m.width != (self.width >> delta.min(31)).max(1)
                || m.height != (self.height >> delta.min(31)).max(1)
                || m.rgba.len() as u64
                    != fo4_core::mul(
                        fo4_core::mul(m.width as u64, m.height as u64, "mip", 0)?,
                        4,
                        "mip",
                        0,
                    )?
                || m.rgba.iter().any(|v| !v.is_finite())
            {
                return Err(Error::new(
                    "InvalidMipChain",
                    "texture",
                    0,
                    "declared finite coarse mip",
                    m.mip,
                ));
            }
            previous = m.mip;
        }
        Ok(())
    }
    pub fn sample_lod(&self, uv: [f64; 2], bilinear: bool, lod: f64) -> ([f32; 4], usize) {
        let mut level = 0;
        if bilinear {
            let wanted = self.mip as f64 + lod.max(0.).round();
            for (i, m) in self.mips.iter().enumerate() {
                if m.mip as f64 <= wanted {
                    level = i + 1;
                }
            }
        }
        let (w, h, rgba) = if level == 0 {
            (self.width, self.height, &self.rgba)
        } else {
            let m = &self.mips[level - 1];
            (m.width, m.height, &m.rgba)
        };
        let texel = |x: i64, y: i64| {
            let at = ((y.rem_euclid(h as i64) as usize * w as usize)
                + x.rem_euclid(w as i64) as usize)
                * 4;
            std::array::from_fn::<_, 4, _>(|i| rgba[at + i])
        };
        if !bilinear {
            return (
                texel(
                    (uv[0].rem_euclid(1.) * w as f64).floor() as i64,
                    (uv[1].rem_euclid(1.) * h as f64).floor() as i64,
                ),
                level,
            );
        }
        let x = uv[0].rem_euclid(1.) * w as f64 - 0.5;
        let y = uv[1].rem_euclid(1.) * h as f64 - 0.5;
        let fx = (x - x.floor()) as f32;
        let fy = (y - y.floor()) as f32;
        let ix = x.floor() as i64;
        let iy = y.floor() as i64;
        let a = texel(ix, iy);
        let b = texel(ix + 1, iy);
        let c = texel(ix, iy + 1);
        let d = texel(ix + 1, iy + 1);
        (
            std::array::from_fn(|i| {
                (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
            }),
            level,
        )
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Alpha {
    pub enabled: u8,
    pub source: u32,
    pub destination: u32,
    pub test_enabled: u8,
    pub test_ref: u8,
    /// NiAlphaProperty comparison number; BGSM has no comparison field.
    pub nif_test_function: Option<u8>,
    pub opacity: f32,
    pub nif_flags: Option<u16>,
}
impl Alpha {
    pub fn validate(self) -> Result<()> {
        if self.enabled > 1
            || self.test_enabled > 1
            || !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
            || (self.enabled == 1
                && if self.nif_test_function.is_some() {
                    self.source > 10 || self.destination > 10
                } else {
                    ![(6, 7), (6, 0), (4, 1)].contains(&(self.source, self.destination))
                })
            || self.nif_test_function.is_some_and(|n| n > 7)
        {
            return Err(Error::new(
                "UnsupportedRenderAlpha",
                "draw list",
                0,
                "disabled or registered raw tuple and finite opacity",
                format!("{self:?}"),
            ));
        }
        Ok(())
    }
    fn passes(self, alpha: f32) -> bool {
        if self.test_enabled == 0 {
            return true;
        }
        let x = alpha * 255.;
        let r = self.test_ref as f32;
        match self.nif_test_function {
            Some(0) => true,
            Some(1) => x < r,
            Some(2) => x == r,
            Some(3) => x <= r,
            Some(4) => x > r,
            Some(5) => x != r,
            Some(6) => x >= r,
            Some(7) => false,
            // Reference BGSM threshold policy, not a claimed engine comparison.
            _ => x >= r,
        }
    }
}
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Sidedness {
    pub material_two_sided: Option<u8>,
    pub nif_shader_flags_2: Option<u32>,
}
impl Sidedness {
    pub fn double_sided(self) -> Result<bool> {
        if self.material_two_sided.is_some_and(|v| v > 1) {
            return Err(Error::new(
                "UnsupportedSidedness",
                "draw",
                0,
                "wire bool",
                format!("{self:?}"),
            ));
        }
        if let Some(v) = self.material_two_sided {
            return Ok(v == 1);
        }
        self.nif_shader_flags_2.map(|v| v & 16 != 0).ok_or_else(|| {
            Error::new(
                "MissingCullDeclarations",
                "draw",
                0,
                "material or NIF sidedness",
                "absent",
            )
        })
    }
}
#[derive(Debug, Serialize)]
pub struct Draw {
    pub sidedness: Sidedness,
    pub instance: u32,
    pub mesh: usize,
    pub texture: usize,
    pub world: Transform,
    pub alpha: Alpha,
    pub uv_offset: [f64; 2],
    pub uv_scale: [f64; 2],
    pub raw_tile_flags: Option<u32>,
}
#[derive(Debug, Default)]
pub struct DrawList {
    pub meshes: Vec<Mesh>,
    pub textures: Vec<Texture>,
    pub draws: Vec<Draw>,
}
pub(crate) fn append<T>(v: &mut Vec<T>, value: T, b: &Budget) -> Result<()> {
    b.reserve(v, 1, "render", 0)?;
    v.push(value);
    Ok(())
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod scene_fixture;
