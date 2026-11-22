//! Packages (PACK), read as far as the census of scene package actions and
//! the interpreter's package destinations need (`spec/package-census.txt`,
//! `spec/vm.md`, Package destinations).
//!
//! The layout was established on the winning PACK records of one
//! installation by `package_census` and is kept only as far as that census
//! holds. PKCU is three u32: a count, a FormID (a PACK or none) and a
//! third word kept as stored; the count equals the number of data inputs.
//! A data input is an ANAM text (its type name) followed by at most one
//! value subrecord (CNAM, PLDT, PTDA or another tag, kept as stored); the
//! inputs run from PKCU to the first subrecord that is neither ANAM nor a
//! value tag. PLDT is four u32 (a kind, a word that is a FormID for some
//! kinds, a third and a fourth word; 12 bytes without the fourth in eight
//! records), PTDA three (a kind, a word, a third word). The PNAM texts that follow an ANAM `Procedure` are kept as the
//! procedure names. Every other subrecord is kept by tag, in stored order.
//! That the count is of inputs, that the FormID is a template, and the
//! roles of the tags were recognised from prior general knowledge; what a
//! package makes an actor do is behaviour, held by pending rules where
//! code relies on it.
use crate::{actor::Context, condition, typed::FormRef, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// The value tags a data input can carry, as measured (PDTO after a
/// `Topic` input).
pub const VALUE_TAGS: [[u8; 4]; 4] = [*b"CNAM", *b"PLDT", *b"PTDA", *b"PDTO"];

/// A PLDT: (kind, word, third, fourth) as stored, with the word resolved
/// as a FormID where it is one (a word that is not a FormID of the
/// record's masters, such as 0xFFFFFFFF, has no key). Eight PLDT of 12
/// bytes store no fourth word.
#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub kind: u32,
    pub word: FormRef,
    pub third: u32,
    pub fourth: Option<u32>,
}

/// A PTDA: (kind, word, third) as stored.
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub kind: u32,
    pub word: FormRef,
    pub third: u32,
}

/// One data input.
#[derive(Debug, Clone, Serialize)]
pub struct Input {
    /// The ANAM text.
    pub kind: Vec<u8>,
    /// The value subrecord's tag and bytes, when one follows.
    pub value: Option<([u8; 4], Vec<u8>)>,
    pub location: Option<Location>,
    pub target: Option<Target>,
}

/// A PACK record.
#[derive(Debug, Clone, Serialize)]
pub struct Package {
    pub editor_id: Option<Vec<u8>>,
    /// PKDT, as stored.
    pub data: Option<Vec<u8>>,
    /// PKCU: the count, the FormID and the third word.
    pub count: Option<u32>,
    pub template: Option<FormRef>,
    pub version: Option<u32>,
    pub inputs: Vec<Input>,
    /// PNAM texts after an ANAM `Procedure`, in stored order.
    pub procedures: Vec<Vec<u8>>,
    /// Number of conditions (CTDA), all places.
    pub conditions: usize,
    /// Tags of every subrecord, in stored order.
    pub tags: Vec<[u8; 4]>,
}

fn text(d: &[u8]) -> Vec<u8> {
    let end = d.iter().position(|c| *c == 0).unwrap_or(d.len());
    d[..end].to_vec()
}

impl Package {
    /// The inputs whose PLDT word names a record, with their index.
    pub fn locations(&self) -> impl Iterator<Item = (usize, &Location)> {
        self.inputs
            .iter()
            .enumerate()
            .filter_map(|(i, x)| x.location.as_ref().map(|l| (i, l)))
    }

    /// Reads a PACK record. A PKCU that is not 12 bytes, a PLDT neither 12
    /// nor 16 or a PTDA not 12 is `PackageFieldSize`; a value subrecord
    /// with no ANAM before it `PackageOrder`; another record type
    /// `PackageType`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Package> {
        let file = &r.source.file;
        if r.signature != *b"PACK" {
            return Err(Error::new(
                "PackageType",
                file,
                r.source.offset,
                "PACK",
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let mut p = Package {
            editor_id: None,
            data: None,
            count: None,
            template: None,
            version: None,
            inputs: Vec::new(),
            procedures: Vec::new(),
            conditions: 0,
            tags: Vec::new(),
        };
        b.reserve(&mut p.tags, r.subrecords.len(), file, r.source.offset)?;
        let size = |s: &crate::Subrecord, n: usize| -> Result<()> {
            let len = s.data.end - s.data.start;
            if len == n || (s.signature == *b"PLDT" && len == 12) {
                Ok(())
            } else {
                Err(Error::new(
                    "PackageFieldSize",
                    file,
                    s.offset,
                    format!("{} of {n} bytes", String::from_utf8_lossy(&s.signature)),
                    len,
                ))
            }
        };
        let word =
            |d: &[u8], at: usize| u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]);
        let form = |raw: u32, i: usize, offset: u64| -> Result<FormRef> {
            Ok(FormRef {
                raw,
                key: condition::form(ctx, raw, file, offset)?,
                subrecord: i,
            })
        };
        // An input's word is a FormID only for some kinds: one that names
        // no master of the record is kept without a key.
        let word_ref = |raw: u32, i: usize, offset: u64| FormRef {
            raw,
            key: condition::form(ctx, raw, file, offset).ok().flatten(),
            subrecord: i,
        };
        // Whether the subrecords since PKCU are still data inputs, and
        // whether the last ANAM was `Procedure`.
        let mut in_inputs = false;
        let mut procedure = false;
        for (i, s) in r.subrecords.iter().enumerate() {
            let d = r.data(s);
            p.tags.push(s.signature);
            let tag = &s.signature;
            if in_inputs && tag != b"ANAM" && !VALUE_TAGS.contains(tag) {
                in_inputs = false;
            }
            match tag {
                b"EDID" => p.editor_id = Some(text(d)),
                b"PKDT" => p.data = Some(d.to_vec()),
                b"CTDA" => p.conditions += 1,
                b"PKCU" => {
                    size(s, 12)?;
                    p.count = Some(word(d, 0));
                    p.template = Some(form(word(d, 4), i, s.offset)?);
                    p.version = Some(word(d, 8));
                    in_inputs = true;
                }
                b"ANAM" if in_inputs => {
                    b.reserve(&mut p.inputs, 1, file, s.offset)?;
                    p.inputs.push(Input {
                        kind: text(d),
                        value: None,
                        location: None,
                        target: None,
                    });
                }
                t if in_inputs && VALUE_TAGS.contains(t) => {
                    let Some(input) = p.inputs.last_mut().filter(|x| x.value.is_none()) else {
                        return Err(Error::new(
                            "PackageOrder",
                            file,
                            s.offset,
                            "an ANAM before each input value",
                            String::from_utf8_lossy(t),
                        ));
                    };
                    match t {
                        b"PLDT" => {
                            size(s, 16)?;
                            input.location = Some(Location {
                                kind: word(d, 0),
                                word: word_ref(word(d, 4), i, s.offset),
                                third: word(d, 8),
                                fourth: (d.len() == 16).then(|| word(d, 12)),
                            });
                        }
                        b"PTDA" => {
                            size(s, 12)?;
                            input.target = Some(Target {
                                kind: word(d, 0),
                                word: word_ref(word(d, 4), i, s.offset),
                                third: word(d, 8),
                            });
                        }
                        _ => {}
                    }
                    input.value = Some((*t, d.to_vec()));
                }
                b"ANAM" => procedure = text(d) == b"Procedure",
                b"PNAM" if procedure => {
                    procedure = false;
                    p.procedures.push(text(d));
                }
                _ => {}
            }
        }
        Ok(p)
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
    fn reads_inputs_template_and_procedures() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let mut pldt = 0u32.to_le_bytes().to_vec();
        pldt.extend(0x801u32.to_le_bytes());
        pldt.extend(150u32.to_le_bytes());
        pldt.extend(0u32.to_le_bytes());
        let mut pkcu = 2u32.to_le_bytes().to_vec();
        pkcu.extend(0x802u32.to_le_bytes());
        pkcu.extend(1u32.to_le_bytes());
        let r = record(
            b"PACK",
            &[
                (b"EDID", b"P\0".to_vec()),
                (b"PKDT", vec![0; 12]),
                (b"PKCU", pkcu),
                (b"ANAM", b"Location\0".to_vec()),
                (b"PLDT", pldt),
                (b"ANAM", b"Bool\0".to_vec()),
                (b"CNAM", vec![1]),
                (b"UNAM", vec![0]),
                (b"XNAM", vec![1]),
                (b"ANAM", b"Procedure\0".to_vec()),
                (b"PNAM", b"Walk\0".to_vec()),
            ],
        );
        let p = Package::read(&r, ctx, &Budget::new(1 << 20)).unwrap();
        assert_eq!(p.count, Some(2));
        assert_eq!(p.inputs.len(), 2);
        assert_eq!(p.inputs[0].kind, b"Location");
        let l = p.inputs[0].location.as_ref().unwrap();
        assert_eq!((l.kind, l.word.raw, l.third), (0, 0x801, 150));
        assert_eq!(p.inputs[1].value.as_ref().map(|v| v.0), Some(*b"CNAM"));
        assert_eq!(p.procedures, [b"Walk".to_vec()]);
        assert_eq!(p.template.as_ref().map(|t| t.raw), Some(0x802));
        assert_eq!(p.locations().count(), 1);
    }

    #[test]
    fn refuses_a_short_location_and_a_value_without_its_type() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let pkcu = vec![0u8; 12];
        let short = record(
            b"PACK",
            &[
                (b"PKCU", pkcu.clone()),
                (b"ANAM", b"Location\0".to_vec()),
                (b"PLDT", vec![0; 8]),
            ],
        );
        assert_eq!(
            Package::read(&short, ctx, &b).unwrap_err().code,
            "PackageFieldSize"
        );
        let orphan = record(b"PACK", &[(b"PKCU", pkcu), (b"CNAM", vec![1])]);
        assert_eq!(
            Package::read(&orphan, ctx, &b).unwrap_err().code,
            "PackageOrder"
        );
        let dial = record(b"DIAL", &[]);
        assert_eq!(
            Package::read(&dial, ctx, &b).unwrap_err().code,
            "PackageType"
        );
    }
}
