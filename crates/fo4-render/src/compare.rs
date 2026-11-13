//! Frozen image diagnostics. No registration, exposure fitting or hypothesis verdicts.
use crate::{append, png::Png};
use fo4_core::{Budget, Error, Limits, Result};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CompareOptions {
    pub gradient_threshold: u8,
    pub depth_relative_threshold: f32,
    pub depth_absolute_threshold: f32,
}
impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            gradient_threshold: 32,
            depth_relative_threshold: 0.01,
            depth_absolute_threshold: 1.,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct EdgeSummary {
    pub count: usize,
    pub median_pixels: f64,
    pub percentile95_pixels: f64,
}
#[derive(Debug, Serialize)]
pub struct InstanceColor {
    pub instance: u32,
    pub pixels: u64,
    pub photo_mean: [f64; 3],
    pub engine_mean: [f64; 3],
    pub engine_srgb_mean: [f64; 3],
}
#[derive(Debug, Serialize)]
pub struct Metrics {
    pub options: CompareOptions,
    pub included_pixels: usize,
    pub photo_edge_pixels: usize,
    pub instance_edges: Option<EdgeSummary>,
    pub depth_edges: Option<EdgeSummary>,
    pub union_edges: Option<EdgeSummary>,
    pub colors: Vec<InstanceColor>,
}
pub struct Comparison {
    pub metrics: Metrics,
    pub distances: Vec<f32>,
    pub side_by_side: Vec<[f32; 4]>,
    pub overlay: Vec<[f32; 4]>,
    pub distance_image: Vec<[f32; 4]>,
}
fn error(code: &str, expected: impl std::fmt::Display, found: impl std::fmt::Display) -> Error {
    Error::new(code, "image comparison", 0, expected, found)
}
fn neighbors(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let x = i % w;
    let y = i / w;
    [
        if x > 0 { Some(i - 1) } else { None },
        if x + 1 < w { Some(i + 1) } else { None },
        if y > 0 { Some(i - w) } else { None },
        if y + 1 < h { Some(i + w) } else { None },
    ]
    .into_iter()
    .flatten()
}
fn one_dimension(f: &[f64], g: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let mut count = 0;
    for q in 0..f.len() {
        if !f[q].is_finite() {
            continue;
        }
        let mut boundary = f64::NEG_INFINITY;
        while count > 0 {
            let p = v[count - 1];
            boundary =
                ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2. * (q as f64 - p as f64));
            if count == 1 || boundary > z[count - 1] {
                break;
            }
            count -= 1;
        }
        if count == 0 {
            boundary = f64::NEG_INFINITY;
        }
        v[count] = q;
        z[count] = boundary;
        count += 1;
        z[count] = f64::INFINITY;
    }
    if count == 0 {
        g.fill(f64::INFINITY);
        return;
    }
    let mut k = 0;
    for (q, out) in g.iter_mut().enumerate() {
        while k + 1 < count && z[k + 1] < (q as f64) {
            k += 1;
        }
        let d = q.abs_diff(v[k]);
        *out = (d * d) as f64 + f[v[k]];
    }
}
/// Exact squared Euclidean lower envelope, separable because dx²+dy² is additive.
pub fn edge_distances(edges: &[bool], size: [u32; 2], b: &Budget) -> Result<Vec<f32>> {
    let [w, h] = size.map(|v| v as usize);
    let n = w
        .checked_mul(h)
        .ok_or_else(|| error("Overflow", "pixel count", w))?;
    if w == 0 || h == 0 || edges.len() != n || !edges.iter().any(|v| *v) {
        return Err(error("NoPhotoEdges", "nonempty edge image", edges.len()));
    }
    let scratch = b.child();
    let mut rows = Vec::new();
    scratch.reserve(&mut rows, n, "edge transform", 0)?;
    rows.resize(n, 0f64);
    let max = w.max(h);
    let mut f = Vec::new();
    let mut g = Vec::new();
    let mut v = Vec::new();
    let mut z = Vec::new();
    for arr in [&mut f, &mut g] {
        scratch.reserve(arr, max, "edge transform", 0)?;
        arr.resize(max, 0f64);
    }
    scratch.reserve(&mut v, max, "edge transform", 0)?;
    v.resize(max, 0usize);
    scratch.reserve(&mut z, max + 1, "edge transform", 0)?;
    z.resize(max + 1, 0f64);
    for y in 0..h {
        for x in 0..w {
            f[x] = if edges[y * w + x] { 0. } else { f64::INFINITY };
        }
        one_dimension(&f[..w], &mut g[..w], &mut v, &mut z);
        rows[y * w..(y + 1) * w].copy_from_slice(&g[..w]);
    }
    let mut out = Vec::new();
    b.reserve(&mut out, n, "edge distances", 0)?;
    out.resize(n, 0f32);
    for x in 0..w {
        for y in 0..h {
            f[y] = rows[y * w + x];
        }
        one_dimension(&f[..h], &mut g[..h], &mut v, &mut z);
        for y in 0..h {
            out[y * w + x] = g[y].sqrt() as f32;
        }
    }
    Ok(out)
}
pub(crate) fn summarize(
    edges: &[bool],
    distance: &[f32],
    b: &Budget,
) -> Result<Option<EdgeSummary>> {
    let scratch = b.child();
    let mut samples = Vec::new();
    for (e, d) in edges.iter().zip(distance) {
        if *e {
            scratch.reserve(&mut samples, 1, "edge quantiles", 0)?;
            samples.push(*d as f64);
        }
    }
    if samples.is_empty() {
        return Ok(None);
    }
    samples.sort_unstable_by(f64::total_cmp);
    let n = samples.len();
    let median = if n % 2 == 0 {
        (samples[n / 2 - 1] + samples[n / 2]) / 2.
    } else {
        samples[n / 2]
    };
    let p95 = (95 * n).div_ceil(100) - 1;
    Ok(Some(EdgeSummary {
        count: n,
        median_pixels: median,
        percentile95_pixels: samples[p95],
    }))
}
#[allow(clippy::too_many_arguments)]
pub fn compare(
    engine: &Png,
    srgb: &Png,
    instances: &[u32],
    depth: &[f32],
    photo: &Png,
    mask: &Png,
    options: CompareOptions,
    l: &Limits,
    b: &Budget,
) -> Result<Comparison> {
    let size = engine.size;
    let [w, h] = size.map(|x| x as usize);
    let n = w
        .checked_mul(h)
        .ok_or_else(|| error("Overflow", "pixel count", w))?;
    fo4_core::limit(n as u64, l.max_entries, "comparison pixels", 0)?;
    let rgba_len = fo4_core::mul(n as u64, 4, "comparison RGBA", 0)?;
    if n == 0
        || [srgb.size, photo.size, mask.size]
            .iter()
            .any(|s| *s != size)
        || instances.len() != n
        || depth.len() != n
        || [engine, srgb, photo, mask]
            .iter()
            .any(|p| p.rgba.len() as u64 != rgba_len)
        || !options.depth_relative_threshold.is_finite()
        || options.depth_relative_threshold < 0.
        || !options.depth_absolute_threshold.is_finite()
        || options.depth_absolute_threshold < 0.
    {
        return Err(error(
            "ComparisonInputMismatch",
            "matching bounded images, maps, thresholds",
            n,
        ));
    }
    let scratch = b.child();
    let mut included = Vec::new();
    scratch.reserve(&mut included, n, "comparison mask", 0)?;
    for p in mask.rgba.as_chunks::<4>().0 {
        if p[0] != p[1] || p[1] != p[2] || ![0, 255].contains(&p[0]) || p[3] != 255 {
            return Err(error(
                "NonBinaryComparisonMask",
                "opaque binary grey",
                fo4_core::HexBytes(p),
            ));
        }
        included.push(p[0] == 255);
    }
    for (i, d) in instances.iter().zip(depth) {
        if (*i == u32::MAX && *d != f32::INFINITY)
            || (*i != u32::MAX && (!d.is_finite() || *d <= 0.))
        {
            return Err(error(
                "InvalidComparisonDepth",
                "positive foreground / +infinity background",
                d,
            ));
        }
    }
    let mut photo_edges = Vec::new();
    let mut instance_edges = Vec::new();
    let mut depth_edges = Vec::new();
    let mut union = Vec::new();
    for a in [
        &mut photo_edges,
        &mut instance_edges,
        &mut depth_edges,
        &mut union,
    ] {
        scratch.reserve(a, n, "comparison edges", 0)?;
        a.resize(n, false);
    }
    for i in 0..n {
        if !included[i] {
            continue;
        }
        for j in neighbors(i, w, h).filter(|j| included[*j]) {
            photo_edges[i] |= (0..3).any(|c| {
                photo.rgba[i * 4 + c].abs_diff(photo.rgba[j * 4 + c]) >= options.gradient_threshold
            });
            instance_edges[i] |= instances[i] != instances[j];
            depth_edges[i] |= match (depth[i].is_finite(), depth[j].is_finite()) {
                (true, true) => {
                    (depth[i] - depth[j]).abs()
                        > options.depth_absolute_threshold.max(
                            options.depth_relative_threshold * depth[i].abs().min(depth[j].abs()),
                        )
                }
                (a, c) => a != c,
            };
        }
        union[i] = instance_edges[i] || depth_edges[i];
    }
    let distances = edge_distances(&photo_edges, size, b)?;
    let metrics_instance = summarize(&instance_edges, &distances, &scratch)?;
    let metrics_depth = summarize(&depth_edges, &distances, &scratch)?;
    let metrics_union = summarize(&union, &distances, &scratch)?;
    type ColorSums = ([f64; 3], [f64; 3], [f64; 3], u64);
    let mut sums: BTreeMap<u32, ColorSums> = BTreeMap::new();
    let mut side = Vec::new();
    b.reserve(
        &mut side,
        n.checked_mul(2)
            .ok_or_else(|| error("Overflow", "side-by-side size", n))?,
        "comparison visual",
        0,
    )?;
    side.resize(n * 2, [0., 0., 0., 1.]);
    let mut overlay = Vec::new();
    b.reserve(&mut overlay, n, "edge overlay", 0)?;
    let mut distance_image = Vec::new();
    b.reserve(&mut distance_image, n, "distance image", 0)?;
    for i in 0..n {
        let a = std::array::from_fn::<_, 4, _>(|c| {
            if c == 3 {
                1.
            } else {
                engine.rgba[i * 4 + c] as f32 / 255.
            }
        });
        let p = std::array::from_fn::<_, 4, _>(|c| {
            if c == 3 {
                1.
            } else {
                photo.rgba[i * 4 + c] as f32 / 255.
            }
        });
        let y = i / w;
        let x = i % w;
        side[y * w * 2 + x] = a;
        side[y * w * 2 + w + x] = p;
        let mut col = p;
        if included[i] {
            if photo_edges[i] {
                col = [0., 1., 1., 1.];
            }
            if union[i] {
                col = if photo_edges[i] {
                    [1., 1., 0., 1.]
                } else {
                    [1., 0., 0., 1.]
                };
            }
        } else {
            col = [0.15, 0.15, 0.15, 1.];
        }
        overlay.push(col);
        distance_image.push(if included[i] && union[i] {
            let d = distances[i] / 16.;
            [d.min(1.), (1. - d).max(0.), 0., 1.]
        } else {
            [0., 0., 0., 1.]
        });
        if included[i] && instances[i] != u32::MAX {
            if !sums.contains_key(&instances[i]) {
                scratch.charge(512, "instance means", 0)?;
            }
            let s = sums.entry(instances[i]).or_default();
            for c in 0..3 {
                s.0[c] += photo.rgba[i * 4 + c] as f64;
                s.1[c] += engine.rgba[i * 4 + c] as f64;
                s.2[c] += srgb.rgba[i * 4 + c] as f64;
            }
            s.3 += 1;
        }
    }
    let mut colors = Vec::new();
    for (instance, (p, e, s, count)) in sums {
        append(
            &mut colors,
            InstanceColor {
                instance,
                pixels: count,
                photo_mean: p.map(|v| v / count as f64),
                engine_mean: e.map(|v| v / count as f64),
                engine_srgb_mean: s.map(|v| v / count as f64),
            },
            b,
        )?;
    }
    Ok(Comparison {
        metrics: Metrics {
            options,
            included_pixels: included.iter().filter(|v| **v).count(),
            photo_edge_pixels: photo_edges.iter().filter(|v| **v).count(),
            instance_edges: metrics_instance,
            depth_edges: metrics_depth,
            union_edges: metrics_union,
            colors,
        },
        distances,
        side_by_side: side,
        overlay,
        distance_image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_euclidean_transform_matches_brute_force_including_diagonals() {
        let w = 13;
        let h = 9;
        let edges: Vec<_> = (0..w * h).map(|i| i % 17 == 3 || i == 0).collect();
        let d = edge_distances(&edges, [w as u32, h as u32], &Budget::new(8 << 20)).unwrap();
        for (i, actual) in d.iter().enumerate().take(w * h) {
            let expected = (0..w * h)
                .filter(|j| edges[*j])
                .map(|j| {
                    let dx = (i % w).abs_diff(j % w);
                    let dy = (i / w).abs_diff(j / w);
                    ((dx * dx + dy * dy) as f64).sqrt() as f32
                })
                .min_by(f32::total_cmp)
                .unwrap();
            assert_eq!(*actual, expected);
        }
        assert!(edge_distances(&[true], [1, 1], &Budget::new(0)).is_err());
        assert!(edge_distances(&[false], [1, 1], &Budget::new(1024)).is_err());
    }
    #[test]
    fn known_vertical_boundary_recovers_one_and_three_pixel_displacements_without_registration() {
        let w = 32;
        let h = 24;
        let pixels = w * h;
        let make = |shift: usize| Png {
            size: [w as u32, h as u32],
            rgba: (0..pixels)
                .flat_map(|i| {
                    let c = if i % w < 12 + shift { 0 } else { 255 };
                    [c, c, c, 255]
                })
                .collect(),
            ancillary_chunks: Vec::new(),
        };
        let engine = make(0);
        let mask = Png {
            size: engine.size,
            rgba: vec![255; pixels * 4],
            ancillary_chunks: Vec::new(),
        };
        let ids: Vec<_> = (0..pixels)
            .map(|i| if i % w < 12 { 1 } else { 2 })
            .collect();
        for shift in [1, 3] {
            let c = compare(
                &engine,
                &engine,
                &ids,
                &vec![2.; pixels],
                &make(shift),
                &mask,
                CompareOptions::default(),
                &Limits::default(),
                &Budget::new(8 << 20),
            )
            .unwrap();
            assert_eq!(
                c.metrics.instance_edges.unwrap().percentile95_pixels,
                shift as f64
            );
            assert_eq!(c.metrics.colors.len(), 2);
            assert_eq!(c.metrics.included_pixels, pixels);
        }
    }
}
