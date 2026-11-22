//! Dialogue topics (DIAL), read as far as the census of dialogue outside
//! scenes and the interpreter's lines outside scenes need
//! (`spec/talk-census.txt`, `spec/vm.md`, Dialogue outside scenes).
//!
//! The subrecords of a winning DIAL measured on one installation are
//! EDID, FULL, PNAM, BNAM, QNAM, KNAM, DATA, SNAM and TIFC, each but EDID
//! and FULL four bytes in every record (`talk_census`). Their names and
//! roles were recognised from prior general knowledge and are kept only
//! as far as that census holds: PNAM read as an f32 (a priority), BNAM,
//! QNAM and KNAM as FormIDs (QNAM names a QUST in every winning record),
//! SNAM as a four-letter code (the topic's subtype; every value is four
//! upper-case letters or `_`), DATA as four bytes of which the first two
//! are given names (flag bits, a category) and the last two kept as a
//! word whose meaning is not read (it varies within one SNAM code), TIFC
//! as a u32. Nothing here says what a subtype does in the running game:
//! that is behaviour, held by pending rules where code relies on it.
use crate::{actor::Context, condition, typed::FormRef, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// A DIAL record.
#[derive(Debug, Clone, Serialize)]
pub struct Topic {
    /// EDID: the bytes before the NUL.
    pub editor_id: Option<Vec<u8>>,
    /// FULL: present (a text; not read).
    pub full: bool,
    /// PNAM as an f32.
    pub priority: Option<f32>,
    /// BNAM: a DLBR.
    pub branch: Option<FormRef>,
    /// QNAM: the owning quest.
    pub quest: Option<FormRef>,
    /// KNAM: a keyword.
    pub keyword: Option<FormRef>,
    /// DATA, as stored.
    pub data: Option<[u8; 4]>,
    /// SNAM: the subtype code, as stored.
    pub subtype: Option<[u8; 4]>,
    /// TIFC.
    pub info_count: Option<u32>,
    /// Tags of subrecords not read here, in stored order.
    pub other: Vec<[u8; 4]>,
}

impl Topic {
    /// DATA byte 0: flag bits (which bit means what is not read).
    pub fn flags(&self) -> Option<u8> {
        self.data.map(|d| d[0])
    }
    /// DATA byte 1: the category.
    pub fn category(&self) -> Option<u8> {
        self.data.map(|d| d[1])
    }
    /// DATA bytes 2 and 3 as a little-endian u16, kept as stored.
    pub fn data_word(&self) -> Option<u16> {
        self.data.map(|d| u16::from_le_bytes([d[2], d[3]]))
    }
    /// SNAM as text, when it is four printable ASCII bytes.
    pub fn subtype_text(&self) -> Option<String> {
        let s = self.subtype?;
        s.iter()
            .all(|c| c.is_ascii_graphic())
            .then(|| String::from_utf8_lossy(&s).into_owned())
    }

    /// Reads a DIAL record. A PNAM, BNAM, QNAM, KNAM, DATA, SNAM or TIFC
    /// that is not four bytes is `TopicFieldSize`; any other record type
    /// `TopicType`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Topic> {
        let file = &r.source.file;
        if r.signature != *b"DIAL" {
            return Err(Error::new(
                "TopicType",
                file,
                r.source.offset,
                "DIAL",
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let mut t = Topic {
            editor_id: None,
            full: false,
            priority: None,
            branch: None,
            quest: None,
            keyword: None,
            data: None,
            subtype: None,
            info_count: None,
            other: Vec::new(),
        };
        for (i, s) in r.subrecords.iter().enumerate() {
            let d = r.data(s);
            let four = || -> Result<[u8; 4]> {
                d.try_into().map_err(|_| {
                    Error::new(
                        "TopicFieldSize",
                        file,
                        s.offset,
                        format!("{} of 4 bytes", String::from_utf8_lossy(&s.signature)),
                        d.len(),
                    )
                })
            };
            let form = |w: [u8; 4]| -> Result<FormRef> {
                let raw = u32::from_le_bytes(w);
                Ok(FormRef {
                    raw,
                    key: condition::form(ctx, raw, file, s.offset)?,
                    subrecord: i,
                })
            };
            match &s.signature {
                b"EDID" => {
                    let end = d.iter().position(|c| *c == 0).unwrap_or(d.len());
                    t.editor_id = Some(d[..end].to_vec());
                }
                b"FULL" => t.full = true,
                b"PNAM" => t.priority = Some(f32::from_le_bytes(four()?)),
                b"BNAM" => t.branch = Some(form(four()?)?),
                b"QNAM" => t.quest = Some(form(four()?)?),
                b"KNAM" => t.keyword = Some(form(four()?)?),
                b"DATA" => t.data = Some(four()?),
                b"SNAM" => t.subtype = Some(four()?),
                b"TIFC" => t.info_count = Some(u32::from_le_bytes(four()?)),
                other => {
                    b.reserve(&mut t.other, 1, file, s.offset)?;
                    t.other.push(*other);
                }
            }
        }
        Ok(t)
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

    #[test]
    fn reads_the_topic_fields() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let r = record(
            b"DIAL",
            &[
                (b"EDID", b"Topic\0".to_vec()),
                (b"PNAM", 50.0f32.to_le_bytes().to_vec()),
                (b"QNAM", 0x812u32.to_le_bytes().to_vec()),
                (b"DATA", vec![1, 7, 0x58, 0]),
                (b"SNAM", b"HELO".to_vec()),
                (b"TIFC", 3u32.to_le_bytes().to_vec()),
                (b"XXXX", vec![9]),
            ],
        );
        let t = Topic::read(&r, ctx, &b).unwrap();
        assert_eq!(t.editor_id.as_deref(), Some(&b"Topic"[..]));
        assert_eq!(t.priority, Some(50.0));
        assert_eq!(
            t.quest.as_ref().unwrap().key.as_ref().unwrap().object_id,
            0x812
        );
        assert_eq!(t.flags(), Some(1));
        assert_eq!(t.category(), Some(7));
        assert_eq!(t.data_word(), Some(0x58));
        assert_eq!(t.subtype_text().as_deref(), Some("HELO"));
        assert_eq!(t.info_count, Some(3));
        assert_eq!(t.other, vec![*b"XXXX"]);
        assert!(!t.full && t.branch.is_none() && t.keyword.is_none());
    }

    #[test]
    fn bad_sizes_and_types_are_refused() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let short = record(b"DIAL", &[(b"SNAM", b"HEL".to_vec())]);
        assert_eq!(
            Topic::read(&short, ctx, &b).unwrap_err().code,
            "TopicFieldSize"
        );
        let other = record(b"INFO", &[]);
        assert_eq!(Topic::read(&other, ctx, &b).unwrap_err().code, "TopicType");
        let empty = Topic::read(&record(b"DIAL", &[]), ctx, &b).unwrap();
        assert!(empty.subtype.is_none() && empty.subtype_text().is_none());
    }
}
