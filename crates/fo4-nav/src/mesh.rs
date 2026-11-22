//! The `NVNM` geometry subrecord of a `NAVM` record, read to the last byte.
//!
//! Sections, in order (`nav_nvnm_layout_v1`), every count a `u32`:
//!
//! | Section | Element bytes |
//! | --- | ---: |
//! | header: version, magic, world, parent | 16 (once) |
//! | vertices: three `f32` | 12 |
//! | triangles: three `u16` vertices, three `i16` edge words, a word, five bytes | 21 |
//! | edge links: `u32` kind, `u32` navmesh FormID, `u16` triangle, `u8` edge | 11 |
//! | door links: `u16` triangle, `u32` word, `u32` door FormID | 10 |
//! | cover segments: `u16`, `u16`, `u16` data, `u16` flags | 8 |
//! | cover mappings: `u16` triangle times four plus edge, `u16` segment | 4 |
//! | waypoints: three `f32`, `u16` triangle, `u32` flags | 18 |
//! | grid: `u32` divisions, two `f32`, six `f32` bounds, then divisions squared lists of `u16` triangles | 36 + lists |
use crate::bytes::Bytes;
use crate::{reserve, MAGIC, VERSION};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Bytes of the fixed header.
pub const HEADER_BYTES: usize = 16;
pub const VERTEX_BYTES: usize = 12;
pub const TRIANGLE_BYTES: usize = 21;
pub const EDGE_LINK_BYTES: usize = 11;
pub const DOOR_LINK_BYTES: usize = 10;
pub const COVER_SEGMENT_BYTES: usize = 8;
pub const COVER_MAPPING_BYTES: usize = 4;
pub const WAYPOINT_BYTES: usize = 18;
/// Grid head: divisions, two sizes, six bounds.
pub const GRID_HEAD_BYTES: usize = 36;
/// Largest grid division count accepted (the census's largest is far below).
pub const MAX_GRID_DIVISIONS: u32 = 1024;

/// One triangle (`nav_triangle_v1`). The word at byte 12 and the five bytes
/// after it are kept as stored; only the bits named by
/// `nav_triangle_link_bits_v1` and `nav_triangle_cover_bits_v1` are read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Triangle {
    pub vertices: [u16; 3],
    /// Edge `i` runs from `vertices[i]` to `vertices[(i + 1) % 3]`. A
    /// triangle index, `-1` for none, or an edge-link index when
    /// [`Triangle::is_link`] says so.
    pub edges: [i16; 3],
    /// Bytes 12 to 15 as an `f32` (finite or the largest finite value);
    /// meaning not established.
    pub word: f32,
    /// Bytes 16 to 20 as stored.
    pub flags: [u8; 5],
}

/// What lies across one triangle edge, under the triangle rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Edge {
    /// No neighbour (word `-1` without the link bit).
    Open,
    /// A triangle of the same mesh.
    Triangle(u16),
    /// An entry of [`Navmesh::edge_links`].
    Link(u16),
    /// A word that is neither (counted by the census).
    Invalid(i16),
}

impl Triangle {
    /// Byte 17, bit `edge`: the edge word indexes the edge links
    /// (`nav_triangle_link_bits_v1`).
    pub fn is_link(&self, edge: usize) -> bool {
        edge < 3 && self.flags[1] >> edge & 1 == 1
    }
    /// Byte 19, bit `edge`: the edge has a cover mapping
    /// (`nav_triangle_cover_bits_v1`).
    pub fn has_cover(&self, edge: usize) -> bool {
        edge < 3 && self.flags[3] >> edge & 1 == 1
    }
    /// What lies across edge `edge` of a mesh with `triangles` triangles
    /// and `links` edge links.
    pub fn edge(&self, edge: usize, triangles: usize, links: usize) -> Edge {
        let w = self.edges[edge % 3];
        if self.is_link(edge) {
            return match usize::try_from(w) {
                Ok(i) if i < links => Edge::Link(w as u16),
                _ => Edge::Invalid(w),
            };
        }
        match w {
            -1 => Edge::Open,
            _ => match usize::try_from(w) {
                Ok(i) if i < triangles => Edge::Triangle(w as u16),
                _ => Edge::Invalid(w),
            },
        }
    }
    /// The two vertex indices of edge `edge`.
    pub fn edge_vertices(&self, edge: usize) -> [u16; 2] {
        [self.vertices[edge % 3], self.vertices[(edge + 1) % 3]]
    }
}

/// An edge link (`nav_edge_link_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EdgeLink {
    /// Word kept as stored (values 0, 1 and 2 measured).
    pub kind: u32,
    /// FormID of the linked `NAVM`, local to the plugin it was read from.
    pub navmesh: u32,
    /// Triangle index in the linked mesh.
    pub triangle: u16,
    /// Edge byte (0, 1 or 2 measured).
    pub edge: u8,
}

/// A door link (`nav_door_link_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DoorLink {
    pub triangle: u16,
    /// Word kept as stored.
    pub word: u32,
    /// FormID of a placed reference, local to the plugin.
    pub door: u32,
}

/// A cover segment (`nav_cover_segment_v1`), kept as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CoverSegment {
    /// The two point words; often vertex indices, not always.
    pub points: [u16; 2],
    pub data: u16,
    pub flags: u16,
}

/// A cover mapping (`nav_cover_map_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CoverMapping {
    /// Triangle times four plus edge, as stored.
    pub packed: u16,
    /// Index into [`Navmesh::cover`].
    pub segment: u16,
}
impl CoverMapping {
    pub fn triangle(&self) -> u16 {
        self.packed >> 2
    }
    pub fn edge(&self) -> u8 {
        (self.packed & 3) as u8
    }
}

/// A waypoint (`nav_waypoint_v1`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Waypoint {
    pub position: [f32; 3],
    pub triangle: u16,
    pub flags: u32,
}

/// The search grid (`nav_grid_v1`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Grid {
    pub divisions: u32,
    /// Two `f32` after the division count: candidate square sizes along x
    /// and y.
    pub size: [f32; 2],
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Start of each square's list in [`Grid::triangles`]; one entry per
    /// square plus the end.
    pub starts: Vec<u32>,
    pub triangles: Vec<u16>,
}
impl Grid {
    /// The stored triangle list of square `index`.
    pub fn square(&self, index: usize) -> &[u16] {
        match (self.starts.get(index), self.starts.get(index + 1)) {
            (Some(&a), Some(&b)) => &self.triangles[a as usize..b as usize],
            _ => &[],
        }
    }
    pub fn squares(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }
}

/// Bytes of the subrecord by section; they sum to its length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Parts {
    pub header: u64,
    pub vertices: u64,
    pub triangles: u64,
    pub edge_links: u64,
    pub doors: u64,
    pub cover: u64,
    pub cover_map: u64,
    pub waypoints: u64,
    pub grid: u64,
}
impl Parts {
    pub const NAMES: [&'static str; 9] = [
        "header",
        "vertices",
        "triangles",
        "edge links",
        "door links",
        "cover segments",
        "cover mappings",
        "waypoints",
        "grid",
    ];
    pub fn values(&self) -> [u64; 9] {
        [
            self.header,
            self.vertices,
            self.triangles,
            self.edge_links,
            self.doors,
            self.cover,
            self.cover_map,
            self.waypoints,
            self.grid,
        ]
    }
    pub fn total(&self) -> u64 {
        self.values().iter().sum()
    }
}

/// One `NVNM` subrecord.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Navmesh {
    pub version: u32,
    pub magic: u32,
    /// Header word 2: a world FormID, zero for an interior mesh
    /// (`nav_header_parent_v1`).
    pub world: u32,
    /// Header word 3: the parent cell FormID when `world` is zero, else the
    /// cell grid as two `i16` (see [`Navmesh::grid_cell`]).
    pub parent: u32,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<Triangle>,
    pub edge_links: Vec<EdgeLink>,
    pub doors: Vec<DoorLink>,
    pub cover: Vec<CoverSegment>,
    pub cover_map: Vec<CoverMapping>,
    pub waypoints: Vec<Waypoint>,
    pub grid: Grid,
    pub parts: Parts,
}

impl Navmesh {
    /// The header's cell grid `[x, y]` for an exterior mesh: the high half
    /// of word 3 is x and the low half y (`nav_header_parent_v1`).
    pub fn grid_cell(&self) -> Option<[i16; 2]> {
        (self.world != 0).then_some([(self.parent >> 16) as u16 as i16, self.parent as u16 as i16])
    }

    /// Reads one `NVNM` payload. `file` and `offset` (the payload's file
    /// offset, or its position in a decompressed record) are for errors.
    pub fn read(data: &[u8], file: &str, offset: u64, b: &Budget) -> Result<Self> {
        let mut r = Bytes::new(data, file, offset);
        let version = r.u32()?;
        if version != VERSION {
            return Err(Error::new(
                "NavUnsupportedVersion",
                file,
                offset,
                VERSION,
                version,
            ));
        }
        let magic = r.u32()?;
        if magic != MAGIC {
            return Err(Error::new(
                "NavMagic",
                file,
                offset + 4,
                format!("{MAGIC:#x}"),
                format!("{magic:#x}"),
            ));
        }
        let world = r.u32()?;
        let parent = r.u32()?;
        let mut parts = Parts {
            header: HEADER_BYTES as u64,
            ..Parts::default()
        };
        let mut mark = r.pos;
        let part = |r: &Bytes<'_>, mark: &mut usize| {
            let n = (r.pos - *mark) as u64;
            *mark = r.pos;
            n
        };

        let n = r.count(VERTEX_BYTES, "vertices")?;
        let mut vertices = Vec::new();
        reserve(&mut vertices, n, "vertices", file, r.at(), b)?;
        for _ in 0..n {
            vertices.push(r.vec3()?);
        }
        parts.vertices = part(&r, &mut mark);

        let n = r.count(TRIANGLE_BYTES, "triangles")?;
        let mut triangles = Vec::new();
        reserve(&mut triangles, n, "triangles", file, r.at(), b)?;
        for _ in 0..n {
            let at = r.at();
            let vertices_of = [r.u16()?, r.u16()?, r.u16()?];
            if let Some(v) = vertices_of
                .iter()
                .find(|v| usize::from(**v) >= vertices.len())
            {
                return Err(Error::new(
                    "NavVertexIndex",
                    file,
                    at,
                    format!("< {}", vertices.len()),
                    v,
                ));
            }
            triangles.push(Triangle {
                vertices: vertices_of,
                edges: [r.i16()?, r.i16()?, r.i16()?],
                word: r.f32()?,
                flags: r.array()?,
            });
        }
        parts.triangles = part(&r, &mut mark);

        let n = r.count(EDGE_LINK_BYTES, "edge links")?;
        let mut edge_links = Vec::new();
        reserve(&mut edge_links, n, "edge links", file, r.at(), b)?;
        for _ in 0..n {
            edge_links.push(EdgeLink {
                kind: r.u32()?,
                navmesh: r.u32()?,
                triangle: r.u16()?,
                edge: r.u8()?,
            });
        }
        parts.edge_links = part(&r, &mut mark);

        let n = r.count(DOOR_LINK_BYTES, "door links")?;
        let mut doors = Vec::new();
        reserve(&mut doors, n, "door links", file, r.at(), b)?;
        for _ in 0..n {
            doors.push(DoorLink {
                triangle: r.u16()?,
                word: r.u32()?,
                door: r.u32()?,
            });
        }
        parts.doors = part(&r, &mut mark);

        let n = r.count(COVER_SEGMENT_BYTES, "cover segments")?;
        let mut cover = Vec::new();
        reserve(&mut cover, n, "cover segments", file, r.at(), b)?;
        for _ in 0..n {
            cover.push(CoverSegment {
                points: [r.u16()?, r.u16()?],
                data: r.u16()?,
                flags: r.u16()?,
            });
        }
        parts.cover = part(&r, &mut mark);

        let n = r.count(COVER_MAPPING_BYTES, "cover mappings")?;
        let mut cover_map = Vec::new();
        reserve(&mut cover_map, n, "cover mappings", file, r.at(), b)?;
        for _ in 0..n {
            cover_map.push(CoverMapping {
                packed: r.u16()?,
                segment: r.u16()?,
            });
        }
        parts.cover_map = part(&r, &mut mark);

        let n = r.count(WAYPOINT_BYTES, "waypoints")?;
        let mut waypoints = Vec::new();
        reserve(&mut waypoints, n, "waypoints", file, r.at(), b)?;
        for _ in 0..n {
            waypoints.push(Waypoint {
                position: r.vec3()?,
                triangle: r.u16()?,
                flags: r.u32()?,
            });
        }
        parts.waypoints = part(&r, &mut mark);

        let at = r.at();
        let divisions = r.u32()?;
        if divisions > MAX_GRID_DIVISIONS {
            return Err(Error::new(
                "NavGridDivisions",
                file,
                at,
                format!("<= {MAX_GRID_DIVISIONS}"),
                divisions,
            ));
        }
        let size = [r.f32()?, r.f32()?];
        let min = r.vec3()?;
        let max = r.vec3()?;
        let squares = divisions * divisions;
        let mut starts = Vec::new();
        reserve(&mut starts, squares + 1, "grid squares", file, r.at(), b)?;
        let mut grid_triangles: Vec<u16> = Vec::new();
        starts.push(0);
        for _ in 0..squares {
            let n = r.count(2, "grid square")?;
            reserve(&mut grid_triangles, n, "grid triangles", file, r.at(), b)?;
            for _ in 0..n {
                grid_triangles.push(r.u16()?);
            }
            // At most MAX_COUNT^2 in theory, but the bytes bound it far lower.
            starts.push(
                u32::try_from(grid_triangles.len())
                    .map_err(|_| r.error("NavCountTooLarge", "grid triangles <= u32", "more"))?,
            );
        }
        parts.grid = part(&r, &mut mark);
        r.finish("NVNM")?;
        Ok(Self {
            version,
            magic,
            world,
            parent,
            vertices,
            triangles,
            edge_links,
            doors,
            cover,
            cover_map,
            waypoints,
            grid: Grid {
                divisions,
                size,
                min,
                max,
                starts,
                triangles: grid_triangles,
            },
            parts,
        })
    }

    /// What lies across edge `edge` of triangle `triangle`.
    pub fn edge(&self, triangle: usize, edge: usize) -> Edge {
        match self.triangles.get(triangle) {
            Some(t) => t.edge(edge, self.triangles.len(), self.edge_links.len()),
            None => Edge::Invalid(-1),
        }
    }

    /// The three corners of a triangle.
    pub fn corners(&self, triangle: usize) -> Option<[[f32; 3]; 3]> {
        let t = self.triangles.get(triangle)?;
        Some(t.vertices.map(|v| self.vertices[usize::from(v)]))
    }

    /// Centroid of a triangle.
    pub fn centroid(&self, triangle: usize) -> Option<[f64; 3]> {
        let c = self.corners(triangle)?;
        let mut out = [0.0; 3];
        for p in c {
            for (o, v) in out.iter_mut().zip(p) {
                *o += f64::from(v) / 3.0;
            }
        }
        Some(out)
    }

    /// The triangle whose top-down footprint contains `(x, y)`, choosing
    /// the one whose plane is closest to `z` when several do.
    pub fn locate(&self, p: [f64; 3]) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, _) in self.triangles.iter().enumerate() {
            let Some(c) = self.corners(i) else { continue };
            let Some(z) = crate::path::height_in(&c, p[0], p[1]) else {
                continue;
            };
            let d = (z - p[2]).abs();
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }
}
