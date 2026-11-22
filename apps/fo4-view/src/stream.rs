//! The moving grid's bookkeeping, as pure functions and plain data: which
//! grid cell a point lies in, which cells to load and drop when the player
//! moves, and reference counts of data that several cells share.
//!
//! Loading the cells within `--around N` steps and dropping them past
//! N + 1 steps is a **viewer policy** (`spec/scene.md`, "The moving
//! grid"), chosen so that standing on a border does not load and drop the
//! same cells over and over. It claims nothing about which cells the game
//! keeps loaded or when.
use crate::scene::{GpuMesh, GpuTexture};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

/// The grid cell whose terrain footprint holds (x, y): x and y divided by
/// the cell size and rounded down, under the pending terrain placement
/// rule `land_grid_spacing_128_v1` (`spec/land.md`). `None` off the i32
/// grid or for a coordinate that is not finite.
pub fn cell_at(x: f64, y: f64) -> Option<[i32; 2]> {
    let one = |v: f64| {
        let c = (v / fo4_scene::terrain::CELL_SIZE).floor();
        (c.is_finite() && c >= f64::from(i32::MIN) && c <= f64::from(i32::MAX)).then_some(c as i32)
    };
    Some([one(x)?, one(y)?])
}

/// Grid steps between two cells along the farther axis.
pub fn steps(a: [i32; 2], b: [i32; 2]) -> u64 {
    let d = |k: usize| (i64::from(a[k]) - i64::from(b[k])).unsigned_abs();
    d(0).max(d(1))
}

/// Viewer policy: load the cells within `load` steps of the cell under
/// the player, drop loaded cells more than `drop` steps away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub load: u32,
    pub drop: u32,
}
impl Policy {
    /// `--around N`: load within N, drop past N + 1.
    pub fn around(n: u32) -> Self {
        Self {
            load: n,
            drop: n.saturating_add(1),
        }
    }
}

/// What to do when the cell under the player is `centre`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Loaded cells past the drop distance, in grid order.
    pub drop: Vec<[i32; 2]>,
    /// Cells within the load distance that are not loaded, nearest first
    /// (by steps, then by distance between centres, then south to north
    /// and west to east), so the cell under the player and its neighbours
    /// come before the outer ring.
    pub load: Vec<[i32; 2]>,
}

/// The plan for `centre` given the cells already `loaded`.
pub fn plan<'a>(
    centre: [i32; 2],
    loaded: impl IntoIterator<Item = &'a [i32; 2]>,
    policy: Policy,
) -> Plan {
    let loaded: std::collections::BTreeSet<[i32; 2]> = loaded.into_iter().copied().collect();
    let drop = loaded
        .iter()
        .filter(|g| steps(**g, centre) > u64::from(policy.drop))
        .copied()
        .collect();
    let r = i64::from(policy.load);
    let mut load: Vec<[i32; 2]> = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let (Ok(x), Ok(y)) = (
                i32::try_from(i64::from(centre[0]) + dx),
                i32::try_from(i64::from(centre[1]) + dy),
            ) else {
                continue;
            };
            if !loaded.contains(&[x, y]) {
                load.push([x, y]);
            }
        }
    }
    let order = |g: &[i32; 2]| {
        let dx = i64::from(g[0]) - i64::from(centre[0]);
        let dy = i64::from(g[1]) - i64::from(centre[1]);
        (steps(*g, centre), dx * dx + dy * dy, g[1], g[0])
    };
    load.sort_by_key(order);
    Plan { drop, load }
}

/// Values shared by several users, each kept while at least one user
/// holds it: [`Shared::acquire`] adds a user, [`Shared::release`] removes
/// one and hands the value back when it was the last.
#[derive(Debug)]
pub struct Shared<K, V> {
    map: BTreeMap<K, (u64, V)>,
}
impl<K, V> Default for Shared<K, V> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
}
impl<K: Ord + Clone, V> Shared<K, V> {
    /// One more user of `key` when it is held; false when it is not.
    pub fn acquire(&mut self, key: &K) -> bool {
        match self.map.get_mut(key) {
            Some((n, _)) => {
                *n += 1;
                true
            }
            None => false,
        }
    }
    /// `key` held by one user with `value`; a key already held only gains
    /// a user and `value` is handed back.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.acquire(&key) {
            return Some(value);
        }
        self.map.insert(key, (1, value));
        None
    }
    /// One user fewer; the value when that was the last user.
    pub fn release(&mut self, key: &K) -> Option<V> {
        let (n, _) = self.map.get_mut(key)?;
        *n -= 1;
        if *n > 0 {
            return None;
        }
        self.map.remove(key).map(|(_, v)| v)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn users(&self, key: &K) -> u64 {
        self.map.get(key).map_or(0, |(n, _)| *n)
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key).map(|(_, v)| v)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Identity of a mesh or texture by its content: two cells drawing the
/// same model or texture produce equal data and share one GPU copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetKey {
    /// 0 mesh, 1 texture.
    pub kind: u8,
    /// Bytes of the data.
    pub bytes: u64,
    /// Two 64-bit hashes of the content under different prefixes.
    pub hash: [u64; 2],
}

fn hashed(kind: u8, bytes: u64, feed: impl Fn(&mut std::hash::DefaultHasher)) -> AssetKey {
    let hash = [0u8, 1].map(|salt| {
        let mut h = std::hash::DefaultHasher::new();
        (salt, kind, bytes).hash(&mut h);
        feed(&mut h);
        h.finish()
    });
    AssetKey { kind, bytes, hash }
}

/// Bytes a mesh holds on the CPU before upload.
pub fn mesh_bytes(m: &GpuMesh) -> u64 {
    let n = m.positions.len() * 12
        + m.uvs.len() * 8
        + m.colors.as_ref().map_or(0, |c| c.len() * 16)
        + m.normals.as_ref().map_or(0, |n| n.len() * 12)
        + m.indices.len() * 2;
    n as u64
}

pub fn mesh_key(m: &GpuMesh) -> AssetKey {
    hashed(0, mesh_bytes(m), |h| {
        for p in &m.positions {
            p.map(f32::to_bits).hash(h);
        }
        for p in &m.uvs {
            p.map(f32::to_bits).hash(h);
        }
        match &m.colors {
            Some(c) => {
                1u8.hash(h);
                for p in c {
                    p.map(f32::to_bits).hash(h);
                }
            }
            None => 0u8.hash(h),
        }
        // Hashed only when present, so a mesh without normals keeps the
        // key it had before normals were carried.
        if let Some(n) = &m.normals {
            2u8.hash(h);
            for p in n {
                p.map(f32::to_bits).hash(h);
            }
        }
        m.indices.hash(h);
    })
}

pub fn texture_key(t: &GpuTexture) -> AssetKey {
    hashed(1, t.bytes.len() as u64, |h| {
        (t.width, t.height, t.levels, t.opaque).hash(h);
        format!("{:?}", t.format).hash(h);
        t.bytes.hash(h);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cell_under_a_point_rounds_down() {
        assert_eq!(cell_at(-80200., 85000.), Some([-20, 20]));
        assert_eq!(cell_at(0., -0.5), Some([0, -1]));
        assert_eq!(cell_at(4096., 4095.9), Some([1, 0]));
        assert_eq!(cell_at(f64::NAN, 0.), None);
        assert_eq!(cell_at(1e20, 0.), None);
        assert_eq!(steps([0, 0], [-2, 1]), 2);
    }

    fn square(c: [i32; 2], r: i32) -> Vec<[i32; 2]> {
        let mut v = Vec::new();
        for dy in -r..=r {
            for dx in -r..=r {
                v.push([c[0] + dx, c[1] + dy]);
            }
        }
        v
    }

    #[test]
    fn a_crossing_loads_the_new_row_and_keeps_the_old_one_until_the_next() {
        let p = Policy::around(1);
        // Start: nothing loaded; the centre first, then its ring.
        let start = plan([0, 0], &[], p);
        assert!(start.drop.is_empty());
        assert_eq!(start.load.len(), 9);
        assert_eq!(start.load[0], [0, 0]);
        assert_eq!(&start.load[1..5], &[[0, -1], [-1, 0], [1, 0], [0, 1]]);
        let mut loaded = start.load.clone();
        // One step east: the column at x = 2 loads, nothing drops (x = -1
        // is two steps away, within the drop distance).
        let east = plan([1, 0], &loaded, p);
        assert!(east.drop.is_empty());
        assert_eq!(east.load, vec![[2, 0], [2, -1], [2, 1]]);
        loaded.extend(&east.load);
        // A second step east drops the column at x = -1.
        let further = plan([2, 0], &loaded, p);
        assert_eq!(further.drop, vec![[-1, -1], [-1, 0], [-1, 1]]);
        assert_eq!(further.load, vec![[3, 0], [3, -1], [3, 1]]);
        // Radius 2: the plan from nothing is the 5 x 5 square.
        let mut r2 = plan([5, 5], &[], Policy::around(2)).load;
        r2.sort_unstable();
        let mut want = square([5, 5], 2);
        want.sort_unstable();
        assert_eq!(r2, want);
    }

    #[test]
    fn standing_on_a_border_does_not_load_and_drop_again() {
        let p = Policy::around(1);
        let mut loaded: Vec<[i32; 2]> = square([0, 0], 1);
        let mut loads = 0;
        let mut drops = 0;
        // Back and forth across the border between (0, 0) and (1, 0).
        for k in 0..20 {
            let centre = if k % 2 == 0 { [1, 0] } else { [0, 0] };
            let step = plan(centre, &loaded, p);
            loads += step.load.len();
            drops += step.drop.len();
            loaded.retain(|g| !step.drop.contains(g));
            loaded.extend(step.load);
        }
        // Only the first crossing loads (one column), nothing ever drops.
        assert_eq!((loads, drops), (3, 0));
        // Without the margin the same walk would thrash.
        let tight = Policy { load: 1, drop: 1 };
        let mut loaded = square([0, 0], 1);
        let mut drops = 0;
        for k in 0..20 {
            let centre = if k % 2 == 0 { [1, 0] } else { [0, 0] };
            let step = plan(centre, &loaded, tight);
            drops += step.drop.len();
            loaded.retain(|g| !step.drop.contains(g));
            loaded.extend(step.load);
        }
        assert_eq!(drops, 60);
    }

    #[test]
    fn a_shared_value_lives_while_any_user_holds_it() {
        let mut s: Shared<&str, u32> = Shared::default();
        assert!(!s.acquire(&"a"));
        assert_eq!(s.insert("a", 7), None);
        assert!(s.acquire(&"a"));
        assert_eq!(s.insert("a", 8), Some(8));
        assert_eq!(s.users(&"a"), 3);
        assert_eq!(s.release(&"a"), None);
        assert_eq!(s.release(&"a"), None);
        assert_eq!(s.get(&"a"), Some(&7));
        assert_eq!(s.release(&"a"), Some(7));
        assert!(s.is_empty());
        assert_eq!(s.release(&"a"), None);
    }

    #[test]
    fn equal_content_has_one_key_and_any_difference_another() {
        let mesh = GpuMesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            uvs: vec![[0., 0.]; 3],
            colors: None,
            normals: None,
            indices: vec![0, 1, 2],
        };
        let same = GpuMesh {
            positions: mesh.positions.clone(),
            uvs: mesh.uvs.clone(),
            colors: None,
            normals: None,
            indices: vec![0, 1, 2],
        };
        assert_eq!(mesh_key(&mesh), mesh_key(&same));
        let moved = GpuMesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 1.]],
            ..same
        };
        assert_ne!(mesh_key(&mesh), mesh_key(&moved));
        let turned = GpuMesh {
            normals: Some(vec![[0., 0., 1.]; 3]),
            positions: mesh.positions.clone(),
            uvs: mesh.uvs.clone(),
            colors: None,
            indices: vec![0, 1, 2],
        };
        assert_ne!(mesh_key(&mesh), mesh_key(&turned));
        let texture = |opaque, byte| GpuTexture {
            width: 4,
            height: 4,
            levels: 1,
            format: crate::scene::TexelFormat::Blocks(fo4_render::BlockFormat::Bc1),
            opaque,
            bytes: vec![byte; 8],
        };
        assert_eq!(
            texture_key(&texture(false, 1)),
            texture_key(&texture(false, 1))
        );
        assert_ne!(
            texture_key(&texture(true, 1)),
            texture_key(&texture(false, 1))
        );
        assert_ne!(
            texture_key(&texture(false, 2)),
            texture_key(&texture(false, 1))
        );
        assert_ne!(mesh_key(&mesh).kind, texture_key(&texture(false, 1)).kind);
    }
}
