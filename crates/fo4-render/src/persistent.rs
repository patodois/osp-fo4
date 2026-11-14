//! Photo-first edges: fixed-scale blur, nonmaximum suppression, paired persistence.
//! Selection never reads the engine. Frozen parameters: spec/photo-metrics.md.
use crate::{
    compare::{edge_distances, EdgeSummary},
    png::Png,
};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;
const KERNEL: [u32; 17] = [
    1, 16, 120, 560, 1820, 4368, 8008, 11440, 12870, 11440, 8008, 4368, 1820, 560, 120, 16, 1,
];
const MAX_PIXELS: u64 = 16_777_216;
fn err(code: &str, expected: impl std::fmt::Display, found: impl std::fmt::Display) -> Error {
    Error::new(code, "persistent photo edges", 0, expected, found)
}
fn array<T: Clone>(n: usize, value: T, b: &Budget) -> Result<Vec<T>> {
    let mut v = Vec::new();
    b.reserve(&mut v, n, "persistent edge buffer", 0)?;
    v.resize(n, value);
    Ok(v)
}
fn dimensions(p: &Png) -> Result<usize> {
    let n = fo4_core::mul(p.size[0] as u64, p.size[1] as u64, "photo pixels", 0)?;
    fo4_core::limit(n, MAX_PIXELS, "photo pixels", 0)?;
    if p.size.contains(&0) || fo4_core::mul(n, 4, "photo RGBA", 0)? != p.rgba.len() as u64 {
        return Err(err(
            "PersistentImageMismatch",
            "positive width*height RGBA",
            p.rgba.len(),
        ));
    }
    usize::try_from(n).map_err(|e| err("Overflow", "usize pixels", e))
}
pub fn included(mask: &Png, b: &Budget) -> Result<Vec<bool>> {
    dimensions(mask)?;
    let mut out = Vec::new();
    b.reserve(&mut out, mask.rgba.len() / 4, "persistent mask", 0)?;
    for p in mask.rgba.as_chunks::<4>().0 {
        if p[0] != p[1] || p[1] != p[2] || ![0, 255].contains(&p[0]) || p[3] != 255 {
            return Err(err(
                "NonBinaryComparisonMask",
                "opaque black or white",
                fo4_core::HexBytes(p),
            ));
        }
        out.push(p[0] == 255);
    }
    Ok(out)
}
fn sample(values: &[[f32; 3]], w: usize, h: usize, x: f64, y: f64) -> f64 {
    let x = x.clamp(0., (w - 1) as f64);
    let y = y.clamp(0., (h - 1) as f64);
    let a = x.floor() as usize;
    let b = y.floor() as usize;
    let c = (a + 1).min(w - 1);
    let d = (b + 1).min(h - 1);
    let tx = x - a as f64;
    let ty = y - b as f64;
    (values[b * w + a][0] as f64 * (1. - tx) + values[b * w + c][0] as f64 * tx) * (1. - ty)
        + (values[d * w + a][0] as f64 * (1. - tx) + values[d * w + c][0] as f64 * tx) * ty
}
fn single(p: &Png, mask: &[bool], b: &Budget) -> Result<(Vec<bool>, Vec<f32>)> {
    let scratch = b.child();
    let [w, h] = p.size.map(|v| v as usize);
    let n = dimensions(p)?;
    if mask.len() != n {
        return Err(err("PersistentMaskMismatch", n, mask.len()));
    }
    let mut horizontal = array(n, [0f32; 3], &scratch)?;
    let mut blur = array(n, [0f32; 3], &scratch)?;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !mask[i] {
                continue;
            }
            let mut sum = [0f64; 3];
            let mut weight = 0u32;
            for (k, kw) in KERNEL.iter().enumerate() {
                let xx = x as i64 + k as i64 - 8;
                if xx < 0 || xx >= w as i64 {
                    continue;
                }
                let j = y * w + xx as usize;
                if !mask[j] {
                    continue;
                }
                weight += kw;
                for (c, v) in sum.iter_mut().enumerate() {
                    *v += p.rgba[j * 4 + c] as f64 * *kw as f64;
                }
            }
            // The included centre always contributes positive weight.
            horizontal[i] = sum.map(|v| (v / weight as f64) as f32);
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !mask[i] {
                continue;
            }
            let mut sum = [0f64; 3];
            let mut weight = 0u32;
            for (k, kw) in KERNEL.iter().enumerate() {
                let yy = y as i64 + k as i64 - 8;
                if yy < 0 || yy >= h as i64 {
                    continue;
                }
                let j = yy as usize * w + x;
                if !mask[j] {
                    continue;
                }
                weight += kw;
                for (c, v) in sum.iter_mut().enumerate() {
                    *v += horizontal[j][c] as f64 * *kw as f64;
                }
            }
            blur[i] = sum.map(|v| (v / weight as f64) as f32);
        }
    }
    let mut gradient = array(n, [0f32; 3], &scratch)?;
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = y * w + x;
            if ![i, i - 1, i + 1, i - w, i + w].iter().all(|j| mask[*j]) {
                continue;
            }
            for ((right, left), (down, up)) in blur[i + 1]
                .iter()
                .zip(&blur[i - 1])
                .zip(blur[i + w].iter().zip(&blur[i - w]))
            {
                let dx = (right - left) * 0.5;
                let dy = (down - up) * 0.5;
                let magnitude = dx.hypot(dy);
                if magnitude > gradient[i][0] {
                    gradient[i] = [magnitude, dx, dy];
                }
            }
        }
    }
    let mut edges = array(n, false, b)?;
    let mut strength = array(n, 0f32, b)?;
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = y * w + x;
            let [m, dx, dy] = gradient[i];
            if m < 4. {
                continue;
            }
            let dx = (dx / m) as f64;
            let dy = (dy / m) as f64;
            if m as f64 >= sample(&gradient, w, h, x as f64 + dx, y as f64 + dy)
                && m as f64 >= sample(&gradient, w, h, x as f64 - dx, y as f64 - dy)
            {
                edges[i] = true;
                strength[i] = m;
            }
        }
    }
    Ok((edges, strength))
}
pub struct Detection {
    pub size: [u32; 2],
    pub edges: Vec<bool>,
    pub strength: Vec<f32>,
    pub primary_count: usize,
    pub secondary_count: usize,
}
pub fn detect(primary: &Png, secondary: &Png, mask: &Png, b: &Budget) -> Result<Detection> {
    dimensions(primary)?;
    dimensions(secondary)?;
    dimensions(mask)?;
    if primary.size != secondary.size || primary.size != mask.size {
        return Err(err(
            "PersistentImageMismatch",
            "same pair and mask dimensions",
            format_args!("{:?}/{:?}/{:?}", primary.size, secondary.size, mask.size),
        ));
    }
    let scratch = b.child();
    let inside = included(mask, &scratch)?;
    let (mut edges, mut strength) = single(primary, &inside, b)?;
    let primary_count = edges.iter().filter(|v| **v).count();
    let (repeat, _) = single(secondary, &inside, &scratch)?;
    let secondary_count = repeat.iter().filter(|v| **v).count();
    if secondary_count == 0 {
        edges.fill(false);
        strength.fill(0.);
    } else {
        let distance = edge_distances(&repeat, primary.size, &scratch)?;
        for i in 0..edges.len() {
            edges[i] &= distance[i] <= 1.;
            if !edges[i] {
                strength[i] = 0.;
            }
        }
    }
    Ok(Detection {
        size: primary.size,
        edges,
        strength,
        primary_count,
        secondary_count,
    })
}
#[derive(Debug, Serialize)]
pub struct Side {
    pub name: &'static str,
    pub required: usize,
    pub covered: usize,
    pub photo_to_engine: Option<EdgeSummary>,
    pub maximum_pixels: Option<f32>,
    pub passed: bool,
}
#[derive(Debug, Serialize)]
pub struct Acceptance {
    pub rectangle: [u32; 4],
    pub sides: Vec<Side>,
    pub passed: bool,
}
pub fn opening(
    d: &Detection,
    engine_distance: &[f32],
    rect: [u32; 4],
    b: &Budget,
) -> Result<Acceptance> {
    let [w, h] = d.size.map(|v| v as usize);
    let [left, top, right, bottom] = rect.map(|v| v as usize);
    if left > right
        || top > bottom
        || right >= w
        || bottom >= h
        || engine_distance.len() != d.edges.len()
        || d.strength.len() != d.edges.len()
        || d.edges.len() != w * h
    {
        return Err(err(
            "InvalidOpeningRectangle",
            "ordered in-bounds rectangle and matching arrays",
            format_args!("{rect:?}"),
        ));
    }
    if let Some((i, v)) = engine_distance
        .iter()
        .enumerate()
        .find(|(_, v)| !v.is_finite() || **v < 0.)
    {
        return Err(Error::new(
            "InvalidOpeningDistances",
            "persistent photo edges",
            i as u64,
            "finite nonnegative engine distances",
            v,
        ));
    }
    if let Some((i, v)) = d
        .strength
        .iter()
        .enumerate()
        .find(|(i, v)| !v.is_finite() || **v < 0. || (d.edges[*i] && **v < 4.))
    {
        return Err(Error::new(
            "InvalidPersistentStrength",
            "persistent photo edges",
            i as u64,
            "finite nonnegative strength, >=4 at edges",
            v,
        ));
    }
    let mut sides = Vec::new();
    for (name, vertical, normal, start, end) in [
        ("left", true, left, top, bottom),
        ("right", true, right, top, bottom),
        ("top", false, top, left, right),
        ("bottom", false, bottom, left, right),
    ] {
        let scratch = b.child();
        let mut samples = Vec::new();
        let mut maximum = 0f32;
        let mut covered = 0;
        for tangent in start..=end {
            let mut chosen = None::<usize>;
            for t in
                tangent.saturating_sub(1)..=(tangent + 1).min(if vertical { h - 1 } else { w - 1 })
            {
                for n in normal.saturating_sub(8)..=(normal + 8).min(if vertical {
                    w - 1
                } else {
                    h - 1
                }) {
                    let i = if vertical { t * w + n } else { n * w + t };
                    if d.edges[i]
                        && chosen.is_none_or(|j| {
                            d.strength[i] > d.strength[j]
                                || (d.strength[i] == d.strength[j] && i < j)
                        })
                    {
                        chosen = Some(i);
                    }
                }
            }
            if let Some(i) = chosen {
                covered += 1;
                maximum = maximum.max(engine_distance[i]);
                scratch.reserve(&mut samples, 1, "opening distances", i as u64)?;
                samples.push(engine_distance[i] as f64);
            }
        }
        samples.sort_unstable_by(f64::total_cmp);
        let n = samples.len();
        let summary = if n == 0 {
            None
        } else {
            Some(EdgeSummary {
                count: n,
                median_pixels: if n % 2 == 0 {
                    (samples[n / 2 - 1] + samples[n / 2]) / 2.
                } else {
                    samples[n / 2]
                },
                percentile95_pixels: samples[(95 * n).div_ceil(100) - 1],
            })
        };
        b.reserve(&mut sides, 1, "opening sides", 0)?;
        sides.push(Side {
            name,
            required: end - start + 1,
            covered,
            photo_to_engine: summary,
            maximum_pixels: if covered == 0 { None } else { Some(maximum) },
            passed: covered == end - start + 1 && maximum <= 1.,
        });
    }
    let passed = sides.iter().all(|s| s.passed);
    Ok(Acceptance {
        rectangle: rect,
        sides,
        passed,
    })
}
#[derive(Debug, Serialize)]
pub struct Region {
    pub label: u32,
    pub pixels: usize,
    pub bounds: [u32; 4],
    pub centroid: [f64; 2],
    pub maximum_distance_pixels: Option<f32>,
    pub reportable: bool,
}
pub struct Missing {
    pub regions: Vec<Region>,
    pub labels: Vec<u32>,
    pub distances: Vec<f32>,
    pub persistent_edges: usize,
    pub missing_edges: usize,
    pub unavailable: Vec<&'static str>,
}
pub fn missing(d: &Detection, engine: &[bool], b: &Budget) -> Result<Missing> {
    let n = d.edges.len();
    let [w, h] = d.size.map(|v| v as usize);
    if n != w
        .checked_mul(h)
        .ok_or_else(|| err("Overflow", "pixel count", w))?
        || engine.len() != n
    {
        return Err(err("PersistentImageMismatch", n, engine.len()));
    }
    let mut unavailable = Vec::new();
    let distances = if engine.iter().any(|v| *v) {
        edge_distances(engine, d.size, b)?
    } else {
        b.reserve(&mut unavailable, 1, "edge statuses", 0)?;
        unavailable.push("NoEngineSilhouetteEdges");
        array(n, f32::INFINITY, b)?
    };
    if !d.edges.iter().any(|v| *v) {
        b.reserve(&mut unavailable, 1, "edge statuses", 0)?;
        unavailable.push("NoPersistentPhotoEdges");
    }
    let mut labels = array(n, 0u32, b)?;
    let mut regions = Vec::new();
    let scratch = b.child();
    let mut queue = Vec::new();
    let mut missing_edges = 0;
    for i in 0..n {
        if !d.edges[i] || distances[i] <= 2. || labels[i] != 0 {
            continue;
        }
        let label = u32::try_from(regions.len() + 1).map_err(|e| err("RegionCount", "u32", e))?;
        queue.clear();
        scratch.reserve(&mut queue, 1, "region queue", i as u64)?;
        queue.push(i);
        labels[i] = label;
        let mut at = 0;
        let mut bounds = [w, h, 0, 0];
        let mut sum = [0u64; 2];
        let mut max = 0f32;
        while at < queue.len() {
            let j = queue[at];
            at += 1;
            let x = j % w;
            let y = j / w;
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
            sum[0] += x as u64;
            sum[1] += y as u64;
            max = max.max(distances[j]);
            for yy in y.saturating_sub(3)..=(y + 3).min(h - 1) {
                for xx in x.saturating_sub(3)..=(x + 3).min(w - 1) {
                    if x.abs_diff(xx).pow(2) + y.abs_diff(yy).pow(2) > 9 {
                        continue;
                    }
                    let k = yy * w + xx;
                    if labels[k] == 0 && d.edges[k] && distances[k] > 2. {
                        labels[k] = label;
                        scratch.reserve(&mut queue, 1, "region queue", k as u64)?;
                        queue.push(k);
                    }
                }
            }
        }
        missing_edges += queue.len();
        b.reserve(&mut regions, 1, "missing regions", i as u64)?;
        regions.push(Region {
            label,
            pixels: queue.len(),
            bounds: bounds.map(|v| v as u32),
            centroid: sum.map(|v| v as f64 / queue.len() as f64),
            maximum_distance_pixels: if max.is_finite() { Some(max) } else { None },
            reportable: queue.len() >= 8,
        });
    }
    Ok(Missing {
        regions,
        labels,
        distances,
        persistent_edges: d.edges.iter().filter(|v| **v).count(),
        missing_edges,
        unavailable,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image(shift: usize, texture: bool) -> Png {
        let mut rgba = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                let value = if (16 + shift..48 + shift).contains(&x) && (16..48).contains(&y) {
                    200
                } else {
                    20
                };
                let value = if texture {
                    value + if (x + y) % 2 == 0 { 15 } else { 0 }
                } else {
                    value
                };
                rgba.extend([value, value, value, 255]);
            }
        }
        Png {
            size: [64, 64],
            rgba,
            ancillary_chunks: vec![],
        }
    }
    fn mask() -> Png {
        Png {
            size: [64, 64],
            rgba: vec![255; 64 * 64 * 4],
            ancillary_chunks: vec![],
        }
    }
    #[test]
    fn pair_blur_rejects_fine_texture_and_nonpersistent_edges() {
        let b = Budget::new(16 << 20);
        let d = detect(&image(0, true), &image(0, false), &mask(), &b).unwrap();
        assert!(d.edges.iter().any(|v| *v));
        assert!(!d.edges[32 * 64 + 32]);
        let shifted = detect(&image(0, false), &image(5, false), &mask(), &b).unwrap();
        assert!(!shifted.edges[32 * 64 + 15] && !shifted.edges[32 * 64 + 16]);
        let empty = Png {
            size: [64, 64],
            rgba: vec![255; 64 * 64 * 4],
            ancillary_chunks: vec![],
        };
        let d = detect(&image(0, false), &empty, &mask(), &b).unwrap();
        assert_eq!(d.edges.iter().filter(|v| **v).count(), 0);
    }
    #[test]
    fn four_sides_use_photo_selection_and_engine_distance_without_registration() {
        let b = Budget::new(16 << 20);
        let d = detect(&image(0, false), &image(0, false), &mask(), &b).unwrap();
        let dist = edge_distances(&d.edges, d.size, &b).unwrap();
        let a = opening(&d, &dist, [16, 16, 47, 47], &b).unwrap();
        assert!(a.passed);
        let mut corrupt = dist.clone();
        corrupt[0] = f32::NAN;
        assert_eq!(
            opening(&d, &corrupt, [16, 16, 47, 47], &b)
                .unwrap_err()
                .code,
            "InvalidOpeningDistances"
        );
        let shifted = detect(&image(4, false), &image(4, false), &mask(), &b).unwrap();
        let wrong = edge_distances(&shifted.edges, d.size, &b).unwrap();
        assert!(!opening(&d, &wrong, [16, 16, 47, 47], &b).unwrap().passed);
        let m = missing(&d, &shifted.edges, &b).unwrap();
        assert!(m.missing_edges > 0);
        assert_eq!(
            m.regions.iter().map(|r| r.pixels).sum::<usize>(),
            m.missing_edges
        );
    }
    #[test]
    fn bounded_inputs_and_empty_engine_are_explicit() {
        assert!(detect(&image(0, false), &image(0, false), &mask(), &Budget::new(0)).is_err());
        let mut bad = mask();
        bad.rgba[0] = 1;
        assert_eq!(
            detect(
                &image(0, false),
                &image(0, false),
                &bad,
                &Budget::new(16 << 20)
            )
            .unwrap_err_code(),
            "NonBinaryComparisonMask"
        );
        let b = Budget::new(16 << 20);
        let d = detect(&image(0, false), &image(0, false), &mask(), &b).unwrap();
        let m = missing(&d, &vec![false; 4096], &b).unwrap();
        assert_eq!(m.unavailable, ["NoEngineSilhouetteEdges"]);
        assert!(m
            .regions
            .iter()
            .all(|r| r.maximum_distance_pixels.is_none()));
        let mut p = image(0, false);
        p.rgba.pop();
        assert!(detect(&p, &p, &mask(), &b).is_err());
        p.size = [u32::MAX, u32::MAX];
        assert!(detect(&p, &p, &mask(), &b).is_err());
    }
    trait ErrorCode {
        fn unwrap_err_code(self) -> String;
    }
    impl<T> ErrorCode for Result<T> {
        fn unwrap_err_code(self) -> String {
            match self {
                Err(e) => e.code,
                Ok(_) => panic!("expected failure"),
            }
        }
    }
}
