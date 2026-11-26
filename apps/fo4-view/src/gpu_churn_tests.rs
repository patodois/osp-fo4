//! The moving grid's drawing side on the GPU, with synthetic cells only:
//! cells spawned, dropped and spawned again as the window does
//! ([`super::upload_waiting`], [`Live::drop_cell`]) must draw the same
//! frame as when they were first spawned. Offscreen, no window; the test
//! that draws needs a GPU, so it is ignored by default (`cargo test --
//! --ignored`); the others need none.
use super::*;
use crate::cells::{CellPiece, LoadReport, SharedCredits};
use crate::scene::{SurfaceKey, SurfaceParams, TEST_ALWAYS};
use crate::stream::{mesh_key, texture_key};
use bevy::render::render_resource::BlendFactor;
use std::collections::BTreeMap;

const SIZE: [u32; 2] = [320, 180];

/// A flat quad of side `side` in the game's XY plane, with vertex colours
/// whose alpha is `alpha` (the terrain overlays' weight).
fn quad(side: f32, alpha: f32, colors: bool) -> GpuMesh {
    let h = side / 2.;
    GpuMesh {
        positions: vec![[-h, -h, 0.], [h, -h, 0.], [h, h, 0.], [-h, h, 0.]],
        uvs: vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
        colors: colors.then(|| vec![[1., 1., 1., alpha]; 4]),
        normals: None,
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// A 4 x 4 decoded texture of one colour.
fn texture(rgba: [f32; 4]) -> GpuTexture {
    let mut bytes = Vec::new();
    for _ in 0..16 {
        for v in rgba {
            bytes.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
        }
    }
    GpuTexture {
        width: 4,
        height: 4,
        levels: 1,
        format: TexelFormat::Rgba16Float,
        opaque: false,
        bytes,
    }
}

fn surface(texture: usize, blend: Option<(BlendFactor, BlendFactor)>, overlay: bool) -> Surface {
    Surface {
        texture,
        palette: None,
        params: SurfaceParams {
            uv_scale: [1.; 2],
            uv_offset: [0.; 2],
            test_function: TEST_ALWAYS,
            test_ref: 0.,
            opacity: 1.,
            skip_zero_alpha: false,
            opaque_texture: false,
            falloff: None,
            palette: 0,
            colour: None,
            lit: false,
            smooth: false,
        },
        key: SurfaceKey {
            blend,
            cull_back: false,
            overlay,
        },
    }
}

/// One synthetic cell at grid `g` (cells 256 units apart): an opaque
/// ground quad, an additive overlay on it (as a terrain texture pass),
/// small opaque and blended quads standing on it. Meshes and textures are
/// shared between cells by content, as real cells share models.
fn cell(g: [i32; 2]) -> (Vec<GpuMesh>, Vec<GpuTexture>, Vec<Surface>, Vec<Instance>) {
    let meshes = vec![
        // Ground and its overlay: equal positions, as a terrain's passes.
        quad(256., 1., true),
        quad(256., 0.5, true),
        // A shared small quad, and one only some cells draw.
        quad(24., 1., false),
        quad(16. + 8. * (g[0].rem_euclid(3)) as f32, 1., false),
    ];
    // The ground's overlay and the blended quads have a texture of their
    // own in each cell; the others are shared.
    let own = |k: i32| ((g[0] + 8) * 17 + (g[1] + 8) + k).rem_euclid(256) as f32 / 256.;
    let textures = vec![
        texture([0.4, 0.3, 0.2, 1.]),
        texture([0.1, 0.4, own(0), 1.]),
        texture([0.8, 0.8, 0.7, 1.]),
        texture([0.2, own(1), 0.9, 0.5]),
    ];
    let additive = Some((BlendFactor::SrcAlpha, BlendFactor::One));
    let blended = Some((BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha));
    let surfaces = vec![
        surface(0, None, false),
        surface(1, additive, true),
        surface(2, None, false),
        surface(3, blended, false),
    ];
    let at = |x: f64, y: f64, z: f64| {
        let t = fo4_scene::transform::Transform {
            translation: [x, y, z],
            ..fo4_scene::transform::Transform::IDENTITY
        };
        coords::instance_transform(&t).unwrap()
    };
    let (cx, cy) = (f64::from(g[0]) * 256., f64::from(g[1]) * 256.);
    let mut instances = Vec::new();
    let mut push = |mesh: usize, surface: usize, (translation, rotation, scale)| {
        instances.push(Instance {
            mesh,
            surface,
            tag: instances.len() as u32,
            translation,
            rotation,
            scale,
        })
    };
    push(0, 0, at(cx, cy, 0.));
    push(1, 1, at(cx, cy, 0.));
    for k in 0..48 {
        let (dx, dy) = (f64::from(k % 8) * 28. - 98., f64::from(k / 8) * 36. - 90.);
        let z = 2. + f64::from(k);
        push(2 + k as usize % 2, 2, at(cx + dx, cy + dy, z));
        push(2, 3, at(cx + dx + 12., cy + dy + 9., z + 30.));
    }
    (meshes, textures, surfaces, instances)
}

/// The loading side of the test: which keys travel with a cell, as
/// [`crate::cells::Streamer`] decides.
struct Loading {
    root: fo4_core::Budget,
    shared: SharedCredits,
    kept: BTreeMap<[i32; 2], Vec<AssetKey>>,
}
impl Loading {
    fn new() -> Self {
        Self {
            root: fo4_core::Budget::new(64 << 20),
            shared: SharedCredits::default(),
            kept: BTreeMap::new(),
        }
    }
    fn load(&mut self, g: [i32; 2]) -> CellPiece {
        let (meshes, textures, surfaces, instances) = cell(g);
        let mut keys = Vec::new();
        let mut piece = CellPiece {
            grid: g,
            collision: None,
            meshes: Vec::new(),
            textures: Vec::new(),
            new_meshes: Vec::new(),
            new_textures: Vec::new(),
            surfaces,
            instances,
            report: LoadReport::default(),
        };
        for m in meshes {
            let k = mesh_key(&m);
            piece.meshes.push(k);
            if self.shared.hold(&self.root, &mut keys, k).unwrap() {
                piece.new_meshes.push((k, m));
            }
        }
        for t in textures {
            let k = texture_key(&t);
            piece.textures.push(k);
            if self.shared.hold(&self.root, &mut keys, k).unwrap() {
                piece.new_textures.push((k, t));
            }
        }
        self.kept.insert(g, keys);
        piece
    }
    fn drop(&mut self, g: [i32; 2]) {
        if let Some(keys) = self.kept.remove(&g) {
            self.shared.release(&keys);
        }
    }
}

/// An offscreen app with the moving grid window's plugins and a camera
/// above the cells around the origin.
fn app(pbr: bevy::pbr::PbrPlugin) -> (App, Handle<Image>) {
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .set(pbr)
        .disable::<WinitPlugin>()
        // The render world in this thread, as for `headless`.
        .disable::<PipelinedRenderingPlugin>();
    app.add_plugins(plugins).add_plugins(ViewerPlugin);
    app.finish();
    app.cleanup();
    let camera = fo4_scene::camera::look_at(
        [0., -700., 500.],
        [0., 0., 0.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let pose = coords::camera_pose(&camera).unwrap();
    let world = app.world_mut();
    let mut target = Image::new_target_texture(SIZE[0], SIZE[1], TextureFormat::Rgba8Unorm, None);
    target.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    world.spawn((
        camera_bundle(pose, camera.frustum, false, Color::BLACK, 0, 0),
        RenderTarget::Image(target.clone().into()),
    ));
    (app, target)
}

fn frame(app: &mut App) {
    app.update();
    app.world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .unwrap();
}

/// Frames until no pipeline has waited for three frames in a row (as
/// `headless` does: the shaders load on task pools that parallel tests
/// share, so the number of frames is not fixed), then the target's bytes
/// once two captures in a row agree.
fn capture(app: &mut App, target: &Handle<Image>) -> Vec<u8> {
    let mut ready = 0;
    for _ in 0..600 {
        frame(app);
        ready = if crate::gpu::pipelines_ready(app).unwrap() {
            ready + 1
        } else {
            0
        };
        if ready >= 3 {
            break;
        }
    }
    assert!(ready >= 3, "pipelines still waiting after 600 frames");
    let mut previous: Option<Vec<u8>> = None;
    for _ in 0..20 {
        let slot: Arc<Mutex<Option<Vec<u8>>>> = Arc::default();
        let s = slot.clone();
        app.world_mut()
            .spawn(Screenshot::image(target.clone()))
            .observe(move |shot: On<ScreenshotCaptured>| {
                *s.lock().unwrap() = shot.event().image.data.clone();
            });
        let bytes = loop {
            frame(app);
            if let Some(b) = slot.lock().unwrap().take() {
                break b;
            }
        };
        if previous.as_ref() == Some(&bytes) {
            return bytes;
        }
        previous = Some(bytes);
    }
    panic!("no two identical captures in a row");
}

/// Applies a drop as the window does: data no cell uses any more is freed
/// and the cell's entities despawned.
fn drop_cell(app: &mut App, live: &mut Live, loading: &mut Loading, g: [i32; 2]) {
    loading.drop(g);
    let world = app.world_mut();
    for e in live.drop_cell(g, &mut fo4_walk::WorldSet::new()) {
        world.despawn(e);
    }
}

fn load_cell(app: &mut App, live: &mut Live, loading: &mut Loading, g: [i32; 2]) {
    let piece = loading.load(g);
    let world = app.world_mut();
    for e in live.receive(piece, &mut fo4_walk::WorldSet::new()).unwrap() {
        world.despawn(e);
    }
    upload_waiting(world, live).unwrap();
    assert!(live.waiting.is_empty());
}

/// Walks a 3 x 3 grid east and back (load within 1, drop past 2, as
/// `--around 1`), a frame drawn after every grid change, and returns the
/// frames at the start and back at the start, with their cells.
fn round_trip(pbr: bevy::pbr::PbrPlugin) -> (Vec<u8>, Vec<u8>) {
    let (mut app, target) = app(pbr);
    let mut live = Live::default();
    let mut loading = Loading::new();
    let policy = crate::stream::Policy::around(2);
    let mut centre = [0, 0];
    let step = |app: &mut App, live: &mut Live, loading: &mut Loading, c: [i32; 2]| {
        let plan = crate::stream::plan(c, live.cells.keys(), policy);
        for g in plan.drop {
            drop_cell(app, live, loading, g);
        }
        for g in plan.load {
            load_cell(app, live, loading, g);
        }
        capture(app, &target);
    };
    step(&mut app, &mut live, &mut loading, centre);
    let start = capture(&mut app, &target);
    let start_cells: Vec<[i32; 2]> = live.cells.keys().copied().collect();
    let route = [
        [1, 0],
        [2, 0],
        [3, 0],
        [4, 0],
        [4, -1],
        [4, -2],
        [4, -1],
        [4, -2],
        [3, -2],
        [2, -2],
        [1, -1],
        [0, -1],
        [-1, -1],
        [-1, 0],
        [0, 0],
        [1, 0],
        [0, 0],
        [0, 1],
        [0, 2],
        [0, 3],
        [0, 2],
        [0, 1],
        [0, 0],
    ];
    for c in route {
        centre = c;
        step(&mut app, &mut live, &mut loading, centre);
    }
    // Drop what the start did not hold, so the same cells are drawn.
    let extra: Vec<[i32; 2]> = live
        .cells
        .keys()
        .copied()
        .filter(|g| !start_cells.contains(g))
        .collect();
    for g in extra {
        drop_cell(&mut app, &mut live, &mut loading, g);
    }
    assert_eq!(live.cells.keys().copied().collect::<Vec<_>>(), start_cells);
    let back = capture(&mut app, &target);
    (start, back)
}

#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cells_spawned_again_draw_the_same_frame() {
    let (start, back) = round_trip(stream_pbr());
    assert!(start.iter().any(|b| *b != 0), "nothing drawn");
    let differing = start
        .as_chunks::<4>()
        .0
        .iter()
        .zip(back.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 0,
        "{differing} pixels differ after the round trip"
    );
}

/// Without a GPU: the window's spawning gives equal surfaces of different
/// cells one material, keeps it while any spawned cell draws it, and gives
/// a cell spawned again a material equal to its first one.
#[test]
fn equal_surfaces_share_one_material_while_any_cell_draws_them() {
    let mut world = World::new();
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<SurfaceMaterial>::default());
    let mut live = Live::default();
    let mut loading = Loading::new();
    let mut space = fo4_walk::WorldSet::new();
    let ground = |world: &mut World, live: &Live, g: [i32; 2]| {
        let e = live.cells[&g].entities[0];
        world
            .get::<MeshMaterial3d<SurfaceMaterial>>(e)
            .unwrap()
            .0
            .clone()
    };
    for g in [[0, 0], [1, 0]] {
        live.receive(loading.load(g), &mut space).unwrap();
    }
    upload_waiting(&mut world, &mut live).unwrap();
    assert!(live.waiting.is_empty());
    // Ground (shared texture) and small quads share; each cell's overlay
    // and blended quads have textures of their own.
    assert_eq!(live.materials.len(), 2 + 2 * 2);
    let a = ground(&mut world, &live, [0, 0]);
    assert_eq!(a, ground(&mut world, &live, [1, 0]));
    let first = world
        .resource::<Assets<SurfaceMaterial>>()
        .get(&a)
        .cloned()
        .unwrap();
    // Every instance of a cell draws the material of its surface.
    for g in [[0, 0], [1, 0]] {
        let cell = &live.cells[&g];
        for (e, i) in cell.entities.iter().zip(&cell.instances) {
            let m = &world.get::<MeshMaterial3d<SurfaceMaterial>>(*e).unwrap().0;
            assert_eq!(Some(m), live.materials.get(&cell.surface_ids[i.surface]));
        }
    }
    // Dropping one cell keeps what the other draws.
    loading.drop([0, 0]);
    for e in live.drop_cell([0, 0], &mut space) {
        world.despawn(e);
    }
    assert_eq!(live.materials.len(), 2 + 2);
    assert_eq!(live.materials.users(&live.cells[&[1, 0]].surface_ids[0]), 1);
    assert_eq!(ground(&mut world, &live, [1, 0]), a);
    // Dropped and spawned again: the same material, by content.
    loading.drop([1, 0]);
    for e in live.drop_cell([1, 0], &mut space) {
        world.despawn(e);
    }
    assert!(live.materials.is_empty() && live.meshes.is_empty() && live.textures.is_empty());
    live.receive(loading.load([0, 0]), &mut space).unwrap();
    upload_waiting(&mut world, &mut live).unwrap();
    let again = ground(&mut world, &live, [0, 0]);
    let again = world
        .resource::<Assets<SurfaceMaterial>>()
        .get(&again)
        .cloned()
        .unwrap();
    assert_eq!(again.key, first.key);
    assert_eq!(format!("{:?}", again.params), format!("{:?}", first.params));
}

#[test]
fn the_moving_grid_builds_instance_data_on_the_cpu() {
    assert!(!stream_pbr().use_gpu_instance_buffer_builder);
}

/// Without a GPU: headless spawning gives every surface a material and a
/// pipeline rank of its own, in surface order, in both passes, and every
/// instance its tag as a `MeshTag`; the window's spawning ranks nothing.
/// Before this, the tag pass made one material per tag and the colour pass
/// shared pipelines, so the order of draws at exact depth ties followed the
/// material slots, which Bevy assigns in a hash order of asset ids that
/// changes with the build.
#[test]
fn headless_surfaces_have_pipelines_of_their_own_in_surface_order() {
    let (meshes, textures, surfaces, instances) = cell([0, 0]);
    let mut world = World::new();
    world.insert_resource(Assets::<SurfaceMaterial>::default());
    let images: Vec<Handle<Image>> = textures.iter().map(|_| Handle::default()).collect();
    let meshes: Vec<Handle<Mesh>> = meshes.iter().map(|_| Handle::default()).collect();
    for (tag_pass, ranked) in [(false, true), (true, true), (false, false)] {
        let before = world.resource::<Assets<SurfaceMaterial>>().len();
        let entities = spawn_instances(
            &mut world, &surfaces, &instances, &meshes, &images, tag_pass, 0, ranked,
        );
        assert_eq!(
            world.resource::<Assets<SurfaceMaterial>>().len() - before,
            surfaces.len()
        );
        for (e, i) in entities.iter().zip(&instances) {
            let handle = world
                .get::<MeshMaterial3d<SurfaceMaterial>>(*e)
                .unwrap()
                .0
                .clone();
            let m = world
                .resource::<Assets<SurfaceMaterial>>()
                .get(&handle)
                .unwrap();
            let rank = if ranked { i.surface as u32 + 1 } else { 0 };
            assert_eq!((m.key.rank, m.key.tag_pass), (rank, tag_pass));
            assert_eq!(world.get::<MeshTag>(*e).unwrap().value, i.tag);
        }
    }
}

/// Two coplanar quads of two surfaces: the frame is the same in every run,
/// and the surface whose instance comes first in the list wins the exact
/// depth tie, as in the CPU frame. Needs a GPU.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn exact_depth_ties_resolve_by_list_order_in_every_run() {
    let red = [1., 0., 0., 1.];
    let blue = [0., 0., 1., 1.];
    let scene = |first: [f32; 4], second: [f32; 4]| {
        let at = coords::instance_transform(&fo4_scene::transform::Transform::IDENTITY).unwrap();
        let instance = |surface: usize| Instance {
            mesh: 0,
            surface,
            tag: surface as u32,
            translation: at.0,
            rotation: at.1,
            scale: at.2,
        };
        GpuScene {
            meshes: vec![quad(400., 1., false)],
            draw_meshes: BTreeMap::new(),
            textures: vec![texture(first), texture(second)],
            surfaces: vec![surface(0, None, false), surface(1, None, false)],
            instances: vec![instance(0), instance(1)],
            animated: Vec::new(),
            moving: Vec::new(),
            draws: Vec::new(),
            platforms: Vec::new(),
            doors: Vec::new(),
            leaves: Vec::new(),
            skins: Vec::new(),
            light: None,
            view_exposure: None,
            cast_places: Vec::new(),
            cast_space: None,
            cast_ride: false,
            cast_keep: false,
        }
    };
    let camera = fo4_scene::camera::look_at(
        [0., -300., 400.],
        [0., 0., 0.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let centre = |f: &Frames| {
        let at = ((SIZE[1] / 2 * SIZE[0] + SIZE[0] / 2) * 4) as usize;
        f.color[at..at + 3].to_vec()
    };
    let a = headless(&mut scene(red, blue), &camera, [0., 0., 0., 1.], true).unwrap();
    let b = headless(&mut scene(red, blue), &camera, [0., 0., 0., 1.], true).unwrap();
    assert_eq!(a.color, b.color);
    assert_eq!(a.tags, b.tags);
    assert_eq!(centre(&a), [255, 0, 0]);
    let c = headless(&mut scene(blue, red), &camera, [0., 0., 0., 1.], true).unwrap();
    assert_eq!(centre(&c), [0, 0, 255]);
}

/// An effect scene of the draw list's own terms, drawn by the CPU
/// reference and by the GPU path: a grey opaque floor, a blended card
/// whose vertex normals turn away from the eye toward its edges (falloff),
/// and an additive card with both palette lookups and vertex colours above
/// 1 (the clamp). The frames agree within the filtering differences of the
/// earlier comparisons; each term changes the frame where it applies.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_falloff_palette_colour_alpha_clamp_and_vertex_alpha() {
    use fo4_render::{Alpha, Draw, DrawList, Falloff, Mesh, Palette, Sidedness, Texture, Vertex};
    let quad = |h: f64, z: f64| -> Vec<Vertex> {
        [[-h, -h], [h, -h], [h, h], [-h, h]]
            .iter()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
            .map(|(p, uv)| Vertex {
                position: [p[0], p[1], z],
                uv,
            })
            .collect()
    };
    let texture = |width: u32, height: u32, texel: &dyn Fn(u32, u32) -> [f32; 4]| Texture {
        mips: Vec::new(),
        width,
        height,
        rgba: (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| texel(x, y)))
            .collect(),
        mip: 0,
        srgb: false,
    };
    let alpha = |enabled: u8, source: u32, destination: u32, opacity: f32| Alpha {
        enabled,
        source,
        destination,
        test_enabled: 0,
        test_ref: 0,
        nif_test_function: Some(4),
        opacity,
        nif_flags: None,
    };
    let draw = |instance: u32, mesh: usize, texture: usize, alpha: Alpha| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh,
        texture,
        world: fo4_scene::transform::Transform::IDENTITY,
        alpha,
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let tri = vec![[0, 1, 2], [0, 2, 3]];
    let mut list = DrawList {
        meshes: vec![
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad(400., 0.),
                triangles: tri.clone(),
            },
            // Normals tilted outward at the corners: the falloff fades
            // the card toward them.
            Mesh {
                vertex_colors: None,
                normals: Some(
                    [[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]
                        .map(|[x, y]| [x * 0.9, y * 0.9, 0.45])
                        .to_vec(),
                ),
                vertices: quad(120., 40.)
                    .into_iter()
                    .map(|v| Vertex {
                        position: [v.position[0] - 130., v.position[1], v.position[2]],
                        ..v
                    })
                    .collect(),
                triangles: tri.clone(),
            },
            // Colours above 1 in RGB, alpha 0.75 (the palette's V).
            Mesh {
                vertex_colors: Some(vec![[2., 1.5, 1., 0.75]; 4]),
                normals: None,
                vertices: quad(120., 60.)
                    .into_iter()
                    .map(|v| Vertex {
                        position: [v.position[0] + 130., v.position[1], v.position[2]],
                        ..v
                    })
                    .collect(),
                triangles: tri.clone(),
            },
            // Behind the two cards: a blended card with an opacity above 1.
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad(110., 20.)
                    .into_iter()
                    .map(|v| Vertex {
                        position: [v.position[0], v.position[1] + 250., v.position[2]],
                        ..v
                    })
                    .collect(),
                triangles: tri.clone(),
            },
            // In front: a blended textured card whose vertex alpha (alpha
            // channel only, RGB 1) fades from 1 to 0 across it, as a
            // blended lighting surface under `lighting_vertex_alpha_blend_v1`.
            Mesh {
                vertex_colors: Some(vec![
                    [1., 1., 1., 1.],
                    [1., 1., 1., 0.],
                    [1., 1., 1., 0.],
                    [1., 1., 1., 1.],
                ]),
                normals: None,
                vertices: quad(90., 80.)
                    .into_iter()
                    .map(|v| Vertex {
                        position: [v.position[0], v.position[1] - 230., v.position[2]],
                        ..v
                    })
                    .collect(),
                triangles: tri,
            },
        ],
        textures: vec![
            texture(1, 1, &|_, _| [0.5, 0.5, 0.5, 1.]),
            texture(4, 4, &|x, y| {
                [0.9, 0.6, 0.3, 0.25 + 0.25 * ((x + y) % 4) as f32]
            }),
            // Green rises along U, alpha along V.
            texture(8, 8, &|x, y| [0.2, x as f32 / 7., 0.8, y as f32 / 7.]),
            // The palette: a 2D table, dark and clear at U = 0, V = 0.
            texture(8, 4, &|x, y| {
                let (u, v) = (x as f32 / 7., y as f32 / 3.);
                [u, v, u * v, (u * v).sqrt()]
            }),
            // Alpha from 0.2 to 0.9 along U: times 2, above 1 from the
            // middle on.
            texture(8, 8, &|x, y| {
                [0.9, 0.3 + 0.08 * y as f32, 0.2, 0.2 + 0.1 * x as f32]
            }),
        ],
        draws: Vec::new(),
        overlays: Vec::new(),
        blocks: Default::default(),
        animations: Vec::new(),
    };
    list.draws.push(draw(1, 0, 0, alpha(0, 0, 0, 1.)));
    let mut card = draw(2, 1, 1, alpha(1, 6, 7, 1.));
    card.falloff = Some(Falloff {
        start_angle: 1.,
        stop_angle: 0.3,
        start_opacity: 1.,
        stop_opacity: 0.,
    });
    list.draws.push(card);
    let mut additive = draw(3, 2, 2, alpha(1, 6, 0, 0.8));
    additive.palette = Some(Palette {
        texture: 3,
        colour: true,
        alpha: true,
    });
    list.draws.push(additive);
    // An opacity above 1 with blending (`blend_alpha_clamped_before_blend_v1`)
    // and a V offset (a material's driven field drawn under
    // `effect_material_over_controller_v1`).
    let mut strong = draw(4, 3, 4, alpha(1, 6, 7, 2.));
    strong.uv_offset = [0., 0.25];
    list.draws.push(strong);
    list.draws.push(draw(5, 4, 1, alpha(1, 6, 7, 1.)));
    let camera = fo4_scene::camera::look_at(
        [0., -500., 450.],
        [0., 0., 0.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let cpu_frame = |list: &DrawList| {
        let f = fo4_render::render(
            list,
            &camera,
            options,
            &fo4_core::Limits::default(),
            &fo4_core::Budget::new(256 << 20),
        )
        .unwrap();
        let rgba: Vec<u8> = f
            .color
            .iter()
            .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
            .collect();
        (rgba, f.instances)
    };
    let (cpu, cpu_tags) = cpu_frame(&list);
    let mut gpu_scene = crate::scene::convert(&mut list, &options).unwrap();
    let gpu = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
    let report = crate::compare::compare(&crate::compare::Inputs {
        size: SIZE,
        cpu: &cpu,
        gpu: &gpu.color,
        tags: Some((&cpu_tags, &gpu_tags)),
        clear: crate::compare::clear_rgb(options.clear),
    })
    .unwrap();
    println!(
        "agreement {:.6}, both {}, cpu only {}, gpu only {}, mean {:?}, p95 {:?}, max {:?}",
        report.agreement(),
        report.both,
        report.cpu_only,
        report.gpu_only,
        report.mean_abs,
        report.p95_abs,
        report.max_abs
    );
    assert!(report.agreement() > 0.999, "{}", report.agreement());
    assert!(
        report.mean_abs.iter().all(|m| *m < 1.5),
        "{:?}",
        report.mean_abs
    );
    assert!(
        report.p95_abs.iter().all(|m| *m <= 4),
        "{:?}",
        report.p95_abs
    );
    // Each term shows: without it the CPU frame differs there.
    let strips: [fn(&mut DrawList); 6] = [
        |l: &mut DrawList| l.draws[1].falloff = None,
        |l: &mut DrawList| l.draws[2].palette = None,
        |l: &mut DrawList| l.meshes[2].vertex_colors = Some(vec![[1., 1., 1., 0.75]; 4]),
        |l: &mut DrawList| l.draws[3].alpha.opacity = 1.,
        |l: &mut DrawList| l.draws[3].uv_offset = [0.; 2],
        |l: &mut DrawList| l.meshes[4].vertex_colors = None,
    ];
    for strip in strips {
        let mut other = DrawList {
            meshes: list
                .meshes
                .iter()
                .map(|m| Mesh {
                    vertex_colors: m.vertex_colors.clone(),
                    normals: m.normals.clone(),
                    vertices: m.vertices.clone(),
                    triangles: m.triangles.clone(),
                })
                .collect(),
            textures: list
                .textures
                .iter()
                .map(|t| {
                    texture(t.width, t.height, &|x, y| {
                        let at = ((y * t.width + x) * 4) as usize;
                        [t.rgba[at], t.rgba[at + 1], t.rgba[at + 2], t.rgba[at + 3]]
                    })
                })
                .collect(),
            draws: list.draws.iter().map(|d| Draw { ..*d }).collect(),
            overlays: Vec::new(),
            blocks: Default::default(),
            animations: Vec::new(),
        };
        strip(&mut other);
        assert_ne!(cpu_frame(&other).0, cpu);
    }
}

/// The game clock's terms (`fo4_render::Animation`): a textured opaque card
/// whose V offset a looping linear track drives, a blended card whose
/// opacity a clamped quadratic track drives, an opaque card whose colour an
/// emissive multiple track drives (`Draw::colour`) and a blended falloff
/// card whose stop opacity and stop angle tracks drive, over a grey floor. The CPU
/// reference at a fixed time (`DrawList::at_time`) and the GPU frame of the
/// list converted at that time agree; the GPU scene set to a second time
/// (`GpuScene::at_time`, the window's per-frame update) agrees with the CPU
/// at that time; and the two times draw different frames.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_clock_driven_uv_opacity_colour_and_falloff() {
    use fo4_render::{Alpha, Animation, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
    use fo4_scene::clock::{Key, Keys, Timing, Track};
    let quad = |h: f64, x: f64, z: f64| -> Vec<Vertex> {
        [[-h, -h], [h, -h], [h, h], [-h, h]]
            .iter()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
            .map(|(p, uv)| Vertex {
                position: [p[0] + x, p[1], z],
                uv,
            })
            .collect()
    };
    let texture = |width: u32, height: u32, texel: &dyn Fn(u32, u32) -> [f32; 4]| Texture {
        mips: Vec::new(),
        width,
        height,
        rgba: (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| texel(x, y)))
            .collect(),
        mip: 0,
        srgb: false,
    };
    let alpha = |enabled: u8, opacity: f32| Alpha {
        enabled,
        source: 6,
        destination: 7,
        test_enabled: 0,
        test_ref: 0,
        nif_test_function: Some(4),
        opacity,
        nif_flags: None,
    };
    let draw = |instance: u32, mesh: usize, texture: usize, alpha: Alpha| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh,
        texture,
        world: fo4_scene::transform::Transform::IDENTITY,
        alpha,
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let key = |time: f32, value: f32, tangent: f32| Key {
        time,
        value: [value, 0., 0.],
        forward: [tangent, 0., 0.],
        backward: [tangent, 0., 0.],
        tbc: [0.; 3],
    };
    let track = |cycle: u16, kind: u32, keys: Vec<Key>| Track {
        timing: Timing {
            flags: 8 | (cycle << 1),
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 2.,
        },
        keys: Keys {
            components: 1,
            kind,
            keys,
        },
    };
    let tri = vec![[0, 1, 2], [0, 2, 3]];
    let mesh = |vertices: Vec<Vertex>| Mesh {
        vertex_colors: None,
        normals: None,
        vertices,
        triangles: tri.clone(),
    };
    let mut tilted = mesh(quad(80., 300., 100.));
    tilted.normals = Some(vec![
        [-0.4, 0., 1.],
        [0.4, 0., 1.],
        [0.4, 0.4, 1.],
        [-0.4, 0.4, 1.],
    ]);
    let mut list = DrawList {
        meshes: vec![
            mesh(quad(400., 0., 0.)),
            mesh(quad(120., -130., 40.)),
            mesh(quad(120., 130., 60.)),
            mesh(quad(80., -300., 80.)),
            tilted,
        ],
        textures: vec![
            texture(1, 1, &|_, _| [0.5, 0.5, 0.5, 1.]),
            // Bands along V: the offset moves them.
            texture(8, 8, &|_, y| {
                let v = y as f32 / 7.;
                [v, 1. - v, 0.3, 1.]
            }),
            texture(4, 4, &|x, _| [0.9, 0.6, 0.2, 0.5 + 0.1 * x as f32]),
        ],
        draws: Vec::new(),
        overlays: Vec::new(),
        blocks: Default::default(),
        animations: Vec::new(),
    };
    list.draws.push(draw(1, 0, 0, alpha(0, 1.)));
    list.draws.push(draw(2, 1, 1, alpha(0, 1.)));
    list.draws.push(draw(3, 2, 2, alpha(1, 1.)));
    let mut scroll: [Option<Track>; fo4_render::TRACKS] = Default::default();
    scroll[fo4_render::TRACK_V_OFFSET] = Some(track(
        0,
        fo4_scene::clock::LINEAR,
        vec![key(0., 0., 0.), key(2., 1., 0.)],
    ));
    let mut fade: [Option<Track>; fo4_render::TRACKS] = Default::default();
    fade[fo4_render::TRACK_OPACITY] = Some(track(
        2,
        fo4_scene::clock::QUADRATIC,
        vec![key(0., 1., 0.), key(2., 0.1, 0.)],
    ));
    list.animations.push(Animation::new(1, scroll));
    list.animations.push(Animation::new(2, fade));
    // The colour: (1, 0.5, 0.25) times a multiple from 0.4 to 1.8.
    list.draws.push(Draw {
        colour: Some([0.4, 0.2, 0.1]),
        ..draw(4, 3, 0, alpha(0, 1.))
    });
    let mut glow: [Option<Track>; fo4_render::TRACKS] = Default::default();
    glow[fo4_render::TRACK_EMISSIVE_MULTIPLE] = Some(track(
        0,
        fo4_scene::clock::LINEAR,
        vec![key(0., 0.4, 0.), key(2., 1.8, 0.)],
    ));
    list.animations.push(Animation {
        draw: 3,
        tracks: glow,
        colour: [1., 0.5, 0.25],
        scale: 1.,
        motion: None,
    });
    // The falloff: stop opacity 1 to 0.1 and stop angle 60 to 20 degrees.
    list.draws.push(Draw {
        falloff: Some(fo4_render::Falloff {
            start_angle: 1.,
            stop_angle: 0.5,
            start_opacity: 1.,
            stop_opacity: 1.,
        }),
        ..draw(5, 4, 2, alpha(1, 1.))
    });
    let mut edge: [Option<Track>; fo4_render::TRACKS] = Default::default();
    edge[fo4_render::TRACK_FALLOFF_STOP_OPACITY] = Some(track(
        0,
        fo4_scene::clock::LINEAR,
        vec![key(0., 1., 0.), key(2., 0.1, 0.)],
    ));
    edge[fo4_render::TRACK_FALLOFF_STOP_ANGLE] = Some(track(
        2,
        fo4_scene::clock::LINEAR,
        vec![key(0., 60., 0.), key(2., 20., 0.)],
    ));
    list.animations.push(Animation::new(4, edge));
    let camera = fo4_scene::camera::look_at(
        [0., -500., 450.],
        [0., 0., 0.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let cpu_frame = |list: &DrawList| {
        let f = fo4_render::render(
            list,
            &camera,
            options,
            &fo4_core::Limits::default(),
            &fo4_core::Budget::new(256 << 20),
        )
        .unwrap();
        let rgba: Vec<u8> = f
            .color
            .iter()
            .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
            .collect();
        (rgba, f.instances)
    };
    let agree = |cpu: &(Vec<u8>, Vec<u32>), gpu: &Frames| {
        let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
        let report = crate::compare::compare(&crate::compare::Inputs {
            size: SIZE,
            cpu: &cpu.0,
            gpu: &gpu.color,
            tags: Some((&cpu.1, &gpu_tags)),
            clear: crate::compare::clear_rgb(options.clear),
        })
        .unwrap();
        println!(
            "agreement {:.6}, mean {:?}, p95 {:?}, max {:?}",
            report.agreement(),
            report.mean_abs,
            report.p95_abs,
            report.max_abs
        );
        assert!(report.agreement() > 0.999, "{}", report.agreement());
        assert!(
            report.mean_abs.iter().all(|m| *m < 1.5),
            "{:?}",
            report.mean_abs
        );
        assert!(
            report.p95_abs.iter().all(|m| *m <= 4),
            "{:?}",
            report.p95_abs
        );
    };
    let (t1, t2) = (0.5, 1.5);
    list.at_time(t1).unwrap();
    assert_eq!(list.draws[1].uv_offset, [0., 0.25]);
    let cpu1 = cpu_frame(&list);
    // Two scenes converted at the first time (`convert` moves nothing out
    // of a list without stored blocks; `headless` moves the texels out of
    // the scene it draws): one drawn there, one set to the second time.
    let mut gpu_scene = crate::scene::convert(&mut list, &options).unwrap();
    let mut later = crate::scene::convert(&mut list, &options).unwrap();
    assert_eq!(gpu_scene.animated.len(), 4);
    let gpu1 = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    agree(&cpu1, &gpu1);
    list.at_time(t2).unwrap();
    let cpu2 = cpu_frame(&list);
    assert_ne!(cpu1.0, cpu2.0);
    // The new cards' fields at the second time.
    let c = list.draws[3].colour.unwrap();
    assert!(
        (c[0] - 1.45).abs() < 1e-5 && (c[2] - 0.3625).abs() < 1e-5,
        "{c:?}"
    );
    let f = list.draws[4].falloff.unwrap();
    assert!((f.stop_opacity - 0.325).abs() < 1e-5, "{f:?}");
    assert!(
        (f.stop_angle - 30f32.to_radians().cos()).abs() < 1e-5,
        "{f:?}"
    );
    later.at_time(t2).unwrap();
    let gpu2 = headless(&mut later, &camera, options.clear, true).unwrap();
    agree(&cpu2, &gpu2);
    assert_ne!(gpu1.color, gpu2.color);
}

/// Two placements of one surface, coplanar and overlapping (two floating
/// debris cards at one height, offset along the view, as in a real
/// interior view where the frames' instance tags disagreed): where only one
/// covers a pixel, the CPU and GPU tags agree; where both do, the depths are
/// equal in exact arithmetic and each renderer keeps the one its own
/// arithmetic puts nearer or first (the CPU's f64 depths, rounded to f32,
/// tie and keep the first drawn: list order for opaque draws, the far-first
/// triangle order for blended ones; the GPU's f32 transforms and depths
/// split the tie by rounding). Both tags are one of the two placements;
/// which one is not the same (`spec/render.md`, Instance tags of the CPU
/// and the viewer). Needs a GPU.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn coplanar_placements_disagree_on_tags_only_where_they_overlap() {
    use fo4_render::{Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
    let h = 400.;
    let vertices: Vec<Vertex> = [[-h, -h], [h, -h], [h, h], [-h, h]]
        .iter()
        .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
        .map(|(p, uv)| Vertex {
            position: [p[0], p[1], 0.],
            uv,
        })
        .collect();
    let draw = |instance: u32, y: f64, enabled: u8| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh: 0,
        texture: 0,
        world: fo4_scene::transform::Transform {
            translation: [655.99 + y * 0.0001, 2208.09 + y, 27.23],
            rotation: {
                let (s, c) = (4.712_389f64).sin_cos();
                [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]]
            },
            ..fo4_scene::transform::Transform::IDENTITY
        },
        alpha: Alpha {
            enabled,
            source: 6,
            destination: 7,
            test_enabled: 0,
            test_ref: 0,
            nif_test_function: Some(4),
            opacity: 1.,
            nif_flags: None,
        },
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let mut differ = 0;
    for (order, enabled) in [
        ([0., 224.], 1),
        ([224., 0.], 1),
        ([0., 224.], 0),
        ([224., 0.], 0),
    ] {
        let list = |draws: Vec<Draw>| DrawList {
            meshes: vec![Mesh {
                vertex_colors: None,
                normals: None,
                vertices: vertices.clone(),
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            }],
            textures: vec![Texture {
                mips: Vec::new(),
                width: 1,
                height: 1,
                rgba: vec![0.8, 0.4, 0.2, 0.5],
                mip: 0,
                srgb: false,
            }],
            draws,
            overlays: Vec::new(),
            blocks: Default::default(),
            animations: Vec::new(),
        };
        let camera = fo4_scene::camera::look_at(
            [1050., 2250., 900.],
            [1100., 2600., 250.],
            fo4_scene::camera::reference_frustum(SIZE),
            SIZE,
        )
        .unwrap();
        let options = fo4_render::RenderOptions::REFERENCE;
        let cpu = |l: &DrawList| {
            fo4_render::render(
                l,
                &camera,
                options,
                &fo4_core::Limits::default(),
                &fo4_core::Budget::new(256 << 20),
            )
            .unwrap()
            .instances
        };
        // Each placement alone gives its coverage.
        let one = cpu(&list(vec![draw(1, order[0], enabled)]));
        let two = cpu(&list(vec![draw(2, order[1], enabled)]));
        let mut both = list(vec![draw(1, order[0], enabled), draw(2, order[1], enabled)]);
        let tags_cpu = cpu(&both);
        let mut scene = crate::scene::convert(&mut both, &options).unwrap();
        let gpu = headless(&mut scene, &camera, options.clear, true).unwrap();
        let tags_gpu = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
        for k in 0..tags_cpu.len() {
            let (c, g) = (tags_cpu[k], tags_gpu[k]);
            if one[k] != u32::MAX && two[k] != u32::MAX {
                assert!([1, 2].contains(&c) && [1, 2].contains(&g), "{k}: {c} {g}");
                differ += usize::from(c != g);
            } else {
                assert_eq!(c, g, "pixel {k} covered by one placement");
            }
        }
    }
    // The overlap does split differently.
    assert!(differ > 0);
}

/// Geometry that free-running transform controllers move on the game clock
/// (`fo4_render::Motion`, behind the clock's transform switches): over a
/// grey floor, a card that a looping translation track slides, and a card
/// under two moving nodes, a pivot turned about Z by linear quaternion keys
/// (slerp) and below it a node tilted about X by XYZ angle keys and grown
/// by scale keys. The CPU reference at a time (`DrawList::at_time`) and the
/// GPU frame of the list converted at that time agree; the GPU scene moved
/// to a second time (`GpuScene::at_time`, the window's per-frame update)
/// agrees with the CPU at that time; the two times draw different frames,
/// and no moving draw gets a surface of its own. Needs a GPU.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_clock_moved_geometry() {
    use fo4_render::{
        Alpha, Animation, Draw, DrawList, Mesh, Motion, MotionStep, Sidedness, Texture, Vertex,
    };
    use fo4_scene::clock::{Key, Keys, NodeKeys, Rotation, Timing, TransformTrack};
    use fo4_scene::transform::Transform as T;
    let quad = |h: f64| -> Vec<Vertex> {
        [[-h, -h], [h, -h], [h, h], [-h, h]]
            .iter()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
            .map(|(p, uv)| Vertex {
                position: [p[0], p[1], 0.],
                uv,
            })
            .collect()
    };
    let texture = |width: u32, height: u32, texel: &dyn Fn(u32, u32) -> [f32; 4]| Texture {
        mips: Vec::new(),
        width,
        height,
        rgba: (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| texel(x, y)))
            .collect(),
        mip: 0,
        srgb: false,
    };
    let alpha = Alpha {
        enabled: 0,
        source: 6,
        destination: 7,
        test_enabled: 0,
        test_ref: 0,
        nif_test_function: Some(4),
        opacity: 1.,
        nif_flags: None,
    };
    let draw = |instance: u32, mesh: usize, texture: usize| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh,
        texture,
        world: T::IDENTITY,
        alpha,
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let tri = vec![[0, 1, 2], [0, 2, 3]];
    let mesh = |vertices: Vec<Vertex>| Mesh {
        vertex_colors: None,
        normals: None,
        vertices,
        triangles: tri.clone(),
    };
    let key = |time: f32, value: [f32; 3]| Key {
        time,
        value,
        forward: [0.; 3],
        backward: [0.; 3],
        tbc: [0.; 3],
    };
    let group = |components: usize, keys: Vec<Key>| Keys {
        components,
        kind: fo4_scene::clock::LINEAR,
        keys,
    };
    let empty = |components: usize| Keys {
        components,
        kind: 0,
        keys: Vec::new(),
    };
    let node = |keys: NodeKeys| TransformTrack {
        timing: Timing {
            flags: 8,
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 2.,
        },
        node: 0,
        stored_translation: [0.; 3],
        stored_rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        stored_scale: 1.,
        pose_translation: None,
        pose_rotation: None,
        pose_scale: None,
        keys: Some(keys),
    };
    let at = |x: f64, y: f64, z: f64| T {
        translation: [x, y, z],
        ..T::IDENTITY
    };
    let slide = node(NodeKeys {
        rotation: None,
        translation: group(3, vec![key(0., [0.; 3]), key(2., [0., 260., 0.])]),
        scale: empty(1),
    });
    let half = std::f32::consts::FRAC_PI_4;
    let pivot = node(NodeKeys {
        rotation: Some(Rotation::Quaternion {
            kind: fo4_scene::clock::LINEAR,
            keys: vec![
                (0., [1., 0., 0., 0.]),
                (2., [half.cos(), 0., 0., half.sin()]),
            ],
        }),
        translation: empty(3),
        scale: empty(1),
    });
    let tilt = node(NodeKeys {
        rotation: Some(Rotation::Xyz(Box::new([
            group(1, vec![key(0., [0.; 3]), key(2., [0.7, 0., 0.])]),
            group(1, vec![key(0., [0.; 3])]),
            group(1, vec![key(0., [0.; 3])]),
        ]))),
        translation: empty(3),
        scale: group(1, vec![key(0., [1., 0., 0.]), key(2., [1.6, 0., 0.])]),
    });
    let mut list = DrawList {
        meshes: vec![mesh(quad(400.)), mesh(quad(70.)), mesh(quad(60.))],
        textures: vec![
            texture(1, 1, &|_, _| [0.5, 0.5, 0.5, 1.]),
            texture(8, 8, &|x, y| [x as f32 / 7., y as f32 / 7., 0.3, 1.]),
            texture(4, 4, &|x, _| [0.9, 0.6 - 0.1 * x as f32, 0.2, 1.]),
        ],
        draws: vec![draw(1, 0, 0), draw(2, 1, 1), draw(3, 2, 2)],
        overlays: Vec::new(),
        blocks: Default::default(),
        animations: Vec::new(),
    };
    list.animations.push(Animation {
        motion: Some(Motion {
            pre: at(-230., -120., 40.),
            steps: vec![MotionStep {
                node: slide,
                after: T::IDENTITY,
            }],
        }),
        ..Animation::new(1, Default::default())
    });
    list.animations.push(Animation {
        motion: Some(Motion {
            pre: at(170., -60., 70.),
            steps: vec![
                MotionStep {
                    node: pivot,
                    after: at(90., 0., 0.),
                },
                MotionStep {
                    node: tilt,
                    after: T::IDENTITY,
                },
            ],
        }),
        ..Animation::new(2, Default::default())
    });
    let camera = fo4_scene::camera::look_at(
        [0., -500., 450.],
        [0., 0., 0.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let cpu_frame = |list: &DrawList| {
        let f = fo4_render::render(
            list,
            &camera,
            options,
            &fo4_core::Limits::default(),
            &fo4_core::Budget::new(256 << 20),
        )
        .unwrap();
        let rgba: Vec<u8> = f
            .color
            .iter()
            .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
            .collect();
        (rgba, f.instances)
    };
    let agree = |cpu: &(Vec<u8>, Vec<u32>), gpu: &Frames| {
        let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
        let report = crate::compare::compare(&crate::compare::Inputs {
            size: SIZE,
            cpu: &cpu.0,
            gpu: &gpu.color,
            tags: Some((&cpu.1, &gpu_tags)),
            clear: crate::compare::clear_rgb(options.clear),
        })
        .unwrap();
        let same_tag = cpu
            .1
            .iter()
            .zip(&gpu_tags)
            .filter(|(a, b)| **a != u32::MAX && a == b)
            .count();
        let drawn = cpu.1.iter().filter(|a| **a != u32::MAX).count();
        println!(
            "agreement {:.6}, same tag {same_tag} of {drawn}, mean {:?}, p95 {:?}, max {:?}",
            report.agreement(),
            report.mean_abs,
            report.p95_abs,
            report.max_abs
        );
        assert!(report.agreement() > 0.999, "{}", report.agreement());
        assert!(same_tag as f64 > 0.999 * drawn as f64, "{same_tag} {drawn}");
        assert!(
            report.mean_abs.iter().all(|m| *m < 1.5),
            "{:?}",
            report.mean_abs
        );
        assert!(
            report.p95_abs.iter().all(|m| *m <= 4),
            "{:?}",
            report.p95_abs
        );
    };
    let (t1, t2) = (0.5, 1.5);
    list.at_time(t1).unwrap();
    assert!((list.draws[1].world.translation[1] - (-120. + 65.)).abs() < 1e-9);
    let cpu1 = cpu_frame(&list);
    let mut gpu_scene = crate::scene::convert(&mut list, &options).unwrap();
    let mut later = crate::scene::convert(&mut list, &options).unwrap();
    assert_eq!(gpu_scene.moving.len(), 2);
    assert!(gpu_scene.animated.is_empty());
    assert_eq!(gpu_scene.surfaces.len(), 3);
    let gpu1 = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    agree(&cpu1, &gpu1);
    list.at_time(t2).unwrap();
    let cpu2 = cpu_frame(&list);
    assert_ne!(cpu1.0, cpu2.0);
    // The nested card at the second time: the pivot an eighth of a turn
    // and three quarters of the way (slerp), the tilt node's scale 1.45.
    assert!((list.draws[2].world.scale - 1.45).abs() < 1e-6);
    later.at_time(t2).unwrap();
    let gpu2 = headless(&mut later, &camera, options.clear, true).unwrap();
    agree(&cpu2, &gpu2);
    assert_ne!(gpu1.color, gpu2.color);
}

/// A synthetic run timeline's frame (`fo4_scene::timeline`): a floor and
/// a wall card, two actors standing where the timeline puts them at the
/// second (one moved by an assumed move after its stored place), drawn as
/// the timeline markers (`fo4_render::timeline`), seen from the camera the
/// frame gives (eye height above the track, looking along its move). The
/// CPU reference and the GPU frame of the same list agree; the markers
/// show in both under their own tags.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_a_timeline_frame() {
    use fo4_render::{Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
    use fo4_scene::timeline::{self as tl, Event, Spot, Timeline, What};
    let key = |id: u32| fo4_plugin::FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let room = key(0x800);
    let event = |t: f64, what: What| Event { t, what };
    let actor = |t: f64, id: u32, pos: [f64; 3]| {
        event(
            t,
            What::Actor {
                reference: key(id),
                at: Spot::At {
                    space: room.clone(),
                    pos,
                },
                source: "test".into(),
            },
        )
    };
    let player = |t: f64, pos: [f64; 3]| {
        event(
            t,
            What::Player {
                space: room.clone(),
                cell: Some(room.clone()),
                pos,
                facing: None,
            },
        )
    };
    let t = Timeline {
        events: vec![
            player(0., [0., -600., 0.]),
            actor(0., 0x900, [-150., 0., 0.]),
            actor(0., 0x901, [400., 300., 0.]),
            actor(5., 0x901, [150., 50., 0.]),
            player(10., [0., -200., 0.]),
        ],
        ..Timeline::default()
    };
    let text = t.write();
    let frame = Timeline::parse(&text).unwrap().at(7.5);
    let (eye, look, from) = tl::eye_look(&frame, 120., None).unwrap();
    assert_eq!(eye, [0., -300., 120.]);
    assert_eq!(from, "the track's current move");
    let at: Vec<[f64; 3]> = frame
        .actors
        .values()
        .map(|(_, s, _)| match s {
            Spot::At { pos, .. } => *pos,
            Spot::Open(_) => unreachable!(),
        })
        .collect();
    assert_eq!(at, vec![[-150., 0., 0.], [150., 50., 0.]]);
    let quad = |corners: [[f64; 3]; 4]| -> Vec<Vertex> {
        corners
            .iter()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
            .map(|(p, uv)| Vertex { position: *p, uv })
            .collect()
    };
    let texture = |rgba: [f32; 4]| Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
        mip: 0,
        srgb: false,
    };
    let draw = |instance: u32, mesh: usize, texture: usize| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh,
        texture,
        world: fo4_scene::transform::Transform::IDENTITY,
        alpha: Alpha {
            enabled: 0,
            source: 0,
            destination: 0,
            test_enabled: 0,
            test_ref: 0,
            nif_test_function: Some(4),
            opacity: 1.,
            nif_flags: None,
        },
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let tri = vec![[0, 1, 2], [0, 2, 3]];
    let mut list = DrawList {
        meshes: vec![
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad([
                    [-800., -800., 0.],
                    [800., -800., 0.],
                    [800., 800., 0.],
                    [-800., 800., 0.],
                ]),
                triangles: tri.clone(),
            },
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad([
                    [-800., 600., 0.],
                    [800., 600., 0.],
                    [800., 600., 400.],
                    [-800., 600., 400.],
                ]),
                triangles: tri,
            },
        ],
        textures: vec![texture([0.4, 0.4, 0.4, 1.]), texture([0.2, 0.3, 0.6, 1.])],
        draws: Vec::new(),
        overlays: Vec::new(),
        blocks: Default::default(),
        animations: Vec::new(),
    };
    list.draws.push(draw(0, 0, 0));
    list.draws.push(draw(1, 1, 1));
    let first = fo4_render::timeline::next_tag(&list);
    let b = fo4_core::Budget::new(64 << 20);
    let tags = fo4_render::timeline::append_markers(&mut list, &at, first, &b).unwrap();
    assert_eq!(tags, vec![2, 3]);
    let camera =
        fo4_scene::camera::look_at(eye, look, fo4_scene::camera::reference_frustum(SIZE), SIZE)
            .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let f = fo4_render::render(
        &list,
        &camera,
        options,
        &fo4_core::Limits::default(),
        &fo4_core::Budget::new(256 << 20),
    )
    .unwrap();
    let cpu: Vec<u8> = f
        .color
        .iter()
        .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
        .collect();
    let mut gpu_scene = crate::scene::convert(&mut list, &options).unwrap();
    let gpu = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
    for tag in [2u32, 3] {
        assert!(f.instances.contains(&tag), "CPU marker {tag}");
        assert!(gpu_tags.contains(&tag), "GPU marker {tag}");
    }
    let report = crate::compare::compare(&crate::compare::Inputs {
        size: SIZE,
        cpu: &cpu,
        gpu: &gpu.color,
        tags: Some((&f.instances, &gpu_tags)),
        clear: crate::compare::clear_rgb(options.clear),
    })
    .unwrap();
    println!(
        "timeline frame: agreement {:.6}, same tag {:?} of {}, mean {:?}, p95 {:?}, max {:?}",
        report.agreement(),
        report.same_tag,
        report.both,
        report.mean_abs,
        report.p95_abs,
        report.max_abs
    );
    assert!(report.agreement() > 0.999, "{}", report.agreement());
    assert!(
        report.same_tag.unwrap() as f64 > 0.999 * report.both as f64,
        "{:?}",
        report.same_tag
    );
    assert!(
        report.p95_abs.iter().all(|m| *m <= 4),
        "{:?}",
        report.p95_abs
    );
}

/// A synthetic run timeline's frame with its cast (`--timeline-cast`,
/// `fo4_scene::timeline::cast`): a skinned actor of the synthetic room
/// (`fo4_render::fixture::actor_room`) drawn as its character where the
/// run moved it, and an actor whose base is missing drawn as its marker,
/// through the viewer's own timeline path (`crate::timeline::cast` and
/// `finish`), the camera aimed at the caller's point. The CPU reference and
/// the GPU frame of the same list agree; the character and the marker show
/// in both under their own tags.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_a_timeline_cast_frame() {
    use fo4_render::fixture::{actor_room, Placed, ACTOR_BASE, ACTOR_ROOM};
    use fo4_scene::timeline::{self as tl, Aim, Event, Spot, Timeline, What};
    use fo4_scene::{Loader, RuleLevel};
    let temp = fo4_core::TestDir::new("view-timeline-cast");
    let root = temp.join("Data");
    let m = actor_room(
        &root,
        &[
            Placed {
                id: 0x900,
                base: ACTOR_BASE,
                data: [0., 0., 0., 0., 0., std::f32::consts::FRAC_PI_2],
                scale: Some(30.),
            },
            Placed {
                id: 0x901,
                base: 0x8ff,
                data: [150., 100., 0., 0., 0., 0.],
                scale: None,
            },
        ],
        &[],
    );
    let level = RuleLevel::Hypotheses;
    let b = fo4_core::Budget::new(256 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
    let mut loader = Loader::new(&store, &vfs, "english", level, &b);
    let mut scene = loader.by_editor_id(ACTOR_ROOM, &b).unwrap();
    let key = |id: u32| fo4_plugin::FormKey {
        plugin: "base.esm".into(),
        object_id: id,
    };
    let room = key(0x800);
    let event = |t: f64, what: What| Event { t, what };
    let player = |t: f64, pos: [f64; 3]| {
        event(
            t,
            What::Player {
                space: room.clone(),
                cell: Some(room.clone()),
                pos,
                facing: None,
            },
        )
    };
    let actor = |t: f64, id: u32, pos: [f64; 3]| {
        event(
            t,
            What::Actor {
                reference: key(id),
                at: Spot::At {
                    space: room.clone(),
                    pos,
                },
                source: "test".into(),
            },
        )
    };
    let timeline = Timeline {
        events: vec![
            player(0., [60., -250., 0.]),
            actor(0., 0x900, [0., 0., 0.]),
            actor(0., 0x901, [150., 100., 0.]),
            actor(1., 0x900, [0., 100., 0.]),
            player(10., [60., -200., 0.]),
        ],
        ..Timeline::default()
    };
    let frame = timeline.at(5.);
    let p = crate::timeline::Prepared { timeline, frame };
    let args = crate::timeline::Args {
        file: String::new(),
        at: 5.,
        eye_height: 250.,
        heading: None,
        nowait: false,
        overlay: None,
        child_flag_agrees: false,
        cast: true,
        target_rotation: false,
        missing_bones_at_bind: false,
        missing_bones_nearest: false,
        pose: fo4_scene::char_graph::Switches::default(),
        aim: Aim::Point([60., 100., 250.]),
        dead: tl::Dead::Standing,
        sit_marker: false,
        look: fo4_scene::actor::Readings::default(),
        model_graph_idle: false,
        skinned_models: false,
        head_conditions: false,
        ragdoll_left_out: false,
        memory_report: None,
        ride: None,
    };
    let applied = tl::apply(&mut scene, &mut loader, &p.frame, level, &b).unwrap();
    let c = crate::timeline::cast(&p, &args, &scene, &mut loader, level, &b).unwrap();
    let cast = c.cast.as_ref().unwrap();
    let mut build = fo4_render::draw::build_with_actors(
        &scene,
        &cast.actors,
        &loader,
        &m.limits,
        fo4_render::draw::BuildOptions::reference(level),
        &b,
    )
    .unwrap();
    let drawn = crate::timeline::finish(&p, &args, c, level, &applied, &mut build, &b).unwrap();
    assert!(
        drawn.overlay.contains("1 drawn as characters"),
        "{}",
        drawn.overlay
    );
    assert!(
        drawn.overlay.contains("marker instead\ttag 2"),
        "{}",
        drawn.overlay
    );
    assert!(drawn.overlay.contains("timeline_actor_stored_rotation_v1"));
    assert_eq!(drawn.eye, [60., -225., 250.]);
    let camera = fo4_scene::camera::look_at(
        drawn.eye,
        drawn.look,
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let f = fo4_render::render(
        &build.list,
        &camera,
        options,
        &fo4_core::Limits::default(),
        &fo4_core::Budget::new(256 << 20),
    )
    .unwrap();
    let cpu: Vec<u8> = f
        .color
        .iter()
        .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
        .collect();
    let mut gpu_scene = crate::scene::convert(&mut build.list, &options).unwrap();
    let gpu = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
    // The character (instance 0) and the marker (tag 2).
    for tag in [0u32, 2] {
        assert!(f.instances.contains(&tag), "CPU tag {tag}");
        assert!(gpu_tags.contains(&tag), "GPU tag {tag}");
    }
    let report = crate::compare::compare(&crate::compare::Inputs {
        size: SIZE,
        cpu: &cpu,
        gpu: &gpu.color,
        tags: Some((&f.instances, &gpu_tags)),
        clear: crate::compare::clear_rgb(options.clear),
    })
    .unwrap();
    println!(
        "timeline cast frame: agreement {:.6}, same tag {:?} of {}, mean {:?}, p95 {:?}, max {:?}",
        report.agreement(),
        report.same_tag,
        report.both,
        report.mean_abs,
        report.p95_abs,
        report.max_abs
    );
    assert!(report.agreement() > 0.999, "{}", report.agreement());
    assert!(
        report.same_tag.unwrap() as f64 > 0.999 * report.both as f64,
        "{:?}",
        report.same_tag
    );
    assert!(
        report.p95_abs.iter().all(|m| *m <= 4),
        "{:?}",
        report.p95_abs
    );
}

/// A synthetic frame under a light (`fo4_scene::light`): a floor and a
/// wall lit by an ambient colour, an ambient cube, a directional light and
/// two points, fogged by distance, a marker left unlit, and the sky
/// filling the background. The CPU's `render_lit` and the GPU frame of
/// `convert_lit` agree on coverage and tags, closely on colour, and the
/// sky agrees in the background.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_a_lit_frame() {
    lit_frame_agrees(false);
}

/// The same lit frame with the wall lit by its vertex normals (tilted)
/// and every exposure part set (`frame_exposure_seen_light_v1`'s scale,
/// `image_space_hdr_white_v1`, `image_space_cinematic_tint_v1`,
/// `light_vertex_normals_v1`): the CPU and the viewer still agree.
#[test]
#[ignore = "needs a GPU: draws offscreen through Bevy"]
fn cpu_and_gpu_frames_agree_on_an_exposed_frame_with_vertex_normals() {
    lit_frame_agrees(true);
}

fn lit_frame_agrees(exposed: bool) {
    use fo4_render::{Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
    use fo4_scene::light::{Fog, Light, Point, Sky};
    let quad = |corners: [[f64; 3]; 4]| -> Vec<Vertex> {
        corners
            .iter()
            .zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]])
            .map(|(p, uv)| Vertex { position: *p, uv })
            .collect()
    };
    let texture = |rgba: [f32; 4]| Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
        mip: 0,
        srgb: false,
    };
    let draw = |instance: u32, mesh: usize, texture: usize| Draw {
        sidedness: Sidedness {
            material_two_sided: Some(1),
            nif_shader_flags_2: None,
        },
        instance,
        mesh,
        texture,
        world: fo4_scene::transform::Transform::IDENTITY,
        alpha: Alpha {
            enabled: 0,
            source: 0,
            destination: 0,
            test_enabled: 0,
            test_ref: 0,
            nif_test_function: Some(4),
            opacity: 1.,
            nif_flags: None,
        },
        uv_offset: [0.; 2],
        uv_scale: [1.; 2],
        raw_tile_flags: None,
        falloff: None,
        palette: None,
        colour: None,
    };
    let tri = vec![[0, 1, 2], [0, 2, 3]];
    let mut list = DrawList {
        meshes: vec![
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad([
                    [-800., -800., 0.],
                    [800., -800., 0.],
                    [800., 800., 0.],
                    [-800., 800., 0.],
                ]),
                triangles: tri.clone(),
            },
            Mesh {
                vertex_colors: None,
                normals: None,
                vertices: quad([
                    [-800., 600., 0.],
                    [800., 600., 0.],
                    [800., 600., 400.],
                    [-800., 600., 400.],
                ]),
                triangles: tri,
            },
        ],
        textures: vec![texture([0.8, 0.8, 0.8, 1.]), texture([0.6, 0.7, 0.9, 1.])],
        draws: Vec::new(),
        overlays: Vec::new(),
        blocks: Default::default(),
        animations: Vec::new(),
    };
    list.draws.push(draw(0, 0, 0));
    list.draws.push(draw(1, 1, 1));
    let b = fo4_core::Budget::new(64 << 20);
    let tags =
        fo4_render::timeline::append_markers(&mut list, &[[-200., 200., 0.]], 2, &b).unwrap();
    assert_eq!(tags, vec![2]);
    // The floor and the wall are lit, the marker is not.
    let lit: Vec<bool> = (0..list.draws.len()).map(|i| i < 2).collect();
    let light = Light {
        ambient: [0.15, 0.12, 0.1],
        cube: Some([
            [0.05, 0.05, 0.05],
            [0.1, 0.05, 0.05],
            [0.05, 0.1, 0.05],
            [0.05, 0.05, 0.1],
            [0.2, 0.2, 0.25],
            [0.05, 0.05, 0.05],
        ]),
        directional: Some(([0.3, 0.25, 0.2], fo4_scene::light::toward(30, 200))),
        points: vec![
            Point {
                position: [150., 100., 120.],
                radius: 500.,
                colour: [1., 0.6, 0.3],
            },
            Point {
                position: [-300., 500., 250.],
                radius: 400.,
                colour: [0.3, 0.5, 1.],
            },
        ],
        fog: Some(Fog {
            near: 300.,
            far: 2000.,
            max: 0.5,
            colour: [0.4, 0.45, 0.5],
        }),
        sky: Some(Sky {
            zenith: [0.1, 0.2, 0.3],
            lower: [0.45, 0.6, 0.75],
            horizon: [0.85, 0.7, 0.55],
        }),
        lit: true,
        rules: Default::default(),
        report: Vec::new(),
        exposure: Default::default(),
    };
    let mut light = light;
    if exposed {
        light.exposure = fo4_scene::light::Exposure {
            eye: true,
            scale: Some(0.7),
            white: Some(1.5),
            cinematic: Some([0.8, 1.1, 1.05]),
            tint: Some([0.2, 1., 0.6, 0.3]),
            floor: false,
            visible: false,
        };
        list.meshes[1].normals = Some(vec![
            [0.3, -0.9, 0.2],
            [-0.3, -0.9, 0.2],
            [-0.2, -0.9, -0.3],
            [0.2, -0.9, -0.3],
        ]);
    }
    let camera = fo4_scene::camera::look_at(
        [0., -300., 120.],
        [0., 600., 260.],
        fo4_scene::camera::reference_frustum(SIZE),
        SIZE,
    )
    .unwrap();
    let options = fo4_render::RenderOptions::REFERENCE;
    let f = fo4_render::render_lit(
        &list,
        &camera,
        options,
        Some(fo4_render::Lighting {
            light: &light,
            lit: &lit,
        }),
        &fo4_core::Limits::default(),
        &fo4_core::Budget::new(256 << 20),
    )
    .unwrap();
    let cpu: Vec<u8> = f
        .color
        .iter()
        .flat_map(|c| c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
        .collect();
    let mut gpu_scene =
        crate::scene::convert_lit(&mut list, &options, Some((&light, &lit))).unwrap();
    assert!(gpu_scene.light.as_ref().is_some_and(|l| l.sky));
    assert_eq!(
        gpu_scene
            .surfaces
            .iter()
            .filter(|x| x.params.smooth)
            .count(),
        usize::from(exposed)
    );
    let gpu = headless(&mut gpu_scene, &camera, options.clear, true).unwrap();
    let gpu_tags = crate::compare::tags_from_bytes(gpu.tags.as_ref().unwrap()).unwrap();
    assert!(f.instances.contains(&2) && gpu_tags.contains(&2), "marker");
    let report = crate::compare::compare(&crate::compare::Inputs {
        size: SIZE,
        cpu: &cpu,
        gpu: &gpu.color,
        tags: Some((&f.instances, &gpu_tags)),
        clear: crate::compare::clear_rgb(options.clear),
    })
    .unwrap();
    // The background (sky) of both, compared byte by byte.
    let mut sky_max = 0u8;
    let mut sky_pixels = 0u64;
    for (k, (c, g)) in f.instances.iter().zip(&gpu_tags).enumerate() {
        if *c == u32::MAX && *g == u32::MAX {
            sky_pixels += 1;
            for i in 0..3 {
                sky_max = sky_max.max(cpu[4 * k + i].abs_diff(gpu.color[4 * k + i]));
            }
        }
    }
    println!(
        "lit frame: agreement {:.6}, same tag {:?} of {}, mean {:?}, p95 {:?}, max {:?}; sky {} px, max {}",
        report.agreement(),
        report.same_tag,
        report.both,
        report.mean_abs,
        report.p95_abs,
        report.max_abs,
        sky_pixels,
        sky_max
    );
    assert!(sky_pixels > 1000, "{sky_pixels}");
    assert!(sky_max <= 2, "{sky_max}");
    assert!(report.agreement() > 0.999, "{}", report.agreement());
    assert!(
        report.same_tag.unwrap() as f64 > 0.999 * report.both as f64,
        "{:?}",
        report.same_tag
    );
    assert!(
        report.p95_abs.iter().all(|m| *m <= 2),
        "{:?}",
        report.p95_abs
    );
    // Every pixel drawn by both within a few steps (rounding of the
    // normal, the point and the light in f32 against the CPU's f64).
    assert!(
        report.max_abs.iter().all(|m| *m <= 8),
        "{:?}",
        report.max_abs
    );
}
