//! The `NAVI` navigation-info record: `NVER`, then one `NVMI` per navmesh,
//! then `NVPP` (`navi_nvmi_layout_v1`, `navi_nvpp_layout_v1`).
//!
//! `NVMI`, in order: `u32` navmesh FormID, `u32` flags, three `f32`, one
//! `f32`, a counted list of `u32` FormIDs, a second counted list of `u32`
//! FormIDs, a counted list of pairs (`u32` word, `u32` FormID), a byte that
//! is 1 exactly when flags bit 5 is set and then carries the island data
//! (two corners, counted `u16` triangles, counted vertices), and last the
//! magic word, a world FormID and the parent word as in `NVNM`.
//!
//! `NVPP`: a counted list of sets, each a counted list of `u32` FormIDs,
//! then a counted list of pairs (`u32` FormID, `u32` index).
use crate::bytes::Bytes;
use crate::{reserve, MAGIC, VERSION};
use fo4_core::{Budget, Error, Result};
use fo4_plugin::Record;
use serde::Serialize;

/// Flags bit that announces island data.
pub const ISLAND_FLAG: u32 = 0x20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IslandData {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub triangles: Vec<[u16; 3]>,
    pub vertices: Vec<[f32; 3]>,
}

/// One `NVMI` entry. FormIDs are local to the plugin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NavMeshInfo {
    pub navmesh: u32,
    pub flags: u32,
    pub position: [f32; 3],
    pub word: f32,
    pub first: Vec<u32>,
    pub second: Vec<u32>,
    pub pairs: Vec<(u32, u32)>,
    pub island: Option<IslandData>,
    pub magic: u32,
    pub world: u32,
    pub parent: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreferredPathing {
    pub sets: Vec<Vec<u32>>,
    pub pairs: Vec<(u32, u32)>,
}

/// A decoded `NAVI` record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NavInfo {
    pub version: u32,
    pub meshes: Vec<NavMeshInfo>,
    pub preferred: PreferredPathing,
}

fn u32_list(r: &mut Bytes<'_>, what: &str, b: &Budget) -> Result<Vec<u32>> {
    let n = r.count(4, what)?;
    let mut v = Vec::new();
    reserve(&mut v, n, what, r.file, r.at(), b)?;
    for _ in 0..n {
        v.push(r.u32()?);
    }
    Ok(v)
}

fn pair_list(r: &mut Bytes<'_>, what: &str, b: &Budget) -> Result<Vec<(u32, u32)>> {
    let n = r.count(8, what)?;
    let mut v = Vec::new();
    reserve(&mut v, n, what, r.file, r.at(), b)?;
    for _ in 0..n {
        v.push((r.u32()?, r.u32()?));
    }
    Ok(v)
}

impl NavMeshInfo {
    pub fn read(data: &[u8], file: &str, offset: u64, b: &Budget) -> Result<Self> {
        let mut r = Bytes::new(data, file, offset);
        let navmesh = r.u32()?;
        let flags = r.u32()?;
        let position = r.vec3()?;
        let word = r.f32()?;
        let first = u32_list(&mut r, "NVMI first list", b)?;
        let second = u32_list(&mut r, "NVMI second list", b)?;
        let pairs = pair_list(&mut r, "NVMI pairs", b)?;
        let at = r.at();
        let island = match r.u8()? {
            0 => None,
            1 => {
                let min = r.vec3()?;
                let max = r.vec3()?;
                let n = r.count(6, "island triangles")?;
                let mut triangles = Vec::new();
                reserve(&mut triangles, n, "island triangles", file, r.at(), b)?;
                for _ in 0..n {
                    triangles.push([r.u16()?, r.u16()?, r.u16()?]);
                }
                let n = r.count(12, "island vertices")?;
                let mut vertices = Vec::new();
                reserve(&mut vertices, n, "island vertices", file, r.at(), b)?;
                for _ in 0..n {
                    vertices.push(r.vec3()?);
                }
                Some(IslandData {
                    min,
                    max,
                    triangles,
                    vertices,
                })
            }
            x => return Err(Error::new("NavIslandByte", file, at, "0 or 1", x)),
        };
        if island.is_some() != (flags & ISLAND_FLAG != 0) {
            return Err(Error::new(
                "NavIslandFlag",
                file,
                at,
                "island byte equal to flags bit 5",
                format!("flags {flags:#x}"),
            ));
        }
        let at = r.at();
        let magic = r.u32()?;
        if magic != MAGIC {
            return Err(Error::new(
                "NavMagic",
                file,
                at,
                format!("{MAGIC:#x}"),
                format!("{magic:#x}"),
            ));
        }
        let world = r.u32()?;
        let parent = r.u32()?;
        r.finish("NVMI")?;
        Ok(Self {
            navmesh,
            flags,
            position,
            word,
            first,
            second,
            pairs,
            island,
            magic,
            world,
            parent,
        })
    }
}

impl PreferredPathing {
    pub fn read(data: &[u8], file: &str, offset: u64, b: &Budget) -> Result<Self> {
        let mut r = Bytes::new(data, file, offset);
        let n = r.count(4, "NVPP sets")?;
        let mut sets = Vec::new();
        reserve(&mut sets, n, "NVPP sets", file, r.at(), b)?;
        for _ in 0..n {
            sets.push(u32_list(&mut r, "NVPP set", b)?);
        }
        let pairs = pair_list(&mut r, "NVPP pairs", b)?;
        r.finish("NVPP")?;
        Ok(Self { sets, pairs })
    }
}

impl NavInfo {
    /// Decodes a `NAVI` record: `NVER`, any number of `NVMI`, one `NVPP`.
    pub fn decode(record: &Record, b: &Budget) -> Result<Self> {
        let file = record.source.file.as_str();
        if record.signature != *b"NAVI" {
            return Err(Error::new(
                "RecordTypeMismatch",
                file,
                record.source.offset,
                "NAVI",
                String::from_utf8_lossy(&record.signature),
            ));
        }
        let mut subs = record.subrecords.iter().peekable();
        let Some(s) = subs.next().filter(|s| s.signature == *b"NVER") else {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                record.source.offset,
                "NVER first",
                "other",
            ));
        };
        let mut r = Bytes::new(record.data(s), file, s.offset);
        let version = r.u32()?;
        r.finish("NVER")?;
        if version != VERSION {
            return Err(Error::new(
                "NavUnsupportedVersion",
                file,
                s.offset,
                VERSION,
                version,
            ));
        }
        let mut meshes = Vec::new();
        while let Some(s) = subs.next_if(|s| s.signature == *b"NVMI") {
            b.reserve(&mut meshes, 1, file, s.offset)?;
            meshes.push(NavMeshInfo::read(record.data(s), file, s.offset, b)?);
        }
        let Some(s) = subs.next().filter(|s| s.signature == *b"NVPP") else {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                record.source.offset,
                "NVPP after the NVMI entries",
                "other or none",
            ));
        };
        let preferred = PreferredPathing::read(record.data(s), file, s.offset, b)?;
        if let Some(s) = subs.next() {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                s.offset,
                "nothing after NVPP",
                String::from_utf8_lossy(&s.signature),
            ));
        }
        Ok(Self {
            version,
            meshes,
            preferred,
        })
    }
}
