use crate::{append, Alpha, Draw, DrawList, Texture};
use fo4_core::{Budget, Error, Limits, Result};
use fo4_scene::camera::Camera;
use serde::Serialize;
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub workers: usize,
    pub clear: [f32; 4],
    pub bilinear: bool,
    pub backface_cull: bool,
}
impl RenderOptions {
    /// Options of the reference renders (the `render` example and the GPU viewer).
    pub const REFERENCE: Self = Self {
        workers: 1,
        bilinear: true,
        backface_cull: false,
        clear: [0.025, 0.025, 0.025, 1.],
    };
}
#[derive(Debug, Default, Serialize)]
pub struct Statistics {
    pub input_triangles: u64,
    pub clipped_triangles: u64,
    pub shaded_samples: u64,
    pub contributing_samples: u64,
    pub culled_triangles: u64,
}
#[derive(Debug)]
pub struct Frame {
    pub size: [u32; 2],
    pub color: Vec<[f32; 4]>,
    pub instances: Vec<u32>,
    pub depth: Vec<f32>,
    /// Analytic screen derivatives of 1/z for the nearest contributing triangle.
    /// NaNs mean background, whose instance tag is u32::MAX.
    pub inverse_depth_gradient: Vec<[f64; 2]>,
    pub statistics: Statistics,
    pub mip_usage: Vec<Vec<u64>>,
}
impl Frame {
    pub fn png_srgb(&self, b: &Budget) -> Result<Vec<u8>> {
        crate::png::encode_srgb(self.size, &self.color, b)
    }
    pub fn png(&self, b: &Budget) -> Result<Vec<u8>> {
        crate::png::encode(self.size, &self.color, b)
    }
    pub fn instance_bytes(&self, b: &Budget) -> Result<Vec<u8>> {
        let mut v = Vec::new();
        b.reserve(
            &mut v,
            self.instances
                .len()
                .checked_mul(4)
                .ok_or_else(|| err("RenderSizeOverflow", "instance output"))?,
            "instance output",
            0,
        )?;
        for x in &self.instances {
            v.extend_from_slice(&x.to_le_bytes());
        }
        Ok(v)
    }
    pub fn depth_bytes(&self, b: &Budget) -> Result<Vec<u8>> {
        let mut v = Vec::new();
        b.reserve(
            &mut v,
            self.depth
                .len()
                .checked_mul(4)
                .ok_or_else(|| err("RenderSizeOverflow", "depth output"))?,
            "depth output",
            0,
        )?;
        for x in &self.depth {
            v.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        Ok(v)
    }
}
fn err(code: &str, found: impl std::fmt::Display) -> Error {
    Error::new(code, "raster", 0, "valid bounded draw list", found)
}
#[derive(Clone, Copy, Default)]
struct Clip {
    a: [f64; 3],
    uv: [f64; 2],
    color: Option<[f64; 4]>,
}
impl Clip {
    fn mix(self, b: Self, t: f64) -> Self {
        Self {
            color: self
                .color
                .zip(b.color)
                .map(|(a, b)| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)),
            a: std::array::from_fn(|i| self.a[i] + (b.a[i] - self.a[i]) * t),
            uv: std::array::from_fn(|i| self.uv[i] + (b.uv[i] - self.uv[i]) * t),
        }
    }
}
#[derive(Clone, Copy)]
struct Transparent {
    draw: usize,
    vertices: [Clip; 3],
    distance: f64,
    ordinal: u64,
}
fn plane(v: Clip, i: usize, c: &Camera) -> f64 {
    let [l, r, t, b, n, f] = c.frustum;
    match i {
        0 => v.a[0] - n,
        1 => f - v.a[0],
        2 => v.a[2] - l * v.a[0],
        3 => r * v.a[0] - v.a[2],
        4 => v.a[1] - b * v.a[0],
        _ => t * v.a[0] - v.a[1],
    }
}
fn clipped(
    input: [Clip; 3],
    c: &Camera,
    mut emit: impl FnMut([Clip; 3]) -> Result<()>,
) -> Result<()> {
    let mut a = [Clip::default(); 16];
    a[..3].copy_from_slice(&input);
    let mut len = 3;
    for p in 0..6 {
        if len == 0 {
            return Ok(());
        }
        let mut output = [Clip::default(); 16];
        let mut n = 0;
        let mut prev = a[len - 1];
        let mut dp = plane(prev, p, c);
        if !dp.is_finite() {
            return Err(err("RenderClipPlaneNonFinite", dp));
        }
        for cur in a[..len].iter().copied() {
            let dc = plane(cur, p, c);
            if !dc.is_finite() {
                return Err(err("RenderClipPlaneNonFinite", dc));
            }
            if (dc >= 0.) != (dp >= 0.) {
                if n >= 16 {
                    return Err(err("RenderClipLimit", n));
                }
                output[n] = prev.mix(cur, dp / (dp - dc));
                n += 1;
            }
            if dc >= 0. {
                if n >= 16 {
                    return Err(err("RenderClipLimit", n));
                }
                output[n] = cur;
                n += 1;
            }
            prev = cur;
            dp = dc;
        }
        a = output;
        len = n;
    }
    for i in 1..len.saturating_sub(1) {
        emit([a[0], a[i], a[i + 1]])?;
    }
    Ok(())
}
fn edge(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
fn top_left(a: [f64; 2], b: [f64; 2]) -> bool {
    let dy = b[1] - a[1];
    dy < 0. || dy == 0. && b[0] > a[0]
}
fn blend(source: [f32; 4], destination: [f32; 4], alpha: Alpha) -> [f32; 4] {
    if alpha.enabled == 0 {
        return source;
    }
    fn factor(code: u32, i: usize, s: [f32; 4], d: [f32; 4]) -> f32 {
        match code {
            0 => 1.,
            1 => 0.,
            2 => s[i],
            3 => 1. - s[i],
            4 => d[i],
            5 => 1. - d[i],
            6 => s[3],
            7 => 1. - s[3],
            8 => d[3],
            9 => 1. - d[3],
            10 => {
                if i == 3 {
                    1.
                } else {
                    s[3].min(1. - d[3])
                }
            }
            _ => unreachable!("validated alpha function"),
        }
    }
    std::array::from_fn(|i| {
        (source[i] * factor(alpha.source, i, source, destination)
            + destination[i] * factor(alpha.destination, i, source, destination))
        .clamp(0., 1.)
    })
}
struct Raster<'a> {
    frame: Frame,
    opaque_depth: Vec<f32>,
    camera: &'a Camera,
    options: RenderOptions,
}
impl Raster<'_> {
    fn triangle(
        &mut self,
        mut v: [Clip; 3],
        draw: &Draw,
        tex: &Texture,
        overlay: bool,
    ) -> Result<()> {
        let mut p = [[0.; 2]; 3];
        for i in 0..3 {
            p[i] = self
                .camera
                .project_coordinates(v[i].a)
                .ok_or_else(|| err("RenderProjection", v[i].a[0]))?
                .pixel;
        }
        let mut area = edge(p[0], p[1], p[2]);
        if self.options.backface_cull && !draw.sidedness.double_sided()? && area > 0. {
            self.frame.statistics.culled_triangles += 1;
            return Ok(());
        }
        if area == 0. {
            return Ok(());
        }
        if area < 0. {
            p.swap(1, 2);
            v.swap(1, 2);
            area = -area;
        }
        if !area.is_finite() {
            return Err(err("RenderNonFiniteTriangle", area));
        }
        let weights_dx = [
            -(p[2][1] - p[1][1]) / area,
            -(p[0][1] - p[2][1]) / area,
            -(p[1][1] - p[0][1]) / area,
        ];
        let weights_dy = [
            (p[2][0] - p[1][0]) / area,
            (p[0][0] - p[2][0]) / area,
            (p[1][0] - p[0][0]) / area,
        ];
        let inverse_gradient = [weights_dx, weights_dy]
            .map(|weights| (0..3).map(|i| weights[i] / v[i].a[0]).sum::<f64>());
        let [w, h] = self.frame.size;
        let lo = std::array::from_fn::<_, 2, _>(|i| {
            p.iter()
                .map(|q| q[i])
                .fold(f64::INFINITY, f64::min)
                .ceil()
                .max(0.) as u32
        });
        let hi = std::array::from_fn::<_, 2, _>(|i| {
            p.iter()
                .map(|q| q[i])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .max(0.) as u32
        });
        // Pixel centers (x+.5,y+.5). Bounds deliberately include one extra lower pixel.
        for y in lo[1].saturating_sub(1)..hi[1].min(h) {
            for x in lo[0].saturating_sub(1)..hi[0].min(w) {
                let q = [x as f64 + 0.5, y as f64 + 0.5];
                let [vl, vr, vt, vb] = self.camera.viewport;
                if q[0] < vl * w as f64
                    || q[0] >= vr * w as f64
                    || q[1] < (1. - vt) * h as f64
                    || q[1] >= (1. - vb) * h as f64
                {
                    continue;
                }
                let e = [
                    edge(p[1], p[2], q),
                    edge(p[2], p[0], q),
                    edge(p[0], p[1], q),
                ];
                if (0..3)
                    .any(|i| e[i] < 0. || e[i] == 0. && !top_left(p[(i + 1) % 3], p[(i + 2) % 3]))
                {
                    continue;
                }
                let weight = e.map(|x| x / area);
                let inv = (0..3).map(|i| weight[i] / v[i].a[0]).sum::<f64>();
                if !inv.is_finite() || inv <= 0. {
                    return Err(err("RenderReciprocalDepth", inv));
                }
                let depth = (1. / inv) as f32;
                let at = y as usize * w as usize + x as usize;
                let stored = self.opaque_depth[at];
                if depth > stored || depth == stored && !overlay {
                    continue;
                }
                let uv = std::array::from_fn(|j| {
                    (0..3)
                        .map(|i| weight[i] * v[i].uv[j] / v[i].a[0])
                        .sum::<f64>()
                        / inv
                });
                if uv.iter().any(|x| !x.is_finite()) {
                    return Err(err("RenderNonFiniteUv", depth));
                }
                let derivative = |weights: [f64; 3], axis: usize| {
                    let numerator = (0..3)
                        .map(|i| weights[i] * v[i].uv[axis] / v[i].a[0])
                        .sum::<f64>();
                    let denominator = (0..3).map(|i| weights[i] / v[i].a[0]).sum::<f64>();
                    (numerator - uv[axis] * denominator) / inv
                };
                let dx = [derivative(weights_dx, 0), derivative(weights_dx, 1)];
                let dy = [derivative(weights_dy, 0), derivative(weights_dy, 1)];
                let footprint =
                    ((dx[0] * dy[1] - dx[1] * dy[0]) * tex.width as f64 * tex.height as f64).abs();
                if !footprint.is_finite() {
                    return Err(err("RenderTextureFootprint", footprint));
                }
                let lod = 0.5 * footprint.max(1.).log2();
                let (mut color, level) = tex.sample_lod(uv, self.options.bilinear, lod);
                self.frame.mip_usage[draw.texture][level] += 1;
                if v[0].color.is_some() {
                    for (channel, value) in color.iter_mut().enumerate() {
                        let mut component = 0.;
                        for i in 0..3 {
                            component += weight[i]
                                * v[i]
                                    .color
                                    .ok_or_else(|| err("RenderVertexColorLayout", i))?[channel]
                                / v[i].a[0];
                        }
                        let component = component / inv;
                        *value *= component as f32;
                    }
                }
                color[3] *= draw.alpha.opacity;
                if color.iter().any(|v| !v.is_finite()) {
                    return Err(err("RenderNonFiniteSurfaceColor", at));
                }
                self.frame.statistics.shaded_samples += 1;
                if !draw.alpha.passes(color[3]) {
                    continue;
                }
                if draw.alpha.enabled == 1 && color[3] == 0. && draw.alpha.source == 6 {
                    continue;
                }
                self.frame.color[at] = blend(color, self.frame.color[at], draw.alpha);
                if draw.alpha.enabled == 0 {
                    self.opaque_depth[at] = depth;
                }
                if depth < self.frame.depth[at] {
                    self.frame.depth[at] = depth;
                    self.frame.instances[at] = draw.instance;
                    self.frame.inverse_depth_gradient[at] = inverse_gradient;
                }
                self.frame.statistics.contributing_samples += 1;
            }
        }
        Ok(())
    }
    fn emit(&mut self, v: [Clip; 3], draw: &Draw, texture: &Texture, overlay: bool) -> Result<()> {
        clipped(v, self.camera, |t| {
            self.frame.statistics.clipped_triangles += 1;
            self.triangle(t, draw, texture, overlay)
        })
    }
}
pub fn render(
    list: &DrawList,
    camera: &Camera,
    options: RenderOptions,
    l: &Limits,
    b: &Budget,
) -> Result<Frame> {
    camera.validate()?;
    l.validate("render")?;
    if options.workers == 0 || options.workers > 4 || options.clear.iter().any(|x| !x.is_finite()) {
        return Err(err("InvalidRenderOptions", options.workers));
    }
    let count = fo4_core::mul(camera.size[0] as u64, camera.size[1] as u64, "frame", 0)?;
    fo4_core::limit(count, l.max_entries, "frame pixels", 0)?;
    let n = usize::try_from(count).map_err(|e| err("RenderSizeOverflow", e))?;
    let mut color = Vec::new();
    b.reserve(&mut color, n, "frame color", 0)?;
    color.resize(n, options.clear);
    let mut instances = Vec::new();
    b.reserve(&mut instances, n, "frame instances", 0)?;
    instances.resize(n, u32::MAX);
    let mut depth = Vec::new();
    b.reserve(&mut depth, n, "frame depth", 0)?;
    depth.resize(n, f32::INFINITY);
    let mut inverse_depth_gradient = Vec::new();
    b.reserve(
        &mut inverse_depth_gradient,
        n,
        "frame inverse depth planes",
        0,
    )?;
    inverse_depth_gradient.resize(n, [f64::NAN; 2]);
    let scratch = b.child();
    let mut opaque_depth = Vec::new();
    scratch.reserve(&mut opaque_depth, n, "opaque depth", 0)?;
    opaque_depth.resize(n, f32::INFINITY);
    let mut mip_usage = Vec::new();
    b.reserve(&mut mip_usage, list.textures.len(), "mip usage", 0)?;
    for t in &list.textures {
        let mut counts = Vec::new();
        b.reserve(&mut counts, t.mips.len() + 1, "mip usage", 0)?;
        counts.resize(t.mips.len() + 1, 0u64);
        mip_usage.push(counts);
    }
    let mut raster = Raster {
        frame: Frame {
            size: camera.size,
            color,
            instances,
            depth,
            inverse_depth_gradient,
            statistics: Statistics::default(),
            mip_usage,
        },
        opaque_depth,
        camera,
        options,
    };
    for tex in &list.textures {
        tex.validate()?;
    }
    list.validate_overlays()?;
    let mut transparent = Vec::new();
    let mut ordinal = 0;
    for (di, draw) in list.draws.iter().enumerate() {
        draw.alpha.validate()?;
        let mesh = list
            .meshes
            .get(draw.mesh)
            .ok_or_else(|| err("RenderMeshIndex", draw.mesh))?;
        let tex = list
            .textures
            .get(draw.texture)
            .ok_or_else(|| err("RenderTextureIndex", draw.texture))?;
        if draw
            .uv_offset
            .iter()
            .chain(&draw.uv_scale)
            .any(|x| !x.is_finite())
            || !draw.world.scale.is_finite()
            || draw
                .world
                .rotation
                .iter()
                .flatten()
                .chain(&draw.world.translation)
                .any(|x| !x.is_finite())
        {
            return Err(err("RenderNonFiniteTransform", di));
        }
        if let Some(colors) = &mesh.vertex_colors {
            if colors.len() != mesh.vertices.len()
                || colors.iter().flatten().any(|v| !v.is_finite())
            {
                return Err(err("RenderVertexColorLayout", colors.len()));
            }
        }
        for tri in &mesh.triangles {
            ordinal = fo4_core::add(ordinal, 1, "triangles", 0)?;
            fo4_core::limit(ordinal, l.max_entries, "triangles", 0)?;
            let mut v = [Clip::default(); 3];
            for i in 0..3 {
                let vertex = mesh
                    .vertices
                    .get(tri[i] as usize)
                    .ok_or_else(|| err("RenderVertexIndex", tri[i]))?;
                v[i] = Clip {
                    color: mesh
                        .vertex_colors
                        .as_ref()
                        .map(|c| c[tri[i] as usize].map(f64::from)),
                    a: camera.coordinates(draw.world.apply(vertex.position)),
                    uv: std::array::from_fn(|j| {
                        vertex.uv[j] * draw.uv_scale[j] + draw.uv_offset[j]
                    }),
                };
                if v[i].a.iter().chain(&v[i].uv).any(|x| !x.is_finite()) {
                    return Err(err("RenderNonFiniteVertex", tri[i]));
                }
            }
            raster.frame.statistics.input_triangles += 1;
            if draw.alpha.enabled == 1 {
                append(
                    &mut transparent,
                    Transparent {
                        draw: di,
                        vertices: v,
                        distance: v.iter().map(|p| p.a[0]).sum::<f64>() / 3.,
                        ordinal,
                    },
                    &scratch,
                )?;
            } else {
                raster.emit(v, draw, tex, list.is_overlay(di))?;
            }
        }
    }
    transparent.sort_unstable_by(|a, b| {
        b.distance
            .total_cmp(&a.distance)
            .then(a.ordinal.cmp(&b.ordinal))
    });
    for t in transparent {
        let d = &list.draws[t.draw];
        raster.emit(
            t.vertices,
            d,
            &list.textures[d.texture],
            list.is_overlay(t.draw),
        )?;
    }
    Ok(raster.frame)
}
