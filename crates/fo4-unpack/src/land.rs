//! `land`: the LAND of one exterior cell (`fo4_plugin::land`,
//! `fo4_scene::terrain`, `spec/land.md`) as a report, with offline evidence
//! for the open terrain rules:
//!
//! - edge continuity: adjacent cells must agree on the heights of their
//!   shared edge. It is measured for the candidate accumulation and grid
//!   orientation and, as controls, for the transposed accumulation and the
//!   eight symmetries of the square;
//! - objects against ground: the lowest collision point of each placed
//!   reference in the cell and its eight neighbours against the terrain
//!   under it, for the candidate and for other height factors, orientations
//!   and diagonals.
//!
//! With `--census` every exterior cell of the cell's worldspace is read
//! (one LAND at a time; only the 1,093 stored height bytes of each are
//! kept) and the continuity of every shared edge is counted.
use crate::Reports;
use fo4_core::{Budget, Error, Manifest, Result};
use fo4_plugin::land::{Heights, Land, POINTS, SIDE};
use fo4_plugin::typed::{Cell, CellKind, Loaded, Store};
use fo4_plugin::FormKey;
use fo4_scene::bounds::Bounds;
use fo4_scene::terrain::{self, Diagonal, Outcome, CELL_SIZE, HEIGHT_UNIT, QUADS, SPACING};
use fo4_scene::{collision, hypotheses, Loader, RuleLevel};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

const LAST: usize = SIDE - 1;
/// Two edge heights agree when they differ by at most this many world units.
pub const EDGE_TOLERANCE: f64 = 1e-3;
/// Distinct values kept per raw-byte histogram before the rest is pooled.
const HISTOGRAM_KEYS: usize = 64;

/// How the stored signed bytes accumulate, in stored (a, b) order where the
/// stored index is `33 a + b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accumulation {
    /// Candidate (`land_vhgt_row_accumulation_v1`): b = 0 runs along a from
    /// the offset; every other value adds to its predecessor in b.
    Rows,
    /// Control: a = 0 runs along b from the offset; every other value adds
    /// to its predecessor in a.
    Columns,
}
impl Accumulation {
    pub const ALL: [Self; 2] = [Self::Rows, Self::Columns];
    fn name(self) -> &'static str {
        match self {
            Self::Rows => "rows",
            Self::Columns => "columns",
        }
    }
}

/// Accumulated VHGT units in stored order, or `None` for a non-finite offset.
pub fn stored_grid(h: &Heights, a: Accumulation) -> Option<Vec<f64>> {
    if h.gradient.len() != POINTS || !h.offset.is_finite() {
        return None;
    }
    let g = |a: usize, b: usize| f64::from(h.gradient[a * SIDE + b]);
    let mut out = vec![0.0; POINTS];
    match a {
        Accumulation::Rows => {
            let mut start = f64::from(h.offset);
            for i in 0..SIDE {
                start += g(i, 0);
                let mut along = start;
                out[i * SIDE] = along;
                for j in 1..SIDE {
                    along += g(i, j);
                    out[i * SIDE + j] = along;
                }
            }
        }
        Accumulation::Columns => {
            let mut start = f64::from(h.offset);
            for j in 0..SIDE {
                start += g(0, j);
                let mut along = start;
                out[j] = along;
                for i in 1..SIDE {
                    along += g(i, j);
                    out[i * SIDE + j] = along;
                }
            }
        }
    }
    Some(out)
}

/// One of the eight symmetries of the square, mapping stored (a, b) to the
/// world grid (east i, north j). Bit 0 swaps, bit 1 mirrors east-west,
/// bit 2 mirrors north-south. Symmetry 0 is the candidate
/// (`land_grid_origin_southwest_v1`): i = b, j = a.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Symmetry(pub u8);
impl Symmetry {
    pub const ALL: [Self; 8] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
    ];
    pub fn name(self) -> &'static str {
        [
            "candidate",
            "transposed",
            "mirror_x",
            "transposed_mirror_x",
            "mirror_y",
            "transposed_mirror_y",
            "rotated_180",
            "transposed_rotated_180",
        ][self.0 as usize & 7]
    }
    fn place(self, a: usize, b: usize) -> (usize, usize) {
        let (mut i, mut j) = if self.0 & 1 == 0 { (b, a) } else { (a, b) };
        if self.0 & 2 != 0 {
            i = LAST - i;
        }
        if self.0 & 4 != 0 {
            j = LAST - j;
        }
        (i, j)
    }
    /// World-oriented grid: index `33 j + i`, j north, i east.
    pub fn orient(self, stored: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; POINTS];
        for a in 0..SIDE {
            for b in 0..SIDE {
                let (i, j) = self.place(a, b);
                out[j * SIDE + i] = stored[a * SIDE + b];
            }
        }
        out
    }
}

/// Largest height difference along the shared edge of two world-oriented
/// grids, in VHGT units: `west`'s east column against `east`'s west column.
pub fn east_gap(west: &[f64], east: &[f64]) -> f64 {
    (0..SIDE)
        .map(|j| (west[j * SIDE + LAST] - east[j * SIDE]).abs())
        .fold(0.0, f64::max)
}
/// `south`'s north row against `north`'s south row.
pub fn north_gap(south: &[f64], north: &[f64]) -> f64 {
    (0..SIDE)
        .map(|i| (south[LAST * SIDE + i] - north[i]).abs())
        .fold(0.0, f64::max)
}

/// Height of a world-oriented grid (world units) at (x, y), the grid's
/// south-west corner at `origin`.
pub fn ground(oriented: &[f64], origin: [f64; 2], d: Diagonal, x: f64, y: f64) -> Option<f64> {
    let gx = (x - origin[0]) / SPACING;
    let gy = (y - origin[1]) / SPACING;
    let n = QUADS as f64;
    if !(0.0..=n).contains(&gx) || !(0.0..=n).contains(&gy) {
        return None;
    }
    let c = (gx.floor() as usize).min(QUADS - 1);
    let r = (gy.floor() as usize).min(QUADS - 1);
    let h = |r: usize, c: usize| oriented[r * SIDE + c];
    Some(terrain::quad_height(
        d,
        [h(r, c), h(r, c + 1), h(r + 1, c), h(r + 1, c + 1)],
        gx - c as f64,
        gy - r as f64,
    ))
}

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}
/// Counts a raw value, pooling values beyond the first [`HISTOGRAM_KEYS`].
fn bump_raw(m: &mut BTreeMap<String, u64>, k: String) {
    if m.len() < HISTOGRAM_KEYS || m.contains_key(&k) {
        bump(m, k);
    } else {
        bump(m, "other".into());
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn gap_bucket(units: f64) -> &'static str {
    let w = units * HEIGHT_UNIT;
    match w {
        w if w <= EDGE_TOLERANCE => "0:equal",
        w if w <= 8.0 => "1:<=8",
        w if w <= 64.0 => "2:8-64",
        w if w <= 512.0 => "3:64-512",
        _ => "4:>512",
    }
}
fn diff_bucket(v: f64) -> &'static str {
    match v {
        v if v < -256.0 => "00:<-256",
        v if v < -64.0 => "01:-256..-64",
        v if v < -16.0 => "02:-64..-16",
        v if v < -4.0 => "03:-16..-4",
        v if v < -1.0 => "04:-4..-1",
        v if v < 1.0 => "05:-1..1",
        v if v < 4.0 => "06:1..4",
        v if v < 16.0 => "07:4..16",
        v if v < 64.0 => "08:16..64",
        v if v < 256.0 => "09:64..256",
        _ => "10:>=256",
    }
}

/// Raw and decoded counts of LAND records.
#[derive(Default, Serialize)]
pub struct Layouts {
    pub records: u64,
    pub compressed: u64,
    pub subrecords_by_size: BTreeMap<String, u64>,
    pub uninterpreted: BTreeMap<String, u64>,
    pub with_heights: u64,
    pub with_normals: u64,
    pub with_colors: u64,
    pub base_layers: u64,
    pub alpha_layers: u64,
    pub alpha_points: u64,
    pub texture_entries: u64,
    pub non_finite_values: u64,
    pub data_raw: BTreeMap<String, u64>,
    pub vhgt_trailing: BTreeMap<String, u64>,
    pub btxt_raw: BTreeMap<String, u64>,
    pub base_quadrants: BTreeMap<u8, u64>,
    pub alpha_quadrants: BTreeMap<u8, u64>,
    pub atxt_unknown: BTreeMap<String, u64>,
    pub alpha_layer_numbers: BTreeMap<u16, u64>,
    pub vtxt_unknown: BTreeMap<String, u64>,
    pub vtxt_position_max: u16,
    pub vtxt_opacity_min: Option<f32>,
    pub vtxt_opacity_max: Option<f32>,
}
impl Layouts {
    fn add(&mut self, l: &Loaded<Land>) {
        let (r, land) = (&l.record, &l.data);
        self.records += 1;
        self.compressed += u64::from(r.payload_is_decompressed);
        for s in &r.subrecords {
            // VTXT sizes vary with the point count: keep only the multiple-of-8 test.
            let size = match (&s.signature, s.data.len()) {
                (b"VTXT", n) if n % 8 == 0 => "8n".to_owned(),
                (_, n) => n.to_string(),
            };
            bump(
                &mut self.subrecords_by_size,
                format!("{}:{size}", String::from_utf8_lossy(&s.signature)),
            );
        }
        for u in &land.uninterpreted {
            bump(
                &mut self.uninterpreted,
                format!(
                    "{}:{}",
                    String::from_utf8_lossy(&u.signature),
                    u.open_field.unwrap_or("unknown_signature")
                ),
            );
        }
        self.with_heights += u64::from(land.heights.is_some());
        self.with_normals += u64::from(land.normals.is_some());
        self.with_colors += u64::from(land.colors.is_some());
        self.base_layers += land.base_layers.len() as u64;
        self.alpha_layers += land.alpha_layers.len() as u64;
        self.texture_entries += land.textures.len() as u64;
        self.non_finite_values += land.non_finite_values;
        if let Some(d) = &land.data {
            bump_raw(&mut self.data_raw, hex(&d.raw));
        }
        if let Some(h) = &land.heights {
            bump_raw(&mut self.vhgt_trailing, hex(&h.trailing));
        }
        for b in &land.base_layers {
            bump_raw(&mut self.btxt_raw, hex(&b.raw));
            bump(&mut self.base_quadrants, b.quadrant);
        }
        for a in &land.alpha_layers {
            bump(&mut self.alpha_quadrants, a.header.quadrant);
            bump_raw(&mut self.atxt_unknown, hex(&[a.header.unknown]));
            bump(&mut self.alpha_layer_numbers, a.header.layer);
            self.alpha_points += a.points.len() as u64;
            for p in &a.points {
                bump_raw(&mut self.vtxt_unknown, hex(&p.unknown));
                self.vtxt_position_max = self.vtxt_position_max.max(p.position);
                if p.opacity.is_finite() {
                    self.vtxt_opacity_min = Some(
                        self.vtxt_opacity_min
                            .map_or(p.opacity, |m| m.min(p.opacity)),
                    );
                    self.vtxt_opacity_max = Some(
                        self.vtxt_opacity_max
                            .map_or(p.opacity, |m| m.max(p.opacity)),
                    );
                }
            }
        }
    }
}

/// Edge continuity under one (accumulation, symmetry) pair.
#[derive(Default, Clone, Serialize)]
pub struct Continuity {
    pub edges: u64,
    pub equal: u64,
    /// Edges between two cells whose heights are all equal: they agree
    /// under every pair and do not discriminate.
    pub flat_edges: u64,
    pub equal_not_flat: u64,
    /// World units (VHGT units times 8), candidate pair only.
    pub max_gap: f64,
    pub gaps: BTreeMap<&'static str, u64>,
}

/// Objects against ground under one variant.
#[derive(Default, Serialize)]
pub struct GroundFit {
    pub objects: u64,
    /// Objects whose box centre lies over no loaded terrain.
    pub off_terrain: u64,
    pub median: Option<f64>,
    pub median_abs: Option<f64>,
    pub within_16: u64,
    pub within_64: u64,
    pub histogram: BTreeMap<&'static str, u64>,
}
impl GroundFit {
    fn from(diffs: &mut [f64], off_terrain: u64) -> Self {
        let mut g = GroundFit {
            objects: diffs.len() as u64,
            off_terrain,
            ..Self::default()
        };
        for d in diffs.iter() {
            g.within_16 += u64::from(d.abs() <= 16.0);
            g.within_64 += u64::from(d.abs() <= 64.0);
            bump(&mut g.histogram, diff_bucket(*d));
        }
        diffs.sort_by(f64::total_cmp);
        g.median = diffs.get(diffs.len() / 2).copied();
        let mut abs: Vec<f64> = diffs.iter().map(|d| d.abs()).collect();
        abs.sort_by(f64::total_cmp);
        g.median_abs = abs.get(abs.len() / 2).copied();
        g
    }
}

#[derive(Default, Serialize)]
pub struct Census {
    pub world: Option<FormKey>,
    pub exterior_cells: u64,
    pub cells_without_grid: u64,
    pub duplicate_grids: u64,
    /// Grids claimed by more than one cell; neither cell is used.
    pub duplicate_grid_cells: Vec<[i32; 2]>,
    pub land_per_cell: BTreeMap<String, u64>,
    pub deleted_land: u64,
    pub land_group_kinds: BTreeMap<u32, u64>,
    pub xclc_low_flags: BTreeMap<u32, u64>,
    pub land_errors: BTreeMap<String, u64>,
    pub layouts: Layouts,
    pub cells_with_heights: u64,
    pub non_finite_offsets: u64,
    /// World units under the candidate rules.
    pub height_min: Option<f64>,
    pub height_max: Option<f64>,
    pub grid_min: Option<[i32; 2]>,
    pub grid_max: Option<[i32; 2]>,
    /// Key "accumulation/symmetry".
    pub continuity: BTreeMap<String, Continuity>,
    /// Diagonal neighbours share one corner vertex.
    pub corners: Continuity,
    /// Unequal candidate edges by whether both LAND winners come from the
    /// same plugin.
    pub unequal_edges_by_winner: BTreeMap<String, u64>,
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub cell: Option<FormKey>,
    pub editor_id: Option<String>,
    pub world: Option<FormKey>,
    pub grid: Option<[i32; 2]>,
    pub grid_flags: Option<u32>,
    pub level: Option<RuleLevel>,
    /// `fo4_scene::terrain::load` at the requested level.
    pub outcome: Option<String>,
    pub land: Option<FormKey>,
    pub layouts: Layouts,
    /// Candidate rules, world units.
    pub height_min: Option<f64>,
    pub height_max: Option<f64>,
    pub offset: Option<f32>,
    /// Neighbour position ("east", "north", ...) to its cell and, for the
    /// four edge neighbours, the largest gap under the candidate, world units.
    pub neighbours: BTreeMap<&'static str, serde_json::Value>,
    pub neighbour_continuity: BTreeMap<String, Continuity>,
    /// Key "variant/bottom".
    pub objects: BTreeMap<String, GroundFit>,
    pub object_cells: u64,
    pub census: Option<Census>,
    pub index_seconds: f64,
    pub objects_seconds: f64,
    pub census_seconds: f64,
}

/// An exterior cell of one worldspace.
struct WorldCell {
    key: FormKey,
    flags: u32,
}
/// Exterior cells of `world` by XCLC grid; also the census counters of the
/// index itself.
fn world_index(
    store: &Store,
    world: &FormKey,
    census: &mut Census,
    b: &Budget,
) -> Result<HashMap<(i32, i32), WorldCell>> {
    let mut out: HashMap<(i32, i32), WorldCell> = HashMap::new();
    let mut duplicates = Vec::new();
    for key in store.cells() {
        if store.kind(&key) != Some(CellKind::Exterior) || store.world(&key).as_ref() != Some(world)
        {
            continue;
        }
        census.exterior_cells += 1;
        let work = b.child();
        let Some(c) = store.get::<Cell>(&key, &work)? else {
            continue;
        };
        if c.record.deleted() {
            continue;
        }
        let Some(g) = &c.data.grid else {
            census.cells_without_grid += 1;
            continue;
        };
        bump(&mut census.xclc_low_flags, g.flags & 0xf);
        census.grid_min = Some(
            census
                .grid_min
                .map_or([g.i, g.j], |m| [m[0].min(g.i), m[1].min(g.j)]),
        );
        census.grid_max = Some(
            census
                .grid_max
                .map_or([g.i, g.j], |m| [m[0].max(g.i), m[1].max(g.j)]),
        );
        b.charge(64 + key.plugin.len() as u64, &key.plugin, 0)?;
        if out.contains_key(&(g.i, g.j)) {
            census.duplicate_grids += 1;
            duplicates.push((g.i, g.j));
        } else {
            out.try_reserve(1).map_err(|e| {
                Error::new("AllocationFailed", &key.plugin, 0, "world cell index", e)
            })?;
            out.insert(
                (g.i, g.j),
                WorldCell {
                    key: key.clone(),
                    flags: g.flags,
                },
            );
        }
    }
    // A grid claimed twice is ambiguous: neither cell is used for neighbours.
    for d in duplicates {
        out.remove(&d);
        if census.duplicate_grid_cells.len() < HISTOGRAM_KEYS {
            census.duplicate_grid_cells.push([d.0, d.1]);
        }
    }
    Ok(out)
}

/// The single winning LAND of a cell, decoded, plus how it was found.
struct CellLand {
    land: Option<Loaded<Land>>,
    candidates: usize,
    deleted: usize,
    group_kinds: Vec<u32>,
}
fn cell_land(store: &Store, cell: &FormKey, b: &Budget) -> Result<CellLand> {
    let lands = terrain::land_children(store, cell, b)?;
    let group_kinds = lands.candidates.iter().map(|c| c.group_kind).collect();
    let land = match lands.candidates.as_slice() {
        [one] => store.get_land(&one.key, b)?,
        _ => None,
    };
    Ok(CellLand {
        land,
        candidates: lands.candidates.len(),
        deleted: lands.deleted.len(),
        group_kinds,
    })
}

/// Stored grids of a cell under each accumulation, VHGT units.
type Grids = [Option<Vec<f64>>; 2];
fn grids(h: Option<&Heights>) -> Grids {
    Accumulation::ALL.map(|a| h.and_then(|h| stored_grid(h, a)))
}

fn continuity_key(a: Accumulation, s: Symmetry) -> String {
    format!("{}/{}", a.name(), s.name())
}
/// Adds the shared edge of `a` (west or south) and `b` to every pair.
fn add_edge(
    out: &mut BTreeMap<String, Continuity>,
    first: &Grids,
    second: &Grids,
    east: bool,
) -> Option<f64> {
    let flat = |g: &Grids| g[0].as_ref().is_some_and(|z| z.iter().all(|v| *v == z[0]));
    let both_flat = flat(first) && flat(second);
    let mut candidate = None;
    for (k, acc) in Accumulation::ALL.into_iter().enumerate() {
        let (Some(x), Some(y)) = (&first[k], &second[k]) else {
            continue;
        };
        for s in Symmetry::ALL {
            let (x, y) = (s.orient(x), s.orient(y));
            let gap = if east {
                east_gap(&x, &y)
            } else {
                north_gap(&x, &y)
            };
            let c = out.entry(continuity_key(acc, s)).or_default();
            let equal = gap * HEIGHT_UNIT <= EDGE_TOLERANCE;
            c.edges += 1;
            c.equal += u64::from(equal);
            c.flat_edges += u64::from(both_flat);
            c.equal_not_flat += u64::from(equal && !both_flat);
            if acc == Accumulation::Rows && s.0 == 0 {
                c.max_gap = c.max_gap.max(gap * HEIGHT_UNIT);
                bump(&mut c.gaps, gap_bucket(gap));
                candidate = Some(gap * HEIGHT_UNIT);
            }
        }
    }
    candidate
}

fn status_name(rule: &str) -> String {
    match hypotheses::status(rule) {
        Some(s) => format!("{s:?}").to_lowercase(),
        None => "unregistered".into(),
    }
}

pub fn run(
    m: &Manifest,
    editor_id: &str,
    r: &mut Reports,
    level: RuleLevel,
    census: bool,
) -> Result<()> {
    r.items_enabled = true;
    r.summary.rule_level = Some(level);
    let started = std::time::Instant::now();
    let store = Store::open(m, &Budget::new(m.limits.max_override_memory))?;
    for error in &store.diagnostics {
        r.error(error)?;
    }
    let budget = Budget::new(m.limits.max_memory);
    let key = store.find_cell(editor_id, &budget)?.ok_or_else(|| {
        Error::new(
            "MissingCellEditorId",
            "plugins",
            0,
            "CELL EditorID",
            editor_id,
        )
    })?;
    let world = store.world(&key).ok_or_else(|| {
        Error::new(
            "NotWorldCell",
            &key.plugin,
            0,
            "exterior CELL inside a world children group",
            editor_id,
        )
    })?;
    let mut s = Summary {
        cell: Some(key.clone()),
        editor_id: Some(editor_id.to_owned()),
        world: Some(world.clone()),
        level: Some(level),
        ..Summary::default()
    };
    for rule in terrain::RULES
        .into_iter()
        .chain(Diagonal::ALL.map(Diagonal::rule))
    {
        r.item(&serde_json::json!({"kind":"rule","rule":rule,"status":status_name(rule)}))?;
    }
    let outcome = terrain::load(
        &store,
        &key,
        terrain::Options {
            level,
            diagonal: None,
        },
        &budget,
    )?;
    s.outcome = Some(match &outcome {
        Outcome::Built(t) => format!("built:{}", t.diagonal.name()),
        Outcome::Refused(f) => format!("refused:{}", f.code),
    });
    let mut index_census = Census::default();
    let index = world_index(&store, &world, &mut index_census, &budget)?;
    s.index_seconds = started.elapsed().as_secs_f64();
    let me = index
        .iter()
        .find(|(_, c)| c.key == key)
        .map(|(g, c)| (*g, c.flags))
        .ok_or_else(|| {
            Error::new(
                "CellWithoutUniqueGrid",
                &key.plugin,
                0,
                "unique XCLC grid",
                editor_id,
            )
        })?;
    let ((gx, gy), flags) = me;
    s.grid = Some([gx, gy]);
    s.grid_flags = Some(flags);

    // The cell and its eight neighbours.
    const AROUND: [(&str, i32, i32); 9] = [
        ("self", 0, 0),
        ("east", 1, 0),
        ("west", -1, 0),
        ("north", 0, 1),
        ("south", 0, -1),
        ("north_east", 1, 1),
        ("north_west", -1, 1),
        ("south_east", 1, -1),
        ("south_west", -1, -1),
    ];
    let mut block: HashMap<(i32, i32), (FormKey, Grids)> = HashMap::new();
    for (name, dx, dy) in AROUND {
        let at = (gx + dx, gy + dy);
        let Some(c) = index.get(&at) else {
            s.neighbours
                .insert(name, serde_json::json!({"grid":[at.0,at.1],"cell":null}));
            continue;
        };
        let work = budget.child();
        let found = cell_land(&store, &c.key, &work)?;
        let heights = found.land.as_ref().and_then(|l| l.data.heights.as_ref());
        let g = grids(heights);
        if name == "self" {
            if let Some(l) = &found.land {
                s.land = Some(l.key.clone());
                s.layouts.add(l);
                r.item(&serde_json::json!({"kind":"land","cell":c.key,"land":l.key,"subrecords":l.record.subrecords.iter().map(|x|(String::from_utf8_lossy(&x.signature).into_owned(),x.data.len())).collect::<Vec<_>>(),"data":l.data.data,"base_layers":l.data.base_layers,"alpha_layers":l.data.alpha_layers.iter().map(|a|serde_json::json!({"header":a.header,"points":a.points.len()})).collect::<Vec<_>>(),"textures":l.data.textures,"uninterpreted":l.data.uninterpreted,"vhgt_trailing":heights.map(|h|h.trailing)}))?;
            }
            s.offset = heights.map(|h| h.offset);
            if let Some(z) = &g[0] {
                s.height_min = z.iter().copied().reduce(f64::min).map(|v| v * HEIGHT_UNIT);
                s.height_max = z.iter().copied().reduce(f64::max).map(|v| v * HEIGHT_UNIT);
            }
        }
        s.neighbours.insert(
            name,
            serde_json::json!({"grid":[at.0,at.1],"cell":c.key,"land":found.land.as_ref().map(|l|&l.key),"land_candidates":found.candidates,"deleted_land":found.deleted,"group_kinds":found.group_kinds,"heights":g[0].is_some()}),
        );
        b_insert(&mut block, at, (c.key.clone(), g), &budget)?;
    }
    let mine = block.get(&(gx, gy)).map(|(_, g)| g);
    for (name, dx, dy, east, mine_first) in [
        ("east", 1, 0, true, true),
        ("west", -1, 0, true, false),
        ("north", 0, 1, false, true),
        ("south", 0, -1, false, false),
    ] {
        let (Some(mine), Some((_, other))) = (mine, block.get(&(gx + dx, gy + dy))) else {
            continue;
        };
        let gap = if mine_first {
            add_edge(&mut s.neighbour_continuity, mine, other, east)
        } else {
            add_edge(&mut s.neighbour_continuity, other, mine, east)
        };
        if let Some(v) = s.neighbours.get_mut(name).and_then(|v| v.as_object_mut()) {
            v.insert("candidate_gap".into(), serde_json::json!(gap));
        }
    }

    let started = std::time::Instant::now();
    objects(m, &store, &block, &mut s, r, &budget)?;
    s.objects_seconds = started.elapsed().as_secs_f64();

    if census {
        let started = std::time::Instant::now();
        let mut c = index_census;
        c.world = Some(world.clone());
        world_census(&store, &index, &mut c, r, &budget)?;
        s.census_seconds = started.elapsed().as_secs_f64();
        s.census = Some(c);
    }
    r.summary.files_attempted = s.object_cells;
    r.summary.files_valid = s.object_cells;
    r.tsv = Some(("land.tsv", tsv(&s)));
    r.summary.land = Some(s);
    Ok(())
}

fn b_insert<K: std::hash::Hash + Eq, V>(
    m: &mut HashMap<K, V>,
    k: K,
    v: V,
    b: &Budget,
) -> Result<()> {
    b.charge(
        (std::mem::size_of::<V>() + 2 * POINTS * 8) as u64,
        "land",
        0,
    )?;
    m.try_reserve(1)
        .map_err(|e| Error::new("AllocationFailed", "land", 0, "cell block", e))?;
    m.insert(k, v);
    Ok(())
}

/// Every exterior cell of the world: LAND counts and layouts, and the
/// continuity of every shared edge and corner.
fn world_census(
    store: &Store,
    index: &HashMap<(i32, i32), WorldCell>,
    c: &mut Census,
    r: &mut Reports,
    b: &Budget,
) -> Result<()> {
    // Stored heights only: offset and 1,089 signed bytes per cell.
    let mut heights: HashMap<(i32, i32), (Heights, String)> = HashMap::new();
    let mut cells: Vec<(&(i32, i32), &WorldCell)> = index.iter().collect();
    cells.sort_by_key(|(g, _)| **g);
    for (grid, cell) in cells {
        let work = b.child();
        let found = match cell_land(store, &cell.key, &work) {
            Ok(f) => f,
            Err(e) => {
                bump(&mut c.land_errors, e.code.clone());
                r.error(&e)?;
                continue;
            }
        };
        bump(
            &mut c.land_per_cell,
            match found.candidates {
                0 => "0".into(),
                1 => "1".into(),
                _ => "2+".into(),
            },
        );
        c.deleted_land += found.deleted as u64;
        for k in &found.group_kinds {
            bump(&mut c.land_group_kinds, *k);
        }
        let Some(l) = found.land else {
            continue;
        };
        c.layouts.add(&l);
        let Some(h) = l.data.heights else {
            continue;
        };
        if !h.offset.is_finite() {
            c.non_finite_offsets += 1;
            continue;
        }
        c.cells_with_heights += 1;
        if let Some(z) = stored_grid(&h, Accumulation::Rows) {
            let lo = z.iter().copied().fold(f64::INFINITY, f64::min) * HEIGHT_UNIT;
            let hi = z.iter().copied().fold(f64::NEG_INFINITY, f64::max) * HEIGHT_UNIT;
            c.height_min = Some(c.height_min.map_or(lo, |m| m.min(lo)));
            c.height_max = Some(c.height_max.map_or(hi, |m| m.max(hi)));
        }
        b.charge(64 + POINTS as u64, "land census", 0)?;
        heights
            .try_reserve(1)
            .map_err(|e| Error::new("AllocationFailed", "land census", 0, "heights", e))?;
        let plugin = store.order().headers()[l.plugin_index].name.clone();
        b.charge(plugin.len() as u64, "land census", 0)?;
        heights.insert(*grid, (h, plugin));
    }
    let mut keys: Vec<(i32, i32)> = heights.keys().copied().collect();
    keys.sort_unstable();
    for (x, y) in keys {
        let (mine, my_plugin) = &heights[&(x, y)];
        let here = grids(Some(mine));
        for (dx, dy, east) in [(1, 0, true), (0, 1, false)] {
            if let Some((other, plugin)) = heights.get(&(x + dx, y + dy)) {
                let gap = add_edge(&mut c.continuity, &here, &grids(Some(other)), east);
                if let Some(gap) = gap.filter(|g| *g > EDGE_TOLERANCE) {
                    let same = |p: &str| if p == my_plugin { "same" } else { "different" };
                    bump(&mut c.unequal_edges_by_winner, same(plugin).to_owned());
                    r.item(&serde_json::json!({"kind":"edge_gap","grid":[x,y],"neighbour":[x+dx,y+dy],"gap":gap,"land_plugins":[my_plugin,plugin]}))?;
                }
            }
        }
        // Corner shared with the north-east neighbour, candidate only.
        if let (Some(a), Some((other, _))) = (&here[0], heights.get(&(x + 1, y + 1))) {
            if let Some(o) = stored_grid(other, Accumulation::Rows) {
                let gap = (a[LAST * SIDE + LAST] - o[0]).abs();
                c.corners.edges += 1;
                c.corners.equal += u64::from(gap * HEIGHT_UNIT <= EDGE_TOLERANCE);
                c.corners.max_gap = c.corners.max_gap.max(gap * HEIGHT_UNIT);
                bump(&mut c.corners.gaps, gap_bucket(gap));
            }
        }
    }
    Ok(())
}

/// One height variant for the objects check.
#[derive(Clone, Copy)]
struct Variant {
    symmetry: Symmetry,
    factor: f64,
    diagonal: Diagonal,
}
impl Variant {
    fn name(self) -> String {
        format!(
            "{}/x{}/{}",
            self.symmetry.name(),
            self.factor,
            self.diagonal.name()
        )
    }
}
fn variants() -> Vec<Variant> {
    let mut out = Vec::new();
    for symmetry in Symmetry::ALL {
        for factor in [4.0, 8.0, 16.0] {
            out.push(Variant {
                symmetry,
                factor,
                diagonal: Diagonal::SouthwestNortheast,
            });
        }
    }
    out.push(Variant {
        symmetry: Symmetry(0),
        factor: HEIGHT_UNIT,
        diagonal: Diagonal::NorthwestSoutheast,
    });
    out
}

/// Per reference: collision box, render box of its placed models, origin.
type Boxes = (Option<Bounds>, Option<Bounds>, [f64; 3]);

/// Placed references of the block's cells against the terrain under them.
fn objects(
    m: &Manifest,
    store: &Store,
    block: &HashMap<(i32, i32), (FormKey, Grids)>,
    s: &mut Summary,
    r: &mut Reports,
    b: &Budget,
) -> Result<()> {
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    vfs.check_complete()?;
    // Evidence only: the scene and collision rules are evaluated under
    // hypotheses so that every placed reference with collision is seen.
    let level = RuleLevel::Hypotheses;
    let cache = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(store, &vfs, &m.language, level, &cache);
    let variants = variants();
    // Oriented and scaled ground grids per variant and cell.
    let mut oriented: Vec<HashMap<(i32, i32), Vec<f64>>> = Vec::new();
    for v in &variants {
        let mut per = HashMap::new();
        for (g, (_, grids)) in block {
            if let Some(z) = &grids[0] {
                let mut o = v.symmetry.orient(z);
                for h in &mut o {
                    *h *= v.factor;
                }
                b.charge((POINTS * 8) as u64, "land objects", 0)?;
                per.insert(*g, o);
            }
        }
        oriented.push(per);
    }
    let ground_at = |k: usize, x: f64, y: f64| -> Option<f64> {
        let cx = (x / CELL_SIZE).floor() as i32;
        let cy = (y / CELL_SIZE).floor() as i32;
        let grid = oriented[k].get(&(cx, cy))?;
        ground(
            grid,
            [f64::from(cx) * CELL_SIZE, f64::from(cy) * CELL_SIZE],
            variants[k].diagonal,
            x,
            y,
        )
    };
    let candidate = variants
        .iter()
        .position(|v| {
            v.symmetry.0 == 0
                && v.factor == HEIGHT_UNIT
                && v.diagonal == Diagonal::SouthwestNortheast
        })
        .unwrap_or(0);
    // Per variant and bottom kind: differences bottom - ground.
    let mut diffs: BTreeMap<String, (Vec<f64>, u64)> = BTreeMap::new();
    let mut cells: Vec<_> = block.iter().collect();
    cells.sort_by_key(|(g, _)| **g);
    for (grid, (key, _)) in cells {
        loader.reset_asset_cache(&cache);
        let work = b.child();
        let scene = match loader.cell(key, &work) {
            Ok(scene) => scene,
            Err(e) => {
                r.error(&e)?;
                continue;
            }
        };
        let options = collision::Options {
            level,
            body_placement: None,
        };
        let c = collision::cell(&scene, &loader, &m.limits, options, &work)?;
        s.object_cells += 1;
        let v = c
            .body_placement
            .unwrap_or(collision::BodyPlacement::Ignored);
        let mut refs: BTreeMap<(String, u32), Boxes> = BTreeMap::new();
        let union = |a: Option<Bounds>, b: Bounds| -> Bounds {
            match a {
                None => b,
                Some(a) => Bounds {
                    min: std::array::from_fn(|k| a.min[k].min(b.min[k])),
                    max: std::array::from_fn(|k| a.max[k].max(b.max[k])),
                },
            }
        };
        for p in &c.placements {
            let e = refs
                .entry((p.reference.plugin.clone(), p.reference.object_id))
                .or_insert((None, None, p.frame.translation));
            for q in &c.models[p.model].render_points {
                let w = p.frame.apply(*q);
                if w.iter().all(|x| x.is_finite()) {
                    e.1 = Some(union(e.1, Bounds { min: w, max: w }));
                }
            }
        }
        for i in &c.instances {
            if let Some(bx) = c.world_bounds(i, v) {
                let p = c.placement(i);
                if let Some(e) = refs.get_mut(&(p.reference.plugin.clone(), p.reference.object_id))
                {
                    e.0 = Some(union(e.0, bx));
                }
            }
        }
        for ((plugin, id), (collision_box, render_box, origin)) in &refs {
            let Some(cb) = collision_box else {
                continue;
            };
            let (cx, cy) = (0.5 * (cb.min[0] + cb.max[0]), 0.5 * (cb.min[1] + cb.max[1]));
            let mut record = serde_json::Map::new();
            for (k, var) in variants.iter().enumerate() {
                let g = ground_at(k, cx, cy);
                let bottoms = [
                    ("collision", Some(cb.min[2])),
                    ("render", render_box.map(|b| b.min[2])),
                    ("origin", Some(origin[2])),
                ];
                for (name, bottom) in bottoms {
                    if k != candidate && name != "collision" {
                        continue;
                    }
                    let Some(bottom) = bottom else { continue };
                    let e = diffs.entry(format!("{}/{name}", var.name())).or_default();
                    match g {
                        Some(g) => e.0.push(bottom - g),
                        None => e.1 += 1,
                    }
                    if k == candidate {
                        record.insert(name.into(), serde_json::json!(bottom));
                        record.insert(
                            format!("{name}_minus_ground"),
                            serde_json::json!(g.map(|g| bottom - g)),
                        );
                    }
                }
                if k == candidate {
                    record.insert("ground".into(), serde_json::json!(g));
                }
            }
            r.item(&serde_json::json!({"kind":"object_ground","cell_grid":[grid.0,grid.1],"reference":{"plugin":plugin,"object_id":id},"centre":[cx,cy],"candidate":record}))?;
        }
    }
    for (k, (mut d, off)) in diffs {
        s.objects.insert(k, GroundFit::from(&mut d, off));
    }
    Ok(())
}

fn tsv(s: &Summary) -> String {
    let mut out = String::from("# section\tkey\tvalue\n");
    let mut row = |section: &str, key: &str, value: String| {
        out.push_str(&format!("{section}\t{key}\t{value}\n"));
    };
    row("cell", "editor_id", s.editor_id.clone().unwrap_or_default());
    row("cell", "grid", format!("{:?}", s.grid));
    row("cell", "outcome", s.outcome.clone().unwrap_or_default());
    row("cell", "height_min", format!("{:?}", s.height_min));
    row("cell", "height_max", format!("{:?}", s.height_max));
    for (k, c) in &s.neighbour_continuity {
        row("neighbour_edges", k, format!("{}/{}", c.equal, c.edges));
    }
    for (k, g) in &s.objects {
        row(
            "objects",
            k,
            format!(
                "n={} median={:?} median_abs={:?} within16={} within64={} off_terrain={}",
                g.objects, g.median, g.median_abs, g.within_16, g.within_64, g.off_terrain
            ),
        );
    }
    if let Some(c) = &s.census {
        for (k, v) in [
            ("exterior_cells", c.exterior_cells),
            ("cells_without_grid", c.cells_without_grid),
            ("duplicate_grids", c.duplicate_grids),
            ("deleted_land", c.deleted_land),
            ("land_records", c.layouts.records),
            ("cells_with_heights", c.cells_with_heights),
            ("non_finite_offsets", c.non_finite_offsets),
        ] {
            row("census", k, v.to_string());
        }
        row("census", "height_min", format!("{:?}", c.height_min));
        row("census", "height_max", format!("{:?}", c.height_max));
        for (k, v) in &c.land_per_cell {
            row("land_per_cell", k, v.to_string());
        }
        for (k, v) in &c.layouts.subrecords_by_size {
            row("subrecord_size", k, v.to_string());
        }
        for (k, v) in &c.layouts.uninterpreted {
            row("uninterpreted", k, v.to_string());
        }
        for (k, cont) in &c.continuity {
            row(
                "edges",
                k,
                format!(
                    "{}/{} equal; {} flat; {} equal and not flat",
                    cont.equal, cont.edges, cont.flat_edges, cont.equal_not_flat
                ),
            );
        }
        row(
            "corners",
            "candidate",
            format!("{}/{}", c.corners.equal, c.corners.edges),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights(offset: f32, g: impl Fn(usize, usize) -> i8) -> Heights {
        Heights {
            subrecord: 0,
            offset,
            gradient: (0..POINTS).map(|i| g(i / SIDE, i % SIDE)).collect(),
            trailing: [0; 3],
        }
    }

    #[test]
    fn accumulations_differ_only_off_the_first_row_and_column() {
        let h = heights(1.0, |a, b| (a * 3 + b) as i8 % 5 - 2);
        let rows = stored_grid(&h, Accumulation::Rows).unwrap();
        let columns = stored_grid(&h, Accumulation::Columns).unwrap();
        assert_eq!(rows[0], columns[0]);
        assert_ne!(rows, columns);
        let t = fo4_scene::terrain::accumulate(&h, &Budget::new(1 << 16)).unwrap();
        assert_eq!(rows, t);
        let mut bad = h.clone();
        bad.offset = f32::NAN;
        assert!(stored_grid(&bad, Accumulation::Rows).is_none());
    }

    #[test]
    fn symmetries_are_distinct_and_the_candidate_is_the_identity() {
        let stored: Vec<f64> = (0..POINTS).map(|i| i as f64).collect();
        assert_eq!(Symmetry(0).orient(&stored), stored);
        let mut seen: Vec<Vec<f64>> = Symmetry::ALL.iter().map(|s| s.orient(&stored)).collect();
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
        seen.dedup();
        assert_eq!(seen.len(), 8);
        // Transposed: stored (a, b) goes to east a, north b.
        assert_eq!(
            Symmetry(1).orient(&stored)[2 * SIDE + 5],
            (5 * SIDE + 2) as f64
        );
    }

    #[test]
    fn continuity_singles_out_the_convention_that_built_the_edges() {
        // Three cells cut from one field under the candidate convention:
        // the east cell starts at column 32, the north cell at row 32.
        let field = |x: usize, y: usize| ((x * 7 + y * 3) % 11) as f64 + (x * y % 5) as f64;
        let cell = |x0: usize, y0: usize| {
            let mut z = vec![0.0; POINTS];
            for j in 0..SIDE {
                for i in 0..SIDE {
                    z[j * SIDE + i] = field(x0 + i, y0 + j);
                }
            }
            z
        };
        // Inverse of the rows accumulation: the stored deltas.
        let encode = |z: &[f64]| {
            let mut g = vec![0i8; POINTS];
            for a in 0..SIDE {
                g[a * SIDE] = (z[a * SIDE] - if a == 0 { 0.0 } else { z[(a - 1) * SIDE] }) as i8;
                for b in 1..SIDE {
                    g[a * SIDE + b] = (z[a * SIDE + b] - z[a * SIDE + b - 1]) as i8;
                }
            }
            Heights {
                subrecord: 0,
                offset: 0.0,
                gradient: g,
                trailing: [0; 3],
            }
        };
        let home = grids(Some(&encode(&cell(0, 0))));
        let east = grids(Some(&encode(&cell(LAST, 0))));
        let north = grids(Some(&encode(&cell(0, LAST))));
        assert_eq!(home[0].as_ref().unwrap(), &cell(0, 0));
        let mut out = BTreeMap::new();
        assert_eq!(add_edge(&mut out, &home, &east, true), Some(0.0));
        assert_eq!(add_edge(&mut out, &home, &north, false), Some(0.0));
        // One edge direction cannot tell a mirror along it; both together
        // leave only the convention that built the cells.
        let equal: Vec<_> = out
            .iter()
            .filter(|(_, c)| c.equal == c.edges)
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(equal, ["rows/candidate"]);
        assert_eq!(out.len(), 16);
        assert!(out.values().all(|c| c.edges == 2));
    }

    #[test]
    fn ground_matches_the_scene_terrain_on_both_diagonals() {
        let h = heights(3.0, |a, b| ((a * 5 + b * 7) % 9) as i8 - 4);
        let z: Vec<f64> = stored_grid(&h, Accumulation::Rows)
            .unwrap()
            .iter()
            .map(|v| v * HEIGHT_UNIT)
            .collect();
        for d in Diagonal::ALL {
            let t = fo4_scene::terrain::Terrain::from_heights(
                FormKey {
                    plugin: "a".into(),
                    object_id: 1,
                },
                FormKey {
                    plugin: "a".into(),
                    object_id: 2,
                },
                None,
                [-1, 2],
                0,
                &h,
                d,
                fo4_scene::State::Blocked,
                &Budget::new(1 << 16),
            )
            .unwrap();
            for (x, y) in [
                (-4096.0, 8192.0),
                (-4000.5, 8300.25),
                (-1.0, 12287.0),
                (0.0, 12288.0),
            ] {
                assert_eq!(ground(&z, [-4096.0, 8192.0], d, x, y), t.height_at(x, y));
            }
            assert_eq!(ground(&z, [-4096.0, 8192.0], d, 1.0, 8192.0), None);
        }
        let mut d = [3.0, -1.0, 20.0, -40.0, 0.5];
        let g = GroundFit::from(&mut d, 2);
        assert_eq!(
            (g.objects, g.off_terrain, g.within_16, g.within_64),
            (5, 2, 3, 5)
        );
        assert_eq!((g.median, g.median_abs), (Some(0.5), Some(3.0)));
    }
}
