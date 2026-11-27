//! Bevy side: the unlit surface material, the reference projection, scene
//! upload, one offscreen frame, and the windowed free-fly viewer.
use crate::{
    cells::Message,
    coords,
    fly::{FlyInput, FlySettings, FlyState},
    live::{Live, Slot},
    scene::{GpuMesh, GpuScene, GpuTexture, Instance, Surface, TexelFormat},
    script::{Phase, Steer, SETTLE_FRAMES},
    stream::AssetKey,
    walk::WalkView,
};
use bevy::render::storage::ShaderBuffer;
use bevy::{
    asset::{embedded_asset, embedded_path, AssetPath, RenderAssetUsages},
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        CameraProjection, ClearColorConfig, Projection, RenderTarget, SubCameraView,
    },
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::AccumulatedMouseMotion,
    math::{Mat4, Vec3A},
    mesh::{Indices, MeshTag, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{
            AsBindGroup, BlendComponent, BlendOperation, BlendState, CachedPipelineState,
            CompareFunction, Extent3d, Face, PipelineCache, PollType, RenderPipelineDescriptor,
            ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
        },
        renderer::RenderDevice,
        settings::WgpuFeatures,
        view::screenshot::{Screenshot, ScreenshotCaptured},
        RenderApp, RenderPlugin,
    },
    shader::{ShaderCacheError, ShaderRef},
    window::{ExitCondition, PrimaryWindow},
    winit::WinitPlugin,
};
use fo4_render::BlockFormat;
use fo4_scene::camera::Camera as CpuCamera;
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc, Mutex,
};

pub struct ViewerPlugin;
impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "surface.wesl");
        app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default())
            .add_plugins((
                bevy::render::extract_resource::ExtractResourcePlugin::<SoftDepthBuffer>::default(),
                bevy::render::extract_component::ExtractComponentPlugin::<SoftDepthCamera>::default(
                ),
            ));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            use bevy::core_pipeline::{
                core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
                schedule::Core3d,
            };
            render.add_systems(
                Core3d,
                copy_soft_depth
                    .after(main_opaque_pass_3d)
                    .before(main_transparent_pass_3d),
            );
        }
    }
}

/// Bytes before the copied depth in [`SoftDepthBuffer`]: its header (width,
/// height, words a row, 1 when the depth below is this frame's) padded to
/// the copy offset alignment.
pub const SOFT_DEPTH_HEADER_BYTES: u64 = 256;
/// The window's soft depth buffer holds a view of up to this size; a larger
/// view draws its soft surfaces without the fade (`--draw-soft-effects`).
pub const SOFT_DEPTH_MAX_VIEW: [u32; 2] = [4096, 2880];

/// `--draw-soft-effects` (`effect_soft_depth_fade_linear_v1`): the buffer
/// every material binds at 5, into which the colour camera's opaque depth
/// is copied after the opaque pass and before the blended one
/// ([`copy_soft_depth`]); its header says whether it holds this frame's.
/// Without the switch a header alone (nothing copied, no fade).
#[derive(Resource, Clone, bevy::render::extract_resource::ExtractResource)]
#[extract_app(RenderApp)]
pub struct SoftDepthBuffer(pub Handle<ShaderBuffer>);

/// The window was started with `--draw-soft-effects` ([`Aids::soft_effects`]).
#[derive(Resource)]
struct SoftEffectsOn;

/// The camera whose opaque depth [`copy_soft_depth`] copies.
#[derive(Component, Clone, Copy, bevy::render::extract_component::ExtractComponent)]
#[extract_app(RenderApp)]
pub struct SoftDepthCamera;

/// Bytes of the soft depth buffer for a view of `size` (rows padded to 256
/// bytes, as a copy from a texture to a buffer asks).
pub fn soft_depth_bytes(size: [u32; 2]) -> (u32, u64) {
    let row = (size[0] * 4).div_ceil(256) * 256;
    (
        row,
        SOFT_DEPTH_HEADER_BYTES + u64::from(row) * u64::from(size[1]),
    )
}

/// The soft depth buffer every material binds: inserted for a view of
/// `size` (`Some`: the switch is on), or found, or a header alone.
fn soft_depth_buffer(world: &mut World, size: Option<[u32; 2]>) -> Handle<ShaderBuffer> {
    if size.is_none() {
        if let Some(b) = world.get_resource::<SoftDepthBuffer>() {
            return b.0.clone();
        }
    }
    let bytes = size.map_or(SOFT_DEPTH_HEADER_BYTES, |s| soft_depth_bytes(s).1);
    world.init_resource::<Assets<ShaderBuffer>>();
    let handle = world
        .resource_mut::<Assets<ShaderBuffer>>()
        .add(ShaderBuffer::with_size(
            bytes,
            RenderAssetUsages::RENDER_WORLD,
        ));
    world.insert_resource(SoftDepthBuffer(handle.clone()));
    handle
}

/// Copies the soft depth camera's opaque depth into the soft depth buffer
/// and writes its header (a render system between Bevy's opaque and
/// blended passes): the blended surfaces then read the depth every opaque
/// draw left, as the CPU's `opaque_depth` holds it. A view larger than the
/// buffer, or a depth that cannot be copied, writes a header that says so
/// and nothing fades.
fn copy_soft_depth(
    view: bevy::render::renderer::ViewQuery<
        &bevy::render::view::ViewDepthStencilTexture,
        With<SoftDepthCamera>,
    >,
    soft: Option<Res<SoftDepthBuffer>>,
    buffers: Res<bevy::render::render_asset::RenderAssets<bevy::render::storage::GpuShaderBuffer>>,
    queue: Res<bevy::render::renderer::RenderQueue>,
    mut ctx: bevy::render::renderer::RenderContext,
) {
    use bevy::render::render_resource::{
        TexelCopyBufferInfo, TexelCopyBufferLayout, TextureUsages,
    };
    let Some(soft) = soft else {
        return;
    };
    let Some(gpu) = buffers.get(&soft.0) else {
        return;
    };
    let texture = view.into_inner().texture();
    let size = texture.size();
    let (row, bytes) = soft_depth_bytes([size.width, size.height]);
    let fits = bytes <= gpu.buffer.size() && texture.usage().contains(TextureUsages::COPY_SRC);
    let header: Vec<u8> = [size.width, size.height, row / 4, u32::from(fits)]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    queue.write_buffer(&gpu.buffer, 0, &header);
    if !fits {
        return;
    }
    ctx.command_encoder().copy_texture_to_buffer(
        texture.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &gpu.buffer,
            layout: TexelCopyBufferLayout {
                offset: SOFT_DEPTH_HEADER_BYTES,
                bytes_per_row: Some(row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
}

/// The colour camera's depth usages with the soft depth copy.
fn soft_depth_camera() -> (Camera3d, SoftDepthCamera) {
    use bevy::render::render_resource::TextureUsages;
    (
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC)
                .into(),
            ..default()
        },
        SoftDepthCamera,
    )
}

/// Uniform block of `surface.wesl` (`SurfaceParams`), field for field.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct SurfaceUniform {
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    pub test_ref: f32,
    pub opacity: f32,
    pub test_function: u32,
    pub skip_zero_alpha: u32,
    pub opaque_texture: u32,
    /// 1: `falloff` holds an effect falloff (start and stop angle as
    /// cosines, start and stop opacity).
    pub falloff_on: u32,
    /// Palette lookups: bit 0 colour, bit 1 alpha.
    pub palette_lookups: u32,
    pub falloff: Vec4,
    /// `SurfaceParams::colour` (RGB; 1 without one).
    pub tint: Vec4,
    /// Bit 0: lit by the light buffer (`SurfaceParams::lit`); bit 1: lit
    /// with the vertex normals (`SurfaceParams::smooth`).
    pub mode: u32,
    /// `SurfaceParams::soft_depth` (0 without one: no fade).
    pub soft_depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialKey {
    pub surface: crate::scene::SurfaceKey,
    /// Instance-tag pass: no blending, depth written, tag bytes as colour.
    pub tag_pass: bool,
    /// A pipeline of its own for this surface (its index plus one; 0:
    /// shared by key). Bevy orders opaque draws by pipeline, then by the
    /// slot each material was given, which follows a hash of asset ids that
    /// changes from build to build; with one pipeline per surface,
    /// specialised in spawn order, exact depth ties resolve the same way in
    /// every build and run (headless frames).
    pub rank: u32,
    /// The sky of the light buffer ([`spawn_sky`]): the shader's `sky`
    /// entry point, at the far depth, kept only where nothing is drawn.
    pub sky: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(MaterialKey)]
pub struct SurfaceMaterial {
    #[uniform(0)]
    pub params: SurfaceUniform,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
    /// The palette of a palette lookup (sampled with the texture's sampler
    /// at its base level, the coordinates kept inside its edge texel
    /// centres, which is a clamp to the edge); Bevy's fallback image when
    /// the surface has none.
    #[texture(3)]
    pub palette: Option<Handle<Image>>,
    /// The frame's light (`scene::light_words`), one buffer for every
    /// material; a buffer without light when the frame has none.
    #[storage(4, read_only)]
    pub lights: Handle<ShaderBuffer>,
    /// The opaque depth after the opaque pass ([`SoftDepthBuffer`]), one
    /// buffer for every material.
    #[storage(5, read_only)]
    pub soft_depth: Handle<ShaderBuffer>,
    pub key: MaterialKey,
}

/// The light buffer every material binds ([`SurfaceMaterial::lights`]).
#[derive(Resource, Clone)]
struct LightBuffer(Handle<ShaderBuffer>);

/// Inserts the light buffer of `light` (a buffer without light for `None`)
/// and returns its handle.
fn insert_light_buffer(
    world: &mut World,
    light: Option<&crate::scene::GpuLight>,
) -> Handle<ShaderBuffer> {
    let words = match light {
        Some(l) => l.words.clone(),
        None => vec![0f32; 4 * crate::scene::LIGHT_HEADER_ROWS],
    };
    // An app has the asset store; a bare world (the tests') gets one.
    world.init_resource::<Assets<ShaderBuffer>>();
    let handle = world
        .resource_mut::<Assets<ShaderBuffer>>()
        .add(ShaderBuffer::new(words, RenderAssetUsages::RENDER_WORLD));
    world.insert_resource(LightBuffer(handle.clone()));
    handle
}
impl From<&SurfaceMaterial> for MaterialKey {
    fn from(m: &SurfaceMaterial) -> Self {
        m.key
    }
}

impl Material for SurfaceMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("surface.wesl")).with_source("embedded"),
        )
    }
    /// Blended surfaces go to Bevy's transparent phase (sorted back to front,
    /// no depth write), like the CPU's second, sorted pass of blended
    /// triangles that test against but never write the opaque depth.
    fn alpha_mode(&self) -> AlphaMode {
        if !self.key.tag_pass && self.key.surface.blend.is_some() {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        }
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_oit() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;
        descriptor.primitive.cull_mode = key.surface.cull_back.then_some(Face::Back);
        // The CPU keeps a sample only when it is strictly nearer than the
        // stored depth (`depth >= opaque_depth` is rejected); Bevy's default
        // GreaterEqual (reverse Z) would also keep exact ties. An overlay
        // draw (the terrain's texture passes) keeps exact ties in colour, as
        // the CPU does; its tag never replaces a stored one at equal depth.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(if key.surface.overlay && !key.tag_pass {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Greater
            });
            // The sky writes the far depth (0, reverse Z) and no depth: it
            // passes only where nothing has been drawn, before or after it.
            if key.sky {
                depth.depth_compare = Some(CompareFunction::GreaterEqual);
                depth.depth_write_enabled = Some(false);
            }
        }
        // Material pipelines always have a fragment stage with one target.
        let Some(fragment) = descriptor.fragment.as_mut() else {
            return Ok(());
        };
        let srgb = fragment
            .targets
            .first()
            .and_then(Option::as_ref)
            .is_some_and(|t| t.format.is_srgb());
        // The module holds two entry points: the surfaces' and the sky's.
        fragment.entry_point = Some(if key.sky { "sky" } else { "fragment" }.into());
        if key.tag_pass {
            fragment.shader_defs.push("INSTANCE_TAG".into());
        } else if srgb {
            fragment.shader_defs.push("TARGET_SRGB".into());
        }
        if let Some(Some(target)) = fragment.targets.first_mut() {
            target.blend = if key.tag_pass {
                None
            } else {
                key.surface.blend.map(|(src, dst)| {
                    let c = BlendComponent {
                        src_factor: src,
                        dst_factor: dst,
                        operation: BlendOperation::Add,
                    };
                    BlendState { color: c, alpha: c }
                })
            };
        }
        Ok(())
    }
}

/// `Camera::project_coordinates` as a Bevy projection. With `follow_aspect`
/// (windowed mode) a resize keeps the horizontal slopes and sets the vertical
/// ones from the aspect ratio, as `reference_slopes` does; otherwise the
/// frustum is fixed to the CPU camera's.
#[derive(Debug, Clone)]
pub struct ViewerProjection {
    pub frustum: [f64; 6],
    pub follow_aspect: bool,
}
impl CameraProjection for ViewerProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        coords::clip_from_view(self.frustum)
    }
    fn get_clip_from_view_for_sub(&self, sub: &SubCameraView) -> Mat4 {
        coords::clip_from_view(coords::sub_frustum(
            self.frustum,
            [f64::from(sub.full_size.x), f64::from(sub.full_size.y)],
            [f64::from(sub.offset.x), f64::from(sub.offset.y)],
            [f64::from(sub.size.x), f64::from(sub.size.y)],
        ))
    }
    fn update(&mut self, width: f32, height: f32) {
        if self.follow_aspect && width > 0. && height > 0. {
            let top = self.frustum[1] * f64::from(height) / f64::from(width);
            self.frustum[2] = top;
            self.frustum[3] = -top;
        }
    }
    fn far(&self) -> f32 {
        self.frustum[5] as f32
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        coords::frustum_corners(self.frustum, z_near, z_far)
    }
}

struct Uploaded {
    meshes: Vec<Handle<Mesh>>,
    images: Vec<Handle<Image>>,
}

/// The wgpu format of a texture's texels: RGBA16F for decoded texels, the
/// matching block format (unorm: stored values, as the CPU reads them) for
/// stored blocks.
fn texel_format(format: TexelFormat) -> TextureFormat {
    match format {
        TexelFormat::Rgba16Float => TextureFormat::Rgba16Float,
        TexelFormat::Blocks(b) => match b {
            BlockFormat::Bc1 | BlockFormat::Bc1Alpha => TextureFormat::Bc1RgbaUnorm,
            BlockFormat::Bc2 => TextureFormat::Bc2RgbaUnorm,
            BlockFormat::Bc3 => TextureFormat::Bc3RgbaUnorm,
            BlockFormat::Bc4 => TextureFormat::Bc4RUnorm,
            BlockFormat::Bc5 => TextureFormat::Bc5RgUnorm,
            BlockFormat::Bc7 => TextureFormat::Bc7RgbaUnorm,
        },
    }
}

/// Texture bytes added per frame at most (one texture at least): each
/// frame's staging copies are freed before the next batch, so all the
/// textures are never held twice at once.
const UPLOAD_BATCH_BYTES: usize = 64 << 20;

/// Uploads the meshes; textures follow in batches ([`upload_textures`]).
/// Stored blocks need a device that samples BC formats.
fn upload_meshes(world: &mut World, scene: &GpuScene) -> Result<Uploaded, String> {
    let blocks = scene
        .textures
        .iter()
        .any(|t| matches!(t.format, TexelFormat::Blocks(_)));
    if blocks && !samples_blocks(world) {
        return Err(
            "GpuWithoutBcTextures: this device cannot sample block-compressed textures; use --reference-textures"
                .into(),
        );
    }
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let meshes = scene
        .meshes
        .iter()
        .map(|m| {
            meshes.add(bevy_mesh(GpuMesh {
                positions: m.positions.clone(),
                uvs: m.uvs.clone(),
                colors: m.colors.clone(),
                normals: m.normals.clone(),
                indices: m.indices.clone(),
            }))
        })
        .collect();
    Ok(Uploaded {
        meshes,
        images: Vec::new(),
    })
}

/// Whether the device samples block-compressed textures.
fn samples_blocks(world: &World) -> bool {
    world
        .resource::<RenderDevice>()
        .features()
        .contains(WgpuFeatures::TEXTURE_COMPRESSION_BC)
}

/// A triangle-list mesh of `m`'s data, moved in.
fn bevy_mesh(m: GpuMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs)
    .with_inserted_indices(Indices::U16(m.indices));
    if let Some(colors) = m.colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    if let Some(normals) = m.normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    }
    mesh
}

/// The image of `t`, its bytes moved in.
fn bevy_image(t: &mut GpuTexture) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texel_format(t.format),
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = t.levels;
    image.data = Some(std::mem::take(&mut t.bytes));
    // Wrap and bilinear within one level, the level chosen in the
    // shader: the CPU `Texture::sample_lod` with `bilinear`.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image
}

/// Adds the next batch of textures, their bytes moved into the images;
/// true once every texture has been added.
fn upload_textures(world: &mut World, scene: &mut GpuScene, up: &mut Uploaded) -> bool {
    let mut images = world.resource_mut::<Assets<Image>>();
    let mut bytes = 0;
    while let Some(t) = scene.textures.get_mut(up.images.len()) {
        if bytes > 0 && bytes + t.bytes.len() > UPLOAD_BATCH_BYTES {
            return false;
        }
        bytes += t.bytes.len();
        up.images.push(images.add(bevy_image(t)));
    }
    true
}

fn spawn(
    world: &mut World,
    scene: &GpuScene,
    up: &Uploaded,
    tag_pass: bool,
    layer: usize,
    ranked: bool,
) -> Vec<Entity> {
    spawn_instances(
        world,
        &scene.surfaces,
        &scene.instances,
        &up.meshes,
        &up.images,
        tag_pass,
        layer,
        ranked,
    )
}

/// The uniform block of `p`.
fn uniform(p: &crate::scene::SurfaceParams) -> SurfaceUniform {
    SurfaceUniform {
        uv_scale: Vec2::from_array(p.uv_scale),
        uv_offset: Vec2::from_array(p.uv_offset),
        test_ref: p.test_ref,
        opacity: p.opacity,
        test_function: p.test_function,
        skip_zero_alpha: u32::from(p.skip_zero_alpha),
        opaque_texture: u32::from(p.opaque_texture),
        falloff_on: u32::from(p.falloff.is_some()),
        palette_lookups: p.palette,
        falloff: Vec4::from_array(p.falloff.unwrap_or([0.; 4])),
        tint: {
            let c = p.colour.unwrap_or([1.; 3]);
            Vec4::new(c[0], c[1], c[2], 1.)
        },
        mode: u32::from(p.lit) | u32::from(p.smooth) << 1,
        soft_depth: p.soft_depth.unwrap_or(0.),
    }
}

/// The material of `surface` drawing `image`, with its own pipeline when
/// `rank` is not 0 ([`MaterialKey::rank`]).
fn surface_material(
    surface: &Surface,
    image: Handle<Image>,
    palette: Option<Handle<Image>>,
    lights: Handle<ShaderBuffer>,
    soft_depth: Handle<ShaderBuffer>,
    tag_pass: bool,
    rank: u32,
) -> SurfaceMaterial {
    SurfaceMaterial {
        params: uniform(&surface.params),
        texture: image,
        palette,
        lights,
        soft_depth,
        key: MaterialKey {
            surface: surface.key,
            tag_pass,
            rank,
            sky: false,
        },
    }
}

/// The components of one drawn instance.
type InstanceBundle = (
    Mesh3d,
    MeshMaterial3d<SurfaceMaterial>,
    MeshTag,
    Transform,
    NoFrustumCulling,
    RenderLayers,
);

/// One drawn instance.
fn instance_bundle(
    mesh: Handle<Mesh>,
    material: Handle<SurfaceMaterial>,
    instance: &Instance,
    layer: usize,
) -> InstanceBundle {
    (
        Mesh3d(mesh),
        MeshMaterial3d(material),
        // `Draw::instance`, read by the tag pass's shader.
        MeshTag::new(instance.tag),
        Transform {
            translation: instance.translation,
            rotation: instance.rotation,
            scale: instance.scale,
        },
        // The CPU draws every triangle and clips; no culling by bounds.
        NoFrustumCulling,
        RenderLayers::layer(layer),
    )
}

/// Spawns `instances`, whose surfaces index `surfaces`, with the uploaded
/// `meshes` and `images` their indices name; returns the entities. With
/// `ranked` each surface has a pipeline of its own ([`MaterialKey::rank`]).
#[allow(clippy::too_many_arguments)]
fn spawn_instances(
    world: &mut World,
    surfaces: &[Surface],
    instances: &[Instance],
    meshes: &[Handle<Mesh>],
    images: &[Handle<Image>],
    tag_pass: bool,
    layer: usize,
    ranked: bool,
) -> Vec<Entity> {
    let lights = match world.get_resource::<LightBuffer>() {
        Some(b) => b.0.clone(),
        None => insert_light_buffer(world, None),
    };
    let soft = soft_depth_buffer(world, None);
    let mut materials = world.resource_mut::<Assets<SurfaceMaterial>>();
    let mut handles = std::collections::BTreeMap::new();
    let mut entities = Vec::with_capacity(instances.len());
    for instance in instances {
        let surface = &surfaces[instance.surface];
        // One material per surface in both passes: the tag pass reads each
        // instance's tag from its `MeshTag`.
        let rank = if ranked {
            u32::try_from(instance.surface)
                .ok()
                .and_then(|r| r.checked_add(1))
                .unwrap_or(u32::MAX)
        } else {
            0
        };
        let material = handles
            .entry(instance.surface)
            .or_insert_with(|| {
                materials.add(surface_material(
                    surface,
                    images[surface.texture].clone(),
                    surface.palette.map(|p| images[p].clone()),
                    lights.clone(),
                    soft.clone(),
                    tag_pass,
                    rank,
                ))
            })
            .clone();
        entities.push(instance_bundle(
            meshes[instance.mesh].clone(),
            material,
            instance,
            layer,
        ));
    }
    if !ranked {
        return world.spawn_batch(entities).collect();
    }
    // Bevy specialises and queues the newest entity first: spawned last to
    // first, the first instance of the list gets the first pipeline and is
    // drawn first, so it wins an exact depth tie as in the CPU frame.
    let mut spawned: Vec<Entity> = world.spawn_batch(entities.into_iter().rev()).collect();
    spawned.reverse();
    spawned
}

/// The sky of a headless colour frame: a cube around the eye on the colour
/// pass's layer whose material colours each pixel by its view direction
/// from the light buffer's sky at the far depth, so that it shows only
/// where nothing is drawn and blended surfaces blend over it (as
/// `fo4_render::render_lit` fills the background before any draw).
fn spawn_sky(
    world: &mut World,
    eye: Vec3,
    layer: usize,
    lights: Handle<ShaderBuffer>,
    up: &Uploaded,
) -> Entity {
    let half = 100.0f32;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    let corners: Vec<[f32; 3]> = (0..8)
        .map(|k| {
            [
                if k & 1 == 0 { -half } else { half },
                if k & 2 == 0 { -half } else { half },
                if k & 4 == 0 { -half } else { half },
            ]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1., 0.]; 8]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32, 0.]; 8]);
    mesh.insert_indices(Indices::U16(vec![
        0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2, 6, 0, 6, 4,
        1, 5, 7, 1, 7, 3,
    ]));
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let image = up.images.first().cloned().unwrap_or_default();
    let material = SurfaceMaterial {
        params: SurfaceUniform {
            uv_scale: Vec2::ONE,
            uv_offset: Vec2::ZERO,
            test_ref: 0.,
            opacity: 1.,
            test_function: crate::scene::TEST_ALWAYS,
            skip_zero_alpha: 0,
            opaque_texture: 0,
            falloff_on: 0,
            palette_lookups: 0,
            falloff: Vec4::ZERO,
            tint: Vec4::ONE,
            mode: 0,
            soft_depth: 0.,
        },
        texture: image,
        palette: None,
        lights,
        soft_depth: soft_depth_buffer(world, None),
        key: MaterialKey {
            surface: crate::scene::SurfaceKey {
                blend: None,
                cull_back: false,
                overlay: false,
            },
            tag_pass: false,
            rank: 0,
            sky: true,
        },
    };
    let material = world
        .resource_mut::<Assets<SurfaceMaterial>>()
        .add(material);
    world
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(eye),
            NoFrustumCulling,
            RenderLayers::layer(layer),
        ))
        .id()
}

fn camera_bundle(
    pose: (Vec3, Quat),
    frustum: [f64; 6],
    follow_aspect: bool,
    clear: Color,
    order: isize,
    layer: usize,
) -> impl Bundle {
    (
        Camera3d::default(),
        Camera {
            order,
            clear_color: ClearColorConfig::Custom(clear),
            ..default()
        },
        Projection::custom(ViewerProjection {
            frustum,
            follow_aspect,
        }),
        Transform::from_translation(pose.0).with_rotation(pose.1),
        // One sample per pixel, no tone curve, no dither: the CPU frame.
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(layer),
    )
}

pub struct Frames {
    /// RGBA8 as stored, row-major, top row first.
    pub color: Vec<u8>,
    /// Little-endian `Draw::instance` per pixel, `u32::MAX` for background.
    pub tags: Option<Vec<u8>>,
}

const MAX_FRAMES: u32 = 600;
const READY_FRAMES: u32 = 3;

/// Whether every pipeline of `app`'s render world is ready: `Ok(true)`
/// when none is waiting, `Ok(false)` while some are queued, being created,
/// or waiting for a shader or a shader import that the asset server is
/// still loading (Bevy keeps those in an error state for one frame and
/// retries them, `ShaderNotLoaded` and `ShaderImportNotYetAvailable`), and
/// `Err` for a pipeline that failed for good. Shaders load on Bevy's task
/// pools, which every app of a process shares, so how many frames the
/// loads take depends on what else the process runs (tests in parallel).
pub(crate) fn pipelines_ready(app: &App) -> Result<bool, String> {
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    for p in cache.pipelines() {
        if let CachedPipelineState::Err(e) = &p.state {
            if !matches!(
                e,
                ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable
            ) {
                return Err(format!("PipelineError: {e}"));
            }
        }
    }
    Ok(cache.waiting_pipelines().next().is_none())
}

/// One offscreen frame of `camera`. Renders until no pipeline is waiting,
/// then captures until two consecutive frames are byte-identical, so a frame
/// with an asset or pipeline still loading is never returned.
pub fn headless(
    scene: &mut GpuScene,
    camera: &CpuCamera,
    clear: [f32; 4],
    want_tags: bool,
) -> Result<Frames, String> {
    let pose = coords::camera_pose(camera)?;
    let [width, height] = camera.size;
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>()
            // Keep the render world in this thread so its pipeline cache can
            // be inspected between frames.
            .disable::<PipelinedRenderingPlugin>(),
    )
    .add_plugins(ViewerPlugin);
    app.finish();
    app.cleanup();

    let mut up = upload_meshes(app.world_mut(), scene)?;
    while !upload_textures(app.world_mut(), scene, &mut up) {
        app.update();
        app.world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("GpuPoll: {e}"))?;
    }
    let world = app.world_mut();
    let lights = insert_light_buffer(world, scene.light.as_ref());
    // `--draw-soft-effects`: a list with a soft surface copies the colour
    // pass's opaque depth for it.
    let soft = scene.surfaces.iter().any(|s| s.params.soft_depth.is_some());
    if soft {
        soft_depth_buffer(world, Some([width, height]));
    }
    let sky = scene.light.as_ref().is_some_and(|l| l.sky);
    let mut targets = Vec::new();
    let passes: &[bool] = if want_tags { &[false, true] } else { &[false] };
    for (k, &tag_pass) in passes.iter().enumerate() {
        spawn(world, scene, &up, tag_pass, k, true);
        let mut target = Image::new_target_texture(width, height, TextureFormat::Rgba8Unorm, None);
        target.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let target = world.resource_mut::<Assets<Image>>().add(target);
        // The tag pass clears to all-ones bytes: u32::MAX, the CPU background
        // tag. On this linear (Rgba8Unorm) target a linear colour is stored
        // as is, like the CPU frame's clear value.
        let [r, g, b, a] = if tag_pass { [1.; 4] } else { clear };
        let clear = Color::LinearRgba(LinearRgba::new(r, g, b, a));
        // The sky on the colour pass's layer (never on the tag pass's).
        if sky && !tag_pass {
            spawn_sky(world, pose.0, k, lights.clone(), &up);
        }
        let mut c = world.spawn((
            camera_bundle(pose, camera.frustum, false, clear, k as isize, k),
            RenderTarget::Image(target.clone().into()),
            // Indirect drawing orders a batch's draws on the GPU, which
            // changes exact depth ties from run to run.
            bevy::render::view::NoIndirectDrawing,
        ));
        if soft && !tag_pass {
            c.insert(soft_depth_camera());
        }
        targets.push(target);
    }

    let captured: Arc<Mutex<Vec<Option<Image>>>> = Arc::new(Mutex::new(vec![None; targets.len()]));
    let mut ready = 0;
    let mut requested = false;
    let mut previous: Option<Vec<Vec<u8>>> = None;
    for _ in 0..MAX_FRAMES {
        app.update();
        app.world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("GpuPoll: {e}"))?;
        ready = if pipelines_ready(&app)? { ready + 1 } else { 0 };
        if ready < READY_FRAMES {
            continue;
        }
        if !requested {
            let world = app.world_mut();
            for (k, target) in targets.iter().enumerate() {
                let slot = captured.clone();
                world.spawn(Screenshot::image(target.clone())).observe(
                    move |shot: On<ScreenshotCaptured>| {
                        if let Ok(mut slots) = slot.lock() {
                            slots[k] = Some(shot.event().image.clone());
                        }
                    },
                );
            }
            requested = true;
            continue;
        }
        let images = {
            let mut slots = captured.lock().map_err(|_| "ScreenshotLock")?;
            if slots.iter().any(Option::is_none) {
                continue;
            }
            std::mem::replace(&mut *slots, vec![None; targets.len()])
        };
        let mut bytes = Vec::with_capacity(images.len());
        for image in images.into_iter().flatten() {
            if image.texture_descriptor.format != TextureFormat::Rgba8Unorm {
                return Err(format!(
                    "UnexpectedCaptureFormat: {:?}",
                    image.texture_descriptor.format
                ));
            }
            let data = image.data.ok_or("EmptyCapture")?;
            if data.len() != width as usize * height as usize * 4 {
                return Err(format!("CaptureSize: {} bytes", data.len()));
            }
            bytes.push(data);
        }
        requested = false;
        if previous.as_ref() == Some(&bytes) {
            let mut frames = bytes.into_iter();
            return Ok(Frames {
                color: frames.next().ok_or("MissingColorCapture")?,
                tags: frames.next(),
            });
        }
        previous = Some(bytes);
    }
    Err(format!(
        "HeadlessFrameNotStable: no two consecutive identical frames within {MAX_FRAMES} updates"
    ))
}

#[derive(Resource)]
struct Fly {
    state: FlyState,
    settings: FlySettings,
    /// The pose of `--eye`/`--look`, restored by R.
    start: FlyState,
    /// Whether the window starts walking; R restores that mode too.
    start_walking: bool,
}
/// Title-bar readout of mode, speed and position, and when it was last
/// written to the window.
#[derive(Resource, Default)]
struct Readout {
    text: String,
    shown: String,
    since: f64,
}
/// Seconds between title-bar updates.
const READOUT_PERIOD: f64 = 0.2;
/// Walk mode: the walking world, one piece per loaded cell when the grid
/// moves, and, while walking, the walker.
#[derive(Resource)]
struct Walk {
    space: fo4_walk::WorldSet<[i32; 2]>,
    active: Option<WalkView>,
}

/// The moving grid in the window: the channel to the loading thread, the
/// cell last sent as the centre and the cells received.
#[derive(Resource)]
pub struct Stream {
    centres: Sender<[i32; 2]>,
    messages: Mutex<Receiver<Message>>,
    sent: Option<[i32; 2]>,
    live: Live,
    /// Grid steps loaded around the player (`--around`), for a scripted
    /// run's check that the cells around it are drawn.
    load: u32,
}
impl Stream {
    pub fn new(centres: Sender<[i32; 2]>, messages: Receiver<Message>) -> Self {
        Self {
            centres,
            messages: Mutex::new(messages),
            sent: None,
            live: Live::default(),
            load: 0,
        }
    }
    /// The same, loading `n` grid steps around the player.
    pub fn loading(self, n: u32) -> Self {
        Self { load: n, ..self }
    }
}
/// The scene until it is spawned: its clear colour and what was uploaded.
#[derive(Resource)]
struct PendingScene(GpuScene, [f32; 4], Option<Uploaded>);
/// `--actor-clip`'s animation, waiting for the scene's meshes to upload.
#[derive(Resource)]
struct PendingClip(crate::clip::Animation);

/// `--time`: the game clock's seconds at the window's first frame.
#[derive(Resource)]
struct PendingClock(f64);

/// The window's game clock (`--time`): every animated surface's material,
/// its params as converted and its tracks; each frame the params at the
/// clock's time are written to the material. Instances the clock moves
/// (behind its transform switches): each frame their entity's transform is
/// set from the same chain as the CPU's.
#[derive(Resource)]
struct SurfaceClock {
    start: f64,
    started: Option<f64>,
    entries: Vec<(
        Handle<SurfaceMaterial>,
        crate::scene::SurfaceParams,
        crate::scene::Animated,
    )>,
    moving: Vec<(Entity, fo4_render::Motion)>,
}

/// Advances the game clock with the frame clock and sets the animated
/// surfaces' params at its time.
fn advance_clock(
    time: Res<Time>,
    clock: Option<ResMut<SurfaceClock>>,
    mut materials: ResMut<Assets<SurfaceMaterial>>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(mut c) = clock else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let started = *c.started.get_or_insert(now);
    let t = c.start + now - started;
    for (handle, base, a) in &c.entries {
        let p = match crate::scene::animated_params(*base, a, t) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        };
        if let Some(mut m) = materials.get_mut(handle) {
            m.params = uniform(&p);
        }
    }
    for (entity, motion) in &c.moving {
        let (translation, rotation, scale) = match crate::scene::moving_transform(motion, t) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        };
        if let Ok(mut tr) = transforms.get_mut(*entity) {
            *tr = Transform {
                translation,
                rotation,
                scale,
            };
        }
    }
}
#[derive(Component)]
struct FlyCamera;

/// The window's sky (the light switches' `--sky-weather`), kept about the
/// camera.
#[derive(Component)]
struct WindowSky;

/// Keeps the sky's cube about the camera.
fn follow_sky(
    cameras: Query<&Transform, With<FlyCamera>>,
    mut skies: Query<&mut Transform, (With<WindowSky>, Without<FlyCamera>)>,
) {
    let Some(c) = cameras.iter().next() else {
        return;
    };
    for mut t in &mut skies {
        t.translation = c.translation;
    }
}

/// The run's actors drawn by the scripted play mode's cast
/// (`--timeline-cast`): spawned apart from the cell's instances, which the
/// live world's decisions do not reach.
///
/// It carries its actor's reference and place and its spawned translation
/// ([`ride_cast`] with `--draw-cast-on-platforms`, [`watch_play`]).
#[cfg(feature = "script-event")]
#[derive(Component)]
struct CastEntity {
    place: Option<(fo4_plugin::FormKey, [f64; 3])>,
    base: Vec3,
    /// `--draw-cast-on-platforms`.
    ride: bool,
    /// `--draw-cast-keeps-its-part`.
    keep: bool,
    /// `--draw-cast-keeps-clear`.
    clear: bool,
    /// `--draw-cast-settles`, for an actor it may stand on the ground
    /// (`crate::scene::CastPlace::settle`).
    settle: bool,
    /// With it, its actor's drawn body (`crate::scene::cast_body`).
    body: std::sync::Arc<crate::scene::CastBody>,
    /// The space of the cell the cast was built for.
    space: Option<fo4_plugin::FormKey>,
    /// Its scale as spawned, and when: with `--draw-cast-settles` an actor
    /// whose ground the play thread has not answered yet is not drawn for
    /// up to [`CAST_SETTLE_WAIT`] (no frames at the stored height, then a
    /// drop).
    scale: Vec3,
    born: std::time::Instant,
}

/// Longest wall-clock seconds a new cast's actor waits undrawn for the play
/// thread's answer on its ground (`--draw-cast-settles`, viewer policy);
/// after it the actor is drawn at its place.
#[cfg(feature = "script-event")]
const CAST_SETTLE_WAIT: f64 = 1.5;

/// A cast built for the drawn cell, until it is uploaded and replaces the
/// one drawn.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PendingCast(GpuScene, Option<Uploaded>, std::time::Instant);

/// Uploads a pending cast a batch of textures per frame, then replaces the
/// drawn cast by it.
#[cfg(feature = "script-event")]
fn spawn_cast(world: &mut World) {
    // Not before the cell it stands in is spawned.
    if world.contains_resource::<PendingScene>()
        || world
            .get_resource::<Playing>()
            .is_some_and(|p| p.2.held.is_some())
    {
        return;
    }
    let Some(PendingCast(mut scene, up, asked)) = world.remove_resource::<PendingCast>() else {
        return;
    };
    let mut up = match up {
        Some(up) => up,
        None => match upload_meshes(world, &scene) {
            Ok(up) => up,
            Err(e) => {
                eprintln!("play: the cast's upload: {e}");
                return;
            }
        },
    };
    if !upload_textures(world, &mut scene, &mut up) {
        world.insert_resource(PendingCast(scene, Some(up), asked));
        return;
    }
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<CastEntity>>()
        .iter(world)
        .collect();
    let removed = old.len();
    for e in old {
        world.despawn(e);
    }
    let entities = spawn(world, &scene, &up, false, 0, false);
    let places: std::collections::BTreeMap<
        u32,
        (
            &crate::scene::CastPlace,
            std::sync::Arc<crate::scene::CastBody>,
        ),
    > = scene
        .cast_places
        .iter()
        .map(|c| (c.tag, (c, std::sync::Arc::new(c.body.clone()))))
        .collect();
    let born = std::time::Instant::now();
    for (e, i) in entities.iter().zip(&scene.instances) {
        let place = places.get(&i.tag);
        let scale = world
            .entity(*e)
            .get::<Transform>()
            .map_or(Vec3::ONE, |t| t.scale);
        world.entity_mut(*e).insert(CastEntity {
            scale,
            born,
            place: place.map(|(c, _)| (c.reference.clone(), c.place)),
            body: place.map(|(_, b)| b.clone()).unwrap_or_default(),
            base: i.translation,
            ride: scene.cast_ride,
            keep: scene.cast_keep,
            clear: scene.cast_clear,
            settle: place.is_some_and(|(c, _)| c.settle),
            space: scene.cast_space.clone(),
        });
    }
    println!(
        "play: cast drawn: {} instances in place of {removed}, uploaded in {:.2} s",
        entities.len(),
        asked.elapsed().as_secs_f64()
    );
}

/// Bevy pose of a free-fly state, through the same `look_at` as the CPU.
fn fly_pose(state: &FlyState) -> Result<(Vec3, Quat), String> {
    // Only the pose is used; the frame size and frustum do not affect it.
    let size = fo4_scene::camera::REFERENCE_SIZE;
    let frustum = fo4_scene::camera::reference_frustum(size);
    let camera = fo4_scene::camera::look_at(state.eye, state.look_point(), frustum, size)
        .map_err(|e| e.to_string())?;
    coords::camera_pose(&camera)
}

/// Uploads the scene a batch of textures per frame, then spawns it and the
/// camera.
fn spawn_window_scene(world: &mut World) {
    let Some(PendingScene(mut scene, clear, up)) = world.remove_resource::<PendingScene>() else {
        return;
    };
    let mut up = match up {
        Some(up) => up,
        None => match upload_meshes(world, &scene) {
            Ok(up) => up,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        },
    };
    if !upload_textures(world, &mut scene, &mut up) {
        world.insert_resource(PendingScene(scene, clear, Some(up)));
        return;
    }
    // The scene's light (the light switches of the scripted play mode; a
    // buffer without light otherwise, as before) and its sky, which
    // follows the camera ([`follow_sky`]).
    let lights = insert_light_buffer(world, scene.light.as_ref());
    // The scripted play window's exposure per view ([`expose_view`]).
    #[cfg(feature = "script-event")]
    let view_exposure = {
        world.remove_resource::<PlayExposure>();
        scene.view_exposure.take()
    };
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<WindowSky>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    if scene.light.as_ref().is_some_and(|l| l.sky) {
        let eye = fly_pose(&world.resource::<Fly>().state).map_or(Vec3::ZERO, |p| p.0);
        let e = spawn_sky(world, eye, 0, lights, &up);
        world.entity_mut(e).insert(WindowSky);
    }
    // The scripted play window's cell before, drawn until now.
    #[cfg(feature = "script-event")]
    {
        let old = world
            .get_resource_mut::<Playing>()
            .map(|mut p| std::mem::take(&mut p.2.old))
            .unwrap_or_default();
        for e in old {
            if let Ok(e) = world.get_entity_mut(e) {
                e.despawn();
            }
        }
    }
    let entities = spawn(world, &scene, &up, false, 0, false);
    // The exposure per view measures on what is drawn: each draw's
    // instance and the transform it was spawned with.
    #[cfg(feature = "script-event")]
    if let Some(v) = view_exposure {
        let n = scene.draws.iter().max().map_or(0, |d| d + 1);
        let mut by_draw = vec![None; n];
        for ((e, i), d) in entities.iter().zip(&scene.instances).zip(&scene.draws) {
            by_draw[*d] = Some((
                *e,
                Transform {
                    translation: i.translation,
                    rotation: i.rotation,
                    scale: i.scale,
                },
            ));
        }
        world.insert_resource(PlayExposure {
            view: v,
            by_draw,
            draw_of: scene.draws.clone(),
            checked: None,
        });
    }
    // The scripted play window's named platforms: their instances, to
    // follow the walker's parts ([`follow_platforms`]).
    #[cfg(feature = "script-event")]
    {
        world.remove_resource::<PlayPlatforms>();
        let drawn_platforms = scene.platforms.clone();
        let drawn: Vec<(Entity, crate::scene::PlatformInstance)> =
            std::mem::take(&mut scene.platforms)
                .into_iter()
                .filter_map(|p| entities.get(p.instance).map(|e| (*e, p)))
                .collect();
        if !drawn.is_empty() {
            world.insert_resource(PlayPlatforms {
                drawn,
                reported: false,
            });
        }
        world.remove_resource::<PlaySkins>();
        let skins: Vec<SkinTarget> = std::mem::take(&mut scene.skins)
            .into_iter()
            .filter_map(|k| {
                let platform = drawn_platforms
                    .iter()
                    .find(|p| p.instance == k.instance)?
                    .clone();
                let g = scene.meshes.get(k.mesh)?;
                Some(SkinTarget {
                    platform,
                    handle: up.meshes.get(k.mesh)?.clone(),
                    uvs: g.uvs.clone(),
                    colors: g.colors.clone(),
                    normals: g.normals.clone(),
                    indices: g.indices.clone(),
                    skin: k,
                    last: None,
                    faces: None,
                })
            })
            .collect();
        if !skins.is_empty() {
            println!(
                "play: {} skinned platform draws kept to follow their bones' parts",
                skins.len()
            );
            world.insert_resource(PlaySkins {
                drawn: skins,
                reported: false,
                positions: Vec::new(),
            });
        }
        world.remove_resource::<PlayLeaves>();
        let leaves: Vec<(Entity, crate::scene::LeafInstance)> = std::mem::take(&mut scene.leaves)
            .into_iter()
            .filter_map(|l| entities.get(l.instance).map(|e| (*e, l)))
            .collect();
        if !leaves.is_empty() {
            world.insert_resource(PlayLeaves {
                drawn: leaves,
                shown: std::collections::BTreeMap::new(),
            });
        }
        world.remove_resource::<PlayDoors>();
        let doors: Vec<(Entity, crate::scene::DoorInstance)> = std::mem::take(&mut scene.doors)
            .into_iter()
            .filter_map(|d| entities.get(d.instance).map(|e| (*e, d)))
            .collect();
        if !doors.is_empty() {
            world.insert_resource(PlayDoors {
                drawn: doors,
                shown: std::collections::BTreeMap::new(),
            });
        }
    }
    if let Some(PendingClock(start)) = world.remove_resource::<PendingClock>() {
        // Each animated surface's material, through its first instance.
        let mut entries = Vec::new();
        for a in std::mem::take(&mut scene.animated) {
            let Some(k) = scene.instances.iter().position(|i| i.surface == a.surface) else {
                continue;
            };
            let Some(handle) = entities
                .get(k)
                .and_then(|e| world.get::<MeshMaterial3d<SurfaceMaterial>>(*e))
                .map(|m| m.0.clone())
            else {
                continue;
            };
            entries.push((handle, scene.surfaces[a.surface].params, a));
        }
        let moving = std::mem::take(&mut scene.moving)
            .into_iter()
            .filter_map(|m| entities.get(m.instance).map(|e| (*e, m.motion)))
            .collect();
        world.insert_resource(SurfaceClock {
            start,
            started: None,
            entries,
            moving,
        });
    }
    if let Some(PendingClip(animation)) = world.remove_resource::<PendingClip>() {
        let playing = crate::clip::Playing::new(animation, &scene, &up.meshes);
        world.insert_resource(playing);
    }
    // The window's swap chain is expected to be sRGB (the shader checks the
    // real format; the clear colour cannot): an sRGB colour with the CPU's
    // clear values is stored as those values.
    let clear = Color::Srgba(Srgba::new(clear[0], clear[1], clear[2], clear[3]));
    // A scene loaded later (the scripted play mode's change of cell) keeps
    // the camera there is.
    if world
        .query_filtered::<(), With<FlyCamera>>()
        .iter(world)
        .next()
        .is_some()
    {
        return;
    }
    let fly = world.resource::<Fly>();
    let Ok(pose) = fly_pose(&fly.state) else {
        return;
    };
    // The window's first resize sets the vertical slopes from its aspect.
    let frustum = fo4_scene::camera::reference_frustum(fo4_scene::camera::REFERENCE_SIZE);
    let soft = world.contains_resource::<SoftEffectsOn>();
    let mut c = world.spawn((camera_bundle(pose, frustum, true, clear, 0, 0), FlyCamera));
    if soft {
        c.insert(soft_depth_camera());
    }
}

/// Mouse motion used for looking this frame: the raw device motion when
/// there is any, else the motion of the cursor in the window. Synthetic
/// input (for example scripted drags on macOS) can move the cursor without
/// producing raw device motion; both are never added.
pub fn look_delta(raw: Vec2, cursor: Vec2) -> [f64; 2] {
    let d = if raw != Vec2::ZERO { raw } else { cursor };
    [f64::from(d.x), f64::from(d.y)]
}

/// Title-bar text: mode, speed and position.
fn readout(fly: &Fly, walk: Option<&Walk>, input: FlyInput) -> String {
    let p = |v: [f64; 3]| format!("{:.1}, {:.1}, {:.1}", v[0], v[1], v[2]);
    match walk.and_then(|w| w.active.as_ref().map(|v| (w, v))) {
        Some((w, v)) => {
            let c = &v.character;
            let gait = if input.fast { "run" } else { "walk" };
            let set = if input.fast {
                c.params.run_speed
            } else {
                c.params.walk_speed
            };
            let mut t = format!(
                "fo4-view | walk | {gait} {set:.0} u/s, moving {:.0} u/s | feet {} | {}",
                c.velocity[0].hypot(c.velocity[1]),
                p(c.feet),
                if c.grounded { "on ground" } else { "in air" },
            );
            if v.recoveries > 0 {
                t.push_str(&format!(" | put back {}x", v.recoveries));
            }
            if fo4_walk::Space::bounds(&w.space).is_none() {
                t.push_str(" | no collision");
            }
            t
        }
        None => {
            let speed = if input.fast {
                fly.settings.fast_speed
            } else {
                fly.settings.speed
            };
            format!(
                "fo4-view | fly | {speed:.0} u/s{} | eye {}",
                if input.fast { " (Shift)" } else { "" },
                p(fly.state.eye)
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fly_camera(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut cursor: MessageReader<CursorMoved>,
    time: Res<Time>,
    mut fly: ResMut<Fly>,
    walk: Option<ResMut<Walk>>,
    stream: Option<Res<Stream>>,
    mut readout_text: ResMut<Readout>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
    // The cell the walker last waited for, and whether its landing after
    // the wait is still to be logged.
    mut waited: Local<(Option<[i32; 2]>, bool)>,
    mut script: Option<ResMut<Script>>,
) {
    let cursor_delta: Vec2 = cursor.read().filter_map(|m| m.delta).sum();
    // A scripted run steers the walker; the keys and mouse do nothing.
    let keys_on = script.is_none();
    let input = FlyInput::from_keys(
        [
            keys.pressed(KeyCode::KeyW),
            keys.pressed(KeyCode::KeyS),
            keys.pressed(KeyCode::KeyA),
            keys.pressed(KeyCode::KeyD),
        ],
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        buttons
            .pressed(MouseButton::Left)
            .then(|| look_delta(motion.delta, cursor_delta)),
    );
    let input = if keys_on { input } else { FlyInput::default() };
    let dt = time.delta_secs_f64();
    let restart = match script.as_deref_mut() {
        None => keys.just_pressed(KeyCode::KeyR),
        Some(s) => std::mem::take(&mut s.restart),
    };
    if restart {
        fly.state = fly.start;
    }
    let mut walk = walk;
    // With the moving grid, the cell under the walker that has not
    // arrived yet (the walker waits).
    let mut waiting = None;
    let next = match walk.as_deref_mut() {
        Some(walk) => {
            if restart {
                walk.active = if fly.start_walking {
                    WalkView::from_fly(
                        fly.start,
                        fo4_walk::Params::PROVISIONAL,
                        fo4_walk::Settings::DEFAULT,
                    )
                    .ok()
                } else {
                    None
                };
            } else if keys_on && keys.just_pressed(KeyCode::Tab) {
                walk.active = match walk.active.take() {
                    Some(_) => None,
                    None => WalkView::from_fly(
                        fly.state,
                        fo4_walk::Params::PROVISIONAL,
                        fo4_walk::Settings::DEFAULT,
                    )
                    .ok(),
                };
            }
            match walk.active.as_mut() {
                Some(view) => {
                    let feet = view.character.feet;
                    waiting = stream.as_ref().and_then(|s| {
                        crate::stream::cell_at(feet[0], feet[1]).filter(|g| !s.live.arrived(*g))
                    });
                    let steering = script.as_deref_mut().filter(|s| s.phase == Phase::Walking);
                    if waiting.is_some() {
                        view.hold(input, fly.settings, dt);
                    } else if let Some(s) = steering {
                        s.walk(view, &walk.space, dt);
                    } else {
                        view.step(&walk.space, input, fly.settings, dt);
                    }
                    view.look
                }
                None => fly.state.step(input, fly.settings, dt),
            }
        }
        None => fly.state.step(input, fly.settings, dt),
    };
    let Ok(pose) = fly_pose(&next) else {
        return;
    };
    fly.state = next;
    for mut transform in &mut cameras {
        transform.translation = pose.0;
        transform.rotation = pose.1;
    }
    if stream.is_some() {
        log_wait(&mut waited, waiting, walk.as_deref());
    }
    let mut text = readout(&fly, walk.as_deref(), input);
    if let Some(s) = &stream {
        text.push_str(&format!(
            " | cells {} ({} uploading)",
            s.live.cells.len(),
            s.live.waiting.len()
        ));
    }
    if let Some(g) = waiting {
        text.push_str(&format!(" | waiting for cell {},{}", g[0], g[1]));
    }
    readout_text.text = text;
    readout_text.since += dt;
}

/// Logs when the walker starts and stops waiting for a cell, and where it
/// first stands after a wait.
fn log_wait(state: &mut (Option<[i32; 2]>, bool), waiting: Option<[i32; 2]>, walk: Option<&Walk>) {
    let Some(c) = walk.and_then(|w| w.active.as_ref()).map(|v| &v.character) else {
        return;
    };
    let f = c.feet;
    match (state.0, waiting) {
        (None, Some(g)) => println!(
            "stream: walker waits for cell {},{} at feet {:.1},{:.1},{:.1}",
            g[0], g[1], f[0], f[1], f[2]
        ),
        (Some(g), None) => {
            println!("stream: cell {},{} arrived; walker moves", g[0], g[1]);
            state.1 = true;
        }
        _ => {}
    }
    state.0 = waiting;
    if state.1 && c.grounded {
        println!(
            "stream: walker stands at feet {:.1},{:.1},{:.1}",
            f[0], f[1], f[2]
        );
        state.1 = false;
    }
}

/// Writes the readout to the window title when it changed, at most every
/// [`READOUT_PERIOD`] seconds.
fn show_readout(
    mut readout: ResMut<Readout>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if readout.text == readout.shown || readout.since < READOUT_PERIOD {
        return;
    }
    for mut window in &mut windows {
        window.title.clone_from(&readout.text);
    }
    readout.shown = readout.text.clone();
    readout.since = 0.0;
}

/// Windowed viewer, free-fly or, with a walking world, walking (Tab
/// switches); returns when the window is closed.
#[allow(clippy::too_many_arguments)]
pub fn window(
    scene: GpuScene,
    start: FlyState,
    settings: FlySettings,
    walk: Option<fo4_walk::World>,
    clear: [f32; 4],
    aids: Aids,
    clip: Option<crate::clip::Animation>,
    time: Option<f64>,
    play: Option<crate::play::PlayRun>,
) -> Result<(), String> {
    fly_pose(&start)?;
    let start_walking = walk.is_some();
    let walk = match walk {
        Some(world) => {
            let view = WalkView::from_fly(
                start,
                fo4_walk::Params::PROVISIONAL,
                fo4_walk::Settings::DEFAULT,
            )?;
            // The world loaded at start, as the one piece of a set.
            let mut space = fo4_walk::WorldSet::new();
            space.insert([0, 0], world).map_err(|e| e.to_string())?;
            Some(Walk {
                space,
                active: Some(view),
            })
        }
        None => None,
    };
    let mut app = App::new();
    if let Some(walk) = walk {
        app.insert_resource(walk);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "fo4-view".into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(ViewerPlugin)
    .insert_resource(Fly {
        state: start,
        settings,
        start,
        start_walking,
    })
    .init_resource::<Readout>()
    .insert_resource(PendingScene(scene, clear, None))
    .add_systems(
        Update,
        (spawn_window_scene, fly_camera, show_readout).chain(),
    )
    .add_systems(Update, follow_sky.after(fly_camera));
    if let Some(animation) = clip {
        app.insert_resource(PendingClip(animation))
            .add_systems(Update, crate::clip::animate.after(spawn_window_scene));
    }
    if let Some(t) = time {
        app.insert_resource(PendingClock(t))
            .add_systems(Update, advance_clock.after(spawn_window_scene));
    }
    #[cfg(feature = "script-event")]
    if let Some(mut p) = play {
        let scripted = p.scripted();
        // --play-keys: the person's keys and mouse, read after the free-fly
        // system and turned into commands by the scripted window's system.
        if p.keys() {
            let person = crate::person::Person::new(
                start.yaw.to_degrees(),
                start.pitch.to_degrees(),
                settings.radians_per_pixel.to_degrees(),
            );
            let synthetic = p.synthetic.take();
            let with_synthetic = synthetic.is_some();
            app.insert_resource(PersonInput {
                person,
                synthetic: synthetic
                    .map(|(events, speed)| (crate::person::Synthetic::new(events), speed)),
                last_dt: 0.0,
                synthetic_done: false,
                drag: crate::person::Drag::default(),
                shift_modifier: false,
            })
            .add_systems(Update, person_keys.after(fly_camera).before(play_scripted));
            // The pad read with the keys: a scripted pad, or the device
            // (`--play-pad`), whose reader starts its own thread here.
            if let Some(w) = p.pad.take() {
                app.insert_resource(PadInput {
                    source: crate::pad::open(w)?,
                    state: crate::person::PadState::default(),
                    status: crate::pad::Status::default(),
                    focused: None,
                });
                println!(
                    "play: pad: left stick walks (runs near full tilt), right stick looks, A activates, Menu pauses, View or B held {} s ends (viewer policy, not measured game behaviour)",
                    crate::person::PAD_END_HOLD_SECONDS
                );
            }
            if with_synthetic {
                app.add_systems(PreUpdate, person_inject.before(bevy::input::InputSystems));
            }
        }
        // --play-hud (on with --play-keys): the viewport's crosshair and
        // lines, drawn by a camera of their own after the scene.
        if p.hud && scripted {
            app.add_systems(Startup, hud_spawn)
                .add_systems(Update, hud_update.after(play_scripted));
        }
        app.insert_resource(Playing(p, String::new(), PlayDrawn::default()));
        if let Some(w) = PlayWatch::from_env() {
            println!("play: watching the cast and the held doors (FO4_VIEW_PLAY_WATCH)");
            app.insert_resource(w);
        }
        if scripted {
            app.add_systems(Update, play_scripted.after(fly_camera).before(show_readout))
                .add_systems(Update, spawn_cast.after(play_scripted));
        } else {
            app.add_systems(Update, play_frame.after(fly_camera).before(show_readout));
        }
    }
    #[cfg(not(feature = "script-event"))]
    if let Some(p) = play {
        match p {}
    }
    aids.add_to(&mut app);
    app.run();
    Ok(())
}

/// The play mode's link to the run, its last title text and what the
/// live world last did to the drawn instances.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct Playing(crate::play::PlayRun, String, PlayDrawn);

/// The render layer of the HUD's camera and quads (`--play-hud`): the
/// scene's cameras draw layer 0 or their own.
#[cfg(feature = "script-event")]
const HUD_LAYER: usize = 31;

/// One quad of the HUD (`crate::hud`): its slot, what it shows and the
/// assets it draws with.
#[cfg(feature = "script-event")]
#[derive(Component)]
struct HudQuad {
    slot: crate::hud::Slot,
    shown: Option<(Option<String>, [u32; 2])>,
    image: Handle<Image>,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// A quad `size` window pixels wide and high, centred, facing +Z.
#[cfg(feature = "script-event")]
fn hud_mesh(size: [f32; 2]) -> Mesh {
    let [w, h] = [size[0] / 2.0, size[1] / 2.0];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-w, -h, 0.0], [w, -h, 0.0], [w, h, 0.0], [-w, h, 0.0]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    )
    .with_inserted_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3]))
}

/// An sRGB image of a HUD raster, sampled by its nearest texel.
#[cfg(feature = "script-event")]
fn hud_image(r: &crate::hud::Raster) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: r.size[0],
            height: r.size[1],
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(r.rgba.clone());
    image.sampler = ImageSampler::nearest();
    image
}

/// `--play-hud`, at start: the HUD's camera (an orthographic one whose
/// units are window pixels, drawing after the scene with no clear) and one
/// hidden quad per slot.
#[cfg(feature = "script-event")]
fn hud_spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection::default_3d()),
        Transform::IDENTITY,
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::layer(HUD_LAYER),
    ));
    for slot in crate::hud::Slot::ALL {
        let image = images.add(hud_image(&crate::hud::Raster {
            size: [1, 1],
            rgba: vec![0; 4],
        }));
        let mesh = meshes.add(hud_mesh([1.0, 1.0]));
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(image.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(0.0, 0.0, -10.0),
            Visibility::Hidden,
            RenderLayers::layer(HUD_LAYER),
            HudQuad {
                slot,
                shown: None,
                image,
                mesh,
                material,
            },
        ));
    }
}

/// `--play-hud`, each frame: what the HUD shows (what E would activate
/// when it is a thing E uses, the line being spoken, paused, loading),
/// each slot's image made again only when its text or the window's size
/// changed.
#[cfg(feature = "script-event")]
#[allow(clippy::too_many_arguments)]
fn hud_update(
    playing: Res<Playing>,
    person: Option<Res<PersonInput>>,
    mut confirmed: Local<(u64, Option<std::time::Instant>)>,
    pending: Option<Res<PendingScene>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut quads: Query<(&mut HudQuad, &mut Transform, &mut Visibility)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let size = [window.width(), window.height()];
    let Playing(p, _, drawn) = &*playing;
    let seen = p.seen().unwrap_or_default();
    // An activation's confirmation shows for a moment once.
    if let Some((n, _)) = &seen.confirm {
        if *n != confirmed.0 {
            *confirmed = (*n, Some(std::time::Instant::now()));
        }
    }
    let confirm = confirmed
        .1
        .filter(|at| at.elapsed().as_secs_f64() < HUD_CONFIRM_SECONDS)
        .and_then(|_| seen.confirm.as_ref().map(|c| c.1.clone()));
    // The play clock standing while nothing is paused: the run is busy
    // (a walking world built, a load) and the viewport says so.
    let stood = drawn
        .clock
        .1
        .is_some_and(|at| at.elapsed().as_secs_f64() > HUD_STOOD_SECONDS);
    let texts = p.spoken_texts();
    let shown = crate::hud::Shown {
        prompt: seen.prompt.clone().filter(|_| seen.aim_usable),
        confirm,
        subtitle: (!texts.is_empty()).then(|| texts.join(" / ")),
        // The objective, and where to go when the run has waited a while
        // on a place the walker passed (`crate::play::cue_line`).
        // The cue's volume key (after a tab) is for the watch log only.
        objective: match (
            p.objective(),
            seen.cue
                .as_deref()
                .map(|c| c.split('\t').next().unwrap_or(c)),
        ) {
            (Some(o), Some(c)) => Some(format!("{o} - {c}")),
            (o, c) => o.or(c).map(str::to_string),
        },
        paused: person.as_ref().is_some_and(|x| x.person.paused),
        loading: p.loading()
            || drawn.uploading.is_some()
            || drawn.held.is_some()
            || pending.is_some()
            || seen.building
            || stood,
    };
    let k = crate::hud::scale(size[1]);
    let columns = crate::hud::columns(size[0], k);
    let key = [size[0] as u32, size[1] as u32];
    for (mut q, mut t, mut v) in &mut quads {
        let lines = shown.lines(q.slot, columns);
        let now = Some((lines.as_ref().map(|l| l.join("\n")), key));
        if q.shown == now {
            continue;
        }
        q.shown = now;
        let raster = match (&lines, q.slot) {
            (None, _) => None,
            (Some(_), crate::hud::Slot::Crosshair) => Some(crate::hud::crosshair(5)),
            (Some(l), crate::hud::Slot::Objective) => crate::hud::lines(l, true),
            (Some(l), _) => crate::hud::lines(l, false),
        };
        let Some(r) = raster else {
            *v = Visibility::Hidden;
            continue;
        };
        let at = crate::hud::place_sized(q.slot, size, k, r.size);
        let _ = images.insert(&q.image, hud_image(&r));
        // The material is prepared again with the new image (its bind
        // group holds the image it was prepared with).
        if let Some(mut m) = materials.get_mut(&q.material) {
            m.base_color_texture = Some(q.image.clone());
        }
        let _ = meshes.insert(
            &q.mesh,
            hud_mesh([r.size[0] as f32 * k, r.size[1] as f32 * k]),
        );
        t.translation = Vec3::new(at[0], at[1], -10.0);
        *v = Visibility::Visible;
    }
}

/// The live world's generation last applied to the window
/// (`--play-live-world`), and each drawn reference instance's translation
/// as spawned.
#[cfg(feature = "script-event")]
#[derive(Default)]
struct PlayDrawn {
    /// `--draw-cast-on-platforms`: each actor's place, the height its
    /// feet were last drawn at and, with `--draw-cast-keeps-its-part`, the
    /// part it stood on ([`ride_cast`]).
    feet: std::collections::BTreeMap<
        fo4_plugin::FormKey,
        ([f64; 3], f64, Option<crate::scene::PartKey>),
    >,
    applied: Option<u64>,
    /// `--draw-cast-settles`: the census last printed (actors drawn on the
    /// ground, most units dropped, most raised; [`ride_cast`]).
    settled: (usize, i64, i64, [usize; 4]),
    /// `--draw-cast-keeps-clear`: the actors drawn moved clear of the
    /// walker the frame before, each with its offset, and the run's second
    /// of that frame ([`clear_cast`]).
    cleared: (
        std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 2]>,
        Option<f64>,
    ),
    spawned: std::collections::HashMap<Entity, Vec3>,
    /// The scripted run: frames to wait before a loaded cell's upload is
    /// read (its scene resource arrives through the commands), with the
    /// cell's load.
    uploading: Option<(u32, crate::play::Loaded)>,
    /// The scripted run's frame captures (`--play-shots`).
    shots: PlayShots,
    /// The play clock at the last frame and, while it stands with nothing
    /// paused, since when (the HUD's loading line).
    clock: (f64, Option<std::time::Instant>),
    /// The drawn cell's entities (and its cast's, when it leaves with
    /// it) a loaded cell replaces: drawn until the new cell's are spawned
    /// ([`spawn_window_scene`]), so the old view stays meanwhile.
    old: Vec<Entity>,
    /// A loaded cell of another space held back until its cast is built
    /// or [`CAST_REVEAL_WAIT`] passes (viewer policy): its scene, its
    /// load, since when, and whether the cast came.
    held: Option<(PendingScene, crate::play::Loaded, std::time::Instant, bool)>,
}

/// Wall seconds a loaded cell of another space waits for its cast before
/// it is drawn without it (viewer policy: geometry and cast revealed
/// together when the cast is that close).
#[cfg(feature = "script-event")]
const CAST_REVEAL_WAIT: f64 = 1.5;

/// Seconds an activation's confirmation stays in the prompt's place, and
/// the wall seconds the play clock may stand before the viewport says
/// loading (viewer policy).
#[cfg(feature = "script-event")]
const HUD_CONFIRM_SECONDS: f64 = 1.5;
#[cfg(feature = "script-event")]
const HUD_STOOD_SECONDS: f64 = 0.5;

/// `--play-shots`: the next multiple of the shot period, and the frame
/// asked for (at which second, where the walker stood) until it arrives.
#[cfg(feature = "script-event")]
#[derive(Default)]
struct PlayShots {
    next: f64,
    asked: Option<(f64, crate::play::Seen)>,
    captured: Arc<Mutex<Option<Image>>>,
    written: u32,
    /// The frame at the run's end was asked for.
    last: bool,
    /// The frames being encoded and written on their own threads.
    writing: Vec<std::thread::JoinHandle<()>>,
}

/// With `--play-live-world`, when the live world changed (and once the
/// scene is spawned): the walking world mirrors it, and each drawn instance
/// of a reference it knows is hidden where the reference is absent and
/// moved by the reference's move where it was moved.
#[cfg(feature = "script-event")]
fn play_live<F: bevy::ecs::query::QueryFilter>(
    p: &crate::play::PlayRun,
    drawn: &mut PlayDrawn,
    walk: Option<&mut Walk>,
    instances: &mut Query<(Entity, &MeshTag, &mut Visibility, &mut Transform), F>,
) {
    let Some((live, _)) = &p.live else { return };
    if drawn.applied == Some(p.generation) || instances.is_empty() {
        return;
    }
    if let Some(w) = walk {
        if let Err(e) = live.mirror_into(&mut w.space) {
            eprintln!("play: the live world's collision: {e}");
        }
    }
    let (mut hidden, mut moved) = (0usize, 0usize);
    for (e, tag, mut vis, mut t) in instances.iter_mut() {
        let Some(d) = p.decision(tag.value) else {
            continue;
        };
        let base = *drawn.spawned.entry(e).or_insert(t.translation);
        match d {
            fo4_play::live::Decision::Absent => {
                *vis = Visibility::Hidden;
                hidden += 1;
            }
            fo4_play::live::Decision::Present(v) => {
                *vis = Visibility::Inherited;
                let b = crate::coords::game_to_bevy(v);
                t.translation = base + Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32);
                moved += usize::from(v != [0.0; 3]);
            }
        }
    }
    println!(
        "play: live world generation {} drawn: {hidden} instances hidden, {moved} moved",
        p.generation
    );
    drawn.applied = Some(p.generation);
}

/// Each frame of the scripted play window with `--draw-cast-on-platforms`
/// (`play_cast_stands_on_platform_v1`): each actor of the cast is drawn
/// with its feet on the named platforms' part under its place, from where
/// they were drawn the frame before ([`crate::scene::stand_on_platform_part`]),
/// else at its place; a change of place starts from the place again. With
/// `--draw-cast-keeps-its-part` (`play_cast_keeps_its_part_v1`) an actor
/// rides the part it stood on the frame before while that part is under it
/// ([`crate::scene::stand_on_platform_part`]).
#[cfg(feature = "script-event")]
fn ride_cast<F: bevy::ecs::query::QueryFilter>(
    seen: &crate::play::Seen,
    feet: &mut std::collections::BTreeMap<
        fo4_plugin::FormKey,
        ([f64; 3], f64, Option<crate::scene::PartKey>),
    >,
    census: &mut (usize, i64, i64, [usize; 4]),
    casts: &mut Query<(Entity, &CastEntity, &mut Transform), F>,
) {
    let reach = fo4_walk::Params::PROVISIONAL.height;
    // Each actor's height, and whether it waits undrawn for its ground.
    let mut now: std::collections::BTreeMap<fo4_plugin::FormKey, (f64, bool)> =
        std::collections::BTreeMap::new();
    // `--draw-cast-settles` (`play_cast_settles_on_ground_v1`): an actor
    // no platform's part holds stands on the ground the play thread found
    // under its place (`crate::scene::settle_z`).
    let mut settled: std::collections::BTreeMap<fo4_plugin::FormKey, f64> =
        std::collections::BTreeMap::new();
    // The others asked about, each by why it stays (see `why`; 4 on a
    // platform's part).
    let mut stays: Vec<u8> = Vec::new();
    for (_, c, mut t) in casts.iter_mut() {
        let Some((k, place)) = &c.place else {
            continue;
        };
        if !c.ride && !c.settle {
            continue;
        }
        let (z, waits) = *now.entry(k.clone()).or_insert_with(|| {
            let answered = seen.ground.get(k).is_some_and(|(q, _)| q == place);
            let ground = || {
                c.settle
                    .then(|| seen.ground.get(k))
                    .flatten()
                    .filter(|(q, _)| q == place)
                    .and_then(|(_, hit)| crate::scene::settle_z(*place, *hit))
            };
            // Why an actor asked about stays at its place: 1 not answered
            // yet, 2 nothing under it, 3 ground out of reach or too steep.
            let why = || match seen.ground.get(k).filter(|(q, _)| q == place) {
                None => 1,
                Some((_, None)) => 2,
                Some(_) => 3,
            };
            if !c.ride {
                let g = ground();
                if g.is_none() && c.settle {
                    stays.push(why());
                }
                let z = g.unwrap_or(place[2]);
                if c.settle {
                    settled.insert(k.clone(), z - place[2]);
                }
                return (z, c.settle && !answered);
            }
            let (was, kept) = match feet.get(k) {
                Some((p, z, part)) if p == place => (*z, part.clone()),
                _ => (place[2], None),
            };
            let stood = crate::scene::stand_on_platform_part(
                &seen.platforms,
                c.space.as_ref(),
                *place,
                was,
                reach,
                kept.as_ref().filter(|_| c.keep),
            );
            let z = stood.as_ref().map_or(place[2], |s| s.0);
            let part_now = stood.as_ref().map(|s| &s.1);
            let part_was = feet.get(k).and_then(|f| f.2.as_ref());
            if part_now != part_was {
                // Which part an actor stands on, and the parts under its
                // place then, each with its top (a report of the play
                // window's reading; nothing changes).
                let under = crate::scene::parts_under(&seen.platforms, c.space.as_ref(), *place);
                println!(
                    "play: cast {} part {} at {:.3} s of the run (feet {:.1}, from {:.1}, place {:.1}); under its place: {}",
                    fo4_scene::timeline::key_text(k),
                    part_now.map_or("none".to_string(), |(r, i)| format!(
                        "{}#{i}",
                        fo4_scene::timeline::key_text(r)
                    )),
                    seen.t,
                    z,
                    was,
                    place[2],
                    under
                );
            }
            let boarded = (z - place[2]).abs() > 1e-6 && (was - place[2]).abs() <= 1e-6;
            if boarded {
                println!(
                    "play: cast {} stands on a named platform's part {:.1} units from its place at {:.3} s of the run (play_cast_stands_on_platform_v1)",
                    fo4_scene::timeline::key_text(k),
                    z - place[2],
                    seen.t
                );
            }
            let on_part = stood.is_some();
            feet.insert(k.clone(), (*place, z, stood.map(|s| s.1)));
            // Not one a platform's part holds (the ride down).
            if !on_part && c.settle {
                if let Some(g) = ground() {
                    settled.insert(k.clone(), g - place[2]);
                    return (g, false);
                }
                stays.push(why());
                return (z, !answered);
            } else if c.settle {
                stays.push(4);
            }
            (z, false)
        });
        let dz = (z - place[2]) as f32;
        // Game +Z is Bevy +Y (`coords`).
        let want = c.base + Vec3::new(0.0, dz, 0.0);
        if t.translation.y != want.y {
            t.translation.y = want.y;
        }
        // Not drawn while its ground is not answered yet, for a while.
        let scale = if waits && c.born.elapsed().as_secs_f64() < CAST_SETTLE_WAIT {
            Vec3::ZERO
        } else {
            c.scale
        };
        if t.scale != scale {
            t.scale = scale;
        }
    }
    // The census, at each change: actors drawn off their place's height,
    // the most dropped and the most raised (whole units).
    let moved: Vec<f64> = settled
        .values()
        .copied()
        .filter(|d| d.abs() >= 0.5)
        .collect();
    let count = |w: u8| stays.iter().filter(|x| **x == w).count();
    let line = (
        moved.len(),
        moved.iter().fold(0.0f64, |m, d| m.max(-d)).round() as i64,
        moved.iter().fold(0.0f64, |m, d| m.max(*d)).round() as i64,
        [count(1), count(2), count(3), count(4)],
    );
    if !(settled.is_empty() && stays.is_empty()) && line != *census {
        // Which actor dropped most (a report: who the census's drop is).
        let most = settled
            .iter()
            .filter(|(_, d)| **d <= -0.5)
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map_or("-".to_string(), |(k, _)| fo4_scene::timeline::key_text(k));
        println!(
            "play: settled: {} actors, max drop {} ({most}), max rise {}; kept at their place: {} not answered yet, {} with nothing under them, {} with ground out of reach or too steep, {} on a platform's part; at {:.3} s of the run (play_cast_settles_on_ground_v1)",
            line.0, line.1, line.2, line.3[0], line.3[1], line.3[2], line.3[3], seen.t
        );
        *census = line;
    }
}

/// Each frame of the scripted play window with `--draw-cast-keeps-clear`
/// (`play_cast_kept_clear_of_walker_v1`): each actor of the cast is drawn
/// moved horizontally so that its drawn body at the eye's height
/// ([`crate::scene::cast_clear_circle`]) stays out of the eye's reach, eased
/// from the frame before ([`crate::scene::ease_cast_clear`]: carried round
/// the eye, never through it), over whatever height `ride_cast` gave it.
/// `cleared` holds the actors moved the frame before with their offsets and
/// that frame's second, for one log line each time an actor starts being
/// moved.
#[cfg(feature = "script-event")]
fn clear_cast<F: bevy::ecs::query::QueryFilter>(
    seen: &crate::play::Seen,
    cleared: &mut (
        std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 2]>,
        Option<f64>,
    ),
    casts: &mut Query<(Entity, &CastEntity, &mut Transform), F>,
) {
    let dt = cleared.1.map_or(0.0, |t| (seen.t - t).clamp(0.0, 0.25));
    let eye = seen
        .feet
        .map(|f| [f[0], f[1], f[2] + fo4_walk::Params::PROVISIONAL.eye_height]);
    let mut now: std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 2]> =
        std::collections::BTreeMap::new();
    for (_, c, mut t) in casts.iter_mut() {
        let (Some((k, place)), true) = (&c.place, c.clear) else {
            continue;
        };
        let o = match now.get(k) {
            Some(o) => *o,
            None => {
                let shown = cleared.0.get(k).copied().unwrap_or([0.0; 2]);
                // Its feet's height as `ride_cast` drew them (game +Z is
                // Bevy +Y).
                let dz = f64::from(t.translation.y - c.base.y);
                let circle =
                    eye.and_then(|e| crate::scene::cast_clear_circle(*place, dz, &c.body, e));
                let o = match eye {
                    Some(e) => crate::scene::ease_cast_clear(shown, circle, [e[0], e[1]], dt),
                    None => [0.0; 2],
                };
                let moved = o[0].hypot(o[1]);
                if moved >= 0.05 && !cleared.0.contains_key(k) {
                    // Its body at the eye's height: the circle's radius and
                    // how far its centre lies from the place.
                    let (r, off) = circle.map_or((0.0, 0.0), |(c, r)| {
                        (r, (c[0] - place[0]).hypot(c[1] - place[1]))
                    });
                    println!(
                        "play: cast {} kept clear of the walker: drawn {:.1} units from its place at {:.3} s of the run (its body at the eye's height {:.1} units round, {:.1} from its place; play_cast_kept_clear_of_walker_v1)",
                        fo4_scene::timeline::key_text(k),
                        moved,
                        seen.t,
                        r,
                        off
                    );
                }
                now.insert(k.clone(), o);
                o
            }
        };
        // Game +X is Bevy +X, game +Y is Bevy -Z (`coords`).
        let (x, z) = (c.base.x + o[0] as f32, c.base.z - o[1] as f32);
        if t.translation.x != x || t.translation.z != z {
            t.translation.x = x;
            t.translation.z = z;
        }
    }
    now.retain(|_, o| o[0].hypot(o[1]) >= 0.05);
    *cleared = (now, Some(seen.t));
}

/// `FO4_VIEW_PLAY_WATCH=FILE` (measurement only, off by default): the
/// scripted play window writes, at each change, where each actor of the
/// drawn cast is drawn and whether it is in the cast, and the state of each
/// door the walker's held doors' reading holds within
/// [`WATCH_DOOR_REACH`] of the walker, each line with the play clock and
/// the walker's feet (spec/play.md, Watching the actors at a shut door);
/// and every [`WATCH_SAMPLE_SECONDS`] of the play clock, and with each door
/// change, a sample: the walker's feet and where the [`WATCH_NEAR`] actors
/// of the drawn cast nearest it are drawn, moved or not.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlayWatch {
    out: std::io::BufWriter<std::fs::File>,
    actors: std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 3]>,
    doors: std::collections::BTreeMap<fo4_plugin::FormKey, Option<bool>>,
    feet: Option<[f64; 3]>,
    /// The play clock's second of the next sample.
    sample: f64,
    /// The prompt, the aim's label and the confirmation last written.
    prompt: Option<WatchedPrompt>,
    /// The viewport's where-to-go cue at the last change.
    cue: Option<Option<String>>,
}

/// What [`PlayWatch`] compares for its prompt lines: the prompt, the aim's
/// label and the last confirmation.
#[cfg(feature = "script-event")]
type WatchedPrompt = (Option<String>, Option<String>, Option<(u64, String)>);

/// Units from the walker within which [`PlayWatch`] writes the held doors.
#[cfg(feature = "script-event")]
const WATCH_DOOR_REACH: f64 = 2048.0;
/// An actor drawn this many units from where it was last written is
/// written again; the walker, when a line is written or it moved this far.
#[cfg(feature = "script-event")]
const WATCH_MOVE: f64 = 8.0;
/// Seconds of the play clock between two samples of [`PlayWatch`].
#[cfg(feature = "script-event")]
const WATCH_SAMPLE_SECONDS: f64 = 1.0;
/// How many of the drawn cast's actors nearest the walker a sample writes.
#[cfg(feature = "script-event")]
const WATCH_NEAR: usize = 8;

#[cfg(feature = "script-event")]
impl PlayWatch {
    /// The log the environment names, its header written; `None` without
    /// `FO4_VIEW_PLAY_WATCH`.
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("FO4_VIEW_PLAY_WATCH")?;
        match std::fs::File::create(&path) {
            Ok(f) => {
                let mut out = std::io::BufWriter::new(f);
                use std::io::Write as _;
                let _ = writeln!(
                    out,
                    "# fo4-view play watch (measurement only): KIND\tRUN_SECONDS\tWHAT\t...\n# walker\tT\tX\tY\tZ\n# actor\tT\tREF\tdrawn|gone\tX\tY\tZ\tDISTANCE_TO_WALKER\n# door\tT\tREF\tclosed|open|not-held\tX\tY\tZ\tDISTANCE_TO_WALKER (X Y Z: its first drawn shape's place, - when not drawn)\n# sample\tT\tX\tY\tZ (the walker's feet; every {WATCH_SAMPLE_SECONDS} s of the play clock and at each door line, moved or not)\n# near\tT\tREF\tX\tY\tZ\tDISTANCE_TO_WALKER (after a sample: the {WATCH_NEAR} actors of the drawn cast nearest the walker, moved or not)\n# prompt\tT\tPROMPT|-\tAIM|-\tCONFIRM|- (at each change: the E prompt as the viewport shows it, what the activation key meets, the last activation's confirmation)"
                );
                Some(PlayWatch {
                    out,
                    actors: std::collections::BTreeMap::new(),
                    doors: std::collections::BTreeMap::new(),
                    feet: None,
                    sample: 0.0,
                    prompt: None,
                    cue: None,
                })
            }
            Err(e) => {
                eprintln!("play: FO4_VIEW_PLAY_WATCH: {e}");
                None
            }
        }
    }
}

/// Each frame of the scripted play window with [`PlayWatch`]: the changes
/// since the last frame written.
#[cfg(feature = "script-event")]
fn watch_play<F: bevy::ecs::query::QueryFilter>(
    seen: &crate::play::Seen,
    watch: &mut PlayWatch,
    casts: &Query<(Entity, &CastEntity, &mut Transform), F>,
    doors: Option<&PlayDoors>,
) {
    use std::io::Write as _;
    let t = seen.t;
    let feet = seen.feet;
    let dist = |p: [f64; 3]| -> Option<f64> {
        feet.map(|f| (0..3).map(|i| (p[i] - f[i]).powi(2)).sum::<f64>().sqrt())
    };
    let far = |p: [f64; 3]| -> String { dist(p).map_or("-".to_string(), |d| format!("{d:.1}")) };
    let mut lines: Vec<String> = Vec::new();
    let mut door_lines = false;
    // Where each actor of the cast is drawn: its place, raised by what
    // its draws were moved by (`ride_cast`).
    let mut now: std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 3]> =
        std::collections::BTreeMap::new();
    for (_, c, tr) in casts.iter() {
        let Some((k, place)) = &c.place else {
            continue;
        };
        now.entry(k.clone()).or_insert_with(|| {
            let d = tr.translation - c.base;
            // Bevy (x, y, z) is game (x, -z, y) (`coords`).
            [
                place[0] + f64::from(d.x),
                place[1] - f64::from(d.z),
                place[2] + f64::from(d.y),
            ]
        });
    }
    for (k, p) in &now {
        let moved = watch.actors.get(k).is_none_or(|q| {
            (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f64>().sqrt() >= WATCH_MOVE
        });
        if moved {
            lines.push(format!(
                "actor\t{t:.3}\t{}\tdrawn\t{:.1}\t{:.1}\t{:.1}\t{}",
                fo4_scene::timeline::key_text(k),
                p[0],
                p[1],
                p[2],
                far(*p)
            ));
            watch.actors.insert(k.clone(), *p);
        }
    }
    let gone: Vec<fo4_plugin::FormKey> = watch
        .actors
        .keys()
        .filter(|k| !now.contains_key(*k))
        .cloned()
        .collect();
    for k in gone {
        if let Some(p) = watch.actors.remove(&k) {
            lines.push(format!(
                "actor\t{t:.3}\t{}\tgone\t{:.1}\t{:.1}\t{:.1}\t{}",
                fo4_scene::timeline::key_text(&k),
                p[0],
                p[1],
                p[2],
                far(p)
            ));
        }
    }
    // The held doors near the walker: closed while their box stands.
    let mut at: std::collections::BTreeMap<&fo4_plugin::FormKey, [f64; 3]> =
        std::collections::BTreeMap::new();
    for (_, d) in doors.map_or(&[][..], |d| &d.drawn[..]) {
        at.entry(&d.reference).or_insert(d.closed.translation);
    }
    for (k, held) in &seen.doors {
        let p = at.get(k).copied();
        if p.and_then(dist).is_some_and(|d| d > WATCH_DOOR_REACH) {
            continue;
        }
        let state = Some(*held);
        if watch.doors.get(k) == Some(&state) {
            continue;
        }
        watch.doors.insert(k.clone(), state);
        let (x, d) = match p {
            Some(p) => (format!("{:.1}\t{:.1}\t{:.1}", p[0], p[1], p[2]), far(p)),
            None => ("-\t-\t-".to_string(), "-".to_string()),
        };
        door_lines = true;
        lines.push(format!(
            "door\t{t:.3}\t{}\t{}\t{x}\t{d}",
            fo4_scene::timeline::key_text(k),
            if *held { "closed" } else { "open" }
        ));
    }
    let left: Vec<fo4_plugin::FormKey> = watch
        .doors
        .keys()
        .filter(|k| !seen.doors.contains_key(*k))
        .cloned()
        .collect();
    for k in left {
        watch.doors.remove(&k);
        door_lines = true;
        lines.push(format!(
            "door\t{t:.3}\t{}\tnot-held\t-\t-\t-\t-",
            fo4_scene::timeline::key_text(&k)
        ));
    }
    let walked = match (feet, watch.feet) {
        (Some(f), Some(w)) => {
            (0..3).map(|i| (f[i] - w[i]).powi(2)).sum::<f64>().sqrt() >= WATCH_MOVE
        }
        (Some(_), None) => true,
        _ => false,
    };
    if let (Some(f), true) = (feet, walked || !lines.is_empty()) {
        let _ = writeln!(
            watch.out,
            "walker\t{t:.3}\t{:.1}\t{:.1}\t{:.1}",
            f[0], f[1], f[2]
        );
        watch.feet = Some(f);
    }
    // The E prompt and what the key meets, at each change.
    let prompt = (
        seen.prompt.clone().filter(|_| seen.aim_usable),
        seen.aim.clone(),
        seen.confirm.clone(),
    );
    if watch.prompt.as_ref() != Some(&prompt) {
        lines.push(format!(
            "prompt\t{t:.3}\t{}\t{}\t{}",
            prompt.0.as_deref().unwrap_or("-"),
            prompt.1.as_deref().unwrap_or("-"),
            prompt.2.as_ref().map_or("-", |c| c.1.as_str())
        ));
        watch.prompt = Some(prompt);
    }
    if watch.cue.as_ref() != Some(&seen.cue) {
        lines.push(format!(
            "cue\t{t:.3}\t{}",
            seen.cue.as_deref().unwrap_or("-")
        ));
        watch.cue = Some(seen.cue.clone());
    }
    let mut wrote = !lines.is_empty() || walked;
    for l in lines {
        let _ = writeln!(watch.out, "{l}");
    }
    // A sample: the walker and the actors nearest it, moved or not, so a
    // reader tells one that stood from one not written.
    if let (Some(f), true) = (feet, t >= watch.sample || door_lines) {
        if t >= watch.sample {
            watch.sample = ((t / WATCH_SAMPLE_SECONDS).floor() + 1.0) * WATCH_SAMPLE_SECONDS;
        }
        let _ = writeln!(
            watch.out,
            "sample\t{t:.3}\t{:.1}\t{:.1}\t{:.1}",
            f[0], f[1], f[2]
        );
        let mut near: Vec<(f64, &fo4_plugin::FormKey, [f64; 3])> = now
            .iter()
            .filter_map(|(k, p)| dist(*p).map(|d| (d, k, *p)))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        for (d, k, p) in near.into_iter().take(WATCH_NEAR) {
            let _ = writeln!(
                watch.out,
                "near\t{t:.3}\t{}\t{:.1}\t{:.1}\t{:.1}\t{d:.1}",
                fo4_scene::timeline::key_text(k),
                p[0],
                p[1],
                p[2]
            );
        }
        wrote = true;
    }
    // The window may end by the process's exit: nothing waits in memory.
    if wrote {
        let _ = watch.out.flush();
    }
}

/// The drawn instances of the named platforms' models in the scripted play
/// window (`--play-input` with `--play-elevator`), and whether what they
/// follow was reported.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlayPlatforms {
    drawn: Vec<(Entity, crate::scene::PlatformInstance)>,
    reported: bool,
}

/// The drawn instances of the held doors' models in the scripted play
/// window (`--draw-held-doors`), and the state each door was last drawn in.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlayDoors {
    drawn: Vec<(Entity, crate::scene::DoorInstance)>,
    shown: std::collections::BTreeMap<fo4_plugin::FormKey, Option<bool>>,
}

/// What the scripted play window's drawn doors, leaves and skins read and
/// write, for [`play_scripted`].
#[cfg(feature = "script-event")]
type FollowParams<'w> = (
    Option<ResMut<'w, PlayDoors>>,
    Option<ResMut<'w, PlayLeaves>>,
    Option<ResMut<'w, PlaySkins>>,
    ResMut<'w, Assets<Mesh>>,
);

/// A named platform's skinned draw the scripted play window skins again
/// (`--draw-platform-skins`): its platform instance, its skin and the mesh
/// it draws with that mesh's other attributes, and the matrices it was
/// last skinned with.
#[cfg(feature = "script-event")]
struct SkinTarget {
    platform: crate::scene::PlatformInstance,
    skin: crate::scene::SkinInstance,
    handle: Handle<Mesh>,
    uvs: Vec<[f32; 2]>,
    colors: Option<Vec<[f32; 4]>>,
    normals: Option<Vec<[f32; 3]>>,
    indices: Vec<u16>,
    last: Option<Vec<fo4_scene::actor::pose::Affine>>,
    /// The faces as last skinned again, world (game) axes: what the
    /// exposure per view measures in place of the load's.
    faces: Option<std::sync::Arc<Vec<[[f32; 3]; 3]>>>,
}

/// The scripted play window's skinned platform draws
/// (`play_platform_skin_follows_parts_v1`), whether the first frame that
/// skinned them was reported, and the positions' buffer.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlaySkins {
    drawn: Vec<SkinTarget>,
    reported: bool,
    positions: Vec<[f64; 3]>,
}

/// Each frame of the scripted play window with `--draw-platform-skins`
/// (`play_platform_skin_follows_parts_v1`): a named platform's skinned draw
/// whose bones go with more than one part, or some with none
/// ([`crate::scene::platform_part`]), is skinned again with each bone's
/// matrix moved by its part's motion ([`crate::scene::skin_matrices`]) and
/// its mesh replaced, when the matrices changed. A draw whose bones all go
/// with one part follows it as an instance ([`follow_platforms`]).
#[cfg(feature = "script-event")]
fn follow_skins(seen: &crate::play::Seen, skins: &mut PlaySkins, meshes: &mut Assets<Mesh>) {
    if seen.platforms.is_empty() {
        return;
    }
    let PlaySkins {
        drawn,
        reported,
        positions,
    } = skins;
    let mut moving = 0usize;
    for t in drawn.iter_mut() {
        let d = &t.platform;
        let Some(p) = seen
            .platforms
            .iter()
            .find(|p| fo4_scene::timeline::norm(&p.reference) == d.reference && p.model == d.model)
        else {
            continue;
        };
        if !crate::scene::platform_part(p, d).1 {
            continue;
        }
        let Some(m) = crate::scene::skin_matrices(p, d, &t.skin.rest) else {
            continue;
        };
        moving += 1;
        if t.last.as_ref() == Some(&m) {
            continue;
        }
        t.skin.shape.positions(&m, positions);
        let mut mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            positions
                .iter()
                .map(|v| v.map(|x| x as f32))
                .collect::<Vec<[f32; 3]>>(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, t.uvs.clone())
        .with_inserted_indices(bevy::mesh::Indices::U16(t.indices.clone()));
        if let Some(c) = &t.colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, c.clone());
        }
        if let Some(n) = &t.normals {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n.clone());
        }
        if let Err(e) = meshes.insert(t.handle.id(), mesh) {
            eprintln!("PlatformSkinMesh: {e}");
            continue;
        }
        let world = d.world;
        let faces: Vec<[[f32; 3]; 3]> = t
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|tri| {
                let mut f = [[0f32; 3]; 3];
                for (k, i) in tri.iter().enumerate() {
                    f[k] = world
                        .apply(*positions.get(usize::from(*i))?)
                        .map(|x| x as f32);
                }
                Some(f)
            })
            .collect();
        t.faces = Some(std::sync::Arc::new(faces));
        t.last = Some(m);
    }
    if !*reported && moving > 0 {
        *reported = true;
        println!(
            "play: platform skins at {:.3} s of the run: {moving} skinned draws with bones under more than one part skinned again by their parts (play_platform_skin_follows_parts_v1)",
            seen.t
        );
    }
}

/// The drawn instances the static leaves turn in the scripted play window
/// (`--draw-static-leaves`), and the pose each static was last drawn in.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlayLeaves {
    drawn: Vec<(Entity, crate::scene::LeafInstance)>,
    shown: std::collections::BTreeMap<fo4_plugin::FormKey, crate::scene::LeafPose>,
}

/// Each frame of the scripted play window with `--draw-static-leaves`
/// (`play_static_leaf_two_pose_v1`): each static's leaf is drawn in the pose
/// the walker's collision marker boxes give it
/// ([`crate::scene::LeafInstance::pose`]); a static the run disabled is
/// hidden.
#[cfg(feature = "script-event")]
fn follow_leaves<F: bevy::ecs::query::QueryFilter>(
    p: &crate::play::PlayRun,
    seen: &crate::play::Seen,
    leaves: &mut PlayLeaves,
    instances: &mut Query<CellInstance, F>,
) {
    let boxes: Vec<([f64; 3], [f64; 3], bool)> = seen
        .boxes
        .values()
        .map(|m| (m.min, m.max, m.standing))
        .collect();
    let mut poses = std::collections::BTreeMap::new();
    for (e, l) in &leaves.drawn {
        let pose = *poses
            .entry(l.reference.clone())
            .or_insert_with(|| l.pose(&boxes));
        let Ok((_, tag, mut vis, mut t)) = instances.get_mut(*e) else {
            continue;
        };
        if p.decision(tag.value) == Some(fo4_play::live::Decision::Absent) {
            *vis = Visibility::Hidden;
            continue;
        }
        let w = if pose.keyed() { l.keyed } else { l.stored };
        match crate::coords::instance_transform(&w) {
            Some((translation, rotation, scale)) => {
                *vis = Visibility::Inherited;
                *t = Transform {
                    translation,
                    rotation,
                    scale,
                };
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (k, pose) in poses {
        if leaves.shown.get(&k) != Some(&pose) {
            leaves.shown.insert(k.clone(), pose);
            let what = match pose {
                crate::scene::LeafPose::Stored => "as stored (no box meets it)",
                crate::scene::LeafPose::Holding { keyed: false } => {
                    "as stored, holding the way (a box meeting it stands)"
                }
                crate::scene::LeafPose::Holding { keyed: true } => {
                    "at its keyed pose, holding the way (a box meeting it stands)"
                }
                crate::scene::LeafPose::Let { keyed: false } => {
                    "as stored, letting the way (no box meeting it stands)"
                }
                crate::scene::LeafPose::Let { keyed: true } => {
                    "at its keyed pose, letting the way (no box meeting it stands)"
                }
            };
            println!(
                "play: static leaf {} drawn {what} at {:.3} s of the run",
                fo4_scene::timeline::key_text(&k),
                seen.t
            );
        }
    }
}

/// Each frame of the scripted play window with `--draw-held-doors`
/// (`play_door_drawn_as_held_v1`): a door whose box the walker's held
/// doors' reading holds is drawn as stored (closed); one it held and let go
/// is drawn posed open, or hidden when no open pose was read; one it never
/// held, or the run disabled, is hidden.
#[cfg(feature = "script-event")]
fn follow_doors<F: bevy::ecs::query::QueryFilter>(
    p: &crate::play::PlayRun,
    seen: &crate::play::Seen,
    doors: &mut PlayDoors,
    instances: &mut Query<CellInstance, F>,
) {
    for (e, d) in &doors.drawn {
        let Ok((_, tag, mut vis, mut t)) = instances.get_mut(*e) else {
            continue;
        };
        let state = seen.doors.get(&d.reference).copied();
        let gone = p.decision(tag.value) == Some(fo4_play::live::Decision::Absent);
        let world = match (gone, state) {
            (false, Some(true)) => Some(d.closed),
            (false, Some(false)) => d.open,
            _ => None,
        };
        match world.and_then(|w| crate::coords::instance_transform(&w)) {
            Some((translation, rotation, scale)) => {
                *vis = Visibility::Inherited;
                *t = Transform {
                    translation,
                    rotation,
                    scale,
                };
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (k, held) in &seen.doors {
        let now = Some(*held);
        if doors.shown.get(k) != Some(&now) {
            doors.shown.insert(k.clone(), now);
            println!(
                "play: door {} drawn {} at {:.3} s of the run",
                fo4_scene::timeline::key_text(k),
                if *held {
                    "closed (its box stands)"
                } else {
                    "open (its box left)"
                },
                seen.t
            );
        }
    }
}

/// Each frame of the scripted play window: each drawn shape of a named
/// platform whose node chain goes with a moving part
/// (`fo4_play::elevator::Platform::part_of`) is drawn where the part's
/// collision stands: its world transform as built, moved by the part's
/// rigid motion (the walker's, `play_elevator_rigid_v1` or the translation
/// reading; nothing of the drawing's own). Shapes under no moving part
/// stay as built.
#[cfg(feature = "script-event")]
fn follow_platforms<F: bevy::ecs::query::QueryFilter>(
    seen: &crate::play::Seen,
    platforms: &mut PlayPlatforms,
    instances: &mut Query<CellInstance, F>,
    skinned_again: bool,
) {
    if seen.platforms.is_empty() {
        return;
    }
    let (mut moved, mut still, mut unknown, mut split) = (0usize, 0usize, 0usize, 0usize);
    for (e, d) in &platforms.drawn {
        let Some(p) = seen
            .platforms
            .iter()
            .find(|p| fo4_scene::timeline::norm(&p.reference) == d.reference && p.model == d.model)
        else {
            unknown += 1;
            continue;
        };
        let (part, mixed) = crate::scene::platform_part(p, d);
        split += usize::from(mixed);
        let Some(k) = part else {
            still += 1;
            if !platforms.reported && d.bones.is_empty() {
                // Measurement: why a rigid shape stays (its chain's nodes
                // a sequence sets, the parts' nodes).
                let set: Vec<usize> = d
                    .chain
                    .iter()
                    .copied()
                    .filter(|b| p.set.binary_search(b).is_ok())
                    .collect();
                let parts: Vec<String> = p
                    .parts
                    .iter()
                    .map(|q| match &q.members {
                        fo4_play::elevator::Members::Deepest(b) => b.to_string(),
                        fo4_play::elevator::Members::Nodes(n) => format!("{n:?}"),
                    })
                    .collect();
                println!(
                    "play: platforms: instance {} of {} stays: its chain's set nodes {set:?}, the parts' nodes [{}]",
                    d.instance,
                    fo4_scene::timeline::key_text(&d.reference),
                    parts.join(", ")
                );
            }
            continue;
        };
        let world = fo4_play::elevator::transform(&p.parts[k].motion).compose(d.world);
        let Some((translation, rotation, scale)) = crate::coords::instance_transform(&world) else {
            continue;
        };
        if let Ok((_, _, _, mut t)) = instances.get_mut(*e) {
            *t = Transform {
                translation,
                rotation,
                scale,
            };
            moved += 1;
        }
    }
    if !platforms.reported {
        platforms.reported = true;
        println!(
            "play: platforms at {:.3} s of the run: {moved} drawn instances follow their parts, {still} under no moving part, {unknown} of no named platform's model{}",
            seen.t,
            if split > 0 {
                format!(
                    ", {split} skinned with bones under more than one part ({})",
                    if skinned_again {
                        "skinned again by their parts, --draw-platform-skins"
                    } else {
                        "still"
                    }
                )
            } else {
                String::new()
            }
        );
    }
}

/// The scripted play window's exposure measured per view
/// (`crate::scene::ViewExposure`, with `--exposure-eye`), with each draw's
/// instance and spawned transform (`None`: the draw has no instance) and
/// when what is drawn was last read.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PlayExposure {
    view: crate::scene::ViewExposure,
    by_draw: Vec<Option<(Entity, Transform)>>,
    /// The draw of each spawned instance (`GpuScene::draws`).
    draw_of: Vec<usize>,
    checked: Option<std::time::Instant>,
}

/// What the window draws apart from its load ([`crate::scene::DrawnNow`]):
/// a draw whose instance is hidden or missing, or whose transform is not
/// the one it was spawned with (by the motion between them, game axes).
/// A named platform's skinned draw skinned again
/// ([`follow_skins`]) is measured by its faces as last skinned.
#[cfg(feature = "script-event")]
fn drawn_now<F: bevy::ecs::query::QueryFilter>(
    exposure: &PlayExposure,
    skins: Option<&PlaySkins>,
    instances: &Query<CellInstance, F>,
) -> crate::scene::DrawnNow {
    let by_draw = &exposure.by_draw;
    let mut now = crate::scene::DrawnNow::default();
    for t in skins.map_or(&[][..], |s| &s.drawn[..]) {
        if let (Some(f), Some(d)) = (&t.faces, exposure.draw_of.get(t.platform.instance)) {
            now.replaced
                .insert(u32::try_from(*d).unwrap_or(u32::MAX), f.clone());
        }
    }
    for (d, slot) in by_draw.iter().enumerate() {
        let d = u32::try_from(d).unwrap_or(u32::MAX);
        let Some((e, spawned)) = slot else {
            now.hidden.insert(d);
            continue;
        };
        let Ok((_, _, vis, t)) = instances.get(*e) else {
            now.hidden.insert(d);
            continue;
        };
        if *vis == Visibility::Hidden {
            now.hidden.insert(d);
        } else if *t != *spawned && !now.replaced.contains_key(&d) {
            now.moved.insert(d, game_motion(spawned, t));
        }
    }
    now
}

/// The motion taking what was drawn at `from` to `to` (Bevy transforms),
/// as three rows of a game-axes affine map.
#[cfg(feature = "script-event")]
fn game_motion(from: &Transform, to: &Transform) -> [[f64; 4]; 3] {
    let a = to.compute_affine() * from.compute_affine().inverse();
    let b = crate::coords::GAME_TO_BEVY;
    let l: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| f64::from(a.matrix3.col(c)[r])));
    let t = [
        f64::from(a.translation.x),
        f64::from(a.translation.y),
        f64::from(a.translation.z),
    ];
    // game = B^T bevy: rows B^T L B, translation B^T t.
    std::array::from_fn(|r| {
        let mut row = [0.0; 4];
        for (c, x) in row.iter_mut().take(3).enumerate() {
            *x = (0..3)
                .map(|i| (0..3).map(|j| b[i][r] * l[i][j] * b[j][c]).sum::<f64>())
                .sum();
        }
        row[3] = (0..3).map(|i| b[i][r] * t[i]).sum();
        row
    })
}

/// What [`expose_view`] reads and writes, for [`play_scripted`].
#[cfg(feature = "script-event")]
type ExposureParams<'w> = (
    Option<ResMut<'w, PlayExposure>>,
    Option<Res<'w, LightBuffer>>,
    ResMut<'w, Assets<ShaderBuffer>>,
);

/// With `--exposure-eye` in the scripted play window: when the walker's
/// view changed enough ([`crate::scene::ViewExposure::due`]) the exposure is
/// measured from it and the light buffer every material binds is replaced
/// by one with the new row 14 (a viewer policy; the rule is
/// `frame_exposure_seen_light_v1`, with `--exposure-eye-floor` its floor).
/// What the window draws is read again at most once a measure's period
/// ([`crate::scene::ViewExposure::PERIOD`]): a draw hidden, shown or moved
/// since the last measure makes the view due again, and the measure leaves
/// out what is hidden and takes what moved where it is drawn.
#[cfg(feature = "script-event")]
fn expose_view<F: bevy::ecs::query::QueryFilter>(
    seen: &crate::play::Seen,
    exposure: &mut PlayExposure,
    skins: Option<&PlaySkins>,
    instances: &Query<CellInstance, F>,
    lights: &LightBuffer,
    buffers: &mut Assets<ShaderBuffer>,
) {
    let Some((eye, look)) = crate::play::eye_look(seen) else {
        return;
    };
    let now = std::time::Instant::now();
    let period = std::time::Duration::from_secs_f64(crate::scene::ViewExposure::PERIOD);
    if exposure
        .checked
        .is_none_or(|at| now.duration_since(at) >= period)
    {
        exposure.checked = Some(now);
        let d = drawn_now(exposure, skins, instances);
        exposure.view.set_drawn(d);
    }
    let Some(line) = exposure.view.step(eye, look, now) else {
        return;
    };
    let d = &exposure.view.drawn;
    println!(
        "play: exposure at {:.3} s of the run: {line} (as drawn: {} draws hidden, {} moved, {} skinned again)",
        seen.t,
        d.hidden.len(),
        d.moved.len(),
        d.replaced.len()
    );
    let words = exposure.view.words.clone();
    if let Err(e) = buffers.insert(
        lights.0.id(),
        ShaderBuffer::new(words, RenderAssetUsages::RENDER_WORLD),
    ) {
        eprintln!("play: the light buffer: {e:?}");
    }
}

/// The person's mode (`--play-keys`): what the person does at the window,
/// and the synthetic person (`--play-keys-from`, test and measurement only)
/// with the walking speed it steers at and the last frame's run seconds.
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PersonInput {
    person: crate::person::Person,
    synthetic: Option<(crate::person::Synthetic, f64)>,
    last_dt: f64,
    /// The synthetic person's end was logged.
    synthetic_done: bool,
    /// The left button's drags by the window's cursor.
    drag: crate::person::Drag,
    /// Shift is on as the movement key's modifier.
    shift_modifier: bool,
}

/// The pad read with the keys (`--play-pad`, or a synthetic person's
/// `pad` lines): its source, its state between frames, and its presence as
/// last told. Dropped with the app, after the run is closed (the device's
/// reader may take seconds to let go).
#[cfg(feature = "script-event")]
#[derive(Resource)]
struct PadInput {
    source: Box<dyn crate::pad::PadSource>,
    state: crate::person::PadState,
    status: crate::pad::Status,
    /// Whether the window had the focus at the last frame (told at each
    /// change: without it the pad reads as let go).
    focused: Option<bool>,
}

/// `--play-keys`, each frame: the keys held, E, Escape and P, the arrow
/// keys, and the mouse's motion while the left button is held, as the
/// person's state (`crate::person`: the key map). The look reads the raw
/// device motion, or when there is none the window's cursor between its
/// positions while the button is down, in the order the window's events
/// came (`person::Drag`: a tool's drag carries its motion on the release,
/// or presses and lets go within one frame); Shift counts as a key of its
/// own or as the movement key's modifier (`person::shift_of_modifiers`).
/// With a pad (`PadInput`), its held keys are added to the keyboard's,
/// its right stick looks after the arrow keys, and A, Menu and View or B
/// press as E, P and Escape do; a window without the focus reads the pad
/// as let go (`person::PadState`). The scripted window's system sends the
/// commands when the run's clock moves.
#[cfg(feature = "script-event")]
#[allow(clippy::too_many_arguments)]
fn person_keys(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut window_events: MessageReader<bevy::window::WindowEvent>,
    mut modifiers: MessageReader<bevy::winit::RawWinitWindowEvent>,
    time: Res<Time>,
    mut input: ResMut<PersonInput>,
    pad: Option<ResMut<PadInput>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    use crate::person::Pointer;
    use bevy::window::WindowEvent;
    for e in window_events.read() {
        let p = match e {
            WindowEvent::CursorMoved(m) => {
                Pointer::Moved([f64::from(m.position.x), f64::from(m.position.y)])
            }
            WindowEvent::CursorLeft(_) => Pointer::Left,
            WindowEvent::MouseButtonInput(b) if b.button == MouseButton::Left => {
                Pointer::Button(b.state.is_pressed())
            }
            _ => continue,
        };
        input.drag.feed(p);
    }
    for e in modifiers.read() {
        if let Some(on) = crate::person::shift_of_modifiers(&format!("{:?}", e.event)) {
            input.shift_modifier = on;
        }
    }
    let (cursor, dragged) = input.drag.take();
    let held = crate::person::Held {
        forward: keys.pressed(KeyCode::KeyW),
        back: keys.pressed(KeyCode::KeyS),
        left: keys.pressed(KeyCode::KeyA),
        right: keys.pressed(KeyCode::KeyD),
        run: false,
    };
    if !(held.forward || held.back || held.left || held.right) {
        input.shift_modifier = false;
    }
    let shift_key = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let run = crate::person::run_held(shift_key, input.shift_modifier, held);
    let dt = time.delta_secs_f64();
    let (pad_held, presses, stick) = match pad {
        Some(mut pad) => {
            let pad = &mut *pad;
            let poll = pad.source.poll(input.person.clock);
            let why = if poll.connected {
                None
            } else {
                pad.source.absent()
            };
            if let Some(line) = pad.status.update(poll.connected, why) {
                println!("{line} at {:.3} s of the play clock", input.person.clock);
            }
            let focused = windows.iter().next().is_none_or(|w| w.focused);
            if pad.focused.replace(focused) != Some(focused) {
                println!(
                    "play: pad: the window {} the focus at {:.3} s of the play clock{}",
                    if focused { "has" } else { "lost" },
                    input.person.clock,
                    if focused {
                        ""
                    } else {
                        " (the pad reads as let go)"
                    }
                );
            }
            let presses = pad.state.frame(&poll, focused, dt);
            let stick = if focused && poll.connected {
                poll.right
            } else {
                [0.0; 2]
            };
            (pad.state.held, presses, stick)
        }
        None => Default::default(),
    };
    let p = &mut input.person;
    if keys.just_pressed(KeyCode::KeyP) || presses.pause {
        p.toggle_pause();
        println!(
            "play: person: {} at {:.3} s of the play clock",
            if p.paused { "paused" } else { "resumed" },
            p.clock
        );
    }
    if keys.just_pressed(KeyCode::Escape) || presses.end {
        p.press_quit();
    }
    if keys.just_pressed(KeyCode::KeyE) || presses.activate {
        p.press_activate();
    }
    p.held = crate::person::held_either(crate::person::Held { run, ..held }, pad_held);
    p.arrows(
        keys.pressed(KeyCode::ArrowLeft),
        keys.pressed(KeyCode::ArrowRight),
        keys.pressed(KeyCode::ArrowUp),
        keys.pressed(KeyCode::ArrowDown),
        dt,
    );
    p.stick_look(stick, dt);
    if dragged || buttons.pressed(MouseButton::Left) || buttons.just_released(MouseButton::Left) {
        let raw = [f64::from(motion.delta.x), f64::from(motion.delta.y)];
        p.look(crate::person::look_motion(raw, cursor));
    }
}

/// The window's ordered events of a drag by the cursor alone by `d`
/// pixels: the cursor at a point, the left button pressed, the cursor moved
/// and the button let go (`person::Act::Drag`).
#[cfg(feature = "script-event")]
fn synthetic_drag(window: Entity, d: [f64; 2]) -> Vec<bevy::window::WindowEvent> {
    use bevy::input::mouse::MouseButtonInput;
    use bevy::input::ButtonState;
    use bevy::window::{CursorMoved, WindowEvent};
    let at = Vec2::new(400.0, 300.0);
    let to = at + Vec2::new(d[0] as f32, d[1] as f32);
    let button = |state| {
        WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        })
    };
    vec![
        WindowEvent::CursorMoved(CursorMoved {
            window,
            position: at,
            delta: None,
        }),
        button(ButtonState::Pressed),
        WindowEvent::CursorMoved(CursorMoved {
            window,
            position: to,
            delta: Some(to - at),
        }),
        button(ButtonState::Released),
    ]
}

/// `--play-keys-from` (test and measurement only), each frame before the
/// input systems: the synthetic person's events due at the play clock,
/// written as the key, button and motion messages a real keyboard and
/// mouse send, so the key map reads them as it reads a person's.
#[cfg(feature = "script-event")]
fn person_inject(
    mut input: ResMut<PersonInput>,
    playing: Res<Playing>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut keyboard: MessageWriter<bevy::input::keyboard::KeyboardInput>,
    mut mouse_buttons: MessageWriter<bevy::input::mouse::MouseButtonInput>,
    mut mouse_motion: MessageWriter<bevy::input::mouse::MouseMotion>,
    mut window_events: MessageWriter<bevy::window::WindowEvent>,
) {
    use crate::person::KeyName;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::ButtonState;
    let Some(window) = windows.iter().next() else {
        return;
    };
    let PersonInput {
        person,
        synthetic,
        last_dt,
        synthetic_done,
        ..
    } = &mut *input;
    let Some((s, speed)) = synthetic.as_mut() else {
        return;
    };
    let seen = playing.0.seen();
    let feet = seen
        .as_ref()
        .and_then(|x| x.feet)
        .map(|f| (f, fo4_walk::Params::PROVISIONAL.eye_height));
    let walker_t = seen.as_ref().map_or(person.clock, |x| x.t);
    // Let W go early enough that the walker, a frame or two behind the
    // window, stops near the point.
    let lead = (person.clock - walker_t).max(0.0) + *last_dt;
    let arrive = *speed * lead + 1.0;
    let f = s.frame(
        person.clock,
        (person.heading, person.pitch),
        person.degrees_per_pixel,
        feet,
        arrive,
    );
    let code = |k: KeyName| match k {
        KeyName::W => KeyCode::KeyW,
        KeyName::A => KeyCode::KeyA,
        KeyName::S => KeyCode::KeyS,
        KeyName::D => KeyCode::KeyD,
        KeyName::E => KeyCode::KeyE,
        KeyName::P => KeyCode::KeyP,
        KeyName::Escape => KeyCode::Escape,
        KeyName::Shift => KeyCode::ShiftLeft,
        KeyName::Left => KeyCode::ArrowLeft,
        KeyName::Right => KeyCode::ArrowRight,
        KeyName::Up => KeyCode::ArrowUp,
        KeyName::Down => KeyCode::ArrowDown,
    };
    for (keys, state) in [
        (&f.down, ButtonState::Pressed),
        (&f.up, ButtonState::Released),
    ] {
        for k in keys {
            keyboard.write(KeyboardInput {
                key_code: code(*k),
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window,
            });
        }
    }
    if let Some(b) = f.button {
        mouse_buttons.write(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state: if b {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            window,
        });
    }
    if f.motion != [0.0; 2] {
        mouse_motion.write(bevy::input::mouse::MouseMotion {
            delta: Vec2::new(f.motion[0] as f32, f.motion[1] as f32),
        });
    }
    // A drag by the cursor alone: the window's ordered events as the
    // window system sends a tool's drag (the cursor at a point, the press,
    // the release carrying the cursor's end), no raw motion.
    if let Some(d) = f.drag {
        window_events.write_batch(synthetic_drag(window, d));
    }
    if s.done() && !*synthetic_done {
        *synthetic_done = true;
        println!(
            "play: synthetic person: every event given at {:.3} s of the play clock; {} steered points reached",
            person.clock,
            s.arrived.len()
        );
    }
}

/// The play mode, each frame after the walker moved: the frame's seconds,
/// the walker's feet and, when E was pressed, what the eye's ray meets go
/// to the run; the run's second, last stage and spoken lines join the
/// title bar. While flying (Tab) the player stands where the walker left.
#[cfg(feature = "script-event")]
fn play_frame(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut walk: Option<ResMut<Walk>>,
    mut playing: ResMut<Playing>,
    mut readout: ResMut<Readout>,
    mut instances: Query<(Entity, &MeshTag, &mut Visibility, &mut Transform)>,
) {
    let dt = time.delta_secs_f64();
    let Playing(p, title, drawn) = &mut *playing;
    let walker = walk
        .as_deref()
        .and_then(|w| w.active.as_ref().map(|v| (w, v)));
    let (feet, activated) = match walker {
        Some((w, v)) => {
            let act = if keys.just_pressed(KeyCode::KeyE) {
                w.space
                    .get(&[0, 0])
                    .and_then(|world| p.pick(world, v.character.eye(), v.look.forward()))
            } else {
                None
            };
            (v.character.feet, act)
        }
        None => (p.frames.last(), None),
    };
    if let Some(t) = p.frame(dt, feet, activated) {
        *title = t;
    }
    play_live(p, drawn, walk.as_deref_mut(), &mut instances);
    if !title.is_empty() {
        readout.text.push_str(" | ");
        readout.text.push_str(title);
    }
}

/// A drawn instance of the cell, as the live world's decisions reach it.
#[cfg(feature = "script-event")]
type CellInstance = (
    Entity,
    &'static MeshTag,
    &'static mut Visibility,
    &'static mut Transform,
);

/// Run seconds one frame may add to the scripted run's clock at most: a
/// long frame (an upload) does not let the run pass far beyond what was
/// drawn.
#[cfg(feature = "script-event")]
const PLAY_FRAME_MAX: f64 = 0.25;

/// The scripted play mode (`--play-input`), each frame: a cell the
/// loading thread finished replaces the scene (its upload then follows as
/// at the start); while the scene is drawn and no cell loads, the window's
/// clock lets the run go on by the frame's seconds; the camera stands at
/// the walker's eye looking along its heading; a cell the walker's walking
/// world is about that the window does not draw is loaded (the run waits);
/// the frame is captured each time the run passes a multiple of the shot
/// period; the window closes when the run is done.
#[cfg(feature = "script-event")]
#[allow(clippy::too_many_arguments)]
fn play_scripted(
    mut commands: Commands,
    time: Res<Time>,
    mut playing: ResMut<Playing>,
    mut fly: ResMut<Fly>,
    mut readout: ResMut<Readout>,
    pending: Option<Res<PendingScene>>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
    mut instances: Query<CellInstance, (Without<FlyCamera>, Without<CastEntity>)>,
    mut casts: Query<(Entity, &CastEntity, &mut Transform), Without<FlyCamera>>,
    mut platforms: Option<ResMut<PlayPlatforms>>,
    (mut doors, mut leaves, mut skins, mut meshes): FollowParams,
    (mut exposure, lights, mut buffers): ExposureParams,
    mut exit: MessageWriter<AppExit>,
    mut person: Option<ResMut<PersonInput>>,
    mut watch: Option<ResMut<PlayWatch>>,
) {
    let Playing(p, title, drawn) = &mut *playing;
    match p.loaded() {
        Some(Ok(r)) => {
            // The cell before stays drawn until the new one is spawned
            // (`spawn_window_scene`); its cast too while the new cell lies
            // in the same worldspace, else it goes with its cell.
            let n = instances.iter().count();
            drawn.old.extend(instances.iter().map(|x| x.0));
            if !r.keep_cast {
                drawn.old.extend(casts.iter().map(|x| x.0));
                commands.remove_resource::<PendingCast>();
            }
            println!(
                "play: cell {} loaded in {:.2} s (asked at {:.3} s of the run); {n} drawn instances of the cell before removed as it is drawn{}",
                fo4_scene::timeline::key_text(&r.cell),
                r.load_seconds,
                r.asked_at,
                if r.keep_cast {
                    ", the cast drawn kept until the new cell's (one worldspace)"
                } else {
                    ", the cast with them"
                }
            );
            let scene = PendingScene(r.gpu, r.clear, None);
            let loaded = crate::play::Loaded {
                cell: r.cell,
                load_seconds: r.load_seconds,
                started: std::time::Instant::now(),
            };
            drawn.spawned.clear();
            drawn.applied = None;
            if r.keep_cast || !p.casts() {
                commands.insert_resource(scene);
                drawn.uploading = Some((1, loaded));
            } else {
                drawn.held = Some((scene, loaded, std::time::Instant::now(), false));
            }
        }
        Some(Err(e)) => {
            eprintln!("play: loading the walker's cell: {e}");
            std::process::exit(2);
        }
        None => {}
    }
    // A cell of another space held for its cast: drawn once the cast came
    // or the wait is over.
    let release = drawn
        .held
        .as_ref()
        .is_some_and(|h| h.3 || h.2.elapsed().as_secs_f64() >= CAST_REVEAL_WAIT);
    if release {
        if let Some((scene, mut loaded, since, cast)) = drawn.held.take() {
            println!(
                "play: cell {} drawn {} after {:.2} s held for its cast",
                fo4_scene::timeline::key_text(&loaded.cell),
                if cast {
                    "with its cast"
                } else {
                    "without its cast"
                },
                since.elapsed().as_secs_f64()
            );
            loaded.started = std::time::Instant::now();
            commands.insert_resource(scene);
            drawn.uploading = Some((1, loaded));
        }
    }
    let mut ready = pending.is_none() && !p.loading() && drawn.held.is_none();
    if let Some((frames, l)) = drawn.uploading.as_mut() {
        ready = false;
        if *frames > 0 {
            *frames -= 1;
        } else if pending.is_none() {
            println!(
                "play: cell {} drawn: load {:.2} s, upload {:.2} s",
                fo4_scene::timeline::key_text(&l.cell),
                l.load_seconds,
                l.started.elapsed().as_secs_f64()
            );
            drawn.uploading = None;
            // The new scene's instances take the live world's decisions.
            drawn.applied = None;
            ready = true;
        }
    }
    let Some(seen) = p.seen() else {
        return;
    };
    if ready {
        if let Some(cell) = p.cell_wanted(&seen) {
            p.load(cell, seen.t, &seen);
            ready = false;
        }
    }
    // The window's clock starts once the walker stands somewhere (and
    // stops while the person pauses).
    let dt = time.delta_secs_f64().min(PLAY_FRAME_MAX);
    let paused = person.as_ref().is_some_and(|x| x.person.paused);
    // --play-from: the run goes there unpaced; the clock is held until
    // the window stands ready there.
    let held_from = p.play_from_holds(seen.t, ready && seen.feet.is_some());
    let go = ready && seen.feet.is_some() && !paused && !held_from;
    let given = if go { dt } else { 0.0 };
    let mut play_clock = seen.t;
    let changed = match person.as_deref_mut() {
        // --play-keys: the person's commands of the frame go with its
        // seconds.
        Some(x) => {
            // The person's commands of every frame but a paused one: made
            // while the play clock stands (a cell loading, the run busy),
            // the gate keeps them for when it runs again.
            let commands = if !paused && seen.feet.is_some() {
                x.person.commands()
            } else {
                Vec::new()
            };
            let (title, clock) = p.person_frame(given, time.elapsed_secs_f64(), commands);
            play_clock = clock;
            // The play clock the walker goes by (it stands while a busy
            // run is owed its most), for the synthetic person's events.
            x.last_dt = (clock - x.person.clock).max(0.0);
            x.person.clock = clock;
            title
        }
        None => p.scripted_frame(given),
    };
    // The title's second is the play clock's (what the window draws), not
    // the run's last event's.
    let _ = changed;
    *title = p.title_at(play_clock);
    if play_clock != drawn.clock.0 || paused || seen.feet.is_none() {
        drawn.clock = (play_clock, None);
    } else if drawn.clock.1.is_none() {
        drawn.clock.1 = Some(std::time::Instant::now());
    }
    if let Some(feet) = seen.feet {
        // --play-keys: the camera looks where the person looks (the walker
        // takes the look a frame or so later).
        let look = person.as_ref().map(|x| (x.person.heading, x.person.pitch));
        let state = FlyState {
            eye: [
                feet[0],
                feet[1],
                feet[2] + fo4_walk::Params::PROVISIONAL.eye_height,
            ],
            yaw: match look {
                Some((h, _)) => h.to_radians(),
                None => seen.heading.map_or(fly.state.yaw, f64::to_radians),
            },
            pitch: look
                .map_or(seen.pitch, |l| l.1)
                .to_radians()
                .clamp(-crate::fly::MAX_PITCH, crate::fly::MAX_PITCH),
        };
        if let Ok(pose) = fly_pose(&state) {
            fly.state = state;
            for mut t in &mut cameras {
                t.translation = pose.0;
                t.rotation = pose.1;
            }
        }
    }
    // Not while the old scene leaves and the new one uploads.
    if drawn.uploading.is_none() && drawn.held.is_none() {
        play_live(p, drawn, None, &mut instances);
        if let Some(pp) = platforms.as_deref_mut() {
            follow_platforms(&seen, pp, &mut instances, skins.is_some());
        }
        // `--draw-cast-settles`: the places whose ground the play thread
        // is asked for.
        let asks: std::collections::BTreeMap<fo4_plugin::FormKey, [f64; 3]> = casts
            .iter()
            .filter(|(_, c, _)| c.settle)
            .filter_map(|(_, c, _)| c.place.clone())
            .collect();
        let settles = !asks.is_empty();
        if settles || !seen.ground_asked.is_empty() {
            p.ask_ground(asks);
        }
        if !seen.platforms.is_empty() || settles {
            ride_cast(&seen, &mut drawn.feet, &mut drawn.settled, &mut casts);
        }
        clear_cast(&seen, &mut drawn.cleared, &mut casts);
        if let Some(w) = watch.as_deref_mut() {
            watch_play(&seen, w, &casts, doors.as_deref());
        }
        if let Some(pd) = doors.as_deref_mut() {
            follow_doors(p, &seen, pd, &mut instances);
        }
        if let Some(pl) = leaves.as_deref_mut() {
            follow_leaves(p, &seen, pl, &mut instances);
        }
        if let Some(ps) = skins.as_deref_mut() {
            follow_skins(&seen, ps, &mut meshes);
        }
        if let (Some(x), Some(l)) = (exposure.as_deref_mut(), lights.as_deref()) {
            expose_view(&seen, x, skins.as_deref(), &instances, l, &mut buffers);
        }
    }
    // --timeline-cast: the run's actors of the drawn cell, built again on
    // the cast's thread when what decides them changed.
    if ready {
        p.cast_ask(&seen);
    }
    // The first cell read again on the play data thread, once.
    p.prepare();
    if let Some(c) = p.cast_done() {
        println!("play: cast at {:.3} s of the run: {}", seen.t, c.line);
        for d in &c.details {
            println!("play: cast: {d}");
        }
        commands.insert_resource(PendingCast(c.gpu, None, std::time::Instant::now()));
        if let Some(h) = drawn.held.as_mut() {
            h.3 = true;
        }
    }
    // --play-shots: the frame each time the run passes a multiple of the
    // period, once the scene is drawn.
    let shots = &mut drawn.shots;
    if let Some((dir, every)) = p.shots() {
        if let Some((at, s)) = &shots.asked {
            let image = shots.captured.lock().ok().and_then(|mut c| c.take());
            if let Some(image) = image {
                let file = format!("{dir}/t{at:07.1}.jpg");
                let root = p.data_root().map(std::path::PathBuf::from);
                let (at, s) = (*at, s.clone());
                // Encoded and written on a thread of its own (viewer
                // policy): a frame's PNG took 0.7 to 1.0 s on the window's
                // frame loop. JPEG at quality 80 (viewer policy: a person
                // looks at these frames; nothing compares them).
                let write = move || {
                    match capture_jpeg(image).and_then(|(bytes, size)| {
                        use std::io::Write;
                        crate::load::output_file(&file, root.as_deref())
                            .map_err(|e| e.to_string())?
                            .write_all(&bytes)
                            .map(|()| size)
                            .map_err(|e| format!("{file}: {e}"))
                    }) {
                        Ok(size) => println!(
                            "play: frame {file} ({}x{}) at {:.3} s of the run (asked {at} s): cell {}, feet {:?}, heading {:?}, pitch {}",
                            size[0],
                            size[1],
                            s.t,
                            s.cell.as_ref().map(fo4_scene::timeline::key_text).unwrap_or_default(),
                            s.feet,
                            s.heading,
                            s.pitch
                        ),
                        Err(e) => eprintln!("play: frame capture: {e}"),
                    }
                };
                match std::thread::Builder::new()
                    .name("fo4-view play shot".into())
                    .spawn(write)
                {
                    Ok(h) => shots.writing.push(h),
                    Err(e) => eprintln!("play: frame capture: {e}"),
                }
                shots.writing.retain(|h| !h.is_finished());
                shots.asked = None;
                shots.written += 1;
            }
        } else if let Some(from) = p.play_from_pending().filter(|_| seen.done.is_none()) {
            // --play-from: no frame before the window's pace takes over.
            shots.next = p
                .next_shot(from - 1e-6)
                .unwrap_or(((from / every).floor() + 1.0) * every);
        } else if ready
            && seen.feet.is_some()
            && (seen.t >= shots.next || seen.done.is_some() && !shots.last)
        {
            let slot = shots.captured.clone();
            commands.spawn(Screenshot::primary_window()).observe(
                move |shot: On<ScreenshotCaptured>| {
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(shot.event().image.clone());
                    }
                },
            );
            let at = if seen.done.is_some() {
                shots.last = true;
                seen.t
            } else {
                shots.next
            };
            shots.asked = Some((at, seen.clone()));
            shots.next = p
                .next_shot(seen.t)
                .unwrap_or(((seen.t / every).floor() + 1.0) * every);
        }
    }
    if let Some(done) = &seen.done {
        let shot_due = p.shots().is_some() && (shots.asked.is_some() || !shots.last);
        if !shot_due && drawn.uploading.is_none() {
            // The frames being written are written before the window closes.
            for h in std::mem::take(&mut shots.writing) {
                let _ = h.join();
            }
            match done {
                Ok(()) => println!(
                    "play: the scripted run is done at {:.3} s; the window closes",
                    seen.t
                ),
                Err(e) => eprintln!("play: the scripted run failed: {e}"),
            }
            exit.write(AppExit::Success);
        }
    }
    if !title.is_empty() {
        readout.text.push_str(" | ");
        readout.text.push_str(title);
    }
    // --play-keys: what E would activate, and the clock's state.
    if let Some(x) = person.as_ref() {
        readout.text.push_str(" | E: ");
        readout
            .text
            .push_str(seen.aim.as_deref().unwrap_or("nothing within reach"));
        if x.person.paused {
            readout.text.push_str(" | paused (P resumes)");
        }
        if x.person.ended {
            readout
                .text
                .push_str(" | input ended: the run goes on to its end");
        }
    }
}

/// Texture and mesh bytes uploaded per frame at most for streamed cells
/// (one item at least), as for the start-up scene.
const STREAM_BATCH_BYTES: u64 = UPLOAD_BATCH_BYTES as u64;

/// The moving grid, each frame: sends the cell under the player (the
/// walker's feet, or the eye when flying) when it changed, applies every
/// message of the loading thread in order (a cell's collision becomes
/// walkable at once; a dropped cell is despawned and data no cell uses any
/// more is freed), then uploads the waiting cells' data, a bounded batch
/// per frame, and spawns each cell once its data is on the GPU.
fn stream_cells(world: &mut World) {
    let Some(mut stream) = world.remove_resource::<Stream>() else {
        return;
    };
    let result = stream_frame(world, &mut stream);
    world.insert_resource(stream);
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

fn mib(n: u64) -> f64 {
    n as f64 / f64::from(1u32 << 20)
}

fn stream_frame(world: &mut World, stream: &mut Stream) -> Result<(), String> {
    let point = match world.get_resource::<Walk>().and_then(|w| w.active.as_ref()) {
        Some(view) => view.character.feet,
        None => world.resource::<Fly>().state.eye,
    };
    if let Some(c) = crate::stream::cell_at(point[0], point[1]) {
        if stream.sent != Some(c) {
            println!("stream: centre {},{}", c[0], c[1]);
            // A closed channel means the loading thread stopped; its
            // message says why.
            let _ = stream.centres.send(c);
            stream.sent = Some(c);
        }
    }
    let messages: Vec<Message> = {
        let rx = stream.messages.lock().map_err(|_| "StreamChannelLock")?;
        rx.try_iter().collect()
    };
    for m in messages {
        match m {
            Message::Load(piece) => {
                let r = &piece.report;
                println!(
                    "stream: cell {},{} {} loaded in {:.3} s (worker: {}): {} placements, {} omitted, terrain {}, {} triangles, {} new assets ({:.1} MiB), {} shared; credits {:.1} MiB",
                    piece.grid[0],
                    piece.grid[1],
                    r.outcome,
                    r.seconds,
                    crate::cells::Phases::line(&r.phases),
                    r.placements,
                    r.omitted,
                    r.terrain,
                    r.triangles,
                    r.new_assets,
                    mib(r.new_bytes),
                    r.shared_assets,
                    mib(r.credits),
                );
                let old = match world.get_resource_mut::<Walk>() {
                    Some(mut walk) => stream.live.receive(*piece, &mut walk.space)?,
                    None => stream
                        .live
                        .receive(*piece, &mut fo4_walk::WorldSet::new())?,
                };
                for e in old {
                    world.despawn(e);
                }
            }
            Message::Drop { grid, credits } => {
                let entities = match world.get_resource_mut::<Walk>() {
                    Some(mut walk) => stream.live.drop_cell(grid, &mut walk.space),
                    None => stream.live.drop_cell(grid, &mut fo4_walk::WorldSet::new()),
                };
                let n = entities.len();
                for e in entities {
                    world.despawn(e);
                }
                println!(
                    "stream: cell {},{} dropped ({n} entities); credits {:.1} MiB; {} meshes and {} textures held",
                    grid[0],
                    grid[1],
                    mib(credits),
                    stream.live.meshes.len(),
                    stream.live.textures.len(),
                );
            }
            Message::Failed(e) => return Err(e),
            Message::Ready(_) => {}
        }
    }
    upload_waiting(world, &mut stream.live)
}

fn handle_of<D, H>(slot: Option<&Slot<D, H>>) -> Result<&H, String> {
    match slot {
        Some(Slot::Ready(h)) => Ok(h),
        _ => Err("StreamDataNotUploaded".into()),
    }
}

/// Uploads the data of the waiting cells in arrival order, at most
/// [`STREAM_BATCH_BYTES`] per frame, and spawns each cell whose data is
/// all on the GPU.
fn upload_waiting(world: &mut World, live: &mut Live) -> Result<(), String> {
    let mut bytes = 0u64;
    while let Some(&grid) = live.waiting.front() {
        let Some(cell) = live.cells.get_mut(&grid) else {
            live.waiting.pop_front();
            continue;
        };
        cell.frames += 1;
        let (mesh_keys, texture_keys) = (cell.meshes.clone(), cell.textures.clone());
        for k in &texture_keys {
            let Some(slot) = live.textures.get_mut(k) else {
                return Err("StreamTextureNotHeld".into());
            };
            if let Slot::Waiting(t) = slot {
                if bytes > 0 && bytes + k.bytes > STREAM_BATCH_BYTES {
                    return Ok(());
                }
                if matches!(t.format, TexelFormat::Blocks(_)) && !samples_blocks(world) {
                    return Err("GpuWithoutBcTextures: this device cannot sample block-compressed textures; use --reference-textures".into());
                }
                bytes += k.bytes;
                let image = bevy_image(t);
                *slot = Slot::Ready(world.resource_mut::<Assets<Image>>().add(image));
            }
        }
        for k in &mesh_keys {
            let Some(slot) = live.meshes.get_mut(k) else {
                return Err("StreamMeshNotHeld".into());
            };
            if let Slot::Waiting(m) = slot {
                if bytes > 0 && bytes + k.bytes > STREAM_BATCH_BYTES {
                    return Ok(());
                }
                bytes += k.bytes;
                let mesh = bevy_mesh(std::mem::take(m));
                *slot = Slot::Ready(world.resource_mut::<Assets<Mesh>>().add(mesh));
            }
        }
        let meshes: Vec<Handle<Mesh>> = mesh_keys
            .iter()
            .map(|k| handle_of(live.meshes.get(k)).cloned())
            .collect::<Result<_, _>>()?;
        let images: Vec<Handle<Image>> = texture_keys
            .iter()
            .map(|k: &AssetKey| handle_of(live.textures.get(k)).cloned())
            .collect::<Result<_, _>>()?;
        let Live {
            cells, materials, ..
        } = &mut *live;
        let Some(cell) = cells.get_mut(&grid) else {
            continue;
        };
        // One material per surface identity, shared with every spawned
        // cell drawing the same surface, so that instances of different
        // cells batch as in one joined scene.
        let mut handles = Vec::with_capacity(cell.surfaces.len());
        for (surface, id) in cell.surfaces.iter().zip(&cell.surface_ids) {
            let handle = if materials.acquire(id) {
                materials.get(id).cloned().ok_or("StreamMaterialNotHeld")?
            } else {
                let image = images
                    .get(surface.texture)
                    .cloned()
                    .ok_or("StreamSurfaceTexture")?;
                let palette = match surface.palette {
                    Some(p) => Some(images.get(p).cloned().ok_or("StreamSurfaceTexture")?),
                    None => None,
                };
                let lights = match world.get_resource::<LightBuffer>() {
                    Some(b) => b.0.clone(),
                    None => insert_light_buffer(world, None),
                };
                let soft = soft_depth_buffer(world, None);
                let h = world
                    .resource_mut::<Assets<SurfaceMaterial>>()
                    .add(surface_material(
                        surface, image, palette, lights, soft, false, 0,
                    ));
                materials.insert(*id, h.clone());
                h
            };
            handles.push(handle);
        }
        let mut bundles = Vec::with_capacity(cell.instances.len());
        for i in &cell.instances {
            let (Some(mesh), Some(material)) = (meshes.get(i.mesh), handles.get(i.surface)) else {
                return Err("StreamInstanceIndex".into());
            };
            bundles.push(instance_bundle(mesh.clone(), material.clone(), i, 0));
        }
        cell.entities = world.spawn_batch(bundles).collect();
        cell.spawned = true;
        println!(
            "stream: cell {},{} drawn: {} instances, uploaded over {} frames, {:.0} ms after it arrived",
            grid[0],
            grid[1],
            cell.instances.len(),
            cell.frames,
            cell.received.elapsed().as_secs_f64() * 1e3,
        );
        live.waiting.pop_front();
    }
    Ok(())
}

/// Windowed viewer whose loaded grid follows the player: `stream` talks to
/// the loading thread ([`crate::cells::worker`]), which has already
/// answered that it is ready. Walking starts at once; the walker waits
/// over a cell that has not arrived yet.
pub fn window_stream(
    stream: Stream,
    start: FlyState,
    settings: FlySettings,
    clear: [f32; 4],
    aids: Aids,
) -> Result<(), String> {
    fly_pose(&start)?;
    let view = WalkView::from_fly(
        start,
        fo4_walk::Params::PROVISIONAL,
        fo4_walk::Settings::DEFAULT,
    )?;
    let mut app = App::new();
    app.insert_resource(Walk {
        space: fo4_walk::WorldSet::new(),
        active: Some(view),
    })
    .insert_resource(stream)
    .add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "fo4-view".into(),
                    ..default()
                }),
                ..default()
            })
            .set(stream_pbr()),
    )
    .add_plugins(ViewerPlugin)
    .insert_resource(Fly {
        state: start,
        settings,
        start,
        start_walking: true,
    })
    .init_resource::<Readout>()
    // No scene to upload at start: the camera is spawned at once.
    .insert_resource(PendingScene(GpuScene::default(), clear, None))
    .add_systems(
        Update,
        (spawn_window_scene, stream_cells, fly_camera, show_readout).chain(),
    );
    aids.add_to(&mut app);
    app.run();
    Ok(())
}

/// The moving grid window's PBR plugin: per-instance data built on the
/// CPU (`use_gpu_instance_buffer_builder: false`). With Bevy's GPU-built
/// instance data and a material per cell, cells despawned and spawned
/// again were measured to draw wrongly, and the frame took more than
/// twice the GPU time of the same cells loaded once (`spec/scene.md`,
/// "Drawing the moving grid"); the window that loads once keeps Bevy's
/// default.
pub(crate) fn stream_pbr() -> bevy::pbr::PbrPlugin {
    bevy::pbr::PbrPlugin {
        use_gpu_instance_buffer_builder: false,
        ..default()
    }
}

/// Measurement aids of the window (`--frame-log`, `--script-path`,
/// `--script-shots`); nothing runs without them.
#[derive(Default)]
pub struct Aids {
    /// `--draw-soft-effects`: the window's soft depth buffer and copy
    /// ([`SoftDepthBuffer`], [`SOFT_DEPTH_MAX_VIEW`]).
    pub soft_effects: bool,
    pub frame_log: Option<std::fs::File>,
    pub script: Option<Script>,
}
impl Aids {
    fn add_to(self, app: &mut App) {
        if self.soft_effects {
            soft_depth_buffer(app.world_mut(), Some(SOFT_DEPTH_MAX_VIEW));
            app.insert_resource(SoftEffectsOn);
        }
        if let Some(file) = self.frame_log {
            app.insert_resource(FrameLog::new(file))
                .add_systems(First, frame_begin)
                .add_systems(Last, frame_end);
        }
        if let Some(script) = self.script {
            app.insert_resource(script)
                .add_systems(Update, script_frame.after(fly_camera));
        }
    }
}

/// `--frame-log`: one tab-separated line per frame.
#[derive(Resource)]
struct FrameLog {
    out: std::io::BufWriter<std::fs::File>,
    frame: u64,
    /// When this frame's and the previous frame's updates began.
    begin: Option<std::time::Instant>,
    previous: Option<std::time::Instant>,
}
impl FrameLog {
    fn new(file: std::fs::File) -> Self {
        let mut out = std::io::BufWriter::new(file);
        use std::io::Write;
        let _ = writeln!(
            out,
            "frame\tunix_s\tinterval_ms\tupdate_ms\tcells\tuploading\tentities\tphase\tx\ty"
        );
        Self {
            out,
            frame: 0,
            begin: None,
            previous: None,
        }
    }
}

fn frame_begin(mut log: ResMut<FrameLog>) {
    log.previous = log.begin;
    log.begin = Some(std::time::Instant::now());
}

/// Writes the frame's line: the interval since the previous frame's
/// update began, the time from this update's start to its end, and what
/// is loaded.
fn frame_end(world: &mut World) {
    let entities = world.entities().count_spawned();
    let (cells, uploading) = world
        .get_resource::<Stream>()
        .map_or((0, 0), |s| (s.live.cells.len(), s.live.waiting.len()));
    let phase = world
        .get_resource::<Script>()
        .map_or("-", |s| s.phase.name());
    let point = match world.get_resource::<Walk>().and_then(|w| w.active.as_ref()) {
        Some(view) => view.character.feet,
        None => world.resource::<Fly>().state.eye,
    };
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mut log = world.resource_mut::<FrameLog>();
    let now = std::time::Instant::now();
    let ms = |a: Option<std::time::Instant>, b: Option<std::time::Instant>| match (a, b) {
        (Some(a), Some(b)) => format!("{:.3}", b.duration_since(a).as_secs_f64() * 1e3),
        _ => "-".into(),
    };
    let line = format!(
        "{}\t{unix:.3}\t{}\t{}\t{cells}\t{uploading}\t{entities}\t{phase}\t{:.1}\t{:.1}",
        log.frame,
        ms(log.previous, log.begin),
        ms(log.begin, Some(now)),
        point[0],
        point[1],
    );
    log.frame += 1;
    use std::io::Write;
    let _ = writeln!(log.out, "{line}");
}

/// A scripted run (`--script-path`, [`crate::script`]).
#[derive(Resource)]
pub struct Script {
    steer: Steer,
    phase: Phase,
    /// Set for one frame: return to the start pose as R does.
    restart: bool,
    /// Seconds not yet walked in whole ticks.
    accumulator: f64,
    /// The start and return frames' files (`--script-shots`).
    shots: Option<[std::fs::File; 2]>,
    /// The camera pose of the start frame, also used for the return frame.
    pose: Option<Transform>,
    captured: Arc<Mutex<Option<Image>>>,
    requested: bool,
}
impl Script {
    pub fn new(path: Vec<[f64; 2]>, shots: Option<[std::fs::File; 2]>) -> Self {
        Self {
            steer: Steer::new(path),
            phase: Phase::Loading,
            restart: false,
            accumulator: 0.0,
            shots,
            pose: None,
            captured: Arc::new(Mutex::new(None)),
            requested: false,
        }
    }

    /// One frame of the scripted walk: whole ticks of the controller, each
    /// with the input the steering gives; the view looks along the walk.
    fn walk(&mut self, view: &mut WalkView, space: &fo4_walk::WorldSet<[i32; 2]>, dt: f64) {
        let tick = view.character.settings.tick;
        if dt.is_finite() && dt > 0.0 {
            self.accumulator += dt;
        }
        while self.accumulator >= tick {
            self.accumulator -= tick;
            let c = &mut view.character;
            match self.steer.input(c.feet, c.params.run_speed, tick) {
                Some(input) => {
                    c.tick(space, input);
                    view.look.yaw = input.heading;
                }
                None => {
                    self.accumulator = 0.0;
                    break;
                }
            }
        }
        view.recover(space, dt);
        view.look.eye = view.character.eye();
    }
}

/// Whether the scene is drawn: the start-up scene spawned and, with the
/// moving grid, every cell within the loaded steps of the cell under the
/// player received and spawned.
fn drawn_around(world: &World) -> bool {
    if world.contains_resource::<PendingScene>() {
        return false;
    }
    let Some(stream) = world.get_resource::<Stream>() else {
        return true;
    };
    let point = match world.get_resource::<Walk>().and_then(|w| w.active.as_ref()) {
        Some(view) => view.character.feet,
        None => world.resource::<Fly>().state.eye,
    };
    let Some(c) = crate::stream::cell_at(point[0], point[1]) else {
        return false;
    };
    if !stream.live.waiting.is_empty() {
        return false;
    }
    let r = i64::from(stream.load);
    (-r..=r).all(|dy| {
        (-r..=r).all(|dx| {
            let g = [i64::from(c[0]) + dx, i64::from(c[1]) + dy];
            let Ok(g) = g
                .map(i32::try_from)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
            else {
                return true;
            };
            stream
                .live
                .cells
                .get(&[g[0], g[1]])
                .is_some_and(|cell| cell.spawned)
        })
    })
}

fn script_frame(world: &mut World) {
    let Some(mut script) = world.remove_resource::<Script>() else {
        return;
    };
    let result = script_step(world, &mut script);
    world.insert_resource(script);
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

/// A window capture as PNG bytes, and its size.
fn capture_png(image: Image) -> Result<(Vec<u8>, [u32; 2]), String> {
    let (rgba, size) = capture_rgba(image)?;
    let png = crate::load::png_rgba8(size, &rgba).map_err(|e| e.to_string())?;
    Ok((png, size))
}

/// A captured frame as a JPEG file at [`crate::jpeg::SHOT_QUALITY`].
#[cfg(feature = "script-event")]
fn capture_jpeg(image: Image) -> Result<(Vec<u8>, [u32; 2]), String> {
    let (rgba, size) = capture_rgba(image)?;
    let jpeg = crate::jpeg::encode_rgba8(size, &rgba, crate::jpeg::SHOT_QUALITY)?;
    Ok((jpeg, size))
}

/// A captured frame's RGBA8 pixels and size.
fn capture_rgba(image: Image) -> Result<(Vec<u8>, [u32; 2]), String> {
    let size = [image.width(), image.height()];
    let bgra = match image.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        f => return Err(format!("UnexpectedCaptureFormat: {f:?}")),
    };
    let mut rgba = image.data.ok_or("EmptyCapture")?;
    if bgra {
        for p in rgba.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
    }
    Ok((rgba, size))
}

fn script_step(world: &mut World, s: &mut Script) -> Result<(), String> {
    match s.phase {
        Phase::Loading => {
            if drawn_around(world) {
                println!("script: start drawn");
                s.phase = Phase::Start(0);
            }
        }
        Phase::Walking => {
            if s.steer.done() {
                println!(
                    "script: walked, {} waypoints given up; back to the start pose",
                    s.steer.missed
                );
                s.restart = true;
                s.phase = Phase::Returning;
            }
        }
        Phase::Returning => {
            if drawn_around(world) {
                println!("script: return drawn");
                s.phase = Phase::Return(0);
            }
        }
        Phase::Start(n) | Phase::Return(n) => {
            let start = matches!(s.phase, Phase::Start(_));
            if n < SETTLE_FRAMES {
                s.phase = if start {
                    Phase::Start(n + 1)
                } else {
                    Phase::Return(n + 1)
                };
                return Ok(());
            }
            // The same camera pose for both frames: the settled start pose.
            let mut cameras = world.query_filtered::<&mut Transform, With<FlyCamera>>();
            if s.pose.is_none() {
                s.pose = cameras.iter(world).next().copied();
            }
            if let Some(pose) = s.pose {
                for mut t in cameras.iter_mut(world) {
                    *t = pose;
                }
            }
            let next = if start { Phase::Walking } else { Phase::Done };
            let Some(files) = s.shots.as_mut() else {
                s.phase = next;
                return Ok(());
            };
            if !s.requested {
                let slot = s.captured.clone();
                world.spawn(Screenshot::primary_window()).observe(
                    move |shot: On<ScreenshotCaptured>| {
                        if let Ok(mut slot) = slot.lock() {
                            *slot = Some(shot.event().image.clone());
                        }
                    },
                );
                s.requested = true;
                return Ok(());
            }
            let Some(image) = s.captured.lock().map_err(|_| "ScreenshotLock")?.take() else {
                return Ok(());
            };
            s.requested = false;
            let (png, size) = capture_png(image)?;
            use std::io::Write;
            files[usize::from(!start)]
                .write_all(&png)
                .map_err(|e| e.to_string())?;
            println!(
                "script: {} frame captured ({}x{})",
                if start { "start" } else { "return" },
                size[0],
                size[1]
            );
            s.phase = next;
        }
        Phase::Done => {
            world.write_message(AppExit::Success);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "gpu_churn_tests.rs"]
mod churn_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// The surface shader takes no screen derivative of the f32 world
    /// position: far from the origin its rounding is a few hundredths of a
    /// pixel's span and drew the light in screen-column streaks (the GPU
    /// test `a_close_wall_far_from_the_origin_is_lit_without_column_streaks`
    /// draws it).
    #[test]
    fn the_surface_shader_takes_no_derivative_of_the_world_position() {
        let shader = include_str!("surface.wesl");
        for d in ["dpdx", "dpdy", "fwidth"] {
            for (i, _) in shader.match_indices(d) {
                let call = &shader[i..shader[i..].find(')').map_or(shader.len(), |e| i + e)];
                assert!(!call.contains("world_position"), "{call}");
            }
        }
    }

    #[test]
    fn projection_update_follows_reference_slopes_only_when_asked() {
        let frustum = fo4_scene::camera::reference_frustum([1280, 720]);
        let mut fixed = ViewerProjection {
            frustum,
            follow_aspect: false,
        };
        fixed.update(100., 100.);
        assert_eq!(fixed.frustum, frustum);
        let mut window = ViewerProjection {
            frustum,
            follow_aspect: true,
        };
        window.update(1280., 720.);
        assert_eq!(window.frustum, frustum);
        window.update(800., 600.);
        let (_, top) = fo4_scene::camera::reference_slopes([800, 600]);
        assert_eq!(window.frustum[2], top);
        assert_eq!(window.frustum[3], -top);
        assert_eq!(window.far(), 12000.);
    }

    #[test]
    fn fly_pose_matches_the_cpu_camera_pose() {
        let state = FlyState::from_eye_look([0., 153.6, 104.49], [70.7, 224.3, 104.49]).unwrap();
        let (t, r) = fly_pose(&state).unwrap();
        assert_eq!(t, Vec3::new(0., 104.49, -153.6));
        // Camera -Z (forward) in Bevy is the game look direction converted.
        let forward = r * Vec3::NEG_Z;
        let want = coords::game_to_bevy(state.forward());
        let want = Vec3::new(want[0] as f32, want[1] as f32, want[2] as f32);
        assert!(forward.abs_diff_eq(want, 1e-6), "{forward} {want}");
    }

    /// The windowed input system in an app without a window or renderer:
    /// input resources are set by hand and time advances by 1/60 s.
    struct InputApp(App);
    impl InputApp {
        fn new(walking: bool) -> Self {
            let b = fo4_core::Budget::new(16 << 20);
            let mut floor = fo4_walk::World::new();
            let (a, c, d, e) = (
                [-5000.0, -5000.0, 0.0],
                [5000.0, -5000.0, 0.0],
                [5000.0, 5000.0, 0.0],
                [-5000.0, 5000.0, 0.0],
            );
            floor
                .add_triangles(&[[a, c, d], [a, d, e]], "floor", &b)
                .unwrap();
            let eye = fo4_walk::Params::PROVISIONAL.eye_height;
            let start = FlyState::from_eye_look([0., 0., eye], [1., 0., eye]).unwrap();
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<AccumulatedMouseMotion>()
                .add_message::<CursorMoved>()
                .insert_resource(Time::<()>::default())
                .insert_resource(Fly {
                    state: start,
                    settings: crate::cli::DEFAULT_FLY,
                    start,
                    start_walking: walking,
                })
                .init_resource::<Readout>()
                .insert_resource(Walk {
                    space: {
                        let mut space = fo4_walk::WorldSet::new();
                        space.insert([0, 0], floor).unwrap();
                        space
                    },
                    active: walking.then(|| {
                        WalkView::from_fly(
                            start,
                            fo4_walk::Params::PROVISIONAL,
                            fo4_walk::Settings::DEFAULT,
                        )
                        .unwrap()
                    }),
                })
                .add_systems(Update, fly_camera);
            Self(app)
        }
        fn frame(&mut self) {
            let w = self.0.world_mut();
            w.resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
            self.0.update();
            let w = self.0.world_mut();
            w.resource_mut::<ButtonInput<KeyCode>>().clear();
            w.resource_mut::<ButtonInput<MouseButton>>().clear();
            w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::ZERO;
        }
        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame();
            }
        }
        fn keys(&mut self) -> Mut<'_, ButtonInput<KeyCode>> {
            self.0.world_mut().resource_mut::<ButtonInput<KeyCode>>()
        }
        fn fly(&self) -> &Fly {
            self.0.world().resource::<Fly>()
        }
        fn walker(&self) -> &WalkView {
            self.0.world().resource::<Walk>().active.as_ref().unwrap()
        }
    }

    /// The person's key map in an app without a window: a drag by the
    /// cursor alone (no raw motion, press and release in one frame) turns
    /// the view; the arrow keys turn and pitch; raw motion and the cursor
    /// are never both counted.
    #[cfg(feature = "script-event")]
    #[test]
    fn the_cursor_and_the_arrow_keys_look_in_the_person_mode() {
        let mut app = App::new();
        let window = app.world_mut().spawn_empty().id();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .add_message::<bevy::window::WindowEvent>()
            .add_message::<bevy::winit::RawWinitWindowEvent>()
            .insert_resource(Time::<()>::default())
            .insert_resource(PersonInput {
                person: crate::person::Person::new(90.0, 0.0, 0.1),
                synthetic: None,
                last_dt: 0.0,
                synthetic_done: false,
                drag: crate::person::Drag::default(),
                shift_modifier: false,
            })
            .add_systems(Update, person_keys);
        let frame = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f64(1. / 60.));
            app.update();
            let w = app.world_mut();
            w.resource_mut::<ButtonInput<KeyCode>>().clear();
            w.resource_mut::<ButtonInput<MouseButton>>().clear();
            w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::ZERO;
        };
        let look = |app: &App| {
            let p = &app.world().resource::<PersonInput>().person;
            (p.heading, p.pitch)
        };
        frame(&mut app);
        // A tool's drag: 100 pixels right, 50 up, by the cursor alone.
        app.world_mut()
            .write_message_batch(synthetic_drag(window, [100.0, -50.0]));
        frame(&mut app);
        let (h, p) = look(&app);
        assert!((h - 80.0).abs() < 1e-4 && (p - 5.0).abs() < 1e-4, "{h} {p}");
        // Raw motion with the button held: the raw motion alone.
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(-20.0, 0.0);
        app.world_mut()
            .write_message_batch(synthetic_drag(window, [500.0, 0.0]));
        frame(&mut app);
        let (h, _) = look(&app);
        assert!((h - 82.0).abs() < 1e-4, "{h}");
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        frame(&mut app);
        // The left arrow for half a second (30 frames): 45 degrees left;
        // the up arrow for 15 frames: 15 degrees up.
        let (h0, p0) = look(&app);
        for k in 0..30 {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ArrowLeft);
            if k < 15 {
                keys.press(KeyCode::ArrowUp);
            } else {
                keys.release(KeyCode::ArrowUp);
            }
            frame(&mut app);
        }
        let (h, p) = look(&app);
        assert!((h - h0 - 45.0).abs() < 1e-6, "{h} {h0}");
        assert!((p - p0 - 15.0).abs() < 1e-6, "{p} {p0}");
    }

    /// The pad in the person's mode, in an app without a device: a
    /// scripted pad's left stick holds the keys with the keyboard's, its
    /// right stick looks, A activates, View held a second ends; a window
    /// without the focus reads it as let go.
    #[cfg(feature = "script-event")]
    #[test]
    fn the_pad_walks_looks_and_ends_in_the_person_mode() {
        use crate::person::{Held, PadPoll};
        let up = PadPoll {
            connected: true,
            ..PadPoll::default()
        };
        let states = vec![
            (0.0, up),
            (
                1.0,
                PadPoll {
                    left: [0.0, 0.6],
                    right: [1.0, 0.0],
                    ..up
                },
            ),
            (
                2.0,
                PadPoll {
                    left: [0.0, 0.6],
                    activate: true,
                    ..up
                },
            ),
            (3.0, PadPoll { end: true, ..up }),
        ];
        let mut app = App::new();
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .add_message::<bevy::window::WindowEvent>()
            .add_message::<bevy::winit::RawWinitWindowEvent>()
            .insert_resource(Time::<()>::default())
            .insert_resource(PersonInput {
                person: crate::person::Person::new(90.0, 0.0, 0.1),
                synthetic: None,
                last_dt: 0.0,
                synthetic_done: false,
                drag: crate::person::Drag::default(),
                shift_modifier: false,
            })
            .insert_resource(PadInput {
                source: crate::pad::open(crate::pad::Wanted::Fake(crate::pad::FakePad::new(
                    states,
                )))
                .unwrap(),
                state: crate::person::PadState::default(),
                status: crate::pad::Status::default(),
                focused: None,
            })
            .add_systems(Update, person_keys);
        // One frame at a second of the play clock (the scripted window's
        // system moves it; here the test does).
        let frame = |app: &mut App, clock: f64| {
            app.world_mut().resource_mut::<PersonInput>().person.clock = clock;
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f64(0.25));
            app.update();
            let w = app.world_mut();
            w.resource_mut::<ButtonInput<KeyCode>>().clear();
            let mut input = w.resource_mut::<PersonInput>();
            input.person.commands()
        };
        use fo4_play::input::{Command, Key};
        // The first frame tells the start's heading.
        assert_eq!(frame(&mut app, 0.0), [Command::Face(90.0)]);
        // Half tilt forward with the keyboard's D: forward and right held;
        // the right stick turns right at 120 degrees a second for 0.25 s.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyD);
        assert_eq!(
            frame(&mut app, 1.0),
            [
                Command::Face(60.0),
                Command::Key(Key::Forward, true),
                Command::Key(Key::Right, true),
            ]
        );
        // A: the activation, once; the look stopped with the stick.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyD);
        assert_eq!(
            frame(&mut app, 2.0),
            [Command::Key(Key::Right, false), Command::Activate]
        );
        assert!(frame(&mut app, 2.25).is_empty());
        // The focus lost: the pad's keys let go.
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        assert_eq!(frame(&mut app, 2.5), [Command::Key(Key::Forward, false)]);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        // Focused again with A still held: the stick walks, A waits for
        // its release.
        assert_eq!(frame(&mut app, 2.75), [Command::Key(Key::Forward, true)]);
        // View held: the end after a second (four frames of 0.25 s).
        assert_eq!(frame(&mut app, 3.0), [Command::Key(Key::Forward, false)]);
        for t in [3.25, 3.5] {
            assert!(frame(&mut app, t).is_empty(), "{t}");
        }
        assert_eq!(frame(&mut app, 3.75), [Command::End]);
        let p = &app.world().resource::<PersonInput>().person;
        assert!(p.ended && p.held == Held::default());
    }

    #[test]
    fn shift_runs_and_the_readout_says_so_in_the_input_system() {
        let mut app = InputApp::new(true);
        app.frames(10);
        assert!(app.walker().character.grounded);
        let p = fo4_walk::Params::PROVISIONAL;
        app.keys().press(KeyCode::KeyW);
        let x0 = app.fly().state.eye[0];
        app.frames(60);
        let walked = app.fly().state.eye[0] - x0;
        assert!((walked - p.walk_speed).abs() < 1e-3, "{walked}");
        let text = app.0.world().resource::<Readout>().text.clone();
        assert!(
            text.contains("| walk | walk 100 u/s, moving 100 u/s"),
            "{text}"
        );
        app.keys().press(KeyCode::ShiftLeft);
        let x1 = app.fly().state.eye[0];
        app.frames(60);
        let ran = app.fly().state.eye[0] - x1;
        assert!((ran - p.run_speed).abs() < 1e-3, "{ran}");
        let text = app.0.world().resource::<Readout>().text.clone();
        assert!(text.contains("run 350 u/s, moving 350 u/s"), "{text}");
        assert!(text.contains("on ground"), "{text}");
        // R returns to the start pose, walking.
        app.keys().release(KeyCode::KeyW);
        app.keys().release(KeyCode::ShiftLeft);
        app.keys().press(KeyCode::KeyR);
        app.frame();
        let eye = app.fly().state.eye;
        assert!(eye[0].abs() < 1e-9 && eye[1].abs() < 1e-9, "{eye:?}");
        assert!(app.0.world().resource::<Walk>().active.is_some());
        // Tab flies; Shift is then the fast fly speed.
        app.keys().press(KeyCode::Tab);
        app.frame();
        assert!(app.0.world().resource::<Walk>().active.is_none());
        app.keys().press(KeyCode::KeyW);
        app.keys().press(KeyCode::ShiftRight);
        let x2 = app.fly().state.eye[0];
        app.frames(30);
        let flown = app.fly().state.eye[0] - x2;
        assert!(
            (flown - 0.5 * crate::cli::DEFAULT_FLY.fast_speed).abs() < 1e-3,
            "{flown}"
        );
        let text = app.0.world().resource::<Readout>().text.clone();
        assert!(
            text.starts_with("fo4-view | fly | 1500 u/s (Shift)"),
            "{text}"
        );
    }

    #[test]
    fn the_walker_waits_over_a_cell_that_has_not_arrived() {
        let mut app = InputApp::new(true);
        let (centres, sent) = std::sync::mpsc::channel();
        let (_messages, received) = std::sync::mpsc::channel();
        app.0.insert_resource(Stream::new(centres, received));
        app.keys().press(KeyCode::KeyW);
        let feet = app.walker().character.feet;
        app.frames(30);
        assert_eq!(app.walker().character.feet, feet);
        let text = app.0.world().resource::<Readout>().text.clone();
        assert!(text.contains("| waiting for cell 0,0"), "{text}");
        assert!(text.contains("| cells 0 (0 uploading)"), "{text}");
        // The cell arrives (here without collision; the floor is there).
        let piece = crate::cells::CellPiece {
            grid: [0, 0],
            collision: None,
            meshes: Vec::new(),
            textures: Vec::new(),
            new_meshes: Vec::new(),
            new_textures: Vec::new(),
            surfaces: Vec::new(),
            instances: Vec::new(),
            report: crate::cells::LoadReport::default(),
        };
        app.0
            .world_mut()
            .resource_scope(|world, mut stream: Mut<Stream>| {
                let mut walk = world.resource_mut::<Walk>();
                stream.live.receive(piece, &mut walk.space).unwrap();
            });
        app.frames(60);
        let moved = app.walker().character.feet[0] - feet[0];
        assert!(moved > 90.0, "{moved}");
        let text = app.0.world().resource::<Readout>().text.clone();
        assert!(!text.contains("waiting"), "{text}");
        drop(sent);
    }

    #[test]
    fn dragging_with_the_left_button_turns_the_walker_and_its_walk() {
        let rpp = crate::cli::DEFAULT_FLY.radians_per_pixel;
        let mut app = InputApp::new(true);
        app.frames(5);
        let yaw0 = app.fly().state.yaw;
        // Motion without the button does not turn.
        app.0
            .world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(100., 0.);
        app.frame();
        assert_eq!(app.fly().state.yaw, yaw0);
        // Raw device motion with the button held.
        app.0
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.0
            .world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(100., 0.);
        app.frame();
        let turned = (yaw0 - app.fly().state.yaw).rem_euclid(std::f64::consts::TAU);
        assert!((turned - 100. * rpp).abs() < 1e-6, "{turned}");
        // Cursor motion alone (no raw motion), as synthetic drags produce.
        app.0.world_mut().write_message(CursorMoved {
            window: Entity::PLACEHOLDER,
            position: Vec2::new(10., 10.),
            delta: Some(Vec2::new(50., 0.)),
        });
        app.frame();
        let turned = (yaw0 - app.fly().state.yaw).rem_euclid(std::f64::consts::TAU);
        assert!((turned - 150. * rpp).abs() < 1e-6, "{turned}");
        assert_eq!(app.walker().look.yaw, app.fly().state.yaw);
        // The walk follows the new heading.
        let before = app.fly().state.eye;
        app.keys().press(KeyCode::KeyW);
        app.frames(60);
        let after = app.fly().state.eye;
        let yaw = app.fly().state.yaw;
        let moved = [after[0] - before[0], after[1] - before[1]];
        assert!((moved[0] - 100. * yaw.cos()).abs() < 1e-3, "{moved:?}");
        assert!((moved[1] - 100. * yaw.sin()).abs() < 1e-3, "{moved:?}");
        assert_eq!(look_delta(Vec2::new(1., 2.), Vec2::new(5., 5.)), [1., 2.]);
        assert_eq!(look_delta(Vec2::ZERO, Vec2::new(5., -5.)), [5., -5.]);
    }
}
