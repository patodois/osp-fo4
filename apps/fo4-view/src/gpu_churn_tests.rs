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
        params: SurfaceParams {
            uv_scale: [1.; 2],
            uv_offset: [0.; 2],
            test_function: TEST_ALWAYS,
            test_ref: 0.,
            opacity: 1.,
            skip_zero_alpha: false,
            opaque_texture: false,
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

/// A few frames, then the target's bytes once two captures in a row agree.
fn capture(app: &mut App, target: &Handle<Image>) -> Vec<u8> {
    for _ in 0..4 {
        frame(app);
    }
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
