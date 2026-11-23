//! `DrawList` to GPU-ready data, with no Bevy world involved.
//!
//! Every value here is read from the draw list exactly as the CPU rasterizer
//! (`fo4_render::render`) reads it, and the same validations run first, so a
//! list the CPU refuses is refused here too. Meshes keep the draw list's
//! sharing; textures are uploaded once each; identical surfaces share one
//! material.
use crate::coords;
use bevy::math::{Quat, Vec3};
use bevy::render::render_resource::BlendFactor;
use fo4_render::{Alpha, BlockFormat, DrawList, RenderOptions};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct GpuMesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Option<Vec<[f32; 4]>>,
    /// Model-space vertex normals, kept for falloff draws
    /// (`fo4_render::Mesh::normals`).
    pub normals: Option<Vec<[f32; 3]>>,
    pub indices: Vec<u16>,
}

/// Texel layout of a [`GpuTexture`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexelFormat {
    /// Decoded texels converted to RGBA16F.
    Rgba16Float,
    /// Stored blocks uploaded as they are (`fo4_render::Blocks`).
    Blocks(BlockFormat),
}

/// Texels, base level first, then each coarser level in order.
pub struct GpuTexture {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub format: TexelFormat,
    /// Sampled alpha is replaced by 1 (`fo4_render::Blocks::opaque`).
    pub opaque: bool,
    /// Moved into the GPU image at upload.
    pub bytes: Vec<u8>,
}

/// Alpha test as the CPU's `Alpha::passes` evaluates it.
pub const TEST_ALWAYS: u32 = 9;
/// No comparison function (BGSM): the CPU reference policy `alpha*255 >= ref`.
pub const TEST_REFERENCE_THRESHOLD: u32 = 8;

/// Per-surface values the fragment shader needs. Plain data, hashed by bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceParams {
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    /// `NiAlphaProperty` function 0..=7, `TEST_REFERENCE_THRESHOLD` or `TEST_ALWAYS`.
    pub test_function: u32,
    pub test_ref: f32,
    pub opacity: f32,
    /// The CPU skips a blended sample whose alpha is exactly zero when the
    /// source factor is code 6 (source alpha).
    pub skip_zero_alpha: bool,
    /// The texture's sampled alpha is replaced by 1 before anything else.
    pub opaque_texture: bool,
    /// An effect falloff (`fo4_render::Falloff`): start and stop angle
    /// (cosines), start and stop opacity.
    pub falloff: Option<[f32; 4]>,
    /// The palette lookups of `Surface::palette` (`fo4_render::Palette`):
    /// bit 0 colour, bit 1 alpha.
    pub palette: u32,
    /// `fo4_render::Draw::colour`: an RGB multiplier after the vertex
    /// colour (1 without one).
    pub colour: Option<[f32; 3]>,
    /// Lit by the scene's light (`GpuScene::light`), as the CPU's
    /// `fo4_render::Lighting::lit` marks the draw.
    pub lit: bool,
    /// Lit with the mesh's vertex normals (a lit draw whose mesh keeps
    /// normals: `light_vertex_normals_v1`), as the CPU's raster does.
    pub smooth: bool,
}
/// Words of [`SurfaceParams::bits`].
pub const PARAM_WORDS: usize = 19;
impl SurfaceParams {
    pub fn bits(&self) -> [u32; PARAM_WORDS] {
        let falloff = self.falloff.unwrap_or([0.; 4]);
        let colour = self.colour.unwrap_or([1.; 3]);
        [
            self.uv_scale[0].to_bits(),
            self.uv_scale[1].to_bits(),
            self.uv_offset[0].to_bits(),
            self.uv_offset[1].to_bits(),
            self.test_function,
            self.test_ref.to_bits(),
            self.opacity.to_bits(),
            u32::from(self.skip_zero_alpha),
            u32::from(self.opaque_texture),
            u32::from(self.falloff.is_some()),
            falloff[0].to_bits(),
            falloff[1].to_bits(),
            falloff[2].to_bits(),
            falloff[3].to_bits(),
            self.palette,
            colour[0].to_bits(),
            colour[1].to_bits(),
            colour[2].to_bits(),
            u32::from(self.lit) | u32::from(self.smooth) << 1,
        ]
    }
}

/// The pipeline-level part of a surface: blend factors (colour and alpha use
/// the same pair, as in the CPU `blend`) and back-face culling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceKey {
    pub blend: Option<(BlendFactor, BlendFactor)>,
    pub cull_back: bool,
    /// `Draw::overlay`: a sample at exactly the stored depth is kept.
    pub overlay: bool,
}

/// Hashes as the two-field key did before `overlay` existed when it is
/// false, so the pipelines and their order (which decides exact depth ties
/// between draws) stay those of a list without overlays.
impl std::hash::Hash for SurfaceKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.blend.hash(state);
        self.cull_back.hash(state);
        if self.overlay {
            self.overlay.hash(state);
        }
    }
}

pub struct Surface {
    pub texture: usize,
    /// The palette texture of an effect surface's palette lookup, an index
    /// into the same textures as `texture`.
    pub palette: Option<usize>,
    pub params: SurfaceParams,
    pub key: SurfaceKey,
}

pub struct Instance {
    pub mesh: usize,
    pub surface: usize,
    /// `Draw::instance`, the CPU frame's per-pixel tag.
    pub tag: u32,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// `base` with the clock fields of animated surface `a` at `seconds` of
/// the game clock: UV offset, UV scale, opacity, and the colour and falloff
/// where their tracks exist.
pub fn animated_params(
    base: SurfaceParams,
    a: &Animated,
    seconds: f64,
) -> Result<SurfaceParams, String> {
    let f = a
        .animation
        .fields(seconds, a.held)
        .map_err(|e| e.to_string())?;
    Ok(SurfaceParams {
        uv_offset: f.uv_offset.map(|v| v as f32),
        uv_scale: f.uv_scale.map(|v| v as f32),
        opacity: f.opacity,
        colour: f.colour,
        falloff: f
            .falloff
            .map(|f| [f.start_angle, f.stop_angle, f.start_opacity, f.stop_opacity]),
        ..base
    })
}

/// A surface whose fields the game clock drives (`fo4_render::Animation`):
/// a surface of its own, never shared, its params set again at any time
/// from the same tracks and held values as the CPU's `DrawList::at_time`.
pub struct Animated {
    pub surface: usize,
    pub animation: fo4_render::Animation,
    /// The draw's clock fields as converted: the values of the fields
    /// without a track.
    pub held: fo4_render::ClockFields,
}

/// An instance whose world transform free-running transform controllers
/// move on the game clock (`fo4_render::Motion`): its transform is set
/// again at any time from the same chain as the CPU's `DrawList::at_time`.
pub struct Moving {
    /// Index into `GpuScene::instances`.
    pub instance: usize,
    pub motion: fo4_render::Motion,
}
/// The instance transform (translation, rotation, scale in Bevy's axes) of
/// `motion` at `seconds` of the game clock.
pub fn moving_transform(
    motion: &fo4_render::Motion,
    seconds: f64,
) -> Result<(Vec3, Quat, Vec3), String> {
    let world = motion.world(seconds).map_err(|e| e.to_string())?;
    coords::instance_transform(&world)
        .ok_or_else(|| format!("UnsupportedInstanceShear: moving instance {world:?}"))
}

#[derive(Default)]
pub struct GpuScene {
    pub meshes: Vec<GpuMesh>,
    /// For each draw-list mesh that was converted, its index in `meshes`.
    pub draw_meshes: BTreeMap<usize, usize>,
    pub textures: Vec<GpuTexture>,
    pub surfaces: Vec<Surface>,
    pub instances: Vec<Instance>,
    /// Surfaces the game clock drives (empty without a build time).
    pub animated: Vec<Animated>,
    /// Instances the game clock moves (empty without a build time and the
    /// clock's transform switches).
    pub moving: Vec<Moving>,
    /// The frame's light (`fo4_scene::light`) as the surface shader reads
    /// it ([`light_words`]); `None` without the light switches.
    pub light: Option<GpuLight>,
}

/// A frame's light for the surface shader: the storage buffer's values and
/// whether a sky fills the background.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuLight {
    pub words: Vec<f32>,
    pub sky: bool,
}

/// Rows of four values before the first point in [`light_words`].
pub const LIGHT_HEADER_ROWS: usize = 17;

/// The storage buffer of `surface.wesl` for `light`, rows of four f32 in
/// game axes: 0 ambient and the point count; 1 the directional colour and
/// whether there is one; 2 the direction toward it and whether anything
/// lights surfaces (`Light::lit`; else only the fog applies); 3 to 8 the ambient
/// cube's entries for a normal toward +X, -X, +Y, -Y, +Z, -Z (row 3's
/// fourth value: whether there is one); 9 the fog colour and whether there
/// is fog; 10 its near, far and maximum; 11 to 13 the sky's zenith (and
/// whether there is a sky), lower sky (and the elevation sine of its stop)
/// and horizon; 14 the exposure (`fo4_scene::light::Exposure`): scale,
/// white, and flags (bit 0 scale, bit 1 white, bit 2 cinematic and tint) as
/// a float; 15 saturation, brightness, contrast, tint amount; 16 the tint's
/// colour; then two rows per point: position and radius, colour.
pub fn light_words(light: &fo4_scene::light::Light) -> Vec<f32> {
    let mut w = vec![0f32; 4 * (LIGHT_HEADER_ROWS + 2 * light.points.len())];
    let row = |r: usize, v: [f32; 4], w: &mut Vec<f32>| w[4 * r..4 * r + 4].copy_from_slice(&v);
    let rgb = |c: [f32; 3], x: f32| [c[0], c[1], c[2], x];
    let count = if light.lit { light.points.len() } else { 0 };
    row(0, rgb(light.ambient, count as f32), &mut w);
    let lit = if light.lit { 1. } else { 0. };
    match (light.lit, light.directional) {
        (true, Some((c, d))) => {
            row(1, rgb(c, 1.), &mut w);
            row(2, [d[0] as f32, d[1] as f32, d[2] as f32, lit], &mut w);
        }
        _ => row(2, [0., 0., 0., lit], &mut w),
    }
    if let (true, Some(cube)) = (light.lit, light.cube) {
        for (k, e) in cube.iter().enumerate() {
            row(3 + k, rgb(*e, if k == 0 { 1. } else { 0. }), &mut w);
        }
    }
    if let Some(f) = light.fog {
        row(9, rgb(f.colour, 1.), &mut w);
        row(10, [f.near as f32, f.far as f32, f.max, 0.], &mut w);
    }
    if let Some(s) = light.sky {
        row(11, rgb(s.zenith, 1.), &mut w);
        row(12, rgb(s.lower, fo4_scene::light::Sky::LOWER_AT), &mut w);
        row(13, rgb(s.horizon, 0.), &mut w);
    }
    let x = &light.exposure;
    let flags = u8::from(x.scale.is_some())
        | u8::from(x.white.is_some()) << 1
        | u8::from(x.cinematic.is_some() || x.tint.is_some()) << 2;
    row(
        14,
        [
            x.scale.unwrap_or(1.),
            x.white.unwrap_or(1.),
            f32::from(flags),
            0.,
        ],
        &mut w,
    );
    let [sat, bright, contrast] = x.cinematic.unwrap_or([1.; 3]);
    let [amount, tr, tg, tb] = x.tint.unwrap_or([0., 1., 1., 1.]);
    row(15, [sat, bright, contrast, amount], &mut w);
    row(16, [tr, tg, tb, 0.], &mut w);
    for (k, p) in light.points.iter().enumerate() {
        let r = LIGHT_HEADER_ROWS + 2 * k;
        row(
            r,
            [
                p.position[0] as f32,
                p.position[1] as f32,
                p.position[2] as f32,
                p.radius as f32,
            ],
            &mut w,
        );
        row(r + 1, rgb(p.colour, 0.), &mut w);
    }
    w
}
impl GpuScene {
    /// Sets every animated surface's params to their values at `seconds`
    /// (the window sets its materials' the same way, `gpu::advance_clock`).
    #[cfg(test)]
    pub fn at_time(&mut self, seconds: f64) -> Result<(), String> {
        for a in &self.animated {
            let base = self
                .surfaces
                .get(a.surface)
                .ok_or_else(|| format!("RenderSurfaceIndex: {}", a.surface))?
                .params;
            self.surfaces[a.surface].params = animated_params(base, a, seconds)?;
        }
        for m in &self.moving {
            let (translation, rotation, scale) = moving_transform(&m.motion, seconds)?;
            let i = self
                .instances
                .get_mut(m.instance)
                .ok_or_else(|| format!("RenderInstanceIndex: {}", m.instance))?;
            i.translation = translation;
            i.rotation = rotation;
            i.scale = scale;
        }
        Ok(())
    }
    pub fn blended_instances(&self) -> usize {
        self.instances
            .iter()
            .filter(|i| self.surfaces[i.surface].key.blend.is_some())
            .count()
    }
}

/// The CPU `blend` factor codes 0..=10 as wgpu factors. Colour and alpha
/// components follow the same table (code 10 gives alpha 1 in both).
pub fn blend_factor(code: u32) -> Result<BlendFactor, String> {
    Ok(match code {
        0 => BlendFactor::One,
        1 => BlendFactor::Zero,
        2 => BlendFactor::Src,
        3 => BlendFactor::OneMinusSrc,
        4 => BlendFactor::Dst,
        5 => BlendFactor::OneMinusDst,
        6 => BlendFactor::SrcAlpha,
        7 => BlendFactor::OneMinusSrcAlpha,
        8 => BlendFactor::DstAlpha,
        9 => BlendFactor::OneMinusDstAlpha,
        10 => BlendFactor::SrcAlphaSaturated,
        _ => return Err(format!("UnsupportedBlendFactor: {code}")),
    })
}

fn surface_params(alpha: Alpha, uv_offset: [f64; 2], uv_scale: [f64; 2]) -> SurfaceParams {
    SurfaceParams {
        uv_scale: uv_scale.map(|v| v as f32),
        uv_offset: uv_offset.map(|v| v as f32),
        test_function: if alpha.test_enabled == 0 {
            TEST_ALWAYS
        } else {
            alpha
                .nif_test_function
                .map_or(TEST_REFERENCE_THRESHOLD, u32::from)
        },
        test_ref: f32::from(alpha.test_ref),
        opacity: alpha.opacity,
        skip_zero_alpha: alpha.enabled == 1 && alpha.source == 6,
        opaque_texture: false,
        falloff: None,
        palette: 0,
        colour: None,
        lit: false,
        smooth: false,
    }
}

fn to_f16_bytes(rgba: &[f32], out: &mut Vec<u8>) {
    for v in rgba {
        out.extend_from_slice(&half::f16::from_f32(*v).to_le_bytes());
    }
}

/// `t` has passed validation (see `convert`), with its stored blocks when
/// it has them; their bytes are moved out, not copied.
fn texture(
    t: &fo4_render::Texture,
    blocks: Option<&mut fo4_render::Blocks>,
) -> Result<GpuTexture, String> {
    if let Some(blocks) = blocks {
        return Ok(GpuTexture {
            width: t.width,
            height: t.height,
            levels: blocks.levels,
            format: TexelFormat::Blocks(blocks.format),
            opaque: blocks.opaque,
            bytes: std::mem::take(&mut blocks.bytes),
        });
    }
    let total: usize = std::iter::once(t.rgba.len())
        .chain(t.mips.iter().map(|m| m.rgba.len()))
        .try_fold(0usize, |a, n| a.checked_add(n))
        .and_then(|n| n.checked_mul(2))
        .ok_or("TextureSizeOverflow")?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(total).map_err(|e| e.to_string())?;
    to_f16_bytes(&t.rgba, &mut bytes);
    // `Texture::validate` guarantees each listed mip is the next coarser level
    // of the chosen base (`m.mip` strictly increasing, sizes halving), and the
    // builder lists them from `base.mip + 1` without gaps, which is what a GPU
    // mip chain requires; a gap is refused rather than filled.
    for (k, m) in t.mips.iter().enumerate() {
        let expected = t.mip.checked_add(k as u32 + 1).ok_or("MipOverflow")?;
        if m.mip != expected {
            return Err(format!(
                "NonContiguousMipChain: base {} lists mip {} at position {k}",
                t.mip, m.mip
            ));
        }
        to_f16_bytes(&m.rgba, &mut bytes);
    }
    Ok(GpuTexture {
        width: t.width,
        height: t.height,
        levels: 1 + t.mips.len() as u32,
        format: TexelFormat::Rgba16Float,
        opaque: false,
        bytes,
    })
}

fn mesh(m: &fo4_render::Mesh) -> Result<GpuMesh, String> {
    if let Some(colors) = &m.vertex_colors {
        if colors.len() != m.vertices.len() || colors.iter().flatten().any(|v| !v.is_finite()) {
            return Err(format!("RenderVertexColorLayout: {}", colors.len()));
        }
    }
    let mut indices = Vec::new();
    let count = m
        .triangles
        .len()
        .checked_mul(3)
        .ok_or("RenderIndexCountOverflow")?;
    indices
        .try_reserve_exact(count)
        .map_err(|e| e.to_string())?;
    for tri in &m.triangles {
        for &i in tri {
            if usize::from(i) >= m.vertices.len() {
                return Err(format!("RenderVertexIndex: {i}"));
            }
            indices.push(i);
        }
    }
    let normals = match &m.normals {
        Some(n) => {
            if n.len() != m.vertices.len() || n.iter().flatten().any(|v| !v.is_finite()) {
                return Err(format!("RenderFalloffNormals: {}", n.len()));
            }
            Some(n.iter().map(|v| v.map(|x| x as f32)).collect())
        }
        None => None,
    };
    let mut positions = Vec::with_capacity(m.vertices.len());
    let mut uvs = Vec::with_capacity(m.vertices.len());
    for v in &m.vertices {
        if v.position.iter().chain(&v.uv).any(|x| !x.is_finite()) {
            return Err("RenderNonFiniteVertex".into());
        }
        positions.push(v.position.map(|x| x as f32));
        uvs.push(v.uv.map(|x| x as f32));
    }
    Ok(GpuMesh {
        positions,
        uvs,
        colors: m.vertex_colors.clone(),
        normals,
        indices,
    })
}

/// Convert a validated draw list. `options` must be the CPU options the GPU
/// frame is compared with; only bilinear filtering is implemented. Stored
/// blocks are moved out of `list`, which no CPU render can then draw.
pub fn convert(list: &mut DrawList, options: &RenderOptions) -> Result<GpuScene, String> {
    convert_lit(list, options, None)
}

/// [`convert`] with a frame's light: the draws `lit` marks get lit
/// surfaces of their own and the scene keeps the light's buffer, as the
/// CPU's `fo4_render::render_lit` lights them.
pub fn convert_lit(
    list: &mut DrawList,
    options: &RenderOptions,
    light: Option<(&fo4_scene::light::Light, &[bool])>,
) -> Result<GpuScene, String> {
    if let Some((_, lit)) = light {
        if lit.len() != list.draws.len() {
            return Err(format!(
                "RenderLighting: {} lit flags for {} draws",
                lit.len(),
                list.draws.len()
            ));
        }
    }
    if !options.bilinear {
        return Err(
            "UnsupportedViewerOption: the GPU path implements bilinear sampling only".into(),
        );
    }
    // The CPU validates every texture of the list before drawing; stored
    // blocks are validated as such.
    for i in 0..list.textures.len() {
        list.validate_texture(i).map_err(|e| e.to_string())?;
    }
    list.validate_overlays().map_err(|e| e.to_string())?;
    let mut out = GpuScene::default();
    let mut mesh_index = BTreeMap::new();
    let mut texture_index = BTreeMap::new();
    // Texture, palette, parameter bits, raw blend codes, culling, overlay,
    // and for a draw the clock drives its index (a surface of its own).
    type SurfaceIdentity = (
        usize,
        Option<usize>,
        [u32; PARAM_WORDS],
        Option<(u32, u32)>,
        bool,
        bool,
        Option<usize>,
    );
    let animations: BTreeMap<usize, &fo4_render::Animation> =
        list.animations.iter().map(|a| (a.draw, a)).collect();
    if animations.len() != list.animations.len()
        || animations.keys().any(|d| *d >= list.draws.len())
    {
        return Err("RenderAnimationDraws: one animation per draw of the list".into());
    }
    let mut surface_index: BTreeMap<SurfaceIdentity, usize> = BTreeMap::new();
    for (di, draw) in list.draws.iter().enumerate() {
        draw.alpha.validate().map_err(|e| e.to_string())?;
        let source = list
            .meshes
            .get(draw.mesh)
            .ok_or_else(|| format!("RenderMeshIndex: {}", draw.mesh))?;
        if draw.texture >= list.textures.len() {
            return Err(format!("RenderTextureIndex: {}", draw.texture));
        }
        if draw
            .uv_offset
            .iter()
            .chain(&draw.uv_scale)
            .any(|x| !x.is_finite())
            || !draw.world.scale.is_finite()
            || draw
                .world
                .rotation
                .iter()
                .flatten()
                .chain(&draw.world.translation)
                .any(|x| !x.is_finite())
        {
            return Err(format!("RenderNonFiniteTransform: {di}"));
        }
        if source.triangles.is_empty() {
            continue;
        }
        // As the CPU: a falloff reads the mesh's normals, a palette its
        // texture.
        if let Some(f) = draw.falloff {
            f.validate().map_err(|e| e.to_string())?;
            if source.normals.is_none() {
                return Err(format!("RenderFalloffNormals: {di}"));
            }
        }
        if let Some(p) = draw.palette {
            if p.texture >= list.textures.len() || list.blocks.contains_key(&p.texture) {
                return Err(format!("RenderPaletteTexture: {}", p.texture));
            }
        }
        let mesh_slot = match mesh_index.get(&draw.mesh) {
            Some(&i) => i,
            None => {
                out.meshes.push(mesh(source)?);
                mesh_index.insert(draw.mesh, out.meshes.len() - 1);
                out.meshes.len() - 1
            }
        };
        let mut slot = |t: usize, out: &mut GpuScene| -> Result<usize, String> {
            Ok(match texture_index.get(&t) {
                Some(&i) => i,
                None => {
                    out.textures
                        .push(texture(&list.textures[t], list.blocks.get_mut(&t))?);
                    texture_index.insert(t, out.textures.len() - 1);
                    out.textures.len() - 1
                }
            })
        };
        let texture_slot = slot(draw.texture, &mut out)?;
        let palette_slot = match draw.palette {
            Some(p) => Some(slot(p.texture, &mut out)?),
            None => None,
        };
        // The CPU culls only when asked, and then fails on a draw without a
        // sidedness declaration; the same error is raised here.
        let cull_back =
            options.backface_cull && !draw.sidedness.double_sided().map_err(|e| e.to_string())?;
        let blend_codes =
            (draw.alpha.enabled == 1).then_some((draw.alpha.source, draw.alpha.destination));
        let params = SurfaceParams {
            opaque_texture: out.textures[texture_slot].opaque,
            falloff: draw
                .falloff
                .map(|f| [f.start_angle, f.stop_angle, f.start_opacity, f.stop_opacity]),
            palette: draw
                .palette
                .map_or(0, |p| u32::from(p.colour) | u32::from(p.alpha) << 1),
            colour: draw.colour,
            lit: light.is_some_and(|(l, lit)| (l.lit || l.fog.is_some()) && lit[di]),
            smooth: light.is_some_and(|(l, lit)| (l.lit || l.fog.is_some()) && lit[di])
                && draw.falloff.is_none()
                && source.normals.is_some(),
            ..surface_params(draw.alpha, draw.uv_offset, draw.uv_scale)
        };
        let moved = animations.get(&di).and_then(|a| a.motion.clone());
        // A surface of its own only where a surface field is tracked.
        let animated = animations.get(&di).copied().filter(|a| a.has_tracks());
        let surface_key = (
            texture_slot,
            palette_slot,
            params.bits(),
            blend_codes,
            cull_back,
            list.is_overlay(di),
            animated.map(|_| di),
        );
        let surface = match surface_index.get(&surface_key) {
            Some(&i) => i,
            None => {
                let blend = match blend_codes {
                    Some((s, d)) => Some((blend_factor(s)?, blend_factor(d)?)),
                    None => None,
                };
                out.surfaces.push(Surface {
                    texture: texture_slot,
                    palette: palette_slot,
                    params,
                    key: SurfaceKey {
                        blend,
                        cull_back,
                        overlay: list.is_overlay(di),
                    },
                });
                surface_index.insert(surface_key, out.surfaces.len() - 1);
                if let Some(a) = animated {
                    out.animated.push(Animated {
                        surface: out.surfaces.len() - 1,
                        animation: a.clone(),
                        held: draw.clock_fields(),
                    });
                }
                out.surfaces.len() - 1
            }
        };
        let (translation, rotation, scale) = coords::instance_transform(&draw.world)
            .ok_or_else(|| format!("UnsupportedInstanceShear: draw {di} {:?}", draw.world))?;
        out.instances.push(Instance {
            mesh: mesh_slot,
            surface,
            tag: draw.instance,
            translation,
            rotation,
            scale,
        });
        if let Some(motion) = moved {
            out.moving.push(Moving {
                instance: out.instances.len() - 1,
                motion,
            });
        }
    }
    out.draw_meshes = mesh_index;
    out.light = light.map(|(l, _)| GpuLight {
        words: light_words(l),
        sky: l.sky.is_some(),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_render::{Draw, Mesh, Mip, Sidedness, Texture, Vertex};
    use fo4_scene::transform::Transform;

    fn alpha(enabled: u8, source: u32, destination: u32, test: Option<(u8, Option<u8>)>) -> Alpha {
        Alpha {
            enabled,
            source,
            destination,
            test_enabled: u8::from(test.is_some()),
            test_ref: test.map_or(0, |t| t.0),
            nif_test_function: test.and_then(|t| t.1),
            opacity: 0.5,
            nif_flags: None,
        }
    }
    fn draw(instance: u32, mesh: usize, texture: usize, alpha: Alpha) -> Draw {
        Draw {
            sidedness: Sidedness {
                material_two_sided: Some(0),
                nif_shader_flags_2: None,
            },
            instance,
            mesh,
            texture,
            world: Transform {
                translation: [instance as f64, 0., 0.],
                ..Transform::IDENTITY
            },
            alpha,
            uv_offset: [0.25, 0.],
            uv_scale: [2., 1.],
            raw_tile_flags: None,
            falloff: None,
            palette: None,
            colour: None,
        }
    }
    fn list() -> DrawList {
        let vertices = vec![
            Vertex {
                position: [0., 0., 0.],
                uv: [0., 0.],
            },
            Vertex {
                position: [1., 0., 0.],
                uv: [1., 0.],
            },
            Vertex {
                position: [0., 1., 0.],
                uv: [0., 1.],
            },
        ];
        DrawList {
            meshes: vec![Mesh {
                vertex_colors: None,
                normals: None,
                vertices,
                triangles: vec![[0, 1, 2]],
            }],
            textures: vec![Texture {
                mips: vec![Mip {
                    width: 1,
                    height: 1,
                    rgba: vec![0.5; 4],
                    mip: 1,
                }],
                width: 2,
                height: 2,
                rgba: vec![0.25; 16],
                mip: 0,
                srgb: false,
            }],
            draws: vec![
                draw(7, 0, 0, alpha(0, 0, 0, Some((128, None)))),
                draw(8, 0, 0, alpha(0, 0, 0, Some((128, None)))),
                draw(9, 0, 0, alpha(1, 6, 7, None)),
            ],
            overlays: Vec::new(),
            blocks: Default::default(),
            animations: Vec::new(),
        }
    }

    #[test]
    fn meshes_textures_and_equal_surfaces_are_shared() {
        let scene = convert(&mut list(), &RenderOptions::REFERENCE).unwrap();
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.textures.len(), 1);
        assert_eq!(scene.surfaces.len(), 2);
        assert_eq!(scene.instances.len(), 3);
        assert_eq!(scene.instances[0].surface, scene.instances[1].surface);
        assert_eq!(
            scene.instances.iter().map(|i| i.tag).collect::<Vec<_>>(),
            [7, 8, 9]
        );
        assert_eq!(scene.blended_instances(), 1);
        let t = &scene.textures[0];
        assert_eq!((t.width, t.height, t.levels), (2, 2, 2));
        assert_eq!(t.bytes.len(), (16 + 4) * 2);
        assert_eq!(&t.bytes[..2], &half::f16::from_f32(0.25).to_le_bytes());
        assert_eq!(&t.bytes[32..34], &half::f16::from_f32(0.5).to_le_bytes());
    }

    #[test]
    fn falloff_and_palette_draws_carry_their_terms_and_need_their_data() {
        let falloff = fo4_render::Falloff {
            start_angle: 1.,
            stop_angle: 0.,
            start_opacity: 1.,
            stop_opacity: 0.,
        };
        let mut l = list();
        l.draws[1].falloff = Some(falloff);
        // Without normals the draw is refused, as by the CPU.
        let e = convert(&mut l, &RenderOptions::REFERENCE).err().unwrap();
        assert!(e.starts_with("RenderFalloffNormals"), "{e}");
        l.meshes[0].normals = Some(vec![[0., 0., 1.]; 3]);
        l.textures.push(Texture {
            mips: Vec::new(),
            width: 2,
            height: 1,
            rgba: vec![0., 0., 0., 0., 1., 1., 1., 1.],
            mip: 0,
            srgb: false,
        });
        l.draws[1].palette = Some(fo4_render::Palette {
            texture: 1,
            colour: false,
            alpha: true,
        });
        let scene = convert(&mut l, &RenderOptions::REFERENCE).unwrap();
        assert_eq!(
            scene.instances.iter().map(|i| i.tag).collect::<Vec<_>>(),
            [7, 8, 9]
        );
        assert_eq!(scene.meshes[0].normals.as_ref().unwrap()[0], [0., 0., 1.]);
        let s = &scene.surfaces[scene.instances[1].surface];
        assert_eq!(s.params.falloff, Some([1., 0., 1., 0.]));
        assert_eq!(s.params.palette, 2);
        let palette = s.palette.unwrap();
        assert_eq!(scene.textures[palette].width, 2);
        assert_ne!(palette, s.texture);
        // The other draws keep their shared surface without either term.
        let plain = &scene.surfaces[scene.instances[0].surface];
        assert_eq!((plain.params.falloff, plain.palette), (None, None));
        assert_ne!(scene.instances[0].surface, scene.instances[1].surface);
        // A palette kept as stored blocks is refused: the lookup reads
        // decoded texels.
        let mut l = list();
        l.draws[0].palette = Some(fo4_render::Palette {
            texture: 5,
            colour: true,
            alpha: false,
        });
        let e = convert(&mut l, &RenderOptions::REFERENCE).err().unwrap();
        assert!(e.starts_with("RenderPaletteTexture"), "{e}");
    }

    #[test]
    fn stored_blocks_are_moved_and_carry_their_opaque_alpha() {
        let mut l = list();
        l.textures.push(Texture {
            mips: Vec::new(),
            width: 8,
            height: 4,
            rgba: Vec::new(),
            mip: 1,
            srgb: false,
        });
        l.blocks.insert(
            1,
            fo4_render::Blocks {
                format: BlockFormat::Bc1,
                levels: 3,
                bytes: (0..32).collect(),
                opaque: true,
            },
        );
        l.draws[2].texture = 1;
        let scene = convert(&mut l, &RenderOptions::REFERENCE).unwrap();
        let t = &scene.textures[1];
        assert_eq!(
            (t.width, t.height, t.levels, t.format, t.opaque),
            (8, 4, 3, TexelFormat::Blocks(BlockFormat::Bc1), true)
        );
        assert_eq!(t.bytes, (0..32).collect::<Vec<u8>>());
        assert!(l.blocks[&1].bytes.is_empty());
        assert!(
            scene.surfaces[scene.instances[2].surface]
                .params
                .opaque_texture
        );
        assert!(
            !scene.surfaces[scene.instances[0].surface]
                .params
                .opaque_texture
        );
        assert_eq!(scene.textures[0].format, TexelFormat::Rgba16Float);
        // Blocks that do not fit their texture are refused.
        let mut bad = list();
        bad.blocks.insert(
            0,
            fo4_render::Blocks {
                format: BlockFormat::Bc1,
                levels: 1,
                bytes: vec![0; 8],
                opaque: false,
            },
        );
        assert!(convert(&mut bad, &RenderOptions::REFERENCE)
            .err()
            .unwrap()
            .contains("InvalidBlockTexture"));
    }

    #[test]
    fn alpha_fields_follow_the_cpu_reading() {
        let scene = convert(&mut list(), &RenderOptions::REFERENCE).unwrap();
        let tested = &scene.surfaces[scene.instances[0].surface];
        assert_eq!(tested.params.test_function, TEST_REFERENCE_THRESHOLD);
        assert_eq!(tested.params.test_ref, 128.);
        assert_eq!(tested.params.uv_scale, [2., 1.]);
        assert_eq!(tested.params.uv_offset, [0.25, 0.]);
        assert_eq!(tested.key.blend, None);
        assert!(!tested.params.skip_zero_alpha);
        let blended = &scene.surfaces[scene.instances[2].surface];
        assert_eq!(blended.params.test_function, TEST_ALWAYS);
        assert_eq!(
            blended.key.blend,
            Some((BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha))
        );
        assert!(blended.params.skip_zero_alpha);
        let nif = surface_params(alpha(0, 0, 0, Some((10, Some(4)))), [0.; 2], [1.; 2]);
        assert_eq!(nif.test_function, 4);
    }

    #[test]
    fn culling_follows_the_cpu_option_and_its_errors() {
        let mut options = RenderOptions::REFERENCE;
        let scene = convert(&mut list(), &options).unwrap();
        assert!(scene.surfaces.iter().all(|s| !s.key.cull_back));
        options.backface_cull = true;
        let scene = convert(&mut list(), &options).unwrap();
        assert!(scene.surfaces.iter().all(|s| s.key.cull_back));
        let mut undeclared = list();
        undeclared.draws[0].sidedness = Sidedness::default();
        let e = convert(&mut undeclared, &options).err().unwrap();
        assert!(e.contains("MissingCullDeclarations"), "{e}");
    }

    #[test]
    fn overlay_draws_get_their_own_surface_and_keep_the_old_hash_otherwise() {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut l = list();
        l.overlays = vec![2];
        let scene = convert(&mut l, &RenderOptions::REFERENCE).unwrap();
        let keys: Vec<_> = scene
            .instances
            .iter()
            .map(|i| scene.surfaces[i.surface].key.overlay)
            .collect();
        assert_eq!(keys, [false, false, true]);
        for bad in [vec![2, 2], vec![3]] {
            l.overlays = bad;
            let e = convert(&mut l, &RenderOptions::REFERENCE).err().unwrap();
            assert!(e.contains("InvalidOverlayDraws"), "{e}");
        }
        // Without overlay the key hashes as the two-field key did, so the
        // pipelines of a list without overlays are unchanged.
        let state = std::hash::RandomState::new();
        let key = SurfaceKey {
            blend: Some((BlendFactor::SrcAlpha, BlendFactor::One)),
            cull_back: true,
            overlay: false,
        };
        let a = state.hash_one(key);
        let mut b = state.build_hasher();
        key.blend.hash(&mut b);
        key.cull_back.hash(&mut b);
        assert_eq!(a, b.finish());
        let c = state.hash_one(SurfaceKey {
            overlay: true,
            ..key
        });
        assert_ne!(a, c);
    }

    #[test]
    fn cpu_refusals_are_refusals_here() {
        let mut bad = list();
        bad.meshes[0].triangles[0][2] = 3;
        assert!(convert(&mut bad, &RenderOptions::REFERENCE)
            .err()
            .unwrap()
            .contains("RenderVertexIndex"));
        let mut bad = list();
        bad.draws[0].alpha.source = 11;
        bad.draws[0].alpha.enabled = 1;
        assert!(convert(&mut bad, &RenderOptions::REFERENCE).is_err());
        let mut gap = list();
        gap.textures[0].mips[0].mip = 2;
        gap.textures[0].mips[0].width = 1;
        assert!(convert(&mut gap, &RenderOptions::REFERENCE).is_err());
        let mut nearest = RenderOptions::REFERENCE;
        nearest.bilinear = false;
        assert!(convert(&mut list(), &nearest).is_err());
    }

    #[test]
    fn every_cpu_blend_code_has_a_factor() {
        let expected = [
            BlendFactor::One,
            BlendFactor::Zero,
            BlendFactor::Src,
            BlendFactor::OneMinusSrc,
            BlendFactor::Dst,
            BlendFactor::OneMinusDst,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
            BlendFactor::SrcAlphaSaturated,
        ];
        for (code, want) in expected.iter().enumerate() {
            assert_eq!(blend_factor(code as u32).unwrap(), *want);
        }
        assert!(blend_factor(11).is_err());
    }
}
