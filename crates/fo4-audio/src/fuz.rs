//! The voice container (`.fuz`) that bundles lip data with audio
//! (`spec/audio.md`, "Voice container").
//!
//! Candidate layout (`audio_fuz_layout_v1`, recognised from prior knowledge
//! and held by the census): the four bytes `FUZE`, a u32 version, a u32
//! byte count of the lip half, the lip half, and the audio half, which runs
//! to the file's last byte and is itself a RIFF file. The lip half is kept
//! raw.
use crate::riff::Span;
use fo4_core::{Error, Result};
use serde::Serialize;

/// The first four bytes of every voice container.
pub const MAGIC: [u8; 4] = *b"FUZE";
/// Bytes before the lip half.
pub const HEADER_BYTES: u64 = 12;

/// A voice container split into its two halves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Fuz {
    pub version: u32,
    pub lip: Span,
    pub audio: Span,
}

impl Fuz {
    /// Splits a whole voice container held in `bytes`.
    pub fn read(bytes: &[u8], file: &str) -> Result<Self> {
        let len = bytes.len() as u64;
        if bytes.get(..4) != Some(&MAGIC[..]) {
            return Err(Error::new(
                "FuzMagic",
                file,
                0,
                "FUZE",
                fo4_core::HexBytes(&bytes[..bytes.len().min(4)]),
            ));
        }
        if len < HEADER_BYTES {
            return Err(Error::new(
                "FuzTruncated",
                file,
                0,
                format!(">= {HEADER_BYTES} bytes"),
                len,
            ));
        }
        let word =
            |p: usize| u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
        let version = word(4);
        let lip = Span {
            offset: HEADER_BYTES,
            length: u64::from(word(8)),
        };
        if lip.end() > len {
            return Err(Error::new(
                "FuzLipPastEnd",
                file,
                8,
                format!("lip end <= {len}"),
                lip.end(),
            ));
        }
        Ok(Self {
            version,
            lip,
            audio: Span {
                offset: lip.end(),
                length: len - lip.end(),
            },
        })
    }
}
