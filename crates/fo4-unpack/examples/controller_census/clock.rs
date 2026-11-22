//! The clock part of `controller_census` (`spec/controller-clock.md`):
//! every controller of a counted model by what it drives (the target field)
//! and by what runs it (free-running, a manager's flag, or the sequences
//! naming it), and for the free-running ones whether the clock
//! (`fo4_scene::clock`) can run them, their timing fields, key types and
//! counts, the keys against the controller's span, the stored field against
//! the value the clock gives at load, the seam of a loop and the tangents
//! of quadratic keys; the looping sequences (`manager-loop`) by base record
//! type, sequence count and controlled block types; and the transform
//! controllers whose keys move a node (`transform-varies`) by key types,
//! cycle and collision under the node. Counts only.
use super::{add, bump, Counts};
use fo4_core::{Budget, Result};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_scene::{clock, model_policy::fields, rest};
use std::collections::{BTreeMap, BTreeSet};

fn type_of<'n>(n: &'n Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map_or("", String::as_str)
}
fn block_fields<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [Field]> {
    n.blocks.get(i)?.decoded.as_ref().map(fields)
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
fn floats(f: &[Field], name: &str) -> Vec<f32> {
    f.iter()
        .filter(|x| x.name == name)
        .filter_map(|x| match x.value {
            Value::FloatBits(b) => Some(f32::from_bits(b)),
            _ => None,
        })
        .collect()
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

/// The field a controller drives, by type and controlled variable (nifxml
/// enums, paired by name as in `fo4_scene::rest`).
fn field(ty: &str, variable: Option<u32>) -> String {
    let v = variable.unwrap_or(u32::MAX);
    match (ty, v) {
        ("BSEffectShaderPropertyFloatController", 0) => "effect emissive multiple".into(),
        ("BSEffectShaderPropertyFloatController", 1..=4) => "effect falloff".into(),
        ("BSEffectShaderPropertyFloatController", 5) => "effect alpha".into(),
        ("BSEffectShaderPropertyFloatController", 6) => "effect U offset".into(),
        ("BSEffectShaderPropertyFloatController", 7) => "effect U scale".into(),
        ("BSEffectShaderPropertyFloatController", 8) => "effect V offset".into(),
        ("BSEffectShaderPropertyFloatController", 9) => "effect V scale".into(),
        ("BSEffectShaderPropertyColorController", _) => "effect emissive colour".into(),
        ("BSLightingShaderPropertyFloatController", 12) => "lighting alpha".into(),
        ("BSLightingShaderPropertyFloatController", 20) => "lighting U offset".into(),
        ("BSLightingShaderPropertyFloatController", 21) => "lighting U scale".into(),
        ("BSLightingShaderPropertyFloatController", 22) => "lighting V offset".into(),
        ("BSLightingShaderPropertyFloatController", 23) => "lighting V scale".into(),
        ("BSLightingShaderPropertyFloatController", v) => format!("lighting float {v}"),
        ("BSLightingShaderPropertyColorController", v) => format!("lighting colour {v}"),
        ("NiTransformController" | "NiKeyframeController", _) => "node transform".into(),
        ("NiAlphaController", _) => "material property alpha".into(),
        ("BSNiAlphaPropertyTestRefController", _) => "alpha test reference".into(),
        ("NiTextureTransformController" | "NiUVController", _) => {
            "texturing property transform".into()
        }
        ("NiVisController", _) => "visibility".into(),
        (t, _) if t.starts_with("NiLight") => "light".into(),
        (t, _) => format!("other ({t})"),
    }
}
/// The stored value of the shader field a controller drives, where the
/// field is one the draw reads (UV, alpha, the effect terms).
fn stored(n: &Nif<'_>, ty: &str, f: &[Field], variable: u32) -> Option<Vec<f32>> {
    let shader = links(f, "Target").first().copied()?;
    let sf = block_fields(n, shader)?;
    match (ty, type_of(n, shader)) {
        ("BSEffectShaderPropertyFloatController", "BSEffectShaderProperty") => {
            rest::effect_field(sf, false, variable)
        }
        ("BSEffectShaderPropertyColorController", "BSEffectShaderProperty") => {
            rest::effect_field(sf, true, variable)
        }
        ("BSLightingShaderPropertyFloatController", "BSLightingShaderProperty") => {
            if variable == 12 {
                floats(sf, "Alpha").first().map(|v| vec![*v])
            } else {
                rest::lighting_texture_field(sf, variable).map(|v| vec![v])
            }
        }
        _ => None,
    }
}
fn names_material(n: &Nif<'_>, f: &[Field]) -> bool {
    let Some(shader) = links(f, "Target").first().copied() else {
        return false;
    };
    block_fields(n, shader).is_some_and(|sf| {
        sf.iter().any(|x| match x.value {
            Value::StringIndex(s) if x.name == "Name" => {
                n.strings.get(s as usize).is_some_and(|v| {
                    let v = v.to_ascii_lowercase();
                    v.ends_with(b".bgsm") || v.ends_with(b".bgem")
                })
            }
            _ => false,
        })
    })
}
fn bucket(k: usize) -> &'static str {
    match k {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=8 => "5-8",
        9..=32 => "9-32",
        _ => "more than 32",
    }
}
fn kind(k: u32) -> &'static str {
    match k {
        0 => "none",
        clock::LINEAR => "linear",
        clock::QUADRATIC => "quadratic",
        clock::TBC => "TBC",
        clock::XYZ => "XYZ",
        clock::CONSTANT => "constant",
        _ => "other",
    }
}
fn cycle(c: u32) -> &'static str {
    match c {
        clock::CYCLE_LOOP => "loop",
        clock::CYCLE_REVERSE => "reverse",
        clock::CYCLE_CLAMP => "clamp",
        _ => "3",
    }
}
fn cmp(a: f32, b: f32) -> &'static str {
    if (a - b).abs() <= 1e-4 {
        "at"
    } else if a < b {
        "before"
    } else {
        "after"
    }
}
fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= rest::EFFECT_TOLERANCE)
}

/// Whether node `i` or a block under it has a collision object.
fn collision_under(n: &Nif<'_>, i: usize) -> bool {
    let mut stack = vec![i];
    let mut seen = BTreeSet::new();
    while let Some(i) = stack.pop() {
        if !seen.insert(i) || seen.len() > 4096 {
            continue;
        }
        let Some(d) = n.blocks.get(i).and_then(|b| b.decoded.as_ref()) else {
            continue;
        };
        if !links(fields(d), "Collision").is_empty() {
            return true;
        }
        if let Data::Node { children, .. } = d {
            stack.extend(children.iter().filter(|x| **x >= 0).map(|x| *x as usize));
        }
    }
    false
}
/// The key type of each group of an `NiTransformData` (rotation,
/// translation, scale) in the reader's field order; `None` for an empty
/// group.
fn transform_kinds(f: &[Field]) -> [Option<u32>; 3] {
    let mut it = f
        .iter()
        .filter(|x| {
            matches!(
                x.name,
                "Num Rotation Keys" | "Rotation Type" | "Num Keys" | "Interpolation"
            )
        })
        .filter_map(|x| match x.value {
            Value::U32(v) => Some(v),
            _ => None,
        });
    let mut out = [None; 3];
    let group = |it: &mut dyn Iterator<Item = u32>| -> Option<u32> {
        let count = it.next()?;
        if count == 0 {
            None
        } else {
            it.next()
        }
    };
    if it.next().unwrap_or(0) > 0 {
        out[0] = it.next();
        if out[0] == Some(clock::XYZ) {
            for _ in 0..3 {
                group(&mut it);
            }
        }
    }
    out[1] = group(&mut it);
    out[2] = group(&mut it);
    out
}

/// Tallies the clock part for one model: `state` its reached state blocks,
/// `tags` the rest tags they carry, `uses` its enabled placed references,
/// `bases` those by base record type.
pub fn census(
    c: &mut Counts,
    n: &Nif<'_>,
    state: &[usize],
    tags: &BTreeSet<&str>,
    uses: u64,
    bases: &BTreeMap<String, u64>,
    b: &Budget,
) -> Result<()> {
    let has_manager = (0..n.blocks.len()).any(|i| type_of(n, i) == "NiControllerManager");
    let graph = (0..n.blocks.len()).any(|i| type_of(n, i) == "BSBehaviorGraphExtraData");
    // Cycle types of the sequences naming each controller.
    let mut by_sequence: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
    let mut sequences = 0u32;
    let mut looping = 0u32;
    let mut loop_types: BTreeSet<String> = BTreeSet::new();
    for &i in state {
        if type_of(n, i) != "NiControllerSequence" {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            continue;
        };
        sequences += 1;
        let cyc = word(f, "Cycle Type").unwrap_or(u32::MAX);
        for ctl in links(f, "Controller") {
            by_sequence.entry(ctl).or_default().insert(cyc);
        }
        let freq = floats(f, "Frequency").first().copied().unwrap_or(f32::NAN);
        let (start, stop) = (
            floats(f, "Start Time").first().copied().unwrap_or(f32::NAN),
            floats(f, "Stop Time").first().copied().unwrap_or(f32::NAN),
        );
        let label =
            format!(
            "clock: sequences: cycle {}, frequency {}, start {}, span {}, behaviour graph {graph}",
            cycle(cyc),
            if freq == 1. { "1" } else { "other" },
            if start == 0. { "0" } else { "other" },
            if stop > start { "positive" } else { "not positive" },
        );
        bump(c, label.clone());
        add(c, format!("{label} (placements)"), uses);
        if cyc == clock::CYCLE_LOOP {
            looping += 1;
            let names: Vec<String> = f
                .iter()
                .filter(|x| x.name == "Controller Type")
                .map(|x| match x.value {
                    Value::StringIndex(s) => n
                        .strings
                        .get(s as usize)
                        .map(|v| String::from_utf8_lossy(v).into_owned())
                        .unwrap_or_else(|| "none".into()),
                    _ => "none".into(),
                })
                .collect();
            loop_types.extend(names);
        }
    }
    if tags.contains(rest::MANAGER_LOOP) {
        let sigs: Vec<&str> = bases.keys().map(String::as_str).collect();
        let open: Vec<&str> = tags
            .iter()
            .copied()
            .filter(|t| *t != rest::MANAGER_LOOP && rest::rule(t).is_none())
            .collect();
        let lines = [
            format!(
                "manager-loop: models by base record types: {}",
                if sigs.is_empty() {
                    "unplaced".into()
                } else {
                    sigs.join(" + ")
                }
            ),
            format!(
                "manager-loop: models by sequences {sequences}, looping {looping}, behaviour graph {graph}"
            ),
            format!(
                "manager-loop: models by controlled block types of looping sequences: {}",
                loop_types.iter().cloned().collect::<Vec<_>>().join(" + ")
            ),
            format!(
                "manager-loop: models by other undecided tags: {}",
                if open.is_empty() {
                    "none".into()
                } else {
                    open.join(" + ")
                }
            ),
        ];
        for l in lines {
            bump(c, l.clone());
            add(c, l.replacen("models", "placements", 1), uses);
        }
        if open.is_empty() {
            let l = format!(
                "manager-loop: models decided by base record types: {}; sequences {sequences}, looping {looping}",
                if sigs.is_empty() {
                    "unplaced".into()
                } else {
                    sigs.join(" + ")
                }
            );
            bump(c, l.clone());
            add(c, l.replacen("models", "placements", 1), uses);
        }
    }
    // Models with a UV or alpha track, and whether a free-running track of
    // another effect field (emissive, colour, falloff) varies beside it.
    let mut uv_alpha_track = false;
    let mut other_varies = false;
    for &i in state {
        let ty = type_of(n, i);
        if matches!(
            ty,
            "NiControllerManager" | "NiMultiTargetTransformController"
        ) || ty.starts_with("NiPS")
            || ty.starts_with("BSPSys")
        {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            continue;
        };
        let Some(timing) = clock::Timing::read(f) else {
            continue;
        };
        let variable = word(f, "Controlled Variable");
        let fld = field(ty, variable);
        let driver = match by_sequence.get(&i) {
            Some(cycles) => format!(
                "sequence ({})",
                cycles
                    .iter()
                    .map(|c| cycle(*c))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None if timing.manager() && has_manager => "manager flag, no sequence".into(),
            None if timing.manager() => "manager flag without a manager".into(),
            None => "free-running".into(),
        };
        let label = format!("clock: controllers: {fld}; {driver}");
        bump(c, label.clone());
        add(c, format!("{label} (placements)"), uses);
        if driver != "free-running" {
            continue;
        }
        if ty == "NiTransformController" {
            transform(c, n, f, &timing, tags, uses);
            continue;
        }
        let flags = format!(
            "clock: free {fld}: flags active {}, backwards {}, APP_INIT {}, scaled time {}",
            timing.active(),
            timing.backwards(),
            timing.app_init(),
            timing.flags & 0x40 != 0
        );
        bump(c, flags);
        let track = match clock::track(n, i, b)? {
            Ok(t) => t,
            Err(why) => {
                let why = if why == "span not positive" {
                    let k = |v: f32| {
                        if v == f32::MAX {
                            "FLT_MAX".to_string()
                        } else if v == f32::MIN {
                            "-FLT_MAX".to_string()
                        } else if v == f32::MIN_POSITIVE {
                            "FLT_MIN".to_string()
                        } else if v == 0. {
                            "0".to_string()
                        } else {
                            "other".to_string()
                        }
                    };
                    format!("{why} (start {}, stop {})", k(timing.start), k(timing.stop))
                } else {
                    why.to_string()
                };
                let l = format!("clock: free {fld}: not a clock track: {why}");
                bump(c, l.clone());
                add(c, format!("{l} (placements)"), uses);
                continue;
            }
        };
        let k = &track.keys;
        let tm = &track.timing;
        let keys = format!(
            "clock: free {fld}: track: cycle {}, key type {}, keys {}",
            cycle(tm.cycle()),
            kind(k.kind),
            bucket(k.keys.len())
        );
        bump(c, keys.clone());
        add(c, format!("{keys} (placements)"), uses);
        bump(
            c,
            format!(
                "clock: free {fld}: timing: frequency {}, phase {}, start {}",
                if tm.frequency == 1. { "1" } else { "other" },
                if tm.phase == 0. { "0" } else { "other" },
                if tm.start == 0. { "0" } else { "other" },
            ),
        );
        let (first, last) = (k.keys[0].time, k.keys[k.keys.len() - 1].time);
        bump(
            c,
            format!(
                "clock: free {fld}: first key {} start, last key {} stop",
                cmp(first, tm.start),
                cmp(last, tm.stop)
            ),
        );
        let period = f64::from(tm.stop - tm.start) / f64::from(tm.frequency).abs().max(1e-9);
        bump(
            c,
            format!(
                "clock: free {fld}: seconds per cycle {}",
                match period {
                    p if p < 1. => "below 1",
                    p if p < 5. => "1 to 5",
                    p if p < 30. => "5 to 30",
                    p if p < 300. => "30 to 300",
                    _ => "300 or more",
                }
            ),
        );
        bump(
            c,
            format!("clock: free {fld}: track constant {}", k.constant()),
        );
        let comps = k.components;
        let at0 = track.at(0.).map(|v| v[..comps].to_vec());
        if let Some(stored) = variable.and_then(|v| stored(n, ty, f, v)) {
            let first_key = k.keys[0].value[..comps].to_vec();
            let some_key = k.keys.iter().any(|x| close(&x.value[..comps], &stored));
            let inside = (0..comps).all(|j| {
                let lo = k
                    .keys
                    .iter()
                    .map(|x| x.value[j])
                    .fold(f32::INFINITY, f32::min);
                let hi = k
                    .keys
                    .iter()
                    .map(|x| x.value[j])
                    .fold(f32::NEG_INFINITY, f32::max);
                stored[j] >= lo - 1e-4 && stored[j] <= hi + 1e-4
            });
            let l = format!(
                "clock: free {fld}: stored field {}; equals the clock at load {}",
                if close(&first_key, &stored) {
                    "equals the first key"
                } else if some_key {
                    "equals another key"
                } else if inside {
                    "inside the key range"
                } else {
                    "outside the key range"
                },
                at0.as_deref().is_some_and(|v| close(v, &stored))
            );
            bump(c, l.clone());
            add(c, format!("{l} (placements)"), uses);
        }
        bump(
            c,
            format!(
                "clock: free {fld}: shader names a material {}",
                names_material(n, f)
            ),
        );
        if tm.cycle() == clock::CYCLE_LOOP {
            let (a, z) = (k.sample(f64::from(tm.start)), k.sample(f64::from(tm.stop)));
            if let (Some(a), Some(z)) = (a, z) {
                let d = z[0] - a[0];
                let seam = if d.abs() <= 1e-4 {
                    "same value at start and stop"
                } else if (d - d.round()).abs() <= 1e-4 {
                    "whole-number step at the seam"
                } else {
                    "other step at the seam"
                };
                bump(c, format!("clock: free {fld}: loop seam: {seam}"));
            }
        }
        if k.kind == clock::QUADRATIC {
            for w in k.keys.windows(2) {
                let dt = f64::from(w[1].time - w[0].time);
                let dv = f64::from(w[1].value[0] - w[0].value[0]);
                if dt <= 0. || (dt - 1.).abs() < 1e-3 || dv.abs() < 1e-4 {
                    continue;
                }
                let fwd = f64::from(w[0].forward[0]);
                let (per_interval, per_second) = ((fwd - dv).abs(), (fwd - dv / dt).abs());
                let l = if per_interval <= 1e-3 * dv.abs().max(1.) && per_second > per_interval {
                    "forward tangent equals the interval's difference"
                } else if per_second <= 1e-3 * dv.abs().max(1.) {
                    "forward tangent equals the difference per second"
                } else if fwd == 0. {
                    "forward tangent zero"
                } else {
                    "forward tangent neither"
                };
                bump(
                    c,
                    format!("clock: quadratic intervals with a time step not 1: {l}"),
                );
            }
        }
        for r in track.rules() {
            add(
                c,
                format!("clock: placements of models with a track relying on {r}"),
                uses,
            );
        }
        if rest::clock_field(n, i).is_some() {
            uv_alpha_track = true;
        } else if ty.starts_with("BSEffectShaderProperty") && !k.constant() {
            other_varies = true;
        }
    }
    if uv_alpha_track {
        let decided = tags.iter().all(|t| rest::rule(t).is_some());
        let l = format!(
            "clock: models with a UV or alpha track: decided by the rest rules {decided}, a free-running emissive, colour or falloff track that varies {other_varies}"
        );
        bump(c, l.clone());
        add(c, l.replacen("models", "placements", 1), uses);
    }
    Ok(())
}

/// A free-running transform controller: key types, cycle, constancy and
/// collision under its node.
fn transform(
    c: &mut Counts,
    n: &Nif<'_>,
    f: &[Field],
    timing: &clock::Timing,
    tags: &BTreeSet<&str>,
    uses: u64,
) {
    let varies = tags.contains(rest::TRANSFORM_VARIES);
    let data = links(f, "Interpolator")
        .first()
        .and_then(|ip| block_fields(n, *ip))
        .and_then(|ip| links(ip, "Data").first().copied());
    let kinds = data
        .and_then(|d| block_fields(n, d))
        .map(transform_kinds)
        .unwrap_or([None; 3]);
    let k = |x: Option<u32>| x.map_or("none", kind);
    let collision = links(f, "Target")
        .first()
        .is_some_and(|t| collision_under(n, *t));
    let l = format!(
        "transform: free controllers (model tag transform-varies {varies}): cycle {}, rotation {}, translation {}, scale {}, collision under the node {collision}",
        cycle(timing.cycle()),
        k(kinds[0]),
        k(kinds[1]),
        k(kinds[2]),
    );
    bump(c, l.clone());
    add(c, format!("{l} (placements)"), uses);
}
