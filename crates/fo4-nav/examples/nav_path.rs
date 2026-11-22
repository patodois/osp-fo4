#![forbid(unsafe_code)]
//! Navigation meshes of test cells and a sample path over them with
//! `fo4_nav` (`spec/navmesh.md`). Measurement support.
//!
//! `nav_path MANIFEST OUT_DIR CELL[:RADIUS]...` loads, for each cell given
//! (an editor id, with the exterior cells within RADIUS grid steps), the
//! winning live navmeshes of those cells, joins them (`fo4_nav::NavGraph`),
//! counts triangles and connected components, finds the path between two
//! far-apart triangles of the largest component, and compares the vertex
//! heights of the cell's own meshes with the cell's placed collision and
//! the terrain around it (`fo4_walk::World`). It writes `nav-path.txt`
//! (counts and lengths only; the cells are numbered, not named; committed as
//! `spec/navmesh-path.txt`), `nav-path-names.txt` (cell names; never
//! versioned) and `nav-path-cell-N.png` (top-down picture of the meshes and
//! the path; never versioned).
use fo4_core::{Budget, Manifest};
use fo4_nav::{Corridor, NavGraph, Navmesh, NavmeshRecord};
use fo4_plugin::typed::Store;
use fo4_plugin::FormKey;
use fo4_scene::collision;
use fo4_scene::{Loader, RuleLevel};
use fo4_vfs::Vfs;
use fo4_walk::World;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

struct Loaded {
    key: FormKey,
    plugin: usize,
    /// Index of the cell in the cell list (0 = the test cell itself).
    cell: usize,
    mesh: Navmesh,
}

fn load_meshes(
    store: &Store,
    m: &Manifest,
    cells: &[FormKey],
) -> Result<Vec<Loaded>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for (ci, cell) in cells.iter().enumerate() {
        let b = Budget::new(m.limits.max_memory);
        let ch = store.children(cell, &b)?;
        for kid in ch.excluded.iter().filter(|k| k.signature == *b"NAVM") {
            let b = Budget::new(m.limits.max_memory);
            let Some((r, plugin)) = store.raw_with_plugin(&kid.key, &b)? else {
                continue;
            };
            let n = NavmeshRecord::decode(&r, &b)?;
            out.push(Loaded {
                key: kid.key.clone(),
                plugin,
                cell: ci,
                mesh: n.mesh,
            });
        }
    }
    Ok(out)
}

fn graph<'a>(
    store: &Store,
    meshes: &'a [Loaded],
    follow: impl Fn(u32) -> bool,
    b: &Budget,
) -> Result<NavGraph<'a>, Box<dyn std::error::Error>> {
    let index: HashMap<&FormKey, usize> = meshes
        .iter()
        .enumerate()
        .map(|(i, l)| (&l.key, i))
        .collect();
    let resolve = |mi: usize, id: u32| {
        let l = &meshes[mi];
        let k = store
            .order()
            .remap(l.plugin, id, &l.key.plugin, 0)
            .ok()
            .flatten()?;
        index.get(&k).copied()
    };
    Ok(NavGraph::build(
        meshes.iter().map(|l| &l.mesh).collect(),
        resolve,
        follow,
        b,
    )?)
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Two far-apart nodes of a component by a double sweep over centroids.
fn far_pair(g: &NavGraph<'_>, labels: &[u32], comp: u32) -> Option<(u32, u32)> {
    let nodes: Vec<u32> = (0..labels.len() as u32)
        .filter(|i| labels[*i as usize] == comp)
        .collect();
    let c = |i: u32| {
        let n = g.node(i);
        g.meshes()[n.mesh as usize]
            .centroid(usize::from(n.triangle))
            .unwrap_or_default()
    };
    let first = *nodes.first()?;
    let far = |from: u32| {
        nodes
            .iter()
            .copied()
            .max_by(|a, b| dist(c(from), c(*a)).total_cmp(&dist(c(from), c(*b))))
    };
    let a = far(first)?;
    let b = far(a)?;
    Some((a, b))
}

fn quantiles(v: &mut [f64]) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    format!(
        "n {} | 5% {:.2} | 25% {:.2} | 50% {:.2} | 75% {:.2} | 95% {:.2} | max {:.2}",
        v.len(),
        q(0.05),
        q(0.25),
        q(0.5),
        q(0.75),
        q(0.95),
        q(1.0)
    )
}

fn buckets(v: &[f64], edges: &[f64]) -> String {
    let mut counts = vec![0u64; edges.len() + 1];
    for x in v {
        counts[edges.partition_point(|e| e <= x)] += 1;
    }
    let mut s = String::new();
    for (i, n) in counts.iter().enumerate() {
        let lo = if i == 0 {
            "-inf".into()
        } else {
            format!("{}", edges[i - 1])
        };
        let hi = if i == edges.len() {
            "+inf".into()
        } else {
            format!("{}", edges[i])
        };
        let _ = writeln!(s, "{n}\t[{lo}, {hi})");
    }
    s
}

#[allow(clippy::too_many_arguments)]
fn test_cell(
    m: &Manifest,
    store: &Store,
    vfs: &Vfs,
    n: usize,
    name: &str,
    radius: u32,
    out_dir: &Path,
    out: &mut String,
    names: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let b = Budget::new(m.limits.max_memory);
    writeln!(names, "test cell {n}: {name} radius {radius}")?;
    let Some(cell) = store.find_cell(name, &b)? else {
        writeln!(out, "\n# test cell {n}: not found")?;
        return Ok(());
    };
    let around = fo4_scene::grid::around(store, &cell, radius, &b)?;
    let mut cells = vec![cell.clone()];
    cells.extend(around.neighbours.iter().filter_map(|x| x.cell.clone()));
    let interior = store.world(&cell).is_none();
    let started = Instant::now();
    let meshes = load_meshes(store, m, &cells)?;
    let load_s = started.elapsed().as_secs_f64();
    writeln!(
        out,
        "\n# test cell {n} ({}, radius {radius}, {} cells)",
        if interior { "interior" } else { "exterior" },
        cells.len()
    )?;
    let own: Vec<&Loaded> = meshes.iter().filter(|l| l.cell == 0).collect();
    let sum = |f: &dyn Fn(&Navmesh) -> usize, only_own: bool| -> usize {
        meshes
            .iter()
            .filter(|l| !only_own || l.cell == 0)
            .map(|l| f(&l.mesh))
            .sum()
    };
    writeln!(out, "navmeshes in the cell\t{}", own.len())?;
    writeln!(
        out,
        "navmeshes in all {} cells\t{}",
        cells.len(),
        meshes.len()
    )?;
    for (label, only) in [("cell", true), ("all cells", false)] {
        writeln!(
            out,
            "vertices ({label})\t{}",
            sum(&|x| x.vertices.len(), only)
        )?;
        writeln!(
            out,
            "triangles ({label})\t{}",
            sum(&|x| x.triangles.len(), only)
        )?;
        writeln!(
            out,
            "edge links ({label})\t{}",
            sum(&|x| x.edge_links.len(), only)
        )?;
        writeln!(
            out,
            "door links ({label})\t{}",
            sum(&|x| x.doors.len(), only)
        )?;
        writeln!(
            out,
            "waypoints ({label})\t{}",
            sum(&|x| x.waypoints.len(), only)
        )?;
    }
    writeln!(out, "load wall seconds\t{load_s:.2}")?;
    let mut best_path: Option<(Corridor, Vec<u32>)> = None;
    for (label, kinds) in [
        ("edge-link kind 0 followed", false),
        ("every edge-link kind followed", true),
    ] {
        let g = graph(store, &meshes, |k| kinds || k == 0, &b)?;
        let (labels, sizes) = g.components(&b)?;
        let mut sorted = sizes.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        writeln!(out, "\n## graph, {label}")?;
        let s = g.stats;
        writeln!(out, "nodes (triangles)\t{}", s.nodes)?;
        writeln!(out, "neighbour arcs\t{}", s.neighbour_arcs)?;
        writeln!(out, "edge-link arcs\t{}", s.link_arcs)?;
        writeln!(
            out,
            "edge links to meshes not loaded\t{}",
            s.links_unresolved
        )?;
        writeln!(
            out,
            "edge links not followed (kind)\t{}",
            s.links_not_followed
        )?;
        writeln!(
            out,
            "edge links with a triangle outside the linked mesh\t{}",
            s.links_bad_triangle
        )?;
        writeln!(out, "invalid edge words\t{}", s.invalid_edges)?;
        writeln!(out, "connected components\t{}", sizes.len())?;
        writeln!(
            out,
            "largest components (triangles)\t{}",
            sorted
                .iter()
                .take(8)
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )?;
        writeln!(
            out,
            "components of one triangle\t{}",
            sizes.iter().filter(|x| **x == 1).count()
        )?;
        let largest = sizes
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| **s)
            .map(|(i, _)| i as u32);
        let Some(comp) = largest else { continue };
        let Some((a, z)) = far_pair(&g, &labels, comp) else {
            continue;
        };
        let (na, nz) = (g.node(a), g.node(z));
        let ca = g.meshes()[na.mesh as usize]
            .centroid(usize::from(na.triangle))
            .unwrap_or_default();
        let cz = g.meshes()[nz.mesh as usize]
            .centroid(usize::from(nz.triangle))
            .unwrap_or_default();
        let t = Instant::now();
        match g.find_path_between(na, ca, nz, cz, &b) {
            Ok(c) => {
                let secs = t.elapsed().as_secs_f64();
                let meshes_crossed: std::collections::BTreeSet<u32> =
                    c.nodes.iter().map(|x| x.mesh).collect();
                let links = c
                    .crossings
                    .iter()
                    .filter(|x| matches!(x, fo4_nav::path::Crossing::Link(_)))
                    .count();
                writeln!(out, "sample path: between the two triangles of the largest component found by a double sweep over centroid distance")?;
                writeln!(
                    out,
                    "straight-line distance between the two centroids\t{:.1}",
                    dist(ca, cz)
                )?;
                writeln!(out, "corridor triangles\t{}", c.nodes.len())?;
                writeln!(out, "meshes crossed\t{}", meshes_crossed.len())?;
                writeln!(out, "edge-link crossings\t{links}")?;
                writeln!(
                    out,
                    "polyline points (start, corners, goal)\t{}",
                    c.points.len()
                )?;
                writeln!(out, "polyline length\t{:.1}", c.length)?;
                writeln!(
                    out,
                    "centroid route length (what A* minimised)\t{:.1}",
                    c.centroid_length
                )?;
                writeln!(
                    out,
                    "polyline length / straight line\t{:.3}",
                    c.length / dist(ca, cz).max(1e-9)
                )?;
                writeln!(out, "nodes expanded\t{}", c.expanded)?;
                writeln!(out, "search wall seconds\t{secs:.4}")?;
                if !kinds {
                    best_path = Some((c, labels));
                }
            }
            Err(e) => writeln!(out, "sample path failed\t{e:?}")?,
        }
    }
    heights(m, store, vfs, name, &cell, &own, out)?;
    if let Some((c, labels)) = &best_path {
        let png = picture(&meshes, labels, c, &b)?;
        std::fs::write(out_dir.join(format!("nav-path-cell-{n}.png")), png)?;
    }
    Ok(())
}

/// Vertex heights of the cell's own meshes against its placed collision and
/// the terrain of the cells around it.
fn heights(
    m: &Manifest,
    store: &Store,
    vfs: &Vfs,
    name: &str,
    cell: &FormKey,
    own: &[&Loaded],
    out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let level = RuleLevel::Hypotheses;
    let budget = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(store, vfs, &m.language, level, &budget);
    let scene = loader.by_editor_id(name, &budget)?;
    let c = collision::cell(
        &scene,
        &loader,
        &m.limits,
        collision::Options {
            level,
            body_placement: None,
        },
        &budget,
    )?;
    let Some(placement) = c.body_placement else {
        writeln!(out, "\n## heights: no body placement level accepted")?;
        return Ok(());
    };
    let mut world = World::build(&c, placement, &budget)?;
    let collision_entries = world.entries.len();
    let mut ground = fo4_walk::terrain::load_around(store, cell, level, true, &budget)?;
    ground.add_to(&mut world, &budget)?;
    writeln!(
        out,
        "\n## heights of the cell's own navmesh vertices against collision"
    )?;
    writeln!(
        out,
        "collision world entries (placed collision, then with terrain)\t{collision_entries}\t{}",
        world.entries.len()
    )?;
    let mut nearest = Vec::new();
    let mut below = Vec::new();
    let mut none = 0u64;
    for l in own {
        for v in &l.mesh.vertices {
            let (x, y, z) = (f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
            let s = world.surfaces(x, y);
            match s
                .iter()
                .map(|h| z - h.z)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
            {
                Some(d) => nearest.push(d),
                None => none += 1,
            }
            if let Some(h) = world.ray_down(x, y, z + 64.0) {
                below.push(z - h.z);
            }
        }
    }
    writeln!(
        out,
        "vertices with no surface on their vertical line\t{none}"
    )?;
    let edges = [-64.0, -16.0, -4.0, -1.0, -0.25, 0.25, 1.0, 4.0, 16.0, 64.0];
    writeln!(
        out,
        "vertex z minus the nearest surface on its vertical line: {}",
        quantiles(&mut nearest)
    )?;
    write!(out, "{}", buckets(&nearest, &edges))?;
    writeln!(
        out,
        "vertex z minus the highest surface at or below z + 64: {}",
        quantiles(&mut below)
    )?;
    write!(out, "{}", buckets(&below, &edges))?;
    Ok(())
}

/// A top-down picture: triangles shaded by component, path in red.
fn picture(
    meshes: &[Loaded],
    labels: &[u32],
    c: &Corridor,
    b: &Budget,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    const W: usize = 1400;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for l in meshes {
        for v in &l.mesh.vertices {
            for (k, x) in v.iter().take(2).enumerate() {
                lo[k] = lo[k].min(f64::from(*x));
                hi[k] = hi[k].max(f64::from(*x));
            }
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let scale = (W as f64 - 20.0) / span;
    let px = |x: f64, y: f64| {
        [
            10.0 + (x - lo[0]) * scale,
            W as f64 - 10.0 - (y - lo[1]) * scale,
        ]
    };
    let mut img = vec![[0.08f32, 0.08, 0.1, 1.0]; W * W];
    let on_path: std::collections::HashSet<(u32, u16)> =
        c.nodes.iter().map(|n| (n.mesh, n.triangle)).collect();
    let mut node = 0usize;
    for (mi, l) in meshes.iter().enumerate() {
        for (ti, t) in l.mesh.triangles.iter().enumerate() {
            let comp = labels.get(node).copied().unwrap_or(0);
            node += 1;
            let h = (comp.wrapping_mul(2654435761) >> 8) as f32 / 16777216.0;
            let mut col = [0.25 + 0.5 * h, 0.35 + 0.3 * (1.0 - h), 0.5, 1.0];
            if comp == 0 {
                col = [0.3, 0.45, 0.35, 1.0];
            }
            if on_path.contains(&(mi as u32, ti as u16)) {
                col = [0.85, 0.75, 0.3, 1.0];
            }
            let p: Vec<[f64; 2]> = t
                .vertices
                .iter()
                .map(|v| {
                    let q = l.mesh.vertices[usize::from(*v)];
                    px(f64::from(q[0]), f64::from(q[1]))
                })
                .collect();
            fill(&mut img, W, &p, col);
            for (k, a) in p.iter().enumerate() {
                line(&mut img, W, *a, p[(k + 1) % 3], [0.05, 0.05, 0.05, 1.0]);
            }
        }
    }
    for w in c.points.windows(2) {
        line(
            &mut img,
            W,
            px(w[0][0], w[0][1]),
            px(w[1][0], w[1][1]),
            [1.0, 0.1, 0.1, 1.0],
        );
    }
    Ok(fo4_render::png::encode([W as u32, W as u32], &img, b)?)
}

fn fill(img: &mut [[f32; 4]], w: usize, p: &[[f64; 2]], col: [f32; 4]) {
    let x0 = p
        .iter()
        .map(|v| v[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let x1 = (p
        .iter()
        .map(|v| v[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as usize)
        .min(w - 1);
    let y0 = p
        .iter()
        .map(|v| v[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.0) as usize;
    let y1 = (p
        .iter()
        .map(|v| v[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as usize)
        .min(w - 1);
    let e = |a: [f64; 2], b: [f64; 2], q: [f64; 2]| {
        (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0])
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let q = [x as f64 + 0.5, y as f64 + 0.5];
            let (a, b, c) = (e(p[0], p[1], q), e(p[1], p[2], q), e(p[2], p[0], q));
            if (a >= 0.0 && b >= 0.0 && c >= 0.0) || (a <= 0.0 && b <= 0.0 && c <= 0.0) {
                img[y * w + x] = col;
            }
        }
    }
}

fn line(img: &mut [[f32; 4]], w: usize, a: [f64; 2], b: [f64; 2], col: [f32; 4]) {
    let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let (x, y) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
        if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < w {
            img[y as usize * w + x as usize] = col;
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("nav_path MANIFEST OUT_DIR CELL[:RADIUS]...".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let out_dir = Path::new(&args[1]);
    std::fs::create_dir_all(out_dir)?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut out = String::from(
        "# navmesh test cells and sample paths (fo4_nav::NavGraph, this project's own search)\n",
    );
    let mut names = String::new();
    for (i, a) in args[2..].iter().enumerate() {
        let (cell, radius) = match a.split_once(':') {
            Some((c, r)) => (c, r.parse::<u32>()?),
            None => (a.as_str(), 0),
        };
        test_cell(
            &m,
            &store,
            &vfs,
            i + 1,
            cell,
            radius,
            out_dir,
            &mut out,
            &mut names,
        )?;
    }
    std::fs::write(out_dir.join("nav-path.txt"), out)?;
    std::fs::write(out_dir.join("nav-path-names.txt"), names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
