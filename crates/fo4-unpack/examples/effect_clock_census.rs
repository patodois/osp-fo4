#![forbid(unsafe_code)]
//! Census of the effect-shader fields that animation controllers drive
//! (`spec/controller-clock.md`, Effect fields on the clock and Effect
//! controllers run by a manager): counts only, written to `OUT.txt`; with
//! `NAMES.tsv`, a local listing with model paths (never committed).
//!
//! `effect_clock_census MANIFEST OUT.txt [NAMES.tsv]`
//!
//! Every winning NIF of the archives is read. For each controller of an
//! effect shader (`BSEffectShaderPropertyFloatController`,
//! `BSEffectShaderPropertyColorController` targeting a
//! `BSEffectShaderProperty`): the field it drives (nifxml
//! EffectShaderControlledVariable and EffectShaderControlledColor), what
//! runs it, for a free-running one whether the clock runs it
//! (`fo4_scene::clock::track`), whether its track varies and the range of
//! its values, and its rest tag with the clock's switches off (as the scene
//! reads models by default) and on (`fo4_scene::rest::tags_with`). By model:
//! whether every state block is decided by a rule, either way. For models
//! with an effect controller run by a manager (`effect-idle`): the
//! sequences that name such controllers, their cycle types, where else
//! their names occur (the model's behaviour graph, the strings of the
//! compiled scripts), the stored field against the sequences' keys, the
//! records that place the model and their scripts. Placements are counted
//! through the base records that name each model (placed REFR, enabled).
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Field, Nif, Value};
use fo4_pex::Pex;
use fo4_plugin::typed::Store;
use fo4_scene::{clock, model_policy, rest};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
/// Counts a model-level label once and its placements beside it.
fn both(m: &mut Counts, k: &str, uses: u64) {
    add(m, k, 1);
    add(m, format!("{k} (placements)"), uses);
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

/// The label of an effect-shader field (nifxml enums, by name).
fn field_label(color: bool, variable: u32) -> &'static str {
    match (color, variable) {
        (true, 0) => "emissive colour",
        (true, _) => "colour (other)",
        (false, 0) => "emissive multiple",
        (false, 1) => "falloff start angle",
        (false, 2) => "falloff stop angle",
        (false, 3) => "falloff start opacity",
        (false, 4) => "falloff stop opacity",
        (false, 5) => "alpha",
        (false, 6..=9) => "UV offset or scale",
        _ => "other",
    }
}

/// The range of a track's values over every time, by component.
fn range(t: &clock::Track) -> Option<(f32, f32)> {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for c in 0..t.keys.components {
        let (a, z) = t.keys.bounds(c)?;
        lo = lo.min(a);
        hi = hi.max(z);
    }
    Some((lo, hi))
}

/// (controller, interpolator, sequence block) of every sequence's
/// controlled blocks.
fn sequence_links(n: &Nif<'_>) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for s in 0..n.blocks.len() {
        if type_of(n, s) != "NiControllerSequence" {
            continue;
        }
        let Some(f) = block_fields(n, s) else {
            continue;
        };
        let ips = f.iter().filter(|x| x.name == "Interpolator");
        let ctls = f.iter().filter(|x| x.name == "Controller");
        for (ip, ctl) in ips.zip(ctls) {
            if let (Value::Ref(ip), Value::Ref(ctl)) = (&ip.value, &ctl.value) {
                if *ip >= 0 && *ctl >= 0 {
                    out.push((*ctl as usize, *ip as usize, s));
                }
            }
        }
    }
    out
}

/// The first and last key values of a keyframe interpolator's data, in the
/// reader's field order.
fn end_keys(n: &Nif<'_>, ip: usize, components: usize, b: &Budget) -> Option<(Vec<f32>, Vec<f32>)> {
    let f = block_fields(n, ip)?;
    let d = refs(f, "Data").first().copied()?;
    let k = clock::Keys::read(block_fields(n, d)?, components, b).ok()??;
    let (a, z) = (k.keys.first()?, k.keys.last()?);
    Some((
        a.value[..components].to_vec(),
        z.value[..components].to_vec(),
    ))
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= rest::EFFECT_TOLERANCE)
}

#[derive(Default)]
struct Use {
    placements: u64,
    scripted: u64,
    bases: BTreeMap<String, u64>,
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("effect_clock_census MANIFEST OUT.txt [NAMES.tsv]".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    // Placements by model path: placed REFR -> base -> MODL, with whether
    // the reference or its base carries scripts (VMAD).
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut base_uses: HashMap<fo4_plugin::FormKey, (u64, u64)> = HashMap::new();
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
        let vmad = r.subrecords.iter().any(|s| s.signature == *b"VMAD");
        if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"NAME") {
            if let Ok(d) = <[u8; 4]>::try_from(r.data(s)) {
                if let Ok(Some(base)) =
                    store
                        .order()
                        .remap(plugin, u32::from_le_bytes(d), &r.source.file, s.offset)
                {
                    let e = base_uses.entry(base).or_default();
                    e.0 += 1;
                    e.1 += u64::from(vmad);
                }
            }
        }
    }
    let mut uses: HashMap<Vec<u8>, Use> = HashMap::new();
    for (base, (n, scripted_refs)) in &base_uses {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(base, &b)? else {
            continue;
        };
        let signature = String::from_utf8_lossy(&r.signature).into_owned();
        let base_vmad = r.subrecords.iter().any(|s| s.signature == *b"VMAD");
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
        let u = uses.entry(path).or_default();
        u.placements += n;
        u.scripted += if base_vmad { *n } else { *scripted_refs };
        *u.bases.entry(signature).or_default() += n;
    }
    drop(store);
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    // Every string of every compiled script, lowercased.
    let mut script_strings: HashSet<String> = HashSet::new();
    let mut scripts = 0u64;
    for c in &vfs.candidates {
        let Some(k) = &c.key else { continue };
        if !(k.starts_with(b"scripts/") && k.ends_with(b".pex")) {
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let key = String::from_utf8_lossy(k).into_owned();
        let Ok(bytes) = vfs.read(&key, &b) else {
            continue;
        };
        let Ok(p) = Pex::read(&bytes, &key, &b) else {
            continue;
        };
        scripts += 1;
        for s in &p.strings {
            script_strings.insert(String::from_utf8_lossy(s).to_ascii_lowercase());
        }
    }
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".nif"))
        .collect();
    paths.sort();
    paths.dedup();
    let on = clock::Switches {
        emissive_multiple: true,
        emissive_colour: true,
        falloff: true,
    };
    let mut c = Counts::new();
    let mut names = String::new();
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
        let controllers: Vec<(usize, bool, u32)> = (0..n.blocks.len())
            .filter_map(|i| {
                let ty = type_of(&n, i);
                let color = match ty {
                    "BSEffectShaderPropertyFloatController" => false,
                    "BSEffectShaderPropertyColorController" => true,
                    _ => return None,
                };
                let f = block_fields(&n, i)?;
                let target = refs(f, "Target").first().copied()?;
                (type_of(&n, target) == "BSEffectShaderProperty")
                    .then(|| word(f, "Controlled Variable").map(|v| (i, color, v)))
                    .flatten()
            })
            .collect();
        if controllers.is_empty() {
            continue;
        }
        let u = uses.get(path);
        let placed = u.map_or(0, |u| u.placements);
        let Ok(off) = rest::tags(&n, &b) else {
            add(&mut c, "models: rest tags not read", 1);
            continue;
        };
        let with = rest::tags_with(&n, on, &b)?;
        let has_manager = (0..n.blocks.len()).any(|i| type_of(&n, i) == "NiControllerManager");
        let links = sequence_links(&n);
        both(&mut c, "models: with an effect-shader controller", placed);
        // 1. Every effect controller by field, driver and tag.
        let mut varying_switched = false;
        let mut idle = false;
        for &(i, color, variable) in &controllers {
            let field = field_label(color, variable);
            let f = block_fields(&n, i).unwrap_or(&[]);
            let timing = clock::Timing::read(f);
            let named = links.iter().any(|(ctl, _, _)| *ctl == i);
            let manager_flag = timing.is_some_and(|t| t.manager());
            let driver = if named {
                "named by a sequence"
            } else if manager_flag && has_manager {
                "manager flag, no sequence"
            } else if manager_flag {
                "free-running (manager flag without a manager)"
            } else {
                "free-running"
            };
            add(&mut c, format!("1 controllers: {field}; {driver}"), 1);
            add(
                &mut c,
                format!("1 controllers: {field}; {driver} (placements)"),
                placed,
            );
            let tag_off = off.get(i).map(|t| t.join("+")).unwrap_or_default();
            let tag_on = with.get(i).map(|t| t.join("+")).unwrap_or_default();
            add(
                &mut c,
                format!("1 rest tag: {field}; {driver}; switches off {tag_off}; on {tag_on}"),
                1,
            );
            add(
                &mut c,
                format!(
                    "1 rest tag: {field}; {driver}; switches off {tag_off}; on {tag_on} (placements)"
                ),
                placed,
            );
            if off.get(i).is_some_and(|t| t.contains(&rest::EFFECT_IDLE)) {
                idle = true;
            }
            if !driver.starts_with("free-running") {
                continue;
            }
            match clock::track(&n, i, &b)? {
                Err(why) => {
                    add(
                        &mut c,
                        format!("1 free {field}: not a clock track: {why}"),
                        1,
                    );
                    add(
                        &mut c,
                        format!("1 free {field}: not a clock track: {why} (placements)"),
                        placed,
                    );
                }
                Ok(t) => {
                    let varies = !t.keys.constant();
                    let cycle = match t.timing.cycle() {
                        clock::CYCLE_LOOP => "loop",
                        clock::CYCLE_REVERSE => "reverse",
                        _ => "clamp",
                    };
                    let key = match t.keys.kind {
                        clock::LINEAR => "linear",
                        clock::QUADRATIC => "quadratic",
                        _ => "constant",
                    };
                    let stored = rest::effect_field(
                        block_fields(&n, refs(f, "Target")[0]).unwrap_or(&[]),
                        color,
                        variable,
                    );
                    let degrees = !color && matches!(variable, 1 | 2);
                    let at_load = t.at(0.).map(|v| {
                        v[..t.keys.components]
                            .iter()
                            .map(|x| {
                                if degrees {
                                    rest::degrees_cosine(*x)
                                } else {
                                    *x
                                }
                            })
                            .collect::<Vec<f32>>()
                    });
                    let start = match (&stored, &at_load) {
                        (Some(s), Some(a)) if close(s, a) => "stored = clock at load",
                        (Some(_), Some(_)) => "stored differs from the clock at load",
                        _ => "stored or value not read",
                    };
                    let values = match range(&t) {
                        Some((lo, hi)) if degrees => {
                            format!("degrees {lo:.0} to {hi:.0}").replace("-0", "0")
                        }
                        Some((lo, hi)) => format!(
                            "{}{}",
                            if lo < 0. { "below 0, " } else { "" },
                            if hi > 1. { "above 1" } else { "within 0 to 1" }
                        ),
                        None => "range not read".into(),
                    };
                    let label = format!(
                        "1 free {field}: track: {cycle}, {key}, varies {varies}; {start}; values {values}"
                    );
                    add(&mut c, &label, 1);
                    add(&mut c, format!("{label} (placements)"), placed);
                    if varies && variable <= 4 {
                        varying_switched = true;
                    }
                }
            }
        }
        // By model: decided by the rest rules with the switches off and on.
        let decided =
            |tags: &[Vec<&'static str>]| tags.iter().flatten().all(|t| rest::rule(t).is_some());
        let (d_off, d_on) = (decided(&off), decided(&with));
        both(
            &mut c,
            &format!(
                "1 models: decided with the switches off {d_off}, on {d_on}; a free-running emissive, colour or falloff track that varies {varying_switched}"
            ),
            placed,
        );
        let open_off: BTreeSet<&str> = off
            .iter()
            .flatten()
            .copied()
            .filter(|t| rest::rule(t).is_none())
            .collect();
        let open_on: BTreeSet<&str> = with
            .iter()
            .flatten()
            .copied()
            .filter(|t| rest::rule(t).is_none())
            .collect();
        if open_off != open_on {
            both(
                &mut c,
                &format!(
                    "1 models: open tags off [{}] -> on [{}]",
                    open_off.iter().copied().collect::<Vec<_>>().join(" + "),
                    open_on.iter().copied().collect::<Vec<_>>().join(" + ")
                ),
                placed,
            );
        }
        // 2. Effect controllers run by a manager.
        if !idle {
            continue;
        }
        both(
            &mut c,
            "2 models: an effect controller run by a manager",
            placed,
        );
        let others: Vec<&str> = open_off
            .iter()
            .copied()
            .filter(|t| *t != rest::EFFECT_IDLE)
            .collect();
        both(
            &mut c,
            &format!(
                "2 models: other open tags [{}]",
                if others.is_empty() {
                    "none".to_string()
                } else {
                    others.join(" + ")
                }
            ),
            placed,
        );
        let sequences: BTreeSet<usize> = links
            .iter()
            .filter(|(ctl, _, _)| controllers.iter().any(|(i, _, _)| i == ctl))
            .map(|(_, _, s)| *s)
            .collect();
        let all_sequences = (0..n.blocks.len())
            .filter(|i| type_of(&n, *i) == "NiControllerSequence")
            .count();
        let cycles: BTreeSet<String> = sequences
            .iter()
            .map(|s| {
                match block_fields(&n, *s).and_then(|f| word(f, "Cycle Type")) {
                    Some(0) => "loop",
                    Some(1) => "reverse",
                    Some(2) => "clamp",
                    _ => "other",
                }
                .to_string()
            })
            .collect();
        both(
            &mut c,
            &format!(
                "2 models: sequences naming effect controllers {} of {}; their cycle types [{}]",
                match sequences.len() {
                    0..=3 => sequences.len().to_string(),
                    _ => "4 or more".into(),
                },
                match all_sequences {
                    0..=3 => all_sequences.to_string(),
                    _ => "4 or more".into(),
                },
                cycles.into_iter().collect::<Vec<_>>().join(", ")
            ),
            placed,
        );
        let seq_names: Vec<String> = sequences
            .iter()
            .filter_map(|s| block_fields(&n, *s).and_then(|f| text(&n, f, "Name")))
            .collect();
        let in_scripts = seq_names
            .iter()
            .filter(|s| script_strings.contains(*s))
            .count();
        both(
            &mut c,
            &format!(
                "2 models: names of those sequences held as a string by a compiled script: {}",
                match (in_scripts, seq_names.len()) {
                    (0, _) => "none",
                    (k, t) if k == t => "all",
                    _ => "some",
                }
            ),
            placed,
        );
        // The behaviour graph, from the model's own text under `meshes`.
        let anim = fo4_scene::anim_graph::nif_animation(&n);
        let graph = match anim.graph_file {
            None => "no behaviour graph".to_string(),
            Some(_) => {
                let model = name.strip_prefix("meshes/").unwrap_or(&name);
                match fo4_scene::anim_graph::model_graph(&vfs, model, &m.limits, &b) {
                    Err(_) => "behaviour graph not read".into(),
                    Ok(g) => {
                        let start = g
                            .machine
                            .state(g.machine.start)
                            .and_then(|s| s.sequence.clone())
                            .map(|s| s.to_ascii_lowercase());
                        let played: BTreeSet<String> = g
                            .machine
                            .states
                            .iter()
                            .filter_map(|s| s.sequence.clone())
                            .map(|s| s.to_ascii_lowercase())
                            .collect();
                        let start_effect = start.as_ref().is_some_and(|s| seq_names.contains(s));
                        let any = seq_names.iter().filter(|s| played.contains(*s)).count();
                        format!(
                            "graph start state plays {}; graph states play {} of the effect sequences",
                            match (&start, start_effect) {
                                (None, _) => "no sequence",
                                (Some(_), true) => "an effect sequence",
                                (Some(_), false) => "another sequence",
                            },
                            match (any, seq_names.len()) {
                                (0, _) => "none",
                                (k, t) if k == t => "all",
                                _ => "some",
                            }
                        )
                    }
                }
            }
        };
        both(&mut c, &format!("2 models: {graph}"), placed);
        // A model without a graph whose manager holds one sequence: what
        // that sequence drives.
        if graph == "no behaviour graph" && all_sequences == 1 {
            let mut drives: BTreeSet<String> = BTreeSet::new();
            let mut loops = "other";
            for s in &sequences {
                let Some(f) = block_fields(&n, *s) else {
                    continue;
                };
                loops = match word(f, "Cycle Type") {
                    Some(0) => "loop",
                    Some(2) => "clamp",
                    _ => "other",
                };
                let ctls = f.iter().filter(|x| x.name == "Controller");
                for x in ctls {
                    let label = match x.value {
                        Value::Ref(r) if r >= 0 => {
                            let r = r as usize;
                            match controllers.iter().find(|(i, _, _)| *i == r) {
                                Some((_, color, variable)) => {
                                    format!("effect {}", field_label(*color, *variable))
                                }
                                None => type_of(&n, r).to_string(),
                            }
                        }
                        _ => "no controller link".into(),
                    };
                    drives.insert(label);
                }
            }
            both(
                &mut c,
                &format!(
                    "2 models: no graph, one sequence ({loops}), driving [{}]",
                    drives.into_iter().collect::<Vec<_>>().join(" + ")
                ),
                placed,
            );
        }
        // The stored field against the first and last keys of the
        // sequences' interpolators that drive each manager-run controller.
        for &(i, color, variable) in &controllers {
            if !off.get(i).is_some_and(|t| t.contains(&rest::EFFECT_IDLE)) {
                continue;
            }
            let field = field_label(color, variable);
            let Some(target) = block_fields(&n, i).and_then(|f| refs(f, "Target").first().copied())
            else {
                continue;
            };
            let Some(stored) =
                block_fields(&n, target).and_then(|f| rest::effect_field(f, color, variable))
            else {
                continue;
            };
            let degrees = !color && matches!(variable, 1 | 2);
            let read = |v: Vec<f32>| -> Vec<f32> {
                if degrees {
                    v.into_iter().map(rest::degrees_cosine).collect()
                } else {
                    v
                }
            };
            let mut firsts = 0;
            let mut lasts = 0;
            let mut total = 0;
            let mut unread = 0;
            for (_, ip, _) in links.iter().filter(|(ctl, _, _)| *ctl == i) {
                total += 1;
                match end_keys(&n, *ip, stored.len(), &b) {
                    Some((a, z)) => {
                        firsts += usize::from(close(&read(a), &stored));
                        lasts += usize::from(close(&read(z), &stored));
                    }
                    None => unread += 1,
                }
            }
            let label = format!(
                "2 manager-run controllers: {field}; sequences {}; stored = first key in {}, = last key in {}{}",
                match total {
                    0 => "none".to_string(),
                    1 => "1".to_string(),
                    _ => "2 or more".to_string(),
                },
                match (firsts, total) {
                    (0, _) => "none",
                    (k, t) if k == t => "all",
                    _ => "some",
                },
                match (lasts, total) {
                    (0, _) => "none",
                    (k, t) if k == t => "all",
                    _ => "some",
                },
                if unread > 0 { "; keys not read in some" } else { "" }
            );
            add(&mut c, &label, 1);
            add(&mut c, format!("{label} (placements)"), placed);
        }
        if let Some(u) = u {
            add(
                &mut c,
                "2 placements: whose reference or base carries scripts",
                u.scripted,
            );
            for (sig, k) in &u.bases {
                add(
                    &mut c,
                    format!("2 placements: by base record type {sig}"),
                    *k,
                );
            }
        }
        let _ = writeln!(
            names,
            "{name}\t{placed}\t{}\t{graph}\t{}",
            seq_names.join(","),
            others.join("+")
        );
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# effect_clock_census: effect-shader controllers by field and driver (spec/controller-clock.md)\n# Counts only. Lines ending in (placements) count enabled placed references of the models."
    );
    let _ = writeln!(
        out,
        "winning NIF read {files}, unreadable {failed}; compiled scripts read {scripts} ({} s)",
        started.elapsed().as_secs()
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&args[1], out)?;
    if let Some(p) = args.get(2) {
        std::fs::write(p, names)?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
