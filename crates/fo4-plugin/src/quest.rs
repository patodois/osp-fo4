//! The quest record (QUST) read to its last byte (`spec/quest.md`).
//!
//! Every subrecord of the record is placed by an order grammar measured
//! over every winning QUST of one installation (`quest_census`): a header
//! (EDID, VMAD, FULL, DNAM, ENAM, LNAM, XNAM, QTGL, FLTR), two condition
//! lists split by NEXT, the stages (INDX) with their log entries (QSDT),
//! the objectives (QOBJ) with their targets (QSTA), ANAM, the aliases
//! (ALST, ALLS or ALCS ... ALED) and two closing subrecords (GNAM, SNAM).
//! The field names and the layout were recognised from prior general
//! knowledge and are kept only where that census holds (sizes, counts,
//! FormIDs that resolve to records of one type, alias indices that name an
//! alias of the quest); see `docs/provenance.md`. A subrecord whose bytes
//! are not given meaning here (CTDA, a condition, and VMAD, read by
//! [`crate::vmad`]) is kept as stored with its tag and size and counted as
//! raw in [`Quest::fields`]. Anything that does not fit is a named error
//! with the file, offset, expected and found.
//!
//! Nothing here says what a stage, flag or alias does in the running game:
//! that is behaviour, registered as pending rules where code relies on it.
use crate::{actor::Context, typed::FormRef, Record, LOCALIZED};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// One subrecord of the record, in stored order, and whether its bytes
/// are given a field here (`typed`) or kept as stored (raw).
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub index: usize,
    pub tag: [u8; 4],
    pub size: usize,
    pub typed: bool,
}

/// A text field: a string table id (u32) in a localized plugin, the bytes
/// before the closing NUL otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Text {
    Id(u32),
    Bytes(Vec<u8>),
}

/// A condition: CTDA (32 bytes, kept as stored) and the CIS1 and CIS2
/// texts that follow it (bytes before the NUL).
#[derive(Debug, Clone, Serialize)]
pub struct Condition {
    pub index: usize,
    pub data: [u8; 32],
    pub cis1: Option<Vec<u8>>,
    pub cis2: Option<Vec<u8>>,
}

/// DNAM, 12 bytes.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct General {
    /// u16 at 0: flag bits; which bit means what is not read here.
    pub flags: u16,
    /// u8 at 2.
    pub priority: u8,
    /// u8 at 3, kept as stored.
    pub byte3: u8,
    /// u32 at 4: zero in every measured record.
    pub word: u32,
    /// u32 at 8: small values (a quest type), kept as stored.
    pub kind: u32,
}

/// One log entry of a stage: QSDT, its conditions, NAM2 and CNAM.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// The QSDT subrecord.
    pub index: usize,
    /// QSDT, one byte of flag bits.
    pub flags: u8,
    pub conditions: Vec<Condition>,
    /// NAM2: a text (bytes before the NUL).
    pub note: Option<Vec<u8>>,
    /// CNAM: a text.
    pub text: Option<Text>,
}

/// One stage: INDX and its log entries.
#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    /// The INDX subrecord.
    pub index: usize,
    /// INDX u16 at 0: the stage index scripts name.
    pub stage: u16,
    /// INDX u8 at 2: flag bits.
    pub flags: u8,
    /// INDX u8 at 3, kept as stored.
    pub byte3: u8,
    pub entries: Vec<LogEntry>,
}

/// One target of an objective: QSTA (12 bytes) and its conditions.
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub index: usize,
    /// i32 at 0: an alias index of the quest.
    pub alias: i32,
    /// u32 at 4: flag bits.
    pub flags: u32,
    /// u32 at 8: zero or a KYWD.
    pub keyword: FormRef,
    pub conditions: Vec<Condition>,
}

/// One objective: QOBJ, FNAM, NNAM and its targets.
#[derive(Debug, Clone, Serialize)]
pub struct Objective {
    pub index: usize,
    /// QOBJ u16: the objective index scripts name.
    pub objective: u16,
    /// FNAM u32: flag bits.
    pub flags: u32,
    /// NNAM: a text.
    pub text: Option<Text>,
    pub targets: Vec<Target>,
}

/// What kind of alias an entry starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AliasKind {
    /// ALST.
    Reference,
    /// ALLS.
    Location,
    /// ALCS (its own u32) and ALMI (u8), then ALST.
    Collection { collection: u32, max_initial: u8 },
}

/// One value of an alias entry, by tag.
#[derive(Debug, Clone, Serialize)]
pub enum AliasValue {
    /// ALFR: a placed reference.
    ForcedReference(FormRef),
    /// ALUA: an NPC_.
    UniqueActor(FormRef),
    /// ALFL: an LCTN.
    ForcedLocation(FormRef),
    /// ALFA (i32 alias index of the quest), then optionally ALRT (an LCRT).
    LocationAliasReference {
        alias: i32,
        ref_type: Option<FormRef>,
    },
    /// ALFE then ALFD: two four-byte codes, kept as stored.
    FromEvent { event: [u8; 4], data: [u8; 4] },
    /// ALCO (a base record), ALCA (u16 alias index, u16 word), ALCL (u32).
    CreateObject {
        object: FormRef,
        alias: u16,
        word: u16,
        level: u32,
    },
    /// ALEQ (a QUST) then ALEA (i32 alias index of that quest).
    External { quest: FormRef, alias: i32 },
    /// ALFI: i32 alias index of the quest.
    Alfi(i32),
    /// ALNA (i32 alias index) then ALNT (u32, 0 or 1 in every measured
    /// entry).
    LinkedAlias { alias: i32, word: u32 },
    /// ALCC: u32, kept as stored.
    Alcc(u32),
    /// A four-byte FormID under its tag: ALDN, ALFC, ALSP, ALPC, ECOR,
    /// SPOR, ALFV, ALDI, VTCK.
    Form([u8; 4], FormRef),
    /// KSIZ (u32 count) then KWDA (that many KYWD).
    Keywords(Vec<FormRef>),
    /// COCT (u32 count) then that many CNTO (FormID, i32 count).
    Items(Vec<(FormRef, i32)>),
    /// ALLA: pairs of a FormID (zero or a KYWD) and an i32 alias index.
    KeywordAliases(Vec<(FormRef, i32)>),
    /// CTDA and its texts.
    Condition(Condition),
}
impl AliasValue {
    /// The tag that starts this value.
    pub fn tag(&self) -> [u8; 4] {
        match self {
            AliasValue::ForcedReference(_) => *b"ALFR",
            AliasValue::UniqueActor(_) => *b"ALUA",
            AliasValue::ForcedLocation(_) => *b"ALFL",
            AliasValue::LocationAliasReference { .. } => *b"ALFA",
            AliasValue::FromEvent { .. } => *b"ALFE",
            AliasValue::CreateObject { .. } => *b"ALCO",
            AliasValue::External { .. } => *b"ALEQ",
            AliasValue::Alfi(_) => *b"ALFI",
            AliasValue::LinkedAlias { .. } => *b"ALNA",
            AliasValue::Alcc(_) => *b"ALCC",
            AliasValue::Form(t, _) => *t,
            AliasValue::Keywords(_) => *b"KSIZ",
            AliasValue::Items(_) => *b"COCT",
            AliasValue::KeywordAliases(_) => *b"ALLA",
            AliasValue::Condition(_) => *b"CTDA",
        }
    }
}

/// One alias entry, from its first subrecord to ALED.
#[derive(Debug, Clone, Serialize)]
pub struct Alias {
    /// The first subrecord (ALCS, ALST or ALLS).
    pub index: usize,
    pub kind: AliasKind,
    /// ALST or ALLS: the alias index (the number VMAD alias values and
    /// alias index fields name).
    pub id: u32,
    /// ALID: a name (bytes before the NUL).
    pub name: Vec<u8>,
    /// FNAM u32: flag bits.
    pub flags: u32,
    /// The values between FNAM and ALED, in stored order.
    pub values: Vec<AliasValue>,
}
impl Alias {
    /// The ALFR value, when there is one.
    pub fn forced_reference(&self) -> Option<&FormRef> {
        self.values.iter().find_map(|v| match v {
            AliasValue::ForcedReference(f) => Some(f),
            _ => None,
        })
    }
}

/// A QUST record.
#[derive(Debug, Clone, Serialize)]
pub struct Quest {
    /// EDID (bytes before the NUL).
    pub editor_id: Option<Vec<u8>>,
    /// The VMAD subrecord (read by [`crate::vmad::Vmad`]).
    pub vmad: Option<usize>,
    /// FULL.
    pub name: Option<Text>,
    /// DNAM.
    pub general: Option<General>,
    /// ENAM: a four-byte code, kept as stored.
    pub event: Option<[u8; 4]>,
    /// LNAM: an LCTN.
    pub location: Option<FormRef>,
    /// XNAM: a GLOB.
    pub xnam: Option<FormRef>,
    /// QTGL: GLOBs, in order.
    pub text_globals: Vec<FormRef>,
    /// FLTR: a text (bytes before the NUL).
    pub filter: Option<Vec<u8>>,
    /// Conditions before NEXT.
    pub conditions: Vec<Condition>,
    /// The NEXT subrecord (no bytes), when present.
    pub next: Option<usize>,
    /// Conditions after NEXT.
    pub event_conditions: Vec<Condition>,
    pub stages: Vec<Stage>,
    pub objectives: Vec<Objective>,
    /// ANAM: u32.
    pub next_alias: Option<u32>,
    pub aliases: Vec<Alias>,
    /// GNAM: a KYWD.
    pub gnam: Option<FormRef>,
    /// SNAM: a text (bytes before the NUL).
    pub snam: Option<Vec<u8>>,
    /// Every subrecord in stored order, typed or raw.
    pub fields: Vec<Field>,
}
impl Quest {
    /// The stage of that index, when the record holds it.
    pub fn stage(&self, stage: u16) -> Option<&Stage> {
        self.stages.iter().find(|s| s.stage == stage)
    }
    /// The alias entry of that index (ALST or ALLS value).
    pub fn alias(&self, id: u32) -> Option<&Alias> {
        self.aliases.iter().find(|a| a.id == id)
    }
    /// The objective of that index.
    pub fn objective(&self, objective: u16) -> Option<&Objective> {
        self.objectives.iter().find(|o| o.objective == objective)
    }
}

/// Alias values that are a single four-byte FormID under their own tag.
const ALIAS_FORMS: [&[u8; 4]; 9] = [
    b"ALDN", b"ALFC", b"ALSP", b"ALPC", b"ECOR", b"SPOR", b"ALFV", b"ALDI", b"VTCK",
];
/// Every tag that may start a value inside an alias entry.
const ALIAS_STARTS: &str = "ALFR ALUA ALFL ALFA ALFE ALCO ALEQ ALFI ALNA ALCC ALDN ALFC ALSP ALPC ECOR SPOR ALFV ALDI VTCK KSIZ COCT ALLA CTDA or ALED";

struct Q<'a> {
    r: &'a Record,
    ctx: Context<'a>,
    b: &'a Budget,
    localized: bool,
    i: usize,
    fields: Vec<Field>,
}
impl<'a> Q<'a> {
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
    fn at(&self, tag: &[u8; 4]) -> bool {
        self.peek() == Some(*tag)
    }
    fn order(&self, expected: &str) -> Error {
        let found = match self.peek() {
            Some(t) => String::from_utf8_lossy(&t).into_owned(),
            None => "the end of the record".into(),
        };
        Error::new(
            "QuestSubrecordOrder",
            self.file(),
            self.offset_at(self.i),
            expected,
            found,
        )
    }
    fn push<T>(&self, v: &mut Vec<T>, x: T) -> Result<()> {
        self.b.reserve(v, 1, self.file(), self.offset_at(self.i))?;
        v.push(x);
        Ok(())
    }
    /// Takes the next subrecord, which must be `tag`, and records it.
    fn take(&mut self, tag: &[u8; 4], typed: bool, expected: &str) -> Result<(usize, &'a [u8])> {
        if !self.at(tag) {
            return Err(self.order(expected));
        }
        let i = self.i;
        let r: &'a Record = self.r;
        let d = r.data(&r.subrecords[i]);
        let f = Field {
            index: i,
            tag: *tag,
            size: d.len(),
            typed,
        };
        let mut fields = std::mem::take(&mut self.fields);
        self.push(&mut fields, f)?;
        self.fields = fields;
        self.i += 1;
        Ok((i, d))
    }
    fn sized(&self, i: usize, d: &'a [u8], size: usize) -> Result<&'a [u8]> {
        if d.len() != size {
            return Err(Error::new(
                "QuestFieldSize",
                self.file(),
                self.offset_at(i),
                format!(
                    "QUST.{} of {size} bytes",
                    String::from_utf8_lossy(&self.r.subrecords[i].signature)
                ),
                d.len(),
            ));
        }
        Ok(d)
    }
    fn u32_of(&self, i: usize, d: &'a [u8]) -> Result<u32> {
        let d = self.sized(i, d, 4)?;
        Ok(u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
    }
    fn word(&mut self, tag: &[u8; 4], expected: &str) -> Result<(usize, u32)> {
        let (i, d) = self.take(tag, true, expected)?;
        Ok((i, self.u32_of(i, d)?))
    }
    fn code(&mut self, tag: &[u8; 4], expected: &str) -> Result<[u8; 4]> {
        let (i, d) = self.take(tag, true, expected)?;
        let d = self.sized(i, d, 4)?;
        Ok([d[0], d[1], d[2], d[3]])
    }
    fn reference(&self, i: usize, raw: u32) -> Result<FormRef> {
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
        let (i, v) = self.word(tag, expected)?;
        self.reference(i, v)
    }
    fn zstring(&self, i: usize, d: &'a [u8]) -> Result<Vec<u8>> {
        match d.split_last() {
            Some((0, text)) if !text.contains(&0) => {
                self.b
                    .charge(text.len() as u64, self.file(), self.offset_at(i))?;
                Ok(text.to_vec())
            }
            _ => Err(Error::new(
                "QuestFieldText",
                self.file(),
                self.offset_at(i),
                "bytes ending with one NUL",
                d.len(),
            )),
        }
    }
    fn text_bytes(&mut self, tag: &[u8; 4], expected: &str) -> Result<Vec<u8>> {
        let (i, d) = self.take(tag, true, expected)?;
        self.zstring(i, d)
    }
    fn text(&mut self, tag: &[u8; 4], expected: &str) -> Result<Text> {
        let (i, d) = self.take(tag, true, expected)?;
        if self.localized {
            Ok(Text::Id(self.u32_of(i, d)?))
        } else {
            Ok(Text::Bytes(self.zstring(i, d)?))
        }
    }
    fn condition(&mut self) -> Result<Condition> {
        let (i, d) = self.take(b"CTDA", false, "CTDA")?;
        let d = self.sized(i, d, 32)?;
        let mut data = [0u8; 32];
        data.copy_from_slice(d);
        let cis1 = if self.at(b"CIS1") {
            Some(self.text_bytes(b"CIS1", "CIS1")?)
        } else {
            None
        };
        let cis2 = if self.at(b"CIS2") {
            Some(self.text_bytes(b"CIS2", "CIS2")?)
        } else {
            None
        };
        Ok(Condition {
            index: i,
            data,
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
        Ok(v)
    }
    fn stage(&mut self) -> Result<Stage> {
        let (i, d) = self.take(b"INDX", true, "INDX")?;
        let d = self.sized(i, d, 4)?;
        let mut entries = Vec::new();
        while self.at(b"QSDT") {
            let (k, q) = self.take(b"QSDT", true, "QSDT")?;
            let flags = self.sized(k, q, 1)?[0];
            let conditions = self.conditions()?;
            let note = if self.at(b"NAM2") {
                Some(self.text_bytes(b"NAM2", "NAM2")?)
            } else {
                None
            };
            let text = if self.at(b"CNAM") {
                Some(self.text(b"CNAM", "CNAM")?)
            } else {
                None
            };
            let e = LogEntry {
                index: k,
                flags,
                conditions,
                note,
                text,
            };
            self.push(&mut entries, e)?;
        }
        Ok(Stage {
            index: i,
            stage: u16::from_le_bytes([d[0], d[1]]),
            flags: d[2],
            byte3: d[3],
            entries,
        })
    }
    fn objective(&mut self) -> Result<Objective> {
        let (i, d) = self.take(b"QOBJ", true, "QOBJ")?;
        let d = self.sized(i, d, 2)?;
        let (_, flags) = self.word(b"FNAM", "FNAM after QOBJ")?;
        let text = if self.at(b"NNAM") {
            Some(self.text(b"NNAM", "NNAM")?)
        } else {
            None
        };
        let mut targets = Vec::new();
        while self.at(b"QSTA") {
            let (k, q) = self.take(b"QSTA", true, "QSTA")?;
            let q = self.sized(k, q, 12)?;
            let w = |o: usize| u32::from_le_bytes([q[o], q[o + 1], q[o + 2], q[o + 3]]);
            let keyword = self.reference(k, w(8))?;
            let conditions = self.conditions()?;
            let t = Target {
                index: k,
                alias: w(0) as i32,
                flags: w(4),
                keyword,
                conditions,
            };
            self.push(&mut targets, t)?;
        }
        Ok(Objective {
            index: i,
            objective: u16::from_le_bytes([d[0], d[1]]),
            flags,
            text,
            targets,
        })
    }
    fn alias(&mut self) -> Result<Alias> {
        let first = self.i;
        let kind = if self.at(b"ALCS") {
            let (_, collection) = self.word(b"ALCS", "ALCS")?;
            let (k, d) = self.take(b"ALMI", true, "ALMI after ALCS")?;
            let max_initial = self.sized(k, d, 1)?[0];
            if !self.at(b"ALST") {
                return Err(self.order("ALST after ALCS and ALMI"));
            }
            AliasKind::Collection {
                collection,
                max_initial,
            }
        } else if self.at(b"ALST") {
            AliasKind::Reference
        } else {
            AliasKind::Location
        };
        let id_tag = if kind == AliasKind::Location {
            b"ALLS"
        } else {
            b"ALST"
        };
        let (_, id) = self.word(id_tag, "ALST, ALLS or ALCS")?;
        let name = self.text_bytes(b"ALID", "ALID after the alias index")?;
        let (_, flags) = self.word(b"FNAM", "FNAM after ALID")?;
        let mut values = Vec::new();
        loop {
            let Some(tag) = self.peek() else {
                return Err(self.order(ALIAS_STARTS));
            };
            let v = match &tag {
                b"ALED" => {
                    let (k, d) = self.take(b"ALED", true, "ALED")?;
                    self.sized(k, d, 0)?;
                    break;
                }
                b"ALFR" => AliasValue::ForcedReference(self.form(b"ALFR", "ALFR")?),
                b"ALUA" => AliasValue::UniqueActor(self.form(b"ALUA", "ALUA")?),
                b"ALFL" => AliasValue::ForcedLocation(self.form(b"ALFL", "ALFL")?),
                b"ALFA" => {
                    let (_, alias) = self.word(b"ALFA", "ALFA")?;
                    let ref_type = if self.at(b"ALRT") {
                        Some(self.form(b"ALRT", "ALRT")?)
                    } else {
                        None
                    };
                    AliasValue::LocationAliasReference {
                        alias: alias as i32,
                        ref_type,
                    }
                }
                b"ALFE" => {
                    let event = self.code(b"ALFE", "ALFE")?;
                    let data = self.code(b"ALFD", "ALFD after ALFE")?;
                    AliasValue::FromEvent { event, data }
                }
                b"ALCO" => {
                    let object = self.form(b"ALCO", "ALCO")?;
                    let (_, a) = self.word(b"ALCA", "ALCA after ALCO")?;
                    let (_, level) = self.word(b"ALCL", "ALCL after ALCA")?;
                    AliasValue::CreateObject {
                        object,
                        alias: a as u16,
                        word: (a >> 16) as u16,
                        level,
                    }
                }
                b"ALEQ" => {
                    let quest = self.form(b"ALEQ", "ALEQ")?;
                    let (_, alias) = self.word(b"ALEA", "ALEA after ALEQ")?;
                    AliasValue::External {
                        quest,
                        alias: alias as i32,
                    }
                }
                b"ALFI" => AliasValue::Alfi(self.word(b"ALFI", "ALFI")?.1 as i32),
                b"ALNA" => {
                    let (_, alias) = self.word(b"ALNA", "ALNA")?;
                    let (_, word) = self.word(b"ALNT", "ALNT after ALNA")?;
                    AliasValue::LinkedAlias {
                        alias: alias as i32,
                        word,
                    }
                }
                b"ALCC" => AliasValue::Alcc(self.word(b"ALCC", "ALCC")?.1),
                t if ALIAS_FORMS.contains(&t) => {
                    let t = *t;
                    AliasValue::Form(t, self.form(&t, "a FormID")?)
                }
                b"KSIZ" => {
                    let (_, n) = self.word(b"KSIZ", "KSIZ")?;
                    let (k, d) = self.take(b"KWDA", true, "KWDA after KSIZ")?;
                    if d.len() as u64 != u64::from(n) * 4 {
                        return Err(Error::new(
                            "QuestCount",
                            self.file(),
                            self.offset_at(k),
                            format!("KWDA of {n} FormIDs (KSIZ)"),
                            d.len(),
                        ));
                    }
                    let mut v = Vec::new();
                    self.b
                        .reserve(&mut v, n as usize, self.file(), self.offset_at(k))?;
                    for c in d.as_chunks::<4>().0 {
                        v.push(self.reference(k, u32::from_le_bytes([c[0], c[1], c[2], c[3]]))?);
                    }
                    AliasValue::Keywords(v)
                }
                b"COCT" => {
                    let (k, n) = self.word(b"COCT", "COCT")?;
                    let mut v = Vec::new();
                    for _ in 0..n {
                        let (c, d) = self.take(b"CNTO", true, "CNTO, as many as COCT counts")?;
                        let d = self.sized(c, d, 8)?;
                        let item =
                            self.reference(c, u32::from_le_bytes([d[0], d[1], d[2], d[3]]))?;
                        let count = i32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                        self.push(&mut v, (item, count))?;
                    }
                    if self.at(b"CNTO") {
                        return Err(Error::new(
                            "QuestCount",
                            self.file(),
                            self.offset_at(k),
                            format!("{n} CNTO (COCT)"),
                            "one more CNTO",
                        ));
                    }
                    AliasValue::Items(v)
                }
                b"ALLA" => {
                    let (k, d) = self.take(b"ALLA", true, "ALLA")?;
                    if d.len() % 8 != 0 {
                        return Err(Error::new(
                            "QuestFieldSize",
                            self.file(),
                            self.offset_at(k),
                            "QUST.ALLA of a multiple of 8 bytes",
                            d.len(),
                        ));
                    }
                    let mut v = Vec::new();
                    self.b
                        .reserve(&mut v, d.len() / 8, self.file(), self.offset_at(k))?;
                    for c in d.as_chunks::<8>().0 {
                        let f = self.reference(k, u32::from_le_bytes([c[0], c[1], c[2], c[3]]))?;
                        v.push((f, i32::from_le_bytes([c[4], c[5], c[6], c[7]])));
                    }
                    AliasValue::KeywordAliases(v)
                }
                b"CTDA" => AliasValue::Condition(self.condition()?),
                _ => return Err(self.order(ALIAS_STARTS)),
            };
            self.push(&mut values, v)?;
        }
        Ok(Alias {
            index: first,
            kind,
            id,
            name,
            flags,
            values,
        })
    }
}

impl Quest {
    /// Reads a QUST record. `ctx` names the plugin its FormIDs resolve
    /// through; texts are string table ids when that plugin is localized.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        if r.signature != *b"QUST" {
            return Err(Error::new(
                "RecordTypeMismatch",
                &r.source.file,
                r.source.offset,
                "QUST",
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let localized = ctx
            .order
            .headers()
            .get(ctx.plugin)
            .is_some_and(|h| h.flags & LOCALIZED != 0);
        let mut q = Q {
            r,
            ctx,
            b,
            localized,
            i: 0,
            fields: Vec::new(),
        };
        b.reserve(
            &mut q.fields,
            r.subrecords.len(),
            &r.source.file,
            r.source.offset,
        )?;
        let opt_text = |q: &mut Q<'_>, tag: &[u8; 4]| -> Result<Option<Vec<u8>>> {
            if q.at(tag) {
                Ok(Some(q.text_bytes(tag, "a text")?))
            } else {
                Ok(None)
            }
        };
        let editor_id = opt_text(&mut q, b"EDID")?;
        let vmad = if q.at(b"VMAD") {
            Some(q.take(b"VMAD", false, "VMAD")?.0)
        } else {
            None
        };
        let name = if q.at(b"FULL") {
            Some(q.text(b"FULL", "FULL")?)
        } else {
            None
        };
        let general = if q.at(b"DNAM") {
            let (i, d) = q.take(b"DNAM", true, "DNAM")?;
            let d = q.sized(i, d, 12)?;
            let w = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
            Some(General {
                flags: u16::from_le_bytes([d[0], d[1]]),
                priority: d[2],
                byte3: d[3],
                word: w(4),
                kind: w(8),
            })
        } else {
            None
        };
        let event = if q.at(b"ENAM") {
            Some(q.code(b"ENAM", "ENAM")?)
        } else {
            None
        };
        let location = if q.at(b"LNAM") {
            Some(q.form(b"LNAM", "LNAM")?)
        } else {
            None
        };
        let xnam = if q.at(b"XNAM") {
            Some(q.form(b"XNAM", "XNAM")?)
        } else {
            None
        };
        let mut text_globals = Vec::new();
        while q.at(b"QTGL") {
            let f = q.form(b"QTGL", "QTGL")?;
            q.push(&mut text_globals, f)?;
        }
        let filter = opt_text(&mut q, b"FLTR")?;
        let conditions = q.conditions()?;
        let (next, event_conditions) = if q.at(b"NEXT") {
            let (i, d) = q.take(b"NEXT", true, "NEXT")?;
            q.sized(i, d, 0)?;
            (Some(i), q.conditions()?)
        } else {
            (None, Vec::new())
        };
        let mut stages = Vec::new();
        while q.at(b"INDX") {
            let s = q.stage()?;
            q.push(&mut stages, s)?;
        }
        let mut objectives = Vec::new();
        while q.at(b"QOBJ") {
            let o = q.objective()?;
            q.push(&mut objectives, o)?;
        }
        let next_alias = if q.at(b"ANAM") {
            Some(q.word(b"ANAM", "ANAM")?.1)
        } else {
            None
        };
        let mut aliases = Vec::new();
        while q.at(b"ALST") || q.at(b"ALLS") || q.at(b"ALCS") {
            let a = q.alias()?;
            q.push(&mut aliases, a)?;
        }
        let gnam = if q.at(b"GNAM") {
            Some(q.form(b"GNAM", "GNAM")?)
        } else {
            None
        };
        let snam = opt_text(&mut q, b"SNAM")?;
        if q.peek().is_some() {
            return Err(q.order(
                "the subrecord order of a QUST (EDID VMAD FULL DNAM ENAM LNAM XNAM QTGL* FLTR CTDA* NEXT CTDA* INDX... QOBJ... ANAM aliases GNAM SNAM)",
            ));
        }
        Ok(Quest {
            editor_id,
            vmad,
            name,
            general,
            event,
            location,
            xnam,
            text_globals,
            filter,
            conditions,
            next,
            event_conditions,
            stages,
            objectives,
            next_alias,
            aliases,
            gnam,
            snam,
            fields: q.fields,
        })
    }
}

#[cfg(test)]
mod tests;
