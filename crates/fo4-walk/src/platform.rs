//! A moving platform the walker can stand on and ride (`spec/walk.md`,
//! Platforms), each reading a pending rule:
//!
//! - `walk_collision_marker_box_v1` ([`rules::COLLISION_MARKER_BOX`]): a
//!   placed reference whose base is a static with no model and which
//!   stores a 32-byte `XPRM` of kind word 1 is a solid box: half extents
//!   the three bound words times the reference's scale, centred on the
//!   reference, along its rotated axes (the reading `world_trigger_box_v1`
//!   gives a trigger's box), placed in the walking world as a hull
//!   ([`marker_boxes`], [`add_boxes`]). Its enable state is reported, not
//!   applied (the live world decides presence).
//! - `walk_platform_sequence_ride_v1` ([`rules::PLATFORM_RIDE`]): a piece
//!   of the walking world moves by what a model's sequence does to the
//!   node that carries its collision, from the second
//!   its caller says the sequence starts, and a walker standing on the
//!   piece moves with it ([`Ride`]). Which piece rides, which sequence
//!   plays and from when are the caller's: the run's animation graph knows
//!   the sequence and its start (`fo4_vm`'s world model), the timeline does
//!   not record it yet.
//! - `walk_collision_marker_box_presence_v1` ([`rules::BOX_PRESENCE`]): a
//!   collision marker's box stands in the walking world only while a frame
//!   of the run at that second has its reference present
//!   ([`presence`]: `fo4_scene::timeline::presence`, the run's own Enable
//!   or Disable, else its enable chain under the enable parent rules with
//!   the run's changes, else its initially-disabled flag); a box whose
//!   presence is undecided is left out.
//! - `walk_ride_rider_passes_terrain_v1` ([`rules::RIDE_PASSES_TERRAIN`]):
//!   while the walker stands on a ride's piece, the terrain's triangles do
//!   not touch it ([`Ride::pass_terrain`]): a platform whose shaft goes
//!   below the terrain carries its rider through it.
use crate::controller::Character;
use crate::geometry::V3;
use crate::set::WorldSet;
use crate::world::World;
use fo4_core::{Budget, Result};
use fo4_plugin::{typed::Store, FormKey, Record};
use fo4_scene::transform::Transform;

/// The platform rules' names in `spec/hypotheses.tsv`.
pub mod rules {
    pub const COLLISION_MARKER_BOX: &str = "walk_collision_marker_box_v1";
    pub const PLATFORM_RIDE: &str = "walk_platform_sequence_ride_v1";
    pub const BOX_PRESENCE: &str = "walk_collision_marker_box_presence_v1";
    pub const RIDE_PASSES_TERRAIN: &str = "walk_ride_rider_passes_terrain_v1";
}

/// A collision marker's box ([`rules::COLLISION_MARKER_BOX`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerBox {
    pub reference: FormKey,
    /// The placement as stored (position, then rotation in radians).
    pub placement: [f32; 6],
    pub scale: f32,
    /// The three bound words.
    pub half: [f32; 3],
    /// Whether the reference names an enable parent (XESP).
    pub enable_parent: bool,
    /// Whether its record flag 0x800 is set.
    pub initially_disabled: bool,
}

fn sub<'r>(r: &'r Record, sig: &[u8; 4]) -> Option<&'r [u8]> {
    r.subrecords
        .iter()
        .find(|s| s.signature == *sig)
        .map(|s| r.data(s))
}

fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    let w = d.get(at..at + 4)?;
    Some(f32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// The box of placed reference `key` when it is a collision marker's box,
/// else `None`.
pub fn marker_box(store: &Store, key: &FormKey, b: &Budget) -> Result<Option<MarkerBox>> {
    let Some((r, plugin)) = store.raw_with_plugin(key, b)? else {
        return Ok(None);
    };
    if r.signature != *b"REFR" {
        return Ok(None);
    }
    let Some(x) = sub(&r, b"XPRM").filter(|d| d.len() == 32) else {
        return Ok(None);
    };
    if u32::from_le_bytes([x[28], x[29], x[30], x[31]]) != 1 {
        return Ok(None);
    }
    let Some(name) = r.subrecords.iter().find(|s| s.signature == *b"NAME") else {
        return Ok(None);
    };
    let nd = r.data(name);
    let Some(w) = nd.get(..4) else {
        return Ok(None);
    };
    let Some(base) = store.order().remap(
        plugin,
        u32::from_le_bytes([w[0], w[1], w[2], w[3]]),
        &r.source.file,
        name.offset,
    )?
    else {
        return Ok(None);
    };
    let Some(br) = store.raw(&base, b)? else {
        return Ok(None);
    };
    if br.signature != *b"STAT" || sub(&br, b"MODL").is_some() {
        return Ok(None);
    }
    let Some(d) = sub(&r, b"DATA").filter(|d| d.len() == 24) else {
        return Ok(None);
    };
    let mut placement = [0f32; 6];
    for (i, p) in placement.iter_mut().enumerate() {
        *p = f32_at(d, 4 * i).unwrap_or(f32::NAN);
    }
    let half = [
        f32_at(x, 0).unwrap_or(f32::NAN),
        f32_at(x, 4).unwrap_or(f32::NAN),
        f32_at(x, 8).unwrap_or(f32::NAN),
    ];
    if placement.iter().chain(&half).any(|v| !v.is_finite()) || half.iter().any(|v| *v <= 0.) {
        return Ok(None);
    }
    let scale = sub(&r, b"XSCL").and_then(|d| f32_at(d, 0)).unwrap_or(1.);
    Ok(Some(MarkerBox {
        reference: key.clone(),
        placement,
        scale,
        half,
        enable_parent: sub(&r, b"XESP").is_some(),
        initially_disabled: r.flags & 0x800 != 0,
    }))
}

/// Every collision marker's box of the load order whose reference stands
/// within the axis box `lo`..`hi` (every placed reference is looked at).
pub fn marker_boxes(store: &Store, lo: V3, hi: V3, b: &Budget) -> Result<Vec<MarkerBox>> {
    let keys: Vec<FormKey> = store
        .placement_contexts()
        .filter(|(w, _)| w.signature == *b"REFR")
        .map(|(w, _)| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut out = Vec::new();
    for k in keys {
        let Some(m) = marker_box(store, &k, b)? else {
            continue;
        };
        let p = m.placement;
        if (0..3).all(|i| f64::from(p[i]) >= lo[i] && f64::from(p[i]) <= hi[i]) {
            out.push(m);
        }
    }
    Ok(out)
}

/// Whether the box's reference is present in `frame` of a run
/// ([`rules::BOX_PRESENCE`]): `fo4_scene::timeline::presence`; `None` when
/// the rules leave it undecided (or it has no winning record).
pub fn presence(
    store: &Store,
    frame: &fo4_scene::timeline::Frame,
    m: &MarkerBox,
    b: &Budget,
) -> Result<Option<bool>> {
    Ok(fo4_scene::timeline::presence(store, frame, &m.reference, b)?.present())
}

/// The eight corners of a box in world axes.
pub fn corners(m: &MarkerBox) -> Result<[V3; 8]> {
    let t = Transform::euler(m.placement, m.scale)?;
    let h = m.half.map(f64::from);
    Ok(std::array::from_fn(|k| {
        let s = |bit: usize| if k & (1 << bit) != 0 { 1. } else { -1. };
        t.apply([s(0) * h[0], s(1) * h[1], s(2) * h[2]])
    }))
}

/// Adds each box to `world` as a hull labelled with its reference;
/// returns the sources added.
pub fn add_boxes(world: &mut World, boxes: &[MarkerBox], b: &Budget) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for m in boxes {
        let label = format!(
            "collision marker box {}:{:08x} ({})",
            m.reference.plugin,
            m.reference.object_id,
            rules::COLLISION_MARKER_BOX
        );
        out.push(world.add_hull(&corners(m)?, 0., &label, b)?);
    }
    Ok(out)
}

/// A piece of a [`WorldSet`] that moves along a track of translations,
/// carrying a walker that stands on it ([`rules::PLATFORM_RIDE`]).
#[derive(Debug, Clone)]
pub struct Ride<K> {
    /// The piece's key in the set.
    pub key: K,
    /// The piece as built, unmoved.
    base: World,
    /// (second, translation) points, seconds increasing; before the first
    /// the first translation holds, after the last the last.
    pub track: Vec<(f64, V3)>,
    /// The translation the piece stands at now.
    pub offset: V3,
    /// How far the ride carried a walker in all, vertically.
    pub carried: f64,
    /// [`rules::RIDE_PASSES_TERRAIN`]: while the walker stands on this
    /// piece, the set's terrain sources are hidden. Off by default.
    pub pass_terrain: bool,
    /// The terrain sources this ride hid (piece key, source).
    hidden: Vec<(K, u32)>,
}

impl<K: Ord + Clone> Ride<K> {
    /// A ride of `base`, put into `set` under `key` at the track's offset at
    /// `second`.
    pub fn new(
        set: &mut WorldSet<K>,
        key: K,
        base: World,
        track: Vec<(f64, V3)>,
        second: f64,
        b: &Budget,
    ) -> Result<Self> {
        let mut r = Self {
            key,
            base,
            track,
            offset: [0.; 3],
            carried: 0.,
            pass_terrain: false,
            hidden: Vec::new(),
        };
        r.offset = r.offset_at(second);
        let moved = r.moved(r.offset, b)?;
        set.insert(r.key.clone(), moved)?;
        Ok(r)
    }

    /// The track's translation at `second` (straight between points).
    pub fn offset_at(&self, second: f64) -> V3 {
        let (Some(first), Some(last)) = (self.track.first(), self.track.last()) else {
            return [0.; 3];
        };
        if second <= first.0 {
            return first.1;
        }
        if second >= last.0 {
            return last.1;
        }
        for w in self.track.windows(2) {
            let (a, z) = (w[0], w[1]);
            if second < z.0 {
                let k = if z.0 > a.0 {
                    (second - a.0) / (z.0 - a.0)
                } else {
                    1.
                };
                return std::array::from_fn(|i| a.1[i] + k * (z.1[i] - a.1[i]));
            }
        }
        last.1
    }

    fn moved(&self, offset: V3, b: &Budget) -> Result<World> {
        let all: Vec<u32> = (0..self.base.sources.len() as u32).collect();
        self.base.copy_sources(&all, offset, b)
    }

    /// Moves the piece to its translation at `second`; a walker grounded on
    /// the piece moves by the same amount first. Returns the vertical move
    /// given to the walker (0 when it does not stand on the piece).
    pub fn advance(
        &mut self,
        set: &mut WorldSet<K>,
        walker: &mut Character,
        second: f64,
        b: &Budget,
    ) -> Result<f64> {
        let on = walker.grounded
            && walker
                .ground
                .and_then(|g| set.source(g.source))
                .is_some_and(|(k, _)| *k == self.key);
        if self.pass_terrain {
            self.hide_terrain(set, on);
        }
        let to = self.offset_at(second);
        if to == self.offset {
            return Ok(0.);
        }
        let delta: V3 = std::array::from_fn(|i| to[i] - self.offset[i]);
        let moved = self.moved(to, b)?;
        set.insert(self.key.clone(), moved)?;
        self.offset = to;
        if on {
            for (f, d) in walker.feet.iter_mut().zip(delta) {
                *f += d;
            }
            self.carried += delta[2];
            Ok(delta[2])
        } else {
            Ok(0.)
        }
    }

    /// Hides every terrain source of `set` (`hide`), or shows again those
    /// this ride hid.
    fn hide_terrain(&mut self, set: &mut WorldSet<K>, hide: bool) {
        if !hide {
            for (k, s) in self.hidden.drain(..) {
                set.set_hidden(&k, s, false);
            }
            return;
        }
        if !self.hidden.is_empty() {
            return;
        }
        let terrain: Vec<(K, u32)> = set
            .pieces()
            .flat_map(|(k, w, _)| {
                w.sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.terrain.is_some())
                    .map(|(i, _)| (k.clone(), i as u32))
                    .collect::<Vec<_>>()
            })
            .filter(|(k, s)| !set.is_hidden(k, *s))
            .collect();
        for (k, s) in &terrain {
            set.set_hidden(k, *s, true);
        }
        self.hidden = terrain;
    }
}
