use crate::{exact, zstring, Record};
use fo4_core::{Budget, Cursor, Error, Limits, Result, SliceSource};
use serde::Serialize;

pub const INTERPRETED_TYPES: [[u8; 4]; 26] = [
    *b"CELL", *b"REFR", *b"WRLD", *b"STAT", *b"SCOL", *b"MSTT", *b"ACTI", *b"DOOR", *b"FURN",
    *b"LIGH", *b"LGTM", *b"MSWP", *b"TXST", *b"MISC", *b"CONT", *b"ALCH", *b"BOOK", *b"AMMO",
    *b"KEYM", *b"NOTE", *b"FLOR", *b"TERM", *b"TACT", *b"IDLM", *b"SOUN", *b"ASPC",
];
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", content = "value")]
pub enum Value {
    Text(String),
    Localized(u32),
    FormId(u32),
    U16(u16),
    U8(u8),
    Float(#[serde(serialize_with = "fo4_core::ieee::serialize")] f32),
    Transform(#[serde(serialize_with = "fo4_core::ieee::serialize")] [f32; 6]),
    Bounds([i16; 6]),
    Grid {
        i: i32,
        j: i32,
        flags: u32,
    },
    EnableParent {
        id: u32,
        flags: u8,
        raw: [u8; 3],
    },
    Light {
        time: i32,
        radius: u32,
        color: [u8; 4],
        flags: u32,
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        parameters: Vec<f32>,
        value: Option<u32>,
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        weight: Option<f32>,
    },
    PartPositions(#[serde(serialize_with = "fo4_core::ieee::serialize")] Vec<[f32; 7]>),
    LightingPrefix {
        ambient: [u8; 4],
        directional: [u8; 4],
        fog_color: [u8; 4],
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        fog_near: f32,
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        fog_far: f32,
        rotation: [i32; 2],
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        fade: f32,
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        clip: f32,
        #[serde(serialize_with = "fo4_core::ieee::serialize")]
        power: f32,
        uninterpreted_tail: std::ops::Range<usize>,
    },
    Raw,
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub signature: [u8; 4],
    pub subrecord: usize,
    pub value: Value,
    pub non_finite_values: u64,
}
impl Value {
    pub fn non_finite_values(&self) -> u64 {
        use fo4_core::ieee::count;
        match self {
            Self::Float(v) => count(v),
            Self::Transform(v) => count(v),
            Self::PartPositions(v) => count(v),
            Self::Light {
                parameters, weight, ..
            } => count(parameters) + count(weight),
            Self::LightingPrefix {
                fog_near,
                fog_far,
                fade,
                clip,
                power,
                ..
            } => count(&[*fog_near, *fog_far, *fade, *clip, *power]),
            _ => 0,
        }
    }
}
pub fn interpret(r: &Record, l: &Limits, b: &Budget) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    b.reserve(
        &mut fields,
        r.subrecords.len(),
        &r.source.file,
        r.source.offset,
    )?;
    for (i, s) in r.subrecords.iter().enumerate() {
        let data = r.data(s);
        let file = &r.source.file;
        let o = s.offset;
        let sig = &s.signature;
        let ty = &r.signature;
        let model = matches!(
            ty,
            b"STAT"
                | b"SCOL"
                | b"MSTT"
                | b"ACTI"
                | b"DOOR"
                | b"FURN"
                | b"LIGH"
                | b"WRLD"
                | b"MISC"
                | b"CONT"
                | b"ALCH"
                | b"BOOK"
                | b"AMMO"
                | b"KEYM"
                | b"NOTE"
                | b"FLOR"
                | b"TERM"
                | b"TACT"
        );
        let src = SliceSource {
            name: file,
            bytes: data,
        };
        let mut c = Cursor::new(&src);
        // Context matters: an EDID inside an unhandled record is still an envelope.
        let value = if !INTERPRETED_TYPES.contains(ty) {
            Value::Raw
        } else if sig == b"EDID"
            || model && sig == b"MODL"
            || (*ty == *b"TXST"
                && (matches!(
                    sig,
                    b"TX00"
                        | b"TX01"
                        | b"TX02"
                        | b"TX03"
                        | b"TX04"
                        | b"TX05"
                        | b"TX06"
                        | b"TX07"
                        | b"MNAM"
                )))
            || (*ty == *b"MSWP" && matches!(sig, b"BNAM" | b"SNAM" | b"FNAM"))
        {
            let text = zstring(data, l, file, o)?;
            b.charge(text.len() as u64, file, o)?;
            Value::Text(text.into())
        } else if sig == b"FULL" {
            // The plugin localized flag is supplied separately by localize_full(). Keep
            // raw here so four-byte inline strings are never guessed to be string IDs.
            Value::Raw
        } else if model && sig == b"MODS"
            || (*ty == *b"REFR" && matches!(sig, b"NAME" | b"XESP" | b"XMSP"))
            || (*ty == *b"CELL" && matches!(sig, b"LTMP" | b"XCWT" | b"XLCN" | b"XCIM" | b"XGDR"))
            || (*ty == *b"WRLD" && matches!(sig, b"WNAM" | b"CNAM" | b"NAM2" | b"LTMP"))
            || (*ty == *b"LGTM" && sig == b"WGDR")
            || (*ty == *b"SCOL" && sig == b"ONAM")
        {
            if sig == b"XESP" {
                exact(data, 8, file, o)?;
                Value::EnableParent {
                    id: c.u32()?,
                    flags: c.u8()?,
                    raw: c.take()?,
                }
            } else {
                exact(data, 4, file, o)?;
                Value::FormId(c.u32()?)
            }
        } else if *ty == *b"REFR" && sig == b"DATA" {
            exact(data, 24, file, o)?;
            let mut values = [0.; 6];
            for v in &mut values {
                *v = field_float(&mut c, o)?;
            }
            Value::Transform(values)
        } else if *ty == *b"REFR" && sig == b"XSCL" || *ty == *b"MSWP" && sig == b"CNAM" {
            exact(data, 4, file, o)?;
            Value::Float(field_float(&mut c, o)?)
        } else if (model && ty != b"WRLD" || ty == b"TXST") && sig == b"OBND" {
            exact(data, 12, file, o)?;
            let mut bounds = [0; 6];
            for v in &mut bounds {
                *v = c.u16()? as i16;
            }
            Value::Bounds(bounds)
        } else if *ty == *b"CELL" && sig == b"XCLC" {
            exact(data, 12, file, o)?;
            Value::Grid {
                i: c.u32()? as i32,
                j: c.u32()? as i32,
                flags: c.u32()?,
            }
        } else if *ty == *b"CELL" && sig == b"DATA" || *ty == *b"TXST" && sig == b"DNAM" {
            exact(data, 2, file, o)?;
            Value::U16(c.u16()?)
        } else if *ty == *b"WRLD" && sig == b"DATA" {
            exact(data, 1, file, o)?;
            Value::U8(c.u8()?)
        } else if *ty == *b"LIGH" && sig == b"DATA" {
            // FO4 fixed prefix plus optional suffix. Unknown sizes are never padded.
            if !matches!(data.len(), 48 | 60 | 64) {
                fields.push(Field {
                    signature: s.signature,
                    subrecord: i,
                    value: Value::Raw,
                    non_finite_values: 0,
                });
                continue;
            }
            let time = c.u32()? as i32;
            let radius = c.u32()?;
            let color = c.take()?;
            let flags = c.u32()?;
            let mut parameters = Vec::new();
            b.reserve(&mut parameters, (data.len() - 16) / 4, file, o)?;
            while c.pos < c.end.min(56) {
                parameters.push(field_float(&mut c, o)?);
            }
            let value = if c.pos < c.end { Some(c.u32()?) } else { None };
            let weight = if c.pos < c.end {
                Some(field_float(&mut c, o)?)
            } else {
                None
            };
            Value::Light {
                time,
                radius,
                color,
                flags,
                parameters,
                value,
                weight,
            }
        } else if (*ty == *b"LGTM" && sig == b"DATA") || (*ty == *b"CELL" && sig == b"XCLL") {
            // Only the version-independent first 40 bytes are interpreted. The
            // remainder stays explicitly raw, with no photometric conversion.
            if data.len() < 40 {
                return Err(Error::new(
                    "InvalidFieldSize",
                    file,
                    o,
                    "lighting prefix >=40 bytes",
                    data.len(),
                ));
            }
            Value::LightingPrefix {
                ambient: c.take()?,
                directional: c.take()?,
                fog_color: c.take()?,
                fog_near: field_float(&mut c, o)?,
                fog_far: field_float(&mut c, o)?,
                rotation: [c.u32()? as i32, c.u32()? as i32],
                fade: field_float(&mut c, o)?,
                clip: field_float(&mut c, o)?,
                power: field_float(&mut c, o)?,
                uninterpreted_tail: s.data.start + 40..s.data.end,
            }
        } else if *ty == *b"SCOL" && sig == b"DATA" {
            if !data.len().is_multiple_of(28) {
                return Err(Error::new(
                    "InvalidFieldSize",
                    file,
                    o,
                    "SCOL placement array multiple of 28",
                    data.len(),
                ));
            }
            let mut positions = Vec::new();
            b.reserve(&mut positions, data.len() / 28, file, o)?;
            while c.pos < c.end {
                let mut p = [0.; 7];
                for v in &mut p {
                    *v = field_float(&mut c, o)?;
                }
                positions.push(p);
            }
            Value::PartPositions(positions)
        } else {
            Value::Raw
        };
        fields.push(Field {
            signature: s.signature,
            subrecord: i,
            non_finite_values: value.non_finite_values(),
            value,
        });
    }
    Ok(fields)
}
impl Record {
    /// FULL is localized only under the TES4 localized flag, never by payload length.
    pub fn full_name(&self, localized: bool, l: &Limits, b: &Budget) -> Result<Option<Value>> {
        let mut names = self.subrecords.iter().filter(|s| s.signature == *b"FULL");
        let found = names.next();
        if let Some(second) = names.next() {
            return Err(Error::new(
                "DuplicateSubrecord",
                &self.source.file,
                second.offset,
                "one FULL per record",
                "multiple FULL",
            ));
        }
        found
            .map(|s| {
                let data = self.data(s);
                if localized {
                    exact(data, 4, &self.source.file, s.offset)?;
                    let src = SliceSource {
                        name: &self.source.file,
                        bytes: data,
                    };
                    Ok(Value::Localized(Cursor::new(&src).u32()?))
                } else {
                    let text = zstring(data, l, &self.source.file, s.offset)?;
                    b.charge(text.len() as u64, &self.source.file, s.offset)?;
                    Ok(Value::Text(text.into()))
                }
            })
            .transpose()
    }
}

fn field_float(c: &mut Cursor<'_>, base: u64) -> Result<f32> {
    c.f32().map_err(|mut e| {
        e.source.offset += base + 6;
        e
    })
}
