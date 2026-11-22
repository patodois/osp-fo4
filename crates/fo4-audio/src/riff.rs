//! RIFF chunking (`spec/audio.md`, "Containers").
//!
//! A file is `RIFF`, a u32 little-endian size, a four-byte form type and a
//! sequence of chunks (four-byte id, u32 size, that many bytes, one pad byte
//! after an odd size). This framing is a public container convention
//! recognised from prior knowledge (`docs/provenance.md`,
//! `audio_riff_framing_v1`); the reader keeps it only as far as the census
//! of `spec/audio-census.txt` holds it. The reader is strict: the declared
//! size must frame the whole file, every chunk must end inside it and the
//! chunks must reach its last byte, or the file is refused with a named
//! error.
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// The first four bytes of every RIFF file.
pub const RIFF: [u8; 4] = *b"RIFF";
/// Largest number of chunks one sequence may hold; the census finds at most
/// a handful per file.
pub const MAX_CHUNKS: usize = 4096;

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
    /// The bytes of the span inside `bytes`; `None` when it does not fit.
    pub fn of<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        let s = usize::try_from(self.offset).ok()?;
        let e = usize::try_from(self.end()).ok()?;
        bytes.get(s..e)
    }
}

/// One chunk: its id, the span of its payload and its pad byte when the
/// payload length is odd.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Chunk {
    pub id: [u8; 4],
    /// Offset of the chunk's id.
    pub header: u64,
    pub data: Span,
    pub pad: Option<u8>,
}

/// A RIFF file read to its last byte.
#[derive(Debug, Clone, Serialize)]
pub struct Riff {
    pub form: [u8; 4],
    /// The stored size word: the file length minus eight.
    pub declared: u32,
    pub chunks: Vec<Chunk>,
}

fn u32_at(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]])
}

/// Reads a sequence of chunks that must fill `bytes[start..end]` exactly.
/// Offsets are relative to `bytes`.
pub fn chunks(bytes: &[u8], start: u64, end: u64, file: &str, b: &Budget) -> Result<Vec<Chunk>> {
    if end > bytes.len() as u64 || start > end {
        return Err(Error::new(
            "ChunkRange",
            file,
            start,
            format!("range inside {} bytes", bytes.len()),
            end,
        ));
    }
    let mut out = Vec::new();
    let mut pos = start;
    while pos < end {
        if end - pos < 8 {
            return Err(Error::new(
                "ChunkHeaderTruncated",
                file,
                pos,
                "8-byte chunk header",
                end - pos,
            ));
        }
        let p = pos as usize;
        let id = [bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]];
        let size = u32_at(bytes, p + 4);
        let data = Span {
            offset: pos + 8,
            length: u64::from(size),
        };
        if data.end() > end {
            return Err(Error::new(
                "ChunkPastEnd",
                file,
                pos,
                format!("chunk end <= {end}"),
                data.end(),
            ));
        }
        let mut next = data.end();
        let mut pad = None;
        if size % 2 == 1 {
            if next >= end {
                return Err(Error::new(
                    "ChunkPadMissing",
                    file,
                    next,
                    "pad byte after odd chunk",
                    "end of range",
                ));
            }
            pad = Some(bytes[next as usize]);
            next += 1;
        }
        if out.len() >= MAX_CHUNKS {
            return Err(Error::new(
                "LimitExceeded",
                file,
                pos,
                MAX_CHUNKS,
                "more chunks",
            ));
        }
        b.reserve(&mut out, 1, file, pos)?;
        out.push(Chunk {
            id,
            header: pos,
            data,
            pad,
        });
        pos = next;
    }
    Ok(out)
}

impl Riff {
    /// Reads the chunk list of a whole file held in `bytes`.
    pub fn read(bytes: &[u8], file: &str, b: &Budget) -> Result<Self> {
        let len = bytes.len() as u64;
        if bytes.get(..4) != Some(&RIFF[..]) {
            return Err(Error::new(
                "RiffMagic",
                file,
                0,
                "RIFF",
                fo4_core::HexBytes(&bytes[..bytes.len().min(4)]),
            ));
        }
        if len < 12 {
            return Err(Error::new("RiffTruncated", file, 0, ">= 12 bytes", len));
        }
        let declared = u32_at(bytes, 4);
        let end = u64::from(declared) + 8;
        if end != len {
            return Err(Error::new(
                "RiffLength",
                file,
                4,
                format!("size word + 8 == file length {len}"),
                end,
            ));
        }
        let form = [bytes[8], bytes[9], bytes[10], bytes[11]];
        let chunks = chunks(bytes, 12, end, file, b)?;
        Ok(Self {
            form,
            declared,
            chunks,
        })
    }

    /// The one chunk with this id; an error when absent or repeated.
    pub fn only(&self, id: &[u8; 4], file: &str) -> Result<&Chunk> {
        let mut it = self.chunks.iter().filter(|c| &c.id == id);
        let first = it.next().ok_or_else(|| {
            Error::new(
                "ChunkMissing",
                file,
                0,
                String::from_utf8_lossy(id),
                "absent",
            )
        })?;
        if let Some(second) = it.next() {
            return Err(Error::new(
                "ChunkRepeated",
                file,
                second.header,
                format!("one {}", String::from_utf8_lossy(id)),
                "two or more",
            ));
        }
        Ok(first)
    }

    /// Bytes of framing: the twelve-byte file header, eight per chunk header
    /// and the pad bytes.
    pub fn framing_bytes(&self) -> u64 {
        12 + self
            .chunks
            .iter()
            .map(|c| 8 + u64::from(c.pad.is_some()))
            .sum::<u64>()
    }
}
