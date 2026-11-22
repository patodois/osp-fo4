//! Conditions: the CTDA subrecord and the CIS1 and CIS2 texts that follow
//! it, read to the last byte (`spec/conditions.md`).
//!
//! The layout below was recognised from prior general knowledge (no
//! public page giving it could be read at this step) and is kept only
//! where the census over every winning record of one installation
//! (`cond_census`, `spec/conditions-census.txt`) could have refuted it and
//! did not: the size, the byte that holds the operator and the flag bits,
//! the three zero bytes after it, the comparison word that names a GLOB
//! exactly when one flag bit is set, the function index and the two zero
//! bytes after it, the run-on word and the reference that is set only for
//! one run-on value. The two parameter words and the last word are kept as
//! stored: what they hold depends on the function, which no source read
//! here describes; [`form`] resolves any word as a FormID on request.
//!
//! What a condition means in the running game (how a list combines, what
//! the operator compares, what each function answers) is behaviour, held
//! as pending rules (`cond_*` in `spec/hypotheses.tsv`) by the code that
//! relies on it (`fo4_vm::cond`).
use crate::{actor::Context, quest, FormKey, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Bytes of a CTDA subrecord.
pub const SIZE: usize = 32;

/// Flag bits of byte 0 (bits 0 to 4). Candidate meanings recognised from
/// prior knowledge; only [`GLOBAL`] is checked by the census (the
/// comparison word names a GLOB exactly when it is set), the others are
/// counted and named in the rules that rely on them.
pub mod flag {
    /// Joins this condition to the next by OR (`cond_list_or_groups_v1`).
    pub const OR: u8 = 0x01;
    /// The parameters name aliases (`cond_flag_parameters_v1`).
    pub const ALIASES: u8 = 0x02;
    /// The comparison word is a GLOB (`cond_use_global_v1`).
    pub const GLOBAL: u8 = 0x04;
    /// The parameters name package data (`cond_flag_parameters_v1`).
    pub const PACK_DATA: u8 = 0x08;
    /// Subject and target swapped (`cond_flag_parameters_v1`).
    pub const SWAP: u8 = 0x10;
}

/// The comparison side of a condition (word at 4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Comparison {
    /// [`flag::GLOBAL`] clear: an f32.
    Float(f32),
    /// [`flag::GLOBAL`] set: a FormID, as stored (resolve with [`form`]).
    Global(u32),
}

/// One CTDA, 32 bytes, field by field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Ctda {
    /// u8 at 0: the operator (bits 5 to 7) and the flag bits (0 to 4).
    pub byte0: u8,
    /// Bytes 1 to 3, kept as stored (zero in every measured CTDA).
    pub unused: [u8; 3],
    /// u32 at 4: the comparison word ([`Ctda::comparison`]).
    pub value: u32,
    /// u16 at 8: the function index.
    pub function: u16,
    /// Bytes 10 and 11, kept as stored.
    pub pad: [u8; 2],
    /// u32 at 12: the first parameter, as stored.
    pub param1: u32,
    /// u32 at 16: the second parameter, as stored.
    pub param2: u32,
    /// u32 at 20: what the condition runs on, as stored.
    pub run_on: u32,
    /// u32 at 24: a FormID, zero unless the run-on word is 2 (as stored).
    pub reference: u32,
    /// i32 at 28, as stored.
    pub param3: i32,
}

impl Ctda {
    /// The fields of 32 stored bytes.
    pub fn from_bytes(d: &[u8; SIZE]) -> Self {
        let w = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        Ctda {
            byte0: d[0],
            unused: [d[1], d[2], d[3]],
            value: w(4),
            function: u16::from_le_bytes([d[8], d[9]]),
            pad: [d[10], d[11]],
            param1: w(12),
            param2: w(16),
            run_on: w(20),
            reference: w(24),
            param3: w(28) as i32,
        }
    }
    /// Reads a CTDA's bytes; `ConditionSize` when they are not 32.
    pub fn read(d: &[u8], file: &str, offset: u64) -> Result<Self> {
        let a: &[u8; SIZE] = d.try_into().map_err(|_| {
            Error::new(
                "ConditionSize",
                file,
                offset,
                format!("CTDA of {SIZE} bytes"),
                d.len(),
            )
        })?;
        Ok(Self::from_bytes(a))
    }
    /// The stored bytes again.
    pub fn to_bytes(&self) -> [u8; SIZE] {
        let mut d = [0u8; SIZE];
        d[0] = self.byte0;
        d[1..4].copy_from_slice(&self.unused);
        d[4..8].copy_from_slice(&self.value.to_le_bytes());
        d[8..10].copy_from_slice(&self.function.to_le_bytes());
        d[10..12].copy_from_slice(&self.pad);
        d[12..16].copy_from_slice(&self.param1.to_le_bytes());
        d[16..20].copy_from_slice(&self.param2.to_le_bytes());
        d[20..24].copy_from_slice(&self.run_on.to_le_bytes());
        d[24..28].copy_from_slice(&self.reference.to_le_bytes());
        d[28..32].copy_from_slice(&self.param3.to_le_bytes());
        d
    }
    /// The operator code, bits 5 to 7 of byte 0 (0 to 7).
    pub fn operator(&self) -> u8 {
        self.byte0 >> 5
    }
    /// The flag bits, bits 0 to 4 of byte 0.
    pub fn flags(&self) -> u8 {
        self.byte0 & 0x1f
    }
    /// Whether the OR bit is set.
    pub fn or(&self) -> bool {
        self.byte0 & flag::OR != 0
    }
    /// The comparison word, typed by [`flag::GLOBAL`].
    pub fn comparison(&self) -> Comparison {
        if self.byte0 & flag::GLOBAL != 0 {
            Comparison::Global(self.value)
        } else {
            Comparison::Float(f32::from_bits(self.value))
        }
    }
}

/// Resolves a stored word as a FormID through the plugin `ctx` names
/// (`None` for zero); an error when its master index names no plugin.
pub fn form(ctx: Context<'_>, raw: u32, file: &str, offset: u64) -> Result<Option<FormKey>> {
    ctx.order.remap(ctx.plugin, raw, file, offset)
}

/// One condition: the CTDA and the texts that follow it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Condition {
    /// The CTDA subrecord's index in its record.
    pub index: usize,
    pub ctda: Ctda,
    /// CIS1: bytes before the closing NUL.
    pub cis1: Option<Vec<u8>>,
    /// CIS2: bytes before the closing NUL.
    pub cis2: Option<Vec<u8>>,
}

impl From<&quest::Condition> for Condition {
    fn from(c: &quest::Condition) -> Self {
        Condition {
            index: c.index,
            ctda: Ctda::from_bytes(&c.data),
            cis1: c.cis1.clone(),
            cis2: c.cis2.clone(),
        }
    }
}

/// A run of conditions in a record: CTDA subrecords, each followed by an
/// optional CIS1 and an optional CIS2, not interrupted by another tag.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct List {
    /// The first CTDA's subrecord index.
    pub first: usize,
    /// The subrecord index after the run.
    pub end: usize,
    /// The tag of the subrecord before the run, when there is one.
    pub before: Option<[u8; 4]>,
    /// The tag of the subrecord after the run, when there is one.
    pub after: Option<[u8; 4]>,
    pub conditions: Vec<Condition>,
}

fn text(r: &Record, i: usize, b: &Budget) -> Result<Vec<u8>> {
    let s = &r.subrecords[i];
    let d = r.data(s);
    match d.split_last() {
        Some((0, t)) if !t.contains(&0) => {
            b.charge(t.len() as u64, &r.source.file, s.offset)?;
            Ok(t.to_vec())
        }
        _ => Err(Error::new(
            "ConditionText",
            &r.source.file,
            s.offset,
            format!(
                "{} of bytes ending with one NUL",
                String::from_utf8_lossy(&s.signature)
            ),
            d.len(),
        )),
    }
}

/// Every run of conditions in a record, in stored order. A CIS1 or CIS2
/// that does not follow a CTDA (or a CIS1 after the CIS2 of the same
/// CTDA, a second CIS1 or CIS2) is `ConditionStringOrder`; a CTDA that is
/// not 32 bytes `ConditionSize`; a text without one closing NUL
/// `ConditionText`.
pub fn lists(r: &Record, b: &Budget) -> Result<Vec<List>> {
    let mut out = Vec::new();
    let n = r.subrecords.len();
    let mut i = 0;
    let file = &r.source.file;
    while i < n {
        let s = &r.subrecords[i];
        if s.signature == *b"CIS1" || s.signature == *b"CIS2" {
            return Err(Error::new(
                "ConditionStringOrder",
                file,
                s.offset,
                "CIS1 or CIS2 after a CTDA",
                String::from_utf8_lossy(&s.signature),
            ));
        }
        if s.signature != *b"CTDA" {
            i += 1;
            continue;
        }
        let first = i;
        let before = first
            .checked_sub(1)
            .and_then(|p| r.subrecords.get(p))
            .map(|p| p.signature);
        let mut conditions = Vec::new();
        while let Some(s) = r.subrecords.get(i).filter(|s| s.signature == *b"CTDA") {
            let ctda = Ctda::read(r.data(s), file, s.offset)?;
            let index = i;
            i += 1;
            let mut cis1 = None;
            let mut cis2 = None;
            while let Some(t) = r.subrecords.get(i) {
                match &t.signature {
                    b"CIS1" if cis1.is_none() && cis2.is_none() => cis1 = Some(text(r, i, b)?),
                    b"CIS2" if cis2.is_none() => cis2 = Some(text(r, i, b)?),
                    b"CIS1" | b"CIS2" => {
                        return Err(Error::new(
                            "ConditionStringOrder",
                            file,
                            t.offset,
                            "at most one CIS1 then at most one CIS2 after a CTDA",
                            String::from_utf8_lossy(&t.signature),
                        ))
                    }
                    _ => break,
                }
                i += 1;
            }
            b.reserve(&mut conditions, 1, file, s.offset)?;
            conditions.push(Condition {
                index,
                ctda,
                cis1,
                cis2,
            });
        }
        let after = r.subrecords.get(i).map(|s| s.signature);
        b.reserve(&mut out, 1, file, r.source.offset)?;
        out.push(List {
            first,
            end: i,
            before,
            after,
            conditions,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_core::SourceSpan;

    fn ctda(byte0: u8, function: u16, value: u32) -> [u8; 32] {
        let c = Ctda {
            byte0,
            unused: [0; 3],
            value,
            function,
            pad: [0; 2],
            param1: 0x0102_0304,
            param2: 7,
            run_on: 2,
            reference: 0x14,
            param3: -1,
        };
        c.to_bytes()
    }
    fn record(subs: &[(&[u8; 4], Vec<u8>)]) -> Record {
        let mut payload = Vec::new();
        let mut subrecords = Vec::new();
        for (i, (t, d)) in subs.iter().enumerate() {
            let start = payload.len();
            payload.extend_from_slice(d);
            subrecords.push(crate::Subrecord {
                signature: **t,
                data: start..payload.len(),
                offset: 100 + i as u64,
                extended: false,
            });
        }
        Record {
            signature: *b"INFO",
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
    fn fields_round_trip() {
        let d = ctda(0x65, 59, 1.0f32.to_bits());
        let c = Ctda::from_bytes(&d);
        assert_eq!(c.to_bytes(), d);
        assert_eq!(c.operator(), 3);
        assert_eq!(c.flags(), 0x05);
        assert!(c.or());
        assert_eq!(c.comparison(), Comparison::Global(1.0f32.to_bits()));
        assert_eq!(c.function, 59);
        assert_eq!(c.param3, -1);
        let f = Ctda::from_bytes(&ctda(0x20, 1, 2.5f32.to_bits()));
        assert_eq!(f.comparison(), Comparison::Float(2.5));
        assert!(!f.or());
    }

    #[test]
    fn size_is_checked() {
        let e = Ctda::read(&[0; 28], "t.esm", 9).unwrap_err();
        assert_eq!(e.code, "ConditionSize");
        assert_eq!(e.found, "28");
    }

    #[test]
    fn lists_split_on_other_tags_and_keep_texts() {
        let r = record(&[
            (b"EDID", b"x\0".to_vec()),
            (b"CTDA", ctda(0x01, 58, 0).to_vec()),
            (b"CIS1", b"a\0".to_vec()),
            (b"CIS2", b"b\0".to_vec()),
            (b"CTDA", ctda(0, 59, 0).to_vec()),
            (b"NEXT", Vec::new()),
            (b"CTDA", ctda(0, 74, 0).to_vec()),
            (b"CIS2", b"c\0".to_vec()),
        ]);
        let b = Budget::new(1 << 20);
        let l = lists(&r, &b).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].first, l[0].end), (1, 5));
        assert_eq!(l[0].before, Some(*b"EDID"));
        assert_eq!(l[0].after, Some(*b"NEXT"));
        assert_eq!(l[0].conditions.len(), 2);
        assert_eq!(l[0].conditions[0].cis1.as_deref(), Some(&b"a"[..]));
        assert_eq!(l[0].conditions[0].cis2.as_deref(), Some(&b"b"[..]));
        assert_eq!(l[1].conditions[0].cis1, None);
        assert_eq!(l[1].conditions[0].cis2.as_deref(), Some(&b"c"[..]));
        assert_eq!(l[1].after, None);
    }

    #[test]
    fn strings_out_of_place_are_refused() {
        let b = Budget::new(1 << 20);
        let r = record(&[(b"CIS1", b"a\0".to_vec())]);
        assert_eq!(lists(&r, &b).unwrap_err().code, "ConditionStringOrder");
        let r = record(&[
            (b"CTDA", ctda(0, 1, 0).to_vec()),
            (b"CIS2", b"a\0".to_vec()),
            (b"CIS1", b"b\0".to_vec()),
        ]);
        assert_eq!(lists(&r, &b).unwrap_err().code, "ConditionStringOrder");
        let r = record(&[(b"CTDA", ctda(0, 1, 0).to_vec()), (b"CIS1", b"a".to_vec())]);
        assert_eq!(lists(&r, &b).unwrap_err().code, "ConditionText");
        let r = record(&[(b"CTDA", vec![0; 20])]);
        assert_eq!(lists(&r, &b).unwrap_err().code, "ConditionSize");
    }
}
