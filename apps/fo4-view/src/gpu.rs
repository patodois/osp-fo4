//! Bevy side: the unlit surface material, the reference projection, scene
//! upload, one offscreen frame, and the windowed free-fly viewer.
use crate::{
    coords,
    fly::{FlyInput, FlySettings, FlyState},
    scene::GpuScene,
    walk::WalkView,
};
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
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
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
        view::screenshot::{Screenshot, ScreenshotCaptured},
        RenderApp, RenderPlugin,
    },
    shader::ShaderRef,
    window::{ExitCondition, PrimaryWindow},
    winit::WinitPlugin,
};
use fo4_scene::camera::Camera as CpuCamera;
use std::sync::{Arc, Mutex};

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
    pub tag: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialKey {
    pub surface: crate::scene::SurfaceKey,
    /// Instance-tag pass: no blending, depth written, tag bytes as colour.
    pub tag_pass: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(MaterialKey)]
pub struct SurfaceMaterial {
    #[uniform(0)]
    pub params: SurfaceUniform,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
    pub key: MaterialKey,
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

fn upload(world: &mut World, scene: &GpuScene) -> Uploaded {
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let meshes = scene
        .meshes
        .iter()
        .map(|m| {
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
            .with_inserted_indices(Indices::U16(m.indices.clone()));
            if let Some(colors) = &m.colors {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone());
            }
            meshes.add(mesh)
        })
        .collect();
    let mut images = world.resource_mut::<Assets<Image>>();
    let images = scene
        .textures
        .iter()
        .map(|t| {
            let mut image = Image::new_uninit(
                Extent3d {
                    width: t.width,
                    height: t.height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Rgba16Float,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.texture_descriptor.mip_level_count = t.levels;
            image.data = Some(t.bytes.clone());
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
            images.add(image)
        })
        .collect();
    Uploaded { meshes, images }
}

fn spawn(world: &mut World, scene: &GpuScene, up: &Uploaded, tag_pass: bool, layer: usize) {
    let mut materials = world.resource_mut::<Assets<SurfaceMaterial>>();
    let mut handles = std::collections::BTreeMap::new();
    let mut entities = Vec::with_capacity(scene.instances.len());
    for instance in &scene.instances {
        let surface = &scene.surfaces[instance.surface];
        // The tag pass needs the tag in the material, so it shares per
        // (surface, tag); the colour pass shares per surface.
        let tag = if tag_pass { instance.tag } else { 0 };
        let material = handles
            .entry((instance.surface, tag))
            .or_insert_with(|| {
                let p = surface.params;
                materials.add(SurfaceMaterial {
                    params: SurfaceUniform {
                        uv_scale: Vec2::from_array(p.uv_scale),
                        uv_offset: Vec2::from_array(p.uv_offset),
                        test_ref: p.test_ref,
                        opacity: p.opacity,
                        test_function: p.test_function,
                        skip_zero_alpha: u32::from(p.skip_zero_alpha),
                        tag,
                    },
                    texture: up.images[surface.texture].clone(),
                    key: MaterialKey {
                        surface: surface.key,
                        tag_pass,
                    },
                })
            })
            .clone();
        entities.push((
            Mesh3d(up.meshes[instance.mesh].clone()),
            MeshMaterial3d(material),
            Transform {
                translation: instance.translation,
                rotation: instance.rotation,
                scale: instance.scale,
            },
            // The CPU draws every triangle and clips; no culling by bounds.
            NoFrustumCulling,
            RenderLayers::layer(layer),
        ));
    }
    world.spawn_batch(entities);
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

/// One offscreen frame of `camera`. Renders until no pipeline is waiting,
/// then captures until two consecutive frames are byte-identical, so a frame
/// with an asset or pipeline still loading is never returned.
pub fn headless(
    scene: &GpuScene,
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

    let world = app.world_mut();
    let up = upload(world, scene);
    let mut targets = Vec::new();
    let passes: &[bool] = if want_tags { &[false, true] } else { &[false] };
    for (k, &tag_pass) in passes.iter().enumerate() {
        spawn(world, scene, &up, tag_pass, k);
        let mut target = Image::new_target_texture(width, height, TextureFormat::Rgba8Unorm, None);
        target.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let target = world.resource_mut::<Assets<Image>>().add(target);
        // The tag pass clears to all-ones bytes: u32::MAX, the CPU background
        // tag. On this linear (Rgba8Unorm) target a linear colour is stored
        // as is, like the CPU frame's clear value.
        let [r, g, b, a] = if tag_pass { [1.; 4] } else { clear };
        let clear = Color::LinearRgba(LinearRgba::new(r, g, b, a));
        world.spawn((
            camera_bundle(pose, camera.frustum, false, clear, k as isize, k),
            RenderTarget::Image(target.clone().into()),
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
        let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        if let Some(e) = cache.pipelines().find_map(|p| match &p.state {
            CachedPipelineState::Err(e) => Some(e.to_string()),
            _ => None,
        }) {
            return Err(format!("PipelineError: {e}"));
        }
        ready = if cache.waiting_pipelines().next().is_none() {
            ready + 1
        } else {
            0
        };
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
/// Walk mode: the cell's walking world and, while walking, the walker.
#[derive(Resource)]
struct Walk {
    world: fo4_walk::World,
    active: Option<WalkView>,
}
#[derive(Resource)]
struct PendingScene(Option<GpuScene>, [f32; 4]);
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

fn spawn_window_scene(world: &mut World) {
    let Some(mut pending) = world.get_resource_mut::<PendingScene>() else {
        return;
    };
    let clear = pending.1;
    let Some(scene) = pending.0.take() else {
        return;
    };
    let up = upload(world, &scene);
    spawn(world, &scene, &up, false, 0);
    // The window's swap chain is expected to be sRGB (the shader checks the
    // real format; the clear colour cannot): an sRGB colour with the CPU's
    // clear values is stored as those values.
    let clear = Color::Srgba(Srgba::new(clear[0], clear[1], clear[2], clear[3]));
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
            if w.world.bounds.is_none() {
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
    mut readout_text: ResMut<Readout>,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    let cursor_delta: Vec2 = cursor.read().filter_map(|m| m.delta).sum();
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
    let dt = time.delta_secs_f64();
    let restart = keys.just_pressed(KeyCode::KeyR);
    if restart {
        fly.state = fly.start;
    }
    let mut walk = walk;
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
            } else if keys.just_pressed(KeyCode::Tab) {
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
                    view.step(&walk.world, input, fly.settings, dt);
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
    readout_text.text = readout(&fly, walk.as_deref(), input);
    readout_text.since += dt;
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
pub fn window(
    scene: GpuScene,
    start: FlyState,
    settings: FlySettings,
    walk: Option<fo4_walk::World>,
    clear: [f32; 4],
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
            Some(Walk {
                world,
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
    .insert_resource(PendingScene(Some(scene), clear))
    .add_systems(Startup, spawn_window_scene)
    .add_systems(Update, (fly_camera, show_readout).chain())
    .run();
    Ok(())
}

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
                    world: floor,
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
