#![forbid(unsafe_code)]
//! TES4-family FO4 envelopes, selected fields and explicit FormKey resolution.
//! See spec/plugin.md and spec/strings.md; unhandled bytes remain available.
pub mod ficha;
mod semantics;
pub mod still;
mod strings;
pub mod typed;
pub mod writer;
use fo4_core::{
    add, limit, range, read_bytes_inside_operation, zlib, Budget, Cursor, Error, Limits, ReadAt,
    Result, SliceSource, SourceSpan,
};
pub use semantics::{Field, Value, PHASE1_TYPES};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};
pub use strings::{resolve_string, StringCache, StringDiagnostic, StringKind, StringTable};

pub const MASTER: u32 = 1;
pub const LOCALIZED: u32 = 0x80;
pub const LIGHT: u32 = 0x200;
pub const DELETED: u32 = 0x20;
pub const DISABLED: u32 = 0x800;
pub const COMPRESSED: u32 = 0x40000;
#[derive(Debug, Serialize)]
pub struct Subrecord {
    pub signature: [u8; 4],
    pub data: Range<usize>,
    pub offset: u64,
    pub extended: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub source: SourceSpan,
    pub label: [u8; 4],
    pub kind: u32,
    pub raw_tail: [u8; 8],
}
#[derive(Debug, Serialize)]
pub struct Record {
    pub signature: [u8; 4],
    pub flags: u32,
    pub form_id: u32,
    pub revision: u32,
    pub version: u16,
    pub unknown: u16,
    pub source: SourceSpan,
    pub payload: Vec<u8>,
    pub subrecords: Vec<Subrecord>,
    /// Positions in decompressed payload when compressed; never claimed as disk offsets.
    pub payload_is_decompressed: bool,
}
impl Record {
    pub fn deleted(&self) -> bool {
        self.flags & DELETED != 0
    }
    pub fn disabled(&self) -> bool {
        matches!(
            &self.signature,
            b"REFR"
                | b"ACHR"
                | b"PGRE"
                | b"PMIS"
                | b"PHZD"
                | b"PARW"
                | b"PBAR"
                | b"PBEA"
                | b"PCON"
                | b"PFLA"
        ) && self.flags & DISABLED != 0
    }
    pub fn data(&self, s: &Subrecord) -> &[u8] {
        &self.payload[s.data.clone()]
    }
    pub fn fields_for_game(
        &self,
        game: fo4_core::Game,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<Field>> {
        if game == fo4_core::Game::Fallout4 {
            return self.fields(l, b);
        }
        // SSE record bodies remain envelopes: FO4 field layouts are not shared by assumption.
        let mut fields = Vec::new();
        b.reserve(
            &mut fields,
            self.subrecords.len(),
            &self.source.file,
            self.source.offset,
        )?;
        for (i, s) in self.subrecords.iter().enumerate() {
            fields.push(Field {
                signature: s.signature,
                subrecord: i,
                value: Value::Raw,
                non_finite_values: 0,
            });
        }
        Ok(fields)
    }
    pub fn fields(&self, l: &Limits, b: &Budget) -> Result<Vec<Field>> {
        semantics::interpret(self, l, b).map_err(|mut e| {
            if self.payload_is_decompressed {
                e.source.decompressed_offset = Some(e.source.offset);
                e.source.offset = self.source.offset + 24;
                e.source.length = self.source.length - 24;
            }
            e
        })
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub game: fo4_core::Game,
    pub form_version: u16,
    pub diagnostics: Vec<Error>,
    pub name: String,
    pub source: SourceSpan,
    pub hedr_offset: u64,
    pub master_offsets: Vec<u64>,
    pub raw_subrecords: Vec<SourceSpan>,
    pub flags: u32,
    pub hedr_version: f32,
    pub declared_records: u32,
    pub next_object_id: u32,
    pub masters: Vec<String>,
    pub master_data: Vec<[u8; 8]>,
    pub end: u64,
    pub uninterpreted_subrecords: u64,
    pub raw_flags: u32,
}
impl Header {
    pub fn is_master(&self) -> bool {
        self.flags & MASTER != 0
            || self.name.rsplit_once('.').is_some_and(|(_, ext)| {
                ext.eq_ignore_ascii_case("esm") || ext.eq_ignore_ascii_case("esl")
            })
    }
    pub fn is_light(&self) -> bool {
        self.flags & (MASTER | LIGHT) == (MASTER | LIGHT)
            || self.name.rsplit_once('.').is_some_and(|(_, ext)| {
                ext.eq_ignore_ascii_case("esl")
                    || (ext.eq_ignore_ascii_case("esp") && self.flags & LIGHT != 0)
            })
    }
    pub fn read(name: &str, source: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        Self::read_for_game(fo4_core::Game::Fallout4, name, source, l, b)
    }
    pub fn read_for_game(
        game: fo4_core::Game,
        name: &str,
        source: &dyn ReadAt,
        l: &Limits,
        b: &Budget,
    ) -> Result<Self> {
        source.check_stable()?;
        let r = record_at(source, 0, source.len(), l, b)?;
        source.check_stable()?;
        if r.signature != *b"TES4" || r.form_id != 0 || r.payload_is_decompressed {
            return Err(Error::new(
                "InvalidPluginHeader",
                source.name(),
                0,
                "uncompressed TES4, FormID 0",
                format!("{:?}, {}, {}", r.signature, r.form_id, r.flags),
            ));
        }
        let mut version = None;
        let mut hedr_offset = 0;
        let mut master_offsets = Vec::new();
        let mut raw_subrecords = Vec::new();
        let mut count = None;
        let mut next = None;
        let mut masters = Vec::new();
        let mut master_data = Vec::new();
        let mut pending = false;
        let mut unknown = 0;
        for s in &r.subrecords {
            let data = r.data(s);
            let f = source.name();
            if pending && s.signature != *b"DATA" {
                return Err(Error::new(
                    "MissingMasterData",
                    f,
                    s.offset,
                    "DATA following MAST",
                    format!("{:?}", s.signature),
                ));
            }
            match &s.signature {
                b"HEDR" => {
                    exact(data, 12, f, s.offset)?;
                    if version.is_some() {
                        return Err(Error::new("DuplicateHedr", f, s.offset, "one HEDR", 2));
                    }
                    let src = SliceSource {
                        name: f,
                        bytes: data,
                    };
                    let mut c = Cursor::new(&src);
                    version = Some(c.f32().map_err(|mut e| {
                        e.source.offset += s.offset + 6;
                        e
                    })?);
                    hedr_offset = s.offset;
                    count = Some(c.u32()?);
                    next = Some(c.u32()?);
                }
                b"MAST" => {
                    limit(masters.len() as u64 + 1, l.max_entries, f, s.offset)?;
                    let n = zstring(data, l, f, s.offset)?;
                    if n.contains(['/', '\\']) || n.is_empty() || !n.is_ascii() {
                        return Err(Error::new(
                            "InvalidMasterName",
                            f,
                            s.offset,
                            "ASCII plugin basename",
                            n,
                        ));
                    }
                    if masters.iter().any(|m: &String| m.eq_ignore_ascii_case(n)) {
                        return Err(Error::new("DuplicateMaster", f, s.offset, "unique MAST", n));
                    }
                    b.charge(n.len() as u64, f, s.offset)?;
                    b.reserve(&mut masters, 1, f, s.offset)?;
                    masters.push(n.into());
                    b.reserve(&mut master_offsets, 1, f, s.offset)?;
                    master_offsets.push(s.offset);
                    pending = true;
                }
                b"DATA" if pending => {
                    exact(data, 8, f, s.offset)?;
                    let mut raw = [0; 8];
                    raw.copy_from_slice(data);
                    b.reserve(&mut master_data, 1, f, s.offset)?;
                    master_data.push(raw);
                    pending = false;
                }
                _ => {
                    unknown += 1;
                    b.reserve(&mut raw_subrecords, 1, f, s.offset)?;
                    b.charge(f.len() as u64, f, s.offset)?;
                    raw_subrecords.push(SourceSpan::new(f, s.offset, 6 + data.len() as u64));
                }
            }
        }
        if pending {
            return Err(Error::new(
                "MissingMasterData",
                source.name(),
                r.source.length,
                "DATA after MAST",
                "EOF",
            ));
        }
        let hedr_version = version
            .ok_or_else(|| Error::new("MissingHedr", source.name(), 24, "HEDR", "absent"))?;
        let supported = match game {
            fo4_core::Game::Fallout4 => matches!(hedr_version, 0.95 | 1.0),
            fo4_core::Game::SkyrimSe => matches!(hedr_version, 1.7 | 1.71),
        };
        if !supported {
            return Err(Error::new(
                "UnsupportedPluginVersion",
                source.name(),
                hedr_offset + 6,
                format!("{game:?}: FO4 0.95/1.0, SSE 1.7/1.71"),
                hedr_version,
            ));
        }
        let mut diagnostics = Vec::new();
        if masters.len() > 253 {
            b.reserve(&mut diagnostics, 1, source.name(), 24)?;
            diagnostics.push(Error::new(
                "UnverifiedMasterCountRule",
                source.name(),
                24,
                "on-disk high byte must be representable; no sourced 253-header cap",
                masters.len(),
            ));
        }
        // Newer update-plugin semantics are deliberately not treated as ordinary full/light.
        if r.flags & 0x100000 != 0 {
            return Err(Error::new(
                "UnsupportedPluginFlags",
                source.name(),
                8,
                "full/light FO4 plugin",
                format!("{:#x}", r.flags),
            ));
        }
        Ok(Self {
            game,
            form_version: r.version,
            diagnostics,
            name: name.into(),
            source: r.source.clone(),
            hedr_offset,
            master_offsets,
            raw_subrecords,
            flags: r.flags,
            hedr_version,
            declared_records: count.ok_or_else(|| {
                Error::new("MissingHedr", source.name(), 24, "record count", "absent")
            })?,
            next_object_id: next.ok_or_else(|| {
                Error::new("MissingHedr", source.name(), 24, "next object ID", "absent")
            })?,
            masters,
            master_data,
            end: r.source.length,
            uninterpreted_subrecords: unknown,
            raw_flags: r.flags & !(MASTER | LOCALIZED | LIGHT),
        })
    }
}
pub(crate) fn exact(data: &[u8], n: usize, file: &str, o: u64) -> Result<()> {
    if data.len() != n {
        Err(Error::new("InvalidFieldSize", file, o, n, data.len()))
    } else {
        Ok(())
    }
}
pub(crate) fn zstring<'a>(data: &'a [u8], l: &Limits, file: &str, o: u64) -> Result<&'a str> {
    limit(data.len() as u64, l.max_string, file, o)?;
    let bytes = data.strip_suffix(&[0]).ok_or_else(|| {
        Error::new(
            "UnterminatedString",
            file,
            o,
            "NUL-terminated string",
            data.len(),
        )
    })?;
    if bytes.contains(&0) {
        return Err(Error::new(
            "EmbeddedNul",
            file,
            o,
            "single final NUL",
            "embedded NUL",
        ));
    }
    std::str::from_utf8(bytes).map_err(|e| {
        Error::new(
            "UnsupportedTextEncoding",
            file,
            o,
            "UTF-8 (raw bytes retained)",
            e,
        )
    })
}
/// Reads one record. Callers check the source's stability around their operation.
fn record_at(source: &dyn ReadAt, o: u64, end: u64, l: &Limits, b: &Budget) -> Result<Record> {
    let mut c = Cursor::at(source, o, end)?;
    let signature = c.take::<4>()?;
    let size = c.u32()? as u64;
    let flags = c.u32()?;
    let form_id = c.u32()?;
    let revision = c.u32()?;
    let version = c.u16()?;
    let unknown = c.u16()?;
    range(c.pos, size, end, source.name())?;
    let raw = read_bytes_inside_operation(source, c.pos, size, l, b)?;
    let compressed = flags & COMPRESSED != 0;
    let payload = if compressed {
        let s = SliceSource {
            name: source.name(),
            bytes: &raw,
        };
        let mut cur = Cursor::new(&s);
        let declared = cur.u32()? as u64;
        zlib(&raw[4..], declared, l, b, source.name(), c.pos + 4)?
    } else {
        raw
    };
    let payload_base = if compressed { 0 } else { c.pos };
    let subrecords =
        parse_subrecords(&payload, source.name(), payload_base, l, b).map_err(|mut error| {
            if compressed {
                error.source.decompressed_offset = Some(error.source.offset);
                error.source.offset = c.pos;
                error.source.length = size;
            }
            error
        })?;
    Ok(Record {
        signature,
        flags,
        form_id,
        revision,
        version,
        unknown,
        source: SourceSpan::new(source.name(), o, add(24, size, source.name(), o)?),
        payload,
        subrecords,
        payload_is_decompressed: compressed,
    })
}
fn parse_subrecords(
    payload: &[u8],
    file: &str,
    base: u64,
    l: &Limits,
    b: &Budget,
) -> Result<Vec<Subrecord>> {
    let s = SliceSource {
        name: file,
        bytes: payload,
    };
    let mut c = Cursor::new(&s);
    let mut result = Vec::new();
    let mut extended = None;
    while c.pos < c.end {
        limit(
            result.len() as u64 + 1,
            l.max_subrecords,
            file,
            base + c.pos,
        )?;
        let o = c.pos;
        let signature = c.take::<4>()?;
        let small = c.u16()? as u64;
        if signature == *b"XXXX" {
            if small != 4 || extended.is_some() {
                return Err(Error::new(
                    "InvalidXxxx",
                    file,
                    base + o,
                    "one XXXX of size 4",
                    small,
                ));
            }
            extended = Some(c.u32()? as u64);
            continue;
        }
        let is_extended = extended.is_some();
        let n = extended.take().unwrap_or(small);
        range(c.pos, n, c.end, file)?;
        limit(n, l.max_payload, file, base + o)?;
        b.reserve(&mut result, 1, file, base + o)?;
        let start = c.pos as usize;
        c.pos = add(c.pos, n, file, base + o)?;
        result.push(Subrecord {
            signature,
            data: start..c.pos as usize,
            offset: base + o,
            extended: is_extended,
        });
    }
    if extended.is_some() {
        return Err(Error::new(
            "DanglingXxxx",
            file,
            base + c.pos,
            "subrecord following XXXX",
            "EOF",
        ));
    }
    Ok(result)
}
#[derive(Debug, Default, Serialize)]
pub struct ScanStats {
    pub count_matches: bool,
    pub groups: u64,
    pub records: u64,
    pub subrecords: u64,
    pub compressed: u64,
    pub envelope_records: u64,
    pub uninterpreted_subrecords: u64,
}
/// Structural errors stop the bounded region. Visitor errors can be recorded by the caller,
/// allowing subsequent well-delimited records to be visited without hiding a failure.
pub fn scan(
    source: &dyn ReadAt,
    h: &Header,
    l: &Limits,
    mut visit: impl FnMut(Record, &[Group]) -> Result<()>,
) -> Result<ScanStats> {
    scan_bounded(
        source,
        h,
        l,
        &Budget::new(l.max_memory),
        |record, groups, _| visit(record, groups),
    )
}
pub fn scan_bounded(
    source: &dyn ReadAt,
    h: &Header,
    l: &Limits,
    b: &Budget,
    mut visit: impl FnMut(Record, &[Group], &Budget) -> Result<()>,
) -> Result<ScanStats> {
    // One stability check around the whole scan, not two per record.
    source.check_stable()?;
    let mut c = Cursor::at(source, h.end, source.len())?;
    let mut stack: Vec<Group> = Vec::new();
    let mut stats = ScanStats::default();
    let stack_budget = b.child();
    while c.pos < source.len() {
        while stack
            .last()
            .is_some_and(|g| g.source.offset + g.source.length == c.pos)
        {
            stack.pop();
        }
        c.end = stack
            .last()
            .map_or(source.len(), |g| g.source.offset + g.source.length);
        let o = c.pos;
        let sig = c.take::<4>()?;
        if sig == *b"GRUP" {
            limit(stack.len() as u64 + 1, l.max_depth, source.name(), o)?;
            limit(
                add(stats.groups, stats.records, source.name(), o)? + 1,
                l.max_records,
                source.name(),
                o,
            )?;
            let size = c.u32()? as u64;
            if size < 24 {
                return Err(Error::new(
                    "InvalidGroupSize",
                    source.name(),
                    o + 4,
                    ">=24",
                    size,
                ));
            }
            range(o, size, c.end, source.name())?;
            let label = c.take()?;
            let kind = c.u32()?;
            let raw_tail = c.take()?;
            // Group kinds beyond FO4's 0..10 are preserved as raw, never assigned semantics.
            stack_budget.reserve(&mut stack, 1, source.name(), o)?;
            stack.push(Group {
                source: SourceSpan::new(source.name(), o, size),
                label,
                kind,
                raw_tail,
            });
            stats.groups += 1;
        } else {
            limit(
                add(stats.groups, stats.records, source.name(), o)? + 1,
                l.max_records,
                source.name(),
                o,
            )?;
            let record_budget = b.child();
            let record = record_at(source, o, c.end, l, &record_budget)?;
            if record.signature == *b"TES4" {
                return Err(Error::new(
                    "UnexpectedTes4",
                    source.name(),
                    o,
                    "single initial TES4",
                    "TES4",
                ));
            }
            c.pos = add(o, record.source.length, source.name(), o)?;
            stats.records += 1;
            stats.subrecords = add(
                stats.subrecords,
                record.subrecords.len() as u64,
                source.name(),
                o,
            )?;
            limit(stats.subrecords, l.max_subrecords, source.name(), o)?;
            if record.payload_is_decompressed {
                stats.compressed += 1;
            }
            if !PHASE1_TYPES.contains(&record.signature) {
                stats.envelope_records += 1;
                stats.uninterpreted_subrecords += record.subrecords.len() as u64;
            }
            visit(record, &stack, &record_budget)?;
        }
    }
    // Every group must finish at the physical endpoint or have been popped above.
    if stack
        .iter()
        .any(|g| g.source.offset + g.source.length != c.pos)
    {
        return Err(Error::new(
            "Truncated",
            source.name(),
            c.pos,
            "all group ends reached",
            "EOF",
        ));
    }
    source.check_stable()?;
    stats.count_matches = stats.groups + stats.records == h.declared_records as u64;
    Ok(stats)
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash, Serialize)]
pub struct FormKey {
    pub plugin: String,
    pub object_id: u32,
}
#[derive(Debug, Clone, Serialize)]
pub enum Slot {
    Full(u8),
    Light(u16),
}
/// Validated order with immutable public accessors.
/// ```compile_fail
/// fn tamper(order:&mut fo4_plugin::LoadOrder){order.headers.clear();}
/// ```
pub struct LoadOrder {
    headers: Vec<Header>,
    slots: Vec<Slot>,
    master_indices: Vec<Vec<usize>>,
    names: HashMap<String, usize>,
}
impl LoadOrder {
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }
    pub fn new(headers: Vec<Header>, b: &Budget) -> Result<Self> {
        if let Some(first) = headers.first() {
            for h in &headers {
                if h.game != first.game {
                    return Err(Error::new(
                        "MixedGameLoadOrder",
                        &h.source.file,
                        0,
                        format!("{:?}", first.game),
                        format!("{:?}", h.game),
                    ));
                }
            }
        }
        let mut names = HashMap::new();
        b.charge(
            (headers.len() * std::mem::size_of::<(String, usize)>() * 2) as u64,
            "manifest",
            0,
        )?;
        names
            .try_reserve(headers.len())
            .map_err(|e| Error::new("AllocationFailed", "manifest", 0, "plugin name index", e))?;
        for (i, h) in headers.iter().enumerate() {
            b.charge(h.name.len() as u64, &h.name, 0)?;
            if names.insert(h.name.to_ascii_lowercase(), i).is_some() {
                return Err(Error::new(
                    "AmbiguousPath",
                    &h.name,
                    0,
                    "unique plugin name",
                    &h.name,
                ));
            }
        }
        let mut graph = Vec::new();
        b.reserve(&mut graph, headers.len(), "manifest", 0)?;
        for h in &headers {
            let mut masters = Vec::new();
            b.reserve(&mut masters, h.masters.len(), &h.name, 0)?;
            for name in &h.masters {
                masters.push(
                    *names
                        .get(&name.to_ascii_lowercase())
                        .ok_or_else(|| Error::new("MissingMaster", &h.name, 24, name, "absent"))?,
                );
            }
            graph.push(masters);
        }
        let mut colors = b.bytes(headers.len() as u64, "manifest", 0)?;
        let mut stack = Vec::new();
        b.reserve(&mut stack, headers.len(), "manifest", 0)?;
        for start in 0..headers.len() {
            if colors[start] != 0 {
                continue;
            }
            colors[start] = 1;
            stack.push((start, 0usize));
            while let Some((node, next)) = stack.last_mut() {
                if *next == graph[*node].len() {
                    colors[*node] = 2;
                    stack.pop();
                    continue;
                }
                let target = graph[*node][*next];
                *next += 1;
                if colors[target] == 1 {
                    return Err(Error::new(
                        "CyclicMaster",
                        &headers[*node].source.file,
                        headers[*node].master_offsets[*next - 1],
                        "acyclic MAST graph",
                        &headers[target].name,
                    ));
                }
                if colors[target] == 0 {
                    colors[target] = 1;
                    stack.push((target, 0));
                }
            }
        }
        let mut slots = Vec::new();
        b.reserve(&mut slots, headers.len(), "manifest", 0)?;
        let mut full = 0u16;
        let mut saw_nonmaster = false;
        let mut light = 0u16;
        for (i, h) in headers.iter().enumerate() {
            if h.is_master() && saw_nonmaster {
                return Err(Error::new(
                    "NonMasterBeforeMaster",
                    &h.source.file,
                    8,
                    "all ESM-flagged plugins before nonmasters",
                    &h.name,
                ));
            }
            if !h.is_master() {
                saw_nonmaster = true;
            }
            if headers[..i]
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&h.name))
            {
                return Err(Error::new(
                    "AmbiguousPath",
                    &h.name,
                    0,
                    "unique plugin name",
                    &h.name,
                ));
            }
            for (master_index, m) in h.masters.iter().enumerate() {
                let pos = headers
                    .iter()
                    .position(|x| x.name.eq_ignore_ascii_case(m))
                    .ok_or_else(|| Error::new("MissingMaster", &h.name, 24, m, "absent"))?;
                if pos >= i {
                    return Err(Error::new(
                        if pos == i {
                            "CyclicMaster"
                        } else {
                            "MasterAfterDependent"
                        },
                        &h.source.file,
                        h.master_offsets[master_index],
                        "master before dependent",
                        m,
                    ));
                }
            }
            if h.is_light() {
                limit(light as u64 + 1, 4096, &h.name, 8)?;
                slots.push(Slot::Light(light));
                light += 1;
            } else {
                limit(full as u64 + 1, 254, &h.name, 8)?;
                slots.push(Slot::Full(full as u8));
                full += 1;
            }
        }
        Ok(Self {
            headers,
            slots,
            master_indices: graph,
            names,
        })
    }
    /// On-disk index is relative to MAST, including light masters. 0 means null.
    pub fn remap(&self, plugin: usize, id: u32, file: &str, o: u64) -> Result<Option<FormKey>> {
        if id == 0 {
            return Ok(None);
        }
        let h = self
            .headers
            .get(plugin)
            .ok_or_else(|| Error::new("InvalidPluginIndex", file, o, self.headers.len(), plugin))?;
        let local = (id >> 24) as usize;
        let object = id & 0x00ff_ffff;
        let target = if local < h.masters.len() {
            self.master_indices[plugin][local]
        } else if local == h.masters.len() {
            plugin
        } else {
            return Err(Error::new(
                "InvalidFormId",
                file,
                o,
                format!("local master index <= {}", h.masters.len()),
                format!("{id:#010x}"),
            ));
        };
        if h.game == fo4_core::Game::SkyrimSe
            && h.hedr_version < 1.71
            && self.headers[target].is_light()
            && object < 0x800
        {
            return Err(Error::new(
                "InvalidFormId",
                file,
                o,
                "SSE HEDR 1.71 for extended light reference",
                format!("{id:#010x}"),
            ));
        }
        if object == 0
            || (self.headers[target].is_light()
                && !(self.light_min(target)..=0xfff).contains(&object))
        {
            return Err(Error::new(
                "InvalidFormId",
                file,
                o,
                format!(
                    "nonzero full ID; light object {:#x}..=0xFFF",
                    self.light_min(target)
                ),
                format!("{id:#010x}"),
            ));
        }
        Ok(Some(FormKey {
            plugin: self.headers[target].name.to_ascii_lowercase(),
            object_id: object,
        }))
    }
    fn light_min(&self, p: usize) -> u32 {
        let header = &self.headers[p];
        let extended = match header.game {
            fo4_core::Game::Fallout4 => 1.0,
            fo4_core::Game::SkyrimSe => 1.71,
        };
        if header.hedr_version < extended {
            0x800
        } else {
            1
        }
    }
    pub fn runtime_id(&self, key: &FormKey) -> Result<u32> {
        let p = *self
            .names
            .get(&key.plugin.to_ascii_lowercase())
            .ok_or_else(|| {
                Error::new(
                    "MissingMaster",
                    &key.plugin,
                    0,
                    "loaded defining plugin",
                    "absent",
                )
            })?;
        match self.slots[p] {
            Slot::Full(s) => {
                if key.object_id == 0 || key.object_id > 0xffffff {
                    return Err(Error::new(
                        "InvalidFormId",
                        &key.plugin,
                        0,
                        "24-bit nonzero object ID",
                        key.object_id,
                    ));
                }
                Ok((s as u32) << 24 | key.object_id)
            }
            Slot::Light(s) => {
                if !(self.light_min(p)..=0xfff).contains(&key.object_id) {
                    return Err(Error::new(
                        "InvalidFormId",
                        &key.plugin,
                        0,
                        format!("{:#x}..=0xFFF", self.light_min(p)),
                        key.object_id,
                    ));
                }
                Ok(0xfe00_0000 | ((s as u32) << 12) | key.object_id)
            }
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Override {
    /// Upper 32 bits: defining plugin index, lower 32 bits: object ID.
    pub key: u64,
    pub plugin: usize,
    pub offset: u64,
    pub length: u64,
    pub signature: [u8; 4],
    pub deleted: bool,
    pub disabled: bool,
    pub injected: bool,
    pub previous: Option<u32>,
}
pub struct Overrides {
    pub history: Vec<Override>,
    winners: HashMap<u64, u32>,
    names: HashMap<String, u32>,
    complete: HashSet<usize>,
}
impl Default for Overrides {
    fn default() -> Self {
        Self::new()
    }
}
impl Overrides {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            winners: HashMap::new(),
            names: HashMap::new(),
            complete: HashSet::new(),
        }
    }
    pub fn mark_complete(&mut self, plugin: usize, order: &LoadOrder, b: &Budget) -> Result<()> {
        let h = order.headers.get(plugin).ok_or_else(|| {
            Error::new(
                "InvalidPluginIndex",
                "load order",
                0,
                order.headers.len(),
                plugin,
            )
        })?;
        if !self.complete.contains(&plugin) {
            b.charge(32, &h.source.file, 0)?;
            self.complete.try_reserve(1).map_err(|e| {
                Error::new("AllocationFailed", &h.source.file, 0, "completion index", e)
            })?;
            self.complete.insert(plugin);
        }
        Ok(())
    }
    pub fn insert(
        &mut self,
        order: &LoadOrder,
        plugin: usize,
        r: &Record,
        b: &Budget,
    ) -> Result<()> {
        if self.names.is_empty() {
            b.charge(
                (order.headers.len() * 64) as u64,
                &r.source.file,
                r.source.offset,
            )?;
            self.names.try_reserve(order.headers.len()).map_err(|e| {
                Error::new(
                    "AllocationFailed",
                    &r.source.file,
                    r.source.offset,
                    "namespace index",
                    e,
                )
            })?;
            for (i, h) in order.headers.iter().enumerate() {
                b.charge(h.name.len() as u64, &r.source.file, 0)?;
                self.names.insert(h.name.to_ascii_lowercase(), i as u32);
            }
        }
        let key = order
            .remap(plugin, r.form_id, &r.source.file, r.source.offset + 12)?
            .ok_or_else(|| {
                Error::new(
                    "InvalidFormId",
                    &r.source.file,
                    r.source.offset + 12,
                    "non-null record ID",
                    0,
                )
            })?;
        let defining = *self.names.get(&key.plugin).ok_or_else(|| {
            Error::new(
                "MissingMaster",
                &r.source.file,
                r.source.offset + 12,
                &key.plugin,
                "absent",
            )
        })?;
        let compact = ((defining as u64) << 32) | key.object_id as u64;
        let previous = self.winners.get(&compact).copied();
        if let Some(i) = previous {
            let prior = &self.history[i as usize];
            if prior.plugin >= plugin {
                return Err(Error::new(
                    "DuplicateRecord",
                    &r.source.file,
                    r.source.offset,
                    "one record per plugin",
                    r.form_id,
                ));
            }
            if prior.signature != r.signature {
                return Err(Error::new(
                    "OverrideTypeMismatch",
                    &r.source.file,
                    r.source.offset,
                    format!("{:?}", prior.signature),
                    format!("{:?}", r.signature),
                ));
            }
        }
        if previous.is_none()
            && defining as usize != plugin
            && !self.complete.contains(&(defining as usize))
        {
            return Err(Error::new(
                "BaseUnavailable",
                &r.source.file,
                r.source.offset + 12,
                "defining plugin completely indexed before claiming injected",
                &key.plugin,
            ));
        }
        let injected = previous.map_or(defining as usize != plugin, |i| {
            self.history[i as usize].injected
        });
        let index = u32::try_from(self.history.len()).map_err(|e| {
            Error::new(
                "LimitExceeded",
                &r.source.file,
                r.source.offset,
                "u32 history index",
                e,
            )
        })?;
        b.reserve(&mut self.history, 1, &r.source.file, r.source.offset)?;
        if previous.is_none() {
            b.charge(40, &r.source.file, r.source.offset)?;
            self.winners.try_reserve(1).map_err(|e| {
                Error::new(
                    "AllocationFailed",
                    &r.source.file,
                    r.source.offset,
                    "override index",
                    e,
                )
            })?;
        }
        self.winners.insert(compact, index);
        self.history.push(Override {
            key: compact,
            plugin,
            offset: r.source.offset,
            length: r.source.length,
            signature: r.signature,
            deleted: r.deleted(),
            disabled: r.disabled(),
            injected,
            previous,
        });
        Ok(())
    }
    pub fn capacities(&self) -> (usize, usize) {
        (self.history.capacity(), self.winners.capacity())
    }
    pub fn read_winner(
        &self,
        key: &FormKey,
        order: &LoadOrder,
        sources: &[&dyn ReadAt],
        l: &Limits,
        b: &Budget,
    ) -> Result<Option<Record>> {
        let Some(w) = self.winner(key) else {
            return Ok(None);
        };
        let header = order.headers.get(w.plugin).ok_or_else(|| {
            Error::new(
                "InvalidPluginIndex",
                "winner",
                w.offset,
                order.headers.len(),
                w.plugin,
            )
        })?;
        let source = sources.get(w.plugin).ok_or_else(|| {
            Error::new(
                "SourceUnavailable",
                &header.source.file,
                w.offset,
                "source for winning plugin index",
                w.plugin,
            )
        })?;
        if source.name() != header.source.file {
            return Err(Error::new(
                "SourceMismatch",
                source.name(),
                w.offset,
                &header.source.file,
                source.name(),
            ));
        }
        source.check_stable()?;
        let record = record_at(*source, w.offset, source.len(), l, b)?;
        source.check_stable()?;
        Ok(Some(record))
    }
    pub fn form_key(&self, key: u64, order: &LoadOrder) -> FormKey {
        FormKey {
            plugin: order.headers[(key >> 32) as usize]
                .name
                .to_ascii_lowercase(),
            object_id: key as u32,
        }
    }
    pub fn winner_id(&self, key: u64) -> Option<&Override> {
        self.winners
            .get(&key)
            .and_then(|i| self.history.get(*i as usize))
    }
    pub fn winner(&self, key: &FormKey) -> Option<&Override> {
        let defining = *self.names.get(&key.plugin.to_ascii_lowercase())?;
        self.winner_id(((defining as u64) << 32) | key.object_id as u64)
    }
}
#[cfg(test)]
mod tests;
