//! Scene records (SCEN), read to the last byte as far as a scene's
//! fragments need to be placed in order (`spec/scene-record.md`): the
//! phases with their start and completion conditions, the actors, the
//! actions with their kind, alias, index, flags and start and end phases,
//! the owning quest. Every other subrecord is placed and kept as stored
//! under a named reason ([`Field::unsettled`]).
//!
//! No public page of this layout could be read at this step; the subrecord
//! order and the roles of the tags were recognised from prior general
//! knowledge and are kept only where the census over every winning SCEN
//! of one installation (`scene_census`, `spec/scene-census.txt`) could
//! have refuted them and did not. What a scene does in the running game
//! (when a phase starts and ends, what an action waits for) is behaviour,
//! held as pending rules by the code that relies on it (`fo4_vm::scene`).
use crate::{
    actor::Context,
    condition::{Condition, Ctda},
    typed::FormRef,
    FormKey, Record,
};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Why a subrecord is kept as stored, by where it stands and its tag.
pub const UNSETTLED: &[(&str, [u8; 4], &str)] = &[
    (
        "scene",
        *b"FNAM",
        "u32 flag word, counted; no bit is read here",
    ),
    (
        "scene",
        *b"INAM",
        "u32 counted against the actions' INAM; no meaning read",
    ),
    ("scene", *b"VNAM", "bytes counted; no meaning read"),
    ("scene", *b"CNAM", "bytes counted; no meaning read"),
    ("scene", *b"ACTV", "bytes counted; no meaning read"),
    ("scene", *b"CRIS", "bytes counted; no meaning read"),
    (
        "scene",
        *b"CITC",
        "u32 counted against the conditions after it",
    ),
    (
        "scene",
        *b"KSIZ",
        "u32 counted against KWDA; no meaning read",
    ),
    ("scene", *b"KWDA", "words counted; no meaning read"),
    ("scene", *b"SCQS", "bytes counted; no meaning read"),
    ("scene", *b"NNAM", "text counted; no meaning read"),
    ("scene", *b"TNAM", "bytes counted; no meaning read"),
    ("scene", *b"XNAM", "bytes counted; no meaning read"),
    ("phase", *b"WNAM", "u32 counted; no meaning read"),
    ("phase", *b"FNAM", "bytes counted; no meaning read"),
    ("phase", *b"SCQS", "bytes counted; no meaning read"),
    (
        "actor",
        *b"LNAM",
        "u32 flag word, counted; no bit is read here",
    ),
    (
        "actor",
        *b"DNAM",
        "u32 flag word, counted; no bit is read here",
    ),
    (
        "action",
        *b"FNAM",
        "u32 flag word, counted; no bit is read here",
    ),
    (
        "action",
        *b"CITC",
        "u32 counted against the conditions after it",
    ),
];

/// Why an action subrecord after ENAM is kept as stored.
pub const ACTION_FIELD: &str = "an action field after its end phase: placed and counted by kind and tag (FormID words resolved where the census names the record type they hold); no meaning read";

/// The reason a subrecord at `place` (`scene`, `phase`, `actor`,
/// `action`) is kept as stored, when [`UNSETTLED`] names it.
pub fn unsettled(place: &str, tag: [u8; 4]) -> Option<&'static str> {
    UNSETTLED
        .iter()
        .find(|(p, t, _)| *p == place && *t == tag)
        .map(|(_, _, r)| *r)
}

/// One subrecord in stored order: where it stands, its tag and size, and
/// whether it is typed or kept as stored under a reason.
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub index: usize,
    pub place: &'static str,
    pub tag: [u8; 4],
    pub size: usize,
    pub unsettled: Option<&'static str>,
}

/// One phase: `HNAM NAM0 (CTDA [CIS1] [CIS2])* NEXT (CTDA ..)* NEXT WNAM
/// [FNAM] [SCQS] HNAM`.
#[derive(Debug, Clone, Serialize)]
pub struct Phase {
    /// NAM0: the bytes before the NUL (often empty).
    pub name: Vec<u8>,
    /// The conditions before the first NEXT.
    pub start: Vec<Condition>,
    /// The conditions between the two NEXT.
    pub completion: Vec<Condition>,
    /// WNAM, as stored.
    pub wnam: u32,
    /// FNAM, as stored.
    pub fnam: Option<Vec<u8>>,
    /// SCQS, as stored.
    pub scqs: Option<Vec<u8>>,
}

/// One actor: `ALID LNAM DNAM`.
#[derive(Debug, Clone, Serialize)]
pub struct Actor {
    /// ALID: an alias id of the owning quest.
    pub alias: i32,
    /// LNAM, as stored.
    pub lnam: u32,
    /// DNAM, as stored.
    pub dnam: u32,
}

/// One field of an action after its end phase.
#[derive(Debug, Clone, Serialize)]
pub struct ActionField {
    pub index: usize,
    pub tag: [u8; 4],
    /// The stored bytes.
    pub data: Vec<u8>,
    /// A four-byte field resolved as a FormID through the record's plugin
    /// (any four-byte field: whether it holds one is the census's to say;
    /// a word that is not a FormID of the master context has no key).
    pub form: Option<FormRef>,
}

/// One action: `ANAM(kind) NAM0 ALID INAM [FNAM] SNAM ENAM fields... ANAM`.
#[derive(Debug, Clone, Serialize)]
pub struct Action {
    /// The u16 of the opening ANAM: the action kind, as stored.
    pub kind: u16,
    /// NAM0: the bytes before the NUL.
    pub name: Vec<u8>,
    /// ALID: an alias id of the owning quest.
    pub alias: i32,
    /// INAM: the action's index.
    pub index: u32,
    /// FNAM, as stored.
    pub flags: Option<u32>,
    /// SNAM: the phase the action starts in.
    pub start: u32,
    /// ENAM: the phase the action ends in.
    pub end: u32,
    /// Conditions inside the action (CTDA after a CITC), when any.
    pub conditions: Vec<Condition>,
    /// Every other field, in stored order.
    pub fields: Vec<ActionField>,
}

impl Action {
    /// The first field after the end phase with this tag.
    pub fn field(&self, tag: &[u8; 4]) -> Option<&ActionField> {
        self.fields.iter().find(|f| f.tag == *tag)
    }
    /// The first field after the end phase with this tag, when it is four
    /// bytes, read as a little-endian f32 (what the value means is the
    /// caller's rule to say).
    pub fn float(&self, tag: &[u8; 4]) -> Option<f32> {
        let d: [u8; 4] = self.field(tag)?.data.as_slice().try_into().ok()?;
        Some(f32::from_le_bytes(d))
    }
}

/// One SCEN record, every subrecord placed.
#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    /// The load-order index of the plugin the record was read from.
    pub plugin: usize,
    pub file: String,
    pub editor_id: Option<Vec<u8>>,
    /// Whether the record carries a VMAD (read by `crate::vmad`).
    pub vmad: Option<usize>,
    /// FNAM, as stored.
    pub flags: u32,
    pub phases: Vec<Phase>,
    pub actors: Vec<Actor>,
    pub actions: Vec<Action>,
    /// PNAM: the owning quest.
    pub quest: FormRef,
    /// INAM, as stored.
    pub inam: u32,
    /// Conditions after VNAM (CTDA after CNAM, ACTV or CRIS), when any.
    pub conditions: Vec<Condition>,
    pub fields: Vec<Field>,
}

struct Reader<'a> {
    r: &'a Record,
    ctx: Context<'a>,
    b: &'a Budget,
    i: usize,
    fields: Vec<Field>,
    place: &'static str,
}
impl<'a> Reader<'a> {
    fn file(&self) -> &str {
        &self.r.source.file
    }
    fn offset_at(&self, i: usize) -> u64 {
        self.r
            .subrecords
            .get(i)
            .map_or(self.r.source.offset, |s| s.offset)
    }
    fn peek(&self) -> Option<[u8; 4]> {
        self.r.subrecords.get(self.i).map(|s| s.signature)
    }
    fn size(&self) -> Option<usize> {
        self.r.subrecords.get(self.i).map(|s| s.data.len())
    }
    fn at(&self, tag: &[u8; 4]) -> bool {
        self.peek() == Some(*tag)
    }
    fn order(&self, expected: &str) -> Error {
        let found = match self.peek() {
            Some(t) => String::from_utf8_lossy(&t).into_owned(),
            None => "the end of the record".into(),
        };
        Error::new(
            "SceneOrder",
            self.file(),
            self.offset_at(self.i),
            format!("{expected} (in a {})", self.place),
            found,
        )
    }
    fn push<T>(&self, v: &mut Vec<T>, x: T) -> Result<()> {
        self.b.reserve(v, 1, self.file(), self.offset_at(self.i))?;
        v.push(x);
        Ok(())
    }
    fn take(&mut self, tag: &[u8; 4], expected: &str) -> Result<(usize, &'a [u8])> {
        self.take_as(tag, expected, None)
    }
    fn take_as(
        &mut self,
        tag: &[u8; 4],
        expected: &str,
        reason: Option<&'static str>,
    ) -> Result<(usize, &'a [u8])> {
        if !self.at(tag) {
            return Err(self.order(expected));
        }
        let i = self.i;
        let r: &'a Record = self.r;
        let d = r.data(&r.subrecords[i]);
        let f = Field {
            index: i,
            place: self.place,
            tag: *tag,
            size: d.len(),
            unsettled: reason.or_else(|| unsettled(self.place, *tag)),
        };
        let mut fields = std::mem::take(&mut self.fields);
        self.push(&mut fields, f)?;
        self.fields = fields;
        self.i += 1;
        Ok((i, d))
    }
    fn size_error(&self, i: usize, tag: &[u8; 4], want: usize, got: usize) -> Error {
        Error::new(
            "SceneFieldSize",
            self.file(),
            self.offset_at(i),
            format!("{} of {want} bytes", String::from_utf8_lossy(tag)),
            got,
        )
    }
    fn word(&mut self, tag: &[u8; 4], expected: &str) -> Result<(usize, u32)> {
        let (i, d) = self.take(tag, expected)?;
        let d: [u8; 4] = d
            .try_into()
            .map_err(|_| self.size_error(i, tag, 4, d.len()))?;
        Ok((i, u32::from_le_bytes(d)))
    }
    fn empty(&mut self, tag: &[u8; 4], expected: &str) -> Result<()> {
        let (i, d) = self.take(tag, expected)?;
        if !d.is_empty() {
            return Err(self.size_error(i, tag, 0, d.len()));
        }
        Ok(())
    }
    fn bytes(&mut self, tag: &[u8; 4]) -> Result<Vec<u8>> {
        let (i, d) = self.take(tag, "a field")?;
        self.b
            .charge(d.len() as u64, self.file(), self.offset_at(i))?;
        Ok(d.to_vec())
    }
    fn form_of(&self, i: usize, raw: u32) -> Result<FormRef> {
        Ok(FormRef {
            raw,
            key: self
                .ctx
                .order
                .remap(self.ctx.plugin, raw, self.file(), self.offset_at(i))?,
            subrecord: i,
        })
    }
    fn form(&mut self, tag: &[u8; 4], expected: &str) -> Result<FormRef> {
        let (i, raw) = self.word(tag, expected)?;
        self.form_of(i, raw)
    }
    fn text(&mut self, tag: &[u8; 4]) -> Result<Vec<u8>> {
        let (i, d) = self.take(tag, "text")?;
        match d.split_last() {
            Some((0, t)) if !t.contains(&0) => {
                self.b
                    .charge(t.len() as u64, self.file(), self.offset_at(i))?;
                Ok(t.to_vec())
            }
            _ => Err(Error::new(
                "SceneFieldText",
                self.file(),
                self.offset_at(i),
                format!(
                    "{} of bytes ending with one NUL",
                    String::from_utf8_lossy(tag)
                ),
                d.len(),
            )),
        }
    }
    fn condition(&mut self) -> Result<Condition> {
        let (i, d) = self.take(b"CTDA", "CTDA")?;
        let ctda = Ctda::read(d, &self.r.source.file, self.offset_at(i))?;
        let cis1 = if self.at(b"CIS1") {
            Some(self.text(b"CIS1")?)
        } else {
            None
        };
        let cis2 = if self.at(b"CIS2") {
            Some(self.text(b"CIS2")?)
        } else {
            None
        };
        Ok(Condition {
            index: i,
            ctda,
            cis1,
            cis2,
        })
    }
    fn conditions(&mut self) -> Result<Vec<Condition>> {
        let mut v = Vec::new();
        while self.at(b"CTDA") {
            let c = self.condition()?;
            self.push(&mut v, c)?;
        }
        if self.at(b"CIS1") || self.at(b"CIS2") {
            return Err(self.order("CTDA before CIS1 or CIS2"));
        }
        Ok(v)
    }
    /// An opening or closing marker: `tag` with `size` bytes.
    fn marker(&self, tag: &[u8; 4], size: usize) -> bool {
        self.at(tag) && self.size() == Some(size)
    }
    fn phase(&mut self) -> Result<Phase> {
        self.place = "phase";
        self.empty(b"HNAM", "HNAM")?;
        let name = self.text(b"NAM0")?;
        let start = self.conditions()?;
        self.empty(b"NEXT", "NEXT")?;
        let completion = self.conditions()?;
        self.empty(b"NEXT", "NEXT")?;
        let (_, wnam) = self.word(b"WNAM", "WNAM")?;
        let fnam = if self.at(b"FNAM") {
            Some(self.bytes(b"FNAM")?)
        } else {
            None
        };
        let scqs = if self.at(b"SCQS") {
            Some(self.bytes(b"SCQS")?)
        } else {
            None
        };
        self.empty(b"HNAM", "WNAM, FNAM, SCQS or the closing HNAM")?;
        self.place = "scene";
        Ok(Phase {
            name,
            start,
            completion,
            wnam,
            fnam,
            scqs,
        })
    }
    fn action(&mut self) -> Result<Action> {
        self.place = "action";
        let (i, d) = self.take(b"ANAM", "ANAM")?;
        let kind = u16::from_le_bytes(
            d.try_into()
                .map_err(|_| self.size_error(i, b"ANAM", 2, d.len()))?,
        );
        let name = self.text(b"NAM0")?;
        let (_, alias) = self.word(b"ALID", "ALID")?;
        let (_, index) = self.word(b"INAM", "INAM")?;
        let flags = if self.at(b"FNAM") {
            Some(self.word(b"FNAM", "FNAM")?.1)
        } else {
            None
        };
        let (_, start) = self.word(b"SNAM", "FNAM or SNAM")?;
        let (_, end) = self.word(b"ENAM", "ENAM")?;
        let mut conditions = Vec::new();
        let mut fields = Vec::new();
        loop {
            if self.marker(b"ANAM", 0) {
                self.empty(b"ANAM", "ANAM")?;
                break;
            }
            match self.peek() {
                None => return Err(self.order("the closing ANAM")),
                Some(t) if t == *b"CTDA" => {
                    let mut c = self.conditions()?;
                    conditions.append(&mut c);
                }
                Some(t) if t == *b"CIS1" || t == *b"CIS2" => {
                    return Err(self.order("CTDA before CIS1 or CIS2"))
                }
                Some(t) if t == *b"ANAM" || t == *b"HNAM" => {
                    return Err(self.order("the closing ANAM"))
                }
                Some(t) => {
                    let known = unsettled("action", t);
                    let (k, d) =
                        self.take_as(&t, "a field", Some(known.unwrap_or(ACTION_FIELD)))?;
                    self.b
                        .charge(d.len() as u64, self.file(), self.offset_at(k))?;
                    // A word that is not a valid FormID in the record's
                    // master context is kept with no key: these fields are
                    // not all FormIDs.
                    let form = match <[u8; 4]>::try_from(d) {
                        Ok(w) if known.is_none() => {
                            let raw = u32::from_le_bytes(w);
                            Some(self.form_of(k, raw).unwrap_or(FormRef {
                                raw,
                                key: None,
                                subrecord: k,
                            }))
                        }
                        _ => None,
                    };
                    let f = ActionField {
                        index: k,
                        tag: t,
                        data: d.to_vec(),
                        form,
                    };
                    self.push(&mut fields, f)?;
                }
            }
        }
        self.place = "scene";
        Ok(Action {
            kind,
            name,
            alias: alias as i32,
            index,
            flags,
            start,
            end,
            conditions,
            fields,
        })
    }
}

/// The trailing tags after VNAM, in the measured order; each at most once
/// (conditions between CRIS and KSIZ).
const TRAILER: [[u8; 4]; 9] = [
    *b"CNAM", *b"ACTV", *b"CRIS", *b"KSIZ", *b"KWDA", *b"SCQS", *b"NNAM", *b"TNAM", *b"XNAM",
];

impl Scene {
    /// Reads a SCEN record in the master context `ctx`. The order (every
    /// part required unless in brackets):
    ///
    /// ```text
    /// [EDID] [VMAD] FNAM phase* actor* action* PNAM INAM [VNAM]
    ///   [CNAM] [ACTV] [CRIS] condition* [KSIZ] [KWDA] [SCQS] [NNAM] [TNAM] [XNAM]
    /// phase:  HNAM NAM0 condition* NEXT condition* NEXT WNAM [FNAM] [SCQS] HNAM
    /// actor:  ALID LNAM DNAM
    /// action: ANAM(2 bytes) NAM0 ALID INAM [FNAM] SNAM ENAM (field | condition)* ANAM(0)
    /// ```
    ///
    /// A record of another type is `SceneType`; a subrecord out of this
    /// order `SceneOrder`; a field of another size `SceneFieldSize`; a
    /// text without one closing NUL `SceneFieldText`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Scene> {
        if r.signature != *b"SCEN" {
            return Err(Error::new(
                "SceneType",
                &r.source.file,
                r.source.offset,
                "SCEN",
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let mut x = Reader {
            r,
            ctx,
            b,
            i: 0,
            fields: Vec::new(),
            place: "scene",
        };
        let editor_id = if x.at(b"EDID") {
            Some(x.text(b"EDID")?)
        } else {
            None
        };
        let vmad = if x.at(b"VMAD") {
            let (i, _) = x.take_as(b"VMAD", "VMAD", None)?;
            Some(i)
        } else {
            None
        };
        let (_, flags) = x.word(b"FNAM", "FNAM")?;
        let mut phases = Vec::new();
        while x.marker(b"HNAM", 0) {
            let p = x.phase()?;
            x.push(&mut phases, p)?;
        }
        let mut actors = Vec::new();
        while x.at(b"ALID") {
            x.place = "actor";
            let (_, alias) = x.word(b"ALID", "ALID")?;
            let (_, lnam) = x.word(b"LNAM", "LNAM")?;
            let (_, dnam) = x.word(b"DNAM", "DNAM")?;
            x.place = "scene";
            let a = Actor {
                alias: alias as i32,
                lnam,
                dnam,
            };
            x.push(&mut actors, a)?;
        }
        let mut actions = Vec::new();
        while x.marker(b"ANAM", 2) {
            let a = x.action()?;
            x.push(&mut actions, a)?;
        }
        let quest = x.form(b"PNAM", "a phase, an actor, an action or PNAM")?;
        let (_, inam) = x.word(b"INAM", "INAM")?;
        if x.at(b"VNAM") {
            x.bytes(b"VNAM")?;
        }
        let mut conditions = Vec::new();
        let mut next = 0;
        while let Some(t) = x.peek() {
            if t == *b"CTDA" && next <= 3 {
                let mut c = x.conditions()?;
                conditions.append(&mut c);
                next = 3;
                continue;
            }
            let Some(k) = TRAILER.iter().position(|w| *w == t) else {
                return Err(x.order("a trailing field or the end of the record"));
            };
            if k < next {
                return Err(x.order("trailing fields in the measured order, each once"));
            }
            x.bytes(&t)?;
            next = k + 1;
        }
        Ok(Scene {
            plugin: ctx.plugin,
            file: r.source.file.clone(),
            editor_id,
            vmad,
            flags,
            phases,
            actors,
            actions,
            quest,
            inam,
            conditions,
            fields: x.fields,
        })
    }
    /// The owning quest, resolved.
    pub fn quest_key(&self) -> Option<&FormKey> {
        self.quest.key.as_ref()
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

    fn record(subs: &[(&[u8; 4], Vec<u8>)]) -> Record {
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
            signature: *b"SCEN",
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
    fn w(n: u32) -> Vec<u8> {
        n.to_le_bytes().to_vec()
    }
    fn ctda(function: u16) -> Vec<u8> {
        let mut d = vec![0u8; 32];
        d[8..10].copy_from_slice(&function.to_le_bytes());
        d
    }
    fn scene() -> Vec<(&'static [u8; 4], Vec<u8>)> {
        vec![
            (b"EDID", b"S\0".to_vec()),
            (b"FNAM", w(0x24)),
            // Phase 0: one start condition.
            (b"HNAM", vec![]),
            (b"NAM0", b"\0".to_vec()),
            (b"CTDA", ctda(59)),
            (b"CIS2", b"x\0".to_vec()),
            (b"NEXT", vec![]),
            (b"NEXT", vec![]),
            (b"WNAM", w(500)),
            (b"HNAM", vec![]),
            // Phase 1: one completion condition, FNAM and SCQS.
            (b"HNAM", vec![]),
            (b"NAM0", b"p\0".to_vec()),
            (b"NEXT", vec![]),
            (b"CTDA", ctda(58)),
            (b"NEXT", vec![]),
            (b"WNAM", w(400)),
            (b"FNAM", vec![1, 0]),
            (b"SCQS", w(7)),
            (b"HNAM", vec![]),
            (b"ALID", w(3)),
            (b"LNAM", w(4)),
            (b"DNAM", w(10)),
            // A package action and a start-scene action with a condition.
            (b"ANAM", vec![1, 0]),
            (b"NAM0", b"\0".to_vec()),
            (b"ALID", w(3)),
            (b"INAM", w(1)),
            (b"SNAM", w(0)),
            (b"ENAM", w(1)),
            (b"PNAM", w(0x1234)),
            (b"ANAM", vec![]),
            (b"ANAM", vec![4, 0]),
            (b"NAM0", b"\0".to_vec()),
            (b"ALID", w(u32::MAX)),
            (b"INAM", w(2)),
            (b"FNAM", w(0x8000)),
            (b"SNAM", w(1)),
            (b"ENAM", w(1)),
            (b"LCEP", w(0x99)),
            (b"INTT", vec![0, 0]),
            (b"CITC", w(1)),
            (b"CTDA", ctda(56)),
            (b"ANAM", vec![]),
            (b"PNAM", w(0x55)),
            (b"INAM", w(3)),
            (b"VNAM", vec![0; 16]),
            (b"ACTV", w(0)),
            (b"SCQS", w(0)),
            (b"XNAM", w(0)),
        ]
    }

    #[test]
    fn reads_phases_actors_actions_and_the_trailer() {
        let r = record(&scene());
        let order = order();
        let ctx = Context {
            order: &order,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let s = Scene::read(&r, ctx, &b).unwrap();
        assert_eq!(s.flags, 0x24);
        assert_eq!(s.phases.len(), 2);
        assert_eq!(s.phases[0].start.len(), 1);
        assert_eq!(s.phases[0].start[0].cis2.as_deref(), Some(&b"x"[..]));
        assert!(s.phases[0].completion.is_empty());
        assert_eq!(s.phases[1].completion[0].ctda.function, 58);
        assert_eq!(s.phases[1].fnam.as_deref(), Some(&[1u8, 0][..]));
        assert_eq!(s.actors.len(), 1);
        assert_eq!(s.actions.len(), 2);
        let a = &s.actions[1];
        assert_eq!(
            (a.kind, a.alias, a.index, a.flags),
            (4, -1, 2, Some(0x8000))
        );
        assert_eq!((a.start, a.end), (1, 1));
        assert_eq!(a.conditions[0].ctda.function, 56);
        assert_eq!(
            a.fields.iter().map(|f| f.tag).collect::<Vec<_>>(),
            [*b"LCEP", *b"INTT", *b"CITC"]
        );
        assert_eq!(a.field(b"INTT").map(|f| f.data.len()), Some(2));
        assert_eq!(a.float(b"INTT"), None);
        assert_eq!(a.float(b"NONE"), None);
        assert_eq!(s.actions[0].float(b"PNAM"), Some(f32::from_bits(0x1234)));
        assert_eq!(s.quest.raw, 0x55);
        // Every subrecord placed exactly once.
        assert_eq!(s.fields.len(), r.subrecords.len());
        for (i, f) in s.fields.iter().enumerate() {
            assert_eq!(f.index, i);
        }
    }

    #[test]
    fn out_of_order_and_bad_sizes_are_refused() {
        let order = order();
        let ctx = Context {
            order: &order,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let full = scene();
        // Every truncation that cuts an element is refused.
        for n in 3..full.len() - 5 {
            let r = record(&full[..n]);
            assert!(Scene::read(&r, ctx, &b).is_err(), "{n}");
        }
        // A trailing field out of the measured order.
        let mut v = full.clone();
        let l = v.len();
        v.swap(l - 2, l - 3);
        let e = Scene::read(&record(&v), ctx, &b).unwrap_err();
        assert_eq!(e.code, "SceneOrder");
        // A phase with a WNAM of 2 bytes.
        let mut v = full.clone();
        v[8].1 = vec![0, 0];
        let e = Scene::read(&record(&v), ctx, &b).unwrap_err();
        assert_eq!(e.code, "SceneFieldSize");
        // A NEXT that is not empty.
        let mut v = full;
        v[6].1 = vec![0];
        let e = Scene::read(&record(&v), ctx, &b).unwrap_err();
        assert_eq!(e.code, "SceneFieldSize");
    }
}
