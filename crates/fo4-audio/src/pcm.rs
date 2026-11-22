//! Integer PCM: decoding from the container's declared parameters and
//! writing a plain wave file (`spec/audio.md`, "Decoding").
//!
//! Candidate reading (`audio_pcm_layout_v1`, prior knowledge): format tag 1;
//! frames of `block_align` bytes, one sample per channel in channel order,
//! each `bits / 8` bytes of little-endian two's complement. Only the widths
//! the census measured (16 and 24 bits) are decoded; any other width, an
//! extension, or a block align or data length that does not fit is a named
//! error.
use crate::format::{WaveFormat, TAG_PCM, TAG_WMA2, TAG_WMA3};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Sample widths this crate decodes, all measured in the installation.
pub const BITS: [u16; 2] = [16, 24];
/// Largest channel count accepted.
pub const MAX_CHANNELS: u16 = 8;

/// Whether a format is decoded by this crate, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Status {
    /// Integer PCM of a measured width: [`decode`] reads it.
    Pcm,
    /// A WMA-family codec: the packets are counted, the payload stays raw.
    WmaOpen,
    /// Any other format: not decoded.
    Unsupported,
}

/// What this crate does with a format.
pub fn status(f: &WaveFormat) -> Status {
    match f.tag {
        TAG_PCM if f.extension.is_none() && BITS.contains(&f.bits) => Status::Pcm,
        TAG_WMA2 | TAG_WMA3 => Status::WmaOpen,
        _ => Status::Unsupported,
    }
}

/// Decoded samples, interleaved by channel, each sign-extended to i32.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
    pub samples: Vec<i32>,
}

impl Pcm {
    /// Frames (one sample per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
}

/// Bytes per frame that a PCM format implies.
fn frame_bytes(f: &WaveFormat, file: &str, offset: u64) -> Result<usize> {
    if f.tag != TAG_PCM {
        return Err(Error::new("PcmTag", file, offset, TAG_PCM, f.tag));
    }
    if f.extension.is_some() {
        return Err(Error::new(
            "PcmExtension",
            file,
            offset,
            "16-byte fmt chunk",
            "extension",
        ));
    }
    if !BITS.contains(&f.bits) {
        return Err(Error::new("PcmBits", file, offset, "16 or 24", f.bits));
    }
    if f.channels == 0 || f.channels > MAX_CHANNELS {
        return Err(Error::new(
            "PcmChannels",
            file,
            offset,
            format!("1..={MAX_CHANNELS}"),
            f.channels,
        ));
    }
    let frame = usize::from(f.channels) * usize::from(f.bits / 8);
    if usize::from(f.block_align) != frame {
        return Err(Error::new(
            "PcmBlockAlign",
            file,
            offset,
            frame,
            f.block_align,
        ));
    }
    Ok(frame)
}

/// Decodes the payload of a `data` chunk under its `fmt ` chunk.
pub fn decode(f: &WaveFormat, data: &[u8], file: &str, offset: u64, b: &Budget) -> Result<Pcm> {
    let frame = frame_bytes(f, file, offset)?;
    if !data.len().is_multiple_of(frame) {
        return Err(Error::new(
            "PcmPartialFrame",
            file,
            offset,
            format!("data length a multiple of {frame}"),
            data.len(),
        ));
    }
    let width = usize::from(f.bits / 8);
    let n = data.len() / width;
    let mut samples = Vec::new();
    b.reserve(&mut samples, n, file, offset)?;
    match width {
        2 => samples.extend(
            data.as_chunks::<2>()
                .0
                .iter()
                .map(|s| i32::from(i16::from_le_bytes(*s))),
        ),
        _ => samples.extend(
            data.as_chunks::<3>()
                .0
                .iter()
                .map(|s| i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8),
        ),
    }
    Ok(Pcm {
        channels: f.channels,
        sample_rate: f.sample_rate,
        bits: f.bits,
        samples,
    })
}

/// Writes `p` as a `WAVE` file with a 16-byte `fmt ` chunk and one `data`
/// chunk, at its own sample width.
pub fn encode_wav(p: &Pcm, b: &Budget) -> Result<Vec<u8>> {
    const FILE: &str = "wav writer";
    let f = WaveFormat {
        tag: TAG_PCM,
        channels: p.channels,
        sample_rate: p.sample_rate,
        avg_bytes_per_sec: 0,
        block_align: p.channels.saturating_mul(p.bits / 8),
        bits: p.bits,
        extension: None,
    };
    let frame = frame_bytes(&f, FILE, 0)?;
    if !p.samples.len().is_multiple_of(usize::from(p.channels)) {
        return Err(Error::new(
            "PcmPartialFrame",
            FILE,
            0,
            "whole frames",
            p.samples.len(),
        ));
    }
    let width = usize::from(p.bits / 8);
    let (lo, hi) = if width == 2 {
        (i32::from(i16::MIN), i32::from(i16::MAX))
    } else {
        (-(1 << 23), (1 << 23) - 1)
    };
    if let Some(s) = p.samples.iter().find(|s| !(lo..=hi).contains(*s)) {
        return Err(Error::new(
            "PcmSampleRange",
            FILE,
            0,
            format!("{lo}..={hi}"),
            s,
        ));
    }
    let data_len = p.samples.len() * width;
    let data_u32 = u32::try_from(data_len)
        .ok()
        .filter(|n| n.checked_add(36 + 1).is_some())
        .ok_or_else(|| Error::new("WavTooLarge", FILE, 0, "< 4 GiB", data_len))?;
    let avg = u32::try_from(frame)
        .ok()
        .and_then(|fr| fr.checked_mul(p.sample_rate))
        .ok_or_else(|| Error::new("Overflow", FILE, 0, "rate * frame", p.sample_rate))?;
    let pad = data_len % 2;
    let total = 44 + data_len + pad;
    let mut out = Vec::new();
    b.reserve(&mut out, total, FILE, 0)?;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((total - 8) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&TAG_PCM.to_le_bytes());
    out.extend_from_slice(&p.channels.to_le_bytes());
    out.extend_from_slice(&p.sample_rate.to_le_bytes());
    out.extend_from_slice(&avg.to_le_bytes());
    out.extend_from_slice(&f.block_align.to_le_bytes());
    out.extend_from_slice(&p.bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_u32.to_le_bytes());
    for s in &p.samples {
        out.extend_from_slice(&s.to_le_bytes()[..width]);
    }
    if pad == 1 {
        out.push(0);
    }
    Ok(out)
}
