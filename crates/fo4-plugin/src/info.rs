//! Dialogue lines (INFO), read only as far as finding their voice files
//! needs (`spec/voice.md`): the responses (one TRDA each, with the
//! response number its voice file names), the conditions, the shared line
//! a line without responses names (DNAM) and the ENAM word. Every other
//! subrecord is left to the generic readers; the TRDA bytes other than
//! the response number are kept as stored.
//!
//! The layout was not taken from any source: the TRDA size and the
//! position of the response number were found by measurement (the byte
//! at offset 4 equals the number ending a voice file's name for every
//! response that has a file; `spec/voice-census.txt`), held under
//! `voice_path_convention_v1`.
use crate::{
    actor::Context,
    condition::{self, Condition},
    typed::FormRef,
    Record,
};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Bytes of one TRDA subrecord, measured.
pub const TRDA_BYTES: usize = 20;
/// Offset of the response number inside TRDA, measured.
pub const RESPONSE_NUMBER: usize = 4;

/// One response: a TRDA subrecord.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Response {
    /// The TRDA subrecord's index in the record.
    pub index: usize,
    /// The byte at [`RESPONSE_NUMBER`].
    pub number: u8,
    /// The whole TRDA, as stored.
    pub trda: [u8; TRDA_BYTES],
}

/// What the voice lookup reads of an INFO record.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub responses: Vec<Response>,
    /// Every CTDA of the record, with their texts, in stored order.
    pub conditions: Vec<Condition>,
    /// DNAM: a four-byte word resolved as a FormID through the record's
    /// plugin (the census counts what it names).
    pub shared: Option<FormRef>,
    /// ENAM, as stored (four bytes in every measured record).
    pub flags: Option<u32>,
}

fn word(r: &Record, i: usize, what: &str) -> Result<u32> {
    let s = &r.subrecords[i];
    let d = r.data(s);
    let a: [u8; 4] = d.try_into().map_err(|_| {
        Error::new(
            "InfoFieldSize",
            &r.source.file,
            s.offset,
            format!("{what} of 4 bytes"),
            d.len(),
        )
    })?;
    Ok(u32::from_le_bytes(a))
}

impl Info {
    /// Reads the responses, conditions, DNAM and ENAM of an INFO record.
    /// A TRDA that is not [`TRDA_BYTES`] long is `InfoResponseSize`; a DNAM
    /// or ENAM that is not four bytes `InfoFieldSize`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Info> {
        let file = &r.source.file;
        if r.signature != *b"INFO" {
            return Err(Error::new(
                "InfoType",
                file,
                r.source.offset,
                "INFO",
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let mut responses = Vec::new();
        let mut shared = None;
        let mut flags = None;
        for (i, s) in r.subrecords.iter().enumerate() {
            match &s.signature {
                b"TRDA" => {
                    let d = r.data(s);
                    let trda: [u8; TRDA_BYTES] = d.try_into().map_err(|_| {
                        Error::new(
                            "InfoResponseSize",
                            file,
                            s.offset,
                            format!("TRDA of {TRDA_BYTES} bytes"),
                            d.len(),
                        )
                    })?;
                    b.reserve(&mut responses, 1, file, s.offset)?;
                    responses.push(Response {
                        index: i,
                        number: trda[RESPONSE_NUMBER],
                        trda,
                    });
                }
                b"DNAM" => {
                    let raw = word(r, i, "DNAM")?;
                    shared = Some(FormRef {
                        raw,
                        key: condition::form(ctx, raw, file, s.offset)?,
                        subrecord: i,
                    });
                }
                b"ENAM" => flags = Some(word(r, i, "ENAM")?),
                _ => {}
            }
        }
        let mut conditions = Vec::new();
        for l in condition::lists(r, b)? {
            b.reserve(&mut conditions, l.conditions.len(), file, r.source.offset)?;
            conditions.extend(l.conditions);
        }
        Ok(Info {
            responses,
            conditions,
            shared,
            flags,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, LoadOrder, Subrecord};
    use fo4_core::{Limits, SliceSource, SourceSpan};

    fn order() -> LoadOrder {
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(0u32.to_le_bytes());
        hedr.extend(0x900u32.to_le_bytes());
        let mut data = b"HEDR".to_vec();
        data.extend((hedr.len() as u16).to_le_bytes());
        data.extend(hedr);
        let mut bytes = b"TES4".to_vec();
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend([0; 8]);
        bytes.extend(131u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(data);
        let h = Header::read(
            "t.esm",
            &SliceSource {
                name: "t.esm",
                bytes: &bytes,
            },
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap();
        LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap()
    }

    fn record(sig: &[u8; 4], subs: &[(&[u8; 4], Vec<u8>)]) -> Record {
        let mut payload = Vec::new();
        let mut subrecords = Vec::new();
        for (i, (t, d)) in subs.iter().enumerate() {
            let start = payload.len();
            payload.extend_from_slice(d);
            subrecords.push(Subrecord {
                signature: **t,
                data: start..payload.len(),
                offset: 100 + i as u64,
                extended: false,
            });
        }
        Record {
            signature: *sig,
            flags: 0,
            form_id: 0x800,
            revision: 0,
            version: 131,
            unknown: 0,
            source: SourceSpan::new("t.esm", 0, 0),
            payload,
            subrecords,
            payload_is_decompressed: false,
        }
    }
    fn trda(n: u8) -> Vec<u8> {
        let mut d = vec![0xffu8; 4];
        d.extend([n, 0, 0, 0, 0, 1, 0, 0]);
        d.extend([0xff; 8]);
        d
    }

    #[test]
    fn reads_responses_conditions_and_the_shared_line() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let mut ctda = vec![0u8; 32];
        ctda[8..10].copy_from_slice(&59u16.to_le_bytes());
        let r = record(
            b"INFO",
            &[
                (b"ENAM", 0x40u32.to_le_bytes().to_vec()),
                (b"TRDA", trda(2)),
                (b"NAM1", vec![1, 0, 0, 0]),
                (b"TRDA", trda(1)),
                (b"NAM1", vec![2, 0, 0, 0]),
                (b"CTDA", ctda),
                (b"DNAM", 0x812u32.to_le_bytes().to_vec()),
                (b"INAM", vec![0; 4]),
            ],
        );
        let i = Info::read(&r, ctx, &b).unwrap();
        assert_eq!(
            i.responses.iter().map(|x| x.number).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(i.responses[0].index, 1);
        assert_eq!(i.responses[1].trda[9], 1);
        assert_eq!(i.conditions.len(), 1);
        assert_eq!(i.conditions[0].ctda.function, 59);
        let s = i.shared.unwrap();
        assert_eq!(s.raw, 0x812);
        assert_eq!(s.key.unwrap().object_id, 0x812);
        assert_eq!(i.flags, Some(0x40));
    }

    #[test]
    fn bad_sizes_and_types_are_refused() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let short = record(b"INFO", &[(b"TRDA", vec![0; 19])]);
        assert_eq!(
            Info::read(&short, ctx, &b).unwrap_err().code,
            "InfoResponseSize"
        );
        let dnam = record(b"INFO", &[(b"DNAM", vec![0; 2])]);
        assert_eq!(
            Info::read(&dnam, ctx, &b).unwrap_err().code,
            "InfoFieldSize"
        );
        let other = record(b"DIAL", &[]);
        assert_eq!(Info::read(&other, ctx, &b).unwrap_err().code, "InfoType");
        let empty = Info::read(&record(b"INFO", &[]), ctx, &b).unwrap();
        assert!(empty.responses.is_empty() && empty.shared.is_none() && empty.flags.is_none());
    }
}
