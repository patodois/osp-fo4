#![forbid(unsafe_code)]
//! Own DDS framing and bounded subresource decoder. Format facts: spec/dds.md.
use fo4_core::{add, limit, mul, range, Budget, Cursor, Error, Limits, Result, SliceSource};
use serde::Serialize;
use std::ops::Range;
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc1Alpha,
    Bc2,
    Bc3,
    Bc4,
    Bc4Snorm,
    Bc5,
    Bc5Snorm,
    Bc6hUf16,
    Bc6hSf16,
    Bc7,
    Rgba8,
    Bgra8,
    Bgrx8,
    R8,
    Rg8,
    Rgb8,
    Bgr8,
    Rgba16Float,
    Rgba32Float,
}
impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bc1 => "BC1",
            Self::Bc1Alpha => "BC1Alpha",
            Self::Bc2 => "BC2",
            Self::Bc3 => "BC3",
            Self::Bc4 => "BC4",
            Self::Bc4Snorm => "BC4_SNORM",
            Self::Bc5 => "BC5",
            Self::Bc5Snorm => "BC5_SNORM",
            Self::Bc6hUf16 => "BC6H_UF16",
            Self::Bc6hSf16 => "BC6H_SF16",
            Self::Bc7 => "BC7",
            Self::Rgba8 => "RGBA8",
            Self::Bgra8 => "BGRA8",
            Self::Bgrx8 => "BGRX8",
            Self::R8 => "R8",
            Self::Rg8 => "RG8",
            Self::Rgb8 => "RGB8",
            Self::Bgr8 => "BGR8",
            Self::Rgba16Float => "RGBA16_FLOAT",
            Self::Rgba32Float => "RGBA32_FLOAT",
        }
    }
    fn bcn(self) -> Option<bcn_decode::Format> {
        use bcn_decode::Format as F;
        Some(match self {
            Self::Bc1 => F::Bc1,
            Self::Bc1Alpha => F::Bc1Alpha,
            Self::Bc2 => F::Bc2,
            Self::Bc3 => F::Bc3,
            Self::Bc4 => F::Bc4,
            Self::Bc4Snorm => F::Bc4Snorm,
            Self::Bc5 => F::Bc5,
            Self::Bc5Snorm => F::Bc5Snorm,
            Self::Bc6hUf16 => F::Bc6hUf16,
            Self::Bc6hSf16 => F::Bc6hSf16,
            Self::Bc7 => F::Bc7,
            _ => return None,
        })
    }
    fn unit(self) -> u64 {
        match self {
            Self::Bc1 | Self::Bc1Alpha | Self::Bc4 | Self::Bc4Snorm => 8,
            Self::Bc2
            | Self::Bc3
            | Self::Bc5
            | Self::Bc5Snorm
            | Self::Bc6hUf16
            | Self::Bc6hSf16
            | Self::Bc7 => 16,
            Self::R8 => 1,
            Self::Rg8 => 2,
            Self::Rgb8 | Self::Bgr8 => 3,
            Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
            _ => 4,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Surface {
    pub array: u32,
    pub face: u8,
    pub mip: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub data: Range<usize>,
}
#[derive(Debug, Serialize)]
pub struct Texture<'a> {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mipmaps: u32,
    pub array_size: u32,
    pub cubemap: bool,
    pub format: Format,
    pub srgb: bool,
    pub uninterpreted_header_fields: u64,
    pub header: [u32; 31],
    pub dx10: Option<[u32; 5]>,
    pub surfaces: Vec<Surface>,
    #[serde(skip)]
    bytes: &'a [u8],
    file: String,
}
impl<'a> Texture<'a> {
    pub fn read(bytes: &'a [u8], file: &str, l: &Limits, b: &Budget) -> Result<Self> {
        l.validate(file)?;
        limit(bytes.len() as u64, l.max_payload, file, 0)?;
        let source = SliceSource { name: file, bytes };
        let mut c = Cursor::new(&source);
        if c.take::<4>()? != *b"DDS " {
            return Err(Error::new("InvalidDdsMagic", file, 0, "DDS ", "other"));
        }
        let mut header = [0; 31];
        for v in &mut header {
            *v = c.u32()?;
        }
        if header[0] != 124 || header[18] != 32 {
            return Err(Error::new(
                "InvalidDdsHeaderSize",
                file,
                4,
                "DDS_HEADER=124; DDS_PIXELFORMAT=32",
                format!("{}/{}", header[0], header[18]),
            ));
        }
        let height = header[2];
        let width = header[3];
        if width == 0 || height == 0 {
            return Err(Error::new(
                "InvalidTextureDimensions",
                file,
                12,
                "nonzero width/height",
                format!("{width}x{height}"),
            ));
        }
        let mipmaps = if header[6] == 0 { 1 } else { header[6] };
        let four = header[20].to_le_bytes();
        let mut dx10 = None;
        let mut array_size = 1;
        let mut depth = if header[27] & 0x200000 != 0 {
            header[5]
        } else {
            1
        };
        let mut srgb = false;
        let mut faces: Vec<u8> = Vec::new();
        b.reserve(&mut faces, 6, file, 112)?;
        let format = if header[19] & 4 != 0 && four == *b"DX10" {
            let mut d = [0; 5];
            for v in &mut d {
                *v = c.u32()?;
            }
            if !matches!(d[1], 2..=4) {
                return Err(Error::new(
                    "UnsupportedDdsDimension",
                    file,
                    132,
                    "1D, 2D or 3D (2..4)",
                    d[1],
                ));
            }
            if d[1] == 2 && height != 1 {
                return Err(Error::new(
                    "InvalidTextureDimensions",
                    file,
                    12,
                    "height 1 for 1D",
                    height,
                ));
            }
            if d[2] & !4 != 0 || d[4] & !7 != 0 {
                return Err(Error::new(
                    "UnsupportedDdsMiscFlags",
                    file,
                    136,
                    "cube flag and documented alpha mode bits",
                    format!("{}/{}", d[2], d[4]),
                ));
            }
            array_size = d[3];
            if array_size == 0 {
                return Err(Error::new("InvalidDdsArraySize", file, 140, ">0", 0));
            }
            limit(array_size as u64, l.max_entries, file, 140)?;
            if d[1] == 4 {
                if array_size != 1 || d[2] & 4 != 0 {
                    return Err(Error::new(
                        "InvalidDdsVolume",
                        file,
                        132,
                        "3D array size 1, no cube",
                        array_size,
                    ));
                }
                depth = header[5];
            } else {
                depth = 1;
            }
            if d[2] & 4 != 0 {
                if width != height {
                    return Err(Error::new(
                        "InvalidDdsCube",
                        file,
                        12,
                        "square faces",
                        format!("{width}/{height}"),
                    ));
                }
                faces.extend(0..6);
            } else {
                faces.push(0);
            }
            srgb = matches!(d[0], 29 | 72 | 75 | 78 | 91 | 93 | 99);
            let f = match d[0] {
                71 | 72 => Format::Bc1Alpha,
                74 | 75 => Format::Bc2,
                77 | 78 => Format::Bc3,
                80 => Format::Bc4,
                81 => Format::Bc4Snorm,
                83 => Format::Bc5,
                84 => Format::Bc5Snorm,
                95 => Format::Bc6hUf16,
                96 => Format::Bc6hSf16,
                98 | 99 => Format::Bc7,
                28 | 29 => Format::Rgba8,
                87 | 91 => Format::Bgra8,
                88 | 93 => Format::Bgrx8,
                61 => Format::R8,
                49 => Format::Rg8,
                10 => Format::Rgba16Float,
                2 => Format::Rgba32Float,
                _ => {
                    return Err(Error::new(
                        "UnsupportedDxgiFormat",
                        file,
                        128,
                        "documented supported typed format",
                        d[0],
                    ))
                }
            };
            dx10 = Some(d);
            f
        } else {
            if header[27] & 0x200 != 0 {
                if width != height || depth != 1 {
                    return Err(Error::new(
                        "InvalidDdsCube",
                        file,
                        12,
                        "square 2D faces",
                        format!("{width}/{height}/{depth}"),
                    ));
                }
                for face in 0..6 {
                    if header[27] & (0x400 << face) != 0 {
                        faces.push(face);
                    }
                }
                if faces.is_empty() {
                    return Err(Error::new(
                        "InvalidDdsCube",
                        file,
                        112,
                        "at least one face",
                        0,
                    ));
                }
            } else {
                faces.push(0);
            }
            if header[19] & 4 != 0 {
                match &four {
                    b"DXT1" => {
                        if header[19] & 1 != 0 {
                            Format::Bc1Alpha
                        } else {
                            Format::Bc1
                        }
                    }
                    b"DXT3" => Format::Bc2,
                    b"DXT5" => Format::Bc3,
                    b"ATI1" | b"BC4U" => Format::Bc4,
                    b"BC4S" => Format::Bc4Snorm,
                    b"ATI2" | b"BC5U" => Format::Bc5,
                    b"BC5S" => Format::Bc5Snorm,
                    _ => {
                        return Err(Error::new(
                            "UnsupportedDdsFourcc",
                            file,
                            84,
                            "DXT1/3/5, ATI1/2, BC4/5, DX10",
                            format!("{four:?}"),
                        ))
                    }
                }
            } else {
                match (header[21], header[22], header[23], header[24], header[25]) {
                    (32, 255, 65280, 16711680, 4278190080) => Format::Rgba8,
                    (32, 16711680, 65280, 255, 4278190080) => Format::Bgra8,
                    (32, 16711680, 65280, 255, 0) => Format::Bgrx8,
                    (24, 255, 65280, 16711680, 0) => Format::Rgb8,
                    (24, 16711680, 65280, 255, 0) => Format::Bgr8,
                    (8, 255, 0, 0, 0) => Format::R8,
                    (16, 255, 65280, 0, 0) => Format::Rg8,
                    _ => {
                        return Err(Error::new(
                            "UnsupportedDdsPixelMasks",
                            file,
                            88,
                            "documented byte-aligned masks",
                            format!("{:?}", &header[21..26]),
                        ))
                    }
                }
            }
        };
        if depth == 0 {
            return Err(Error::new(
                "InvalidTextureDimensions",
                file,
                24,
                "nonzero volume depth",
                depth,
            ));
        }
        let max_mips = 32 - width.max(height).max(depth).leading_zeros();
        limit(mipmaps as u64, max_mips as u64, file, 28)?;
        if format.bcn().is_none()
            && header[1] & 8 != 0
            && header[4] as u64 != mul(width as u64, format.unit(), file, 20)?
        {
            return Err(Error::new(
                "UnsupportedDdsPitch",
                file,
                20,
                "tightly packed uncompressed rows",
                header[4],
            ));
        }
        let cubemap =
            faces.len() > 1 || header[27] & 0x200 != 0 || dx10.is_some_and(|d| d[2] & 4 != 0);
        let count = mul(
            mul(array_size as u64, faces.len() as u64, file, 140)?,
            mipmaps as u64,
            file,
            28,
        )?;
        limit(count, l.max_entries, file, 28)?;
        let mut surfaces = Vec::new();
        b.reserve(&mut surfaces, count as usize, file, 28)?;
        for array in 0..array_size {
            for face in &faces {
                for mip in 0..mipmaps {
                    let w = (width >> mip).max(1);
                    let h = (height >> mip).max(1);
                    let d = (depth >> mip).max(1);
                    let size = if format.bcn().is_some() {
                        mul(
                            mul((w as u64).div_ceil(4), (h as u64).div_ceil(4), file, c.pos)?,
                            format.unit(),
                            file,
                            c.pos,
                        )?
                    } else {
                        mul(
                            mul(w as u64, h as u64, file, c.pos)?,
                            format.unit(),
                            file,
                            c.pos,
                        )?
                    };
                    let size = mul(size, d as u64, file, c.pos)?;
                    range(c.pos, size, bytes.len() as u64, file)?;
                    let start = c.pos;
                    c.pos = add(c.pos, size, file, c.pos)?;
                    surfaces.push(Surface {
                        array,
                        face: *face,
                        mip,
                        width: w,
                        height: h,
                        depth: d,
                        data: start as usize..c.pos as usize,
                    });
                }
            }
        }
        if c.pos != c.end {
            return Err(Error::new(
                "TrailingDdsData",
                file,
                c.pos,
                "exact subresource bytes",
                c.end - c.pos,
            ));
        }
        b.charge(file.len() as u64, file, 0)?;
        let mut retained_file = String::new();
        retained_file.try_reserve_exact(file.len()).map_err(|e| {
            Error::new("AllocationFailed", file, 0, "DDS source name allocation", e)
        })?;
        retained_file.push_str(file);
        let uninterpreted_header_fields = header[7..18].iter().filter(|v| **v != 0).count() as u64
            + header[28..31].iter().filter(|v| **v != 0).count() as u64;
        Ok(Self {
            uninterpreted_header_fields,
            width,
            height,
            depth,
            mipmaps,
            array_size,
            cubemap,
            format,
            srgb,
            header,
            dx10,
            surfaces,
            bytes,
            file: retained_file,
        })
    }
    /// The stored bytes of surface `index` as the file holds them.
    pub fn surface_bytes(&self, index: usize) -> Option<&'a [u8]> {
        self.surfaces
            .get(index)
            .and_then(|s| self.bytes.get(s.data.clone()))
    }
    /// Decode each surface independently; f32 preserves BC6H HDR and signed BC4/5.
    pub fn decode(&self, index: usize, l: &Limits, b: &Budget) -> Result<Vec<f32>> {
        let s = self.surfaces.get(index).ok_or_else(|| {
            Error::new(
                "InvalidSurfaceIndex",
                &self.file,
                0,
                self.surfaces.len(),
                index,
            )
        })?;
        let elements = mul(
            mul(
                mul(s.width as u64, s.height as u64, &self.file, 0)?,
                s.depth as u64,
                &self.file,
                0,
            )?,
            4,
            &self.file,
            0,
        )?;
        limit(
            mul(elements, 4, &self.file, 0)?,
            l.max_payload,
            &self.file,
            s.data.start as u64,
        )?;
        let mut out = Vec::new();
        b.reserve(&mut out, elements as usize, &self.file, s.data.start as u64)?;
        out.resize(elements as usize, 0.0);
        let source = &self.bytes[s.data.clone()];
        if let Some(f) = self.format.bcn() {
            let plane = mul(
                mul(s.width as u64, s.height as u64, &self.file, 0)?,
                4,
                &self.file,
                0,
            )? as usize;
            let compressed = source.len() / s.depth as usize;
            for (input, output) in source
                .chunks_exact(compressed)
                .zip(out.chunks_exact_mut(plane))
            {
                bcn_decode::decode_rgba_f32(f, input, s.width as usize, s.height as usize, output)
                    .map_err(|e| {
                        Error::new(
                            "BcnDecode",
                            &self.file,
                            s.data.start as u64,
                            "valid BC surface",
                            e,
                        )
                    })?;
            }
        } else {
            decode_plain(
                self.format,
                source,
                &mut out,
                &self.file,
                s.data.start as u64,
            )?;
        }

        Ok(out)
    }
}
pub use fo4_core::half_to_f32;

impl Texture<'_> {
    /// Bounded strips, including every mip/face/depth slice; no full-image allocation.
    pub fn verify_decode(&self, l: &Limits, b: &Budget) -> Result<u64> {
        let mut total = 0;
        for s in &self.surfaces {
            let work = b.child();
            let rows = if self.format.bcn().is_some() {
                s.height.min(4)
            } else {
                1
            };
            let elements = mul(
                mul(s.width as u64, rows as u64, &self.file, 0)?,
                4,
                &self.file,
                0,
            )?;
            limit(
                mul(elements, 4, &self.file, 0)?,
                l.max_payload,
                &self.file,
                s.data.start as u64,
            )?;
            let mut out = Vec::new();
            work.reserve(&mut out, elements as usize, &self.file, s.data.start as u64)?;
            out.resize(elements as usize, 0.);
            let bytes_per_row = if self.format.bcn().is_some() {
                mul(
                    (s.width as u64).div_ceil(4),
                    self.format.unit(),
                    &self.file,
                    0,
                )?
            } else {
                mul(s.width as u64, self.format.unit(), &self.file, 0)?
            } as usize;
            let input = &self.bytes[s.data.clone()];
            let strip_rows = if self.format.bcn().is_some() {
                s.height.div_ceil(4)
            } else {
                s.height
            };
            for (index, row) in input.chunks_exact(bytes_per_row).enumerate() {
                let y = (index % (strip_rows as usize)) as u32;
                let height = rows.min(s.height - y * rows);
                let n = s.width as usize * height as usize * 4;
                if let Some(format) = self.format.bcn() {
                    bcn_decode::decode_rgba_f32(
                        format,
                        row,
                        s.width as usize,
                        height as usize,
                        &mut out[..n],
                    )
                    .map_err(|e| {
                        Error::new(
                            "BcnDecode",
                            &self.file,
                            s.data.start as u64,
                            "valid BC strip",
                            e,
                        )
                    })?;
                } else {
                    decode_plain(
                        self.format,
                        row,
                        &mut out[..n],
                        &self.file,
                        s.data.start as u64,
                    )?;
                }

                total = add(total, mul(n as u64, 4, &self.file, 0)?, &self.file, 0)?;
            }
        }
        Ok(total)
    }
}
#[cfg(test)]
mod tests;

fn decode_plain(
    format: Format,
    input: &[u8],
    out: &mut [f32],
    _file: &str,
    _offset: u64,
) -> Result<()> {
    let unit = format.unit() as usize;
    for (input, output) in input
        .chunks_exact(unit)
        .zip(out.as_chunks_mut::<4>().0.iter_mut())
    {
        output.copy_from_slice(&[0., 0., 0., 1.]);
        match format {
            Format::Rgba16Float => {
                for (i, v) in input.as_chunks::<2>().0.iter().enumerate() {
                    output[i] = half_to_f32(u16::from_le_bytes([v[0], v[1]]));
                }
            }
            Format::Rgba32Float => {
                for (i, v) in input.as_chunks::<4>().0.iter().enumerate() {
                    output[i] = f32::from_le_bytes(*v);
                }
            }
            Format::Bgra8 | Format::Bgrx8 | Format::Bgr8 => {
                output[0] = input[2] as f32 / 255.;
                output[1] = input[1] as f32 / 255.;
                output[2] = input[0] as f32 / 255.;
                output[3] = if matches!(format, Format::Bgrx8 | Format::Bgr8) {
                    1.
                } else {
                    input[3] as f32 / 255.
                };
            }
            _ => {
                for (i, v) in input.iter().enumerate() {
                    output[i] = *v as f32 / 255.;
                }
            }
        }
    }
    Ok(())
}
