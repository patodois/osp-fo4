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
}
impl SurfaceParams {
    pub fn bits(&self) -> [u32; 9] {
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

#[derive(Default)]
pub struct GpuScene {
    pub meshes: Vec<GpuMesh>,
    pub textures: Vec<GpuTexture>,
    pub surfaces: Vec<Surface>,
    pub instances: Vec<Instance>,
}
impl GpuScene {
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
        indices,
    })
}

/// Convert a validated draw list. `options` must be the CPU options the GPU
/// frame is compared with; only bilinear filtering is implemented. Stored
/// blocks are moved out of `list`, which no CPU render can then draw.
pub fn convert(list: &mut DrawList, options: &RenderOptions) -> Result<GpuScene, String> {
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
    // Texture, parameter bits, raw blend codes, culling, overlay.
    type SurfaceIdentity = (usize, [u32; 9], Option<(u32, u32)>, bool, bool);
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
        let mesh_slot = match mesh_index.get(&draw.mesh) {
            Some(&i) => i,
            None => {
                out.meshes.push(mesh(source)?);
                mesh_index.insert(draw.mesh, out.meshes.len() - 1);
                out.meshes.len() - 1
            }
        };
        let texture_slot = match texture_index.get(&draw.texture) {
            Some(&i) => i,
            None => {
                out.textures.push(texture(
                    &list.textures[draw.texture],
                    list.blocks.get_mut(&draw.texture),
                )?);
                texture_index.insert(draw.texture, out.textures.len() - 1);
                out.textures.len() - 1
            }
        };
        // The CPU culls only when asked, and then fails on a draw without a
        // sidedness declaration; the same error is raised here.
        let cull_back =
            options.backface_cull && !draw.sidedness.double_sided().map_err(|e| e.to_string())?;
        let blend_codes =
            (draw.alpha.enabled == 1).then_some((draw.alpha.source, draw.alpha.destination));
        let params = SurfaceParams {
            opaque_texture: out.textures[texture_slot].opaque,
            ..surface_params(draw.alpha, draw.uv_offset, draw.uv_scale)
        };
        let surface_key = (
            texture_slot,
            params.bits(),
            blend_codes,
            cull_back,
            list.is_overlay(di),
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
                    params,
                    key: SurfaceKey {
                        blend,
                        cull_back,
                        overlay: list.is_overlay(di),
                    },
                });
                surface_index.insert(surface_key, out.surfaces.len() - 1);
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
    }
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
