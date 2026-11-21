//! Terrain of an exterior cell from its LAND record (`spec/land.md`).
//!
//! `fo4_plugin::land` decodes the stored bytes. Turning them into world
//! positions needs rules no source settles for Fallout 4: which LAND belongs
//! to the cell, how the VHGT bytes accumulate, the height unit, which corner
//! is the grid origin, the vertex spacing, and how each quad is split into
//! triangles. Each is a registered rule in `spec/hypotheses.tsv`; [`load`]
//! returns a terrain only at a [`RuleLevel`] that accepts all of them. The
//! diagonal rule is open with two levels, [`Diagonal::SouthwestNortheast`]
//! and [`Diagonal::NorthwestSoutheast`], like the body placement rule of
//! `crate::collision`.
use crate::bounds::Bounds;
use crate::collision::rules_state;
use crate::hypotheses::{self, Status};
use crate::{RuleLevel, State};
use fo4_core::{Budget, Error, Result};
use fo4_plugin::land::{Heights, Land, POINTS, SIDE};
use fo4_plugin::typed::{Cell, CellKind, Child, Store};
use fo4_plugin::FormKey;
use serde::Serialize;

/// World units per cell side under `land_grid_spacing_128_v1`.
pub const CELL_SIZE: f64 = 4096.0;
/// World units between neighbouring vertices under `land_grid_spacing_128_v1`.
pub const SPACING: f64 = 128.0;
/// World units of height per VHGT unit under `land_vhgt_units_8_v1`.
pub const HEIGHT_UNIT: f64 = 8.0;
/// Quads per side.
pub const QUADS: usize = SIDE - 1;

/// The open rule on how a quad is split into two triangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Diagonal {
    /// [SW, SE, NE] and [SW, NE, NW].
    SouthwestNortheast,
    /// [SW, SE, NW] and [SE, NE, NW].
    NorthwestSoutheast,
}
impl Diagonal {
    pub const ALL: [Self; 2] = [Self::SouthwestNortheast, Self::NorthwestSoutheast];
    pub fn rule(self) -> &'static str {
        match self {
            Self::SouthwestNortheast => hypotheses::LAND_DIAGONAL_SW_NE,
            Self::NorthwestSoutheast => hypotheses::LAND_DIAGONAL_NW_SE,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::SouthwestNortheast => "sw_ne",
            Self::NorthwestSoutheast => "nw_se",
        }
    }
}

/// Rules every terrain relies on besides the diagonal level.
pub const RULES: [&str; 5] = [
    hypotheses::LAND_OF_CELL,
    hypotheses::LAND_ACCUMULATION,
    hypotheses::LAND_UNITS,
    hypotheses::LAND_ORIGIN,
    hypotheses::LAND_SPACING,
];

/// The diagonal level the registry selects, as
/// `crate::collision::default_body_placement` selects a body placement:
/// none at [`RuleLevel::Facts`], the one supported level at
/// [`RuleLevel::Measured`], otherwise the first pending one in
/// [`Diagonal::ALL`]. A refuted level is never chosen.
pub fn default_diagonal(
    level: RuleLevel,
    status: impl Fn(&str) -> Option<Status>,
) -> Option<Diagonal> {
    let with = |wanted: Status| -> Vec<Diagonal> {
        Diagonal::ALL
            .into_iter()
            .filter(|d| status(d.rule()) == Some(wanted))
            .collect()
    };
    let supported = with(Status::Supported);
    match level {
        RuleLevel::Facts => None,
        RuleLevel::Measured => (supported.len() == 1).then(|| supported[0]),
        RuleLevel::Hypotheses => match supported.len() {
            1 => Some(supported[0]),
            0 => with(Status::Pending).first().copied(),
            _ => None,
        },
    }
}

/// Accumulated VHGT values in stored (row, column) order, before the unit
/// factor, under `land_vhgt_row_accumulation_v1`: index `33 r + c`.
pub fn accumulate(h: &Heights, b: &Budget) -> Result<Vec<f64>> {
    let file = "LAND.VHGT";
    if h.gradient.len() != POINTS {
        return Err(Error::new(
            "LandGradientSize",
            file,
            0,
            POINTS,
            h.gradient.len(),
        ));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, POINTS, file, 0)?;
    let mut column = f64::from(h.offset);
    for row in h.gradient.as_chunks::<SIDE>().0 {
        column += f64::from(row[0]);
        let mut along = column;
        out.push(along);
        for v in &row[1..] {
            along += f64::from(*v);
            out.push(along);
        }
    }
    Ok(out)
}

/// Height in a unit quad, `u` east and `v` north in [0, 1], from the corner
/// heights south-west, south-east, north-west, north-east.
pub fn quad_height(d: Diagonal, [sw, se, nw, ne]: [f64; 4], u: f64, v: f64) -> f64 {
    match d {
        Diagonal::SouthwestNortheast if u >= v => sw + u * (se - sw) + v * (ne - se),
        Diagonal::SouthwestNortheast => sw + v * (nw - sw) + u * (ne - nw),
        Diagonal::NorthwestSoutheast if u + v <= 1.0 => sw + u * (se - sw) + v * (nw - sw),
        Diagonal::NorthwestSoutheast => ne + (1.0 - u) * (nw - ne) + (1.0 - v) * (se - ne),
    }
}

/// Terrain of one exterior cell placed in world space.
#[derive(Debug, Serialize)]
pub struct Terrain {
    pub cell: FormKey,
    pub land: FormKey,
    pub world: Option<FormKey>,
    /// XCLC grid (X, Y).
    pub grid: [i32; 2],
    /// XCLC flags as stored. The UESP Skyrim CELL page names bits 0-3
    /// "Force Hide Land Quad"; they are not applied here.
    pub grid_flags: u32,
    pub diagonal: Diagonal,
    /// Every rule this terrain relies on and their standing.
    pub rules: Vec<&'static str>,
    pub state: State,
    /// World z of vertex (row r, column c) at index `33 r + c`; row 0 south,
    /// column 0 west.
    pub heights: Vec<f64>,
}
impl Terrain {
    /// Builds a terrain from decoded LAND heights under the candidate rules,
    /// without consulting the registry: `rules` and `state` are recorded as
    /// given. [`load`] is the gated entry point.
    #[allow(clippy::too_many_arguments)]
    pub fn from_heights(
        cell: FormKey,
        land: FormKey,
        world: Option<FormKey>,
        grid: [i32; 2],
        grid_flags: u32,
        heights: &Heights,
        diagonal: Diagonal,
        state: State,
        b: &Budget,
    ) -> Result<Self> {
        let mut z = accumulate(heights, b)?;
        for v in &mut z {
            *v *= HEIGHT_UNIT;
        }
        let mut rules = Vec::new();
        b.reserve(&mut rules, RULES.len() + 1, "terrain", 0)?;
        rules.extend(RULES);
        rules.push(diagonal.rule());
        rules.sort_unstable();
        Ok(Self {
            cell,
            land,
            world,
            grid,
            grid_flags,
            diagonal,
            rules,
            state,
            heights: z,
        })
    }
    /// World x, y of the south-west corner.
    pub fn origin(&self) -> [f64; 2] {
        [
            f64::from(self.grid[0]) * CELL_SIZE,
            f64::from(self.grid[1]) * CELL_SIZE,
        ]
    }
    pub fn vertex(&self, row: usize, column: usize) -> [f64; 3] {
        let [x, y] = self.origin();
        [
            x + column as f64 * SPACING,
            y + row as f64 * SPACING,
            self.heights[row * SIDE + column],
        ]
    }
    /// Vertex indices (`33 r + c`) of the two triangles of every quad, quads
    /// row by row from the south-west, each wound counter-clockwise seen
    /// from above (+z). Which side the game treats as solid is not claimed.
    pub fn triangle_indices(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        let d = self.diagonal;
        (0..QUADS * QUADS).flat_map(move |q| {
            let (r, c) = (q / QUADS, q % QUADS);
            let sw = r * SIDE + c;
            let (se, nw, ne) = (sw + 1, sw + SIDE, sw + SIDE + 1);
            match d {
                Diagonal::SouthwestNortheast => [[sw, se, ne], [sw, ne, nw]],
                Diagonal::NorthwestSoutheast => [[sw, se, nw], [se, ne, nw]],
            }
        })
    }
    /// The 2048 world-space triangles of the cell.
    pub fn world_triangles(&self) -> impl Iterator<Item = [[f64; 3]; 3]> + '_ {
        self.triangle_indices()
            .map(|t| t.map(|i| self.vertex(i / SIDE, i % SIDE)))
    }
    /// Height of the triangulated surface at world (x, y); `None` outside
    /// the cell's footprint (edges included).
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        let [ox, oy] = self.origin();
        let gx = (x - ox) / SPACING;
        let gy = (y - oy) / SPACING;
        let n = QUADS as f64;
        if !(0.0..=n).contains(&gx) || !(0.0..=n).contains(&gy) {
            return None;
        }
        let c = (gx.floor() as usize).min(QUADS - 1);
        let r = (gy.floor() as usize).min(QUADS - 1);
        let h = |r: usize, c: usize| self.heights[r * SIDE + c];
        Some(quad_height(
            self.diagonal,
            [h(r, c), h(r, c + 1), h(r + 1, c), h(r + 1, c + 1)],
            gx - c as f64,
            gy - r as f64,
        ))
    }
    pub fn bounds(&self) -> Bounds {
        let [x, y] = self.origin();
        let (lo, hi) = self
            .heights
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
                (lo.min(*z), hi.max(*z))
            });
        Bounds {
            min: [x, y, lo],
            max: [x + CELL_SIZE, y + CELL_SIZE, hi],
        }
    }
}

/// LAND children of a cell, split as `Store::children` splits them.
#[derive(Debug, Default, Serialize)]
pub struct LandChildren {
    pub candidates: Vec<Child>,
    pub deleted: Vec<Child>,
}
pub fn land_children(store: &Store, cell: &FormKey, b: &Budget) -> Result<LandChildren> {
    let children = store.children(cell, b)?;
    let mut out = LandChildren::default();
    for c in children.excluded {
        if c.signature == *b"LAND" {
            b.reserve(&mut out.candidates, 1, &cell.plugin, c.source.offset)?;
            out.candidates.push(c);
        }
    }
    for c in children.deleted {
        if c.signature == *b"LAND" {
            b.reserve(&mut out.deleted, 1, &cell.plugin, c.source.offset)?;
            out.deleted.push(c);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub level: RuleLevel,
    /// `None` selects [`default_diagonal`] from the registry.
    pub diagonal: Option<Diagonal>,
}

/// Why a cell yields no terrain.
#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub detail: String,
}
#[derive(Debug, Serialize)]
pub enum Outcome {
    Built(Box<Terrain>),
    Refused(Refusal),
}
fn refused(code: &'static str, detail: impl ToString) -> Result<Outcome> {
    Ok(Outcome::Refused(Refusal {
        code,
        detail: detail.to_string(),
    }))
}

/// Terrain of `cell` at `options.level` with the embedded registry.
pub fn load(store: &Store, cell: &FormKey, options: Options, b: &Budget) -> Result<Outcome> {
    load_with(store, cell, options, &hypotheses::status, b)
}

/// [`load`] against an explicit status table.
pub fn load_with(
    store: &Store,
    cell: &FormKey,
    options: Options,
    status: &dyn Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<Outcome> {
    let Some(c) = store.get::<Cell>(cell, b)? else {
        return Err(Error::new(
            "MissingCell",
            &cell.plugin,
            0,
            "CELL winner",
            cell.object_id,
        ));
    };
    if store.kind(cell) != Some(CellKind::Exterior) {
        return refused("NotExterior", format!("{:?}", store.kind(cell)));
    }
    let Some(grid) = &c.data.grid else {
        return refused("NoGrid", "exterior CELL without XCLC");
    };
    // The rules every level shares are checked first, then the diagonal.
    let common = rules_state(&RULES, status);
    if !common.ready_at(options.level) {
        return refused(
            "RulesNotAccepted",
            format!("{:?} at {:?}", common, options.level),
        );
    }
    let diagonal = match options
        .diagonal
        .or_else(|| default_diagonal(options.level, status))
    {
        Some(d) => d,
        None => return refused("NoDiagonalLevel", format!("{:?}", options.level)),
    };
    let mut rules: Vec<&'static str> = RULES.to_vec();
    rules.push(diagonal.rule());
    rules.sort_unstable();
    let state = rules_state(&rules, status);
    if !state.ready_at(options.level) {
        return refused(
            "RulesNotAccepted",
            format!("{:?} at {:?}", state, options.level),
        );
    }
    let lands = land_children(store, cell, b)?;
    let land_key = match lands.candidates.as_slice() {
        [] => return refused("NoLand", format!("deleted LAND: {}", lands.deleted.len())),
        [one] => one.key.clone(),
        many => return refused("MultipleLand", many.len()),
    };
    let Some(land) = store.get_land(&land_key, b)? else {
        return refused("NoLand", "LAND winner absent");
    };
    let Some(heights) = &land.data.heights else {
        return refused("NoHeights", "LAND without a decoded VHGT");
    };
    if !heights.offset.is_finite() {
        return refused("NonFiniteHeightOffset", heights.offset.to_bits());
    }
    Ok(Outcome::Built(Box::new(Terrain::from_heights(
        cell.clone(),
        land_key,
        store.world(cell),
        [grid.i, grid.j],
        grid.flags,
        heights,
        diagonal,
        state,
        b,
    )?)))
}

/// Reads a LAND's heights for reports: decoded LAND, stored-order
/// accumulation, without any rule gating.
pub fn land_heights(land: &Land, b: &Budget) -> Result<Option<Vec<f64>>> {
    match &land.heights {
        Some(h) if h.offset.is_finite() => Ok(Some(accumulate(h, b)?)),
        _ => Ok(None),
    }
}

pub mod layers;

#[cfg(test)]
mod tests;
