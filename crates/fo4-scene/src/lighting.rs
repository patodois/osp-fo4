//! Lighting declarations only. No attenuation, inheritance, shading or runtime evaluation.
use crate::Scene;
use fo4_core::{Budget, Result, SourceSpan};
use fo4_plugin::{
    ficha,
    typed::{Common, Ligh, LightData, Loaded, Refr, Store},
    FormKey, Record,
};
use serde::Serialize;
use std::ops::Range;
#[derive(Debug, Serialize)]
pub struct NamedField {
    pub field: ficha::Field,
    pub record_source: SourceSpan,
    pub payload_is_decompressed: bool,
}
#[derive(Debug, Serialize)]
pub struct Gap {
    pub code: &'static str,
    pub field: &'static str,
    pub signature: [u8; 4],
    /// None means the declaration is absent, not a fabricated byte range.
    pub payload_bytes: Option<Range<usize>>,
    pub payload_is_decompressed: bool,
    pub record_source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct PlacedLight {
    pub reference: FormKey,
    pub base: Option<FormKey>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub position_rotation: Option<[f32; 6]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub reference_scale: Option<f32>,
    pub reference_flags: u32,
    pub nominal_radius: Option<u32>,
    pub color_rgb: Option<[u8; 3]>,
    pub unused_color_byte: Option<u8>,
    pub time: Option<i32>,
    pub raw_flags: Option<u32>,
    /// Exact printed declarations, not a default point/spot classification.
    pub type_declarations: Vec<&'static str>,
    pub base_fields: Vec<NamedField>,
    pub reference_fields: Vec<NamedField>,
    pub gaps: Vec<Gap>,
    pub fields_completely_described: bool,
}
#[derive(Debug, Serialize, Default)]
pub struct LightingData {
    /// Active and initially disabled light references; flags do not imply runtime state.
    pub placed: Vec<PlacedLight>,
    /// Owning raw payloads for fields/gaps of disabled lights, outside Scene.instances.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inactive_records: Vec<InactiveLightRecords>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreadable_inactive_placements: Vec<fo4_core::Error>,
    pub cell_fields: Vec<NamedField>,
    pub template_fields: Vec<NamedField>,
    pub cell_gaps: Vec<Gap>,
    pub lighting_inputs_completely_described: bool,
}
fn fields(record: &Record, common: &Common, b: &Budget) -> Result<Vec<NamedField>> {
    let mut out = Vec::new();
    for f in &common.ficha_fields {
        b.charge(
            record.source.file.len() as u64
                + record.source.archive_entry.as_ref().map_or(0, |s| s.len()) as u64,
            "light field source",
            record.source.offset,
        )?;
        if let ficha::Value::Form(v) = &f.value {
            if let Some(k) = &v.key {
                b.charge(
                    k.plugin.len() as u64,
                    "light FormID source",
                    record.source.offset,
                )?;
            }
        }
        b.reserve(&mut out, 1, "light fields", record.source.offset)?;
        out.push(NamedField {
            field: f.clone(),
            record_source: record.source.clone(),
            payload_is_decompressed: record.payload_is_decompressed,
        });
    }
    Ok(out)
}
fn gap(
    out: &mut Vec<Gap>,
    code: &'static str,
    field: &'static str,
    sig: [u8; 4],
    bytes: Range<usize>,
    record: &Record,
    b: &Budget,
) -> Result<()> {
    b.charge(
        record.source.file.len() as u64
            + record.source.archive_entry.as_ref().map_or(0, |s| s.len()) as u64,
        "light gap source",
        record.source.offset,
    )?;
    b.reserve(out, 1, "light gaps", record.source.offset)?;
    out.push(Gap {
        code,
        field,
        signature: sig,
        payload_bytes: if bytes.is_empty() { None } else { Some(bytes) },
        payload_is_decompressed: record.payload_is_decompressed,
        record_source: record.source.clone(),
    });
    Ok(())
}
fn raw_gaps(out: &mut Vec<Gap>, record: &Record, common: &Common, b: &Budget) -> Result<()> {
    for raw in &common.uninterpreted {
        let relevant = matches!(
            (&record.signature, &raw.signature),
            (b"CELL", b"XCLL") | (b"LGTM", b"DATA") | (b"REFR", b"XLIG")
        ) || (record.signature != *b"CELL"
            && record.signature != *b"LGTM"
            && fo4_plugin::still::classify_size(
                &record.signature,
                &raw.signature,
                raw.bytes.len(),
            ) != fo4_plugin::still::Class::Outside);
        if relevant {
            gap(
                out,
                "UninterpretedLightingField",
                raw.open_field.unwrap_or("uninterpreted"),
                raw.signature,
                raw.bytes.clone(),
                record,
                b,
            )?;
        }
    }
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct InactiveLightRecords {
    pub reference: Record,
    pub base: Record,
}
fn retain_record_credit(record: &Record, b: &Budget) -> Result<()> {
    let mut bytes = fo4_core::add(
        record.payload.capacity() as u64,
        fo4_core::mul(
            record.subrecords.capacity() as u64,
            std::mem::size_of::<fo4_plugin::Subrecord>() as u64,
            &record.source.file,
            record.source.offset,
        )?,
        &record.source.file,
        record.source.offset,
    )?;
    for s in [
        Some(&record.source.file),
        record.source.archive_entry.as_ref(),
        record.source.sha256.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        bytes = fo4_core::add(
            bytes,
            s.capacity() as u64,
            &record.source.file,
            record.source.offset,
        )?;
    }
    // Transfer surviving allocations out of the scoped decoder; conservatively
    // charge both accounts during transfer, without cloning the payloads.
    b.charge(bytes, &record.source.file, record.source.offset)
}
pub(crate) fn describe(scene: &Scene, store: &Store, b: &Budget) -> Result<LightingData> {
    let mut out = LightingData {
        cell_fields: fields(&scene.cell.record, &scene.cell.data.common, b)?,
        ..LightingData::default()
    };
    raw_gaps(
        &mut out.cell_gaps,
        &scene.cell.record,
        &scene.cell.data.common,
        b,
    )?;
    if let Some(t) = &scene.lighting_template {
        out.template_fields = fields(&t.record, &t.data.common, b)?;
        raw_gaps(&mut out.cell_gaps, &t.record, &t.data.common, b)?;
        if t.record.deleted() {
            gap(
                &mut out.cell_gaps,
                "DeletedLightingTemplate",
                "LGTM",
                *b"LTMP",
                0..0,
                &t.record,
                b,
            )?;
        }
    }
    if !scene
        .cell
        .record
        .subrecords
        .iter()
        .any(|s| s.signature == *b"XCLL")
        && out.template_fields.is_empty()
    {
        gap(
            &mut out.cell_gaps,
            "MissingCellLightingDeclaration",
            "CELL.XCLL/LTMP",
            *b"XCLL",
            0..0,
            &scene.cell.record,
            b,
        )?;
    }
    for i in &scene.instances {
        let Some(base) = i.base.as_ref().filter(|v| v.record.signature == *b"LIGH") else {
            continue;
        };
        let light = placed(
            &i.reference,
            LightBase {
                key: &base.key,
                record: &base.record,
                common: &base.common,
                light: base.light.as_ref(),
            },
            b,
        )?;
        b.reserve(
            &mut out.placed,
            1,
            "placed light data",
            i.reference.record.source.offset,
        )?;
        out.placed.push(light);
    }
    // Metadata only: no material/model load, drawing, enable-parent evaluation or default.
    for child in &scene.disabled {
        if child.signature != *b"REFR" {
            continue;
        }
        let work = b.child();
        let got = (|| -> Result<Option<(PlacedLight, InactiveLightRecords)>> {
            let reference = store.get::<Refr>(&child.key, &work)?.ok_or_else(|| {
                fo4_core::Error::new(
                    "MissingInactiveReference",
                    &child.source.file,
                    child.source.offset,
                    "REFR winner",
                    child.key.object_id,
                )
            })?;
            let key = reference
                .data
                .base
                .as_ref()
                .and_then(|v| v.key.as_ref())
                .ok_or_else(|| {
                    fo4_core::Error::new(
                        "UnknownInactiveBaseType",
                        &child.source.file,
                        child.source.offset,
                        "explicit NAME to classify possible LIGH",
                        "absent",
                    )
                })?;
            let winner = store.overrides().winner(key).ok_or_else(|| {
                fo4_core::Error::new(
                    "MissingInactiveBase",
                    &child.source.file,
                    child.source.offset,
                    "base winner",
                    key.object_id,
                )
            })?;
            if winner.signature != *b"LIGH" {
                return Ok(None);
            }
            let base = store.get::<Ligh>(key, &work)?.ok_or_else(|| {
                fo4_core::Error::new(
                    "MissingInactiveLightBase",
                    &child.source.file,
                    child.source.offset,
                    "LIGH winner",
                    key.object_id,
                )
            })?;
            let light = placed(
                &reference,
                LightBase {
                    key: &base.key,
                    record: &base.record,
                    common: &base.data.common,
                    light: base.data.light.as_ref(),
                },
                b,
            )?;
            retain_record_credit(&reference.record, b)?;
            retain_record_credit(&base.record, b)?;
            Ok(Some((
                light,
                InactiveLightRecords {
                    reference: reference.record,
                    base: base.record,
                },
            )))
        })();
        match got {
            Ok(Some((light, records))) => {
                b.reserve(
                    &mut out.inactive_records,
                    1,
                    "inactive light raw records",
                    child.source.offset,
                )?;
                out.inactive_records.push(records);
                b.reserve(
                    &mut out.placed,
                    1,
                    "inactive light declarations",
                    child.source.offset,
                )?;
                out.placed.push(light);
            }
            Ok(None) => {}
            Err(e) => crate::push_error(&mut out.unreadable_inactive_placements, e, b)?,
        }
    }
    out.lighting_inputs_completely_described = out.cell_gaps.is_empty()
        && out.unreadable_inactive_placements.is_empty()
        && out.placed.iter().all(|l| l.fields_completely_described);
    Ok(out)
}

struct LightBase<'a> {
    key: &'a FormKey,
    record: &'a Record,
    common: &'a Common,
    light: Option<&'a LightData>,
}
fn placed(reference: &Loaded<Refr>, base: LightBase<'_>, b: &Budget) -> Result<PlacedLight> {
    b.charge(
        reference.key.plugin.len() as u64 + base.key.plugin.len() as u64,
        "light identity",
        reference.record.source.offset,
    )?;
    let data = base.light;
    let mut light = PlacedLight {
        reference: reference.key.clone(),
        base: Some(base.key.clone()),
        position_rotation: reference.data.transform,
        reference_scale: reference.data.scale,
        reference_flags: reference.record.flags,
        nominal_radius: data.map(|v| v.radius),
        color_rgb: data.map(|v| [v.color[0], v.color[1], v.color[2]]),
        unused_color_byte: data.map(|v| v.color[3]),
        time: data.map(|v| v.time),
        raw_flags: data.map(|v| v.flags),
        type_declarations: Vec::new(),
        base_fields: fields(base.record, base.common, b)?,
        reference_fields: fields(&reference.record, &reference.data.common, b)?,
        gaps: Vec::new(),
        fields_completely_described: false,
    };
    if data.is_none() {
        gap(
            &mut light.gaps,
            "MissingLightData",
            "LIGH.DATA",
            *b"DATA",
            0..0,
            base.record,
            b,
        )?;
    }
    for f in &base.common.ficha_fields {
        if base.record.subrecords[f.subrecord].signature != *b"DATA" {
            continue;
        }
        if let ficha::Value::Flags {
            printed_labels: Some(labels),
            fully_named,
            ..
        } = f.value
        {
            for name in labels.split(", ") {
                if matches!(
                    name,
                    "Shadow Spotlight"
                        | "Shadow Hemisphere"
                        | "Shadow OmniDirectional"
                        | "NonShadow Spotlight"
                ) {
                    b.reserve(
                        &mut light.type_declarations,
                        1,
                        "light type declarations",
                        f.bytes.start as u64,
                    )?;
                    light.type_declarations.push(name);
                }
            }
            if !fully_named {
                gap(
                    &mut light.gaps,
                    "UnmappedLightFlags",
                    "Flags",
                    *b"DATA",
                    f.bytes.clone(),
                    base.record,
                    b,
                )?;
            }
        }
    }
    if light.type_declarations.is_empty() {
        gap(
            &mut light.gaps,
            "UndeclaredLightType",
            "Flags contain no sourced type declaration; no point-light default",
            *b"DATA",
            base.common
                .ficha_fields
                .iter()
                .find(|f| {
                    base.record.subrecords[f.subrecord].signature == *b"DATA"
                        && matches!(f.value, ficha::Value::Flags { .. })
                })
                .map_or(0..0, |f| f.bytes.clone()),
            base.record,
            b,
        )?;
    }
    raw_gaps(&mut light.gaps, base.record, base.common, b)?;
    raw_gaps(
        &mut light.gaps,
        &reference.record,
        &reference.data.common,
        b,
    )?;
    if reference.data.transform.is_none() {
        gap(
            &mut light.gaps,
            "MissingLightPlacement",
            "REFR.DATA",
            *b"DATA",
            0..0,
            &reference.record,
            b,
        )?;
    }
    light.fields_completely_described = light.gaps.is_empty();
    Ok(light)
}
