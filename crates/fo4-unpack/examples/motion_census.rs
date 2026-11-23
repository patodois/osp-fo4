#![forbid(unsafe_code)]
//! Census of moving geometry and of the sequences a controller manager runs
//! (`spec/controller-clock.md`, Moving geometry and Sequences of the
//! manager-run effect models): counts only, written to `OUT.txt`; with
//! `NAMES.tsv`, a local listing with model paths (never committed).
//!
//! `motion_census MANIFEST OUT.txt [NAMES.tsv]`
//!
//! Every winning NIF of the archives is read.
//!
//! Section 1, free-running transform controllers (`NiTransformController`,
//! `NiKeyframeController` not run by a manager and named by no sequence):
//! why the clock does not run one (`fo4_scene::clock::transform_track`),
//! and for a track its interpolator data, the key type of each channel,
//! which channels vary against the node's stored transform, what a channel
//! without keys holds, the cycle type and seconds per cycle, the stored
//! words against the clock's at load, every reading of XYZ angle keys
//! (axis order, sign, radians or degrees) against the stored rotation at
//! load, quaternion keys' hemispheres and steps, collision and drawn
//! geometry under the node, whether the node is a root, and whether the
//! clock's switches run it. By model: the rest tags with the transform
//! switches off and on.
//!
//! Section 2, the models whose effect-shader controllers a manager runs
//! (rest tag `effect-idle`): every sequence of their manager by what its
//! controlled blocks drive, cycle type, length, frequency, text keys and
//! whether its keys at its start or end give the stored fields, and by
//! model which sequences each data-only signal picks (the manager's first
//! sequence, the only looping sequence, the sequences whose start keys give
//! every stored field) and whether those signals agree; the subrecords of
//! the placed references and of the base records. Behaviour graph files
//! are not read here (only whether the NIF names one).
//!
//! Placements are the enabled placed references (REFR) of the base records
//! that name each model; their area is the largest face of the base's OBND
//! box times the reference scale squared, as in `coverage_census`.
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_plugin::typed::Store;
use fo4_scene::{clock, model_policy, rest};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;

#[derive(Default)]
struct Tally {
    n: u64,
    placements: u64,
    area: f64,
}
#[derive(Default)]
struct Census {
    lines: BTreeMap<String, Tally>,
    total_area: f64,
    /// The lines the current model has counted its placements in.
    seen: BTreeSet<String>,
}
impl Census {
    /// Counts `k` once, with the placements and area of the current model
    /// the first time the model reaches the line (a model's placements
    /// count once per line however many of its blocks the line counts).
    fn add(&mut self, k: impl Into<String>, u: &Use) {
        let k = k.into();
        let first = self.seen.insert(k.clone());
        let t = self.lines.entry(k).or_default();
        t.n += 1;
        if first {
            t.placements += u.placements;
            t.area += u.area;
        }
    }
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
fn refs(f: &[Field], name: &str) -> Vec<usize> {
    f.iter()
        .filter(|x| x.name == name)
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
            Value::U8(v) => Some(u32::from(v)),
            _ => None,
        })
}
fn float(f: &[Field], name: &str) -> Option<f32> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match x.value {
            Value::FloatBits(b) => Some(f32::from_bits(b)),
            _ => None,
        })
}
fn text(n: &Nif<'_>, f: &[Field], name: &str) -> Option<String> {
    f.iter()
        .find(|x| x.name == name)
        .and_then(|x| match x.value {
            Value::StringIndex(i) => n
                .strings
                .get(i as usize)
                .map(|s| String::from_utf8_lossy(s).to_ascii_lowercase()),
            _ => None,
        })
}
fn cycle(c: u32) -> &'static str {
    match c {
        clock::CYCLE_LOOP => "loop",
        clock::CYCLE_REVERSE => "reverse",
        clock::CYCLE_CLAMP => "clamp",
        _ => "other",
    }
}
fn kind(k: u32) -> &'static str {
    match k {
        clock::LINEAR => "linear",
        clock::QUADRATIC => "quadratic",
        clock::TBC => "TBC",
        clock::XYZ => "XYZ",
        clock::CONSTANT => "step",
        _ => "other",
    }
}
fn seconds(s: f64) -> &'static str {
    match s {
        s if s < 1. => "below 1 s",
        s if s < 5. => "1 to 5 s",
        s if s < 30. => "5 to 30 s",
        s if s < 300. => "30 to 300 s",
        _ => "300 s or more",
    }
}

/// Whether node `i` or a block under it has a collision object.
fn collision_under(n: &Nif<'_>, i: usize) -> bool {
    under(n, i, |d| !refs(fields(d), "Collision").is_empty())
}
/// Whether node `i` or a block under it is drawn geometry (mesh data of
/// its own or through its Data link).
fn geometry_under(n: &Nif<'_>, i: usize) -> bool {
    under(n, i, |d| match d {
        Data::Mesh(_) => true,
        _ => refs(fields(d), "Data").iter().any(|k| {
            matches!(
                n.blocks.get(*k).and_then(|b| b.decoded.as_ref()),
                Some(Data::Mesh(_))
            )
        }),
    })
}
fn under(n: &Nif<'_>, i: usize, hit: impl Fn(&Data) -> bool) -> bool {
    let mut stack = vec![i];
    let mut seen = BTreeSet::new();
    while let Some(i) = stack.pop() {
        if !seen.insert(i) || seen.len() > 4096 {
            continue;
        }
        let Some(d) = n.blocks.get(i).and_then(|b| b.decoded.as_ref()) else {
            continue;
        };
        if hit(d) {
            return true;
        }
        if let Data::Node { children, .. } = d {
            stack.extend(children.iter().filter(|x| **x >= 0).map(|x| *x as usize));
        }
    }
    false
}

fn close(a: &[f64], b: &[f32], tol: f32) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - f64::from(*y)).abs() <= f64::from(tol))
}
const PERMUTATIONS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];
fn order_label(o: [usize; 3]) -> String {
    o.iter()
        .map(|k| ["X", "Y", "Z"][*k])
        .collect::<Vec<_>>()
        .join(" then ")
}

#[derive(Default, Clone)]
struct Use {
    placements: u64,
    area: f64,
    bases: BTreeMap<String, u64>,
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("motion_census MANIFEST OUT.txt [NAMES.tsv]".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let mut c = Census::default();
    // Placements by model path, with area; the subrecord signatures of the
    // placed references by base, for section 2.
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut base_refs: HashMap<fo4_plugin::FormKey, Vec<(f64, BTreeSet<String>)>> = HashMap::new();
    let placed: Vec<_> = store
        .placement_cells()
        .filter(|(o, _)| !o.deleted && !o.disabled && o.signature == *b"REFR")
        .map(|(o, cell)| (store.overrides().form_key(o.key, store.order()), cell))
        .collect();
    // For the local listing: a few cells that place each base.
    let mut base_cells: HashMap<fo4_plugin::FormKey, Vec<fo4_plugin::FormKey>> = HashMap::new();
    for (k, cell) in &placed {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        let mut scale = 1f64;
        let mut base = None;
        let mut sigs = BTreeSet::new();
        for s in &r.subrecords {
            let d = r.data(s);
            sigs.insert(String::from_utf8_lossy(&s.signature).into_owned());
            if s.signature == *b"XSCL" && d.len() == 4 {
                scale = f64::from(f32::from_le_bytes([d[0], d[1], d[2], d[3]]));
            }
            if s.signature == *b"NAME" {
                if let Ok(d) = <[u8; 4]>::try_from(d) {
                    base = store.order().remap(
                        plugin,
                        u32::from_le_bytes(d),
                        &r.source.file,
                        s.offset,
                    )?;
                }
            }
        }
        if let Some(base) = base {
            let cells = base_cells.entry(base.clone()).or_default();
            if cells.len() < 3 && !cells.contains(cell) {
                cells.push(cell.clone());
            }
            base_refs.entry(base).or_default().push((scale, sigs));
        }
    }
    let mut uses: HashMap<Vec<u8>, Use> = HashMap::new();
    let mut model_ref_sigs: HashMap<Vec<u8>, BTreeMap<String, u64>> = HashMap::new();
    let mut model_base_sigs: HashMap<Vec<u8>, BTreeMap<String, u64>> = HashMap::new();
    let mut model_cells: HashMap<Vec<u8>, Vec<fo4_plugin::FormKey>> = HashMap::new();
    for (base, refs_of) in &base_refs {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(base, &b)? else {
            continue;
        };
        let signature = String::from_utf8_lossy(&r.signature).into_owned();
        let mut bounds = None;
        let mut model = None;
        let mut sigs = BTreeSet::new();
        for s in &r.subrecords {
            let d = r.data(s);
            sigs.insert(String::from_utf8_lossy(&s.signature).into_owned());
            if s.signature == *b"OBND" && d.len() == 12 {
                let v: [i16; 6] =
                    std::array::from_fn(|k| i16::from_le_bytes([d[2 * k], d[2 * k + 1]]));
                bounds = Some(v);
            }
            if s.signature == *b"MODL" && model.is_none() {
                if let Ok(p) = fo4_scene::paths::resolve(
                    fo4_scene::paths::Citation::RecordModel,
                    d.strip_suffix(&[0]).unwrap_or(d),
                    &b,
                ) {
                    model = Some(p);
                }
            }
        }
        let face = bounds.map_or(0., |v| {
            let d = [0, 1, 2].map(|k| (f64::from(v[k + 3]) - f64::from(v[k])).abs());
            (d[0] * d[1]).max(d[0] * d[2]).max(d[1] * d[2])
        });
        let area: f64 = refs_of.iter().map(|(s, _)| face * s * s).sum();
        c.total_area += area;
        let Some(path) = model else {
            continue;
        };
        let mc = model_cells.entry(path.clone()).or_default();
        for c in base_cells.get(base).into_iter().flatten() {
            if mc.len() < 3 && !mc.contains(c) {
                mc.push(c.clone());
            }
        }
        let u = uses.entry(path.clone()).or_default();
        u.placements += refs_of.len() as u64;
        u.area += area;
        *u.bases.entry(signature.clone()).or_default() += refs_of.len() as u64;
        let rs = model_ref_sigs.entry(path.clone()).or_default();
        for (_, s) in refs_of {
            for sig in s {
                *rs.entry(sig.clone()).or_default() += 1;
            }
        }
        let bs = model_base_sigs.entry(path).or_default();
        for sig in sigs {
            *bs.entry(format!("{signature}.{sig}")).or_default() += refs_of.len() as u64;
        }
    }
    // The cells' editor ids, for the local listing only.
    let mut cell_names: HashMap<Vec<u8>, String> = HashMap::new();
    if args.len() == 3 {
        for (path, cells) in &model_cells {
            let mut names = Vec::new();
            for c in cells {
                let b = Budget::new(m.limits.max_memory);
                if let Some(r) = store.raw(c, &b)? {
                    if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"EDID") {
                        let d = r.data(s);
                        names.push(
                            String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned(),
                        );
                    }
                }
            }
            cell_names.insert(path.clone(), names.join(","));
        }
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
    let on = clock::Switches {
        translation: true,
        rotation: true,
        rotation_xyz: true,
        scale: true,
        ..clock::Switches::OFF
    };
    let mut names = String::new();
    let (mut files, mut failed) = (0u64, 0u64);
    let nothing = Use::default();
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
        c.seen.clear();
        let u = uses.get(path).unwrap_or(&nothing);
        let transforms: Vec<usize> = (0..n.blocks.len())
            .filter(|i| {
                matches!(
                    type_of(&n, *i),
                    "NiTransformController" | "NiKeyframeController"
                )
            })
            .collect();
        let off = match rest::tags(&n, &b) {
            Ok(t) => t,
            Err(_) => {
                c.add("0 models: rest tags not read", u);
                continue;
            }
        };
        // As effect_clock_census: an effect-shader controller of an effect
        // shader whose own rest tag is effect-idle.
        let effect_idle_model = (0..n.blocks.len()).any(|i| {
            matches!(
                type_of(&n, i),
                "BSEffectShaderPropertyFloatController" | "BSEffectShaderPropertyColorController"
            ) && block_fields(&n, i)
                .and_then(|f| refs(f, "Target").first().copied())
                .is_some_and(|t| type_of(&n, t) == "BSEffectShaderProperty")
                && off.get(i).is_some_and(|t| t.contains(&rest::EFFECT_IDLE))
        });
        if !transforms.is_empty() {
            section1(&mut c, &n, &transforms, &off, on, u, &name, &mut names, &b)?;
            if let Some(cells) = cell_names.get(path) {
                let _ = writeln!(names, "1 cells\t{name}\t{cells}");
            }
        }
        if effect_idle_model {
            section2(
                &mut c,
                &n,
                &off,
                u,
                model_ref_sigs.get(path),
                model_base_sigs.get(path),
                &name,
                &mut names,
            );
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# motion_census: free-running transform controllers and manager-run sequences (spec/controller-clock.md)"
    );
    let _ = writeln!(
        out,
        "# Counts only. Each line: count, placements of the models (enabled REFR), area proxy as % of all placements' area, label."
    );
    let _ = writeln!(
        out,
        "winning NIF read {files}, unreadable {failed}; placements {} (area proxy {:.4e}) ({} s)",
        placed.len(),
        c.total_area,
        started.elapsed().as_secs()
    );
    for (k, t) in &c.lines {
        let _ = writeln!(
            out,
            "{}\t{}\t{:.3}%\t{k}",
            t.n,
            t.placements,
            if c.total_area > 0. {
                100. * t.area / c.total_area
            } else {
                0.
            }
        );
    }
    std::fs::write(&args[1], out)?;
    if let Some(p) = args.get(2) {
        std::fs::write(p, names)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn section1(
    c: &mut Census,
    n: &Nif<'_>,
    transforms: &[usize],
    off: &[Vec<&'static str>],
    on: clock::Switches,
    u: &Use,
    name: &str,
    names: &mut String,
    b: &Budget,
) -> Result<(), E> {
    let has_manager = (0..n.blocks.len()).any(|i| type_of(n, i) == "NiControllerManager");
    let roots: BTreeSet<usize> = n
        .roots
        .iter()
        .filter(|r| **r >= 0)
        .map(|r| *r as usize)
        .collect();
    let mut moving_model = false;
    let mut runs_model = false;
    let mut collision_model = false;
    let mut geometry_model = false;
    for &i in transforms {
        let ty = type_of(n, i);
        let f = block_fields(n, i).unwrap_or(&[]);
        let timing = clock::Timing::read(f);
        let driver = if clock::in_sequence(n, i) {
            "named by a sequence"
        } else if timing.is_some_and(|t| t.manager()) && has_manager {
            "manager flag"
        } else {
            "free-running"
        };
        c.add(format!("1 controllers: {ty}; {driver}"), u);
        if driver != "free-running" {
            continue;
        }
        let tag_off = off.get(i).map(|t| t.join("+")).unwrap_or_default();
        c.add(
            format!("1 free: rest tag with the switches off {tag_off}"),
            u,
        );
        let target = refs(f, "Target").first().copied();
        let collision = target.is_some_and(|t| collision_under(n, t));
        let geometry = target.is_some_and(|t| geometry_under(n, t));
        let root = target.is_some_and(|t| roots.contains(&t));
        let ip = refs(f, "Interpolator");
        let ip_type = match ip.as_slice() {
            [] => "none".to_string(),
            [k] => type_of(n, *k).to_string(),
            _ => "several".into(),
        };
        c.add(format!("1 free: interpolator {ip_type}"), u);
        let t = match clock::transform_track(n, i, b)? {
            Err(why) => {
                c.add(format!("1 free: not a clock track: {why}"), u);
                continue;
            }
            Ok(t) => t,
        };
        let v = t.varies();
        let any = v.iter().any(|x| *x);
        let has = |k: &clock::Keys| !k.keys.is_empty();
        let chan = |keys: Option<&clock::Keys>, pose: bool, varies: bool| -> String {
            let form = match keys.filter(|k| has(k)) {
                Some(k) => kind(k.kind).to_string(),
                None if pose => "pose".into(),
                None => "none".into(),
            };
            format!("{form}{}", if varies { " (varies)" } else { "" })
        };
        let keys = t.keys.as_ref();
        let rot = match keys.and_then(|k| k.rotation.as_ref()) {
            Some(clock::Rotation::Quaternion { kind: k, .. }) => format!("quaternion {}", kind(*k)),
            Some(clock::Rotation::Xyz(axes)) => format!(
                "XYZ [{}]",
                axes.iter()
                    .map(|a| if a.keys.is_empty() {
                        "empty"
                    } else {
                        kind(a.kind)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None if t.pose_rotation.is_some() => "pose".into(),
            None => "none".into(),
        };
        let line = format!(
            "1 track: data {}; translation {}, rotation {}{}, scale {}",
            if keys.is_some() { "keys" } else { "none" },
            chan(
                keys.map(|k| &k.translation),
                t.pose_translation.is_some(),
                v[clock::TRANSLATION]
            ),
            rot,
            if v[clock::ROTATION] { " (varies)" } else { "" },
            chan(
                keys.map(|k| &k.scale),
                t.pose_scale.is_some(),
                v[clock::SCALE]
            ),
        );
        c.add(line, u);
        let cyc = cycle(t.timing.cycle());
        let span = f64::from(t.timing.stop - t.timing.start) / f64::from(t.timing.frequency);
        c.add(
            format!(
                "1 track: varies {any}; cycle {cyc}; seconds per cycle {}; frequency 1 {}, phase 0 {}",
                seconds(span),
                t.timing.frequency == 1.,
                t.timing.phase == 0.
            ),
            u,
        );
        c.add(
            format!(
                "1 track: varies {any}; collision under the node {collision}; drawn geometry under the node {geometry}; the node is a root {root}"
            ),
            u,
        );
        if !any {
            continue;
        }
        moving_model = true;
        collision_model |= collision;
        geometry_model |= geometry;
        // A channel without keys: its pose against the stored words.
        let [tt, tr, ts] = rest::TOLERANCE;
        if keys.is_none_or(|k| !has(&k.translation)) {
            let p = match t.pose_translation {
                None => "unset",
                Some(p) if close(&p.map(f64::from), &t.stored_translation, tt) => "equals stored",
                Some(_) => "differs",
            };
            c.add(
                format!("1 varying track: translation without keys, pose {p}"),
                u,
            );
        }
        if keys.is_none_or(|k| !has(&k.scale)) {
            let p = match t.pose_scale {
                None => "unset",
                Some(p) if close(&[f64::from(p)], &[t.stored_scale], ts) => "equals stored",
                Some(_) => "differs",
            };
            c.add(format!("1 varying track: scale without keys, pose {p}"), u);
        }
        if keys.is_none_or(|k| k.rotation.is_none()) {
            let p = match t.pose_rotation {
                None => "unset",
                Some(q)
                    if close(
                        &clock::quaternion_rows(q.map(f64::from)),
                        &t.stored_rotation,
                        tr,
                    ) =>
                {
                    "equals stored"
                }
                Some(_) => "differs",
            };
            c.add(
                format!("1 varying track: rotation without keys, pose {p}"),
                u,
            );
        }
        // The words at load (the controller's time at second 0) against
        // the stored ones, channel by channel.
        let load = t.timing.time(0.);
        if let (Some(time), Some(k)) = (load, keys) {
            if has(&k.translation) {
                let same = k
                    .translation
                    .sample(time)
                    .is_some_and(|x| close(&x.map(f64::from), &t.stored_translation, tt));
                c.add(
                    format!("1 varying track: translation at load equals stored {same}"),
                    u,
                );
            }
            if has(&k.scale) {
                let same = k
                    .scale
                    .sample(time)
                    .is_some_and(|x| close(&[f64::from(x[0])], &[t.stored_scale], ts));
                c.add(
                    format!("1 varying track: scale at load equals stored {same}"),
                    u,
                );
            }
            match &k.rotation {
                Some(clock::Rotation::Quaternion { kind: qk, keys: q }) => {
                    let same = clock::quaternion_at(*qk, q, time)
                        .is_some_and(|x| close(&clock::quaternion_rows(x), &t.stored_rotation, tr));
                    let first = q.first().is_some_and(|x| {
                        close(
                            &clock::quaternion_rows(x.1.map(f64::from)),
                            &t.stored_rotation,
                            tr,
                        )
                    });
                    c.add(
                        format!("1 varying track: quaternion at load equals stored {same}; first key equals stored {first}"),
                        u,
                    );
                    let mut hemisphere = true;
                    let mut largest = 0f64;
                    for w in q.windows(2) {
                        let d: f64 = (0..4)
                            .map(|j| f64::from(w[0].1[j]) * f64::from(w[1].1[j]))
                            .sum();
                        let na: f64 = w[0]
                            .1
                            .iter()
                            .map(|x| f64::from(*x).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        let nb: f64 = w[1]
                            .1
                            .iter()
                            .map(|x| f64::from(*x).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        let d = d / (na * nb);
                        hemisphere &= d >= 0.;
                        largest = largest.max(2. * d.abs().min(1.).acos().to_degrees());
                    }
                    let step = match largest {
                        x if x < 1. => "below 1 degree",
                        x if x < 10. => "1 to 10 degrees",
                        x if x < 45. => "10 to 45 degrees",
                        x if x < 90. => "45 to 90 degrees",
                        _ => "90 degrees or more",
                    };
                    let count = match q.len() {
                        0..=2 => q.len().to_string(),
                        3..=10 => "3 to 10".into(),
                        11..=100 => "11 to 100".into(),
                        _ => "more than 100".into(),
                    };
                    c.add(
                        format!("1 varying track: quaternion keys {count}; consecutive keys in one hemisphere {hemisphere}; largest step {step}"),
                        u,
                    );
                }
                Some(clock::Rotation::Xyz(axes)) => {
                    let lens: Vec<String> = axes
                        .iter()
                        .map(|a| a.keys.len().min(3).to_string())
                        .collect();
                    c.add(
                        format!(
                            "1 varying track: XYZ key counts (3 = 3 or more) [{}]",
                            lens.join(", ")
                        ),
                        u,
                    );
                    if axes.iter().any(|a| a.keys.is_empty()) {
                        c.add("1 varying track: XYZ with an empty angle group", u);
                    } else {
                        let mut range = 0f64;
                        for a in axes.iter() {
                            if let Some((lo, hi)) = a.bounds(0) {
                                range = range.max(f64::from(lo).abs()).max(f64::from(hi).abs());
                            }
                        }
                        let r = match range {
                            x if x <= 0.0 => "0",
                            x if x <= std::f64::consts::PI => "within pi",
                            x if x <= 2. * std::f64::consts::PI => "pi to 2 pi",
                            x if x <= 360. => "2 pi to 360",
                            _ => "above 360",
                        };
                        c.add(
                            format!("1 varying track: XYZ largest absolute angle {r}"),
                            u,
                        );
                        let angles: Option<Vec<f64>> = axes
                            .iter()
                            .map(|a| a.sample(time).map(|x| f64::from(x[0])))
                            .collect();
                        if let Some(angles) = angles {
                            // An angle that is a whole number of turns
                            // turns nothing in either unit's reading.
                            let turning = |a: &f64| {
                                let r = a.rem_euclid(std::f64::consts::TAU);
                                r.min(std::f64::consts::TAU - r) >= 1e-6
                            };
                            let trivial = !angles.iter().any(turning);
                            let all_one = angles.iter().filter(|a| turning(a)).count() <= 1;
                            let mut hits = Vec::new();
                            for unit in ["radians", "degrees"] {
                                for sign in [1f64, -1.] {
                                    for o in PERMUTATIONS {
                                        let a: [f64; 3] = std::array::from_fn(|k| {
                                            let x = angles[k] * sign;
                                            if unit == "degrees" {
                                                x.to_radians()
                                            } else {
                                                x
                                            }
                                        });
                                        if close(&clock::xyz_rows(a, o), &t.stored_rotation, tr) {
                                            hits.push(format!(
                                                "{unit} {} {}",
                                                if sign > 0. { "+" } else { "-" },
                                                order_label(o)
                                            ));
                                        }
                                    }
                                }
                            }
                            let class = if trivial {
                                "all angles 0".to_string()
                            } else if all_one {
                                "one angle not 0".to_string()
                            } else {
                                "two or three angles not 0".to_string()
                            };
                            c.add(format!("1 varying track: XYZ at load against the stored rotation ({class}): readings that match {}", hits.len()), u);
                            if !trivial && !all_one {
                                for h in &hits {
                                    c.add(
                                        format!(
                                            "1 varying track: XYZ at load ({class}) matches {h}"
                                        ),
                                        u,
                                    );
                                }
                                if hits.len() <= 4 {
                                    c.add(format!("1 varying track: XYZ at load ({class}) matches exactly [{}]", hits.join("; ")), u);
                                }
                            }
                        }
                    }
                }
                None => {}
            }
        }
        let unread = t.unread();
        let runs = t.runs_with(on);
        runs_model |= runs;
        c.add(
            format!(
                "1 varying track: the clock reads it {}; runs with the transform switches on {runs}",
                unread.map_or("yes".to_string(), |w| format!("no ({w})"))
            ),
            u,
        );
        c.add(
            format!(
                "1 varying track: rules [{}]",
                if runs {
                    t.rules().join(", ")
                } else {
                    "-".into()
                }
            ),
            u,
        );
        if runs {
            let _ = writeln!(
                names,
                "1 runs\t{name}\t{}\t{}",
                u.placements,
                t.rules().join(",")
            );
        }
    }
    if !moving_model {
        return Ok(());
    }
    // By model: open rest tags with the transform switches off and on.
    c.add(
        format!("2m models with a varying free-running transform track: a moving node has drawn geometry under it {geometry_model}"),
        u,
    );
    let with = rest::tags_with(n, on, b)?;
    let open = |tags: &[Vec<&'static str>]| -> BTreeSet<&'static str> {
        tags.iter()
            .flatten()
            .copied()
            .filter(|t| rest::rule(t).is_none())
            .collect()
    };
    let (o_off, o_on) = (open(off), open(&with));
    let join = |s: &BTreeSet<&str>| {
        if s.is_empty() {
            "none".to_string()
        } else {
            s.iter().copied().collect::<Vec<_>>().join(" + ")
        }
    };
    c.add(
        format!(
            "2m models with a varying free-running transform track: collision under a moving node {collision_model}; a track runs with the switches {runs_model}"
        ),
        u,
    );
    c.add(
        format!(
            "2m models with a varying free-running transform track: open tags off [{}] -> on [{}]",
            join(&o_off),
            join(&o_on)
        ),
        u,
    );
    c.add(
        format!(
            "2m models with a varying free-running transform track: decided off {}, on {}",
            o_off.is_empty(),
            o_on.is_empty()
        ),
        u,
    );
    for (sig, k) in &u.bases {
        let t = c
            .lines
            .entry(format!("2m placements by base record type: {sig}"))
            .or_default();
        t.n += 1;
        t.placements += k;
    }
    let _ = writeln!(
        names,
        "1 model\t{name}\t{}\t{} -> {}",
        u.placements,
        join(&o_off),
        join(&o_on)
    );
    Ok(())
}

/// What a sequence's controlled block drives, by controller type (and
/// controlled variable where the controller is linked).
fn drives(n: &Nif<'_>, ctl: Option<usize>, type_name: Option<&str>) -> String {
    let (ty, variable) = match ctl {
        Some(k) => (
            type_of(n, k).to_string(),
            block_fields(n, k).and_then(|f| word(f, "Controlled Variable")),
        ),
        None => (type_name.unwrap_or("no type").to_string(), None),
    };
    match (ty.as_str(), variable) {
        ("BSEffectShaderPropertyFloatController", Some(0)) => "effect emissive multiple".into(),
        ("BSEffectShaderPropertyFloatController", Some(1..=4)) => "effect falloff".into(),
        ("BSEffectShaderPropertyFloatController", Some(5)) => "effect alpha".into(),
        ("BSEffectShaderPropertyFloatController", Some(6..=9)) => "effect UV".into(),
        ("BSEffectShaderPropertyColorController", _) => "effect emissive colour".into(),
        (
            "NiTransformController" | "NiKeyframeController" | "NiMultiTargetTransformController",
            _,
        ) => "transform".into(),
        ("NiVisController", _) => "visibility".into(),
        (t, _) if t.starts_with("NiPS") || t.starts_with("BSPSys") => "particles".into(),
        (t, _) if t.starts_with("NiLight") => "light".into(),
        (
            "BSLightingShaderPropertyFloatController" | "BSLightingShaderPropertyColorController",
            _,
        ) => "lighting shader".into(),
        (t, _) => t.to_string(),
    }
}

/// Whether the interpolator `ip` of a float or point controlled block
/// gives `stored` at its keys' first (`end` false) or last time.
fn keys_give(n: &Nif<'_>, ip: usize, stored: &[f32], end: bool, b: &Budget) -> Option<bool> {
    let f = block_fields(n, ip)?;
    let components = match type_of(n, ip) {
        "NiFloatInterpolator" => 1,
        "NiPoint3Interpolator" => 3,
        _ => return None,
    };
    if stored.len() != components {
        return None;
    }
    let d = refs(f, "Data").first().copied()?;
    let k = clock::Keys::read(block_fields(n, d)?, components, b).ok()??;
    let key = if end { k.keys.last()? } else { k.keys.first()? };
    Some(
        key.value[..components]
            .iter()
            .zip(stored)
            .all(|(a, s)| (a - s).abs() <= rest::EFFECT_TOLERANCE),
    )
}

#[allow(clippy::too_many_arguments)]
fn section2(
    c: &mut Census,
    n: &Nif<'_>,
    off: &[Vec<&'static str>],
    u: &Use,
    ref_sigs: Option<&BTreeMap<String, u64>>,
    base_sigs: Option<&BTreeMap<String, u64>>,
    name: &str,
    names: &mut String,
) {
    let b = Budget::new(1 << 28);
    c.add("3 models: an effect controller run by a manager", u);
    let graph = (0..n.blocks.len()).any(|i| type_of(n, i) == "BSBehaviorGraphExtraData");
    c.add(
        format!("3 models: the NIF names a behaviour graph {graph}"),
        u,
    );
    // The manager's sequences in its list order.
    let managers: Vec<usize> = (0..n.blocks.len())
        .filter(|i| type_of(n, *i) == "NiControllerManager")
        .collect();
    let mut order: Vec<usize> = Vec::new();
    for mg in &managers {
        if let Some(f) = block_fields(n, *mg) {
            for s in refs(f, "Controller Sequence") {
                if !order.contains(&s) {
                    order.push(s);
                }
            }
        }
    }
    let all: Vec<usize> = (0..n.blocks.len())
        .filter(|i| type_of(n, *i) == "NiControllerSequence")
        .collect();
    let in_list = all.iter().all(|s| order.contains(s));
    let cumulative = managers
        .iter()
        .filter_map(|mg| block_fields(n, *mg))
        .any(|f| {
            f.iter()
                .any(|x| x.name == "Cumulative" && matches!(x.value, Value::U8(1)))
        });
    c.add(
        format!("3 models: a manager with Cumulative set {cumulative}"),
        u,
    );
    c.add(
        format!(
            "3 models: managers {}, sequences {}, every sequence in a manager's list {in_list}",
            managers.len().min(2),
            match all.len() {
                0..=3 => all.len().to_string(),
                _ => "4 or more".into(),
            }
        ),
        u,
    );
    let effect_tagged = |k: usize| off.get(k).is_some_and(|t| t.contains(&rest::EFFECT_IDLE));
    struct Seq {
        looping: bool,
        start_gives_stored: bool,
        effect: bool,
    }
    let mut seqs: Vec<Seq> = Vec::new();
    for (index, &s) in order.iter().enumerate() {
        let Some(f) = block_fields(n, s) else {
            continue;
        };
        let cyc = word(f, "Cycle Type").unwrap_or(9);
        let freq = float(f, "Frequency");
        let (st, sp) = (float(f, "Start Time"), float(f, "Stop Time"));
        let len = match (st, sp) {
            (Some(a), Some(z)) => seconds(f64::from(z - a)),
            _ => "not read",
        };
        let ips: Vec<Option<usize>> = f
            .iter()
            .filter(|x| x.name == "Interpolator")
            .map(|x| match x.value {
                Value::Ref(r) if r >= 0 => Some(r as usize),
                _ => None,
            })
            .collect();
        let ctls: Vec<Option<usize>> = f
            .iter()
            .filter(|x| x.name == "Controller")
            .map(|x| match x.value {
                Value::Ref(r) if r >= 0 => Some(r as usize),
                _ => None,
            })
            .collect();
        let tnames: Vec<Option<String>> = f
            .iter()
            .filter(|x| x.name == "Controller Type")
            .map(|x| match x.value {
                Value::StringIndex(i) => n
                    .strings
                    .get(i as usize)
                    .map(|s| String::from_utf8_lossy(s).into_owned()),
                _ => None,
            })
            .collect();
        let mut what: BTreeSet<String> = BTreeSet::new();
        let mut gives_start = true;
        let mut gives_end = true;
        let mut compared = 0;
        let mut effect = false;
        for (k, ip) in ips.iter().enumerate() {
            let ctl = ctls.get(k).copied().flatten();
            what.insert(drives(n, ctl, tnames.get(k).cloned().flatten().as_deref()));
            effect |= ctl.is_some_and(effect_tagged);
            // Effect fields: the stored value against the keys.
            if let (Some(ctl), Some(ip)) = (ctl, ip) {
                let color = match type_of(n, ctl) {
                    "BSEffectShaderPropertyFloatController" => false,
                    "BSEffectShaderPropertyColorController" => true,
                    _ => continue,
                };
                let Some(cf) = block_fields(n, ctl) else {
                    continue;
                };
                let (Some(var), Some(target)) = (
                    word(cf, "Controlled Variable"),
                    refs(cf, "Target").first().copied(),
                ) else {
                    continue;
                };
                let Some(stored) =
                    block_fields(n, target).and_then(|tf| rest::effect_field(tf, color, var))
                else {
                    continue;
                };
                if matches!(var, 1 | 2) && !color {
                    continue;
                }
                if let (Some(a), Some(z)) = (
                    keys_give(n, *ip, &stored, false, &b),
                    keys_give(n, *ip, &stored, true, &b),
                ) {
                    compared += 1;
                    gives_start &= a;
                    gives_end &= z;
                }
            }
        }
        let text_keys = refs(f, "Text Keys")
            .first()
            .and_then(|t| block_fields(n, *t))
            .map_or(0, |tf| tf.iter().filter(|x| x.name == "Time").count());
        let accum = text(n, f, "Accum Root Name").is_some_and(|t| !t.is_empty());
        let weight = float(f, "Weight") == Some(1.);
        let gives = if compared == 0 {
            "no effect field compared"
        } else {
            match (gives_start, gives_end) {
                (true, true) => "start and end keys give the stored fields",
                (true, false) => "start keys give the stored fields",
                (false, true) => "end keys give the stored fields",
                (false, false) => "neither end gives the stored fields",
            }
        };
        let label = format!(
            "3 sequences: {} (first in the list {}); drives [{}]",
            cycle(cyc),
            index == 0,
            what.iter().cloned().collect::<Vec<_>>().join(" + ")
        );
        c.add(label, u);
        c.add(
            format!(
                "3 sequences: {}; length {len}; frequency 1 {}; weight 1 {weight}; text keys {}; accumulation root named {accum}; names an effect controller {effect}; {gives}",
                cycle(cyc),
                freq == Some(1.),
                text_keys.min(3)
            ),
            u,
        );
        seqs.push(Seq {
            looping: cyc == 0,
            start_gives_stored: compared > 0 && gives_start,
            effect,
        });
    }
    // Data-only signals by model.
    let looping: Vec<usize> = (0..seqs.len()).filter(|k| seqs[*k].looping).collect();
    let start: Vec<usize> = (0..seqs.len())
        .filter(|k| seqs[*k].start_gives_stored && seqs[*k].effect)
        .collect();
    let count = |v: &[usize]| match v.len() {
        0 => "none".to_string(),
        1 => "one".to_string(),
        _ => "several".to_string(),
    };
    let first_loops = seqs.first().is_some_and(|s| s.looping);
    let only_loop_first = looping.len() == 1 && looping[0] == 0;
    let start_first = start.first() == Some(&0) && start.len() == 1;
    let agree = looping.len() == 1 && start.len() == 1 && looping[0] == start[0];
    c.add(
        format!(
            "3 models: looping sequences {}; sequences whose start keys give every compared stored field {}; the first sequence loops {first_loops}",
            count(&looping),
            count(&start)
        ),
        u,
    );
    c.add(
        format!(
            "3 models: the only looping sequence is the first {only_loop_first}; the only start-gives-stored sequence is the first {start_first}; the two signals pick one same sequence {agree}"
        ),
        u,
    );
    c.add(
        format!(
            "3 models: behaviour graph named {graph}; looping {}; start-gives-stored {}",
            count(&looping),
            count(&start)
        ),
        u,
    );
    for (sig, k) in &u.bases {
        let t = c
            .lines
            .entry(format!("3 placements by base record type: {sig}"))
            .or_default();
        t.n += 1;
        t.placements += k;
    }
    // Subrecords of the placed references and of the base records: how
    // many placed references carry each (first column), and for the bases
    // each record type and subrecord with the placements of the bases that
    // carry it (second column).
    if let Some(r) = ref_sigs {
        for (sig, k) in r {
            let t = c
                .lines
                .entry(format!("3 placed references carrying subrecord {sig}"))
                .or_default();
            t.n += k;
        }
    }
    if let Some(r) = base_sigs {
        for (sig, k) in r {
            let t = c
                .lines
                .entry(format!("3 models whose base records carry subrecord {sig}"))
                .or_default();
            t.n += 1;
            t.placements += k;
        }
    }
    let _ = writeln!(
        names,
        "3 model\t{name}\t{}\tgraph {graph}\tlooping {}\tstart {}",
        u.placements,
        count(&looping),
        count(&start)
    );
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
