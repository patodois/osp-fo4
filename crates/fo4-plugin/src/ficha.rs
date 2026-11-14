//! Facts from independently checked black-box field tables, without extrapolation to other pairs.
use crate::{typed::FormRef, LoadOrder, Record};
use fo4_core::{Budget, Limits, Result};
use serde::Serialize;
use std::ops::Range;
#[path = "ficha_data.rs"]
mod data;
enum Kind {
    Float,
    Form,
    I32,
    U32,
    Text,
    Color,
    Presence,
    Bytes,
    ObservedBooleanByte,
}
struct FieldSpec {
    label: &'static str,
    offset: usize,
    width: usize,
    kind: Kind,
    sizes: &'static [usize],
}
struct FlagWord {
    pair: &'static str,
    raw: u32,
    label: Option<&'static str>,
    closed: bool,
}
fn flag_word(pair: &str, raw: u32) -> Option<&'static FlagWord> {
    data::FLAG_WORDS
        .iter()
        .find(|word| word.pair == pair && word.raw == raw)
}
pub struct Schema {
    pair: &'static str,
    sizes: &'static [usize],
    closed: bool,
    pub open_field: &'static str,
    fields: &'static [FieldSpec],
}
impl Schema {
    pub fn accepts(&self, size: usize) -> bool {
        self.sizes.contains(&size)
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn complete_for(&self, data: &[u8]) -> bool {
        if !self.accepts(data.len()) {
            return false;
        }
        if self.pair == "MSTT.DATA" {
            return matches!(data, [0] | [1]);
        }
        if self.closed {
            return true;
        }
        let Some(offset) = self.flag_offset() else {
            return false;
        };
        let Some(bytes) = data.get(offset..offset + 4) else {
            return false;
        };
        let Ok(bytes) = <[u8; 4]>::try_from(bytes) else {
            return false;
        };
        flag_word(self.pair, u32::from_le_bytes(bytes)).is_some_and(|word| word.closed)
    }
    fn flag_offset(&self) -> Option<usize> {
        match self.pair {
            "LIGH.DATA" => Some(12),
            "CELL.XCLL" => Some(88),
            _ => None,
        }
    }
}
pub fn schema(record: &[u8; 4], tag: &[u8; 4]) -> Option<&'static Schema> {
    let key = (u32::from_be_bytes(*record) as u64) << 32 | u32::from_be_bytes(*tag) as u64;
    let index = INDEX.binary_search_by_key(&key, |entry| entry.0).ok()?;
    Some(&data::SCHEMAS[INDEX[index].1])
}
const fn index() -> [(u64, usize); data::SCHEMAS.len()] {
    let mut out = [(0, 0); data::SCHEMAS.len()];
    let mut i = 0;
    while i < data::SCHEMAS.len() {
        let bytes = data::SCHEMAS[i].pair.as_bytes();
        let mut key = 0;
        let mut j = 0;
        while j < 9 {
            if j != 4 {
                key = (key << 8) | bytes[j] as u64;
            }
            j += 1;
        }
        let mut pos = i;
        while pos > 0 && out[pos - 1].0 > key {
            out[pos] = out[pos - 1];
            pos -= 1;
        }
        out[pos] = (key, i);
        i += 1;
    }
    out
}
static INDEX: [(u64, usize); data::SCHEMAS.len()] = index();
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum Value {
    Float(#[serde(serialize_with = "fo4_core::ieee::serialize")] f32),
    Form(FormRef),
    I32(i32),
    U32(u32),
    /// Complete word-to-label observation; no inferred bit positions.
    Flags {
        raw: u32,
        printed_labels: Option<&'static str>,
        fully_named: bool,
    },
    /// Three RGB bytes plus the explicitly unused source byte.
    Color([u8; 4]),
    ObservedByte {
        raw: u8,
        printed_label: Option<&'static str>,
        fully_named: bool,
    },
    Presence,
    /// Raw field bytes remain in the owning record, including non-UTF8 text.
    TextBytes(Range<usize>),
    Bytes(Range<usize>),
}
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub subrecord: usize,
    pub label: &'static str,
    pub bytes: Range<usize>,
    pub value: Value,
}
pub(crate) fn decode(
    r: &Record,
    order: &LoadOrder,
    plugin: usize,
    b: &Budget,
    l: &Limits,
) -> Result<Vec<Field>> {
    let mut out = Vec::new();
    let max_form_name = order.headers().iter().map(|h| h.name.len()).max();
    for (i, s) in r.subrecords.iter().enumerate() {
        let Some(schema) = schema(&r.signature, &s.signature) else {
            continue;
        };
        if schema.closed
            && crate::still::classify(&r.signature, &s.signature) != crate::still::Class::Used
        {
            continue;
        }
        let data = r.data(s);
        if !schema.accepts(data.len()) {
            continue;
        }
        for f in schema
            .fields
            .iter()
            .filter(|f| f.sizes.contains(&data.len()))
        {
            let field_offset = fo4_core::add(
                fo4_core::add(s.offset, 6, &r.source.file, s.offset)?,
                f.offset as u64,
                &r.source.file,
                s.offset,
            )?;
            // These constant offsets are audited schema facts; still range-check every access.
            let end = fo4_core::add(f.offset as u64, f.width as u64, &r.source.file, s.offset)?;
            fo4_core::range(
                f.offset as u64,
                f.width as u64,
                data.len() as u64,
                &r.source.file,
            )?;
            let bytes = &data[f.offset..end as usize];
            let word = || -> Result<[u8; 4]> {
                bytes.try_into().map_err(|_| {
                    fo4_core::Error::new(
                        "FichaFieldWidth",
                        &r.source.file,
                        field_offset,
                        "4 bytes",
                        bytes.len(),
                    )
                })
            };
            let value = match f.kind {
                Kind::ObservedBooleanByte => Value::ObservedByte {
                    raw: bytes[0],
                    printed_label: match bytes[0] {
                        0 => Some("False"),
                        1 => Some("True"),
                        _ => None,
                    },
                    fully_named: bytes[0] <= 1,
                },
                Kind::Float => Value::Float(f32::from_le_bytes(word()?)),
                Kind::I32 => Value::I32(i32::from_le_bytes(word()?)),
                Kind::U32 => {
                    let raw = u32::from_le_bytes(word()?);
                    if schema.flag_offset() == Some(f.offset) {
                        match flag_word(schema.pair, raw) {
                            Some(word) => Value::Flags {
                                raw,
                                printed_labels: word.label,
                                fully_named: word.closed,
                            },
                            None => Value::Flags {
                                raw,
                                printed_labels: None,
                                fully_named: false,
                            },
                        }
                    } else {
                        Value::U32(raw)
                    }
                }
                Kind::Color => Value::Color(word()?),
                Kind::Form => {
                    let raw = u32::from_le_bytes(word()?);
                    if raw != 0 {
                        let bytes = max_form_name.ok_or_else(|| {
                            fo4_core::Error::new(
                                "InvalidPluginIndex",
                                &r.source.file,
                                field_offset,
                                "nonempty load order for nonzero FormID",
                                0,
                            )
                        })?;
                        b.charge(bytes as u64, &r.source.file, field_offset)?;
                    }
                    Value::Form(FormRef {
                        raw,
                        key: order.remap(plugin, raw, &r.source.file, field_offset)?,
                        subrecord: i,
                    })
                }
                Kind::Presence => Value::Presence,
                Kind::Text => {
                    let len = bytes.iter().position(|v| *v == 0).unwrap_or(bytes.len());
                    fo4_core::limit(len as u64, l.max_string, &r.source.file, field_offset)?;
                    Value::TextBytes(s.data.start + f.offset..s.data.start + f.offset + len)
                }
                Kind::Bytes => Value::Bytes(s.data.start + f.offset..s.data.start + end as usize),
            };
            b.reserve(&mut out, 1, &r.source.file, s.offset)?;
            out.push(Field {
                subrecord: i,
                label: f.label,
                bytes: s.data.start + f.offset..s.data.start + end as usize,
                value,
            });
        }
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_ficha_offset_fits_only_its_verified_variants() {
        for schema in data::SCHEMAS {
            let record = schema.pair.as_bytes()[..4].try_into().unwrap();
            let tag = schema.pair.as_bytes()[5..].try_into().unwrap();
            assert_eq!(super::schema(record, tag).unwrap().pair, schema.pair);
            for field in schema.fields {
                for size in field.sizes {
                    assert!(schema.accepts(*size), "{} {}", schema.pair, field.label);
                    assert!(field
                        .offset
                        .checked_add(field.width)
                        .is_some_and(|end| end <= *size));
                    if matches!(
                        field.kind,
                        Kind::Float | Kind::Form | Kind::I32 | Kind::U32 | Kind::Color
                    ) {
                        assert_eq!(field.width, 4);
                    }
                }
            }
            if schema.closed {
                for size in schema.sizes {
                    for byte in 0..*size {
                        assert!(
                            schema.fields.iter().any(|f| f.sizes.contains(size)
                                && byte >= f.offset
                                && byte < f.offset + f.width),
                            "{} size={} uncovered={byte}",
                            schema.pair,
                            size
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn dnam_optional_leaf_fields_and_truncations_are_not_defaulted() {
        let l = Limits::default();
        let order = LoadOrder::new(Vec::new(), &Budget::new(1 << 20)).unwrap();
        let mut payload = [0; 16];
        payload[..4].copy_from_slice(&90f32.to_le_bytes());
        payload[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        for size in 0..=20 {
            let data = payload[..size.min(16)]
                .iter()
                .copied()
                .chain(std::iter::repeat_n(0, size.saturating_sub(16)))
                .collect::<Vec<_>>();
            let r = Record {
                signature: *b"STAT",
                flags: 0,
                form_id: 0x800,
                revision: 0,
                version: 131,
                unknown: 0,
                source: fo4_core::SourceSpan::new("synthetic", 0, size as u64),
                payload: data,
                subrecords: vec![crate::Subrecord {
                    signature: *b"DNAM",
                    data: 0..size,
                    offset: 24,
                    extended: false,
                }],
                payload_is_decompressed: false,
            };
            let b = Budget::new(16 << 10);
            let fields = decode(&r, &order, 0, &b, &l).unwrap();
            assert_eq!(
                fields.len(),
                match size {
                    8 => 2,
                    16 => 4,
                    _ => 0,
                }
            );
            assert!(b.used() <= 16 << 10);
        }
    }
    fn synthetic(record: [u8; 4], tag: [u8; 4], bytes: Vec<u8>) -> Record {
        let n = bytes.len();
        Record {
            signature: record,
            flags: 0,
            form_id: 0x800,
            revision: 0,
            version: 131,
            unknown: 0,
            source: fo4_core::SourceSpan::new("synthetic-case18", 0, n as u64),
            payload: bytes,
            subrecords: vec![crate::Subrecord {
                signature: tag,
                data: 0..n,
                offset: 24,
                extended: false,
            }],
            payload_is_decompressed: false,
        }
    }
    #[test]
    fn only_observed_boolean_bytes_and_sourced_xlig_offsets_are_named() {
        let order = LoadOrder::new(Vec::new(), &Budget::new(1 << 20)).unwrap();
        let schema = super::schema(b"MSTT", b"DATA").unwrap();
        for value in 0..=255u8 {
            let r = synthetic(*b"MSTT", *b"DATA", vec![value]);
            let fields = decode(&r, &order, 0, &Budget::new(4096), &Limits::default()).unwrap();
            assert_eq!(schema.complete_for(&r.payload), value <= 1);
            assert!(
                matches!(fields[0].value,Value::ObservedByte{raw,printed_label,fully_named} if raw==value && fully_named==(value<=1) && printed_label==match value {0=>Some("False"),1=>Some("True"),_=>None})
            );
        }
        for n in [0, 2, 3] {
            assert!(!schema.complete_for(&vec![0; n]));
        }
        for n in 0..=25 {
            let mut bytes = vec![0xaa; n];
            if n >= 16 {
                bytes[4..8].copy_from_slice(&1.451157f32.to_le_bytes());
                bytes[12..16].copy_from_slice(&2.0f32.to_le_bytes());
            }
            let r = synthetic(*b"REFR", *b"XLIG", bytes);
            let b = Budget::new(4096);
            let fields = decode(&r, &order, 0, &b, &Limits::default()).unwrap();
            assert!(!super::schema(b"REFR", b"XLIG")
                .unwrap()
                .complete_for(&r.payload));
            if n == 24 {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].bytes, 12..16);
                assert!(matches!(fields[0].value,Value::Float(v) if v==2.0));
            } else if [16, 20].contains(&n) {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].bytes, 4..8);
                assert_eq!(fields[1].bytes, 12..16);
                assert!(
                    matches!(fields[0].value,Value::Float(v) if v.to_bits()==1.451157f32.to_bits())
                );
            } else {
                assert!(fields.is_empty());
            }
            assert!(b.used() <= 4096);
        }
        let r = synthetic(*b"REFR", *b"XLIG", vec![0; 16]);
        assert!(decode(&r, &order, 0, &Budget::new(0), &Limits::default()).is_err());
    }
    #[test]
    fn ficha_pair_index_has_no_duplicates() {
        assert!(INDEX.windows(2).all(|w| w[0].0 < w[1].0));
    }
    #[test]
    fn observed_flag_words_are_not_bit_masks_for_unseen_combinations() {
        let light = schema(b"LIGH", b"DATA").unwrap();
        let known = data::FLAG_WORDS
            .iter()
            .filter(|word| word.pair == "LIGH.DATA" && word.closed)
            .map(|word| word.raw)
            .collect::<Vec<_>>();
        assert_eq!(known, [0, 8, 0x8000, 0xc8008]);
        let mut bytes = [0; 64];
        for word in [0u32, 8, 0x8000, 0xc8008] {
            bytes[12..16].copy_from_slice(&word.to_le_bytes());
            assert!(light.complete_for(&bytes));
        }
        // This combines two known words; the combination has no observed label.
        bytes[12..16].copy_from_slice(&0x8008u32.to_le_bytes());
        assert!(!light.complete_for(&bytes));
        let cell = schema(b"CELL", b"XCLL").unwrap();
        let mut bytes = [0; 136];
        assert!(cell.complete_for(&bytes));
        bytes[88..92].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!cell.complete_for(&bytes));
        assert!(!cell.complete_for(&bytes[..135]));
    }
}
