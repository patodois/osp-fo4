#![forbid(unsafe_code)]
//! Which scripts the doors, terminals and activators placed in given cells
//! carry, and which native functions their code reaches: measurement
//! support for choosing what a first interpreter needs (`spec/vmad.md`,
//! Survey).
//!
//! `script_survey MANIFEST OUT_DIR CELL[:RADIUS]...` reads every placed
//! reference of each cell (and of the exterior cells within RADIUS) that is
//! not deleted, follows NAME to its base, and for DOOR, TERM and ACTI bases
//! takes the scripts of the base's VMAD, of the reference's VMAD and of the
//! TERM fragment block (`ALL` instead of a cell takes every winning DOOR,
//! TERM and ACTI base). From every function of those scripts and of their
//! parents up to the first that declares a native function, it follows the
//! calls that resolve (`fo4_pex::index`) to scripted functions, and counts
//! the call sites that resolve to native functions. Writes `survey.txt`
//! (names; never versioned) and `survey-counts.txt` (counts only).
use fo4_core::{Budget, Manifest};
use fo4_pex::{
    index::{lower, Frame, Index},
    Pex, Place,
};
use fo4_plugin::{typed::Store, vmad::Vmad, FormKey};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::Write as _,
    path::Path,
};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

const KINDS: [&[u8; 4]; 3] = [b"DOOR", b"TERM", b"ACTI"];

/// Script names of a record's VMAD (list and, for TERM, the fragment block).
fn vmad_scripts(
    store: &Store,
    key: &FormKey,
    b: &Budget,
) -> Result<Vec<(String, &'static str)>, Box<dyn std::error::Error>> {
    let Some(r) = store.raw(key, b)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
        let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b)?;
        for sc in &v.scripts {
            out.push((lower(sc.name), "attached"));
        }
        if let Some(t) = &v.terminal {
            out.push((lower(t.script.name), "fragment"));
        }
    }
    Ok(out)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("script_survey MANIFEST OUT_DIR CELL[:RADIUS]...".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let out_dir = Path::new(&args[1]);
    // Every .pex of the archives, kept as bytes by object name.
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut files: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    let mut ix = Index::default();
    for c in &vfs.candidates {
        let Some(k) = &c.key else { continue };
        if !k.ends_with(b".pex") {
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let bytes = vfs.read_candidate(c, &b)?;
        let path = String::from_utf8_lossy(k).into_owned();
        let p = Pex::read(&bytes, &path, &b)?;
        ix.add(&p);
        let name = lower(p.str(p.objects[0].name));
        files.insert(name, (path, bytes));
    }
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);

    let mut counts = Counts::new();
    // script -> (base kind, attachment) -> placements
    let mut by_script: BTreeMap<String, Counts> = BTreeMap::new();
    for spec in &args[2..] {
        if spec == "ALL" {
            // Every winning DOOR, TERM and ACTI base record instead of cells.
            let bases: Vec<FormKey> = store
                .winners()
                .filter(|w| !w.deleted && KINDS.contains(&&w.signature))
                .map(|w| store.overrides().form_key(w.key, store.order()))
                .collect();
            for base in bases {
                let kind = store
                    .signature(&base)
                    .map(|s| String::from_utf8_lossy(&s).into_owned())
                    .unwrap_or_default();
                let scripts = vmad_scripts(&store, &base, &b)?;
                bump(
                    &mut counts,
                    format!("base {kind} with scripts: {}", !scripts.is_empty()),
                );
                for (sc, how) in scripts {
                    bump(
                        by_script.entry(sc).or_default(),
                        format!("{kind} base {how}"),
                    );
                }
            }
            continue;
        }
        let (cell, radius) = match spec.split_once(':') {
            Some((c, r)) => (c, r.parse::<u32>()?),
            None => (spec.as_str(), 0),
        };
        let key = store
            .find_cell(cell, &b)?
            .ok_or("no cell of that editor ID")?;
        let mut cells = vec![key.clone()];
        if radius > 0 {
            let a = fo4_scene::grid::around(&store, &key, radius, &b)?;
            cells.extend(a.neighbours.into_iter().filter_map(|n| n.cell));
        }
        bump(&mut counts, format!("cell arguments with radius {radius}"));
        for c in &cells {
            bump(&mut counts, "cells");
            let ch = store.children(c, &b)?;
            for x in ch.active.iter().chain(&ch.disabled).chain(&ch.excluded) {
                bump(
                    &mut counts,
                    format!("placed {}", String::from_utf8_lossy(&x.signature)),
                );
                let Some((r, plugin)) = store.raw_with_plugin(&x.key, &b)? else {
                    continue;
                };
                let Some(name) = r.subrecords.iter().find(|s| s.signature == *b"NAME") else {
                    continue;
                };
                let d = r.data(name);
                if d.len() != 4 {
                    continue;
                }
                let id = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                let Some(base) = store
                    .order()
                    .remap(plugin, id, &r.source.file, name.offset)?
                else {
                    continue;
                };
                let Some(sig) = store.signature(&base) else {
                    continue;
                };
                let kind = String::from_utf8_lossy(&sig).into_owned();
                if !KINDS.contains(&&sig) {
                    let any = !vmad_scripts(&store, &base, &b)?.is_empty()
                        || r.subrecords.iter().any(|s| s.signature == *b"VMAD");
                    if any {
                        bump(
                            &mut counts,
                            format!("placed with scripts, other base type {kind}"),
                        );
                    }
                    continue;
                }
                bump(&mut counts, format!("placed {kind}"));
                let mut scripts: Vec<(String, String)> = vmad_scripts(&store, &base, &b)?
                    .into_iter()
                    .map(|(s, how)| (s, format!("{kind} base {how}")))
                    .collect();
                scripts.extend(
                    vmad_scripts(&store, &x.key, &b)?
                        .into_iter()
                        .map(|(s, how)| (s, format!("{kind} reference {how}"))),
                );
                bump(
                    &mut counts,
                    format!("placed {kind} with scripts: {}", !scripts.is_empty()),
                );
                for (s, how) in scripts {
                    bump(by_script.entry(s).or_default(), how);
                }
            }
        }
    }

    // Reachable functions from every function of the attached scripts.
    // The attached scripts and their parents up to the first that declares
    // a native function (whose code every script of that type shares).
    let mut roots: BTreeSet<String> = by_script.keys().cloned().collect();
    let mut parents = 0u64;
    for r in by_script.keys() {
        for (name, d) in ix.chain(r).into_iter().skip(1) {
            if d.functions.values().flatten().any(|f| f.native()) {
                break;
            }
            if roots.insert(name.to_string()) {
                parents += 1;
            }
        }
    }
    counts.insert(
        "parent scripts added as roots (up to the first declaring a native function)".into(),
        parents,
    );
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut queue: VecDeque<(String, Option<String>)> =
        roots.iter().map(|s| (s.clone(), None)).collect();
    let mut native_sites = Counts::new();
    let mut native_users: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut direct_sites = Counts::new();
    let mut scripted_sites = 0u64;
    let mut unresolved = 0u64;
    let mut missing = BTreeSet::new();
    while let Some((script, only)) = queue.pop_front() {
        let Some((path, bytes)) = files.get(&script) else {
            missing.insert(script);
            continue;
        };
        let p = Pex::read(bytes, path, &b)?;
        for site in p.functions() {
            let fname = match site.place {
                Place::State(_, n) => lower(p.str(n)),
                Place::Read(n) => format!("{}.read", lower(p.str(n))),
                Place::Write(n) => format!("{}.write", lower(p.str(n))),
            };
            if only.as_ref().is_some_and(|o| *o != fname) {
                continue;
            }
            let state = match site.place {
                Place::State(s, _) => lower(p.str(s)),
                _ => String::new(),
            };
            if !seen.insert((script.clone(), format!("{state}.{fname}"))) {
                continue;
            }
            let frame = Frame::new(&p, site.object, site.function, &ix);
            for ins in &site.function.code {
                if !(23..=25).contains(&ins.op) {
                    continue;
                }
                match frame.callee(&ix, ins) {
                    None => unresolved += 1,
                    Some(c) if c.decl.native() => {
                        let k = format!("{}.{}", c.script, c.name);
                        if roots.contains(&script) {
                            bump(&mut direct_sites, k.clone());
                        }
                        bump(&mut native_sites, k.clone());
                        native_users.entry(k).or_default().insert(script.clone());
                    }
                    Some(c) => {
                        scripted_sites += 1;
                        queue.push_back((c.script.clone(), Some(c.name.clone())));
                    }
                }
            }
        }
    }

    // Function names defined by the attached scripts themselves.
    let mut defined = Counts::new();
    for r in by_script.keys() {
        if let Some(d) = ix.scripts.get(r) {
            for (n, v) in &d.functions {
                if v.iter().any(|f| !f.native()) {
                    bump(&mut defined, n.clone());
                }
            }
        }
    }
    let mut names = String::new();
    writeln!(
        names,
        "# scripts on placed DOOR, TERM and ACTI of {:?}: placements by base type and attachment",
        &args[2..]
    )?;
    for (s, c) in &by_script {
        let parts: Vec<String> = c.iter().map(|(k, v)| format!("{k} {v}")).collect();
        let parent = ix
            .scripts
            .get(s)
            .map(|d| d.parent.clone())
            .unwrap_or_else(|| "(not a .pex of the archives)".into());
        writeln!(names, "{s}\t(extends {parent})\t{}", parts.join("; "))?;
    }
    let mut ranked: Vec<(u64, &String)> = native_sites.iter().map(|(k, v)| (*v, k)).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    writeln!(names, "\n# native functions reached: call sites in reachable functions, scripts calling, direct sites in the attached scripts")?;
    for (v, k) in &ranked {
        writeln!(
            names,
            "{v}\t{}\t{}\t{k}",
            native_users[*k].len(),
            direct_sites.get(*k).copied().unwrap_or(0)
        )?;
    }
    let mut ranked: Vec<(u64, &String)> = defined.iter().map(|(k, v)| (*v, k)).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    writeln!(
        names,
        "\n# function names defined (with code) by the attached scripts, by number of scripts"
    )?;
    for (v, k) in ranked.iter().take(60) {
        writeln!(names, "{v}\t{k}")?;
    }
    writeln!(names, "\n# scripts named but absent from the archives")?;
    for s in &missing {
        writeln!(names, "{s}")?;
    }
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("survey.txt"), names)?;

    let mut agg = String::new();
    for (k, v) in &counts {
        writeln!(agg, "{v}\t{k}")?;
    }
    writeln!(agg, "{}\tdistinct scripts attached", by_script.len())?;
    writeln!(agg, "{}\treachable functions", seen.len())?;
    writeln!(
        agg,
        "{}\tscripts in the reach",
        seen.iter().map(|(s, _)| s).collect::<BTreeSet<_>>().len()
    )?;
    writeln!(agg, "{}\tcall sites to scripted functions", scripted_sites)?;
    writeln!(
        agg,
        "{}\tcall sites to native functions",
        native_sites.values().sum::<u64>()
    )?;
    writeln!(
        agg,
        "{}\tdistinct native functions reached",
        native_sites.len()
    )?;
    writeln!(
        agg,
        "{}\tdistinct native functions called directly by attached scripts",
        direct_sites.len()
    )?;
    writeln!(agg, "{unresolved}\tcall sites not resolved")?;
    std::fs::write(out_dir.join("survey-counts.txt"), agg)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
