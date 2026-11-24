//! A frame's light and sky, each reading under a pending rule behind its own
//! switch, off by default (`spec/light.md`, `spec/hypotheses.tsv`).
//!
//! [`frame`] reads what the data stores for the scene's cell and returns a
//! [`Light`]: an ambient colour or an ambient cube read by a surface's
//! normal, a directional colour and direction, placed lights as points with
//! a radius, a fog by distance and a sky gradient. The renderers draw it
//! (`fo4_render::render_lit`, the viewer's surface shader); with every
//! switch off [`frame`] returns `None` and nothing changes.
//!
//! What the data decides and what it does not:
//! - interior cells store their light in CELL.XCLL and may name a lighting
//!   template (LTMP, an LGTM) whose fields a word of XCLL (`Inherits`)
//!   names by printed labels (`spec/record-flag-words.tsv`); both layouts
//!   are sourced (`spec/record-layouts.tsv`). How the game combines them,
//!   the direction the two rotation angles give and how the light falls
//!   off are not in the data: each is a rule.
//! - placed lights (LIGH) store a radius and a colour; the falloff curve,
//!   spot cones and shadows are not in the data.
//! - exterior cells take their light from their worldspace's climate
//!   (WRLD CNAM, a CLMT) and its weathers (`fo4_plugin::weather`); which
//!   weather applies when the climate lists several, and the hour, are the
//!   caller's ([`Assumptions`]); the slot an hour reads and the rows of the
//!   weather's colour table that are sky and fog are rules.
use crate::{hypotheses, Scene};
use fo4_core::{Budget, Error, Result};
use fo4_plugin::{
    actor::Context,
    layout,
    typed::{CellKind, Store, Wrld},
    weather::{Climate, Weather},
    FormKey, Record,
};
use serde::Serialize;
use std::collections::BTreeSet;

/// The switches of the light step, each turning on one reading (all off
/// by default; with all off [`frame`] returns `None`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Switches {
    /// `--light-cell`: an interior cell's ambient and directional colour
    /// ([`hypotheses::LIGHT_CELL`]).
    pub cell: bool,
    /// `--light-inherits`: the fields the cell's `Inherits` word names
    /// taken from its lighting template ([`hypotheses::LIGHT_INHERITS`]).
    pub inherits: bool,
    /// `--light-points`: placed lights as points with a linear falloff to
    /// their radius ([`hypotheses::LIGHT_POINTS`]).
    pub points: bool,
    /// `--light-reference-radius`: a placed light's radius is its base's
    /// plus its reference's XRDS ([`hypotheses::LIGHT_REFERENCE_RADIUS`]).
    pub reference_radius: bool,
    /// `--light-fade`: a placed light's colour times its base's fade value
    /// (LIGH FNAM) ([`hypotheses::LIGHT_FADE`]).
    pub fade: bool,
    /// `--light-fog`: an interior cell's fog by distance
    /// ([`hypotheses::LIGHT_CELL_FOG`]).
    pub fog: bool,
    /// `--sky-weather`: an exterior frame's background as a gradient of
    /// the weather's sky rows ([`hypotheses::WEATHER_SKY`]).
    pub sky: bool,
    /// `--light-weather`: an exterior frame's ambient cube from the
    /// weather's directional ambient ([`hypotheses::WEATHER_AMBIENT`]).
    pub weather_ambient: bool,
    /// `--fog-weather`: an exterior frame's fog from the weather's fog row
    /// and distances ([`hypotheses::WEATHER_FOG`]).
    pub weather_fog: bool,
    /// `--exposure-eye`: a lit colour divided by the mean light falling on
    /// what the eye sees ([`hypotheses::EXPOSURE_EYE`], [`Light::expose_seen`]).
    pub exposure_eye: bool,
    /// `--exposure-white`: a lit colour tone mapped with the white of the
    /// frame's image space ([`hypotheses::IMAGE_SPACE_WHITE`]).
    pub exposure_white: bool,
    /// `--exposure-cinematic`: a lit colour's saturation, brightness,
    /// contrast and tint from the frame's image space
    /// ([`hypotheses::IMAGE_SPACE_CINEMATIC`]).
    pub exposure_cinematic: bool,
    /// `--light-vertex-normals`: a lit surface whose mesh stores vertex
    /// normals is lit with them, interpolated, instead of its face normal
    /// ([`hypotheses::LIGHT_VERTEX_NORMALS`]; the draw build keeps the
    /// normals, `fo4_render::draw::BuildOptions::vertex_normals`).
    pub vertex_normals: bool,
    /// `--exposure-eye-floor` (with `--exposure-eye`): a view whose seen
    /// light is below [`EXPOSURE_FLOOR`] (or has none) takes one over the
    /// floor as its scale ([`hypotheses::EXPOSURE_FLOOR`]).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exposure_floor: bool,
}
impl Switches {
    const FLAGS: [&'static str; 14] = [
        "--light-cell",
        "--light-inherits",
        "--light-points",
        "--light-reference-radius",
        "--light-fade",
        "--light-fog",
        "--sky-weather",
        "--light-weather",
        "--fog-weather",
        "--exposure-eye",
        "--exposure-white",
        "--exposure-cinematic",
        "--light-vertex-normals",
        "--exposure-eye-floor",
    ];
    fn slots(&mut self) -> [&mut bool; 14] {
        [
            &mut self.cell,
            &mut self.inherits,
            &mut self.points,
            &mut self.reference_radius,
            &mut self.fade,
            &mut self.fog,
            &mut self.sky,
            &mut self.weather_ambient,
            &mut self.weather_fog,
            &mut self.exposure_eye,
            &mut self.exposure_white,
            &mut self.exposure_cinematic,
            &mut self.vertex_normals,
            &mut self.exposure_floor,
        ]
    }
    /// Sets the switch a command-line flag names; false for any other text.
    pub fn set_flag(&mut self, flag: &str) -> bool {
        match Self::FLAGS.iter().position(|f| *f == flag) {
            Some(k) => {
                *self.slots()[k] = true;
                true
            }
            None => false,
        }
    }
    /// The flags of the switches that are on, in [`Switches::FLAGS`] order.
    pub fn flags(&self) -> Vec<&'static str> {
        let mut copy = *self;
        let on: Vec<bool> = copy.slots().iter().map(|v| **v).collect();
        Self::FLAGS
            .iter()
            .zip(on)
            .filter_map(|(f, on)| on.then_some(*f))
            .collect()
    }
    pub fn any(&self) -> bool {
        *self != Self::default()
    }
    /// Whether a switch of the exterior's weather is on.
    pub fn weather(&self) -> bool {
        self.sky || self.weather_ambient || self.weather_fog
    }
    /// Whether a switch reading the frame's image space is on.
    pub fn image_space(&self) -> bool {
        self.exposure_white || self.exposure_cinematic
    }
}

/// The share of the seen triangles whose light the exposure brings to 1
/// or below (`frame_exposure_seen_light_v1`): chosen.
pub const EXPOSURE_QUANTILE: f64 = 0.9;

/// The seen light below which a view takes this floor's scale
/// (`frame_exposure_seen_light_floor_v1`, `--exposure-eye-floor`): at most
/// four times the lit colour. Chosen below every lit view the opening's
/// window measured (0.52 to 0.68 outside, 0.65 to 1.53 in the vaults) and
/// far above the room of 730 s (0.0013).
pub const EXPOSURE_FLOOR: f32 = 0.25;

/// What maps a lit colour to the frame ([`Light::exposure`]), each part
/// under its own switch; with every part `None` a lit colour is as before
/// (`light_sum_clamped_v1`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Exposure {
    /// [`hypotheses::EXPOSURE_EYE`]: the eye's reading is asked
    /// (`--exposure-eye`); [`Light::expose_seen`] sets `scale`.
    pub eye: bool,
    /// [`hypotheses::EXPOSURE_FLOOR`]: the floor is asked
    /// (`--exposure-eye-floor`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// [`hypotheses::EXPOSURE_VISIBLE`]: the points given are what the
    /// eye sees, the nearest face at each cell of a grid over the frame
    /// (`fo4_render::seen_visible`; the viewer's `--exposure-eye-visible`),
    /// not the centroids of the lit triangles in view.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub visible: bool,
    /// The factor a lit colour is multiplied by after the light (before the
    /// fog): one over the mean light falling on what the eye sees.
    pub scale: Option<f32>,
    /// [`hypotheses::IMAGE_SPACE_WHITE`]: after the fog, each channel `x`
    /// becomes `x (1 + x / W^2) / (1 + x)`.
    pub white: Option<f32>,
    /// [`hypotheses::IMAGE_SPACE_CINEMATIC`]: saturation, brightness,
    /// contrast (the image space's CNAM words), applied after the white.
    pub cinematic: Option<[f32; 3]>,
    /// The tint's amount and colour (TNAM words), after the cinematic.
    pub tint: Option<[f32; 4]>,
}
impl Exposure {
    /// Whether any part changes a lit colour.
    pub fn any(&self) -> bool {
        self.scale.is_some()
            || self.white.is_some()
            || self.cinematic.is_some()
            || self.tint.is_some()
    }
    /// A lit colour after the fog: the white's tone curve, then the
    /// cinematic values and the tint (each part only where set). The
    /// scale is applied by the renderer before the fog.
    pub fn map(&self, c: [f32; 3]) -> [f32; 3] {
        let mut c = c;
        if let Some(w) = self.white {
            for x in &mut c {
                let v = x.max(0.);
                *x = v * (1. + v / (w * w)) / (1. + v);
            }
        }
        if let Some([saturation, brightness, contrast]) = self.cinematic {
            let grey = (c[0] + c[1] + c[2]) / 3.;
            for x in &mut c {
                *x = grey + saturation * (*x - grey);
                *x *= brightness;
                *x = 0.5 + contrast * (*x - 0.5);
            }
        }
        if let Some([amount, r, g, b]) = self.tint {
            let grey = (c[0] + c[1] + c[2]) / 3.;
            for (x, t) in c.iter_mut().zip([r, g, b]) {
                *x += amount * (t * grey - *x);
            }
        }
        c
    }
}

/// The caller's assumptions for an exterior frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Assumptions {
    /// `--assume-hour H`: the game hour (0 to 24).
    pub hour: Option<f64>,
    /// `--assume-weather N`: the index of the climate's weather list entry.
    pub weather: Option<usize>,
}

/// A placed light as a point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub position: [f64; 3],
    pub radius: f64,
    /// Linear RGB, the stored bytes over 255.
    pub colour: [f32; 3],
}

/// The placed lights bounded by a uniform grid of cubes: each cube lists,
/// increasing, the points whose sphere reaches it ([`Grid::of`]). A point
/// adds nothing at or beyond its radius (`light_point_linear_cutoff_v1`), so
/// the points listed at a position's cube give the light every point gives
/// there, summed in the same order ([`Light::at_some`]): an exact bound of
/// the work, not a reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// The lowest corner of the grid (game units).
    pub origin: [f64; 3],
    /// The side of a cube.
    pub cell: f64,
    /// Cubes along X, Y and Z.
    pub dims: [u32; 3],
    /// Per cube (X fastest, then Y, then Z): the first index into
    /// `indices` and how many.
    pub cells: Vec<(u32, u32)>,
    /// Point indices, increasing within each cube.
    pub indices: Vec<u32>,
}

impl Grid {
    /// What a cube's box is widened by on every side before a sphere is
    /// tested against it, so that a position a renderer computes in single
    /// precision near a cube's face still finds every point that reaches it.
    pub const MARGIN: f64 = 1.0;
    /// The first side tried; doubled until the grid fits the bounds below.
    pub const FIRST_CELL: f64 = 128.0;
    /// At most this many cubes, and this many listed indices.
    pub const MAX_CELLS: u64 = 1 << 18;
    pub const MAX_INDICES: u64 = 1 << 22;

    /// The grid of `points`; `None` when no point has a positive, finite
    /// radius and position.
    pub fn of(points: &[Point]) -> Option<Grid> {
        let usable = |p: &Point| {
            p.radius > 0. && p.radius.is_finite() && p.position.iter().all(|x| x.is_finite())
        };
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in points.iter().filter(|p| usable(p)) {
            for k in 0..3 {
                lo[k] = lo[k].min(p.position[k] - p.radius - Self::MARGIN);
                hi[k] = hi[k].max(p.position[k] + p.radius + Self::MARGIN);
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        let mut cell = Self::FIRST_CELL;
        loop {
            let dims: [u32; 3] = std::array::from_fn(|k| {
                (((hi[k] - lo[k]) / cell).floor() as u32).saturating_add(1)
            });
            let cells = dims.iter().map(|d| u64::from(*d)).product::<u64>();
            let span = |p: &Point, k: usize| {
                let a = ((p.position[k] - p.radius - Self::MARGIN - lo[k]) / cell)
                    .floor()
                    .max(0.) as u32;
                let b = (((p.position[k] + p.radius + Self::MARGIN - lo[k]) / cell).floor() as u32)
                    .min(dims[k] - 1);
                (a, b)
            };
            let listed: u64 = points
                .iter()
                .filter(|p| usable(p))
                .map(|p| {
                    (0..3)
                        .map(|k| {
                            let (a, b) = span(p, k);
                            u64::from(b - a + 1)
                        })
                        .product::<u64>()
                })
                .sum();
            if cells > Self::MAX_CELLS || listed > Self::MAX_INDICES {
                cell *= 2.;
                continue;
            }
            // The cubes each point's sphere reaches, the point's box widened
            // by the margin against each cube's.
            let reaches = |p: &Point, c: [u32; 3]| {
                let mut d2 = 0.;
                for k in 0..3 {
                    let a = lo[k] + f64::from(c[k]) * cell - Self::MARGIN;
                    let b = a + cell + 2. * Self::MARGIN;
                    let x = p.position[k];
                    let e = if x < a {
                        a - x
                    } else if x > b {
                        x - b
                    } else {
                        0.
                    };
                    d2 += e * e;
                }
                d2 < p.radius * p.radius
            };
            let n = cells as usize;
            let mut count = vec![0u32; n];
            let index = |c: [u32; 3]| {
                c[0] as usize
                    + dims[0] as usize * (c[1] as usize + dims[1] as usize * c[2] as usize)
            };
            let each = |p: &Point, f: &mut dyn FnMut(usize)| {
                let (x0, x1) = span(p, 0);
                let (y0, y1) = span(p, 1);
                let (z0, z1) = span(p, 2);
                for z in z0..=z1 {
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            if reaches(p, [x, y, z]) {
                                f(index([x, y, z]));
                            }
                        }
                    }
                }
            };
            for p in points.iter().filter(|p| usable(p)) {
                each(p, &mut |c| count[c] += 1);
            }
            let mut cells_out = Vec::with_capacity(n);
            let mut first = 0u32;
            for c in &count {
                cells_out.push((first, 0u32));
                first += c;
            }
            let mut indices = vec![0u32; first as usize];
            for (k, p) in points.iter().enumerate().filter(|(_, p)| usable(p)) {
                each(p, &mut |c| {
                    let (f, m) = &mut cells_out[c];
                    indices[(*f + *m) as usize] = k as u32;
                    *m += 1;
                });
            }
            return Some(Grid {
                origin: lo,
                cell,
                dims,
                cells: cells_out,
                indices,
            });
        }
    }

    /// The points listed at `p`'s cube (none outside the grid).
    pub fn at(&self, p: [f64; 3]) -> &[u32] {
        let mut c = [0usize; 3];
        for k in 0..3 {
            let x = (p[k] - self.origin[k]) / self.cell;
            if !(x >= 0. && x < f64::from(self.dims[k])) {
                return &[];
            }
            c[k] = (x.floor() as usize).min(self.dims[k] as usize - 1);
        }
        let i = c[0] + self.dims[0] as usize * (c[1] + self.dims[1] as usize * c[2]);
        let (f, n) = self.cells[i];
        &self.indices[f as usize..(f + n) as usize]
    }
}

/// Fog by distance from the eye: the fraction of `colour` is
/// `min(clamp((d - near) / (far - near), 0, 1), max)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fog {
    pub near: f64,
    pub far: f64,
    pub max: f32,
    pub colour: [f32; 3],
}
impl Fog {
    /// The fraction of fog at distance `d`.
    pub fn fraction(&self, d: f64) -> f32 {
        let span = self.far - self.near;
        let t = if span > 0. {
            ((d - self.near) / span).clamp(0., 1.)
        } else if d >= self.far {
            1.
        } else {
            0.
        };
        (t as f32).min(self.max).max(0.)
    }
}

/// The sky gradient by the sine of a view direction's elevation `s`:
/// `horizon` at and below 0, `lower` at [`Sky::LOWER_AT`], `zenith` at 1,
/// linear between (the stops are presentation).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Sky {
    pub zenith: [f32; 3],
    pub lower: [f32; 3],
    pub horizon: [f32; 3],
}
impl Sky {
    /// The sine of the elevation of the lower sky's stop (presentation).
    pub const LOWER_AT: f32 = 0.25;
    /// The colour at elevation sine `s`.
    pub fn colour(&self, s: f32) -> [f32; 3] {
        let mix =
            |a: [f32; 3], b: [f32; 3], t: f32| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
        if s <= 0. {
            self.horizon
        } else if s < Self::LOWER_AT {
            mix(self.horizon, self.lower, s / Self::LOWER_AT)
        } else {
            mix(
                self.lower,
                self.zenith,
                ((s - Self::LOWER_AT) / (1. - Self::LOWER_AT)).min(1.),
            )
        }
    }
}

/// What a frame's renderers apply, in game axes (Z up), linear RGB.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Light {
    /// Added to every lit surface.
    pub ambient: [f32; 3],
    /// An ambient cube by the surface's facing: entries for a normal toward
    /// +X, -X, +Y, -Y, +Z, -Z, weighted by the squares of the normal's
    /// components.
    pub cube: Option<[[f32; 3]; 6]>,
    /// A directional light: its colour and the unit vector toward it.
    pub directional: Option<([f32; 3], [f64; 3])>,
    pub points: Vec<Point>,
    pub fog: Option<Fog>,
    pub sky: Option<Sky>,
    /// Whether anything lights surfaces (ambient, cube, directional or
    /// points read); without it only the sky or nothing applies.
    pub lit: bool,
    /// Every rule relied on.
    pub rules: BTreeSet<&'static str>,
    /// One line per reading, for the overlay (`light\t...`).
    pub report: Vec<String>,
    /// What maps a lit colour to the frame beyond the clamp (`--exposure-*`).
    pub exposure: Exposure,
}
impl Light {
    /// Under [`hypotheses::EXPOSURE_EYE`] (asked by `--exposure-eye`): the
    /// scale a lit colour takes, one over the light that nine in ten
    /// ([`EXPOSURE_QUANTILE`]) of `points` stay at or below (the triangles
    /// of the lit draws the camera sees, each a centroid and a unit normal
    /// toward the eye, chosen by the renderer: `fo4_render::seen_lit_points`),
    /// each one's light the mean over the three channels of [`Light::at`];
    /// nothing when that light is not positive, no point is given or the
    /// reading is not asked. Returns that light when it applied.
    pub fn expose_seen(&mut self, points: &[([f64; 3], [f64; 3])]) -> Option<f32> {
        self.expose_seen_with(points, None)
    }
    /// [`Light::expose_seen`], the light at each point summed over the
    /// points `grid` lists there ([`Grid::at`]; the same light); with the
    /// floor asked ([`Exposure::floor`]) a view whose light is below
    /// [`EXPOSURE_FLOOR`], or that has none, takes one over the floor.
    pub fn expose_seen_with(
        &mut self,
        points: &[([f64; 3], [f64; 3])],
        grid: Option<&Grid>,
    ) -> Option<f32> {
        if !self.exposure.eye || !self.lit {
            return None;
        }
        // What the points are (`frame_exposure_seen_visible_v1`: what the
        // eye sees at the cells of a grid; else lit triangles in view).
        let (none, what) = if self.exposure.visible {
            self.rules.insert(hypotheses::EXPOSURE_VISIBLE);
            ("no lit face seen", "points of lit faces the eye sees")
        } else {
            ("no lit triangle in view", "lit triangles in view")
        };
        if points.is_empty() {
            if self.exposure.floor {
                return Some(self.floored(none));
            }
            self.report.push(format!("exposure: {none}, no scale"));
            return None;
        }
        let mut each: Vec<f32> = points
            .iter()
            .map(|(p, n)| {
                let l = match grid {
                    Some(g) => self.at_some(*p, *n, g.at(*p).iter().map(|k| *k as usize)),
                    None => self.at(*p, *n),
                };
                l.iter().sum::<f32>() / 3.
            })
            .collect();
        each.sort_by(f32::total_cmp);
        let at = |q: f64| each[((each.len() - 1) as f64 * q).floor() as usize];
        let (median, mean) = (
            at(0.5),
            each.iter().map(|x| f64::from(*x)).sum::<f64>() / each.len() as f64,
        );
        // The light nine in ten of the seen triangles stay at or below.
        let l = at(EXPOSURE_QUANTILE);
        let under = matches!(
            l.partial_cmp(&EXPOSURE_FLOOR),
            None | Some(std::cmp::Ordering::Less)
        );
        if self.exposure.floor && under {
            return Some(self.floored(&format!(
                "the light nine in ten of {} {what} stay at or below {l:.4}",
                points.len()
            )));
        }
        if !(l > 0. && l.is_finite()) {
            self.report
                .push("exposure: no light falls on what is seen, no scale".into());
            return None;
        }
        self.exposure.scale = Some(1. / l);
        self.rules.insert(hypotheses::EXPOSURE_EYE);
        self.report.push(format!(
            "exposure: the light nine in ten of {} {what} stay at or below {l:.4} (median {median:.4}, mean {mean:.4}; face normals toward the eye, three channels); lit colours times {:.4}",
            points.len(),
            1. / l
        ));
        Some(l)
    }
    /// The floor's scale ([`hypotheses::EXPOSURE_FLOOR`]), with its line.
    fn floored(&mut self, why: &str) -> f32 {
        self.exposure.scale = Some(1. / EXPOSURE_FLOOR);
        self.rules.insert(hypotheses::EXPOSURE_EYE);
        self.rules.insert(hypotheses::EXPOSURE_FLOOR);
        self.report.push(format!(
            "exposure: {why}, under the floor {EXPOSURE_FLOOR} ({}): lit colours times {:.4}",
            hypotheses::EXPOSURE_FLOOR,
            1. / EXPOSURE_FLOOR
        ));
        EXPOSURE_FLOOR
    }
    /// The light falling on a surface at `p` with unit normal `n` (game
    /// axes), before it multiplies the surface colour: ambient, the cube,
    /// the directional term and the points (`light_sum_clamped_v1` clamps
    /// the product, not this sum).
    pub fn at(&self, p: [f64; 3], n: [f64; 3]) -> [f32; 3] {
        self.at_some(p, n, 0..self.points.len())
    }
    /// [`Light::at`] with only the points `which` (indices into `points`):
    /// a renderer passes those whose sphere reaches a draw, which gives the
    /// same light, since a point adds nothing at or beyond its radius.
    pub fn at_some(
        &self,
        p: [f64; 3],
        n: [f64; 3],
        which: impl IntoIterator<Item = usize>,
    ) -> [f32; 3] {
        let mut c = self.ambient;
        if let Some(cube) = &self.cube {
            for axis in 0..3 {
                let w = (n[axis] * n[axis]) as f32;
                let e = cube[2 * axis + usize::from(n[axis] < 0.)];
                for k in 0..3 {
                    c[k] += w * e[k];
                }
            }
        }
        if let Some((colour, toward)) = &self.directional {
            let d = (0..3).map(|i| n[i] * toward[i]).sum::<f64>().max(0.) as f32;
            for k in 0..3 {
                c[k] += colour[k] * d;
            }
        }
        for k in which {
            let Some(point) = self.points.get(k) else {
                continue;
            };
            let v: [f64; 3] = std::array::from_fn(|i| point.position[i] - p[i]);
            let d = v.iter().map(|x| x * x).sum::<f64>().sqrt();
            if d >= point.radius || d <= 0. {
                continue;
            }
            let facing = ((0..3).map(|i| n[i] * v[i]).sum::<f64>() / d).max(0.);
            let w = ((1. - d / point.radius) * facing) as f32;
            for (value, colour) in c.iter_mut().zip(point.colour) {
                *value += colour * w;
            }
        }
        c
    }
}

/// The overlay lines of `light`: one `light` line per reading, then the
/// rules.
pub fn overlay(light: &Light) -> String {
    let mut out = String::new();
    for line in &light.report {
        out.push_str("light\t");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("light rules\t");
    out.push_str(&light.rules.iter().copied().collect::<Vec<_>>().join(" "));
    out.push('\n');
    out
}

/// Values of CELL.XCLL or of an LGTM's DATA and DALC, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CellValues {
    pub ambient: Option<[u8; 4]>,
    pub directional: Option<[u8; 4]>,
    /// `Directional Rotation XY`, `Directional Rotation Z`.
    pub rotation: [Option<i32>; 2],
    pub fade: Option<f32>,
    pub fog_near_colour: Option<[u8; 4]>,
    pub fog_far_colour: Option<[u8; 4]>,
    pub fog_near: Option<f32>,
    pub fog_far: Option<f32>,
    pub fog_power: Option<f32>,
    pub fog_max: Option<f32>,
    pub clip: Option<f32>,
    /// The directional ambient colours `X+` to `Z-`, and its `Scale`.
    pub cube: [Option<[u8; 4]>; 6],
    pub cube_scale: Option<f32>,
    /// `Inherits` (XCLL only): the word and its printed labels.
    pub inherits: Option<(u32, Option<&'static str>)>,
}

const AXES: [&str; 6] = ["X+", "X-", "Y+", "Y-", "Z+", "Z-"];

/// The values of `record`'s lighting subrecord `data` (XCLL, or DATA of an
/// LGTM) and its directional ambient (in XCLL itself, or the LGTM's DALC),
/// read from the record's layout fields; `None` when the record has no
/// `data` subrecord.
pub fn cell_values(
    record: &Record,
    fields: &[layout::Field],
    data: &[u8; 4],
    cube: &[u8; 4],
) -> Option<CellValues> {
    if !record.subrecords.iter().any(|s| s.signature == *data) {
        return None;
    }
    let find = |tag: &[u8; 4], label: &str| -> Option<&layout::Value> {
        fields.iter().find_map(|f| {
            (record.subrecords.get(f.subrecord)?.signature == *tag && f.label == label)
                .then_some(&f.value)
        })
    };
    let quoted = |s: &str| format!("\"{s}\"");
    let colour = |tag: &[u8; 4], label: &str| match find(tag, label) {
        Some(layout::Value::Color(c)) => Some(*c),
        _ => None,
    };
    let float = |label: &str| match find(data, &quoted(label)) {
        Some(layout::Value::Float(v)) => Some(*v),
        _ => None,
    };
    let int = |label: &str| match find(data, &quoted(label)) {
        Some(layout::Value::I32(v)) => Some(*v),
        _ => None,
    };
    let axis = |k: usize| {
        colour(
            cube,
            &format!(
                "Directional Ambient Lighting Colors/Directional / \"{}\"",
                AXES[k]
            ),
        )
    };
    let inherits = match find(data, "\"Inherits\"") {
        Some(layout::Value::Flags {
            raw,
            printed_labels,
            fully_named,
        }) => Some((*raw, printed_labels.filter(|_| *fully_named))),
        _ => None,
    };
    let cube_scale = match find(cube, "Directional Ambient Lighting Colors / \"Scale\"") {
        Some(layout::Value::Float(v)) => Some(*v),
        _ => None,
    };
    Some(CellValues {
        ambient: colour(data, &quoted("Ambient Color")),
        directional: colour(data, &quoted("Directional Color")),
        rotation: [
            int("Directional Rotation XY"),
            int("Directional Rotation Z"),
        ],
        fade: float("Directional Fade"),
        fog_near_colour: colour(data, &quoted("Fog Color Near")),
        fog_far_colour: colour(data, &quoted("Fog Color Far")),
        fog_near: float("Fog Near"),
        fog_far: float("Fog Far"),
        fog_power: float("Fog Power"),
        fog_max: float("Fog Max"),
        clip: float("Fog Clip Distance"),
        cube: std::array::from_fn(axis),
        cube_scale,
        inherits,
    })
}

/// The printed labels of an `Inherits` word and the fields of
/// [`CellValues`] each takes from the template.
const INHERITED: [&str; 7] = [
    "Ambient Color",
    "Directional Color",
    "Fog Color",
    "Fog Near",
    "Fog Far",
    "Directional Rotation",
    "Fog Max",
];

/// `cell` with the fields its `Inherits` labels name taken from
/// `template` (`cell_light_template_inherits_v1`); the names taken.
fn inherit(
    cell: &CellValues,
    template: &CellValues,
    labels: &str,
) -> (CellValues, Vec<&'static str>) {
    let mut out = *cell;
    let mut taken = Vec::new();
    for name in labels.split(", ") {
        let Some(&known) = INHERITED.iter().find(|n| **n == name) else {
            continue;
        };
        match known {
            "Ambient Color" => out.ambient = template.ambient,
            "Directional Color" => out.directional = template.directional,
            "Fog Color" => {
                out.fog_near_colour = template.fog_near_colour;
                out.fog_far_colour = template.fog_far_colour;
            }
            "Fog Near" => out.fog_near = template.fog_near,
            "Fog Far" => out.fog_far = template.fog_far,
            "Directional Rotation" => out.rotation = template.rotation,
            _ => out.fog_max = template.fog_max,
        }
        taken.push(known);
    }
    (out, taken)
}

/// The f32 layout field `label` of `record`'s subrecord `tag`.
fn float_field(
    record: &Record,
    fields: &[layout::Field],
    tag: &[u8; 4],
    label: &str,
) -> Option<f32> {
    fields.iter().find_map(|f| {
        match (
            record.subrecords.get(f.subrecord)?.signature == *tag && f.label == label,
            &f.value,
        ) {
            (true, layout::Value::Float(v)) if v.is_finite() => Some(*v),
            _ => None,
        }
    })
}

fn rgb(c: [u8; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]].map(|v| f32::from(v) / 255.)
}

fn text(c: [f32; 3]) -> String {
    format!("{:.3},{:.3},{:.3}", c[0], c[1], c[2])
}

/// The unit vector toward a directional light whose stored rotation is
/// `xy` and `z` degrees (`cell_light_ambient_directional_v1`): `z` turns
/// counter-clockwise about +Z from +X, `xy` raises it above the plane.
pub fn toward(xy: i32, z: i32) -> [f64; 3] {
    let (e, a) = (f64::from(xy).to_radians(), f64::from(z).to_radians());
    [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()]
}

/// The weather slot of `hour` for a climate whose TNAM is `times`
/// (`climate_hour_slot_v1`): its first four bytes read as times of day in
/// units of ten minutes; before the first and from the fourth on, slot 3;
/// from the first, slot 0; from the second, slot 1; from the third, slot 2.
pub fn hour_slot(times: [u8; 6], hour: f64) -> usize {
    let minutes = hour.rem_euclid(24.) * 60.;
    let at = |k: usize| f64::from(times[k]) * 10.;
    if minutes < at(0) || minutes >= at(3) {
        3
    } else if minutes < at(1) {
        0
    } else if minutes < at(2) {
        1
    } else {
        2
    }
}

/// The rows of a weather's colour table read as the sky's zenith, lower
/// sky and horizon (`weather_sky_rows_v1`), and as fog (`weather_fog_rows_v1`).
pub const SKY_ROWS: [usize; 3] = [0, 7, 8];
pub const FOG_ROW: usize = 1;

fn say(report: &mut Vec<String>, s: String) {
    report.push(s);
}

fn error(
    code: &str,
    file: &str,
    expected: impl std::fmt::Display,
    found: impl std::fmt::Display,
) -> Error {
    Error::new(code, file, 0, expected, found)
}

/// The climate and weather of an exterior scene, as read.
/// The image space record's words a frame applies (`--exposure-white`,
/// `--exposure-cinematic`): HNAM word 5 as the white, CNAM's three words as
/// saturation, brightness and contrast, TNAM's four as the tint's amount
/// and colour (labels from prior general knowledge of the record, not from
/// the data; `spec/seen-census.txt`, section 4).
fn read_image_space(
    store: &Store,
    light: &mut Light,
    found: Option<(FormKey, String)>,
    switches: Switches,
    b: &Budget,
) -> Result<()> {
    let Some((key, from)) = found else {
        say(
            &mut light.report,
            "image space: none found for the frame".into(),
        );
        return Ok(());
    };
    let Some(r) = store.raw(&key, b)? else {
        say(&mut light.report, "image space: no winning record".into());
        return Ok(());
    };
    let words = |sig: &[u8; 4]| -> Vec<f32> {
        r.subrecords
            .iter()
            .find(|s| s.signature == *sig)
            .map(|s| {
                r.data(s)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default()
    };
    if switches.exposure_white {
        match words(b"HNAM").get(5).copied() {
            Some(w) if w > 0. && w.is_finite() => {
                light.exposure.white = Some(w);
                light.rules.insert(hypotheses::IMAGE_SPACE_WHITE);
                say(
                    &mut light.report,
                    format!("image space white {w} (HNAM word 5) from {from}"),
                );
            }
            _ => say(
                &mut light.report,
                format!("image space white: HNAM word 5 not a positive number in {from}"),
            ),
        }
    }
    if switches.exposure_cinematic {
        let c = words(b"CNAM");
        let t = words(b"TNAM");
        match (
            <[f32; 3]>::try_from(c.as_slice()),
            <[f32; 4]>::try_from(t.as_slice()),
        ) {
            (Ok(c), Ok(t)) if c.iter().chain(&t).all(|v| v.is_finite()) => {
                light.exposure.cinematic = Some(c);
                light.exposure.tint = Some(t);
                light.rules.insert(hypotheses::IMAGE_SPACE_CINEMATIC);
                say(
                    &mut light.report,
                    format!(
                        "image space saturation {} brightness {} contrast {} (CNAM), tint {} of {} {} {} (TNAM) from {from}",
                        c[0], c[1], c[2], t[0], t[1], t[2], t[3]
                    ),
                );
            }
            _ => say(
                &mut light.report,
                format!("image space cinematic: CNAM or TNAM not as measured in {from}"),
            ),
        }
    }
    Ok(())
}

struct Exterior {
    weather: Weather,
    times: Option<[u8; 6]>,
    rule: Option<&'static str>,
    entries: usize,
}

fn exterior(
    store: &Store,
    cell: &FormKey,
    a: &Assumptions,
    b: &Budget,
) -> Result<std::result::Result<Exterior, String>> {
    let Some(world) = store.world(cell) else {
        return Ok(Err("the cell has no worldspace".into()));
    };
    let Some(w) = store.get::<Wrld>(&world, b)? else {
        return Ok(Err("no winning worldspace record".into()));
    };
    let Some(climate) = w.data.climate.as_ref().and_then(|r| r.key.clone()) else {
        return Ok(Err("the worldspace names no climate".into()));
    };
    let Some((r, plugin)) = store.raw_with_plugin(&climate, b)? else {
        return Ok(Err("no winning climate record".into()));
    };
    let ctx = Context {
        order: store.order(),
        plugin,
    };
    let c = Climate::read(&r, ctx, b)?;
    let (entry, rule) = match a.weather {
        Some(n) => (
            c.weathers.get(n).ok_or_else(|| {
                error(
                    "AssumedWeatherIndex",
                    &r.source.file,
                    format!("an index below {}", c.weathers.len()),
                    n,
                )
            })?,
            None,
        ),
        None if c.weathers.len() == 1 => (&c.weathers[0], Some(hypotheses::CLIMATE_SINGLE_WEATHER)),
        None => {
            return Ok(Err(format!(
                "the climate lists {} weathers: the caller's --assume-weather N picks one",
                c.weathers.len()
            )))
        }
    };
    let Some(key) = entry.weather.key.clone() else {
        return Ok(Err("the weather list entry names no record".into()));
    };
    let Some((wr, wp)) = store.raw_with_plugin(&key, b)? else {
        return Ok(Err("no winning weather record".into()));
    };
    let weather = Weather::read(
        &wr,
        Context {
            order: store.order(),
            plugin: wp,
        },
        b,
    )?;
    Ok(Ok(Exterior {
        weather,
        times: c.times,
        rule,
        entries: c.weathers.len(),
    }))
}

/// The light of a frame of `scene` (its cell's kind decides interior or
/// exterior readings) under `switches` and the caller's `assumptions`;
/// `None` with every switch off. Placed lights are the scene's instances
/// whose base is a LIGH with its data, less those the enable parent rules
/// leave out ([`crate::Instance::disabled_by_enable_parent`]).
pub fn frame(
    store: &Store,
    scene: &Scene,
    switches: Switches,
    assumptions: Assumptions,
    b: &Budget,
) -> Result<Option<Light>> {
    if !switches.any() {
        return Ok(None);
    }
    let mut light = Light {
        ambient: [0.; 3],
        cube: None,
        directional: None,
        points: Vec::new(),
        fog: None,
        sky: None,
        lit: false,
        rules: BTreeSet::new(),
        report: Vec::new(),
        exposure: Exposure {
            eye: switches.exposure_eye,
            floor: switches.exposure_eye && switches.exposure_floor,
            ..Exposure::default()
        },
    };
    let interior = scene.kind == Some(CellKind::Interior);
    // The frame's image space: the interior cell's XCIM, or the weather
    // slot's IMSP entry (found below).
    let mut image_space: Option<(FormKey, String)> = None;
    if interior && switches.image_space() {
        image_space = scene
            .cell
            .data
            .image_space
            .as_ref()
            .and_then(|r| r.key.clone())
            .map(|k| (k, "the interior cell's image space (XCIM)".to_string()));
    }
    if interior && (switches.cell || switches.fog) {
        let cell = cell_values(
            &scene.cell.record,
            &scene.cell.data.common.layout_fields,
            b"XCLL",
            b"XCLL",
        );
        let template = scene
            .lighting_template
            .as_ref()
            .and_then(|t| cell_values(&t.record, &t.data.common.layout_fields, b"DATA", b"DALC"));
        let values = match (cell, template, switches.inherits) {
            (Some(c), Some(t), true) => match c.inherits {
                Some((raw, Some(labels))) => {
                    let (v, taken) = inherit(&c, &t, labels);
                    light.rules.insert(hypotheses::LIGHT_INHERITS);
                    say(
                        &mut light.report,
                        format!(
                            "inherits word {raw:#x}: from the template {}",
                            if taken.is_empty() {
                                "nothing".into()
                            } else {
                                taken.join(", ")
                            }
                        ),
                    );
                    Some(v)
                }
                Some((raw, None)) => {
                    say(&mut light.report, format!("inherits word {raw:#x} has no fully named labels: the cell's own values"));
                    Some(c)
                }
                None => Some(c),
            },
            (None, Some(t), true) => {
                light.rules.insert(hypotheses::LIGHT_INHERITS);
                say(
                    &mut light.report,
                    "the cell stores no XCLL: every value from its template".into(),
                );
                Some(t)
            }
            (Some(c), _, _) => Some(c),
            (None, _, _) => {
                say(&mut light.report, "the cell stores no XCLL: unlit".into());
                None
            }
        };
        if let Some(v) = values {
            if switches.cell {
                light.rules.insert(hypotheses::LIGHT_CELL);
                light.ambient = v.ambient.map(rgb).unwrap_or([0.; 3]);
                if let (Some(c), [Some(xy), Some(z)]) = (v.directional, v.rotation) {
                    light.directional = Some((rgb(c), toward(xy, z)));
                    say(
                        &mut light.report,
                        format!(
                            "cell ambient {} directional {} rotation {xy} {z}",
                            text(light.ambient),
                            text(rgb(c))
                        ),
                    );
                } else {
                    say(
                        &mut light.report,
                        format!(
                            "cell ambient {}; no directional colour or rotation",
                            text(light.ambient)
                        ),
                    );
                }
                light.lit = true;
            }
            if switches.fog {
                match (v.fog_near, v.fog_far, v.fog_near_colour) {
                    (Some(near), Some(far), Some(c)) => {
                        let fog = Fog {
                            near: f64::from(near),
                            far: f64::from(far),
                            max: v.fog_max.unwrap_or(1.),
                            colour: rgb(c),
                        };
                        light.rules.insert(hypotheses::LIGHT_CELL_FOG);
                        say(
                            &mut light.report,
                            format!(
                                "cell fog {} to {} max {} colour {}",
                                fog.near,
                                fog.far,
                                fog.max,
                                text(fog.colour)
                            ),
                        );
                        light.fog = Some(fog);
                    }
                    _ => say(
                        &mut light.report,
                        "cell fog: near, far or colour not stored".into(),
                    ),
                }
            }
        }
    }
    if !interior && (switches.weather() || switches.image_space()) {
        match exterior(store, &scene.cell.key, &assumptions, b)? {
            Err(why) => say(&mut light.report, format!("weather undecided: {why}")),
            Ok(e) => {
                light.rules.extend(e.rule);
                say(
                    &mut light.report,
                    match e.rule {
                        Some(_) => "weather: the climate's one weather".into(),
                        None => format!(
                            "weather: the caller's entry {} of {}",
                            assumptions.weather.unwrap_or_default(),
                            e.entries
                        ),
                    },
                );
                let slot = match (assumptions.hour, e.times) {
                    (Some(h), Some(t)) => {
                        light.rules.insert(hypotheses::CLIMATE_HOUR_SLOT);
                        Some(hour_slot(t, h))
                    }
                    (None, _) => {
                        say(
                            &mut light.report,
                            "no hour: the caller's --assume-hour H picks the weather's slot".into(),
                        );
                        None
                    }
                    (_, None) => {
                        say(&mut light.report, "the climate stores no times".into());
                        None
                    }
                };
                if let Some(slot) = slot.filter(|s| *s < e.weather.slots()) {
                    if switches.image_space() {
                        image_space = e
                            .weather
                            .image_spaces
                            .get(slot)
                            .and_then(|r| r.key.clone())
                            .map(|k| {
                                (
                                    k,
                                    format!("the weather's image space of slot {slot} (IMSP)"),
                                )
                            });
                    }
                    say(
                        &mut light.report,
                        format!(
                            "hour {} reads slot {slot} of {}",
                            assumptions.hour.unwrap_or_default(),
                            e.weather.slots()
                        ),
                    );
                    if switches.sky {
                        let row = |r: usize| e.weather.colour(r, slot).map(rgb);
                        if let (Some(z), Some(l), Some(h)) =
                            (row(SKY_ROWS[0]), row(SKY_ROWS[1]), row(SKY_ROWS[2]))
                        {
                            light.rules.insert(hypotheses::WEATHER_SKY);
                            say(
                                &mut light.report,
                                format!(
                                    "sky zenith {} lower {} horizon {}",
                                    text(z),
                                    text(l),
                                    text(h)
                                ),
                            );
                            light.sky = Some(Sky {
                                zenith: z,
                                lower: l,
                                horizon: h,
                            });
                        } else {
                            say(
                                &mut light.report,
                                "sky: the weather has no such rows".into(),
                            );
                        }
                    }
                    if switches.weather_ambient {
                        let d = e.weather.ambient[slot];
                        // A normal toward +axis reads the axis's "-" entry.
                        let cube: [[f32; 3]; 6] = std::array::from_fn(|k| {
                            let stored = d.axes[k ^ 1];
                            rgb(stored).map(|v| v * d.scale)
                        });
                        light.rules.insert(hypotheses::WEATHER_AMBIENT);
                        say(
                            &mut light.report,
                            format!(
                                "weather ambient cube up {} down {} scale {}",
                                text(cube[4]),
                                text(cube[5]),
                                d.scale
                            ),
                        );
                        light.cube = Some(cube);
                        light.lit = true;
                    }
                    if switches.weather_fog {
                        let night = slot == 3;
                        let f = |k: usize| e.weather.fog.get(k).copied();
                        let (near, far, max) = if night {
                            (f(2), f(3), f(7))
                        } else {
                            (f(0), f(1), f(6))
                        };
                        match (near, far, e.weather.colour(FOG_ROW, slot)) {
                            (Some(n), Some(fa), Some(c)) => {
                                let fog = Fog {
                                    near: f64::from(n),
                                    far: f64::from(fa),
                                    max: max.unwrap_or(1.),
                                    colour: rgb(c),
                                };
                                light.rules.insert(hypotheses::WEATHER_FOG);
                                say(
                                    &mut light.report,
                                    format!(
                                        "weather fog {} to {} max {} colour {}",
                                        fog.near,
                                        fog.far,
                                        fog.max,
                                        text(fog.colour)
                                    ),
                                );
                                light.fog = Some(fog);
                            }
                            _ => say(&mut light.report, "weather fog: values not stored".into()),
                        }
                    }
                } else if let Some(s) = slot {
                    say(
                        &mut light.report,
                        format!("slot {s} beyond the weather's {} slots", e.weather.slots()),
                    );
                }
            }
        }
    }
    if switches.image_space() {
        read_image_space(store, &mut light, image_space, switches, b)?;
    }
    if switches.points {
        let (mut adjusted, mut faded) = (0u64, 0u64);
        for i in &scene.instances {
            let Some(base) = i.base.as_ref() else {
                continue;
            };
            let Some(data) = base.light.as_ref() else {
                continue;
            };
            if i.disabled_by_enable_parent() {
                continue;
            }
            let Some(t) = i.reference.data.transform else {
                continue;
            };
            let mut radius = f64::from(data.radius);
            if switches.reference_radius {
                if let Some(x) = float_field(
                    &i.reference.record,
                    &i.reference.data.common.layout_fields,
                    b"XRDS",
                    "\"XRDS - Radius\"",
                ) {
                    radius += f64::from(x);
                    adjusted += 1;
                }
            }
            let mut colour = rgb(data.color);
            if switches.fade {
                if let Some(v) = float_field(
                    &base.record,
                    &base.common.layout_fields,
                    b"FNAM",
                    "\"FNAM - Fade value\"",
                ) {
                    colour = colour.map(|c| c * v.max(0.));
                    faded += 1;
                }
            }
            if radius <= 0. || !radius.is_finite() {
                continue;
            }
            b.reserve(&mut light.points, 1, "light points", 0)?;
            light.points.push(Point {
                position: [f64::from(t[0]), f64::from(t[1]), f64::from(t[2])],
                radius,
                colour,
            });
        }
        light.rules.insert(hypotheses::LIGHT_POINTS);
        say(
            &mut light.report,
            format!("{} placed lights as points", light.points.len()),
        );
        if switches.reference_radius {
            light.rules.insert(hypotheses::LIGHT_REFERENCE_RADIUS);
            say(
                &mut light.report,
                format!("{adjusted} radii with their reference's XRDS added"),
            );
        }
        if switches.fade {
            light.rules.insert(hypotheses::LIGHT_FADE);
            say(
                &mut light.report,
                format!("{faded} colours times their base's fade value"),
            );
        }
        light.lit |= !light.points.is_empty();
    }
    if light.lit || light.fog.is_some() {
        light.rules.insert(hypotheses::LIGHT_SUM);
    }
    Ok(Some(light))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_read_slots_by_the_climate_times() {
        let t = [30, 54, 102, 126, 0, 0];
        for (h, s) in [
            (0., 3),
            (4.99, 3),
            (5., 0),
            (8.9, 0),
            (9., 1),
            (16.99, 1),
            (17., 2),
            (20.9, 2),
            (21., 3),
            (23.9, 3),
            (29., 0),
        ] {
            assert_eq!(hour_slot(t, h), s, "{h}");
        }
    }

    #[test]
    fn the_points_grid_lists_every_point_that_reaches_a_position_and_sums_the_same() {
        // A deterministic scatter of points (a linear congruential walk).
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / f64::from(1u32 << 31)
        };
        let mut points = Vec::new();
        for _ in 0..300 {
            points.push(Point {
                position: [
                    next() * 4000. - 2000.,
                    next() * 3000. - 1500.,
                    next() * 800.,
                ],
                radius: 50. + next() * 600.,
                colour: [next() as f32, next() as f32, next() as f32],
            });
        }
        // Points that reach nothing are never listed.
        points.push(Point {
            position: [0.; 3],
            radius: 0.,
            colour: [1.; 3],
        });
        let g = Grid::of(&points).unwrap();
        assert!(g.cell >= Grid::FIRST_CELL);
        let light = Light {
            ambient: [0.01; 3],
            cube: None,
            directional: None,
            points: points.clone(),
            fog: None,
            sky: None,
            lit: true,
            rules: BTreeSet::new(),
            report: Vec::new(),
            exposure: Exposure::default(),
        };
        for _ in 0..5000 {
            let p = [
                next() * 5000. - 2500.,
                next() * 4000. - 2000.,
                next() * 1200. - 200.,
            ];
            let n = [next() - 0.5, next() - 0.5, next() - 0.5];
            let l = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            let n = n.map(|x| x / l);
            let listed = g.at(p);
            assert!(listed.windows(2).all(|w| w[0] < w[1]));
            for (k, q) in points.iter().enumerate() {
                let d = (0..3)
                    .map(|i| (q.position[i] - p[i]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if d < q.radius {
                    assert!(
                        listed.contains(&(k as u32)),
                        "point {k} at {d} of {}",
                        q.radius
                    );
                }
            }
            let some = light.at_some(p, n, listed.iter().map(|k| *k as usize));
            assert_eq!(some, light.at(p, n));
        }
        // Outside the grid nothing is listed.
        assert!(g.at([1e9, 0., 0.]).is_empty());
        assert!(Grid::of(&[]).is_none());
    }

    #[test]
    fn the_light_at_a_point_adds_ambient_cube_directional_and_points() {
        let mut l = Light {
            ambient: [0.1, 0.1, 0.1],
            cube: Some([
                [0.; 3],
                [0.; 3],
                [0.; 3],
                [0.; 3],
                [0.2, 0., 0.],
                [0., 0.3, 0.],
            ]),
            directional: Some(([0.5, 0.5, 0.5], [0., 0., 1.])),
            points: vec![Point {
                position: [0., 0., 100.],
                radius: 200.,
                colour: [1., 0., 0.],
            }],
            fog: None,
            sky: None,
            lit: true,
            rules: BTreeSet::new(),
            report: Vec::new(),
            exposure: Exposure::default(),
        };
        // Facing up under the point at distance 100: half its colour.
        let c = l.at([0., 0., 0.], [0., 0., 1.]);
        assert!((c[0] - (0.1 + 0.2 + 0.5 + 0.5)).abs() < 1e-6, "{c:?}");
        assert!((c[1] - (0.1 + 0.5)).abs() < 1e-6, "{c:?}");
        // Facing down: the cube's -Z entry, no directional, no point.
        let c = l.at([0., 0., 0.], [0., 0., -1.]);
        assert!(
            (c[1] - (0.1 + 0.3)).abs() < 1e-6 && (c[0] - 0.1).abs() < 1e-6,
            "{c:?}"
        );
        // Beyond the radius the point adds nothing.
        l.cube = None;
        l.directional = None;
        assert_eq!(l.at([0., 0., -150.], [0., 0., 1.]), [0.1; 3]);
    }

    #[test]
    fn fog_and_sky_are_linear_between_their_stops() {
        let f = Fog {
            near: 100.,
            far: 300.,
            max: 0.8,
            colour: [1.; 3],
        };
        assert_eq!(f.fraction(50.), 0.);
        assert_eq!(f.fraction(200.), 0.5);
        assert_eq!(f.fraction(1000.), 0.8);
        let s = Sky {
            zenith: [0., 0., 1.],
            lower: [0., 1., 0.],
            horizon: [1., 0., 0.],
        };
        assert_eq!(s.colour(-0.5), [1., 0., 0.]);
        assert_eq!(s.colour(Sky::LOWER_AT), [0., 1., 0.]);
        assert_eq!(s.colour(1.), [0., 0., 1.]);
        let mid = s.colour(Sky::LOWER_AT / 2.);
        assert!((mid[0] - 0.5).abs() < 1e-6 && (mid[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn inherits_takes_the_named_fields_from_the_template() {
        let cell = CellValues {
            ambient: Some([1, 1, 1, 0]),
            directional: Some([2, 2, 2, 0]),
            rotation: [Some(0), Some(90)],
            fade: None,
            fog_near_colour: Some([3, 3, 3, 0]),
            fog_far_colour: None,
            fog_near: Some(1.),
            fog_far: Some(2.),
            fog_power: None,
            fog_max: None,
            clip: None,
            cube: [None; 6],
            cube_scale: None,
            inherits: None,
        };
        let template = CellValues {
            ambient: Some([9, 9, 9, 0]),
            rotation: [Some(45), Some(180)],
            fog_near: Some(7.),
            ..cell
        };
        let (v, taken) = inherit(
            &cell,
            &template,
            "Ambient Color, Directional Rotation, Directional Fade, Light Fade Distances",
        );
        assert_eq!(taken, vec!["Ambient Color", "Directional Rotation"]);
        assert_eq!(v.ambient, Some([9, 9, 9, 0]));
        assert_eq!(v.rotation, [Some(45), Some(180)]);
        assert_eq!(v.fog_near, Some(1.));
        let d = toward(0, 90);
        assert!(d[0].abs() < 1e-12 && (d[1] - 1.).abs() < 1e-12 && d[2] == 0.);
    }

    #[test]
    fn switches_follow_their_flags() {
        let mut s = Switches::default();
        assert!(!s.any());
        assert!(s.set_flag("--light-points") && s.set_flag("--sky-weather"));
        assert!(!s.set_flag("--light-everything"));
        assert_eq!(s.flags(), vec!["--light-points", "--sky-weather"]);
        assert!(s.weather() && s.any());
    }

    /// The exposure's parts (`frame_exposure_seen_light_v1`,
    /// `image_space_hdr_white_v1`, `image_space_cinematic_tint_v1`): the
    /// scale is one over the light at the eye; a white of 1 leaves a colour
    /// as it is, another compresses it; neutral cinematic values and a tint
    /// of amount 0 change nothing.
    #[test]
    fn exposure_scales_by_the_light_at_the_eye_and_maps_by_the_image_space() {
        let mut l = Light {
            ambient: [2., 2., 2.],
            cube: None,
            directional: None,
            points: Vec::new(),
            fog: None,
            sky: None,
            lit: true,
            rules: BTreeSet::new(),
            report: Vec::new(),
            exposure: Exposure::default(),
        };
        let up = [0., 0., 1.];
        assert_eq!(l.expose_seen(&[([0.; 3], up)]), None);
        l.exposure.eye = true;
        assert_eq!(l.expose_seen(&[]), None);
        assert_eq!(l.expose_seen(&[([0.; 3], up), ([5.; 3], up)]), Some(2.));
        // The quantile, not the mean: nine dim points and one bright.
        l.points.push(Point {
            position: [100., 0., 1.],
            radius: 10.,
            colour: [30., 30., 30.],
        });
        let mut pts = vec![([0.; 3], up); 9];
        pts.push(([100., 0., 0.], up));
        assert_eq!(l.expose_seen(&pts), Some(2.));
        assert_eq!(l.exposure.scale, Some(0.5));
        assert!(l.rules.contains(hypotheses::EXPOSURE_EYE));
        // The grid's list gives the same reading.
        let grid = Grid::of(&l.points).unwrap();
        let mut g = l.clone();
        assert_eq!(g.expose_seen_with(&pts, Some(&grid)), Some(2.));
        assert_eq!(g.exposure.scale, Some(0.5));
        // The floor (`frame_exposure_seen_light_floor_v1`): a view seen
        // under it, or with nothing in view, takes one over the floor; a
        // view above it is as before.
        let mut d = l.clone();
        d.ambient = [0.01; 3];
        d.rules.clear();
        assert_eq!(d.expose_seen(&pts[..9]), Some(0.01));
        assert!(!d.rules.contains(hypotheses::EXPOSURE_FLOOR));
        d.exposure.floor = true;
        assert_eq!(d.expose_seen(&pts[..9]), Some(EXPOSURE_FLOOR));
        assert_eq!(d.exposure.scale, Some(1. / EXPOSURE_FLOOR));
        assert!(d.rules.contains(hypotheses::EXPOSURE_FLOOR));
        assert_eq!(d.expose_seen(&[]), Some(EXPOSURE_FLOOR));
        d.ambient = [2.; 3];
        assert_eq!(d.expose_seen(&pts[..9]), Some(2.));
        assert_eq!(d.exposure.scale, Some(0.5));
        // The switch: only with --exposure-eye.
        let mut s = Switches::default();
        assert!(s.set_flag("--exposure-eye-floor"));
        assert_eq!(s.flags(), vec!["--exposure-eye-floor"]);
        let c = [0.2, 1.5, 4.];
        let x = Exposure {
            white: Some(1.),
            ..Exposure::default()
        };
        for (a, b) in x.map(c).iter().zip(c) {
            assert!((a - b).abs() < 1e-6);
        }
        let x = Exposure {
            white: Some(2.),
            cinematic: Some([1., 1., 1.]),
            tint: Some([0., 1., 0., 0.]),
            ..Exposure::default()
        };
        let m = x.map([1., 1., 1.]);
        assert!((m[0] - 0.625).abs() < 1e-6, "{m:?}");
        let grey = Exposure {
            cinematic: Some([0., 1., 1.]),
            ..Exposure::default()
        };
        let g = grey.map([0.3, 0.6, 0.9]);
        assert!(
            g.iter()
                .all(|v| (v - g[0]).abs() < 1e-6 && (v - 0.6).abs() < 1e-6),
            "{g:?}"
        );
        assert!(!Exposure::default().any());
    }
}
