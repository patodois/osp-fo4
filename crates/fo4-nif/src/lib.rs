#![forbid(unsafe_code)]
//! Independent NIF 20.2.0.7 / user 12 / Bethesda 100 and 130 reader.
mod blocks;
mod collision;
mod geometry;
pub mod skin;
pub use blocks::{Data, Field, Value};
pub use collision::{Collision, CollisionObject, Owner, System, SystemKind};
use fo4_core::{add, limit, mul, range, Budget, Cursor, Error, Game, Limits, Result, SliceSource};
pub use geometry::{Mesh, Vertex};
use serde::Serialize;
use std::ops::Range;
#[derive(Debug, Serialize)]
pub struct Block {
    pub type_index: u16,
    pub data: Range<usize>,
    pub decoded: Option<Data>,
    pub raw_tail: Range<usize>,
    pub diagnostics: Vec<Error>,
}
#[derive(Debug, Serialize)]
pub struct Nif<'a> {
    pub version: u32,
    pub user_version: u32,
    pub bs_version: u32,
    pub exports: Vec<Vec<u8>>,
    pub block_types: Vec<String>,
    pub strings: Vec<Vec<u8>>,
    pub max_string_length: u32,
    pub groups: Vec<u32>,
    pub roots: Vec<i32>,
    pub blocks: Vec<Block>,
    #[serde(skip)]
    pub bytes: &'a [u8],
}
impl<'a> Nif<'a> {
    pub fn read(bytes: &'a [u8], file: &str, game: Game, l: &Limits, b: &Budget) -> Result<Self> {
        l.validate(file)?;
        limit(bytes.len() as u64, l.max_payload, file, 0)?;
        let src = SliceSource { name: file, bytes };
        let mut c = Cursor::new(&src);
        for byte in b"Gamebryo File Format, Version 20.2.0.7\n" {
            let o = c.pos;
            let found = c.u8()?;
            if found != *byte {
                return Err(Error::new("UnsupportedNifHeader", file, o, byte, found));
            }
        }
        let version = c.u32()?;
        if version != 0x14020007 {
            return Err(Error::new(
                "UnsupportedNifVersion",
                file,
                c.pos - 4,
                "20.2.0.7",
                version,
            ));
        }
        let endian = c.u8()?;
        if endian != 1 {
            return Err(Error::new(
                "UnsupportedNifEndian",
                file,
                c.pos - 1,
                "little endian (1)",
                endian,
            ));
        }
        let user_version = c.u32()?;
        if user_version != 12 {
            return Err(Error::new(
                "UnsupportedNifUserVersion",
                file,
                c.pos - 4,
                12,
                user_version,
            ));
        }
        let count = c.u32()? as u64;
        limit(count, l.max_entries, file, c.pos - 4)?;
        let bs_version = c.u32()?;
        let expected = match game {
            Game::Fallout4 => "130",
            Game::SkyrimSe => "83 or 100",
        };
        let supported = match game {
            Game::Fallout4 => bs_version == 130,
            Game::SkyrimSe => matches!(bs_version, 83 | 100),
        };
        if !supported {
            return Err(Error::new(
                "UnsupportedNifBsVersion",
                file,
                c.pos - 4,
                expected,
                bs_version,
            ));
        }
        let mut exports = Vec::new();
        b.reserve(&mut exports, 4, file, c.pos)?;
        for _ in 0..if bs_version == 130 { 4 } else { 3 } {
            let n = c.u8()? as u64;
            limit(n, l.max_string, file, c.pos - 1)?;
            exports.push(c.bytes(n, l, b)?);
        }
        let types = c.u16()? as u64;
        limit(types, l.max_entries, file, c.pos - 2)?;
        range(c.pos, mul(types, 4, file, c.pos)?, c.end, file)?;
        let mut block_types: Vec<String> = Vec::new();
        b.reserve(&mut block_types, types as usize, file, c.pos)?;
        for _ in 0..types {
            let raw = sized(&mut c, l, b)?;
            let text = std::str::from_utf8(&raw).map_err(|e| {
                Error::new(
                    "InvalidNifTypeName",
                    file,
                    c.pos - raw.len() as u64,
                    "UTF-8 type name",
                    e,
                )
            })?;
            if text.is_empty() {
                return Err(Error::new("InvalidNifTypeName", file, c.pos, "nonempty", 0));
            }
            block_types.push(
                String::from_utf8(raw)
                    .map_err(|e| Error::new("InvalidNifTypeName", file, c.pos, "UTF-8", e))?,
            );
        }
        range(c.pos, mul(count, 6, file, c.pos)?, c.end, file)?;
        let mut indices = Vec::new();
        b.reserve(&mut indices, count as usize, file, c.pos)?;
        for _ in 0..count {
            let o = c.pos;
            let index = c.u16()?;
            if index as u64 >= types {
                return Err(Error::new(
                    "InvalidNifTypeIndex",
                    file,
                    o,
                    format!("< {types}"),
                    index,
                ));
            }
            indices.push(index);
        }
        let mut sizes = Vec::new();
        b.reserve(&mut sizes, count as usize, file, c.pos)?;
        for _ in 0..count {
            let o = c.pos;
            let size = c.u32()?;
            limit(size as u64, l.max_payload, file, o)?;
            sizes.push(size);
        }
        let num_strings = c.u32()? as u64;
        limit(num_strings, l.max_entries, file, c.pos - 4)?;
        let max_string_length = c.u32()?;
        limit(max_string_length as u64, l.max_string, file, c.pos - 4)?;
        range(c.pos, mul(num_strings, 4, file, c.pos)?, c.end, file)?;
        let mut strings = Vec::new();
        b.reserve(&mut strings, num_strings as usize, file, c.pos)?;
        for _ in 0..num_strings {
            let raw = sized(&mut c, l, b)?;
            limit(
                raw.len() as u64,
                max_string_length as u64,
                file,
                c.pos - raw.len() as u64,
            )?;
            strings.push(raw);
        }
        let num_groups = c.u32()? as u64;
        limit(num_groups, l.max_entries, file, c.pos - 4)?;
        range(c.pos, mul(num_groups, 4, file, c.pos)?, c.end, file)?;
        let mut groups = Vec::new();
        b.reserve(&mut groups, num_groups as usize, file, c.pos)?;
        for _ in 0..num_groups {
            groups.push(c.u32()?);
        }
        let mut blocks = Vec::new();
        b.reserve(&mut blocks, count as usize, file, c.pos)?;
        for (index, size) in indices.into_iter().zip(sizes) {
            let start = c.pos;
            range(start, size as u64, c.end, file)?;
            c.pos = add(start, size as u64, file, start)?;
            blocks.push(Block {
                type_index: index,
                data: start as usize..c.pos as usize,
                decoded: None,
                raw_tail: start as usize..c.pos as usize,
                diagnostics: Vec::new(),
            });
        }
        let roots_n = c.u32()? as u64;
        limit(roots_n, l.max_entries, file, c.pos - 4)?;
        range(c.pos, mul(roots_n, 4, file, c.pos)?, c.end, file)?;
        let mut roots = Vec::new();
        b.reserve(&mut roots, roots_n as usize, file, c.pos)?;
        for _ in 0..roots_n {
            roots.push(reference(&mut c, count as usize)?);
        }
        if c.pos != c.end {
            return Err(Error::new(
                "TrailingNifData",
                file,
                c.pos,
                "footer ends at EOF",
                c.end - c.pos,
            ));
        }
        for block in &mut blocks {
            let name = &block_types[block.type_index as usize];
            let mut bc = Cursor::at(&src, block.data.start as u64, block.data.end as u64)?;
            match blocks::read(name, &mut bc, bs_version, &strings, count as usize, l, b) {
                Ok(Some(data)) => {
                    block.decoded = Some(data);
                    block.raw_tail = bc.pos as usize..block.data.end;
                }
                Ok(None) => {}
                Err(mut e) => {
                    e.source.version = Some(bs_version);
                    b.reserve(&mut block.diagnostics, 1, file, e.source.offset)?;
                    let mut retained = 512;
                    for capacity in [
                        e.source.file.capacity(),
                        e.code.capacity(),
                        e.expected.capacity(),
                        e.found.capacity(),
                        e.source.archive_entry.as_ref().map_or(0, String::capacity),
                        e.source.sha256.as_ref().map_or(0, String::capacity),
                    ] {
                        retained = add(retained, capacity as u64, file, e.source.offset)?;
                    }
                    b.charge(retained, file, e.source.offset)?;
                    block.diagnostics.push(e);
                }
            }
        }
        let nif = Self {
            version,
            user_version,
            bs_version,
            exports,
            block_types,
            strings,
            max_string_length,
            groups,
            roots,
            blocks,
            bytes,
        };
        nif.check_scene(file, l, b)?;
        Ok(nif)
    }
    pub fn raw_block(&self, index: usize) -> Option<&'a [u8]> {
        self.blocks
            .get(index)
            .map(|block| &self.bytes[block.data.clone()])
    }
    fn check_scene(&self, file: &str, l: &Limits, b: &Budget) -> Result<()> {
        let mut colors = b.bytes(self.blocks.len() as u64, file, 0)?;
        let mut depths: Vec<u64> = Vec::new();
        b.reserve(&mut depths, self.blocks.len(), file, 0)?;
        depths.resize(self.blocks.len(), 0);
        let mut stack = Vec::new();
        b.reserve(
            &mut stack,
            self.blocks.len().min(l.max_depth as usize + 1),
            file,
            0,
        )?;
        for root in 0..self.blocks.len() {
            if colors[root] != 0 {
                continue;
            }
            stack.push((root, 0usize));
            colors[root] = 1;
            while let Some((node, next)) = stack.last_mut() {
                let children = match self.blocks[*node].decoded.as_ref() {
                    Some(Data::Node { children, .. }) => children.as_slice(),
                    _ => &[],
                };
                if *next == children.len() {
                    let mut height = 1;
                    for child in children {
                        if *child >= 0 {
                            height = height.max(add(
                                depths[*child as usize],
                                1,
                                file,
                                self.blocks[*node].data.start as u64,
                            )?);
                        }
                    }
                    limit(
                        height,
                        l.max_depth,
                        file,
                        self.blocks[*node].data.start as u64,
                    )?;
                    depths[*node] = height;
                    colors[*node] = 2;
                    stack.pop();
                    continue;
                }
                let child = children[*next];
                *next += 1;
                if child < 0 {
                    continue;
                }
                let child = child as usize;
                if colors[child] == 1 {
                    return Err(Error::new(
                        "CyclicNifScene",
                        file,
                        self.blocks[*node].data.start as u64,
                        "acyclic node children",
                        child,
                    ));
                }
                if colors[child] == 0 {
                    limit(
                        stack.len() as u64 + 1,
                        l.max_depth,
                        file,
                        self.blocks[child].data.start as u64,
                    )?;
                    b.reserve(&mut stack, 1, file, 0)?;
                    colors[child] = 1;
                    stack.push((child, 0));
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn sized(c: &mut Cursor<'_>, l: &Limits, b: &Budget) -> Result<Vec<u8>> {
    let n = c.u32()? as u64;
    limit(n, l.max_string, c.source.name(), c.pos - 4)?;
    c.bytes(n, l, b)
}
pub(crate) fn reference(c: &mut Cursor<'_>, count: usize) -> Result<i32> {
    let o = c.pos;
    let v = c.u32()? as i32;
    if v < -1 || v >= 0 && v as usize >= count {
        return Err(Error::new(
            "InvalidNifReference",
            c.source.name(),
            o,
            format!("-1 or 0..{count}"),
            v,
        ));
    }
    Ok(v)
}
pub(crate) fn string_index(c: &mut Cursor<'_>, strings: &[Vec<u8>]) -> Result<u32> {
    let o = c.pos;
    let v = c.u32()?;
    if v != u32::MAX && v as usize >= strings.len() {
        return Err(Error::new(
            "InvalidNifStringIndex",
            c.source.name(),
            o,
            format!("null or < {}", strings.len()),
            v,
        ));
    }
    Ok(v)
}
pub(crate) fn boolean(c: &mut Cursor<'_>) -> Result<bool> {
    let o = c.pos;
    match c.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        v => Err(Error::new(
            "InvalidNifBoolean",
            c.source.name(),
            o,
            "0 or 1",
            v,
        )),
    }
}
#[cfg(test)]
mod tests;

mod numeric;
pub use numeric::NonFiniteCounts;
