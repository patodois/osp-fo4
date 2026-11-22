//! VMAD, the subrecord that attaches compiled scripts (`.pex` objects) to a
//! record, with the values of their properties (`spec/vmad.md`).
//!
//! Every field here was measured over every VMAD subrecord of the winning
//! records of one installation (`vmad_census`); the layout was recognised
//! from prior general knowledge before it was measured and is kept only
//! where that census holds (see `docs/provenance.md`). The script list is
//! read for every record type. What follows it is read only for TERM (its
//! fragment block) and QUST (its stage fragments and aliases); for every
//! other record type the bytes after the script list are kept in `tail` as
//! stored.
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Bytes 0..2 of every measured VMAD but one (5).
pub const VERSION: u16 = 6;
/// Bytes 2..4 of every measured VMAD.
pub const OBJECT_FORMAT: u16 = 2;
/// Deepest struct nesting read; the measured data never nests a struct in
/// a struct.
pub const MAX_DEPTH: usize = 4;

/// An object value: three stored words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObjectRef {
    /// u16, 0 in every measured value.
    pub zero: u16,
    /// i16: -1 in most values; non-negative in 3,903 (3,375 of them on
    /// QUST). A label: what it indexes is not read here.
    pub alias: i16,
    /// Plugin-local FormID (resolve it through the record's plugin).
    pub form_id: u32,
}

/// A stored property value by kind.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PropertyValue<'a> {
    /// Kind 0, no payload.
    None,
    /// Kind 1.
    Object(ObjectRef),
    /// Kind 2: u16-prefixed text.
    String(&'a [u8]),
    /// Kind 3.
    Int(i32),
    /// Kind 4.
    Float(f32),
    /// Kind 5: one byte, 0 or 1 in every measured value.
    Bool(bool),
    /// Kind 7: u32 count of member properties.
    Struct(Vec<Property<'a>>),
    /// Kinds 11 to 15: u32 count, then values of kind 1 to 5.
    Array(u8, Vec<PropertyValue<'a>>),
    /// Kind 17: u32 count of structs, each as kind 7.
    StructArray(Vec<Vec<Property<'a>>>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Property<'a> {
    pub name: &'a [u8],
    pub kind: u8,
    /// One byte after the kind: 1 in almost every measured property.
    pub flags: u8,
    pub value: PropertyValue<'a>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Script<'a> {
    /// The `.pex` object name (`:` separates folders under `scripts/`).
    pub name: &'a [u8],
    /// One byte after the name.
    pub flags: u8,
    pub properties: Vec<Property<'a>>,
}

/// One entry of a TERM fragment block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TerminalFragment<'a> {
    /// u32: an index (1-based in every measured entry).
    pub index: u32,
    /// One byte, 1 in every measured entry.
    pub byte: u8,
    /// The script holding the function (the block's script in every measured
    /// entry).
    pub script: &'a [u8],
    pub function: &'a [u8],
}

/// The block after the script list of a TERM VMAD.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TerminalFragments<'a> {
    /// One byte, 3 in every measured block.
    pub version: u8,
    /// The fragment script with its properties, laid out as a script entry.
    pub script: Script<'a>,
    pub fragments: Vec<TerminalFragment<'a>>,
}

/// One stage fragment of a QUST block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestFragment<'a> {
    /// u16: a stage index.
    pub stage: u16,
    /// u16 after the stage.
    pub word: u16,
    /// i32: a log entry index.
    pub log_entry: i32,
    /// One byte before the texts.
    pub byte: u8,
    pub script: &'a [u8],
    pub function: &'a [u8],
}

/// One alias entry of a QUST block: the alias named by an object value
/// (the quest's FormID or 0, and the alias index), then a script list laid
/// out as a VMAD's.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestAlias<'a> {
    pub object: ObjectRef,
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
}

/// The block after the script list of a QUST VMAD.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestBlock<'a> {
    /// One byte, 3 in every measured block.
    pub version: u8,
    /// The fragment script: a script entry, or only an empty name (then no
    /// flags and no properties are stored).
    pub script: Script<'a>,
    pub fragments: Vec<QuestFragment<'a>>,
    pub aliases: Vec<QuestAlias<'a>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Vmad<'a> {
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
    /// Read for TERM records only.
    pub terminal: Option<TerminalFragments<'a>>,
    /// Read for QUST records only.
    pub quest: Option<QuestBlock<'a>>,
    /// Bytes after what was read, kept as stored: empty in every measured
    /// VMAD except those of INFO, PACK, PERK and SCEN records, whose blocks
    /// are not read.
    pub tail: &'a [u8],
}

struct R<'a, 'b> {
    d: &'a [u8],
    p: usize,
    file: &'b str,
    base: u64,
    b: &'b Budget,
}
impl<'a> R<'a, '_> {
    fn err(&self, code: &str, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(code, self.file, self.base + self.p as u64, expected, found)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.d.len() - self.p {
            return Err(self.err("Truncated", format!("{n} bytes"), self.d.len() - self.p));
        }
        let v = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        let v = self.take(2)?;
        Ok(u16::from_le_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let v = self.take(4)?;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn text(&mut self) -> Result<&'a [u8]> {
        let n = self.u16()?;
        self.take(n.into())
    }
    /// A count of elements of at least `min` bytes, checked against what is
    /// left before anything is reserved.
    fn vec<T>(&mut self, n: usize, min: usize) -> Result<Vec<T>> {
        if n.saturating_mul(min) > self.d.len() - self.p {
            return Err(self.err(
                "Truncated",
                format!("{n} elements of at least {min} bytes"),
                self.d.len() - self.p,
            ));
        }
        let mut v = Vec::new();
        self.b
            .reserve(&mut v, n, self.file, self.base + self.p as u64)?;
        Ok(v)
    }
    fn simple(&mut self, kind: u8) -> Result<PropertyValue<'a>> {
        Ok(match kind {
            0 => PropertyValue::None,
            1 => PropertyValue::Object(self.object()?),
            2 => PropertyValue::String(self.text()?),
            3 => PropertyValue::Int(self.u32()? as i32),
            4 => PropertyValue::Float(f32::from_bits(self.u32()?)),
            5 => {
                let v = self.u8()?;
                if v > 1 {
                    return Err(self.err("BoolValue", "0 or 1", v));
                }
                PropertyValue::Bool(v == 1)
            }
            k => return Err(self.err("PropertyKind", "0..=5, 7, 11..=15 or 17", k)),
        })
    }
    fn members(&mut self, depth: usize) -> Result<Vec<Property<'a>>> {
        if depth > MAX_DEPTH {
            return Err(self.err("StructDepth", format!("<= {MAX_DEPTH}"), depth));
        }
        let n = self.u32()? as usize;
        let mut v = self.vec(n, 5)?;
        for _ in 0..n {
            v.push(self.property(depth + 1)?);
        }
        Ok(v)
    }
    fn property(&mut self, depth: usize) -> Result<Property<'a>> {
        let name = self.text()?;
        let kind = self.u8()?;
        let flags = self.u8()?;
        let value = match kind {
            0..=5 => self.simple(kind)?,
            7 => PropertyValue::Struct(self.members(depth)?),
            11..=15 => {
                let n = self.u32()? as usize;
                let mut v = self.vec(n, 1)?;
                for _ in 0..n {
                    v.push(self.simple(kind - 10)?);
                }
                PropertyValue::Array(kind - 10, v)
            }
            17 => {
                let n = self.u32()? as usize;
                let mut v = self.vec(n, 4)?;
                for _ in 0..n {
                    v.push(self.members(depth)?);
                }
                PropertyValue::StructArray(v)
            }
            k => return Err(self.err("PropertyKind", "0..=5, 7, 11..=15 or 17", k)),
        };
        Ok(Property {
            name,
            kind,
            flags,
            value,
        })
    }
    fn script(&mut self) -> Result<Script<'a>> {
        let name = self.text()?;
        let flags = self.u8()?;
        let n = self.u16()? as usize;
        let mut properties = self.vec(n, 4)?;
        for _ in 0..n {
            properties.push(self.property(0)?);
        }
        Ok(Script {
            name,
            flags,
            properties,
        })
    }
    fn script_list(&mut self) -> Result<Vec<Script<'a>>> {
        let n = self.u16()? as usize;
        let mut scripts = self.vec(n, 5)?;
        for _ in 0..n {
            scripts.push(self.script()?);
        }
        Ok(scripts)
    }
    fn object(&mut self) -> Result<ObjectRef> {
        Ok(ObjectRef {
            zero: self.u16()?,
            alias: self.u16()? as i16,
            form_id: self.u32()?,
        })
    }
    fn quest(&mut self) -> Result<QuestBlock<'a>> {
        let version = self.u8()?;
        let n = self.u16()? as usize;
        let name = self.text()?;
        let script = if name.is_empty() {
            Script {
                name,
                flags: 0,
                properties: Vec::new(),
            }
        } else {
            let flags = self.u8()?;
            let k = self.u16()? as usize;
            let mut properties = self.vec(k, 4)?;
            for _ in 0..k {
                properties.push(self.property(0)?);
            }
            Script {
                name,
                flags,
                properties,
            }
        };
        let mut fragments = self.vec(n, 13)?;
        for _ in 0..n {
            fragments.push(QuestFragment {
                stage: self.u16()?,
                word: self.u16()?,
                log_entry: self.u32()? as i32,
                byte: self.u8()?,
                script: self.text()?,
                function: self.text()?,
            });
        }
        let k = self.u16()? as usize;
        let mut aliases = self.vec(k, 14)?;
        for _ in 0..k {
            let object = self.object()?;
            let version = self.u16()?;
            let object_format = self.u16()?;
            if object_format != OBJECT_FORMAT {
                return Err(self.err("VmadObjectFormat", OBJECT_FORMAT, object_format));
            }
            let scripts = self.script_list()?;
            aliases.push(QuestAlias {
                object,
                version,
                object_format,
                scripts,
            });
        }
        Ok(QuestBlock {
            version,
            script,
            fragments,
            aliases,
        })
    }
    fn terminal(&mut self) -> Result<TerminalFragments<'a>> {
        let version = self.u8()?;
        let script = self.script()?;
        let n = self.u16()? as usize;
        let mut fragments = self.vec(n, 9)?;
        for _ in 0..n {
            fragments.push(TerminalFragment {
                index: self.u32()?,
                byte: self.u8()?,
                script: self.text()?,
                function: self.text()?,
            });
        }
        Ok(TerminalFragments {
            version,
            script,
            fragments,
        })
    }
}

impl<'a> Vmad<'a> {
    /// Reads one VMAD payload of a record of type `signature`. `file` and
    /// `offset` locate the payload for errors.
    pub fn read(
        d: &'a [u8],
        signature: [u8; 4],
        file: &str,
        offset: u64,
        b: &Budget,
    ) -> Result<Self> {
        let mut r = R {
            d,
            p: 0,
            file,
            base: offset,
            b,
        };
        let version = r.u16()?;
        let object_format = r.u16()?;
        if object_format != OBJECT_FORMAT {
            return Err(r.err("VmadObjectFormat", OBJECT_FORMAT, object_format));
        }
        let scripts = r.script_list()?;
        let terminal = if signature == *b"TERM" && r.p < d.len() {
            Some(r.terminal()?)
        } else {
            None
        };
        let quest = if signature == *b"QUST" && r.p < d.len() {
            Some(r.quest()?)
        } else {
            None
        };
        Ok(Vmad {
            version,
            object_format,
            scripts,
            terminal,
            quest,
            tail: &d[r.p..],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &mut Vec<u8>, t: &str) {
        v.extend((t.len() as u16).to_le_bytes());
        v.extend(t.as_bytes());
    }
    /// Two scripts: one with an object, an int array and a struct
    /// property, one without properties.
    fn fixture() -> Vec<u8> {
        let mut v = vec![6, 0, 2, 0, 2, 0];
        text(&mut v, "A:B");
        v.push(0);
        v.extend(3u16.to_le_bytes());
        text(&mut v, "Target");
        v.extend([1, 1, 0, 0, 0xff, 0xff, 0x34, 0x12, 0, 0]);
        text(&mut v, "Counts");
        v.extend([13, 1]);
        v.extend(2u32.to_le_bytes());
        v.extend(7i32.to_le_bytes());
        v.extend((-1i32).to_le_bytes());
        text(&mut v, "Pair");
        v.extend([7, 1]);
        v.extend(1u32.to_le_bytes());
        text(&mut v, "on");
        v.extend([5, 1, 1]);
        text(&mut v, "C");
        v.extend([0, 0, 0]);
        v
    }

    #[test]
    fn reads_scripts_properties_and_values() {
        let d = fixture();
        let b = Budget::new(1 << 16);
        let m = Vmad::read(&d, *b"ACTI", "t", 0, &b).unwrap();
        assert_eq!(m.scripts.len(), 2);
        let p = &m.scripts[0].properties;
        assert_eq!(m.scripts[0].name, b"A:B");
        assert_eq!(
            p[0].value,
            PropertyValue::Object(ObjectRef {
                zero: 0,
                alias: -1,
                form_id: 0x1234
            })
        );
        assert_eq!(
            p[1].value,
            PropertyValue::Array(3, vec![PropertyValue::Int(7), PropertyValue::Int(-1)])
        );
        assert!(
            matches!(&p[2].value, PropertyValue::Struct(m) if m[0].value == PropertyValue::Bool(true))
        );
        assert!(m.tail.is_empty() && m.terminal.is_none());
        for n in 0..d.len() {
            if let Ok(m) = Vmad::read(&d[..n], *b"ACTI", "t", 0, &b) {
                panic!("{n}: {m:?}");
            }
        }
    }

    #[test]
    fn reads_the_terminal_block_and_keeps_other_tails() {
        let mut d = vec![6, 0, 2, 0, 0, 0, 3];
        text(&mut d, "F:T");
        d.extend([0, 0, 0, 1, 0, 1, 0, 0, 0, 1]);
        text(&mut d, "F:T");
        text(&mut d, "Fragment_1");
        let b = Budget::new(1 << 16);
        let m = Vmad::read(&d, *b"TERM", "t", 0, &b).unwrap();
        let t = m.terminal.unwrap();
        assert_eq!((t.version, t.script.name), (3, &b"F:T"[..]));
        assert_eq!(t.fragments[0].function, b"Fragment_1");
        assert!(m.tail.is_empty());
        let m = Vmad::read(&d, *b"INFO", "t", 0, &b).unwrap();
        assert_eq!(m.tail.len(), d.len() - 6);
        d[2] = 3;
        assert_eq!(
            Vmad::read(&d, *b"TERM", "t", 0, &b).unwrap_err().code,
            "VmadObjectFormat"
        );
    }

    /// A QUST VMAD: one script, then the block: version 3, two stage
    /// fragments of the block script, two aliases (one with a script).
    fn quest_fixture(empty_block_script: bool) -> Vec<u8> {
        let mut d = vec![6, 0, 2, 0, 1, 0];
        text(&mut d, "Q");
        d.extend([0, 0, 0]);
        d.push(3);
        let frags: u16 = if empty_block_script { 0 } else { 2 };
        d.extend(frags.to_le_bytes());
        if empty_block_script {
            text(&mut d, "");
        } else {
            text(&mut d, "F:QF");
            d.extend([0, 1, 0]);
            text(&mut d, "Alias_A");
            d.extend([1, 1, 0, 0, 4, 0, 0x10, 0, 0, 0]);
            for (stage, f) in [(10u16, "Fragment_Stage_0010"), (20, "Fragment_Stage_0020")] {
                d.extend(stage.to_le_bytes());
                d.extend([0, 0, 0, 0, 0, 0, 1]);
                text(&mut d, "F:QF");
                text(&mut d, f);
            }
        }
        d.extend(2u16.to_le_bytes());
        for (alias, scripts) in [(4i16, 1u16), (7, 0)] {
            d.extend([0, 0]);
            d.extend(alias.to_le_bytes());
            d.extend(0x10u32.to_le_bytes());
            d.extend([6, 0, 2, 0]);
            d.extend(scripts.to_le_bytes());
            if scripts == 1 {
                text(&mut d, "AliasScript");
                d.extend([0, 1, 0]);
                text(&mut d, "Count");
                d.extend([3, 1]);
                d.extend(5i32.to_le_bytes());
            }
        }
        d
    }

    #[test]
    fn reads_the_quest_block_to_the_last_byte() {
        let b = Budget::new(1 << 16);
        let d = quest_fixture(false);
        let m = Vmad::read(&d, *b"QUST", "t", 0, &b).unwrap();
        assert!(m.tail.is_empty());
        let q = m.quest.unwrap();
        assert_eq!((q.version, q.script.name), (3, &b"F:QF"[..]));
        assert_eq!(q.script.properties.len(), 1);
        assert_eq!(
            q.fragments.iter().map(|f| f.stage).collect::<Vec<_>>(),
            [10, 20]
        );
        assert_eq!(q.fragments[1].function, b"Fragment_Stage_0020");
        assert_eq!(q.aliases.len(), 2);
        assert_eq!(
            (q.aliases[0].object.alias, q.aliases[0].object.form_id),
            (4, 0x10)
        );
        assert_eq!(q.aliases[0].scripts[0].name, b"AliasScript");
        assert_eq!(
            q.aliases[0].scripts[0].properties[0].value,
            PropertyValue::Int(5)
        );
        assert!(q.aliases[1].scripts.is_empty());
        // An empty block script name stores no flags and no properties.
        let d = quest_fixture(true);
        let q = Vmad::read(&d, *b"QUST", "t", 0, &b).unwrap().quest.unwrap();
        assert!(q.script.name.is_empty() && q.fragments.is_empty());
        assert_eq!(q.aliases.len(), 2);
        // Every truncation is refused; the same bytes on another record
        // type stay a tail.
        let d = quest_fixture(false);
        // (Twelve bytes end the script list: no block follows.)
        for n in (7..12).chain(13..d.len()) {
            assert!(Vmad::read(&d[..n], *b"QUST", "t", 0, &b).is_err(), "{n}");
        }
        assert!(Vmad::read(&d[..12], *b"QUST", "t", 0, &b)
            .unwrap()
            .quest
            .is_none());
        assert_eq!(
            Vmad::read(&d, *b"SCEN", "t", 0, &b).unwrap().tail.len(),
            d.len() - 12
        );
    }

    #[test]
    fn unknown_kinds_are_refused() {
        let mut d = vec![6, 0, 2, 0, 1, 0];
        text(&mut d, "S");
        d.extend([0, 1, 0]);
        text(&mut d, "p");
        d.extend([9, 1]);
        let b = Budget::new(1 << 16);
        assert_eq!(
            Vmad::read(&d, *b"ACTI", "t", 0, &b).unwrap_err().code,
            "PropertyKind"
        );
    }
}
