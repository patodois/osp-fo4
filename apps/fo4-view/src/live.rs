//! The drawing and walking side of the moving grid: the cells received
//! from [`crate::cells`], their collision in one [`WorldSet`], and every
//! mesh and texture held once by content while any cell uses it
//! (`spec/scene.md`, "The moving grid").
//!
//! Messages are applied in the order they were sent, and the loading
//! side decides whether a key's data travels with a cell from the same
//! order, so the two sides agree on which keys are held without asking
//! each other. Only the GPU upload of a received cell waits for later
//! frames ([`Live::waiting`]); its collision is walkable at once.
use crate::cells::CellPiece;
use crate::gpu::SurfaceMaterial;
use crate::scene::{GpuMesh, GpuScene, GpuTexture, Instance, Surface};
use crate::stream::{AssetKey, Shared};
use bevy::prelude::{Entity, Handle, Image, Mesh};
use fo4_walk::WorldSet;
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

/// A mesh or texture: its data until it is uploaded, then its GPU handle.
pub enum Slot<D, H> {
    Waiting(D),
    Ready(H),
}

/// Identity of a drawn surface across cells: its texture by content, its
/// shader parameters by bits, its blend factors (as the CPU codes),
/// culling and overlay. Cells drawing equal surfaces share one material,
/// as the surfaces of one joined scene do (`crate::scene::convert`), so
/// their instances are drawn together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceId {
    pub texture: AssetKey,
    pub palette: Option<AssetKey>,
    pub params: [u32; crate::scene::PARAM_WORDS],
    pub blend: Option<(u32, u32)>,
    pub cull_back: bool,
    pub overlay: bool,
}
impl SurfaceId {
    pub fn of(surface: &Surface, texture: AssetKey, palette: Option<AssetKey>) -> Self {
        // The factors were made from these codes (`scene::blend_factor`).
        let code = |f| (0..=10u32).find(|c| crate::scene::blend_factor(*c) == Ok(f));
        Self {
            texture,
            palette,
            params: surface.params.bits(),
            blend: surface
                .key
                .blend
                .map(|(a, b)| (code(a).unwrap_or(u32::MAX), code(b).unwrap_or(u32::MAX))),
            cull_back: surface.key.cull_back,
            overlay: surface.key.overlay,
        }
    }
}

pub struct LiveCell {
    pub meshes: Vec<AssetKey>,
    pub textures: Vec<AssetKey>,
    /// Each key once, as acquired.
    keys: Vec<AssetKey>,
    pub surfaces: Vec<Surface>,
    /// Identity of each surface, for the materials shared by identity.
    pub surface_ids: Vec<SurfaceId>,
    pub instances: Vec<Instance>,
    /// Entities drawing it, once spawned.
    pub entities: Vec<Entity>,
    /// Spawned: its entities exist and it holds one user of the material
    /// of each of its surfaces.
    pub spawned: bool,
    pub received: Instant,
    /// Frames spent uploading.
    pub frames: u32,
}

#[derive(Default)]
pub struct Live {
    pub cells: BTreeMap<[i32; 2], LiveCell>,
    pub meshes: Shared<AssetKey, Slot<GpuMesh, Handle<Mesh>>>,
    pub textures: Shared<AssetKey, Slot<GpuTexture, Handle<Image>>>,
    /// The material of each surface identity a spawned cell draws.
    pub materials: Shared<SurfaceId, Handle<SurfaceMaterial>>,
    /// Received cells not spawned yet, in arrival order.
    pub waiting: VecDeque<[i32; 2]>,
}

impl Live {
    /// Whether the cell at `grid` has arrived (its collision, if any, is
    /// in the set); an empty grid position arrives as a cell without
    /// collision.
    pub fn arrived(&self, grid: [i32; 2]) -> bool {
        self.cells.contains_key(&grid)
    }

    /// Takes a loaded cell: its keys are held, its new data kept until
    /// upload and its collision added to `space`. A cell already present
    /// at the same grid is dropped first; its entities are returned.
    pub fn receive(
        &mut self,
        piece: CellPiece,
        space: &mut WorldSet<[i32; 2]>,
    ) -> Result<Vec<Entity>, String> {
        let CellPiece {
            grid,
            collision,
            meshes,
            textures,
            new_meshes,
            new_textures,
            surfaces,
            instances,
            ..
        } = piece;
        let old = self.drop_cell(grid, space);
        let mut keys: Vec<AssetKey> = Vec::new();
        for (k, data) in new_meshes {
            keys.push(k);
            // A key the loading side thought new is never held here; if
            // it were, the copy sent is dropped.
            self.meshes.insert(k, Slot::Waiting(data));
        }
        for (k, data) in new_textures {
            keys.push(k);
            self.textures.insert(k, Slot::Waiting(data));
        }
        for k in meshes.iter().chain(&textures) {
            if keys.contains(k) {
                continue;
            }
            let held = match k.kind {
                0 => self.meshes.acquire(k),
                _ => self.textures.acquire(k),
            };
            if !held {
                // Undo, so the two sides stay consistent for the others.
                for k in &keys {
                    self.release(k);
                }
                return Err(format!(
                    "StreamDataMissing: cell {grid:?} uses data no cell holds"
                ));
            }
            keys.push(*k);
        }
        if let Some(world) = collision {
            space.insert(grid, world).map_err(|e| e.to_string())?;
        }
        let mut surface_ids = Vec::with_capacity(surfaces.len());
        for s in &surfaces {
            let texture = textures.get(s.texture);
            let palette = s.palette.map(|p| textures.get(p));
            let (Some(texture), None | Some(Some(_))) = (texture, palette) else {
                for k in &keys {
                    self.release(k);
                }
                space.remove(&grid);
                return Err(format!(
                    "StreamSurfaceTexture: cell {grid:?} names texture {} (palette {:?})",
                    s.texture, s.palette
                ));
            };
            surface_ids.push(SurfaceId::of(s, *texture, palette.flatten().copied()));
        }
        self.cells.insert(
            grid,
            LiveCell {
                meshes,
                textures,
                keys,
                surfaces,
                surface_ids,
                instances,
                entities: Vec::new(),
                spawned: false,
                received: Instant::now(),
                frames: 0,
            },
        );
        self.waiting.push_back(grid);
        Ok(old)
    }

    fn release(&mut self, k: &AssetKey) {
        // The handle or data of the last user is dropped here.
        match k.kind {
            0 => drop(self.meshes.release(k)),
            _ => drop(self.textures.release(k)),
        }
    }

    /// Drops the cell at `grid`: its collision leaves `space`, its keys
    /// are released (data no other cell uses is freed) and its entities
    /// are returned for despawning.
    pub fn drop_cell(&mut self, grid: [i32; 2], space: &mut WorldSet<[i32; 2]>) -> Vec<Entity> {
        let Some(cell) = self.cells.remove(&grid) else {
            return Vec::new();
        };
        space.remove(&grid);
        self.waiting.retain(|g| *g != grid);
        for k in &cell.keys {
            self.release(k);
        }
        if cell.spawned {
            for id in &cell.surface_ids {
                // The last user's handle is dropped here.
                drop(self.materials.release(id));
            }
        }
        cell.entities
    }

    /// Every cell as one scene, in grid order, its meshes and textures
    /// moved out of their slots (headless: nothing was uploaded).
    pub fn take_scene(&mut self) -> Result<GpuScene, String> {
        let mut out = GpuScene::default();
        let mut meshes: BTreeMap<AssetKey, usize> = BTreeMap::new();
        let mut textures: BTreeMap<AssetKey, usize> = BTreeMap::new();
        for cell in self.cells.values() {
            let base = out.surfaces.len();
            for s in &cell.surfaces {
                let mut slot = |key: AssetKey| -> Result<usize, String> {
                    Ok(match textures.get(&key) {
                        Some(i) => *i,
                        None => {
                            let Some(Slot::Waiting(t)) = self.textures.get_mut(&key) else {
                                return Err("StreamTextureNotHeld".into());
                            };
                            let t = std::mem::replace(
                                t,
                                GpuTexture {
                                    bytes: Vec::new(),
                                    ..*t
                                },
                            );
                            out.textures.push(t);
                            textures.insert(key, out.textures.len() - 1);
                            out.textures.len() - 1
                        }
                    })
                };
                let index = slot(cell.textures[s.texture])?;
                let palette = match s.palette {
                    Some(p) => Some(slot(cell.textures[p])?),
                    None => None,
                };
                out.surfaces.push(Surface {
                    texture: index,
                    palette,
                    params: s.params,
                    key: s.key,
                });
            }
            for i in &cell.instances {
                let key = cell.meshes[i.mesh];
                let index = match meshes.get(&key) {
                    Some(m) => *m,
                    None => {
                        let Some(Slot::Waiting(m)) = self.meshes.get_mut(&key) else {
                            return Err("StreamMeshNotHeld".into());
                        };
                        out.meshes.push(std::mem::take(m));
                        meshes.insert(key, out.meshes.len() - 1);
                        out.meshes.len() - 1
                    }
                };
                out.instances.push(Instance {
                    mesh: index,
                    surface: base + i.surface,
                    ..*i
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::LoadReport;
    use crate::scene::{SurfaceKey, SurfaceParams, TexelFormat};
    use bevy::math::{Quat, Vec3};

    fn mesh(z: f32) -> GpuMesh {
        GpuMesh {
            positions: vec![[0., 0., z], [1., 0., z], [0., 1., z]],
            uvs: vec![[0., 0.]; 3],
            colors: None,
            normals: None,
            indices: vec![0, 1, 2],
        }
    }
    fn texture(byte: u8) -> GpuTexture {
        GpuTexture {
            width: 4,
            height: 4,
            levels: 1,
            format: TexelFormat::Blocks(fo4_render::BlockFormat::Bc1),
            opaque: false,
            bytes: vec![byte; 8],
        }
    }
    fn surface(texture: usize) -> Surface {
        Surface {
            texture,
            palette: None,
            params: SurfaceParams {
                uv_scale: [1.; 2],
                uv_offset: [0.; 2],
                test_function: crate::scene::TEST_ALWAYS,
                test_ref: 0.,
                opacity: 1.,
                skip_zero_alpha: false,
                opaque_texture: false,
                falloff: None,
                palette: 0,
            },
            key: SurfaceKey {
                blend: None,
                cull_back: false,
                overlay: false,
            },
        }
    }
    fn floor(x: f64) -> fo4_walk::World {
        let b = fo4_core::Budget::new(1 << 20);
        let mut w = fo4_walk::World::new();
        let (a, c, d, e) = (
            [x, 0., 0.],
            [x + 100., 0., 0.],
            [x + 100., 100., 0.],
            [x, 100., 0.],
        );
        w.add_triangles(&[[a, c, d], [a, d, e]], "floor", &b)
            .unwrap();
        w
    }
    /// A cell drawing `meshes` and `textures`, sending the data of the
    /// ones in `new`, as the loading side does.
    fn piece(grid: [i32; 2], meshes: &[f32], textures: &[u8], new: bool) -> CellPiece {
        let ms: Vec<GpuMesh> = meshes.iter().map(|z| mesh(*z)).collect();
        let ts: Vec<GpuTexture> = textures.iter().map(|b| texture(*b)).collect();
        let mk: Vec<AssetKey> = ms.iter().map(crate::stream::mesh_key).collect();
        let tk: Vec<AssetKey> = ts.iter().map(crate::stream::texture_key).collect();
        CellPiece {
            grid,
            collision: Some(floor(f64::from(grid[0]) * 100.)),
            new_meshes: if new {
                mk.iter().copied().zip(ms).collect()
            } else {
                Vec::new()
            },
            new_textures: if new {
                tk.iter().copied().zip(ts).collect()
            } else {
                Vec::new()
            },
            surfaces: (0..textures.len()).map(surface).collect(),
            instances: (0..meshes.len())
                .map(|k| Instance {
                    mesh: k,
                    surface: k % textures.len(),
                    tag: k as u32,
                    translation: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                })
                .collect(),
            meshes: mk,
            textures: tk,
            report: LoadReport::default(),
        }
    }

    #[test]
    fn shared_data_lives_while_any_cell_uses_it() {
        let mut live = Live::default();
        let mut space = WorldSet::new();
        live.receive(piece([0, 0], &[1., 2.], &[7], true), &mut space)
            .unwrap();
        // The second cell uses mesh 2 and texture 7 held for the first and
        // brings mesh 3.
        let mut second = piece([1, 0], &[2., 3.], &[7], false);
        let new = crate::stream::mesh_key(&mesh(3.));
        second.new_meshes = vec![(new, mesh(3.))];
        live.receive(second, &mut space).unwrap();
        assert_eq!((live.meshes.len(), live.textures.len()), (3, 1));
        let shared = crate::stream::mesh_key(&mesh(2.));
        assert_eq!(live.meshes.users(&shared), 2);
        assert_eq!(space.len(), 2);
        assert!(live.arrived([1, 0]));
        // Dropping the first keeps what the second uses.
        live.drop_cell([0, 0], &mut space);
        assert_eq!((live.meshes.len(), live.textures.len()), (2, 1));
        assert_eq!(live.meshes.users(&shared), 1);
        assert!(!live.arrived([0, 0]) && space.len() == 1);
        assert_eq!(live.waiting, [[1, 0]]);
        // The whole scene is the second cell's, with shared data in it.
        let scene = live.take_scene().unwrap();
        assert_eq!((scene.meshes.len(), scene.textures.len()), (2, 1));
        assert_eq!(scene.instances.len(), 2);
        assert_eq!(scene.textures[0].bytes, vec![7; 8]);
        live.drop_cell([1, 0], &mut space);
        assert!(live.meshes.is_empty() && live.textures.is_empty() && space.is_empty());
    }

    #[test]
    fn a_cell_using_data_nobody_holds_is_refused_and_changes_nothing() {
        let mut live = Live::default();
        let mut space = WorldSet::new();
        live.receive(piece([0, 0], &[1.], &[7], true), &mut space)
            .unwrap();
        let e = live
            .receive(piece([1, 0], &[1., 9.], &[7], false), &mut space)
            .unwrap_err();
        assert!(e.starts_with("StreamDataMissing"), "{e}");
        assert_eq!(live.meshes.users(&crate::stream::mesh_key(&mesh(1.))), 1);
        assert_eq!(
            live.textures
                .users(&crate::stream::texture_key(&texture(7))),
            1
        );
        assert!(!live.arrived([1, 0]) && space.len() == 1);
    }
}
