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
/// What [`render_lit`] lights: a frame's light (`fo4_scene::light`) and,
/// for each draw of the list, whether it is lit (`lit[i]`); the sky, when
/// the light has one, fills the background instead of the clear colour.
#[derive(Debug, Clone, Copy)]
pub struct Lighting<'a> {
    pub light: &'a fo4_scene::light::Light,
    pub lit: &'a [bool],
}
/// The light of one lit draw: the points whose sphere reaches the draw's
/// bounds (`Light::at_some` gives the same light with them alone).
struct DrawLight<'a> {
    light: &'a fo4_scene::light::Light,
    points: Vec<usize>,
}
/// Most points [`seen_lit_points`] returns.
pub const SEEN_POINTS: usize = 20_000;

/// The centroids of the triangles of the draws `lit` marks whose centroid
/// lies in front of `camera`, between its near and far distances and inside
/// its frame, each with the triangle's face normal turned toward the eye
/// (world axes), in draw and triangle order; with more than
/// [`SEEN_POINTS`], every k-th of them (k the smallest that keeps at most
/// that many). What `frame_exposure_seen_light_v1` measures the light on
/// (`fo4_scene::light::Light::expose_seen`): the CPU render and the viewer
/// call it with the same list, flags and camera.
pub fn seen_lit_points(
    list: &DrawList,
    lit: &[bool],
    camera: &Camera,
) -> Vec<([f64; 3], [f64; 3])> {
    seen_of(&lit_triangles(list, lit, usize::MAX), camera)
}

/// The triangles of the draws `lit` marks as their centroid and unit face
/// normal (world axes; a triangle without a normal left out), in draw and
/// triangle order; with more than `most`, every k-th of them (k the
/// smallest that keeps at most that many). What [`seen_of`] chooses from.
pub fn lit_triangles(list: &DrawList, lit: &[bool], most: usize) -> Vec<([f64; 3], [f64; 3])> {
    let mut all = Vec::new();
    for (di, draw) in list.draws.iter().enumerate() {
        if !lit.get(di).copied().unwrap_or(false) {
            continue;
        }
        let Some(mesh) = list.meshes.get(draw.mesh) else {
            continue;
        };
        for tri in &mesh.triangles {
            let Some(v) = tri
                .iter()
                .map(|i| {
                    mesh.vertices
                        .get(*i as usize)
                        .map(|v| draw.world.apply(v.position))
                })
                .collect::<Option<Vec<[f64; 3]>>>()
            else {
                continue;
            };
            let c: [f64; 3] = std::array::from_fn(|i| (v[0][i] + v[1][i] + v[2][i]) / 3.);
            let e1: [f64; 3] = std::array::from_fn(|i| v[1][i] - v[0][i]);
            let e2: [f64; 3] = std::array::from_fn(|i| v[2][i] - v[0][i]);
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let l = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            if !(l > 0. && l.is_finite()) {
                continue;
            }
            all.push((c, n.map(|x| x / l)));
        }
    }
    if all.len() <= most || most == 0 {
        return all;
    }
    let k = all.len().div_ceil(most);
    all.into_iter().step_by(k).collect()
}

/// Of `triangles` ([`lit_triangles`]), those whose centroid lies in front
/// of `camera`, between its near and far distances and inside its frame,
/// each normal turned toward the eye, in their order; with more than
/// [`SEEN_POINTS`], every k-th of them. [`seen_lit_points`] is this over
/// every lit triangle.
pub fn seen_of(triangles: &[([f64; 3], [f64; 3])], camera: &Camera) -> Vec<([f64; 3], [f64; 3])> {
    let [w, h] = camera.size;
    let mut all = Vec::new();
    for (c, n) in triangles {
        let a = camera.coordinates(*c);
        if !(a[0] >= camera.frustum[4] && a[0] <= camera.frustum[5]) {
            continue;
        }
        let Some(q) = camera.project_coordinates(a) else {
            continue;
        };
        if !(q.pixel[0] >= 0.
            && q.pixel[0] < f64::from(w)
            && q.pixel[1] >= 0.
            && q.pixel[1] < f64::from(h))
        {
            continue;
        }
        let to_eye: [f64; 3] = std::array::from_fn(|i| camera.translation[i] - c[i]);
        let n = if (0..3).map(|i| n[i] * to_eye[i]).sum::<f64>() < 0. {
            n.map(|x| -x)
        } else {
            *n
        };
        all.push((*c, n));
    }
    if all.len() <= SEEN_POINTS {
        return all;
    }
    let k = all.len().div_ceil(SEEN_POINTS);
    all.into_iter().step_by(k).collect()
}

/// The triangles of the draws `lit` marks as their three corners (world
/// axes, single precision), in draw and triangle order: what
/// [`seen_visible`] depth-tests.
pub fn lit_faces(list: &DrawList, lit: &[bool]) -> Vec<[[f32; 3]; 3]> {
    let mut all = Vec::new();
    each_lit_face(list, lit, |_, f| all.push(f));
    all
}

/// The draw (its index in `list.draws`) of each face [`lit_faces`] gives,
/// in the same order: what lets a caller leave out or move the faces of a
/// draw it hides or moves ([`seen_visible_of`]).
pub fn lit_face_draws(list: &DrawList, lit: &[bool]) -> Vec<u32> {
    let mut all = Vec::new();
    each_lit_face(list, lit, |di, _| {
        all.push(u32::try_from(di).unwrap_or(u32::MAX))
    });
    all
}

/// Each face of the draws `lit` marks, with its draw's index, in draw and
/// triangle order; a triangle naming a vertex its mesh lacks is skipped.
fn each_lit_face(list: &DrawList, lit: &[bool], mut each: impl FnMut(usize, [[f32; 3]; 3])) {
    for (di, draw) in list.draws.iter().enumerate() {
        if !lit.get(di).copied().unwrap_or(false) {
            continue;
        }
        let Some(mesh) = list.meshes.get(draw.mesh) else {
            continue;
        };
        for tri in &mesh.triangles {
            let mut f = [[0f32; 3]; 3];
            let mut ok = true;
            for (k, i) in tri.iter().enumerate() {
                match mesh.vertices.get(*i as usize) {
                    Some(v) => f[k] = draw.world.apply(v.position).map(|x| x as f32),
                    None => ok = false,
                }
            }
            if ok {
                each(di, f);
            }
        }
    }
}

/// The cells of the grid [`seen_visible`] lays over the frame, across and
/// down (one per 13.3 pixels of the reference frame).
pub const VISIBLE_GRID: [u32; 2] = [96, 54];

/// What the eye sees of `faces` ([`lit_faces`]): at the centre of each
/// cell of a `grid` over `camera`'s frame, the nearest face there between
/// the near and far distances (a depth test at the cells' centres, faces
/// cut at the near distance), as the point of the face on that line of
/// sight and the face's unit normal turned toward the eye, in row order;
/// cells where no face is are left out. What
/// `frame_exposure_seen_visible_v1` measures the light on, in place of
/// [`seen_of`]'s centroids in view (which count the faces hidden behind
/// others and a near wall's two triangles as two).
pub fn seen_visible(
    faces: &[[[f32; 3]; 3]],
    camera: &Camera,
    grid: [u32; 2],
) -> Vec<([f64; 3], [f64; 3])> {
    seen_visible_of(faces, &|_| true, &[], camera, grid)
        .into_iter()
        .map(|(_, p, n)| (p, n))
        .collect()
}

/// [`seen_visible`] over the faces of `faces` that `keep` keeps (by index)
/// and then every face of `extra`, each point with the index of its face
/// (an index of `extra` after `faces.len()`): a window that hides some
/// draws and moves others measures on what it draws (the moved draws'
/// faces given again, moved, in `extra`, their stored ones not kept).
/// With every face kept and no extra face the points are
/// [`seen_visible`]'s.
pub fn seen_visible_of(
    faces: &[[[f32; 3]; 3]],
    keep: &dyn Fn(usize) -> bool,
    extra: &[[[f32; 3]; 3]],
    camera: &Camera,
    grid: [u32; 2],
) -> Vec<(usize, [f64; 3], [f64; 3])> {
    let [gw, gh] = grid.map(|v| v.max(1) as usize);
    let [l, r, t, b, near, far] = camera.frustum;
    // Inverse forward distance of the nearest face at each cell (0: none).
    let mut depth = vec![0f64; gw * gh];
    let mut who = vec![u32::MAX; gw * gh];
    let to_grid = |a: [f64; 3]| -> [f64; 3] {
        let w = 1. / a[0];
        [
            (a[2] * w - l) / (r - l) * gw as f64,
            (1. - (a[1] * w - b) / (t - b)) * gh as f64,
            w,
        ]
    };
    let n_faces = faces.len();
    let all = faces
        .iter()
        .enumerate()
        .filter(|(fi, _)| keep(*fi))
        .chain(extra.iter().enumerate().map(|(k, f)| (n_faces + k, f)));
    for (fi, f) in all {
        let c: [[f64; 3]; 3] = f.map(|p| camera.coordinates(p.map(f64::from)));
        if c.iter().all(|a| a[0] < near) || c.iter().all(|a| a[0] > far) {
            continue;
        }
        // Cut at the near distance (at most four corners).
        let mut poly: Vec<[f64; 3]> = Vec::with_capacity(4);
        for k in 0..3 {
            let (p, q) = (c[k], c[(k + 1) % 3]);
            let (pin, qin) = (p[0] >= near, q[0] >= near);
            if pin {
                poly.push(p);
            }
            if pin != qin {
                let s = (near - p[0]) / (q[0] - p[0]);
                poly.push(std::array::from_fn(|i| p[i] + s * (q[i] - p[i])));
            }
        }
        if poly.len() < 3 || poly.iter().any(|a| !a.iter().all(|v| v.is_finite())) {
            continue;
        }
        let g: Vec<[f64; 3]> = poly.iter().map(|a| to_grid(*a)).collect();
        for k in 1..g.len() - 1 {
            let (p0, p1, p2) = (g[0], g[k], g[k + 1]);
            let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
            if area.abs() <= 1e-12 || !area.is_finite() {
                continue;
            }
            let lo_x = p0[0].min(p1[0]).min(p2[0]).floor().max(0.) as usize;
            let hi_x = (p0[0].max(p1[0]).max(p2[0]).ceil().min(gw as f64)).max(0.) as usize;
            let lo_y = p0[1].min(p1[1]).min(p2[1]).floor().max(0.) as usize;
            let hi_y = (p0[1].max(p1[1]).max(p2[1]).ceil().min(gh as f64)).max(0.) as usize;
            for y in lo_y..hi_y {
                for x in lo_x..hi_x {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let e = |a: [f64; 3], b: [f64; 3]| {
                        ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])) / area
                    };
                    let (w0, w1, w2) = (e(p1, p2), e(p2, p0), e(p0, p1));
                    if w0 < 0. || w1 < 0. || w2 < 0. {
                        continue;
                    }
                    let inv = w0 * p0[2] + w1 * p1[2] + w2 * p2[2];
                    if inv <= 1. / far || !inv.is_finite() {
                        continue;
                    }
                    let at = y * gw + x;
                    if inv > depth[at] {
                        depth[at] = inv;
                        who[at] = fi as u32;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for y in 0..gh {
        for x in 0..gw {
            let at = y * gw + x;
            let fi = who[at] as usize;
            let f = if fi < n_faces {
                faces.get(fi)
            } else {
                extra.get(fi - n_faces)
            };
            let Some(f) = f else {
                continue;
            };
            let u = l + (x as f64 + 0.5) / gw as f64 * (r - l);
            let v = b + (1. - (y as f64 + 0.5) / gh as f64) * (t - b);
            let fwd = 1. / depth[at];
            let a = [fwd, v * fwd, u * fwd];
            let p: [f64; 3] = std::array::from_fn(|i| {
                camera.translation[i]
                    + camera.scale * (0..3).map(|k| camera.rotation[k][i] * a[k]).sum::<f64>()
            });
            let v: [[f64; 3]; 3] = f.map(|q| q.map(f64::from));
            let e1: [f64; 3] = std::array::from_fn(|i| v[1][i] - v[0][i]);
            let e2: [f64; 3] = std::array::from_fn(|i| v[2][i] - v[0][i]);
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            if !(len > 0. && len.is_finite()) {
                continue;
            }
            let n = n.map(|x| x / len);
            let to_eye: [f64; 3] = std::array::from_fn(|i| camera.translation[i] - p[i]);
            let n = if (0..3).map(|i| n[i] * to_eye[i]).sum::<f64>() < 0. {
                n.map(|x| -x)
            } else {
                n
            };
            out.push((fi, p, n));
        }
    }
    out
}

/// The sine of the elevation of the view direction through the centre of
/// pixel `(x, y)` of `camera` (game axes, Z up).
pub fn pixel_elevation(camera: &Camera, x: u32, y: u32) -> f64 {
    let [l, r, t, b, _, _] = camera.frustum;
    let [w, h] = camera.size;
    let u = (f64::from(x) + 0.5) / f64::from(w);
    let v = 1. - (f64::from(y) + 0.5) / f64::from(h);
    let d_camera = [1., b + v * (t - b), l + u * (r - l)];
    let d: [f64; 3] =
        std::array::from_fn(|i| (0..3).map(|k| d_camera[k] * camera.rotation[k][i]).sum());
    d[2] / d.iter().map(|x| x * x).sum::<f64>().sqrt()
}
#[derive(Clone, Copy, Default)]
struct Clip {
    a: [f64; 3],
    uv: [f64; 2],
    color: Option<[f64; 4]>,
    /// The vertex normal in camera axes, for a draw with a falloff.
    normal: Option<[f64; 3]>,
}
impl Clip {
    fn mix(self, b: Self, t: f64) -> Self {
        Self {
            normal: self
                .normal
                .zip(b.normal)
                .map(|(a, b)| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)),
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
    // The blend reads the source alpha within 0 to 1: an opacity above 1
    // reaches it clamped, after the alpha test saw the product
    // (`blend_alpha_clamped_before_blend_v1`, gated by the draw build; a
    // no-op at or below 1, as an 8-bit target clamps a fragment's output).
    let source = [source[0], source[1], source[2], source[3].clamp(0., 1.)];
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
        palette: Option<&Texture>,
        overlay: bool,
        lit: Option<&DrawLight<'_>>,
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
        // A lit draw's normal: the triangle's face normal turned toward
        // the eye (the origin of camera axes), in game axes for the light.
        let face = match lit {
            Some(_) => {
                let e1: [f64; 3] = std::array::from_fn(|i| v[1].a[i] - v[0].a[i]);
                let e2: [f64; 3] = std::array::from_fn(|i| v[2].a[i] - v[0].a[i]);
                let mut n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let length = n.iter().map(|x| x * x).sum::<f64>().sqrt();
                if length <= 0. || !length.is_finite() {
                    return Ok(());
                }
                if (0..3).map(|i| -n[i] * v[0].a[i]).sum::<f64>() < 0. {
                    n = n.map(|x| -x);
                }
                let n_camera = n.map(|x| x / length);
                Some(std::array::from_fn::<f64, 3, _>(|i| {
                    (0..3)
                        .map(|k| n_camera[k] * self.camera.rotation[k][i])
                        .sum()
                }))
            }
            None => None,
        };
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
                let mut vertex = None;
                if v[0].color.is_some() {
                    let mut c = [0f32; 4];
                    for (channel, value) in c.iter_mut().enumerate() {
                        let mut component = 0.;
                        for i in 0..3 {
                            component += weight[i]
                                * v[i]
                                    .color
                                    .ok_or_else(|| err("RenderVertexColorLayout", i))?[channel]
                                / v[i].a[0];
                        }
                        *value = (component / inv) as f32;
                    }
                    vertex = Some(c);
                }
                // The palette lookups read the texel and the surface's
                // alpha before the palette (opacity times vertex alpha).
                let mut palette_alpha = None;
                if let (Some(p), Some(pt)) = (draw.palette, palette) {
                    let before = draw.alpha.opacity * vertex.map_or(1., |c| c[3]);
                    let [at_colour, at_alpha] = crate::Palette::coordinates(color, before);
                    if p.colour {
                        let looked = pt.sample_clamped(at_colour);
                        color[..3].copy_from_slice(&looked[..3]);
                    }
                    if p.alpha {
                        palette_alpha = Some(pt.sample_clamped(at_alpha)[3]);
                    }
                }
                if let Some(c) = vertex {
                    for (value, component) in color.iter_mut().zip(c) {
                        *value *= component;
                    }
                }
                // An effect's colour the clock drives multiplies the RGB
                // where its tint would have been in the vertex colours.
                if let Some(t) = draw.colour {
                    for (value, component) in color.iter_mut().zip(t) {
                        *value *= component;
                    }
                }
                // A lit draw: the colour times the light at the surface
                // point (game axes), then the fog by distance from the eye
                // (`light_sum_clamped_v1`).
                if let (Some(d), Some(face_n)) = (lit, face) {
                    let point: [f64; 3] = std::array::from_fn(|j| {
                        (0..3)
                            .map(|i| weight[i] * v[i].a[j] / v[i].a[0])
                            .sum::<f64>()
                            / inv
                    });
                    // With the mesh's vertex normals kept for a lit draw
                    // (`light_vertex_normals_v1`): the normal interpolated
                    // by reciprocal depth, turned toward the eye.
                    let mut n = face_n;
                    if draw.falloff.is_none() && v.iter().all(|c| c.normal.is_some()) {
                        let mut m = [0.; 3];
                        for i in 0..3 {
                            let normal = v[i].normal.unwrap_or_default();
                            for j in 0..3 {
                                m[j] += weight[i] * normal[j] / v[i].a[0];
                            }
                        }
                        let length = m.iter().map(|x| x * x).sum::<f64>().sqrt();
                        if length > 0. && length.is_finite() {
                            if (0..3).map(|j| -m[j] * point[j]).sum::<f64>() < 0. {
                                m = m.map(|x| -x);
                            }
                            let m = m.map(|x| x / length);
                            n = std::array::from_fn(|i| {
                                (0..3).map(|k| m[k] * self.camera.rotation[k][i]).sum()
                            });
                        }
                    }
                    let world: [f64; 3] = std::array::from_fn(|i| {
                        self.camera.translation[i]
                            + (0..3)
                                .map(|k| point[k] * self.camera.rotation[k][i])
                                .sum::<f64>()
                    });
                    if d.light.lit {
                        let l = d.light.at_some(world, n, d.points.iter().copied());
                        for (value, component) in color.iter_mut().zip(l) {
                            *value *= component;
                        }
                        // `frame_exposure_seen_light_v1`.
                        if let Some(k) = d.light.exposure.scale {
                            for value in &mut color[..3] {
                                *value *= k;
                            }
                        }
                    }
                    if let Some(fog) = &d.light.fog {
                        let distance = point.iter().map(|x| x * x).sum::<f64>().sqrt();
                        let f = fog.fraction(distance);
                        for (value, component) in color.iter_mut().zip(fog.colour) {
                            *value = *value * (1. - f) + component * f;
                        }
                    }
                    // `image_space_hdr_white_v1`, `image_space_cinematic_tint_v1`.
                    let x = &d.light.exposure;
                    if x.white.is_some() || x.cinematic.is_some() || x.tint.is_some() {
                        let mapped = x.map([color[0], color[1], color[2]]);
                        color[..3].copy_from_slice(&mapped);
                    }
                }
                color[3] *= draw.alpha.opacity;
                if let Some(a) = palette_alpha {
                    color[3] = a;
                }
                if let Some(falloff) = draw.falloff {
                    // The normal and the surface point, interpolated by
                    // reciprocal depth like UV; the eye is the origin of
                    // camera axes.
                    let mut n = [0.; 3];
                    let mut point = [0.; 3];
                    for i in 0..3 {
                        let normal = v[i].normal.ok_or_else(|| err("RenderFalloffNormals", i))?;
                        for j in 0..3 {
                            n[j] += weight[i] * normal[j] / v[i].a[0];
                            point[j] += weight[i] * v[i].a[j] / v[i].a[0];
                        }
                    }
                    let dot: f64 = (0..3).map(|j| -n[j] * point[j]).sum();
                    let length = (0..3).map(|j| n[j] * n[j]).sum::<f64>().sqrt()
                        * (0..3).map(|j| point[j] * point[j]).sum::<f64>().sqrt();
                    let cosine = dot / length;
                    if !cosine.is_finite() {
                        return Err(err("RenderFalloffNormals", at));
                    }
                    color[3] *= falloff.factor(cosine.clamp(-1., 1.)) as f32;
                }
                // The soft fade (`effect_soft_depth_fade_linear_v1`): the
                // gap from this sample to the opaque depth behind it over
                // the soft falloff depth, clamped to 0..1 (nothing behind:
                // no fade); before the alpha test, as the falloff.
                if let Some(soft) = draw.soft_depth.filter(|d| *d > 0.) {
                    let gap = f64::from(stored) - f64::from(depth);
                    color[3] *= (gap / f64::from(soft)).clamp(0., 1.) as f32;
                }
                if color.iter().any(|v| !v.is_finite()) {
                    return Err(err("RenderNonFiniteSurfaceColor", at));
                }
                // The frame holds colour within 0 to 1: a colour above 1
                // (an effect's base colour times a scale above 1) is
                // clamped before the test and the blend, as an 8-bit
                // target clamps a fragment's output
                // (`effect_colour_clamped_before_blend_v1`).
                for value in &mut color[..3] {
                    *value = value.clamp(0., 1.);
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
    fn emit(
        &mut self,
        v: [Clip; 3],
        draw: &Draw,
        texture: &Texture,
        palette: Option<&Texture>,
        overlay: bool,
        lit: Option<&DrawLight<'_>>,
    ) -> Result<()> {
        clipped(v, self.camera, |t| {
            self.frame.statistics.clipped_triangles += 1;
            self.triangle(t, draw, texture, palette, overlay, lit)
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
    render_lit(list, camera, options, None, l, b)
}
/// [`render`] with a frame's light: the draws `lighting.lit` marks are lit
/// and fogged, and the light's sky fills the background
/// (`fo4_scene::light`, `spec/light.md`). With `None` it is [`render`].
pub fn render_lit(
    list: &DrawList,
    camera: &Camera,
    options: RenderOptions,
    lighting: Option<Lighting<'_>>,
    l: &Limits,
    b: &Budget,
) -> Result<Frame> {
    camera.validate()?;
    if let Some(g) = &lighting {
        if g.lit.len() != list.draws.len() || camera.scale != 1. {
            return Err(err(
                "RenderLighting",
                format!(
                    "{} lit flags for {} draws, camera scale {}",
                    g.lit.len(),
                    list.draws.len(),
                    camera.scale
                ),
            ));
        }
    }
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
    // The sky fills the background by each pixel's view direction.
    if let Some(sky) = lighting.and_then(|g| g.light.sky) {
        let w = camera.size[0] as usize;
        for (at, c) in color.iter_mut().enumerate() {
            let s = pixel_elevation(camera, (at % w) as u32, (at / w) as u32) as f32;
            let [r, g, bl] = sky.colour(s);
            *c = [r, g, bl, options.clear[3]];
        }
    }
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
    if let Some(i) = list.blocks.keys().next() {
        // Stored blocks are for a GPU consumer; this reference samples
        // only decoded texels.
        return Err(err("CompressedTextureNotDecoded", i));
    }
    for tex in &list.textures {
        tex.validate()?;
    }
    list.validate_overlays()?;
    let mut transparent = Vec::new();
    let mut ordinal = 0;
    // The light of each lit draw, kept for its blended triangles.
    let mut draw_lights: Vec<Option<DrawLight<'_>>> = Vec::new();
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
        let palette = match draw.palette {
            Some(p) => Some(
                list.textures
                    .get(p.texture)
                    .ok_or_else(|| err("RenderTextureIndex", p.texture))?,
            ),
            None => None,
        };
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
        if draw
            .colour
            .is_some_and(|c| c.iter().any(|v| !v.is_finite() || *v < 0.))
        {
            return Err(err("RenderDrawColour", di));
        }
        if let Some(colors) = &mesh.vertex_colors {
            if colors.len() != mesh.vertices.len()
                || colors.iter().flatten().any(|v| !v.is_finite())
            {
                return Err(err("RenderVertexColorLayout", colors.len()));
            }
        }
        // A falloff reads the mesh's normals, in camera axes: the draw's
        // rotation (rows, as `Transform::apply`), then the camera's.
        let normals = match draw.falloff {
            Some(f) => {
                f.validate()?;
                let n = mesh
                    .normals
                    .as_ref()
                    .filter(|n| {
                        n.len() == mesh.vertices.len() && n.iter().flatten().all(|v| v.is_finite())
                    })
                    .ok_or_else(|| err("RenderFalloffNormals", di))?;
                Some(n)
            }
            // A lit draw whose mesh keeps normals (only under
            // `BuildOptions::vertex_normals`: otherwise only falloff draws,
            // never lit, keep them) is lit with them.
            None if lighting.is_some_and(|g| g.lit.get(di).copied().unwrap_or(false)) => {
                mesh.normals.as_ref().filter(|n| {
                    n.len() == mesh.vertices.len() && n.iter().flatten().all(|v| v.is_finite())
                })
            }
            None => None,
        };
        let to_camera = |n: [f64; 3]| -> [f64; 3] {
            let w: [f64; 3] =
                std::array::from_fn(|i| (0..3).map(|j| draw.world.rotation[j][i] * n[j]).sum());
            std::array::from_fn(|i| (0..3).map(|j| camera.rotation[i][j] * w[j]).sum())
        };
        // A lit draw's points: those whose sphere reaches the bounds of
        // its vertices in game axes.
        let draw_light = match lighting {
            Some(g) if g.lit[di] && (g.light.lit || g.light.fog.is_some()) => {
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for v in &mesh.vertices {
                    let p = draw.world.apply(v.position);
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let mut points = Vec::new();
                for (k, p) in g.light.points.iter().enumerate() {
                    let d2: f64 = (0..3)
                        .map(|i| {
                            let e = (lo[i] - p.position[i]).max(p.position[i] - hi[i]).max(0.);
                            e * e
                        })
                        .sum();
                    if d2 < p.radius * p.radius {
                        scratch.reserve(&mut points, 1, "draw lights", 0)?;
                        points.push(k);
                    }
                }
                Some(DrawLight {
                    light: g.light,
                    points,
                })
            }
            _ => None,
        };
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
                    normal: normals.map(|n| to_camera(n[tri[i] as usize])),
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
                raster.emit(
                    v,
                    draw,
                    tex,
                    palette,
                    list.is_overlay(di),
                    draw_light.as_ref(),
                )?;
            }
        }
        if lighting.is_some() {
            scratch.reserve(&mut draw_lights, 1, "draw lights", 0)?;
            draw_lights.push(draw_light);
        }
    }
    // The terrain's texture passes (overlays) first: they only add to the
    // terrain's own colour where the terrain is the nearest opaque surface,
    // so every other blended surface (water among them) blends over the
    // finished terrain, whatever its distance; then back to front.
    transparent.sort_unstable_by(|a, b| {
        list.is_overlay(b.draw)
            .cmp(&list.is_overlay(a.draw))
            .then(b.distance.total_cmp(&a.distance))
            .then(a.ordinal.cmp(&b.ordinal))
    });
    for t in transparent {
        let d = &list.draws[t.draw];
        raster.emit(
            t.vertices,
            d,
            &list.textures[d.texture],
            d.palette.map(|p| &list.textures[p.texture]),
            list.is_overlay(t.draw),
            draw_lights.get(t.draw).and_then(Option::as_ref),
        )?;
    }
    Ok(raster.frame)
}
