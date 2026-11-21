//! Terrain under the walker (`spec/walk.md`, "Terrain"): the terrain of a
//! cell and of its eight neighbours by XCLC grid, loaded with
//! `fo4_scene::terrain::load` at the walk's rule level, for
//! [`World::add_terrain`]. Each piece records its outcome, so a refused
//! terrain is reported rather than silently missing.
use crate::World;
use fo4_core::{Budget, Result};
use fo4_plugin::typed::{Cell, CellKind, Store};
use fo4_plugin::FormKey;
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
    /// `self` or the direction of the neighbour.
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
    /// `pieces[0]` is the cell itself; neighbours follow in [`AROUND`]
    /// order when they were searched.
    pub pieces: Vec<Piece>,
    /// Whether the eight neighbours were searched: only for an exterior
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

/// XCLC grid of a non-deleted exterior cell.
fn grid_of(store: &Store, key: &FormKey, b: &Budget) -> Result<Option<[i32; 2]>> {
    if store.kind(key) != Some(CellKind::Exterior) {
        return Ok(None);
    }
    let Some(c) = store.get::<Cell>(key, b)? else {
        return Ok(None);
    };
    if c.record.deleted() {
        return Ok(None);
    }
    Ok(c.data.grid.as_ref().map(|g| [g.i, g.j]))
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
    let options = terrain::Options {
        level,
        diagonal: None,
    };
    let mut out = TerrainSet::default();
    let own = terrain::load(store, cell, options, b)?;
    let stop = take(&mut out, piece("self", None, Some(cell.clone())), own, b)?;
    let world = store.world(cell);
    let grid = match out.pieces[0].grid {
        Some(g) => Some(g),
        None => grid_of(store, cell, &b.child())?,
    };
    if out.pieces[0].grid.is_none() {
        out.pieces[0].grid = grid;
    }
    let (Some([gx, gy]), Some(world), None, true) = (grid, world, stop, neighbours) else {
        return Ok(out);
    };
    out.neighbours_searched = true;
    // One pass over the world's exterior cells, keeping those within one
    // grid step.
    let mut near: Vec<([i32; 2], FormKey)> = Vec::new();
    for key in store.cells() {
        if key == *cell
            || store.kind(&key) != Some(CellKind::Exterior)
            || store.world(&key).as_ref() != Some(&world)
        {
            continue;
        }
        out.cells_scanned += 1;
        let Some([x, y]) = grid_of(store, &key, &b.child())? else {
            continue;
        };
        let (dx, dy) = (i64::from(x) - i64::from(gx), i64::from(y) - i64::from(gy));
        if dx.abs() > 1 || dy.abs() > 1 {
            continue;
        }
        if dx == 0 && dy == 0 {
            out.same_grid_cells += 1;
            continue;
        }
        b.reserve(&mut near, 1, "walk terrain", 0)?;
        b.charge(64 + key.plugin.len() as u64, "walk terrain", 0)?;
        near.push(([x, y], key));
    }
    for (name, dx, dy) in &AROUND[1..] {
        // Within one step of a grid read from an i32, so no overflow
        // unless the cell sits on the i32 edge; such a neighbour is absent.
        let (Some(x), Some(y)) = (gx.checked_add(*dx), gy.checked_add(*dy)) else {
            take_absent(&mut out, name, None, "no_cell", b)?;
            continue;
        };
        let at = [x, y];
        let mut keys = near.iter().filter(|(g, _)| *g == at).map(|(_, k)| k);
        match (keys.next(), keys.next()) {
            (None, _) => take_absent(&mut out, name, Some(at), "no_cell", b)?,
            (Some(_), Some(_)) => take_absent(&mut out, name, Some(at), "ambiguous_grid", b)?,
            (Some(key), None) => {
                let outcome = terrain::load(store, key, options, b)?;
                take(
                    &mut out,
                    piece(name, Some(at), Some(key.clone())),
                    outcome,
                    b,
                )?;
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
