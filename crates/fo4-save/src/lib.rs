#![forbid(unsafe_code)]
//! Bounded, read-only reader for the save files (`.fos`) of Fallout 4.
//!
//! Every field this reader takes and every rule it enforces was measured over
//! all the save files of one machine; `spec/save.md` gives each one with its
//! count, and `spec/save-census.txt` holds the census aggregates. The overall
//! container layout was recognised from prior general knowledge before it was
//! measured (see `docs/provenance.md`); it is kept only where the census, which
//! could have refuted it, holds. No save editor, format write-up, wiki page,
//! decompilation or game executable was consulted.
//!
//! A file is read front to back and must end exactly where the last part
//! ends; every byte belongs to a framed part. Each offset stored in the
//! location table must equal the position the sequential reading reaches.
//! Parts whose content is not interpreted (the screenshot, the global data
//! bodies, the change-form payloads, the closing block) are kept as spans of
//! the file and read on request. The meaning given to a field (a name, a
//! level, a form reference) is a pending rule in `spec/hypotheses.tsv`,
//! never a fact. A file outside the measured profile is a named error with
//! the file offset; nothing here panics.
use fo4_core::{add, limit, mul, range, Budget, Cursor, Error, Limits, ReadAt, Result};
use serde::Serialize;

mod refid;
pub use refid::{RefId, RefKind};

/// The first twelve bytes of every measured file.
pub const MAGIC: &[u8; 12] = b"FO4_SAVEGAME";
/// Header word 0 in every measured file.
pub const HEADER_VERSION: u32 = 15;
/// Bytes per screenshot pixel: width times height times this is the
/// screenshot's length in every measured file.
pub const SHOT_BYTES_PER_PIXEL: u64 = 4;
/// Reserved words after the counts of the location table, zero in every
/// measured file (kept, not enforced).
pub const RESERVED_WORDS: usize = 15;
/// Bytes of the location table: six offsets, four counts, the reserved words.
pub const LOCATION_TABLE_BYTES: u64 = 4 * (6 + 4 + RESERVED_WORDS as u64);
/// Largest stored count this reader accepts for one table, before the bytes
/// it implies are checked against the file.
pub const MAX_COUNT: u64 = 4_000_000;

/// A range of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}
impl Span {
    pub fn end(&self) -> u64 {
        self.offset.saturating_add(self.length)
    }
}

/// The header block that the header size word frames. Field names are this
/// project's labels for the candidate meanings of `save_header_fields_v1`;
/// only the widths and the framing are measured.
#[derive(Debug, Clone, Serialize)]
pub struct Header {
    /// Header word 0; [`HEADER_VERSION`] in every measured file.
    pub version: u32,
    /// Candidate: the save's number.
    pub save_number: u32,
    /// Candidate: the character's name (text, bytes as stored).
    pub character: Vec<u8>,
    /// Candidate: the character's level.
    pub level: u32,
    /// Candidate: the location's name.
    pub location: Vec<u8>,
    /// Candidate: the play time, as text.
    pub play_time: Vec<u8>,
    /// Candidate: the race's editor id.
    pub race: Vec<u8>,
    /// Candidate: the character's sex (0 or 1).
    pub sex: u16,
    /// Candidate: experience points.
    pub experience: f32,
    /// Candidate: experience points needed for the next level.
    pub next_experience: f32,
    /// Candidate: a Windows file time (100 ns since 1601) of the save.
    pub file_time: u64,
    /// Screenshot width in pixels (measured: frames the screenshot).
    pub shot_width: u32,
    /// Screenshot height in pixels (measured: frames the screenshot).
    pub shot_height: u32,
}

/// The location table that follows the plugin block, in stored order.
#[derive(Debug, Clone, Serialize)]
pub struct Locations {
    /// Offset of the form id array.
    pub form_id_array: u32,
    /// Offset of the closing block.
    pub closing: u32,
    /// Offsets of global data tables 1, 2 and 3.
    pub globals: [u32; 3],
    /// Offset of the change-form table.
    pub change_forms: u32,
    /// Entry counts of global data tables 1, 2 and 3.
    pub global_counts: [u32; 3],
    /// Entry count of the change-form table.
    pub change_form_count: u32,
    /// Fifteen words after the counts, kept as stored.
    pub reserved: [u32; RESERVED_WORDS],
}

/// One entry of a global data table: a type word, a length word and that
/// many bytes, kept as a span.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Global {
    pub kind: u32,
    pub data: Span,
}

/// One entry of the change-form table. The payload stays in the file.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ChangeForm {
    /// Offset of the entry's first byte.
    pub offset: u64,
    /// Three-byte form reference (see [`RefId`]).
    pub form: RefId,
    /// Change-flag word as stored.
    pub flags: u32,
    /// Type byte as stored: [`ChangeForm::kind`] in the low six bits,
    /// [`ChangeForm::width_code`] in the top two.
    pub type_byte: u8,
    /// Version byte as stored.
    pub version: u8,
    /// First length: bytes stored.
    pub length1: u32,
    /// Second length: 0 for a stored payload, otherwise the length of the
    /// zlib stream's output (`save_change_form_zlib_v1`).
    pub length2: u32,
    /// The stored payload.
    pub data: Span,
}
impl ChangeForm {
    /// Low six bits of the type byte.
    pub fn kind(&self) -> u8 {
        self.type_byte & 0x3f
    }
    /// Top two bits of the type byte: 0, 1, 2 select u8, u16, u32 lengths.
    pub fn width_code(&self) -> u8 {
        self.type_byte >> 6
    }
    /// Whether the second length is not zero.
    pub fn compressed(&self) -> bool {
        self.length2 != 0
    }
    /// Bytes of the entry's header: reference, flags, type, version and the
    /// two lengths.
    pub fn header_len(&self) -> u64 {
        9 + 2 * width_bytes(self.width_code()).unwrap_or(0)
    }
    /// The payload: the stored bytes, or the output of the exact zlib stream
    /// when the second length is not zero.
    pub fn payload(&self, source: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Vec<u8>> {
        let stored = fo4_core::read_bytes(source, self.data.offset, self.data.length, l, b)?;
        if !self.compressed() {
            return Ok(stored);
        }
        fo4_core::zlib(
            &stored,
            u64::from(self.length2),
            l,
            b,
            source.name(),
            self.data.offset,
        )
    }
}

fn width_bytes(code: u8) -> Option<u64> {
    match code {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        _ => None,
    }
}

/// Bytes per framed part of a file, in stored order.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Parts {
    /// Magic and header size word.
    pub preamble: u64,
    /// The header block the size word frames.
    pub header: u64,
    pub screenshot: u64,
    /// Form version byte, game version text, plugin block with its size word.
    pub versions_and_plugins: u64,
    pub location_table: u64,
    /// Global data tables 1 and 2 (type and length words included).
    pub globals_1_2: u64,
    /// Change-form entry headers.
    pub change_form_headers: u64,
    /// Change-form payloads as stored.
    pub change_form_payloads: u64,
    /// Global data table 3.
    pub globals_3: u64,
    /// Form id array with its count word.
    pub form_id_array: u64,
    /// Second array with its count word.
    pub second_array: u64,
    pub closing: u64,
}
impl Parts {
    pub fn total(&self) -> u64 {
        [
            self.preamble,
            self.header,
            self.screenshot,
            self.versions_and_plugins,
            self.location_table,
            self.globals_1_2,
            self.change_form_headers,
            self.change_form_payloads,
            self.globals_3,
            self.form_id_array,
            self.second_array,
            self.closing,
        ]
        .iter()
        .sum()
    }
}

/// A whole save file, read to its last byte.
#[derive(Debug, Clone, Serialize)]
pub struct Save {
    pub file: String,
    pub length: u64,
    /// Header size word: bytes of [`Header`] as stored.
    pub header_size: u32,
    pub header: Header,
    pub screenshot: Span,
    /// The byte after the screenshot.
    pub form_version: u8,
    /// The text after the form version (bytes as stored).
    pub game_version: Vec<u8>,
    /// The bytes the plugin size word frames.
    pub plugin_block: Span,
    /// Plugin names, as stored (u8 count).
    pub plugins: Vec<Vec<u8>>,
    /// Second list of plugin names (u16 count) closing the plugin block;
    /// candidate light plugins (`save_plugin_lists_v1`).
    pub light_plugins: Vec<Vec<u8>>,
    pub locations: Locations,
    /// Global data tables 1, 2 and 3.
    pub globals: [Vec<Global>; 3],
    pub change_forms: Vec<ChangeForm>,
    /// The form id array (`save_form_id_array_v1`).
    pub form_ids: Vec<u32>,
    /// The array after the form id array (u32 count, u32 words).
    pub second_array: Vec<u32>,
    /// The closing block: from its declared offset to the end of the file.
    pub closing: Span,
    /// The closing block's bytes, kept raw.
    pub closing_bytes: Vec<u8>,
    pub parts: Parts,
}

fn text(c: &mut Cursor<'_>, l: &Limits, b: &Budget) -> Result<Vec<u8>> {
    let at = c.pos;
    let n = u64::from(c.u16()?);
    limit(n, l.max_string, c.source.name(), at)?;
    c.bytes(n, l, b)
}

fn count(c: &mut Cursor<'_>, l: &Limits, per_item: u64) -> Result<u32> {
    let at = c.pos;
    let n = c.u32()?;
    limit(
        u64::from(n),
        MAX_COUNT.min(l.max_entries),
        c.source.name(),
        at,
    )?;
    range(
        c.pos,
        mul(u64::from(n), per_item, c.source.name(), at)?,
        c.end,
        c.source.name(),
    )?;
    Ok(n)
}

fn expect_at(c: &Cursor<'_>, declared: u32, what: &str) -> Result<()> {
    if u64::from(declared) != c.pos {
        return Err(Error::new(
            "SaveOffsetMismatch",
            c.source.name(),
            c.pos,
            format!("{what} declared at {declared}"),
            format!("sequential reading at {}", c.pos),
        ));
    }
    Ok(())
}

fn globals(c: &mut Cursor<'_>, n: u32, l: &Limits, b: &Budget) -> Result<Vec<Global>> {
    let name = c.source.name().to_owned();
    let mut v = Vec::new();
    b.reserve(&mut v, n as usize, &name, c.pos)?;
    for _ in 0..n {
        let kind = c.u32()?;
        let at = c.pos;
        let length = u64::from(c.u32()?);
        limit(length, l.max_payload, &name, at)?;
        range(c.pos, length, c.end, &name)?;
        v.push(Global {
            kind,
            data: Span {
                offset: c.pos,
                length,
            },
        });
        c.pos = add(c.pos, length, &name, at)?;
    }
    Ok(v)
}

fn u32_array(c: &mut Cursor<'_>, l: &Limits, b: &Budget) -> Result<Vec<u32>> {
    let n = count(c, l, 4)?;
    let mut v = Vec::new();
    b.reserve(&mut v, n as usize, c.source.name(), c.pos)?;
    for _ in 0..n {
        v.push(c.u32()?);
    }
    Ok(v)
}

fn change_form(c: &mut Cursor<'_>, l: &Limits) -> Result<ChangeForm> {
    let offset = c.pos;
    let form = RefId::from_bytes(c.take::<3>()?);
    let flags = c.u32()?;
    let type_byte = c.u8()?;
    let version = c.u8()?;
    let code = type_byte >> 6;
    let lengths = |c: &mut Cursor<'_>| -> Result<u32> {
        Ok(match code {
            0 => u32::from(c.u8()?),
            1 => u32::from(c.u16()?),
            2 => c.u32()?,
            _ => {
                return Err(Error::new(
                    "UnmeasuredChangeFormWidth",
                    c.source.name(),
                    offset + 7,
                    "type byte width code 0, 1 or 2",
                    code,
                ))
            }
        })
    };
    let length1 = lengths(c)?;
    let length2 = lengths(c)?;
    let name = c.source.name();
    limit(u64::from(length1), l.max_payload, name, offset)?;
    limit(u64::from(length2), l.max_payload, name, offset)?;
    range(c.pos, u64::from(length1), c.end, name)?;
    let data = Span {
        offset: c.pos,
        length: u64::from(length1),
    };
    c.pos = add(c.pos, data.length, name, offset)?;
    Ok(ChangeForm {
        offset,
        form,
        flags,
        type_byte,
        version,
        length1,
        length2,
        data,
    })
}

impl Save {
    /// Reads the whole file. Payloads stay in the source; only the tables
    /// are kept.
    pub fn read(source: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        source.check_stable()?;
        let save = Self::read_inside(source, l, b)?;
        source.check_stable()?;
        Ok(save)
    }

    fn read_inside(source: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        let name = source.name().to_owned();
        let length = source.len();
        let mut c = Cursor::new(source);
        let mut parts = Parts::default();
        let magic = c.take::<12>()?;
        if &magic != MAGIC {
            return Err(Error::new(
                "SaveMagic",
                &name,
                0,
                "FO4_SAVEGAME",
                String::from_utf8_lossy(&magic),
            ));
        }
        let header_size = c.u32()?;
        parts.preamble = c.pos;
        let header_start = c.pos;
        let header_end = add(header_start, u64::from(header_size), &name, 12)?;
        range(header_start, u64::from(header_size), length, &name)?;
        let mut h = Cursor::at(source, header_start, header_end)?;
        let version = h.u32()?;
        if version != HEADER_VERSION {
            return Err(Error::new(
                "UnmeasuredSaveVersion",
                &name,
                header_start,
                HEADER_VERSION,
                version,
            ));
        }
        let header = Header {
            version,
            save_number: h.u32()?,
            character: text(&mut h, l, b)?,
            level: h.u32()?,
            location: text(&mut h, l, b)?,
            play_time: text(&mut h, l, b)?,
            race: text(&mut h, l, b)?,
            sex: h.u16()?,
            experience: h.f32()?,
            next_experience: h.f32()?,
            file_time: h.u64()?,
            shot_width: h.u32()?,
            shot_height: h.u32()?,
        };
        if h.pos != header_end {
            return Err(Error::new(
                "SaveHeaderSize",
                &name,
                h.pos,
                format!("header fields end at {header_end}"),
                h.pos,
            ));
        }
        c.pos = header_end;
        parts.header = u64::from(header_size);
        let shot_len = mul(
            mul(
                u64::from(header.shot_width),
                u64::from(header.shot_height),
                &name,
                header_end,
            )?,
            SHOT_BYTES_PER_PIXEL,
            &name,
            header_end,
        )?;
        limit(shot_len, l.max_payload, &name, header_end)?;
        range(c.pos, shot_len, length, &name)?;
        let screenshot = Span {
            offset: c.pos,
            length: shot_len,
        };
        c.pos = screenshot.end();
        parts.screenshot = shot_len;

        let versions_start = c.pos;
        let form_version = c.u8()?;
        let game_version = text(&mut c, l, b)?;
        let block_size = c.u32()?;
        let plugin_block = Span {
            offset: c.pos,
            length: u64::from(block_size),
        };
        range(plugin_block.offset, plugin_block.length, length, &name)?;
        let mut p = Cursor::at(source, plugin_block.offset, plugin_block.end())?;
        let n = p.u8()?;
        let mut plugins = Vec::new();
        b.reserve(&mut plugins, usize::from(n), &name, p.pos)?;
        for _ in 0..n {
            plugins.push(text(&mut p, l, b)?);
        }
        let n = p.u16()?;
        let mut light_plugins = Vec::new();
        b.reserve(&mut light_plugins, usize::from(n), &name, p.pos)?;
        for _ in 0..n {
            light_plugins.push(text(&mut p, l, b)?);
        }
        if p.pos != plugin_block.end() {
            return Err(Error::new(
                "SavePluginBlockSize",
                &name,
                p.pos,
                format!("plugin lists end at {}", plugin_block.end()),
                p.pos,
            ));
        }
        c.pos = plugin_block.end();
        parts.versions_and_plugins = c.pos - versions_start;

        let table_start = c.pos;
        let mut w = [0u32; 10];
        for x in w.iter_mut() {
            *x = c.u32()?;
        }
        let mut reserved = [0u32; RESERVED_WORDS];
        for x in reserved.iter_mut() {
            *x = c.u32()?;
        }
        let locations = Locations {
            form_id_array: w[0],
            closing: w[1],
            globals: [w[2], w[3], w[5]],
            change_forms: w[4],
            global_counts: [w[6], w[7], w[8]],
            change_form_count: w[9],
            reserved,
        };
        parts.location_table = c.pos - table_start;
        for (i, n) in locations
            .global_counts
            .iter()
            .chain([&locations.change_form_count])
            .enumerate()
        {
            limit(
                u64::from(*n),
                MAX_COUNT.min(l.max_entries),
                &name,
                table_start + 24 + 4 * i as u64,
            )?;
        }

        let g12_start = c.pos;
        expect_at(&c, locations.globals[0], "global data table 1")?;
        let g1 = globals(&mut c, locations.global_counts[0], l, b)?;
        expect_at(&c, locations.globals[1], "global data table 2")?;
        let g2 = globals(&mut c, locations.global_counts[1], l, b)?;
        parts.globals_1_2 = c.pos - g12_start;

        expect_at(&c, locations.change_forms, "change-form table")?;
        let mut change_forms = Vec::new();
        b.reserve(
            &mut change_forms,
            locations.change_form_count as usize,
            &name,
            c.pos,
        )?;
        for _ in 0..locations.change_form_count {
            let f = change_form(&mut c, l)?;
            parts.change_form_headers += f.header_len();
            parts.change_form_payloads += f.data.length;
            change_forms.push(f);
        }

        let g3_start = c.pos;
        expect_at(&c, locations.globals[2], "global data table 3")?;
        let g3 = globals(&mut c, locations.global_counts[2], l, b)?;
        parts.globals_3 = c.pos - g3_start;

        let a = c.pos;
        expect_at(&c, locations.form_id_array, "form id array")?;
        let form_ids = u32_array(&mut c, l, b)?;
        parts.form_id_array = c.pos - a;
        let a = c.pos;
        let second_array = u32_array(&mut c, l, b)?;
        parts.second_array = c.pos - a;

        expect_at(&c, locations.closing, "closing block")?;
        let closing = Span {
            offset: c.pos,
            length: length - c.pos,
        };
        let closing_bytes = c.bytes(closing.length, l, b)?;
        parts.closing = closing.length;
        Ok(Self {
            file: name,
            length,
            header_size,
            header,
            screenshot,
            form_version,
            game_version,
            plugin_block,
            plugins,
            light_plugins,
            locations,
            globals: [g1, g2, g3],
            change_forms,
            form_ids,
            second_array,
            closing,
            closing_bytes,
            parts,
        })
    }

    /// The runtime form id a reference names under `save_ref_id_kinds_v1`:
    /// kind 0 is a 1-based index into the form id array (0 names nothing),
    /// kind 1 the value itself (a form of the first plugin), kind 2 the value
    /// with `0xFF` as its top byte. Kind 3 and an index past the array give
    /// `None`.
    pub fn resolve(&self, r: RefId) -> Option<u32> {
        match r.kind() {
            RefKind::Indexed => {
                let i = r.value() as usize;
                i.checked_sub(1).and_then(|i| self.form_ids.get(i)).copied()
            }
            RefKind::Direct => Some(r.value()),
            RefKind::Created => Some(0xff00_0000 | r.value()),
            RefKind::Other => None,
        }
    }
}

#[cfg(test)]
mod tests;
