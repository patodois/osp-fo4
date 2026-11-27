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
mod effect;
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
    /// Instance indices of the placed actors, after the placed references.
    pub actors: Vec<u32>,
    /// Shapes of drawn actor models without a skin, not drawn.
    pub actor_rigid_shapes: u64,
    /// Heads of drawn actors, or parts of them, left out, with the causes.
    pub actor_head_omissions: Vec<Omission>,
    pub list: DrawList,
    pub instances: Vec<Identity>,
    pub omitted: Vec<Omission>,
    pub texture_choices: Vec<TextureChoice>,
    /// Indices into `list.draws` of the draws of effect shader surfaces,
    /// increasing: a frame's light leaves them unlit (`fo4_scene::light`).
    pub effect_draws: Vec<usize>,
    /// With [`BuildOptions::chains`]: the node chain of each draw of a
    /// placed model's shape, in draw order; empty otherwise.
    pub chains: Vec<DrawChain>,
    /// The shapes of the play run's named platforms the draw refused and
    /// left out (`play_platform_shapes_left_out_v1`): the instance and the
    /// refusal's code. Empty for every other instance.
    pub left_out: Vec<(u32, String)>,
}
/// The node chain a draw of a model's shape hangs under
/// ([`BuildOptions::chains`]): for a caller that moves the draws of some
/// nodes itself (the play window's named platforms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawChain {
    /// Index into `list.draws`.
    pub draw: usize,
    /// The model's asset path, as the loader holds it.
    pub model: Vec<u8>,
    /// The model's blocks from the root to the shape.
    pub blocks: Vec<usize>,
    /// For a skinned shape drawn on its model's own nodes (a named
    /// platform's): each bone's chain of blocks from the root; empty for a
    /// rigid shape.
    pub bones: Vec<Vec<usize>>,
}
/// One skinned actor mesh of a posed build ([`build_with_posed_actors`]),
/// kept so that a caller can pose it again: the draw-list mesh, the skeleton
/// asset that poses it and the shape's skin.
#[derive(Debug, Clone)]
pub struct SkinnedMesh {
    pub mesh: usize,
    pub skeleton: usize,
    pub shape: fo4_scene::actor::pose::SkinnedShape,
}
#[derive(Clone, Copy)]
pub struct BuildOptions {
    pub max_texture_dimension: u32,
    pub max_texture_bytes: u64,
    /// Instances whose state is not ready at this level are omitted with a cause.
    pub level: fo4_scene::RuleLevel,
    /// Keep block-compressed textures as their stored blocks (for a GPU
    /// consumer) instead of decoding them; `None` in the reference.
    pub blocks: Option<BlockOptions>,
    /// Seconds of the game clock since the models loaded: free-running
    /// controllers of shader UV fields and alpha are evaluated at this time
    /// under the clock rules (`fo4_scene::clock`) and recorded in
    /// `DrawList::animations`. `None` (the reference): no clock; an instance
    /// that relies on `nif_controller_clock_free_running_v1` is omitted
    /// (`UnsettledControllerTime`) and every other field is drawn as before.
    pub time: Option<f64>,
    /// The clock's readings that are off by default
    /// (`fo4_scene::clock::Switches`): with a time, the effect fields whose
    /// switch is on are carried by the draw's animation instead of being
    /// held at their stored value. The scene must be loaded with the same
    /// switches (`fo4_scene::Loader::set_clock_switches`).
    pub clock: fo4_scene::clock::Switches,
    /// `light_vertex_normals_v1` (`--light-vertex-normals`): every mesh
    /// that stores vertex normals keeps them (not only falloff draws'), so
    /// that a lit draw is lit with them; skinned or posed meshes do not
    /// (their posed normals are not computed). Off: as before.
    pub vertex_normals: bool,
    /// Keep each model shape draw's node chain ([`Build::chains`]). Off:
    /// none kept, the list as before.
    pub chains: bool,
    /// `effect_soft_depth_fade_linear_v1` (`--draw-soft-effects`): an
    /// effect surface whose shader or material sets its soft flag carries
    /// its soft falloff depth as stored ([`Draw::soft_depth`]); the frame
    /// fades its alpha by the gap to the opaque depth behind it. Off: the
    /// term is left out (`effect_soft_lighting_left_out_v1`), as before.
    pub soft_effects: bool,
    /// `effect_environment_left_out_v1` for an untextured effect surface
    /// (`--draw-effect-environment-left-out`): a colour-only effect shader
    /// whose one feature this frame does not draw is its environment map
    /// (Shader Flags 1 bit 7, the environment map and mask its only
    /// textures) is drawn by its colour with that term left out, as the
    /// effect path draws it. Off: refused, as before.
    pub effect_environment_left_out: bool,
    /// `lighting_refraction_surface_left_out_v1`
    /// (`--draw-refraction-left-out`): a shape whose lighting shader sets
    /// Shader Flags 1 bit 15 (nifxml: Refraction) is left out
    /// ([`Build::left_out`], `RefractionSurface`): its texture set's first
    /// slot is the normal map that displaces the scene behind it, not a
    /// colour. Off: drawn with that map as its colour, as before.
    pub refraction_left_out: bool,
}
/// Limits of the textures kept as stored blocks ([`BuildOptions::blocks`]).
/// A texture whose format or chosen level cannot be kept as blocks is
/// decoded under the reference limits instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockOptions {
    /// Largest side of the first level kept.
    pub max_dimension: u32,
    /// The same for a texture drawn with an alpha test.
    pub max_alpha_tested_dimension: u32,
    /// Total stored bytes of the kept levels, apart from `max_texture_bytes`.
    pub max_bytes: u64,
}
impl BuildOptions {
    /// Texture limits of the reference renders (the `render` example, and the
    /// GPU viewer with `--reference-textures`).
    pub const fn reference(level: fo4_scene::RuleLevel) -> Self {
        Self {
            max_texture_dimension: 256,
            max_texture_bytes: 96 << 20,
            level,
            blocks: None,
            time: None,
            clock: fo4_scene::clock::Switches::OFF,
            vertex_normals: false,
            chains: false,
            soft_effects: false,
            effect_environment_left_out: false,
            refraction_left_out: false,
        }
    }
    /// The reference limits, with block-compressed textures kept as blocks.
    pub const fn with_blocks(level: fo4_scene::RuleLevel, blocks: BlockOptions) -> Self {
        Self {
            blocks: Some(blocks),
            ..Self::reference(level)
        }
    }
    /// The largest first-level side of a texture kept as blocks, by whether
    /// it is drawn with an alpha test; `None` in the reference.
    pub fn block_dimension(&self, alpha_tested: bool) -> Option<u32> {
        self.blocks.map(|b| {
            if alpha_tested {
                b.max_alpha_tested_dimension
            } else {
                b.max_dimension
            }
        })
    }
}
/// The four readings of a path under swap scopes `swaps` (outermost
/// first), with `one` applying a single swap: the outermost alone, the
/// innermost alone, every swap from the outermost in, and from the
/// innermost out (`mswp_scopes_readings_agree_v1`).
fn readings<S>(
    path: &[u8],
    swaps: &[S],
    b: &Budget,
    mut one: impl FnMut(Vec<u8>, &S) -> Result<Vec<u8>>,
) -> Result<[Vec<u8>; 4]> {
    let (Some(first), Some(last)) = (swaps.first(), swaps.last()) else {
        return Err(error("RenderSwapPair", "material swap", 0, "no swap"));
    };
    let owned = || -> Result<Vec<u8>> {
        let mut v = Vec::new();
        b.reserve(&mut v, path.len(), "material swap", 0)?;
        v.extend_from_slice(path);
        Ok(v)
    };
    let outer = one(owned()?, first)?;
    let inner = one(owned()?, last)?;
    let mut down = owned()?;
    for s in swaps {
        down = one(down, s)?;
    }
    let mut up = owned()?;
    for s in swaps.iter().rev() {
        up = one(up, s)?;
    }
    Ok([outer, inner, down, up])
}
/// How two or more swap scopes settle one path where the four
/// [`readings`] differ.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Settled {
    /// No scope maps a material that another scope put in its place: the
    /// outermost swap alone gives the material. `outer` names
    /// `mswp_outer_scope_wins_v1` (the outermost swap maps the path), and
    /// `replace` names `mswp_outer_scope_replaces_v1` (it does not, and an
    /// inner swap that maps it is left out).
    Ruled {
        material: Vec<u8>,
        outer: bool,
        replace: bool,
    },
    /// A scope maps the material another scope put in place: the order of
    /// application decides. `material` follows the chain
    /// (`mswp_chain_followed_v1`): from the path the model names, the
    /// outermost scope not yet applied that maps the current material
    /// applies, each scope once.
    Chained { material: Vec<u8> },
}
/// [`Settled`] for `path` under swap scopes `swaps` (outermost first), with
/// `one` applying a single swap: the outermost swap alone, compared with the
/// path the model names, unless a swap maps what another puts in place.
fn settle<S>(
    path: &[u8],
    swaps: &[S],
    b: &Budget,
    mut one: impl FnMut(Vec<u8>, &S) -> Result<Vec<u8>>,
) -> Result<Settled> {
    let owned = |p: &[u8]| -> Result<Vec<u8>> {
        let mut v = Vec::new();
        b.reserve(&mut v, p.len(), "material swap", 0)?;
        v.extend_from_slice(p);
        Ok(v)
    };
    let mut targets = Vec::new();
    b.reserve(&mut targets, swaps.len(), "material swap", 0)?;
    for s in swaps {
        targets.push(one(owned(path)?, s)?);
    }
    // A target another scope maps again: the readings depend on the order.
    for (k, t) in targets.iter().enumerate() {
        if t.as_slice() == path {
            continue;
        }
        for (j, s) in swaps.iter().enumerate() {
            if j != k && one(owned(t)?, s)? != *t {
                let mut used = Vec::new();
                b.reserve(&mut used, swaps.len(), "material swap", 0)?;
                used.resize(swaps.len(), false);
                let mut current = owned(path)?;
                'follow: loop {
                    for (i, s) in swaps.iter().enumerate() {
                        if used[i] {
                            continue;
                        }
                        let next = one(owned(&current)?, s)?;
                        if next != current {
                            used[i] = true;
                            current = next;
                            continue 'follow;
                        }
                    }
                    break;
                }
                return Ok(Settled::Chained { material: current });
            }
        }
    }
    let Some(first) = targets.first() else {
        return Err(error("RenderSwapPair", "material swap", 0, "no swap"));
    };
    let outer = first.as_slice() != path;
    let inner = targets[1..].iter().any(|t| t.as_slice() != path);
    Ok(Settled::Ruled {
        material: owned(first)?,
        outer,
        replace: !outer && inner,
    })
}
fn error(code: &str, file: &str, offset: u64, found: impl std::fmt::Display) -> Error {
    Error::new(code, file, offset, "documented unlit draw binding", found)
}
/// [`Build::left_out`]'s code for a refraction surface left out
/// (`lighting_refraction_surface_left_out_v1`).
pub const REFRACTION_SURFACE: &str = "RefractionSurface";

/// Shader Flags 1's Refraction bit (bit 15, nifxml's
/// `Fallout4ShaderPropertyFlags1`).
const REFRACTION_BIT: u32 = 1 << 15;

/// Whether a shape (its fields `shape`) is drawn by a lighting shader
/// whose Shader Flags 1 sets the Refraction bit.
pub(crate) fn refraction_surface(n: &Nif<'_>, shape: &[Field]) -> bool {
    shape.iter().any(|f| {
        let Value::Ref(i) = f.value else {
            return false;
        };
        let Some(block) = usize::try_from(i).ok().and_then(|i| n.blocks.get(i)) else {
            return false;
        };
        if n.block_types
            .get(block.type_index as usize)
            .map(String::as_str)
            != Some("BSLightingShaderProperty")
        {
            return false;
        }
        let Some(d) = block.decoded.as_ref() else {
            return false;
        };
        fields(d).iter().any(|f| {
            f.name == "Shader Flags 1"
                && matches!(f.value, Value::U32(x) if x & REFRACTION_BIT != 0)
        })
    })
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
    /// Effect surfaces (`draw::effect`): base colour times its scale,
    /// multiplied into the vertex colours.
    tint: Option<[f32; 3]>,
    /// Effect surfaces drawn with their falloff; the mesh keeps normals.
    falloff: Option<crate::Falloff>,
    /// Effect surfaces drawn with a greyscale-to-palette lookup.
    palette: Option<crate::Palette>,
    /// The clock tracks of the surface's fields (`crate::Animation`).
    clock: [Option<fo4_scene::clock::Track>; crate::TRACKS],
    /// `Draw::colour` (an effect colour the clock drives) and the held
    /// base colour and scale of its animation.
    colour: Option<[f32; 3]>,
    colour_terms: ([f32; 3], f32),
    /// An effect shader's surface (not a lighting shader's): drawn unlit
    /// under a frame's light (`Build::effect_draws`).
    effect: bool,
    /// Its blending is enabled by a material file's fields (their factor
    /// numbering under `bgsm_blend_factors_alpha_function_v1`), not by the
    /// model's own alpha property.
    material_blend: bool,
    /// [`Draw::soft_depth`].
    soft_depth: Option<f32>,
}
struct Builder<'a, 's> {
    loader: &'a Loader<'s>,
    limits: &'a Limits,
    budget: &'a Budget,
    options: BuildOptions,
    output: Build,
    /// By model, shape, effect channels, UV use, the effect tint's bits,
    /// for a skinned mesh the skeleton asset it is posed by, and whether
    /// it keeps normals (a falloff draw).
    #[allow(clippy::type_complexity)]
    meshes: BTreeMap<
        (
            usize,
            usize,
            u8,
            bool,
            Option<[u32; 3]>,
            Option<usize>,
            bool,
        ),
        usize,
    >,
    skeletons: BTreeMap<usize, fo4_scene::actor::pose::Skeleton>,
    /// Skeleton assets posed by the caller, whose meshes are recorded.
    posed: std::collections::BTreeSet<usize>,
    recorded: Vec<SkinnedMesh>,
    /// By path and the block limit applied (0 when decoded).
    textures: BTreeMap<(Vec<u8>, u32), usize>,
    texture_bytes: u64,
    block_bytes: u64,
    visits: u64,
    allow_path_strip: bool,
    allow_path_root: bool,
    allow_path_relative: bool,
    allow_effect_color: bool,
    /// The instance relies on `nif_controller_rest_effect_first_key_v1`:
    /// its effect shaders are drawn by `draw::effect`.
    allow_effect_surface: bool,
    /// The instance relies on `nif_missing_material_embedded_shader_v1`: a
    /// lighting shader whose material file is missing is drawn with its own
    /// fields and texture set.
    allow_missing_embedded: bool,
    /// The instance relies on `nif_controller_rest_texture_first_key_v1`: a
    /// lighting shader whose UV fields a controller drives is drawn with its
    /// stored fields only where a material it names holds the same values.
    texture_first_key: bool,
    /// The instance relies on `nif_controller_clock_free_running_v1`.
    clock_rest: bool,
    /// The instance relies on a transform channel rule of the clock
    /// (`nif_clock_transform_*_v1`): nodes that free-running transform
    /// controllers move are drawn at the build time.
    transform_clock: bool,
    /// The moving nodes of the model being drawn, by node block, with
    /// their tracks (`fo4_scene::rest::transform_clock`).
    node_tracks: BTreeMap<usize, fo4_scene::clock::TransformTrack>,
    allow_scol_parts: bool,
    allow_editor_marker_subtrees: bool,
    allow_marker_suffix: bool,
    include_marker_subtrees: bool,
    allow_visibility_marker: bool,
    allow_swap_folder: bool,
    /// The instance relies on `mswp_empty_entry_inert_v1`.
    allow_swap_empty: bool,
    /// Swap sources holding `*` are read as patterns
    /// (`mswp_wildcard_pattern_v1` usable at the build's level).
    allow_swap_pattern: bool,
    /// The instance relies on `mswp_wildcard_missing_target_inert_v1`.
    allow_swap_pattern_missing: bool,
    /// Pattern rules [`Self::swap_one`] relied on since they were last
    /// recorded ([`Self::pattern_rules`]): bit 0 a pattern gave the
    /// material, bit 1 a literal source won over a pattern.
    pattern_used: std::cell::Cell<u8>,
    /// Meshes built with triangles left out for a texture coordinate that
    /// is not finite (`nif_uv_nonfinite_triangles_left_out_v1`).
    uv_left_out: std::collections::BTreeSet<usize>,
    /// Registered rules the current instance's draws relied on beyond its
    /// state's (`Identity::hypotheses` gains them).
    draw_rules: Vec<&'static str>,
    /// The actor being drawn skins a bone its skeleton lacks at its bind
    /// transform (`fo4_scene::actor::Actor::missing_bones_at_bind`).
    missing_bones_at_bind: bool,
    /// With it, such a bone skinned as the nearest bone of its skin the
    /// skeleton holds (`fo4_scene::actor::Actor::missing_bones_nearest`).
    missing_bones_nearest: bool,
    /// The instance or actor drawn is a named platform's
    /// (`play_platform_shapes_left_out_v1` among its rules): a shape the
    /// draw refuses is left out ([`Build::left_out`]) instead of refusing
    /// the instance, and its rigid traversal leaves skinned shapes to the
    /// actor drawn on the model's own nodes.
    /// Under [`Builder::platform`] with [`BuildOptions::chains`]: each
    /// block's chain from the root in the actor model being drawn.
    node_chains: BTreeMap<usize, Vec<usize>>,
    platform: bool,
    /// The instance drawn is a static under `static_unmoved_parts_drawn_v1`
    /// (`fo4_scene::parts`): its model's reached shapes, particle systems
    /// and material-blended shapes are left out ([`Build::left_out`]), and
    /// so is a shape the draw refuses.
    static_parts: bool,
    /// `furn_parts_drawn_at_rest_v1`: the parts read at rest
    /// (`fo4_scene::parts::read_with`).
    parts_at_rest: bool,
    /// Under [`Builder::static_parts`], the parts of the model being drawn.
    parts: Option<fo4_scene::parts::Parts>,
}
/// A diffuse texture decoded under the reference limits, with the DDS
/// declarations the summary reports.
pub struct DecodedTexture {
    /// With stored blocks, `rgba` and `mips` are empty and `blocks` holds
    /// the texels.
    pub texture: Texture,
    pub blocks: Option<crate::Blocks>,
    pub declared_dxgi: Option<u32>,
    pub declared_fourcc: u32,
}
/// Decodes texture asset `id` of `loader` as the draw-list builder does: the
/// first single-surface level not larger than `options.max_texture_dimension`
/// (else the smallest), then every coarser level while the running total
/// `texture_bytes` stays within `options.max_texture_bytes`. Returns the
/// texture before `Texture::validate`.
pub fn decode_texture(
    loader: &Loader<'_>,
    id: usize,
    options: BuildOptions,
    texture_bytes: &mut u64,
    limits: &Limits,
    budget: &Budget,
) -> Result<DecodedTexture> {
    let scratch = budget.child();
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
                    && s.width <= options.max_texture_dimension
                    && s.height <= options.max_texture_dimension
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
                    options.max_texture_dimension,
                )
            })?;
        let s = &t.surfaces[index];
        let bytes = fo4_core::mul(
            fo4_core::mul(s.width as u64, s.height as u64, "texture", 0)?,
            16,
            "texture",
            0,
        )?;
        let total = fo4_core::add(*texture_bytes, bytes, "diffuse texture budget", 0)?;
        fo4_core::limit(
            total,
            options.max_texture_bytes,
            "diffuse texture budget",
            0,
        )?;
        let rgba = t.decode(index, limits, budget)?;
        *texture_bytes = total;
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
            let total = fo4_core::add(*texture_bytes, size, "mip budget", 0)?;
            if total > options.max_texture_bytes {
                break;
            }
            let rgba = t.decode(i, limits, budget)?;
            append(
                &mut mips,
                crate::Mip {
                    width: level.width,
                    height: level.height,
                    rgba,
                    mip: level.mip,
                },
                budget,
            )?;
            *texture_bytes = total;
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
    Ok(DecodedTexture {
        texture,
        blocks: None,
        declared_dxgi,
        declared_fourcc,
    })
}
/// The mip number of the level [`block_texture`] would keep of texture
/// asset `id` under `max_dimension` (the first single-surface level whose
/// sides are at most it), without keeping anything; `None` where it would
/// keep none at that first step.
pub fn block_level(
    loader: &Loader<'_>,
    id: usize,
    max_dimension: u32,
    budget: &Budget,
) -> Result<Option<u32>> {
    let scratch = budget.child();
    loader.assets.with_decoded(id, &scratch, |d| {
        let DecodedAsset::Texture(t) = d else {
            return Err(error("RenderAssetKind", "texture", 0, id));
        };
        Ok(t.surfaces
            .iter()
            .find(|s| {
                s.array == 0
                    && s.face == 0
                    && s.depth == 1
                    && s.width <= max_dimension
                    && s.height <= max_dimension
            })
            .map(|s| s.mip))
    })
}
/// Keeps texture asset `id` as its stored blocks: the first single-surface
/// level whose sides are at most `max_dimension`, and every coarser stored
/// level after it, counted in `block_bytes` against `max_bytes`. `Ok(None)`
/// when the texture cannot be kept so (not a 2D texture, a layout without a
/// GPU block format, no such level, or a level whose sides are not
/// multiples of 4); the caller then decodes it.
pub fn block_texture(
    loader: &Loader<'_>,
    id: usize,
    max_dimension: u32,
    max_bytes: u64,
    block_bytes: &mut u64,
    budget: &Budget,
) -> Result<Option<DecodedTexture>> {
    use fo4_texture::Format;
    let scratch = budget.child();
    loader.assets.with_decoded(id, &scratch, |d| {
        let DecodedAsset::Texture(t) = d else {
            return Err(error("RenderAssetKind", "texture", 0, id));
        };
        if t.cubemap || t.array_size != 1 || t.depth != 1 {
            return Ok(None);
        }
        let (format, opaque) = match t.format {
            Format::Bc1 => (crate::BlockFormat::Bc1, true),
            Format::Bc1Alpha => (crate::BlockFormat::Bc1Alpha, false),
            Format::Bc2 => (crate::BlockFormat::Bc2, false),
            Format::Bc3 => (crate::BlockFormat::Bc3, false),
            Format::Bc4 => (crate::BlockFormat::Bc4, false),
            Format::Bc5 => (crate::BlockFormat::Bc5, false),
            Format::Bc7 => (crate::BlockFormat::Bc7, false),
            _ => return Ok(None),
        };
        let plain = |s: &fo4_texture::Surface| s.array == 0 && s.face == 0 && s.depth == 1;
        let Some(index) = t
            .surfaces
            .iter()
            .position(|s| plain(s) && s.width <= max_dimension && s.height <= max_dimension)
        else {
            return Ok(None);
        };
        let s = &t.surfaces[index];
        if !s.width.is_multiple_of(4) || !s.height.is_multiple_of(4) {
            return Ok(None);
        }
        let mut chosen = Vec::new();
        for (i, level) in t.surfaces.iter().enumerate() {
            if !plain(level) || level.mip < s.mip {
                continue;
            }
            let k = level.mip - s.mip;
            if k as usize != chosen.len()
                || level.width != (s.width >> k.min(31)).max(1)
                || level.height != (s.height >> k.min(31)).max(1)
            {
                return Ok(None);
            }
            chosen.push(i);
        }
        let mut size = 0u64;
        for &i in &chosen {
            let level = &t.surfaces[i];
            let n = t
                .surface_bytes(i)
                .ok_or_else(|| error("RenderBlockSurface", "texture", 0, i))?
                .len() as u64;
            if n != format.level_bytes(level.width, level.height) {
                return Ok(None);
            }
            size = fo4_core::add(size, n, "block texture", 0)?;
        }
        let total = fo4_core::add(*block_bytes, size, "block texture budget", 0)?;
        fo4_core::limit(total, max_bytes, "block texture budget", 0)?;
        let mut bytes = Vec::new();
        budget.reserve(&mut bytes, size as usize, "block texture", 0)?;
        for &i in &chosen {
            bytes.extend_from_slice(t.surface_bytes(i).unwrap_or_default());
        }
        *block_bytes = total;
        Ok(Some(DecodedTexture {
            declared_dxgi: t.dx10.map(|d| d[0]),
            declared_fourcc: t.header[20],
            texture: Texture {
                mips: Vec::new(),
                width: s.width,
                height: s.height,
                rgba: Vec::new(),
                mip: s.mip,
                srgb: t.srgb,
            },
            blocks: Some(crate::Blocks {
                format,
                levels: chosen.len() as u32,
                bytes,
                opaque,
            }),
        }))
    })
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
        self.loader.assets.find(kind, path).ok_or_else(|| {
            error(
                "RenderAssetNotInScene",
                &fo4_core::HexBytes(path).to_string(),
                0,
                format!("{kind:?}"),
            )
        })
    }
    /// The draw-list texture of `path`; `alpha_tested` selects the block
    /// limit when textures are kept as blocks, and is ignored otherwise.
    fn texture(&mut self, path: Vec<u8>, alpha_tested: bool) -> Result<usize> {
        let cap = self.options.block_dimension(alpha_tested);
        self.texture_at(path, cap)
    }
    /// The draw-list texture of `path` as decoded texels whatever the
    /// options: an effect surface's palette, which is sampled at its base
    /// level with clamped coordinates, never mipmapped.
    pub(super) fn decoded_texture(&mut self, path: Vec<u8>) -> Result<usize> {
        self.texture_at(path, None)
    }
    /// The draw-list texture of `path`, kept as stored blocks up to `cap`
    /// when the options keep blocks and `cap` is given.
    fn texture_at(&mut self, path: Vec<u8>, cap: Option<u32>) -> Result<usize> {
        if let Some(i) = self.textures.get(&(path.clone(), cap.unwrap_or(0))) {
            return Ok(*i);
        }
        let id = self.asset(AssetKind::Texture, &path)?;
        // The same path kept under the other block cap with the same first
        // level holds the same blocks: that entry is shared, not kept twice.
        if let (Some(cap), Some(blocks)) = (cap, self.options.blocks) {
            for other in [blocks.max_dimension, blocks.max_alpha_tested_dimension] {
                let Some(&i) = self.textures.get(&(path.clone(), other)) else {
                    continue;
                };
                if other == cap || !self.output.list.blocks.contains_key(&i) {
                    continue;
                }
                if block_level(self.loader, id, cap, self.budget)?
                    == Some(self.output.list.textures[i].mip)
                {
                    self.textures.insert((path, cap), i);
                    return Ok(i);
                }
            }
        }
        let kept = match (cap, self.options.blocks) {
            (Some(cap), Some(blocks)) => block_texture(
                self.loader,
                id,
                cap,
                blocks.max_bytes,
                &mut self.block_bytes,
                self.budget,
            )?,
            _ => None,
        };
        let loaded = match kept {
            Some(t) => t,
            None => decode_texture(
                self.loader,
                id,
                self.options,
                &mut self.texture_bytes,
                self.limits,
                self.budget,
            )?,
        };
        let (texture, blocks, declared_dxgi, declared_fourcc) = (
            loaded.texture,
            loaded.blocks,
            loaded.declared_dxgi,
            loaded.declared_fourcc,
        );
        match &blocks {
            Some(b) => texture.validate_blocks(b)?,
            None => texture.validate()?,
        }
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
        if let Some(b) = blocks {
            self.budget.charge(64, "block texture", 0)?;
            self.output.list.blocks.insert(index, b);
        }
        self.textures.insert((path, cap.unwrap_or(0)), index);
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
        self.constant_texture(rgba, file, offset)
    }
    /// A one-texel texture of a computed colour.
    fn constant_texture(&mut self, rgba: [f32; 4], file: &str, offset: u64) -> Result<usize> {
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
    /// The material a shape draws for `path` under the swaps that apply to
    /// it, outermost first (the reference's, then the bases' down through
    /// SCOL parts). With two or more, the four readings of
    /// [`Self::swap_readings`] agree (`mswp_scopes_readings_agree_v1`,
    /// checked for the whole base by [`Self::swap_scopes`]), or [`settle`]
    /// gives the material under `mswp_outer_scope_wins_v1` and
    /// `mswp_outer_scope_replaces_v1`.
    fn swap(
        &mut self,
        path: Vec<u8>,
        swaps: &[&Loaded<Mswp>],
        scratch: &Budget,
    ) -> Result<Vec<u8>> {
        let material = self.swap_scoped(path, swaps, scratch);
        self.pattern_rules()?;
        material
    }
    /// Records the pattern rules [`Self::swap_one`] relied on.
    fn pattern_rules(&mut self) -> Result<()> {
        use fo4_scene::hypotheses as h;
        let used = self.pattern_used.replace(0);
        if used & 1 != 0 {
            self.draw_rule(h::SWAP_PATTERN)?;
        }
        if used & 2 != 0 {
            self.draw_rule(h::SWAP_PATTERN_LITERAL)?;
        }
        Ok(())
    }
    fn swap_scoped(
        &mut self,
        path: Vec<u8>,
        swaps: &[&Loaded<Mswp>],
        scratch: &Budget,
    ) -> Result<Vec<u8>> {
        match swaps {
            [] => Ok(path),
            [one] => self.swap_one(path, one, scratch),
            _ => {
                let [outer, inner, down, up] = self.swap_readings(&path, swaps, scratch)?;
                if outer == inner && outer == down && outer == up {
                    return Ok(outer);
                }
                match settle(&path, swaps, scratch, |p, s| self.swap_one(p, s, scratch))? {
                    Settled::Ruled {
                        material,
                        outer,
                        replace,
                    } if self.scope_rules(outer, replace)? => Ok(material),
                    Settled::Ruled { .. } => Err(error(
                        "UnsupportedRenderSwapScopes",
                        "material swap",
                        0,
                        "readings differ",
                    )),
                    Settled::Chained { material }
                        if self.draw_rule(fo4_scene::hypotheses::SWAP_CHAIN)? =>
                    {
                        Ok(material)
                    }
                    Settled::Chained { .. } => Err(error(
                        "UnsupportedRenderSwapScopes",
                        "material swap",
                        0,
                        "readings differ, chained",
                    )),
                }
            }
        }
    }
    /// The vertex alpha of a blended lighting surface whose shape sets
    /// Shader Flags 2 Vertex_Colors and stores a vertex alpha below 1, under
    /// `lighting_vertex_alpha_blend_v1` (vertex RGB stays out of the frame).
    /// Without the rule the surface is drawn as before, without vertex
    /// alpha.
    fn lighting_vertex_alpha(
        &mut self,
        surface: &mut Surface,
        flags2: Option<u32>,
        geometry: Option<&fo4_nif::Mesh>,
    ) -> Result<()> {
        if surface.channels != 0
            || surface.alpha.enabled == 0
            || flags2.is_none_or(|f| f & (1 << 5) == 0)
        {
            return Ok(());
        }
        let below = geometry
            .and_then(|m| m.vertices.as_ref())
            .is_some_and(|vs| {
                vs.iter().any(|v| {
                    v.color
                        .map(|c| c[3] != 255)
                        .or(v.color_float.map(|c| c[3] != 1.))
                        .unwrap_or(false)
                })
            });
        if below && self.draw_rule(fo4_scene::hypotheses::LIGHTING_VERTEX_ALPHA)? {
            surface.channels = 2;
        }
        Ok(())
    }
    /// Whether the scope rules a settled path needs are usable at the
    /// build's level; used ones are recorded.
    fn scope_rules(&mut self, outer: bool, replace: bool) -> Result<bool> {
        use fo4_scene::hypotheses as h;
        Ok((!outer || self.draw_rule(h::SWAP_OUTER_WINS)?)
            && (!replace || self.draw_rule(h::SWAP_OUTER_REPLACES)?))
    }
    /// The material for `path` under two or more swap scopes, by each
    /// reading the data leaves open: the outermost swap alone (a reference
    /// swap replacing the base's), the innermost alone, every swap in turn
    /// from the outermost in, and from the innermost out.
    fn swap_readings(
        &self,
        path: &[u8],
        swaps: &[&Loaded<Mswp>],
        scratch: &Budget,
    ) -> Result<[Vec<u8>; 4]> {
        readings(path, swaps, scratch, |p, s| self.swap_one(p, s, scratch))
    }
    /// Before a base with two or more swap scopes draws anything: every
    /// material its models' shaders name must get the same material under
    /// every reading, or one [`settle`] gives under the scope rules; the
    /// rules must be usable at the build's level.
    fn swap_scopes(&mut self, base: &Base, swaps: &[&Loaded<Mswp>]) -> Result<()> {
        let scratch = self.budget.child();
        let loader = self.loader;
        let (mut chained, mut outer_wins, mut replace) = (false, false, false);
        for id in &base.assets {
            let Some(node) = loader.assets.nodes.get(*id) else {
                continue;
            };
            if node.kind != AssetKind::Model {
                continue;
            }
            for (d, c) in node.dependencies.iter().zip(&node.dependency_citations) {
                if c.class != Citation::NifMaterial {
                    continue;
                }
                let Some(target) = loader.assets.nodes.get(*d) else {
                    continue;
                };
                let [outer, inner, down, up] = self.swap_readings(&target.path, swaps, &scratch)?;
                if outer == inner && outer == down && outer == up {
                    continue;
                }
                match settle(&target.path, swaps, &scratch, |p, s| {
                    self.swap_one(p, s, &scratch)
                })? {
                    Settled::Ruled {
                        outer, replace: r, ..
                    } => {
                        outer_wins |= outer;
                        replace |= r;
                    }
                    Settled::Chained { .. } => chained = true,
                }
            }
        }
        self.pattern_rules()?;
        let refused = |found: &str| {
            Err(error(
                "UnsupportedRenderSwapScopes",
                &base.record.source.file,
                base.record.source.offset,
                found,
            ))
        };
        if chained && !self.draw_rule(fo4_scene::hypotheses::SWAP_CHAIN)? {
            return refused("readings differ, chained");
        }
        if (outer_wins || replace) && !self.scope_rules(outer_wins, replace)? {
            return refused("readings differ");
        }
        if !self.draw_rule(fo4_scene::hypotheses::SWAP_SCOPES_AGREE)? {
            return refused("readings agree");
        }
        Ok(())
    }
    /// A shape whose shader names no material under one or more swaps: its
    /// swaps are read as for any shape (their refusals stand), and the shape
    /// keeps its own texture under `mswp_shape_without_material_untouched_v1`.
    pub(crate) fn swap_without_material(
        &mut self,
        swaps: &[&Loaded<Mswp>],
        file: &str,
        shader: usize,
        scratch: &Budget,
    ) -> Result<()> {
        for s in swaps {
            self.swap_one(Vec::new(), s, scratch)?;
        }
        self.pattern_rules()?;
        if self.draw_rule(fo4_scene::hypotheses::SWAP_WITHOUT_MATERIAL)? {
            Ok(())
        } else {
            Err(error("RenderSwapWithoutMaterial", file, 0, shader))
        }
    }
    fn swap_one(&self, path: Vec<u8>, swap: &Loaded<Mswp>, scratch: &Budget) -> Result<Vec<u8>> {
        let mut from = None;
        let mut selected = None;
        // The target a pattern entry gives `path` (`mswp_wildcard_pattern_v1`).
        let mut pattern: Option<Vec<u8>> = None;
        // A literal source with an empty target matched `path`.
        let mut literal_kept = false;
        for entry in &swap.data.entries {
            match (&entry.signature, &entry.value) {
                (b"BNAM", SwapValue::Path(p)) => {
                    if from.is_some() {
                        return Err(error("RenderSwapPair", "material swap", 0, "unpaired BNAM"));
                    }
                    // An empty source matches no material under
                    // `mswp_empty_entry_inert_v1` (an empty key never
                    // equals a resolved path).
                    from = Some(if p.is_empty() && self.allow_swap_empty {
                        Vec::new()
                    } else {
                        self.resolve_path(Citation::SwapMaterial, p.as_bytes(), scratch)?
                    });
                }
                (b"SNAM", SwapValue::Path(p)) => {
                    let source = from.take().ok_or_else(|| {
                        error("RenderSwapPair", "material swap", 0, "SNAM without BNAM")
                    })?;
                    if source.is_empty() {
                        // The pair of an empty source changes nothing.
                    } else if self.allow_swap_pattern
                        && fo4_scene::swap_pattern::is_pattern(&source)
                    {
                        // An empty target leaves the material as it is.
                        let target = if p.is_empty() && self.allow_swap_empty {
                            fo4_scene::swap_pattern::apply(&source, &path, &path, scratch)?
                                .map(|_| path.clone())
                        } else {
                            let t =
                                self.resolve_path(Citation::SwapMaterial, p.as_bytes(), scratch)?;
                            fo4_scene::swap_pattern::apply(&source, &t, &path, scratch)?
                        };
                        if let Some(t) = target {
                            if pattern.as_ref().is_some_and(|q| *q != t) {
                                return Err(error(
                                    "AmbiguousMaterialSwap",
                                    "material swap",
                                    0,
                                    "two patterns, two targets",
                                ));
                            }
                            pattern = Some(t);
                        }
                    } else if p.is_empty() && self.allow_swap_empty {
                        // An empty target leaves its source as it is.
                        if source == path && selected.is_some() {
                            return Err(error(
                                "AmbiguousMaterialSwap",
                                "material swap",
                                0,
                                "duplicate source",
                            ));
                        }
                        literal_kept |= source == path;
                    } else if source == path {
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
                // An editor folder label under `mswp_fnam_folder_label_v1`.
                (b"FNAM", SwapValue::Path(_)) if self.allow_swap_folder => {}
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
        let Some(target) = pattern else {
            return Ok(selected.unwrap_or(path));
        };
        // A literal source of the same swap wins over a pattern
        // (`mswp_wildcard_literal_wins_v1`).
        if let Some(literal) = selected.or_else(|| literal_kept.then(|| path.clone())) {
            if literal != target {
                self.pattern_used.set(self.pattern_used.get() | 2);
            }
            return Ok(literal);
        }
        self.pattern_used.set(self.pattern_used.get() | 1);
        if target == path {
            return Ok(path);
        }
        // A target no archive holds changes nothing under
        // `mswp_wildcard_missing_target_inert_v1` (the scene raised
        // `MissingSwapPatternTarget` for it).
        let absent = self
            .loader
            .assets
            .find(AssetKind::Material, &target)
            .is_some_and(|id| {
                self.loader.assets.nodes[id]
                    .blockers
                    .iter()
                    .any(|e| e.code == "MissingMaterial")
            });
        if absent {
            return if self.allow_swap_pattern_missing {
                Ok(path)
            } else {
                Err(error(
                    "MissingSwapPatternTarget",
                    "material swap",
                    0,
                    fo4_core::HexBytes(&target),
                ))
            };
        }
        Ok(target)
    }
    fn surface(
        &mut self,
        n: &Nif<'_>,
        shape: &[Field],
        swap: &[&Loaded<Mswp>],
        file: &str,
        geometry: Option<&fo4_nif::Mesh>,
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
        let lighting =
            n.block_types[n.blocks[shader].type_index as usize] == "BSLightingShaderProperty";
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
        if self.allow_effect_surface
            && n.blocks[shader].type_index as usize
                == n.block_types
                    .iter()
                    .position(|t| t == "BSEffectShaderProperty")
                    .unwrap_or(usize::MAX)
        {
            let surface = self.effect_surface(
                n,
                shader,
                f,
                material.map(Vec::as_slice),
                nif_alpha,
                (nif_flags1, nif_flags2),
                swap,
                file,
                geometry,
            )?;
            self.uv_rule(&surface)?;
            return Ok(surface);
        }
        let material = match material {
            Some(material) => {
                let path = self.resolve_path(Citation::NifMaterial, material, &scratch)?;
                let path = self.swap(path, swap, &scratch)?;
                let id = self.asset(AssetKind::Material, &path)?;
                // A material file no archive holds: the shader's own fields
                // and texture set, under the scene's pending rule.
                let missing = self.loader.assets.nodes[id]
                    .blockers
                    .iter()
                    .any(|e| e.code == "MissingMaterial");
                if missing && self.allow_missing_embedded {
                    None
                } else {
                    Some(id)
                }
            }
            None => None,
        };
        let uv_driven = if self.texture_first_key {
            fo4_scene::rest::texture_driven(n, shader)
        } else {
            [false; 4]
        };
        if let Some(id) = material {
            let loader = self.loader;
            let nif_uv = [
                floats::<2>(f, "UV Offset", file)?,
                floats::<2>(f, "UV Scale", file)?,
            ];
            let mut surface = loader.assets.with_decoded(id, &scratch, |d| {
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
                    self.texture(
                        self.resolve_path(Citation::MaterialTexture, diffuse.bytes(), &scratch)?,
                        m.common.alpha_test.0 != 0,
                    )?
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
                self.opacity_rule(&alpha)?;
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
                    tint: None,
                    falloff: None,
                    palette: None,
                    clock: Default::default(),
                    colour: None,
                    soft_depth: None,
                    material_blend: c.alpha_blend.enabled != 0,
                    colour_terms: ([1.; 3], 1.),
                    effect: !lighting,
                })
            })?;
            // A controller drives the NIF shader's field; the draw takes the
            // material's, so the two must hold the same value.
            let drawn = [
                surface.offset[0],
                surface.scale[0],
                surface.offset[1],
                surface.scale[1],
            ];
            let stored = [nif_uv[0][0], nif_uv[1][0], nif_uv[0][1], nif_uv[1][1]];
            if uv_driven
                .iter()
                .zip(drawn.iter().zip(&stored))
                .any(|(d, (a, b))| *d && (*a as f32) != (*b as f32))
            {
                return Err(error(
                    "UnsettledTextureMaterialField",
                    file,
                    n.blocks[shader].data.start as u64,
                    "material UV field differs from the controlled NIF field",
                ));
            }
            // A field the clock drives: the material must hold the NIF's
            // stored value there too (the controller drives the NIF field).
            let tracks = self.shader_clock(n, shader, file)?;
            let at = n.blocks[shader].data.start as u64;
            let stored_opacity = if lighting {
                floats::<1>(f, "Alpha", file)?[0]
            } else {
                floats::<4>(f, "Base Color", file)?[3]
            };
            let held = [
                stored[0],
                nif_uv[0][1],
                stored[1],
                nif_uv[1][1],
                stored_opacity,
            ];
            let drawn = [
                surface.offset[0],
                surface.offset[1],
                surface.scale[0],
                surface.scale[1],
                f64::from(surface.alpha.opacity),
            ];
            if tracks
                .iter()
                .zip(drawn.iter().zip(&held))
                .any(|(t, (a, b))| t.is_some() && (*a as f32) != (*b as f32))
            {
                return Err(error(
                    "UnsettledClockMaterialField",
                    file,
                    at,
                    "material field differs from the NIF field the clock drives",
                ));
            }
            self.clock_surface(&mut surface, tracks, file, at)?;
            if lighting {
                self.lighting_vertex_alpha(&mut surface, nif_flags2, geometry)?;
            }
            self.uv_rule(&surface)?;
            return Ok(surface);
        }
        if !swap.is_empty() {
            self.swap_without_material(swap, file, shader, &scratch)?;
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
            // The environment map alone (bit 7 and its map and mask)
            // under its switch and rules: left out, the rest drawn.
            let environment = 1 << 7;
            let textured = |f: &&Field, allowed: &[&str]| {
                f.name.ends_with("Texture")
                    && !allowed.contains(&f.name)
                    && matches!(&f.value, Value::Bytes(p) if !p.is_empty())
            };
            let environment_only = flags & unsupported == environment
                && !f.iter().any(|f| {
                    textured(&f, &["Environment Map Texture", "Environment Mask Texture"])
                });
            let left_out = environment_only
                && self.options.effect_environment_left_out
                && self.draw_rule(fo4_scene::hypotheses::EFFECT_ENVIRONMENT_ADDS)?
                && self.draw_rule(fo4_scene::hypotheses::EFFECT_ENVIRONMENT_LEFT_OUT)?;
            if !left_out && (flags & unsupported != 0 || f.iter().any(|f| textured(&f, &[]))) {
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
            self.texture(
                self.resolve_path(Citation::NifTexture, raw, &scratch)?,
                nif_alpha.is_some_and(|(flags, _)| (flags >> 9) & 1 == 1),
            )?
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
        self.opacity_rule(&alpha)?;
        alpha.validate()?;
        let mut surface = Surface {
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
            tint: None,
            falloff: None,
            palette: None,
            clock: Default::default(),
            colour: None,
            colour_terms: ([1.; 3], 1.),
            effect: !lighting,
            material_blend: false,
            soft_depth: None,
        };
        let tracks = self.shader_clock(n, shader, file)?;
        self.clock_surface(
            &mut surface,
            tracks,
            file,
            n.blocks[shader].data.start as u64,
        )?;
        if kind == "BSLightingShaderProperty" {
            self.lighting_vertex_alpha(&mut surface, nif_flags2, geometry)?;
        }
        self.uv_rule(&surface)?;
        Ok(surface)
    }
    fn mesh(
        &mut self,
        path: &[u8],
        i: usize,
        m: &fo4_nif::Mesh,
        surface: &Surface,
        skinned: Option<(usize, &[[f64; 3]])>,
    ) -> Result<usize> {
        let model = self.asset(AssetKind::Model, path)?;
        let key = (
            model,
            i,
            surface.channels,
            surface.requires_uv,
            surface.tint.map(|t| t.map(f32::to_bits)),
            skinned.map(|s| s.0),
            surface.falloff.is_some(),
        );
        if let Some(index) = self.meshes.get(&key).copied() {
            if self.uv_left_out.contains(&index) {
                self.draw_rule(fo4_scene::hypotheses::UV_NONFINITE)?;
            }
            return Ok(index);
        }
        if skinned.is_some_and(|(_, p)| m.vertices.as_ref().is_none_or(|v| v.len() != p.len())) {
            return Err(error("RenderSkinnedVertexCount", "mesh", 0, i));
        }
        if m.dynamic_vertices.is_some() {
            return Err(error("UnsupportedRenderDynamicGeometry", "mesh", 0, i));
        }
        if surface.falloff.is_some() && skinned.is_some() {
            // Posed positions come without posed normals.
            return Err(error("UnsupportedSkinnedFalloff", "mesh", 0, i));
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
            normals: None,
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        if surface.falloff.is_some() {
            let mut normals = Vec::new();
            self.budget
                .reserve(&mut normals, vertices.len(), "falloff normals", 0)?;
            for v in vertices {
                let n = v
                    .normal
                    .ok_or_else(|| error("MissingEffectFalloffNormals", "mesh", 0, i))?;
                if n.iter().any(|x| !x.is_finite()) {
                    return Err(error("RenderNonFiniteNormal", "mesh", 0, i));
                }
                normals.push(n.map(f64::from));
            }
            mesh.normals = Some(normals);
        } else if self.options.vertex_normals
            && skinned.is_none()
            && vertices
                .iter()
                .all(|v| v.normal.is_some_and(|n| n.iter().all(|x| x.is_finite())))
            && !vertices.is_empty()
        {
            let mut normals = Vec::new();
            self.budget
                .reserve(&mut normals, vertices.len(), "vertex normals", 0)?;
            for v in vertices {
                normals.push(v.normal.unwrap_or_default().map(f64::from));
            }
            mesh.normals = Some(normals);
        }
        self.budget
            .reserve(&mut mesh.vertices, vertices.len(), "render vertices", 0)?;
        if surface.channels != 0 || surface.tint.is_some() {
            let tint = surface.tint.unwrap_or([1.; 3]);
            let mut colors = Vec::new();
            self.budget
                .reserve(&mut colors, vertices.len(), "effect vertex colors", 0)?;
            for v in vertices {
                let c = match v
                    .color
                    .map(|c| c.map(|v| v as f32 / 255.))
                    .or(v.color_float)
                {
                    Some(c) => c,
                    None if surface.channels == 0 => [1.; 4],
                    None => return Err(error("MissingEnabledEffectVertexColor", "mesh", 0, i)),
                };
                if c.iter().any(|v| !v.is_finite()) {
                    return Err(error("RenderNonFiniteVertexColor", "mesh", 0, i));
                }
                colors.push(std::array::from_fn(|j| {
                    let own = if (j < 3 && surface.channels & 1 != 0)
                        || (j == 3 && surface.channels & 2 != 0)
                    {
                        c[j]
                    } else {
                        1.
                    };
                    match tint.get(j) {
                        Some(t) if surface.tint.is_some() => own * t,
                        _ => own,
                    }
                }));
            }
            mesh.vertex_colors = Some(colors);
        }
        let mut bad_uv: Vec<bool> = Vec::new();
        for (k, v) in vertices.iter().enumerate() {
            let position = match skinned {
                Some((_, p)) => p[k],
                None => v
                    .position
                    .ok_or_else(|| error("MissingRenderPosition", "mesh", 0, i))?
                    .map(f64::from),
            };
            // Constant surfaces do not sample UV; zero is an internal lookup coordinate.
            let uv = if surface.requires_uv {
                v.uv.ok_or_else(|| error("MissingRenderUv", "mesh", 0, i))?
                    .map(f64::from)
            } else {
                [0.; 2]
            };
            if position.iter().any(|x| !x.is_finite()) {
                return Err(error("RenderNonFiniteVertex", "mesh", 0, i));
            }
            // A texture coordinate that is not a finite number: the
            // triangles using the vertex are left out under
            // `nif_uv_nonfinite_triangles_left_out_v1`, and the unused
            // vertex keeps an internal zero coordinate.
            let uv = if uv.iter().any(|x| !x.is_finite()) {
                if !self.draw_rule(fo4_scene::hypotheses::UV_NONFINITE)? {
                    return Err(error("RenderNonFiniteVertex", "mesh", 0, i));
                }
                if bad_uv.is_empty() {
                    self.budget
                        .reserve(&mut bad_uv, vertices.len(), "render vertices", 0)?;
                    bad_uv.resize(vertices.len(), false);
                }
                bad_uv[k] = true;
                [0.; 2]
            } else {
                uv
            };
            mesh.vertices.push(Vertex { position, uv });
        }
        self.budget
            .reserve(&mut mesh.triangles, triangles.len(), "render indices", 0)?;
        if bad_uv.is_empty() {
            mesh.triangles.extend_from_slice(triangles);
        } else {
            mesh.triangles.extend(
                triangles
                    .iter()
                    .filter(|t| t.iter().all(|v| !bad_uv[*v as usize])),
            );
        }
        let index = self.output.list.meshes.len();
        append(&mut self.output.list.meshes, mesh, self.budget)?;
        self.budget
            .charge(path.len() as u64 + 256, "mesh identity", 0)?;
        self.meshes.insert(key, index);
        if !bad_uv.is_empty() {
            self.uv_left_out.insert(index);
        }
        Ok(index)
    }
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        n: &Nif<'_>,
        path: &[u8],
        i: usize,
        parent: Transform,
        motion: Option<&crate::Motion>,
        instance: u32,
        swap: &[&Loaded<Mswp>],
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
        // `static_unmoved_parts_drawn_v1`: a part the reading leaves out
        // (`fo4_scene::parts`) is counted and not drawn.
        if let Some(code) = self.parts.as_ref().and_then(|p| p.left_out(i)) {
            let code = code.to_string();
            append(&mut self.output.left_out, (instance, code), self.budget)?;
            return Ok(());
        }
        let ty = &n.block_types[block.type_index as usize];
        if matches!(
            block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return Ok(());
        }
        let data = match decoded(n, i, file) {
            Ok(d) => d,
            Err(e) if self.leave_out(instance, &e)? => return Ok(()),
            Err(e) => return Err(e),
        };
        let f = fields(data);
        // A node a free-running transform controller moves (behind the
        // clock's transform switches) takes its track's local transform at
        // the build time, and the draws below it keep the chain of moving
        // nodes (`crate::Motion`) for every other time.
        let mut here: Option<crate::Motion> = None;
        let world = if let Some(track) = self.node_tracks.get(&i).cloned() {
            if depth == 0 {
                return Err(error(
                    "UnsettledClockRootTransform",
                    file,
                    block.data.start as u64,
                    "a transform controller moving a root node",
                ));
            }
            let time = self.options.time.ok_or_else(|| {
                error("UnsettledControllerTime", file, block.data.start as u64, i)
            })?;
            let mut m = motion.cloned().unwrap_or(crate::Motion {
                pre: parent,
                steps: Vec::new(),
            });
            m.steps.push(crate::MotionStep {
                node: track,
                after: Transform::IDENTITY,
            });
            let w = m.world(time)?;
            here = Some(m);
            w
        } else if f.iter().any(|f| f.name == "Translation") {
            let local = Transform::from_fields(f, file)?;
            // A root node's own transform is left out, the placement standing
            // in its place, only under `nif_root_transform_replaced_v1`; at
            // the other levels it composes as the schema's node transform.
            if depth == 0
                && !local.is_identity()
                && self.draw_rule(fo4_scene::hypotheses::ROOT_TRANSFORM)?
            {
                parent
            } else if let Some(m) = motion {
                let mut m = m.clone();
                if let Some(last) = m.steps.last_mut() {
                    last.after = last.after.compose(local);
                }
                let time = self.options.time.unwrap_or(0.);
                let w = m.world(time)?;
                here = Some(m);
                w
            } else {
                parent.compose(local)
            }
        } else {
            here = motion.cloned();
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
                        here.as_ref(),
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
            // A named platform's skinned shape is drawn on the model's own
            // nodes by its actor, not here.
            if self.platform
                && geometry.is_some()
                && fo4_nif::skin::shape_skin(n, i, file, self.limits, self.budget)?.is_some()
            {
                return Ok(());
            }
            if let Some(mesh) = geometry {
                // A refraction surface under its rule: left out, the scene
                // behind it drawn undisplaced.
                if self.options.refraction_left_out
                    && refraction_surface(n, f)
                    && self.draw_rule(fo4_scene::hypotheses::LIGHTING_REFRACTION_LEFT_OUT)?
                {
                    append(
                        &mut self.output.left_out,
                        (instance, REFRACTION_SURFACE.into()),
                        self.budget,
                    )?;
                    return Ok(());
                }
                let surface = self.surface(n, f, swap, file, Some(mesh))?;
                // A named platform's effect surfaces (glows, light shafts:
                // fields its controllers drive) are left out.
                if self.platform && surface.effect {
                    append(
                        &mut self.output.left_out,
                        (instance, "PlatformEffectSurface".into()),
                        self.budget,
                    )?;
                    return Ok(());
                }
                // A part whose blend factors come from a material file is
                // left out under `static_unmoved_parts_drawn_v1`.
                if self.parts.is_some() && surface.material_blend {
                    append(
                        &mut self.output.left_out,
                        (instance, fo4_scene::parts::MATERIAL_BLEND.into()),
                        self.budget,
                    )?;
                    return Ok(());
                }
                let mesh = self.mesh(path, mesh_index, mesh, &surface, None)?;
                self.animate(&surface, here.as_ref())?;
                if surface.effect {
                    let at = self.output.list.draws.len();
                    append(&mut self.output.effect_draws, at, self.budget)?;
                }
                if self.options.chains {
                    let mut blocks = Vec::new();
                    self.budget
                        .reserve(&mut blocks, active.len(), "draw chain", 0)?;
                    blocks.extend_from_slice(active);
                    let mut model = Vec::new();
                    self.budget
                        .reserve(&mut model, path.len(), "draw chain", 0)?;
                    model.extend_from_slice(path);
                    let chain = DrawChain {
                        draw: self.output.list.draws.len(),
                        model,
                        blocks,
                        bones: Vec::new(),
                    };
                    append(&mut self.output.chains, chain, self.budget)?;
                }
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
                        falloff: surface.falloff,
                        palette: surface.palette,
                        colour: surface.colour,
                        soft_depth: surface.soft_depth,
                    },
                    self.budget,
                )?;
            }
            Ok(())
        })();
        active.pop();
        // A named platform's shape the draw refuses is left out.
        match result {
            Err(e) if !matches!(data, Data::Node { .. }) && self.leave_out(instance, &e)? => Ok(()),
            r => r,
        }
    }
    fn base(
        &mut self,
        base: &Base,
        world: Transform,
        instance: u32,
        inherited: &[&Loaded<Mswp>],
        depth: u64,
    ) -> Result<()> {
        fo4_core::limit(depth, self.limits.max_depth, "render base", 0)?;
        if base.editor_marker {
            return Ok(());
        }
        let mut stack: Vec<&Loaded<Mswp>> = Vec::new();
        self.budget
            .reserve(&mut stack, inherited.len() + 1, "material swap", 0)?;
        stack.extend_from_slice(inherited);
        if let Some(s) = &base.material_swap {
            stack.push(s);
        }
        let swap = stack.as_slice();
        let collection_parts = base.record.signature == *b"SCOL" && !base.parts.is_empty();
        // A SCOL drawn by its parts draws no model of its own: each part
        // checks its own models with the whole stack.
        if stack.len() > 1 && !collection_parts {
            self.swap_scopes(base, &stack)?;
        }
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
                    self.node_tracks = self.moving_nodes(&n, path)?;
                    // `static_unmoved_parts_drawn_v1`: the model's parts by
                    // its own blockers.
                    self.parts = if self.static_parts {
                        let blockers = &loader.assets.nodes[*id].blockers;
                        Some(fo4_scene::parts::read_with(
                            &n,
                            blockers,
                            self.parts_at_rest,
                            self.budget,
                        )?)
                    } else {
                        None
                    };
                    for root in n.roots.iter().filter(|r| **r >= 0) {
                        self.node(
                            &n,
                            path,
                            *root as usize,
                            world,
                            None,
                            instance,
                            swap,
                            &mut active,
                            0,
                        )?;
                    }
                    self.node_tracks.clear();
                    self.parts = None;
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
impl Builder<'_, '_> {
    /// Whether a registered rule that the draw itself needs (not one of the
    /// instance's state) may be used at the build's level; a used rule is
    /// recorded for the instance.
    fn draw_rule(&mut self, rule: &'static str) -> Result<bool> {
        use fo4_scene::hypotheses::{status, Status};
        let usable = match status(rule) {
            Some(Status::Supported) => self.options.level != fo4_scene::RuleLevel::Facts,
            Some(Status::Pending) => self.options.level == fo4_scene::RuleLevel::Hypotheses,
            Some(Status::Refuted) | None => false,
        };
        if usable && !self.draw_rules.contains(&rule) {
            append(&mut self.draw_rules, rule, self.budget)?;
        }
        Ok(usable)
    }
    /// Adds the rules the current instance's draws relied on to its
    /// identity, the last one pushed.
    fn record_draw_rules(&mut self) -> Result<()> {
        if self.draw_rules.is_empty() {
            return Ok(());
        }
        let Some(id) = self.output.instances.last_mut() else {
            return Ok(());
        };
        for rule in &self.draw_rules {
            if !id.hypotheses.contains(rule) {
                self.budget
                    .reserve(&mut id.hypotheses, 1, "draw hypotheses", 0)?;
                id.hypotheses.push(rule);
            }
        }
        id.hypotheses.sort_unstable();
        Ok(())
    }
    /// An alpha whose opacity is above 1 is drawn only under
    /// `bgsm_opacity_above_one_test_v1` on a surface without blending and
    /// under `blend_alpha_clamped_before_blend_v1` on a blended one.
    fn opacity_rule(&mut self, alpha: &Alpha) -> Result<()> {
        let rule = if alpha.enabled == 0 {
            fo4_scene::hypotheses::OPACITY_ABOVE_ONE
        } else {
            fo4_scene::hypotheses::BLEND_OPACITY_CLAMPED
        };
        if alpha.opacity.is_finite() && alpha.opacity > 1. && !self.draw_rule(rule)? {
            return Err(Error::new(
                "UnsupportedRenderAlpha",
                "draw list",
                0,
                "opacity at most 1 without a registered rule",
                format!("{alpha:?}"),
            ));
        }
        Ok(())
    }
    /// A textured surface whose UV offset is not 0 or whose UV scale is not
    /// 1 is drawn only under `uv_scale_then_offset_v1` (texture coordinate
    /// = stored UV x scale + offset, per component).
    fn uv_rule(&mut self, s: &Surface) -> Result<()> {
        if s.requires_uv
            && (s.offset != [0., 0.] || s.scale != [1., 1.])
            && !self.draw_rule(fo4_scene::hypotheses::UV_SCALE_THEN_OFFSET)?
        {
            return Err(error(
                "UnsupportedUvTransform",
                "draw list",
                0,
                "UV offset 0 and scale 1 without a registered rule",
            ));
        }
        Ok(())
    }
    /// The nodes of model `n` that free-running transform controllers move
    /// on the clock, with their tracks: only with a build time, for an
    /// instance relying on a transform channel rule; two controllers moving
    /// one node are refused (`AmbiguousClockField`).
    fn moving_nodes(
        &self,
        n: &Nif<'_>,
        path: &[u8],
    ) -> Result<BTreeMap<usize, fo4_scene::clock::TransformTrack>> {
        let mut out = BTreeMap::new();
        if self.options.time.is_none() || !self.transform_clock {
            return Ok(out);
        }
        for i in 0..n.blocks.len() {
            let Some(t) = fo4_scene::rest::transform_clock(n, i, self.options.clock, self.budget)?
            else {
                continue;
            };
            if out.contains_key(&t.node) {
                return Err(error(
                    "AmbiguousClockField",
                    &String::from_utf8_lossy(path),
                    n.blocks[i].data.start as u64,
                    format!("two transform controllers move node {}", t.node),
                ));
            }
            out.insert(t.node, t);
        }
        Ok(out)
    }
    /// Records the animation of the draw about to be appended for
    /// `surface`, when the clock drives one of its fields or moves a node
    /// above it.
    fn animate(&mut self, surface: &Surface, motion: Option<&crate::Motion>) -> Result<()> {
        if surface.clock.iter().all(Option::is_none) && motion.is_none() {
            return Ok(());
        }
        let draw = self.output.list.draws.len();
        append(
            &mut self.output.list.animations,
            crate::Animation {
                draw,
                tracks: surface.clock.clone(),
                colour: surface.colour_terms.0,
                scale: surface.colour_terms.1,
                motion: motion.cloned(),
            },
            self.budget,
        )
    }
    /// The clock tracks of the free-running controllers that drive shader
    /// `shader`'s UV offset and scale components and alpha
    /// (`fo4_scene::rest::clock_field`, `fo4_scene::clock::track`), by
    /// `crate::Animation` slot; none without a build time. A controller the
    /// clock does not run leaves its field to the rest rules.
    fn shader_clock(
        &mut self,
        n: &Nif<'_>,
        shader: usize,
        file: &str,
    ) -> Result<[Option<fo4_scene::clock::Track>; crate::TRACKS]> {
        let mut out: [Option<fo4_scene::clock::Track>; crate::TRACKS] = Default::default();
        if self.options.time.is_none() {
            return Ok(out);
        }
        let effect =
            n.block_types[n.blocks[shader].type_index as usize] == "BSEffectShaderProperty";
        let switches = self.options.clock;
        for i in 0..n.blocks.len() {
            // Alpha and UV fields, and the effect fields behind their
            // switches (slots 5 to 10).
            let slot = match fo4_scene::rest::clock_field(n, i) {
                Some(variable) => match (effect, variable) {
                    (true, 5) | (false, 12) => crate::TRACK_OPACITY,
                    (true, 6) | (false, 20) => crate::TRACK_U_OFFSET,
                    (true, 7) | (false, 21) => crate::TRACK_U_SCALE,
                    (true, 8) | (false, 22) => crate::TRACK_V_OFFSET,
                    (true, 9) | (false, 23) => crate::TRACK_V_SCALE,
                    _ => continue,
                },
                None => match fo4_scene::rest::effect_clock_field(n, i)
                    .filter(|(c, v)| effect && switches.effect_field(*c, *v))
                {
                    Some((true, _)) => crate::TRACK_EMISSIVE_COLOUR,
                    Some((false, 0)) => crate::TRACK_EMISSIVE_MULTIPLE,
                    Some((false, 1)) => crate::TRACK_FALLOFF_START_ANGLE,
                    Some((false, 2)) => crate::TRACK_FALLOFF_STOP_ANGLE,
                    Some((false, 3)) => crate::TRACK_FALLOFF_START_OPACITY,
                    Some((false, 4)) => crate::TRACK_FALLOFF_STOP_OPACITY,
                    _ => continue,
                },
            };
            let target = n
                .blocks
                .get(i)
                .and_then(|b| b.decoded.as_ref())
                .and_then(|d| raw_ref(fields(d), "Target"));
            if target != Some(shader as i32) {
                continue;
            }
            let Ok(track) = fo4_scene::clock::track(n, i, self.budget)? else {
                continue;
            };
            if out[slot].is_some() {
                return Err(error(
                    "AmbiguousClockField",
                    file,
                    n.blocks[i].data.start as u64,
                    format!("two controllers drive slot {slot}"),
                ));
            }
            out[slot] = Some(track);
        }
        Ok(out)
    }
    /// Sets `s`'s fields to the clock's values at the build time and keeps
    /// the tracks for the draw's animation: the clock rules each track
    /// relies on, the UV order rule for a tracked UV field of a textured
    /// surface and the opacity rules over the whole range an opacity track
    /// can take must be usable at the build's level.
    fn clock_surface(
        &mut self,
        s: &mut Surface,
        tracks: [Option<fo4_scene::clock::Track>; crate::TRACKS],
        file: &str,
        at: u64,
    ) -> Result<()> {
        let Some(time) = self.options.time else {
            return Ok(());
        };
        if tracks.iter().all(Option::is_none) {
            return Ok(());
        }
        use fo4_scene::hypotheses as h;
        // The effect fields behind their switches each rely on their rule,
        // a falloff angle's keys also on the degrees reading.
        let field_rules = |slot: usize| -> &'static [&'static str] {
            match slot {
                crate::TRACK_EMISSIVE_MULTIPLE => &[h::CLOCK_EMISSIVE_MULTIPLE],
                crate::TRACK_EMISSIVE_COLOUR => &[h::CLOCK_EMISSIVE_COLOUR],
                crate::TRACK_FALLOFF_START_ANGLE | crate::TRACK_FALLOFF_STOP_ANGLE => {
                    &[h::CLOCK_FALLOFF, h::REST_FALLOFF_DEGREES]
                }
                crate::TRACK_FALLOFF_START_OPACITY | crate::TRACK_FALLOFF_STOP_OPACITY => {
                    &[h::CLOCK_FALLOFF]
                }
                _ => &[],
            }
        };
        for (slot, t) in tracks.iter().enumerate() {
            let Some(t) = t else { continue };
            for rule in t
                .rules()
                .into_iter()
                .chain(field_rules(slot).iter().copied())
            {
                if !self.draw_rule(rule)? {
                    return Err(error("UnverifiedControllerClock", file, at, rule));
                }
            }
        }
        if s.requires_uv
            && tracks[..4].iter().any(Option::is_some)
            && !self.draw_rule(fo4_scene::hypotheses::UV_SCALE_THEN_OFFSET)?
        {
            return Err(error(
                "UnsupportedUvTransform",
                file,
                at,
                "a UV field the clock drives without the UV order rule",
            ));
        }
        if let Some(t) = &tracks[crate::TRACK_OPACITY] {
            let (lo, hi) = t
                .keys
                .bounds(0)
                .ok_or_else(|| error("ClockTrackValue", file, at, "opacity bounds"))?;
            if lo < 0. || !lo.is_finite() || !hi.is_finite() {
                return Err(error(
                    "UnsupportedRenderAlpha",
                    file,
                    at,
                    format!("opacity track from {lo} to {hi}"),
                ));
            }
            self.opacity_rule(&Alpha {
                opacity: hi,
                ..s.alpha
            })?;
        }
        let a = crate::Animation {
            draw: 0,
            tracks,
            colour: s.colour_terms.0,
            scale: s.colour_terms.1,
            motion: None,
        };
        let f = a.fields(
            time,
            crate::ClockFields {
                uv_offset: s.offset,
                uv_scale: s.scale,
                opacity: s.alpha.opacity,
                colour: s.colour,
                falloff: s.falloff,
            },
        )?;
        s.offset = f.uv_offset;
        s.scale = f.uv_scale;
        s.alpha.opacity = f.opacity;
        s.colour = f.colour;
        s.falloff = f.falloff;
        s.alpha.validate()?;
        s.clock = a.tracks;
        Ok(())
    }
    /// Sets the rule flags the shared walkers read from a state's rules.
    fn allow(&mut self, rules: &[&'static str]) {
        use fo4_scene::hypotheses as h;
        self.allow_effect_color = rules.contains(&h::EFFECT_COLOR);
        self.allow_effect_surface = rules.contains(&h::REST_EFFECT_FIRST_KEY)
            || rules.contains(&h::CLOCK_FREE_RUNNING)
            || rules.contains(&h::ACTOR_MODEL_GRAPH_START_IDLE);
        self.clock_rest = rules.contains(&h::CLOCK_FREE_RUNNING);
        self.allow_missing_embedded = rules.contains(&h::MISSING_MATERIAL_EMBEDDED);
        self.texture_first_key = rules.contains(&h::REST_TEXTURE_FIRST_KEY);
        self.allow_path_strip = rules.contains(&h::PATH_STRIP);
        self.allow_path_root = rules.contains(&h::PATH_ROOT);
        self.allow_path_relative = rules.contains(&h::PATH_DATA_RELATIVE);
        self.allow_visibility_marker = rules.contains(&h::VISIBILITY_MARKER);
        self.allow_marker_suffix = rules.contains(&h::MARKER_SUFFIX);
        self.allow_swap_folder = rules.contains(&h::SWAP_FOLDER);
        self.allow_swap_empty = rules.contains(&h::SWAP_EMPTY_ENTRY);
        self.allow_swap_pattern_missing = rules.contains(&h::SWAP_PATTERN_MISSING);
        self.pattern_used.set(0);
        self.allow_editor_marker_subtrees = rules.contains(&h::NAMED_MARKER);
        self.allow_scol_parts = rules.contains(&h::SCOL);
        self.platform = rules.contains(&h::PLAY_PLATFORM_LEFT_OUT);
        self.static_parts = rules.contains(&h::STATIC_PARTS)
            || rules.contains(&h::ACTIVATOR_PARTS)
            || rules.contains(&h::FURNITURE_PARTS)
            || rules.contains(&h::FURNITURE_AT_REST);
        self.parts_at_rest = rules.contains(&h::FURNITURE_AT_REST);
    }
    /// Under [`Builder::platform`]: whether `e`, raised drawing one shape
    /// or block, is left out (an allocation, limit or arithmetic failure
    /// still fails the build).
    fn leave_out(&mut self, instance: u32, e: &Error) -> Result<bool> {
        if !(self.platform || self.parts.is_some())
            || matches!(
                e.code.as_str(),
                "AllocationFailed" | "LimitExceeded" | "Overflow" | "RenderNodeCycle"
            )
        {
            return Ok(false);
        }
        append(
            &mut self.output.left_out,
            (instance, e.code.clone()),
            self.budget,
        )?;
        Ok(true)
    }
    /// The rest pose of skeleton asset `id`, read once.
    fn skeleton(&mut self, id: usize) -> Result<()> {
        if self.skeletons.contains_key(&id) {
            return Ok(());
        }
        let loader = self.loader;
        let path = &loader.assets.nodes[id].path;
        let file = String::from_utf8_lossy(path).into_owned();
        let pose = loader.assets.with_decoded(id, &self.budget.child(), |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(error("RenderAssetKind", "skeleton", 0, id));
            };
            fo4_scene::actor::pose::Skeleton::read(&n, &file, self.limits, self.budget)
        })?;
        self.skeletons.insert(id, pose);
        Ok(())
    }
    /// One node of an actor model: skinned shapes are posed by `skeleton`
    /// and drawn at `world`; shapes without a skin are counted, not drawn.
    #[allow(clippy::too_many_arguments)]
    fn actor_node(
        &mut self,
        n: &Nif<'_>,
        path: &[u8],
        i: usize,
        world: Transform,
        instance: u32,
        skeleton: usize,
        active: &mut Vec<usize>,
        depth: u64,
    ) -> Result<()> {
        let file = String::from_utf8_lossy(path).into_owned();
        fo4_core::limit(depth, self.limits.max_depth, "render node", 0)?;
        self.visits = fo4_core::add(self.visits, 1, "render visits", 0)?;
        fo4_core::limit(self.visits, self.limits.max_entries, "render visits", 0)?;
        if active.contains(&i) {
            return Err(error("RenderNodeCycle", &file, 0, i));
        }
        let block = n
            .blocks
            .get(i)
            .ok_or_else(|| error("RenderNodeIndex", &file, 0, i))?;
        let at = block.data.start as u64;
        let marker = fo4_scene::model_policy::visibility_editor_marker(n, i);
        let suffix = fo4_scene::model_policy::editor_marker_suffix(n, i);
        let named = fo4_scene::model_policy::editor_marker_object(n, i);
        if !self.include_marker_subtrees && (marker || suffix || named) {
            let allowed = (!marker || self.allow_visibility_marker)
                && (!suffix || self.allow_marker_suffix)
                && (!named || self.allow_editor_marker_subtrees);
            if !allowed {
                return Err(error(
                    "UnverifiedActorModelMarker",
                    &file,
                    at,
                    "registered helper visibility rule",
                ));
            }
            self.output.skipped_editor_marker_subtrees =
                fo4_core::add(self.output.skipped_editor_marker_subtrees, 1, &file, at)?;
            return Ok(());
        }
        let ty = &n.block_types[block.type_index as usize];
        if matches!(
            block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return Ok(());
        }
        let data = decoded(n, i, &file)?;
        append(active, i, self.budget)?;
        let result = (|| {
            match data {
                Data::Node { children, .. } => {
                    for c in children.iter().filter(|c| **c >= 0) {
                        self.actor_node(
                            n,
                            path,
                            *c as usize,
                            world,
                            instance,
                            skeleton,
                            active,
                            depth + 1,
                        )?;
                    }
                }
                Data::Mesh(m) => {
                    let f = fields(data);
                    let Some(skin) =
                        fo4_nif::skin::shape_skin(n, i, &file, self.limits, self.budget)?
                    else {
                        self.output.actor_rigid_shapes =
                            fo4_core::add(self.output.actor_rigid_shapes, 1, &file, at)?;
                        return Ok(());
                    };
                    // 23,367 of 23,368 skinned shapes measured have exactly the
                    // identity as their own transform; whether another one
                    // applies is not established, so such a shape is refused.
                    let own = fo4_scene::actor::pose::Affine::from_fields(f, &file)?;
                    if own.is_some_and(|a| a != fo4_scene::actor::pose::Affine::IDENTITY) {
                        return Err(error(
                            "ActorSkinnedShapeTransform",
                            &file,
                            at,
                            "identity shape transform",
                        ));
                    }
                    let pose = &self.skeletons[&skeleton];
                    use fo4_scene::actor::pose::MissingBones;
                    let mode = match (self.missing_bones_at_bind, self.missing_bones_nearest) {
                        (false, _) => MissingBones::Fail,
                        (true, false) => MissingBones::Identity,
                        (true, true) => MissingBones::Nearest,
                    };
                    let (matrices, missing) = fo4_scene::actor::pose::bone_matrices_missing(
                        n,
                        &skin,
                        pose,
                        &file,
                        mode,
                        self.budget,
                    )?;
                    if missing > 0 {
                        let mut rules = vec![fo4_scene::hypotheses::ACTOR_MISSING_BONE_BIND];
                        if mode == MissingBones::Nearest {
                            rules.push(fo4_scene::hypotheses::ACTOR_MISSING_BONE_NEAREST);
                        }
                        for rule in rules {
                            if !self.draw_rules.contains(&rule) {
                                append(&mut self.draw_rules, rule, self.budget)?;
                            }
                        }
                    }
                    let positions = fo4_scene::actor::pose::skin_positions(
                        n,
                        i,
                        &matrices,
                        &file,
                        self.budget,
                    )?;
                    if self.options.refraction_left_out
                        && refraction_surface(n, f)
                        && self.draw_rule(fo4_scene::hypotheses::LIGHTING_REFRACTION_LEFT_OUT)?
                    {
                        append(
                            &mut self.output.left_out,
                            (instance, REFRACTION_SURFACE.into()),
                            self.budget,
                        )?;
                        return Ok(());
                    }
                    let surface = self.surface(n, f, &[], &file, Some(m))?;
                    if self.platform && surface.effect {
                        append(
                            &mut self.output.left_out,
                            (instance, "PlatformEffectSurface".into()),
                            self.budget,
                        )?;
                        return Ok(());
                    }
                    let mesh = self.mesh(path, i, m, &surface, Some((skeleton, &positions)))?;
                    if self.posed.contains(&skeleton)
                        && !self.recorded.iter().any(|r| r.mesh == mesh)
                    {
                        let shape = fo4_scene::actor::pose::SkinnedShape::read(
                            n,
                            i,
                            &skin,
                            &file,
                            self.budget,
                        )?;
                        append(
                            &mut self.recorded,
                            SkinnedMesh {
                                mesh,
                                skeleton,
                                shape,
                            },
                            self.budget,
                        )?;
                    }
                    self.animate(&surface, None)?;
                    if surface.effect {
                        let at = self.output.list.draws.len();
                        append(&mut self.output.effect_draws, at, self.budget)?;
                    }
                    // A named platform's skinned shape keeps its chain and
                    // its bones' chains, for the window to move it.
                    if self.platform && self.options.chains {
                        let mut bones = Vec::new();
                        self.budget
                            .reserve(&mut bones, skin.bones.len(), "draw chain", 0)?;
                        for bone in &skin.bones {
                            bones.push(
                                self.node_chains
                                    .get(&bone.node)
                                    .cloned()
                                    .unwrap_or_default(),
                            );
                        }
                        let chain = DrawChain {
                            draw: self.output.list.draws.len(),
                            model: path.to_vec(),
                            blocks: active.clone(),
                            bones,
                        };
                        append(&mut self.output.chains, chain, self.budget)?;
                    }
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
                            falloff: surface.falloff,
                            palette: surface.palette,
                            colour: surface.colour,
                            soft_depth: surface.soft_depth,
                        },
                        self.budget,
                    )?;
                }
                Data::Fields(_) => {}
            }
            Ok(())
        })();
        active.pop();
        // A named platform's shape the draw refuses is left out.
        match result {
            Err(e) if !matches!(data, Data::Node { .. }) && self.leave_out(instance, &e)? => Ok(()),
            r => r,
        }
    }
    /// Draws one resolved actor as instance `index`.
    fn actor(&mut self, a: &fo4_scene::actor::Actor, index: u32) -> Result<()> {
        let src = &a.source;
        if !a.state.ready_at(self.options.level) {
            let why = if a.plan.is_none() {
                a.notes.join("; ")
            } else {
                format!("{:?} at rule level {:?}", a.state, self.options.level)
            };
            return Err(error("ActorNotDrawable", &src.file, src.offset, why));
        }
        if a.disabled_by_enable_parent() {
            return Err(error(
                "DisabledThroughEnableParent",
                &src.file,
                src.offset,
                fo4_scene::hypotheses::ENABLE_FOLLOWS,
            ));
        }
        let plan = a.plan.as_ref().ok_or_else(|| {
            error(
                "ActorNotDrawable",
                &src.file,
                src.offset,
                a.notes.join("; "),
            )
        })?;
        let p = a.transform.ok_or_else(|| {
            error(
                "MissingRenderTransform",
                &src.file,
                src.offset,
                "ACHR DATA absent",
            )
        })?;
        // Absent XSCL is 1 under achr_placement_as_refr_v1, which the state holds.
        let world = Transform::euler(p, a.scale.unwrap_or(1.))?;
        self.allow(a.state.hypotheses());
        self.missing_bones_at_bind = a.missing_bones_at_bind;
        self.missing_bones_nearest = a.missing_bones_nearest;
        // A pose key the caller gave (`Actor::pose`) must be one of the
        // given poses; without one the skeleton asset is the key.
        let skeleton = match a.pose {
            Some(k) if !self.skeletons.contains_key(&k) => {
                return Err(error(
                    "ActorPoseMissing",
                    &src.file,
                    src.offset,
                    format!("pose key {k} among the given poses"),
                ));
            }
            Some(k) => k,
            None => {
                self.skeleton(plan.skeleton_asset)?;
                plan.skeleton_asset
            }
        };
        let first = self.output.list.draws.len();
        for part in &plan.parts {
            self.actor_model(part.asset, world, index, skeleton)?;
        }
        if self.output.list.draws.len() == first {
            return Err(error(
                "NoDrawableGeometry",
                &src.file,
                src.offset,
                a.reference.object_id,
            ));
        }
        if let Some(head) = &plan.head {
            self.actor_head(a, head, world, index, skeleton)?;
        }
        Ok(())
    }
    /// Draws the skinned shapes of one actor model.
    fn actor_model(
        &mut self,
        asset: usize,
        world: Transform,
        index: u32,
        skeleton: usize,
    ) -> Result<()> {
        let scratch = self.budget.child();
        let loader = self.loader;
        let path = &loader.assets.nodes[asset].path;
        loader.assets.with_decoded(asset, &scratch, |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(error("RenderAssetKind", "actor model", 0, asset));
            };
            self.node_chains = if self.platform && self.options.chains {
                node_chains(&n, &String::from_utf8_lossy(path), self.budget)?
            } else {
                BTreeMap::new()
            };
            let mut active = Vec::new();
            for root in n.roots.iter().filter(|r| **r >= 0) {
                self.actor_node(
                    &n,
                    path,
                    *root as usize,
                    world,
                    index,
                    skeleton,
                    &mut active,
                    0,
                )?;
            }
            Ok(())
        })
    }
    /// Draws an actor's head parts (`npc_head_parts_unmorphed_v1`). A head
    /// not ready at the build's level, and each part that fails, is an
    /// entry of `Build::actor_head_omissions`; the body stays drawn.
    fn actor_head(
        &mut self,
        a: &fo4_scene::actor::Actor,
        head: &fo4_scene::actor::Head,
        world: Transform,
        index: u32,
        skeleton: usize,
    ) -> Result<()> {
        let mut causes = Vec::new();
        if !head.state.ready_at(self.options.level) {
            let src = &a.source;
            append(
                &mut causes,
                error(
                    "ActorHeadNotDrawable",
                    &src.file,
                    src.offset,
                    format!("{:?} at rule level {:?}", head.state, self.options.level),
                ),
                self.budget,
            )?;
            for e in head
                .blockers
                .iter()
                .filter(|x| fo4_scene::hypotheses::covering(x).is_none())
            {
                append(&mut causes, e.clone(), self.budget)?;
            }
        } else {
            self.allow(head.state.hypotheses());
            for part in &head.parts {
                let first = self.output.list.draws.len();
                if let Err(e) = self.actor_model(part.asset, world, index, skeleton) {
                    self.output.list.draws.truncate(first);
                    append(&mut causes, e, self.budget)?;
                }
            }
        }
        if !causes.is_empty() {
            append(
                &mut self.output.actor_head_omissions,
                Omission {
                    instance: index,
                    reference: a.reference.clone(),
                    causes,
                },
                self.budget,
            )?;
        }
        Ok(())
    }
}
/// Each block's chain of node blocks from a root of `n` down to it (the
/// block itself last), by the nodes' children.
fn node_chains(n: &Nif<'_>, file: &str, b: &Budget) -> Result<BTreeMap<usize, Vec<usize>>> {
    let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    for r in n.roots.iter().filter(|r| **r >= 0) {
        stack.push((*r as usize, Vec::new()));
    }
    while let Some((i, mut chain)) = stack.pop() {
        if out.contains_key(&i) || chain.contains(&i) || i >= n.blocks.len() {
            continue;
        }
        b.charge(8 * (chain.len() as u64 + 1), file, 0)?;
        chain.push(i);
        if let Some(Data::Node { children, .. }) = n.blocks[i].decoded.as_ref() {
            for c in children.iter().filter(|c| **c >= 0) {
                stack.push((*c as usize, chain.clone()));
            }
        }
        out.insert(i, chain);
    }
    Ok(out)
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
/// [`build`], then the scene's placed actors as further instances, in the
/// order of `actors` (`fo4_scene::actor::resolve`). An actor that cannot be
/// drawn at the options' level is an omission with its reasons.
pub fn build_with_actors(
    scene: &Scene,
    actors: &fo4_scene::actor::Actors,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<Build> {
    build_inner(scene, Some(actors), None, loader, limits, options, false, b).map(|x| x.0)
}
/// [`build_with_actors`], with the actors whose skeleton asset is a key of
/// `poses` skinned in that pose instead of the skeleton's rest pose; their
/// skinned meshes are returned with the build so that they can be posed
/// again. With no pose given, the draw list is that of
/// [`build_with_actors`].
#[allow(clippy::too_many_arguments)]
pub fn build_with_posed_actors(
    scene: &Scene,
    actors: &fo4_scene::actor::Actors,
    poses: BTreeMap<usize, fo4_scene::actor::pose::Skeleton>,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    b: &Budget,
) -> Result<(Build, Vec<SkinnedMesh>)> {
    build_inner(
        scene,
        Some(actors),
        Some(poses),
        loader,
        limits,
        options,
        false,
        b,
    )
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
    build_inner(
        scene,
        None,
        None,
        loader,
        limits,
        options,
        include_marker_subtrees,
        b,
    )
    .map(|x| x.0)
}
#[allow(clippy::too_many_arguments)]
fn build_inner(
    scene: &Scene,
    actors: Option<&fo4_scene::actor::Actors>,
    poses: Option<BTreeMap<usize, fo4_scene::actor::pose::Skeleton>>,
    loader: &Loader<'_>,
    limits: &Limits,
    options: BuildOptions,
    include_marker_subtrees: bool,
    b: &Budget,
) -> Result<(Build, Vec<SkinnedMesh>)> {
    if options.max_texture_dimension == 0
        || options.max_texture_bytes == 0
        || options.blocks.is_some_and(|b| {
            b.max_dimension == 0 || b.max_alpha_tested_dimension == 0 || b.max_bytes == 0
        })
    {
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
            actors: Vec::new(),
            actor_rigid_shapes: 0,
            actor_head_omissions: Vec::new(),
            list: DrawList::default(),
            instances: Vec::new(),
            omitted: Vec::new(),
            texture_choices: Vec::new(),
            effect_draws: Vec::new(),
            chains: Vec::new(),
            left_out: Vec::new(),
        },
        meshes: BTreeMap::new(),
        posed: poses
            .as_ref()
            .map(|p| p.keys().copied().collect())
            .unwrap_or_default(),
        skeletons: poses.unwrap_or_default(),
        recorded: Vec::new(),
        textures: BTreeMap::new(),
        texture_bytes: 0,
        block_bytes: 0,
        visits: 0,
        allow_path_strip: false,
        allow_path_root: false,
        allow_path_relative: false,
        allow_effect_color: false,
        allow_effect_surface: false,
        allow_missing_embedded: false,
        texture_first_key: false,
        clock_rest: false,
        transform_clock: false,
        node_tracks: BTreeMap::new(),
        allow_scol_parts: false,
        allow_editor_marker_subtrees: false,
        allow_marker_suffix: false,
        include_marker_subtrees,
        allow_visibility_marker: false,
        allow_swap_folder: false,
        allow_swap_empty: false,
        allow_swap_pattern: fo4_scene::hypotheses::usable_at(
            fo4_scene::hypotheses::SWAP_PATTERN,
            options.level,
        ),
        allow_swap_pattern_missing: false,
        pattern_used: std::cell::Cell::new(0),
        uv_left_out: std::collections::BTreeSet::new(),
        draw_rules: Vec::new(),
        missing_bones_at_bind: false,
        missing_bones_nearest: false,
        platform: false,
        node_chains: BTreeMap::new(),
        static_parts: false,
        parts_at_rest: false,
        parts: None,
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
        builder.clock_rest = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::CLOCK_FREE_RUNNING);
        builder.transform_clock = [
            fo4_scene::hypotheses::CLOCK_TRANSLATION,
            fo4_scene::hypotheses::CLOCK_ROTATION_QUATERNION,
            fo4_scene::hypotheses::CLOCK_ROTATION_XYZ,
            fo4_scene::hypotheses::CLOCK_SCALE,
        ]
        .iter()
        .any(|r| i.state.hypotheses().contains(r));
        // Effect shaders of a model settled by the first-key rule, or run
        // by the clock, are drawn by `draw::effect`.
        builder.allow_effect_surface = builder.clock_rest
            || i.state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::REST_EFFECT_FIRST_KEY);
        builder.allow_missing_embedded = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::MISSING_MATERIAL_EMBEDDED);
        builder.texture_first_key = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::REST_TEXTURE_FIRST_KEY);
        builder.platform = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::PLAY_PLATFORM_LEFT_OUT);
        builder.static_parts = {
            let h = i.state.hypotheses();
            h.contains(&fo4_scene::hypotheses::STATIC_PARTS)
                || h.contains(&fo4_scene::hypotheses::ACTIVATOR_PARTS)
                || h.contains(&fo4_scene::hypotheses::FURNITURE_PARTS)
                || h.contains(&fo4_scene::hypotheses::FURNITURE_AT_REST)
        };
        builder.parts_at_rest = i
            .state
            .hypotheses()
            .contains(&fo4_scene::hypotheses::FURNITURE_AT_REST);
        let first = builder.output.list.draws.len();
        builder.draw_rules.clear();
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
            if !i.state.ready_at(options.level) {
                return Err(error(
                    "SceneInstanceNotDrawable",
                    &src.file,
                    src.offset,
                    format!("{:?} at rule level {:?}", i.state, options.level),
                ));
            }
            // A free-running controller whose value depends on the clock
            // draws nothing without a time.
            if builder.clock_rest && options.time.is_none() {
                return Err(error(
                    "UnsettledControllerTime",
                    &src.file,
                    src.offset,
                    "a build time for a controller the game clock runs",
                ));
            }
            // Ready only at the hypotheses level when it has an XESP: the
            // enable parent rules say whether it starts enabled.
            if i.disabled_by_enable_parent() {
                return Err(error(
                    "DisabledThroughEnableParent",
                    &src.file,
                    src.offset,
                    fo4_scene::hypotheses::ENABLE_FOLLOWS,
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
            builder.allow_swap_folder = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::SWAP_FOLDER);
            builder.allow_swap_empty = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::SWAP_EMPTY_ENTRY);
            builder.allow_swap_pattern_missing = i
                .state
                .hypotheses()
                .contains(&fo4_scene::hypotheses::SWAP_PATTERN_MISSING);
            builder.pattern_used.set(0);
            let world = Transform::euler(p, scale)?;
            let swaps: Vec<&Loaded<Mswp>> = i.material_swap.iter().collect();
            builder.base(base, world, index, &swaps, 0)?;
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
        if attempt.is_ok() {
            builder.record_draw_rules()?;
        }
        if let Err(e) = attempt {
            builder.output.list.draws.truncate(first);
            builder.output.list.animations.retain(|a| a.draw < first);
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
    let Some(actors) = actors else {
        return Ok((builder.output, builder.recorded));
    };
    let mut index = u32::try_from(builder.output.instances.len())
        .map_err(|e| error("RenderInstanceLimit", "scene", 0, e))?;
    for a in &actors.actors {
        b.charge(
            (a.reference.plugin.len() + a.base.as_ref().map_or(0, |v| v.plugin.len())) as u64 + 128,
            "draw identity",
            0,
        )?;
        let mut hypotheses = Vec::new();
        b.reserve(
            &mut hypotheses,
            a.state.hypotheses().len(),
            "draw hypotheses",
            0,
        )?;
        hypotheses.extend_from_slice(a.state.hypotheses());
        append(
            &mut builder.output.instances,
            Identity {
                index,
                reference: a.reference.clone(),
                base: a.base.clone(),
                hypotheses,
                editor_marker: false,
            },
            b,
        )?;
        append(&mut builder.output.actors, index, b)?;
        let first = builder.output.list.draws.len();
        builder.draw_rules.clear();
        let attempt = builder.actor(a, index);
        if attempt.is_ok() {
            builder.record_draw_rules()?;
        }
        if let Err(e) = attempt {
            builder.output.list.draws.truncate(first);
            builder.output.list.animations.retain(|a| a.draw < first);
            let mut causes = Vec::new();
            for e in std::iter::once(&e).chain(
                a.blockers
                    .iter()
                    .filter(|x| fo4_scene::hypotheses::covering(x).is_none()),
            ) {
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
                    reference: a.reference.clone(),
                    causes,
                },
                b,
            )?;
        }
        index = index
            .checked_add(1)
            .ok_or_else(|| error("RenderInstanceLimit", "scene", 0, index))?;
    }
    Ok((builder.output, builder.recorded))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swap_scope_readings_agree_only_where_no_order_matters() {
        let b = Budget::new(1 << 16);
        type Pairs = Vec<(&'static [u8], &'static [u8])>;
        let apply = |p: Vec<u8>, s: &Pairs| -> Result<Vec<u8>> {
            Ok(s.iter()
                .find(|(from, _)| *from == p.as_slice())
                .map_or(p, |(_, to)| to.to_vec()))
        };
        let read = |path: &[u8], swaps: &[Pairs]| readings(path, swaps, &b, apply).unwrap();
        let agree = |r: &[Vec<u8>; 4]| r.iter().all(|x| *x == r[0]);
        // The same mapping in both scopes, and a path neither maps.
        let same: Vec<Pairs> = vec![vec![(b"a", b"b")], vec![(b"a", b"b")]];
        assert!(agree(&read(b"a", &same)));
        assert_eq!(read(b"a", &same)[0], b"b");
        assert!(agree(&read(b"c", &same)));
        // Only the inner (base) swap maps it: the outer-alone reading keeps it.
        let inner_only: Vec<Pairs> = vec![vec![(b"x", b"y")], vec![(b"a", b"b")]];
        let r = read(b"a", &inner_only);
        assert_eq!(
            r,
            [b"a".to_vec(), b"b".to_vec(), b"b".to_vec(), b"b".to_vec()]
        );
        assert!(!agree(&r));
        // Chained: the two orders differ.
        let chain: Vec<Pairs> = vec![vec![(b"b", b"c")], vec![(b"a", b"b")]];
        let r = read(b"a", &chain);
        assert_eq!(r[2], b"b");
        assert_eq!(r[3], b"c");
        assert!(readings::<Pairs>(b"a", &[], &b, apply).is_err());
    }
    #[test]
    fn swap_scopes_whose_readings_differ_settle_outermost_first_unless_chained() {
        let b = Budget::new(1 << 16);
        type Pairs = Vec<(&'static [u8], &'static [u8])>;
        let apply = |p: Vec<u8>, s: &Pairs| -> Result<Vec<u8>> {
            Ok(s.iter()
                .find(|(from, _)| *from == p.as_slice())
                .map_or(p, |(_, to)| to.to_vec()))
        };
        let ruled = |m: &[u8], outer, replace| Settled::Ruled {
            material: m.to_vec(),
            outer,
            replace,
        };
        let go = |path: &[u8], swaps: &[Pairs]| settle(path, swaps, &b, apply).unwrap();
        // Both scopes map it: the outer (reference) target wins.
        let both: Vec<Pairs> = vec![vec![(b"a", b"c")], vec![(b"a", b"b")]];
        assert_eq!(go(b"a", &both), ruled(b"c", true, false));
        // Only the outer maps it: its target, against the inner-alone reading.
        let outer: Vec<Pairs> = vec![vec![(b"a", b"c")], vec![(b"x", b"y")]];
        assert_eq!(go(b"a", &outer), ruled(b"c", true, false));
        // Only the inner maps it: the outer swap replaces it, the path stays.
        let inner: Vec<Pairs> = vec![vec![(b"x", b"y")], vec![(b"a", b"b")]];
        assert_eq!(go(b"a", &inner), ruled(b"a", false, true));
        // Three scopes, only the middle one maps it: replaced too.
        let middle: Vec<Pairs> = vec![vec![], vec![(b"a", b"m")], vec![]];
        assert_eq!(go(b"a", &middle), ruled(b"a", false, true));
        // A scope maps what another put in place: the chain is followed,
        // outermost first among the scopes that map the current material,
        // each scope once.
        let chained = |m: &[u8]| Settled::Chained {
            material: m.to_vec(),
        };
        let chain: Vec<Pairs> = vec![vec![(b"b", b"c")], vec![(b"a", b"b")]];
        assert_eq!(go(b"a", &chain), chained(b"c"));
        let chain: Vec<Pairs> = vec![vec![(b"a", b"b")], vec![(b"b", b"c")]];
        assert_eq!(go(b"a", &chain), chained(b"c"));
        // Both map the path; the outer's target is mapped again by the inner.
        let chain: Vec<Pairs> = vec![vec![(b"a", b"b")], vec![(b"b", b"c"), (b"a", b"d")]];
        assert_eq!(go(b"a", &chain), chained(b"c"));
        // Back to the path: each scope applies once.
        let chain: Vec<Pairs> = vec![vec![(b"b", b"a")], vec![(b"a", b"b")]];
        assert_eq!(go(b"a", &chain), chained(b"a"));
        // Nothing maps it.
        assert_eq!(go(b"z", &both), ruled(b"z", false, false));
    }
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
