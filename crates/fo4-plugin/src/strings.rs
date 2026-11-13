use fo4_core::{add, limit, mul, range, Budget, Cursor, Error, Limits, Result, SliceSource};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::{collections::HashMap, ops::Range};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum StringKind {
    Strings,
    DlStrings,
    IlStrings,
}
impl StringKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Strings => "STRINGS",
            Self::DlStrings => "DLSTRINGS",
            Self::IlStrings => "ILSTRINGS",
        }
    }
}
#[derive(Debug, Serialize)]
pub struct StringDiagnostic {
    pub id: u32,
    pub error: Error,
    pub raw: Range<usize>,
    reported: std::cell::Cell<bool>,
    pub offset: u64,
    pub length: u64,
}
impl StringDiagnostic {
    pub fn mark_reported(&self) -> bool {
        !self.reported.replace(true)
    }
}
pub struct StringTable {
    pub language: String,
    pub bom_prefixed: u64,
    pub english_utf8_multibyte: u64,
    decoded: Vec<(u32, String)>,
    pub file: String,
    pub kind: StringKind,
    pub bytes: Vec<u8>,
    pub entries: Vec<(u32, Range<usize>)>,
    pub diagnostics: Vec<StringDiagnostic>,
}
impl StringTable {
    pub fn read(
        file: &str,
        kind: StringKind,
        bytes: Vec<u8>,
        l: &Limits,
        b: &Budget,
    ) -> Result<Self> {
        Self::read_for_language(file, kind, bytes, "", l, b)
    }
    pub fn read_for_language(
        file: &str,
        kind: StringKind,
        bytes: Vec<u8>,
        language: &str,
        l: &Limits,
        b: &Budget,
    ) -> Result<Self> {
        limit(bytes.len() as u64, l.max_payload, file, 0)?;
        let src = SliceSource {
            name: file,
            bytes: &bytes,
        };
        let mut c = Cursor::new(&src);
        let count = c.u32()? as u64;
        let size = c.u32()? as u64;
        limit(count, l.max_entries, file, 0)?;
        let start = add(8, mul(count, 8, file, 0)?, file, 0)?;
        let end = add(start, size, file, 4)?;
        range(start, size, bytes.len() as u64, file)?;
        if end != bytes.len() as u64 {
            return Err(Error::new(
                "StringDataSizeMismatch",
                file,
                4,
                end,
                bytes.len(),
            ));
        }
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let mut decoded = Vec::new();
        let mut bom_prefixed = 0;
        let mut english_utf8_multibyte = 0;
        b.reserve(&mut decoded, count as usize, file, 0)?;
        b.reserve(&mut entries, count as usize, file, 0)?;
        for _ in 0..count {
            let directory = c.pos;
            let id = c.u32()?;
            let offset = c.u32()? as u64;
            let p = add(start, offset, file, directory + 4)?;
            let fallback = if p < end {
                p as usize..end as usize
            } else {
                directory as usize..directory as usize + 8
            };
            let mut raw = fallback.clone();
            let parsed = (|| -> Result<Range<usize>> {
                range(p, 1, end, file)?;
                let data = if kind == StringKind::Strings {
                    let tail = &bytes[p as usize..end as usize];
                    let n = tail
                        .iter()
                        .take(l.max_string as usize)
                        .position(|x| *x == 0)
                        .ok_or_else(|| {
                            Error::new(
                                "UnterminatedString",
                                file,
                                p,
                                format!("NUL within {} bytes", l.max_string),
                                tail.len(),
                            )
                        })?
                        + 1;
                    p as usize..p as usize + n
                } else {
                    let mut dc = Cursor::at(&src, p, end)?;
                    let n = dc.u32()? as u64;
                    limit(n, l.max_string, file, p)?;
                    range(dc.pos, n, end, file)?;
                    dc.pos as usize..(dc.pos + n) as usize
                };
                raw = data.clone();
                let text = bytes[data.clone()].strip_suffix(&[0]).ok_or_else(|| {
                    Error::new(
                        "UnterminatedString",
                        file,
                        data.start as u64,
                        "final NUL",
                        data.len(),
                    )
                })?;
                if text.contains(&0) {
                    return Err(Error::new(
                        "EmbeddedNul",
                        file,
                        data.start as u64,
                        "single final NUL",
                        "embedded NUL",
                    ));
                }
                // Observe original, delimited bytes. These counters never select a codec.
                if text.starts_with(&[0xef, 0xbb, 0xbf]) {
                    bom_prefixed = add(bom_prefixed, 1, file, data.start as u64)?;
                }
                if is_english(language)
                    && text.iter().any(|byte| *byte >= 0x80)
                    && std::str::from_utf8(text).is_ok()
                {
                    english_utf8_multibyte =
                        add(english_utf8_multibyte, 1, file, data.start as u64)?;
                }
                let text = decode(text, language, file, data.start as u64, b)?;
                decoded.push((id, text));
                Ok(data)
            })();
            match parsed {
                Ok(data) => entries.push((id, data)),
                Err(error) => {
                    b.reserve(&mut diagnostics, 1, file, directory)?;
                    b.charge(512, file, directory)?;
                    diagnostics.push(StringDiagnostic {
                        id,
                        error,
                        reported: std::cell::Cell::new(false),
                        offset: raw.start as u64,
                        length: raw.len() as u64,
                        raw: raw.start..raw.end.min(raw.start + 64),
                    });
                    entries.push((id, raw));
                }
            }
        }
        entries.sort_by_key(|x| x.0);
        for pair in entries.windows(2) {
            if pair[0].0 == pair[1].0 {
                b.reserve(&mut diagnostics, 1, file, 8)?;
                b.charge(512, file, 8)?;
                diagnostics.push(StringDiagnostic {
                    id: pair[0].0,
                    error: Error::new("DuplicateStringId", file, 8, "unique ID", pair[0].0),
                    reported: std::cell::Cell::new(false),
                    offset: pair[0].1.start as u64,
                    length: pair[0].1.len() as u64,
                    raw: pair[0].1.start..pair[0].1.end.min(pair[0].1.start + 64),
                });
            }
        }
        diagnostics.sort_by_key(|d| d.id);
        decoded.sort_by_key(|(id, _)| *id);
        Ok(Self {
            language: language.to_ascii_lowercase(),
            bom_prefixed,
            english_utf8_multibyte,
            decoded,
            file: file.into(),
            kind,
            bytes,
            entries,
            diagnostics,
        })
    }
    pub fn get(&self, id: u32, _l: &Limits) -> Result<&str> {
        if let Ok(i) = self.diagnostics.binary_search_by_key(&id, |d| d.id) {
            return Err(self.diagnostics[i].error.clone());
        }
        let i = self
            .entries
            .binary_search_by_key(&id, |x| x.0)
            .map_err(|_| Error::new("MissingStringId", &self.file, 8, "existing string ID", id))?;
        let decoded = self
            .decoded
            .binary_search_by_key(&id, |(id, _)| *id)
            .map_err(|_| {
                Error::new(
                    "InvalidStringEncoding",
                    &self.file,
                    self.entries[i].1.start as u64,
                    "decoded text",
                    "unavailable",
                )
            })?;
        Ok(&self.decoded[decoded].1)
    }

    pub fn raw(&self, id: u32) -> Option<&[u8]> {
        let i = self.entries.binary_search_by_key(&id, |x| x.0).ok()?;
        if let Ok(j) = self.diagnostics.binary_search_by_key(&id, |d| d.id) {
            return Some(&self.bytes[self.diagnostics[j].raw.clone()]);
        }
        let data = &self.entries[i].1;
        Some(&self.bytes[data.start..data.end.min(data.start + 64)])
    }
}
type CacheKey = (String, String, StringKind);
pub struct StringCache {
    tables: HashMap<CacheKey, Result<StringTable>>,
    allocation: Budget,
    pub loads: u64,
}
impl StringCache {
    pub fn new(b: &Budget) -> Self {
        Self {
            tables: HashMap::new(),
            allocation: b.child(),
            loads: 0,
        }
    }
    pub fn resolve(
        &mut self,
        vfs: &Vfs,
        plugin: &str,
        language: &str,
        kind: StringKind,
        id: u32,
    ) -> Result<&str> {
        self.load(vfs, plugin, language, kind)?
            .get(id, vfs.limits())
    }
    pub fn load(
        &mut self,
        vfs: &Vfs,
        plugin: &str,
        language: &str,
        kind: StringKind,
    ) -> Result<&StringTable> {
        let key = (
            plugin.to_ascii_lowercase(),
            language.to_ascii_lowercase(),
            kind,
        );
        if !self.tables.contains_key(&key) {
            self.allocation.charge(
                256 + key.0.len() as u64 + key.1.len() as u64,
                "strings cache",
                0,
            )?;
            self.tables
                .try_reserve(1)
                .map_err(|e| Error::new("AllocationFailed", "strings cache", 0, "table slot", e))?;
            let stem = plugin.rsplit_once('.').map_or(plugin, |x| x.0);
            let table = (|| {
                let legacy = format!("Strings/{stem}_{language}.{}", kind.extension());
                // Explicit legacy token alias, backed by observed real table names.
                let actual =
                    if language.eq_ignore_ascii_case("english") && vfs.chain(&legacy)?.is_empty() {
                        "en"
                    } else {
                        language
                    };
                let path = format!("Strings/{stem}_{actual}.{}", kind.extension());
                let bytes = vfs.read(&path, &self.allocation).map_err(|mut e| {
                    if e.code == "MissingPath" {
                        e.code = "MissingStringTable".into();
                    }
                    e
                })?;
                StringTable::read_for_language(
                    &path,
                    kind,
                    bytes,
                    actual,
                    vfs.limits(),
                    &self.allocation,
                )
            })();
            self.loads += 1;
            self.tables.insert(key.clone(), table);
        }
        match &self.tables[&key] {
            Ok(table) => Ok(table),
            Err(error) => Err(error.clone()),
        }
    }
    pub fn raw(&self, plugin: &str, language: &str, kind: StringKind, id: u32) -> Option<&[u8]> {
        self.tables
            .get(&(
                plugin.to_ascii_lowercase(),
                language.to_ascii_lowercase(),
                kind,
            ))?
            .as_ref()
            .ok()?
            .raw(id)
    }
    pub fn diagnostic(
        &self,
        plugin: &str,
        language: &str,
        kind: StringKind,
        id: u32,
    ) -> Option<(&StringTable, &StringDiagnostic)> {
        let table = self
            .tables
            .get(&(
                plugin.to_ascii_lowercase(),
                language.to_ascii_lowercase(),
                kind,
            ))?
            .as_ref()
            .ok()?;
        let index = table.diagnostics.binary_search_by_key(&id, |d| d.id).ok()?;
        Some((table, &table.diagnostics[index]))
    }
    pub fn tables(&self) -> impl Iterator<Item = &StringTable> {
        self.tables.values().filter_map(|v| v.as_ref().ok())
    }
}
/// Compatibility one-shot API. Scanners must retain StringCache for repeated lookups.
pub fn resolve_string(
    vfs: &Vfs,
    plugin: &str,
    language: &str,
    kind: StringKind,
    id: u32,
    b: &Budget,
) -> Result<String> {
    let mut cache = StringCache::new(b);
    let text = cache.resolve(vfs, plugin, language, kind, id)?;
    b.charge(text.len() as u64, plugin, 0)?;
    Ok(text.into())
}

fn is_english(language: &str) -> bool {
    language.eq_ignore_ascii_case("en") || language.eq_ignore_ascii_case("english")
}

fn decode(bytes: &[u8], language: &str, file: &str, offset: u64, b: &Budget) -> Result<String> {
    let cp1252 = is_english(language);
    if !cp1252 {
        let text = std::str::from_utf8(bytes).map_err(|e| {
            Error::new(
                "InvalidStringEncoding",
                file,
                offset + e.valid_up_to() as u64,
                "strict UTF-8",
                e,
            )
        })?;
        b.charge(text.len() as u64, file, offset)?;
        let mut out = String::new();
        out.try_reserve_exact(text.len())
            .map_err(|e| Error::new("AllocationFailed", file, offset, "decoded string", e))?;
        out.push_str(text);
        return Ok(out);
    }
    fo4_core::decode_cp1252(bytes, file, offset, b)
}
