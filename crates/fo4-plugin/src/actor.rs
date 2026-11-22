//! Records of the placed actor chain: ACHR, NPC_, RACE, HDPT, OTFT, ARMO,
//! ARMA and the leveled lists LVLN and LVLI. Every layout here was established by a
//! census of the winning records of one installation (`spec/actor.md`):
//! sizes, offsets, value distributions and the share of FormIDs that resolve
//! to a record of the expected type. Subrecords not listed stay raw: their
//! indices are kept in `unread`, their bytes in the record.
use crate::{typed::FormRef, LoadOrder, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// The plugin a record was read from, for resolving its FormIDs.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub order: &'a LoadOrder,
    pub plugin: usize,
}

struct Reader<'a> {
    r: &'a Record,
    ctx: Context<'a>,
    b: &'a Budget,
    read: Vec<bool>,
}
impl<'a> Reader<'a> {
    fn new(r: &'a Record, ctx: Context<'a>, signature: &[u8; 4], b: &'a Budget) -> Result<Self> {
        if r.signature != *signature {
            return Err(Error::new(
                "RecordTypeMismatch",
                &r.source.file,
                r.source.offset,
                String::from_utf8_lossy(signature),
                String::from_utf8_lossy(&r.signature),
            ));
        }
        let mut read = Vec::new();
        b.reserve(
            &mut read,
            r.subrecords.len(),
            &r.source.file,
            r.source.offset,
        )?;
        read.resize(r.subrecords.len(), false);
        Ok(Self { r, ctx, b, read })
    }
    fn offset(&self, i: usize) -> u64 {
        self.r.subrecords[i].offset
    }
    fn data(&self, i: usize) -> &'a [u8] {
        self.r.data(&self.r.subrecords[i])
    }
    fn sized(&self, i: usize, size: usize) -> Result<&'a [u8]> {
        let d = self.data(i);
        if d.len() != size {
            return Err(Error::new(
                "ActorFieldSize",
                &self.r.source.file,
                self.offset(i),
                format!(
                    "{}.{} of {size} bytes",
                    String::from_utf8_lossy(&self.r.signature),
                    String::from_utf8_lossy(&self.r.subrecords[i].signature)
                ),
                d.len(),
            ));
        }
        Ok(d)
    }
    /// The only subrecord `tag`, marked read.
    fn single(&mut self, tag: &[u8; 4]) -> Result<Option<usize>> {
        let mut found = None;
        for (i, s) in self.r.subrecords.iter().enumerate() {
            if s.signature == *tag {
                if found.is_some() {
                    return Err(Error::new(
                        "DuplicateActorField",
                        &self.r.source.file,
                        s.offset,
                        "at most one",
                        String::from_utf8_lossy(tag),
                    ));
                }
                found = Some(i);
            }
        }
        if let Some(i) = found {
            self.read[i] = true;
        }
        Ok(found)
    }
    fn reference(&self, i: usize, id: u32) -> Result<FormRef> {
        Ok(FormRef {
            raw: id,
            key: self
                .ctx
                .order
                .remap(self.ctx.plugin, id, &self.r.source.file, self.offset(i))?,
            subrecord: i,
        })
    }
    fn form_at(&self, i: usize) -> Result<FormRef> {
        let d = self.sized(i, 4)?;
        self.reference(i, u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
    }
    fn form(&mut self, tag: &[u8; 4]) -> Result<Option<FormRef>> {
        match self.single(tag)? {
            Some(i) => Ok(Some(self.form_at(i)?)),
            None => Ok(None),
        }
    }
    fn word(&mut self, tag: &[u8; 4]) -> Result<Option<u32>> {
        match self.single(tag)? {
            Some(i) => {
                let d = self.sized(i, 4)?;
                Ok(Some(u32::from_le_bytes([d[0], d[1], d[2], d[3]])))
            }
            None => Ok(None),
        }
    }
    fn path_at(&self, i: usize) -> Result<Vec<u8>> {
        let d = self.data(i);
        match d.split_last() {
            Some((0, text)) if !text.contains(&0) => {
                self.b
                    .charge(text.len() as u64, &self.r.source.file, self.offset(i))?;
                Ok(text.to_vec())
            }
            _ => Err(Error::new(
                "ActorFieldText",
                &self.r.source.file,
                self.offset(i),
                "bytes ending with one NUL",
                d.len(),
            )),
        }
    }
    fn push<T>(&self, v: &mut Vec<T>, value: T, i: usize) -> Result<()> {
        self.b.reserve(v, 1, &self.r.source.file, self.offset(i))?;
        v.push(value);
        Ok(())
    }
    fn unread(&self) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        for (i, done) in self.read.iter().enumerate() {
            if !done {
                self.push(&mut out, i, i)?;
            }
        }
        Ok(out)
    }
}

/// A placed actor (ACHR): its base (NAME, an NPC_ in every winning record
/// measured), position and rotation (DATA, six f32 as REFR DATA) and scale
/// (XSCL, one f32).
#[derive(Debug, Serialize)]
pub struct Achr {
    pub base: Option<FormRef>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub transform: Option<[f32; 6]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub scale: Option<f32>,
    pub unread: Vec<usize>,
}
impl Achr {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"ACHR", b)?;
        let base = d.form(b"NAME")?;
        let transform = match d.single(b"DATA")? {
            Some(i) => {
                let v = d.sized(i, 24)?;
                Some(std::array::from_fn(|k| {
                    f32::from_le_bytes([v[4 * k], v[4 * k + 1], v[4 * k + 2], v[4 * k + 3]])
                }))
            }
            None => None,
        };
        let scale = d.word(b"XSCL")?.map(f32::from_bits);
        Ok(Self {
            base,
            transform,
            scale,
            unread: d.unread()?,
        })
    }
}

/// Number of four-byte template slots in NPC_ TPTA.
pub const TEMPLATE_SLOTS: usize = 13;

/// An actor base (NPC_).
#[derive(Debug, Serialize)]
pub struct Npc {
    /// ACBS (20 bytes): the u32 at 0, whose bit 0 tracks the second VTCK
    /// slot of the race (`spec/actor.md`).
    pub flags: Option<u32>,
    /// ACBS u16 at 14: bit k is set exactly when TPTA slot k is not zero.
    pub template_flags: Option<u16>,
    /// RACE (RNAM).
    pub race: Option<FormRef>,
    /// NPC_ or LVLN (TPLT).
    pub template: Option<FormRef>,
    /// TPTA: 13 FormIDs (NPC_ or LVLN), zero for none.
    pub template_slots: Option<Vec<Option<FormRef>>>,
    /// ARMO (WNAM).
    pub skin: Option<FormRef>,
    /// OTFT (DOFT).
    pub outfit: Option<FormRef>,
    /// HDPT (PNAM, repeated).
    pub head_parts: Vec<FormRef>,
    pub unread: Vec<usize>,
}
impl Npc {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"NPC_", b)?;
        let (flags, template_flags) = match d.single(b"ACBS")? {
            Some(i) => {
                let v = d.sized(i, 20)?;
                (
                    Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
                    Some(u16::from_le_bytes([v[14], v[15]])),
                )
            }
            None => (None, None),
        };
        // ACBS stays partly unread: mark it unread again, its other bytes are open.
        if let Some(i) = r.subrecords.iter().position(|s| s.signature == *b"ACBS") {
            d.read[i] = false;
        }
        let race = d.form(b"RNAM")?;
        let template = d.form(b"TPLT")?;
        let template_slots = match d.single(b"TPTA")? {
            Some(i) => {
                let v = d.sized(i, 4 * TEMPLATE_SLOTS)?;
                let mut slots = Vec::new();
                b.reserve(&mut slots, TEMPLATE_SLOTS, &r.source.file, d.offset(i))?;
                for k in 0..TEMPLATE_SLOTS {
                    let id =
                        u32::from_le_bytes([v[4 * k], v[4 * k + 1], v[4 * k + 2], v[4 * k + 3]]);
                    slots.push(if id == 0 {
                        None
                    } else {
                        Some(d.reference(i, id)?)
                    });
                }
                Some(slots)
            }
            None => None,
        };
        let skin = d.form(b"WNAM")?;
        let outfit = d.form(b"DOFT")?;
        let mut head_parts = Vec::new();
        for i in 0..r.subrecords.len() {
            if r.subrecords[i].signature == *b"PNAM" {
                d.read[i] = true;
                let f = d.form_at(i)?;
                d.push(&mut head_parts, f, i)?;
            }
        }
        Ok(Self {
            flags,
            template_flags,
            race,
            template,
            template_slots,
            skin,
            outfit,
            head_parts,
            unread: d.unread()?,
        })
    }
}

/// A race (RACE): its skin armour (WNAM) and the two skeleton paths, the
/// ANAM after the MNAM marker and the ANAM after the FNAM marker.
#[derive(Debug, Serialize)]
pub struct Race {
    pub skin: Option<FormRef>,
    #[serde(serialize_with = "serialize_paths")]
    pub skeletons: [Option<Vec<u8>>; 2],
    /// HDPT (HEAD, repeated), every one.
    pub head_parts: Vec<FormRef>,
    /// The HEAD of each section: after an NAM0 marker, then MNAM (0) or
    /// FNAM (1), as all 141 HEAD of one installation are.
    pub section_head_parts: [Vec<FormRef>; 2],
    pub unread: Vec<usize>,
}
fn serialize_paths<S: serde::Serializer>(
    v: &[Option<Vec<u8>>; 2],
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(2))?;
    for p in v {
        seq.serialize_element(&p.as_ref().map(|p| String::from_utf8_lossy(p).into_owned()))?;
    }
    seq.end()
}
impl Race {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"RACE", b)?;
        let skin = d.form(b"WNAM")?;
        let mut skeletons: [Option<Vec<u8>>; 2] = [None, None];
        let mut section = None;
        let mut head_group = false;
        let mut head_parts = Vec::new();
        let mut section_head_parts: [Vec<FormRef>; 2] = [Vec::new(), Vec::new()];
        for i in 0..r.subrecords.len() {
            match &r.subrecords[i].signature {
                b"MNAM" => section = Some(0),
                b"FNAM" => section = Some(1),
                b"NAM0" => head_group = true,
                // Later groups reuse the markers for other fields.
                b"NAM1" | b"NAM3" => {
                    section = None;
                    head_group = false;
                }
                b"ANAM" => {
                    let Some(k) = section else {
                        return Err(Error::new(
                            "ActorRaceSkeletonSection",
                            &r.source.file,
                            d.offset(i),
                            "ANAM after MNAM or FNAM",
                            "no marker",
                        ));
                    };
                    if skeletons[k].is_some() {
                        return Err(Error::new(
                            "DuplicateActorField",
                            &r.source.file,
                            d.offset(i),
                            "one ANAM per section",
                            k,
                        ));
                    }
                    skeletons[k] = Some(d.path_at(i)?);
                    d.read[i] = true;
                }
                b"HEAD" => {
                    d.read[i] = true;
                    let f = d.form_at(i)?;
                    if let (true, Some(k)) = (head_group, section) {
                        d.push(&mut section_head_parts[k], f.clone(), i)?;
                    }
                    d.push(&mut head_parts, f, i)?;
                }
                _ => {}
            }
        }
        Ok(Self {
            skin,
            skeletons,
            head_parts,
            section_head_parts,
            unread: d.unread()?,
        })
    }
}

/// A head part (HDPT): its model (MODL), the u32 of PNAM (a kind: 0 for
/// the 154 parts that other parts name in HNAM but 2), the DATA byte (bit 3
/// set in those 154 and 2 more), the further parts it names (HNAM, 0 to 3)
/// and the number of its conditions (CTDA, 32 bytes each; 40 of 420
/// records). TNAM (a TXST), RNAM (a FLST), NAM0 and NAM1 (morph files)
/// stay unread.
#[derive(Debug, Serialize)]
pub struct Hdpt {
    #[serde(serialize_with = "serialize_path")]
    pub model: Option<Vec<u8>>,
    pub kind: Option<u32>,
    pub flags: Option<u8>,
    pub extra_parts: Vec<FormRef>,
    pub conditions: usize,
    pub unread: Vec<usize>,
}
fn serialize_path<S: serde::Serializer>(
    v: &Option<Vec<u8>>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_some(&v.as_ref().map(|p| String::from_utf8_lossy(p).into_owned()))
}
impl Hdpt {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"HDPT", b)?;
        let model = match d.single(b"MODL")? {
            Some(i) => Some(d.path_at(i)?),
            None => None,
        };
        let kind = d.word(b"PNAM")?;
        let flags = match d.single(b"DATA")? {
            Some(i) => Some(d.sized(i, 1)?[0]),
            None => None,
        };
        let mut extra_parts = Vec::new();
        let mut conditions = 0;
        for i in 0..r.subrecords.len() {
            match &r.subrecords[i].signature {
                b"HNAM" => {
                    d.read[i] = true;
                    let f = d.form_at(i)?;
                    d.push(&mut extra_parts, f, i)?;
                }
                b"CTDA" => {
                    d.sized(i, 32)?;
                    d.read[i] = true;
                    conditions += 1;
                }
                _ => {}
            }
        }
        Ok(Self {
            model,
            kind,
            flags,
            extra_parts,
            conditions,
            unread: d.unread()?,
        })
    }
}

/// An outfit (OTFT): INAM, an array of FormIDs (ARMO or LVLI).
#[derive(Debug, Serialize)]
pub struct Otft {
    pub items: Vec<FormRef>,
    pub unread: Vec<usize>,
}
impl Otft {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"OTFT", b)?;
        let mut items = Vec::new();
        if let Some(i) = d.single(b"INAM")? {
            let v = d.data(i);
            if v.len() % 4 != 0 {
                return Err(Error::new(
                    "ActorFieldSize",
                    &r.source.file,
                    d.offset(i),
                    "OTFT.INAM of 4n bytes",
                    v.len(),
                ));
            }
            for c in v.as_chunks::<4>().0 {
                let f = d.reference(i, u32::from_le_bytes(*c))?;
                d.push(&mut items, f, i)?;
            }
        }
        Ok(Self {
            items,
            unread: d.unread()?,
        })
    }
}

/// One armour addon of an ARMO: the INDX u16 before it and MODL (an ARMA).
#[derive(Debug, Serialize)]
pub struct AddonRef {
    pub index: u16,
    pub addon: FormRef,
}
/// An armour (ARMO).
#[derive(Debug, Serialize)]
pub struct Armo {
    pub race: Option<FormRef>,
    /// BOD2 (4 bytes) as one word of bits.
    pub slots: Option<u32>,
    pub addons: Vec<AddonRef>,
    pub unread: Vec<usize>,
}
impl Armo {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"ARMO", b)?;
        let race = d.form(b"RNAM")?;
        let slots = d.word(b"BOD2")?;
        let mut addons = Vec::new();
        let mut index = None;
        for i in 0..r.subrecords.len() {
            match &r.subrecords[i].signature {
                b"INDX" => {
                    let v = d.sized(i, 2)?;
                    if index.is_some() {
                        return Err(Error::new(
                            "ActorArmorAddonPair",
                            &r.source.file,
                            d.offset(i),
                            "INDX then MODL",
                            "INDX twice",
                        ));
                    }
                    index = Some(u16::from_le_bytes([v[0], v[1]]));
                    d.read[i] = true;
                }
                b"MODL" => {
                    let Some(k) = index.take() else {
                        return Err(Error::new(
                            "ActorArmorAddonPair",
                            &r.source.file,
                            d.offset(i),
                            "INDX then MODL",
                            "MODL alone",
                        ));
                    };
                    let addon = d.form_at(i)?;
                    d.read[i] = true;
                    d.push(&mut addons, AddonRef { index: k, addon }, i)?;
                }
                _ => {}
            }
        }
        if index.is_some() {
            return Err(Error::new(
                "ActorArmorAddonPair",
                &r.source.file,
                r.source.offset,
                "INDX then MODL",
                "INDX at the end",
            ));
        }
        Ok(Self {
            race,
            slots,
            addons,
            unread: d.unread()?,
        })
    }
}

/// An armour addon (ARMA).
#[derive(Debug, Serialize)]
pub struct Arma {
    pub race: Option<FormRef>,
    /// MODL (repeated): further races.
    pub additional_races: Vec<FormRef>,
    /// BOD2 (4 bytes) as one word of bits.
    pub slots: Option<u32>,
    /// MOD2, MOD3, MOD4, MOD5 model paths.
    #[serde(serialize_with = "serialize_models")]
    pub models: [Option<Vec<u8>>; 4],
    pub unread: Vec<usize>,
}
fn serialize_models<S: serde::Serializer>(
    v: &[Option<Vec<u8>>; 4],
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(4))?;
    for p in v {
        seq.serialize_element(&p.as_ref().map(|p| String::from_utf8_lossy(p).into_owned()))?;
    }
    seq.end()
}
impl Arma {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let mut d = Reader::new(r, ctx, b"ARMA", b)?;
        let race = d.form(b"RNAM")?;
        let slots = d.word(b"BOD2")?;
        let mut models: [Option<Vec<u8>>; 4] = [None, None, None, None];
        for (k, tag) in [b"MOD2", b"MOD3", b"MOD4", b"MOD5"].iter().enumerate() {
            if let Some(i) = d.single(tag)? {
                models[k] = Some(d.path_at(i)?);
            }
        }
        let mut additional_races = Vec::new();
        for i in 0..r.subrecords.len() {
            if r.subrecords[i].signature == *b"MODL" {
                d.read[i] = true;
                let f = d.form_at(i)?;
                d.push(&mut additional_races, f, i)?;
            }
        }
        Ok(Self {
            race,
            additional_races,
            slots,
            models,
            unread: d.unread()?,
        })
    }
    /// Whether `race` is the addon's RNAM or one of its MODL races.
    pub fn serves(&self, race: &crate::FormKey) -> bool {
        self.race
            .iter()
            .chain(&self.additional_races)
            .any(|f| f.key.as_ref() == Some(race))
    }
}

/// One LVLO entry (12 bytes): u16 at 0 and the FormID at 4; the other
/// bytes stay raw.
#[derive(Debug, Serialize)]
pub struct LeveledEntry {
    pub level: u16,
    pub reference: FormRef,
    pub raw: [u8; 12],
}
/// A leveled list (LVLN or LVLI). LVLD, LVLM and LVLF are one byte each
/// whose meaning is not established; they are kept as read.
#[derive(Debug, Serialize)]
pub struct Leveled {
    pub entries: Vec<LeveledEntry>,
    pub bytes: [Option<u8>; 3],
    pub unread: Vec<usize>,
}
impl Leveled {
    pub fn decode(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Self> {
        let signature = if r.signature == *b"LVLI" {
            b"LVLI"
        } else {
            b"LVLN"
        };
        let mut d = Reader::new(r, ctx, signature, b)?;
        let mut bytes = [None; 3];
        for (k, tag) in [b"LVLD", b"LVLM", b"LVLF"].iter().enumerate() {
            if let Some(i) = d.single(tag)? {
                bytes[k] = Some(d.sized(i, 1)?[0]);
            }
        }
        let declared = match d.single(b"LLCT")? {
            Some(i) => Some(d.sized(i, 1)?[0]),
            None => None,
        };
        let mut entries = Vec::new();
        for i in 0..r.subrecords.len() {
            if r.subrecords[i].signature == *b"LVLO" {
                let v = d.sized(i, 12)?;
                d.read[i] = true;
                let reference = d.reference(i, u32::from_le_bytes([v[4], v[5], v[6], v[7]]))?;
                let raw = std::array::from_fn(|k| v[k]);
                d.push(
                    &mut entries,
                    LeveledEntry {
                        level: u16::from_le_bytes([v[0], v[1]]),
                        reference,
                        raw,
                    },
                    i,
                )?;
            }
        }
        if declared.is_some_and(|n| usize::from(n) != entries.len()) {
            return Err(Error::new(
                "ActorLeveledCount",
                &r.source.file,
                r.source.offset,
                format!("LLCT {:?}", declared),
                entries.len(),
            ));
        }
        Ok(Self {
            entries,
            bytes,
            unread: d.unread()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, Subrecord};
    use fo4_core::{Limits, SliceSource, SourceSpan};

    fn header() -> Header {
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(0u32.to_le_bytes());
        hedr.extend(0x900u32.to_le_bytes());
        let mut data = b"HEDR".to_vec();
        data.extend((hedr.len() as u16).to_le_bytes());
        data.extend(hedr);
        let mut bytes = b"TES4".to_vec();
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend([0; 4]);
        bytes.extend(131u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(data);
        Header::read(
            "test.esm",
            &SliceSource {
                name: "test.esm",
                bytes: &bytes,
            },
            &Limits::default(),
            &Budget::new(1 << 20),
        )
        .unwrap()
    }
    fn order() -> LoadOrder {
        LoadOrder::new(vec![header()], &Budget::new(1 << 20)).unwrap()
    }
    fn rec(signature: &[u8; 4], fields: &[(&[u8; 4], Vec<u8>)]) -> Record {
        let mut payload = Vec::new();
        let mut subrecords = Vec::new();
        for (tag, data) in fields {
            let offset = 24 + payload.len() as u64;
            payload.extend(*tag);
            payload.extend((data.len() as u16).to_le_bytes());
            let start = payload.len();
            payload.extend(data);
            subrecords.push(Subrecord {
                signature: **tag,
                data: start..payload.len(),
                offset,
                extended: false,
            });
        }
        Record {
            signature: *signature,
            flags: 0,
            form_id: 0x801,
            revision: 0,
            version: 131,
            unknown: 0,
            source: SourceSpan::new("synthetic", 0, 24 + payload.len() as u64),
            payload,
            subrecords,
            payload_is_decompressed: false,
        }
    }
    fn id(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn key(f: &Option<FormRef>) -> u32 {
        f.as_ref().unwrap().key.as_ref().unwrap().object_id
    }

    #[test]
    fn achr_reads_base_placement_and_scale_and_keeps_the_rest_unread() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let r = rec(
            b"ACHR",
            &[
                (b"NAME", id(0x810)),
                (b"XLKR", vec![0; 8]),
                (b"DATA", floats(&[1., 2., 3., 0.5, 0., -1.])),
                (b"XSCL", floats(&[0.75])),
            ],
        );
        let a = Achr::decode(&r, ctx, &b).unwrap();
        assert_eq!(key(&a.base), 0x810);
        assert_eq!(a.transform, Some([1., 2., 3., 0.5, 0., -1.]));
        assert_eq!(a.scale, Some(0.75));
        assert_eq!(a.unread, [1]);
        let short = rec(b"ACHR", &[(b"DATA", vec![0; 20])]);
        assert_eq!(
            Achr::decode(&short, ctx, &b).unwrap_err().code,
            "ActorFieldSize"
        );
        let twice = rec(b"ACHR", &[(b"NAME", id(0x810)), (b"NAME", id(0x811))]);
        assert_eq!(
            Achr::decode(&twice, ctx, &b).unwrap_err().code,
            "DuplicateActorField"
        );
        assert_eq!(
            Achr::decode(&rec(b"REFR", &[]), ctx, &b).unwrap_err().code,
            "RecordTypeMismatch"
        );
    }

    #[test]
    fn npc_reads_flags_template_slots_and_repeated_head_parts() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let mut acbs = vec![0u8; 20];
        acbs[0] = 1;
        acbs[14..16].copy_from_slice(&0b101u16.to_le_bytes());
        let mut tpta = vec![0u8; 52];
        tpta[0..4].copy_from_slice(&id(0x820));
        tpta[8..12].copy_from_slice(&id(0x821));
        let r = rec(
            b"NPC_",
            &[
                (b"ACBS", acbs),
                (b"TPLT", id(0x820)),
                (b"TPTA", tpta),
                (b"RNAM", id(0x830)),
                (b"WNAM", id(0x840)),
                (b"PNAM", id(0x850)),
                (b"PNAM", id(0x851)),
                (b"DOFT", id(0x860)),
            ],
        );
        let n = Npc::decode(&r, ctx, &b).unwrap();
        assert_eq!(n.flags, Some(1));
        assert_eq!(n.template_flags, Some(0b101));
        let slots = n.template_slots.unwrap();
        assert_eq!(slots.len(), TEMPLATE_SLOTS);
        assert_eq!(slots.iter().filter(|s| s.is_some()).count(), 2);
        assert_eq!(key(&slots[2]), 0x821);
        assert_eq!(
            (key(&n.race), key(&n.skin), key(&n.outfit), key(&n.template)),
            (0x830, 0x840, 0x860, 0x820)
        );
        assert_eq!(n.head_parts.len(), 2);
        // ACBS has bytes whose role is not established: it stays listed.
        assert_eq!(n.unread, [0]);
        let bad = rec(b"NPC_", &[(b"TPTA", vec![0; 48])]);
        assert_eq!(
            Npc::decode(&bad, ctx, &b).unwrap_err().code,
            "ActorFieldSize"
        );
    }

    #[test]
    fn hdpt_reads_model_kind_flags_extras_and_counts_conditions() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let r = rec(
            b"HDPT",
            &[
                (b"MODL", b"a\\head.nif\0".to_vec()),
                (b"DATA", vec![3]),
                (b"PNAM", 1u32.to_le_bytes().to_vec()),
                (b"HNAM", id(0x880)),
                (b"HNAM", id(0x881)),
                (b"CTDA", vec![0; 32]),
                (b"TNAM", id(0x890)),
            ],
        );
        let h = Hdpt::decode(&r, ctx, &b).unwrap();
        assert_eq!(h.model.as_deref(), Some(&b"a\\head.nif"[..]));
        assert_eq!((h.kind, h.flags, h.conditions), (Some(1), Some(3), 1));
        assert_eq!(
            h.extra_parts
                .iter()
                .map(|f| key(&Some(f.clone())))
                .collect::<Vec<_>>(),
            [0x880, 0x881]
        );
        assert_eq!(h.unread, [6]);
        let bad = rec(b"HDPT", &[(b"DATA", vec![1, 2])]);
        assert_eq!(
            Hdpt::decode(&bad, ctx, &b).unwrap_err().code,
            "ActorFieldSize"
        );
        let cond = rec(b"HDPT", &[(b"CTDA", vec![0; 28])]);
        assert_eq!(
            Hdpt::decode(&cond, ctx, &b).unwrap_err().code,
            "ActorFieldSize"
        );
    }

    #[test]
    fn race_skeletons_follow_their_section_markers_only() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let r = rec(
            b"RACE",
            &[
                (b"WNAM", id(0x840)),
                (b"MNAM", vec![]),
                (b"ANAM", b"a\\first.nif\0".to_vec()),
                (b"FNAM", vec![]),
                (b"ANAM", b"a\\second.nif\0".to_vec()),
                (b"NAM1", vec![]),
                (b"MNAM", vec![]),
                (b"MODL", b"other.egt\0".to_vec()),
                (b"HEAD", id(0x870)),
                (b"NAM0", vec![]),
                (b"MNAM", vec![]),
                (b"INDX", vec![0; 4]),
                (b"HEAD", id(0x871)),
                (b"NAM0", vec![]),
                (b"FNAM", vec![]),
                (b"HEAD", id(0x872)),
                (b"HEAD", id(0x873)),
            ],
        );
        let race = Race::decode(&r, ctx, &b).unwrap();
        assert_eq!(race.skeletons[0].as_deref(), Some(&b"a\\first.nif"[..]));
        assert_eq!(race.skeletons[1].as_deref(), Some(&b"a\\second.nif"[..]));
        assert_eq!(key(&race.skin), 0x840);
        assert_eq!(race.head_parts.len(), 4);
        // Only HEAD after NAM0 and a section marker belongs to a section.
        assert_eq!(
            race.section_head_parts
                .each_ref()
                .map(|v| v.iter().map(|f| key(&Some(f.clone()))).collect::<Vec<_>>()),
            [vec![0x871], vec![0x872, 0x873]]
        );
        let loose = rec(b"RACE", &[(b"ANAM", b"x.nif\0".to_vec())]);
        assert_eq!(
            Race::decode(&loose, ctx, &b).unwrap_err().code,
            "ActorRaceSkeletonSection"
        );
        let late = rec(
            b"RACE",
            &[
                (b"MNAM", vec![]),
                (b"NAM1", vec![]),
                (b"ANAM", b"x.nif\0".to_vec()),
            ],
        );
        assert_eq!(
            Race::decode(&late, ctx, &b).unwrap_err().code,
            "ActorRaceSkeletonSection"
        );
        let twice = rec(
            b"RACE",
            &[
                (b"MNAM", vec![]),
                (b"ANAM", b"x.nif\0".to_vec()),
                (b"ANAM", b"y.nif\0".to_vec()),
            ],
        );
        assert_eq!(
            Race::decode(&twice, ctx, &b).unwrap_err().code,
            "DuplicateActorField"
        );
        let unterminated = rec(b"RACE", &[(b"MNAM", vec![]), (b"ANAM", b"x.nif".to_vec())]);
        assert_eq!(
            Race::decode(&unterminated, ctx, &b).unwrap_err().code,
            "ActorFieldText"
        );
    }

    #[test]
    fn outfit_armour_and_addon_lists() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let mut inam = id(0x900);
        inam.extend(id(0x901));
        let otft = Otft::decode(&rec(b"OTFT", &[(b"INAM", inam)]), ctx, &b).unwrap();
        assert_eq!(otft.items.len(), 2);
        assert_eq!(
            Otft::decode(&rec(b"OTFT", &[(b"INAM", vec![0; 6])]), ctx, &b)
                .unwrap_err()
                .code,
            "ActorFieldSize"
        );
        let armo = Armo::decode(
            &rec(
                b"ARMO",
                &[
                    (b"BOD2", id(0b1100)),
                    (b"RNAM", id(0x830)),
                    (b"INDX", 0u16.to_le_bytes().to_vec()),
                    (b"MODL", id(0x910)),
                    (b"INDX", 2u16.to_le_bytes().to_vec()),
                    (b"MODL", id(0x911)),
                ],
            ),
            ctx,
            &b,
        )
        .unwrap();
        assert_eq!(armo.slots, Some(0b1100));
        assert_eq!(
            armo.addons.iter().map(|a| a.index).collect::<Vec<_>>(),
            [0, 2]
        );
        let alone = rec(b"ARMO", &[(b"MODL", id(0x910))]);
        assert_eq!(
            Armo::decode(&alone, ctx, &b).unwrap_err().code,
            "ActorArmorAddonPair"
        );
        let dangling = rec(b"ARMO", &[(b"INDX", vec![0, 0])]);
        assert_eq!(
            Armo::decode(&dangling, ctx, &b).unwrap_err().code,
            "ActorArmorAddonPair"
        );
        let arma = Arma::decode(
            &rec(
                b"ARMA",
                &[
                    (b"BOD2", id(0b100)),
                    (b"RNAM", id(0x830)),
                    (b"MOD2", b"m.nif\0".to_vec()),
                    (b"MOD3", b"f.nif\0".to_vec()),
                    (b"MODL", id(0x831)),
                ],
            ),
            ctx,
            &b,
        )
        .unwrap();
        assert_eq!(arma.models[0].as_deref(), Some(&b"m.nif"[..]));
        assert_eq!(arma.models[1].as_deref(), Some(&b"f.nif"[..]));
        assert!(arma.models[2].is_none());
        let race = |id| crate::FormKey {
            plugin: "test.esm".into(),
            object_id: id,
        };
        assert!(
            arma.serves(&race(0x830)) && arma.serves(&race(0x831)) && !arma.serves(&race(0x832))
        );
    }

    #[test]
    fn leveled_entries_and_their_declared_count() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let b = Budget::new(1 << 20);
        let mut lvlo = 5u16.to_le_bytes().to_vec();
        lvlo.extend([0, 0]);
        lvlo.extend(id(0x820));
        lvlo.extend([1, 0, 0, 0]);
        let l = Leveled::decode(
            &rec(
                b"LVLN",
                &[
                    (b"LVLD", vec![0]),
                    (b"LLCT", vec![1]),
                    (b"LVLO", lvlo.clone()),
                ],
            ),
            ctx,
            &b,
        )
        .unwrap();
        assert_eq!(l.entries.len(), 1);
        assert_eq!(
            (
                l.entries[0].level,
                l.entries[0].reference.key.as_ref().unwrap().object_id
            ),
            (5, 0x820)
        );
        assert_eq!(l.bytes, [Some(0), None, None]);
        let wrong = rec(b"LVLI", &[(b"LLCT", vec![2]), (b"LVLO", lvlo)]);
        assert_eq!(
            Leveled::decode(&wrong, ctx, &b).unwrap_err().code,
            "ActorLeveledCount"
        );
    }
}
