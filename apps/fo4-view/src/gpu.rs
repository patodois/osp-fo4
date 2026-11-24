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
        app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default());
    }
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
    }
}

/// The material of `surface` drawing `image`, with its own pipeline when
/// `rank` is not 0 ([`MaterialKey::rank`]).
fn surface_material(
    surface: &Surface,
    image: Handle<Image>,
    palette: Option<Handle<Image>>,
    lights: Handle<ShaderBuffer>,
    tag_pass: bool,
    rank: u32,
) -> SurfaceMaterial {
    SurfaceMaterial {
        params: uniform(&surface.params),
        texture: image,
        palette,
        lights,
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
) {
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
        },
        texture: image,
        palette: None,
        lights,
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
    world.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(eye),
        NoFrustumCulling,
        RenderLayers::layer(layer),
    ));
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
        world.spawn((
            camera_bundle(pose, camera.frustum, false, clear, k as isize, k),
            RenderTarget::Image(target.clone().into()),
            // Indirect drawing orders a batch's draws on the GPU, which
            // changes exact depth ties from run to run.
            bevy::render::view::NoIndirectDrawing,
        ));
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
    let entities = spawn(world, &scene, &up, false, 0, false);
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
    world.spawn((camera_bundle(pose, frustum, true, clear, 0, 0), FlyCamera));
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
    );
    if let Some(animation) = clip {
        app.insert_resource(PendingClip(animation))
            .add_systems(Update, crate::clip::animate.after(spawn_window_scene));
    }
    if let Some(t) = time {
        app.insert_resource(PendingClock(t))
            .add_systems(Update, advance_clock.after(spawn_window_scene));
    }
    #[cfg(feature = "script-event")]
    if let Some(p) = play {
        let scripted = p.scripted();
        app.insert_resource(Playing(p, String::new(), PlayDrawn::default()));
        if scripted {
            app.add_systems(Update, play_scripted.after(fly_camera).before(show_readout));
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

/// The live world's generation last applied to the window
/// (`--play-live-world`), and each drawn reference instance's translation
/// as spawned.
#[cfg(feature = "script-event")]
#[derive(Default)]
struct PlayDrawn {
    applied: Option<u64>,
    spawned: std::collections::HashMap<Entity, Vec3>,
    /// The scripted run: frames to wait before a loaded cell's upload is
    /// read (its scene resource arrives through the commands), with the
    /// cell's load.
    uploading: Option<(u32, crate::play::Loaded)>,
    /// The scripted run's frame captures (`--play-shots`).
    shots: PlayShots,
}

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
    mut instances: Query<(Entity, &MeshTag, &mut Visibility, &mut Transform), Without<FlyCamera>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Playing(p, title, drawn) = &mut *playing;
    match p.loaded() {
        Some(Ok(r)) => {
            let n = instances.iter().count();
            for (e, ..) in instances.iter() {
                commands.entity(e).despawn();
            }
            println!(
                "play: cell {} loaded in {:.2} s (asked at {:.3} s of the run); {n} drawn instances of the cell before removed",
                fo4_scene::timeline::key_text(&r.cell),
                r.load_seconds,
                r.asked_at,
            );
            let (gpu, clear) = (r.gpu, r.clear);
            commands.insert_resource(PendingScene(gpu, clear, None));
            drawn.spawned.clear();
            drawn.applied = None;
            drawn.uploading = Some((
                1,
                crate::play::Loaded {
                    cell: r.cell,
                    load_seconds: r.load_seconds,
                    started: std::time::Instant::now(),
                },
            ));
        }
        Some(Err(e)) => {
            eprintln!("play: loading the walker's cell: {e}");
            std::process::exit(2);
        }
        None => {}
    }
    let mut ready = pending.is_none() && !p.loading();
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
            p.load(cell, seen.t);
            ready = false;
        }
    }
    // The window's clock starts once the walker stands somewhere.
    let dt = time.delta_secs_f64().min(PLAY_FRAME_MAX);
    let go = ready && seen.feet.is_some();
    if let Some(t) = p.scripted_frame(if go { dt } else { 0.0 }) {
        *title = t;
    }
    if let Some(feet) = seen.feet {
        let state = FlyState {
            eye: [
                feet[0],
                feet[1],
                feet[2] + fo4_walk::Params::PROVISIONAL.eye_height,
            ],
            yaw: seen.heading.map_or(fly.state.yaw, f64::to_radians),
            pitch: seen
                .pitch
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
    if drawn.uploading.is_none() {
        play_live(p, drawn, None, &mut instances);
    }
    // --play-shots: the frame each time the run passes a multiple of the
    // period, once the scene is drawn.
    let shots = &mut drawn.shots;
    if let Some((dir, every)) = p.shots() {
        if let Some((at, s)) = &shots.asked {
            let image = shots.captured.lock().ok().and_then(|mut c| c.take());
            if let Some(image) = image {
                let file = format!("{dir}/t{at:07.1}.png");
                let root = p.data_root().map(std::path::Path::new);
                match capture_png(image).and_then(|(png, size)| {
                    use std::io::Write;
                    crate::load::output_file(&file, root)
                        .map_err(|e| e.to_string())?
                        .write_all(&png)
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
                shots.asked = None;
                shots.written += 1;
            }
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
            shots.next = ((seen.t / every).floor() + 1.0) * every;
        }
    }
    if let Some(done) = &seen.done {
        let shot_due = p.shots().is_some() && (shots.asked.is_some() || !shots.last);
        if !shot_due && drawn.uploading.is_none() {
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
                let h = world
                    .resource_mut::<Assets<SurfaceMaterial>>()
                    .add(surface_material(surface, image, palette, lights, false, 0));
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
    pub frame_log: Option<std::fs::File>,
    pub script: Option<Script>,
}
impl Aids {
    fn add_to(self, app: &mut App) {
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
    let png = crate::load::png_rgba8(size, &rgba).map_err(|e| e.to_string())?;
    Ok((png, size))
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
