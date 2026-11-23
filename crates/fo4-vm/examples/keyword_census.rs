#![forbid(unsafe_code)]
//! Keywords in the compiled scripts and the records (`spec/vm.md`,
//! Keywords): `keyword_census MANIFEST OUT.txt NAMES.txt
//! [--family EDITOR_ID_PREFIX]`.
//!
//! OUT.txt holds counts and kinds only (`spec/keyword-census.txt`);
//! NAMES.txt names scripts and records and is never versioned. Nothing
//! runs. The native names are read from the scripts (the functions
//! without code that scripts declaring natives declare), never from a list
//! of the game's:
//!
//! 0. the natives whose name holds `keyword`, by the script declaring them;
//! 1. their call sites over every script, by called name, by the engine
//!    type the calling script extends, by the receiver (`self` or another
//!    value) and by the keyword argument (a variable or property, a literal
//!    none, another literal);
//! 2. where base keywords come from in the records: the winning records
//!    carrying `KWDA` by record type, with their entry counts, whether
//!    `KSIZ` agrees, and what the entries name; the placed records carrying
//!    `KWDA` or `KSIZ` themselves; actor bases with a template (`TPLT`);
//!    placed actors and references by what the keyword reading of
//!    `fo4_vm::keyword` can settle for them (`vm_keyword_records_v1`);
//! 3. with `--family`: the same call sites in the scripts the quests of
//!    that editor ID prefix attach (their own, their aliases', their stage
//!    fragment script), the fragment scripts of the scenes of that prefix,
//!    and those scripts' ancestors.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, vmad::Vmad, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{bind, bind::record_key, Lit, MapLibrary};
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

/// Engine types a script can extend, most specific first.
const TYPES: [&str; 9] = [
    "actor",
    "objectreference",
    "referencealias",
    "refcollectionalias",
    "activemagiceffect",
    "quest",
    "location",
    "form",
    "scriptobject",
];

/// Record types of placed records.
const PLACED: [&[u8; 4]; 10] = [
    b"REFR", b"ACHR", b"PGRE", b"PMIS", b"PHZD", b"PARW", b"PBAR", b"PBEA", b"PCON", b"PFLA",
];

fn extends(lib: &MapLibrary, script: &str) -> String {
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        if TYPES.contains(&t.as_str()) {
            return t;
        }
        let Some(s) = lib.scripts.get(&t) else {
            return "a script the library does not hold".into();
        };
        t = s.parent.clone();
        if t.is_empty() {
            return "another type".into();
        }
    }
    "a chain too deep".into()
}

/// The script and its ancestors.
fn chain(lib: &MapLibrary, script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else {
            break;
        };
        out.push(t.clone());
        t = s.parent.clone();
        if t.is_empty() {
            break;
        }
    }
    out
}

fn call_name(i: &fo4_vm::Ins) -> Option<&str> {
    match (i.op, i.args.first(), i.args.get(1)) {
        (23 | 24, Some(Lit::Ident(n)), _) => Some(n.as_str()),
        (25, Some(Lit::Ident(_)), Some(Lit::Ident(n))) => Some(n.as_str()),
        _ => None,
    }
}

/// Every function with code of a script, property accessors included.
fn bodies(s: &fo4_vm::Script) -> Vec<std::rc::Rc<fo4_vm::Func>> {
    let mut fs: Vec<_> = s.functions.values().cloned().collect();
    for p in &s.properties {
        fs.extend(p.read.iter().cloned());
        fs.extend(p.write.iter().cloned());
    }
    fs
}

/// The call sites of the keyword natives in one script.
fn calls_in(
    lib: &MapLibrary,
    s: &fo4_vm::Script,
    natives: &BTreeSet<String>,
    c: &mut Counts,
    sec: &str,
) {
    let ext = extends(lib, &s.name);
    for f in bodies(s) {
        for i in &f.code {
            let Some(name) = call_name(i) else { continue };
            if !natives.contains(name) {
                continue;
            }
            bump(c, format!("{sec} call sites by called name\t{name}"));
            bump(
                c,
                format!("{sec} call sites by the type the calling script extends\t{name}\t{ext}"),
            );
            if i.op == 23 {
                let recv = match i.args.get(1) {
                    Some(Lit::Ident(x)) if x == "self" => "self",
                    Some(Lit::Ident(_)) => "a variable or property",
                    _ => "another value",
                };
                bump(
                    c,
                    format!("{sec} call sites by the receiver\t{name}\t{recv}"),
                );
            } else {
                bump(
                    c,
                    format!("{sec} call sites by the receiver\t{name}\ta static call"),
                );
            }
            let arg = match i.extra.first() {
                Some(Lit::Ident(_)) => "a variable or property",
                Some(Lit::None) => "a literal none",
                None => "no argument",
                Some(_) => "another literal",
            };
            bump(
                c,
                format!("{sec} call sites by the first argument\t{name}\t{arg}"),
            );
        }
    }
}

/// The FormIDs of a record's KWDA, resolved; `None` for an entry naming no
/// record.
fn kwda(
    store: &Store,
    r: &fo4_plugin::Record,
    plugin: usize,
) -> Option<(Vec<Option<FormKey>>, Option<u32>)> {
    let s = r.subrecords.iter().find(|s| s.signature == *b"KWDA")?;
    let d = r.data(s);
    let ksiz = r
        .subrecords
        .iter()
        .find(|s| s.signature == *b"KSIZ")
        .map(|s| r.data(s))
        .filter(|d| d.len() == 4)
        .map(|d| u32::from_le_bytes([d[0], d[1], d[2], d[3]]));
    let (words, _) = d.as_chunks::<4>();
    let mut out = Vec::new();
    for w in words {
        let id = u32::from_le_bytes(*w);
        out.push(
            store
                .order()
                .remap(plugin, id, &r.source.file, s.offset)
                .ok()
                .flatten(),
        );
    }
    Some((out, ksiz))
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut family = None;
    if let Some(i) = a.iter().position(|x| x == "--family") {
        family = Some(
            a.get(i + 1)
                .cloned()
                .ok_or("--family EDITOR_ID_PREFIX")?
                .to_ascii_lowercase(),
        );
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err("keyword_census MANIFEST OUT.txt NAMES.txt [--family EDITOR_ID_PREFIX]".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
    // 0: the keyword natives, read from the scripts that declare natives.
    let mut natives: BTreeSet<String> = BTreeSet::new();
    for s in lib.scripts.values() {
        for ((_, n), f) in &s.functions {
            if f.native() && n.contains("keyword") {
                natives.insert(n.clone());
                bump(
                    &mut c,
                    format!("0 keyword natives declared\t{}.{n}", s.name),
                );
            }
        }
    }
    *c.entry("0 scripts read".into()).or_default() += lib.scripts.len() as u64;
    let _ = writeln!(names, "## keyword native names: {natives:?}");
    // 1: call sites over every script.
    for s in lib.scripts.values() {
        calls_in(&lib, s, &natives, &mut c, "1 over every script:");
    }
    // 2 and 3: the records.
    let winners: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut templated: BTreeSet<FormKey> = BTreeSet::new();
    let mut placed_bases: Vec<(FormKey, [u8; 4], FormKey)> = Vec::new();
    let mut family_scripts: BTreeSet<String> = BTreeSet::new();
    // Totals written even when nothing is found.
    for k in [
        "2 placed records carrying KWDA or KSIZ, any type",
        "2 KWDA whose KSIZ is missing or disagrees, any type",
    ] {
        c.entry(k.into()).or_default();
    }
    for k in winners {
        let b = Budget::new(max);
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let sig = String::from_utf8_lossy(&r.signature).into_owned();
        let placed = PLACED.contains(&&r.signature);
        if placed {
            if r.subrecords
                .iter()
                .any(|s| s.signature == *b"KWDA" || s.signature == *b"KSIZ")
            {
                bump(
                    &mut c,
                    format!("2 placed records carrying KWDA or KSIZ\t{sig}"),
                );
                bump(&mut c, "2 placed records carrying KWDA or KSIZ, any type");
            }
            if let Ok(Some(base)) = bind::base_of(&store, &k, &b) {
                placed_bases.push((k.clone(), r.signature, base));
            }
        }
        if r.signature == *b"NPC_" && r.subrecords.iter().any(|s| s.signature == *b"TPLT") {
            templated.insert(k.clone());
            bump(&mut c, "2 actor bases (NPC_) with a template (TPLT)");
            if r.subrecords.iter().any(|s| s.signature == *b"KWDA") {
                bump(
                    &mut c,
                    "2 actor bases (NPC_) with a template (TPLT) and their own KWDA",
                );
            }
        }
        if let Some((entries, ksiz)) = kwda(&store, &r, plugin) {
            bump(
                &mut c,
                format!("2 winning records carrying KWDA by type\t{sig}"),
            );
            *c.entry(format!(
                "2 KWDA entries by the carrying record's type\t{sig}"
            ))
            .or_default() += entries.len() as u64;
            let agrees = match ksiz {
                Some(n) => n as usize == entries.len(),
                None => false,
            };
            if !agrees {
                bump(
                    &mut c,
                    format!("2 KWDA whose KSIZ is missing or disagrees\t{sig}"),
                );
                bump(
                    &mut c,
                    "2 KWDA whose KSIZ is missing or disagrees, any type",
                );
            }
            for e in &entries {
                let what = match e.as_ref().and_then(|k| store.signature(k)) {
                    Some(s) => format!("a winning {}", String::from_utf8_lossy(&s)),
                    None => "no winning record".into(),
                };
                bump(&mut c, format!("2 KWDA entries naming\t{what}"));
            }
        }
        // 3: the family's scripts.
        let Some(p) = family.as_ref() else { continue };
        if r.signature != *b"QUST" && r.signature != *b"SCEN" {
            continue;
        }
        if !bind::editor_id(&r).is_some_and(|e| e.to_ascii_lowercase().starts_with(p)) {
            continue;
        }
        let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"VMAD") else {
            continue;
        };
        let Ok(v) = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b) else {
            bump(&mut c, "3 the family: VMAD not read");
            continue;
        };
        let _ = writeln!(names, "## family {sig} {}", record_key(&k));
        for x in &v.scripts {
            family_scripts.insert(fo4_pex::index::lower(x.name));
        }
        if let Some(q) = &v.quest {
            if !q.script.name.is_empty() {
                family_scripts.insert(fo4_pex::index::lower(q.script.name));
            }
            for al in &q.aliases {
                for x in &al.scripts {
                    family_scripts.insert(fo4_pex::index::lower(x.name));
                }
            }
        }
        if let Some(sc) = &v.scene {
            if !sc.block.script.name.is_empty() {
                family_scripts.insert(fo4_pex::index::lower(sc.block.script.name));
            }
        }
    }
    // 2: placed records by what the keyword reading settles.
    for (k, sig, base) in &placed_bases {
        let psig = String::from_utf8_lossy(sig);
        let bsig = store
            .signature(base)
            .map(|s| String::from_utf8_lossy(&s).into_owned())
            .unwrap_or_else(|| "no winning record".into());
        let what = match bsig.as_str() {
            "NPC_" if templated.contains(base) => {
                "open where the keyword is absent: the actor base has a template"
            }
            "NPC_" => "settled: the actor base's and its race's KWDA",
            "LVLN" => "open: a leveled actor base",
            "WEAP" | "ARMO" => "open where the keyword is absent: object modifications not read",
            "no winning record" => "open: the base names no winning record",
            _ => "settled: the base's KWDA",
        };
        bump(
            &mut c,
            format!("2 placed records by what the keyword reading settles\t{psig}\t{what}"),
        );
        if what.starts_with("open") {
            let _ = writeln!(names, "## open keyword reading: {} ({what})", record_key(k));
        }
    }
    // 3: the family's scripts and their ancestors.
    if family.is_some() {
        let mut all: BTreeSet<String> = BTreeSet::new();
        for s in &family_scripts {
            all.extend(chain(&lib, s));
        }
        *c.entry("3 the family: scripts and ancestors read".into())
            .or_default() += all.len() as u64;
        for t in &all {
            let _ = writeln!(names, "## family script {t}");
            calls_in(&lib, &lib.scripts[t], &natives, &mut c, "3 the family:");
        }
    }
    let mut out = String::from(
        "# keyword_census: kinds and counts only (spec/vm.md, Keywords). Sections: 0 keyword natives read from the scripts; 1 their call sites over every script;\n# 2 where base keywords come from in the records, and placed records by what the keyword reading settles; 3 the same call sites in the scripts the opening quest's family and its scenes attach.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
