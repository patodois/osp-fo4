#![forbid(unsafe_code)]
//! Census of why a run timeline's actors fall back to a marker and what
//! the look readings change (`spec/faces-census.txt`, the faces step:
//! `spec/actor.md`, Picked looks; `spec/timeline.md`, The elevator second).
//! Counts, stored numbers and descriptions only in `OUT.txt`; record keys,
//! editor ids and file names only in `NAMES.txt` (never versioned).
//!
//! `faces_census MANIFEST TIMELINE OUT.txt NAMES.txt [--seed N] [--shaft PLUGIN:HEX] [--models]`
//!
//! 1. the actors the timeline places, each resolved as the cast resolves
//!    it (`fo4_scene::actor::resolve_reference_with`) under the four
//!    combinations of `--actor-template-bits` and `--actor-leveled-seed`
//!    (seed `--seed`, default 1), classed by why it has no drawable plan;
//! 2. the chains of those actors: their bases, the leveled lists (LVLN)
//!    they reach and the leveled item lists (LVLI) their outfits reach,
//!    by their LVLD, LVLM and LVLF bytes, entries and levels;
//! 3. over every NPC_ of the load order with TPTA: whether its own RNAM,
//!    WNAM, PNAM, DOFT and KWDA are present, by template bit set or clear,
//!    for every bit;
//! 4. `--shaft`: the placed reference's draw blockers, each with the rule
//!    that covers it or none, and how many base models of the load order
//!    hold the block types the uncovered blockers name;
//! 5. `--models`: every base record of the placed kinds with a model
//!    (MODL): how many of their models hold skinned shapes
//!    (`BSSkin::Instance`), and of those how many a controller manager or
//!    a behaviour graph also runs.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::{Leveled, Npc, Otft},
    typed::Store,
    FormKey, Record,
};
use fo4_scene::{
    actor::{self, Readings},
    timeline::{self as tl, Timeline, What},
    Loader, RuleLevel,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}
fn edid(r: &Record) -> String {
    r.subrecords
        .iter()
        .find(|s| s.signature == *b"EDID")
        .map(|s| {
            let d = r.data(s);
            String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
        })
        .unwrap_or_default()
}
fn has(r: &Record, s: &[u8; 4]) -> bool {
    r.subrecords.iter().any(|x| x.signature == *s)
}
/// A reason's text with record keys and numbers of entries replaced, so
/// that equal reasons count together.
fn class(text: &str) -> String {
    let mut out = String::new();
    for w in text.split(' ') {
        let ends = [';', ',', ')', ':'];
        let hex = w.split_once(':').is_some_and(|(p, h)| {
            !p.is_empty()
                && h.trim_end_matches(ends).len() >= 5
                && h.trim_end_matches(ends)
                    .chars()
                    .all(|c| c.is_ascii_hexdigit())
        });
        if !out.is_empty() {
            out.push(' ');
        }
        if hex {
            out.push('K');
            let tail = &w[w.trim_end_matches(ends).len()..];
            out.push_str(tail);
        } else if w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty() {
            out.push('N');
        } else {
            out.push_str(w);
        }
    }
    out
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str =
        "faces_census MANIFEST TIMELINE OUT.txt NAMES.txt [--seed N] [--shaft PLUGIN:HEX] [--models]";
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    let (mut seed, mut shaft, mut models) = (1u64, None, false);
    let mut it = args[4..].iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--seed" => seed = it.next().ok_or(USAGE)?.parse()?,
            "--shaft" => shaft = tl::parse_key(it.next().ok_or(USAGE)?),
            "--models" => models = true,
            _ => return Err(USAGE.into()),
        }
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let b = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let t = Timeline::parse(&std::fs::read_to_string(&args[1])?)?;
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(
        out,
        "# Why a run timeline's actors fall back to a marker, and what the look readings change (faces_census; counts only)"
    );

    // 1. The actors the timeline places, under the four readings.
    let mut actors: BTreeSet<FormKey> = BTreeSet::new();
    for e in &t.events {
        if let What::Actor { reference, .. } = &e.what {
            actors.insert(tl::norm(reference));
        }
    }
    let _ = writeln!(
        out,
        "\n## 1. The timeline's actors as the cast resolves them\nactors placed by the timeline\t{}",
        actors.len()
    );
    let r = |template_bits, picked: bool, items: bool| Readings {
        template_bits,
        leveled_seed: picked.then_some(seed),
        chance_none: items,
        use_all: items,
        head_slots: false,
    };
    let variants = [
        ("every reading off", r(false, false, false)),
        ("--actor-template-bits", r(true, false, false)),
        ("--actor-leveled-seed", r(false, true, false)),
        (
            "--actor-leveled-seed --actor-leveled-chance-none --actor-leveled-use-all",
            r(false, true, true),
        ),
        (
            "--actor-template-bits --actor-leveled-seed",
            r(true, true, false),
        ),
        ("all four", r(true, true, true)),
    ];
    let mut loader = Loader::new(&store, &vfs, &m.language, RuleLevel::Hypotheses, &b);
    let mut per_actor: BTreeMap<FormKey, Vec<String>> = BTreeMap::new();
    for (vi, (label, readings)) in variants.iter().enumerate() {
        let mut classes = Counts::new();
        let mut rules = Counts::new();
        for k in &actors {
            let src = fo4_core::SourceSpan::new("timeline", 0, 0);
            let a = actor::resolve_reference_with(&mut loader, k, &src, *readings, &b)?;
            let c = match (&a.plan, a.state.ready_at(RuleLevel::Hypotheses)) {
                (Some(_), true) => {
                    "a plan, ready under hypotheses (drawn unless the build refuses it)".to_string()
                }
                (Some(_), false) => {
                    let open: BTreeSet<String> = a
                        .blockers
                        .iter()
                        .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
                        .map(|e| e.code.to_string())
                        .collect();
                    format!(
                        "a plan, blocked: {}",
                        open.into_iter().collect::<Vec<_>>().join(" + ")
                    )
                }
                (None, _) => {
                    let why: Vec<&str> = a
                        .notes
                        .iter()
                        .filter(|n| !n.starts_with("head:") && !n.contains("hidden by worn"))
                        .map(String::as_str)
                        .collect();
                    format!(
                        "no plan: {}",
                        class(
                            if why.is_empty() {
                                "not resolved".into()
                            } else {
                                why.join("; ")
                            }
                            .as_str()
                        )
                    )
                }
            };
            if a.plan.is_some() {
                if let fo4_scene::State::ReadyUnderHypotheses { hypotheses } = &a.state {
                    for h in hypotheses {
                        if [
                            fo4_scene::hypotheses::ACTOR_LEVELED_PICK,
                            fo4_scene::hypotheses::ACTOR_LEVELED_CHANCE_NONE,
                            fo4_scene::hypotheses::ACTOR_LEVELED_USE_ALL,
                            fo4_scene::hypotheses::ACTOR_TEMPLATE_BITS,
                            fo4_scene::hypotheses::ACTOR_CANDIDATES,
                            fo4_scene::hypotheses::ACTOR_LEVELED_FLAGS,
                            fo4_scene::hypotheses::ACTOR_TEMPLATE_ALL_BITS,
                        ]
                        .contains(h)
                        {
                            bump(&mut rules, *h);
                        }
                    }
                }
                if a.keyword_base.is_some() {
                    bump(&mut rules, "keywords' NPC_ settled (for the start clip)");
                }
            }
            bump(&mut classes, c.clone());
            let e = per_actor.entry(k.clone()).or_default();
            if e.len() < vi + 1 {
                e.resize(vi + 1, String::new());
            }
            let open: Vec<String> = a
                .blockers
                .iter()
                .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
                .map(|e| format!("{} {}", e.code, e.found))
                .collect();
            e[vi] = format!(
                "{c} | {}{} | open: {}",
                a.notes.join("; "),
                a.look
                    .as_deref()
                    .map(|l| format!(" | look: {l}"))
                    .unwrap_or_default(),
                open.join("; ")
            );
        }
        let _ = writeln!(out, "### {label}");
        for (c, n) in &classes {
            let _ = writeln!(out, "  {n}\t{c}");
        }
        for (r, n) in &rules {
            let _ = writeln!(out, "  ready actors relying on {r}\t{n}");
        }
    }
    for (k, v) in &per_actor {
        let label = t.labels.get(k).cloned().unwrap_or_default();
        for (vi, line) in v.iter().enumerate() {
            let _ = writeln!(
                names,
                "actor {} {label} [{}] {line}",
                tl::key_text(k),
                variants[vi].0
            );
        }
    }

    // 2. The chains.
    let _ = writeln!(out, "\n## 2. The chains of the timeline's actors");
    let mut bases = Counts::new();
    let mut lvln: BTreeMap<FormKey, String> = BTreeMap::new();
    let mut lvli: BTreeMap<FormKey, String> = BTreeMap::new();
    let mut npcs: BTreeSet<FormKey> = BTreeSet::new();
    let mut bits = Counts::new();
    // Walk every chain (every entry, every set template slot), bounded.
    let mut stack: Vec<(FormKey, u32)> = Vec::new();
    for k in &actors {
        let Some((r, p)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin: p,
        };
        let Ok(a) = fo4_plugin::actor::Achr::decode(&r, ctx, &b) else {
            bump(&mut bases, "reference not an ACHR");
            continue;
        };
        let Some(base) = a.base.and_then(|f| f.key) else {
            bump(&mut bases, "no base");
            continue;
        };
        let kind = match store.raw(&base, &b)? {
            Some(br) if br.signature == *b"NPC_" => {
                if has(&br, b"TPTA") || has(&br, b"TPLT") {
                    "NPC_ with a template"
                } else {
                    "NPC_ without a template"
                }
            }
            Some(br) if br.signature == *b"LVLN" => "LVLN",
            Some(_) => "other record",
            None => "no winning record",
        };
        bump(&mut bases, kind);
        stack.push((base, 0));
    }
    let mut seen: BTreeSet<FormKey> = BTreeSet::new();
    while let Some((k, d)) = stack.pop() {
        if d > 8 || !seen.insert(k.clone()) {
            continue;
        }
        let Some((r, p)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin: p,
        };
        match &r.signature {
            b"LVLN" => {
                let Ok(l) = Leveled::decode(&r, ctx, &b) else {
                    continue;
                };
                let levels: BTreeSet<u16> = l.entries.iter().map(|e| e.level).collect();
                lvln.insert(
                    k.clone(),
                    format!(
                        "LVLD {:?} LVLM {:?} LVLF {:?}, {} entries, {} levels",
                        l.bytes[0],
                        l.bytes[1],
                        l.bytes[2],
                        l.entries.len(),
                        levels.len()
                    ),
                );
                let _ = writeln!(names, "lvln {} {} {}", tl::key_text(&k), edid(&r), lvln[&k]);
                for e in &l.entries {
                    if let Some(x) = e.reference.key.clone() {
                        stack.push((x, d + 1));
                    }
                }
            }
            b"NPC_" => {
                let Ok(n) = Npc::decode(&r, ctx, &b) else {
                    continue;
                };
                npcs.insert(k.clone());
                let tf = n.template_flags.unwrap_or(0);
                for bit in 0..13 {
                    if tf & (1 << bit) != 0 {
                        bump(&mut bits, format!("bit {bit:2}"));
                    }
                }
                match &n.template_slots {
                    Some(slots) => {
                        for (bit, s) in slots.iter().enumerate() {
                            if tf & (1 << bit) != 0 {
                                if let Some(x) = s.as_ref().and_then(|f| f.key.clone()) {
                                    stack.push((x, d + 1));
                                }
                            }
                        }
                    }
                    None => stack.extend(n.template.and_then(|f| f.key).map(|x| (x, d + 1))),
                }
                if let Some(o) = n.outfit.and_then(|f| f.key) {
                    let Some((or, op)) = store.raw_with_plugin(&o, &b)? else {
                        continue;
                    };
                    let octx = fo4_plugin::actor::Context {
                        order: store.order(),
                        plugin: op,
                    };
                    let Ok(otft) = Otft::decode(&or, octx, &b) else {
                        continue;
                    };
                    let mut items: Vec<FormKey> =
                        otft.items.iter().filter_map(|f| f.key.clone()).collect();
                    let mut depth = 0;
                    while let Some(x) = items.pop() {
                        depth += 1;
                        if depth > 512 || store.signature(&x) != Some(*b"LVLI") {
                            continue;
                        }
                        if lvli.contains_key(&x) {
                            continue;
                        }
                        let Some((ir, ip)) = store.raw_with_plugin(&x, &b)? else {
                            continue;
                        };
                        let ictx = fo4_plugin::actor::Context {
                            order: store.order(),
                            plugin: ip,
                        };
                        let Ok(l) = Leveled::decode(&ir, ictx, &b) else {
                            continue;
                        };
                        let kinds: BTreeSet<String> = l
                            .entries
                            .iter()
                            .filter_map(|e| e.reference.key.as_ref())
                            .filter_map(|k| store.signature(k).map(sig))
                            .collect();
                        lvli.insert(
                            x.clone(),
                            format!(
                                "LVLD {:?} LVLM {:?} LVLF {:?}, {} entries of {}",
                                l.bytes[0],
                                l.bytes[1],
                                l.bytes[2],
                                l.entries.len(),
                                kinds.into_iter().collect::<Vec<_>>().join("+")
                            ),
                        );
                        let _ = writeln!(
                            names,
                            "lvli {} {} {}",
                            tl::key_text(&x),
                            edid(&ir),
                            lvli[&x]
                        );
                        items.extend(l.entries.iter().filter_map(|e| e.reference.key.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    let _ = writeln!(out, "bases of the timeline's actors:");
    for (c, n) in &bases {
        let _ = writeln!(out, "  {n}\t{c}");
    }
    let _ = writeln!(
        out,
        "NPC_ reached through templates and leveled entries\t{}",
        npcs.len()
    );
    let _ = writeln!(out, "template bits set among them:");
    for (c, n) in &bits {
        let _ = writeln!(out, "  {n}\t{c}");
    }
    let mut c = Counts::new();
    for v in lvln.values() {
        bump(&mut c, v.split(", ").next().unwrap_or_default());
    }
    let _ = writeln!(out, "LVLN reached: {} (by bytes)", lvln.len());
    for (k, n) in &c {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let mut c = Counts::new();
    for v in lvln.values() {
        bump(&mut c, v.split_once(", ").map_or("", |x| x.1));
    }
    let _ = writeln!(out, "LVLN reached (by entries and distinct levels):");
    for (k, n) in &c {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let mut c = Counts::new();
    for v in lvli.values() {
        bump(&mut c, v.clone());
    }
    let _ = writeln!(
        out,
        "LVLI reached through the outfits of those NPC_: {} (by bytes, entries and entry records)",
        lvli.len()
    );
    for (k, n) in &c {
        let _ = writeln!(out, "  {n}\t{k}");
    }

    // 3. Template bits against own fields, over the load order.
    let _ = writeln!(
        out,
        "\n## 3. Every NPC_ with TPTA: own field present, by template bit clear / set"
    );
    let fields: [&[u8; 4]; 5] = [b"RNAM", b"WNAM", b"PNAM", b"DOFT", b"KWDA"];
    let mut table: BTreeMap<(usize, usize), [u64; 4]> = BTreeMap::new();
    let mut with_tpta = 0u64;
    let all: Vec<FormKey> = store
        .winners()
        .filter(|w| w.signature == *b"NPC_" && !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    for k in &all {
        let work = b.child();
        let Some((r, p)) = store.raw_with_plugin(k, &work)? else {
            continue;
        };
        if !has(&r, b"TPTA") {
            continue;
        }
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin: p,
        };
        let Ok(n) = Npc::decode(&r, ctx, &work) else {
            continue;
        };
        with_tpta += 1;
        let tf = n.template_flags.unwrap_or(0);
        for (fi, f) in fields.iter().enumerate() {
            for bit in 0..13usize {
                let e = table.entry((fi, bit)).or_default();
                let at = if tf & (1 << bit) != 0 { 2 } else { 0 };
                e[at] += 1;
                if has(&r, f) {
                    e[at + 1] += 1;
                }
            }
        }
    }
    let _ = writeln!(out, "NPC_ with TPTA\t{with_tpta}");
    let _ = writeln!(
        out,
        "field\tbit\tclear\town present\tshare\tset\town present\tshare"
    );
    for ((fi, bit), e) in &table {
        let share = |n: u64, d: u64| {
            if d == 0 {
                "-".to_string()
            } else {
                format!("{:.3}", n as f64 / d as f64)
            }
        };
        let _ = writeln!(
            out,
            "{}\t{bit}\t{}\t{}\t{}\t{}\t{}\t{}",
            sig(*fields[*fi]),
            e[0],
            e[1],
            share(e[1], e[0]),
            e[2],
            e[3],
            share(e[3], e[2])
        );
    }

    // 4. The shaft's draw blockers.
    if let Some(sk) = &shaft {
        let _ = writeln!(
            out,
            "\n## 4. The moving platform's model as the draw sees it"
        );
        let inst = loader.instance(sk, &b)?;
        let _ = writeln!(
            out,
            "draw state {:?}; collision {:?}",
            format!("{:?}", inst.state)
                .split([' ', '{'])
                .next()
                .unwrap_or(""),
            format!("{:?}", inst.collision)
                .split([' ', '{'])
                .next()
                .unwrap_or("")
        );
        let mut by: BTreeMap<(String, String, String), u64> = BTreeMap::new();
        for e in &inst.blockers {
            let rule = fo4_scene::hypotheses::covering(e).unwrap_or("none");
            let found = e.found.split(';').next().unwrap_or("");
            // No file names in the versioned file: paths and hex paths
            // are named as such.
            let found = if found.contains(['/', '\\'])
                || [".nif", ".dds", ".bgsm", ".bgem", ".hkx"]
                    .iter()
                    .any(|x| found.to_ascii_lowercase().contains(x))
                || (found.len() > 16 && found.chars().all(|c| c.is_ascii_hexdigit()))
            {
                "(a path)".to_string()
            } else {
                found.to_string()
            };
            let _ = writeln!(names, "shaft blocker {} {}", e.code, e.found);
            *by.entry((e.code.to_string(), found, rule.to_string()))
                .or_default() += 1;
        }
        let _ = writeln!(
            out,
            "blockers (code, first part of found, covering rule): count"
        );
        for ((code, found, rule), n) in &by {
            let _ = writeln!(out, "  {code}\t{found}\t{rule}\t{n}");
        }
    }

    // 5. Skinned models of the placed kinds.
    if models {
        let _ = writeln!(
            out,
            "\n## 5. Base records of the placed kinds with a model (MODL): skinned models"
        );
        let mut paths: BTreeMap<Vec<u8>, BTreeSet<String>> = BTreeMap::new();
        let mut bases = 0u64;
        let winners: Vec<FormKey> = store
            .winners()
            .filter(|w| !w.deleted && fo4_scene::BASE_TYPES.contains(&w.signature))
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        for k in &winners {
            let work = b.child();
            let Some(r) = store.raw(k, &work)? else {
                continue;
            };
            let Some(m) = r.subrecords.iter().find(|x| x.signature == *b"MODL") else {
                continue;
            };
            let d = r.data(m);
            let raw = d.strip_suffix(&[0]).unwrap_or(d);
            let Ok(p) = fo4_scene::paths::resolve_with(
                fo4_scene::paths::Citation::RecordModel,
                raw,
                &vfs,
                RuleLevel::Hypotheses,
                &work,
            ) else {
                continue;
            };
            bases += 1;
            paths.entry(p.key).or_default().insert(sig(r.signature));
        }
        let mut c = Counts::new();
        for (p, kinds) in &paths {
            let work = b.child();
            let name = String::from_utf8_lossy(p).into_owned();
            let Ok(bytes) = vfs.read(&name, &work) else {
                bump(&mut c, "models not read");
                continue;
            };
            let Ok(n) =
                fo4_nif::Nif::read(&bytes, &name, fo4_core::Game::Fallout4, &m.limits, &work)
            else {
                bump(&mut c, "models not decoded");
                continue;
            };
            bump(&mut c, "models read");
            let has = |t: &str| n.block_types.iter().any(|x| x == t);
            if !has("BSSkin::Instance") {
                continue;
            }
            bump(&mut c, "models with skinned shapes");
            for k in kinds {
                bump(&mut c, format!("  of base kind {k}"));
            }
            let manager = has("NiControllerManager");
            let graph = has("BSBehaviorGraphExtraData");
            bump(
                &mut c,
                format!("  with a controller manager {manager}, with a behaviour graph {graph}"),
            );
            let _ = writeln!(
                names,
                "skinned model {name} kinds {kinds:?} manager {manager} graph {graph}"
            );
        }
        let _ = writeln!(
            out,
            "bases with a model path that resolves\t{bases}\ndistinct models\t{}",
            paths.len()
        );
        for (k, n) in &c {
            let _ = writeln!(out, "  {n}\t{k}");
        }
    }

    std::fs::write(&args[2], out)?;
    std::fs::write(&args[3], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
