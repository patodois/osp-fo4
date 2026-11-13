//! Bounded payload sampling. A partial prefix explicitly does not certify the whole stream.
use crate::GameArchive;
use fo4_core::{add, limit, range, Budget, Error, Limits, ReadAt, Result};
use serde::Serialize;
use std::io::{BufReader, Read};
#[derive(Debug, Serialize)]
pub struct Prefix {
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub bytes: Vec<u8>,
    pub declared_size: u64,
    pub full_stream_validated: bool,
}
struct Interval<'a> {
    source: &'a dyn ReadAt,
    pos: u64,
    end: u64,
}
impl Read for Interval<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = out.len().min(4096).min((self.end - self.pos) as usize);
        if n == 0 {
            return Ok(0);
        }
        self.source
            .read_exact_at(self.pos, &mut out[..n])
            .map_err(std::io::Error::other)?;
        self.pos += n as u64;
        Ok(n)
    }
}
impl GameArchive {
    pub fn payload_size(&self, index: usize) -> Result<u64> {
        self.entry(index)?;
        match self {
            Self::Fallout4(a) => a.entries[index].chunks.iter().try_fold(
                if a.entries[index].texture.is_some() {
                    148
                } else {
                    0
                },
                |n, c| {
                    add(
                        n,
                        c.unpacked as u64,
                        &a.entries[index].source.file,
                        c.table_offset,
                    )
                },
            ),
            Self::SkyrimSe(a) => Ok(a.entries[index].unpacked),
        }
    }
    pub fn read_prefix(
        &self,
        s: &dyn ReadAt,
        index: usize,
        max: u64,
        l: &Limits,
        b: &Budget,
    ) -> Result<Prefix> {
        self.entry(index)?;
        s.check_stable()?;
        limit(max, 4096, s.name(), 0)?;
        limit(max, l.max_payload, s.name(), 0)?;
        let Self::Fallout4(a) = self else {
            return Err(Error::new(
                "UnsupportedPrefixArchive",
                s.name(),
                0,
                "BA2 GNRL",
                "BSA",
            ));
        };
        let e = &a.entries[index];
        if e.texture.is_some() || e.chunks.len() != 1 {
            return Err(Error::new(
                "UnsupportedPrefixEntry",
                s.name(),
                e.source.offset,
                "one GNRL chunk",
                e.chunks.len(),
            ));
        }
        let c = &e.chunks[0];
        range(c.offset, c.stored_size(), s.len(), s.name())?;
        let n = max.min(c.unpacked as u64);
        let mut bytes = Vec::new();
        b.reserve(&mut bytes, n as usize, s.name(), c.offset)?;
        bytes.resize(n as usize, 0);
        let full = n == c.unpacked as u64;
        if c.packed == 0 {
            s.read_exact_at(c.offset, &mut bytes)?;
        } else {
            b.charge(65536, s.name(), c.offset)?;
            let input = Interval {
                source: s,
                pos: c.offset,
                end: add(c.offset, c.packed as u64, s.name(), c.offset)?,
            };
            let mut decoder =
                flate2::bufread::ZlibDecoder::new(BufReader::with_capacity(4096, input));
            decoder.read_exact(&mut bytes).map_err(|e| {
                Error::new(
                    "InvalidCompressedPrefix",
                    s.name(),
                    c.offset,
                    format!("{n} decoded prefix bytes"),
                    e,
                )
            })?;
            if full {
                let mut extra = [0];
                let count = decoder.read(&mut extra).map_err(|e| {
                    Error::new(
                        "InvalidCompressedPrefix",
                        s.name(),
                        c.offset,
                        "valid zlib EOF",
                        e,
                    )
                })?;
                if count != 0 || decoder.total_in() != c.packed as u64 {
                    return Err(Error::new(
                        "DecompressedSizeMismatch",
                        s.name(),
                        c.offset,
                        format!("{} bytes and exact stored EOF", c.unpacked),
                        format!("extra={count}, consumed={}", decoder.total_in()),
                    ));
                }
            }
        }
        s.check_stable()?;
        Ok(Prefix {
            bytes,
            declared_size: c.unpacked as u64,
            full_stream_validated: full,
        })
    }
}
