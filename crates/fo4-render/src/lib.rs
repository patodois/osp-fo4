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
pub mod terrain;
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
    /// Model-space vertex normals, kept only for draws with a [`Falloff`].
    pub normals: Option<Vec<[f64; 3]>>,
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
/// Block-compressed layout of [`Blocks`]: 4 x 4 texel blocks of 8 or 16
/// bytes, as the DDS stores them (`spec/dds.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BlockFormat {
    /// BC1 whose DDS declares no alpha: the reference decodes the
    /// three-colour mode's fourth index as opaque black, so [`Blocks::opaque`]
    /// is set.
    Bc1,
    /// BC1 whose fourth index of the three-colour mode is transparent black.
    Bc1Alpha,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
}
impl BlockFormat {
    /// Bytes of one 4 x 4 block.
    pub fn block_bytes(self) -> u64 {
        match self {
            Self::Bc1 | Self::Bc1Alpha | Self::Bc4 => 8,
            _ => 16,
        }
    }
    /// Bytes of a level of `width` x `height` texels.
    pub fn level_bytes(self, width: u32, height: u32) -> u64 {
        u64::from(width.div_ceil(4)) * u64::from(height.div_ceil(4)) * self.block_bytes()
    }
}
/// A texture's stored blocks, kept for a GPU that decodes them itself (the
/// viewer); the CPU reference does not sample them. They stand for the
/// texels of the draw-list texture of the same index ([`DrawList::blocks`]),
/// whose `rgba` and `mips` are then empty. `bytes` holds `levels` levels one
/// after the other: level 0 has the texture's `width` and `height`, both
/// multiples of 4; level k is `max(1, side >> k)`, each the next stored mip
/// without gaps.
#[derive(Debug, Clone)]
pub struct Blocks {
    pub format: BlockFormat,
    pub levels: u32,
    pub bytes: Vec<u8>,
    /// Sampled alpha is 1 whatever the blocks hold: BC1 declared without
    /// alpha, or a terrain texture whose alpha is unused.
    pub opaque: bool,
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
    /// Checks `self` as the texture whose texels are the stored blocks `b`.
    pub fn validate_blocks(&self, b: &Blocks) -> Result<()> {
        let invalid = |found: String| {
            Error::new(
                "InvalidBlockTexture",
                "render texture",
                0,
                "sides multiple of 4, one stored level per mip from the base, no RGBA",
                found,
            )
        };
        if self.width == 0
            || self.height == 0
            || !self.width.is_multiple_of(4)
            || !self.height.is_multiple_of(4)
            || !self.rgba.is_empty()
            || !self.mips.is_empty()
            || b.levels == 0
            || b.levels > 32
        {
            return Err(invalid(format!(
                "{}x{}, {} RGBA, {} mips, {} levels",
                self.width,
                self.height,
                self.rgba.len(),
                self.mips.len(),
                b.levels
            )));
        }
        let expected: u64 = (0..b.levels)
            .map(|k| {
                b.format
                    .level_bytes((self.width >> k).max(1), (self.height >> k).max(1))
            })
            .sum();
        if b.bytes.len() as u64 != expected {
            return Err(invalid(format!(
                "{} bytes for {} levels of {}x{}",
                b.bytes.len(),
                b.levels,
                self.width,
                self.height
            )));
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
impl Texture {
    /// Bilinear sample of the base level at `uv` with both coordinates
    /// clamped to 0 to 1 and texel indices clamped to the edge (no wrap):
    /// the palette lookup of an effect surface ([`Palette`]).
    pub fn sample_clamped(&self, uv: [f64; 2]) -> [f32; 4] {
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let texel = |x: i64, y: i64| {
            let at = (y.clamp(0, h - 1) as usize * w as usize + x.clamp(0, w - 1) as usize) * 4;
            std::array::from_fn::<_, 4, _>(|i| self.rgba[at + i])
        };
        let x = uv[0].clamp(0., 1.) * w as f64 - 0.5;
        let y = uv[1].clamp(0., 1.) * h as f64 - 0.5;
        let fx = (x - x.floor()) as f32;
        let fy = (y - y.floor()) as f32;
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        let a = texel(ix, iy);
        let b = texel(ix + 1, iy);
        let c = texel(ix, iy + 1);
        let d = texel(ix + 1, iy + 1);
        std::array::from_fn(|i| {
            (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
        })
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
            || self.opacity < 0.
            // Above 1 the draw build gates it: without blending by
            // `bgsm_opacity_above_one_test_v1` (it then only scales the
            // tested alpha), with blending by
            // `blend_alpha_clamped_before_blend_v1` (the blend reads the
            // alpha clamped to 1).
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
/// The falloff of an effect surface (`draw::effect`,
/// `spec/effect-surfaces.md`, Falloff): the angle fields are cosines
/// (nifxml), the opacities the alpha factor at each. Every term is a
/// pending rule: the cosine is that of the angle between the interpolated
/// surface normal and the direction to the eye
/// (`effect_falloff_view_angle_v1`), taken by its absolute value
/// (`effect_falloff_cosine_absolute_v1`); the factor is linear in that
/// cosine between the two angles and constant outside them
/// (`effect_falloff_linear_in_cosine_v1`), and it multiplies alpha only
/// (`effect_falloff_multiplies_alpha_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Falloff {
    pub start_angle: f32,
    pub stop_angle: f32,
    pub start_opacity: f32,
    pub stop_opacity: f32,
}
impl Falloff {
    /// Finite angles, opacities within 0 to 1, and two different angles
    /// unless the two opacities are equal (the factor is then that
    /// opacity whatever the cosine).
    pub fn validate(self) -> Result<()> {
        let angles = [self.start_angle, self.stop_angle];
        let opacities = [self.start_opacity, self.stop_opacity];
        if angles.iter().any(|v| !v.is_finite())
            || opacities.iter().any(|v| !(0. ..=1.).contains(v))
            || (self.start_angle == self.stop_angle && self.start_opacity != self.stop_opacity)
        {
            return Err(Error::new(
                "UnsupportedRenderFalloff",
                "draw list",
                0,
                "finite angles, opacities within 0 to 1, different angles or equal opacities",
                format!("{self:?}"),
            ));
        }
        Ok(())
    }
    /// The alpha factor at `cosine`, the signed cosine between the surface
    /// normal and the direction from the surface to the eye.
    pub fn factor(self, cosine: f64) -> f64 {
        let (a, b) = (f64::from(self.start_angle), f64::from(self.stop_angle));
        let (p, q) = (f64::from(self.start_opacity), f64::from(self.stop_opacity));
        if a == b {
            return p;
        }
        let t = ((cosine.abs() - a) / (b - a)).clamp(0., 1.);
        p + (q - p) * t
    }
}
/// The greyscale-to-palette lookup of an effect surface (`draw::effect`,
/// `spec/effect-surfaces.md`, Palette). Every term is a pending rule: the
/// palette texture is read bilinearly at its base level with clamped
/// coordinates (`effect_palette_sampled_clamped_v1`); its vertical
/// coordinate is the surface's alpha before the palette, the base colour's
/// alpha (the draw's opacity) times the vertex alpha where read, clamped
/// to 0 to 1 (`effect_palette_v_from_alpha_v1`); the colour lookup's
/// horizontal coordinate is the texel's green channel
/// (`effect_palette_colour_u_green_v1`) and its RGB replaces the texel's
/// (`effect_palette_colour_replaces_rgb_v1`); the alpha lookup's
/// horizontal coordinate is the texel's alpha
/// (`effect_palette_alpha_u_alpha_v1`) and its alpha replaces the
/// surface's alpha before the falloff
/// (`effect_palette_alpha_replaces_alpha_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Palette {
    /// Index of the palette (the shader's greyscale texture) in
    /// `DrawList::textures`; always decoded texels, never stored blocks.
    pub texture: usize,
    /// GreyscaleToPalette_Color.
    pub colour: bool,
    /// GreyscaleToPalette_Alpha.
    pub alpha: bool,
}
impl Palette {
    /// The lookup coordinates of the colour and of the alpha lookup for a
    /// `texel` and the surface's alpha `v` before the palette.
    pub fn coordinates(texel: [f32; 4], v: f32) -> [[f64; 2]; 2] {
        let v = f64::from(v.clamp(0., 1.));
        [[f64::from(texel[1]), v], [f64::from(texel[3]), v]]
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
    /// An effect surface's falloff; its mesh then has `normals`.
    pub falloff: Option<Falloff>,
    /// An effect surface's greyscale-to-palette lookup.
    pub palette: Option<Palette>,
    /// An effect surface's base colour times its scale, multiplied into the
    /// RGB of the interpolated vertex colour (its mesh's vertex colours
    /// then hold no tint): set only where the clock drives the emissive
    /// colour or multiple (`fo4_scene::clock::Switches`); `None`, the mesh's
    /// vertex colours hold the tint as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<[f32; 3]>,
}
/// Slots of [`Animation::tracks`].
pub const TRACK_U_OFFSET: usize = 0;
pub const TRACK_V_OFFSET: usize = 1;
pub const TRACK_U_SCALE: usize = 2;
pub const TRACK_V_SCALE: usize = 3;
pub const TRACK_OPACITY: usize = 4;
/// The emissive multiple (the base colour scale), behind
/// `effect_clock_emissive_multiple_v1`'s switch.
pub const TRACK_EMISSIVE_MULTIPLE: usize = 5;
/// The emissive colour (three components, the base colour's RGB), behind
/// `effect_clock_emissive_colour_v1`'s switch.
pub const TRACK_EMISSIVE_COLOUR: usize = 6;
/// Falloff start and stop angle (keys in degrees, the field their cosine)
/// and start and stop opacity, behind `effect_clock_falloff_v1`'s switch.
pub const TRACK_FALLOFF_START_ANGLE: usize = 7;
pub const TRACK_FALLOFF_STOP_ANGLE: usize = 8;
pub const TRACK_FALLOFF_START_OPACITY: usize = 9;
pub const TRACK_FALLOFF_STOP_OPACITY: usize = 10;
/// Number of [`Animation::tracks`] slots.
pub const TRACKS: usize = 11;
/// The fields of a draw the game clock can drive ([`Animation::fields`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockFields {
    pub uv_offset: [f64; 2],
    pub uv_scale: [f64; 2],
    pub opacity: f32,
    /// [`Draw::colour`].
    pub colour: Option<[f32; 3]>,
    pub falloff: Option<Falloff>,
}
impl Draw {
    /// The fields the game clock can drive, as the draw holds them.
    pub fn clock_fields(&self) -> ClockFields {
        ClockFields {
            uv_offset: self.uv_offset,
            uv_scale: self.uv_scale,
            opacity: self.alpha.opacity,
            colour: self.colour,
            falloff: self.falloff,
        }
    }
    pub fn set_clock_fields(&mut self, f: ClockFields) {
        self.uv_offset = f.uv_offset;
        self.uv_scale = f.uv_scale;
        self.alpha.opacity = f.opacity;
        self.colour = f.colour;
        self.falloff = f.falloff;
    }
}
/// A draw whose surface fields free-running controllers drive on the game
/// clock (`draw::BuildOptions::time`, `fo4_scene::clock`, each value under
/// the clock rules the build recorded for the instance): the draw's UV
/// offset and scale components and its opacity at any time, and behind
/// their switches its colour (emissive colour times emissive multiple) and
/// falloff fields, from the same tracks for the CPU reference and the GPU
/// viewer.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// Index into `DrawList::draws`.
    pub draw: usize,
    /// U offset, V offset, U scale, V scale, opacity, emissive multiple,
    /// emissive colour, falloff start angle, stop angle, start opacity,
    /// stop opacity ([`TRACK_U_OFFSET`] and the others); `None`: the field
    /// is held as the draw stores it.
    pub tracks: [Option<fo4_scene::clock::Track>; TRACKS],
    /// The base colour's RGB and its scale where no track drives them: the
    /// terms of [`Draw::colour`] when a colour or multiple track exists.
    pub colour: [f32; 3],
    pub scale: f32,
    /// The draw's world transform over time where free-running transform
    /// controllers move nodes above it (behind the clock's transform
    /// switches); `None`: the draw's world is fixed.
    pub motion: Option<Motion>,
}
/// The world transform of a draw under nodes that free-running transform
/// controllers move on the game clock (`fo4_scene::clock::TransformTrack`,
/// behind the clock's transform switches): `pre` (the placement and the
/// fixed nodes above the first moving one), then for each moving node, top
/// down, its local transform at the time followed by the fixed nodes below
/// it (`after`, down to the next moving node or the shape).
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub pre: fo4_scene::transform::Transform,
    pub steps: Vec<MotionStep>,
}
/// A moving node of a [`Motion`] and the fixed transform below it.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionStep {
    pub node: fo4_scene::clock::TransformTrack,
    pub after: fo4_scene::transform::Transform,
}
impl Motion {
    /// The world transform `seconds` after load: `pre`, then each step's
    /// local transform and fixed transform, composed parent first. A value
    /// that is not finite, or a scale that is not positive, is an error.
    pub fn world(&self, seconds: f64) -> Result<fo4_scene::transform::Transform> {
        let mut w = self.pre;
        for s in &self.steps {
            let local = s.node.local(seconds).ok_or_else(|| {
                Error::new(
                    "ClockTrackValue",
                    "draw list",
                    s.node.node as u64,
                    "a node transform at the time",
                    seconds,
                )
            })?;
            w = w.compose(local).compose(s.after);
        }
        let finite = w
            .rotation
            .iter()
            .flatten()
            .chain(&w.translation)
            .all(|v| v.is_finite());
        if !finite || !(w.scale.is_finite() && w.scale > 0.) {
            return Err(Error::new(
                "ClockTrackValue",
                "draw list",
                0,
                "a finite world transform with a positive scale",
                format!("{w:?}"),
            ));
        }
        Ok(w)
    }
}
impl Animation {
    /// An animation of UV, opacity or falloff tracks alone (the colour terms
    /// unused).
    pub fn new(draw: usize, tracks: [Option<fo4_scene::clock::Track>; TRACKS]) -> Self {
        Self {
            draw,
            tracks,
            colour: [1.; 3],
            scale: 1.,
            motion: None,
        }
    }
    /// Whether a surface field is tracked (not only the world transform).
    pub fn has_tracks(&self) -> bool {
        self.tracks.iter().any(Option::is_some)
    }
    fn value(&self, slot: usize, seconds: f64) -> Result<Option<[f32; 3]>> {
        let Some(t) = &self.tracks[slot] else {
            return Ok(None);
        };
        let n = t.keys.components.min(3);
        t.at(seconds)
            .filter(|x| x[..n].iter().all(|v| v.is_finite()))
            .map(Some)
            .ok_or_else(|| {
                Error::new(
                    "ClockTrackValue",
                    "draw list",
                    self.draw as u64,
                    "finite value at the time",
                    seconds,
                )
            })
    }
    /// The draw's clock fields at `seconds` of the clock after load: each
    /// tracked field replaced by its track's value (a falloff angle's key
    /// read as degrees and turned into its cosine,
    /// `nif_controller_falloff_angle_degrees_v1`; the colour the emissive
    /// colour times the emissive multiple, each tracked or held), the
    /// others as given in `held`. A value that is not finite, an opacity
    /// below 0, a colour below 0 or a falloff that fails
    /// `Falloff::validate` is an error.
    pub fn fields(&self, seconds: f64, held: ClockFields) -> Result<ClockFields> {
        let mut out = held;
        let one = |slot: usize| -> Result<Option<f64>> {
            Ok(self.value(slot, seconds)?.map(|v| f64::from(v[0])))
        };
        if let Some(v) = one(TRACK_U_OFFSET)? {
            out.uv_offset[0] = v;
        }
        if let Some(v) = one(TRACK_V_OFFSET)? {
            out.uv_offset[1] = v;
        }
        if let Some(v) = one(TRACK_U_SCALE)? {
            out.uv_scale[0] = v;
        }
        if let Some(v) = one(TRACK_V_SCALE)? {
            out.uv_scale[1] = v;
        }
        if let Some(v) = one(TRACK_OPACITY)? {
            if v < 0. {
                return Err(Error::new(
                    "ClockTrackValue",
                    "draw list",
                    self.draw as u64,
                    "opacity at least 0",
                    v,
                ));
            }
            out.opacity = v as f32;
        }
        let scale = self.value(TRACK_EMISSIVE_MULTIPLE, seconds)?;
        let colour = self.value(TRACK_EMISSIVE_COLOUR, seconds)?;
        if scale.is_some() || colour.is_some() {
            let s = scale.map_or(self.scale, |v| v[0]);
            let c = colour.unwrap_or([self.colour[0], self.colour[1], self.colour[2]]);
            let tint: [f32; 3] = std::array::from_fn(|i| c[i] * s);
            if tint.iter().any(|v| !v.is_finite() || *v < 0.) {
                return Err(Error::new(
                    "ClockTrackValue",
                    "draw list",
                    self.draw as u64,
                    "colour at least 0",
                    format!("{tint:?}"),
                ));
            }
            out.colour = Some(tint);
        }
        let falloff = [
            TRACK_FALLOFF_START_ANGLE,
            TRACK_FALLOFF_STOP_ANGLE,
            TRACK_FALLOFF_START_OPACITY,
            TRACK_FALLOFF_STOP_OPACITY,
        ];
        if falloff.iter().any(|s| self.tracks[*s].is_some()) {
            let mut f = held.falloff.ok_or_else(|| {
                Error::new(
                    "ClockTrackValue",
                    "draw list",
                    self.draw as u64,
                    "a falloff to drive",
                    "none",
                )
            })?;
            if let Some(v) = one(TRACK_FALLOFF_START_ANGLE)? {
                f.start_angle = fo4_scene::rest::degrees_cosine(v as f32);
            }
            if let Some(v) = one(TRACK_FALLOFF_STOP_ANGLE)? {
                f.stop_angle = fo4_scene::rest::degrees_cosine(v as f32);
            }
            if let Some(v) = one(TRACK_FALLOFF_START_OPACITY)? {
                f.start_opacity = v as f32;
            }
            if let Some(v) = one(TRACK_FALLOFF_STOP_OPACITY)? {
                f.stop_opacity = v as f32;
            }
            f.validate()?;
            out.falloff = Some(f);
        }
        Ok(out)
    }
}
#[derive(Debug, Default)]
pub struct DrawList {
    pub meshes: Vec<Mesh>,
    pub textures: Vec<Texture>,
    pub draws: Vec<Draw>,
    /// Draws whose surface fields the game clock drives; empty unless the
    /// list was built with a time (`draw::BuildOptions::time`).
    pub animations: Vec<Animation>,
    /// Indices into `draws`, strictly increasing, of draws laid over an
    /// opaque draw of the same triangles (the terrain's texture passes): a
    /// sample of such a draw whose depth equals the stored opaque depth is
    /// kept. Every other draw keeps only samples strictly nearer than it.
    pub overlays: Vec<usize>,
    /// Stored blocks by texture index, for a GPU consumer; empty in the
    /// reference, and a list with any is not drawn by the CPU.
    pub blocks: std::collections::BTreeMap<usize, Blocks>,
}
impl DrawList {
    /// Validates texture `i`, as its stored blocks when it has them.
    pub fn validate_texture(&self, i: usize) -> Result<()> {
        let t = self
            .textures
            .get(i)
            .ok_or_else(|| Error::new("RenderTextureIndex", "draw list", 0, "texture index", i))?;
        match self.blocks.get(&i) {
            Some(b) => t.validate_blocks(b),
            None => t.validate(),
        }
    }
    /// Whether draw `i` is an overlay; fails unless `overlays` is strictly
    /// increasing and within `draws`.
    pub fn validate_overlays(&self) -> Result<()> {
        let ordered = self.overlays.windows(2).all(|w| w[0] < w[1]);
        if !ordered || self.overlays.last().is_some_and(|i| *i >= self.draws.len()) {
            return Err(Error::new(
                "InvalidOverlayDraws",
                "draw list",
                0,
                "strictly increasing draw indices",
                format!("{} of {} draws", self.overlays.len(), self.draws.len()),
            ));
        }
        Ok(())
    }
    pub fn is_overlay(&self, draw: usize) -> bool {
        self.overlays.binary_search(&draw).is_ok()
    }
    /// Sets every animated draw's clock fields to their values at
    /// `seconds` of the game clock after load ([`Animation::fields`]), and
    /// a moving draw's world transform ([`Motion::world`]).
    pub fn at_time(&mut self, seconds: f64) -> Result<()> {
        for a in &self.animations {
            let d = self.draws.get_mut(a.draw).ok_or_else(|| {
                Error::new("RenderDrawIndex", "draw list", 0, "animated draw", a.draw)
            })?;
            let f = a.fields(seconds, d.clock_fields())?;
            d.set_clock_fields(f);
            if let Some(m) = &a.motion {
                d.world = m.world(seconds)?;
            }
        }
        Ok(())
    }
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
