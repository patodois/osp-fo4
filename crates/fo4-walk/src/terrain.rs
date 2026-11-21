//! Terrain under the walker (`spec/walk.md`, "Terrain"): the terrain of a
//! cell and of its eight neighbours (or the cells within a larger radius)
//! by XCLC grid, found with `fo4_scene::grid::around` and loaded with
//! `fo4_scene::terrain::load` at the walk's rule level, for
//! [`World::add_terrain`]. Each piece records its outcome, so a refused
//! terrain is reported rather than silently missing.
use crate::World;
use fo4_core::{Budget, Result};
use fo4_plugin::typed::Store;
use fo4_plugin::FormKey;
use fo4_scene::grid::{self, Around};
use fo4_scene::terrain::{self, Outcome, Refusal, Terrain};
use fo4_scene::RuleLevel;
use serde::Serialize;

/// The cell and its eight neighbours, as grid offsets, in report order.
pub const AROUND: [(&str, i32, i32); 9] = [
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

/// One cell's terrain as the walk sees it.
#[derive(Debug, Clone, Serialize)]
pub struct Piece {
    /// `self`, the direction of a neighbour one step away, or `ring_<n>`
    /// ([`piece_name`]).
    pub name: &'static str,
    /// XCLC grid, when known.
    pub grid: Option<[i32; 2]>,
    pub cell: Option<FormKey>,
    pub land: Option<FormKey>,
    /// `built:<diagonal>`, `refused:<code>`, `no_cell` (no exterior cell of
    /// the world at that grid) or `ambiguous_grid` (two or more claim it;
    /// neither is used).
    pub outcome: String,
    pub refusal: Option<Refusal>,
    /// World source of its triangles, once added ([`TerrainSet::add_to`]).
    pub source: Option<u32>,
}

/// The terrain pieces around a cell.
#[derive(Debug, Default, Serialize)]
pub struct TerrainSet {
    /// `pieces[0]` is the cell itself; neighbours follow in
    /// `fo4_scene::grid::offsets` order (for one step, [`AROUND`] order)
    /// when they were searched.
    pub pieces: Vec<Piece>,
    /// Whether the neighbours were searched: only for an exterior
    /// cell with a grid, in a world, at a level that accepts the terrain
    /// rules.
    pub neighbours_searched: bool,
    /// Exterior cells of the same world read while searching.
    pub cells_scanned: u64,
    /// Other cells claiming the cell's own grid (ignored).
    pub same_grid_cells: u64,
    #[serde(skip)]
    pub terrains: Vec<Terrain>,
}

impl TerrainSet {
    /// The outcome for the cell itself.
    pub fn own(&self) -> Option<&Piece> {
        self.pieces.first()
    }
    /// Whether the cell's own terrain was built.
    pub fn own_built(&self) -> bool {
        self.own().is_some_and(|p| p.outcome.starts_with("built:"))
    }
    /// The terrain height at (x, y) from the first built piece whose
    /// footprint contains it, with that piece's grid.
    pub fn height_at(&self, x: f64, y: f64) -> Option<(f64, [i32; 2])> {
        self.terrains
            .iter()
            .find_map(|t| t.height_at(x, y).map(|z| (z, t.grid)))
    }
    /// Adds every built piece to `world` ([`World::add_terrain`]) and
    /// records its source.
    pub fn add_to(&mut self, world: &mut World, b: &Budget) -> Result<()> {
        for t in &self.terrains {
            let source = world.add_terrain(t, b)?;
            if let Some(p) = self
                .pieces
                .iter_mut()
                .find(|p| p.cell.as_ref() == Some(&t.cell))
            {
                p.source = Some(source);
            }
        }
        Ok(())
    }
}

fn piece(name: &'static str, grid: Option<[i32; 2]>, cell: Option<FormKey>) -> Piece {
    Piece {
        name,
        grid,
        cell,
        land: None,
        outcome: String::new(),
        refusal: None,
        source: None,
    }
}

/// Refusals that hold for every cell at the level, or that leave no grid
/// to search around.
fn stops_search(code: &str) -> bool {
    matches!(
        code,
        "NotExterior" | "NoGrid" | "RulesNotAccepted" | "NoDiagonalLevel"
    )
}

fn take(
    out: &mut TerrainSet,
    mut p: Piece,
    outcome: Outcome,
    b: &Budget,
) -> Result<Option<&'static str>> {
    let mut stop = None;
    match outcome {
        Outcome::Built(t) => {
            p.outcome = format!("built:{}", t.diagonal.name());
            p.land = Some(t.land.clone());
            p.grid = Some(t.grid);
            b.reserve(&mut out.terrains, 1, "walk terrain", 0)?;
            out.terrains.push(*t);
        }
        Outcome::Refused(r) => {
            p.outcome = format!("refused:{}", r.code);
            if stops_search(r.code) {
                stop = Some(r.code);
            }
            p.refusal = Some(r);
        }
    }
    b.reserve(&mut out.pieces, 1, "walk terrain", 0)?;
    out.pieces.push(p);
    Ok(stop)
}

/// Name of a piece at a grid offset: the [`AROUND`] name within one
/// step, `ring_<n>` beyond.
pub fn piece_name(offset: [i32; 2]) -> &'static str {
    if let Some((name, _, _)) = AROUND.iter().find(|(_, dx, dy)| [*dx, *dy] == offset) {
        return name;
    }
    match offset[0].abs().max(offset[1].abs()) {
        2 => "ring_2",
        3 => "ring_3",
        _ => "ring_far",
    }
}

/// Where the neighbours come from.
enum Search<'a> {
    /// Search the world within this many grid steps.
    Radius(u32),
    /// Already searched.
    Given(&'a Around),
}

/// Loads the terrain of `cell` at `level` and, when `neighbours` is set
/// and the cell is an exterior cell of a world whose terrain rules the
/// level accepts, of the eight cells around it by XCLC grid. Every read
/// goes through `b`.
pub fn load_around(
    store: &Store,
    cell: &FormKey,
    level: RuleLevel,
    neighbours: bool,
    b: &Budget,
) -> Result<TerrainSet> {
    load_grid(store, cell, level, u32::from(neighbours), b)
}

/// [`load_around`] for the cells within `radius` grid steps
/// ([`fo4_scene::grid::around`], at most [`fo4_scene::grid::MAX_RADIUS`]):
/// the cell first, then the neighbours in [`fo4_scene::grid::offsets`]
/// order. Radius 1 is [`load_around`] with neighbours.
pub fn load_grid(
    store: &Store,
    cell: &FormKey,
    level: RuleLevel,
    radius: u32,
    b: &Budget,
) -> Result<TerrainSet> {
    load(store, cell, level, Search::Radius(radius), b)
}

/// [`load_grid`] with the neighbours already found, so that the terrain
/// and the placed references of a grid of cells come from one search.
pub fn load_with(
    store: &Store,
    around: &Around,
    level: RuleLevel,
    b: &Budget,
) -> Result<TerrainSet> {
    load(store, &around.cell, level, Search::Given(around), b)
}

fn load(
    store: &Store,
    cell: &FormKey,
    level: RuleLevel,
    search: Search<'_>,
    b: &Budget,
) -> Result<TerrainSet> {
    let options = terrain::Options {
        level,
        diagonal: None,
    };
    let mut out = TerrainSet::default();
    let own = terrain::load(store, cell, options, b)?;
    let stop = take(&mut out, piece("self", None, Some(cell.clone())), own, b)?;
    if out.pieces[0].grid.is_none() {
        out.pieces[0].grid = grid::grid_of(store, cell, &b.child())?;
    }
    if stop.is_some() {
        return Ok(out);
    }
    let searched;
    let around = match search {
        Search::Radius(0) => return Ok(out),
        Search::Radius(r) => {
            searched = grid::around(store, cell, r, b)?;
            &searched
        }
        Search::Given(a) => a,
    };
    if !around.searched {
        return Ok(out);
    }
    out.neighbours_searched = true;
    out.cells_scanned = around.cells_scanned;
    out.same_grid_cells = around.same_grid_cells;
    for n in &around.neighbours {
        let name = piece_name(n.offset);
        match &n.cell {
            None => take_absent(&mut out, name, n.grid, n.outcome, b)?,
            Some(key) => {
                let outcome = terrain::load(store, key, options, b)?;
                take(&mut out, piece(name, n.grid, Some(key.clone())), outcome, b)?;
            }
        }
    }
    Ok(out)
}

fn take_absent(
    out: &mut TerrainSet,
    name: &'static str,
    grid: Option<[i32; 2]>,
    outcome: &str,
    b: &Budget,
) -> Result<()> {
    let mut p = piece(name, grid, None);
    p.outcome = outcome.to_owned();
    b.reserve(&mut out.pieces, 1, "walk terrain", 0)?;
    out.pieces.push(p);
    Ok(())
}

#[cfg(test)]
mod tests;
