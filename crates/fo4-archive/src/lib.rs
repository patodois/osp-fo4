#![forbid(unsafe_code)]
//! Independent PC BA2 index and bounded range decoder. Facts: spec/ba2.md, spec/dds.md.
mod bsa;
mod prefix;
pub use prefix::Prefix;
pub mod clock;
mod game;
mod lz4_frame;
pub use bsa::{BsaArchive, BsaEntry, BsaFolder};
use fo4_core::{
    add, limit, mul, path_key_bytes, range, read_bytes, zlib, Budget, Cursor, Error, Limits,
    ReadAt, Result, SourceSpan,
};
pub use game::{EntryView, GameArchive};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Chunk {
    pub offset: u64,
    pub packed: u32,
    pub unpacked: u32,
    pub first_mip: Option<u16>,
    pub last_mip: Option<u16>,
    pub sentinel: u32,
    pub table_offset: u64,
}
impl Chunk {
    pub fn stored_size(&self) -> u64 {
        if self.packed == 0 {
            self.unpacked as u64
        } else {
            self.packed as u64
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Texture {
    pub height: u16,
    pub width: u16,
    pub mips: u8,
    pub format: u8,
    pub cube: u8,
    pub tile_mode: u8,
    pub unknown: u8,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub diagnostics: Vec<Error>,
    pub name: String,
    pub name_encoding: &'static str,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub original_name: Vec<u8>,
    #[serde(rename = "key_hex", serialize_with = "fo4_core::serialize_hex")]
    pub key: Vec<u8>,
    pub name_hash: u32,
    pub extension: [u8; 4],
    pub directory_hash: u32,
    pub raw_flags: Option<u32>,
    pub texture: Option<Texture>,
    pub chunks: Vec<Chunk>,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct Archive {
    pub version: u32,
    pub kind: [u8; 4],
    pub entries: Vec<Entry>,
    pub name_table: SourceSpan,
    pub uninterpreted_fields: u64,
}
impl Archive {
    pub fn read(source: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        l.validate(source.name())?;
        source.check_stable()?;
        let file = source.name();
        let mut c = Cursor::new(source);
        let magic = c.take::<4>()?;
        if magic != *b"BTDX" {
            return Err(Error::new(
                "InvalidBa2Magic",
                file,
                0,
                "BTDX",
                format!("{magic:?}"),
            ));
        }
        let version = c.u32()?;
        if !matches!(version, 1 | 7 | 8) {
            return Err(Error::new(
                "UnsupportedBa2Version",
                file,
                4,
                "1, 7, 8",
                version,
            ));
        }
        let kind = c.take::<4>()?;
        if kind != *b"GNRL" && kind != *b"DX10" {
            return Err(Error::new(
                "UnsupportedBa2Type",
                file,
                8,
                "GNRL or DX10",
                format!("{kind:?}"),
            ));
        }
        let count = c.u32()? as u64;
        limit(count, l.max_entries, file, 12)?;
        let names = c.u64()?;
        range(names, mul(count, 2, file, 16)?, source.len(), file)?;
        let min = mul(count, if kind == *b"GNRL" { 36 } else { 24 }, file, 12)?;
        range(c.pos, min, names, file)?;
        c.end = names;
        let mut entries = Vec::new();
        b.reserve(&mut entries, count as usize, file, 12)?;
        let mut unknown = 0;
        for _ in 0..count {
            let start = c.pos;
            let mut diagnostics = Vec::new();
            let name_hash = c.u32()?;
            let extension = c.take()?;
            let directory_hash = c.u32()?;
            let mut chunks = Vec::new();
            let (raw_flags, texture) = if kind == *b"GNRL" {
                let flags = c.u32()?;
                b.reserve(&mut chunks, 1, file, c.pos)?;
                let o = c.pos;
                let offset = c.u64()?;
                let packed = c.u32()?;
                let unpacked = c.u32()?;
                let sentinel = c.u32()?;
                chunks.push(Chunk {
                    offset,
                    packed,
                    unpacked,
                    first_mip: None,
                    last_mip: None,
                    sentinel,
                    table_offset: o,
                });
                unknown += 4;
                (Some(flags), None)
            } else {
                let raw = c.u8()?;
                let n = c.u8()? as u64;
                let stride = c.u16()?;
                if stride != 24 {
                    return Err(Error::new(
                        "UnsupportedChunkHeaderSize",
                        file,
                        start + 14,
                        24,
                        stride,
                    ));
                }
                limit(n, l.max_chunks, file, start + 13)?;
                if n == 0 {
                    diagnose(
                        &mut diagnostics,
                        Error::new("UnsupportedChunkCount", file, start + 13, ">0", n),
                        b,
                    )?;
                }
                let height = c.u16()?;
                let width = c.u16()?;
                let mips = c.u8()?;
                let format = c.u8()?;
                let cube = c.u8()?;
                let tile_mode = c.u8()?;
                let t = Texture {
                    height,
                    width,
                    mips,
                    format,
                    cube,
                    tile_mode,
                    unknown: raw,
                };
                let layout_supported = match t.validate(file, start) {
                    Ok(()) => true,
                    Err(mut e) => {
                        if !e.code.starts_with("Unsupported") {
                            e.code = "UnsupportedTextureLayout".into();
                        }
                        diagnose(&mut diagnostics, e, b)?;
                        false
                    }
                };
                if cube == 1 && n > 1 {
                    diagnose(
                        &mut diagnostics,
                        Error::new(
                            "UnsupportedCubeChunkLayout",
                            file,
                            start + 13,
                            "one cube chunk until layout is sourced",
                            n,
                        ),
                        b,
                    )?;
                }
                if extension != *b"dds\0" {
                    diagnose(
                        &mut diagnostics,
                        Error::new(
                            "UnsupportedTextureExtension",
                            file,
                            start + 4,
                            "dds NUL",
                            format!("{extension:?}"),
                        ),
                        b,
                    )?;
                }
                range(c.pos, mul(n, 24, file, c.pos)?, names, file)?;
                b.reserve(&mut chunks, n as usize, file, c.pos)?;
                let mut next = 0;
                for _ in 0..n {
                    let o = c.pos;
                    let offset = c.u64()?;
                    let packed = c.u32()?;
                    let unpacked = c.u32()?;
                    let first = c.u16()?;
                    let last = c.u16()?;
                    let sentinel = c.u32()?;
                    let interval_valid =
                        first as u32 == next && last >= first && last < mips as u16;
                    if !interval_valid {
                        diagnose(
                            &mut diagnostics,
                            Error::new(
                                "UnsupportedMipRange",
                                file,
                                o + 16,
                                format!("contiguous from {next}, end < {mips}"),
                                format!("{first}..={last}"),
                            ),
                            b,
                        )?;
                    }
                    next = last as u32 + 1;
                    if layout_supported && interval_valid {
                        let expected = mul(
                            t.mip_bytes(first, last, file, start)?,
                            if cube == 1 { 6 } else { 1 },
                            file,
                            o,
                        )?;
                        if unpacked as u64 != expected {
                            diagnose(
                                &mut diagnostics,
                                Error::new(
                                    "UnsupportedTextureSize",
                                    file,
                                    o + 12,
                                    expected,
                                    unpacked,
                                ),
                                b,
                            )?;
                        }
                    }
                    chunks.push(Chunk {
                        offset,
                        packed,
                        unpacked,
                        first_mip: Some(first),
                        last_mip: Some(last),
                        sentinel,
                        table_offset: o,
                    });
                }
                if next != mips as u32 {
                    diagnose(
                        &mut diagnostics,
                        Error::new(
                            "UnsupportedMipRange",
                            file,
                            start + 20,
                            format!("all {mips} mips"),
                            next,
                        ),
                        b,
                    )?;
                }
                unknown += 4;
                (None, Some(t))
            };
            for chunk in &chunks {
                limit(
                    chunk.packed as u64,
                    l.max_payload,
                    file,
                    chunk.table_offset + 8,
                )?;
                limit(
                    chunk.unpacked as u64,
                    l.max_payload,
                    file,
                    chunk.table_offset + 12,
                )?;
                if chunk.sentinel != 0xBAADF00D {
                    return Err(Error::new(
                        "InvalidBa2Sentinel",
                        file,
                        chunk.table_offset + if kind == *b"GNRL" { 16 } else { 20 },
                        "0xBAADF00D",
                        format!("{:#x}", chunk.sentinel),
                    ));
                }
                range(chunk.offset, chunk.stored_size(), names, file)?;
            }
            entries.push(Entry {
                diagnostics,
                name: String::new(),
                original_name: Vec::new(),
                key: Vec::new(),
                name_encoding: "windows-1252",
                name_hash,
                extension,
                directory_hash,
                raw_flags,
                texture,
                chunks,
                source: SourceSpan {
                    version: Some(version),
                    ..SourceSpan::new(file, start, c.pos - start)
                },
            });
        }
        let table_end = c.pos;
        for e in &entries {
            for chunk in &e.chunks {
                if chunk.offset < table_end {
                    return Err(Error::new(
                        "OverlappingRange",
                        file,
                        chunk.table_offset,
                        format!("payload >= {table_end}"),
                        chunk.offset,
                    ));
                }
            }
        }
        // Overlapping payloads are rejected; shared/aliased payload layout is not established here.
        let mut ranges = Vec::new();
        let total = entries
            .iter()
            .try_fold(0u64, |n, e| add(n, e.chunks.len() as u64, file, 0))?;
        b.reserve(&mut ranges, total as usize, file, 0)?;
        for (ranges_entry, e) in entries.iter().enumerate() {
            for chunk in &e.chunks {
                if chunk.stored_size() > 0 {
                    ranges.push((
                        chunk.offset,
                        add(chunk.offset, chunk.stored_size(), file, chunk.offset)?,
                        ranges_entry,
                    ));
                }
            }
        }
        ranges.sort_unstable();
        let mut active: Option<(u64, usize)> = None;
        for &(start, end, index) in &ranges {
            if let Some((prior_end, prior_index)) = active {
                if prior_end > start {
                    for affected in [prior_index, index] {
                        diagnose(
                            &mut entries[affected].diagnostics,
                            Error::new(
                                "UnsupportedSharedPayload",
                                file,
                                start,
                                "independent payload layout",
                                format!("overlap with entry {prior_index}"),
                            ),
                            b,
                        )?;
                    }
                }
                if end > prior_end {
                    active = Some((end, index));
                }
            } else {
                active = Some((end, index));
            }
        }
        let mut nc = Cursor::at(source, names, source.len())?;
        let mut keys = Vec::new();
        b.reserve(&mut keys, count as usize, file, names)?;
        for (i, e) in entries.iter_mut().enumerate() {
            let o = nc.pos;
            let n = nc.u16()? as u64;
            let raw = nc.bytes(n, l, b)?;
            let bytes = raw.as_slice();
            let parsed = (|| -> Result<(String, Vec<u8>)> {
                limit(n, l.max_string, file, o)?;
                b.charge(mul(n, 2, file, o)?, file, o)?;
                let key = path_key_bytes(bytes)?;
                let name = match fo4_core::decode_cp1252(bytes, file, o, b) {
                    Ok(name) => name,
                    Err(error) if error.code == "InvalidStringEncoding" => {
                        e.name_encoding = "hex (undefined Windows-1252 byte)";
                        unknown = add(unknown, 1, file, o)?;
                        let size = mul(n, 2, file, o)?;
                        b.charge(size, file, o)?;
                        let mut hex = String::new();
                        hex.try_reserve_exact(size as usize).map_err(|error| {
                            Error::new("AllocationFailed", file, o, size, error)
                        })?;
                        use std::fmt::Write;
                        write!(&mut hex, "{}", fo4_core::HexBytes(bytes)).map_err(|error| {
                            Error::new("DisplayFailed", file, o, "hex bytes", error)
                        })?;
                        hex
                    }
                    Err(error) => return Err(error),
                };
                Ok((name, key))
            })();
            match parsed {
                Ok((name, key)) => {
                    e.name = name;
                    e.key = key.clone();
                    keys.push((key, i));
                }
                Err(mut err) => {
                    err.code = "UnsupportedEntryName".into();
                    err.source = Box::new(SourceSpan::new(file, o, n));
                    diagnose(&mut e.diagnostics, err, b)?;
                }
            }
            e.original_name = raw;
        }
        keys.sort_unstable();
        for pair in keys.windows(2) {
            if pair[0].0 == pair[1].0 {
                for index in [pair[0].1, pair[1].1] {
                    diagnose(
                        &mut entries[index].diagnostics,
                        Error::new(
                            "UnsupportedAmbiguousName",
                            file,
                            names,
                            "unique normalized name",
                            fo4_core::HexBytes(&pair[0].0),
                        ),
                        b,
                    )?;
                }
            }
        }
        for e in &mut entries {
            for diagnostic in &mut e.diagnostics {
                diagnostic.source.archive_entry = if e.name.is_empty() {
                    None
                } else {
                    Some(e.name.clone())
                };
                diagnostic.source.version = Some(version);
            }
        }
        source.check_stable()?;
        if nc.pos != nc.end {
            return Err(Error::new(
                "TrailingBa2Names",
                file,
                nc.pos,
                "end of name table",
                nc.end - nc.pos,
            ));
        }
        Ok(Self {
            version,
            kind,
            entries,
            name_table: SourceSpan::new(file, names, nc.pos - names),
            uninterpreted_fields: unknown,
        })
    }
    pub fn decode_chunk(
        &self,
        source: &dyn ReadAt,
        entry: usize,
        chunk: usize,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        let e = self.entries.get(entry).ok_or_else(|| {
            Error::new(
                "InvalidEntryIndex",
                source.name(),
                0,
                self.entries.len(),
                entry,
            )
        })?;
        let ch = e.chunks.get(chunk).ok_or_else(|| {
            Error::new(
                "InvalidChunkIndex",
                source.name(),
                e.source.offset,
                e.chunks.len(),
                chunk,
            )
        })?;
        let raw = clock::reading(|| read_bytes(source, ch.offset, ch.stored_size(), l, b))?;
        let result = if ch.packed == 0 {
            Ok(raw)
        } else {
            clock::inflating(|| zlib(&raw, ch.unpacked as u64, l, b, source.name(), ch.offset))
        };
        result.map_err(|mut err| {
            err.source.archive_entry = Some(e.name.clone());
            err.source.version = Some(self.version);
            err
        })
    }
    /// Materialize just one requested asset. Sweeps should call decode_chunk and discard each result.
    pub fn read_entry(
        &self,
        source: &dyn ReadAt,
        index: usize,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        let e = self.entries.get(index).ok_or_else(|| {
            Error::new(
                "InvalidEntryIndex",
                source.name(),
                0,
                self.entries.len(),
                index,
            )
        })?;
        if let Some(error) = e.diagnostics.first() {
            return Err(error.clone());
        }
        let total = e
            .chunks
            .iter()
            .try_fold(if e.texture.is_some() { 148 } else { 0 }, |n, c| {
                add(n, c.unpacked as u64, source.name(), c.table_offset)
            })?;
        limit(total, l.max_payload, source.name(), e.source.offset)?;
        let mut out = Vec::new();
        b.reserve(&mut out, total as usize, source.name(), e.source.offset)?;
        if let Some(t) = &e.texture {
            out.extend_from_slice(&t.dds_header(source.name(), e.source.offset)?);
            // A supported cube has one chunk; multi-chunk cube order is unknown.
            // Non-cube intervals are already validated in increasing mip order.
            for chunk in 0..e.chunks.len() {
                out.extend_from_slice(&self.decode_chunk(source, index, chunk, l, b)?);
            }
        } else {
            out.extend_from_slice(&self.decode_chunk(source, index, 0, l, b)?);
        }
        Ok(out)
    }
}
fn diagnose(diagnostics: &mut Vec<Error>, error: Error, b: &Budget) -> Result<()> {
    b.reserve(diagnostics, 1, &error.source.file, error.source.offset)?;
    b.charge(
        (error.expected.len() + error.found.len() + error.source.file.len() + error.code.len())
            as u64,
        &error.source.file,
        error.source.offset,
    )?;
    diagnostics.push(error);
    Ok(())
}
impl Texture {
    fn validate(&self, file: &str, o: u64) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::new(
                "InvalidTextureDimensions",
                file,
                o + 16,
                "nonzero width and height",
                format!("{}x{}", self.width, self.height),
            ));
        }
        let max = 16 - self.width.max(self.height).leading_zeros();
        if self.mips == 0 || self.mips as u32 > max {
            return Err(Error::new(
                "InvalidMipCount",
                file,
                o + 20,
                format!("1..={max}"),
                self.mips,
            ));
        }
        if self.cube > 1 || self.tile_mode != 8 {
            return Err(Error::new(
                "UnsupportedTextureLayout",
                file,
                o + 22,
                "cube 0/1, PC tile mode 8",
                format!("{}, {}", self.cube, self.tile_mode),
            ));
        }
        if self.cube == 1 && self.width != self.height {
            return Err(Error::new(
                "InvalidCubeDimensions",
                file,
                o + 16,
                "square faces",
                format!("{}x{}", self.width, self.height),
            ));
        }
        self.surface(0, file, o)?;
        Ok(())
    }
    fn format_size(&self, file: &str, o: u64) -> Result<(u64, bool)> {
        Ok(match self.format {
            70..=72 | 79..=81 => (8, true),
            73..=78 | 82..=84 | 94..=99 => (16, true),
            1..=4 => (16, false),
            5..=8 => (12, false),
            9..=22 => (8, false),
            23..=47 | 67 | 87..=93 => (4, false),
            48..=59 | 85 | 86 | 115 => (2, false),
            60..=65 => (1, false),
            n => {
                return Err(Error::new(
                    "UnsupportedDxgiFormat",
                    file,
                    o + 21,
                    "documented byte-addressable/BC DXGI format",
                    n,
                ))
            }
        })
    }
    fn surface(&self, mip: u16, file: &str, o: u64) -> Result<(u64, u64, bool)> {
        let (size, bc) = self.format_size(file, o)?;
        let w = (self.width as u64 >> mip).max(1);
        let h = (self.height as u64 >> mip).max(1);
        let pitch = mul(if bc { w.div_ceil(4) } else { w }, size, file, o)?;
        Ok((
            pitch,
            mul(pitch, if bc { h.div_ceil(4) } else { h }, file, o)?,
            bc,
        ))
    }
    fn mip_bytes(&self, first: u16, last: u16, file: &str, o: u64) -> Result<u64> {
        let mut n = 0;
        for mip in first..=last {
            n = add(n, self.surface(mip, file, o)?.1, file, o)?;
        }
        Ok(n)
    }
    pub fn dds_header(&self, file: &str, o: u64) -> Result<[u8; 148]> {
        self.validate(file, o)?;
        let mut h = [0u8; 148];
        h[..4].copy_from_slice(b"DDS ");
        let (pitch, size, bc) = self.surface(0, file, o)?;
        let mip_flags = if self.mips > 1 { 0x20000 } else { 0 };
        let caps =
            0x1000 | if self.mips > 1 { 0x400008 } else { 0 } | if self.cube == 1 { 8 } else { 0 };
        for (offset, value) in [
            (4, 124),
            (8, 0x1007 | mip_flags | if bc { 0x80000 } else { 8 }),
            (12, self.height as u32),
            (16, self.width as u32),
            (
                20,
                u32::try_from(if bc { size } else { pitch })
                    .map_err(|x| Error::new("Overflow", file, o, "u32 pitch", x))?,
            ),
            (28, self.mips as u32),
            (76, 32),
            (80, 4),
            (108, caps),
            (112, if self.cube == 1 { 0xfe00 } else { 0 }),
            (128, self.format as u32),
            (132, 3),
            (136, if self.cube == 1 { 4 } else { 0 }),
            (140, 1),
        ] {
            h[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        h[84..88].copy_from_slice(b"DX10");
        Ok(h)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bsa_tests;
