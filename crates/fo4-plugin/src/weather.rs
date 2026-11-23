//! Climates (CLMT) and weathers (WTHR), read as far as the light step's
//! census and its pending sky, ambient and fog rules need
//! (`spec/light.md`, `spec/light-census.txt`).
//!
//! What the bytes are was measured on one installation (`light_census`),
//! not taken from a source: every CLMT WLST is a multiple of 12 bytes whose
//! first and third words resolve as FormIDs (a WTHR, a GLOB or zero) with
//! a small integer between them; every CLMT TNAM is 6 bytes. In a WTHR,
//! DALC repeats 4 or 8 times, 32 bytes each, the size of the sourced
//! `LGTM.DALC` layout (`spec/record-layouts.tsv`); NAM0 holds four-byte
//! colours (fourth byte zero) whose count is the DALC count times 17 or 19,
//! read here as rows of one colour per DALC slot; FNAM holds f32 values;
//! IMSP holds one FormID per slot. Which row, slot, word or value means
//! what in the running game is not read here: the pending rules that use
//! them name their reading (`spec/hypotheses.tsv`).
use crate::{actor::Context, condition, typed::FormRef, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// One WLST entry: three words as stored.
#[derive(Debug, Clone, Serialize)]
pub struct ClimateWeather {
    /// First word, a FormID (a WTHR in every measured entry).
    pub weather: FormRef,
    /// Second word, as stored.
    pub chance: i32,
    /// Third word, a FormID or zero.
    pub global: Option<FormRef>,
}

/// A CLMT record.
#[derive(Debug, Clone, Serialize)]
pub struct Climate {
    /// WLST entries in stored order (every WLST subrecord, in order).
    pub weathers: Vec<ClimateWeather>,
    /// TNAM, the six bytes as stored.
    pub times: Option<[u8; 6]>,
    /// Tags of subrecords not read here, in stored order.
    pub other: Vec<[u8; 4]>,
}

/// One DALC subrecord read with the `LGTM.DALC` layout: six RGB colours
/// (the fourth byte kept), a seventh colour and an f32.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DirectionalAmbient {
    /// `X+`, `X-`, `Y+`, `Y-`, `Z+`, `Z-` as stored.
    pub axes: [[u8; 4]; 6],
    /// `Specular`.
    pub specular: [u8; 4],
    /// `Scale`.
    pub scale: f32,
}
impl DirectionalAmbient {
    pub fn read(d: &[u8], file: &str, offset: u64) -> Result<Self> {
        if d.len() != 32 {
            return Err(Error::new(
                "DirectionalAmbientSize",
                file,
                offset,
                "32 bytes",
                d.len(),
            ));
        }
        let colour = |k: usize| -> [u8; 4] { [d[4 * k], d[4 * k + 1], d[4 * k + 2], d[4 * k + 3]] };
        Ok(Self {
            axes: std::array::from_fn(colour),
            specular: colour(6),
            scale: f32::from_le_bytes([d[28], d[29], d[30], d[31]]),
        })
    }
}

/// A WTHR record, the parts the light step reads.
#[derive(Debug, Clone, Serialize)]
pub struct Weather {
    /// DALC subrecords in stored order (one per slot).
    pub ambient: Vec<DirectionalAmbient>,
    /// NAM0: colours as stored, row-major, `slots()` per row.
    pub colours: Vec<[u8; 4]>,
    /// FNAM as f32 values in stored order.
    pub fog: Vec<f32>,
    /// IMSP: one FormID per slot, as stored.
    pub image_spaces: Vec<FormRef>,
    /// Tags of subrecords not read here, in stored order (one entry per
    /// tag and occurrence).
    pub other: Vec<[u8; 4]>,
}
impl Weather {
    /// Colours per NAM0 row: the number of DALC subrecords.
    pub fn slots(&self) -> usize {
        self.ambient.len()
    }
    /// NAM0 rows.
    pub fn rows(&self) -> usize {
        self.colours.len().checked_div(self.slots()).unwrap_or(0)
    }
    /// NAM0 row `row` at slot `slot`.
    pub fn colour(&self, row: usize, slot: usize) -> Option<[u8; 4]> {
        if slot >= self.slots() || row >= self.rows() {
            return None;
        }
        self.colours.get(row * self.slots() + slot).copied()
    }
}

fn type_check(r: &Record, want: &[u8; 4], code: &'static str) -> Result<()> {
    if r.signature != *want {
        return Err(Error::new(
            code,
            &r.source.file,
            r.source.offset,
            String::from_utf8_lossy(want),
            String::from_utf8_lossy(&r.signature),
        ));
    }
    Ok(())
}

impl Climate {
    /// Reads a CLMT record. A WLST that is not a multiple of 12 bytes is
    /// `ClimateWeatherListSize`, a TNAM that is not 6 bytes
    /// `ClimateTimesSize`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Climate> {
        type_check(r, b"CLMT", "ClimateType")?;
        let file = &r.source.file;
        let mut c = Climate {
            weathers: Vec::new(),
            times: None,
            other: Vec::new(),
        };
        for (i, s) in r.subrecords.iter().enumerate() {
            let d = r.data(s);
            match &s.signature {
                b"WLST" => {
                    if !d.len().is_multiple_of(12) {
                        return Err(Error::new(
                            "ClimateWeatherListSize",
                            file,
                            s.offset,
                            "a multiple of 12 bytes",
                            d.len(),
                        ));
                    }
                    for e in d.as_chunks::<12>().0 {
                        let word =
                            |k: usize| u32::from_le_bytes([e[k], e[k + 1], e[k + 2], e[k + 3]]);
                        let form = |raw: u32| -> Result<FormRef> {
                            Ok(FormRef {
                                raw,
                                key: condition::form(ctx, raw, file, s.offset)?,
                                subrecord: i,
                            })
                        };
                        let global = match word(8) {
                            0 => None,
                            raw => Some(form(raw)?),
                        };
                        b.reserve(&mut c.weathers, 1, file, s.offset)?;
                        c.weathers.push(ClimateWeather {
                            weather: form(word(0))?,
                            chance: word(4) as i32,
                            global,
                        });
                    }
                }
                b"TNAM" => {
                    c.times = Some(d.try_into().map_err(|_| {
                        Error::new("ClimateTimesSize", file, s.offset, "6 bytes", d.len())
                    })?);
                }
                other => {
                    b.reserve(&mut c.other, 1, file, s.offset)?;
                    c.other.push(*other);
                }
            }
        }
        Ok(c)
    }
}

impl Weather {
    /// Reads a WTHR record. A DALC that is not 32 bytes is
    /// `DirectionalAmbientSize`; a NAM0 that is not a whole number of
    /// four-byte colours per DALC slot, or comes without any DALC, is
    /// `WeatherColourLayout`; an FNAM that is not whole f32 values or an
    /// IMSP that is not whole FormIDs is `WeatherFieldSize`.
    pub fn read(r: &Record, ctx: Context<'_>, b: &Budget) -> Result<Weather> {
        type_check(r, b"WTHR", "WeatherType")?;
        let file = &r.source.file;
        let mut w = Weather {
            ambient: Vec::new(),
            colours: Vec::new(),
            fog: Vec::new(),
            image_spaces: Vec::new(),
            other: Vec::new(),
        };
        let mut nam0 = None;
        for (i, s) in r.subrecords.iter().enumerate() {
            let d = r.data(s);
            let words = |what: &str| -> Result<&[[u8; 4]]> {
                if !d.len().is_multiple_of(4) {
                    return Err(Error::new(
                        "WeatherFieldSize",
                        file,
                        s.offset,
                        format!("{what}: whole four-byte words"),
                        d.len(),
                    ));
                }
                Ok(d.as_chunks::<4>().0)
            };
            match &s.signature {
                b"DALC" => {
                    b.reserve(&mut w.ambient, 1, file, s.offset)?;
                    w.ambient.push(DirectionalAmbient::read(d, file, s.offset)?);
                }
                b"NAM0" => nam0 = Some((d, s.offset)),
                b"FNAM" => {
                    let v = words("FNAM")?;
                    b.reserve(&mut w.fog, v.len(), file, s.offset)?;
                    w.fog.extend(v.iter().map(|x| f32::from_le_bytes(*x)));
                }
                b"IMSP" => {
                    let v = words("IMSP")?;
                    b.reserve(&mut w.image_spaces, v.len(), file, s.offset)?;
                    for x in v {
                        let raw = u32::from_le_bytes(*x);
                        w.image_spaces.push(FormRef {
                            raw,
                            key: condition::form(ctx, raw, file, s.offset)?,
                            subrecord: i,
                        });
                    }
                }
                other => {
                    b.reserve(&mut w.other, 1, file, s.offset)?;
                    w.other.push(*other);
                }
            }
        }
        if let Some((d, offset)) = nam0 {
            let row = w.ambient.len().checked_mul(4).filter(|n| *n > 0);
            if row.is_none_or(|n| !d.len().is_multiple_of(n)) {
                return Err(Error::new(
                    "WeatherColourLayout",
                    file,
                    offset,
                    format!("whole rows of {} four-byte colours", w.ambient.len()),
                    d.len(),
                ));
            }
            b.reserve(&mut w.colours, d.len() / 4, file, offset)?;
            w.colours.extend(d.as_chunks::<4>().0.iter().copied());
        }
        Ok(w)
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

    fn dalc(level: u8) -> Vec<u8> {
        let mut d = Vec::new();
        for k in 0..7u8 {
            d.extend([level, k, 0, 0]);
        }
        d.extend(1.5f32.to_le_bytes());
        d
    }

    #[test]
    fn reads_a_climate_weather_list_and_times() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        let mut wlst = Vec::new();
        for (w, c, g) in [(0x801u32, 60i32, 0u32), (0x802, 40, 0x803)] {
            wlst.extend(w.to_le_bytes());
            wlst.extend(c.to_le_bytes());
            wlst.extend(g.to_le_bytes());
        }
        let r = record(
            b"CLMT",
            &[
                (b"EDID", b"C\0".to_vec()),
                (b"WLST", wlst),
                (b"TNAM", vec![30, 54, 102, 126, 0, 68]),
            ],
        );
        let c = Climate::read(&r, ctx, &Budget::new(1 << 20)).unwrap();
        assert_eq!(c.weathers.len(), 2);
        assert_eq!(c.weathers[0].weather.key.as_ref().unwrap().object_id, 0x801);
        assert_eq!(c.weathers[0].chance, 60);
        assert!(c.weathers[0].global.is_none());
        assert_eq!(
            c.weathers[1]
                .global
                .as_ref()
                .unwrap()
                .key
                .as_ref()
                .unwrap()
                .object_id,
            0x803
        );
        assert_eq!(c.times, Some([30, 54, 102, 126, 0, 68]));
        assert_eq!(c.other, vec![*b"EDID"]);
        let bad = record(b"CLMT", &[(b"WLST", vec![0; 13])]);
        let e = Climate::read(&bad, ctx, &Budget::new(1 << 20)).unwrap_err();
        assert_eq!(e.code, "ClimateWeatherListSize");
        let bad = record(b"CLMT", &[(b"TNAM", vec![0; 5])]);
        assert_eq!(
            Climate::read(&bad, ctx, &Budget::new(1 << 20))
                .unwrap_err()
                .code,
            "ClimateTimesSize"
        );
    }

    #[test]
    fn reads_weather_rows_by_slot() {
        let o = order();
        let ctx = Context {
            order: &o,
            plugin: 0,
        };
        // Two slots, three rows: colour (row, slot) = [row, slot, 9, 0].
        let mut nam0 = Vec::new();
        for row in 0..3u8 {
            for slot in 0..2u8 {
                nam0.extend([row, slot, 9, 0]);
            }
        }
        let mut fnam = Vec::new();
        for v in [800f32, 200000.] {
            fnam.extend(v.to_le_bytes());
        }
        let r = record(
            b"WTHR",
            &[
                (b"NAM0", nam0.clone()),
                (b"FNAM", fnam),
                (b"DALC", dalc(10)),
                (b"DALC", dalc(20)),
                (
                    b"IMSP",
                    [0x801u32, 0].iter().flat_map(|w| w.to_le_bytes()).collect(),
                ),
            ],
        );
        let w = Weather::read(&r, ctx, &Budget::new(1 << 20)).unwrap();
        assert_eq!((w.slots(), w.rows()), (2, 3));
        assert_eq!(w.colour(2, 1), Some([2, 1, 9, 0]));
        assert_eq!(w.colour(3, 0), None);
        assert_eq!(w.colour(0, 2), None);
        assert_eq!(w.fog, vec![800., 200000.]);
        assert_eq!(w.ambient[1].axes[5], [20, 5, 0, 0]);
        assert_eq!(w.ambient[1].specular, [20, 6, 0, 0]);
        assert_eq!(w.ambient[0].scale, 1.5);
        assert_eq!(w.image_spaces.len(), 2);
        assert!(w.image_spaces[1].key.is_none());
        // NAM0 not a whole number of rows of the DALC count.
        let r = record(
            b"WTHR",
            &[
                (b"NAM0", nam0[..20].to_vec()),
                (b"DALC", dalc(1)),
                (b"DALC", dalc(2)),
            ],
        );
        assert_eq!(
            Weather::read(&r, ctx, &Budget::new(1 << 20))
                .unwrap_err()
                .code,
            "WeatherColourLayout"
        );
        let r = record(b"WTHR", &[(b"NAM0", nam0), (b"DALC", vec![0; 31])]);
        assert_eq!(
            Weather::read(&r, ctx, &Budget::new(1 << 20))
                .unwrap_err()
                .code,
            "DirectionalAmbientSize"
        );
    }
}
