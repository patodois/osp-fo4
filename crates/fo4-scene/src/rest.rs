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
/// An effect-shader controller whose target field holds, as stored, the
/// first key of every interpolator that drives it (its own, and the
/// controlled blocks of every sequence that names it), with what those
/// interpolators reach, and a looping sequence whose controlled blocks are
/// all such controllers (`nif_controller_rest_effect_first_key_v1`).
pub const EFFECT_FIRST_KEY: &str = "effect-first-key";
/// A lighting-shader texture-transform controller (U or V offset or scale)
/// whose target field holds, as stored, the first key of every
/// interpolator that drives it, with what those interpolators reach, where
/// it is not run by a manager or is run by a looping sequence
/// (`nif_controller_rest_texture_first_key_v1`).
pub const TEXTURE_FIRST_KEY: &str = "texture-first-key";
/// An effect-shader controller driving Falloff Start Angle or Falloff Stop
/// Angle (EffectShaderControlledVariable 1, 2) settled under the first-key
/// rule only when its key and pose values are read as degrees and the
/// stored field as their cosine (`nif_controller_falloff_angle_degrees_v1`),
/// with what its interpolators reach. Always given with
/// [`EFFECT_FIRST_KEY`].
pub const FALLOFF_DEGREES: &str = "falloff-degrees";
/// Tolerance of "equal" between a stored effect-shader field and a key.
pub const EFFECT_TOLERANCE: f32 = 1e-4;

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

/// The stored effect-shader field a controller drives, by the nifxml
/// enums EffectShaderControlledColor (colour controller, 0: Emissive Color,
/// read as the RGB of Base Color) and EffectShaderControlledVariable (0:
/// EmissiveMultiple, read as Base Color Scale; 1 to 4 the falloff fields;
/// 5: Alpha Transparency, read as the alpha of Base Color; 6 to 9 the UV
/// offset and scale components). The pairing of variable and field is by
/// name only; `None` for another variable.
pub fn effect_field(shader: &[Field], color: bool, variable: u32) -> Option<Vec<f32>> {
    let one = |name: &str| floats(shader, name).first().map(|v| vec![*v]);
    let base = floats(shader, "Base Color");
    match (color, variable) {
        (true, 0) => base.get(..3).map(<[f32]>::to_vec),
        (false, 0) => one("Base Color Scale"),
        (false, 1) => one("Falloff Start Angle"),
        (false, 2) => one("Falloff Stop Angle"),
        (false, 3) => one("Falloff Start Opacity"),
        (false, 4) => one("Falloff Stop Opacity"),
        (false, 5) => base.get(3).map(|v| vec![*v]),
        (false, 6) => floats(shader, "UV Offset").first().map(|v| vec![*v]),
        (false, 7) => floats(shader, "UV Scale").first().map(|v| vec![*v]),
        (false, 8) => floats(shader, "UV Offset").get(1).map(|v| vec![*v]),
        (false, 9) => floats(shader, "UV Scale").get(1).map(|v| vec![*v]),
        _ => None,
    }
}
/// The first key (lowest time; key times must not decrease) of an
/// NiFloatData or NiPosData read in the reader's field order.
pub fn first_key(f: &[Field], components: usize) -> Option<Vec<f32>> {
    let mut it = f.iter();
    let count = match it.next() {
        Some(x) if x.name == "Num Keys" => match x.value {
            Value::U32(v) => v,
            _ => return None,
        },
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let mut keys: Vec<(f32, Vec<f32>)> = Vec::new();
    for x in it {
        let Value::FloatBits(b) = x.value else {
            continue;
        };
        let v = f32::from_bits(b);
        match x.name {
            "Key Time" => keys.push((v, Vec::new())),
            "Key Value" => {
                let last = keys.last_mut()?;
                if last.1.len() < components {
                    last.1.push(v);
                }
            }
            _ => {}
        }
    }
    let ordered = keys.windows(2).all(|w| w[0].0 <= w[1].0);
    (keys.len() == count as usize && ordered && keys.iter().all(|k| k.1.len() == components))
        .then(|| keys.swap_remove(0).1)
}
fn effect_close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.is_finite() && (x - y).abs() <= EFFECT_TOLERANCE)
}
/// Whether interpolator `ip` starts at `stored`: a keyframe interpolator
/// whose set pose value and first key equal it, or a blend interpolator
/// whose set pose value equals it (its keys come from the sequences).
fn starts_at(n: &Nif<'_>, ip: usize, stored: &[f32], blend_ok: bool) -> bool {
    starts_at_as(n, ip, stored, blend_ok, false)
}
/// The cosine of a key or pose value read as degrees.
pub fn degrees_cosine(v: f32) -> f32 {
    (f64::from(v).to_radians().cos()) as f32
}
/// [`starts_at`], with key and pose values read as degrees and compared
/// through their cosine when `degrees` is set.
fn starts_at_as(n: &Nif<'_>, ip: usize, stored: &[f32], blend_ok: bool, degrees: bool) -> bool {
    let read = |v: Vec<f32>| -> Vec<f32> {
        if degrees {
            v.into_iter().map(degrees_cosine).collect()
        } else {
            v
        }
    };
    let Some(f) = block_fields(n, ip) else {
        return false;
    };
    let comps = stored.len();
    match type_of(n, ip) {
        "NiFloatInterpolator" | "NiPoint3Interpolator" => {
            let pose = floats(f, "Value");
            if pose.first().is_some_and(|v| *v != INVALID) && !effect_close(&read(pose), stored) {
                return false;
            }
            let Some(d) = links(f, &["Data"]).first().copied() else {
                return false;
            };
            let want = if comps == 3 {
                "NiPosData"
            } else {
                "NiFloatData"
            };
            type_of(n, d) == want
                && block_fields(n, d)
                    .and_then(|k| first_key(k, comps))
                    .is_some_and(|k| effect_close(&read(k), stored))
        }
        "NiBlendFloatInterpolator" | "NiBlendPoint3Interpolator" if blend_ok => {
            let pose = floats(f, "Pose Value");
            pose.first().is_none_or(|v| *v == INVALID) || effect_close(&read(pose), stored)
        }
        _ => false,
    }
}
/// The interpolators of the sequences' controlled blocks, by the controller
/// block each names.
fn sequence_interpolators(n: &Nif<'_>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..n.blocks.len() {
        if type_of(n, i) != "NiControllerSequence" {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            continue;
        };
        let ips = f.iter().filter(|x| x.name == "Interpolator");
        let ctls = f.iter().filter(|x| x.name == "Controller");
        for (ip, ctl) in ips.zip(ctls) {
            if let (Value::Ref(ip), Value::Ref(ctl)) = (&ip.value, &ctl.value) {
                if *ip >= 0 && *ctl >= 0 {
                    out.push((*ctl as usize, *ip as usize));
                }
            }
        }
    }
    out
}
/// Whether effect-shader controller `ctl` is settled at rest under
/// `nif_controller_rest_effect_first_key_v1`: its target is an effect
/// shader whose driven field holds the first key of every interpolator that
/// drives it. `sequences` are the (controller, interpolator) pairs of
/// [`sequence_interpolators`].
fn effect_settled(n: &Nif<'_>, ctl: usize, sequences: &[(usize, usize)]) -> bool {
    let ty = type_of(n, ctl);
    if !ty.starts_with("BSEffectShaderProperty") {
        return false;
    }
    let Some(f) = block_fields(n, ctl) else {
        return false;
    };
    let Some(target) = links(f, &["Target"]).first().copied() else {
        return false;
    };
    if type_of(n, target) != "BSEffectShaderProperty" {
        return false;
    }
    let (Some(shader), Some(variable)) = (block_fields(n, target), word(f, "Controlled Variable"))
    else {
        return false;
    };
    let color = ty == "BSEffectShaderPropertyColorController";
    let Some(stored) = effect_field(shader, color, variable) else {
        return false;
    };
    let degrees = !color && matches!(variable, 1 | 2);
    let from_sequences: Vec<usize> = sequences
        .iter()
        .filter(|(c, _)| *c == ctl)
        .map(|(_, ip)| *ip)
        .collect();
    let own = links(f, &["Interpolator"]);
    if own.is_empty() && from_sequences.is_empty() {
        return false;
    }
    own.iter()
        .all(|ip| starts_at_as(n, *ip, &stored, !from_sequences.is_empty(), degrees))
        && from_sequences
            .iter()
            .all(|ip| starts_at_as(n, *ip, &stored, false, degrees))
}
/// Whether block `ctl` is an effect-shader float controller of a falloff
/// angle (EffectShaderControlledVariable 1 or 2), whose keys the first-key
/// rule reads as degrees.
pub fn falloff_angle_controller(n: &Nif<'_>, ctl: usize) -> bool {
    type_of(n, ctl) == "BSEffectShaderPropertyFloatController"
        && block_fields(n, ctl)
            .and_then(|f| word(f, "Controlled Variable"))
            .is_some_and(|v| matches!(v, 1 | 2))
}
/// The stored lighting-shader field a texture-transform controller drives,
/// by the nifxml enum LightingShaderControlledFloat (20: U Offset, 21: U
/// Scale, 22: V Offset, 23: V Scale), read as the components of the
/// shader's `UV Offset` and `UV Scale`; `None` for another variable.
pub fn lighting_texture_field(shader: &[Field], variable: u32) -> Option<f32> {
    match variable {
        20 => floats(shader, "UV Offset").first().copied(),
        21 => floats(shader, "UV Scale").first().copied(),
        22 => floats(shader, "UV Offset").get(1).copied(),
        23 => floats(shader, "UV Scale").get(1).copied(),
        _ => None,
    }
}
/// What the census and the rest tags read of a texture-transform
/// controller of a lighting shader.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureController {
    /// The controlled variable (20 to 23).
    pub variable: u32,
    /// Controller flag 0x20 (run by a manager).
    pub manager_flag: bool,
    /// Named by a sequence's controlled blocks.
    pub in_sequence: bool,
    /// The target shader block.
    pub shader: usize,
    /// The stored field the controller drives.
    pub stored: f32,
    /// Every interpolator that drives it starts at `stored`
    /// (`nif_controller_rest_texture_first_key_v1`).
    pub first_key_is_stored: bool,
}
/// The texture-transform controller `ctl` of a lighting shader, or `None`
/// when `ctl` is not a `BSLightingShaderPropertyFloatController` driving a
/// UV field of a `BSLightingShaderProperty`.
pub fn texture_controller(n: &Nif<'_>, ctl: usize) -> Option<TextureController> {
    texture_controller_with(n, ctl, &sequence_interpolators(n))
}
fn texture_controller_with(
    n: &Nif<'_>,
    ctl: usize,
    sequences: &[(usize, usize)],
) -> Option<TextureController> {
    if type_of(n, ctl) != "BSLightingShaderPropertyFloatController" {
        return None;
    }
    let f = block_fields(n, ctl)?;
    let shader = links(f, &["Target"]).first().copied()?;
    if type_of(n, shader) != "BSLightingShaderProperty" {
        return None;
    }
    let variable = word(f, "Controlled Variable")?;
    let stored = lighting_texture_field(block_fields(n, shader)?, variable)?;
    let from_sequences: Vec<usize> = sequences
        .iter()
        .filter(|(c, _)| *c == ctl)
        .map(|(_, ip)| *ip)
        .collect();
    let own = links(f, &["Interpolator"]);
    let first_key_is_stored = !(own.is_empty() && from_sequences.is_empty())
        && own
            .iter()
            .all(|ip| starts_at(n, *ip, &[stored], !from_sequences.is_empty()))
        && from_sequences
            .iter()
            .all(|ip| starts_at(n, *ip, &[stored], false));
    Some(TextureController {
        variable,
        manager_flag: word(f, "Controller Flags").is_some_and(|v| v & 0x20 != 0),
        in_sequence: !from_sequences.is_empty(),
        shader,
        stored,
        first_key_is_stored,
    })
}
/// Which UV fields of lighting shader `shader` a texture-transform
/// controller drives, in the order of LightingShaderControlledFloat 20 to
/// 23: U offset, U scale, V offset, V scale.
pub fn texture_driven(n: &Nif<'_>, shader: usize) -> [bool; 4] {
    let mut out = [false; 4];
    for i in 0..n.blocks.len() {
        if type_of(n, i) != "BSLightingShaderPropertyFloatController" {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            continue;
        };
        if links(f, &["Target"]).first() != Some(&shader) {
            continue;
        }
        if let Some(slot) = word(f, "Controlled Variable")
            .and_then(|v| v.checked_sub(20))
            .and_then(|v| out.get_mut(v as usize))
        {
            *slot = true;
        }
    }
    out
}
/// The (colour, controlled variable) pairs of the effect-shader
/// controllers that target shader block `shader`.
pub fn effect_controlled(n: &Nif<'_>, shader: usize) -> Vec<(bool, u32)> {
    let mut out = Vec::new();
    for i in 0..n.blocks.len() {
        let ty = type_of(n, i);
        if !ty.starts_with("BSEffectShaderProperty") {
            continue;
        }
        let Some(f) = block_fields(n, i) else {
            continue;
        };
        if links(f, &["Target"]).first() == Some(&shader) {
            if let Some(v) = word(f, "Controlled Variable") {
                out.push((ty == "BSEffectShaderPropertyColorController", v));
            }
        }
    }
    out
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
    let sequences = sequence_interpolators(n);
    let settled = |c: usize| effect_settled(n, c, &sequences);
    // A settled falloff angle controller relies on the degrees reading too.
    let degrees = |c: usize| settled(c) && falloff_angle_controller(n, c);
    let texture_settled =
        |c: usize| texture_controller_with(n, c, &sequences).is_some_and(|t| t.first_key_is_stored);
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
                // A looping sequence whose controlled blocks all name a
                // settled effect-shader controller starts at their first
                // keys.
                let ctl_links = links(f, &["Controller"]);
                let all_links = f.iter().filter(|x| x.name == "Controller").count()
                    == ctl_links.len()
                    && !ctl_links.is_empty();
                let all_effect = all_links && ctl_links.iter().all(|c| settled(*c));
                // The same with lighting texture-transform controllers among
                // them: each kind under its own first-key rule.
                let all_first_key =
                    all_links && ctl_links.iter().all(|c| settled(*c) || texture_settled(*c));
                if looping && all_first_key && !all_effect {
                    add(&mut out, i, TEXTURE_FIRST_KEY);
                    if ctl_links.iter().any(|c| settled(*c)) {
                        add(&mut out, i, EFFECT_FIRST_KEY);
                    }
                } else {
                    add(
                        &mut out,
                        i,
                        if looping && all_effect {
                            EFFECT_FIRST_KEY
                        } else if looping {
                            MANAGER_LOOP
                        } else {
                            MANAGER_IDLE
                        },
                    );
                }
                if looping && all_first_key && ctl_links.iter().any(|c| degrees(*c)) {
                    add(&mut out, i, FALLOFF_DEGREES);
                }
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
                    let settled_ctl = ctls
                        .get(k)
                        .filter(|c| **c >= 0)
                        .is_some_and(|c| settled(*c as usize));
                    let degrees_ctl = ctls
                        .get(k)
                        .filter(|c| **c >= 0)
                        .is_some_and(|c| degrees(*c as usize));
                    let texture_ctl = ctls
                        .get(k)
                        .filter(|c| **c >= 0)
                        .is_some_and(|c| texture_settled(*c as usize));
                    let tag = if particle(ctl_ty) {
                        PARTICLE
                    } else if settled_ctl {
                        EFFECT_FIRST_KEY
                    } else if ctl_ty == "NiVisController" {
                        VISIBILITY
                    } else if !KNOWN.contains(&ctl_ty) {
                        OTHER
                    } else if looping && texture_ctl {
                        TEXTURE_FIRST_KEY
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
                            if degrees_ctl {
                                add(&mut out, x, FALLOFF_DEGREES);
                            }
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
                } else if ty.starts_with("BSEffectShaderProperty") && settled(i) {
                    EFFECT_FIRST_KEY
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
                } else if class == TEXTURE_TRANSFORM && texture_settled(i) {
                    TEXTURE_FIRST_KEY
                } else {
                    class
                };
                add(&mut out, i, tag);
                let angle = tag == EFFECT_FIRST_KEY && degrees(i);
                if angle {
                    add(&mut out, i, FALLOFF_DEGREES);
                }
                for ip in ips {
                    for x in chain(ip) {
                        add(&mut out, x, tag);
                        if angle {
                            add(&mut out, x, FALLOFF_DEGREES);
                        }
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

    fn block(type_index: u16, fields: Vec<Field>) -> fo4_nif::Block {
        fo4_nif::Block {
            type_index,
            data: 0..0,
            decoded: Some(fo4_nif::Data::Fields(fields)),
            raw_tail: 0..0,
            diagnostics: Vec::new(),
        }
    }
    /// An effect shader with a stored U offset, a U offset controller and a
    /// float interpolator with keys `keys`; with `sequence`, a looping
    /// sequence names the controller.
    fn effect_nif(stored: f32, keys: &[(f32, f32)], sequence: bool) -> Nif<'static> {
        let shader = vec![
            fl("UV Offset", stored),
            fl("UV Offset", 0.),
            fl("UV Scale", 1.),
            fl("UV Scale", 1.),
            fl("Base Color", 1.),
            fl("Base Color", 1.),
            fl("Base Color", 1.),
            fl("Base Color", 0.5),
            fl("Base Color Scale", 1.),
        ];
        let controller = vec![
            f(
                "Controller Flags",
                Value::U16(if sequence { 0x6c } else { 0x48 }),
            ),
            f("Target", Value::Ref(0)),
            f("Interpolator", Value::Ref(2)),
            f("Controlled Variable", Value::U32(6)),
        ];
        let interpolator = vec![fl("Value", INVALID), f("Data", Value::Ref(3))];
        let mut data = vec![
            f("Num Keys", Value::U32(keys.len() as u32)),
            f("Interpolation", Value::U32(1)),
        ];
        for (t, v) in keys {
            data.push(fl("Key Time", *t));
            data.push(fl("Key Value", *v));
        }
        let mut blocks = vec![
            block(0, shader),
            block(1, controller),
            block(2, interpolator),
            block(3, data),
        ];
        if sequence {
            blocks.push(block(
                4,
                vec![
                    f("Interpolator", Value::Ref(2)),
                    f("Controller", Value::Ref(1)),
                    f("Cycle Type", Value::U32(0)),
                ],
            ));
            blocks.push(block(5, vec![f("Controller Sequence", Value::Ref(4))]));
        }
        Nif {
            version: 0x14020007,
            user_version: 12,
            bs_version: 130,
            exports: Vec::new(),
            block_types: vec![
                "BSEffectShaderProperty".into(),
                "BSEffectShaderPropertyFloatController".into(),
                "NiFloatInterpolator".into(),
                "NiFloatData".into(),
                "NiControllerSequence".into(),
                "NiControllerManager".into(),
            ],
            strings: Vec::new(),
            max_string_length: 0,
            groups: Vec::new(),
            roots: vec![0],
            blocks,
            bytes: &[],
        }
    }

    #[test]
    fn effect_fields_and_first_keys_follow_the_field_order() {
        let n = effect_nif(0.25, &[(0., 0.25), (1., 1.)], false);
        let shader = block_fields(&n, 0).unwrap();
        assert_eq!(effect_field(shader, false, 6), Some(vec![0.25]));
        assert_eq!(effect_field(shader, false, 5), Some(vec![0.5]));
        assert_eq!(effect_field(shader, true, 0), Some(vec![1., 1., 1.]));
        assert_eq!(effect_field(shader, false, 10), None);
        assert_eq!(first_key(block_fields(&n, 3).unwrap(), 1), Some(vec![0.25]));
        // Key times out of order, or a count that does not match: none.
        let bad = effect_nif(0.25, &[(1., 0.25), (0., 1.)], false);
        assert_eq!(first_key(block_fields(&bad, 3).unwrap(), 1), None);
        assert_eq!(first_key(block_fields(&n, 3).unwrap(), 3), None);
        assert_eq!(effect_controlled(&n, 0), vec![(false, 6)]);
    }

    #[test]
    fn an_effect_controller_is_settled_only_where_the_stored_field_is_its_first_key() {
        let b = Budget::new(1 << 20);
        for sequence in [false, true] {
            let n = effect_nif(0.25, &[(0., 0.25), (1., 1.)], sequence);
            let t = tags(&n, &b).unwrap();
            for (i, tag) in t.iter().enumerate().take(4).skip(1) {
                assert_eq!(tag, &vec![EFFECT_FIRST_KEY], "{sequence} {i}");
            }
            if sequence {
                // A looping sequence of settled effect controllers alone.
                assert_eq!(t[4], vec![EFFECT_FIRST_KEY]);
                assert_eq!(t[5], vec![MANAGER_IDLE]);
            }
            // Stored inside the key range but not at the first key.
            let n = effect_nif(0.5, &[(0., 0.25), (1., 1.)], sequence);
            let t = tags(&n, &b).unwrap();
            if sequence {
                assert_eq!(t[1], vec![EFFECT_IDLE]);
                assert_eq!(t[3], vec![EFFECT_IDLE, MANAGER_LOOP]);
                assert_eq!(t[4], vec![MANAGER_LOOP]);
            } else {
                assert_eq!(t[1], vec![TEXTURE_TRANSFORM]);
                assert_eq!(t[3], vec![TEXTURE_TRANSFORM]);
            }
        }
    }

    /// [`effect_nif`] with a lighting shader and its U offset controller
    /// (LightingShaderControlledFloat 20).
    fn lighting_nif(stored: f32, keys: &[(f32, f32)], sequence: bool) -> Nif<'static> {
        let mut n = effect_nif(stored, keys, sequence);
        n.block_types[0] = "BSLightingShaderProperty".into();
        n.block_types[1] = "BSLightingShaderPropertyFloatController".into();
        if let Some(fo4_nif::Data::Fields(fields)) = &mut n.blocks[1].decoded {
            fields[3] = f("Controlled Variable", Value::U32(20));
        }
        n
    }

    #[test]
    fn a_lighting_texture_controller_is_settled_only_where_the_stored_field_is_its_first_key() {
        let b = Budget::new(1 << 20);
        let n = lighting_nif(0.25, &[(0., 0.25), (1., 1.)], false);
        assert_eq!(
            lighting_texture_field(block_fields(&n, 0).unwrap(), 20),
            Some(0.25)
        );
        assert_eq!(
            lighting_texture_field(block_fields(&n, 0).unwrap(), 21),
            Some(1.)
        );
        assert_eq!(texture_driven(&n, 0), [true, false, false, false]);
        let c = texture_controller(&n, 1).unwrap();
        assert!(c.first_key_is_stored && !c.manager_flag && !c.in_sequence);
        let t = tags(&n, &b).unwrap();
        for tag in &t[1..4] {
            assert_eq!(tag, &vec![TEXTURE_FIRST_KEY]);
        }
        // Stored away from the first key: undecided, as before.
        let n = lighting_nif(0.5, &[(0., 0.25), (1., 1.)], false);
        assert!(!texture_controller(&n, 1).unwrap().first_key_is_stored);
        assert_eq!(tags(&n, &b).unwrap()[1], vec![TEXTURE_TRANSFORM]);
        // Run by a looping sequence: the sequence and the chain it drives
        // take the first-key tag; the controller run by the manager keeps
        // the idle-shader tag.
        let n = lighting_nif(0.25, &[(0., 0.25), (1., 1.)], true);
        let t = tags(&n, &b).unwrap();
        assert_eq!(t[1], vec![IDLE_SHADER]);
        assert_eq!(t[3], vec![IDLE_SHADER, TEXTURE_FIRST_KEY]);
        assert_eq!(t[4], vec![TEXTURE_FIRST_KEY]);
        let n = lighting_nif(0.5, &[(0., 0.25), (1., 1.)], true);
        assert_eq!(tags(&n, &b).unwrap()[4], vec![MANAGER_LOOP]);
        let e = fo4_core::Error::new(
            "UnsupportedAnimationRuntime",
            "t.nif",
            0,
            "x",
            format!("NiFloatData;target_in_visual=None;rest={TEXTURE_FIRST_KEY}"),
        );
        assert_eq!(
            covering(&e),
            Some(crate::hypotheses::REST_TEXTURE_FIRST_KEY)
        );
    }

    /// [`effect_nif`] with its controller driving Falloff Stop Angle
    /// (EffectShaderControlledVariable 2), the shader storing `stored` there.
    fn falloff_nif(stored: f32, keys: &[(f32, f32)], sequence: bool) -> Nif<'static> {
        let mut n = effect_nif(0., keys, sequence);
        if let Some(fo4_nif::Data::Fields(fields)) = &mut n.blocks[0].decoded {
            fields.push(fl("Falloff Stop Angle", stored));
        }
        if let Some(fo4_nif::Data::Fields(fields)) = &mut n.blocks[1].decoded {
            fields[3] = f("Controlled Variable", Value::U32(2));
        }
        n
    }

    #[test]
    fn a_falloff_angle_controller_is_settled_where_its_first_key_in_degrees_is_the_stored_cosine() {
        let b = Budget::new(1 << 20);
        let cos70 = degrees_cosine(70.);
        assert!((cos70 - 0.342_020_15).abs() < 1e-6);
        for sequence in [false, true] {
            // Keys 70 then 63 degrees; the field stores cos 70.
            let n = falloff_nif(cos70, &[(0., 70.), (1., 63.)], sequence);
            assert!(falloff_angle_controller(&n, 1));
            let t = tags(&n, &b).unwrap();
            for (i, tag) in t.iter().enumerate().take(4).skip(1) {
                assert_eq!(
                    tag,
                    &vec![EFFECT_FIRST_KEY, FALLOFF_DEGREES],
                    "{sequence} {i}"
                );
            }
            if sequence {
                assert_eq!(t[4], vec![EFFECT_FIRST_KEY, FALLOFF_DEGREES]);
            }
            // The key read as the cosine itself, or a stored value that is
            // the cosine of another key: not settled, as before.
            for (stored, keys) in [
                (0.5, [(0., 0.5), (1., 1.)]),
                (degrees_cosine(63.), [(0., 70.), (1., 63.)]),
            ] {
                let n = falloff_nif(stored, &keys, sequence);
                let t = tags(&n, &b).unwrap();
                assert!(t.iter().all(|x| !x.contains(&FALLOFF_DEGREES)), "{t:?}");
                assert!(t[1].contains(&if sequence { EFFECT_IDLE } else { FALLOFF }));
            }
        }
        // Another controlled variable is never read in degrees.
        let n = effect_nif(0.25, &[(0., 0.25), (1., 1.)], false);
        assert!(!falloff_angle_controller(&n, 1));
        assert!(tags(&n, &b).unwrap()[1] == vec![EFFECT_FIRST_KEY]);
        let e = fo4_core::Error::new(
            "UnsupportedAnimationRuntime",
            "t.nif",
            0,
            "x",
            format!("NiFloatData;target_in_visual=None;rest={FALLOFF_DEGREES}"),
        );
        assert_eq!(covering(&e), Some(crate::hypotheses::REST_FALLOFF_DEGREES));
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
        assert_eq!(
            covering(&e(EFFECT_FIRST_KEY)),
            Some(crate::hypotheses::REST_EFFECT_FIRST_KEY)
        );
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
