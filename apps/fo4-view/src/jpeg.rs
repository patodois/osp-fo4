//! A baseline JPEG encoder for the play window's periodic frames
//! (`--play-shots`): ITU-T T.81 sequential DCT with Huffman coding, eight
//! bits a sample, in a JFIF file. Colour is converted to YCbCr as JFIF
//! defines it, the two chroma components are averaged over 2 by 2 pixels,
//! the quantization tables are the examples of T.81 Annex K.1 scaled by a
//! quality from 1 to 100 (the scaling the IJG library uses: 5000 / q below
//! 50, 200 - 2 q from 50) and the Huffman tables are the examples of
//! Annex K.3. Presentation only: a frame for a person to look at, never
//! compared byte for byte (the CPU frame and `--headless` stay PNG).

/// The natural (row-major) index of each coefficient in zigzag order.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// T.81 Table K.1 (luminance), natural order.
const LUMA_Q: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

/// T.81 Table K.2 (chrominance), natural order.
const CHROMA_Q: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// T.81 Table K.3 to K.6: the number of codes of each length 1 to 16 and
/// the symbols in code order.
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALUES: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALUES: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// The quality the play window's frames are written at.
pub const SHOT_QUALITY: u8 = 80;

/// A Huffman table as codes and lengths by symbol (T.81 Annex C).
struct Huffman {
    code: [u16; 256],
    size: [u8; 256],
}

impl Huffman {
    fn new(bits: &[u8; 16], values: &[u8]) -> Huffman {
        let mut h = Huffman {
            code: [0; 256],
            size: [0; 256],
        };
        let (mut code, mut k) = (0u16, 0usize);
        for (len, n) in bits.iter().enumerate() {
            for _ in 0..*n {
                let v = usize::from(values[k]);
                h.code[v] = code;
                h.size[v] = len as u8 + 1;
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        h
    }
}

/// The entropy-coded segment's bits, with a zero byte stuffed after each
/// 0xFF.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u16, size: u8) {
        if size == 0 {
            return;
        }
        let size = u32::from(size);
        self.acc = (self.acc << size) | (u32::from(value) & ((1 << size) - 1));
        self.n += size;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xff {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }

    /// Pads the last byte with one bits.
    fn finish(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad as u8);
        }
    }
}

/// The quantization table `base` at `quality`.
fn scaled(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((u32::from(b) * scale + 50) / 100).clamp(1, 255) as u16)
}

/// The forward DCT of a level-shifted block (T.81 A.3.3), natural order.
fn dct(block: &[f32; 64], cos: &[[f32; 8]; 8]) -> [f32; 64] {
    let mut rows = [0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut s = 0.0;
            for x in 0..8 {
                s += block[y * 8 + x] * cos[u][x];
            }
            rows[y * 8 + u] = s;
        }
    }
    let mut out = [0f32; 64];
    for v in 0..8 {
        for u in 0..8 {
            let mut s = 0.0;
            for y in 0..8 {
                s += rows[y * 8 + u] * cos[v][y];
            }
            out[v * 8 + u] = s / 4.0;
        }
    }
    out
}

/// The magnitude category of `v` and its additional bits (T.81 F.1.2.1).
fn category(v: i32) -> (u8, u16) {
    let a = v.unsigned_abs();
    let size = (32 - a.leading_zeros()) as u8;
    let bits = if v < 0 { v - 1 } else { v };
    (size, (bits as u32 & ((1u32 << size) - 1)) as u16)
}

/// One block quantized and Huffman coded after the previous DC `dc`.
fn encode_block(
    bits: &mut Bits,
    block: &[f32; 64],
    q: &[u16; 64],
    dc: &mut i32,
    tables: (&Huffman, &Huffman),
    cos: &[[f32; 8]; 8],
) {
    let f = dct(block, cos);
    let mut z = [0i32; 64];
    for (k, n) in ZIGZAG.iter().enumerate() {
        z[k] = (f[*n] / f32::from(q[*n])).round() as i32;
    }
    let (dc_t, ac_t) = tables;
    let (size, extra) = category(z[0] - *dc);
    *dc = z[0];
    bits.put(dc_t.code[usize::from(size)], dc_t.size[usize::from(size)]);
    bits.put(extra, size);
    let mut run = 0usize;
    for v in z.iter().skip(1) {
        if *v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            bits.put(ac_t.code[0xf0], ac_t.size[0xf0]);
            run -= 16;
        }
        let (size, extra) = category(*v);
        let symbol = (run << 4) | usize::from(size);
        bits.put(ac_t.code[symbol], ac_t.size[symbol]);
        bits.put(extra, size);
        run = 0;
    }
    if run > 0 {
        bits.put(ac_t.code[0], ac_t.size[0]);
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
}

/// A JFIF file of `size` RGBA8 pixels (alpha dropped) at `quality` (1 to
/// 100).
pub fn encode_rgba8(size: [u32; 2], rgba: &[u8], quality: u8) -> Result<Vec<u8>, String> {
    let [w, h] = size;
    if w == 0 || h == 0 || w > 65_535 || h > 65_535 {
        return Err(format!("JpegSize: {w}x{h} (1 to 65535 a side)"));
    }
    let (wu, hu) = (w as usize, h as usize);
    if rgba.len() != wu * hu * 4 {
        return Err(format!(
            "JpegPixels: {} bytes for {w}x{h} RGBA8",
            rgba.len()
        ));
    }
    let (lq, cq) = (scaled(&LUMA_Q, quality), scaled(&CHROMA_Q, quality));
    let mut out = Vec::with_capacity(wu * hu / 4 + 1024);
    out.extend_from_slice(&[0xff, 0xd8]);
    segment(
        &mut out,
        0xe0,
        &[b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0],
    );
    let mut dqt = Vec::with_capacity(130);
    for (id, t) in [(0u8, &lq), (1, &cq)] {
        dqt.push(id);
        dqt.extend(ZIGZAG.iter().map(|n| t[*n] as u8));
    }
    segment(&mut out, 0xdb, &dqt);
    let mut sof = vec![8];
    sof.extend_from_slice(&(h as u16).to_be_bytes());
    sof.extend_from_slice(&(w as u16).to_be_bytes());
    sof.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    segment(&mut out, 0xc0, &sof);
    let mut dht = Vec::new();
    for (class, bits, values) in [
        (0x00u8, &DC_LUMA_BITS, &DC_VALUES[..]),
        (0x10, &AC_LUMA_BITS, &AC_LUMA_VALUES[..]),
        (0x01, &DC_CHROMA_BITS, &DC_VALUES[..]),
        (0x11, &AC_CHROMA_BITS, &AC_CHROMA_VALUES[..]),
    ] {
        dht.push(class);
        dht.extend_from_slice(bits);
        dht.extend_from_slice(values);
    }
    segment(&mut out, 0xc4, &dht);
    segment(&mut out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    let tables = (
        Huffman::new(&DC_LUMA_BITS, &DC_VALUES),
        Huffman::new(&AC_LUMA_BITS, &AC_LUMA_VALUES),
        Huffman::new(&DC_CHROMA_BITS, &DC_VALUES),
        Huffman::new(&AC_CHROMA_BITS, &AC_CHROMA_VALUES),
    );
    let mut cos = [[0f32; 8]; 8];
    for (u, row) in cos.iter_mut().enumerate() {
        let c = if u == 0 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        for (x, v) in row.iter_mut().enumerate() {
            *v = c * ((2 * x + 1) as f32 * u as f32 * std::f32::consts::PI / 16.0).cos();
        }
    }
    // Each pixel's YCbCr (JFIF), edges repeated past the frame.
    let pixel = |x: usize, y: usize| -> [f32; 3] {
        let i = (y.min(hu - 1) * wu + x.min(wu - 1)) * 4;
        let (r, g, b) = (
            f32::from(rgba[i]),
            f32::from(rgba[i + 1]),
            f32::from(rgba[i + 2]),
        );
        [
            0.299 * r + 0.587 * g + 0.114 * b,
            -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0,
            0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0,
        ]
    };
    let mut bits = Bits { out, acc: 0, n: 0 };
    let mut dc = [0i32; 3];
    for my in (0..hu).step_by(16) {
        for mx in (0..wu).step_by(16) {
            let mut ys = [[0f32; 64]; 4];
            let (mut cb, mut cr) = ([0f32; 64], [0f32; 64]);
            for y in 0..16 {
                for x in 0..16 {
                    let p = pixel(mx + x, my + y);
                    ys[(y / 8) * 2 + x / 8][(y % 8) * 8 + x % 8] = p[0] - 128.0;
                    let c = (y / 2) * 8 + x / 2;
                    cb[c] += (p[1] - 128.0) / 4.0;
                    cr[c] += (p[2] - 128.0) / 4.0;
                }
            }
            for b in &ys {
                encode_block(&mut bits, b, &lq, &mut dc[0], (&tables.0, &tables.1), &cos);
            }
            encode_block(
                &mut bits,
                &cb,
                &cq,
                &mut dc[1],
                (&tables.2, &tables.3),
                &cos,
            );
            encode_block(
                &mut bits,
                &cr,
                &cq,
                &mut dc[2],
                (&tables.2, &tables.3),
                &cos,
            );
        }
    }
    bits.finish();
    let mut out = bits.out;
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_tables_hold_their_counts() {
        let n = |b: &[u8; 16]| b.iter().map(|v| usize::from(*v)).sum::<usize>();
        assert_eq!(n(&DC_LUMA_BITS), DC_VALUES.len());
        assert_eq!(n(&DC_CHROMA_BITS), DC_VALUES.len());
        assert_eq!(n(&AC_LUMA_BITS), AC_LUMA_VALUES.len());
        assert_eq!(n(&AC_CHROMA_BITS), AC_CHROMA_VALUES.len());
        let mut seen = [false; 64];
        for z in ZIGZAG {
            assert!(!seen[z]);
            seen[z] = true;
        }
        // Quality 50 is the example tables themselves.
        assert_eq!(scaled(&LUMA_Q, 50), LUMA_Q);
        assert!(scaled(&LUMA_Q, 100).iter().all(|v| *v == 1));
    }

    #[test]
    fn categories_and_their_bits_follow_the_standard() {
        assert_eq!(category(0), (0, 0));
        assert_eq!(category(1), (1, 1));
        assert_eq!(category(-1), (1, 0));
        assert_eq!(category(-3), (2, 0));
        assert_eq!(category(5), (3, 5));
        assert_eq!(category(-5), (3, 2));
        assert_eq!(category(1023), (10, 1023));
    }

    #[test]
    fn a_frame_is_a_jfif_file_with_its_size() {
        let (w, h) = (37u32, 21u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [(x * 7) as u8, (y * 11) as u8, ((x + y) * 3) as u8, 255]
            })
            .collect();
        let j = encode_rgba8([w, h], &rgba, SHOT_QUALITY).unwrap();
        assert_eq!(&j[..4], &[0xff, 0xd8, 0xff, 0xe0]);
        assert_eq!(&j[6..11], b"JFIF\0");
        assert_eq!(&j[j.len() - 2..], &[0xff, 0xd9]);
        let sof = j.windows(2).position(|m| m == [0xff, 0xc0]).unwrap();
        assert_eq!(u16::from_be_bytes([j[sof + 5], j[sof + 6]]), h as u16);
        assert_eq!(u16::from_be_bytes([j[sof + 7], j[sof + 8]]), w as u16);
        // In the entropy-coded data every 0xFF is followed by a stuffed 0.
        let sos = j.windows(2).position(|m| m == [0xff, 0xda]).unwrap();
        let data = &j[sos + 14..j.len() - 2];
        for (i, b) in data.iter().enumerate() {
            if *b == 0xff {
                assert_eq!(data.get(i + 1), Some(&0));
            }
        }
        assert!(encode_rgba8([0, 4], &[], 80).is_err());
        assert!(encode_rgba8([2, 2], &[0; 15], 80).is_err());
    }
}
