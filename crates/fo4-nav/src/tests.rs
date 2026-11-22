//! Synthetic tests: hand-built meshes, no game data.
use crate::info::{NavInfo, NavMeshInfo, ISLAND_FLAG};
use crate::mesh::{Edge, Navmesh};
use crate::path::{funnel, Crossing, NavGraph, PathError};
use crate::record::NavmeshRecord;
use crate::{MAGIC, VERSION};
use fo4_core::{Budget, SourceSpan};
use fo4_plugin::{Record, Subrecord};

fn budget() -> Budget {
    Budget::new(64 << 20)
}

/// A test encoder for the measured `NVNM` layout.
#[derive(Default, Clone)]
struct Spec {
    world: u32,
    parent: u32,
    vertices: Vec<[f32; 3]>,
    /// vertices, edges, link bits, cover bits
    triangles: Vec<([u16; 3], [i16; 3], u8, u8)>,
    links: Vec<(u32, u32, u16, u8)>,
    doors: Vec<(u16, u32, u32)>,
    cover: Vec<[u16; 4]>,
    cover_map: Vec<[u16; 2]>,
    waypoints: Vec<([f32; 3], u16, u32)>,
    divisions: u32,
    squares: Vec<Vec<u16>>,
}

impl Spec {
    fn bytes(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let u32s = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
        for w in [VERSION, MAGIC, self.world, self.parent] {
            u32s(&mut o, w);
        }
        u32s(&mut o, self.vertices.len() as u32);
        for v in &self.vertices {
            for c in v {
                o.extend_from_slice(&c.to_le_bytes());
            }
        }
        u32s(&mut o, self.triangles.len() as u32);
        for (v, e, link, cover) in &self.triangles {
            for x in v {
                o.extend_from_slice(&x.to_le_bytes());
            }
            for x in e {
                o.extend_from_slice(&x.to_le_bytes());
            }
            o.extend_from_slice(&f32::MAX.to_le_bytes());
            o.extend_from_slice(&[4, *link, 8, *cover, 0]);
        }
        u32s(&mut o, self.links.len() as u32);
        for (k, n, t, e) in &self.links {
            u32s(&mut o, *k);
            u32s(&mut o, *n);
            o.extend_from_slice(&t.to_le_bytes());
            o.push(*e);
        }
        u32s(&mut o, self.doors.len() as u32);
        for (t, w, d) in &self.doors {
            o.extend_from_slice(&t.to_le_bytes());
            u32s(&mut o, *w);
            u32s(&mut o, *d);
        }
        u32s(&mut o, self.cover.len() as u32);
        for c in &self.cover {
            for x in c {
                o.extend_from_slice(&x.to_le_bytes());
            }
        }
        u32s(&mut o, self.cover_map.len() as u32);
        for c in &self.cover_map {
            for x in c {
                o.extend_from_slice(&x.to_le_bytes());
            }
        }
        u32s(&mut o, self.waypoints.len() as u32);
        for (p, t, f) in &self.waypoints {
            for c in p {
                o.extend_from_slice(&c.to_le_bytes());
            }
            o.extend_from_slice(&t.to_le_bytes());
            u32s(&mut o, *f);
        }
        u32s(&mut o, self.divisions);
        for f in [10.0f32, 10.0, 0.0, 0.0, 0.0, 10.0, 10.0, 0.0] {
            o.extend_from_slice(&f.to_le_bytes());
        }
        for s in &self.squares {
            u32s(&mut o, s.len() as u32);
            for t in s {
                o.extend_from_slice(&t.to_le_bytes());
            }
        }
        o
    }
}

/// A grid of `w` by `h` unit squares of side `s` starting at `x0`, two
/// triangles per square, neighbour words filled in; `holes` are squares
/// left out.
fn grid_mesh(w: usize, h: usize, s: f32, x0: f32, holes: &[(usize, usize)]) -> Spec {
    let mut spec = Spec {
        divisions: 1,
        ..Spec::default()
    };
    for r in 0..=h {
        for c in 0..=w {
            spec.vertices.push([x0 + c as f32 * s, r as f32 * s, 0.0]);
        }
    }
    let v = |r: usize, c: usize| (r * (w + 1) + c) as u16;
    // Triangle ids per square: (lower, upper), or None for a hole.
    let mut ids = vec![vec![None; w]; h];
    let mut n = 0u16;
    for (r, row) in ids.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            if !holes.contains(&(r, c)) {
                *slot = Some((n, n + 1));
                n += 2;
            }
        }
    }
    let at = |r: isize, c: isize| -> Option<(u16, u16)> {
        if r < 0 || c < 0 || r >= h as isize || c >= w as isize {
            return None;
        }
        ids[r as usize][c as usize]
    };
    for (r, row) in ids.iter().enumerate() {
        for (c, slot) in row.iter().enumerate() {
            let Some((lo, up)) = slot else { continue };
            let (ri, ci) = (r as isize, c as isize);
            // Lower: sw, se, ne. Edges: south, east, diagonal.
            let south = at(ri - 1, ci).map_or(-1, |(_, u)| u as i16);
            let east = at(ri, ci + 1).map_or(-1, |(_, u)| u as i16);
            spec.triangles.push((
                [v(r, c), v(r, c + 1), v(r + 1, c + 1)],
                [south, east, *up as i16],
                0,
                0,
            ));
            // Upper: sw, ne, nw. Edges: diagonal, north, west.
            let north = at(ri + 1, ci).map_or(-1, |(l, _)| l as i16);
            let west = at(ri, ci - 1).map_or(-1, |(l, _)| l as i16);
            spec.triangles.push((
                [v(r, c), v(r + 1, c + 1), v(r + 1, c)],
                [*lo as i16, north, west],
                0,
                0,
            ));
        }
    }
    spec.squares = vec![(0..n).collect()];
    spec
}

#[test]
fn a_full_mesh_reads_to_the_last_byte() {
    let mut s = grid_mesh(1, 1, 10.0, 0.0, &[]);
    s.world = 0x3c;
    s.parent = (3u32 << 16) | 0xfffe;
    s.links.push((0, 0x0100_0abc, 7, 2));
    s.triangles[0].1[0] = 0;
    s.triangles[0].2 = 1;
    s.doors.push((1, 0xe48b_73f3, 0x0001_2345));
    s.cover.push([0, 1, 0x910, 0x8000]);
    s.cover_map.push([1, 0]);
    s.triangles[0].3 = 2;
    s.waypoints.push(([1.0, 2.0, 3.0], 0, 0));
    let bytes = s.bytes();
    let m = Navmesh::read(&bytes, "t", 100, &budget()).unwrap();
    assert_eq!(m.parts.total(), bytes.len() as u64);
    assert_eq!(m.vertices.len(), 4);
    assert_eq!(m.triangles.len(), 2);
    assert_eq!(m.grid_cell(), Some([3, -2]));
    assert_eq!(m.edge(0, 0), Edge::Link(0));
    assert_eq!(m.edge(0, 2), Edge::Triangle(1));
    assert_eq!(m.edge(1, 1), Edge::Open);
    assert!(m.triangles[0].has_cover(1));
    assert_eq!(m.cover_map[0].triangle(), 0);
    assert_eq!(m.cover_map[0].edge(), 1);
    assert_eq!(m.edge_links[0].navmesh, 0x0100_0abc);
    assert_eq!(m.doors[0].door, 0x0001_2345);
    assert_eq!(m.waypoints[0].position, [1.0, 2.0, 3.0]);
    assert_eq!(m.grid.square(0), &[0, 1]);
    assert_eq!(m.grid.squares(), 1);
}

#[test]
fn an_interior_mesh_has_no_grid_cell() {
    let s = grid_mesh(1, 1, 10.0, 0.0, &[]);
    let m = Navmesh::read(&s.bytes(), "t", 0, &budget()).unwrap();
    assert_eq!(m.grid_cell(), None);
}

#[test]
fn malformed_meshes_are_named_errors() {
    let s = grid_mesh(1, 1, 10.0, 0.0, &[]);
    let good = s.bytes();
    let b = budget();
    let mut extra = good.clone();
    extra.push(0);
    assert_eq!(
        Navmesh::read(&extra, "t", 0, &b).unwrap_err().code,
        "NavTrailingBytes"
    );
    for cut in [3, 16, 30, good.len() - 1] {
        let e = Navmesh::read(&good[..cut], "t", 0, &b).unwrap_err();
        assert_eq!(e.code, "NavTruncated", "cut at {cut}");
    }
    let mut version = good.clone();
    version[0] = 12;
    assert_eq!(
        Navmesh::read(&version, "t", 0, &b).unwrap_err().code,
        "NavUnsupportedVersion"
    );
    let mut magic = good.clone();
    magic[4] ^= 1;
    assert_eq!(
        Navmesh::read(&magic, "t", 0, &b).unwrap_err().code,
        "NavMagic"
    );
    let mut index = s.clone();
    index.triangles[0].0[2] = 9;
    let e = Navmesh::read(&index.bytes(), "t", 0, &b).unwrap_err();
    assert_eq!(e.code, "NavVertexIndex");
    let mut huge = good.clone();
    huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        Navmesh::read(&huge, "t", 0, &b).unwrap_err().code,
        "NavCountTooLarge"
    );
    let mut divisions = s.clone();
    divisions.divisions = 5000;
    assert_eq!(
        Navmesh::read(&divisions.bytes(), "t", 0, &b)
            .unwrap_err()
            .code,
        "NavGridDivisions"
    );
    // A small budget refuses rather than allocating.
    let big = grid_mesh(20, 20, 1.0, 0.0, &[]).bytes();
    assert!(Navmesh::read(&big, "t", 0, &Budget::new(64)).is_err());
}

#[test]
fn an_invalid_edge_word_is_reported_not_followed() {
    let mut s = grid_mesh(1, 1, 10.0, 0.0, &[]);
    s.triangles[0].1[0] = 40;
    s.triangles[1].2 = 4; // link bit on edge 2 without links
    let m = Navmesh::read(&s.bytes(), "t", 0, &budget()).unwrap();
    assert_eq!(m.edge(0, 0), Edge::Invalid(40));
    assert_eq!(m.edge(1, 2), Edge::Invalid(-1));
    let g = NavGraph::build(vec![&m], |_, _| None, |_| true, &budget()).unwrap();
    assert_eq!(g.stats.invalid_edges, 2);
}

fn record(sig: &[u8; 4], subs: &[(&[u8; 4], Vec<u8>)]) -> Record {
    let mut payload = Vec::new();
    let mut subrecords = Vec::new();
    for (s, d) in subs {
        payload.extend_from_slice(*s);
        payload.extend_from_slice(&(d.len() as u16).to_le_bytes());
        let start = payload.len();
        payload.extend_from_slice(d);
        subrecords.push(Subrecord {
            signature: **s,
            data: start..payload.len(),
            offset: start as u64,
            extended: false,
        });
    }
    Record {
        signature: *sig,
        flags: 0,
        form_id: 1,
        revision: 0,
        version: 131,
        unknown: 0,
        source: SourceSpan::new("t", 0, payload.len() as u64 + 24),
        payload,
        subrecords,
        payload_is_decompressed: false,
    }
}

#[test]
fn a_navm_record_reads_its_companions_in_order() {
    let mesh = grid_mesh(1, 1, 10.0, 0.0, &[]).bytes();
    let mut mnam = Vec::new();
    mnam.extend_from_slice(&0x0001_0002u32.to_le_bytes());
    mnam.extend_from_slice(&2u16.to_le_bytes());
    mnam.extend_from_slice(&[0, 0, 1, 0]);
    let r = record(
        b"NAVM",
        &[
            (b"NVNM", mesh.clone()),
            (b"ONAM", vec![1, 2, 3, 0]),
            (b"NNAM", vec![0, 0, 3, 0]),
            (b"MNAM", mnam.clone()),
        ],
    );
    let n = NavmeshRecord::decode(&r, &budget()).unwrap();
    assert_eq!(n.onam, Some(0x0003_0201));
    assert_eq!(n.nnam, Some(vec![0, 3]));
    assert_eq!(n.mnam.len(), 1);
    assert_eq!(n.mnam[0].triangles, vec![0, 1]);
    assert_eq!(n.sizes, [mesh.len() as u64, 4, 4, 10]);
    let empty = record(b"NAVM", &[(b"NVNM", mesh.clone()), (b"MNAM", vec![])]);
    assert!(NavmeshRecord::decode(&empty, &budget())
        .unwrap()
        .mnam
        .is_empty());
    let swapped = record(
        b"NAVM",
        &[
            (b"NVNM", mesh.clone()),
            (b"MNAM", vec![]),
            (b"ONAM", vec![0; 4]),
        ],
    );
    assert_eq!(
        NavmeshRecord::decode(&swapped, &budget()).unwrap_err().code,
        "NavSubrecordOrder"
    );
    let no_mnam = record(b"NAVM", &[(b"NVNM", mesh.clone())]);
    assert_eq!(
        NavmeshRecord::decode(&no_mnam, &budget()).unwrap_err().code,
        "NavSubrecordOrder"
    );
    let cut = record(b"NAVM", &[(b"NVNM", mesh), (b"MNAM", mnam[..9].to_vec())]);
    assert_eq!(
        NavmeshRecord::decode(&cut, &budget()).unwrap_err().code,
        "NavTruncated"
    );
}

fn nvmi(flags: u32, island: bool) -> Vec<u8> {
    let mut o = Vec::new();
    let w = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    w(&mut o, 0x0001_0001);
    w(&mut o, flags);
    for f in [1.0f32, 2.0, 3.0, 0.0] {
        o.extend_from_slice(&f.to_le_bytes());
    }
    w(&mut o, 2);
    w(&mut o, 7);
    w(&mut o, 8);
    w(&mut o, 0);
    w(&mut o, 1);
    w(&mut o, 0xe48b_73f3);
    w(&mut o, 9);
    if island {
        o.push(1);
        for f in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0] {
            o.extend_from_slice(&f.to_le_bytes());
        }
        w(&mut o, 1);
        o.extend_from_slice(&[0, 0, 1, 0, 2, 0]);
        w(&mut o, 3);
        for f in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            o.extend_from_slice(&f.to_le_bytes());
        }
    } else {
        o.push(0);
    }
    w(&mut o, MAGIC);
    w(&mut o, 0x3c);
    w(&mut o, 0x0014_ffeb);
    o
}

#[test]
fn navigation_info_reads_entries_and_preferred_sets() {
    let b = budget();
    let plain = NavMeshInfo::read(&nvmi(0, false), "t", 0, &b).unwrap();
    assert_eq!(plain.first, vec![7, 8]);
    assert_eq!(plain.pairs, vec![(0xe48b_73f3, 9)]);
    assert!(plain.island.is_none());
    let island = NavMeshInfo::read(&nvmi(ISLAND_FLAG, true), "t", 0, &b).unwrap();
    assert_eq!(island.island.as_ref().unwrap().vertices.len(), 3);
    assert_eq!(
        NavMeshInfo::read(&nvmi(ISLAND_FLAG, false), "t", 0, &b)
            .unwrap_err()
            .code,
        "NavIslandFlag"
    );
    let mut nvpp = Vec::new();
    for v in [2u32, 1, 5, 2, 6, 7, 1, 5, 0] {
        nvpp.extend_from_slice(&v.to_le_bytes());
    }
    let r = record(
        b"NAVI",
        &[
            (b"NVER", 15u32.to_le_bytes().to_vec()),
            (b"NVMI", nvmi(0, false)),
            (b"NVMI", nvmi(ISLAND_FLAG, true)),
            (b"NVPP", nvpp),
        ],
    );
    let info = NavInfo::decode(&r, &b).unwrap();
    assert_eq!(info.meshes.len(), 2);
    assert_eq!(info.preferred.sets, vec![vec![5], vec![6, 7]]);
    assert_eq!(info.preferred.pairs, vec![(5, 0)]);
}

#[test]
fn a_straight_corridor_pulls_to_one_segment() {
    let s = grid_mesh(5, 1, 10.0, 0.0, &[]);
    let m = Navmesh::read(&s.bytes(), "t", 0, &budget()).unwrap();
    let g = NavGraph::build(vec![&m], |_, _| None, |_| true, &budget()).unwrap();
    let c = g
        .find_path([1.0, 5.0, 0.0], [49.0, 5.0, 0.0], &budget())
        .unwrap();
    assert_eq!(c.points.len(), 2);
    assert!((c.length - 48.0).abs() < 1e-9);
    assert_eq!(c.nodes.first().unwrap().triangle, 1);
    assert_eq!(c.portals.len(), c.nodes.len() - 1);
    assert!(c.crossings.iter().all(|x| *x == Crossing::Neighbour));
}

#[test]
fn a_hole_forces_a_detour_around_its_corner() {
    // 3 x 3 squares of 10 with the middle column's lower two squares
    // missing: the path from bottom-left to bottom-right goes over them.
    let s = grid_mesh(3, 3, 10.0, 0.0, &[(0, 1), (1, 1)]);
    let m = Navmesh::read(&s.bytes(), "t", 0, &budget()).unwrap();
    let g = NavGraph::build(vec![&m], |_, _| None, |_| true, &budget()).unwrap();
    let c = g
        .find_path([5.0, 5.0, 0.0], [25.0, 5.0, 0.0], &budget())
        .unwrap();
    // Corners at (10, 20) and (20, 20).
    assert_eq!(c.points.len(), 4, "{:?}", c.points);
    assert!((c.points[1][0] - 10.0).abs() < 1e-9 && (c.points[1][1] - 20.0).abs() < 1e-9);
    assert!((c.points[2][0] - 20.0).abs() < 1e-9 && (c.points[2][1] - 20.0).abs() < 1e-9);
    let expect = 2.0 * (5.0f64.powi(2) + 15.0f64.powi(2)).sqrt() + 10.0;
    assert!((c.length - expect).abs() < 1e-9, "{}", c.length);
    // Every corridor triangle is one the mesh has, and each step is a
    // neighbour of the previous.
    for w in c.nodes.windows(2) {
        let t = &m.triangles[usize::from(w[0].triangle)];
        assert!(t.edges.contains(&(w[1].triangle as i16)));
    }
}

#[test]
fn a_cut_mesh_has_two_components_and_no_path() {
    let s = grid_mesh(3, 1, 10.0, 0.0, &[(0, 1)]);
    let m = Navmesh::read(&s.bytes(), "t", 0, &budget()).unwrap();
    let g = NavGraph::build(vec![&m], |_, _| None, |_| true, &budget()).unwrap();
    let (labels, sizes) = g.components(&budget()).unwrap();
    assert_eq!(sizes, vec![2, 2]);
    assert_eq!(labels, vec![0, 0, 1, 1]);
    match g.find_path([5.0, 5.0, 0.0], [25.0, 5.0, 0.0], &budget()) {
        Err(PathError::NoPath { .. }) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(
        g.find_path([500.0, 5.0, 0.0], [25.0, 5.0, 0.0], &budget())
            .unwrap_err(),
        PathError::StartNotOnMesh
    );
}

#[test]
fn an_edge_link_joins_two_meshes() {
    // Mesh A covers x 0..10, mesh B x 10..20; A's lower triangle's east
    // edge links to B's upper triangle's west edge and back.
    let mut a = grid_mesh(1, 1, 10.0, 0.0, &[]);
    let mut b = grid_mesh(1, 1, 10.0, 10.0, &[]);
    a.links.push((0, 0xb, 1, 2));
    a.triangles[0].1[1] = 0;
    a.triangles[0].2 = 0b010;
    b.links.push((0, 0xa, 0, 1));
    b.triangles[1].1[2] = 0;
    b.triangles[1].2 = 0b100;
    let ma = Navmesh::read(&a.bytes(), "a", 0, &budget()).unwrap();
    let mb = Navmesh::read(&b.bytes(), "b", 0, &budget()).unwrap();
    let resolve = |_: usize, id: u32| match id {
        0xa => Some(0),
        0xb => Some(1),
        _ => None,
    };
    let g = NavGraph::build(vec![&ma, &mb], resolve, |k| k == 0, &budget()).unwrap();
    assert_eq!(g.stats.link_arcs, 2);
    let (_, sizes) = g.components(&budget()).unwrap();
    assert_eq!(sizes, vec![4]);
    let c = g
        .find_path([2.0, 8.0, 0.0], [18.0, 2.0, 0.0], &budget())
        .unwrap();
    assert!(c.crossings.contains(&Crossing::Link(0)));
    assert_eq!(c.nodes.first().unwrap().mesh, 0);
    assert_eq!(c.nodes.last().unwrap().mesh, 1);
    assert_eq!(c.points.len(), 2);
    // A kind the caller does not follow is left out.
    let other = NavGraph::build(vec![&ma, &mb], resolve, |k| k == 1, &budget()).unwrap();
    assert_eq!(other.stats.links_not_followed, 2);
    assert_eq!(other.stats.link_arcs, 0);
    // Without the resolver the meshes stay apart.
    let apart = NavGraph::build(vec![&ma, &mb], |_, _| None, |_| true, &budget()).unwrap();
    assert_eq!(apart.stats.links_unresolved, 2);
    assert_eq!(apart.components(&budget()).unwrap().1.len(), 2);
}

#[test]
fn the_funnel_keeps_a_straight_line_when_nothing_blocks_it() {
    let portals = [
        [[5.0, 10.0, 0.0], [5.0, -10.0, 0.0]],
        [[10.0, 10.0, 0.0], [10.0, -10.0, 0.0]],
    ];
    let p = funnel([0.0, 0.0, 0.0], [20.0, 0.0, 0.0], &portals);
    assert_eq!(p, vec![[0.0, 0.0, 0.0], [20.0, 0.0, 0.0]]);
    // A narrow portal to the side bends the line at its right end.
    let bent = [[[10.0, 10.0, 0.0], [10.0, 5.0, 0.0]]];
    let p = funnel([0.0, 0.0, 0.0], [20.0, 0.0, 0.0], &bent);
    assert_eq!(p.len(), 3);
    assert_eq!(p[1], [10.0, 5.0, 0.0]);
}

#[test]
fn every_rule_is_registered_pending_with_seven_columns() {
    let registry = include_str!("../../../spec/hypotheses.tsv");
    for rule in crate::rules::ALL {
        let row = registry
            .lines()
            .find(|l| l.split('\t').next() == Some(rule))
            .unwrap_or_else(|| panic!("{rule} not registered"));
        let cols: Vec<&str> = row.split('\t').collect();
        assert_eq!(cols.len(), 7, "{rule}");
        assert_eq!(cols[5], "pending", "{rule}");
    }
}
