//! Prospective triangle-plane silhouette diagnostic, without image registration or fitting.
use crate::{
    compare::{EdgeSummary, InstanceColor},
    png::Png,
};
use fo4_core::{Budget, Error, Limits, Result};
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Options {
    pub plane_absolute_tolerance: f64,
    pub plane_relative_tolerance: f64,
    pub gradient_contrast_fraction: f64,
    pub minimum_rgb_gradient: u8,
    pub contrast_radius: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            plane_absolute_tolerance: 0.25,
            plane_relative_tolerance: 0.00001,
            gradient_contrast_fraction: 0.25,
            minimum_rgb_gradient: 2,
            contrast_radius: 4,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct OpeningAcceptance {
    pub rectangle: [u32; 4],
    pub perimeter_pixels: usize,
    pub engine_distance_maximum: Option<f32>,
    pub photo_distance_maximum: Option<f32>,
    pub engine_edge_pixels_in_band: usize,
    pub engine_to_photo_maximum: Option<f32>,
    pub passed: bool,
}
#[derive(Debug, Serialize)]
pub struct Metrics {
    pub metric: &'static str,
    pub options: Options,
    pub included_pixels: usize,
    pub engine_edge_pixels: usize,
    pub photo_edge_pixels: usize,
    pub engine_to_photo: Option<EdgeSummary>,
    pub photo_to_engine: Option<EdgeSummary>,
    pub unavailable: Vec<&'static str>,
    pub opening_acceptance: Option<OpeningAcceptance>,
    pub colors: Vec<InstanceColor>,
}
pub struct Comparison {
    pub metrics: Metrics,
    pub distances: Vec<f32>,
    pub reverse_distances: Vec<f32>,
    pub side_by_side: Vec<[f32; 4]>,
    pub overlay: Vec<[f32; 4]>,
    pub distance_image: Vec<[f32; 4]>,
    pub reverse_distance_image: Vec<[f32; 4]>,
}
fn err(code: &str, found: impl std::fmt::Display) -> Error {
    Error::new(
        code,
        "silhouette comparison",
        0,
        "matching finite bounded images, depth and triangle planes",
        found,
    )
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
pub fn engine_edges(
    size: [u32; 2],
    ids: &[u32],
    depth: &[f32],
    planes: &[[f64; 2]],
    included: &[bool],
    o: Options,
    b: &Budget,
) -> Result<Vec<bool>> {
    if !o.plane_absolute_tolerance.is_finite()
        || !o.plane_relative_tolerance.is_finite()
        || o.plane_absolute_tolerance < 0.
        || o.plane_relative_tolerance < 0.
    {
        return Err(err(
            "InvalidSilhouetteOptions",
            "finite nonnegative plane tolerances",
        ));
    }
    let [w, h] = size.map(|v| v as usize);
    let n = fo4_core::mul(w as u64, h as u64, "silhouette pixels", 0)? as usize;
    if w == 0
        || h == 0
        || [ids.len(), depth.len(), planes.len(), included.len()]
            .iter()
            .any(|v| *v != n)
    {
        return Err(err("SilhouetteInputMismatch", n));
    }
    let mut out = Vec::new();
    b.reserve(&mut out, n, "engine silhouette", 0)?;
    out.resize(n, false);
    for i in 0..n {
        let background = ids[i] == u32::MAX;
        if (background && depth[i] != f32::INFINITY)
            || (!background
                && (!depth[i].is_finite()
                    || depth[i] <= 0.
                    || planes[i].iter().any(|v| !v.is_finite())))
        {
            return Err(err("InvalidSilhouetteTrianglePlane", i));
        }
        if !included[i] {
            continue;
        }
        for j in neighbors(i, w, h).filter(|j| included[*j]) {
            let other_background = ids[j] == u32::MAX;
            if background != other_background {
                out[i] = true;
                continue;
            }
            if background {
                continue;
            }
            let dx = (j % w) as f64 - (i % w) as f64;
            let dy = (j / w) as f64 - (i / w) as f64;
            let q = 1. / depth[i] as f64 + planes[i][0] * dx + planes[i][1] * dy;
            if !q.is_finite() {
                return Err(err("InvalidPredictedReciprocalDepth", i));
            }
            // A plane crossing infinity is a geometric discontinuity, never a fabricated depth.
            if q <= 0. {
                out[i] = true;
                continue;
            }
            let predicted = 1. / q;
            let actual = depth[j] as f64;
            let tolerance =
                o.plane_absolute_tolerance + o.plane_relative_tolerance * actual.max(predicted);
            out[i] |= (actual - predicted).abs() > tolerance;
        }
    }
    Ok(out)
}
fn photo_edges(photo: &Png, included: &[bool], o: Options, b: &Budget) -> Result<Vec<bool>> {
    let [w, h] = photo.size.map(|v| v as usize);
    let n = included.len();
    let mut out = Vec::new();
    b.reserve(&mut out, n, "photo silhouette candidates", 0)?;
    out.resize(n, false);
    for i in 0..n {
        if !included[i] {
            continue;
        }
        let x = i % w;
        let y = i / w;
        let mut low = [255u8; 3];
        let mut high = [0u8; 3];
        for yy in
            y.saturating_sub(o.contrast_radius)..=y.saturating_add(o.contrast_radius).min(h - 1)
        {
            for xx in
                x.saturating_sub(o.contrast_radius)..=x.saturating_add(o.contrast_radius).min(w - 1)
            {
                let j = yy * w + xx;
                if !included[j] {
                    continue;
                }
                for c in 0..3 {
                    let v = photo.rgba[j * 4 + c];
                    low[c] = low[c].min(v);
                    high[c] = high[c].max(v);
                }
            }
        }
        let contrast = (0..3)
            .map(|c| high[c].abs_diff(low[c]))
            .max()
            .ok_or_else(|| err("EmptyColorChannels", i))?;
        let threshold =
            (o.minimum_rgb_gradient as f64).max(o.gradient_contrast_fraction * contrast as f64);
        for j in neighbors(i, w, h).filter(|j| included[*j]) {
            out[i] |= (0..3)
                .any(|c| photo.rgba[i * 4 + c].abs_diff(photo.rgba[j * 4 + c]) as f64 >= threshold);
        }
    }
    Ok(out)
}
fn distance(edges: &[bool], size: [u32; 2], b: &Budget) -> Result<Vec<f32>> {
    if edges.iter().any(|v| *v) {
        crate::compare::edge_distances(edges, size, b)
    } else {
        let mut v = Vec::new();
        b.reserve(&mut v, edges.len(), "unavailable distances", 0)?;
        v.resize(edges.len(), f32::NAN);
        Ok(v)
    }
}
fn opening(
    rect: [u32; 4],
    size: [u32; 2],
    engine: &[bool],
    to_engine: &[f32],
    to_photo: &[f32],
) -> Result<OpeningAcceptance> {
    // Same convention as `persistent::opening`: [left, top, right, bottom], inclusive.
    let [x0, y0, x1, y1] = rect;
    if x0 >= x1 || y0 >= y1 || x1 >= size[0] || y1 >= size[1] {
        return Err(err("InvalidAcceptanceRectangle", format!("{rect:?}")));
    }
    let w = size[0] as usize;
    let mut count = 0;
    let mut emax = None::<f32>;
    let mut pmax = None::<f32>;
    let mut valid = true;
    for y in y0..=y1 {
        for x in x0..=x1 {
            if x == x0 || x == x1 || y == y0 || y == y1 {
                count += 1;
                let i = y as usize * w + x as usize;
                let (e, p) = (to_engine[i], to_photo[i]);
                if e.is_finite() && p.is_finite() {
                    emax = Some(emax.map_or(e, |m| m.max(e)));
                    pmax = Some(pmax.map_or(p, |m| m.max(p)));
                } else {
                    valid = false;
                }
            }
        }
    }
    let mut in_band = 0;
    let mut forward_max = None::<f32>;
    for (i, is_edge) in engine.iter().enumerate().filter(|(_, v)| **v) {
        let _ = is_edge;
        let x = (i % w) as f64;
        let y = (i / w) as f64;
        let vertical = (x - x0 as f64).abs().min((x - x1 as f64).abs());
        let horizontal = (y - y0 as f64).abs().min((y - y1 as f64).abs());
        let band = (y >= y0 as f64 && y <= y1 as f64 && vertical <= 1.)
            || (x >= x0 as f64 && x <= x1 as f64 && horizontal <= 1.);
        if band {
            in_band += 1;
            let d = to_photo[i];
            if d.is_finite() {
                forward_max = Some(forward_max.map_or(d, |m| m.max(d)));
            } else {
                valid = false;
            }
        }
    }
    let passed = valid
        && in_band > 0
        && emax.is_some_and(|v| v <= 1.)
        && pmax.is_some_and(|v| v <= 1.)
        && forward_max.is_some_and(|v| v <= 1.);
    Ok(OpeningAcceptance {
        rectangle: rect,
        perimeter_pixels: count,
        engine_distance_maximum: emax,
        photo_distance_maximum: pmax,
        engine_edge_pixels_in_band: in_band,
        engine_to_photo_maximum: forward_max,
        passed,
    })
}
#[allow(clippy::too_many_arguments)]
pub fn compare(
    engine: &Png,
    srgb: &Png,
    ids: &[u32],
    depth: &[f32],
    planes: &[[f64; 2]],
    photo: &Png,
    mask: &Png,
    o: Options,
    rectangle: Option<[u32; 4]>,
    l: &Limits,
    b: &Budget,
) -> Result<Comparison> {
    let [w, h] = engine.size.map(|v| v as usize);
    let count = fo4_core::mul(w as u64, h as u64, "silhouette pixels", 0)?;
    fo4_core::limit(count, l.max_entries, "silhouette pixels", 0)?;
    let n = count as usize;
    let rgba = fo4_core::mul(count, 4, "comparison RGBA", 0)?;
    if n == 0
        || [photo, srgb, mask]
            .iter()
            .any(|p| p.size != engine.size || p.rgba.len() as u64 != rgba)
        || engine.rgba.len() as u64 != rgba
        || o.contrast_radius > 16
        || ![
            o.plane_absolute_tolerance,
            o.plane_relative_tolerance,
            o.gradient_contrast_fraction,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
    {
        return Err(err("SilhouetteInputMismatch", n));
    }
    let scratch = b.child();
    let mut included = Vec::new();
    scratch.reserve(&mut included, n, "comparison mask", 0)?;
    for p in mask.rgba.as_chunks::<4>().0 {
        if p[0] != p[1] || p[1] != p[2] || ![0, 255].contains(&p[0]) || p[3] != 255 {
            return Err(err("NonBinaryComparisonMask", fo4_core::HexBytes(p)));
        }
        included.push(p[0] == 255);
    }
    let ee = engine_edges(engine.size, ids, depth, planes, &included, o, &scratch)?;
    let pe = photo_edges(photo, &included, o, &scratch)?;
    let distances = distance(&pe, engine.size, b)?;
    let reverse_distances = distance(&ee, engine.size, b)?;
    let mut unavailable = Vec::new();
    let engine_to_photo = if pe.iter().any(|v| *v) {
        crate::compare::summarize(&ee, &distances, &scratch)?
    } else {
        b.reserve(&mut unavailable, 1, "metric statuses", 0)?;
        unavailable.push("NoPhotoGradientEdges");
        None
    };
    let photo_to_engine = if ee.iter().any(|v| *v) {
        crate::compare::summarize(&pe, &reverse_distances, &scratch)?
    } else {
        b.reserve(&mut unavailable, 1, "metric statuses", 0)?;
        unavailable.push("NoEngineSilhouetteEdges");
        None
    };
    let acceptance = rectangle
        .map(|rect| opening(rect, engine.size, &ee, &reverse_distances, &distances))
        .transpose()?;
    let mut side = Vec::new();
    b.reserve(
        &mut side,
        n.checked_mul(2).ok_or_else(|| err("Overflow", n))?,
        "side by side",
        0,
    )?;
    side.resize(n * 2, [0., 0., 0., 1.]);
    let mut overlay = Vec::new();
    let mut image = Vec::new();
    let mut reverse = Vec::new();
    for a in [&mut overlay, &mut image, &mut reverse] {
        b.reserve(a, n, "silhouette visuals", 0)?;
    }
    type Sums = ([f64; 3], [f64; 3], [f64; 3], u64);
    let mut sums = BTreeMap::<u32, Sums>::new();
    for i in 0..n {
        let pixel = |p: &Png| {
            std::array::from_fn::<_, 4, _>(|c| {
                if c == 3 {
                    1.
                } else {
                    p.rgba[i * 4 + c] as f32 / 255.
                }
            })
        };
        let a = pixel(engine);
        let p = pixel(photo);
        let x = i % w;
        let y = i / w;
        side[y * w * 2 + x] = a;
        side[y * w * 2 + w + x] = p;
        overlay.push(if !included[i] {
            [0.15, 0.15, 0.15, 1.]
        } else {
            match (ee[i], pe[i]) {
                (true, true) => [1., 1., 0., 1.],
                (true, false) => [1., 0., 0., 1.],
                (false, true) => [0., 1., 1., 1.],
                _ => p,
            }
        });
        let color = |d: f32, is_source: bool| {
            if !included[i] || !is_source {
                [0., 0., 0., 1.]
            } else if !d.is_finite() {
                [0.3, 0.3, 0.3, 1.]
            } else {
                let d = d / 16.;
                [d.min(1.), (1. - d).max(0.), 0., 1.]
            }
        };
        image.push(color(distances[i], ee[i]));
        reverse.push(color(reverse_distances[i], pe[i]));
        if included[i] && ids[i] != u32::MAX {
            if !sums.contains_key(&ids[i]) {
                scratch.charge(512, "instance means", 0)?;
            }
            let s = sums.entry(ids[i]).or_default();
            for c in 0..3 {
                s.0[c] += photo.rgba[i * 4 + c] as f64;
                s.1[c] += engine.rgba[i * 4 + c] as f64;
                s.2[c] += srgb.rgba[i * 4 + c] as f64;
            }
            s.3 += 1;
        }
    }
    let mut colors = Vec::new();
    for (instance, (p, e, s, n)) in sums {
        b.reserve(&mut colors, 1, "instance means", 0)?;
        colors.push(InstanceColor {
            instance,
            pixels: n,
            photo_mean: p.map(|v| v / n as f64),
            engine_mean: e.map(|v| v / n as f64),
            engine_srgb_mean: s.map(|v| v / n as f64),
        });
    }
    Ok(Comparison {
        metrics: Metrics {
            metric: "triangle_plane_silhouette_v1",
            options: o,
            included_pixels: included.iter().filter(|v| **v).count(),
            engine_edge_pixels: ee.iter().filter(|v| **v).count(),
            photo_edge_pixels: pe.iter().filter(|v| **v).count(),
            engine_to_photo,
            photo_to_engine,
            unavailable,
            opening_acceptance: acceptance,
            colors,
        },
        distances,
        reverse_distances,
        side_by_side: side,
        overlay,
        distance_image: image,
        reverse_distance_image: reverse,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreshortened_coplanar_checkerboard_has_no_internal_silhouette() {
        let (w, h) = (32, 24);
        let n = w * h;
        let ids: Vec<_> = (0..n).map(|i| (i % 2) as u32).collect();
        let depth: Vec<_> = (0..n)
            .map(|i| (1. / (0.002 + 0.00001 * (i % w) as f64 + 0.00008 * (i / w) as f64)) as f32)
            .collect();
        assert!((depth[0] - depth[w]).abs() > depth[0] * 0.01);
        let e = engine_edges(
            [w as u32, h as u32],
            &ids,
            &depth,
            &vec![[0.00001, 0.00008]; n],
            &vec![true; n],
            Options::default(),
            &Budget::new(1 << 20),
        )
        .unwrap();
        assert!(!e.iter().any(|v| *v));
        let mut bad = vec![[0., 0.]; n];
        bad[0] = [f64::NAN; 2];
        assert_eq!(
            engine_edges(
                [w as u32, h as u32],
                &ids,
                &depth,
                &bad,
                &vec![true; n],
                Options::default(),
                &Budget::new(1 << 20)
            )
            .unwrap_err()
            .code,
            "InvalidSilhouetteTrianglePlane"
        );
    }
    #[test]
    fn both_directions_recover_one_and_three_pixel_displacements() {
        let (w, h) = (32, 24);
        let n = w * h;
        let photo = |shift: usize| Png {
            size: [w as u32, h as u32],
            rgba: (0..n)
                .flat_map(|i| {
                    let c = if i % w < 12 + shift { 0 } else { 255 };
                    [c, c, c, 255]
                })
                .collect(),
            ancillary_chunks: Vec::new(),
        };
        let mask = Png {
            size: [w as u32, h as u32],
            rgba: vec![255; n * 4],
            ancillary_chunks: Vec::new(),
        };
        let ids = vec![0; n];
        let depth: Vec<_> = (0..n)
            .map(|i| if i % w < 12 { 100. } else { 300. })
            .collect();
        for shift in [1, 3] {
            let c = compare(
                &photo(0),
                &photo(0),
                &ids,
                &depth,
                &vec![[0., 0.]; n],
                &photo(shift),
                &mask,
                Options::default(),
                None,
                &Limits::default(),
                &Budget::new(8 << 20),
            )
            .unwrap();
            assert_eq!(
                c.metrics.engine_to_photo.unwrap().percentile95_pixels,
                shift as f64
            );
            assert_eq!(
                c.metrics.photo_to_engine.unwrap().percentile95_pixels,
                shift as f64
            );
        }
        assert!(compare(
            &photo(0),
            &photo(0),
            &ids,
            &depth,
            &vec![[0., 0.]; n],
            &photo(0),
            &mask,
            Options::default(),
            None,
            &Limits::default(),
            &Budget::new(0)
        )
        .is_err());
    }
}

#[cfg(test)]
mod opening_tests {
    use super::*;
    #[test]
    fn opening_rectangle_is_left_top_right_bottom() {
        let size = [8u32, 6];
        let n = 48;
        let near = vec![0.5f32; n];
        let mut engine = vec![false; n];
        // One engine edge pixel on the top side of the rectangle.
        engine[2 * 8 + 3] = true;
        // 6 columns by 3 rows: a non-square rectangle distinguishes the orders.
        let a = opening([1, 2, 6, 4], size, &engine, &near, &near).unwrap();
        assert_eq!(a.rectangle, [1, 2, 6, 4]);
        assert_eq!(a.perimeter_pixels, 14);
        assert_eq!(a.engine_edge_pixels_in_band, 1);
        // Read as [x0, x1, y0, y1] these numbers would not even be ordered.
        assert!(opening([1, 6, 2, 4], size, &engine, &near, &near).is_err());
        assert!(opening([1, 2, 8, 4], size, &engine, &near, &near).is_err());
    }
}
