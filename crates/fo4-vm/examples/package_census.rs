#![forbid(unsafe_code)]
//! Packages (PACK) and the scene actions that name them, the data the
//! package destination reading needs (`spec/vm.md`, Package destinations):
//! `package_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/package-census.txt`);
//! NAMES.txt names records and is never versioned. Nothing runs:
//!
//! 1. every winning PACK read by `fo4_plugin::pack`: refusals by code,
//!    subrecord tags by records holding them, PKCU (whether its count is
//!    the number of data inputs, what its FormID names, its third word);
//! 2. data inputs by type text and value tag with its size;
//! 3. PLDT by kind: what the word names, whether the third word is zero,
//!    the fourth word; PTDA by kind: what the word names;
//! 4. procedure names (PNAM after an ANAM `Procedure`) of packages without
//!    a template, and of the templates packages name;
//! 5. scene actions of kind 1: whether PNAM names a PACK, the package's
//!    template's first procedure, and how many of its Location inputs name
//!    a placed reference (the destination `pack_location_destination_v1`
//!    reads: exactly one);
//! 6. quest alias packages (ALPC) by the same destination count;
//! 7. the layout table: which fields are read and which are not
//!    established.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    pack::Package,
    quest::{AliasValue, Quest},
    scen::Scene,
    typed::Store,
    FormKey,
};
use fo4_vm::bind::{self, record_key};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

fn sig(store: &Store, k: Option<&FormKey>) -> String {
    match k {
        None => "no record (the word does not resolve)".into(),
        Some(k) => store
            .signature(k)
            .map(|s| String::from_utf8_lossy(&s).into_owned())
            .unwrap_or_else(|| "no winning record".into()),
    }
}

fn edid(store: &Store, k: &FormKey, b: &Budget) -> String {
    store
        .raw(k, &b.child())
        .ok()
        .flatten()
        .and_then(|r| bind::editor_id(&r))
        .unwrap_or_default()
}

/// The placed references a package's Location inputs name (PLDT words
/// resolving to a REFR or ACHR).
fn destinations(store: &Store, p: &Package) -> Vec<FormKey> {
    p.locations()
        .filter_map(|(_, l)| l.word.key.clone())
        .filter(|k| {
            matches!(
                store.signature(k).as_ref().map(|s| &s[..]),
                Some(b"REFR" | b"ACHR")
            )
        })
        .collect()
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("package_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let mut c = Counts::new();
    let mut names = String::new();
    let winners = |s: &[u8; 4]| -> Vec<FormKey> {
        store
            .winners()
            .filter(|w| !w.deleted && w.signature == *s)
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect()
    };
    // 1-4: every package.
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    for k in winners(b"PACK") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        bump(&mut c, "1 packages\twinning PACK");
        let p = match Package::read(&r, ctx, &bb) {
            Ok(p) => p,
            Err(e) => {
                bump(&mut c, format!("1 packages\trefused\t{}", e.code));
                let _ = writeln!(
                    names,
                    "# refused {} {}: {e}",
                    record_key(&k),
                    edid(&store, &k, &b)
                );
                continue;
            }
        };
        // The tag that ended the data inputs (the first after PKCU that is
        // neither ANAM nor a value tag).
        if let Some(at) = p.tags.iter().position(|t| t == b"PKCU") {
            let end = p.tags[at + 1..]
                .iter()
                .find(|t| *t != b"ANAM" && !fo4_plugin::pack::VALUE_TAGS.contains(t))
                .map(|t| String::from_utf8_lossy(t).into_owned())
                .unwrap_or_else(|| "the end of the record".into());
            bump(
                &mut c,
                format!(
                    "1 packages\tthe data inputs end at\t{end}\tcount equals inputs {}",
                    p.count == Some(p.inputs.len() as u32)
                ),
            );
        }
        let mut tags: Vec<[u8; 4]> = p.tags.clone();
        tags.sort();
        tags.dedup();
        for t in tags {
            bump(
                &mut c,
                format!(
                    "1 packages\trecords holding the tag\t{}",
                    String::from_utf8_lossy(&t)
                ),
            );
        }
        match (p.count, &p.template) {
            (Some(n), Some(t)) => {
                bump(
                    &mut c,
                    format!(
                        "1 packages\tPKCU count equals the number of data inputs\t{}",
                        n as usize == p.inputs.len()
                    ),
                );
                bump(
                    &mut c,
                    format!(
                        "1 packages\tPKCU FormID names\t{}",
                        if t.raw == 0 {
                            "none (zero)".into()
                        } else {
                            sig(&store, t.key.as_ref())
                        }
                    ),
                );
                bump(
                    &mut c,
                    format!("1 packages\tPKCU third word\t{}", p.version.unwrap_or(0)),
                );
            }
            _ => bump(&mut c, "1 packages\tno PKCU"),
        }
        for x in &p.inputs {
            let kind = String::from_utf8_lossy(&x.kind).into_owned();
            let v = match &x.value {
                Some((t, d)) => format!("{} of {} bytes", String::from_utf8_lossy(t), d.len()),
                None => "no value subrecord".into(),
            };
            bump(&mut c, format!("2 data inputs\t{kind}\t{v}"));
            if let Some(l) = &x.location {
                bump(
                    &mut c,
                    format!(
                        "3 PLDT\tkind {}\tword names {}",
                        l.kind,
                        if l.word.raw == 0 {
                            "nothing (zero)".into()
                        } else {
                            sig(&store, l.word.key.as_ref())
                        }
                    ),
                );
                bump(
                    &mut c,
                    format!("3 PLDT\tkind {}\tthird word zero {}", l.kind, l.third == 0),
                );
                bump(
                    &mut c,
                    format!(
                        "3 PLDT\tkind {}\tfourth word {}",
                        l.kind,
                        l.fourth
                            .map(|x| x.to_string())
                            .unwrap_or_else(|| "absent (12 bytes)".into())
                    ),
                );
            }
            if let Some(t) = &x.target {
                bump(
                    &mut c,
                    format!(
                        "3 PTDA\tkind {}\tword names {}",
                        t.kind,
                        if t.word.raw == 0 {
                            "nothing (zero)".into()
                        } else {
                            sig(&store, t.word.key.as_ref())
                        }
                    ),
                );
            }
        }
        let templated = p.template.as_ref().is_some_and(|t| t.raw != 0);
        for pr in &p.procedures {
            bump(
                &mut c,
                format!(
                    "4 procedures\t{}\t{}",
                    if templated {
                        "of a package with a template"
                    } else {
                        "of a package without a template"
                    },
                    String::from_utf8_lossy(pr)
                ),
            );
        }
        packages.insert(record_key(&k), p);
    }
    let first_procedure = |p: &Package| -> String {
        let t = p
            .template
            .as_ref()
            .filter(|t| t.raw != 0)
            .and_then(|t| t.key.as_ref())
            .and_then(|k| packages.get(&record_key(k)));
        match t {
            Some(t) => t
                .procedures
                .first()
                .map(|x| String::from_utf8_lossy(x).into_owned())
                .unwrap_or_else(|| "a template with no procedure".into()),
            None if !p.procedures.is_empty() => {
                format!("{} (its own)", String::from_utf8_lossy(&p.procedures[0]))
            }
            None => "no template and no procedure".into(),
        }
    };
    let count_label = |n: usize| match n {
        0 => "none",
        1 => "one (a destination under the rule)",
        _ => "several (no destination under the rule)",
    };
    // 5: scene actions of kind 1.
    let _ = writeln!(names, "# scene package actions: scene, action index, alias, package, first procedure of its template, destinations");
    for k in winners(b"SCEN") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(s) = Scene::read(&r, ctx, &bb) else {
            continue;
        };
        for (j, act) in s.actions.iter().enumerate().filter(|(_, x)| x.kind == 1) {
            bump(&mut c, "5 scene package actions\tactions of kind 1");
            let pk = act
                .field(b"PNAM")
                .and_then(|f| f.form.as_ref())
                .and_then(|f| f.key.clone());
            let Some(p) = pk.as_ref().and_then(|k| packages.get(&record_key(k))) else {
                bump(
                    &mut c,
                    "5 scene package actions\tPNAM names no package read",
                );
                continue;
            };
            let d = destinations(&store, p);
            let alias_in_scene = s.actors.iter().any(|x| x.alias == act.alias);
            bump(
                &mut c,
                format!(
                    "5 scene package actions\tLocation inputs naming a placed reference\t{}",
                    count_label(d.len())
                ),
            );
            bump(
                &mut c,
                format!(
                    "5 scene package actions\tfirst procedure of the template\t{}",
                    first_procedure(p)
                ),
            );
            bump(
                &mut c,
                format!(
                    "5 scene package actions\tthe action's alias is one of the scene's actors\t{alias_in_scene}"
                ),
            );
            let pk = pk.expect("a package key");
            let _ = writeln!(
                names,
                "{} {} action {j} alias {} package {} {} ({}) destinations [{}]",
                s.editor_id
                    .as_ref()
                    .map(|e| String::from_utf8_lossy(e).into_owned())
                    .unwrap_or_default(),
                record_key(&k),
                act.alias,
                record_key(&pk),
                edid(&store, &pk, &b),
                first_procedure(p),
                d.iter()
                    .map(|x| format!("{} {}", record_key(x), edid(&store, x, &b)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    // 6: quest alias packages.
    let _ = writeln!(
        names,
        "# quest alias packages: quest, alias, package, first procedure, destinations"
    );
    for k in winners(b"QUST") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(q) = Quest::read(&r, ctx, &bb) else {
            continue;
        };
        for al in &q.aliases {
            let mut any = false;
            for v in &al.values {
                let AliasValue::Form(t, f) = v else { continue };
                if t != b"ALPC" {
                    continue;
                }
                any = true;
                bump(&mut c, "6 alias packages\tALPC values");
                let Some(p) = f.key.as_ref().and_then(|k| packages.get(&record_key(k))) else {
                    bump(&mut c, "6 alias packages\tALPC names no package read");
                    continue;
                };
                let d = destinations(&store, p);
                bump(
                    &mut c,
                    format!(
                        "6 alias packages\tLocation inputs naming a placed reference\t{}",
                        count_label(d.len())
                    ),
                );
                let pk = f.key.clone().expect("a package key");
                let _ = writeln!(
                    names,
                    "{} alias {} {} package {} {} ({}) conditions {} destinations [{}]",
                    q.editor_id
                        .as_ref()
                        .map(|e| String::from_utf8_lossy(e).into_owned())
                        .unwrap_or_default(),
                    al.id,
                    String::from_utf8_lossy(&al.name),
                    record_key(&pk),
                    edid(&store, &pk, &b),
                    first_procedure(p),
                    p.conditions,
                    d.iter()
                        .map(|x| format!("{} {}", record_key(x), edid(&store, x, &b)))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if any {
                bump(&mut c, "6 alias packages\taliases with packages");
            }
        }
    }
    let mut out = String::from(
        "# package_census: kinds and counts only (spec/vm.md, Package destinations). Sections: 1 packages (PACK) and PKCU; 2 data inputs by type text and value;\n# 3 PLDT and PTDA by kind and what their word names; 4 procedure names; 5 scene actions of kind 1 and their packages' destinations; 6 quest alias packages (ALPC); 7 the layout table.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    out.push_str(LAYOUT);
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

/// The layout table (section 7): what `fo4_plugin::pack` reads, and what
/// is not established.
const LAYOUT: &str = "\
7 layout\tEDID\ttext\tread
7 layout\tPKDT\t12 bytes\tkept as stored; fields not established
7 layout\tPSDT\t12 bytes\tnot read; not established
7 layout\tCTDA, CIS1, CIS2\tconditions\tcounted (fo4_plugin::condition reads them elsewhere)
7 layout\tQNAM\t4 bytes\tnot read here
7 layout\tPKCU +0\tu32\tread: the count of data inputs (section 1 checks it)
7 layout\tPKCU +4\tFormID\tread: a PACK or zero (section 1)
7 layout\tPKCU +8\tu32\tkept as stored; not established
7 layout\tANAM (in the inputs)\ttext\tread: the input's type name
7 layout\tCNAM, PLDT, PTDA, PDTO (after an input's ANAM)\tvalue\tkept as stored (section 2 sizes)
7 layout\tPLDT +0\tu32\tread: a kind (section 3)
7 layout\tPLDT +4\tword\tread: a FormID for the kinds section 3 shows naming records; other kinds not established
7 layout\tPLDT +8\tu32\tkept as stored; not established
7 layout\tPLDT +12\tu32\tkept as stored; not established (absent in the PLDT of 12 bytes)
7 layout\tPTDA +0\tu32\tread: a kind (section 3)
7 layout\tPTDA +4\tword\tread: a FormID for the kinds section 3 shows naming records; other kinds not established
7 layout\tPTDA +8\tu32\tkept as stored; not established
7 layout\tUNAM, XNAM, BNAM, PRCB, CITC, FNAM, PKC2, POBA, POEA, POCA, INAM, PDTO after POBA, POEA or POCA, and the rest\tvarious\tnot read; not established
7 layout\tPNAM after an ANAM Procedure\ttext\tread: a procedure name (section 4)
";

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
