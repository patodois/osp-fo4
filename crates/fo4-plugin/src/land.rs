//! Typed view of LAND, the landscape record of an exterior cell (`spec/land.md`).
//!
//! Only byte layouts are decoded here, each from the public documentation
//! cited in the spec. How the stored values become world positions (height
//! unit, accumulation order, grid origin and spacing, which LAND belongs to a
//! cell) is not settled by those sources for Fallout 4 and lives in
//! registered rules applied by `fo4_scene::terrain`. Every subrecord that is
//! not decoded, or whose size differs from the documented one, stays in
//! `uninterpreted` with its payload range.
use crate::typed::{FormRef, RawSubrecord};
use crate::{LoadOrder, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Vertices per side of the stored grids.
pub const SIDE: usize = 33;
/// Vertices of one stored grid, 33 x 33.
pub const POINTS: usize = SIDE * SIDE;
/// VHGT: f32 offset, one signed byte per vertex, three trailing bytes.
pub const VHGT_SIZE: usize = 4 + POINTS + 3;
/// VNML and VCLR: three bytes per vertex.
pub const POINT_GRID_SIZE: usize = 3 * POINTS;
/// BTXT and ATXT.
pub const LAYER_HEADER_SIZE: usize = 8;
/// One VTXT entry.
pub const ALPHA_POINT_SIZE: usize = 8;

/// DATA: four bytes; the sources describe the first as a bit mask of the
/// subrecords present and the others as unknown. Kept as stored.
#[derive(Debug, Clone, Serialize)]
pub struct DataFlags {
    pub subrecord: usize,
    pub raw: [u8; 4],
}
/// VHGT as stored: the values are not combined here.
#[derive(Debug, Clone, Serialize)]
pub struct Heights {
    pub subrecord: usize,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub offset: f32,
    /// One signed byte per vertex, in stored order.
    pub gradient: Vec<i8>,
    /// The three bytes after the grid; no meaning is claimed.
    pub trailing: [u8; 3],
}
/// VNML or VCLR: three bytes per vertex in stored order. Signedness of the
/// normal components and the colour space are not claimed.
#[derive(Debug, Clone, Serialize)]
pub struct PointGrid {
    pub subrecord: usize,
    pub values: Vec<[u8; 3]>,
}
/// BTXT: texture, quadrant and three bytes the sources leave unknown.
#[derive(Debug, Clone, Serialize)]
pub struct BaseLayer {
    pub subrecord: usize,
    pub texture: Option<FormRef>,
    pub quadrant: u8,
    pub raw: [u8; 3],
}
/// ATXT: texture, quadrant, one unknown byte and the layer number.
#[derive(Debug, Clone, Serialize)]
pub struct AlphaHeader {
    pub subrecord: usize,
    pub texture: Option<FormRef>,
    pub quadrant: u8,
    pub unknown: u8,
    pub layer: u16,
}
/// One VTXT entry: position in the quadrant, two unknown bytes, opacity.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AlphaPoint {
    pub position: u16,
    pub unknown: [u8; 2],
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub opacity: f32,
}
/// An ATXT and the VTXT subrecords that immediately follow it.
#[derive(Debug, Clone, Serialize)]
pub struct AlphaLayer {
    pub header: AlphaHeader,
    pub point_subrecords: Vec<usize>,
    pub points: Vec<AlphaPoint>,
}
#[derive(Debug, Serialize)]
pub struct Land {
    pub data: Option<DataFlags>,
    pub heights: Option<Heights>,
    pub normals: Option<PointGrid>,
    pub colors: Option<PointGrid>,
    pub base_layers: Vec<BaseLayer>,
    pub alpha_layers: Vec<AlphaLayer>,
    /// VTEX FormIDs in stored order.
    pub textures: Vec<Option<FormRef>>,
    /// Subrecords not decoded: unknown signatures, undocumented sizes and
    /// VTXT without a preceding ATXT. `open_field` names the reason.
    pub uninterpreted: Vec<RawSubrecord>,
    /// Non-finite VHGT offsets and VTXT opacities, preserved.
    pub non_finite_values: u64,
}

fn duplicate(record: &Record, i: usize) -> Error {
    let s = &record.subrecords[i];
    Error::new(
        "DuplicateLandField",
        &record.source.file,
        s.offset,
        "at most one per LAND",
        String::from_utf8_lossy(&s.signature),
    )
}
fn form(
    record: &Record,
    order: &LoadOrder,
    plugin: usize,
    i: usize,
    raw: u32,
) -> Result<Option<FormRef>> {
    if raw == 0 {
        return Ok(None);
    }
    Ok(Some(FormRef {
        raw,
        key: order.remap(
            plugin,
            raw,
            &record.source.file,
            record.subrecords[i].offset,
        )?,
        subrecord: i,
    }))
}
fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
}
fn raw(
    out: &mut Vec<RawSubrecord>,
    record: &Record,
    i: usize,
    why: Option<&'static str>,
    b: &Budget,
) -> Result<()> {
    let s = &record.subrecords[i];
    b.reserve(out, 1, &record.source.file, s.offset)?;
    out.push(RawSubrecord {
        signature: s.signature,
        subrecord: i,
        bytes: s.data.clone(),
        open_field: why,
    });
    Ok(())
}
fn grid(record: &Record, i: usize, b: &Budget) -> Result<PointGrid> {
    let d = record.data(&record.subrecords[i]);
    let mut values = Vec::new();
    b.reserve(
        &mut values,
        POINTS,
        &record.source.file,
        record.subrecords[i].offset,
    )?;
    values.extend(d.as_chunks::<3>().0.iter().copied());
    Ok(PointGrid {
        subrecord: i,
        values,
    })
}

impl Land {
    /// Decode a LAND record read from plugin `plugin` of `order`. FormIDs are
    /// remapped through that plugin's masters. Errors on a compressed record
    /// carry the payload position in `decompressed_offset`.
    pub fn decode(record: &Record, order: &LoadOrder, plugin: usize, b: &Budget) -> Result<Self> {
        Self::decode_inner(record, order, plugin, b).map_err(|mut e| {
            if record.payload_is_decompressed
                && e.source.decompressed_offset.is_none()
                && e.source.offset != record.source.offset
            {
                e.source.decompressed_offset = Some(e.source.offset);
                e.source.offset = record.source.offset + 24;
                e.source.length = record.source.length.saturating_sub(24);
            }
            e
        })
    }
    fn decode_inner(record: &Record, order: &LoadOrder, plugin: usize, b: &Budget) -> Result<Self> {
        if record.signature != *b"LAND" {
            return Err(Error::new(
                "RecordTypeMismatch",
                &record.source.file,
                record.source.offset,
                "LAND",
                String::from_utf8_lossy(&record.signature),
            ));
        }
        let file = &record.source.file;
        let mut land = Land {
            data: None,
            heights: None,
            normals: None,
            colors: None,
            base_layers: Vec::new(),
            alpha_layers: Vec::new(),
            textures: Vec::new(),
            uninterpreted: Vec::new(),
            non_finite_values: 0,
        };
        // Index into alpha_layers while VTXT may still attach to it.
        let mut open_alpha: Option<usize> = None;
        for (i, s) in record.subrecords.iter().enumerate() {
            let d = record.data(s);
            let attach = open_alpha.take();
            match (&s.signature, d.len()) {
                (b"DATA", 4) => {
                    if land.data.is_some() {
                        return Err(duplicate(record, i));
                    }
                    land.data = Some(DataFlags {
                        subrecord: i,
                        raw: [d[0], d[1], d[2], d[3]],
                    });
                }
                (b"VHGT", VHGT_SIZE) => {
                    if land.heights.is_some() {
                        return Err(duplicate(record, i));
                    }
                    let offset = f32::from_bits(u32_at(d, 0));
                    land.non_finite_values += u64::from(!offset.is_finite());
                    let mut gradient = Vec::new();
                    b.reserve(&mut gradient, POINTS, file, s.offset)?;
                    gradient.extend(d[4..4 + POINTS].iter().map(|v| *v as i8));
                    land.heights = Some(Heights {
                        subrecord: i,
                        offset,
                        gradient,
                        trailing: [d[4 + POINTS], d[5 + POINTS], d[6 + POINTS]],
                    });
                }
                (b"VNML", POINT_GRID_SIZE) => {
                    if land.normals.is_some() {
                        return Err(duplicate(record, i));
                    }
                    land.normals = Some(grid(record, i, b)?);
                }
                (b"VCLR", POINT_GRID_SIZE) => {
                    if land.colors.is_some() {
                        return Err(duplicate(record, i));
                    }
                    land.colors = Some(grid(record, i, b)?);
                }
                (b"BTXT", LAYER_HEADER_SIZE) => {
                    let texture = form(record, order, plugin, i, u32_at(d, 0))?;
                    b.reserve(&mut land.base_layers, 1, file, s.offset)?;
                    land.base_layers.push(BaseLayer {
                        subrecord: i,
                        texture,
                        quadrant: d[4],
                        raw: [d[5], d[6], d[7]],
                    });
                }
                (b"ATXT", LAYER_HEADER_SIZE) => {
                    let texture = form(record, order, plugin, i, u32_at(d, 0))?;
                    b.reserve(&mut land.alpha_layers, 1, file, s.offset)?;
                    land.alpha_layers.push(AlphaLayer {
                        header: AlphaHeader {
                            subrecord: i,
                            texture,
                            quadrant: d[4],
                            unknown: d[5],
                            layer: u16::from_le_bytes([d[6], d[7]]),
                        },
                        point_subrecords: Vec::new(),
                        points: Vec::new(),
                    });
                    open_alpha = Some(land.alpha_layers.len() - 1);
                }
                (b"VTXT", n) if n % ALPHA_POINT_SIZE == 0 && attach.is_some() => {
                    let layer = &mut land.alpha_layers[attach.unwrap_or_default()];
                    b.reserve(&mut layer.point_subrecords, 1, file, s.offset)?;
                    layer.point_subrecords.push(i);
                    b.reserve(&mut layer.points, n / ALPHA_POINT_SIZE, file, s.offset)?;
                    for c in d.as_chunks::<ALPHA_POINT_SIZE>().0 {
                        let opacity = f32::from_bits(u32_at(c, 4));
                        land.non_finite_values += u64::from(!opacity.is_finite());
                        layer.points.push(AlphaPoint {
                            position: u16::from_le_bytes([c[0], c[1]]),
                            unknown: [c[2], c[3]],
                            opacity,
                        });
                    }
                    open_alpha = attach;
                }
                (b"VTXT", n) => raw(
                    &mut land.uninterpreted,
                    record,
                    i,
                    Some(if n % ALPHA_POINT_SIZE != 0 {
                        "vtxt_size_not_multiple_of_8"
                    } else {
                        "vtxt_without_atxt"
                    }),
                    b,
                )?,
                (b"VTEX", n) if n % 4 == 0 => {
                    if !land.textures.is_empty() {
                        return Err(duplicate(record, i));
                    }
                    b.reserve(&mut land.textures, n / 4, file, s.offset)?;
                    for k in 0..n / 4 {
                        land.textures
                            .push(form(record, order, plugin, i, u32_at(d, 4 * k))?);
                    }
                }
                (b"DATA" | b"VHGT" | b"VNML" | b"VCLR" | b"BTXT" | b"ATXT" | b"VTEX", _) => raw(
                    &mut land.uninterpreted,
                    record,
                    i,
                    Some("unverified_size"),
                    b,
                )?,
                _ => raw(&mut land.uninterpreted, record, i, None, b)?,
            }
        }
        Ok(land)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scan, Header, LoadOrder, COMPRESSED};
    use fo4_core::{Limits, SliceSource};
    use std::io::Write;

    fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = sig.to_vec();
        v.extend((data.len() as u16).to_le_bytes());
        v.extend(data);
        v
    }
    fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
        let body = if flags & COMPRESSED != 0 {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(data).unwrap();
            let mut v = (data.len() as u32).to_le_bytes().to_vec();
            v.extend(z.finish().unwrap());
            v
        } else {
            data.to_vec()
        };
        let mut v = sig.to_vec();
        for n in [body.len() as u32, flags, id, 0] {
            v.extend(n.to_le_bytes());
        }
        v.extend(131u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(body);
        v
    }
    fn plugin(masters: &[&str], body: &[u8]) -> Vec<u8> {
        let mut hedr = 0.95f32.to_le_bytes().to_vec();
        hedr.extend(1u32.to_le_bytes());
        hedr.extend(0x900u32.to_le_bytes());
        let mut data = sub(b"HEDR", &hedr);
        for m in masters {
            data.extend(sub(b"MAST", &[m.as_bytes(), &[0]].concat()));
            data.extend(sub(b"DATA", &[0; 8]));
        }
        let mut v = record(b"TES4", 0, 0, &data);
        v.extend(body);
        v
    }
    pub(crate) fn vhgt(offset: f32, value: impl Fn(usize) -> i8, trailing: [u8; 3]) -> Vec<u8> {
        let mut v = offset.to_le_bytes().to_vec();
        v.extend((0..POINTS).map(|i| value(i) as u8));
        v.extend(trailing);
        v
    }
    /// Decodes the single LAND of a one-plugin file named `synthetic.esp`
    /// (or with one master `base.esm`).
    fn decode(payload: &[u8], flags: u32, masters: &[&str]) -> Result<Land> {
        let bytes = plugin(masters, &record(b"LAND", 0x0100_0800, flags, payload));
        let source = SliceSource {
            name: "synthetic.esp",
            bytes: &bytes,
        };
        let l = Limits::default();
        let b = Budget::new(1 << 20);
        let h = Header::read("synthetic.esp", &source, &l, &b)?;
        let mut headers = vec![h];
        for m in masters {
            let master = plugin(&[], &[]);
            headers.insert(
                0,
                Header::read(
                    m,
                    &SliceSource {
                        name: m,
                        bytes: &master,
                    },
                    &l,
                    &b,
                )?,
            );
        }
        let plugin_index = headers.len() - 1;
        let order = LoadOrder::new(headers, &b)?;
        let mut out = None;
        scan(&source, &order.headers()[plugin_index], &l, |r, _| {
            if r.signature == *b"LAND" {
                out = Some(Land::decode(&r, &order, plugin_index, &b));
            }
            Ok(())
        })?;
        out.expect("one LAND")
    }

    #[test]
    fn documented_subrecords_decode_and_keep_every_unknown_byte() {
        let mut p = sub(b"DATA", &[1, 2, 3, 4]);
        p.extend(sub(b"VNML", &[0x81; POINT_GRID_SIZE]));
        p.extend(sub(b"VHGT", &vhgt(-2.5, |i| (i % 7) as i8 - 3, [9, 8, 7])));
        p.extend(sub(b"VCLR", &[0xfe; POINT_GRID_SIZE]));
        p.extend(sub(b"BTXT", &[0x10, 8, 0, 1, 2, 0xaa, 0xbb, 0xcc]));
        p.extend(sub(b"ATXT", &[0x11, 8, 0, 1, 3, 0x55, 4, 0]));
        let mut points = Vec::new();
        for (pos, o) in [(5u16, 0.25f32), (288, f32::NAN)] {
            points.extend(pos.to_le_bytes());
            points.extend([0x12, 0x34]);
            points.extend(o.to_le_bytes());
        }
        p.extend(sub(b"VTXT", &points));
        p.extend(sub(b"VTXT", &points[..8]));
        p.extend(sub(b"VTEX", &[0, 0, 0, 0, 0x12, 8, 0, 0]));
        p.extend(sub(b"ZZZZ", &[1, 2]));
        let land = decode(&p, COMPRESSED, &["base.esm"]).unwrap();
        assert_eq!(land.data.as_ref().unwrap().raw, [1, 2, 3, 4]);
        let h = land.heights.as_ref().unwrap();
        assert_eq!((h.offset, h.trailing), (-2.5, [9, 8, 7]));
        assert_eq!(
            (h.gradient[0], h.gradient[6], h.gradient.len()),
            (-3, 3, POINTS)
        );
        assert_eq!(land.normals.as_ref().unwrap().values[1088], [0x81; 3]);
        assert_eq!(land.colors.as_ref().unwrap().values.len(), POINTS);
        let base = &land.base_layers[0];
        assert_eq!((base.quadrant, base.raw), (2, [0xaa, 0xbb, 0xcc]));
        // Index 1 of a one-master plugin is the plugin itself.
        let key = base.texture.as_ref().unwrap().key.as_ref().unwrap();
        assert_eq!(
            (key.plugin.as_str(), key.object_id),
            ("synthetic.esp", 0x810)
        );
        let alpha = &land.alpha_layers[0];
        assert_eq!(
            (
                alpha.header.quadrant,
                alpha.header.unknown,
                alpha.header.layer
            ),
            (3, 0x55, 4)
        );
        assert_eq!(alpha.point_subrecords.len(), 2);
        assert_eq!(alpha.points.len(), 3);
        assert_eq!(
            (alpha.points[1].position, alpha.points[1].unknown),
            (288, [0x12, 0x34])
        );
        assert!(alpha.points[1].opacity.is_nan());
        assert_eq!(land.non_finite_values, 1);
        assert!(land.textures[0].is_none());
        let vtex = land.textures[1].as_ref().unwrap().key.as_ref().unwrap();
        assert_eq!((vtex.plugin.as_str(), vtex.object_id), ("base.esm", 0x812));
        assert_eq!(land.uninterpreted.len(), 1);
        assert_eq!(&land.uninterpreted[0].signature, b"ZZZZ");
        assert_eq!(land.uninterpreted[0].open_field, None);
    }

    #[test]
    fn undocumented_sizes_and_orphan_points_stay_raw() {
        let mut p = sub(b"DATA", &[1, 2, 3]);
        p.extend(sub(b"VHGT", &vhgt(0.0, |_| 0, [0; 3])[..VHGT_SIZE - 1]));
        p.extend(sub(b"VNML", &[0; POINT_GRID_SIZE + 3]));
        p.extend(sub(b"VTXT", &[0; 8]));
        p.extend(sub(b"ATXT", &[0; 8]));
        p.extend(sub(b"VTXT", &[0; 9]));
        p.extend(sub(b"BTXT", &[0; 8]));
        p.extend(sub(b"VTXT", &[0; 8]));
        let land = decode(&p, 0, &[]).unwrap();
        assert!(land.data.is_none() && land.heights.is_none() && land.normals.is_none());
        let reasons: Vec<_> = land
            .uninterpreted
            .iter()
            .map(|r| (r.signature, r.open_field))
            .collect();
        assert_eq!(
            reasons,
            [
                (*b"DATA", Some("unverified_size")),
                (*b"VHGT", Some("unverified_size")),
                (*b"VNML", Some("unverified_size")),
                (*b"VTXT", Some("vtxt_without_atxt")),
                (*b"VTXT", Some("vtxt_size_not_multiple_of_8")),
                (*b"VTXT", Some("vtxt_without_atxt")),
            ]
        );
        assert!(land.alpha_layers[0].points.is_empty());
        assert_eq!(land.base_layers.len(), 1);
    }

    #[test]
    fn duplicates_and_bad_form_ids_are_named_errors_with_payload_offsets() {
        let one = sub(b"VHGT", &vhgt(0.0, |_| 0, [0; 3]));
        let e = decode(&[one.clone(), one].concat(), COMPRESSED, &[]).unwrap_err();
        assert_eq!(e.code, "DuplicateLandField");
        assert_eq!(e.source.decompressed_offset, Some(6 + VHGT_SIZE as u64));
        // Master index 5 does not exist in a plugin with no masters.
        let e = decode(&sub(b"BTXT", &[1, 0, 0, 5, 0, 0, 0, 0]), 0, &[]).unwrap_err();
        assert_eq!(e.code, "InvalidFormId");
        assert_eq!(e.source.decompressed_offset, None);
    }

    #[test]
    fn every_truncation_and_byte_value_of_a_land_is_bounded() {
        let mut p = sub(b"VHGT", &vhgt(1.0, |i| i as i8, [0; 3]));
        p.extend(sub(b"ATXT", &[0; 8]));
        p.extend(sub(b"VTXT", &[0; 16]));
        // Truncated payloads fail in the envelope reader or decode the prefix.
        for n in 0..p.len() {
            let _ = decode(&p[..n], 0, &[]);
        }
        for at in [0, 4, 5, 6, 10, VHGT_SIZE + 6, VHGT_SIZE + 10] {
            for value in [0u8, 0x7f, 0x80, 0xff] {
                let mut q = p.clone();
                q[at] = value;
                let _ = decode(&q, 0, &[]);
            }
        }
        let small = Budget::new(64);
        let bytes = plugin(&[], &record(b"LAND", 0x800, 0, &p));
        let source = SliceSource {
            name: "x",
            bytes: &bytes,
        };
        let l = Limits::default();
        let h = Header::read("x", &source, &l, &Budget::new(1 << 20)).unwrap();
        let order = LoadOrder::new(vec![h], &Budget::new(1 << 20)).unwrap();
        scan(&source, &order.headers()[0], &l, |r, _| {
            if r.signature == *b"LAND" {
                assert_eq!(
                    Land::decode(&r, &order, 0, &small).unwrap_err().code,
                    "LimitExceeded"
                );
            }
            Ok(())
        })
        .unwrap();
    }
}
