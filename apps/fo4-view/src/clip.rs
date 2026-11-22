//! `--actor-clip`: every drawn actor whose skeleton the clip's binding fits
//! is posed by the clip (`fo4_scene::actor::clip`, `spec/actor.md`, Clip
//! pose): at `--actor-clip-time` in a headless frame, looping in the
//! window, CPU-skinned again every frame on the main thread. Which clip an
//! actor plays in the game is behaviour-graph logic that is not read; the
//! clip is the one named on the command line.
use crate::load::AnyResult;
use bevy::prelude::*;
use fo4_core::{Budget, Limits};
use fo4_render::draw::SkinnedMesh;
use fo4_scene::actor::{
    clip::{self, Clip, HkSkeleton, Rig},
    pose::{Affine, Skeleton},
};
use fo4_scene::{DecodedAsset, Loader, RuleLevel};
use std::collections::BTreeMap;

/// The clip named by `--actor-clip` and the time of `--actor-clip-time`.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub path: String,
    pub time: f64,
}

/// One skeleton asset posed by the clip.
pub struct Posed {
    pub asset: usize,
    pub file: String,
    pub skeleton: HkSkeleton,
    pub rig: Rig,
}

/// What the window needs to pose the actors again at any time.
pub struct Animation {
    pub file: String,
    pub clip: Clip,
    pub skeletons: Vec<Posed>,
    pub meshes: Vec<SkinnedMesh>,
    pub start: f64,
}
impl Animation {
    /// The pose of every posed skeleton at clip time `t`.
    pub fn poses(&self, t: f64) -> fo4_core::Result<BTreeMap<usize, Skeleton>> {
        let mut out = BTreeMap::new();
        for p in &self.skeletons {
            let locals = self.clip.locals(&p.skeleton, t);
            let worlds = clip::worlds(&p.skeleton, &locals, &p.file)?;
            out.insert(p.asset, p.rig.pose(&worlds));
        }
        Ok(out)
    }
}

/// Reads the clip and, for each skeleton asset of the actors drawable at
/// `level`, the Havok skeleton beside it; returns the poses at `o.time` of
/// those the clip's binding fits, and prints a line for each skeleton.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    o: &Options,
    actors: &fo4_scene::actor::Actors,
    loader: &Loader<'_>,
    vfs: &fo4_vfs::Vfs,
    level: RuleLevel,
    limits: &Limits,
    budget: &Budget,
) -> AnyResult<(BTreeMap<usize, Skeleton>, Animation)> {
    let state = clip::state(budget)?;
    if !state.ready_at(level) {
        return Err(format!(
            "ActorClipRuleLevel: --actor-clip relies on the pending rules {:?} (--under-hypotheses)",
            state.hypotheses()
        )
        .into());
    }
    let key = fo4_core::path_key(&o.path)?;
    let scope = budget.child();
    let bytes = vfs.read(&key, &scope)?;
    let clip = Clip::read(&bytes, &key, &scope)?;
    drop(bytes);
    println!(
        "actor clip {key}: {:.3} s, {} frames, {} blocks, {} tracks, skeleton name {}; posed at {:.3} s",
        clip.duration(),
        clip.animation.frames,
        clip.animation.blocks.len(),
        clip.tracks.len(),
        String::from_utf8_lossy(&clip.skeleton_name),
        o.time,
    );
    let mut assets: Vec<usize> = actors
        .drawable(level)
        .filter_map(|a| a.plan.as_ref().map(|p| p.skeleton_asset))
        .collect();
    assets.sort_unstable();
    assets.dedup();
    let mut skeletons = Vec::new();
    for asset in assets {
        let nif = loader.assets.nodes[asset].path.clone();
        let nif_name = String::from_utf8_lossy(&nif).into_owned();
        let Some(hkx) = clip::skeleton_hkx_path(&nif) else {
            println!("  skeleton {nif_name}: not posed: not a .nif path");
            continue;
        };
        let hkx = String::from_utf8_lossy(&hkx).into_owned();
        let read = vfs
            .read(&hkx, &scope)
            .and_then(|b| HkSkeleton::read(&b, &hkx, &scope));
        let hk = match read {
            Ok(s) => s,
            Err(e) => {
                println!("  skeleton {nif_name}: not posed: {hkx}: {}", e.code);
                continue;
            }
        };
        if !clip.fits(&hk) {
            println!(
                "  skeleton {nif_name}: not posed: the clip's binding does not fit {hkx} ({} bones, name {})",
                hk.bones.len(),
                String::from_utf8_lossy(&hk.name)
            );
            continue;
        }
        let rig = loader.assets.with_decoded(asset, &budget.child(), |d| {
            let DecodedAsset::Model(n) = d else {
                return Err(fo4_core::Error::new(
                    "ActorClipSkeletonKind",
                    &nif_name,
                    0,
                    "model",
                    "other asset",
                ));
            };
            Rig::new(&n, &nif_name, &hk, limits, budget)
        })?;
        println!(
            "  skeleton {nif_name}: posed with {hkx}: {} bones, {} of them named in the NIF",
            hk.bones.len(),
            rig.matched()
        );
        skeletons.push(Posed {
            asset,
            file: hkx,
            skeleton: hk,
            rig,
        });
    }
    let animation = Animation {
        file: key,
        clip,
        skeletons,
        meshes: Vec::new(),
        start: o.time,
    };
    let poses = animation.poses(o.time)?;
    Ok((poses, animation))
}

/// One animated GPU mesh: the recorded skin and the mesh's other attributes.
struct Target {
    shape: usize,
    handle: Handle<Mesh>,
    uvs: Vec<[f32; 2]>,
    colors: Option<Vec<[f32; 4]>>,
    indices: Vec<u16>,
}

/// The window's clip loop, inserted once the scene is uploaded.
#[derive(Resource)]
pub struct Playing {
    animation: Animation,
    targets: Vec<Target>,
    started: Option<f64>,
    matrices: Vec<Affine>,
    positions: Vec<[f64; 3]>,
    /// Per report: frames, total and largest seconds spent posing and skinning.
    frames: u64,
    total: f64,
    largest: f64,
    vertices: usize,
}
impl Playing {
    /// The animated meshes of `scene` (`GpuScene::draw_meshes` maps the
    /// draw list's meshes) and their uploaded handles.
    pub fn new(
        animation: Animation,
        scene: &crate::scene::GpuScene,
        handles: &[Handle<Mesh>],
    ) -> Self {
        let mut targets = Vec::new();
        let mut vertices = 0;
        for (k, m) in animation.meshes.iter().enumerate() {
            let Some(&g) = scene.draw_meshes.get(&m.mesh) else {
                continue;
            };
            let (Some(gpu), Some(handle)) = (scene.meshes.get(g), handles.get(g)) else {
                continue;
            };
            vertices += m.shape.vertices.len();
            targets.push(Target {
                shape: k,
                handle: handle.clone(),
                uvs: gpu.uvs.clone(),
                colors: gpu.colors.clone(),
                indices: gpu.indices.clone(),
            });
        }
        println!(
            "actor clip: {} skinned meshes ({} vertices) posed every frame, looping {:.3} s",
            targets.len(),
            vertices,
            animation.clip.duration()
        );
        Self {
            animation,
            targets,
            started: None,
            matrices: Vec::new(),
            positions: Vec::new(),
            frames: 0,
            total: 0.0,
            largest: 0.0,
            vertices,
        }
    }
}

/// Frames between two lines of the skinning cost.
const REPORT_FRAMES: u64 = 60;

/// Each frame: the clip time (looping from `--actor-clip-time`), the pose of
/// each skeleton, then every animated mesh skinned again and replaced.
pub fn animate(
    time: Res<Time>,
    playing: Option<ResMut<Playing>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut p) = playing else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let started = *p.started.get_or_insert(now);
    let duration = p.animation.clip.duration();
    let t = if duration > 0.0 {
        (p.animation.start + now - started).rem_euclid(duration)
    } else {
        0.0
    };
    let clock = std::time::Instant::now();
    let poses = match p.animation.poses(t) {
        Ok(poses) => poses,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let Playing {
        animation,
        targets,
        matrices,
        positions,
        ..
    } = &mut *p;
    for target in targets.iter() {
        let m = &animation.meshes[target.shape];
        let Some(pose) = poses.get(&m.skeleton) else {
            continue;
        };
        if let Err(e) = m.shape.matrices(pose, &animation.file, matrices) {
            eprintln!("{e}");
            std::process::exit(2);
        }
        m.shape.positions(matrices, positions);
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
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, target.uvs.clone())
        .with_inserted_indices(bevy::mesh::Indices::U16(target.indices.clone()));
        if let Some(c) = &target.colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, c.clone());
        }
        if let Err(e) = meshes.insert(target.handle.id(), mesh) {
            eprintln!("ActorClipMesh: {e}");
            std::process::exit(2);
        }
    }
    let spent = clock.elapsed().as_secs_f64();
    p.frames += 1;
    p.total += spent;
    p.largest = p.largest.max(spent);
    if p.frames == REPORT_FRAMES {
        println!(
            "actor clip: posing and skinning {} vertices: mean {:.3} ms, largest {:.3} ms over {} frames (clip time {:.3} s)",
            p.vertices,
            1e3 * p.total / p.frames as f64,
            1e3 * p.largest,
            p.frames,
            t
        );
        p.frames = 0;
        p.total = 0.0;
        p.largest = 0.0;
    }
}
