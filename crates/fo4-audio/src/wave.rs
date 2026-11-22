//! `WAVE` and `XWMA` forms (`spec/audio.md`, "Containers").
//!
//! Both are RIFF files with exactly one `fmt ` chunk and one `data` chunk.
//! An `XWMA` file also holds exactly one `dpds` chunk, read as u32 words
//! (`audio_xwma_dpds_v1`). Every other chunk is kept as a span; the views
//! below ([`Smpl`], [`Cue`], [`list`]) are candidate layouts recognised from
//! prior knowledge that the census applies to every chunk of their id and
//! counts, and nothing in the reader depends on them.
use crate::format::WaveFormat;
use crate::riff::{self, Chunk, Riff, Span};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Form type of a plain wave file.
pub const FORM_WAVE: [u8; 4] = *b"WAVE";
/// Form type of the file whose `fmt ` names a WMA-family codec.
pub const FORM_XWMA: [u8; 4] = *b"XWMA";
/// Largest seek table this reader keeps, in entries.
pub const MAX_DPDS: u64 = 16_000_000;

/// A `WAVE` or `XWMA` file read to its last byte.
#[derive(Debug, Clone, Serialize)]
pub struct Wave {
    pub riff: Riff,
    pub format: WaveFormat,
    /// Index of the `fmt ` chunk in `riff.chunks`.
    pub format_chunk: usize,
    /// The `data` chunk's payload.
    pub data: Span,
    /// Index of the `data` chunk in `riff.chunks`.
    pub data_chunk: usize,
    /// The `dpds` words of an `XWMA` file; empty for `WAVE`.
    pub dpds: Vec<u32>,
}

fn index_of(r: &Riff, id: &[u8; 4], file: &str) -> Result<usize> {
    let c = r.only(id, file)?;
    Ok(r.chunks
        .iter()
        .position(|x| x.header == c.header)
        .unwrap_or_default())
}

impl Wave {
    /// Reads a whole `WAVE` or `XWMA` file.
    pub fn read(bytes: &[u8], file: &str, b: &Budget) -> Result<Self> {
        let riff = Riff::read(bytes, file, b)?;
        if riff.form != FORM_WAVE && riff.form != FORM_XWMA {
            return Err(Error::new(
                "RiffForm",
                file,
                8,
                "WAVE or XWMA",
                fo4_core::HexBytes(&riff.form),
            ));
        }
        let format_chunk = index_of(&riff, b"fmt ", file)?;
        let fc = riff.chunks[format_chunk];
        let format =
            WaveFormat::parse(fc.data.of(bytes).unwrap_or_default(), file, fc.data.offset)?;
        let data_chunk = index_of(&riff, b"data", file)?;
        let data = riff.chunks[data_chunk].data;
        let mut dpds = Vec::new();
        if riff.form == FORM_XWMA {
            let c = riff.only(b"dpds", file)?;
            if c.data.length % 4 != 0 {
                return Err(Error::new(
                    "DpdsSize",
                    file,
                    c.header,
                    "multiple of 4",
                    c.data.length,
                ));
            }
            let n = c.data.length / 4;
            fo4_core::limit(n, MAX_DPDS, file, c.header)?;
            b.reserve(&mut dpds, n as usize, file, c.header)?;
            dpds.extend(
                c.data
                    .of(bytes)
                    .unwrap_or_default()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|w| u32::from_le_bytes(*w)),
            );
        } else if riff.chunks.iter().any(|c| &c.id == b"dpds") {
            return Err(Error::new(
                "DpdsInWave",
                file,
                0,
                "no dpds chunk in a WAVE form",
                "dpds",
            ));
        }
        Ok(Self {
            riff,
            format,
            format_chunk,
            data,
            data_chunk,
            dpds,
        })
    }

    /// Chunks other than `fmt `, `data` and `dpds`.
    pub fn extra_chunks(&self) -> impl Iterator<Item = &Chunk> {
        self.riff
            .chunks
            .iter()
            .filter(|c| !matches!(&c.id, b"fmt " | b"data" | b"dpds"))
    }
}

/// One loop of a [`Smpl`] chunk: six u32 words as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SmplLoop {
    pub cue_id: u32,
    pub kind: u32,
    pub start: u32,
    pub end: u32,
    pub fraction: u32,
    pub play_count: u32,
}

/// Candidate `smpl` layout (`audio_wave_smpl_v1`): nine u32 header words,
/// the eighth a loop count and the ninth a byte count of trailing data,
/// then that many loops of six words and those bytes, ending the chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Smpl {
    pub header: [u32; 9],
    pub loops: Vec<SmplLoop>,
    pub trailing: Vec<u8>,
}

fn words<const N: usize>(d: &[u8], at: usize) -> Option<[u32; N]> {
    let mut out = [0u32; N];
    for (i, w) in out.iter_mut().enumerate() {
        let p = at + 4 * i;
        *w = u32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?);
    }
    Some(out)
}

impl Smpl {
    /// Parses a `smpl` payload; `None` when the candidate does not frame it
    /// exactly.
    pub fn parse(d: &[u8]) -> Option<Self> {
        let header: [u32; 9] = words(d, 0)?;
        let n = header[7] as usize;
        let extra = header[8] as usize;
        let need = 36usize
            .checked_add(n.checked_mul(24)?)?
            .checked_add(extra)?;
        if need != d.len() {
            return None;
        }
        let loops = (0..n)
            .map(|i| {
                let w: [u32; 6] = words(d, 36 + 24 * i)?;
                Some(SmplLoop {
                    cue_id: w[0],
                    kind: w[1],
                    start: w[2],
                    end: w[3],
                    fraction: w[4],
                    play_count: w[5],
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            header,
            loops,
            trailing: d[36 + 24 * n..].to_vec(),
        })
    }
}

/// Candidate `cue ` layout (`audio_wave_cue_v1`): a u32 count and that many
/// points of six u32 words, ending the chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cue {
    pub points: Vec<[u32; 6]>,
}
impl Cue {
    pub fn parse(d: &[u8]) -> Option<Self> {
        let [n]: [u32; 1] = words(d, 0)?;
        let n = n as usize;
        if 4usize.checked_add(n.checked_mul(24)?)? != d.len() {
            return None;
        }
        let points = (0..n)
            .map(|i| words::<6>(d, 4 + 24 * i))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { points })
    }
}

/// Candidate `LIST` layout (`audio_wave_list_v1`): a four-byte list type and
/// a chunk sequence that fills the rest of the payload exactly. Returns the
/// list type and the sub-chunks with offsets relative to `d`.
pub fn list(d: &[u8], b: &Budget) -> Option<([u8; 4], Vec<Chunk>)> {
    let kind: [u8; 4] = d.get(..4)?.try_into().ok()?;
    let sub = riff::chunks(d, 4, d.len() as u64, "LIST", b).ok()?;
    Some((kind, sub))
}
