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
            return Ok((count == 0 && it.next().is_none()).then_some(out));
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
        Ok((it.next().is_none() && ordered).then_some(out))
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
}
