#![forbid(unsafe_code)]
//! Bounded reader for the navigation meshes stored in Fallout 4 plugins
//! (`NAVM` records and their `NVNM` geometry, the `NAVI` navigation-info
//! record) and a path search over them.
//!
//! Every layout this crate reads was measured over every navmesh record of
//! one installation; `spec/navmesh.md` gives each field with its count and
//! `spec/navmesh-census.txt` holds the census aggregates. Some parts of the
//! layout (the order of the sections, the record and subrecord names, what a
//! few fields mean) were recognised from prior general knowledge before they
//! were measured; they are listed in `docs/provenance.md` and kept only where
//! the census, which could have refuted them, holds. No editor, tool, wiki
//! page, other implementation, decompilation or game executable was consulted.
//!
//! A subrecord is read front to back and must end exactly where its last
//! section ends; every byte belongs to a framed section. Words whose meaning
//! is not established are kept as stored (`flags`, `word` fields) and only
//! counted. FormIDs are kept as stored in the plugin (local to that plugin's
//! master list); resolving them is the caller's job through
//! `fo4_plugin::LoadOrder::remap`. The meaning given to a field is a pending
//! rule in `spec/hypotheses.tsv` ([`rules`]), never a fact. Input outside the
//! measured profile is a named error with the subrecord offset; nothing here
//! panics on file data.
//!
//! The path search ([`path`]) is this project's own algorithm (A* over
//! triangles, then a funnel over the corridor). It is not claimed to match
//! how the game's actors choose paths, which nothing here measures.
use fo4_core::{Budget, Error, Result};

mod bytes;
pub mod info;
pub mod mesh;
pub mod path;
pub mod record;

pub use info::{IslandData, NavInfo, NavMeshInfo, PreferredPathing};
pub use mesh::{
    CoverMapping, CoverSegment, DoorLink, Edge, EdgeLink, Grid, Navmesh, Parts, Triangle, Waypoint,
};
pub use path::{Corridor, NavGraph, NodeId, PathError};
pub use record::{MnamGroup, NavmeshRecord};

/// Names of the pending rules this crate relies on (`spec/hypotheses.tsv`).
pub mod rules {
    /// The section sequence of `NVNM` and the size of each element.
    pub const NVNM_LAYOUT: &str = "nav_nvnm_layout_v1";
    /// Header words 2 and 3: world FormID (zero inside) and the parent cell
    /// FormID or the cell grid.
    pub const HEADER_PARENT: &str = "nav_header_parent_v1";
    /// Triangle vertices and neighbour words, vertices in world units.
    pub const TRIANGLE: &str = "nav_triangle_v1";
    /// Triangle byte 17 bits 0 to 2: that edge's word indexes the edge links.
    pub const TRIANGLE_LINK_BITS: &str = "nav_triangle_link_bits_v1";
    /// Triangle byte 19 bits 0 to 2: that edge has a cover mapping.
    pub const TRIANGLE_COVER_BITS: &str = "nav_triangle_cover_bits_v1";
    /// Edge link: kind word, navmesh FormID, triangle there, edge byte.
    pub const EDGE_LINK: &str = "nav_edge_link_v1";
    /// Door link: triangle, a word kept raw, placed door FormID.
    pub const DOOR_LINK: &str = "nav_door_link_v1";
    /// Cover segment: two point words, a data word and a flag word whose
    /// bit 15 starts a chain.
    pub const COVER_SEGMENT: &str = "nav_cover_segment_v1";
    /// Cover mapping: triangle times four plus edge, cover segment index.
    pub const COVER_MAP: &str = "nav_cover_map_v1";
    /// Waypoint: position, triangle, flags.
    pub const WAYPOINT: &str = "nav_waypoint_v1";
    /// Search grid: divisions, square size, bounds and the triangles of
    /// every square.
    pub const GRID: &str = "nav_grid_v1";
    /// `NAVM` companions: `ONAM` FormID, `NNAM` vertex list, `MNAM` groups.
    pub const RECORD_COMPANIONS: &str = "nav_record_companions_v1";
    /// `NAVI` `NVMI` per-navmesh entries.
    pub const NVMI_LAYOUT: &str = "navi_nvmi_layout_v1";
    /// `NAVI` `NVPP` preferred pathing sets.
    pub const NVPP_LAYOUT: &str = "navi_nvpp_layout_v1";
    /// Every `NAVI` occurrence's `NVMI` entries apply together (a census
    /// finding; nothing in this crate merges them).
    pub const NAVI_COMBINED: &str = "navi_occurrences_combined_v1";
    /// The path search is this project's own and claims nothing about the
    /// game's.
    pub const PATH_SEARCH: &str = "nav_path_search_v1";
    pub const ALL: [&str; 16] = [
        NVNM_LAYOUT,
        HEADER_PARENT,
        TRIANGLE,
        TRIANGLE_LINK_BITS,
        TRIANGLE_COVER_BITS,
        EDGE_LINK,
        DOOR_LINK,
        COVER_SEGMENT,
        COVER_MAP,
        WAYPOINT,
        GRID,
        RECORD_COMPANIONS,
        NVMI_LAYOUT,
        NVPP_LAYOUT,
        NAVI_COMBINED,
        PATH_SEARCH,
    ];
}

/// Version word of every measured `NVNM` and `NVMI` block and of `NVER`.
pub const VERSION: u32 = 15;
/// Word 1 of every measured `NVNM` and the word before the parent in every
/// measured `NVMI` (kept, compared, not interpreted).
pub const MAGIC: u32 = 0xA5E9_A03C;
/// Largest element count accepted for one section before its bytes are
/// checked against the subrecord.
pub const MAX_COUNT: u32 = 1 << 20;

fn count_error(file: &str, offset: u64, what: &str, n: u64) -> Error {
    Error::new(
        "NavCountTooLarge",
        file,
        offset,
        format!("{what} count <= {MAX_COUNT}"),
        n,
    )
}

/// Reserve `n` elements after checking the count against [`MAX_COUNT`].
fn reserve<T>(
    v: &mut Vec<T>,
    n: u32,
    what: &str,
    file: &str,
    offset: u64,
    b: &Budget,
) -> Result<()> {
    if n > MAX_COUNT {
        return Err(count_error(file, offset, what, u64::from(n)));
    }
    b.reserve(v, n as usize, file, offset)
}

#[cfg(test)]
mod tests;
