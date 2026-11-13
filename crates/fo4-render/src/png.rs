//! Own PNG framing: signature, IHDR RGB8, IDAT zlib, IEND. No external image library.
use flate2::{write::ZlibEncoder, Compression};
use fo4_core::{Budget, Error, Result};
use std::io::Write;
fn crc(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for x in bytes {
        c ^= *x as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320u32.wrapping_mul(c & 1));
        }
    }
    !c
}
fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8], b: &Budget) -> Result<()> {
    let len =
        u32::try_from(data.len()).map_err(|e| Error::new("PngSize", "PNG", 0, "u32 chunk", e))?;
    let count = data
        .len()
        .checked_add(12)
        .ok_or_else(|| Error::new("PngSize", "PNG", 0, "bounded chunk", data.len()))?;
    b.reserve(out, count, "PNG", 0)?;
    out.extend_from_slice(&len.to_be_bytes());
    let at = out.len();
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let sum = crc(&out[at..]);
    out.extend_from_slice(&sum.to_be_bytes());
    Ok(())
}
struct Sink<'a> {
    bytes: Vec<u8>,
    b: &'a Budget,
}
impl Write for Sink<'_> {
    fn write(&mut self, v: &[u8]) -> std::io::Result<usize> {
        self.b
            .reserve(&mut self.bytes, v.len(), "PNG zlib", 0)
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(v);
        Ok(v.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn encode(size: [u32; 2], pixels: &[[f32; 4]], b: &Budget) -> Result<Vec<u8>> {
    encode_with_transfer(size, pixels, false, b)
}
fn encode_with_transfer(
    size: [u32; 2],
    pixels: &[[f32; 4]],
    srgb: bool,
    b: &Budget,
) -> Result<Vec<u8>> {
    let [w, h] = size;
    let count = fo4_core::mul(w as u64, h as u64, "PNG", 0)?;
    if w == 0
        || h == 0
        || count != pixels.len() as u64
        || pixels.iter().flatten().any(|x| !x.is_finite())
    {
        return Err(Error::new(
            "InvalidPngPixels",
            "PNG",
            0,
            "finite width*height RGBA",
            pixels.len(),
        ));
    }
    let scratch = b.child();
    scratch.charge(512 * 1024, "PNG zlib workspace", 0)?;
    let mut z = ZlibEncoder::new(
        Sink {
            bytes: Vec::new(),
            b: &scratch,
        },
        Compression::new(6),
    );
    for row in pixels.chunks_exact(w as usize) {
        z.write_all(&[0])
            .map_err(|e| Error::new("PngCompression", "PNG", 0, "filter byte", e))?;
        for p in row {
            let c = std::array::from_fn::<_, 3, _>(|i| {
                ((if srgb {
                    srgb_encode(p[i])
                } else {
                    p[i].clamp(0., 1.)
                }) * 255.)
                    .round() as u8
            });
            z.write_all(&c)
                .map_err(|e| Error::new("PngCompression", "PNG", 0, "RGB", e))?;
        }
    }
    let packed = z
        .finish()
        .map_err(|e| Error::new("PngCompression", "PNG", 0, "finished zlib", e))?
        .bytes;
    let mut out = Vec::new();
    b.reserve(&mut out, 8, "PNG", 0)?;
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&w.to_be_bytes());
    ihdr[4..8].copy_from_slice(&h.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 2;
    chunk(&mut out, b"IHDR", &ihdr, b)?;
    if srgb {
        chunk(&mut out, b"sRGB", &[0], b)?;
    }
    chunk(&mut out, b"IDAT", &packed, b)?;
    chunk(&mut out, b"IEND", &[], b)?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_crc_matches_png_iend_and_budget_is_enforced() {
        assert_eq!(crc(b"IEND"), 0xae426082);
        assert!(encode([1, 1], &[[1.; 4]], &Budget::new(1024)).is_err());
    }
}

#[derive(Debug)]
pub struct Png {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub ancillary_chunks: Vec<([u8; 4], Vec<u8>)>,
}
fn png_error(
    code: &str,
    at: usize,
    expected: impl std::fmt::Display,
    found: impl std::fmt::Display,
) -> Error {
    Error::new(code, "PNG", at as u64, expected, found)
}
fn take<'a>(input: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8]> {
    let end = at
        .checked_add(n)
        .ok_or_else(|| png_error("Overflow", *at, "offset+length", n))?;
    let bytes = input
        .get(*at..end)
        .ok_or_else(|| png_error("TruncatedPng", *at, n, input.len().saturating_sub(*at)))?;
    *at = end;
    Ok(bytes)
}
/// Original bounded PNG decoder: non-interlaced 8-bit grey/RGB/indexed/grey-alpha/RGBA.
pub fn decode(input: &[u8], l: &fo4_core::Limits, b: &Budget) -> Result<Png> {
    fo4_core::limit(input.len() as u64, l.max_payload, "PNG", 0)?;
    let mut at = 0;
    if take(input, &mut at, 8)? != b"\x89PNG\r\n\x1a\n" {
        return Err(png_error(
            "InvalidPngSignature",
            0,
            "PNG signature",
            "mismatch",
        ));
    }
    let scratch = b.child();
    let mut header = None;
    let mut packed = Vec::new();
    let mut ancillary = Vec::new();
    let mut palette = None;
    let mut transparency = None;
    let mut ended = false;
    let mut seen_data = false;
    let mut data_closed = false;
    let mut chunks = 0u64;
    while at < input.len() {
        chunks = fo4_core::add(chunks, 1, "PNG", at as u64)?;
        fo4_core::limit(chunks, l.max_entries, "PNG chunks", at as u64)?;
        let start = at;
        let len = u32::from_be_bytes(
            take(input, &mut at, 4)?
                .try_into()
                .map_err(|e| png_error("PngLength", start, "u32", e))?,
        ) as usize;
        let tag: [u8; 4] = take(input, &mut at, 4)?
            .try_into()
            .map_err(|e| png_error("PngTag", start, "four letters", e))?;
        if !tag.iter().all(u8::is_ascii_alphabetic) || tag[2] & 32 != 0 {
            return Err(png_error(
                "InvalidPngTag",
                start,
                "valid chunk type",
                fo4_core::HexBytes(&tag),
            ));
        }
        fo4_core::limit(len as u64, l.max_payload, "PNG chunk", start as u64)?;
        let data = take(input, &mut at, len)?;
        let checksum = u32::from_be_bytes(
            take(input, &mut at, 4)?
                .try_into()
                .map_err(|e| png_error("PngCrc", start, "u32", e))?,
        );
        let calculated = crc(&input[start + 4..at - 4]);
        if checksum != calculated {
            return Err(png_error(
                "PngCrcMismatch",
                start,
                format!("{checksum:08x}"),
                format!("{calculated:08x}"),
            ));
        }
        if header.is_none() && &tag != b"IHDR" {
            return Err(png_error(
                "PngChunkOrder",
                start,
                "IHDR first",
                fo4_core::HexBytes(&tag),
            ));
        }
        if seen_data && &tag != b"IDAT" {
            data_closed = true;
        }
        match &tag {
            b"IHDR" => {
                if header.is_some() || data.len() != 13 {
                    return Err(png_error(
                        "PngHeader",
                        start,
                        "one 13-byte IHDR",
                        data.len(),
                    ));
                }
                let w = u32::from_be_bytes(
                    data[..4]
                        .try_into()
                        .map_err(|e| png_error("PngHeader", start, "width", e))?,
                );
                let h = u32::from_be_bytes(
                    data[4..8]
                        .try_into()
                        .map_err(|e| png_error("PngHeader", start, "height", e))?,
                );
                if w == 0
                    || h == 0
                    || data[8] != 8
                    || ![0, 2, 3, 4, 6].contains(&data[9])
                    || data[10..13] != [0, 0, 0]
                {
                    return Err(png_error(
                        "UnsupportedPngHeader",
                        start,
                        "noninterlaced 8-bit PNG",
                        fo4_core::HexBytes(data),
                    ));
                }
                let pixels = fo4_core::mul(w as u64, h as u64, "PNG", start as u64)?;
                fo4_core::limit(pixels, l.max_entries, "PNG pixels", start as u64)?;
                header = Some((w, h, data[9]));
            }
            b"PLTE" => {
                if seen_data
                    || palette.is_some()
                    || data.is_empty()
                    || data.len() % 3 != 0
                    || data.len() > 768
                {
                    return Err(png_error(
                        "PngPalette",
                        start,
                        "one bounded palette before IDAT",
                        data.len(),
                    ));
                }
                let mut v = Vec::new();
                scratch.reserve(&mut v, data.len(), "PNG palette", start as u64)?;
                v.extend_from_slice(data);
                palette = Some(v);
            }
            b"IDAT" => {
                if data_closed {
                    return Err(png_error(
                        "PngChunkOrder",
                        start,
                        "consecutive IDAT",
                        "interrupted",
                    ));
                }
                seen_data = true;
                let n = fo4_core::add(
                    packed.len() as u64,
                    data.len() as u64,
                    "PNG IDAT",
                    start as u64,
                )?;
                fo4_core::limit(n, l.max_payload, "PNG IDAT", start as u64)?;
                scratch.reserve(&mut packed, data.len(), "PNG IDAT", start as u64)?;
                packed.extend_from_slice(data);
            }
            b"IEND" => {
                if !seen_data || !data.is_empty() || at != input.len() {
                    return Err(png_error(
                        "PngEnd",
                        start,
                        "final empty IEND after IDAT",
                        input.len() - at,
                    ));
                }
                ended = true;
                break;
            }
            _ => {
                if tag[0] & 32 == 0 {
                    return Err(png_error(
                        "UnsupportedPngCriticalChunk",
                        start,
                        "known critical chunk",
                        fo4_core::HexBytes(&tag),
                    ));
                }
                if &tag == b"tRNS" {
                    if seen_data || transparency.is_some() {
                        return Err(png_error(
                            "PngTransparency",
                            start,
                            "one tRNS before IDAT",
                            "duplicate/late",
                        ));
                    }
                    let mut v = Vec::new();
                    scratch.reserve(&mut v, data.len(), "PNG tRNS", start as u64)?;
                    v.extend_from_slice(data);
                    transparency = Some(v);
                }
                let mut raw = Vec::new();
                b.reserve(&mut raw, data.len(), "PNG ancillary", start as u64)?;
                raw.extend_from_slice(data);
                b.reserve(&mut ancillary, 1, "PNG ancillary", start as u64)?;
                ancillary.push((tag, raw));
            }
        }
    }
    if !ended {
        return Err(png_error("TruncatedPng", at, "IEND", "EOF"));
    }
    let (w, h, kind) = header.ok_or_else(|| png_error("PngHeader", at, "IHDR", "absent"))?;
    let channels = match kind {
        0 | 3 => 1usize,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    if kind == 3 && palette.is_none() {
        return Err(png_error(
            "PngPalette",
            at,
            "palette for indexed image",
            "absent",
        ));
    }
    if let Some(t) = &transparency {
        let valid = match kind {
            0 => t.len() == 2,
            2 => t.len() == 6,
            3 => palette.as_ref().is_some_and(|p| t.len() <= p.len() / 3),
            _ => false,
        };
        if !valid {
            return Err(png_error(
                "PngTransparency",
                at,
                "valid tRNS for color type",
                t.len(),
            ));
        }
    }
    let stride = (w as usize)
        .checked_mul(channels)
        .ok_or_else(|| png_error("Overflow", at, "row size", w))?;
    let declared = fo4_core::mul(
        fo4_core::add(stride as u64, 1, "PNG", 0)?,
        h as u64,
        "PNG",
        0,
    )?;
    let filtered = fo4_core::zlib(&packed, declared, l, &scratch, "PNG IDAT", 0)?;
    let len = fo4_core::mul(stride as u64, h as u64, "PNG", 0)?;
    let mut raw = scratch.bytes(len, "PNG scanlines", 0)?;
    for y in 0..h as usize {
        let filter = filtered[y * (stride + 1)];
        if filter > 4 {
            return Err(png_error(
                "UnsupportedPngFilter",
                y * (stride + 1),
                "0..4",
                filter,
            ));
        }
        for x in 0..stride {
            let a = if x >= channels {
                raw[y * stride + x - channels]
            } else {
                0
            };
            let above = if y > 0 { raw[(y - 1) * stride + x] } else { 0 };
            let c = if y > 0 && x >= channels {
                raw[(y - 1) * stride + x - channels]
            } else {
                0
            };
            let predicted = match filter {
                0 => 0,
                1 => a,
                2 => above,
                3 => ((a as u16 + above as u16) / 2) as u8,
                _ => {
                    let p = a as i32 + above as i32 - c as i32;
                    let da = (p - a as i32).abs();
                    let db = (p - above as i32).abs();
                    let dc = (p - c as i32).abs();
                    if da <= db && da <= dc {
                        a
                    } else if db <= dc {
                        above
                    } else {
                        c
                    }
                }
            };
            raw[y * stride + x] = filtered[y * (stride + 1) + 1 + x].wrapping_add(predicted);
        }
    }
    let count = fo4_core::mul(w as u64, h as u64, "PNG", 0)?;
    let mut rgba = Vec::new();
    b.reserve(
        &mut rgba,
        usize::try_from(fo4_core::mul(count, 4, "PNG", 0)?)
            .map_err(|e| png_error("Overflow", 0, "RGBA size", e))?,
        "PNG RGBA",
        0,
    )?;
    for v in raw.chunks_exact(channels) {
        let p = match kind {
            0 => {
                let alpha = if transparency.as_deref() == Some(&[0, v[0]][..]) {
                    0
                } else {
                    255
                };
                [v[0], v[0], v[0], alpha]
            }
            2 => {
                let transparent = transparency
                    .as_deref()
                    .is_some_and(|t| t == [0, v[0], 0, v[1], 0, v[2]]);
                [v[0], v[1], v[2], if transparent { 0 } else { 255 }]
            }
            3 => {
                let table = palette
                    .as_ref()
                    .ok_or_else(|| png_error("PngPalette", 0, "palette", "absent"))?;
                let i = v[0] as usize;
                let c = table
                    .get(i * 3..i * 3 + 3)
                    .ok_or_else(|| png_error("PngPaletteIndex", 0, table.len() / 3, i))?;
                [
                    c[0],
                    c[1],
                    c[2],
                    transparency
                        .as_ref()
                        .and_then(|t| t.get(i))
                        .copied()
                        .unwrap_or(255),
                ]
            }
            4 => [v[0], v[0], v[0], v[1]],
            _ => [v[0], v[1], v[2], v[3]],
        };
        rgba.extend_from_slice(&p);
    }
    Ok(Png {
        size: [w, h],
        rgba,
        ancillary_chunks: ancillary,
    })
}
/// IEC sRGB forward transfer, applied only as a separately labelled output variant.
pub fn srgb_encode(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}
pub fn encode_srgb(size: [u32; 2], pixels: &[[f32; 4]], b: &Budget) -> Result<Vec<u8>> {
    encode_with_transfer(size, pixels, true, b)
}

#[cfg(test)]
mod stage16_tests {
    use super::*;
    fn image(w: u32, h: u32, kind: u8, rows: &[u8]) -> Vec<u8> {
        let b = Budget::new(8 << 20);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = [0u8; 13];
        header[..4].copy_from_slice(&w.to_be_bytes());
        header[4..8].copy_from_slice(&h.to_be_bytes());
        header[8] = 8;
        header[9] = kind;
        chunk(&mut out, b"IHDR", &header, &b).unwrap();
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(rows).unwrap();
        chunk(&mut out, b"IDAT", &z.finish().unwrap(), &b).unwrap();
        chunk(&mut out, b"IEND", &[], &b).unwrap();
        out
    }
    #[test]
    fn all_five_filters_have_independent_known_scanlines_and_every_prefix_fails() {
        let cases = [
            [0, 10, 15, 20, 0, 20, 25, 30],
            [1, 10, 5, 5, 1, 20, 5, 5],
            [2, 10, 15, 20, 2, 10, 10, 10],
            [3, 10, 10, 13, 3, 15, 8, 8],
            [4, 10, 5, 5, 4, 10, 5, 5],
        ];
        for row in cases {
            let bytes = image(3, 2, 0, &row);
            let l = fo4_core::Limits::default();
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end], &l, &Budget::new(8 << 20)).is_err());
            }
            let p = decode(&bytes, &l, &Budget::new(8 << 20)).unwrap();
            assert_eq!(
                p.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| p[0])
                    .collect::<Vec<_>>(),
                [10, 15, 20, 20, 25, 30]
            );
            let mut bad = bytes.clone();
            bad[40] ^= 1;
            assert_eq!(
                decode(&bad, &l, &Budget::new(8 << 20)).unwrap_err().code,
                "PngCrcMismatch"
            );
        }
    }
    #[test]
    fn png_rejects_overlong_short_output_bad_dimensions_and_budget() {
        let l = fo4_core::Limits::default();
        for bytes in [
            image(0, 2, 0, &[0; 4]),
            image(u32::MAX, u32::MAX, 0, &[]),
            image(2, 2, 0, &[0; 5]),
            image(2, 2, 0, &[0; 7]),
            image(2, 2, 1, &[0; 6]),
        ] {
            assert!(decode(&bytes, &l, &Budget::new(8 << 20)).is_err());
        }
        assert!(decode(&image(1, 1, 2, &[0, 1, 2, 3]), &l, &Budget::new(0)).is_err());
        let rgb = decode(&image(1, 1, 2, &[0, 1, 2, 3]), &l, &Budget::new(8 << 20)).unwrap();
        assert_eq!(rgb.rgba, [1, 2, 3, 255]);
        let rgba = decode(&image(1, 1, 6, &[0, 1, 2, 3, 4]), &l, &Budget::new(8 << 20)).unwrap();
        assert_eq!(rgba.rgba, [1, 2, 3, 4]);
        let encoded =
            encode_srgb([1, 1], &[[0.18, 0.18, 0.18, 1.]], &Budget::new(8 << 20)).unwrap();
        let decoded = decode(&encoded, &l, &Budget::new(8 << 20)).unwrap();
        assert_eq!(decoded.rgba, [118, 118, 118, 255]);
        assert!(decoded
            .ancillary_chunks
            .iter()
            .any(|(tag, value)| tag == b"sRGB" && value == &[0]));
        assert!((srgb_encode(0.18) - 0.46135613).abs() < 1e-6);
        assert_eq!(srgb_encode(0.), 0.);
        assert!((srgb_encode(1.) - 1.).abs() < 1e-6);
    }
}
