//! The NAVM pass: every occurrence read to the last byte, then the winning
//! live meshes checked against themselves, each other, the cells, placed
//! doors and the terrain.
use crate::geom::{box_meets_rect, triangle_meets_rect};
use crate::stats::{
    add, bump, check, checks, counts, distribution, section, top, Checks, Counts, Range,
};
use fo4_core::{Budget, Manifest};
use fo4_nav::mesh::Parts;
use fo4_nav::path::height_in;
use fo4_nav::{Edge, Navmesh, NavmeshRecord};
use fo4_plugin::typed::{Cell, CellKind, Store};
use fo4_plugin::{FormKey, Record};
use fo4_scene::terrain::{self, Outcome};
use fo4_scene::RuleLevel;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

/// What the NAVI pass needs of each winning live mesh.
pub struct MeshInfo {
    pub key: FormKey,
    pub world: Option<FormKey>,
    pub parent: u32,
    /// Word 3 resolved as a FormID when word 2 is zero.
    pub parent_cell: Option<FormKey>,
    pub triangles: usize,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub link_targets: BTreeSet<FormKey>,
    /// Other meshes named by edge links, by kind word.
    pub link_targets_by_kind: std::collections::BTreeMap<u32, BTreeSet<FormKey>>,
    /// Plugins holding an occurrence (definition or override) of the mesh.
    pub plugins: BTreeSet<usize>,
    pub doors: BTreeSet<FormKey>,
    pub vertices: Vec<[f32; 3]>,
}

pub struct Summary {
    pub index: HashMap<FormKey, usize>,
    pub meshes: Vec<MeshInfo>,
}

struct LinkRec {
    mesh: u32,
    tri: u16,
    edge: u8,
    kind: u32,
    target: Result<FormKey, &'static str>,
    ttri: u16,
    tedge: u8,
    ends: [[f32; 3]; 2],
}

struct DoorRec {
    mesh: u32,
    /// Centroid of the door link's triangle.
    centroid: Option<[f64; 3]>,
    door: FormKey,
}

#[derive(Default)]
struct Acc {
    c: Counts,
    chk: Checks,
    raw: std::collections::BTreeMap<String, Counts>,
    sizes: std::collections::BTreeMap<String, Range>,
    parts: [u64; 9],
    dz_terrain: Vec<f64>,
    word_minus_top: Vec<f64>,
    waypoint_dz: Vec<f64>,
    bounds_out: Vec<f64>,
    door_distance: Vec<f64>,
}
impl Acc {
    fn raw(&mut self, field: &str, v: impl std::fmt::Display) {
        bump(self.raw.entry(field.into()).or_default(), v.to_string());
    }
    fn size(&mut self, field: &str, v: u64) {
        self.sizes.entry(field.into()).or_default().put(v);
    }
}

fn resolve(store: &Store, plugin: usize, id: u32, r: &Record, off: u64) -> Option<FormKey> {
    store
        .order()
        .remap(plugin, id, &r.source.file, off)
        .ok()
        .flatten()
}

/// The type of the winning record of `k`, or why there is none.
fn kind_of(store: &Store, k: &FormKey) -> String {
    match store.overrides().winner(k) {
        None => "no record".into(),
        Some(w) if w.deleted => format!("{} (deleted)", String::from_utf8_lossy(&w.signature)),
        Some(w) => String::from_utf8_lossy(&w.signature).into_owned(),
    }
}

pub fn run(
    m: &Manifest,
    store: &Store,
    out: &mut String,
    names: &mut String,
) -> Result<Summary, Box<dyn std::error::Error>> {
    let mut a = Acc::default();
    // Group parent of every winning NAVM.
    let mut group_cell: HashMap<u64, FormKey> = HashMap::new();
    for (o, cell) in store.placement_cells() {
        if o.signature == *b"NAVM" {
            group_cell.insert(o.key, cell);
        }
    }
    let mut summary = Summary {
        index: HashMap::new(),
        meshes: Vec::new(),
    };
    let mut links: Vec<LinkRec> = Vec::new();
    let mut doors: Vec<DoorRec> = Vec::new();
    let history = &store.overrides().history;
    for (i, o) in history.iter().enumerate() {
        if o.signature != *b"NAVM" {
            continue;
        }
        let winner = store
            .overrides()
            .winner_id(o.key)
            .is_some_and(|w| std::ptr::eq(w, o));
        let role = if winner { "winning" } else { "overridden" };
        bump(&mut a.c, format!("occurrences: {role}"));
        bump(
            &mut a.c,
            format!("occurrences by plugin index: {}", o.plugin),
        );
        if o.deleted {
            bump(&mut a.c, format!("deleted ({role})"));
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let (r, plugin) = store.raw_occurrence(i, &b)?;
        a.raw("record version", r.version);
        a.raw("record flags", format!("{:#x}", r.flags));
        add(
            &mut a.c,
            "record payload bytes (live)",
            r.payload.len() as u64,
        );
        let decoded = NavmeshRecord::decode(&r, &b);
        let n = match decoded {
            Err(e) => {
                bump(&mut a.c, format!("refused ({role}): {}", e.code));
                add(
                    &mut a.c,
                    "record payload bytes refused",
                    r.payload.len() as u64,
                );
                writeln!(names, "refused {role} NAVM {e}")?;
                continue;
            }
            Ok(n) => n,
        };
        bump(&mut a.c, format!("read to the last byte ({role})"));
        add(
            &mut a.c,
            "record payload bytes read to the last byte",
            r.payload.len() as u64,
        );
        let framing: u64 = r.subrecords.len() as u64 * 6
            + r.subrecords.iter().filter(|s| s.extended).count() as u64 * 10;
        add(&mut a.c, "subrecord framing bytes", framing);
        for (name, v) in ["NVNM", "ONAM", "NNAM", "MNAM"].iter().zip(n.sizes) {
            add(&mut a.c, format!("payload bytes {name}"), v);
        }
        let seq: Vec<String> = r
            .subrecords
            .iter()
            .map(|s| String::from_utf8_lossy(&s.signature).into_owned())
            .collect();
        a.raw("subrecord sequence", seq.join(" "));
        layout(&mut a, &n);
        if !winner {
            continue;
        }
        let key = store.overrides().form_key(o.key, store.order());
        let mi = summary.meshes.len() as u32;
        let info = winner_checks(
            &mut a,
            store,
            m,
            &r,
            plugin,
            &n,
            key.clone(),
            group_cell.get(&o.key),
            mi,
            &mut links,
            &mut doors,
        )?;
        summary.index.insert(key, summary.meshes.len());
        summary.meshes.push(info);
    }
    let mut occ_plugins: HashMap<u64, BTreeSet<usize>> = HashMap::new();
    for o in history.iter().filter(|o| o.signature == *b"NAVM") {
        occ_plugins.entry(o.key).or_default().insert(o.plugin);
    }
    for mi in summary.meshes.iter_mut() {
        if let Some(w) = store.overrides().winner(&mi.key) {
            mi.plugins = occ_plugins.remove(&w.key).unwrap_or_default();
        }
    }
    cross_links(&mut a, &summary, &links);
    door_checks(&mut a, store, m, &summary, &doors)?;
    report(out, &mut a);
    Ok(summary)
}

/// Field histograms of one decoded record (every occurrence).
fn layout(a: &mut Acc, n: &NavmeshRecord) {
    let mesh = &n.mesh;
    for (slot, v) in a.parts.iter_mut().zip(mesh.parts.values()) {
        *slot += v;
    }
    a.raw("header word 2 is zero", mesh.world == 0);
    a.size("vertices per mesh", mesh.vertices.len() as u64);
    a.size("triangles per mesh", mesh.triangles.len() as u64);
    a.size("edge links per mesh", mesh.edge_links.len() as u64);
    a.size("door links per mesh", mesh.doors.len() as u64);
    a.size("cover segments per mesh", mesh.cover.len() as u64);
    a.size("cover mappings per mesh", mesh.cover_map.len() as u64);
    a.size("waypoints per mesh", mesh.waypoints.len() as u64);
    a.size("grid divisions", u64::from(mesh.grid.divisions));
    a.size(
        "grid triangle entries per mesh",
        mesh.grid.triangles.len() as u64,
    );
    for t in &mesh.triangles {
        a.raw(
            "triangle bytes 12-15 (f32)",
            if t.word == f32::MAX {
                "largest finite f32"
            } else if t.word.is_finite() {
                "other finite"
            } else {
                "not finite"
            },
        );
        for (bi, v) in t.flags.iter().enumerate() {
            a.raw(&format!("triangle byte {}", 16 + bi), format!("{v:#04x}"));
        }
    }
    for l in &mesh.edge_links {
        a.raw("edge link kind word", l.kind);
        a.raw("edge link edge byte", l.edge);
    }
    for d in &mesh.doors {
        a.raw("door link word", format!("{:#010x}", d.word));
    }
    for c in &mesh.cover {
        a.raw("cover segment data word", format!("{:#06x}", c.data));
        a.raw("cover segment flags word", format!("{:#06x}", c.flags));
    }
    for w in &mesh.waypoints {
        a.raw("waypoint flags", format!("{:#x}", w.flags));
    }
    if n.onam.is_some() {
        a.raw("ONAM size", n.sizes[1]);
    }
    if let Some(v) = &n.nnam {
        a.size("NNAM entries", v.len() as u64);
    }
    a.size("MNAM groups", n.mnam.len() as u64);
}

#[allow(clippy::too_many_arguments)]
fn winner_checks(
    a: &mut Acc,
    store: &Store,
    m: &Manifest,
    r: &Record,
    plugin: usize,
    n: &NavmeshRecord,
    key: FormKey,
    cell: Option<&FormKey>,
    mi: u32,
    links: &mut Vec<LinkRec>,
    doors: &mut Vec<DoorRec>,
) -> Result<MeshInfo, Box<dyn std::error::Error>> {
    let mesh = &n.mesh;
    let off = r.source.offset;
    let nt = mesh.triangles.len();
    let nv = mesh.vertices.len();
    let nl = mesh.edge_links.len();
    bump(&mut a.c, "winning live meshes checked");
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in &mesh.vertices {
        for ((lo, hi), x) in min.iter_mut().zip(max.iter_mut()).zip(v) {
            *lo = lo.min(*x);
            *hi = hi.max(*x);
        }
    }
    // Neighbour words.
    let mut link_refs = vec![0u32; nl];
    for (ti, t) in mesh.triangles.iter().enumerate() {
        for e in 0..3 {
            match t.edge(e, nt, nl) {
                Edge::Open => bump(&mut a.c, "edges: open"),
                Edge::Invalid(_) => {
                    bump(&mut a.c, "edges: invalid word");
                    check(
                        &mut a.chk,
                        "edge word is -1, a triangle index, or (link bit) an edge-link index",
                        false,
                    );
                }
                Edge::Link(k) => {
                    bump(&mut a.c, "edges: edge link");
                    link_refs[usize::from(k)] += 1;
                    check(
                        &mut a.chk,
                        "edge word is -1, a triangle index, or (link bit) an edge-link index",
                        true,
                    );
                }
                Edge::Triangle(u) => {
                    bump(&mut a.c, "edges: neighbour");
                    check(
                        &mut a.chk,
                        "edge word is -1, a triangle index, or (link bit) an edge-link index",
                        true,
                    );
                    let other = &mesh.triangles[usize::from(u)];
                    let [p, q] = t.edge_vertices(e);
                    let back = (0..3).find(|f| other.edge(*f, nt, nl) == Edge::Triangle(ti as u16));
                    check(
                        &mut a.chk,
                        "neighbour names this triangle back",
                        back.is_some(),
                    );
                    if let Some(f) = back {
                        let [x, y] = other.edge_vertices(f);
                        check(
                            &mut a.chk,
                            "neighbour names it back across the same two vertices (reversed)",
                            x == q && y == p,
                        );
                        check(
                            &mut a.chk,
                            "neighbour names it back across the same two vertices (any order)",
                            (x == q && y == p) || (x == p && y == q),
                        );
                    }
                    check(
                        &mut a.chk,
                        "neighbour is not the triangle itself",
                        usize::from(u) != ti,
                    );
                }
            }
        }
        if t.word == f32::MAX {
            continue;
        }
        let top = t
            .vertices
            .iter()
            .map(|v| f64::from(mesh.vertices[usize::from(*v)][2]))
            .fold(f64::NEG_INFINITY, f64::max);
        a.word_minus_top.push(f64::from(t.word) - top);
    }
    for refs in &link_refs {
        check(
            &mut a.chk,
            "edge link named by exactly one triangle edge with the link bit",
            *refs == 1,
        );
    }
    // Edge links, for the cross-mesh pass.
    for (ti, t) in mesh.triangles.iter().enumerate() {
        for e in 0..3 {
            let Edge::Link(k) = t.edge(e, nt, nl) else {
                continue;
            };
            let l = mesh.edge_links[usize::from(k)];
            let target = match resolve(store, plugin, l.navmesh, r, off) {
                None => Err("FormID does not resolve"),
                Some(tk) => match store.overrides().winner(&tk) {
                    None => Err("no record"),
                    Some(w) if w.signature != *b"NAVM" => Err("not a NAVM"),
                    Some(w) if w.deleted => Err("deleted NAVM"),
                    Some(_) => Ok(tk),
                },
            };
            let [p, q] = t.edge_vertices(e);
            links.push(LinkRec {
                mesh: mi,
                tri: ti as u16,
                edge: e as u8,
                kind: l.kind,
                target,
                ttri: l.triangle,
                tedge: l.edge,
                ends: [mesh.vertices[usize::from(p)], mesh.vertices[usize::from(q)]],
            });
        }
    }
    let mut link_targets = BTreeSet::new();
    let mut link_targets_by_kind: std::collections::BTreeMap<u32, BTreeSet<FormKey>> =
        std::collections::BTreeMap::new();
    for l in &mesh.edge_links {
        if let Some(k) = resolve(store, plugin, l.navmesh, r, off) {
            if k != key {
                link_targets_by_kind
                    .entry(l.kind)
                    .or_default()
                    .insert(k.clone());
            }
            link_targets.insert(k);
        }
    }
    // Cover.
    let mut flagged = 0u64;
    for t in &mesh.triangles {
        flagged += u64::from((t.flags[3] & 7).count_ones());
        a.raw("triangle byte 19 bits 3-7", t.flags[3] >> 3);
    }
    check(
        &mut a.chk,
        "cover mappings per mesh equal the cover bits set in byte 19",
        flagged == mesh.cover_map.len() as u64,
    );
    let mut mapped = vec![false; mesh.cover.len()];
    for c in &mesh.cover_map {
        let ok_seg = usize::from(c.segment) < mesh.cover.len();
        check(&mut a.chk, "cover mapping segment index < segments", ok_seg);
        if ok_seg {
            mapped[usize::from(c.segment)] = true;
        }
        let t = usize::from(c.triangle());
        let e = usize::from(c.edge());
        check(
            &mut a.chk,
            "cover mapping triangle (packed / 4) < triangles",
            t < nt,
        );
        check(&mut a.chk, "cover mapping edge (packed % 4) < 3", e < 3);
        if t < nt && e < 3 {
            check(
                &mut a.chk,
                "cover mapping names an edge whose byte-19 bit is set",
                mesh.triangles[t].has_cover(e),
            );
            check(
                &mut a.chk,
                "cover mapping names an open edge (word -1, no link bit)",
                mesh.edge(t, e) == Edge::Open,
            );
        }
    }
    for x in &mapped {
        check(
            &mut a.chk,
            "cover segment named by at least one mapping",
            *x,
        );
    }
    for (si, s) in mesh.cover.iter().enumerate() {
        for p in s.points {
            check(
                &mut a.chk,
                "cover segment point word < vertex count",
                usize::from(p) < nv,
            );
        }
        let start = s.flags & 0x8000 != 0;
        if si == 0 {
            check(&mut a.chk, "first cover segment has flags bit 15", start);
        } else if !start {
            check(
                &mut a.chk,
                "a segment without bit 15 starts where the previous one ends",
                mesh.cover[si - 1].points[1] == s.points[0],
            );
        } else {
            check(
                &mut a.chk,
                "a segment with bit 15 does not start where the previous one ends",
                mesh.cover[si - 1].points[1] != s.points[0],
            );
        }
    }
    // Waypoints.
    for w in &mesh.waypoints {
        let t = usize::from(w.triangle);
        check(&mut a.chk, "waypoint triangle < triangles", t < nt);
        if let Some(c) = mesh.corners(t) {
            let p = w.position.map(f64::from);
            match height_in(&c, p[0], p[1]) {
                Some(z) => {
                    check(&mut a.chk, "waypoint x, y inside its triangle", true);
                    a.waypoint_dz.push(p[2] - z);
                }
                None => check(&mut a.chk, "waypoint x, y inside its triangle", false),
            }
        }
    }
    // Doors.
    let mut door_keys = BTreeSet::new();
    for d in &mesh.doors {
        check(
            &mut a.chk,
            "door link triangle < triangles",
            usize::from(d.triangle) < nt,
        );
        let k = resolve(store, plugin, d.door, r, off);
        check(&mut a.chk, "door link FormID resolves", k.is_some());
        if let Some(k) = k {
            door_keys.insert(k.clone());
            doors.push(DoorRec {
                mesh: mi,
                centroid: mesh.centroid(usize::from(d.triangle)),
                door: k,
            });
        }
    }
    // Companions.
    if let Some(o) = n.onam {
        match resolve(store, plugin, o, r, off) {
            Some(k) => a.raw("ONAM names", kind_of(store, &k)),
            None => a.raw("ONAM names", "unresolved"),
        }
    }
    if let Some(v) = &n.nnam {
        for x in v {
            check(
                &mut a.chk,
                "NNAM entry < vertex count",
                usize::from(*x) < nv,
            );
        }
        check(
            &mut a.chk,
            "NNAM entries strictly ascending",
            v.windows(2).all(|w| w[0] < w[1]),
        );
    }
    for g in &n.mnam {
        match resolve(store, plugin, g.form, r, off) {
            Some(k) => a.raw("MNAM group FormID names", kind_of(store, &k)),
            None => a.raw("MNAM group FormID names", "unresolved"),
        }
        for t in &g.triangles {
            check(
                &mut a.chk,
                "MNAM triangle < triangles",
                usize::from(*t) < nt,
            );
        }
    }
    // Grid.
    grid_checks(a, mesh, min, max);
    // Parent.
    let world = resolve(store, plugin, mesh.world, r, off);
    let b = Budget::new(m.limits.max_memory);
    match cell {
        None => bump(&mut a.c, "winning mesh outside a cell children group"),
        Some(cell) => {
            let kind = store.kind(cell);
            let kind_name = match kind {
                Some(CellKind::Interior) => "interior cell",
                Some(CellKind::Exterior) => "exterior cell",
                None => "cell of unknown kind",
            };
            bump(
                &mut a.c,
                format!(
                    "group parent: {kind_name}, header word 2 {}",
                    if mesh.world == 0 { "zero" } else { "non-zero" }
                ),
            );
            if mesh.world == 0 {
                let p = resolve(store, plugin, mesh.parent, r, off);
                check(
                    &mut a.chk,
                    "word 2 zero: word 3 is the FormID of the group's parent cell",
                    p.as_ref() == Some(cell),
                );
            } else {
                check(
                    &mut a.chk,
                    "word 2 non-zero: it names the world whose group holds the parent cell",
                    world.is_some() && world == store.world(cell),
                );
                let grid = store.get::<Cell>(cell, &b)?.and_then(|c| c.data.grid);
                if let Some(g) = grid {
                    let hi = i32::from((mesh.parent >> 16) as u16 as i16);
                    let lo = i32::from(mesh.parent as u16 as i16);
                    check(
                        &mut a.chk,
                        "word 3 high half = cell grid x and low half = grid y",
                        hi == g.i && lo == g.j,
                    );
                    check(
                        &mut a.chk,
                        "word 3 low half = cell grid x and high half = grid y",
                        lo == g.i && hi == g.j,
                    );
                    exterior_vertices(a, store, mesh, cell, [g.i, g.j], &b)?;
                } else {
                    bump(&mut a.c, "exterior parent cell without XCLC");
                }
            }
        }
    }
    Ok(MeshInfo {
        key,
        world,
        parent: mesh.parent,
        parent_cell: if mesh.world == 0 {
            resolve(store, plugin, mesh.parent, r, off)
        } else {
            None
        },
        triangles: nt,
        min,
        max,
        link_targets,
        link_targets_by_kind,
        plugins: BTreeSet::new(),
        doors: door_keys,
        vertices: mesh.vertices.clone(),
    })
}

fn exterior_vertices(
    a: &mut Acc,
    store: &Store,
    mesh: &Navmesh,
    cell: &FormKey,
    g: [i32; 2],
    b: &Budget,
) -> Result<(), Box<dyn std::error::Error>> {
    const SIZE: f64 = 4096.0;
    let (x0, y0) = (f64::from(g[0]) * SIZE, f64::from(g[1]) * SIZE);
    for v in &mesh.vertices {
        let (x, y) = (f64::from(v[0]), f64::from(v[1]));
        let dx = (x0 - x).max(x - (x0 + SIZE)).max(0.0);
        let dy = (y0 - y).max(y - (y0 + SIZE)).max(0.0);
        let d = dx.max(dy);
        check(
            &mut a.chk,
            "exterior vertex inside its cell (grid x 4096) footprint",
            d == 0.0,
        );
        if d > 0.0 {
            a.bounds_out.push(d);
        }
    }
    let opts = terrain::Options {
        level: RuleLevel::Hypotheses,
        diagonal: None,
    };
    match terrain::load(store, cell, opts, b)? {
        Outcome::Refused(r) => bump(&mut a.c, format!("terrain refused: {}", r.code)),
        Outcome::Built(t) => {
            bump(&mut a.c, "terrain built for the parent cell");
            for v in &mesh.vertices {
                match t.height_at(f64::from(v[0]), f64::from(v[1])) {
                    Some(h) => a.dz_terrain.push(f64::from(v[2]) - h),
                    None => bump(&mut a.c, "vertex outside the terrain footprint"),
                }
            }
        }
    }
    Ok(())
}

fn grid_checks(a: &mut Acc, mesh: &Navmesh, min: [f32; 3], max: [f32; 3]) {
    let g = &mesh.grid;
    let nt = mesh.triangles.len();
    let div = g.divisions as usize;
    check(
        &mut a.chk,
        "grid square count = divisions squared",
        g.squares() == div * div,
    );
    if div == 0 || nt == 0 {
        bump(&mut a.c, "grid with no division or mesh with no triangle");
        return;
    }
    for (k, ((size, lo), hi)) in g.size.iter().zip(&g.min).zip(&g.max).enumerate() {
        let derived = (hi - lo) / g.divisions as f32;
        check(
            &mut a.chk,
            format!("grid size {k} = (max - min) / divisions within 1e-3"),
            (derived - size).abs() <= 1e-3 * size.abs().max(1.0),
        );
    }
    let top_word = mesh
        .triangles
        .iter()
        .map(|t| t.word)
        .fold(f32::NEG_INFINITY, f32::max);
    check(
        &mut a.chk,
        "grid max z = the larger of the vertex maximum z and the largest triangle word",
        g.max[2] == max[2].max(top_word),
    );
    for (k, (((glo, ghi), lo), hi)) in g.min.iter().zip(&g.max).zip(&min).zip(&max).enumerate() {
        check(
            &mut a.chk,
            format!("grid min {k} = vertex minimum"),
            glo == lo,
        );
        check(
            &mut a.chk,
            format!("grid max {k} = vertex maximum"),
            ghi == hi,
        );
    }
    for t in &g.triangles {
        check(
            &mut a.chk,
            "grid triangle < triangles",
            usize::from(*t) < nt,
        );
    }
    let mut listed = vec![0u32; nt];
    for t in &g.triangles {
        if let Some(x) = listed.get_mut(usize::from(*t)) {
            *x += 1;
        }
    }
    for x in &listed {
        check(
            &mut a.chk,
            "every triangle listed in at least one square",
            *x > 0,
        );
    }
    // Squares each triangle meets, as (row along y, column along x).
    let sx = f64::from(g.size[0]);
    let sy = f64::from(g.size[1]);
    let (gx, gy) = (f64::from(g.min[0]), f64::from(g.min[1]));
    // Candidate criteria: the triangle itself or its bounding box, with the
    // square grown (positive slack) or shrunk (negative slack).
    let criteria: [(&str, f64, bool); 6] = [
        ("triangle, slack +0.001", 1e-3, true),
        ("triangle, slack -0.001", -1e-3, true),
        ("triangle, slack +16", 16.0, true),
        ("bounding box, slack +0.001", 1e-3, false),
        ("bounding box, slack -0.001", -1e-3, false),
        ("bounding box, slack -16", -16.0, false),
    ];
    for (ci, (crit, eps, exact_shape)) in criteria.iter().enumerate() {
        let mut computed: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); div * div];
        for (ti, _) in mesh.triangles.iter().enumerate() {
            let Some(c) = mesh.corners(ti) else { continue };
            for row in 0..div {
                let y0 = gy + row as f64 * sy;
                for col in 0..div {
                    let x0 = gx + col as f64 * sx;
                    let meets = if *exact_shape {
                        triangle_meets_rect(&c, x0, x0 + sx, y0, y0 + sy, *eps)
                    } else {
                        box_meets_rect(&c, x0, x0 + sx, y0, y0 + sy, *eps)
                    };
                    if meets {
                        computed[row * div + col].insert(ti as u16);
                    }
                }
            }
        }
        let orders: &[(&str, bool)] = if ci == 0 {
            &[
                ("square = row (y) x divisions + column (x)", false),
                ("square = column (x) x divisions + row (y)", true),
            ]
        } else {
            &[("square = row (y) x divisions + column (x)", false)]
        };
        for (order, transpose) in orders {
            let mut exact = true;
            let mut extra = 0u64;
            let mut missing = 0u64;
            for k in 0..div * div {
                let (row, col) = (k / div, k % div);
                let idx = if *transpose { col * div + row } else { k };
                let stored: BTreeSet<u16> = g.square(k).iter().copied().collect();
                let want = &computed[idx];
                let ok = &stored == want;
                exact &= ok;
                extra += stored.difference(want).count() as u64;
                missing += want.difference(&stored).count() as u64;
                check(
                    &mut a.chk,
                    format!("grid square lists exactly the triangles meeting it: {crit}, {order}"),
                    ok,
                );
            }
            check(
                &mut a.chk,
                format!("grid mesh with every square exact: {crit}, {order}"),
                exact,
            );
            add(
                &mut a.c,
                format!("grid: listed but not meeting ({crit}, {order})"),
                extra,
            );
            add(
                &mut a.c,
                format!("grid: meeting but not listed ({crit}, {order})"),
                missing,
            );
        }
    }
}

fn cross_links(a: &mut Acc, summary: &Summary, links: &[LinkRec]) {
    let target = |l: &LinkRec| -> Result<u32, &'static str> {
        match &l.target {
            Err(e) => Err(*e),
            Ok(k) => summary
                .index
                .get(k)
                .map(|i| *i as u32)
                .ok_or("NAVM not among the winning live meshes"),
        }
    };
    let mut by_src: HashMap<(u32, u16, u8), usize> = HashMap::new();
    for (i, l) in links.iter().enumerate() {
        by_src.insert((l.mesh, l.tri, l.edge), i);
    }
    let mut kinds = Counts::new();
    for l in links {
        let t = target(l);
        match &t {
            Err(e) => bump(&mut a.c, format!("edge link target: {e}")),
            Ok(_) => bump(&mut a.c, "edge link target: winning live NAVM"),
        }
        check(&mut a.chk, "edge link names a winning live NAVM", t.is_ok());
        let Ok(ti) = t else { continue };
        let tm = &summary.meshes[ti as usize];
        let in_range = usize::from(l.ttri) < tm.triangles;
        check(
            &mut a.chk,
            "edge link triangle < triangles of the linked mesh",
            in_range,
        );
        check(&mut a.chk, "edge link names another mesh", ti != l.mesh);
        bump(
            &mut a.c,
            format!(
                "edge link to {} mesh: kind {}",
                if ti == l.mesh { "its own" } else { "another" },
                l.kind
            ),
        );
        if !in_range {
            continue;
        }
        // The linked triangle's edges that link back to this triangle.
        let back: Vec<(u8, &LinkRec)> = (0..3u8)
            .filter_map(|g| by_src.get(&(ti, l.ttri, g)).map(|j| (g, &links[*j])))
            .filter(|(_, b)| target(b) == Ok(l.mesh) && b.ttri == l.tri)
            .collect();
        check(
            &mut a.chk,
            "edge link is mutual (linked triangle links back to this triangle)",
            !back.is_empty(),
        );
        let Some((g, bl)) = back.first() else {
            continue;
        };
        check(
            &mut a.chk,
            "mutual link: edge byte = the linked triangle's edge that links back",
            *g == l.tedge,
        );
        check(
            &mut a.chk,
            "mutual link: the back link's edge byte = this edge",
            bl.tedge == l.edge,
        );
        check(
            &mut a.chk,
            "mutual link: same kind word both ways",
            bl.kind == l.kind,
        );
        bump(&mut kinds, format!("kind {} <-> kind {}", l.kind, bl.kind));
        // The two edges' end points: same places, reversed.
        let d = |p: [f32; 3], q: [f32; 3]| {
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
        };
        let rev = d(l.ends[0], bl.ends[1]).max(d(l.ends[1], bl.ends[0]));
        let same = d(l.ends[0], bl.ends[0]).max(d(l.ends[1], bl.ends[1]));
        let best = rev.min(same);
        bump(
            &mut a.c,
            format!(
                "mutual link ({} mesh) edge end points: largest distance {}",
                if ti == l.mesh { "own" } else { "other" },
                match best {
                    0.0 => "0 (identical)",
                    x if x < 0.01 => "< 0.01",
                    x if x < 1.0 => "< 1",
                    x if x < 16.0 => "< 16",
                    _ => ">= 16",
                }
            ),
        );
        bump(
            &mut a.c,
            format!(
                "mutual link edge end points: {}",
                if rev <= same {
                    "reversed order closer"
                } else {
                    "same order closer"
                }
            ),
        );
    }
    for (k, v) in kinds {
        add(&mut a.c, format!("mutual link kinds: {k}"), v);
    }
}

fn door_checks(
    a: &mut Acc,
    store: &Store,
    m: &Manifest,
    summary: &Summary,
    doors: &[DoorRec],
) -> Result<(), Box<dyn std::error::Error>> {
    let wanted: HashSet<&FormKey> = doors.iter().map(|d| &d.door).collect();
    let mut placed_in: HashMap<FormKey, FormKey> = HashMap::new();
    for (o, cell) in store.placement_cells() {
        let k = store.overrides().form_key(o.key, store.order());
        if wanted.contains(&k) {
            placed_in.insert(k, cell);
        }
    }
    let mut navm_cell: HashMap<&FormKey, FormKey> = HashMap::new();
    for (o, cell) in store.placement_cells() {
        if o.signature == *b"NAVM" {
            let k = store.overrides().form_key(o.key, store.order());
            if let Some(i) = summary.index.get(&k) {
                navm_cell.insert(&summary.meshes[*i].key, cell);
            }
        }
    }
    let mut base_of: HashMap<FormKey, (String, Option<[f32; 3]>)> = HashMap::new();
    for d in doors {
        let entry = match base_of.get(&d.door) {
            Some(x) => x.clone(),
            None => {
                let b = Budget::new(m.limits.max_memory);
                let k = kind_of(store, &d.door);
                let x = if k == "REFR" {
                    let (r, plugin) = store.raw_with_plugin(&d.door, &b)?.ok_or("door vanished")?;
                    let base = r
                        .subrecords
                        .iter()
                        .find(|s| s.signature == *b"NAME")
                        .and_then(|s| {
                            let v = r.data(s);
                            let id = u32::from_le_bytes(v.get(..4)?.try_into().ok()?);
                            resolve(store, plugin, id, &r, s.offset)
                        })
                        .map_or("REFR without a resolved base".into(), |bk| {
                            format!("REFR of {}", kind_of(store, &bk))
                        });
                    let pos = r
                        .subrecords
                        .iter()
                        .find(|s| s.signature == *b"DATA")
                        .and_then(|s| {
                            let v = r.data(s);
                            (v.len() >= 12).then(|| {
                                [0, 4, 8].map(|o| {
                                    f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]])
                                })
                            })
                        });
                    (base, pos)
                } else {
                    (k, None)
                };
                base_of.insert(d.door.clone(), x.clone());
                x
            }
        };
        bump(&mut a.c, format!("door link names: {}", entry.0));
        check(
            &mut a.chk,
            "door link names a placed reference of a DOOR",
            entry.0 == "REFR of DOOR",
        );
        let mesh = &summary.meshes[d.mesh as usize];
        let same_cell = placed_in.get(&d.door).zip(navm_cell.get(&mesh.key));
        bump(
            &mut a.c,
            format!(
                "door link: door placed in {}",
                match same_cell {
                    None => "an unknown cell",
                    Some((x, y)) if x == y => "the mesh's own cell",
                    Some(_) => "another cell",
                }
            ),
        );
        if let (Some(p), Some(c)) = (entry.1, d.centroid) {
            let dxy = ((f64::from(p[0]) - c[0]).powi(2) + (f64::from(p[1]) - c[1]).powi(2)).sqrt();
            a.door_distance.push(dxy);
        }
    }
    Ok(())
}

fn report(out: &mut String, a: &mut Acc) {
    counts(out, "records, bytes and outcomes", &a.c);
    section(
        out,
        "NVNM bytes by section (every occurrence read to the last byte)",
    );
    let total: u64 = a.parts.iter().sum();
    for (name, v) in Parts::NAMES.iter().zip(a.parts) {
        let _ = writeln!(out, "{v}\t{name}");
    }
    let _ = writeln!(out, "{total}\ttotal");
    section(out, "sizes per occurrence read");
    for (k, r) in &a.sizes {
        let _ = writeln!(out, "{k}: {r}");
    }
    for (k, m) in &a.raw {
        top(out, k, m, 24);
    }
    checks(
        out,
        "consistency checks over the winning live meshes",
        &a.chk,
    );
    distribution(
        out,
        "exterior mesh vertex z minus terrain height at its x, y (parent cell's LAND, default diagonal)",
        &mut a.dz_terrain,
    );
    distribution(
        out,
        "triangle bytes 12-15 (finite values) minus the triangle's highest vertex z",
        &mut a.word_minus_top,
    );
    distribution(
        out,
        "waypoint z minus its triangle's plane at x, y",
        &mut a.waypoint_dz,
    );
    distribution(
        out,
        "exterior vertices outside their cell footprint: distance outside",
        &mut a.bounds_out,
    );
    distribution(
        out,
        "door link: horizontal distance from the placed door to its triangle's centroid",
        &mut a.door_distance,
    );
}
