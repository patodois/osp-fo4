#![forbid(unsafe_code)]
//! Bounded, read-only reader for the sound files of Fallout 4.
//!
//! The archives hold three kinds of sound file, told apart by their first
//! bytes: RIFF `WAVE` files, RIFF `XWMA` files and the voice container
//! `FUZE`, which carries a lip half and an `XWMA` audio half. [`Sound::read`]
//! reads any of them to the file's last byte: every byte belongs to a framed
//! part ([`Parts`]) or the file is refused with a named error.
//!
//! Integer PCM is decoded ([`pcm::decode`]) from the container's own
//! declared parameters. The WMA-family payload of `XWMA` files is not
//! decoded: its packets are framed by the block align and counted against
//! the `dpds` table, and the bytes stay raw. No ADPCM occurs in the measured
//! installation, so none is decoded.
//!
//! The container layouts were recognised from prior general knowledge before
//! they were measured (see `docs/provenance.md`); each is a pending rule in
//! `spec/hypotheses.tsv`, kept only as far as `spec/audio-census.txt` holds
//! it. No decoder, tool, format write-up, wiki page or game executable was
//! consulted.
pub mod format;
pub mod fuz;
pub mod pcm;
pub mod riff;
pub mod wave;

#[cfg(test)]
mod tests;

use fo4_core::{Budget, Error, Result};
use fuz::Fuz;
use riff::Span;
use serde::Serialize;
use wave::{Wave, FORM_WAVE, FORM_XWMA};

/// Which container a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Container {
    Wave,
    Xwma,
    /// The voice container; its audio half is an `XWMA` or `WAVE` file.
    Fuz,
}
impl Container {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wave => "RIFF WAVE",
            Self::Xwma => "RIFF XWMA",
            Self::Fuz => "FUZE",
        }
    }
    /// The container a file's first bytes name; `None` for any other file.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..4) == Some(&fuz::MAGIC[..]) {
            return Some(Self::Fuz);
        }
        if bytes.get(..4) != Some(&riff::RIFF[..]) {
            return None;
        }
        match bytes.get(8..12) {
            Some(f) if f == FORM_WAVE => Some(Self::Wave),
            Some(f) if f == FORM_XWMA => Some(Self::Xwma),
            _ => None,
        }
    }
}

/// Bytes of a file by part; [`Parts::total`] equals the file length for
/// every file [`Sound::read`] accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Parts {
    /// Voice container header (twelve bytes).
    pub fuz_header: u64,
    /// Lip half of a voice container, raw.
    pub lip: u64,
    /// RIFF header, chunk headers and pad bytes.
    pub framing: u64,
    /// `fmt ` payload.
    pub format: u64,
    /// `dpds` payload.
    pub seek: u64,
    /// `data` payload.
    pub data: u64,
    /// Payloads of every other chunk, raw.
    pub other: u64,
}
impl Parts {
    pub fn total(&self) -> u64 {
        self.fuz_header + self.lip + self.framing + self.format + self.seek + self.data + self.other
    }
}

/// A sound file read to its last byte.
#[derive(Debug, Clone, Serialize)]
pub struct Sound {
    pub container: Container,
    /// The voice container split, for [`Container::Fuz`].
    pub fuz: Option<Fuz>,
    /// The RIFF file: the whole file, or the audio half of a voice
    /// container. Its spans are relative to [`Sound::audio_offset`].
    pub wave: Wave,
    /// Offset of the RIFF file inside the file.
    pub audio_offset: u64,
    pub parts: Parts,
}

impl Sound {
    /// Reads a whole sound file held in `bytes`.
    pub fn read(bytes: &[u8], file: &str, b: &Budget) -> Result<Self> {
        let container = Container::sniff(bytes).ok_or_else(|| {
            Error::new(
                "SoundMagic",
                file,
                0,
                "RIFF WAVE, RIFF XWMA or FUZE",
                fo4_core::HexBytes(&bytes[..bytes.len().min(12)]),
            )
        })?;
        let (fuz, audio) = match container {
            Container::Fuz => {
                let f = Fuz::read(bytes, file)?;
                (Some(f), f.audio)
            }
            _ => (
                None,
                Span {
                    offset: 0,
                    length: bytes.len() as u64,
                },
            ),
        };
        let audio_bytes = audio.of(bytes).unwrap_or_default();
        let wave = Wave::read(audio_bytes, file, b).map_err(|mut e| {
            e.source.offset += audio.offset;
            e
        })?;
        let mut parts = Parts {
            framing: wave.riff.framing_bytes(),
            format: wave.riff.chunks[wave.format_chunk].data.length,
            data: wave.data.length,
            ..Parts::default()
        };
        for c in &wave.riff.chunks {
            match &c.id {
                b"dpds" => parts.seek += c.data.length,
                b"fmt " | b"data" => {}
                _ => parts.other += c.data.length,
            }
        }
        if let Some(f) = &fuz {
            parts.fuz_header = fuz::HEADER_BYTES;
            parts.lip = f.lip.length;
        }
        if parts.total() != bytes.len() as u64 {
            return Err(Error::new(
                "SoundUnframedBytes",
                file,
                0,
                bytes.len(),
                parts.total(),
            ));
        }
        Ok(Self {
            container,
            fuz,
            wave,
            audio_offset: audio.offset,
            parts,
        })
    }

    /// The `data` payload inside the whole file's bytes.
    pub fn data<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        Span {
            offset: self.audio_offset + self.wave.data.offset,
            length: self.wave.data.length,
        }
        .of(bytes)
    }

    /// Whether this crate decodes the file's codec.
    pub fn status(&self) -> pcm::Status {
        pcm::status(&self.wave.format)
    }

    /// Decodes a PCM file; a named error for any other codec.
    pub fn decode(&self, bytes: &[u8], file: &str, b: &Budget) -> Result<pcm::Pcm> {
        let off = self.audio_offset + self.wave.data.offset;
        let data = self
            .data(bytes)
            .ok_or_else(|| Error::new("SoundSpan", file, off, "data inside the file", "outside"))?;
        pcm::decode(&self.wave.format, data, file, off, b)
    }
}

/// Packets of a WMA-family `data` chunk: its length divided by the block
/// align, when it divides exactly (`audio_xwma_packets_v1`).
pub fn packets(w: &Wave) -> Option<u64> {
    let a = u64::from(w.format.block_align);
    (a > 0 && w.data.length.is_multiple_of(a)).then(|| w.data.length / a)
}

/// Seconds of audio a file's header declares, without decoding
/// (`spec/voice.md`, Duration). For integer PCM: whole frames of the
/// `data` chunk over the sample rate (`audio_pcm_layout_v1`). For the
/// WMA family: the last `dpds` word read as the count of decoded 16-bit
/// bytes up to the end of the last packet, over 2 x channels x sample
/// rate (`audio_xwma_dpds_seconds_v1`, pending: the census supports it
/// against the stored byte rate, nothing decoded confirms it). `None`
/// when the fields needed are zero or absent, or for another tag.
pub fn header_seconds(w: &Wave) -> Option<f64> {
    let f = &w.format;
    let rate = f64::from(f.sample_rate);
    if rate <= 0.0 || f.channels == 0 {
        return None;
    }
    match f.tag {
        format::TAG_PCM => {
            let a = u64::from(f.block_align);
            (a > 0).then(|| (w.data.length / a) as f64 / rate)
        }
        format::TAG_WMA2 => {
            let last = f64::from(*w.dpds.last()?);
            Some(last / (2.0 * f64::from(f.channels) * rate))
        }
        _ => None,
    }
}
