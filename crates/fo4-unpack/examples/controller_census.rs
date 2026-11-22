#![forbid(unsafe_code)]
//! Census of the animation-controller blocks that keep models out of the
//! frame (`spec/controller-rest.md`): counts only, written to `OUT.txt`.
//!
//! `controller_census MANIFEST OUT.txt`
//!
//! Every winning NIF of the archives is read and walked as the scene's
//! asset graph walks it (`fo4_scene::assets`: children and every link,
//! weak back pointers of controller blocks and outside blocks excepted). A
//! model is counted here when a reached block of the `state` class is not a
//! particle-system block (the coverage census's family "animation-controller
//! blocks"). For those models: the state block types, whether each is read
//! to its last byte, the controllers' flags, the class of what each drives
//! (by controller type, shader controlled variable and target block), the
//! sequences' controlled blocks, and for every transform interpolator its
//! node's stored transform against the interpolator's pose value, the first
//! key of its data and the key at the start time. Placements are counted
//! through the base records that name each model.
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_plugin::typed::Store;
use fo4_scene::model_policy;
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
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}

fn fields(d: &Data) -> &[Field] {
    model_policy::fields(d)
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
fn strings<'n>(n: &'n Nif<'_>, f: &[Field], name: &str) -> Vec<Option<&'n [u8]>> {
    f.iter()
        .filter(|x| x.name == name)
        .map(|x| match x.value {
            Value::StringIndex(i) => n.strings.get(i as usize).map(Vec::as_slice),
            _ => None,
        })
        .collect()
}
fn particle(ty: &str) -> bool {
    ty.starts_with("NiPSys")
        || ty.starts_with("BSPSys")
        || ty.starts_with("NiPS")
        || ty == "NiParticleSystem"
}

/// What a controller drives, by its type and, for shader controllers, the
/// controlled variable (nifxml enums EffectShaderControlledVariable,
/// EffectShaderControlledColor, LightingShaderControlledFloat and
/// LightingShaderControlledColor).
fn drives(ty: &str, variable: Option<u32>) -> &'static str {
    match ty {
        "NiTransformController" | "NiKeyframeController" | "NiMultiTargetTransformController" => {
            "transform"
        }
        "NiVisController" => "visibility",
        "NiAlphaController" | "BSNiAlphaPropertyTestRefController" => "alpha",
        "NiTextureTransformController" | "NiUVController" => "texture transform",
        "BSEffectShaderPropertyFloatController" => match variable {
            Some(0) => "shader emissive",
            Some(1..=4) => "shader falloff",
            Some(5) => "alpha",
            Some(6..=9) => "texture transform",
            _ => "shader other",
        },
        "BSLightingShaderPropertyFloatController" => match variable {
            Some(11) => "shader emissive",
            Some(0 | 8 | 9 | 10) => "shader lighting terms",
            Some(12) => "alpha",
            Some(20..=23) => "texture transform",
            _ => "shader other",
        },
        "BSEffectShaderPropertyColorController" => "shader emissive colour (effect)",
        "BSLightingShaderPropertyColorController" => match variable {
            Some(0) => "shader lighting terms",
            Some(1) => "shader emissive",
            _ => "shader other",
        },
        "NiLightColorController"
        | "NiLightDimmerController"
        | "NiLightRadiusController"
        | "NiLightIntensityController" => "light",
        "BSMaterialEmittanceMultController" => "shader emissive",
        "BSRefractionStrengthController" | "BSRefractionFirePeriodController" => {
            "shader lighting terms"
        }
        _ => "other",
    }
}

/// A node's stored transform: translation, the nine rotation words, scale.
struct Stored {
    t: [f32; 3],
    r: [f32; 9],
    s: f32,
}
fn stored(n: &Nif<'_>, i: usize) -> Option<Stored> {
    let f = fields(n.blocks.get(i)?.decoded.as_ref()?);
    let (t, r, s) = (
        floats(f, "Translation"),
        floats(f, "Rotation"),
        floats(f, "Scale"),
    );
    Some(Stored {
        t: t.try_into().ok()?,
        r: r.try_into().ok()?,
        s: *s.first()?,
    })
}
/// Rotation matrix of a unit quaternion (w, x, y, z), row-major, acting on
/// column vectors.
fn quat_rows(q: [f32; 4]) -> [f32; 9] {
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
fn transpose(m: [f32; 9]) -> [f32; 9] {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}
fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0., f32::max)
}

/// The keys of an NiTransformData, as read by fo4-nif in field order.
#[derive(Default)]
struct Keys {
    /// (time, quaternion w x y z); `None` for XYZ rotation keys.
    rotation: Option<Vec<(f32, [f32; 4])>>,
    translation: Vec<(f32, [f32; 3])>,
    scale: Vec<(f32, f32)>,
}
fn keys(f: &[Field]) -> Option<Keys> {
    let mut it = f.iter().peekable();
    let mut next = |name: &str| -> Option<&Field> {
        let x = it.next()?;
        (x.name == name).then_some(x)
    };
    let num = |x: &Field| match x.value {
        Value::U32(v) => Some(v),
        _ => None,
    };
    let fl = |x: &Field| match x.value {
        Value::FloatBits(b) => Some(f32::from_bits(b)),
        _ => None,
    };
    let mut out = Keys::default();
    let nrot = num(next("Num Rotation Keys")?)?;
    if nrot > 0 {
        let kind = num(next("Rotation Type")?)?;
        if kind == 4 {
            // Three float key groups (XYZ): not compared.
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
    } else {
        out.rotation = Some(Vec::new());
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
    Some(out)
}

const TOL_T: f32 = 1e-3;
const TOL_R: f32 = 1e-3;
const TOL_S: f32 = 1e-4;
const INVALID: f32 = -3.402_823_4e38;

#[derive(Default)]
struct Worst(BTreeMap<String, f32>);
impl Worst {
    fn see(&mut self, k: &str, v: f32) {
        let e = self.0.entry(k.to_string()).or_insert(0.);
        if v > *e {
            *e = v;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn verdict(
    c: &mut Counts,
    worst: &mut Worst,
    who: &str,
    channel: &str,
    against: &str,
    d: Option<f32>,
    tol: f32,
) {
    match d {
        None => bump(c, format!("{who}: {channel} vs {against}: no value")),
        Some(d) => {
            worst.see(&format!("{who}: {channel} vs {against}"), d);
            bump(
                c,
                format!(
                    "{who}: {channel} vs {against}: {}",
                    if d <= tol { "equal" } else { "differs" }
                ),
            );
        }
    }
}

/// Compares one interpolator's values with its node's stored transform.
fn compare(
    c: &mut Counts,
    worst: &mut Worst,
    node: &Stored,
    interp: &[Field],
    data: Option<&Keys>,
    start: Option<f32>,
    who: &str,
) {
    // Pose value (NiQuatTransform: translation, rotation w x y z, scale).
    let (pt, pq, ps) = (
        floats(interp, "Translation"),
        floats(interp, "Quaternion"),
        floats(interp, "Scale"),
    );
    let pose_t = (pt.len() == 3 && pt[0] != INVALID).then(|| max_diff(&pt, &node.t));
    verdict(c, worst, who, "translation", "pose", pose_t, TOL_T);
    let pose_r = (pq.len() == 4 && pq[0] != INVALID).then(|| {
        let m = quat_rows([pq[0], pq[1], pq[2], pq[3]]);
        let (a, b) = (max_diff(&m, &node.r), max_diff(&transpose(m), &node.r));
        if a <= TOL_R || b <= TOL_R {
            bump(
                c,
                format!(
                    "{who}: rotation vs pose: matches as {}",
                    match (a <= TOL_R, b <= TOL_R) {
                        (true, true) => "both readings (symmetric)",
                        (true, false) => "rows of the quaternion matrix",
                        _ => "its transpose",
                    }
                ),
            );
        }
        a.min(b)
    });
    verdict(c, worst, who, "rotation", "pose", pose_r, TOL_R);
    let pose_s = (ps.len() == 1 && ps[0] != INVALID).then(|| (ps[0] - node.s).abs());
    verdict(c, worst, who, "scale", "pose", pose_s, TOL_S);
    let Some(k) = data else {
        bump(c, format!("{who}: no transform data"));
        return;
    };
    for (label, pick) in [("first key", None), ("key at start time", start)] {
        let at = |t: f32| pick.is_none_or(|s| (s - t).abs() <= 1e-6);
        let first_t = k
            .translation
            .iter()
            .filter(|x| at(x.0))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        verdict(
            c,
            worst,
            who,
            "translation",
            label,
            first_t.map(|x| max_diff(&x.1, &node.t)),
            TOL_T,
        );
        match &k.rotation {
            None => bump(
                c,
                format!("{who}: rotation vs {label}: XYZ keys, not compared"),
            ),
            Some(r) => {
                let first = r
                    .iter()
                    .filter(|x| at(x.0))
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let d = first.map(|x| {
                    let m = quat_rows(x.1);
                    max_diff(&m, &node.r).min(max_diff(&transpose(m), &node.r))
                });
                verdict(c, worst, who, "rotation", label, d, TOL_R);
            }
        }
        let first_s = k
            .scale
            .iter()
            .filter(|x| at(x.0))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        verdict(
            c,
            worst,
            who,
            "scale",
            label,
            first_s.map(|x| (x.1 - node.s).abs()),
            TOL_S,
        );
    }
    // Whether the stored transform equals some key of each channel at all.
    let any_t = k
        .translation
        .iter()
        .any(|x| max_diff(&x.1, &node.t) <= TOL_T);
    if !k.translation.is_empty() {
        bump(
            c,
            format!(
                "{who}: translation equal to {} key",
                if any_t { "some" } else { "no" }
            ),
        );
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("controller_census MANIFEST OUT.txt".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    // Placements by model path: placed REFR -> base -> MODL.
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut base_uses: HashMap<fo4_plugin::FormKey, u64> = HashMap::new();
    let placed: Vec<_> = store
        .placement_cells()
        .filter(|(o, _)| !o.deleted && !o.disabled && o.signature == *b"REFR")
        .map(|(o, _)| store.overrides().form_key(o.key, store.order()))
        .collect();
    for k in &placed {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"NAME") {
            let d = r.data(s);
            if let Ok(d) = <[u8; 4]>::try_from(d) {
                if let Ok(Some(base)) =
                    store
                        .order()
                        .remap(plugin, u32::from_le_bytes(d), &r.source.file, s.offset)
                {
                    *base_uses.entry(base).or_default() += 1;
                }
            }
        }
    }
    let mut model_uses: HashMap<Vec<u8>, (u64, u64)> = HashMap::new();
    for (base, n) in &base_uses {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(base, &b)? else {
            continue;
        };
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"MODL") else {
            continue;
        };
        let d = r.data(s);
        let Ok(path) = fo4_scene::paths::resolve(
            fo4_scene::paths::Citation::RecordModel,
            d.strip_suffix(&[0]).unwrap_or(d),
            &b,
        ) else {
            continue;
        };
        let e = model_uses.entry(path).or_default();
        e.0 += 1;
        e.1 += n;
    }
    drop(store);
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".nif"))
        .collect();
    paths.sort();
    paths.dedup();
    let mut c = Counts::new();
    let mut worst = Worst::default();
    let (mut files, mut failed) = (0u64, 0u64);
    for path in &paths {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read_path_bytes(path, &b) else {
            failed += 1;
            continue;
        };
        let name = String::from_utf8_lossy(path).into_owned();
        let Ok(n) = Nif::read(&bytes, &name, Game::Fallout4, &m.limits, &b) else {
            failed += 1;
            continue;
        };
        files += 1;
        // Reached blocks, as the asset graph walks them.
        let mut seen = vec![false; n.blocks.len()];
        let mut stack: Vec<usize> = n
            .roots
            .iter()
            .filter(|r| **r >= 0)
            .map(|r| *r as usize)
            .collect();
        let mut state: Vec<usize> = Vec::new();
        while let Some(i) = stack.pop() {
            if i >= seen.len() || seen[i] {
                continue;
            }
            seen[i] = true;
            let block = &n.blocks[i];
            let ty = &n.block_types[block.type_index as usize];
            let class = model_policy::block_class(ty);
            if class == fo4_plugin::static_frame::Class::Outside {
                continue;
            }
            let is_state = class == fo4_plugin::static_frame::Class::State;
            if is_state {
                state.push(i);
            }
            let Some(d) = &block.decoded else { continue };
            if let Data::Node { children, .. } = d {
                stack.extend(children.iter().filter(|x| **x >= 0).map(|x| *x as usize));
            }
            for f in fields(d) {
                if let Value::Ref(v) = f.value {
                    if v >= 0 && !(is_state && model_policy::weak_edge(ty, f.name)) {
                        stack.push(v as usize);
                    }
                }
            }
        }
        let ty_of = |i: usize| n.block_types[n.blocks[i].type_index as usize].as_str();
        let controllers: Vec<usize> = state
            .iter()
            .copied()
            .filter(|i| !particle(ty_of(*i)))
            .collect();
        if controllers.is_empty() {
            continue;
        }
        let (bases, uses) = model_uses.get(path).copied().unwrap_or((0, 0));
        bump(&mut c, "models: refused for animation-controller blocks");
        add(
            &mut c,
            "models: placements of those models (placed REFR, enabled)",
            uses,
        );
        add(&mut c, "models: base records naming those models", bases);
        if uses > 0 {
            bump(&mut c, "models: of them named by placed bases");
        }
        if state.iter().any(|i| particle(ty_of(*i))) {
            bump(&mut c, "models: also with particle-system blocks");
        }
        let policy = model_policy::ModelPolicy::inspect(&n, &vfs, &b)?;
        let mut types_here = BTreeSet::new();
        let mut classes_here = BTreeSet::new();
        let mut fully = true;
        // Interpolator users: interpolator -> (node, start time, label).
        let mut users: Vec<(usize, Option<usize>, Option<f32>, &'static str)> = Vec::new();
        let mut node_by_name: HashMap<&[u8], usize> = HashMap::new();
        for (i, bl) in n.blocks.iter().enumerate() {
            if let Some(Data::Node { fields: f, .. }) = &bl.decoded {
                if let Some(Some(nm)) = strings(&n, f, "Name").first() {
                    node_by_name.entry(nm).or_insert(i);
                }
            }
        }
        for &i in &controllers {
            let bl = &n.blocks[i];
            let ty = ty_of(i);
            types_here.insert(ty.to_string());
            bump(&mut c, format!("block: {ty}"));
            let read = bl.decoded.is_some()
                && bl.raw_tail.is_empty()
                && bl.diagnostics.is_empty()
                && bl
                    .decoded
                    .as_ref()
                    .is_some_and(|d| !fields(d).iter().any(|f| matches!(f.value, Value::Raw(_))));
            if !read {
                fully = false;
            }
            bump(
                &mut c,
                format!(
                    "block read to the last byte: {ty}: {}",
                    if read { "yes" } else { "no" }
                ),
            );
            let Some(d) = &bl.decoded else { continue };
            let f = fields(d);
            if let Some(flags) = word(f, "Controller Flags") {
                let variable = word(f, "Controlled Variable");
                let class = drives(ty, variable);
                classes_here.insert(class);
                bump(&mut c, format!("controller drives: {class}"));
                if let Some(v) = variable {
                    bump(&mut c, format!("controlled variable: {ty} {v}"));
                }
                bump(
                    &mut c,
                    format!(
                        "controller flags: {class}: active {}, manager controlled {}, cycle {}",
                        flags & 8 != 0,
                        flags & 0x20 != 0,
                        (flags >> 1) & 3
                    ),
                );
                let target = refs(f, "Target").first().copied().unwrap_or(-1);
                let tt = if target >= 0 && (target as usize) < n.blocks.len() {
                    ty_of(target as usize).to_string()
                } else {
                    "none".into()
                };
                let vis =
                    target >= 0 && policy.visual.get(target as usize).copied().unwrap_or(false);
                bump(
                    &mut c,
                    format!("controller target: {class}: {tt}, in visual closure {vis}"),
                );
                if class == "transform" && ty != "NiMultiTargetTransformController" {
                    let start = floats(f, "Start Time").first().copied();
                    for ip in refs(f, "Interpolator").into_iter().filter(|x| *x >= 0) {
                        users.push((
                            ip as usize,
                            (target >= 0).then_some(target as usize),
                            start,
                            "controller",
                        ));
                    }
                }
            }
            if ty == "NiControllerSequence" {
                let ctypes = strings(&n, f, "Controller Type");
                let nodes = strings(&n, f, "Node Name");
                let ips = refs(f, "Interpolator");
                let start = floats(f, "Start Time").first().copied();
                let cycle = word(f, "Cycle Type");
                bump(&mut c, format!("sequence cycle type: {cycle:?}"));
                for (k, ct) in ctypes.iter().enumerate() {
                    let ct = ct.map(|s| String::from_utf8_lossy(s).into_owned());
                    let class = drives(ct.as_deref().unwrap_or(""), None);
                    bump(
                        &mut c,
                        format!(
                            "sequence controlled block: {}",
                            ct.as_deref().unwrap_or("no type")
                        ),
                    );
                    classes_here.insert(if ct.is_some() { class } else { "other" });
                    if class == "transform" {
                        let node = nodes
                            .get(k)
                            .copied()
                            .flatten()
                            .and_then(|nm| node_by_name.get(nm).copied());
                        if let Some(ip) = ips.get(k).copied().filter(|x| *x >= 0) {
                            users.push((ip as usize, node, start, "sequence"));
                        }
                    }
                }
            }
        }
        bump(
            &mut c,
            format!(
                "models: every controller block read to the last byte: {}",
                if fully { "yes" } else { "no" }
            ),
        );
        let set: Vec<&str> = classes_here.iter().copied().collect();
        bump(
            &mut c,
            format!("models by set of driven classes: {}", set.join(" + ")),
        );
        add(
            &mut c,
            format!("placements by set of driven classes: {}", set.join(" + ")),
            uses,
        );
        let _ = types_here;
        // Rest tags (`fo4_scene::rest`) of the reached state blocks.
        let tags = fo4_scene::rest::tags(&n, &b)?;
        let mut set: BTreeSet<&str> = BTreeSet::new();
        for &i in &state {
            set.extend(tags[i].iter().copied());
        }
        let decided = [
            fo4_scene::rest::MANAGER_IDLE,
            fo4_scene::rest::IDLE_TRANSFORM,
            fo4_scene::rest::IDLE_SHADER,
            fo4_scene::rest::TRANSFORM_CONSTANT,
            fo4_scene::rest::UNRENDERED,
        ];
        let open: Vec<&str> = set
            .iter()
            .copied()
            .filter(|t| !decided.contains(t))
            .collect();
        let all: Vec<&str> = set.iter().copied().collect();
        bump(
            &mut c,
            format!("rest: models by tag set: {}", all.join(" + ")),
        );
        add(
            &mut c,
            format!("rest: placements by tag set: {}", all.join(" + ")),
            uses,
        );
        for t in &all {
            bump(&mut c, format!("rest: models with tag {t}"));
            add(&mut c, format!("rest: placements with tag {t}"), uses);
        }
        let verdict = if open.is_empty() {
            "decided by the rest rules".to_string()
        } else {
            format!("not decided: {}", open.join(" + "))
        };
        bump(&mut c, format!("rest: models {verdict}"));
        add(&mut c, format!("rest: placements {verdict}"), uses);
        if open.is_empty() {
            // Sequences of the decided models by cycle type (a looping
            // sequence that the engine starts would move them at rest).
            let mut cycles = BTreeSet::new();
            for &i in &state {
                if ty_of(i) == "NiControllerSequence" {
                    let cyc = n.blocks[i]
                        .decoded
                        .as_ref()
                        .and_then(|d| word(fields(d), "Cycle Type"));
                    bump(
                        &mut c,
                        format!("rest: decided models' sequences, cycle type {cyc:?}"),
                    );
                    cycles.insert(format!("{cyc:?}"));
                }
            }
            let cycles: Vec<String> = cycles.into_iter().collect();
            bump(
                &mut c,
                format!(
                    "rest: decided models by sequence cycle types [{}]",
                    cycles.join(", ")
                ),
            );
            add(
                &mut c,
                format!(
                    "rest: decided placements by sequence cycle types [{}]",
                    cycles.join(", ")
                ),
                uses,
            );
            for t in &all {
                add(
                    &mut c,
                    format!("rest: placements decided, relying on {t}"),
                    uses,
                );
            }
        }
        // Transform interpolators against their nodes.
        let mut model_agrees = true;
        let mut compared = false;
        for (ip, node, start, who) in users {
            let Some(bl) = n.blocks.get(ip) else { continue };
            if ty_of(ip) != "NiTransformInterpolator" {
                bump(
                    &mut c,
                    format!("{who}: transform interpolator type {}", ty_of(ip)),
                );
                model_agrees = false;
                continue;
            }
            let Some(d) = &bl.decoded else { continue };
            let Some(node) = node.and_then(|x| stored(&n, x)) else {
                bump(&mut c, format!("{who}: interpolator without a node"));
                model_agrees = false;
                continue;
            };
            let f = fields(d);
            let data = refs(f, "Data")
                .first()
                .copied()
                .filter(|x| *x >= 0)
                .and_then(|x| n.blocks.get(x as usize))
                .and_then(|x| x.decoded.as_ref())
                .and_then(|x| keys(fields(x)));
            let before = c.clone();
            compare(&mut c, &mut worst, &node, f, data.as_ref(), start, who);
            compared = true;
            // The model's stored pose agrees with every interpolator's start
            // key (or pose value without data) on every channel with a value.
            let differs = c
                .iter()
                .any(|(k, v)| k.contains("start time: differs") && before.get(k) != Some(v))
                || (data.is_none()
                    && c.iter()
                        .any(|(k, v)| k.contains("vs pose: differs") && before.get(k) != Some(v)));
            if differs {
                model_agrees = false;
            }
        }
        if compared {
            bump(
                &mut c,
                format!(
                    "models with transform interpolators: stored pose equals every start key {}",
                    model_agrees
                ),
            );
            add(
                &mut c,
                format!(
                    "placements of models with transform interpolators: stored pose equals every start key {}",
                    model_agrees
                ),
                uses,
            );
        }
    }
    let mut out = String::new();
    writeln!(
        out,
        "# controller_census: models refused for animation-controller blocks (spec/controller-rest.md)"
    )?;
    writeln!(
        out,
        "# Counts only. Tolerances: translation {TOL_T}, rotation word {TOL_R}, scale {TOL_S}."
    )?;
    writeln!(
        out,
        "winning NIF read {files}, unreadable {failed} ({:.0} s)",
        started.elapsed().as_secs_f64()
    )?;
    for (k, v) in &c {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\n## worst differences (largest absolute word difference)"
    )?;
    for (k, v) in &worst.0 {
        writeln!(out, "{v:.6}\t{k}")?;
    }
    std::fs::write(&args[1], out)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
