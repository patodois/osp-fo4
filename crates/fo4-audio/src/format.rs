//! The `fmt ` chunk (`spec/audio.md`, "The format chunk").
//!
//! Six fields in stored order (u16 format tag, u16 channels, u32 sample
//! rate, u32 average bytes per second, u16 block align, u16 bits per
//! sample), then, when the chunk is longer than 16 bytes, a u16 count and
//! that many extension bytes that must end the chunk. This is the public
//! wave-format layout recognised from prior knowledge
//! (`docs/provenance.md`); the census checks every relation the code uses.
use fo4_core::{Error, Result};
use serde::Serialize;

/// Format tag of plain integer PCM (prior knowledge, `audio_format_tags_v1`).
pub const TAG_PCM: u16 = 1;
/// Format tag of the ADPCM variant whose coefficient table the extension
/// carries (prior knowledge, `audio_format_tags_v1`).
pub const TAG_ADPCM_COEF: u16 = 2;
/// Format tag of the IMA-style ADPCM variant (prior knowledge).
pub const TAG_ADPCM_IMA: u16 = 0x11;
/// Format tags of the WMA family (prior knowledge, `audio_format_tags_v1`).
pub const TAG_WMA2: u16 = 0x161;
pub const TAG_WMA3: u16 = 0x162;

/// Name of a format tag for reports; `None` for a tag this crate has no
/// label for.
pub fn tag_name(tag: u16) -> Option<&'static str> {
    Some(match tag {
        TAG_PCM => "pcm",
        TAG_ADPCM_COEF => "adpcm-coef",
        TAG_ADPCM_IMA => "adpcm-ima",
        3 => "ieee-float",
        TAG_WMA2 => "wma2",
        TAG_WMA3 => "wma3",
        0xfffe => "extensible",
        _ => return None,
    })
}

/// A parsed `fmt ` chunk with its extension kept as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WaveFormat {
    pub tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub avg_bytes_per_sec: u32,
    pub block_align: u16,
    pub bits: u16,
    /// The extension count word and bytes; `None` for a 16-byte chunk.
    pub extension: Option<Vec<u8>>,
}

impl WaveFormat {
    /// Parses the payload of a `fmt ` chunk, which must be consumed exactly.
    pub fn parse(data: &[u8], file: &str, offset: u64) -> Result<Self> {
        if data.len() < 16 {
            return Err(Error::new(
                "FormatTruncated",
                file,
                offset,
                ">= 16 bytes",
                data.len(),
            ));
        }
        let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let u32_at =
            |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let extension = match data.len() {
            16 => None,
            17 => {
                return Err(Error::new(
                    "FormatExtension",
                    file,
                    offset + 16,
                    "no extension or a u16 count",
                    "one byte",
                ))
            }
            n => {
                let count = usize::from(u16_at(16));
                if 18 + count != n {
                    return Err(Error::new(
                        "FormatExtension",
                        file,
                        offset + 16,
                        format!("extension count {} == chunk bytes after it", n - 18),
                        count,
                    ));
                }
                Some(data[18..].to_vec())
            }
        };
        Ok(Self {
            tag: u16_at(0),
            channels: u16_at(2),
            sample_rate: u32_at(4),
            avg_bytes_per_sec: u32_at(8),
            block_align: u16_at(12),
            bits: u16_at(14),
            extension,
        })
    }

    /// Bytes this format occupies for a 16-byte chunk or one with its
    /// extension, as stored.
    pub fn stored_len(&self) -> usize {
        16 + self.extension.as_ref().map_or(0, |e| 2 + e.len())
    }
}
