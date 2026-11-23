//! The game clock of animation controllers that nothing but the clock runs
//! (`spec/controller-clock.md`), under pending rules: a controller not run
//! by a manager starts when its model loads and runs on the game clock
//! (`nif_controller_clock_free_running_v1`); its time is folded into its
//! start and stop times by its cycle type (one rule per cycle type) and its
//! keys are read by their key type (one rule per key type). Nothing here is
//! measured; field layouts and enum values are nifxml's (`spec/nif.md`:
//! `TimeControllerFlags`, `CycleType`, `KeyType`, `Key`).
use crate::hypotheses as h;
use crate::model_policy::fields;
use fo4_core::{Budget, Result};
use fo4_nif::{Field, Nif, Value};

/// nifxml `KeyType`: linear, quadratic (forward and backward tangents
/// stored), tension bias continuity, XYZ rotation, constant (step).
pub const LINEAR: u32 = 1;
pub const QUADRATIC: u32 = 2;
pub const TBC: u32 = 3;
pub const XYZ: u32 = 4;
pub const CONSTANT: u32 = 5;
/// nifxml `CycleType`.
pub const CYCLE_LOOP: u32 = 0;
pub const CYCLE_REVERSE: u32 = 1;
pub const CYCLE_CLAMP: u32 = 2;

/// Readings of the clock that are off by default, one switch per pending
/// rule (`spec/controller-clock.md`, Effect fields on the clock and Moving
/// geometry). With every switch off the scene's rest tags and the draw
/// build are those of the alpha and UV clock alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// A free-running controller of an effect shader's emissive multiple
    /// (EffectShaderControlledVariable 0) runs on the clock and gives the
    /// base colour scale the frame multiplies in
    /// (`effect_clock_emissive_multiple_v1`).
    pub emissive_multiple: bool,
    /// The same for the emissive colour (EffectShaderControlledColor 0),
    /// the base colour's RGB (`effect_clock_emissive_colour_v1`).
    pub emissive_colour: bool,
    /// The same for the four falloff fields (EffectShaderControlledVariable
    /// 1 to 4), the angles read as degrees (`effect_clock_falloff_v1`).
    pub falloff: bool,
    /// A free-running transform controller's translation keys move its
    /// node on the clock (`nif_clock_transform_translation_v1`).
    pub translation: bool,
    /// The same for quaternion rotation keys
    /// (`nif_clock_transform_rotation_quaternion_v1`).
    pub rotation: bool,
    /// The same for rotation keys stored as three angle key groups
    /// (`nif_clock_transform_rotation_xyz_v1`).
    pub rotation_xyz: bool,
    /// The same for scale keys (`nif_clock_transform_scale_v1`).
    pub scale: bool,
}
impl Switches {
    /// Every switch off.
    pub const OFF: Self = Self {
        emissive_multiple: false,
        emissive_colour: false,
        falloff: false,
        translation: false,
        rotation: false,
        rotation_xyz: false,
        scale: false,
    };
    /// Whether the switch of an effect-shader field (colour controller,
    /// controlled variable) is on.
    pub fn effect_field(&self, color: bool, variable: u32) -> bool {
        match (color, variable) {
            (true, 0) => self.emissive_colour,
            (false, 0) => self.emissive_multiple,
            (false, 1..=4) => self.falloff,
            _ => false,
        }
    }
    /// Whether any switch of a transform channel is on.
    pub fn any_transform(&self) -> bool {
        self.translation || self.rotation || self.rotation_xyz || self.scale
    }
    pub fn any(&self) -> bool {
        self.emissive_multiple || self.emissive_colour || self.falloff || self.any_transform()
    }
    /// The command-line flags of the switches (`render`, `fo4-view`,
    /// `coverage_census`).
    pub const FLAGS: [&'static str; 7] = [
        "--clock-emissive-multiple",
        "--clock-emissive-colour",
        "--clock-falloff",
        "--clock-translation",
        "--clock-rotation",
        "--clock-rotation-xyz",
        "--clock-scale",
    ];
    fn slots(&mut self) -> [&mut bool; 7] {
        [
            &mut self.emissive_multiple,
            &mut self.emissive_colour,
            &mut self.falloff,
            &mut self.translation,
            &mut self.rotation,
            &mut self.rotation_xyz,
            &mut self.scale,
        ]
    }
    /// Turns on the switch `flag` names; `false` for another argument.
    pub fn set_flag(&mut self, flag: &str) -> bool {
        match Self::FLAGS.iter().position(|f| *f == flag) {
            Some(k) => {
                *self.slots()[k] = true;
                true
            }
            None => false,
        }
    }
    /// The flags of the switches that are on.
    pub fn flags(&self) -> Vec<&'static str> {
        let mut s = *self;
        s.slots()
            .into_iter()
            .zip(Self::FLAGS)
            .filter_map(|(on, f)| (*on).then_some(f))
            .collect()
    }
}

/// One key of a float or point key group, in the reader's field order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub time: f32,
    /// The first `components` entries are read.
    pub value: [f32; 3],
    /// Quadratic keys only.
    pub forward: [f32; 3],
    pub backward: [f32; 3],
    /// Tension, bias, continuity (TBC keys only).
    pub tbc: [f32; 3],
}
/// A key group of an `NiFloatData` (one component) or `NiPosData` (three).
#[derive(Debug, Clone, PartialEq)]
pub struct Keys {
    pub components: usize,
    /// nifxml `KeyType`; 0 for an empty group.
    pub kind: u32,
    pub keys: Vec<Key>,
}
fn float(x: &Field) -> Option<f32> {
    match x.value {
        Value::FloatBits(b) => Some(f32::from_bits(b)),
        _ => None,
    }
}
fn number(x: &Field) -> Option<u32> {
    match x.value {
        Value::U32(v) => Some(v),
        Value::U16(v) => Some(u32::from(v)),
        Value::U8(v) => Some(u32::from(v)),
        _ => None,
    }
}
impl Keys {
    /// Reads the key group of an `NiFloatData` or `NiPosData`'s fields;
    /// `Ok(None)` when the field sequence is not the reader's or a key time
    /// decreases.
    pub fn read(f: &[Field], components: usize, b: &Budget) -> Result<Option<Self>> {
        let mut it = f.iter();
        let out = Self::read_group(&mut it, components, b)?;
        Ok(out.filter(|_| it.next().is_none()))
    }
    /// Reads one key group (Num Keys, Interpolation, the keys) from `it`,
    /// leaving the fields after it; `Ok(None)` when the fields are not the
    /// reader's or a key time decreases.
    pub fn read_group(
        it: &mut std::slice::Iter<'_, Field>,
        components: usize,
        b: &Budget,
    ) -> Result<Option<Self>> {
        let mut next = |name: &str| it.next().filter(|x| x.name == name);
        let Some(count) = next("Num Keys").and_then(number) else {
            return Ok(None);
        };
        let mut out = Keys {
            components,
            kind: 0,
            keys: Vec::new(),
        };
        if count == 0 || !(1..=3).contains(&components) {
            return Ok((count == 0).then_some(out));
        }
        let Some(kind) = next("Interpolation").and_then(number) else {
            return Ok(None);
        };
        out.kind = kind;
        b.reserve(&mut out.keys, count as usize, "clock keys", 0)?;
        for _ in 0..count {
            let mut k = Key {
                time: 0.,
                value: [0.; 3],
                forward: [0.; 3],
                backward: [0.; 3],
                tbc: [0.; 3],
            };
            let Some(t) = next("Key Time").and_then(float) else {
                return Ok(None);
            };
            k.time = t;
            let mut group = |name: &str, into: &mut [f32; 3], n: usize| -> Option<()> {
                for c in into.iter_mut().take(n) {
                    *c = next(name).and_then(float)?;
                }
                Some(())
            };
            if group("Key Value", &mut k.value, components).is_none() {
                return Ok(None);
            }
            if kind == QUADRATIC
                && (group("Forward Tangent", &mut k.forward, components).is_none()
                    || group("Backward Tangent", &mut k.backward, components).is_none())
            {
                return Ok(None);
            }
            if kind == TBC && group("Tension Bias Continuity", &mut k.tbc, 3).is_none() {
                return Ok(None);
            }
            out.keys.push(k);
        }
        let ordered = out.keys.windows(2).all(|w| w[0].time <= w[1].time);
        Ok(ordered.then_some(out))
    }
    /// Whether every key holds the same value (and, for quadratic keys,
    /// zero tangents): the group gives that value at every time.
    pub fn constant(&self) -> bool {
        let n = self.components;
        self.keys
            .windows(2)
            .all(|w| w[0].value[..n] == w[1].value[..n])
            && (self.kind != QUADRATIC
                || self.keys.iter().all(|k| {
                    k.forward[..n]
                        .iter()
                        .chain(&k.backward[..n])
                        .all(|v| *v == 0.)
                }))
    }
    /// The value at key time `time`: the first key's value before it, the
    /// last key's after it, and between two keys by the key type
    /// (`nif_key_linear_v1`, `nif_key_quadratic_hermite_v1`,
    /// `nif_key_constant_step_v1`); `None` for an empty group, a key type
    /// without a rule (TBC, XYZ) or a value that is not finite.
    pub fn sample(&self, time: f64) -> Option<[f32; 3]> {
        let first = self.keys.first()?;
        let last = self.keys.last()?;
        if !matches!(self.kind, LINEAR | QUADRATIC | CONSTANT) || !time.is_finite() {
            return None;
        }
        let n = self.components;
        let mut out = [0f32; 3];
        let finite = |v: [f32; 3]| v[..n].iter().all(|x| x.is_finite()).then_some(v);
        if time <= f64::from(first.time) {
            return finite(first.value);
        }
        if time >= f64::from(last.time) {
            return finite(last.value);
        }
        // The last key at or before `time`, and the one after it.
        let i = self.keys.iter().rposition(|k| f64::from(k.time) <= time)?;
        let (a, z) = (self.keys.get(i)?, self.keys.get(i + 1)?);
        let span = f64::from(z.time) - f64::from(a.time);
        if span <= 0. {
            return finite(z.value);
        }
        let s = (time - f64::from(a.time)) / span;
        for (c, o) in out.iter_mut().enumerate().take(n) {
            let (p, q) = (f64::from(a.value[c]), f64::from(z.value[c]));
            *o = match self.kind {
                LINEAR => p + (q - p) * s,
                CONSTANT => p,
                // Cubic Hermite over the interval, s in 0 to 1, with the
                // stored tangents taken per interval (not per second).
                _ => {
                    let (s2, s3) = (s * s, s * s * s);
                    let h00 = 2. * s3 - 3. * s2 + 1.;
                    let h10 = s3 - 2. * s2 + s;
                    let h01 = -2. * s3 + 3. * s2;
                    let h11 = s3 - s2;
                    h00 * p
                        + h10 * f64::from(a.forward[c])
                        + h01 * q
                        + h11 * f64::from(z.backward[c])
                }
            } as f32;
        }
        finite(out)
    }
    /// The least and greatest value component `c` takes at any time
    /// (between keys by the key type, the end keys' values outside them);
    /// `None` for an empty group or a key type without a rule.
    pub fn bounds(&self, c: usize) -> Option<(f32, f32)> {
        if c >= self.components || self.keys.is_empty() || self.rule().is_none() {
            return None;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut see = |v: f64| {
            lo = lo.min(v);
            hi = hi.max(v);
        };
        for k in &self.keys {
            see(f64::from(k.value[c]));
        }
        if self.kind == QUADRATIC {
            // Each interval's cubic p(s) = a s^3 + b s^2 + m0 s + p0 over
            // s in 0 to 1: its extremes are at the roots of p'(s).
            for w in self.keys.windows(2) {
                let (p0, p1) = (f64::from(w[0].value[c]), f64::from(w[1].value[c]));
                let (m0, m1) = (f64::from(w[0].forward[c]), f64::from(w[1].backward[c]));
                let a = 2. * p0 + m0 - 2. * p1 + m1;
                let b = -3. * p0 - 2. * m0 + 3. * p1 - m1;
                let p = |s: f64| ((a * s + b) * s + m0) * s + p0;
                let (qa, qb, qc) = (3. * a, 2. * b, m0);
                let mut roots = Vec::new();
                if qa.abs() < 1e-12 {
                    if qb.abs() > 1e-12 {
                        roots.push(-qc / qb);
                    }
                } else {
                    let d = qb * qb - 4. * qa * qc;
                    if d >= 0. {
                        roots.push((-qb + d.sqrt()) / (2. * qa));
                        roots.push((-qb - d.sqrt()) / (2. * qa));
                    }
                }
                for r in roots.into_iter().filter(|r| (0. ..=1.).contains(r)) {
                    see(p(r));
                }
            }
        }
        (lo.is_finite() && hi.is_finite()).then_some((lo as f32, hi as f32))
    }
    /// The rule a sample between two keys of this group relies on.
    pub fn rule(&self) -> Option<&'static str> {
        match self.kind {
            LINEAR => Some(h::CLOCK_KEY_LINEAR),
            QUADRATIC => Some(h::CLOCK_KEY_QUADRATIC),
            CONSTANT => Some(h::CLOCK_KEY_STEP),
            _ => None,
        }
    }
}

/// The timing fields of an `NiTimeController` (nifxml order and names).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
}
impl Timing {
    pub fn read(f: &[Field]) -> Option<Self> {
        let one = |name: &str| f.iter().find(|x| x.name == name).and_then(float);
        let flags = f
            .iter()
            .find(|x| x.name == "Controller Flags")
            .and_then(number)?;
        Some(Self {
            flags: u16::try_from(flags).ok()?,
            frequency: one("Frequency")?,
            phase: one("Phase")?,
            start: one("Start Time")?,
            stop: one("Stop Time")?,
        })
    }
    /// `TimeControllerFlags` Cycle Type (bits 1 and 2).
    pub fn cycle(&self) -> u32 {
        u32::from((self.flags >> 1) & 3)
    }
    /// Active (bit 3).
    pub fn active(&self) -> bool {
        self.flags & 8 != 0
    }
    /// Play Backwards (bit 4).
    pub fn backwards(&self) -> bool {
        self.flags & 0x10 != 0
    }
    /// Manager Controlled (bit 5).
    pub fn manager(&self) -> bool {
        self.flags & 0x20 != 0
    }
    /// Anim Type APP_INIT (bit 0).
    pub fn app_init(&self) -> bool {
        self.flags & 1 != 0
    }
    /// The controller's time at `seconds` of the game clock after its model
    /// loaded: Start Time + Phase + Frequency x seconds
    /// (`nif_controller_clock_free_running_v1`), folded into Start Time to
    /// Stop Time by the cycle type: wrapped (`nif_controller_cycle_loop_v1`),
    /// back and forth (`nif_controller_cycle_reverse_v1`) or held at the
    /// ends (`nif_controller_cycle_clamp_v1`). `None` for a span that is not
    /// positive, a field or time that is not finite, or cycle type 3.
    pub fn time(&self, seconds: f64) -> Option<f64> {
        let (lo, hi) = (f64::from(self.start), f64::from(self.stop));
        let span = hi - lo;
        let raw = lo + f64::from(self.phase) + f64::from(self.frequency) * seconds;
        if !(span > 0. && span.is_finite() && raw.is_finite()) {
            return None;
        }
        Some(match self.cycle() {
            CYCLE_LOOP => lo + (raw - lo).rem_euclid(span),
            CYCLE_REVERSE => {
                let u = (raw - lo).rem_euclid(2. * span);
                lo + if u <= span { u } else { 2. * span - u }
            }
            CYCLE_CLAMP => raw.clamp(lo, hi),
            _ => return None,
        })
    }
    /// The rule of this timing's cycle type.
    pub fn rule(&self) -> Option<&'static str> {
        match self.cycle() {
            CYCLE_LOOP => Some(h::CLOCK_CYCLE_LOOP),
            CYCLE_REVERSE => Some(h::CLOCK_CYCLE_REVERSE),
            CYCLE_CLAMP => Some(h::CLOCK_CYCLE_CLAMP),
            _ => None,
        }
    }
}

/// A controller the clock runs: its timing and the keys of its one keyframe
/// interpolator.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub timing: Timing,
    pub keys: Keys,
}
impl Track {
    /// The value at `seconds` of the game clock after the model loaded.
    pub fn at(&self, seconds: f64) -> Option<[f32; 3]> {
        self.keys.sample(self.timing.time(seconds)?)
    }
    /// The rules a value of this track relies on: the free-running clock,
    /// the cycle type and the key type.
    pub fn rules(&self) -> [&'static str; 3] {
        [
            h::CLOCK_FREE_RUNNING,
            self.timing.rule().unwrap_or(h::CLOCK_FREE_RUNNING),
            self.keys.rule().unwrap_or(h::CLOCK_FREE_RUNNING),
        ]
    }
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
fn links(f: &[Field], name: &str) -> Vec<usize> {
    f.iter()
        .filter(|x| x.name == name)
        .filter_map(|x| match x.value {
            Value::Ref(r) if r >= 0 => Some(r as usize),
            _ => None,
        })
        .collect()
}
/// Whether a `NiControllerSequence` of the model names controller `ctl`
/// among its controlled blocks.
pub fn in_sequence(n: &Nif<'_>, ctl: usize) -> bool {
    (0..n.blocks.len()).any(|i| {
        type_of(n, i) == "NiControllerSequence"
            && block_fields(n, i).is_some_and(|f| links(f, "Controller").contains(&ctl))
    })
}

/// Why a controller is not a track of the clock (census labels).
pub type Refusal = &'static str;

/// The track of controller block `ctl` when the clock runs it: a controller
/// with the Active flag, without Manager Controlled (where the model has a
/// `NiControllerManager`) and named by no sequence, played forwards, with
/// one `NiFloatInterpolator` (or `NiPoint3Interpolator`) whose data holds a
/// key group of a key type with a rule, a cycle type with a rule and a
/// positive span. Otherwise the reason, by name.
pub fn track(n: &Nif<'_>, ctl: usize, b: &Budget) -> Result<std::result::Result<Track, Refusal>> {
    let Some(f) = block_fields(n, ctl) else {
        return Ok(Err("controller not decoded"));
    };
    let Some(timing) = Timing::read(f) else {
        return Ok(Err("no timing fields"));
    };
    let has_manager = (0..n.blocks.len()).any(|i| type_of(n, i) == "NiControllerManager");
    if timing.manager() && has_manager {
        return Ok(Err("manager controlled"));
    }
    if in_sequence(n, ctl) {
        return Ok(Err("named by a sequence"));
    }
    if !timing.active() {
        return Ok(Err("not active"));
    }
    if timing.backwards() {
        return Ok(Err("play backwards"));
    }
    if timing.app_init() {
        return Ok(Err("anim type APP_INIT"));
    }
    if timing.rule().is_none() {
        return Ok(Err("cycle type 3"));
    }
    let span = timing.stop - timing.start;
    if !(span > 0. && span.is_finite()) {
        return Ok(Err("span not positive"));
    }
    if !(timing.frequency.is_finite() && timing.phase.is_finite()) {
        return Ok(Err("frequency or phase not finite"));
    }
    let ips = links(f, "Interpolator");
    let [ip] = ips.as_slice() else {
        return Ok(Err("not one interpolator"));
    };
    let components = match type_of(n, *ip) {
        "NiFloatInterpolator" => 1,
        "NiPoint3Interpolator" => 3,
        _ => return Ok(Err("interpolator type")),
    };
    let Some(ipf) = block_fields(n, *ip) else {
        return Ok(Err("interpolator not decoded"));
    };
    let data = links(ipf, "Data");
    let Some(d) = data.first().copied() else {
        return Ok(Err("interpolator without data"));
    };
    let want = if components == 1 {
        "NiFloatData"
    } else {
        "NiPosData"
    };
    if type_of(n, d) != want {
        return Ok(Err("data type"));
    }
    let Some(keys) = block_fields(n, d) else {
        return Ok(Err("data not decoded"));
    };
    let Some(keys) = Keys::read(keys, components, b)? else {
        return Ok(Err("keys out of order"));
    };
    if keys.keys.is_empty() {
        return Ok(Err("no keys"));
    }
    if keys.rule().is_none() {
        return Ok(Err(if keys.kind == TBC {
            "key type TBC"
        } else {
            "key type without a rule"
        }));
    }
    Ok(Ok(Track { timing, keys }))
}

/// The rotation keys of an `NiTransformData` (nifxml `NiKeyframeData`),
/// in the reader's field order.
#[derive(Debug, Clone, PartialEq)]
pub enum Rotation {
    /// Quaternion keys: nifxml `KeyType` of the group and (time, w x y z).
    Quaternion {
        kind: u32,
        keys: Vec<(f32, [f32; 4])>,
    },
    /// Rotation Type XYZ: three float key groups, X, Y and Z.
    Xyz(Box<[Keys; 3]>),
}
/// The keys of an `NiTransformData`: rotation (`None` without rotation
/// keys), translation (three components) and scale (one).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeKeys {
    pub rotation: Option<Rotation>,
    pub translation: Keys,
    pub scale: Keys,
}
impl NodeKeys {
    /// Reads an `NiTransformData`'s fields; `Ok(None)` when the field
    /// sequence is not the reader's or a key time decreases.
    pub fn read(f: &[Field], b: &Budget) -> Result<Option<Self>> {
        let mut it = f.iter();
        fn take<'a>(it: &mut std::slice::Iter<'a, Field>, name: &str) -> Option<&'a Field> {
            it.next().filter(|x| x.name == name)
        }
        let Some(nrot) = take(&mut it, "Num Rotation Keys").and_then(number) else {
            return Ok(None);
        };
        let mut rotation = None;
        if nrot > 0 {
            let Some(kind) = take(&mut it, "Rotation Type").and_then(number) else {
                return Ok(None);
            };
            if kind == XYZ {
                let mut axes = Vec::new();
                for _ in 0..3 {
                    let Some(k) = Keys::read_group(&mut it, 1, b)? else {
                        return Ok(None);
                    };
                    axes.push(k);
                }
                let Ok(axes) = <[Keys; 3]>::try_from(axes) else {
                    return Ok(None);
                };
                rotation = Some(Rotation::Xyz(Box::new(axes)));
            } else {
                let mut keys = Vec::new();
                b.reserve(&mut keys, nrot as usize, "clock rotation keys", 0)?;
                for _ in 0..nrot {
                    let Some(t) = take(&mut it, "Key Time").and_then(float) else {
                        return Ok(None);
                    };
                    let mut q = [0f32; 4];
                    for c in &mut q {
                        let Some(v) = take(&mut it, "Quaternion Value").and_then(float) else {
                            return Ok(None);
                        };
                        *c = v;
                    }
                    if kind == TBC {
                        for _ in 0..3 {
                            if take(&mut it, "Tension Bias Continuity").is_none() {
                                return Ok(None);
                            }
                        }
                    }
                    keys.push((t, q));
                }
                if !keys.windows(2).all(|w| w[0].0 <= w[1].0) {
                    return Ok(None);
                }
                rotation = Some(Rotation::Quaternion { kind, keys });
            }
        }
        let Some(translation) = Keys::read_group(&mut it, 3, b)? else {
            return Ok(None);
        };
        let Some(scale) = Keys::read_group(&mut it, 1, b)? else {
            return Ok(None);
        };
        Ok(it.next().is_none().then_some(Self {
            rotation,
            translation,
            scale,
        }))
    }
}

/// The order in which the three angles of XYZ rotation keys compose
/// (`nif_clock_transform_rotation_xyz_v1`): the rotation about axis
/// `XYZ_ORDER[0]` applies first to a child point, then `XYZ_ORDER[1]`, then
/// `XYZ_ORDER[2]` (0: X, 1: Y, 2: Z), each by its angle in radians,
/// counter-clockwise looking down the axis. Chosen from the data
/// (`transform_clock_census`, XYZ readings against the stored rotation).
pub const XYZ_ORDER: [usize; 3] = [0, 1, 2];

/// Rows (row-major, acting on column vectors, as a node's rotation words)
/// of the rotation by `angle` radians about axis `k`.
fn axis_rows(k: usize, angle: f64) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    match k {
        0 => [[1., 0., 0.], [0., c, -s], [0., s, c]],
        1 => [[c, 0., s], [0., 1., 0.], [-s, 0., c]],
        _ => [[c, -s, 0.], [s, c, 0.], [0., 0., 1.]],
    }
}
fn mat_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
/// Rotation words of three angles (X, Y, Z, radians) composed in `order`
/// (the first entry applied first to a child point).
pub fn xyz_rows(angles: [f64; 3], order: [usize; 3]) -> [f64; 9] {
    let m = order
        .iter()
        .fold([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], |acc, &k| {
            mat_mul(axis_rows(k, angles[k]), acc)
        });
    std::array::from_fn(|i| m[i / 3][i % 3])
}
/// Rotation words of a quaternion (w, x y z), as `crate::rest::quaternion_rows`.
pub fn quaternion_rows(q: [f64; 4]) -> [f64; 9] {
    let [w, x, y, z] = q;
    [
        1. - 2. * (y * y + z * z),
        2. * (x * y - w * z),
        2. * (x * z + w * y),
        2. * (x * y + w * z),
        1. - 2. * (x * x + z * z),
        2. * (y * z - w * x),
        2. * (x * z - w * y),
        2. * (y * z + w * x),
        1. - 2. * (x * x + y * y),
    ]
}
/// Spherical linear interpolation from `a` to `b` at `s` in 0 to 1, along
/// the shorter arc (`b` negated when the two lie in opposite hemispheres),
/// normalised; `None` for a zero quaternion.
pub fn slerp(a: [f64; 4], b: [f64; 4], s: f64) -> Option<[f64; 4]> {
    let norm = |q: [f64; 4]| q.iter().map(|v| v * v).sum::<f64>().sqrt();
    let (na, nb) = (norm(a), norm(b));
    if !(na > 0. && nb > 0.) {
        return None;
    }
    let a = a.map(|v| v / na);
    let mut b = b.map(|v| v / nb);
    let mut d: f64 = (0..4).map(|i| a[i] * b[i]).sum();
    if d < 0. {
        b = b.map(|v| -v);
        d = -d;
    }
    let (p, q) = if d > 1. - 1e-9 {
        (1. - s, s)
    } else {
        let theta = d.min(1.).acos();
        let sin = theta.sin();
        (((1. - s) * theta).sin() / sin, (s * theta).sin() / sin)
    };
    let r: [f64; 4] = std::array::from_fn(|i| p * a[i] + q * b[i]);
    let n = norm(r);
    (n > 0. && n.is_finite()).then(|| r.map(|v| v / n))
}

/// A transform controller's channels (`TransformTrack::varies`).
pub const TRANSLATION: usize = 0;
pub const ROTATION: usize = 1;
pub const SCALE: usize = 2;

/// A free-running transform controller the clock runs: its timing, its
/// target node's stored local transform words, its interpolator's pose
/// values (`None` where nifxml's `#INV_FLT#` marks them unset) and its
/// keys (`None`: an interpolator without data).
#[derive(Debug, Clone, PartialEq)]
pub struct TransformTrack {
    pub timing: Timing,
    /// The controller's target node.
    pub node: usize,
    pub stored_translation: [f32; 3],
    /// Rotation words, rows acting on column vectors.
    pub stored_rotation: [f32; 9],
    pub stored_scale: f32,
    pub pose_translation: Option<[f32; 3]>,
    pub pose_rotation: Option<[f32; 4]>,
    pub pose_scale: Option<f32>,
    pub keys: Option<NodeKeys>,
}
/// The local transform words a [`TransformTrack`] gives: translation,
/// rotation words (rows acting on column vectors) and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalWords {
    pub translation: [f64; 3],
    pub rotation: [f64; 9],
    pub scale: f64,
}
impl LocalWords {
    /// The node transform of these words (`crate::transform::Transform`
    /// holds the rotation transposed).
    pub fn transform(&self) -> crate::transform::Transform {
        crate::transform::Transform {
            rotation: std::array::from_fn(|i| std::array::from_fn(|j| self.rotation[j * 3 + i])),
            translation: self.translation,
            scale: self.scale,
        }
    }
}
fn close_words(a: &[f64], b: &[f32], tolerance: f32) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - f64::from(*y)).abs() <= f64::from(tolerance))
}
impl TransformTrack {
    fn translation_keys(&self) -> Option<&Keys> {
        self.keys
            .as_ref()
            .map(|k| &k.translation)
            .filter(|k| !k.keys.is_empty())
    }
    fn scale_keys(&self) -> Option<&Keys> {
        self.keys
            .as_ref()
            .map(|k| &k.scale)
            .filter(|k| !k.keys.is_empty())
    }
    fn rotation_keys(&self) -> Option<&Rotation> {
        self.keys.as_ref().and_then(|k| k.rotation.as_ref())
    }
    /// The words of each channel at key time `time` (the controller's
    /// time, `Timing::time`): a channel with keys takes their value
    /// (translation and scale by key type, quaternion keys by
    /// [`slerp`] between linear keys or the earlier key of step keys, XYZ
    /// keys each angle by its key type composed in [`XYZ_ORDER`]); a
    /// channel without keys takes the interpolator's pose value where set
    /// and the node's stored value otherwise. `None` for a key type
    /// without a reading or a value that is not finite.
    pub fn words_at(&self, time: f64) -> Option<LocalWords> {
        let stored_t = self.stored_translation.map(f64::from);
        let translation = match self.translation_keys() {
            Some(k) => k.sample(time)?.map(f64::from),
            None => self.pose_translation.map_or(stored_t, |p| p.map(f64::from)),
        };
        let scale = match self.scale_keys() {
            Some(k) => f64::from(k.sample(time)?[0]),
            None => f64::from(self.pose_scale.unwrap_or(self.stored_scale)),
        };
        let rotation = match self.rotation_keys() {
            Some(Rotation::Xyz(axes)) => {
                let mut angles = [0f64; 3];
                for (a, k) in angles.iter_mut().zip(axes.iter()) {
                    if k.keys.is_empty() {
                        return None;
                    }
                    *a = f64::from(k.sample(time)?[0]);
                }
                xyz_rows(angles, XYZ_ORDER)
            }
            Some(Rotation::Quaternion { kind, keys }) => {
                quaternion_rows(quaternion_at(*kind, keys, time)?)
            }
            None => match self.pose_rotation {
                Some(q) => quaternion_rows(q.map(f64::from)),
                None => self.stored_rotation.map(f64::from),
            },
        };
        let all = translation
            .iter()
            .chain(&rotation)
            .chain(std::iter::once(&scale));
        let mut all = all;
        all.all(|v| v.is_finite()).then_some(LocalWords {
            translation,
            rotation,
            scale,
        })
    }
    /// The node's local transform `seconds` after its model loaded: a
    /// channel that never varies ([`Self::varies`]) keeps the stored words
    /// exactly.
    pub fn local(&self, seconds: f64) -> Option<crate::transform::Transform> {
        let w = self.words_at(self.timing.time(seconds)?)?;
        let v = self.varies();
        Some(
            LocalWords {
                translation: if v[TRANSLATION] {
                    w.translation
                } else {
                    self.stored_translation.map(f64::from)
                },
                rotation: if v[ROTATION] {
                    w.rotation
                } else {
                    self.stored_rotation.map(f64::from)
                },
                scale: if v[SCALE] {
                    w.scale
                } else {
                    f64::from(self.stored_scale)
                },
            }
            .transform(),
        )
    }
    /// Whether each channel (translation, rotation, scale) can take words
    /// other than the node's stored ones, within `crate::rest::TOLERANCE`:
    /// a key or a set pose value away from them, or quadratic keys with a
    /// tangent other than 0.
    pub fn varies(&self) -> [bool; 3] {
        let [tt, tr, ts] = crate::rest::TOLERANCE;
        let st = self.stored_translation;
        let ss = [self.stored_scale];
        let group = |k: &Keys, stored: &[f32], tol: f32| {
            let n = k.components;
            k.keys.iter().any(|x| {
                !close_words(
                    &x.value[..n]
                        .iter()
                        .map(|v| f64::from(*v))
                        .collect::<Vec<_>>(),
                    stored,
                    tol,
                )
            }) || (k.kind == QUADRATIC
                && k.keys.iter().any(|x| {
                    x.forward[..n]
                        .iter()
                        .chain(&x.backward[..n])
                        .any(|v| *v != 0.)
                }))
        };
        let translation = match self.translation_keys() {
            Some(k) => group(k, &st, tt),
            None => self
                .pose_translation
                .is_some_and(|p| !close_words(&p.map(f64::from), &st, tt)),
        };
        let scale = match self.scale_keys() {
            Some(k) => group(k, &ss, ts),
            None => self
                .pose_scale
                .is_some_and(|p| !close_words(&[f64::from(p)], &ss, ts)),
        };
        let sr = &self.stored_rotation;
        let rotation = match self.rotation_keys() {
            Some(Rotation::Quaternion { keys, .. }) => keys
                .iter()
                .any(|(_, q)| !close_words(&quaternion_rows(q.map(f64::from)), sr, tr)),
            Some(Rotation::Xyz(axes)) => {
                axes.iter().any(|k| !k.constant())
                    || axes.iter().any(|k| k.keys.is_empty())
                    || !close_words(
                        &xyz_rows(
                            std::array::from_fn(|i| f64::from(axes[i].keys[0].value[0])),
                            XYZ_ORDER,
                        ),
                        sr,
                        tr,
                    )
            }
            None => self
                .pose_rotation
                .is_some_and(|q| !close_words(&quaternion_rows(q.map(f64::from)), sr, tr)),
        };
        [translation, rotation, scale]
    }
    /// The form of the rotation keys: `Some(true)` for XYZ angle keys,
    /// `Some(false)` for quaternion keys or a pose quaternion, `None`
    /// without either.
    pub fn rotation_xyz(&self) -> Option<bool> {
        match self.rotation_keys() {
            Some(Rotation::Xyz(_)) => Some(true),
            Some(Rotation::Quaternion { .. }) => Some(false),
            None => self.pose_rotation.map(|_| false),
        }
    }
    /// The key types (nifxml `KeyType`) whose key rules the channels that
    /// vary rely on: translation and scale groups, each XYZ angle group, and
    /// step quaternion keys (linear quaternion keys are read by [`slerp`]
    /// under the rotation rule alone).
    pub fn varying_key_kinds(&self) -> Vec<u32> {
        let v = self.varies();
        let mut out = Vec::new();
        if v[TRANSLATION] {
            out.extend(self.translation_keys().map(|k| k.kind));
        }
        if v[SCALE] {
            out.extend(self.scale_keys().map(|k| k.kind));
        }
        if v[ROTATION] {
            match self.rotation_keys() {
                Some(Rotation::Quaternion { kind: CONSTANT, .. }) => out.push(CONSTANT),
                Some(Rotation::Xyz(axes)) => out.extend(axes.iter().map(|k| k.kind)),
                _ => {}
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
    /// Whether the switches cover every channel that varies, and the clock
    /// has a reading for each of their key types; `false` when nothing
    /// varies.
    pub fn runs_with(&self, s: Switches) -> bool {
        let v = self.varies();
        if !v.iter().any(|x| *x) {
            return false;
        }
        let rotation_ok = !v[ROTATION]
            || match self.rotation_xyz() {
                Some(true) => s.rotation_xyz,
                Some(false) => s.rotation,
                None => false,
            };
        (!v[TRANSLATION] || s.translation)
            && (!v[SCALE] || s.scale)
            && rotation_ok
            && self.unread().is_none()
    }
    /// Why the clock cannot sample a channel that varies: a key type
    /// without a reading (TBC, quadratic quaternion keys), an XYZ angle
    /// group without keys.
    pub fn unread(&self) -> Option<Refusal> {
        let v = self.varies();
        let float_ok = |k: u32| matches!(k, LINEAR | QUADRATIC | CONSTANT);
        if v[TRANSLATION] && self.translation_keys().is_some_and(|k| !float_ok(k.kind)) {
            return Some("translation key type without a reading");
        }
        if v[SCALE] && self.scale_keys().is_some_and(|k| !float_ok(k.kind)) {
            return Some("scale key type without a reading");
        }
        if v[ROTATION] {
            match self.rotation_keys() {
                Some(Rotation::Quaternion { kind, .. }) if !matches!(*kind, LINEAR | CONSTANT) => {
                    return Some("quaternion key type without a reading")
                }
                Some(Rotation::Xyz(axes)) => {
                    if axes.iter().any(|k| k.keys.is_empty()) {
                        return Some("XYZ angle group without keys");
                    }
                    if axes.iter().any(|k| !float_ok(k.kind)) {
                        return Some("XYZ angle key type without a reading");
                    }
                }
                _ => {}
            }
        }
        None
    }
    /// The rules a local transform of this track relies on: the
    /// free-running clock and its cycle type, the key types of the
    /// channels that vary, and each varying channel's own rule.
    pub fn rules(&self) -> Vec<&'static str> {
        let v = self.varies();
        let mut out = vec![
            h::CLOCK_FREE_RUNNING,
            self.timing.rule().unwrap_or(h::CLOCK_FREE_RUNNING),
        ];
        for k in self.varying_key_kinds() {
            match k {
                LINEAR => out.push(h::CLOCK_KEY_LINEAR),
                QUADRATIC => out.push(h::CLOCK_KEY_QUADRATIC),
                CONSTANT => out.push(h::CLOCK_KEY_STEP),
                _ => {}
            }
        }
        if v[TRANSLATION] {
            out.push(h::CLOCK_TRANSLATION);
        }
        if v[ROTATION] {
            out.push(if self.rotation_xyz() == Some(true) {
                h::CLOCK_ROTATION_XYZ
            } else {
                h::CLOCK_ROTATION_QUATERNION
            });
        }
        if v[SCALE] {
            out.push(h::CLOCK_SCALE);
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
/// The quaternion of keys of type `kind` at key time `time`: the first
/// key's before it, the last key's after it, between two linear keys their
/// [`slerp`], between two step keys the earlier one; `None` for another
/// key type.
pub fn quaternion_at(kind: u32, keys: &[(f32, [f32; 4])], time: f64) -> Option<[f64; 4]> {
    let first = keys.first()?;
    let last = keys.last()?;
    if !matches!(kind, LINEAR | CONSTANT) || !time.is_finite() {
        return None;
    }
    let q = |k: &(f32, [f32; 4])| k.1.map(f64::from);
    if time <= f64::from(first.0) {
        return slerp(q(first), q(first), 0.);
    }
    if time >= f64::from(last.0) {
        return slerp(q(last), q(last), 0.);
    }
    let i = keys.iter().rposition(|k| f64::from(k.0) <= time)?;
    let (a, z) = (keys.get(i)?, keys.get(i + 1)?);
    let span = f64::from(z.0) - f64::from(a.0);
    if span <= 0. {
        return slerp(q(z), q(z), 0.);
    }
    if kind == CONSTANT {
        return slerp(q(a), q(a), 0.);
    }
    slerp(q(a), q(z), (time - f64::from(a.0)) / span)
}

fn floats_of(f: &[Field], name: &str) -> Vec<f32> {
    f.iter()
        .filter(|x| x.name == name)
        .filter_map(float)
        .collect()
}
/// A transform controller (`NiTransformController`, `NiKeyframeController`)
/// that runs by itself: Manager Controlled clear (or no manager in the
/// model) and named by no sequence.
pub fn free_transform_controller(n: &Nif<'_>, ctl: usize) -> bool {
    if !matches!(
        type_of(n, ctl),
        "NiTransformController" | "NiKeyframeController"
    ) {
        return false;
    }
    let Some(timing) = block_fields(n, ctl).and_then(Timing::read) else {
        return false;
    };
    let has_manager = (0..n.blocks.len()).any(|i| type_of(n, i) == "NiControllerManager");
    !(timing.manager() && has_manager) && !in_sequence(n, ctl)
}

/// The transform track of controller block `ctl` when the clock can run
/// it: a free-running [`free_transform_controller`] with the timing
/// [`track`] requires, one `NiTransformInterpolator` (with an
/// `NiTransformData` or none) and a target node with stored transform
/// words. Otherwise the reason, by name. Whether its channels vary and
/// whether the clock reads their keys is the track's
/// ([`TransformTrack::varies`], [`TransformTrack::unread`]).
pub fn transform_track(
    n: &Nif<'_>,
    ctl: usize,
    b: &Budget,
) -> Result<std::result::Result<TransformTrack, Refusal>> {
    if !matches!(
        type_of(n, ctl),
        "NiTransformController" | "NiKeyframeController"
    ) {
        return Ok(Err("not a transform controller"));
    }
    let Some(f) = block_fields(n, ctl) else {
        return Ok(Err("controller not decoded"));
    };
    let Some(timing) = Timing::read(f) else {
        return Ok(Err("no timing fields"));
    };
    let has_manager = (0..n.blocks.len()).any(|i| type_of(n, i) == "NiControllerManager");
    if timing.manager() && has_manager {
        return Ok(Err("manager controlled"));
    }
    if in_sequence(n, ctl) {
        return Ok(Err("named by a sequence"));
    }
    if !timing.active() {
        return Ok(Err("not active"));
    }
    if timing.backwards() {
        return Ok(Err("play backwards"));
    }
    if timing.app_init() {
        return Ok(Err("anim type APP_INIT"));
    }
    if timing.rule().is_none() {
        return Ok(Err("cycle type 3"));
    }
    let span = timing.stop - timing.start;
    if !(span > 0. && span.is_finite()) {
        return Ok(Err("span not positive"));
    }
    if !(timing.frequency.is_finite() && timing.phase.is_finite()) {
        return Ok(Err("frequency or phase not finite"));
    }
    let [node] = links(f, "Target")[..] else {
        return Ok(Err("not one target"));
    };
    let Some(nf) = block_fields(n, node) else {
        return Ok(Err("target not decoded"));
    };
    let (st, sr, ss) = (
        floats_of(nf, "Translation"),
        floats_of(nf, "Rotation"),
        floats_of(nf, "Scale"),
    );
    let (Ok(st), Ok(sr), Ok([ss])) = (
        <[f32; 3]>::try_from(st),
        <[f32; 9]>::try_from(sr),
        <[f32; 1]>::try_from(ss),
    ) else {
        return Ok(Err("target without transform words"));
    };
    let ips = links(f, "Interpolator");
    let [ip] = ips.as_slice() else {
        return Ok(Err("not one interpolator"));
    };
    if type_of(n, *ip) != "NiTransformInterpolator" {
        return Ok(Err("interpolator type"));
    }
    let Some(ipf) = block_fields(n, *ip) else {
        return Ok(Err("interpolator not decoded"));
    };
    let (pt, pq, ps) = (
        floats_of(ipf, "Translation"),
        floats_of(ipf, "Quaternion"),
        floats_of(ipf, "Scale"),
    );
    let (Ok(pt), Ok(pq), Ok([ps])) = (
        <[f32; 3]>::try_from(pt),
        <[f32; 4]>::try_from(pq),
        <[f32; 1]>::try_from(ps),
    ) else {
        return Ok(Err("interpolator without pose fields"));
    };
    let unset = crate::rest::INVALID;
    let keys = match links(ipf, "Data").first().copied() {
        None => None,
        Some(d) => {
            if type_of(n, d) != "NiTransformData" {
                return Ok(Err("data type"));
            }
            let Some(df) = block_fields(n, d) else {
                return Ok(Err("data not decoded"));
            };
            let Some(k) = NodeKeys::read(df, b)? else {
                return Ok(Err("keys not read"));
            };
            Some(k)
        }
    };
    Ok(Ok(TransformTrack {
        timing,
        node,
        stored_translation: st,
        stored_rotation: sr,
        stored_scale: ss,
        pose_translation: (pt[0] != unset).then_some(pt),
        pose_rotation: (pq[0] != unset).then_some(pq),
        pose_scale: (ps != unset).then_some(ps),
        keys,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(name: &'static str, value: Value) -> Field {
        Field {
            name,
            offset: 0,
            value,
        }
    }
    fn fl(name: &'static str, v: f32) -> Field {
        f(name, Value::FloatBits(v.to_bits()))
    }
    fn group(kind: u32, keys: &[(f32, f32, f32, f32)]) -> Vec<Field> {
        let mut v = vec![f("Num Keys", Value::U32(keys.len() as u32))];
        if !keys.is_empty() {
            v.push(f("Interpolation", Value::U32(kind)));
        }
        for (t, x, fwd, back) in keys {
            v.push(fl("Key Time", *t));
            v.push(fl("Key Value", *x));
            if kind == QUADRATIC {
                v.push(fl("Forward Tangent", *fwd));
                v.push(fl("Backward Tangent", *back));
            }
        }
        v
    }
    fn timing(cycle: u16, start: f32, stop: f32) -> Timing {
        Timing {
            flags: 8 | (cycle << 1),
            frequency: 1.,
            phase: 0.,
            start,
            stop,
        }
    }

    #[test]
    fn keys_are_read_in_field_order_and_sampled_by_key_type() {
        let b = Budget::new(1 << 20);
        let k = Keys::read(&group(LINEAR, &[(0., 0., 0., 0.), (2., 1., 0., 0.)]), 1, &b)
            .unwrap()
            .unwrap();
        assert_eq!(k.sample(-1.).unwrap()[0], 0.);
        assert_eq!(k.sample(0.5).unwrap()[0], 0.25);
        assert_eq!(k.sample(3.).unwrap()[0], 1.);
        assert!(!k.constant());
        let s = Keys::read(
            &group(CONSTANT, &[(0., 0., 0., 0.), (2., 1., 0., 0.)]),
            1,
            &b,
        )
        .unwrap()
        .unwrap();
        assert_eq!(s.sample(1.9).unwrap()[0], 0.);
        assert_eq!(s.sample(2.).unwrap()[0], 1.);
        // Hermite with tangents equal to the interval's difference is the
        // straight line; zero tangents ease in and out.
        let q = Keys::read(
            &group(QUADRATIC, &[(0., 0., 1., 1.), (2., 1., 1., 1.)]),
            1,
            &b,
        )
        .unwrap()
        .unwrap();
        assert!((q.sample(0.5).unwrap()[0] - 0.25).abs() < 1e-6);
        let e = Keys::read(
            &group(QUADRATIC, &[(0., 0., 0., 0.), (2., 1., 0., 0.)]),
            1,
            &b,
        )
        .unwrap()
        .unwrap();
        assert!((e.sample(0.5).unwrap()[0] - 0.15625).abs() < 1e-6);
        // TBC keys have no rule; times out of order are refused.
        let t = Keys {
            kind: TBC,
            ..k.clone()
        };
        assert_eq!(t.sample(0.5), None);
        assert!(
            Keys::read(&group(LINEAR, &[(1., 0., 0., 0.), (0., 1., 0., 0.)]), 1, &b)
                .unwrap()
                .is_none()
        );
        // A trailing field is not the reader's order.
        let mut g = group(LINEAR, &[(0., 0., 0., 0.)]);
        g.push(fl("Key Time", 1.));
        assert!(Keys::read(&g, 1, &b).unwrap().is_none());
        assert!(
            Keys::read(&group(LINEAR, &[(0., 3., 0., 0.), (1., 3., 0., 0.)]), 1, &b)
                .unwrap()
                .unwrap()
                .constant()
        );
    }

    #[test]
    fn bounds_take_the_extremes_between_quadratic_keys() {
        let b = Budget::new(1 << 20);
        let l = Keys::read(
            &group(LINEAR, &[(0., 0.5, 0., 0.), (1., 2., 0., 0.)]),
            1,
            &b,
        )
        .unwrap()
        .unwrap();
        assert_eq!(l.bounds(0), Some((0.5, 2.)));
        assert_eq!(l.bounds(1), None);
        // Tangents that overshoot: 0 to 1 with forward and backward -3
        // dips below 0 and rises above 1 inside the interval.
        let q = Keys::read(
            &group(QUADRATIC, &[(0., 0., -3., -3.), (1., 1., -3., -3.)]),
            1,
            &b,
        )
        .unwrap()
        .unwrap();
        let (lo, hi) = q.bounds(0).unwrap();
        let samples: Vec<f32> = (0..=1000)
            .map(|i| q.sample(f64::from(i) / 1000.).unwrap()[0])
            .collect();
        let (slo, shi) = samples
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, z), v| {
                (a.min(*v), z.max(*v))
            });
        assert!(lo < 0. && hi > 1.);
        assert!(
            (lo - slo).abs() < 1e-4 && (hi - shi).abs() < 1e-4,
            "{lo} {hi} {slo} {shi}"
        );
    }

    #[test]
    fn switches_are_off_by_default_and_each_flag_names_one() {
        let mut s = Switches::default();
        assert!(!s.any() && s.flags().is_empty());
        assert!(!s.effect_field(false, 0) && !s.effect_field(true, 0));
        assert!(!s.set_flag("--time"));
        assert!(s.set_flag("--clock-falloff"));
        assert!(s.effect_field(false, 3) && !s.effect_field(false, 0));
        assert!(!s.effect_field(false, 5));
        assert_eq!(s.flags(), ["--clock-falloff"]);
        for f in Switches::FLAGS {
            assert!(s.set_flag(f));
        }
        assert!(s.effect_field(true, 0) && s.effect_field(false, 0));
        assert_eq!(s.flags(), Switches::FLAGS);
    }

    #[test]
    fn controller_time_is_folded_by_cycle_type() {
        let l = timing(0, 1., 3.);
        assert_eq!(l.time(0.), Some(1.));
        assert_eq!(l.time(1.5), Some(2.5));
        assert_eq!(l.time(2.5), Some(1.5));
        let r = timing(1, 1., 3.);
        assert_eq!(r.time(1.5), Some(2.5));
        assert_eq!(r.time(2.5), Some(2.5));
        assert_eq!(r.time(4.), Some(1.));
        let c = timing(2, 1., 3.);
        assert_eq!(c.time(5.), Some(3.));
        let fast = Timing {
            frequency: 2.,
            phase: 0.5,
            ..l
        };
        assert_eq!(fast.time(0.5), Some(2.5));
        assert_eq!(timing(0, 1., 1.).time(0.), None);
        assert_eq!(timing(3, 0., 1.).time(0.), None);
        assert_eq!(
            [l.rule(), r.rule(), c.rule()],
            [
                Some(h::CLOCK_CYCLE_LOOP),
                Some(h::CLOCK_CYCLE_REVERSE),
                Some(h::CLOCK_CYCLE_CLAMP)
            ]
        );
    }

    /// An NiTransformData's fields: quaternion or XYZ rotation keys,
    /// linear translation keys, linear scale keys.
    fn transform_data(
        quaternions: &[(f32, [f32; 4])],
        xyz: Option<[Vec<(f32, f32)>; 3]>,
        translations: &[(f32, [f32; 3])],
        scales: &[(f32, f32)],
    ) -> Vec<Field> {
        let mut v = Vec::new();
        match &xyz {
            Some(axes) => {
                v.push(f("Num Rotation Keys", Value::U32(1)));
                v.push(f("Rotation Type", Value::U32(XYZ)));
                for a in axes {
                    v.extend(group(
                        LINEAR,
                        &a.iter().map(|(t, x)| (*t, *x, 0., 0.)).collect::<Vec<_>>(),
                    ));
                }
            }
            None => {
                v.push(f("Num Rotation Keys", Value::U32(quaternions.len() as u32)));
                if !quaternions.is_empty() {
                    v.push(f("Rotation Type", Value::U32(LINEAR)));
                }
                for (t, q) in quaternions {
                    v.push(fl("Key Time", *t));
                    for c in q {
                        v.push(fl("Quaternion Value", *c));
                    }
                }
            }
        }
        v.push(f("Num Keys", Value::U32(translations.len() as u32)));
        if !translations.is_empty() {
            v.push(f("Interpolation", Value::U32(LINEAR)));
        }
        for (t, x) in translations {
            v.push(fl("Key Time", *t));
            for c in x {
                v.push(fl("Key Value", *c));
            }
        }
        v.extend(group(
            LINEAR,
            &scales
                .iter()
                .map(|(t, x)| (*t, *x, 0., 0.))
                .collect::<Vec<_>>(),
        ));
        v
    }
    fn moving(keys: Vec<Field>, pose_scale: Option<f32>) -> TransformTrack {
        let b = Budget::new(1 << 20);
        TransformTrack {
            timing: timing(0, 0., 2.),
            node: 1,
            stored_translation: [0.; 3],
            stored_rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            stored_scale: 1.,
            pose_translation: None,
            pose_rotation: None,
            pose_scale,
            keys: Some(NodeKeys::read(&keys, &b).unwrap().unwrap()),
        }
    }

    #[test]
    fn transform_keys_are_read_in_field_order_and_move_their_channels() {
        let b = Budget::new(1 << 20);
        // A quarter turn about Z over two seconds, a translation from the
        // stored origin to (10, 0, 0), no scale keys.
        let (s, c) = (
            std::f32::consts::FRAC_PI_4.sin(),
            std::f32::consts::FRAC_PI_4.cos(),
        );
        let data = transform_data(
            &[(0., [1., 0., 0., 0.]), (2., [c, 0., 0., s])],
            None,
            &[(0., [0., 0., 0.]), (2., [10., 0., 0.])],
            &[],
        );
        let t = moving(data, None);
        assert_eq!(t.varies(), [true, true, false]);
        assert_eq!(t.rotation_xyz(), Some(false));
        let half = t.local(1.).unwrap();
        assert!((half.translation[0] - 5.).abs() < 1e-9);
        // Slerp half-way: an eighth of a turn; a point on X goes to
        // (cos 45, sin 45). `Transform::apply` reads rotation transposed.
        let p = half.apply([1., 0., 0.]);
        let r = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (p[0] - 5. - r).abs() < 1e-6 && (p[1] - r).abs() < 1e-6,
            "{p:?}"
        );
        assert_eq!(half.scale, 1.);
        // Looping: a whole cycle later the same.
        let again = t.local(3.).unwrap();
        assert!((again.translation[0] - 5.).abs() < 1e-9);
        let mut rules = t.rules();
        rules.sort_unstable();
        assert_eq!(
            rules,
            [
                h::CLOCK_ROTATION_QUATERNION,
                h::CLOCK_TRANSLATION,
                h::CLOCK_FREE_RUNNING,
                h::CLOCK_CYCLE_LOOP,
                h::CLOCK_KEY_LINEAR,
            ]
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
        // Each varying channel needs its own switch.
        let mut sw = Switches::OFF;
        assert!(!t.runs_with(sw));
        sw.translation = true;
        assert!(!t.runs_with(sw));
        sw.rotation_xyz = true;
        assert!(!t.runs_with(sw));
        sw.rotation = true;
        assert!(t.runs_with(sw));
        // A field sequence that is not the reader's is refused.
        let mut bad = transform_data(&[], None, &[(0., [0.; 3])], &[]);
        bad.push(fl("Key Time", 1.));
        assert!(NodeKeys::read(&bad, &b).unwrap().is_none());
    }

    #[test]
    fn xyz_angles_compose_x_then_y_then_z_and_keyless_channels_hold_pose_or_stored() {
        // X a quarter turn, then Z a quarter turn: X goes to Y under Z
        // alone, and Y goes to Z under X, then stays Z under Z.
        let q = std::f32::consts::FRAC_PI_2;
        let data = transform_data(
            &[],
            Some([
                vec![(0., q), (2., q)],
                vec![(0., 0.), (2., 0.)],
                vec![(0., 0.), (2., 2. * q)],
            ]),
            &[],
            &[(0., 1.), (2., 3.)],
        );
        let t = moving(data, Some(7.));
        assert_eq!(t.varies(), [false, true, true]);
        assert_eq!(t.rotation_xyz(), Some(true));
        let w = t.local(1.).unwrap();
        // At one second: X a quarter turn, Z a quarter turn, scale 2.
        let px = w.apply([1., 0., 0.]);
        let py = w.apply([0., 1., 0.]);
        assert!((px[1] - 2.).abs() < 1e-6 && px[0].abs() < 1e-6, "{px:?}");
        assert!((py[2] - 2.).abs() < 1e-6, "{py:?}");
        let rows = xyz_rows([f64::from(q), 0., f64::from(q)], XYZ_ORDER);
        let z_then_x = xyz_rows([f64::from(q), 0., f64::from(q)], [2, 1, 0]);
        assert_ne!(rows, z_then_x);
        assert!(t.rules().contains(&h::CLOCK_ROTATION_XYZ));
        assert!(t.rules().contains(&h::CLOCK_SCALE));
        assert!(!t.rules().contains(&h::CLOCK_TRANSLATION));
        // Translation has no keys and no pose: the stored words, exactly.
        assert_eq!(w.translation, [0.; 3]);
        // Scale keys win over the pose; without them the pose holds.
        let pose_only = TransformTrack {
            keys: None,
            ..t.clone()
        };
        assert_eq!(pose_only.varies(), [false, false, true]);
        assert_eq!(pose_only.local(5.).unwrap().scale, 7.);
        // An empty XYZ angle group has no reading.
        let mut empty = t.clone();
        if let Some(NodeKeys {
            rotation: Some(Rotation::Xyz(axes)),
            ..
        }) = &mut empty.keys
        {
            axes[1].keys.clear();
        }
        assert_eq!(empty.unread(), Some("XYZ angle group without keys"));
        assert!(!empty.runs_with(Switches {
            rotation_xyz: true,
            scale: true,
            ..Switches::OFF
        }));
    }

    #[test]
    fn slerp_takes_the_shorter_arc_and_normalises() {
        let a = [1., 0., 0., 0.];
        let b = [-0.5f64.sqrt(), 0., 0., -0.5f64.sqrt()];
        // b is the same rotation as -b: the shorter arc from a.
        let m = slerp(a, b, 0.5).unwrap();
        let n: f64 = m.iter().map(|v| v * v).sum();
        assert!((n - 1.).abs() < 1e-12);
        assert!(m[0] > 0. && m[3] > 0., "{m:?}");
        assert_eq!(slerp([0.; 4], a, 0.5), None);
    }
}
