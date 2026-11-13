//! BSA 105, range reads. Opaque hash/flags/folder locator bytes are retained.
use fo4_core::{
    add, limit, mul, path_key_bytes, range, read_bytes, Budget, Cursor, Error, Limits, ReadAt,
    Result, SourceSpan,
};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct BsaFolder {
    pub hash: u64,
    pub files: u32,
    pub raw_locator: [u8; 12],
    pub name: Vec<u8>,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct BsaEntry {
    pub diagnostics: Vec<Error>,
    pub name: String,
    pub name_encoding: &'static str,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub original_name: Vec<u8>,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub key: Vec<u8>,
    pub hash: u64,
    pub size_flags: u32,
    pub offset: u32,
    pub source: SourceSpan,
    pub embedded_name: Option<Vec<u8>>,
    pub compressed: bool,
    pub payload_offset: u64,
    pub payload_size: u64,
    pub unpacked: u64,
}
#[derive(Debug, Serialize)]
pub struct BsaArchive {
    pub version: u32,
    pub flags: u32,
    pub file_flags: u32,
    pub folder_name_length: u32,
    pub file_name_length: u32,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub raw_name_tail: Vec<u8>,
    pub folders: Vec<BsaFolder>,
    pub entries: Vec<BsaEntry>,
    pub uninterpreted_fields: u64,
}
impl BsaArchive {
    pub fn read(s: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        l.validate(s.name())?;
        s.check_stable()?;
        let file = s.name();
        let mut c = Cursor::new(s);
        let magic = c.take::<4>()?;
        if magic != *b"BSA\0" {
            return Err(Error::new(
                "InvalidBsaMagic",
                file,
                0,
                "BSA\\0",
                format!("{magic:?}"),
            ));
        }
        let version = c.u32()?;
        if version != 105 {
            return Err(Error::new("UnsupportedBsaVersion", file, 4, 105, version));
        }
        let start = c.u32()?;
        if start != 36 {
            return Err(Error::new("UnsupportedBsaHeaderOffset", file, 8, 36, start));
        }
        let flags = c.u32()?;
        if flags & 3 != 3 || flags & 0xffff_fe40 != 0 {
            return Err(Error::new(
                "UnsupportedBsaFlags",
                file,
                12,
                "PC, folder/file names, known flags <=0x1BF",
                format!("{flags:#x}"),
            ));
        }
        let folder_count = c.u32()? as u64;
        let file_count = c.u32()? as u64;
        limit(folder_count, l.max_entries, file, 16)?;
        limit(file_count, l.max_entries, file, 20)?;
        let folder_name_length = c.u32()?;
        let file_name_length = c.u32()?;
        let file_flags = c.u32()?;
        range(c.pos, mul(folder_count, 24, file, 16)?, s.len(), file)?;
        let table_end = add(c.pos, mul(folder_count, 24, file, 16)?, file, 16)?;
        let minimum = add(
            add(mul(file_count, 16, file, 20)?, folder_count, file, 16)?,
            add(folder_name_length as u64, file_name_length as u64, file, 24)?,
            file,
            24,
        )?;
        range(table_end, minimum, s.len(), file)?;
        b.charge(
            mul(
                add(folder_count, file_count, file, 16)?,
                file.len() as u64,
                file,
                16,
            )?,
            file,
            16,
        )?;
        let mut folders = Vec::new();
        b.reserve(&mut folders, folder_count as usize, file, 16)?;
        let mut total = 0;
        for _ in 0..folder_count {
            let o = c.pos;
            let hash = c.u64()?;
            let files = c.u32()?;
            let raw_locator = c.take()?;
            total = add(total, files as u64, file, o + 8)?;
            limit(total, file_count, file, o + 8)?;
            folders.push(BsaFolder {
                hash,
                files,
                raw_locator,
                name: Vec::new(),
                source: SourceSpan::new(file, o, 24),
            });
        }
        if total != file_count {
            return Err(Error::new(
                "BsaFileCountMismatch",
                file,
                20,
                file_count,
                total,
            ));
        }
        // The documented order is contiguous folder blocks followed by the flat name table.
        // The 12 locator bytes lack a sufficiently precise source; do not derive addresses from them.
        let mut entries = Vec::new();
        b.reserve(&mut entries, file_count as usize, file, 20)?;
        let mut folders_seen = 0u64;
        for folder in &mut folders {
            let o = c.pos;
            let n = c.u8()? as u64;
            limit(n, l.max_string, file, o)?;
            folder.name = c.bytes(n, l, b)?;
            if folder.name.last() != Some(&0) {
                return Err(Error::new(
                    "InvalidBsaFolderName",
                    file,
                    o,
                    "length includes NUL terminator",
                    n,
                ));
            }
            folder.name.pop();
            folders_seen = add(folders_seen, n, file, o)?;
            range(
                c.pos,
                mul(folder.files as u64, 16, file, c.pos)?,
                s.len(),
                file,
            )?;
            for _ in 0..folder.files {
                let o = c.pos;
                let hash = c.u64()?;
                let size_flags = c.u32()?;
                let offset = c.u32()?;
                if size_flags & 0x8000_0000 != 0 {
                    return Err(Error::new(
                        "UnsupportedBsaSizeFlags",
                        file,
                        o + 8,
                        "size and toggle bit 30",
                        size_flags,
                    ));
                }
                let size = (size_flags & 0x3fff_ffff) as u64;
                limit(size, l.max_payload, file, o + 8)?;
                range(offset as u64, size, s.len(), file)?;
                let mut original_name = Vec::new();
                b.reserve(&mut original_name, folder.name.len() + 1, file, o)?;
                original_name.extend_from_slice(&folder.name);
                if !original_name.is_empty() {
                    original_name.push(b'/');
                }
                entries.push(BsaEntry {
                    diagnostics: Vec::new(),
                    name: String::new(),
                    name_encoding: "windows-1252",
                    original_name,
                    key: Vec::new(),
                    hash,
                    size_flags,
                    offset,
                    source: SourceSpan::new(file, o, 16),
                    embedded_name: None,
                    compressed: (flags & 4 != 0) ^ (size_flags & 0x4000_0000 != 0),
                    payload_offset: offset as u64,
                    payload_size: size,
                    unpacked: size,
                });
            }
        }
        if folders_seen != folder_name_length as u64 {
            return Err(Error::new(
                "BsaFolderNameLengthMismatch",
                file,
                24,
                folder_name_length,
                folders_seen,
            ));
        }
        range(c.pos, file_name_length as u64, s.len(), file)?;
        let names_end = add(c.pos, file_name_length as u64, file, c.pos)?;
        c.end = names_end;
        let mut unknown = add(mul(folder_count, 2, file, 16)?, file_count, file, 20)?;
        // File flags, folder-name hash and opaque locator, per-file hash, retention flags.
        unknown = add(unknown, 1 + u64::from(flags & 0xb8 != 0), file, 32)?;
        for e in &mut entries {
            let name_offset = c.pos;
            let mut n = 0u64;
            loop {
                let byte = c.u8()?;
                if byte == 0 {
                    break;
                }
                n = add(n, 1, file, name_offset)?;
                limit(n, l.max_string, file, name_offset)?;
                b.reserve(&mut e.original_name, 1, file, name_offset)?;
                e.original_name.push(byte);
            }
            limit(
                e.original_name.len() as u64,
                l.max_string,
                file,
                name_offset,
            )?;
            b.charge(e.original_name.len() as u64, file, name_offset)?;
            match path_key_bytes(&e.original_name) {
                Ok(key) => e.key = key,
                Err(mut err) => {
                    err.source = Box::new(SourceSpan::new(file, name_offset, n));
                    super::diagnose(&mut e.diagnostics, err, b)?;
                }
            }
            e.name = match fo4_core::decode_cp1252(&e.original_name, file, name_offset, b) {
                Ok(s) => s,
                Err(err) if err.code == "InvalidStringEncoding" => {
                    e.name_encoding = "hex (undefined Windows-1252 byte)";
                    unknown = add(unknown, 1, file, name_offset)?;
                    let size = mul(e.original_name.len() as u64, 2, file, name_offset)?;
                    b.charge(size, file, name_offset)?;
                    let mut name = String::new();
                    name.try_reserve_exact(size as usize).map_err(|err| {
                        Error::new("AllocationFailed", file, name_offset, size, err)
                    })?;
                    use std::fmt::Write;
                    write!(&mut name, "{}", fo4_core::HexBytes(&e.original_name)).map_err(
                        |err| Error::new("DisplayFailed", file, name_offset, "hex bytes", err),
                    )?;
                    name
                }
                Err(e) => return Err(e),
            };
            if e.payload_offset < names_end {
                return Err(Error::new(
                    "BsaPayloadOverlapsIndex",
                    file,
                    e.source.offset + 12,
                    names_end,
                    e.payload_offset,
                ));
            }
            let end = add(e.payload_offset, e.payload_size, file, e.source.offset)?;
            let mut p = Cursor::at(s, e.payload_offset, end)?;
            if flags & 0x100 != 0 {
                let o = p.pos;
                let n = p.u8()? as u64;
                limit(n, l.max_string, file, o)?;
                let embedded = p.bytes(n, l, b)?;
                // Retain the bytes without assuming an encoding or a matching pathname.
                e.embedded_name = Some(embedded);
                unknown = add(unknown, 1, file, o)?;
            }
            if e.compressed {
                let o = p.pos;
                e.unpacked = p.u32()? as u64;
                limit(e.unpacked, l.max_payload, file, o)?;
            }
            e.payload_offset = p.pos;
            e.payload_size = end - p.pos;
            if !e.compressed {
                e.unpacked = e.payload_size;
            }
        }
        let tail_size = c.end - c.pos;
        let raw_name_tail = c.bytes(tail_size, l, b)?;
        if !raw_name_tail.is_empty() {
            unknown = add(unknown, 1, file, c.pos - tail_size)?;
        }
        let mut by_key = Vec::new();
        b.reserve(&mut by_key, entries.len(), file, 20)?;
        by_key.extend(0..entries.len());
        by_key.sort_unstable_by(|a, b| entries[*a].key.cmp(&entries[*b].key));
        for pair in by_key.windows(2) {
            if !entries[pair[0]].key.is_empty() && entries[pair[0]].key == entries[pair[1]].key {
                for i in pair {
                    let e = &mut entries[*i];
                    super::diagnose(
                        &mut e.diagnostics,
                        Error::new(
                            "UnsupportedAmbiguousName",
                            file,
                            e.source.offset,
                            "unique normalized name",
                            &e.name,
                        ),
                        b,
                    )?;
                }
            }
        }
        for e in &mut entries {
            for diagnostic in &mut e.diagnostics {
                b.charge(e.name.len() as u64, file, e.source.offset)?;
                diagnostic.source.archive_entry = Some(e.name.clone());
                diagnostic.source.version = Some(version);
            }
        }
        s.check_stable()?;
        Ok(Self {
            version,
            flags,
            file_flags,
            folder_name_length,
            file_name_length,
            raw_name_tail,
            folders,
            entries,
            uninterpreted_fields: unknown,
        })
    }
    pub fn read_entry(
        &self,
        s: &dyn ReadAt,
        index: usize,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        let e = self.entries.get(index).ok_or_else(|| {
            Error::new("InvalidEntryIndex", s.name(), 0, self.entries.len(), index)
        })?;
        if let Some(err) = e.diagnostics.first() {
            return Err(err.clone());
        }
        let raw = read_bytes(s, e.payload_offset, e.payload_size, l, b)?;
        let result = if e.compressed {
            super::lz4_frame::decode(&raw, e.unpacked, l, b, s.name(), e.payload_offset)
        } else {
            Ok(raw)
        };
        result.map_err(|mut err| {
            err.source.archive_entry = Some(e.name.clone());
            err.source.version = Some(105);
            err
        })
    }
}
