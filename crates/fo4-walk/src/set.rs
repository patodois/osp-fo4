//! Collision the controller can walk in ([`Space`]): one [`World`], or a
//! [`WorldSet`] of worlds queried together, each under a key (for example
//! the grid of the cell it was built from), so that a caller can add and
//! remove one piece of collision without rebuilding the others
//! (`spec/walk.md`, "A set of worlds").
use crate::geometry::{Aabb, V3};
use crate::world::{Contact, RayHit, Source, World};
use fo4_core::{Error, Result};

/// What the controller needs from the collision it walks in.
pub trait Space {
    /// Every primitive the capsule `segment` widened by `radius + margin`
    /// touches ([`World::contacts`]), into `out` (cleared first).
    fn contacts(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        scratch: &mut Vec<u32>,
        out: &mut Vec<Contact>,
    );
    /// The source of entry `entry` as numbered by [`Space::contacts`].
    fn source_of(&self, entry: u32) -> u32;
    /// Box of every primitive, `None` when there is none.
    fn bounds(&self) -> Option<Aabb>;
}

impl Space for World {
    fn contacts(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        scratch: &mut Vec<u32>,
        out: &mut Vec<Contact>,
    ) {
        World::contacts(self, segment, radius, margin, scratch, out);
    }
    fn source_of(&self, entry: u32) -> u32 {
        self.entries[entry as usize].source
    }
    fn bounds(&self) -> Option<Aabb> {
        self.bounds
    }
}

/// Worlds queried as one, in increasing key order: entries and sources
/// are numbered across the worlds in that order (the first world's from
/// zero, then the next world's after them), so the same pieces give the
/// same contacts in the same order whatever order they were added in. A
/// set holding one world numbers exactly as that world does.
#[derive(Debug, Clone)]
pub struct WorldSet<K> {
    pieces: Vec<(K, World)>,
    /// First entry and first source of each piece.
    bases: Vec<(u32, u32)>,
    bounds: Option<Aabb>,
}

impl<K> Default for WorldSet<K> {
    fn default() -> Self {
        Self {
            pieces: Vec::new(),
            bases: Vec::new(),
            bounds: None,
        }
    }
}

impl<K: Ord> WorldSet<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `world` under `key`, returning the world it replaces. Fails,
    /// leaving the set unchanged, when the entries or sources of the set
    /// would no longer be numbered by a u32.
    pub fn insert(&mut self, key: K, world: World) -> Result<Option<World>> {
        let at = self.pieces.binary_search_by(|(k, _)| k.cmp(&key));
        let old = match at {
            Ok(i) => Some(std::mem::replace(&mut self.pieces[i].1, world)),
            Err(i) => {
                self.pieces.insert(i, (key, world));
                None
            }
        };
        if let Err(e) = self.renumber() {
            // Undo: put the old world back or take the new one out.
            match (at, old) {
                (Ok(i), Some(w)) => self.pieces[i].1 = w,
                (Err(i), _) => {
                    self.pieces.remove(i);
                }
                _ => {}
            }
            self.renumber()?;
            return Err(e);
        }
        Ok(old)
    }

    /// Takes out the world under `key`.
    pub fn remove(&mut self, key: &K) -> Option<World> {
        let i = self.pieces.binary_search_by(|(k, _)| k.cmp(key)).ok()?;
        let (_, w) = self.pieces.remove(i);
        // Fewer entries than before always fit.
        let _ = self.renumber();
        Some(w)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn get(&self, key: &K) -> Option<&World> {
        let i = self.pieces.binary_search_by(|(k, _)| k.cmp(key)).ok()?;
        Some(&self.pieces[i].1)
    }

    /// The keys, in increasing order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.pieces.iter().map(|(k, _)| k)
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Primitives in every world.
    pub fn entries(&self) -> usize {
        self.pieces.iter().map(|(_, w)| w.entries.len()).sum()
    }

    /// Source `source` as numbered across the set, with the key of its
    /// world.
    pub fn source(&self, source: u32) -> Option<(&K, &Source)> {
        let k = self
            .bases
            .partition_point(|b| b.1 <= source)
            .checked_sub(1)?;
        let (key, w) = &self.pieces[k];
        w.sources
            .get((source - self.bases[k].1) as usize)
            .map(|s| (key, s))
    }

    /// The highest surface at or below `z_from` on the vertical line
    /// through `(x, y)` in any world ([`World::ray_down`]), with its entry
    /// and source numbered across the set.
    pub fn ray_down(&self, x: f64, y: f64, z_from: f64) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        for ((_, w), (entry, source)) in self.pieces.iter().zip(&self.bases) {
            if let Some(h) = w.ray_down(x, y, z_from) {
                if best.is_none_or(|b| h.z > b.z) {
                    best = Some(RayHit {
                        entry: entry + h.entry,
                        source: source + h.source,
                        ..h
                    });
                }
            }
        }
        best
    }

    fn renumber(&mut self) -> Result<()> {
        let too_many = || Error::new("LimitExceeded", "walk world set", 0, "u32 entries", "more");
        let mut bases = Vec::with_capacity(self.pieces.len());
        let (mut entries, mut sources) = (0u32, 0u32);
        for (_, w) in &self.pieces {
            bases.push((entries, sources));
            let n = u32::try_from(w.entries.len()).map_err(|_| too_many())?;
            let s = u32::try_from(w.sources.len()).map_err(|_| too_many())?;
            entries = entries.checked_add(n).ok_or_else(too_many)?;
            sources = sources.checked_add(s).ok_or_else(too_many)?;
        }
        self.bases = bases;
        self.bounds = self
            .pieces
            .iter()
            .filter_map(|(_, w)| w.bounds)
            .reduce(Aabb::union);
        Ok(())
    }
}

impl<K: Ord> Space for WorldSet<K> {
    fn contacts(
        &self,
        segment: [V3; 2],
        radius: f64,
        margin: f64,
        scratch: &mut Vec<u32>,
        out: &mut Vec<Contact>,
    ) {
        out.clear();
        let Some(q) = Aabb::of(&segment, radius + margin) else {
            return;
        };
        for ((_, w), (entry, _)) in self.pieces.iter().zip(&self.bases) {
            if w.bounds.is_some_and(|b| b.overlaps(&q)) {
                w.push_contacts(segment, radius, margin, scratch, *entry, out);
            }
        }
    }
    fn source_of(&self, entry: u32) -> u32 {
        let k = self
            .bases
            .partition_point(|b| b.0 <= entry)
            .saturating_sub(1);
        let (e, s) = self.bases[k];
        s + self.pieces[k].1.entries[(entry - e) as usize].source
    }
    fn bounds(&self) -> Option<Aabb> {
        self.bounds
    }
}
