//! A whole `NAVM` record: `NVNM`, then optionally `ONAM` and `NNAM`, then
//! `MNAM`, in that order (`nav_nvnm_layout_v1`,
//! `nav_record_companions_v1`). Any other subrecord or order is refused.
use crate::bytes::Bytes;
use crate::mesh::Navmesh;
use crate::reserve;
use fo4_core::{Budget, Error, Result};
use fo4_plugin::Record;
use serde::Serialize;

/// One `MNAM` group: a FormID and a list of triangle indices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MnamGroup {
    /// FormID local to the plugin.
    pub form: u32,
    pub triangles: Vec<u16>,
}

/// A decoded `NAVM` record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NavmeshRecord {
    pub mesh: Navmesh,
    /// `ONAM`: one FormID local to the plugin.
    pub onam: Option<u32>,
    /// `NNAM`: a list of `u16` (vertex indices, ascending, in every measured
    /// record).
    pub nnam: Option<Vec<u16>>,
    /// `MNAM` groups; empty when `MNAM` is empty.
    pub mnam: Vec<MnamGroup>,
    /// Bytes of each subrecord payload: NVNM, ONAM, NNAM, MNAM.
    pub sizes: [u64; 4],
}

impl NavmeshRecord {
    /// Decodes a `NAVM` record that is not deleted.
    pub fn decode(record: &Record, b: &Budget) -> Result<Self> {
        let file = record.source.file.as_str();
        if record.signature != *b"NAVM" {
            return Err(Error::new(
                "RecordTypeMismatch",
                file,
                record.source.offset,
                "NAVM",
                String::from_utf8_lossy(&record.signature),
            ));
        }
        // Subrecord offsets are file offsets, or payload positions in a
        // decompressed record; errors carry them as they are.
        let mut subs = record.subrecords.iter().peekable();
        let at = |s: &fo4_plugin::Subrecord| s.offset;
        let Some(first) = subs.next().filter(|s| s.signature == *b"NVNM") else {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                record.source.offset,
                "NVNM first",
                record.subrecords.first().map_or("none".into(), |s| {
                    String::from_utf8_lossy(&s.signature).into_owned()
                }),
            ));
        };
        let mesh = Navmesh::read(record.data(first), file, at(first), b)?;
        let mut sizes = [record.data(first).len() as u64, 0, 0, 0];
        let mut onam = None;
        let mut nnam = None;
        let mut mnam = None;
        if let Some(s) = subs.next_if(|s| s.signature == *b"ONAM") {
            let d = record.data(s);
            sizes[1] = d.len() as u64;
            let mut r = Bytes::new(d, file, at(s));
            onam = Some(r.u32()?);
            r.finish("ONAM")?;
        }
        if let Some(s) = subs.next_if(|s| s.signature == *b"NNAM") {
            let d = record.data(s);
            sizes[2] = d.len() as u64;
            let mut r = Bytes::new(d, file, at(s));
            if !d.len().is_multiple_of(2) {
                return Err(r.error("NavNnamSize", "even length", d.len()));
            }
            let mut v = Vec::new();
            reserve(&mut v, (d.len() / 2) as u32, "NNAM", file, at(s), b)?;
            while r.remaining() > 0 {
                v.push(r.u16()?);
            }
            nnam = Some(v);
        }
        if let Some(s) = subs.next_if(|s| s.signature == *b"MNAM") {
            let d = record.data(s);
            sizes[3] = d.len() as u64;
            let mut r = Bytes::new(d, file, at(s));
            let mut groups = Vec::new();
            while r.remaining() > 0 {
                let form = r.u32()?;
                let at_count = r.at();
                let n = r.u16()?;
                if usize::from(n) * 2 > r.remaining() {
                    return Err(Error::new(
                        "NavTruncated",
                        file,
                        at_count,
                        format!("MNAM group of {n} triangles"),
                        format!("{} bytes left", r.remaining()),
                    ));
                }
                let mut triangles = Vec::new();
                reserve(&mut triangles, u32::from(n), "MNAM", file, at_count, b)?;
                for _ in 0..n {
                    triangles.push(r.u16()?);
                }
                b.reserve(&mut groups, 1, file, at_count)?;
                groups.push(MnamGroup { form, triangles });
            }
            mnam = Some(groups);
        }
        if let Some(s) = subs.next() {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                at(s),
                "NVNM [ONAM] [NNAM] MNAM and nothing after",
                String::from_utf8_lossy(&s.signature),
            ));
        }
        let Some(mnam) = mnam else {
            return Err(Error::new(
                "NavSubrecordOrder",
                file,
                record.source.offset,
                "an MNAM subrecord",
                "none",
            ));
        };
        Ok(Self {
            mesh,
            onam,
            nnam,
            mnam,
            sizes,
        })
    }
}
