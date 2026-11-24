//! A headless picture of a movie's first frame: the static shapes the
//! root's first frame places, with the first frame of every sprite it
//! places, filled by a minimal scanline fill (`spec/ui.md`, First frame).
//!
//! Presentation only, never a claim about how the game draws the screen:
//! solid fills are drawn as stored, a gradient as the mean of its colours,
//! a bitmap fill as mid grey; lines, texts, bitmaps placed on their own,
//! morph shapes, buttons, filters, blend modes and masks are left out and
//! counted. Fill semantics follow SWF19 (a fill style on each side of an
//! edge, closed areas filled by the non-zero rule over the edges each
//! style bounds); curves are flattened into eight segments.
use crate::{walk, Limits, Movie};
use fo4_core::{Budget, Error, Result};
use std::collections::BTreeMap;

/// An image, straight (not premultiplied) RGBA in 0..1, rows top down.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
}

/// What the picture holds and what it leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub shapes_drawn: u64,
    pub fills_solid: u64,
    pub fills_gradient_mean: u64,
    pub fills_bitmap_grey: u64,
    pub lines_left_out: u64,
    pub sprites_entered: u64,
    /// Placed characters that are not shapes or sprites (texts, bitmaps,
    /// morph shapes, buttons, video), left out.
    pub others_left_out: u64,
    /// Placements whose character the movie never defines.
    pub undefined: u64,
    /// Sprites not entered for the nesting limit.
    pub depth_cut: u64,
    /// Placements that mask others (a clip depth), left out with no mask
    /// applied.
    pub masks_left_out: u64,
    /// Placements not visited once the visit cap was reached.
    pub visits_cut: u64,
}

#[derive(Debug, Clone, Copy)]
struct Affine([f64; 6]);
impl Affine {
    const ID: Self = Affine([1., 0., 0., 1., 0., 0.]);
    fn of(m: &swf::Matrix) -> Self {
        Affine([
            f64::from(m.a.to_f32()),
            f64::from(m.b.to_f32()),
            f64::from(m.c.to_f32()),
            f64::from(m.d.to_f32()),
            f64::from(m.tx.get()),
            f64::from(m.ty.get()),
        ])
    }
    /// self after other: x -> self(other(x)).
    fn then(self, inner: Affine) -> Affine {
        let [a, b, c, d, tx, ty] = self.0;
        let [e, f, g, h, ux, uy] = inner.0;
        Affine([
            a * e + c * f,
            b * e + d * f,
            a * g + c * h,
            b * g + d * h,
            a * ux + c * uy + tx,
            b * ux + d * uy + ty,
        ])
    }
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, tx, ty] = self.0;
        (a * x + c * y + tx, b * x + d * y + ty)
    }
}

#[derive(Debug, Clone, Copy)]
struct Tint {
    mul: [f32; 4],
    add: [f32; 4],
}
impl Tint {
    const ID: Self = Tint {
        mul: [1.; 4],
        add: [0.; 4],
    };
    fn of(c: &swf::ColorTransform) -> Self {
        Tint {
            mul: [
                c.r_multiply.to_f32(),
                c.g_multiply.to_f32(),
                c.b_multiply.to_f32(),
                c.a_multiply.to_f32(),
            ],
            add: [
                f32::from(c.r_add) / 255.,
                f32::from(c.g_add) / 255.,
                f32::from(c.b_add) / 255.,
                f32::from(c.a_add) / 255.,
            ],
        }
    }
    fn then(self, inner: Tint) -> Tint {
        let mut t = Tint::ID;
        for k in 0..4 {
            t.mul[k] = self.mul[k] * inner.mul[k];
            t.add[k] = self.mul[k] * inner.add[k] + self.add[k];
        }
        t
    }
    fn apply(&self, c: [f32; 4]) -> [f32; 4] {
        let mut o = [0.; 4];
        for k in 0..4 {
            o[k] = (c[k] * self.mul[k] + self.add[k]).clamp(0., 1.);
        }
        o
    }
}

#[derive(Debug, Clone)]
struct Place {
    character: u16,
    mask: bool,
    matrix: Affine,
    tint: Tint,
}

#[derive(Default)]
struct Library {
    shapes: BTreeMap<u16, swf::Shape>,
    others: BTreeMap<u16, ()>,
    /// First frame display list of each sprite and of the root (`None`).
    frames: BTreeMap<Option<u16>, BTreeMap<u16, Place>>,
    done: BTreeMap<Option<u16>, bool>,
}

fn colour(c: &swf::Color) -> [f32; 4] {
    [
        f32::from(c.r) / 255.,
        f32::from(c.g) / 255.,
        f32::from(c.b) / 255.,
        f32::from(c.a) / 255.,
    ]
}

/// The colour a fill draws with, and its kind (0 solid, 1 gradient mean,
/// 2 bitmap grey).
fn fill_colour(f: &swf::FillStyle) -> ([f32; 4], usize) {
    let mean = |g: &swf::Gradient| {
        let mut s = [0f32; 4];
        for r in &g.records {
            let c = colour(&r.color);
            for k in 0..4 {
                s[k] += c[k];
            }
        }
        let n = g.records.len().max(1) as f32;
        s.map(|v| v / n)
    };
    match f {
        swf::FillStyle::Color(c) => (colour(c), 0),
        swf::FillStyle::LinearGradient(g) | swf::FillStyle::RadialGradient(g) => (mean(g), 1),
        swf::FillStyle::FocalGradient { gradient, .. } => (mean(gradient), 1),
        swf::FillStyle::Bitmap { .. } => ([0.5, 0.5, 0.5, 1.], 2),
    }
}

/// Edges in twips, before the placement: (x0, y0, x1, y1) per fill style.
fn shape_edges(sh: &swf::Shape) -> (Vec<swf::FillStyle>, Vec<Vec<[f64; 4]>>, u64) {
    let mut fills: Vec<swf::FillStyle> = sh.styles.fill_styles.clone();
    let mut lines = sh.styles.line_styles.len() as u64;
    let mut edges: Vec<Vec<[f64; 4]>> = vec![Vec::new(); fills.len()];
    let (mut base, mut f0, mut f1) = (0usize, 0u32, 0u32);
    let (mut x, mut y) = (0f64, 0f64);
    let push = |edges: &mut Vec<Vec<[f64; 4]>>, f0: u32, f1: u32, base: usize, s: [f64; 4]| {
        // Fill style 1 lies on the edge's right (SWF19): kept in the
        // edge's direction; fill style 0 on its left: reversed.
        if f1 > 0 {
            if let Some(e) = edges.get_mut(base + f1 as usize - 1) {
                e.push(s);
            }
        }
        if f0 > 0 {
            if let Some(e) = edges.get_mut(base + f0 as usize - 1) {
                e.push([s[2], s[3], s[0], s[1]]);
            }
        }
    };
    for r in &sh.shape {
        match r {
            swf::ShapeRecord::StyleChange(c) => {
                if let Some(n) = &c.new_styles {
                    base = fills.len();
                    fills.extend(n.fill_styles.iter().cloned());
                    edges.resize(fills.len(), Vec::new());
                    lines += n.line_styles.len() as u64;
                    f0 = 0;
                    f1 = 0;
                }
                if let Some(p) = &c.move_to {
                    x = f64::from(p.x.get());
                    y = f64::from(p.y.get());
                }
                if let Some(v) = c.fill_style_0 {
                    f0 = v;
                }
                if let Some(v) = c.fill_style_1 {
                    f1 = v;
                }
            }
            swf::ShapeRecord::StraightEdge { delta } => {
                let (nx, ny) = (x + f64::from(delta.dx.get()), y + f64::from(delta.dy.get()));
                push(&mut edges, f0, f1, base, [x, y, nx, ny]);
                (x, y) = (nx, ny);
            }
            swf::ShapeRecord::CurvedEdge {
                control_delta,
                anchor_delta,
            } => {
                let (cx, cy) = (
                    x + f64::from(control_delta.dx.get()),
                    y + f64::from(control_delta.dy.get()),
                );
                let (ax, ay) = (
                    cx + f64::from(anchor_delta.dx.get()),
                    cy + f64::from(anchor_delta.dy.get()),
                );
                let (mut px, mut py) = (x, y);
                for i in 1..=8 {
                    let t = f64::from(i) / 8.;
                    let u = 1. - t;
                    let qx = u * u * x + 2. * u * t * cx + t * t * ax;
                    let qy = u * u * y + 2. * u * t * cy + t * t * ay;
                    push(&mut edges, f0, f1, base, [px, py, qx, qy]);
                    (px, py) = (qx, qy);
                }
                (x, y) = (ax, ay);
            }
        }
    }
    (fills, edges, lines)
}

/// Rows sampled per pixel.
const SUB: usize = 4;

fn fill(img: &mut Image, edges: &[[f64; 4]], c: [f32; 4]) {
    if edges.is_empty() || c[3] <= 0. {
        return;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let mut ymin = f64::MAX;
    let mut ymax = f64::MIN;
    for e in edges {
        ymin = ymin.min(e[1]).min(e[3]);
        ymax = ymax.max(e[1]).max(e[3]);
    }
    let r0 = (ymin.floor().max(0.)) as usize;
    let r1 = (ymax.ceil().min(h as f64)) as usize;
    let mut cover = vec![0f32; w];
    let mut xs: Vec<(f64, i32)> = Vec::new();
    for row in r0..r1 {
        cover.iter_mut().for_each(|v| *v = 0.);
        for s in 0..SUB {
            let sy = row as f64 + (s as f64 + 0.5) / SUB as f64;
            xs.clear();
            for e in edges {
                let (y0, y1) = (e[1], e[3]);
                if (y0 <= sy && sy < y1) || (y1 <= sy && sy < y0) {
                    let t = (sy - y0) / (y1 - y0);
                    xs.push((e[0] + t * (e[2] - e[0]), if y1 > y0 { 1 } else { -1 }));
                }
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for k in 0..xs.len() {
                wind += xs[k].1;
                if wind != 0 && k + 1 < xs.len() {
                    let a = xs[k].0.max(0.);
                    let b = xs[k + 1].0.min(w as f64);
                    let mut px = a.ceil() as i64 - 1;
                    // Pixel centres inside the span.
                    px = px.max(0);
                    while (px as f64) < b {
                        let centre = px as f64 + 0.5;
                        if centre >= a && centre < b && (px as usize) < w {
                            cover[px as usize] += 1. / SUB as f32;
                        }
                        px += 1;
                    }
                }
            }
        }
        for (x, cv) in cover.iter().enumerate() {
            if *cv <= 0. {
                continue;
            }
            let a = c[3] * cv.min(1.);
            let p = &mut img.pixels[row * w + x];
            let out_a = a + p[3] * (1. - a);
            if out_a > 0. {
                for k in 0..3 {
                    p[k] = (c[k] * a + p[k] * p[3] * (1. - a)) / out_a;
                }
            }
            p[3] = out_a;
        }
    }
}

struct Painter<'l> {
    lib: &'l Library,
    view: Affine,
    img: Image,
    counts: Counts,
    max_depth: u32,
    visits: u64,
    max_visits: u64,
}
impl Painter<'_> {
    fn draw_list(&mut self, timeline: Option<u16>, m: Affine, t: Tint, depth: u32) {
        let Some(list) = self.lib.frames.get(&timeline) else {
            return;
        };
        for p in list.values() {
            if self.visits >= self.max_visits {
                self.counts.visits_cut += 1;
                continue;
            }
            self.visits += 1;
            if p.mask {
                self.counts.masks_left_out += 1;
                continue;
            }
            let mm = m.then(p.matrix);
            let tt = t.then(p.tint);
            if let Some(sh) = self.lib.shapes.get(&p.character) {
                self.shape(sh, mm, tt);
            } else if self.lib.frames.contains_key(&Some(p.character)) {
                if depth >= self.max_depth {
                    self.counts.depth_cut += 1;
                } else {
                    self.counts.sprites_entered += 1;
                    self.draw_list(Some(p.character), mm, tt, depth + 1);
                }
            } else if self.lib.others.contains_key(&p.character) {
                self.counts.others_left_out += 1;
            } else {
                self.counts.undefined += 1;
            }
        }
    }
    fn shape(&mut self, sh: &swf::Shape, m: Affine, t: Tint) {
        self.counts.shapes_drawn += 1;
        let (fills, edges, lines) = shape_edges(sh);
        self.counts.lines_left_out += lines;
        let to = self.view.then(m);
        for (f, es) in fills.iter().zip(&edges) {
            if es.is_empty() {
                continue;
            }
            let (c, kind) = fill_colour(f);
            match kind {
                0 => self.counts.fills_solid += 1,
                1 => self.counts.fills_gradient_mean += 1,
                _ => self.counts.fills_bitmap_grey += 1,
            }
            let placed: Vec<[f64; 4]> = es
                .iter()
                .map(|e| {
                    let (a, b) = to.apply(e[0], e[1]);
                    let (c, d) = to.apply(e[2], e[3]);
                    [a, b, c, d]
                })
                .collect();
            fill(&mut self.img, &placed, t.apply(c));
        }
    }
}

/// The root's first frame at `scale` pixels per stage pixel (a stage pixel
/// is 20 twips), over the movie's background colour (transparent without
/// one). `max_pixels` caps the image.
pub fn first_frame(
    m: &Movie,
    scale: f64,
    max_pixels: u64,
    l: &Limits,
    b: &Budget,
) -> Result<(Image, Counts)> {
    let name = m.name.as_str();
    if !(scale.is_finite() && scale > 0.) {
        return Err(Error::new("BadScale", name, 0, "finite scale > 0", scale));
    }
    let wpx = (f64::from(m.stage[1] - m.stage[0]) / 20. * scale).ceil();
    let hpx = (f64::from(m.stage[3] - m.stage[2]) / 20. * scale).ceil();
    if !(wpx >= 1. && hpx >= 1.) {
        return Err(Error::new(
            "EmptyStage",
            name,
            0,
            "stage of at least one pixel",
            format!("{wpx} x {hpx}"),
        ));
    }
    let (w, h) = (wpx as u64, hpx as u64);
    fo4_core::limit(fo4_core::mul(w, h, name, 0)?, max_pixels, name, 0)?;
    let mut lib = Library::default();
    let mut count = 0u64;
    walk(
        &m.data,
        m.version,
        name,
        l,
        &mut count,
        &mut |v| {
            use swf::Tag as T;
            match v.tag {
                T::DefineShape(s) => {
                    lib.shapes.insert(s.id, (**s).clone());
                }
                T::DefineSprite(s) => {
                    lib.frames.entry(Some(s.id)).or_default();
                }
                T::DefineEditText(t) => {
                    lib.others.insert(t.id(), ());
                }
                T::DefineText(t) | T::DefineText2(t) => {
                    lib.others.insert(t.id, ());
                }
                T::DefineMorphShape(s) => {
                    lib.others.insert(s.id, ());
                }
                T::DefineBitsLossless(x) => {
                    lib.others.insert(x.id, ());
                }
                T::DefineBits { id, .. } | T::DefineBitsJpeg2 { id, .. } => {
                    lib.others.insert(*id, ());
                }
                T::DefineBitsJpeg3(x) => {
                    lib.others.insert(x.id, ());
                }
                T::DefineButton(x) | T::DefineButton2(x) => {
                    lib.others.insert(x.id, ());
                }
                T::ShowFrame => {
                    lib.done.insert(v.sprite, true);
                }
                T::PlaceObject(p) if !lib.done.get(&v.sprite).copied().unwrap_or(false) => {
                    let list = lib.frames.entry(v.sprite).or_default();
                    match p.action {
                        swf::PlaceObjectAction::Place(id) | swf::PlaceObjectAction::Replace(id) => {
                            let old = list.get(&p.depth).cloned();
                            list.insert(
                                p.depth,
                                Place {
                                    character: id,
                                    mask: p.clip_depth.is_some(),
                                    matrix: p
                                        .matrix
                                        .as_ref()
                                        .map(Affine::of)
                                        .or(old.as_ref().map(|o| o.matrix))
                                        .unwrap_or(Affine::ID),
                                    tint: p
                                        .color_transform
                                        .as_ref()
                                        .map(Tint::of)
                                        .or(old.as_ref().map(|o| o.tint))
                                        .unwrap_or(Tint::ID),
                                },
                            );
                        }
                        swf::PlaceObjectAction::Modify => {
                            if let Some(o) = list.get_mut(&p.depth) {
                                if let Some(mx) = &p.matrix {
                                    o.matrix = Affine::of(mx);
                                }
                                if let Some(c) = &p.color_transform {
                                    o.tint = Tint::of(c);
                                }
                                if p.clip_depth.is_some() {
                                    o.mask = true;
                                }
                            }
                        }
                        swf::PlaceObjectAction::None => {}
                    }
                }
                T::RemoveObject(r) if !lib.done.get(&v.sprite).copied().unwrap_or(false) => {
                    if let Some(list) = lib.frames.get_mut(&v.sprite) {
                        list.remove(&r.depth);
                    }
                }
                _ => {}
            }
        },
        &mut |_| {},
    )?;
    let n = (w * h) as usize;
    let mut pixels = Vec::new();
    b.reserve(&mut pixels, n, name, 0)?;
    let bg = m.background.map_or([0.; 4], |c| {
        [
            f32::from(c[0]) / 255.,
            f32::from(c[1]) / 255.,
            f32::from(c[2]) / 255.,
            f32::from(c[3]) / 255.,
        ]
    });
    pixels.resize(n, bg);
    let s = scale / 20.;
    let view = Affine([
        s,
        0.,
        0.,
        s,
        -f64::from(m.stage[0]) * s,
        -f64::from(m.stage[2]) * s,
    ]);
    let mut p = Painter {
        lib: &lib,
        view,
        img: Image {
            width: w as u32,
            height: h as u32,
            pixels,
        },
        counts: Counts::default(),
        max_depth: l.max_depth.saturating_mul(8),
        visits: 0,
        max_visits: l.max_tags,
    };
    p.draw_list(None, Affine::ID, Tint::ID, 0);
    Ok((p.img, p.counts))
}
