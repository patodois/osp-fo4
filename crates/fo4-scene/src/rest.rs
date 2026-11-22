//! What a model with animation-controller blocks looks like at rest, under
//! pending rules split by what each controller drives
//! (`spec/controller-rest.md`, `spec/hypotheses.tsv`).
//!
//! [`tags`] gives every block of the `state` class the tags of what uses
//! it: a controller its own, an interpolator or key data block the tags of
//! the controllers and sequences whose interpolator links reach it. Five
//! tags have a rule ([`MANAGER_IDLE`], [`IDLE_TRANSFORM`], [`IDLE_SHADER`],
//! [`TRANSFORM_CONSTANT`], [`UNRENDERED`]); every other tag names why the
//! rest state is not decided.
//! Field layouts are nifxml's (`spec/nif.md`); the controller flag bits,
//! the shader controlled-variable enums and the quaternion field order are
//! the pinned nifxml copy's.
use crate::model_policy::{block_class, fields};
use fo4_core::{Budget, Result};
use fo4_nif::{Field, Nif, Value};
use fo4_plugin::static_frame::Class;

/// A controller manager, its sequences, object palette and text keys
/// (`nif_controller_rest_manager_idle_v1`).
pub const MANAGER_IDLE: &str = "manager-idle";
/// A transform controller run only by a manager's sequences, and what its
/// interpolator links reach (`nif_controller_rest_idle_transform_v1`).
pub const IDLE_TRANSFORM: &str = "idle-transform";
/// An alpha, texture transform, shader or light controller run only by a
/// manager's sequences (`nif_controller_rest_idle_shader_v1`).
pub const IDLE_SHADER: &str = "idle-shader";
/// A transform controller whose interpolator holds only the node's stored
/// transform (`nif_controller_rest_transform_constant_v1`).
pub const TRANSFORM_CONSTANT: &str = "transform-constant";
/// Controllers of lights and of lighting-shader terms the frame does not
/// draw (`nif_controller_rest_unrendered_v1`).
pub const UNRENDERED: &str = "unrendered";
/// Not decided: tags without a rule.
pub const PARTICLE: &str = "particle";
pub const BEHAVIOR_GRAPH: &str = "behavior-graph";
pub const VISIBILITY: &str = "visibility";
pub const TRANSFORM_VARIES: &str = "transform-varies";
pub const ALPHA: &str = "alpha";
pub const TEXTURE_TRANSFORM: &str = "texture-transform";
pub const EFFECT_EMISSIVE: &str = "effect-emissive";
pub const FALLOFF: &str = "falloff";
pub const OTHER: &str = "other";
/// A sequence that loops (nifxml CycleType 0), and what its controlled
/// blocks reach: it may be playing at rest.
pub const MANAGER_LOOP: &str = "manager-loop";
/// An effect-shader controller run by a manager: the frame draws effects
/// without their falloff and soft terms, so their stored alpha and colour
/// are not settled to be what shows at rest.
pub const EFFECT_IDLE: &str = "effect-idle";

/// Translation, rotation word and scale tolerances of "equal".
pub const TOLERANCE: [f32; 3] = [1e-3, 1e-3, 1e-4];
/// nifxml `#INV_FLT#`: a pose value that is not set.
pub const INVALID: f32 = -3.402_823_4e38;

/// What a controller type drives, with the shader controlled variable
/// (nifxml EffectShaderControlledVariable, EffectShaderControlledColor,
/// LightingShaderControlledFloat, LightingShaderControlledColor).
pub fn drives(ty: &str, variable: Option<u32>) -> &'static str {
    match ty {
        "NiTransformController" | "NiKeyframeController" | "NiMultiTargetTransformController" => {
            "transform"
        }
        "NiVisController" => VISIBILITY,
        "NiAlphaController" | "BSNiAlphaPropertyTestRefController" => ALPHA,
        "NiTextureTransformController" | "NiUVController" => TEXTURE_TRANSFORM,
        "BSEffectShaderPropertyFloatController" => match variable {
            Some(0) => EFFECT_EMISSIVE,
            Some(1..=4) => FALLOFF,
            Some(5) => ALPHA,
            Some(6..=9) => TEXTURE_TRANSFORM,
            _ => OTHER,
        },
        "BSEffectShaderPropertyColorController" => EFFECT_EMISSIVE,
        "BSLightingShaderPropertyFloatController" => match variable {
            Some(0 | 8 | 9 | 10 | 11) => UNRENDERED,
            Some(12) => ALPHA,
            Some(20..=23) => TEXTURE_TRANSFORM,
            _ => OTHER,
        },
        "BSLightingShaderPropertyColorController" => match variable {
            Some(0 | 1) => UNRENDERED,
            _ => OTHER,
        },
        "NiLightColorController"
        | "NiLightDimmerController"
        | "NiLightRadiusController"
        | "NiLightIntensityController" => UNRENDERED,
        _ => OTHER,
    }
}
/// Controller types whose target the frame draws from stored fields, so
/// that a controller left idle leaves its target as stored.
const KNOWN: [&str; 14] = [
    "NiTransformController",
    "NiKeyframeController",
    "NiMultiTargetTransformController",
    "NiAlphaController",
    "BSNiAlphaPropertyTestRefController",
    "NiTextureTransformController",
    "NiUVController",
    "BSEffectShaderPropertyFloatController",
    "BSEffectShaderPropertyColorController",
    "BSLightingShaderPropertyFloatController",
    "BSLightingShaderPropertyColorController",
    "NiLightColorController",
    "NiLightDimmerController",
    "NiLightRadiusController",
];
fn particle(ty: &str) -> bool {
    ty.starts_with("NiPS") || ty.starts_with("BSPSys") || ty == "NiParticleSystem"
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
fn links(f: &[Field], names: &[&str]) -> Vec<usize> {
    f.iter()
        .filter(|x| names.contains(&x.name))
        .filter_map(|x| match x.value {
            Value::Ref(r) if r >= 0 => Some(r as usize),
            _ => None,
        })
        .collect()
}
fn word(f: &[Field], name: &str) -> Option<u32> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match x.value {
            Value::U32(v) => Some(v),
            Value::U16(v) => Some(u32::from(v)),
            _ => None,
        })
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

/// Rotation words (row-major) of a quaternion (w, x, y, z): rows of the
/// matrix acting on column vectors. The census found this reading equal to
/// the node's stored words wherever one of the two readings was
/// (`spec/controller-rest.md`, Transform values).
pub fn quaternion_rows([w, x, y, z]: [f32; 4]) -> [f32; 9] {
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

/// The keys of an NiTransformData in the reader's field order.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TransformKeys {
    /// (time, quaternion w x y z); `None` for XYZ rotation keys.
    pub rotation: Option<Vec<(f32, [f32; 4])>>,
    pub translation: Vec<(f32, [f32; 3])>,
    pub scale: Vec<(f32, f32)>,
}
/// Reads the keys from an NiTransformData's fields; `None` when the field
/// sequence is not the reader's.
pub fn transform_keys(f: &[Field]) -> Option<TransformKeys> {
    let mut it = f.iter();
    let mut next = |name: &str| -> Option<&Field> { it.next().filter(|x| x.name == name) };
    let num = |x: &Field| match x.value {
        Value::U32(v) => Some(v),
        _ => None,
    };
    let fl = |x: &Field| match x.value {
        Value::FloatBits(b) => Some(f32::from_bits(b)),
        _ => None,
    };
    let mut out = TransformKeys::default();
    let nrot = num(next("Num Rotation Keys")?)?;
    out.rotation = Some(Vec::new());
    if nrot > 0 {
        let kind = num(next("Rotation Type")?)?;
        if kind == 4 {
            out.rotation = None;
            for _ in 0..3 {
                let n = num(next("Num Keys")?)?;
                if n > 0 {
                    let k = num(next("Interpolation")?)?;
                    for _ in 0..n {
                        next("Key Time")?;
                        next("Key Value")?;
                        if k == 2 {
                            next("Forward Tangent")?;
                            next("Backward Tangent")?;
                        }
                        if k == 3 {
                            for _ in 0..3 {
                                next("Tension Bias Continuity")?;
                            }
                        }
                    }
                }
            }
        } else {
            let mut v = Vec::new();
            for _ in 0..nrot {
                let t = fl(next("Key Time")?)?;
                let mut q = [0.; 4];
                for c in &mut q {
                    *c = fl(next("Quaternion Value")?)?;
                }
                if kind == 3 {
                    for _ in 0..3 {
                        next("Tension Bias Continuity")?;
                    }
                }
                v.push((t, q));
            }
            out.rotation = Some(v);
        }
    }
    for comps in [3usize, 1] {
        let n = num(next("Num Keys")?)?;
        if n == 0 {
            continue;
        }
        let k = num(next("Interpolation")?)?;
        for _ in 0..n {
            let t = fl(next("Key Time")?)?;
            let mut v = [0.; 3];
            for c in v.iter_mut().take(comps) {
                *c = fl(next("Key Value")?)?;
            }
            if k == 2 {
                for _ in 0..comps {
                    next("Forward Tangent")?;
                }
                for _ in 0..comps {
                    next("Backward Tangent")?;
                }
            }
            if k == 3 {
                for _ in 0..3 {
                    next("Tension Bias Continuity")?;
                }
            }
            if comps == 3 {
                out.translation.push((t, v));
            } else {
                out.scale.push((t, v[0]));
            }
        }
    }
    it.next().is_none().then_some(out)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0., f32::max)
}

/// Whether transform interpolator `ip` can only give node `node` its stored
/// transform: every set pose value and every key equal to it.
pub fn transform_constant(n: &Nif<'_>, ip: usize, node: usize) -> bool {
    if type_of(n, ip) != "NiTransformInterpolator" {
        return false;
    }
    let (Some(f), Some(nf)) = (block_fields(n, ip), block_fields(n, node)) else {
        return false;
    };
    let (st, sr, ss) = (
        floats(nf, "Translation"),
        floats(nf, "Rotation"),
        floats(nf, "Scale"),
    );
    if st.len() != 3 || sr.len() != 9 || ss.len() != 1 {
        return false;
    }
    let [tt, tr, ts] = TOLERANCE;
    let t_ok = |v: &[f32]| max_diff(v, &st) <= tt;
    let r_ok = |q: [f32; 4]| max_diff(&quaternion_rows(q), &sr) <= tr;
    let s_ok = |v: f32| (v - ss[0]).abs() <= ts;
    let (pt, pq, ps) = (
        floats(f, "Translation"),
        floats(f, "Quaternion"),
        floats(f, "Scale"),
    );
    if pt.len() != 3 || pq.len() != 4 || ps.len() != 1 {
        return false;
    }
    if pt[0] != INVALID && !t_ok(&pt) {
        return false;
    }
    if pq[0] != INVALID && !r_ok([pq[0], pq[1], pq[2], pq[3]]) {
        return false;
    }
    if ps[0] != INVALID && !s_ok(ps[0]) {
        return false;
    }
    let data = links(f, &["Data"]);
    let Some(d) = data.first() else {
        return true;
    };
    if type_of(n, *d) != "NiTransformData" {
        return false;
    }
    let Some(k) = block_fields(n, *d).and_then(transform_keys) else {
        return false;
    };
    let Some(rot) = &k.rotation else {
        return false;
    };
    rot.iter().all(|(_, q)| r_ok(*q))
        && k.translation.iter().all(|(_, v)| t_ok(v))
        && k.scale.iter().all(|(_, v)| s_ok(*v))
}

/// The rest tags of every block (empty for blocks outside the `state` class).
pub fn tags(n: &Nif<'_>, b: &Budget) -> Result<Vec<Vec<&'static str>>> {
    let count = n.blocks.len();
    let mut out: Vec<Vec<&'static str>> = Vec::new();
    b.reserve(&mut out, count, "rest tags", 0)?;
    out.resize(count, Vec::new());
    let add = |out: &mut Vec<Vec<&'static str>>, i: usize, t: &'static str| {
        if let Some(v) = out.get_mut(i) {
            if !v.contains(&t) {
                v.push(t);
            }
        }
    };
    let has_manager = (0..count).any(|i| type_of(n, i) == "NiControllerManager");
    // Interpolator chains: interpolator -> data, blend -> interpolators.
    let chain = |start: usize| -> Vec<usize> {
        let mut seen = Vec::new();
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            if i >= count || seen.contains(&i) || seen.len() > 4096 {
                continue;
            }
            seen.push(i);
            if let Some(f) = block_fields(n, i) {
                stack.extend(links(f, &["Data", "Interpolator"]));
            }
        }
        seen
    };
    for i in 0..count {
        let ty = type_of(n, i);
        if block_class(ty) != Class::State {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            add(&mut out, i, OTHER);
            continue;
        };
        if particle(ty) {
            add(&mut out, i, PARTICLE);
            continue;
        }
        match ty {
            "BSBehaviorGraphExtraData" => add(&mut out, i, BEHAVIOR_GRAPH),
            "NiControllerManager" | "NiMultiTargetTransformController" => {
                add(&mut out, i, MANAGER_IDLE)
            }
            "NiDefaultAVObjectPalette" | "NiTextKeyExtraData" => {
                add(&mut out, i, if has_manager { MANAGER_IDLE } else { OTHER })
            }
            "NiControllerSequence" => {
                let looping = word(f, "Cycle Type") == Some(0);
                add(
                    &mut out,
                    i,
                    if looping { MANAGER_LOOP } else { MANAGER_IDLE },
                );
                // Controlled blocks: interpolator, controller, ... in order.
                let ips: Vec<i32> = f
                    .iter()
                    .filter(|x| x.name == "Interpolator")
                    .filter_map(|x| match x.value {
                        Value::Ref(r) => Some(r),
                        _ => None,
                    })
                    .collect();
                let ctls: Vec<i32> = f
                    .iter()
                    .filter(|x| x.name == "Controller")
                    .filter_map(|x| match x.value {
                        Value::Ref(r) => Some(r),
                        _ => None,
                    })
                    .collect();
                let names: Vec<Option<&[u8]>> = f
                    .iter()
                    .filter(|x| x.name == "Controller Type")
                    .map(|x| match x.value {
                        Value::StringIndex(s) => n.strings.get(s as usize).map(Vec::as_slice),
                        _ => None,
                    })
                    .collect();
                for (k, ip) in ips.iter().enumerate() {
                    let ctl_ty = ctls
                        .get(k)
                        .filter(|c| **c >= 0)
                        .map(|c| type_of(n, *c as usize).as_bytes())
                        .or_else(|| names.get(k).copied().flatten())
                        .unwrap_or(b"");
                    let ctl_ty = std::str::from_utf8(ctl_ty).unwrap_or("");
                    let tag = if particle(ctl_ty) {
                        PARTICLE
                    } else if ctl_ty == "NiVisController" {
                        VISIBILITY
                    } else if !KNOWN.contains(&ctl_ty) {
                        OTHER
                    } else if looping {
                        MANAGER_LOOP
                    } else if ctl_ty.starts_with("BSEffectShaderProperty") {
                        EFFECT_IDLE
                    } else if drives(ctl_ty, None) == "transform" {
                        IDLE_TRANSFORM
                    } else {
                        IDLE_SHADER
                    };
                    if *ip >= 0 {
                        for x in chain(*ip as usize) {
                            add(&mut out, x, tag);
                        }
                    }
                }
            }
            _ => {
                let Some(flags) = word(f, "Controller Flags") else {
                    continue;
                };
                let class = drives(ty, word(f, "Controlled Variable"));
                let manager = flags & 0x20 != 0;
                let ips = links(f, &["Interpolator", "Visibility Interpolator"]);
                let tag = if class == VISIBILITY || class == OTHER && !KNOWN.contains(&ty) {
                    class
                } else if manager && has_manager && class == "transform" {
                    IDLE_TRANSFORM
                } else if manager && has_manager && ty.starts_with("BSEffectShaderProperty") {
                    EFFECT_IDLE
                } else if manager && has_manager {
                    IDLE_SHADER
                } else if class == "transform" {
                    let target = links(f, &["Target"]).first().copied();
                    match (target, ips.as_slice()) {
                        (Some(t), [ip]) if transform_constant(n, *ip, t) => TRANSFORM_CONSTANT,
                        _ => TRANSFORM_VARIES,
                    }
                } else {
                    class
                };
                add(&mut out, i, tag);
                for ip in ips {
                    for x in chain(ip) {
                        add(&mut out, x, tag);
                    }
                }
            }
        }
    }
    for (i, v) in out.iter_mut().enumerate() {
        if v.is_empty() && block_class(type_of(n, i)) == Class::State {
            v.push(OTHER);
        }
        v.sort_unstable();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hypotheses::{covering, REST_IDLE_SHADER, REST_MANAGER_IDLE, REST_UNRENDERED};

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

    #[test]
    fn controller_classes_follow_the_nifxml_enums() {
        assert_eq!(drives("NiTransformController", None), "transform");
        assert_eq!(
            drives("BSLightingShaderPropertyFloatController", Some(11)),
            UNRENDERED
        );
        assert_eq!(
            drives("BSLightingShaderPropertyFloatController", Some(12)),
            ALPHA
        );
        assert_eq!(
            drives("BSLightingShaderPropertyFloatController", Some(22)),
            TEXTURE_TRANSFORM
        );
        assert_eq!(
            drives("BSLightingShaderPropertyFloatController", Some(1)),
            OTHER
        );
        assert_eq!(
            drives("BSEffectShaderPropertyFloatController", Some(0)),
            EFFECT_EMISSIVE
        );
        assert_eq!(
            drives("BSEffectShaderPropertyFloatController", Some(3)),
            FALLOFF
        );
        assert_eq!(drives("NiVisController", None), VISIBILITY);
        assert_eq!(drives("NiLightDimmerController", None), UNRENDERED);
        assert_eq!(drives("BSLagBoneController", None), OTHER);
    }

    #[test]
    fn quaternion_rows_and_keys_are_read_in_field_order() {
        // A quarter turn about z: rows (0,-1,0), (1,0,0), (0,0,1).
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let m = quaternion_rows([h, 0., 0., h]);
        assert!(max_diff(&m, &[0., -1., 0., 1., 0., 0., 0., 0., 1.]) < 1e-6);
        // One linear quaternion key, two translation keys, no scale keys.
        let mut v = vec![
            f("Num Rotation Keys", Value::U32(1)),
            f("Rotation Type", Value::U32(1)),
            fl("Key Time", 0.),
        ];
        v.extend([1., 0., 0., 0.].map(|x| fl("Quaternion Value", x)));
        v.push(f("Num Keys", Value::U32(2)));
        v.push(f("Interpolation", Value::U32(1)));
        for t in [0., 1.] {
            v.push(fl("Key Time", t));
            v.extend([t, 2., 3.].map(|x| fl("Key Value", x)));
        }
        v.push(f("Num Keys", Value::U32(0)));
        let k = transform_keys(&v).unwrap();
        assert_eq!(k.rotation.as_deref(), Some(&[(0., [1., 0., 0., 0.])][..]));
        assert_eq!(k.translation, [(0., [0., 2., 3.]), (1., [1., 2., 3.])]);
        assert!(k.scale.is_empty());
        // A field out of order is refused, as is a trailing one.
        v.swap(0, 1);
        assert!(transform_keys(&v).is_none());
        v.swap(0, 1);
        v.push(fl("Key Time", 0.));
        assert!(transform_keys(&v).is_none());
    }

    #[test]
    fn only_decided_tags_are_covered() {
        let e = |tag: &str| {
            fo4_core::Error::new(
                "UnsupportedAnimationRuntime",
                "t.nif",
                0,
                "x",
                format!("NiControllerSequence;target_in_visual=None;rest={tag}"),
            )
        };
        assert_eq!(covering(&e(MANAGER_IDLE)), Some(REST_MANAGER_IDLE));
        assert_eq!(covering(&e(IDLE_SHADER)), Some(REST_IDLE_SHADER));
        assert_eq!(covering(&e(UNRENDERED)), Some(REST_UNRENDERED));
        for open in [
            PARTICLE,
            BEHAVIOR_GRAPH,
            VISIBILITY,
            TRANSFORM_VARIES,
            ALPHA,
            TEXTURE_TRANSFORM,
            EFFECT_EMISSIVE,
            FALLOFF,
            OTHER,
            MANAGER_LOOP,
            EFFECT_IDLE,
        ] {
            assert_eq!(covering(&e(open)), None, "{open}");
        }
        // The text before this step names no rest state: not covered.
        let old = fo4_core::Error::new(
            "UnsupportedAnimationRuntime",
            "t.nif",
            0,
            "x",
            "NiTransformController;target_in_visual=Some(true)",
        );
        assert_eq!(covering(&old), None);
    }
}
