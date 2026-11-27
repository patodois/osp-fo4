//! Exterior cells around a cell by XCLC grid, and one scene holding the
//! placed references of a cell and of the cells around it
//! (`spec/scene.md`, "Cells around an exterior cell").
//!
//! Which cells are around a cell is read from their XCLC grid in the same
//! world, as the terrain report does (`spec/land.md`): a grid claimed by
//! two or more cells is ambiguous and neither is used. Loading them
//! together is a viewing choice; it claims nothing about which cells the
//! game keeps loaded.
use crate::{Loader, Scene};
use fo4_core::{Budget, Error, Result};
use fo4_plugin::typed::{Cell, CellKind, Store};
use fo4_plugin::FormKey;
use serde::Serialize;

/// Largest radius [`around`] accepts: 7 x 7 cells.
pub const MAX_RADIUS: u32 = 3;

/// The eight offsets of radius 1, in the order the walk's terrain pieces
/// are reported: east, west, north, south, north-east, north-west,
/// south-east, south-west.
pub const RING_1: [[i32; 2]; 8] = [
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
    [1, 1],
    [-1, 1],
    [1, -1],
    [-1, -1],
];

const FILE: &str = "cell grid";

/// Grid offsets within `radius` steps on both axes, the centre excluded:
/// [`RING_1`] first, then each further ring from south to north and west
/// to east.
pub fn offsets(radius: u32) -> Vec<[i32; 2]> {
    let r = radius.min(MAX_RADIUS) as i32;
    let mut out = Vec::new();
    if r >= 1 {
        out.extend_from_slice(&RING_1);
    }
    for ring in 2..=r {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) == ring {
                    out.push([dx, dy]);
                }
            }
        }
    }
    out
}

/// XCLC grid of a non-deleted exterior cell.
pub fn grid_of(store: &Store, key: &FormKey, b: &Budget) -> Result<Option<[i32; 2]>> {
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

/// One grid position around the cell.
#[derive(Debug, Clone, Serialize)]
pub struct Neighbour {
    /// Steps east and north of the cell.
    pub offset: [i32; 2],
    /// `None` only when the position falls outside the i32 grid.
    pub grid: Option<[i32; 2]>,
    pub cell: Option<FormKey>,
    /// `cell`, `no_cell` (no exterior cell of the world claims the grid)
    /// or `ambiguous_grid` (two or more do; neither is used).
    pub outcome: &'static str,
}

/// The exterior cells around a cell.
#[derive(Debug, Clone, Serialize)]
pub struct Around {
    pub cell: FormKey,
    pub grid: Option<[i32; 2]>,
    pub world: Option<FormKey>,
    pub radius: u32,
    /// Whether the world was searched: only for a radius above zero and
    /// an exterior cell with a grid, in a world.
    pub searched: bool,
    /// In [`offsets`] order; empty when not searched.
    pub neighbours: Vec<Neighbour>,
    /// Exterior cells of the same world read while searching.
    pub cells_scanned: u64,
    /// Other cells claiming the cell's own grid (ignored).
    pub same_grid_cells: u64,
}

/// The cells within `radius` grid steps of `cell` in its world, read in
/// one pass over the world's exterior cells. Every read goes through `b`.
pub fn around(store: &Store, cell: &FormKey, radius: u32, b: &Budget) -> Result<Around> {
    if radius > MAX_RADIUS {
        return Err(Error::new(
            "InvalidGridRadius",
            FILE,
            0,
            format!("at most {MAX_RADIUS}"),
            radius,
        ));
    }
    let grid = grid_of(store, cell, &b.child())?;
    let world = store.world(cell);
    let mut out = Around {
        cell: cell.clone(),
        grid,
        world: world.clone(),
        radius,
        searched: false,
        neighbours: Vec::new(),
        cells_scanned: 0,
        same_grid_cells: 0,
    };
    let (Some([gx, gy]), Some(world), true) = (grid, world, radius > 0) else {
        return Ok(out);
    };
    out.searched = true;
    let r = i64::from(radius);
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
        if dx.abs() > r || dy.abs() > r {
            continue;
        }
        if dx == 0 && dy == 0 {
            out.same_grid_cells += 1;
            continue;
        }
        b.reserve(&mut near, 1, FILE, 0)?;
        b.charge(64 + key.plugin.len() as u64, FILE, 0)?;
        near.push(([x, y], key));
    }
    let offsets = offsets(radius);
    b.reserve(&mut out.neighbours, offsets.len(), FILE, 0)?;
    for [dx, dy] in offsets {
        // A position off the i32 grid has no cell.
        let at = match (gx.checked_add(dx), gy.checked_add(dy)) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => None,
        };
        let mut keys = near.iter().filter(|(g, _)| Some(*g) == at).map(|(_, k)| k);
        let (cell, outcome) = match (keys.next(), keys.next()) {
            (None, _) => (None, "no_cell"),
            (Some(_), Some(_)) => (None, "ambiguous_grid"),
            (Some(k), None) => (Some(k.clone()), "cell"),
        };
        if let Some(k) = &cell {
            b.charge(64 + k.plugin.len() as u64, FILE, 0)?;
        }
        out.neighbours.push(Neighbour {
            offset: [dx, dy],
            grid: at,
            cell,
            outcome,
        });
    }
    Ok(out)
}

/// Who claims one grid position of a world ([`WorldGrid`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Claim {
    /// One non-deleted exterior cell.
    Cell(FormKey),
    /// Two or more; neither is used, as in [`around`].
    Ambiguous,
}

/// Every exterior cell of a cell's world by XCLC grid, read once, so that
/// the cells around any grid position can be found without reading the
/// world again (the viewer's moving grid, `spec/scene.md`). The cell's own
/// grid is the cell's, as in [`around`]: other cells claiming it are
/// counted and ignored. Like [`around`] it claims nothing about which
/// cells the game keeps loaded.
#[derive(Debug, Clone, Serialize)]
pub struct WorldGrid {
    pub cell: FormKey,
    pub grid: Option<[i32; 2]>,
    pub world: Option<FormKey>,
    /// Whether the world was read: only for an exterior cell with a grid,
    /// in a world.
    pub searched: bool,
    pub cells: std::collections::BTreeMap<[i32; 2], Claim>,
    /// Exterior cells of the same world read.
    pub cells_scanned: u64,
    /// Other cells claiming the cell's own grid (ignored).
    pub same_grid_cells: u64,
}

impl WorldGrid {
    /// The cell at `grid` and the outcome of [`Neighbour::outcome`]:
    /// `cell`, `no_cell` or `ambiguous_grid`.
    pub fn at(&self, grid: [i32; 2]) -> (Option<&FormKey>, &'static str) {
        match self.cells.get(&grid) {
            Some(Claim::Cell(k)) => (Some(k), "cell"),
            Some(Claim::Ambiguous) => (None, "ambiguous_grid"),
            None => (None, "no_cell"),
        }
    }
}

/// The exterior cells of `cell`'s world by grid, in one pass over the
/// world's cells. Every read goes through `b`.
pub fn world_grid(store: &Store, cell: &FormKey, b: &Budget) -> Result<WorldGrid> {
    let grid = grid_of(store, cell, &b.child())?;
    let world = store.world(cell);
    let mut out = WorldGrid {
        cell: cell.clone(),
        grid,
        world: world.clone(),
        searched: false,
        cells: std::collections::BTreeMap::new(),
        cells_scanned: 0,
        same_grid_cells: 0,
    };
    let (Some(own), Some(world)) = (grid, world) else {
        return Ok(out);
    };
    out.searched = true;
    b.charge(64 + cell.plugin.len() as u64, FILE, 0)?;
    out.cells.insert(own, Claim::Cell(cell.clone()));
    for key in store.cells() {
        if key == *cell
            || store.kind(&key) != Some(CellKind::Exterior)
            || store.world(&key).as_ref() != Some(&world)
        {
            continue;
        }
        out.cells_scanned += 1;
        let Some(at) = grid_of(store, &key, &b.child())? else {
            continue;
        };
        if at == own {
            out.same_grid_cells += 1;
            continue;
        }
        match out.cells.get_mut(&at) {
            Some(claim) => *claim = Claim::Ambiguous,
            None => {
                b.charge(64 + key.plugin.len() as u64, FILE, 0)?;
                out.cells.insert(at, Claim::Cell(key));
            }
        }
    }
    Ok(out)
}

/// What [`absorb_around`] did with one grid position.
#[derive(Debug, Clone, Serialize)]
pub struct CellLoad {
    pub offset: [i32; 2],
    pub grid: Option<[i32; 2]>,
    pub cell: Option<FormKey>,
    /// `loaded`, or the neighbour's `no_cell` / `ambiguous_grid`.
    pub outcome: &'static str,
    /// Placed references the cell added to the scene.
    pub instances: usize,
}

/// Loads every cell of `around` with `loader` and appends its placed
/// references, its deleted, disabled and excluded children, its
/// diagnostics, its precombine and previs declarations and its assets to
/// `scene`, the scene of `around.cell`. The scene keeps the centre cell's
/// record, name, kind and lighting. Asset indices are the loader's, so the
/// draw list and the collision of the merged scene read the references of
/// every cell as one placement list (both sort by reference). Each cell is
/// loaded in a child scope of `b` that `b` then keeps, so a root with a
/// scope limit ([`Budget::with_scope_limit`]) bounds each cell by it and
/// the whole scene by its ceiling. Returns one entry per position, the
/// centre first.
pub fn absorb_around(
    loader: &mut Loader<'_>,
    scene: &mut Scene,
    around: &Around,
    b: &Budget,
) -> Result<Vec<CellLoad>> {
    let mut out = Vec::new();
    b.reserve(&mut out, around.neighbours.len() + 1, FILE, 0)?;
    out.push(CellLoad {
        offset: [0, 0],
        grid: around.grid,
        cell: Some(scene.cell.key.clone()),
        outcome: "loaded",
        instances: scene.instances.len(),
    });
    for n in &around.neighbours {
        let Some(key) = &n.cell else {
            out.push(CellLoad {
                offset: n.offset,
                grid: n.grid,
                cell: None,
                outcome: n.outcome,
                instances: 0,
            });
            continue;
        };
        // Each cell is read in its own scope of `b` (one cell's work stays
        // within `b`'s scope limit) and what it read is then kept by `b`,
        // which also pays for joining it to the scene.
        let scope = b.child();
        let other = loader.cell(key, &scope)?;
        scope.commit(b)?;
        let instances = other.instances.len();
        absorb(scene, other, b)?;
        out.push(CellLoad {
            offset: n.offset,
            grid: n.grid,
            cell: Some(key.clone()),
            outcome: "loaded",
            instances,
        });
    }
    Ok(out)
}

/// Side of an exterior grid square in game units.
pub const SQUARE_SIZE: f64 = 4096.0;

/// The grid square holding a point.
pub fn square_of(p: [f64; 3]) -> [i32; 2] {
    [
        (p[0] / SQUARE_SIZE).floor() as i32,
        (p[1] / SQUARE_SIZE).floor() as i32,
    ]
}

/// The placed references (`REFR`, not actors) of `world`'s cells'
/// persistent groups whose stored position lies in one of `squares`
/// (`world_persistent_refs_drawn_in_square_v1`): those placed active, and
/// those initially disabled, each in stored order.
pub fn persistent_in(
    store: &Store,
    world: &FormKey,
    squares: &[[i32; 2]],
    b: &Budget,
) -> Result<(Vec<fo4_plugin::typed::Child>, Vec<fo4_plugin::typed::Child>)> {
    let (mut active, mut disabled) = (Vec::new(), Vec::new());
    let world = crate::timeline::norm(world);
    for c in store.cells() {
        if store.world(&c).map(|w| crate::timeline::norm(&w)) != Some(world.clone()) {
            continue;
        }
        let cb = b.child();
        let ch = store.children(&c, &cb)?;
        for (kid, on) in ch
            .active
            .into_iter()
            .map(|k| (k, true))
            .chain(ch.disabled.into_iter().map(|k| (k, false)))
        {
            if kid.group_kind != 8 || kid.signature != *b"REFR" {
                continue;
            }
            let rb = b.child();
            let Some(r) = store.get::<fo4_plugin::typed::Refr>(&kid.key, &rb)? else {
                continue;
            };
            let Some(t) = r.data.transform else { continue };
            if !squares.contains(&square_of([t[0], t[1], t[2]].map(f64::from))) {
                continue;
            }
            let to = if on { &mut active } else { &mut disabled };
            b.reserve(to, 1, FILE, 0)?;
            to.push(kid);
        }
    }
    Ok((active, disabled))
}

/// What [`absorb_persistent`] joined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistentJoin {
    /// Instances added to the scene.
    pub added: usize,
    /// Initially disabled references added to the scene's disabled list.
    pub disabled: usize,
    /// References already among the scene's instances or disabled list.
    pub already: usize,
    /// References whose instance did not build (left out).
    pub failed: usize,
}

/// Joins to `scene` the persistent references of `around`'s world standing
/// in its loaded grid squares (the cell's and each loaded neighbour's;
/// [`persistent_in`], `world_persistent_refs_drawn_in_square_v1`): each
/// active one as the instance a cell load makes for it, each initially
/// disabled one to the disabled list (where a run's enable finds it).
pub fn absorb_persistent(
    loader: &mut Loader<'_>,
    scene: &mut Scene,
    around: &Around,
    b: &Budget,
) -> Result<PersistentJoin> {
    let mut out = PersistentJoin::default();
    let Some(world) = &around.world else {
        return Ok(out);
    };
    let squares: Vec<[i32; 2]> = around
        .grid
        .into_iter()
        .chain(
            around
                .neighbours
                .iter()
                .filter(|n| n.cell.is_some())
                .filter_map(|n| n.grid),
        )
        .collect();
    let (active, disabled) = persistent_in(loader.plugins, world, &squares, b)?;
    let have: std::collections::BTreeSet<FormKey> = scene
        .instances
        .iter()
        .map(|i| crate::timeline::norm(&i.reference.key))
        .chain(scene.disabled.iter().map(|c| crate::timeline::norm(&c.key)))
        .collect();
    for kid in active {
        if have.contains(&crate::timeline::norm(&kid.key)) {
            out.already += 1;
            continue;
        }
        let scope = b.child();
        match loader.instance(&kid.key, &scope) {
            Ok(instance) => {
                scope.commit(b)?;
                b.reserve(&mut scene.assets, instance.assets.len(), FILE, 0)?;
                scene.assets.extend_from_slice(&instance.assets);
                b.reserve(&mut scene.instances, 1, FILE, 0)?;
                scene.instances.push(instance);
                out.added += 1;
            }
            Err(e) => {
                reference_error(e, &scope)?;
                out.failed += 1;
            }
        }
    }
    for kid in disabled {
        if have.contains(&crate::timeline::norm(&kid.key)) {
            out.already += 1;
            continue;
        }
        b.reserve(&mut scene.disabled, 1, FILE, 0)?;
        scene.disabled.push(kid);
        out.disabled += 1;
    }
    scene.assets.sort_unstable();
    scene.assets.dedup();
    Ok(out)
}

/// One persistent reference's instance error in [`absorb_persistent`]: the
/// budget's own refusal (an allocation that failed, a charge past its
/// ceiling or its scope limit; [`Budget::refused`]) is the load's and is
/// returned; any other error, a limit of the data read among them, is the
/// reference's: `Ok`, and the join counts it as failed and goes on.
fn reference_error(e: Error, b: &Budget) -> Result<()> {
    if b.refused(&e) {
        Err(e)
    } else {
        Ok(())
    }
}

fn append<T>(to: &mut Vec<T>, from: Vec<T>, b: &Budget) -> Result<()> {
    b.reserve(to, from.len(), FILE, 0)?;
    to.extend(from);
    Ok(())
}

/// Appends the children, diagnostics, declarations and assets of `other`
/// to `scene`.
fn absorb(scene: &mut Scene, other: Scene, b: &Budget) -> Result<()> {
    append(&mut scene.instances, other.instances, b)?;
    append(&mut scene.deleted, other.deleted, b)?;
    append(&mut scene.disabled, other.disabled, b)?;
    append(&mut scene.exclusions, other.exclusions, b)?;
    append(&mut scene.diagnostics, other.diagnostics, b)?;
    scene.shared_diagnostic_count += other.shared_diagnostic_count;
    append(
        &mut scene.visibility.precombine,
        other.visibility.precombine,
        b,
    )?;
    append(&mut scene.visibility.previs, other.visibility.previs, b)?;
    append(&mut scene.assets, other.assets, b)?;
    scene.assets.sort_unstable();
    scene.assets.dedup();
    Ok(())
}

#[cfg(test)]
mod tests;
