//! The tracks of one block of an `hkaSplineCompressedAnimation` and their
//! values at a time (`spec/havok-animation.md`, Track data).
//!
//! Every byte rule below consumed every block of the 15,060 spline objects
//! of the measured installation exactly up to the block's float offset; the
//! value readings (quantised rotations, control-point scaling, curve
//! evaluation, absent components) are the measured ones of the spec, with
//! the evidence and its limits stated there. Nothing here panics on any
//! input.
use super::SplineAnimation;
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Quantisation code of the translation and scale fields of mask byte 0:
/// two bytes per control-point value. The only value stored.
pub const VECTOR_16: u8 = 1;
/// Rotation quantisation codes of mask byte 0 and their stored sizes.
pub const ROTATION_40: u8 = 1;
pub const ROTATION_48: u8 = 2;
const CODE: &str = "InvalidHavokSplineBlock";

/// One stored component of a translation or scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Component {
    /// Neither bit set: no value stored. Sampled as 0 for a translation and
    /// 1 for a scale (measured across block boundaries).
    Absent,
    Static(f32),
    /// Control points of this component are u16 values scaled into
    /// [min, max].
    Spline {
        min: f32,
        max: f32,
    },
}

/// A clamped curve: `degree`, knots in frames of the block, and its control
/// points (`knots.len() == points.len() + degree + 1`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Curve<T> {
    pub degree: u8,
    pub knots: Vec<u8>,
    pub points: Vec<T>,
}

/// Translation or scale of a track.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VectorTrack {
    /// Mask byte: bits 0 to 2 static x, y, z; bits 4 to 6 spline x, y, z.
    pub flags: u8,
    pub components: [Component; 3],
    /// When a component is a spline: one row per control point, holding the
    /// stored u16 of each spline component in x, y, z order (0 elsewhere).
    pub curve: Option<Curve<[u16; 3]>>,
}

/// Rotation of a track: absent (identity), one quaternion, or a curve of
/// quaternions; quaternions as (x, y, z, w).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RotationTrack {
    Absent,
    Static([f32; 4]),
    Spline(Curve<[f32; 4]>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    /// The four mask bytes: quantisation, translation, rotation, scale.
    pub mask: [u8; 4],
    pub translation: VectorTrack,
    pub rotation: RotationTrack,
    pub scale: VectorTrack,
}

/// A local transform sampled from a track: translation, unit quaternion
/// (x, y, z, w) and scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TrackPose {
    pub translation: [f64; 3],
    pub rotation: [f64; 4],
    pub scale: [f64; 3],
}

/// The stored parts of a quantised rotation: three components, the index of
/// the component they leave out and its sign.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Quantised {
    pub fields: [f64; 3],
    pub index: usize,
    pub negative: bool,
}
impl Quantised {
    /// A 5-byte value (40 bits, little-endian): three 12-bit fields at bits 0,
    /// 12 and 24 mapped to `(v - 2047) / 2047 / sqrt 2`, the left-out
    /// component's index at bits 36 and 37, its sign at bit 38.
    pub fn from_40(b: [u8; 5]) -> Self {
        let v = b
            .iter()
            .rev()
            .fold(0u64, |acc, x| (acc << 8) | u64::from(*x));
        let field = |shift: u32| ((v >> shift) & 0xfff) as f64;
        Self {
            fields: [0, 12, 24]
                .map(|s| (field(s) - 2047.0) / 2047.0 * std::f64::consts::FRAC_1_SQRT_2),
            index: ((v >> 36) & 3) as usize,
            negative: (v >> 38) & 1 == 1,
        }
    }
    /// A 6-byte value: three u16 whose low 15 bits map to
    /// `(v - 16383) / 16383 / sqrt 2`; the top bits of the first two give the
    /// left-out component's index (first word low), the third's its sign.
    pub fn from_48(b: [u8; 6]) -> Self {
        let w = [0, 2, 4].map(|k| u16::from_le_bytes([b[k], b[k + 1]]));
        Self {
            fields: w.map(|x| {
                (f64::from(x & 0x7fff) - 16383.0) / 16383.0 * std::f64::consts::FRAC_1_SQRT_2
            }),
            index: usize::from(w[0] >> 15) | (usize::from(w[1] >> 15) << 1),
            negative: w[2] >> 15 == 1,
        }
    }
    /// Sum of the squares of the three stored components; at most 1 when the
    /// left-out component is real.
    pub fn square_sum(&self) -> f64 {
        self.fields.iter().map(|x| x * x).sum()
    }
    /// The quaternion (x, y, z, w): the left-out component is the square root
    /// of what the others leave to 1 (0 if they exceed it), with its sign.
    pub fn quaternion(&self) -> [f32; 4] {
        let big = (1.0 - self.square_sum()).max(0.0).sqrt();
        let big = if self.negative { -big } else { big };
        let mut q = [0f32; 4];
        let mut k = 0;
        for (c, slot) in q.iter_mut().enumerate() {
            if c == self.index {
                *slot = big as f32;
            } else {
                *slot = self.fields[k] as f32;
                k += 1;
            }
        }
        q
    }
}

/// de Boor evaluation of a clamped curve at `u` (frames of the block),
/// clamped to the knot range.
pub fn de_boor<const N: usize>(
    knots: &[u8],
    points: &[[f64; N]],
    degree: usize,
    u: f64,
) -> [f64; N] {
    let n = points.len();
    if n == 0 {
        return [0.0; N];
    }
    if degree == 0 || n <= degree || knots.len() != n + degree + 1 {
        return points[0];
    }
    let lo = f64::from(knots[degree]);
    let hi = f64::from(knots[n]);
    let u = u.clamp(lo, hi);
    // Span k with knots[k] <= u < knots[k + 1], degree <= k < n.
    let mut k = degree;
    while k + 1 < n && f64::from(knots[k + 1]) <= u {
        k += 1;
    }
    let mut d: Vec<[f64; N]> = (0..=degree).map(|j| points[j + k - degree]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let den = f64::from(knots[i + degree + 1 - r]) - f64::from(knots[i]);
            let a = if den == 0.0 {
                0.0
            } else {
                (u - f64::from(knots[i])) / den
            };
            let prev = d[j - 1];
            for c in 0..N {
                d[j][c] = (1.0 - a) * prev[c] + a * d[j][c];
            }
        }
    }
    d[degree]
}

impl VectorTrack {
    /// Value at `u`, with `absent` for components without data.
    pub fn sample(&self, u: f64, absent: f64) -> [f64; 3] {
        let mut out = [absent; 3];
        let mut spline = [0.0; 3];
        if let Some(c) = &self.curve {
            let pts: Vec<[f64; 3]> = c.points.iter().map(|p| p.map(f64::from)).collect();
            spline = de_boor(&c.knots, &pts, usize::from(c.degree), u);
        }
        for (k, comp) in self.components.iter().enumerate() {
            out[k] = match comp {
                Component::Absent => absent,
                Component::Static(v) => f64::from(*v),
                Component::Spline { min, max } => {
                    f64::from(*min) + (f64::from(*max) - f64::from(*min)) * spline[k] / 65535.0
                }
            };
        }
        out
    }
}

impl RotationTrack {
    /// Unit quaternion (x, y, z, w) at `u`.
    pub fn sample(&self, u: f64) -> [f64; 4] {
        let q = match self {
            Self::Absent => return [0.0, 0.0, 0.0, 1.0],
            Self::Static(q) => q.map(f64::from),
            Self::Spline(c) => {
                let pts: Vec<[f64; 4]> = c.points.iter().map(|p| p.map(f64::from)).collect();
                de_boor(&c.knots, &pts, usize::from(c.degree), u)
            }
        };
        let n = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n > 0.0 && n.is_finite() {
            q.map(|x| x / n)
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    }
}

impl Track {
    pub fn sample(&self, u: f64) -> TrackPose {
        TrackPose {
            translation: self.translation.sample(u, 0.0),
            rotation: self.rotation.sample(u),
            scale: self.scale.sample(u, 1.0),
        }
    }
}

struct Reader<'a, 'f> {
    bytes: &'a [u8],
    p: usize,
    file: &'f str,
    at0: u64,
}
impl<'a> Reader<'a, '_> {
    fn err(&self, at: usize, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(
            CODE,
            self.file,
            self.at0.saturating_add(at as u64),
            expected,
            found,
        )
    }
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8]> {
        let s = self
            .p
            .checked_add(n)
            .and_then(|e| self.bytes.get(self.p..e))
            .ok_or_else(|| {
                self.err(
                    self.p,
                    format!(
                        "{n} bytes of {what} inside the {}-byte block",
                        self.bytes.len()
                    ),
                    "end of block",
                )
            })?;
        self.p += n;
        Ok(s)
    }
    fn u16(&mut self, what: &str) -> Result<u16> {
        let s = self.take(2, what)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn f32(&mut self, what: &str) -> Result<f32> {
        let at = self.p;
        let s = self.take(4, what)?;
        let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
        if !v.is_finite() {
            return Err(self.err(at, format!("finite {what}"), v));
        }
        Ok(v)
    }
    /// Skips zero bytes up to the next multiple of `to` (zero in every
    /// measured block).
    fn align(&mut self, to: usize) -> Result<()> {
        let pad = (to - self.p % to) % to;
        let at = self.p;
        let s = self.take(pad, "alignment")?;
        if s.iter().any(|x| *x != 0) {
            return Err(self.err(
                at,
                format!("{pad} zero bytes up to a multiple of {to}"),
                hex(s),
            ));
        }
        Ok(())
    }
    /// Curve header: u16 last control-point index, u8 degree, then the
    /// knots; checks them against the block's frames.
    fn curve_head(&mut self, frames: u32, b: &Budget) -> Result<(usize, u8, Vec<u8>)> {
        let at = self.p;
        let last = usize::from(self.u16("control-point count")?);
        let degree = self.take(1, "degree")?[0];
        let d = usize::from(degree);
        if degree == 0 || last < d {
            return Err(self.err(
                at,
                "degree at least 1 and more control points than the degree",
                format!("degree {degree}, {} control points", last + 1),
            ));
        }
        let n = last + d + 2;
        let at = self.p;
        let raw = self.take(n, "knots")?;
        let mut knots = Vec::new();
        b.reserve(&mut knots, n, self.file, self.at0.saturating_add(at as u64))?;
        knots.extend_from_slice(raw);
        let end = frames.saturating_sub(1);
        let clamped = knots[..=d].iter().all(|k| *k == 0)
            && knots[n - d - 1..].iter().all(|k| u32::from(*k) == end)
            && knots.windows(2).all(|w| w[0] <= w[1]);
        if !clamped {
            return Err(self.err(
                at,
                format!(
                    "knots non-decreasing, {} zeros first and {} values {end} last",
                    d + 1,
                    d + 1
                ),
                hex(&knots),
            ));
        }
        Ok((last + 1, degree, knots))
    }
    fn vector(&mut self, flags: u8, frames: u32, b: &Budget) -> Result<VectorTrack> {
        let at = self.p;
        let static_bits = flags & 0x0f;
        let spline_bits = flags >> 4;
        if flags & 0x88 != 0 || static_bits & spline_bits != 0 {
            return Err(self.err(
                at,
                "a component flag byte with bits 3 and 7 clear and no component both static and spline",
                format!("{flags:02x}"),
            ));
        }
        let mut t = VectorTrack {
            flags,
            components: [Component::Absent; 3],
            curve: None,
        };
        if spline_bits == 0 {
            for c in 0..3 {
                if static_bits >> c & 1 == 1 {
                    t.components[c] = Component::Static(self.f32("static value")?);
                }
            }
            return Ok(t);
        }
        let (count, degree, knots) = self.curve_head(frames, b)?;
        self.align(4)?;
        for c in 0..3 {
            if spline_bits >> c & 1 == 1 {
                let min = self.f32("range minimum")?;
                let max = self.f32("range maximum")?;
                t.components[c] = Component::Spline { min, max };
            } else if static_bits >> c & 1 == 1 {
                t.components[c] = Component::Static(self.f32("static value")?);
            }
        }
        let mut points = Vec::new();
        b.reserve(
            &mut points,
            count,
            self.file,
            self.at0.saturating_add(self.p as u64),
        )?;
        for _ in 0..count {
            let mut row = [0u16; 3];
            for (c, v) in row.iter_mut().enumerate() {
                if spline_bits >> c & 1 == 1 {
                    *v = self.u16("control point")?;
                }
            }
            points.push(row);
        }
        self.align(4)?;
        t.curve = Some(Curve {
            degree,
            knots,
            points,
        });
        Ok(t)
    }
    fn quaternion(&mut self, kind: u8) -> Result<[f32; 4]> {
        Ok(if kind == ROTATION_40 {
            let s = self.take(5, "rotation")?;
            Quantised::from_40([s[0], s[1], s[2], s[3], s[4]]).quaternion()
        } else {
            let s = self.take(6, "rotation")?;
            Quantised::from_48([s[0], s[1], s[2], s[3], s[4], s[5]]).quaternion()
        })
    }
    fn rotation(&mut self, flags: u8, kind: u8, frames: u32, b: &Budget) -> Result<RotationTrack> {
        let out = if flags >> 4 != 0 {
            let (count, degree, knots) = self.curve_head(frames, b)?;
            if kind == ROTATION_48 {
                self.align(2)?;
            }
            let mut points = Vec::new();
            b.reserve(
                &mut points,
                count,
                self.file,
                self.at0.saturating_add(self.p as u64),
            )?;
            for _ in 0..count {
                points.push(self.quaternion(kind)?);
            }
            RotationTrack::Spline(Curve {
                degree,
                knots,
                points,
            })
        } else if flags & 0x0f != 0 {
            if kind == ROTATION_48 {
                self.align(2)?;
            }
            RotationTrack::Static(self.quaternion(kind)?)
        } else {
            return Ok(RotationTrack::Absent);
        };
        self.align(4)?;
        Ok(out)
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().take(32).map(|x| format!("{x:02x}")).collect()
}

impl SplineAnimation<'_> {
    /// Frames of block `i`: the block's frames per block, the last block
    /// what remains (blocks share their boundary frame).
    pub fn block_frames(&self, i: usize) -> u32 {
        let step = self.frames_per_block.saturating_sub(1);
        let first = (i as u32).saturating_mul(step);
        self.frames.saturating_sub(first).min(self.frames_per_block)
    }

    /// Decodes the transform tracks of block `i`; `file` names the file for
    /// errors, whose offsets are absolute as the read's.
    pub fn decode_block(&self, i: usize, file: &str, b: &Budget) -> Result<Vec<Track>> {
        let bytes = self.block(i).ok_or_else(|| {
            Error::new(
                CODE,
                file,
                self.data_at,
                format!("block index < {}", self.block_offsets.len()),
                i,
            )
        })?;
        let block_at = self
            .data_at
            .saturating_add(u64::from(self.block_offsets[i]));
        let mut r = Reader {
            bytes,
            p: 0,
            file,
            at0: block_at,
        };
        let tracks = self.header.transform_tracks as usize;
        let masks = r.take(self.mask_bytes as usize, "track masks")?;
        let frames = self.block_frames(i);
        let mut out = Vec::new();
        b.reserve(&mut out, tracks, file, r.at0)?;
        for t in 0..tracks {
            let m = [
                masks[4 * t],
                masks[4 * t + 1],
                masks[4 * t + 2],
                masks[4 * t + 3],
            ];
            let rotation_kind = (m[0] >> 2) & 0x0f;
            if m[0] & 3 != VECTOR_16
                || m[0] >> 6 != VECTOR_16
                || !(rotation_kind == ROTATION_40 || rotation_kind == ROTATION_48)
            {
                return Err(r.err(
                    4 * t,
                    "quantisation byte with translation and scale code 1 and rotation code 1 or 2",
                    format!("{:02x}", m[0]),
                ));
            }
            let translation = r.vector(m[1], frames, b)?;
            let rotation = r.rotation(m[2], rotation_kind, frames, b)?;
            let scale = r.vector(m[3], frames, b)?;
            out.push(Track {
                mask: m,
                translation,
                rotation,
                scale,
            });
        }
        let end = self.float_offsets.get(i).copied().unwrap_or(0) as usize;
        if r.p != end {
            return Err(r.err(
                r.p,
                format!("transform data ending at the block's float offset {end}"),
                r.p,
            ));
        }
        if self.header.float_tracks == 0 && bytes[end.min(bytes.len())..].iter().any(|x| *x != 0) {
            return Err(r.err(end, "zero bytes after the transform data", "other bytes"));
        }
        Ok(out)
    }

    /// Decodes every block.
    pub fn decode(&self, file: &str, b: &Budget) -> Result<DecodedSpline> {
        let mut blocks = Vec::new();
        b.reserve(&mut blocks, self.block_offsets.len(), file, self.data_at)?;
        for i in 0..self.block_offsets.len() {
            blocks.push(self.decode_block(i, file, b)?);
        }
        Ok(DecodedSpline {
            blocks,
            frames: self.frames,
            frames_per_block: self.frames_per_block,
            frame_duration: f64::from(self.frame_duration),
            duration: f64::from(self.header.duration),
        })
    }
}

/// Every block of a spline animation, decoded, and its timing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecodedSpline {
    pub blocks: Vec<Vec<Track>>,
    pub frames: u32,
    pub frames_per_block: u32,
    pub frame_duration: f64,
    pub duration: f64,
}
impl DecodedSpline {
    /// Block and frame within it for clip time `t` seconds, clamped to the
    /// clip: frame `t / frame duration`, blocks of `frames per block - 1`
    /// frames sharing their boundary frame.
    pub fn locate(&self, t: f64) -> (usize, f64) {
        let last = f64::from(self.frames.saturating_sub(1));
        let f = if self.frame_duration > 0.0 && t.is_finite() {
            (t / self.frame_duration).clamp(0.0, last)
        } else {
            0.0
        };
        let step = f64::from(self.frames_per_block.saturating_sub(1)).max(1.0);
        let j = ((f / step).floor() as usize).min(self.blocks.len().saturating_sub(1));
        (j, f - j as f64 * step)
    }
    /// Local transform of every track at clip time `t`.
    pub fn sample(&self, t: f64) -> Vec<TrackPose> {
        let (j, u) = self.locate(t);
        self.blocks
            .get(j)
            .map(|tracks| tracks.iter().map(|x| x.sample(u)).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
