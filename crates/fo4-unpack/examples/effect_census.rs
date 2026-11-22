#![forbid(unsafe_code)]
//! Census of the effect models that the scene refuses for effect-shader or
//! looping-controller reasons (`spec/effect-surfaces.md`): counts only,
//! written to `OUT.txt`; one line per model with its path and class to
//! `OUT.txt.models.tsv`, which is never committed.
//!
//! `effect_census MANIFEST OUT.txt [--omitted BASES.tsv]`
//!
//! Every winning NIF of the archives is read and walked as the asset graph
//! walks it. A model is in scope when a reached block of the `state` class
//! (particle systems aside) carries a rest tag without a rule
//! (`fo4_scene::rest`) and either that tag is `effect-idle` or
//! `manager-loop`, or the block is an effect-shader controller. For those
//! models and their reached shapes: the shader block type and its flag
//! bits, the alpha property's flag word joined with the shader flags, the
//! effect shader's fields, the effect material (BGEM) a shader names and
//! its fields, the drawn texture's format and alpha range on a small mip,
//! vertex colours and vertex alpha; and for every effect-shader controller
//! (direct or through a sequence's controlled block) what it drives, its
//! keys' range, start key and how the stored value relates to them.
//! Placements and the area proxy of `coverage_census` (largest OBND face
//! times the reference scale squared) are counted through the base records
//! that name each model; with `--omitted`, the omitted placements and area
//! of a coverage census's `bases.tsv` are added up by class too.
//!
//! Further sections: falloff, palette and colour above 1, the environment
//! map (its cube map and mask decoded, the scale, the other terms of the
//! same shapes), and the material swaps (MSWP) that replace the effect
//! material of a shape with controllers, with each driven field of the
//! replacement against the NIF's stored value; the swaps and materials are
//! listed in `OUT.txt.swaps.tsv`, never committed.
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_plugin::typed::Store;
use fo4_scene::{model_policy, paths, rest};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

fn fields(d: &Data) -> &[Field] {
    model_policy::fields(d)
}
fn block_fields<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [Field]> {
    n.blocks.get(i)?.decoded.as_ref().map(fields)
}
fn type_of<'n>(n: &'n Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map_or("", String::as_str)
}
fn floats(f: &[Field], name: &str) -> Vec<f32> {
    f.iter()
        .filter(|x| x.name == name)
        .filter_map(|x| match x.value {
            Value::FloatBits(b) => Some(f32::from_bits(b)),
            _ => None,
        })
        .collect()
}
fn refs(f: &[Field], name: &str) -> Vec<i32> {
    f.iter()
        .filter(|x| x.name == name)
        .filter_map(|x| match x.value {
            Value::Ref(r) => Some(r),
            _ => None,
        })
        .collect()
}
fn link(f: &[Field], name: &str) -> Option<usize> {
    refs(f, name)
        .first()
        .copied()
        .filter(|r| *r >= 0)
        .map(|r| r as usize)
}
fn word(f: &[Field], name: &str) -> Option<u32> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match x.value {
            Value::U32(v) => Some(v),
            Value::U16(v) => Some(u32::from(v)),
            Value::U8(v) => Some(u32::from(v)),
            _ => None,
        })
}
fn bytes<'f>(f: &'f [Field], name: &str) -> Option<&'f [u8]> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match &x.value {
            Value::Bytes(v) => Some(v.as_slice()),
            _ => None,
        })
}
fn string<'n>(n: &'n Nif<'_>, f: &[Field], name: &str) -> Option<&'n [u8]> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match x.value {
            Value::StringIndex(i) => n.strings.get(i as usize).map(Vec::as_slice),
            _ => None,
        })
}
fn particle(ty: &str) -> bool {
    ty.starts_with("NiPS") || ty.starts_with("BSPSys") || ty == "NiParticleSystem"
}
/// A value for a histogram key: shortest round-trip text of the f32.
fn v(x: f32) -> String {
    if x == rest::INVALID {
        "INV".into()
    } else {
        format!("{x}")
    }
}
/// A bucket of a continuous value for the committed histograms.
fn bucket(x: f32) -> String {
    let edges = [0., 0.1, 0.25, 0.5, 0.75, 1., 1.5, 2., 5., 10., 100., 1000.];
    if x.is_nan() {
        return "NaN".into();
    }
    if edges.contains(&x) {
        return format!("= {x}");
    }
    if x < 0. {
        return "< 0".into();
    }
    match edges.iter().position(|e| x < *e) {
        Some(k) => format!("{} to {}", edges[k - 1], edges[k]),
        None => "> 1000".into(),
    }
}
/// White, black, grey or coloured.
fn rgb(c: &[f32]) -> String {
    if c.len() < 3 {
        return "absent".into();
    }
    if c[..3].iter().all(|x| *x == 1.) {
        "1 1 1".into()
    } else if c[..3].iter().all(|x| *x == 0.) {
        "0 0 0".into()
    } else if (c[0] - c[1]).abs() < 1e-3 && (c[0] - c[2]).abs() < 1e-3 {
        "grey (r = g = b)".into()
    } else {
        "colour".into()
    }
}
fn bits(x: u32) -> Vec<u32> {
    (0..32).filter(|b| x >> b & 1 == 1).collect()
}
/// A falloff angle field (a cosine, nifxml) as the whole degree whose
/// cosine it is, when it is one within 1e-4.
fn cosine_label(x: f32) -> String {
    if !(-1. ..=1.).contains(&x) {
        return format!("{x} (not a cosine)");
    }
    let deg = f64::from(x).acos().to_degrees();
    let whole = deg.round();
    if (whole.to_radians().cos() - f64::from(x)).abs() <= 1e-4 {
        format!("cos {whole}")
    } else {
        format!("{x}")
    }
}
/// Vertex alpha of a mesh against its open boundary (edges used by one
/// triangle): whether the alpha is lower on the boundary than inside.
fn alpha_against_boundary(m: &fo4_nif::Mesh) -> String {
    let (Some(vs), Some(ts)) = (m.vertices.as_ref(), m.triangles.as_ref()) else {
        return "no vertex or triangle data".into();
    };
    let alpha: Vec<Option<f32>> = vs
        .iter()
        .map(|x| {
            x.color
                .map(|c| f32::from(c[3]) / 255.)
                .or(x.color_float.map(|c| c[3]))
        })
        .collect();
    if alpha.iter().any(Option::is_none) || alpha.is_empty() {
        return "no vertex alpha".into();
    }
    let mut edges: HashMap<(u16, u16), u32> = HashMap::new();
    for t in ts {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut boundary = vec![false; vs.len()];
    for ((a, b), n) in &edges {
        if *n == 1 {
            for v in [a, b] {
                if let Some(x) = boundary.get_mut(*v as usize) {
                    *x = true;
                }
            }
        }
    }
    let mean = |on: bool| {
        let v: Vec<f32> = alpha
            .iter()
            .zip(&boundary)
            .filter(|(_, b)| **b == on)
            .filter_map(|(a, _)| *a)
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    match (mean(true), mean(false)) {
        (None, _) => "closed (no open boundary)".into(),
        (Some(_), None) => "every vertex on the boundary".into(),
        (Some(b), Some(i)) => {
            let zero = alpha
                .iter()
                .zip(&boundary)
                .filter(|(a, b)| **b && a.is_some_and(|x| x == 0.))
                .count();
            let on = boundary.iter().filter(|b| **b).count();
            format!(
                "boundary mean {} interior mean; boundary vertices with alpha 0: {}",
                if b + 0.25 <= i {
                    "at least 0.25 below"
                } else if b < i {
                    "below"
                } else {
                    "not below"
                },
                if zero == on {
                    "all"
                } else if zero * 2 >= on {
                    "half or more"
                } else if zero > 0 {
                    "under half"
                } else {
                    "none"
                }
            )
        }
    }
}
/// A field an effect controller drives: (colour, controlled variable, the
/// NIF's stored value, the first key, every key equal).
type Driven = (bool, u32, Vec<f32>, Vec<f32>, bool);
/// Shapes, placements of their models and omitted placements by key.
type Weighted = BTreeMap<String, [u64; 3]>;
fn weigh(m: &mut Weighted, k: impl Into<String>, uses: &Uses) {
    let e = m.entry(k.into()).or_default();
    e[0] += 1;
    e[1] += uses.placements;
    e[2] += uses.omitted;
}

/// Keys of an NiFloatData or NiPosData: (time, value).
/// (interpolation type, keys as (time, value)).
type Keys = (u32, Vec<(f32, Vec<f32>)>);
fn keys(f: &[Field], comps: usize) -> Option<Keys> {
    let mut it = f.iter();
    let n = match it.next()? {
        x if x.name == "Num Keys" => match x.value {
            Value::U32(v) => v,
            _ => return None,
        },
        _ => return None,
    };
    if n == 0 {
        return Some((0, Vec::new()));
    }
    let kind = match it.next()? {
        x if x.name == "Interpolation" => match x.value {
            Value::U32(v) => v,
            _ => return None,
        },
        _ => return None,
    };
    let mut out = Vec::new();
    let mut current: Option<(f32, Vec<f32>)> = None;
    for x in it {
        let val = match x.value {
            Value::FloatBits(b) => f32::from_bits(b),
            _ => continue,
        };
        match x.name {
            "Key Time" => {
                if let Some(k) = current.take() {
                    out.push(k);
                }
                current = Some((val, Vec::new()));
            }
            "Key Value" => {
                if let Some((_, vals)) = current.as_mut() {
                    if vals.len() < comps {
                        vals.push(val);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(k) = current.take() {
        out.push(k);
    }
    (out.len() == n as usize && out.iter().all(|(_, v)| v.len() == comps)).then_some((kind, out))
}

/// The NIF effect-shader field and the BGEM field a controlled variable
/// names (nifxml EffectShaderControlledVariable / ...Color, by name).
fn controlled_field(color: bool, var: u32) -> &'static str {
    if color {
        return if var == 0 {
            "base colour rgb"
        } else {
            "colour ?"
        };
    }
    match var {
        0 => "base colour scale (emissive multiple)",
        1 => "falloff start angle",
        2 => "falloff stop angle",
        3 => "falloff start opacity",
        4 => "falloff stop opacity",
        5 => "alpha (base colour alpha)",
        6 => "u offset",
        7 => "u scale",
        8 => "v offset",
        9 => "v scale",
        _ => "other variable",
    }
}
fn bgem_value(m: &fo4_material::Material<'_>, color: bool, var: u32) -> Option<Vec<f32>> {
    let fo4_material::Kind::Bgem(e) = &m.kind else {
        return None;
    };
    let c = &m.common;
    Some(match (color, var) {
        (true, 0) => e.base_color.to_vec(),
        (false, 0) => vec![e.base_color_scale],
        (false, 1) => vec![e.falloff_start_angle],
        (false, 2) => vec![e.falloff_stop_angle],
        (false, 3) => vec![e.falloff_start_opacity],
        (false, 4) => vec![e.falloff_stop_opacity],
        (false, 5) => vec![c.alpha],
        (false, 6) => vec![c.u_offset],
        (false, 7) => vec![c.u_scale],
        (false, 8) => vec![c.v_offset],
        (false, 9) => vec![c.v_scale],
        _ => return None,
    })
}
fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-4)
}

/// Alpha and colour facts of a texture's small mip.
#[derive(Clone)]
struct TexFacts {
    format: String,
    alpha: String,
    grey: bool,
}
fn tex_facts(vfs: &Vfs, m: &Manifest, path: &[u8]) -> Option<TexFacts> {
    let b = Budget::new(m.limits.max_memory);
    let bytes = vfs.read_path_bytes(path, &b).ok()?;
    let t = fo4_texture::Texture::read(&bytes, "t", &m.limits, &b).ok()?;
    let format = t.format.name().to_string();
    let pick = t
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, s)| s.array == 0 && s.face == 0)
        .filter(|(_, s)| s.width.min(s.height) >= 16 || s.mip == 0)
        .max_by_key(|(_, s)| s.mip)
        .map(|(i, _)| i)?;
    let rgba = t.decode(pick, &m.limits, &b).ok()?;
    let (mut lo, mut hi, mut spread) = (f32::MAX, f32::MIN, 0f32);
    for p in rgba.as_chunks::<4>().0 {
        lo = lo.min(p[3]);
        hi = hi.max(p[3]);
        spread = spread.max((p[0] - p[1]).abs()).max((p[0] - p[2]).abs());
    }
    let alpha = if lo >= 1. {
        "alpha 1 everywhere".to_string()
    } else if hi <= 0. {
        "alpha 0 everywhere".to_string()
    } else {
        format!(
            "alpha varies (min {:.2}, max {:.2})",
            (lo * 4.).floor() / 4.,
            (hi * 4.).ceil() / 4.
        )
    };
    Some(TexFacts {
        format,
        alpha,
        grey: spread <= 2. / 255.,
    })
}

/// Statistics of the level of a texture the reference draws (the first
/// with both sides at most 256), for the palette census.
#[derive(Clone)]
struct TexStats {
    format: String,
    top: [u32; 2],
    level: [u32; 2],
    /// Largest |R - G|, |R - B| over the level.
    spread: f32,
    red: [f32; 2],
    alpha: [f32; 2],
    /// Largest |A - R|.
    alpha_vs_red: f32,
    /// Largest difference of any channel between a row and the first row,
    /// and between a column and the first column.
    rows: f32,
    columns: f32,
    /// Along the first row: red and alpha non-decreasing, non-increasing.
    red_along_u: &'static str,
    alpha_along_u: &'static str,
    rgb_max: f32,
    /// Means of (R + G + B) / 3 and of A over an 8 x 8 grid of the level,
    /// row by row (V down), for the unversioned palette listing.
    grid: Vec<[f32; 2]>,
    /// Standard deviations of the green channel and of alpha.
    deviation: [f32; 2],
}
impl TexStats {
    /// On the coarse grid: whether luminance and alpha never fall (by more
    /// than 4 of 255) along U in any row and along V in any column.
    fn monotone(&self) -> String {
        let w = self.level[0].min(8) as usize;
        let h = self.grid.len() / w.max(1);
        let rises = |k: usize, along_u: bool| {
            (0..h).all(|y| {
                (0..w).all(|x| {
                    let (nx, ny) = if along_u { (x + 1, y) } else { (x, y + 1) };
                    nx >= w
                        || ny >= h
                        || self.grid[ny * w + nx][k] >= self.grid[y * w + x][k] - 4. / 255.
                })
            })
        };
        format!(
            "luminance rises along U {} along V {}; alpha rises along U {} along V {}",
            rises(0, true),
            rises(0, false),
            rises(1, true),
            rises(1, false)
        )
    }
}
fn tex_stats(vfs: &Vfs, m: &Manifest, path: &[u8]) -> Option<TexStats> {
    let b = Budget::new(m.limits.max_memory);
    let bytes = vfs.read_path_bytes(path, &b).ok()?;
    let t = fo4_texture::Texture::read(&bytes, "t", &m.limits, &b).ok()?;
    let surfaces: Vec<(usize, &fo4_texture::Surface)> = t
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, s)| s.array == 0 && s.face == 0 && s.depth == 1)
        .collect();
    let (pick, s) = surfaces
        .iter()
        .find(|(_, s)| s.width <= 256 && s.height <= 256)
        .or(surfaces.last())?;
    let rgba = t.decode(*pick, &m.limits, &b).ok()?;
    let (w, h) = (s.width as usize, s.height as usize);
    let px = |x: usize, y: usize| -> [f32; 4] {
        let at = (y * w + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    };
    let mut st = TexStats {
        format: t.format.name().to_string(),
        top: [t.width, t.height],
        level: [s.width, s.height],
        spread: 0.,
        red: [f32::MAX, f32::MIN],
        alpha: [f32::MAX, f32::MIN],
        alpha_vs_red: 0.,
        rows: 0.,
        columns: 0.,
        red_along_u: "",
        alpha_along_u: "",
        rgb_max: 0.,
        grid: Vec::new(),
        deviation: [0.; 2],
    };
    let n = (w * h).max(1) as f64;
    for (k, ch) in [(0, 1), (1, 3)] {
        let mean = (0..w * h).map(|i| f64::from(rgba[i * 4 + ch])).sum::<f64>() / n;
        let var = (0..w * h)
            .map(|i| (f64::from(rgba[i * 4 + ch]) - mean).powi(2))
            .sum::<f64>()
            / n;
        st.deviation[k] = var.sqrt() as f32;
    }
    for y in 0..h {
        for x in 0..w {
            let p = px(x, y);
            st.spread = st.spread.max((p[0] - p[1]).abs()).max((p[0] - p[2]).abs());
            st.red = [st.red[0].min(p[0]), st.red[1].max(p[0])];
            st.alpha = [st.alpha[0].min(p[3]), st.alpha[1].max(p[3])];
            st.alpha_vs_red = st.alpha_vs_red.max((p[3] - p[0]).abs());
            st.rgb_max = st.rgb_max.max(p[0]).max(p[1]).max(p[2]);
            let (r, c) = (px(x, 0), px(0, y));
            for k in 0..4 {
                st.rows = st.rows.max((p[k] - r[k]).abs());
                st.columns = st.columns.max((p[k] - c[k]).abs());
            }
        }
    }
    let order = |ch: usize| -> &'static str {
        let v: Vec<f32> = (0..w).map(|x| px(x, 0)[ch]).collect();
        let up = v.windows(2).all(|p| p[1] >= p[0] - 1. / 255.);
        let down = v.windows(2).all(|p| p[1] <= p[0] + 1. / 255.);
        match (up, down) {
            (true, true) => "constant",
            (true, false) => "non-decreasing",
            (false, true) => "non-increasing",
            _ => "rises and falls",
        }
    };
    st.red_along_u = order(0);
    st.alpha_along_u = order(3);
    let (gw, gh) = (w.min(8), h.min(8));
    for gy in 0..gh {
        for gx in 0..gw {
            let (mut l, mut a, mut n) = (0f32, 0f32, 0f32);
            for y in gy * h / gh..(gy + 1) * h / gh {
                for x in gx * w / gw..(gx + 1) * w / gw {
                    let p = px(x, y);
                    l += (p[0] + p[1] + p[2]) / 3.;
                    a += p[3];
                    n += 1.;
                }
            }
            st.grid.push([l / n.max(1.), a / n.max(1.)]);
        }
    }
    Some(st)
}
/// What an environment map slot names: missing, unreadable (the reader's
/// error code), or the texture's kind, format, size and levels, with the
/// mean RGB over every face of a small level.
fn env_texture_facts(vfs: &Vfs, m: &Manifest, path: &[u8]) -> String {
    let b = Budget::new(m.limits.max_memory);
    let bytes = match vfs.read_path_bytes(path, &b) {
        Ok(x) => x,
        Err(e) => return format!("not read ({})", e.code),
    };
    let t = match fo4_texture::Texture::read(&bytes, "t", &m.limits, &b) {
        Ok(t) => t,
        Err(e) => return format!("unreadable ({})", e.code),
    };
    let kind = if t.cubemap {
        format!(
            "cube of {} faces",
            t.surfaces.iter().map(|s| s.face).max().map_or(0, |f| f + 1)
        )
    } else {
        "not a cube".to_string()
    };
    // The first level with a side of at most 64, on every face.
    let level = t
        .surfaces
        .iter()
        .filter(|s| s.array == 0 && s.width <= 64)
        .map(|s| s.mip)
        .min();
    let mut sum = [0f64; 3];
    let mut n = 0f64;
    for (i, s) in t.surfaces.iter().enumerate() {
        if s.array != 0 || Some(s.mip) != level {
            continue;
        }
        let Ok(rgba) = t.decode(i, &m.limits, &b) else {
            return format!(
                "{kind}, {} {}x{}, level not decoded",
                t.format.name(),
                t.width,
                t.height
            );
        };
        for p in rgba.as_chunks::<4>().0 {
            for k in 0..3 {
                sum[k] += f64::from(p[k]);
            }
            n += 1.;
        }
    }
    let mean = sum.map(|x| (x / n.max(1.)) as f32);
    format!(
        "{kind}, {} {}x{}, {} levels, mean rgb {}",
        t.format.name(),
        t.width,
        t.height,
        t.mipmaps.max(1),
        rgb(&mean)
    )
}
/// Per channel (R, G, B, A) range and mean of an environment mask's level
/// the reference would draw (the first with both sides at most 256).
fn mask_facts(vfs: &Vfs, m: &Manifest, path: &[u8]) -> String {
    let b = Budget::new(m.limits.max_memory);
    let bytes = match vfs.read_path_bytes(path, &b) {
        Ok(x) => x,
        Err(e) => return format!("not read ({})", e.code),
    };
    let t = match fo4_texture::Texture::read(&bytes, "t", &m.limits, &b) {
        Ok(t) => t,
        Err(e) => return format!("unreadable ({})", e.code),
    };
    let Some((i, _)) = t
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, s)| s.array == 0 && s.face == 0 && s.depth == 1)
        .find(|(_, s)| s.width <= 256 && s.height <= 256)
    else {
        return format!("{}, no small level", t.format.name());
    };
    let Ok(rgba) = t.decode(i, &m.limits, &b) else {
        return format!("{}, level not decoded", t.format.name());
    };
    let px = rgba.as_chunks::<4>().0;
    let n = px.len().max(1) as f32;
    let channel = |k: usize| {
        let lo = px.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
        let hi = px.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
        let mean = px.iter().map(|p| p[k]).sum::<f32>() / n;
        format!("{} mean {:.1}", range([lo, hi]), (mean * 10.).round() / 10.)
    };
    format!(
        "{}{}, R {}, G {}, B {}, A {}",
        t.format.name(),
        if t.cubemap { " cube" } else { "" },
        channel(0),
        channel(1),
        channel(2),
        channel(3)
    )
}
/// The fields in which two effect materials differ, by name.
fn bgem_diff(a: &fo4_material::Material<'_>, b: &fo4_material::Material<'_>) -> Vec<&'static str> {
    let (fo4_material::Kind::Bgem(x), fo4_material::Kind::Bgem(y)) = (&a.kind, &b.kind) else {
        return vec!["kind"];
    };
    let (c, d) = (&a.common, &b.common);
    [
        (
            x.base_texture.bytes() != y.base_texture.bytes(),
            "base texture",
        ),
        (
            x.grayscale_texture.bytes() != y.grayscale_texture.bytes(),
            "greyscale texture",
        ),
        (
            x.envmap_texture.bytes() != y.envmap_texture.bytes(),
            "environment texture",
        ),
        (x.base_color != y.base_color, "base colour"),
        (
            x.base_color_scale != y.base_color_scale,
            "base colour scale",
        ),
        (c.alpha != d.alpha, "alpha"),
        (
            [c.u_offset, c.v_offset] != [d.u_offset, d.v_offset],
            "uv offset",
        ),
        ([c.u_scale, c.v_scale] != [d.u_scale, d.v_scale], "uv scale"),
        (
            [
                x.falloff_start_angle,
                x.falloff_stop_angle,
                x.falloff_start_opacity,
                x.falloff_stop_opacity,
            ] != [
                y.falloff_start_angle,
                y.falloff_stop_angle,
                y.falloff_start_opacity,
                y.falloff_stop_opacity,
            ],
            "falloff fields",
        ),
        (
            (
                c.alpha_blend.enabled,
                c.alpha_blend.source,
                c.alpha_blend.destination,
                c.alpha_test.0,
            ) != (
                d.alpha_blend.enabled,
                d.alpha_blend.source,
                d.alpha_blend.destination,
                d.alpha_test.0,
            ),
            "blend",
        ),
        (
            (
                x.falloff_enabled.0,
                x.soft_enabled.0,
                x.grayscale_to_palette_alpha.0,
                c.grayscale_to_palette_color.0,
            ) != (
                y.falloff_enabled.0,
                y.soft_enabled.0,
                y.grayscale_to_palette_alpha.0,
                d.grayscale_to_palette_color.0,
            ),
            "flags",
        ),
    ]
    .into_iter()
    .filter_map(|(differs, k)| differs.then_some(k))
    .collect()
}
/// A difference between channel values, in 8-bit steps.
fn steps(x: f32) -> &'static str {
    let s = x * 255.;
    if s <= 2. {
        "at most 2 steps"
    } else if s <= 8. {
        "3 to 8 steps"
    } else if s <= 32. {
        "9 to 32 steps"
    } else {
        "more than 32 steps"
    }
}
/// A 0 to 1 range in quarter steps.
fn range(r: [f32; 2]) -> String {
    format!(
        "{:.2} to {:.2}",
        (r[0] * 4.).floor() / 4.,
        (r[1] * 4.).ceil() / 4.
    )
}

#[derive(Default, Clone)]
struct Uses {
    placements: u64,
    area: f64,
    bases: u64,
    omitted: u64,
    omitted_area: f64,
}
fn obnd_area(r: &fo4_plugin::Record) -> Option<f64> {
    let s = r.subrecords.iter().find(|s| s.signature == *b"OBND")?;
    let d = r.data(s);
    if d.len() != 12 {
        return None;
    }
    let b: [i16; 6] = std::array::from_fn(|k| i16::from_le_bytes([d[2 * k], d[2 * k + 1]]));
    let e = [0, 1, 2].map(|k| (f64::from(b[k + 3]) - f64::from(b[k])).abs());
    Some((e[0] * e[1]).max(e[0] * e[2]).max(e[1] * e[2]))
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "effect_census MANIFEST OUT.txt [--omitted BASES.tsv]";
    let omitted_file = match args.as_slice() {
        [_, _] => None,
        [_, _, flag, file] if flag == "--omitted" => Some(file.clone()),
        _ => return Err(usage.into()),
    };
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let mut c = Counts::new();

    // Placements and area by model path: enabled placed REFR -> base -> MODL.
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut base_uses: HashMap<fo4_plugin::FormKey, (u64, f64)> = HashMap::new();
    let placed: Vec<_> = store
        .placement_cells()
        .filter(|(o, _)| !o.deleted && !o.disabled && o.signature == *b"REFR")
        .map(|(o, _)| store.overrides().form_key(o.key, store.order()))
        .collect();
    let mut base_area: HashMap<fo4_plugin::FormKey, Option<f64>> = HashMap::new();
    let mut total_area = 0.;
    for k in &placed {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        let scale = r
            .subrecords
            .iter()
            .find(|s| s.signature == *b"XSCL")
            .and_then(|s| <[u8; 4]>::try_from(r.data(s)).ok())
            .map_or(1., |d| f64::from(f32::from_le_bytes(d)));
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"NAME") else {
            continue;
        };
        let Ok(d) = <[u8; 4]>::try_from(r.data(s)) else {
            continue;
        };
        let Ok(Some(base)) =
            store
                .order()
                .remap(plugin, u32::from_le_bytes(d), &r.source.file, s.offset)
        else {
            continue;
        };
        let face = match base_area.get(&base) {
            Some(a) => *a,
            None => {
                let a = store.raw(&base, &b)?.as_ref().and_then(obnd_area);
                base_area.insert(base.clone(), a);
                a
            }
        };
        let a = face.unwrap_or(0.) * scale * scale;
        total_area += a;
        let e = base_uses.entry(base).or_default();
        e.0 += 1;
        e.1 += a;
    }
    let mut model_uses: HashMap<Vec<u8>, Uses> = HashMap::new();
    for (base, (n, a)) in &base_uses {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(base, &b)? else {
            continue;
        };
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"MODL") else {
            continue;
        };
        let d = r.data(s);
        let Ok(path) = paths::resolve(
            paths::Citation::RecordModel,
            d.strip_suffix(&[0]).unwrap_or(d),
            &b,
        ) else {
            continue;
        };
        let e = model_uses.entry(path).or_default();
        e.bases += 1;
        e.placements += n;
        e.area += a;
    }
    drop(store);
    // Omitted placements and area by model, from a coverage census's
    // bases.tsv (one line per reason and base; a base counted once).
    let mut omitted_total = (0u64, 0f64);
    if let Some(file) = &omitted_file {
        let text = std::fs::read_to_string(file)?;
        let mut seen = BTreeSet::new();
        for line in text.lines().skip(1) {
            let col: Vec<&str> = line.split('\t').collect();
            let [_, base, _, _, model, n, a] = col.as_slice() else {
                continue;
            };
            if !seen.insert(base.to_string()) {
                continue;
            }
            let (n, a): (u64, f64) = (n.parse()?, a.parse()?);
            omitted_total.0 += n;
            omitted_total.1 += a;
            let b = Budget::new(m.limits.max_memory);
            if let Ok(path) = paths::resolve(paths::Citation::RecordModel, model.as_bytes(), &b) {
                let e = model_uses.entry(path).or_default();
                e.omitted += n;
                e.omitted_area += a;
            }
        }
    }

    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut nifs: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".nif"))
        .collect();
    nifs.sort();
    nifs.dedup();
    let mut textures: HashMap<Vec<u8>, Option<TexFacts>> = HashMap::new();
    let mut rows = String::from("model\tclass\tplacements\tarea\tomitted\tomitted_area\tdetail\n");
    let mut classes: BTreeMap<String, Uses> = BTreeMap::new();
    let mut class_models: Counts = Counts::new();
    let mut detail_rows = String::new();
    let mut falloff_weights = Weighted::new();
    let mut palette_weights = Weighted::new();
    let mut env_weights = Weighted::new();
    let mut env_textures: HashMap<Vec<u8>, String> = HashMap::new();
    // Effect materials whose shapes have controllers, by resolved path.
    let mut driven_materials: HashMap<Vec<u8>, Vec<Driven>> = HashMap::new();
    let mut stats: HashMap<Vec<u8>, Option<TexStats>> = HashMap::new();
    let (mut files, mut failed) = (0u64, 0u64);
    for path in &nifs {
        let b = Budget::new(m.limits.max_memory);
        let Ok(nb) = vfs.read_path_bytes(path, &b) else {
            failed += 1;
            continue;
        };
        let Ok(n) = Nif::read(&nb, "m", Game::Fallout4, &m.limits, &b) else {
            failed += 1;
            continue;
        };
        files += 1;
        // Reached blocks, as the asset graph walks them.
        let count = n.blocks.len();
        let mut seen = vec![false; count];
        let mut stack: Vec<usize> = n
            .roots
            .iter()
            .filter(|r| **r >= 0)
            .map(|r| *r as usize)
            .collect();
        let mut reached = Vec::new();
        while let Some(i) = stack.pop() {
            if i >= count || seen[i] {
                continue;
            }
            seen[i] = true;
            let ty = type_of(&n, i);
            let class = model_policy::block_class(ty);
            if class == fo4_plugin::static_frame::Class::Outside {
                continue;
            }
            reached.push(i);
            let is_state = class == fo4_plugin::static_frame::Class::State;
            let Some(d) = &n.blocks[i].decoded else {
                continue;
            };
            if let Data::Node { children, .. } = d {
                stack.extend(children.iter().filter(|x| **x >= 0).map(|x| *x as usize));
            }
            for f in fields(d) {
                if let Value::Ref(r) = f.value {
                    if r >= 0 && !(is_state && model_policy::weak_edge(ty, f.name)) {
                        stack.push(r as usize);
                    }
                }
            }
        }
        reached.sort_unstable();
        let state: Vec<usize> = reached
            .iter()
            .copied()
            .filter(|i| {
                model_policy::block_class(type_of(&n, *i)) == fo4_plugin::static_frame::Class::State
                    && !particle(type_of(&n, *i))
            })
            .collect();
        if state.is_empty() {
            continue;
        }
        let Ok(tags) = rest::tags(&n, &b) else {
            bump(&mut c, "models: rest tags not computed");
            continue;
        };
        let ruled = [
            rest::MANAGER_IDLE,
            rest::IDLE_TRANSFORM,
            rest::IDLE_SHADER,
            rest::TRANSFORM_CONSTANT,
            rest::UNRENDERED,
        ];
        // Tags of the earlier rules, and the effect first-key tag of this
        // census's rule (counted apart).
        let mut open: BTreeSet<&str> = BTreeSet::new();
        let mut in_scope = false;
        for &i in &state {
            let ty = type_of(&n, i);
            for t in tags.get(i).into_iter().flatten() {
                if ruled.contains(t) {
                    continue;
                }
                open.insert(t);
                if *t == rest::EFFECT_IDLE
                    || *t == rest::MANAGER_LOOP
                    || *t == rest::EFFECT_FIRST_KEY
                    || *t == rest::FALLOFF_DEGREES
                    || ty.starts_with("BSEffectShaderProperty")
                {
                    in_scope = true;
                }
            }
        }
        if !in_scope {
            continue;
        }
        let uses = model_uses.get(path).cloned().unwrap_or_default();
        bump(&mut c, "models: in scope");
        if uses.placements > 0 {
            bump(&mut c, "models: in scope, named by placed bases");
        }
        let tagset: Vec<&str> = open.iter().copied().collect();
        bump(
            &mut c,
            format!("models by open rest tags: {}", tagset.join(" + ")),
        );

        // Shapes: reached blocks with a shader property link.
        let mut shader_of_shape: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();
        for &i in &reached {
            let Some(f) = block_fields(&n, i) else {
                continue;
            };
            let sh = refs(f, "Shader Property");
            if sh.is_empty() {
                continue;
            }
            let shader = link(f, "Shader Property");
            let alpha = link(f, "Alpha Property");
            shader_of_shape.push((i, shader, alpha));
        }
        // Effect controllers: (controller block, shader, source label).
        let mut ctls: Vec<(usize, Option<usize>, String)> = Vec::new();
        let has_manager = reached
            .iter()
            .any(|i| type_of(&n, *i) == "NiControllerManager");
        for &i in &state {
            let ty = type_of(&n, i);
            let Some(f) = block_fields(&n, i) else {
                continue;
            };
            if ty.starts_with("BSEffectShaderProperty") {
                let flags = word(f, "Controller Flags").unwrap_or(0);
                let src = if flags & 0x20 != 0 && has_manager {
                    "controller run by a manager"
                } else {
                    "controller not run by a manager"
                };
                ctls.push((i, link(f, "Target"), src.into()));
            }
        }
        // Sequences: which effect controllers they drive, loop or clamp.
        let mut seq_of: HashMap<usize, (String, f32, f32, f32, Option<usize>)> = HashMap::new();
        for &i in &state {
            if type_of(&n, i) != "NiControllerSequence" {
                continue;
            }
            let Some(f) = block_fields(&n, i) else {
                continue;
            };
            let cycle = word(f, "Cycle Type").unwrap_or(99);
            let label = match cycle {
                0 => "sequence, loop",
                1 => "sequence, reverse",
                2 => "sequence, clamp",
                _ => "sequence, other cycle",
            };
            let start = floats(f, "Start Time").first().copied().unwrap_or(f32::NAN);
            let stop = floats(f, "Stop Time").first().copied().unwrap_or(f32::NAN);
            let freq = floats(f, "Frequency").first().copied().unwrap_or(f32::NAN);
            let ips = refs(f, "Interpolator");
            let cs = refs(f, "Controller");
            bump(&mut c, format!("sequences: {label}"));
            bump(
                &mut c,
                format!("sequences: {label}: frequency {}", bucket(freq)),
            );
            bump(
                &mut c,
                format!(
                    "sequences: {label}: length in seconds {}",
                    bucket(stop - start)
                ),
            );
            for (k, ip) in ips.iter().enumerate() {
                let ctl = cs.get(k).copied().unwrap_or(-1);
                if ctl < 0 {
                    bump(
                        &mut c,
                        "sequence controlled blocks: without a controller link",
                    );
                    continue;
                }
                let ctl = ctl as usize;
                if type_of(&n, ctl).starts_with("BSEffectShaderProperty") {
                    let ipl = (*ip >= 0).then_some(*ip as usize);
                    // A looping sequence wins over another that names the
                    // same controller.
                    if cycle == 0 || !seq_of.contains_key(&ctl) {
                        seq_of.insert(ctl, (label.into(), start, stop, freq, ipl));
                    }
                    if *ip >= 0 {
                        bump(
                            &mut c,
                            format!(
                                "sequence controlled blocks on effect shaders: interpolator {}",
                                type_of(&n, *ip as usize)
                            ),
                        );
                    }
                }
            }
        }
        // Per shape facts.
        let mut model_facts: BTreeSet<String> = BTreeSet::new();
        let mut detail: Vec<String> = Vec::new();
        // Draw gates of the effect surface rules, by shape.
        let mut gates: BTreeSet<&'static str> = BTreeSet::new();
        let mut effect_shapes = 0;
        for (shape, shader, alpha) in &shader_of_shape {
            let sty = shader.map_or("none", |s| type_of(&n, s));
            bump(&mut c, format!("shapes: shader {sty}"));
            let Some(s) = *shader else { continue };
            let Some(sf) = block_fields(&n, s) else {
                continue;
            };
            let controlled: Vec<&(usize, Option<usize>, String)> =
                ctls.iter().filter(|x| x.1 == Some(s)).collect();
            let ctl_label = if controlled.is_empty() {
                "not controlled"
            } else {
                "controlled"
            };
            if sty != "BSEffectShaderProperty" {
                model_facts.insert(format!("{sty} shape"));
                continue;
            }
            effect_shapes += 1;
            let f1 = word(sf, "Shader Flags 1").unwrap_or(0);
            let f2 = word(sf, "Shader Flags 2").unwrap_or(0);
            for bit in bits(f1) {
                bump(&mut c, format!("effect shapes: flags 1 bit {bit:02}"));
            }
            for bit in bits(f2) {
                bump(&mut c, format!("effect shapes: flags 2 bit {bit:02}"));
            }
            bump(&mut c, "effect shapes");
            bump(&mut c, format!("effect shapes: {ctl_label}"));
            // Alpha property joined with the shader's flags that read alpha.
            let af = alpha.and_then(|a| block_fields(&n, a)).map(|f| {
                (
                    word(f, "Alpha Flags").unwrap_or(0),
                    word(f, "Threshold").unwrap_or(0),
                )
            });
            let alpha_key = match af {
                None => "no alpha property".to_string(),
                Some((w, t)) => format!(
                    "blend {} src {} dst {} test {} func {} threshold {}{}",
                    w & 1,
                    w >> 1 & 15,
                    w >> 5 & 15,
                    w >> 9 & 1,
                    w >> 10 & 7,
                    if w >> 9 & 1 == 1 { t } else { 0 },
                    if w & 0xE000 != 0 {
                        format!(" high bits 0x{:04x}", w & 0xE000)
                    } else {
                        String::new()
                    }
                ),
            };
            let palette_alpha = f1 >> 5 & 1;
            let palette_color = f1 >> 4 & 1;
            let falloff = f1 >> 6 & 1;
            let soft = f1 >> 30 & 1;
            let vcol = f2 >> 5 & 1;
            let valpha = f1 >> 3 & 1;
            bump(
                &mut c,
                format!(
                    "effect shapes: alpha property x flags: {alpha_key}; palette colour {palette_color} palette alpha {palette_alpha} falloff {falloff} soft {soft} vertex colours {vcol} vertex alpha {valpha}"
                ),
            );
            bump(
                &mut c,
                format!("effect shapes: alpha property: {alpha_key}"),
            );
            // Effect shader fields.
            let bc = floats(sf, "Base Color");
            let fo = [
                "Falloff Start Angle",
                "Falloff Stop Angle",
                "Falloff Start Opacity",
                "Falloff Stop Opacity",
            ]
            .map(|k| floats(sf, k).first().copied().unwrap_or(f32::NAN));
            let one = |k: &str| floats(sf, k).first().copied().unwrap_or(f32::NAN);
            let hist = |c: &mut Counts, k: &str, x: String| {
                bump(c, format!("effect shader fields: {k}: {x}"));
            };
            if bc.len() == 4 {
                hist(&mut c, "base colour alpha", bucket(bc[3]));
                hist(&mut c, "base colour rgb", rgb(&bc));
            }
            hist(&mut c, "base colour scale", bucket(one("Base Color Scale")));
            hist(
                &mut c,
                "soft falloff depth",
                bucket(one("Soft Falloff Depth")),
            );
            hist(
                &mut c,
                "lighting influence (byte)",
                match word(sf, "Lighting Influence") {
                    Some(255) => "255".into(),
                    Some(0) => "0".into(),
                    Some(x) => format!(
                        "1 to 254 ({})",
                        if x < 128 { "below 128" } else { "128 or more" }
                    ),
                    None => "absent".into(),
                },
            );
            if falloff == 1 {
                hist(
                    &mut c,
                    "falloff flag set: angles (start stop)",
                    format!("{} {}", v(fo[0]), v(fo[1])),
                );
                hist(
                    &mut c,
                    "falloff flag set: opacities (start stop)",
                    format!("{} {}", v(fo[2]), v(fo[3])),
                );
            }
            hist(
                &mut c,
                "texture clamp mode",
                format!("{}", word(sf, "Texture Clamp Mode").unwrap_or(999)),
            );
            hist(
                &mut c,
                "env map min lod / unused byte",
                format!(
                    "{} {}",
                    word(sf, "Env Map Min LOD").unwrap_or(999),
                    word(sf, "Unused Byte").unwrap_or(999)
                ),
            );
            hist(
                &mut c,
                "environment map scale",
                bucket(one("Environment Map Scale")),
            );
            for t in [
                "Source Texture",
                "Greyscale Texture",
                "Environment Map Texture",
                "Normal Texture",
                "Environment Mask Texture",
            ] {
                hist(
                    &mut c,
                    &format!("{t} set"),
                    format!("{}", bytes(sf, t).is_some_and(|p| !p.is_empty())),
                );
            }
            let uvo = floats(sf, "UV Offset");
            let uvs = floats(sf, "UV Scale");
            hist(
                &mut c,
                "uv offset / scale",
                format!(
                    "offset {} scale {}",
                    if uvo.iter().all(|x| *x == 0.) {
                        "0 0"
                    } else {
                        "other"
                    },
                    if uvs.iter().all(|x| *x == 1.) {
                        "1 1"
                    } else {
                        "other"
                    }
                ),
            );
            // The block read to its last byte?
            let blk = &n.blocks[s];
            hist(
                &mut c,
                "block read to the last byte",
                format!("{}", blk.raw_tail.is_empty() && blk.diagnostics.is_empty()),
            );
            // Vertex colours and alpha.
            let mesh = n.blocks[*shape].decoded.as_ref().and_then(|d| match d {
                Data::Mesh(m) => Some(m),
                _ => None,
            });
            let vertex_alpha_min = mesh.and_then(|m| m.vertices.as_ref()).map(|vs| {
                vs.iter()
                    .filter_map(|x| {
                        x.color
                            .map(|c| f32::from(c[3]) / 255.)
                            .or(x.color_float.map(|c| c[3]))
                    })
                    .fold(1f32, f32::min)
            });
            let vkey = match mesh.and_then(|m| m.vertices.as_ref()) {
                None => "no vertex data".to_string(),
                Some(vs) => {
                    let cols: Vec<[f32; 4]> = vs
                        .iter()
                        .filter_map(|x| {
                            x.color
                                .map(|c| c.map(|v| f32::from(v) / 255.))
                                .or(x.color_float)
                        })
                        .collect();
                    if cols.is_empty() {
                        "no vertex colours".into()
                    } else {
                        let amin = cols.iter().map(|c| c[3]).fold(1f32, f32::min);
                        let white = cols.iter().all(|c| c[0] >= 1. && c[1] >= 1. && c[2] >= 1.);
                        format!(
                            "vertex colours: rgb {}, alpha {}",
                            if white { "all 1" } else { "vary" },
                            if amin >= 1. {
                                "all 1".to_string()
                            } else {
                                format!("min {:.1}", (amin * 10.).floor() / 10.)
                            }
                        )
                    }
                }
            };
            bump(
                &mut c,
                format!("effect shapes: vertex data x flags: {vkey}; vertex colours flag {vcol} vertex alpha flag {valpha}"),
            );
            // Material.
            let name = string(&n, sf, "Name").unwrap_or(b"");
            let lower = name.to_ascii_lowercase();
            let mut tex_path: Option<Vec<u8>> = None;
            let mut mat_key = "no material named".to_string();
            let mut mat_read = None;
            let mut mat_path: Option<Vec<u8>> = None;
            let mbytes;
            if lower.ends_with(b".bgem") || lower.ends_with(b".bgsm") {
                let resolved =
                    paths::resolve(paths::Citation::NifMaterial, name, &b).or_else(|_| {
                        paths::resolve_with(
                            paths::Citation::NifMaterial,
                            name,
                            &vfs,
                            fo4_scene::RuleLevel::Hypotheses,
                            &b,
                        )
                        .map(|r| r.key)
                    });
                mat_path = resolved.as_ref().ok().cloned();
                match resolved.ok().and_then(|p| vfs.read_path_bytes(&p, &b).ok()) {
                    None => mat_key = "material missing".into(),
                    Some(bytesm) => {
                        mbytes = bytesm;
                        match fo4_material::Material::read(&mbytes, "m", &m.limits, &b) {
                            Err(_) => mat_key = "material unreadable".into(),
                            Ok(mat) => {
                                mat_key = mat.magic().to_string();
                                mat_read = Some(mat);
                            }
                        }
                    }
                }
            } else if let Some(t) = bytes(sf, "Source Texture").filter(|p| !p.is_empty()) {
                tex_path = paths::resolve(paths::Citation::NifTexture, t, &b).ok();
            }
            bump(&mut c, format!("effect shapes: material: {mat_key}"));
            if let Some(mat) = &mat_read {
                let cm = &mat.common;
                let ab = &cm.alpha_blend;
                let mk = format!(
                    "blend {} src {} dst {} test {} ref {}",
                    ab.enabled,
                    ab.source,
                    ab.destination,
                    cm.alpha_test.0,
                    if cm.alpha_test.0 != 0 {
                        cm.alpha_test_ref
                    } else {
                        0
                    },
                );
                if let fo4_material::Kind::Bgem(e) = &mat.kind {
                    detail.push(format!(
                        "bgem rgb {:?} scale {} alpha {} falloff {} {:?} soft {} palette a/c {}/{} {mk}",
                        e.base_color,
                        e.base_color_scale,
                        cm.alpha,
                        e.falloff_enabled.0,
                        [e.falloff_start_angle, e.falloff_stop_angle, e.falloff_start_opacity, e.falloff_stop_opacity],
                        e.soft_enabled.0,
                        e.grayscale_to_palette_alpha.0,
                        cm.grayscale_to_palette_color.0
                    ));
                }
                bump(
                    &mut c,
                    format!("effect shapes with material: material alpha: {mk}"),
                );
                bump(
                    &mut c,
                    format!("effect shapes with material: material alpha vs NIF alpha property: material {mk} | NIF {alpha_key}"),
                );
                let bf = |c: &mut Counts, k: &str, x: String| {
                    bump(c, format!("effect material fields: {k}: {x}"));
                };
                bf(
                    &mut c,
                    "z write / z test",
                    format!("{} {}", cm.z_buffer_write.0, cm.z_buffer_test.0),
                );
                bf(&mut c, "two sided", format!("{}", cm.two_sided.0));
                bf(
                    &mut c,
                    "grayscale to palette colour",
                    format!("{}", cm.grayscale_to_palette_color.0),
                );
                bf(&mut c, "refraction", format!("{}", cm.refraction.0));
                bf(&mut c, "tile flags", format!("{}", cm.tile_flags));
                if let fo4_material::Kind::Bgem(e) = &mat.kind {
                    bf(
                        &mut c,
                        "flags (palette alpha, falloff, falloff colour, soft, effect lighting, blood)",
                        format!(
                            "{} {} {} {} {} {}",
                            e.grayscale_to_palette_alpha.0,
                            e.falloff_enabled.0,
                            e.falloff_color_enabled.0,
                            e.soft_enabled.0,
                            e.effect_lighting_enabled.0,
                            e.blood_enabled.0
                        ),
                    );
                    bf(&mut c, "base colour rgb", rgb(&e.base_color));
                    bf(&mut c, "base colour scale", bucket(e.base_color_scale));
                    bf(&mut c, "alpha", bucket(cm.alpha));
                    bf(
                        &mut c,
                        "falloff angles (start stop)",
                        format!("{} {}", v(e.falloff_start_angle), v(e.falloff_stop_angle)),
                    );
                    bf(
                        &mut c,
                        "falloff opacities (start stop)",
                        format!(
                            "{} {}",
                            v(e.falloff_start_opacity),
                            v(e.falloff_stop_opacity)
                        ),
                    );
                    bf(&mut c, "lighting influence", bucket(e.lighting_influence));
                    bf(&mut c, "soft depth", bucket(e.soft_depth));
                    bf(
                        &mut c,
                        "textures set (base, greyscale, envmap, normal, envmask)",
                        format!(
                            "{} {} {} {} {}",
                            !e.base_texture.bytes().is_empty(),
                            !e.grayscale_texture.bytes().is_empty(),
                            !e.envmap_texture.bytes().is_empty(),
                            !e.normal_texture.bytes().is_empty(),
                            !e.envmap_mask_texture.bytes().is_empty()
                        ),
                    );
                    bf(
                        &mut c,
                        "optional fields present (glass, envmap flag, glowmap, pbr specular, emittance)",
                        format!(
                            "{} {} {} {} {}",
                            e.glass_enabled.map_or("-".into(), |x| x.0.to_string()),
                            e.environment_mapping.map_or("-".into(), |x| x.0.to_string()),
                            e.glowmap.map_or("-".into(), |x| x.0.to_string()),
                            e.effect_pbr_specular.map_or("-".into(), |x| x.0.to_string()),
                            e.emittance_color.is_some()
                        ),
                    );
                    bf(&mut c, "version", format!("{}", mat.version));
                    // NIF flags against the material's flags.
                    bump(
                        &mut c,
                        format!(
                            "effect shapes with BGEM: palette alpha NIF/BGEM {palette_alpha}/{} falloff NIF/BGEM {falloff}/{} soft NIF/BGEM {soft}/{}",
                            e.grayscale_to_palette_alpha.0, e.falloff_enabled.0, e.soft_enabled.0
                        ),
                    );
                    if !e.base_texture.bytes().is_empty() {
                        tex_path = paths::resolve(
                            paths::Citation::MaterialTexture,
                            e.base_texture.bytes(),
                            &b,
                        )
                        .ok();
                    }
                    model_facts.insert(format!(
                        "bgem palette alpha {} palette colour {} falloff {} soft {} blend {} {}-{}",
                        e.grayscale_to_palette_alpha.0,
                        cm.grayscale_to_palette_color.0,
                        e.falloff_enabled.0,
                        e.soft_enabled.0,
                        ab.enabled,
                        ab.source,
                        ab.destination
                    ));
                }
            } else {
                model_facts.insert(format!(
                    "nif palette alpha {palette_alpha} palette colour {palette_color} falloff {falloff} soft {soft} {alpha_key}"
                ));
            }
            // Drawn texture.
            let tkey = match &tex_path {
                None => "no texture".to_string(),
                Some(p) => {
                    let f = textures
                        .entry(p.clone())
                        .or_insert_with(|| tex_facts(&vfs, &m, p))
                        .clone();
                    match f {
                        None => "texture missing or unreadable".into(),
                        Some(t) => format!(
                            "{} {}, {}",
                            t.format,
                            if t.grey { "grey" } else { "colour" },
                            t.alpha
                        ),
                    }
                }
            };
            bump(&mut c, format!("effect shapes: drawn texture: {tkey}"));
            detail.push(format!(
                "shape f1 {f1:#x} f2 {f2:#x} {alpha_key} bc {bc:?} scale {} falloff {fo:?} tex {tkey} {vkey}",
                one("Base Color Scale")
            ));
            // Gates, with the material's fields where one is named.
            let bgem = mat_read.as_ref().and_then(|mm| match &mm.kind {
                fo4_material::Kind::Bgem(e) => Some((e, &mm.common)),
                _ => None,
            });
            let (pal, fall, rgb_fall, env, colour, scale_v, alpha_v) = match bgem {
                Some((e, cm)) => (
                    e.grayscale_to_palette_alpha.0 != 0 || cm.grayscale_to_palette_color.0 != 0,
                    e.falloff_enabled.0 != 0,
                    e.falloff_color_enabled.0 != 0,
                    cm.refraction.0 != 0
                        || e.environment_mapping.is_some_and(|x| x.0 != 0)
                        || cm.environment_mapping.is_some_and(|x| x.0 != 0)
                        || e.glass_enabled.is_some_and(|x| x.0 != 0),
                    e.base_color.to_vec(),
                    e.base_color_scale,
                    cm.alpha,
                ),
                None => (
                    palette_alpha != 0 || palette_color != 0,
                    falloff != 0,
                    f1 >> 8 & 1 != 0,
                    f1 & (1 << 7 | 1 << 15 | 1 << 16 | 1 << 2) != 0,
                    bc.get(..3).map_or(Vec::new(), <[f32]>::to_vec),
                    one("Base Color Scale"),
                    bc.get(3).copied().unwrap_or(f32::NAN),
                ),
            };
            if mat_read.is_none() && mat_key != "no material named" {
                gates.insert("material missing or unreadable");
            }
            // A palette is drawn under its rules where the greyscale slot
            // names a texture.
            let grey_named = match bgem {
                Some((e, _)) => !e.grayscale_texture.bytes().is_empty(),
                None => bytes(sf, "Greyscale Texture").is_some_and(|p| !p.is_empty()),
            };
            if pal && !grey_named {
                gates.insert("palette texture not set");
            }
            if rgb_fall {
                gates.insert("colour falloff");
            }
            let vertex_alpha_max = mesh.and_then(|m| m.vertices.as_ref()).and_then(|vs| {
                vs.iter()
                    .filter_map(|x| {
                        x.color
                            .map(|c| f32::from(c[3]) / 255.)
                            .or(x.color_float.map(|c| c[3]))
                    })
                    .reduce(f32::max)
            });
            if let Some(hi) = vertex_alpha_max {
                bump(
                    &mut c,
                    format!(
                        "effect shapes: largest vertex alpha x flags: {}; vertex colours flag {vcol} vertex alpha flag {valpha}",
                        if hi == 0. {
                            "0 (every vertex alpha 0)".to_string()
                        } else if hi < 0.5 {
                            "below 0.5".to_string()
                        } else if hi < 1. {
                            "0.5 to 1".to_string()
                        } else {
                            "1".to_string()
                        }
                    ),
                );
            }
            // Vertex_Colors alone is read under its rule; Vertex_Alpha alone
            // has none.
            if vcol == 0 && valpha == 1 && vertex_alpha_min.is_some_and(|a| a < 1.) {
                gates.insert("vertex alpha flag alone");
            }
            // Above 1 is drawn clamped under its rule; below 0 is not.
            if colour
                .iter()
                .any(|x| !(x * scale_v).is_finite() || x * scale_v < 0.)
            {
                gates.insert("colour negative");
            }
            let blending = match bgem {
                Some((_, cm)) => cm.alpha_blend.enabled != 0,
                None => af.is_some_and(|(w, _)| w & 1 == 1),
            };
            if !(0. ..=1.).contains(&alpha_v) && (blending || alpha_v < 0.) {
                gates.insert("alpha outside 0 to 1");
            }
            if tkey == "texture missing or unreadable" {
                gates.insert("texture missing");
            }
            // Falloff as the draw build reads it: the material's fields
            // where a BGEM is named, the shader's otherwise.
            let (fall_angles, fall_two_sided, fall_soft) = match bgem {
                Some((e, cm)) => (
                    [
                        e.falloff_start_angle,
                        e.falloff_stop_angle,
                        e.falloff_start_opacity,
                        e.falloff_stop_opacity,
                    ],
                    cm.two_sided.0 != 0,
                    e.soft_enabled.0 != 0,
                ),
                None => (fo, f2 >> 4 & 1 == 1, soft == 1),
            };
            if fall || rgb_fall {
                let w = &mut falloff_weights;
                let source = if bgem.is_some() { "BGEM" } else { "NIF" };
                let [sa, ea, so, eo] = fall_angles;
                weigh(
                    w,
                    format!("terms: falloff {fall} rgb falloff {rgb_fall} (from {source})"),
                    &uses,
                );
                weigh(
                    w,
                    format!(
                        "angles and opacities: {} -> {} | {} -> {}",
                        cosine_label(sa),
                        cosine_label(ea),
                        v(so),
                        v(eo)
                    ),
                    &uses,
                );
                weigh(
                    w,
                    format!(
                        "angle order: {}",
                        if sa > ea {
                            "start cosine above stop (opaque facing the eye)"
                        } else if sa < ea {
                            "start cosine below stop (opaque at grazing angles)"
                        } else {
                            "start equal to stop"
                        }
                    ),
                    &uses,
                );
                weigh(w, format!("negative cosine: {}", sa < 0. || ea < 0.), &uses);
                weigh(
                    w,
                    format!(
                        "opacities within 0 to 1: {}",
                        [so, eo].iter().all(|x| (0. ..=1.).contains(x))
                    ),
                    &uses,
                );
                weigh(w, format!("blend: {alpha_key}"), &uses);
                weigh(
                    w,
                    format!("two sided {fall_two_sided}, soft {fall_soft}, palette {pal}, feature {env}, colour above 1 {}",
                        colour.iter().any(|x| x * scale_v > 1.)),
                    &uses,
                );
                weigh(
                    w,
                    format!(
                        "greyscale texture set {} with palette {pal}",
                        bytes(sf, "Greyscale Texture").is_some_and(|p| !p.is_empty())
                            || bgem.is_some_and(|(e, _)| !e.grayscale_texture.bytes().is_empty())
                    ),
                    &uses,
                );
                let normals = mesh
                    .and_then(|m| m.vertices.as_ref())
                    .map(|vs| vs.iter().all(|x| x.normal.is_some()));
                weigh(
                    w,
                    format!(
                        "vertex normals on every vertex: {normals:?}; skinned flag {}",
                        f1 >> 1 & 1
                    ),
                    &uses,
                );
                // The draw build's falloff gates (`fo4_render::Falloff`).
                let fields_ok = [sa, ea].iter().all(|x| x.is_finite())
                    && [so, eo].iter().all(|x| (0. ..=1.).contains(x))
                    && (sa != ea || so == eo);
                if fall && !rgb_fall && !(fields_ok && normals == Some(true)) {
                    gates.insert("falloff fields or normals");
                }
                weigh(
                    w,
                    format!("vertex flags: colours {vcol} alpha {valpha}; {vkey}"),
                    &uses,
                );
                if vcol != valpha {
                    weigh(
                        w,
                        format!(
                            "one vertex flag: colours {vcol} alpha {valpha}; {}",
                            mesh.map_or("no mesh".into(), alpha_against_boundary)
                        ),
                        &uses,
                    );
                }
                weigh(w, format!("drawn texture: {tkey}"), &uses);
                if (sa - ea).abs() <= 1e-6 {
                    weigh(
                        w,
                        format!("equal angles: opacities {} {}", v(so), v(eo)),
                        &uses,
                    );
                }
            }
            // Palette, colour range and features, as the draw build reads
            // them (the material's fields where a BGEM is named).
            let (pal_colour, pal_alpha) = match bgem {
                Some((e, cm)) => (
                    cm.grayscale_to_palette_color.0 != 0,
                    e.grayscale_to_palette_alpha.0 != 0,
                ),
                None => (palette_color != 0, palette_alpha != 0),
            };
            let features: Vec<&str> = match bgem {
                Some((e, cm)) => [
                    (cm.refraction.0 != 0, "refraction"),
                    (
                        cm.environment_mapping.is_some_and(|x| x.0 != 0)
                            || e.environment_mapping.is_some_and(|x| x.0 != 0),
                        "environment map",
                    ),
                    (e.glass_enabled.is_some_and(|x| x.0 != 0), "glass"),
                    (e.glowmap.is_some_and(|x| x.0 != 0), "glow map"),
                    (
                        e.effect_pbr_specular.is_some_and(|x| x.0 != 0),
                        "pbr specular",
                    ),
                    (e.blood_enabled.0 != 0, "blood"),
                    (e.emittance_color.is_some(), "emittance colour"),
                ]
                .into_iter()
                .filter_map(|(on, k)| on.then_some(k))
                .collect(),
                None => [
                    (f1 >> 2 & 1 != 0, "temp refraction"),
                    (f1 >> 7 & 1 != 0, "environment map"),
                    (f1 >> 15 & 1 != 0, "refraction"),
                    (f1 >> 16 & 1 != 0, "fire refraction"),
                ]
                .into_iter()
                .filter_map(|(on, k)| on.then_some(k))
                .collect(),
            };
            let blend_key = match bgem {
                Some((_, cm)) => format!(
                    "material blend {} src {} dst {} test {}",
                    cm.alpha_blend.enabled,
                    cm.alpha_blend.source,
                    cm.alpha_blend.destination,
                    cm.alpha_test.0
                ),
                None => alpha_key.clone(),
            };
            let tint_max = colour
                .iter()
                .map(|x| x * scale_v)
                .fold(f32::NEG_INFINITY, f32::max);
            let above = colour.iter().any(|x| x * scale_v > 1.);
            let source = if bgem.is_some() { "BGEM" } else { "NIF" };
            let combo = format!(
                "palette {}{}, colour above 1 {above}, falloff {fall}, feature {}, vertex flags colours {vcol} alpha {valpha}",
                if pal_colour { "colour" } else { "-" },
                if pal_alpha { "+alpha" } else { "" },
                !features.is_empty()
            );
            let pw = &mut palette_weights;
            // The environment map is drawn without its term under its rules
            // (`effect_environment_left_out_v1`); every other feature
            // refuses the surface.
            if features.iter().any(|k| *k != "environment map") {
                gates.insert("feature (refraction, glass, glow map, blood, emittance)");
            }
            if !features.is_empty() {
                weigh(
                    pw,
                    format!("feature: {} (from {source})", features.join(" + ")),
                    &uses,
                );
                for k in &features {
                    if *k == "environment map" {
                        let env_tex = match bgem {
                            Some((e, _)) => !e.envmap_texture.bytes().is_empty(),
                            None => {
                                bytes(sf, "Environment Map Texture").is_some_and(|p| !p.is_empty())
                            }
                        };
                        weigh(
                            pw,
                            format!(
                                "feature: environment map: texture set {env_tex}, scale {}",
                                bucket(one("Environment Map Scale"))
                            ),
                            &uses,
                        );
                    }
                }
            }
            // Environment map, as the draw build reads it (the material's
            // fields where a BGEM is named).
            if features.contains(&"environment map") {
                let ew = &mut env_weights;
                let nif_flag = f1 >> 7 & 1;
                let nif_scale = one("Environment Map Scale");
                let (env_slot, mask_slot, scale, normal_set) = match bgem {
                    Some((e, cm)) => {
                        weigh(
                            ew,
                            format!(
                                "flags: NIF Environment_Mapping {nif_flag}, BGEM flag (version 10 on) {}, common flag (before 10) {}",
                                e.environment_mapping.map_or("-".into(), |x| x.0.to_string()),
                                cm.environment_mapping.map_or("-".into(), |x| x.0.to_string())
                            ),
                            &uses,
                        );
                        (
                            Some(e.envmap_texture.bytes().to_vec()).filter(|p| !p.is_empty()),
                            Some(e.envmap_mask_texture.bytes().to_vec()).filter(|p| !p.is_empty()),
                            e.environment_mapping_mask_scale
                                .or(cm.environment_mapping_mask_scale)
                                .unwrap_or(f32::NAN),
                            !e.normal_texture.bytes().is_empty(),
                        )
                    }
                    None => (
                        bytes(sf, "Environment Map Texture")
                            .filter(|p| !p.is_empty())
                            .map(<[u8]>::to_vec),
                        bytes(sf, "Environment Mask Texture")
                            .filter(|p| !p.is_empty())
                            .map(<[u8]>::to_vec),
                        nif_scale,
                        bytes(sf, "Normal Texture").is_some_and(|p| !p.is_empty()),
                    ),
                };
                let citation = if bgem.is_some() {
                    paths::Citation::MaterialTexture
                } else {
                    paths::Citation::NifTexture
                };
                let mut facts = |slot: &Option<Vec<u8>>, mask: bool| match slot {
                    None => "slot empty".to_string(),
                    Some(p) => match paths::resolve(citation, p, &b) {
                        Err(e) => format!("path not resolved ({})", e.code),
                        Ok(p) => env_textures
                            .entry(p.clone())
                            .or_insert_with(|| {
                                if mask {
                                    mask_facts(&vfs, &m, &p)
                                } else {
                                    env_texture_facts(&vfs, &m, &p)
                                }
                            })
                            .clone(),
                    },
                };
                let (env_key, mask_key) = (facts(&env_slot, false), facts(&mask_slot, true));
                weigh(ew, format!("source: {source}"), &uses);
                weigh(ew, format!("environment texture: {env_key}"), &uses);
                weigh(ew, format!("mask texture: {mask_key}"), &uses);
                weigh(
                    ew,
                    format!(
                        "scale (the drawn field): {}{}",
                        bucket(scale),
                        if bgem.is_some() {
                            format!(", NIF field {}", bucket(nif_scale))
                        } else {
                            String::new()
                        }
                    ),
                    &uses,
                );
                weigh(
                    ew,
                    format!(
                        "slots: environment {}, mask {}, normal {normal_set}; scale 0 {}",
                        env_slot.is_some(),
                        mask_slot.is_some(),
                        scale == 0.
                    ),
                    &uses,
                );
                weigh(ew, format!("blend: {blend_key}"), &uses);
                weigh(
                    ew,
                    format!(
                        "base colour {}, scale {}, alpha {}",
                        rgb(&colour),
                        bucket(scale_v),
                        bucket(alpha_v)
                    ),
                    &uses,
                );
                weigh(ew, format!("drawn texture: {tkey}"), &uses);
                weigh(
                    ew,
                    format!(
                        "other terms: palette {}{}, falloff {fall}, colour above 1 {above}, other features {}",
                        if pal_colour { "colour" } else { "-" },
                        if pal_alpha { "+alpha" } else { "" },
                        features
                            .iter()
                            .filter(|k| **k != "environment map")
                            .copied()
                            .collect::<Vec<_>>()
                            .join(" + ")
                    ),
                    &uses,
                );
                weigh(
                    ew,
                    format!("vertex data: {vkey}; flags colours {vcol} alpha {valpha}"),
                    &uses,
                );
                detail.push(format!(
                    "env from {source} texture {} ({env_key}) mask {} ({mask_key}) scale {scale} normal {normal_set}",
                    env_slot.as_ref().map_or("-".into(), |p| String::from_utf8_lossy(p).into_owned()),
                    mask_slot.as_ref().map_or("-".into(), |p| String::from_utf8_lossy(p).into_owned()),
                ));
            }
            if above {
                weigh(
                    pw,
                    format!(
                        "colour above 1: largest base colour x scale {}, base colour {}, scale {}",
                        bucket(tint_max),
                        rgb(&colour),
                        bucket(scale_v)
                    ),
                    &uses,
                );
                weigh(pw, format!("colour above 1: blend: {blend_key}"), &uses);
                weigh(pw, format!("colour above 1: combination: {combo}"), &uses);
                let brightest = tex_path.as_ref().and_then(|p| {
                    stats
                        .entry(p.clone())
                        .or_insert_with(|| tex_stats(&vfs, &m, p))
                        .clone()
                });
                weigh(
                    pw,
                    format!(
                        "colour above 1: drawn texture's largest RGB x largest base colour x scale: {}",
                        match (&tex_path, brightest) {
                            (None, _) => format!("no texture, {}", bucket(tint_max)),
                            (Some(_), None) => "texture missing or unreadable".into(),
                            (Some(_), Some(t)) => bucket(t.rgb_max * tint_max),
                        }
                    ),
                    &uses,
                );
            }
            if pal_colour || pal_alpha {
                weigh(
                    pw,
                    format!(
                        "palette: variant colour {pal_colour} alpha {pal_alpha} (from {source})"
                    ),
                    &uses,
                );
                weigh(pw, format!("palette: combination: {combo}"), &uses);
                weigh(pw, format!("palette: blend: {blend_key}"), &uses);
                weigh(
                    pw,
                    format!(
                        "palette: base colour {}, scale {}, alpha {}",
                        rgb(&colour),
                        bucket(scale_v),
                        bucket(alpha_v)
                    ),
                    &uses,
                );
                weigh(pw, format!("palette: vertex data: {vkey}"), &uses);
                let grey = match bgem {
                    Some((e, _)) => paths::resolve(
                        paths::Citation::MaterialTexture,
                        e.grayscale_texture.bytes(),
                        &b,
                    )
                    .ok()
                    .filter(|_| !e.grayscale_texture.bytes().is_empty()),
                    None => bytes(sf, "Greyscale Texture")
                        .filter(|p| !p.is_empty())
                        .and_then(|p| paths::resolve(paths::Citation::NifTexture, p, &b).ok()),
                };
                let mut stat = |p: &Option<Vec<u8>>| {
                    p.as_ref().and_then(|p| {
                        stats
                            .entry(p.clone())
                            .or_insert_with(|| tex_stats(&vfs, &m, p))
                            .clone()
                    })
                };
                let (gs, ss) = (stat(&grey), stat(&tex_path));
                detail.push(format!(
                    "palette colour {pal_colour} alpha {pal_alpha} palette {} source {}",
                    grey.as_ref()
                        .map_or("-".into(), |p| String::from_utf8_lossy(p).into_owned()),
                    tex_path
                        .as_ref()
                        .map_or("-".into(), |p| String::from_utf8_lossy(p).into_owned())
                ));
                match (&grey, &gs) {
                    (None, _) => weigh(pw, "palette texture: not set", &uses),
                    (Some(_), None) => weigh(pw, "palette texture: missing or unreadable", &uses),
                    (Some(_), Some(g)) => {
                        weigh(
                            pw,
                            format!(
                                "palette texture: {} {}x{} (drawn level {}x{})",
                                g.format, g.top[0], g.top[1], g.level[0], g.level[1]
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette texture: rows differ by {}; columns differ by {}",
                                steps(g.rows),
                                steps(g.columns)
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette texture: along U red {}, alpha {}; rgb spread {}; alpha {}",
                                g.red_along_u,
                                g.alpha_along_u,
                                steps(g.spread),
                                range(g.alpha)
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette texture x variant: colour {pal_colour} alpha {pal_alpha}: {}",
                                g.monotone()
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette texture x variant: colour {pal_colour} alpha {pal_alpha}: alpha {}, alpha along U {}",
                                range(g.alpha),
                                g.alpha_along_u
                            ),
                            &uses,
                        );
                    }
                }
                match (&tex_path, &ss) {
                    (None, _) => weigh(pw, "palette source texture: none", &uses),
                    (Some(_), None) => {
                        weigh(pw, "palette source texture: missing or unreadable", &uses)
                    }
                    (Some(_), Some(s)) => {
                        weigh(
                            pw,
                            format!(
                                "palette source texture: {}, rgb spread {}",
                                s.format,
                                steps(s.spread)
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette source texture: red {}, alpha {}, alpha against red {}",
                                range(s.red),
                                range(s.alpha),
                                steps(s.alpha_vs_red)
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette source texture x variant: colour {pal_colour} alpha {pal_alpha}: deviation of green {}, of alpha {}",
                                steps(s.deviation[0]),
                                steps(s.deviation[1])
                            ),
                            &uses,
                        );
                        weigh(
                            pw,
                            format!(
                                "palette source texture x variant: colour {pal_colour} alpha {pal_alpha}: rgb spread {}, alpha against red {}, alpha {}",
                                steps(s.spread),
                                steps(s.alpha_vs_red),
                                range(s.alpha)
                            ),
                            &uses,
                        );
                    }
                }
            }
            // Vertex alpha against the open boundary, for every effect
            // shape with one vertex flag and a vertex alpha below 1.
            if vcol != valpha && vertex_alpha_min.is_some_and(|a| a < 1.) {
                weigh(
                    &mut falloff_weights,
                    format!(
                        "every effect shape, one vertex flag (colours {vcol} alpha {valpha}), vertex alpha below 1: {}",
                        mesh.map_or("no mesh".into(), alpha_against_boundary)
                    ),
                    &uses,
                );
            }
            let has_mat_alpha = mat_read
                .as_ref()
                .map_or(palette_alpha, |mat| match &mat.kind {
                    fo4_material::Kind::Bgem(e) => u32::from(e.grayscale_to_palette_alpha.0),
                    _ => palette_alpha,
                });
            bump(
                &mut c,
                format!(
                    "effect shapes: drawn texture x palette alpha {has_mat_alpha}: {}",
                    tkey.split(", ").nth(1).unwrap_or(&tkey)
                ),
            );
            // Controllers on this shader.
            for (ci, _, src) in controlled {
                let cty = type_of(&n, *ci);
                let Some(cf) = block_fields(&n, *ci) else {
                    continue;
                };
                let color = cty == "BSEffectShaderPropertyColorController";
                let var = word(cf, "Controlled Variable").unwrap_or(999);
                let seq = seq_of.get(ci);
                let src = seq.map_or(src.clone(), |s| s.0.clone());
                let field = controlled_field(color, var);
                bump(&mut c, format!("effect controllers: {field}: {src}"));
                model_facts.insert(format!("drives {field} ({src})"));
                let flags = word(cf, "Controller Flags").unwrap_or(0);
                bump(
                    &mut c,
                    format!("effect controllers: flags 0x{flags:04x} ({src})"),
                );
                // The sequence's interpolator holds the keys of a
                // sequence-run controller; its own is a blend interpolator.
                let ip = seq.and_then(|s| s.4).or_else(|| link(cf, "Interpolator"));
                let ipty = ip.map_or("none", |i| type_of(&n, i));
                bump(&mut c, format!("effect controllers: interpolator {ipty}"));
                let Some(ipf) = ip.and_then(|i| block_fields(&n, i)) else {
                    continue;
                };
                let comps = if color { 3 } else { 1 };
                let pose = if color {
                    floats(ipf, "Value")
                } else {
                    floats(ipf, "Value")
                        .into_iter()
                        .chain(floats(ipf, "Pose Value"))
                        .take(1)
                        .collect()
                };
                let data = link(ipf, "Data").and_then(|d| block_fields(&n, d));
                let k = data.and_then(|d| keys(d, comps));
                let nifv = rest::effect_field(sf, color, var);
                let bgemv = mat_read.as_ref().and_then(|mm| bgem_value(mm, color, var));
                let stored = bgemv.clone().or(nifv.clone());
                let Some((kind, ks)) = k else {
                    bump(
                        &mut c,
                        format!(
                            "effect controllers: {field}: no key data (pose {})",
                            pose.iter().map(|x| v(*x)).collect::<Vec<_>>().join(" ")
                        ),
                    );
                    continue;
                };
                bump(
                    &mut c,
                    format!(
                        "effect controllers: key interpolation {kind}, keys {}",
                        ks.len().min(10)
                    ),
                );
                if ks.is_empty() {
                    continue;
                }
                let first = &ks[0].1;
                let comp0: Vec<f32> = ks.iter().map(|(_, x)| x[0]).collect();
                if let (Some(mp), Some(nv), Some(fo4_material::Kind::Bgem(_))) =
                    (&mat_path, &nifv, mat_read.as_ref().map(|x| &x.kind))
                {
                    let equal = ks.iter().all(|(_, x)| close(x, first));
                    driven_materials.entry(mp.clone()).or_default().push((
                        color,
                        var,
                        nv.clone(),
                        first.clone(),
                        equal,
                    ));
                }
                let lo = comp0.iter().copied().fold(f32::MAX, f32::min);
                let hi = comp0.iter().copied().fold(f32::MIN, f32::max);
                let at_start = seq.and_then(|s| {
                    ks.iter()
                        .find(|(t, _)| (*t - s.1).abs() <= 1e-4)
                        .map(|(_, x)| x.clone())
                });
                detail.push(format!(
                    "ctl {field} ({src}) keys {} min {lo} max {hi} first {first:?} stored {stored:?}",
                    ks.len()
                ));
                bump(
                    &mut c,
                    format!(
                        "effect controller ranges: {field} ({src}): min {}, max {}, first key {}",
                        bucket(lo),
                        bucket(hi),
                        bucket(first[0])
                    ),
                );
                if let Some(st) = &stored {
                    let rel = if close(st, first) {
                        "stored = first key"
                    } else if at_start.as_ref().is_some_and(|a| close(st, a)) {
                        "stored = key at sequence start"
                    } else if ks.iter().any(|(_, x)| close(st, x)) {
                        "stored = another key"
                    } else if !color
                        && matches!(var, 1 | 2)
                        && close(st, &[(first[0].to_radians()).cos()])
                    {
                        "stored = cosine of the first key read as degrees"
                    } else if st[0] >= lo - 1e-4 && st[0] <= hi + 1e-4 {
                        "stored inside key range, equal to no key"
                    } else {
                        "stored outside key range"
                    };
                    bump(
                        &mut c,
                        format!(
                            "effect controllers: {field} ({src}): {rel} (stored from {})",
                            if bgemv.is_some() { "BGEM" } else { "NIF" }
                        ),
                    );
                    if let (Some(nv), Some(bv)) = (&nifv, &bgemv) {
                        bump(
                            &mut c,
                            format!(
                                "effect controllers: {field}: NIF value {} BGEM value",
                                if close(nv, bv) { "=" } else { "!=" }
                            ),
                        );
                    }
                    let always_same = comp0.iter().all(|x| (x - comp0[0]).abs() <= 1e-4);
                    if always_same {
                        bump(
                            &mut c,
                            format!("effect controllers: {field} ({src}): every key equal"),
                        );
                    }
                }
            }
        }
        if effect_shapes == 0 {
            model_facts.insert("no effect shape".into());
        }
        // Class: the open tags left besides the effect first-key tag, else
        // the effect gates that fail.
        let others: Vec<&str> = tagset
            .iter()
            .copied()
            .filter(|t| *t != rest::EFFECT_FIRST_KEY && *t != rest::FALLOFF_DEGREES)
            .collect();
        let class = if !others.is_empty() {
            format!("rest not settled: {}", others.join(" + "))
        } else if effect_shapes == 0 {
            "settled, no effect shape".to_string()
        } else if gates.is_empty() {
            "settled, every effect surface passes".to_string()
        } else {
            format!(
                "settled, effect surfaces fail: {}",
                gates.iter().copied().collect::<Vec<_>>().join(" + ")
            )
        };
        let facts = model_facts.iter().cloned().collect::<Vec<_>>().join(" ; ");
        let _ = writeln!(
            detail_rows,
            "{}\t{class}\t{facts}",
            String::from_utf8_lossy(path)
        );
        let e = classes.entry(class.clone()).or_default();
        e.placements += uses.placements;
        e.area += uses.area;
        e.omitted += uses.omitted;
        e.omitted_area += uses.omitted_area;
        bump(&mut class_models, class.clone());
        let _ = writeln!(
            rows,
            "{}\t{class}\t{}\t{:.0}\t{}\t{:.0}\t{}",
            String::from_utf8_lossy(path),
            uses.placements,
            uses.area,
            uses.omitted,
            uses.omitted_area,
            detail.join(" | ")
        );
    }

    // Material swaps that replace an effect material whose shapes have
    // controllers: the replacement's value of each driven field against
    // the NIF's stored value (the first key where the rest rule settles it).
    let mut swap_counts = Counts::new();
    let mut swap_rows = String::from(
        "swap\tsource\treplacement\tfield\tnif\tfirst\treplacement value\tother differences\n",
    );
    {
        let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
        let keys: Vec<_> = store
            .winners()
            .filter(|w| w.signature == *b"MSWP")
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        let read_bgem =
            |p: &[u8], b: &Budget| -> Option<Vec<u8>> { vfs.read_path_bytes(p, b).ok() };
        for key in &keys {
            let b = Budget::new(m.limits.max_memory);
            let Ok(Some(sw)) = store.get::<fo4_plugin::typed::Mswp>(key, &b) else {
                continue;
            };
            if sw.record.deleted() {
                continue;
            }
            let mut from: Option<Vec<u8>> = None;
            for entry in &sw.data.entries {
                let fo4_plugin::typed::SwapValue::Path(text) = &entry.value else {
                    continue;
                };
                match &entry.signature {
                    b"BNAM" => {
                        from =
                            paths::resolve(paths::Citation::SwapMaterial, text.as_bytes(), &b).ok();
                    }
                    b"SNAM" => {
                        let Some(source) = from.take() else { continue };
                        let Some(driven) = driven_materials.get(&source) else {
                            continue;
                        };
                        bump(
                            &mut swap_counts,
                            "swap pairs replacing a material with driven fields",
                        );
                        let Ok(target) =
                            paths::resolve(paths::Citation::SwapMaterial, text.as_bytes(), &b)
                        else {
                            bump(&mut swap_counts, "replacement path not resolved");
                            continue;
                        };
                        let (Some(sb), Some(tb)) = (read_bgem(&source, &b), read_bgem(&target, &b))
                        else {
                            bump(&mut swap_counts, "source or replacement material missing");
                            continue;
                        };
                        let (Ok(sm), Ok(tm)) = (
                            fo4_material::Material::read(&sb, "s", &m.limits, &b),
                            fo4_material::Material::read(&tb, "t", &m.limits, &b),
                        ) else {
                            bump(
                                &mut swap_counts,
                                "source or replacement material unreadable",
                            );
                            continue;
                        };
                        let diff = bgem_diff(&sm, &tm);
                        bump(
                            &mut swap_counts,
                            format!(
                                "pairs: replacement differs from the source in: {}",
                                if diff.is_empty() {
                                    "nothing".into()
                                } else {
                                    diff.join(", ")
                                }
                            ),
                        );
                        let mut seen = BTreeSet::new();
                        for (color, var, nifv, first, equal) in driven {
                            if !seen.insert((*color, *var)) {
                                continue;
                            }
                            let field = controlled_field(*color, *var);
                            let Some(tv) = bgem_value(&tm, *color, *var) else {
                                continue;
                            };
                            let rel = if close(&tv, nifv) {
                                "replacement = NIF stored".to_string()
                            } else {
                                format!(
                                    "replacement != NIF stored; NIF stored {} first key; replacement {} the first key; every key equal {equal}",
                                    if close(nifv, first) { "=" } else { "!=" },
                                    if tv[0] < first[0] { "below" } else { "above" }
                                )
                            };
                            bump(&mut swap_counts, format!("driven fields: {field}: {rel}"));
                            if !close(&tv, nifv) {
                                bump(
                                    &mut swap_counts,
                                    format!(
                                        "driven fields that differ: {field}: NIF {} first key {} replacement {}; other differences: {}",
                                        bucket(nifv[0]),
                                        bucket(first[0]),
                                        bucket(tv[0]),
                                        diff.iter().filter(|k| !field.starts_with(**k) && !(**k == "uv offset" && field.ends_with("offset"))).copied().collect::<Vec<_>>().join(", ")
                                    ),
                                );
                                let _ = writeln!(
                                    swap_rows,
                                    "{}\t{}\t{}\t{field}\t{:?}\t{:?}\t{:?}\t{}",
                                    sw.data.common.editor_id.as_deref().unwrap_or("-"),
                                    String::from_utf8_lossy(&source),
                                    String::from_utf8_lossy(&target),
                                    nifv,
                                    first,
                                    tv,
                                    diff.join(", ")
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Effect census (`effect_census`): {files} NIF read, {failed} unreadable, {:.0} s",
        started.elapsed().as_secs_f64()
    );
    let _ = writeln!(
        out,
        "# enabled placed REFR {}, area proxy {:.4e}; omitted (from bases.tsv) {} placements, area {:.4e}\n",
        placed.len(),
        total_area,
        omitted_total.0,
        omitted_total.1
    );
    for (k, n) in &c {
        let _ = writeln!(out, "{n}\t{k}");
    }
    let _ = writeln!(out, "\n# Classes (open tags || facts): models, placements, area share of all placed, omitted placements, omitted area share of all omitted\n");
    let mut cl: Vec<_> = classes.iter().collect();
    cl.sort_by(|a, b| b.1.area.total_cmp(&a.1.area).then(a.0.cmp(b.0)));
    for (k, u) in cl {
        let _ = writeln!(
            out,
            "{}\t{}\t{:.3}%\t{}\t{:.3}%\t{k}",
            class_models.get(k).copied().unwrap_or(0),
            u.placements,
            100. * u.area / total_area.max(1.),
            u.omitted,
            100. * u.omitted_area / omitted_total.1.max(1.)
        );
    }
    let _ = writeln!(out, "\n# Falloff (shapes whose drawn fields enable falloff or RGB falloff): shapes, placements of their models, omitted placements\n");
    for (k, [shapes, placements, omitted]) in &falloff_weights {
        let _ = writeln!(out, "{shapes}\t{placements}\t{omitted}\t{k}");
    }
    let _ = writeln!(out, "\n# Palette, colour above 1 and features (shapes whose drawn fields set them): shapes, placements of their models, omitted placements\n");
    for (k, [shapes, placements, omitted]) in &palette_weights {
        let _ = writeln!(out, "{shapes}\t{placements}\t{omitted}\t{k}");
    }
    let _ = writeln!(out, "\n# Environment map (shapes whose drawn fields set it): shapes, placements of their models, omitted placements\n");
    for (k, [shapes, placements, omitted]) in &env_weights {
        let _ = writeln!(out, "{shapes}\t{placements}\t{omitted}\t{k}");
    }
    let _ = writeln!(out, "\n# Controlled fields under material swaps (swap pairs whose source material has driven fields)\n");
    for (k, n) in &swap_counts {
        let _ = writeln!(out, "{n}\t{k}");
    }
    std::fs::write(format!("{}.swaps.tsv", args[1]), swap_rows)?;
    std::fs::write(&args[1], out)?;
    std::fs::write(format!("{}.models.tsv", args[1]), rows)?;
    std::fs::write(format!("{}.facts.tsv", args[1]), detail_rows)?;
    // Every texture the palette section read, with its coarse grid (paths:
    // never committed).
    let mut listing = String::from(
        "texture\tformat\ttop\tlevel\trows\tcolumns\tgrid (luminance/alpha, 8 x 8, V down)\n",
    );
    let mut seen: Vec<_> = stats.iter().collect();
    seen.sort_by(|a, b| a.0.cmp(b.0));
    for (p, t) in seen {
        let Some(t) = t else { continue };
        let grid: Vec<String> = t
            .grid
            .chunks(t.level[0].min(8) as usize)
            .map(|row| {
                row.iter()
                    .map(|c| format!("{:02}/{:02}", (c[0] * 99.).round(), (c[1] * 99.).round()))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let _ = writeln!(
            listing,
            "{}\t{}\t{}x{}\t{}x{}\t{:.0}\t{:.0}\t{}",
            String::from_utf8_lossy(p),
            t.format,
            t.top[0],
            t.top[1],
            t.level[0],
            t.level[1],
            t.rows * 255.,
            t.columns * 255.,
            grid.join(" | ")
        );
    }
    std::fs::write(format!("{}.textures.tsv", args[1]), listing)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
