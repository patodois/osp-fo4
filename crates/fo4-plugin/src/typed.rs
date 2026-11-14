//! Owned typed views using the census decoder once. Unconsumed bytes retain payload ranges.
use crate::{Field, FormKey, Group, Header, LoadOrder, Overrides, Record, Value};
use fo4_core::{limit, within, Budget, Error, FileSource, Game, Limits, Manifest, ReadAt, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

#[derive(Debug, Serialize)]
pub struct RawSubrecord {
    pub signature: [u8; 4],
    pub subrecord: usize,
    /// Range in Record.payload (decompressed when indicated by Record).
    pub bytes: Range<usize>,
    pub open_field: Option<&'static str>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FormRef {
    pub raw: u32,
    pub key: Option<FormKey>,
    pub subrecord: usize,
}
#[derive(Debug, Serialize)]
pub struct EnableParent {
    pub parent: FormRef,
    pub flags: u8,
    pub raw: [u8; 3],
}
#[derive(Debug, Serialize, Default)]
pub struct ModelGroup {
    pub path: Option<String>,
    pub material_swap: Option<FormRef>,
    /// MOD* companion indices; decoded ficha values and raw fields remain explicit.
    pub companions: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub enum FullName {
    Inline(String),
    Localized(u32),
}
#[derive(Debug, Serialize)]
pub struct Grid {
    pub i: i32,
    pub j: i32,
    pub flags: u32,
}
#[derive(Debug, Serialize)]
pub struct LightData {
    pub time: i32,
    pub radius: u32,
    pub color: [u8; 4],
    pub flags: u32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub parameters: Vec<f32>,
    pub value: Option<u32>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub weight: Option<f32>,
}
#[derive(Debug, Serialize)]
pub struct Lighting {
    pub ambient: [u8; 4],
    pub directional: [u8; 4],
    pub fog_color: [u8; 4],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub fog_near: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub fog_far: f32,
    pub rotation: [i32; 2],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub fade: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub clip: f32,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub power: f32,
    pub uninterpreted_tail: Range<usize>,
}
#[derive(Debug, Serialize)]
pub struct Common {
    pub ficha_fields: Vec<crate::ficha::Field>,
    pub editor_id: Option<String>,
    pub full_name: Option<FullName>,
    pub model: ModelGroup,
    pub bounds: Option<[i16; 6]>,
    pub uninterpreted: Vec<RawSubrecord>,
    pub non_finite_values: u64,
}
#[derive(Debug, Serialize)]
pub struct Loaded<T> {
    pub key: FormKey,
    pub plugin_index: usize,
    pub record: Record,
    pub data: T,
}
pub trait Typed: Sized {
    const SIGNATURE: [u8; 4];
    fn decode(d: &mut Decoder<'_>) -> Result<Self>;
}
/// Combines Record::fields with the separately sourced ficha decoder.
pub struct Decoder<'a> {
    fields: Vec<Field>,
    consumed: Vec<bool>,
    record: &'a Record,
    order: &'a LoadOrder,
    plugin: usize,
    budget: &'a Budget,
    localized: bool,
    limits: &'a Limits,
}
impl Decoder<'_> {
    fn take(&mut self, tag: &[u8; 4]) -> Result<Option<(usize, Value)>> {
        let mut found = None;
        for f in &mut self.fields {
            if &f.signature == tag && !matches!(f.value, Value::Raw) {
                if found.is_some() {
                    return Err(Error::new(
                        "DuplicateTypedField",
                        &self.record.source.file,
                        self.record.subrecords[f.subrecord].offset,
                        "at most one field",
                        String::from_utf8_lossy(tag),
                    ));
                }
                self.consumed[f.subrecord] = true;
                found = Some((f.subrecord, std::mem::replace(&mut f.value, Value::Raw)));
            }
        }
        Ok(found)
    }
    fn reference(&self, i: usize, id: u32) -> Result<FormRef> {
        Ok(FormRef {
            raw: id,
            key: self.order.remap(
                self.plugin,
                id,
                &self.record.source.file,
                self.record.subrecords[i].offset,
            )?,
            subrecord: i,
        })
    }
    fn form(&mut self, tag: &[u8; 4]) -> Result<Option<FormRef>> {
        match self.take(tag)? {
            Some((i, Value::FormId(id))) => Ok(Some(self.reference(i, id)?)),
            _ => Ok(None),
        }
    }
    fn value(&mut self, tag: &[u8; 4]) -> Result<Option<Value>> {
        Ok(self.take(tag)?.map(|(_, v)| v))
    }
    fn lighting(&mut self, tag: &[u8; 4]) -> Result<Option<Lighting>> {
        Ok(match self.value(tag)? {
            Some(Value::LightingPrefix {
                ambient,
                directional,
                fog_color,
                fog_near,
                fog_far,
                rotation,
                fade,
                clip,
                power,
                uninterpreted_tail,
            }) => Some(Lighting {
                ambient,
                directional,
                fog_color,
                fog_near,
                fog_far,
                rotation,
                fade,
                clip,
                power,
                uninterpreted_tail,
            }),
            _ => None,
        })
    }
    fn common(&mut self) -> Result<Common> {
        let editor_id = match self.value(b"EDID")? {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        };
        let path = match self.value(b"MODL")? {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        };
        let material_swap = self.form(b"MODS")?;
        let bounds = match self.value(b"OBND")? {
            Some(Value::Bounds(v)) => Some(v),
            _ => None,
        };
        let full_name = match self
            .record
            .full_name(self.localized, self.limits, self.budget)?
        {
            Some(Value::Text(s)) => Some(FullName::Inline(s)),
            Some(Value::Localized(i)) => Some(FullName::Localized(i)),
            None => None,
            Some(other) => {
                return Err(Error::new(
                    "UnexpectedDecodedFullName",
                    &self.record.source.file,
                    self.record.source.offset,
                    "inline or localized name",
                    format!("{other:?}"),
                ))
            }
        };
        let mut companions = Vec::new();
        let mut uninterpreted = Vec::new();
        let mut non_finite_values = 0;
        for f in &self.fields {
            non_finite_values += f.non_finite_values;
            let s = &self.record.subrecords[f.subrecord];
            // take() records consumption separately from the Raw enum variant.
            if s.signature.starts_with(b"MOD") && !matches!(&s.signature, b"MODL" | b"MODS") {
                self.budget
                    .reserve(&mut companions, 1, &self.record.source.file, s.offset)?;
                companions.push(f.subrecord);
            }
            if let Value::LightingPrefix {
                uninterpreted_tail, ..
            } = &f.value
            {
                if !uninterpreted_tail.is_empty() {
                    self.budget.reserve(
                        &mut uninterpreted,
                        1,
                        &self.record.source.file,
                        s.offset,
                    )?;
                    uninterpreted.push(RawSubrecord {
                        signature: s.signature,
                        subrecord: f.subrecord,
                        bytes: uninterpreted_tail.clone(),
                        open_field: None,
                    });
                }
            }
        }
        Ok(Common {
            ficha_fields: Vec::new(),
            editor_id,
            full_name,
            model: ModelGroup {
                path,
                material_swap,
                companions,
            },
            bounds,
            uninterpreted,
            non_finite_values,
        })
    }
    fn finish(&self, common: &mut Common) -> Result<()> {
        common.ficha_fields = crate::ficha::decode(
            self.record,
            self.order,
            self.plugin,
            self.budget,
            self.limits,
        )?;
        for field in &common.ficha_fields {
            if let crate::ficha::Value::Float(v) = field.value {
                let sub = &self.record.subrecords[field.subrecord];
                let already_counted = (&self.record.signature == b"LIGH"
                    && &sub.signature == b"DATA")
                    || (matches!(
                        (&self.record.signature, &sub.signature),
                        (b"CELL", b"XCLL") | (b"LGTM", b"DATA")
                    ) && field.bytes.start - sub.data.start < 40);
                if !already_counted {
                    common.non_finite_values += u64::from(!v.is_finite());
                }
            }
        }
        // Supersede the old lighting-prefix tail only for an exact verified schema.
        common
            .uninterpreted
            .retain(|raw| crate::ficha::schema(&self.record.signature, &raw.signature).is_none());
        for f in &self.fields {
            let s = &self.record.subrecords[f.subrecord];
            let schema = crate::ficha::schema(&self.record.signature, &s.signature);
            let verified = schema.is_some_and(|v| v.accepts(self.record.data(s).len()));
            let class = crate::still::classify_size(
                &self.record.signature,
                &s.signature,
                self.record.data(s).len(),
            );
            let closed = schema.is_some_and(|v| v.complete_for(self.record.data(s)));
            let handled = closed
                && matches!(
                    class,
                    crate::still::Class::Used | crate::still::Class::Outside
                );
            let partial = verified && !closed;
            if !handled
                && (!self.consumed[f.subrecord] || partial || schema.is_some() && !verified)
                && !(s.signature == *b"FULL" && common.full_name.is_some())
            {
                self.budget.reserve(
                    &mut common.uninterpreted,
                    1,
                    &self.record.source.file,
                    s.offset,
                )?;
                common.uninterpreted.push(RawSubrecord {
                    signature: s.signature,
                    subrecord: f.subrecord,
                    bytes: s.data.clone(),
                    open_field: if schema.is_some() && !verified {
                        Some("unverified_size")
                    } else if partial {
                        schema.map(|s| s.open_field)
                    } else {
                        None
                    },
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Cell {
    pub common: Common,
    pub flags: Option<u16>,
    pub grid: Option<Grid>,
    pub lighting: Option<Lighting>,
    pub lighting_template: Option<FormRef>,
    pub water: Option<FormRef>,
    pub location: Option<FormRef>,
    pub image_space: Option<FormRef>,
    pub god_rays: Option<FormRef>,
}
#[derive(Debug, Serialize)]
pub struct Wrld {
    pub common: Common,
    pub flags: Option<u8>,
    pub parent: Option<FormRef>,
    pub climate: Option<FormRef>,
    pub water: Option<FormRef>,
    pub lighting_template: Option<FormRef>,
}
#[derive(Debug, Serialize)]
pub struct Refr {
    pub common: Common,
    pub base: Option<FormRef>,
    pub material_swap: Option<FormRef>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub transform: Option<[f32; 6]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub scale: Option<f32>,
    pub enable_parent: Option<EnableParent>,
}
macro_rules! model_type {
    ($name:ident,$tag:literal) => {
        #[derive(Debug, Serialize)]
        pub struct $name {
            pub common: Common,
        }
        impl Typed for $name {
            const SIGNATURE: [u8; 4] = *$tag;
            fn decode(d: &mut Decoder<'_>) -> Result<Self> {
                let mut common = d.common()?;
                d.finish(&mut common)?;
                Ok(Self { common })
            }
        }
    };
}
model_type!(Stat, b"STAT");
model_type!(Mstt, b"MSTT");
model_type!(Acti, b"ACTI");
model_type!(Door, b"DOOR");
model_type!(Furn, b"FURN");
model_type!(Misc, b"MISC");
model_type!(Cont, b"CONT");
model_type!(Alch, b"ALCH");
model_type!(Book, b"BOOK");
model_type!(Ammo, b"AMMO");
model_type!(Keym, b"KEYM");
model_type!(Note, b"NOTE");
model_type!(Flor, b"FLOR");
model_type!(Term, b"TERM");
model_type!(Tact, b"TACT");
model_type!(Idlm, b"IDLM");
model_type!(Soun, b"SOUN");
model_type!(Aspc, b"ASPC");
#[derive(Debug, Serialize)]
pub struct Ligh {
    pub common: Common,
    pub light: Option<LightData>,
}
#[derive(Debug, Serialize)]
pub struct Lgtm {
    pub common: Common,
    pub lighting: Option<Lighting>,
    pub wgdr: Option<FormRef>,
}
#[derive(Debug, Serialize)]
pub struct ScolPart {
    pub base: Option<FormRef>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub placements: Option<Vec<[f32; 7]>>,
    pub placement_subrecord: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Scol {
    pub common: Common,
    pub parts: Vec<ScolPart>,
}
#[derive(Debug, Serialize)]
pub enum SwapValue {
    Path(String),
    Scalar(#[serde(serialize_with = "fo4_core::ieee::serialize")] f32),
}
#[derive(Debug, Serialize)]
pub struct SwapEntry {
    pub signature: [u8; 4],
    pub subrecord: usize,
    pub value: SwapValue,
}
#[derive(Debug, Serialize)]
pub struct Mswp {
    pub common: Common,
    pub entries: Vec<SwapEntry>,
}
#[derive(Debug, Serialize)]
pub struct Txst {
    pub common: Common,
    pub textures: [Option<String>; 8],
    pub material: Option<String>,
    pub flags: Option<u16>,
}

impl Typed for Cell {
    const SIGNATURE: [u8; 4] = *b"CELL";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let flags = match d.value(b"DATA")? {
            Some(Value::U16(v)) => Some(v),
            _ => None,
        };
        let grid = match d.value(b"XCLC")? {
            Some(Value::Grid { i, j, flags }) => Some(Grid { i, j, flags }),
            _ => None,
        };
        let lighting = d.lighting(b"XCLL")?;
        let lighting_template = d.form(b"LTMP")?;
        let water = d.form(b"XCWT")?;
        let location = d.form(b"XLCN")?;
        let image_space = d.form(b"XCIM")?;
        let god_rays = d.form(b"XGDR")?;
        d.finish(&mut common)?;
        Ok(Self {
            common,
            flags,
            grid,
            lighting,
            lighting_template,
            water,
            location,
            image_space,
            god_rays,
        })
    }
}
impl Typed for Wrld {
    const SIGNATURE: [u8; 4] = *b"WRLD";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let flags = match d.value(b"DATA")? {
            Some(Value::U8(v)) => Some(v),
            _ => None,
        };
        let parent = d.form(b"WNAM")?;
        let climate = d.form(b"CNAM")?;
        let water = d.form(b"NAM2")?;
        let lighting_template = d.form(b"LTMP")?;
        d.finish(&mut common)?;
        Ok(Self {
            common,
            flags,
            parent,
            climate,
            water,
            lighting_template,
        })
    }
}
impl Typed for Refr {
    const SIGNATURE: [u8; 4] = *b"REFR";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let base = d.form(b"NAME")?;
        let material_swap = d.form(b"XMSP")?;
        let transform = match d.value(b"DATA")? {
            Some(Value::Transform(v)) => Some(v),
            _ => None,
        };
        let scale = match d.value(b"XSCL")? {
            Some(Value::Float(v)) => Some(v),
            _ => None,
        };
        let enable_parent = match d.take(b"XESP")? {
            Some((i, Value::EnableParent { id, flags, raw })) => Some(EnableParent {
                parent: d.reference(i, id)?,
                flags,
                raw,
            }),
            _ => None,
        };
        d.finish(&mut common)?;
        Ok(Self {
            common,
            base,
            material_swap,
            transform,
            scale,
            enable_parent,
        })
    }
}
impl Typed for Ligh {
    const SIGNATURE: [u8; 4] = *b"LIGH";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let light = match d.value(b"DATA")? {
            Some(Value::Light {
                time,
                radius,
                color,
                flags,
                parameters,
                value,
                weight,
            }) => Some(LightData {
                time,
                radius,
                color,
                flags,
                parameters,
                value,
                weight,
            }),
            _ => None,
        };
        d.finish(&mut common)?;
        Ok(Self { common, light })
    }
}
impl Typed for Lgtm {
    const SIGNATURE: [u8; 4] = *b"LGTM";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let lighting = d.lighting(b"DATA")?;
        let wgdr = d.form(b"WGDR")?;
        d.finish(&mut common)?;
        Ok(Self {
            common,
            lighting,
            wgdr,
        })
    }
}
impl Typed for Scol {
    const SIGNATURE: [u8; 4] = *b"SCOL";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let mut parts: Vec<ScolPart> = Vec::new();
        for i in 0..d.fields.len() {
            match &d.fields[i].value {
                Value::FormId(_) if d.fields[i].signature == *b"ONAM" => {
                    let Value::FormId(id) = std::mem::replace(&mut d.fields[i].value, Value::Raw)
                    else {
                        unreachable!()
                    };
                    d.consumed[i] = true;
                    let base = Some(d.reference(i, id)?);
                    d.budget.reserve(
                        &mut parts,
                        1,
                        &d.record.source.file,
                        d.record.source.offset,
                    )?;
                    parts.push(ScolPart {
                        base,
                        placements: None,
                        placement_subrecord: None,
                    });
                }
                Value::PartPositions(_) => {
                    if parts.last().is_none_or(|p| p.placements.is_some()) {
                        d.budget.reserve(
                            &mut parts,
                            1,
                            &d.record.source.file,
                            d.record.source.offset,
                        )?;
                        parts.push(ScolPart {
                            base: None,
                            placements: None,
                            placement_subrecord: None,
                        });
                    }
                    let placements = match std::mem::replace(&mut d.fields[i].value, Value::Raw) {
                        Value::PartPositions(p) => Some(p),
                        _ => None,
                    };
                    d.consumed[i] = true;
                    if let Some(p) = parts.last_mut() {
                        p.placements = placements;
                        p.placement_subrecord = Some(i);
                    }
                }
                _ => {}
            }
        }
        d.finish(&mut common)?;
        Ok(Self { common, parts })
    }
}
impl Typed for Mswp {
    const SIGNATURE: [u8; 4] = *b"MSWP";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let mut entries = Vec::new();
        for i in 0..d.fields.len() {
            if matches!(
                &d.fields[i].signature,
                b"BNAM" | b"SNAM" | b"FNAM" | b"CNAM"
            ) && !matches!(d.fields[i].value, Value::Raw)
            {
                d.budget.reserve(
                    &mut entries,
                    1,
                    &d.record.source.file,
                    d.record.source.offset,
                )?;
                let f = &mut d.fields[i];
                entries.push(SwapEntry {
                    signature: f.signature,
                    subrecord: i,
                    value: match std::mem::replace(&mut f.value, Value::Raw) {
                        Value::Text(s) => SwapValue::Path(s),
                        Value::Float(v) => SwapValue::Scalar(v),
                        other => {
                            return Err(Error::new(
                                "UnexpectedDecodedSwapField",
                                &d.record.source.file,
                                d.record.subrecords[i].offset,
                                "MSWP path or scalar",
                                format!("{other:?}"),
                            ))
                        }
                    },
                });
                d.consumed[i] = true;
            }
        }
        d.finish(&mut common)?;
        Ok(Self { common, entries })
    }
}
impl Typed for Txst {
    const SIGNATURE: [u8; 4] = *b"TXST";
    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let mut common = d.common()?;
        let mut textures = std::array::from_fn(|_| None);
        for (i, v) in textures.iter_mut().enumerate() {
            *v = match d.value(&[b'T', b'X', b'0', b'0' + i as u8])? {
                Some(Value::Text(v)) => Some(v),
                _ => None,
            };
        }
        let material = match d.value(b"MNAM")? {
            Some(Value::Text(v)) => Some(v),
            _ => None,
        };
        let flags = match d.value(b"DNAM")? {
            Some(Value::U16(v)) => Some(v),
            _ => None,
        };
        d.finish(&mut common)?;
        Ok(Self {
            common,
            textures,
            material,
            flags,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum CellKind {
    Interior,
    Exterior,
}
#[derive(Debug, Serialize)]
pub struct Child {
    pub key: FormKey,
    pub signature: [u8; 4],
    pub group_kind: u32,
    pub source: fo4_core::SourceSpan,
}
#[derive(Debug, Serialize, Default)]
pub struct Children {
    pub active: Vec<Child>,
    pub deleted: Vec<Child>,
    pub disabled: Vec<Child>,
    pub excluded: Vec<Child>,
}
#[derive(Debug)]
struct Membership {
    history: usize,
    cell: u64,
    group_kind: u32,
}
/// Retains only identities, offsets and memberships. Payloads are reread on demand.
pub struct Store {
    order: LoadOrder,
    overrides: Overrides,
    pub diagnostics: Vec<Error>,
    pub record_types: BTreeMap<[u8; 4], u64>,
    /// Normalized MODL paths on winning ACTI/DOOR/STAT records labelled Is Marker.
    pub marker_models: Vec<Vec<u8>>,
    pub limits: Limits,
    sources: Vec<(String, Box<dyn ReadAt>)>,
    memberships: Vec<Membership>,
    cell_kinds: HashMap<u64, (usize, CellKind)>,
    winning_indices: Vec<usize>,
    source_allocation: Option<Budget>,
    _allocation: Budget,
}
impl Store {
    pub fn order(&self) -> &LoadOrder {
        &self.order
    }
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }
    pub fn open(m: &Manifest, b: &Budget) -> Result<Self> {
        Self::open_overlay(m, None, b)
    }
    /// Explicit read-only final plugin, outside Data. It never changes archive mounts.
    pub fn open_overlay(
        m: &Manifest,
        overlay: Option<&std::path::Path>,
        b: &Budget,
    ) -> Result<Self> {
        m.validate("manifest")?;
        let source_allocation = b.child();
        let mut sources: Vec<(String, Box<dyn ReadAt>)> = Vec::new();
        source_allocation.reserve(&mut sources, m.plugins.len(), "manifest", 0)?;
        for p in &m.plugins {
            let path_bytes = fo4_core::add(
                m.root.as_os_str().len() as u64,
                p.path.len() as u64,
                "manifest",
                0,
            )?;
            let retained = fo4_core::add(
                16384,
                fo4_core::add(
                    p.name.len() as u64,
                    fo4_core::mul(path_bytes, 3, "manifest", 0)?,
                    "manifest",
                    0,
                )?,
                "manifest",
                0,
            )?;
            source_allocation.charge(retained, "manifest", 0)?;
            sources.push((
                p.name.clone(),
                Box::new(FileSource::open(&within(&m.root, &p.path)?)?),
            ));
        }
        if let Some(path) = overlay {
            let name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
                Error::new(
                    "OverlayPluginName",
                    &path.to_string_lossy(),
                    0,
                    "UTF-8 plugin basename",
                    "absent",
                )
            })?;
            let retained = fo4_core::add(
                fo4_core::mul(path.as_os_str().len() as u64, 3, "plugin overlay", 0)?,
                fo4_core::add(name.len() as u64, 16384, "plugin overlay", 0)?,
                "plugin overlay",
                0,
            )?;
            source_allocation.charge(retained, "plugin overlay", 0)?;
            source_allocation.reserve(&mut sources, 1, "plugin overlay", 0)?;
            sources.push((name.to_owned(), Box::new(FileSource::open(path)?)));
        }
        let mut store = Self::from_sources(m.game, sources, m.limits.clone(), b)?;
        store.source_allocation = Some(source_allocation);
        Ok(store)
    }
    pub fn from_sources(
        game: Game,
        sources: Vec<(String, Box<dyn ReadAt>)>,
        limits: Limits,
        b: &Budget,
    ) -> Result<Self> {
        limits.validate("plugin store")?;
        limit(sources.len() as u64, 4350, "plugin store", 0)?;
        if game != Game::Fallout4 {
            return Err(Error::new(
                "UnsupportedTypedGame",
                "manifest",
                0,
                "Fallout4",
                format!("{game:?}"),
            ));
        }
        let allocation = b.child();
        let mut headers = Vec::new();
        allocation.reserve(&mut headers, sources.len(), "manifest", 0)?;
        for (name, s) in &sources {
            let work = Budget::new(limits.max_memory);
            headers.push(Header::read_for_game(
                game,
                name,
                s.as_ref(),
                &limits,
                &work,
            )?);
            allocation.charge(work.used(), s.name(), 0)?;
        }
        let order = LoadOrder::new(headers, &allocation)?;
        let mut overrides = Overrides::new();
        let mut memberships = Vec::new();
        let mut cell_kinds = HashMap::new();
        let mut diagnostics = Vec::new();
        let mut record_types = BTreeMap::new();
        let mut marker_observations = Vec::new();
        for (plugin, (_, s)) in sources.iter().enumerate() {
            let mut complete = true;
            let scan_budget = Budget::new(limits.max_memory);
            // The index needs identities, flags and group membership only, so the
            // payloads stay on disk; the few flagged marker bases are reread below.
            let result = crate::scan_envelopes(
                s.as_ref(),
                &order.headers()[plugin],
                &limits,
                &scan_budget,
                |r, groups, record_budget| {
                    if !record_types.contains_key(&r.signature) {
                        allocation.charge(96, s.name(), r.source.offset)?;
                    }
                    *record_types.entry(r.signature).or_insert(0) += 1;
                    if let Err(e) = overrides.insert(&order, plugin, &r, &allocation) {
                        complete = false;
                        retain_diagnostic(&mut diagnostics, e, &allocation)?;
                        return Ok(());
                    }
                    let history = overrides.history.len() - 1;
                    if matches!(&r.signature, b"STAT" | b"ACTI" | b"DOOR")
                        && r.flags & 0x0080_0000 != 0
                    {
                        let full = crate::record_at(
                            s.as_ref(),
                            r.source.offset,
                            s.len(),
                            &limits,
                            record_budget,
                        )?;
                        for sub in full.subrecords.iter().filter(|s| s.signature == *b"MODL") {
                            let raw = full.data(sub);
                            if let Some(path) = raw.strip_suffix(&[0]) {
                                if let Ok(path) = fo4_core::path_key_bytes(path) {
                                    allocation.charge(
                                        path.len() as u64,
                                        &r.source.file,
                                        sub.offset,
                                    )?;
                                    allocation.reserve(
                                        &mut marker_observations,
                                        1,
                                        &r.source.file,
                                        sub.offset,
                                    )?;
                                    marker_observations.push((history, path));
                                }
                            }
                        }
                    }
                    if r.signature == *b"CELL" {
                        let kind = groups.iter().rev().find_map(|g| match g.kind {
                            2 | 3 => Some(CellKind::Interior),
                            1 | 4 | 5 => Some(CellKind::Exterior),
                            _ => None,
                        });
                        let key = overrides.history[history].key;
                        if let Some(kind) = kind {
                            if !cell_kinds.contains_key(&key) {
                                allocation.charge(64, s.name(), r.source.offset)?;
                                cell_kinds.try_reserve(1).map_err(|e| {
                                    Error::new(
                                        "AllocationFailed",
                                        s.name(),
                                        r.source.offset,
                                        "CELL kind index",
                                        e,
                                    )
                                })?;
                            }
                            cell_kinds.insert(key, (history, kind));
                        } else {
                            cell_kinds.remove(&key);
                        }
                    }
                    if let Some(g) = groups.iter().rev().find(|g| matches!(g.kind, 8..=10)) {
                        let cell = compact_label(&order, plugin, g)?.ok_or_else(|| {
                            Error::new(
                                "InvalidCellGroupLabel",
                                &g.source.file,
                                g.source.offset + 8,
                                "non-null CELL FormID",
                                0,
                            )
                        })?;
                        if let Some(parent) = groups.iter().rev().find(|p| p.kind == 6) {
                            if compact_label(&order, plugin, parent)? != Some(cell) {
                                return Err(Error::new(
                                    "CellGroupLabelMismatch",
                                    &g.source.file,
                                    g.source.offset + 8,
                                    "same CELL as enclosing type 6",
                                    u32::from_le_bytes(g.label),
                                ));
                            }
                        }
                        {
                            allocation.reserve(&mut memberships, 1, s.name(), r.source.offset)?;
                            memberships.push(Membership {
                                history,
                                cell,
                                group_kind: g.kind,
                            });
                        }
                    } else if r.signature == *b"REFR" {
                        complete = false;
                        retain_diagnostic(
                            &mut diagnostics,
                            Error::new(
                                "UnassignedCellReference",
                                &r.source.file,
                                r.source.offset,
                                "winning REFR with explicit CELL child group 8/9/10",
                                "REFR outside a declared cell child group",
                            ),
                            &allocation,
                        )?;
                    }
                    Ok(())
                },
            );
            if let Err(e) = result {
                complete = false;
                retain_diagnostic(&mut diagnostics, e, &allocation)?;
            }
            if complete {
                overrides.mark_complete(plugin, &order, &allocation)?;
            }
        }
        memberships.sort_unstable_by_key(|m| m.cell);
        let mut winning_indices = Vec::new();
        for (index, r) in overrides.history.iter().enumerate() {
            if overrides
                .winner_id(r.key)
                .is_some_and(|w| std::ptr::eq(w, r))
            {
                allocation.reserve(&mut winning_indices, 1, "winner index", r.offset)?;
                winning_indices.push(index);
            }
        }
        let mut marker_models = Vec::new();
        for (history, path) in marker_observations {
            let r = &overrides.history[history];
            if !r.deleted
                && overrides
                    .winner_id(r.key)
                    .is_some_and(|w| std::ptr::eq(w, r))
            {
                allocation.reserve(&mut marker_models, 1, "marker model index", r.offset)?;
                marker_models.push(path);
            }
        }
        marker_models.sort();
        marker_models.dedup();
        Ok(Self {
            marker_models,
            order,
            overrides,
            diagnostics,
            record_types,
            limits,
            sources,
            memberships,
            cell_kinds,
            winning_indices,
            source_allocation: None,
            _allocation: allocation,
        })
    }
    pub fn winners(&self) -> impl Iterator<Item = &crate::Override> {
        self.winning_indices
            .iter()
            .map(|i| &self.overrides.history[*i])
    }
    /// Winning placed occurrences and their explicitly indexed cell kind.
    pub fn placement_contexts(&self) -> impl Iterator<Item = (&crate::Override, Option<CellKind>)> {
        self.memberships.iter().filter_map(|m| {
            let record = &self.overrides.history[m.history];
            if !self
                .overrides
                .winner_id(record.key)
                .is_some_and(|w| std::ptr::eq(w, record))
            {
                return None;
            }
            let kind = self.cell_kinds.get(&m.cell).and_then(|(history, kind)| {
                self.overrides
                    .winner_id(m.cell)
                    .filter(|w| std::ptr::eq(*w, &self.overrides.history[*history]))
                    .map(|_| *kind)
            });
            Some((record, kind))
        })
    }
    pub fn cells(&self) -> impl Iterator<Item = FormKey> + '_ {
        self.winners()
            .filter(|r| r.signature == *b"CELL")
            .map(|r| self.overrides.form_key(r.key, &self.order))
    }
    /// Winning placed occurrences with their recorded parent CELL identity.
    pub fn placement_cells(&self) -> impl Iterator<Item = (&crate::Override, FormKey)> + '_ {
        self.memberships.iter().filter_map(|m| {
            let record = &self.overrides.history[m.history];
            if !self
                .overrides
                .winner_id(record.key)
                .is_some_and(|w| std::ptr::eq(w, record))
            {
                return None;
            }
            Some((record, self.overrides.form_key(m.cell, &self.order)))
        })
    }
    pub fn kind(&self, key: &FormKey) -> Option<CellKind> {
        let w = self.overrides.winner(key)?;
        let (history, kind) = self.cell_kinds.get(&w.key)?;
        std::ptr::eq(&self.overrides.history[*history], w).then_some(*kind)
    }
    pub fn raw(&self, key: &FormKey, b: &Budget) -> Result<Option<Record>> {
        let Some(w) = self.overrides.winner(key) else {
            return Ok(None);
        };
        let s = &self.sources[w.plugin].1;
        s.check_stable()?;
        let record = crate::record_at(s.as_ref(), w.offset, s.len(), &self.limits, b)?;
        s.check_stable()?;
        let mapped = self.order.remap(
            w.plugin,
            record.form_id,
            &record.source.file,
            record.source.offset + 12,
        )?;
        if record.signature != w.signature
            || record.deleted() != w.deleted
            || record.disabled() != w.disabled
            || mapped.as_ref().is_none_or(|k| {
                !k.plugin.eq_ignore_ascii_case(&key.plugin) || k.object_id != key.object_id
            })
        {
            return Err(Error::new(
                "IndexedRecordChanged",
                &record.source.file,
                record.source.offset,
                "indexed winning identity/type/flags",
                "different record on reread",
            ));
        }
        Ok(Some(record))
    }
    pub fn get<T: Typed>(&self, key: &FormKey, b: &Budget) -> Result<Option<Loaded<T>>> {
        let Some(w) = self.overrides.winner(key) else {
            return Ok(None);
        };
        if w.signature != T::SIGNATURE {
            return Err(Error::new(
                "RecordTypeMismatch",
                &self.order.headers()[w.plugin].source.file,
                w.offset,
                String::from_utf8_lossy(&T::SIGNATURE),
                String::from_utf8_lossy(&w.signature),
            ));
        }
        let Some(record) = self.raw(key, b)? else {
            return Ok(None);
        };
        // Field/consumption tables are scratch. Charge moved heap values to the
        // retained record budget before releasing the scratch allocation account.
        let scratch = b.child();
        let fields = record.fields(&self.limits, &scratch)?;
        for field in &fields {
            let heap = match &field.value {
                Value::Text(s) => s.capacity() as u64,
                Value::PartPositions(v) => fo4_core::mul(
                    v.capacity() as u64,
                    28,
                    &record.source.file,
                    record.source.offset,
                )?,
                Value::Light { parameters, .. } => fo4_core::mul(
                    parameters.capacity() as u64,
                    4,
                    &record.source.file,
                    record.source.offset,
                )?,
                _ => 0,
            };
            b.charge(heap, &record.source.file, record.source.offset)?;
        }
        let mut consumed = Vec::new();
        scratch.reserve(
            &mut consumed,
            fields.len(),
            &record.source.file,
            record.source.offset,
        )?;
        consumed.resize(fields.len(), false);
        let mut d = Decoder {
            fields,
            consumed,
            record: &record,
            order: &self.order,
            plugin: w.plugin,
            budget: b,
            localized: self.order.headers()[w.plugin].flags & crate::LOCALIZED != 0,
            limits: &self.limits,
        };
        let data = T::decode(&mut d).map_err(|mut e| {
            if record.payload_is_decompressed
                && e.source.decompressed_offset.is_none()
                && (e.source.offset != record.source.offset
                    || !matches!(
                        e.code.as_str(),
                        "LimitExceeded" | "AllocationFailed" | "Overflow"
                    ))
            {
                e.source.decompressed_offset = Some(e.source.offset);
                e.source.offset = record.source.offset + 24;
                e.source.length = record.source.length - 24;
            }
            e
        })?;
        b.charge(
            key.plugin.len() as u64,
            &record.source.file,
            record.source.offset,
        )?;
        Ok(Some(Loaded {
            key: key.clone(),
            plugin_index: w.plugin,
            record,
            data,
        }))
    }
    pub fn children(&self, key: &FormKey, b: &Budget) -> Result<Children> {
        let Some(cell) = self.overrides.winner(key) else {
            return Err(Error::new(
                "MissingCell",
                &key.plugin,
                0,
                "CELL winner",
                key.object_id,
            ));
        };
        if cell.signature != *b"CELL" {
            return Err(Error::new(
                "RecordTypeMismatch",
                &key.plugin,
                cell.offset,
                "CELL",
                String::from_utf8_lossy(&cell.signature),
            ));
        }
        let mut result = Children::default();
        let start = self.memberships.partition_point(|m| m.cell < cell.key);
        let end = self.memberships.partition_point(|m| m.cell <= cell.key);
        for m in &self.memberships[start..end] {
            let r = &self.overrides.history[m.history];
            if !self
                .overrides
                .winner_id(r.key)
                .is_some_and(|w| std::ptr::eq(w, r))
            {
                continue;
            }
            let output = if r.deleted {
                &mut result.deleted
            } else if r.disabled {
                &mut result.disabled
            } else if m.group_kind == 10 || r.signature != *b"REFR" {
                &mut result.excluded
            } else {
                &mut result.active
            };
            b.reserve(output, 1, &key.plugin, r.offset)?;
            let form = self.overrides.form_key(r.key, &self.order);
            b.charge(form.plugin.len() as u64, &key.plugin, r.offset)?;
            output.push(Child {
                key: form,
                signature: r.signature,
                group_kind: m.group_kind,
                source: fo4_core::SourceSpan::new(
                    &self.order.headers()[r.plugin].source.file,
                    r.offset,
                    r.length,
                ),
            });
        }
        Ok(result)
    }
    pub fn find_cell(&self, editor_id: &str, b: &Budget) -> Result<Option<FormKey>> {
        let mut found = None;
        for key in self.cells() {
            let work = b.child();
            if let Some(c) = self.get::<Cell>(&key, &work)? {
                if !c.record.deleted()
                    && c.data
                        .common
                        .editor_id
                        .as_deref()
                        .is_some_and(|s| s.eq_ignore_ascii_case(editor_id))
                {
                    if found.is_some() {
                        return Err(Error::new(
                            "AmbiguousCellEditorId",
                            &c.record.source.file,
                            c.record.source.offset,
                            "unique CELL EditorID",
                            editor_id,
                        ));
                    }
                    b.charge(
                        key.plugin.len() as u64,
                        &c.record.source.file,
                        c.record.source.offset,
                    )?;
                    found = Some(key);
                }
            }
        }
        Ok(found)
    }
}
fn compact_label(order: &LoadOrder, plugin: usize, g: &Group) -> Result<Option<u64>> {
    let Some(key) = order.remap(
        plugin,
        u32::from_le_bytes(g.label),
        &g.source.file,
        g.source.offset + 8,
    )?
    else {
        return Ok(None);
    };
    let index = order
        .headers()
        .iter()
        .position(|h| h.name.eq_ignore_ascii_case(&key.plugin))
        .ok_or_else(|| {
            Error::new(
                "MissingMaster",
                &g.source.file,
                g.source.offset + 8,
                "defining plugin",
                &key.plugin,
            )
        })?;
    Ok(Some(((index as u64) << 32) | key.object_id as u64))
}

fn retain_diagnostic(values: &mut Vec<Error>, e: Error, b: &Budget) -> Result<()> {
    let span = &e.source;
    let mut n = std::mem::size_of::<fo4_core::SourceSpan>() as u64;
    for size in [
        e.code.capacity(),
        e.expected.capacity(),
        e.found.capacity(),
        span.file.capacity(),
        span.archive_entry.as_ref().map_or(0, String::capacity),
        span.sha256.as_ref().map_or(0, String::capacity),
    ] {
        n = fo4_core::add(n, size as u64, &span.file, span.offset)?;
    }
    b.charge(n, &span.file, span.offset)?;
    b.reserve(values, 1, &span.file, span.offset)?;
    values.push(e);
    Ok(())
}
